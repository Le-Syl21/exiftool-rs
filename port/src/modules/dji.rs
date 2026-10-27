//! Image::ExifTool::DJI (Image/ExifTool/DJI.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! DJI Phantom maker notes tags

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, perl_eval1, substr};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

fn or(a: Value, b: impl FnOnce() -> Value) -> Value {
    if a.truthy() {
        a
    } else {
        b()
    }
}

//------------------------------------------------------------------------------
/// Process DJI beauty settings (ref PH)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_settings(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_tbl) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    pl::at(947);
    let data = pl::sderef(&dir_info.get("DataPt"))?;
    pl::method(it, &et, "VerboseDir", vec![dir_info.get("DirName"), Value::Undef, pl::length(&data)])?;
    for item in pl::split(&re(";", ""), &data, 0) {
        // (a list assignment of 2 scalars splits with an implicit limit of 3)
        let p = pl::split(&re("=", ""), &item, 3);
        let (tag, val) = (arg(&p, 0), arg(&p, 1));
        if tag.is_undef() || val.is_undef() {
            continue;
        }
        pl::method(it, &et, "HandleTag", vec![tag_tbl.clone(), tag, val, pl::s("MakeTagInfo"), n(1)])?;
    }
    ret(n(1))
}

//------------------------------------------------------------------------------
/// Process DJI info (ref PH)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_dji_info(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_tbl) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let mut data_pt = dir_info.get("DataPt");
    let dir_start = or(dir_info.get("DirStart"), || n(0));
    let mut dir_len = dir_info.get("DirLen");
    if !dir_len.truthy() {
        pl::at(965);
        dir_len = pl::sub(&pl::length(&pl::sderef(&data_pt)?), &dir_start);
    }
    let own = dir_start.truthy();
    if own {
        pl::at(967);
        let buff = substr(it, &pl::sderef(&data_pt)?, &dir_start, Some(&dir_len))?;
        data_pt = pl::sref(buff);
    }
    pl::at(970);
    pl::method(it, &et, "VerboseDir", vec![pl::s("DJIInfo"), Value::Undef, pl::length(&pl::sderef(&data_pt)?)])?;
    // (\G starts at the pos() of the caller's scalar, and the final failed
    // match resets it)
    let d = cell(data_pt.clone());
    let mut start = 0usize;
    let mut had_pos = false;
    if !own {
        let p = perl_eval1(it, "pos($$d)", "Image::ExifTool::DJI", &[("d", &d)])?;
        if !p.is_undef() {
            had_pos = true;
            start = p.to_i64() as usize;
        }
    }
    let rx = crate::perl::regex::compile(br"\G\[(.*?)\](?=(\[|$))", "s").expect("pattern");
    let subject = data_pt.deref().to_bytes();
    loop {
        let Ok(Some(sp)) = rx.match_at(&subject, start) else { break };
        // (the match is never empty: it holds the brackets)
        start = sp[0].unwrap().1;
        let one = sp[1].map(|(s, e)| Value::bytes(&subject[s..e])).unwrap_or_default();
        let p = pl::split(&re(":", ""), &one, 2);
        let (tag, mut val) = (arg(&p, 0), arg(&p, 1));
        if tag.is_undef() || val.is_undef() {
            continue;
        }
        if let Some(c) = re(r"^([\x20-\x7e]+)\0*$", "").m(&val) {
            val = cap(&c, 1);
        } else {
            let buff = val.clone();
            val = pl::sref(buff);
        }
        pl::method(it, &et, "HandleTag", vec![tag_tbl.clone(), tag, val, pl::s("MakeTagInfo"), n(1)])?;
    }
    if had_pos {
        perl_eval1(it, "pos($$d) = undef", "Image::ExifTool::DJI", &[("d", &d)])?;
    }
    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::DJI::ProcessSettings", process_settings);
    it.define_native("Image::ExifTool::DJI::ProcessDJIInfo", process_dji_info);
}
