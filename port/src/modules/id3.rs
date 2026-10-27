//! Image::ExifTool::ID3 (Image/ExifTool/ID3.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read ID3 meta information
//!
//! ProcessSynText walks `$$dataPt` with `\G` matches whose pos() lives on
//! the caller's scalar: those run as Perl statements (`on_data`). The pos()
//! of the subs' own `my` variables is followed in Rust.

use std::collections::BTreeSet;

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, perl_eval};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

const MODULE: &str = "Image::ExifTool::ID3";

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

fn m(it: &mut Interp, et: &Value, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::method(it, et, name, args)
}

fn warn(it: &mut Interp, et: &Value, msg: Value) -> ER<()> {
    m(it, et, "Warn", vec![msg])?;
    Ok(())
}

fn cat(parts: &[&Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

fn first(r: Vec<Value>) -> Value {
    r.into_iter().last().unwrap_or_default()
}

/// An unsigned integer as perl keeps it (IV when it fits, else UV).
fn uv(v: u64) -> Value {
    if v <= i64::MAX as u64 {
        Value::Int(v as i64)
    } else {
        Value::UInt(v)
    }
}

/// A value as perl's bit operators see it (UV).
fn bits(v: &Value) -> u64 {
    match v {
        Value::UInt(u) => *u,
        Value::Num(f) if *f >= 9.2e18 => *f as u64,
        v => v.to_i64() as u64,
    }
}

/// `$raf->Read($buff, $len)`: the count, with `buff` updated.
fn read(it: &mut Interp, raf: &Value, buff: &mut Value, len: Value) -> ER<Value> {
    let mut args = [buff.clone(), len];
    let r = pl::method_mut(it, raf, "Read", &mut args, Want::Scalar)?;
    *buff = args[0].clone();
    Ok(r.into_iter().next().unwrap_or_default())
}

/// A Perl statement on `$dataPt` (the caller's buffer reference), in list context.
fn on_data(it: &mut Interp, code: &str, data_pt: &Value) -> ER<Vec<Value>> {
    let c = cell(data_pt.clone());
    perl_eval(it, code, MODULE, &[("dataPt", &c)], Want::List)
}

fn lex_hash(it: &mut Interp, name: &str) -> Value {
    crate::runtime::lex_hash(it, MODULE, name)
}

/// `DecodeString($et, $val[, $enc])` in scalar context.
fn decode_string1(it: &mut Interp, et: &Value, val: Value, enc: Option<Value>) -> ER<Value> {
    let mut a = vec![et.clone(), val];
    if let Some(e) = enc {
        a.push(e);
    }
    Ok(first(decode_string(it, a, Want::Scalar)?))
}

/// `DecodeString($et, $val[, $enc])` in list context.
fn decode_string_list(it: &mut Interp, et: &Value, val: Value, enc: Option<Value>) -> ER<Vec<Value>> {
    let mut a = vec![et.clone(), val];
    if let Some(e) = enc {
        a.push(e);
    }
    decode_string(it, a, Want::List)
}

/// Make tag name for user-defined tag
/// Inputs: 0) User defined tag description
/// Returns: Tag name
pub fn make_tag_name(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut name = arg(&a, 0);
    let user_tag_name = lex_hash(it, "userTagName");
    let u = user_tag_name.get(name.to_bytes());
    if u.truthy() {
        return ret(u);
    }
    if !re("[a-z]", "").is_match(&name) {
        name = pl::ucfirst(&pl::lc(&name)); // convert all uppercase to mixed case
    }
    re(r"([a-z])[_ ]([a-z])", "").subst(&mut name, true, |c| cat(&[&cap(c, 1), &pl::uc(&cap(c, 2))]).to_bytes().to_vec());
    ret(call(it, "MakeTagName", vec![name])?)
}

/// Convert ID3v1 text to exiftool character set
/// Inputs: 0) ExifTool object ref, 1) text string
/// Returns: converted text
pub fn convert_id3v1_text(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, val) = (arg(&a, 0), arg(&a, 1));
    let cs = m(it, &et, "Options", vec![pl::s("CharsetID3")])?;
    it.call_method(&et, "Decode", vec![et.clone(), val, cs], want)
}

/// Re-format time stamp in synchronized lyrics
/// Inputs: 0) synchronized lyrics entry (eg. "[84.030]Da do do do")
/// Returns: entry with formatted timestamp (eg. "[01:24.03]Da do do do")
pub fn convert_time_stamp(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let val = arg(&a, 0);
    // do nothing if this isn't a time stamp (frame count doesn't contain a decimal)
    let Some(c) = re(r"^\[(\d+\.\d+)\]", "").m(&val) else { return ret(val) };
    let pos = pl::length(&cap(&c, 0)).to_i64(); // (pos($val) after the //g match)
    let mut time = cap(&c, 1);
    // print hours only if more than 60 minutes
    let mut h = pl::int(&pl::div(&time, &n(3600))?);
    if h.truthy() {
        time = pl::sub(&time, &pl::mul(&h, &n(3600)));
        h = pl::concat(&h, &pl::s(":"));
    } else {
        h = pl::s("");
    }
    let mut mm = pl::int(&pl::div(&time, &n(60))?);
    let s = pl::sub(&time, &pl::mul(&mm, &n(60)));
    let mut ss = pl::sprintf("%05.2f", &[s]);
    if pl::ge(&ss, &n(60)) {
        ss = pl::s("00.00");
        mm = pl::add(&mm, &n(1));
        if pl::ge(&mm, &n(60)) {
            mm = pl::sub(&mm, &n(60));
            h = pl::add(&h, &n(1));
        }
    }
    ret(pl::concat(&pl::sprintf("[%s%.2d:%s]", &[h, mm, ss]), &pl::substr(&val, pos, None)))
}

/// Process ID3 synchronized lyrics/text
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
pub fn process_syn_text(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");

    pl::at(939);
    let data = pl::sderef(&data_pt)?;
    m(it, &et, "VerboseDir", vec![pl::s("SynLyrics"), n(0), pl::length(&data)])?;
    if !pl::gt(&pl::length(&data_pt.deref()), &n(6)) {
        return pl::ret_nothing(want);
    }

    let u = pl::unpack("Ca3CC", &data_pt.deref())?;
    let (enc, mut lang, time_code, type_) = (arg(&u, 0), arg(&u, 1), arg(&u, 2), arg(&u, 3));
    lang = pl::lc(&lang);
    if !re("^[a-z]{3}$", "").is_match(&lang) || lang.eq_str("eng") {
        lang = Value::Undef;
    }
    on_data(it, "pos($$dataPt) = 6", &data_pt)?;
    let (term_len, pat);
    if pl::num_eq(&enc, &n(1)) || pl::num_eq(&enc, &n(2)) {
        if on_data(it, r"($$dataPt =~ /\G(..)*?\0\0/sg) ? (1) : ()", &data_pt)?.is_empty() {
            return pl::ret_nothing(want);
        }
        term_len = 2;
        pat = r"\G(?:..)*?\0\0(....)";
    } else {
        if on_data(it, r"($$dataPt =~ /\0/g) ? (1) : ()", &data_pt)?.is_empty() {
            return pl::ret_nothing(want);
        }
        term_len = 1;
        pat = r"\0(....)";
    }
    let p = first(on_data(it, "pos($$dataPt)", &data_pt)?).to_i64();
    let mut desc = pl::substr(&data_pt.deref(), 6, Some(p - 6 - term_len));
    desc = decode_string1(it, &et, desc, Some(enc.clone()))?;

    let mut tag_info = m(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), pl::s("desc")])?;
    if lang.truthy() {
        tag_info = call(it, "GetLangInfo", vec![tag_info, lang.clone()])?;
    }
    m(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("type"), type_])?;
    m(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("desc"), desc, pl::s("TagInfo"), tag_info])?;
    tag_info = m(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), pl::s("text")])?;
    if lang.truthy() {
        tag_info = call(it, "GetLangInfo", vec![tag_info, lang.clone()])?;
    }

    let code = format!("$$dataPt =~ /{pat}/sg ? ($1, pos($$dataPt)) : ()");
    loop {
        let pos = first(on_data(it, "pos($$dataPt)", &data_pt)?);
        let r = on_data(it, &code, &data_pt)?;
        if r.is_empty() {
            break;
        }
        let time = pl::unpack1("N", &arg(&r, 0))?;
        let np = arg(&r, 1).to_i64();
        let mut text = pl::substr(&data_pt.deref(), pos.to_i64(), Some(np - pos.to_i64() - 4 - term_len));
        text = decode_string1(it, &et, text, Some(enc.clone()))?;
        let time_str;
        if pl::num_eq(&time_code, &n(2)) {
            // time in ms
            time_str = pl::sprintf("%.3f", &[pl::div(&time, &n(1000))?]);
        } else {
            // time in MPEG frames
            let mut t = pl::sprintf("%.4d", &[time]);
            if !pl::num_eq(&time_code, &n(1)) {
                t = pl::concat(&t, &pl::s("?"));
            }
            time_str = t;
        }
        let v = cat(&[&pl::s("["), &time_str, &pl::s("]"), &text]);
        m(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("text"), v, pl::s("TagInfo"), tag_info.clone()])?;
    }
    pl::ret_nothing(want)
}

/// Process ID3 PRIV data
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
pub fn process_private(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let (mut tag, start);
    pl::at(991);
    let data = pl::sderef(&data_pt)?;
    m(it, &et, "VerboseDir", vec![pl::s("PRIV"), n(0), pl::length(&data)])?;
    if let Some(c) = re(r"^(.*?)\0", "s").m(&data_pt.deref()) {
        tag = cap(&c, 1);
        start = pl::add(&pl::length(&tag), &n(1));
    } else {
        tag = pl::s("");
        start = n(0);
    }
    if !tag_table_ptr.get(tag.to_bytes()).truthy() {
        pl::tr(&mut tag, "/ ", "_", "d"); // translate '/' to '_' and remove spaces
        if !re(r"^[-\w]{1,24}$", "").is_match(&tag) {
            tag = pl::s("private");
        }
        if !tag_table_ptr.get(tag.to_bytes()).truthy() {
            let ti = Value::new_hash();
            ti.set("Name", pl::ucfirst(&tag));
            ti.set("Binary", n(1));
            call(it, "AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), ti])?;
        }
    }
    let size = pl::sub(&pl::length(&data_pt.deref()), &start);
    let key = m(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tag, Value::Undef, pl::s("Size"), size, pl::s("Start"), start, pl::s("DataPt"), data_pt.clone()])?;
    // set group1 name
    if key.truthy() {
        return ret(m(it, &et, "SetGroup", vec![key, et.get("ID3_Ver")])?);
    }
    ret(key)
}

/// Print ID3v2 Genre
/// Inputs: TCON or TCO frame data
/// Returns: Content type with decoded genre numbers
pub fn print_genre(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut val = arg(&a, 0);
    let genre = lex_hash(it, "genre");
    // make sure that %genre has an entry for all numbers we are interested in
    // (genre numbers are in brackets for ID3v2.2 and v2.3)
    for c in re(r"\((\d+)\)", "").each(&val) {
        let k = cap(&c, 1);
        if !genre.get(k.to_bytes()).truthy() {
            genre.set(k.to_bytes(), cat(&[&pl::s("Unknown ("), &k, &pl::s(")")]));
        }
    }
    // (genre numbers are separated by nulls in ID3v2.4,
    //  but nulls are converted to '/' by DecodeString())
    for c in re(r"(?:^|\/)(\d+)(\/|$)", "").each(&val) {
        let k = cap(&c, 1);
        if !genre.get(k.to_bytes()).truthy() {
            genre.set(k.to_bytes(), cat(&[&pl::s("Unknown ("), &k, &pl::s(")")]));
        }
    }
    re(r"\((\d+)\)", "").subst(&mut val, true, |c| cat(&[&pl::s("("), &genre.get(cap(c, 1).to_bytes()), &pl::s(")")]).to_bytes().to_vec());
    re(r"(^|\/)(\d+)(?=\/|$)", "").subst(&mut val, true, |c| cat(&[&cap(c, 1), &genre.get(cap(c, 2).to_bytes())]).to_bytes().to_vec());
    re(r"^\(([^)]+)\)\1?$", "").subst(&mut val, false, |c| cap(c, 1).to_bytes().to_vec()); // clean up by removing brackets and duplicates
    ret(val)
}

/// Get Genre ID
/// Inputs: 0) Genre name
/// Returns: genre ID number, or undef
pub fn get_genre_id(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let genre = lex_hash(it, "genre");
    it.call_named("Image::ExifTool::ReverseLookup", vec![arg(&a, 0), genre], want)
}

/// Decode ID3 string
/// Inputs: 0) ExifTool object reference
///         1) string beginning with encoding byte unless specified as argument
///         2) optional encoding (0=ISO-8859-1, 1=UTF-16 BOM, 2=UTF-16BE, 3=UTF-8)
/// Returns: Decoded string in scalar context, or list of strings in list context
pub fn decode_string(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, mut val, mut enc) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if !pl::length(&val).truthy() {
        return ret(pl::s(""));
    }
    if enc.is_undef() {
        enc = pl::unpack1("C", &val)?;
        val = pl::substr(&val, 1, None); // remove encoding byte
    }
    let mut vals: Vec<Value> = Vec::new();
    if pl::num_eq(&enc, &n(0)) || pl::num_eq(&enc, &n(3)) {
        // ISO 8859-1 or UTF-8
        re(r"\0+$", "").subst(&mut val, false, |_| Vec::new()); // remove any null padding
        // (must split before converting because conversion routines truncate at null)
        vals = pl::split(&re(r"\0", ""), &val, 0);
        let cs = pl::s(if enc.truthy() { "UTF8" } else { "Latin" });
        for v in vals.iter_mut() {
            *v = m(it, &et, "Decode", vec![v.clone(), cs.clone()])?;
        }
    } else if pl::num_eq(&enc, &n(1)) || pl::num_eq(&enc, &n(2)) {
        // UTF-16 with BOM, or UTF-16BE
        let mut bom: Vec<u8> = b"\xfe\xff".to_vec();
        loop {
            let mut v;
            // split string at null terminators on word boundaries
            let mut c1 = Value::Undef;
            if re(r"((..)*?)\0\0", "s").subst(&mut val, false, |c| {
                c1 = cap(c, 1);
                Vec::new()
            }) > 0
            {
                v = c1;
            } else {
                if !pl::gt(&pl::length(&val), &n(1)) {
                    break;
                }
                v = val.clone();
                val = pl::s("");
            }
            let mut b1 = None;
            if re(r"^(\xfe\xff|\xff\xfe)", "").subst(&mut v, false, |c| {
                b1 = Some(cap(c, 1).to_bytes().to_vec());
                Vec::new()
            }) > 0
            {
                bom = b1.unwrap_or_default();
            }
            let order = match &bom[..] {
                b"\xfe\xff" => pl::s("MM"),
                b"\xff\xfe" => pl::s("II"),
                _ => Value::Undef,
            };
            vals.push(m(it, &et, "Decode", vec![v, pl::s("UTF16"), order])?);
        }
    } else {
        re(r"\0+$", "").subst(&mut val, false, |_| Vec::new());
        return ret(cat(&[&pl::s("<Unknown encoding "), &enc, &pl::s("> "), &val]));
    }
    if want == Want::List {
        return Ok(vals);
    }
    ret(pl::join("/", &vals))
}

/// Convert sync-safe integer to a number we can use
/// Inputs: 0) int32u sync-safe value
/// Returns: actual number or undef on invalid value
pub fn un_sync_safe(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let val = bits(&arg(&a, 0));
    if val & 0x80808080 != 0 {
        return pl::ret_undef();
    }
    ret(uv((val & 0x0000007f) | ((val & 0x00007f00) >> 1) | ((val & 0x007f0000) >> 2) | ((val & 0x7f000000) >> 3)))
}

fn un_sync_safe1(it: &mut Interp, v: Value) -> ER<Value> {
    Ok(first(un_sync_safe(it, vec![v], Want::Scalar)?))
}

/// Process ID3v2 information
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
pub fn process_id3v2(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let mut offset = dir_info.get("DirStart");
    let size = dir_info.get("DirLen");
    let vers = dir_info.get("Version");
    let verbose = m(it, &et, "Options", vec![pl::s("Verbose")])?;
    let mut len = Value::Undef; // frame data length
    let mut other_table = Value::Undef;
    let data = data_pt.deref();

    m(it, &et, "VerboseDir", vec![tag_table_ptr.get("GROUPS").get("1"), n(0), size.clone()])?;
    m(it, &et, "VerboseDump", vec![data_pt.clone(), pl::s("Len"), size.clone(), pl::s("Start"), offset.clone()])?;

    let mut first_pass = true;
    loop {
        if !first_pass {
            offset = pl::add(&offset, &len);
        }
        first_pass = false;
        let mut id;
        let mut flags = Value::Undef;
        if pl::lt(&vers, &n(0x0300)) {
            // version 2.2 frame header is 6 bytes
            if pl::gt(&pl::add(&offset, &n(6)), &size) {
                break;
            }
            pl::at(1130);
            pl::sderef(&data_pt)?;
            let u = pl::unpack(&format!("x{}a3Cn", offset.to_text()), &data)?;
            id = arg(&u, 0);
            let hi = arg(&u, 1);
            len = arg(&u, 2);
            if id.eq_str("\0\0\0") {
                break;
            }
            len = pl::add(&len, &uv(bits(&hi) << 16));
            offset = pl::add(&offset, &n(6));
        } else {
            // version 2.3/2.4 frame header is 10 bytes
            if pl::gt(&pl::add(&offset, &n(10)), &size) {
                break;
            }
            pl::at(1137);
            pl::sderef(&data_pt)?;
            let u = pl::unpack(&format!("x{}a4Nn", offset.to_text()), &data)?;
            id = arg(&u, 0);
            len = arg(&u, 1);
            flags = arg(&u, 2);
            if id.eq_str("\0\0\0\0") {
                break;
            }
            offset = pl::add(&offset, &n(10));
            // length is a "sync-safe" integer by the ID3v2.4 specification, but
            // reportedly some versions of iTunes write this as a normal integer
            // (ref http://www.id3.org/iTunes)
            #[allow(clippy::never_loop)]
            while pl::ge(&vers, &n(0x0400)) && pl::gt(&len, &n(0x7f)) && bits(&len) & 0x80808080 == 0 {
                let old_len = len.clone();
                len = un_sync_safe1(it, len.clone())?;
                if len.is_undef() || pl::gt(&pl::add(&pl::add(&offset, &len), &n(10)), &size) {
                    if pl::num_eq(&pl::add(&offset, &len), &size) {
                        m(it, &et, "Warn", vec![pl::s("Missing ID3 terminating frame"), n(1)])?;
                    } else {
                        warn(it, &et, pl::s("Invalid ID3 frame size"))?;
                    }
                    break;
                }
                // check next ID to see if it makes sense
                let mut next_id = pl::substr(&data, pl::add(&offset, &len).to_i64(), Some(4));
                if tag_table_ptr.get(next_id.to_bytes()).truthy() {
                    break;
                }
                // try again with the incorrect length word (patch for iTunes bug)
                if pl::gt(&pl::add(&pl::add(&offset, &old_len), &n(10)), &size) {
                    break;
                }
                next_id = pl::substr(&data, pl::add(&offset, &len).to_i64(), Some(4));
                if tag_table_ptr.get(next_id.to_bytes()).truthy() {
                    len = old_len;
                }
                break; // yes, "while" was really a "goto" in disguise
            }
        }
        if pl::gt(&pl::add(&offset, &len), &size) {
            break;
        }
        let mut tag_info = m(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), id.clone()])?;
        if !tag_info.truthy() {
            let other = lex_hash(it, "otherTable").get(tag_table_ptr.to_bytes());
            if !other_table.truthy() && other.truthy() {
                other_table = call(it, "GetTagTable", vec![other])?;
            }
            if other_table.truthy() {
                tag_info = m(it, &et, "GetTagInfo", vec![other_table.clone(), id.clone()])?;
            }
            if tag_info.truthy() {
                m(it, &et, "Warn", vec![cat(&[&pl::s("Frame '"), &id, &pl::s("' is not valid for this ID3 version")]), n(1)])?;
            } else {
                if !(verbose.truthy() || m(it, &et, "Options", vec![pl::s("Unknown")])?.truthy()) {
                    continue;
                }
                pl::tr(&mut id, "-A-Za-z0-9_", "", "dc");
                if !pl::length(&id).truthy() {
                    id = pl::s("unknown");
                }
                if !tag_table_ptr.get(id.to_bytes()).truthy() {
                    tag_info = Value::new_hash();
                    tag_info.set("Name", pl::concat(&pl::s("ID3_"), &id));
                    tag_info.set("Binary", n(1));
                    call(it, "AddTagToTable", vec![tag_table_ptr.clone(), id.clone(), tag_info.clone()])?;
                }
            }
        }
        // decode v2.3 and v2.4 flags
        let mut fl: BTreeSet<&str> = BTreeSet::new();
        let mut extra: Option<Value> = None;
        if flags.truthy() {
            let f = flags.to_i64();
            if pl::lt(&vers, &n(0x0400)) {
                // version 2.3 flags
                if f & 0x80 != 0 {
                    fl.insert("Compress");
                }
                if f & 0x40 != 0 {
                    fl.insert("Encrypt");
                }
                if f & 0x20 != 0 {
                    fl.insert("GroupID");
                }
            } else {
                // version 2.4 flags
                if f & 0x40 != 0 {
                    fl.insert("GroupID");
                }
                if f & 0x08 != 0 {
                    fl.insert("Compress");
                }
                if f & 0x04 != 0 {
                    fl.insert("Encrypt");
                }
                if f & 0x02 != 0 {
                    fl.insert("Unsync");
                }
                if f & 0x01 != 0 {
                    fl.insert("DataLen");
                }
            }
        }
        if fl.contains("Encrypt") {
            warn(it, &et, pl::s("Encrypted frames currently not supported"))?;
            continue;
        }
        // extract the value
        let mut val = pl::substr(&data, offset.to_i64(), Some(len.to_i64()));

        // reverse the unsynchronization
        if fl.contains("Unsync") {
            re(r"\xff\x00", "").subst(&mut val, true, |_| b"\xff".to_vec());
        }

        // read grouping identity
        if fl.contains("GroupID") {
            if !pl::ge(&pl::length(&val), &n(1)) {
                warn(it, &et, cat(&[&pl::s("Short "), &id, &pl::s(" frame")]))?;
                continue;
            }
            val = pl::substr(&val, 1, None); // (ignore it)
        }
        // read data length
        let mut data_len = Value::Undef;
        let mut has_data_len = false;
        if fl.contains("DataLen") || fl.contains("Compress") {
            if !pl::ge(&pl::length(&val), &n(4)) {
                warn(it, &et, cat(&[&pl::s("Short "), &id, &pl::s(" frame")]))?;
                continue;
            }
            data_len = pl::unpack1("N", &val)?; // save the data length word
            has_data_len = true;
            val = pl::substr(&val, 4, None);
        }
        // uncompress data
        if fl.contains("Compress") {
            if pl::eval_block(it, |it| crate::runtime::require(it, "Compress::Zlib"))?.is_some() {
                let inflate = pl::call(it, "Compress::Zlib::inflateInit", vec![])?;
                let (mut buff, mut stat) = (Value::Undef, Value::Undef);
                if inflate.truthy() {
                    let r = pl::method_list(it, &inflate, "inflate", vec![val.clone()])?;
                    buff = arg(&r, 0);
                    stat = arg(&r, 1);
                }
                let ok = inflate.truthy() && {
                    let se = pl::call(it, "Compress::Zlib::Z_STREAM_END", vec![])?;
                    pl::num_eq(&stat, &se)
                };
                if ok {
                    val = buff;
                } else {
                    warn(it, &et, cat(&[&pl::s("Error inflating "), &id, &pl::s(" frame")]))?;
                    continue;
                }
            } else {
                warn(it, &et, pl::s("Install Compress::Zlib to decode compressed frames"))?;
                continue;
            }
        }
        // validate data length
        if has_data_len && !data_len.is_undef() {
            data_len = un_sync_safe1(it, data_len)?;
            if data_len.is_undef() {
                warn(it, &et, cat(&[&pl::s("Invalid length for "), &id, &pl::s(" frame")]))?;
                continue;
            }
            if !pl::num_eq(&data_len, &pl::length(&val)) {
                warn(it, &et, cat(&[&pl::s("Wrong length for "), &id, &pl::s(" frame")]))?;
                continue;
            }
        }
        let flags_extra = |fl: &BTreeSet<&str>| -> Option<Value> {
            if fl.is_empty() {
                None
            } else {
                Some(pl::s(&format!(", Flags={}", fl.iter().copied().collect::<Vec<_>>().join(","))))
            }
        };
        if !tag_info.truthy() {
            if !verbose.truthy() {
                continue;
            }
            extra = flags_extra(&fl).or(extra);
            let mut args = vec![
                id.clone(),
                tag_info.clone(),
                pl::s("Table"),
                tag_table_ptr.clone(),
                pl::s("Value"),
                val.clone(),
                pl::s("DataPt"),
                data_pt.clone(),
                pl::s("DataPos"),
                dir_info.get("DataPos"),
                pl::s("Size"),
                len.clone(),
                pl::s("Start"),
                offset.clone(),
            ];
            if let Some(e) = extra {
                args.push(pl::s("Extra"));
                args.push(e);
            }
            m(it, &et, "VerboseInfo", args)?;
            continue;
        }
        //
        // decode data in this frame (it is bad form to hard-code these, but the ID3 frame formats
        // are so variable that it would be more work to define format types for each of them)
        //
        let mut lang = Value::Undef;
        let val_len = pl::length(&val); // actual value length (after decompression, etc)
        let short = |it: &mut Interp, id: &Value| -> ER<()> { warn(it, &et, cat(&[&pl::s("Short "), id, &pl::s(" frame")])) };
        if re("^(TXX|TXXX)$", "").is_match(&id) {
            // two encoded strings separated by a null
            let mut vals = decode_string_list(it, &et, val.clone(), None)?;
            while vals.len() < 2 {
                vals.push(Value::Undef);
            }
            for v in vals.iter_mut().take(2) {
                if v.is_undef() {
                    *v = pl::s("");
                }
            }
            if pl::length(&vals[0]).truthy() {
                id = cat(&[&id, &pl::s("_"), &vals[0]]);
                tag_info = tag_table_ptr.get(id.to_bytes());
                if !tag_info.truthy() {
                    let name = first(make_tag_name(it, vec![vals[0].clone()], Want::Scalar)?);
                    tag_info = call(it, "AddTagToTable", vec![tag_table_ptr.clone(), id.clone(), name])?;
                }
            }
            val = vals[1].clone();
        } else if re("^T", "").is_match(&id) || re("^(IPL|IPLS|GP1|MVI|MVN)$", "").is_match(&id) {
            val = decode_string1(it, &et, val, None)?;
        } else if re("^(WXX|WXXX)$", "").is_match(&id) {
            // one encoded string and one Latin string separated by a null
            let enc = pl::unpack1("C", &val)?;
            let c = if pl::num_eq(&enc, &n(1)) || pl::num_eq(&enc, &n(2)) { re(r"^(.(?:..)*?)\0\0(.*)", "s").m(&val) } else { re(r"^(..*?)\0(.*)", "s").m(&val) };
            let (mut tag, mut url) = match &c {
                Some(c) => (cap(c, 1), cap(c, 2)),
                None => (Value::Undef, Value::Undef),
            };
            if tag.is_undef() || url.is_undef() {
                warn(it, &et, cat(&[&pl::s("Invalid "), &id, &pl::s(" frame value")]))?;
                continue;
            }
            tag = decode_string1(it, &et, tag, None)?;
            if pl::length(&tag).truthy() {
                id = cat(&[&id, &pl::s("_"), &tag]);
                if !re("url", "i").is_match(&tag) {
                    tag = pl::concat(&tag, &pl::s("_URL"));
                }
                tag_info = tag_table_ptr.get(id.to_bytes());
                if !tag_info.truthy() {
                    let name = first(make_tag_name(it, vec![tag], Want::Scalar)?);
                    tag_info = call(it, "AddTagToTable", vec![tag_table_ptr.clone(), id.clone(), name])?;
                }
            }
            re(r"\0.*", "s").subst(&mut url, false, |_| Vec::new());
            val = m(it, &et, "Decode", vec![url, pl::s("Latin")])?;
        } else if re("^W", "").is_match(&id) {
            re(r"\0.*", "s").subst(&mut val, false, |_| Vec::new()); // truncate at null
        } else if re("^(COM|COMM|ULT|USLT)$", "").is_match(&id) {
            if !pl::gt(&val_len, &n(4)) {
                short(it, &id)?;
                continue;
            }
            lang = pl::substr(&val, 1, Some(3));
            let e = call(it, "Get8u", vec![pl::sref(val.clone()), n(0)])?;
            let mut vals = decode_string_list(it, &et, pl::substr(&val, 4, None), Some(e))?;
            while vals.len() < 2 {
                vals.push(Value::Undef);
            }
            for v in vals.iter_mut().take(2) {
                if v.is_undef() {
                    *v = pl::s("");
                }
            }
            val = if pl::length(&vals[0]).truthy() { cat(&[&pl::s("("), &vals[0], &pl::s(") "), &vals[1]]) } else { vals[1].clone() };
        } else if id.eq_str("USER") {
            if !pl::gt(&val_len, &n(4)) {
                short(it, &id)?;
                continue;
            }
            lang = pl::substr(&val, 1, Some(3));
            let e = call(it, "Get8u", vec![pl::sref(val.clone()), n(0)])?;
            val = decode_string1(it, &et, pl::substr(&val, 4, None), Some(e))?;
        } else if re("^(CNT|PCNT)$", "").is_match(&id) {
            if !pl::ge(&val_len, &n(4)) {
                short(it, &id)?;
                continue;
            }
            let u = pl::unpack("NC*", &val)?;
            let mut cnt = bits(&arg(&u, 0));
            for x in u.iter().skip(1) {
                cnt = (cnt << 8).wrapping_add(bits(x));
            }
            val = uv(cnt);
        } else if re("^(PIC|APIC)$", "").is_match(&id) {
            if !pl::ge(&val_len, &n(4)) {
                short(it, &id)?;
                continue;
            }
            let enc = pl::unpack1("C", &val)?;
            let hdr: &[u8] = if pl::num_eq(&enc, &n(1)) || pl::num_eq(&enc, &n(2)) {
                if id.eq_str("PIC") {
                    b".(...)(.)((?:..)*?)\0\0"
                } else {
                    b".(.*?)\0(.)((?:..)*?)\0\0"
                }
            } else if id.eq_str("PIC") {
                b".(...)(.)(.*?)\0"
            } else {
                b".(.*?)\0(.)(.*?)\0"
            };
            // remove header (encoding, image format or MIME type, picture type, description)
            let mut p = b"^".to_vec();
            p.extend_from_slice(hdr);
            let mut caps = None;
            if pl::re_dyn(&p, "s")?.subst(&mut val, false, |c| {
                caps = Some(c.clone());
                Vec::new()
            }) == 0
            {
                warn(it, &et, cat(&[&pl::s("Invalid "), &id, &pl::s(" frame")]))?;
                continue;
            }
            let c = caps.unwrap_or_default();
            let mut attrs = vec![cap(&c, 1), n(cap(&c, 2).to_bytes().first().copied().unwrap_or(0) as i64)];
            attrs.extend(decode_string_list(it, &et, cap(&c, 3), Some(enc))?);
            let mut i = 1;
            for attr in attrs {
                // must store descriptions even if they are empty to maintain
                // sync between copy numbers when multiple images
                m(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s(&format!("{}-{}", id.to_text(), i)), attr])?;
                i += 1;
            }
        } else if id.eq_str("POP") || id.eq_str("POPM") {
            // _email, 00, rating(1), counter(4-N)
            let (email, dat) = match re(r"^([^\0]*)\0(.*)$", "s").m(&val) {
                Some(c) => (cap(&c, 1), cap(&c, 2)),
                None => (Value::Undef, Value::Undef),
            };
            if !(!dat.is_undef() && pl::length(&dat).truthy()) {
                warn(it, &et, cat(&[&pl::s("Invalid "), &id, &pl::s(" frame")]))?;
                continue;
            }
            let u = pl::unpack("C*", &dat)?;
            let rating = arg(&u, 0);
            let mut cnt: u64 = 0;
            for x in u.iter().skip(1) {
                cnt = (cnt << 8).wrapping_add(bits(x));
            }
            val = cat(&[&email, &pl::s(" "), &rating, &pl::s(" "), &uv(cnt)]);
        } else if id.eq_str("OWNE") {
            // enc(1), _price, 00, _date(8), Seller
            let mut strs = decode_string_list(it, &et, val.clone(), None)?;
            if arg(&strs, 1).truthy() {
                re(r"^(\d{4})(\d{2})(\d{2})", "s").subst(&mut strs[1], false, |c| cat(&[&cap(c, 1), &pl::s(":"), &cap(c, 2), &pl::s(":"), &cap(c, 3), &pl::s(" ")]).to_bytes().to_vec()); // format date
            }
            val = pl::join(" ", &strs);
        } else if id.eq_str("RVA") || id.eq_str("RVAD") {
            let mut dat = pl::unpack("C*", &val)?;
            let flag = if dat.is_empty() { Value::Undef } else { dat.remove(0) };
            let nbits = if dat.is_empty() { Value::Undef } else { dat.remove(0) };
            if !nbits.truthy() {
                short(it, &id)?;
                continue;
            }
            let nb = nbits.to_i64();
            let bytes = pl::int(&pl::div(&pl::add(&nbits, &n(7)), &n(8))?).to_i64();
            let parse: [(&str, i64, i64, i64); 6] = [("Right", 0, 2, 0x01), ("Left", 1, 3, 0x02), ("Back-right", 4, 6, 0x04), ("Back-left", 5, 7, 0x08), ("Center", 8, 9, 0x10), ("Bass", 10, 11, 0x20)];
            val = pl::s("");
            for elem in parse.iter() {
                let j = elem.2 * bytes;
                if (dat.len() as i64) < j + bytes {
                    break;
                }
                let i = elem.1 * bytes;
                if val.truthy() {
                    val = pl::concat(&val, &pl::s(", "));
                }
                let (mut rel, mut pk) = (n(0), n(0));
                for b in 0..bytes {
                    rel = pl::add(&pl::mul(&rel, &n(256)), &arg(&dat, (i + b) as usize));
                    pk = pl::add(&pl::mul(&pk, &n(256)), &arg(&dat, (j + b) as usize)); // (peak - not used in printout)
                }
                let _ = pk;
                if flag.to_i64() & elem.3 == 0 {
                    rel = pl::neg(&rel);
                }
                let shifted: u64 = if (0..64).contains(&nb) { 1u64 << nb } else { 0 };
                let den = pl::sub(&uv(shifted), &n(1));
                val = pl::concat(&val, &pl::sprintf("%+.1f%% %s", &[pl::div(&pl::mul(&n(100), &rel), &den)?, pl::s(elem.0)]));
            }
        } else if id.eq_str("RVA2") {
            let (mut pos, id2) = match re(r"^([^\0]*)\0", "s").m(&val) {
                Some(c) => (pl::length(&cap(&c, 1)).to_i64() + 1, cap(&c, 1)),
                None => (1, pl::s("")),
            };
            let mut vals: Vec<Value> = Vec::new();
            while pl::le(&n(pos + 4), &val_len) {
                let type_ = call(it, "Get8u", vec![pl::sref(val.clone()), n(pos)])?;
                let name = match &*type_.to_bytes() {
                    b"0" => "Other",
                    b"1" => "Master",
                    b"2" => "Front-right",
                    b"3" => "Front-left",
                    b"4" => "Back-right",
                    b"5" => "Back-left",
                    b"6" => "Front-centre",
                    b"7" => "Back-centre",
                    b"8" => "Subwoofer",
                    _ => "",
                };
                let str_ = if name.is_empty() { cat(&[&pl::s("Unknown("), &type_, &pl::s(")")]) } else { pl::s(name) };
                let db = pl::div(&call(it, "Get16s", vec![pl::sref(val.clone()), n(pos + 1)])?, &n(512))?;
                // convert dB to percent as displayed by iTunes 10.5
                // (not sure why I need to divide by 20 instead of 10 as expected - PH)
                let e = pl::add(&pl::div(&db, &n(20))?, &n(2)).to_f64();
                vals.push(pl::sprintf("%+.1f%% %s", &[pl::sub(&Value::Num(10f64.powf(e)), &n(100)), str_]));
                // step to next channel (ignoring peak volume)
                let g = call(it, "Get8u", vec![pl::sref(val.clone()), n(pos + 3)])?;
                pos += 4 + pl::int(&pl::div(&pl::add(&g, &n(7)), &n(8))?).to_i64();
            }
            val = pl::join(", ", &vals);
            if id2.truthy() {
                val = cat(&[&val, &pl::s(" ("), &id2, &pl::s(")")]);
            }
        } else if id.eq_str("PRIV") {
            // save version number to set group1 name for tag later
            et.set("ID3_Ver", tag_table_ptr.get("GROUPS").get("1"));
            m(it, &et, "HandleTag", vec![tag_table_ptr.clone(), id.clone(), val])?;
            continue;
        } else if tag_info.get("Format").truthy() || tag_info.get("SubDirectory").truthy() {
            m(it, &et, "HandleTag", vec![tag_table_ptr.clone(), id.clone(), Value::Undef, pl::s("DataPt"), pl::sref(val)])?;
            continue;
        } else if id.eq_str("GRP1") || id.eq_str("MVNM") || id.eq_str("MVIN") {
            re(r"(^\0+|\0+$)", "").subst(&mut val, true, |_| Vec::new()); // (PH guess)
        } else if !tag_info.get("Binary").truthy() {
            warn(it, &et, cat(&[&pl::s("Don't know how to handle "), &id, &pl::s(" frame")]))?;
            continue;
        }
        if lang.truthy() && re("^[a-z]{3}$", "i").is_match(&lang) && !lang.eq_str("eng") {
            tag_info = call(it, "GetLangInfo", vec![tag_info, pl::lc(&lang)])?;
        }
        extra = flags_extra(&fl).or(extra);
        let mut args = vec![
            tag_table_ptr.clone(),
            id.clone(),
            val,
            pl::s("TagInfo"),
            tag_info,
            pl::s("DataPt"),
            data_pt.clone(),
            pl::s("DataPos"),
            dir_info.get("DataPos"),
            pl::s("Size"),
            len.clone(),
            pl::s("Start"),
            offset.clone(),
        ];
        if let Some(e) = extra {
            args.push(pl::s("Extra"));
            args.push(e);
        }
        m(it, &et, "HandleTag", args)?;
    }
    pl::ret_nothing(want)
}

/// Extract ID3 information from an audio file
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 if this file didn't contain ID3 information
/// - also processes audio data if any ID3 information was found
/// - sets ExifTool DoneID3 to 1 when called, or to trailer size if an ID3v1 trailer exists
pub fn process_id3(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));

    if et.get("DoneID3").truthy() {
        return ret(n(0)); // avoid infinite recursion
    }
    et.set("DoneID3", n(1));

    // allow this to be called with either RAF or DataPt
    let mut raf = dir_info.get("RAF");
    if !raf.truthy() {
        raf = pl::method(it, &pl::s("File::RandomAccess"), "new", vec![dir_info.get("DataPt")])?;
    }
    let data_pos = {
        let d = dir_info.get("DataPos");
        if d.truthy() { d } else { n(0) }
    };
    let (mut buff, mut h_buff, mut t_buff, mut e_buff) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef);
    let id3_header = Value::new_hash();
    let id3_trailer = Value::new_hash();
    let mut tag_table_ptr = Value::Undef;
    let mut rtn_val = false;
    let mut hdr_end = n(0);
    let mut id3_len = n(0);

    // read first 3 bytes of file
    m(it, &raf, "Seek", vec![n(0), n(0)])?;
    if !pl::num_eq(&read(it, &raf, &mut buff, n(3))?, &n(3)) {
        return ret(n(0));
    }
    //
    // identify ID3v2 header
    //
    #[allow(clippy::never_loop)]
    while re("^ID3", "").is_match(&buff) {
        rtn_val = true;
        if !pl::num_eq(&read(it, &raf, &mut h_buff, n(7))?, &n(7)) {
            warn(it, &et, pl::s("Short ID3 header"))?;
            break;
        }
        let u = pl::unpack("nCN", &h_buff)?;
        let (vers, flags) = (arg(&u, 0), arg(&u, 1).to_i64());
        let size = un_sync_safe1(it, arg(&u, 2))?;
        if size.is_undef() {
            warn(it, &et, pl::s("Invalid ID3 header"))?;
            break;
        }
        let vv = vers.to_i64();
        let ver_str = pl::sprintf("2.%d.%d", &[n(vv >> 8), n(vv & 0xff)]);
        if vv >= 0x0500 {
            warn(it, &et, pl::concat(&pl::s("Unsupported ID3 version: "), &ver_str))?;
            break;
        }
        if !pl::num_eq(&read(it, &raf, &mut h_buff, size.clone())?, &size) {
            warn(it, &et, pl::s("Truncated ID3 data"))?;
            break;
        }
        // this flag only indicates use of unsynchronized frames in ID3v2.4
        if flags & 0x80 != 0 && vv < 0x0400 {
            // reverse the unsynchronization
            re(r"\xff\x00", "").subst(&mut h_buff, true, |_| b"\xff".to_vec());
        }
        let mut pos = n(10);
        if flags & 0x40 != 0 {
            // skip the extended header
            if !pl::ge(&size, &n(4)) {
                warn(it, &et, pl::s("Bad ID3 extended header"))?;
                break;
            }
            let len = un_sync_safe1(it, pl::unpack1("N", &h_buff)?)?;
            if pl::gt(&len, &pl::length(&h_buff)) {
                warn(it, &et, pl::s("Truncated ID3 extended header"))?;
                break;
            }
            h_buff = pl::substr(&h_buff, len.to_i64(), None);
            pos = pl::add(&pos, &len);
        }
        if flags & 0x10 != 0 {
            // ignore v2.4 footer (10 bytes long)
            m(it, &raf, "Seek", vec![n(10), n(1)])?;
        }
        id3_header.set("DataPt", pl::sref(h_buff.clone()));
        id3_header.set("DataPos", pl::add(&data_pos, &pos));
        id3_header.set("DirStart", n(0));
        id3_header.set("DirLen", pl::length(&h_buff));
        id3_header.set("Version", vers.clone());
        id3_header.set("DirName", pl::concat(&pl::s("ID3v"), &ver_str));
        id3_len = pl::add(&id3_len, &pl::add(&pl::length(&h_buff), &n(10)));
        tag_table_ptr = if vv >= 0x0400 {
            call(it, "GetTagTable", vec![pl::s("Image::ExifTool::ID3::v2_4")])?
        } else if vv >= 0x0300 {
            call(it, "GetTagTable", vec![pl::s("Image::ExifTool::ID3::v2_3")])?
        } else {
            call(it, "GetTagTable", vec![pl::s("Image::ExifTool::ID3::v2_2")])?
        };
        hdr_end = m(it, &raf, "Tell", vec![])?;
        break;
    }
    //
    // read ID3v1 trailer if it exists
    //
    let mut trail_size: i64 = 0;
    if m(it, &raf, "Seek", vec![n(-128), n(2)])?.truthy() && pl::num_eq(&read(it, &raf, &mut t_buff, n(128))?, &n(128)) && re("^TAG", "").is_match(&t_buff) {
        trail_size = 128;
        id3_trailer.set("DataPt", pl::sref(t_buff.clone()));
        let tell = m(it, &raf, "Tell", vec![])?;
        id3_trailer.set("DataPos", pl::sub(&pl::add(&data_pos, &tell), &n(128)));
        id3_trailer.set("DirStart", n(0));
        id3_trailer.set("DirLen", pl::length(&t_buff));
        id3_len = pl::add(&id3_len, &pl::length(&t_buff));
        rtn_val = true;
        // load 'Enhanced TAG' information if available
        let e_size = 227; // size of ID3 Enhanced TAG info
        if m(it, &raf, "Seek", vec![n(-trail_size - e_size), n(2)])?.truthy() && pl::num_eq(&read(it, &raf, &mut e_buff, n(e_size))?, &n(e_size)) && re("^TAG+", "").is_match(&e_buff) {
            id3_trailer.set("EnhancedTAG", pl::sref(e_buff.clone()));
            trail_size += e_size;
        }
        et.set("DoneID3", n(trail_size)); // save trailer size
    }
    //
    // read Lyrics3 trailer if it exists
    //
    let lyr = if m(it, &raf, "Seek", vec![n(-trail_size - 15), n(2)])?.truthy() && pl::num_eq(&read(it, &raf, &mut buff, n(15))?, &n(15)) {
        re(r"^(.{6})LYRICS(END|200)$", "").m(&buff)
    } else {
        None
    };
    if let Some(c) = lyr {
        let ver = cap(&c, 2); // Lyrics3 version ('END' for version 1)
        let mut len = if ver.eq_str("END") { n(5100) } else { pl::add(&cap(&c, 1), &n(15)) }; // max Lyrics3 length
        let tbl = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::ID3::Lyrics3")])?;
        let tell = m(it, &raf, "Tell", vec![])?;
        if pl::gt(&len, &tell) {
            len = tell;
        }
        // (pos($buff) is followed in `bpos`)
        let mut bpos: Option<i64> = None;
        let ok = m(it, &raf, "Seek", vec![pl::neg(&len), n(1)])?.truthy() && pl::num_eq(&read(it, &raf, &mut buff, len.clone())?, &len) && {
            match re("LYRICSBEGIN", "").m(&buff) {
                Some(_) => {
                    // the end of the first match
                    let b = buff.to_bytes();
                    let p = b.windows(11).position(|w| w == b"LYRICSBEGIN").map(|p| p + 11).unwrap_or(0);
                    bpos = Some(p as i64);
                    true
                }
                None => false,
            }
        };
        if ok {
            let mut pos = bpos.unwrap_or(0);
            et.set("DoneID3", pl::add(&pl::sub(&pl::add(&n(trail_size), &len), &n(pos)), &n(11))); // update trailer length
            let old_indent = et.get("INDENT");
            et.set("INDENT", pl::concat(&et.get("INDENT"), &pl::s("| ")));
            if m(it, &et, "Options", vec![pl::s("Verbose")])?.truthy() {
                m(it, &et, "VPrint", vec![n(0), pl::s("Lyrics3:\n")])?;
                m(it, &et, "VerboseDir", vec![pl::s("Lyrics3"), Value::Undef, len.clone()])?;
                if pos > 11 {
                    buff = pl::substr(&buff, pos - 11, None);
                    bpos = None; // (assignment resets pos)
                    pos = 11;
                }
                m(it, &et, "VerboseDump", vec![pl::sref(buff.clone())])?;
            }
            if ver.eq_str("END") {
                // Lyrics3 v1.00
                let val = pl::substr(&buff, pos, Some(pl::sub(&pl::sub(&len, &n(pos)), &n(9)).to_i64()));
                let d = m(it, &et, "Decode", vec![val, pl::s("Latin")])?;
                m(it, &et, "HandleTag", vec![tbl.clone(), pl::s("LYR"), d])?;
            } else {
                // Lyrics3 v2.00
                loop {
                    // (note: the size field is 5 digits,, not 6 as per the documentation)
                    let start = bpos.unwrap_or(0);
                    let Some(c) = re(r"^(.{3})(\d{5})", "").m(&pl::substr(&buff, start, None)) else { break };
                    // (pos($buff) is now start + 8, set again below before it is used)
                    let (tag, size) = (cap(&c, 1), cap(&c, 2));
                    pos += 8;
                    if pl::gt(&pl::add(&n(pos), &size), &pl::length(&buff)) {
                        break;
                    }
                    if !tbl.get(tag.to_bytes()).truthy() {
                        let ti = Value::new_hash();
                        ti.set("Name", call(it, "MakeTagName", vec![pl::concat(&pl::s("Lyrics3_"), &tag)])?);
                        call(it, "AddTagToTable", vec![tbl.clone(), tag.clone(), ti])?;
                    }
                    let d = m(it, &et, "Decode", vec![pl::substr(&buff, pos, Some(size.to_i64())), pl::s("Latin")])?;
                    m(it, &et, "HandleTag", vec![tbl.clone(), tag, d])?;
                    pos += size.to_i64();
                    bpos = Some(pos);
                }
                if pos != pl::length(&buff).to_i64() - 15 {
                    warn(it, &et, pl::s("Malformed Lyrics3 v2.00 block"))?;
                }
            }
            et.set("INDENT", old_indent);
        } else {
            warn(it, &et, pl::s("Error reading Lyrics3 trailer"))?;
        }
    }
    //
    // process the information
    //
    if rtn_val {
        // first process audio data if it exists
        if dir_info.get("RAF").truthy() {
            let old_type = et.get("FILE_TYPE"); // save file type
            // check current file type first
            let audio_formats = crate::runtime::lex_array(it, MODULE, "audioFormats").items();
            let mut pat = b"^".to_vec();
            pat.extend_from_slice(&old_type.to_bytes());
            pat.push(b'$');
            let rx = pl::re_dyn(&pat, "")?;
            let mut types: Vec<Value> = audio_formats.iter().filter(|t| rx.is_match(t)).cloned().collect();
            types.extend(audio_formats.iter().filter(|t| !rx.is_match(t)).cloned());
            let audio_module = lex_hash(it, "audioModule");
            for type_ in types {
                // seek to end of ID3 header
                m(it, &raf, "Seek", vec![hdr_end.clone(), n(0)])?;
                // set type for this file if we are successful
                et.set("FILE_TYPE", type_.clone());
                let module = {
                    let mm = audio_module.get(type_.to_bytes());
                    if mm.truthy() { mm } else { type_.clone() }
                };
                crate::runtime::require(it, &format!("Image/ExifTool/{}.pm", module.to_text()))?;
                let func = format!("Image::ExifTool::{}::Process{}", module.to_text(), type_.to_text());
                // process the file
                if pl::call(it, &func, vec![et.clone(), dir_info.clone()])?.truthy() {
                    break;
                }
            }
            et.set("FILE_TYPE", old_type); // restore original file type
        }
        // set file type to MP3 if we didn't find audio data
        m(it, &et, "SetFileType", vec![pl::s("MP3")])?;
        // record the size of the ID3 metadata
        m(it, &et, "FoundTag", vec![pl::s("ID3Size"), id3_len])?;
        // process ID3v2 header if it exists
        if !id3_header.is_empty() {
            m(it, &et, "VPrint", vec![n(0), pl::concat(&id3_header.get("DirName"), &pl::s(":\n"))])?;
            m(it, &et, "ProcessDirectory", vec![id3_header.clone(), tag_table_ptr.clone()])?;
        }
        // process ID3v1 trailer if it exists
        if !id3_trailer.is_empty() {
            m(it, &et, "VPrint", vec![n(0), pl::s("ID3v1:\n")])?;
            call(it, "SetByteOrder", vec![pl::s("MM")])?;
            tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::ID3::v1")])?;
            m(it, &et, "ProcessDirectory", vec![id3_trailer.clone(), tag_table_ptr.clone()])?;
            // process "Enhanced TAG" information if available
            if id3_trailer.get("EnhancedTAG").truthy() {
                m(it, &et, "VPrint", vec![n(0), pl::s("ID3v1 Enhanced TAG:\n")])?;
                tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::ID3::v1_Enh")])?;
                id3_trailer.set("DataPt", id3_trailer.get("EnhancedTAG"));
                id3_trailer.set("DataPos", pl::sub(&id3_trailer.get("DataPos"), &n(227))); // (227 = length of Enhanced TAG block)
                id3_trailer.set("DirLen", n(227));
                m(it, &et, "ProcessDirectory", vec![id3_trailer.clone(), tag_table_ptr.clone()])?;
            }
        }
    }
    // return file pointer to start of file to read audio data if necessary
    m(it, &raf, "Seek", vec![n(0), n(0)])?;
    ret(n(if rtn_val { 1 } else { 0 }))
}

/// Process ID3 directory
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference, 2) dummy tag table ref
pub fn process_id3_dir(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    pl::at(1640);
    let len = pl::length(&pl::sderef(&dir_info.get("DataPt"))?);
    m(it, &et, "VerboseDir", vec![pl::s("ID3"), Value::Undef, len])?;
    process_id3(it, vec![et, dir_info], want)
}

/// Process ID3 General Encapsulated Object
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_geob(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    pl::at(1651);
    let len = pl::length(&pl::sderef(&dir_info.get("DataPt"))?);
    m(it, &et, "VerboseDir", vec![pl::s("GEOB"), Value::Undef, len])?;
    let data_pt = dir_info.get("DataPt");
    let len = pl::length(&data_pt.deref());
    if !pl::ge(&len, &n(4)) {
        warn(it, &et, pl::s("Short GEOB frame"))?;
        return ret(n(0));
    }
    let enc = pl::unpack1("C", &data_pt.deref())?;
    let hdr: &str = if pl::num_eq(&enc, &n(1)) || pl::num_eq(&enc, &n(2)) { "^.(.*?)\0((?:..)*?)\0\0((?:..)*?)\0\0" } else { "^.(.*?)\0(.*?)\0(.*?)\0" };
    // remove header (encoding, mime, filename, description)
    let mut d = data_pt.deref();
    let mut caps = None;
    if pl::re_dyn(hdr.as_bytes(), "s")?.subst(&mut d, false, |c| {
        caps = Some(c.clone());
        Vec::new()
    }) == 0
    {
        warn(it, &et, pl::s("Invalid GEOB frame"))?;
        return ret(n(0));
    }
    pl::set_deref(&data_pt, d)?;
    let c = caps.unwrap_or_default();
    let mime = cap(&c, 1);
    let file = decode_string1(it, &et, cap(&c, 2), Some(enc.clone()))?;
    let desc = decode_string1(it, &et, cap(&c, 3), Some(enc))?;
    if pl::length(&mime).truthy() {
        m(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("GEOB-Mime"), mime.clone()])?;
    }
    if pl::length(&file).truthy() {
        m(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("GEOB-File"), file])?;
    }
    if pl::length(&desc).truthy() {
        m(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("GEOB-Desc"), desc])?;
    }
    if tag_table_ptr.get(mime.to_bytes()).truthy() {
        let size = pl::length(&data_pt.deref());
        m(it, &et, "HandleTag", vec![tag_table_ptr.clone(), mime, Value::Undef, pl::s("DataPt"), data_pt.clone(), pl::s("Start"), n(0), pl::s("Size"), size])?;
    } else {
        m(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("GEOB-Data"), data_pt.clone()])?;
    }
    ret(n(1))
}

/// Extract ID3 information from an MP3 audio file
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid MP3 file
pub fn process_mp3(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let mut rtn_val = n(0);

    // must first check for leading/trailing ID3 information
    // (and process the rest of the file if found)
    if !et.get("DoneID3").truthy() {
        rtn_val = first(process_id3(it, vec![et.clone(), dir_info.clone()], Want::Scalar)?);
    }

    // check for MPEG A/V data if not already processed above
    if !rtn_val.truthy() {
        let raf = dir_info.get("RAF");
        let mut buff = Value::Undef;
        //
        // extract information from first audio/video frame headers
        // (if found in the first $scanLen bytes)
        //
        // scan further into a file that should be an MP3
        let fe = et.get("FILE_EXT");
        let scan_len = if fe.truthy() && fe.eq_str("MP3") { 8192 } else { 256 };
        pl::at(1705);
        if read(it, &raf, &mut buff, n(scan_len))?.truthy() {
            crate::runtime::require(it, "Image::ExifTool::MPEG")?;
            if re(r"\0\0\x01(\xb3|\xc0)", "").is_match(&buff) {
                // look for A/V headers in first 64kB
                let mut buf2 = Value::Undef;
                if read(it, &raf, &mut buf2, n(0x10000 - scan_len))?.truthy() {
                    buff = pl::concat(&buff, &buf2);
                }
                if pl::call(it, "Image::ExifTool::MPEG::ParseMPEGAudioVideo", vec![et.clone(), pl::sref(buff)])?.truthy() {
                    rtn_val = n(1);
                }
            } else {
                // look for audio frame sync in first $scanLen bytes
                // (set MP3 flag to 1 so this will fail unless layer 3 audio)
                let ext = if fe.truthy() { fe.clone() } else { pl::s("") };
                let mp3 = n(if ext.eq_str("MUS") { 0 } else { 1 }); // MUS files are MP2
                if pl::call(it, "Image::ExifTool::MPEG::ParseMPEGAudio", vec![et.clone(), pl::sref(buff), mp3])?.truthy() {
                    rtn_val = n(1);
                }
            }
        }
    }

    // check for an APE trailer if this was a valid A/V file and we haven't already done it
    if rtn_val.truthy() && !et.get("DoneAPE").truthy() {
        crate::runtime::require(it, "Image::ExifTool::APE")?;
        pl::call(it, "Image::ExifTool::APE::ProcessAPE", vec![et.clone(), dir_info.clone()])?;
    }
    ret(rtn_val)
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::ID3::MakeTagName", make_tag_name);
    it.define_native("Image::ExifTool::ID3::ConvertID3v1Text", convert_id3v1_text);
    it.define_native("Image::ExifTool::ID3::ConvertTimeStamp", convert_time_stamp);
    it.define_native("Image::ExifTool::ID3::ProcessSynText", process_syn_text);
    it.define_native("Image::ExifTool::ID3::ProcessPrivate", process_private);
    it.define_native("Image::ExifTool::ID3::PrintGenre", print_genre);
    it.define_native("Image::ExifTool::ID3::GetGenreID", get_genre_id);
    it.define_native("Image::ExifTool::ID3::DecodeString", decode_string);
    it.define_native("Image::ExifTool::ID3::UnSyncSafe", un_sync_safe);
    it.define_native("Image::ExifTool::ID3::ProcessID3v2", process_id3v2);
    it.define_native("Image::ExifTool::ID3::ProcessID3", process_id3);
    it.define_native("Image::ExifTool::ID3::ProcessID3Dir", process_id3_dir);
    it.define_native("Image::ExifTool::ID3::ProcessGEOB", process_geob);
    it.define_native("Image::ExifTool::ID3::ProcessMP3", process_mp3);
}
