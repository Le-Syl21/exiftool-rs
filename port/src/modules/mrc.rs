//! Image::ExifTool::MRC (Image/ExifTool/MRC.pm), ported sub by sub (see docs/port/PORTING.md).

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

/// Extract metadata from a MRC image
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid MRC file
pub fn process_mrc(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let buff = cell(Value::Undef);
    let mut tag_table_ptr;

    // verify this is a valid MRC file
    if !pl::num_eq(&raf_read_cell(it, &raf, &buff, n(1024))?, &n(1024)) {
        return ret(n(0));
    }
    // validate axes, "MAP" file type and machine stamp
    if !re(r"^.{64}[\x01\x02\x03]\0\0\0[\x01\x02\x03]\0\0\0[\x01\x02\x03]\0\0\0.{132}MAP[\0 ](\x44\x44|\x44\x41|\x11\x11)\0\0", "s").is_match(&buff.borrow()) {
        return ret(n(0));
    }

    pl::method(it, &et, "SetFileType", vec![])?;
    call(it, "SetByteOrder", vec![pl::s("II")])?;
    let dir_info2 = Value::new_hash();
    dir_info2.set("DataPt", rf(&buff));
    dir_info2.set("DirStart", n(0));
    dir_info2.set("DirLen", pl::length(&buff.borrow()));
    tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::MRC::Main")])?;
    pl::method(it, &et, "ProcessDirectory", vec![dir_info2.clone(), tag_table_ptr.clone()])?;

    // (I don't have any samples with extended headers for testing, so these are not yet decoded)
    if et.get("ExtendedHeaderSize").truthy() && re(r"^FEI[12]", "").is_match(&et.get("ExtendedHeaderType")) {
        // read metadata size
        if !(pl::num_eq(&raf_read_cell(it, &raf, &buff, n(4))?, &n(4)) && pl::method(it, &raf, "Seek", vec![n(-4), n(1)])?.truthy()) {
            pl::method(it, &et, "Warn", vec![pl::s("Error reading extended header")])?;
            return ret(n(1));
        }
        let size = call(it, "Get32u", vec![rf(&buff), n(0)])?;
        if pl::gt(&pl::mul(&size, &et.get("ImageDepth")), &et.get("ExtendedHeaderSize")) {
            pl::method(it, &et, "Warn", vec![pl::s("Corrupted extended header")])?;
            return ret(n(1));
        }
        dir_info2.set("DirLen", size.clone());
        tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::MRC::FEI12")])?;
        let mut i = n(0);
        loop {
            dir_info2.set("DataPos", pl::method(it, &raf, "Tell", vec![])?);
            if !pl::num_eq(&raf_read_cell(it, &raf, &buff, size.clone())?, &size) {
                pl::method(it, &et, "Warn", vec![pl::concat(&pl::s("Error reading extended header "), &i)])?;
                break;
            }
            pl::method(it, &et, "ProcessDirectory", vec![dir_info2.clone(), tag_table_ptr.clone()])?;
            i = pl::add(&i, &n(1));
            if pl::ge(&i, &et.get("ImageDepth")) {
                break;
            }
            if !et.path(&["OPTIONS", "ExtractEmbedded"]).truthy() {
                pl::method(it, &et, "Warn", vec![pl::s("Use the ExtractEmbedded option to read metadata for all frames"), n(3)])?;
                break;
            }
            let dc = pl::add(&et.get("DOC_COUNT"), &n(1));
            et.set("DOC_COUNT", dc.clone());
            et.set("DOC_NUM", dc);
        }
        et.delete("DOC_NUM");
    }

    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::MRC::ProcessMRC", process_mrc);
}
