//! Image::ExifTool::ITC (Image/ExifTool/ITC.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell};
use crate::perl::Value;
use crate::pl::{self, arg, n, ret};

fn val(c: &Cell) -> Value {
    c.borrow().clone()
}

/// `\$var` of a `my` variable kept in a cell.
fn rf(c: &Cell) -> Value {
    Value::ScalarRef(c.clone())
}

/// `$raf->Read($buff, $len) == $len`
fn read_eq(it: &mut Interp, raf: &Value, buff: &Cell, len: &Value) -> ER<bool> {
    let r = raf_read_cell(it, raf, buff, len.clone())?;
    Ok(pl::num_eq(&r, len))
}

fn seek(it: &mut Interp, raf: &Value, pos: Value, whence: i64) -> ER<bool> {
    Ok(pl::method(it, raf, "Seek", vec![pos, n(whence)])?.truthy())
}

/// Process an iTunes Cover Flow (ITC) file
/// Inputs: 0) ExifTool object reference, 1) Directory information reference
/// Returns: 1 on success, 0 if this wasn't a valid ITC file
pub fn process_itc(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let mut rtn_val = n(0);
    let buff = cell(Value::Undef);
    let mut err = Value::Undef;
    let (mut pos, mut tag_table_ptr);

    // loop through all blocks in this image
    loop {
        // read the block header
        let nr = raf_read_cell(it, &raf, &buff, n(8))?;
        if !pl::num_eq(&nr, &n(8)) {
            // no error if we reached the EOF normally
            if !nr.truthy() {
                err = Value::Undef;
            }
            break;
        }
        let u = pl::unpack("Na4", &val(&buff))?;
        let (mut size, tag) = (arg(&u, 0), arg(&u, 1));
        if rtn_val.truthy() {
            if !(pl::ge(&size, &n(8)) && pl::lt(&size, &Value::Int(0x80000000))) {
                break;
            }
        } else {
            // check to be sure this is a valid ITC image
            // (first block must be 'itch')
            if !pl::str_eq(&tag, "itch") {
                break;
            }
            if !(pl::ge(&size, &n(0x1c)) && pl::lt(&size, &n(0x10000))) {
                break;
            }
            pl::method(it, &et, "SetFileType", vec![])?;
            pl::call(it, "Image::ExifTool::SetByteOrder", vec![pl::s("MM")])?;
            rtn_val = n(1); // this is an ITC file
            err = n(1); // format error unless we read to EOF
        }
        if pl::str_eq(&tag, "itch") {
            pos = pl::method(it, &raf, "Tell", vec![])?;
            size = pl::sub(&size, &n(8)); // size of remaining data in block
            if !read_eq(it, &raf, &buff, &size)? {
                break;
            }
            // extract header information
            let dir_info2 = Value::new_hash();
            dir_info2.set("DirName", pl::s("ITC Header"));
            dir_info2.set("DataPt", rf(&buff));
            dir_info2.set("DataPos", pos.clone());
            let tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![pl::s("Image::ExifTool::ITC::Header")])?;
            pl::method(it, &et, "ProcessDirectory", vec![dir_info2, tag_table_ptr])?;
        } else if pl::str_eq(&tag, "item") {
            // don't want to read the entire item data (includes image)
            if !pl::gt(&size, &n(12)) {
                break;
            }
            if !read_eq(it, &raf, &buff, &n(4))? {
                break;
            }
            let mut len = pl::unpack1("N", &val(&buff))?;
            if !(pl::ge(&len, &n(0xd0)) && pl::le(&len, &size)) {
                break;
            }
            size = pl::sub(&size, &len); // size of data after item header
            len = pl::sub(&len, &n(12)); // length of remaining item header
            // read in 4-byte blocks until we find the null terminator
            // (this is just a guess about how to parse this variable-length part)
            while pl::ge(&len, &n(4)) {
                if !read_eq(it, &raf, &buff, &n(4))? {
                    break;
                }
                len = pl::sub(&len, &n(4));
                if &val(&buff).to_bytes()[..] == b"\0\0\0\0" {
                    break;
                }
            }
            if pl::lt(&len, &n(4)) {
                break;
            }
            pos = pl::method(it, &raf, "Tell", vec![])?;
            if !read_eq(it, &raf, &buff, &len)? {
                break;
            }
            if !(pl::ge(&len, &n(0xb4)) && pl::str_eq(&pl::substr(&val(&buff), 0xb0, Some(4)), "data")) {
                pl::method(it, &et, "Warn", vec![pl::s("Parsing error. Please submit this ITC file for testing")])?;
                break;
            }
            let dir_info2 = Value::new_hash();
            dir_info2.set("DirName", pl::s("ITC Item"));
            dir_info2.set("DataPt", rf(&buff));
            dir_info2.set("DataPos", pos.clone());
            tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![pl::s("Image::ExifTool::ITC::Item")])?;
            pl::method(it, &et, "ProcessDirectory", vec![dir_info2, tag_table_ptr.clone()])?;
            // extract embedded image
            pos = pl::add(&pos, &len);
            if pl::gt(&size, &n(0)) {
                tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![pl::s("Image::ExifTool::ITC::Main")])?;
                let tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), pl::s("data")])?;
                let image = pl::method(it, &et, "ExtractBinary", vec![pos.clone(), size.clone(), tag_info.get("Name")])?;
                pl::method(it, &et, "FoundTag", vec![tag_info, pl::sref(image)])?;
                // skip the rest of the block if necessary
                if !seek(it, &raf, pl::add(&pos, &size), 0)? {
                    break;
                }
            } else if pl::lt(&size, &n(0)) {
                break;
            }
        } else {
            let msg = pl::concat(&pl::concat(&pl::s("Unknown "), &tag), &pl::concat(&pl::concat(&pl::s(" block ("), &size), &pl::s(" bytes)\n")));
            pl::method(it, &et, "VPrint", vec![n(0), msg])?;
            if !seek(it, &raf, pl::sub(&size, &n(8)), 1)? {
                break;
            }
        }
    }
    if err.truthy() {
        pl::method(it, &et, "Warn", vec![pl::s("ITC file format error")])?;
    }
    ret(rtn_val)
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::ITC::ProcessITC", process_itc);
}
