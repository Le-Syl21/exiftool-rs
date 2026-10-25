//! Image::ExifTool (ExifTool.pm), ported sub by sub (see docs/port/PORTING.md).
//! This part: the subs from line 7255 to the end (`DirStart` ...
//! `ProcessBinaryData`), the heart of the read pipeline.

use std::cell::RefCell;
use std::rc::Rc;

use crate::interp::eval::{substr_range, Flow, ER};
use crate::interp::{Binding, Interp, Scope, Want};
use crate::perl::value::Hash;
use crate::perl::Value;
use crate::pl::{self, arg, cap, re, ret};
use crate::runtime;

const ET: &str = "Image::ExifTool";

fn n(i: i64) -> Value {
    Value::Int(i)
}

// ---- local helpers (Perl builtins and idioms with no helper in crate::pl) ----

/// A package hash of ExifTool.pm (`%fileTypeLookup`, `%specialTags`...).
fn pkg_hash(it: &mut Interp, name: &str) -> Value {
    match it.global(b'%', &format!("{ET}::{name}")) {
        Binding::Hash(h) => Value::HashRef(h),
        _ => unreachable!(),
    }
}

/// A package array of ExifTool.pm (`@tableOrder`...).
fn pkg_array(it: &mut Interp, name: &str) -> Value {
    match it.global(b'@', &format!("{ET}::{name}")) {
        Binding::Array(a) => Value::ArrayRef(a),
        _ => unreachable!(),
    }
}

/// A file lexical hash of ExifTool.pm (`my %formatSize`...).
fn lex_hash(it: &mut Interp, name: &str) -> Value {
    runtime::lex_hash(it, ET, name)
}

/// `substr(STR, OFF, LEN)` as an rvalue (warns and gives undef outside the
/// string, as perl does).
fn substr(it: &mut Interp, s: &Value, off: &Value, len: Option<&Value>) -> ER<Value> {
    let b = s.to_bytes();
    let l = len.map(|v| if v.is_undef() { 0 } else { v.to_i64() });
    Ok(match substr_range(b.len(), off.to_i64(), l) {
        Some((x, y)) => Value::bytes(&b[x..y]),
        None => {
            it.warn(Value::str("substr outside of string at (eval) line 1.\n"))?;
            Value::Undef
        }
    })
}

/// `length $x` as a number (undef counts as 0 where the Perl compares it).
fn len_of(v: &Value) -> Value {
    pl::length(v)
}

/// `uc $x`
fn uc(v: &Value) -> Value {
    if v.is_undef() {
        return pl::s("");
    }
    Value::bytes(v.to_bytes().to_ascii_uppercase())
}

/// `lc $x`
fn lc(v: &Value) -> Value {
    if v.is_undef() {
        return pl::s("");
    }
    Value::bytes(v.to_bytes().to_ascii_lowercase())
}

/// `ucfirst $x`
fn ucfirst(v: &Value) -> Value {
    if v.is_undef() {
        return pl::s("");
    }
    let mut o = v.to_bytes().to_vec();
    if let Some(f) = o.first_mut() {
        *f = f.to_ascii_uppercase();
    }
    Value::bytes(o)
}

/// `ref $v` (a blessed reference gives its class).
fn ref_of(v: &Value) -> String {
    if let Value::HashRef(h) = v {
        if let Some(c) = &h.borrow().blessed {
            return c.to_string();
        }
    }
    v.ref_type().to_string()
}

/// `$a ne $b` / `$a eq $b` between two values.
fn seq(a: &Value, b: &Value) -> bool {
    a.to_bytes() == b.to_bytes()
}

/// `$obj->Method(ARGS)` in a given context.
fn meth(it: &mut Interp, obj: &Value, name: &str, args: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let mut a = vec![obj.clone()];
    a.extend(args);
    it.call_method(obj, name, a, want)
}

/// `&$code(ARGS)`: a code reference, or a sub name (`no strict 'refs'`).
fn call_code(it: &mut Interp, code: &Value, args: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    match code {
        Value::Code(c) => {
            let c = c.clone();
            it.call(&c, args, want)
        }
        _ => {
            let name = code.to_text();
            it.call_named(&name, args, want)
        }
    }
}

/// A scalar result in scalar context (the last value, as perl's `fit`).
fn scalar_of(v: Vec<Value>) -> Value {
    v.into_iter().last().unwrap_or_default()
}

/// `\&Image::ExifTool::Name`
fn code_ref(it: &mut Interp, full: &str) -> Value {
    Value::Code(it.sub_ref(full))
}

/// Run a Perl snippet for a builtin with no helper here (`print $fh`,
/// method calls whose arguments are written back through `@_`, `pos`):
/// the snippet sees exactly the lexicals given. `$@` is left as it was, and
/// a die or exit inside propagates as it would from the Perl.
fn snippet(it: &mut Interp, code: &str, scope: &Rc<Scope>, want: Want) -> ER<Vec<Value>> {
    let saved_at = it.get_scalar("main::@");
    let had_exit = it.exit_code.is_some();
    let r = it.eval_string(code.as_bytes(), ET, scope, want);
    it.set_scalar("main::@", saved_at);
    match r {
        Ok(v) => Ok(v),
        Err(e) => {
            if !had_exit {
                if let Some(c) = it.exit_code.take() {
                    return Err(Flow::Exit(c));
                }
            }
            Err(Flow::Die(e))
        }
    }
}

/// `print $out LIST`
fn print_to(it: &mut Interp, out: &Value, list: Vec<Value>) -> ER<()> {
    let scope = Scope::new(None);
    scope.scalar("out", out.clone());
    scope.array("args", list);
    snippet(it, "print $out @args", &scope, Want::Void)?;
    Ok(())
}

/// The lexical scope an eval site of ExifTool.pm sees above its sub: the
/// file's own `my` variables.
fn file_scope(it: &mut Interp) -> Rc<Scope> {
    const LEXICALS: &[(u8, &str)] = &[
        (b'@', "writeTypes"),
        (b'%', "writeTypes"),
        (b'%', "onlyWriteFile"),
        (b'%', "createTypes"),
        (b'%', "fileTypeExt"),
        (b'%', "fileDescription"),
        (b'%', "moduleName"),
        (b'%', "weakMagic"),
        (b'%', "processType"),
        (b'%', "compactOpt"),
        (b'%', "xmpShorthandOpt"),
        (b'@', "availableOptions"),
        (b'@', "defaultWriteGroups"),
        (b'%', "allGroupsExifTool"),
        (b'%', "geoInfo"),
        (b'$', "purgeFlag"),
        (b'@', "purgeTags"),
        (b'%', "systemTagsNotes"),
        (b'%', "compositeID"),
        (b'$', "k32GetFullPathName"),
        (b'$', "k32CreateDir"),
        (b'$', "k32GetFileTime"),
        (b'%', "isPC"),
        (b'%', "unpackMotorola"),
        (b'%', "unpackIntel"),
        (b'%', "unpackRev"),
        (b'$', "ratNumer"),
        (b'$', "ratDenom"),
        (b'%', "formatSize"),
        (b'%', "readValueProc"),
        (b'%', "isRational"),
        (b'%', "markerLenBytes"),
    ];
    let scope = Scope::new(None);
    for (sigil, name) in LEXICALS {
        let b = runtime::lexical(it, *sigil, ET, name);
        scope.declare(*sigil, name, b);
    }
    scope
}

/// Bind a lexical scalar of the sub in an eval scope.
fn bind(scope: &Rc<Scope>, name: &str, cell: &Rc<RefCell<Value>>) {
    scope.declare(b'$', name, Binding::Scalar(cell.clone()));
}

fn cell(v: Value) -> Rc<RefCell<Value>> {
    Rc::new(RefCell::new(v))
}

/// `local $SIG{'__WARN__'} = \&SetWarning;`: returns what to restore.
fn local_sig_warn(it: &mut Interp) -> (Rc<RefCell<Hash>>, Option<Value>) {
    let sig = match it.global(b'%', "main::SIG") {
        Binding::Hash(h) => h,
        _ => unreachable!(),
    };
    let old = sig.borrow().get(b"__WARN__").cloned();
    let handler = code_ref(it, "Image::ExifTool::SetWarning");
    sig.borrow_mut().insert(b"__WARN__", handler);
    (sig, old)
}

fn restore_sig_warn(saved: (Rc<RefCell<Hash>>, Option<Value>)) {
    let (sig, old) = saved;
    match old {
        Some(v) => sig.borrow_mut().insert(b"__WARN__", v),
        None => {
            sig.borrow_mut().remove(b"__WARN__");
        }
    }
}

/// `local $_;`: a new undefined `$_` until the guard is restored.
fn local_topic(it: &mut Interp) -> Binding {
    let old = it.global(b'$', "main::_");
    it.set_global(b'$', "main::_", Binding::Scalar(cell(Value::Undef)));
    old
}

fn restore_topic(it: &mut Interp, old: Binding) {
    it.set_global(b'$', "main::_", old);
}

/// `$evalWarning` (a package variable of ExifTool.pm).
fn eval_warning(it: &mut Interp) -> Value {
    it.get_scalar("Image::ExifTool::evalWarning")
}

fn set_eval_warning(it: &mut Interp, v: Value) {
    it.set_scalar("Image::ExifTool::evalWarning", v);
}

/// `eval $code` at an eval site: `$@` is set as perl does; the result is
/// undef (or the empty list) on error.
fn eval_site(it: &mut Interp, code: &Value, scope: &Rc<Scope>, want: Want) -> Vec<Value> {
    match it.eval_string(&code.to_bytes(), ET, scope, want) {
        Ok(v) => v,
        Err(_) => {
            if want == Want::List {
                vec![]
            } else {
                vec![Value::Undef]
            }
        }
    }
}

/// `$@`
fn eval_error(it: &mut Interp) -> Value {
    it.get_scalar("main::@")
}

// ---- the subs ----

/// Adjust directory start position
/// Inputs: 0) dirInfo ref, 1) start offset
///         2) Base for offsets (relative to DataPos, defaults to absolute Base of 0)
pub fn dir_start(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (dir_info, start, base) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    dir_info.set("DirStart", start.clone());
    dir_info.set("DirLen", pl::sub(&dir_info.get("DirLen"), &start));
    if !base.is_undef() {
        dir_info.set("Base", pl::add(&dir_info.get("DataPos"), &base));
        let v = pl::neg(&base); // (relative to Base!)
        dir_info.set("DataPos", v.clone());
        return ret(v);
    }
    ret(pl::s(""))
}

/// Extract metadata from a jpg image
/// Inputs: 0) ExifTool object reference, 1) dirInfo ref with RAF set
/// Returns: 1 on success, 0 if this wasn't a valid JPEG file
pub fn process_exv(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (self_, dir_info) = (arg(&a, 0), arg(&a, 1));
    meth(it, &self_, "ProcessJPEG", vec![dir_info], want)
}

/// Process EXIF file
/// Inputs/Returns: same as ProcessTIFF
pub fn process_exif(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (self_, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    meth(it, &self_, "ProcessTIFF", vec![dir_info, tag_table_ptr], want)
}

/// Process TIFF data (wrapper for DoProcessTIFF to allow re-entry)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) optional tag table ref
/// Returns: 1 if this looked like a valid EXIF block, 0 otherwise, or -1 on write error
pub fn process_tiff(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let exif_data = self_.get("EXIF_DATA");
    let exif_pos = self_.get("EXIF_POS");
    let rtn_val = pl::method(it, &self_, "DoProcessTIFF", vec![dir_info, tag_table_ptr])?;
    // restore original EXIF information (in case ProcessTIFF is nested)
    if !exif_data.is_undef() {
        self_.set("EXIF_DATA", exif_data);
        self_.set("EXIF_POS", exif_pos);
    }
    ret(rtn_val)
}

/// Process TIFF as a sub-document
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) optional tag table ref
/// Returns: 1 if this looked like a valid EXIF block, 0 otherwise, or -1 on write error
pub fn process_sub_tiff(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let count = pl::add(&self_.get("DOC_COUNT"), &n(1));
    self_.set("DOC_COUNT", count.clone());
    self_.set("DOC_NUM", count);
    let rtn_val = pl::method(it, &self_, "ProcessTIFF", vec![dir_info, tag_table_ptr])?;
    self_.delete("DOC_NUM");
    ret(rtn_val)
}

/// Return list of tag table keys (ignoring special keys)
/// Inputs: 0) reference to tag table
/// Returns: List of table keys (unsorted)
pub fn tag_table_keys(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let tag_table_ptr = arg(&a, 0);
    let key_list = tag_table_keys_list(it, &tag_table_ptr);
    if want == Want::List {
        Ok(key_list)
    } else {
        ret(n(key_list.len() as i64))
    }
}

fn tag_table_keys_list(it: &mut Interp, tag_table_ptr: &Value) -> Vec<Value> {
    let special_tags = pkg_hash(it, "specialTags");
    let mut key_list = Vec::new();
    for k in tag_table_ptr.keys() {
        if !special_tags.get(&k).truthy() {
            key_list.push(Value::Str(k));
        }
    }
    key_list
}

/// Get Metadata path
/// Inputs: 0) ExifTool object ref
/// Return: Metadata path string
pub fn metadata_path(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let self_ = arg(&a, 0);
    let parts: Vec<Vec<u8>> = self_.get("PATH").items().iter().map(|v| v.to_bytes().to_vec()).collect();
    ret(Value::bytes(parts.join(&b'-')))
}

/// Get standardized file extension
/// Inputs: 0) file name
/// Returns: standardized extension (all uppercase), or undefined if no extension
pub fn get_file_extension(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let filename = arg(&a, 0);
    let mut file_ext = Value::Undef;
    if filename.truthy() {
        if let Some(m) = re(r"^.*\.([^.]+)$", "s").m(&filename) {
            file_ext = uc(&cap(&m, 1)); // change extension to upper case
            // convert TIF extension to TIFF because we use the
            // extension for the file type tag of TIFF images
            if pl::str_eq(&file_ext, "TIF") {
                file_ext = pl::s("TIFF");
            }
        }
    }
    ret(file_ext)
}

/// Get list of tag information hashes for given tag ID
/// Inputs: 0) Tag table reference, 1) tag ID
/// Returns: Array of tag information references
/// Notes: Generates tagInfo hash if necessary
pub fn get_tag_info_list(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let list = tag_info_list(it, &arg(&a, 0), &arg(&a, 1))?;
    if want == Want::List {
        Ok(list.0)
    } else if list.1 {
        // (return @$tagInfo in scalar context: the count)
        ret(n(list.0.len() as i64))
    } else {
        Ok(vec![list.0.into_iter().last().unwrap_or_default()])
    }
}

/// GetTagInfoList's list, and whether it was `@$tagInfo` (an array).
fn tag_info_list(it: &mut Interp, tag_table_ptr: &Value, tag_id: &Value) -> ER<(Vec<Value>, bool)> {
    let key = tag_id.to_bytes();
    let tag_info = tag_table_ptr.get(&key);
    let special_tags = pkg_hash(it, "specialTags");
    if special_tags.get(&key).truthy() {
        // (hopefully this won't happen)
        let msg = pl::concat(&pl::concat(&pl::concat(&pl::s("Tag "), tag_id), &pl::s(" conflicts with internal ExifTool variable in ")), &pl::concat(&tag_table_ptr.get("TABLE_NAME"), &pl::s("\n")));
        it.warn(msg)?;
    } else if ref_of(&tag_info) == "HASH" {
        return Ok((vec![tag_info], false));
    } else if ref_of(&tag_info) == "ARRAY" {
        return Ok((tag_info.items(), true));
    } else if tag_info.truthy() {
        // create hash with name
        let h = Value::new_hash();
        h.set("Name", tag_info);
        tag_table_ptr.set(&key, h.clone());
        return Ok((vec![h], false));
    }
    Ok((vec![], false))
}

/// Make current directory the priority directory if not set already
/// Inputs: 0) ExifTool object reference
pub fn set_priority_dir(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let self_ = arg(&a, 0);
    let cur = self_.get("PRIORITY_DIR");
    if !cur.truthy() {
        let d = self_.get("DIR_NAME");
        self_.set("PRIORITY_DIR", d.clone());
        return ret(d);
    }
    ret(cur)
}

/// Set family 0 or 1 group name specific to this tag instance
/// Inputs: 0) ExifTool ref, 1) tag key, 2) group name, 3) family (default 1)
pub fn set_group(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, tag_key, extra, fam) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    let k = if !fam.is_undef() { pl::concat(&pl::s("G"), &fam) } else { pl::s("G1") };
    self_.get_or_new_hash("TAG_EXTRA").get_or_new_hash(tag_key.to_bytes()).set(k.to_bytes(), extra.clone());
    ret(extra)
}

/// Delete specified tag
/// Inputs: 0) ExifTool object ref, 1) tag key
pub fn delete_tag(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, tag) = (arg(&a, 0), arg(&a, 1));
    let k = tag.to_bytes();
    self_.get_or_new_hash("VALUE").delete(&k);
    self_.get_or_new_hash("FILE_ORDER").delete(&k);
    self_.get_or_new_hash("TAG_INFO").delete(&k);
    self_.get_or_new_hash("TAG_EXTRA").delete(&k);
    self_.get_or_new_hash("PRIORITY").delete(&k);
    ret(self_.get_or_new_hash("BOTH").delete(&k))
}

/// Escape all elements of a value
/// Inputs: 0) value, 1) escape proc
pub fn do_escape(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (v, proc_) = (arg(&a, 0), arg(&a, 1));
    if let Some(new) = do_escape_value(it, &v, &proc_)? {
        it.set_arg(0, new)?;
    }
    Ok(vec![])
}

/// DoEscape on one value: the new value when it is a plain scalar (which
/// the caller writes back where `$_[0]` aliases).
fn do_escape_value(it: &mut Interp, v: &Value, proc_: &Value) -> ER<Option<Value>> {
    if !v.is_ref() {
        return Ok(Some(scalar_of(call_code(it, proc_, vec![v.clone()], Want::Scalar)?)));
    } else if ref_of(v) == "ARRAY" {
        let len = v.len();
        for i in 0..len {
            let val = v.at(i as i64);
            if let Some(new) = do_escape_value(it, &val, proc_)? {
                v.set_at(i, new);
            }
        }
    } else if ref_of(v) == "HASH" {
        for key in v.keys() {
            let val = v.get(&key);
            if let Some(new) = do_escape_value(it, &val, proc_)? {
                v.set(&key, new);
            }
        }
    }
    Ok(None)
}

/// Set the FileType and MIMEType tags
/// Inputs: 0) ExifTool object reference
///         1) Optional file type (uses FILE_TYPE if not specified)
///         2) Optional MIME type (uses our lookup if not specified)
///         3) Optional recommended extension (converted to lower case; uses FileType if undef)
/// Notes:  Will NOT set file type twice (subsequent calls ignored)
pub fn set_file_type(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, mut file_type, mut mime_type, mut norm_ext) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    // use only the first FileType set if called again for the main document
    if !(self_.get("FileType").truthy() && !self_.get("DOC_NUM").truthy()) {
        let base_type = self_.get("FILE_TYPE");
        let ext = self_.get("FILE_EXT");
        if !file_type.truthy() {
            file_type = base_type.clone();
        }
        let file_type_lookup = pkg_hash(it, "fileTypeLookup");
        // handle sub-types which are identified by extension
        if !ext.is_undef() && !seq(&ext, &file_type) && !self_.get("DOC_NUM").truthy() {
            let (f, e) = (file_type_lookup.get(file_type.to_bytes()), file_type_lookup.get(ext.to_bytes()));
            if ref_of(&f) == "ARRAY" && ref_of(&e) == "ARRAY" && seq(&f.at(0), &e.at(0)) {
                // make sure $fileType was a root type and not another sub-type
                if seq(&f.at(0), &file_type) || !file_type_lookup.get(f.at(0).to_bytes()).truthy() {
                    file_type = ext.clone();
                }
            }
        }
        let mime_type_h = pkg_hash(it, "mimeType");
        if !mime_type.truthy() {
            mime_type = mime_type_h.get(file_type.to_bytes());
        }
        // use base file type if necessary (except if 'TIFF', which is a special case)
        if !(mime_type.truthy() || pl::str_eq(&base_type, "TIFF")) {
            mime_type = mime_type_h.get(base_type.to_bytes());
        }
        if norm_ext.is_undef() {
            norm_ext = lex_hash(it, "fileTypeExt").get(file_type.to_bytes());
            if norm_ext.is_undef() {
                norm_ext = file_type.clone();
            }
        }
        // ($$self{FileType} is the file type of the main document)
        if !self_.get("DOC_NUM").truthy() {
            self_.set("FileType", file_type.clone());
        }
        found_tag_call(it, &self_, vec![pl::s("FileType"), file_type])?;
        found_tag_call(it, &self_, vec![pl::s("FileTypeExtension"), uc(&norm_ext)])?;
        let m = if mime_type.truthy() { mime_type } else { pl::s("application/unknown") };
        let r = found_tag_call(it, &self_, vec![pl::s("MIMEType"), m])?;
        return ret(r);
    }
    ret(pl::s(""))
}

/// `$self->FoundTag(...)` (a method call, so a subclass may override it).
fn found_tag_call(it: &mut Interp, self_: &Value, args: Vec<Value>) -> ER<Value> {
    pl::method(it, self_, "FoundTag", args)
}

/// Override the FileType and MIMEType tags
/// Inputs: 0) ExifTool object ref, 1) file type, 2) MIME type, 3) normal extension (lower case)
/// Notes:  does nothing if FileType was not previously defined (ie. when writing)
pub fn override_file_type(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, file_type, mut mime_type, mut norm_ext) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    let value = self_.get("VALUE");
    if !value.get("FileType").is_undef() && !seq(&file_type, &value.get("FileType")) {
        self_.set("FileType", file_type.clone());
        value.set("FileType", file_type.clone());
        if norm_ext.is_undef() {
            norm_ext = lex_hash(it, "fileTypeExt").get(file_type.to_bytes());
            if norm_ext.is_undef() {
                norm_ext = file_type.clone();
            }
        }
        value.set("FileTypeExtension", uc(&norm_ext));
        if !mime_type.truthy() {
            mime_type = pkg_hash(it, "mimeType").get(file_type.to_bytes());
        }
        if mime_type.truthy() {
            value.set("MIMEType", mime_type.clone());
        }
        if self_.path(&["OPTIONS", "Verbose"]).truthy() {
            let indent = self_.get("INDENT");
            let line = |parts: &[&Value]| {
                let mut v = Vec::new();
                for p in parts {
                    v.extend_from_slice(&p.to_bytes());
                }
                Value::bytes(v)
            };
            let s1 = line(&[&indent, &pl::s("FileType [override] = "), &file_type, &pl::s("\n")]);
            vprint(it, &self_, &n(0), vec![s1])?;
            let s2 = line(&[&indent, &pl::s("FileTypeExtension [override] = "), &value.get("FileTypeExtension"), &pl::s("\n")]);
            vprint(it, &self_, &n(0), vec![s2])?;
            if mime_type.truthy() {
                let s3 = line(&[&indent, &pl::s("MIMEType [override] = "), &mime_type, &pl::s("\n")]);
                vprint(it, &self_, &n(0), vec![s3])?;
            }
        }
    }
    Ok(vec![])
}

/// Modify the value of the MIMEType tag
/// Inputs: 0) ExifTool object reference, 1) file or MIME type
/// Notes: combines existing type with new type: ie) a/b + c/d => c/b-d
pub fn modify_mime_type(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (self_, mut mime) = (arg(&a, 0), arg(&a, 1));
    if !re("/", "").is_match(&mime) {
        mime = pkg_hash(it, "mimeType").get(mime.to_bytes());
        if !mime.truthy() {
            return pl::ret_nothing(want);
        }
    }
    let old = self_.path(&["VALUE", "MIMEType"]);
    if !old.is_undef() {
        let slash = re("/", "");
        let p = pl::split(&slash, &old, 0);
        let b = arg(&p, 1);
        let q = pl::split(&slash, &mime, 0);
        let (c, mut d) = (arg(&q, 0), arg(&q, 1));
        re("^x-", "").subst(&mut d, false, |_| Vec::new());
        let mut s = c.to_bytes().to_vec();
        s.push(b'/');
        s.extend_from_slice(&b.to_bytes());
        s.push(b'-');
        s.extend_from_slice(&d.to_bytes());
        self_.get_or_new_hash("VALUE").set("MIMEType", Value::bytes(&s));
        let mut msg = b"  Modified MIMEType = ".to_vec();
        msg.extend_from_slice(&s);
        msg.push(b'\n');
        return ret(scalar_of(vprint(it, &self_, &n(0), vec![Value::bytes(msg)])?));
    }
    ret(found_tag_call(it, &self_, vec![pl::s("MIMEType"), mime])?)
}

/// Print verbose output
/// Inputs: 0) ExifTool ref, 1) verbose level (prints if level > this), 2-N) print args
pub fn v_print(it: &mut Interp, mut a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let self_ = if a.is_empty() { Value::Undef } else { a.remove(0) };
    let level = if a.is_empty() { Value::Undef } else { a.remove(0) };
    vprint(it, &self_, &level, a)
}

fn vprint(it: &mut Interp, self_: &Value, level: &Value, args: Vec<Value>) -> ER<Vec<Value>> {
    let verbose = self_.path(&["OPTIONS", "Verbose"]);
    if verbose.truthy() && pl::gt(&verbose, level) {
        let out = self_.path(&["OPTIONS", "TextOut"]);
        let last = args.last().cloned().unwrap_or_default();
        print_to(it, &out, args)?;
        if !re(r"\n$", "").is_match(&last) {
            print_to(it, &out, vec![pl::s("\n")])?;
        }
        return ret(n(1));
    }
    ret(pl::s(""))
}

/// Print verbose directory information
/// Inputs: 0) ExifTool object reference, 1) directory name or dirInfo ref
///         2) number of entries in directory (or 0 if unknown)
///         3) optional size of directory in bytes, 4) optional byte order for -v3 output
pub fn verbose_dir(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (self_, mut name, entries, mut size, byte_order) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4));
    let verbose = self_.path(&["OPTIONS", "Verbose"]);
    if !verbose.truthy() {
        return pl::ret_nothing(want);
    }
    if ref_of(&name) == "HASH" {
        if !size.truthy() {
            size = name.get("DirLen");
        }
        let nm = name.get("Name");
        name = if nm.truthy() { nm } else { name.get("DirName") };
    }
    let indent = substr(it, &self_.get("INDENT"), &n(0), Some(&n(-2)))?;
    let out = self_.path(&["OPTIONS", "TextOut"]);
    let mut str_ = if entries.truthy() || (!entries.is_undef() && !size.truthy()) {
        pl::concat(&pl::concat(&pl::s(" with "), &entries), &pl::s(" entries"))
    } else {
        pl::s("")
    };
    if size.truthy() {
        str_ = pl::concat(&str_, &pl::concat(&pl::concat(&pl::s(", "), &size), &pl::s(" bytes")));
    }
    if byte_order.truthy() && pl::gt(&self_.path(&["OPTIONS", "Verbose"]), &n(2)) {
        let bo = pl::call(it, "Image::ExifTool::GetByteOrder", vec![])?;
        str_ = pl::concat(&str_, &pl::s(if pl::str_eq(&bo, "II") { ", Little-endian" } else { ", Big-endian" }));
    }
    let mut line = indent.to_bytes().to_vec();
    line.extend_from_slice(b"+ [");
    line.extend_from_slice(&name.to_bytes());
    line.extend_from_slice(b" directory");
    line.extend_from_slice(&str_.to_bytes());
    line.extend_from_slice(b"]\n");
    print_to(it, &out, vec![Value::bytes(line)])?;
    ret(n(1))
}

/// Verbose dump
/// Inputs: 0) ExifTool ref, 1) data ref, 2-N) HexDump options
pub fn verbose_dump(it: &mut Interp, mut a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let self_ = if a.is_empty() { Value::Undef } else { a.remove(0) };
    let data_pt = if a.is_empty() { Value::Undef } else { a.remove(0) };
    let verbose = self_.path(&["OPTIONS", "Verbose"]);
    if verbose.truthy() && pl::gt(&verbose, &n(2)) {
        let max_len = if pl::lt(&verbose, &n(4)) {
            n(96)
        } else if pl::lt(&verbose, &n(5)) {
            n(2048)
        } else {
            Value::Undef
        };
        let mut parms = Hash::default();
        parms.insert(b"Prefix", self_.get("INDENT"));
        parms.insert(b"Out", self_.path(&["OPTIONS", "TextOut"]));
        parms.insert(b"MaxLen", max_len);
        let mut args = vec![data_pt, Value::Undef];
        for (k, v) in parms.iter() {
            args.push(Value::Str(k.clone()));
            args.push(v.clone());
        }
        args.extend(a);
        return pl::call_list(it, "Image::ExifTool::HexDump", args);
    }
    ret(pl::s(""))
}

/// Print data in hex
/// Inputs: 0) data
/// Returns: hex string
/// (this is a convenience function for use in debugging PrintConv statements)
pub fn print_hex(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let val = arg(&a, 0);
    let len = pl::length(&val).to_i64().max(0) as usize;
    let tmpl = b"H2".repeat(len);
    let parts = match crate::perl::pack::unpack(&tmpl, &val.to_bytes()) {
        Ok(p) => p,
        Err(e) => return crate::interp::eval::die(e),
    };
    let _ = it;
    let s: Vec<Vec<u8>> = parts.iter().map(|v| v.to_bytes().to_vec()).collect();
    ret(Value::bytes(s.join(&b' ')))
}

/// Extract binary data from file
/// 0) ExifTool object reference, 1) offset, 2) length, 3) tag name if conditional
/// Returns: binary data, or undef on error
/// Notes: Returns "Binary data #### bytes" instead of data unless tag is
///        specifically requested or the Binary option is set
pub fn extract_binary(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, offset, length, mut tag) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    let mut is_preview = false;

    if tag.truthy() {
        if pl::str_eq(&tag, "PreviewImage") {
            // save PreviewImage start/length in case we want to dump trailer
            self_.set("PreviewImageStart", offset.clone());
            self_.set("PreviewImageLength", length.clone());
            is_preview = true;
        }
        let lc_tag = lc(&tag).to_bytes();
        let options = self_.get("OPTIONS");
        if (!options.get("Binary").truthy() || self_.get("EXCL_TAG_LOOKUP").get(&lc_tag).truthy())
            && !options.get("Verbose").truthy()
            && !options.get("Validate").truthy()
            && !self_.get("REQ_TAG_LOOKUP").get(&lc_tag).truthy()
        {
            return ret(pl::concat(&pl::concat(&pl::s("Binary data "), &length), &pl::s(" bytes")));
        }
    }
    let scope = Scope::new(None);
    scope.scalar("self", self_.clone());
    scope.scalar("offset", offset);
    scope.scalar("length", length);
    let buff = scope.scalar("buff", Value::Undef);
    let ok = scalar_of(snippet(it, "$$self{RAF}->Seek($offset,0) and $$self{RAF}->Read($buff, $length) == $length", &scope, Want::Scalar)?);
    if !ok.truthy() {
        if !tag.truthy() {
            tag = pl::s("binary data");
        }
        if is_preview && !self_.get("BuildingComposite").truthy() {
            self_.set("PreviewError", n(1));
        } else {
            let msg = pl::concat(&pl::concat(&pl::s("Error reading "), &tag), &pl::s(" from file"));
            pl::method(it, &self_, "Warn", vec![msg, if is_preview { n(1) } else { Value::Undef }])?;
        }
        return pl::ret_undef();
    }
    let b = buff.borrow().clone();
    ret(b)
}

/// Process an image directory
/// Inputs: 0) ExifTool object reference, 1) directory information reference
///         2) tag table reference, 3) optional reference to processing procedure
/// Returns: Result from processing (1=success)
pub fn process_directory(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, dir_info, tag_table_ptr, mut proc_) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));

    if !(tag_table_ptr.truthy() && dir_info.truthy()) {
        return ret(n(0));
    }
    // use default proc from tag table or EXIF proc as fallback if no proc specified
    if !proc_.truthy() {
        proc_ = tag_table_ptr.get("PROCESS_PROC");
        if !proc_.truthy() {
            proc_ = code_ref(it, "Image::ExifTool::Exif::ProcessExif");
        }
    }
    // set directory name from default group0 name if not done already
    let mut dir_name = dir_info.get("DirName");
    if !dir_name.truthy() {
        dir_name = tag_table_ptr.path(&["GROUPS", "0"]);
        if re(r"^APP\d+$", "").is_match(&dir_name) {
            dir_name = tag_table_ptr.path(&["GROUPS", "1"]); // (use specific APP name)
        }
        dir_info.set("DirName", dir_name.clone());
    }

    // guard against cyclical recursion into the same directory
    if !dir_info.get("DirStart").is_undef() && !dir_info.get("DataPos").is_undef() &&
        // directories don't overlap if the length is zero
        (dir_info.get("DirLen").truthy() || dir_info.get("DirLen").is_undef())
    {
        let base = dir_info.get("Base");
        let base = if base.truthy() { base } else { n(0) };
        let addr = pl::add(&pl::add(&pl::add(&dir_info.get("DirStart"), &dir_info.get("DataPos")), &base), &self_.get("BASE"));
        let addr_k = addr.to_bytes();
        let processed = self_.get_or_new_hash("PROCESSED");
        if processed.get(&addr_k).truthy() && !dir_info.get("NotDup").truthy() {
            let msg = pl::concat(&pl::concat(&pl::concat(&dir_name, &pl::s(" pointer references previous ")), &processed.get(&addr_k)), &pl::s(" directory"));
            pl::method(it, &self_, "Warn", vec![msg])?;
            // patch for bug in Windows phone 7.5 O/S that writes incorrect InteropIFD pointer
            if !(pl::str_eq(&dir_name, "GPS") && pl::str_eq(&processed.get(&addr_k), "InteropIFD")) {
                return ret(n(0));
            }
        }
        let vars = tag_table_ptr.get("VARS");
        if !(vars.truthy() && vars.get("ALLOW_REPROCESS").truthy()) {
            processed.set(&addr_k, dir_name.clone());
        }
    }
    let old_order = pl::call(it, "Image::ExifTool::GetByteOrder", vec![])?;
    let keys = ["INDENT", "DIR_NAME", "Compression", "SubfileType"];
    let save: Vec<Value> = keys.iter().map(|k| self_.get(k)).collect();
    self_.set("LIST_TAGS", Value::new_hash()); // don't build lists across different directories
    self_.set("INDENT", pl::concat(&self_.get("INDENT"), &pl::s("| ")));
    self_.set("DIR_NAME", dir_name.clone());
    self_.get_or_new_array("PATH").push(dir_name.clone());
    self_.get_or_new_hash("FOUND_DIR").set(dir_name.to_bytes(), n(1));

    // process the directory
    let rtn_val = scalar_of(call_code(it, &proc_, vec![self_.clone(), dir_info.clone(), tag_table_ptr.clone()], Want::Scalar)?);

    if let Value::ArrayRef(p) = self_.get_or_new_array("PATH") {
        p.borrow_mut().pop();
    }
    for (k, v) in keys.iter().zip(save) {
        self_.set(k, v);
    }
    pl::call(it, "Image::ExifTool::SetByteOrder", vec![old_order])?;
    ret(rtn_val)
}

/// `$a || $b`
fn or(a: Value, b: Value) -> Value {
    if a.truthy() {
        a
    } else {
        b
    }
}

/// `$$v{k1}{k2}...{kn}` as an rvalue: perl autovivifies the hashes on the
/// way (not the last element).
fn vget(v: &Value, keys: &[&[u8]]) -> Value {
    let mut cur = v.clone();
    for (i, k) in keys.iter().enumerate() {
        if i + 1 == keys.len() {
            return cur.get(k);
        }
        cur = cur.get_or_new_hash(k);
    }
    cur
}

/// A new eval scope: the file's lexicals, then the sub's.
fn eval_scope(it: &mut Interp) -> Rc<Scope> {
    let parent = file_scope(it);
    Scope::new(Some(parent))
}

/// `tr/-_a-zA-Z0-9//dc`: remove illegal characters.
fn clean_name(v: &Value) -> Value {
    let b: Vec<u8> = v.to_bytes().iter().copied().filter(|c| *c == b'-' || *c == b'_' || c.is_ascii_alphanumeric()).collect();
    Value::bytes(b)
}

/// String concatenation of several values.
fn cat(parts: &[&Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

/// GetTagTable
/// Inputs: 0) table name
/// Returns: tag table reference, or undefined if not found
/// Notes: Always use this function instead of requiring module and using table
/// directly since this function also does the following the first time the table
/// is loaded:
/// - requires new module if necessary
/// - generates default GROUPS hash and Group 0 name from module name
/// - registers Composite tags if Composite table found
/// - saves descriptions for tags in specified table
/// - generates default TAG_PREFIX to be used for unknown tags
pub fn get_tag_table(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let table_name = arg(&a, 0);
    if !table_name.truthy() {
        return pl::ret_undef();
    }
    let tn = table_name.to_text();
    let all_tables = pkg_hash(it, "allTables");
    let mut table = all_tables.get(table_name.to_bytes());

    if !table.truthy() {
        // no strict 'refs';
        let sym = |it: &mut Interp| match it.global(b'%', &tn) {
            Binding::Hash(h) => Value::HashRef(h),
            _ => unreachable!(),
        };
        if sym(it).is_empty() {
            // try to load module for this table
            if let Some(m) = re("(.*)::", "").m(&table_name) {
                let module = cap(&m, 1);
                let scope = eval_scope(it);
                let code = cat(&[&pl::s("require "), &module]);
                let ok = scalar_of(eval_site(it, &code, &scope, Want::Scalar));
                if !ok.truthy() {
                    let err = eval_error(it);
                    if err.truthy() {
                        it.warn(err)?;
                    }
                } else if sym(it).is_empty() {
                    // load additional modules if required
                    if pl::str_eq(&module, "Image::ExifTool::XMP") {
                        runtime::require(it, "Image/ExifTool/XMP2.pl")?;
                    } else if pl::str_eq(&table_name, "Image::ExifTool::QuickTime::Stream") {
                        runtime::require(it, "Image/ExifTool/QuickTimeStream.pl")?;
                    }
                }
            }
            if sym(it).is_empty() {
                it.warn(cat(&[&pl::s("Can't find table "), &table_name, &pl::s("\n")]))?;
                return pl::ret_undef();
            }
        }
        table = sym(it);
        // use strict 'refs';
        let init = table.get("INIT_TABLE");
        if init.truthy() {
            call_code(it, &init, vec![table.clone()], Want::Void)?;
        }
        table.set("TABLE_NAME", table_name.clone()); // set table name
        let mut short = table_name.clone();
        re("^Image::ExifTool::", "").subst(&mut short, false, |_| Vec::new());
        table.set("SHORT_NAME", short);
        // set default group 0 and 1 from module name unless already specified
        let mut default_groups = table.get("GROUPS");
        if !default_groups.truthy() {
            default_groups = Value::new_hash();
            table.set("GROUPS", default_groups.clone());
        }
        if !(default_groups.get("0").truthy() && default_groups.get("1").truthy()) {
            if let Some(m) = re("Image::.*?::([^:]*)", "").m(&table_name) {
                let g = cap(&m, 1);
                if !default_groups.get("0").truthy() {
                    default_groups.set("0", g.clone());
                }
                if !default_groups.get("1").truthy() {
                    default_groups.set("1", g);
                }
            } else {
                if !default_groups.get("0").truthy() {
                    default_groups.set("0", table_name.clone());
                }
                if !default_groups.get("1").truthy() {
                    default_groups.set("1", table_name.clone());
                }
            }
        }
        if !default_groups.get("2").truthy() {
            default_groups.set("2", pl::s("Other"));
        }
        if pl::str_eq(&default_groups.get("0"), "XMP") || table.get("NAMESPACE").truthy() {
            // initialize some XMP table defaults
            runtime::require(it, "Image::ExifTool::XMP")?;
            pl::call(it, "Image::ExifTool::XMP::RegisterNamespace", vec![table.clone()])?; // register all table namespaces
            // set default write/check procs
            if !table.get("WRITE_PROC").truthy() {
                let c = code_ref(it, "Image::ExifTool::XMP::WriteXMP");
                table.set("WRITE_PROC", c);
            }
            if !table.get("CHECK_PROC").truthy() {
                let c = code_ref(it, "Image::ExifTool::XMP::CheckXMP");
                table.set("CHECK_PROC", c);
            }
            if !table.get("LANG_INFO").truthy() {
                let c = code_ref(it, "Image::ExifTool::XMP::GetLangInfo");
                table.set("LANG_INFO", c);
            }
        }
        // generate a tag prefix for unknown tags if necessary
        if table.get("TAG_PREFIX").is_undef() {
            let mut tag_prefix;
            let m = re("Image::.*?::(.*)::Main", "").m(&table_name).or_else(|| re("Image::.*?::(.*)", "").m(&table_name));
            if let Some(m) = m {
                tag_prefix = cap(&m, 1);
                re("::", "").subst(&mut tag_prefix, true, |_| b"_".to_vec());
            } else {
                tag_prefix = table_name.clone();
            }
            table.set("TAG_PREFIX", tag_prefix);
        }
        // set up the new table
        pl::call(it, "Image::ExifTool::SetupTagTable", vec![table.clone()])?;
        // add any user-defined tags (except Composite tags, which are handled specially)
        let user_defined = pkg_hash(it, "UserDefined");
        let composite = pkg_hash(it, "Composite");
        if !user_defined.is_empty() && user_defined.get(table_name.to_bytes()).truthy() && !table.same_ref(&composite) {
            let ud = user_defined.get(table_name.to_bytes());
            let special_tags = pkg_hash(it, "specialTags");
            for tag_id in tag_table_keys_list(it, &ud) {
                if special_tags.get(tag_id.to_bytes()).truthy() {
                    continue;
                }
                table.delete(tag_id.to_bytes()); // replace any existing entry
                let info = user_defined.get(table_name.to_bytes()).get(tag_id.to_bytes());
                add_tag_to_table_impl(it, &table, &tag_id, info, &n(1))?;
            }
        }
        // remember order we loaded the tables in
        pkg_array(it, "tableOrder").push(table_name.clone());
        // insert newly loaded table into list
        all_tables.set(table_name.to_bytes(), table.clone());
    }
    // must check each time to add UserDefined Composite tags because the Composite table
    // may be loaded before the UserDefined tags are available
    let composite = pkg_hash(it, "Composite");
    if table.same_ref(&composite) && !vget(&table, &[b"VARS", b"LOADED_USERDEFINED"]).truthy() {
        let user_defined = pkg_hash(it, "UserDefined");
        if !user_defined.is_empty() && user_defined.get(table_name.to_bytes()).truthy() {
            let user_comp = user_defined.get(table_name.to_bytes());
            user_defined.delete(table_name.to_bytes()); // (must delete first to avoid infinite recursion)
            pl::call(it, "Image::ExifTool::AddCompositeTags", vec![user_comp.clone(), n(1)])?;
            user_defined.set(table_name.to_bytes(), user_comp); // (add back again for adding writable tags later)
            table.get_or_new_hash("VARS").set("LOADED_USERDEFINED", n(1)); // set flag to avoid doing this again
        }
    }
    ret(table)
}

/// Find tag information, processing conditional tags
/// Inputs: 0) ExifTool object reference, 1) tagTable pointer, 2) tag ID
///         3) optional value reference (usually reference to binary data value, but
///            depends on information type), 4) optional format type, 5) optional value count
/// Returns: pointer to tagInfo hash, undefined if none found, or '' if $valPt needed
/// Notes: You should always call this routine to find a tag in a table because
/// this routine will evaluate conditional tags.
/// Arguments 3-5 are only required if the information type allows $valPt, $format and/or
/// $count in a Condition, and if not given when needed this routine returns ''.
pub fn get_tag_info(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(get_tag_info_impl(it, a)?)
}

fn get_tag_info_impl(it: &mut Interp, a: Vec<Value>) -> ER<Value> {
    let (self_, tag_table_ptr, tag_id) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let (mut val_pt, mut format, mut count) = (Value::Undef, Value::Undef, Value::Undef);
    let mut args = a;

    let info_array = tag_info_list(it, &tag_table_ptr, &tag_id)?.0;
    let options = self_.get("OPTIONS");
    // evaluate condition
    for tag_info in info_array.iter() {
        let condition = tag_info.get("Condition");
        if condition.truthy() {
            if args.len() > 3 {
                let rest = args.split_off(3);
                val_pt = arg(&rest, 0);
                format = arg(&rest, 1);
                count = arg(&rest, 2);
            }
            if re(r"\$(valPt|format|count)\b", "").is_match(&condition) && val_pt.is_undef() {
                return Ok(pl::s(""));
            }
            // set old value for use in condition if needed
            let saved = local_sig_warn(it);
            set_eval_warning(it, Value::Undef);
            //### eval Condition ($self, [$valPt, $format, $count])
            let scope = eval_scope(it);
            scope.scalar("self", self_.clone());
            scope.scalar("tagTablePtr", tag_table_ptr.clone());
            scope.scalar("tagID", tag_id.clone());
            scope.scalar("valPt", val_pt.clone());
            scope.scalar("format", format.clone());
            scope.scalar("count", count.clone());
            scope.array("infoArray", info_array.clone());
            scope.scalar("options", options.clone());
            scope.scalar("tagInfo", tag_info.clone());
            scope.scalar("condition", condition.clone());
            scope.array("_", args.clone());
            let r = scalar_of(eval_site(it, &condition, &scope, Want::Scalar));
            if !r.truthy() {
                let err = eval_error(it);
                if err.truthy() {
                    set_eval_warning(it, err);
                }
                let res = (|| -> ER<()> {
                    if eval_warning(it).truthy() {
                        let w = pl::call(it, "Image::ExifTool::CleanWarning", vec![])?;
                        let msg = cat(&[&pl::s("Condition "), &tag_info.get("Name"), &pl::s(": "), &w]);
                        pl::method(it, &self_, "Warn", vec![msg])?;
                    }
                    Ok(())
                })();
                restore_sig_warn(saved);
                res?;
                continue;
            }
            restore_sig_warn(saved);
        }
        // don't return Unknown tags unless that option is set or we are writing (also see forum13716)
        if tag_info.get("Unknown").truthy()
            && !options.get("Unknown").truthy()
            && (!self_.get("IsWriting").truthy() || tag_info.get("AddedUnknown").truthy())
            && !(options.get("Verbose").truthy() || self_.get("HTML_DUMP").truthy() || (options.get("Validate").truthy() && !tag_info.get("AddedUnknown").truthy()))
        {
            return Ok(Value::Undef);
        }
        // return the tag information we found
        return Ok(tag_info.clone());
    }
    // generate information for unknown tags (numerical only) if required
    let mut tag_info = Value::Undef;
    if (options.get("Unknown").truthy() || options.get("Verbose").truthy() || self_.get("HTML_DUMP").truthy()) && re(r"^\d+$", "").is_match(&tag_id) && !self_.get("NO_UNKNOWN").truthy() {
        let print_conv = if !tag_table_ptr.get("PRINT_CONV").is_undef() {
            tag_table_ptr.get("PRINT_CONV")
        } else {
            // limit length of printout (can be very long)
            code_ref(it, "Image::ExifTool::LimitLongValues")
        };
        let hex = pl::sprintf("0x%.4x", &[tag_id.clone()]);
        let prefix = tag_table_ptr.get("TAG_PREFIX");
        let ti = Value::new_hash();
        ti.set("Name", cat(&[&prefix, &pl::s("_"), &hex]));
        let desc = pl::call(it, "Image::ExifTool::MakeDescription", vec![prefix, hex])?;
        ti.set("Description", desc);
        ti.set("Unknown", n(1));
        ti.set("Writable", n(0)); // can't write unknown tags
        ti.set("PrintConv", print_conv);
        ti.set("AddedUnknown", n(1));
        // add tag information to table
        add_tag_to_table_impl(it, &tag_table_ptr, &tag_id, ti.clone(), &Value::Undef)?;
        tag_info = ti;
    }
    Ok(tag_info)
}

/// Add new tag to table (must use this routine to add new tags to a table)
/// Inputs: 0) reference to tag table, 1) tag ID
///         2) [optional] tag name or reference to tag information hash
///         3) [optional] flag to avoid adding prefix when generating tag name
/// Returns: tagInfo ref
/// Notes: - will not override existing entry in table
/// - info need contain no entries when this routine is called
/// - tag name is cleaned if necessary
pub fn add_tag_to_table(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let r = add_tag_to_table_impl(it, &arg(&a, 0), &arg(&a, 1), arg(&a, 2), &arg(&a, 3))?;
    ret(r)
}

fn add_tag_to_table_impl(it: &mut Interp, tag_table_ptr: &Value, tag_id: &Value, mut tag_info: Value, no_prefix: &Value) -> ER<Value> {
    // generate tag info hash if necessary
    if ref_of(&tag_info) != "HASH" {
        let h = Value::new_hash();
        if tag_info.truthy() {
            h.set("Name", tag_info);
        }
        tag_info = h;
    }

    // define necessary entries in information hash
    if tag_info.get("Groups").truthy() {
        // fill in default groups from table GROUPS
        let table_groups = tag_table_ptr.get_or_new_hash("GROUPS");
        for k in table_groups.keys() {
            if vget(&tag_info, &[b"Groups", &k]).truthy() {
                continue;
            }
            tag_info.get_or_new_hash("Groups").set(&k, table_groups.get(&k));
        }
    } else {
        tag_info.set("Groups", tag_table_ptr.get("GROUPS").hash_copy());
    }
    if tag_info.get("Flags").truthy() {
        pl::call(it, "Image::ExifTool::ExpandFlags", vec![tag_info.clone()])?;
    }
    tag_info.set("GotGroups", n(1));
    tag_info.set("Table", tag_table_ptr.clone());
    tag_info.set("TagID", tag_id.clone());
    if tag_info.get("Hidden").is_undef() {
        tag_info.set("Hidden", n(1));
    }
    if !tag_table_ptr.get("AVOID").is_undef() && tag_info.get("Avoid").is_undef() {
        tag_info.set("Avoid", tag_table_ptr.get("AVOID"));
    }

    let mut name = tag_info.get("Name");
    if name.is_undef() {
        name = tag_id.clone();
    }
    name = clean_name(&name); // remove illegal characters
    name = ucfirst(&name); // capitalize first letter
    // add tag-name prefix if specified and tag name not provided
    if !(!tag_info.get("Name").is_undef() || no_prefix.truthy() || !tag_table_ptr.get("TAG_PREFIX").truthy()) {
        // make description to prevent tagID from getting mangled by MakeDescription()
        let desc = pl::call(it, "Image::ExifTool::MakeDescription", vec![tag_table_ptr.get("TAG_PREFIX"), name.clone()])?;
        tag_info.set("Description", desc);
        name = cat(&[&tag_table_ptr.get("TAG_PREFIX"), &pl::s("_"), &name]);
    }
    // tag names must be at least 2 characters long and prefer them to start with a letter
    if name.to_bytes().len() < 2 || !re("^[A-Z]", "i").is_match(&name) {
        name = cat(&[&pl::s("Tag"), &name]);
    }
    tag_info.set("Name", name);
    // add tag to table, but never override existing entries (could potentially happen
    // if someone thinks there isn't any tagInfo because a condition wasn't satisfied)
    let special_tags = pkg_hash(it, "specialTags");
    let key = tag_id.to_bytes();
    if !(!tag_table_ptr.get(&key).is_undef() || special_tags.get(&key).truthy()) {
        tag_table_ptr.set(&key, tag_info.clone());
        let purge_flag = runtime::lex_scalar(it, ET, "purgeFlag").borrow().clone();
        if purge_flag.truthy() && tag_info.get("Unknown").truthy() && !tag_info.get("SubDirectory").truthy() {
            runtime::lex_array(it, ET, "purgeTags").push(tag_info.clone());
        }
    }
    if tag_info.get("Unknown").truthy() {
        tag_info.set("AddedUnknown", n(1));
    }
    Ok(tag_info)
}

/// Handle simple extraction of new tag information
/// Inputs: 0) ExifTool object ref, 1) tag table reference, 2) tagID, 3) raw value,
///         4-N) parameters hash: Index, DataPt, DataPos, Base, Start, Size, Parent,
///              TagInfo, ProcessProc, RAF, Format, Count, MakeTagInfo
/// Returns: tag key or undef if tag not found
/// Notes: if value is not defined, it is extracted from DataPt using TagInfo
///        Format and Count if provided
/// - set MakeTagInfo to add tag info for unknown tags with name made from tag ID
pub fn handle_tag(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let args_all = a.clone();
    let (self_, tag_table_ptr, tag) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    // ($val is a variable whose reference is taken: \$val)
    let val = cell(arg(&a, 3));
    let parms: Rc<RefCell<Hash>> = Rc::new(RefCell::new(crate::interp::eval::pairs_to_hash(a.into_iter().skip(4).collect())));
    let pget = |k: &str| parms.borrow().get(k.as_bytes()).cloned().unwrap_or_default();
    let verbose = self_.path(&["OPTIONS", "Verbose"]);
    let pfmt = pget("Format");
    let val_pt = if !val.borrow().is_undef() { Value::ScalarRef(val.clone()) } else { Value::Undef };
    let mut tag_info = pget("TagInfo");
    if !tag_info.truthy() {
        tag_info = get_tag_info_impl(it, vec![self_.clone(), tag_table_ptr.clone(), tag.clone(), val_pt.clone(), pfmt.clone(), pget("Count")])?;
    }
    let mut data_pt = pget("DataPt");
    let (mut subdir, mut format, mut no_tag_info, mut binval) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef);
    let rational = cell(Value::Undef);
    let format_size = lex_hash(it, "formatSize");

    // get binary data for Condition if necessary
    if !tag_info.truthy() && !tag_info.is_undef() && data_pt.truthy() {
        let start = or(pget("Start"), n(0));
        let mut size = pget("Size");
        if size.is_undef() {
            size = pl::sub(&len_of(&data_pt.deref()), &start);
        }
        if pl::gt(&pl::add(&start, &size), &len_of(&data_pt.deref())) {
            return pl::ret_undef();
        }
        if pl::gt(&size, &n(1024)) {
            size = n(1024); // max 1024 bytes available for the Condition
        }
        let dat = substr(it, &data_pt.deref(), &start, Some(&size))?;
        let dat_pt = Value::ScalarRef(cell(dat));
        tag_info = get_tag_info_impl(it, vec![self_.clone(), tag_table_ptr.clone(), tag.clone(), dat_pt, pfmt.clone(), pget("Count")])?;
    }
    if tag_info.truthy() {
        subdir = tag_info.get("SubDirectory");
    } else if pget("MakeTagInfo").truthy() {
        if verbose.truthy() {
            let indent = self_.get("INDENT");
            vprint(it, &self_, &n(0), vec![indent, cat(&[&pl::s("[adding "), &tag, &pl::s("]\n")])])?;
        }
        let mut name = tag.clone();
        re("([A-Z]) ([A-Z][ A-Z])", "").subst(&mut name, true, |c| [&cap(c, 1).to_bytes()[..], b"_", &cap(c, 2).to_bytes()[..]].concat()); // underline between acronyms
        re("([^A-Za-z])([a-z])", "").subst(&mut name, true, |c| [&cap(c, 1).to_bytes()[..], &ucfirst(&cap(c, 2)).to_bytes()[..]].concat()); // capitalize words
        name = clean_name(&name); // remove illegal characters
        if name.to_bytes().len() < 2 || re("^[-0-9]", "").is_match(&name) {
            name = cat(&[&pl::s("Tag"), &name]);
        }
        tag_info = Value::new_hash();
        tag_info.set("Name", ucfirst(&name));
        add_tag_to_table_impl(it, &tag_table_ptr, &tag, tag_info.clone(), &Value::Undef)?;
    } else {
        if !verbose.truthy() {
            return pl::ret_undef();
        }
        tag_info = Value::new_hash(); // create temporary tagInfo hash
        tag_info.set("Name", cat(&[&pl::s("tag "), &tag]));
        no_tag_info = n(1);
    }
    // read value if not done already (not necessary for subdir)
    if !(!val.borrow().is_undef() || (subdir.truthy() && !tag_info.get("Writable").truthy() && !tag_info.get("RawConv").truthy())) {
        let start = or(pget("Start"), n(0));
        let d_len = if data_pt.truthy() { len_of(&data_pt.deref()) } else { n(-1) };
        let mut size = pget("Size");
        if size.is_undef() {
            size = if pl::gt(&d_len, &n(0)) { d_len.clone() } else { n(0) };
        }
        // read from data in memory if possible
        if pl::ge(&start, &n(0)) && pl::le(&pl::add(&start, &size), &d_len) {
            format = or(tag_info.get("Format"), tag_table_ptr.get("FORMAT"));
            if !format.truthy() && pfmt.truthy() && format_size.get(pfmt.to_bytes()).truthy() {
                format = pfmt.clone();
            }
            let rat_ref = Value::ScalarRef(rational.clone());
            let v = if !format.truthy() {
                substr(it, &data_pt.deref(), &start, Some(&size))?
            } else if !tag_info.get("ByteOrder").truthy() {
                pl::call(it, "Image::ExifTool::ReadValue", vec![data_pt.clone(), start.clone(), format.clone(), tag_info.get("Count"), size.clone(), rat_ref])?
            } else {
                let old_order = pl::call(it, "Image::ExifTool::GetByteOrder", vec![])?;
                pl::call(it, "Image::ExifTool::SetByteOrder", vec![tag_info.get("ByteOrder")])?;
                let v = pl::call(it, "Image::ExifTool::ReadValue", vec![data_pt.clone(), start.clone(), format.clone(), tag_info.get("Count"), size.clone(), rat_ref])?;
                pl::call(it, "Image::ExifTool::SetByteOrder", vec![old_order])?;
                v
            };
            *val.borrow_mut() = v;
            if self_.path(&["OPTIONS", "SaveBin"]).truthy() {
                binval = substr(it, &data_pt.deref(), &start, Some(&size))?;
            }
        } else {
            let msg = cat(&[&pl::s("Error extracting value for "), &tag_info.get("Name")]);
            pl::method(it, &self_, "Warn", vec![msg])?;
            return pl::ret_undef();
        }
    }
    // do verbose print if necessary
    if verbose.truthy() {
        if no_tag_info.truthy() {
            tag_info = Value::Undef;
        }
        parms.borrow_mut().insert(b"Value", val.borrow().clone());
        let r = rational.borrow().clone();
        if !r.is_undef() {
            let v = cat(&[&pget("Value"), &pl::s(" ("), &r, &pl::s(")")]);
            parms.borrow_mut().insert(b"Value", v);
        }
        parms.borrow_mut().insert(b"Table", tag_table_ptr.clone());
        if format.truthy() {
            let fs = or(format_size.get(format.to_bytes()), n(1));
            let count = pl::int(&pl::div(&or(pget("Size"), n(0)), &fs)?);
            parms.borrow_mut().insert(b"Format", cat(&[&format, &pl::s("["), &count, &pl::s("]")]));
        }
        let mut args = vec![tag.clone(), tag_info.clone()];
        for (k, v) in parms.borrow().iter() {
            args.push(Value::Str(k.clone()));
            args.push(v.clone());
        }
        meth(it, &self_, "VerboseInfo", args, Want::Void)?;
    }
    if tag_info.truthy() {
        if subdir.truthy() {
            if tag_info.get("MakerNotes").truthy() && self_.path(&["OPTIONS", "FastScan"]).truthy() && pl::gt(&self_.path(&["OPTIONS", "FastScan"]), &n(1)) {
                return pl::ret_undef(); // don't process maker note directories when FastScan > 1
            }
            let mut subdir_start = pget("Start");
            let mut subdir_len = pget("Size");
            if tag_info.get("RawConv").truthy() && !tag_info.get("Writable").truthy() {
                let conv = tag_info.get("RawConv");
                let saved = local_sig_warn(it);
                set_eval_warning(it, Value::Undef);
                let res = (|| -> ER<()> {
                    if ref_of(&conv) == "CODE" {
                        let v = scalar_of(call_code(it, &conv, vec![val.borrow().clone(), self_.clone()], Want::Scalar)?);
                        *val.borrow_mut() = v;
                    } else {
                        // NOTE: RawConv is evaluated in Writer.pl and twice in ExifTool.pm
                        //### eval RawConv ($self, $val, $tag, $tagInfo, $priority, @grps)
                        let scope = eval_scope(it);
                        scope.scalar("self", self_.clone());
                        scope.scalar("tagTablePtr", tag_table_ptr.clone());
                        let tag_c = scope.scalar("tag", tag.clone());
                        bind(&scope, "val", &val);
                        scope.declare(b'%', "parms", Binding::Hash(parms.clone()));
                        scope.scalar("verbose", verbose.clone());
                        scope.scalar("pfmt", pfmt.clone());
                        scope.scalar("valPt", val_pt.clone());
                        scope.scalar("tagInfo", tag_info.clone());
                        let data_pt_c = scope.scalar("dataPt", data_pt.clone());
                        scope.scalar("subdir", subdir.clone());
                        scope.scalar("format", format.clone());
                        scope.scalar("noTagInfo", no_tag_info.clone());
                        bind(&scope, "rational", &rational);
                        scope.scalar("binVal", binval.clone());
                        scope.scalar("subdirStart", subdir_start.clone());
                        scope.scalar("subdirLen", subdir_len.clone());
                        scope.scalar("conv", conv.clone());
                        scope.scalar("priority", Value::Undef);
                        scope.array("grps", vec![]);
                        scope.array("_", args_all.clone());
                        let v = scalar_of(eval_site(it, &conv, &scope, Want::Scalar));
                        let _ = (tag_c, data_pt_c);
                        *val.borrow_mut() = v;
                        let err = eval_error(it);
                        if err.truthy() {
                            set_eval_warning(it, err);
                        }
                    }
                    if eval_warning(it).truthy() {
                        let w = pl::call(it, "Image::ExifTool::CleanWarning", vec![])?;
                        let msg = cat(&[&pl::s("RawConv "), &tag, &pl::s(": "), &w]);
                        pl::method(it, &self_, "Warn", vec![msg])?;
                    }
                    Ok(())
                })();
                restore_sig_warn(saved);
                res?;
                if val.borrow().is_undef() {
                    return pl::ret_undef();
                }
                let v = val.borrow().clone();
                data_pt = if ref_of(&v) == "SCALAR" { v } else { Value::ScalarRef(val.clone()) };
                subdir_start = n(0);
                subdir_len = len_of(&data_pt.deref());
            } else if !data_pt.truthy() {
                let v = val.borrow().clone();
                data_pt = if ref_of(&v) == "SCALAR" { v } else { Value::ScalarRef(val.clone()) };
            }
            if subdir.get("Start").truthy() {
                //### eval Start ($valuePtr)
                let scope = eval_scope(it);
                scope.scalar("self", self_.clone());
                scope.scalar("tagTablePtr", tag_table_ptr.clone());
                scope.scalar("tag", tag.clone());
                bind(&scope, "val", &val);
                scope.declare(b'%', "parms", Binding::Hash(parms.clone()));
                scope.scalar("verbose", verbose.clone());
                scope.scalar("pfmt", pfmt.clone());
                scope.scalar("valPt", val_pt.clone());
                scope.scalar("tagInfo", tag_info.clone());
                scope.scalar("dataPt", data_pt.clone());
                scope.scalar("subdir", subdir.clone());
                scope.scalar("format", format.clone());
                scope.scalar("noTagInfo", no_tag_info.clone());
                bind(&scope, "rational", &rational);
                scope.scalar("binVal", binval.clone());
                scope.scalar("subdirStart", subdir_start.clone());
                scope.scalar("subdirLen", subdir_len.clone());
                scope.scalar("valuePtr", n(0));
                scope.array("_", args_all.clone());
                let off = scalar_of(eval_site(it, &subdir.get("Start"), &scope, Want::Scalar));
                subdir_start = pl::add(&subdir_start, &off);
                subdir_len = pl::sub(&subdir_len, &off);
            }
            // process subdirectory information
            let dir_info = Value::new_hash();
            dir_info.set("DirName", or(subdir.get("DirName"), tag_info.get("Name")));
            dir_info.set("DataPt", data_pt.clone());
            dir_info.set("DataLen", len_of(&data_pt.deref()));
            dir_info.set("DataPos", pget("DataPos"));
            dir_info.set("DirStart", subdir_start.clone());
            dir_info.set("DirLen", subdir_len.clone());
            dir_info.set("DirID", tag.clone());
            dir_info.set("Parent", pget("Parent"));
            dir_info.set("Base", pget("Base"));
            dir_info.set("Multi", subdir.get("Multi"));
            dir_info.set("TagInfo", tag_info.clone());
            dir_info.set("IgnoreProp", subdir.get("IgnoreProp"));
            dir_info.set("RAF", pget("RAF"));
            let old_order = pl::call(it, "Image::ExifTool::GetByteOrder", vec![])?;
            if subdir.get("ByteOrder").truthy() {
                if pl::str_eq(&subdir.get("ByteOrder"), "Unknown") {
                    if pl::le(&pl::add(&subdir_start, &n(2)), &subdir_len) {
                        // attempt to determine the byte ordering of an IFD-style subdirectory
                        let num = pl::call(it, "Image::ExifTool::Get16u", vec![data_pt.clone(), subdir_start.clone()])?;
                        use crate::perl::ops;
                        if ops::bit_and(&num, &n(0xff00)).truthy() && pl::gt(&ops::shift_right(&num, &n(8)), &ops::bit_and(&num, &n(0xff))) {
                            pl::call(it, "Image::ExifTool::ToggleByteOrder", vec![])?;
                        }
                    }
                } else {
                    pl::call(it, "Image::ExifTool::SetByteOrder", vec![subdir.get("ByteOrder")])?;
                }
            }
            let sub_table_ptr = or(pl::call(it, "Image::ExifTool::GetTagTable", vec![subdir.get("TagTable")])?, tag_table_ptr.clone());
            let proc_ = or(subdir.get("ProcessProc"), pget("ProcessProc"));
            pl::method(it, &self_, "ProcessDirectory", vec![dir_info, sub_table_ptr, proc_])?;
            pl::call(it, "Image::ExifTool::SetByteOrder", vec![old_order])?;
            // return now unless directory is writable as a block
            if !tag_info.get("Writable").truthy() {
                return pl::ret_undef();
            }
        }
        let v = val.borrow().clone();
        let key = found_tag_call(it, &self_, vec![tag_info.clone(), v])?;
        if !key.is_undef() {
            // save original components of rational numbers and original binary value
            let r = rational.borrow().clone();
            if !r.is_undef() {
                self_.get_or_new_hash("TAG_EXTRA").get_or_new_hash(key.to_bytes()).set("Rational", r);
            }
            if !binval.is_undef() {
                self_.get_or_new_hash("TAG_EXTRA").get_or_new_hash(key.to_bytes()).set("BinVal", binval);
            }
        }
        return ret(key);
    }
    pl::ret_undef()
}

/// Add tag to hash of extracted information
/// Inputs: 0) ExifTool object reference
///         1) reference to tagInfo hash or tag name
///         2) data value (or reference to require hash if Composite)
///         3) optional family 0 group, 4) optional family 1 group
/// Returns: tag key or undef if no value
pub fn found_tag(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    // local $_;
    let topic = local_topic(it);
    let r = found_tag_impl(it, a);
    restore_topic(it, topic);
    ret(r?)
}

fn found_tag_impl(it: &mut Interp, a: Vec<Value>) -> ER<Value> {
    let args_all = a.clone();
    let (self_, mut tag_info, mut value) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let mut grps: Vec<Value> = a.into_iter().skip(3).collect();
    let (mut tag, mut no_list_del, tbl);
    no_list_del = Value::Undef;
    let options = self_.get("OPTIONS");

    if ref_of(&tag_info) == "HASH" {
        tag = tag_info.get("Name");
        if !tag.truthy() {
            it.warn(pl::s("No tag name\n"))?;
            return Ok(Value::Undef);
        }
        tbl = tag_info.get("Table");
    } else {
        tag = tag_info.clone();
        // look for tag in Extra
        tbl = pl::call(it, "Image::ExifTool::GetTagTable", vec![pl::s("Image::ExifTool::Extra")])?;
        tag_info = get_tag_info_impl(it, vec![self_.clone(), tbl.clone(), tag.clone()])?;
        // make temporary hash if tag doesn't exist in Extra
        // (not advised to do this since the tag won't show in list)
        if !tag_info.truthy() {
            tag_info = Value::new_hash();
            tag_info.set("Name", tag.clone());
            let g = lex_hash(it, "allGroupsExifTool");
            tag_info.set("Groups", g);
        }
        if options.get("Verbose").truthy() {
            meth(it, &self_, "VerboseInfo", vec![Value::Undef, tag_info.clone(), pl::s("Value"), value.clone()], Want::Void)?;
        }
    }
    // get tag priority
    let mut priority = tag_info.get("Priority");
    if priority.is_undef() {
        priority = tbl.get("PRIORITY");
        if priority.is_undef() && tag_info.get("Avoid").truthy() {
            priority = n(0);
        }
    }
    if grps.is_empty() {
        grps.push(Value::Undef);
    }
    if !grps[0].truthy() {
        grps[0] = self_.get("SET_GROUP0");
    }
    if grps.len() < 2 {
        grps.push(Value::Undef);
    }
    if !grps[1].truthy() {
        grps[1] = self_.get("SET_GROUP1");
    }
    if options.get("IgnoreGroups").truthy() {
        for i in 0..2i64 {
            let k = Value::Int(i).to_bytes();
            let g = lc(&or(or(grps.get(i as usize).cloned().unwrap_or_default(), vget(&tag_info, &[b"Groups", &k])), vget(&tag_info, &[b"Table", b"GROUPS", &k])));
            let ig = options.get("IgnoreGroups");
            if ig.get(g.to_bytes()).truthy() || ig.get(cat(&[&Value::Int(i), &g]).to_bytes()).truthy() {
                return Ok(Value::Undef);
            }
        }
    }
    let value_hash = self_.get("VALUE");

    if tag_info.get("RawConv").truthy() {
        // initialize @val for use in Composite RawConv expressions
        let mut val_list: Vec<Value> = Vec::new();
        if ref_of(&value) == "HASH" && tag_info.get("IsComposite").truthy() {
            for k in value.keys() {
                let i = Value::Str(k.clone()).to_i64();
                let v = value_hash.get(value.get(&k).to_bytes());
                if i >= 0 {
                    let i = i as usize;
                    if i >= val_list.len() {
                        val_list.resize(i + 1, Value::Undef);
                    }
                    val_list[i] = v;
                }
            }
        }
        let conv = tag_info.get("RawConv");
        let saved = local_sig_warn(it);
        set_eval_warning(it, Value::Undef);
        let res = (|| -> ER<()> {
            if ref_of(&conv) == "CODE" {
                value = scalar_of(call_code(it, &conv, vec![value.clone(), self_.clone()], Want::Scalar)?);
                let g = self_.get("grps");
                if g.truthy() {
                    grps = g.items();
                    self_.delete("grps");
                }
            } else {
                let val = value.clone(); // do this so eval can use $val
                // NOTE: RawConv is also evaluated in Writer.pl
                //### eval RawConv ($self, $val, $tag, $tagInfo, $priority, @grps)
                let scope = eval_scope(it);
                scope.scalar("self", self_.clone());
                let tag_info_c = scope.scalar("tagInfo", tag_info.clone());
                scope.scalar("value", value.clone());
                let grps_c = scope.array("grps", grps.clone());
                let tag_c = scope.scalar("tag", tag.clone());
                let no_list_del_c = scope.scalar("noListDel", no_list_del.clone());
                scope.scalar("tbl", tbl.clone());
                scope.scalar("options", options.clone());
                let priority_c = scope.scalar("priority", priority.clone());
                scope.scalar("valueHash", value_hash.clone());
                scope.array("val", val_list.clone());
                scope.scalar("conv", conv.clone());
                scope.scalar("val", val);
                scope.array("_", args_all.clone());
                value = scalar_of(eval_site(it, &conv, &scope, Want::Scalar));
                tag_info = tag_info_c.borrow().clone();
                grps = grps_c.borrow().clone();
                tag = tag_c.borrow().clone();
                no_list_del = no_list_del_c.borrow().clone();
                priority = priority_c.borrow().clone();
                let err = eval_error(it);
                if err.truthy() {
                    set_eval_warning(it, err);
                }
            }
            if eval_warning(it).truthy() {
                let w = pl::call(it, "Image::ExifTool::CleanWarning", vec![])?;
                let msg = cat(&[&pl::s("RawConv "), &tag, &pl::s(": "), &w]);
                pl::method(it, &self_, "Warn", vec![msg])?;
            }
            Ok(())
        })();
        restore_sig_warn(saved);
        res?;
        if value.is_undef() {
            return Ok(Value::Undef);
        }
    }
    // ignore specified tags (AFTER doing RawConv if necessary!)
    if options.get("IgnoreTags").truthy() {
        let lc_tag = lc(&tag).to_bytes();
        if vget(&options, &[b"IgnoreTags", b"all"]).truthy() {
            if !vget(&self_, &[b"REQ_TAG_LOOKUP", &lc_tag]).truthy() {
                return Ok(Value::Undef);
            }
        } else if vget(&options, &[b"IgnoreTags", &lc_tag]).truthy() {
            return Ok(Value::Undef);
        }
    }
    let tag_extra = |s: &Value| s.get_or_new_hash("TAG_EXTRA");
    // handle duplicate tag names
    if !value_hash.get(tag.to_bytes()).is_undef() {
        // add to list if there is an active list for this tag
        let ti_key = tag_info.to_bytes();
        if vget(&self_, &[b"LIST_TAGS", &ti_key]).truthy() {
            tag = self_.get("LIST_TAGS").get(&ti_key); // use key from previous list tag
            let tk = tag.to_bytes();
            if !self_.get("NO_LIST").is_undef() {
                // accumulate list in TAG_EXTRA "NoList" element
                let te = tag_extra(&self_).get_or_new_hash(&tk);
                if !te.get("NoList").is_undef() {
                    te.get_or_new_array("NoList").push(value.clone());
                } else {
                    te.set("NoList", Value::new_array(vec![value_hash.get(&tk), value.clone()]));
                }
                no_list_del = n(1); // set flag to delete this tag if re-listed
            } else {
                if ref_of(&value_hash.get(&tk)) != "ARRAY" {
                    value_hash.set(&tk, Value::new_array(vec![value_hash.get(&tk)]));
                }
                value_hash.get(&tk).push(value.clone());
                return Ok(tag); // return without creating a new entry
            }
        }
        let tk = tag.to_bytes();
        // get next available tag key
        let dupl = self_.get_or_new_hash("DUPL_TAG");
        let next_ind = pl::add(&or(dupl.get(&tk), n(0)), &n(1));
        dupl.set(&tk, next_ind.clone());
        let next_tag = cat(&[&tag, &pl::s(" ("), &next_ind, &pl::s(")")]);
        let ntk = next_tag.to_bytes();
//
// take tag with highest priority
//
        // promote existing 0-priority tag so it takes precedence over a new 0-tag
        // (unless old tag was a sub-document and new tag isn't.  Also, never override
        //  a Warning tag because they may be added by ValueConv, which could be confusing)
        let mut old_priority = vget(&self_, &[b"PRIORITY", &tk]);
        if !old_priority.truthy() {
            if self_.get("DOC_NUM").truthy() || pl::str_eq(&tag, "Warning") || !vget(&self_, &[b"TAG_EXTRA", &tk, b"G3"]).truthy() {
                old_priority = n(1);
            } else {
                old_priority = n(0); // don't promote sub-document tag over main document
            }
        }
        // set priority for this tag
        if !priority.is_undef() {
            // increase 0-priority tags if this is the priority directory
            if !priority.truthy() && self_.get("DIR_NAME").truthy() && seq(&self_.get("DIR_NAME"), &self_.get("PRIORITY_DIR")) {
                priority = n(1);
            }
        } else if vget(&self_, &[b"LOW_PRIORITY_DIR", b"*"]).truthy() || (self_.get("DIR_NAME").truthy() && vget(&self_, &[b"LOW_PRIORITY_DIR", &self_.get("DIR_NAME").to_bytes()]).truthy()) {
            priority = n(0); // default is 0 for a LOW_PRIORITY_DIR
        } else {
            priority = n(1); // the normal default
        }
        if pl::ge(&priority, &old_priority)
            && (!self_.get("DOC_NUM").truthy() || (vget(&self_, &[b"TAG_EXTRA", &tk, b"G3"]).truthy() && seq(&self_.get("DOC_NUM"), &vget(&self_, &[b"TAG_EXTRA", &tk, b"G3"]))))
            && !no_list_del.truthy()
        {
            // move existing tag out of the way since this tag is higher priority
            // (NOTE: any new members added here must also be added to DeleteTag())
            let pr = self_.get_or_new_hash("PRIORITY");
            pr.set(&ntk, pr.get(&tk));
            value_hash.set(&ntk, value_hash.get(&tk));
            let fo = self_.get_or_new_hash("FILE_ORDER");
            fo.set(&ntk, fo.get(&tk));
            let ti = self_.get_or_new_hash("TAG_INFO");
            let old_info = ti.get(&tk);
            ti.set(&ntk, old_info.clone());
            let te = tag_extra(&self_);
            te.set(&ntk, te.get(&tk));
            te.set(&tk, Value::new_hash());
            self_.get_or_new_hash("BOTH").delete(&tk);
            // update tag key for list if necessary
            let oik = old_info.to_bytes();
            if vget(&self_, &[b"LIST_TAGS", &oik]).truthy() {
                self_.get("LIST_TAGS").set(&oik, next_tag.clone());
            }
            // update this key if used in a Composite tag
            if vget(&self_, &[b"COMP_KEYS", &tk]).truthy() {
                let ck = self_.get("COMP_KEYS");
                for e in ck.get(&tk).items() {
                    let k1 = e.at(1).to_bytes();
                    e.at(0).set(&k1, next_tag.clone());
                }
                ck.set(&ntk, ck.get(&tk));
                ck.delete(&tk);
            }
        } else {
            tag = next_tag; // don't override the existing tag
        }
        let tk = tag.to_bytes();
        self_.get_or_new_hash("PRIORITY").set(&tk, priority.clone());
        if no_list_del.truthy() {
            tag_extra(&self_).get_or_new_hash(&tk).set("NoListDel", n(1));
        }
    } else if priority.truthy() {
        // set tag priority (only if exists and is non-zero)
        self_.get_or_new_hash("PRIORITY").set(tag.to_bytes(), priority.clone());
    }

    // save the raw value, file order, tagInfo ref, group1 name,
    // and tag key for lists if necessary
    let tk = tag.to_bytes();
    value_hash.set(&tk, value.clone());
    let num = pl::add(&self_.get("NUM_FOUND"), &n(1));
    self_.set("NUM_FOUND", num.clone());
    self_.get_or_new_hash("FILE_ORDER").set(&tk, num);
    self_.get_or_new_hash("TAG_INFO").set(&tk, tag_info.clone());
    let te = tag_extra(&self_);
    if !te.get(&tk).truthy() {
        te.set(&tk, Value::new_hash());
    }
    // set dynamic groups 0, 1 and 3 if necessary
    if grps[0].truthy() {
        te.get_or_new_hash(&tk).set("G0", grps[0].clone());
    }
    if grps[1].truthy() {
        te.get_or_new_hash(&tk).set("G1", grps[1].clone());
    }
    let doc_num = self_.get("DOC_NUM");
    if doc_num.truthy() {
        te.get_or_new_hash(&tk).set("G3", doc_num.clone());
        self_.get_or_new_hash("HAS_DOC").set(doc_num.to_bytes(), n(1));
        if let Some(m) = re(r"^(\d+)", "").m(&doc_num) {
            // keep track of maximum 1st-level sub-document number
            let d = cap(&m, 1);
            if !pl::ge(&self_.get("DOC_COUNT"), &d) {
                self_.set("DOC_COUNT", d);
            }
        }
    }
    // save path if requested
    if options.get("SavePath").truthy() {
        let p = pl::method(it, &self_, "MetadataPath", vec![])?;
        te.get_or_new_hash(&tk).set("G5", p);
    }

    // remember this tagInfo if we will be accumulating values in a list
    // (but don't override earlier list if this may be deleted by NoListDel flag)
    if tag_info.get("List").truthy() && !self_.get("NO_LIST").truthy() && !no_list_del.truthy() {
        self_.get_or_new_hash("LIST_TAGS").set(tag_info.to_bytes(), tag.clone());
    }

    // validate tag if requested (but only for simple values -- could result
    // in infinite recursion if called for a Composite tag (HASH ref value)
    // because FoundTag is called in the middle of building Composite tags
    if options.get("Validate").truthy() && !value.is_ref() {
        pl::call(it, "Image::ExifTool::Validate::ValidateRaw", vec![self_.clone(), tag.clone(), value.clone()])?;
    }

    Ok(tag)
}

/// The value of a lexical held in a cell.
fn g(c: &Rc<RefCell<Value>>) -> Value {
    c.borrow().clone()
}

fn s(c: &Rc<RefCell<Value>>, v: Value) {
    *c.borrow_mut() = v;
}

/// An eval scope binding these lexical cells (in declaration order).
fn scope_of(it: &mut Interp, scalars: &[(&str, &Rc<RefCell<Value>>)]) -> Rc<Scope> {
    let scope = eval_scope(it);
    for (name, c) in scalars {
        bind(&scope, name, c);
    }
    scope
}

/// Process binary data
/// Inputs: 0) ExifTool object ref, 1) directory information ref, 2) tag table ref
/// Returns: 1 on success
/// Notes: dirInfo may contain VarFormatData (reference to empty list) to return
///        details about any variable-length-format tags in the table (used when writing)
pub fn process_binary_data(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    use crate::perl::ops;
    let args_all: Rc<RefCell<Vec<Value>>> = Rc::new(RefCell::new(a.clone()));
    let (self_, dir_info, tag_table_ptr) = (cell(arg(&a, 0)), cell(arg(&a, 1)), cell(arg(&a, 2)));
    let et = g(&self_);
    let di = g(&dir_info);
    let ttp = g(&tag_table_ptr);
    let data_pt = cell(di.get("DataPt"));
    let dp = g(&data_pt);
    let data_len = cell(len_of(&dp.deref()));
    let dir_start = cell(or(di.get("DirStart"), n(0)));
    let max_len = cell(pl::sub(&g(&data_len), &g(&dir_start)));
    let size = cell(di.get("DirLen"));
    let base = cell(or(di.get("Base"), n(0)));
    let verbose = cell(et.path(&["OPTIONS", "Verbose"]));
    let unknown = cell(et.path(&["OPTIONS", "Unknown"]));
    let data_pos = cell(or(di.get("DataPos"), n(0)));
    let format_size = lex_hash(it, "formatSize");

    if g(&size).is_undef() || pl::gt(&g(&size), &g(&max_len)) {
        s(&size, g(&max_len));
    }
    // get default format ('int8u' unless specified)
    let default_format = cell(or(ttp.get("FORMAT"), pl::s("int8u")));
    let increment = cell(format_size.get(g(&default_format).to_bytes()));
    if !g(&increment).truthy() {
        it.warn(cat(&[&pl::s("Unknown format "), &g(&default_format), &pl::s("\n")]))?;
        s(&default_format, pl::s("int8u"));
        s(&increment, format_size.get(g(&default_format).to_bytes()));
    }
    // prepare list of tag numbers to extract
    let tags: Rc<RefCell<Vec<Value>>> = Rc::new(RefCell::new(Vec::new()));
    let (top_index, bin_val) = (cell(Value::Undef), cell(Value::Undef));
    let num_sort = |v: &mut Vec<Value>, key: &dyn Fn(&Value) -> Value| {
        v.sort_by(|x, y| ops::num_cmp(&key(x), &key(y)).unwrap_or(std::cmp::Ordering::Equal));
    };
    if pl::gt(&g(&unknown), &n(1)) && !ttp.get("FIRST_ENTRY").is_undef() {
        // don't create a stupid number of tags if data is huge
        let size_limit = if pl::lt(&g(&size), &n(65536)) { g(&size) } else { n(65536) };
        // scan through entire binary table
        s(&top_index, pl::int(&pl::div(&size_limit, &g(&increment))?));
        let (lo, hi) = (ttp.get("FIRST_ENTRY").to_i64(), pl::sub(&g(&top_index), &n(1)).to_i64());
        let mut t: Vec<Value> = (lo..=hi).map(n).collect();
        // add in floating point tag ID's if they exist
        let ftags: Vec<Value> = tag_table_keys_list(it, &ttp).into_iter().filter(|k| re(r"\.", "").is_match(k)).collect();
        if !ftags.is_empty() {
            t.extend(ftags);
            num_sort(&mut t, &|x| x.clone());
        }
        *tags.borrow_mut() = t;
    } else if di.get("DataMember").truthy() {
        *tags.borrow_mut() = di.get("DataMember").items();
        s(&verbose, n(0)); // no verbose output of extracted values when writing
    } else if di.get("MixedTags").truthy() {
        // process sorted integer-ID tags only
        let mut t: Vec<Value> = tag_table_keys_list(it, &ttp).into_iter().filter(|k| re(r"^\d+$", "").is_match(k)).collect();
        num_sort(&mut t, &|x| x.clone());
        *tags.borrow_mut() = t;
    } else {
        // extract known tags in numerical order
        let mut t = tag_table_keys_list(it, &ttp);
        num_sort(&mut t, &|x| if pl::lt(x, &n(0)) { pl::add(x, &Value::Num(1e9)) } else { x.clone() });
        *tags.borrow_mut() = t;
    }
    if g(&verbose).truthy() {
        let bo = pl::call(it, "Image::ExifTool::GetByteOrder", vec![])?;
        verbose_dir(it, vec![et.clone(), pl::s("BinaryData"), Value::Undef, g(&size), bo], Want::Void)?;
    }
    // avoid creating unknown tags for tags that fail condition if Unknown is 1
    if pl::lt(&g(&unknown), &n(2)) {
        et.set("NO_UNKNOWN", n(1));
    }
    let index = cell(Value::Undef);
    let val_h: Rc<RefCell<Hash>> = Rc::new(RefCell::new(Hash::default()));
    let next_index = cell(n(0));
    let var_size = cell(n(0));
    let tag_list = tags.borrow().clone();
    'tags: for idx in tag_list {
        s(&index, idx);
        let (tag_info, val, save_next_index, len, mask, was_var, rational, off_adj) = (cell(Value::Undef), cell(Value::Undef), cell(Value::Undef), cell(Value::Undef), cell(Value::Undef), cell(Value::Undef), cell(Value::Undef), cell(Value::Undef));
        // the lexicals an eval site of this loop sees
        let base_vars = |extra: &[(&'static str, &Rc<RefCell<Value>>)]| -> Vec<(&'static str, Rc<RefCell<Value>>)> {
            let mut v: Vec<(&'static str, Rc<RefCell<Value>>)> = vec![
                ("self", self_.clone()),
                ("dirInfo", dir_info.clone()),
                ("tagTablePtr", tag_table_ptr.clone()),
                ("dataPt", data_pt.clone()),
                ("dataLen", data_len.clone()),
                ("dirStart", dir_start.clone()),
                ("maxLen", max_len.clone()),
                ("size", size.clone()),
                ("base", base.clone()),
                ("verbose", verbose.clone()),
                ("unknown", unknown.clone()),
                ("dataPos", data_pos.clone()),
                ("defaultFormat", default_format.clone()),
                ("increment", increment.clone()),
                ("topIndex", top_index.clone()),
                ("binVal", bin_val.clone()),
                ("index", index.clone()),
                ("nextIndex", next_index.clone()),
                ("varSize", var_size.clone()),
                ("tagInfo", tag_info.clone()),
                ("val", val.clone()),
                ("saveNextIndex", save_next_index.clone()),
                ("len", len.clone()),
                ("mask", mask.clone()),
                ("wasVar", was_var.clone()),
                ("rational", rational.clone()),
                ("offAdj", off_adj.clone()),
            ];
            for (k, c) in extra {
                v.push((k, (*c).clone()));
            }
            v
        };
        let make_scope = |it: &mut Interp, vars: Vec<(&'static str, Rc<RefCell<Value>>)>| -> Rc<Scope> {
            let refs: Vec<(&str, &Rc<RefCell<Value>>)> = vars.iter().map(|(k, c)| (*k, c)).collect();
            let scope = scope_of(it, &refs);
            scope.declare(b'@', "tags", Binding::Array(tags.clone()));
            scope.declare(b'%', "val", Binding::Hash(val_h.clone()));
            scope.declare(b'@', "_", Binding::Array(args_all.clone()));
            scope
        };
        let index_v = g(&index);
        if ttp.get(index_v.to_bytes()).truthy() {
            s(&tag_info, get_tag_info_impl(it, vec![et.clone(), ttp.clone(), index_v.clone()])?);
            if !g(&tag_info).truthy() {
                if g(&tag_info).is_undef() {
                    continue;
                }
                // $entry = offset of value relative to directory start (or end if negative)
                let mut entry = pl::add(&pl::mul(&pl::int(&index_v), &g(&increment)), &g(&var_size));
                if pl::lt(&entry, &n(0)) {
                    entry = pl::add(&entry, &g(&size));
                    if pl::lt(&entry, &n(0)) {
                        continue;
                    }
                }
                if pl::ge(&entry, &g(&size)) {
                    continue;
                }
                let mut more = pl::sub(&g(&size), &entry);
                if pl::gt(&more, &n(128)) {
                    more = n(128);
                }
                let v = substr(it, &g(&data_pt).deref(), &pl::add(&entry, &g(&dir_start)), Some(&more))?;
                s(&tag_info, get_tag_info_impl(it, vec![et.clone(), ttp.clone(), index_v.clone(), Value::ScalarRef(cell(v))])?);
                if !g(&tag_info).truthy() {
                    continue;
                }
            }
            let ti = g(&tag_info);
            if ti.get("Unknown").truthy() && (pl::gt(&ti.get("Unknown"), &g(&unknown)) || pl::lt(&index_v, &g(&next_index))) {
                continue;
            }
        } else if g(&top_index).truthy() && ttp.get(pl::sub(&index_v, &g(&top_index)).to_bytes()).truthy() {
            s(&tag_info, get_tag_info_impl(it, vec![et.clone(), ttp.clone(), pl::sub(&index_v, &g(&top_index))])?);
            if !g(&tag_info).truthy() {
                continue;
            }
        } else {
            // don't generate unknown tags in binary tables unless Unknown > 1
            if !pl::gt(&g(&unknown), &n(1)) {
                continue;
            }
            if pl::lt(&index_v, &g(&next_index)) {
                continue; // skip if data already used
            }
            s(&tag_info, get_tag_info_impl(it, vec![et.clone(), ttp.clone(), index_v.clone()])?);
            if !g(&tag_info).truthy() {
                continue;
            }
            g(&tag_info).set("Unknown", n(2)); // set unknown to 2 for binary unknowns
        }
        let mut ti = g(&tag_info);
        // get relative offset of this entry
        let entry = cell(pl::add(&pl::mul(&pl::int(&index_v), &g(&increment)), &g(&var_size)));
        // allow negative indices to represent bytes from end
        if pl::lt(&g(&entry), &n(0)) {
            s(&entry, pl::add(&g(&entry), &g(&size)));
            if pl::lt(&g(&entry), &n(0)) {
                continue;
            }
        }
        let more = cell(pl::sub(&g(&size), &g(&entry)));
        if pl::le(&g(&more), &n(0)) {
            break; // all done if we have reached the end of data
        }
        let count = cell(n(1));
        let format = cell(ti.get("Format"));
        let loop_vars = |extra: &[(&'static str, &Rc<RefCell<Value>>)]| {
            let mut v = base_vars(&[("entry", &entry), ("more", &more), ("count", &count), ("format", &format)]);
            for (k, c) in extra {
                v.push((k, (*c).clone()));
            }
            v
        };
        if !g(&format).truthy() {
            s(&format, g(&default_format));
        } else if pl::str_eq(&g(&format), "string") {
            // string with no specified count runs to end of block
            s(&count, g(&more));
        } else if pl::str_eq(&g(&format), "pstring") {
            s(&format, pl::s("string"));
            let e = g(&entry);
            s(&entry, pl::add(&e, &n(1)));
            s(&count, pl::call(it, "Image::ExifTool::Get8u", vec![g(&data_pt), pl::add(&e, &g(&dir_start))])?);
            s(&more, pl::sub(&g(&more), &n(1)));
        } else if !format_size.get(g(&format).to_bytes()).truthy() {
            if let Some(m) = re(r"(.*)\[(.*)\]", "").m(&g(&format)) {
                // handle format count field
                s(&format, cap(&m, 1));
                s(&count, cap(&m, 2));
                // evaluate count to allow count to be based on previous values
                //### eval Format size (%val, $size, $varSize, $self)
                let scope = make_scope(it, loop_vars(&[]));
                let code = g(&count);
                let r = scalar_of(eval_site(it, &code, &scope, Want::Scalar));
                s(&count, r);
                let err = eval_error(it);
                if err.truthy() {
                    it.warn(cat(&[&pl::s("Format "), &ti.get("Name"), &pl::s(": "), &err]))?;
                    continue;
                }
                if pl::lt(&g(&count), &n(0)) {
                    continue;
                }
                // allow a variable-length value of any format
                // (note: the next incremental index points to data immediately after
                //  this value, regardless of the size of this value, even if it is zero)
                let mut f = g(&format);
                if re("^var_", "").subst(&mut f, false, |_| Vec::new()) > 0 {
                    s(&format, f);
                    let fs = or(format_size.get(g(&format).to_bytes()), n(1));
                    s(&var_size, pl::sub(&pl::add(&g(&var_size), &pl::mul(&g(&count), &fs)), &g(&increment)));
                    s(&was_var, n(1));
                    // save variable size data if required for writing
                    if di.get("VarFormatData").truthy() {
                        di.get_or_new_array("VarFormatData").push(Value::new_array(vec![g(&index), g(&var_size), g(&format)]));
                    }
                    // don't extract value if large and we wanted it just to get
                    // the variable-format information when writing
                    if ti.get("LargeTag").truthy() && di.get("VarFormatData").truthy() {
                        continue;
                    }
                }
            } else if re("^var_", "").is_match(&g(&format)) {
                // handle variable-length string formats
                s(&format, substr(it, &g(&format), &n(4), None)?);
                let pscope = make_scope(it, loop_vars(&[]));
                snippet(it, "pos($$dataPt) = $entry + $dirStart;", &pscope, Want::Void)?;
                s(&count, Value::Undef);
                let fmt = g(&format);
                if pl::str_eq(&fmt, "ustring") {
                    let r = scalar_of(snippet(it, "($$dataPt =~ /\\G(..)*?\\0\\0/sg) ? [pos($$dataPt)] : undef", &pscope, Want::Scalar)?);
                    if r.truthy() {
                        s(&count, pl::sub(&r.at(0), &pl::add(&g(&entry), &g(&dir_start))));
                    }
                    s(&var_size, pl::sub(&g(&var_size), &n(2))); // ($count includes base size of 2 bytes)
                } else if pl::str_eq(&fmt, "pstring") {
                    let e = g(&entry);
                    s(&entry, pl::add(&e, &n(1)));
                    s(&count, pl::call(it, "Image::ExifTool::Get8u", vec![g(&data_pt), pl::add(&e, &g(&dir_start))])?);
                    s(&more, pl::sub(&g(&more), &n(1)));
                } else if pl::str_eq(&fmt, "pstr32") || pl::str_eq(&fmt, "ustr32") {
                    if pl::lt(&g(&more), &n(4)) {
                        break;
                    }
                    s(&count, pl::call(it, "Image::ExifTool::Get32u", vec![g(&data_pt), pl::add(&g(&entry), &g(&dir_start))])?);
                    if pl::str_eq(&fmt, "ustr32") {
                        s(&count, pl::mul(&g(&count), &n(2)));
                    }
                    s(&entry, pl::add(&g(&entry), &n(4)));
                    s(&more, pl::sub(&g(&more), &n(4)));
                    s(&next_index, pl::add(&g(&next_index), &pl::div(&n(4), &g(&increment))?)); // (increment next index for int32u)
                } else if pl::str_eq(&fmt, "int16u") {
                    // int16u size of binary data to follow
                    if pl::lt(&g(&more), &n(2)) {
                        break;
                    }
                    s(&count, pl::add(&pl::call(it, "Image::ExifTool::Get16u", vec![g(&data_pt), pl::add(&g(&entry), &g(&dir_start))])?, &n(2)));
                    s(&var_size, pl::sub(&g(&var_size), &n(2))); // ($count includes size word)
                    s(&format, pl::s("undef"));
                } else if pl::str_eq(&fmt, "ue7") {
                    runtime::require(it, "Image::ExifTool::BPG")?;
                    let r = pl::call_list(it, "Image::ExifTool::BPG::Get_ue7", vec![g(&data_pt), pl::add(&g(&entry), &g(&dir_start))])?;
                    s(&val, arg(&r, 0));
                    s(&count, arg(&r, 1));
                    if g(&val).is_undef() {
                        break;
                    }
                    s(&var_size, pl::sub(&g(&var_size), &n(1))); // ($count includes base size of 1 byte)
                } else {
                    let r = scalar_of(snippet(it, "($$dataPt =~ /\\0/g) ? [pos($$dataPt)] : undef", &pscope, Want::Scalar)?);
                    if r.truthy() {
                        s(&count, pl::sub(&r.at(0), &pl::add(&g(&entry), &g(&dir_start))));
                        s(&var_size, pl::sub(&g(&var_size), &n(1))); // ($count includes base size of 1 byte)
                    }
                }
                if g(&count).is_undef() || pl::gt(&g(&count), &g(&more)) {
                    s(&count, g(&more));
                }
                s(&var_size, pl::add(&g(&var_size), &g(&count))); // shift subsequent indices
                if g(&val).is_undef() {
                    let mut v = substr(it, &g(&data_pt).deref(), &pl::add(&g(&entry), &g(&dir_start)), Some(&g(&count)))?;
                    let fmt = g(&format);
                    if pl::str_eq(&fmt, "ustring") || pl::str_eq(&fmt, "ustr32") {
                        v = pl::method(it, &et, "Decode", vec![v, pl::s("UTF16")])?;
                    }
                    if !pl::str_eq(&fmt, "undef") {
                        re(r"\0.*", "s").subst(&mut v, false, |_| Vec::new()); // truncate at null
                    }
                    s(&val, v);
                }
                if et.path(&["OPTIONS", "SaveBin"]).truthy() {
                    s(&bin_val, substr(it, &g(&data_pt).deref(), &pl::add(&g(&entry), &g(&dir_start)), Some(&g(&count)))?);
                }
                s(&was_var, n(1));
                // save variable size data if required for writing
                if di.get("VarFormatData").truthy() {
                    di.get_or_new_array("VarFormatData").push(Value::new_array(vec![g(&index), g(&var_size), g(&format)]));
                }
            }
        }
        // hook to allow format, etc to be set dynamically
        if !ti.get("Hook").is_undef() {
            let old_var_size = cell(g(&var_size));
            let pos = cell(pl::add(&g(&entry), &g(&dir_start)));
            //### eval Hook ($format, $varSize, $size, $dataPt, $pos)
            let scope = make_scope(it, loop_vars(&[("oldVarSize", &old_var_size), ("pos", &pos)]));
            eval_site(it, &ti.get("Hook"), &scope, Want::Void);
            ti = g(&tag_info);
            // save variable size data if required for writing (in case changed by Hook)
            if di.get("VarFormatData").truthy() {
                let vfd = di.get_or_new_array("VarFormatData");
                if g(&was_var).truthy() {
                    // remove previous entry for this tag
                    if let Value::ArrayRef(arr) = &vfd {
                        let l = arr.borrow().len();
                        arr.borrow_mut().truncate(l.saturating_sub(1));
                    }
                }
                vfd.push(Value::new_array(vec![g(&index), g(&var_size), g(&format)]));
            } else if !pl::num_eq(&g(&var_size), &g(&old_var_size)) && pl::gt(&g(&verbose), &n(2)) {
                let (mut tmp, mut sign) = (g(&var_size), "+");
                if pl::lt(&tmp, &n(0)) {
                    tmp = pl::neg(&tmp);
                    sign = "-";
                }
                let fmt = cat(&[&et.get("INDENT"), &pl::s("[offsets adjusted by "), &pl::s(sign), &pl::s("0x%.4x after 0x%.4x "), &ti.get("Name"), &pl::s("]\n")]);
                s(&off_adj, pl::sprintf_v(&fmt, &[tmp, g(&index)]));
            }
        }
        if pl::gt(&g(&unknown), &n(1)) {
            // calculate next valid index for unknown tag
            let mut ni = pl::int(&g(&index));
            if !g(&was_var).truthy() {
                let fs = or(format_size.get(g(&format).to_bytes()), n(1));
                ni = pl::add(&ni, &pl::div(&pl::mul(&fs, &g(&count)), &g(&increment))?);
            }
            s(&save_next_index, g(&next_index));
            if !pl::gt(&g(&next_index), &ni) {
                s(&next_index, ni);
            }
        }
        // allow large tags to be excluded from extraction
        // (provides a work-around for some tight memory situations)
        if ti.get("LargeTag").truthy() && vget(&et, &[b"EXCL_TAG_LOOKUP", &lc(&ti.get("Name")).to_bytes()]).truthy() {
            continue;
        }
        // read value now if necessary
        if !(!g(&val).is_undef() && !ti.get("SubDirectory").truthy()) {
            let v = pl::call(it, "Image::ExifTool::ReadValue", vec![g(&data_pt), pl::add(&g(&entry), &g(&dir_start)), g(&format), g(&count), g(&more), Value::ScalarRef(rational.clone())])?;
            s(&val, v);
            if g(&val).is_undef() {
                continue;
            }
            s(&mask, ti.get("Mask"));
            if g(&mask).truthy() {
                s(&val, ops::shift_right(&ops::bit_and(&g(&val), &g(&mask)), &ti.get("BitShift")));
            }
        }
        if g(&verbose).truthy() && !ti.get("Hidden").truthy() {
            if !ti.get("SubDirectory").truthy() || ti.get("Format").truthy() {
                let fs = or(format_size.get(g(&format).to_bytes()), n(1));
                s(&len, pl::mul(&g(&count), &fs));
                if pl::gt(&g(&len), &g(&more)) {
                    s(&len, g(&more));
                }
            } else {
                s(&len, g(&more));
            }
            let start = pl::add(&g(&entry), &g(&dir_start));
            let extra = if g(&mask).truthy() { pl::sprintf(", mask 0x%.2x", &[g(&mask)]) } else { Value::Undef };
            let args = vec![
                g(&index),
                ti.clone(),
                pl::s("Table"),
                ttp.clone(),
                pl::s("Value"),
                g(&val),
                pl::s("DataPt"),
                g(&data_pt),
                pl::s("Size"),
                g(&len),
                pl::s("Start"),
                start.clone(),
                pl::s("Addr"),
                pl::add(&pl::add(&start, &g(&base)), &g(&data_pos)),
                pl::s("Format"),
                g(&format),
                pl::s("Count"),
                g(&count),
                pl::s("Extra"),
                extra,
            ];
            meth(it, &et, "VerboseInfo", args, Want::Void)?;
        }
        if g(&off_adj).truthy() {
            vprint(it, &et, &n(2), vec![g(&off_adj)])?;
        }
        // parse nested BinaryData directories
        if ti.get("SubDirectory").truthy() {
            let subdir = cell(ti.get("SubDirectory"));
            let sd = g(&subdir);
            let sub_table_ptr = cell(pl::call(it, "Image::ExifTool::GetTagTable", vec![sd.get("TagTable")])?);
            // use specified subdirectory length if given
            if ti.get("Format").truthy() && format_size.get(g(&format).to_bytes()).truthy() {
                s(&len, pl::mul(&g(&count), &format_size.get(g(&format).to_bytes())));
                if pl::gt(&g(&len), &g(&more)) {
                    s(&len, g(&more));
                }
            } else {
                s(&len, g(&more)); // directory size is all of remaining data
                let stp = g(&sub_table_ptr);
                if stp.get("PROCESS_PROC").truthy() && seq(&stp.get("PROCESS_PROC"), &code_ref(it, "Image::ExifTool::ProcessBinaryData")) {
                    // the rest of the data will be printed in the subdirectory
                    s(&next_index, pl::div(&g(&size), &g(&increment))?);
                }
            }
            let subdir_base = cell(g(&base));
            if !sd.get("Base").is_undef() {
                //### eval Base ($start,$base)
                let start = cell(pl::add(&pl::add(&g(&entry), &g(&dir_start)), &g(&data_pos)));
                let scope = make_scope(it, loop_vars(&[("subdir", &subdir), ("subTablePtr", &sub_table_ptr), ("subdirBase", &subdir_base), ("start", &start)]));
                let r = scalar_of(eval_site(it, &sd.get("Base"), &scope, Want::Scalar));
                s(&subdir_base, pl::add(&r, &g(&base)));
            }
            let start = cell(or(sd.get("Start"), n(0)));
            let not_dup = cell(Value::Undef);
            if re(r"\$", "").is_match(&g(&start)) {
                // ignore directories with a zero offset (ie. missing Nikon ShotInfo entries)
                if !g(&val).truthy() {
                    continue 'tags;
                }
                //### eval Start ($val, $dirStart)
                let scope = make_scope(it, loop_vars(&[("subdir", &subdir), ("subTablePtr", &sub_table_ptr), ("subdirBase", &subdir_base), ("start", &start), ("notDup", &not_dup)]));
                let code = g(&start);
                let r = scalar_of(eval_site(it, &code, &scope, Want::Scalar));
                s(&start, r);
                if pl::lt(&g(&start), &g(&dir_start)) || pl::gt(&g(&start), &g(&data_len)) {
                    continue 'tags;
                }
                s(&len, sd.get("DirLen"));
                let room = pl::sub(&g(&data_len), &g(&start));
                if !(g(&len).truthy() && pl::le(&g(&len), &room)) {
                    s(&len, room);
                }
            } else {
                s(&start, pl::add(&g(&start), &pl::add(&g(&dir_start), &g(&entry))));
                s(&not_dup, n(1));
            }
            let subdir_info = Value::new_hash();
            subdir_info.set("DataPt", g(&data_pt));
            subdir_info.set("DataPos", g(&data_pos));
            subdir_info.set("DataLen", g(&data_len));
            subdir_info.set("DirStart", g(&start));
            subdir_info.set("DirLen", g(&len));
            subdir_info.set("Base", g(&subdir_base));
            subdir_info.set("NotDup", g(&not_dup));
            et.delete("NO_UNKNOWN");
            pl::method(it, &et, "ProcessDirectory", vec![subdir_info, g(&sub_table_ptr), sd.get("ProcessProc")])?;
            if pl::lt(&g(&unknown), &n(2)) {
                et.set("NO_UNKNOWN", n(1));
            }
            continue;
        }
        if ti.get("IsOffset").truthy() && !pl::str_eq(&ti.get("IsOffset"), "3") {
            let et_c = cell(et.clone());
            //### eval IsOffset ($val, $et)
            let scope = make_scope(it, loop_vars(&[("et", &et_c)]));
            let r = scalar_of(eval_site(it, &ti.get("IsOffset"), &scope, Want::Scalar));
            if r.truthy() {
                s(&val, pl::add(&g(&val), &pl::add(&g(&base), &et.get("BASE"))));
            }
        }
        val_h.borrow_mut().insert(&g(&index).to_bytes(), g(&val));
        let mut old_base = Value::Undef;
        if ti.get("SetBase").truthy() {
            old_base = et.get("BASE");
            et.set("BASE", pl::add(&et.get("BASE"), &g(&base)));
        }
        let key = found_tag_call(it, &et, vec![ti.clone(), g(&val)])?;
        if !old_base.is_undef() {
            et.set("BASE", old_base);
        }
        if key.truthy() {
            let r = g(&rational);
            if !r.is_undef() {
                et.get_or_new_hash("TAG_EXTRA").get_or_new_hash(key.to_bytes()).set("Rational", r);
            }
            let b = g(&bin_val);
            if !b.is_undef() {
                et.get_or_new_hash("TAG_EXTRA").get_or_new_hash(key.to_bytes()).set("BinVal", b);
            }
        } else {
            // don't increment nextIndex if we didn't extract a tag
            if !g(&save_next_index).is_undef() {
                s(&next_index, g(&save_next_index));
            }
        }
    }
    et.delete("NO_UNKNOWN");
    ret(n(1))
}

/// `$raf->Read($buff, $len)`, where `$buff` holds `initial` before the call
/// (Read writes its first argument through `@_`): the result and the new
/// value of the buffer.
fn raf_read(it: &mut Interp, raf: &Value, initial: Value, len: &Value) -> ER<(Value, Value)> {
    let scope = Scope::new(None);
    scope.scalar("raf", raf.clone());
    scope.scalar("len", len.clone());
    let buff = scope.scalar("buff", initial);
    let r = scalar_of(snippet(it, "$raf->Read($buff, $len)", &scope, Want::Scalar)?);
    let b = buff.borrow().clone();
    Ok((r, b))
}

/// `\$$h{key}`: a reference to a hash element.
fn elem_ref(h: &Value, key: &str) -> Value {
    match h {
        Value::HashRef(hr) => Value::LvRef(Rc::new(crate::interp::eval::Lv::HElem(hr.clone(), Rc::from(key.as_bytes())))),
        _ => Value::Undef,
    }
}

/// `$$r = $v` on a scalar reference.
fn set_deref(r: &Value, v: Value) -> ER<()> {
    match r {
        Value::ScalarRef(c) => {
            *c.borrow_mut() = v;
            Ok(())
        }
        Value::LvRef(l) => l.set(v),
        _ => Ok(()),
    }
}

/// `Write($outfile, ...)` (Writer.pl)
fn write_out(it: &mut Interp, outfile: &Value, data: Vec<Value>) -> ER<Value> {
    let mut a = vec![outfile.clone()];
    a.extend(data);
    pl::call(it, "Image::ExifTool::Write", a)
}

/// Process TIFF data
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) optional tag table ref
/// Returns: 1 if this looked like a valid EXIF block, 0 otherwise, or -1 on write error
pub fn do_process_tiff(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, dir_info, mut tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let mut data_pt = dir_info.get("DataPt");
    let mut file_type = or(dir_info.get("Parent"), pl::s(""));
    let mut raf = dir_info.get("RAF");
    let base = or(dir_info.get("Base"), n(0));
    let outfile = dir_info.get("OutFile");
    let (mut err, mut sig, mut canon_sig, mut other_sig) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef);
    let set_err = |e: &mut Value, r: &Value| {
        if !r.truthy() {
            *e = n(1);
        }
    };

    // attempt to read TIFF header
    if raf.truthy() {
        self_.set("EXIF_DATA", pl::s(""));
        if outfile.truthy() {
            if !pl::method(it, &raf, "Seek", vec![n(0), n(0)])?.truthy() {
                return ret(n(0));
            }
            if base.truthy() {
                let (r, b) = raf_read(it, &raf, data_pt.deref(), &base)?;
                set_deref(&data_pt, b)?;
                if !pl::num_eq(&r, &base) {
                    return ret(n(0));
                }
                let r = write_out(it, &outfile, vec![data_pt.deref()])?;
                set_err(&mut err, &r);
            }
        } else if !pl::method(it, &raf, "Seek", vec![base.clone(), n(0)])?.truthy() {
            return ret(n(0));
        }
        // extract full EXIF block (for block copy) from EXIF file
        let amount = if pl::str_eq(&file_type, "EXIF") { n(65536 * 8) } else { n(8) };
        let (nr, b) = raf_read(it, &raf, self_.get("EXIF_DATA"), &amount)?;
        self_.set("EXIF_DATA", b);
        if pl::lt(&nr, &n(8)) {
            if nr.truthy() || !outfile.truthy() || !pl::str_eq(&file_type, "EXIF") {
                return ret(n(0));
            }
            // create EXIF file from scratch
            self_.delete("EXIF_DATA");
            raf = Value::Undef;
        }
        if pl::gt(&nr, &n(8)) {
            pl::method(it, &raf, "Seek", vec![n(8), n(0)])?;
            if pl::num_eq(&nr, &amount) {
                let d = substr(it, &self_.get("EXIF_DATA"), &n(0), Some(&n(8)))?;
                self_.set("EXIF_DATA", d);
                pl::method(it, &self_, "Warn", vec![pl::s("EXIF too large to extract as a block")])?; //(shouldn't happen)
            }
        }
    } else if data_pt.truthy() && pl::length(&data_pt.deref()).truthy() {
        // save a copy of the EXIF data
        let dir_start = or(dir_info.get("DirStart"), n(0));
        let dir_len = or(dir_info.get("DirLen"), pl::sub(&len_of(&data_pt.deref()), &dir_start));
        if pl::gt(&dir_len, &n(0)) || !outfile.truthy() {
            let d = substr(it, &data_pt.deref(), &dir_start, Some(&dir_len))?;
            self_.set("EXIF_DATA", d);
        } else {
            self_.delete("EXIF_DATA"); // create from scratch;
        }
        if self_.path(&["OPTIONS", "Verbose"]).truthy() && pl::gt(&len_of(&self_.get("INDENT")), &n(2)) {
            verbose_dir(it, vec![self_.clone(), pl::s("TIFF")], Want::Void)?;
        }
    } else if outfile.truthy() {
        self_.delete("EXIF_DATA"); // create from scratch
    } else {
        self_.set("EXIF_DATA", pl::s(""));
    }
    if self_.get("EXIF_DATA").is_undef() {
        // set default byte order for creating new GPS in CR3 images
        let mut default_byte_order = Value::Undef;
        if dir_info.get("DirName").truthy() && pl::str_eq(&dir_info.get("DirName"), "GPS") {
            default_byte_order = self_.get("SaveExifByteOrder");
        }
        // create TIFF information from scratch
        if pl::str_eq(&pl::method(it, &self_, "SetPreferredByteOrder", vec![default_byte_order])?, "MM") {
            self_.set("EXIF_DATA", Value::bytes(b"MM\0\x2a\0\0\0\x08"));
        } else {
            self_.set("EXIF_DATA", Value::bytes(b"II\x2a\0\x08\0\0\0"));
        }
    }
    self_.set("EXIF_POS", pl::add(&base, &self_.get("BASE")));
    if self_.get("FIRST_EXIF_POS").is_undef() {
        self_.set("FIRST_EXIF_POS", self_.get("EXIF_POS"));
    }
    data_pt = elem_ref(&self_, "EXIF_DATA");

    // set byte ordering
    let byte_order = substr(it, &data_pt.deref(), &n(0), Some(&n(2)))?;
    if !pl::call(it, "Image::ExifTool::SetByteOrder", vec![byte_order.clone()])?.truthy() {
        return ret(n(0));
    }

    // verify the byte ordering
    let identifier = pl::call(it, "Image::ExifTool::Get16u", vec![data_pt.clone(), n(2)])?;
    // identifier is 0x2a for TIFF (but 0x4f52, 0x5352 or ?? for ORF)
    // no longer do this because various files use different values
    // (TIFF=0x2a, RW2/RWL=0x55, HDP=0xbc, BTF=0x2b, ORF=0x4f52/0x5352/0x????)
    //  return 0 unless $identifier == 0x2a;
    if pl::str_eq(&file_type, "APP1") && !pl::num_eq(&identifier, &n(0x2a)) {
        pl::method(it, &self_, "Warn", vec![pl::s("Invalid magic number in EXIF TIFF header")])?;
    }

    // get offset to IFD0
    if pl::lt(&len_of(&data_pt.deref()), &n(8)) {
        return ret(n(0));
    }
    let offset = pl::call(it, "Image::ExifTool::Get32u", vec![data_pt.clone(), n(4)])?;
    if !pl::ge(&offset, &n(8)) {
        return ret(n(0));
    }

    if raf.truthy() {
        // check for canon or EXIF signature
        // (Canon CR2 images should have an offset of 16, but it may be
        //  greater if edited by PhotoMechanic)
        if pl::num_eq(&identifier, &n(0x2a)) && pl::ge(&offset, &n(16)) {
            let (r, b) = raf_read(it, &raf, sig.clone(), &n(8))?;
            sig = b;
            if !pl::num_eq(&r, &n(8)) {
                return ret(n(0));
            }
            set_deref(&data_pt, pl::concat(&data_pt.deref(), &sig))?;
            if re("^(CR\\x02\\0|\\xba\\xb0\\xac\\xbb|ExifMeta)", "").is_match(&sig) {
                if pl::str_eq(&sig, "ExifMeta") {
                    file_type = pl::s("EXIF");
                    pl::method(it, &self_, "SetFileType", vec![file_type.clone()])?;
                    other_sig = sig.clone();
                } else {
                    file_type = pl::s(if re("^CR", "").is_match(&sig) { "CR2" } else { "Canon 1D RAW" });
                    canon_sig = sig.clone();
                }
                if self_.get("HTML_DUMP").truthy() {
                    pl::method(it, &self_, "HDump", vec![pl::add(&base, &n(8)), n(8), cat(&[&pl::s("["), &file_type, &pl::s(" header]")])])?;
                }
            }
        } else if pl::num_eq(&identifier, &n(0x55)) && re("^(RAW|RW2|RWL|TIFF)$", "").is_match(&file_type) {
            // panasonic RAW, RW2 or RWL file
            let mut magic = Value::Undef;
            // test for RW2/RWL magic number
            let mut ok = false;
            if pl::ge(&offset, &n(0x18)) {
                let (r, b) = raf_read(it, &raf, magic.clone(), &n(16))?;
                magic = b;
                ok = r.truthy() && &magic.to_bytes()[..] == b"\x88\xe7\x74\xd8\xf8\x25\x1d\x4d\x94\x7a\x6e\x77\x82\x2b\x5d\x6a";
            }
            if ok {
                if !pl::str_eq(&file_type, "RWL") {
                    file_type = pl::s("RW2");
                }
                if self_.get("HTML_DUMP").truthy() {
                    pl::method(it, &self_, "HDump", vec![pl::add(&base, &n(8)), n(16), pl::s("[RW2/RWL header]")])?;
                }
                other_sig = magic; // save signature for writing
            } else {
                file_type = pl::s("RAW");
            }
            tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![pl::s("Image::ExifTool::PanasonicRaw::Main")])?;
        } else if pl::str_eq(&file_type, "TIFF") {
            if pl::num_eq(&identifier, &n(0x2b)) {
                // this looks like a BigTIFF image
                pl::method(it, &raf, "Seek", vec![n(0)])?;
                runtime::require(it, "Image::ExifTool::BigTIFF")?;
                let result = pl::call(it, "Image::ExifTool::BigTIFF::ProcessBTF", vec![self_.clone(), dir_info.clone()])?;
                if result.truthy() {
                    if self_.get("MultiPage").truthy() {
                        found_tag_call(it, &self_, vec![pl::s("PageCount"), self_.get("PageCount")])?;
                    }
                    return ret(n(1));
                }
            } else if pl::num_eq(&identifier, &n(0x4f52)) || pl::num_eq(&identifier, &n(0x5352)) {
                // Olympus ORF image (set FileType now because base type is 'ORF')
                file_type = pl::s("ORF");
                pl::method(it, &self_, "SetFileType", vec![file_type.clone()])?;
            } else if pl::num_eq(&identifier, &n(0x4352)) {
                file_type = pl::s("DCP");
            } else if pl::str_eq(&byte_order, "II") && pl::num_eq(&crate::perl::ops::bit_and(&identifier, &n(0xff)), &n(0xbc)) {
                file_type = pl::s("HDP"); // Windows HD Photo file
                // check version number
                let ver = pl::call(it, "Image::ExifTool::Get8u", vec![data_pt.clone(), n(3)])?;
                if pl::gt(&ver, &n(1)) {
                    let msg = cat(&[&pl::s("Windows HD Photo version "), &ver, &pl::s(" files not yet supported")]);
                    pl::method(it, &self_, "Error", vec![msg])?;
                    return ret(n(1));
                }
            }
        } else if pl::str_eq(&file_type, "ARW") {
            self_.get_or_new_hash("LOW_PRIORITY_DIR").set("IFD1", n(1)); // lower priority of IFD1 tags in ARW files
        }
        // we have a valid TIFF (or whatever) file
        if file_type.truthy() && !vget(&self_, &[b"VALUE", b"FileType"]).truthy() {
            let file_type_lookup = pkg_hash(it, "fileTypeLookup");
            let mut lookup = file_type_lookup.get(file_type.to_bytes());
            if !(lookup.is_ref() || !lookup.truthy()) {
                lookup = file_type_lookup.get(lookup.to_bytes());
            }
            // use file extension to pre-determine type if extension is TIFF-based or type is RAW
            let base_type = if lookup.truthy() {
                if lookup.at(0).is_ref() {
                    lookup.at(0).at(0)
                } else {
                    lookup.at(0)
                }
            } else {
                pl::s("")
            };
            let t = if pl::str_eq(&base_type, "TIFF") || re("RAW", "").is_match(&file_type) { file_type.clone() } else { Value::Undef };
            pl::method(it, &self_, "SetFileType", vec![t])?;
        }
        // don't process file if FastScan > 2
        if !outfile.truthy() && self_.path(&["OPTIONS", "FastScan"]).truthy() && pl::gt(&self_.path(&["OPTIONS", "FastScan"]), &n(2)) {
            return ret(n(1));
        }
    }
    // (accommodate CR3 images which have a TIFF directory with ExifIFD at the top level)
    let mut ifd_name = pl::s("IFD0");
    if dir_info.get("DirName").truthy() {
        if let Some(m) = re("^(ExifIFD|GPS)$", "").m(&dir_info.get("DirName")) {
            ifd_name = cap(&m, 1);
        }
    }
    if !tag_table_ptr.truthy() || pl::str_eq(&vget(&tag_table_ptr, &[b"GROUPS", b"0"]), "EXIF") {
        if !outfile.truthy() {
            found_tag_call(it, &self_, vec![pl::s("ExifByteOrder"), byte_order.clone()])?;
        }
        self_.set("ExifByteOrder", byte_order.clone());
    } else if pl::str_eq(&vget(&tag_table_ptr, &[b"GROUPS", b"0"]), "MakerNotes") {
        // (for writing CR3 maker notes)
        ifd_name = vget(&tag_table_ptr, &[b"GROUPS", b"0"]);
    } else {
        ifd_name = vget(&tag_table_ptr, &[b"GROUPS", b"1"]);
    }
    if self_.get("HTML_DUMP").truthy() {
        let fmt = cat(&[&pl::s("Byte order: %s endian\nIdentifier: 0x%.4x\n"), &ifd_name, &pl::s(" offset: 0x%.4x")]);
        let tip = pl::sprintf_v(&fmt, &[pl::s(if pl::str_eq(&byte_order, "II") { "Little" } else { "Big" }), identifier.clone(), offset.clone()]);
        pl::method(it, &self_, "HDump", vec![base.clone(), n(8), pl::s("TIFF header"), tip, n(0)])?;
    }
    // remember where we found the TIFF data (APP1, APP3, TIFF, NEF, etc...)
    self_.set("TIFF_TYPE", file_type.clone());

    // get reference to the main EXIF table
    if !tag_table_ptr.truthy() {
        tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![pl::s("Image::ExifTool::Exif::Main")])?;
    }

    // build directory information hash
    let dinfo = Value::new_hash();
    dinfo.set("Base", base.clone());
    dinfo.set("DataPt", data_pt.clone());
    dinfo.set("DataLen", len_of(&data_pt.deref()));
    dinfo.set("DataPos", n(0));
    dinfo.set("DirStart", offset.clone());
    dinfo.set("DirLen", pl::sub(&len_of(&data_pt.deref()), &offset));
    dinfo.set("RAF", raf.clone());
    dinfo.set("DirName", ifd_name.clone());
    dinfo.set("Parent", file_type.clone());
    dinfo.set("ImageData", pl::s("Main")); // set flag to get information to copy main image data later
    dinfo.set("Multi", dir_info.get("Multi"));

    // extract information from the image
    if !outfile.truthy() {
        // process the directory
        pl::method(it, &self_, "ProcessDirectory", vec![dinfo.clone(), tag_table_ptr.clone()])?;
        // process GeoTiff information if available
        if vget(&self_, &[b"VALUE", b"GeoTiffDirectory"]).truthy() {
            runtime::require(it, "Image::ExifTool::GeoTiff")?;
            pl::call(it, "Image::ExifTool::GeoTiff::ProcessGeoTiff", vec![self_.clone()])?;
        }
        // process information in recognized trailers
        if raf.truthy() {
            let trail_info = pl::method(it, &self_, "IdentifyTrailer", vec![raf.clone()])?;
            if trail_info.truthy() {
                // scan to find AFCP if necessary (Note: we are scanning
                // from a random file position in the TIFF)
                trail_info.set("ScanForTrailer", n(1));
                pl::method(it, &self_, "ProcessTrailers", vec![trail_info.clone()])?;
            }
            // dump any other known trailer (eg. A100 RAW Data)
            if self_.get("HTML_DUMP").truthy() && self_.get("KnownTrailer").truthy() {
                let known = self_.get("KnownTrailer");
                pl::method(it, &raf, "Seek", vec![n(0), n(2)])?;
                let mut len = pl::sub(&pl::method(it, &raf, "Tell", vec![])?, &known.get("Start"));
                if trail_info.truthy() {
                    len = pl::sub(&len, &trail_info.get("Offset")); // account for other trailers
                }
                if pl::gt(&len, &n(0)) {
                    pl::method(it, &self_, "HDump", vec![known.get("Start"), len, cat(&[&pl::s("["), &known.get("Name"), &pl::s("]")])])?;
                }
            }
        }
        // update FileType if necessary now that we know more about the file
        if self_.get("DNGVersion").truthy() && pl::str_eq(&self_.get("FILE_TYPE"), "TIFF") && !re("^(DNG|GPR)$", "").is_match(&self_.get("FileType")) {
            // override whatever FileType we set since we now know it is DNG
            self_.set("TIFF_TYPE", pl::s("DNG"));
            pl::method(it, &self_, "OverrideFileType", vec![pl::s("DNG")])?;
        }
        if pl::str_eq(&self_.get("TIFF_TYPE"), "TIFF") {
            if self_.get("MultiPage").truthy() {
                found_tag_call(it, &self_, vec![pl::s("PageCount"), self_.get("PageCount")])?;
            }
        } else if pl::str_eq(&self_.get("TIFF_TYPE"), "NRW") && vget(&self_, &[b"VALUE", b"NEFLinearizationTable"]).truthy() {
            // fix NEF type if misidentified as NRW
            self_.set("TIFF_TYPE", pl::s("NEF"));
            pl::method(it, &self_, "OverrideFileType", vec![pl::s("NEF")])?;
        }
        if self_.get("ImageDataHash").truthy() && self_.get("A100DataOffset").truthy() && pl::method(it, &raf, "Seek", vec![self_.get("A100DataOffset"), n(0)])?.truthy() {
            pl::method(it, &self_, "ImageDataHash", vec![raf.clone(), Value::Undef, pl::s("A100")])?;
        }
        return ret(n(1));
    }
//
// rewrite the image
//
    if dir_info.get("NoTiffEnd").truthy() {
        self_.delete("TIFF_END");
    } else {
        // initialize TIFF_END so it will be updated by WriteExif()
        self_.set("TIFF_END", n(0));
    }
    if canon_sig.truthy() {
        // write Canon CR2 specially because it has a header we want to preserve,
        // and possibly trailers added by the Canon utilities and/or PhotoMechanic
        dinfo.set("OutFile", outfile.clone());
        runtime::require(it, "Image::ExifTool::CanonRaw")?;
        let r = pl::call(it, "Image::ExifTool::CanonRaw::WriteCR2", vec![self_.clone(), dinfo.clone(), tag_table_ptr.clone()])?;
        set_err(&mut err, &r);
    } else {
        // write TIFF header (8 bytes [plus optional signature] followed by IFD)
        if pl::str_eq(&file_type, "EXIF") {
            other_sig = pl::s("ExifMeta"); // force this signature for all EXIF files
        } else if other_sig.is_undef() {
            other_sig = pl::s("");
        }
        let offset = pl::add(&n(8), &len_of(&other_sig));
        // construct tiff header
        let h4 = substr(it, &data_pt.deref(), &n(0), Some(&n(4)))?;
        let s32 = pl::call(it, "Image::ExifTool::Set32u", vec![offset.clone()])?;
        let header = cell(cat(&[&h4, &s32, &other_sig]));
        dinfo.set("NewDataPos", offset.clone());
        dinfo.set("HeaderPtr", Value::ScalarRef(header.clone()));
        // preserve padding between image data blocks in ORF images
        // (otherwise dcraw has problems because it assumes fixed block spacing)
        if pl::str_eq(&file_type, "ORF") || !pl::num_eq(&identifier, &n(0x2a)) {
            dinfo.set("PreserveImagePadding", n(1));
        }
        let mut new_data = pl::method(it, &self_, "WriteDirectory", vec![dinfo.clone(), tag_table_ptr.clone()])?;
        if new_data.is_undef() {
            err = n(1);
        } else if pl::length(&new_data).truthy() {
            // update header length in case more was added
            let hdr_len = len_of(&g(&header));
            if !pl::num_eq(&hdr_len, &n(8)) {
                pl::call(it, "Image::ExifTool::Set32u", vec![hdr_len.clone(), Value::ScalarRef(header.clone()), n(4)])?;
                // also update preview fixup if necessary
                let pi = self_.get("PREVIEW_INFO");
                if pi.truthy() && pi.get("Fixup").truthy() {
                    let fx = pi.get("Fixup");
                    fx.set("Start", pl::add(&fx.get("Start"), &pl::sub(&hdr_len, &n(8))));
                }
            }
            if pl::str_eq(&self_.get("TIFF_TYPE"), "ARW") && !err.truthy() {
                // write any required ARW trailer and patch other ARW quirks
                runtime::require(it, "Image::ExifTool::Sony")?;
                let nd = cell(new_data.clone());
                let err_str = pl::call(it, "Image::ExifTool::Sony::FinishARW", vec![self_.clone(), dir_info.clone(), Value::ScalarRef(nd.clone()), dinfo.get("ImageData")])?;
                new_data = g(&nd);
                if err_str.truthy() {
                    pl::method(it, &self_, "Error", vec![err_str])?;
                }
                dinfo.delete("ImageData"); // (was copied by FinishARW)
            } else {
                let r = write_out(it, &outfile, vec![g(&header), new_data.clone()])?;
                set_err(&mut err, &r);
            }
            let _ = new_data; // free memory
        }
        // copy over image data now if necessary
        if dinfo.get("ImageData").is_ref() && !err.truthy() {
            let r = pl::method(it, &self_, "CopyImageData", vec![dinfo.get("ImageData"), outfile.clone()])?;
            set_err(&mut err, &r);
            dinfo.delete("ImageData");
        }
    }
    // make local copy of TIFF_END now (it may be reset when processing trailers)
    let mut tiff_end = self_.get("TIFF_END");
    self_.delete("TIFF_END");

    // rewrite trailers if they exist
    if raf.truthy() && tiff_end.truthy() && !err.truthy() {
        let mut buf = Value::Undef;
        let mut trail_info = Value::Undef;
        let r = pl::method(it, &raf, "Seek", vec![n(0), n(2)])?;
        set_err(&mut err, &r);
        let mut extra = pl::sub(&pl::method(it, &raf, "Tell", vec![])?, &tiff_end);
        // check for trailer and process if possible
        #[allow(clippy::never_loop)]
        loop {
            if !pl::gt(&extra, &n(12)) {
                break;
            }
            pl::method(it, &raf, "Seek", vec![tiff_end.clone()])?; // seek back to end of image
            trail_info = pl::method(it, &self_, "IdentifyTrailer", vec![raf.clone()])?;
            if !trail_info.truthy() {
                break;
            }
            let tbuf = cell(pl::s(""));
            trail_info.set("OutFile", Value::ScalarRef(tbuf)); // rewrite trailer(s)
            trail_info.set("ScanForTrailer", n(1)); // scan for AFCP if necessary
            self_.set("TrailerStart", tiff_end.clone());
            // rewrite all trailers to buffer
            if !pl::method(it, &self_, "ProcessTrailers", vec![trail_info.clone()])?.truthy() {
                trail_info = Value::Undef;
                err = n(1);
                break;
            }
            // calculate unused bytes before trailer
            extra = pl::sub(&trail_info.get("DataPos"), &tiff_end);
            break; // yes, the 'for' loop was just a cheap 'goto'
        }
        // ignore a single zero byte if used for padding
        if pl::gt(&extra, &n(0)) && crate::perl::ops::bit_and(&tiff_end, &n(0x01)).truthy() {
            let r = pl::method(it, &raf, "Seek", vec![tiff_end.clone(), n(0)])?;
            set_err(&mut err, &r);
            let (r, b) = raf_read(it, &raf, buf.clone(), &n(1))?;
            buf = b;
            set_err(&mut err, &r);
            if !buf.is_undef() && pl::str_eq(&buf, "\0") {
                extra = pl::sub(&extra, &n(1));
                tiff_end = pl::add(&tiff_end, &n(1));
            }
        }
        if pl::gt(&extra, &n(0)) {
            let known = self_.get("KnownTrailer");
            if vget(&self_, &[b"DEL_GROUP", b"Trailer"]).truthy() && !known.truthy() {
                vprint(it, &self_, &n(0), vec![cat(&[&pl::s("  Deleting unknown trailer ("), &extra, &pl::s(" bytes)\n")])])?;
                self_.set("CHANGED", pl::add(&self_.get("CHANGED"), &n(1)));
            } else if known.truthy() {
                vprint(it, &self_, &n(0), vec![cat(&[&pl::s("  Copying "), &known.get("Name"), &pl::s(" ("), &extra, &pl::s(" bytes)\n")])])?;
                let r = pl::method(it, &raf, "Seek", vec![tiff_end.clone(), n(0)])?;
                set_err(&mut err, &r);
                let r = pl::call(it, "Image::ExifTool::CopyBlock", vec![raf.clone(), outfile.clone(), extra.clone()])?;
                set_err(&mut err, &r);
            } else {
                let r = pl::method(it, &raf, "Seek", vec![tiff_end.clone(), n(0)])?;
                set_err(&mut err, &r);
                // preserve unknown trailer only if it contains non-null data
                // (Photoshop CS adds a trailer with 2 null bytes)
                let mut size = extra.clone();
                loop {
                    let nn = if pl::gt(&size, &n(65536)) { n(65536) } else { size.clone() };
                    let (r, b) = raf_read(it, &raf, buf.clone(), &nn)?;
                    buf = b;
                    if !pl::num_eq(&r, &nn) {
                        err = n(1);
                        break;
                    }
                    if re("[^\\0]", "").is_match(&buf) {
                        vprint(it, &self_, &n(0), vec![cat(&[&pl::s("  Preserving unknown trailer ("), &extra, &pl::s(" bytes)\n")])])?;
                        // copy the trailer since it contains non-null data
                        if !pl::num_eq(&size, &extra) {
                            let pad = Value::bytes(vec![0u8; pl::sub(&extra, &size).to_i64().max(0) as usize]);
                            if !write_out(it, &outfile, vec![pad])?.truthy() {
                                err = n(1);
                                break;
                            }
                        }
                        if !write_out(it, &outfile, vec![buf.clone()])?.truthy() {
                            err = n(1);
                            break;
                        }
                        if pl::gt(&size, &nn) {
                            let r = pl::call(it, "Image::ExifTool::CopyBlock", vec![raf.clone(), outfile.clone(), pl::sub(&size, &nn)])?;
                            set_err(&mut err, &r);
                        }
                        break;
                    }
                    size = pl::sub(&size, &nn);
                    if pl::gt(&size, &n(0)) {
                        continue;
                    }
                    vprint(it, &self_, &n(0), vec![cat(&[&pl::s("  Deleting blank trailer ("), &extra, &pl::s(" bytes)\n")])])?;
                    break;
                }
            }
        }
        // write trailer buffer if necessary
        if trail_info.truthy() {
            let r = pl::method(it, &self_, "WriteTrailerBuffer", vec![trail_info.clone(), outfile.clone()])?;
            set_err(&mut err, &r);
        }
        // add any new trailers we are creating
        let trail_pt = pl::method(it, &self_, "AddNewTrailers", vec![])?;
        if trail_pt.truthy() {
            let r = write_out(it, &outfile, vec![trail_pt.deref()])?;
            set_err(&mut err, &r);
        }
    }
    // check DNG version
    if self_.get("DNGVersion").truthy() {
        let mut ver = self_.get("DNGVersion");
        // currently support up to DNG version 1.7
        let ok = match re(r"^(\d+) (\d+)", "").m(&ver) {
            Some(m) => pl::le(&cat(&[&cap(&m, 1), &pl::s("."), &cap(&m, 2)]), &Value::Num(1.7)),
            None => false,
        };
        if !ok {
            re(" ", "").subst(&mut ver, true, |_| b".".to_vec());
            let msg = cat(&[&pl::s("DNG Version "), &ver, &pl::s(" not yet tested")]);
            pl::method(it, &self_, "Error", vec![msg, n(1)])?;
        }
    }
    ret(if err.truthy() { n(-1) } else { n(1) })
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::DirStart", dir_start);
    it.define_native("Image::ExifTool::ProcessEXV", process_exv);
    it.define_native("Image::ExifTool::ProcessEXIF", process_exif);
    it.define_native("Image::ExifTool::ProcessTIFF", process_tiff);
    it.define_native("Image::ExifTool::ProcessSubTIFF", process_sub_tiff);
    it.define_native("Image::ExifTool::TagTableKeys", tag_table_keys);
    it.define_native("Image::ExifTool::ProcessDirectory", process_directory);
    it.define_native("Image::ExifTool::MetadataPath", metadata_path);
    it.define_native("Image::ExifTool::GetFileExtension", get_file_extension);
    it.define_native("Image::ExifTool::GetTagInfoList", get_tag_info_list);
    it.define_native("Image::ExifTool::SetPriorityDir", set_priority_dir);
    it.define_native("Image::ExifTool::SetGroup", set_group);
    it.define_native("Image::ExifTool::DeleteTag", delete_tag);
    it.define_native("Image::ExifTool::DoEscape", do_escape);
    it.define_native("Image::ExifTool::SetFileType", set_file_type);
    it.define_native("Image::ExifTool::OverrideFileType", override_file_type);
    it.define_native("Image::ExifTool::ModifyMimeType", modify_mime_type);
    it.define_native("Image::ExifTool::VPrint", v_print);
    it.define_native("Image::ExifTool::VerboseDir", verbose_dir);
    it.define_native("Image::ExifTool::VerboseDump", verbose_dump);
    it.define_native("Image::ExifTool::PrintHex", print_hex);
    it.define_native("Image::ExifTool::ExtractBinary", extract_binary);
    it.define_native("Image::ExifTool::GetTagTable", get_tag_table);
    it.define_native("Image::ExifTool::GetTagInfo", get_tag_info);
    it.define_native("Image::ExifTool::AddTagToTable", add_tag_to_table);
    it.define_native("Image::ExifTool::HandleTag", handle_tag);
    it.define_native("Image::ExifTool::FoundTag", found_tag);
    it.define_native("Image::ExifTool::ProcessBinaryData", process_binary_data);
    it.define_native("Image::ExifTool::DoProcessTIFF", do_process_tiff);
}
