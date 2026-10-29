//! Image::ExifTool::TNEF (Image/ExifTool/TNEF.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read TNEF meta information. The file's `my` hashes (%propType,
//! %fmtSize) stay Perl data, read with `lex_hash`.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell};
use crate::perl::ops;
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};
use crate::runtime;

/// `$raf->Read(...)` runs as a perl statement (random_access::perl_eval),
/// whose errors would name "(eval N)": an invocant that is not an object
/// gives perl's error here instead, at the native's location.
fn need_obj(v: &Value, method: &str) -> ER<()> {
    match v {
        Value::Undef => crate::interp::eval::die(format!("Can't call method \"{method}\" on an undefined value")),
        Value::HashRef(h) if h.borrow().blessed.is_none() => crate::interp::eval::die(format!("Can't call method \"{method}\" on unblessed reference")),
        Value::ArrayRef(_) | Value::ScalarRef(_) | Value::Code(_) => crate::interp::eval::die(format!("Can't call method \"{method}\" on unblessed reference")),
        _ => Ok(()),
    }
}

const MODULE: &str = "Image::ExifTool::TNEF";

fn s(x: &str) -> Value {
    pl::s(x)
}

fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

//------------------------------------------------------------------------------
// Decompress RTF text (ref https://metacpan.org/pod/Mail::Exchange::Message)
// Inputs: 0) ExifTool ref, 1) compressed RTF
// Returns: Decompressed RTF or '' on error
pub fn decompress_rtf(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, cdat) = (arg(&a, 0), arg(&a, 1));
    if !pl::gt(&pl::length(&cdat), &n(16)) {
        return ret(s(""));
    }
    let comp = pl::unpack1("x8V", &cdat)?;

    if pl::num_eq(&comp, &n(0x414c454D)) {
        return ret(pl::substr(&cdat, 16, None));
    } else if !pl::num_eq(&comp, &n(0x75465a4c)) {
        let msg = pl::sprintf("Unknown RTF compression 0x%x", &[comp]);
        pl::method(it, &et, "Warn", vec![msg])?;
        return ret(s(""));
    }
    let mut dict: Vec<u8> = [
        r"{\rtf1\ansi\mac\deff0\deftab720{\fonttbl;}",
        r"{\f0\fnil \froman \fswiss \fmodern ",
        r"\fscript \fdecor MS Sans SerifSymbolArialTimes",
        r" New RomanCourier{\colortbl\red0\green0\blue0",
        "\r\n",
        r"\par \pard\plain\f0\fs20\b\i\u\tab\tx",
    ]
    .concat()
    .into_bytes();
    let cd = cdat.to_bytes();
    let mut cpos: usize = 16;
    let clen = cd.len();
    let mut dpos: usize = dict.len();
    let mut rtn_val: Vec<u8> = Vec::new();
    // substr($dict, ($dpos++ % 4096), 1) = $ch (the offset is never past the end)
    fn put(dict: &mut Vec<u8>, at: usize, ch: u8) {
        if at < dict.len() {
            dict[at] = ch;
        } else {
            dict.push(ch);
        }
    }
    while cpos < clen {
        let control = cd[cpos] as u32;
        cpos += 1;
        let mut i = 0;
        while i < 8 && cpos < clen {
            if control & (1 << i) != 0 {
                if cpos + 2 > clen {
                    return ret(Value::bytes(rtn_val));
                }
                let r#ref = ((cd[cpos] as usize) << 8) | cd[cpos + 1] as usize;
                cpos += 2;
                let mut off = r#ref >> 4;
                let len = (r#ref & 0x0f) + 2;
                if off == dpos % 4096 || off % 4096 >= dict.len() {
                    return ret(Value::bytes(rtn_val));
                }
                for _ in 0..len {
                    let ch = dict[off % 4096];
                    off += 1;
                    put(&mut dict, dpos % 4096, ch);
                    dpos += 1;
                    rtn_val.push(ch);
                }
            } else {
                let ch = cd[cpos];
                cpos += 1;
                put(&mut dict, dpos % 4096, ch);
                dpos += 1;
                rtn_val.push(ch);
            }
            i += 1;
        }
    }
    ret(Value::bytes(rtn_val))
}

//------------------------------------------------------------------------------
// Process TNEF message properties
// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
// Returns: 1 on success
pub fn process_props(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_tbl) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let data_pos = dir_info.get("DataPos");
    pl::at(285);
    let dir_len = pl::length(&pl::sderef(&data_pt)?);
    if !pl::gt(&dir_len, &n(4)) {
        return ret(n(0));
    }
    let prop_type = runtime::lex_hash(it, MODULE, "propType");
    let fmt_size = runtime::lex_hash(it, MODULE, "fmtSize");
    pl::at(287);
    let entries = pl::unpack1("V", &pl::sderef(&data_pt)?)?;
    pl::method(it, &et, "VerboseDir", vec![s("TNEF Properties"), entries.clone()])?;
    let mut pos = n(4);
    let mut i: i64 = 0;
    while pl::lt(&n(i), &entries) {
        if pl::gt(&pl::add(&pos, &n(4)), &dir_len) {
            break;
        }
        let mut type_ = call(it, "Get16u", vec![data_pt.clone(), pos.clone()])?;
        let mut tag = call(it, "Get16u", vec![data_pt.clone(), pl::add(&pos, &n(2))])?;
        pos = pl::add(&pos, &n(4));
        // handle named properties (bit 0x8000 set)
        if ops::bit_and(&tag, &n(0x8000)).truthy() {
            if pl::gt(&pl::add(&pos, &n(24)), &dir_len) {
                break;
            }
            let mut uid = pl::call(it, "Image::ExifTool::ASF::GetGUID", vec![pl::substr(&data_pt.deref(), pos.to_i64(), Some(16))])?;
            re(r"-0000-0000-C000-000000000046$", "").subst(&mut uid, false, |_| Vec::new()); // remove common suffix
            let idtype = call(it, "Get32u", vec![data_pt.clone(), pl::add(&pos, &n(16))])?;
            let num = call(it, "Get32u", vec![data_pt.clone(), pl::add(&pos, &n(20))])?;
            pos = pl::add(&pos, &n(24));
            if pl::num_eq(&idtype, &n(0)) {
                // number
                tag = pl::concat(&uid, &pl::sprintf("_%.8x", &[num]));
            } else if pl::num_eq(&idtype, &n(1)) {
                // string
                if pl::gt(&pl::add(&pos, &num), &dir_len) || pl::lt(&num, &n(2)) {
                    break;
                }
                // decode string (ignoring null terminator)
                let sub = pl::substr(&data_pt.deref(), pos.to_i64(), Some(pl::sub(&num, &n(2)).to_i64()));
                let name = pl::method(it, &et, "Decode", vec![sub, s("UTF16")])?;
                tag = pl::concat(&pl::concat(&uid, &s("_")), &name);
                if !tag_tbl.get(tag.to_bytes()).truthy() {
                    let ti = Value::new_hash();
                    ti.set("Name", pl::call(it, "Image::ExifTool::MakeTagName", vec![name])?);
                    call(it, "AddTagToTable", vec![tag_tbl.clone(), tag.clone(), ti])?;
                }
                pos = pl::add(&pos, &ops::bit_and(&pl::add(&num, &n(3)), &n(0xfffffffc))); // (padded to an even 4 bytes)
            } else {
                break; // error
            }
        }
        let mut count = n(1);
        let mut multi = false;
        if ops::bit_and(&type_, &n(0x1000)).truthy() {
            multi = true;
            type_ = ops::bit_and(&type_, &n(0x0fff));
            if pl::gt(&pl::add(&pos, &n(4)), &dir_len) {
                break;
            }
            count = call(it, "Get32u", vec![data_pt.clone(), pos.clone()])?;
            pos = pl::add(&pos, &n(4));
        }
        let fmt = prop_type.get(type_.to_bytes());
        if !fmt.truthy() {
            break;
        }
        while count.truthy() {
            let mut size = fmt_size.get(fmt.to_bytes());
            let mut val = Value::Undef;
            if !size.truthy() {
                if let Some(m) = re(r"(\d+)", "").m(&fmt) {
                    size = pl::div(&pl::mul(&count, &cap(&m, 1)), &n(8))?;
                } else if pl::str_eq(&fmt, "null") {
                    val = s(""); // ($size is already 0)
                } else {
                    // skip 1 count for "special case" stupidity
                    if !multi {
                        pos = pl::add(&pos, &n(4));
                    }
                    if pl::gt(&pl::add(&pos, &n(4)), &dir_len) {
                        break;
                    }
                    size = call(it, "Get32u", vec![data_pt.clone(), pos.clone()])?;
                    pos = pl::add(&pos, &n(4));
                    if pl::gt(&pl::add(&pos, &size), &dir_len) {
                        break;
                    }
                    pl::at(344);
                    val = pl::substr(&pl::sderef(&data_pt)?, pos.to_i64(), Some(size.to_i64()));
                }
            }
            if val.is_undef() {
                val = call(it, "ReadValue", vec![data_pt.clone(), pos.clone(), fmt.clone(), count.clone(), size.clone()])?;
                if [0x06, 0x07, 0x0b, 0x40].iter().any(|t| pl::num_eq(&type_, &n(*t))) {
                    let mut a = pl::split_ws(&val);
                    if pl::num_eq(&type_, &n(0x06)) {
                        // currency
                        for x in a.iter_mut() {
                            *x = pl::div(x, &n(10000))?;
                        }
                    } else if pl::num_eq(&type_, &n(0x07)) {
                        // OLE date
                        // convert time from days since Dec 30, 1899
                        for x in a.iter_mut() {
                            if !pl::num_eq(x, &n(0)) {
                                *x = pl::mul(&pl::mul(&pl::sub(x, &n(25569)), &n(24)), &n(3600));
                            }
                            *x = call(it, "ConvertUnixTime", vec![x.clone()])?;
                        }
                    } else if pl::num_eq(&type_, &n(0x0b)) {
                        // boolean
                        for x in a.iter_mut() {
                            *x = s(if x.truthy() { "True" } else { "False" });
                        }
                    } else if pl::num_eq(&type_, &n(0x40)) {
                        // SYSTIME
                        // convert time from 100-ns intervals since Jan 1, 1601
                        for x in a.iter_mut() {
                            let t = pl::sub(&pl::div(x, &Value::Num(1e7))?, &n(11644473600));
                            *x = call(it, "ConvertUnixTime", vec![t, n(1)])?;
                        }
                    }
                    val = if a.len() > 1 { Value::new_array(a) } else { arg(&a, 0) };
                }
                count = n(1); // (read them all already)
            } else if pl::str_eq(&fmt, "GUID") {
                val = pl::call(it, "Image::ExifTool::ASF::GetGUID", vec![val])?;
            } else if pl::str_eq(&fmt, "Unicode") {
                val = pl::method(it, &et, "Decode", vec![val, s("UTF16")])?;
                re(r"\0+$", "").subst(&mut val, false, |_| Vec::new());
            } else if pl::str_eq(&fmt, "string") {
                re(r"\0+$", "").subst(&mut val, false, |_| Vec::new());
                // convert from specified code page if supported
                let cs = et.get("Charset");
                if cs.truthy() {
                    val = pl::method(it, &et, "Decode", vec![val, cs])?;
                }
            } else if pl::str_eq(&fmt, "undef") && pl::length(&val).truthy() {
                let copy = val.clone();
                val = pl::sref(copy); // change to a binary data reference
            }
            let format = pl::sprintf("%s, type 0x%.2x", &[fmt.clone(), type_.clone()]);
            pl::method(
                it,
                &et,
                "HandleTag",
                vec![
                    tag_tbl.clone(),
                    tag.clone(),
                    val,
                    s("DataPt"),
                    data_pt.clone(),
                    s("DataPos"),
                    data_pos.clone(),
                    s("Start"),
                    pos.clone(),
                    s("Size"),
                    size.clone(),
                    s("Format"),
                    format,
                    s("Index"),
                    n(i),
                ],
            )?;
            pos = pl::add(&pos, &ops::bit_and(&pl::add(&size, &n(3)), &n(0xfffffffc)));
            count = pl::sub(&count, &n(1));
        }
        i += 1;
    }
    if !pl::num_eq(&n(i), &entries) {
        pl::method(it, &et, "Warn", vec![s("Error parsing message properties")])?;
    }
    ret(n(1))
}

//------------------------------------------------------------------------------
// Extract EXIF information from a TNEF image
// Inputs: 0) ExifTool object reference, 1) dirInfo reference
// Returns: 1 on success, 0 if this wasn't a valid TNEF file
pub fn process_tnef(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let buff = cell(Value::Undef);

    // verify this is a valid TNEF file (read TNEFHeader and TNEFVersion)
    pl::at(407);
    need_obj(&raf, "Read")?;
    if !(pl::num_eq(&raf_read_cell(it, &raf, &buff, n(0x15))?, &n(0x15)) && pl::method(it, &raf, "Seek", vec![n(6), n(0)])?.truthy()) {
        return ret(n(0));
    }
    if !re(r"^\x78\x9f\x3e\x22..\x01\x06\x90\x08\0", "s").is_match(&buff.borrow()) {
        return ret(n(0));
    }
    pl::method(it, &et, "SetFileType", vec![s("TNEF")])?;
    call(it, "SetByteOrder", vec![s("II")])?;
    let tag_tbl = call(it, "GetTagTable", vec![s("Image::ExifTool::TNEF::Main")])?;
    // read through the attributes
    while pl::num_eq(&raf_read_cell(it, &raf, &buff, n(9))?, &n(9)) {
        // (ignore the attrLevel byte: 1 for message, 2 for attachment)
        let u = pl::unpack("x1VV", &buff.borrow())?;
        let (tag, len) = (arg(&u, 0), arg(&u, 1));
        // increment document number for each attachment
        if pl::num_eq(&tag, &n(0x069002)) {
            let dc = pl::add(&et.get("DOC_COUNT"), &n(1));
            et.set("DOC_COUNT", dc.clone());
            et.set("DOC_NUM", dc);
        }
        if !pl::num_eq(&raf_read_cell(it, &raf, &buff, len.clone())?, &len) {
            break;
        }
        let tag_info = tag_tbl.get(tag.to_bytes());
        let (mut val, mut fmt) = (Value::Undef, Value::Undef);
        if tag_info.truthy() && tag_info.get("Format").truthy() {
            fmt = tag_info.get("Format");
            let b = buff.borrow().clone();
            if pl::str_eq(&fmt, "date") && pl::length(&b).to_i64() >= 12 {
                let date = pl::unpack("v6", &b)?;
                val = pl::sprintf("%.4d:%.2d:%.2d %.2d:%.2d:%.2d", &date);
            }
        } else {
            val = buff.borrow().clone();
        }
        let pos = pl::sub(&pl::method(it, &raf, "Tell", vec![])?, &len);
        pl::method(
            it,
            &et,
            "HandleTag",
            vec![tag_tbl.clone(), tag.clone(), val, s("DataPt"), Value::ScalarRef(buff.clone()), s("DataPos"), pos, s("Format"), fmt],
        )?;
        if pl::num_eq(&tag, &n(0x069005)) {
            et.delete("DOC_NUM"); // end of attachment
        }
        pl::method(it, &raf, "Seek", vec![n(2), n(1)])?; // skip checksum
    }
    et.delete("DOC_NUM");
    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::TNEF::DecompressRTF", decompress_rtf);
    it.define_native("Image::ExifTool::TNEF::ProcessProps", process_props);
    it.define_native("Image::ExifTool::TNEF::ProcessTNEF", process_tnef);
}
