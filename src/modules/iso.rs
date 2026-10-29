//! Image::ExifTool::ISO (Image/ExifTool/ISO.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell};
use crate::perl::Value;
use crate::pl::{self, arg, n, re, ret};

/// `\$buff` of a `my` variable kept in a cell.
fn rf(c: &Cell) -> Value {
    Value::ScalarRef(c.clone())
}

fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// Extract information from an ISO 9660 disk image
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid ISO 9660 image
pub fn process_iso(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let buff = cell(Value::Undef);
    let mut tag_table_ptr = Value::Undef;

    // verify this is a valid ISO file
    pl::at(143);
    if !pl::method(it, &raf, "Seek", vec![n(32768), n(0)])?.truthy() {
        return ret(n(0));
    }

    while pl::num_eq(&raf_read_cell(it, &raf, &buff, n(2048))?, &n(2048)) {
        let b = buff.borrow().clone();
        if !re(r"^[\0-\x03\xff]CD001", "").is_match(&b) {
            break;
        }
        if !tag_table_ptr.truthy() {
            pl::method(it, &et, "SetFileType", vec![])?; // set the FileType tag
            call(it, "SetByteOrder", vec![pl::s("II")])?; // read little-endian values only
            tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::ISO::Main")])?;
        }
        let type_ = pl::unpack1("C", &b)?;
        let volume_descriptor_type = crate::runtime::lex_hash(it, "Image::ExifTool::ISO", "volumeDescriptorType");
        let desc = volume_descriptor_type.get(type_.to_bytes());
        let msg = pl::concat(&pl::concat(&pl::concat(&pl::concat(&pl::s("Volume descriptor type "), &type_), &pl::s(" (")), &desc), &pl::s(")\n"));
        pl::method(it, &et, "VPrint", vec![n(0), msg])?;
        if pl::num_eq(&type_, &n(255)) {
            break; // stop at terminator
        }
        if !tag_table_ptr.get(type_.to_bytes()).truthy() {
            continue;
        }
        let sub_table_ptr = call(it, "GetTagTable", vec![tag_table_ptr.get(type_.to_bytes()).path(&["SubDirectory", "TagTable"])])?;
        let dir_info2 = Value::new_hash();
        dir_info2.set("DataPt", rf(&buff));
        dir_info2.set("DataPos", pl::sub(&pl::method(it, &raf, "Tell", vec![])?, &n(2048)));
        dir_info2.set("DirStart", n(0));
        dir_info2.set("DirLen", pl::length(&b));
        pl::method(it, &et, "ProcessDirectory", vec![dir_info2, sub_table_ptr])?;
    }
    ret(n(if tag_table_ptr.truthy() { 1 } else { 0 }))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::ISO::ProcessISO", process_iso);
}
