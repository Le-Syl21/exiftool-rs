//! The main program of the `exiftool` script (`main::<program>`, run in
//! place of the script's top-level code when EXIFTOOL_NATIVE_MAIN=1, see
//! examples/run_t.rs): the BEGIN block and the module loading (script lines
//! 1-96), the file lexicals and their initial values (99-327), the arguments
//! common to all commands (383-398), the command loop (400-689), the end of
//! the argument loop (1449-1470) and the processing of each command
//! (1471-2108).
//!
//! The script's subs that are not ported yet are still its Perl: the
//! program loads them from the script itself (its `sub` definitions and its
//! END block, every other line left out), in a scope holding the file
//! lexicals, which are the `main::<my>::NAME` variables the natives share.

use std::cell::RefCell;
use std::rc::Rc;

use crate::interp::eval::{ArrayRc, Flow, HashRc, ER};
use crate::interp::{Code, Interp, Scope, Want};
use crate::perl::Value;
use crate::pl::{self, cap, re};

pub fn register(it: &mut Interp) {
    super::define(it, "main::<program>", program);
}

// ---- the file lexicals ----

/// The script's file lexicals (lines 14-327), by sigil and name, and the
/// variables of the command loop (lines 578-591), which the option chain
/// (options.rs) also reads as file lexicals.
const LEXICALS: &[(u8, &str)] = &[
    (b'$', "version"),
    (b'$', "exePath"),
    (b'@', "commonArgs"),
    (b'@', "condition"),
    (b'@', "csvExclude"),
    (b'@', "csvFiles"),
    (b'@', "csvTags"),
    (b'@', "delFiles"),
    (b'@', "dynamicFiles"),
    (b'@', "echo3"),
    (b'@', "echo4"),
    (b'@', "efile"),
    (b'@', "exclude"),
    (b'@', "files"),
    (b'@', "moreArgs"),
    (b'@', "newValues"),
    (b'@', "requestTags"),
    (b'@', "srcFmt"),
    (b'@', "tags"),
    (b'%', "altFile"),
    (b'%', "appended"),
    (b'%', "countLink"),
    (b'%', "created"),
    (b'%', "csvTags"),
    (b'%', "database"),
    (b'%', "filterExt"),
    (b'%', "ignore"),
    (b'%', "outComma"),
    (b'%', "outTrailer"),
    (b'%', "preserveTime"),
    (b'%', "printFmt"),
    (b'%', "seqFileDir"),
    (b'%', "setTags"),
    (b'%', "setTagsList"),
    (b'%', "usedFileName"),
    (b'%', "utf8FileName"),
    (b'%', "warnedOnce"),
    (b'%', "wext"),
    (b'%', "wroteHEAD"),
    (b'$', "allGroup"),
    (b'$', "altEnc"),
    (b'$', "argFormat"),
    (b'$', "binaryOutput"),
    (b'$', "binaryStdout"),
    (b'$', "binSep"),
    (b'$', "binTerm"),
    (b'$', "comma"),
    (b'$', "count"),
    (b'$', "countBad"),
    (b'$', "countBadCr"),
    (b'$', "countBadWr"),
    (b'$', "countCopyWr"),
    (b'$', "countDir"),
    (b'$', "countFailed"),
    (b'$', "countGoodCr"),
    (b'$', "countGoodWr"),
    (b'$', "countNewDir"),
    (b'$', "countSameWr"),
    (b'$', "critical"),
    (b'$', "csv"),
    (b'$', "csvDelim"),
    (b'$', "dbAdd"),
    (b'$', "dbSaveCount"),
    (b'$', "deleteOrig"),
    (b'$', "diff"),
    (b'$', "disableOutput"),
    (b'$', "doSetFileName"),
    (b'$', "doUnzip"),
    (b'$', "end"),
    (b'$', "endDir"),
    (b'%', "endDir"),
    (b'$', "escapeC"),
    (b'$', "escapeHTML"),
    (b'$', "evalWarning"),
    (b'$', "executeID"),
    (b'$', "failCondition"),
    (b'$', "fastCondition"),
    (b'$', "fileHeader"),
    (b'$', "fileTrailer"),
    (b'$', "filtered"),
    (b'$', "filterFlag"),
    (b'$', "fixLen"),
    (b'$', "forcePrint"),
    (b'$', "geoOnly"),
    (b'$', "helped"),
    (b'$', "html"),
    (b'$', "ignoreHidden"),
    (b'$', "interrupted"),
    (b'$', "isBinary"),
    (b'$', "isWriting"),
    (b'$', "joinLists"),
    (b'$', "json"),
    (b'$', "langOpt"),
    (b'$', "listDir"),
    (b'$', "listItem"),
    (b'$', "listSep"),
    (b'$', "mt"),
    (b'$', "multiFile"),
    (b'$', "noBinary"),
    (b'$', "outFormat"),
    (b'$', "outOpt"),
    (b'$', "overwriteOrig"),
    (b'$', "pause"),
    (b'$', "plot"),
    (b'$', "preserveTime"),
    (b'$', "progress"),
    (b'$', "progressCount"),
    (b'$', "progressIncr"),
    (b'$', "progressMax"),
    (b'$', "progressNext"),
    (b'$', "progStr"),
    (b'$', "purge"),
    (b'$', "quiet"),
    (b'$', "rafStdin"),
    (b'$', "recurse"),
    (b'$', "rtnVal"),
    (b'$', "rtnValPrev"),
    (b'$', "saveCount"),
    (b'$', "scanWritable"),
    (b'$', "sectHeader"),
    (b'$', "sectTrailer"),
    (b'$', "seqFileDir"),
    (b'$', "seqFileNum"),
    (b'$', "setCharset"),
    (b'$', "showGroup"),
    (b'$', "showTagID"),
    (b'$', "stayOpenBuff"),
    (b'$', "stayOpenFile"),
    (b'$', "stayOpenStdin"),
    (b'$', "structOpt"),
    (b'$', "tabFormat"),
    (b'$', "tagOut"),
    (b'$', "textOut"),
    (b'$', "textOut2"),
    (b'$', "textOverwrite"),
    (b'$', "tmpFile"),
    (b'$', "tmpText"),
    (b'$', "validFile"),
    (b'$', "verbose"),
    (b'$', "vout"),
    (b'$', "windowTitle"),
    (b'$', "xml"),
    (b'$', "stayOpen"),
    (b'$', "rtnValApp"),
    (b'$', "curTitle"),
    (b'$', "isCRLF"),
    (b'%', "jsonChar"),
    (b'%', "escC"),
    (b'%', "unescC"),
    (b'%', "optArgs"),
    (b'@', "recommends"),
    (b'%', "altRecommends"),
    (b'%', "unescapeChar"),
    // the command loop's own (lines 578-591)
    (b'@', "fileOrder"),
    (b'$', "fileOrderFast"),
    (b'$', "addGeotime"),
    (b'$', "doGlob"),
    (b'$', "endOfOpts"),
    (b'$', "escapeXML"),
    (b'$', "setTagsFile"),
    (b'$', "sortOpt"),
    (b'$', "srcStdin"),
    (b'$', "tagsFrom"),
    (b'$', "useMWG"),
    (b'$', "argsLeft"),
    (b'@', "nextPass"),
    (b'$', "badCmd"),
    (b'$', "pass"),
];

/// A scope declaring every file lexical as its shared variable
/// (`main::<my>::NAME`): the scope of the script's own subs.
fn file_scope(it: &mut Interp) -> Rc<Scope> {
    let scope = Scope::new(None);
    for &(sigil, name) in LEXICALS {
        let b = it.global(sigil, &format!("main::<my>::{name}"));
        scope.declare(sigil, name, b);
    }
    scope
}

/// `$name`
fn sv(it: &mut Interp, name: &str) -> Value {
    super::lex_scalar(it, name).borrow().clone()
}

/// `$name = v`
fn set(it: &mut Interp, name: &str, v: impl Into<Value>) {
    *super::lex_scalar(it, name).borrow_mut() = v.into();
}

/// `@name`
fn av(it: &mut Interp, name: &str) -> ArrayRc {
    match super::lex_array(it, name) {
        Value::ArrayRef(a) => a,
        _ => unreachable!(),
    }
}

/// `%name`
fn hv(it: &mut Interp, name: &str) -> HashRc {
    match super::lex_hash(it, name) {
        Value::HashRef(h) => h,
        _ => unreachable!(),
    }
}

/// `@name` in boolean context.
fn has(it: &mut Interp, name: &str) -> bool {
    !av(it, name).borrow().is_empty()
}

/// `%name` in boolean context.
fn hhas(it: &mut Interp, name: &str) -> bool {
    !hv(it, name).borrow().is_empty()
}

/// `$name` in boolean context.
fn t(it: &mut Interp, name: &str) -> bool {
    sv(it, name).truthy()
}

fn n(i: i64) -> Value {
    Value::Int(i)
}

fn s(x: &str) -> Value {
    Value::str(x)
}

/// A string from pieces (perl's interpolation: undef is "").
fn cat(parts: &[&dyn AsBytes]) -> Value {
    let mut out = Vec::new();
    for p in parts {
        out.extend_from_slice(&p.bytes());
    }
    Value::bytes(out)
}

trait AsBytes {
    fn bytes(&self) -> Vec<u8>;
}

impl AsBytes for Value {
    fn bytes(&self) -> Vec<u8> {
        self.to_bytes().to_vec()
    }
}

impl AsBytes for &str {
    fn bytes(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }
}

/// `@ARGV`
fn argv(it: &mut Interp) -> ArrayRc {
    match pl::pkg_array(it, "main::ARGV") {
        Value::ArrayRef(a) => a,
        _ => unreachable!(),
    }
}

/// `shift @ARGV`
fn shift_argv(it: &mut Interp) -> Value {
    let a = argv(it);
    let mut a = a.borrow_mut();
    if a.is_empty() {
        Value::Undef
    } else {
        a.remove(0)
    }
}

/// `$_`
fn topic(it: &mut Interp) -> Value {
    it.get_scalar("main::_")
}

fn set_topic(it: &mut Interp, v: Value) {
    it.set_scalar("main::_", v);
}

/// `Func(ARGS)` of the script (or of a module) in void context.
fn call_void(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<()> {
    it.call_named(name, args, Want::Void)?;
    Ok(())
}

/// `Warn MSG`
fn warn_(it: &mut Interp, msg: Value) -> ER<()> {
    call_void(it, "main::Warn", vec![msg])
}

/// `Error MSG`
fn error(it: &mut Interp, msg: Value) -> ER<()> {
    call_void(it, "main::Error", vec![msg])
}

/// `$mt->Options(ARGS)` in scalar context.
fn options(it: &mut Interp, args: Vec<Value>) -> ER<Value> {
    let mt = sv(it, "mt");
    pl::method(it, &mt, "Options", args)
}

/// Run a snippet of Perl for a primitive that has no helper (`open`,
/// `close`, `binmode`, `system`, `<STDIN>`, `\*FH`, `$|`...), in package
/// main, with the scalars `vars` as lexicals. `$@` is left as it was; a die
/// or an exit goes through.
fn perl(it: &mut Interp, code: &str, vars: &[(&str, Value)], want: Want) -> ER<Vec<Value>> {
    let scope = Scope::new(None);
    for (name, v) in vars {
        scope.scalar(name, v.clone());
    }
    let saved_err = it.get_scalar("main::@");
    let r = it.eval_string(code.as_bytes(), "main", &scope, want);
    it.set_scalar("main::@", saved_err);
    match r {
        Ok(v) => Ok(v),
        Err(e) => match it.exit_code {
            Some(code) => Err(Flow::Exit(code)),
            None => Err(Flow::Die(e)),
        },
    }
}

/// `perl` in scalar context.
fn perl1(it: &mut Interp, code: &str, vars: &[(&str, Value)]) -> ER<Value> {
    Ok(perl(it, code, vars, Want::Scalar)?.pop().unwrap_or_default())
}

/// `\*NAME` (in package main unless qualified)
fn glob(it: &mut Interp, name: &str) -> ER<Value> {
    perl1(it, &format!("\\*{name}"), &[])
}

/// `printf FH FMT, LIST` (STDOUT when `fh` is None)
fn printf(it: &mut Interp, fh: Option<&Value>, fmt: &str, args: &[Value]) -> ER<()> {
    let text = pl::sprintf(fmt, args);
    pl::print_to(it, fh, &[text])?;
    Ok(())
}

/// `print STDERR LIST`
fn print_stderr(it: &mut Interp, items: &[Value]) -> ER<()> {
    let fh = glob(it, "STDERR")?;
    pl::print_to(it, Some(&fh), items)?;
    Ok(())
}

/// `sub { $evalWarning = $_[0] }`
fn eval_warning_handler() -> Value {
    let f = |it: &mut Interp, a: Vec<Value>, _w: Want| -> ER<Vec<Value>> {
        let v = pl::arg(&a, 0);
        set(it, "evalWarning", v.clone());
        Ok(vec![v])
    };
    Value::Code(Rc::new(Code::Native(Rc::from("main::__ANON__"), Rc::new(f))))
}

// ---- the main program ----

/// The script's top-level code.
pub fn program(it: &mut Interp, _a: Vec<Value>, _want: Want) -> ER<Vec<Value>> {
    // use strict;
    it.strict = true;
    // (use warnings: the script's subs have theirs; require 5.004: nothing)
    begin(it)?;
    // use Image::ExifTool qw{:Public};
    perl(it, "use Image::ExifTool qw{:Public}; 1", &[], Want::Void)?;
    // (the script's subs are all natives)
    let scope = file_scope(it);
    // END {
    //     Cleanup();
    // }
    it.add_end_sub("main::Cleanup");

    // my $version = '13.60';
    set(it, "version", s("13.60"));

    // $^W = 1;    # enable global warnings
    it.set_scalar("main::^W", n(1));

    // # function prototypes (lines 45-90: nothing to do at run time)

    // $SIG{INT}  = 'SigInt';  # do cleanup on Ctrl-C
    // $SIG{CONT} = 'SigCont'; # (allows break-out of delays)
    let sig = pl::pkg_hash(it, "main::SIG");
    sig.set("INT", s("SigInt"));
    sig.set("CONT", s("SigCont"));

    init_lexicals(it)?;
    common_args(it);
    command_loop(it, &scope)?;

    // close STAYOPEN if $stayOpen >= 2;
    let stay_open = sv(it, "stayOpen");
    if pl::ge(&stay_open, &n(2)) {
        perl(it, "close STAYOPEN", &[], Want::Void)?;
    }

    // Exit $rtnValApp;    # all done
    let rtn = sv(it, "rtnValApp");
    call_void(it, "main::Exit", vec![rtn])?;
    Ok(vec![])
}

/// The BEGIN block (lines 20-42): add our 'lib' directory to the include
/// list BEFORE 'use Image::ExifTool'.
fn begin(it: &mut Interp) -> ER<()> {
    // (undocumented -xpath option added in 11.91, must come before other options)
    // $exePath = @ARGV && lc($ARGV[0]) eq '-xpath' && shift() ? $^X : $0;
    let args = argv(it);
    let first = args.borrow().first().cloned();
    let xpath = match first {
        Some(a0) if pl::str_eq(&pl::lc(&a0), "-xpath") => shift_argv(it).truthy(),
        _ => false,
    };
    let exe_path = if xpath { it.get_scalar("main::^X") } else { it.get_scalar("main::0") };
    set(it, "exePath", exe_path.clone());
    // get exe directory
    // my $exeDir = ($exePath =~ /(.*)[\\\/]/) ? $1 : '.';
    let mut exe_dir = match re(r"(.*)[\\\/]", "").m(&exe_path) {
        Some(c) => cap(&c, 1),
        None => s("."),
    };
    // my $incDir = ($0 =~ /(.*)[\\\/]/) ? "$1/lib" : './lib';
    let zero = it.get_scalar("main::0");
    let mut inc_dir = match re(r"(.*)[\\\/]", "").m(&zero) {
        Some(c) => cat(&[&cap(&c, 1), &"/lib"]),
        None => s("./lib"),
    };
    // if (-l $0) {
    if perl1(it, "-l $f", &[("f", zero.clone())])?.truthy() {
        // my $lnk = eval { readlink $0 };
        let lnk = pl::eval_block(it, |it| perl1(it, "readlink $f", &[("f", zero.clone())]))?.unwrap_or_default();
        if !lnk.is_undef() {
            // my $lnkDir = ($lnk =~ /(.*)[\\\/]/) ? $1 : '.';
            let lnk_dir = match re(r"(.*)[\\\/]", "").m(&lnk) {
                Some(c) => cap(&c, 1),
                None => s("."),
            };
            // $exeDir = (($lnk =~ m(^/)) ? '' : $exeDir . '/') . $lnkDir;
            let base = if re(r"^/", "").is_match(&lnk) { s("") } else { cat(&[&exe_dir, &"/"]) };
            exe_dir = cat(&[&base, &lnk_dir]);
            // $incDir = "$exeDir/lib";
            inc_dir = cat(&[&exe_dir, &"/lib"]);
        }
    }
    // $Image::ExifTool::exeDir = $exeDir; # use our exeDir for loading config file
    it.set_scalar("Image::ExifTool::exeDir", exe_dir);
    // add lib directory at start of include path
    // unshift @INC, $incDir;
    pl::pkg_array(it, "main::INC").unshift(inc_dir);
    // load or disable config file if specified
    // while (@ARGV and lc($ARGV[0]) eq '-config') {
    loop {
        let first = argv(it).borrow().first().cloned();
        match first {
            Some(a0) if pl::str_eq(&pl::lc(&a0), "-config") => {}
            _ => break,
        }
        // shift;
        shift_argv(it);
        // push @Image::ExifTool::configFiles, shift;
        let f = shift_argv(it);
        pl::pkg_array(it, "Image::ExifTool::configFiles").push(f);
    }
    Ok(())
}


/// The file lexicals' initial values (lines 99-327).
fn init_lexicals(it: &mut Interp) -> ER<()> {
    // (the `my` declarations without a value leave the variables undefined
    //  and empty, which they are; $exePath has the BEGIN block's value)

    // my $stayOpenBuff='';# buffer for -stay_open file
    set(it, "stayOpenBuff", s(""));

    // flag to keep the input -@ argfile open:
    // 0 = normal behaviour
    // 1 = received "-stay_open true" and waiting for argfile to keep open
    // 2 = currently reading from STAYOPEN argfile
    // 3 = waiting for -@ to switch to a new STAYOPEN argfile
    // my $stayOpen = 0;
    set(it, "stayOpen", n(0));

    // my $rtnValApp = 0;  # app return value (0=success)
    set(it, "rtnValApp", n(0));
    // my $curTitle = '';  # current window title
    set(it, "curTitle", s(""));

    // lookup for O/S names which use CR/LF newlines
    // my $isCRLF = { MSWin32 => 1, os2 => 1, dos => 1 }->{$^O};
    let os = it.get_scalar("main::^O");
    let crlf = Value::new_hash();
    crlf.set("MSWin32", n(1));
    crlf.set("os2", n(1));
    crlf.set("dos", n(1));
    set(it, "isCRLF", crlf.get(os.to_bytes()));

    // lookup for JSON characters that we escape specially
    // my %jsonChar = ( '"'=>'"', '\\'=>'\\', "\t"=>'t', "\n"=>'n', "\r"=>'r' );
    fill_hash(it, "jsonChar", &[("\"", "\""), ("\\", "\\"), ("\t", "t"), ("\n", "n"), ("\r", "r")]);

    // lookup for C-style escape sequences
    // my %escC = ( "\n" => '\n', "\r" => '\r', "\t" => '\t', '\\' => '\\\\');
    fill_hash(it, "escC", &[("\n", "\\n"), ("\r", "\\r"), ("\t", "\\t"), ("\\", "\\\\")]);
    // my %unescC = ( a => "\a", b => "\b", f => "\f", n => "\n", r => "\r",
    //                t => "\t", 0 => "\0", '\\' => '\\' );
    fill_hash(
        it,
        "unescC",
        &[("a", "\x07"), ("b", "\x08"), ("f", "\x0c"), ("n", "\n"), ("r", "\r"), ("t", "\t"), ("0", "\0"), ("\\", "\\")],
    );

    // options requiring additional arguments
    // (used only to skip over these arguments when reading -stay_open ARGFILE)
    // (arg is converted to lower case then tested again unless an entry was found with the same case)
    let opt_args: &[(&str, i64)] = &[
        ("-tagsfromfile", 1),
        ("-addtagsfromfile", 1),
        ("-alltagsfromfile", 1),
        ("-@", 1),
        ("-api", 1),
        ("-c", 1),
        ("-coordformat", 1),
        ("-charset", 0), // (optional arg; OK because arg cannot begin with "-")
        ("-config", 1),
        ("-csvdelim", 1),
        ("-d", 1),
        ("-dateformat", 1),
        ("-D", 0), // necessary to avoid matching lower-case equivalent
        ("-diff", 1),
        ("-echo", 1),
        ("-echo#", 1),
        ("-efile", 1),
        ("-efile#", 1),
        ("-efile!", 1),
        ("-efile#!", 1),
        ("-ext", 1),
        ("--ext", 1),
        ("-ext+", 1),
        ("--ext+", 1),
        ("-extension", 1),
        ("--extension", 1),
        ("-extension+", 1),
        ("--extension+", 1),
        ("-fileorder", 1),
        ("-fileorder#", 1),
        ("-file#", 1),
        ("-geotag", 1),
        ("-globaltimeshift", 1),
        ("-i", 1),
        ("-ignore", 1),
        ("-if", 1),
        ("-if#", 1),
        ("-lang", 0), // (optional arg; cannot begin with "-")
        ("-listitem", 1),
        ("-o", 1),
        ("-out", 1),
        ("-p", 1),
        ("-printformat", 1),
        ("-p-", 1),
        ("-printformat-", 1),
        ("-P", 0),
        ("-password", 1),
        ("-require", 1),
        ("-sep", 1),
        ("-separator", 1),
        ("-srcfile", 1),
        ("-stay_open", 1),
        ("-use", 1),
        ("-userparam", 1),
        ("-w", 1),
        ("-w!", 1),
        ("-w+", 1),
        ("-w+!", 1),
        ("-w!+", 1),
        ("-textout", 1),
        ("-textout!", 1),
        ("-textout+", 1),
        ("-textout+!", 1),
        ("-textout!+", 1),
        ("-tagout", 1),
        ("-tagout!", 1),
        ("-tagout+", 1),
        ("-tagout+!", 1),
        ("-tagout!+", 1),
        ("-wext", 1),
        ("-wm", 1),
        ("-writemode", 1),
        ("-x", 1),
        ("-exclude", 1),
        ("-X", 0),
    ];
    let h = hv(it, "optArgs");
    {
        let mut h = h.borrow_mut();
        h.clear();
        for (k, v) in opt_args {
            h.insert(k.as_bytes(), n(*v));
        }
    }

    // recommended packages and alternatives
    let recommends = [
        "Archive::Zip",
        "Compress::Zlib",
        "Digest::MD5",
        "Digest::SHA",
        "IO::Compress::Bzip2",
        "POSIX::strptime",
        "Time::Local",
        "Unicode::LineBreak",
        "File::StatX",
        "Compress::Raw::Lzma",
        "IO::Compress::RawDeflate",
        "IO::Uncompress::RawInflate",
        "IO::Compress::Brotli",
        "IO::Uncompress::Brotli",
        "Win32::API",
        "Win32::FindFile",
        "Win32API::File",
    ];
    *av(it, "recommends").borrow_mut() = recommends.iter().map(|r| s(r)).collect();
    // my %altRecommends = (
    //    'POSIX::strptime' => 'Time::Piece', # (can use Time::Piece instead of POSIX::strptime)
    // );
    fill_hash(it, "altRecommends", &[("POSIX::strptime", "Time::Piece")]);

    // my %unescapeChar = ( 't'=>"\t", 'n'=>"\n", 'r'=>"\r" );
    fill_hash(it, "unescapeChar", &[("t", "\t"), ("n", "\n"), ("r", "\r")]);
    Ok(())
}

/// `%name = (k => v, ...)` with string values.
fn fill_hash(it: &mut Interp, name: &str, pairs: &[(&str, &str)]) {
    let h = hv(it, name);
    let mut h = h.borrow_mut();
    h.clear();
    for (k, v) in pairs {
        h.insert(k.as_bytes(), s(v));
    }
}

/// Isolate arguments common to all commands (lines 383-398).
fn common_args(it: &mut Interp) {
    // if (grep /^-common_args$/i, @ARGV) {
    let re_common = re(r"^-common_args$", "i");
    let args: Vec<Value> = argv(it).borrow().clone();
    if args.iter().any(|a| re_common.is_match(a)) {
        // my (@newArgs, $common, $end);
        let (mut new_args, mut common, mut end) = (Vec::new(), false, false);
        let common_args = av(it, "commonArgs");
        // foreach (@ARGV) {
        for a in args {
            if re_common.is_match(&a) && !end {
                common = true;
            } else if common {
                common_args.borrow_mut().push(a);
            } else {
                if pl::str_eq(&a, "--") {
                    end = true;
                }
                new_args.push(a);
            }
        }
        // @ARGV = @newArgs if $common;
        if common {
            *argv(it).borrow_mut() = new_args;
        }
    }
}

/// `undef @name` / `undef %name` / `undef $name` for each of `names`.
fn undef_all(it: &mut Interp, arrays: &[&str], hashes: &[&str], scalars: &[&str]) {
    for a in arrays {
        av(it, a).borrow_mut().clear();
    }
    for h in hashes {
        hv(it, h).borrow_mut().clear();
    }
    for v in scalars {
        set(it, v, Value::Undef);
    }
}

/// Loop over sets of command-line arguments separated by "-execute"
/// (`Command: for (;;) {`, lines 400-2102).
fn command_loop(it: &mut Interp, _scope: &Rc<Scope>) -> ER<()> {
    let status = re(r"\$\{status\}", "i");
    'command: loop {
        for (echo, stderr) in [("echo3", false), ("echo4", true)] {
            // if (@echo3) {
            if has(it, echo) {
                // my $str = join("\n", @echo3) . "\n";
                let items = av(it, echo).borrow().clone();
                let mut str = cat(&[&pl::join("\n", &items), &"\n"]);
                // $str =~ s/\$\{status\}/$rtnVal/ig;
                let rtn_val = sv(it, "rtnVal");
                // (perl warns here if $rtnVal is undef)
                status.subst(&mut str, true, |_| rtn_val.to_bytes().to_vec());
                // print STDOUT $str;  /  print STDERR $str;
                if stderr {
                    print_stderr(it, &[str])?;
                } else {
                    let fh = glob(it, "STDOUT")?;
                    pl::print_to(it, Some(&fh), &[str])?;
                }
            }
        }

        // $rafStdin->Close() if $rafStdin;
        let raf_stdin = sv(it, "rafStdin");
        if raf_stdin.truthy() {
            pl::method(it, &raf_stdin, "Close", vec![])?;
        }
        // undef $rafStdin;
        set(it, "rafStdin", Value::Undef);

        // save our previous return codes
        // $rtnValPrev = $rtnVal;
        let rtn_val = sv(it, "rtnVal");
        set(it, "rtnValPrev", rtn_val.clone());
        // $rtnValApp = $rtnVal if $rtnVal;
        if rtn_val.truthy() {
            set(it, "rtnValApp", rtn_val.clone());
        }

        // exit Command loop now if we are all done processing commands
        // last unless @ARGV or not defined $rtnVal or $stayOpen >= 2 or @commonArgs;
        let more = !argv(it).borrow().is_empty() || rtn_val.is_undef() || pl::ge(&sv(it, "stayOpen"), &n(2)) || has(it, "commonArgs");
        if !more {
            break 'command;
        }

        // attempt to restore text mode for STDOUT if necessary
        if t(it, "binaryStdout") {
            // binmode(STDOUT,':crlf') if $] >= 5.006 and $isCRLF;
            if pl::ge(&it.get_scalar("main::]"), &Value::Num(5.006)) && t(it, "isCRLF") {
                perl(it, "binmode(STDOUT,':crlf')", &[], Want::Void)?;
            }
            set(it, "binaryStdout", n(0));
        }

        // flush console and print "{ready}" message if -stay_open is in effect
        if pl::ge(&sv(it, "stayOpen"), &n(2)) {
            if t(it, "quiet") && sv(it, "executeID").is_undef() {
                // flush output if possible
                // eval { require IO::Handle } and STDERR->flush(), STDOUT->flush();
                if require_ok(it, "IO::Handle")? {
                    perl(it, "STDERR->flush()", &[], Want::Void)?;
                }
                perl(it, "STDOUT->flush()", &[], Want::Void)?;
            } else {
                // eval { require IO::Handle } and STDERR->flush();
                if require_ok(it, "IO::Handle")? {
                    perl(it, "STDERR->flush()", &[], Want::Void)?;
                }
                // my $id = defined $executeID ? $executeID : '';
                let id = sv(it, "executeID");
                let id = if id.is_undef() { s("") } else { id };
                // my $save = $|;
                let save = perl1(it, "$|", &[])?;
                // $| = 1;     # turn on output autoflush for stdout
                perl(it, "$| = 1", &[], Want::Void)?;
                // print "{ready$id}\n";
                pl::print_to(it, None, &[cat(&[&"{ready", &id, &"}\n"])])?;
                // $| = $save; # restore original autoflush setting
                perl(it, "$| = $save", &[("save", save)], Want::Void)?;
            }
        }

        // initialize necessary static file-scope variables
        // (not done: @commonArgs, @moreArgs, $critical, $binaryStdout, $helped,
        //  $interrupted, $mt, $pause, $rtnValApp, $rtnValPrev, $stayOpen, $stayOpenBuff,
        //  $stayOpenFile, $stayOpenStdin)
        undef_all(
            it,
            &[
                "condition",
                "csvExclude",
                "csvFiles",
                "csvTags",
                "delFiles",
                "dynamicFiles",
                "echo3",
                "echo4",
                "efile",
                "exclude",
                "files",
                "newValues",
                "requestTags",
                "srcFmt",
                "tags",
            ],
            &[
                "altFile",
                "appended",
                "countLink",
                "created",
                "csvTags",
                "database",
                "endDir",
                "filterExt",
                "ignore",
                "outComma",
                "outTrailer",
                "preserveTime",
                "printFmt",
                "seqFileDir",
                "setTags",
                "setTagsList",
                "usedFileName",
                "utf8FileName",
                "warnedOnce",
                "wext",
                "wroteHEAD",
            ],
            &[
                "allGroup",
                "altEnc",
                "argFormat",
                "binaryOutput",
                "binSep",
                "binTerm",
                "comma",
                "csv",
                "dbAdd",
                "deleteOrig",
                "diff",
                "disableOutput",
                "doSetFileName",
                "doUnzip",
                "end",
                "endDir",
                "escapeC",
                "escapeHTML",
                "evalWarning",
                "executeID",
                "failCondition",
                "fastCondition",
                "fileHeader",
                "filtered",
                "fixLen",
                "forcePrint",
                "geoOnly",
                "ignoreHidden",
                "isBinary",
                "joinLists",
                "langOpt",
                "listDir",
                "listItem",
                "multiFile",
                "noBinary",
                "outOpt",
                "plot",
                "preserveTime",
                "progress",
                "progressCount",
                "progressIncr",
                "progressMax",
                "progressNext",
                "purge",
                "rafStdin",
                "recurse",
                "scanWritable",
                "sectHeader",
                "setCharset",
                "showGroup",
                "showTagID",
                "structOpt",
                "tagOut",
                "textOut",
                "textOut2",
                "textOverwrite",
                "tmpFile",
                "tmpText",
                "validFile",
                "verbose",
                "windowTitle",
            ],
        );

        for name in [
            "count",
            "countBad",
            "countBadCr",
            "countBadWr",
            "countCopyWr",
            "countDir",
            "countFailed",
            "countGoodCr",
            "countGoodWr",
            "countNewDir",
            "countSameWr",
        ] {
            set(it, name, n(0));
        }
        set(it, "csvDelim", s(","));
        set(it, "dbSaveCount", n(0));
        set(it, "fileTrailer", s(""));
        set(it, "filterFlag", n(0));
        set(it, "html", n(0));
        set(it, "isWriting", n(0));
        set(it, "json", n(0));
        set(it, "listSep", s(", "));
        set(it, "outFormat", n(0));
        set(it, "overwriteOrig", n(0));
        set(it, "progStr", s(""));
        set(it, "quiet", n(0));
        set(it, "rtnVal", n(0));
        set(it, "saveCount", n(0));
        set(it, "sectTrailer", s(""));
        set(it, "seqFileDir", n(0));
        set(it, "seqFileNum", n(0));
        set(it, "tabFormat", n(0));
        // $vout = \*STDOUT;
        let stdout = glob(it, "STDOUT")?;
        set(it, "vout", stdout);
        set(it, "xml", n(0));

        // define local variables used only in this command loop
        // my @fileOrder;      # tags to use for ordering of input files
        // my $fileOrderFast;  # -fast level for -fileOrder option
        // my $addGeotime;     # automatically added geotime argument
        // my $doGlob;         # flag set to do filename wildcard expansion
        // my $endOfOpts;      # flag set if "--" option encountered
        // my $escapeXML;      # flag to escape printed values for xml
        // my $setTagsFile;    # filename for last TagsFromFile option
        // my $sortOpt;        # sort option is used
        // my $srcStdin;       # one of the source files is STDIN
        // my $tagsFrom = '';  # tags on command line come from 'CSV' or 'File'
        // my $useMWG;         # flag set if we are using any MWG tag
        //
        // my ($argsLeft, @nextPass, $badCmd);
        // my $pass = 0;
        undef_all(
            it,
            &["fileOrder", "nextPass"],
            &[],
            &["fileOrderFast", "addGeotime", "doGlob", "endOfOpts", "escapeXML", "setTagsFile", "sortOpt", "srcStdin", "useMWG", "argsLeft", "badCmd"],
        );
        set(it, "tagsFrom", s(""));
        set(it, "pass", n(0));

        // for Windows, use globbing for wildcard expansion if available - MK/20061010
        let os = it.get_scalar("main::^O");
        if pl::str_eq(&os, "MSWin32") && require_ok(it, "File::Glob")? {
            // override the core glob forcing case insensitivity
            perl(it, "import File::Glob qw(:globally :nocase)", &[], Want::Void)?;
            set(it, "doGlob", n(1));
        }

        // $mt = Image::ExifTool->new;      # create ExifTool object
        let mt = pl::method(it, &s("Image::ExifTool"), "new", vec![])?;
        set(it, "mt", mt.clone());

        // don't extract duplicates by default unless set by UserDefined::Options
        // $mt->Options(Duplicates => 0) unless %Image::ExifTool::UserDefined::Options
        //     and defined $Image::ExifTool::UserDefined::Options{Duplicates};
        let user_opts = pl::pkg_hash(it, "Image::ExifTool::UserDefined::Options");
        if !(!user_opts.is_empty() && !user_opts.get("Duplicates").is_undef()) {
            options(it, vec![s("Duplicates"), n(0)])?;
        }

        // default is to join lists if the List option was set to zero in the config file
        // $joinLists = 1 if defined $mt->Options('List') and not $mt->Options('List');
        if !options(it, vec![s("List")])?.is_undef() && !options(it, vec![s("List")])?.truthy() {
            set(it, "joinLists", n(1));
        }

        // preserve FileCreateDate if possible
        if !t(it, "preserveTime") && pl::str_eq(&os, "MSWin32") && require_ok(it, "Win32::API")? && require_ok(it, "Win32API::File")? {
            set(it, "preserveTime", n(2));
        }

        // add user-defined command-line arguments
        let user_args = pl::pkg_array(it, "Image::ExifTool::UserDefined::Arguments");
        if !user_args.is_empty() {
            // unshift @ARGV, @Image::ExifTool::UserDefined::Arguments;
            let items = user_args.items();
            let a = argv(it);
            a.borrow_mut().splice(0..0, items);
        }

        // if ($version ne $Image::ExifTool::VERSION) {
        let version = sv(it, "version");
        let lib_version = it.get_scalar("Image::ExifTool::VERSION");
        if version.to_bytes() != lib_version.to_bytes() {
            warn_(it, cat(&[&"Application version ", &version, &" does not match Image::ExifTool library version ", &lib_version, &"\n"]))?;
        }

        // parse command-line options in 2 passes...
        // pass 1: set all of our ExifTool options
        // pass 2: print all of our help and informational output (-list, -ver, etc)
        let execute = re(r"^(-|\xe2\x88\x92)execute(\d+)?$", "i");
        let dash = re(r"^(-|\xe2\x88\x92)", "");
        'args: loop {
            // execute the command now if no more arguments or -execute is used
            // if (not @ARGV or ($ARGV[0] =~ /^(-|\xe2\x88\x92)execute(\d+)?$/i and not $endOfOpts)) {
            let first = argv(it).borrow().first().cloned();
            let exec_caps = match &first {
                None => None,
                Some(a0) => match execute.m(a0) {
                    Some(c) if !t(it, "endOfOpts") => Some(c),
                    _ => None,
                },
            };
            if first.is_none() || exec_caps.is_some() {
                if let Some(c) = &exec_caps {
                    // $executeID = $2;        # save -execute number for "{ready}" response
                    set(it, "executeID", cap(c, 2));
                    // $helped = 1;            # don't show help if we used -execute
                    set(it, "helped", n(1));
                    // $badCmd and shift, $rtnVal=1, next Command;
                    if t(it, "badCmd") {
                        shift_argv(it);
                        set(it, "rtnVal", n(1));
                        continue 'command;
                    }
                } else if pl::ge(&sv(it, "stayOpen"), &n(2)) {
                    // ReadStayOpen(\@ARGV);   # read more arguments from -stay_open file
                    let a = pl::pkg_array(it, "main::ARGV");
                    call_void(it, "main::ReadStayOpen", vec![a])?;
                    continue 'args;
                } else if t(it, "badCmd") {
                    // undef @commonArgs;      # all done.  Flush common arguments
                    av(it, "commonArgs").borrow_mut().clear();
                    set(it, "rtnVal", n(1));
                    continue 'command;
                }
                if pl::num_eq(&sv(it, "pass"), &n(0)) {
                    // insert common arguments now if not done already
                    if has(it, "commonArgs") && sv(it, "argsLeft").is_undef() {
                        // count the number of arguments remaining for subsequent commands
                        // $argsLeft = scalar(@ARGV) + scalar(@moreArgs);
                        let left = argv(it).borrow().len() + av(it, "moreArgs").borrow().len();
                        set(it, "argsLeft", Value::from(left));
                        // unshift @ARGV, @commonArgs;
                        let common = av(it, "commonArgs").borrow().clone();
                        argv(it).borrow_mut().splice(0..0, common);
                        // all done with commonArgs if this is the end of the command
                        // undef @commonArgs unless $argsLeft;
                        if left == 0 {
                            av(it, "commonArgs").borrow_mut().clear();
                        }
                        continue 'args;
                    }
                    // check if we have more arguments now than we did before we processed
                    // the common arguments.  If so, then we have an infinite processing loop
                    let args_left = sv(it, "argsLeft");
                    let total = |it: &mut Interp| n((argv(it).borrow().len() + av(it, "moreArgs").borrow().len()) as i64);
                    if !args_left.is_undef() && pl::lt(&args_left, &total(it)) {
                        let a0 = argv(it).borrow().first().cloned().unwrap_or_default();
                        // (perl warns here if @ARGV is empty)
                        warn_(it, cat(&[&"Ignoring -common_args from ", &a0, &" onwards to avoid infinite recursion\n"]))?;
                        while pl::lt(&args_left, &total(it)) {
                            // @ARGV and shift(@ARGV), next;
                            if !argv(it).borrow().is_empty() {
                                shift_argv(it);
                                continue;
                            }
                            // shift @moreArgs;
                            let more = av(it, "moreArgs");
                            let mut more = more.borrow_mut();
                            if !more.is_empty() {
                                more.remove(0);
                            }
                        }
                    }
                    // require MWG module if used in any argument
                    // (note: doesn't cover the -p option because these tags will be parsed on the 2nd pass)
                    // $useMWG = 1 if not $useMWG and grep /^([--_0-9A-Z]+:)*1?mwg:/i, @tags, @requestTags;
                    if !t(it, "useMWG") {
                        let mwg = re(r"^([--_0-9A-Z]+:)*1?mwg:", "i");
                        let mut all = av(it, "tags").borrow().clone();
                        all.extend(av(it, "requestTags").borrow().iter().cloned());
                        if all.iter().any(|v| mwg.is_match(v)) {
                            set(it, "useMWG", n(1));
                        }
                    }
                    if t(it, "useMWG") {
                        // require Image::ExifTool::MWG;
                        perl(it, "require Image::ExifTool::MWG", &[], Want::Void)?;
                        // Image::ExifTool::MWG::Load();
                        call_void(it, "Image::ExifTool::MWG::Load", vec![])?;
                    }
                    // update necessary variables for 2nd pass
                    if !sv(it, "forcePrint").is_undef() {
                        if options(it, vec![s("MissingTagValue")])?.is_undef() {
                            options(it, vec![s("MissingTagValue"), s("-")])?;
                        }
                        let v = options(it, vec![s("MissingTagValue")])?;
                        set(it, "forcePrint", v);
                    }
                }
                if has(it, "nextPass") {
                    // process arguments which were deferred to the next pass
                    // unshift @ARGV, @nextPass;
                    let next_pass = av(it, "nextPass").borrow().clone();
                    argv(it).borrow_mut().splice(0..0, next_pass);
                    // undef @nextPass;
                    av(it, "nextPass").borrow_mut().clear();
                    // undef $endOfOpts;
                    set(it, "endOfOpts", Value::Undef);
                    // ++$pass;
                    let p = pl::add(&sv(it, "pass"), &n(1));
                    set(it, "pass", p);
                    continue 'args;
                }
                // @ARGV and shift;    # remove -execute from argument list
                if !argv(it).borrow().is_empty() {
                    shift_argv(it);
                }
                // last;               # process the command now
                break 'args;
            }
            // $_ = shift;
            let arg = shift_argv(it);
            set_topic(it, arg);
            // next if $badCmd;      # flush remaining arguments if aborting this command
            if t(it, "badCmd") {
                continue 'args;
            }

            // allow funny dashes (nroff dash bug for cut-n-paste from pod)
            // if (not $endOfOpts and s/^(-|\xe2\x88\x92)//) {
            if !t(it, "endOfOpts") {
                let mut topic_v = topic(it);
                let before = topic_v.to_bytes().to_vec();
                if dash.subst(&mut topic_v, false, |_| Vec::new()) > 0 {
                    set_topic(it, topic_v);
                    // the substitution's match: the chain's last branch
                    // reads its $1 (the dash)
                    let n = if before.first() == Some(&b'-') { 1 } else { 3 };
                    it.last_match = Some(Rc::new(crate::interp::eval::LastMatch { subject: Rc::from(before.as_slice()), spans: vec![Some((0, n)), Some((0, n))], names: Vec::new(), utf8: false }));
                    super::options::parse_option(it)?;
                    continue 'args;
                }
            }
            argument(it)?;
        }

        process_command(it)?;
    }
    Ok(())
}

/// `eval { require MODULE }` in boolean context.
fn require_ok(it: &mut Interp, module: &str) -> ER<bool> {
    let r = pl::eval_block(it, |it| perl1(it, &format!("require {module}"), &[]))?;
    Ok(r.is_some_and(|v| v.truthy()))
}

/// The end of the argument loop (lines 1449-1470): an argument that is not
/// an option, a file name.
fn argument(it: &mut Interp) -> ER<()> {
    let arg = topic(it);
    // unless ($pass) {
    if !t(it, "pass") {
        // defer to next pass so the filename charset is available
        // push @nextPass, $_;
        av(it, "nextPass").borrow_mut().push(arg);
        // next;
        return Ok(());
    }
    // if ($doGlob and HasWildcards($_)) {
    if t(it, "doGlob") && pl::call(it, "main::HasWildcards", vec![arg.clone()])?.truthy() {
        let os = it.get_scalar("main::^O");
        if pl::str_eq(&os, "MSWin32") && require_ok(it, "Win32::FindFile")? {
            // push @files, FindFileWindows($mt, $_);
            let mt = sv(it, "mt");
            let found = pl::call_list(it, "main::FindFileWindows", vec![mt, arg])?;
            av(it, "files").borrow_mut().extend(found);
        } else {
            // glob each filespec if necessary - MK/20061010
            // push @files, File::Glob::bsd_glob($_);
            let found = pl::call_list(it, "File::Glob::bsd_glob", vec![arg])?;
            av(it, "files").borrow_mut().extend(found);
        }
        // $doGlob = 2;
        set(it, "doGlob", n(2));
    } else {
        // push @files, $_;
        av(it, "files").borrow_mut().push(arg.clone());
        // $srcStdin = 1 if $_ eq '-';
        if pl::str_eq(&arg, "-") {
            set(it, "srcStdin", n(1));
        }
    }
    Ok(())
}

/// The processing of the command once its arguments are read (lines
/// 1472-2100); returning early is the script's `next` (`next Command`).
fn process_command(it: &mut Interp) -> ER<()> {
    let mt = sv(it, "mt");

    // set "OK" UserParam based on result of last command
    // $mt->Options(UserParam => 'OK=' . (not $rtnValPrev));
    let ok = if sv(it, "rtnValPrev").truthy() { s("") } else { n(1) };
    options(it, vec![s("UserParam"), cat(&[&"OK=", &ok])])?;

    // set verbose output to STDERR if output could be to console
    // $vout = \*STDERR if $srcStdin and ($isWriting or @newValues);
    if t(it, "srcStdin") && (t(it, "isWriting") || has(it, "newValues")) {
        let stderr = glob(it, "STDERR")?;
        set(it, "vout", stderr);
    }
    // $mt->Options(TextOut => $vout) if $vout eq \*STDERR;
    let vout = sv(it, "vout");
    if perl1(it, "$v eq \\*STDERR", &[("v", vout.clone())])?.truthy() {
        options(it, vec![s("TextOut"), vout])?;
    }

    // change default EXIF string encoding if MWG used
    if t(it, "useMWG") && options(it, vec![s("CharsetEXIF")])?.is_undef() {
        options(it, vec![s("CharsetEXIF"), s("UTF8")])?;
    }

    // allow geolocation without input file if set to a position
    if !has(it, "files") && !t(it, "outOpt") && !has(it, "newValues") {
        // my $loc = $mt->Options('Geolocation');
        let loc = options(it, vec![s("Geolocation")])?;
        // use undocumented feature to input JSON file directly from command line
        // $loc and $loc ne '1' and push(@files, qq(\@JSON:{})), $geoOnly = 1;
        if loc.truthy() && !pl::str_eq(&loc, "1") {
            av(it, "files").borrow_mut().push(s("@JSON:{}"));
            set(it, "geoOnly", n(1));
        }
    }

    // print help
    // unless ((@tags and not $outOpt) or @files or @newValues or $geoOnly) {
    if !((has(it, "tags") && !t(it, "outOpt")) || has(it, "files") || has(it, "newValues") || t(it, "geoOnly")) {
        let do_glob = sv(it, "doGlob");
        if do_glob.truthy() && pl::num_eq(&do_glob, &n(2)) {
            error(it, s("No matching files\n"))?;
            return Ok(());
        }
        // $outOpt and Error("Nothing to write\n"), next;
        if t(it, "outOpt") {
            error(it, s("Nothing to write\n"))?;
            return Ok(());
        }
        if !t(it, "helped") {
            // catch warnings if we have problems running perldoc
            // local $SIG{'__WARN__'} = sub { $evalWarning = $_[0] };
            pl::local_sig_warn(it, eval_warning_handler(), |it| {
                // my $dummy = \*SAVEERR;  # avoid "used only once" warning
                glob(it, "SAVEERR")?;
                let os = it.get_scalar("main::^O");
                if !pl::str_eq(&os, "os2") {
                    perl(it, "open SAVEERR, \">&STDERR\"", &[], Want::Void)?;
                    perl(it, "open STDERR, '>/dev/null'", &[], Want::Void)?;
                }
                // if (system('perldoc',$0)) {
                let zero = it.get_scalar("main::0");
                if perl1(it, "system('perldoc',$f)", &[("f", zero)])?.truthy() {
                    pl::print_to(it, None, &[s("Syntax:  exiftool [OPTIONS] FILE\n\n")])?;
                    pl::print_to(it, None, &[s("Consult the exiftool documentation for a full list of options.\n")])?;
                }
                if !pl::str_eq(&os, "os2") {
                    perl(it, "close STDERR", &[], Want::Void)?;
                    perl(it, "open STDERR, '>&SAVEERR'", &[], Want::Void)?;
                }
                Ok(())
            })?;
        }
        return Ok(());
    }

    // do sanity check on -delete_original and -restore_original
    let delete_orig = sv(it, "deleteOrig");
    if !delete_orig.is_undef() && (has(it, "newValues") || has(it, "tags")) {
        if !has(it, "newValues") {
            // my $verb = $deleteOrig ? 'deleting' : 'restoring from';
            let verb = if delete_orig.truthy() { "deleting" } else { "restoring from" };
            error(it, cat(&[&"Can't specify tags when ", &verb, &" originals\n"]))?;
        } else if delete_orig.truthy() {
            error(it, s("Can't use -delete_original when writing.\n"))?;
            error(it, s("Maybe you meant -overwrite_original ?\n"))?;
        } else {
            error(it, s("It makes no sense to use -restore_original when writing\n"))?;
        }
        return Ok(());
    }

    // if ($overwriteOrig > 1 and $outOpt) {
    if pl::gt(&sv(it, "overwriteOrig"), &n(1)) && t(it, "outOpt") {
        error(it, s("Can't overwrite in place when -o option is used\n"))?;
        return Ok(());
    }

    // if (($tagOut or defined $diff) and ($csv or $json or %printFmt or $tabFormat or $xml or $plot or
    //     ($verbose and $html)))
    if (t(it, "tagOut") || !sv(it, "diff").is_undef())
        && (t(it, "csv") || t(it, "json") || hhas(it, "printFmt") || t(it, "tabFormat") || t(it, "xml") || t(it, "plot") || (t(it, "verbose") && t(it, "html")))
    {
        // my $opt = $tagOut ? '-W' : '-diff';
        let opt = if t(it, "tagOut") { "-W" } else { "-diff" };
        error(it, cat(&[&"Sorry, ", &opt, &" may not be combined with -csv, -htmlDump, -j, -p, -t or -X\n"]))?;
        return Ok(());
    }

    // if ($csv and $csv eq 'CSV' and not $isWriting) {
    let csv = sv(it, "csv");
    if csv.truthy() && pl::str_eq(&csv, "CSV") && !t(it, "isWriting") {
        set(it, "json", n(0)); // (not compatible)
        if t(it, "textOut") {
            let text_out = sv(it, "textOut");
            set(it, "textOut2", text_out);
            set(it, "textOut", Value::Undef);
        }
        if t(it, "binaryOutput") {
            set(it, "binaryOutput", n(0));
            // $setCharset = 'default' unless defined $setCharset;
            if sv(it, "setCharset").is_undef() {
                set(it, "setCharset", s("default"));
            }
        }
        if hhas(it, "printFmt") {
            warn_(it, s("The -csv option has no effect when -p is used\n"))?;
            set(it, "csv", Value::Undef);
        }
        // require Image::ExifTool::XMP if $setCharset;
        if t(it, "setCharset") {
            perl(it, "require Image::ExifTool::XMP", &[], Want::Void)?;
        }
    }
    if t(it, "plot") && t(it, "textOut") {
        let text_out = sv(it, "textOut");
        set(it, "textOut2", text_out);
        set(it, "textOut", Value::Undef);
    }
    if t(it, "textOut2") {
        let text_out2 = sv(it, "textOut2");
        // if ($textOverwrite > 1) {
        // (perl warns here if $textOverwrite is undef)
        if pl::gt(&sv(it, "textOverwrite"), &n(1)) {
            error(it, s("Can not append to multi-file output format\n"))?;
            set(it, "textOut2", Value::Undef);
            return Ok(());
        }
        // if (not $textOverwrite and $mt->Exists($textOut2, 1)) {
        if !t(it, "textOverwrite") && pl::method(it, &mt, "Exists", vec![text_out2.clone(), n(1)])?.truthy() {
            error(it, cat(&[&"Output file ", &text_out2, &" already exists\n"]))?;
            set(it, "textOut2", Value::Undef);
            return Ok(());
        }
        // make sure we can write the output text file before processing all input files
        // CreateDirectory($textOut2);  # create directory if necessary
        call_void(it, "main::CreateDirectory", vec![text_out2.clone()])?;
        // if ($mt->Open(\*OUTFILE, $textOut2, '>')) {
        let outfile = glob(it, "OUTFILE")?;
        if pl::method(it, &mt, "Open", vec![outfile.clone(), text_out2.clone(), s(">")])?.truthy() {
            // close(\*OUTFILE);
            perl(it, "close($f)", &[("f", outfile)], Want::Void)?;
            // unlink($textOut2);  # (this was just a test)
            perl(it, "unlink($f)", &[("f", text_out2)], Want::Void)?;
        } else {
            error(it, cat(&[&"Error creating ", &text_out2, &"\n"]))?;
            set(it, "textOut2", Value::Undef);
            return Ok(());
        }
    }

    if t(it, "escapeHTML") || t(it, "json") {
        // must be UTF8 for HTML conversion and JSON output
        // $mt->Options(Charset => 'UTF8') if $json;
        if t(it, "json") {
            options(it, vec![s("Charset"), s("UTF8")])?;
        }
        // use Escape option to do our HTML escaping unless XML output
        // $mt->Options(Escape => 'HTML') if $escapeHTML and not $xml;
        if t(it, "escapeHTML") && !t(it, "xml") {
            options(it, vec![s("Escape"), s("HTML")])?;
        }
    } else if t(it, "escapeXML") && !t(it, "xml") {
        options(it, vec![s("Escape"), s("XML")])?;
    }

    // set sort option
    if t(it, "sortOpt") {
        // (note that -csv sorts alphabetically by default anyway if more than 1 file)
        // my $sort = ($outFormat > 0 or $xml or $json or $csv or $plot) ? 'Tag' : 'Descr';
        let sort = if pl::gt(&sv(it, "outFormat"), &n(0)) || t(it, "xml") || t(it, "json") || t(it, "csv") || t(it, "plot") { "Tag" } else { "Descr" };
        options(it, vec![s("Sort"), s(sort), s("Sort2"), s(sort)])?;
    }

    // set $structOpt in case set by API option
    if options(it, vec![s("Struct")])?.truthy() && !t(it, "structOpt") {
        let st = options(it, vec![s("Struct")])?;
        set(it, "structOpt", st);
        // require 'Image/ExifTool/XMPStruct.pl';
        perl(it, "require 'Image/ExifTool/XMPStruct.pl'", &[], Want::Void)?;
    }

    // set up for RDF/XML, JSON and PHP output formats
    if t(it, "plot") {
        set(it, "joinLists", Value::Undef);
        options(it, vec![s("List"), n(1)])?;
        // $plot->Settings($mt->Options('Plot'));
        let plot = sv(it, "plot");
        let settings = pl::method_list(it, &mt, "Options", vec![s("Plot")])?;
        let mut args = vec![plot.clone()];
        args.extend(settings);
        it.call_method(&plot, "Settings", args, Want::Void)?;
    } else if t(it, "xml") {
        // require Image::ExifTool::XMP;   # for EscapeXML()
        perl(it, "require Image::ExifTool::XMP", &[], Want::Void)?;
        // my $charset = $mt->Options('Charset');
        let mut charset = options(it, vec![s("Charset")])?;
        // standard XML encoding names for supported Charset settings
        // (ref http://www.iana.org/assignments/character-sets)
        let encoding = |c: &Value| -> Option<&'static str> {
            Some(match &*c.to_bytes() {
                b"UTF8" => "UTF-8",
                b"Latin" => "windows-1252",
                b"Latin2" => "windows-1250",
                b"Cyrillic" => "windows-1251",
                b"Greek" => "windows-1253",
                b"Turkish" => "windows-1254",
                b"Hebrew" => "windows-1255",
                b"Arabic" => "windows-1256",
                b"Baltic" => "windows-1257",
                b"Vietnam" => "windows-1258",
                b"MacRoman" => "macintosh",
                _ => return None,
            })
        };
        // switch to UTF-8 if we don't have a standard encoding name
        // (perl warns here if $charset is undef)
        if encoding(&charset).is_none() {
            charset = s("UTF8");
            options(it, vec![s("Charset"), charset.clone()])?;
        }
        // set file header/trailer for XML output
        let enc = encoding(&charset).unwrap_or("");
        set(
            it,
            "fileHeader",
            cat(&[&"<?xml version='1.0' encoding='", &enc, &"'?>\n", &"<rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'>\n"]),
        );
        set(it, "fileTrailer", s("</rdf:RDF>\n"));
        // extract as a list unless short output format
        // $joinLists = 1 if $outFormat > 0;
        if pl::gt(&sv(it, "outFormat"), &n(0)) {
            set(it, "joinLists", n(1));
        }
        // $mt->Options(List => 1) unless $joinLists;
        if !t(it, "joinLists") {
            options(it, vec![s("List"), n(1)])?;
        }
        // $showGroup = $allGroup = 1;         # always show group 1
        set(it, "allGroup", n(1));
        set(it, "showGroup", n(1));
        // set binaryOutput flag to 0 or undef (0 = output encoded binary in XML)
        // $binaryOutput = ($outFormat > 0 ? undef : 0) if $binaryOutput;
        if t(it, "binaryOutput") {
            let v = if pl::gt(&sv(it, "outFormat"), &n(0)) { Value::Undef } else { n(0) };
            set(it, "binaryOutput", v);
        }
        // $showTagID = 'D' if $tabFormat and not $showTagID;
        if t(it, "tabFormat") && !t(it, "showTagID") {
            set(it, "showTagID", s("D"));
        }
    } else if t(it, "json") {
        if pl::num_eq(&sv(it, "json"), &n(1)) {
            // JSON
            set(it, "fileHeader", s("["));
            set(it, "fileTrailer", s("]\n"));
        } else {
            // PHP
            set(it, "fileHeader", s("Array("));
            set(it, "fileTrailer", s(");\n"));
        }
        // allow binary output in a text-mode file when -php/-json and -b used together
        // (this works because PHP strings are simple arrays of bytes, and CR/LF
        //  won't be messed up in the text mode output because they are converted
        //  to escape sequences in the strings)
        if t(it, "binaryOutput") {
            set(it, "binaryOutput", n(0));
            // require Image::ExifTool::XMP if $json == 1;  # (for EncodeBase64)
            if pl::num_eq(&sv(it, "json"), &n(1)) {
                perl(it, "require Image::ExifTool::XMP", &[], Want::Void)?;
            }
        }
        // $mt->Options(List => 1) unless $joinLists;
        if !t(it, "joinLists") {
            options(it, vec![s("List"), n(1)])?;
        }
        // $showTagID = 'D' if $tabFormat and not $showTagID;
        if t(it, "tabFormat") && !t(it, "showTagID") {
            set(it, "showTagID", s("D"));
        }
    } else if t(it, "structOpt") {
        options(it, vec![s("List"), n(1)])?;
    } else {
        set(it, "joinLists", n(1)); // join lists for all other unstructured output formats
    }

    if t(it, "argFormat") {
        set(it, "outFormat", n(3));
        // $allGroup = 1 if defined $showGroup;
        if !sv(it, "showGroup").is_undef() {
            set(it, "allGroup", n(1));
        }
    }

    // change to forward slashes if necessary in all filenames (like CleanFilename)
    if pl::call(it, "Image::ExifTool::IsPC", vec![])?.truthy() {
        // tr/\\/\// foreach @files;
        let files = av(it, "files");
        for f in files.borrow_mut().iter_mut() {
            pl::tr(f, "\\\\", "/", "");
        }
    }

    // can't do anything if no file specified
    if !has(it, "files") {
        if !t(it, "outOpt") {
            let do_glob = sv(it, "doGlob");
            if do_glob.truthy() && pl::num_eq(&do_glob, &n(2)) {
                error(it, s("No matching files\n"))?;
            } else {
                error(it, s("No file specified\n"))?;
            }
            return Ok(());
        }
        // push @files, '';    # create file from nothing
        av(it, "files").borrow_mut().push(s(""));
    }

    // set Verbose and HtmlDump options
    let verbose = sv(it, "verbose");
    if verbose.truthy() {
        // $disableOutput = 1 unless @tags or @exclude or $tagOut;
        if !(has(it, "tags") || has(it, "exclude") || t(it, "tagOut")) {
            set(it, "disableOutput", n(1));
        }
        // undef $binaryOutput unless $tagOut;    # disable conflicting option
        if !t(it, "tagOut") {
            set(it, "binaryOutput", Value::Undef);
        }
        if t(it, "html") {
            set(it, "html", n(2)); // flag for html dump
            options(it, vec![s("HtmlDump"), verbose])?;
        } else if !t(it, "tagOut") {
            // $mt->Options(Verbose => $verbose) unless $tagOut;
            options(it, vec![s("Verbose"), verbose])?;
        }
    } else if !verbose.is_undef() {
        // auto-flush output when -v0 is used
        perl(it, "require FileHandle; STDOUT->autoflush(1); STDERR->autoflush(1)", &[], Want::Void)?;
    }

    // validate all tags we're writing
    // my $needSave = 1;
    let mut need_save = true;
    if has(it, "newValues") {
        if !new_values(it, &mt, &mut need_save)? {
            return Ok(());
        }
    } else {
        // } elsif (grep /^(\*:)?\*$/, @exclude) {
        let all = re(r"^(\*:)?\*$", "");
        let excl = av(it, "exclude").borrow().clone();
        if excl.iter().any(|e| all.is_match(e)) {
            error(it, s("All tags excluded -- nothing to do.\n"))?;
            return Ok(());
        }
    }
    if t(it, "isWriting") {
        if !sv(it, "diff").is_undef() {
            error(it, s("Can't use -diff option when writing tags\n"))?;
            return Ok(());
        } else if t(it, "plot") {
            error(it, s("Can't use -plot option when writing tags\n"))?;
            return Ok(());
        } else if has(it, "tags") && !t(it, "outOpt") && !t(it, "csv") {
            // my ($tg, $s) = @tags > 1 ? ("$tags[0] ...", 's') : ($tags[0], '');
            let tags = av(it, "tags").borrow().clone();
            let (tg, s_) = if tags.len() > 1 { (cat(&[&tags[0], &" ..."]), "s") } else { (tags[0].clone(), "") };
            warn_(it, cat(&[&"Ignored superfluous tag name", &s_, &" or invalid option", &s_, &": -", &tg, &"\n"]))?;
        }
    }
    // save current state of new values if setting values from target file
    // or if we may be translating to a different format
    // $mt->SaveNewValues() if $outOpt or (@dynamicFiles and $needSave);
    if t(it, "outOpt") || (has(it, "dynamicFiles") && need_save) {
        it.call_method(&mt, "SaveNewValues", vec![mt.clone()], Want::Void)?;
    }

    // $multiFile = 1 if @files > 1;
    if av(it, "files").borrow().len() > 1 {
        set(it, "multiFile", n(1));
    }
    // @exclude and $mt->Options(Exclude => \@exclude);
    if has(it, "exclude") {
        let excl = super::lex_array(it, "exclude");
        options(it, vec![s("Exclude"), excl])?;
    }

    // undef $binaryOutput if $html;
    if t(it, "html") {
        set(it, "binaryOutput", Value::Undef);
    }

    if t(it, "binaryOutput") {
        set(it, "outFormat", n(99)); // shortest possible output format
        options(it, vec![s("PrintConv"), n(0)])?;
        if !(t(it, "textOut") || t(it, "binaryStdout")) {
            // binmode(STDOUT);
            perl(it, "binmode(STDOUT)", &[], Want::Void)?;
            set(it, "binaryStdout", n(1));
            // $mt->Options(TextOut => ($vout = \*STDERR));
            let stderr = glob(it, "STDERR")?;
            set(it, "vout", stderr.clone());
            options(it, vec![s("TextOut"), stderr])?;
        }
        // disable conflicting options
        set(it, "showGroup", Value::Undef);
    }

    // sort by groups to look nicer depending on options
    // if (defined $showGroup and not (@tags and ($allGroup or $csv)) and ($sortOpt or not defined $sortOpt)) {
    let show_group = sv(it, "showGroup");
    let sort_opt = sv(it, "sortOpt");
    if !show_group.is_undef() && !(has(it, "tags") && (t(it, "allGroup") || t(it, "csv"))) && (sort_opt.truthy() || sort_opt.is_undef()) {
        options(it, vec![s("Sort"), cat(&[&"Group", &show_group])])?;
    }

    if t(it, "textOut") {
        // CleanFilename($textOut);  # make all forward slashes
        let mut args = [sv(it, "textOut")];
        pl::call_mut(it, "main::CleanFilename", &mut args, Want::Void)?;
        let [text_out] = args;
        set(it, "textOut", text_out.clone());
        // add '.' before output extension if necessary
        // $textOut = ".$textOut" unless $textOut =~ /[.%]/ or defined $tagOut;
        if !(re(r"[.%]", "").is_match(&text_out) || !sv(it, "tagOut").is_undef()) {
            set(it, "textOut", cat(&[&".", &text_out]));
        }
    }

    // determine if we should scan for only writable files
    let out_opt = sv(it, "outOpt");
    if out_opt.truthy() {
        // my $type = GetFileType($outOpt);
        let mut type_ = pl::call(it, "main::GetFileType", vec![out_opt.clone()])?;
        if type_.truthy() {
            // (must test original file name because we can write .webp but not other RIFF types)
            // my $canWrite = CanWrite($outOpt);
            let can_write = pl::call(it, "main::CanWrite", vec![out_opt.clone()])?;
            if !can_write.truthy() {
                if !can_write.is_undef() && pl::str_eq(&can_write, "") {
                    type_ = pl::call(it, "Image::ExifTool::GetFileExtension", vec![out_opt.clone()])?;
                    // $type = uc($outOpt) unless defined $type;
                    if type_.is_undef() {
                        type_ = pl::uc(&out_opt);
                    }
                }
                error(it, cat(&[&"Can't write ", &type_, &" files\n"]))?;
                return Ok(());
            }
            // $scanWritable = $type unless CanCreate($type);
            if !pl::call(it, "main::CanCreate", vec![type_.clone()])?.truthy() {
                set(it, "scanWritable", type_);
            }
        } else {
            set(it, "scanWritable", n(1));
        }
        set(it, "isWriting", n(1)); // set writing flag
    } else if t(it, "isWriting") || !sv(it, "deleteOrig").is_undef() {
        set(it, "scanWritable", n(1));
    }

    // initialize alternate encoding flag
    // $altEnc = $mt->Options('Charset');
    let alt_enc = options(it, vec![s("Charset")])?;
    set(it, "altEnc", alt_enc.clone());
    // undef $altEnc if $altEnc eq 'UTF8';
    if pl::str_eq(&alt_enc, "UTF8") {
        set(it, "altEnc", Value::Undef);
    }

    // set flag to fix description lengths if necessary
    // if (not $altEnc and $mt->Options('Lang') ne 'en') {
    if !t(it, "altEnc") && !pl::str_eq(&options(it, vec![s("Lang")])?, "en") {
        // (note that Unicode::GCString is part of the Unicode::LineBreak package)
        // $fixLen = eval { require Unicode::GCString } ? 2 : 1;
        let fix_len = if require_ok(it, "Unicode::GCString")? { 2 } else { 1 };
        set(it, "fixLen", n(fix_len));
    }

    // sort input files if specified
    if has(it, "fileOrder") {
        sort_files(it, &mt)?;
    } else if !sv(it, "progress").is_undef() {
        // expand FILE argument to count the number of files to process
        // my @allFiles;
        let all_files = Value::new_array(vec![]);
        // ProcessFiles($mt, \@allFiles);
        call_void(it, "main::ProcessFiles", vec![mt.clone(), all_files.clone()])?;
        // @files = @allFiles;
        *av(it, "files").borrow_mut() = all_files.items();
    }
    // set file count for progress message
    // $progressMax = scalar @files if defined $progress;
    if !sv(it, "progress").is_undef() {
        let max = av(it, "files").borrow().len();
        set(it, "progressMax", Value::from(max));
    }

    // store duplicate database information under absolute path and with standardized case
    database_keys(it)?;

    // process all specified files
    // ProcessFiles($mt);
    call_void(it, "main::ProcessFiles", vec![mt.clone()])?;

    // Error "No file with specified extension\n" if $filtered and not $validFile;
    if t(it, "filtered") && !t(it, "validFile") {
        error(it, s("No file with specified extension\n"))?;
    }

    // print folder/file trailer if necessary
    if t(it, "textOut") {
        // foreach (keys %outTrailer) {
        let out_trailer = hv(it, "outTrailer");
        let keys: Vec<Value> = out_trailer.borrow().keys().map(|k| Value::bytes(&**k)).collect();
        for k in keys {
            // next unless $outTrailer{$_};
            let trailer = out_trailer.borrow().get(&k.to_bytes()).cloned().unwrap_or_default();
            if !trailer.truthy() {
                continue;
            }
            // if ($mt->Open(\*OUTTRAIL, $_, '>>')) {
            let outtrail = glob(it, "OUTTRAIL")?;
            if pl::method(it, &mt, "Open", vec![outtrail.clone(), k.clone(), s(">>")])?.truthy() {
                // my $fp = \*OUTTRAIL;
                // print $fp $outTrailer{$_};
                pl::print_to(it, Some(&outtrail), &[trailer])?;
                // close $fp;
                perl(it, "close $fp", &[("fp", outtrail)], Want::Void)?;
            } else {
                error(it, cat(&[&"Error appending to ", &k, &"\n"]))?;
            }
        }
    } else {
        // print $sectTrailer if $sectTrailer;
        let sect_trailer = sv(it, "sectTrailer");
        if sect_trailer.truthy() {
            pl::print_to(it, None, &[sect_trailer])?;
        }
        // print $fileTrailer if $fileTrailer and not $fileHeader;
        let file_trailer = sv(it, "fileTrailer");
        if file_trailer.truthy() && !t(it, "fileHeader") {
            pl::print_to(it, None, &[file_trailer])?;
        }
        // print CSV or SVG output file if necessary
        // my ($fp, $err);
        let (mut fp, mut err) = (Value::Undef, false);
        if t(it, "textOut2") {
            let text_out2 = sv(it, "textOut2");
            let outfile = glob(it, "OUTFILE")?;
            if pl::method(it, &mt, "Open", vec![outfile.clone(), text_out2.clone(), s(">")])?.truthy() {
                fp = outfile;
            } else {
                error(it, cat(&[&"Error creating ", &text_out2, &"\n"]))?;
                err = true;
            }
        }
        if !err {
            // PrintCSV($fp) if $csv and not $isWriting;
            if t(it, "csv") && !t(it, "isWriting") {
                call_void(it, "main::PrintCSV", vec![fp.clone()])?;
            }
            // print SVG plot
            if t(it, "plot") {
                let plot = sv(it, "plot");
                // $plot->Draw($fp || \*STDOUT);
                let out = if fp.truthy() { fp.clone() } else { glob(it, "STDOUT")? };
                it.call_method(&plot, "Draw", vec![plot.clone(), out], Want::Void)?;
                let plot_err = plot.get("Error");
                let plot_warn = plot.get("Warn");
                if plot_err.truthy() {
                    error(it, cat(&[&"Error: ", &plot_err, &"\n"]))?;
                    err = true;
                } else if plot_warn.truthy() {
                    warn_(it, cat(&[&"Warning: ", &plot_warn, &"\n"]))?;
                }
            }
        }
        if fp.truthy() {
            // close($fp) or $err = 1;
            if !perl1(it, "close($fp)", &[("fp", fp)])?.truthy() {
                err = true;
            }
            let text_out2 = sv(it, "textOut2");
            if err {
                it.call_method(&mt, "Unlink", vec![mt.clone(), text_out2], Want::Void)?;
            } else {
                // $created{$textOut2} = 1;
                hv(it, "created").borrow_mut().insert(&text_out2.to_bytes(), n(1));
            }
        }
    }

    // my $totWr = $countGoodWr + $countBadWr + $countSameWr + $countCopyWr +
    //             $countGoodCr + $countBadCr;
    let mut tot_wr = n(0);
    for c in ["countGoodWr", "countBadWr", "countSameWr", "countCopyWr", "countGoodCr", "countBadCr"] {
        tot_wr = pl::add(&tot_wr, &sv(it, c));
    }
    let (count, count_dir, count_new_dir, count_failed) = (sv(it, "count"), sv(it, "countDir"), sv(it, "countNewDir"), sv(it, "countFailed"));

    if !sv(it, "deleteOrig").is_undef() {
        // print summary and delete requested files
        if !t(it, "quiet") {
            if count_dir.truthy() {
                printf(it, None, "%5d directories scanned\n", &[count_dir.clone()])?;
            }
            if count_new_dir.truthy() {
                printf(it, None, "%5d directories created\n", &[count_new_dir.clone()])?;
            }
            if count_failed.truthy() {
                printf(it, None, "%5d files failed condition\n", &[count_failed.clone()])?;
            }
            printf(it, None, "%5d image files found\n", &[count.clone()])?;
        }
        if has(it, "delFiles") {
            // verify deletion unless "-delete_original!" was specified
            let del_files = av(it, "delFiles").borrow().clone();
            if pl::num_eq(&sv(it, "deleteOrig"), &n(1)) {
                printf(it, None, "%5d originals will be deleted!  Are you sure [y/n]? ", &[Value::from(del_files.len())])?;
                // my $response = <STDIN>;
                let response = perl1(it, "scalar <STDIN>", &[])?;
                // (perl warns here if $response is undef)
                if !re(r"^(y|yes)\s*$", "i").is_match(&response) {
                    warn_(it, s("Originals not deleted.\n"))?;
                    return Ok(());
                }
            }
            // $countGoodWr = $mt->Unlink(@delFiles);
            let mut args = vec![mt.clone()];
            args.extend(del_files.iter().cloned());
            let good = it.call_method(&mt, "Unlink", args, Want::Scalar)?.pop().unwrap_or_default();
            set(it, "countGoodWr", good.clone());
            // $countBad = scalar(@delFiles) - $countGoodWr;
            set(it, "countBad", pl::sub(&Value::from(del_files.len()), &good));
        }
        let (count_good_wr, count_bad) = (sv(it, "countGoodWr"), sv(it, "countBad"));
        if t(it, "quiet") {
            // no more messages
        } else if count.truthy() && !count_good_wr.truthy() && !count_bad.truthy() {
            printf(it, None, "%5d original files found\n", &[count_good_wr])?; // (this will be 0)
        } else if t(it, "deleteOrig") {
            if count.truthy() {
                printf(it, None, "%5d original files deleted\n", &[count_good_wr])?;
            }
            if count_bad.truthy() {
                printf(it, None, "%5d originals not deleted due to errors\n", &[count_bad])?;
            }
        } else {
            if count.truthy() {
                printf(it, None, "%5d image files restored from original\n", &[count_good_wr])?;
            }
            if count_bad.truthy() {
                printf(it, None, "%5d files not restored due to errors\n", &[count_bad])?;
            }
        }
    } else if (!t(it, "binaryStdout") || t(it, "verbose")) && !t(it, "quiet") {
        // print summary
        // my $tot = $count + $countBad;
        let count_bad = sv(it, "countBad");
        let tot = pl::add(&count, &count_bad);
        let count_link = hv(it, "countLink");
        let has_link = !count_link.borrow().is_empty();
        if count_dir.truthy() || tot_wr.truthy() || count_failed.truthy() || pl::gt(&tot, &n(1)) || t(it, "textOut") || has_link {
            // my $o = (($html or $json or $xml or %printFmt or $csv or $plot) and not $textOut) ? \*STDERR : $vout;
            let o = if (t(it, "html") || t(it, "json") || t(it, "xml") || hhas(it, "printFmt") || t(it, "csv") || t(it, "plot")) && !t(it, "textOut") {
                glob(it, "STDERR")?
            } else {
                sv(it, "vout")
            };
            let o = Some(&o);
            let (count_good_cr, count_bad_cr, count_copy_wr) = (sv(it, "countGoodCr"), sv(it, "countBadCr"), sv(it, "countCopyWr"));
            let (count_good_wr, count_same_wr, count_bad_wr) = (sv(it, "countGoodWr"), sv(it, "countSameWr"), sv(it, "countBadWr"));
            if count_dir.truthy() {
                printf(it, o, "%5d directories scanned\n", &[count_dir.clone()])?;
            }
            if count_new_dir.truthy() {
                printf(it, o, "%5d directories created\n", &[count_new_dir.clone()])?;
            }
            if count_failed.truthy() {
                printf(it, o, "%5d files failed condition\n", &[count_failed.clone()])?;
            }
            if count_good_cr.truthy() {
                printf(it, o, "%5d image files created\n", &[count_good_cr.clone()])?;
            }
            // if $totWr - $countGoodCr - $countBadCr - $countCopyWr;
            if pl::sub(&pl::sub(&pl::sub(&tot_wr, &count_good_cr), &count_bad_cr), &count_copy_wr).truthy() {
                printf(it, o, "%5d image files updated\n", &[count_good_wr])?;
            }
            if count_same_wr.truthy() {
                printf(it, o, "%5d image files unchanged\n", &[count_same_wr])?;
            }
            if count_copy_wr.truthy() {
                let what = if t(it, "overwriteOrig") { s("moved") } else { s("copied") };
                printf(it, o, "%5d image files %s\n", &[count_copy_wr, what])?;
            }
            if count_bad_wr.truthy() {
                printf(it, o, "%5d files weren't updated due to errors\n", &[count_bad_wr])?;
            }
            if count_bad_cr.truthy() {
                printf(it, o, "%5d files weren't created due to errors\n", &[count_bad_cr])?;
            }
            // if ($tot+$countFailed)>1 or ($countDir and not $totWr);
            if pl::gt(&pl::add(&tot, &count_failed), &n(1)) || (count_dir.truthy() && !tot_wr.truthy()) {
                printf(it, o, "%5d image files read\n", &[count.clone()])?;
            }
            if count_bad.truthy() {
                printf(it, o, "%5d files could not be read\n", &[count_bad.clone()])?;
            }
            if t(it, "textOut") || t(it, "textOut2") {
                let created = hv(it, "created").borrow().len();
                printf(it, o, "%5d output files created\n", &[Value::from(created)])?;
            }
            let appended = hv(it, "appended").borrow().len();
            if appended > 0 {
                printf(it, o, "%5d output files appended\n", &[Value::from(appended)])?;
            }
            let link = |k: &str| count_link.borrow().get(k.as_bytes()).cloned().unwrap_or_default();
            if link("Hard").truthy() || link("BadHard").truthy() {
                let v = if link("Hard").truthy() { link("Hard") } else { n(0) };
                printf(it, o, "%5d hard links created\n", &[v])?;
            }
            if link("BadHard").truthy() {
                printf(it, o, "%5d hard links could not be created\n", &[link("BadHard")])?;
            }
            if link("Sym").truthy() || link("BadSym").truthy() {
                let v = if link("Sym").truthy() { link("Sym") } else { n(0) };
                printf(it, o, "%5d symbolic links created\n", &[v])?;
            }
            if link("BadSym").truthy() {
                printf(it, o, "%5d symbolic links could not be created\n", &[link("BadSym")])?;
            }
        }
    }

    // set error status if we had any errors or if all files failed the "-if" condition
    if t(it, "countBadWr") || t(it, "countBadCr") || t(it, "countBad") {
        set(it, "rtnVal", n(1));
    } else if t(it, "countFailed") && !(t(it, "count") || tot_wr.truthy()) && !t(it, "rtnVal") {
        set(it, "rtnVal", n(2));
    }

    // Image::ExifTool::Purge(0) if $purge;    # do final purging
    if t(it, "purge") {
        call_void(it, "Image::ExifTool::Purge", vec![n(0)])?;
    }

    // clean up after each command
    call_void(it, "main::Cleanup", vec![])?;
    Ok(())
}

/// The validation of the tags to write (lines 1714-1814): false for the
/// script's `next` (or `next Command`).
fn new_values(it: &mut Interp, mt: &Value, need_save: &mut bool) -> ER<bool> {
    // assume -geotime value if -geotag specified without -geotime
    let add_geotime = sv(it, "addGeotime");
    if add_geotime.truthy() {
        // AddSetTagsFile($setTagsFile = '@') unless $setTagsFile and $setTagsFile eq '@';
        let stf = sv(it, "setTagsFile");
        if !(stf.truthy() && pl::str_eq(&stf, "@")) {
            set(it, "setTagsFile", s("@"));
            call_void(it, "main::AddSetTagsFile", vec![s("@")])?;
        }
        // push @{$setTags{$setTagsFile}}, $addGeotime;
        let stf = sv(it, "setTagsFile");
        let set_tags = super::lex_hash(it, "setTags");
        set_tags.get_or_new_array(stf.to_bytes()).push(add_geotime.clone());
        // $verbose and print $vout qq(Using default "-$addGeotime"\n);
        if t(it, "verbose") {
            let vout = sv(it, "vout");
            pl::print_to(it, Some(&vout), &[cat(&[&"Using default \"-", &add_geotime, &"\"\n"])])?;
        }
    }
    // my %setTagsIndex;
    let set_tags_index = Value::new_hash();
    // add/delete option lookup
    // my %addDelOpt = ( '+' => 'AddValue', '-' => 'DelValue', "\xe2\x88\x92" => 'DelValue' );
    let add_del_opt = |k: &Value| -> Value {
        match &*k.to_bytes() {
            b"+" => s("AddValue"),
            b"-" | b"\xe2\x88\x92" => s("DelValue"),
            _ => Value::Undef,
        }
    };
    // $saveCount = 0;
    set(it, "saveCount", n(0));
    let eq_re = re(r"(.*?)=(.*)", "s");
    let all_re = re(r"\ball\b", "i");
    let caret_re = re(r"\^([-+]*)$", "");
    let tff_re = re(r"^(All)?TagsFromFile$", "i");
    let file_num_re = re(r"\bfile\d+:", "i");
    let add_del_re = re(r"([-+]|\xe2\x88\x92)$", "");
    let unesc_re = re(r"\\(x([0-9a-fA-F]{2})|.)", "s");
    let new_values = av(it, "newValues");
    // foreach (@newValues) {
    let go_on = pl::local_topic(it, |it| -> ER<bool> {
        let mut i = 0;
        loop {
            let Some(item) = new_values.borrow().get(i).cloned() else { break };
            i += 1;
            set_topic(it, item.clone());
            // if (ref $_ eq 'HASH') {
            if pl::ref_of(&item) == "HASH" {
                // save new values now if we stored a "SaveCount" marker
                let sc = item.get("SaveCount");
                if sc.truthy() {
                    // $saveCount = $mt->SaveNewValues();
                    let count = it.call_method(mt, "SaveNewValues", vec![mt.clone()], Want::Scalar)?.pop().unwrap_or_default();
                    set(it, "saveCount", count);
                    *need_save = false;
                    // insert marker to load values from CSV file now if this was the CSV file
                    // push @dynamicFiles, \$csv if $$_{SaveCount} == $dbSaveCount;
                    if pl::num_eq(&sc, &sv(it, "dbSaveCount")) {
                        let csv_ref = Value::ScalarRef(super::lex_scalar(it, "csv"));
                        av(it, "dynamicFiles").borrow_mut().push(csv_ref);
                    }
                }
                continue;
            }
            // /(.*?)=(.*)/s or next;
            let Some(c) = eq_re.m(&item) else { continue };
            // my ($tag, $newVal) = ($1, $2);
            let (mut tag, mut new_val) = (cap(&c, 1), cap(&c, 2));
            // $tag =~ s/\ball\b/\*/ig;    # replace 'all' with '*' in tag names
            all_re.subst(&mut tag, true, |_| b"*".to_vec());
            // $newVal eq '' and undef $newVal unless $tag =~ s/\^([-+]*)$/$1/;  # undefined to delete tag
            if caret_re.subst(&mut tag, false, |c| cap(c, 1).to_bytes().to_vec()) == 0 && pl::str_eq(&new_val, "") {
                new_val = Value::Undef;
            }
            if tff_re.is_match(&tag) {
                // defined $newVal or Error("Need file name for -tagsFromFile\n"), next Command;
                if new_val.is_undef() {
                    error(it, s("Need file name for -tagsFromFile\n"))?;
                    return Ok(false);
                }
                // ++$isWriting;
                let w = pl::add(&sv(it, "isWriting"), &n(1));
                set(it, "isWriting", w);
                // if ($newVal eq '@' or not defined FilenameSPrintf($newVal) or
                //     # can't set tags yet if we are using tags from other files with the -fileNUM option
                //     grep /\bfile\d+:/i, @{$setTags{$newVal}})
                let dynamic = pl::str_eq(&new_val, "@") || pl::call(it, "main::FilenameSPrintf", vec![new_val.clone()])?.is_undef() || {
                    let set_tags = super::lex_hash(it, "setTags");
                    set_tags.get_or_new_array(new_val.to_bytes()).items().iter().any(|v| file_num_re.is_match(v))
                };
                if dynamic {
                    av(it, "dynamicFiles").borrow_mut().push(new_val.clone());
                    continue; // set tags from dynamic file later
                }
                // unless ($mt->Exists($newVal) or $newVal eq '-') {
                if !(pl::method(it, mt, "Exists", vec![new_val.clone()])?.truthy() || pl::str_eq(&new_val, "-")) {
                    error(it, cat(&[&"File '", &new_val, &"' does not exist for -tagsFromFile option\n"]))?;
                    return Ok(false);
                }
                // my $setTags = $setTags{$newVal};
                let mut set_tags = super::lex_hash(it, "setTags").get(new_val.to_bytes());
                // do we have multiple -tagsFromFile options with this file?
                let list = super::lex_hash(it, "setTagsList").get(new_val.to_bytes());
                if list.truthy() {
                    // use the tags set in the i-th occurrence
                    // my $i = $setTagsIndex{$newVal} || 0;
                    let idx = set_tags_index.get(new_val.to_bytes());
                    let idx = if idx.truthy() { idx } else { n(0) };
                    // $setTagsIndex{$newVal} = $i + 1;
                    set_tags_index.set(new_val.to_bytes(), pl::add(&idx, &n(1)));
                    // $setTags = $setTagsList{$newVal}[$i] if $setTagsList{$newVal}[$i];
                    let e = list.at(idx.to_i64());
                    if e.truthy() {
                        set_tags = e;
                    }
                }
                // set specified tags from this file
                // unless (DoSetFromFile($mt, $newVal, $setTags)) {
                if !pl::call(it, "main::DoSetFromFile", vec![mt.clone(), new_val.clone(), set_tags])?.truthy() {
                    set(it, "rtnVal", n(1));
                    return Ok(false);
                }
                *need_save = true;
                continue;
            }
            // my %opts = ( Shift => 0 );  # shift values if possible instead of adding/deleting
            let opts = Value::new_hash();
            opts.set("Shift", n(0));
            // allow writing of 'Unsafe' tags unless specified by wildcard
            // $opts{Protected} = 1 unless $tag =~ /[?*]/;
            if !re(r"[?*]", "").is_match(&tag) {
                opts.set("Protected", n(1));
            }

            // if ($tag =~ s/<// and defined $newVal) {
            if re("<", "").subst(&mut tag, false, |_| Vec::new()) > 0 && !new_val.is_undef() {
                if !pl::call(it, "main::FilenameSPrintf", vec![new_val.clone()])?.is_undef() {
                    // SlurpFile($newVal, \$newVal) or next;   # read file data into $newVal
                    let cell = Rc::new(RefCell::new(new_val.clone()));
                    let ok = pl::call(it, "main::SlurpFile", vec![new_val.clone(), Value::ScalarRef(cell.clone())])?;
                    new_val = cell.borrow().clone();
                    if !ok.truthy() {
                        continue;
                    }
                } else {
                    // $tag =~ s/([-+]|\xe2\x88\x92)$// and $opts{$addDelOpt{$1}} = 1;
                    let mut which = Value::Undef;
                    if add_del_re.subst(&mut tag, false, |c| {
                        which = cap(c, 1);
                        Vec::new()
                    }) > 0
                    {
                        opts.set(add_del_opt(&which).to_bytes(), n(1));
                    }
                    // verify that this tag can be written
                    // my $result = Image::ExifTool::IsWritable($tag);
                    let result = pl::call(it, "Image::ExifTool::IsWritable", vec![tag.clone()])?;
                    if result.truthy() {
                        // $opts{ProtectSaved} = $saveCount;   # protect new values set after this
                        opts.set("ProtectSaved", sv(it, "saveCount"));
                        // add to list of dynamic tag values
                        // push @dynamicFiles, [ $tag, $newVal, \%opts ];
                        av(it, "dynamicFiles").borrow_mut().push(Value::new_array(vec![tag.clone(), new_val.clone(), opts.clone()]));
                        // ++$isWriting;
                        let w = pl::add(&sv(it, "isWriting"), &n(1));
                        set(it, "isWriting", w);
                    } else if !result.is_undef() {
                        warn_(it, cat(&[&"Tag '", &tag, &"' is not writable\n"]))?;
                    } else {
                        warn_(it, cat(&[&"Tag '", &tag, &"' does not exist\n"]))?;
                    }
                    continue;
                }
            }
            // if ($tag =~ s/([-+]|\xe2\x88\x92)$//) {
            let mut which = Value::Undef;
            if add_del_re.subst(&mut tag, false, |c| {
                which = cap(c, 1);
                Vec::new()
            }) > 0
            {
                opts.set(add_del_opt(&which).to_bytes(), n(1)); // set AddValue or DelValue option
                // set $newVal to '' if deleting nothing
                // $newVal = '' if $1 eq '-' and not defined $newVal;
                if pl::str_eq(&which, "-") && new_val.is_undef() {
                    new_val = s("");
                }
            }
            if t(it, "escapeC") && !new_val.is_undef() {
                // $newVal =~ s/\\(x([0-9a-fA-F]{2})|.)/$2 ? chr(hex($2)) : $unescC{$1} || $1/seg;
                let unesc_c = hv(it, "unescC");
                unesc_re.subst(&mut new_val, true, |c| {
                    let (c1, c2) = (cap(c, 1), cap(c, 2));
                    if c2.truthy() {
                        let hex = u8::from_str_radix(&c2.to_text(), 16).unwrap_or(0);
                        vec![hex]
                    } else {
                        let u = unesc_c.borrow().get(&c1.to_bytes()).cloned().unwrap_or_default();
                        if u.truthy() {
                            u.to_bytes().to_vec()
                        } else {
                            c1.to_bytes().to_vec()
                        }
                    }
                });
            }
            // my ($rtn, $wrn) = $mt->SetNewValue($tag, $newVal, %opts);
            let mut args = vec![mt.clone(), tag.clone(), new_val.clone()];
            for k in opts.keys() {
                args.push(Value::bytes(&*k));
                args.push(opts.get(&k));
            }
            let r = it.call_method(mt, "SetNewValue", args, Want::List)?;
            let (rtn, wrn) = (pl::arg(&r, 0), pl::arg(&r, 1));
            *need_save = true;
            // ++$isWriting if $rtn;
            if rtn.truthy() {
                let w = pl::add(&sv(it, "isWriting"), &n(1));
                set(it, "isWriting", w);
            }
            // $wrn and Warning($mt, $wrn);
            if wrn.truthy() {
                call_void(it, "main::Warning", vec![mt.clone(), wrn])?;
            }
        }
        Ok(true)
    })?;
    if !go_on {
        return Ok(false);
    }
    // exclude specified tags
    // foreach (@exclude) {
    let exclude = av(it, "exclude").borrow().clone();
    pl::local_topic(it, |it| {
        for e in exclude {
            set_topic(it, e.clone());
            // $mt->SetNewValue($_, undef, Replace => 2);
            it.call_method(mt, "SetNewValue", vec![mt.clone(), e, Value::Undef, s("Replace"), n(2)], Want::Void)?;
            *need_save = true;
        }
        Ok(())
    })?;
    // unless ($isWriting or $outOpt or @tags) {
    if !(t(it, "isWriting") || t(it, "outOpt") || has(it, "tags")) {
        error(it, s("Nothing to do.\n"))?;
        return Ok(false);
    }
    Ok(true)
}

/// Sort the input files by the values of the -fileOrder tags (lines
/// 1908-1950).
fn sort_files(it: &mut Interp, mt: &Value) -> ER<()> {
    // my @allFiles;
    let all_files = Value::new_array(vec![]);
    // ProcessFiles($mt, \@allFiles);
    call_void(it, "main::ProcessFiles", vec![mt.clone(), all_files.clone()])?;
    // my $sortTool = Image::ExifTool->new;
    let sort_tool = pl::method(it, &s("Image::ExifTool"), "new", vec![])?;
    // $sortTool->Options(FastScan => $fileOrderFast) if $fileOrderFast;
    let fast = sv(it, "fileOrderFast");
    if fast.truthy() {
        pl::method(it, &sort_tool, "Options", vec![s("FastScan"), fast])?;
    }
    // $sortTool->Options(PrintConv => $mt->Options('PrintConv'));
    let pc = pl::method_list(it, mt, "Options", vec![s("PrintConv")])?;
    let mut args = vec![s("PrintConv")];
    args.extend(pc);
    pl::method(it, &sort_tool, "Options", args)?;
    // $sortTool->Options(Duplicates => 0);
    pl::method(it, &sort_tool, "Options", vec![s("Duplicates"), n(0)])?;
    // my (%sortBy, %isFloat, @rev, $file);
    let sort_by = Value::new_hash();
    let is_float = Value::new_hash();
    let mut rev: Vec<bool> = Vec::new();
    // save reverse sort flags
    // push @rev, (s/^-// ? 1 : 0) foreach @fileOrder;
    let file_order = av(it, "fileOrder");
    for f in file_order.borrow_mut().iter_mut() {
        rev.push(re("^-", "").subst(f, false, |_| Vec::new()) > 0);
    }
    let digits = re(r"(\d+)", "");
    // foreach $file (@allFiles) {
    for file in all_files.items() {
        // my @tags;
        let tags = Value::new_array(vec![]);
        // my $info = $sortTool->ImageInfo(Infile($file,1), @fileOrder, \@tags);
        let mut args = vec![sort_tool.clone()];
        args.extend(pl::call_list(it, "main::Infile", vec![file.clone(), n(1)])?);
        args.extend(file_order.borrow().iter().cloned());
        args.push(tags.clone());
        let info = it.call_method(&sort_tool, "ImageInfo", args, Want::Scalar)?.pop().unwrap_or_default();
        // get values of all tags (or '~' to sort last if not defined)
        // foreach (@tags) {
        let Value::ArrayRef(tags_rc) = &tags else { unreachable!() };
        let len = tags_rc.borrow().len();
        for i in 0..len {
            // $_ = $$info{$_};    # put tag value into @tag list
            let tag = tags_rc.borrow()[i].clone();
            let mut v = info.get(tag.to_bytes());
            // defined $_ or $_ = '~', next;
            if v.is_undef() {
                tags_rc.borrow_mut()[i] = s("~");
                continue;
            }
            // $isFloat{$_} = Image::ExifTool::IsFloat($_);
            let f = pl::call(it, "Image::ExifTool::IsFloat", vec![v.clone()])?;
            is_float.set(v.to_bytes(), f.clone());
            // pad numbers to 12 digits to keep them sequential
            // s/(\d+)/(length($1) < 12 ? '0'x(12-length($1)) : '') . $1/eg unless $isFloat{$_};
            if !f.truthy() {
                digits.subst(&mut v, true, |c| {
                    let d = cap(c, 1).to_bytes().to_vec();
                    let mut out = Vec::new();
                    if d.len() < 12 {
                        out.resize(12 - d.len(), b'0');
                    }
                    out.extend_from_slice(&d);
                    out
                });
            }
            tags_rc.borrow_mut()[i] = v;
        }
        // $sortBy{$file} = \@tags;    # save tag values for each file
        sort_by.set(file.to_bytes(), tags.clone());
    }
    // sort in specified order
    let fl = |v: &Value| is_float.get(v.to_bytes()).truthy();
    let mut sorted = all_files.items();
    sorted.sort_by(|a, b| {
        use std::cmp::Ordering;
        // my ($i, $cmp);
        for (i, &r) in rev.iter().enumerate() {
            let u = sort_by.get(a.to_bytes()).at(i as i64);
            let v = sort_by.get(b.to_bytes()).at(i as i64);
            let cmp = if !fl(&u) && !fl(&v) {
                // $cmp = $u cmp $v;               # alphabetically
                Some(u.to_bytes().cmp(&v.to_bytes()))
            } else if fl(&u) && fl(&v) {
                // $cmp = $u <=> $v;               # numerically
                crate::perl::ops::num_cmp(&u, &v)
            } else {
                // $cmp = $isFloat{$u} ? -1 : 1;   # numbers first
                Some(if fl(&u) { Ordering::Less } else { Ordering::Greater })
            };
            // return $rev[$i] ? -$cmp : $cmp if $cmp;
            if let Some(c) = cmp {
                if c != Ordering::Equal {
                    return if r { c.reverse() } else { c };
                }
            }
        }
        // return $a cmp $b;   # default to sort by name
        a.to_bytes().cmp(&b.to_bytes())
    });
    *av(it, "files").borrow_mut() = sorted;
    Ok(())
}

/// Store duplicate database information under absolute path and with
/// standardized case (lines 1955-1979).
fn database_keys(it: &mut Interp) -> ER<()> {
    // my @dbKeys = keys %database;
    let database = hv(it, "database");
    let db_keys: Vec<Value> = database.borrow().keys().map(|k| Value::bytes(&**k)).collect();
    if db_keys.is_empty() {
        return Ok(());
    }
    // if (eval { require Cwd }) {
    if !require_ok(it, "Cwd")? {
        return Ok(());
    }
    // undef $evalWarning;
    set(it, "evalWarning", Value::Undef);
    // local $SIG{'__WARN__'} = sub { $evalWarning = $_[0] };
    pl::local_sig_warn(it, eval_warning_handler(), |it| {
        let get = |k: &Value| database.borrow().get(&k.to_bytes()).cloned().unwrap_or_default();
        // foreach (@dbKeys) {
        pl::local_topic(it, |it| {
            for key in db_keys {
                let mut key = key;
                set_topic(it, key.clone());
                // my $db = $database{$_};
                let db = get(&key);
                // tr/\\/\// and $database{$_} = $db;  # allow for backslashes in SourceFile
                if pl::tr(&mut key, "\\\\", "/", "").truthy() {
                    database.borrow_mut().insert(&key.to_bytes(), db.clone());
                }
                set_topic(it, key.clone());
                // $database{lc} = $db unless $database{lc};   # duplicate entry with lower case
                let lc = pl::lc(&key);
                if !get(&lc).truthy() {
                    database.borrow_mut().insert(&lc.to_bytes(), db.clone());
                }
                // (punt on using ConvertFileName here, so $absPath may be a mix of encodings)
                // my $absPath = AbsPath($_);
                let abs_path = pl::call(it, "main::AbsPath", vec![key.clone()])?;
                let verbose = sv(it, "verbose");
                if !abs_path.is_undef() {
                    // $database{$absPath} = $db unless $database{$absPath};
                    if !get(&abs_path).truthy() {
                        database.borrow_mut().insert(&abs_path.to_bytes(), db.clone());
                    }
                    if verbose.truthy() && pl::gt(&verbose, &n(1)) {
                        let vout = sv(it, "vout");
                        pl::print_to(it, Some(&vout), &[cat(&[&"Imported entry for '", &key, &"' (full path: '", &abs_path, &"')\n"])])?;
                    }
                    // $database{lc $absPath} = $db unless $database{lc $absPath};
                    let lc_abs = pl::lc(&abs_path);
                    if !get(&lc_abs).truthy() {
                        database.borrow_mut().insert(&lc_abs.to_bytes(), db.clone());
                    }
                } else if verbose.truthy() && pl::gt(&verbose, &n(1)) {
                    let vout = sv(it, "vout");
                    pl::print_to(it, Some(&vout), &[cat(&[&"Imported entry for '", &key, &"' (no full path)\n"])])?;
                }
            }
            Ok(())
        })
    })
}
