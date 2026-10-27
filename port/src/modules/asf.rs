//! Image::ExifTool::ASF (Image/ExifTool/ASF.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read ASF/WMA/WMV meta information

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell, substr};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// `$et->Decode(STR, 'UTF16', 'II')`
fn decode16(it: &mut Interp, et: &Value, s: Value) -> ER<Value> {
    pl::method(it, et, "Decode", vec![s, pl::s("UTF16"), pl::s("II")])
}

/// `unpack("x${pos}TEMPLATE", $data)`
fn unpack_at(pos: &Value, t: &str, data: &Value) -> ER<Vec<Value>> {
    pl::unpack(&format!("x{}{}", pos.to_text(), t), data)
}

fn get_guid_impl(val: &Value) -> ER<Value> {
    // must do some byte swapping
    if !pl::num_eq(&pl::length(val), &n(16)) {
        return Ok(val.clone());
    }
    let u = pl::unpack("VvvNN", val)?;
    let p = pl::pack("NnnNN", &u)?;
    let mut buff = pl::unpack1("H*", &p)?;
    re(r"(.{8})(.{4})(.{4})(.{4})", "").subst(&mut buff, false, |c| {
        let mut v = Vec::new();
        for i in 1..=4 {
            v.extend_from_slice(&cap(c, i).to_bytes());
            v.push(b'-');
        }
        v
    });
    Ok(pl::uc(&buff))
}

/// Generate GUID from 16 bytes of binary data
/// Inputs: 0) data
/// Returns: GUID
pub fn get_guid(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(get_guid_impl(&arg(&a, 0))?)
}

/// Process ASF content description
/// Inputs: 0) ExifTool object reference, 1) dirInfo ref, 2) tag table reference
/// Returns: 1 on success
pub fn process_content_description(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let dir_len = dir_info.get("DirLen");
    if pl::lt(&dir_len, &n(10)) {
        return ret(n(0));
    }
    let data = data_pt.deref();
    let mut lens = pl::unpack("v5", &data)?.into_iter();
    let mut pos = n(10);
    for tag in 0..=4 {
        let len = lens.next().unwrap_or_default();
        if !len.truthy() {
            continue;
        }
        if pl::gt(&pl::add(&pos, &len), &dir_len) {
            return ret(n(0));
        }
        let s = substr(it, &data, &pos, Some(&len))?;
        let val = decode16(it, &et, s)?;
        pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), n(tag), val])?;
        pos = pl::add(&pos, &len);
    }
    ret(n(1))
}

/// Process ASF content branding
/// Inputs: 0) ExifTool object reference, 1) dirInfo ref, 2) tag table reference
/// Returns: 1 on success
pub fn process_content_branding(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let dir_len = dir_info.get("DirLen");
    if pl::lt(&dir_len, &n(40)) {
        return ret(n(0));
    }
    let data = data_pt.deref();
    // decode banner image type
    let v = pl::unpack1("V", &data)?;
    pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), n(0), v])?;
    // decode banner image, banner URL and copyright URL
    let mut pos = n(4);
    for tag in 1..=3 {
        if pl::gt(&pl::add(&pos, &n(4)), &dir_len) {
            return ret(n(0));
        }
        let size = arg(&unpack_at(&pos, "V", &data)?, 0);
        pos = pl::add(&pos, &n(4));
        if !size.truthy() {
            continue;
        }
        if pl::gt(&pl::add(&pos, &size), &dir_len) {
            return ret(n(0));
        }
        let val = substr(it, &data, &pos, Some(&size))?;
        pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), n(tag), val])?;
        pos = pl::add(&pos, &size);
    }
    ret(n(1))
}

fn read_asf_impl(it: &mut Interp, et: &Value, data_pt: &Value, pos: &Value, format: &Value, size: &Value) -> ER<Value> {
    let mut vals: Vec<Value> = Vec::new();
    let read_value = |it: &mut Interp, fmt: &str| pl::call_list(it, "Image::ExifTool::ReadValue", vec![data_pt.clone(), pos.clone(), pl::s(fmt), Value::Undef, size.clone()]);
    if pl::num_eq(format, &n(0)) {
        // unicode string
        let s = substr(it, &data_pt.deref(), pos, Some(size))?;
        vals.push(decode16(it, et, s)?);
    } else if pl::num_eq(format, &n(2)) {
        // 4-byte boolean
        vals = read_value(it, "int32u")?;
        for v in vals.iter_mut() {
            *v = pl::s(if v.truthy() { "True" } else { "False" });
        }
    } else if pl::num_eq(format, &n(3)) {
        // int32u
        vals = read_value(it, "int32u")?;
    } else if pl::num_eq(format, &n(4)) {
        // int64u
        vals = read_value(it, "int64u")?;
    } else if pl::num_eq(format, &n(5)) {
        // int16u
        vals = read_value(it, "int16u")?;
    } else {
        // any other format (including 1, byte array): return raw data
        vals.push(substr(it, &data_pt.deref(), pos, Some(size))?);
    }
    Ok(pl::join(" ", &vals))
}

/// Read ASF value
/// Inputs: 0) ExifTool object ref, 1) data reference, 2) value offset,
///         3) format number, 4) size
/// Returns: converted value
pub fn read_asf(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(read_asf_impl(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2), &arg(&a, 3), &arg(&a, 4))?)
}

/// `Image::ExifTool::Decode(undef, STR, 'UTF16', 'II', 'Latin')`, then the leading "WM/" removed
fn wm_tag(it: &mut Interp, s: Value) -> ER<Value> {
    let mut tag = pl::call(it, "Image::ExifTool::Decode", vec![Value::Undef, s, pl::s("UTF16"), pl::s("II"), pl::s("Latin")])?;
    re(r"^WM\/", "").subst(&mut tag, false, |_| Vec::new()); // remove leading "WM/"
    Ok(tag)
}

/// Process extended content description
/// Inputs: 0) ExifTool object reference, 1) dirInfo ref, 2) tag table reference
/// Returns: 1 on success
pub fn process_extended_content_description(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let dir_len = dir_info.get("DirLen");
    if pl::lt(&dir_len, &n(2)) {
        return ret(n(0));
    }
    let count = call(it, "Get16u", vec![data_pt.clone(), n(0)])?;
    pl::method(it, &et, "VerboseDir", vec![dir_info.clone(), count.clone()])?;
    let mut pos = n(2);
    let mut i = n(0);
    while pl::lt(&i, &count) {
        if pl::gt(&pl::add(&pos, &n(6)), &dir_len) {
            return ret(n(0));
        }
        let data = data_pt.deref();
        let name_len = arg(&unpack_at(&pos, "v", &data)?, 0);
        pos = pl::add(&pos, &n(2));
        if pl::gt(&pl::add(&pl::add(&pos, &name_len), &n(4)), &dir_len) {
            return ret(n(0));
        }
        let s = substr(it, &data, &pos, Some(&name_len))?;
        let tag = wm_tag(it, s)?;
        pos = pl::add(&pos, &name_len);
        let u = unpack_at(&pos, "v2", &data)?;
        let (d_type, d_len) = (arg(&u, 0), arg(&u, 1));
        pos = pl::add(&pos, &n(4));
        if pl::gt(&pl::add(&pos, &d_len), &dir_len) {
            return ret(n(0));
        }
        let val = read_asf_impl(it, &et, &data_pt, &pos, &d_type, &d_len)?;
        pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tag, val, pl::s("DataPt"), data_pt.clone(), pl::s("Start"), pos.clone(), pl::s("Size"), d_len.clone()])?;
        pos = pl::add(&pos, &d_len);
        i = pl::add(&i, &n(1));
    }
    ret(n(1))
}

/// Process ASF metadata library (similar to ProcessExtendedContentDescription above)
/// Inputs: 0) ExifTool object reference, 1) dirInfo ref, 2) tag table reference
/// Returns: 1 on success
pub fn process_metadata(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let dir_len = dir_info.get("DirLen");
    if pl::lt(&dir_len, &n(2)) {
        return ret(n(0));
    }
    let count = call(it, "Get16u", vec![data_pt.clone(), n(0)])?;
    pl::method(it, &et, "VerboseDir", vec![dir_info.clone(), count.clone()])?;
    let mut pos = n(2);
    let mut i = n(0);
    while pl::lt(&i, &count) {
        if pl::gt(&pl::add(&pos, &n(12)), &dir_len) {
            return ret(n(0));
        }
        let data = data_pt.deref();
        let u = unpack_at(&pos, "v4V", &data)?;
        let (_index, _stream, name_len, d_type, d_len) = (arg(&u, 0), arg(&u, 1), arg(&u, 2), arg(&u, 3), arg(&u, 4));
        pos = pl::add(&pos, &n(12));
        if pl::gt(&pl::add(&pl::add(&pos, &name_len), &d_len), &dir_len) {
            return ret(n(0));
        }
        let s = substr(it, &data, &pos, Some(&name_len))?;
        let tag = wm_tag(it, s)?;
        pos = pl::add(&pos, &name_len);
        let val = read_asf_impl(it, &et, &data_pt, &pos, &d_type, &d_len)?;
        pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tag, val, pl::s("DataPt"), data_pt.clone(), pl::s("Start"), pos.clone(), pl::s("Size"), d_len.clone()])?;
        pos = pl::add(&pos, &d_len);
        i = pl::add(&i, &n(1));
    }
    ret(n(1))
}

/// Process WM/Picture preview
/// Inputs: 0) ExifTool object reference, 1) dirInfo ref, 2) tag table reference
/// Returns: 1 on success
pub fn process_picture(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let dir_start = dir_info.get("DirStart");
    let dir_len = dir_info.get("DirLen");
    if !pl::gt(&dir_len, &n(9)) {
        return ret(n(0));
    }
    let data = data_pt.deref();
    // extract picture type and length
    let u = unpack_at(&dir_start, "CV", &data)?;
    let (type_, pic_len) = (arg(&u, 0), arg(&u, 1));
    pl::method(it, &et, "VerboseDir", vec![pl::s("Picture")])?;
    pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), n(0), type_])?;
    // extract mime type and description strings (null-terminated unicode strings)
    let nn = pl::sub(&pl::sub(&dir_len, &n(5)), &pic_len);
    if nn.to_i64() & 0x01 != 0 || pl::lt(&nn, &n(4)) {
        return ret(n(0));
    }
    let str_ = substr(it, &data, &pl::add(&dir_start, &n(5)), Some(&nn))?;
    if let Some(m) = re(r"^((?:..)*?)\0\0((?:..)*?)\0\0", "s").m(&str_) {
        let (mime, desc) = (cap(&m, 1), cap(&m, 2));
        let v = decode16(it, &et, mime)?;
        pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), n(1), v])?;
        if pl::length(&desc).truthy() {
            let v = decode16(it, &et, desc)?;
            pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), n(2), v])?;
        }
    }
    let pic = substr(it, &data, &pl::add(&pl::add(&dir_start, &n(5)), &nn), Some(&pic_len))?;
    pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), n(3), pic])?;
    ret(n(1))
}

/// Process codec list
/// Inputs: 0) ExifTool object reference, 1) dirInfo ref, 2) tag table reference
/// Returns: 1 on success
pub fn process_codec_list(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let dir_len = dir_info.get("DirLen");
    if pl::lt(&dir_len, &n(20)) {
        return ret(n(0));
    }
    let count = call(it, "Get32u", vec![data_pt.clone(), n(16)])?;
    pl::method(it, &et, "VerboseDir", vec![dir_info.clone(), count.clone()])?;
    let mut pos = n(20);
    let mut i = n(0);
    while pl::lt(&i, &count) {
        if pl::gt(&pl::add(&pos, &n(8)), &dir_len) {
            return ret(n(0));
        }
        let ct = call(it, "Get16u", vec![data_pt.clone(), pos.clone()])?;
        let ct = if pl::str_eq(&ct, "1") {
            "Video"
        } else if pl::str_eq(&ct, "2") {
            "Audio"
        } else {
            "Other"
        };
        let type_ = format!("{ct}Codec");
        // stupid Windows programmers: these lengths are in characters (others are in bytes)
        let name_len = pl::mul(&call(it, "Get16u", vec![data_pt.clone(), pl::add(&pos, &n(2))])?, &n(2));
        pos = pl::add(&pos, &n(4));
        if pl::gt(&pl::add(&pl::add(&pos, &name_len), &n(2)), &dir_len) {
            return ret(n(0));
        }
        let s = substr(it, &data_pt.deref(), &pos, Some(&name_len))?;
        let name = decode16(it, &et, s)?;
        pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), Value::from(format!("{type_}Name")), name])?;
        let desc_len = pl::mul(&call(it, "Get16u", vec![data_pt.clone(), pl::add(&pos, &name_len)])?, &n(2));
        pos = pl::add(&pos, &pl::add(&name_len, &n(2)));
        if pl::gt(&pl::add(&pl::add(&pos, &desc_len), &n(2)), &dir_len) {
            return ret(n(0));
        }
        let s = substr(it, &data_pt.deref(), &pos, Some(&desc_len))?;
        let desc = decode16(it, &et, s)?;
        pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), Value::from(format!("{type_}Description")), desc])?;
        let info_len = call(it, "Get16u", vec![data_pt.clone(), pl::add(&pos, &desc_len)])?;
        pos = pl::add(&pos, &pl::add(&pl::add(&desc_len, &n(2)), &info_len));
        i = pl::add(&i, &n(1));
    }
    ret(n(1))
}

/// Extract information from a ASF file
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference, 2) tag table ref
/// Returns: 1 on success, 0 if this wasn't a valid ASF file
pub fn process_asf(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, mut tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let raf = dir_info.get("RAF");
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    let mut rtn_val = n(0);
    let mut pos = n(0);
    let buff = cell(Value::Undef);
    let buff_ref = Value::ScalarRef(buff.clone());
    let mut err = Value::Undef;
    let (mut parent_table, mut child_end): (Vec<Value>, Vec<Value>) = (Vec::new(), Vec::new());
    let process_asf_ref = Value::Code(it.sub_ref("Image::ExifTool::ASF::ProcessASF"));

    loop {
        if !pl::num_eq(&raf_read_cell(it, &raf, &buff, n(24))?, &n(24)) {
            break;
        }
        pos = pl::add(&pos, &n(24));
        let b = buff.borrow().clone();
        let tag = get_guid_impl(&substr(it, &b, &n(0), Some(&n(16)))?)?;
        if !tag_table_ptr.truthy() {
            // verify this is a valid ASF file
            if !tag.eq_str("75B22630-668E-11CF-A6D9-00AA0062CE6C") {
                break;
            }
            let mut file_type = et.get("FILE_EXT");
            if !(file_type.truthy() && re(r"^(ASF|WMV|WMA|DIVX)$", "").is_match(&file_type)) {
                file_type = pl::s("ASF");
            }
            pl::method(it, &et, "SetFileType", vec![file_type])?;
            call(it, "SetByteOrder", vec![pl::s("II")])?;
            tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::ASF::Main")])?;
            rtn_val = n(1);
        }
        let size = pl::sub(&pl::call(it, "Image::ExifTool::Get64u", vec![buff_ref.clone(), n(16)])?, &n(24));
        if pl::lt(&size, &n(0)) {
            err = pl::s("Invalid ASF object size");
            break;
        }
        if pl::gt(&size, &n(0x7fffffff)) {
            if pl::gt(&size, &pl::mul(&n(0x7fffffff), &n(4294967296))) {
                err = pl::s("Invalid ASF object size");
            } else if pl::method(it, &et, "Options", vec![pl::s("LargeFileSupport")])?.truthy() {
                if pl::method(it, &et, "Options", vec![pl::s("LargeFileSupport")])?.eq_str("2") {
                    pl::method(it, &et, "Warn", vec![pl::s("Skipping large ASF object (LargeFileSupport is 2)")])?;
                }
                if pl::method(it, &raf, "Seek", vec![size.clone(), n(1)])?.truthy() {
                    let msg = pl::concat(&pl::concat(&pl::s("  Skipped large ASF object ("), &size), &pl::s(" bytes)\n"));
                    pl::method(it, &et, "VPrint", vec![n(0), msg])?;
                    pos = pl::add(&pos, &size);
                    continue;
                }
                err = pl::s("Error seeking past large ASF object");
            } else {
                err = pl::s("Large ASF objects not supported (LargeFileSupport not set)");
            }
            break;
        }
        // go back to parent tag table if done with previous children
        if !child_end.is_empty() && pl::ge(&pos, child_end.last().unwrap()) {
            child_end.pop();
            tag_table_ptr = parent_table.pop().unwrap_or_default();
            let indent = substr(it, &et.get("INDENT"), &n(0), Some(&n(-2)))?;
            et.set("INDENT", indent);
        }
        let tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
        if verbose.truthy() {
            pl::method(it, &et, "VerboseInfo", vec![tag.clone(), tag_info.clone()])?;
        }
        if tag_info.truthy() {
            let subdir = tag_info.get("SubDirectory");
            if subdir.truthy() {
                let sub_table = call(it, "GetTagTable", vec![subdir.get("TagTable")])?;
                if sub_table.get("PROCESS_PROC").to_bytes() == process_asf_ref.to_bytes() {
                    if !subdir.get("Size").is_undef() {
                        let s = subdir.get("Size");
                        if pl::gt(&verbose, &n(2)) {
                            if !pl::num_eq(&raf_read_cell(it, &raf, &buff, s.clone())?, &s) {
                                err = pl::s("Truncated file");
                                break;
                            }
                            pl::method(it, &et, "VerboseDump", vec![buff_ref.clone()])?;
                        } else if !pl::method(it, &raf, "Seek", vec![s.clone(), n(1)])?.truthy() {
                            err = pl::s("Seek error");
                            break;
                        }
                        // continue processing linearly using subTable
                        parent_table.push(tag_table_ptr.clone());
                        child_end.push(pl::add(&pos, &size));
                        tag_table_ptr = sub_table;
                        pos = pl::add(&pos, &subdir.get("Size"));
                        if verbose.truthy() {
                            et.set("INDENT", pl::concat(&et.get("INDENT"), &pl::s("| ")));
                            pl::method(it, &et, "VerboseDir", vec![tag_info.get("Name")])?;
                        }
                        continue;
                    }
                } else if pl::num_eq(&raf_read_cell(it, &raf, &buff, size.clone())?, &size) {
                    let subdir_info = Value::new_hash();
                    subdir_info.set("DataPt", buff_ref.clone());
                    subdir_info.set("DirStart", n(0));
                    subdir_info.set("DirLen", size.clone());
                    subdir_info.set("DirName", tag_info.get("Name"));
                    if pl::gt(&verbose, &n(2)) {
                        pl::method(it, &et, "VerboseDump", vec![buff_ref.clone()])?;
                    }
                    if !pl::method(it, &et, "ProcessDirectory", vec![subdir_info, sub_table, subdir.get("ProcessProc")])?.truthy() {
                        let msg = pl::concat(&pl::concat(&pl::s("Error processing "), &tag_info.get("Name")), &pl::s(" directory"));
                        pl::method(it, &et, "Warn", vec![msg])?;
                    }
                    pos = pl::add(&pos, &size);
                    continue;
                } else {
                    err = pl::s("Unexpected end of file");
                    break;
                }
            }
        }
        if pl::gt(&verbose, &n(2)) {
            if !pl::num_eq(&raf_read_cell(it, &raf, &buff, size.clone())?, &size) {
                err = pl::s("Truncated file");
                break;
            }
            pl::method(it, &et, "VerboseDump", vec![buff_ref.clone()])?;
        } else if !pl::method(it, &raf, "Seek", vec![size.clone(), n(1)])?.truthy() {
            // skip the block
            err = pl::s("Seek error");
            break;
        }
        pos = pl::add(&pos, &size);
    }
    if err.truthy() {
        pl::method(it, &et, "Warn", vec![err])?;
    }
    ret(rtn_val)
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::ASF::GetGUID", get_guid);
    it.define_native("Image::ExifTool::ASF::ProcessContentDescription", process_content_description);
    it.define_native("Image::ExifTool::ASF::ProcessContentBranding", process_content_branding);
    it.define_native("Image::ExifTool::ASF::ReadASF", read_asf);
    it.define_native("Image::ExifTool::ASF::ProcessExtendedContentDescription", process_extended_content_description);
    it.define_native("Image::ExifTool::ASF::ProcessMetadata", process_metadata);
    it.define_native("Image::ExifTool::ASF::ProcessPicture", process_picture);
    it.define_native("Image::ExifTool::ASF::ProcessCodecList", process_codec_list);
    it.define_native("Image::ExifTool::ASF::ProcessASF", process_asf);
}
