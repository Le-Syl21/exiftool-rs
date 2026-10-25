//! Image::ExifTool (ExifTool.pm), ported sub by sub (see docs/port/PORTING.md).
//! This part: the subs from line 1 to line 4386 (`SetWarning` ... `Init`).

use std::cell::RefCell;
use std::rc::Rc;

use crate::interp::eval::{Binding, ER};
use crate::interp::{Flow, Interp, Want};
use crate::perl::Value;
use crate::pl::{self, arg, cap, re, ret};

// ---- local helpers (Perl idioms not in crate::pl) ----

fn n(i: i64) -> Value {
    Value::Int(i)
}

/// A package hash (`%Image::ExifTool::fileTypeLookup`) as a reference.
fn ghash(it: &mut Interp, name: &str) -> Value {
    match it.global(b'%', name) {
        Binding::Hash(h) => Value::HashRef(h),
        _ => unreachable!(),
    }
}

/// A package array as a reference.
#[allow(dead_code)] // (used by the subs still to port)
fn garray(it: &mut Interp, name: &str) -> Value {
    match it.global(b'@', name) {
        Binding::Array(a) => Value::ArrayRef(a),
        _ => unreachable!(),
    }
}

/// A file lexical hash of ExifTool.pm (`my %moduleName`).
fn lhash(it: &mut Interp, name: &str) -> Value {
    crate::runtime::lex_hash(it, "Image::ExifTool", name)
}

/// A file lexical array of ExifTool.pm (`my @availableOptions`).
fn larray(it: &mut Interp, name: &str) -> Value {
    crate::runtime::lex_array(it, "Image::ExifTool", name)
}

/// `local $_;` around the body of a sub: a new `$_`, the caller's back on
/// every exit path.
fn local_topic<T>(it: &mut Interp, f: impl FnOnce(&mut Interp) -> ER<T>) -> ER<T> {
    let old = it.global(b'$', "main::_");
    it.set_global(b'$', "main::_", Binding::Scalar(Rc::new(RefCell::new(Value::Undef))));
    let r = f(it);
    it.set_global(b'$', "main::_", old);
    r
}

/// `local $SIG{'__WARN__'} = VALUE;` around a block.
#[allow(dead_code)] // (used by the subs still to port)
fn local_sig_warn<T>(it: &mut Interp, handler: Value, f: impl FnOnce(&mut Interp) -> ER<T>) -> ER<T> {
    let sig = ghash(it, "main::SIG");
    let had = sig.exists("__WARN__");
    let old = sig.get("__WARN__");
    sig.set("__WARN__", handler);
    let r = f(it);
    if had {
        sig.set("__WARN__", old);
    } else {
        sig.delete("__WARN__");
    }
    r
}

/// `\&SetWarning`
#[allow(dead_code)] // (used by the subs still to port)
fn set_warning_ref(it: &mut Interp) -> Value {
    Value::Code(it.sub_ref("Image::ExifTool::SetWarning"))
}

/// `eval { BLOCK }`: `Some(result)` with `$@` cleared, or `None` with `$@`
/// set when the block died. Other exits (`return`, `last`...) go through.
fn eval_block<T>(it: &mut Interp, f: impl FnOnce(&mut Interp) -> ER<T>) -> ER<Option<T>> {
    match f(it) {
        Ok(v) => {
            it.set_scalar("main::@", Value::str(""));
            Ok(Some(v))
        }
        Err(Flow::Die(e)) => {
            it.set_scalar("main::@", e);
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

/// `ref $v`, with the class of a blessed reference as perl gives it.
fn ref_of(v: &Value) -> String {
    if let Value::HashRef(h) = v {
        if let Some(c) = &h.borrow().blessed {
            return c.to_string();
        }
    }
    v.ref_type().to_string()
}

/// `lc $v`
fn lc(v: &Value) -> Value {
    if v.is_undef() {
        return pl::s("");
    }
    Value::bytes(v.to_bytes().to_ascii_lowercase())
}

/// `uc $v`
fn uc(v: &Value) -> Value {
    if v.is_undef() {
        return pl::s("");
    }
    Value::bytes(v.to_bytes().to_ascii_uppercase())
}

/// `$a cmp $b`
fn cmp(a: &Value, b: &Value) -> std::cmp::Ordering {
    a.to_bytes()[..].cmp(&b.to_bytes()[..])
}

/// `$a <=> $b` (undef, as for NaN, counts as 0 in a sort block).
fn ncmp(a: &Value, b: &Value) -> std::cmp::Ordering {
    crate::perl::ops::num_cmp(a, b).unwrap_or(std::cmp::Ordering::Equal)
}

/// `sort LIST`
fn sort_str(mut v: Vec<Value>) -> Vec<Value> {
    v.sort_by(cmp);
    v
}

/// `keys %$h` as values.
fn keys(h: &Value) -> Vec<Value> {
    h.keys().into_iter().map(Value::Str).collect()
}

/// `return LIST` of an array or `grep`: the list, or its count in scalar
/// context.
fn ret_count(v: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    if want == Want::List {
        Ok(v)
    } else {
        ret(n(v.len() as i64))
    }
}

/// `return sort ...`: the list, or undef in scalar context (as perl).
fn ret_sorted(v: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    if want == Want::List {
        Ok(v)
    } else {
        pl::ret_undef()
    }
}

/// `$1` of the last successful match in the calling scope (a failed match
/// leaves it unchanged).
fn last_cap(it: &Interp, i: usize) -> Value {
    match &it.last_match {
        Some(m) => match m.spans.get(i).copied().flatten() {
            Some((s, e)) => Value::bytes(&m.subject[s..e]),
            None => Value::Undef,
        },
        None => Value::Undef,
    }
}

/// `$obj->Method(ARGS)` in void context.
fn method_void(it: &mut Interp, obj: &Value, name: &str, args: Vec<Value>) -> ER<()> {
    let mut a = vec![obj.clone()];
    a.extend(args);
    it.call_method(obj, name, a, Want::Void)?;
    Ok(())
}

/// `Func(ARGS)` in void context.
fn call_void(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<()> {
    it.call_named(name, args, Want::Void)?;
    Ok(())
}

/// `$^O`
fn os(it: &mut Interp) -> Value {
    it.get_scalar("main::^O")
}

/// `$evalWarning`
const EVAL_WARNING: &str = "Image::ExifTool::evalWarning";

/// Run a snippet of Perl (a primitive no helper provides: a call whose
/// arguments the callee writes back as `$_[i] = ...`, `stat`, `-f`, `\*FH`)
/// with the scalars `vars` bound as lexicals; returns its result and the
/// variables' values afterwards. `$@` is left as it was; a die goes through.
fn perl_snippet(it: &mut Interp, code: &str, vars: &[(&str, Value)], want: Want) -> ER<(Vec<Value>, Vec<Value>)> {
    let scope = crate::interp::Scope::new(None);
    let cells: Vec<_> = vars.iter().map(|(name, v)| scope.scalar(name, v.clone())).collect();
    let saved_err = it.get_scalar("main::@");
    let r = it.eval_string(code.as_bytes(), "Image::ExifTool", &scope, want);
    it.set_scalar("main::@", saved_err);
    let out: Vec<Value> = cells.iter().map(|c| c.borrow().clone()).collect();
    match r {
        Ok(v) => Ok((v, out)),
        Err(e) => match it.exit_code {
            Some(code) => Err(Flow::Exit(code)),
            None => Err(Flow::Die(e)),
        },
    }
}

/// `Func(ARGS)` where Func may assign its arguments (`$_[0] = ...`): `args`
/// get the values the callee left in them.
fn call_lv(it: &mut Interp, name: &str, args: &mut [Value], want: Want) -> ER<Vec<Value>> {
    let names: Vec<String> = (0..args.len()).map(|i| format!("a{i}")).collect();
    let code = format!("{name}({})", names.iter().map(|n| format!("${n}")).collect::<Vec<_>>().join(","));
    let vars: Vec<(&str, Value)> = names.iter().map(|n| n.as_str()).zip(args.iter().cloned()).collect();
    let (r, out) = perl_snippet(it, &code, &vars, want)?;
    for (a, v) in args.iter_mut().zip(out) {
        *a = v;
    }
    Ok(r)
}

/// `$obj->Method(ARGS)` where the method may assign its arguments.
#[allow(dead_code)] // (used by the subs still to port)
fn method_lv(it: &mut Interp, obj: &Value, name: &str, args: &mut [Value], want: Want) -> ER<Vec<Value>> {
    let names: Vec<String> = (0..args.len()).map(|i| format!("a{i}")).collect();
    let code = format!("$obj->{name}({})", names.iter().map(|n| format!("${n}")).collect::<Vec<_>>().join(","));
    let mut vars: Vec<(&str, Value)> = vec![("obj", obj.clone())];
    vars.extend(names.iter().map(|n| n.as_str()).zip(args.iter().cloned()));
    let (r, out) = perl_snippet(it, &code, &vars, want)?;
    for (a, v) in args.iter_mut().zip(out.into_iter().skip(1)) {
        *a = v;
    }
    Ok(r)
}

/// `DoEscape($val, $proc)`: escapes `$val` in place.
fn do_escape(it: &mut Interp, val: &mut Value, proc_: &Value) -> ER<()> {
    let mut args = [val.clone(), proc_.clone()];
    call_lv(it, "Image::ExifTool::DoEscape", &mut args, Want::Void)?;
    *val = args[0].clone();
    Ok(())
}

/// `\*NAME` (a glob reference, in package Image::ExifTool unless qualified).
fn glob_ref(it: &mut Interp, name: &str) -> ER<Value> {
    Ok(perl_snippet(it, &format!("\\*{name}"), &[], Want::Scalar)?.0.pop().unwrap_or_default())
}

/// `$$h{KEY}` as the base of a longer element access (`$$h{KEY}{...}`):
/// perl creates the inner hash when it is undefined, even to read it.
fn viv(h: &Value, key: &str) -> Value {
    if h.is_hash() {
        h.get_or_new_hash(key)
    } else {
        Value::Undef
    }
}

/// `@$ref = LIST`: replace the contents of an existing array in place.
fn array_assign(r: &Value, items: Vec<Value>) {
    if let Value::ArrayRef(a) = r {
        *a.borrow_mut() = items;
    }
}

/// `$a[$i] = $v` on a Vec standing for a Perl array (grows with undefs).
fn set_at(v: &mut Vec<Value>, i: usize, x: Value) {
    if i >= v.len() {
        v.resize(i + 1, Value::Undef);
    }
    v[i] = x;
}

/// `$a[$i]` on a Vec standing for a Perl array.
fn at(v: &[Value], i: usize) -> Value {
    v.get(i).cloned().unwrap_or_default()
}

// ---- the subs ----

/// sub DummyWriteProc { return 1; }
pub fn dummy_write_proc(_it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(n(1))
}

/// Set warning message
/// Inputs: 0) warning string (undef to reset warning)
pub fn set_warning(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let v = arg(&a, 0);
    it.set_scalar(EVAL_WARNING, v.clone());
    ret(v)
}

/// Get warning message
pub fn get_warning(it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(it.get_scalar(EVAL_WARNING))
}

/// Clean unnecessary information (line number, LF) from warning
/// Inputs: 0) warning string or undef to use $evalWarning
/// Returns: cleaned warning
pub fn clean_warning(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut str = arg(&a, 0);
    if str.is_undef() {
        let ew = it.get_scalar(EVAL_WARNING);
        if ew.is_undef() {
            return pl::ret_undef();
        }
        str = ew;
    }
    // truncate at first " at " for warnings like "syntax error at (eval 80) line 1, at EOF"
    if let Some(c) = re(r"(.*?) at ", "s").m(&str) {
        str = cap(&c, 1);
    }
    re(r"\s+$", "s").subst(&mut str, false, |_| Vec::new());
    ret(str)
}

/// New - create new ExifTool object
/// Inputs: 0) reference to exiftool object or ExifTool class name
/// Returns: blessed ExifTool object ref
pub fn new(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    local_topic(it, |it| {
        let that = arg(&a, 0);
        let r = ref_of(&that);
        let class = if !r.is_empty() {
            r
        } else if that.truthy() {
            that.to_text()
        } else {
            "Image::ExifTool".to_string()
        };
        let self_ = Value::new_hash();
        if let Value::HashRef(h) = &self_ {
            h.borrow_mut().blessed = Some(Rc::from(class.as_str()));
        }

        // make sure our main Exif tag table has been loaded
        call_void(it, "Image::ExifTool::GetTagTable", vec![pl::s("Image::ExifTool::Exif::Main")])?;

        method_void(it, &self_, "ClearOptions", vec![])?; // create default options hash
        self_.set("VALUE", Value::new_hash()); // must initialize this for warning messages
        self_.set("PATH", Value::new_array(vec![])); // (this too)
        self_.set("DEL_GROUP", Value::new_hash()); // lookup for groups to delete when writing
        self_.set("SAVE_COUNT", n(0)); // count calls to SaveNewValues()
        self_.set("NV_COUNT", n(0)); // count of NEW_VALUE entries
        self_.set("FILE_SEQUENCE", n(0)); // sequence number for files when reading
        self_.set("FILES_WRITTEN", n(0)); // count of files successfully written
        self_.set("INDENT2", pl::s("")); // indentation of verbose messages from SetNewValue
        self_.set("ALT_EXIFTOOL", Value::new_hash()); // alternate exiftool objects

        // initialize our new groups for writing
        let groups = larray(it, "defaultWriteGroups").items();
        method_void(it, &self_, "SetNewGroups", groups)?;

        ret(self_)
    })
}

/// ImageInfo - return specified information from image file
/// Inputs: 0) [optional] ExifTool object reference
///         1) filename, file reference, RAF reference, or scalar data reference
///         2-N) list of tag names to find (or tag list reference or options reference)
/// Returns: reference to hash of tag/value pairs (with "Error" entry on error)
pub fn image_info(it: &mut Interp, mut a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    local_topic(it, |it| {
        // get our ExifTool object ($self) or create one if necessary
        let self_;
        let first = arg(&a, 0);
        if first.is_ref() && pl::call(it, "UNIVERSAL::isa", vec![first.clone(), pl::s("Image::ExifTool")])?.truthy() {
            self_ = a.remove(0);
        } else {
            self_ = pl::method(it, &pl::s("Image::ExifTool"), "new", vec![])?;
        }
        let save_options = self_.get("OPTIONS").hash_copy(); // save original options

        // initialize file information
        self_.set("RAF", Value::Undef);
        self_.set("FILENAME", Value::Undef);

        method_void(it, &self_, "ParseArguments", a)?; // parse our function arguments
        method_void(it, &self_, "ExtractInfo", vec![Value::Undef])?; // extract meta information from image
        let info = pl::method(it, &self_, "GetInfo", vec![Value::Undef])?; // get requested information

        self_.set("OPTIONS", save_options); // restore original options

        ret(info) // return requested information
    })
}

/// ClearOptions - set options to default values
/// Inputs: 0) ExifTool object reference
pub fn clear_options(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    local_topic(it, |it| {
        let self_ = arg(&a, 0);
        let opts = Value::new_hash();
        self_.set("OPTIONS", opts.clone()); // clear all options

        // load default options
        for o in larray(it, "availableOptions").items() {
            opts.set(o.at(0).to_bytes(), o.at(1));
        }

        // enable WindowsLongPath if Win32::API is available
        if pl::str_eq(&os(it), "MSWin32") && eval_block(it, |it| crate::runtime::require(it, "Win32::API").map(|_| n(1)))?.is_some() {
            opts.set("WindowsLongPath", n(1));
        }

        // keep necessary member variables in sync with options
        self_.delete("CUR_LANG");
        self_.delete("ESCAPE_PROC");

        // load user-defined default options
        let ud = ghash(it, "Image::ExifTool::UserDefined::Options");
        if !ud.is_empty() {
            for k in keys(&ud) {
                let v = ud.get(k.to_bytes());
                pl::method(it, &self_, "Options", vec![k, v])?;
            }
            // (the value of the last statement: the foreach)
            return pl::ret_nothing(want);
        }
        ret(pl::s(""))
    })
}

/// Get list of requested tags
/// Inputs: 0) ExifTool object reference
/// Returns: List of requested tag keys
pub fn get_requested_tags(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    local_topic(it, |_it| ret_count(arg(&a, 0).get("REQUESTED_TAGS").items(), want))
}

/// Get tag identification number
/// Inputs: 0) ExifTool object reference, 1) tag key
/// Returns: Scalar context: tag ID if available, otherwise ''
///          List context: 0) tag ID (or ''), 1) language code (or undef)
pub fn get_tag_id(_it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (self_, tag) = (arg(&a, 0), arg(&a, 1));
    let tag_info = viv(&self_, "TAG_INFO").get(tag.to_bytes());
    if !(tag_info.truthy() && !tag_info.get("TagID").is_undef()) {
        return ret(pl::s(""));
    }
    let mut id = tag_info.get("KeysID");
    if !id.truthy() {
        id = tag_info.get("TagID");
    }
    if want == Want::List {
        return Ok(vec![id, tag_info.get("LangCode")]);
    }
    ret(id)
}

/// Get group names for specified tags
/// Inputs: 0) ExifTool object reference
///         1) [optional] information hash reference (default all extracted info)
///         2) [optional] group family (default 0)
/// Returns: List of group names in alphabetical order
pub fn get_groups(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    local_topic(it, |it| {
        let self_ = arg(&a, 0);
        let mut info = arg(&a, 1);
        let mut family;

        // figure out our arguments
        if ref_of(&info) != "HASH" {
            family = info;
            info = self_.get("VALUE");
        } else {
            family = arg(&a, 2);
        }
        if family.is_undef() {
            family = n(0);
        }

        // get a list of all groups in specified information
        let groups = Value::new_hash();
        for tag in keys(&info) {
            let g = pl::method(it, &self_, "GetGroup", vec![tag, family.clone()])?;
            groups.set(g.to_bytes(), n(1));
        }
        ret_sorted(sort_str(keys(&groups)), want)
    })
}

/// Set priority for group where new values are written
/// Inputs: 0) ExifTool object reference,
///         1-N) group names (reset to default if no groups specified)
/// - used when new tag values are set (ie. before files are written)
pub fn set_new_groups(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    local_topic(it, |it| {
        let self_ = arg(&a, 0);
        let mut groups: Vec<Value> = a.iter().skip(1).cloned().collect();
        if groups.is_empty() {
            groups = larray(it, "defaultWriteGroups").items();
        }
        let mut count = n(groups.len() as i64 * 10);
        let priority = Value::new_hash();
        for g in &groups {
            priority.set(lc(g).to_bytes(), count.clone());
            count = pl::sub(&count, &n(10));
        }
        priority.set("file", n(500)); // 'File' group is always written (Comment)
        priority.set("composite", n(500)); // 'Composite' group is always written
        // set write priority (higher # is higher priority)
        self_.set("WRITE_PRIORITY", priority);
        let g = Value::new_array(groups);
        self_.set("WRITE_GROUPS", g.clone());
        ret(g)
    })
}

/// Get reference to Composite tag info hash
/// Inputs: 0) case-sensitive Composite tag name
/// Returns: tagInfo hash or undef
pub fn get_composite_tag_info(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let tag = arg(&a, 0);
    let cid = lhash(it, "compositeID");
    let ids = cid.get(tag.to_bytes());
    if !ids.truthy() {
        return pl::ret_undef();
    }
    let comp = ghash(it, "Image::ExifTool::Composite");
    ret(comp.get(ids.at(0).to_bytes()))
}

/// Return List ExifTool API options
/// Returns: 0) reference to list of available options -- each entry is a list
///            [0=option name, 1=default value, 2=description]
pub fn available_options(it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(larray(it, "availableOptions"))
}

/// Get tag name (removes copy index)
/// Inputs: 0) Tag key
/// Returns: Tag name
pub fn get_tag_name(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    local_topic(it, |it| match re(r"^(\S+)", "").m(&arg(&a, 0)) {
        Some(c) => ret(cap(&c, 1)),
        None => ret(last_cap(it, 1)),
    })
}

/// Get list of shortcuts
/// Returns: Shortcut list (sorted alphabetically)
pub fn get_shortcuts(it: &mut Interp, _a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    local_topic(it, |it| {
        crate::runtime::require(it, "Image::ExifTool::Shortcuts")?;
        let h = ghash(it, "Image::ExifTool::Shortcuts::Main");
        ret_sorted(sort_str(keys(&h)), want)
    })
}

/// Get file type for specified extension
/// Inputs: 0) file name or extension (case is not significant),
///            or FileType value if a description is requested
///         1) flag to return long description instead of type ('0' to return any recognized type)
/// Returns: File type (or desc) or undef if extension not supported or if
///          description is the same as the input FileType.  In list context,
///          may return more than one file type if the file may be different formats.
///          Returns list of all supported extensions if no file specified
pub fn get_file_type(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    local_topic(it, |it| {
        let (mut file, mut desc) = (arg(&a, 0), arg(&a, 1));
        let file_type_lookup = ghash(it, "Image::ExifTool::fileTypeLookup");
        let module_name = lhash(it, "moduleName");
        if file.is_undef() {
            let mut types = Vec::new();
            if !desc.is_undef() && pl::str_eq(&desc, "0") {
                // return all recognized types
                types = sort_str(keys(&file_type_lookup));
            } else {
                // return all supported types
                for t in sort_str(keys(&file_type_lookup)) {
                    let mut module = module_name.get(t.to_bytes());
                    if module.is_undef() {
                        module = module_name.get(file_type_lookup.get(t.to_bytes()).to_bytes());
                    }
                    if !(!module.is_undef() && pl::str_eq(&module, "0")) {
                        types.push(t);
                    }
                }
            }
            return ret_count(types, want);
        }
        let mut file_type;
        let mut sub_type = Value::Undef;
        let mut file_ext = pl::call(it, "Image::ExifTool::GetFileExtension", vec![file.clone()])?;
        if !file_ext.truthy() {
            let mut st = Value::Undef;
            if re(r" \((.*)\)$", "").subst(&mut file, false, |c| {
                st = cap(c, 1);
                Vec::new()
            }) > 0
            {
                sub_type = st;
                file_ext = pl::call(it, "Image::ExifTool::GetFileExtension", vec![file.clone()])?;
            }
            if !file_ext.truthy() {
                file_ext = uc(&file);
            }
        }
        file_type = Value::Undef;
        if file_ext.truthy() {
            file_type = file_type_lookup.get(file_ext.to_bytes()); // look up the file type
        }
        while file_type.truthy() && !file_type.is_ref() {
            file_type = file_type_lookup.get(file_type.to_bytes());
        }
        // return description if specified
        // (allow input $file to be a FileType for this purpose)
        if desc.truthy() {
            if file_type.truthy() {
                let sv = ghash(it, "Image::ExifTool::static_vars");
                let ofd = sv.get("OverrideFileDescription");
                if ofd.truthy() && ofd.get(file_ext.to_bytes()).truthy() {
                    desc = ofd.get(file_ext.to_bytes());
                } else {
                    desc = file_type.at(1);
                }
            } else {
                let fd = lhash(it, "fileDescription").get(file.to_bytes());
                desc = if fd.truthy() { fd } else { file.clone() };
            }
            if sub_type.truthy() {
                desc = pl::concat(&desc, &pl::concat(&pl::s(", "), &sub_type));
            }
            return ret(desc);
        } else if file_type.truthy() && (desc.is_undef() || !pl::str_eq(&desc, "0")) {
            // return only supported file types
            let m = module_name.get(file_type.at(0).to_bytes());
            if !m.is_undef() && pl::str_eq(&m, "0") {
                file_type = Value::Undef;
            }
        }
        if !file_type.truthy() {
            return pl::ret_nothing(want);
        }
        file_type = file_type.at(0); // get file type (or list of types)
        if want == Want::List {
            if file_type.is_array() {
                return Ok(file_type.items());
            }
        } else if file_type.truthy() && file_type.is_array() {
            file_type = file_ext;
        }
        ret(file_type)
    })
}

/// Return true if we can write the specified file type
/// Inputs: 0) file name or ext
/// Returns: true if writable, 0 if not writable, '' if not writable due to extension,
///          undef if unrecognized
pub fn can_write(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    local_topic(it, |it| {
        let file = arg(&a, 0);
        if !file.truthy() {
            return pl::ret_undef();
        }
        let types = get_file_type(it, vec![file.clone()], Want::List)?;
        if types.is_empty() {
            return pl::ret_undef();
        }
        let type_ = types[0].clone();
        let no_write_file = ghash(it, "Image::ExifTool::noWriteFile");
        let ext_of = |it: &mut Interp| -> ER<Value> {
            let e = pl::call(it, "Image::ExifTool::GetFileExtension", vec![file.clone()])?;
            Ok(if e.truthy() { e } else { uc(&file) })
        };
        if no_write_file.get(type_.to_bytes()).truthy() {
            // can't write TIFF files with certain extensions (various RAW formats)
            let ext = ext_of(it)?;
            if ext.truthy() {
                let p = [b"^", &ext.to_bytes()[..], b"$"].concat();
                let r = pl::re_dyn(&p, "")?;
                let found = no_write_file.get(type_.to_bytes()).items().iter().any(|x| r.is_match(x));
                return ret(if found { pl::s("") } else { n(1) });
            }
        }
        let only_write_file = lhash(it, "onlyWriteFile");
        if only_write_file.get(type_.to_bytes()).truthy() {
            let ext = ext_of(it)?;
            if ext.truthy() {
                let p = [b"^", &ext.to_bytes()[..], b"$"].concat();
                let r = pl::re_dyn(&p, "")?;
                let found = only_write_file.get(type_.to_bytes()).items().iter().any(|x| r.is_match(x));
                return ret(if found { n(1) } else { n(0) });
            }
        }
        let write_types = lhash(it, "writeTypes");
        if write_types.is_empty() {
            for t in larray(it, "writeTypes").items() {
                write_types.set(t.to_bytes(), n(1));
            }
        }
        ret(write_types.get(type_.to_bytes()))
    })
}

/// Return true if we can create the specified file type
/// Inputs: 0) file name or ext
/// Returns: true if creatable, 0 if not writable, undef if unrecognized
pub fn can_create(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    local_topic(it, |it| {
        let file = arg(&a, 0);
        if !file.truthy() {
            return pl::ret_undef();
        }
        let mut ext = pl::call(it, "Image::ExifTool::GetFileExtension", vec![file.clone()])?;
        if !ext.truthy() {
            ext = uc(&file);
        }
        let type_ = get_file_type(it, vec![file.clone()], Want::Scalar)?.pop().unwrap_or_default();
        if !type_.truthy() {
            return pl::ret_undef();
        }
        let create_types = lhash(it, "createTypes");
        if create_types.get(ext.to_bytes()).truthy() || create_types.get(type_.to_bytes()).truthy() {
            return ret(n(1));
        }
        ret(n(0))
    })
}

/// Return list of ordered keys if available, otherwise just sort alphabetically
/// Inputs: 0) hash ref
/// Returns: List of ordered/sorted keys
pub fn ordered_keys(_it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let hash = arg(&a, 0);
    let ok = hash.get("_ordered_keys_");
    if ok.truthy() {
        ret_count(ok.items(), want)
    } else {
        ret_sorted(sort_str(keys(&hash)), want)
    }
}

/// Get hash of extracted meta information
/// Inputs: 0) ExifTool object reference
///         1-N) options hash reference, tag list reference or tag names
/// Returns: Reference to information hash
/// Notes: - pass an undefined value to avoid parsing arguments
///        - If groups are specified, first groups take precedence if duplicate
///          tags found but Duplicates option not set.
///        - tag names may end in '#' to extract ValueConv value
pub fn get_info(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    local_topic(it, |it| {
        let self_ = arg(&a, 0);
        let args: Vec<Value> = a[1.min(a.len())..].to_vec();
        let mut save_options: Option<Value> = None;
        let mut save_members: Vec<&str> = Vec::new();
        let mut saved_members: Vec<Value> = Vec::new();
        // save necessary members to allow GetInfo to be called from within ExtractInfo
        if self_.get("InExtract").truthy() {
            save_members = vec!["REQUESTED_TAGS", "REQ_TAG_LOOKUP", "IO_TAG_LIST"];
            saved_members = save_members.iter().map(|m| self_.get(m)).collect();
        }
        if !(!args.is_empty() && args[0].is_undef()) {
            save_options = Some(self_.get("OPTIONS").hash_copy()); // save original options
            // must set FILENAME so it isn't parsed from the arguments
            if self_.get("FILENAME").is_undef() {
                self_.set("FILENAME", pl::s(""));
            }
            method_void(it, &self_, "ParseArguments", args)?;
        }

        // get reference to list of tags for which we will return info
        let r = pl::method_list(it, &self_, "SetFoundTags", vec![])?;
        let (mut rtn_tags, by_value, wild_tags) = (arg(&r, 0), arg(&r, 1), arg(&r, 2));

        // build hash of tag information
        let info = Value::new_hash();
        let ignored = Value::new_hash();
        let conv = if self_.get("OPTIONS").get("PrintConv").truthy() { pl::s("PrintConv") } else { pl::s("ValueConv") };
        for t in rtn_tags.items() {
            let val = pl::method(it, &self_, "GetValue", vec![t.clone(), conv.clone()])?;
            if val.is_undef() {
                ignored.set(t.to_bytes(), n(1));
                continue;
            }
            info.set(t.to_bytes(), val);
        }

        // override specified tags with ValueConv value if necessary
        if !by_value.is_empty() {
            // first determine the number of times each non-ValueConv value is used
            let non_val = Value::new_hash();
            for t in rtn_tags.items() {
                let c = non_val.get(t.to_bytes());
                non_val.set(t.to_bytes(), pl::add(&if c.truthy() { c } else { n(0) }, &n(1)));
            }
            for i in by_value.items() {
                let k = rtn_tags.at(i.to_i64()).to_bytes();
                let c = non_val.get(&k);
                non_val.set(&k, pl::sub(&c, &n(1)));
            }
            // loop through ValueConv tags, updating tag keys and returned values
            for i in by_value.items() {
                let tag = rtn_tags.at(i.to_i64());
                let val = pl::method(it, &self_, "GetValue", vec![tag.clone(), pl::s("ValueConv")])?;
                if val.is_undef() {
                    continue;
                }
                let mut vtag = tag.clone();
                // generate a new tag key like "Tag #" or "Tag #(1)"
                re(r"( |$)", "").subst(&mut vtag, false, |_| b" #".to_vec());
                let value = self_.get("VALUE");
                if value.get(vtag.to_bytes()).is_undef() {
                    let (vk, tk) = (vtag.to_bytes(), tag.to_bytes());
                    value.set(&vk, value.get(&tk));
                    for m in ["TAG_INFO", "TAG_EXTRA", "FILE_ORDER"] {
                        let h = self_.get_or_new_hash(m);
                        h.set(&vk, h.get(&tk));
                    }
                    // remove existing PrintConv entry unless we are using it too
                    if !non_val.get(&tk).truthy() {
                        info.delete(&tk);
                    }
                }
                if let Value::ArrayRef(arr) = &rtn_tags {
                    let idx = i.to_i64();
                    let len = arr.borrow().len() as i64;
                    let idx = if idx < 0 { idx + len } else { idx };
                    if idx >= 0 {
                        rtn_tags.set_at(idx as usize, vtag.clone()); // store ValueConv value with new tag key
                    }
                }
                info.set(vtag.to_bytes(), val); // return ValueConv value
            }
        }

        // remove ignored tags from the list
        let mut req_tags = self_.get("REQUESTED_TAGS");
        if !req_tags.truthy() {
            req_tags = Value::new_array(vec![]);
        }
        if !ignored.is_empty() {
            if req_tags.is_empty() {
                let mut good_tags = Vec::new();
                for t in rtn_tags.items() {
                    if !ignored.get(t.to_bytes()).truthy() {
                        good_tags.push(t);
                    }
                }
                rtn_tags = Value::new_array(good_tags);
                self_.set("FOUND_TAGS", rtn_tags.clone());
            } else if !wild_tags.is_empty() {
                // only remove tags specified by wildcard
                let mut good_tags = Vec::new();
                let mut i = n(0);
                for t in rtn_tags.items() {
                    if !wild_tags.is_empty() && pl::num_eq(&i, &wild_tags.at(0)) {
                        if let Value::ArrayRef(w) = &wild_tags {
                            w.borrow_mut().remove(0);
                        }
                        if !ignored.get(t.to_bytes()).truthy() {
                            good_tags.push(t);
                        }
                    } else {
                        good_tags.push(t);
                    }
                    i = pl::add(&i, &n(1));
                }
                rtn_tags = Value::new_array(good_tags);
                self_.set("FOUND_TAGS", rtn_tags.clone());
            }
        }

        // return sorted tag list if provided with a list reference
        let io = self_.get("IO_TAG_LIST");
        if io.truthy() {
            // use file order by default if no tags specified
            // (no such thing as 'Input' order in this case)
            let mut sort = self_.get("OPTIONS").get("Sort");
            if !(!req_tags.is_empty() || (sort.truthy() && !pl::str_eq(&sort, "Input"))) {
                sort = pl::s("File");
            }
            // return tags in specified sort order
            let sort2 = self_.get("OPTIONS").get("Sort2");
            let list = get_tag_list(it, vec![self_.clone(), rtn_tags.clone(), sort, sort2], Want::List)?;
            array_assign(&self_.get("IO_TAG_LIST"), list);
        }

        // restore original options and member variables
        if let Some(so) = save_options {
            if !so.is_empty() {
                self_.set("OPTIONS", so);
            }
        }
        if !save_members.is_empty() {
            for (m, v) in save_members.iter().zip(saved_members) {
                self_.set(m, v);
            }
        }

        ret(info)
    })
}

/// Inputs: 0) ExifTool object reference
///         1) [optional] reference to info hash or tag list ref (default is found tags)
///         2) [optional] sort order ('File', 'Input', ...)
///         3) [optional] secondary sort order
/// Returns: List of tags in specified order
pub fn get_tag_list(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    local_topic(it, |it| {
        let (self_, info, mut sort, mut sort2) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));

        let mut found_tags = Value::Undef;
        if ref_of(&info) == "HASH" {
            found_tags = Value::new_array(keys(&info));
        } else if ref_of(&info) == "ARRAY" {
            found_tags = info.clone();
        }
        let file_order = self_.get("FILE_ORDER");

        if found_tags.truthy() {
            // make sure a FILE_ORDER entry exists for all tags
            // (note: already generated bogus entries for FOUND_TAGS case below)
            for t in found_tags.items() {
                if !file_order.get(t.to_bytes()).is_undef() {
                    continue;
                }
                file_order.set(t.to_bytes(), n(999));
            }
        } else {
            if info.truthy() && !sort.truthy() {
                sort = info.clone();
            }
            found_tags = self_.get("FOUND_TAGS");
            if !found_tags.truthy() {
                found_tags = pl::method(it, &self_, "SetFoundTags", vec![])?;
            }
            if !found_tags.truthy() {
                return pl::ret_undef();
            }
        }
        if !sort.truthy() {
            sort = self_.get("OPTIONS").get("Sort");
        }

        // return original list if no sort order specified
        if !(sort.truthy() && !pl::str_eq(&sort, "Input")) {
            return ret_count(found_tags.items(), want);
        }

        let fo = |t: &Value| file_order.get(t.to_bytes());
        let mut tags = found_tags.items();
        if pl::str_eq(&sort, "Tag") || pl::str_eq(&sort, "Alpha") {
            return ret_sorted(sort_str(tags), want);
        } else if let Some(c) = re(r"^Group(\d*(:\d+)*)", "").m(&sort) {
            let mut family = cap(&c, 1);
            if !family.truthy() {
                family = n(0);
            }
            // want to maintain a basic file order with the groups
            // ordered in the way they appear in the file
            let group_count = Value::new_hash();
            let group_order = Value::new_hash();
            let mut num_groups = n(0);
            let mut by_file = tags.clone();
            by_file.sort_by(|x, y| ncmp(&fo(x), &fo(y)));
            for tag in by_file {
                let group = get_group(it, vec![self_.clone(), tag.clone(), family.clone()], Want::Scalar)?.pop().unwrap_or_default();
                let mut num = group_count.get(group.to_bytes());
                if !num.truthy() {
                    num_groups = pl::add(&num_groups, &n(1));
                    num = num_groups.clone();
                    group_count.set(group.to_bytes(), num.clone());
                }
                group_order.set(tag.to_bytes(), num);
            }
            let go = |t: &Value| group_order.get(t.to_bytes());
            if !sort2.truthy() {
                sort2 = self_.get("OPTIONS").get("Sort2");
            }
            if sort2.truthy() {
                if pl::str_eq(&sort2, "Tag") || pl::str_eq(&sort2, "Alpha") {
                    tags.sort_by(|x, y| ncmp(&go(x), &go(y)).then_with(|| cmp(x, y)));
                    return ret_sorted(tags, want);
                } else if pl::str_eq(&sort2, "Descr") {
                    let desc = pl::method(it, &self_, "GetDescriptions", vec![found_tags.clone()])?;
                    tags.sort_by(|x, y| ncmp(&go(x), &go(y)).then_with(|| cmp(&desc.get(x.to_bytes()), &desc.get(y.to_bytes()))));
                    return ret_sorted(tags, want);
                }
            }
            tags.sort_by(|x, y| ncmp(&go(x), &go(y)).then_with(|| ncmp(&fo(x), &fo(y))));
            ret_sorted(tags, want)
        } else if pl::str_eq(&sort, "Descr") {
            let desc = pl::method(it, &self_, "GetDescriptions", vec![found_tags.clone()])?;
            tags.sort_by(|x, y| cmp(&desc.get(x.to_bytes()), &desc.get(y.to_bytes())).then_with(|| cmp(x, y)));
            ret_sorted(tags, want)
        } else {
            tags.sort_by(|x, y| ncmp(&fo(x), &fo(y)));
            ret_sorted(tags, want)
        }
    })
}

/// Get list of found tags in specified sort order
/// Inputs: 0) ExifTool object reference, 1) sort order ('File', 'Input', ...)
///         2) secondary sort order
/// Returns: List of tag keys in specified order
/// Notes: If not specified, sort order is taken from OPTIONS
pub fn get_found_tags(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    local_topic(it, |it| {
        let (self_, sort, sort2) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
        let mut found_tags = self_.get("FOUND_TAGS");
        if !found_tags.truthy() {
            found_tags = pl::method(it, &self_, "SetFoundTags", vec![])?;
        }
        if !found_tags.truthy() {
            return pl::ret_undef();
        }
        get_tag_list(it, vec![self_, found_tags, sort, sort2], want)
    })
}

/// Get description for specified tag
/// Inputs: 0) ExifTool object reference, 1) tag key
/// Returns: Tag description
/// Notes: Will always return a defined value, even if description isn't available
pub fn get_description(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    local_topic(it, |it| {
        let (self_, tag) = (arg(&a, 0), arg(&a, 1));
        let mut desc = Value::Undef;
        let tag_info = viv(&self_, "TAG_INFO").get(tag.to_bytes());
        // ($tagInfo won't be defined for missing tags extracted with -f)
        if tag_info.truthy() {
            // use alternate language description if available
            while self_.get("CUR_LANG").truthy() {
                let cur_lang = self_.get("CUR_LANG");
                desc = cur_lang.get(tag_info.get("Name").to_bytes());
                if desc.truthy() {
                    // must look up Description if this tag also has a PrintConv
                    if desc.is_ref() {
                        desc = desc.get("Description");
                        if !desc.truthy() {
                            break;
                        }
                    }
                } else {
                    // look up default language of lang-alt tag
                    let lang_code = tag_info.get("LangCode");
                    if !lang_code.truthy() {
                        break;
                    }
                    let mut name = tag_info.get("Name");
                    let p = [b"-", &lang_code.to_bytes()[..], b"$"].concat();
                    if pl::re_dyn(&p, "")?.subst(&mut name, false, |_| Vec::new()) == 0 {
                        break;
                    }
                    desc = cur_lang.get(name.to_bytes());
                    if !desc.truthy() {
                        break;
                    }
                    if desc.is_ref() {
                        desc = desc.get("Description");
                        if !desc.truthy() {
                            break;
                        }
                    }
                    desc = pl::concat(&desc, &pl::concat(&pl::s(" ("), &pl::concat(&lang_code, &pl::s(")"))));
                }
                // escape description if necessary
                let ep = self_.get("ESCAPE_PROC");
                if ep.truthy() {
                    do_escape(it, &mut desc, &ep)?;
                }
                // return description in proper Charset
                return it.call_method(&self_, "Decode", vec![self_.clone(), desc, pl::s("UTF8")], want);
            }
            desc = tag_info.get("Description");
        }
        // just make the tag more readable if description doesn't exist
        if !desc.truthy() {
            let name = get_tag_name(it, vec![tag.clone()], Want::List)?;
            desc = pl::call(it, "Image::ExifTool::MakeDescription", name)?;
            // save description in tag information
            if tag_info.truthy() {
                tag_info.set("Description", desc.clone());
            }
        }
        ret(desc)
    })
}

/// Get group name for specified tag
/// Inputs: 0) ExifTool object reference
///         1) tag key (or reference to tagInfo hash, not part of the public API)
///         2) [optional] group family (-1 to get extended group list, or multiple
///            families separated by colons to return multiple groups as a string)
/// Returns: Scalar context: group name (for family 0 if not otherwise specified)
///          List context: group name if family specified, otherwise list of
///          group names for each family.  Returns '' for undefined tag.
/// Notes: Multiple families may be specified with ':' in family argument (eg. '1:2')
pub fn get_group(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    local_topic(it, |_it| {
        let (self_, mut tag, mut family) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
        let tag_info;
        let ex;
        let mut groups: Vec<Value> = Vec::new();
        let mut families: Vec<Value> = Vec::new();
        let (mut simplify, mut by_tag_info, mut no_id) = (false, false, false);
        if ref_of(&tag) == "HASH" {
            tag_info = tag.clone();
            tag = tag_info.get("Name");
            // set flag so we don't get extra information for an extracted tag
            by_tag_info = true;
            ex = Value::new_hash();
        } else {
            let ti = viv(&self_, "TAG_INFO").get(tag.to_bytes());
            tag_info = if ti.truthy() { ti } else { Value::new_hash() };
            let e = viv(&self_, "TAG_EXTRA").get(tag.to_bytes());
            ex = if e.truthy() { e } else { Value::new_hash() };
        }
        let mut groups_h = tag_info.get("Groups");
        // fill in default groups unless already done
        // (after this, Groups 0-2 in tagInfo are guaranteed to be defined)
        if !tag_info.get("GotGroups").truthy() {
            let mut tag_table_ptr = tag_info.get("Table");
            if !tag_table_ptr.truthy() {
                tag_table_ptr = Value::new_hash();
                tag_table_ptr.set("GROUPS", Value::new_hash());
            }
            // construct our group list
            if !groups_h.truthy() {
                groups_h = Value::new_hash();
                tag_info.set("Groups", groups_h.clone());
            }
            // fill in default groups
            for i in 0..3 {
                let k = i.to_string();
                if !groups_h.get(&k).truthy() {
                    let g = viv(&tag_table_ptr, "GROUPS").get(&k);
                    groups_h.set(&k, if g.truthy() { g } else { pl::s("") });
                }
            }
            // set flag indicating group list was built
            tag_info.set("GotGroups", n(1));
        }
        let or = |x: Value, y: Value| if x.truthy() { x } else { y };
        if !family.is_undef() && !pl::str_eq(&family, "-1") {
            if re(r"[^\d]", "").is_match(&family) {
                families = re(r"\d+", "").all(&family);
                if families.is_empty() {
                    return ret(or(ex.get("G0"), groups_h.get("0")));
                }
                if !re(r"^:", "").is_match(&family) {
                    simplify = true;
                }
                family = Value::Undef;
                for i in 0..3 {
                    set_at(&mut groups, i, groups_h.get(i.to_string()));
                }
                if families.len() == 1 && !pl::num_eq(&families[0], &n(7)) {
                    no_id = true;
                }
            } else {
                if pl::num_eq(&family, &n(0)) || pl::num_eq(&family, &n(2)) {
                    return ret(or(ex.get(pl::concat(&pl::s("G"), &family).to_bytes()), groups_h.get(family.to_bytes())));
                }
                set_at(&mut groups, 1, groups_h.get("1"));
            }
        } else {
            if want != Want::List {
                return ret(or(ex.get("G0"), groups_h.get("0")));
            }
            for i in 0..3 {
                set_at(&mut groups, i, groups_h.get(i.to_string()));
            }
        }
        set_at(&mut groups, 3, pl::s("Main"));
        let g4 = match re(r"\((\d+)\)$", "").m(&tag) {
            Some(c) if !pl::str_eq(&cap(&c, 1), "0") => pl::concat(&pl::s("Copy"), &cap(&c, 1)),
            _ => pl::s(""),
        };
        set_at(&mut groups, 4, g4);
        // handle dynamic group names if necessary
        if !by_tag_info {
            if ex.get("G0").truthy() {
                groups[0] = ex.get("G0");
            }
            let g1 = ex.get("G1");
            if g1.truthy() {
                groups[1] = match re(r"^\+(.*)", "").m(&g1) {
                    Some(c) => pl::concat(&groups[1], &cap(&c, 1)),
                    None => g1,
                };
            }
            if ex.get("G3").truthy() {
                groups[3] = pl::concat(&pl::s("Doc"), &ex.get("G3"));
            }
            if !ex.get("G5").is_undef() {
                let g = or(ex.get("G5"), at(&groups, 1));
                set_at(&mut groups, 5, g);
            }
            if !ex.get("G6").is_undef() {
                if at(&groups, 5).is_undef() {
                    set_at(&mut groups, 5, pl::s("")); // (can't leave a hole in the array)
                }
                set_at(&mut groups, 6, ex.get("G6"));
            }
            if ex.get("G8").truthy() {
                set_at(&mut groups, 7, pl::s(""));
                set_at(&mut groups, 8, ex.get("G8"));
            }
            // generate tag ID group names unless obviously not needed
            if !no_id {
                let mut id = or(tag_info.get("KeysID"), tag_info.get("TagID"));
                if id.is_undef() {
                    id = pl::s(""); // (just to be safe)
                } else if re(r"^\d+$", "").is_match(&id) {
                    if self_.get("OPTIONS").get("HexTagIDs").truthy() {
                        id = pl::sprintf("0x%x", &[id]);
                    }
                } else {
                    re(r"([^-_A-Za-z0-9])", "").subst(&mut id, true, |c| pl::sprintf("%.2x", &[n(cap(c, 1).to_bytes()[0] as i64)]).to_bytes().to_vec());
                }
                set_at(&mut groups, 7, pl::concat(&pl::s("ID-"), &id));
                for i in [5, 6] {
                    if at(&groups, i).is_undef() {
                        set_at(&mut groups, i, pl::s(""));
                    }
                }
            }
        }
        if family.truthy() {
            if pl::gt(&family, &n(0)) {
                let i = family.to_i64();
                let g = if i >= 0 { at(&groups, i as usize) } else { Value::Undef };
                return ret(or(g, pl::s("")));
            }
            // add additional matching group names to list
            // eg) for MIE-Doc, also add MIE1, MIE1-Doc, MIE-Doc1 and MIE1-Doc1
            // and for MIE2-Doc3, also add MIE2, MIE-Doc3, MIE2-Doc and MIE-Doc
            if let Some(c) = re(r"^MIE(\d*)-(.+?)(\d*)$", "").m(&at(&groups, 1)) {
                let (c1, c2, c3) = (cap(&c, 1), cap(&c, 2), cap(&c, 3));
                let s = |x: &str| pl::s(x);
                let cat = |v: &[Value]| v.iter().fold(pl::s(""), |acc, x| pl::concat(&acc, x));
                groups.push(cat(&[s("MIE"), if c1.truthy() { c1.clone() } else { s("1") }]));
                groups.push(cat(&[s("MIE"), if c1.truthy() { s("") } else { s("1") }, s("-"), c2.clone(), c3.clone()]));
                groups.push(cat(&[s("MIE"), c1.clone(), s("-"), c2.clone(), if c3.truthy() { s("") } else { s("1") }]));
                groups.push(cat(&[s("MIE"), if c1.truthy() { s("") } else { s("1") }, s("-"), c2.clone(), if c3.truthy() { s("") } else { s("1") }]));
            }
        }
        if !families.is_empty() {
            let mut grps: Vec<Value> = Vec::new();
            // create list of group names (without identical adjacent groups if simplifying)
            for f in &families {
                let i = f.to_i64();
                let mut grp = if i >= 0 { at(&groups, i as usize) } else { Value::Undef };
                if !grp.truthy() {
                    if simplify {
                        continue;
                    }
                    grp = pl::s("");
                }
                if !(simplify && !grps.is_empty() && grp.to_bytes() == grps[grps.len() - 1].to_bytes()) {
                    grps.push(grp);
                }
            }
            // remove leading "Main:" if simplifying
            if simplify && grps.len() > 1 && pl::str_eq(&grps[0], "Main") {
                grps.remove(0);
            }
            // return colon-separated string of group names
            let joined: Vec<Vec<u8>> = grps.iter().map(|g| g.to_bytes().to_vec()).collect();
            return ret(Value::bytes(joined.join(&b':')));
        }
        ret_count(groups, want)
    })
}

/// Initialize member variables before reading or writing a new file
/// Inputs: 0) ExifTool object reference
pub fn init(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    local_topic(it, |it| {
        let self_ = arg(&a, 0);
        // delete all DataMember variables (lower-case names)
        let lower = re(r"[a-z]", "");
        for k in keys(&self_) {
            if lower.is_match(&k) {
                self_.delete(k.to_bytes());
            }
        }
        // reset static variables
        let opts = self_.get("OPTIONS");
        if let Binding::Hash(sv) = it.global(b'%', "Image::ExifTool::static_vars") {
            let mut sv = sv.borrow_mut();
            sv.clear();
            sv.insert(b"KeepUTCTime", opts.get("KeepUTCTime"));
            sv.insert(b"SystemTimeRes", opts.get("SystemTimeRes"));
        }
        self_.delete("FOUND_TAGS"); // list of found tags
        self_.delete("EXIF_DATA"); // the EXIF data block
        self_.delete("EXIF_POS"); // EXIF position in file
        self_.delete("FIRST_EXIF_POS"); // position of first EXIF in file
        self_.delete("HTML_DUMP"); // html dump information
        self_.delete("SET_GROUP0"); // group0 name override
        self_.delete("SET_GROUP1"); // group1 name override
        self_.delete("DOC_NUM"); // current embedded document number
        self_.set("DOC_COUNT", n(0)); // count of embedded documents processed
        self_.set("BASE", n(0)); // base for offsets from start of file
        self_.set("FILE_ORDER", Value::new_hash()); // * hash of tag order in file ('*' = based on tag key)
        self_.set("VALUE", Value::new_hash()); // * hash of raw tag values
        self_.set("BOTH", Value::new_hash()); // * hash for Value/PrintConv values of Require'd tags
        self_.set("TAG_INFO", Value::new_hash()); // * hash of tag information
        self_.set("TAG_EXTRA", Value::new_hash()); // * hash of extra tag information (dynamic group names)
        self_.set("PRIORITY", Value::new_hash()); // * priority of current tags
        self_.set("LIST_TAGS", Value::new_hash()); // hash of tagInfo refs for active List-type tags
        self_.set("PROCESSED", Value::new_hash()); // hash of processed directory start positions
        self_.set("DIR_COUNT", Value::new_hash()); // count various types of directories
        self_.set("DUPL_TAG", Value::new_hash()); // last-used index for duplicate-tag keys
        self_.set("WAS_WARNED", Value::new_hash()); // number of times each warning was issued
        self_.set("WRITTEN", Value::new_hash()); // list of tags written (selected tags only)
        self_.set("FORCE_WRITE", Value::new_hash()); // ForceWrite lookup (set from ForceWrite tag)
        self_.set("FOUND_DIR", Value::new_hash()); // hash of directory names found in file
        self_.set("COMP_KEYS", Value::new_hash()); // lookup for tag keys used in Composite tags
        self_.set("PATH", Value::new_array(vec![])); // current subdirectory path in file when reading
        self_.set("NUM_FOUND", n(0)); // total number of tags found (incl. duplicates)
        self_.set("CHANGED", n(0)); // number of tags changed (writer only)
        self_.set("INDENT", pl::s("  ")); // initial indent for verbose messages
        self_.set("PRIORITY_DIR", pl::s("")); // the priority directory name
        let lpd = Value::new_hash();
        lpd.set("PreviewIFD", n(1));
        self_.set("LOW_PRIORITY_DIR", lpd); // names of priority 0 directories
        self_.set("TIFF_TYPE", pl::s("")); // type of TIFF data (APP1, TIFF, NEF, etc...)
        self_.set("FMT_EXPR", Value::Undef); // current advanced formatting expression
        self_.set("HAS_DOC", Value::new_hash()); // lookup for all document numbers in this file
        self_.set("Make", pl::s("")); // camera make
        self_.set("Model", pl::s("")); // camera model
        self_.set("CameraType", pl::s("")); // Olympus camera type
        self_.set("FileType", pl::s("")); // identified file type
        if pl::method(it, &self_, "Options", vec![pl::s("HtmlDump")])?.truthy() {
            crate::runtime::require(it, "Image::ExifTool::HtmlDump")?;
            let hd = pl::method(it, &pl::s("Image::ExifTool::HtmlDump"), "new", vec![])?;
            self_.set("HTML_DUMP", hd);
        }
        // make sure our TextOut is a file reference
        let opts = self_.get("OPTIONS");
        if !opts.get("TextOut").is_ref() {
            let g = glob_ref(it, "STDOUT")?;
            opts.set("TextOut", g.clone());
            return ret(g);
        }
        ret(pl::s(""))
    })
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::DummyWriteProc", dummy_write_proc);
    it.define_native("Image::ExifTool::SetWarning", set_warning);
    it.define_native("Image::ExifTool::GetWarning", get_warning);
    it.define_native("Image::ExifTool::CleanWarning", clean_warning);
    it.define_native("Image::ExifTool::new", new);
    it.define_native("Image::ExifTool::ImageInfo", image_info);
    it.define_native("Image::ExifTool::ClearOptions", clear_options);
    it.define_native("Image::ExifTool::GetRequestedTags", get_requested_tags);
    it.define_native("Image::ExifTool::GetTagID", get_tag_id);
    it.define_native("Image::ExifTool::GetGroups", get_groups);
    it.define_native("Image::ExifTool::SetNewGroups", set_new_groups);
    it.define_native("Image::ExifTool::GetCompositeTagInfo", get_composite_tag_info);
    it.define_native("Image::ExifTool::AvailableOptions", available_options);
    it.define_native("Image::ExifTool::GetTagName", get_tag_name);
    it.define_native("Image::ExifTool::GetShortcuts", get_shortcuts);
    it.define_native("Image::ExifTool::GetFileType", get_file_type);
    it.define_native("Image::ExifTool::CanWrite", can_write);
    it.define_native("Image::ExifTool::CanCreate", can_create);
    it.define_native("Image::ExifTool::OrderedKeys", ordered_keys);
    it.define_native("Image::ExifTool::GetInfo", get_info);
    it.define_native("Image::ExifTool::GetTagList", get_tag_list);
    it.define_native("Image::ExifTool::GetFoundTags", get_found_tags);
    it.define_native("Image::ExifTool::GetDescription", get_description);
    it.define_native("Image::ExifTool::GetGroup", get_group);
    it.define_native("Image::ExifTool::Init", init);
}
