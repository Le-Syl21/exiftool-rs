//! Image::ExifTool::JVC (Image/ExifTool/JVC.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::substr;
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

/// Process JVC text-based maker notes
/// Inputs: 0) ExifTool object reference
///         1) Reference to directory information hash
///         2) Pointer to tag table for this directory
/// Returns: 1 on success, otherwise returns 0 and sets a Warning
pub fn process_jvc_text(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let dir_start = dir_info.get("DirStart");
    let dir_start = if dir_start.truthy() { dir_start } else { n(0) };
    let data_len = dir_info.get("DataLen");
    let dir_len = dir_info.get("DirLen");
    let dir_len = if dir_len.truthy() { dir_len } else { pl::sub(&data_len, &dir_start) };
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;

    pl::at(73);
    let data = substr(it, &pl::sderef(&data_pt)?, &dir_start, Some(&dir_len))?;
    // validate text maker notes
    if !re(r"^VER:", "").is_match(&data) {
        pl::method(it, &et, "Warn", vec![pl::s("Bad JVC text maker notes")])?;
        return ret(n(0));
    }
    for c in re(r"([A-Z]+):(.{3,4})", "s").each(&data) {
        let (tag, val) = (cap(&c, 1), cap(&c, 2));
        let mut tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
        if verbose.truthy() {
            pl::method(it, &et, "VerboseInfo", vec![tag.clone(), tag_info.clone(), pl::s("Table"), tag_table_ptr.clone(), pl::s("Value"), val.clone()])?;
        }
        if !tag_info.truthy() {
            if !et.path(&["OPTIONS", "Unknown"]).truthy() {
                continue;
            }
            tag_info = Value::new_hash();
            tag_info.set("Name", pl::concat(&pl::s("JVC_Text_"), &tag));
            tag_info.set("Unknown", n(1));
            tag_info.set("PrintConv", Value::Code(it.sub_ref("Image::ExifTool::LimitLongValues")));
            // add tag information to table
            pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), tag_info.clone()])?;
        }
        pl::method(it, &et, "FoundTag", vec![tag_info, val])?;
    }
    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::JVC::ProcessJVCText", process_jvc_text);
}
