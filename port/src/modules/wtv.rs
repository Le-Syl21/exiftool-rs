//! Image::ExifTool::WTV (Image/ExifTool/WTV.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read WTV meta information

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read, raf_read_cell, substr};
use crate::perl::Value;
use crate::pl::{self, arg, n, re, ret};

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

fn read_sectors_impl(it: &mut Interp, raf: &Value, sec_pt: &Value, pos: &Value, sec_size: &Value) -> ER<Value> {
    let mut pos = pos.clone();
    let (mut data, mut buff) = (Value::Undef, Value::Undef);
    while pl::le(&pos, &pl::sub(&pl::length(&sec_pt.deref()), &n(4))) {
        let sec = call(it, "Get32u", vec![sec_pt.clone(), pos.clone()])?;
        if pl::num_eq(&sec, &n(0xffff)) {
            return Ok(Value::Undef); // (just in case)
        }
        if !sec.truthy() {
            break; // a null marks the end of the sector table
        }
        if !data.is_undef() {
            data = pl::concat(&data, &buff);
        } else {
            data = buff.clone();
        }
        if !(pl::method(it, raf, "Seek", vec![pl::mul(&sec, sec_size), n(0)])?.truthy() && pl::num_eq(&raf_read(it, raf, &mut buff, sec_size.clone())?, sec_size)) {
            return Ok(Value::Undef);
        }
        pos = pl::add(&pos, &n(4));
    }
    Ok(if !data.is_undef() { pl::concat(&data, &buff) } else { buff })
}

/// Read specified sectors from the file
/// Inputs: 0) raf ref, 1) sector table ref, 2) offset in sector table, 3) sector size
/// Returns: Data or undef on error
pub fn read_sectors(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(read_sectors_impl(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2), &arg(&a, 3))?)
}

/// Process WTV metadata
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference, 2) tag table ref
/// Returns: 1 on success
pub fn process_metadata(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let data = data_pt.deref();
    let mut pos = n(0);
    let end = pl::length(&data);
    pl::method(it, &et, "VerboseDir", vec![pl::s("WTV Metadata"), Value::Undef, end.clone()])?;
    // (the sub has no return: it gives the value of its last statement, the
    // false loop condition, or an empty list after "last")
    while pl::lt(&pl::add(&pos, &n(0x18)), &end) {
        if &substr(it, &data, &pos, Some(&n(16)))?.to_bytes()[..] != b"\x5a\xfe\xd7\x6d\xc8\x1d\x8f\x4a\x99\x22\xfa\xb1\x1c\x38\x14\x53" {
            return pl::ret_nothing(want);
        }
        let mut fmt = call(it, "Get32u", vec![data_pt.clone(), pl::add(&pos, &n(0x10))])?;
        let len = call(it, "Get32u", vec![data_pt.clone(), pl::add(&pos, &n(0x14))])?;
        let mut str_ = pl::s("");
        pos = pl::add(&pos, &n(0x18));
        loop {
            if pl::gt(&pl::add(&pos, &n(2)), &end) {
                pl::method(it, &et, "Warn", vec![pl::s("Corrupt metadata directory")])?;
                break;
            }
            let ch = substr(it, &data, &pos, Some(&n(2)))?;
            pos = pl::add(&pos, &n(2));
            if ch.eq_str("\0\0") {
                break;
            }
            str_ = pl::concat(&str_, &ch);
        }
        if pl::gt(&pl::add(&pos, &len), &end) {
            return pl::ret_nothing(want);
        }
        let tag = pl::method(it, &et, "Decode", vec![str_, pl::s("UTF16"), Value::Undef, pl::s("UTF8")])?;
        let dat = substr(it, &data, &pos, Some(&len))?;
        // add tag if not already there
        if !tag_table_ptr.get(tag.to_bytes()).truthy() {
            let mut name = tag.clone();
            re(r"^(WTV_Metadata_)?WM/(WMRV)?", "").subst(&mut name, false, |_| Vec::new());
            call(it, "AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), name.clone()])?;
            let msg = pl::concat(&pl::concat(&pl::s("[adding WTV:"), &name), &pl::s("]\n"));
            pl::method(it, &et, "VPrint", vec![n(0), et.get("INDENT"), msg])?;
        }
        let val;
        let dat_ref = pl::sref(dat.clone());
        if pl::num_eq(&fmt, &n(0)) || pl::num_eq(&fmt, &n(3)) {
            // int32u or boolean32
            val = call(it, "Get32s", vec![dat_ref.clone(), n(0)])?;
        } else if pl::num_eq(&fmt, &n(1)) {
            // string
            val = pl::method(it, &et, "Decode", vec![dat.clone(), pl::s("UTF16")])?;
        } else if pl::num_eq(&fmt, &n(6)) {
            // GUID
            val = pl::unpack1("H*", &dat)?;
        } else if pl::num_eq(&fmt, &n(4)) {
            // int64u (date/time values use this)
            val = call(it, "Get64u", vec![dat_ref.clone(), n(0)])?;
        } else {
            val = dat.clone();
            fmt = pl::concat(&pl::concat(&pl::s("Unknown("), &fmt), &pl::s(")"));
        }
        pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tag, val, pl::s("Format"), pl::concat(&pl::s("format "), &fmt), pl::s("Size"), pl::length(&dat)])?;
        pl::method(it, &et, "VerboseDump", vec![dat_ref])?;
        pos = pl::add(&pos, &len);
    }
    ret(pl::s(""))
}

/// Extract information from a WTV video
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid WTV file
pub fn process_wtv(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    let buff_c = cell(Value::Undef);

    // verify this is a valid WTV file
    if !pl::num_eq(&raf_read_cell(it, &raf, &buff_c, n(0x60))?, &n(0x60)) {
        return ret(n(0));
    }
    if !re(r"^\xb7\xd8\x00\x20\x37\x49\xda\x11\xa6\x4e\x00\x07\xe9\x5e\xad\x8d", "").is_match(&buff_c.borrow().clone()) {
        return ret(n(0));
    }
    pl::method(it, &et, "SetFileType", vec![])?;
    call(it, "SetByteOrder", vec![pl::s("II")])?;
    // 0x28 - int32u: sector size? (=0x1000) (PH NC)
    // 0x38 - int32u: sector number for main WTV directory (PH assume this is a sector table, NC)
    // 0x58 - int32u: total number of sectors in file
    let mut sec_size = call(it, "Get32u", vec![Value::ScalarRef(buff_c.clone()), n(0x28)])?;
    // in case I'm wrong about this, constrain sector size to
    // either 0x1000 (standard) or 0x100 (ExifTool test file) - PH
    if !(pl::num_eq(&sec_size, &n(0x1000)) || pl::num_eq(&sec_size, &n(0x100))) {
        sec_size = n(0x1000);
    }
    let buff = read_sectors_impl(it, &raf, &Value::ScalarRef(buff_c.clone()), &n(0x38), &sec_size)?; // read the WTV directory
    if buff.is_undef() {
        return ret(n(0));
    }
    let buff_ref = pl::sref(buff.clone());
    let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::WTV::Main")])?;
    // parse the WTV directory
    pl::method(it, &et, "VerboseDir", vec![pl::s("WTV")])?;
    let mut pos = n(0);
    let mut len;
    while pl::lt(&pos, &pl::sub(&pl::length(&buff), &n(0x28))) {
        'next: {
            if &substr(it, &buff, &pos, Some(&n(0x10)))?.to_bytes()[..] != b"\x92\xb7\x74\x91\x59\x70\x70\x44\x88\xdf\x06\x3b\x82\xcc\x21\x3d" {
                if !pos.truthy() {
                    pl::method(it, &et, "Warn", vec![pl::s("WTV directory wasn't at expected location")])?;
                }
                return ret(n(1));
            }
            len = call(it, "Get32u", vec![buff_ref.clone(), pl::add(&pos, &n(0x10))])?;
            if pl::gt(&pl::add(&pos, &len), &pl::length(&buff)) {
                return ret(n(1));
            }
            let nn = call(it, "Get32u", vec![buff_ref.clone(), pl::add(&pos, &n(0x20))])?;
            if pl::gt(&pl::add(&pl::add(&n(0x28), &pl::mul(&nn, &n(2))), &n(8)), &len) {
                pl::method(it, &et, "Warn", vec![pl::s("WTV directory error")])?;
                return ret(n(1));
            }
            let s = substr(it, &buff, &pl::add(&pos, &n(0x28)), Some(&pl::mul(&nn, &n(2))))?;
            let tag = pl::method(it, &et, "Decode", vec![s, pl::s("UTF16"), Value::Undef, pl::s("UTF8")])?;
            let ptr = pl::add(&pl::add(&pos, &n(0x28)), &pl::mul(&nn, &n(2)));
            let flg = call(it, "Get32u", vec![buff_ref.clone(), pl::add(&ptr, &n(4))])?;
            if verbose.truthy() {
                let mut s = call(it, "Get32s", vec![buff_ref.clone(), ptr.clone()])?;
                if !pl::lt(&s, &n(0)) {
                    s = pl::sprintf("0x%x", &[s]);
                }
                let msg = pl::concat(&pl::concat(&pl::concat(&pl::concat(&pl::concat(&pl::concat(&pl::s("- Tag '"), &tag), &pl::s("' (sector=")), &s), &pl::s(", flag=")), &flg), &pl::s(")"));
                pl::method(it, &et, "VPrint", vec![n(1), msg])?;
            }
            if !(tag_table_ptr.get(tag.to_bytes()).truthy() && (pl::num_eq(&flg, &n(0)) || pl::num_eq(&flg, &n(1)))) {
                break 'next;
            }
            let sec = substr(it, &buff, &ptr, Some(&n(4)))?;
            let mut data = read_sectors_impl(it, &raf, &pl::sref(sec), &n(0), &sec_size)?;
            if data.is_undef() {
                return ret(n(1));
            }
            // read sectors from table if necessary (flag=1 indicates a sector table)
            if pl::num_eq(&flg, &n(1)) {
                data = read_sectors_impl(it, &raf, &pl::sref(data), &n(0), &sec_size)?;
            }
            if data.is_undef() {
                pl::method(it, &et, "Warn", vec![pl::concat(&pl::s("Error fetching data for "), &tag)])?;
                break 'next;
            }
            pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tag, data])?;
        }
        pos = pl::add(&pos, &len);
    }
    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::WTV::ReadSectors", read_sectors);
    it.define_native("Image::ExifTool::WTV::ProcessMetadata", process_metadata);
    it.define_native("Image::ExifTool::WTV::ProcessWTV", process_wtv);
}
