//! Image::ExifTool::WavPack (Image/ExifTool/WavPack.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read metadata from WavPack audio files

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell};
use crate::perl::Value;
use crate::pl::{self, arg, n, re, ret};

/// `$raf->Read(...)`: die before the call, as perl does on an undefined or
/// unblessed invocant (the read helper runs the call as interpreted code,
/// which would name its own location in the message).
fn check_invocant(it: &mut Interp, raf: &Value, name: &str) -> ER<()> {
    match raf {
        Value::Str(_) | Value::UStr(_) => Ok(()),
        Value::HashRef(h) if h.borrow().blessed.is_some() => Ok(()),
        _ => pl::method(it, raf, name, vec![]).map(|_| ()),
    }
}

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// Extract metadata from a WavPack file
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid WavPack file
pub fn process_wv(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let buff = cell(Value::Undef);

    // verify this is a valid WavPack file
    pl::at(87);
    check_invocant(it, &raf, "Read")?;
    if !pl::num_eq(&raf_read_cell(it, &raf, &buff, n(32))?, &n(32)) {
        return ret(n(0));
    }
    let b = buff.borrow().clone();
    if !re(r"^wvpk.{4}[\x02\x10]\x04", "s").is_match(&b) {
        return ret(n(0));
    }
    pl::method(it, &et, "SetFileType", vec![])?;
    let dir_info2 = Value::new_hash();
    dir_info2.set("DataPt", Value::ScalarRef(buff.clone()));
    dir_info2.set("DirStart", n(0));
    dir_info2.set("DirLen", pl::length(&b));
    let table = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::WavPack::Main")])?;
    pl::method(it, &et, "ProcessBinaryData", vec![dir_info2, table])?;
    // historically, the RIFF module has handled RIFF-type WavPack files
    pl::method(it, &raf, "Seek", vec![n(0), n(0)])?;
    et.get_or_new_array("PATH").push(pl::s("RIFF")); // update metadata path
    pl::call(it, "Image::ExifTool::RIFF::ProcessRIFF", vec![et.clone(), dir_info.clone()])?;
    // also look for ID3 and APE trailers (ProcessAPE also checks for ID3)
    let path = et.get_or_new_array("PATH");
    let last = path.len() as i64 - 1;
    if last >= 0 {
        path.set_at(last as usize, pl::s("APE"));
    }
    pl::call(it, "Image::ExifTool::APE::ProcessAPE", vec![et.clone(), dir_info])?;
    if let Value::ArrayRef(p) = et.get("PATH") {
        p.borrow_mut().pop();
    }
    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::WavPack::ProcessWV", process_wv);
}
