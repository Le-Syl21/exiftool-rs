//! Filehandles: what ExifTool's test harness and config files read and
//! write with (`open`, `print FH`, `<FH>`, `close`, `unlink`).
//!
//! ExifTool reads image files through its own File::RandomAccess object,
//! which is ported natively; these are only the plain perl handles.

use std::fs::File;
use std::io::{BufRead, BufReader, Cursor, Seek, SeekFrom, Write};

use super::*;

pub enum Input {
    File(BufReader<File>),
    Memory(Cursor<Vec<u8>>),
    /// `+<`: read and write the same file (writes go through `Output::Rw`).
    Rw(RwFile),
}

impl Input {
    fn reader(&mut self) -> &mut dyn BufRead {
        match self {
            Input::File(f) => f,
            Input::Memory(m) => m,
            Input::Rw(f) => f,
        }
    }
}

/// A file opened for update: reads are buffered, and the buffer is dropped
/// (the file position put back) before a write or a seek.
pub struct RwFile {
    f: File,
    buf: Vec<u8>,
    at: usize,
}

impl RwFile {
    fn sync(&mut self) {
        let unread = (self.buf.len() - self.at) as i64;
        if unread > 0 {
            let _ = self.f.seek(SeekFrom::Current(-unread));
        }
        self.buf.clear();
        self.at = 0;
    }
}

impl std::io::Read for RwFile {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let avail = self.fill_buf()?;
        let n = avail.len().min(out.len());
        out[..n].copy_from_slice(&avail[..n]);
        self.consume(n);
        Ok(n)
    }
}

impl BufRead for RwFile {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        if self.at >= self.buf.len() {
            self.buf.resize(8192, 0);
            let n = std::io::Read::read(&mut self.f, &mut self.buf)?;
            self.buf.truncate(n);
            self.at = 0;
        }
        Ok(&self.buf[self.at..])
    }

    fn consume(&mut self, n: usize) {
        self.at = (self.at + n).min(self.buf.len());
    }
}

impl Seek for RwFile {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        self.sync();
        self.f.seek(to)
    }
}

impl Write for RwFile {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.sync();
        self.f.write(data)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.f.flush()
    }
}

pub enum Output {
    Stdout,
    Stderr,
    File(std::io::BufWriter<File>),
    /// `open(FH, '>', \$buf)`
    Scalar(Cell),
    /// The update side of `Input::Rw`.
    Rw,
}

#[derive(Default)]
pub struct FileHandle {
    pub input: Option<Input>,
    pub output: Option<Output>,
    pub eof: bool,
    pub lines: i64,
    /// The file opened, for stat and file tests on the handle.
    pub path: Option<String>,
    /// The class it is blessed into (`bless $fh, 'IO::File'`).
    pub class: Option<Rc<str>>,
}

impl std::fmt::Debug for FileHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GLOB")
    }
}

pub type HandleRc = Rc<RefCell<FileHandle>>;

impl Interp {
    fn std_handle(&mut self, name: &str) -> HandleRc {
        let key: Rc<str> = Rc::from(format!("main::{name}"));
        if let Some(h) = self.handles.get(&key) {
            return h.clone();
        }
        let fh = FileHandle {
            output: match name {
                "STDOUT" => Some(Output::Stdout),
                "STDERR" => Some(Output::Stderr),
                _ => None,
            },
            ..Default::default()
        };
        let h = Rc::new(RefCell::new(fh));
        self.handles.insert(key, h.clone());
        h
    }

    /// The handle a value names: a handle reference, `"*NAME"`, or a name.
    pub(super) fn handle_of(&mut self, v: &Value, create: bool) -> Option<HandleRc> {
        match v {
            Value::Handle(h) => Some(h.clone()),
            Value::ScalarRef(c) => {
                let inner = c.borrow().clone();
                self.handle_of(&inner, create)
            }
            Value::Str(s) => {
                let name = String::from_utf8_lossy(s).into_owned();
                let name = name.trim_start_matches('*').to_string();
                let name = name.strip_prefix("main::").unwrap_or(&name).to_string();
                if matches!(name.as_str(), "STDOUT" | "STDERR" | "STDIN") {
                    return Some(self.std_handle(&name));
                }
                let key: Rc<str> = if name.contains("::") { Rc::from(name.as_str()) } else { Rc::from(format!("{}::{name}", self.package)) };
                if let Some(h) = self.handles.get(&key) {
                    return Some(h.clone());
                }
                if create {
                    let h: HandleRc = Rc::new(RefCell::new(FileHandle::default()));
                    self.handles.insert(key, h.clone());
                    return Some(h);
                }
                None
            }
            _ => None,
        }
    }

    /// Everything behind a handle, for natives that read a whole file
    /// (Archive::Zip): its file, or its in-memory data.
    pub fn handle_contents(&mut self, v: &Value) -> Option<Vec<u8>> {
        let h = self.handle_of(v, false)?;
        let fh = h.borrow();
        match &fh.input {
            Some(Input::Memory(m)) => Some(m.get_ref().clone()),
            _ => std::fs::read(fh.path.as_ref()?).ok(),
        }
    }

    /// Write to a handle (print's work after formatting).
    pub(super) fn write_handle(&mut self, h: &HandleRc, data: &[u8]) -> bool {
        let mut fh = h.borrow_mut();
        match &mut fh.output {
            Some(Output::Stdout) => {
                if self.passthrough {
                    let mut o = std::io::stdout();
                    let _ = o.write_all(data);
                    let _ = o.flush();
                } else {
                    self.stdout.extend_from_slice(data);
                }
                true
            }
            Some(Output::Stderr) => {
                if self.passthrough {
                    let _ = std::io::stderr().write_all(data);
                } else {
                    self.stderr.extend_from_slice(data);
                }
                true
            }
            Some(Output::File(f)) => f.write_all(data).is_ok(),
            Some(Output::Scalar(c)) => {
                let mut cur = c.borrow().to_bytes().to_vec();
                cur.extend_from_slice(data);
                *c.borrow_mut() = Value::bytes(cur);
                true
            }
            Some(Output::Rw) => match &mut fh.input {
                Some(Input::Rw(f)) => f.write_all(data).is_ok(),
                _ => false,
            },
            None => false,
        }
    }

    /// `<FH>`: one record in scalar context, all of them in list context.
    pub(super) fn readline(&mut self, h: &Expr, scope: &Rc<Scope>, want: Want) -> ER<Vec<Value>> {
        let hv = self.scalar(h, scope)?;
        let Some(h) = self.handle_of(&hv, false) else { return Ok(if want == Want::List { vec![] } else { vec![Value::Undef] }) };
        let sep = self.get_scalar("main::/");
        let mut out = Vec::new();
        loop {
            let rec = {
                let mut fh = h.borrow_mut();
                let Some(input) = fh.input.as_mut() else { break };
                let r = input.reader();
                let mut buf = Vec::new();
                if sep.is_undef() {
                    let _ = r.read_to_end(&mut buf);
                } else {
                    let s = sep.to_bytes();
                    let last = *s.last().unwrap_or(&b'\n');
                    loop {
                        let n = r.read_until(last, &mut buf).unwrap_or(0);
                        if n == 0 || buf.ends_with(&s) {
                            break;
                        }
                    }
                }
                if buf.is_empty() {
                    fh.eof = true;
                    None
                } else {
                    fh.lines += 1;
                    Some(buf)
                }
            };
            match rec {
                Some(b) => {
                    let n = h.borrow().lines;
                    self.set_scalar("main::.", Value::Int(n));
                    out.push(Value::bytes(b));
                    if want != Want::List {
                        break;
                    }
                }
                None => break,
            }
        }
        if want != Want::List {
            return Ok(vec![out.pop().unwrap_or_default()]);
        }
        Ok(out)
    }
}

/// The I/O builtins, or `None` when `name` is not one.
pub(super) fn call(it: &mut Interp, name: &str, args: &[Expr], scope: &Rc<Scope>, _want: Want) -> Option<ER<Vec<Value>>> {
    Some(match name {
        "open" => open(it, args, scope),
        "close" => {
            let h = match args.first() {
                Some(a) => match it.scalar(a, scope) {
                    Ok(v) => it.handle_of(&v, false),
                    Err(e) => return Some(Err(e)),
                },
                None => None,
            };
            Ok(vec![Value::from(match h {
                Some(h) => {
                    let mut fh = h.borrow_mut();
                    let was_open = fh.input.is_some() || fh.output.is_some();
                    if let Some(Output::File(f)) = &mut fh.output {
                        let _ = f.flush();
                    }
                    if !matches!(fh.output, Some(Output::Stdout | Output::Stderr)) {
                        fh.output = None;
                    }
                    fh.input = None;
                    fh.lines = 0;
                    was_open
                }
                None => false,
            })])
        }
        "binmode" => Ok(vec![Value::Int(1)]),
        "eof" => {
            let h = match args.first() {
                Some(a) => match it.scalar(a, scope) {
                    Ok(v) => it.handle_of(&v, false),
                    Err(e) => return Some(Err(e)),
                },
                None => None,
            };
            let at_end = match h {
                Some(h) => {
                    let mut fh = h.borrow_mut();
                    match fh.input.as_mut() {
                        Some(i) => i.reader().fill_buf().map(|b| b.is_empty()).unwrap_or(true),
                        None => true,
                    }
                }
                None => true,
            };
            Ok(vec![Value::from(at_end)])
        }
        "read" | "sysread" => read(it, args, scope),
        "seek" | "sysseek" => {
            let r = (|| -> ER<bool> {
                let hv = it.scalar(&args[0], scope)?;
                let pos = it.scalar(&args[1], scope)?.to_i64();
                let whence = it.scalar(&args[2], scope)?.to_i64();
                let Some(h) = it.handle_of(&hv, false) else { return Ok(false) };
                let mut fh = h.borrow_mut();
                let fh = &mut *fh;
                let target = match whence {
                    0 => SeekFrom::Start(pos.max(0) as u64),
                    1 => SeekFrom::Current(pos),
                    _ => SeekFrom::End(pos),
                };
                let ok = match (fh.input.as_mut(), fh.output.as_mut()) {
                    (Some(Input::File(f)), _) => f.seek(target).is_ok(),
                    (Some(Input::Memory(m)), _) => m.seek(target).is_ok(),
                    (Some(Input::Rw(f)), _) => f.seek(target).is_ok(),
                    (None, Some(Output::File(w))) => w.seek(target).is_ok(),
                    _ => false,
                };
                fh.eof = false;
                Ok(ok)
            })();
            r.map(|b| vec![Value::from(b)])
        }
        "tell" => {
            let r = (|| -> ER<i64> {
                let hv = it.scalar(&args[0], scope)?;
                let Some(h) = it.handle_of(&hv, false) else { return Ok(-1) };
                let mut fh = h.borrow_mut();
                let fh = &mut *fh;
                Ok(match (fh.input.as_mut(), fh.output.as_mut()) {
                    (Some(Input::File(f)), _) => f.stream_position().map(|p| p as i64).unwrap_or(-1),
                    (Some(Input::Memory(m)), _) => m.position() as i64,
                    (Some(Input::Rw(f)), _) => f.seek(SeekFrom::Current(0)).map(|p| p as i64).unwrap_or(-1),
                    (None, Some(Output::File(w))) => w.stream_position().map(|p| p as i64).unwrap_or(-1),
                    (None, Some(Output::Scalar(c))) => c.borrow().to_bytes().len() as i64,
                    _ => -1,
                })
            })();
            r.map(|n| vec![Value::Int(n)])
        }
        "fileno" => Ok(vec![Value::Int(3)]),
        "truncate" => {
            let r = (|| -> ER<bool> {
                let hv = it.scalar(&args[0], scope)?;
                let len = it.scalar(&args[1], scope)?.to_i64().max(0) as u64;
                if let Some(h) = it.handle_of(&hv, false) {
                    let mut fh = h.borrow_mut();
                    let fh = &mut *fh;
                    return Ok(match (&mut fh.input, &mut fh.output) {
                        (Some(Input::Rw(f)), _) => {
                            f.sync();
                            f.f.set_len(len).is_ok()
                        }
                        (_, Some(Output::File(w))) => w.flush().is_ok() && w.get_ref().set_len(len).is_ok(),
                        _ => false,
                    });
                }
                Ok(std::fs::OpenOptions::new().write(true).open(hv.to_text()).and_then(|f| f.set_len(len)).is_ok())
            })();
            r.map(|b| vec![Value::from(b)])
        }
        "stat" | "lstat" => {
            let r = (|| -> ER<Vec<Value>> {
                let m = match args.first() {
                    Some(a) => {
                        let target = file_target(it, a, scope)?;
                        match target {
                            Target::Last => return Ok(it.last_stat.clone()),
                            Target::Path(p) => if name == "lstat" { std::fs::symlink_metadata(p) } else { std::fs::metadata(p) },
                        }
                    }
                    None => std::fs::metadata(it.read_scalar("_", scope).to_text()),
                };
                let v = match m {
                    Ok(m) => stat_list(&m),
                    Err(_) => vec![],
                };
                it.last_stat = v.clone();
                Ok(v)
            })();
            r
        }
        "select" => Ok(vec![Value::str("main::STDOUT")]),
        "unlink" => {
            let mut n = 0;
            let files = if args.is_empty() {
                vec![it.read_scalar("_", scope)]
            } else {
                let mut v = Vec::new();
                for a in args {
                    match it.list(a, scope) {
                        Ok(x) => v.extend(x),
                        Err(e) => return Some(Err(e)),
                    }
                }
                v
            };
            for f in files {
                if std::fs::remove_file(f.to_text()).is_ok() {
                    n += 1;
                }
            }
            Ok(vec![Value::Int(n)])
        }
        _ => return None,
    })
}

pub(super) enum Target {
    Path(String),
    /// `_`: the last stat
    Last,
}

/// What a stat or file test applies to: a name, a handle's file, or `_`.
pub(super) fn file_target(it: &mut Interp, e: &Expr, scope: &Rc<Scope>) -> ER<Target> {
    match e {
        Expr::Call { name, amp: true, args } if &**name == "_" && args.is_empty() => return Ok(Target::Last),
        Expr::Const(Value::Str(s)) if &s[..] == b"*_" || s.ends_with(b"::_") => return Ok(Target::Last),
        _ => {}
    }
    let v = it.scalar(e, scope)?;
    if let Value::Handle(_) | Value::ScalarRef(_) = &v {
        if let Some(h) = it.handle_of(&v, false) {
            return Ok(Target::Path(h.borrow().path.clone().unwrap_or_default()));
        }
    }
    if let Value::Str(s) = &v {
        if s.starts_with(b"*") {
            if let Some(h) = it.handle_of(&v, false) {
                return Ok(Target::Path(h.borrow().path.clone().unwrap_or_default()));
            }
        }
    }
    Ok(Target::Path(v.to_text()))
}

/// perl's 13-element stat list.
pub(super) fn stat_list(m: &std::fs::Metadata) -> Vec<Value> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        vec![
            Value::Int(m.dev() as i64),
            Value::Int(m.ino() as i64),
            Value::Int(m.mode() as i64),
            Value::Int(m.nlink() as i64),
            Value::Int(m.uid() as i64),
            Value::Int(m.gid() as i64),
            Value::Int(m.rdev() as i64),
            Value::Int(m.size() as i64),
            Value::Int(m.atime()),
            Value::Int(m.mtime()),
            Value::Int(m.ctime()),
            Value::Int(m.blksize() as i64),
            Value::Int(m.blocks() as i64),
        ]
    }
    #[cfg(not(unix))]
    {
        let t = |x: std::io::Result<std::time::SystemTime>| x.ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
        vec![
            Value::Int(0),
            Value::Int(0),
            Value::Int(if m.is_dir() { 0o40777 } else if m.permissions().readonly() { 0o100444 } else { 0o100666 }),
            Value::Int(1),
            Value::Int(0),
            Value::Int(0),
            Value::Int(0),
            Value::Int(m.len() as i64),
            Value::Int(t(m.accessed())),
            Value::Int(t(m.modified())),
            Value::Int(t(m.created())),
            Value::str(""),
            Value::str(""),
        ]
    }
}

fn open(it: &mut Interp, args: &[Expr], scope: &Rc<Scope>) -> ER<Vec<Value>> {
    let Some(first) = args.first() else { return die("Not enough arguments for open") };
    // The handle: a bareword, or a scalar that receives a new handle.
    let h: HandleRc = match first {
        Expr::Const(v) => it.handle_of(v, true).unwrap(),
        e => {
            let lv = it.lvalue(e, scope)?;
            let cur = lv.get();
            match cur {
                Value::Undef => {
                    let h: HandleRc = Rc::new(RefCell::new(FileHandle::default()));
                    lv.set(Value::Handle(h.clone()))?;
                    h
                }
                // A handle, or a glob (\*FH) naming one.
                v => match it.handle_of(&v, true) {
                    Some(h) => h,
                    None => return die("Bad filehandle"),
                },
            }
        }
    };
    let (mode, target) = if args.len() >= 3 {
        (it.scalar(&args[1], scope)?.to_text(), it.scalar(&args[2], scope)?)
    } else {
        let spec = match args.get(1) {
            Some(a) => it.scalar(a, scope)?.to_text(),
            None => return die("1-argument open is not supported"),
        };
        let spec = spec.trim_start().to_string();
        let (m, rest) = if let Some(r) = spec.strip_prefix(">>") {
            (">>", r)
        } else if let Some(r) = spec.strip_prefix("+<") {
            ("+<", r)
        } else if let Some(r) = spec.strip_prefix('>') {
            (">", r)
        } else if let Some(r) = spec.strip_prefix('<') {
            ("<", r)
        } else if spec.ends_with('|') || spec.starts_with('|') {
            return die("pipes are not supported");
        } else {
            ("<", spec.as_str())
        };
        // perl trims the name, but a NUL ends it (protecting trailing spaces).
        let rest = rest.trim_start();
        let rest = match rest.find('\0') {
            Some(p) => &rest[..p],
            None => rest.trim_end(),
        };
        (m.to_string(), Value::str(rest))
    };
    let mode_base: String = mode.chars().take_while(|c| !c.is_whitespace() && *c != ':').collect();
    let mut fh = h.borrow_mut();
    fh.input = None;
    fh.output = None;
    fh.eof = false;
    fh.lines = 0;
    fh.path = if target.is_ref() { None } else { Some(target.to_text()) };
    let ok = match (mode_base.as_str(), &target) {
        ("<", Value::ScalarRef(c)) => {
            fh.input = Some(Input::Memory(Cursor::new(c.borrow().to_bytes().to_vec())));
            true
        }
        (">" | ">>", Value::ScalarRef(c)) => {
            if mode_base == ">" {
                *c.borrow_mut() = Value::str("");
            }
            fh.output = Some(Output::Scalar(c.clone()));
            true
        }
        ("+<", t) => match std::fs::OpenOptions::new().read(true).write(true).open(t.to_text()) {
            Ok(f) => {
                fh.input = Some(Input::Rw(RwFile { f, buf: Vec::new(), at: 0 }));
                fh.output = Some(Output::Rw);
                true
            }
            Err(e) => {
                drop(fh);
                it.set_scalar("main::!", Value::from(os_error_text(&e)));
                return Ok(vec![Value::from(false)]);
            }
        },
        ("<", t) => match File::open(t.to_text()) {
            Ok(f) => {
                fh.input = Some(Input::File(BufReader::new(f)));
                true
            }
            Err(e) => {
                drop(fh);
                it.set_scalar("main::!", Value::from(os_error_text(&e)));
                return Ok(vec![Value::from(false)]);
            }
        },
        (">" | ">>", t) => {
            let r = if mode_base == ">" { File::create(t.to_text()) } else { std::fs::OpenOptions::new().append(true).create(true).open(t.to_text()) };
            match r {
                Ok(f) => {
                    fh.output = Some(Output::File(std::io::BufWriter::new(f)));
                    true
                }
                Err(e) => {
                    drop(fh);
                    it.set_scalar("main::!", Value::from(os_error_text(&e)));
                    return Ok(vec![Value::from(false)]);
                }
            }
        }
        _ => return die(format!("Unknown open() mode '{mode}'")),
    };
    Ok(vec![Value::from(ok)])
}

fn os_error_text(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "No such file or directory".into(),
        std::io::ErrorKind::PermissionDenied => "Permission denied".into(),
        _ => e.to_string(),
    }
}

/// `read(FH, $buf, LEN, OFFSET)`
fn read(it: &mut Interp, args: &[Expr], scope: &Rc<Scope>) -> ER<Vec<Value>> {
    if args.len() < 3 {
        return die("Not enough arguments for read");
    }
    let hv = it.scalar(&args[0], scope)?;
    let lv = it.lvalue(&args[1], scope)?;
    let len = it.scalar(&args[2], scope)?.to_i64().max(0) as usize;
    let off = match args.get(3) {
        Some(o) => it.scalar(o, scope)?.to_i64().max(0) as usize,
        None => 0,
    };
    let Some(h) = it.handle_of(&hv, false) else { return Ok(vec![Value::Undef]) };
    let mut data = vec![0u8; len];
    let n = {
        let mut fh = h.borrow_mut();
        match fh.input.as_mut() {
            Some(i) => {
                let r = i.reader();
                let mut got = 0;
                while got < len {
                    match r.read(&mut data[got..]) {
                        Ok(0) => break,
                        Ok(k) => got += k,
                        Err(_) => break,
                    }
                }
                got
            }
            None => return Ok(vec![Value::Undef]),
        }
    };
    data.truncate(n);
    let mut cur = lv.get().to_bytes().to_vec();
    cur.resize(off, 0);
    cur.extend_from_slice(&data);
    lv.set(Value::bytes(cur))?;
    Ok(vec![Value::Int(n as i64)])
}
