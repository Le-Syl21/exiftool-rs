//! Image::ExifTool::ICO (Image/ExifTool/ICO.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read Windows ICO (icon) and CUR (cursor) files.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::raf_read;
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

fn s(x: &str) -> Value {
    pl::s(x)
}

//------------------------------------------------------------------------------
// Process ICO/CUR file
// Inputs: 0) ExifTool ref, 1) dirInfo ref
// Returns: 1 on success, 0 if this wasn't a valid ICO/CUR file
pub fn process_ico(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let mut buff = Value::Undef;
    // verify this is a valid ICO/CUR file
    if !pl::num_eq(&raf_read(it, &raf, &mut buff, n(6))?, &n(6)) {
        return ret(n(0));
    }
    let Some(m) = re(r"^\0\0([\x01\x02])\0[^0]\0", "s").m(&buff) else {
        return ret(n(0));
    };
    // (note: have seen cursor files in the wild with an 0x01 here,
    // but SetFileType will use the .cur extension to identify these)
    let ft = if pl::str_eq(&cap(&m, 1), "\x01") { "ICO" } else { "CUR" };
    pl::method(it, &et, "SetFileType", vec![s(ft)])?;
    pl::call(it, "Image::ExifTool::SetByteOrder", vec![s("II")])?;
    let tag_tbl = pl::call(it, "Image::ExifTool::GetTagTable", vec![s("Image::ExifTool::ICO::Main")])?;
    let num = pl::call(it, "Image::ExifTool::Get16u", vec![pl::sref(buff.clone()), n(4)])?;
    pl::method(it, &et, "HandleTag", vec![tag_tbl.clone(), n(4), num.clone()])?;
    let mut i: i64 = 0;
    while pl::lt(&n(i), &num) {
        if !pl::num_eq(&raf_read(it, &raf, &mut buff, n(16))?, &n(16)) {
            pl::method(it, &et, "Warn", vec![s("Truncated file")])?;
            break;
        }
        let dc = pl::add(&et.get("DOC_COUNT"), &n(1));
        et.set("DOC_COUNT", dc.clone());
        et.set("DOC_NUM", dc);
        pl::method(it, &et, "HandleTag", vec![tag_tbl.clone(), n(6), buff.clone()])?;
        i += 1;
    }
    et.delete("DOC_NUM");
    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::ICO::ProcessICO", process_ico);
}
