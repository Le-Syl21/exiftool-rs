//! The option chain of the exiftool script's argument loop (script lines
//! 690-1449): one argument that starts with "-", as `parse_option` (see
//! docs/port/CLI.md for its contract with program.rs).
//!
//! `$_`, `@ARGV` and `$1` are what the loop has when it calls this; the
//! script's lexicals (`$pass`, `@nextPass`, `$mt`...) are read and written
//! through `lex_scalar` / `lex_array` / `lex_hash`. The filehandle builtins
//! the chain uses (`\*ARGFILE`, `<ARGFILE>`, `close`, `binmode`, `unlink`,
//! `$^O`...) run as perl's, in package main, through `perl_code`.

use std::rc::Rc;

use super::{lex_array, lex_hash, lex_scalar};
use crate::interp::eval::{Code, Flow, ER};
use crate::interp::{Interp, Scope, Want};
use crate::perl::{ops, Value};
use crate::pl::{self, cap, re, s};

pub fn register(it: &mut Interp) {
    // (nothing calls parse_option until program.rs does: this native lets
    // the script, or a test of it, call the chain as a sub)
    super::define(it, "main::<parse_option>", parse_option_native);
}

fn parse_option_native(it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let r = parse_option(it)?;
    Ok(vec![pbool(r)])
}

// ---- helpers ----

fn n(i: i64) -> Value {
    Value::Int(i)
}

/// Perl's boolean results: 1 or "".
fn pbool(b: bool) -> Value {
    if b {
        n(1)
    } else {
        s("")
    }
}

/// The script lexical `$name`.
fn get(it: &mut Interp, name: &str) -> Value {
    lex_scalar(it, name).borrow().clone()
}

/// `$name = v` for the script lexical `$name`.
fn set(it: &mut Interp, name: &str, v: Value) {
    *lex_scalar(it, name).borrow_mut() = v;
}

/// `push @name, v` for the script lexical `@name`.
fn push(it: &mut Interp, name: &str, v: Value) {
    lex_array(it, name).push(v);
}

/// `undef @name` for the script lexical `@name`.
fn clear(it: &mut Interp, name: &str) {
    if let Value::ArrayRef(a) = lex_array(it, name) {
        a.borrow_mut().clear();
    }
}

fn topic(it: &mut Interp) -> Value {
    it.get_scalar("main::_")
}

fn set_topic(it: &mut Interp, v: Value) {
    it.set_scalar("main::_", v);
}

fn argv(it: &mut Interp) -> Value {
    pl::pkg_array(it, "main::ARGV")
}

/// `shift` (of @ARGV, at the program's level).
fn shift(it: &mut Interp) -> Value {
    match argv(it) {
        Value::ArrayRef(a) => {
            let mut a = a.borrow_mut();
            if a.is_empty() {
                Value::Undef
            } else {
                a.remove(0)
            }
        }
        _ => Value::Undef,
    }
}

/// The string concatenation of the parts.
fn cat(parts: &[Value]) -> Value {
    let mut out = Vec::new();
    for p in parts {
        out.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(out)
}

fn truthy(it: &mut Interp, name: &str) -> bool {
    get(it, name).truthy()
}

/// `$mt->Options(ARGS)`
fn options(it: &mut Interp, args: Vec<Value>) -> ER<Value> {
    let mt = get(it, "mt");
    pl::method(it, &mt, "Options", args)
}

/// `Warn(MSG)` (the script's sub)
fn warn_(it: &mut Interp, msg: Value) -> ER<Value> {
    pl::call(it, "main::Warn", vec![msg])
}

/// `Error(MSG)` (the script's sub)
fn error(it: &mut Interp, msg: Value) -> ER<()> {
    pl::call(it, "main::Error", vec![msg])?;
    Ok(())
}

/// A call in void context.
fn vcall(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<()> {
    it.call_named(name, args, Want::Void)?;
    Ok(())
}

/// Perl code run in package main with `vars` as its lexicals (the
/// filehandle builtins); its die is the native's.
fn perl_code(it: &mut Interp, code: &str, vars: &[(&str, Value)], want: Want) -> ER<Vec<Value>> {
    let scope = Scope::new(None);
    for (name, v) in vars {
        scope.scalar(name, v.clone());
    }
    let saved = it.get_scalar("main::@");
    let r = it.eval_string(code.as_bytes(), "main", &scope, want);
    it.set_scalar("main::@", saved);
    r.map_err(Flow::Die)
}

/// `perl_code` in scalar context.
fn perl1(it: &mut Interp, code: &str, vars: &[(&str, Value)]) -> ER<Value> {
    Ok(perl_code(it, code, vars, Want::Scalar)?.pop().unwrap_or_default())
}

/// `\*NAME`
fn glob(it: &mut Interp, name: &str) -> ER<Value> {
    perl1(it, &format!("\\*{name}"), &[])
}

/// `eval STRING` (in package main): the result, or undef with `$@` set.
fn eval_string(it: &mut Interp, code: &Value) -> Value {
    let scope = Scope::new(None);
    match it.eval_string(&code.to_bytes(), "main", &scope, Want::Scalar) {
        Ok(v) => v.last().cloned().unwrap_or_default(),
        Err(_) => Value::Undef,
    }
}

/// `eval { require MODULE }` is true
fn eval_require(it: &mut Interp, module: &str) -> ER<bool> {
    Ok(pl::eval_block(it, |it| crate::runtime::require(it, module))?.is_some())
}

/// Capture `i` of the last successful match of the caller (perl's `$1`
/// is still the loop body's when the chain reads it without a match of
/// its own).
fn caller_cap(it: &Interp, i: usize) -> Value {
    match &it.last_match {
        Some(m) => m.group(i),
        None => Value::Undef,
    }
}

/// `quotemeta`
fn quotemeta(v: &Value) -> Value {
    let mut out = Vec::new();
    for &b in v.to_bytes().iter() {
        if !(b.is_ascii_alphanumeric() || b == b'_') {
            out.push(b'\\');
        }
        out.push(b);
    }
    Value::bytes(out)
}

/// `sub { $evalWarning = $_[0] }`
fn eval_warning_handler() -> Value {
    Value::Code(Rc::new(Code::Native(
        Rc::from("main::__ANON__"),
        Rc::new(|it: &mut Interp, a: Vec<Value>, _w: Want| {
            let v = pl::arg(&a, 0);
            set(it, "evalWarning", v.clone());
            Ok(vec![v])
        }),
    )))
}

fn num_is(v: &Value, i: i64) -> bool {
    pl::num_eq(v, &n(i))
}

// ---- the option chain ----

/// One argument that starts with "-" (already in `$_`, the dash removed by
/// the caller as the script does): returns true for the script's `next`
/// (go to the next argument), false when it falls through to the end of
/// the loop's body.
pub fn parse_option(it: &mut Interp) -> ER<bool> {
    // (the loop body's $1: the caller's s/^(-|\xe2\x88\x92)//, then this
    // chain's own successful matches that the script reads it from)
    let mut dollar1 = caller_cap(it, 1);
    let mut t = topic(it);
    if re(r"^\xe2\x88\x92", "").subst(&mut t, false, |_| b"-".to_vec()) > 0 {
        // translate double-dash too
        set_topic(it, t);
        dollar1 = Value::Undef;
    }
    if pl::str_eq(&topic(it), "-") {
        if !truthy(it, "pass") {
            push(it, "nextPass", s("--"));
        }
        set(it, "endOfOpts", n(1));
        return Ok(true);
    }
    let a = pl::lc(&topic(it));
    let a_is = |x: &str| pl::str_eq(&a, x);
    if let Some(c) = re(r"^list([wfrdx]|wf|g(\d*)|geo)?$", "i").m(&topic(it)) {
        if !truthy(it, "pass") {
            let v = cat(&[s("-"), topic(it)]);
            push(it, "nextPass", v);
        }
        let c1 = cap(&c, 1);
        let typ = pl::lc(&if c1.truthy() { c1 } else { s("") });
        if !typ.truthy() || pl::str_eq(&typ, "w") || pl::str_eq(&typ, "x") {
            let mut group = Value::Undef;
            let a0 = argv(it).at(0);
            let m = if a0.truthy() { re(r"^(-|\xe2\x88\x92)(.+):(all|\*)$", "i").m(&a0) } else { None };
            if let Some(g) = m {
                if num_is(&get(it, "pass"), 0) {
                    if pl::str_eq(&pl::lc(&cap(&g, 2)), "mwg") {
                        set(it, "useMWG", n(1));
                    }
                    let v = shift(it);
                    push(it, "nextPass", v);
                    return Ok(true);
                }
                group = cap(&g, 2);
                shift(it);
                if re("IFD", "i").is_match(&group) {
                    warn_(it, s("Can't list tags for specific IFD\n"))?;
                    set(it, "helped", n(1));
                    return Ok(true);
                }
                if re(r"^(all|\*)$", "").is_match(&group) {
                    group = Value::Undef;
                }
            } else if !truthy(it, "pass") {
                return Ok(true);
            }
            set(it, "helped", n(1));
            if pl::str_eq(&typ, "x") {
                crate::runtime::require(it, "Image::ExifTool::TagInfoXML")?;
                let mut opts = Vec::new();
                if !get(it, "forcePrint").is_undef() {
                    opts.extend([s("Flags"), n(1)]);
                }
                // (perl warns here if $outFormat is undef)
                if pl::gt(&get(it, "outFormat"), &n(0)) {
                    opts.extend([s("NoDesc"), n(1)]);
                }
                opts.extend([s("Lang"), get(it, "langOpt")]);
                let mut args = vec![Value::Undef, group];
                args.extend(opts);
                vcall(it, "Image::ExifTool::TagInfoXML::Write", args)?;
                return Ok(true);
            }
            let wr = pl::str_eq(&typ, "w");
            let msg = cat(&[
                s(if wr { "Writable" } else { "Available" }),
                if group.truthy() { cat(&[s(" "), group.clone()]) } else { s("") },
                s(" tags"),
            ]);
            let list = if wr {
                pl::call_list(it, "main::GetWritableTags", vec![group.clone()])?
            } else {
                pl::call_list(it, "main::GetAllTags", vec![group.clone()])?
            };
            let mut args = vec![msg];
            args.extend(list);
            vcall(it, "main::PrintTagList", args)?;
            // also print shortcuts if listing all tags
            if group.truthy() || wr {
                return Ok(true);
            }
            let tag_list = pl::call_list(it, "main::GetShortcuts", vec![])?;
            if !tag_list.is_empty() {
                let mut args = vec![s("Command-line shortcuts")];
                args.extend(tag_list);
                vcall(it, "main::PrintTagList", args)?;
            }
            return Ok(true);
        }
        if !truthy(it, "pass") {
            return Ok(true);
        }
        set(it, "helped", n(1));
        if pl::str_eq(&typ, "wf") {
            let mut wf = vec![s("Writable file extensions")];
            for ft in pl::call_list(it, "main::GetFileType", vec![])? {
                if pl::call(it, "main::CanWrite", vec![ft.clone()])?.truthy() {
                    wf.push(ft);
                }
            }
            vcall(it, "main::PrintTagList", wf)?;
        } else if pl::str_eq(&typ, "f") {
            let mut args = vec![s("Supported file extensions")];
            args.extend(pl::call_list(it, "main::GetFileType", vec![])?);
            vcall(it, "main::PrintTagList", args)?;
        } else if pl::str_eq(&typ, "r") {
            let mut args = vec![s("Recognized file extensions")];
            args.extend(pl::call_list(it, "main::GetFileType", vec![Value::Undef, n(0)])?);
            vcall(it, "main::PrintTagList", args)?;
        } else if pl::str_eq(&typ, "d") {
            let mut args = vec![s("Deletable groups")];
            args.extend(pl::call_list(it, "main::GetDeleteGroups", vec![])?);
            vcall(it, "main::PrintTagList", args)?;
        } else if pl::str_eq(&typ, "geo") {
            list_geo(it)?;
        } else {
            // 'g(\d*)'
            // list all groups in specified family
            let c2 = cap(&c, 2);
            let family = if c2.truthy() { c2 } else { n(0) };
            let mt = get(it, "mt");
            let mut args = vec![cat(&[s("Groups in family "), family.clone()])];
            args.extend(pl::method_list(it, &mt, "GetAllGroups", vec![family])?);
            vcall(it, "main::PrintTagList", args)?;
        }
        return Ok(true);
    }
    if a_is("ver") {
        if !truthy(it, "pass") {
            push(it, "nextPass", s("-ver"));
            return Ok(true);
        }
        print_version(it)?;
        set(it, "helped", n(1));
        return Ok(true);
    }
    if let Some(c) = re(r"^(all|add)?tagsfromfile(=.*)?$", "i").m(&topic(it)) {
        let c2 = cap(&c, 2);
        let stf = if c2.truthy() {
            pl::substr(&c2, 1, None)
        } else if !argv(it).is_empty() {
            shift(it)
        } else {
            s("")
        };
        set(it, "setTagsFile", stf.clone());
        if pl::str_eq(&stf, "") {
            error(it, s("File must be specified for -tagsFromFile option\n"))?;
            set(it, "badCmd", n(1));
            return Ok(true);
        }
        // create necessary lists, etc for this new -tagsFromFile file
        let c1 = cap(&c, 1);
        let replace = if c1.truthy() && pl::str_eq(&pl::lc(&c1), "add") { 0 } else { 1 };
        let opts = Value::new_hash();
        opts.set("Replace", n(replace));
        vcall(it, "main::AddSetTagsFile", vec![stf, opts])?;
        set(it, "tagsFrom", s("File"));
        return Ok(true);
    }
    if a_is("@") {
        return arg_file(it);
    }
    if let Some(c) = re(r"^(-?)(a|duplicates)$", "i").m(&topic(it)) {
        let v = if cap(&c, 1).truthy() { 0 } else { 1 };
        options(it, vec![s("Duplicates"), n(v)])?;
        return Ok(true);
    }
    if a_is("api") {
        let mut opt = shift(it);
        if !opt.is_undef() && pl::length(&opt).truthy() {
            let mut eq_val = None;
            re("=(.*)", "s").subst(&mut opt, false, |c| {
                eq_val = Some(cap(c, 1));
                Vec::new()
            });
            let mut val = eq_val.unwrap_or_else(|| n(1));
            // empty string means an undefined value unless ^= is used
            if re(r"\^$", "").subst(&mut opt, false, |_| Vec::new()) == 0 && !pl::length(&val).truthy() {
                val = Value::Undef;
            }
            options(it, vec![opt, val])?;
        } else {
            if !truthy(it, "pass") {
                push(it, "nextPass", s("-api"));
                if !opt.is_undef() {
                    push(it, "nextPass", opt);
                }
                return Ok(true);
            }
            pl::print_to(it, None, &[s("Available API Options:\n")])?;
            let available_options = pl::call(it, "Image::ExifTool::AvailableOptions", vec![])?;
            for o in pl::aderef(&available_options)? {
                if !o.at(3).truthy() {
                    let line = pl::sprintf("  %-17s - %s\n", &[o.at(0), o.at(2)]);
                    pl::print_to(it, None, &[line])?;
                }
            }
            set(it, "helped", n(1));
        }
        return Ok(true);
    }
    if re(r"^arg(s|format)$", "i").is_match(&topic(it)) {
        set(it, "argFormat", n(1));
        return Ok(true);
    }
    if let Some(c) = re(r"^(-?)b(inary)?$", "i").m(&topic(it)) {
        let (bo, nb) = if cap(&c, 1).truthy() { (Value::Undef, n(1)) } else { (n(1), Value::Undef) };
        set(it, "binaryOutput", bo.clone());
        set(it, "noBinary", nb);
        options(it, vec![s("Binary"), bo.clone(), s("NoPDFList"), bo])?;
        return Ok(true);
    }
    if re(r"^c(oordFormat)?$", "i").is_match(&topic(it)) {
        let fmt = shift(it);
        if !fmt.truthy() {
            error(it, s("Expecting coordinate format for -c option\n"))?;
            set(it, "badCmd", n(1));
            return Ok(true);
        }
        options(it, vec![s("CoordFormat"), fmt])?;
        return Ok(true);
    }
    if a_is("charset") {
        return charset(it);
    }
    if re(r"^config$", "i").is_match(&topic(it)) {
        warn_(it, s("Ignored -config option (not first on command line)\n"))?;
        shift(it);
        return Ok(true);
    }
    if let Some(c) = re(r"^(csv|j(son)?)(\+?=.*)?$", "i").m(&topic(it)) {
        return csv_json(it, &c);
    }
    if re(r"^csvdelim$", "i").is_match(&topic(it)) {
        let mut csv_delim = shift(it);
        set(it, "csvDelim", csv_delim.clone());
        if csv_delim.is_undef() {
            error(it, s("Expecting argument for -csvDelim option\n"))?;
            set(it, "badCmd", n(1));
            return Ok(true);
        }
        if re("\"", "").is_match(&csv_delim) {
            error(it, s("CSV delimiter can not contain a double quote\n"))?;
            set(it, "badCmd", n(1));
            return Ok(true);
        }
        re(r"\\(.)", "s").subst(&mut csv_delim, true, |c| {
            let k = cap(c, 1).to_bytes();
            match &*k {
                b"t" => b"\t".to_vec(),
                b"n" => b"\n".to_vec(),
                b"r" => b"\r".to_vec(),
                b"\\" => b"\\".to_vec(),
                _ => [b"\\", &*k].concat(),
            }
        });
        set(it, "csvDelim", csv_delim.clone());
        options(it, vec![s("CSVDelim"), csv_delim])?;
        return Ok(true);
    }
    if re("^d$", "").is_match(&topic(it)) || a_is("dateformat") {
        let fmt = shift(it);
        if !fmt.truthy() {
            error(it, s("Expecting date format for -d option\n"))?;
            set(it, "badCmd", n(1));
            return Ok(true);
        }
        options(it, vec![s("DateFormat"), fmt])?;
        return Ok(true);
    }
    if re("^D$", "").is_match(&topic(it)) || a_is("decimal") {
        set(it, "showTagID", s("D"));
        return Ok(true);
    }
    if re("^diff$", "i").is_match(&topic(it)) {
        let mut diff = shift(it);
        set(it, "diff", diff.clone());
        if diff.is_undef() {
            let msg = cat(&[s("Expecting file name for -"), topic(it), s(" option\n")]);
            error(it, msg)?;
            set(it, "badCmd", n(1));
        }
        // change to forward slashes if necessary
        pl::call_mut(it, "main::CleanFilename", std::slice::from_mut(&mut diff), Want::Void)?;
        set(it, "diff", diff);
        return Ok(true);
    }
    if let Some(c) = re("^delete_original(!?)$", "i").m(&topic(it)) {
        set(it, "deleteOrig", n(if cap(&c, 1).truthy() { 2 } else { 1 }));
        return Ok(true);
    }
    if re("^list_dir$", "i").is_match(&topic(it)) {
        set(it, "listDir", n(1));
        return Ok(true);
    }
    if re("^e$", "").is_match(&topic(it)) || a_is("-composite") {
        options(it, vec![s("Composite"), n(0)])?;
        return Ok(true);
    }
    if re("^-e$", "").is_match(&topic(it)) || a_is("composite") {
        options(it, vec![s("Composite"), n(1)])?;
        return Ok(true);
    }
    // (-eh is undocumented)
    if re("^E$", "").is_match(&topic(it)) || a_is("escapehtml") || a_is("eh") {
        crate::runtime::require(it, "Image::ExifTool::HTML")?;
        set(it, "escapeHTML", n(1));
        return Ok(true);
    }
    if a_is("ec") || a_is("escapec") {
        set(it, "escapeC", n(1));
        return Ok(true);
    }
    if a_is("ex") || a_is("escapexml") {
        set(it, "escapeXML", n(1));
        return Ok(true);
    }
    if let Some(c) = re(r"^echo(\d)?$", "i").m(&topic(it)) {
        let c1 = cap(&c, 1);
        let num = if c1.truthy() { c1 } else { n(1) };
        let arg = shift(it);
        if arg.is_undef() {
            return Ok(true);
        }
        if pl::gt(&num, &n(4)) {
            warn_(it, s("Invalid -echo number\n"))?;
            return Ok(true);
        }
        if pl::gt(&num, &n(2)) {
            push(it, if num_is(&num, 3) { "echo3" } else { "echo4" }, arg);
        } else {
            let fh = glob(it, if num_is(&num, 2) { "STDERR" } else { "STDOUT" })?;
            pl::print_to(it, Some(&fh), &[arg, s("\n")])?;
        }
        set(it, "helped", n(1));
        return Ok(true);
    }
    if let Some(c) = re(r"^(ee|extractembedded)(\d*)$", "i").m(&topic(it)) {
        let c2 = cap(&c, 2);
        options(it, vec![s("ExtractEmbedded"), if c2.truthy() { c2 } else { n(1) }])?;
        options(it, vec![s("Duplicates"), n(1)])?;
        return Ok(true);
    }
    if let Some(c) = re(r"^efile(\d+)?(!)?$", "i").m(&topic(it)) {
        let arg = shift(it);
        if arg.is_undef() {
            let msg = cat(&[s("Expecting file name for -"), topic(it), s(" option\n")]);
            error(it, msg)?;
            set(it, "badCmd", n(1));
            return Ok(true);
        }
        let c1 = cap(&c, 1);
        let bit = |m: i64| c1.truthy() && ops::bit_and(&c1, &n(m)).truthy();
        let efile = lex_array(it, "efile");
        if !c1.truthy() || ops::bit_and(&c1, &n(0x01)).truthy() {
            efile.set_at(0, arg.clone()); // error
        }
        if bit(0x02) {
            efile.set_at(1, arg.clone()); // unchanged
        }
        if bit(0x04) {
            efile.set_at(2, arg.clone()); // failed -if condition
        }
        if bit(0x08) {
            efile.set_at(3, arg.clone()); // updated
        }
        if bit(0x016) {
            efile.set_at(4, arg.clone()); // created
        }
        if cap(&c, 2).truthy() {
            perl_code(it, "unlink $arg", &[("arg", arg)], Want::Void)?;
        }
        return Ok(true);
    }
    // (-execute handled at top of loop)
    if let Some(c) = re(r"^-?ext(ension)?(\+)?$", "i").m(&topic(it)) {
        let mut ext = shift(it);
        if ext.is_undef() {
            error(it, s("Expecting extension for -ext option\n"))?;
            set(it, "badCmd", n(1));
            return Ok(true);
        }
        let flag = if re("^-", "").is_match(&topic(it)) {
            0
        } else if cap(&c, 2).truthy() {
            2
        } else {
            1
        };
        let ff = ops::bit_or(&get(it, "filterFlag"), &ops::shift_left(&n(0x01), &n(flag)));
        set(it, "filterFlag", ff);
        re(r"^\.", "").subst(&mut ext, false, |_| Vec::new()); // remove leading '.' if it exists
        lex_hash(it, "filterExt").set(pl::uc(&ext).to_bytes(), n(if flag != 0 { 1 } else { 0 }));
        return Ok(true);
    }
    if re("^f$", "").is_match(&topic(it)) || a_is("forceprint") {
        set(it, "forcePrint", n(1));
        return Ok(true);
    }
    let fixbase = re(r"^F([-+]?\d*)$", "").m(&topic(it)).or_else(|| re(r"^fixbase([-+]?\d*)$", "i").m(&topic(it)));
    if let Some(c) = fixbase {
        options(it, vec![s("FixBase"), cap(&c, 1)])?;
        return Ok(true);
    }
    if let Some(c) = re(r"^fast(\d*)$", "i").m(&topic(it)) {
        let c1 = cap(&c, 1);
        options(it, vec![s("FastScan"), if pl::length(&c1).truthy() { c1 } else { n(1) }])?;
        return Ok(true);
    }
    if let Some(c) = re(r"^(file\d+)$", "i").m(&topic(it)) {
        let v = shift(it);
        lex_hash(it, "altFile").set(pl::lc(&cap(&c, 1)).to_bytes(), v.clone());
        if !v.truthy() {
            error(it, s("Expecting file name for -file option\n"))?;
            set(it, "badCmd", n(1));
            return Ok(true);
        }
        return Ok(true);
    }
    if let Some(c) = re(r"^fileorder(\d*)$", "i").m(&topic(it)) {
        if !argv(it).is_empty() {
            let v = shift(it);
            push(it, "fileOrder", v);
        }
        let c1 = cap(&c, 1);
        let num = if c1.truthy() { c1 } else { n(0) };
        let fof = get(it, "fileOrderFast");
        if fof.is_undef() || pl::gt(&fof, &num) {
            set(it, "fileOrderFast", num);
        }
        return Ok(true);
    }
    if a_is("globaltimeshift") {
        let v = shift(it);
        options(it, vec![s("GlobalTimeShift"), v])?;
        return Ok(true);
    }
    if let Some(c) = re(r"^(g)(roupHeadings|roupNames)?([\d:]*)$", "i").m(&topic(it)) {
        let c3 = cap(&c, 3);
        let show_group = if c3.truthy() { c3 } else { n(0) };
        set(it, "showGroup", show_group.clone());
        let c2 = cap(&c, 2);
        let all = if c2.truthy() { pl::str_eq(&pl::lc(&c2), "roupnames") } else { pl::str_eq(&cap(&c, 1), "G") };
        set(it, "allGroup", pbool(all));
        if re(r"\b5\b", "").is_match(&show_group) {
            options(it, vec![s("SavePath"), n(1)])?;
        }
        if re(r"\b6\b", "").is_match(&show_group) {
            options(it, vec![s("SaveFormat"), n(1)])?;
        }
        return Ok(true);
    }
    if a_is("geotag") {
        let mut trkfile = shift(it);
        if !truthy(it, "pass") {
            // defer to next pass so the filename charset is available
            push(it, "nextPass", s("-geotag"));
            push(it, "nextPass", trkfile);
            return Ok(true);
        }
        if !trkfile.truthy() {
            error(it, s("Expecting file name for -geotag option\n"))?;
            set(it, "badCmd", n(1));
            return Ok(true);
        }
        // allow wildcards in filename
        if pl::call(it, "main::HasWildcards", vec![trkfile.clone()])?.truthy() {
            // CORE::glob() splits on white space, so use File::Glob if possible
            let mut trks;
            let os = perl1(it, "$^O", &[])?;
            if pl::str_eq(&os, "MSWin32") && eval_require(it, "Win32::FindFile")? {
                // ("-charset filename=UTF8" must be set for this to work with Unicode file names)
                let mt = get(it, "mt");
                trks = pl::call_list(it, "main::FindFileWindows", vec![mt, trkfile.clone()])?;
            } else if eval_require(it, "File::Glob")? {
                trks = pl::call_list(it, "File::Glob::bsd_glob", vec![trkfile.clone()])?;
            } else {
                trks = perl_code(it, "glob($trkfile)", &[("trkfile", trkfile.clone())], Want::List)?;
            }
            if trks.is_empty() {
                error(it, s("No matching file found for -geotag option\n"))?;
                set(it, "badCmd", n(1));
                return Ok(true);
            }
            while trks.len() > 1 {
                let f = trks.remove(0);
                push(it, "newValues", cat(&[s("geotag="), f]));
            }
            trkfile = trks.pop().unwrap_or_default();
        }
        set_topic(it, cat(&[s("geotag="), trkfile]));
        // (fall through!)
    }
    if re("^h$", "").is_match(&topic(it)) || a_is("htmlformat") {
        crate::runtime::require(it, "Image::ExifTool::HTML")?;
        set(it, "html", n(1));
        set(it, "escapeHTML", n(1));
        set(it, "json", n(0));
        set(it, "xml", n(0));
        return Ok(true);
    }
    if re("^H$", "").is_match(&topic(it)) || a_is("hex") {
        set(it, "showTagID", s("H"));
        return Ok(true);
    }
    if let Some(c) = re(r"^htmldump([-+]?\d+)?$", "i").m(&topic(it)) {
        let v = get(it, "verbose");
        set(it, "verbose", pl::add(&if v.truthy() { v } else { n(0) }, &n(1)));
        set(it, "html", n(2));
        let c1 = cap(&c, 1);
        if !c1.is_undef() {
            options(it, vec![s("HtmlDumpBase"), c1])?;
        }
        return Ok(true);
    }
    if re("^i(gnore)?$", "i").is_match(&topic(it)) {
        let dir = shift(it);
        if dir.is_undef() {
            error(it, s("Expecting directory name for -i option\n"))?;
            set(it, "badCmd", n(1));
            return Ok(true);
        }
        lex_hash(it, "ignore").set(dir.to_bytes(), n(1));
        if pl::str_eq(&dir, "HIDDEN") {
            set(it, "ignoreHidden", n(1));
        }
        return Ok(true);
    }
    if let Some(c) = re(r"^if(\d*)$", "i").m(&topic(it)) {
        let cond = shift(it);
        let c1 = cap(&c, 1);
        let fast = if pl::length(&c1).truthy() { c1 } else { Value::Undef };
        if cond.is_undef() {
            error(it, s("Expecting expression for -if option\n"))?;
            set(it, "badCmd", n(1));
            return Ok(true);
        }
        // use lowest -fast setting if multiple conditions
        let fc = get(it, "fastCondition");
        if lex_array(it, "condition").is_empty() || fast.is_undef() || (!fc.is_undef() && pl::gt(&fc, &fast)) {
            set(it, "fastCondition", fast);
        }
        // prevent processing file unnecessarily for simple case of failed '$ok' or 'not $ok'
        if let Some(m) = re(r"^\s*(not\s*)\$ok\s*$", "i").m(&cond) {
            if cap(&m, 1).truthy() != get(it, "rtnValPrev").truthy() {
                set(it, "failCondition", n(1));
            }
        }
        // add to list of requested tags
        for tag in re(r"\$\{?((?:[-_0-9A-Z]+:)*[-_0-9A-Z?*]+)", "i").all(&cond) {
            push(it, "requestTags", tag);
        }
        push(it, "condition", cond);
        return Ok(true);
    }
    if re("^(k|pause)$", "i").is_match(&topic(it)) {
        set(it, "pause", n(1));
        return Ok(true);
    }
    if re("^l$", "").is_match(&topic(it)) || a_is("long") {
        let v = pl::sub(&get(it, "outFormat"), &n(1));
        set(it, "outFormat", v);
        return Ok(true);
    }
    if re("^L$", "").is_match(&topic(it)) || a_is("latin") {
        options(it, vec![s("Charset"), s("Latin")])?;
        return Ok(true);
    }
    if a_is("lang") {
        return lang(it);
    }
    if a_is("listitem") {
        let li = shift(it);
        if !(!li.is_undef() && pl::call(it, "Image::ExifTool::IsInt", vec![li.clone()])?.truthy()) {
            warn_(it, s("Expecting integer for -listItem option\n"))?;
            return Ok(true);
        }
        options(it, vec![s("ListItem"), li.clone()])?;
        set(it, "listItem", li);
        return Ok(true);
    }
    if re("^(m|ignoreminorerrors)$", "i").is_match(&topic(it)) {
        options(it, vec![s("IgnoreMinorErrors"), n(1)])?;
        return Ok(true);
    }
    if re("^(n|-printconv)$", "i").is_match(&topic(it)) {
        options(it, vec![s("PrintConv"), n(0)])?;
        return Ok(true);
    }
    if re("^(-n|printconv)$", "i").is_match(&topic(it)) {
        options(it, vec![s("PrintConv"), n(1)])?;
        return Ok(true);
    }
    if a_is("nop") {
        // (undocumented) no operation, added in 11.25
        set(it, "helped", n(1));
        return Ok(true);
    }
    if re("^o(ut)?$", "i").is_match(&topic(it)) {
        let mut out_opt = shift(it);
        set(it, "outOpt", out_opt.clone());
        if out_opt.is_undef() {
            error(it, s("Expected output file or directory name for -o option\n"))?;
            set(it, "badCmd", n(1));
            return Ok(true);
        }
        pl::call_mut(it, "main::CleanFilename", std::slice::from_mut(&mut out_opt), Want::Void)?;
        set(it, "outOpt", out_opt);
        // verbose messages go to STDERR of output is to console
        if re(r"^-(\.\w+)?$", "").is_match(&get(it, "vout")) {
            let e = glob(it, "STDERR")?;
            set(it, "vout", e);
        }
        return Ok(true);
    }
    if a_is("overwrite_original") {
        set(it, "overwriteOrig", n(1));
        return Ok(true);
    }
    if a_is("overwrite_original_in_place") {
        set(it, "overwriteOrig", n(2));
        return Ok(true);
    }
    if a_is("plot") {
        crate::runtime::require(it, "Image::ExifTool::Plot")?;
        let plot = pl::method(it, &s("Image::ExifTool::Plot"), "new", vec![])?;
        set(it, "plot", plot);
        return Ok(true);
    }
    let pfmt = re("^p(-?)$", "").m(&topic(it)).or_else(|| re("^printformat(-?)$", "i").m(&topic(it)));
    if let Some(c) = pfmt {
        let fmt = shift(it);
        if truthy(it, "pass") {
            let c1 = cap(&c, 1);
            let flag = if c1.truthy() { c1 } else { get(it, "binaryOutput") };
            vcall(it, "main::LoadPrintFormat", vec![fmt, flag])?;
            // load MWG module now if necessary
            if !truthy(it, "useMWG") {
                let mwg = re(r"^([-_0-9A-Z]+:)*1?mwg:", "i");
                if lex_array(it, "requestTags").items().iter().any(|t| mwg.is_match(t)) {
                    set(it, "useMWG", n(1));
                    crate::runtime::require(it, "Image::ExifTool::MWG")?;
                    vcall(it, "Image::ExifTool::MWG::Load", vec![])?;
                }
            }
        } else {
            // defer to next pass so the filename charset is available
            let v = cat(&[s("-"), topic(it)]);
            push(it, "nextPass", v);
            push(it, "nextPass", fmt);
        }
        return Ok(true);
    }
    if re("^P$", "").is_match(&topic(it)) || a_is("preserve") {
        set(it, "preserveTime", n(1));
        return Ok(true);
    }
    if a_is("password") {
        let v = shift(it);
        options(it, vec![s("Password"), v])?;
        return Ok(true);
    }
    if let Some(c) = re(r"^progress(\d*)(:.*)?$", "i").m(&topic(it)) {
        let c1 = cap(&c, 1);
        set(it, "progressIncr", if c1.truthy() { c1 } else { n(1) });
        set(it, "progressNext", n(0)); // start showing progress at the first file
        let c2 = cap(&c, 2);
        if c2.truthy() {
            let mut wt = pl::substr(&c2, 1, None);
            if !pl::length(&wt).truthy() {
                wt = s("ExifTool %p%%");
            }
            set(it, "windowTitle", wt.clone());
            if get(it, "progress").is_undef() && re(r"%\d*[bpr]", "").is_match(&wt) {
                set(it, "progress", n(0));
            }
        } else {
            set(it, "progress", n(1));
            if get(it, "verbose").is_undef() {
                set(it, "verbose", n(0));
            }
        }
        set(it, "progressCount", n(0));
        return Ok(true);
    }
    if let Some(c) = re(r"^purge(\d*)$", "i").m(&topic(it)) {
        // (undocumented) added in 13.53
        let c1 = cap(&c, 1);
        let purge = if c1.truthy() { c1 } else { n(1) };
        set(it, "purge", purge.clone());
        vcall(it, "Image::ExifTool::Purge", vec![purge])?;
        return Ok(true);
    }
    if re("^q(uiet)?$", "i").is_match(&topic(it)) {
        let v = pl::add(&get(it, "quiet"), &n(1));
        set(it, "quiet", v);
        return Ok(true);
    }
    if let Some(c) = re(r"^r(ecurse)?(\.?)$", "i").m(&topic(it)) {
        set(it, "recurse", n(if cap(&c, 2).truthy() { 2 } else { 1 }));
        return Ok(true);
    }
    if a_is("require") {
        // (undocumented) added in version 8.65
        let ver = shift(it);
        if !(!ver.is_undef() && pl::call(it, "Image::ExifTool::IsFloat", vec![ver.clone()])?.truthy()) {
            error(it, s("Expecting version number for -require option\n"))?;
            set(it, "badCmd", n(1));
            return Ok(true);
        }
        let lib_ver = it.get_scalar("Image::ExifTool::VERSION");
        if !pl::ge(&lib_ver, &ver) {
            error(it, cat(&[s("Requires ExifTool version "), ver, s(" or later\n")]))?;
            set(it, "badCmd", n(1));
        }
        return Ok(true);
    }
    if a_is("restore_original") {
        set(it, "deleteOrig", n(0));
        return Ok(true);
    }
    if re("^S$", "").is_match(&topic(it)) || a_is("veryshort") {
        let v = pl::add(&get(it, "outFormat"), &n(2));
        set(it, "outFormat", v);
        return Ok(true);
    }
    if let Some(c) = re(r"^s(hort)?(\d*)$", "i").m(&topic(it)) {
        let c2 = cap(&c, 2);
        let v = if pl::str_eq(&c2, "") { pl::add(&get(it, "outFormat"), &n(1)) } else { c2 };
        set(it, "outFormat", v);
        return Ok(true);
    }
    if a_is("scanforxmp") {
        options(it, vec![s("ScanForXMP"), n(1)])?;
        return Ok(true);
    }
    if re("^sep(arator)?$", "i").is_match(&topic(it)) {
        let list_sep = shift(it);
        let mut sep = list_sep.clone();
        set(it, "listSep", list_sep.clone());
        if list_sep.is_undef() {
            error(it, s("Expecting list item separator for -sep option\n"))?;
            set(it, "badCmd", n(1));
            return Ok(true);
        }
        let unescape_char = lex_hash(it, "unescapeChar");
        re(r"\\(.)", "s").subst(&mut sep, true, |c| {
            // translate escape sequences
            let k = cap(c, 1);
            let v = unescape_char.get(k.to_bytes());
            if v.truthy() { v.to_bytes().to_vec() } else { k.to_bytes().to_vec() }
        });
        if !get(it, "binSep").is_undef() {
            set(it, "binTerm", sep);
        } else {
            set(it, "binSep", sep);
        }
        options(it, vec![s("ListSep"), list_sep.clone()])?;
        set(it, "joinLists", n(1));
        // also split when writing values
        let mut list_split = quotemeta(&list_sep);
        // a space in the string matches zero or more whitespace characters
        re(r"(\\ )+", "").subst(&mut list_split, true, |_| b"\\s*".to_vec());
        // but a single space alone matches one or more whitespace characters
        if pl::str_eq(&list_split, "\\s*") {
            list_split = s("\\s+");
        }
        options(it, vec![s("ListSplit"), list_split])?;
        return Ok(true);
    }
    if let Some(c) = re("^(-)?sort$", "i").m(&topic(it)) {
        set(it, "sortOpt", n(if cap(&c, 1).truthy() { 0 } else { 1 }));
        return Ok(true);
    }
    if a_is("srcfile") {
        if argv(it).is_empty() {
            warn_(it, s("Expecting FMT for -srcfile option\n"))?;
            return Ok(true);
        }
        let v = shift(it);
        push(it, "srcFmt", v);
        return Ok(true);
    }
    if a_is("stay_open") {
        let arg = shift(it);
        if arg.is_undef() {
            warn_(it, s("Expecting argument for -stay_open option\n"))?;
            return Ok(true);
        }
        let stay_open = get(it, "stayOpen");
        if re("^(1|true)$", "i").is_match(&arg) {
            if !stay_open.truthy() {
                set(it, "stayOpen", n(1));
            } else if num_is(&stay_open, 2) {
                set(it, "stayOpen", n(3)); // chained -stay_open options
            } else {
                warn_(it, s("-stay_open already active\n"))?;
            }
        } else if re("^(0|false)$", "i").is_match(&arg) {
            if pl::ge(&stay_open, &n(2)) {
                // close -stay_open argfile and process arguments up to this point
                perl_code(it, "close STAYOPEN", &[], Want::Void)?;
                let more = lex_array(it, "moreArgs").items();
                let av = argv(it);
                for m in more {
                    av.push(m);
                }
                clear(it, "moreArgs");
            } else if !stay_open.truthy() {
                warn_(it, s("-stay_open wasn't active\n"))?;
            }
            set(it, "stayOpen", n(0));
        } else {
            warn_(it, s("Invalid argument for -stay_open\n"))?;
        }
        return Ok(true);
    }
    if let Some(c) = re("^(-)?struct$", "i").m(&topic(it)) {
        options(it, vec![s("Struct"), n(if cap(&c, 1).truthy() { 0 } else { 1 })])?;
        return Ok(true);
    }
    if re("^t(ab)?$", "").is_match(&topic(it)) {
        set(it, "tabFormat", n(1));
        return Ok(true);
    }
    if re("^T$", "").is_match(&topic(it)) || a_is("table") {
        set(it, "forcePrint", n(1));
        set(it, "tabFormat", n(1));
        let v = pl::add(&get(it, "outFormat"), &n(2));
        set(it, "outFormat", v);
        let q = pl::add(&get(it, "quiet"), &n(1));
        set(it, "quiet", q);
        return Ok(true);
    }
    if let Some(c) = re("^(u)(nknown(2)?)?$", "i").m(&topic(it)) {
        let inc = if cap(&c, 3).truthy() || (!cap(&c, 2).truthy() && pl::str_eq(&cap(&c, 1), "U")) { 2 } else { 1 };
        let cur = options(it, vec![s("Unknown")])?;
        options(it, vec![s("Unknown"), pl::add(&cur, &n(inc))])?;
        return Ok(true);
    }
    if a_is("use") {
        return use_module(it);
    }
    if a_is("userparam") {
        let mut opt = shift(it);
        if opt.is_undef() {
            error(it, s("Expected parameter for -userParam option\n"))?;
            set(it, "badCmd", n(1));
            return Ok(true);
        }
        if !re("=", "").is_match(&opt) {
            opt = cat(&[opt, s("=1")]);
        }
        options(it, vec![s("UserParam"), opt])?;
        return Ok(true);
    }
    if let Some(c) = re(r"^v(erbose)?(\d*)$", "i").m(&topic(it)) {
        let c2 = cap(&c, 2);
        let v = if pl::str_eq(&c2, "") {
            let v = get(it, "verbose");
            pl::add(&if v.truthy() { v } else { n(0) }, &n(1))
        } else {
            c2
        };
        set(it, "verbose", v);
        return Ok(true);
    }
    if let Some(c) = re("^(w|textout|tagout)([!+]*)$", "i").m(&topic(it)) {
        // (note: all logic ignores $textOut of 0 or '')
        let mut text_out = shift(it);
        if !text_out.truthy() {
            let msg = cat(&[s("Expecting argument for -"), topic(it), s(" option\n")]);
            text_out = warn_(it, msg)?;
        }
        set(it, "textOut", text_out.clone());
        let (t1, t2) = (cap(&c, 1), cap(&c, 2));
        let mut text_overwrite = n(0);
        if re("!", "").is_match(&t2) {
            text_overwrite = pl::add(&text_overwrite, &n(1)); // overwrite
        }
        if re(r"\+", "").is_match(&t2) {
            text_overwrite = pl::add(&text_overwrite, &n(2)); // append
        }
        set(it, "textOverwrite", text_overwrite.clone());
        if !pl::str_eq(&t1, "W") && !pl::str_eq(&pl::lc(&t1), "tagout") {
            set(it, "tagOut", Value::Undef);
        } else if pl::ge(&text_overwrite, &n(2)) && !re(r"%[-+]?\d*[.:]?\d*[lu]?[tgso]", "").is_match(&text_out) {
            set(it, "tagOut", n(0)); // append tags to one file
        } else {
            set(it, "tagOut", n(1)); // separate file for each tag
        }
        return Ok(true);
    }
    if let Some(c) = re("^(-?)(wext|tagoutext)$", "i").m(&topic(it)) {
        let mut ext = shift(it);
        if ext.is_undef() {
            error(it, s("Expecting extension for -wext option\n"))?;
            set(it, "badCmd", n(1));
            return Ok(true);
        }
        let mut flag = n(1);
        let wext = lex_hash(it, "wext");
        if cap(&c, 1).truthy() {
            wext.set("*", n(1));
            flag = n(-1);
        }
        re(r"^\.", "").subst(&mut ext, false, |_| Vec::new());
        wext.set(pl::lc(&ext).to_bytes(), flag);
        return Ok(true);
    }
    if a_is("wm") || a_is("writemode") {
        let wm = shift(it);
        if wm.is_undef() {
            let msg = cat(&[s("Expecting argument for -"), topic(it), s(" option\n")]);
            error(it, msg)?;
            set(it, "badCmd", n(1));
            return Ok(true);
        }
        if !re("^[wcg]*$", "i").is_match(&wm) {
            let msg = cat(&[s("Invalid argument for -"), topic(it), s(" option\n")]);
            error(it, msg)?;
            set(it, "badCmd", n(1));
            return Ok(true);
        }
        options(it, vec![s("WriteMode"), wm])?;
        return Ok(true);
    }
    if re("^x$", "").is_match(&topic(it)) || a_is("exclude") {
        let mut tag = shift(it);
        if tag.is_undef() {
            error(it, s("Expecting tag name for -x option\n"))?;
            set(it, "badCmd", n(1));
            return Ok(true);
        }
        re(r"\ball\b", "i").subst(&mut tag, true, |_| b"*".to_vec()); // replace 'all' with '*' in tag names
        let tags_from = get(it, "tagsFrom");
        if !tags_from.truthy() {
            push(it, "exclude", tag);
        } else if pl::str_eq(&tags_from, "CSV") {
            push(it, "csvExclude", tag);
        } else {
            let stf = get(it, "setTagsFile");
            lex_hash(it, "setTags").get_or_new_array(stf.to_bytes()).push(cat(&[s("-"), tag]));
        }
        return Ok(true);
    }
    if re("^X$", "").is_match(&topic(it)) || a_is("xmlformat") {
        set(it, "xml", n(1));
        set(it, "json", n(0));
        set(it, "html", n(0));
        options(it, vec![s("Duplicates"), n(1)])?;
        return Ok(true);
    }
    if a_is("php") {
        set(it, "json", n(2));
        set(it, "xml", n(0));
        set(it, "html", n(0));
        options(it, vec![s("Duplicates"), n(1)])?;
        return Ok(true);
    }
    if re("^z(ip)?$", "i").is_match(&topic(it)) {
        set(it, "doUnzip", n(1));
        options(it, vec![s("Compress"), n(1), s("XMPShorthand"), n(1)])?;
        if !options(it, vec![s("Compact")])?.truthy() {
            options(it, vec![s("Compact"), n(1)])?;
        }
        return Ok(true);
    }
    if pl::str_eq(&topic(it), "") {
        // read STDIN
        push(it, "files", s("-"));
        set(it, "srcStdin", n(1));
        return Ok(true);
    }
    if pl::str_eq(&pl::length(&topic(it)), "1") && !pl::str_eq(&topic(it), "*") {
        let msg = cat(&[s("Unknown option -"), topic(it), s("\n")]);
        error(it, msg)?;
        set(it, "badCmd", n(1));
        return Ok(true);
    }
    tag_argument(it, dollar1)?;
    Ok(false)
}

/// The -listgeo output (the 'geo' type of -list).
fn list_geo(it: &mut Interp) -> ER<()> {
    crate::runtime::require(it, "Image::ExifTool::Geolocation")?;
    if !truthy(it, "quiet") {
        pl::print_to(it, None, &[s("Geolocation database:\n")])?;
    }
    let mut is_alt = if options(it, vec![s("GeolocAltNames")])?.truthy() { s(",AltNames") } else { s("") };
    if is_alt.truthy() && !pl::call(it, "Image::ExifTool::Geolocation::ReadAltNames", vec![])?.truthy() {
        is_alt = s("");
    }
    let head = cat(&[s("City,Region,Subregion,CountryCode,Country,TimeZone,FeatureCode,Population,Latitude,Longitude"), is_alt.clone(), s("\n")]);
    pl::print_to(it, None, &[head])?;
    if truthy(it, "sortOpt") {
        vcall(it, "Image::ExifTool::Geolocation::SortDatabase", vec![s("City")])?;
    }
    let min_pop = options(it, vec![s("GeolocMinPop")])?;
    let f = options(it, vec![s("GeolocFeature")])?;
    let mut feature = if f.truthy() { f } else { s("") };
    let neg = re("^-", "").subst(&mut feature, false, |_| Vec::new()) > 0;
    let fcodes = Value::new_hash();
    for code in pl::split(&re(r"\s*,\s*", ""), &feature, 0) {
        fcodes.set(pl::lc(&code).to_bytes(), n(1));
    }
    let mut is_utf8 = vec![0usize, 1, 2, 4]; // items that need converting from UTF8
    if is_alt.truthy() {
        is_utf8.push(10);
    }
    let lang_opt = get(it, "langOpt");
    let mut i: i64 = 0;
    loop {
        let mut entry = pl::call_list(it, "Image::ExifTool::Geolocation::GetEntry", vec![n(i), lang_opt.clone(), n(1)])?;
        if entry.is_empty() {
            break;
        }
        entry.resize(10, Value::Undef); // remove everything after latitude (eg. feature type)
        // (perl warns here if $entry[7] is undef)
        let skip = (min_pop.truthy() && pl::lt(&entry[7], &min_pop))
            || (!fcodes.is_empty() && {
                let has = fcodes.get(pl::lc(&entry[6]).to_bytes()).truthy();
                if neg { has } else { !has }
            });
        if !skip {
            if is_alt.truthy() {
                entry.extend(pl::call_list(it, "Image::ExifTool::Geolocation::GetAltNames", vec![n(i), n(1)])?);
            }
            let mt = get(it, "mt");
            for &k in &is_utf8 {
                if entry.len() <= k {
                    entry.resize(k + 1, Value::Undef);
                }
                let v = entry[k].clone();
                entry[k] = if v.is_undef() { s("") } else { pl::method(it, &mt, "Decode", vec![v, s("UTF8")])? };
            }
            if is_alt.truthy() && !entry.get(10).is_some_and(|v| v.truthy()) {
                entry.pop();
            }
            let line = pl::join(",", &entry);
            pl::print_to(it, None, &[line, s("\n")])?;
        }
        i += 1;
    }
    Ok(())
}

/// The -ver output (in the second pass).
fn print_version(it: &mut Interp) -> ER<()> {
    let lib_ver = it.get_scalar("Image::ExifTool::VERSION");
    let version = get(it, "version");
    let str_ = if pl::str_eq(&lib_ver, &version.to_text()) { s("") } else { cat(&[s(" [Warning: Library version is "), lib_ver, s("]")]) };
    let release = it.get_scalar("Image::ExifTool::RELEASE");
    let verbose = get(it, "verbose");
    if verbose.truthy() {
        pl::print_to(it, None, &[cat(&[s("ExifTool version "), version, str_, release, s("\n")])])?;
        let perl_ver = perl1(it, "$]", &[])?;
        let unicode = perl1(it, "${^UNICODE}", &[])?;
        let uc_str = if !unicode.is_undef() { cat(&[s(" (-C"), unicode, s(")")]) } else { s("") };
        pl::print_to(it, None, &[pl::sprintf("Perl version %s%s\n", &[perl_ver, uc_str])])?;
        let os = perl1(it, "$^O", &[])?;
        pl::print_to(it, None, &[cat(&[s("Platform: "), os.clone(), s("\n")])])?;
        if pl::gt(&verbose, &n(8)) {
            if eval_require(it, "Cwd")? {
                let cwd = pl::call(it, "Cwd::getcwd", vec![])?;
                pl::print_to(it, None, &[cat(&[s("Current Dir: "), cwd, s("\n")])])?;
            }
            let script = it.get_scalar("main::0");
            pl::print_to(it, None, &[cat(&[s("Script Name: "), script, s("\n")])])?;
            let exe = perl1(it, "$^X", &[])?;
            pl::print_to(it, None, &[cat(&[s("Exe Name:    "), exe, s("\n")])])?;
            let exe_dir = it.get_scalar("Image::ExifTool::exeDir");
            pl::print_to(it, None, &[cat(&[s("Exe Dir:     "), exe_dir, s("\n")])])?;
            let exe_path = get(it, "exePath");
            pl::print_to(it, None, &[cat(&[s("Exe Path:    "), exe_path, s("\n")])])?;
        }
        pl::print_to(it, None, &[s("Optional libraries:\n")])?;
        let recommends = lex_array(it, "recommends");
        let alt_recommends = lex_hash(it, "altRecommends");
        // (foreach aliases $_ to the elements of @recommends)
        for k in 0..recommends.len() {
            let mut m = recommends.at(k as i64);
            if re("^Win32", "").is_match(&m) && !pl::str_eq(&os, "MSWin32") {
                continue;
            }
            if re("StatX", "").is_match(&m) && !pl::str_eq(&os, "linux") {
                continue;
            }
            let mut ver = eval_string(it, &cat(&[s("require "), m.clone(), s(" and $"), m.clone(), s("::VERSION")]));
            let alt = alt_recommends.get(m.to_bytes());
            // check for alternative if primary not available
            if !ver.truthy() && alt.truthy() {
                ver = eval_string(it, &cat(&[s("require "), alt.clone(), s(" and $"), alt.clone(), s("::VERSION")]));
                if ver.truthy() {
                    m = alt;
                    recommends.set_at(k, m.clone());
                }
            }
            let shown = if ver.truthy() { ver } else { s("(not installed)") };
            pl::print_to(it, None, &[pl::sprintf("  %-28s %s\n", &[m, shown])])?;
        }
        if pl::gt(&verbose, &n(1)) {
            pl::print_to(it, None, &[s("Include directories:\n")])?;
            for d in pl::pkg_array(it, "main::INC").items() {
                if !d.is_ref() {
                    pl::print_to(it, None, &[cat(&[s("  "), d, s("\n")])])?;
                }
            }
        }
    } else {
        pl::print_to(it, None, &[cat(&[version, str_, release, s("\n")])])?;
    }
    Ok(())
}

/// The -@ option: read the arguments of an argfile.
fn arg_file(it: &mut Interp) -> ER<bool> {
    let arg_file = shift(it);
    if !arg_file.truthy() {
        error(it, s("Expecting filename for -@ option\n"))?;
        set(it, "badCmd", n(1));
        return Ok(true);
    }
    // switch to new ARGFILE if using chained -stay_open options
    let stay_open = get(it, "stayOpen");
    if num_is(&stay_open, 1) {
        // defer remaining arguments until we close this argfile
        let av = argv(it).items();
        let more = lex_array(it, "moreArgs");
        if let Value::ArrayRef(m) = &more {
            *m.borrow_mut() = av;
        }
        if let Value::ArrayRef(a) = argv(it) {
            a.borrow_mut().clear();
        }
    } else if num_is(&stay_open, 3) {
        let sof = get(it, "stayOpenFile");
        if sof.truthy() && !pl::str_eq(&sof, "-") && pl::str_eq(&arg_file, &sof.to_text()) {
            // don't allow user to switch to the same -stay_open argfile
            // because it will result in endless recursion
            set(it, "stayOpen", n(2));
            warn_(it, cat(&[s("Ignoring request to switch to the same -stay_open ARGFILE ("), arg_file, s(")\n")]))?;
            return Ok(true);
        }
        perl_code(it, "close STAYOPEN", &[], Want::Void)?;
        set(it, "stayOpen", n(1)); // switch to this -stay_open file
    }
    let fp = if num_is(&get(it, "stayOpen"), 1) { glob(it, "STAYOPEN")? } else { glob(it, "ARGFILE")? };
    let mt = get(it, "mt");
    if !pl::method(it, &mt, "Open", vec![fp.clone(), arg_file.clone()])?.truthy() {
        let exe_dir = it.get_scalar("Image::ExifTool::exeDir");
        let ok = !re("^/", "").is_match(&arg_file) && pl::method(it, &mt, "Open", vec![fp, cat(&[exe_dir, s("/"), arg_file.clone()])])?.truthy();
        if !ok {
            error(it, cat(&[s("Error opening arg file "), arg_file, s("\n")]))?;
            set(it, "badCmd", n(1));
            return Ok(true);
        }
    }
    if num_is(&get(it, "stayOpen"), 1) {
        set(it, "stayOpenFile", arg_file); // remember the name of the file we have open
        set(it, "stayOpenBuff", s("")); // initialize buffer for reading this file
        set(it, "stayOpen", n(2));
        set(it, "helped", n(1));
        let av = argv(it);
        vcall(it, "main::ReadStayOpen", vec![av])?;
        return Ok(true);
    }
    let mut new_args = Vec::new();
    let mut did_bom = false;
    for mut line in perl_code(it, "<ARGFILE>", &[], Want::List)? {
        // filter Byte Order Mark if it exists from start of UTF-8 text file
        if !did_bom {
            re(r"^\xef\xbb\xbf", "").subst(&mut line, false, |_| Vec::new());
            did_bom = true;
        }
        let line = pl::call(it, "main::FilterArgfileLine", vec![line])?;
        if !line.is_undef() {
            new_args.push(line);
        }
    }
    perl_code(it, "close ARGFILE", &[], Want::Void)?;
    if let Value::ArrayRef(a) = argv(it) {
        let mut a = a.borrow_mut();
        let rest = std::mem::take(&mut *a);
        a.extend(new_args);
        a.extend(rest);
    }
    Ok(true)
}

/// The -charset option.
fn charset(it: &mut Interp) -> ER<bool> {
    let a0 = argv(it).at(0);
    let mut charset = if !argv(it).is_empty() && !re(r"^(-|\xe2\x88\x92)", "").is_match(&a0) { shift(it) } else { Value::Undef };
    if !charset.truthy() {
        if !truthy(it, "pass") {
            push(it, "nextPass", s("-charset"));
            if !charset.is_undef() {
                push(it, "nextPass", charset);
            }
            return Ok(true);
        }
        let charsets = Value::new_hash();
        if let Value::HashRef(h) = pl::pkg_hash(it, "Image::ExifTool::charsetName") {
            for v in h.borrow().values() {
                charsets.set(v.to_bytes(), n(1));
            }
        }
        let mut keys: Vec<Value> = charsets.keys().iter().map(|k| Value::bytes(&**k)).collect();
        keys.sort_by(|x, y| x.to_bytes().cmp(&y.to_bytes()));
        let mut args = vec![s("Available character sets")];
        args.extend(keys);
        vcall(it, "main::PrintTagList", args)?;
        set(it, "helped", n(1));
        return Ok(true);
    }
    let mut ty = None;
    let matched = re(r"^(\w+)=", "").subst(&mut charset, false, |c| {
        ty = Some(cap(c, 1));
        Vec::new()
    }) > 0;
    let ty = ty.unwrap_or_default();
    if !matched || pl::str_eq(&pl::lc(&ty), "exiftool") {
        pl::local_sig_warn(it, eval_warning_handler(), |it| {
            set(it, "evalWarning", Value::Undef);
            options(it, vec![s("Charset"), charset.clone()])?;
            Ok(())
        })?;
        let ew = get(it, "evalWarning");
        if ew.truthy() {
            warn_(it, ew)?;
        } else {
            let cs = options(it, vec![s("Charset")])?;
            set(it, "setCharset", cs);
        }
    } else {
        // set internal encoding of specified metadata type
        let t = match &*pl::lc(&ty).to_bytes() {
            b"id3" => "ID3",
            b"iptc" => "IPTC",
            b"exif" => "EXIF",
            b"filename" => "FileName",
            b"photoshop" => "Photoshop",
            b"quicktime" => "QuickTime",
            b"riff" => "RIFF",
            _ => "",
        };
        if t.is_empty() {
            warn_(it, cat(&[s("Unknown type for -charset option: "), ty, s("\n")]))?;
            return Ok(true);
        }
        options(it, vec![s(&format!("Charset{t}")), charset])?;
    }
    Ok(true)
}

/// The -csv and -json options (and their -csv=FILE / -json=FILE import).
fn csv_json(it: &mut Interp, c: &pl::Caps) -> ER<bool> {
    let mut db_file = cap(c, 3);
    let db_type = if pl::str_eq(&pl::lc(&cap(c, 1)), "csv") { "CSV" } else { "JSON" };
    if !db_file.truthy() {
        if db_type == "CSV" {
            set(it, "csv", s(db_type));
        } else {
            set(it, "json", n(1));
            set(it, "html", n(0));
            set(it, "xml", n(0));
            options(it, vec![s("Duplicates"), n(1)])?;
            crate::runtime::require(it, "Image::ExifTool::XMP")?; // for FixUTF8()
        }
        return Ok(true);
    }
    // must process on 2nd pass so -f and -charset options are available
    if !truthy(it, "pass") {
        if !lex_array(it, "tags").is_empty() {
            warn_(it, cat(&[s("Tag arguments should come after the -"), cap(c, 1), s("= option\n")]))?;
        }
        let v = cat(&[s("-"), topic(it)]);
        push(it, "nextPass", v);
        let save_count = pl::add(&get(it, "saveCount"), &n(1));
        set(it, "saveCount", save_count.clone());
        let marker = Value::new_hash();
        marker.set("SaveCount", save_count.clone());
        push(it, "newValues", marker); // marker to save new values now
        set(it, "dbSaveCount", save_count);
        set(it, "tagsFrom", s("CSV"));
        return Ok(true);
    }
    let mut prefix = Value::Undef;
    re(r"^(\+?=)", "").subst(&mut db_file, false, |m| {
        prefix = cap(m, 1);
        Vec::new()
    });
    // (perl warns here if $1 is undef)
    if pl::str_eq(&prefix, "+=") {
        set(it, "dbAdd", n(2));
    }
    if truthy(it, "srcStdin") {
        let e = glob(it, "STDERR")?;
        set(it, "vout", e);
    }
    if truthy(it, "verbose") {
        let vout = get(it, "vout");
        pl::print_to(it, Some(&vout), &[cat(&[s("Reading "), s(db_type), s(" file "), db_file.clone(), s("\n")])])?;
    }
    let msg;
    let csvfile = glob(it, "CSVFILE")?;
    let mt = get(it, "mt");
    if pl::method(it, &mt, "Open", vec![csvfile.clone(), db_file.clone()])?.truthy() {
        perl_code(it, "binmode CSVFILE", &[], Want::Void)?;
        crate::runtime::require(it, "Image::ExifTool::Import")?;
        let database = lex_hash(it, "database");
        let force_print = get(it, "forcePrint");
        if db_type == "CSV" {
            let csv_delim = get(it, "csvDelim");
            msg = pl::call(it, "Image::ExifTool::Import::ReadCSV", vec![csvfile, database, force_print, csv_delim])?;
        } else {
            let chset = options(it, vec![s("Charset")])?;
            msg = pl::call(it, "Image::ExifTool::Import::ReadJSON", vec![csvfile, database, force_print, chset])?;
        }
        perl_code(it, "close(CSVFILE)", &[], Want::Void)?;
    } else {
        msg = cat(&[s("Error opening "), s(db_type), s(" file '"), db_file, s("'")]);
    }
    if msg.truthy() {
        warn_(it, cat(&[msg, s("\n")]))?;
    }
    set(it, "isWriting", n(1));
    set(it, "csv", s(db_type));
    Ok(true)
}

/// The -lang option.
fn lang(it: &mut Interp) -> ER<bool> {
    let a0 = argv(it).at(0);
    let mut lang_opt = if !argv(it).is_empty() && !re(r"^(-|\xe2\x88\x92)", "").is_match(&a0) { shift(it) } else { Value::Undef };
    set(it, "langOpt", lang_opt.clone());
    if lang_opt.truthy() {
        // make lower case and use underline as a separator (eg. 'en_ca')
        pl::tr(&mut lang_opt, "-A-Z", "_a-z", "");
        set(it, "langOpt", lang_opt.clone());
        options(it, vec![s("Lang"), lang_opt.clone()])?;
        let cur = options(it, vec![s("Lang")])?;
        if pl::str_eq(&lang_opt, &cur.to_text()) {
            return Ok(true);
        }
    } else if !truthy(it, "pass") {
        push(it, "nextPass", s("-lang"));
        if !lang_opt.is_undef() {
            push(it, "nextPass", lang_opt);
        }
        return Ok(true);
    }
    let mut langs = if truthy(it, "quiet") { s("") } else { s("Available languages:\n") };
    let lang_name = pl::pkg_hash(it, "Image::ExifTool::langName");
    for l in pl::pkg_array(it, "Image::ExifTool::langs").items() {
        let name = lang_name.get(l.to_bytes());
        langs = cat(&[langs, s("  "), l, s(" - "), name, s("\n")]);
    }
    pl::tr(&mut langs, "_", "-", ""); // display dashes instead of underlines in language codes
    if truthy(it, "escapeHTML") {
        langs = pl::call(it, "Image::ExifTool::HTML::EscapeHTML", vec![langs])?;
    }
    let mt = get(it, "mt");
    langs = pl::method(it, &mt, "Decode", vec![langs, s("UTF8")])?;
    if lang_opt.truthy() {
        error(it, cat(&[s("Invalid or unsupported language '"), lang_opt, s("'.\n"), langs]))?;
        set(it, "badCmd", n(1));
        return Ok(true);
    }
    pl::print_to(it, None, &[langs])?;
    set(it, "helped", n(1));
    Ok(true)
}

/// The -use option.
fn use_module(it: &mut Interp) -> ER<bool> {
    let module = shift(it);
    if !module.truthy() {
        error(it, s("Expecting module name for -use option\n"))?;
        set(it, "badCmd", n(1));
        return Ok(true);
    }
    if pl::str_eq(&pl::lc(&module), "mwg") {
        set(it, "useMWG", n(1));
        return Ok(true);
    }
    if re(r"[^\w:]", "").is_match(&module) {
        error(it, cat(&[s("Invalid module name: "), module, s("\n")]))?;
        set(it, "badCmd", n(1));
        return Ok(true);
    }
    pl::local_sig_warn(it, eval_warning_handler(), |it| {
        let ok = eval_string(it, &cat(&[s("require Image::ExifTool::"), module.clone()])).truthy()
            || eval_string(it, &cat(&[s("require "), module.clone()])).truthy()
            || eval_string(it, &cat(&[s("require '"), module.clone(), s("'")])).truthy();
        if !ok {
            error(it, cat(&[s("Error using module "), module.clone(), s("\n")]))?;
            set(it, "badCmd", n(1));
        }
        Ok(())
    })?;
    Ok(true)
}

/// The end of the chain: a tag argument (`-TAG=VALUE`, `-TAG<SRC`,
/// `-TAG`, `--TAG`). `dollar1` is the loop body's `$1` at this point.
fn tag_argument(it: &mut Interp, mut dollar1: Value) -> ER<()> {
    if let Some(c) = re("^[^<]+(<?)=(.*)", "s").m(&topic(it)) {
        let val = cap(&c, 2);
        if cap(&c, 1).truthy()
            && pl::length(&val).truthy()
            && (pl::str_eq(&val, "@") || pl::call(it, "main::FilenameSPrintf", vec![val.clone()])?.is_undef())
        {
            // save count of new values before a dynamic value
            let save_count = pl::add(&get(it, "saveCount"), &n(1));
            set(it, "saveCount", save_count.clone());
            let marker = Value::new_hash();
            marker.set("SaveCount", save_count);
            push(it, "newValues", marker);
        }
        let v = topic(it);
        push(it, "newValues", v);
        let tp = topic(it);
        if re(r"^([-_0-9A-Z]+:)*1?mwg:", "i").is_match(&tp) {
            set(it, "useMWG", n(1));
        } else if re(r"^([-_0-9A-Z]+:)*(filename|directory|testname)\b", "i").is_match(&tp) {
            set(it, "doSetFileName", n(1));
        } else if let Some(g) = re(r"^([-_0-9A-Z]+:)*(geotag|geotime|geosync|geolocate)\b", "i").m(&tp) {
            let g2 = pl::lc(&cap(&g, 2));
            if pl::str_eq(&g2, "geotime") {
                set(it, "addGeotime", s(""));
            } else {
                // add geotag/geosync/geolocate commands first
                if let Value::ArrayRef(nv) = lex_array(it, "newValues") {
                    let mut nv = nv.borrow_mut();
                    let last = nv.pop().unwrap_or_default();
                    nv.insert(0, last);
                }
                let ag = get(it, "addGeotime");
                if pl::str_eq(&g2, "geotag") && (ag.is_undef() || ag.truthy()) && pl::length(&val).truthy() {
                    let g1 = cap(&g, 1);
                    let v = cat(&[
                        if g1.truthy() { g1 } else { s("") },
                        s("Geotime<${DateTimeOriginal#;$_=$self->GetValue('SubSecDateTimeOriginal','ValueConv') || $_}"),
                    ]);
                    set(it, "addGeotime", v);
                }
            }
        }
    } else {
        // assume '-tagsFromFile @' if tags are being redirected
        // and not from CSV and -tagsFromFile hasn't already been specified
        if !truthy(it, "setTagsFile") && !pl::str_eq(&get(it, "tagsFrom"), "CSV") {
            if let Some(m) = re("(<|>)", "").m(&topic(it)) {
                dollar1 = cap(&m, 1);
                set(it, "setTagsFile", s("@"));
                vcall(it, "main::AddSetTagsFile", vec![s("@")])?;
                set(it, "tagsFrom", s("File"));
            }
        }
        if pl::str_eq(&get(it, "tagsFrom"), "CSV") {
            let mut tp = topic(it);
            let removed = re("^-", "").subst(&mut tp, false, |_| Vec::new()) > 0;
            set_topic(it, tp.clone());
            push(it, if removed { "csvExclude" } else { "tags" }, tp);
        } else if truthy(it, "setTagsFile") {
            let stf = get(it, "setTagsFile");
            let tp = topic(it);
            lex_hash(it, "setTags").get_or_new_array(stf.to_bytes()).push(tp.clone());
            // (perl warns here if $1 is undef)
            if pl::str_eq(&dollar1, ">") {
                if re(r"^(.*>\s*)?([-_0-9A-Z]+:)*1?mwg:", "si").is_match(&tp) {
                    set(it, "useMWG", n(1));
                }
                if re(r"\b(filename|directory|testname)#?$", "i").is_match(&tp) {
                    set(it, "doSetFileName", n(1));
                } else if re(r"\bgeotime#?$", "i").is_match(&tp) {
                    set(it, "addGeotime", s(""));
                }
            } else {
                if re(r"^([^<]+<\s*(.*\$\{?)?)?([-_0-9A-Z]+:)*1?mwg:", "si").is_match(&tp) {
                    set(it, "useMWG", n(1));
                }
                if re(r"^([-_0-9A-Z]+:)*(filename|directory|testname)\b", "i").is_match(&tp) {
                    set(it, "doSetFileName", n(1));
                } else if re(r"^([-_0-9A-Z]+:)*geotime\b", "i").is_match(&tp) {
                    set(it, "addGeotime", s(""));
                }
            }
        } else {
            let mut tp = topic(it);
            let removed = re("^-", "").subst(&mut tp, false, |_| Vec::new()) > 0;
            set_topic(it, tp.clone());
            if !re(r"^([-_0-9A-Z*]+:)*([-_0-9A-Z*?]+)#?$", "i").is_match(&tp) {
                warn_(it, cat(&[s("Invalid TAG name: \""), tp.clone(), s("\"\n")]))?;
            }
            push(it, if removed { "exclude" } else { "tags" }, tp); // (push everything for backward compatibility)
        }
    }
    Ok(())
}
