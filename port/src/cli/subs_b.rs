//! The subs of the `exiftool` script from ProcessFiles to the end of its
//! code (script lines 4234-5075): ProcessFiles, ScanDir, FindFileWindows,
//! FileNotFound, PreserveTime, AbsPath, MyConvertFileName, AddPrintFormat,
//! SuggestedExtension, LoadPrintFormat, FilenameSPrintf, Num2Alpha,
//! NextUnusedFilename, CreateDirectory, OpenOutputFile, AcceptFile,
//! SlurpFile, FilterArgfileLine, ReadStayOpen, EFile, Progress,
//! PrintTagList, PrintErrors.
//!
//! The script's bareword file handles (`DIR_HANDLE`, `\*OUTFILE`,
//! `STAYOPEN`...) and the builtins that work on them (`opendir`, `readdir`,
//! `read`, `sysread`, `binmode`, `close`, `-f`, `select`) run as the
//! interpreter's own builtins, through `builtin` below.

use std::collections::HashMap;

use crate::interp::eval::{Binding, Cell, Flow, Scope, ER};
use crate::interp::{Interp, Want};
use crate::perl::regex as pre;
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, s};

// ---- local helpers ----

/// The value of the script's file lexical `$name`.
fn lex(it: &mut Interp, name: &str) -> Value {
    super::lex_scalar(it, name).borrow().clone()
}

/// `$name = v` for the script's file lexical `$name`.
fn set_lex(it: &mut Interp, name: &str, v: Value) {
    *super::lex_scalar(it, name).borrow_mut() = v;
}

/// A builtin (or a bareword file handle) as perl runs it: `code` runs in
/// package main with `vars` as lexicals and the file lexicals named in
/// `lex` bound to the script's own variables. Returns the result and the
/// values of `vars` afterwards. `$@` is kept as it was.
fn builtin(it: &mut Interp, code: &str, vars: &[(&str, Value)], lex: &[&str], want: Want) -> ER<(Vec<Value>, Vec<Value>)> {
    let scope = Scope::new(None);
    for name in lex {
        let c: Cell = super::lex_scalar(it, name);
        scope.declare(b'$', name, Binding::Scalar(c));
    }
    let cells: Vec<Cell> = vars.iter().map(|(name, v)| scope.scalar(name, v.clone())).collect();
    let saved = it.get_scalar("main::@");
    let r = it.eval_string(code.as_bytes(), "main", &scope, want);
    it.set_scalar("main::@", saved);
    match r {
        Ok(v) => Ok((v, cells.iter().map(|c| c.borrow().clone()).collect())),
        Err(e) => Err(Flow::Die(e)),
    }
}

/// One scalar from `builtin`.
fn builtin1(it: &mut Interp, code: &str, vars: &[(&str, Value)]) -> ER<Value> {
    Ok(builtin(it, code, vars, &[], Want::Scalar)?.0.pop().unwrap_or_default())
}

/// `eval { require MODULE }`, with `$@` set as perl's eval sets it.
fn eval_require(it: &mut Interp, module: &str) -> ER<Value> {
    let (r, _) = builtin(it, &format!("my $r = eval {{ require {module} }}; ($r, $@)"), &[], &[], Want::List)?;
    it.set_scalar("main::@", arg(&r, 1));
    Ok(arg(&r, 0))
}

/// `$^O`
fn os_name(it: &mut Interp) -> ER<Value> {
    builtin1(it, "$^O", &[])
}

/// `$et->Options(NAME => VALUE)`
fn set_option(it: &mut Interp, et: &Value, name: &str, v: Value) -> ER<Value> {
    pl::method(it, et, "Options", vec![s(name), v])
}

/// `++$x` (perl's magic string increment for /^[a-zA-Z]*[0-9]*$/).
fn incr(v: &Value) -> Value {
    if let Value::Str(b) = v {
        let alnum = !b.is_empty() && {
            let i = b.iter().position(|c| !c.is_ascii_alphabetic()).unwrap_or(b.len());
            b[i..].iter().all(|c| c.is_ascii_digit())
        };
        if alnum && !v.looks_like_number() {
            let mut out = b.to_vec();
            let mut i = out.len();
            loop {
                if i == 0 {
                    let first = match out[0] {
                        b'0'..=b'9' => b'1',
                        b'a'..=b'z' => b'a',
                        _ => b'A',
                    };
                    out.insert(0, first);
                    break;
                }
                i -= 1;
                match out[i] {
                    b'z' => out[i] = b'a',
                    b'Z' => out[i] = b'A',
                    b'9' => out[i] = b'0',
                    c => {
                        out[i] = c + 1;
                        break;
                    }
                }
            }
            return Value::bytes(out);
        }
    }
    pl::add(v, &n(1))
}

/// `quotemeta`
fn quotemeta(v: &Value) -> Value {
    Value::bytes(pre::quotemeta(&v.to_bytes()))
}

/// The match spans of `re` in `subject` from `start` (for `/.../g` loops
/// that read and set `pos`).
fn match_at(r: &pre::PerlRegex, subject: &[u8], start: usize) -> Option<pre::Spans> {
    if start > subject.len() {
        return None;
    }
    r.match_at(subject, start).ok().flatten()
}

/// A capture of a span list as a value (undef when the group did not match).
fn span_cap(subject: &[u8], sp: &pre::Spans, i: usize) -> Value {
    match sp.get(i).copied().flatten() {
        Some((a, b)) => Value::bytes(&subject[a..b]),
        None => Value::Undef,
    }
}

/// `CleanFilename($x)` (writes its argument).
fn clean_filename(it: &mut Interp, v: &mut Value) -> ER<()> {
    let mut a = [v.clone()];
    pl::call_mut(it, "main::CleanFilename", &mut a, Want::Scalar)?;
    *v = a[0].clone();
    Ok(())
}

fn warn_(it: &mut Interp, msg: Value) -> ER<Value> {
    pl::call(it, "main::Warn", vec![msg])
}

fn error_(it: &mut Interp, msg: Value) -> ER<Value> {
    pl::call(it, "main::Error", vec![msg])
}

/// Concatenate byte pieces into a value.
fn cat(parts: &[&[u8]]) -> Value {
    Value::bytes(parts.concat())
}

fn b(v: &Value) -> std::rc::Rc<[u8]> {
    v.to_bytes()
}

//------------------------------------------------------------------------------
// Process files in our @files list
// Inputs: 0) ExifTool ref, 1) list ref to just return full file names
fn process_files(it: &mut Interp, a: Vec<Value>, _want: Want) -> ER<Vec<Value>> {
    let (et, list) = (arg(&a, 0), arg(&a, 1));
    let enc = pl::method(it, &et, "Options", vec![s("CharsetFileName")])?;
    let utf8_file_name = super::lex_hash(it, "utf8FileName");
    let files = super::lex_array(it, "files").items();
    for file in files {
        if utf8_file_name.get(b(&file)).truthy() {
            set_option(it, &et, "CharsetFileName", s("UTF8"))?;
        }
        if !lex(it, "progressMax").is_undef() {
            if lex(it, "progressNext").is_undef() {
                let mut progress_next = pl::add(&lex(it, "progressCount"), &lex(it, "progressIncr"));
                progress_next = pl::sub(&progress_next, &pl::modulo(&progress_next, &lex(it, "progressIncr"))?); // (show even multiples)
                let progress_max = lex(it, "progressMax");
                if pl::gt(&progress_next, &progress_max) {
                    progress_next = progress_max;
                }
                set_lex(it, "progressNext", progress_next);
            }
            let c = incr(&lex(it, "progressCount"));
            set_lex(it, "progressCount", c);
            if lex(it, "progress").truthy() {
                if pl::ge(&lex(it, "progressCount"), &lex(it, "progressNext")) {
                    let (count, max) = (lex(it, "progressCount"), lex(it, "progressMax"));
                    set_lex(it, "progStr", cat(&[b" [", &b(&count), b"/", &b(&max), b"]"]));
                } else {
                    set_lex(it, "progStr", Value::Undef); // don't update progress yet
                }
            }
        }
        if pl::method(it, &et, "IsDirectory", vec![file.clone()])?.truthy() && !lex(it, "listDir").truthy() {
            set_lex(it, "validFile", n(1));
            set_lex(it, "multiFile", n(1));
            scan_dir(it, vec![et.clone(), file.clone(), list.clone()], Want::Void)?;
        } else if lex(it, "filterFlag").truthy() && !accept_file_(it, &file)?.truthy() {
            if pl::method(it, &et, "Exists", vec![file.clone()])?.truthy() {
                set_lex(it, "filtered", n(1));
                if lex(it, "verbose").truthy() {
                    let vout = lex(it, "vout");
                    progress(it, vec![vout, cat(&[b"-------- ", &b(&file), b" (wrong extension)"])], Want::Void)?;
                }
            } else {
                error_(it, cat(&[b"Error: File not found - ", &b(&file), b"\n"]))?;
                file_not_found(it, vec![file.clone()], Want::Void)?;
            }
        } else {
            set_lex(it, "validFile", n(1));
            if list.truthy() {
                list.push(file.clone());
            } else {
                let end_dir_h = super::lex_hash(it, "endDir");
                if !end_dir_h.keys().is_empty() {
                    let df = pl::call_list(it, "Image::ExifTool::SplitFileName", vec![file.clone()])?;
                    if end_dir_h.get(b(&arg(&df, 0))).truthy() {
                        continue;
                    }
                }
                pl::call(it, "main::GetImageInfo", vec![et.clone(), file.clone()])?;
                let purge = lex(it, "purge");
                if purge.truthy() {
                    pl::call(it, "Image::ExifTool::Purge", vec![purge])?;
                }
                if lex(it, "end").truthy() {
                    warn_(it, cat(&[b"End called - ", &b(&file), b"\n"]))?;
                }
                if lex(it, "endDir").truthy() {
                    warn_(it, cat(&[b"EndDir called - ", &b(&file), b"\n"]))?;
                    let df = pl::call_list(it, "Image::ExifTool::SplitFileName", vec![file.clone()])?;
                    end_dir_h.set(b(&arg(&df, 0)), n(1));
                    set_lex(it, "endDir", Value::Undef);
                }
            }
        }
        if utf8_file_name.get(b(&file)).truthy() {
            set_option(it, &et, "CharsetFileName", enc.clone())?;
        }
        if lex(it, "end").truthy() {
            break;
        }
    }
    Ok(vec![])
}

//------------------------------------------------------------------------------
// Scan directory for image files
// Inputs: 0) ExifTool ref, 1) directory name, 2) list ref to return file names
fn scan_dir(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| scan_dir_body(it, a, want))
}

fn scan_dir_body(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, mut dir, list) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let mut file_list: Vec<Value> = Vec::new();
    let (mut done, mut utf8_name, mut win_surrogate, mut end_this_dir) = (Value::Undef, false, false, false);
    let enc = pl::method(it, &et, "Options", vec![s("CharsetFileName")])?;
    // recode as UTF-8 if necessary
    if enc.truthy() {
        if !pl::str_eq(&enc, "UTF8") {
            dir = pl::method(it, &et, "Decode", vec![dir, enc.clone(), Value::Undef, s("UTF8")])?;
            set_option(it, &et, "CharsetFileName", s("UTF8"))?; // now using UTF8
        }
        utf8_name = true;
    }
    let ignore = super::lex_hash(it, "ignore");
    if ignore.get(b(&dir)).truthy() {
        return pl::ret_nothing(want);
    }
    // use Win32::FindFile on Windows if available
    // (ReadDir will croak if there is a wildcard, so check for this)
    if pl::str_eq(&os_name(it)?, "MSWin32") && !pl::call(it, "main::HasWildcards", vec![dir.clone()])?.truthy() {
        set_lex(it, "evalWarning", Value::Undef);
        let handler = builtin(it, "sub { $evalWarning = $_[0] }", &[], &["evalWarning"], Want::Scalar)?.0.pop().unwrap_or_default();
        pl::local_sig_warn(it, handler, |it| {
            if pl::ge(&pl::call(it, "main::CheckUTF8", vec![dir.clone(), enc.clone()])?, &n(0)) {
                if eval_require(it, "Win32::FindFile")?.truthy() {
                    let r = pl::eval_block(it, |it| {
                        let names = pl::call_list(it, "Win32::FindFile::ReadDir", vec![dir.clone()])?;
                        let mut out = Vec::new();
                        for nm in names {
                            out.push(pl::method(it, &nm, "cFileName", vec![])?);
                        }
                        Ok(out)
                    })?;
                    if let Some(l) = r {
                        file_list = l;
                    }
                    let err = it.get_scalar("main::@");
                    if err.truthy() {
                        set_lex(it, "evalWarning", err);
                    }
                    let mut ew = lex(it, "evalWarning");
                    if ew.truthy() {
                        re(r"\n$", "").subst(&mut ew, false, |_| Vec::new()); // chomp
                        re(r" at .*", "s").subst(&mut ew, false, |_| Vec::new());
                        set_lex(it, "evalWarning", ew.clone());
                        pl::call(it, "main::Warning", vec![et.clone(), cat(&[b"[Win32::FindFile] ", &b(&ew), b" - ", &b(&dir)])])?;
                        if re("surrogate", "").is_match(&ew) {
                            win_surrogate = true;
                        }
                    } else {
                        set_option(it, &et, "CharsetFileName", s("UTF8"))?; // now using UTF8
                        utf8_name = true; // ReadDir returns UTF-8 file names
                        done = n(1);
                    }
                } else {
                    done = n(0);
                }
            }
            Ok(())
        })?;
    }
    if !done.truthy() {
        // use standard perl library routines to read directory
        if !builtin1(it, "opendir(DIR_HANDLE, $dir)", &[("dir", dir.clone())])?.truthy() {
            warn_(it, cat(&[b"Error opening directory ", &b(&dir), b"\n"]))?;
            return pl::ret_nothing(want);
        }
        file_list = builtin(it, "my @l = readdir(DIR_HANDLE); closedir(DIR_HANDLE); @l", &[], &[], Want::List)?.0;
        if !done.is_undef() {
            // issue warning if some names would have required Win32::FindFile
            let check = re(r"[\?\x80-\xff]", "");
            for file in std::iter::once(&dir).chain(file_list.iter()) {
                if !check.is_match(file) {
                    continue;
                }
                pl::call(it, "main::WarnOnce", vec![s("Install Win32::FindFile to support Windows Unicode file names in directories\n")])?;
                break;
            }
        }
    }
    if !re(r"/$", "").is_match(&dir) {
        dir = pl::concat(&dir, &s("/")); // make sure directory name ends with '/'
    }
    let utf8_file_name = super::lex_hash(it, "utf8FileName");
    for file in file_list {
        if pl::str_eq(&file, ".") || pl::str_eq(&file, "..") {
            continue;
        }
        let mut path = pl::concat(&dir, &file);
        if pl::method(it, &et, "IsDirectory", vec![path.clone()])?.truthy() {
            let recurse = lex(it, "recurse");
            if !recurse.truthy() {
                continue;
            }
            // ignore directories starting with "." by default
            if re(r"^\.", "").is_match(&file) && pl::num_eq(&recurse, &n(1)) {
                continue;
            }
            // note: this doesn't work in Windows cmd (see forum17243)
            if ignore.get(b(&file)).truthy() || (ignore.get("SYMLINKS").truthy() && is_symlink(&path)) {
                continue;
            }
            scan_dir(it, vec![et.clone(), path.clone(), list.clone()], Want::Void)?;
            if lex(it, "end").truthy() {
                break;
            }
            continue;
        }
        if end_this_dir {
            continue;
        }
        if lex(it, "ignoreHidden").truthy() && re(r"^\.", "").is_match(&file) {
            continue; // ignore hidden files if specified
        }
        // apply rules from -ext options
        let mut accepted = Value::Undef;
        if lex(it, "filterFlag").truthy() {
            accepted = accept_file_(it, &file)?;
            if !accepted.truthy() {
                continue;
            }
            // must be specifically accepted to bypass selection logic
            accepted = n(accepted.to_i64() & 0x01);
        }
        if !accepted.truthy() {
            // read/write this file if it is a supported type
            let scan_writable = lex(it, "scanWritable");
            if scan_writable.truthy() {
                if pl::str_eq(&scan_writable, "1") {
                    if !pl::call(it, "main::CanWrite", vec![file.clone()])?.truthy() {
                        continue;
                    }
                } else {
                    let typ = pl::call(it, "main::GetFileType", vec![file.clone()])?;
                    if !(!typ.is_undef() && b(&typ) == b(&scan_writable)) {
                        continue;
                    }
                }
            } else if !pl::call(it, "main::GetFileType", vec![file.clone()])?.truthy() {
                if !lex(it, "doUnzip").truthy() {
                    continue;
                }
                if !re(r"\.(gz|bz2)$", "i").is_match(&file) {
                    continue;
                }
            }
        }
        // Windows patch to avoid replacing filename containing Unicode surrogate with 8.3 name
        if win_surrogate && lex(it, "isWriting").truthy() && {
            let oo = lex(it, "overwriteOrig");
            !oo.truthy() || !pl::num_eq(&oo, &n(2))
        } && !lex(it, "doSetFileName").truthy()
            && re("~", "").is_match(&file)
        // (8.3 name will contain a tilde)
        {
            warn_(it, cat(&[b"Not writing ", &b(&path), b"\n"]))?;
            pl::call(it, "main::WarnOnce", vec![s("Use -overwrite_original_in_place to write files with Unicode surrogate characters\n")])?;
            efile(it, vec![file.clone()], Want::Void)?;
            let c = incr(&lex(it, "countBad"));
            set_lex(it, "countBad", c);
            continue;
        }
        if utf8_name {
            utf8_file_name.set(b(&path), n(1));
        }
        if list.truthy() {
            list.push(path.clone());
        } else {
            pl::call(it, "main::GetImageInfo", vec![et.clone(), path.clone()])?;
            let purge = lex(it, "purge");
            if purge.truthy() {
                pl::call(it, "Image::ExifTool::Purge", vec![purge])?;
            }
            if lex(it, "end").truthy() {
                warn_(it, cat(&[b"End called - ", &b(&file), b"\n"]))?;
                break;
            }
            if lex(it, "endDir").truthy() {
                re(r"/$", "").subst(&mut path, false, |_| Vec::new());
                warn_(it, cat(&[b"EndDir called - ", &b(&path), b"\n"]))?;
                super::lex_hash(it, "endDir").set(b(&path), n(1));
                end_this_dir = true;
                set_lex(it, "endDir", Value::Undef);
            }
        }
    }
    let c = incr(&lex(it, "countDir"));
    set_lex(it, "countDir", c);
    let r = set_option(it, &et, "CharsetFileName", enc)?; // restore original setting
    Ok(vec![r])
}

/// `-l $path` (lstat: a symbolic link).
fn is_symlink(path: &Value) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let p = b(path);
    std::fs::symlink_metadata(std::ffi::OsStr::from_bytes(&p)).map(|m| m.file_type().is_symlink()).unwrap_or(false)
}

//------------------------------------------------------------------------------
// Find files with wildcard expression on Windows
// Inputs: 0) ExifTool ref, 1) file name with wildcards
// Returns: list of matching file names
// Notes:
// 1) Win32::FindFile must already be loaded
// 2) Sets flag in %utf8FileName for each file found
fn find_file_windows(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, mut wildfile) = (arg(&a, 0), arg(&a, 1));

    // recode file name as UTF-8 if necessary
    let enc = pl::method(it, &et, "Options", vec![s("CharsetFileName")])?;
    if enc.truthy() && !pl::str_eq(&enc, "UTF8") {
        wildfile = pl::method(it, &et, "Decode", vec![wildfile, enc.clone(), Value::Undef, s("UTF8")])?;
    }
    clean_filename(it, &mut wildfile)?; // use forward slashes
    let (dir, wildname) = match re(r"(.*[:/])(.*)", "").m(&wildfile) {
        Some(c) => (cap(&c, 1), cap(&c, 2)),
        None => (s(""), wildfile.clone()),
    };
    if pl::call(it, "main::HasWildcards", vec![dir.clone()])?.truthy() {
        warn_(it, s("Wildcards don't work in the directory specification\n"))?;
        return Ok(vec![]);
    }
    if !pl::ge(&pl::call(it, "main::CheckUTF8", vec![wildfile.clone(), enc.clone()])?, &n(0)) {
        return Ok(vec![]);
    }
    set_lex(it, "evalWarning", Value::Undef);
    let handler = builtin(it, "sub { $evalWarning = $_[0] }", &[], &["evalWarning"], Want::Scalar)?.0.pop().unwrap_or_default();
    let utf8_file_name = super::lex_hash(it, "utf8FileName");
    pl::local_sig_warn(it, handler, |it| {
        let mut files: Vec<Value> = Vec::new();
        pl::eval_block(it, |it| {
            let mut names = pl::call_list(it, "Win32::FindFile::FindFile", vec![wildfile.clone()])?;
            if names.is_empty() {
                return Ok(());
            }
            // (apparently this isn't always sorted, so do a case-insensitive sort here)
            names.sort_by(|x, y| b(&pl::uc(x)).cmp(&b(&pl::uc(y))));
            // replace "\?" with ".", and "\*" with ".*" for regular expression
            let mut rname = quotemeta(&wildname);
            re(r"\\\?", "").subst(&mut rname, true, |_| b".".to_vec());
            re(r"\\\*", "").subst(&mut rname, true, |_| b".*".to_vec());
            let name_re = pl::re_dyn(&[b"^", &b(&rname)[..], b"$"].concat(), "i")?;
            for nm in names {
                let nm = pl::method(it, &nm, "cFileName", vec![])?;
                // make sure that FindFile behaves
                // (otherwise "*.jpg" matches things like "a.jpg_original"!)
                if !name_re.is_match(&nm) {
                    continue;
                }
                if pl::str_eq(&nm, ".") || pl::str_eq(&nm, "..") {
                    continue; // don't match "." and ".."
                }
                let file = pl::concat(&dir, &nm); // add back directory name
                files.push(file.clone());
                utf8_file_name.set(b(&file), n(1)); // flag this file name as UTF-8 encoded
            }
            Ok(())
        })?;
        let err = it.get_scalar("main::@");
        if err.truthy() {
            set_lex(it, "evalWarning", err);
        }
        let mut ew = lex(it, "evalWarning");
        if ew.truthy() {
            re(r"\n$", "").subst(&mut ew, false, |_| Vec::new()); // chomp
            re(r" at .*", "s").subst(&mut ew, false, |_| Vec::new());
            set_lex(it, "evalWarning", ew.clone());
            warn_(it, cat(&[b"Error: [Win32::FindFile] ", &b(&ew), b" - ", &b(&wildfile), b"\n"]))?;
            files.clear();
            efile(it, vec![wildfile.clone()], Want::Void)?;
            let c = incr(&lex(it, "countBad"));
            set_lex(it, "countBad", c);
        }
        if want == Want::List {
            Ok(files)
        } else {
            Ok(vec![n(files.len() as i64)])
        }
    })
}

//------------------------------------------------------------------------------
// Handle missing file on the command line
// Inputs: 0) file name
fn file_not_found(it: &mut Interp, a: Vec<Value>, _want: Want) -> ER<Vec<Value>> {
    let file = arg(&a, 0);
    if let Some(c) = re(r"^(DIR|FILE)$", "").m(&file) {
        let typ = if pl::str_eq(&cap(&c, 1), "DIR") { "directory" } else { "file" };
        let r = warn_(it, cat(&[b"You were meant to enter any valid ", typ.as_bytes(), b" name, not \"", &b(&file), b"\" literally.\n"]))?;
        return Ok(vec![r]);
    }
    Ok(vec![])
}

//------------------------------------------------------------------------------
// Patch for OS X 10.6 to preserve file modify date
// (this probably isn't a 100% fix, but it may solve a majority of the cases)
fn preserve_time(it: &mut Interp, _a: Vec<Value>, _want: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| {
        let preserve_time = super::lex_hash(it, "preserveTime");
        let mt = lex(it, "mt");
        for k in preserve_time.keys() {
            let key = Value::bytes(&k[..]);
            it.set_scalar("main::_", key.clone());
            let mut args = vec![key];
            args.extend(preserve_time.get_or_new_array(&k[..]).items());
            pl::method(it, &mt, "SetFileTime", args)?;
        }
        if let Value::HashRef(h) = &preserve_time {
            h.borrow_mut().clear();
        }
        Ok(vec![])
    })
}

//------------------------------------------------------------------------------
// Return absolute path for a file
// Inputs: 0) file name
// Returns: absolute path string, or undef if path could not be determined
// Note: Warnings should be suppressed when calling this routine
fn abs_path(it: &mut Interp, a: Vec<Value>, _want: Want) -> ER<Vec<Value>> {
    let file = arg(&a, 0);
    let mut path = Value::Undef;
    if !file.is_undef() {
        if pl::str_eq(&file, "*") {
            return pl::ret_undef(); // (CSV SourceFile may be '*' -- no absolute path for that)
        }
        let mt = lex(it, "mt");
        if pl::str_eq(&os_name(it)?, "MSWin32") && pl::method(it, &mt, "Options", vec![s("WindowsLongPath")])?.truthy() {
            path = pl::method(it, &mt, "WindowsLongPath", vec![file.clone()])?;
        } else if eval_require(it, "Cwd")?.truthy() {
            let handler = builtin1(it, "sub { }", &[])?;
            path = pl::local_sig_warn(it, handler, |it| {
                Ok(pl::eval_block(it, |it| pl::call(it, "Cwd::abs_path", vec![file.clone()]))?.unwrap_or_default())
            })?;
        }
        if !path.is_undef() {
            clean_filename(it, &mut path)?; // use forward slashes
        }
    }
    pl::ret(path)
}

//------------------------------------------------------------------------------
// Convert file name to ExifTool Charset
// Inputs: 0) ExifTool ref, 1) file name in CharsetFileName
// Returns: file name in ExifTool Charset
fn my_convert_file_name(it: &mut Interp, a: Vec<Value>, _want: Want) -> ER<Vec<Value>> {
    let (et, file) = (arg(&a, 0), arg(&a, 1));
    let enc = pl::method(it, &et, "Options", vec![s("CharsetFileName")])?;
    let utf8_file_name = super::lex_hash(it, "utf8FileName");
    if utf8_file_name.get(b(&file)).truthy() {
        set_option(it, &et, "CharsetFileName", s("UTF8"))?;
    }
    let conv_file = pl::method(it, &et, "ConvertFileName", vec![file.clone()])?;
    if utf8_file_name.get(b(&file)).truthy() {
        set_option(it, &et, "CharsetFileName", enc)?;
    }
    pl::ret(conv_file)
}

//------------------------------------------------------------------------------
// Add print format entry
// Inputs: 0) expression string
fn add_print_format(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let mut expr = arg(&a, 0);
    let typ;
    if re(r"^#", "").is_match(&expr) {
        let mut t = Value::Undef;
        if re(r"^#\[(HEAD|SECT|IF|BODY|ENDS|TAIL)\]", "").subst(&mut expr, false, |c| {
            t = cap(c, 1);
            Vec::new()
        }) == 0
        {
            return pl::ret_nothing(want); // ignore comments
        }
        typ = t;
    } else {
        typ = s("BODY");
    }
    let print_fmt = super::lex_hash(it, "printFmt");
    if !print_fmt.get(b(&typ)).truthy() {
        print_fmt.set(b(&typ), Value::new_array(vec![]));
    }
    print_fmt.get_or_new_array(b(&typ)).push(expr.clone());
    // add to list of requested tags
    let request_tags = super::lex_array(it, "requestTags");
    for t in re(r"\$\{?((?:[-_0-9A-Z]+:)*[-_0-9A-Z?*]+)", "i").all(&expr) {
        request_tags.push(t);
    }
    if re(r"\bSetTags\b", "").is_match(&expr) {
        print_fmt.set("SetTags", n(1));
    }
    Ok(vec![])
}

//------------------------------------------------------------------------------
// Get suggested file extension based on tag value for binary output
// Inputs: 0) ExifTool ref, 1) data ref, 2) tag name
// Returns: file extension (lower case), or 'dat' if unknown
fn suggested_extension(it: &mut Interp, a: Vec<Value>, _want: Want) -> ER<Vec<Value>> {
    let (et, val_pt, tag) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let ext;
    if !lex(it, "binaryOutput").truthy() {
        return pl::ret(s("txt"));
    }
    let raw;
    pl::at(4568);
    let val = pl::sderef(&val_pt)?;
    let m = |p: &str, f: &str| re(p, f).is_match(&val);
    if m(r"^\xff\xd8\xff", "") {
        ext = s("jpg");
    } else if m(r"^(\0\0\0\x0cjP(  |\x1a\x1a)\x0d\x0a\x87\x0a|\xff\x4f\xff\x51\0)", "") {
        ext = s("jp2");
    } else if m(r"^(\x89P|\x8aM|\x8bJ)NG\r\n\x1a\n", "") {
        ext = s("png");
    } else if m(r"^GIF8[79]a", "") {
        ext = s("gif");
    } else if m(r"^<\?xpacket", "") || pl::str_eq(&tag, "XMP") {
        // (perl warns here if $tag is undef)
        ext = s("xmp");
    } else if m(r"^<\?xml", "") || pl::str_eq(&tag, "XML") {
        ext = s("xml");
    } else if m(r"^RIFF....WAVE", "s") {
        ext = s("wav");
    } else if pl::str_eq(&tag, "OriginalRawImage") && {
        raw = pl::method(it, &et, "GetValue", vec![s("OriginalRawFileName")])?;
        !raw.is_undef()
    } {
        let mut e = raw;
        re(r"^.*\.", "s").subst(&mut e, false, |_| Vec::new());
        ext = if e.truthy() { pl::lc(&e) } else { s("raw") };
    } else if pl::str_eq(&tag, "EXIF") {
        ext = s("exif");
    } else if pl::str_eq(&tag, "ICC_Profile") {
        ext = s("icc");
    } else if m(r"^(MM\0\x2a|II\x2a\0)", "") {
        ext = s("tiff");
    } else if let Some(c) = re(r"^.{4}ftyp(3gp|mp4|f4v|qt  )", "s").m(&val) {
        let one = cap(&c, 1);
        ext = if pl::str_eq(&one, "qt  ") { s("mov") } else { one };
    } else if m(r"^<(!DOCTYPE )?html", "i") {
        ext = s("html");
    } else if m(r"^[\n\r]*\{[\n\r]*\\rtf", "") {
        ext = s("rtf");
    } else if !m(r"^.{0,4096}\0", "s") {
        ext = s("txt");
    } else if m(r"^BM.{15}\0", "s") {
        ext = s("bmp");
    } else if m(r"^CANON OPTIONAL DATA\0", "") {
        ext = s("vrd");
    } else if m(r"^IIII\x04\0\x04\0", "") {
        ext = s("dr4");
    } else if m(r"^(.{10}|.{522})(\x11\x01|\x00\x11)", "s") {
        ext = s("pict");
    } else if m(r"^\xff\x0a|\0\0\0\x0cJXL \x0d\x0a......ftypjxl", "s") {
        ext = s("jxl");
    } else if m(r"^.{4}jumb\0.{3}jumdc2pa", "s") {
        ext = s("c2pa");
    } else if pl::str_eq(&tag, "JUMBF") {
        ext = s("jumbf");
    } else {
        ext = s("dat");
    }
    pl::ret(ext)
}

//------------------------------------------------------------------------------
// Load print format file
// Inputs: 0) file name, 1) flag to avoid adding newline to input argument
// - saves lines of file to %printFmt list
// - adds tag names to @tags list
fn load_print_format(it: &mut Interp, a: Vec<Value>, _want: Want) -> ER<Vec<Value>> {
    let (mut arg0, no_nl) = (arg(&a, 0), arg(&a, 1));
    if arg0.is_undef() {
        error_(it, s("Must specify file or expression for -p option\n"))?;
    } else if !re(r"\n", "").is_match(&arg0) && builtin1(it, "-f $arg", &[("arg", arg0.clone())])?.truthy() && {
        let mt = lex(it, "mt");
        let fh = builtin1(it, "\\*FMT_FILE", &[])?;
        pl::method(it, &mt, "Open", vec![fh, arg0.clone()])?.truthy()
    } {
        let lines = builtin(it, "<FMT_FILE>", &[], &[], Want::List)?.0;
        pl::local_topic(it, |it| {
            for line in lines {
                it.set_scalar("main::_", line.clone());
                add_print_format(it, vec![line], Want::Void)?;
            }
            Ok(())
        })?;
        builtin(it, "close(FMT_FILE)", &[], &[], Want::Scalar)?;
    } else {
        if !no_nl.truthy() {
            arg0 = pl::concat(&arg0, &s("\n"));
        }
        add_print_format(it, vec![arg0], Want::Void)?;
    }
    Ok(vec![])
}

//------------------------------------------------------------------------------
// A sort of sprintf for filenames
// Inputs: 0) format string (%d=dir, %f=file name, %e=ext),
//         1) source filename or undef to test format string
//         2-4) [%t %g %s %o only] tag name, ref to array of group names,
//              suggested extension, original raw file name
// Returns: new filename or undef on error (or if no file and fmt contains token)
fn filename_sprintf(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| filename_sprintf_body(it, a, want))
}

fn filename_sprintf_body(it: &mut Interp, a: Vec<Value>, _want: Want) -> ER<Vec<Value>> {
    let (fmt, mut file) = (arg(&a, 0), arg(&a, 1));
    let extra: Vec<Value> = a.iter().skip(2).cloned().collect();
    // return format string straight away if no tokens
    if !re(r"%[-+]?\d*[.:]?\d*[lu]?[dDfFeEtgso]", "").is_match(&fmt) {
        return pl::ret(fmt);
    }
    if file.is_undef() {
        return pl::ret_undef();
    }
    clean_filename(it, &mut file)?; // make sure we are using forward slashes
    // split filename into directory, file, extension
    let mut part: HashMap<u8, Value> = HashMap::new();
    if let Some(c) = re(r"^(.*?)([^\/]*?)(\.[^.\/]*)?$", "").m(&file) {
        part.insert(b'd', cap(&c, 1));
        part.insert(b'f', cap(&c, 2));
        part.insert(b'E', cap(&c, 3));
    }
    let get = |part: &HashMap<u8, Value>, k: u8| part.get(&k).cloned().unwrap_or_default();
    if get(&part, b'f').is_undef() {
        warn_(it, cat(&[b"Error: Bad pattern match for file ", &b(&file), b"\n"]))?;
        return pl::ret_undef();
    }
    if get(&part, b'E').truthy() {
        part.insert(b'e', pl::substr(&get(&part, b'E'), 1, None));
    } else {
        part.insert(b'e', s(""));
        part.insert(b'E', s(""));
    }
    part.insert(b'F', pl::concat(&get(&part, b'f'), &get(&part, b'E')));
    let mut dd = get(&part, b'd');
    re(r"/+$", "").subst(&mut dd, false, |_| Vec::new());
    part.insert(b'D', dd);
    for (i, k) in [b't', b'g', b's', b'o'].iter().enumerate() {
        part.insert(*k, arg(&extra, i));
    }
    if get(&part, b'o').truthy() {
        let mut o = get(&part, b'o');
        re(r"^.*[/\\]", "s").subst(&mut o, false, |_| Vec::new()); // remove directory if it exists
        part.insert(b'o', o);
    }
    let (mut filename, mut pos) = (Vec::<u8>::new(), 0usize);
    let fmt_b = b(&fmt);
    let tok = pre::compile(br"(%([-+]?)(\d*)([.:]?)(\d*)([lu]?)([dDfFeEtgso]))", "").unwrap();
    let gnum = pre::compile(br"\G(\d?)", "").unwrap();
    let mut search = 0usize;
    while let Some(sp) = match_at(&tok, &fmt_b, search) {
        let (ms, me) = sp[0].unwrap();
        search = me;
        filename.extend_from_slice(&fmt_b[pos..ms]);
        pos = me;
        let sign = span_cap(&fmt_b, &sp, 2);
        let mut wid = span_cap(&fmt_b, &sp, 3);
        let dot = span_cap(&fmt_b, &sp, 4);
        let mut skip = span_cap(&fmt_b, &sp, 5);
        if !skip.truthy() {
            skip = n(0);
        }
        let mode = span_cap(&fmt_b, &sp, 6);
        let code = span_cap(&fmt_b, &sp, 7);
        let ck = b(&code)[0];
        let mut path: Vec<Value> = Vec::new();
        let len;
        if pl::str_eq(&pl::lc(&code), "d") && dot.truthy() && pl::str_eq(&dot, ":") {
            // field width applies to directory levels instead of characters
            path = pl::split(&re("/", ""), &get(&part, ck), 0);
            len = n(path.len() as i64);
        } else {
            if ck == b'g' {
                let mut groups = get(&part, b'g');
                if !groups.truthy() {
                    groups = Value::new_array(vec![]);
                }
                // look for %g1, %g2, etc
                let gm = gnum.match_at(&fmt_b, search).ok().flatten();
                let mut gi = Value::Undef;
                if let Some(g) = &gm {
                    gi = span_cap(&fmt_b, g, 1);
                    search = g[0].unwrap().1;
                }
                let idx = if gi.truthy() { gi.to_i64() } else { 0 };
                pl::at(4683);
                let gv = match &groups {
                    Value::ArrayRef(_) => groups.at(idx),
                    v => {
                        pl::aderef(v)?;
                        Value::Undef
                    }
                };
                part.insert(b'g', gv);
                pos = search;
            }
            if get(&part, ck).is_undef() {
                part.insert(ck, s(""));
            }
            len = pl::length(&get(&part, ck));
        }
        if !pl::lt(&skip, &len) {
            continue;
        }
        if pl::str_eq(&wid, "") || pl::gt(&pl::add(&wid, &skip), &len) {
            wid = pl::sub(&len, &skip);
        }
        if pl::str_eq(&sign, "-") {
            skip = pl::sub(&pl::sub(&len, &wid), &skip);
        }
        let mut part_v;
        if !path.is_empty() {
            let (s0, w0) = (skip.to_i64(), wid.to_i64());
            let slice: Vec<Value> = (s0..s0 + w0).map(|i| if i >= 0 { path.get(i as usize).cloned().unwrap_or_default() } else { path.get((path.len() as i64 + i) as usize).cloned().unwrap_or_default() }).collect();
            part_v = pl::join("/", &slice); // (perl warns here if a level is undef)
            if ck != b'D' {
                part_v = pl::concat(&part_v, &s("/"));
            }
        } else {
            part_v = pl::substr(&get(&part, ck), skip.to_i64(), Some(wid.to_i64()));
        }
        if mode.truthy() {
            part_v = if pl::str_eq(&mode, "u") { pl::uc(&part_v) } else { pl::lc(&part_v) };
        }
        filename.extend_from_slice(&b(&part_v));
    }
    filename.extend_from_slice(&fmt_b[pos.min(fmt_b.len())..]); // add rest of file name
    // remove double slashes (except at beginning to allow Windows UNC paths)
    let mut filename = Value::bytes(filename);
    re(r"(?!^)//", "").subst(&mut filename, true, |_| b"/".to_vec());
    pl::ret(filename)
}

//------------------------------------------------------------------------------
// Convert number to alphabetical index: a, b, c, ... z, aa, ab ...
// Inputs: 0) number
// Returns: alphabetical index string
fn num2alpha_(num: &Value) -> ER<Value> {
    let mut num = num.clone();
    let chr = |v: Value| Value::bytes([(97 + v.to_i64()) as u8]);
    let mut alpha = chr(pl::modulo(&num, &n(26))?);
    while pl::ge(&num, &n(26)) {
        num = pl::sub(&pl::int(&pl::div(&num, &n(26))?), &n(1));
        alpha = pl::concat(&chr(pl::modulo(&num, &n(26))?), &alpha);
    }
    Ok(alpha)
}

fn num2alpha(_it: &mut Interp, a: Vec<Value>, _want: Want) -> ER<Vec<Value>> {
    pl::ret(num2alpha_(&arg(&a, 0))?)
}

//------------------------------------------------------------------------------
// Expand '%c' and '%C' codes if filename to get next unused file name
// Inputs: 0) file name format string, 1) filename ok to use even if it exists
// Returns: new file name
fn next_unused_filename(it: &mut Interp, a: Vec<Value>, _want: Want) -> ER<Vec<Value>> {
    let (fmt, okfile) = (arg(&a, 0), arg(&a, 1));
    if !re(r"%[-+]?\d*[.:]?\d*[lun]?[cC]", "").is_match(&fmt) {
        return pl::ret(fmt);
    }
    let sep = |sign: &Value| if pl::str_eq(sign, "-") { s("-") } else if pl::str_eq(sign, "+") { s("_") } else { Value::Undef };
    let (mut copy, mut alpha) = (n(0), s("a"));
    let mut last_file = Value::Undef;
    let fmt_b = b(&fmt);
    let used_file_name = super::lex_hash(it, "usedFileName");
    let mt = lex(it, "mt");
    loop {
        let (mut filename, mut pos) = (Vec::<u8>::new(), 0usize);
        let tok_re = pre::compile(br"(%([-+]?)(\d*)([.:]?)(\d*)([lun]?)([cC]))", "").unwrap();
        let mut search = 0usize;
        while let Some(sp) = match_at(&tok_re, &fmt_b, search) {
            let (ms, me) = sp[0].unwrap();
            search = me;
            filename.extend_from_slice(&fmt_b[pos..ms]);
            pos = me;
            let sign = span_cap(&fmt_b, &sp, 2);
            let mut wid = span_cap(&fmt_b, &sp, 3);
            if !wid.truthy() {
                wid = n(0);
            }
            let dec = span_cap(&fmt_b, &sp, 4);
            let mut wid2 = span_cap(&fmt_b, &sp, 5);
            if !wid2.truthy() {
                wid2 = n(0);
            }
            let mode = span_cap(&fmt_b, &sp, 6);
            let tk = span_cap(&fmt_b, &sp, 7);
            let mut seq = Value::Undef;
            if pl::str_eq(&tk, "C") {
                // increment sequence number for %C on collision if ':' is used
                if copy.truthy() && pl::str_eq(&dec, ":") {
                    let name = if pl::str_eq(&sign, "-") { "seqFileDir" } else { "seqFileNum" };
                    let v = incr(&lex(it, name));
                    set_lex(it, name, v);
                }
                let sq = lex(it, if pl::str_eq(&sign, "-") { "seqFileDir" } else { "seqFileNum" });
                seq = pl::sub(&pl::add(&wid, &sq), &n(1));
                wid = wid2;
            } else {
                if !(dec.truthy() || copy.truthy()) {
                    continue;
                }
                if pl::lt(&wid, &wid2) {
                    wid = wid2;
                }
                // add dash or underline separator if '-' or '+' specified
                if sign.truthy() {
                    filename.extend_from_slice(&b(&sep(&sign)));
                }
            }
            if mode.truthy() && !pl::str_eq(&mode, "n") {
                let a = if pl::str_eq(&tk, "C") { num2alpha_(&seq)? } else { alpha.clone() };
                let alen = pl::length(&a);
                let mut str_ = if wid.truthy() && pl::gt(&wid, &alen) { Value::bytes(vec![b'a'; pl::sub(&wid, &alen).to_i64().max(0) as usize]) } else { s("") };
                str_ = pl::concat(&str_, &a);
                if pl::str_eq(&mode, "u") {
                    str_ = pl::uc(&str_);
                }
                filename.extend_from_slice(&b(&str_));
            } else {
                let c = if pl::str_eq(&tk, "C") { seq.clone() } else { copy.clone() };
                let num = pl::add(&c, &n(if mode.truthy() { 1 } else { 0 }));
                let piece = if wid.truthy() { pl::sprintf(&format!("%.{}d", wid.to_text()), &[num]) } else { num };
                filename.extend_from_slice(&b(&piece));
            }
        }
        filename.extend_from_slice(&fmt_b[pos.min(fmt_b.len())..]); // add rest of file name
        let filename = Value::bytes(filename);
        // return now with filename unless file exists
        let exists = pl::method(it, &mt, "Exists", vec![filename.clone(), n(1)])?.truthy();
        let used = used_file_name.get(b(&filename));
        if !((exists && used.is_undef()) || used.truthy()) {
            return pl::ret(filename);
        }
        if !okfile.is_undef() {
            if b(&filename) == b(&okfile) {
                return pl::ret(filename);
            }
            let fname = arg(&abs_path(it, vec![filename.clone()], Want::Scalar)?, 0);
            let ok = arg(&abs_path(it, vec![okfile.clone()], Want::Scalar)?, 0);
            if !fname.is_undef() && !ok.is_undef() && b(&fname) == b(&ok) {
                return pl::ret(okfile);
            }
        }
        if !last_file.is_undef() && b(&last_file) == b(&filename) {
            return pl::ret(filename);
        }
        last_file = filename;
        copy = incr(&copy);
        alpha = incr(&alpha);
    }
}

//------------------------------------------------------------------------------
// Create directory for specified file
// Inputs: 0) complete file name including path
// Returns: true if a directory was created
fn create_directory_(it: &mut Interp, file: &Value) -> ER<Value> {
    let mt = lex(it, "mt");
    let err = pl::method(it, &mt, "CreateDirectory", vec![file.clone()])?;
    if !err.is_undef() {
        if err.truthy() {
            error_(it, pl::concat(&err, &s("\n")))?;
            return Ok(n(0));
        }
        if lex(it, "verbose").truthy() {
            let mut dir = file.clone();
            re(r"/[^/]*$", "").subst(&mut dir, false, |_| Vec::new());
            let vout = lex(it, "vout");
            pl::print_to(it, Some(&vout), &[cat(&[b"Created directory ", &b(&dir), b"\n"])])?;
        }
        let c = incr(&lex(it, "countNewDir"));
        set_lex(it, "countNewDir", c);
        return Ok(n(1));
    }
    Ok(n(0))
}

fn create_directory(it: &mut Interp, a: Vec<Value>, _want: Want) -> ER<Vec<Value>> {
    pl::ret(create_directory_(it, &arg(&a, 0))?)
}

//------------------------------------------------------------------------------
// Open output text file
// Inputs: 0) file name format string, 1-N) extra arguments for FilenameSPrintf
// Returns: 0) file reference (or undef on error), 1) file name if opened, 2) append flag
// Notes: returns reference to STDOUT and no file name if no textOut file needed
fn open_output_file(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let file = arg(&a, 0);
    let args: Vec<Value> = a.iter().skip(1).cloned().collect();
    let (mut outfile, mut append) = (Value::Undef, Value::Undef);
    let fp = if lex(it, "textOut").truthy() {
        let text_out = lex(it, "textOut");
        outfile = file.clone();
        clean_filename(it, &mut outfile)?;
        if re(r"%[-+]?\d*[.:]?\d*[lun]?[dDfFeEtgsocC]", "").is_match(&text_out) || !lex(it, "tagOut").is_undef() {
            // make filename from printf-like $textOut
            let mut fa = vec![text_out.clone(), file.clone()];
            fa.extend(args);
            outfile = arg(&filename_sprintf(it, fa, Want::Scalar)?, 0);
            if outfile.is_undef() {
                return pl::ret_nothing(want);
            }
            outfile = arg(&next_unused_filename(it, vec![outfile], Want::Scalar)?, 0);
            create_directory_(it, &outfile)?; // create directory if necessary
        } else {
            re(r"\.[^.\/]*$", "").subst(&mut outfile, false, |_| Vec::new()); // remove extension if it exists
            outfile = pl::concat(&outfile, &text_out);
        }
        let mut mode = ">";
        let mt = lex(it, "mt");
        if pl::method(it, &mt, "Exists", vec![outfile.clone(), n(1)])?.truthy() {
            let text_overwrite = lex(it, "textOverwrite");
            if !text_overwrite.truthy() {
                warn_(it, cat(&[b"Output file ", &b(&outfile), b" already exists for ", &b(&file), b"\n"]))?;
                return pl::ret_nothing(want);
            }
            if pl::num_eq(&text_overwrite, &n(2)) || (pl::num_eq(&text_overwrite, &n(3)) && super::lex_hash(it, "created").get(b(&outfile)).truthy()) {
                mode = ">>";
                append = n(1);
            }
        }
        let fh = builtin1(it, "\\*OUTFILE", &[])?;
        if !pl::method(it, &mt, "Open", vec![fh.clone(), outfile.clone(), s(mode)])?.truthy() {
            let what = if mode == ">" { "creating" } else { "appending to" };
            error_(it, cat(&[b"Error ", what.as_bytes(), b" ", &b(&outfile), b"\n"]))?;
            return pl::ret_nothing(want);
        }
        if lex(it, "binaryOutput").truthy() {
            builtin(it, "binmode(OUTFILE)", &[], &[], Want::Scalar)?;
        }
        fh
    } else {
        builtin1(it, "\\*STDOUT", &[])?
    };
    Ok(vec![fp, outfile, append])
}

//------------------------------------------------------------------------------
// Filter files based on extension
// Inputs: 0) file name
// Returns: 0 = rejected, 1 = specifically accepted, 2 = accepted by default
// Notes: This routine should only be called if $filterFlag is set
fn accept_file_(it: &mut Interp, file: &Value) -> ER<Value> {
    let ext = match re(r"^.*\.(.+)$", "s").m(file) {
        Some(c) => pl::uc(&cap(&c, 1)),
        None => s(""),
    };
    let filter_ext = super::lex_hash(it, "filterExt");
    let v = filter_ext.get(b(&ext));
    if !v.is_undef() {
        return Ok(v);
    }
    let v = filter_ext.get("*");
    if !v.is_undef() {
        return Ok(v);
    }
    if lex(it, "filterFlag").to_i64() & 0x02 != 0 {
        return Ok(n(0)); // reject if accepting specific extensions
    }
    Ok(n(2)) // accept by default
}

fn accept_file(it: &mut Interp, a: Vec<Value>, _want: Want) -> ER<Vec<Value>> {
    pl::ret(accept_file_(it, &arg(&a, 0))?)
}

//------------------------------------------------------------------------------
// Slurp file into buffer
// Inputs: 0) file name, 1) buffer reference
// Returns: 1 on success
fn slurp_file(it: &mut Interp, a: Vec<Value>, _want: Want) -> ER<Vec<Value>> {
    let (file, buff_pt) = (arg(&a, 0), arg(&a, 1));
    let mt = lex(it, "mt");
    let fh = builtin1(it, "\\*INFILE", &[])?;
    if !pl::method(it, &mt, "Open", vec![fh, file.clone()])?.truthy() {
        warn_(it, cat(&[b"Error opening file ", &b(&file), b"\n"]))?;
        return pl::ret(n(0));
    }
    builtin(it, "binmode(INFILE)", &[], &[], Want::Scalar)?;
    // (CAREFUL!:  must clear buffer first to reset possible utf8 flag because the data
    //  would be corrupted if it was read into a buffer which had the utf8 flag set!)
    pl::at(4871);
    pl::sderef(&buff_pt)?;
    pl::set_deref(&buff_pt, Value::Undef)?;
    let mut bsize = 1024 * 1024;
    let mut num = builtin1(it, "read(INFILE, $$buffPt, $bsize)", &[("buffPt", buff_pt.clone()), ("bsize", n(bsize))])?;
    if num.is_undef() {
        builtin(it, "close(INFILE)", &[], &[], Want::Scalar)?;
        warn_(it, cat(&[b"Error reading ", &b(&file), b"\n"]))?;
        return pl::ret(n(0));
    }
    let bmax = 64 * bsize;
    while pl::num_eq(&num, &n(bsize)) {
        if bsize < bmax {
            bsize *= 2;
        }
        let (r, vars) = builtin(it, "read(INFILE, $buff, $bsize)", &[("buff", Value::Undef), ("bsize", n(bsize))], &[], Want::Scalar)?;
        num = r.last().cloned().unwrap_or_default();
        if !num.truthy() {
            break;
        }
        let cur = buff_pt.deref();
        pl::set_deref(&buff_pt, pl::concat(&cur, &vars[0]))?;
    }
    builtin(it, "close(INFILE)", &[], &[], Want::Scalar)?;
    pl::ret(n(1))
}

//------------------------------------------------------------------------------
// Filter argfile line
// Inputs: 0) line of argfile
// Returns: filtered line or undef to ignore
fn filter_argfile_line(_it: &mut Interp, a: Vec<Value>, _want: Want) -> ER<Vec<Value>> {
    let mut arg0 = arg(&a, 0);
    if re(r"^#", "").is_match(&arg0) {
        // comment lines begin with '#'
        if re(r"^#\[CSTR\]", "").subst(&mut arg0, false, |_| Vec::new()) == 0 {
            return pl::ret_undef();
        }
        re(r"[\x0d\x0a]+$", "s").subst(&mut arg0, false, |_| Vec::new()); // remove trailing newline
        // escape double quotes, dollar signs and ampersands if they aren't already
        // escaped by an odd number of backslashes, and escape a single backslash
        // if it occurs at the end of the string
        re(r#"\\(.)|(["\$\@]|\\$)"#, "s").subst(&mut arg0, true, |c| {
            let two = cap(c, 2);
            let v = if two.truthy() { two } else { cap(c, 1) };
            [&b"\\"[..], &b(&v)].concat()
        });
        // un-escape characters in C string
        re(r"\\(.)", "s").subst(&mut arg0, true, |c| {
            let one = cap(c, 1);
            let e: Option<&[u8]> = match &b(&one)[..] {
                b"a" => Some(b"\x07"),
                b"b" => Some(b"\x08"),
                b"f" => Some(b"\x0c"),
                b"n" => Some(b"\n"),
                b"r" => Some(b"\r"),
                b"t" => Some(b"\t"),
                b"\"" => Some(b"\""),
                b"\\" => Some(b"\\"),
                _ => None,
            };
            match e {
                Some(e) => e.to_vec(),
                None => [&b"\\"[..], &b(&one)].concat(),
            }
        });
    } else {
        re(r"^\s+", "").subst(&mut arg0, false, |_| Vec::new()); // remove leading white space
        re(r"[\x0d\x0a]+$", "s").subst(&mut arg0, false, |_| Vec::new()); // remove trailing newline
        // remove white space before, and single space after '=', '+=', '-=' or '<='
        re(r"^(-[-_0-9A-Z:]+#?)\s*([-+<]?=) ?", "i").subst(&mut arg0, false, |c| [&b(&cap(c, 1))[..], &b(&cap(c, 2))].concat());
        if pl::str_eq(&arg0, "") {
            return pl::ret_undef();
        }
    }
    pl::ret(arg0)
}

//------------------------------------------------------------------------------
// Read arguments from -stay_open argfile
// Inputs: 0) argument list ref
// Notes: blocks until -execute, -stay_open or -@ option is available
//        (or until there was an error reading from the file)
fn read_stay_open(it: &mut Interp, a: Vec<Value>, _want: Want) -> ER<Vec<Value>> {
    let args = arg(&a, 0);
    let mut new_args: Vec<Value> = Vec::new();
    let (mut process_args, mut opt_args) = (false, Value::Undef);
    let mut result;
    let mut last_opt = s("");
    let mut unparsed = pl::length(&lex(it, "stayOpenBuff"));
    let opt_args_h = super::lex_hash(it, "optArgs");
    loop {
        if unparsed.truthy() {
            // parse data already read from argfile
            result = unparsed;
            unparsed = Value::Undef;
        } else {
            // read more data from argfile
            // - this read may block (which is good) if reading from a pipe
            result = builtin(it, "sysread(STAYOPEN, $stayOpenBuff, 65536, length($stayOpenBuff))", &[], &["stayOpenBuff"], Want::Scalar)?.0.pop().unwrap_or_default();
        }
        if result.truthy() {
            let mut pos = 0usize;
            let buff = b(&lex(it, "stayOpenBuff"));
            let mut search = 0usize;
            while let Some(i) = buff[search..].iter().position(|&c| c == b'\n') {
                let end = search + i + 1;
                search = end;
                let len = end - pos;
                let line = Value::bytes(&buff[pos..pos + len]);
                pos += len;
                let line = arg(&filter_argfile_line(it, vec![line], Want::Scalar)?, 0);
                if line.is_undef() {
                    continue;
                }
                new_args.push(line.clone());
                if opt_args.truthy() {
                    // this is an argument for the last option
                    opt_args = Value::Undef;
                    if !(pl::str_eq(&last_opt, "-stay_open") || pl::str_eq(&last_opt, "-@")) {
                        continue;
                    }
                } else {
                    last_opt = pl::lc(&line);
                    opt_args = opt_args_h.get(b(&line));
                    if opt_args.is_undef() {
                        opt_args = opt_args_h.get(b(&last_opt));
                        // handle options with trailing numbers
                        if opt_args.is_undef() {
                            if let Some(c) = re(r"^(.*?)\d+(!?)$", "").m(&last_opt) {
                                opt_args = opt_args_h.get(cat(&[&b(&cap(&c, 1)), b"#", &b(&cap(&c, 2))]).to_bytes());
                            }
                        }
                    }
                    if !re(r"^-execute\d*$", "").is_match(&last_opt) {
                        continue;
                    }
                }
                process_args = true;
                break; // process arguments up to this point
            }
            if pos == 0 {
                continue; // nothing to do if we didn't read any arguments
            }
            // keep unprocessed data in buffer
            let rest = pl::substr(&lex(it, "stayOpenBuff"), pos as i64, None);
            set_lex(it, "stayOpenBuff", rest);
            if process_args {
                // process new arguments after -execute or -stay_open option
                for v in new_args.iter().rev() {
                    args.unshift(v.clone());
                }
                break;
            }
        } else if pl::num_eq(&result, &n(0)) {
            // (perl warns here if $result is undef, a sysread error)
            // sysread() didn't block (eg. when reading from a file),
            // so wait for a short time (1/100 sec) then try again
            // Note: may break out of this early if SIGCONT is received
            builtin(it, "select(undef,undef,undef,0.01)", &[], &[], Want::Scalar)?;
        } else {
            warn_(it, s("Error reading from ARGFILE\n"))?;
            builtin(it, "close STAYOPEN", &[], &[], Want::Scalar)?;
            set_lex(it, "stayOpen", n(0));
            break;
        }
    }
    Ok(vec![])
}

//------------------------------------------------------------------------------
// Add new entry to -efile output file
// Inputs: 0) file name, 1) -efile option number (0=error, 1=same, 2=failed, 3=updated, 4=created)
fn efile(it: &mut Interp, a: Vec<Value>, _want: Want) -> ER<Vec<Value>> {
    let entry = arg(&a, 0);
    let i = arg(&a, 1);
    let efile_a = super::lex_array(it, "efile");
    let efile = efile_a.at(if i.truthy() { i.to_i64() } else { 0 });
    if !efile.is_undef() && pl::length(&entry).truthy() && !pl::str_eq(&entry, "-") {
        let mut err = false;
        create_directory_(it, &efile)?;
        let mt = lex(it, "mt");
        let fh = builtin1(it, "\\*EFILE_FILE", &[])?;
        if pl::method(it, &mt, "Open", vec![fh.clone(), efile.clone(), s(">>")])?.truthy() {
            if !pl::print_to(it, Some(&fh), &[entry.clone(), s("\n")])? {
                warn_(it, cat(&[b"Error writing to ", &b(&efile), b"\n"]))?;
                err = true;
            }
            builtin(it, "close EFILE_FILE", &[], &[], Want::Scalar)?;
        } else {
            warn_(it, cat(&[b"Error opening '", &b(&efile), b"' for append\n"]))?;
            err = true;
        }
        if err {
            for (k, v) in efile_a.items().iter().enumerate() {
                if !v.is_undef() && b(v) == b(&efile) {
                    efile_a.set_at(k, Value::Undef);
                }
            }
        }
    }
    Ok(vec![])
}

//------------------------------------------------------------------------------
// Print progress message if it is time for it
// Inputs: 0) file ref, 1) message
fn progress(it: &mut Interp, a: Vec<Value>, _want: Want) -> ER<Vec<Value>> {
    let (file, msg) = (arg(&a, 0), arg(&a, 1));
    let prog_str = lex(it, "progStr");
    if !prog_str.is_undef() {
        pl::print_to(it, Some(&file), &[msg, prog_str, s("\n")])?;
        if !lex(it, "progressMax").is_undef() {
            set_lex(it, "progressNext", Value::Undef);
        }
    }
    Ok(vec![])
}

//------------------------------------------------------------------------------
// Print list of tags
// Inputs: 0) message, 1-N) list of tag names
fn print_tag_list(it: &mut Interp, a: Vec<Value>, _want: Want) -> ER<Vec<Value>> {
    let msg = arg(&a, 0);
    let tags: Vec<Value> = a.iter().skip(1).cloned().collect();
    let quiet = lex(it, "quiet").truthy();
    if !quiet {
        pl::print_to(it, None, &[msg.clone(), s(":\n")])?;
    }
    if (pl::lt(&lex(it, "outFormat"), &n(0)) || lex(it, "verbose").truthy()) && re(r"file extensions$", "").is_match(&msg) && !tags.is_empty() {
        for tag in &tags {
            let mut args = vec![tag.clone()];
            args.extend(pl::call_list(it, "main::GetFileType", vec![tag.clone(), n(1)])?);
            // (perl warns here if GetFileType returns nothing)
            let line = pl::sprintf("  %-11s %s\n", &args);
            pl::print_to(it, None, &[line])?;
        }
        return Ok(vec![]);
    }
    let pad0 = if quiet { "" } else { "  " };
    let (mut len, mut pad) = (0i64, pad0);
    for tag in &tags {
        let taglen = b(tag).len() as i64;
        if len + taglen > 77 {
            pl::print_to(it, None, &[s("\n")])?;
            len = 0;
            pad = pad0;
        }
        pl::print_to(it, None, &[s(pad), tag.clone()])?;
        len += taglen + 1;
        pad = " ";
    }
    if tags.is_empty() {
        pl::print_to(it, None, &[s(pad), s("[empty list]")])?;
    }
    pl::print_to(it, None, &[s("\n")])?;
    Ok(vec![])
}

//------------------------------------------------------------------------------
// Print warnings and errors from info hash
// Inputs: 0) ExifTool object ref, 1) info hash, 2) file name
// Returns: true if there was an Error
fn print_errors(it: &mut Interp, a: Vec<Value>, _want: Want) -> ER<Vec<Value>> {
    let (et, info, file) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    for tag in ["Warning", "Error"] {
        if !info.get(tag).truthy() {
            continue;
        }
        let mut keys = vec![s(tag)];
        if pl::method(it, &et, "Options", vec![s("Duplicates")])?.truthy() {
            let prefix = format!("{tag} ");
            let mut more: Vec<std::rc::Rc<[u8]>> = info.keys().into_iter().filter(|k| k.starts_with(prefix.as_bytes())).collect();
            more.sort();
            keys.extend(more.into_iter().map(|k| Value::bytes(&k[..])));
        }
        for key in keys {
            let v = info.get(b(&key));
            warn_(it, cat(&[tag.as_bytes(), b": ", &b(&v), b" - ", &b(&file), b"\n"]))?;
        }
    }
    pl::ret(info.get("Error"))
}

pub fn register(it: &mut Interp) {
    super::define(it, "main::ProcessFiles", process_files);
    super::define(it, "main::ScanDir", scan_dir);
    super::define(it, "main::FindFileWindows", find_file_windows);
    super::define(it, "main::FileNotFound", file_not_found);
    super::define(it, "main::PreserveTime", preserve_time);
    super::define(it, "main::AbsPath", abs_path);
    super::define(it, "main::MyConvertFileName", my_convert_file_name);
    super::define(it, "main::AddPrintFormat", add_print_format);
    super::define(it, "main::SuggestedExtension", suggested_extension);
    super::define(it, "main::LoadPrintFormat", load_print_format);
    super::define(it, "main::FilenameSPrintf", filename_sprintf);
    super::define(it, "main::Num2Alpha", num2alpha);
    super::define(it, "main::NextUnusedFilename", next_unused_filename);
    super::define(it, "main::CreateDirectory", create_directory);
    super::define(it, "main::OpenOutputFile", open_output_file);
    super::define(it, "main::AcceptFile", accept_file);
    super::define(it, "main::SlurpFile", slurp_file);
    super::define(it, "main::FilterArgfileLine", filter_argfile_line);
    super::define(it, "main::ReadStayOpen", read_stay_open);
    super::define(it, "main::EFile", efile);
    super::define(it, "main::Progress", progress);
    super::define(it, "main::PrintTagList", print_tag_list);
    super::define(it, "main::PrintErrors", print_errors);
}
