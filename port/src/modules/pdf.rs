//! Image::ExifTool::PDF (Image/ExifTool/PDF.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read PDF meta information: the xref tables and object reader, the
//! dictionaries, the stream filters, the decryption (RC4 and AES) and the
//! date conversion. The tag tables stay Perl; WritePDF.pl is a separate file.

use std::collections::BTreeMap;
use std::rc::Rc;

use crate::interp::eval::{die, Cell, ER};
use crate::interp::{Binding, Interp, Want};
use crate::perl::regex::{self as pre, PerlRegex, Spans};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};
use crate::runtime;

const MODULE: &str = "Image::ExifTool::PDF";

// ---- local helpers ----

/// Where the file lexicals of PDF.pm are kept for the ported subs. The
/// loader files `my` variables under MODULE::<my>, but WritePDF.pl (same
/// package) has its own `my $pdfVer`, which would replace PDF.pm's there
/// once it is loaded; so `register` keeps PDF.pm's own bindings here.
const LEX: &str = "Image::ExifTool::PDF::<my:PDF.pm>::";

/// PDF.pm's file lexicals (`my $cryptInfo` ...), as the Perl subs see them.
const LEX_NAMES: &[(u8, &str)] = &[
    (b'$', "cryptInfo"),
    (b'$', "cryptString"),
    (b'$', "cryptStream"),
    (b'$', "lastOffset"),
    (b'%', "streamObjs"),
    (b'%', "fetched"),
    (b'$', "pdfVer"),
    (b'%', "supportedFilter"),
    (b'%', "closingDelim"),
    (b'$', "cipherMore"),
    (b'%', "pdfWarning"),
];

/// A scalar file lexical of PDF.pm (`$cryptInfo`...).
fn lex(it: &mut Interp, name: &str) -> Cell {
    match it.global(b'$', &format!("{LEX}{name}")) {
        Binding::Scalar(c) => c,
        _ => unreachable!(),
    }
}

fn lget(it: &mut Interp, name: &str) -> Value {
    lex(it, name).borrow().clone()
}

fn lset(it: &mut Interp, name: &str, v: Value) {
    *lex(it, name).borrow_mut() = v;
}

/// A hash file lexical of PDF.pm (`%fetched`...), as a reference.
fn lexh(it: &mut Interp, name: &str) -> Value {
    match it.global(b'%', &format!("{LEX}{name}")) {
        Binding::Hash(h) => Value::HashRef(h),
        _ => unreachable!(),
    }
}

/// `$lastFetched` (a package variable: WritePDF.pl uses it too).
fn last_fetched(it: &mut Interp) -> Value {
    it.get_scalar("Image::ExifTool::PDF::lastFetched")
}

fn set_last_fetched(it: &mut Interp, v: Value) {
    it.set_scalar("Image::ExifTool::PDF::lastFetched", v);
}

/// The first value of a call's result (scalar context).
fn one(r: Vec<Value>) -> Value {
    r.into_iter().next().unwrap_or_default()
}

/// String interpolation: the concatenation of the parts.
fn cat(parts: &[Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

fn s(x: &str) -> Value {
    pl::s(x)
}

/// `$et->Warn(MSG)` (returns Warn's value).
fn warn(it: &mut Interp, et: &Value, msg: Value) -> ER<Value> {
    pl::method(it, et, "Warn", vec![msg])
}

/// A compiled pattern, for the matches that use `pos()` and `\G`.
fn rx(pat: &str, flags: &str) -> Rc<PerlRegex> {
    pre::compile(pat.as_bytes(), flags).unwrap_or_else(|e| panic!("bad pattern /{pat}/: {e}"))
}

/// A match starting at `pos` (where `\G` anchors).
fn m_at(r: &PerlRegex, subject: &[u8], pos: usize) -> Option<Spans> {
    r.match_at(subject, pos).ok().flatten()
}

/// Capture `i` of a match as a value (undef when it did not take part).
fn sp(subject: &[u8], spans: &Spans, i: usize) -> Value {
    match spans.get(i).copied().flatten() {
        Some((a, b)) => Value::bytes(&subject[a..b]),
        None => Value::Undef,
    }
}

/// `$$ref` under `use strict`: dies as perl does on a non-reference.
fn sderef(v: &Value) -> ER<Value> {
    if v.is_scalar_ref() {
        return Ok(v.deref());
    }
    if v.is_undef() {
        return die("Can't use an undefined value as a SCALAR reference");
    }
    if v.is_ref() {
        return die("Not a SCALAR reference");
    }
    die(format!("Can't use string (\"{}\") as a SCALAR ref while \"strict refs\" in use", trunc32(v)))
}

/// `@$ref` under `use strict`.
fn aderef(v: &Value) -> ER<Vec<Value>> {
    if v.is_array() {
        return Ok(v.items());
    }
    if v.is_undef() {
        return Ok(Vec::new());
    }
    if v.is_ref() {
        return die("Not an ARRAY reference");
    }
    die(format!("Can't use string (\"{}\") as an ARRAY ref while \"strict refs\" in use", trunc32(v)))
}

fn trunc32(v: &Value) -> String {
    let t = v.to_text();
    if t.chars().count() > 32 {
        format!("{}\"...", t.chars().take(32).collect::<String>()).trim_end_matches('"').to_string()
    } else {
        t
    }
}

/// `$raf->ReadLine($buff)`
fn read_line(it: &mut Interp, raf: &Value, buff: &mut Value) -> ER<Value> {
    let mut a = [buff.clone()];
    let r = pl::method_mut(it, raf, "ReadLine", &mut a, Want::Scalar)?;
    *buff = a[0].clone();
    Ok(one(r))
}

/// `$raf->Read($buff, $len)`
fn raf_read(it: &mut Interp, raf: &Value, buff: &mut Value, len: Value) -> ER<Value> {
    let mut a = [buff.clone(), len];
    let r = pl::method_mut(it, raf, "Read", &mut a, Want::Scalar)?;
    *buff = a[0].clone();
    Ok(one(r))
}

fn raf_seek(it: &mut Interp, raf: &Value, off: Value, whence: i64) -> ER<bool> {
    Ok(pl::method(it, raf, "Seek", vec![off, n(whence)])?.truthy())
}

fn get_tag_table(it: &mut Interp, name: &str) -> ER<Value> {
    pl::call(it, "Image::ExifTool::GetTagTable", vec![s(name)])
}

// ---- the subs ----

/// AutoLoad our writer routines when necessary
pub fn autoload(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let name = it.get_scalar("Image::ExifTool::PDF::AUTOLOAD");
    let mut args = vec![name];
    args.extend(a);
    it.call_named("Image::ExifTool::DoAutoLoad", args, want)
}

/// Convert from PDF to EXIF-style date/time
/// Inputs: 0) PDF date/time string (D:YYYYmmddHHMMSS+HH'MM')
/// Returns: EXIF date string (YYYY:mm:dd HH:MM:SS+HH:MM)
pub fn convert_pdf_date(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut date = arg(&a, 0);
    // remove optional 'D:' prefix
    re(r"^D:", "").subst(&mut date, false, |_| Vec::new());
    // fill in default values if necessary
    //              YYYYmmddHHMMSS
    let default = s("00000101000000");
    let dl = date.to_bytes().len() as i64;
    if dl < 14 {
        date = pl::concat(&date, &pl::substr(&default, dl, None));
    }
    let Some(m) = re(r"^(\d{4})(\d{2})(\d{2})(\d{2})(\d{2})(\d{2})(.*)", "").m(&date) else {
        return ret(date);
    };
    date = cat(&[cap(&m, 1), s(":"), cap(&m, 2), s(":"), cap(&m, 3), s(" "), cap(&m, 4), s(":"), cap(&m, 5), s(":"), cap(&m, 6)]);
    if cap(&m, 7).truthy() {
        let tz = cap(&m, 7);
        if re(r"^\s*Z", "i").is_match(&tz) {
            // ignore any "HH'mm'" after the Z (OS X 10.6 does this)
            date = pl::concat(&date, &s("Z"));
        // tolerate some improper formatting in timezone specification
        } else if let Some(t) = re(r"^\s*([-+])\s*(\d+)[': ]+(\d*)", "").m(&tz) {
            let mm = if cap(&t, 3).truthy() { cap(&t, 3) } else { s("00") };
            date = cat(&[date, cap(&t, 1), cap(&t, 2), s(":"), mm]);
        }
    }
    ret(date)
}

/// Locate any object in the XRef tables (including compressed objects)
/// Inputs: 0) XRef reference, 1) object reference string (or free object number)
/// Returns: offset to object in file or compressed object reference string,
///          0 if object is free, or undefined on error
pub fn locate_any_object(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (xref, rf) = (arg(&a, 0), arg(&a, 1));
    if !xref.truthy() {
        return pl::ret_undef();
    }
    if xref.exists(rf.to_bytes()) {
        return ret(xref.get(rf.to_bytes()));
    }
    // get the object number
    let Some(m) = re(r"^(\d+)", "").m(&rf) else { return pl::ret_undef() };
    let obj_num = cap(&m, 1);
    // return 0 if the object number has been reused (old object is free)
    if !xref.get(obj_num.to_bytes()).is_undef() {
        return ret(n(0));
    }
    //
    // scan our XRef stream dictionaries for this object
    //
    if !xref.get("dicts").truthy() {
        return pl::ret_undef();
    }
    for dict in aderef(&xref.get("dicts"))? {
        // quick check to see if the object is in the range for this xref stream
        if pl::ge(&obj_num, &dict.get("Size")) {
            continue;
        }
        let index = dict.get("Index");
        if pl::lt(&obj_num, &index.at(0)) {
            continue;
        }
        // scan the tables for the specified object
        let size = dict.get("_entry_size");
        let num = pl::div(&n(aderef(&index)?.len() as i64), &n(2))?;
        let mut tot = n(0);
        let mut i = 0i64;
        while pl::lt(&n(i), &num) {
            let start = index.at(i * 2);
            let count = index.at(i * 2 + 1);
            // table is in ascending order, so quit if we have passed the object
            if pl::lt(&obj_num, &start) {
                break;
            }
            if pl::lt(&obj_num, &pl::add(&start, &count)) {
                let offset = pl::mul(&size, &pl::add(&pl::sub(&obj_num, &start), &tot));
                let stream = dict.get("_stream");
                if pl::gt(&pl::add(&offset, &size), &n(stream.to_bytes().len() as i64)) {
                    break;
                }
                let tmpl = cat(&[s("x"), offset, s(" C"), size.clone()]);
                let mut c = match crate::perl::pack::unpack(&tmpl.to_bytes(), &stream.to_bytes()) {
                    Ok(v) => v,
                    Err(e) => return die(e),
                };
                c.reverse(); // (shifted from the end)
                // extract values from this table entry
                // (can be 1, 2, 3, 4, etc.. bytes per value)
                let mut t = vec![Value::Undef, Value::Undef, Value::Undef];
                let w = dict.get("W");
                for j in 0..3usize {
                    // use default value if W entry is 0 (as per spec)
                    // - 0th element defaults to 1, others default to 0
                    if !w.at(j as i64).truthy() {
                        t[j] = n(if j != 0 { 0 } else { 1 });
                        continue;
                    }
                    t[j] = c.pop().unwrap_or_default();
                    let mut k = 1i64;
                    while pl::lt(&n(k), &w.at(j as i64)) {
                        t[j] = pl::add(&pl::mul(&n(256), &t[j]), &c.pop().unwrap_or_default());
                        k += 1;
                    }
                }
                // by default, use "o g R" as the xref key
                // (o = object number, g = generation number)
                let mut ref2 = cat(&[obj_num.clone(), s(" "), t[2].clone(), s(" R")]);
                if pl::num_eq(&t[0], &n(1)) {
                    // normal object reference:
                    // $t[1]=offset of object from start, $t[2]=generation number
                    xref.set(ref2.to_bytes(), t[1].clone());
                } else if pl::num_eq(&t[0], &n(2)) {
                    // compressed object reference:
                    // $t[1]=stream object number, $t[2]=index of object in stream
                    ref2 = cat(&[obj_num.clone(), s(" 0 R")]);
                    xref.set(ref2.to_bytes(), cat(&[s("I"), t[2].clone(), s(" "), t[1].clone(), s(" 0 R")]));
                } else if pl::num_eq(&t[0], &n(0)) {
                    // free object:
                    // $t[1]=next free object in linked list, $t[2]=generation number
                    xref.set(ref2.to_bytes(), n(0));
                } else {
                    // treat as a null object
                    xref.set(ref2.to_bytes(), Value::Undef);
                }
                xref.set(obj_num.to_bytes(), t[1].clone()); // remember offsets by object number too
                if rf.to_bytes() == ref2.to_bytes() {
                    return ret(xref.get(rf.to_bytes()));
                }
                return ret(n(0)); // object is free or was reused
            }
            tot = pl::add(&tot, &count);
            i += 1;
        }
    }
    pl::ret_undef()
}

fn locate_any(it: &mut Interp, xref: &Value, rf: &Value) -> ER<Value> {
    Ok(one(locate_any_object(it, vec![xref.clone(), rf.clone()], Want::Scalar)?))
}

/// Locate a regular object in the XRef tables (does not include compressed objects)
/// Inputs: 0) XRef reference, 1) object reference string (or free object number)
/// Returns: offset to object in file, 0 if object is free,
///          or undef on error or if object was compressed
pub fn locate_object(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (xref, rf) = (arg(&a, 0), arg(&a, 1));
    let offset = locate_any(it, &xref, &rf)?;
    if offset.truthy() && re(r"^I", "").is_match(&offset) {
        return pl::ret_undef();
    }
    ret(offset)
}

/// Check that the correct object is located at the specified file offset
/// Inputs: 0) ExifTool ref, 1) object name, 2) object reference string, 3) file offset
/// Returns: first non-blank line at start of object, or undef on error
pub fn check_object(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, mut tag, rf, offset) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    let (mut data, mut dat) = (Value::Undef, Value::Undef);

    let raf = et.get("RAF");
    if !raf_seek(it, &raf, pl::add(&offset, &et.get("PDFBase")), 0)? {
        warn(it, &et, cat(&[s("Bad "), tag, s(" offset")]))?;
        return pl::ret_undef();
    }
    // verify that we are reading the expected object
    let mut obj = rf.clone();
    re("R", "").subst(&mut obj, false, |_| b"obj".to_vec());
    loop {
        if !read_line(it, &raf, &mut data)?.truthy() {
            warn(it, &et, cat(&[s("Error reading "), tag, s(" data")]))?;
            return pl::ret_undef();
        }
        let mut p = b"^".to_vec();
        p.extend_from_slice(&obj.to_bytes());
        if pl::re_dyn(&p, "")?.subst(&mut data, false, |_| Vec::new()) > 0 {
            break;
        }
        if re(r"^\s+$", "").is_match(&data) {
            continue; // keep reading if this was a blank line
        }
        // handle cases where other whitespace characters are used in the object ID string
        while re(r"^\d+(\s+\d+)?\s*$", "").is_match(&data) {
            read_line(it, &raf, &mut dat)?;
            data = pl::concat(&data, &dat);
        }
        let mut pat = obj.clone();
        re(" ", "").subst(&mut pat, true, |_| br"\s+".to_vec());
        if pl::re_dyn(&pat.to_bytes(), "")?.subst(&mut data, false, |_| Vec::new()) == 0 {
            tag = pl::ucfirst(&tag);
            warn(it, &et, cat(&[tag, s(" object ("), obj, s(") not found at offset "), offset]))?;
            return pl::ret_undef();
        }
        break;
    }
    // read the first line of data from the object (ignoring blank lines and comments)
    loop {
        if re(r"\S", "").is_match(&data) && !re(r"^\s*%", "").is_match(&data) {
            break;
        }
        if !read_line(it, &raf, &mut data)?.truthy() {
            warn(it, &et, cat(&[s("Error reading "), tag, s(" data")]))?;
            return pl::ret_undef();
        }
    }
    ret(data)
}

fn check_obj(it: &mut Interp, et: &Value, tag: Value, rf: &Value, offset: &Value) -> ER<Value> {
    Ok(one(check_object(it, vec![et.clone(), tag, rf.clone(), offset.clone()], Want::Scalar)?))
}

/// Fetch indirect object from file (from inside a stream if required)
/// Inputs: 0) ExifTool object reference, 1) object reference string,
///         2) xref lookup, 3) object name (for warning messages)
/// Returns: object data or undefined on error
/// Notes: sets $lastFetched to the object reference, or undef if the object
///        was extracted from an encrypted stream
pub fn fetch_object(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, mut rf, xref, tag) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    set_last_fetched(it, rf.clone()); // save this for decoding if necessary
    let mut offset = locate_any(it, &xref, &rf)?;
    lset(it, "lastOffset", offset.clone());
    if !offset.truthy() {
        if offset.is_undef() {
            warn(it, &et, cat(&[s("Bad "), tag, s(" reference")]))?;
        }
        return pl::ret_undef();
    }
    let mut index = Value::Undef;
    if re(r"^I(\d+) ", "").subst(&mut offset, false, |c| {
        index = cap(c, 1);
        Vec::new()
    }) > 0
    {
        // (index is the object index in stream)
        let obj_num = arg(&pl::split_ws(&rf), 0); // save original object number
        rf = offset.clone(); // now a reference to the containing stream object
        let stream_objs = lexh(it, "streamObjs");
        let mut obj = stream_objs.get(rf.to_bytes());
        if !obj.truthy() {
            // don't try to load the same object stream twice
            if !obj.is_undef() {
                return pl::ret_undef();
            }
            stream_objs.set(rf.to_bytes(), s(""));
            // load the parent object stream
            obj = fetch(it, &et, &rf, &xref, &tag)?;
            // make sure it contains everything we need
            if !(!obj.is_undef() && pl::ref_of(&obj) == "HASH") {
                return pl::ret_undef();
            }
            if !(obj.get("First").truthy() && obj.get("N").truthy()) {
                return pl::ret_undef();
            }
            if !decode(it, &et, &obj)? {
                return pl::ret_undef();
            }
            // add a special '_table' entry to this dictionary which contains
            // the list of object number/offset pairs from the stream header
            let num = pl::mul(&obj.get("N"), &n(2));
            let mut table = match pre::split(&rx(r"\s+", ""), true, &obj.get("_stream").to_bytes(), num.to_i64()) {
                Ok(v) => v,
                Err(e) => return die(e),
            };
            if !pl::num_eq(&n(table.len() as i64), &num) {
                return pl::ret_undef();
            }
            // remove everything before first object in stream
            obj.set("_stream", pl::substr(&obj.get("_stream"), obj.get("First").to_i64(), None));
            let last = (num.to_i64() - 1) as usize;
            re(r"^(\d+).*", "s").subst(&mut table[last], false, |c| cap(c, 1).to_bytes().to_vec()); // trim excess from last number
            obj.set("_table", Value::new_array(table));
            // save the object stream so we don't have to re-load it later
            stream_objs.set(rf.to_bytes(), obj.clone());
        }
        // verify that we have the specified object
        let i = pl::mul(&n(2), &index).to_i64();
        let table = obj.get("_table");
        if !(pl::lt(&index, &obj.get("N")) && pl::num_eq(&table.at(i), &obj_num)) {
            warn(it, &et, cat(&[s("Bad index for stream object "), tag]))?;
            return pl::ret_undef();
        }
        // extract the object at the specified index in the stream
        // (offsets in table are in sequential order, so we can subtract from
        //  the next offset to get the object length)
        let offset = table.at(i + 1);
        let next = table.at(i + 3);
        let stream = obj.get("_stream");
        let end = if next.truthy() { next } else { n(stream.to_bytes().len() as i64) };
        let len = pl::sub(&end, &offset);
        let data = pl::substr(&stream, offset.to_i64(), Some(len.to_i64()));
        // avoid re-decrypting data in already decrypted streams
        if lget(it, "cryptStream").truthy() {
            set_last_fetched(it, Value::Undef);
        }
        return extract_object(it, vec![et, pl::sref(data)], Want::Scalar);
    }
    // load the start of the object
    let data = check_obj(it, &et, tag, &rf, &offset)?;
    if data.is_undef() {
        return pl::ret_undef();
    }

    let raf = et.get("RAF");
    extract_object(it, vec![et, pl::sref(data), raf, xref], Want::Scalar)
}

fn fetch(it: &mut Interp, et: &Value, rf: &Value, xref: &Value, tag: &Value) -> ER<Value> {
    Ok(one(fetch_object(it, vec![et.clone(), rf.clone(), xref.clone(), tag.clone()], Want::Scalar)?))
}

/// Convert PDF value to something readable
/// Inputs: 0) PDF object data
/// Returns: converted object
pub fn read_pdf_value(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let str_ = arg(&a, 0);
    ret(read_value(it, str_)?)
}

fn read_value(it: &mut Interp, mut str_: Value) -> ER<Value> {
    // decode all strings in an array
    if pl::ref_of(&str_) == "ARRAY" {
        // create new list to not alter the original data when rewriting
        let mut vals = Vec::new();
        for val in str_.items() {
            vals.push(read_value(it, val)?);
        }
        return Ok(Value::new_array(vals));
    }
    if !pl::length(&str_).truthy() {
        return Ok(str_);
    }
    let b = str_.to_bytes();
    let delim = b[0];
    if delim == b'(' {
        // literal string
        if let Some(m) = re(r"^.*?\((.*)\)", "s").m(&str_) {
            str_ = cap(&m, 1); // remove brackets
        }
        // decode escape sequences in literal strings
        let mut st = str_.to_bytes().to_vec();
        let esc = rx(r"\\(.)", "s");
        let oct2 = rx(r"\G([0-7]{1,2})", "");
        let lf = rx(r"\G(\x0a)", "");
        let mut pos = 0usize;
        while let Some(m) = m_at(&esc, &st, pos) {
            pos = m[0].unwrap().1;
            let nn = pos - 2;
            let mut c = sp(&st, &m, 1).to_bytes().to_vec();
            let r: Vec<u8>;
            if c[0].is_ascii_digit() && c[0] <= b'7' {
                // get up to 2 more octal digits
                if let Some(m2) = m_at(&oct2, &st, pos) {
                    c.extend_from_slice(&sp(&st, &m2, 1).to_bytes());
                }
                // convert octal escape code
                let v = c.iter().fold(0u32, |acc, d| acc * 8 + (d - b'0') as u32);
                r = vec![(v & 0xff) as u8];
            } else if c[0] == 0x0d {
                // the string is continued if the line ends with '\'
                // (also remove "\x0d\x0a")
                if let Some(m2) = m_at(&lf, &st, pos) {
                    c.extend_from_slice(&sp(&st, &m2, 1).to_bytes());
                }
                r = Vec::new();
            } else if c[0] == 0x0a {
                r = Vec::new();
            } else {
                // convert escaped characters
                let mut x = Value::bytes(&c);
                pl::tr(&mut x, "nrtbf", "\n\r\t\x08\x0c", "");
                r = x.to_bytes().to_vec();
            }
            st.splice(nn..nn + c.len() + 1, r.iter().copied());
            // continue search after this character
            pos = nn + r.len();
        }
        str_ = Value::bytes(st);
        if lget(it, "cryptString").truthy() {
            let lf = last_fetched(it);
            let r = pl::sref(str_);
            crypt(it, vec![r.clone(), lf], Want::Void)?;
            str_ = r.deref();
        }
    } else if delim == b'<' {
        // hex string
        // decode hex data
        pl::tr(&mut str_, "0-9A-Fa-f", "", "dc");
        if str_.to_bytes().len() & 0x01 != 0 {
            str_ = pl::concat(&str_, &s("0")); // (by the spec)
        }
        str_ = pl::pack("H*", &[str_])?;
        if lget(it, "cryptString").truthy() {
            let lf = last_fetched(it);
            let r = pl::sref(str_);
            crypt(it, vec![r.clone(), lf], Want::Void)?;
            str_ = r.deref();
        }
    } else if delim == b'/' {
        // name
        str_ = pl::substr(&str_, 1, None);
        // convert escape codes (PDF 1.2 or later)
        if pl::ge(&lget(it, "pdfVer"), &Value::Num(1.2)) {
            re(r"#([0-9a-f]{2})", "si").subst(&mut str_, true, |c| vec![u8::from_str_radix(&cap(c, 1).to_text(), 16).unwrap_or(0)]);
        }
    }
    Ok(str_)
}

/// Extract PDF object from combination of buffered data and file
/// Inputs: 0) ExifTool object reference, 1) data reference,
///         2) optional raf reference, 3) optional xref table
/// Returns: converted PDF object or undef on error
///          a) dictionary object --> hash reference
///          b) array object --> array reference
///          c) indirect reference --> scalar reference
///          d) string, name, integer, boolean, null --> scalar value
/// - updates $$dataPt on return to contain unused data
/// - creates two bogus entries ('_stream' and '_tags') in dictionaries to represent
///   the stream data and a list of the tags (not including '_stream' and '_tags')
///   in their original order
pub fn extract_object(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, data_pt, raf, xref) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    let mut tags: Vec<Value> = Vec::new();
    let mut data = Value::Undef;
    let obj_data: Value;
    let dict = Value::new_hash();
    let delim: Value;

    loop {
        let mut d = sderef(&data_pt)?;
        if let Some(m) = re(r"^\s*(<{1,2}|\[|\()", "s").m(&d) {
            delim = cap(&m, 1);
            re(r"^\s+", "").subst(&mut d, false, |_| Vec::new()); // remove leading white space
            pl::set_deref(&data_pt, d)?;
            obj_data = one(read_to_nested(it, vec![data_pt.clone(), raf.clone()], Want::Scalar)?);
            if obj_data.is_undef() {
                return pl::ret_undef();
            }
            break;
        }
        let mut od = Value::Undef;
        if re(r"^\s*(\S[^[(/<>\s]*)\s*", "s").subst(&mut d, false, |c| {
            od = cap(c, 1);
            Vec::new()
        }) > 0
        {
            pl::set_deref(&data_pt, d.clone())?;
            //
            // extract boolean, numerical, string, name, null object or indirect reference
            //
            let mut obj_data = od;
            // look for an indirect reference
            if re(r"^\d+$", "").is_match(&obj_data) {
                let mut g = Value::Undef;
                if re(r"^(\d+)\s+R", "s").subst(&mut d, false, |c| {
                    g = cap(c, 1);
                    Vec::new()
                }) > 0
                {
                    pl::set_deref(&data_pt, d)?;
                    obj_data = cat(&[obj_data, g, s(" R")]);
                    obj_data = pl::sref(obj_data); // return scalar reference
                }
            }
            return ret(obj_data); // return simple scalar or scalar reference
        }
        if !(raf.truthy() && read_line(it, &raf, &mut data)?.truthy()) {
            return pl::ret_undef();
        }
        pl::set_deref(&data_pt, pl::concat(&sderef(&data_pt)?, &data))?;
    }
    //
    // return literal string or hex string without parsing
    //
    if pl::str_eq(&delim, "(") || pl::str_eq(&delim, "<") {
        return ret(obj_data);
    //
    // extract array
    //
    } else if pl::str_eq(&delim, "[") {
        let Some(m) = re(r"^.*?\[(.*)\]", "s").m(&obj_data) else { return pl::ret_undef() };
        let data = cap(&m, 1).to_bytes().to_vec(); // brackets removed
        let mut list = Vec::new();
        let item = rx(r"\s*(\S[^[(/<>\s]*)", "s");
        let refp = rx(r"\G\s+(\d+)\s+R", "");
        let mut pos = 0usize;
        loop {
            let Some(m) = m_at(&item, &data, pos) else { break };
            pos = m[0].unwrap().1;
            let mut val = sp(&data, &m, 1);
            if re(r"^(<{1,2}|\[|\()", "").is_match(&val) {
                let p = pos - val.to_bytes().len();
                // nested dict, array, literal string or hex string
                let buff = pl::sref(Value::bytes(&data[p..]));
                val = one(read_to_nested(it, vec![buff], Want::Scalar)?);
                if val.is_undef() {
                    break;
                }
                pos = p + val.to_bytes().len();
                val = one(extract_object(it, vec![et.clone(), pl::sref(val)], Want::Scalar)?);
            } else if re(r"^\d", "").is_match(&val) {
                if let Some(m2) = m_at(&refp, &data, pos) {
                    pos = m2[0].unwrap().1;
                    val = pl::sref(cat(&[val, s(" "), sp(&data, &m2, 1), s(" R")])); // make a reference
                }
            }
            list.push(val);
        }
        return ret(Value::new_array(list));
    }
    //
    // extract dictionary
    //
    // Note: entries are not necessarily separated by whitespace (doh!)
    // eg) "/Tag/Name", "/Tag(string)", "/Tag[array]", etc are legal!
    // Also, they may be separated by a comment (eg. "/Tag%comment\nValue"),
    // but comments have already been removed
    let entry = rx(r"(\s*)/([^/[\]()<>{}\s]+)\s*(\S[^[(/<>\s]*)", "s");
    let refp = rx(r"\G\s+(\d+)\s+R", "s");
    let mut od = obj_data.to_bytes().to_vec();
    let mut pos = 0usize;
    while let Some(m) = m_at(&entry, &od, pos) {
        pos = m[0].unwrap().1;
        let tag = sp(&od, &m, 2);
        let mut val = sp(&od, &m, 3);
        if re(r"^(<{1,2}|\[|\()", "").is_match(&val) {
            // nested dict, array, literal string or hex string
            let r = pl::sref(Value::bytes(&od[pos - val.to_bytes().len()..]));
            val = one(read_to_nested(it, vec![r.clone(), raf.clone()], Want::Scalar)?);
            od = r.deref().to_bytes().to_vec();
            if val.is_undef() {
                break;
            }
            val = one(extract_object(it, vec![et.clone(), pl::sref(val)], Want::Scalar)?);
            pos = 0;
        } else if re(r"^\d", "").is_match(&val) {
            if let Some(m2) = m_at(&refp, &od, pos) {
                pos = m2[0].unwrap().1;
                val = pl::sref(cat(&[val, s(" "), sp(&od, &m2, 1), s(" R")])); // make a reference
            }
        }
        if dict.get(tag.to_bytes()).truthy() {
            // duplicate dictionary entries are not allowed
            warn(it, &et, cat(&[s("Duplicate '"), tag, s("' entry in dictionary (ignored)")]))?;
        } else {
            // save the entry
            tags.push(tag.clone());
            dict.set(tag.to_bytes(), val);
        }
    }
    if tags.is_empty() {
        return pl::ret_undef();
    }
    dict.set("_tags", Value::new_array(tags));
    if !raf.truthy() {
        return ret(dict); // direct objects can not have streams
    }
    //
    // extract the stream object
    //
    // dictionary must specify stream Length
    let mut length = dict.get("Length");
    if !length.truthy() {
        return ret(dict);
    }
    if length.is_ref() {
        length = sderef(&length)?;
        let oldpos = pl::method(it, &raf, "Tell", vec![])?;
        // get the location of the object specifying the length
        // (compressed objects are not allowed)
        let offset = one(locate_object(it, vec![xref.clone(), length.clone()], Want::Scalar)?);
        if !offset.truthy() {
            return ret(dict);
        }
        // ($offset or $et->Warn('Bad stream Length object'), return $dict;)
        data = check_obj(it, &et, s("stream Length"), &length, &offset)?;
        if data.is_undef() {
            return ret(dict);
        }
        let Some(m) = re(r"^\s*(\d+)", "").m(&data) else {
            warn(it, &et, s("Stream Length not found"))?;
            return ret(dict);
        };
        length = cap(&m, 1);
        raf_seek(it, &raf, oldpos, 0)?; // restore position to start of stream
    }
    // extract the trailing stream data
    loop {
        // find the stream token
        if let Some(m) = re(r"(\S+)", "").m(&sderef(&data_pt)?) {
            if !pl::str_eq(&cap(&m, 1), "stream") {
                break;
            }
            // read an extra line because it may contain our \x0a
            if read_line(it, &raf, &mut data)?.truthy() {
                pl::set_deref(&data_pt, pl::concat(&sderef(&data_pt)?, &data))?;
            }
            // remove our stream header
            let mut d = sderef(&data_pt)?;
            re(r"^\s*stream(\x0a|\x0d\x0a)", "s").subst(&mut d, false, |_| Vec::new());
            pl::set_deref(&data_pt, d.clone())?;
            let more = pl::sub(&length, &n(d.to_bytes().len() as i64));
            if pl::gt(&more, &n(0)) {
                if !pl::num_eq(&raf_read(it, &raf, &mut data, more.clone())?, &more) {
                    warn(it, &et, s("Error reading stream data"))?;
                    pl::set_deref(&data_pt, s(""))?;
                    return ret(dict);
                }
                dict.set("_stream", pl::concat(&d, &data));
                pl::set_deref(&data_pt, s(""))?;
            } else if pl::lt(&more, &n(0)) {
                dict.set("_stream", pl::substr(&d, 0, Some(length.to_i64())));
                pl::set_deref(&data_pt, pl::substr(&d, length.to_i64(), None))?;
            } else {
                dict.set("_stream", d);
                pl::set_deref(&data_pt, s(""))?;
            }
            break;
        }
        if !read_line(it, &raf, &mut data)?.truthy() {
            break;
        }
        pl::set_deref(&data_pt, pl::concat(&sderef(&data_pt)?, &data))?;
    }
    ret(dict)
}

/// Read to nested delimiter
/// Inputs: 0) data reference, 1) optional raf reference
/// Returns: data up to and including matching delimiter (or undef on error)
/// - updates data reference with trailing data
/// - unescapes characters in literal strings
pub fn read_to_nested(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (data_pt, raf) = (arg(&a, 0), arg(&a, 1));
    let closing_delim = lexh(it, "closingDelim");
    let mut delim: Vec<Value> = vec![s("")]; // closing delimiter list, most deeply nested first
    let mut d = sderef(&data_pt)?.to_bytes().to_vec();
    let mut pos = 0usize; // begin at start of data
    let tok = rx(r"(\\*)(\(|\)|<{1,2}|>{1,2}|\[|\]|%)", "");
    let line = rx(".*", "");
    loop {
        let Some(m) = m_at(&tok, &d, pos) else {
            // must read some more data
            let mut buff = Value::Undef;
            if !(raf.truthy() && read_line(it, &raf, &mut buff)?.truthy()) {
                break;
            }
            let b = buff.to_bytes();
            d.extend_from_slice(&b);
            pos = d.len() - b.len();
            continue;
        };
        pos = m[0].unwrap().1;
        let g1 = sp(&d, &m, 1).to_bytes().len();
        let g2 = sp(&d, &m, 2);
        // are we in a literal string?
        if pl::str_eq(&delim[0], ")") {
            // ignore escaped delimiters (preceded by odd number of \'s)
            if g1 & 0x01 != 0 {
                continue;
            }
            // ignore all delimiters but unescaped braces
            if !(pl::str_eq(&g2, "(") || pl::str_eq(&g2, ")")) {
                continue;
            }
        } else if pl::str_eq(&g2, "%") {
            // ignore the comment
            let p = pos - 1;
            // remove everything from '%' up to but not including newline
            let end = m_at(&line, &d, pos).map(|m| m[0].unwrap().1).unwrap_or(pos);
            d.drain(p..end);
            pos = p;
            continue;
        }
        let cd = closing_delim.get(g2.to_bytes());
        if cd.truthy() {
            // push the corresponding closing delimiter
            delim.insert(0, cd);
            continue;
        }
        if g2.to_bytes() != delim[0].to_bytes() {
            // handle the case where we find a ">>>" and interpret it
            // as ">> >" instead of "> >>"
            if !(pl::str_eq(&g2, ">>") && pl::str_eq(&delim[0], ">")) {
                continue;
            }
            pos -= 1;
        }
        delim.remove(0); // remove from nesting list
        if delim.first().is_some_and(|v| v.truthy()) {
            continue; // keep going if we have more nested delimiters
        }
        let buff = Value::bytes(&d[..pos]);
        pl::set_deref(&data_pt, Value::bytes(&d[pos..]))?;
        return ret(buff); // success!
    }
    pl::set_deref(&data_pt, Value::bytes(d))?;
    pl::ret_undef() // didn't find matching delimiter
}

/// Decode LZW-encoded data (ref 1)
/// Inputs: 0) data reference
/// Returns: true on success and data is decoded, or false and data is untouched
pub fn decode_lzw(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let data_pt = arg(&a, 0);
    let data = sderef(&data_pt)?.to_bytes();
    if data.len() < 4 {
        return ret(n(0));
    }
    // LZW code table
    let mut lzw: Vec<Option<Vec<u8>>> = (0..=255u8).map(|c| Some(vec![c])).collect();
    lzw.push(None);
    lzw.push(None);
    let mut mask: u64 = 0x01ff; // mask for least-significant 9 bits
    let mut padded = data.to_vec();
    padded.push(0);
    let dat: Vec<u64> = padded.chunks_exact(2).map(|c| ((c[0] as u64) << 8) | c[1] as u64).collect();
    let mut word: u64 = (dat[0] << 16) | dat[1];
    let (mut bit, mut pos, mut bits, mut out): (i64, usize, i64, Vec<u8>) = (0, 2, 9, Vec::new());
    let mut last_val: Option<Vec<u8>> = None;
    loop {
        // bits are packed MSB first in PDF LZW (the PDF spec doesn't mention this)
        let mut shift = 32 - (bit + bits);
        if shift < 0 {
            if pos >= dat.len() {
                return ret(n(0)); // missing EOD marker
            }
            word = ((word & 0xffff) << 16) | dat[pos]; // read next word
            pos += 1;
            bit -= 16;
            shift += 16;
        }
        let code = ((word >> shift) & mask) as usize;
        bit += bits;
        let mut val = lzw.get(code).cloned().flatten();
        if let Some(v) = &val {
            // store new code as previous sequence plus 1st char of new sequence
            if let Some(lv) = &last_val {
                let mut e = lv.clone();
                e.extend_from_slice(&v[..v.len().min(1)]);
                lzw.push(Some(e));
            }
        } else if code == lzw.len() {
            // new code
            let Some(lv) = &last_val else { return ret(n(0)) };
            // we are using the code that we are about to generate, so the last
            // character in the new sequence must be the same as the first
            // character in the previous sequence (makes sense if you think about it)
            let mut v = lv.clone();
            v.extend_from_slice(&lv[..lv.len().min(1)]);
            lzw.push(Some(v.clone()));
            val = Some(v);
        } else if code == 256 {
            // clear table
            lzw.truncate(258);
            bits = 9;
            mask = 0x1ff;
            last_val = None;
            continue;
        } else if code == 257 {
            // EOD marker
            break; // all done!
        } else {
            return ret(n(0));
        }
        let val = val.unwrap_or_default();
        out.extend_from_slice(&val); // add this byte sequence to the output
        // we added a new entry to the LZW table, so we must increase
        // the bit width if necessary, up to a maximum of 12
        if lzw.len() as u64 >= mask && bits < 12 {
            bits += 1;
            mask |= mask << 1;
        }
        last_val = Some(val);
    }
    pl::set_deref(&data_pt, Value::bytes(out))?; // return decompressed data
    ret(n(1))
}

/// Decode filtered stream
/// Inputs: 0) ExifTool object reference, 1) dictionary reference
/// Returns: true if stream has been decoded OK
pub fn decode_stream(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    // (local $_: the loops below alias nothing the callees see)
    let (et, dict) = (arg(&a, 0), arg(&a, 1));

    if !dict.get("_stream").truthy() {
        return ret(n(0)); // no stream to decode
    }

    // get list of filters
    let mut filters: Vec<Value> = Vec::new();
    if pl::ref_of(&dict.get("Filter")) == "ARRAY" {
        filters = dict.get("Filter").items();
    } else if !dict.get("Filter").is_undef() {
        filters = vec![dict.get("Filter")];
    }
    // be sure we can process all the filters before we take the time to do the decryption
    let supported_filter = lexh(it, "supportedFilter");
    for filter in &filters {
        if supported_filter.get(filter.to_bytes()).truthy() {
            continue;
        }
        warn(it, &et, cat(&[s("Unsupported Filter "), filter.clone()]))?;
        return ret(n(0));
    }
    // apply decryption first if required (and if the default encryption
    // has not been overridden by a Crypt filter. Note: the Crypt filter
    // must be first in the Filter array: ref 3, page 38)
    let f0 = filters.first().cloned().unwrap_or_default();
    if !(!dict.get("_decrypted").is_undef() || (f0.truthy() && pl::str_eq(&f0, "/Crypt"))) {
        let lf = last_fetched(it);
        crypt_stream(it, vec![dict.clone(), lf], Want::Void)?;
    }
    if !dict.get("Filter").truthy() {
        return ret(n(1)); // Filter entry is mandatory
    }
    if !dict.get("_filtered").is_undef() {
        return ret(n(0)); // avoid double-filtering
    }
    dict.set("_filtered", n(1)); // set flag to prevent double-filtering

    // get array of DecodeParms dictionaries
    let mut decode_parms_list: Vec<Value> = if pl::ref_of(&dict.get("DecodeParms")) == "ARRAY" { dict.get("DecodeParms").items() } else { vec![dict.get("DecodeParms")] };
    decode_parms_list.reverse(); // (shifted from the end)

    for filter in &filters {
        let decode_parms = decode_parms_list.pop().unwrap_or_default();

        if pl::str_eq(filter, "/FlateDecode") {
            // make sure we support the predictor (if used) before decoding
            let mut pre = Value::Undef;
            if pl::ref_of(&decode_parms) == "HASH" {
                pre = decode_parms.get("Predictor");
                if pre.truthy() && !pl::str_eq(&pre, "1") && !pl::str_eq(&pre, "12") {
                    warn(it, &et, cat(&[s("FlateDecode Predictor "), pre, s(" currently not supported")]))?;
                    return ret(n(0));
                }
            }
            if pl::eval_block(it, |it| runtime::require(it, "Compress::Zlib").map(|_| n(1)))?.is_some() {
                let inflate = pl::call(it, "Compress::Zlib::inflateInit", vec![])?;
                let (mut buff, mut stat) = (Value::Undef, Value::Undef);
                if inflate.truthy() {
                    let r = pl::method_list(it, &inflate, "inflate", vec![dict.get("_stream")])?;
                    buff = arg(&r, 0);
                    stat = arg(&r, 1);
                }
                let end = pl::call(it, "Compress::Zlib::Z_STREAM_END", vec![])?;
                if inflate.truthy() && pl::num_eq(&stat, &end) {
                    dict.set("_stream", buff);
                } else {
                    warn(it, &et, s("Error inflating stream"))?;
                    return ret(n(0));
                }
            } else {
                warn(it, &et, s("Install Compress::Zlib to process filtered streams"))?;
                return ret(n(0));
            }
            if !(pre.truthy() && pl::str_eq(&pre, "12")) {
                continue; // 12 = 'up' prediction
            }

            // apply anti-predictor
            let cols = decode_parms.get("Columns");
            if !cols.truthy() {
                // currently only support 'up' prediction
                warn(it, &et, s("No Columns for decoding stream"))?;
                return ret(n(0));
            }
            let bytes = dict.get("_stream").to_bytes();
            let mut bi = 0usize; // (bytes are shifted from the front)
            let ncols = cols.to_i64().max(0) as usize;
            let mut pre_v = vec![0u8; ncols]; // initialize predictor array
            let mut buff = Vec::new();
            while pl::gt(&n((bytes.len() - bi) as i64), &cols) {
                let b = bytes[bi];
                bi += 1;
                if b != 2 {
                    warn(it, &et, cat(&[s("Unsupported PNG filter "), n(b as i64)]))?; // (yes, PNG)
                    return ret(n(0));
                }
                for p in pre_v.iter_mut() {
                    let x = bytes.get(bi).copied().unwrap_or(0);
                    if bi < bytes.len() {
                        bi += 1;
                    }
                    *p = p.wrapping_add(x);
                }
                buff.extend_from_slice(&pre_v);
            }
            dict.set("_stream", Value::bytes(buff));
        } else if pl::str_eq(filter, "/Crypt") {
            // (we shouldn't have to check the _decrypted flag since we
            //  already checked the _filtered flag, but what the heck...)
            if !dict.get("_decrypted").is_undef() {
                continue;
            }
            // assume Identity filter (the default) if DecodeParms are missing
            if pl::ref_of(&decode_parms) != "HASH" {
                continue;
            }
            let name = decode_parms.get("Name");
            if !(!name.is_undef() || pl::str_eq(&name, "Identity")) {
                continue;
            }
            if !pl::str_eq(&name, "StdCF") {
                warn(it, &et, cat(&[s("Unsupported Crypt Filter "), name]))?;
                return ret(n(0));
            }
            if !lget(it, "cryptInfo").truthy() {
                warn(it, &et, s("Missing Encrypt StdCF entry"))?;
                return ret(n(0));
            }
            // decrypt the stream manually because we want to:
            // 1) ignore $cryptStream (StmF) setting
            // 2) ignore EncryptMetadata setting (I can't find mention of how to
            //    reconcile this in the spec., but this would make sense)
            // 3) avoid adding the crypt key extension (ref 3, page 58, Algorithm 1b)
            // 4) set _decrypted flag so we will recrypt according to StmF when
            //    writing (since we don't yet write Filter'd streams)
            crypt(it, vec![pl::elem_ref(&dict, b"_stream"), s("none")], Want::Void)?;
            let cs = lget(it, "cryptStream").truthy();
            dict.set("_decrypted", n(if cs { 1 } else { 0 }));
        } else if pl::str_eq(filter, "/LZWDecode") {
            // make sure we don't have any unsupported decoding parameters
            if pl::ref_of(&decode_parms) == "HASH" {
                if decode_parms.get("Predictor").truthy() {
                    warn(it, &et, cat(&[s("LZWDecode Predictor "), decode_parms.get("Predictor"), s(" currently not supported")]))?;
                    return ret(n(0));
                } else if decode_parms.get("EarlyChange").truthy() {
                    warn(it, &et, s("LZWDecode EarlyChange currently not supported"))?;
                    return ret(n(0));
                }
            }
            if !one(decode_lzw(it, vec![pl::elem_ref(&dict, b"_stream")], Want::Scalar)?).truthy() {
                warn(it, &et, s("LZW decompress error"))?;
                return ret(n(0));
            }
        } else if pl::str_eq(filter, "/ASCIIHexDecode") {
            let mut st = dict.get("_stream");
            re(">.*", "").subst(&mut st, false, |_| Vec::new()); // truncate at '>' (end of data mark)
            pl::tr(&mut st, "0-9a-zA-Z", "", "d"); // remove illegal characters
            dict.set("_stream", pl::pack("H*", &[st])?);
        } else if pl::str_eq(filter, "/ASCII85Decode") {
            let mut err = 0;
            let mut out: Vec<u8> = Vec::new();
            let (mut nn, mut val): (i64, i64) = (0, 0);
            for &c in dict.get("_stream").to_bytes().iter() {
                if (b'!'..=b'u').contains(&c) {
                    val = 85 * val + c as i64 - 33;
                    nn += 1;
                    if nn != 5 {
                        continue;
                    }
                } else if c == b'~' {
                    if nn == 1 {
                        err = 1; // error to have a single char in the last group of 5
                    }
                    for _ in nn..5 {
                        val *= 85;
                    }
                } else if c == b'z' {
                    if nn != 0 {
                        err = 2; // error if 'z' isn't the first char
                        break;
                    }
                    nn = 5;
                } else {
                    if b" \t\n\x0b\x0c\r".contains(&c) {
                        continue; // ignore white space
                    }
                    err = 3; // any other character is an error
                    break;
                }
                val = (val as u64 as u32).swap_bytes() as i64; // reverse byte order
                loop {
                    nn -= 1;
                    if nn <= 0 {
                        break;
                    }
                    out.push((val & 0xff) as u8);
                    val >>= 8;
                }
                if c == b'~' {
                    break;
                }
                // (both $n and $val are zero again now)
            }
            if err != 0 {
                warn(it, &et, cat(&[s("ASCII85Decode error "), n(err)]))?;
            }
            dict.set("_stream", Value::bytes(out));
        }
    }
    ret(n(1))
}

fn decode(it: &mut Interp, et: &Value, dict: &Value) -> ER<bool> {
    Ok(one(decode_stream(it, vec![et.clone(), dict.clone()], Want::Scalar)?).truthy())
}

/// Initialize state for RC4 en/decryption (ref 2)
/// Inputs: 0) RC4 key string
/// Returns: RC4 key hash reference
pub fn rc4_init(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let key = arg(&a, 0).to_bytes();
    let mut state: Vec<i64> = (0..256).collect();
    let (mut i, mut j) = (0usize, 0i64);
    while i < 256 {
        let st = state[i];
        if key.is_empty() {
            return die("Illegal modulus zero");
        }
        j = (j + st + key[i % key.len()] as i64) & 0xff;
        state[i] = state[j as usize];
        i += 1;
        state[j as usize] = st;
    }
    let h = Value::new_hash();
    h.set("State", Value::new_array(state.into_iter().map(n).collect()));
    h.set("XY", Value::new_array(vec![n(0), n(0)]));
    ret(h)
}

/// Apply RC4 en/decryption (ref 2)
/// Inputs: 0) data reference, 1) RC4 key hash reference or RC4 key string
/// - can call this method directly with a key string, or with with the key
///   reference returned by RC4Init
/// - RC4 is a symmetric algorithm, so encryption is the same as decryption
pub fn rc4_crypt(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (data_pt, mut key) = (arg(&a, 0), arg(&a, 1));
    if pl::ref_of(&key) != "HASH" {
        key = one(rc4_init(it, vec![key], Want::Scalar)?);
    }
    let state = key.get("State");
    let xy = key.get("XY");
    let (mut x, mut y) = (xy.at(0).to_i64(), xy.at(1).to_i64());
    let mut st: Vec<i64> = state.items().iter().map(|v| v.to_i64()).collect();
    if st.len() < 256 {
        st.resize(256, 0);
    }

    let mut data: Vec<i64> = sderef(&data_pt)?.to_bytes().iter().map(|&b| b as i64).collect();
    for d in data.iter_mut() {
        x = (x + 1) & 0xff;
        let stx = st[x as usize];
        y = (stx + y) & 0xff;
        let sty = st[y as usize];
        st[x as usize] = sty;
        st[y as usize] = stx;
        *d ^= st[((stx + sty) & 0xff) as usize];
    }
    // (the state array is updated in place)
    if let Value::ArrayRef(arr) = &state {
        let mut arr = arr.borrow_mut();
        let len = arr.len().max(256);
        arr.resize(len, Value::Undef);
        for (k, v) in st.iter().enumerate() {
            arr[k] = n(*v);
        }
    }
    key.set("XY", Value::new_array(vec![n(x), n(y)]));
    let out = pl::pack("C*", &data.into_iter().map(n).collect::<Vec<_>>())?;
    pl::set_deref(&data_pt, out.clone())?;
    ret(out)
}

/// Update AES cipher with a bit of data
/// Inputs: 0) data
/// Returns: encrypted data
pub fn cipher_update(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut dat = arg(&a, 0);
    let mut pos = 0usize;
    if pl::length(&dat).truthy() {
        dat = pl::concat(&lget(it, "cipherMore"), &dat);
    }
    let mut d = dat.to_bytes().to_vec();
    while pos + 16 <= d.len() {
        let c = pl::call(it, "Image::ExifTool::AES::Cipher", vec![Value::bytes(&d[pos..pos + 16])])?;
        d.splice(pos..pos + 16, c.to_bytes().iter().copied());
        pos += 16;
    }
    if pos < d.len() {
        lset(it, "cipherMore", Value::bytes(&d[pos..]));
        d.truncate(pos);
    } else {
        lset(it, "cipherMore", s(""));
    }
    ret(Value::bytes(d))
}

fn cipher_upd(it: &mut Interp, v: &Value) -> ER<Value> {
    Ok(one(cipher_update(it, vec![v.clone()], Want::Scalar)?))
}

/// Get encrypted hash
/// Inputs: 0) Password, 1) salt, 2) vector, 3) encryption revision
/// Returns: hash
pub fn get_hash(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (password, salt, vector, rev) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));

    // return Rev 5 hash
    if pl::num_eq(&rev, &n(5)) {
        return ret(pl::call(it, "Digest::SHA::sha256", vec![password, salt, vector])?);
    }

    // compute Rev 6 hardened hash
    // (ref http://code.google.com/p/origami-pdf/source/browse/lib/origami/encryption.rb)
    let mut block_size = 32i64;
    let sha = pl::call(it, "Digest::SHA::sha256", vec![password.clone(), salt, vector.clone()])?;
    let mut input = pl::concat(&sha, &Value::bytes([0u8; 32]));
    let mut key = pl::substr(&input, 0, Some(16));
    let mut iv = pl::substr(&input, 16, Some(16));
    let mut h = Value::Undef;
    let mut x = s("");
    let mut i = 0i64;
    loop {
        let last = x.to_bytes().last().map(|&b| b as i64).unwrap_or(0);
        if !(i < 64 || i < last + 32) {
            break;
        }

        let block = pl::substr(&input, 0, Some(block_size));
        x = s("");
        let xr = pl::sref(x.clone());
        pl::call(it, "Image::ExifTool::AES::Crypt", vec![xr.clone(), key.clone(), iv.clone(), n(1)])?;
        x = xr.deref();
        lset(it, "cipherMore", s(""));

        let mut digest = Value::Undef;
        for j in 0..64 {
            x = s("");
            if pl::length(&password).truthy() {
                x = pl::concat(&x, &cipher_upd(it, &password)?);
            }
            x = pl::concat(&x, &cipher_upd(it, &block)?);
            if pl::length(&vector).truthy() {
                x = pl::concat(&x, &cipher_upd(it, &vector)?);
            }
            if j == 0 {
                let sum: i64 = x.to_bytes().iter().take(16).map(|&b| b as i64).sum();
                // set SHA block size (32, 48 or 64 bytes = SHA-256, 384 or 512)
                block_size = 32 + (sum % 3) * 16;
                digest = pl::method(it, &s("Digest::SHA"), "new", vec![n(block_size * 8)])?;
            }
            pl::method(it, &digest, "add", vec![x.clone()])?;
        }

        h = pl::method(it, &digest, "digest", vec![])?;
        key = pl::substr(&h, 0, Some(16));
        let rest = pl::substr(&input, 16, None);
        input = pl::concat(&h, &rest);
        iv = pl::substr(&h, 16, Some(16));
        i += 1;
    }
    ret(pl::substr(&h, 0, Some(32)))
}

fn hash(it: &mut Interp, password: &Value, salt: Value, vector: Value, rev: &Value) -> ER<Value> {
    Ok(one(get_hash(it, vec![password.clone(), salt, vector, rev.clone()], Want::Scalar)?))
}

/// Initialize decryption
/// Inputs: 0) ExifTool object reference, 1) Encrypt dictionary reference,
///         2) ID from file trailer dictionary
/// Returns: error string or undef on success (and sets $cryptInfo)
pub fn decrypt_init(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    // (local $_: the loops below alias nothing the callees see)
    let (et, encrypt, id) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));

    lset(it, "cryptInfo", Value::Undef);
    if !(encrypt.truthy() && pl::ref_of(&encrypt) == "HASH") {
        return ret(s("Error loading Encrypt object"));
    }
    let mut filt = encrypt.get("Filter");
    if !(filt.truthy() && re(r"^\/", "").subst(&mut filt, false, |_| Vec::new()) > 0) {
        return ret(s("Encrypt dictionary has no Filter!"));
    }
    // extract some interesting tags
    let ver = if encrypt.get("V").truthy() { encrypt.get("V") } else { n(0) };
    let rev = if encrypt.get("R").truthy() { encrypt.get("R") } else { n(0) };
    let mut enc = cat(&[filt.clone(), s(" V"), ver.clone()]);
    if pl::str_eq(&filt, "Standard") {
        enc = cat(&[enc, s("."), rev.clone()]);
    }
    if encrypt.get("SubFilter").truthy() {
        if let Some(m) = re(r"^\/(.*)", "").m(&encrypt.get("SubFilter")) {
            enc = cat(&[enc, s(" ("), cap(&m, 1), s(")")]);
        }
    }
    if pl::str_eq(&filt, "Standard") {
        let l = if encrypt.get("Length").truthy() { encrypt.get("Length") } else { n(40) };
        enc = cat(&[enc, s(" ("), l, s("-bit)")]);
    }
    let tag_table_ptr = get_tag_table(it, "Image::ExifTool::PDF::Encrypt")?;
    pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), s("Filter"), enc])?;
    if !pl::str_eq(&filt, "Standard") {
        return ret(cat(&[s("Encryption filter "), filt, s(" currently not supported")]));
    } else if encrypt.get("R").is_undef() {
        return ret(s("Standard security handler missing revision"));
    }
    if !(encrypt.get("O").truthy() && encrypt.get("P").truthy() && encrypt.get("U").truthy()) {
        return ret(s("Incomplete Encrypt specification"));
    }
    if pl::ge(&cat(&[ver.clone(), s("."), rev.clone()]), &Value::Num(5.6)) {
        // apologize for poor performance (AES is a pure Perl implementation)
        pl::method(it, &et, "Warn", vec![s("Decryption is very slow for encryption V5.6 or higher"), n(3)])?;
    }
    pl::method(it, &et, "HandleTag", vec![tag_table_ptr, s("P"), encrypt.get("P")])?;

    let parm = Value::new_hash(); // optional parameters extracted from Encrypt dictionary

    if pl::num_eq(&ver, &n(1)) || pl::num_eq(&ver, &n(2)) {
        lset(it, "cryptString", n(1));
        lset(it, "cryptStream", n(1));
    } else if pl::num_eq(&ver, &n(4)) || pl::num_eq(&ver, &n(5)) {
        // initialize our $cryptString and $cryptStream flags
        for k in ["StrF", "StmF"] {
            let flag = if k == "StrF" { "cryptString" } else { "cryptStream" };
            let mut f = encrypt.get(k);
            if f.truthy() && pl::str_eq(&f, "/Identity") {
                f = Value::Undef;
            }
            lset(it, flag, f.clone());
            if f.truthy() && !pl::str_eq(&f, "/StdCF") {
                return ret(cat(&[s("Unsupported "), s(k), s(" encryption "), f]));
            }
        }
        if lget(it, "cryptString").truthy() || lget(it, "cryptStream").truthy() {
            if !(pl::ref_of(&encrypt.get("CF")) == "HASH" && pl::ref_of(&encrypt.path(&["CF", "StdCF"])) == "HASH" && encrypt.path(&["CF", "StdCF", "CFM"]).truthy()) {
                return ret(s("Missing or invalid Encrypt StdCF entry"));
            }
            let crypt_meth = encrypt.path(&["CF", "StdCF", "CFM"]);
            if !re(r"^\/(V2|AESV2|AESV3)$", "").is_match(&crypt_meth) {
                return ret(cat(&[s("Unsupported encryption method "), crypt_meth]));
            }
            // set "_aesv2" or "_aesv3" flag in %$encrypt hash if AES encryption was used
            if let Some(m) = re(r"^\/(AESV2|AESV3)$", "").m(&crypt_meth) {
                encrypt.set(cat(&[s("_"), pl::lc(&cap(&m, 1))]).to_bytes(), n(1));
            }
        }
        if pl::num_eq(&ver, &n(5)) {
            // validate OE and UE entries
            for k in ["OE", "UE"] {
                if !encrypt.get(k).truthy() {
                    return ret(cat(&[s("Missing Encrypt "), s(k), s(" entry")]));
                }
                let v = read_value(it, encrypt.get(k))?;
                parm.set(k, v.clone());
                if !pl::num_eq(&pl::length(&v), &n(32)) {
                    return ret(cat(&[s("Invalid Encrypt "), s(k), s(" entry")]));
                }
            }
            runtime::require(it, "Image::ExifTool::AES")?; // will need this later
        }
    } else {
        return ret(cat(&[s("Encryption version "), ver, s(" currently not supported")]));
    }
    if !id.truthy() {
        return ret(s("Can't decrypt (no document ID)"));
    }

    // make sure we have the necessary libraries available
    if pl::lt(&ver, &n(5)) {
        if pl::eval_block(it, |it| runtime::require(it, "Digest::MD5").map(|_| n(1)))?.is_none() {
            return ret(s("Install Digest::MD5 to process encrypted PDF"));
        }
    } else if pl::eval_block(it, |it| runtime::require(it, "Digest::SHA").map(|_| n(1)))?.is_none() {
        return ret(s("Install Digest::SHA to process AES-256 encrypted PDF"));
    }

    // calculate file-level en/decryption key
    let pad = Value::bytes(b"\x28\xBF\x4E\x5E\x4E\x75\x8A\x41\x64\x00\x4E\x56\xFF\xFA\x01\x08\x2E\x2E\x00\xB6\xD0\x68\x3E\x80\x2F\x0C\xA9\xFE\x64\x53\x69\x7A");
    let o = read_value(it, encrypt.get("O"))?;
    let u = read_value(it, encrypt.get("U"))?;

    // set flag indicating whether metadata is encrypted
    // (in version 4 and higher, metadata streams may not be encrypted)
    let em = encrypt.get("EncryptMetadata");
    if pl::lt(&ver, &n(4)) || !em.truthy() || !re("false", "i").is_match(&em) {
        encrypt.set("_meta", n(1));
    }
    // try no password first, then try provided password if available
    let mut key: Value;
    let mut try_ = 0;
    loop {
        let mut password;
        if try_ == 0 {
            password = s("");
        } else if try_ == 1 {
            password = pl::method(it, &et, "Options", vec![s("Password")])?;
            if password.is_undef() {
                return ret(s("Document is password protected (use Password option)"));
            }
            // make sure there is no UTF-8 flag on the password
            let encode_hangs = et.path(&["OPTIONS", "EncodeHangs"]);
            let repack = pl::ge(&it.get_scalar("main::]"), &Value::Num(5.006))
                && (encode_hangs.truthy() || {
                    let pw = password.clone();
                    let r = pl::eval_block(it, |it| {
                        runtime::require(it, "Encode")?;
                        pl::call(it, "Encode::is_utf8", vec![pw])
                    })?;
                    r.is_some_and(|v| v.truthy())
                } || it.get_scalar("main::@").truthy());
            if repack {
                // local $SIG{'__WARN__'} = sub { };
                // (building the empty sub must not touch $@, read below)
                let saved_err = it.get_scalar("main::@");
                let noop = match it.eval_string(b"sub { }", MODULE, &crate::interp::Scope::new(None), Want::Scalar) {
                    Ok(v) => one(v),
                    Err(e) => return Err(crate::interp::Flow::Die(e)),
                };
                it.set_scalar("main::@", saved_err);
                let pw = password.clone();
                password = pl::local_sig_warn(it, noop, |it| {
                    // repack by hand if Encode isn't available
                    if et.path(&["OPTIONS", "EncodeHangs"]).truthy() || it.get_scalar("main::@").truthy() {
                        let t = if pl::lt(&it.get_scalar("main::]"), &Value::Num(5.010000)) { "U0C*" } else { "C0C*" };
                        let codes = pl::unpack(t, &pw)?;
                        pl::pack("C*", &codes)
                    } else {
                        pl::call(it, "Encode::encode", vec![s("utf8"), pw])
                    }
                })?;
            }
        } else {
            return ret(s("Incorrect password"));
        }
        if pl::lt(&ver, &n(5)) {
            if pl::length(&password).truthy() {
                // password must be encoding in PDFDocEncoding (ref iso32000)
                password = pl::method(it, &et, "Encode", vec![password, s("PDFDoc")])?;
                // truncate or pad the password to exactly 32 bytes
                let pl_ = password.to_bytes().len() as i64;
                if pl_ > 32 {
                    password = pl::substr(&password, 0, Some(32));
                } else if pl_ < 32 {
                    password = pl::concat(&password, &pl::substr(&pad, 0, Some(32 - pl_)));
                }
            } else {
                password = pad.clone();
            }
            key = cat(&[password, o.clone(), pl::pack("V", &[encrypt.get("P")])?, id.clone()]);
            let mut rep = 1;
            if pl::num_eq(&rev, &n(3)) || pl::num_eq(&rev, &n(4)) {
                // must add this if metadata not encrypted
                if !encrypt.get("_meta").truthy() {
                    key = pl::concat(&key, &Value::bytes(b"\xff\xff\xff\xff"));
                }
                rep += 50; // repeat MD5 50 more times if revision is 3 or greater
            }
            let len;
            if pl::num_eq(&ver, &n(1)) {
                len = n(5);
            } else {
                let l = if encrypt.get("Length").truthy() { encrypt.get("Length") } else { n(40) };
                if !pl::ge(&l, &n(40)) {
                    return ret(s("Bad Encrypt Length"));
                }
                len = pl::int(&pl::div(&l, &n(8))?);
            }
            for _ in 0..rep {
                let md = pl::call(it, "Digest::MD5::md5", vec![key])?;
                key = pl::substr(&md, 0, Some(len.to_i64()));
            }
            // decrypt U to see if a user password is required
            let dat;
            if pl::ge(&rev, &n(3)) {
                let dr = pl::sref(pl::call(it, "Digest::MD5::md5", vec![pl::concat(&pad, &id)])?);
                rc4_crypt(it, vec![dr.clone(), key.clone()], Want::Void)?;
                for i in 1..=19i64 {
                    let k: Vec<Value> = key.to_bytes().iter().map(|&b| n(b as i64 ^ i)).collect();
                    let kp = pl::pack("C*", &k)?;
                    rc4_crypt(it, vec![dr.clone(), kp], Want::Void)?;
                }
                dat = pl::concat(&dr.deref(), &pl::substr(&u, 16, None));
            } else {
                let dr = pl::sref(pad.clone());
                rc4_crypt(it, vec![dr.clone(), key.clone()], Want::Void)?;
                dat = dr.deref();
            }
            if dat.to_bytes() == u.to_bytes() {
                break; // all done if this was the correct key
            }
        } else {
            if (o.to_bytes().len() as i64) < 48 || (u.to_bytes().len() as i64) < 48 {
                return ret(s("Invalid O or U Encrypt entries"));
            }
            if pl::length(&password).truthy() {
                // Note: this should be good for passwords containing reasonable characters,
                // but to be bullet-proof we need to apply the SASLprep (IETF RFC 4013) profile
                // of stringprep (IETF RFC 3454) to the password before encoding in UTF-8
                password = pl::method(it, &et, "Encode", vec![password, s("UTF8")])?;
                if password.to_bytes().len() > 127 {
                    password = pl::substr(&password, 0, Some(127));
                }
            }
            // test for the owner password
            let u48 = pl::substr(&u, 0, Some(48));
            let sha = hash(it, &password, pl::substr(&o, 32, Some(8)), u48.clone(), &rev)?;
            if sha.to_bytes() == pl::substr(&o, 0, Some(32)).to_bytes() {
                key = hash(it, &password, pl::substr(&o, 40, Some(8)), u48, &rev)?;
                let dat = pl::sref(pl::concat(&Value::bytes([0u8; 16]), &parm.get("OE")));
                // decrypt with no padding
                let err = pl::call(it, "Image::ExifTool::AES::Crypt", vec![dat.clone(), key.clone(), n(0), n(1)])?;
                if err.truthy() {
                    return ret(err);
                }
                key = dat.deref(); // use this as the file decryption key
                break;
            }
            // test for the user password
            let sha = hash(it, &password, pl::substr(&u, 32, Some(8)), s(""), &rev)?;
            if sha.to_bytes() == pl::substr(&u, 0, Some(32)).to_bytes() {
                key = hash(it, &password, pl::substr(&u, 40, Some(8)), s(""), &rev)?;
                let dat = pl::sref(pl::concat(&Value::bytes([0u8; 16]), &parm.get("UE")));
                let err = pl::call(it, "Image::ExifTool::AES::Crypt", vec![dat.clone(), key.clone(), n(0), n(1)])?;
                if err.truthy() {
                    return ret(err);
                }
                key = dat.deref(); // use this as the file decryption key
                break;
            }
        }
        try_ += 1;
    }
    encrypt.set("_key", key); // save the file-level encryption key
    lset(it, "cryptInfo", encrypt); // save reference to the file-level Encrypt object
    pl::ret_undef() // success!
}

/// Decrypt/Encrypt data
/// Inputs: 0) data ref
///         1) PDF object reference to use as crypt key extension (may be 'none' to
///            avoid extending the encryption key, as for streams with Crypt Filter)
///         2) encrypt flag (false for decryption)
pub fn crypt(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let crypt_info = lget(it, "cryptInfo");
    if !crypt_info.truthy() {
        return pl::ret_nothing(want);
    }
    let (data_pt, key_ext, encrypt) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    // do not decrypt if the key extension object is undefined
    // (this doubles as a flag to disable decryption/encryption)
    if key_ext.is_undef() {
        return pl::ret_nothing(want);
    }
    let mut key = crypt_info.get("_key");
    // apply the necessary crypt key extension
    if !crypt_info.get("_aesv3").truthy() {
        if !pl::str_eq(&key_ext, "none") {
            // extend crypt key using object and generation number
            let Some(m) = re(r"^(I\d+ )?(\d+) (\d+)", "").m(&key_ext) else {
                crypt_info.set("_error", s("Invalid object reference for encryption"));
                return pl::ret_nothing(want);
            };
            let a2 = pl::substr(&pl::pack("V", &[cap(&m, 2)])?, 0, Some(3));
            let a3 = pl::substr(&pl::pack("V", &[cap(&m, 3)])?, 0, Some(2));
            key = cat(&[key, a2, a3]);
        }
        // add AES-128 salt if necessary (this little gem is conveniently
        // omitted from the Adobe PDF 1.6 documentation, causing me to
        // waste 12 hours trying to figure out why this wasn't working --
        // it appears in ISO32000 though, so I should have been using that)
        if crypt_info.get("_aesv2").truthy() {
            key = pl::concat(&key, &s("sAlT"));
        }
        let len = key.to_bytes().len() as i64;
        key = pl::call(it, "Digest::MD5::md5", vec![key])?; // get 16-byte MD5 digest
        if len < 16 {
            key = pl::substr(&key, 0, Some(len)); // trim if necessary
        }
    }
    // perform the decryption/encryption
    if crypt_info.get("_aesv2").truthy() || crypt_info.get("_aesv3").truthy() {
        runtime::require(it, "Image::ExifTool::AES")?;
        let err = pl::call(it, "Image::ExifTool::AES::Crypt", vec![data_pt, key, encrypt])?;
        if err.truthy() {
            crypt_info.set("_error", err.clone());
        }
        ret(err)
    } else {
        rc4_crypt(it, vec![data_pt, key], want)
    }
}

/// Decrypt/Encrypt stream data
/// Inputs: 0) dictionary ref, 1) PDF object reference to use as crypt key extension
pub fn crypt_stream(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    if !lget(it, "cryptStream").truthy() {
        return pl::ret_nothing(want);
    }
    let (dict, key_ext) = (arg(&a, 0), arg(&a, 1));
    let typ = if dict.get("Type").truthy() { dict.get("Type") } else { s("") };
    // XRef streams are not encrypted (ref 3, page 50),
    // and Metadata may or may not be encrypted
    let crypt_info = lget(it, "cryptInfo");
    if crypt_info.truthy() && !pl::str_eq(&typ, "/XRef") && (crypt_info.get("_meta").truthy() || !pl::str_eq(&typ, "/Metadata")) {
        crypt(it, vec![pl::elem_ref(&dict, b"_stream"), key_ext, dict.get("_decrypted")], Want::Void)?;
        // toggle _decrypted flag
        let v = if dict.get("_decrypted").truthy() { Value::Undef } else { n(1) };
        dict.set("_decrypted", v.clone());
        ret(v)
    } else {
        dict.set("_decrypted", n(0)); // stream should never be encrypted
        ret(n(0))
    }
}

/// Generate a new PDF tag (based on its ID) and add it to a tag table
/// Inputs: 0) tag table ref, 1) tag ID
/// Returns: tag info ref
pub fn new_pdf_tag(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (tag_table_ptr, tag) = (arg(&a, 0), arg(&a, 1));
    let mut name = tag.clone();
    // translate URL-like escape sequences
    re(r"#([0-9a-f]{2})", "i").subst(&mut name, true, |c| vec![u8::from_str_radix(&cap(c, 1).to_text(), 16).unwrap_or(0)]);
    re(r"[^-\w]+", "").subst(&mut name, true, |_| b"_".to_vec()); // translate invalid characters to an underline
    re(r"(^|_)([a-z])", "").subst(&mut name, true, |c| cap(c, 2).to_bytes().to_ascii_uppercase()); // start words with upper case
    let tag_info = Value::new_hash();
    tag_info.set("Name", name);
    pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_table_ptr, tag, tag_info.clone()])?;
    ret(tag_info)
}

/// Process AcroForm dictionary to set HasXMLFormsArchitecture flag
/// Inputs: Same as ProcessDict
pub fn process_acro_form(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, tag_table_ptr, dict) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let xfa = if dict.get("XFA").truthy() { s("true") } else { s("false") };
    pl::method(it, &et, "HandleTag", vec![tag_table_ptr, s("_has_xfa"), xfa])?;
    if !pl::method(it, &et, "Options", vec![s("Verbose")])?.truthy() {
        return ret(n(1));
    }
    process_dict(it, a, want)
}

/// Process AF dictionary to extract C2PA manifest
/// Inputs: Same as ProcessDict
pub fn process_af(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, dict) = (arg(&a, 0), arg(&a, 2));
    let afr = dict.get("AFRelationship");
    et.set("AFRelationship", if afr.truthy() { afr } else { s("") });
    // go no further unless Verbose or this is the C2PA_Manifest item
    if !(pl::method(it, &et, "Options", vec![s("Verbose")])?.truthy() || pl::str_eq(&et.get("AFRelationship"), "/C2PA_Manifest")) {
        return ret(n(1));
    }
    process_dict(it, a, want)
}

/// Expand array into a string
/// Inputs: 0) array ref
/// Return: string
pub fn expand_array(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let val = arg(&a, 0);
    let mut list = aderef(&val)?;
    for x in list.iter_mut() {
        if pl::ref_of(x) == "SCALAR" {
            *x = cat(&[s("ref("), x.deref(), s(")")]);
            continue;
        }
        if pl::ref_of(x) == "ARRAY" {
            *x = one(expand_array(it, vec![x.clone()], Want::Scalar)?);
            continue;
        }
        if x.is_undef() {
            *x = s("<undef>");
            continue;
        }
    }
    ret(cat(&[s("["), pl::join(",", &list), s("]")]))
}

/// Process PDF dictionary extract tag values
/// Inputs: 0) ExifTool object reference, 1) tag table reference
///         2) dictionary reference, 3) cross-reference table reference,
///         4) nesting depth, 5) dictionary capture type
pub fn process_dict(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    // (local $_: the loops below alias nothing the callees see)
    let (et, mut tag_table_ptr, mut dict, xref, nesting, typ) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4), arg(&a, 5));
    let verbose = pl::method(it, &et, "Options", vec![s("Verbose")])?;
    let unknown = tag_table_ptr.get("EXTRACT_UNKNOWN");
    let embedded = !unknown.is_undef() && !unknown.truthy() && pl::method(it, &et, "Options", vec![s("ExtractEmbedded")])?.truthy();
    let mut tags: Vec<Value> = aderef(&dict.get("_tags"))?;
    tags.reverse(); // (shifted from the end)
    let mut next = Value::Undef;
    let mut join: BTreeMap<Vec<u8>, Vec<Value>> = BTreeMap::new();
    let mut index = 0i64;
    let fetched = lexh(it, "fetched");

    let nesting = pl::add(&if nesting.truthy() { nesting } else { n(0) }, &n(1));
    if pl::gt(&nesting, &n(50)) {
        warn(it, &et, s("Nesting too deep (directory ignored)"))?;
        return pl::ret_nothing(want);
    }
    // save entire dictionary for rewriting if specified
    if et.get("PDF_CAPTURE").truthy() && tag_table_ptr.get("VARS").truthy() && tag_table_ptr.path(&["VARS", "CAPTURE"]).truthy() {
        for name in aderef(&tag_table_ptr.path(&["VARS", "CAPTURE"]))? {
            if et.get("PDF_CAPTURE").get(name.to_bytes()).truthy() {
                continue;
            }
            // make sure we load the right type if indicated
            if typ.truthy() && typ.to_bytes() != name.to_bytes() {
                continue;
            }
            et.get("PDF_CAPTURE").set(name.to_bytes(), dict.clone());
            break;
        }
    }
    let info_table = pl::pkg_hash(it, "Image::ExifTool::PDF::Info");
    let valid_info = pl::method(it, &et, "Options", vec![s("Validate")])?.truthy() && tag_table_ptr.same_ref(&info_table);
    //
    // extract information from all tags in the dictionary
    //
    'tags: loop {
        let tag;
        let mut is_sub_doc = false;
        if let Some(t) = tags.pop() {
            tag = t;
        } else if !next.is_undef() && !next.truthy() {
            tag = s("Next");
            next = n(1);
        } else {
            break;
        }
        let mut val = dict.get(tag.to_bytes());
        let mut tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
        if tag_info.truthy() {
            if tag_info.get("NoProcess").truthy() {
                tag_info = Value::Undef;
            }
        } else if embedded {
            if let Some(m) = re(r"^(.*?)(\d+)$", "").m(&tag) {
                let (name, num) = (cap(&m, 1), cap(&m, 2));
                if tag_table_ptr.get(name.to_bytes()).truthy() && (pl::ref_of(&val) != "SCALAR" || !fetched.get(sderef(&val)?.to_bytes()).truthy()) {
                    tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), name.clone()])?;
                    if pl::ref_of(&tag_info) == "HASH" && tag_info.get("JoinStreams").truthy() {
                        let vr = sderef(&val)?;
                        fetched.set(vr.to_bytes(), n(1));
                        let obj = fetch(it, &et, &vr, &xref, &tag)?;
                        join.entry(name.to_bytes().to_vec()).or_default();
                        if !(pl::ref_of(&obj) == "HASH" && obj.get("_stream").truthy()) {
                            continue 'tags;
                        }
                        // save all the stream data to join later
                        decode(it, &et, &obj)?;
                        let list = join.get_mut(&name.to_bytes()[..]).unwrap();
                        let ni = num.to_i64().max(0) as usize;
                        if list.len() <= ni {
                            list.resize(ni + 1, Value::Undef);
                        }
                        list[ni] = obj.get("_stream");
                        tag_info = Value::Undef; // don't process
                    } else {
                        is_sub_doc = true; // treat as a sub-document
                    }
                }
            }
        }
        if valid_info && pl::ge(&et.get("PDFVersion"), &Value::Num(2.0)) && (!tag_info.truthy() || !tag_info.get("PDF2").truthy()) {
            let name = if tag_info.truthy() { cat(&[s(":"), tag_info.get("Name")]) } else { cat(&[s(" Info tag '"), tag.clone(), s("'")]) };
            warn(it, &et, cat(&[s("PDF"), name, s(" is deprecated in PDF 2.0")]))?;
        }
        if verbose.truthy() {
            let (mut val2, extra);
            val2 = Value::Undef;
            if pl::ref_of(&val) == "SCALAR" {
                let vr = val.deref();
                extra = cat(&[s(", indirect object ("), vr.clone(), s(")")]);
                if fetched.get(vr.to_bytes()).truthy() {
                    val2 = cat(&[s("ref("), vr, s(")")]);
                } else if pl::str_eq(&tag, "Next") && !next.truthy() {
                    // handle 'Next' links after all others
                    next = n(0);
                    continue 'tags;
                } else {
                    fetched.set(vr.to_bytes(), n(1));
                    val = fetch(it, &et, &vr, &xref, &tag)?;
                    if val.is_undef() {
                        let str_;
                        if !lget(it, "lastOffset").is_undef() {
                            val2 = s("<free>");
                            str_ = s("Object was freed");
                        } else {
                            val2 = s("<err>");
                            str_ = s("Error reading object");
                        }
                        pl::method(it, &et, "VPrint", vec![n(0), cat(&[et.get("INDENT"), str_, s(":\n")])])?;
                    }
                }
            } else if pl::ref_of(&val) == "HASH" {
                extra = s(", direct dictionary");
            } else if pl::ref_of(&val) == "ARRAY" {
                extra = cat(&[s(", direct array of "), n(val.len() as i64), s(" objects")]);
            } else {
                extra = s(", direct object");
            }
            let mut is_subdir = false;
            if pl::ref_of(&val) == "HASH" {
                is_subdir = true;
            } else if pl::ref_of(&val) == "ARRAY" {
                // recurse into objects in arrays only if they are lists of
                // dictionaries or indirect objects which could be dictionaries
                if !val.is_empty() {
                    is_subdir = true;
                }
                for x in val.items() {
                    if pl::ref_of(&x) == "HASH" || pl::ref_of(&x) == "SCALAR" {
                        continue;
                    }
                    is_subdir = false;
                    break;
                }
            }
            if is_subdir {
                // create bogus subdirectory to recurse into this dict
                if !tag_info.truthy() {
                    let sd = Value::new_hash();
                    sd.set("TagTable", s("Image::ExifTool::PDF::Unknown"));
                    tag_info = Value::new_hash();
                    tag_info.set("Name", tag.clone());
                    tag_info.set("SubDirectory", sd);
                }
            } else {
                if pl::ref_of(&val) == "ARRAY" {
                    val2 = one(expand_array(it, vec![val.clone()], Want::Scalar)?);
                }
                // generate tag info if we will use it later
                if !tag_info.truthy() && !val.is_undef() && unknown.truthy() {
                    tag_info = one(new_pdf_tag(it, vec![tag_table_ptr.clone(), tag.clone()], Want::Scalar)?);
                }
            }
            let shown = if val2.truthy() { val2 } else { val.clone() };
            pl::method(it, &et, "VerboseInfo", vec![tag.clone(), tag_info.clone(), s("Value"), shown, s("Extra"), extra, s("Index"), n(index)])?;
            index += 1;
            if val.is_undef() {
                continue 'tags;
            }
        }
        if !tag_info.truthy() {
            // add any tag found in Info dictionary to table
            if !unknown.truthy() {
                continue 'tags;
            }
            tag_info = one(new_pdf_tag(it, vec![tag_table_ptr.clone(), tag.clone()], Want::Scalar)?);
        }
        // increment document number if necessary
        let (mut old_doc_num, mut old_num_tags) = (Value::Undef, Value::Undef);
        if is_sub_doc {
            old_doc_num = et.get("DOC_NUM");
            old_num_tags = et.get("NUM_FOUND");
            let dc = pl::add(&et.get("DOC_COUNT"), &n(1));
            et.set("DOC_COUNT", dc.clone());
            et.set("DOC_NUM", dc);
        }
        if tag_info.get("SubDirectory").truthy() {
            // process the subdirectory
            let mut sub_dicts: Vec<Value>;
            if pl::ref_of(&val) == "ARRAY" {
                // hack to convert array to dictionary if necessary
                if tag_info.get("ConvertToDict").truthy() && val.len() == 2 && !val.at(0).is_ref() {
                    let mut tg = val.at(0);
                    re(r"^/", "").subst(&mut tg, false, |_| Vec::new()); // remove name
                    let d = Value::new_hash();
                    d.set("_tags", Value::new_array(vec![tg.clone()]));
                    d.set(tg.to_bytes(), val.at(1));
                    sub_dicts = vec![d];
                } else {
                    sub_dicts = val.items();
                }
            } else {
                sub_dicts = vec![val.clone()];
            }
            sub_dicts.reverse(); // (shifted from the end)
            // loop through all values of this tag
            loop {
                let mut sub_dict = sub_dicts.pop().unwrap_or_default();
                if !sub_dict.truthy() {
                    break;
                }
                // save last fetched object in case we fetch another one here
                let prev_fetched = last_fetched(it);
                if pl::ref_of(&sub_dict) == "SCALAR" {
                    let sr = sub_dict.deref();
                    // only fetch once (other copies are obsolete)
                    if fetched.get(sr.to_bytes()).truthy() {
                        continue;
                    }
                    if tag_info.get("IgnoreDuplicates").truthy() {
                        let flag = cat(&[s("ProcessedPDF_"), tag.clone()]);
                        if et.get(flag.to_bytes()).truthy() {
                            if pl::method(it, &et, "Warn", vec![cat(&[s("Ignored duplicate "), tag.clone(), s(" dictionary")]), n(2)])?.truthy() {
                                continue;
                            }
                        } else {
                            et.set(flag.to_bytes(), n(1));
                        }
                    }
                    // load dictionary via an indirect reference
                    fetched.set(sr.to_bytes(), n(1));
                    let obj = fetch(it, &et, &sr, &xref, &tag)?;
                    if obj.is_undef() {
                        if lget(it, "lastOffset").is_undef() {
                            warn(it, &et, cat(&[s("Error reading "), tag.clone(), s(" object ("), sr, s(")")]))?;
                        }
                        continue;
                    }
                    sub_dict = obj;
                }
                if pl::ref_of(&sub_dict) == "ARRAY" {
                    // convert array of key/value pairs to a hash
                    if sub_dict.len() < 2 {
                        continue;
                    }
                    let hash = Value::new_hash();
                    let ht = Value::new_array(Vec::new());
                    hash.set("_tags", ht.clone());
                    if let Value::ArrayRef(arr) = &sub_dict {
                        while arr.borrow().len() >= 2 {
                            let mut key = arr.borrow_mut().remove(0);
                            re(r"^\/", "").subst(&mut key, false, |_| Vec::new());
                            ht.push(key.clone());
                            let v = arr.borrow_mut().remove(0);
                            hash.set(key.to_bytes(), v);
                        }
                    }
                    sub_dict = hash;
                } else if pl::ref_of(&sub_dict) != "HASH" {
                    continue;
                }
                // set flag to re-crypt all strings when rewriting if the dictionary
                // came from an encrypted stream
                if !last_fetched(it).truthy() {
                    sub_dict.get_or_new_hash("_needCrypt").set("*", n(1));
                }
                let sub_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![tag_info.path(&["SubDirectory", "TagTable"])])?;
                if !verbose.truthy() {
                    let proc_ = sub_table_ptr.get("PROCESS_PROC");
                    let args = vec![et.clone(), sub_table_ptr.clone(), sub_dict.clone(), xref.clone(), nesting.clone()];
                    if proc_.truthy() {
                        pl::call_code(it, &proc_, args, Want::Scalar)?;
                    } else {
                        process_dict(it, args, Want::Scalar)?;
                    }
                } else if next.truthy() {
                    // handle 'Next' links at this level to avoid deep recursion
                    next = Value::Undef;
                    index = 0;
                    tag_table_ptr = sub_table_ptr;
                    dict = sub_dict.clone();
                    tags = aderef(&sub_dict.get("_tags"))?;
                    let nt = tags.len();
                    tags.reverse(); // (shifted from the end)
                    pl::method(it, &et, "VerboseDir", vec![tag.clone(), n(nt as i64)])?;
                } else {
                    let old_indent = et.get("INDENT");
                    let old_dir = et.get("DIR_NAME");
                    et.set("INDENT", pl::concat(&old_indent, &s("| ")));
                    et.set("DIR_NAME", tag.clone());
                    let nt = aderef(&sub_dict.get("_tags"))?.len();
                    pl::method(it, &et, "VerboseDir", vec![tag.clone(), n(nt as i64)])?;
                    let proc_ = sub_table_ptr.get("PROCESS_PROC");
                    let args = vec![et.clone(), sub_table_ptr.clone(), sub_dict.clone(), xref.clone(), nesting.clone()];
                    if proc_.truthy() {
                        pl::call_code(it, &proc_, args, Want::Scalar)?;
                    } else {
                        process_dict(it, args, Want::Scalar)?;
                    }
                    et.set("INDENT", old_indent);
                    et.set("DIR_NAME", old_dir);
                }
                set_last_fetched(it, prev_fetched);
            }
        } else {
            // fetch object if necessary
            // (OS X 10.6 writes indirect objects in the Info dictionary!)
            if pl::ref_of(&val) == "SCALAR" {
                let prev_fetched = last_fetched(it);
                // (note: fetching the same object multiple times is OK here)
                val = fetch(it, &et, &val.deref(), &xref, &tag)?;
                if !val.is_undef() {
                    val = read_value(it, val)?;
                    // set flag to re-encrypt if necessary if rewritten
                    if lget(it, "cryptString").truthy() {
                        let f = if last_fetched(it).truthy() { n(0) } else { n(1) };
                        dict.get_or_new_hash("_needCrypt").set(tag.to_bytes(), f);
                    }
                    set_last_fetched(it, prev_fetched); // restore last fetched object reference
                }
            } else {
                val = read_value(it, val)?;
            }
            if val.is_ref() {
                if pl::ref_of(&val) == "ARRAY" {
                    if tag_info.get("List").truthy() {
                        et.get_or_new_hash("LIST_TAGS").delete(tag_info.to_bytes());
                    }
                    for v in val.items() {
                        pl::method(it, &et, "FoundTag", vec![tag_info.clone(), v])?;
                    }
                }
            } else if !val.is_undef() {
                // convert from UTF-16 (big endian) to UTF-8 or Latin if necessary
                // unless this is binary data (hex-encoded strings would not have been converted)
                let format = [tag_info.get("Format"), tag_info.get("Writable")].into_iter().find(|v| v.truthy()).unwrap_or_else(|| s("string"));
                if pl::str_eq(&format, "date") {
                    val = one(convert_pdf_date(it, vec![val], Want::Scalar)?);
                }
                if !tag_info.get("Binary").truthy() && re(r"[\x18-\x1f\x80-\xff]", "").is_match(&val) {
                    // text string is already in Unicode if it starts with "\xfe\xff",
                    // otherwise we must first convert from PDFDocEncoding
                    let cs = if re(r"^\xfe\xff", "").subst(&mut val, false, |_| Vec::new()) > 0 { s("UTF16") } else { s("PDFDoc") };
                    val = pl::method(it, &et, "Decode", vec![val, cs, s("MM")])?;
                }
                if tag_info.get("List").truthy() && !et.path(&["OPTIONS", "NoPDFList"]).truthy() {
                    // separate tokens in comma or whitespace delimited lists
                    let comma = val.to_bytes().iter().filter(|&&b| b == b',').count();
                    let semi = val.to_bytes().iter().filter(|&&b| b == b';').count();
                    let values = if comma != 0 || semi != 0 {
                        let split = if comma > semi { r",+\s*" } else { r";+\s*" };
                        pl::split(&re(split, ""), &val, 0)
                    } else {
                        pl::split_ws(&val)
                    };
                    for v in values {
                        pl::method(it, &et, "FoundTag", vec![tag_info.clone(), v])?;
                    }
                } else {
                    // a simple tag value
                    pl::method(it, &et, "FoundTag", vec![tag_info.clone(), val.clone()])?;
                }
            }
        }
        if is_sub_doc {
            // restore original document number
            et.set("DOC_NUM", old_doc_num);
            if pl::num_eq(&old_num_tags, &et.get("NUM_FOUND")) {
                et.set("DOC_COUNT", pl::sub(&et.get("DOC_COUNT"), &n(1)));
            }
        }
    }
    //
    // extract information from joined streams if necessary
    //

    if !join.is_empty() {
        for (tag, list) in join.iter_mut() {
            let tag = Value::bytes(tag);
            let l1 = list.get(1).cloned().unwrap_or_default();
            if l1.is_undef() {
                break;
            }
            let Some(m) = re(r"^%.*?([\x0d\x0a]*)", "").m(&l1) else { break };
            let mut buff = cat(&[s("%!PS-Adobe-3.0"), cap(&m, 1)]); // add PS header with same line break
            let mut i = 1;
            while list.get(i).is_some_and(|v| !v.is_undef()) {
                buff = pl::concat(&buff, &list[i]);
                list[i] = Value::Undef; // free memory
                i += 1;
            }
            // increment document number for tags extracted from embedded EPS
            let old_doc_num = et.get("DOC_NUM");
            let old_num_tags = et.get("NUM_FOUND");
            let dc = pl::add(&et.get("DOC_COUNT"), &n(1));
            et.set("DOC_COUNT", dc.clone());
            et.set("DOC_NUM", dc);
            // extract PostScript information
            pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tag, buff])?;
            et.set("DOC_NUM", old_doc_num);
            // revert document counter if we didn't add any new tags
            if pl::num_eq(&old_num_tags, &et.get("NUM_FOUND")) {
                et.set("DOC_COUNT", pl::sub(&et.get("DOC_COUNT"), &n(1)));
            }
            et.delete("DOC_NUM");
        }
    }
    //
    // extract information from stream object if it exists (eg. Metadata stream)
    //
    #[allow(clippy::never_loop)]
    loop {
        // (cheap goto)
        if !dict.get("_stream").truthy() {
            break;
        }
        let mut tag = s("_stream");
        // add Subtype (if it exists) to stream name and remove leading '/'
        if dict.get("Subtype").truthy() {
            tag = pl::concat(&dict.get("Subtype"), &tag);
            re(r"^\/", "").subst(&mut tag, false, |_| Vec::new());
        }
        if !tag_table_ptr.get(tag.to_bytes()).truthy() {
            break;
        }
        let tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
        if !tag_info.truthy() {
            break;
        }
        let subdir = tag_info.get("SubDirectory");
        if !subdir.truthy() {
            // don't build filter lists across different images
            et.get_or_new_hash("LIST_TAGS").delete(tag_table_ptr.get("Filter").to_bytes());
            // we arrive here only when extracting embedded images
            // - only extract known image types and ignore others
            let mut filter = if dict.get("Filter").truthy() { dict.get("Filter") } else { s("") };
            if pl::ref_of(&filter) == "ARRAY" {
                filter = filter.at(-1); // (get last Filter type)
            }
            let mut result = Value::Undef;
            if pl::str_eq(&filter, "/DCTDecode") || pl::str_eq(&filter, "/JPXDecode") {
                if !decode(it, &et, &dict)? {
                    break;
                }
                // save the image itself
                pl::method(it, &et, "FoundTag", vec![tag_info.clone(), pl::elem_ref(&dict, b"_stream")])?;
                // extract information from embedded image
                let opts = Value::new_hash();
                opts.set("ReEntry", n(1));
                result = pl::method(it, &et, "ExtractInfo", vec![pl::elem_ref(&dict, b"_stream"), opts])?;
            }
            if !result.truthy() {
                let v = if result.is_undef() { s("(unsupported)") } else { s("(unknown)") };
                pl::method(it, &et, "FoundTag", vec![s("FileType"), v])?;
            }
            break;
        }
        // decode stream if necessary
        let crypt_info = lget(it, "cryptInfo");
        if crypt_info.truthy()
            && (crypt_info.get("_aesv2").truthy() || crypt_info.get("_aesv3").truthy() && dict.get("Length").truthy() && pl::gt(&dict.get("Length"), &n(10000)))
            && !dict.get("_decrypted").truthy()
            && !et.get("PDF_CAPTURE").truthy()
        // (capturing PDF for writing?)
        {
            let typ = if dict.get("Type").truthy() { dict.get("Type") } else { s("") };
            if !pl::str_eq(&typ, "/Metadata") || pl::gt(&dict.get("Length"), &n(100000)) {
                if et.path(&["OPTIONS", "IgnoreMinorErrors"]).truthy() {
                    warn(it, &et, cat(&[s("Decrypting large "), tag_info.get("Name"), s(" (will be slow)")]))?;
                } else {
                    pl::method(it, &et, "Warn", vec![cat(&[s("Skipping large AES-encrypted "), tag_info.get("Name")]), n(2)])?;
                    break;
                }
            }
        }
        if !decode(it, &et, &dict)? {
            break;
        }
        if pl::gt(&verbose, &n(2)) {
            pl::method(it, &et, "VPrint", vec![n(2), cat(&[et.get("INDENT"), et.get("DIR_NAME"), s(" stream data\n")])])?;
            pl::method(it, &et, "VerboseDump", vec![pl::elem_ref(&dict, b"_stream")])?;
        }
        // extract information from stream
        let len = n(dict.get("_stream").to_bytes().len() as i64);
        let dir_info = Value::new_hash();
        dir_info.set("DataPt", pl::elem_ref(&dict, b"_stream"));
        dir_info.set("DataLen", len.clone());
        dir_info.set("DirStart", n(0));
        dir_info.set("DirLen", len);
        dir_info.set("Parent", s("PDF"));
        dir_info.set("DirName", subdir.get("DirName"));
        let sub_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![subdir.get("TagTable")])?;
        if !pl::method(it, &et, "ProcessDirectory", vec![dir_info, sub_table_ptr])?.truthy() {
            warn(it, &et, cat(&[s("Error processing "), tag_info.get("Name"), s(" information")]))?;
        }
        break;
    }
    pl::ret_nothing(want)
}

/// Extract information from PDF file
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 0 if not a PDF file, 1 on success, otherwise a negative error number
pub fn read_pdf(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let verbose = pl::method(it, &et, "Options", vec![s("Verbose")])?;
    let mut buff = Value::Undef;
    let (encrypt, id) = (Value::Undef, Value::Undef);
    //
    // validate PDF file
    //
    // (linearization dictionary must be in the first 1024 bytes of the file)
    if !pl::ge(&raf_read(it, &raf, &mut buff, n(1024))?, &n(8)) {
        return ret(n(0));
    }
    let Some(m) = re(r"^(\s*)%PDF-(\d+\.\d+)", "").m(&buff) else { return ret(n(0)) };
    let base = n(cap(&m, 1).to_bytes().len() as i64);
    et.set("PDFBase", base.clone());
    if base.truthy() {
        pl::method(it, &et, "Warn", vec![s("PDF header is not at start of file"), n(1)])?;
    }
    let pdf_ver = cap(&m, 2);
    et.set("PDFVersion", pdf_ver.clone());
    lset(it, "pdfVer", pdf_ver.clone());
    pl::method(it, &et, "SetFileType", vec![])?; // set the FileType tag
    // store PDFVersion tag
    let mut tag_table_ptr = get_tag_table(it, "Image::ExifTool::PDF::Root")?;
    pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), s("Version"), pdf_ver])?;
    tag_table_ptr = get_tag_table(it, "Image::ExifTool::PDF::Main")?;
    //
    // check for a linearized PDF (only if reading)
    //
    let capture = et.get("PDF_CAPTURE");
    if !capture.truthy() {
        let mut lin = s("false");
        let bb = buff.to_bytes();
        if let Some(p) = bb.windows(2).position(|w| w == b"<<") {
            let r = pl::sref(Value::bytes(&bb[p..]));
            let dict = one(extract_object(it, vec![et.clone(), r.clone()], Want::Scalar)?);
            buff = r.deref();
            if pl::ref_of(&dict) == "HASH" && dict.get("Linearized").truthy() && dict.get("L").truthy() {
                let file_size = et.path(&["VALUE", "FileSize"]);
                if !file_size.truthy() {
                    lin = Value::Undef; // can't determine if it is linearized
                } else if pl::num_eq(&dict.get("L"), &pl::sub(&file_size, &et.get("PDFBase"))) {
                    lin = s("true");
                }
            }
        }
        if lin.truthy() {
            pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), s("_linearized"), lin])?;
        }
    }
    //
    // read the xref tables referenced from startxref at the end of the file
    //
    let mut xref_offsets: Vec<Value> = Vec::new();
    if !raf_seek(it, &raf, n(0), 2)? {
        return ret(n(-2));
    }
    // the %%EOF must occur within the last 1024 bytes of the file (PDF spec, appendix H)
    let mut len = pl::method(it, &raf, "Tell", vec![])?;
    if pl::gt(&len, &n(1024)) {
        len = n(1024);
    }
    if !raf_seek(it, &raf, pl::neg(&len), 2)? {
        return ret(n(-2));
    }
    if !pl::num_eq(&raf_read(it, &raf, &mut buff, len.clone())?, &len) {
        return ret(n(-3));
    }
    // find the LAST xref table in the file (may be multiple %%EOF marks,
    // and comments between "startxref" and "%%EOF")
    let Some(m) = re(r"^.*startxref(\s+)(\d+)(\s+)((%[^\x0d\x0a]*\s+)*)%%EOF", "s").m(&buff) else { return ret(n(-4)) };
    // parse comments to read SEAL information
    if cap(&m, 4).truthy() {
        let com = pl::split(&re(r"[\x0d\x0d]+", ""), &cap(&m, 4), 0);
        for c in com {
            let Some(m2) = re(r"^(%+\s*)<seal seal=", "").m(&c) else { continue };
            let dat = pl::substr(&c, cap(&m2, 1).to_bytes().len() as i64, None);
            let tbl = get_tag_table(it, "Image::ExifTool::XMP::SEAL")?;
            let di = Value::new_hash();
            di.set("DataPt", pl::sref(dat));
            pl::method(it, &et, "ProcessDirectory", vec![di, tbl])?;
        }
    }
    let ws = pl::concat(&cap(&m, 1), &cap(&m, 3));
    let xr = cap(&m, 2);
    xref_offsets.push(xr.clone());
    xref_offsets.push(s("Main"));
    // set input record separator
    let nl = match re(r"(\x0d\x0a|\x0d|\x0a)", "").m(&ws) {
        Some(m) => cap(&m, 1),
        None => s("\x0a"),
    };
    pl::local_global(it, b'$', "main::/", |it| {
        it.set_scalar("main::/", nl.clone());
        read_pdf_xref(it, &et, &raf, &verbose, tag_table_ptr, capture, xr, xref_offsets, encrypt, id, nl)
    })
}

/// The rest of ReadPDF, under `local $/`.
#[allow(clippy::too_many_arguments)]
fn read_pdf_xref(it: &mut Interp, et: &Value, raf: &Value, verbose: &Value, tag_table_ptr: Value, capture: Value, xr: Value, mut xref_offsets: Vec<Value>, mut encrypt: Value, mut id: Value, newline: Value) -> ER<Vec<Value>> {
    let mut buff = Value::Undef;
    let xref = Value::new_hash();
    let mut main_dicts: Vec<Value> = Vec::new();
    let loaded = Value::new_hash();
    let mut main_free = Value::Undef;
    let (mut xref_size, mut main_dict_size) = (n(0), n(0));
    // initialize variables to capture when rewriting
    if capture.truthy() {
        capture.set("startxref", xr);
        capture.set("xref", xref.clone());
        capture.set("newline", newline);
        main_free = Value::new_hash();
        capture.set("mainFree", main_free.clone());
    }
    xref_offsets.reverse(); // (shifted from the end)
    'xref: while !xref_offsets.is_empty() {
        let offset = xref_offsets.pop().unwrap_or_default();
        let typ = xref_offsets.pop().unwrap_or_default();
        if loaded.get(offset.to_bytes()).truthy() {
            continue; // avoid infinite recursion
        }
        if !raf_seek(it, raf, pl::add(&offset, &et.get("PDFBase")), 0)? {
            if loaded.is_empty() {
                return ret(n(-5));
            }
            warn(it, et, s("Bad offset for secondary xref table"))?;
            continue;
        }
        // Note: care must be taken because ReadLine may read more than we want if
        // the newline sequence for this table is different than the rest of the file
        loop {
            if !read_line(it, raf, &mut buff)?.truthy() {
                if loaded.is_empty() {
                    return ret(n(-6));
                }
                warn(it, et, s("Bad offset for secondary xref table"))?;
                continue 'xref;
            }
            if re(r"\S", "").is_match(&buff) {
                break; // skip blank lines
            }
        }
        let mut load_xref_stream = false;
        if re(r"^\s*xref\s+", "s").subst(&mut buff, false, |_| Vec::new()) > 0 {
            // load xref table
            loop {
                // read another line if necessary (skipping blank lines)
                while !re(r"\S", "").is_match(&buff) {
                    if !read_line(it, raf, &mut buff)?.truthy() {
                        return ret(n(-6));
                    }
                }
                if re(r"^\s*trailer([\s<[(])", "s").subst(&mut buff, false, |c| cap(c, 1).to_bytes().to_vec()) > 0 {
                    break;
                }
                let (mut start, mut num) = (Value::Undef, Value::Undef);
                if re(r"^\s*(\d+)\s+(\d+)\s+", "s").subst(&mut buff, false, |c| {
                    start = cap(c, 1);
                    num = cap(c, 2);
                    Vec::new()
                }) == 0
                {
                    return ret(n(-4));
                }
                if !raf_seek(it, raf, n(-(buff.to_bytes().len() as i64)), 1)? {
                    return ret(n(-4));
                }
                let mut i = 0i64;
                while pl::lt(&n(i), &num) {
                    if !pl::num_eq(&raf_read(it, raf, &mut buff, n(20))?, &n(20)) {
                        return ret(n(-6));
                    }
                    let Some(m) = re(r"^\s*(\d{10}) (\d{5}) (f|n)", "s").m(&buff) else { return ret(n(-4)) };
                    let num = pl::add(&start, &n(i));
                    i += 1;
                    if pl::gt(&num, &xref_size) {
                        xref_size = num.clone();
                    }
                    // locate object to generate entry from stream if necessary
                    // (must do this before we test $xref{$num})
                    if xref.get("dicts").truthy() {
                        locate_any(it, &xref, &num)?;
                    }
                    // save offset for newest copy of all objects
                    // (or next object number for free objects)
                    if xref.get(num.to_bytes()).is_undef() {
                        let (offset, gen) = (pl::int(&cap(&m, 1)), pl::int(&cap(&m, 2)));
                        xref.set(num.to_bytes(), offset.clone());
                        if pl::str_eq(&cap(&m, 3), "f") {
                            // save free objects in last xref table for rewriting
                            if main_free.truthy() {
                                main_free.set(num.to_bytes(), Value::new_array(vec![offset, gen, s("f")]));
                            }
                            continue;
                        }
                        // also save offset keyed by object reference string
                        xref.set(cat(&[num, s(" "), gen, s(" R")]).to_bytes(), offset);
                    }
                }
                // (I have a sample from Adobe which has an empty xref table)
                // %xref or return -4; # xref table may not be empty
                buff = s("");
            }
            main_free = Value::Undef; // only do this for the last xref table
        } else if re(r"^\s*(\d+)\s+(\d+)\s+obj", "s").subst(&mut buff, false, |_| Vec::new()) > 0 {
            // this is a PDF-1.5 cross-reference stream dictionary
            load_xref_stream = true;
        } else {
            if loaded.is_empty() {
                return ret(n(-4));
            }
            warn(it, et, s("Invalid secondary xref table"))?;
            continue;
        }
        let br = pl::sref(buff.clone());
        let main_dict = one(extract_object(it, vec![et.clone(), br.clone(), raf.clone(), xref.clone()], Want::Scalar)?);
        buff = br.deref();
        if pl::ref_of(&main_dict) != "HASH" {
            if loaded.is_empty() {
                return ret(n(-8));
            }
            warn(it, et, s("Error loading secondary dictionary"))?;
            continue;
        }
        // keep track of total trailer dictionary Size
        if main_dict.get("Size").truthy() && pl::gt(&main_dict.get("Size"), &main_dict_size) {
            main_dict_size = main_dict.get("Size");
        }
        if load_xref_stream {
            // decode and save our XRef stream from PDF-1.5 file
            // (but parse it later as required to save time)
            // Note: this technique can potentially result in an old object
            // being used if the file was incrementally updated and an older
            // object from an xref table was replaced by a newer object in an
            // xref stream.  But doing so isn't a good idea (if allowed at all)
            // because a PDF 1.4 consumer would also make this same mistake.
            if pl::str_eq(&main_dict.get("Type"), "/XRef") && main_dict.get("W").truthy() && aderef(&main_dict.get("W"))?.len() > 2 && main_dict.get("Size").truthy() && decode(it, et, &main_dict)? {
                // create Index entry if it doesn't exist
                if !main_dict.get("Index").truthy() {
                    main_dict.set("Index", Value::new_array(vec![n(0), main_dict.get("Size")]));
                }
                // create '_entry_size' entry for internal use
                let w = main_dict.get("W");
                let mut size = n(0);
                for x in aderef(&w)? {
                    size = pl::add(&size, &x);
                }
                main_dict.set("_entry_size", size);
                // save this stream dictionary to use later if required
                if !xref.get("dicts").truthy() {
                    xref.set("dicts", Value::new_array(Vec::new()));
                }
                xref.get("dicts").push(main_dict.clone());
            } else {
                if loaded.is_empty() {
                    return ret(n(-9));
                }
                warn(it, et, s("Invalid xref stream in secondary dictionary"))?;
            }
        }
        loaded.set(offset.to_bytes(), n(1));
        // load XRef stream in hybrid file if it exists
        if main_dict.get("XRefStm").truthy() {
            xref_offsets.insert(0, main_dict.get("XRefStm"));
            xref_offsets.insert(0, s("XRefStm"));
        }
        if main_dict.get("Encrypt").truthy() {
            encrypt = main_dict.get("Encrypt");
        }
        if encrypt.truthy() && pl::str_eq(&encrypt, "null") {
            encrypt = Value::Undef; // (have seen "null")
        }
        if main_dict.get("ID").truthy() && pl::ref_of(&main_dict.get("ID")) == "ARRAY" {
            id = read_value(it, main_dict.get("ID").at(0))?;
        }
        main_dicts.push(main_dict.clone());
        main_dicts.push(typ);
        // load previous xref table if it exists
        if main_dict.get("Prev").truthy() {
            xref_offsets.insert(0, main_dict.get("Prev"));
            xref_offsets.insert(0, s("Prev"));
        }
    }
    if pl::gt(&xref_size, &main_dict_size) {
        let str_ = cat(&[s("Objects in xref table ("), xref_size, s(") exceed trailer dictionary Size ("), main_dict_size, s(")")]);
        if capture.truthy() {
            pl::method(it, et, "Error", vec![str_])?;
        } else {
            warn(it, et, str_)?;
        }
    }
    //
    // extract encryption information if necessary
    //
    if encrypt.truthy() {
        if pl::ref_of(&encrypt) == "SCALAR" {
            encrypt = fetch(it, et, &encrypt.deref(), &xref, &s("Encrypt"))?;
        }
        // generate Encryption tag information
        let err = one(decrypt_init(it, vec![et.clone(), encrypt.clone(), id.clone()], Want::Scalar)?);
        if err.truthy() {
            warn(it, et, err.clone())?;
            if capture.truthy() {
                capture.set("Error", err);
            }
            return ret(n(-1));
        }
    }
    //
    // extract the information beginning with each of the main dictionaries
    //
    let mut i = 0i64;
    let num = pl::div(&n(main_dicts.len() as i64), &n(2))?;
    main_dicts.reverse(); // (shifted from the end)
    while !main_dicts.is_empty() {
        let dict = main_dicts.pop().unwrap_or_default();
        let typ = main_dicts.pop().unwrap_or_default();
        if verbose.truthy() {
            i += 1;
            let nn = aderef(&dict.get("_tags"))?.len() as i64;
            pl::method(it, et, "VPrint", vec![n(0), cat(&[s("PDF dictionary ("), n(i), s(" of "), num.clone(), s(") with "), n(nn), s(" entries:\n")])])?;
        }
        process_dict(it, vec![et.clone(), tag_table_ptr.clone(), dict, xref.clone(), n(0), typ], Want::Scalar)?;
    }
    // handle any decryption errors
    if encrypt.truthy() {
        let err = encrypt.get("_error");
        if err.truthy() {
            warn(it, et, err.clone())?;
            if capture.truthy() {
                capture.set("Error", err);
            }
            return ret(n(-1));
        }
    }
    ret(n(1))
}

/// Extract information from PDF file
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 if this was a valid PDF file
pub fn process_pdf(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let et = arg(&a, 0);

    lset(it, "cryptInfo", Value::Undef); // (must not delete after returning so writer can use it)
    lset(it, "cryptStream", Value::Undef);
    lset(it, "cryptString", Value::Undef);
    let mut result = one(read_pdf(it, a, Want::Scalar)?);
    if pl::lt(&result, &n(0)) {
        let w = lexh(it, "pdfWarning").get(result.to_bytes());
        if w.truthy() {
            warn(it, &et, w)?;
        }
        result = n(1);
    }
    // clean up and return
    if let Value::HashRef(h) = lexh(it, "streamObjs") {
        h.borrow_mut().clear();
    }
    if let Value::HashRef(h) = lexh(it, "fetched") {
        h.borrow_mut().clear();
    }
    ret(result)
}

pub fn register(it: &mut Interp) {
    // keep PDF.pm's own file lexicals (see LEX)
    for (sigil, name) in LEX_NAMES {
        let b = runtime::lexical(it, *sigil, MODULE, name);
        it.set_global(*sigil, &format!("{LEX}{name}"), b);
    }
    it.define_native("Image::ExifTool::PDF::AUTOLOAD", autoload);
    it.define_native("Image::ExifTool::PDF::ConvertPDFDate", convert_pdf_date);
    it.define_native("Image::ExifTool::PDF::LocateAnyObject", locate_any_object);
    it.define_native("Image::ExifTool::PDF::LocateObject", locate_object);
    it.define_native("Image::ExifTool::PDF::CheckObject", check_object);
    it.define_native("Image::ExifTool::PDF::FetchObject", fetch_object);
    it.define_native("Image::ExifTool::PDF::ReadPDFValue", read_pdf_value);
    it.define_native("Image::ExifTool::PDF::ExtractObject", extract_object);
    it.define_native("Image::ExifTool::PDF::ReadToNested", read_to_nested);
    it.define_native("Image::ExifTool::PDF::DecodeLZW", decode_lzw);
    it.define_native("Image::ExifTool::PDF::DecodeStream", decode_stream);
    it.define_native("Image::ExifTool::PDF::RC4Init", rc4_init);
    it.define_native("Image::ExifTool::PDF::RC4Crypt", rc4_crypt);
    it.define_native("Image::ExifTool::PDF::CipherUpdate", cipher_update);
    it.define_native("Image::ExifTool::PDF::GetHash", get_hash);
    it.define_native("Image::ExifTool::PDF::DecryptInit", decrypt_init);
    it.define_native("Image::ExifTool::PDF::Crypt", crypt);
    it.define_native("Image::ExifTool::PDF::CryptStream", crypt_stream);
    it.define_native("Image::ExifTool::PDF::NewPDFTag", new_pdf_tag);
    it.define_native("Image::ExifTool::PDF::ProcessAcroForm", process_acro_form);
    it.define_native("Image::ExifTool::PDF::ProcessAF", process_af);
    it.define_native("Image::ExifTool::PDF::ExpandArray", expand_array);
    it.define_native("Image::ExifTool::PDF::ProcessDict", process_dict);
    it.define_native("Image::ExifTool::PDF::ReadPDF", read_pdf);
    it.define_native("Image::ExifTool::PDF::ProcessPDF", process_pdf);
}
