//! The `exiftool` script's `GetImageInfo` (script lines 2109-3130): the
//! reading and printing of one file's information, in every output format
//! (or the writing of the file when writing).
//!
//! The sub is one native, `main::GetImageInfo`; its body is split into
//! private functions following the script's own blocks, which share the
//! sub's `my` variables through `St`. Each block function returns
//! `Ok(false)` where the script's `return` ends the sub.

use crate::interp::eval::ER;
use crate::interp::{Flow, Interp, Want};
use crate::perl::Value;
use crate::pl::{self, arg, cap, re};

// ---- local helpers (Perl idioms for the script's code) ----

fn n(i: i64) -> Value {
    Value::Int(i)
}

fn s(x: &str) -> Value {
    Value::str(x)
}

/// `"...$a...$b..."`: the parts interpolated in order.
fn cat(parts: &[&[u8]]) -> Value {
    Value::bytes(parts.concat())
}

/// The script's file lexical `$NAME`, read.
fn sv(it: &mut Interp, name: &str) -> Value {
    super::lex_scalar(it, name).borrow().clone()
}

/// `$NAME = VALUE` on a file lexical.
fn set_sv(it: &mut Interp, name: &str, v: Value) {
    *super::lex_scalar(it, name).borrow_mut() = v;
}

/// `++$NAME` on a file lexical.
fn inc_sv(it: &mut Interp, name: &str) {
    let c = super::lex_scalar(it, name);
    let v = pl::add(&c.borrow(), &n(1));
    *c.borrow_mut() = v;
}

/// The file lexical `@NAME` (an array reference sharing its storage).
fn la(it: &mut Interp, name: &str) -> Value {
    super::lex_array(it, name)
}

/// The file lexical `%NAME` (a hash reference sharing its storage).
fn lh(it: &mut Interp, name: &str) -> Value {
    super::lex_hash(it, name)
}

/// `shift @$a`
fn shift(a: &Value) -> Value {
    match a {
        Value::ArrayRef(r) => {
            let mut v = r.borrow_mut();
            if v.is_empty() {
                Value::Undef
            } else {
                v.remove(0)
            }
        }
        _ => Value::Undef,
    }
}

/// `@$a = LIST`
fn assign(a: &Value, items: Vec<Value>) {
    if let Value::ArrayRef(r) = a {
        *r.borrow_mut() = items;
    }
}

/// `%$h = ()`
fn clear_hash(h: &Value) {
    if let Value::HashRef(r) = h {
        r.borrow_mut().clear();
    }
}

/// `STR x COUNT`
fn rep(x: &[u8], count: &Value) -> Vec<u8> {
    let c = count.to_i64();
    if c <= 0 {
        Vec::new()
    } else {
        x.repeat(c as usize)
    }
}

/// `join SEP, LIST` with the separator held in a value.
fn join_v(sep: &Value, items: &[Value]) -> Value {
    let parts: Vec<Vec<u8>> = items.iter().map(|v| v.to_bytes().to_vec()).collect();
    Value::bytes(parts.join(&sep.to_bytes()[..]))
}

/// `$a eq $b`
fn eq(a: &Value, b: &Value) -> bool {
    a.to_bytes() == b.to_bytes()
}

/// `ref $v eq 'TYPE'`
fn is_ref(v: &Value, t: &str) -> bool {
    pl::ref_of(v) == t
}

/// A script sub (`main::Name`), in scalar context.
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("main::{name}"), args)
}

/// A script sub in list context.
fn call_list(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Vec<Value>> {
    pl::call_list(it, &format!("main::{name}"), args)
}

/// The script's `Warn MSG`.
fn warn_(it: &mut Interp, msg: Value) -> ER<()> {
    call(it, "Warn", vec![msg])?;
    Ok(())
}

/// `EFile(FILE[, NUM])`
fn efile(it: &mut Interp, file: &Value, num: Option<i64>) -> ER<()> {
    let mut a = vec![file.clone()];
    if let Some(x) = num {
        a.push(n(x));
    }
    call(it, "EFile", a)?;
    Ok(())
}

/// `Progress(FH, MSG)`
fn progress(it: &mut Interp, fh: &Value, msg: Value) -> ER<()> {
    call(it, "Progress", vec![fh.clone(), msg])?;
    Ok(())
}

/// The Perl-side helpers for builtins the port cannot call directly
/// (`print $fh LIST`, `close $fh`, `\*STDOUT`, `require`, and the `-if`
/// warning handler that is a closure over the file lexical `$evalWarning`).
/// They are made by one string eval, the first time they are needed, and
/// kept in the interpreter (so a print does not cost a string eval of its
/// own, which would also count in perl's numbering of string evals).
fn helper(it: &mut Interp, i: usize) -> ER<Value> {
    let store = pl::pkg_array(it, "main::<get_image_info>::helpers");
    if store.is_empty() {
        let code: &[u8] = b"(sub { my $fh = shift; print $fh @_ }, sub { close $_[0] }, \\*STDOUT, \\*STDERR, \
            sub { require Image::ExifTool::HTML }, sub { $evalWarning = $_[0] }, sub { $_[0] =~ /(\\d+)/ ? $1 : undef })";
        let scope = crate::runtime::file_scope(it, "main");
        let saved = it.get_scalar("main::@");
        let r = it.eval_string(code, "main", &scope, Want::List);
        it.set_scalar("main::@", saved);
        match r {
            Ok(v) => assign(&store, v),
            Err(e) => return Err(Flow::Die(e)),
        }
    }
    Ok(store.at(i as i64))
}

/// `print $fh LIST`: true on success.
fn print(it: &mut Interp, fh: &Value, items: &[Value]) -> ER<bool> {
    let code = helper(it, 0)?;
    let mut a = vec![fh.clone()];
    a.extend_from_slice(items);
    let r = pl::call_code(it, &code, a, Want::Scalar)?;
    Ok(r.last().is_some_and(|v| v.truthy()))
}

/// `print $fh STR`
fn print1(it: &mut Interp, fh: &Value, x: Value) -> ER<bool> {
    print(it, fh, &[x])
}

/// `close($fh)`
fn close(it: &mut Interp, fh: &Value) -> ER<()> {
    let code = helper(it, 1)?;
    pl::call_code(it, &code, vec![fh.clone()], Want::Scalar)?;
    Ok(())
}

/// `\*STDOUT`
fn stdout(it: &mut Interp) -> ER<Value> {
    helper(it, 2)
}

/// `\*STDERR`
fn stderr(it: &mut Interp) -> ER<Value> {
    helper(it, 3)
}

/// `require Image::ExifTool::HTML;`
fn require_html(it: &mut Interp) -> ER<()> {
    let code = helper(it, 4)?;
    pl::call_code(it, &code, vec![], Want::Scalar)?;
    Ok(())
}

/// `FUNC(\$v)` where FUNC changes `$$v` (CleanXML, FixUTF8): `v` gets the
/// new value.
fn call_ref(it: &mut Interp, name: &str, v: &mut Value) -> ER<()> {
    let r = pl::sref(v.clone());
    pl::call(it, name, vec![r.clone()])?;
    *v = r.deref();
    Ok(())
}

/// The sub's `my` variables shared by the blocks.
struct St {
    et: Value,
    orig: Value,
    found_tags: Value,
    found2: Value,
    info: Value,
    info2: Value,
    et2: Value,
    file: Value,
    file2: Value,
    ind: Value,
    pipe: Value,
    line_count: i64,
    fp: Value,
    outfile: Value,
    append: Value,
}

/// `($fp, $outfile, $append) = OpenOutputFile(ARGS)`
fn open_output_file(it: &mut Interp, st: &mut St, args: Vec<Value>) -> ER<()> {
    let r = call_list(it, "OpenOutputFile", args)?;
    st.fp = arg(&r, 0);
    st.outfile = arg(&r, 1);
    st.append = arg(&r, 2);
    Ok(())
}

pub fn register(it: &mut Interp) {
    super::define(it, "main::GetImageInfo", get_image_info);
}

//------------------------------------------------------------------------------
// Get image information from EXIF data in file (or write file if writing)
// Inputs: 0) ExifTool object reference, 1) file name
fn get_image_info(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let mut st = St {
        et: arg(&a, 0),
        orig: arg(&a, 1),
        found_tags: Value::new_array(Vec::new()),
        found2: Value::new_array(Vec::new()),
        info: Value::Undef,
        info2: Value::Undef,
        et2: Value::Undef,
        file: Value::Undef,
        file2: Value::Undef,
        ind: Value::Undef,
        pipe: Value::Undef,
        line_count: 0,
        fp: Value::Undef,
        outfile: Value::Undef,
        append: Value::Undef,
    };
    body(it, &mut st)?;
    pl::ret_nothing(want)
}

fn body(it: &mut Interp, st: &mut St) -> ER<()> {
    // set window title for this file if necessary
    window_title(it, st)?;
    let orig = st.orig.clone();
    if !(pl::length(&orig).truthy() || sv(it, "outOpt").truthy()) {
        warn_(it, s("Error: Zero-length file name - \"\"\n"))?;
        inc_sv(it, "countBad");
        return Ok(());
    }
    source_file(it, st)?;
    if !unzip(it, st)? {
        return Ok(());
    }
    if !condition(it, st)? {
        return Ok(());
    }
    if !delete_orig(it, st)? {
        return Ok(());
    }
    inc_sv(it, "seqFileNum"); // increment our file counter
    let dir = arg(&pl::call_list(it, "Image::ExifTool::SplitFileName", vec![orig.clone()])?, 0);
    let seq_file_dir = lh(it, "seqFileDir");
    let cur = seq_file_dir.get(dir.to_bytes());
    let v = pl::add(&(if cur.truthy() { cur } else { n(0) }), &n(1));
    seq_file_dir.set(dir.to_bytes(), v.clone());
    set_sv(it, "seqFileDir", v);

    st.line_count = 0;
    if !verbose_out(it, st)? {
        return Ok(());
    }
    if sv(it, "isWriting").truthy() {
        return writing(it, st);
    }
    if !extract(it, st)? {
        return Ok(());
    }
    if !sv(it, "diff").is_undef() {
        return diff(it, st);
    }
    // restore state of comma flag for this file if appending
    if st.append.truthy() && (sv(it, "textOverwrite").to_i64() & 0x02) != 0 {
        let v = lh(it, "outComma").get(st.outfile.to_bytes());
        set_sv(it, "comma", v);
    }

    // print the results for this file
    if !lh(it, "printFmt").is_empty() {
        print_format(it, st)?;
    } else if sv(it, "plot").truthy() {
        plot(it, st)?;
    } else if !sv(it, "disableOutput").truthy() {
        output(it, st)?;
    }
    finish(it, st)
}

/// The window title (`-progress:TITLE`).
fn window_title(it: &mut Interp, st: &mut St) -> ER<()> {
    if sv(it, "windowTitle").is_undef() {
        return Ok(());
    }
    let orig = st.orig.clone();
    if pl::ge(&sv(it, "progressCount"), &sv(it, "progressNext")) {
        let progress_max = sv(it, "progressMax");
        let prog = if progress_max.truthy() {
            cat(&[&sv(it, "progressCount").to_bytes(), b"/", &progress_max.to_bytes()])
        } else {
            s("0/0")
        };
        let mut title = sv(it, "windowTitle");
        let p = pl::split(&re("/", ""), &prog, 0);
        let (num, denom) = (arg(&p, 0), arg(&p, 1));
        let frac = pl::div(&num, &(if denom.truthy() { denom } else { n(1) }))?;
        let mut len = Value::Undef;
        let n_ = if re(r"%(\d+)b", "").subst(&mut title, false, |c| {
            len = cap(c, 1);
            b"%b".to_vec()
        }) > 0
        {
            len
        } else {
            n(20)
        }; // length of bar
        let bar = pl::int(&pl::add(&pl::mul(&frac, &n_), &Value::Num(0.5)));
        let lkup = Value::new_hash();
        let mut b = rep(b"I", &bar);
        b.extend(rep(b".", &pl::sub(&n_, &bar)));
        lkup.set("b", Value::bytes(b));
        lkup.set("f", orig.clone());
        lkup.set("p", pl::int(&pl::add(&pl::mul(&n(100), &frac), &Value::Num(0.5))));
        lkup.set("r", prog.clone());
        lkup.set("%", s("%"));
        re(r"%([%bfpr])", "").subst(&mut title, true, |c| lkup.get(cap(c, 1).to_bytes()).to_bytes().to_vec());
        call(it, "SetWindowTitle", vec![title])?;
        if !progress_max.is_undef() {
            set_sv(it, "progressNext", Value::Undef);
        } else {
            let v = pl::add(&sv(it, "progressNext"), &sv(it, "progressIncr"));
            set_sv(it, "progressNext", v);
        }
    }
    // ($progressMax is not defined for "-progress:%f")
    if sv(it, "progressMax").is_undef() {
        inc_sv(it, "progressCount");
    }
    Ok(())
}

/// The source file name (`-srcfile`) and the alternate files (`-fileNUM`).
fn source_file(it: &mut Interp, st: &mut St) -> ER<()> {
    let (et, orig) = (st.et.clone(), st.orig.clone());
    // determine the name of the source file based on the original input file name
    let src_fmt = la(it, "srcFmt");
    if !src_fmt.is_empty() {
        let mut first = Value::Undef;
        for fmt in src_fmt.items() {
            st.file = if pl::str_eq(&fmt, "@") { orig.clone() } else { call(it, "FilenameSPrintf", vec![fmt.clone(), orig.clone()])? };
            // use this file if it exists
            if pl::method(it, &et, "Exists", vec![st.file.clone()])?.truthy() {
                first = Value::Undef;
                break;
            }
            if sv(it, "verbose").truthy() {
                let vout = sv(it, "vout");
                print1(it, &vout, cat(&[b"Source file ", &st.file.to_bytes(), b" does not exist\n"]))?;
            }
            if first.is_undef() {
                first = st.file.clone();
            }
        }
        if !first.is_undef() {
            st.file = first;
        }
        let df = pl::call_list(it, "Image::ExifTool::SplitFileName", vec![orig.clone()])?;
        let (d, f) = (arg(&df, 0), arg(&df, 1));
        pl::method(it, &et, "Options", vec![s("UserParam"), cat(&[b"OriginalDirectory#=", &d.to_bytes()])])?;
        pl::method(it, &et, "Options", vec![s("UserParam"), cat(&[b"OriginalFileName#=", &f.to_bytes()])])?;
    } else {
        st.file = orig.clone();
    }
    // set alternate file names
    let alt_file = lh(it, "altFile");
    let mut keys = alt_file.keys();
    keys.sort();
    for g8 in keys {
        let mut alt_name = orig.clone();
        let fmt = alt_file.get(&g8[..]);
        if !pl::str_eq(&fmt, "@") {
            // must double any '$' symbols in the original file name because
            // they are used for tag names in a -fileNUM argument
            re(r"\$", "").subst(&mut alt_name, true, |_| b"$$".to_vec());
            alt_name = call(it, "FilenameSPrintf", vec![fmt, alt_name])?;
        }
        pl::method(it, &et, "SetAlternateFile", vec![Value::bytes(&g8[..]), alt_name])?;
    }
    Ok(())
}

/// `-z`: pipe compressed files through gzip or bzip2.
fn unzip(it: &mut Interp, st: &mut St) -> ER<bool> {
    st.pipe = st.file.clone();
    if sv(it, "doUnzip").truthy() {
        // pipe through gzip or bzip2 if necessary
        if let Some(c) = re(r"\.(gz|bz2)$", "i").m(&st.file) {
            let type_ = pl::lc(&cap(&c, 1));
            if re(r"[^-_.'A-Za-z0-9\/\\]", "").is_match(&st.file) {
                warn_(it, s("Error: Insecure zip file name. Skipped\n"))?;
                let file = st.file.clone();
                efile(it, &file, None)?;
                inc_sv(it, "countBad");
                return Ok(false);
            }
            if pl::str_eq(&type_, "gz") {
                st.pipe = cat(&[b"gzip -dc \"", &st.file.to_bytes(), b"\" |"]);
            } else {
                st.pipe = cat(&[b"bzip2 -dc \"", &st.file.to_bytes(), b"\" |"]);
            }
            st.et.set("TRUST_PIPE", n(1));
        }
    }
    Ok(true)
}

/// `-if` conditions (and a `@JSON:` file name).
fn condition(it: &mut Interp, st: &mut St) -> ER<bool> {
    let et = st.et.clone();
    // evaluate -if expression for conditional processing
    let conditions = la(it, "condition");
    if !conditions.is_empty() {
        let mut result = Value::Undef;
        if !sv(it, "failCondition").truthy() {
            // catch run time errors as well as compile errors
            set_sv(it, "evalWarning", Value::Undef);
            let handler = helper(it, 5)?;
            result = pl::local_sig_warn(it, handler, |it| {
                let mut result = Value::Undef;
                let info_h = Value::new_hash();
                // extract information and build expression for evaluation
                let opts = Value::new_hash();
                opts.set("Duplicates", n(1));
                opts.set("RequestTags", la(it, "requestTags"));
                opts.set("Verbose", n(0));
                opts.set("HtmlDump", n(0));
                let fast_condition = sv(it, "fastCondition");
                if !fast_condition.is_undef() {
                    opts.set("FastScan", fast_condition.clone());
                }
                // return all tags but explicitly mention tags on command line so
                // requested images will generate the appropriate warnings
                let tags = la(it, "tags");
                if !tags.is_empty() {
                    let mut v = vec![s("*")];
                    v.extend(tags.items());
                    assign(&st.found_tags, v);
                }
                let is_writing = sv(it, "isWriting");
                let infile = call(it, "Infile", vec![st.pipe.clone(), is_writing])?;
                st.info = pl::method(it, &et, "ImageInfo", vec![infile, st.found_tags.clone(), opts.clone()])?;
                for condition in conditions.items() {
                    let cond = pl::method(it, &et, "InsertTagValues", vec![condition.clone(), st.found_tags.clone(), info_h.clone()])?;
                    {
                        // set package so eval'd functions are in Image::ExifTool namespace
                        let self_ = et.clone();
                        //### eval "-if" condition (%info, $self)
                        let scope = crate::runtime::file_scope(it, "main");
                        let (r, _) = pl::eval_site_in(
                            it,
                            &cond,
                            "Image::ExifTool",
                            Some(scope),
                            &[
                                ("et", et.clone()),
                                ("orig", st.orig.clone()),
                                ("@foundTags", st.found_tags.clone()),
                                ("info", st.info.clone()),
                                ("file", st.file.clone()),
                                ("pipe", st.pipe.clone()),
                                ("result", result.clone()),
                                ("%info", info_h.clone()),
                                ("condition", condition.clone()),
                                ("opts", opts.clone()),
                                ("cond", cond.clone()),
                                ("self", self_),
                            ],
                            Want::Scalar,
                        );
                        if let Some(c) = it.exit_code.take() {
                            return Err(Flow::Exit(c));
                        }
                        result = r;
                        let err = it.get_scalar("main::@");
                        if err.truthy() {
                            set_sv(it, "evalWarning", err);
                        }
                    }
                    let mut eval_warning = sv(it, "evalWarning");
                    if eval_warning.truthy() {
                        // fail condition if warning is issued
                        result = Value::Undef;
                        if sv(it, "verbose").truthy() {
                            re(r"\n$", "").subst(&mut eval_warning, false, |_| Vec::new()); // chomp
                            re(r" at \(eval .*", "s").subst(&mut eval_warning, false, |_| Vec::new());
                            set_sv(it, "evalWarning", eval_warning.clone());
                            warn_(it, cat(&[b"Condition: ", &eval_warning.to_bytes(), b" - ", &st.file.to_bytes(), b"\n"]))?;
                        }
                    }
                    if !result.truthy() {
                        break;
                    }
                }
                if fast_condition.truthy() {
                    assign(&st.found_tags, Vec::new()); // ignore if we didn't get all tags
                }
                Ok(result)
            })?;
        }
        if result.truthy() {
            // discard $info for non-existent file
            if !(pl::str_eq(&st.file, "-") || pl::method(it, &et, "Exists", vec![st.file.clone()])?.truthy()) {
                st.info = Value::Undef;
            }
        } else {
            if sv(it, "verbose").truthy() {
                let vout = sv(it, "vout");
                progress(it, &vout, cat(&[b"-------- ", &st.file.to_bytes(), b" (failed condition)"]))?;
            }
            let file = st.file.clone();
            efile(it, &file, Some(2))?;
            inc_sv(it, "countFailed");
            return Ok(false);
        }
        // can't make use of $info if verbose because we must reprocess
        // the file anyway to generate the verbose output
        // (also if writing just to avoid double-incrementing FileSequence)
        if sv(it, "isWriting").truthy() || sv(it, "verbose").truthy() || !sv(it, "fastCondition").is_undef() || !sv(it, "diff").is_undef() {
            st.info = Value::Undef;
            et.set("FILE_SEQUENCE", pl::sub(&et.get("FILE_SEQUENCE"), &n(1)));
        }
    } else {
        let mut dat = Value::Undef;
        if re(r"^(\@JSON:)(.*)", "").subst(&mut st.file, false, |c| {
            dat = cap(c, 2);
            cap(c, 1).to_bytes().to_vec()
        }) > 0
        {
            // read JSON file from command line
            st.info = pl::method(it, &et, "ImageInfo", vec![pl::sref(dat), st.found_tags.clone()])?;
            if sv(it, "geoOnly").truthy() {
                let info = pl::hderef(&st.info)?;
                for k in info.keys() {
                    if !re("^Geolocation", "").is_match(&Value::bytes(&k[..])) {
                        info.delete(&k[..]);
                    }
                }
                st.file = s(" ");
            }
        }
    }
    Ok(true)
}

/// `-delete_original` / `-restore_original`.
fn delete_orig(it: &mut Interp, st: &mut St) -> ER<bool> {
    let delete_orig = sv(it, "deleteOrig");
    if delete_orig.is_undef() {
        return Ok(true);
    }
    let (et, file) = (st.et.clone(), st.file.clone());
    let vout = sv(it, "vout");
    if !sv(it, "verbose").is_undef() {
        progress(it, &vout, cat(&[b"======== ", &file.to_bytes()]))?;
    }
    inc_sv(it, "count");
    let original = cat(&[&file.to_bytes(), b"_original"]);
    if !pl::method(it, &et, "Exists", vec![original.clone()])?.truthy() {
        return Ok(false);
    }
    if delete_orig.truthy() {
        if sv(it, "verbose").truthy() {
            print1(it, &vout, cat(&[b"Scheduled for deletion: ", &original.to_bytes(), b"\n"]))?;
        }
        la(it, "delFiles").push(original);
    } else if pl::method(it, &et, "Rename", vec![original.clone(), file.clone()])?.truthy() {
        if sv(it, "verbose").truthy() {
            print1(it, &vout, cat(&[b"Restored from ", &original.to_bytes(), b"\n"]))?;
        }
        efile(it, &file, Some(3))?;
        inc_sv(it, "countGoodWr");
    } else {
        warn_(it, cat(&[b"Error renaming ", &original.to_bytes(), b"\n"]))?;
        efile(it, &file, None)?;
        inc_sv(it, "countBad");
    }
    Ok(false)
}

/// The output text file of the verbose and `-csv` outputs with `-w`.
fn verbose_out(it: &mut Interp, st: &mut St) -> ER<bool> {
    let et = st.et.clone();
    if sv(it, "textOut").truthy()
        && (sv(it, "verbose").truthy() || pl::method(it, &et, "Options", vec![s("PrintCSV")])?.truthy())
        && !(sv(it, "tagOut").truthy() || !sv(it, "diff").is_undef() || sv(it, "plot").truthy())
    {
        let orig = st.orig.clone();
        open_output_file(it, st, vec![orig])?;
        if !st.fp.truthy() {
            let file = st.file.clone();
            efile(it, &file, None)?;
            inc_sv(it, "countBad");
            return Ok(false);
        }
        // delete file if we exit prematurely (unless appending)
        if !st.append.truthy() {
            set_sv(it, "tmpText", st.outfile.clone());
        }
        pl::method(it, &et, "Options", vec![s("TextOut"), st.fp.clone()])?;
    }
    Ok(true)
}

/// Writing the file (the sub ends here when writing).
fn writing(it: &mut Interp, st: &mut St) -> ER<()> {
    let (et, file, orig) = (st.et.clone(), st.file.clone(), st.orig.clone());
    if !sv(it, "verbose").is_undef() {
        let vout = sv(it, "vout");
        progress(it, &vout, cat(&[b"======== ", &file.to_bytes()]))?;
    }
    call(it, "SetImageInfo", vec![et.clone(), file.clone(), orig])?;
    st.info = pl::method(it, &et, "GetInfo", vec![s("Warning"), s("Error")])?;
    call(it, "PrintErrors", vec![et.clone(), st.info.clone(), file])?;
    // close output text file if necessary
    if !st.outfile.is_undef() {
        set_sv(it, "tmpText", Value::Undef);
        let fp = st.fp.clone();
        close(it, &fp)?;
        let vout = sv(it, "vout");
        pl::method(it, &et, "Options", vec![s("TextOut"), vout])?;
        let outfile = st.outfile.to_bytes();
        if st.info.get("Error").truthy() {
            pl::method(it, &et, "Unlink", vec![st.outfile.clone()])?; // erase bad file
        } else if st.append.truthy() {
            if !lh(it, "created").get(&outfile).truthy() {
                lh(it, "appended").set(&outfile, n(1));
            }
        } else {
            lh(it, "created").set(&outfile, n(1));
        }
    }
    Ok(())
}

/// Extracting the information (and the `-diff` file's).
fn extract(it: &mut Interp, st: &mut St) -> ER<bool> {
    let et = st.et.clone();
    // extract information from this file
    if !(pl::str_eq(&st.file, "-") || pl::method(it, &et, "Exists", vec![st.file.clone()])?.truthy() || st.info.truthy()) {
        let file = st.file.clone();
        warn_(it, cat(&[b"Error: File not found - ", &file.to_bytes(), b"\n"]))?;
        call(it, "FileNotFound", vec![file.clone()])?;
        if !st.outfile.is_undef() {
            let fp = st.fp.clone();
            close(it, &fp)?;
            set_sv(it, "tmpText", Value::Undef);
            pl::method(it, &et, "Unlink", vec![st.outfile.clone()])?;
        }
        efile(it, &file, None)?;
        inc_sv(it, "countBad");
        return Ok(false);
    }
    // print file/progress message
    let mut o = Value::Undef;
    let print_fmt = lh(it, "printFmt");
    if !(sv(it, "binaryOutput").truthy()
        || sv(it, "textOut").truthy()
        || !print_fmt.is_empty()
        || pl::gt(&sv(it, "html"), &n(1))
        || sv(it, "csv").truthy()
        || sv(it, "plot").truthy())
    {
        if sv(it, "html").truthy() {
            require_html(it)?;
            let f = pl::call(it, "Image::ExifTool::HTML::EscapeHTML", vec![st.file.clone()])?;
            let out = stdout(it)?;
            print1(it, &out, cat(&[b"<!-- ", &f.to_bytes(), b" -->\n"]))?;
        } else if !(sv(it, "json").truthy() || sv(it, "xml").truthy() || !sv(it, "diff").is_undef()) {
            if (sv(it, "multiFile").truthy() && !sv(it, "quiet").truthy()) || sv(it, "progress").truthy() {
                o = stdout(it)?;
            }
        }
    }
    if sv(it, "progress").truthy() && !o.truthy() {
        o = stderr(it)?;
    }
    if o.truthy() {
        progress(it, &o, cat(&[b"======== ", &st.file.to_bytes()]))?;
    }
    if st.info.truthy() {
        // get the information we wanted
        let tags = la(it, "tags");
        if !tags.is_empty() && print_fmt.is_empty() {
            assign(&st.found_tags, tags.items());
            st.info = pl::method(it, &et, "GetInfo", vec![st.found_tags.clone()])?;
        }
    } else {
        // request specified tags unless using print format option
        let old_dups = pl::method(it, &et, "Options", vec![s("Duplicates")])?;
        if !print_fmt.is_empty() {
            pl::method(it, &et, "Options", vec![s("Duplicates"), n(1)])?;
            let request_tags = la(it, "requestTags");
            pl::method(it, &et, "Options", vec![s("RequestTags"), request_tags])?;
            if print_fmt.get("SetTags").truthy() {
                // initialize options so we can set any tags we want
                et.set("TAGS_FROM_FILE", n(1));
                pl::method(it, &et, "Options", vec![s("MakerNotes"), n(1)])?;
                pl::method(it, &et, "Options", vec![s("Struct"), n(2)])?;
                pl::method(it, &et, "Options", vec![s("List"), n(1)])?;
                if !pl::method(it, &et, "Options", vec![s("CoordFormat")])?.truthy() {
                    pl::method(it, &et, "Options", vec![s("CoordFormat"), s("%d %d %.8f")])?;
                }
            }
        } else {
            let tags = la(it, "tags");
            assign(&st.found_tags, tags.items());
        }
        let diff = sv(it, "diff");
        if !diff.is_undef() {
            st.file2 = call(it, "FilenameSPrintf", vec![diff, st.orig.clone()])?;
            if eq(&st.file, &st.file2) {
                warn_(it, cat(&[b"Error: Diffing file with itself - ", &st.file2.to_bytes(), b"\n"]))?;
                let file = st.file.clone();
                efile(it, &file, None)?;
                inc_sv(it, "countBad");
                return Ok(false);
            }
            if pl::method(it, &et, "Exists", vec![st.file2.clone()])?.truthy() {
                if sv(it, "showGroup").is_undef() {
                    set_sv(it, "showGroup", n(1));
                }
                if sv(it, "allGroup").is_undef() {
                    set_sv(it, "allGroup", n(1));
                }
                let show_group = sv(it, "showGroup");
                pl::method(
                    it,
                    &et,
                    "Options",
                    vec![s("Duplicates"), n(1), s("Sort"), cat(&[b"Group", &show_group.to_bytes()]), s("Verbose"), n(0)],
                )?;
                st.et2 = pl::method(it, &s("Image::ExifTool"), "new", vec![])?;
                let et2 = st.et2.clone();
                let opts = pl::hderef(&et.get("OPTIONS"))?;
                let mut flat = Vec::new();
                for k in opts.keys() {
                    flat.push(Value::bytes(&k[..]));
                    flat.push(opts.get(&k[..]));
                }
                pl::method(it, &et2, "Options", flat)?;
                // must set list options specifically because they may have been
                // set incorrectly from deprecated List settings
                pl::method(it, &et2, "Options", vec![s("ListSep"), et.path(&["OPTIONS", "ListSep"])])?;
                pl::method(it, &et2, "Options", vec![s("ListSplit"), et.path(&["OPTIONS", "ListSplit"])])?;
                assign(&st.found2, st.found_tags.items());
                st.info2 = pl::method(it, &et2, "ImageInfo", vec![st.file2.clone(), st.found2.clone()])?;
            } else {
                let h = Value::new_hash();
                h.set("Error", s("Diff file not found"));
                st.info2 = h;
            }
            let err = st.info2.get("Error");
            if err.truthy() {
                warn_(it, cat(&[b"Error: ", &err.to_bytes(), b" - ", &st.file2.to_bytes(), b"\n"]))?;
                let file = st.file.clone();
                efile(it, &file, None)?;
                inc_sv(it, "countBad");
                return Ok(false);
            }
        }
        // extract the information
        let infile = call(it, "Infile", vec![st.pipe.clone()])?;
        st.info = pl::method(it, &et, "ImageInfo", vec![infile, st.found_tags.clone()])?;
        pl::method(it, &et, "Options", vec![s("Duplicates"), old_dups])?;
    }

    // all done now if we already wrote output text file (eg. verbose option)
    if st.fp.truthy() {
        if !st.outfile.is_undef() {
            let out = stdout(it)?;
            pl::method(it, &et, "Options", vec![s("TextOut"), out])?;
            set_sv(it, "tmpText", Value::Undef);
            if st.info.get("Error").truthy() {
                let fp = st.fp.clone();
                close(it, &fp)?;
                pl::method(it, &et, "Unlink", vec![st.outfile.clone()])?; // erase bad file
            } else {
                st.line_count += 1; // output text file (likely) is not empty
            }
        }
        let err = st.info.get("Error");
        if err.truthy() {
            warn_(it, cat(&[b"Error: ", &err.to_bytes(), b" - ", &st.file.to_bytes(), b"\n"]))?;
            let file = st.file.clone();
            efile(it, &file, None)?;
            inc_sv(it, "countBad");
            return Ok(false);
        }
    }

    // print warnings to stderr if using binary output
    // (because we are likely ignoring them and piping stdout to file)
    // or if there is none of the requested information available
    pl::at(2383);
    if sv(it, "binaryOutput").truthy() || pl::hderef(&st.info)?.is_empty() {
        let errs = pl::method(it, &et, "GetInfo", vec![s("Warning"), s("Error")])?;
        if call(it, "PrintErrors", vec![et.clone(), errs, st.file.clone()])?.truthy() {
            let file = st.file.clone();
            efile(it, &file, None)?;
            set_sv(it, "rtnVal", n(1));
        }
    } else if pl::method(it, &et, "GetValue", vec![s("Error")])?.truthy()
        || (et.get("Validate").truthy() && pl::method(it, &et, "GetValue", vec![s("Warning")])?.truthy())
    {
        set_sv(it, "rtnVal", n(1));
    }

    // open output file (or stdout if no output file) if not done already
    if !(!st.outfile.is_undef() || sv(it, "tagOut").truthy()) {
        let orig = st.orig.clone();
        open_output_file(it, st, vec![orig])?;
        if !st.fp.truthy() {
            let file = st.file.clone();
            efile(it, &file, None)?;
            inc_sv(it, "countBad");
            return Ok(false);
        }
        if !st.outfile.is_undef() && !st.append.truthy() {
            set_sv(it, "tmpText", st.outfile.clone());
        }
    }
    Ok(true)
}

/// A `-diff` line: `sprintf "%s %s%s%s: %s\n"`-like, with the value made
/// printable.
fn diff_line(it: &mut Interp, fmt: &str, args: Vec<Value>, val: &Value) -> ER<Value> {
    let p = call_list(it, "Printable", vec![val.clone()])?;
    let mut a = args;
    a.extend(p);
    Ok(pl::sprintf(fmt, &a))
}

/// Print differences (`-diff`); the sub ends here.
fn diff(it: &mut Interp, st: &mut St) -> ER<()> {
    let (et, et2, fp) = (st.et.clone(), st.et2.clone(), st.fp.clone());
    let done = Value::new_hash();
    let done2 = Value::new_hash();
    let mut was_diff = false;
    let mut diffs: Vec<Value> = Vec::new();
    let mut group_tags2: Vec<Value> = Vec::new();
    let v = {
        let x = sv(it, "verbose");
        if x.truthy() {
            x
        } else {
            n(0)
        }
    };
    print1(it, &fp, cat(&[b"======== diff < ", &st.file.to_bytes(), b" > ", &st.file2.to_bytes(), b"\n"]))?;
    // start with $g2 false, but not equal to '' to avoid infinite loop
    let (mut g2, mut same) = (n(0), 0i64);
    loop {
        let mut g = Value::Undef;
        let mut tag2 = Value::Undef;
        let mut val2 = Value::Undef;
        let mut equal = false;
        let used = Value::new_hash();
        let mut dupl: Vec<Value> = Vec::new();
        let tag = shift(&st.found_tags);
        let show_group = sv(it, "showGroup");
        let out_format = sv(it, "outFormat");
        let all_group = sv(it, "allGroup").truthy();
        if !tag.is_undef() {
            done.set(tag.to_bytes(), n(1));
            g = pl::method(it, &et, "GetGroup", vec![tag.clone(), show_group.clone()])?;
        }
        if !g.truthy() {
            // ($g could be '' if requesting non-existent tag, and this will sort last)
            loop {
                tag2 = shift(&st.found2);
                if tag2.is_undef() {
                    g = s("");
                    break;
                }
                if !done2.get(tag2.to_bytes()).truthy() {
                    g = pl::method(it, &et2, "GetGroup", vec![tag2.clone(), show_group.clone()])?;
                    break;
                }
            }
        }
        if !eq(&g, &g2) {
            // add any outstanding tags from diff file not yet handled in previous group ($g2)
            for t2 in group_tags2.clone() {
                if done2.get(t2.to_bytes()).truthy() {
                    continue;
                }
                let val2 = pl::method(it, &et2, "GetValue", vec![t2.clone()])?;
                if val2.is_undef() {
                    continue;
                }
                let name = if pl::lt(&out_format, &n(1)) {
                    pl::method(it, &et2, "GetDescription", vec![t2.clone()])?
                } else {
                    call(it, "GetTagName", vec![t2.clone()])?
                };
                let len = call(it, "LengthUTF8", vec![name.clone()])?;
                let pad = if pl::lt(&out_format, &n(2)) {
                    Value::bytes(rep(b" ", &(if pl::lt(&len, &n(32)) { pl::sub(&n(32), &len) } else { n(0) })))
                } else {
                    s("")
                };
                if all_group {
                    let mut grp = cat(&[b"[", &g2.to_bytes(), b"]"]);
                    let gl = grp.to_bytes().len() as i64;
                    if gl < 15 && pl::lt(&out_format, &n(2)) {
                        grp = cat(&[&grp.to_bytes(), &rep(b" ", &n(15 - gl))]);
                    }
                    diffs.push(diff_line(it, "> %s %s%s: %s\n", vec![grp, name, pad], &val2)?);
                } else {
                    diffs.push(diff_line(it, "> %s%s: %s\n", vec![name, pad], &val2)?);
                }
                done2.set(t2.to_bytes(), n(1));
            }
            let mut str_ = s("");
            if v.truthy() && (same != 0 || pl::gt(&v, &n(1))) {
                str_ = Value::from(format!("  ({same} same tag{})", if same == 1 { "" } else { "s" }));
            }
            if !all_group {
                if g2.truthy() && (str_.truthy() || !diffs.is_empty()) {
                    print1(it, &fp, cat(&[b"---- ", &g2.to_bytes(), b" ----", &str_.to_bytes(), b"\n"]))?;
                }
            } else if str_.truthy() && g2.truthy() {
                let line = pl::sprintf("   %-13s%s\n", &[g2.clone(), str_.clone()]);
                print1(it, &fp, line)?;
            }
            // print all differences for this group
            if !diffs.is_empty() {
                print(it, &fp, &diffs)?;
                was_diff = true;
                diffs.clear();
            }
            if !g.truthy() {
                break;
            }
            g2 = g.clone();
            same = 0;
            // build list of all tags in the new group of the diff file
            group_tags2.clear();
            if !tag2.is_undef() {
                group_tags2.push(tag2.clone());
            }
            for t2 in st.found2.items() {
                if done2.get(t2.to_bytes()).truthy() {
                    continue;
                }
                if !eq(&g, &pl::method(it, &et2, "GetGroup", vec![t2.clone(), show_group.clone()])?) {
                    continue;
                }
                group_tags2.push(t2);
            }
        }
        if tag.is_undef() {
            continue;
        }
        let val = pl::method(it, &et, "GetValue", vec![tag.clone()])?;
        if val.is_undef() {
            continue; // (just in case)
        }
        let name = call(it, "GetTagName", vec![tag.clone()])?;
        let mut desc = if pl::lt(&out_format, &n(1)) { pl::method(it, &et, "GetDescription", vec![tag.clone()])? } else { name.clone() };
        // get matching tag key(s) from diff file
        let mut pat = name.to_bytes().to_vec();
        pat.extend_from_slice(b"( |$)");
        let pat = pl::re_dyn(&[b"^", &pat[..]].concat(), "")?;
        let tags2: Vec<Value> = group_tags2.iter().filter(|t| pat.is_match(t)).cloned().collect();
        't2: for t2 in tags2 {
            if done2.get(t2.to_bytes()).truthy() {
                continue;
            }
            tag2 = t2.clone();
            val2 = pl::method(it, &et2, "GetValue", vec![t2.clone()])?;
            if val2.is_undef() {
                continue;
            }
            if call(it, "IsEqual", vec![val.clone(), val2.clone()])?.truthy() {
                equal = true;
                break;
            }
            // look ahead for upcoming duplicate tags in this group to see
            // if any would later match this value (and skip those for now)
            let dupl_tag = et.get_or_new_hash("DUPL_TAG");
            let nd = dupl_tag.get(name.to_bytes());
            if nd.truthy() && dupl.is_empty() {
                let mut i = 0i64;
                let mut key = name.clone();
                while pl::le(&n(i), &dupl_tag.get(name.to_bytes())) {
                    if !(done.get(key.to_bytes()).truthy()
                        || !eq(&g, &pl::method(it, &et, "GetGroup", vec![key.clone(), show_group.clone()])?))
                    {
                        dupl.push(key.clone());
                    }
                    i += 1;
                    key = cat(&[&name.to_bytes(), b" (", i.to_string().as_bytes(), b")"]);
                }
                if dupl.len() > 1 {
                    let order = et.get_or_new_hash("FILE_ORDER");
                    dupl.sort_by(|a, b| {
                        crate::perl::ops::num_cmp(&order.get(a.to_bytes()), &order.get(b.to_bytes())).unwrap_or(std::cmp::Ordering::Equal)
                    });
                }
            }
            let mut skip = false;
            pl::local_topic(it, |it| {
                for d in dupl.iter() {
                    it.set_scalar("main::_", d.clone());
                    if used.get(d.to_bytes()).truthy() {
                        continue;
                    }
                    let v = pl::method(it, &et, "GetValue", vec![d.clone()])?;
                    if !(!v.is_undef() && call(it, "IsEqual", vec![v, val2.clone()])?.truthy()) {
                        continue;
                    }
                    used.set(d.to_bytes(), n(1)); // would match this upcoming tag
                    skip = true;
                    return Ok(());
                }
                Ok(())
            })?;
            if skip {
                tag2 = Value::Undef;
                val2 = Value::Undef;
                continue 't2;
            }
            break;
        }
        if equal {
            same += 1;
        } else {
            let len = call(it, "LengthUTF8", vec![desc.clone()])?;
            let pad = if pl::lt(&out_format, &n(2)) {
                Value::bytes(rep(b" ", &(if pl::lt(&len, &n(32)) { pl::sub(&n(32), &len) } else { n(0) })))
            } else {
                s("")
            };
            if all_group {
                let mut grp = cat(&[b"[", &g.to_bytes(), b"]"]);
                let gl = grp.to_bytes().len() as i64;
                if gl < 15 && pl::lt(&out_format, &n(2)) {
                    grp = cat(&[&grp.to_bytes(), &rep(b" ", &n(15 - gl))]);
                }
                diffs.push(diff_line(it, "< %s %s%s: %s\n", vec![grp.clone(), desc.clone(), pad.clone()], &val)?);
                if !val2.is_undef() {
                    if pl::lt(&v, &n(3)) {
                        grp = Value::bytes(rep(b" ", &n(grp.to_bytes().len() as i64)));
                        desc = Value::bytes(rep(b" ", &len));
                    }
                    diffs.push(diff_line(it, "> %s %s%s: %s\n", vec![grp, desc, pad], &val2)?);
                }
            } else {
                diffs.push(diff_line(it, "< %s%s: %s\n", vec![desc.clone(), pad.clone()], &val)?);
                if pl::lt(&v, &n(3)) {
                    desc = Value::bytes(rep(b" ", &len));
                }
                if !val2.is_undef() {
                    diffs.push(diff_line(it, "> %s%s: %s\n", vec![desc, pad], &val2)?);
                }
            }
        }
        if !tag2.is_undef() {
            done2.set(tag2.to_bytes(), n(1));
        }
    }
    if !was_diff {
        print1(it, &fp, s("(no metadata differences)\n"))?;
    }
    if !st.outfile.is_undef() {
        lh(it, "created").set(st.outfile.to_bytes(), n(1));
        close(it, &fp)?;
        set_sv(it, "tmpText", Value::Undef);
    }
    inc_sv(it, "count");
    Ok(())
}

/// Output using print format file (-p) option.
fn print_format(it: &mut Interp, st: &mut St) -> ER<()> {
    let (et, fp) = (st.et.clone(), st.fp.clone());
    let print_fmt = lh(it, "printFmt");
    let mut grp = Value::Undef;
    let mut cache = Value::Undef;
    set_sv(it, "fileTrailer", s(""));
    // repeat for each embedded document if necessary (only if -ee used)
    let last_doc = if pl::method(it, &et, "Options", vec![s("ExtractEmbedded")])?.truthy() {
        // (cache tag keys if there are sub-documents)
        let l = et.get("DOC_COUNT");
        if l.truthy() {
            cache = Value::new_hash();
        }
        l
    } else {
        n(0)
    };
    let mut doc: Vec<Value> = vec![n(0)];
    while pl::le(&doc[0], &last_doc) {
        let doc_s = pl::join("-", &doc);
        let mut skip_body = false;
        for type_ in ["HEAD", "SECT", "IF", "BODY", "ENDS", "TAIL"] {
            let prf = print_fmt.get(type_);
            if !prf.truthy() {
                continue;
            }
            if type_ == "HEAD" && !st.outfile.is_undef() {
                let wrote_head = lh(it, "wroteHEAD");
                if wrote_head.get(st.outfile.to_bytes()).truthy() {
                    continue;
                }
                wrote_head.set(st.outfile.to_bytes(), n(1));
            }
            if type_ == "BODY" && skip_body {
                continue;
            }
            // silence "IF" warnings and warnings for subdocuments > 1
            let opt = if type_ == "IF" || ((pl::gt(&doc[0], &n(1)) || doc.len() > 1) && !et.path(&["OPTIONS", "IgnoreMinorErrors"]).truthy()) {
                s("Silent")
            } else {
                s("Warn")
            };
            if last_doc.truthy() {
                if doc_s.truthy() {
                    if type_ == "HEAD" || type_ == "TAIL" {
                        continue; // only repeat SECT/IF/BODY/ENDS
                    }
                    grp = cat(&[b"Doc", &doc_s.to_bytes()]);
                } else {
                    grp = s("Main");
                }
            }
            let mut lines: Vec<Value> = Vec::new();
            let items = pl::aderef(&prf)?;
            pl::local_topic(it, |it| {
                for x in items {
                    it.set_scalar("main::_", x.clone());
                    let line = pl::method(it, &et, "InsertTagValues", vec![x, st.found_tags.clone(), opt.clone(), grp.clone(), cache.clone()])?;
                    if type_ == "IF" {
                        if line.is_undef() {
                            skip_body = true;
                        }
                    } else if !line.is_undef() {
                        lines.push(line);
                    }
                }
                Ok(())
            })?;
            st.line_count += lines.len() as i64;
            if type_ == "SECT" {
                let this_header = pl::join("", &lines);
                let sect_header = sv(it, "sectHeader");
                if sect_header.truthy() && !eq(&sect_header, &this_header) {
                    let sect_trailer = sv(it, "sectTrailer");
                    if sect_trailer.truthy() {
                        print1(it, &fp, sect_trailer)?;
                    }
                    set_sv(it, "sectHeader", Value::Undef);
                }
                set_sv(it, "sectTrailer", s(""));
                if !sv(it, "sectHeader").truthy() {
                    set_sv(it, "sectHeader", this_header.clone());
                    print1(it, &fp, this_header)?;
                }
            } else if type_ == "ENDS" {
                if !sv(it, "sectHeader").is_undef() {
                    let v = pl::concat(&sv(it, "sectTrailer"), &pl::join("", &lines));
                    set_sv(it, "sectTrailer", v);
                }
            } else if type_ == "TAIL" {
                let v = pl::concat(&sv(it, "fileTrailer"), &pl::join("", &lines));
                set_sv(it, "fileTrailer", v);
            } else if !lines.is_empty() {
                print(it, &fp, &lines)?;
            }
        }
        // find next available doc-subdoc
        doc.push(n(1));
        let has_doc = et.get_or_new_hash("HAS_DOC");
        while doc.len() > 1 {
            let next_doc = pl::join("-", &doc);
            if has_doc.get(next_doc.to_bytes()).truthy() {
                break;
            }
            doc.pop();
            let l = doc.len() - 1;
            doc[l] = pl::add(&doc[l], &n(1));
        }
    }
    if st.outfile.is_undef() {
        print_fmt.delete("HEAD"); // print header only once per output file
    }
    let errs = pl::method(it, &et, "GetInfo", vec![s("Warning"), s("Error")])?;
    if call(it, "PrintErrors", vec![et.clone(), errs, st.file.clone()])?.truthy() {
        let file = st.file.clone();
        efile(it, &file, None)?;
    }
    Ok(())
}

/// Add points from this file to the plot (`-plot`).
fn plot(it: &mut Interp, st: &mut St) -> ER<()> {
    let et = st.et.clone();
    let plot = sv(it, "plot");
    let tag_extra = et.get_or_new_hash("TAG_EXTRA");
    let doc_num = Value::new_hash();
    let get_num = helper(it, 6)?;
    pl::at(2624);
    for tag in pl::hderef(&st.info)?.keys() {
        let ex = tag_extra.get(&tag[..]);
        if !(ex.truthy() && ex.get("G3").truthy()) {
            continue;
        }
        let r = pl::call_code(it, &get_num, vec![ex.get("G3")], Want::Scalar)?.pop().unwrap_or_default();
        if !r.is_undef() {
            doc_num.set(&tag[..], r);
        }
    }
    plot.set("DocNum", doc_num);
    if pl::method(it, &et, "Options", vec![s("ExtractEmbedded")])?.truthy() {
        plot.set("EE", n(1));
    }
    pl::method(it, &plot, "AddPoints", vec![st.info.clone(), st.found_tags.clone()])?;
    Ok(())
}

/// The value of one tag for the normal outputs (script lines 2726-2765):
/// `None` for the script's `next` (skip the tag).
fn tag_value(it: &mut Interp, st: &St, tag: &Value, val_list: &mut Value) -> ER<Option<Value>> {
    let et = &st.et;
    let mut val = st.info.get(tag.to_bytes());
    let binary_output = sv(it, "binaryOutput");
    // set flag if this is binary data
    set_sv(it, "isBinary", Value::from(is_ref(&val, "SCALAR") && !binary_output.is_undef()));
    if val.is_ref() {
        // happens with -X, -j or -php when combined with -b:
        if !binary_output.is_undef() && !binary_output.truthy() && et.path(&["TAG_INFO"]).get(tag.to_bytes()).get("Protected").truthy() {
            // avoid extracting Unsafe binary tags (eg. data blocks) [insider information]
            let mut lc_tag = pl::lc(tag);
            re(" .*", "").subst(&mut lc_tag, false, |_| Vec::new());
            let ra = et.path(&["OPTIONS", "RequestAll"]);
            if !(et.path(&["REQ_TAG_LOOKUP"]).get(lc_tag.to_bytes()).truthy() || pl::gt(&(if ra.truthy() { ra } else { n(0) }), &n(2))) {
                return Ok(None);
            }
        }
        val = call(it, "ConvertBinary", vec![val])?; // convert SCALAR references
        if val.is_undef() {
            return Ok(None);
        }
        if sv(it, "structOpt").truthy() && val.is_ref() {
            // serialize structure if necessary
            if !(sv(it, "xml").truthy() || sv(it, "json").truthy()) {
                val = pl::call(it, "Image::ExifTool::XMP::SerializeStruct", vec![et.clone(), val])?;
            }
        } else if is_ref(&val, "ARRAY") {
            let list_item = sv(it, "listItem");
            if !list_item.is_undef() {
                // take only the specified item
                val = val.at(list_item.to_i64());
            // join arrays of simple values (with newlines for binary output)
            } else if binary_output.truthy() {
                if sv(it, "tagOut").truthy() {
                    *val_list = val.clone();
                    val = shift(val_list);
                } else {
                    let bin_sep = sv(it, "binSep");
                    val = join_v(&(if bin_sep.is_undef() { s("\n") } else { bin_sep }), &pl::aderef(&val)?);
                }
            } else if sv(it, "joinLists").truthy() {
                let list_sep = sv(it, "listSep");
                val = join_v(&list_sep, &pl::aderef(&val)?);
            }
        }
    }
    if val.is_undef() {
        // ignore tags that weren't found unless necessary
        if binary_output.truthy() {
            return Ok(None);
        }
        let force_print = sv(it, "forcePrint");
        if !force_print.is_undef() {
            val = force_print; // forced to print all tag values
        } else if !sv(it, "csv").truthy() {
            return Ok(None);
        }
    }
    Ok(Some(val))
}

/// The state of the normal outputs (the `my` variables of the block).
struct Out {
    line: Value,
    no_dups: Value,
    csv_info: Value,
    bra: Value,
    ket: Value,
    sep: Value,
    quote: Value,
}

/// The normal output formats (script lines 2633-2995).
fn output(it: &mut Interp, st: &mut St) -> ER<()> {
    let et = st.et.clone();
    let mut o = Out {
        line: Value::Undef,
        no_dups: Value::new_hash(),
        csv_info: Value::new_hash(),
        bra: Value::Undef,
        ket: Value::Undef,
        sep: Value::Undef,
        quote: Value::Undef,
    };
    let html = sv(it, "html").truthy();
    let xml = sv(it, "xml").truthy();
    let json = sv(it, "json");
    let csv = sv(it, "csv").truthy();
    if st.fp.truthy() {
        let fp = st.fp.clone();
        // print file header (only once)
        let file_header = sv(it, "fileHeader");
        if file_header.truthy() {
            let ob = st.outfile.to_bytes();
            if !(!st.outfile.is_undef() && (lh(it, "created").get(&ob).truthy() || lh(it, "appended").get(&ob).truthy())) {
                print1(it, &fp, file_header)?;
            }
            if !sv(it, "textOut").truthy() {
                set_sv(it, "fileHeader", Value::Undef);
            }
        }
        if html {
            print1(it, &fp, s("<table>\n"))?;
        } else if xml {
            xml_header(it, st)?;
        } else if json.truthy() {
            // set delimiters for JSON or PHP output
            if pl::num_eq(&json, &n(1)) {
                (o.bra, o.ket, o.sep) = (s("{"), s("}"), s(":"));
            } else {
                (o.bra, o.ket, o.sep) = (s("Array("), s(")"), s(" =>"));
            }
            let sf = et.path(&["OPTIONS", "StructFormat"]);
            if sf.truthy() && pl::str_eq(&sf, "JSONQ") {
                o.quote = n(1);
            }
            if sv(it, "comma").truthy() {
                print1(it, &fp, s(",\n"))?;
            }
            let conv = call(it, "MyConvertFileName", vec![et.clone(), st.file.clone()])?;
            let esc = call(it, "EscapeJSON", vec![conv, n(1)])?;
            print(it, &fp, &[cat(&[&o.bra.to_bytes(), b"\n  \"SourceFile\"", &o.sep.to_bytes(), b" "]), esc])?;
            set_sv(it, "comma", n(1));
            st.ind = if !sv(it, "showGroup").is_undef() && !sv(it, "allGroup").truthy() { s("    ") } else { s("  ") };
        } else if csv {
            let file2 = call(it, "MyConvertFileName", vec![et.clone(), st.file.clone()])?;
            lh(it, "database").set(file2.to_bytes(), o.csv_info.clone());
            la(it, "csvFiles").push(file2);
        }
    }
    // suppress duplicates manually in JSON and short XML output
    let no_dups = json.truthy() || (xml && pl::gt(&sv(it, "outFormat"), &n(0)));
    let print_conv = pl::method(it, &et, "Options", vec![s("PrintConv")])?;
    let mut last_group = s("");
    let mut i: i64 = -1;
    for tag in st.found_tags.items() {
        i += 1; // keep track on index in @foundTags
        output_tag(it, st, &mut o, &tag, i, no_dups, &print_conv, &mut last_group)?;
    }
    if st.fp.truthy() {
        let fp = st.fp.clone();
        let out_format = sv(it, "outFormat");
        if html {
            print1(it, &fp, s("</table>\n"))?;
        } else if xml {
            // close rdf:Description element
            print1(it, &fp, s(if pl::lt(&out_format, &n(1)) { "\n</rdf:Description>\n" } else { "/>\n" }))?;
        } else if json.truthy() {
            if last_group.truthy() {
                print1(it, &fp, cat(&[b"\n  ", &o.ket.to_bytes()]))?;
            }
            print1(it, &fp, cat(&[b"\n", &o.ket.to_bytes()]))?;
            set_sv(it, "comma", n(1));
        } else if sv(it, "tabFormat").truthy() && pl::gt(&out_format, &n(1)) && !o.line.is_undef() {
            print1(it, &fp, cat(&[&o.line.to_bytes(), b"\n"]))?;
        }
    }
    Ok(())
}

/// The `rdf:Description` start of the `-X` output.
fn xml_header(it: &mut Interp, st: &mut St) -> ER<()> {
    let (et, fp) = (st.et.clone(), st.fp.clone());
    let mut f = st.file.clone();
    call_ref(it, "main::CleanXML", &mut f)?;
    print1(it, &fp, cat(&[b"\n<rdf:Description rdf:about='", &f.to_bytes(), b"'"]))?;
    print1(it, &fp, s("\n  xmlns:et='http://ns.exiftool.org/1.0/'"))?;
    let version = it.get_scalar("Image::ExifTool::VERSION");
    print1(it, &fp, cat(&[b" et:toolkit='Image::ExifTool ", &version.to_bytes(), b"'"]))?;
    // define namespaces for all tag groups
    let groups = Value::new_hash();
    let groups_a = Value::new_array(Vec::new());
    let force_print = sv(it, "forcePrint");
    for tag in st.found_tags.items() {
        let g = pl::method_list(it, &et, "GetGroup", vec![tag.clone()])?;
        let (mut grp0, mut grp1) = (arg(&g, 0), arg(&g, 1));
        if !grp1.truthy() {
            if force_print.is_undef() {
                continue;
            }
            grp0 = s("Unknown");
            grp1 = s("Unknown");
        }
        // add groups from structure fields
        let v = st.info.get(tag.to_bytes());
        if v.is_ref() {
            call(it, "AddGroups", vec![v, grp0.clone(), groups.clone(), groups_a.clone()])?;
        }
        if groups.get(grp1.to_bytes()).truthy() {
            continue;
        }
        // include family 0 and 1 groups in URI except for internal tags
        // (this will put internal tags in the "XML" group on readback)
        groups.set(grp1.to_bytes(), grp0);
        groups_a.push(grp1);
    }
    for grp1 in groups_a.items() {
        let mut grp = groups.get(grp1.to_bytes());
        if !(eq(&grp, &grp1) && re("^(ExifTool|File|Composite|Unknown)$", "").is_match(&grp)) {
            grp = cat(&[&grp.to_bytes(), b"/", &grp1.to_bytes()]);
        }
        print1(
            it,
            &fp,
            cat(&[b"\n  xmlns:", &grp1.to_bytes(), b"='http://ns.exiftool.org/", &grp.to_bytes(), b"/1.0/'"]),
        )?;
    }
    let out_format = sv(it, "outFormat");
    if pl::lt(&out_format, &n(1)) {
        print1(it, &fp, s(">"))?; // finish rdf:Description token unless short format
    }
    st.ind = if pl::ge(&out_format, &n(0)) { s(" ") } else { s("   ") };
    Ok(())
}

/// Mark the output file of `-W` as written, and close it.
fn close_tag_out(it: &mut Interp, st: &mut St, tag_name: &Value) -> ER<()> {
    let ob = st.outfile.to_bytes();
    if st.append.truthy() {
        if !lh(it, "created").get(&ob).truthy() {
            lh(it, "appended").set(&ob, n(1));
        }
    } else {
        lh(it, "created").set(&ob, n(1));
    }
    let fp = st.fp.clone();
    close(it, &fp)?;
    set_sv(it, "tmpText", Value::Undef);
    if sv(it, "verbose").truthy() {
        let vout = sv(it, "vout");
        print1(it, &vout, cat(&[b"Wrote ", &tag_name.to_bytes(), b" to ", &st.outfile.to_bytes(), b"\n"]))?;
    }
    st.outfile = Value::Undef;
    st.fp = Value::Undef;
    Ok(())
}

/// One tag of the normal outputs (the body of the `TAG:` loop).
#[allow(clippy::too_many_arguments)]
fn output_tag(it: &mut Interp, st: &mut St, o: &mut Out, tag: &Value, i: i64, no_dups: bool, print_conv: &Value, last_group: &mut Value) -> ER<()> {
    let et = st.et.clone();
    let mut tag_name = call(it, "GetTagName", vec![tag.clone()])?;
    let mut group = Value::Undef;
    let mut val_list = Value::Undef;
    // get the value for this tag
    let Some(mut val) = tag_value(it, st, tag, &mut val_list)? else { return Ok(()) };
    let html = sv(it, "html").truthy();
    let xml = sv(it, "xml").truthy();
    let json = sv(it, "json").truthy();
    let csv = sv(it, "csv").truthy();
    let out_format = sv(it, "outFormat");
    let show_tag_id = sv(it, "showTagID");
    let show_group = sv(it, "showGroup");
    let all_group = sv(it, "allGroup").truthy();
    if !show_group.is_undef() {
        group = pl::method(it, &et, "GetGroup", vec![tag.clone(), show_group.clone()])?;
        // look ahead to see if this tag may suppress a priority tag in
        // the same group, and if so suppress this tag instead
        // (note that the tag key may look like "TAG #(1)" when the "#" feature is used)
        if no_dups {
            if let Some(c) = re(r"^(.*?) ?\(", "").m(tag) {
                let k = cap(&c, 1);
                if !st.info.get(k.to_bytes()).is_undef() && eq(&group, &pl::method(it, &et, "GetGroup", vec![k, show_group.clone()])?) {
                    return Ok(());
                }
            }
        }
        if !group.truthy() && (xml || json || csv) {
            if !re(r"\b4\b", "").is_match(&show_group) {
                group = s("Unknown");
            } else if json && !all_group {
                group = s("Copy0");
            }
        }
        if st.fp.truthy() && !(all_group || csv) {
            let fp = st.fp.clone();
            if !eq(last_group, &group) {
                if html {
                    let mut cols = 1;
                    if pl::num_eq(&out_format, &n(0)) || pl::num_eq(&out_format, &n(1)) {
                        cols += 1;
                    }
                    if show_tag_id.truthy() {
                        cols += 1;
                    }
                    print1(it, &fp, cat(&[b"<tr><td colspan=", cols.to_string().as_bytes(), b" bgcolor='#dddddd'>", &group.to_bytes(), b"</td></tr>\n"]))?;
                } else if json {
                    if last_group.truthy() {
                        print1(it, &fp, cat(&[b"\n  ", &o.ket.to_bytes()]))?;
                    }
                    if last_group.truthy() || sv(it, "comma").truthy() {
                        print1(it, &fp, s(","))?;
                    }
                    print1(it, &fp, cat(&[b"\n  \"", &group.to_bytes(), b"\"", &o.sep.to_bytes(), b" ", &o.bra.to_bytes()]))?;
                    set_sv(it, "comma", Value::Undef);
                    clear_hash(&o.no_dups); // allow duplicate names in different groups
                } else {
                    print1(it, &fp, cat(&[b"---- ", &group.to_bytes(), b" ----\n"]))?;
                }
                *last_group = group.clone();
            }
            group = Value::Undef; // undefine so we don't print it below
        }
    } else if no_dups {
        // don't allow duplicates, but avoid suppressing the priority tag
        if let Some(c) = re(r"^(.*?) ?\(", "").m(tag) {
            if !st.info.get(cap(&c, 1).to_bytes()).is_undef() {
                return Ok(());
            }
        }
    }

    st.line_count += 1; // we are printing something meaningful

    // loop through list values when -b -W used
    let tag_out = sv(it, "tagOut").truthy();
    loop {
        if tag_out {
            // determine suggested extension for output file
            let vref = pl::sref(val.clone());
            let ext = call(it, "SuggestedExtension", vec![et.clone(), vref.clone(), tag_name.clone()])?;
            val = vref.deref();
            let wext = lh(it, "wext");
            if !wext.is_empty() {
                let mut w = wext.get(ext.to_bytes());
                if !w.truthy() {
                    w = wext.get("*");
                }
                if !w.truthy() {
                    w = n(-1);
                }
                if pl::lt(&w, &n(0)) {
                    let verbose = sv(it, "verbose");
                    if verbose.truthy() && pl::gt(&verbose, &n(1)) {
                        let vout = sv(it, "vout");
                        print1(it, &vout, cat(&[b"Not writing ", &ext.to_bytes(), b" output file for ", &tag_name.to_bytes(), b"\n"]))?;
                    }
                    return Ok(());
                }
            }
            let groups = pl::method_list(it, &et, "GetGroup", vec![tag.clone()])?;
            if !st.outfile.is_undef() {
                let fp = st.fp.clone();
                close(it, &fp)?;
                set_sv(it, "tmpText", Value::Undef); // (shouldn't happen)
            }
            let mut org = pl::method(it, &et, "GetValue", vec![s("OriginalRawFileName")])?;
            if !org.truthy() {
                org = pl::method(it, &et, "GetValue", vec![s("OriginalFileName")])?;
            }
            let orig = st.orig.clone();
            open_output_file(it, st, vec![orig, tag_name.clone(), Value::new_array(groups), ext, org])?;
            if !st.fp.truthy() {
                inc_sv(it, "countBad");
                return Ok(());
            }
            if !st.append.truthy() {
                set_sv(it, "tmpText", st.outfile.clone());
            }
        }
        // write binary output
        if sv(it, "binaryOutput").truthy() {
            let fp = st.fp.clone();
            print1(it, &fp, val.clone())?;
            let bin_term = sv(it, "binTerm");
            if !bin_term.is_undef() {
                print1(it, &fp, bin_term)?;
            }
            if tag_out {
                close_tag_out(it, st, &tag_name)?;
                if !(val_list.truthy() && !val_list.is_empty()) {
                    return Ok(());
                }
                val = shift(&val_list);
                continue; // loop over values of List tag
            }
            return Ok(());
        }
        break;
    }
    // save information for CSV output
    if csv {
        let mut tn = tag_name.clone();
        if tag.to_bytes().contains(&b'#') {
            tn = cat(&[&tn.to_bytes(), b"#"]); // add ValueConv "#" suffix if used
        }
        let gt = if group.truthy() { cat(&[&group.to_bytes(), b":", &tn.to_bytes()]) } else { tn.clone() };
        // (tag-name case may be different if some tags don't exist
        // in a file, so all logic must use lower-case tag names)
        let lc_tag = pl::lc(&gt);
        // override existing entry only if top priority
        if !o.csv_info.get(lc_tag.to_bytes()).is_undef() && tag.to_bytes().contains(&b'(') {
            return Ok(());
        }
        o.csv_info.set(lc_tag.to_bytes(), val.clone());
        let csv_tags = lh(it, "csvTags");
        if !csv_tags.get(lc_tag.to_bytes()).is_undef() {
            // overwrite with actual extracted tag name
            // (note: can't check "if defined $val" here because -f may be used)
            if !st.info.get(tag.to_bytes()).is_undef() {
                csv_tags.set(lc_tag.to_bytes(), gt);
            }
            return Ok(());
        }
        // must check for "Unknown" group (for tags that don't exist)
        let csv_tags_a = la(it, "csvTags");
        let cti = csv_tags_a.at(i);
        if group.truthy() && !cti.is_undef() {
            let pat = [b"^(.*):", &tn.to_bytes()[..], b"$"].concat();
            if let Some(c) = pl::re_dyn(&pat, "i")?.m(&cti) {
                if pl::str_eq(&group, "Unknown") {
                    return Ok(()); // nothing more to do if we don't know tag group
                }
                if pl::str_eq(&cap(&c, 1), "unknown") {
                    // replace unknown entry in CSV tag lookup and list
                    csv_tags.delete(cti.to_bytes());
                    csv_tags.set(lc_tag.to_bytes(), if !val.is_undef() { gt } else { s("") });
                    csv_tags_a.set_at(i as usize, lc_tag);
                    return Ok(());
                }
            }
        }
        // (don't save unextracted tag name unless -f was used)
        csv_tags.set(lc_tag.to_bytes(), if !val.is_undef() { gt } else { s("") });
        if la(it, "csvFiles").len() == 1 {
            csv_tags_a.push(lc_tag); // save order of tags for first file
        } else if !csv_tags_a.is_empty() {
            assign(&csv_tags_a, Vec::new());
        }
        return Ok(());
    }

    // get description if we need it (use tag name if $outFormat > 0)
    let mut desc = if pl::gt(&out_format, &n(0)) { tag_name.clone() } else { pl::method(it, &et, "GetDescription", vec![tag.clone()])? };
    let fp = st.fp.clone();
    let tab_format = sv(it, "tabFormat").truthy();
    let is_crlf = sv(it, "isCRLF").truthy();

    if xml {
        // RDF/XML output format
        let tok = cat(&[&group.to_bytes(), b":", &tag_name.to_bytes()]);
        if pl::gt(&out_format, &n(0)) {
            if sv(it, "structOpt").truthy() && val.is_ref() {
                val = pl::call(it, "Image::ExifTool::XMP::SerializeStruct", vec![et.clone(), val])?;
            }
            if sv(it, "escapeHTML").truthy() {
                pl::tr(&mut val, r"\0-\x08\x0b\x0c\x0e-\x1f", ".", "");
                let alt_enc = sv(it, "altEnc");
                if !alt_enc.truthy() {
                    call_ref(it, "Image::ExifTool::XMP::FixUTF8", &mut val)?;
                }
                val = pl::call(it, "Image::ExifTool::HTML::EscapeHTML", vec![val, alt_enc])?;
            } else {
                call_ref(it, "main::CleanXML", &mut val)?;
            }
            if !o.no_dups.get(tok.to_bytes()).truthy() {
                // manually un-do CR/LF conversion in Windows because output
                // is in text mode, which will re-convert newlines to CR/LF
                if is_crlf {
                    re(r"\x0d\x0a", "").subst(&mut val, true, |_| b"\x0a".to_vec());
                }
                print1(it, &fp, cat(&[b"\n ", &tok.to_bytes(), b"='", &val.to_bytes(), b"'"]))?;
                // XML does not allow duplicate attributes
                o.no_dups.set(tok.to_bytes(), n(1));
            }
            return Ok(());
        }
        let mut xtra;
        let mut desc_close = Value::Undef;
        if show_tag_id.truthy() {
            let r = pl::method_list(it, &et, "GetTagID", vec![tag.clone()])?;
            let (mut id, lang) = (arg(&r, 0), arg(&r, 1));
            if re(r"^\d+$", "").is_match(&id) {
                if pl::str_eq(&show_tag_id, "H") {
                    id = pl::sprintf("0x%.4x", &[id]);
                }
            } else {
                id = pl::call(it, "Image::ExifTool::XMP::FullEscapeXML", vec![id])?;
            }
            xtra = cat(&[b" et:id='", &id.to_bytes(), b"'"]);
            if lang.truthy() {
                xtra = cat(&[&xtra.to_bytes(), b" xml:lang='", &lang.to_bytes(), b"'"]);
            }
        } else {
            xtra = s("");
        }
        if tab_format {
            let table = pl::method(it, &et, "GetTableName", vec![tag.clone()])?;
            let index = pl::method(it, &et, "GetTagIndex", vec![tag.clone()])?;
            xtra = cat(&[&xtra.to_bytes(), b" et:table='", &table.to_bytes(), b"'"]);
            if !index.is_undef() {
                xtra = cat(&[&xtra.to_bytes(), b" et:index='", &index.to_bytes(), b"'"]);
            }
        }
        // Note: New $xtra attributes must be added to %ignoreEtProp in XMP.pm!
        let last_val = val.clone();
        let mut val_num = 0;
        while val_num < 2 {
            val = call(it, "FormatXML", vec![val, st.ind.clone(), group.clone()])?;
            // manually un-do CR/LF conversion in Windows because output
            // is in text mode, which will re-convert newlines to CR/LF
            if is_crlf {
                re(r"\x0d\x0a", "").subst(&mut val, true, |_| b"\x0a".to_vec());
            }
            if pl::ge(&out_format, &n(0)) {
                // normal output format (note: this will give
                // non-standard RDF/XML if there are any attributes)
                print1(it, &fp, cat(&[b"\n <", &tok.to_bytes(), &xtra.to_bytes(), &val.to_bytes(), b"</", &tok.to_bytes(), b">"]))?;
                break;
            } else if val_num == 0 {
                call_ref(it, "main::CleanXML", &mut desc)?;
                if xtra.truthy() {
                    print1(it, &fp, cat(&[b"\n <", &tok.to_bytes(), b">"]))?;
                    print1(it, &fp, cat(&[b"\n  <rdf:Description", &xtra.to_bytes(), b">"]))?;
                    desc_close = s("\n  </rdf:Description>");
                } else {
                    print1(it, &fp, cat(&[b"\n <", &tok.to_bytes(), b" rdf:parseType='Resource'>"]))?;
                    desc_close = s("");
                }
                // print tag Description
                print1(it, &fp, cat(&[b"\n   <et:desc>", &desc.to_bytes(), b"</et:desc>"]))?;
                if print_conv.truthy() {
                    // print PrintConv value
                    print1(it, &fp, cat(&[b"\n   <et:prt", &val.to_bytes(), b"</et:prt>"]))?;
                    val = pl::method(it, &et, "GetValue", vec![tag.clone(), s("ValueConv")])?;
                    if val.is_undef() {
                        val = s("");
                    }
                    // go back to print ValueConv value only if different
                    if !call(it, "IsEqual", vec![val.clone(), last_val.clone(), n(1)])?.truthy() {
                        val_num += 1;
                        continue;
                    }
                    print1(it, &fp, cat(&[&desc_close.to_bytes(), b"\n </", &tok.to_bytes(), b">"]))?;
                    break;
                }
            }
            // print ValueConv value
            print1(it, &fp, cat(&[b"\n   <et:val", &val.to_bytes(), b"</et:val>"]))?;
            print1(it, &fp, cat(&[&desc_close.to_bytes(), b"\n </", &tok.to_bytes(), b">"]))?;
            break;
        }
        return Ok(());
    } else if json {
        // JSON or PHP output format
        let tok = if all_group { cat(&[&group.to_bytes(), b":", &tag_name.to_bytes()]) } else { tag_name.clone() };
        // (removed due to backward incompatibility)
        // $tok .= '#' if $tag =~ /#/; # add back '#' suffix if used
        if o.no_dups.get(tok.to_bytes()).truthy() {
            return Ok(());
        }
        o.no_dups.set(tok.to_bytes(), n(1));
        if sv(it, "comma").truthy() {
            print1(it, &fp, s(","))?;
        }
        print1(it, &fp, cat(&[b"\n", &st.ind.to_bytes(), b"\"", &tok.to_bytes(), b"\"", &o.sep.to_bytes(), b" "]))?;
        if show_tag_id.truthy() || pl::lt(&out_format, &n(0)) {
            let h = Value::new_hash();
            h.set("val", val.clone());
            val = h;
            if show_tag_id.truthy() {
                let r = pl::method_list(it, &et, "GetTagID", vec![tag.clone()])?;
                let (mut id, lang) = (arg(&r, 0), arg(&r, 1));
                if pl::str_eq(&show_tag_id, "H") && re(r"^\d+$", "").is_match(&id) {
                    id = pl::sprintf("0x%.4x", &[id]);
                }
                if lang.truthy() {
                    val.set("lang", lang);
                }
                val.set("id", id);
            }
            if tab_format {
                let table = pl::method(it, &et, "GetTableName", vec![tag.clone()])?;
                val.set("table", table);
                let index = pl::method(it, &et, "GetTagIndex", vec![tag.clone()])?;
                if !index.is_undef() {
                    val.set("index", index);
                }
            }
            if pl::lt(&out_format, &n(0)) {
                val.set("desc", desc.clone());
                if print_conv.truthy() {
                    let num = pl::method(it, &et, "GetValue", vec![tag.clone(), s("ValueConv")])?;
                    if !num.is_undef() && !call(it, "IsEqual", vec![num.clone(), val.get("val"), n(1)])?.truthy() {
                        val.set("num", num);
                    }
                }
                let ex = et.get_or_new_hash("TAG_EXTRA").get(tag.to_bytes());
                if !ex.get("G6").is_undef() {
                    val.set("fmt", ex.get("G6"));
                }
                let bin_val = ex.get("BinVal");
                if !bin_val.is_undef() {
                    let max = pl::div(&pl::sub(&et.path(&["OPTIONS", "LimitLongValues"]), &n(5)), &n(3))?;
                    if pl::ge(&max, &n(0)) && pl::gt(&pl::length(&bin_val), &pl::int(&max)) {
                        let max = pl::int(&max);
                        let mut h = pl::unpack(&format!("(H2){}", max.to_text()), &bin_val)?;
                        h.push(s("[...]"));
                        val.set("hex", pl::join(" ", &h));
                    } else {
                        val.set("hex", pl::join(" ", &pl::unpack("(H2)*", &bin_val)?));
                    }
                }
                if !ex.get("Rational").is_undef() && et.path(&["OPTIONS", "SaveBin"]).truthy() {
                    val.set("rat", ex.get("Rational"));
                }
            }
        }
        call(it, "FormatJSON", vec![fp.clone(), val, st.ind.clone(), o.quote.clone()])?;
        set_sv(it, "comma", n(1));
        return Ok(());
    }
    let mut id = Value::Undef;
    if show_tag_id.truthy() {
        id = pl::method(it, &et, "GetTagID", vec![tag.clone()])?;
        if let Some(c) = re(r"^(\d+)(\.\d+)?$", "").m(&id) {
            // only print numeric ID's
            if pl::str_eq(&show_tag_id, "H") {
                id = pl::sprintf("0x%.4x", &[cap(&c, 1)]);
            }
        } else {
            id = s("-");
        }
    }

    if sv(it, "escapeC").truthy() {
        let esc_c = lh(it, "escC");
        re(r"([\0-\x1f\\\x7f])", "").subst(&mut val, true, |c| {
            let ch = cap(c, 1);
            let e = esc_c.get(ch.to_bytes());
            if e.truthy() {
                e.to_bytes().to_vec()
            } else {
                format!("\\x{:02x}", ch.to_bytes()[0]).into_bytes()
            }
        });
    } else {
        // translate unprintable chars in value and remove trailing spaces
        pl::tr(&mut val, r"\x01-\x1f\x7f", ".", "");
        re(r"\x00", "").subst(&mut val, true, |_| Vec::new());
        re(r"\s+$", "").subst(&mut val, false, |_| Vec::new());
    }

    if html {
        print1(it, &fp, s("<tr>"))?;
        if !group.is_undef() {
            print1(it, &fp, cat(&[b"<td>", &group.to_bytes(), b"</td>"]))?;
        }
        if show_tag_id.truthy() {
            print1(it, &fp, cat(&[b"<td>", &id.to_bytes(), b"</td>"]))?;
        }
        if pl::le(&out_format, &n(1)) {
            print1(it, &fp, cat(&[b"<td>", &desc.to_bytes(), b"</td>"]))?;
        }
        print1(it, &fp, cat(&[b"<td>", &val.to_bytes(), b"</td></tr>\n"]))?;
    } else {
        let mut buff: Vec<u8> = Vec::new();
        let (gb, ib, db, vb) = (group.to_bytes(), id.to_bytes(), desc.to_bytes(), val.to_bytes());
        if tab_format {
            if !group.is_undef() {
                buff = [&gb[..], b"\t"].concat();
            }
            if show_tag_id.truthy() {
                buff.extend_from_slice(&[&ib[..], b"\t"].concat());
            }
            if pl::le(&out_format, &n(1)) {
                buff.extend_from_slice(&[&db[..], b"\t", &vb[..], b"\n"].concat());
            } else if !o.line.is_undef() {
                o.line = cat(&[&o.line.to_bytes(), b"\t", &vb]);
            } else {
                o.line = val.clone();
            }
        } else if pl::lt(&out_format, &n(0)) {
            // long format
            if !group.is_undef() {
                buff = [b"[", &gb[..], b"] "].concat();
            }
            if show_tag_id.truthy() {
                buff.extend_from_slice(&[&ib[..], b" "].concat());
            }
            buff.extend_from_slice(&[&db[..], b"\n      ", &vb[..], b"\n"].concat());
        } else if pl::num_eq(&out_format, &n(0)) || pl::num_eq(&out_format, &n(1)) {
            let mut len: i64 = 0;
            if !group.is_undef() {
                buff = pl::sprintf("%-15s ", &[cat(&[b"[", &gb, b"]"])]).to_bytes().to_vec();
                len = 16;
            }
            if show_tag_id.truthy() {
                let mut wid: i64 = if pl::str_eq(&show_tag_id, "D") { 5 } else { 6 };
                len += wid + 1;
                wid = len - buff.len() as i64 - 1;
                if wid < 1 {
                    wid = 1;
                }
                buff.extend_from_slice(&pl::sprintf(&format!("%{wid}s "), &[id.clone()]).to_bytes());
            }
            let wid = 32 - (buff.len() as i64 - len);
            // pad description to a constant length
            // (get actual character length when using alternate languages
            // because these descriptions may contain UTF8-encoded characters)
            let mut pad_len = pl::sub(&n(wid), &call(it, "LengthUTF8", vec![desc.clone()])?);
            if pl::lt(&pad_len, &n(0)) {
                pad_len = n(0);
            }
            buff.extend_from_slice(&db);
            buff.extend(rep(b" ", &pad_len));
            buff.extend_from_slice(&[b": ", &vb[..], b"\n"].concat());
        } else if pl::num_eq(&out_format, &n(2)) {
            if !group.is_undef() {
                buff = [b"[", &gb[..], b"] "].concat();
            }
            if show_tag_id.truthy() {
                buff.extend_from_slice(&[&ib[..], b" "].concat());
            }
            buff.extend_from_slice(&[&tag_name.to_bytes()[..], b": ", &vb[..], b"\n"].concat());
        } else if sv(it, "argFormat").truthy() {
            buff = b"-".to_vec();
            if !group.is_undef() {
                buff.extend_from_slice(&[&gb[..], b":"].concat());
            }
            if tag.to_bytes().contains(&b'#') {
                tag_name = cat(&[&tag_name.to_bytes(), b"#"]); // add '#' suffix if used
            }
            buff.extend_from_slice(&[&tag_name.to_bytes()[..], b"=", &vb[..], b"\n"].concat());
        } else {
            if !group.is_undef() {
                buff = [&gb[..], b" "].concat();
            }
            if show_tag_id.truthy() {
                buff.extend_from_slice(&[&ib[..], b" "].concat());
            }
            buff.extend_from_slice(&[&vb[..], b"\n"].concat());
        }
        print1(it, &fp, Value::bytes(buff))?;
    }
    if tag_out {
        close_tag_out(it, st, &tag_name)?;
    }
    Ok(())
}

/// The end of the sub: close the output file, `++$count`.
fn finish(it: &mut Interp, st: &mut St) -> ER<()> {
    if !st.outfile.is_undef() {
        let fp = st.fp.clone();
        let ob = st.outfile.to_bytes();
        if (sv(it, "textOverwrite").to_i64() & 0x02) != 0 {
            // save state of this file if we may be appending
            let comma = sv(it, "comma");
            lh(it, "outComma").set(&ob, comma);
            let out_trailer = lh(it, "outTrailer");
            out_trailer.set(&ob, s(""));
            let sect_trailer = sv(it, "sectTrailer");
            if sect_trailer.truthy() {
                let v = pl::concat(&out_trailer.get(&ob), &sect_trailer);
                out_trailer.set(&ob, v.clone());
                if v.truthy() {
                    set_sv(it, "sectTrailer", s(""));
                }
            }
            let file_trailer = sv(it, "fileTrailer");
            if file_trailer.truthy() {
                let v = pl::concat(&out_trailer.get(&ob), &file_trailer);
                out_trailer.set(&ob, v);
            }
        } else {
            // write section and file trailers before closing the file
            let sect_trailer = sv(it, "sectTrailer");
            if sect_trailer.truthy() && print1(it, &fp, sect_trailer)? {
                set_sv(it, "sectTrailer", s(""));
            }
            let file_trailer = sv(it, "fileTrailer");
            if file_trailer.truthy() {
                print1(it, &fp, file_trailer)?;
            }
        }
        close(it, &fp)?;
        set_sv(it, "tmpText", Value::Undef);
        if st.line_count != 0 {
            if st.append.truthy() {
                if !lh(it, "created").get(&ob).truthy() {
                    lh(it, "appended").set(&ob, n(1));
                }
            } else {
                lh(it, "created").set(&ob, n(1));
            }
        } else if !st.append.truthy() {
            pl::method(it, &st.et.clone(), "Unlink", vec![st.outfile.clone()])?; // don't keep empty output files
        }
        set_sv(it, "comma", Value::Undef);
    }
    inc_sv(it, "count");
    Ok(())
}
