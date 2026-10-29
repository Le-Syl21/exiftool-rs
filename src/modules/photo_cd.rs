//! Image::ExifTool::PhotoCD (Image/ExifTool/PhotoCD.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read Kodak Photo CD Image Pac (PCD) metadata.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell};
use crate::perl::Value;
use crate::pl::{self, arg, n, re, ret};

fn s(x: &str) -> Value {
    pl::s(x)
}

//------------------------------------------------------------------------------
// Extract information from a PhotoCD image
// Inputs: 0) ExifTool object reference, 1) dirInfo reference
// Returns: 1 on success, 0 if this wasn't a valid PhotoCD file
pub fn process_pcd(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let buff = cell(Value::Undef);
    pl::at(454);
    if !(pl::method(it, &raf, "Seek", vec![n(2048), n(0)])?.truthy()
        && pl::num_eq(&raf_read_cell(it, &raf, &buff, n(2048))?, &n(2048))
        && re(r"^PCD_IPI", "").is_match(&buff.borrow()))
    {
        return ret(n(0));
    }
    pl::call(it, "Image::ExifTool::SetByteOrder", vec![s("MM")])?;
    pl::method(it, &et, "SetFileType", vec![])?;
    let dir_info2 = Value::new_hash();
    dir_info2.set("DirName", s("PhotoCD"));
    dir_info2.set("DataPt", Value::ScalarRef(buff.clone()));
    dir_info2.set("DataPos", n(4096));
    let tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![s("Image::ExifTool::PhotoCD::Main")])?;
    ret(pl::method(it, &et, "ProcessBinaryData", vec![dir_info2, tag_table_ptr])?)
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::PhotoCD::ProcessPCD", process_pcd);
}
