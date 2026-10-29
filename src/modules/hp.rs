//! Image::ExifTool::HP (Image/ExifTool/HP.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Hewlett-Packard maker notes tags

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, perl_eval, substr};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, ret};

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// `( KEY => VAL, ... )` as a hash reference
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

//------------------------------------------------------------------------------
/// Process HP APP6 TDHD metadata (ref PH)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_tdhd(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let data_pos = dir_info.get("DataPos");
    let mut pos = dir_info.get("DirStart");
    let dir_end = pl::add(&pos, &dir_info.get("DirLen"));
    let unknown = pl::method(it, &et, "Options", vec![pl::s("Unknown")])?;
    let unknown = if unknown.truthy() { unknown } else { pl::method(it, &et, "Options", vec![pl::s("Verbose")])? };
    pl::method(it, &et, "VerboseDir", vec![pl::s("TDHD"), Value::Undef, dir_info.get("DirLen")])?;
    call(it, "SetByteOrder", vec![pl::s("II")])?;
    while pl::lt(&pl::add(&pos, &n(12)), &dir_end) {
        pl::at(154);
        let tag = substr(it, &pl::sderef(&data_pt)?, &pos, Some(&n(4)))?;
        let type_ = call(it, "Get32u", vec![data_pt.clone(), pl::add(&pos, &n(4))])?;
        let size = call(it, "Get32u", vec![data_pt.clone(), pl::add(&pos, &n(8))])?;
        pos = pl::add(&pos, &n(12));
        if pl::lt(&size, &n(0)) || pl::gt(&pl::add(&pos, &size), &dir_end) {
            break;
        }
        if pl::num_eq(&type_, &n(0x10001)) {
            // this is a subdirectory containing more tags
            let dir_info = hash(vec![("DataPt", data_pt.clone()), ("DataPos", data_pos.clone()), ("DirStart", pos.clone()), ("DirLen", size.clone())]);
            pl::method(it, &et, "ProcessDirectory", vec![dir_info, tag_table_ptr.clone()])?;
        } else {
            if !tag_table_ptr.get(tag.to_bytes()).truthy() && unknown.truthy() {
                let mut name = tag.clone();
                pl::tr(&mut name, "-_A-Za-z0-9", "", "dc"); // remove invalid characters
                let tag_info = hash(vec![("Name", pl::concat(&pl::s("HP_TDHD_"), &name)), ("Unknown", n(1))]);
                // guess format based on data size
                if pl::num_eq(&size, &n(1)) {
                    tag_info.set("Format", pl::s("int8u"));
                } else if pl::num_eq(&size, &n(2)) {
                    tag_info.set("Format", pl::s("int16u"));
                } else if pl::num_eq(&size, &n(4)) {
                    tag_info.set("Format", pl::s("int32s"));
                } else if pl::gt(&size, &n(80)) {
                    tag_info.set("Binary", n(1));
                }
                call(it, "AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), tag_info])?;
            }
            pl::method(
                it,
                &et,
                "HandleTag",
                vec![tag_table_ptr.clone(), tag, Value::Undef, pl::s("DataPt"), data_pt.clone(), pl::s("DataPos"), data_pos.clone(), pl::s("Start"), pos.clone(), pl::s("Size"), size.clone()],
            )?;
        }
        pos = pl::add(&pos, &size);
    }
    ret(n(1))
}

//------------------------------------------------------------------------------
/// Process HP maker notes
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success, otherwise returns 0 and sets a Warning
pub fn process_hp(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let mut data_pt = dir_info.get("DataPt");
    let data_len = dir_info.get("DataLen");
    let dir_start = or(dir_info.get("DirStart"), || n(0));
    let dir_len = or(dir_info.get("DirLen"), || pl::sub(&data_len, &dir_start));

    // look for known text-type tags
    pl::at(213);
    if dir_start.truthy() || !pl::num_eq(&dir_len, &pl::length(&pl::sderef(&data_pt)?)) {
        pl::at(214);
        let buff = substr(it, &pl::sderef(&data_pt)?, &dir_start, Some(&dir_len))?;
        data_pt = pl::sref(buff);
    }
    // brute-force scan for PreviewImage
    // (the match runs in perl so pos() is kept on the scanned scalar, as /g does)
    if tag_table_ptr.get("PreviewImage").truthy() {
        let d = cell(data_pt.clone());
        let m = perl_eval(it, r"$$d =~ /(\xff\xd8\xff\xdb.*\xff\xd9)/gs ? ($1, pos($$d)) : ()", "Image::ExifTool::HP", &[("d", &d)], Want::List)?;
        if m.len() == 2 {
            let (one, p) = (m[0].clone(), m[1].clone());
            pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("PreviewImage"), one.clone()])?;
            // truncate preview to speed subsequent tag scans
            let buff = substr(it, &data_pt.deref(), &n(0), Some(&pl::sub(&p, &pl::length(&one))))?;
            data_pt = pl::sref(buff);
        }
    }
    // scan for other tag ID's
    let mut keys = pl::call_list(it, "Image::ExifTool::TagTableKeys", vec![tag_table_ptr.clone()])?;
    keys.sort_by(|x, y| x.to_bytes().cmp(&y.to_bytes()));
    for tag_id in keys {
        if pl::str_eq(&tag_id, "PreviewImage") {
            continue;
        }
        let mut p = tag_id.to_bytes().to_vec();
        p.extend_from_slice(br":\s*([\x20-\x7e]+)");
        let Some(c) = pl::re_dyn(&p, "i")?.m(&data_pt.deref()) else { continue };
        pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tag_id, cap(&c, 1)])?;
    }
    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::HP::ProcessTDHD", process_tdhd);
    it.define_native("Image::ExifTool::HP::ProcessHP", process_hp);
}
