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
    let old = it.swap_global(b'$', "main::_", Binding::Scalar(Rc::new(RefCell::new(Value::Undef))));
    let r = f(it);
    it.swap_global(b'$', "main::_", old);
    r
}

/// `local $SIG{'__WARN__'} = VALUE;` around a block.
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
        Some(m) => m.group(i),
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
fn viv(h: &Value, key: impl AsRef<[u8]>) -> Value {
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
        ret(n(0)) // (the value of the last statement: the false `%hash`)
    })
}

/// Get list of requested tags
/// Inputs: 0) ExifTool object reference
/// Returns: List of requested tag keys
pub fn get_requested_tags(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    local_topic(it, |_it| {
        pl::at(3427);
        ret_count(pl::aderef(&arg(&a, 0).get("REQUESTED_TAGS"))?, want)
    })
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
    // ($$hash{...} autovivifies an undefined $hash, but dies on anything
    // else that is not a HASH reference)
    if !hash.is_undef() {
        pl::at(4319);
        pl::hderef(&hash)?;
    }
    let ok = hash.get("_ordered_keys_");
    if ok.truthy() {
        pl::at(4319);
        ret_count(pl::aderef(&ok)?, want)
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

/// `grep /^$pat$/i, LIST` (the first match, for `my ($match) = grep ...`).
fn grep_first_ci(pat: &Value, list: &[Value]) -> ER<Value> {
    let pb = pat.to_bytes();
    // A tag name (word characters only, the usual case): its pattern is the
    // name itself, so a byte string matches when it is the name in another
    // case, before an optional final newline (`$`), as PCRE2 matches it.
    // Character strings, which caseless matching folds by Unicode's rules,
    // still go through the pattern.
    if pb.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_') && list.iter().all(|k| !matches!(k, Value::UStr(_))) {
        let found = list.iter().find(|k| {
            let b = k.to_bytes();
            let b = b.strip_suffix(b"\n").unwrap_or(&b);
            b.eq_ignore_ascii_case(&pb)
        });
        return Ok(found.cloned().unwrap_or_default());
    }
    let p = [b"^", &pb[..], b"$"].concat();
    let r = pl::re_dyn(&p, "i")?;
    Ok(list.iter().find(|k| r.is_match(k)).cloned().unwrap_or_default())
}

/// `$a[$i]` with perl's negative indices, on a Vec.
fn at_i(v: &[Value], i: i64) -> Value {
    let idx = if i < 0 { v.len() as i64 + i } else { i };
    if idx < 0 {
        Value::Undef
    } else {
        at(v, idx as usize)
    }
}

/// `&$code(ARGS)` on a code value (perl's errors otherwise).
fn call_code(it: &mut Interp, code: &Value, args: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    match code {
        Value::Code(c) => it.call(c, args, want),
        Value::Undef => crate::interp::eval::die("Can't use an undefined value as a subroutine reference"),
        // (its callers run under "strict refs": a sub name is not a code ref)
        v if !v.is_ref() => {
            let b = v.to_bytes();
            let mut m = String::from("Can't use string (\"");
            m.push_str(&String::from_utf8_lossy(&b[..b.len().min(32)]));
            m.push('"');
            if b.len() > 32 {
                m.push_str("...");
            }
            m.push_str(") as a subroutine ref while \"strict refs\" in use");
            crate::interp::eval::die(m)
        }
        _ => crate::interp::eval::die("Not a CODE reference"),
    }
}

/// `join SEP, LIST`
fn join(sep: &Value, list: &[Value]) -> Value {
    let s = sep.to_bytes();
    let parts: Vec<Vec<u8>> = list.iter().map(|v| v.to_bytes().to_vec()).collect();
    Value::bytes(parts.join(&s[..]))
}

/// Get tag value
/// Inputs: 0) ExifTool object reference
///         1) tag key or tag name with optional group names (case insensitive)
///            (or flattened tagInfo for getting field values, not part of public API)
///         2) [optional] Value type: PrintConv, ValueConv, Both, Raw, Bin or Rational, the
///            default is PrintConv or ValueConv, depending on the PrintConv option setting
///         3) raw field value (not part of public API)
/// Returns: Scalar context: tag value or undefined
///          List context: list of values or empty list
pub fn get_value(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    local_topic(it, |it| get_value_body(it, a, want))
}

fn get_value_body(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (self_, mut tag, mut type_) = (arg(&a, 0), arg(&a, 1), arg(&a, 2)); // plus: ($rawFieldValue)
    let mut conv_types: Vec<&str> = Vec::new();
    let tag_info;
    let mut value_conv = Value::Undef;
    let mut both = Value::Undef;
    let raw_value = self_.get("VALUE");

    // get specific tag key if tag has a group name
    if let Some(c) = re(r"^(.*):(.+)", "").m(&tag) {
        let (gp, mut tg) = (cap(&c, 1), cap(&c, 2));
        let mut keys_: Vec<Value> = Vec::new();
        if raw_value.get(tg.to_bytes()).is_undef() {
            pl::at(3452);
            let m = grep_first_ci(&tg, &keys(&raw_value))?;
            if m.truthy() {
                tg = m;
            }
        }
        // build list of tag keys in the order of priority (no index
        // is top priority, otherwise higher index is higher priority)
        let mut key = tg.clone();
        let d = viv(&self_, "DUPL_TAG").get(tg.to_bytes());
        let mut i = if d.truthy() { d } else { n(0) };
        loop {
            if !raw_value.get(key.to_bytes()).is_undef() {
                keys_.push(key.clone());
            }
            if pl::le(&i, &n(0)) {
                break;
            }
            key = pl::concat(&tg, &pl::concat(&pl::s(" ("), &pl::concat(&i, &pl::s(")"))));
            i = pl::sub(&i, &n(1));
        }
        if !keys_.is_empty() {
            key = pl::method(it, &self_, "GroupMatches", vec![gp, Value::new_array(keys_)])?;
            if key.truthy() {
                tag = key;
            }
        }
    } else if raw_value.get(tag.to_bytes()).is_undef() && !re(r" ", "").is_match(&tag) {
        // fix case of tag name
        if raw_value.get(tag.to_bytes()).is_undef() {
            pl::at(3469);
            let m = grep_first_ci(&tag, &keys(&raw_value))?;
            if m.truthy() {
                tag = m;
            }
        }
    }
    // figure out what conversions to do
    if type_.truthy() {
        if pl::str_eq(&type_, "Rational") {
            return ret(viv(&viv(&self_, "TAG_EXTRA"), tag.to_bytes()).get("Rational"));
        }
        if pl::str_eq(&type_, "Bin") {
            return ret(viv(&viv(&self_, "TAG_EXTRA"), tag.to_bytes()).get("BinVal"));
        }
    } else {
        type_ = if self_.get("OPTIONS").get("PrintConv").truthy() { pl::s("PrintConv") } else { pl::s("ValueConv") };
    }

    // start with the raw value
    let mut value = raw_value.get(tag.to_bytes());
    if value.is_undef() {
        if !tag.is_ref() {
            return pl::ret_nothing(want);
        }
        // get the value of a structure field
        tag_info = tag.clone();
        tag = tag_info.get("Name");
        value = arg(&a, 3);
        // (note: type "Both" is not allowed for structure fields)
        if !pl::str_eq(&type_, "Raw") {
            conv_types.push("ValueConv");
            if !pl::str_eq(&type_, "ValueConv") {
                conv_types.push("PrintConv");
            }
        }
    } else {
        tag_info = viv(&self_, "TAG_INFO").get(tag.to_bytes());
        if tag_info.get("Struct").truthy() && value.is_ref() {
            // must load XMPStruct.pl just in case (should already be loaded if
            // a structure was extracted, but we could also arrive here if a simple
            // list of values was stored incorrectly in a Struct tag)
            crate::runtime::require(it, "Image/ExifTool/XMPStruct.pl")?;
            // convert strucure field values
            if !pl::str_eq(&type_, "Both") {
                // (note: ConvertStruct handles the filtering and escaping too if necessary)
                return it.call_named("Image::ExifTool::XMP::ConvertStruct", vec![self_.clone(), tag_info.clone(), value.clone(), type_.clone()], want);
            }
            let vc = pl::call(it, "Image::ExifTool::XMP::ConvertStruct", vec![self_.clone(), tag_info.clone(), value.clone(), pl::s("ValueConv")])?;
            let v = pl::call(it, "Image::ExifTool::XMP::ConvertStruct", vec![self_.clone(), tag_info.clone(), value.clone(), pl::s("PrintConv")])?;
            // (must not save these in $$self{BOTH} because the values may have been escaped)
            return Ok(vec![vc, v]);
        }
        if !pl::str_eq(&type_, "Raw") {
            // use values we calculated already if we stored them
            both = viv(&self_, "BOTH").get(tag.to_bytes());
            if both.truthy() {
                if pl::str_eq(&type_, "PrintConv") {
                    value = both.at(1);
                } else if pl::str_eq(&type_, "ValueConv") {
                    value = both.at(0);
                    if value.is_undef() {
                        value = both.at(1);
                    }
                } else {
                    value_conv = both.at(0);
                    value = both.at(1);
                }
            } else {
                conv_types.push("ValueConv");
                if !pl::str_eq(&type_, "ValueConv") {
                    conv_types.push("PrintConv");
                }
            }
        }
    }

    // do the conversions
    let val_a: crate::interp::eval::ArrayRc = Rc::new(RefCell::new(Vec::new()));
    let prt_a: crate::interp::eval::ArrayRc = Rc::new(RefCell::new(Vec::new()));
    let raw_a: crate::interp::eval::ArrayRc = Rc::new(RefCell::new(Vec::new()));
    for conv_type in conv_types.iter().copied() {
        // don't convert a scalar reference or structure
        if ref_of(&value) == "SCALAR" && !tag_info.get("ConvertBinary").truthy() {
            break;
        }
        let mut conv = tag_info.get(conv_type);
        if conv.is_undef() {
            if conv_type == "ValueConv" {
                if !tag_info.get("Binary").truthy() {
                    continue;
                }
                conv = pl::s("\\$val"); // return scalar reference for binary values
            } else {
                // use PRINT_CONV from tag table if PrintConv doesn't exist
                conv = viv(&tag_info, "Table").get("PRINT_CONV");
                if conv.is_undef() {
                    continue;
                }
                if tag_info.exists(conv_type) {
                    continue;
                }
            }
        }
        // save old ValueConv value if we want Both
        if pl::str_eq(&type_, "Both") && conv_type == "PrintConv" {
            value_conv = value.clone();
        }
        let mut i: usize = 0;
        let mut val;
        let mut vals = Value::Undef;
        let mut values: Vec<Value> = Vec::new();
        let mut conv_list = Value::Undef;
        // split into list if conversion is an array
        if ref_of(&conv) == "ARRAY" {
            conv_list = conv.clone();
            conv = conv_list.at(0);
            let val_list: Vec<Value> = if ref_of(&value) == "ARRAY" { value.items() } else { pl::split_ws(&value) };
            // reorganize list if specified (Note: The writer currently doesn't
            // relist values, so they may be grouped but the order must not change)
            let relist = tag_info.get("Relist");
            if relist.truthy() {
                let mut new_list = Vec::new();
                for old_index in relist.items() {
                    let mut new_val = Value::Undef;
                    if old_index.is_ref() {
                        let mut join_ = Vec::new();
                        for x in old_index.items() {
                            let v = at_i(&val_list, x.to_i64());
                            if !v.is_undef() {
                                join_.push(v);
                            }
                        }
                        if !join_.is_empty() {
                            new_val = join(&pl::s(" "), &join_);
                        }
                    } else {
                        new_val = at_i(&val_list, old_index.to_i64());
                    }
                    if !new_val.is_undef() {
                        new_list.push(new_val);
                    }
                }
                value = Value::new_array(new_list);
            } else {
                value = Value::new_array(val_list);
            }
            if value.is_empty() {
                return pl::ret_nothing(want);
            }
        }
        // initialize array so we can iterate over values in list
        if ref_of(&value) == "ARRAY" {
            if !tag_info.get("RawJoin").is_undef() {
                val = join(&pl::s(" "), &value.items());
            } else {
                i = 0;
                vals = value.clone();
                val = vals.at(0);
            }
        } else {
            val = value.clone();
        }
        // loop through all values in list
        loop {
            if !conv.is_undef() {
                // get values of required tags if this is a Composite tag
                if ref_of(&val) == "HASH" && val_a.borrow().is_empty() {
                    // disable escape of source values so we don't double escape them
                    let old_escape = self_.get("ESCAPE_PROC");
                    self_.delete("ESCAPE_PROC");
                    // temporarily delete filter so it isn't applied to the Require'd values
                    let options = self_.get("OPTIONS");
                    let old_filter = options.get("Filter");
                    options.delete("Filter");
                    for k in keys(&val) {
                        let vk = val.get(k.to_bytes());
                        if vk.is_undef() {
                            continue;
                        }
                        let idx = k.to_i64().max(0) as usize;
                        let rv = raw_value.get(vk.to_bytes());
                        set_at(&mut raw_a.borrow_mut(), idx, rv);
                        let r = get_value(it, vec![self_.clone(), vk, pl::s("Both")], Want::List)?;
                        set_at(&mut val_a.borrow_mut(), idx, arg(&r, 0));
                        set_at(&mut prt_a.borrow_mut(), idx, arg(&r, 1));
                        if !at(&val_a.borrow(), idx).is_undef() || !viv(&tag_info, "Require").get(k.to_bytes()).truthy() {
                            continue;
                        }
                        let options = self_.get("OPTIONS");
                        if !old_filter.is_undef() {
                            options.set("Filter", old_filter.clone());
                        }
                        self_.set("ESCAPE_PROC", old_escape.clone());
                        return pl::ret_nothing(want);
                    }
                    let options = self_.get("OPTIONS");
                    if !old_filter.is_undef() {
                        options.set("Filter", old_filter.clone());
                    }
                    self_.set("ESCAPE_PROC", old_escape);
                    // set $val to $val[0], or \@val for a CODE ref conversion
                    val = if ref_of(&conv) == "CODE" { Value::ArrayRef(val_a.clone()) } else { at(&val_a.borrow(), 0) };
                }
                if ref_of(&conv) == "HASH" {
                    // look up converted value in hash
                    value = conv.get(val.to_bytes());
                    if value.is_undef() {
                        if conv.get("BITMASK").truthy() {
                            value = pl::call(it, "Image::ExifTool::DecodeBits", vec![val.clone(), conv.get("BITMASK"), tag_info.get("BitsPerWord")])?;
                        } else {
                            // use alternate conversion routine if available
                            if conv.get("OTHER").truthy() {
                                let h = set_warning_ref(it);
                                value = local_sig_warn(it, h, |it| {
                                    it.set_scalar(EVAL_WARNING, Value::Undef);
                                    pl::at(3624);
                                    let v = call_code(it, &conv.get("OTHER"), vec![val.clone(), Value::Undef, conv.clone()], Want::Scalar)?.pop().unwrap_or_default();
                                    if it.get_scalar(EVAL_WARNING).truthy() {
                                        let cw = clean_warning(it, vec![], Want::Scalar)?.pop().unwrap_or_default();
                                        let msg = pl::concat(&pl::s(&format!("{conv_type} ")), &pl::concat(&tag, &pl::concat(&pl::s(": "), &cw)));
                                        method_void(it, &self_, "Warn", vec![msg])?;
                                    }
                                    Ok(v)
                                })?;
                            }
                            if value.is_undef() {
                                if tag_info.get("PrintHex").truthy() && !val.is_undef() && pl::call(it, "Image::ExifTool::IsInt", vec![val.clone()])?.truthy() && conv_type == "PrintConv" {
                                    value = pl::sprintf("Unknown (0x%x)", &[val.clone()]);
                                } else {
                                    value = pl::concat(&pl::s("Unknown ("), &pl::concat(&val, &pl::s(")")));
                                }
                            }
                        }
                    }
                    // override with our localized language PrintConv if available
                    let cur_lang = self_.get("CUR_LANG");
                    if cur_lang.truthy() && conv_type == "PrintConv" {
                        // (no need to check for lang-alt tag names -- they won't have a PrintConv)
                        let mut tmp = cur_lang.get(tag_info.get("Name").to_bytes());
                        if ref_of(&tmp) == "HASH" {
                            tmp = tmp.get("PrintConv");
                            if tmp.truthy() {
                                if conv.get("BITMASK").truthy() && conv.get(val.to_bytes()).is_undef() {
                                    let mut vs = pl::split(&re(", ", ""), &value, 0);
                                    for v in vs.iter_mut() {
                                        let t = tmp.get(v.to_bytes());
                                        if !t.is_undef() {
                                            *v = t;
                                        }
                                    }
                                    value = join(&pl::s(", "), &vs);
                                } else {
                                    let t = tmp.get(value.to_bytes());
                                    if !t.is_undef() {
                                        value = pl::method(it, &self_, "Decode", vec![t, pl::s("UTF8")])?;
                                    }
                                }
                            }
                        }
                    }
                } else {
                    // call subroutine or do eval to convert value
                    let h = set_warning_ref(it);
                    value = local_sig_warn(it, h, |it| {
                        it.set_scalar(EVAL_WARNING, Value::Undef);
                        let v;
                        if ref_of(&conv) == "CODE" {
                            v = call_code(it, &conv, vec![val.clone(), self_.clone()], Want::Scalar)?.pop().unwrap_or_default();
                        } else {
                            //### eval ValueConv/PrintConv ($val, $self, @val, @prt, @raw)
                            // (with every lexical of GetValue in scope at this point, as
                            // the eval sees them: GoPro's PrintConv reads $tag)
                            let scope = crate::interp::Scope::new(None);
                            scope.scalar("self", self_.clone());
                            scope.scalar("tag", tag.clone());
                            scope.scalar("type", type_.clone());
                            scope.array("convTypes", conv_types.iter().map(|c| pl::s(c)).collect());
                            scope.scalar("tagInfo", tag_info.clone());
                            scope.scalar("valueConv", value_conv.clone());
                            scope.scalar("both", both.clone());
                            scope.scalar("rawValue", raw_value.clone());
                            scope.scalar("value", value.clone());
                            scope.declare(b'@', "val", Binding::Array(val_a.clone()));
                            scope.declare(b'@', "prt", Binding::Array(prt_a.clone()));
                            scope.declare(b'@', "raw", Binding::Array(raw_a.clone()));
                            scope.scalar("convType", pl::s(conv_type));
                            scope.scalar("conv", conv.clone());
                            scope.scalar("i", if vals.truthy() { n(i as i64) } else { Value::Undef });
                            let val_cell = scope.scalar("val", val.clone());
                            scope.scalar("vals", vals.clone());
                            scope.array("values", values.clone());
                            scope.scalar("convList", conv_list.clone());
                            let r = it.eval_string(&conv.to_bytes(), "Image::ExifTool", &scope, Want::Scalar);
                            val = val_cell.borrow().clone();
                            v = match r {
                                Ok(mut x) => x.pop().unwrap_or_default(),
                                Err(e) => {
                                    if it.exit_code.is_some() {
                                        return Err(Flow::Exit(it.exit_code.unwrap()));
                                    }
                                    if e.truthy() {
                                        it.set_scalar(EVAL_WARNING, e);
                                    }
                                    Value::Undef
                                }
                            };
                        }
                        if it.get_scalar(EVAL_WARNING).truthy() {
                            let cw = clean_warning(it, vec![], Want::Scalar)?.pop().unwrap_or_default();
                            let msg = pl::concat(&pl::s(&format!("{conv_type} ")), &pl::concat(&tag, &pl::concat(&pl::s(": "), &cw)));
                            method_void(it, &self_, "Warn", vec![msg])?;
                        }
                        Ok(v)
                    })?;
                }
            } else {
                value = val.clone();
            }
            if !vals.truthy() {
                break;
            }
            // must store a separate copy of each binary data value in the list
            if ref_of(&value) == "SCALAR" {
                let tval = value.deref();
                value = Value::ScalarRef(Rc::new(RefCell::new(tval)));
            }
            // save this converted value and step to next value in list
            if !value.is_undef() {
                values.push(value.clone());
            }
            i += 1;
            if i >= vals.len() {
                if !values.is_empty() {
                    value = Value::new_array(values.clone());
                }
                break;
            }
            val = vals.at(i as i64);
            if conv_list.truthy() {
                let next_conv = conv_list.at(i as i64);
                if next_conv.truthy() && pl::str_eq(&next_conv, "REPEAT") {
                    conv_list = Value::Undef;
                } else {
                    conv = next_conv;
                }
            }
        }
        // return undefined now if no value
        if value.is_undef() {
            return pl::ret_nothing(want);
        }
        // join back into single value if split for conversion list
        if conv_list.truthy() && ref_of(&value) == "ARRAY" {
            value = join(&pl::s(if conv_type == "PrintConv" { "; " } else { " " }), &value.items());
        }
    }
    if pl::str_eq(&type_, "Both") {
        // save both (unescaped) values because we often need them again
        // (Composite tags need "Both" and often Require one tag for various Composite tags)
        if !both.truthy() {
            viv(&self_, "BOTH").set(tag.to_bytes(), Value::new_array(vec![value_conv.clone(), value.clone()]));
        }
        // escape values if necessary
        let ep = self_.get("ESCAPE_PROC");
        if ep.truthy() {
            do_escape(it, &mut value, &ep)?;
            if !value_conv.is_undef() {
                do_escape(it, &mut value_conv, &ep)?;
            } else {
                value_conv = value.clone();
            }
        } else if value_conv.is_undef() {
            // $valueConv is undefined if there was no print conversion done
            value_conv = value.clone();
        }
        value = filter_ref(it, &self_, value)?;
        // return Both values as a list (ValueConv, PrintConv)
        return Ok(vec![value_conv, value]);
    }
    // escape value if necessary
    let ep = self_.get("ESCAPE_PROC");
    if ep.truthy() {
        do_escape(it, &mut value, &ep)?;
    }

    // filter if necessary
    if self_.get("OPTIONS").get("Filter").truthy() && pl::str_eq(&type_, "PrintConv") {
        value = filter_ref(it, &self_, value)?;
    }

    if ref_of(&value) == "ARRAY" {
        let options = self_.get("OPTIONS");
        if !options.get("ListItem").is_undef() {
            value = value.at(options.get("ListItem").to_i64());
        } else if want == Want::List {
            // return array if requested
            return Ok(value.items());
        } else if pl::str_eq(&type_, "PrintConv") && !options.get("List").truthy() {
            // join PrintConv values in delimited string if List option not used
            // and list contains simple scalars (otherwise return ARRAY ref)
            if value.items().iter().any(|x| x.is_ref()) {
                return ret(value);
            }
            value = join(&options.get("ListSep"), &value.items());
        }
    }
    ret(value)
}

/// `$self->Filter($$self{OPTIONS}{Filter}, \$value)`: the filtered value.
fn filter_ref(it: &mut Interp, self_: &Value, value: Value) -> ER<Value> {
    let cell = Rc::new(RefCell::new(value));
    let f = self_.get("OPTIONS").get("Filter");
    method_void(it, self_, "Filter", vec![f, Value::ScalarRef(cell.clone())])?;
    let v = cell.borrow().clone();
    Ok(v)
}

/// `"$name ($i)"`
fn copy_key(name: &Value, i: &Value) -> Value {
    pl::concat(name, &pl::concat(&pl::s(" ("), &pl::concat(i, &pl::s(")"))))
}

/// `$h{$k} || $default`
fn or_default(v: Value, d: Value) -> Value {
    if v.truthy() {
        v
    } else {
        d
    }
}

/// Build Composite tags from Require'd/Desire'd tags
/// Inputs: 0) ExifTool object reference, 1) flag to build only tags that require
///         tags from alternate files (without this, these tags are ignored)
/// Note: Tag values are calculated in alphabetical order unless a tag Require's
///       or Desire's another Composite tag, in which case the calculation is
///       deferred until after the other tag is calculated.
pub fn build_composite_tags(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    local_topic(it, |it| {
        let (self_, alt_only) = (arg(&a, 0), arg(&a, 1));

        self_.set("BuildingComposite", n(1));

        let comp_table = pl::call(it, "Image::ExifTool::GetTagTable", vec![pl::s("Image::ExifTool::Composite")])?;
        let mut tag_list = sort_str(keys(&comp_table));
        let raw_value = self_.get("VALUE");
        let comp_keys = self_.get("COMP_KEYS");
        let cache = Value::new_hash();
        let mut all_built = false;
        let special_tags = ghash(it, "Image::ExifTool::specialTags");
        let composite_id = lhash(it, "compositeID");

        loop {
            let not_built = Value::new_hash();
            let mut deferred_tags: Vec<Value> = Vec::new();
            for t in &tag_list {
                if !special_tags.get(t.to_bytes()).truthy() {
                    not_built.set(comp_table.get(t.to_bytes()).get("Name").to_bytes(), n(1));
                }
            }
            'composite_tag: for tag in tag_list.iter() {
                if special_tags.get(tag.to_bytes()).truthy() {
                    continue;
                }
                let tag_info = pl::method(it, &self_, "GetTagInfo", vec![comp_table.clone(), tag.clone()])?;
                if !tag_info.truthy() {
                    continue;
                }
                let tag_name = comp_table.get(tag.to_bytes()).get("Name");
                // put required tags into array and make sure they all exist
                let sd = tag_info.get("SubDoc");
                let sub_doc = if sd.truthy() { self_.get("DOC_COUNT").truthy() } else { false };
                let require = or_default(tag_info.get("Require"), Value::new_hash());
                let desire = or_default(tag_info.get("Desire"), Value::new_hash());
                let inhibit = or_default(tag_info.get("Inhibit"), Value::new_hash());
                // loop through sub-documents if necessary
                let mut doc_num = n(0);
                loop {
                    let tag_key = Value::new_hash();
                    let mut found: Option<bool> = None;
                    let mut require_alt = false;
                    // save Require'd and Desire'd tag values in list
                    let mut index: i64 = 0;
                    loop {
                        let ik = index.to_string();
                        let mut req_tag = or_default(or_default(require.get(&ik), desire.get(&ik)), inhibit.get(&ik));
                        if !req_tag.truthy() {
                            // allow Composite with no Require'd or Desire'd tags
                            if index == 0 {
                                found = Some(true);
                            }
                            break;
                        }
                        if sub_doc {
                            // handle SubDoc tags specially to cache tag keys for faster
                            // processing when there are a large number of sub-documents
                            // - get document number from the tag groups if specified,
                            //   otherwise we are looping through all documents for this tag
                            let mut d2 = None;
                            let doc = if re(r"\b(Main|Doc(\d+)):", "").subst(&mut req_tag, false, |c| {
                                d2 = Some(cap(c, 2));
                                Vec::new()
                            }) > 0
                            {
                                or_default(d2.unwrap_or_default(), n(0))
                            } else {
                                doc_num.clone()
                            };
                            // make fast lookup for keys of this tag with specified groups other than doc group
                            // (similar to code in InsertTagValues(), but this is case-sensitive)
                            let mut cache_tag = cache.get(req_tag.to_bytes());
                            if !cache_tag.truthy() {
                                cache_tag = Value::new_array(vec![]);
                                cache.set(req_tag.to_bytes(), cache_tag.clone());
                                let mut req_group = Value::Undef;
                                let mut g = Value::Undef;
                                if re(r"^(.*):", "").subst(&mut req_tag, false, |c| {
                                    g = cap(c, 1);
                                    Vec::new()
                                }) > 0
                                {
                                    req_group = g;
                                }
                                let mut keys_: Vec<Value> = Vec::new();
                                // build list of tag keys in order of precedence
                                let mut key = req_tag.clone();
                                let mut i = or_default(viv(&self_, "DUPL_TAG").get(req_tag.to_bytes()), n(0));
                                loop {
                                    if !raw_value.get(key.to_bytes()).is_undef() {
                                        keys_.push(key.clone());
                                    }
                                    if pl::le(&i, &n(0)) {
                                        break;
                                    }
                                    key = copy_key(&req_tag, &i);
                                    i = pl::sub(&i, &n(1));
                                }
                                if !req_group.is_undef() {
                                    keys_ = pl::method_list(it, &self_, "GroupMatches", vec![req_group, Value::new_array(keys_)])?;
                                }
                                // loop through tags in reverse order of precedence so the higher
                                // priority tag will win in the case of duplicates within a doc
                                for k in keys_.iter().rev() {
                                    let g3 = viv(&viv(&self_, "TAG_EXTRA"), k.to_bytes()).get("G3");
                                    let idx = or_default(g3, n(0)).to_i64();
                                    let idx = if idx < 0 { idx + cache_tag.len() as i64 } else { idx };
                                    if idx >= 0 {
                                        cache_tag.set_at(idx as usize, k.clone());
                                    }
                                }
                            }
                            // (set $reqTag to a bogus key if not found)
                            req_tag = or_default(cache_tag.at(doc.to_i64()), pl::concat(&req_tag, &pl::s(" (0)")));
                        } else if let Some(c) = re(r"^(.*):(.+)", "").m(&req_tag) {
                            let (req_group, name) = (cap(&c, 1), cap(&c, 2));
                            if pl::str_eq(&req_group, "Composite") && not_built.get(name.to_bytes()).truthy() {
                                // defer only until all other tags are built if
                                // we are inhibiting based on another Composite tag
                                if !(inhibit.get(&ik).truthy() && all_built) {
                                    deferred_tags.push(tag.clone());
                                    continue 'composite_tag;
                                }
                            }
                            let mut keys_: Vec<Value> = Vec::new();
                            let mut alt_file = Value::Undef;
                            let mut et = self_.clone();
                            // get tags from alternate file if a family 8 group was specified
                            if let Some(fc) = re(r"\b(File\d+):", "i").m(&req_tag) {
                                let f = cap(&fc, 1);
                                let alt = viv(&self_, "ALT_EXIFTOOL").get(f.to_bytes());
                                if alt.truthy() {
                                    et = alt;
                                    alt_file = f;
                                    // set flags indicating we require tags from alternate files
                                    require_alt = true;
                                    self_.set("DoAltComposite", n(1));
                                }
                            }
                            // (CAREFUL! keys may not be sequential if one was deleted)
                            let mut key = name.clone();
                            let mut i = or_default(viv(&et, "DUPL_TAG").get(name.to_bytes()), n(0));
                            loop {
                                if !viv(&et, "VALUE").get(key.to_bytes()).is_undef() {
                                    keys_.push(key.clone());
                                }
                                if pl::le(&i, &n(0)) {
                                    break;
                                }
                                key = copy_key(&name, &i);
                                i = pl::sub(&i, &n(1));
                            }
                            let keys_ref = Value::new_array(keys_);
                            // make sure the necessary information is available from the alternate file
                            if alt_file.truthy() {
                                method_void(it, &self_, "CopyAltInfo", vec![alt_file.clone(), keys_ref.clone()])?;
                            }
                            // find first matching tag
                            key = pl::method(it, &self_, "GroupMatches", vec![req_group, keys_ref])?;
                            req_tag = or_default(key, pl::concat(&name, &pl::s(" (0)")));
                        } else if not_built.get(req_tag.to_bytes()).truthy() && !inhibit.get(&ik).truthy() {
                            // calculate this tag later if it relies on another
                            // Composite tag which hasn't been calculated yet
                            deferred_tags.push(tag.clone());
                            continue 'composite_tag;
                        }
                        if !raw_value.get(req_tag.to_bytes()).is_undef() {
                            if inhibit.get(&ik).truthy() {
                                found = Some(false);
                                break;
                            } else {
                                found = Some(true);
                            }
                        } else if require.get(&ik).truthy() {
                            found = Some(false);
                            break; // don't continue since we require this tag
                        }
                        tag_key.set(&ik, req_tag);
                        index += 1;
                    }
                    // stop now if this requires alternate tags and we aren't building them
                    if require_alt != alt_only.truthy() {
                        break;
                    }
                    if doc_num.truthy() {
                        if found == Some(true) {
                            self_.set("DOC_NUM", doc_num.clone());
                            // save pointers to all used tag keys
                            save_comp_keys(&comp_keys, &tag_key);
                            method_void(it, &self_, "FoundTag", vec![tag_info.clone(), tag_key.clone()])?;
                            self_.delete("DOC_NUM");
                        }
                        doc_num = pl::add(&doc_num, &n(1));
                        if pl::le(&doc_num, &self_.get("DOC_COUNT")) {
                            continue;
                        }
                        break;
                    } else if found == Some(true) {
                        not_built.delete(tag_name.to_bytes()); // this tag is OK to build now
                        // keep track of all Require'd tag keys
                        for k in keys(&tag_key) {
                            // only tag keys with same name as a Composite tag
                            // can be replaced (also eliminates keys with
                            // instance numbers which can't be replaced either)
                            if !composite_id.get(tag_key.get(k.to_bytes()).to_bytes()).truthy() {
                                continue;
                            }
                        }
                        // save pointers to all used tag keys
                        save_comp_keys(&comp_keys, &tag_key);
                        // save reference to tag key lookup as value for Composite tag
                        pl::method(it, &self_, "FoundTag", vec![tag_info.clone(), tag_key.clone()])?;
                    } else if found.is_none() {
                        not_built.delete(tag_name.to_bytes()); // tag can't be built anyway
                    }
                    if !sub_doc {
                        break;
                    }
                    // don't process sub-documents if there is no chance to build this tag
                    // (can be very time-consuming if there are many docs)
                    if !require.is_empty() {
                        for k in keys(&require) {
                            let mut req_tag = require.get(k.to_bytes());
                            re(r".*:", "").subst(&mut req_tag, false, |_| Vec::new());
                            if raw_value.get(req_tag.to_bytes()).is_undef() {
                                continue 'composite_tag;
                            }
                        }
                        doc_num = n(1); // go ahead and process the 1st sub-document
                    } else {
                        let sd = tag_info.get("SubDoc");
                        let try_ = if sd.is_ref() { sd.items() } else { keys(&desire) };
                        // at least one of the specified desire tags must exist
                        for t in try_ {
                            let mut des_tag = desire.get(t.to_bytes());
                            if !des_tag.truthy() {
                                continue;
                            }
                            re(r".*:", "").subst(&mut des_tag, false, |_| Vec::new());
                            if !raw_value.get(des_tag.to_bytes()).is_undef() {
                                doc_num = n(1);
                                break;
                            }
                        }
                        if !doc_num.truthy() {
                            break;
                        }
                    }
                }
            }
            if deferred_tags.is_empty() {
                break;
            }
            if deferred_tags.len() == tag_list.len() {
                if all_built {
                    // everything was deferred in the last pass,
                    // must be a circular dependency
                    pl::warn(it, "Circular dependency in Composite tags\n")?;
                    break;
                }
                all_built = true; // try once more, ignoring Composite Inhibit tags
            }
            tag_list = deferred_tags; // calculate deferred tags now
        }
        Ok(vec![self_.delete("BuildingComposite")])
    })
}

/// The two loops of BuildCompositeTags that save pointers to the used tag
/// keys:
///     foreach (keys %tagKey) {
///         $$compKeys{$_} or $$compKeys{$_} = [ ];
///         push @{$$compKeys{$tagKey{$_}}}, [ \%tagKey, $_ ];
///     }
fn save_comp_keys(comp_keys: &Value, tag_key: &Value) {
    for k in keys(tag_key) {
        if !comp_keys.get(k.to_bytes()).truthy() {
            comp_keys.set(k.to_bytes(), Value::new_array(vec![]));
        }
        comp_keys.get_or_new_array(tag_key.get(k.to_bytes()).to_bytes()).push(Value::new_array(vec![tag_key.clone(), k]));
    }
}

/// `tr/-_a-zA-Z0-9#//dc`: delete every other character.
fn tr_param_name(v: &Value) -> Value {
    let b: Vec<u8> = v.to_bytes().iter().copied().filter(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'#')).collect();
    Value::bytes(b)
}

/// `$x =~ /$pat/` compiled at run time, dying with the message the
/// interpreter gives for a bad pattern.
fn re_interp(pat: &Value, flags: &str) -> ER<pl::Re> {
    let mut bytes = pat.to_bytes().to_vec();
    if bytes.is_empty() {
        bytes = b"(?:)".to_vec();
    }
    match pl::re_dyn(&bytes, flags) {
        Ok(r) => Ok(r),
        // (the message is perl's already: see regex::perl_message)
        Err(e) => Err(e),
    }
}

/// Get/set ExifTool options
/// Inputs: 0) ExifTool object reference,
///         1) Parameter name (case insensitive), 2) Value to set the option
///         3-N) More parameter/value pairs
/// Returns: original value of last option specified
pub fn options(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    local_topic(it, |it| options_body(it, a))
}

fn options_body(it: &mut Interp, a: Vec<Value>) -> ER<Vec<Value>> {
    let self_ = arg(&a, 0);
    let mut args: std::collections::VecDeque<Value> = a.into_iter().skip(1).collect();
    let options = self_.get("OPTIONS");
    let mut old_val = Value::Undef;

    while !args.is_empty() {
        let mut param = args.pop_front().unwrap();
        let mut plus = false;
        // fix parameter case if necessary
        if !options.exists(param.to_bytes()) {
            plus = re(r"\+$", "").subst(&mut param, false, |_| Vec::new()) > 0;
            pl::at(2437);
            let fixed = grep_first_ci(&param, &keys(&options))?;
            if fixed.truthy() {
                param = fixed;
            } else {
                re(r"^Group(\d*)$", "i").subst(&mut param, false, |c| [&b"Group"[..], &cap(c, 1).to_bytes()].concat());
            }
        }
        let p = param.to_text();
        old_val = options.get(param.to_bytes());
        if ref_of(&old_val) == "HASH" && (p == "Compact" || p == "XMPShorthand") {
            // get previous Compact/XMPShorthand setting
            old_val = old_val.get(param.to_bytes());
        }
        if args.is_empty() {
            break;
        }
        let mut new_val = args.pop_front().unwrap();
        if p == "Lang" {
            // allow this to be set to undef to select the default language
            let default_lang = it.get_scalar("Image::ExifTool::defaultLang");
            if new_val.is_undef() {
                new_val = default_lang.clone();
            }
            if new_val.to_bytes() == default_lang.to_bytes() {
                options.set(param.to_bytes(), new_val);
                self_.delete("CUR_LANG");
            // make sure the language is available
            } else {
                let langs = Value::new_hash();
                for l in garray(it, "Image::ExifTool::langs").items() {
                    langs.set(l.to_bytes(), n(1));
                }
                // (and the languages exiftool-rs adds, which @langs leaves out
                // so that the lists ExifTool prints stay its own)
                for (l, _) in crate::runtime::extra_langs() {
                    langs.set(l.as_bytes(), n(1));
                }
                if langs.get(new_val.to_bytes()).truthy() {
                    let code = format!("require Image::ExifTool::Lang::{}", new_val.to_text());
                    let scope = crate::interp::Scope::new(None);
                    let ok = match it.eval_string(code.as_bytes(), "Image::ExifTool", &scope, Want::Scalar) {
                        Ok(v) => v.last().is_some_and(|v| v.truthy()),
                        Err(_) => {
                            if let Some(c) = it.exit_code {
                                return Err(Flow::Exit(c));
                            }
                            false
                        }
                    };
                    if ok {
                        let xlat = format!("Image::ExifTool::Lang::{}::Translate", new_val.to_text());
                        let h = ghash(it, &xlat);
                        if !h.is_empty() {
                            self_.set("CUR_LANG", h);
                            options.set(param.to_bytes(), new_val);
                        }
                    }
                }
            } // else don't change Lang
        } else if p == "Exclude" && !new_val.is_undef() {
            // clone Exclude list and expand shortcuts
            let exclude = if ref_of(&new_val) == "ARRAY" { Value::new_array(new_val.items()) } else { Value::new_array(vec![new_val]) };
            call_void(it, "Image::ExifTool::ExpandShortcuts", vec![exclude.clone(), n(1)])?; // (also remove '#' suffix)
            options.set(param.to_bytes(), exclude);
        } else if re(r"^Charset", "").is_match(&param) || p == "IPTCCharset" {
            // only allow valid character sets to be set
            if new_val.truthy() {
                let charset = ghash(it, "Image::ExifTool::charsetName").get(lc(&new_val).to_bytes());
                if charset.truthy() {
                    options.set(param.to_bytes(), charset.clone());
                    // maintain backward-compatibility with old IPTCCharset option
                    if p == "IPTCCharset" {
                        options.set("CharsetIPTC", charset);
                    }
                } else {
                    pl::warn(it, &format!("Invalid Charset {}\n", new_val.to_text()))?;
                }
            } else if p == "CharsetEXIF" || p == "CharsetFileName" || p == "CharsetRIFF" {
                options.set(param.to_bytes(), new_val); // only these may be set to a false value
            } else if p == "CharsetQuickTime" {
                options.set(param.to_bytes(), pl::s("MacRoman")); // QuickTime defaults to MacRoman
            } else {
                options.set(param.to_bytes(), pl::s("Latin")); // all others default to Latin
            }
        } else if p == "UserParam" {
            // clear options if $newVal is undef
            if new_val.is_undef() {
                options.set(param.to_bytes(), Value::new_hash());
                continue;
            }
            let table = pl::call(it, "Image::ExifTool::GetTagTable", vec![pl::s("Image::ExifTool::UserParam")])?;
            // allow initialization of entire UserParam hash
            if ref_of(&new_val) == "HASH" {
                let new_params = Value::new_hash();
                for k in sort_str(keys(&new_val)) {
                    let lc_tag = lc(&k);
                    new_params.set(lc_tag.to_bytes(), new_val.get(k.to_bytes()));
                    table.delete(lc_tag.to_bytes());
                    call_void(it, "Image::ExifTool::AddTagToTable", vec![table.clone(), lc_tag, k])?;
                }
                options.set(param.to_bytes(), new_params);
                continue;
            }
            let mut force = false;
            let mut param_name = Value::Undef;
            // set/reset single UserParam parameter
            if let Some(c) = re(r"(.*?)=(.*)", "s").m(&new_val) {
                param_name = cap(&c, 1);
                new_val = cap(&c, 2);
                if re(r"\^$", "").subst(&mut param_name, false, |_| Vec::new()) > 0 {
                    force = true;
                }
                param_name = tr_param_name(&param_name);
                param = lc(&param_name);
            } else {
                param = tr_param_name(&lc(&new_val));
                new_val = Value::Undef;
            }
            table.delete(param.to_bytes());
            old_val = viv(&options, "UserParam").get(param.to_bytes());
            if !new_val.is_undef() {
                if !new_val.to_bytes().is_empty() || force {
                    viv(&options, "UserParam").set(param.to_bytes(), new_val);
                    call_void(it, "Image::ExifTool::AddTagToTable", vec![table.clone(), param.clone(), param_name.clone()])?;
                } else {
                    viv(&options, "UserParam").delete(param.to_bytes());
                }
            }
            // remove alternate version of tag
            if re(r"#$", "").subst(&mut param, false, |_| Vec::new()) == 0 {
                param = pl::concat(&param, &pl::s("#"));
            }
            table.delete(param.to_bytes());
            viv(&options, "UserParam").delete(param.to_bytes());
        } else if p == "RequestTags" {
            if !new_val.is_undef() {
                // parse list from delimited string if necessary
                let req_list = Value::new_array(if ref_of(&new_val) == "ARRAY" { new_val.items() } else { re(r"[-\w?*:]+", "").all(&new_val) });
                call_void(it, "Image::ExifTool::ExpandShortcuts", vec![req_list.clone()])?;
                // add to existing list
                if !options.get(param.to_bytes()).truthy() {
                    options.set(param.to_bytes(), Value::new_array(vec![]));
                }
                for item in req_list.items() {
                    let Some(c) = re(r"^(.*:)?([-\w?*]*)#?$", "").m(&item) else { continue };
                    let list = options.get(param.to_bytes());
                    if cap(&c, 2).truthy() {
                        list.push(lc(&cap(&c, 2)));
                    }
                    if !cap(&c, 1).truthy() {
                        continue;
                    }
                    // add requested groups with trailing colon
                    for g in pl::split(&re(":", ""), &cap(&c, 1), 0) {
                        list.push(pl::concat(&lc(&g), &pl::s(":")));
                    }
                }
            } else {
                options.set(param.to_bytes(), Value::Undef); // clear the list
            }
        } else if re(r"^(IgnoreTags|IgnoreGroups)$", "").is_match(&param) {
            if !new_val.is_undef() {
                if ref_of(&new_val) == "HASH" {
                    options.set(param.to_bytes(), new_val);
                    continue;
                }
                // parse list from delimited string if necessary
                let ignore_list = Value::new_array(if ref_of(&new_val) == "ARRAY" { new_val.items() } else { re(r"[-\w?*:#]+", "").all(&new_val) });
                if p == "IgnoreTags" {
                    call_void(it, "Image::ExifTool::ExpandShortcuts", vec![ignore_list.clone()])?;
                }
                // add to existing tags/groups to ignore
                if !options.get(param.to_bytes()).truthy() {
                    options.set(param.to_bytes(), Value::new_hash());
                }
                for item in ignore_list.items() {
                    let Some(c) = re(r"^(.*:)?([-\w?*]+)#?$", "").m(&item) else { continue };
                    options.get(param.to_bytes()).set(lc(&cap(&c, 2)).to_bytes(), n(1));
                }
            } else {
                options.set(param.to_bytes(), Value::Undef); // clear the option
            }
        } else if p == "ListJoin" {
            options.set(param.to_bytes(), new_val.clone());
            // set the old List and ListSep options for backward compatibility
            if !new_val.is_undef() {
                options.set("List", n(0));
                options.set("ListSep", new_val);
            } else {
                options.set("List", n(1));
                // (ListSep must be defined)
            }
        } else if p == "List" {
            options.set(param.to_bytes(), new_val.clone());
            // set the new ListJoin option for forward compatibility
            let lj = if new_val.truthy() { Value::Undef } else { options.get("ListSep") };
            options.set("ListJoin", lj);
        } else if p == "Compact" || p == "XMPShorthand" {
            // set Compact and XMPShorthand options, preserving backward compatibility
            let compact = Value::new_hash();
            for pp in ["Compact", "XMPShorthand"] {
                // (allow setting from a HASH (undocumented)
                if ref_of(&new_val) == "HASH" {
                    if let (Value::HashRef(dst), Value::HashRef(src)) = (&compact, &new_val) {
                        let mut c = src.borrow().clone();
                        c.blessed = None;
                        *dst.borrow_mut() = c;
                    }
                    continue;
                }
                let val = if p == pp { new_val.clone() } else { viv(&options, "Compact").get(pp) };
                if !val.is_undef() {
                    let v = re(r"\w+", "").all(&val);
                    let opt = if pp == "Compact" { lhash(it, "compactOpt") } else { lhash(it, "xmpShorthandOpt") };
                    for x in v {
                        let set = opt.get(lc(&x).to_bytes());
                        if !set.truthy() {
                            pl::warn(it, &format!("Invalid {pp} setting '{}'\n", x.to_text()))?;
                            return ret(old_val);
                        }
                        if !set.is_ref() {
                            compact.set(set.to_bytes(), n(1));
                            continue;
                        }
                        for s in set.items() {
                            compact.set(s.to_bytes(), n(1));
                        }
                    }
                }
                compact.set(pp, val); // preserve most recent setting
            }
            options.set("XMPShorthand", compact.clone());
            options.set("Compact", compact);
        } else if p == "NoWarning" {
            // validate regular expression
            it.set_scalar(EVAL_WARNING, Value::Undef);
            if !new_val.is_undef() {
                let h = set_warning_ref(it);
                local_sig_warn(it, h, |it| {
                    eval_block(it, |_it| re_interp(&new_val, "").map(|r| r.is_match(&param)))?;
                    let e = it.get_scalar("main::@");
                    if e.truthy() {
                        it.set_scalar(EVAL_WARNING, e);
                    }
                    Ok(())
                })?;
            }
            if it.get_scalar(EVAL_WARNING).truthy() {
                let cw = clean_warning(it, vec![], Want::Scalar)?.pop().unwrap_or_default();
                pl::warn(it, &format!("NoWarning: {}\n", cw.to_text()))?;
                continue;
            }
            // add to existing expression if specified
            if plus && !old_val.is_undef() {
                new_val = if !new_val.is_undef() { pl::concat(&old_val, &pl::concat(&pl::s("|"), &new_val)) } else { old_val.clone() };
            }
            options.set(param.to_bytes(), new_val);
        } else if p == "ImageHashType" {
            if new_val.is_undef() {
                pl::warn(it, &format!("Can't set {p} to undef\n"))?;
            } else if re(r"^(MD5|SHA256|SHA512)$", "i").is_match(&new_val) {
                options.set(param.to_bytes(), uc(&new_val));
            } else {
                pl::warn(it, &format!("Invalid {p} setting '{}'\n", new_val.to_text()))?;
            }
        } else if p == "StructFormat" {
            if !new_val.is_undef() {
                if !re(r"^(JSON|JSONQ)$", "i").is_match(&new_val) {
                    pl::warn(it, &format!("Invalid {p} setting '{}'\n", new_val.to_text()))?;
                    continue;
                }
                new_val = uc(&new_val);
            }
            options.set(param.to_bytes(), new_val);
        } else if p == "ByteUnit" {
            if !new_val.is_undef() {
                // (allow "Metric" or "SI" for SI, and "IT" or "Binary" for Binary)
                let good_val = if re(r"^S|M", "i").is_match(&new_val) {
                    pl::s("SI")
                } else if re(r"^I|B", "i").is_match(&new_val) {
                    pl::s("Binary")
                } else {
                    Value::Undef
                };
                if !good_val.truthy() {
                    pl::warn(it, &format!("Invalid {p} setting '{}'\n", new_val.to_text()))?;
                    continue;
                }
                options.set(param.to_bytes(), good_val);
            } else {
                pl::warn(it, &format!("Can't set {p} to undef\n"))?;
            }
        } else if p == "Plot" {
            // add to existing plot settings
            if !old_val.is_undef() && !new_val.is_undef() {
                new_val = pl::concat(&old_val, &pl::concat(&pl::s(","), &new_val));
            }
            options.set(param.to_bytes(), new_val);
        } else if p == "KeepUTCTime" || p == "SystemTimeRes" {
            ghash(it, "Image::ExifTool::static_vars").set(param.to_bytes(), new_val.clone());
            options.set(param.to_bytes(), new_val);
        } else if pl::str_eq(&lc(&param), "geodir") {
            it.set_scalar("Image::ExifTool::Geolocation::geoDir", new_val);
        } else {
            if p == "Escape" {
                // set ESCAPE_PROC
                if !new_val.is_undef() && pl::str_eq(&new_val, "XML") {
                    crate::runtime::require(it, "Image::ExifTool::XMP")?;
                    let r = Value::Code(it.sub_ref("Image::ExifTool::XMP::EscapeXML"));
                    self_.set("ESCAPE_PROC", r);
                } else if !new_val.is_undef() && pl::str_eq(&new_val, "HTML") {
                    crate::runtime::require(it, "Image::ExifTool::HTML")?;
                    let r = Value::Code(it.sub_ref("Image::ExifTool::HTML::EscapeHTML"));
                    self_.set("ESCAPE_PROC", r);
                } else {
                    self_.delete("ESCAPE_PROC");
                }
                // must forget saved values since they depend on Escape method
                self_.set("BOTH", Value::new_hash());
            } else if p == "GlobalTimeShift" {
                self_.delete("GLOBAL_TIME_OFFSET"); // reset our calculated offset
            } else if p == "TimeZone" && !new_val.is_undef() && !new_val.to_bytes().is_empty() {
                ghash(it, "main::ENV").set("TZ", new_val.clone());
                if pl::str_eq(&os(it), "MSWin32") {
                    if eval_block(it, |it| crate::runtime::require(it, "Time::Piece"))?.is_some() {
                        eval_block(it, |it| call_void(it, "Time::Piece::_tzset", vec![]))?;
                    } else {
                        pl::warn(it, "Install Time::Piece to set time zone in Windows\n")?;
                    }
                } else {
                    eval_block(it, |it| {
                        crate::runtime::require(it, "POSIX")?;
                        call_void(it, "POSIX::tzset", vec![])
                    })?;
                }
            } else if p == "Validate" {
                // load Validate module if Validate option enabled
                if new_val.truthy() {
                    crate::runtime::require(it, "Image::ExifTool::Validate")?;
                }
            }
            options.set(param.to_bytes(), new_val);
        }
    }
    ret(old_val)
}

/// The first match of a pattern from `pos` 0 (`$s =~ /re/g` on a string
/// just assigned): the captures and where the match ends (`pos($s)`).
fn match_g(pat: &[u8], subject: &Value) -> ER<Option<(pl::Caps, usize)>> {
    let r = match crate::perl::regex::compile(pat, "") {
        Ok(r) => r,
        Err(e) => return crate::interp::eval::die(e),
    };
    let b = subject.to_bytes();
    match r.match_at(&b, 0) {
        Ok(Some(sp)) => {
            let end = sp[0].map(|(_, e)| e).unwrap_or(0);
            let caps = sp.iter().map(|g| g.map(|(s, e)| Value::bytes(&b[s..e]))).collect();
            Ok(Some((caps, end)))
        }
        _ => Ok(None),
    }
}

/// `stat EXPR` (list context).
fn stat(it: &mut Interp, file: &Value) -> ER<Vec<Value>> {
    Ok(perl_snippet(it, "stat $f", &[("f", file.clone())], Want::List)?.0)
}

/// A file test (`-f`, `-p`, `-s`) on a file name or handle.
fn file_test(it: &mut Interp, op: char, file: &Value) -> ER<Value> {
    Ok(perl_snippet(it, &format!("-{op} $f"), &[("f", file.clone())], Want::Scalar)?.0.pop().unwrap_or_default())
}

/// Extract meta information from image
/// Inputs: 0) ExifTool object reference
///         1-N) Same as ImageInfo()
/// Returns: 1 if this was a valid image, 0 otherwise
/// Notes: pass an undefined value to avoid parsing arguments
/// Internal 'ReEntry' option allows this routine to be called recursively
pub fn extract_info(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    local_topic(it, |it| extract_info_body(it, a))
}

fn extract_info_body(it: &mut Interp, a: Vec<Value>) -> ER<Vec<Value>> {
    let self_ = arg(&a, 0);
    let args: Vec<Value> = a[1.min(a.len())..].to_vec();
    let options = self_.get("OPTIONS"); // pointer to current options
    let fast = or_default(options.get("FastScan"), n(0));
    let req = self_.get("REQ_TAG_LOOKUP");
    let req_all = or_default(options.get("RequestAll"), n(0));
    let mut save_options = Value::new_hash();
    let mut re_entry = Value::Undef;
    let mut rsize = Value::Undef;
    let mut zid = false;
    let mut type_ = Value::Undef;
    let mut start_time: Vec<Value> = Vec::new();
    let mut save_order = Value::Undef;
    let mut is_dir = false;
    let s = |x: &str| pl::s(x);
    let found_tag = |it: &mut Interp, name: &str, v: Value| method_void(it, &self_, "FoundTag", vec![pl::s(name), v]);
    let a0 = arg(&args, 0);
    let a1 = arg(&args, 1);

    // check for internal ReEntry option to allow recursive calls to ExtractInfo
    if ref_of(&a1) == "HASH" && a1.get("ReEntry").truthy() && (ref_of(&a0) == "SCALAR" || ref_of(&a0) == "GLOB") {
        // save necessary members for restoring later
        re_entry = Value::new_hash();
        for k in ["RAF", "PROCESSED", "EXIF_DATA", "EXIF_POS", "FILE_TYPE"] {
            re_entry.set(k, self_.get(k));
        }
        save_order = pl::call(it, "Image::ExifTool::GetByteOrder", vec![])?;
        let raf = pl::method(it, &s("File::RandomAccess"), "new", vec![a0.clone()])?;
        self_.set("RAF", raf);
        self_.set("PROCESSED", Value::new_hash());
        self_.delete("EXIF_DATA");
        self_.delete("EXIF_POS");
    } else {
        if !a0.is_undef() || options.get("HtmlDump").truthy() || req.get("validate").truthy() {
            save_options = options.hash_copy(); // save original options

            // require duplicates for html dump
            if options.get("HtmlDump").truthy() {
                self::options(it, vec![self_.clone(), s("Duplicates"), n(1)], Want::Scalar)?;
            }
            // enable Validate option if Validate tag is requested
            if req.get("validate").truthy() {
                self::options(it, vec![self_.clone(), s("Validate"), n(1)], Want::Scalar)?;
            }
            if !a0.is_undef() {
                // only initialize filename if called with arguments
                self_.set("FILENAME", Value::Undef); // name of file (or '' if we didn't open it)
                self_.set("RAF", Value::Undef); // RandomAccess object reference

                method_void(it, &self_, "ParseArguments", args.clone())?; // initialize from our arguments
            }
        }
        // ignore all tags and set ExtractEmbedded if outputting CSV directly
        if self::options(it, vec![self_.clone(), s("PrintCSV")], Want::Scalar)?.pop().unwrap_or_default().truthy() {
            let all = Value::new_hash();
            all.set("all", n(1));
            self_.get("OPTIONS").set("IgnoreTags", all);
            self::options(it, vec![self_.clone(), s("ExtractEmbedded"), n(1)], Want::Scalar)?;
        }
        // initialize ExifTool object members
        init(it, vec![self_.clone()], Want::Void)?;
        self_.set("InExtract", n(1)); // set flag indicating we are inside ExtractInfo

        self_.delete("MAKER_NOTE_FIXUP"); // fixup information for extracted maker notes
        self_.delete("MAKER_NOTE_BYTE_ORDER");

        // return our version number
        let version = pl::concat(&it.get_scalar("Image::ExifTool::VERSION"), &it.get_scalar("Image::ExifTool::RELEASE"));
        found_tag(it, "ExifToolVersion", version)?;
        if req.get("now").truthy() || req_all.truthy() {
            let v = pl::method(it, &self_, "TimeNow", vec![])?;
            found_tag(it, "Now", v)?;
        }
        if req.get("newguid").truthy() || req_all.truthy() {
            let v = pl::call(it, "Image::ExifTool::NewGUID", vec![])?;
            found_tag(it, "NewGUID", v)?;
        }
        if req.get("newuuid").truthy() || req_all.truthy() {
            let v = pl::call(it, "Image::ExifTool::NewUUID", vec![])?;
            found_tag(it, "NewUUID", v)?;
        }
        // generate sequence number if necessary
        if req.get("filesequence").truthy() || req_all.truthy() {
            found_tag(it, "FileSequence", self_.get("FILE_SEQUENCE"))?;
        }

        if req.get("processingtime").truthy() || req_all.truthy() {
            if let Some(t) = eval_block(it, |it| {
                crate::runtime::require(it, "Time::HiRes")?;
                pl::call_list(it, "Time::HiRes::gettimeofday", vec![])
            })? {
                start_time = t;
            }
            if start_time.is_empty() && req.get("processingtime").truthy() {
                method_void(it, &self_, "Warn", vec![s("Install Time::HiRes to generate ProcessingTime")])?;
            }
        }

        // create Hash object if ImageDataHash is requested
        if req.get("imagedatahash").truthy() && !self_.get("ImageDataHash").truthy() {
            let image_hash_type = self::options(it, vec![self_.clone(), s("ImageHashType")], Want::Scalar)?.pop().unwrap_or_default();
            if let Some(c) = re(r"^SHA(256|512)$", "i").m(&image_hash_type) {
                crate::runtime::require(it, "Digest::SHA")?;
                let h = pl::method(it, &s("Digest::SHA"), "new", vec![cap(&c, 1)])?;
                self_.set("ImageDataHash", h);
            } else {
                crate::runtime::require(it, "Digest::MD5")?;
                let h = pl::method(it, &s("Digest::MD5"), "new", vec![])?;
                self_.set("ImageDataHash", h);
            }
        }
        self_.set("FILE_SEQUENCE", pl::add(&self_.get("FILE_SEQUENCE"), &n(1))); // count files read
    }

    let filename = self_.get("FILENAME"); // image file name ('' if already open)
    let mut raf = self_.get("RAF"); // RandomAccess object

    // local *EXIFTOOL_FILE;   # avoid clashes with global namespace
    // (the interpreter's `local *FH` saves nothing: the handle is reopened)

    let mut realname = filename.clone();
    let req_or_all = |key: &str| req.get(key).truthy() || (req_all.truthy() && !viv(&self_, "EXCL_TAG_LOOKUP").get(key).truthy());
    if !raf.truthy() {
        // save file name
        if !filename.is_undef() && !filename.to_bytes().is_empty() {
            if !pl::str_eq(&filename, "-") {
                // extract file name from pipe if necessary
                if re(r"\|$", "").is_match(&realname) {
                    re(r#"^.*?"(.*?)".*"#, "s").subst(&mut realname, false, |c| cap(c, 1).to_bytes().to_vec());
                }
                let dn = pl::call_list(it, "Image::ExifTool::SplitFileName", vec![realname.clone()])?;
                let (dir, name) = (arg(&dn, 0), arg(&dn, 1));
                found_tag(it, "FileName", name.clone())?;
                if req_or_all("basename") {
                    let b = match re(r"(.*)\.", "").m(&name) {
                        Some(c) => cap(&c, 1),
                        None => name.clone(),
                    };
                    found_tag(it, "BaseName", b)?;
                }
                if !dir.is_undef() && !dir.to_bytes().is_empty() {
                    found_tag(it, "Directory", dir.clone())?;
                }
                if req_or_all("filepath") {
                    let h = set_warning_ref(it);
                    local_sig_warn(it, h, |it| {
                        let mut path = Value::Undef;
                        if pl::str_eq(&os(it), "MSWin32") && options.get("WindowsLongPath").truthy() {
                            path = pl::method(it, &self_, "WindowsLongPath", vec![filename.clone()])?;
                        } else if eval_block(it, |it| crate::runtime::require(it, "Cwd"))?.is_some() {
                            path = eval_block(it, |it| pl::call(it, "Cwd::abs_path", vec![filename.clone()]))?.unwrap_or_default();
                        }
                        if !path.is_undef() {
                            if pl::str_eq(&os(it), "MSWin32") {
                                path = Value::bytes(path.to_bytes().iter().map(|&c| if c == b'\\' { b'/' } else { c }).collect::<Vec<u8>>()); // return forward slashes
                            }
                            found_tag(it, "FilePath", path)?;
                        } else if req.get("filepath").truthy() {
                            method_void(it, &self_, "Warn", vec![s("The Perl Cwd module must be installed to use FilePath")])?;
                        }
                        Ok(())
                    })?;
                }
                // get size of resource fork on Mac OS
                if pl::str_eq(&os(it), "darwin") && !self_.get("IN_RESOURCE").truthy() {
                    rsize = file_test(it, 's', &pl::concat(&filename, &s("/..namedfork/rsrc")))?;
                }
                // check to see if Zone.Identifier file exists in Windows
                if pl::str_eq(&os(it), "MSWin32") && eval_block(it, |it| crate::runtime::require(it, "Win32API::File"))?.is_some() {
                    let wattr;
                    let mut zfile = [pl::concat(&filename, &s(":Zone.Identifier"))];
                    if method_lv(it, &self_, "EncodeFileName", &mut zfile, Want::Scalar)?.pop().unwrap_or_default().truthy() {
                        wattr = eval_block(it, |it| pl::call(it, "Win32API::File::GetFileAttributesW", vec![zfile[0].clone()]))?.unwrap_or_default();
                    } else {
                        wattr = eval_block(it, |it| pl::call(it, "Win32API::File::GetFileAttributes", vec![zfile[0].clone()]))?.unwrap_or_default();
                    }
                    let inv = pl::call(it, "Win32API::File::INVALID_FILE_ATTRIBUTES", vec![])?;
                    if !pl::num_eq(&wattr, &inv) {
                        zid = true;
                    }
                }
            }
            // open the file
            let fh = glob_ref(it, "EXIFTOOL_FILE")?;
            if pl::method(it, &self_, "Open", vec![fh.clone(), filename.clone()])?.truthy() {
                // create random access file object
                raf = pl::method(it, &s("File::RandomAccess"), "new", vec![fh])?;
                // patch to force pipe to be buffered because seek returns success
                // in Windows cmd shell pipe even though it really failed
                if pl::str_eq(&filename, "-") || re(r"\|$", "").is_match(&filename) {
                    raf.set("TESTED", n(-1));
                }
                self_.set("RAF", raf.clone());
            } else if pl::method(it, &self_, "IsDirectory", vec![filename.clone()])?.truthy() {
                is_dir = true;
            } else {
                method_void(it, &self_, "Error", vec![s("Error opening file")])?;
                // continue to process alt files if necessary
                if self_.get("ALT_EXIFTOOL").truthy() {
                    method_void(it, &self_, "DoneExtract", vec![])?;
                }
            }
        } else {
            method_void(it, &self_, "Error", vec![s("No file specified")])?;
        }
    }

    while raf.truthy() || is_dir {
        let mut stat_: Vec<Value> = Vec::new();
        let mut plain_file = false;
        if re_entry.truthy() {
            // we already set these tags
        } else if !raf.truthy() {
            stat_ = stat(it, &filename)?;
        } else if !raf.get("FILE_PT").truthy() {
            // get file size from image in memory
            let len = pl::length(&raf.get("BUFF_PT").deref());
            found_tag(it, "FileSize", len)?;
        } else if file_test(it, 'f', &raf.get("FILE_PT"))?.truthy() {
            // get file tags if this is a plain file
            stat_ = perl_snippet(it, "stat _", &[], Want::List)?.0;
            plain_file = true;
            // hack to patch Windows daylight savings time bug
            if pl::str_eq(&os(it), "MSWin32") {
                let t = pl::method_list(it, &self_, "GetFileTime", vec![raf.get("FILE_PT")])?;
                for (j, idx) in [8usize, 9, 10].iter().enumerate() {
                    set_at(&mut stat_, *idx, arg(&t, j));
                }
            }
        } else {
            // (note that Windows directories will still show the
            //  daylight savings time bug -- should fix this sometime)
            stat_ = stat(it, &raf.get("FILE_PT"))?;
            if file_test(it, 'p', &raf.get("FILE_PT"))?.truthy() {
                set_at(&mut stat_, 7, Value::Undef); // (pipe buffer size isn't useful)
            }
        }
        let file_size = at(&stat_, 7);
        if !at(&stat_, 7).is_undef() {
            found_tag(it, "FileSize", at(&stat_, 7))?;
        }
        if rsize.truthy() {
            found_tag(it, "ResourceForkSize", rsize.clone())?;
        }
        if zid {
            found_tag(it, "ZoneIdentifier", s("Exists"))?;
        }
        if !at(&stat_, 9).is_undef() {
            found_tag(it, "FileModifyDate", at(&stat_, 9))?;
        }
        if !at(&stat_, 8).is_undef() {
            found_tag(it, "FileAccessDate", at(&stat_, 8))?;
        }
        let c_tag = if pl::str_eq(&os(it), "MSWin32") { "FileCreateDate" } else { "FileInodeChangeDate" };
        if !at(&stat_, 10).is_undef() {
            found_tag(it, c_tag, at(&stat_, 10))?;
        }
        if pl::str_eq(&os(it), "linux") && !stat_.is_empty() && eval_block(it, |it| crate::runtime::require(it, "File::StatX"))?.is_some() {
            let h = set_warning_ref(it);
            local_sig_warn(it, h, |it| {
                let mut st = Value::Undef;
                if raf.truthy() {
                    if let Some(v) = eval_block(it, |it| {
                        let b = pl::call(it, "File::StatX::STATX_BTIME", vec![])?;
                        pl::call(it, "File::StatX::fstatx", vec![raf.get("FILE_PT"), n(0), b])
                    })? {
                        st = v;
                    }
                } else if let Some(v) = eval_block(it, |it| {
                    let b = pl::call(it, "File::StatX::STATX_BTIME", vec![])?;
                    pl::call(it, "File::StatX::statx", vec![filename.clone(), n(0), b])
                })? {
                    st = v;
                }
                if st.truthy() {
                    let bt = pl::method(it, &st, "btime", vec![])?;
                    if bt.truthy() {
                        let bt = pl::method(it, &st, "btime", vec![])?;
                        found_tag(it, "FileCreateDate", bt)?;
                    }
                }
                Ok(())
            })?;
        }
        if !at(&stat_, 2).is_undef() {
            found_tag(it, "FilePermissions", at(&stat_, 2))?;
        }
        // extract more system info if SystemTags option is set
        if !stat_.is_empty() {
            let st = options.get("SystemTags");
            let sys = st.truthy() || (req_all.truthy() && st.is_undef());
            if sys || req.get("fileattributes").truthy() {
                let ops = crate::perl::ops::bit_and;
                let mut attr = vec![ops(&at(&stat_, 2), &n(0xf000)), ops(&at(&stat_, 2), &n(0x0e00))];
                // add Windows file attributes if available
                if pl::str_eq(&os(it), "MSWin32") && !filename.is_undef() && !filename.to_bytes().is_empty() && !pl::str_eq(&filename, "-") {
                    let h = set_warning_ref(it);
                    local_sig_warn(it, h, |it| {
                        if eval_block(it, |it| crate::runtime::require(it, "Win32API::File"))?.is_some() {
                            let wattr;
                            let mut file = [filename.clone()];
                            if method_lv(it, &self_, "EncodeFileName", &mut file, Want::Scalar)?.pop().unwrap_or_default().truthy() {
                                wattr = eval_block(it, |it| pl::call(it, "Win32API::File::GetFileAttributesW", vec![file[0].clone()]))?.unwrap_or_default();
                            } else {
                                wattr = eval_block(it, |it| pl::call(it, "Win32API::File::GetFileAttributes", vec![file[0].clone()]))?.unwrap_or_default();
                            }
                            if !wattr.is_undef() && !pl::num_eq(&wattr, &Value::Int(0xffffffff)) {
                                attr.push(wattr);
                            }
                        }
                        Ok(())
                    })?;
                }
                let sep = it.get_scalar("main::\"");
                found_tag(it, "FileAttributes", join(&sep, &attr))?;
            }
            for (name, idx, key) in [
                ("FileDeviceNumber", 0, "filedevicenumber"),
                ("FileInodeNumber", 1, "fileinodenumber"),
                ("FileHardLinks", 3, "filehardlinks"),
                ("FileUserID", 4, "fileuserid"),
                ("FileGroupID", 5, "filegroupid"),
                ("FileDeviceID", 6, "filedeviceid"),
                ("FileBlockSize", 11, "fileblocksize"),
                ("FileBlockCount", 12, "fileblockcount"),
            ] {
                if sys || req.get(key).truthy() {
                    found_tag(it, name, at(&stat_, idx))?;
                }
            }
        }
        // extract MDItem tags if requested (only on plain files)
        if pl::str_eq(&os(it), "darwin") && !filename.is_undef() && !filename.to_bytes().is_empty() && !file_size.is_undef() {
            let req_mac_os = pl::gt(&req_all, &n(1)) || req.get("macos:").truthy();
            let cr_date = req_mac_os || req.get("filecreatedate").truthy();
            let md_item = req_mac_os || options.get("MDItemTags").truthy() || keys(&req).iter().any(|k| re(r"^mditem", "").is_match(k));
            let xattr = req_mac_os || options.get("XAttrTags").truthy() || keys(&req).iter().any(|k| re(r"^xattr", "").is_match(k));
            if cr_date || md_item || xattr {
                crate::runtime::require(it, "Image::ExifTool::MacOS")?;
                if cr_date {
                    call_void(it, "Image::ExifTool::MacOS::GetFileCreateDate", vec![self_.clone(), filename.clone()])?;
                }
                if md_item && plain_file {
                    call_void(it, "Image::ExifTool::MacOS::ExtractMDItemTags", vec![self_.clone(), filename.clone()])?;
                }
                if xattr {
                    call_void(it, "Image::ExifTool::MacOS::ExtractXAttrTags", vec![self_.clone(), filename.clone()])?;
                }
            }
        }
        // do whatever else we can with directories, then return
        if is_dir || (!at(&stat_, 2).is_undef() && pl::num_eq(&crate::perl::ops::bit_and(&at(&stat_, 2), &n(0o170000)), &n(0o040000))) {
            found_tag(it, "FileType", s("DIR"))?;
            found_tag(it, "FileTypeExtension", s(""))?;
            method_void(it, &self_, "DoneExtract", vec![])?;
            if raf.truthy() {
                method_void(it, &raf, "Close", vec![])?;
            }
            if !save_options.is_empty() {
                self_.set("OPTIONS", save_options.clone());
            }
            if !re_entry.truthy() {
                self_.delete("InExtract");
            }
            return ret(n(1));
        }
        // get list of file types to check
        let tiff_type;
        let no_magic = Value::new_hash();
        let mut recognized_ext = Value::Undef;
        let ext = pl::call(it, "Image::ExifTool::GetFileExtension", vec![realname.clone()])?;
        self_.set("FILE_EXT", ext.clone());
        let magic_number = ghash(it, "Image::ExifTool::magicNumber");
        let module_name = lhash(it, "moduleName");
        let weak_magic = lhash(it, "weakMagic");
        let process_type = lhash(it, "processType");
        // set $recognizedExt if this file type is recognized by extension only
        if !ext.is_undef() && magic_number.get(ext.to_bytes()).is_undef() && !module_name.get(ext.to_bytes()).is_undef() && !module_name.get(ext.to_bytes()).truthy() {
            recognized_ext = ext.clone();
        }
        let mut file_type_list: std::collections::VecDeque<Value> = get_file_type(it, vec![realname.clone()], Want::List)?.into();
        if pl::ge(&fast, &n(4)) {
            if !file_type_list.is_empty() {
                type_ = file_type_list.pop_front().unwrap();
                self_.set("FILE_TYPE", type_.clone());
                method_void(it, &self_, "SetFileType", vec![type_.clone()])?;
            } else {
                method_void(it, &self_, "Error", vec![s("Unknown file type")])?;
            }
            method_void(it, &self_, "DoneExtract", vec![])?;
            break; // don't read the file
        }
        let file_types = garray(it, "Image::ExifTool::fileTypes").items();
        if !file_type_list.is_empty() {
            // add remaining types to end of list so we test them all
            let pat = join(&s("|"), &file_type_list.iter().cloned().collect::<Vec<_>>());
            let r = pl::re_dyn(&[b"^(", &pat.to_bytes()[..], b")$"].concat(), "")?;
            for t in file_types {
                if !r.is_match(&t) {
                    file_type_list.push_back(t);
                }
            }
            tiff_type = self_.get("FILE_EXT");
            if !pl::num_eq(&fast, &n(3)) {
                no_magic.set("MXF", n(1)); // don't do magic number test on MXF or DV files
                no_magic.set("DV", n(1));
            }
        } else {
            // scan through all recognized file types
            file_type_list = file_types.into();
            tiff_type = s("TIFF");
        }
        file_type_list.push_back(s("")); // end of list marker
        // initialize the input file for seeking in binary data
        method_void(it, &raf, "BinMode", vec![])?; // set binary mode before we start reading
        let pos = pl::method(it, &raf, "Tell", vec![])?; // get file position so we can rewind
        // loop through list of file types to test
        let buff: crate::interp::eval::Cell = Rc::new(RefCell::new(Value::Undef));
        let mut err = Value::Undef;
        let dir_info = Value::new_hash();
        dir_info.set("RAF", raf.clone());
        dir_info.set("Base", pos.clone());
        dir_info.set("TestBuff", Value::ScalarRef(buff.clone()));
        let test_len = it.get_scalar("Image::ExifTool::testLen");
        // read start of file for testing
        let raf_read = |it: &mut Interp, len: Value| -> ER<Value> {
            let mut ra = [buff.borrow().clone(), len];
            let r = method_lv(it, &raf, "Read", &mut ra, Want::Scalar)?.pop().unwrap_or_default();
            *buff.borrow_mut() = ra[0].clone();
            Ok(r)
        };
        if raf_read(it, test_len)?.truthy() {
            if !pl::method(it, &raf, "Seek", vec![pos.clone(), n(0)])?.truthy() {
                err = s("Error seeking in file");
            }
        } else {
            err = raf.get("ERROR");
            *buff.borrow_mut() = s("");
        }
        while !err.truthy() {
            let mut unk_header = false;
            type_ = file_type_list.pop_front().unwrap_or_default();
            let b = buff.borrow().clone();
            if type_.truthy() {
                let mn = magic_number.get(type_.to_bytes());
                if mn.truthy() {
                    // do quick test for this file type to avoid loading module unnecessarily
                    let r = pl::re_dyn(&[b"^", &mn.to_bytes()[..]].concat(), "s")?;
                    if !r.is_match(&b) && !no_magic.get(type_.to_bytes()).truthy() {
                        continue;
                    }
                } else {
                    // keep checking for other types if we recognize this file only by extension
                    let m = module_name.get(type_.to_bytes());
                    if !m.is_undef() && !m.truthy() {
                        continue;
                    }
                    if pl::gt(&fast, &n(2)) {
                        continue; // keep checking if we aren't processing the file
                    }
                }
                if weak_magic.get(type_.to_bytes()).truthy() && !recognized_ext.is_undef() {
                    continue;
                }
            } else if type_.is_undef() {
                break;
            } else if recognized_ext.truthy() {
                type_ = recognized_ext.clone(); // set type from recognized file extension only
            } else {
                // last ditch effort to scan past unknown header for JPEG/TIFF
                let Some((c, end)) = match_g(br"(\xff\xd8\xff|MM\0\x2a|II\x2a\0)", &b)? else { continue };
                let m1 = cap(&c, 1);
                type_ = if &m1.to_bytes()[..] == b"\xff\xd8\xff" { s("JPEG") } else { s("TIFF") };
                let skip = n(end as i64 - m1.to_bytes().len() as i64);
                dir_info.set("Base", pl::add(&pos, &skip));
                if !pl::method(it, &raf, "Seek", vec![pl::add(&pos, &skip), n(0)])?.truthy() {
                    err = s("Error seeking in file");
                    break;
                }
                let msg = format!("Processing {}-like data after unknown {}-byte header", type_.to_text(), skip.to_text());
                method_void(it, &self_, "Warn", vec![s(&msg)])?;
                if !self_.get("DOC_NUM").truthy() {
                    unk_header = true;
                }
            }
            // save file type in member variable
            self_.set("FILE_TYPE", type_.clone());
            dir_info.set("Parent", if pl::str_eq(&type_, "TIFF") { tiff_type.clone() } else { type_.clone() });
            // don't process the file when FastScan > 2
            if pl::gt(&fast, &n(2)) && !process_type.get(type_.to_bytes()).truthy() {
                if !(weak_magic.get(type_.to_bytes()).truthy() && (!ext.truthy() || ext.to_bytes() != type_.to_bytes())) {
                    method_void(it, &self_, "SetFileType", vec![dir_info.get("Parent")])?;
                }
                break;
            }
            let mut module = module_name.get(type_.to_bytes());
            if module.is_undef() {
                module = type_.clone();
            }
            let mut func = format!("Process{}", type_.to_text());

            // load module if necessary
            if module.truthy() {
                crate::runtime::require(it, &format!("Image/ExifTool/{}.pm", module.to_text()))?;
                func = format!("Image::ExifTool::{}::{func}", module.to_text());
            } else if pl::str_eq(&module, "0") {
                method_void(it, &self_, "SetFileType", vec![])?;
                method_void(it, &self_, "Warn", vec![s("Unsupported file type")])?;
                break;
            } else {
                // (a sub of this package, as `&$func` finds it here)
                func = format!("Image::ExifTool::{func}");
            }
            viv_array(&self_, "PATH").push(type_.clone()); // save file type in metadata PATH

            // process the file
            let result = match it.call_named(&func, vec![self_.clone(), dir_info.clone()], Want::Scalar) {
                Ok(mut v) => v.pop().unwrap_or_default(),
                // a `last` run outside any loop of the processing sub (perl
                // looks up the call stack) leaves this loop, right here
                Err(crate::interp::eval::Flow::Last(None)) => break,
                Err(e) => return Err(e),
            };

            if let Value::ArrayRef(p) = viv_array(&self_, "PATH") {
                p.borrow_mut().pop();
            }

            if result.truthy() {
                // all done if successful
                if unk_header {
                    method_void(it, &self_, "DeleteTag", vec![s("FileType")])?;
                    method_void(it, &self_, "DeleteTag", vec![s("FileTypeExtension")])?;
                    method_void(it, &self_, "DeleteTag", vec![s("MIMEType")])?;
                    method_void(it, &self_, "VPrint", vec![n(0), s("Reset file type due to unknown header\n")])?;
                }
                break;
            }
            // seek back to try again from the same position in the file
            if !pl::method(it, &raf, "Seek", vec![pos.clone(), n(0)])?.truthy() {
                err = s("Error seeking in file");
            }
        }
        if !err.truthy() && type_.is_undef() && !self_.get("DOC_NUM").truthy() {
            // if we were given a single image with a known type there
            // must be a format error since we couldn't read it, otherwise
            // it is likely we don't support images of this type
            let file_type = or_default(get_file_type(it, vec![realname.clone()], Want::Scalar)?.pop().unwrap_or_default(), s(""));
            let b = buff.borrow().clone();
            if !pl::length(&b).truthy() {
                err = s("File is empty");
            } else {
                let ch = Value::bytes(&b.to_bytes()[..1]);
                let not_ch = [b"[^", &crate::perl::regex::quotemeta(&ch.to_bytes())[..], b"]"].concat();
                if b.to_bytes().len() < 16 || pl::re_dyn(&not_ch, "")?.is_match(&b) {
                    if pl::str_eq(&file_type, "RAW") {
                        err = s("Unsupported RAW file type");
                    } else if file_type.truthy() {
                        err = s("File format error");
                    } else {
                        err = s("Unknown file type");
                    }
                } else {
                    // provide some insight into the content of some corrupted files
                    if self_.get("OPTIONS").get("FastScan").truthy() {
                        err = s("File header is all");
                    } else {
                        let mut num = n(0);
                        loop {
                            if !raf_read(it, n(65536))?.truthy() {
                                num = Value::Undef;
                                break;
                            }
                            let b = buff.borrow().clone();
                            if let Some((_, end)) = match_g(&not_ch, &b)? {
                                num = pl::add(&num, &n(end as i64 - 1));
                                break;
                            }
                            num = pl::add(&num, &pl::length(&b));
                        }
                        if num.truthy() {
                            let fs = pl::call(it, "Image::ExifTool::ConvertFileSize", vec![num])?;
                            err = pl::concat(&s("First "), &pl::concat(&fs, &s(" of file is")));
                        } else {
                            err = s("Entire file is");
                        }
                    }
                    let tail = if pl::str_eq(&ch, "\0") {
                        s(" binary zeros")
                    } else if pl::str_eq(&ch, " ") {
                        s(" ASCII spaces")
                    } else if re(r"[a-zA-Z0-9]", "").is_match(&ch) {
                        pl::concat(&s(" ASCII '"), &pl::concat(&ch, &s("' characters")))
                    } else {
                        pl::sprintf(" binary 0x%.2x's", &[n(ch.to_bytes()[0] as i64)])
                    };
                    err = pl::concat(&err, &tail);
                }
            }
        }
        if err.truthy() {
            method_void(it, &self_, "Error", vec![err.clone()])?;
        } else if self::options(it, vec![self_.clone(), s("ScanForXMP")], Want::Scalar)?.pop().unwrap_or_default().truthy() && (type_.is_undef() || (!fast.truthy() && !self_.get("FoundXMP").truthy())) {
            // scan for XMP
            method_void(it, &raf, "Seek", vec![pos.clone(), n(0)])?;
            crate::runtime::require(it, "Image::ExifTool::XMP")?;
            if pl::call(it, "Image::ExifTool::XMP::ScanForXMP", vec![self_.clone(), raf.clone()])?.truthy() {
                type_ = s("");
            }
        }
        // extract binary EXIF data block only if requested
        let ed = self_.get("EXIF_DATA");
        if !ed.is_undef() && pl::gt(&pl::length(&ed), &n(16)) && (req.get("exif").truthy() || (self_.get("TAGS_FROM_FILE").truthy() && !viv(&self_, "EXCL_TAG_LOOKUP").get("exif").truthy())) {
            found_tag(it, "EXIF", self_.get("EXIF_DATA"))?;
        }
        if !re_entry.truthy() {
            self_.set("PATH", Value::new_array(vec![])); // reset PATH
            method_void(it, &self_, "DoneExtract", vec![])?;
            // do our HTML dump if requested
            let hd = self_.get("HTML_DUMP");
            if hd.truthy() {
                method_void(it, &raf, "Seek", vec![n(0), n(2)])?; // seek to end of file
                let t = pl::method(it, &raf, "Tell", vec![])?;
                method_void(it, &hd, "FinishTiffDump", vec![self_.clone(), t])?;
                let mut pos = options.get("HtmlDumpBase");
                if pos.is_undef() {
                    pos = or_default(self_.get("FIRST_EXIF_POS"), n(0));
                }
                let mut data_pt = if !self_.get("EXIF_DATA").is_undef() {
                    match &self_ {
                        Value::HashRef(h) => Value::LvRef(Rc::new(crate::interp::eval::Lv::HElem(h.clone(), Rc::from(&b"EXIF_DATA"[..])))),
                        _ => Value::Undef,
                    }
                } else {
                    Value::Undef
                };
                if !self_.get("EXIF_POS").is_undef() && !pl::num_eq(&pos, &self_.get("EXIF_POS")) {
                    data_pt = Value::Undef;
                }
                if self_.get("ExtendedEXIF").truthy() {
                    data_pt = Value::Undef; // can't use EXIF block if not contiguous
                }
                let fname = self_.get("FILENAME");
                let title = if fname.truthy() { pl::concat(&s("HTML Dump ("), &pl::concat(&fname, &s(")"))) } else { s("HTML Dump") };
                let success = pl::method(it, &hd, "Print", vec![raf.clone(), data_pt, pos, options.get("TextOut"), options.get("HtmlDump"), title])?;
                if pl::lt(&success, &n(0)) {
                    let e = self_.get("HTML_DUMP").get("ERROR");
                    method_void(it, &self_, "Warn", vec![pl::concat(&s("Error reading "), &e)])?;
                }
            }
        }
        if filename.truthy() {
            method_void(it, &raf, "Close", vec![])?; // close the file if we opened it
            // process the resource fork as an embedded file on Mac filesystems
            if rsize.truthy() && options.get("ExtractEmbedded").truthy() {
                // local *RESOURCE_FILE;
                let fh = glob_ref(it, "RESOURCE_FILE")?;
                if pl::method(it, &self_, "Open", vec![fh.clone(), pl::concat(&filename, &s("/..namedfork/rsrc"))])?.truthy() {
                    self_.set("DOC_NUM", pl::add(&self_.get("DOC_COUNT"), &n(1)));
                    self_.set("IN_RESOURCE", n(1));
                    let opts = Value::new_hash();
                    opts.set("ReEntry", n(1));
                    extract_info(it, vec![self_.clone(), fh, opts], Want::Void)?;
                    perl_snippet(it, "close RESOURCE_FILE", &[], Want::Void)?;
                    self_.delete("IN_RESOURCE");
                } else {
                    method_void(it, &self_, "Warn", vec![s("Error opening resource fork")])?;
                }
            }
        }
        break; // (loop was a cheap "goto")
    }

    // Note: This should be the only tag generated after BuildCompositeTags,
    // and as such it can't be used in user-defined Composite tags
    if !start_time.is_empty() {
        let t = pl::call(it, "Time::HiRes::tv_interval", vec![Value::new_array(start_time.clone())])?;
        found_tag(it, "ProcessingTime", t)?;
    }

    // add numbers to warnings with multiple occurrences
    let was_warned = self_.get("WAS_WARNED");
    if !was_warned.is_empty() {
        let (mut tag, val) = (s("Warning"), self_.get("VALUE"));
        let mut i = 1;
        while val.get(tag.to_bytes()).truthy() {
            let nn = viv(&self_, "WAS_WARNED").get(val.get(tag.to_bytes()).to_bytes());
            if nn.truthy() && pl::gt(&nn, &n(1)) {
                let v = pl::concat(&val.get(tag.to_bytes()), &pl::concat(&s(" [x"), &pl::concat(&nn, &s("]"))));
                val.set(tag.to_bytes(), v);
            }
            tag = s(&format!("Warning ({i})"));
            i += 1;
        }
    }
    // restore original options
    if !save_options.is_empty() {
        self_.set("OPTIONS", save_options);
    }

    if re_entry.truthy() {
        // restore necessary members when exiting re-entrant code
        for k in keys(&re_entry) {
            self_.set(k.to_bytes(), re_entry.get(k.to_bytes()));
        }
        call_void(it, "Image::ExifTool::SetByteOrder", vec![save_order])?;
    } else {
        // call cleanup routines if necessary
        let cleanup = self_.get("Cleanup");
        if cleanup.truthy() {
            pl::at(3215);
            for f in pl::aderef(&cleanup)? {
                pl::at(3215);
                call_code(it, &f, vec![self_.clone()], Want::Void)?;
            }
            self_.delete("Cleanup");
        }
        self_.delete("InExtract");
    }

    // ($type may be undef without an Error when processing sub-documents)
    if type_.is_undef() || viv(&self_, "VALUE").exists("Error") {
        return ret(n(0));
    }
    ret(n(1))
}

/// `@{$$h{KEY}}` as an lvalue: the array, created when undefined.
fn viv_array(h: &Value, key: &str) -> Value {
    if h.is_hash() {
        h.get_or_new_array(key)
    } else {
        Value::Undef
    }
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
    it.define_native("Image::ExifTool::GetValue", get_value);
    it.define_native("Image::ExifTool::BuildCompositeTags", build_composite_tags);
    it.define_native("Image::ExifTool::Options", options);
    it.define_native("Image::ExifTool::ExtractInfo", extract_info);
}
