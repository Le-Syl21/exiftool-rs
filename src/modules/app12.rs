//! Image::ExifTool::APP12 (Image/ExifTool/APP12.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read APP12 meta information

use std::collections::HashSet;

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::write_exif::{g16u, inc, keys, len, set_byte_order, sort_num};
use crate::perl::Value;
use crate::pl::{self, arg, cap, re, ret};

fn n(i: i64) -> Value {
    Value::Int(i)
}

fn s(x: &str) -> Value {
    Value::str(x)
}

/// Write APP12 Ducky segment
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: New directory data or undefined on error
pub fn write_ducky(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if !et.truthy() {
        return ret(n(1)); // allow dummy access to autoload this package
    }
    let mut data_pt = dir_info.get("DataPt");
    let mut pos = dir_info.get("DirStart");
    let new_tags = pl::method(it, &et, "GetNewTagInfoHash", vec![tag_table_ptr.clone()])?;
    let mut add_tags = keys(&new_tags);
    sort_num(&mut add_tags);
    let dir_end;
    let mut done_tags: HashSet<Vec<u8>> = HashSet::new();
    if data_pt.truthy() {
        dir_end = pl::add(&pos, &dir_info.get("DirLen"));
    } else {
        data_pt = pl::sref(s(""));
        pos = n(0);
        dir_end = n(0);
    }
    let mut new_data = s("");
    set_byte_order(it, s("MM"))?;
    // process all data blocks in Ducky segment
    loop {
        let (tag, mut val);
        if pl::le(&pl::add(&pos, &n(4)), &dir_end) {
            tag = g16u(it, &data_pt, pos.clone())?;
            let l = g16u(it, &data_pt, pl::add(&pos, &n(2)))?;
            pos = pl::add(&pos, &n(4));
            if pl::gt(&pl::add(&pos, &l), &dir_end) {
                pl::method(it, &et, "Warn", vec![s("Invalid Ducky block length")])?;
                return pl::ret_undef();
            }
            pl::at(164);
            val = pl::substr(&pl::sderef(&data_pt)?, pos.to_i64(), Some(l.to_i64()));
            pos = pl::add(&pos, &l);
        } else {
            let Some(t) = add_tags.pop() else { break };
            tag = t;
            val = Value::Undef;
            if done_tags.contains(&tag.to_bytes()[..]) {
                continue;
            }
        }
        done_tags.insert(tag.to_bytes().to_vec());
        let tag_info = new_tags.get(tag.to_bytes());
        if tag_info.truthy() {
            let nv_hash = pl::method(it, &et, "GetNewValueHash", vec![tag_info.clone()])?;
            let mut is_new = false;
            if !val.is_undef() {
                if pl::method(it, &et, "IsOverwriting", vec![nv_hash.clone(), val.clone()])?.truthy() {
                    pl::method(it, &et, "VerboseValue", vec![pl::concat(&s("- Ducky:"), &tag_info.get("Name")), val.clone()])?;
                    is_new = true;
                }
            } else {
                if !nv_hash.get("IsCreating").truthy() {
                    continue;
                }
                is_new = true;
            }
            if is_new {
                val = pl::method(it, &et, "GetNewValue", vec![nv_hash.clone()])?;
                inc(&et, "CHANGED");
                if val.is_undef() {
                    continue; // next if tag is being deleted
                }
                pl::method(it, &et, "VerboseValue", vec![pl::concat(&s("+ Ducky:"), &tag_info.get("Name")), val.clone()])?;
            }
        }
        let hdr = pl::pack("nn", &[tag.clone(), len(&val)])?;
        new_data = pl::concat(&pl::concat(&new_data, &hdr), &val);
    }
    if len(&new_data).to_i64() > 0 {
        new_data = pl::concat(&new_data, &s("\0\0"));
    }
    ret(new_data)
}

/// Process APP12 Ducky segment (ref PH)
/// Inputs: 0) ExifTool object reference, 1) Directory information ref, 2) tag table ref
/// Returns: 1 on success, 0 if this wasn't a recognized Ducky segment
pub fn process_ducky(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let mut pos = dir_info.get("DirStart");
    let dir_end = pl::add(&pos, &dir_info.get("DirLen"));
    set_byte_order(it, s("MM"))?;
    // process all data blocks in Ducky segment
    loop {
        if pl::gt(&pl::add(&pos, &n(4)), &dir_end) {
            break;
        }
        let tag = g16u(it, &data_pt, pos.clone())?;
        let l = g16u(it, &data_pt, pl::add(&pos, &n(2)))?;
        pos = pl::add(&pos, &n(4));
        if pl::gt(&pl::add(&pos, &l), &dir_end) {
            pl::method(it, &et, "Warn", vec![s("Invalid Ducky block length")])?;
            break;
        }
        pl::at(225);
        let val = pl::substr(&pl::sderef(&data_pt)?, pos.to_i64(), Some(l.to_i64()));
        pl::method(
            it,
            &et,
            "HandleTag",
            vec![tag_table_ptr.clone(), tag, val, s("DataPt"), data_pt.clone(), s("DataPos"), dir_info.get("DataPos"), s("Start"), pos.clone(), s("Size"), l.clone()],
        )?;
        pos = pl::add(&pos, &l);
    }
    ret(n(1))
}

/// Process APP12 Picture Info segment (ref PH)
/// Inputs: 0) ExifTool object reference, 1) Directory information ref, 2) tag table ref
/// Returns: 1 on success, 0 if this wasn't a recognized APP12
pub fn process_app12(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let mut data_pt = dir_info.get("DataPt");
    let ds = dir_info.get("DirStart");
    let dir_start = if ds.truthy() { ds } else { n(0) };
    let dl = dir_info.get("DirLen");
    pl::at(246);
    let dir_len = if dl.truthy() { dl } else { pl::sub(&len(&pl::sderef(&data_pt)?), &dir_start) };
    pl::at(247);
    let d = pl::sderef(&data_pt)?;
    if !pl::num_eq(&dir_len, &pl::add(&dir_start, &len(&d))) {
        pl::at(248);
        let buff = crate::modules::write_exif::substr(it, &d, &dir_start, Some(&dir_len))?;
        data_pt = pl::sref(buff);
    }
    // (pos($$dataPt) is set here, then reset to 0 before the search below)
    let verbose = pl::method(it, &et, "Options", vec![s("Verbose")])?;
    let mut success = 0;
    let mut section = s("");

    // this regular expression is a bit complex, but basically we are looking for
    // section headers (eg. "[Camera Info]") and tag/value pairs (eg. "tag=value",
    // where "value" may contain white space), separated by spaces or CR/LF.
    // (APP12 uses CR/LF, but Olympus TextualInfo is similar and uses spaces)
    let tokens = re(r"(\[.*?\]|[\w#-]+=[\x20-\x7e]+?(?=\s*([\n\r\0]|[\w#-]+=|\[|$)))", "").each(&data_pt.deref());
    for c in tokens {
        let token = cap(&c, 1);
        // was this a section name?
        if let Some(m) = re(r"^\[(.*)\]", "").m(&token) {
            if verbose.truthy() {
                pl::method(it, &et, "VerboseDir", vec![cap(&m, 1)])?;
            }
            section = match re(r"\[(\S+) ?Info\]", "i").m(&token) {
                Some(m2) => cap(&m2, 1),
                None => s(""),
            };
            success = 1;
            continue;
        }
        if verbose.truthy() && success == 0 {
            pl::method(it, &et, "VerboseDir", vec![dir_info.get("DirName")])?;
        }
        success = 1;
        let (tag, val) = match re(r"(\S+)=(.+)", "").m(&token) {
            Some(m) => (cap(&m, 1), cap(&m, 2)),
            None => (Value::Undef, Value::Undef),
        };
        let mut tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
        if verbose.truthy() {
            pl::method(it, &et, "VerboseInfo", vec![tag.clone(), tag_info.clone(), s("Value"), val.clone()])?;
        }
        if !tag_info.truthy() {
            // add new tag to table
            tag_info = Value::new_hash();
            tag_info.set("Name", pl::ucfirst(&tag));
            // put in Camera group if information in "Camera" section
            if re(r"camera", "i").is_match(&section) {
                let g = Value::new_hash();
                g.set("2", s("Camera"));
                tag_info.set("Groups", g);
            }
            let msg = Value::bytes([&b"[adding APP12:"[..], &tag_info.get("Name").to_bytes(), b"]\n"].concat());
            pl::method(it, &et, "VPrint", vec![n(0), et.get("INDENT"), msg])?;
            pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), tag_info.clone()])?;
        }
        pl::method(it, &et, "FoundTag", vec![tag_info, val])?;
    }
    ret(n(success))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::APP12::WriteDucky", write_ducky);
    it.define_native("Image::ExifTool::APP12::ProcessDucky", process_ducky);
    it.define_native("Image::ExifTool::APP12::ProcessAPP12", process_app12);
}
