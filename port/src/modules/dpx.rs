//! Image::ExifTool::DPX (Image/ExifTool/DPX.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read DPX (Digital Picture Exchange) meta information.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

fn s(x: &str) -> Value {
    pl::s(x)
}

fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

//------------------------------------------------------------------------------
// Extract EXIF information from a DPX image
// Inputs: 0) ExifTool object reference, 1) dirInfo reference
// Returns: 1 on success, 0 if this wasn't a valid DPX file
pub fn process_dpx(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let buff = cell(Value::Undef);
    let buff_pt = Value::ScalarRef(buff.clone());

    // verify this is a valid DPX file
    if !pl::num_eq(&raf_read_cell(it, &raf, &buff, n(2080))?, &n(2080)) {
        return ret(n(0));
    }
    let Some(m) = re(r"^(SDPX|XPDS)", "").m(&buff.borrow()) else {
        return ret(n(0));
    };
    call(it, "SetByteOrder", vec![s(if pl::str_eq(&cap(&m, 1), "SDPX") { "MM" } else { "II" })])?;
    pl::method(it, &et, "SetFileType", vec![])?; // set the FileType tag
    let hdr_len = pl::add(&call(it, "Get32u", vec![buff_pt.clone(), n(24)])?, &call(it, "Get32u", vec![buff_pt.clone(), n(28)])?);
    if !pl::num_eq(&hdr_len, &n(2048)) {
        let msg = pl::concat(&pl::concat(&s("Unexpected DPX header length ("), &hdr_len), &s(")"));
        pl::method(it, &et, "Warn", vec![msg])?;
    }
    let dir_info2 = Value::new_hash();
    dir_info2.set("DataPt", buff_pt);
    dir_info2.set("DirStart", n(0));
    dir_info2.set("DirLen", pl::length(&buff.borrow()));
    let tag_table_ptr = call(it, "GetTagTable", vec![s("Image::ExifTool::DPX::Main")])?;
    pl::method(it, &et, "ProcessDirectory", vec![dir_info2, tag_table_ptr])?;

    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::DPX::ProcessDPX", process_dpx);
}
