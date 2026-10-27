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
            // (a warning only with warnings enabled: -w, as ExifTool's
            // modules do not enable them)
            if it.get_scalar("main::^W").truthy() {
                it.warn(Value::str("substr outside of string at (eval) line 1.\n"))?;
            }
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

/// `s` under another name, for subs with a lexical `$s`.
fn s_(c: &Rc<RefCell<Value>>, v: Value) {
    s(c, v);
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
                // $raf->Read($$dataPt, ...) autovivifies an undefined $dataPt
                if data_pt.is_undef() {
                    data_pt = pl::sref(Value::Undef);
                }
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

/// `$raf->ReadLine($buff)` (see `raf_read`).
fn raf_readline(it: &mut Interp, raf: &Value, initial: Value) -> ER<(Value, Value)> {
    let scope = Scope::new(None);
    scope.scalar("raf", raf.clone());
    let buff = scope.scalar("buff", initial);
    let r = scalar_of(snippet(it, "$raf->ReadLine($buff)", &scope, Want::Scalar)?);
    let b = buff.borrow().clone();
    Ok((r, b))
}

/// `$subject =~ /PAT/g` in scalar context from the start of the string:
/// the end of the match (what `pos()` is then) and the captures.
fn match_g(pat: &[u8], flags: &str, subject: &Value) -> ER<Option<(usize, pl::Caps)>> {
    let r = match crate::perl::regex::compile(pat, flags) {
        Ok(r) => r,
        Err(e) => return crate::interp::eval::die(e),
    };
    let b = subject.to_bytes();
    match r.match_at(&b, 0) {
        Ok(Some(sp)) => {
            let end = sp[0].map(|(_, e)| e).unwrap_or(0);
            let caps = sp.iter().map(|g| g.map(|(x, y)| Value::bytes(&b[x..y]))).collect();
            Ok(Some((end, caps)))
        }
        _ => Ok(None),
    }
}

/// A pattern made of a package variable's value (interpolated, so its
/// characters are regex syntax, as in perl) and literal pattern text.
fn re_var(it: &mut Interp, pre: &str, var: &str, post: &str) -> ER<pl::Re> {
    let v = it.get_scalar(&format!("{ET}::{var}"));
    let mut p = pre.as_bytes().to_vec();
    p.extend_from_slice(&v.to_bytes());
    p.extend_from_slice(post.as_bytes());
    pl::re_dyn(&p, "")
}

/// Extract information from a JPEG image
/// Inputs: 0) ExifTool object reference, 1) dirInfo ref with RAF set
///         2) tag table ref to process JPEG-like metadata
/// Returns: 1 on success, 0 if this wasn't a valid JPEG file
pub fn process_jpeg(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    // local $_;
    let topic = local_topic(it);
    // (and the `local $/` below, restored here on every exit)
    let saved_rs = it.get_scalar("main::/");
    let r = process_jpeg_impl(it, a);
    it.set_scalar("main::/", saved_rs);
    restore_topic(it, topic);
    ret(r?)
}

// (some assignments are never read, as in the Perl, which is kept as it is)
#[allow(unused_assignments)]
fn process_jpeg_impl(it: &mut Interp, a: Vec<Value>) -> ER<Value> {
    use crate::perl::ops;
    let (self_, dir_info, mut optional_tag_table) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let options = self_.get("OPTIONS");
    let verbose = options.get("Verbose");
    let out = options.get("TextOut");
    let fast = or(options.get("FastScan"), n(0));
    let mut raf = dir_info.get("RAF");
    let req = self_.get("REQ_TAG_LOOKUP");
    let html_dump = self_.get("HTML_DUMP");
    let mut dump_parms = Hash::default();
    dump_parms.insert(b"Out", out.clone());
    dump_parms.insert(b"Prefix", self_.get("INDENT"));
    let (mut s, mut length, mut hash, mut hashsize) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef);
    let (mut success, mut want_trailer, mut trail_info, mut found_sos, mut got_size) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef, Value::Undef);
    let jumbf_chunk = Value::new_hash();
    let (mut icc_chunk, mut icc_chunk_count, mut icc_chunks_total): (Vec<Value>, Value, Value) = (Vec::new(), Value::Undef, Value::Undef);
    let (mut flir_chunk, mut flir_count, mut flir_total): (Vec<Value>, Value, Value) = (Vec::new(), Value::Undef, Value::Undef);
    let mut preview = Value::Undef;
    let scalado = cell(Value::Undef);
    let dqt = Value::new_array(vec![]);
    let (mut sub_sampling, mut dump_end) = (Value::Undef, Value::Undef);
    let extended_xmp = Value::new_hash();
    let mk = |m: &Value, x: i64| pl::num_eq(m, &n(x));
    let method = |it: &mut Interp, name: &str, args: Vec<Value>| pl::method(it, &self_, name, args);
    let table = |it: &mut Interp, name: &str| pl::call(it, "Image::ExifTool::GetTagTable", vec![pl::s(name)]);
    let set_bo = |it: &mut Interp, bo: Value| pl::call(it, "Image::ExifTool::SetByteOrder", vec![bo]);

    let mut indent = self_.get("INDENT");
    re("  $", "").subst(&mut indent, false, |_| Vec::new());
    if !raf.truthy() {
        let class = pl::s("File::RandomAccess");
        raf = scalar_of(it.call_method(&class, "new", vec![class.clone(), dir_info.get("DataPt")], Want::Scalar)?);
        verbose_dir(it, vec![self_.clone(), pl::s("JPEG"), Value::Undef, len_of(&dir_info.get("DataPt").deref())], Want::Void)?;
    }
    // get pointer to hash object if it exists and we are the top-level JPEG or JP2
    if re("^(JPEG|JP2)$", "").is_match(&self_.get("FILE_TYPE")) && !self_.get("DOC_NUM").truthy() {
        hash = self_.get("ImageDataHash");
        hashsize = n(0);
    }
    // check to be sure this is a valid JPG (or J2C, or EXV) file
    let (r, b) = raf_read(it, &raf, s.clone(), &n(2))?;
    s = b;
    if pl::num_eq(&r, &n(2)) && re("^\\xff[\\xd8\\x4f\\x01]", "").is_match(&s) {
        optional_tag_table = Value::Undef;
    } else {
        if !(optional_tag_table.truthy() && re("^\\xff[\\xe0-\\xef]", "").is_match(&s)) {
            return Ok(n(0));
        }
        if !pl::method(it, &raf, "Seek", vec![n(-2), n(1)])?.truthy() {
            method(it, "Error", vec![pl::s("Seek error")])?;
            return Ok(n(1));
        }
    }
    if &s.to_bytes()[..] == b"\xff\x01" {
        let (r, b) = raf_read(it, &raf, s.clone(), &n(5))?;
        s = b;
        if !(pl::num_eq(&r, &n(5)) && pl::str_eq(&s, "Exiv2")) {
            return Ok(n(0));
        }
        self_.set("FILE_TYPE", pl::s("EXV"));
    }
    let mut app_bytes = n(0);
    let mut calc_image_len = req.get("jpegimagelength");
    if options.get("RequestAll").truthy() && pl::gt(&options.get("RequestAll"), &n(2)) {
        calc_image_len = n(1);
    }
    if !vget(&self_, &[b"VALUE", b"FileType"]).truthy() || (self_.get("DOC_NUM").truthy() && options.get("ExtractEmbedded").truthy()) {
        method(it, "SetFileType", vec![])?; // set FileType tag
        if pl::gt(&fast, &n(2)) {
            return Ok(n(1)); // don't process file when FastScan > 2
        }
        self_.get_or_new_hash("LOW_PRIORITY_DIR").set("IFD1", n(1)); // lower priority of IFD1 tags
    }
    if method(it, "Options", vec![pl::s("FastScan")])?.truthy() {
        raf.set("NoBuffer", n(1)); // disable buffering in FastScan mode
    }

    if pl::lt(&verbose, &n(4)) {
        dump_parms.insert(b"MaxLen", n(128));
    }
    if html_dump.truthy() && !optional_tag_table.truthy() {
        dump_end = pl::method(it, &raf, "Tell", vec![])?;
        let (nn, t, m) = if pl::str_eq(&s, "Exiv2") { (7, "EXV", "TEM") } else { (2, "JPEG", "SOI") };
        let pos = pl::sub(&dump_end, &n(nn));
        if pos.truthy() {
            method(it, "HDump", vec![n(0), pos.clone(), pl::s("[unknown header]")])?;
        }
        method(it, "HDump", vec![pos, n(nn), pl::s(&format!("{t} header")), pl::s(&format!("{m} Marker"))])?;
    }
    let path = self_.get("PATH");
    let pn = path.len();

    // set input record separator to 0xff (the JPEG marker) to make reading quicker
    it.set_scalar("main::/", Value::bytes(b"\xff"));

    let (mut next_marker, mut next_seg_data_pt, mut next_seg_pos, mut first_seg_pos) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef);
    let combined_seg_data = cell(Value::Undef);
    let skip_data = Value::new_array(vec![]);
    let print = |it: &mut Interp, parts: &[&Value]| print_to(it, &out, vec![cat(parts)]);
    let hex_dump = |it: &mut Interp, data_pt: &Value, dp: &Hash, extra: &[(&str, Value)]| -> ER<()> {
        let mut args = vec![data_pt.clone(), Value::Undef];
        for (k, v) in dp.iter() {
            args.push(Value::Str(k.clone()));
            args.push(v.clone());
        }
        for (k, v) in extra {
            args.push(pl::s(k));
            args.push(v.clone());
        }
        pl::call(it, "Image::ExifTool::HexDump", args)?;
        Ok(())
    };
    let marker_len_bytes = lex_hash(it, "markerLenBytes");

    // read file until we reach an end of image (EOI) or start of scan (SOS)
    'marker: loop {
        // set marker and data pointer for current segment
        let marker = next_marker.clone();
        if marker.truthy() && pl::lt(&marker, &n(0)) {
            break;
        }
        let seg_data_pt = next_seg_data_pt.clone();
        let mut seg_pos = next_seg_pos.clone();
        let mut skipped = Value::Undef;
        next_marker = Value::Undef;
        next_seg_data_pt = Value::Undef;
//
// read ahead to the next segment unless we have reached EOI, SOS or SOD
//
        loop {
            if marker.truthy() && (mk(&marker, 0xd9) || (mk(&marker, 0xda) && !want_trailer.truthy() && !hash.truthy()) || mk(&marker, 0x93)) {
                break;
            }
            // read up to next marker (JPEG markers begin with 0xff)
            let buff = cell(Value::Undef);
            let (r, b) = raf_readline(it, &raf, g(&buff))?;
            s_(&buff, b);
            if !r.truthy() {
                if !optional_tag_table.truthy() {
                    break 'marker;
                }
                next_marker = n(-1);
                success = n(1);
                break;
            }
            skipped = pl::sub(&len_of(&g(&buff)), &n(1));
            // JPEG markers can be padded with unlimited 0xff's
            loop {
                let (r, ch) = raf_read(it, &raf, Value::Undef, &n(1))?;
                if !r.truthy() {
                    break 'marker;
                }
                next_marker = n(ch.to_bytes().first().copied().unwrap_or(0) as i64);
                if !mk(&next_marker, 0xff) {
                    break;
                }
                skipped = pl::add(&skipped, &n(1));
            }
            // read segment data if it exists
            let mlb = marker_len_bytes.get(next_marker.to_bytes());
            if mlb.is_undef() {
                // read record length word
                let (r, b) = raf_read(it, &raf, s.clone(), &n(2))?;
                s = b;
                if !pl::num_eq(&r, &n(2)) {
                    break 'marker;
                }
                let up = crate::perl::pack::unpack(b"n", &s.to_bytes()).unwrap_or_default();
                let mut len = arg(&up, 0); // get data length
                if !(!len.is_undef() && pl::ge(&len, &n(2))) {
                    break 'marker;
                }
                next_seg_pos = pl::method(it, &raf, "Tell", vec![])?;
                len = pl::sub(&len, &n(2)); // subtract size of length word
                let (r, b) = raf_read(it, &raf, g(&buff), &len)?;
                s_(&buff, b);
                if !pl::num_eq(&r, &len) {
                    break 'marker;
                }
                next_seg_data_pt = Value::ScalarRef(buff.clone()); // set pointer to our next data
            } else if pl::num_eq(&mlb, &n(4)) {
                // handle J2C extensions with 4-byte length word
                let (r, b) = raf_read(it, &raf, s.clone(), &n(4))?;
                s = b;
                if !pl::num_eq(&r, &n(4)) {
                    break 'marker;
                }
                let up = crate::perl::pack::unpack(b"N", &s.to_bytes()).unwrap_or_default();
                let mut len = arg(&up, 0); // get data length
                if !(!len.is_undef() && pl::ge(&len, &n(4))) {
                    break 'marker;
                }
                next_seg_pos = pl::method(it, &raf, "Tell", vec![])?;
                len = pl::sub(&len, &n(4)); // subtract size of length word
                if !pl::method(it, &raf, "Seek", vec![len, n(1)])?.truthy() {
                    break 'marker;
                }
            } else if hash.truthy() && !marker.is_undef() && (mk(&marker, 0x00) || mk(&marker, 0xda) || (pl::ge(&marker, &n(0xd0)) && pl::le(&marker, &n(0xd7)))) {
                // calculate hash for image data (includes leading ff d9 but not trailing ff da)
                let mut m = b"\xff".to_vec();
                m.push(marker.to_i64() as u8);
                pl::method(it, &hash, "add", vec![Value::bytes(m)])?;
                let nn = pl::sub(&skipped, &pl::sub(&len_of(&g(&buff)), &n(1))); // number of extra 0xff's
                if !nn.truthy() {
                    let b = substr(it, &g(&buff), &n(0), Some(&n(-1)))?; // remove trailing 0xff
                    s_(&buff, b);
                } else if pl::gt(&nn, &n(1)) {
                    let extra = vec![0xffu8; pl::sub(&nn, &n(1)).to_i64().max(0) as usize];
                    s_(&buff, pl::concat(&g(&buff), &Value::bytes(extra))); // add back extra 0xff's
                }
                pl::method(it, &hash, "add", vec![g(&buff)])?;
                hashsize = pl::add(&hashsize, &pl::add(&skipped, &n(2)));
            }
            // read second segment too if this was the first
            if marker.is_undef() {
                continue 'marker;
            }
            break;
        }
        // set some useful variables for the current segment
        let marker_name = pl::call(it, "Image::ExifTool::JpegMarkerName", vec![marker.clone()])?;
        path.set_at(pn, marker_name.clone());
        // issue warning if we skipped some garbage
        if skipped.truthy() && !found_sos.truthy() && !pl::str_eq(&marker_name, "SOS") {
            method(it, "Warn", vec![cat(&[&pl::s("Skipped unknown "), &skipped, &pl::s(" bytes after JPEG "), &marker_name, &pl::s(" segment")]), n(1)])?;
            if html_dump.truthy() {
                method(it, "HDump", vec![pl::sub(&pl::sub(&next_seg_pos, &n(4)), &skipped), skipped.clone(), cat(&[&pl::s("[unknown "), &skipped, &pl::s(" bytes]")]), Value::Undef, n(0x08)])?;
                dump_end = pl::sub(&next_seg_pos, &n(4));
            }
        }
//
// parse the current segment
//
        let m_and = |mask: i64| ops::bit_and(&marker, &n(mask));
        // handle SOF markers: SOF0-SOF15, except DHT(0xc4), JPGA(0xc8) and DAC(0xcc)
        if pl::num_eq(&m_and(0xf0), &n(0xc0)) && (mk(&marker, 0xc0) || m_and(0x03).truthy()) {
            length = len_of(&seg_data_pt.deref());
            if verbose.truthy() {
                print(it, &[&indent, &pl::s("JPEG "), &marker_name, &pl::s(" ("), &length, &pl::s(" bytes):\n")])?;
                if pl::gt(&verbose, &n(2)) {
                    hex_dump(it, &seg_data_pt, &dump_parms, &[("Addr", seg_pos.clone())])?;
                }
            } else if html_dump.truthy() {
                method(it, "HDump", vec![pl::sub(&seg_pos, &n(4)), pl::add(&length, &n(4)), cat(&[&pl::s("[JPEG "), &marker_name, &pl::s("]")]), Value::Undef, n(0x08)])?;
                dump_end = pl::add(&seg_pos, &length);
            }
            if pl::lt(&length, &n(6)) || got_size.truthy() {
                continue;
            }
            got_size = n(1); // (ignore subsequent SOF segments in probably corrupted JPEG)
            // extract some useful information
            let up = crate::perl::pack::unpack(b"Cn2C", &seg_data_pt.deref().to_bytes()).unwrap_or_default();
            let (p, h, w, nc) = (arg(&up, 0), arg(&up, 1), arg(&up, 2), arg(&up, 3));
            let sof = table(it, "Image::ExifTool::JPEG::SOF")?;
            method(it, "HandleTag", vec![sof.clone(), pl::s("ImageWidth"), w])?;
            method(it, "HandleTag", vec![sof.clone(), pl::s("ImageHeight"), h])?;
            method(it, "HandleTag", vec![sof.clone(), pl::s("EncodingProcess"), pl::sub(&marker, &n(0xc0))])?;
            method(it, "HandleTag", vec![sof.clone(), pl::s("BitsPerSample"), p])?;
            method(it, "HandleTag", vec![sof.clone(), pl::s("ColorComponents"), nc.clone()])?;
            if !(pl::num_eq(&nc, &n(3)) && pl::ge(&length, &n(15))) {
                continue;
            }
            let (mut hmin, mut hmax, mut vmin, mut vmax) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef);
            // loop through all components to determine sampling frequency
            sub_sampling = pl::s("");
            let mut i = 0i64;
            while pl::lt(&n(i), &nc) {
                let sf = pl::call(it, "Image::ExifTool::Get8u", vec![seg_data_pt.clone(), n(7 + 3 * i)])?;
                sub_sampling = pl::concat(&sub_sampling, &pl::sprintf("%.2x", &[sf.clone()]));
                // isolate horizontal and vertical components
                let (hf, vf) = (ops::shift_right(&sf, &n(4)), ops::bit_and(&sf, &n(0x0f)));
                if i == 0 {
                    hmin = hf.clone();
                    hmax = hf;
                    vmin = vf.clone();
                    vmax = vf;
                    i += 1;
                    continue;
                }
                // determine min/max frequencies
                if pl::lt(&hf, &hmin) {
                    hmin = hf.clone();
                }
                if pl::gt(&hf, &hmax) {
                    hmax = hf;
                }
                if pl::lt(&vf, &vmin) {
                    vmin = vf.clone();
                }
                if pl::gt(&vf, &vmax) {
                    vmax = vf;
                }
                i += 1;
            }
            if hmin.truthy() && vmin.truthy() {
                let (hs, vs) = (pl::div(&hmax, &hmin)?, pl::div(&vmax, &vmin)?);
                method(it, "HandleTag", vec![sof, pl::s("YCbCrSubSampling"), cat(&[&hs, &pl::s(" "), &vs])])?;
            }
            continue;
        } else if mk(&marker, 0xd9) {
            // EOI
            if let Value::ArrayRef(p) = &path {
                p.borrow_mut().pop();
            }
            if verbose.truthy() {
                print(it, &[&indent, &pl::s("JPEG EOI\n")])?;
            }
            let pos = pl::method(it, &raf, "Tell", vec![])?;
            if !self_.get("DOC_NUM").truthy() {
                self_.set("TrailerStart", pos.clone());
            }
            if html_dump.truthy() && dump_end.truthy() {
                method(it, "HDump", vec![dump_end.clone(), pl::sub(&pl::sub(&pos, &n(2)), &dump_end), pl::s("[JPEG Image Data]"), Value::Undef, n(0x08)])?;
                method(it, "HDump", vec![pl::sub(&pos, &n(2)), n(2), pl::s("JPEG EOI"), Value::Undef])?;
                dump_end = n(0);
            }
            if found_sos.truthy() || pl::str_eq(&self_.get("FILE_TYPE"), "EXV") {
                success = n(1);
            } else {
                method(it, "Warn", vec![pl::s("Missing JPEG SOS")])?;
            }
            if req.get("trailer").truthy() {
                // read entire trailer into memory
                if pl::method(it, &raf, "Seek", vec![n(0), n(2)])?.truthy() {
                    let len = pl::sub(&pl::method(it, &raf, "Tell", vec![])?, &pos);
                    if len.truthy() {
                        let buff = cell(Value::Undef);
                        pl::method(it, &raf, "Seek", vec![pos.clone(), n(0)])?;
                        let (r, b) = raf_read(it, &raf, g(&buff), &len)?;
                        s_(&buff, b);
                        if pl::num_eq(&r, &len) {
                            found_tag_call(it, &self_, vec![pl::s("Trailer"), Value::ScalarRef(buff)])?;
                        }
                        pl::method(it, &raf, "Seek", vec![pos.clone(), n(0)])?;
                    }
                } else {
                    method(it, "Warn", vec![pl::s("Error seeking in file")])?;
                }
            }
            // we are here because we are looking for trailer information
            if want_trailer.truthy() {
                let start = self_.get("PreviewImageStart");
                if start.truthy() || options.get("ExtractEmbedded").truthy() {
                    let buff = cell(Value::Undef);
                    // most previews start right after the JPEG EOI, but the Olympus E-20
                    // preview is 508 bytes into the trailer, the K-M Maxxum 7D preview is
                    // 979 bytes in, and Sony previews can start up to 32 kB into the trailer.
                    // (and Minolta and Sony previews can have a random first byte...)
                    let scan_len = if re("Sony", "i").is_match(&self_.get("Make")) { n(65536) } else { n(1024) };
                    let (r, b) = raf_read(it, &raf, g(&buff), &scan_len)?;
                    s_(&buff, b);
                    if r.truthy() {
                        let m2;
                        if re("^.{4}ftyp", "s").is_match(&g(&buff)) {
                            let mut val = Value::Undef;
                            if pl::method(it, &raf, "Seek", vec![n(0), n(2)])?.truthy() {
                                let len = pl::sub(&pl::method(it, &raf, "Tell", vec![])?, &pos);
                                if options.get("Binary").truthy() {
                                    if pl::method(it, &raf, "Seek", vec![pos.clone(), n(0)])?.truthy() {
                                        let (r, b) = raf_read(it, &raf, g(&buff), &len)?;
                                        s_(&buff, b);
                                        if pl::num_eq(&r, &len) {
                                            val = Value::ScalarRef(buff.clone());
                                        }
                                    }
                                } else {
                                    val = Value::ScalarRef(cell(cat(&[&pl::s("Binary data "), &len, &pl::s(" bytes")])));
                                }
                                if val.truthy() {
                                    found_tag_call(it, &self_, vec![pl::s("EmbeddedVideo"), val])?;
                                } else {
                                    method(it, "Warn", vec![pl::s("Error reading trailer")])?;
                                }
                            } else {
                                method(it, "Warn", vec![pl::s("Error seeking to end of file")])?;
                            }
                        } else if {
                            m2 = match match_g(b"\\xff\\xd8\\xff.", "", &g(&buff))? {
                                Some(x) => Some(x),
                                None => {
                                    if re("(Minolta|Sony)", "i").is_match(&self_.get("Make")) {
                                        match_g(b".\\xd8\\xff\\xdb", "", &g(&buff))?
                                    } else {
                                        None
                                    }
                                }
                            };
                            m2.is_some()
                        } {
                            // adjust PreviewImageStart to this location
                            let p = m2.unwrap().0;
                            let actual = pl::sub(&pl::add(&pos, &n(p as i64)), &n(4));
                            if start.truthy() && !seq(&start, &actual) && pl::gt(&verbose, &n(1)) {
                                print(it, &[&indent, &pl::s("(Fixed PreviewImage location: "), &start, &pl::s(" -> "), &actual, &pl::s(")\n")])?;
                            }
                            // update preview image offsets
                            if start.truthy() {
                                if vget(&self_, &[b"VALUE", b"PreviewImageStart"]).truthy() {
                                    self_.get("VALUE").set("PreviewImageStart", actual.clone());
                                }
                                self_.set("PreviewImageStart", actual.clone());
                            }
                            // load preview now if we tried and failed earlier
                            if self_.get("PreviewError").truthy() && self_.get("PreviewImageLength").truthy() {
                                let mut ok = false;
                                if pl::method(it, &raf, "Seek", vec![actual.clone(), n(0)])?.truthy() {
                                    let (r, b) = raf_read(it, &raf, g(&buff), &self_.get("PreviewImageLength"))?;
                                    s_(&buff, b);
                                    ok = r.truthy();
                                }
                                if ok {
                                    found_tag_call(it, &self_, vec![pl::s("PreviewImage"), g(&buff)])?;
                                    self_.delete("PreviewError");
                                }
                            }
                        }
                    }
                    pl::method(it, &raf, "Seek", vec![pos.clone(), n(0)])?;
                }
            }
            // process trailer now or finish processing trailers
            // and scan for AFCP if necessary
            let mut from_end = n(0);
            if trail_info.truthy() {
                trail_info.set("ScanForTrailer", n(1)); // scan now if necessary
                method(it, "ProcessTrailers", vec![trail_info.clone()])?;
                // save offset from end of file to start of first trailer
                from_end = trail_info.get("Offset");
                trail_info = Value::Undef;
            }
            if self_.get("LeicaTrailer").truthy() {
                pl::method(it, &raf, "Seek", vec![n(0), n(2)])?;
                let lt = self_.get("LeicaTrailer");
                lt.set("TrailPos", pos.clone());
                let tell = pl::method(it, &raf, "Tell", vec![])?;
                lt.set("TrailLen", pl::sub(&pl::sub(&tell, &pos), &from_end));
                pl::call(it, "Image::ExifTool::Panasonic::ProcessLeicaTrailer", vec![self_.clone()])?;
            }
            // finally, dump remaining information in JPEG trailer
            if verbose.truthy() || html_dump.truthy() {
                let mut end_pos = self_.get("LeicaTrailerPos");
                if !end_pos.truthy() {
                    pl::method(it, &raf, "Seek", vec![n(0), n(2)])?;
                    end_pos = pl::sub(&pl::method(it, &raf, "Tell", vec![])?, &from_end);
                }
                if pl::gt(&end_pos, &pos) {
                    let h = Value::new_hash();
                    h.set("RAF", raf.clone());
                    h.set("DataPos", pos.clone());
                    h.set("DirLen", pl::sub(&end_pos, &pos));
                    method(it, "DumpUnknownTrailer", vec![h])?;
                }
            }
            if calc_image_len.truthy() {
                found_tag_call(it, &self_, vec![pl::s("JPEGImageLength"), pl::sub(&pos, &app_bytes)])?;
            }
            break; // all done parsing file
        } else if mk(&marker, 0xda) {
            // SOS
            if let Value::ArrayRef(p) = &path {
                p.borrow_mut().pop();
            }
            found_sos = n(1);
            // all done with meta information unless we have a trailer
            if verbose.truthy() {
                print(it, &[&indent, &pl::s("JPEG SOS\n")])?;
            }
            // process extended XMP now if it existed
            // (must do this before trailers because XMP is required to process Google trailer)
            if !extended_xmp.is_empty() {
                // GUID indicated by the last main XMP segment
                let good_guid = or(vget(&self_, &[b"VALUE", b"HasExtendedXMP"]), pl::s(""));
                // GUID of the extended XMP that we will process ('2' for all)
                let mut read_guid = or(options.get("ExtendedXMP"), n(0));
                if pl::str_eq(&read_guid, "1") {
                    read_guid = good_guid.clone();
                }
                let mut guids = extended_xmp.keys();
                guids.sort();
                for gk in guids {
                    let guid = Value::Str(gk.clone());
                    if gk.len() != 32 {
                        continue; // ignore other (internal) keys
                    }
                    let ext_xmp = extended_xmp.get(&gk);
                    let mut offsets: Vec<Value> = Vec::new();
                    let mut warn = Value::Undef;
                    // make sure we have all chunks, and create a list of sorted offsets
                    let mut off = n(0);
                    while pl::lt(&off, &ext_xmp.get("Size")) {
                        if ext_xmp.get(off.to_bytes()).is_undef() {
                            break;
                        }
                        offsets.push(off.clone());
                        off = pl::add(&off, &len_of(&ext_xmp.get(off.to_bytes())));
                    }
                    if !pl::num_eq(&off, &ext_xmp.get("Size")) {
                        method(it, "Warn", vec![cat(&[&pl::s("Incomplete extended XMP (GUID "), &guid, &pl::s(")")])])?;
                        continue;
                    }
                    if seq(&guid, &read_guid) || pl::str_eq(&read_guid, "2") {
                        if !seq(&guid, &good_guid) {
                            warn = pl::s("Reading non-");
                        }
                        let buff = cell(pl::s(""));
                        // assemble XMP all together
                        for o in &offsets {
                            s_(&buff, pl::concat(&g(&buff), &ext_xmp.get(o.to_bytes())));
                        }
                        let tag_table_ptr = table(it, "Image::ExifTool::XMP::Main")?;
                        let di = Value::new_hash();
                        di.set("DataPt", Value::ScalarRef(buff));
                        di.set("Parent", pl::s("APP1"));
                        di.set("IsExtended", n(1));
                        path.set_at(pn, pl::s("APP1"));
                        method(it, "ProcessDirectory", vec![di, tag_table_ptr])?;
                        if let Value::ArrayRef(p) = &path {
                            p.borrow_mut().pop();
                        }
                    } else {
                        warn = pl::s("Ignored ");
                        if !seq(&guid, &good_guid) {
                            warn = pl::concat(&warn, &pl::s("non-"));
                        }
                    }
                    if warn.truthy() {
                        method(it, "Warn", vec![cat(&[&warn, &pl::s("standard extended XMP (GUID "), &guid, &pl::s(")")])])?;
                    }
                    extended_xmp.delete(&gk);
                }
            }
            if !fast.truthy() {
                trail_info = method(it, "IdentifyTrailer", vec![raf.clone()])?;
                // process trailer now unless we are doing verbose dump
                if trail_info.truthy() && pl::lt(&verbose, &n(3)) && !html_dump.truthy() {
                    // process trailers (keep trailInfo to finish processing later
                    // only if we can't finish without scanning from JPEG EOF)
                    if method(it, "ProcessTrailers", vec![trail_info.clone()])?.truthy() {
                        trail_info = Value::Undef;
                    }
                }
                if want_trailer.truthy() && self_.get("PreviewImageStart").truthy() {
                    // seek ahead and validate preview image
                    let cur_pos = pl::method(it, &raf, "Tell", vec![])?;
                    let mut ok = false;
                    if pl::method(it, &raf, "Seek", vec![self_.get("PreviewImageStart"), n(0)])?.truthy() {
                        let (r, buff) = raf_read(it, &raf, Value::Undef, &n(4))?;
                        ok = pl::num_eq(&r, &n(4)) && re("^.\\xd8\\xff[\\xc4\\xdb\\xe0-\\xef]", "").is_match(&buff);
                    }
                    if ok {
                        want_trailer = Value::Undef;
                    }
                    if !pl::method(it, &raf, "Seek", vec![cur_pos, n(0)])?.truthy() {
                        break;
                    }
                }
                // seek ahead and process Leica trailer
                if self_.get("LeicaTrailer").truthy() {
                    runtime::require(it, "Image::ExifTool::Panasonic")?;
                    pl::call(it, "Image::ExifTool::Panasonic::ProcessLeicaTrailer", vec![self_.clone()])?;
                    if self_.get("LeicaTrailer").truthy() {
                        want_trailer = n(1);
                    }
                } else if options.get("ExtractEmbedded").truthy()
                    || (vget(&self_, &[b"VALUE", b"HiddenDataOffset"]).truthy() && vget(&self_, &[b"VALUE", b"HiddenDataLength"]).truthy() && (options.get("Validate").truthy() || req.get("hiddendata").truthy()))
                {
                    want_trailer = n(1);
                }
                if trail_info.truthy() || want_trailer.truthy() || pl::gt(&verbose, &n(2)) || html_dump.truthy() {
                    continue;
                }
            }
            // must scan to EOI if Validate or JpegCompressionFactor used
            if options.get("Validate").truthy() || calc_image_len.truthy() || req.get("trailer").truthy() || hash.truthy() {
                continue;
            }
            // nothing interesting to parse after start of scan (SOS)
            success = n(1);
            break; // all done parsing file
        } else if mk(&marker, 0x93) {
            if let Value::ArrayRef(p) = &path {
                p.borrow_mut().pop();
            }
            if verbose.truthy() {
                print(it, &[&indent, &pl::s("JPEG SOD\n")])?;
            }
            success = n(1);
            if hash.truthy() && pl::str_eq(&self_.get("FILE_TYPE"), "JP2") {
                let pos = pl::method(it, &raf, "Tell", vec![])?;
                method(it, "ImageDataHash", vec![raf.clone(), Value::Undef, pl::s("SOD")])?;
                pl::method(it, &raf, "Seek", vec![pos, n(0)])?;
            }
            if pl::gt(&verbose, &n(2)) || html_dump.truthy() {
                continue;
            }
            break; // all done parsing file
        } else if !marker_len_bytes.get(marker.to_bytes()).is_undef() {
            // handle other stand-alone markers and segments we skipped over
            if verbose.truthy() && marker.truthy() {
                if pl::lt(&verbose, &n(4)) && pl::num_eq(&m_and(0xf8), &n(0xd0)) {
                    continue;
                }
                print(it, &[&indent, &pl::s("JPEG "), &marker_name, &pl::s("\n")])?;
            }
            continue;
        } else if mk(&marker, 0xdb) && pl::length(&seg_data_pt.deref()).truthy() &&    // DQT
            // save the DQT data only if JPEGDigest has been requested
            // (Note: since we aren't checking the API RequestAll option here, the application
            //  must use the RequestTags option to generate these tags if they have not been
            //  specifically requested.  The reason is that there is too much overhead involved
            //  in the calculation of this tag to make this worth the CPU time.)
            (req.get("jpegdigest").truthy() || req.get("jpegqualityestimate").truthy() || (options.get("RequestAll").truthy() && pl::gt(&options.get("RequestAll"), &n(2))))
        {
            let up = crate::perl::pack::unpack(b"C", &seg_data_pt.deref().to_bytes()).unwrap_or_default();
            let num = ops::bit_and(&arg(&up, 0), &n(0x0f)); // get table index
            if pl::lt(&num, &n(4)) {
                dqt.set_at(num.to_i64() as usize, seg_data_pt.deref()); // save for hash calculation
            }
        }
        // handle all other markers
        let mut dump_type = pl::s("");
        let (mut desc, mut tip, mut xtra, mut use_jpeg_main) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef);
        length = len_of(&seg_data_pt.deref());
        if pl::num_eq(&m_and(0xf0), &n(0xe0)) {
            app_bytes = pl::add(&app_bytes, &pl::add(&length, &n(4))); // total size of APP segments
        }
        if verbose.truthy() {
            print(it, &[&indent, &pl::s("JPEG "), &marker_name, &pl::s(" ("), &length, &pl::s(" bytes):\n")])?;
            if pl::gt(&verbose, &n(2)) {
                let mut extra = vec![("Addr", seg_pos.clone())];
                if pl::num_eq(&verbose, &n(4)) {
                    extra.push(("MaxLen", n(128)));
                }
                hex_dump(it, &seg_data_pt, &dump_parms, &extra)?;
            }
        }
        // prepare dirInfo hash for processing this information
        let dinfo = Value::new_hash();
        dinfo.set("Parent", marker_name.clone());
        dinfo.set("DataPt", seg_data_pt.clone());
        dinfo.set("DataPos", seg_pos.clone());
        dinfo.set("DataLen", length.clone());
        dinfo.set("DirStart", n(0));
        dinfo.set("DirLen", length.clone());
        dinfo.set("Base", n(0));
        let seg = seg_data_pt.deref();
        let is = |p: &str, f: &str| re(p, f).is_match(&seg);
        let dir_start = |it: &mut Interp, args: Vec<Value>| -> ER<()> {
            let mut a = vec![dinfo.clone()];
            a.extend(args);
            dir_start(it, a, Want::Void)?;
            Ok(())
        };
        let process_dir = |it: &mut Interp, di: &Value, t: Value| pl::method(it, &self_, "ProcessDirectory", vec![di.clone(), t]);
        if mk(&marker, 0xe0) {
            // APP0 (JFIF, JFXX, CIFF, AVI1, Ocad)
            if is("^JFIF\\0", "") {
                dump_type = pl::s("JFIF");
                dir_start(it, vec![n(5)])?; // start at byte 5
                set_bo(it, pl::s("MM"))?;
                let tag_table_ptr = table(it, "Image::ExifTool::JFIF::Main")?;
                process_dir(it, &dinfo, tag_table_ptr)?;
            } else if let Some(m) = re("^JFXX\\0(\\x10|\\x11|\\x13)", "").m(&seg) {
                let tag = n(cap(&m, 1).to_bytes().first().copied().unwrap_or(0) as i64);
                dump_type = pl::s("JFXX");
                let tag_table_ptr = table(it, "Image::ExifTool::JFIF::Extension")?;
                let tag_info = get_tag_info_impl(it, vec![self_.clone(), tag_table_ptr, tag])?;
                let v = substr(it, &seg, &n(6), None)?;
                found_tag_call(it, &self_, vec![tag_info, v])?;
            } else if is("^(II|MM).{4}HEAPJPGM", "s") {
                if pl::gt(&fast, &n(1)) {
                    continue; // skip processing for very fast
                }
                dump_type = pl::s("CIFF");
                let class = pl::s("File::RandomAccess");
                let r = scalar_of(it.call_method(&class, "new", vec![class.clone(), seg_data_pt.clone()], Want::Scalar)?);
                let di = Value::new_hash();
                di.set("RAF", r);
                self_.set("SET_GROUP1", pl::s("CIFF"));
                self_.get_or_new_array("PATH").push(pl::s("CIFF"));
                runtime::require(it, "Image::ExifTool::CanonRaw")?;
                pl::call(it, "Image::ExifTool::CanonRaw::ProcessCRW", vec![self_.clone(), di])?;
                if let Value::ArrayRef(p) = self_.get_or_new_array("PATH") {
                    p.borrow_mut().pop();
                }
                self_.delete("SET_GROUP1");
            } else if let Some(m) = re("^(AVI1|Ocad)", "").m(&seg) {
                dump_type = cap(&m, 1);
                set_bo(it, pl::s("MM"))?;
                let tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![cat(&[&pl::s("Image::ExifTool::JPEG::"), &dump_type])])?;
                dir_start(it, vec![n(4)])?;
                process_dir(it, &dinfo, tag_table_ptr)?;
            }
        } else if mk(&marker, 0xe1) {
            // APP1 (EXIF, XMP, QVCI, PARROT)
            // (some Kodak cameras don't put a second "\0", and I have seen an
            //  example where there was a second 4-byte APP1 segment header)
            if let Some(m) = re("^(.{0,4})Exif\\0.", "is").m(&seg) {
                dump_type = Value::Undef; // (will be dumped here)
                // this is EXIF data --
                // get the data block (into a common variable)
                let exif_app1hdr = it.get_scalar("Image::ExifTool::exifAPP1hdr");
                let mut hdr_len = len_of(&exif_app1hdr);
                if pl::length(&cap(&m, 1)).truthy() {
                    hdr_len = pl::add(&hdr_len, &len_of(&cap(&m, 1)));
                    method(it, "Warn", vec![pl::s("Unknown garbage at start of EXIF segment"), n(1)])?;
                } else if !is("^Exif\\0", "") {
                    method(it, "Warn", vec![pl::s("Incorrect EXIF segment identifier"), n(1)])?;
                }
                if html_dump.truthy() {
                    method(it, "HDump", vec![pl::sub(&seg_pos, &n(4)), n(4), pl::s("APP1 header"), cat(&[&pl::s("Data size: "), &length, &pl::s(" bytes")])])?;
                    method(it, "HDump", vec![seg_pos.clone(), hdr_len.clone(), pl::s("Exif header"), pl::s("APP1 data type: Exif")])?;
                    dump_end = pl::add(&seg_pos, &length);
                }
                let mut data_pt = seg_data_pt.clone();
                if !g(&combined_seg_data).is_undef() {
                    skip_data.push(Value::new_array(vec![pl::sub(&seg_pos, &n(4)), pl::add(&seg_pos, &hdr_len)]));
                    let tail = substr(it, &seg_data_pt.deref(), &hdr_len, None)?;
                    s_(&combined_seg_data, pl::concat(&g(&combined_seg_data), &tail));
                    set_deref(&seg_data_pt, Value::Undef)?;
                    data_pt = Value::ScalarRef(combined_seg_data.clone());
                    seg_pos = first_seg_pos.clone();
                }
                // peek ahead to see if the next segment is extended EXIF
                if pl::num_eq(&next_marker, &marker) && re_var(it, "^", "exifAPP1hdr", "(?!(MM\\0\\x2a|II\\x2a\\0))")?.is_match(&next_seg_data_pt.deref()) {
                    // initialize combined data if necessary
                    if g(&combined_seg_data).is_undef() {
                        s_(&combined_seg_data, seg_data_pt.deref());
                        set_deref(&seg_data_pt, Value::Undef)?;
                        first_seg_pos = seg_pos.clone();
                        method(it, "Warn", vec![pl::s("File contains multi-segment EXIF"), n(1)])?;
                        self_.set("ExtendedEXIF", n(1));
                    }
                    continue;
                }
                dinfo.set("DataPt", data_pt.clone());
                dinfo.set("DataPos", seg_pos.clone());
                let l = len_of(&data_pt.deref());
                dinfo.set("DirLen", l.clone());
                dinfo.set("DataLen", l);
                dir_start(it, vec![hdr_len.clone(), hdr_len.clone()])?;
                if !skip_data.is_empty() {
                    self_.set("SkipData", skip_data.clone());
                }
                // extract the EXIF information (it is in standard TIFF format)
                if !method(it, "ProcessTIFF", vec![dinfo.clone()])?.truthy() {
                    method(it, "Warn", vec![pl::s("Malformed APP1 EXIF segment")])?;
                }
                // scan for Vivo HiddenData if necessary
                if pl::str_eq(&self_.get("Make"), "vivo") &&
                    // (stored as UserComment by some models)
                    !(vget(&self_, &[b"VALUE", b"UserComment"]).truthy() && re("^filter:", "").is_match(&self_.get("VALUE").get("UserComment")))
                {
                    if let Some((p, c)) = match_g(b"(filter: .*?; \\n)\\0", "s", &data_pt.deref())? {
                        if html_dump.truthy() {
                            let nn = pl::add(&len_of(&cap(&c, 1)), &n(1));
                            method(it, "HDump", vec![pl::sub(&pl::add(&seg_pos, &n(p as i64)), &nn), nn.clone(), pl::s("[Vivo HiddenData]"), Value::Undef, n(0x08)])?;
                        }
                        let tbl = table(it, "Image::ExifTool::Trailer::Vivo")?;
                        method(it, "HandleTag", vec![tbl, pl::s("HiddenData"), cap(&c, 1)])?;
                    }
                }
                // avoid looking for preview unless necessary because it really slows
                // us down -- only look for it if we found pointer, and preview is
                // outside EXIF, and PreviewImage is specifically requested
                let mut start = method(it, "GetValue", vec![pl::s("PreviewImageStart"), pl::s("ValueConv")])?;
                let mut plen = method(it, "GetValue", vec![pl::s("PreviewImageLength"), pl::s("ValueConv")])?;
                if !start.truthy() || (!plen.truthy() && self_.get("PreviewError").truthy()) {
                    start = self_.get("PreviewImageStart");
                    plen = self_.get("PreviewImageLength");
                }
                if start.truthy()
                    && plen.truthy()
                    && pl::call(it, "Image::ExifTool::IsInt", vec![start.clone()])?.truthy()
                    && pl::call(it, "Image::ExifTool::IsInt", vec![plen.clone()])?.truthy()
                    && pl::gt(&pl::add(&start, &plen), &pl::add(&self_.get("EXIF_POS"), &len_of(&self_.get("EXIF_DATA"))))
                    && (req.get("previewimage").truthy() ||
                    // (extracted normally, so check Binary option)
                    (options.get("Binary").truthy() && !vget(&self_, &[b"EXCL_TAG_LOOKUP", b"previewimage"]).truthy()))
                {
                    self_.set("PreviewImageStart", start);
                    self_.set("PreviewImageLength", plen);
                    want_trailer = n(1);
                }
                if !skip_data.is_empty() {
                    if let Value::ArrayRef(sd) = &skip_data {
                        sd.borrow_mut().clear();
                    }
                    self_.delete("SkipData");
                }
                set_deref(&data_pt, Value::Undef)?;
                continue;
            } else if re_var(it, "^", "xmpExtAPP1hdr", "")?.is_match(&seg) {
                // off len -- extended XMP header (75 bytes total):
                //   0  35 bytes - signature
                //  35  32 bytes - GUID (MD5 hash of full extended XMP data in ASCII)
                //  67   4 bytes - total size of extended XMP data
                //  71   4 bytes - offset for this XMP data portion
                dump_type = pl::s("Extended XMP");
                if pl::gt(&length, &n(75)) {
                    let up = crate::perl::pack::unpack(b"x67N2", &seg.to_bytes()).unwrap_or_default();
                    let (size, off) = (arg(&up, 0), arg(&up, 1));
                    let guid = substr(it, &seg, &n(35), Some(&n(32)))?;
                    if re("[^A-Za-z0-9]", "").is_match(&guid) {
                        // (technically, should be uppercase)
                        tip = pl::s("Invalid extended XMP GUID");
                        method(it, "Warn", vec![tip.clone()])?;
                    } else {
                        let mut ext_xmp = extended_xmp.get(guid.to_bytes());
                        if !ext_xmp.truthy() {
                            ext_xmp = Value::new_hash();
                            extended_xmp.set(guid.to_bytes(), ext_xmp.clone());
                        } else if !pl::num_eq(&size, &ext_xmp.get("Size")) {
                            method(it, "Warn", vec![pl::s("Inconsistent extended XMP size")])?;
                        }
                        ext_xmp.set("Size", size.clone());
                        let chunk = substr(it, &seg, &n(75), None)?;
                        ext_xmp.set(off.to_bytes(), chunk);
                        tip = cat(&[&pl::s("Full length: "), &size, &pl::s("\nChunk offset: "), &off, &pl::s("\nChunk length: "), &pl::sub(&length, &n(75)), &pl::s("\nGUID: "), &guid]);
                        // (delay processing extended XMP until after reading all segments)
                    }
                } else {
                    tip = pl::s("Invalid extended XMP segment");
                    method(it, "Warn", vec![tip.clone()])?;
                }
            } else if is("^QVCI\\0", "") {
                dump_type = pl::s("QVCI");
                let tag_table_ptr = table(it, "Image::ExifTool::Casio::QVCI")?;
                process_dir(it, &dinfo, tag_table_ptr)?;
            } else if is("^FLIR\\0", "") && pl::ge(&length, &n(8)) {
                dump_type = pl::s("FLIR");
                // must concatenate FLIR chunks (note: handle the case where
                // some software erroneously writes zeros for the chunk counts)
                let chunk_num = pl::call(it, "Image::ExifTool::Get8u", vec![seg_data_pt.clone(), n(6)])?;
                let chunks_tot = pl::add(&pl::call(it, "Image::ExifTool::Get8u", vec![seg_data_pt.clone(), n(7)])?, &n(1)); // (note the "+ 1"!)
                if verbose.truthy() {
                    let fmt = cat(&[&indent, &pl::s("FLIR chunk %d of %d\n")]);
                    print_to(it, &out, vec![pl::sprintf_v(&fmt, &[pl::add(&chunk_num, &n(1)), chunks_tot.clone()])])?;
                }
                if !flir_total.is_undef() {
                    // abort parsing FLIR if the total chunk count is inconsistent
                    if !pl::num_eq(&chunks_tot, &flir_total) {
                        flir_count = Value::Undef;
                    }
                } else {
                    flir_count = n(0);
                    flir_total = chunks_tot.clone();
                }
                if !flir_count.is_undef() {
                    let ci = chunk_num.to_i64().max(0) as usize;
                    if flir_chunk.len() <= ci {
                        flir_chunk.resize(ci + 1, Value::Undef);
                    }
                    let chunk = substr(it, &seg, &n(8), None)?;
                    if !flir_chunk[ci].is_undef() {
                        method(it, "Warn", vec![pl::s("Duplicate FLIR chunk number(s)")])?;
                        flir_chunk[ci] = pl::concat(&flir_chunk[ci], &chunk);
                    } else {
                        flir_chunk[ci] = chunk;
                    }
                    // process the FLIR information if we have all of the chunks
                    flir_count = pl::add(&flir_count, &n(1));
                    if pl::ge(&flir_count, &flir_total) {
                        let flir = cell(pl::s(""));
                        for c in flir_chunk.iter() {
                            if !c.is_undef() {
                                s_(&flir, pl::concat(&g(&flir), c));
                            }
                        }
                        flir_chunk.clear(); // free memory
                        let tag_table_ptr = table(it, "Image::ExifTool::FLIR::FFF")?;
                        let di = Value::new_hash();
                        di.set("DataPt", Value::ScalarRef(flir));
                        di.set("Parent", marker_name.clone());
                        di.set("DirName", pl::s("FLIR"));
                        process_dir(it, &di, tag_table_ptr)?;
                        flir_count = Value::Undef; // prevent reprocessing
                    }
                } else {
                    method(it, "Warn", vec![pl::s("Invalid or extraneous FLIR chunk(s)")])?;
                }
            } else if is("^PARROT\\0(II\\x2a\\0|MM\\0\\x2a)", "") {
                // (don't know if this could span multiple segments)
                let tag_table_ptr = table(it, "Image::ExifTool::JPEG::Main")?;
                method(it, "HandleTag", vec![tag_table_ptr, pl::s("APP1"), seg.clone()])?;
                dump_type = pl::s("Parrot");
            } else {
                // Hmmm.  Could be XMP, let's see
                let mut processed = Value::Undef;
                if is("^(http|XMP\\0)", "") || is("<(exif:|\\?xpacket)", "") {
                    dump_type = pl::s("XMP");
                    // also try to parse XMP with a non-standard header
                    // (note: this non-standard XMP is ignored when writing)
                    let start = if re_var(it, "^", "xmpAPP1hdr", "")?.is_match(&seg) { len_of(&it.get_scalar("Image::ExifTool::xmpAPP1hdr")) } else { n(0) };
                    let tag_table_ptr = table(it, "Image::ExifTool::XMP::Main")?;
                    dir_start(it, vec![start.clone()])?;
                    dinfo.set("DirName", pl::s(if start.truthy() { "XMP" } else { "XML" }));
                    processed = process_dir(it, &dinfo, tag_table_ptr)?;
                    if processed.truthy() && !start.truthy() {
                        method(it, "Warn", vec![pl::s("Non-standard header for APP1 XMP segment")])?;
                    }
                }
                if verbose.truthy() && !processed.truthy() {
                    method(it, "Warn", vec![cat(&[&pl::s("Ignored APP1 segment length "), &length, &pl::s(" (unknown header)")])])?;
                }
            }
        } else if mk(&marker, 0xe2) {
            // APP2 (ICC Profile, FPXR, MPF, InfiRay, URN, PreviewImage)
            if is("^ICC_PROFILE\\0", "") && pl::ge(&length, &n(14)) {
                dump_type = pl::s("ICC_Profile");
                // must concatenate profile chunks (note: handle the case where
                // some software erroneously writes zeros for the chunk counts)
                let chunk_num = pl::call(it, "Image::ExifTool::Get8u", vec![seg_data_pt.clone(), n(12)])?;
                let chunks_tot = pl::call(it, "Image::ExifTool::Get8u", vec![seg_data_pt.clone(), n(13)])?;
                if verbose.truthy() {
                    print(it, &[&indent, &pl::s("ICC_Profile chunk "), &chunk_num, &pl::s(" of "), &chunks_tot, &pl::s("\n")])?;
                }
                if !icc_chunks_total.is_undef() {
                    // abort parsing ICC_Profile if the total chunk count is inconsistent
                    if !pl::num_eq(&chunks_tot, &icc_chunks_total) {
                        icc_chunk_count = Value::Undef;
                    }
                } else {
                    icc_chunk_count = n(0);
                    icc_chunks_total = chunks_tot.clone();
                    if !chunks_tot.truthy() {
                        method(it, "Warn", vec![pl::s("ICC_Profile chunk count is zero")])?;
                    }
                }
                if !icc_chunk_count.is_undef() {
                    let ci = chunk_num.to_i64().max(0) as usize;
                    if icc_chunk.len() <= ci {
                        icc_chunk.resize(ci + 1, Value::Undef);
                    }
                    let chunk = substr(it, &seg, &n(14), None)?;
                    if !icc_chunk[ci].is_undef() {
                        method(it, "Warn", vec![pl::s("Duplicate ICC_Profile chunk number(s)")])?;
                        icc_chunk[ci] = pl::concat(&icc_chunk[ci], &chunk);
                    } else {
                        icc_chunk[ci] = chunk;
                    }
                    // process profile if we have all of the chunks
                    icc_chunk_count = pl::add(&icc_chunk_count, &n(1));
                    if pl::ge(&icc_chunk_count, &icc_chunks_total) {
                        let icc_profile = cell(pl::s(""));
                        for c in icc_chunk.iter() {
                            if !c.is_undef() {
                                s_(&icc_profile, pl::concat(&g(&icc_profile), c));
                            }
                        }
                        icc_chunk.clear(); // free memory
                        let tag_table_ptr = table(it, "Image::ExifTool::ICC_Profile::Main")?;
                        let l = len_of(&g(&icc_profile));
                        let di = Value::new_hash();
                        di.set("DataPt", Value::ScalarRef(icc_profile));
                        di.set("DataPos", pl::add(&seg_pos, &n(14)));
                        di.set("DataLen", l.clone());
                        di.set("DirStart", n(0));
                        di.set("DirLen", l);
                        di.set("Parent", marker_name.clone());
                        process_dir(it, &di, tag_table_ptr)?;
                        icc_chunk_count = Value::Undef; // prevent reprocessing
                    }
                } else {
                    method(it, "Warn", vec![pl::s("Invalid or extraneous ICC_Profile chunk(s)")])?;
                }
            } else if is("^FPXR\\0", "") {
                if pl::gt(&fast, &n(1)) {
                    continue; // skip processing for very fast
                }
                dump_type = pl::s("FPXR");
                let tag_table_ptr = table(it, "Image::ExifTool::FlashPix::Main")?;
                // set flag if this is the last FPXR segment
                let last = !(pl::num_eq(&next_marker, &marker) && re("^FPXR\\0", "").is_match(&next_seg_data_pt.deref()));
                dinfo.set("LastFPXR", Value::from(last));
                process_dir(it, &dinfo, tag_table_ptr)?;
            } else if is("^MPF\\0", "") {
                dump_type = Value::Undef; // (will be dumped here)
                dir_start(it, vec![n(4), n(4)])?;
                dinfo.set("Multi", n(1)); // the MP Attribute IFD will be MPF1
                if html_dump.truthy() {
                    method(it, "HDump", vec![pl::sub(&seg_pos, &n(4)), n(4), pl::s("APP2 header"), cat(&[&pl::s("Data size: "), &length, &pl::s(" bytes")])])?;
                    method(it, "HDump", vec![seg_pos.clone(), n(4), pl::s("MPF header"), pl::s("APP2 data type: MPF")])?;
                    dump_end = pl::add(&seg_pos, &length);
                }
                // extract the MPF information (it is in standard TIFF format)
                let tag_table_ptr = table(it, "Image::ExifTool::MPF::Main")?;
                method(it, "ProcessTIFF", vec![dinfo.clone(), tag_table_ptr])?;
            } else if is("^....IJPEG\\0", "s") {
                dump_type = pl::s("InfiRay Version");
                self_.set("HasIJPEG", n(1));
                set_bo(it, pl::s("II"))?;
                let tag_table_ptr = table(it, "Image::ExifTool::InfiRay::Version")?;
                process_dir(it, &dinfo, tag_table_ptr)?;
            } else if let Some(m) = re("^(|QVGA\\0|BGTH)\\xff\\xd8\\xff[\\xdb\\xe0\\xe1]", "").m(&seg) {
                // Samsung/GE/GoPro="", BenQ DC C1220/Pentacon/Polaroid="QVGA\0",
                // Digilife DDC-690/Rollei="BGTH"
                dump_type = pl::s("Preview Image");
                preview = substr(it, &seg, &len_of(&cap(&m, 1)), None)?;
            } else if is("^urn:", "") {
                // (found in Apple HDR images)
                dump_type = pl::s("URN");
                use_jpeg_main = n(1);
            } else if preview.truthy() {
                dump_type = pl::s("Preview Image");
                preview = pl::concat(&preview, &seg);
            }
            if preview.truthy() && !seq(&next_marker, &marker) {
                found_tag_call(it, &self_, vec![pl::s("PreviewImage"), preview.clone()])?;
                preview = Value::Undef;
            }
        } else if mk(&marker, 0xe3) {
            // APP3 (Kodak "Meta", Stim)
            if is("^(Meta|META|Exif)\\0\\0", "") {
                dump_type = Value::Undef; // (will be dumped here)
                dir_start(it, vec![n(6), n(6)])?;
                if html_dump.truthy() {
                    method(it, "HDump", vec![pl::sub(&seg_pos, &n(4)), n(10), pl::s("APP3 Meta header")])?;
                    dump_end = pl::add(&seg_pos, &length);
                }
                let tag_table_ptr = table(it, "Image::ExifTool::Kodak::Meta")?;
                method(it, "ProcessTIFF", vec![dinfo.clone(), tag_table_ptr])?;
            } else if is("^Stim\\0", "") {
                dump_type = Value::Undef; // (will be dumped here)
                dir_start(it, vec![n(6), n(6)])?;
                if html_dump.truthy() {
                    method(it, "HDump", vec![pl::sub(&seg_pos, &n(4)), n(4), pl::s("APP3 header"), cat(&[&pl::s("Data size: "), &length, &pl::s(" bytes")])])?;
                    method(it, "HDump", vec![seg_pos.clone(), n(5), pl::s("Stim header"), pl::s("APP3 data type: Stim")])?;
                    dump_end = pl::add(&seg_pos, &length);
                }
                // extract the Stim information (it is in standard TIFF format)
                let tag_table_ptr = table(it, "Image::ExifTool::Stim::Main")?;
                method(it, "ProcessTIFF", vec![dinfo.clone(), tag_table_ptr])?;
            } else if is("^_JPSJPS_", "") {
                dump_type = pl::s("JPS");
                if pl::str_eq(&self_.get("FILE_TYPE"), "JPEG") {
                    method(it, "OverrideFileType", vec![pl::s("JPS")])?;
                }
                set_bo(it, pl::s("MM"))?;
                let tag_table_ptr = table(it, "Image::ExifTool::JPEG::JPS")?;
                process_dir(it, &dinfo, tag_table_ptr)?;
            } else if self_.get("HasIJPEG").truthy() || pl::str_eq(&self_.get("Make"), "DJI") {
                dump_type = pl::s(if self_.get("HasIJPEG").truthy() { "InfiRay ImagingData" } else { "DJI ThermalData" });
                // add this data to the combined data if it exists
                let mut data_pt = seg_data_pt.clone();
                if !g(&combined_seg_data).is_undef() {
                    s_(&combined_seg_data, pl::concat(&g(&combined_seg_data), &seg));
                    data_pt = Value::ScalarRef(combined_seg_data.clone());
                }
                if pl::num_eq(&next_marker, &marker) {
                    if g(&combined_seg_data).is_undef() {
                        s_(&combined_seg_data, seg.clone());
                    }
                } else {
                    // process InfiRay/DJI thermal data
                    let tag_table_ptr = table(it, "Image::ExifTool::JPEG::Main")?;
                    method(it, "HandleTag", vec![tag_table_ptr, pl::s("APP3"), data_pt.deref()])?;
                    s_(&combined_seg_data, Value::Undef);
                }
            } else if is("^\\xff\\xd8\\xff\\xdb", "") {
                dump_type = pl::s("PreviewImage"); // (Samsung, HP, BenQ)
                preview = seg.clone();
            }
            if preview.truthy() && !pl::str_eq(&next_marker, "228") {
                // this preview continues in APP4
                found_tag_call(it, &self_, vec![pl::s("PreviewImage"), preview.clone()])?;
                preview = Value::Undef;
            }
        } else if mk(&marker, 0xe4) {
            // APP4 (InfiRay, "SCALADO", FPXR, DJI, PreviewImage)
            if is("^SCALADO\\0", "") && pl::ge(&length, &n(16)) {
                dump_type = pl::s("SCALADO");
                let up = crate::perl::pack::unpack(b"x8n2N", &seg.to_bytes()).unwrap_or_default();
                let (num, idx, len) = (arg(&up, 0), arg(&up, 1), arg(&up, 2));
                // assume that the segments are in order and just concatinate them
                if g(&scalado).is_undef() {
                    s_(&scalado, pl::s(""));
                }
                let chunk = substr(it, &seg, &n(16), None)?;
                s_(&scalado, pl::concat(&g(&scalado), &chunk));
                if pl::num_eq(&idx, &pl::sub(&num, &n(1))) {
                    if !pl::num_eq(&len, &len_of(&g(&scalado))) {
                        method(it, "Warn", vec![pl::s("Possibly corrupted APP4 SCALADO data"), n(1)])?;
                    }
                    let di = Value::new_hash();
                    di.set("Parent", marker_name.clone());
                    di.set("DataPt", Value::ScalarRef(scalado.clone()));
                    let tag_table_ptr = table(it, "Image::ExifTool::Scalado::Main")?;
                    process_dir(it, &di, tag_table_ptr)?;
                    s_(&scalado, Value::Undef);
                }
            } else if is("^Qualcomm Dual Camera Attributes", "") {
                dump_type = pl::s("Qualcomm Dual Camera");
                let tag_table_ptr = table(it, "Image::ExifTool::Qualcomm::DualCamera")?;
                dir_start(it, vec![n(31)])?;
                dinfo.set("DirName", pl::s("Qualcomm Dual Camera"));
                process_dir(it, &dinfo, tag_table_ptr)?;
            } else if is("^FPXR\\0", "") {
                if pl::gt(&fast, &n(1)) {
                    continue; // skip processing for very fast
                }
                dump_type = pl::s("FPXR");
                let tag_table_ptr = table(it, "Image::ExifTool::FlashPix::Main")?;
                // set flag if this is the last FPXR segment
                let last = !(pl::num_eq(&next_marker, &marker) && re("^FPXR\\0", "").is_match(&next_seg_data_pt.deref()));
                dinfo.set("LastFPXR", Value::from(last));
                process_dir(it, &dinfo, tag_table_ptr)?;
            } else if pl::str_eq(&self_.get("Make"), "DJI") && is("^\\xaa\\x55\\x12\\x06", "") {
                dump_type = pl::s("DJI ThermalParams");
                dir_start(it, vec![n(0), n(0)])?;
                let tag_table_ptr = table(it, "Image::ExifTool::DJI::ThermalParams")?;
                process_dir(it, &dinfo, tag_table_ptr)?;
            } else if let Some(m) = if pl::str_eq(&self_.get("Make"), "DJI") { re("^(.{32})?.{32}\\x2c\\x01\\x20\\0", "s").m(&seg) } else { None } {
                dump_type = pl::s("DJI ThermalParams2");
                dir_start(it, vec![if cap(&m, 1).truthy() { n(32) } else { n(0) }, n(0)])?;
                let tag_table_ptr = table(it, "Image::ExifTool::DJI::ThermalParams2")?;
                process_dir(it, &dinfo, tag_table_ptr)?;
            } else if pl::str_eq(&self_.get("Make"), "DJI") && is("^.{32}\\xaa\\x55\\x38\\0", "s") {
                dump_type = pl::s("DJI ThermalParams3");
                dir_start(it, vec![n(32), n(0)])?;
                let tag_table_ptr = table(it, "Image::ExifTool::DJI::ThermalParams3")?;
                process_dir(it, &dinfo, tag_table_ptr)?;
            } else if self_.get("HasIJPEG").truthy() && pl::ge(&length, &n(120)) {
                dump_type = pl::s("InfiRay Factory");
                set_bo(it, pl::s("II"))?;
                let tag_table_ptr = table(it, "Image::ExifTool::InfiRay::Factory")?;
                process_dir(it, &dinfo, tag_table_ptr)?;
            } else if preview.truthy() {
                // continued Samsung S1060 preview from APP3
                dump_type = pl::s("PreviewImage");
                preview = pl::concat(&preview, &seg);
            }
            // (also seen "QTI Debug Metadata\0" segment in some newer Samsung images)
            // BenQ DC E1050 continues preview in APP5
            if preview.truthy() && !pl::str_eq(&next_marker, "229") {
                found_tag_call(it, &self_, vec![pl::s("PreviewImage"), preview.clone()])?;
                preview = Value::Undef;
            }
        } else if mk(&marker, 0xe5) {
            // APP5 (InfiRay, Ricoh "RMETA")
            if is("^RMETA\\0", "") {
                // (NOTE: apparently these may span multiple segments, but I haven't seen
                // a sample like this, so multi-segment support hasn't yet been implemented)
                dump_type = pl::s("Ricoh RMETA");
                dir_start(it, vec![n(6), n(6)])?;
                let tag_table_ptr = table(it, "Image::ExifTool::Ricoh::RMETA")?;
                process_dir(it, &dinfo, tag_table_ptr)?;
            } else if is("^ssuniqueid\\0", "") {
                let tag_table_ptr = table(it, "Image::ExifTool::Samsung::APP5")?;
                let v = substr(it, &seg, &n(11), None)?;
                method(it, "HandleTag", vec![tag_table_ptr, pl::s("ssuniqueid"), v])?;
            } else if pl::str_eq(&self_.get("Make"), "DJI") {
                dump_type = pl::s("DJI ThermalCal");
                let tag_table_ptr = table(it, "Image::ExifTool::JPEG::Main")?;
                method(it, "HandleTag", vec![tag_table_ptr, pl::s("APP5"), seg.clone()])?;
            } else if self_.get("HasIJPEG").truthy() && pl::ge(&length, &n(38)) {
                dump_type = pl::s("InfiRay Picture");
                set_bo(it, pl::s("II"))?;
                let tag_table_ptr = table(it, "Image::ExifTool::InfiRay::Picture")?;
                process_dir(it, &dinfo, tag_table_ptr)?;
            } else if preview.truthy() {
                dump_type = pl::s("PreviewImage");
                preview = pl::concat(&preview, &seg);
                found_tag_call(it, &self_, vec![pl::s("PreviewImage"), preview.clone()])?;
                preview = Value::Undef;
            }
        } else if mk(&marker, 0xe6) {
            // APP6 (InfiRay, Toshiba EPPIM, NITF, HP_TDHD)
            if is("^EPPIM\\0", "") {
                dump_type = Value::Undef; // (will be dumped here)
                dir_start(it, vec![n(6), n(6)])?;
                if html_dump.truthy() {
                    method(it, "HDump", vec![pl::sub(&seg_pos, &n(4)), n(10), pl::s("APP6 EPPIM header")])?;
                    dump_end = pl::add(&seg_pos, &length);
                }
                let tag_table_ptr = table(it, "Image::ExifTool::JPEG::EPPIM")?;
                method(it, "ProcessTIFF", vec![dinfo.clone(), tag_table_ptr])?;
            } else if is("^NITF\\0", "") {
                dump_type = pl::s("NITF");
                set_bo(it, pl::s("MM"))?;
                let tag_table_ptr = table(it, "Image::ExifTool::JPEG::NITF")?;
                dir_start(it, vec![n(5)])?;
                process_dir(it, &dinfo, tag_table_ptr)?;
            } else if is("^TDHD\\x01\\0\\0\\0", "") && pl::gt(&length, &n(12)) {
                // HP Photosmart R837 APP6 "TDHD" segment
                dump_type = pl::s("TDHD");
                let tag_table_ptr = table(it, "Image::ExifTool::HP::TDHD")?;
                // (ignore first TDHD element because size includes 12-byte tag header)
                dir_start(it, vec![n(12)])?;
                process_dir(it, &dinfo, tag_table_ptr)?;
            } else if is("^GoPro\\0", "") {
                // GoPro segment
                dump_type = pl::s("GoPro");
                let tag_table_ptr = table(it, "Image::ExifTool::GoPro::GPMF")?;
                dir_start(it, vec![n(6)])?;
                process_dir(it, &dinfo, tag_table_ptr)?;
            } else if is("^DTAT\\0\\0.\\{", "s") {
                dump_type = pl::s("DJI_DTAT");
                let tag_table_ptr = table(it, "Image::ExifTool::JPEG::Main")?;
                method(it, "HandleTag", vec![tag_table_ptr, pl::s("APP6"), seg.clone()])?;
            } else if self_.get("HasIJPEG").truthy() && pl::ge(&length, &n(129)) {
                dump_type = pl::s("InfiRay MixMode");
                set_bo(it, pl::s("II"))?;
                let tag_table_ptr = table(it, "Image::ExifTool::InfiRay::MixMode")?;
                process_dir(it, &dinfo, tag_table_ptr)?;
            }
        } else if mk(&marker, 0xe7) {
            // APP7 (InfiRay, Pentax, Huawei, Qualcomm)
            if let Some(m) = re("^(PENTAX |RICOH)\\0(II|MM)", "").m(&seg) {
                // found in K-3 and Ricoh GR_IV images (is this multi-segment??)
                set_bo(it, cap(&m, 2))?;
                dump_type = Value::Undef; // (dump this ourself)
                let hdr_len = pl::add(&len_of(&cap(&m, 1)), &n(3));
                let tag_table_ptr = table(it, "Image::ExifTool::Pentax::Main")?;
                dir_start(it, vec![hdr_len.clone(), n(0)])?;
                dinfo.set("DirName", pl::s("Pentax APP7"));
                if html_dump.truthy() {
                    method(it, "HDump", vec![pl::sub(&seg_pos, &n(4)), n(4), pl::s("APP7 header"), cat(&[&pl::s("Data size: "), &length, &pl::s(" bytes")])])?;
                    method(it, "HDump", vec![seg_pos.clone(), hdr_len.clone(), pl::s("Pentax header"), pl::s("APP7 data type: Pentax")])?;
                    dump_end = pl::add(&seg_pos, &length);
                }
                process_dir(it, &dinfo, tag_table_ptr)?;
            } else if let Some(m) = re("^HUAWEI\\0\\0(II|MM)", "").m(&seg) {
                set_bo(it, cap(&m, 1))?;
                dump_type = Value::Undef; // (dump this ourself)
                let hdr_len = n(16);
                let tag_table_ptr = table(it, "Image::ExifTool::Unknown::Main")?;
                dir_start(it, vec![hdr_len.clone(), n(8)])?;
                dinfo.set("DirName", pl::s("Huawei APP7"));
                if html_dump.truthy() {
                    method(it, "HDump", vec![pl::sub(&seg_pos, &n(4)), n(4), pl::s("APP7 header"), cat(&[&pl::s("Data size: "), &length, &pl::s(" bytes")])])?;
                    method(it, "HDump", vec![seg_pos.clone(), hdr_len.clone(), pl::s("Huawei header"), pl::s("APP7 data type: Huawei")])?;
                    dump_end = pl::add(&seg_pos, &length);
                }
                self_.set("SET_GROUP0", pl::s("APP7"));
                self_.set("SET_GROUP1", pl::s("Huawei"));
                process_dir(it, &dinfo, tag_table_ptr)?;
                self_.delete("SET_GROUP0");
                self_.delete("SET_GROUP1");
            } else if is("^DJI-DBG\\0", "") {
                dump_type = pl::s("DJI Info");
                let tag_table_ptr = table(it, "Image::ExifTool::DJI::Info")?;
                dir_start(it, vec![n(8), n(0)])?;
                self_.set("SET_GROUP0", pl::s("APP7"));
                process_dir(it, &dinfo, tag_table_ptr)?;
                self_.delete("SET_GROUP0");
            } else if is("^\\x1aQualcomm Camera Attributes", "") {
                // found in HP iPAQ_VoiceMessenger
                dump_type = pl::s("Qualcomm");
                let tag_table_ptr = table(it, "Image::ExifTool::Qualcomm::Main")?;
                dir_start(it, vec![n(27)])?;
                dinfo.set("DirName", pl::s("Qualcomm"));
                process_dir(it, &dinfo, tag_table_ptr)?;
            } else if self_.get("HasIJPEG").truthy() && pl::ge(&length, &n(32)) {
                dump_type = pl::s("InfiRay OpMode");
                set_bo(it, pl::s("II"))?;
                let tag_table_ptr = table(it, "Image::ExifTool::InfiRay::OpMode")?;
                process_dir(it, &dinfo, tag_table_ptr)?;
            }
        } else if mk(&marker, 0xe8) {
            // APP8 (InfiRay, SPIFF)
            // my sample SPIFF has 32 bytes of data, but spec states 30
            if is("^SPIFF\\0", "") && pl::num_eq(&length, &n(32)) {
                dump_type = pl::s("SPIFF");
                dir_start(it, vec![n(6)])?;
                let tag_table_ptr = table(it, "Image::ExifTool::JPEG::SPIFF")?;
                process_dir(it, &dinfo, tag_table_ptr)?;
            } else if self_.get("HasIJPEG").truthy() && pl::ge(&length, &n(32)) {
                dump_type = pl::s("InfiRay Isothermal");
                set_bo(it, pl::s("II"))?;
                let tag_table_ptr = table(it, "Image::ExifTool::InfiRay::Isothermal")?;
                process_dir(it, &dinfo, tag_table_ptr)?;
            } else if is("^SEAL\\0", "") {
                dump_type = pl::s("SEAL");
                dir_start(it, vec![n(5)])?;
                let t = table(it, "Image::ExifTool::XMP::SEAL")?;
                process_dir(it, &dinfo, t)?;
            }
        } else if mk(&marker, 0xe9) {
            // APP9 (InfiRay, Media Jukebox)
            if is("^Media Jukebox\\0", "") && pl::gt(&length, &n(22)) {
                dump_type = pl::s("MediaJukebox");
                // (start parsing after the "<MJMD>")
                dir_start(it, vec![n(22)])?;
                dinfo.set("DirName", pl::s("MediaJukebox"));
                runtime::require(it, "Image::ExifTool::XMP")?;
                let tag_table_ptr = table(it, "Image::ExifTool::JPEG::MediaJukebox")?;
                let p = code_ref(it, "Image::ExifTool::XMP::ProcessXMP");
                method(it, "ProcessDirectory", vec![dinfo.clone(), tag_table_ptr, p])?;
            } else if self_.get("HasIJPEG").truthy() && pl::ge(&length, &n(768)) {
                dump_type = pl::s("InfiRay Sensor");
                set_bo(it, pl::s("II"))?;
                let tag_table_ptr = table(it, "Image::ExifTool::InfiRay::Sensor")?;
                process_dir(it, &dinfo, tag_table_ptr)?;
            } else if is("^SEAL\\0", "") {
                dump_type = pl::s("SEAL");
                dir_start(it, vec![n(5)])?;
                let t = table(it, "Image::ExifTool::XMP::SEAL")?;
                process_dir(it, &dinfo, t)?;
            }
        } else if mk(&marker, 0xea) {
            // APP10 (PhotoStudio Unicode comments, HDR gain curve)
            if is("^UNICODE\\0", "") {
                dump_type = pl::s("PhotoStudio");
                let v = substr(it, &seg, &n(8), None)?;
                let comment = method(it, "Decode", vec![v, pl::s("UTF16"), pl::s("MM")])?;
                found_tag_call(it, &self_, vec![pl::s("Comment"), comment])?;
            } else if is("^AROT\\0\\0.{4}", "s") {
                dump_type = pl::s("AROT"); // (HDR gain curve? PH guess)
                use_jpeg_main = n(1);
            }
        } else if mk(&marker, 0xeb) {
            // APP11 (JPEG-HDR, JUMBF)
            if is("^HDR_RI ", "") {
                dump_type = pl::s("JPEG-HDR");
                let mut data_pt = seg_data_pt.clone();
                if !g(&combined_seg_data).is_undef() {
                    if let Some((p, _)) = match_g(b"~\\0", "", &seg)? {
                        let tail = substr(it, &seg, &n(p as i64), None)?;
                        s_(&combined_seg_data, pl::concat(&g(&combined_seg_data), &tail));
                    } else {
                        method(it, "Warn", vec![pl::s("Invalid format for JPEG-HDR extended segment")])?;
                    }
                    data_pt = Value::ScalarRef(combined_seg_data.clone());
                }
                if pl::num_eq(&next_marker, &marker) && re("^HDR_RI ", "").is_match(&next_seg_data_pt.deref()) {
                    if g(&combined_seg_data).is_undef() {
                        s_(&combined_seg_data, seg.clone());
                    }
                } else {
                    let tag_table_ptr = table(it, "Image::ExifTool::JPEG::HDR")?;
                    let di = Value::new_hash();
                    di.set("DataPt", data_pt);
                    process_dir(it, &di, tag_table_ptr)?;
                    s_(&combined_seg_data, Value::Undef);
                }
            } else if re("^(JP..)", "s").is_match(&seg) && pl::ge(&len_of(&seg), &n(16)) {
                // JUMBF extension marker
                dump_type = pl::s("JUMBF");
                set_bo(it, pl::s("MM"))?;
                // (sequence should start from 1, but some software incorrectly writes 0)
                let seq_ = pl::call(it, "Image::ExifTool::Get32u", vec![seg_data_pt.clone(), n(4)])?;
                let mut len = pl::call(it, "Image::ExifTool::Get32u", vec![seg_data_pt.clone(), n(8)])?;
                let mut type_ = substr(it, &seg_data_pt.deref(), &n(12), Some(&n(4)))?;
                // a Microsoft bug writes $len and $type incorrectly as little-endian
                if pl::str_eq(&type_, "bmuj") {
                    method(it, "Warn", vec![pl::s("Wrong byte order in C2PA APP11 JUMBF header")])?;
                    type_ = pl::s("jumb");
                    let up = crate::perl::pack::unpack(b"x8V", &seg_data_pt.deref().to_bytes()).unwrap_or_default();
                    len = arg(&up, 0);
                    // fix the header
                    let repl = cat(&[&pl::call(it, "Image::ExifTool::Set32u", vec![len.clone()])?, &type_]);
                    let cur = seg_data_pt.deref().to_bytes();
                    if let Some((x, y)) = substr_range(cur.len(), 8, Some(8)) {
                        let mut nb = cur[..x].to_vec();
                        nb.extend_from_slice(&repl.to_bytes());
                        nb.extend_from_slice(&cur[y..]);
                        set_deref(&seg_data_pt, Value::bytes(nb))?;
                    } else {
                        return crate::interp::eval::die("substr outside of string");
                    }
                }
                let hdr_len;
                if pl::num_eq(&len, &n(1)) && pl::ge(&len_of(&seg_data_pt.deref()), &n(24)) {
                    // (haven't seen this with the Microsoft bug)
                    len = pl::call(it, "Image::ExifTool::Get64u", vec![seg_data_pt.deref(), n(16)])?;
                    hdr_len = n(16);
                } else {
                    hdr_len = n(8);
                }
                let tk = type_.to_bytes();
                if !jumbf_chunk.get(&tk).truthy() {
                    jumbf_chunk.set(&tk, Value::new_array(vec![pl::s("")]));
                }
                let chunks = jumbf_chunk.get(&tk);
                if pl::lt(&len, &hdr_len) {
                    method(it, "Warn", vec![pl::s("Invalid JUMBF segment")])?;
                } else if !chunks.at(seq_.to_i64()).is_undef() && pl::length(&chunks.at(seq_.to_i64())).truthy() {
                    method(it, "Warn", vec![pl::s("Duplicate JUMBF sequence number")])?;
                } else {
                    if !seq_.truthy() {
                        method(it, "Warn", vec![pl::s("Incorrect JUMBF sequence numbering (should start from 0, not 1)")])?;
                    }
                    // add to list of JUMBF chunks
                    let c = substr(it, &seg_data_pt.deref(), &pl::add(&n(8), &hdr_len), None)?;
                    chunks.set_at(seq_.to_i64().max(0) as usize, c);
                    // check to see if we have a complete JUMBF box
                    let mut size = hdr_len.clone();
                    for c in chunks.items() {
                        if c.is_undef() {
                            size = n(0);
                            break;
                        }
                        size = pl::add(&size, &len_of(&c));
                    }
                    if pl::num_eq(&size, &len) {
                        let mut b = substr(it, &seg_data_pt.deref(), &n(8), Some(&hdr_len))?.to_bytes().to_vec();
                        for c in chunks.items() {
                            b.extend_from_slice(&c.to_bytes());
                        }
                        let buff = cell(Value::bytes(b));
                        dinfo.set("DataPt", Value::ScalarRef(buff));
                        dinfo.set("DataPos", pl::add(&seg_pos, &n(8))); // (shows correct offsets for single-segment JUMBF)
                        dinfo.set("DirLen", size.clone());
                        dinfo.set("DataLen", size);
                        dinfo.set("DirName", pl::s("JUMBF"));
                        let tag_table_ptr = table(it, "Image::ExifTool::Jpeg2000::Main")?;
                        process_dir(it, &dinfo, tag_table_ptr)?;
                        jumbf_chunk.delete(&tk);
                    }
                }
            }
        } else if mk(&marker, 0xec) {
            // APP12 (Ducky, Picture Info)
            if is("^Ducky", "") {
                dump_type = pl::s("Ducky");
                dir_start(it, vec![n(5)])?;
                let tag_table_ptr = table(it, "Image::ExifTool::APP12::Ducky")?;
                process_dir(it, &dinfo, tag_table_ptr)?;
            } else {
                let tag_table_ptr = table(it, "Image::ExifTool::APP12::PictureInfo")?;
                if process_dir(it, &dinfo, tag_table_ptr)?.truthy() {
                    dump_type = pl::s("Picture Info");
                }
            }
        } else if mk(&marker, 0xed) {
            // APP13 (Photoshop, Adobe_CM)
            let mut is_old = false;
            let is_ps = re_var(it, "^", "psAPP13hdr", "")?.is_match(&seg) || {
                is_old = re_var(it, "^", "psAPP13old", "")?.is_match(&seg);
                is_old
            };
            if is_ps {
                dump_type = pl::s("Photoshop");
                // add this data to the combined data if it exists
                let mut data_pt = seg_data_pt.clone();
                if !g(&combined_seg_data).is_undef() {
                    let hl = len_of(&it.get_scalar("Image::ExifTool::psAPP13hdr"));
                    let tail = substr(it, &seg, &hl, None)?;
                    s_(&combined_seg_data, pl::concat(&g(&combined_seg_data), &tail));
                    data_pt = Value::ScalarRef(combined_seg_data.clone());
                }
                // peek ahead to see if the next segment is photoshop data too
                if pl::num_eq(&next_marker, &marker) && re_var(it, "^", "psAPP13hdr", "")?.is_match(&next_seg_data_pt.deref()) {
                    // initialize combined data if necessary
                    if g(&combined_seg_data).is_undef() {
                        s_(&combined_seg_data, seg.clone());
                    }
                    // (will handle the Photoshop data the next time around)
                } else {
                    let hdr_len = n(if is_old { 27 } else { 14 });
                    // process APP13 Photoshop record
                    let tag_table_ptr = table(it, "Image::ExifTool::Photoshop::Main")?;
                    let l = len_of(&data_pt.deref());
                    let di = Value::new_hash();
                    di.set("DataPt", data_pt.clone());
                    di.set("DataPos", seg_pos.clone());
                    di.set("DataLen", l.clone());
                    di.set("DirStart", hdr_len.clone()); // directory starts after identifier
                    di.set("DirLen", pl::sub(&l, &hdr_len));
                    di.set("Parent", marker_name.clone());
                    process_dir(it, &di, tag_table_ptr)?;
                    s_(&combined_seg_data, Value::Undef);
                }
            } else if is("^Adobe_CM", "") {
                dump_type = pl::s("Adobe_CM");
                set_bo(it, pl::s("MM"))?;
                let tag_table_ptr = table(it, "Image::ExifTool::JPEG::AdobeCM")?;
                dir_start(it, vec![n(8)])?;
                process_dir(it, &dinfo, tag_table_ptr)?;
            }
        } else if mk(&marker, 0xee) {
            // APP14 (Adobe)
            if is("^Adobe", "") {
                // extract as a block if requested, or if copying tags from file
                if req.get("adobe").truthy() ||
                    // (not extracted normally, so check TAGS_FROM_FILE)
                    (self_.get("TAGS_FROM_FILE").truthy() && !vget(&self_, &[b"EXCL_TAG_LOOKUP", b"adobe"]).truthy())
                {
                    found_tag_call(it, &self_, vec![pl::s("Adobe"), seg.clone()])?;
                }
                dump_type = pl::s("Adobe");
                set_bo(it, pl::s("MM"))?;
                let tag_table_ptr = table(it, "Image::ExifTool::JPEG::Adobe")?;
                dir_start(it, vec![n(5)])?;
                process_dir(it, &dinfo, tag_table_ptr)?;
            }
        } else if mk(&marker, 0xef) {
            // APP15 (GraphicConverter)
            match re("^Q\\s*(\\d+)", "").m(&seg) {
                Some(m) if pl::num_eq(&length, &n(4)) => {
                    dump_type = pl::s("GraphicConverter");
                    let tag_table_ptr = table(it, "Image::ExifTool::JPEG::GraphConv")?;
                    method(it, "HandleTag", vec![tag_table_ptr, pl::s("Q"), cap(&m, 1)])?;
                }
                _ => {}
            }
        } else if mk(&marker, 0xfe) {
            // COM (JPEG comment)
            dump_type = pl::s("Comment");
            let mut v = seg_data_pt.deref();
            if re("\\0+$", "").subst(&mut v, false, |_| Vec::new()) > 0 {
                // some dumb softwares add null terminators
                set_deref(&seg_data_pt, v)?;
            }
            found_tag_call(it, &self_, vec![pl::s("Comment"), seg_data_pt.deref()])?;
        } else if mk(&marker, 0x64) {
            // CME (J2C comment and extension)
            dump_type = pl::s("Comment");
            if pl::gt(&length, &n(2)) {
                let up = crate::perl::pack::unpack(b"n", &seg.to_bytes()).unwrap_or_default();
                let reg = arg(&up, 0); // get registration value
                let mut val = substr(it, &seg, &n(2), None)?;
                if pl::num_eq(&reg, &n(1)) {
                    val = method(it, "Decode", vec![val, pl::s("Latin")])?;
                }
                // (actually an extension for $reg==65535, but store as binary comment)
                let v = if pl::num_eq(&reg, &n(0)) || pl::num_eq(&reg, &n(65535)) { Value::ScalarRef(cell(val)) } else { val };
                found_tag_call(it, &self_, vec![pl::s("Comment"), v])?;
            }
        } else if mk(&marker, 0x51) {
            // SIZ (J2C)
            let up = crate::perl::pack::unpack(b"x2N2", &seg.to_bytes()).unwrap_or_default();
            let (w, h) = (arg(&up, 0), arg(&up, 1));
            if !got_size.truthy() {
                got_size = n(1);
                found_tag_call(it, &self_, vec![pl::s("ImageWidth"), w])?;
                found_tag_call(it, &self_, vec![pl::s("ImageHeight"), h])?;
            }
        } else if !pl::num_eq(&m_and(0xf0), &n(0xe0)) {
            dump_type = cat(&[&marker_name, &pl::s(" segment")]);
            desc = cat(&[&pl::s("[JPEG "), &marker_name, &pl::s("]")]); // (other known JPEG segments)
        }
        if !dump_type.is_undef() {
            let seg = seg_data_pt.deref();
            if use_jpeg_main.truthy() {
                let tag_table_ptr = table(it, "Image::ExifTool::JPEG::Main")?;
                method(it, "HandleTag", vec![tag_table_ptr, marker_name.clone(), seg.clone()])?;
            }
            if !dump_type.truthy() && (options.get("Unknown").truthy() || options.get("Validate").truthy()) {
                let str_ = match re("^([\\x20-\\x7e]{1,20})\\0", "").m(&seg) {
                    Some(m) => cat(&[&pl::s(" '"), &cap(&m, 1), &pl::s("'")]),
                    None => pl::s(""),
                };
                if !xtra.truthy() {
                    xtra = pl::s("segment");
                }
                method(it, "Warn", vec![cat(&[&pl::s("Unknown "), &marker_name, &str_, &pl::s(" "), &xtra]), n(1)])?;
            }
            if html_dump.truthy() {
                if !desc.truthy() {
                    desc = cat(&[&marker_name, &if dump_type.truthy() { cat(&[&pl::s(" "), &dump_type]) } else { pl::s("") }, &pl::s(" segment")]);
                }
                method(it, "HDump", vec![pl::sub(&seg_pos, &n(4)), pl::add(&length, &n(4)), desc.clone(), tip.clone(), n(0x08)])?;
                dump_end = pl::add(&seg_pos, &length);
            }
        }
        set_deref(&seg_data_pt, Value::Undef)?;
    }
    // print verbose hash message if necessary
    if hashsize.truthy() && verbose.truthy() {
        print(it, &[&indent, &pl::s("(ImageDataHash: "), &hashsize, &pl::s(" bytes of JPEG image data)\n")])?;
    }
    // calculate JPEGDigest if requested
    if !dqt.is_empty() {
        runtime::require(it, "Image::ExifTool::JPEGDigest")?;
        pl::call(it, "Image::ExifTool::JPEGDigest::Calculate", vec![self_.clone(), dqt.clone(), sub_sampling.clone()])?;
    }
    // issue necessary warnings
    if !jumbf_chunk.is_empty() {
        method(it, "Warn", vec![pl::s("Invalid JUMBF size or missing JUMBF chunk")])?;
    }
    if !icc_chunk_count.is_undef() {
        method(it, "Warn", vec![pl::s("Incomplete ICC_Profile record"), n(1)])?;
    }
    if !flir_count.is_undef() {
        method(it, "Warn", vec![pl::s("Incomplete FLIR record"), n(1)])?;
    }
    if self_.get("PreviewError").truthy() {
        method(it, "Warn", vec![pl::s("Error reading PreviewImage"), n(1)])?;
    }
    if !success.truthy() {
        method(it, "Warn", vec![pl::s("JPEG format error")])?;
    }
    if path.len() > pn {
        if let Value::ArrayRef(p) = &path {
            p.borrow_mut().pop();
        }
    }
    Ok(n(1))
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
    it.define_native("Image::ExifTool::ProcessJPEG", process_jpeg);
}
