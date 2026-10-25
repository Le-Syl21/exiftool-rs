//! Image::ExifTool::XMPStruct (Image/ExifTool/XMPStruct.pl): XMP structure
//! support. The file is loaded by path and its subs are in package
//! Image::ExifTool::XMP; ported sub by sub (see docs/port/PORTING.md).

use std::rc::Rc;

use crate::interp::eval::{Lv, ER};
use crate::interp::{Interp, Want};
use crate::modules::xmp::{
    apop, cat, elem_ref, escape_xml_v, hash_of, hh, is_hash, lc, m, n, or, pkg_hash, register_namespace_v, require, rx, rx_dyn, same_str, set_deref, sref, subst, substr, tr, uc, ucfirst,
};
use crate::perl::Value;
use crate::pl::{self, arg, cap, ret, Caps};

/// A file lexical of XMPStruct.pl (`my %jsonChar`).
fn lex(it: &mut Interp, name: &str) -> Value {
    crate::runtime::lex_hash(it, "Image::ExifTool::XMPStruct", name)
}

/// `Image::ExifTool::OrderedKeys($h)`
fn ordered_keys(it: &mut Interp, h: &Value) -> ER<Vec<Value>> {
    pl::call_list(it, "Image::ExifTool::OrderedKeys", vec![h.clone()])
}

/// `$$obj =~ s/RE//` on the string a SCALAR reference holds: the captures
/// when it matched (the string is updated).
fn s_obj(obj: &Value, re: &crate::perl::regex::PerlRegex) -> ER<Option<Caps>> {
    let mut v = obj.deref();
    let mut caps = None;
    if subst(re, &mut v, false, |c| {
        caps = Some(c.clone());
        Ok(Vec::new())
    })? > 0
    {
        set_deref(obj, v)?;
    }
    Ok(caps)
}

/// `print LIST` to the selected handle.
fn print(it: &mut Interp, s: Value) -> ER<()> {
    let scope = crate::interp::Scope::new(None);
    scope.scalar("s", s);
    let saved_err = it.get_scalar("main::@");
    let r = it.eval_string(b"print $s", "Image::ExifTool::XMP", &scope, Want::Void);
    it.set_scalar("main::@", saved_err);
    match r {
        Ok(_) => Ok(()),
        Err(e) => Err(crate::interp::Flow::Die(e)),
    }
}

/// Numeric `<=>` for sort.
fn num_cmp(a: &Value, b: &Value) -> std::cmp::Ordering {
    crate::perl::ops::num_cmp(a, b).unwrap_or(std::cmp::Ordering::Equal)
}

/// `$$h{a}{b}` on the hash a value refers to, for the stdXlatNS lookup in
/// `s/^(\w+)/$stdXlatNS{$1} || $1/e`.
fn xlat_first(it: &mut Interp, p: &mut Value) -> ER<()> {
    let std_xlat_ns = pkg_hash(it, "Image::ExifTool::XMP::stdXlatNS");
    subst(&rx(r"^(\w+)", ""), p, false, |c| Ok(or(std_xlat_ns.get(cap(c, 1).to_bytes()), cap(c, 1)).to_bytes().to_vec()))?;
    Ok(())
}

/// Serialize a structure (or other object) into a simple string
/// Inputs: 0) ExifTool ref, 1) HASH ref, ARRAY ref, or SCALAR, 2) closing bracket (or undef)
/// Returns: serialized structure string (in format specified by StructFormat option)
/// eg) "{field=text with {braces|}|, and a comma, field2=val2,field3={field4=[a,b]}}"
pub fn serialize_struct(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(serialize_struct_v(it, &arg(&a, 0), arg(&a, 1), &arg(&a, 2))?)
}

pub(crate) fn serialize_struct_v(it: &mut Interp, et: &Value, mut obj: Value, ket: &Value) -> ER<Value> {
    let mut vals: Vec<Value> = Vec::new();
    let rtn_val;
    let sfmt = pl::method(it, et, "Options", vec![pl::s("StructFormat")])?;

    if obj.ref_type() == "HASH" {
        // support hashes with ordered keys
        for key in ordered_keys(it, &obj)? {
            let hdr = if sfmt.truthy() { pl::concat(&escape_json_v(it, &key, &Value::Undef)?, &pl::s(":")) } else { pl::concat(&key, &pl::s("=")) };
            let v = serialize_struct_v(it, et, obj.get(key.to_bytes()), &pl::s("}"))?;
            vals.push(pl::concat(&hdr, &v));
        }
        rtn_val = cat!("{", crate::modules::xmp::join(b",", &vals), "}");
    } else if obj.ref_type() == "ARRAY" {
        for val in obj.items() {
            vals.push(serialize_struct_v(it, et, val, &pl::s("]"))?);
        }
        rtn_val = cat!("[", crate::modules::xmp::join(b",", &vals), "]");
    } else if !obj.is_undef() {
        if obj.ref_type() == "SCALAR" {
            obj = obj.deref();
        }
        // escape necessary characters in string (closing bracket plus "," and "|")
        if sfmt.truthy() {
            rtn_val = escape_json_v(it, &obj, &Value::from(pl::str_eq(&sfmt, "JSONQ")))?;
        } else {
            let pat = if ket.truthy() { cat!("\\", ket, r"|,|\|") } else { pl::s(r",|\|") };
            let mut r = obj.clone();
            let mut p = b"(".to_vec();
            p.extend_from_slice(&pat.to_bytes());
            p.push(b')');
            subst(&*rx_dyn(&p, "")?, &mut r, true, |c| Ok(cat!("|", cap(c, 1)).to_bytes().to_vec()))?;
            // also must escape opening bracket or whitespace at start of string
            subst(&rx(r"^([\s\[\{])", ""), &mut r, false, |c| Ok(cat!("|", cap(c, 1)).to_bytes().to_vec()))?;
            rtn_val = r;
        }
    } else if sfmt.truthy() {
        rtn_val = pl::s("null");
    } else {
        rtn_val = pl::s(""); // allow undefined list items
    }
    Ok(rtn_val)
}

/// Inflate structure (or other object) from a serialized string
/// Inputs: 0) ExifTool ref, 1) reference to object in string form
///           (serialized using the '|' escape, or JSON)
///         2) extra delimiter for scalar values delimiters
/// Returns: 0) object as a SCALAR, HASH ref, or ARRAY ref (or undef on error),
///          1) warning string (or undef)
/// Notes: modifies input string to remove parsed objects
pub fn inflate_struct(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (v, w) = inflate_struct_v(it, &arg(&a, 0), &arg(&a, 1), arg(&a, 2))?;
    if want == Want::List {
        Ok(vec![v, w])
    } else {
        ret(w)
    }
}

pub(crate) fn inflate_struct_v(it: &mut Interp, et: &Value, obj: &Value, mut delim: Value) -> ER<(Value, Value)> {
    let (mut val, mut warn);
    warn = Value::Undef;
    let sfmt = pl::method(it, et, "Options", vec![pl::s("StructFormat")])?;

    if s_obj(obj, &rx(r"^\s*\{", ""))?.is_some() {
        let struct_ = Value::new_hash();
        loop {
            let c = if sfmt.truthy() { s_obj(obj, &rx(r#"^\s*"(.*?)"\s*:"#, "s"))? } else { s_obj(obj, &rx(r"^\s*([-\w:.]+#?)\s*=", "s"))? };
            let Some(c) = c else { break };
            let tag = cap(&c, 1);
            let (v, w) = inflate_struct_v(it, et, obj, pl::s("}"))?;
            if w.truthy() && !warn.truthy() {
                warn = w;
            }
            if v.is_undef() {
                return Ok((Value::Undef, warn));
            }
            struct_.set(tag.to_bytes(), v);
            // eat comma separator, or all done if there wasn't one
            if s_obj(obj, &rx(r"^\s*,", "s"))?.is_none() {
                break;
            }
        }
        // eat closing brace and warn if we didn't find one
        if !(s_obj(obj, &rx(r"^\s*\}", "s"))?.is_some() || warn.truthy()) {
            if pl::length(&obj.deref()).truthy() {
                let mut part = obj.deref();
                subst(&rx(r"^\s*", "s"), &mut part, false, |_| Ok(Vec::new()))?;
                subst(&rx(r"[\x0d\x0a].*", "s"), &mut part, false, |_| Ok(Vec::new()))?;
                if part.to_bytes().len() > 30 {
                    part = pl::concat(&substr(&part.to_bytes(), 0, Some(27)), &pl::s("..."));
                }
                warn = cat!("Invalid structure field at '", part, "'");
            } else {
                warn = pl::s("Missing closing brace for structure");
            }
        }
        val = struct_;
    } else if s_obj(obj, &rx(r"^\s*\[", ""))?.is_some() {
        let list = Value::new_array(Vec::new());
        loop {
            let (v, w) = inflate_struct_v(it, et, obj, pl::s("]"))?;
            if w.truthy() && !warn.truthy() {
                warn = w;
            }
            if v.is_undef() {
                return Ok((Value::Undef, warn));
            }
            list.push(v);
            if s_obj(obj, &rx(r"^\s*,", "s"))?.is_none() {
                break;
            }
        }
        // eat closing bracket and warn if we didn't find one
        if s_obj(obj, &rx(r"^\s*\]", "s"))?.is_none() && !warn.truthy() {
            warn = pl::s("Missing closing bracket for list");
        }
        val = list;
    } else {
        s_obj(obj, &rx(r"^\s+", "s"))?; // remove leading whitespace
        if sfmt.truthy() {
            if s_obj(obj, &rx(r#"^""#, ""))?.is_some() {
                val = pl::s("");
                while let Some(c) = s_obj(obj, &rx(r#"(.*?)""#, ""))? {
                    val = pl::concat(&val, &cap(&c, 1));
                    let odd = m(&rx(r"([\\]+)$", ""), &val).is_some_and(|c| cap(&c, 1).to_bytes().len() & 0x01 != 0);
                    if !odd {
                        break;
                    }
                    let vb = val.to_bytes();
                    let mut nv = vb[..vb.len() - 1].to_vec();
                    nv.push(b'"'); // (was an escaped quote)
                    val = Value::bytes(nv);
                }
                if subst(&rx(r"^base64:", ""), &mut val, false, |_| Ok(Vec::new()))? > 0 {
                    val = crate::modules::xmp::decode_base64_v(val)?;
                } else {
                    // un-escape characters in JSON string
                    let json_esc = lex(it, "jsonEsc");
                    subst(&rx(r"\\(.)", "s"), &mut val, true, |c| Ok(or(json_esc.get(cap(c, 1).to_bytes()), cat!("\\", cap(c, 1))).to_bytes().to_vec()))?;
                }
            } else if let Some(c) = s_obj(obj, &rx(r"^(true|false)\b", ""))? {
                val = cat!("\"", ucfirst(&cap(&c, 1)), "\"");
            } else if let Some(c) = s_obj(obj, &rx(r"^([+-]?(?=\d|\.\d)\d*(\.\d*)?([Ee]([+-]?\d+))?)", ""))? {
                val = cap(&c, 1);
            } else {
                if !warn.truthy() {
                    warn = pl::s("Unknown JSON object");
                }
                val = pl::s("\"\"");
            }
        } else {
            // read scalar up to specified delimiter (or "," if not defined)
            delim = if delim.truthy() { cat!("\\", delim, r"|,|\||$") } else { pl::s(r",|\||$") };
            val = pl::s("");
            let mut p = b"^(.*?)(".to_vec();
            p.extend_from_slice(&delim.to_bytes());
            p.push(b')');
            let re = rx_dyn(&p, "s")?;
            loop {
                let Some(c) = s_obj(obj, &re)? else { break };
                val = pl::concat(&val, &cap(&c, 1));
                let c2 = cap(&c, 2);
                if !c2.truthy() {
                    break;
                }
                if !pl::str_eq(&c2, "|") {
                    set_deref(obj, pl::concat(&c2, &obj.deref()))?;
                    break;
                }
                if let Some(c) = s_obj(obj, &rx(r"^(.)", "s"))? {
                    val = pl::concat(&val, &cap(&c, 1)); // add escaped character
                }
            }
        }
    }
    Ok((val, warn))
}

/// Escape string for JSON
/// Inputs: 0) string, 1) flag to force numbers to be quoted too
/// Returns: Escaped string (quoted if necessary)
pub fn escape_json(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(escape_json_v(it, &arg(&a, 0), &arg(&a, 1))?)
}

pub(crate) fn escape_json_v(it: &mut Interp, str: &Value, quote: &Value) -> ER<Value> {
    let mut str = str.clone();
    if !quote.truthy() {
        if str.is_undef() {
            return Ok(pl::s("null"));
        }
        // JSON boolean (true or false)
        if m(&rx(r"^(true|false)$", "i"), &str).is_some() {
            return Ok(lc(&str));
        }
        // JSON number (see json.org for numerical format)
        // return $str if $str =~ /^-?(\d|[1-9]\d+)(\.\d+)?(e[-+]?\d+)?$/i;
        // (these big numbers caused problems for some JSON parsers, so be more conservative)
        if m(&rx(r"^-?(\d|[1-9]\d{1,14})(\.\d{1,16})?(e[-+]?\d{1,3})?$", "i"), &str).is_some() {
            return Ok(str);
        }
    }
    if str.is_undef() {
        return Ok(pl::s("\"\""));
    }
    // encode JSON string in base64 if necessary
    let r = sref(str.clone());
    let u = pl::call(it, "Image::ExifTool::IsUTF8", vec![r])?;
    if pl::lt(&u, &n(0)) {
        let b = pl::call(it, "Image::ExifTool::XMP::EncodeBase64", vec![str, n(1)])?;
        return Ok(cat!("\"base64:", b, "\""));
    }
    // escape special characters
    let json_char = lex(it, "jsonChar");
    subst(&rx(r#"(["\t\n\r\\])"#, "s"), &mut str, true, |c| Ok(cat!("\\", json_char.get(cap(c, 1).to_bytes())).to_bytes().to_vec()))?;
    tr(&mut str, br"\0", b"", "d"); // remove all nulls
    // escape other control characters with \u
    subst(&rx(r"([\0-\x1f])", "s"), &mut str, true, |c| {
        let ord = cap(c, 1).to_bytes().first().copied().unwrap_or(0);
        Ok(pl::sprintf("\\u%.4X", &[n(ord as i64)]).to_bytes().to_vec())
    })?;
    Ok(cat!("\"", str, "\"")) // return the quoted string
}

/// Get XMP language code from tag name string
/// Inputs: 0) tag name string
/// Returns: 0) separated tag name, 1) language code (in standard case), or '' if
///          language code was 'x-default', or undef if the tag had no language code
pub fn get_lang_code(_it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (t, l) = get_lang_code_v(&arg(&a, 0));
    if want == Want::List {
        Ok(vec![t, l])
    } else {
        ret(l)
    }
}

pub(crate) fn get_lang_code_v(tag: &Value) -> (Value, Value) {
    if let Some(c) = m(&rx(r"^(\w+)[-_]([a-z]{2,3}|[xi])([-_][a-z\d]{2,8}([-_][a-z\d]{1,8})*)?$", "i"), tag) {
        // normalize case of language codes
        let (tg, mut lang_code) = (cap(&c, 1), lc(&cap(&c, 2)));
        let c3 = cap(&c, 3);
        if c3.truthy() {
            lang_code = pl::concat(&lang_code, &if c3.to_bytes().len() == 3 { uc(&c3) } else { lc(&c3) });
        }
        tr(&mut lang_code, b"_", b"-", ""); // RFC 3066 specifies '-' as a separator
        if pl::str_eq(&lc(&lang_code), "x-default") {
            lang_code = pl::s("");
        }
        (tg, lang_code)
    } else {
        (tag.clone(), Value::Undef)
    }
}

/// Debugging routine to dump a structure, list or scalar
/// Inputs: 0) scalar, ARRAY ref or HASH ref, 1) indent (or undef)
pub fn dump_struct(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    crate::modules::xmp::local_topic(it, |it| dump_struct_v(it, &arg(&a, 0), arg(&a, 1)))?;
    pl::ret_nothing(want)
}

fn dump_struct_v(it: &mut Interp, obj: &Value, mut indent: Value) -> ER<()> {
    if !indent.truthy() {
        indent = pl::s("");
    }
    if obj.ref_type() == "HASH" {
        print(it, pl::s("{\n"))?;
        for k in ordered_keys(it, obj)? {
            print(it, cat!(indent, "  ", k, " = "))?;
            dump_struct_v(it, &obj.get(k.to_bytes()), cat!(indent, "  "))?;
        }
        print(it, cat!(indent, "},\n"))?;
    } else if obj.ref_type() == "ARRAY" {
        print(it, pl::s("[\n"))?;
        for x in obj.items() {
            print(it, cat!(indent, "  "))?;
            dump_struct_v(it, &x, cat!(indent, "  "))?;
        }
        print(it, cat!(indent, "],\n"))?;
    } else {
        print(it, cat!("\"", obj, "\",\n"))?;
    }
    Ok(())
}

/// `my ($fix) = reverse sort grep /^$tag$/i, keys %$strTable;`
fn fix_case(str_table: &Value, tag: &Value) -> ER<Value> {
    let mut p = b"^".to_vec();
    p.extend_from_slice(&tag.to_bytes());
    p.push(b'$');
    let re = rx_dyn(&p, "i")?;
    let mut keys: Vec<Rc<[u8]>> = str_table.keys().into_iter().filter(|k| crate::modules::xmp::m_at(&re, k, 0).is_some()).collect();
    keys.sort();
    Ok(keys.pop().map(Value::from).unwrap_or_default())
}

/// Recursively validate structure fields (tags)
/// Inputs: 0) ExifTool ref, 1) Structure ref, 2) structure table definition ref
/// Returns: 0) validated structure ref, 1) error string, or undef on success
/// Notes:
/// - fixes field names in structure and applies inverse conversions to values
/// - copies structure to avoid interdependencies with calling code on referenced values
/// - handles lang-alt tags, and '#' on field names
/// - resets UTF-8 flag of SCALAR values
/// - un-escapes for XML or HTML as per Escape option setting
pub fn check_struct(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (r, e) = check_struct_v(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2))?;
    if want == Want::List {
        Ok(vec![r, e])
    } else {
        ret(r)
    }
}

pub(crate) fn check_struct_v(it: &mut Interp, et: &Value, struct_: &Value, str_table: &Value) -> ER<(Value, Value)> {
    let str_name = {
        let s = str_table.get("STRUCT_NAME");
        if s.truthy() {
            s
        } else {
            pl::concat(&pl::s("XMP "), &register_namespace_v(it, str_table)?)
        }
    };
    if struct_.ref_type() != "HASH" {
        return Ok((Value::Undef, cat!("Expecting ", str_name, " structure")));
    }

    let special_struct = pkg_hash(it, "Image::ExifTool::XMP::specialStruct");
    let (mut err, mut warn, mut rtn_val);
    (warn, rtn_val) = (Value::Undef, Value::Undef);
    let copy = Value::new_hash();
    // copy the ordered keys if they exist
    if struct_.get("_ordered_keys_").truthy() {
        copy.set("_ordered_keys_", Value::new_array(Vec::new()));
    }
    'key: for key in ordered_keys(it, struct_)? {
        let mut tag = key.clone();
        // allow trailing '#' to disable print conversion on a per-field basis
        let (mut type_, mut field_info) = (Value::Undef, Value::Undef);
        if subst(&rx("#$", ""), &mut tag, false, |_| Ok(Vec::new()))? > 0 {
            type_ = pl::s("ValueConv");
        }
        if !special_struct.get(tag.to_bytes()).truthy() {
            field_info = str_table.get(tag.to_bytes());
        }
        // fix case of field name if necessary
        if !field_info.truthy() {
            // (sort in reverse to get lower case (not special) tags first)
            let fix = fix_case(str_table, &tag)?;
            if fix.truthy() && !special_struct.get(fix.to_bytes()).truthy() {
                tag = fix;
                field_info = str_table.get(tag.to_bytes());
            }
        }
        if !is_hash(&field_info) {
            'until: {
                // generate wildcard fields on the fly (eg. mwg-rs:Extensions)
                if !str_table.get("NAMESPACE").truthy() {
                    let (mut grp, mut tg, mut lang_code);
                    lang_code = Value::Undef;
                    match m(&rx(r"^(.+):(.+)", ""), &tag) {
                        Some(c) => (grp, tg) = (lc(&cap(&c, 1)), cap(&c, 2)),
                        None => (grp, tg) = (pl::s(""), tag.clone()),
                    }
                    if pl::str_eq(&grp, "XMP") {
                        grp = Value::Undef; // (a group of 'XMP' is implied)
                    }
                    require(it, "Image::ExifTool::TagLookup")?;
                    let mut matches = pl::call_list(it, "Image::ExifTool::TagLookup::FindTagInfo", vec![tg.clone()])?;
                    // also look for lang-alt tags
                    if matches.is_empty() {
                        (tg, lang_code) = get_lang_code_v(&tg);
                        if !lang_code.is_undef() {
                            matches = pl::call_list(it, "Image::ExifTool::TagLookup::FindTagInfo", vec![tg.clone()])?;
                        }
                    }
                    let (mut tag_info, mut priority, mut g1) = (Value::Undef, Value::Undef, Value::Undef);
                    // find best matching tag
                    for ti in matches {
                        let grps = pl::method_list(it, et, "GetGroup", vec![ti.clone()])?;
                        if !pl::str_eq(&pl::arg(&grps, 0), "XMP") {
                            continue;
                        }
                        if grp.truthy() && !same_str(&grp, &lc(&pl::arg(&grps, 1))) {
                            continue;
                        }
                        // must be lang-alt tag if we are writing an alternate language
                        if !lang_code.is_undef() && !(ti.get("Writable").truthy() && pl::str_eq(&ti.get("Writable"), "lang-alt")) {
                            continue;
                        }
                        let mut pri = or(ti.get("Priority"), n(1));
                        if ti.get("Avoid").truthy() {
                            pri = pl::sub(&pri, &n(10));
                        }
                        if !priority.is_undef() && pl::ge(&priority, &pri) {
                            continue;
                        }
                        priority = pri;
                        tag_info = ti;
                        g1 = pl::arg(&grps, 1);
                    }
                    if !tag_info.truthy() {
                        warn = cat!("'", tag, "' is not a writable XMP tag");
                        continue 'key;
                    }
                    it.call_named("Image::ExifTool::XMP::GetPropertyPath", vec![tag_info.clone()], Want::Void)?; // make sure property path is generated for this tag
                    tag = tag_info.get("Name");
                    if grp.truthy() {
                        tag = cat!(g1, ":", tag);
                    }
                    if lang_code.truthy() {
                        tag = cat!(tag, "-", lang_code);
                    }
                    field_info = str_table.get(tag.to_bytes());
                    // create new structure field if necessary
                    if !field_info.truthy() {
                        field_info = tag_info.hash_copy(); // (also copies the necessary TagID and PropertyPath)
                        field_info.set("Namespace", or(tag_info.get("Namespace"), hh(&tag_info, "Table").get("NAMESPACE")));
                        field_info.set("LangCode", lang_code.clone());
                        str_table.set(tag.to_bytes(), field_info.clone());
                    }
                    // delete stuff we don't need (shouldn't cause harm, but better safe than sorry)
                    // - need to keep StructType and Table in case we need to call AddStructType later
                    field_info.delete("Description");
                    field_info.delete("Groups");
                    break 'until; // write this dynamically-generated field
                }
                // generate lang-alt fields on the fly (eg. Iptc4xmpExt:AOTitle)
                let (mut tg, lang_code) = get_lang_code_v(&tag);
                if !lang_code.is_undef() {
                    if !special_struct.get(tg.to_bytes()).truthy() {
                        field_info = str_table.get(tg.to_bytes());
                    }
                    if !field_info.truthy() {
                        let fix = fix_case(str_table, &tg)?;
                        if fix.truthy() && !special_struct.get(fix.to_bytes()).truthy() {
                            tg = fix;
                            field_info = str_table.get(tg.to_bytes());
                        }
                    }
                    if is_hash(&field_info) && field_info.get("Writable").truthy() && pl::str_eq(&field_info.get("Writable"), "lang-alt") {
                        let src_info = field_info.clone();
                        if lang_code.truthy() {
                            tag = cat!(tg, "-", lang_code);
                        }
                        field_info = str_table.get(tag.to_bytes());
                        // create new structure field if necessary
                        if !field_info.truthy() {
                            field_info = src_info.hash_copy();
                            field_info.set("TagID", tg.clone());
                            field_info.set("LangCode", lang_code.clone());
                            str_table.set(tag.to_bytes(), field_info.clone());
                        }
                        break 'until; // write this lang-alt field
                    }
                }
                warn = cat!("'", tag, "' is not a field of ", str_name);
                continue 'key;
            }
        }
        let sv = struct_.get(key.to_bytes());
        if sv.ref_type() == "HASH" {
            if !field_info.get("Struct").truthy() {
                warn = cat!(tag, " is not a structure in ", str_name);
                continue 'key;
            }
            // recursively check this structure
            let (val, e) = check_struct_v(it, et, &sv, &field_info.get("Struct"))?;
            err = e;
            if err.truthy() {
                warn = err.clone();
                continue 'key;
            }
            copy.set(tag.to_bytes(), val);
        } else if sv.ref_type() == "ARRAY" {
            if !field_info.get("List").truthy() {
                warn = cat!(tag, " is not a list in ", str_name);
                continue 'key;
            }
            // check all items in the list
            let copy_l = Value::new_array(Vec::new());
            let copy_rc = match &copy_l {
                Value::ArrayRef(r) => r.clone(),
                _ => unreachable!(),
            };
            let sv_rc = match &sv {
                Value::ArrayRef(r) => r.clone(),
                _ => unreachable!(),
            };
            let mut i: usize = 0;
            let count = sv.len();
            for idx in 0..count {
                let mut item = sv.at(idx as i64);
                if !item.is_ref() {
                    if item.is_undef() {
                        item = pl::s(""); // use empty string for missing items
                        sv.set_at(idx, item.clone());
                    }
                    if field_info.get("Struct").truthy() {
                        // (allow empty structures)
                        if m(&rx(r"^\s*$", ""), &item).is_none() {
                            warn = cat!(tag, " items are not valid structures");
                            continue 'key;
                        }
                        copy_l.set_at(i, Value::new_hash()); // create hash for empty structure
                    } else {
                        let item_ref = Value::LvRef(Rc::new(Lv::AElem(sv_rc.clone(), idx as i64)));
                        pl::method(it, et, "Sanitize", vec![item_ref])?;
                        item = sv.at(idx as i64);
                        let r = pl::method_list(it, et, "ConvInv", vec![item, field_info.clone(), tag.clone(), str_name.clone(), type_.clone(), pl::s("")])?;
                        copy_l.set_at(i, pl::arg(&r, 0));
                        err = pl::arg(&r, 1);
                        if copy_l.at(i as i64).is_undef() {
                            copy_l.set_at(i, pl::s("")); // avoid undefined item
                        }
                        if err.truthy() {
                            warn = err.clone();
                            continue 'key;
                        }
                        let elem = Value::LvRef(Rc::new(Lv::AElem(copy_rc.clone(), i as i64)));
                        err = pl::call(it, "Image::ExifTool::XMP::CheckXMP", vec![et.clone(), field_info.clone(), elem])?;
                        if err.truthy() {
                            warn = cat!(err, " in ", str_name, " ", tag);
                            continue 'key;
                        }
                    }
                } else if item.ref_type() == "HASH" {
                    if !field_info.get("Struct").truthy() {
                        warn = cat!(tag, " is not a structure in ", str_name);
                        continue 'key;
                    }
                    let (v, e) = check_struct_v(it, et, &item, &field_info.get("Struct"))?;
                    copy_l.set_at(i, v);
                    err = e;
                    if err.truthy() {
                        warn = err.clone();
                        continue 'key;
                    }
                } else {
                    warn = cat!("Invalid value for ", tag, " in ", str_name);
                    continue 'key;
                }
                i += 1;
            }
            copy.set(tag.to_bytes(), copy_l);
        } else if field_info.get("Struct").truthy() {
            warn = cat!("Improperly formed structure in ", str_name, " ", tag);
            continue;
        } else {
            pl::method(it, et, "Sanitize", vec![elem_ref(struct_, &key.to_bytes())])?;
            let r = pl::method_list(it, et, "ConvInv", vec![struct_.get(key.to_bytes()), field_info.clone(), tag.clone(), str_name.clone(), type_.clone(), pl::s("")])?;
            let val = pl::arg(&r, 0);
            err = pl::arg(&r, 1);
            if err.truthy() {
                warn = err.clone();
                continue 'key;
            }
            if val.is_undef() {
                continue 'key; // check for undefined
            }
            let vr = sref(val);
            err = pl::call(it, "Image::ExifTool::XMP::CheckXMP", vec![et.clone(), field_info.clone(), vr.clone()])?;
            let val = vr.deref();
            if err.truthy() {
                warn = cat!(err, " in ", str_name, " ", tag);
                continue 'key;
            }
            // turn this into a list if necessary
            copy.set(tag.to_bytes(), if field_info.get("List").truthy() { Value::new_array(vec![val]) } else { val });
        }
        if copy.get("_ordered_keys_").truthy() {
            copy.get("_ordered_keys_").push(tag.clone()); // save ordered keys
        }
    }
    if !copy.is_empty() || !warn.truthy() {
        rtn_val = copy;
        err = Value::Undef;
        if warn.truthy() {
            et.set("CHECK_WARN", warn);
        }
    } else {
        err = warn;
    }
    Ok((rtn_val, err))
}

/// Delete matching structures from existing linearized XMP
/// Inputs: 0) ExifTool ref, 1) capture hash ref, 2) structure path ref,
///         3) new value hash ref, 4) reference to change counter
/// Returns: 0) delete flag, 1) list index of deleted structure if adding to list
///          2) flag set if structure existed
/// Notes: updates path to new base path for structure to be added
pub fn delete_struct(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (d, ad, ex) = delete_struct_v(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2), &arg(&a, 3), &arg(&a, 4))?;
    if want == Want::List {
        Ok(vec![d, ad, ex])
    } else {
        ret(ex)
    }
}

fn sorted_keys(h: &Value) -> Vec<Value> {
    let mut k = h.keys();
    k.sort();
    k.into_iter().map(Value::from).collect()
}

fn sort_values(v: &mut [Value]) {
    v.sort_by(|a, b| a.to_bytes().cmp(&b.to_bytes()));
}

fn delete_struct_v(it: &mut Interp, et: &Value, capture: &Value, path_pt: &Value, nv_hash: &Value, changed: &Value) -> ER<(Value, Value, Value)> {
    let (mut deleted, mut added, mut existed, mut del_path) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef);
    let (mut struct_paths, mut matching_paths, mut del_paths): (Vec<Value>, Vec<Value>, Vec<Value>) = (Vec::new(), Vec::new(), Vec::new());

    // find all existing elements belonging to this structure
    let mut pp = path_pt.deref();
    subst(&rx(r" \d+", ""), &mut pp, true, |_| Ok(br" \d+".to_vec()))?;
    {
        let re = rx_dyn(&cat!("^", pp, r"(\/|$)").to_bytes(), "")?;
        for k in sorted_keys(capture) {
            if m(&re, &k).is_some() {
                struct_paths.push(k);
            }
        }
    }
    if !struct_paths.is_empty() {
        existed = n(1);
    }
    let pp_re = rx_dyn(&cat!("^(", pp, ")").to_bytes(), "")?;
    // delete only structures with matching fields if necessary
    if nv_hash.get("DelValue").truthy() {
        if !nv_hash.get("DelValue").is_empty() {
            let str_table = hh(nv_hash, "TagInfo").get("Struct");
            // all fields must match corresponding elements in the same
            // root structure for it to be deleted
            for val in nv_hash.get("DelValue").items() {
                if val.ref_type() != "HASH" {
                    continue;
                }
                let cap_h = Value::new_hash();
                let match_h = Value::new_hash();
                if add_new_struct_v(it, &Value::Undef, &Value::Undef, &cap_h, &path_pt.deref(), &val, &str_table)? == 0 {
                    continue;
                }
                for p in cap_h.keys().into_iter().map(Value::from) {
                    if m(&rx(" ", ""), &p).is_some() {
                        let mut p2 = p.clone();
                        subst(&rx(r" \d+", ""), &mut p2, true, |_| Ok(br" \d+".to_vec()))?;
                        let re = rx_dyn(&cat!("^", p2, "$").to_bytes(), "")?;
                        matching_paths = struct_paths.iter().filter(|x| m(&re, x).is_some()).cloned().collect();
                        sort_values(&mut matching_paths);
                    } else {
                        matching_paths.push(p.clone());
                    }
                    for p2 in matching_paths.clone() {
                        let Some(c) = m(&pp_re, &p2) else { continue };
                        // language attribute must also match if it exists
                        let attr = cap_h.get(p.to_bytes()).at(1);
                        if attr.get("xml:lang").truthy() {
                            let a2 = capture.get(p2.to_bytes()).at(1);
                            if !(a2.get("xml:lang").truthy() && same_str(&a2.get("xml:lang"), &attr.get("xml:lang"))) {
                                continue;
                            }
                        }
                        let cp2 = capture.get(p2.to_bytes());
                        if cp2.truthy() && same_str(&cp2.at(0), &cap_h.get(p.to_bytes()).at(0)) {
                            // ($1 contains root path for this structure)
                            let c1 = cap(&c, 1);
                            match_h.set(c1.to_bytes(), pl::add(&or(match_h.get(c1.to_bytes()), n(0)), &n(1)));
                        }
                    }
                }
                let num = n(cap_h.len() as i64);
                for p in match_h.keys().into_iter().map(Value::from) {
                    // do nothing unless all fields matched the same structure
                    if !pl::num_eq(&match_h.get(p.to_bytes()), &num) {
                        continue;
                    }
                    // delete all elements of this structure
                    let re = rx_dyn(&cat!("^", p).to_bytes(), "")?;
                    for p2 in &struct_paths {
                        if m(&re, p2).is_some() {
                            del_paths.push(p2.clone());
                        }
                    }
                    // remember path of first deleted structure
                    if !del_path.truthy() || del_path.to_bytes() > p.to_bytes() {
                        del_path = p;
                    }
                }
            }
        } // (else don't delete anything)
    } else if !struct_paths.is_empty() {
        del_paths = struct_paths.clone(); // delete all
        if let Some(c) = m(&pp_re, &struct_paths[0]) {
            del_path = cap(&c, 1);
        }
    }
    if !del_paths.is_empty() {
        let verbose = pl::method(it, et, "Options", vec![pl::s("Verbose")])?;
        if pl::gt(&verbose, &n(1)) {
            sort_values(&mut del_paths);
        }
        for p in &del_paths {
            if pl::gt(&verbose, &n(1)) {
                let mut p2 = p.clone();
                xlat_first(it, &mut p2)?;
                pl::method(it, et, "VerboseValue", vec![cat!("- XMP-", p2), capture.get(p.to_bytes()).at(0)])?;
            }
            capture.delete(p.to_bytes());
            deleted = n(1);
            set_deref(changed, pl::add(&changed.deref(), &n(1)))?;
        }
        if !del_path.truthy() {
            it.warn(pl::s("Internal error 1 in DeleteStruct\n"))?;
            return Ok((Value::Undef, Value::Undef, existed));
        }
        set_deref(path_pt, del_path)?; // return path of first element deleted
    } else if hh(nv_hash, "TagInfo").get("List").truthy() {
        // NOTE: we don't yet properly handle lang-alt elements!!!!
        if !struct_paths.is_empty() {
            let Some(c) = m(&pp_re, struct_paths.last().unwrap()) else {
                it.warn(pl::s("Internal error 2 in DeleteStruct\n"))?;
                return Ok((Value::Undef, Value::Undef, existed));
            };
            let path = cap(&c, 1);
            // delete any improperly formatted xmp
            if capture.get(path.to_bytes()).truthy() {
                let cp = capture.get(path.to_bytes());
                // an error unless this was an empty structure
                if cp.ref_type() != "ARRAY" || cp.at(0).truthy() {
                    pl::method(it, et, "Error", vec![cat!("Improperly structured XMP (", path, ")"), n(1)])?;
                }
                capture.delete(path.to_bytes());
            }
            // (match last index to put in same lang-alt list for Bag of lang-alt items)
            let pb = path.to_bytes();
            let Some(sp) = crate::modules::xmp::m_at(&rx(r".* (\d+)", ""), &pb, 0) else {
                it.warn(pl::s("Internal error 3 in DeleteStruct\n"))?;
                return Ok((Value::Undef, Value::Undef, existed));
            };
            let (a0, a1) = sp[1].unwrap();
            added = Value::bytes(&pb[a0..a1]);
            // add after last item in list
            let len = added.to_bytes().len() as i64;
            let pos = crate::modules::xmp::end_of(&sp) as i64 - len;
            let nxt = pl::add(&substr(&added.to_bytes(), 1, None), &n(1));
            let repl = cat!(n(nxt.to_bytes().len() as i64), nxt);
            let mut np = pb[..pos as usize].to_vec();
            np.extend_from_slice(&repl.to_bytes());
            np.extend_from_slice(&pb[(pos + len) as usize..]);
            set_deref(path_pt, Value::bytes(np))?;
        } else {
            added = pl::s("10");
        }
    }
    Ok((deleted, added, existed))
}

/// Add new element to XMP capture hash
/// Inputs: 0) ExifTool ref, 1) TagInfo ref, 2) capture hash ref,
///         3) resource path, 4) value ref, 5) hash ref for last used index numbers
pub fn add_new_tag(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    add_new_tag_v(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2), arg(&a, 3), &arg(&a, 4).deref(), &arg(&a, 5))?;
    pl::ret_nothing(want)
}

fn add_new_tag_v(it: &mut Interp, et: &Value, tag_info: &Value, capture: &Value, mut path: Value, val_ptr_val: &Value, lang_idx: &Value) -> ER<()> {
    let mut val = escape_xml_v(it, val_ptr_val.clone())?;
    let attrs = Value::new_hash();
    // support writing RDF "resource" values
    if tag_info.get("Resource").truthy() {
        attrs.set("rdf:resource", val);
        val = pl::s("");
    }
    if tag_info.get("Writable").truthy() && pl::str_eq(&tag_info.get("Writable"), "lang-alt") {
        // write the lang-alt tag
        let lang_code = tag_info.get("LangCode");
        // add indexed lang-alt list properties
        let i = or(lang_idx.get(path.to_bytes()), n(0));
        lang_idx.set(path.to_bytes(), pl::add(&i, &n(1))); // save next list index
        if i.truthy() {
            let idx = cat!(n(i.to_bytes().len() as i64), i);
            subst(&rx(r"(.*) \d+", ""), &mut path, false, |c| Ok(cat!(cap(c, 1), " ", idx).to_bytes().to_vec()))?; // set list index
        }
        attrs.set("xml:lang", or(lang_code, pl::s("x-default")));
    }
    capture.set(path.to_bytes(), Value::new_array(vec![val.clone(), attrs]));
    // print verbose message
    if et.truthy() && pl::gt(&pl::method(it, et, "Options", vec![pl::s("Verbose")])?, &n(1)) {
        let mut p = path.clone();
        xlat_first(it, &mut p)?;
        pl::method(it, et, "VerboseValue", vec![cat!("+ XMP-", p), val])?;
    }
    Ok(())
}

/// Add new structure to capture hash for writing
/// Inputs: 0) ExifTool object ref (or undef for no warnings),
///         1) tagInfo ref (or undef if no ExifTool), 2) capture hash ref,
///         3) base path, 4) struct ref, 5) struct hash ref
/// Returns: number of tags changed
/// Notes: Escapes values for XML
pub fn add_new_struct(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(n(add_new_struct_v(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2), &arg(&a, 3), &arg(&a, 4), &arg(&a, 5))?))
}

fn conform(it: &mut Interp, et: &Value, p: &Value) -> ER<Value> {
    pl::call(it, "Image::ExifTool::XMP::ConformPathToNamespace", vec![et.clone(), p.clone()])
}

fn add_new_struct_v(it: &mut Interp, et: &Value, tag_info: &Value, capture: &Value, base_path: &Value, struct_: &Value, str_table: &Value) -> ER<i64> {
    let verbose = if et.truthy() { pl::method(it, et, "Options", vec![pl::s("Verbose")])? } else { n(0) };
    let lang_idx = Value::new_hash();

    let ns = or(str_table.get("NAMESPACE"), pl::s(""));
    let mut changed: i64 = 0;

    // add dummy field to allow empty structures (name starts with '~' so it will come
    // after all valid structure fields, which is necessary when serializing the XMP later)
    if struct_.is_empty() {
        struct_.set("~dummy~", pl::s(""));
    }

    for tag in ordered_keys(it, struct_)? {
        let mut field_info = str_table.get(tag.to_bytes());
        if !field_info.truthy() {
            if !pl::str_eq(&tag, "~dummy~") {
                continue; // check for dummy field
            }
            field_info = Value::new_hash(); // create dummy field info for dummy structure
        }
        let val = struct_.get(tag.to_bytes());
        let mut prop_path = field_info.get("PropertyPath");
        if !prop_path.truthy() {
            prop_path = cat!(or(field_info.get("Namespace"), ns.clone()), ":", or(field_info.get("TagID"), tag.clone()));
            if field_info.get("List").truthy() {
                prop_path = cat!(prop_path, "/rdf:", field_info.get("List"), "/rdf:li 10");
            }
            if field_info.get("Writable").truthy() && pl::str_eq(&field_info.get("Writable"), "lang-alt") {
                prop_path = cat!(prop_path, "/rdf:Alt/rdf:li 10");
            }
            field_info.set("PropertyPath", prop_path.clone()); // save for next time
        }
        let path = cat!(base_path, "/", conform(it, et, &prop_path)?);
        let mut added_tag = false;
        if val.ref_type() == "HASH" {
            let sub_struct = field_info.get("Struct");
            if !sub_struct.truthy() {
                continue;
            }
            changed += add_new_struct_v(it, et, tag_info, capture, &path, &val, &sub_struct)?;
        } else if val.ref_type() == "ARRAY" {
            if !field_info.get("List").truthy() {
                continue;
            }
            let mut i: i64 = 0;
            let level = rx(r" \d+", "").match_all(&prop_path.to_bytes(), 0).map(|v| v.len()).unwrap_or(0);
            // loop through all list items (note: can't yet write multi-dimensional lists)
            for item in val.items() {
                let p;
                if i != 0 {
                    // update first index in field property (may be list of lang-alt lists)
                    let mut pc = conform(it, et, &prop_path)?;
                    let idx = cat!(n(i.to_string().len() as i64), n(i));
                    subst(&rx(r" \d+", ""), &mut pc, false, |_| Ok(cat!(" ", idx).to_bytes().to_vec()))?;
                    p = cat!(base_path, "/", pc);
                } else {
                    p = path.clone();
                }
                if item.ref_type() == "HASH" {
                    let sub_struct = field_info.get("Struct");
                    if !sub_struct.truthy() {
                        continue;
                    }
                    if add_new_struct_v(it, et, tag_info, capture, &p, &item, &sub_struct)? == 0 {
                        continue;
                    }
                // don't write empty items in upper-level list
                } else if pl::length(&item).truthy() || (!item.is_undef() && level == 1) {
                    add_new_tag_v(it, et, &field_info, capture, p, &item, &lang_idx)?;
                    added_tag = true;
                }
                changed += 1;
                i += 1;
            }
        } else {
            add_new_tag_v(it, et, &field_info, capture, path, &val, &lang_idx)?;
            added_tag = true;
            changed += 1;
        }
        // this is tricky, but we must add the rdf:type for contained structures
        // in the case that a whole hierarchy was added at once by writing a
        // flattened tag inside a variable-namespace structure
        if added_tag && field_info.get("StructType").truthy() && field_info.get("Table").truthy() {
            it.call_named("Image::ExifTool::XMP::AddStructType", vec![et.clone(), field_info.get("Table"), capture.clone(), prop_path.clone(), base_path.clone()], Want::Void)?;
        }
    }
    // add 'rdf:type' property if necessary
    if str_table.get("TYPE").truthy() && changed != 0 {
        let path = cat!(base_path, "/", conform(it, et, &pl::s("rdf:type"))?);
        if !capture.get(path.to_bytes()).truthy() {
            capture.set(path.to_bytes(), Value::new_array(vec![pl::s(""), hash_of(vec![("rdf:resource", str_table.get("TYPE"))])]));
            if pl::gt(&verbose, &n(1)) {
                let mut p = path.clone();
                xlat_first(it, &mut p)?;
                pl::method(it, et, "VerboseValue", vec![cat!("+ XMP-", p), str_table.get("TYPE")])?;
            }
        }
    }
    Ok(changed)
}

/// Convert structure field values for printing
/// Inputs: 0) ExifTool ref, 1) tagInfo ref for structure tag, 2) value,
///         3) conversion type: PrintConv, ValueConv or Raw (Both not allowed)
///         4) tagID of parent structure (needed only if there was no flattened tag)
/// Notes: Makes a copy of the hash so any applied escapes won't affect raw values
pub fn convert_struct(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    convert_struct_v(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2), &arg(&a, 3), arg(&a, 4), want)
}

fn convert_struct_s(it: &mut Interp, et: &Value, tag_info: &Value, value: &Value, type_: &Value, parent_id: Value) -> ER<Value> {
    Ok(convert_struct_v(it, et, tag_info, value, type_, parent_id, Want::Scalar)?.pop().unwrap_or_default())
}

fn convert_struct_v(it: &mut Interp, et: &Value, tag_info: &Value, value: &Value, type_: &Value, mut parent_id: Value, want: Want) -> ER<Vec<Value>> {
    if value.ref_type() == "HASH" {
        let struct_ = Value::new_hash();
        let table = tag_info.get("Table");
        if !parent_id.truthy() {
            parent_id = tag_info.get("TagID");
        }
        if value.get("_ordered_keys_").truthy() {
            struct_.set("_ordered_keys_", Value::new_array(Vec::new()));
        }
        for key in ordered_keys(it, value)? {
            let mut tag_id = pl::concat(&parent_id, &ucfirst(&key));
            let mut flat_info = table.get(tag_id.to_bytes());
            if !flat_info.truthy() {
                // handle variable-namespace structures
                if let Some(c) = m(&rx(r"^XMP-(.*?:)(.*)", ""), &key) {
                    tag_id = cat!(cap(&c, 1), parent_id, ucfirst(&cap(&c, 2)));
                    flat_info = table.get(tag_id.to_bytes());
                }
                if !flat_info.truthy() {
                    flat_info = tag_info.clone();
                }
            }
            let mut v = value.get(key.to_bytes());
            if v.is_ref() {
                v = convert_struct_s(it, et, &flat_info, &v, type_, tag_id)?;
            } else {
                v = pl::method(it, et, "GetValue", vec![flat_info, type_.clone(), v])?;
            }
            if !v.is_undef() {
                struct_.set(key.to_bytes(), v); // save the converted value
                // maintain ordered keys if necessary
                if struct_.get("_ordered_keys_").truthy() {
                    struct_.get("_ordered_keys_").push(key.clone());
                }
            }
        }
        ret(struct_)
    } else if value.ref_type() == "ARRAY" {
        let opts = hh(et, "OPTIONS");
        if !opts.get("ListItem").is_undef() {
            let li = opts.get("ListItem");
            if value.at(li.to_i64()).is_undef() {
                return pl::ret_undef();
            }
            opts.set("ListItem", Value::Undef); // only do top-level list
            let val = convert_struct_s(it, et, tag_info, &value.at(li.to_i64()), type_, parent_id)?;
            opts.set("ListItem", li);
            ret(val)
        } else {
            let list = Value::new_array(Vec::new());
            for val in value.items() {
                let v = convert_struct_s(it, et, tag_info, &val, type_, parent_id.clone())?;
                if !v.is_undef() {
                    list.push(v);
                }
            }
            ret(list)
        }
    } else {
        it.call_method(et, "GetValue", vec![et.clone(), tag_info.clone(), type_.clone(), value.clone()], want)
    }
}

/// Restore XMP structures in extracted information
/// Inputs: 0) ExifTool object ref, 1) flag to keep original flattened tags
/// Notes: also restores lists (including multi-dimensional)
pub fn restore_struct(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    crate::modules::xmp::local_topic(it, |it| restore_struct_v(it, &arg(&a, 0), &arg(&a, 1)))?;
    pl::ret_nothing(want)
}

fn restore_struct_v(it: &mut Interp, et: &Value, keep_flat: &Value) -> ER<()> {
    // (hashes keyed by references, as in the Perl)
    let (structs, var, lists, list_keys) = (Value::new_hash(), Value::new_hash(), Value::new_hash(), Value::new_hash());
    let mut si_list: Vec<Value> = Vec::new();
    let value_hash = et.get("VALUE");
    let file_order = et.get("FILE_ORDER");
    let tag_extra = et.get("TAG_EXTRA");
    let std_xlat_ns = pkg_hash(it, "Image::ExifTool::XMP::stdXlatNS");
    let ti_keys: Vec<Value> = et.get("TAG_INFO").keys().into_iter().map(Value::from).collect();
    'key: for key in ti_keys {
        let kb = key.to_bytes();
        let struct_props = hh(&tag_extra, &kb).get("Struct");
        if !struct_props.truthy() {
            continue;
        }
        tag_extra.get(&kb).delete("Struct"); // (don't re-use)
        let tag_info = et.get("TAG_INFO").get(&kb); // tagInfo for flattened tag
        let table = tag_info.get("Table");
        let mut prop = shift(&struct_props);
        let mut tag = prop.at(0);
        // get reference to structure tag (or normal list tag if not a structure)
        let mut str_info = if !struct_props.is_empty() { table.get(tag.to_bytes()) } else { tag_info.clone() };
        if str_info.truthy() {
            if !is_hash(&str_info) {
                continue; // (just to be safe)
            }
            if !struct_props.is_empty() && !str_info.get("Struct").truthy() {
                // this could happen for invalid XMP containing mixed lists
                // (or for something like this -- what should we do here?:
                // <meta:user-defined meta:name="License">test</meta:user-defined>)
                if !et.get("NO_STRUCT_WARN").truthy() {
                    pl::method(it, et, "Warn", vec![cat!(str_info.get("Name"), " is not a structure!")])?;
                }
                continue;
            }
        } else {
            // create new entry in tag table for this structure
            let mut g1 = or(hh(&table, "GROUPS").get("0"), pl::s("XMP"));
            let mut name = tag.clone();
            // tag keys will have a group 1 prefix when coming from import of XML from -X option
            if let Some(c) = m(&rx(r"(.+):(.+)", ""), &tag) {
                let mut ns;
                (ns, name) = (cap(&c, 1), cap(&c, 2));
                subst(&rx("^XMP-", ""), &mut ns, false, |_| Ok(Vec::new()))?; // remove leading "XMP-" if it exists because we add it later
                if std_xlat_ns.get(ns.to_bytes()).truthy() {
                    ns = std_xlat_ns.get(ns.to_bytes());
                }
                g1 = cat!(g1, "-", ns);
            }
            str_info = hash_of(vec![("Name", ucfirst(&name)), ("Groups", hash_of(vec![("1", g1)])), ("Struct", pl::s("Unknown"))]);
            // add Struct entry if this is a structure
            if !struct_props.is_empty() {
                // this is a structure
                str_info.set("Struct", hash_of(vec![("STRUCT_NAME", pl::s("XMP Unknown"))]));
            } else if tag_info.get("LangCode").truthy() {
                // this is lang-alt list
                tag = cat!(tag, "-", tag_info.get("LangCode"));
                str_info.set("LangCode", tag_info.get("LangCode"));
            }
            pl::call(it, "Image::ExifTool::AddTagToTable", vec![table.clone(), tag.clone(), str_info.clone()])?;
        }
        // use strInfo ref for base key to avoid collisions
        tag = str_info.clone();
        let mut struct_ = structs.clone();
        let si_key = str_info.to_bytes();
        let old_struct = structs.get(&si_key);
        // (fyi: 'lang-alt' Writable type will be valid even if tag is not pre-defined)
        let writable = or(tag_info.get("Writable"), pl::s(""));
        // walk through the stored structure property information
        // to rebuild this structure
        let mut err: i64 = 0;
        loop {
            let mut index = prop.at(1);
            if index.truthy() && struct_props.is_empty() {
                // ignore this list if it is a simple lang-alt tag
                if pl::str_eq(&writable, "lang-alt") {
                    apop(&prop); // remove lang-alt index
                    if prop.len() < 2 {
                        index = Value::Undef;
                    }
                }
                // add language code if necessary
                if tag_info.get("LangCode").truthy() && !tag.is_ref() {
                    tag = cat!(tag, "-", tag_info.get("LangCode"));
                }
            }
            let mut next_struct = struct_.get(tag.to_bytes());
            if !index.is_undef() {
                // the field is a list
                index = substr(&index.to_bytes(), 1, None); // remove digit count
                if next_struct.truthy() {
                    if next_struct.ref_type() != "ARRAY" {
                        err = 2;
                        break;
                    }
                    struct_ = next_struct;
                } else {
                    let a = Value::new_array(Vec::new());
                    struct_.set(tag.to_bytes(), a.clone());
                    struct_ = a;
                }
                next_struct = struct_.at(index.to_i64());
                // descend into multi-dimensional lists
                let mut i = 2;
                while prop.at(i).truthy() {
                    if next_struct.truthy() {
                        if next_struct.ref_type() != "ARRAY" {
                            break;
                        }
                        struct_ = next_struct;
                    } else {
                        lists.set(struct_.to_bytes(), struct_.clone());
                        let a = Value::new_array(Vec::new());
                        struct_.set_at(index.to_i64() as usize, a.clone());
                        struct_ = a;
                    }
                    next_struct = struct_.at(index.to_i64());
                    index = substr(&prop.at(i).to_bytes(), 1, None);
                    i += 1;
                }
                if next_struct.ref_type() == "HASH" {
                    struct_ = next_struct; // continue building sub-structure
                } else if !struct_props.is_empty() {
                    lists.set(struct_.to_bytes(), struct_.clone());
                    let h = Value::new_hash();
                    struct_.set_at(index.to_i64() as usize, h.clone());
                    struct_ = h;
                } else {
                    lists.set(struct_.to_bytes(), struct_.clone());
                    struct_.set_at(index.to_i64() as usize, value_hash.get(&kb));
                    break;
                }
            } else if next_struct.truthy() {
                if next_struct.ref_type() != "HASH" {
                    err = 3;
                    break;
                }
                struct_ = next_struct;
            } else if !struct_props.is_empty() {
                let h = Value::new_hash();
                struct_.set(tag.to_bytes(), h.clone());
                struct_ = h;
            } else {
                struct_.set(tag.to_bytes(), value_hash.get(&kb));
                break;
            }
            prop = shift(&struct_props);
            if !prop.truthy() {
                break;
            }
            tag = prop.at(0);
            if let Some(c) = m(&rx(r"(.+):(.+)", ""), &tag) {
                // tag in variable-namespace tables will have a leading
                // XMP namespace on the tag name.  In this case, add
                // the corresponding group1 name to the tag ID.
                let (mut ns, name) = (cap(&c, 1), cap(&c, 2));
                if std_xlat_ns.get(ns.to_bytes()).truthy() {
                    ns = std_xlat_ns.get(ns.to_bytes());
                }
                tag = cat!("XMP-", ns, ":", ucfirst(&name));
            } else {
                tag = ucfirst(&tag);
            }
        }
        if err != 0 {
            // this may happen if we have a structural error in the XMP
            // (like an improperly contained list for example)
            if !et.get("NO_STRUCT_WARN").truthy() {
                let ns = or(or(tag_info.get("Namespace"), hh(&tag_info, "Table").get("NAMESPACE")), pl::s(""));
                pl::method(it, et, "Warn", vec![cat!("Error ", n(err), " placing ", ns, ":", tag_info.get("TagID"), " in structure or list"), n(1)])?;
            }
            if !old_struct.truthy() {
                structs.delete(&si_key);
            }
        } else if same_str(&tag_info, &str_info) {
            // just a regular list tag (or an empty structure)
            if old_struct.truthy() {
                // keep tag with lowest numbered key (well, not exactly, since
                // "Tag (10)" is lt "Tag (2)", but at least "Tag" is lt
                // everything else, and this is really what we care about)
                let k = list_keys.get(old_struct.to_bytes());
                if k.truthy() {
                    // ($k will be undef for an empty structure)
                    if k.to_bytes() < kb {
                        // keep lowest file order
                        if pl::gt(&file_order.get(k.to_bytes()), &file_order.get(&kb)) {
                            file_order.set(k.to_bytes(), file_order.get(&kb));
                        }
                        pl::method(it, et, "DeleteTag", vec![key.clone()])?;
                        continue 'key;
                    }
                    if pl::gt(&file_order.get(&kb), &file_order.get(k.to_bytes())) {
                        file_order.set(&kb, file_order.get(k.to_bytes()));
                    }
                    pl::method(it, et, "DeleteTag", vec![k])?; // remove tag with greater copy number
                }
            }
            // replace existing value with new list
            value_hash.set(&kb, structs.get(&si_key));
            list_keys.set(structs.get(&si_key).to_bytes(), key.clone()); // save key for this list tag
        } else {
            // save strInfo ref and file order
            let v = var.get(&si_key);
            if v.truthy() {
                // set file order to just before the first associated flattened tag
                if pl::gt(&v.at(1), &file_order.get(&kb)) {
                    v.set_at(1, pl::sub(&file_order.get(&kb), &Value::Num(0.5)));
                }
            } else {
                var.set(&si_key, Value::new_array(vec![str_info.clone(), pl::sub(&file_order.get(&kb), &Value::Num(0.5))]));
            }
            // preserve original flattened tags if requested
            if keep_flat.truthy() {
                let extra = tag_extra.get(&kb);
                // restore list behaviour of this flattened tag
                if extra.get("NoList").truthy() {
                    value_hash.set(&kb, extra.get("NoList"));
                    extra.delete("NoList");
                } else if extra.get("NoListDel").truthy() {
                    // delete this tag since its value was included another list
                    pl::method(it, et, "DeleteTag", vec![key.clone()])?;
                }
            } else {
                pl::method(it, et, "DeleteTag", vec![key.clone()])?; // delete the flattened tag
            }
        }
    }
    // fill in undefined items in lists.  In theory, undefined list items should
    // be fine, but in practice the calling code may not check for this (and
    // historically this wasn't necessary, so do this for backward compatibility)
    for si in lists.keys() {
        let l = lists.get(&si);
        for (i, x) in l.items().into_iter().enumerate() {
            if x.is_undef() {
                l.set_at(i, pl::s(""));
            }
        }
    }
    // make a list of all new structures we generated
    for k in structs.keys() {
        if var.get(&k).truthy() {
            si_list.push(Value::from(k));
        }
    }
    // save new structures in the same order they were read from file
    si_list.sort_by(|a, b| num_cmp(&var.get(a.to_bytes()).at(1), &var.get(b.to_bytes()).at(1)));
    for si in si_list {
        let sb = si.to_bytes();
        // test to see if a tag for this structure has already been generated
        // (this could happen only if one of the structures in a list was empty)
        let mut key = var.get(&sb).at(0).get("Name");
        let mut found = false;
        if value_hash.get(key.to_bytes()).truthy() {
            let mut p = b"^".to_vec();
            p.extend_from_slice(&key.to_bytes());
            p.extend_from_slice(br"( \(\d+\))?$");
            let re = rx_dyn(&p, "")?;
            let keys: Vec<Value> = value_hash.keys().into_iter().filter(|k| crate::modules::xmp::m_at(&re, k, 0).is_some()).map(Value::from).collect();
            for k in keys {
                if !same_str(&value_hash.get(k.to_bytes()), &structs.get(&sb)) {
                    continue;
                }
                found = true;
                break;
            }
        }
        if !found {
            // otherwise, generate a new tag for this structure
            key = pl::method(it, et, "FoundTag", vec![var.get(&sb).at(0), pl::s("")])?;
            value_hash.set(key.to_bytes(), structs.get(&sb));
        }
        file_order.set(key.to_bytes(), var.get(&sb).at(1));
    }
    Ok(())
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

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::XMP::SerializeStruct", serialize_struct);
    it.define_native("Image::ExifTool::XMP::InflateStruct", inflate_struct);
    it.define_native("Image::ExifTool::XMP::EscapeJSON", escape_json);
    it.define_native("Image::ExifTool::XMP::GetLangCode", get_lang_code);
    it.define_native("Image::ExifTool::XMP::DumpStruct", dump_struct);
    it.define_native("Image::ExifTool::XMP::CheckStruct", check_struct);
    it.define_native("Image::ExifTool::XMP::DeleteStruct", delete_struct);
    it.define_native("Image::ExifTool::XMP::AddNewTag", add_new_tag);
    it.define_native("Image::ExifTool::XMP::AddNewStruct", add_new_struct);
    it.define_native("Image::ExifTool::XMP::ConvertStruct", convert_struct);
    it.define_native("Image::ExifTool::XMP::RestoreStruct", restore_struct);
}
