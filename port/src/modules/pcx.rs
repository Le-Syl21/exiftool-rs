//! Image::ExifTool::PCX (Image/ExifTool/PCX.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read metadata from PC Paintbrush files.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell};
use crate::perl::Value;
use crate::pl::{self, arg, n, re, ret};

fn s(x: &str) -> Value {
    pl::s(x)
}

/// `$raf->METHOD(...)` dies as perl does on an undefined `$raf` (the read
/// itself runs as a Perl snippet, whose messages name the snippet).
fn raf_defined(raf: &Value, method: &str) -> ER<()> {
    if raf.is_undef() {
        return crate::interp::eval::die(format!("Can't call method \"{method}\" on an undefined value"));
    }
    Ok(())
}

//------------------------------------------------------------------------------
// Extract information from a PCX image
// Inputs: 0) ExifTool object reference, 1) dirInfo reference
// Returns: 1 on success, 0 if this wasn't a valid PCX file
pub fn process_pcx(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let buff = cell(Value::Undef);
    pl::at(89);
    raf_defined(&raf, "Read")?;
    if !(pl::num_eq(&raf_read_cell(it, &raf, &buff, n(0x50))?, &n(0x50)) && re(r"^\x0a[\0-\x05]\x01[\x01\x02\x04\x08].{64}[\0-\x02]", "s").is_match(&buff.borrow())) {
        return ret(n(0));
    }
    pl::call(it, "Image::ExifTool::SetByteOrder", vec![s("II")])?;
    pl::method(it, &et, "SetFileType", vec![])?;
    let dir_info2 = Value::new_hash();
    dir_info2.set("DirName", s("PCX"));
    dir_info2.set("DataPt", Value::ScalarRef(buff.clone()));
    let tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![s("Image::ExifTool::PCX::Main")])?;
    ret(pl::method(it, &et, "ProcessBinaryData", vec![dir_info2, tag_table_ptr])?)
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::PCX::ProcessPCX", process_pcx);
}
