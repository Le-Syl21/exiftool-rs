//! Image::ExifTool::KyoceraRaw (Image/ExifTool/KyoceraRaw.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read Kyocera RAW meta information

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell, substr};
use crate::perl::Value;
use crate::pl::{self, arg, n, ret};

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// utility to reverse order of characters in a string
pub fn reverse_string(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut c = pl::unpack("C*", &arg(&a, 0))?;
    c.reverse();
    ret(pl::pack("C*", &c)?)
}

/// Extract information from Kyocera RAW image
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 if this was a valid Kyocera RAW image
pub fn process_raw(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let size = n(156); // size of header
    let buff = cell(Value::Undef);

    if !pl::num_eq(&raf_read_cell(it, &raf, &buff, size.clone())?, &size) {
        return ret(n(0));
    }
    // validate Make string ('KYOCERA' reversed)
    let b = buff.borrow().clone();
    if !substr(it, &b, &n(0x19), Some(&n(7)))?.eq_str("ARECOYK") {
        return ret(n(0));
    }
    pl::method(it, &et, "SetFileType", vec![])?;
    call(it, "SetByteOrder", vec![pl::s("MM")])?;
    let sub_dir_info = Value::new_hash();
    sub_dir_info.set("DataPt", Value::ScalarRef(buff.clone()));
    sub_dir_info.set("DataPos", n(0));
    sub_dir_info.set("DataLen", size.clone());
    sub_dir_info.set("DirStart", n(0));
    sub_dir_info.set("DirLen", size.clone());
    let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::KyoceraRaw::Main")])?;
    pl::method(it, &et, "ProcessDirectory", vec![sub_dir_info, tag_table_ptr])?;
    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::KyoceraRaw::ReverseString", reverse_string);
    it.define_native("Image::ExifTool::KyoceraRaw::ProcessRAW", process_raw);
}
