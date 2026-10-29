//! The facade's engine: Image::ExifTool, run by the port's interpreter.
//!
//! Every call goes through a small Perl package (`ExifToolRs::API`, below)
//! that uses ExifTool's documented API as a Perl program would (`new`,
//! `Options`, `ImageInfo`, `GetTagList`, `GetGroup`, `GetValue`,
//! `SetNewValue`, `SetNewValuesFromFile`, `WriteInfo`) and hands back plain
//! arrays. Values are never reformatted on the Rust side: the printed value
//! is the one ExifTool's application prints, and the raw value is
//! ExifTool's ValueConv value, typed only where its text is unchanged.
//!
//! An [`Engine`] (one per `ExifTool` object) keeps one interpreter, and one
//! Image::ExifTool object on it, from one call to the next, as a Perl
//! program reuses its `Image::ExifTool->new` object for file after file (and
//! as the exiftool application does for the files of one command). ExifTool
//! keeps state in its tables from one file to the next, in Perl too (in one
//! process, XMP.xml gets an AutoFocus composite that a module loaded for an
//! earlier file defines; after writing Kodak.jpg's Make, the output reads
//! "Unrecognized MakerNoteUnknown" instead of "Unrecognized MakerNotes"), so
//! a call may depend on the ones before it, exactly as it would in Perl.
//! With `Options::fresh_interpreter`, each call runs on an interpreter of its
//! own instead, as each run of the exiftool application is a process of its
//! own, and no call depends on another.
//!
//! The interpreter is not `Send` (it holds `Rc`s, some shared with caches
//! local to its thread), so a reused one lives on a thread of its own, the
//! engine's worker, which runs the calls one at a time: `ExifTool` stays
//! `Send + Sync`, and calls from several threads take turns.

use std::cell::RefCell;
use std::rc::Rc;

use crate::interp::{Binding, Flow, Interp, Scope, Want};
use crate::perl::Value as PV;

use super::error::{Error, Result};
use super::tag::{Tag, TagGroup, TagId, MAIN_DOCUMENT};
use super::value::{format_g15, Value};

/// The Perl side of the facade.
const API_PL: &str = r#"
package ExifToolRs::API;
require Image::ExifTool;

# A value with its hashes turned into ordered pairs, { "\0" => [k, v, ...] },
# in ExifTool's order (OrderedKeys), and binary data as SCALAR references.
sub Ordered {
    my $obj = shift;
    if (ref $obj eq 'HASH') {
        return { "\0" => [ map { ($_, Ordered($$obj{$_})) } Image::ExifTool::OrderedKeys($obj) ] };
    } elsif (ref $obj eq 'ARRAY') {
        return [ map { Ordered($_) } @$obj ];
    }
    return $obj;
}

# The exiftool application's ConvertBinary, on a copy: the binary data of a
# value replaced by its "(Binary data N bytes, ...)" text.
sub ConvBin {
    my $obj = shift;
    if (ref $obj eq 'HASH') {
        my %copy;
        $copy{$_} = ConvBin($$obj{$_}) foreach keys %$obj;
        return \%copy;
    } elsif (ref $obj eq 'ARRAY') {
        return [ map { ConvBin($_) } @$obj ];
    } elsif (ref $obj eq 'SCALAR') {
        return "($$obj, use -b option to extract)" if $$obj =~ /^Binary data \d+ bytes$/;
        return '(Binary data ' . length($$obj) . ' bytes, use -b option to extract)';
    }
    return $obj;
}

# The printed form of an ImageInfo value, as `exiftool -s` prints it.
sub PrintText {
    my ($et, $val, $struct) = @_;
    return $val unless ref $val;
    $val = ConvBin($val);
    if ($struct and ref $val) {
        require 'Image/ExifTool/XMPStruct.pl';
        $val = Image::ExifTool::XMP::SerializeStruct($et, $val);
    } elsif (ref $val eq 'ARRAY') {
        $val = join $$et{OPTIONS}{ListSep}, @$val;
    } elsif (ref $val) {
        require 'Image/ExifTool/XMPStruct.pl';
        $val = Image::ExifTool::XMP::SerializeStruct($et, $val);
    }
    return $val;
}

# The ExifTool object of a call: the one of the calls before on this
# interpreter, its options and new values cleared, as a program reusing its
# Image::ExifTool object for another file does.
our $tool;
sub Tool {
    my $opts = shift;
    if ($tool) {
        $tool->ClearOptions();
        $tool->SetNewValue();
    } else {
        $tool = Image::ExifTool->new;
    }
    my $et = $tool;
    $et->Options(@$opts) if $opts and @$opts;
    return $et;
}

# Read a file (a name, or a SCALAR reference to its data).
# Returns one record per tag, in GetTagList order: [ key, name, group 0-3,
# format (family 6), tag ID, description, priority, write group, ValueConv
# value, printed value, writable ].
sub Read {
    my ($file, $opts, $tags) = @_;
    my $et = Tool($opts);
    my $info = $et->ImageInfo($file, @$tags);
    my $struct = $et->Options('Struct');
    my @recs;
    foreach my $key ($et->GetTagList($info)) {
        my $prt = $$info{$key};
        next unless defined $prt;
        my $tagInfo = $$et{TAG_INFO}{$key} || { };
        my @g = $et->GetGroup($key);
        my $raw = $et->GetValue($key, 'ValueConv');
        my $table = $$tagInfo{Table};
        my $wg = $$tagInfo{WriteGroup};
        $wg = $$table{WRITE_GROUP} if not defined $wg and ref $table eq 'HASH';
        my $writable = $$tagInfo{Writable};
        $writable = $$table{WRITABLE} if not defined $writable and ref $table eq 'HASH';
        push @recs, [ $key, Image::ExifTool::GetTagName($key), @g[0..3], $g[6],
            $$tagInfo{TagID}, $et->GetDescription($key), $$tagInfo{Priority}, $wg,
            Ordered($raw), PrintText($et, $prt, $struct), $writable ? 1 : 0 ];
    }
    return \@recs;
}

# Queue the requests of `ops` on a new ExifTool: [ 'set', TAG, VALUE ] as
# the exiftool application's -TAG=VALUE does, or [ 'copy', FILE, [TAGS] ] as
# its -tagsFromFile. Returns the tool, the warnings, and the number of tags
# the last copy set (as the application counts them).
sub Queue {
    my ($ops, $opts) = @_;
    my $et = Tool($opts);
    my (@warn, $numSet);
    foreach my $op (@$ops) {
        if ($$op[0] eq 'copy') {
            my $info = $et->SetNewValuesFromFile($$op[1], @{$$op[2]});
            my @msgs = grep /^(Error|Warning)\b/, keys %$info;
            $numSet = scalar(keys %$info) - scalar(@msgs);
            if (defined $$info{Error}) {
                push @warn, $$info{Error};
            } else {
                push @warn, map { $$info{$_} } sort @msgs;
            }
        } else {
            my %opts = (Shift => 0);
            $opts{Protected} = 1 unless $$op[1] =~ /[?*]/;
            my ($n, $w) = $et->SetNewValue($$op[1], $$op[2], %opts);
            push @warn, $w if $w;
        }
    }
    return ($et, \@warn, $numSet);
}

# The number of tags a copy from FILE would set, and its error if any.
sub CountCopy {
    my ($file, $tags, $opts) = @_;
    my $et = Tool($opts);
    my $info = $et->SetNewValuesFromFile($file, @$tags);
    my @msgs = grep /^(Error|Warning)\b/, keys %$info;
    return [ scalar(keys %$info) - scalar(@msgs), $$info{Error} ];
}

# Write SRC with the queued requests into memory, for DST.
# Returns [ WriteInfo's result, \$output, [ warnings ], error ].
sub Write {
    my ($src, $dst, $ops, $opts) = @_;
    # the exiftool application's check of an output file name (-o)
    my $type = Image::ExifTool::GetFileType($dst);
    if ($type) {
        my $canWrite = Image::ExifTool::CanWrite($dst);
        unless ($canWrite) {
            if (defined $canWrite and $canWrite eq '') {
                $type = Image::ExifTool::GetFileExtension($dst);
                $type = uc($dst) unless defined $type;
            }
            return [ 0, undef, [ ], "Can't write $type files" ];
        }
    }
    my ($et, $warn) = Queue($ops, $opts);
    my $out = '';
    my $rtn = $et->WriteInfo($src, \$out);
    my $info = $et->GetInfo('Warning', 'Error');
    my $err;
    foreach my $key ($et->GetTagList($info)) {
        if ($key =~ /^Error\b/) {
            $err = $$info{$key} unless defined $err;
        } else {
            push @$warn, $$info{$key};
        }
    }
    return [ $rtn, \$out, $warn, $err ];
}

# For a write ExifTool refused: the FileType of SRC (as ImageInfo finds
# it), whether ExifTool can write that type, and the type of DST's name.
sub WriteTypes {
    my ($src, $dst) = @_;
    my $et = Tool();
    my $info = $et->ImageInfo($src, 'FileType');
    my $type = $$info{FileType};
    my $canWrite = (defined $type and Image::ExifTool::CanWrite($type)) ? 1 : 0;
    return [ $type, $canWrite, scalar Image::ExifTool::GetFileType($dst) ];
}

# The FileType and FileTypeExtension of a file (a name or a SCALAR
# reference), from its magic number and extension as ExtractInfo finds
# them, without reading metadata.
sub FileType {
    my $file = shift;
    my $et = Tool([ FastScan => 3 ]);
    my $info = $et->ImageInfo($file, 'FileType', 'FileTypeExtension');
    return [ $$info{FileType}, $$info{FileTypeExtension} ];
}

sub TypeOfExtension {
    my $ext = shift;
    my $type = Image::ExifTool::GetFileType("x.$ext");
    return $type;
}

sub Support {
    my $type = shift;
    return 2 if Image::ExifTool::CanCreate($type);
    return 1 if Image::ExifTool::CanWrite($type);
    return 0;
}

sub WritableTags {
    return [ Image::ExifTool::GetWritableTags() ];
}

1;
"#;

fn new_engine(mwg: bool) -> Interp {
    let mut it = crate::runtime::new_interp();
    it.strict = false;
    let scope = Scope::new(None);
    if let Err(e) = it.eval_string(API_PL.as_bytes(), "main", &scope, Want::Void) {
        panic!("exiftool-port: loading the API: {}", e.to_text());
    }
    if mwg {
        load_mwg(&mut it);
    }
    it
}

fn load_mwg(it: &mut Interp) {
    let scope = Scope::new(None);
    let code = b"require Image::ExifTool::MWG; Image::ExifTool::MWG::Load(); 1;";
    if let Err(e) = it.eval_string(code, "main", &scope, Want::Void) {
        panic!("exiftool-port: loading MWG: {}", e.to_text());
    }
}

/// Run `f` on a new interpreter, with MWG loaded if `mwg`.
pub(crate) fn with_engine<R>(mwg: bool, f: impl FnOnce(&mut Interp) -> R) -> R {
    let mut it = new_engine(mwg);
    run(&mut it, f)
}

/// The interpreter of an `ExifTool` object, kept on its worker thread
/// (started on first use, ended when the engine is dropped).
#[derive(Default)]
pub(crate) struct Engine {
    worker: std::sync::Mutex<Option<Worker>>,
}

type Job = Box<dyn FnOnce(&mut WorkerState) + Send>;

struct Worker {
    jobs: std::sync::mpsc::Sender<Job>,
}

/// What a worker thread keeps: its interpreter, made on first use, and
/// whether MWG is loaded in it.
#[derive(Default)]
struct WorkerState {
    it: Option<(Interp, bool)>,
}

impl WorkerState {
    /// The interpreter, with MWG loaded if `mwg`. MWG cannot be unloaded:
    /// for a call without it, an interpreter that has it is replaced.
    fn interp(&mut self, mwg: bool) -> &mut Interp {
        match &mut self.it {
            Some((_, loaded)) if *loaded == mwg => {}
            Some((it, loaded)) if mwg => {
                load_mwg(it);
                *loaded = true;
            }
            _ => self.it = Some((new_engine(mwg), mwg)),
        }
        &mut self.it.as_mut().expect("interpreter").0
    }
}

/// A worker's stack: the size of a main thread's (the interpreter recurses
/// deeply).
const WORKER_STACK: usize = 8 << 20;

impl Engine {
    /// Run `f` on this engine's interpreter, with MWG loaded if `mwg`, or on
    /// a new interpreter if `fresh`. A panic in `f` is the caller's, and the
    /// interpreter it leaves is dropped.
    pub(crate) fn run<R: Send + 'static>(&self, fresh: bool, mwg: bool, f: impl FnOnce(&mut Interp) -> R + Send + 'static) -> R {
        if fresh {
            return with_engine(mwg, f);
        }
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let job: Job = Box::new(move |state: &mut WorkerState| {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(state.interp(mwg), f)));
            if r.is_err() {
                state.it = None;
            }
            let _ = tx.send(r);
        });
        // (held until the result is back: calls from several threads take
        // turns)
        let mut worker = self.worker.lock().unwrap_or_else(|e| e.into_inner());
        let mut job = Some(job);
        while let Some(j) = job.take() {
            let w = worker.get_or_insert_with(Worker::spawn);
            if let Err(std::sync::mpsc::SendError(j)) = w.jobs.send(j) {
                // the worker has ended: start another one
                *worker = None;
                job = Some(j);
            }
        }
        match rx.recv() {
            Ok(Ok(r)) => r,
            Ok(Err(panic)) => std::panic::resume_unwind(panic),
            Err(_) => {
                *worker = None;
                panic!("exiftool-port: the interpreter's thread has ended");
            }
        }
    }
}

impl Worker {
    fn spawn() -> Worker {
        let (jobs, rx) = std::sync::mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("exiftool-port".into())
            .stack_size(WORKER_STACK)
            .spawn(move || {
                let mut state = WorkerState::default();
                // until the engine, which holds the sender, is dropped
                while let Ok(job) = rx.recv() {
                    job(&mut state);
                }
            })
            .expect("exiftool-port: starting the interpreter's thread");
        Worker { jobs }
    }
}

fn run<R>(it: &mut Interp, f: impl FnOnce(&mut Interp) -> R) -> R {
    // The interpreter's %ENV is the process's at its creation; its
    // localtime copies $ENV{TZ} back to the process. Keep it the
    // process's own, so that a call never changes the environment.
    if let Binding::Hash(h) = it.global(b'%', "main::ENV") {
        match std::env::var("TZ") {
            Ok(tz) => h.borrow_mut().insert(b"TZ", PV::str(&tz)),
            Err(_) => {
                h.borrow_mut().remove(b"TZ");
            }
        }
    }
    let r = f(it);
    // (the library prints nothing worth keeping; don't let it pile up)
    it.stdout.clear();
    it.stderr.clear();
    r
}

/// Call `ExifToolRs::API::NAME`.
pub(crate) fn call(it: &mut Interp, name: &str, args: Vec<PV>, want: Want) -> Result<Vec<PV>> {
    match it.call_named(&format!("ExifToolRs::API::{name}"), args, want) {
        Ok(v) => Ok(v),
        Err(Flow::Die(e)) => Err(Error::InvalidData(e.to_text().trim_end().to_string())),
        Err(Flow::Exit(code)) => Err(Error::InvalidData(format!("ExifTool exited ({code})"))),
        Err(_) => Err(Error::InvalidData("ExifTool: unexpected control flow".into())),
    }
}

// ---- conversions between Rust and Perl ----

pub(crate) fn pv_str(s: &str) -> PV {
    PV::bytes(s.as_bytes())
}

pub(crate) fn pv_array(items: Vec<PV>) -> PV {
    PV::ArrayRef(Rc::new(RefCell::new(items)))
}

/// A path as Perl gets a file name: its bytes.
pub(crate) fn pv_path(p: &std::path::Path) -> PV {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        PV::bytes(p.as_os_str().as_bytes())
    }
    #[cfg(not(unix))]
    {
        PV::bytes(p.to_string_lossy().as_bytes())
    }
}

/// The text of a Perl string: a character string by its characters, a
/// byte string (ExifTool's values are UTF-8 bytes) as UTF-8.
pub(crate) fn text(v: &PV) -> String {
    match v {
        PV::Undef => String::new(),
        PV::UStr(s) => String::from_utf8_lossy(s).into_owned(),
        other => String::from_utf8_lossy(&other.to_bytes()).into_owned(),
    }
}

pub(crate) fn opt_text(v: &PV) -> Option<String> {
    (!v.is_undef()).then(|| text(v))
}

pub(crate) fn array(v: &PV) -> Vec<PV> {
    match v {
        PV::ArrayRef(a) => a.borrow().clone(),
        _ => Vec::new(),
    }
}

/// A raw (ValueConv) value as a [`Value`], with the TIFF format ExifTool
/// read it with (family 6 group, `SaveFormat`) when known.
///
/// Numbers are typed where their text form is exactly ExifTool's: an
/// integer format gives `U8`..`I32` (a list of them for a space-separated
/// count), a float or rational one `F64`; anything else stays a string.
fn raw_value(v: &PV, format: &str) -> Value {
    match v {
        // (a SCALAR reference to an element, `\$$dict{Data}`, is binary
        //  data too: PDF's EmbeddedImage)
        PV::ScalarRef(_) | PV::LvRef(_) => {
            let inner = v.deref().to_bytes();
            if is_binary_placeholder(&inner) {
                Value::String(format!("({}, use -b option to extract)", String::from_utf8_lossy(&inner)))
            } else {
                Value::Binary(inner.to_vec())
            }
        }
        PV::ArrayRef(a) => Value::List(a.borrow().iter().map(|x| raw_value(x, "")).collect()),
        PV::HashRef(h) => {
            let h = h.borrow();
            let pairs = h.get(b"\0").map(array).unwrap_or_default();
            Value::Struct(pairs.chunks(2).map(|kv| (text(&kv[0]), kv.get(1).map(|x| raw_value(x, "")).unwrap_or(Value::String(String::new())))).collect())
        }
        _ => typed(v, format),
    }
}

fn is_binary_placeholder(b: &[u8]) -> bool {
    let Some(rest) = b.strip_prefix(b"Binary data ") else { return false };
    let Some(n) = rest.strip_suffix(b" bytes") else { return false };
    !n.is_empty() && n.iter().all(u8::is_ascii_digit)
}

fn typed(v: &PV, format: &str) -> Value {
    let s = text(v);
    let one = |t: &str| -> Option<Value> {
        let int = |t: &str| t.parse::<i64>().ok().filter(|n| n.to_string() == t);
        let val = match format {
            "int8u" => Value::U8(u8::try_from(int(t)?).ok()?),
            "int16u" => Value::U16(u16::try_from(int(t)?).ok()?),
            "int32u" | "ifd" => Value::U32(u32::try_from(int(t)?).ok()?),
            "int8s" | "int16s" => Value::I16(i16::try_from(int(t)?).ok()?),
            "int32s" => Value::I32(i32::try_from(int(t)?).ok()?),
            "rational64u" | "rational64s" | "rational32u" | "rational32s" | "float" | "double" => {
                let f: f64 = t.parse().ok()?;
                if !f.is_finite() || format_g15(f) != t {
                    return None;
                }
                Value::F64(f)
            }
            _ => return None,
        };
        Some(val)
    };
    if !format.is_empty() {
        if let Some(val) = one(&s) {
            return val;
        }
        // a count of several numbers, as ExifTool joins them
        if s.contains(' ') {
            let items: Option<Vec<Value>> = s.split(' ').map(one).collect();
            if let Some(items) = items.filter(|i| i.len() > 1) {
                return Value::List(items);
            }
        }
        return Value::String(s);
    }
    // no format: a number Perl holds as one
    match v {
        PV::Int(n) => {
            if let Ok(u) = u32::try_from(*n) {
                Value::U32(u)
            } else if let Ok(i) = i32::try_from(*n) {
                Value::I32(i)
            } else {
                Value::String(s)
            }
        }
        PV::UInt(n) => u32::try_from(*n).map(Value::U32).unwrap_or(Value::String(s)),
        PV::Num(f) if f.is_finite() && format_g15(*f) == s => Value::F64(*f),
        _ => Value::String(s),
    }
}

/// A read record (see `Read` above): the key ExifTool gave the tag, the
/// facade's [`Tag`], its write group and whether ExifTool can write it.
pub(crate) fn record_to_tag(rec: &PV) -> (String, Tag, Option<String>, bool) {
    let f = array(rec);
    let get = |i: usize| f.get(i).cloned().unwrap_or_default();
    let key = text(&get(0));
    let format = text(&get(6));
    let id = match get(7) {
        PV::Int(n) => u16::try_from(n).map(TagId::Numeric).unwrap_or_else(|_| TagId::Text(n.to_string())),
        PV::Undef => TagId::Text(text(&get(1))),
        other => {
            let t = text(&other);
            match t.parse::<u16>() {
                Ok(n) if n.to_string() == t => TagId::Numeric(n),
                _ => TagId::Text(t),
            }
        }
    };
    let family3 = text(&get(5));
    let priority = match get(9) {
        PV::Undef => 1,
        p => p.to_i64() as i32,
    };
    let tag = Tag {
        id,
        name: text(&get(1)),
        description: text(&get(8)),
        group: TagGroup {
            family0: text(&get(2)),
            family1: text(&get(3)),
            family2: text(&get(4)),
            family3: if family3.is_empty() { MAIN_DOCUMENT.to_string() } else { family3 },
        },
        raw_value: raw_value(&get(11), &format),
        print_value: text(&get(12)),
        priority,
    };
    (key, tag, opt_text(&get(10)), get(13).truthy())
}

// ---- file types ----

/// ExifTool's `FileType` and `FileTypeExtension` for some data.
pub(crate) fn type_of_data(data: &[u8]) -> Option<(String, Option<String>)> {
    type_of(PV::binary(data))
}

/// ExifTool's `FileType` and `FileTypeExtension` for a file.
pub(crate) fn type_of_file(path: &std::path::Path) -> Option<(String, Option<String>)> {
    type_of(pv_path(path))
}

fn type_of(source: PV) -> Option<(String, Option<String>)> {
    with_engine(false, |it| {
        let r = call(it, "FileType", vec![source], Want::Scalar).ok()?;
        let f = array(r.first()?);
        Some((opt_text(f.first()?)?, f.get(1).and_then(opt_text)))
    })
}

pub(crate) fn type_of_extension(ext: &str) -> Option<String> {
    with_engine(false, |it| {
        let r = call(it, "TypeOfExtension", vec![pv_str(ext)], Want::Scalar).ok()?;
        r.first().and_then(opt_text)
    })
}

pub(crate) fn support(code: &str) -> super::file_type::Support {
    use super::file_type::Support;
    let n = with_engine(false, |it| call(it, "Support", vec![pv_str(code)], Want::Scalar).ok().and_then(|r| r.first().map(PV::to_i64)).unwrap_or(0));
    match n {
        2 => Support::ReadWriteCreate,
        1 => Support::ReadWrite,
        _ => Support::Read,
    }
}

pub(crate) fn writable_tags() -> Vec<String> {
    with_engine(false, |it| call(it, "WritableTags", vec![], Want::Scalar).ok().and_then(|r| r.first().map(array)).unwrap_or_default().iter().map(text).collect())
}
