//! Image::ExifTool::FlashPix (Image/ExifTool/FlashPix.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read FlashPix meta information: the OLE/CFB compound document reader,
//! property sets, the FPXR segments of JPEG images and the MS Office
//! document specifics (Word document table, comment authors...).
//!
//! The file's `my` variables (%oleFormat, %oleFlags, %oleFormatSize,
//! @dirEntryType, %fpxFileType) stay Perl data, read with `lex_hash` and
//! `lex_array`.

use std::collections::HashSet;

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::exiftool::part2::{get16u as get16u_n, get32u as get32u_n, get8u as get8u_n};
use crate::modules::random_access::{cell, perl_eval, perl_eval1, substr};
use crate::perl::regex as pre;
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, n, re, ret};
use crate::runtime;

const MODULE: &str = "Image::ExifTool::FlashPix";

// sector type constants
const HDR_SIZE_C: i64 = 512;
const END_OF_CHAIN_C: i64 = 0xfffffffe;
const FREE_SECT_C: i64 = 0xffffffff;
// format flags
const VT_VECTOR_C: i64 = 0x1000;
// other constants
const VT_VARIANT_C: i64 = 12;

fn s(x: &str) -> Value {
    pl::s(x)
}

/// String interpolation: the parts concatenated.
fn cat(parts: &[Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

fn first(v: Vec<Value>) -> Value {
    v.into_iter().next().unwrap_or_default()
}

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn et_call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

fn get8u(it: &mut Interp, dp: &Value, pos: Value) -> ER<Value> {
    Ok(first(get8u_n(it, vec![dp.clone(), pos], Want::Scalar)?))
}

fn get16u(it: &mut Interp, dp: &Value, pos: Value) -> ER<Value> {
    Ok(first(get16u_n(it, vec![dp.clone(), pos], Want::Scalar)?))
}

fn get32u(it: &mut Interp, dp: &Value, pos: Value) -> ER<Value> {
    Ok(first(get32u_n(it, vec![dp.clone(), pos], Want::Scalar)?))
}

fn warn(it: &mut Interp, et: &Value, msg: Value) -> ER<()> {
    pl::method(it, et, "Warn", vec![msg])?;
    Ok(())
}

fn options(it: &mut Interp, et: &Value, name: &str) -> ER<Value> {
    pl::method(it, et, "Options", vec![s(name)])
}

fn add(a: &Value, b: i64) -> Value {
    pl::add(a, &n(b))
}

fn and(a: &Value, b: i64) -> Value {
    ops::bit_and(a, &n(b))
}

/// `$$h{k} = ...` value of `$x || $y`.
fn or(a: Value, b: Value) -> Value {
    if a.truthy() {
        a
    } else {
        b
    }
}

/// `$s =~ s/\0.*//s` (truncate at null terminator)
fn trunc_null(v: &mut Value) {
    re(r"\0.*", "s").subst(v, false, |_| Vec::new());
}

/// `$raf->Read($buff, $len)` into the `my $buff` cell: the count read.
fn raf_read(it: &mut Interp, raf: &Value, buff: &Cell, len: Value) -> ER<Value> {
    let mut args = [buff.borrow().clone(), len];
    let r = pl::method_mut(it, raf, "Read", &mut args, Want::Scalar)?;
    let [b, _] = args;
    *buff.borrow_mut() = b;
    Ok(first(r))
}

fn raf_seek(it: &mut Interp, raf: &Value, offset: Value, whence: i64) -> ER<bool> {
    Ok(pl::method(it, raf, "Seek", vec![offset, n(whence)])?.truthy())
}

/// `print $out STRING`
fn print_out(it: &mut Interp, out: &Value, msg: Value) -> ER<()> {
    pl::print_to(it, Some(out), &[msg])?;
    Ok(())
}

/// `pos($$dataPt)`
fn get_pos(it: &mut Interp, data_pt: &Value) -> ER<Option<usize>> {
    let d = cell(data_pt.clone());
    let v = perl_eval1(it, "pos($$d)", "main", &[("d", &d)])?;
    Ok(if v.is_undef() { None } else { Some(v.to_i64() as usize) })
}

/// `pos($$dataPt) = POS` (undef resets it, as a failed `//g` match does).
fn set_pos(it: &mut Interp, data_pt: &Value, p: Option<usize>) -> ER<()> {
    let d = cell(data_pt.clone());
    let v = cell(match p {
        Some(p) => n(p as i64),
        None => Value::Undef,
    });
    perl_eval(it, "pos($$d) = $p", "main", &[("d", &d), ("p", &v)], Want::Void)?;
    Ok(())
}

/// A scalar `$str =~ /PAT/g` from position `p` (pos() of the string): the
/// captures and the new pos() on a match; `p` becomes None on failure,
/// as perl resets pos() after a failed `//g` match.
fn match_g(pat: &str, flags: &str, subject: &[u8], p: &mut Option<usize>) -> Option<pl::Caps> {
    let rx = pre::compile(pat.as_bytes(), flags).unwrap_or_else(|e| panic!("bad pattern /{pat}/: {e}"));
    match rx.match_at(subject, p.unwrap_or(0)) {
        Ok(Some(sp)) => {
            *p = Some(sp[0].unwrap().1);
            Some(sp.iter().map(|g| g.map(|(a, b)| Value::bytes(&subject[a..b]))).collect())
        }
        _ => {
            *p = None;
            None
        }
    }
}

/// `hex($str)` for the hex strings this file builds with sprintf('%x').
fn hex(v: &Value) -> Value {
    let t = v.to_text();
    let t = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(&t);
    let mut r: u64 = 0;
    for c in t.bytes() {
        let d = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            b'_' => continue,
            _ => break,
        };
        r = r.wrapping_mul(16).wrapping_add(d as u64);
    }
    ops::from_uv(r)
}

/// `shift @$aref`
fn shift_array(a: &Value) -> Value {
    match a {
        Value::ArrayRef(v) => {
            let mut v = v.borrow_mut();
            if v.is_empty() {
                Value::Undef
            } else {
                v.remove(0)
            }
        }
        _ => Value::Undef,
    }
}

// sector type constants
pub fn hdr_size(_it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(n(HDR_SIZE_C))
}
pub fn dif_sect(_it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(n(0xfffffffc))
}
pub fn fat_sect(_it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(n(0xfffffffd))
}
pub fn end_of_chain(_it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(n(END_OF_CHAIN_C))
}
pub fn free_sect(_it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(n(FREE_SECT_C))
}

// format flags
pub fn vt_vector(_it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(n(VT_VECTOR_C))
}
pub fn vt_array(_it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(n(0x2000))
}
pub fn vt_byref(_it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(n(0x4000))
}
pub fn vt_reserved(_it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(n(0x8000))
}

// other constants
pub fn vt_variant(_it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(n(VT_VARIANT_C))
}
pub fn vt_lpstr(_it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(n(30))
}

/// Convert Microsoft DTTM structure to date/time
/// Inputs: 0) DTTM value
/// Returns: EXIF-format date/time string ("0000:00:00 00:00:00" for zero date/time)
pub fn convert_dttm(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let val = arg(&a, 0);
    let mut yr = and(&ops::shift_right(&val, &n(20)), 0x1ff);
    let mon = and(&ops::shift_right(&val, &n(16)), 0x0f);
    let day = and(&ops::shift_right(&val, &n(11)), 0x1f);
    let hr = and(&ops::shift_right(&val, &n(6)), 0x1f);
    let min = and(&val, 0x3f);
    if val.truthy() {
        yr = add(&yr, 1900);
    }
    // ExifTool 12.48 dropped the "Z" on the time here because a test .doc
    // file written by Word 2011 on Mac certainly used local time here
    ret(pl::sprintf("%.4d:%.2d:%.2d %.2d:%.2d:00", &[yr, mon, day, hr, min]))
}

/// Process hyperlinks from PID_HYPERLINKS array
/// (ref http://msdn.microsoft.com/archive/default.asp?url=/archive/en-us/dnaro97ta/html/msdn_hyper97.asp)
/// Inputs: 0) value, 1) ExifTool ref
/// Returns: list of hyperlinks
pub fn process_hyperlinks(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (val, et) = (arg(&a, 0), arg(&a, 1));

    // process as an array of VT_VARIANT's
    let dir_end = pl::length(&val);
    if pl::lt(&dir_end, &n(4)) {
        return pl::ret_undef();
    }
    let val_ref = pl::sref(val);
    let num = get32u(it, &val_ref, n(0))?;
    let mut val_pos = n(4);
    let mut vals: Vec<Value> = Vec::new();
    let mut i = n(0);
    while pl::lt(&i, &num) {
        // read VT_BLOB entries as an array of VT_VARIANT's
        let value = first(read_fpx_value_impl(it, &et, &val_ref, &mut val_pos, &n(VT_VARIANT_C), &dir_end, Value::Undef, &Value::Undef, false)?);
        if value.is_undef() {
            break;
        }
        vals.push(value);
        i = add(&i, 1);
    }
    // filter values to extract only the links
    let mut links: Vec<Value> = Vec::new();
    let mut i = 0usize;
    while i < vals.len() {
        let mut l = vals.get(i + 4).cloned().unwrap_or_default(); // get address
        let sub = vals.get(i + 5).cloned().unwrap_or_default();
        if pl::length(&sub).truthy() {
            l = cat(&[l, s("#"), sub]); // add subaddress
        }
        links.push(l);
        i += 6;
    }
    ret(Value::new_array(links))
}

/// Read FlashPix value
/// Inputs: 0) ExifTool ref, 1) data ref, 2) value offset, 3) FPX format number,
///         4) end offset, 5) flag for no padding, 6) code page
/// Returns: converted value (or list of values in list context) and updates
///          value offset to end of value if successful, or returns undef on error
pub fn read_fpx_value(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let mut val_pos = arg(&a, 2);
    let r = read_fpx_value_impl(it, &arg(&a, 0), &arg(&a, 1), &mut val_pos, &arg(&a, 3), &arg(&a, 4), arg(&a, 5), &arg(&a, 6), want == Want::List)?;
    it.set_arg(2, val_pos)?; // return updated value position
    Ok(r)
}

/// The body of ReadFPXValue: `val_pos` is `$_[2]`, `wantarray` the context.
#[allow(clippy::too_many_arguments)]
fn read_fpx_value_impl(it: &mut Interp, et: &Value, data_pt: &Value, val_pos: &mut Value, type_: &Value, dir_end: &Value, mut no_pad: Value, code_page: &Value, wantarray: bool) -> ER<Vec<Value>> {
    let ole_format = runtime::lex_hash(it, MODULE, "oleFormat");
    let ole_format_size = runtime::lex_hash(it, MODULE, "oleFormatSize");
    let mut vals: Vec<Value> = Vec::new();

    let format = ole_format.get(and(type_, 0x0fff).to_bytes());
    // while ($format) { ... last; }
    if format.truthy() {
        'once: {
            let mut count = n(1);
            // handle VT_VECTOR types
            let flags = and(type_, 0xf000);
            if flags.truthy() {
                if pl::num_eq(&flags, &n(VT_VECTOR_C)) {
                    no_pad = n(1); // values sometimes aren't padded inside vectors!!
                    let size = ole_format_size.get("VT_VECTOR");
                    if pl::gt(&pl::add(val_pos, &size), dir_end) {
                        warn(it, et, s("Incorrect FPX VT_VECTOR size"))?;
                        break 'once;
                    }
                    count = get32u(it, data_pt, val_pos.clone())?;
                    if pl::num_eq(&count, &n(0)) {
                        vals.push(s("")); // allow zero-element vector
                    }
                    *val_pos = add(val_pos, 4);
                } else {
                    // can't yet handle this property flag
                    warn(it, et, s("Unknown FPX property"))?;
                    break 'once;
                }
            }
            if !re(r"^VT_", "").is_match(&format) {
                let size = pl::mul(&et_call(it, "FormatSize", vec![format.clone()])?, &count);
                if pl::gt(&pl::add(val_pos, &size), dir_end) {
                    warn(it, et, cat(&[s("Incorrect FPX "), format.clone(), s(" size")]))?;
                    break 'once;
                }
                vals = pl::call_list(it, "Image::ExifTool::ReadValue", vec![data_pt.clone(), val_pos.clone(), format.clone(), count.clone(), size.clone()])?;
                // update position to end of value plus padding
                *val_pos = pl::add(val_pos, &and(&add(&pl::mul(&count, &size), 3), 0xfffffffc));
                break 'once;
            }
            let size = ole_format_size.get(format.to_bytes());
            let (mut val, mut len) = (Value::Undef, Value::Undef);
            let mut item = n(0);
            while pl::lt(&item, &count) {
                if pl::gt(&pl::add(val_pos, &size), dir_end) {
                    warn(it, et, cat(&[s("Truncated FPX "), format.clone(), s(" value")]))?;
                    break;
                }
                // sometimes VT_VECTOR items are padded to even 4-byte boundaries, and sometimes they aren't
                if no_pad.truthy() && !len.is_undef() && and(&len, 0x03).truthy() {
                    let pad = pl::sub(&n(4), &and(&len, 0x03));
                    if pl::le(&pl::add(&pl::add(val_pos, &pad), &size), dir_end) {
                        // skip padding if all zeros
                        pl::at(1332);
                        let d = substr(it, &pl::sderef(&data_pt)?, val_pos, Some(&pad))?;
                        let zeros = Value::bytes(vec![0u8; pad.to_i64().max(0) as usize]);
                        if d.to_bytes() == zeros.to_bytes() {
                            *val_pos = pl::add(val_pos, &pad);
                        }
                    }
                }
                len = Value::Undef;
                if format.eq_str("VT_VARIANT") {
                    let sub_type = get32u(it, data_pt, val_pos.clone())?;
                    *val_pos = pl::add(val_pos, &size);
                    val = first(read_fpx_value_impl(it, et, data_pt, val_pos, &sub_type, dir_end, no_pad.clone(), code_page, false)?);
                    if val.is_undef() {
                        break;
                    }
                    vals.push(val.clone());
                    item = add(&item, 1);
                    continue; // avoid adding $size to $valPos again
                } else if format.eq_str("VT_FILETIME") {
                    // convert from time in 100 ns increments to time in seconds
                    val = pl::mul(&Value::Num(1e-7), &et_call(it, "Get64u", vec![data_pt.clone(), val_pos.clone()])?);
                    // print as date/time if value is greater than one year (PH hack)
                    let sec_day = pl::mul(&n(24), &n(3600));
                    if pl::gt(&val, &pl::mul(&n(365), &sec_day)) {
                        // shift from Jan 1, 1601 to Jan 1, 1970
                        let unix_time_zero = pl::mul(&n(134774), &sec_day);
                        val = pl::sub(&val, &unix_time_zero);
                        // there are a lot of bad programmers out there...
                        let sec100yr = pl::mul(&pl::mul(&n(100), &n(365)), &sec_day);
                        if pl::lt(&val, &n(0)) || pl::gt(&val, &sec100yr) {
                            // some software writes the wrong byte order (but the proper word order)
                            pl::at(1356);
                            let w = pl::unpack(&format!("x{}NN", val_pos.to_text()), &pl::sderef(&data_pt)?)?;
                            let v2 = pl::sub(&pl::mul(&pl::add(&arg(&w, 0), &pl::mul(&arg(&w, 1), &n(4294967296))), &Value::Num(1e-7)), &unix_time_zero);
                            if pl::gt(&v2, &n(0)) && pl::lt(&v2, &sec100yr) {
                                val = v2;
                            // also check for wrong time base
                            } else if pl::lt(&val, &n(0)) && pl::gt(&pl::add(&val, &unix_time_zero), &n(0)) {
                                val = pl::add(&val, &unix_time_zero);
                            }
                        }
                        val = et_call(it, "ConvertUnixTime", vec![val])?;
                    }
                } else if format.eq_str("VT_DATE") {
                    val = et_call(it, "GetDouble", vec![data_pt.clone(), val_pos.clone()])?;
                    // shift zero from Dec 30, 1899 to Jan 1, 1970 and convert to secs
                    if !pl::num_eq(&val, &n(0)) {
                        val = pl::mul(&pl::mul(&pl::sub(&val, &n(25569)), &n(24)), &n(3600));
                    }
                    val = et_call(it, "ConvertUnixTime", vec![val])?;
                } else if re(r"STR$", "").is_match(&format) {
                    len = get32u(it, data_pt, val_pos.clone())?;
                    if format.eq_str("VT_LPWSTR") {
                        len = pl::mul(&len, &n(2)); // convert to byte count
                    }
                    if pl::gt(&add(&pl::add(val_pos, &len), 4), dir_end) {
                        warn(it, et, cat(&[s("Truncated "), format.clone(), s(" value")]))?;
                        break;
                    }
                    pl::at(1379);
                    val = substr(it, &pl::sderef(&data_pt)?, &add(val_pos, 4), Some(&len))?;
                    if format.eq_str("VT_LPWSTR") {
                        // convert wide string from Unicode
                        val = pl::method(it, et, "Decode", vec![val, s("UTF16")])?;
                    } else if code_page.truthy() {
                        let charset = pl::pkg_hash(it, "Image::ExifTool::charsetName").get(cat(&[s("cp"), code_page.clone()]).to_bytes());
                        if charset.truthy() {
                            val = pl::method(it, et, "Decode", vec![val, charset])?;
                        } else if pl::num_eq(code_page, &n(1200)) {
                            // UTF-16, little endian
                            val = pl::method(it, et, "Decode", vec![val, s("UTF16"), s("II")])?;
                        }
                    }
                    trunc_null(&mut val); // truncate at null terminator
                    // update position for string length
                    // (the spec states that strings should be padded to align
                    //  on even 32-bit boundaries, but this isn't always the case)
                    *val_pos = pl::add(val_pos, &if no_pad.truthy() { len.clone() } else { and(&add(&len, 3), 0xfffffffc) });
                } else if format.eq_str("VT_BLOB") || format.eq_str("VT_CF") {
                    let len = get32u(it, data_pt, val_pos.clone())?; // (use local $len because we always expect padding)
                    if pl::gt(&add(&pl::add(val_pos, &len), 4), dir_end) {
                        warn(it, et, cat(&[s("Truncated "), format.clone(), s(" value")]))?;
                        break;
                    }
                    pl::at(1402);
                    val = substr(it, &pl::sderef(&data_pt)?, &add(val_pos, 4), Some(&len))?;
                    // update position for data length plus padding
                    // (does this padding disappear in arrays too?)
                    *val_pos = pl::add(val_pos, &and(&add(&len, 3), 0xfffffffc));
                } else if format.eq_str("VT_CLSID") {
                    // (a sub argument: $$dataPt autovivifies the lexical $dataPt)
                    pl::at(1407);
                    let dv = if data_pt.is_undef() { Value::Undef } else { pl::sderef(data_pt)? };
                    let d = substr(it, &dv, val_pos, Some(&size))?;
                    val = pl::call(it, "Image::ExifTool::ASF::GetGUID", vec![d])?;
                }
                *val_pos = pl::add(val_pos, &size); // update value pointer to end of value
                vals.push(val.clone());
                item = add(&item, 1);
            }
            // join VT_ values with commas unless we want an array
            if vals.len() > 1 && !wantarray {
                let sep = options(it, et, "ListSep")?;
                vals = vec![pl::join(&sep.to_text(), &vals)];
            }
        }
    }

    if type_.eq_str("0") {
        vals.push(s("")); // (VT_EMPTY)
    }
    if wantarray {
        Ok(vals)
    } else if vals.len() > 1 {
        ret(pl::join(" ", &vals))
    } else {
        ret(vals.into_iter().next().unwrap_or_default())
    }
}

/// Scan for XMP in FLA Contents (ref PH)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
/// Notes: FLA format is proprietary and I couldn't find any documentation,
///        so this routine is entirely based on observations from sample files
pub fn process_contents(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    pl::at(1446);
    let data = pl::sderef(&data_pt)?;
    let b = data.to_bytes();
    let mut is_fla = false;
    // pos($$dataPt), kept here and written back to the scalar
    let mut p = get_pos(it, &data_pt)?;
    let mut xmp_done = false;

    // all of my FLA samples contain "Contents" data, and no other FPX-like samples have
    // this (except Scene7 VNT viles), but check the data for a familiar pattern to be
    // sure this is FLA: the Contents of all of my FLA samples start with two bytes
    // (0x29,0x38,0x3f,0x43 or 0x47, then 0x01) followed by a number of zero bytes
    // (from 0x18 to 0x26 of them, related somehow to the value of the first byte),
    // followed by the string "DocumentPage"
    if re(r"^..\0+\xff\xff\x01\0\x0d\0CDocumentPage", "s").is_match(&data) {
        is_fla = true;
    } else if let Some(m) = match_g(r"^\0{4}.(.{1,255})\x60\xa1\x3f\x22\0{5}(.{8})", "s", &b, &mut p) {
        // this looks like a VNT file
        pl::method(it, &et, "OverrideFileType", vec![s("VNT"), s("image/x-vignette")])?;
        // hack to set proper file description (extension is the same for V-Note files)
        pl::pkg_hash(it, "Image::ExifTool::static_vars").get_or_new_hash("OverrideFileDescription").set("VNT", s("Scene7 Vignette"));
        let name = cap(&m, 1);
        let wh = pl::unpack("V2", &cap(&m, 2))?;
        let (w, h) = (arg(&wh, 0), arg(&wh, 1));
        pl::method(it, &et, "FoundTag", vec![s("ImageWidth"), w])?;
        pl::method(it, &et, "FoundTag", vec![s("ImageHeight"), h])?;
        pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), s("OriginalFileName"), name])?;
        if let Some(m) = match_g(r"\G\x01\0{4}(.{12})", "s", &b, &mut p) {
            // (first 4 bytes seem to be number of objects, next 4 bytes are zero, then ICC size)
            let size = pl::unpack1("x8V", &cap(&m, 1))?;
            // (not useful?) $et->FoundTag(NumObjects => $num);
            let cur = n(p.unwrap_or(0) as i64);
            if size.truthy() && pl::lt(&pl::add(&cur, &size), &n(b.len() as i64)) {
                let dat = substr(it, &data, &cur, Some(&size))?;
                pl::method(it, &et, "FoundTag", vec![s("ICC_Profile"), dat])?;
                p = Some((p.unwrap_or(0) as i64 + size.to_i64()).clamp(0, b.len() as i64) as usize);
            }
            et.set("IeImg_lkup", Value::new_hash());
            et.set("IeImg_class", Value::new_hash());
            // - the byte before \x80 is 0x0d, 0x11 or 0x1f for separate images in my samples,
            //   and 0x1c or 0x23 for inline masks
            // - the byte after \xff\xff is 0x3b in my samples for $1 containing 'VnMask' or 'VnCache'
            while let Some(m) = match_g(r"\x0bTargetRole1(?:.\x80|\xff\xff.\0.\0Vn(\w+))\0\0\x01.{4}(.{24})", "s", &b, &mut p) {
                let u = pl::unpack("Vx4V4", &cap(&m, 2))?;
                let index = arg(&u, 0);
                let coords: Vec<Value> = u.iter().skip(1).cloned().collect();
                if pl::num_eq(&index, &n(0xffffffff)) {
                    continue;
                }
                if et.get("IeImg_lkup").get(index.to_bytes()).truthy() {
                    warn(it, &et, s("Duplicate image index"))?;
                }
                et.get("IeImg_lkup").set(index.to_bytes(), pl::join(" ", &coords));
                if cap(&m, 1).truthy() {
                    et.get("IeImg_class").set(index.to_bytes(), cap(&m, 1));
                }
            }
        }
    }

    // do a brute-force scan of the "Contents" for UTF-16 XMP
    // (this may always be little-endian, but allow for either endianness)
    if match_g(r#"<\0\?\0x\0p\0a\0c\0k\0e\0t\0 \0b\0e\0g\0i\0n\0=\0['"](\0\xff\xfe|\xfe\xff)"#, "", &b, &mut p).is_some() {
        dir_info.set("DirStart", n(p.unwrap_or(0) as i64 - 36));
        if match_g(r#"<\0\?\0x\0p\0a\0c\0k\0e\0t\0 \0e\0n\0d\0=\0['"]\0[wr]\0['"]\0\?\0>\0?"#, "", &b, &mut p).is_some() {
            dir_info.set("DirLen", pl::sub(&n(p.unwrap_or(0) as i64), &dir_info.get("DirStart")));
            set_pos(it, &data_pt, p)?;
            xmp_done = true;
            pl::call(it, "Image::ExifTool::XMP::ProcessXMP", vec![et.clone(), dir_info.clone(), tag_table_ptr.clone()])?;
            // override format if not already FLA but XMP-dc:Format indicates it is
            let format = et.get("VALUE").get("Format");
            if !et.get("FILE_TYPE").eq_str("FLA") && format.truthy() && format.eq_str("application/vnd.adobe.fla") {
                is_fla = true;
            }
        }
    }
    if !xmp_done {
        set_pos(it, &data_pt, p)?;
    }
    if is_fla {
        pl::method(it, &et, "OverrideFileType", vec![s("FLA")])?;
    }
    ret(n(1))
}

/// Process WordDocument stream of MSWord doc file (ref 6)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_word_document(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    if !data_pt.truthy() {
        return ret(n(0));
    }
    pl::at(1506);
    let dir_len = pl::length(&pl::sderef(&data_pt)?);
    // validate the FIB signature
    if !(pl::gt(&dir_len, &n(2)) && pl::num_eq(&get16u(it, &data_pt, n(0))?, &n(0xa5ec))) {
        pl::method(it, &et, "Warn", vec![s("Invalid FIB signature"), n(1)])?;
        return ret(n(0));
    }
    pl::method(it, &et, "ProcessBinaryData", vec![dir_info.clone(), tag_table_ptr.clone()])?; // process FIB
    // continue parsing the WordDocument stream until we find the FibRgFcLcb
    let mut pos = n(32);
    if pl::gt(&add(&pos, 2), &dir_len) {
        return ret(n(0));
    }
    let mut nn = get16u(it, &data_pt, pos.clone())?; // read csw
    pos = pl::add(&pos, &pl::add(&n(2), &pl::mul(&nn, &n(2)))); // skip fibRgW
    if pl::gt(&add(&pos, 2), &dir_len) {
        return ret(n(0));
    }
    nn = get16u(it, &data_pt, pos.clone())?; // read cslw
    pos = pl::add(&pos, &pl::add(&n(2), &pl::mul(&nn, &n(4)))); // skip fibRgLw
    if pl::gt(&add(&pos, 2), &dir_len) {
        return ret(n(0));
    }
    nn = get16u(it, &data_pt, pos.clone())?; // read cbRgFcLcb
    pos = add(&pos, 2); // point to start of fibRgFcLcbBlob
    if pl::gt(&pl::add(&pos, &pl::mul(&nn, &n(8))), &dir_len) {
        return ret(n(0));
    }
    let mut table_offsets: Vec<Value> = Vec::new();
    // save necessary entries for later processing of document table
    // (DOP, CommentBy, LastSavedBy)
    for off in [0xf8, 0x120, 0x238] {
        if pl::gt(&n(off + 8), &pl::mul(&nn, &n(8))) {
            break;
        }
        table_offsets.push(get32u(it, &data_pt, add(&pos, off))?);
        table_offsets.push(get32u(it, &data_pt, add(&pos, off + 4))?);
    }
    let tbl = et_call(it, "GetTagTable", vec![s("Image::ExifTool::FlashPix::DocTable")])?;
    // extract ModifyDate if it exists
    pl::method(it, &et, "HandleTag", vec![tbl.clone(), s("ModifyDate"), Value::Undef, s("DataPt"), data_pt.clone(), s("Start"), add(&pos, 0x2b8), s("Size"), n(8)])?;
    pl::method(it, &et, "HandleTag", vec![tbl, s("TableOffsets"), Value::new_array(table_offsets)])?; // save for later
    // $pos += $n * 8;                 # skip fibRgFcLcbBlob
    // return 0 if $pos + 2 > $dirLen;
    // $n = Get16u($dataPt, $pos);     # read cswNew
    // return 0 if $pos + 2 + $n * 2 > $dirLen;
    // my $nFib = Get16u($dataPt, 2 + ($n ? $pos : 0));
    // $pos += 2 + $n * 2;             # skip fibRgCswNew
    ret(n(1))
}

/// `$key . ($i ? " ($i)" : '')`
fn dup_key(tag: &str, i: i64) -> String {
    if i != 0 {
        format!("{tag} ({i})")
    } else {
        tag.to_string()
    }
}

/// Process Microsoft Word Document Table
/// Inputs: 0) ExifTool object ref
pub fn process_document_table(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let et = arg(&a, 0);
    let value = et.get("VALUE");
    let extra = et.get("TAG_EXTRA");
    // loop through TableOffsets for each sub-document
    let mut i = 0i64;
    loop {
        let key = dup_key("TableOffsets", i);
        let offsets = value.get(&key);
        if offsets.is_undef() {
            break;
        }
        'doc: {
            let doc = or(extra.get_or_new_hash(&key).get("G3"), s(""));
            // get DocFlags for this sub-document
            let mut doc_flags = Value::Undef;
            let mut j = 0i64;
            loop {
                let key = dup_key("DocFlags", j);
                if value.get(&key).is_undef() {
                    break;
                }
                let tmp = or(extra.get_or_new_hash(&key).get("G3"), s(""));
                if tmp.to_bytes() == doc.to_bytes() {
                    doc_flags = value.get(&key);
                    break;
                }
                j += 1;
            }
            if doc_flags.is_undef() {
                break 'doc;
            }
            let tag = if and(&doc_flags, 0x200).truthy() { "Table1" } else { "Table0" };
            // get table for this sub-document
            let mut doc_table = Value::Undef;
            let mut j = 0i64;
            loop {
                let key = dup_key(tag, j);
                if value.get(&key).is_undef() {
                    break;
                }
                let tmp = or(extra.get_or_new_hash(&key).get("G3"), s(""));
                if tmp.to_bytes() == doc.to_bytes() {
                    doc_table = pl::elem_ref(&value, key.as_bytes());
                    break;
                }
                j += 1;
            }
            if doc_table.is_undef() {
                break 'doc;
            }
            // extract DOP and LastSavedBy information from document table
            et.set("DOC_NUM", doc); // use same document number
            let tag_table_ptr = et_call(it, "GetTagTable", vec![s("Image::ExifTool::FlashPix::DocTable")])?;
            for tag in ["DOP", "CommentByBlock", "LastSavedByBlock"] {
                pl::at(1596);
                if pl::aderef(&offsets)?.is_empty() {
                    break;
                }
                let off = shift_array(&offsets);
                let len = shift_array(&offsets);
                if !(len.truthy() && pl::le(&pl::add(&off, &len), &pl::length(&doc_table.deref()))) {
                    continue;
                }
                pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), s(tag), Value::Undef, s("DataPt"), doc_table.clone(), s("Start"), off, s("Size"), len])?;
            }
            et.delete("DOC_NUM");
        }
        i += 1;
    }
    // delete intermediate tags
    for tag in ["TableOffsets", "Table0", "Table1"] {
        let mut i = 0i64;
        loop {
            let key = dup_key(tag, i);
            if value.get(&key).is_undef() {
                break;
            }
            pl::method(it, &et, "DeleteTag", vec![s(&key)])?;
            i += 1;
        }
    }
    // (no return: the value of the last statement, the false loop conditions)
    pl::ret_nothing(want)
}

/// Extract names of comment authors (ref 6)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_comment_by(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let mut pos = dir_info.get("DirStart");
    let end = pl::add(&dir_info.get("DirLen"), &pos);
    pl::method(it, &et, "VerboseDir", vec![dir_info.get("DirName")])?;
    while pl::lt(&add(&pos, 2), &end) {
        let len = get16u(it, &data_pt, pos.clone())?;
        pos = add(&pos, 2);
        if pl::gt(&pl::add(&pos, &pl::mul(&len, &n(2))), &end) {
            break;
        }
        pl::at(1633);
        let d = substr(it, &pl::sderef(&data_pt)?, &pos, Some(&pl::mul(&len, &n(2))))?;
        let author = pl::method(it, &et, "Decode", vec![d, s("UTF16")])?;
        pos = pl::add(&pos, &pl::mul(&len, &n(2)));
        pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), s("CommentBy"), author])?;
    }
    ret(n(1))
}

/// Extract last-saved-by names (ref 5)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_last_saved_by(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let mut pos = dir_info.get("DirStart");
    let end = pl::add(&dir_info.get("DirLen"), &pos);
    if pl::gt(&add(&pos, 6), &end) {
        return ret(n(0));
    }
    pl::method(it, &et, "VerboseDir", vec![dir_info.get("DirName")])?;
    let mut num = get16u(it, &data_pt, add(&pos, 2))?;
    pos = add(&pos, 6);
    while pl::ge(&num, &n(2)) {
        if pl::gt(&add(&pos, 2), &end) {
            break;
        }
        let mut len = get16u(it, &data_pt, pos.clone())?;
        pos = add(&pos, 2);
        if pl::gt(&pl::add(&pos, &pl::mul(&len, &n(2))), &end) {
            break;
        }
        pl::at(1659);
        let d = substr(it, &pl::sderef(&data_pt)?, &pos, Some(&pl::mul(&len, &n(2))))?;
        let author = pl::method(it, &et, "Decode", vec![d, s("UTF16")])?;
        pos = pl::add(&pos, &pl::mul(&len, &n(2)));
        if pl::gt(&add(&pos, 2), &end) {
            break;
        }
        len = get16u(it, &data_pt, pos.clone())?;
        pos = add(&pos, 2);
        if pl::gt(&pl::add(&pos, &pl::mul(&len, &n(2))), &end) {
            break;
        }
        pl::at(1665);
        let d = substr(it, &pl::sderef(&data_pt)?, &pos, Some(&pl::mul(&len, &n(2))))?;
        let path = pl::method(it, &et, "Decode", vec![d, s("UTF16")])?;
        pos = pl::add(&pos, &pl::mul(&len, &n(2)));
        pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), s("LastSavedBy"), cat(&[author, s(" ("), path, s(")")])])?;
        num = pl::sub(&num, &n(2));
    }
    ret(n(1))
}

/// Check FPX byte order mark (BOM) and set byte order appropriately
/// Inputs: 0) data ref, 1) offset to BOM
/// Returns: true on success
pub fn check_bom(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (data_pt, pos) = (arg(&a, 0), arg(&a, 1));
    ret(n(check_bom_impl(it, &data_pt, pos)? as i64))
}

fn check_bom_impl(it: &mut Interp, data_pt: &Value, pos: Value) -> ER<bool> {
    let bom = get16u(it, data_pt, pos)?;
    if pl::num_eq(&bom, &n(0xfffe)) {
        return Ok(true);
    }
    if !pl::num_eq(&bom, &n(0xfeff)) {
        return Ok(false);
    }
    et_call(it, "ToggleByteOrder", vec![])?;
    Ok(true)
}

/// Process FlashPix properties
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_properties(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let mut pos = or(dir_info.get("DirStart"), n(0));
    let mut dir_len = dir_info.get("DirLen");
    if !dir_len.truthy() {
        pl::at(1696);
        dir_len = pl::sub(&pl::length(&pl::sderef(&data_pt)?), &pos);
    }
    let dir_end = pl::add(&pos, &dir_len);
    let verbose = options(it, &et, "Verbose")?;
    let ole_format = runtime::lex_hash(it, MODULE, "oleFormat");
    let ole_flags = runtime::lex_hash(it, MODULE, "oleFlags");

    if pl::lt(&dir_len, &n(48)) {
        warn(it, &et, s("Truncated FPX properties"))?;
        return ret(n(0));
    }
    // check and set our byte order if necessary
    if !check_bom_impl(it, &data_pt, pos.clone())? {
        warn(it, &et, s("Bad FPX property byte order mark"))?;
        return ret(n(0));
    }
    // get position of start of section
    pos = get32u(it, &data_pt, add(&pos, 44))?;
    if pl::lt(&pos, &n(48)) {
        warn(it, &et, s("Bad FPX property section offset"))?;
        return ret(n(0));
    }
    for _n in 0..2 {
        let dictionary = Value::new_hash(); // dictionary to translate user-defined properties
        let mut code_page = Value::Undef;
        if pl::gt(&add(&pos, 8), &dir_end) {
            break;
        }
        // read property section header
        let size = get32u(it, &data_pt, pos.clone())?;
        if !size.truthy() {
            break;
        }
        let num_entries = get32u(it, &data_pt, add(&pos, 4))?;
        if verbose.truthy() {
            pl::method(it, &et, "VerboseDir", vec![s("Property Info"), num_entries.clone(), size.clone()])?;
        }
        if pl::gt(&pl::add(&add(&pos, 8), &pl::mul(&n(8), &num_entries)), &dir_end) {
            warn(it, &et, s("Truncated property list"))?;
            break;
        }
        let mut index = n(0);
        'index: while pl::lt(&index, &num_entries) {
            'entry: {
                let entry = pl::add(&add(&pos, 8), &pl::mul(&n(8), &index));
                let mut tag = get32u(it, &data_pt, entry.clone())?;
                let offset = get32u(it, &data_pt, add(&entry, 4))?;
                let val_start = pl::add(&add(&pos, 4), &offset);
                if pl::ge(&val_start, &dir_end) {
                    break 'index; // last
                }
                let mut val_pos = val_start.clone();
                let type_ = get32u(it, &data_pt, pl::add(&pos, &offset))?;
                if pl::num_eq(&tag, &n(0)) {
                    // read dictionary to get tag name lookup for this property set
                    let mut i = n(0);
                    'dictloop: while pl::lt(&i, &type_) {
                        'dict: {
                            if pl::gt(&add(&val_pos, 8), &dir_end) {
                                break 'dictloop; // last
                            }
                            tag = get32u(it, &data_pt, val_pos.clone())?;
                            let len = get32u(it, &data_pt, add(&val_pos, 4))?;
                            val_pos = pl::add(&val_pos, &add(&len, 8));
                            if pl::gt(&val_pos, &dir_end) {
                                break 'dictloop; // last
                            }
                            pl::at(1747);
                            let mut name = substr(it, &pl::sderef(&data_pt)?, &pl::sub(&val_pos, &len), Some(&len))?;
                            trunc_null(&mut name);
                            if !pl::length(&name).truthy() {
                                break 'dict;
                            }
                            dictionary.set(tag.to_bytes(), name.clone());
                            if tag_table_ptr.get(name.to_bytes()).truthy() {
                                break 'dict;
                            }
                            tag = name.clone();
                            re(r"(^| )([a-z])", "").subst(&mut name, true, |c| pl::uc(&cap(c, 2)).to_bytes().to_vec()); // start with uppercase
                            pl::tr(&mut name, "-_a-zA-Z0-9", "", "dc"); // remove illegal characters
                            if !pl::length(&name).truthy() {
                                break 'dict;
                            }
                            if verbose.truthy() {
                                pl::method(it, &et, "VPrint", vec![n(0), cat(&[et.get("INDENT"), s("[adding "), name.clone(), s("]\n")])])?;
                            }
                            let ti = Value::new_hash();
                            ti.set("Name", name.clone());
                            et_call(it, "AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), ti])?;
                        }
                        i = add(&i, 1);
                    }
                    break 'entry; // next
                }
                // use tag name from dictionary if available
                let mut custom = false;
                if !dictionary.get(tag.to_bytes()).is_undef() {
                    tag = dictionary.get(tag.to_bytes());
                    custom = true;
                }
                let vals = read_fpx_value_impl(it, &et, &data_pt, &mut val_pos, &type_, &dir_end, Value::Undef, &code_page, true)?;
                if vals.is_empty() {
                    warn(it, &et, s("Error reading property value"))?;
                }
                let mut val = if vals.len() > 1 { Value::new_array(vals) } else { vals.into_iter().next().unwrap_or_default() };
                let format = and(&type_, 0x0fff);
                let flags = and(&type_, 0xf000);
                let mut form_str = or(ole_format.get(format.to_bytes()), cat(&[s("Type "), format.clone()]));
                if flags.truthy() {
                    let f = or(ole_flags.get(flags.to_bytes()), pl::sprintf("0x%x", &[flags.clone()]));
                    form_str = cat(&[form_str, s("|"), f]);
                }
                let mut tag_info = Value::Undef;
                // check for common tag ID's: Dictionary, CodePage and LocaleIndicator
                // (must be done before masking because masked tags may overlap these ID's)
                if !custom && (pl::num_eq(&tag, &n(1)) || pl::num_eq(&tag, &n(0x80000000))) {
                    // get tagInfo from SummaryInfo table
                    let summary_table = et_call(it, "GetTagTable", vec![s("Image::ExifTool::FlashPix::SummaryInfo")])?;
                    tag_info = pl::method(it, &et, "GetTagInfo", vec![summary_table, tag.clone()])?;
                    if pl::num_eq(&tag, &n(1)) {
                        if pl::lt(&val, &n(0)) {
                            val = add(&val, 0x10000); // (may be incorrectly stored as int16s)
                        }
                        code_page = val.clone(); // save code page for translating values
                    }
                } else if tag_table_ptr.get(tag.to_bytes()).truthy() {
                    tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
                } else if tag_table_ptr.get("VARS").truthy() && !custom {
                    // mask off insignificant bits of tag ID if necessary
                    let masked = tag_table_ptr.get("VARS");
                    for mask in masked.keys() {
                        let mask = Value::from(mask);
                        let tm = ops::bit_and(&tag, &mask);
                        if masked.get(mask.to_bytes()).get(tm.to_bytes()).truthy() {
                            tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tm])?;
                            break;
                        }
                    }
                }
                pl::method(
                    it,
                    &et,
                    "HandleTag",
                    vec![
                        tag_table_ptr.clone(),
                        tag.clone(),
                        val,
                        s("DataPt"),
                        data_pt.clone(),
                        s("Start"),
                        val_start.clone(),
                        s("Size"),
                        pl::sub(&val_pos, &val_start),
                        s("Format"),
                        form_str,
                        s("Index"),
                        index.clone(),
                        s("TagInfo"),
                        tag_info,
                        s("Extra"),
                        cat(&[s(", type="), type_.clone()]),
                    ],
                )?;
            }
            index = add(&index, 1);
        }
        // issue warning if we hit end of property section prematurely
        if pl::lt(&index, &num_entries) {
            warn(it, &et, s("Truncated property data"))?;
        }
        if !dir_info.get("Multi").truthy() {
            break;
        }
        pos = pl::add(&pos, &size);
    }

    ret(n(1))
}

/// Load chain of sectors from file
/// Inputs: 0) RAF ref, 1) first sector number, 2) FAT ref, 3) sector size, 4) header size
pub fn load_chain(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(load_chain_impl(it, &arg(&a, 0), arg(&a, 1), &arg(&a, 2), &arg(&a, 3), &arg(&a, 4))?)
}

fn load_chain_impl(it: &mut Interp, raf: &Value, mut sect: Value, fat_pt: &Value, sect_size: &Value, hdr_size: &Value) -> ER<Value> {
    if !raf.truthy() {
        return Ok(Value::Undef);
    }
    let mut chain: Vec<u8> = Vec::new();
    let buff = cell(Value::Undef);
    let mut loaded_sect: HashSet<Vec<u8>> = HashSet::new();
    loop {
        if pl::ge(&sect, &n(END_OF_CHAIN_C)) {
            break;
        }
        let key = sect.to_bytes().to_vec();
        if loaded_sect.contains(&key) {
            return Ok(Value::Undef); // avoid infinite loop
        }
        loaded_sect.insert(key);
        let offset = pl::add(&pl::mul(&sect, sect_size), hdr_size);
        let ok = (pl::le(&offset, &n(0x7fffffff)) || raf.get("LargeFileSupport").truthy()) && raf_seek(it, raf, offset, 0)? && pl::num_eq(&raf_read(it, raf, &buff, sect_size.clone())?, sect_size);
        if !ok {
            return Ok(Value::Undef);
        }
        chain.extend_from_slice(&buff.borrow().to_bytes());
        // step to next sector in chain
        pl::at(1836);
        if pl::gt(&pl::mul(&sect, &n(4)), &pl::sub(&pl::length(&pl::sderef(&fat_pt)?), &n(4))) {
            return Ok(Value::Undef);
        }
        sect = get32u(it, fat_pt, pl::mul(&sect, &n(4)))?;
    }
    Ok(Value::bytes(chain))
}

/// Extract information from a JPEG APP2 FPXR segment
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_fpxr(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let dir_start = dir_info.get("DirStart");
    let dir_len = dir_info.get("DirLen");
    let verbose = options(it, &et, "Verbose")?;

    if pl::lt(&dir_len, &n(13)) {
        warn(it, &et, s("FPXR segment too small"))?;
        return ret(n(0));
    }
    pl::at(1860);
    let data = pl::sderef(&data_pt)?;

    // get version and segment type (version is 0 in all my samples)
    let vt = pl::unpack("x5C2", &data)?;
    let (vers, type_) = (arg(&vt, 0), arg(&vt, 1));

    if pl::num_eq(&type_, &n(1)) {
        // a "Contents List" segment

        if !pl::num_eq(&vers, &n(0)) {
            warn(it, &et, cat(&[s("Untested FPXR version "), vers.clone()]))?;
        }
        if et.get("FPXR").truthy() {
            warn(it, &et, s("Multiple FPXR contents lists"))?;
            et.delete("FPXR");
        }
        let num_entries = pl::unpack1("x7n", &data)?;
        let mut contents: Vec<Value> = Vec::new();
        if verbose.truthy() {
            pl::method(it, &et, "VerboseDir", vec![s("Contents List"), num_entries.clone()])?;
        }
        let b = data.to_bytes();
        let mut pos = n(9);
        // pos($$dataPt), once this sub sets it
        let mut p: Option<Option<usize>> = None;
        let mut entry = n(0);
        while pl::lt(&entry, &num_entries) {
            if pl::gt(&add(&pos, 4), &dir_len) {
                warn(it, &et, s("Truncated FPXR contents"))?;
                if let Some(p) = p {
                    set_pos(it, &data_pt, p)?;
                }
                return ret(n(0));
            }
            let u = pl::unpack(&format!("x{}Na", pos.to_text()), &data)?;
            let (size, default) = (arg(&u, 0), arg(&u, 1));
            let mut cur = Some((pos.to_i64() + 5).clamp(0, b.len() as i64) as usize);
            // according to the spec, this string is little-endian
            // (very odd, since the size word is big-endian),
            // and the first char must be '/'
            let Some(m) = match_g(r"\G(/\0(..)*?)\0\0", "s", &b, &mut cur) else {
                set_pos(it, &data_pt, cur)?;
                warn(it, &et, s("Invalid FPXR stream name"))?;
                return ret(n(0));
            };
            p = Some(cur);
            // convert stream pathname to ascii
            let mut name = et_call(it, "Decode", vec![Value::Undef, cap(&m, 1), s("UTF16"), s("II"), s("Latin")])?;
            if verbose.truthy() {
                let psize = if pl::num_eq(&size, &n(0xffffffff)) { s("storage") } else { cat(&[size.clone(), s(" bytes")]) };
                pl::method(it, &et, "VPrint", vec![n(0), cat(&[s("  |  "), entry.clone(), s(") Name: '"), name.clone(), s("' ["), psize, s("]\n")])])?;
            }
            // remove directory specification
            re(r".*/", "s").subst(&mut name, false, |_| Vec::new());
            // read storage class ID if necessary
            let mut class_id = Value::Undef;
            if pl::num_eq(&size, &n(0xffffffff)) {
                let Some(m) = match_g(r"(.{16})", "s", &b, &mut cur) else {
                    set_pos(it, &data_pt, cur)?;
                    warn(it, &et, s("Truncated FPXR storage class ID"))?;
                    return ret(n(0));
                };
                p = Some(cur);
                // unpack class ID in case we want to use it sometime
                class_id = pl::call(it, "Image::ExifTool::ASF::GetGUID", vec![cap(&m, 1)])?;
            }
            // find the tagInfo if available
            let mut tag_info = Value::Undef;
            if !tag_table_ptr.get(name.to_bytes()).truthy() {
                // remove instance number or class ID from tag if necessary
                if let Some(id) = base_id(&tag_table_ptr, &name, &[r"(.*) \d{6}$", r"(.*)_[0-9a-f]{16}$"]) {
                    tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), id])?;
                }
            }
            // update position in list
            pos = n(cur.unwrap_or(0) as i64);
            // add to our contents list
            let h = Value::new_hash();
            h.set("Name", name);
            h.set("Size", size);
            h.set("Default", default);
            h.set("ClassID", class_id);
            h.set("TagInfo", tag_info);
            contents.push(h);
            entry = add(&entry, 1);
        }
        if let Some(p) = p {
            set_pos(it, &data_pt, p)?;
        }
        // save contents list as $et member variable
        // (must do this last so we don't save list on error)
        et.set("FPXR", Value::new_array(contents));
    } else if pl::num_eq(&type_, &n(2)) {
        // a "Stream Data" segment

        // get the contents list index and stream data offset
        let u = pl::unpack("x7nN", &data)?;
        let (index, offset) = (arg(&u, 0), arg(&u, 1));
        let fpxr = et.get("FPXR");
        if fpxr.truthy() && fpxr.at(index.to_i64()).truthy() {
            let obj = fpxr.at(index.to_i64());
            // extract stream data (after 13-byte header)
            if obj.get("Stream").is_undef() {
                // ignore offset for first segment of this type
                // (in my sample images, this isn't always zero as one would expect)
                let st = substr(it, &data, &add(&dir_start, 13), None)?;
                obj.set("Stream", st);
            } else {
                // add data at the proper offset to the stream
                let overlap = pl::sub(&pl::length(&obj.get("Stream")), &offset);
                let mut start = add(&dir_start, 13);
                if pl::lt(&overlap, &n(0)) || pl::lt(&pl::sub(&dir_len, &overlap), &n(13)) {
                    pl::method(it, &et, "Warn", vec![cat(&[s("Bad FPXR stream "), index.clone(), s(" offset")]), n(1)])?;
                } else {
                    // ignore any overlapping data in this segment
                    // (this seems to be the convention)
                    start = pl::add(&start, &overlap);
                }
                // concatenate data with this stream
                let more = substr(it, &data, &start, None)?;
                obj.set("Stream", pl::concat(&obj.get("Stream"), &more));
            }
            // save value for this tag if stream is complete
            let len = pl::length(&obj.get("Stream"));
            if pl::ge(&len, &obj.get("Size")) {
                if verbose.truthy() {
                    pl::method(it, &et, "VPrint", vec![n(0), cat(&[s("  + [FPXR stream "), index.clone(), s(", "), len.clone(), s(" bytes]\n")])])?;
                }
                if pl::gt(&len, &obj.get("Size")) {
                    warn(it, &et, s("Extra data in FPXR segment (truncated)"))?;
                    let st = substr(it, &obj.get("Stream"), &n(0), Some(&obj.get("Size")))?;
                    obj.set("Stream", st);
                }
                // handle this tag
                pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), obj.get("Name"), obj.get("Stream"), s("DataPt"), pl::elem_ref(&obj, b"Stream"), s("TagInfo"), obj.get("TagInfo")])?;
                obj.delete("Stream"); // done with this stream
            }
        // hack for improperly stored FujiFilm PreviewImage (stored with no contents list)
        } else if pl::num_eq(&index, &n(512))
            && pl::gt(&dir_len, &n(60))
            && (et.get("FujiPreview").truthy() || (pl::gt(&dir_len, &n(64)) && substr(it, &data, &add(&dir_start, 60), Some(&n(4)))?.to_bytes()[..] == b"\xff\xd8\xff\xdb"[..]))
        {
            if et.get("FujiPreview").is_undef() {
                et.set("FujiPreview", s(""));
            }
            // recombine PreviewImage, skipping 13-byte FPXR header + 47-byte Fuji header
            let more = substr(it, &data, &add(&dir_start, 60), None)?;
            et.set("FujiPreview", pl::concat(&et.get("FujiPreview"), &more));
        } else {
            // (Kodak uses index 255 for a free segment in images from some cameras)
            if !pl::num_eq(&index, &n(255)) {
                warn(it, &et, cat(&[s("Unlisted FPXR segment (index "), index.clone(), s(")")]))?;
            }
        }
    } else if !pl::num_eq(&type_, &n(3)) {
        // not a "Reserved" segment

        warn(it, &et, cat(&[s("Unknown FPXR segment (type "), type_.clone(), s(")")]))?;
    }

    // clean up if this was the last FPXR segment
    if dir_info.get("LastFPXR").truthy() {
        if et.get("FPXR").truthy() {
            for obj in et.get("FPXR").items() {
                let st = obj.get("Stream");
                if !(!st.is_undef() && pl::length(&st).truthy()) {
                    continue;
                }
                // parse it even though it isn't the proper length
                pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), obj.get("Name"), obj.get("Stream"), s("DataPt"), pl::elem_ref(&obj, b"Stream"), s("TagInfo"), obj.get("TagInfo")])?;
            }
            et.delete("FPXR"); // delete our temporary variables
        }
        if et.get("FujiPreview").truthy() {
            pl::method(it, &et, "FoundTag", vec![s("PreviewImage"), et.get("FujiPreview")])?;
            et.delete("FujiPreview");
        }
    }
    ret(n(1))
}

/// `($tag =~ /RE1/s and $$tagTablePtr{$1}) or ($tag =~ /RE2/s and $$tagTablePtr{$1}) ...`:
/// the `$1` of the first alternative that holds.
fn base_id(tag_table_ptr: &Value, tag: &Value, pats: &[&'static str]) -> Option<Value> {
    for pat in pats {
        if let Some(m) = re(*pat, "s").m(tag) {
            let id = cap(&m, 1);
            if tag_table_ptr.get(id.to_bytes()).truthy() {
                return Some(id);
            }
        }
    }
    None
}

/// Set document number for objects
/// Inputs: 0) object hierarchy hash ref, 1) object index, 2) doc number list ref,
///         3) doc numbers used at each level, 4) flag set for metadata levels
pub fn set_doc_num(_it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    set_doc_num_impl(&arg(&a, 0), &arg(&a, 1), &arg(&a, 2), arg(&a, 3), &arg(&a, 4));
    pl::ret_nothing(want)
}

fn set_doc_num_impl(hier: &Value, index: &Value, doc: &Value, mut used: Value, meta: &Value) {
    let obj = hier.get(index.to_bytes());
    if !obj.truthy() {
        return;
    }
    if obj.exists("DocNum") {
        return;
    }
    obj.set("DocNum", doc.clone());
    if obj.get("Left").truthy() {
        set_doc_num_impl(hier, &obj.get("Left"), doc, used.clone(), meta);
    }
    if obj.get("Right").truthy() {
        set_doc_num_impl(hier, &obj.get("Right"), doc, used.clone(), meta);
    }
    if !obj.get("Child").is_undef() {
        if !used.truthy() {
            used = Value::new_array(Vec::new());
        }
        let mut sub_doc: Vec<Value> = Vec::new();
        if doc.truthy() {
            sub_doc.extend(doc.items());
        }
        // we must dive down 2 levels for each sub-document, so use the
        // $meta flag to add a sub-document level only for every 2nd generation
        if meta.truthy() {
            let sub_num = or(used.at(sub_doc.len() as i64), n(0));
            used.set_at(sub_doc.len(), sub_num.clone());
            sub_doc.push(sub_num);
        } else if !sub_doc.is_empty() {
            let last = sub_doc.len() - 1;
            let v = add(&used.at(last as i64), 1);
            used.set_at(last, v.clone());
            sub_doc[last] = v;
        }
        let not_meta = if meta.truthy() { s("") } else { n(1) };
        set_doc_num_impl(hier, &obj.get("Child"), &Value::new_array(sub_doc), used, &not_meta);
    }
}

/// Extract information from a FlashPix (FPX) file
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref
/// Returns: 1 on success, 0 if this wasn't a valid FPX-format file
pub fn process_fpx(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let mut raf = dir_info.get("RAF");
    let buff = cell(Value::Undef);
    let mini_stream_buff = cell(Value::Undef);
    let (mut out, mut old_indent) = (Value::Undef, Value::Undef);
    let hier = Value::new_hash();
    let obj_index = Value::new_hash();
    let mut loaded_dif_sect: HashSet<Vec<u8>> = HashSet::new();
    let fpx_file_type = runtime::lex_hash(it, MODULE, "fpxFileType");
    let dir_entry_type = runtime::lex_array(it, MODULE, "dirEntryType");

    // handle FPX format in memory from PNG cpIp chunk
    if !raf.truthy() {
        raf = pl::call(it, "File::RandomAccess::new", vec![s("File::RandomAccess"), dir_info.get("DataPt")])?;
    }

    // read header
    if !pl::num_eq(&raf_read(it, &raf, &buff, n(HDR_SIZE_C))?, &n(HDR_SIZE_C)) {
        return ret(n(0));
    }
    // check signature
    if !re(r"^\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1", "").is_match(&buff.borrow()) {
        return ret(n(0));
    }

    // set FileType initially based on file extension (we may override this later)
    let mut file_type = et.get("FILE_EXT");
    if !(file_type.truthy() && fpx_file_type.get(file_type.to_bytes()).truthy()) {
        file_type = s("FPX");
    }
    pl::method(it, &et, "SetFileType", vec![file_type])?;
    let bo = substr(it, &buff.borrow().clone(), &n(0x1c), Some(&n(2)))?;
    et_call(it, "SetByteOrder", vec![s(if bo.to_bytes()[..] == b"\xff\xfe"[..] { "MM" } else { "II" })])?;
    let tag_table_ptr = et_call(it, "GetTagTable", vec![s("Image::ExifTool::FlashPix::Main")])?;
    let verbose = options(it, &et, "Verbose")?;
    // copy LargeFileSupport option to RAF for use in LoadChain
    let lfs = options(it, &et, "LargeFileSupport")?;
    raf.set("LargeFileSupport", lfs);

    let hdr = pl::sref(buff.borrow().clone());
    let sect_size = ops::shift_left(&n(1), &get16u(it, &hdr, n(0x1e))?);
    let mini_size = ops::shift_left(&n(1), &get16u(it, &hdr, n(0x20))?);
    let fat_count = get32u(it, &hdr, n(0x2c))?; // number of FAT sectors
    let dir_start = get32u(it, &hdr, n(0x30))?; // first directory sector
    let mini_cutoff = get32u(it, &hdr, n(0x38))?; // minimum size for big-FAT streams
    let mini_start = get32u(it, &hdr, n(0x3c))?; // first sector of mini-FAT
    let mini_count = get32u(it, &hdr, n(0x40))?; // number of mini-FAT sectors
    let mut dif_start = get32u(it, &hdr, n(0x44))?; // first sector of DIF chain
    let dif_count = get32u(it, &hdr, n(0x48))?; // number of DIF sectors

    if verbose.truthy() {
        out = options(it, &et, "TextOut")?;
        print_out(it, &out, cat(&[s("  Sector size="), sect_size.clone(), s("\n  FAT: Count="), fat_count.clone(), s("\n")]))?;
        print_out(it, &out, cat(&[s("  DIR: Start="), dir_start.clone(), s("\n")]))?;
        print_out(it, &out, cat(&[s("  MiniFAT: Mini-sector size="), mini_size.clone(), s(" Start="), mini_start.clone(), s(" Count="), mini_count.clone(), s(" Cutoff="), mini_cutoff.clone(), s("\n")]))?;
        print_out(it, &out, cat(&[s("  DIF FAT: Start="), dif_start.clone(), s(" Count="), dif_count.clone(), s("\n")]))?;
    }
    //
    // load the FAT
    //
    let mut pos = n(0x4c);
    let mut end_pos = pl::length(&buff.borrow());
    let mut fat: Vec<u8> = Vec::new();
    let mut fat_count_check = n(0);
    let mut dif_count_check = n(0);
    let hdr_size = if pl::gt(&sect_size, &n(HDR_SIZE_C)) { sect_size.clone() } else { n(HDR_SIZE_C) };

    loop {
        while pl::le(&pos, &pl::sub(&end_pos, &n(4))) {
            let sect = get32u(it, &pl::sref(buff.borrow().clone()), pos.clone())?;
            pos = add(&pos, 4);
            if pl::num_eq(&sect, &n(FREE_SECT_C)) {
                continue;
            }
            let offset = pl::add(&pl::mul(&sect, &sect_size), &hdr_size);
            let fat_sect = cell(Value::Undef);
            if !(raf_seek(it, &raf, offset, 0)? && pl::num_eq(&raf_read(it, &raf, &fat_sect, sect_size.clone())?, &sect_size)) {
                pl::method(it, &et, "Error", vec![cat(&[s("Error reading FAT from sector "), sect])])?;
                return ret(n(1));
            }
            fat.extend_from_slice(&fat_sect.borrow().to_bytes());
            fat_count_check = add(&fat_count_check, 1);
        }
        if pl::ge(&dif_start, &n(END_OF_CHAIN_C)) {
            break;
        }
        // read next DIF (Dual Indirect FAT) sector
        dif_count_check = add(&dif_count_check, 1);
        if pl::gt(&dif_count_check, &dif_count) {
            warn(it, &et, s("Unterminated DIF FAT"))?;
            break;
        }
        if loaded_dif_sect.contains(&dif_start.to_bytes().to_vec()) {
            warn(it, &et, s("Cyclical reference in DIF FAT"))?;
            break;
        }
        let offset = pl::add(&pl::mul(&dif_start, &sect_size), &hdr_size);
        if !(raf_seek(it, &raf, offset, 0)? && pl::num_eq(&raf_read(it, &raf, &buff, sect_size.clone())?, &sect_size)) {
            pl::method(it, &et, "Error", vec![cat(&[s("Error reading DIF sector "), dif_start])])?;
            return ret(n(1));
        }
        loaded_dif_sect.insert(dif_start.to_bytes().to_vec());
        // set end of sector information in this DIF
        pos = n(0);
        end_pos = pl::sub(&sect_size, &n(4));
        // next time around we want to read next DIF in chain
        dif_start = get32u(it, &pl::sref(buff.borrow().clone()), end_pos.clone())?;
    }
    if !pl::num_eq(&fat_count_check, &fat_count) {
        warn(it, &et, cat(&[s("Bad number of FAT sectors (expected "), fat_count.clone(), s(" but found "), fat_count_check.clone(), s(")")]))?;
    }
    //
    // load the mini-FAT and the directory
    //
    let fat = Value::bytes(fat);
    let fat_ref = pl::sref(fat.clone());
    let mini_fat = load_chain_impl(it, &raf, mini_start, &fat_ref, &sect_size, &hdr_size)?;
    let dir = load_chain_impl(it, &raf, dir_start, &fat_ref, &sect_size, &hdr_size)?;
    if mini_fat.is_undef() || dir.is_undef() {
        pl::method(it, &et, "Error", vec![s("Error reading mini-FAT or directory stream")])?;
        return ret(n(1));
    }
    let mini_fat_ref = pl::sref(mini_fat.clone());
    let dir_ref = pl::sref(dir.clone());
    if verbose.truthy() {
        print_out(it, &out, cat(&[s("  FAT ["), pl::length(&fat), s(" bytes]:\n")]))?;
        pl::method(it, &et, "VerboseDump", vec![fat_ref.clone()])?;
        print_out(it, &out, cat(&[s("  Mini-FAT ["), pl::length(&mini_fat), s(" bytes]:\n")]))?;
        pl::method(it, &et, "VerboseDump", vec![mini_fat_ref.clone()])?;
        print_out(it, &out, cat(&[s("  Directory ["), pl::length(&dir), s(" bytes]:\n")]))?;
        pl::method(it, &et, "VerboseDump", vec![dir_ref.clone()])?;
    }
    //
    // process the directory
    //
    if verbose.truthy() {
        old_indent = et.get("INDENT");
        et.set("INDENT", pl::concat(&et.get("INDENT"), &s("| ")));
        pl::method(it, &et, "VerboseDir", vec![s("FPX"), Value::Undef, pl::length(&dir)])?;
    }
    let mut mini_stream = Value::Undef;
    let end_pos = pl::length(&dir);
    let mut index = n(0);
    let mut ee = Value::Undef; // name of next tag to extract if unknown
    if options(it, &et, "ExtractEmbedded")?.truthy() {
        ee = n(0);
    }

    let mut pos = n(0);
    'dirent: while pl::le(&pos, &pl::sub(&end_pos, &n(128))) {
        'body: {
            // get directory entry type
            // (0=invalid, 1=storage, 2=stream, 3=lockbytes, 4=property, 5=root)
            let type_ = get8u(it, &dir_ref, add(&pos, 0x42))?;
            if pl::num_eq(&type_, &n(0)) {
                break 'body; // skip invalid entries
            }
            if pl::gt(&type_, &n(5)) {
                warn(it, &et, cat(&[s("Invalid directory entry type "), type_.clone()]))?;
                break 'dirent; // rest of directory is probably garbage
            }
            // get entry name (note: this is supposed to be length in 2-byte
            // characters but this isn't what is done in my sample FPX file, so
            // be very tolerant of this count -- it's null terminated anyway)
            let mut len = get16u(it, &dir_ref, add(&pos, 0x40))?;
            if pl::gt(&len, &n(32)) {
                len = n(32);
            }
            let nm = substr(it, &dir, &pos, Some(&pl::mul(&len, &n(2))))?;
            let mut tag = et_call(it, "Decode", vec![Value::Undef, nm, s("UTF16"), s("II"), s("Latin")])?;
            trunc_null(&mut tag); // truncate at null (in case length was wrong)

            if tag.eq_str("0") && ee.is_undef() {
                pl::method(it, &et, "Warn", vec![s("Use the ExtractEmbedded option to extract embedded information"), n(3)])?;
            }
            let sect = get32u(it, &dir_ref, add(&pos, 0x74))?; // start sector number
            let mut size = get32u(it, &dir_ref, add(&pos, 0x78))?; // stream length

            // load Ministream (referenced from first directory entry)
            if !mini_stream.truthy() {
                let msb = load_chain_impl(it, &raf, sect.clone(), &fat_ref, &sect_size, &hdr_size)?;
                *mini_stream_buff.borrow_mut() = msb;
                if mini_stream_buff.borrow().is_undef() {
                    warn(it, &et, s("Error loading Mini-FAT stream"))?;
                    break 'dirent;
                }
                mini_stream = pl::call(it, "File::RandomAccess::new", vec![s("File::RandomAccess"), Value::ScalarRef(mini_stream_buff.clone())])?;
            }

            let mut tag_info = Value::Undef;
            if tag_table_ptr.get(tag.to_bytes()).truthy() {
                tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
            } else if !ee.is_undef() && tag.to_bytes() == ee.to_bytes() {
                tag_info = s(""); // won't know the actual tagID untile we read the stream
                ee = pl::sprintf("%x", &[add(&hex(&ee), 1)]); // tag to look for next
            } else {
                // remove instance number or class ID from tag if necessary
                if let Some(id) = base_id(&tag_table_ptr, &tag, &[r"(.*) \d{6}$", r"(.*)_[0-9a-f]{16}$", r"(.*)_[0-9]{4}$"]) {
                    // (the last one is for IeImg instances)
                    tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), id])?;
                }
            }

            let l_sib = get32u(it, &dir_ref, add(&pos, 0x44))?; // left sibling
            let r_sib = get32u(it, &dir_ref, add(&pos, 0x48))?; // right sibling
            let chld = get32u(it, &dir_ref, add(&pos, 0x4c))?; // child directory

            // save information about object hierarchy
            let obj = hier.get_or_new_hash(index.to_bytes());
            if !pl::num_eq(&l_sib, &n(FREE_SECT_C)) {
                obj.set("Left", l_sib.clone());
            }
            if !pl::num_eq(&r_sib, &n(FREE_SECT_C)) {
                obj.set("Right", r_sib.clone());
            }
            if !pl::num_eq(&chld, &n(FREE_SECT_C)) {
                obj.set("Child", chld.clone());
                let sub = hier.get_or_new_hash(chld.to_bytes());
                sub.set("Parent", index.clone());
            }

            if !(!tag_info.is_undef() || verbose.truthy()) {
                break 'body;
            }

            // load the data for stream types
            let mut extra = s("");
            let type_str = or(dir_entry_type.at(type_.to_i64()), type_.clone());
            let mut size_defined = true;
            if type_str.eq_str("STREAM") {
                let b = if pl::ge(&size, &mini_cutoff) {
                    // stream is in the main FAT
                    load_chain_impl(it, &raf, sect.clone(), &fat_ref, &sect_size, &hdr_size)?
                } else if size.truthy() {
                    // stream is in the mini-FAT
                    load_chain_impl(it, &mini_stream, sect.clone(), &mini_fat_ref, &mini_size, &n(0))?
                } else {
                    s("") // an empty stream
                };
                *buff.borrow_mut() = b;
                if buff.borrow().is_undef() {
                    let name = if tag_info.truthy() { tag_info.get("Name") } else { s("unknown") };
                    warn(it, &et, cat(&[s("Error reading "), name, s(" stream")]))?;
                    *buff.borrow_mut() = s("");
                }
            } else if type_str.eq_str("ROOT") {
                let b = mini_stream_buff.borrow().clone();
                *buff.borrow_mut() = b;
                extra = pl::concat(&extra, &s(" (Ministream)"));
            } else {
                *buff.borrow_mut() = s("");
                size = Value::Undef;
                size_defined = false;
            }
            if verbose.truthy() {
                let flags = get8u(it, &dir_ref, add(&pos, 0x43))?; // 0=red, 1=black
                let col = or(
                    match &flags.to_bytes()[..] {
                        b"0" => s("Red"),
                        b"1" => s("Black"),
                        _ => Value::Undef,
                    },
                    flags.clone(),
                );
                extra = cat(&[extra, s(" Type="), type_str.clone(), s(" Flags="), col]);
                if !pl::num_eq(&l_sib, &n(FREE_SECT_C)) {
                    extra = cat(&[extra, s(" Left="), l_sib.clone()]);
                }
                if !pl::num_eq(&r_sib, &n(FREE_SECT_C)) {
                    extra = cat(&[extra, s(" Right="), r_sib.clone()]);
                }
                if !pl::num_eq(&chld, &n(FREE_SECT_C)) {
                    extra = cat(&[extra, s(" Child="), chld.clone()]);
                }
                if size_defined {
                    extra = cat(&[extra, s(" Size="), size.clone()]);
                }
                let mut name = Value::Undef;
                if !tag_info.truthy() && re(r"^[0-9a-f]{1,3}$", "").is_match(&tag) {
                    name = cat(&[s("Unknown_0x"), tag.clone()]);
                }
                let bv = buff.borrow().clone();
                pl::method(
                    it,
                    &et,
                    "VerboseInfo",
                    vec![
                        tag.clone(),
                        tag_info.clone(),
                        s("Index"),
                        index.clone(),
                        s("Value"),
                        bv,
                        s("DataPt"),
                        Value::ScalarRef(buff.clone()),
                        s("Extra"),
                        extra.clone(),
                        // Size   => $size, (moved to $extra so we can see the rest of the stream if larger)
                        s("Name"),
                        name,
                    ],
                )?;
            }
            if !tag_info.is_undef() && buff.borrow().truthy() {
                let mut num = et.get("NUM_FOUND");
                if tag_info.truthy() && tag_info.get("SubDirectory").truthy() {
                    let subdir = tag_info.get("SubDirectory");
                    let di = Value::new_hash();
                    di.set("DataPt", Value::ScalarRef(buff.clone()));
                    di.set("DirStart", subdir.get("DirStart"));
                    di.set("DirLen", pl::length(&buff.borrow()));
                    di.set("Multi", tag_info.get("Multi"));
                    let sub_table_ptr = et_call(it, "GetTagTable", vec![subdir.get("TagTable")])?;
                    pl::method(it, &et, "ProcessDirectory", vec![di, sub_table_ptr, subdir.get("ProcessProc")])?;
                } else if size_defined && pl::gt(&size, &pl::length(&buff.borrow())) {
                    warn(it, &et, s("Truncated object"))?;
                } else {
                    if size_defined && pl::lt(&size, &pl::length(&buff.borrow())) {
                        let b = substr(it, &buff.borrow().clone(), &n(0), Some(&size))?;
                        *buff.borrow_mut() = b;
                    }
                    let bv = buff.borrow().clone();
                    if let Some(m) = re(r"^IeImg_0*(\d+)$", "").m(&tag) {
                        // set document number for embedded images and their positions (if available, VNT files)
                        let num = cap(&m, 1);
                        let dc = add(&et.get("DOC_COUNT"), 1);
                        et.set("DOC_COUNT", dc.clone());
                        et.set("DOC_NUM", dc);
                        pl::method(it, &et, "FoundTag", vec![tag_info.clone(), bv])?;
                        if et.get("IeImg_lkup").truthy() && et.get("IeImg_lkup").get(num.to_bytes()).truthy() {
                            // save position of this image
                            let r = et.get("IeImg_lkup").get(num.to_bytes());
                            pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), s("IeImg_rect"), r])?;
                            et.get("IeImg_lkup").delete(num.to_bytes());
                            if et.get("IeImg_class").truthy() && et.get("IeImg_class").get(num.to_bytes()).truthy() {
                                let c = et.get("IeImg_class").get(num.to_bytes());
                                pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), s("IeImg_class"), c])?;
                                et.get("IeImg_class").delete(num.to_bytes());
                            }
                        }
                        et.delete("DOC_NUM");
                    } else if !tag_info.truthy() {
                        // extract some embedded information from PNG Plus images
                        // (the //g match leaves pos() on $buff, which nothing reads
                        //  before $buff is assigned again)
                        if let Some(m) = re(r"^(.{19,40})(\xff\xd8\xff\xe0|\x89PNG\r\n\x1a\n)", "s").m(&bv) {
                            let id = if cap(&m, 2).to_bytes()[..] == b"\xff\xd8\xff\xe0"[..] { "_eeJPG" } else { "_eePNG" };
                            let d = substr(it, &bv, &pl::length(&cap(&m, 1)), None)?;
                            pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), s(id), d])?;
                        } else if re(r"^\0\x80\0\0\x01\0\0\0\x0e\0", "").is_match(&bv) && pl::gt(&pl::length(&bv), &n(18)) {
                            let len = pl::unpack1("x17C", &bv)?;
                            if pl::gt(&add(&len, 18), &pl::length(&bv)) {
                                break 'body; // next
                            }
                            let d = substr(it, &bv, &n(18), Some(&len))?;
                            let filename = pl::method(it, &et, "Decode", vec![d, s("UTF16"), s("II")])?;
                            pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), s("_eeLink"), filename])?;
                        } else {
                            break 'body; // next
                        }
                    } else {
                        pl::method(it, &et, "FoundTag", vec![tag_info.clone(), bv])?;
                    }
                }
                // save object index number for all found tags
                let num2 = et.get("NUM_FOUND");
                while pl::lt(&num, &num2) {
                    num = add(&num, 1);
                    obj_index.set(num.to_bytes(), index.clone());
                }
            }
        }
        pos = add(&pos, 128);
        index = add(&index, 1);
    }
    // set document numbers for tags extracted from embedded documents
    if !et.get("DOC_NUM").truthy() {
        // initialize document number for all objects, beginning at root (index 0)
        set_doc_num_impl(&hier, &n(0), &Value::Undef, Value::Undef, &Value::Undef);
        // set family 3 group name for all tags in embedded documents
        let order = et.get("FILE_ORDER");
        let mut pri: Vec<Value> = Vec::new();
        for tag in order.keys() {
            let tag = Value::from(tag);
            let num = order.get(tag.to_bytes());
            if num.is_undef() || !obj_index.get(num.to_bytes()).truthy() {
                continue;
            }
            let obj = hier.get(obj_index.get(num.to_bytes()).to_bytes());
            if !obj.truthy() {
                continue;
            }
            let doc_nums = obj.get("DocNum");
            if !(doc_nums.truthy() && !doc_nums.is_empty()) {
                continue;
            }
            et.get_or_new_hash("TAG_EXTRA").get_or_new_hash(tag.to_bytes()).set("G3", pl::join("-", &doc_nums.items()));
            if !re(r" ", "").is_match(&tag) {
                pri.push(tag); // save keys for priority sub-doc tags
            }
        }
        // swap priority sub-document tags with main document tags if they exist
        for tag in pri {
            let mut copy = 1;
            loop {
                let key = cat(&[tag.clone(), s(&format!(" ({copy})"))]);
                if et.get("VALUE").get(key.to_bytes()).is_undef() {
                    break;
                }
                if et.get_or_new_hash("TAG_EXTRA").get_or_new_hash(key.to_bytes()).get("G3").truthy() {
                    copy += 1;
                    continue; // not Main if family 3 group is set
                }
                for member in ["PRIORITY", "VALUE", "FILE_ORDER", "TAG_INFO", "TAG_EXTRA"] {
                    let p_hash = et.get(member);
                    let t = p_hash.get(tag.to_bytes());
                    p_hash.set(tag.to_bytes(), p_hash.get(key.to_bytes()));
                    p_hash.set(key.to_bytes(), t);
                }
                break;
            }
        }
    }
    if verbose.truthy() {
        et.set("INDENT", old_indent);
    }
    // try to better identify the file type
    if et.get("FileType").eq_str("FPX") {
        let val = or(et.get("CompObjUserType"), et.get("Software"));
        if val.truthy() {
            let type_ = [("^3ds Max", "MAX"), ("Word", "DOC"), ("PowerPoint", "PPT"), ("Excel", "XLS")];
            let mut pats: Vec<&(&str, &str)> = type_.iter().collect();
            pats.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
            for (pat, t) in pats {
                if !pl::re_dyn(pat.as_bytes(), "")?.is_match(&val) {
                    continue;
                }
                pl::method(it, &et, "OverrideFileType", vec![s(t)])?;
                break;
            }
        }
    }
    // process Word document table
    process_document_table(it, vec![et.clone()], Want::Void)?;

    if et.get("IeImg_lkup").truthy() && !et.get("IeImg_lkup").is_empty() {
        warn(it, &et, s("Image positions exist without corresponding images"))?;
    }

    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::FlashPix::HDR_SIZE", hdr_size);
    it.define_native("Image::ExifTool::FlashPix::DIF_SECT", dif_sect);
    it.define_native("Image::ExifTool::FlashPix::FAT_SECT", fat_sect);
    it.define_native("Image::ExifTool::FlashPix::END_OF_CHAIN", end_of_chain);
    it.define_native("Image::ExifTool::FlashPix::FREE_SECT", free_sect);
    it.define_native("Image::ExifTool::FlashPix::VT_VECTOR", vt_vector);
    it.define_native("Image::ExifTool::FlashPix::VT_ARRAY", vt_array);
    it.define_native("Image::ExifTool::FlashPix::VT_BYREF", vt_byref);
    it.define_native("Image::ExifTool::FlashPix::VT_RESERVED", vt_reserved);
    it.define_native("Image::ExifTool::FlashPix::VT_VARIANT", vt_variant);
    it.define_native("Image::ExifTool::FlashPix::VT_LPSTR", vt_lpstr);
    it.define_native("Image::ExifTool::FlashPix::ConvertDTTM", convert_dttm);
    it.define_native("Image::ExifTool::FlashPix::ProcessHyperlinks", process_hyperlinks);
    it.define_native("Image::ExifTool::FlashPix::ReadFPXValue", read_fpx_value);
    it.define_native("Image::ExifTool::FlashPix::ProcessContents", process_contents);
    it.define_native("Image::ExifTool::FlashPix::ProcessWordDocument", process_word_document);
    it.define_native("Image::ExifTool::FlashPix::ProcessDocumentTable", process_document_table);
    it.define_native("Image::ExifTool::FlashPix::ProcessCommentBy", process_comment_by);
    it.define_native("Image::ExifTool::FlashPix::ProcessLastSavedBy", process_last_saved_by);
    it.define_native("Image::ExifTool::FlashPix::CheckBOM", check_bom);
    it.define_native("Image::ExifTool::FlashPix::ProcessProperties", process_properties);
    it.define_native("Image::ExifTool::FlashPix::LoadChain", load_chain);
    it.define_native("Image::ExifTool::FlashPix::ProcessFPXR", process_fpxr);
    it.define_native("Image::ExifTool::FlashPix::SetDocNum", set_doc_num);
    it.define_native("Image::ExifTool::FlashPix::ProcessFPX", process_fpx);
}
