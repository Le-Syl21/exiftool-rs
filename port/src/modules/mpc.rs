//! Image::ExifTool::MPC (Image/ExifTool/MPC.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read Musepack audio meta information

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

/// `$raf->Read(...)` runs as a perl statement (random_access::perl_eval),
/// whose errors would name "(eval N)": an invocant that is not an object
/// gives perl's error here instead, at the native's location.
fn need_obj(v: &Value, method: &str) -> ER<()> {
    match v {
        Value::Undef => crate::interp::eval::die(format!("Can't call method \"{method}\" on an undefined value")),
        Value::HashRef(h) if h.borrow().blessed.is_none() => crate::interp::eval::die(format!("Can't call method \"{method}\" on unblessed reference")),
        Value::ArrayRef(_) | Value::ScalarRef(_) | Value::Code(_) => crate::interp::eval::die(format!("Can't call method \"{method}\" on unblessed reference")),
        _ => Ok(()),
    }
}

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// `ord($ch)`
fn ord(v: &Value) -> i64 {
    v.to_bytes().first().map_or(0, |b| *b as i64)
}

/// Extract information from an MPC file
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// - Just looks for MPC trailer if FileType is already set
/// Returns: 1 on success, 0 if this wasn't a valid MPC file
pub fn process_mpc(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));

    // must first check for leading ID3 information
    if !et.get("DoneID3").truthy() {
        crate::runtime::require(it, "Image::ExifTool::ID3")?;
        if pl::call(it, "Image::ExifTool::ID3::ProcessID3", vec![et.clone(), dir_info.clone()])?.truthy() {
            return ret(n(1));
        }
    }
    let raf = dir_info.get("RAF");
    let buff = cell(Value::Undef);

    // check MPC signature
    pl::at(92);
    need_obj(&raf, "Read")?;
    if !pl::num_eq(&raf_read_cell(it, &raf, &buff, n(32))?, &n(32)) {
        return ret(n(0));
    }
    let b = buff.borrow().clone();
    let Some(m) = re(r"^MP\+(.)", "s").m(&b) else {
        return ret(n(0));
    };
    let vers = ord(&cap(&m, 1)) & 0x0f;
    pl::method(it, &et, "SetFileType", vec![])?;

    // extract audio information (currently only from version 7 MPC files)
    if vers == 0x07 {
        call(it, "SetByteOrder", vec![pl::s("II")])?;
        let pos = pl::sub(&pl::method(it, &raf, "Tell", vec![])?, &n(32));
        if pl::method(it, &et, "Options", vec![pl::s("Verbose")])?.truthy() {
            pl::method(it, &et, "VPrint", vec![n(0), pl::s("MPC Header (32 bytes):\n")])?;
            pl::method(it, &et, "VerboseDump", vec![Value::ScalarRef(buff.clone()), pl::s("DataPos"), pos.clone()])?;
        }
        let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::MPC::Main")])?;
        let dir_info2 = Value::new_hash();
        dir_info2.set("DataPt", Value::ScalarRef(buff.clone()));
        dir_info2.set("DataPos", pos);
        pl::method(it, &et, "ProcessDirectory", vec![dir_info2, tag_table_ptr])?;
    } else {
        pl::method(it, &et, "Warn", vec![pl::s("Audio info currently not extracted from this version MPC file")])?;
    }

    // process APE trailer if it exists
    crate::runtime::require(it, "Image::ExifTool::APE")?;
    pl::call(it, "Image::ExifTool::APE::ProcessAPE", vec![et.clone(), dir_info])?;

    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::MPC::ProcessMPC", process_mpc);
}
