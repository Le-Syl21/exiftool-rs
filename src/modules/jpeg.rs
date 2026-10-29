//! Image::ExifTool::JPEG (Image/ExifTool/JPEG.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, perl_eval, perl_eval1, substr};
use crate::perl::Value;
use crate::pl::{self, arg, cap, re, ret};

fn n(i: i64) -> Value {
    Value::Int(i)
}

/// Extract information from the JPEG APP0 Ocad segment
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_ocad(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    pl::at(775);
    let len = pl::length(&pl::sderef(&data_pt)?);
    pl::method(it, &et, "VerboseDir", vec![pl::s("APP0 Ocad"), Value::Undef, len])?;
    let dp = cell(data_pt.clone());
    loop {
        // (the //g match continues from pos($$dataPt), so it runs as the Perl)
        let m = perl_eval(it, r"$$dataPt =~ /\$(\w+):([^\0\$]+)/g ? ($1, $2) : ()", "Image::ExifTool::JPEG", &[("dataPt", &dp)], Want::List)?;
        if m.is_empty() {
            break;
        }
        let (tag, mut val) = (arg(&m, 0), arg(&m, 1));
        // remove leading/trailing spaces
        re(r"^\s+", "").subst(&mut val, false, |_| Vec::new());
        re(r"\s+$", "").subst(&mut val, false, |_| Vec::new());
        if !tag_table_ptr.get(tag.to_bytes()).truthy() {
            pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_table_ptr.clone(), tag.clone()])?;
        }
        pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tag, val])?;
    }
    ret(n(1))
}

/// Extract information from the JPEG APP0 Ocad segment
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_jpeg_hdr(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let dp = cell(data_pt.clone());
    // $$dataPt =~ /~\0/g (pos($$dataPt) is left set, as in the Perl)
    pl::at(794);
    pl::sderef(&data_pt)?;
    let pos = perl_eval1(it, r"$$dataPt =~ /~\0/g ? pos($$dataPt) : undef", "Image::ExifTool::JPEG", &[("dataPt", &dp)])?;
    if pos.is_undef() {
        pl::method(it, &et, "Warn", vec![pl::s("Unrecognized JPEG-HDR format")])?;
        return ret(n(0));
    }
    let meta = substr(it, &data_pt.deref(), &n(7), Some(&pl::sub(&pos, &n(9))))?;
    pl::method(it, &et, "VerboseDir", vec![pl::s("APP11 JPEG-HDR"), Value::Undef, pl::length(&data_pt.deref())])?;
    for c in re(r"(\w+)=([^,\s]*)", "").each(&meta) {
        let (tag, val) = (cap(&c, 1), cap(&c, 2));
        if !tag_table_ptr.get(tag.to_bytes()).truthy() {
            pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_table_ptr.clone(), tag.clone()])?;
        }
        pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tag, val])?;
    }
    let ratio = substr(it, &data_pt.deref(), &pos, None)?;
    pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("RatioImage"), ratio])?;
    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::JPEG::ProcessOcad", process_ocad);
    it.define_native("Image::ExifTool::JPEG::ProcessJPEG_HDR", process_jpeg_hdr);
}
