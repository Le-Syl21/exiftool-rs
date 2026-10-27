//! Image::ExifTool::PLIST (Image/ExifTool/PLIST.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read Apple PLIST information (binary, XML or JSON format)

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, perl_eval1};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, n, re};

const MODULE: &str = "Image::ExifTool::PLIST";

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// `$raf->Read($buff, $len)`: the count, with `buff` updated.
fn read(it: &mut Interp, raf: &Value, buff: &mut Value, len: Value) -> ER<Value> {
    let mut args = [buff.clone(), len];
    let r = pl::method_mut(it, raf, "Read", &mut args, Want::Scalar)?;
    *buff = args[0].clone();
    Ok(r.into_iter().next().unwrap_or_default())
}

/// `$raf->Read($buff, $len) == $len`
fn read_eq(it: &mut Interp, raf: &Value, buff: &mut Value, len: &Value) -> ER<bool> {
    let r = read(it, raf, buff, len.clone())?;
    Ok(pl::num_eq(&r, len))
}

/// `$raf->Seek(POS, WHENCE)`
fn seek(it: &mut Interp, raf: &Value, pos: Value, whence: i64) -> ER<bool> {
    Ok(pl::method(it, raf, "Seek", vec![pos, n(whence)])?.truthy())
}

/// `ord($ch)`
fn ord(v: &Value) -> Value {
    n(v.to_bytes().first().map_or(0, |b| *b as i64))
}

/// `"a" . "b" . ...`
fn cat(parts: &[Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

/// `$readProc{$size}`
fn read_proc(it: &mut Interp, size: &Value) -> Value {
    crate::runtime::lex_hash(it, MODULE, "readProc").get(size.to_bytes())
}

/// `&$proc(\$buff, $off)`
fn call_proc(it: &mut Interp, proc_: &Value, buff: &Value, off: Value) -> ER<Value> {
    Ok(
        pl::call_code(it, proc_, vec![pl::sref(buff.clone()), off], Want::Scalar)?
            .pop()
            .unwrap_or_default(),
    )
}

/// `$name =~ s/([^A-Za-z])([a-z])/$1\u$2/g; $name =~ tr/-_a-zA-Z0-9//dc;`
fn clean_name(name: &mut Value) {
    re(r"([^A-Za-z])([a-z])", "").subst(name, true, |c| {
        let mut v = cap(c, 1).to_bytes().to_vec();
        v.extend_from_slice(&pl::ucfirst(&cap(c, 2)).to_bytes());
        v
    }); // capitalize words
    pl::tr(name, "-_a-zA-Z0-9", "", "dc"); // remove illegal characters
}

/// `$$et{LastPListTag}` bookkeeping: allow list-behaviour only for
/// consecutive tags with the same ID.
fn last_plist_tag(et: &Value, tag_info: &Value) {
    let last = et.get("LastPListTag");
    if last.truthy() && last.to_bytes() != tag_info.to_bytes() {
        et.get_or_new_hash("LIST_TAGS").delete(last.to_bytes());
    }
    et.set("LastPListTag", tag_info.clone());
}

/// We found a PLIST XML property name/value
/// Inputs: 0) ExifTool object ref, 1) tag table ref
///         2) reference to array of XML property names (last is current property)
///         3) property value, 4) attribute hash ref (not used here)
/// Returns: 1 if valid tag was found
pub fn found_tag(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, tag_table_ptr, props, mut val) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    if props.is_empty() {
        return pl::ret(n(0));
    }
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    let mut keys = et.get("PListKeys");
    if !keys.truthy() {
        keys = Value::new_array(Vec::new());
        et.set("PListKeys", keys.clone());
    }

    let prop = props.at(-1);
    if pl::gt(&verbose, &n(1)) {
        let printable = pl::method(it, &et, "Printable", vec![val.clone()])?;
        pl::method(
            it,
            &et,
            "VPrint",
            vec![
                n(0),
                et.get("INDENT"),
                pl::s("["),
                pl::join("/", &props.items()),
                pl::s(" = "),
                printable,
                pl::s("]\n"),
            ],
        )?;
    }
    // un-escape XML character entities
    val = pl::call(it, "Image::ExifTool::XMP::UnescapeXML", vec![val])?;

    // handle the various PLIST properties
    if pl::str_eq(&prop, "data") {
        if re(r"^[0-9a-f]+$", "").is_match(&val) && pl::length(&val).to_i64() & 0x01 == 0 {
            // MODD files use ASCII-hex encoded "data"...
            let buff = pl::pack("H*", &[val.clone()])?;
            val = pl::sref(buff);
        } else {
            // ...but the PLIST DTD specifies Base64 encoding
            val = pl::call(it, "Image::ExifTool::XMP::DecodeBase64", vec![val])?;
        }
    } else if pl::str_eq(&prop, "date") {
        val = pl::call(it, "Image::ExifTool::XMP::ConvertXMPDate", vec![val])?;
    } else if pl::str_eq(&prop, "true") || pl::str_eq(&prop, "false") {
        val = pl::ucfirst(&prop);
    } else {
        // convert from UTF8 to ExifTool Charset
        val = pl::method(it, &et, "Decode", vec![val, pl::s("UTF8")])?;
        if pl::str_eq(&prop, "key") {
            let np = props.len() as i64;
            if np <= 3 {
                // top-level key should be plist/dict/key
                if let Value::ArrayRef(k) = &keys {
                    *k.borrow_mut() = vec![val];
                }
            } else if let Value::ArrayRef(k) = &keys {
                // save key names to be used in tag name
                let mut k = k.borrow_mut();
                while (k.len() as i64) < np - 3 {
                    k.push(pl::s(""));
                }
                while (k.len() as i64) > np - 2 {
                    k.pop();
                }
                let idx = (np - 3) as usize;
                while k.len() <= idx {
                    k.push(Value::Undef);
                }
                k[idx] = val;
            }
            return pl::ret(n(0));
        }
    }

    if keys.is_empty() {
        return pl::ret(n(0)); // can't store value if no associated key
    }

    let tag = pl::join("/", &keys.items()); // generate tag ID from 'key' values
    let mut tag_info = tag_table_ptr.get(tag.to_bytes());
    if !tag_info.truthy() {
        if verbose.truthy() {
            pl::method(
                it,
                &et,
                "VPrint",
                vec![
                    n(0),
                    et.get("INDENT"),
                    cat(&[pl::s("[adding "), tag.clone(), pl::s("]\n")]),
                ],
            )?;
        }
        // generate tag name from ID
        let mut name = tag.clone();
        re(r"^MetaDataList//", "").subst(&mut name, false, |_| Vec::new()); // shorten long MODD metadata tag names
        re(r"//name$", "").subst(&mut name, false, |_| Vec::new()); // remove unnecessary MODD "name" property
        clean_name(&mut name);
        tag_info = Value::new_hash();
        tag_info.set("Name", pl::ucfirst(&name));
        tag_info.set("List", n(1));
        if pl::str_eq(&prop, "date") {
            tag_info.get_or_new_hash("Groups").set("2", pl::s("Time"));
            tag_info.set("PrintConv", pl::s("$self->ConvertDateTime($val)"));
        }
        call(
            it,
            "AddTagToTable",
            vec![tag_table_ptr.clone(), tag.clone(), tag_info.clone()],
        )?;
    }
    // allow list-behaviour only for consecutive tags with the same ID
    last_plist_tag(&et, &tag_info);
    // override file type if applicable
    let plist_type = crate::runtime::lex_hash(it, MODULE, "plistType");
    let pt = plist_type.get(tag.to_bytes());
    if pt.truthy() && pl::str_eq(&et.get("FILE_TYPE"), "XMP") {
        pl::method(it, &et, "OverrideFileType", vec![pt])?;
    }
    // handle compressed PLIST/JSON data
    let proc_ = Value::Undef;
    if tag_info.get("CompressedPLIST").truthy()
        && pl::ref_of(&val) == "SCALAR"
        && !re(r"^bplist00", "").is_match(&val.deref())
    {
        if pl::eval_block(it, |it| {
            crate::runtime::require(it, "IO::Uncompress::RawInflate")
        })?
        .is_some()
        {
            let inflated = pl::sref(Value::Undef);
            if pl::call(
                it,
                "IO::Uncompress::RawInflate::rawinflate",
                vec![val.clone(), inflated.clone()],
            )?
            .truthy()
            {
                val = inflated;
            } else {
                pl::method(
                    it,
                    &et,
                    "Warn",
                    vec![cat(&[
                        pl::s("Error inflating PLIST::"),
                        tag_info.get("Name"),
                    ])],
                )?;
            }
        } else {
            pl::method(
                it,
                &et,
                "Warn",
                vec![pl::s(
                    "Install IO::Uncompress to decode compressed PLIST data",
                )],
            )?;
        }
    }
    // save the tag
    pl::method(
        it,
        &et,
        "HandleTag",
        vec![tag_table_ptr, tag, val, pl::s("ProcessProc"), proc_],
    )?;

    pl::ret(n(1))
}

/// Get big-endian 24-bit integer
/// Inputs: 0) data ref, 1) offset
/// Returns: integer value
pub fn get24u(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (data_pt, off) = (arg(&a, 0), arg(&a, 1));
    let s = pl::concat(
        &Value::bytes(b"\0"),
        &pl::substr(&data_pt.deref(), off.to_i64(), Some(3)),
    );
    pl::ret(pl::unpack1("N", &s)?)
}

/// Extract object from binary PLIST file at the current file position (ref 2)
/// Inputs: 0) ExifTool ref, 1) PLIST info ref, 2) parent tag ID (undef for top)
/// Returns: the object, or undef on error
pub fn extract_object(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, plist_info, parent) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let raf = plist_info.get("RAF");
    let mut buff = Value::Undef;
    let mut val = Value::Undef;

    if !read_eq(it, &raf, &mut buff, &n(1))? {
        return pl::ret_undef();
    }
    let typ = ops::shift_right(&ord(&buff), &n(4)).to_i64();
    let mut size = ops::bit_and(&ord(&buff), &n(0x0f));
    if typ == 0 {
        // null/bool/fill
        val = match size.to_i64() {
            0x00 => pl::s("<null>"),
            0x08 => pl::s("True"),
            0x09 => pl::s("False"),
            0x0f => pl::s("<fill>"),
            _ => Value::Undef,
        };
    } else if typ == 1 || typ == 2 || typ == 3 {
        // int, float or date
        size = ops::shift_left(&n(1), &size);
        let proc_ = if typ == 1 {
            read_proc(it, &size)
        } else {
            read_proc(it, &pl::add(&size, &n(0x100)))
        };
        if !proc_.truthy() {
            return pl::ret_undef();
        }
        if read_eq(it, &raf, &mut buff, &size)? {
            val = call_proc(it, &proc_, &buff, n(0))?;
        }
        if typ == 3 && !val.is_undef() {
            // date
            // dates are referenced to Jan 1, 2001 (11323 days from Unix time zero)
            val = call(
                it,
                "ConvertUnixTime",
                vec![pl::add(&val, &n(11323 * 24 * 3600)), n(1)],
            )?;
            plist_info.set("DateFormat", n(1));
        }
    } else if typ == 8 {
        // UID
        size = pl::add(&size, &n(1));
        if !read_eq(it, &raf, &mut buff, &size)? {
            return pl::ret_undef();
        }
        let proc_ = read_proc(it, &size);
        if proc_.truthy() {
            val = call_proc(it, &proc_, &buff, n(0))?;
        } else if pl::num_eq(&size, &n(16)) {
            crate::runtime::require(it, "Image::ExifTool::ASF")?;
            val = pl::call(it, "Image::ExifTool::ASF::GetGUID", vec![buff.clone()])?;
        } else {
            val = pl::concat(&pl::s("0x"), &pl::unpack1("H*", &buff)?);
        }
    } else {
        // $size is the size of the remaining types
        if pl::num_eq(&size, &n(0x0f)) {
            // size is stored in extra integer object
            size = extract_object(it, vec![et.clone(), plist_info.clone()], Want::Scalar)?
                .pop()
                .unwrap_or_default();
            if size.is_undef() || !re(r"^\d+$", "").is_match(&size) {
                return pl::ret_undef();
            }
        }
        if typ == 4 {
            // data
            if pl::lt(&size, &n(1000000))
                || pl::method(it, &et, "Options", vec![pl::s("Binary")])?.truthy()
            {
                if !read_eq(it, &raf, &mut buff, &size)? {
                    return pl::ret_undef();
                }
            } else {
                buff = cat(&[pl::s("Binary data "), size.clone(), pl::s(" bytes")]);
            }
            val = pl::sref(buff); // (return reference for binary data)
        } else if typ == 5 {
            // ASCII string
            if !read_eq(it, &raf, &mut val, &size)? {
                return pl::ret_undef();
            }
        } else if typ == 6 {
            // UCS-2BE string
            size = pl::mul(&size, &n(2));
            if !read_eq(it, &raf, &mut buff, &size)? {
                return pl::ret_undef();
            }
            val = pl::method(it, &et, "Decode", vec![buff.clone(), pl::s("UTF16")])?;
        // (might as well support surrogates too)
        } else if typ == 10 || typ == 12 || typ == 13 {
            // array, set or dict
            // the remaining types store a list of references
            let ref_size = plist_info.get("RefSize");
            let ref_proc = plist_info.get("RefProc");
            let num = if typ == 13 {
                pl::mul(&size, &n(2))
            } else {
                size.clone()
            };
            let len = pl::mul(&num, &ref_size);
            if !read_eq(it, &raf, &mut buff, &len)? {
                return pl::ret_undef();
            }
            let table = plist_info.get("Table");
            let mut refs: Vec<Value> = Vec::new();
            let mut array: Vec<Value> = Vec::new();
            let mut i = n(0);
            while pl::lt(&i, &num) {
                let r = call_proc(it, &ref_proc, &buff, pl::mul(&i, &ref_size))?;
                if pl::ge(&r, &n(table.len() as i64)) {
                    return pl::ret(n(0));
                }
                refs.push(r);
                i = pl::add(&i, &n(1));
            }
            if typ == 13 {
                // dict
                // prevent infinite recursion
                if !parent.is_undef() && pl::length(&parent).to_i64() > 1000 {
                    pl::method(
                        it,
                        &et,
                        "Warn",
                        vec![pl::s("Possible deep recursion while parsing PLIST")],
                    )?;
                    return pl::ret_undef();
                }
                let tag_table_ptr = plist_info.get("TagTablePtr");
                let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
                val = Value::new_hash(); // initialize return dictionary (will stay empty if tags are saved)
                let size_i = size.to_i64();
                let mut i = 0i64;
                while i < size_i {
                    'entry: {
                        // get the entry key
                        if !seek(it, &raf, table.at(refs[i as usize].to_i64()), 0)? {
                            return pl::ret_undef();
                        }
                        let mut key =
                            extract_object(it, vec![et.clone(), plist_info.clone()], Want::Scalar)?
                                .pop()
                                .unwrap_or_default();
                        if key.is_undef() || !pl::length(&key).truthy() {
                            break 'entry; // silently ignore bad dict entries
                        }
                        // get the entry value
                        if !seek(it, &raf, table.at(refs[(i + size_i) as usize].to_i64()), 0)? {
                            return pl::ret_undef();
                        }
                        // generate an ID for this tag
                        let tag = if !parent.is_undef() {
                            cat(&[parent.clone(), pl::s("/"), key.clone()])
                        } else {
                            key.clone()
                        };
                        plist_info.set("DateFormat", Value::Undef);
                        let obj = extract_object(
                            it,
                            vec![et.clone(), plist_info.clone(), tag.clone()],
                            Want::Scalar,
                        )?
                        .pop()
                        .unwrap_or_default();
                        if obj.is_undef() {
                            break 'entry;
                        }
                        if !tag_table_ptr.truthy() {
                            // make sure this is a valid structure field name
                            if key.is_undef() || !re(r"^[-_a-zA-Z0-9]+$", "").is_match(&key) {
                                key = pl::s(&format!("Tag{i}")); // (generate fake tag name if it had illegal characters)
                            } else if !re(r"^[_a-zA-Z]", "").is_match(&key) {
                                key = pl::concat(&pl::s("_"), &key); // (must begin with alpha or underline)
                            }
                            if !obj.is_undef() {
                                val.set(key.to_bytes(), obj);
                            }
                            break 'entry;
                        }
                        if pl::ref_of(&obj) == "HASH" {
                            break 'entry;
                        }
                        let mut tag_info = pl::method(
                            it,
                            &et,
                            "GetTagInfo",
                            vec![tag_table_ptr.clone(), tag.clone()],
                        )?;
                        if !tag_info.truthy() {
                            if verbose.truthy() {
                                pl::method(
                                    it,
                                    &et,
                                    "VPrint",
                                    vec![
                                        n(0),
                                        et.get("INDENT"),
                                        cat(&[pl::s("[adding "), tag.clone(), pl::s("]\n")]),
                                    ],
                                )?;
                            }
                            let mut name = tag.clone();
                            clean_name(&mut name);
                            if pl::length(&name).to_i64() < 2 || re(r"^[-0-9]", "").is_match(&name)
                            {
                                name = pl::concat(&pl::s("Tag"), &pl::ucfirst(&name));
                            }
                            tag_info = Value::new_hash();
                            tag_info.set("Name", pl::ucfirst(&name));
                            tag_info.set("List", n(1));
                            if plist_info.get("DateFormat").truthy() {
                                tag_info.get_or_new_hash("Groups").set("2", pl::s("Time"));
                                tag_info.set("PrintConv", pl::s("$self->ConvertDateTime($val)"));
                            }
                            call(
                                it,
                                "AddTagToTable",
                                vec![tag_table_ptr.clone(), tag.clone(), tag_info.clone()],
                            )?;
                        }
                        // allow list-behaviour only for consecutive tags with the same ID
                        last_plist_tag(&et, &tag_info);
                        pl::method(
                            it,
                            &et,
                            "HandleTag",
                            vec![tag_table_ptr.clone(), tag.clone(), obj],
                        )?;
                    }
                    i += 1;
                }
            } else {
                // extract the referenced objects
                for r in &refs {
                    if !seek(it, &raf, table.at(r.to_i64()), 0)? {
                        return pl::ret_undef(); // seek to this object
                    }
                    val = extract_object(
                        it,
                        vec![et.clone(), plist_info.clone(), parent.clone()],
                        Want::Scalar,
                    )?
                    .pop()
                    .unwrap_or_default();
                    if val.is_undef() || pl::ref_of(&val) == "HASH" {
                        continue;
                    }
                    array.push(val.clone());
                }
                val = Value::new_array(array);
            }
        }
    }
    pl::ret(val)
}

/// Process binary PLIST data (ref 2)
/// Inputs: 0) ExifTool object ref, 1) DirInfo ref, 2) tag table ref
/// Returns: 1 on success (and returns plist value as $$dirInfo{Value})
pub fn process_binary_plist(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let mut buff = Value::Undef;
    let mut table: Vec<Value> = Vec::new();
    let data_pt = dir_info.get("DataPt");

    if !dir_info.get("NoVerboseDir").truthy() {
        pl::method(it, &et, "VerboseDir", vec![pl::s("Binary PLIST")])?;
    }
    call(it, "SetByteOrder", vec![pl::s("MM")])?;

    if data_pt.truthy() {
        let start = dir_info.get("DirStart");
        let dir_len = dir_info.get("DirLen");
        if start.truthy()
            || (dir_len.truthy() && !pl::num_eq(&dir_len, &pl::length(&data_pt.deref())))
        {
            let off = if start.truthy() { start.to_i64() } else { 0 };
            let buf2 = pl::substr(&data_pt.deref(), off, Some(dir_len.to_i64()));
            let r = pl::call(
                it,
                "File::RandomAccess::new",
                vec![pl::s("File::RandomAccess"), pl::sref(buf2)],
            )?;
            dir_info.set("RAF", r);
        } else {
            let r = pl::call(
                it,
                "File::RandomAccess::new",
                vec![pl::s("File::RandomAccess"), data_pt.clone()],
            )?;
            dir_info.set("RAF", r);
        }
    }
    // read and parse the trailer
    let raf = dir_info.get("RAF");
    if !raf.truthy() {
        return pl::ret(n(0));
    }
    if !(seek(it, &raf, n(-32), 2)? && read_eq(it, &raf, &mut buff, &n(32))?) {
        return pl::ret(n(0));
    }
    let bref = pl::sref(buff.clone());
    let int_size = call(it, "Get8u", vec![bref.clone(), n(6)])?;
    let ref_size = call(it, "Get8u", vec![bref.clone(), n(7)])?;
    let num_obj = call(it, "Get64u", vec![bref.clone(), n(8)])?;
    let top_obj = call(it, "Get64u", vec![bref.clone(), n(16)])?;
    let table_off = call(it, "Get64u", vec![bref, n(24)])?;

    if pl::ge(&top_obj, &num_obj) {
        return pl::ret(n(0));
    }
    let int_proc = read_proc(it, &int_size);
    if !int_proc.truthy() {
        return pl::ret(n(0));
    }
    let ref_proc = read_proc(it, &ref_size);
    if !ref_proc.truthy() {
        return pl::ret(n(0));
    }

    // read and parse the offset table
    let table_size = pl::mul(&int_size, &num_obj);
    if !(seek(it, &raf, table_off, 0)? && read_eq(it, &raf, &mut buff, &table_size)?) {
        return pl::ret(n(0));
    }
    let mut i = n(0);
    while pl::lt(&i, &num_obj) {
        let v = call_proc(it, &int_proc, &buff, pl::mul(&i, &int_size))?;
        table.push(v);
        i = pl::add(&i, &n(1));
    }
    let table = Value::new_array(table);
    let plist_info = Value::new_hash();
    plist_info.set("RAF", raf.clone());
    plist_info.set("RefSize", ref_size);
    plist_info.set("RefProc", ref_proc);
    plist_info.set("Table", table.clone());
    plist_info.set("TagTablePtr", tag_table_ptr);
    // position file pointer at the top object, and extract it
    if !seek(it, &raf, table.at(top_obj.to_i64()), 0)? {
        return pl::ret(n(0));
    }
    let v = extract_object(it, vec![et, plist_info], Want::Scalar)?
        .pop()
        .unwrap_or_default();
    dir_info.set("Value", v);
    pl::ret(n(if dir_info.get("Value").is_undef() {
        0
    } else {
        1
    }))
}

/// `pos($$dataPt) = $start; $$dataPt =~ /\GPAT/` (with /g when `g`): the
/// match, leaving pos() on the scalar as perl does.
fn match_at_start(it: &mut Interp, data_pt: &Value, start: &Value, pat: &str, g: bool) -> ER<bool> {
    let d = cell(data_pt.clone());
    let s = cell(start.clone());
    let code = format!(
        "pos($$d) = $s; ($$d =~ /\\G{pat}/{}) ? 1 : 0",
        if g { "g" } else { "" }
    );
    Ok(perl_eval1(it, &code, MODULE, &[("d", &d), ("s", &s)])?.truthy())
}

/// Extract information from a PLIST file (binary, XML or JSON format)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success, 0 if this wasn't valid PLIST
pub fn process_plist(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let mut data_pt = dir_info.get("DataPt");
    let start = {
        let v = dir_info.get("DirStart");
        if v.truthy() {
            v
        } else {
            n(0)
        }
    };
    let mut result = Value::Undef;
    let mut not_xml = false;

    if data_pt.truthy() && !match_at_start(it, &data_pt, &start, "<", true)? {
        not_xml = true;
    }
    if !not_xml {
        // process XML PLIST data using the XMP module
        let found = Value::Code(it.sub_ref("Image::ExifTool::PLIST::FoundTag"));
        dir_info
            .get_or_new_hash("XMPParseOpts")
            .set("FoundProc", found);
        result = pl::call(
            it,
            "Image::ExifTool::XMP::ProcessXMP",
            vec![et.clone(), dir_info.clone(), tag_table_ptr.clone()],
        )?;
        dir_info.delete("XMPParseOpts");
        if result.truthy() {
            return pl::ret(result);
        }
    }
    let raf = dir_info.get("RAF");
    if raf.truthy() {
        let mut buff = Value::Undef;
        if !(seek(it, &raf, n(0), 0)? && read(it, &raf, &mut buff, n(64))?.truthy()) {
            return pl::ret(n(0));
        }
        data_pt = pl::sref(buff);
    } else if !data_pt.truthy() {
        return pl::ret(n(0));
    }
    if match_at_start(it, &data_pt, &start, "bplist0", false)? {
        // binary PLIST
        // binary PLIST file
        let tag_table_ptr = call(
            it,
            "GetTagTable",
            vec![pl::s("Image::ExifTool::PLIST::Main")],
        )?;
        pl::method(
            it,
            &et,
            "SetFileType",
            vec![pl::s("PLIST"), pl::s("application/x-plist")],
        )?;
        et.set("SET_GROUP1", pl::s("PLIST"));
        if !process_binary_plist(
            it,
            vec![et.clone(), dir_info.clone(), tag_table_ptr],
            Want::Scalar,
        )?
        .pop()
        .unwrap_or_default()
        .truthy()
        {
            pl::method(
                it,
                &et,
                "Error",
                vec![pl::s("Error reading binary PLIST file")],
            )?;
        }
        et.delete("SET_GROUP1");
        result = n(1);
    } else if re(r#"^\{""#, "").is_match(&data_pt.deref()) {
        // JSON PLIST
        if raf.truthy() {
            pl::method(it, &raf, "Seek", vec![n(0)])?;
        }
        crate::runtime::require(it, "Image::ExifTool::JSON")?;
        result = pl::call(
            it,
            "Image::ExifTool::JSON::ProcessJSON",
            vec![et.clone(), dir_info.clone()],
        )?;
    } else if et.get("FILE_EXT").truthy()
        && pl::str_eq(&et.get("FILE_EXT"), "PLIST")
        && re(r"^\xfe\xff\x00", "").is_match(&data_pt.deref())
    {
        // (have seen very old PLIST files encoded as UCS-2BE with leading BOM)
        pl::method(
            it,
            &et,
            "Error",
            vec![pl::s("Old PLIST format currently not supported")],
        )?;
        result = n(1);
    }
    pl::ret(result)
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::PLIST::FoundTag", found_tag);
    it.define_native("Image::ExifTool::PLIST::Get24u", get24u);
    it.define_native("Image::ExifTool::PLIST::ExtractObject", extract_object);
    it.define_native(
        "Image::ExifTool::PLIST::ProcessBinaryPLIST",
        process_binary_plist,
    );
    it.define_native("Image::ExifTool::PLIST::ProcessPLIST", process_plist);
}
