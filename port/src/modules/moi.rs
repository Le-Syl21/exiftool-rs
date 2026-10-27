//! Image::ExifTool::MOI (Image/ExifTool/MOI.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read MOI meta information.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell};
use crate::perl::Value;
use crate::pl::{self, arg, n, re, ret};

fn s(x: &str) -> Value {
    pl::s(x)
}

//------------------------------------------------------------------------------
// Validate and extract metadata from MOI file
// Inputs: 0) ExifTool ref, 1) dirInfo ref
// Returns: 1 on success, 0 if this wasn't a valid MOI file
pub fn process_moi(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let buff = cell(Value::Undef);
    // read enough to allow skipping over run-in if it exists
    if !(pl::num_eq(&raf_read_cell(it, &raf, &buff, n(256))?, &n(256)) && re(r"^V6", "").is_match(&buff.borrow())) {
        return ret(n(0));
    }
    let file_size = et.path(&["VALUE", "FileSize"]);
    if !file_size.is_undef() {
        let size = pl::unpack1("x2N", &buff.borrow())?;
        if !pl::num_eq(&size, &file_size) {
            return ret(n(0));
        }
    }
    pl::method(it, &et, "SetFileType", vec![])?;
    pl::call(it, "Image::ExifTool::SetByteOrder", vec![s("MM")])?;
    let tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![s("Image::ExifTool::MOI::Main")])?;
    let di = Value::new_hash();
    di.set("DataPt", Value::ScalarRef(buff.clone()));
    ret(pl::method(it, &et, "ProcessBinaryData", vec![di, tag_table_ptr])?)
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::MOI::ProcessMOI", process_moi);
}
