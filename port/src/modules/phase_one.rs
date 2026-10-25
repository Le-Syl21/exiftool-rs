//! Image::ExifTool::PhaseOne (Image/ExifTool/PhaseOne.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Phase One maker notes tags

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, substr};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, n, re, ret};

const MODULE: &str = "Image::ExifTool::PhaseOne";

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

fn hash(pairs: Vec<(&str, Value)>) -> Value {
    let h = Value::new_hash();
    for (k, v) in pairs {
        h.set(k, v);
    }
    h
}

fn or(a: Value, b: impl FnOnce() -> Value) -> Value {
    if a.truthy() {
        a
    } else {
        b()
    }
}

fn val(c: &Cell) -> Value {
    c.borrow().clone()
}

fn rf(c: &Cell) -> Value {
    Value::ScalarRef(c.clone())
}

fn append(c: &Cell, v: &Value) {
    let x = pl::concat(&val(c), v);
    *c.borrow_mut() = x;
}

fn cat(parts: &[Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

//------------------------------------------------------------------------------
/// Do HTML dump of an IFD entry
/// Inputs: 0) ExifTool ref, 1) tag table ref, 3) tag ID, 4) tag value,
///         5) IFD entry offset, 6) IFD entry size, 7) parameter hash
pub fn html_dump(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, tag_table_ptr, tag_id, value, entry, entry_len) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4), arg(&a, 5));
    let parms = Value::new_hash();
    let mut i = 6;
    while i < a.len() {
        parms.set(a[i].to_bytes(), arg(&a, i + 1));
        i += 2;
    }
    let (dir_name, index, mut format_str, data_pos, mut base, size, value_ptr) =
        (parms.get("DirName"), parms.get("Index"), parms.get("Format"), parms.get("DataPos"), parms.get("Base"), parms.get("Size"), parms.get("Start"));
    let tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag_id.clone()])?;
    let (tag_name, col_name, mut subdir);
    subdir = Value::Undef;
    let mut count = or(parms.get("Count"), || size.clone());
    if base.is_undef() {
        base = n(0);
    }
    if tag_info.truthy() {
        tag_name = tag_info.get("Name");
        subdir = tag_info.get("SubDirectory");
        if tag_info.get("Format").truthy() {
            format_str = tag_info.get("Format");
            let fs = pl::call(it, "Image::ExifTool::FormatSize", vec![format_str.clone()])?;
            count = pl::div(&size, &fs)?;
        }
    } else {
        tag_name = pl::sprintf("Tag 0x%.4x", &[tag_id.clone()]);
    }
    let dname = pl::sprintf_v(&pl::concat(&dir_name, &pl::s("-%.2d")), &[index]);
    // build our tool tip
    let fstr = cat(&[format_str.clone(), pl::s("["), count.clone(), pl::s("]")]);
    let mut tip = pl::concat(&pl::sprintf("Tag ID: 0x%.4x\n", &[tag_id.clone()]), &cat(&[pl::s("Format: "), fstr, pl::s("\nSize: "), size.clone(), pl::s(" bytes\n")]));
    if pl::gt(&size, &n(4)) {
        tip = pl::concat(&tip, &pl::sprintf("Value offset: 0x%.4x\n", &[pl::sub(&value_ptr, &base)]));
        tip = pl::concat(&tip, &pl::sprintf("Actual offset: 0x%.4x\n", &[pl::add(&value_ptr, &data_pos)]));
        tip = pl::concat(&tip, &pl::sprintf("Offset base: 0x%.4x\n", &[pl::add(&data_pos, &base)]));
        col_name = cat(&[pl::s("<span class=F>"), tag_name.clone(), pl::s("</span>")]);
    } else {
        col_name = tag_name.clone();
    }
    if !value.is_ref() {
        let mut tval = if pl::gt(&pl::length(&value), &n(32)) { pl::concat(&substr(it, &value, &n(0), Some(&n(28)))?, &pl::s("[...]")) } else { value.clone() };
        pl::tr(&mut tval, r"\x00-\x1f\x7f-\xff", ".", "");
        tip = pl::concat(&tip, &pl::concat(&pl::s("Value: "), &tval));
    }
    pl::method(it, &et, "HDump", vec![pl::add(&entry, &data_pos), entry_len, cat(&[dname, pl::s(" "), col_name]), tip, n(1)])?;
    if pl::gt(&size, &n(4)) {
        let dump_pos = pl::add(&value_ptr, &data_pos);
        // add value data block
        let r = pl::method(it, &et, "HDump", vec![dump_pos, size, pl::concat(&tag_name, &pl::s(" value")), pl::s("SAME"), n(if subdir.truthy() { 0x04 } else { 0 })])?;
        return ret(r);
    }
    // (the sub's value is that of its last statement: the false condition)
    ret(pl::s(""))
}

//------------------------------------------------------------------------------
/// Write PhaseOne maker notes (both types of PhaseOne IFD)
/// Inputs: 0) ExifTool object ref, 1) source dirInfo ref, 2) tag table ref
/// Returns: data block or undef on error
pub fn write_phase_one(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if !et.truthy() {
        return ret(n(1)); // allow dummy access to autoload this package
    }

    // nothing to do if we aren't changing any PhaseOne tags
    let new_tags = pl::method(it, &et, "GetNewTagInfoHash", vec![tag_table_ptr.clone()])?;
    if !(!new_tags.is_empty() || et.get("DropTags").truthy() || et.get("EDIT_DIRS").get("PhaseOne").truthy()) {
        return pl::ret_undef();
    }

    let data_pt = dir_info.get("DataPt");
    let _data_pos = or(dir_info.get("DataPos"), || n(0));
    let dir_start = or(dir_info.get("DirStart"), || n(0));
    let dir_len = or(dir_info.get("DirLen"), || pl::sub(&dir_info.get("DataLen"), &dir_start));
    let dir_name = dir_info.get("DirName");
    let _verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;

    if pl::lt(&dir_len, &n(12)) {
        return pl::ret_undef();
    }
    if !(tag_table_ptr.get("VARS").truthy() && tag_table_ptr.get("VARS").get("ENTRY_SIZE").truthy()) {
        let msg = pl::concat(&pl::s("No ENTRY_SIZE for "), &tag_table_ptr.get("TABLE_NAME"));
        pl::method(it, &et, "Warn", vec![msg])?;
        return pl::ret_undef();
    }
    let entry_size = tag_table_ptr.get("VARS").get("ENTRY_SIZE");
    let ifd_type = or(tag_table_ptr.get("TAG_PREFIX"), || pl::s("PhaseOne"));
    let hdr = substr(it, &data_pt.deref(), &dir_start, Some(&n(12)))?;
    if pl::num_eq(&entry_size, &n(16)) {
        if !re(r"^(IIII.waR|MMMMRaw.)", "s").is_match(&hdr) {
            return pl::ret_undef();
        }
    } else if !re(r"^(IIII\x01\0\0\0|MMMM\0\0\0\x01)", "s").is_match(&hdr) {
        let msg = cat(&[pl::s("Unrecognized "), ifd_type.clone(), pl::s(" directory version")]);
        pl::method(it, &et, "Warn", vec![msg])?;
        return pl::ret_undef();
    }
    let order = substr(it, &hdr, &n(0), Some(&n(2)))?;
    call(it, "SetByteOrder", vec![order])?;
    // get offset to start of PhaseOne directory
    let ifd_start = call(it, "Get32u", vec![pl::sref(hdr.clone()), n(8)])?;
    if pl::gt(&pl::add(&ifd_start, &n(8)), &dir_len) {
        return pl::ret_undef();
    }
    // initialize output directory buffer with (fixed) number of entries plus 4-byte padding
    let dir_buff = cell(substr(it, &data_pt.deref(), &pl::add(&dir_start, &ifd_start), Some(&n(8)))?);
    // get number of entries in PhaseOne directory
    let num_entries = call(it, "Get32u", vec![rf(&dir_buff), n(0)])?;
    let ifd_end = pl::add(&pl::add(&ifd_start, &n(8)), &pl::mul(&entry_size, &num_entries));
    if pl::lt(&num_entries, &n(2)) || pl::gt(&num_entries, &n(300)) || pl::gt(&ifd_end, &dir_len) {
        return pl::ret_undef();
    }
    let hdr_buff = cell(hdr.clone());
    let val_buff = cell(pl::s("")); // buffer for value data
    let fixup = pl::method(it, &pl::s("Image::ExifTool::Fixup"), "new", vec![])?;
    let format_name = crate::runtime::lex_array(it, MODULE, "formatName");
    let mut index = n(0);
    while pl::lt(&index, &num_entries) {
        'entry: {
            let entry = pl::add(&pl::add(&pl::add(&dir_start, &ifd_start), &n(8)), &pl::mul(&entry_size, &index));
            let tag_id = call(it, "Get32u", vec![data_pt.clone(), entry.clone()])?;
            let mut size = call(it, "Get32u", vec![data_pt.clone(), pl::sub(&pl::add(&entry, &entry_size), &n(8))])?;
            let (_format_size, mut format_str);
            if pl::num_eq(&entry_size, &n(16)) {
                _format_size = call(it, "Get32u", vec![data_pt.clone(), pl::add(&entry, &n(4))])?;
                format_str = format_name.at(_format_size.to_i64());
                if !format_str.truthy() {
                    let msg = cat(&[pl::s("Possibly invalid "), ifd_type.clone(), pl::s(" IFD entry "), index.clone()]);
                    pl::method(it, &et, "Warn", vec![msg, n(1)])?;
                    new_tags.delete(tag_id.to_bytes()); // make sure we don't try to change this one
                }
            } else {
                // (no format code for SensorCalibration IFD entries)
                _format_size = n(1);
                format_str = pl::s("undef");
            }
            let mut value_ptr = pl::sub(&pl::add(&entry, &entry_size), &n(4));
            if pl::gt(&size, &n(4)) {
                if pl::gt(&size, &n(0x7fffffff)) {
                    let msg = cat(&[pl::s("Invalid size for "), ifd_type.clone(), pl::s(" IFD entry "), index.clone()]);
                    pl::method(it, &et, "Error", vec![msg, n(1)])?;
                    return pl::ret_undef();
                }
                value_ptr = call(it, "Get32u", vec![data_pt.clone(), value_ptr])?;
                if pl::gt(&pl::add(&value_ptr, &size), &dir_len) {
                    let fmt = cat(&[pl::s("Invalid offset 0x%.4x for "), ifd_type.clone(), pl::s(" IFD entry "), index.clone()]);
                    let msg = pl::sprintf_v(&fmt, &[value_ptr.clone()]);
                    pl::method(it, &et, "Error", vec![msg, n(1)])?;
                    return pl::ret_undef();
                }
                value_ptr = pl::add(&value_ptr, &dir_start);
            }
            let mut value = substr(it, &data_pt.deref(), &value_ptr, Some(&size))?;
            let mut tag_info = or(new_tags.get(tag_id.to_bytes()), || tag_table_ptr.get(tag_id.to_bytes()));
            if tag_info.truthy() && pl::ref_of(&tag_info) != "HASH" {
                tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag_id.clone()])?;
            }
            if new_tags.get(tag_id.to_bytes()).truthy() {
                if tag_info.get("Format").truthy() {
                    format_str = tag_info.get("Format");
                }
                let fs = pl::call(it, "Image::ExifTool::FormatSize", vec![format_str.clone()])?;
                let mut count = pl::int(&pl::div(&size, &fs)?);
                let v = call(it, "ReadValue", vec![pl::sref(value.clone()), n(0), format_str.clone(), count.clone(), size.clone()])?;
                let nv_hash = pl::method(it, &et, "GetNewValueHash", vec![tag_info.clone()])?;
                if pl::method(it, &et, "IsOverwriting", vec![nv_hash.clone(), v.clone()])?.truthy() {
                    let new_val = pl::method(it, &et, "GetNewValue", vec![nv_hash])?;
                    // allow count to change for string and undef types only
                    if pl::str_eq(&format_str, "string") || pl::str_eq(&format_str, "undef") {
                        count = Value::Undef;
                    }
                    let new_value = call(it, "WriteValue", vec![new_val.clone(), format_str.clone(), count])?;
                    if !new_value.is_undef() {
                        value = new_value.clone();
                        size = pl::length(&new_value);
                        let tn = tag_info.get("Name");
                        pl::method(it, &et, "VerboseValue", vec![cat(&[pl::s("- "), dir_name.clone(), pl::s(":"), tn.clone()]), v])?;
                        pl::method(it, &et, "VerboseValue", vec![cat(&[pl::s("+ "), dir_name.clone(), pl::s(":"), tn]), new_val])?;
                        et.set("CHANGED", pl::add(&et.get("CHANGED"), &n(1)));
                    }
                }
            } else if tag_info.truthy() && tag_info.get("SubDirectory").truthy() {
                let sub_table = call(it, "GetTagTable", vec![tag_info.get("SubDirectory").get("TagTable")])?;
                let subdir_info = hash(vec![("DirName", tag_info.get("Name")), ("DataPt", pl::sref(value.clone())), ("DataLen", pl::length(&value))]);
                let new_value = pl::method(it, &et, "WriteDirectory", vec![subdir_info, sub_table])?;
                if !new_value.is_undef() && pl::length(&new_value).truthy() {
                    value = new_value.clone();
                    size = pl::length(&new_value);
                }
            } else if et.get("DropTags").truthy() && ((tag_info.truthy() && tag_info.get("Drop").truthy()) || pl::gt(&size, &n(8192))) {
                // decrease the number of entries in the directory
                let cnt = call(it, "Get32u", vec![rf(&dir_buff), n(0)])?;
                call(it, "Set32u", vec![pl::sub(&cnt, &n(1)), rf(&dir_buff), n(0)])?;
                break 'entry; // drop this tag
            }
            // add the tagID, possibly format size, and size to this directory entry
            let e = substr(it, &data_pt.deref(), &entry, Some(&pl::sub(&entry_size, &n(8))))?;
            let s32 = call(it, "Set32u", vec![size.clone()])?;
            append(&dir_buff, &pl::concat(&e, &s32));

            // pad value to an even 4-byte boundary just in case
            let low = ops::bit_and(&size, &n(0x03));
            if low.truthy() || !size.truthy() {
                let pad = pl::sub(&n(4), &low).to_i64().max(0) as usize;
                value = pl::concat(&value, &Value::bytes(vec![0u8; pad]));
            }
            if pl::le(&size, &n(4)) {
                // store value in place of the IFD value pointer (already padded to 4 bytes)
                append(&dir_buff, &value);
            } else if tag_info.truthy() && tag_info.get("PutFirst").truthy() {
                // store value immediately after header
                let p = call(it, "Set32u", vec![pl::length(&val(&hdr_buff))])?;
                append(&dir_buff, &p);
                append(&hdr_buff, &value);
            } else {
                // store value at end of value buffer
                pl::method(it, &fixup, "AddFixup", vec![pl::length(&val(&dir_buff))])?;
                let p = call(it, "Set32u", vec![pl::length(&val(&val_buff))])?;
                append(&dir_buff, &p);
                append(&val_buff, &value);
            }
        }
        index = pl::add(&index, &n(1));
    }
    // apply necessary fixup to offsets in PhaseOne directory
    fixup.set("Shift", pl::length(&val(&hdr_buff)));
    pl::method(it, &fixup, "ApplyFixup", vec![rf(&dir_buff)])?;
    // set pointer to PhaseOneIFD in header
    let p = pl::add(&pl::length(&val(&hdr_buff)), &pl::length(&val(&val_buff)));
    call(it, "Set32u", vec![p, rf(&hdr_buff), n(8)])?;
    ret(cat(&[val(&hdr_buff), val(&val_buff), val(&dir_buff)]))
}

//------------------------------------------------------------------------------
/// Read Phase One maker notes
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
/// Notes: This routine processes both the main PhaseOne IFD type (with 16 bytes
///        per entry), and the SensorCalibration IFD type (12 bytes per entry)
pub fn process_phase_one(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let data_pos = pl::add(&or(dir_info.get("DataPos"), || n(0)), &or(dir_info.get("Base"), || n(0)));
    let dir_start = or(dir_info.get("DirStart"), || n(0));
    let dir_len = or(dir_info.get("DirLen"), || pl::sub(&dir_info.get("DataLen"), &dir_start));
    let binary = pl::method(it, &et, "Options", vec![pl::s("Binary")])?;
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    let hash_obj = et.get("ImageDataHash");
    let html_dump_on = et.get("HTML_DUMP");

    if pl::lt(&dir_len, &n(12)) {
        return ret(n(0));
    }
    if !(tag_table_ptr.get("VARS").truthy() && tag_table_ptr.get("VARS").get("ENTRY_SIZE").truthy()) {
        let msg = pl::concat(&pl::s("No ENTRY_SIZE for "), &tag_table_ptr.get("TABLE_NAME"));
        pl::method(it, &et, "Warn", vec![msg])?;
        return pl::ret_undef();
    }
    let entry_size = tag_table_ptr.get("VARS").get("ENTRY_SIZE");
    let ifd_type = or(tag_table_ptr.get("TAG_PREFIX"), || pl::s("PhaseOne"));

    let hdr = substr(it, &data_pt.deref(), &dir_start, Some(&n(12)))?;
    if pl::num_eq(&entry_size, &n(16)) {
        if !re(r"^(IIII.waR|MMMMRaw.)", "s").is_match(&hdr) {
            return ret(n(0));
        }
    } else if !re(r"^(IIII\x01\0\0\0|MMMM\0\0\0\x01)", "s").is_match(&hdr) {
        let msg = cat(&[pl::s("Unrecognized "), ifd_type.clone(), pl::s(" directory version")]);
        pl::method(it, &et, "Warn", vec![msg])?;
        return ret(n(0));
    }
    let order = substr(it, &hdr, &n(0), Some(&n(2)))?;
    call(it, "SetByteOrder", vec![order])?;
    // get offset to start of PhaseOne directory
    let ifd_start = call(it, "Get32u", vec![pl::sref(hdr.clone()), n(8)])?;
    if pl::gt(&pl::add(&ifd_start, &n(8)), &dir_len) {
        return ret(n(0));
    }
    // get number of entries in PhaseOne directory
    let num_entries = call(it, "Get32u", vec![data_pt.clone(), pl::add(&dir_start, &ifd_start)])?;
    let ifd_end = pl::add(&pl::add(&ifd_start, &n(8)), &pl::mul(&entry_size, &num_entries));
    if pl::lt(&num_entries, &n(2)) || pl::gt(&num_entries, &n(300)) || pl::gt(&ifd_end, &dir_len) {
        return ret(n(0));
    }
    pl::method(it, &et, "VerboseDir", vec![ifd_type.clone(), num_entries.clone()])?;
    if html_dump_on.truthy() {
        let p = pl::add(&dir_start, &data_pos);
        pl::method(it, &et, "HDump", vec![p.clone(), n(8), pl::concat(&ifd_type, &pl::s(" header"))])?;
        pl::method(it, &et, "HDump", vec![pl::add(&p, &n(8)), n(4), pl::concat(&ifd_type, &pl::s(" IFD offset"))])?;
        pl::method(it, &et, "HDump", vec![pl::add(&p, &ifd_start), n(4), pl::concat(&ifd_type, &pl::s(" entries")), pl::concat(&pl::s("Entry count: "), &num_entries)])?;
        pl::method(it, &et, "HDump", vec![pl::add(&pl::add(&p, &ifd_start), &n(4)), n(4), pl::s("[unused]")])?;
    }
    let format_name = crate::runtime::lex_array(it, MODULE, "formatName");
    let mut index = n(0);
    while pl::lt(&index, &num_entries) {
        'entry: {
            let entry = pl::add(&pl::add(&pl::add(&dir_start, &ifd_start), &n(8)), &pl::mul(&entry_size, &index));
            let tag_id = call(it, "Get32u", vec![data_pt.clone(), entry.clone()])?;
            let size = call(it, "Get32u", vec![data_pt.clone(), pl::sub(&pl::add(&entry, &entry_size), &n(8))])?;
            let mut value_ptr = pl::sub(&pl::add(&entry, &entry_size), &n(4));
            let (_format_size, mut format_str, mut value);
            if pl::num_eq(&entry_size, &n(16)) {
                // (format code only for the 16-byte IFD entry)
                let mut fsz = call(it, "Get32u", vec![data_pt.clone(), pl::add(&entry, &n(4))])?;
                format_str = format_name.at(fsz.to_i64());
                if !format_str.truthy() {
                    let msg = cat(&[pl::s("Unrecognized "), ifd_type.clone(), pl::s(" format size "), fsz.clone()]);
                    pl::method(it, &et, "Warn", vec![msg, n(1)])?;
                    fsz = n(1);
                    format_str = pl::s("undef");
                }
                _format_size = fsz;
            } else if pl::modulo(&size, &n(4))?.truthy() {
                _format_size = n(1);
                format_str = pl::s("undef");
            } else {
                _format_size = n(4);
                format_str = pl::s("int32s");
            }
            if pl::gt(&size, &n(4)) {
                if pl::gt(&size, &n(0x7fffffff)) {
                    let msg = cat(&[pl::s("Invalid size for "), ifd_type.clone(), pl::s(" IFD entry "), index.clone()]);
                    pl::method(it, &et, "Warn", vec![msg])?;
                    return ret(n(0));
                }
                value_ptr = call(it, "Get32u", vec![data_pt.clone(), value_ptr])?;
                if pl::gt(&pl::add(&value_ptr, &size), &dir_len) {
                    let fmt = cat(&[pl::s("Invalid offset 0x%.4x for "), ifd_type.clone(), pl::s(" IFD entry "), index.clone()]);
                    let msg = pl::sprintf_v(&fmt, &[value_ptr.clone()]);
                    pl::method(it, &et, "Warn", vec![msg])?;
                    return ret(n(0));
                }
                value_ptr = pl::add(&value_ptr, &dir_start);
            }
            let tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag_id.clone()])?;
            if tag_info.truthy() {
                if tag_info.get("Format").truthy() {
                    format_str = tag_info.get("Format");
                }
            } else if !(verbose.truthy() || html_dump_on.truthy()) {
                break 'entry;
            }
            let fs = pl::call(it, "Image::ExifTool::FormatSize", vec![format_str.clone()])?;
            let count = pl::int(&pl::div(&size, &fs)?);
            if pl::gt(&count, &n(100000)) && !binary.truthy() {
                value = pl::sref(cat(&[pl::s("Binary data "), size.clone(), pl::s(" bytes")]));
            } else {
                value = call(it, "ReadValue", vec![data_pt.clone(), value_ptr.clone(), format_str.clone(), count.clone(), size.clone()])?;
                // try to distinguish between the various format types
                if pl::str_eq(&format_str, "int32s") {
                    let v = pl::split_ws(&value);
                    let v0 = arg(&v, 0);
                    if !v0.is_undef() {
                        // get floating point exponent (has bias of 127)
                        let exp = ops::shift_right(&ops::bit_and(&v0, &n(0x7f800000)), &n(23));
                        if pl::gt(&exp, &n(120)) && pl::lt(&exp, &n(140)) {
                            format_str = pl::s("float");
                            value = call(it, "ReadValue", vec![data_pt.clone(), value_ptr.clone(), format_str.clone(), count.clone(), size.clone()])?;
                        }
                    }
                }
            }
            if hash_obj.truthy() && tag_info.truthy() && tag_info.get("IsImageData").truthy() {
                let (mut pos, mut len) = (value_ptr.clone(), size.clone());
                while len.truthy() {
                    let nn = if pl::gt(&len, &n(65536)) { n(65536) } else { len.clone() };
                    let tmp = substr(it, &data_pt.deref(), &pos, Some(&nn))?;
                    pl::method(it, &hash_obj, "add", vec![tmp])?;
                    len = pl::sub(&len, &nn);
                    pos = pl::add(&pos, &nn);
                }
                let msg = cat(&[et.get("INDENT"), pl::s("(ImageDataHash: "), size.clone(), pl::s(" bytes of PhaseOne:"), tag_info.get("Name"), pl::s(")\n")]);
                pl::method(it, &et, "VPrint", vec![n(0), msg])?;
            }
            let parms = vec![
                pl::s("DirName"),
                ifd_type.clone(),
                pl::s("Index"),
                index.clone(),
                pl::s("DataPt"),
                data_pt.clone(),
                pl::s("DataPos"),
                data_pos.clone(),
                pl::s("Size"),
                size.clone(),
                pl::s("Start"),
                value_ptr.clone(),
                pl::s("Format"),
                format_str.clone(),
                pl::s("Count"),
                count.clone(),
            ];
            if html_dump_on.truthy() {
                let mut args = vec![et.clone(), tag_table_ptr.clone(), tag_id.clone(), value.clone(), entry.clone(), entry_size.clone()];
                args.extend(parms.iter().cloned());
                args.extend([pl::s("Base"), dir_start.clone()]);
                html_dump(it, args, Want::Scalar)?;
            }
            let mut args = vec![tag_table_ptr.clone(), tag_id, value];
            args.extend(parms);
            pl::method(it, &et, "HandleTag", args)?;
        }
        index = pl::add(&index, &n(1));
    }
    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::PhaseOne::HtmlDump", html_dump);
    it.define_native("Image::ExifTool::PhaseOne::WritePhaseOne", write_phase_one);
    it.define_native("Image::ExifTool::PhaseOne::ProcessPhaseOne", process_phase_one);
}
