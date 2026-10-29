//! Image::ExifTool::DSF (Image/ExifTool/DSF.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read DSF meta information

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell, substr};
use crate::perl::Value;
use crate::pl::{self, arg, n, re, ret};
use crate::modules::quick_time_stream::check_invocant;

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// Extract metadata from a DSF file
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid DSF file
pub fn process_dsf(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let (buff, buf2) = (cell(Value::Undef), cell(Value::Undef));

    // verify this is a valid DSF file
    pl::at(62);
    check_invocant(it, &raf, "Read")?;
    if !pl::num_eq(&raf_read_cell(it, &raf, &buff, n(40))?, &n(40)) {
        return ret(n(0));
    }
    if !re(r"^DSD \x1c\0{7}.{16}fmt ", "s").is_match(&buff.borrow().clone()) {
        return ret(n(0));
    }
    pl::method(it, &et, "SetFileType", vec![])?; // set the FileType tag
    let tag_tbl = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::DSF::Main")])?;
    call(it, "SetByteOrder", vec![pl::s("II")])?;
    let buff_ref = Value::ScalarRef(buff.clone());
    let fmt_len = call(it, "Get64u", vec![buff_ref.clone(), n(32)])?;
    let ok = pl::gt(&fmt_len, &n(12)) && pl::lt(&fmt_len, &n(1000)) && {
        let len = pl::sub(&fmt_len, &n(12));
        pl::num_eq(&raf_read_cell(it, &raf, &buf2, len.clone())?, &len)
    };
    if !ok {
        pl::method(it, &et, "Warn", vec![pl::s("Error reading DSF fmt chunk")])?;
        return ret(n(1));
    }
    let file_size = call(it, "Get64u", vec![buff_ref.clone(), n(12)])?;
    let meta_pos = call(it, "Get64u", vec![buff_ref.clone(), n(20)])?;
    let b = buff.borrow().clone();
    let b = pl::concat(&substr(it, &b, &n(28), None)?, &buf2.borrow().clone());
    *buff.borrow_mut() = b.clone();
    //
    // process the DSF 'fmt ' chunk
    //
    let dir_info2 = Value::new_hash();
    dir_info2.set("DataPt", buff_ref.clone());
    dir_info2.set("DirStart", n(0));
    dir_info2.set("DirLen", pl::length(&b));
    pl::method(it, &et, "ProcessBinaryData", vec![dir_info2.clone(), tag_tbl])?;
    //
    // process ID3v2 if it exists
    //
    let meta_len = pl::sub(&file_size, &meta_pos);
    if meta_pos.truthy()
        && pl::gt(&meta_len, &n(0))
        && pl::lt(&meta_len, &n(20000000))
        && pl::method(it, &raf, "Seek", vec![meta_pos.clone(), n(0)])?.truthy()
        && pl::num_eq(&raf_read_cell(it, &raf, &buff, meta_len.clone())?, &meta_len)
    {
        dir_info2.set("DataPos", meta_pos);
        dir_info2.set("DirLen", meta_len);
        let id3_tbl = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::ID3::Main")])?;
        pl::method(it, &et, "ProcessDirectory", vec![dir_info2, id3_tbl])?;
    }
    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::DSF::ProcessDSF", process_dsf);
}
