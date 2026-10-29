//! Image::ExifTool::BMP (Image/ExifTool/BMP.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell};
use crate::perl::Value;
use crate::pl::{self, arg, n, re, ret};
use crate::modules::quick_time_stream::check_invocant;

/// `\$buff` of a `my` variable kept in a cell.
fn rf(c: &Cell) -> Value {
    Value::ScalarRef(c.clone())
}

fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// Extract metadata from a BMP image
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid BMP file
pub fn process_bmp(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let buff = cell(Value::Undef);
    let tag_table_ptr;

    // verify this is a valid BMP file
    pl::at(261);
    check_invocant(it, &raf, "Read")?;
    if !pl::num_eq(&raf_read_cell(it, &raf, &buff, n(18))?, &n(18)) {
        return ret(n(0));
    }
    if !re(r"^BM", "").is_match(&buff.borrow()) {
        return ret(n(0));
    }
    call(it, "SetByteOrder", vec![pl::s("II")])?;
    let len = call(it, "Get32u", vec![rf(&buff), n(14)])?;
    // len = v1:12, v4:108, v5:124
    if !(pl::num_eq(&len, &n(12)) || pl::num_eq(&len, &n(16)) || (pl::ge(&len, &n(40)) && pl::lt(&len, &n(1000000)))) {
        return ret(n(0));
    }
    if !(pl::method(it, &raf, "Seek", vec![n(-4), n(1)])?.truthy() && pl::num_eq(&raf_read_cell(it, &raf, &buff, len.clone())?, &len)) {
        return ret(n(0));
    }
    pl::method(it, &et, "SetFileType", vec![])?; // set the FileType tag
    //
    // process the BMP header
    //
    let dir_info2 = Value::new_hash();
    dir_info2.set("DataPt", rf(&buff));
    dir_info2.set("DirStart", n(0));
    dir_info2.set("DirLen", pl::length(&buff.borrow()));
    if pl::num_eq(&len, &n(12)) || pl::num_eq(&len, &n(16)) || pl::num_eq(&len, &n(64)) {
        // old OS/2 format BMP
        tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::BMP::OS2")])?;
    } else {
        tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::BMP::Main")])?;
    }
    pl::method(it, &et, "ProcessDirectory", vec![dir_info2, tag_table_ptr])?;
    //
    // extract any embedded images
    //
    let extra_table = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::BMP::Extra")])?;
    let comp = et.get("BMPCompression");
    if comp.truthy() && et.get("BMPImageLength").truthy() && (pl::num_eq(&comp, &n(4)) || pl::num_eq(&comp, &n(5))) {
        let tag = pl::s(if pl::num_eq(&comp, &n(4)) { "EmbeddedJPG" } else { "EmbeddedPNG" });
        let pos = pl::method(it, &raf, "Tell", vec![])?;
        let val = pl::method(it, &et, "ExtractBinary", vec![pos, et.get("BMPImageLength"), tag.clone()])?;
        if val.truthy() {
            pl::method(it, &et, "HandleTag", vec![extra_table.clone(), tag, val])?;
        }
    }
    //
    // process profile data if it exists (v5 header only)
    //
    if pl::num_eq(&len, &n(124)) && et.get("BMPProfileOffset").truthy() {
        let pos = pl::add(&et.get("BMPProfileOffset"), &n(14)); // (note the 14-byte shift!)
        let size = et.get("BMPProfileSize");
        if pl::method(it, &raf, "Seek", vec![pos.clone(), n(0)])?.truthy() && pl::num_eq(&raf_read_cell(it, &raf, &buff, size.clone())?, &size) {
            let tag;
            let mut b = buff.borrow().clone();
            if et.get("BMPColorSpace").to_bytes()[..] == b"LINK"[..] {
                re(r"\0+$", "").subst(&mut b, false, |_| Vec::new()); // remove null terminator(s)
                b = pl::method(it, &et, "Decode", vec![b, pl::s("Latin")])?; // convert from Latin
                *buff.borrow_mut() = b.clone();
                tag = pl::s("LinkedProfileName");
            } else {
                tag = pl::s("ICC_Profile");
            }
            pl::method(it, &et, "HandleTag", vec![extra_table, tag, b, pl::s("Size"), size, pl::s("DataPos"), pos])?;
        } else {
            pl::method(it, &et, "Warn", vec![pl::s("Error loading profile data"), n(1)])?;
        }
    }
    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::BMP::ProcessBMP", process_bmp);
}
