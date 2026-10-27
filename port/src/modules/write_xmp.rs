//! Image::ExifTool::WriteXMP (Image/ExifTool/WriteXMP.pl), ported sub by sub (see docs/port/PORTING.md).
//!
//! Write XMP meta information. The file is autoloaded in package
//! Image::ExifTool::XMP; its `my` variables (`$pktOpen`, `%extendedRes`,
//! `$sp`, `$nl`...) live under `Image::ExifTool::XMP::<my:WriteXMP.pl>::NAME`.
//!
//! `m//g` in scalar context and `pos()` are kept as explicit match end
//! offsets; `foreach` loops that alias a variable use a loop variable of
//! their own, as the aliased variable is restored after the loop.

use std::cmp::Ordering;
use std::rc::Rc;

use crate::interp::eval::{substr_range, ER};
use crate::interp::{Binding, Interp, Want};
use crate::modules::xmp::{cat, end_of, hh, m, m_at, rx_dyn};
use crate::perl::regex::{quotemeta, PerlRegex};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret, Caps};

const XMP: &str = "Image::ExifTool::XMP";

// ---- local helpers ----

fn lex_name(name: &str) -> String {
    format!("{XMP}::<my:WriteXMP.pl>::{name}")
}

/// A scalar `my` variable of WriteXMP.pl.
fn lex_s(it: &mut Interp, name: &str) -> Value {
    match it.global(b'$', &lex_name(name)) {
        Binding::Scalar(c) => c.borrow().clone(),
        _ => Value::Undef,
    }
}

fn set_lex_s(it: &mut Interp, name: &str, v: Value) {
    if let Binding::Scalar(c) = it.global(b'$', &lex_name(name)) {
        *c.borrow_mut() = v;
    }
}

/// A hash `my` variable of WriteXMP.pl.
fn lex_h(it: &mut Interp, name: &str) -> Value {
    match it.global(b'%', &lex_name(name)) {
        Binding::Hash(h) => Value::HashRef(h),
        _ => Value::new_hash(),
    }
}

fn ns_uri(it: &mut Interp) -> Value {
    pl::pkg_hash(it, "Image::ExifTool::XMP::nsURI")
}

/// An interpolated pattern.
fn rd(p: &[u8], flags: &str) -> ER<Rc<PerlRegex>> {
    rx_dyn(p, flags)
}

/// `$s =~ /PAT/` with an interpolated pattern.
fn md(p: &[u8], flags: &str, s: &Value) -> ER<Option<Caps>> {
    Ok(m(&*rd(p, flags)?, s))
}

/// Bytes of a pattern built from pieces.
fn pb(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

fn cmp_str(a: &Value, b: &Value) -> Ordering {
    a.to_bytes().cmp(&b.to_bytes())
}

/// `sort keys %$h` (string order).
fn sorted_keys(h: &Value) -> Vec<Value> {
    let mut k: Vec<Rc<[u8]>> = h.keys();
    k.sort();
    k.into_iter().map(Value::Str).collect()
}

fn keys(h: &Value) -> Vec<Value> {
    h.keys().into_iter().map(Value::Str).collect()
}

/// `$x x $n`
fn rep(s: &Value, k: i64) -> Value {
    if k <= 0 {
        return pl::s("");
    }
    Value::bytes(s.to_bytes().repeat(k as usize))
}

/// `substr($s, $off, $len) = $repl`
fn substr_set(s: &Value, off: i64, len: Option<i64>, repl: &Value) -> Value {
    let b = s.to_bytes();
    match substr_range(b.len(), off, len) {
        Some((a, e)) => {
            let mut o = b[..a].to_vec();
            o.extend_from_slice(&repl.to_bytes());
            o.extend_from_slice(&b[e..]);
            Value::bytes(o)
        }
        None => s.clone(),
    }
}

/// `$$a[i] .= $s` (negative from the end)
fn app(a: &Value, i: i64, s: Value) {
    let len = a.len() as i64;
    let idx = if i < 0 { len + i } else { i };
    if idx < 0 {
        return;
    }
    a.set_at(idx as usize, pl::concat(&a.at(idx), &s));
}

/// `$$a[i] = $s` (negative from the end)
fn set_i(a: &Value, i: i64, s: Value) {
    let len = a.len() as i64;
    let idx = if i < 0 { len + i } else { i };
    if idx >= 0 {
        a.set_at(idx as usize, s);
    }
}

/// `%$dst = %$src` (the same hash, new contents)
fn hash_assign(dst: &Value, src: &Value) {
    if let (Value::HashRef(d), Value::HashRef(s)) = (dst, src) {
        if Rc::ptr_eq(d, s) {
            return;
        }
        let mut c = s.borrow().clone();
        c.blessed = None;
        *d.borrow_mut() = c;
    } else if let Value::HashRef(d) = dst {
        d.borrow_mut().clear();
    }
}

/// `$#$a = $n - 1`: truncate or extend an array to `len` elements.
fn set_len(a: &Value, len: usize) {
    if let Value::ArrayRef(r) = a {
        r.borrow_mut().resize(len, Value::Undef);
    }
}

fn apop(a: &Value) -> Value {
    match a {
        Value::ArrayRef(r) => r.borrow_mut().pop().unwrap_or_default(),
        _ => Value::Undef,
    }
}

fn ashift(a: &mut Vec<Value>) -> Value {
    if a.is_empty() {
        Value::Undef
    } else {
        a.remove(0)
    }
}

fn split_slash(s: &Value) -> Vec<Value> {
    pl::split(&re("/", ""), s, 0)
}

fn meth(it: &mut Interp, et: &Value, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::method(it, et, name, args)
}

fn warn_et(it: &mut Interp, et: &Value, msg: Value) -> ER<()> {
    meth(it, et, "Warn", vec![msg])?;
    Ok(())
}

fn xcall(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("{XMP}::{name}"), args)
}

fn escape_xml(it: &mut Interp, v: Value) -> ER<Value> {
    xcall(it, "EscapeXML", vec![v])
}

fn unescape_xml(it: &mut Interp, v: Value) -> ER<Value> {
    xcall(it, "UnescapeXML", vec![v])
}

/// `GetXMPTagID(\@props)` in scalar context
fn get_xmp_tag_id1(it: &mut Interp, props: &Value) -> ER<Value> {
    xcall(it, "GetXMPTagID", vec![props.clone()])
}

fn inc(v: &mut Value) {
    *v = pl::add(v, &n(1));
}

/// `hex($s)` as perl's builtin (optional "0x"/"x" prefix, underscores).
fn hex(v: &Value) -> Value {
    let b = v.to_bytes();
    let mut s: &[u8] = &b;
    if s.starts_with(b"0x") || s.starts_with(b"0X") {
        s = &s[2..];
    } else if s.starts_with(b"x") || s.starts_with(b"X") {
        s = &s[1..];
    }
    let digits: Vec<u8> = s.iter().copied().take_while(|c| c.is_ascii_hexdigit() || *c == b'_').filter(|c| *c != b'_').collect();
    crate::modules::xmp::hex(&Value::bytes(digits))
}

/// `Pkg::Func($a)` where the sub writes its arguments (`$_[0] = ...`).
/// (`pl::call_mut` goes through `Interp::sub_ref`, whose indirection drops
/// the aliases when the target is a native: the sub is called directly.)
pub(crate) fn call_mut(it: &mut Interp, name: &str, args: &mut [Value], want: Want) -> ER<Vec<Value>> {
    let cells: Vec<_> = args.iter().map(|v| Rc::new(std::cell::RefCell::new(v.clone()))).collect();
    let al = cells.iter().map(|c| Some(crate::interp::eval::Lv::Scalar(c.clone()))).collect();
    let code = match it.sub(name) {
        Some(c) => c,
        None => it.sub_ref(name),
    };
    let r = it.call_with_aliases(&code, args.to_vec(), al, want);
    for (a, c) in args.iter_mut().zip(cells) {
        *a = c.borrow().clone();
    }
    r
}

// ---- the subs of WriteXMP.pl ----

/// Get XMP opening tag (and set x:xmptk appropriately)
/// Inputs: 0) ExifTool object ref
/// Returns: x:xmpmeta opening tag
pub fn xmp_open(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(xmp_open_v(it, &arg(&a, 0))?)
}

fn xmp_open_v(it: &mut Interp, et: &Value) -> ER<Value> {
    let xmptk = pl::pkg_hash(it, "Image::ExifTool::XMP::x").get("xmptk");
    let nv = hh(et, "NEW_VALUE").get(xmptk.to_bytes());
    let tk;
    if !nv.is_undef() {
        tk = meth(it, et, "GetNewValue", vec![nv])?;
        meth(it, et, "VerboseValue", vec![cat!(if tk.truthy() { "+" } else { "-" }, " XMP-x:XMPToolkit"), tk.clone()])?;
        et.set("CHANGED", pl::add(&et.get("CHANGED"), &n(1)));
    } else {
        tk = cat!("Image::ExifTool ", it.get_scalar("Image::ExifTool::VERSION"));
    }
    let str_ = if tk.truthy() { cat!(" x:xmptk='", escape_xml(it, tk)?, "'") } else { pl::s("") };
    Ok(cat!(lex_s(it, "xmpOpenPrefix"), str_, ">\n"))
}

/// Validate XMP packet and set read or read/write mode
/// Inputs: 0) XMP data reference, 1) 'r' = read only, 'w' or undef = read/write
/// Returns: true if XMP is good (and adds packet header/trailer if necessary)
pub fn validate_xmp(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(validate_xmp_v(it, &arg(&a, 0), arg(&a, 1))?)
}

fn validate_xmp_v(it: &mut Interp, xmp_pt: &Value, mut mode: Value) -> ER<Value> {
    let mut x = xmp_pt.deref();
    if re(r"^\s*<!--.*?-->\s*", "s").subst(&mut x, false, |_| Vec::new()) > 0 {
        pl::set_deref(xmp_pt, x.clone())?; // remove leading comment if it exists
    }
    if !re(r"^\0*<\0*\?\0*x\0*p\0*a\0*c\0*k\0*e\0*t", "").is_match(&x) {
        if !re(r"^<x(mp)?:x[ma]pmeta", "").is_match(&x) {
            return Ok(pl::s(""));
        }
        // add required xpacket header/trailer
        x = cat!(lex_s(it, "pktOpen"), x, lex_s(it, "pktCloseW"));
        pl::set_deref(xmp_pt, x.clone())?;
    }
    if !mode.truthy() {
        mode = pl::s("w");
    }
    let mut end = pl::substr(&x, -32, Some(32));
    // check for proper xpacket trailer and set r/w mode if necessary
    let mut c2 = Value::Undef;
    let mb = mode.to_bytes();
    let k = re(r#"(e\0*n\0*d\0*=\0*['"]\0*)([rw])(\0*['"]\0*\?\0*>)"#, "").subst(&mut end, false, |c| {
        c2 = cap(c, 2);
        pb(&[&cap(c, 1).to_bytes(), &mb, &cap(c, 3).to_bytes()])
    });
    if k == 0 {
        return Ok(pl::s(""));
    }
    if c2.to_bytes() != mode.to_bytes() {
        let x = xmp_pt.deref();
        pl::set_deref(xmp_pt, substr_set(&x, -32, Some(32), &end))?;
    }
    Ok(n(1))
}

/// Validate XMP property
/// Inputs: 0) ExifTool ref, 1) validate hash ref, 2) attribute hash ref
/// - issues warnings if problems detected
pub fn validate_property(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, prop_list, attr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let pl_ = prop_list.items();
    let np = pl_.len() as i64;
    if et.get("XmpValidate").truthy() && np > 2 {
        if re(r"^x:x[ma]pmeta$", "").is_match(&pl_[0]) && pl::str_eq(&pl_[1], "rdf:RDF") && re(r"rdf:Description( |$)", "").is_match(&pl_[2]) {
            if np > 3 {
                let last = prop_list.at(-1);
                if re(r"^rdf:(Bag|Seq|Alt)$", "").is_match(&last) {
                    meth(it, &et, "Warn", vec![cat!("Ignored empty ", last, " list for ", prop_list.at(-2)), n(1)])?;
                } else {
                    if pl::str_eq(&prop_list.at(-2), "rdf:Alt") && attr.truthy() {
                        let lang = attr.get("xml:lang");
                        if lang.truthy() && np >= 5 {
                            let lang_path = pl::join("/", &pl_[3..(np as usize - 1 - 2 + 1)]);
                            let mut val_lang = et.get("XmpValidateLangAlt");
                            if !val_lang.truthy() {
                                val_lang = Value::new_hash();
                                et.set("XmpValidateLangAlt", val_lang.clone());
                            }
                            if !val_lang.get(lang_path.to_bytes()).truthy() {
                                val_lang.set(lang_path.to_bytes(), Value::new_hash());
                            }
                            if hh(&val_lang, lang_path.to_bytes()).get(lang.to_bytes()).truthy() {
                                warn_et(it, &et, cat!("Duplicate language (", lang, ") in lang-alt list: ", lang_path))?;
                            } else {
                                hh(&val_lang, lang_path.to_bytes()).set(lang.to_bytes(), n(1));
                            }
                        }
                    }
                    let validate_ns = lex_h(it, "validateNS");
                    let mut caps = None;
                    if let Some(c) = re(r"^(\w+):(\w+)", "").m(&last) {
                        if validate_ns.get(cap(&c, 1).to_bytes()).truthy() {
                            caps = Some(c);
                        }
                    }
                    if caps.is_none() && pl::str_eq(&last, "rdf:li") {
                        if let Some(c) = re(r"^(\w+):(\w+)", "").m(&prop_list.at(-3)) {
                            if validate_ns.get(cap(&c, 1).to_bytes()).truthy() {
                                caps = Some(c);
                            }
                        }
                    }
                    if let Some(c) = caps {
                        let tbl = pl::call(it, "Image::ExifTool::GetTagTable", vec![cat!("Image::ExifTool::XMP::", cap(&c, 1))])?;
                        if tbl.truthy() && !tbl.get(cap(&c, 2).to_bytes()).truthy() {
                            warn_et(it, &et, cat!("Non-standard XMP property: ", cap(&c, 1), ":", cap(&c, 2)))?;
                        }
                    }
                    let xmp_validate = et.get("XmpValidate");
                    let path = pl::join("/", &pl_[3..]);
                    if !xmp_validate.get(path.to_bytes()).is_undef() {
                        warn_et(it, &et, cat!("Duplicate XMP property: ", path))?;
                    } else {
                        xmp_validate.set(path.to_bytes(), n(1));
                    }
                }
            }
        } else if !pl::str_eq(&pl_[0], "rdf:RDF") || !re(r"rdf:Description( |$)", "").is_match(&pl_[1]) {
            warn_et(it, &et, cat!("Improperly enclosed XMP property: ", pl::join("/", &pl_)))?;
        }
    }
    pl::ret_nothing(want)
}

/// Check XMP date values for validity and format accordingly
/// Inputs: 1) EXIF-format date string (XMP-format also accepted)
/// Returns: XMP date/time string (or undef on error)
pub fn format_xmp_date(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(format_xmp_date_v(arg(&a, 0)))
}

fn format_xmp_date_v(mut val: Value) -> Value {
    let mut tz = Value::Undef;
    let c = re(r"(\d{4}):(\d{2}):(\d{2}) (\d{2}:\d{2}(?::\d{2}(?:\.\d*)?)?)(.*)", "").m(&val).or_else(|| re(r"(\d{4})-(\d{2})-(\d{2})T(\d{2}:\d{2}(?::\d{2}(?:\.\d*)?)?)(.*)", "").m(&val));
    if let Some(c) = c {
        let (y, mm, d, t) = (cap(&c, 1), cap(&c, 2), cap(&c, 3), cap(&c, 4));
        tz = cap(&c, 5);
        val = cat!(y, "-", mm, "-", d, "T", t);
    } else if re(r"^\s*\d{4}(:\d{2}){0,2}\s*$", "").is_match(&val) {
        // this is just a date (YYYY, YYYY-mm or YYYY-mm-dd)
        pl::tr(&mut val, ":", "-", "");
    } else if let Some(c) = re(r"^\s*(\d{2}:\d{2}(?::\d{2}(?:\.\d*)?)?)(.*)\s*$", "").m(&val) {
        // this is just a time
        let t = cap(&c, 1);
        tz = cap(&c, 2);
        val = t;
    } else {
        return Value::Undef;
    }
    if tz.truthy() {
        if !re(r"^(Z|[+-]\d{2}:\d{2})$", "").is_match(&tz) {
            return Value::Undef;
        }
        val = pl::concat(&val, &tz);
    }
    val
}

/// Check XMP values for validity and format accordingly
/// Inputs: 0) ExifTool object ref, 1) tagInfo hash ref, 2) raw value ref, 3) conversion type
/// Returns: error string or undef (and may change value) on success
/// Note: copies structured information to avoid conflicts with calling code
pub fn check_xmp(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(check_xmp_v(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2), &arg(&a, 3))?)
}

fn check_xmp_v(it: &mut Interp, et: &Value, tag_info: &Value, val_ptr: &Value, conv_type: &Value) -> ER<Value> {
    if tag_info.get("Struct").truthy() {
        crate::modules::xmp::require(it, "Image/ExifTool/XMPStruct.pl")?;
        let mut err = Value::Undef;
        let mut warn = Value::Undef;
        if !val_ptr.deref().is_ref() {
            let r = pl::call_list(it, "Image::ExifTool::XMP::InflateStruct", vec![et.clone(), val_ptr.clone()])?;
            pl::set_deref(val_ptr, arg(&r, 0))?;
            warn = arg(&r, 1);
            // expect a structure HASH ref or ARRAY of structures
            if !val_ptr.deref().is_ref() {
                if pl::str_eq(&val_ptr.deref(), "") {
                    pl::set_deref(val_ptr, Value::new_hash())?;
                    return Ok(Value::Undef); // allow empty structures
                }
                return Ok(pl::s("Improperly formed structure"));
            }
        }
        if val_ptr.deref().ref_type() == "ARRAY" {
            if !tag_info.get("List").truthy() {
                return Ok(pl::s("Not a list tag"));
            }
            let copy = Value::new_array(val_ptr.deref().items()); // copy the list for ExifTool to use
            pl::set_deref(val_ptr, copy.clone())?; // return the copy
            for i in 0..copy.len() {
                let item = copy.at(i as i64);
                if item.ref_type() != "HASH" {
                    let ir = pl::sref(item);
                    let r = pl::call_list(it, "Image::ExifTool::XMP::InflateStruct", vec![et.clone(), ir])?; // deserialize structure
                    copy.set_at(i, arg(&r, 0));
                    let w = arg(&r, 1);
                    if w.truthy() {
                        warn = w;
                    }
                    if copy.at(i as i64).ref_type() == "HASH" {
                        continue;
                    }
                    err = pl::s("Improperly formed structure");
                    break;
                }
                let r = pl::call_list(it, "Image::ExifTool::XMP::CheckStruct", vec![et.clone(), copy.at(i as i64), tag_info.get("Struct")])?;
                copy.set_at(i, arg(&r, 0));
                err = arg(&r, 1);
                if err.truthy() {
                    break;
                }
            }
        } else {
            let r = pl::call_list(it, "Image::ExifTool::XMP::CheckStruct", vec![et.clone(), val_ptr.deref(), tag_info.get("Struct")])?;
            pl::set_deref(val_ptr, arg(&r, 0))?;
            err = arg(&r, 1);
        }
        if warn.truthy() {
            et.set("CHECK_WARN", warn);
        }
        return Ok(err);
    }
    let format = tag_info.get("Writable");
    // (if no format specified, value is a simple string)
    if !format.truthy() || pl::str_eq(&format, "string") || pl::str_eq(&format, "lang-alt") {
        // convert value to UTF8 if necessary
        if !pl::str_eq(&hh(et, "OPTIONS").get("Charset"), "UTF8") {
            if re(r"[\x80-\xff]", "").is_match(&val_ptr.deref()) {
                // convert from Charset to UTF-8
                let v = meth(it, et, "Encode", vec![val_ptr.deref(), pl::s("UTF8")])?;
                pl::set_deref(val_ptr, v)?;
            }
        } else {
            // translate invalid XML characters to "."
            let mut v = val_ptr.deref();
            let k = pl::tr(&mut v, "\\0-\\x08\\x0b\\x0c\\x0e-\\x1f", ".", "");
            if k.truthy() || !val_ptr.deref().is_undef() {
                pl::set_deref(val_ptr, v)?;
            }
            // fix any malformed UTF-8 characters
            if xcall(it, "FixUTF8", vec![val_ptr.clone()])?.truthy() && !et.get("WarnBadUTF8").truthy() {
                warn_et(it, et, pl::s("Malformed UTF-8 character(s)"))?;
                et.set("WarnBadUTF8", n(1));
            }
        }
        return Ok(Value::Undef); // success
    }
    let v = val_ptr.deref();
    if pl::str_eq(&format, "rational") || pl::str_eq(&format, "real") {
        // make sure the value is a valid floating point number
        let ok = pl::call(it, "Image::ExifTool::IsFloat", vec![v.clone()])?.truthy()
            // allow 'inf' and 'undef' rational values
            || (pl::str_eq(&format, "rational") && (pl::str_eq(&v, "inf") || pl::str_eq(&v, "undef") || pl::call(it, "Image::ExifTool::IsRational", vec![v.clone()])?.truthy()));
        if !ok {
            return Ok(pl::s("Not a floating point number"));
        }
        if pl::str_eq(&format, "rational") {
            let r = pl::call_list(it, "Image::ExifTool::Rationalize", vec![v, Value::Int(0xffffffff)])?;
            pl::set_deref(val_ptr, pl::join("/", &r))?;
        }
    } else if pl::str_eq(&format, "integer") {
        // make sure the value is integer
        if pl::call(it, "Image::ExifTool::IsInt", vec![v.clone()])?.truthy() {
            // no conversion required (converting to 'int' would remove leading '+')
        } else if pl::call(it, "Image::ExifTool::IsHex", vec![v.clone()])?.truthy() {
            pl::set_deref(val_ptr, hex(&v))?;
        } else {
            return Ok(pl::s("Not an integer"));
        }
    } else if pl::str_eq(&format, "date") {
        let new_date = format_xmp_date_v(v);
        if !new_date.truthy() {
            return Ok(pl::s("Invalid date/time (use YYYY:mm:dd HH:MM:SS[.ss][+/-HH:MM|Z])"));
        }
        pl::set_deref(val_ptr, new_date)?;
    } else if pl::str_eq(&format, "boolean") {
        // (allow lower-case 'true' and 'false' if not setting PrintConv value)
        if !v.truthy() || re(r"false", "i").is_match(&v) || re(r"^no$", "i").is_match(&v) {
            if !v.truthy() || !pl::str_eq(&v, "false") || !conv_type.truthy() || pl::str_eq(conv_type, "PrintConv") {
                pl::set_deref(val_ptr, pl::s("False"))?;
            }
        } else if !pl::str_eq(&v, "true") || !conv_type.truthy() || pl::str_eq(conv_type, "PrintConv") {
            pl::set_deref(val_ptr, pl::s("True"))?;
        }
    } else if pl::str_eq(&format, "1") {
        // this is the entire XMP data block
        if !validate_xmp_v(it, val_ptr, Value::Undef)?.truthy() {
            return Ok(pl::s("Invalid XMP data"));
        }
    } else {
        return Ok(cat!("Unknown XMP format: ", format));
    }
    Ok(Value::Undef) // success!
}

/// Get PropertyPath for specified tagInfo
/// Inputs: 0) tagInfo reference
/// Returns: PropertyPath string
pub fn get_property_path(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(get_property_path_v(it, &arg(&a, 0))?)
}

fn get_property_path_v(it: &mut Interp, tag_info: &Value) -> ER<Value> {
    if !tag_info.get("PropertyPath").truthy() {
        set_property_path_v(it, &tag_info.get("Table"), &tag_info.get("TagID"), &Value::Undef, &Value::Undef, &Value::Undef, Value::Undef)?;
    }
    Ok(tag_info.get("PropertyPath"))
}

/// Set PropertyPath for specified tag (also for associated flattened tags and structure elements)
/// Inputs: 0) tagTable reference, 1) tagID, 2) tagID of parent structure,
///         3) structure definition ref (or undef), 4) property list up to this point (or undef),
///         5) flag set if any containing structure has a TYPE
/// Notes: also generates flattened tags if they don't already exist
pub fn set_property_path(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    set_property_path_v(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2), &arg(&a, 3), &arg(&a, 4), arg(&a, 5))?;
    pl::ret_nothing(want)
}

fn set_property_path_v(it: &mut Interp, tag_table_ptr: &Value, tag_id: &Value, parent_id: &Value, struct_ptr: &Value, prop_list: &Value, mut is_type: Value) -> ER<()> {
    let table = if struct_ptr.truthy() { struct_ptr.clone() } else { tag_table_ptr.clone() };
    let mut tag_info = table.get(tag_id.to_bytes());
    let mut flat_info = Value::Undef;

    if tag_info.ref_type() != "HASH" {
        return Ok(()); // (shouldn't happen)
    }

    if struct_ptr.truthy() {
        let flat_id = pl::concat(parent_id, &pl::ucfirst(tag_id));
        flat_info = tag_table_ptr.get(flat_id.to_bytes());
        if flat_info.truthy() {
            if flat_info.get("PropertyPath").truthy() {
                return Ok(());
            }
        } else if prop_list.len() > 50 {
            return Ok(()); // avoid deep recursion
        } else {
            // flattened tag doesn't exist, so create it now
            // (could happen if we were just writing a structure)
            flat_info = Value::new_hash();
            flat_info.set("Name", pl::ucfirst(&flat_id));
            flat_info.set("Flat", n(1));
            pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_table_ptr.clone(), flat_id, flat_info.clone()])?;
        }
        if struct_ptr.get("TYPE").truthy() {
            is_type = n(1);
        }
    } else {
        // don't override existing main table entry if already set by a Struct
        if tag_info.get("PropertyPath").truthy() {
            return Ok(());
        }
        // use property path from original tagInfo if this is an alternate-language tag
        let src_info = tag_info.get("SrcTagInfo");
        if src_info.truthy() {
            let p = get_property_path_v(it, &src_info)?;
            tag_info.set("PropertyPath", p);
        }
        if tag_info.get("PropertyPath").truthy() {
            return Ok(());
        }
        // set property path for all flattened tags in structure if necessary
        if tag_info.get("RootTagInfo").truthy() {
            let rid = tag_info.get("RootTagInfo").get("TagID");
            set_property_path_v(it, tag_table_ptr, &rid, &Value::Undef, &Value::Undef, &Value::Undef, Value::Undef)?;
            if tag_info.get("PropertyPath").truthy() {
                return Ok(());
            }
            pl::warn(it, &format!("Internal Error: Didn't set path from root for {}\n", tag_id.to_text()))?;
            pl::warn(it, "(Is the Struct NAMESPACE defined?)\n")?;
        }
    }
    let mut ns = tag_info.get("Namespace");
    if !ns.truthy() {
        ns = table.get("NAMESPACE");
    }
    if !ns.truthy() {
        pl::warn(it, &format!("No namespace for {}\n", tag_id.to_text()))?;
        return Ok(());
    }
    let mut prop_l: Vec<Value> = Vec::new();
    if prop_list.truthy() {
        prop_l = prop_list.items();
    }
    prop_l.push(cat!(ns, ":", tag_id));
    let list_type;
    // lang-alt lists are handled specially, signified by Writable='lang-alt'
    if tag_info.get("Writable").truthy() && pl::str_eq(&tag_info.get("Writable"), "lang-alt") {
        list_type = pl::s("Alt");
        // remove language code from property path if it exists
        if tag_info.get("LangCode").truthy() {
            let p = rd(&pb(&[b"-", &tag_info.get("LangCode").to_bytes(), b"$"]), "")?;
            let last = prop_l.last_mut().unwrap();
            crate::modules::xmp::subst(&p, last, false, |_| Ok(Vec::new()))?;
        }
        // handle lists of lang-alt lists (eg. XMP-plus:Custom tags)
        if tag_info.get("List").truthy() && !pl::str_eq(&tag_info.get("List"), "1") {
            prop_l.push(cat!("rdf:", tag_info.get("List")));
            prop_l.push(pl::s("rdf:li 10"));
        }
    } else {
        list_type = tag_info.get("List");
    }
    // add required properties if this is a list
    if list_type.truthy() && !pl::str_eq(&list_type, "1") {
        prop_l.push(cat!("rdf:", list_type));
        prop_l.push(pl::s("rdf:li 10"));
    }
    // set PropertyPath for all flattened tags of this structure if necessary
    let str_table = tag_info.get("Struct");
    if str_table.truthy()
        && !(parent_id.truthy()
            // must test NoSubStruct flag to avoid infinite recursion
            && ((tag_table_ptr.get(parent_id.to_bytes()).truthy() && tag_table_ptr.get(parent_id.to_bytes()).get("NoSubStruct").truthy()) || pl::length(parent_id).to_i64() > 500))
    {
        // avoid deep recursion
        // make sure the structure namespace has been registered
        // (user-defined namespaces may not have been)
        if str_table.get("NAMESPACE").is_ref() {
            xcall(it, "RegisterNamespace", vec![str_table.clone()])?;
        }
        let special_struct = pl::pkg_hash(it, "Image::ExifTool::XMP::specialStruct");
        let prop_ref = Value::new_array(prop_l.clone());
        for tag in keys(&str_table) {
            // ignore special fields and any lang-alt fields we may have added
            if special_struct.get(tag.to_bytes()).truthy() || str_table.get(tag.to_bytes()).get("LangCode").truthy() {
                continue;
            }
            let full_id = if parent_id.truthy() { pl::concat(parent_id, &pl::ucfirst(tag_id)) } else { tag_id.clone() };
            set_property_path_v(it, tag_table_ptr, &tag, &full_id, &str_table, &prop_ref, is_type.clone())?;
        }
    }
    // if this was a structure field and not a normal tag,
    // we set PropertyPath in the corresponding flattened tag
    if struct_ptr.truthy() {
        tag_info = flat_info;
        // set StructType flag if any containing structure has a TYPE
        if is_type.truthy() {
            tag_info.set("StructType", n(1));
        }
    }
    // set property path for tagInfo in main table
    tag_info.set("PropertyPath", pl::join("/", &prop_l));
    Ok(())
}

/// Save XMP property name/value for rewriting
/// Inputs: 0) ExifTool object reference
///         1) reference to array of XMP property path (last is current property)
///         2) property value, 3) optional reference to hash of property attributes
pub fn capture_xmp(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    capture_xmp_v(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2), &arg(&a, 3))?;
    pl::ret_nothing(want)
}

fn capture_xmp_v(it: &mut Interp, et: &Value, prop_list: &Value, val: &Value, attrs: &Value) -> ER<()> {
    if !(!val.is_undef() && prop_list.len() > 2) {
        return Ok(());
    }
    let rdf_desc = lex_s(it, "rdfDesc");
    let desc_re = rd(&pb(&[&rdf_desc.to_bytes(), b"( |$)"]), "")?;
    if re(r"^x:x[ma]pmeta$", "").is_match(&prop_list.at(0)) && pl::str_eq(&prop_list.at(1), "rdf:RDF") && m(&desc_re, &prop_list.at(2)).is_some() {
        // no properties to save yet if this is just the description
        if prop_list.len() <= 3 {
            return Ok(());
        }
        // ignore empty list properties
        if re(r"^rdf:(Bag|Seq|Alt)$", "").is_match(&prop_list.at(-1)) {
            meth(it, et, "Warn", vec![cat!("Ignored empty ", prop_list.at(-1), " list for ", prop_list.at(-2)), n(1)])?;
            return Ok(());
        }
        // save information about this property
        let capture = et.get("XMP_CAPTURE");
        let path = pl::join("/", &prop_list.items()[3..]);
        if !capture.get(path.to_bytes()).is_undef() {
            et.set("XMP_ERROR", cat!("Duplicate XMP property: ", path));
        } else {
            let at = if attrs.truthy() { attrs.clone() } else { Value::new_hash() };
            capture.set(path.to_bytes(), Value::new_array(vec![val.clone(), at]));
        }
    } else if pl::str_eq(&prop_list.at(0), "rdf:RDF") && m(&desc_re, &prop_list.at(1)).is_some() {
        // set flag so we don't write x:xmpmeta element
        et.set("XMP_NO_XMPMETA", n(1));
        // add missing x:xmpmeta element and try again
        prop_list.unshift(pl::s("x:xmpmeta"));
        capture_xmp_v(it, et, prop_list, val, attrs)?;
    } else {
        et.set("XMP_ERROR", cat!("Improperly enclosed XMP property: ", pl::join("/", &prop_list.items())));
    }
    Ok(())
}

/// Save information about resource containing blank node with nodeID
/// Inputs: 0) reference to blank node information hash
///         1) reference to property list
///         2) property value
///         3) [optional] reference to attribute hash
/// Notes: This routine and ProcessBlankInfo() are also used for reading information, but
///        are uncommon so are put in this file to reduce compile time for the common case
pub fn save_blank_info(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (blank_info, prop_list_pt, val, attrs) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));

    let prop_path = pl::join("/", &prop_list_pt.items());
    let ids = re(r" #([^ /]*)", "").all(&prop_path);
    let rdf_desc = lex_s(it, "rdfDesc");
    // split the property path at each nodeID
    for id in ids {
        let p = pb(&[br"^(.*?)/([^/]*) #", &id.to_bytes(), br"((/.*)?)$"]);
        let c = md(&p, "", &prop_path)?;
        let (mut pre, prop, mut post) = match &c {
            Some(c) => (cap(c, 1), cap(c, 2), cap(c, 3)),
            None => (Value::Undef, Value::Undef, Value::Undef),
        };
        if pre.is_undef() {
            pl::warn(it, "internal error parsing nodeID's")?;
            continue;
        }
        // the element with the nodeID should be in the path prefix for subject
        // nodes and the path suffix for object nodes
        if prop.to_bytes() != rdf_desc.to_bytes() {
            if post.truthy() {
                post = cat!("/", prop, post);
            } else {
                pre = cat!(pre, "/", prop);
            }
        }
        let idh = hh(&hh(&blank_info, "Prop"), id.to_bytes());
        hh(&idh, "Pre").set(pre.to_bytes(), n(1));
        if (!post.is_undef() && pl::length(&post).truthy()) || (!val.is_undef() && pl::length(&val).truthy()) {
            // save the property value and attributes for each unique path suffix
            hh(&idh, "Post").set(post.to_bytes(), Value::new_array(vec![val.clone(), attrs.clone(), prop_path.clone()]));
        }
    }
    pl::ret_nothing(want)
}

/// Process blank-node information
/// Inputs: 0) ExifTool object ref, 1) tag table ref,
///         2) blank node information hash ref, 3) flag set for writing
pub fn process_blank_info(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, tag_table_ptr, blank_info, is_writing) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    if !is_writing.truthy() {
        meth(it, &et, "VPrint", vec![n(1), pl::s("  [Elements with nodeID set:]\n")])?;
    }
    let rdf_desc = lex_s(it, "rdfDesc");
    let desc_re = rd(&pb(&[b"/", &rdf_desc.to_bytes(), b"/"]), "")?;
    // handle each nodeID separately
    for id in sorted_keys(&hh(&blank_info, "Prop")) {
        let path = hh(&blank_info, "Prop").get(id.to_bytes());
        // flag all resource names so we can warn later if some are unused
        let unused = Value::new_hash();
        for post in keys(&hh(&path, "Post")) {
            unused.set(post.to_bytes(), n(1));
        }
        // combine property paths for all possible paths through this node
        for pre in sorted_keys(&hh(&path, "Pre")) {
            // there will be no description for the object of a blank node
            if m(&desc_re, &pre).is_none() {
                continue;
            }
            for post in sorted_keys(&hh(&path, "Post")) {
                let prop_list = Value::new_array(split_slash(&pl::concat(&pre, &post)));
                let e = hh(&path, "Post").get(post.to_bytes());
                let (val, attrs) = (e.at(0), e.at(1));
                if is_writing.truthy() {
                    capture_xmp_v(it, &et, &prop_list, &val, &attrs)?;
                } else {
                    it.call_named("Image::ExifTool::XMP::FoundXMP", vec![et.clone(), tag_table_ptr.clone(), prop_list, val], Want::Void)?;
                }
                unused.delete(post.to_bytes());
            }
        }
        // save information from unused properties (if RDF is malformed like f-spot output)
        if !unused.is_empty() {
            if meth(it, &et, "Options", vec![pl::s("Verbose")])?.truthy() {
                warn_et(it, &et, pl::s("An XMP resource is about nothing"))?;
            }
            for post in sorted_keys(&unused) {
                let e = hh(&path, "Post").get(post.to_bytes());
                let (val, attrs, prop_path) = (e.at(0), e.at(1), e.at(2));
                let prop_list = Value::new_array(split_slash(&prop_path));
                if is_writing.truthy() {
                    capture_xmp_v(it, &et, &prop_list, &val, &attrs)?;
                } else {
                    it.call_named("Image::ExifTool::XMP::FoundXMP", vec![et.clone(), tag_table_ptr.clone(), prop_list, val], Want::Void)?;
                }
            }
        }
    }
    pl::ret_nothing(want)
}

/// Convert path to namespace used in file (this is a pain, but the XMP
/// spec only suggests 'preferred' namespace prefixes...)
/// Inputs: 0) ExifTool object reference, 1) property path
/// Returns: conforming property path
pub fn conform_path_to_namespace(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(conform_path_to_namespace_v(it, &arg(&a, 0), &arg(&a, 1))?)
}

fn conform_path_to_namespace_v(it: &mut Interp, et: &Value, path: &Value) -> ER<Value> {
    let mut prop_list = split_slash(path);
    let ns_used = et.get("XMP_NS");
    let nsu = ns_uri(it);
    for prop in prop_list.iter_mut() {
        let (ns, tag) = match re(r"(.+?):(.*)", "").m(prop) {
            Some(c) => (cap(&c, 1), cap(&c, 2)),
            None => (Value::Undef, Value::Undef),
        };
        if ns.is_undef() || ns_used.get(ns.to_bytes()).truthy() {
            continue;
        }
        let uri = nsu.get(ns.to_bytes());
        if !uri.truthy() {
            pl::warn(it, &format!("No URI for namespace prefix {}!\n", ns.to_text()))?;
            continue;
        }
        for ns2 in keys(&ns_used) {
            if ns_used.get(ns2.to_bytes()).to_bytes() != uri.to_bytes() {
                continue;
            }
            // use the existing namespace prefix instead of ours
            *prop = cat!(ns2, ":", tag);
            break;
        }
    }
    Ok(pl::join("/", &prop_list))
}

/// Add necessary rdf:type element when writing structure
/// Inputs: 0) ExifTool ref, 1) tag table ref, 2) capture hash ref, 3) path string
///         4) optional base path (already conformed to namespace) for elements in
///            variable-namespace structures
pub fn add_struct_type(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    add_struct_type_v(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2), &arg(&a, 3), &arg(&a, 4))?;
    pl::ret_nothing(want)
}

fn add_struct_type_v(it: &mut Interp, et: &Value, tag_table_ptr: &Value, capture: &Value, path: &Value, base_path: &Value) -> ER<()> {
    let props = Value::new_array(split_slash(path));
    let done_id = Value::new_hash();
    loop {
        apop(&props);
        if props.is_empty() {
            break;
        }
        let tag_id = get_xmp_tag_id1(it, &props)?;
        if done_id.get(tag_id.to_bytes()).truthy() {
            continue;
        }
        done_id.set(tag_id.to_bytes(), n(1));
        let tag_info = tag_table_ptr.get(tag_id.to_bytes());
        if tag_info.ref_type() != "HASH" {
            break;
        }
        if tag_info.get("Struct").truthy() {
            let type_ = tag_info.get("Struct").get("TYPE");
            if type_.truthy() {
                let mut pat = tag_info.get("PropertyPath");
                if !pat.truthy() {
                    pl::warn(it, "Missing PropertyPath in AddStructType\n")?;
                    break;
                }
                pat = conform_path_to_namespace_v(it, et, &pat)?;
                re(r" \d+", "").subst(&mut pat, true, |_| br" \d+".to_vec());
                let Some(c) = md(&pb(&[b"^(", &pat.to_bytes(), b")"]), "", path)? else {
                    pl::warn(it, "Wrong path in AddStructType\n")?;
                    break;
                };
                let mut p = cat!(cap(&c, 1), "/rdf:type");
                if base_path.truthy() {
                    p = cat!(base_path, "/", p);
                }
                if !capture.get(p.to_bytes()).truthy() {
                    let at = Value::new_hash();
                    at.set("rdf:resource", type_);
                    capture.set(p.to_bytes(), Value::new_array(vec![pl::s(""), at]));
                }
            }
        }
        if !tag_info.get("StructType").truthy() {
            break;
        }
    }
    Ok(())
}

/// Process SphericalVideoXML (see XMP-GSpherical tags documentation)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: SphericalVideoXML data
pub fn process_g_spherical(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    // extract SphericalVideoXML as a block if requested
    if hh(&et, "REQ_TAG_LOOKUP").get("sphericalvideoxml").truthy() {
        let d = pl::substr(&dir_info.get("DataPt").deref(), 16, None);
        meth(it, &et, "FoundTag", vec![pl::s("SphericalVideoXML"), d])?;
    }
    it.call_named("Image::ExifTool::XMP::ProcessXMP", vec![et, dir_info, tag_table_ptr], want)
}

/// Hack to use XMP writer for SphericalVideoXML (see XMP-GSpherical tags documentation)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: SphericalVideoXML data
pub fn write_g_spherical(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    dir_info.set("Compact", n(1));
    let data_pt = dir_info.get("DataPt");
    if data_pt.truthy() && data_pt.deref().truthy() {
        // make it look like XMP for writing
        let mut buff = data_pt.deref();
        re(r"<rdf:SphericalVideo", "").subst(&mut buff, false, |_| b"<?xpacket begin='.*?' id='W5M0MpCehiHzreSzNTczkc9d'?>\n<x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF".to_vec());
        re(r"\s*xmlns:GSpherical", "s").subst(&mut buff, false, |_| b">\n<rdf:Description xmlns:GSpherical".to_vec());
        re(r"</rdf:SphericalVideo>", "").subst(&mut buff, false, |_| b"</rdf:Description>".to_vec());
        buff = pl::concat(&buff, &pl::s("</rdf:RDF></x:xmpmeta><?xpacket end='w'?>"));
        let ds = dir_info.get("DirStart");
        let ds = if ds.truthy() { ds } else { n(0) };
        dir_info.set("DirLen", pl::sub(&pl::length(&buff), &ds));
        dir_info.set("DataPt", pl::sref(buff));
    }
    let mut xmp = write_xmp_v(it, &et, &dir_info, &tag_table_ptr)?;
    if xmp.truthy() {
        // change back to rdf:SphericalVideo structure
        re(r"^<\?xpacket begin.*?<rdf:RDF", "s").subst(&mut xmp, false, |_| b"<rdf:SphericalVideo\n".to_vec());
        re(r">\s*<rdf:Description rdf:about=''\s*", "").subst(&mut xmp, false, |_| b"\n ".to_vec());
        re(r"\s*</rdf:Description>\s*(</rdf:RDF>)", "s").subst(&mut xmp, false, |c| pb(&[b"\n</rdf:SphericalVideo>", &cap(c, 1).to_bytes()]));
        re(r"\s*</rdf:RDF>\s*</x:xmpmeta>.*", "s").subst(&mut xmp, false, |_| Vec::new());
    }
    ret(xmp)
}

/// Utility routine to encode data in base64
/// Inputs: 0) binary data string, 1) flag to avoid inserting newlines
/// Returns:   base64-encoded string
pub fn encode_base64(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    // encode the data in 45-byte chunks
    let chunk_size: i64 = 45;
    let data = arg(&a, 0);
    let len = pl::length(&data).to_i64();
    let mut str_ = Vec::new();
    let mut i = 0;
    while i < len {
        let mut nn = len - i;
        if nn > chunk_size {
            nn = chunk_size;
        }
        // add uuencoded data to output (minus size byte, but including trailing newline)
        let u = pl::pack("u", &[pl::substr(&data, i, Some(nn))])?;
        str_.extend_from_slice(&u.to_bytes()[1..]);
        i += chunk_size;
    }
    let mut str_ = Value::bytes(str_);
    // convert to base64 (remember that "\0" may be encoded as ' ' or '`')
    pl::tr(&mut str_, "` -_", "AA-Za-z0-9+/", "");
    // convert pad characters at the end (remember to account for trailing newline)
    let pad = 3 - (len % 3);
    if pad < 3 {
        str_ = substr_set(&str_, -pad - 1, Some(pad), &Value::bytes(b"=".repeat(pad as usize)));
    }
    if arg(&a, 1).truthy() {
        pl::tr(&mut str_, "\\n", "", "d"); // remove newlines if specified
    }
    ret(str_)
}

/// sort tagInfo hash references by tag name
pub fn by_tag_name(it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (a, b) = (it.get_scalar("Image::ExifTool::XMP::a"), it.get_scalar("Image::ExifTool::XMP::b"));
    ret(ord_value(by_tag_name_cmp(&a, &b)))
}

fn ord_value(o: Ordering) -> Value {
    n(match o {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    })
}

fn by_tag_name_cmp(a: &Value, b: &Value) -> Ordering {
    cmp_str(&a.get("Name"), &b.get("Name"))
}

/// sort alphabetically, but with rdf:type first in the structure
pub fn type_first(it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (a, b) = (it.get_scalar("Image::ExifTool::XMP::a"), it.get_scalar("Image::ExifTool::XMP::b"));
    ret(ord_value(type_first_cmp(&a, &b)))
}

fn type_first_cmp(a: &Value, b: &Value) -> Ordering {
    let t = re(r"rdf:type$", "");
    if t.is_match(a) {
        if !t.is_match(b) {
            return cmp_str(&pl::substr(a, 0, Some(-8)), b);
        }
    } else if t.is_match(b) {
        return cmp_str(a, &pl::substr(b, 0, Some(-8)));
    }
    cmp_str(a, b)
}

/// Limit size of XMP
/// Inputs: 0) ExifTool object ref, 1) XMP data ref (written up to start of $rdfClose),
///         2) max XMP len, 3) rdf:about string, 4) list ref for description start offsets
///         5) start offset of first description recommended for extended XMP
/// Returns: 0) extended XMP ref, 1) GUID and updates $$dataPt (or undef if no extended XMP)
pub fn limit_xmp_size(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    limit_xmp_size_v(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2), &arg(&a, 3), &arg(&a, 4), &arg(&a, 5))
}

fn limit_xmp_size_v(it: &mut Interp, et: &Value, data_pt: &Value, max_len: &Value, about: &Value, start_pt: &Value, ext_start: &Value) -> ER<Vec<Value>> {
    // return straight away if it isn't too big
    if pl::lt(&pl::length(&data_pt.deref()), max_len) {
        return pl::ret_undef();
    }

    let data = data_pt.deref();
    start_pt.push(pl::length(&data)); // add end offset to list
    let mut new_data = pl::substr(&data, 0, Some(start_pt.at(0).to_i64()));
    let mut guid = pl::s(&"0".repeat(32));
    let (sp, nl, rdf_desc) = (lex_s(it, "sp"), lex_s(it, "nl"), lex_s(it, "rdfDesc"));
    let note = ns_uri(it).get("xmpNote");
    // write the required xmpNote:HasExtendedXMP property
    new_data = cat!(new_data, nl, sp, "<", rdf_desc, " rdf:about='", about, "'\n", sp, sp, "xmlns:xmpNote='", note, "'");
    if hh(&hh(et, "OPTIONS"), "Compact").get("Shorthand").truthy() {
        new_data = cat!(new_data, "\n", sp, sp, "xmpNote:HasExtendedXMP='", guid, "'/>\n");
    } else {
        new_data = cat!(new_data, ">", nl, sp, sp, "<xmpNote:HasExtendedXMP>", guid, "</xmpNote:HasExtendedXMP>", nl, sp, "</", rdf_desc, ">\n");
    }

    let desc_size = Value::new_hash();
    // calculate all description block sizes
    for i in 1..start_pt.len() as i64 {
        desc_size.set(start_pt.at(i - 1).to_bytes(), pl::sub(&start_pt.at(i), &start_pt.at(i - 1)));
    }
    apop(start_pt); // remove end offset
    // write the descriptions from smallest to largest, as many in main XMP as possible
    let mut desc_start = start_pt.items();
    desc_start.sort_by(|a, b| crate::perl::ops::num_cmp(&desc_size.get(a.to_bytes()), &desc_size.get(b.to_bytes())).unwrap_or(Ordering::Equal));
    let mut ext_data = pl::concat(&xmp_open_v(it, et)?, &lex_s(it, "rdfOpen"));
    for i in 0..2 {
        for start in &desc_start {
            // write main XMP first (in order of size), then extended XMP afterwards (in order)
            if (i != 0) ^ pl::ge(start, ext_start) {
                continue;
            }
            let ds = desc_size.get(start.to_bytes());
            let piece = pl::substr(&data, start.to_i64(), Some(ds.to_i64()));
            if pl::gt(&pl::add(&pl::length(&new_data), &ds), max_len) {
                ext_data = pl::concat(&ext_data, &piece);
            } else {
                new_data = pl::concat(&new_data, &piece);
            }
        }
    }
    ext_data = cat!(ext_data, lex_s(it, "rdfClose"), lex_s(it, "xmpClose")); // close rdf:RDF and x:xmpmeta
    // calculate GUID from MD5 of extended XMP data
    if pl::eval_block(it, |it| crate::runtime::require(it, "Digest::MD5"))?.is_some() {
        let md5 = pl::call(it, "Digest::MD5::md5", vec![ext_data.clone()])?;
        guid = pl::uc(&pl::unpack1("H*", &md5)?);
        let g = guid.to_bytes();
        re(r"0{32}", "").subst(&mut new_data, false, |_| g.to_vec()); // update GUID in main XMP segment
    }
    meth(it, et, "VerboseValue", vec![pl::s("+ XMP-xmpNote:HasExtendedXMP"), guid.clone()])?;
    pl::set_deref(data_pt, new_data)?; // return main XMP block
    Ok(vec![pl::sref(ext_data), guid]) // return extended XMP and its GUID
}

/// Close out bottom-level property
/// Inputs: 0) current property path list ref, 1) longhand properties at each resource
///         level, 2) shorthand properties at each resource level, 3) resource flag for
///         each property path level (set only if Shorthand is enabled)
pub fn close_property(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    close_property_v(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2), &arg(&a, 3));
    pl::ret_nothing(want)
}

fn close_property_v(it: &mut Interp, cur_prop_list: &Value, long: &Value, short: &Value, res_flag: &Value) {
    let (sp, nl, rdf_desc) = (lex_s(it, "sp"), lex_s(it, "nl"), lex_s(it, "rdfDesc"));
    let mut prop = apop(cur_prop_list);
    re(r" .*", "").subst(&mut prop, false, |_| Vec::new()); // remove list index if it exists
    let ncur = cur_prop_list.len() as i64;
    let pad = rep(&sp, ncur + 1);
    let rf = res_flag.at(ncur);
    if rf.truthy() {
        // close this XMP structure with possible shorthand properties
        if pl::length(&short.at(-1)).truthy() {
            if pl::length(&long.at(-1)).truthy() {
                // require a new Description if both longhand and shorthand properties
                app(long, -2, cat!(">", nl, pad, "<", rdf_desc));
                app(short, -1, cat!(">", nl));
                app(long, -1, cat!(pad, "</", rdf_desc, ">", nl));
            } else {
                // simply close empty property if all shorthand
                app(short, -1, cat!("/>", nl));
            }
        } else {
            // use "parseType" instead of opening a new Description
            app(long, -2, pl::s(" rdf:parseType=\"Resource\""));
            set_i(short, -1, if pl::length(&long.at(-1)).truthy() { cat!(">", nl) } else { cat!("/>", nl) });
        }
        if pl::length(&long.at(-1)).truthy() {
            app(long, -1, cat!(pad, "</", prop, ">", nl));
        }
        app(long, -2, pl::concat(&short.at(-1), &long.at(-1)));
        apop(short);
        apop(long);
    } else if !rf.is_undef() {
        // close this top level Description with possible shorthand values
        if pl::length(&long.at(-1)).truthy() {
            app(long, -2, cat!(short.at(-1), ">", nl, long.at(-1), pad, "</", prop, ">", nl));
        } else {
            app(long, -2, cat!(short.at(-1), "/>", nl)); // empty element (ie. all shorthand)
        }
        set_i(long, -1, pl::s(""));
        set_i(short, -1, pl::s(""));
    } else {
        // close this property (no chance of shorthand)
        app(long, -1, cat!(pad, "</", prop, ">", nl));
        if cur_prop_list.is_empty() {
            // add properties now that this top-level Description is complete
            app(long, -2, cat!(">", nl, long.at(-1)));
            set_i(long, -1, pl::s(""));
        }
    }
    set_len(res_flag, cur_prop_list.len()); // remove expired resource flags
}

/// Write XMP information
/// Inputs: 0) ExifTool ref, 1) source dirInfo ref (with optional WriteGroup),
///         2) [optional] tag table ref
/// Returns: with tag table: new XMP data (may be empty if no XMP data) or undef on error
///          without tag table: 1 on success, 0 if not valid XMP file, -1 on write error
/// Notes: May set dirInfo InPlace flag to rewrite with specified DirLen (=2 to allow larger)
///        May set dirInfo ReadOnly flag to write as read-only XMP ('r' mode and no padding)
///        May set dirInfo Compact flag to force compact (drops 2kB of padding)
///        May set dirInfo MaxDataLen to limit output data length -- this causes ExtendedXMP
///          and ExtendedGUID to be returned in dirInfo if extended XMP was required
pub fn write_xmp(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(write_xmp_v(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2))?)
}

/// `Write($outfile, LIST)`: true on success.
fn write_out(it: &mut Interp, outfile: &Value, items: &[Value]) -> ER<bool> {
    let mut args = vec![outfile.clone()];
    args.extend_from_slice(items);
    Ok(pl::call(it, "Image::ExifTool::Write", args)?.truthy())
}

/// `s/ \d(\d+)$/' '.length($1+1).($1+1)/e`
fn bump_last_index(path: &mut Value) -> bool {
    re(r" \d(\d+)$", "").subst(path, false, |c| {
        let v = pl::add(&cap(c, 1), &n(1));
        pb(&[b" ", &pl::length(&v).to_bytes(), &v.to_bytes()])
    }) > 0
}

pub(crate) fn write_xmp_v(it: &mut Interp, et: &Value, dir_info: &Value, tag_table_ptr: &Value) -> ER<Value> {
    if !et.truthy() {
        return Ok(n(1)); // allow dummy access to autoload this package
    }
    let mut data_pt = dir_info.get("DataPt");
    let capture = Value::new_hash();
    let ns_used = Value::new_hash();
    let mut xmp_err = Value::Undef;
    let mut about = Value::Undef;
    let mut changed = n(0);
    let xmp_file = !tag_table_ptr.truthy(); // this is an XMP data file if no $tagTablePtr
    // prefer XMP over other metadata formats in some types of files
    let preferred = xmp_file || (et.get("PreferredGroup").truthy() && pl::str_eq(&et.get("PreferredGroup"), "XMP"));
    let verbose = hh(et, "OPTIONS").get("Verbose");
    let compact = hh(&hh(et, "OPTIONS"), "Compact").hash_copy(); // (make a copy so we can change settings)
    let mut dir_len = dir_info.get("DirLen");
    if dir_len.is_undef() && data_pt.truthy() {
        dir_len = pl::length(&data_pt.deref());
    }
    let nsu = ns_uri(it);
    //
    // extract existing XMP information into %capture hash
    //
    // define hash in ExifTool object to capture XMP information (also causes
    // CaptureXMP() instead of FoundXMP() to be called from ParseXMPElement())
    //
    // The %capture hash is keyed on the complete property path beginning after
    // rdf:RDF/rdf:Description/.  The values are array references with the
    // following entries: 0) value, 1) attribute hash reference.
    et.set("XMP_CAPTURE", capture.clone());
    et.set("XMP_NS", ns_used.clone());
    et.delete("XMP_NO_XMPMETA");
    et.delete("XMP_NO_XPACKET");
    et.delete("XMP_IS_XML");
    et.delete("XMP_IS_SVG");

    // set current padding characters
    let sp = if compact.get("NoIndent").truthy() { pl::s("") } else { pl::s(" ") };
    let nl = if compact.get("NoNewline").truthy() { pl::s("") } else { pl::s("\n") };
    set_lex_s(it, "sp", sp.clone());
    set_lex_s(it, "nl", nl.clone());

    // get value for new rdf:about
    let mut tag_info = pl::pkg_hash(it, "Image::ExifTool::XMP::rdf").get("about");
    let nvh = hh(et, "NEW_VALUE").get(tag_info.to_bytes());
    if !nvh.is_undef() {
        about = meth(it, et, "GetNewValue", vec![nvh])?;
        if !about.truthy() {
            about = pl::s("");
        }
    }

    if xmp_file || dir_len.truthy() {
        et.delete("XMP_ERROR");
        // extract all existing XMP information (to the XMP_CAPTURE hash)
        let mut success = xcall(it, "ProcessXMP", vec![et.clone(), dir_info.clone(), tag_table_ptr.clone()])?;
        // don't continue if there is nothing to parse or if we had a parsing error
        if !(success.truthy() && !et.get("XMP_ERROR").truthy()) {
            let mut err = et.get("XMP_ERROR");
            if !err.truthy() {
                err = pl::s("Error parsing XMP");
            }
            // may ignore this error only if we were successful
            if xmp_file {
                let raf = dir_info.get("RAF");
                // allow empty XMP data so we can create something from nothing
                if success.truthy() || !meth(it, &raf, "Seek", vec![n(0), n(2)])?.truthy() || meth(it, &raf, "Tell", vec![])?.truthy() {
                    // no error message if not an XMP file
                    if !et.get("XMP_ERROR").truthy() {
                        return Ok(n(0));
                    }
                    if meth(it, et, "Error", vec![err, success.clone()])?.truthy() {
                        et.delete("XMP_CAPTURE");
                        return Ok(n(0));
                    }
                }
            } else {
                if success.truthy() && pl::str_eq(&success, "1") {
                    success = n(2);
                }
                if meth(it, et, "Warn", vec![err, success])?.truthy() {
                    et.delete("XMP_CAPTURE");
                    return Ok(Value::Undef);
                }
            }
        }
        if !about.is_undef() {
            if pl::gt(&verbose, &n(1)) {
                let was_about = et.get("XmpAbout");
                if !was_about.is_undef() {
                    let u = unescape_xml(it, was_about)?;
                    meth(it, et, "VerboseValue", vec![pl::s("- XMP-rdf:About"), u])?;
                }
                meth(it, et, "VerboseValue", vec![pl::s("+ XMP-rdf:About"), about.clone()])?;
            }
            about = escape_xml(it, about)?; // must escape for XML
            inc(&mut changed);
        } else {
            about = et.get("XmpAbout");
            if !about.truthy() {
                about = pl::s("");
            }
        }
        et.delete("XMP_ERROR");

        // call InitWriteDirs to initialize FORCE_WRITE flags if necessary
        if xmp_file && meth(it, et, "GetNewValue", vec![pl::s("ForceWrite")])?.truthy() {
            meth(it, et, "InitWriteDirs", vec![Value::new_hash(), pl::s("XMP")])?;
        }
        // set changed if we are ForceWrite tag was set to "XMP"
        if hh(et, "FORCE_WRITE").get("XMP").truthy() {
            inc(&mut changed);
        }
    } else if !about.is_undef() {
        meth(it, et, "VerboseValue", vec![pl::s("+ XMP-rdf:About"), about.clone()])?;
        about = escape_xml(it, about)?; // must escape for XML
        // (don't increment $changed here because we need another tag to be written)
    } else {
        about = pl::s("");
    }
    //
    // handle writing XMP as a block to XMP file
    //
    if xmp_file {
        tag_info = pl::pkg_hash(it, "Image::ExifTool::Extra").get("XMP");
        if tag_info.truthy() && hh(et, "NEW_VALUE").get(tag_info.to_bytes()).truthy() {
            let mut rtn_val = n(1);
            let nv = hh(et, "NEW_VALUE").get(tag_info.to_bytes());
            let new_val = meth(it, et, "GetNewValue", vec![nv])?;
            if !new_val.is_undef() && pl::length(&new_val).truthy() {
                meth(it, et, "VPrint", vec![n(0), pl::s("  Writing XMP as a block\n")])?;
                et.set("CHANGED", pl::add(&et.get("CHANGED"), &n(1)));
                if !write_out(it, &dir_info.get("OutFile"), &[new_val])? {
                    rtn_val = n(-1);
                }
            }
            et.delete("XMP_CAPTURE");
            return Ok(rtn_val);
        }
    }
    //
    // delete groups in family 1 if requested
    //
    let del_group = hh(et, "DEL_GROUP");
    let et_re = re(r"^http://ns.exiftool.(?:ca|org)/", "");
    if !del_group.is_empty()
        && (keys(&del_group).iter().any(|k| re(r"^XMP-.+$", "").is_match(k))
            // (logic is a bit more complex for group names in exiftool XML files)
            || keys(&ns_used).iter().any(|k| et_re.is_match(&ns_used.get(k.to_bytes()))))
    {
        let del = et.get("DEL_GROUP");
        let std_xlat_ns = pl::pkg_hash(it, "Image::ExifTool::XMP::stdXlatNS");
        for path in keys(&capture) {
            let prop_list = Value::new_array(split_slash(&path)); // get property list
            let r = pl::call_list(it, "Image::ExifTool::XMP::GetXMPTagID", vec![prop_list])?;
            let (tag, mut ns) = (arg(&r, 0), arg(&r, 1));
            // translate namespace if necessary
            if std_xlat_ns.get(ns.to_bytes()).truthy() {
                ns = std_xlat_ns.get(ns.to_bytes());
            }
            let grp;
            // no "XMP-" added to most groups in exiftool RDF/XML output file
            let g = if ns_used.get(ns.to_bytes()).truthy() { re(r"^http://ns.exiftool.(?:ca|org)/(.*?)/(.*?)/", "").m(&ns_used.get(ns.to_bytes())) } else { None };
            if let Some(g) = g {
                let (g0, g1) = (cap(&g, 1), cap(&g, 2));
                if re(r"^\d", "").is_match(&g1) {
                    grp = cat!("XML-", g0);
                    //(all XML-* groups stored as uppercase DEL_GROUP key)
                    let ucg = pl::uc(&grp);
                    if !(del.get(ucg.to_bytes()).truthy() || (del.get("XML-*").truthy() && !del.get(cat!("-", ucg).to_bytes()).truthy())) {
                        continue;
                    }
                } else {
                    grp = g1.clone();
                    if !(del.get(grp.to_bytes()).truthy() || (del.get(g0.to_bytes()).truthy() && !del.get(cat!("-", grp).to_bytes()).truthy())) {
                        continue;
                    }
                }
            } else {
                grp = cat!("XMP-", ns);
                let ucg = pl::uc(&grp);
                if !(del.get(ucg.to_bytes()).truthy() || (del.get("XMP-*").truthy() && !del.get(cat!("-", ucg).to_bytes()).truthy())) {
                    continue;
                }
            }
            meth(it, et, "VerboseValue", vec![cat!("- ", grp, ":", tag), capture.get(path.to_bytes()).at(0)])?;
            capture.delete(path.to_bytes());
            inc(&mut changed);
        }
    }
    // delete HasExtendedXMP tag (we create it as needed)
    let has_ext_tag = "xmpNote:HasExtendedXMP";
    if capture.get(has_ext_tag).truthy() {
        meth(it, et, "VerboseValue", vec![cat!("- XMP-", has_ext_tag), capture.get(has_ext_tag).at(0)])?;
        capture.delete(has_ext_tag);
    }
    // set $xmpOpen now to to handle xmptk tag first
    let xmp_open = if et.get("XMP_NO_XMPMETA").truthy() { pl::s("") } else { xmp_open_v(it, et)? };
    //
    // add, delete or change information as specified
    //
    // get hash of all information we want to change
    // (sorted by tag name so alternate languages come last, but with structures
    // first so flattened tags may be used to override individual structure elements)
    let mut tag_info_list: Vec<Value> = Vec::new();
    let mut struct_list: Vec<Value> = Vec::new();
    let mut del_lang_path = Value::Undef;
    let del_lang_paths = Value::new_hash();
    let del_all_lang = Value::new_hash();
    let mut first_new_path = Value::Undef;
    let mut lang_tags: Vec<Value> = Vec::new();
    let write_group = dir_info.get("WriteGroup");
    let mut list = pl::method_list(it, et, "GetNewTagInfoList", vec![])?;
    list.sort_by(by_tag_name_cmp);
    for ti in list {
        if !pl::str_eq(&meth(it, et, "GetGroup", vec![ti.clone(), n(0)])?, "XMP") {
            continue;
        }
        if pl::str_eq(&ti.get("Name"), "XMP") {
            continue; // (ignore full XMP block if we didn't write it already)
        }
        if write_group.truthy() && write_group.to_bytes() != hh(et, "NEW_VALUE").get(ti.to_bytes()).get("WriteGroup").to_bytes() {
            continue;
        }
        if ti.get("LangCode").truthy() {
            lang_tags.push(ti);
        } else if ti.get("Struct").truthy() {
            struct_list.push(ti);
        } else {
            tag_info_list.push(ti);
        }
    }
    if !lang_tags.is_empty() {
        // keep original order in which lang-alt entries were added
        let nvs = hh(et, "NEW_VALUE");
        lang_tags.sort_by(|a, b| crate::perl::ops::num_cmp(&nvs.get(a.to_bytes()).get("Order"), &nvs.get(b.to_bytes()).get("Order")).unwrap_or(Ordering::Equal));
        for ti in lang_tags {
            if ti.get("Struct").truthy() {
                struct_list.push(ti);
            } else {
                tag_info_list.push(ti);
            }
        }
    }
    let mut all = struct_list;
    all.extend(tag_info_list);
    'tags: for tag_info in all {
        let mut del_paths: Vec<Value> = Vec::new(); // list of deleted paths
        let tag = tag_info.get("TagID");
        let mut path = get_property_path_v(it, &tag_info)?;
        if !path.truthy() {
            warn_et(it, et, cat!("Can't write XMP:", tag, " (namespace unknown)"))?;
            continue;
        }
        // skip tags that were handled specially
        if pl::str_eq(&path, "rdf:about") || pl::str_eq(&path, "x:xmptk") {
            inc(&mut changed);
            continue;
        }
        let is_struct = tag_info.get("Struct");
        // change our property path namespace prefixes to conform
        // to the ones used in this file
        path = conform_path_to_namespace_v(it, et, &path)?;
        // find existing property
        let mut cap_ = capture.get(path.to_bytes());
        // MicrosoftPhoto screws up the case of some tags, and some other software,
        // including Adobe software, has been known to write the wrong list type or
        // not properly enclose properties in a list, so we check for this
        if !cap_.truthy() {
            // find and fix all incorrect property names if this is a structure or a flattened tag
            let mut fix_info: Vec<Value> = Vec::new();
            if is_struct.truthy() || !tag_info.get("Flat").is_undef() {
                // get tagInfo for all containing (possibly nested) structures
                let props = Value::new_array(split_slash(&path));
                let tbl = tag_info.get("Table");
                while !props.is_empty() {
                    let id = get_xmp_tag_id1(it, &props)?;
                    let info = tbl.get(id.to_bytes());
                    if info.ref_type() == "HASH" && info.get("Struct").truthy() && (fix_info.is_empty() || !fix_info[0].same_ref(&info)) {
                        fix_info.insert(0, info);
                    }
                    apop(&props);
                }
                if fix_info.is_empty() {
                    warn_et(it, et, cat!("Error finding parent structure for ", tag_info.get("Name")))?;
                }
            }
            // fix property path for this tag (last in the @fixInfo list)
            if !(!fix_info.is_empty() && is_struct.truthy()) {
                fix_info.push(tag_info.clone());
            }
            // start from outermost containing structure, fixing incorrect list types, etc,
            // finally fixing the actual tag properties after all containing structures
            let mut err = false;
            while !fix_info.is_empty() {
                let fix_info1 = fix_info.remove(0);
                let pp = get_property_path_v(it, &fix_info1)?;
                let fix_path = conform_path_to_namespace_v(it, et, &pp)?;
                let mut regex = Value::bytes(quotemeta(&fix_path.to_bytes()));
                re(r" \d+", "").subst(&mut regex, true, |_| br" \d+".to_vec()); // match any list index
                let mut ok = regex.clone();
                let mut ok2 = Value::Undef;
                // check for incorrect list types
                if re(r"\\/rdf\\:(Bag|Seq|Alt)\\/", "").subst(&mut regex, true, |_| b"/rdf:(Bag|Seq|Alt)/".to_vec()) > 0 {
                    // also look for missing bottom-level list
                    if re(r"/rdf:\(Bag\|Seq\|Alt\)/rdf\\:li\\ \\d\+$", "").subst(&mut regex, false, |_| Vec::new()) > 0 && fix_info.is_empty() {
                        regex = pl::concat(&regex, &pl::s("(/.*)?"));
                    }
                } else if fix_info.is_empty() {
                    ok2 = regex.clone();
                    // check for properties in lists that shouldn't be (ref forum4325)
                    regex = pl::concat(&regex, &pl::s(r"(/rdf:(Bag|Seq|Alt)/rdf:li \d+)?"));
                }
                if !fix_info.is_empty() {
                    regex = pl::concat(&regex, &pl::s("(/.*)?"));
                    ok = pl::concat(&ok, &pl::s("(/.*)?"));
                }
                let rx = rd(&pb(&[b"^", &regex.to_bytes(), b"$"]), "i")?;
                let mut matches: Vec<Value> = keys(&capture).into_iter().filter(|k| m(&rx, k).is_some()).collect();
                matches.sort_by(cmp_str);
                if matches.is_empty() {
                    break;
                }
                if md(&pb(&[b"^", &ok.to_bytes(), b"$"]), "", &matches[0])?.is_some() {
                    if fix_info.is_empty() {
                        path = matches[0].clone();
                        cap_ = capture.get(path.to_bytes());
                    }
                    continue;
                }
                // needs fixing...
                let fix_props = split_slash(&fix_path);
                let mut fixed_list: Vec<Value> = Vec::new();
                let fixed_h = Value::new_hash();
                for mtch in &matches {
                    let mut match_props = split_slash(mtch);
                    // remove superfluous list properties if necessary
                    if ok2.truthy() && match_props.len() > fix_props.len() {
                        match_props.truncate(fix_props.len());
                    }
                    for i in 0..fix_props.len() {
                        if i >= match_props.len() || match_props[i].is_undef() {
                            if i >= match_props.len() {
                                match_props.resize(i + 1, Value::Undef);
                            }
                            match_props[i] = fix_props[i].clone();
                            continue;
                        }
                        if re(r" \d+$", "").is_match(&match_props[i]) || match_props[i].to_bytes() == fix_props[i].to_bytes() {
                            continue;
                        }
                        match_props[i] = fix_props[i].clone();
                    }
                    let fixed = pl::join("/", &match_props);
                    if fixed_h.get(fixed.to_bytes()).truthy() || (capture.get(fixed.to_bytes()).truthy() && mtch.to_bytes() != fixed.to_bytes()) {
                        err = true;
                    }
                    fixed_h.set(fixed.to_bytes(), n(1));
                    fixed_list.push(fixed);
                }
                let tg = cat!(meth(it, et, "GetGroup", vec![fix_info1.clone(), n(1)])?, ":", fix_info1.get("Name"));
                let wrn = if pl::lc(&fixed_list[0]).to_bytes() == pl::lc(&matches[0]).to_bytes() { "tag ID case" } else { "list type" };
                if err {
                    warn_et(it, et, cat!("Incorrect ", wrn, " for existing ", tg, " (not changed)"))?;
                } else {
                    // fix the incorrect property paths for all values of this tag
                    let mut did_fix = false;
                    let mut mq = matches.clone();
                    for fixed in &fixed_list {
                        let mtch = ashift(&mut mq);
                        if fixed.to_bytes() == mtch.to_bytes() {
                            continue;
                        }
                        capture.set(fixed.to_bytes(), capture.get(mtch.to_bytes()));
                        capture.delete(mtch.to_bytes());
                        // remove xml:lang attribute from incorrect lang-alt list if necessary
                        if ok2.truthy() && md(&pb(&[b"^", &ok2.to_bytes(), b"$"]), "", &mtch)?.is_none() {
                            let e = capture.get(fixed.to_bytes());
                            let at = e.at(1);
                            if at.is_undef() {
                                e.set_at(1, Value::new_hash());
                            }
                            e.at(1).delete("xml:lang");
                        }
                        did_fix = true;
                    }
                    if fix_info.is_empty() {
                        cap_ = capture.get(path.to_bytes());
                        if !cap_.truthy() {
                            cap_ = capture.get(fixed_list[0].to_bytes());
                        }
                    }
                    if did_fix {
                        meth(it, et, "Warn", vec![cat!("Fixed incorrect ", wrn, " for ", tg), n(1)])?;
                        inc(&mut changed);
                    }
                }
            }
        }
        let nv_hash = meth(it, et, "GetNewValueHash", vec![tag_info.clone()])?;
        let mut overwrite = meth(it, et, "IsOverwriting", vec![nv_hash.clone()])?;
        let mut writable = tag_info.get("Writable");
        if !writable.truthy() {
            writable = pl::s("");
        }
        let attrs = Value::new_hash();
        let (mut deleted, mut added) = (Value::Undef, Value::Undef);
        let mut new_lang = Value::Undef;
        let mut path_pos: usize = 0; // pos($path) after the last m//g on it
        let is_lang_alt = pl::str_eq(&writable, "lang-alt");
        // set up variables to save/restore paths of deleted lang-alt tags
        if is_lang_alt {
            let lc_ = tag_info.get("LangCode");
            new_lang = pl::lc(&if lc_.truthy() { lc_ } else { pl::s("x-default") });
            if del_lang_path.truthy() && del_lang_path.to_bytes() == path.to_bytes() {
                // restore paths of deleted entries for this language
                if del_lang_paths.get(new_lang.to_bytes()).truthy() {
                    del_paths = del_lang_paths.get(new_lang.to_bytes()).items();
                }
            } else {
                crate::modules::xmp::hclear(&del_lang_paths);
                del_lang_path = path.clone(); // base path for deleted lang-alt tags
                crate::modules::xmp::hclear(&del_all_lang);
                first_new_path = Value::Undef; // reset first path for new lang-alt tag
            }
            if !del_all_lang.is_empty() {
                // add missing paths to delete list for entries where all languages were deleted
                let mut re_sort = false;
                for prefix in keys(&del_all_lang) {
                    let prx = rd(&pb(&[b"^", &prefix.to_bytes()]), "")?;
                    if del_paths.iter().any(|p| m(&prx, p).is_some()) {
                        continue;
                    }
                    del_paths.push(cat!(prefix, "10"));
                    re_sort = true;
                }
                if re_sort {
                    del_paths.sort_by(cmp_str);
                }
            }
        }
        // delete existing entry if necessary
        if is_struct.truthy() {
            // delete all structure (or pseudo-structure) elements
            crate::modules::xmp::require(it, "Image/ExifTool/XMPStruct.pl")?;
            let path_ref = pl::sref(path.clone());
            let changed_ref = pl::sref(changed.clone());
            let r = pl::call_list(it, "Image::ExifTool::XMP::DeleteStruct", vec![et.clone(), capture.clone(), path_ref.clone(), nv_hash.clone(), changed_ref.clone()])?;
            path = path_ref.deref();
            changed = changed_ref.deref();
            deleted = arg(&r, 0);
            added = arg(&r, 1);
            let existed = arg(&r, 2);
            // don't add if it didn't exist and not IsCreating and Avoid
            if !existed.truthy() && !nv_hash.get("IsCreating").truthy() && tag_info.get("Avoid").truthy() {
                added = Value::Undef;
            }
            if !(deleted.truthy() || added.truthy() || meth(it, et, "IsOverwriting", vec![nv_hash.clone()])?.truthy()) {
                continue;
            }
            if existed.truthy() && nv_hash.get("CreateOnly").truthy() {
                continue;
            }
        } else if cap_.truthy() {
            if nv_hash.get("CreateOnly").truthy() {
                continue; // (necessary for List-type tags)
            }
            // take attributes from old values if they exist
            hash_assign(&attrs, &cap_.at(1));
            if overwrite.truthy() {
                let (mut old_lang, mut del_lang, mut add_lang) = (Value::Undef, false, Value::Undef);
                let mut matching_paths: Vec<Value> = Vec::new();
                let mut lang_path_pat = Value::Undef;
                let langs_here = Value::new_hash();
                // check to see if this is an indexed list item
                if re(r" ", "").is_match(&path) {
                    let mut pp = path.clone();
                    re(r" \d+", "").subst(&mut pp, true, |_| br" \d+".to_vec());
                    let prx = rd(&pb(&[b"^", &pp.to_bytes(), b"$"]), "")?;
                    matching_paths = keys(&capture).into_iter().filter(|k| m(&prx, k).is_some()).collect();
                    matching_paths.sort_by(cmp_str);
                } else {
                    matching_paths.push(path.clone());
                }
                let old_overwrite = overwrite.clone();
                for mpath in &matching_paths {
                    let e = capture.get(mpath.to_bytes());
                    let (mut val, attrs_old) = (e.at(0), e.at(1));
                    if is_lang_alt {
                        // get original language code (lc for comparisons)
                        let l = attrs_old.get("xml:lang");
                        old_lang = pl::lc(&if l.truthy() { l } else { pl::s("x-default") });
                        // revert to original overwrite flag if this is in a different structure
                        if !lang_path_pat.truthy() || md(&pb(&[b"^", &lang_path_pat.to_bytes(), b"$"]), "", mpath)?.is_none() {
                            overwrite = old_overwrite.clone();
                            lang_path_pat = mpath.clone();
                            re(r"\d+$", "").subst(&mut lang_path_pat, false, |_| br"\d+".to_vec());
                        }
                        // remember languages in this lang-alt list
                        hh(&langs_here, lang_path_pat.to_bytes()).set(old_lang.to_bytes(), n(1));
                        if add_lang.is_undef() {
                            // add to lang-alt list by default if creating this tag from scratch
                            add_lang = n(if nv_hash.get("IsCreating").truthy() { 1 } else { 0 });
                        }
                        if pl::lt(&overwrite, &n(0)) {
                            if old_lang.to_bytes() != new_lang.to_bytes() {
                                continue;
                            }
                            // only add new tag if we are overwriting this one
                            // (note: this won't match if original XML contains CDATA!)
                            let u = unescape_xml(it, val.clone())?;
                            add_lang = meth(it, et, "IsOverwriting", vec![nv_hash.clone(), u])?;
                            if !add_lang.truthy() {
                                continue;
                            }
                        }
                        // delete all if deleting "x-default" and writing with no LangCode
                        // (XMP spec requires x-default language exist and be first in list)
                        if pl::str_eq(&old_lang, "x-default") && !tag_info.get("LangCode").truthy() {
                            del_lang = true; // delete all languages
                            overwrite = n(1); // force overwrite
                        } else if tag_info.get("LangCode").truthy() && !del_lang {
                            // only overwrite specified language
                            if pl::lc(&tag_info.get("LangCode")).to_bytes() != old_lang.to_bytes() {
                                continue;
                            }
                        }
                    } else if pl::lt(&overwrite, &n(0)) {
                        // only overwrite specific values
                        if nv_hash.get("Shift").truthy() {
                            // values to be shifted are checked (hence re-formatted) late,
                            // so we must un-format the to-be-shifted value for IsOverwriting()
                            let fmt = tag_info.get("Writable");
                            if pl::str_eq(&fmt, "rational") {
                                let mut args = [val.clone()];
                                call_mut(it, "Image::ExifTool::XMP::ConvertRational", &mut args, Want::Scalar)?;
                                val = args[0].clone();
                            } else if pl::str_eq(&fmt, "date") {
                                val = xcall(it, "ConvertXMPDate", vec![val])?;
                            }
                        }
                        // (note: this won't match if original XML contains CDATA!)
                        let u = unescape_xml(it, val.clone())?;
                        if !meth(it, et, "IsOverwriting", vec![nv_hash.clone(), u])?.truthy() {
                            continue;
                        }
                    }
                    if pl::gt(&verbose, &n(1)) {
                        let grp = meth(it, et, "GetGroup", vec![tag_info.clone(), n(1)])?;
                        let mut tag_name = tag_info.get("Name");
                        if tag_info.get("LangCode").truthy() {
                            let p = rd(&pb(&[b"-", &tag_info.get("LangCode").to_bytes(), b"$"]), "")?;
                            crate::modules::xmp::subst(&p, &mut tag_name, false, |_| Ok(Vec::new()))?;
                        }
                        if attrs_old.get("xml:lang").truthy() {
                            tag_name = cat!(tag_name, "-", attrs_old.get("xml:lang"));
                        }
                        meth(it, et, "VerboseValue", vec![cat!("- ", grp, ":", tag_name), val.clone()])?;
                    }
                    // save attributes and path from first deleted property
                    // so we can replace it exactly
                    if del_paths.is_empty() {
                        hash_assign(&attrs, &attrs_old);
                    }
                    if is_lang_alt {
                        hh(&langs_here, lang_path_pat.to_bytes()).set(old_lang.to_bytes(), n(0)); // (lang was deleted)
                    }
                    // save deleted paths so we can replace the same elements
                    // (separately for each language of a lang-alt list)
                    if !is_lang_alt || old_lang.to_bytes() == new_lang.to_bytes() {
                        del_paths.push(mpath.clone());
                    } else {
                        if !del_lang_paths.get(old_lang.to_bytes()).truthy() {
                            del_lang_paths.set(old_lang.to_bytes(), Value::new_array(Vec::new()));
                        }
                        del_lang_paths.get(old_lang.to_bytes()).push(mpath.clone());
                    }
                    // keep track of paths where we deleted all languages of a lang-alt tag
                    if del_lang {
                        let mut p = mpath.clone();
                        re(r"\d+$", "").subst(&mut p, false, |_| Vec::new());
                        del_all_lang.set(p.to_bytes(), n(1));
                    }
                    // delete this tag
                    capture.delete(mpath.to_bytes());
                    inc(&mut changed);
                    // delete rdf:type tag if it is the only thing left in this structure
                    if let Some(c) = re(r"^(.*)/", "").m(mpath) {
                        let pp = cap(&c, 1);
                        if capture.get(cat!(pp, "/rdf:type").to_bytes()).truthy() {
                            let prx = rd(&pb(&[b"^", &quotemeta(&pp.to_bytes()), br"/[^/]+"]), "")?;
                            let count = keys(&capture).iter().filter(|k| m(&prx, k).is_some()).count();
                            if count == 1 {
                                capture.delete(cat!(pp, "/rdf:type").to_bytes());
                            }
                        }
                    }
                }
                if !(!del_paths.is_empty() || tag_info.get("List").truthy() || add_lang.truthy()) {
                    continue;
                }
                if !del_paths.is_empty() {
                    path = ashift(&mut del_paths);
                    // make sure new path is unique
                    while capture.get(path.to_bytes()).truthy() {
                        if !bump_last_index(&mut path) {
                            break;
                        }
                    }
                    deleted = n(1);
                } else {
                    // don't change tag if we couldn't delete old copy
                    // unless this is a list or an lang-alt tag
                    if !(tag_info.get("List").truthy() || old_lang.truthy()) {
                        continue;
                    }
                    // avoid adding duplicate entry to lang-alt in a list
                    if is_lang_alt && !langs_here.is_empty() {
                        for k in sorted_keys(&langs_here) {
                            if md(&pb(&[b"^", &k.to_bytes(), b"$"]), "", &path)?.is_none() {
                                continue;
                            }
                            if !langs_here.get(k.to_bytes()).get(new_lang.to_bytes()).truthy() {
                                break;
                            }
                            let Some(c) = re(r"(.* )\d(\d+)(.*? \d+)$", "").m(&path) else {
                                meth(it, et, "Error", vec![pl::s("Internal error writing lang-alt list")])?;
                                break;
                            };
                            let nxt = pl::add(&cap(&c, 2), &n(1));
                            path = cat!(cap(&c, 1), pl::length(&nxt), nxt, cap(&c, 3)); // step to next index
                        }
                    }
                    // (match last index to put in same lang-alt list for Bag of lang-alt items)
                    let pb_ = path.to_bytes();
                    let Some(sp_) = m_at(&crate::modules::xmp::rx(r".* (\d+)", ""), &pb_, 0) else {
                        pl::warn(it, "Internal error: no list index!\n")?;
                        continue 'tags;
                    };
                    path_pos = end_of(&sp_);
                    let (s1, e1) = sp_[1].unwrap();
                    added = Value::bytes(&pb_[s1..e1]);
                }
            } else {
                // we are never overwriting, so we must be adding to a list
                // match the last index unless this is a list of lang-alt lists
                let mut pat = r".* (\d+)";
                if is_lang_alt {
                    if first_new_path.truthy() {
                        path = first_new_path.clone();
                        overwrite = n(1); // necessary to put x-default entry first below
                    } else {
                        pat = r".* (\d+)(.*? \d+)";
                    }
                }
                let pb_ = path.to_bytes();
                if let Some(sp_) = m_at(&crate::modules::xmp::rx(pat, ""), &pb_, 0) {
                    let (s1, e1) = sp_[1].unwrap();
                    added = Value::bytes(&pb_[s1..e1]);
                    path_pos = end_of(&sp_);
                    // set position to end of matching index number
                    if let Some(Some((s2, e2))) = sp_.get(2) {
                        if Value::bytes(&pb_[*s2..*e2]).truthy() {
                            path_pos -= e2 - s2;
                        }
                    }
                }
            }
            if !added.is_undef() {
                let mut len = pl::length(&added).to_i64();
                let pos = path_pos as i64 - len;
                let mut nxt = pl::add(&pl::substr(&added, 1, None), &n(1));
                // always insert x-default lang-alt entry first (as per XMP spec)
                // (need to test $overwrite because this will be a new lang-alt entry otherwise)
                if overwrite.truthy() && is_lang_alt && (!tag_info.get("LangCode").truthy() || pl::str_eq(&tag_info.get("LangCode"), "x-default")) {
                    let mut save_cap = capture.get(path.to_bytes());
                    while save_cap.truthy() {
                        let p = substr_set(&path, pos, Some(len), &pl::concat(&pl::length(&nxt), &nxt));
                        // increment index in the path of the existing item
                        let next_cap = capture.get(p.to_bytes());
                        capture.set(p.to_bytes(), save_cap);
                        if !next_cap.truthy() {
                            break;
                        }
                        save_cap = next_cap;
                        inc(&mut nxt);
                    }
                } else {
                    // add to end of list
                    while capture.get(path.to_bytes()).truthy() {
                        let try_ = pl::concat(&pl::length(&nxt), &nxt);
                        path = substr_set(&path, pos, Some(len), &try_);
                        len = pl::length(&try_).to_i64();
                        inc(&mut nxt);
                    }
                }
            }
        }
        // check to see if we want to create this tag
        // (create non-avoided tags in XMP data files by default)
        let is_creating = nv_hash.get("IsCreating").truthy() || ((is_struct.truthy() || (preferred && nv_hash.get("Shift").is_undef())) && !tag_info.get("Avoid").truthy() && !nv_hash.get("EditOnly").truthy());

        // don't add new values unless...
        // ...tag existed before and was deleted, or we added it to a list
        if !(deleted.truthy() || !added.is_undef() ||
            // ...tag didn't exist before and we are creating it
            (!cap_.truthy() && is_creating))
        {
            continue;
        }

        // get list of new values (all done if no new values specified)
        let mut new_values = pl::method_list(it, et, "GetNewValue", vec![nv_hash.clone()])?;
        if new_values.is_empty() {
            continue;
        }

        // set language attribute for lang-alt lists
        if is_lang_alt {
            let lc_ = tag_info.get("LangCode");
            attrs.set("xml:lang", if lc_.truthy() { lc_ } else { pl::s("x-default") });
            if !added.is_undef() {
                first_new_path = path.clone(); // save path of first lang-alt tag added
            }
        }
        // add new value(s) to %capture hash
        let mut sub_idx = Value::Undef;
        loop {
            let mut new_value = ashift(&mut new_values);
            if is_struct.truthy() {
                if pl::call(it, "Image::ExifTool::XMP::AddNewStruct", vec![et.clone(), tag_info.clone(), capture.clone(), path.clone(), new_value, tag_info.get("Struct")])?.truthy() {
                    inc(&mut changed);
                }
            } else {
                new_value = escape_xml(it, new_value)?;
                #[allow(clippy::never_loop)]
                loop {
                    // (a cheap 'goto')
                    if tag_info.get("Resource").truthy() {
                        // only store as a resource if it doesn't contain any illegal characters
                        if !re(r"[^a-z0-9\:\/\?\#\[\]\@\!\$\&\'\(\)\*\+\,\;\=\.\-\_\~]", "i").is_match(&new_value) {
                            let h = attrs.hash_copy();
                            h.set("rdf:resource", new_value.clone());
                            capture.set(path.to_bytes(), Value::new_array(vec![pl::s(""), h]));
                            break;
                        }
                        let grp = meth(it, et, "GetGroup", vec![tag_info.clone(), n(1)])?;
                        meth(it, et, "Warn", vec![cat!(grp, ":", tag_info.get("Name"), " written as a literal because value is not a valid URI"), n(1)])?;
                        // fall through to write as a string literal
                    }
                    // remove existing value and/or resource attribute if they exist
                    attrs.delete("rdf:value");
                    attrs.delete("rdf:resource");
                    capture.set(path.to_bytes(), Value::new_array(vec![new_value.clone(), attrs.clone()]));
                    break;
                }
                if pl::gt(&verbose, &n(1)) {
                    let grp = meth(it, et, "GetGroup", vec![tag_info.clone(), n(1)])?;
                    meth(it, et, "VerboseValue", vec![cat!("+ ", grp, ":", tag_info.get("Name")), new_value.clone()])?;
                }
                inc(&mut changed);
                // add rdf:type if necessary
                if tag_info.get("StructType").truthy() {
                    add_struct_type_v(it, et, &tag_info.get("Table"), &capture, &path, &Value::Undef)?;
                }
            }
            if new_values.is_empty() {
                break;
            }
            // match last index except for lang-alt items where we want to put each
            // item in a different lang-alt list (so match the 2nd-last for these)
            let pat = if is_lang_alt { r".* (\d+)(.*? \d+)" } else { r".* (\d+)" };
            let pb_ = path.to_bytes();
            let Some(sp_) = m_at(&crate::modules::xmp::rx(pat, ""), &pb_, 0) else {
                pl::warn(it, &format!("Internal error: no list index for {} ({}) ({})!\n", tag.to_text(), path.to_text(), pat))?;
                continue;
            };
            let (s1, e1) = sp_[1].unwrap();
            let mut idx = Value::bytes(&pb_[s1..e1]);
            let len = (e1 - s1) as i64;
            let c2 = match sp_.get(2) {
                Some(Some((s2, e2))) => Value::bytes(&pb_[*s2..*e2]),
                _ => Value::Undef,
            };
            let pos = end_of(&sp_) as i64 - len - if c2.truthy() { pl::length(&c2).to_i64() } else { 0 };
            // use sub-indices if necessary to store additional values in sequence
            if sub_idx.truthy() {
                idx = pl::substr(&idx, 0, Some(-pl::length(&sub_idx).to_i64())); // remove old sub-index
                sub_idx = pl::add(&pl::substr(&sub_idx, 1, None), &n(1));
                sub_idx = pl::concat(&pl::length(&sub_idx), &sub_idx);
            } else if !del_paths.is_empty() {
                path = ashift(&mut del_paths);
                // make sure new path is unique
                while capture.get(path.to_bytes()).truthy() {
                    if !bump_last_index(&mut path) {
                        break;
                    }
                }
                continue;
            } else {
                sub_idx = pl::s("10");
            }
            path = substr_set(&path, pos, Some(len), &pl::concat(&idx, &sub_idx));
        }
        // make sure any empty structures are deleted
        // (ExifTool shouldn't write these, but other software may)
        if !tag_info.get("Flat").is_undef() {
            let mut p = path.clone();
            while re(r"/[^/]+$", "").subst(&mut p, false, |_| Vec::new()) > 0 {
                if !capture.get(p.to_bytes()).truthy() {
                    continue;
                }
                // it is an error if this property has a value
                if re(r"\S", "").is_match(&capture.get(p.to_bytes()).at(0)) {
                    meth(it, et, "Error", vec![cat!("Improperly structured XMP (", p, ")"), n(1)])?;
                }
                capture.delete(p.to_bytes()); // delete the (hopefully) empty structure
            }
        }
    }
    // remove the ExifTool members we created
    et.delete("XMP_CAPTURE");
    et.delete("XMP_NS");

    let mut max_data_len = dir_info.get("MaxDataLen");
    // get DataPt again because it may have been set by ProcessXMP
    data_pt = dir_info.get("DataPt");

    // return now if we didn't change anything
    if !(changed.truthy() || (max_data_len.truthy() && data_pt.truthy() && !data_pt.deref().is_undef() && pl::gt(&pl::length(&data_pt.deref()), &max_data_len))) {
        if !xmp_file {
            return Ok(Value::Undef); // just rewrite original XMP
        }
        if data_pt.truthy() && !data_pt.deref().is_undef() && !write_out(it, &dir_info.get("OutFile"), &[data_pt.deref()])? {
            return Ok(n(-1));
        }
        return Ok(n(1));
    }
    //
    // write out the new XMP information (serialize it)
    //
    // start writing the XMP data
    let long = Value::new_array(vec![pl::s(""), pl::s("")]);
    let short = Value::new_array(vec![pl::s("")]);
    let res_flag = Value::new_array(Vec::new());
    if et.get("XMP_NO_XPACKET").truthy() {
        // write BOM if flag is set
        if pl::num_eq(&et.get("XMP_NO_XPACKET"), &n(2)) {
            app(&long, -2, Value::bytes(b"\xef\xbb\xbf"));
        }
    } else {
        app(&long, -2, lex_s(it, "pktOpen"));
    }
    if et.get("XMP_IS_XML").truthy() {
        app(&long, -2, lex_s(it, "xmlOpen"));
    }
    app(&long, -2, pl::concat(&xmp_open, &lex_s(it, "rdfOpen")));

    // initialize current property path list
    let cur_prop_list = Value::new_array(Vec::new());
    let mut write_last: Vec<Value> = Vec::new();
    let desc_start = Value::new_array(Vec::new());
    let mut ext_start = Value::Undef;
    let ns_cur = Value::new_hash();
    let rdf_desc = lex_s(it, "rdfDesc");
    let mut path_list = keys(&capture);
    path_list.sort_by(type_first_cmp);
    // order properties to write large values last if we have a MaxDataLen limit
    if max_data_len.truthy() && !path_list.is_empty() {
        let extended_res = lex_h(it, "extendedRes");
        let new_desc_thresh = lex_s(it, "newDescThresh");
        let mut path_tmp: Vec<Value> = Vec::new();
        let (mut last_prop, mut last_ns, mut prop_size) = (pl::s(""), pl::s(""), n(0));
        let mut path_loop = path_list.clone();
        path_loop.push(pl::s("")); // add empty path to end of list for loop
        path_list.clear();
        for path in path_loop {
            let c = re(r"^((\w*)[^/]*)", "").m(&path).unwrap_or_default(); // get path element ($1) and ns ($2)
            let (c1, c2) = (cap(&c, 1), cap(&c, 2));
            if c1.to_bytes() == last_prop.to_bytes() {
                path_tmp.push(path.clone()); // accumulate all paths with same root
            } else {
                // put in list to write last if recommended or values are too large
                if extended_res.get(last_prop.to_bytes()).truthy() || extended_res.get(last_ns.to_bytes()).truthy() || pl::gt(&prop_size, &new_desc_thresh) {
                    write_last.append(&mut path_tmp);
                } else {
                    path_list.append(&mut path_tmp);
                }
                if !path.truthy() {
                    break; // all done if we hit empty path
                }
                path_tmp = vec![path.clone()];
                last_prop = c1;
                last_ns = c2;
                prop_size = n(0);
            }
            prop_size = pl::add(&prop_size, &pl::length(&capture.get(path.to_bytes()).at(0)));
        }
    }

    let debug = lex_s(it, "debug");
    let mut path_list: std::collections::VecDeque<Value> = path_list.into();
    // write out all properties
    loop {
        let ns_new = Value::new_hash();
        let mut new_desc = Value::Undef;
        if path_list.is_empty() {
            if write_last.is_empty() {
                break;
            }
            path_list = std::mem::take(&mut write_last).into();
            new_desc = n(2); // start with a new description for the extended data
        }
        let path = path_list.pop_front().unwrap_or_default();
        let mut prop_list = split_slash(&path); // get property list
        // must open/close rdf:Description too
        prop_list.insert(0, rdf_desc.clone());
        // make sure we have defined all necessary namespaces
        for prop in &prop_list {
            let Some(c) = re(r"(.*):", "").m(prop) else { continue };
            let c1 = cap(&c, 1);
            if pl::str_eq(&c1, "rdf") {
                continue; // rdf namespace already defined
            }
            let mut uri = ns_used.get(c1.to_bytes());
            if !uri.truthy() {
                uri = nsu.get(c1.to_bytes()); // we must have added a namespace
                if !uri.truthy() {
                    // (namespace prefix may be empty if trying to write empty XMP structure, forum12384)
                    if pl::length(&c1).truthy() {
                        let err = cat!("Undefined XMP namespace: ", c1);
                        if !xmp_err.truthy() || err.to_bytes() != xmp_err.to_bytes() {
                            if xmp_file {
                                meth(it, et, "Error", vec![err.clone()])?;
                            } else {
                                warn_et(it, et, err.clone())?;
                            }
                            xmp_err = err;
                        }
                    }
                    continue;
                }
            }
            ns_new.set(c1.to_bytes(), uri);
            // need a new description if any new namespaces
            if !ns_cur.get(c1.to_bytes()).truthy() {
                new_desc = n(1);
            }
        }
        let mut close_to = 0usize;
        if new_desc.truthy() {
            // look forward to see if we will want to also open other namespaces
            // at this level (this is necessary to keep lists and structures from
            // being broken if a property introduces a new namespace; plus it
            // improves formatting)
            for path2 in path_list.iter() {
                let ns2s = re(r"(?:^|/)([^/]+?):", "").all(path2);
                let mut opening = if compact.get("OneDesc").truthy() { 1 } else { 0 };
                for ns2 in &ns2s {
                    if pl::str_eq(ns2, "rdf") {
                        continue;
                    }
                    if ns_new.get(ns2.to_bytes()).truthy() {
                        opening += 1;
                        continue;
                    }
                    if opening == 0 {
                        break;
                    }
                    // get URI for this existing or new namespace
                    let mut uri = ns_used.get(ns2.to_bytes());
                    if !uri.truthy() {
                        uri = nsu.get(ns2.to_bytes());
                    }
                    if !uri.truthy() {
                        break;
                    }
                    ns_new.set(ns2.to_bytes(), uri); // also open this namespace
                }
                if opening == 0 {
                    break;
                }
            }
        } else {
            // find first property where the current path differs from the new path
            let cur = cur_prop_list.items();
            while close_to < cur.len() {
                if close_to >= prop_list.len() {
                    break;
                }
                if prop_list[close_to].to_bytes() != cur[close_to].to_bytes() {
                    break;
                }
                close_to += 1;
            }
        }
        // close out properties down to the common base path
        while cur_prop_list.len() > close_to {
            close_property_v(it, &cur_prop_list, &long, &short, &res_flag);
        }

        // open new description if necessary
        if new_desc.truthy() {
            if pl::num_eq(&new_desc, &n(2)) {
                ext_start = pl::length(&long.at(-2)); // extended data starts after this
            }
            // save rdf:Description start positions so we can reorder them if necessary
            if max_data_len.truthy() {
                desc_start.push(pl::length(&long.at(-2)));
            }
            // open the new description
            let prop = rdf_desc.clone();
            hash_assign(&ns_cur, &ns_new); // save current namespaces
            let ns = sorted_keys(&ns_cur);
            app(&long, -2, cat!(nl, sp, "<", prop, " rdf:about='", about, "'"));
            // generate et:toolkit attribute if this is an exiftool RDF/XML output file
            if et.get("XMP_NO_XMPMETA").truthy() && !ns.is_empty() && et_re.is_match(&ns_cur.get(ns[0].to_bytes())) {
                app(&long, -2, cat!("\n", sp, sp, "xmlns:et='http://ns.exiftool.org/1.0/'", " et:toolkit='Image::ExifTool ", it.get_scalar("Image::ExifTool::VERSION"), "'"));
            }
            for k in &ns {
                app(&long, -2, cat!("\n", sp, sp, "xmlns:", k, "='", ns_cur.get(k.to_bytes()), "'"));
            }
            cur_prop_list.push(prop);
            // set resFlag to 0 to indicate base description when Shorthand enabled
            if compact.get("Shorthand").truthy() {
                res_flag.set_at(0, n(0));
            }
        }
        let e = capture.get(path.to_bytes());
        let (val, attrs) = (e.at(0), e.at(1));
        if debug.truthy() {
            pl::print_to(it, None, &[cat!(path, " = ", val, "\n")])?;
        }
        // open new properties if necessary
        let mut dummy = false;
        let mut nn = cur_prop_list.len();
        while (nn as i64) < prop_list.len() as i64 - 1 {
            let mut prop = prop_list[nn].clone();
            cur_prop_list.push(prop.clone());
            re(r" .*", "").subst(&mut prop, false, |_| Vec::new()); // remove list index if it exists
            // (we may add parseType and shorthand properties later,
            //  so leave off the trailing ">" for now)
            let ind = if compact.get("NoIndent").truthy() { pl::s("") } else { rep(&pl::s(" "), cur_prop_list.len() as i64) };
            app(&long, -1, cat!(ind, "<", prop));
            let next = &prop_list[nn + 1];
            if prop.to_bytes() != rdf_desc.to_bytes() && (!re(r"^rdf:", "").is_match(next) || (pl::str_eq(next, "rdf:type") && nn + 1 == prop_list.len() - 1)) {
                // check for empty structure
                if re(r":~dummy~$", "").is_match(next) {
                    app(&long, -1, cat!(" rdf:parseType='Resource'/>", nl));
                    apop(&cur_prop_list);
                    dummy = true;
                    break;
                }
                if compact.get("Shorthand").truthy() {
                    res_flag.set_at(cur_prop_list.len() - 1, n(1));
                    long.push(pl::s(""));
                    short.push(pl::s(""));
                } else {
                    // use rdf:parseType='Resource' to avoid new 'rdf:Description'
                    app(&long, -1, cat!(" rdf:parseType='Resource'>", nl));
                }
            } else {
                app(&long, -1, cat!(">", nl)); // (will be no shorthand properties)
            }
            nn += 1;
        }
        let mut prop2 = prop_list.pop().unwrap_or_default(); // get new property name
        // add element unless it was a dummy structure field
        if !(dummy || (pl::str_eq(&val, "") && re(r":~dummy~$", "").is_match(&prop2))) {
            re(r" .*", "").subst(&mut prop2, false, |_| Vec::new()); // remove list index if it exists
            let pad = if compact.get("NoIndent").truthy() { pl::s("") } else { rep(&pl::s(" "), cur_prop_list.len() as i64 + 1) };
            // (can't write as shortcut if it has attributes or CDATA)
            if !res_flag.at(cur_prop_list.len() as i64 - 1).is_undef() && attrs.is_empty() && !re(r"<!\[CDATA\[", "").is_match(&val) {
                app(&short, -1, cat!("\n", pad, prop2, "='", val, "'"));
            } else {
                app(&long, -1, cat!(pad, "<", prop2));
                // write out attributes
                for attr in sorted_keys(&attrs) {
                    let attr_val = attrs.get(attr.to_bytes());
                    let quot = if re(r"'", "").is_match(&attr_val) { "\"" } else { "'" };
                    app(&long, -1, cat!(" ", attr, "=", quot, attr_val, quot));
                }
                app(&long, -1, if pl::length(&val).truthy() { cat!(">", val, "</", prop2, ">", nl) } else { cat!("/>", nl) });
            }
        }
    }
    // close out all open properties
    while !cur_prop_list.is_empty() {
        close_property_v(it, &cur_prop_list, &long, &short, &res_flag);
    }

    let mut compact_no_padding = compact.get("NoPadding").truthy();
    // limit XMP length and re-arrange if necessary to fit inside specified size
    if max_data_len.truthy() {
        // adjust maxDataLen to allow room for closing elements
        let closing = pl::add(&pl::add(&pl::length(&lex_s(it, "rdfClose")), &pl::length(&lex_s(it, "xmpClose"))), &pl::length(&lex_s(it, "pktCloseW")));
        max_data_len = pl::sub(&max_data_len, &closing);
        if !ext_start.truthy() {
            ext_start = pl::length(&long.at(-2));
        }
        let lr = pl::sref(long.at(-2));
        let rtn = limit_xmp_size_v(it, et, &lr, &max_data_len, &about, &desc_start, &ext_start)?;
        set_i(&long, -2, lr.deref());
        // return extended XMP information in $dirInfo
        dir_info.set("ExtendedXMP", arg(&rtn, 0));
        dir_info.set("ExtendedGUID", arg(&rtn, 1));
        // compact if necessary to fit
        let num_pad_lines = lex_s(it, "numPadLines");
        if pl::gt(&pl::add(&pl::length(&long.at(-2)), &pl::mul(&n(101), &num_pad_lines)), &max_data_len) {
            compact.set("NoPadding", n(1));
            compact_no_padding = true;
        }
    }
    if dir_info.get("Compact").truthy() {
        compact.set("NoPadding", n(1));
        compact_no_padding = true;
    }
    //
    // close out the XMP, clean up, and return our data
    //
    app(&long, -2, lex_s(it, "rdfClose"));
    if !et.get("XMP_NO_XMPMETA").truthy() {
        app(&long, -2, lex_s(it, "xmpClose"));
    }

    // remove the ExifTool members we created
    et.delete("XMP_CAPTURE");
    et.delete("XMP_NS");
    et.delete("XMP_NO_XMPMETA");

    // (the XMP standard recommends writing 2k-4k of white space before the
    // packet trailer, with a newline every 100 characters)
    if !et.get("XMP_NO_XPACKET").truthy() {
        let pad = pl::s(&(" ".repeat(100) + "\n"));
        let pkt_close_w = lex_s(it, "pktCloseW");
        // get current XMP length without padding
        let mut len = pl::add(&pl::length(&long.at(-2)), &pl::length(&pkt_close_w));
        let in_place = dir_info.get("InPlace");
        if in_place.truthy() && !(pl::num_eq(&in_place, &n(2)) && pl::gt(&len, &dir_len)) {
            // pad to specified DirLen
            if pl::gt(&len, &dir_len) {
                let mut str_ = pl::s("Not enough room to edit XMP in place");
                if !compact.get("Shorthand").truthy() {
                    str_ = pl::concat(&str_, &pl::s(". Try Shorthand feature"));
                }
                warn_et(it, et, str_)?;
                return Ok(Value::Undef);
            }
            let num = pl::int(&pl::div(&pl::sub(&dir_len, &len), &pl::length(&pad))?);
            if num.truthy() {
                app(&long, -2, rep(&pad, num.to_i64()));
                len = pl::add(&len, &pl::mul(&pl::length(&pad), &num));
            }
            if pl::lt(&len, &dir_len) {
                let k = pl::sub(&pl::sub(&dir_len, &len), &n(1)).to_i64();
                app(&long, -2, pl::concat(&rep(&pl::s(" "), k), &pl::s("\n")));
            }
        } else if !compact_no_padding && !xmp_file && !dir_info.get("ReadOnly").truthy() {
            let num_pad_lines = lex_s(it, "numPadLines");
            app(&long, -2, rep(&pad, num_pad_lines.to_i64()));
        }
        app(&long, -2, if dir_info.get("ReadOnly").truthy() { lex_s(it, "pktCloseR") } else { pkt_close_w });
    }
    // return empty data if no properties exist and this is allowed
    if !(!capture.is_empty() || xmp_file || dir_info.get("InPlace").truthy() || dir_info.get("NoDelete").truthy()) {
        set_i(&long, -2, pl::s(""));
    }
    if xmp_err.truthy() {
        return Ok(if xmp_file { n(-1) } else { Value::Undef });
    }
    et.set("CHANGED", pl::add(&et.get("CHANGED"), &changed));
    if pl::gt(&debug, &n(1)) && long.at(-2).truthy() {
        pl::print_to(it, None, &[long.at(-2), pl::s("\n")])?;
    }
    if !xmp_file {
        return Ok(long.at(-2));
    }
    if !write_out(it, &dir_info.get("OutFile"), &[long.at(-2)])? {
        return Ok(n(-1));
    }
    Ok(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::XMP::XMPOpen", xmp_open);
    it.define_native("Image::ExifTool::XMP::ValidateXMP", validate_xmp);
    it.define_native("Image::ExifTool::XMP::ValidateProperty", validate_property);
    it.define_native("Image::ExifTool::XMP::FormatXMPDate", format_xmp_date);
    it.define_native("Image::ExifTool::XMP::CheckXMP", check_xmp);
    it.define_native("Image::ExifTool::XMP::GetPropertyPath", get_property_path);
    it.define_native("Image::ExifTool::XMP::SetPropertyPath", set_property_path);
    it.define_native("Image::ExifTool::XMP::CaptureXMP", capture_xmp);
    it.define_native("Image::ExifTool::XMP::SaveBlankInfo", save_blank_info);
    it.define_native("Image::ExifTool::XMP::ProcessBlankInfo", process_blank_info);
    it.define_native("Image::ExifTool::XMP::ConformPathToNamespace", conform_path_to_namespace);
    it.define_native("Image::ExifTool::XMP::AddStructType", add_struct_type);
    it.define_native("Image::ExifTool::XMP::ProcessGSpherical", process_g_spherical);
    it.define_native("Image::ExifTool::XMP::WriteGSpherical", write_g_spherical);
    it.define_native("Image::ExifTool::XMP::EncodeBase64", encode_base64);
    it.define_native("Image::ExifTool::XMP::ByTagName", by_tag_name);
    it.define_native("Image::ExifTool::XMP::TypeFirst", type_first);
    it.define_native("Image::ExifTool::XMP::LimitXMPSize", limit_xmp_size);
    it.define_native("Image::ExifTool::XMP::CloseProperty", close_property);
    it.define_native("Image::ExifTool::XMP::WriteXMP", write_xmp);
}
