//! Image::ExifTool::Other (Image/ExifTool/Other.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read meta information from other uncommon formats (Portable FloatMap).

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::raf_read;
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

fn s(x: &str) -> Value {
    pl::s(x)
}

//------------------------------------------------------------------------------
// Extract information from a Portable FloatMap image
// Inputs: 0) ExifTool object reference, 1) dirInfo reference
// Returns: 1 on success, 0 if this wasn't a valid PFM file
pub fn process_pfm2(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let mut buff = Value::Undef;
    if !raf_read(it, &raf, &mut buff, n(256))?.truthy() {
        return ret(n(0));
    }
    let Some(m) = re(r"^(P[Ff])\x0a(\d+) (\d+)\x0a([-+0-9.]+)\x0a", "").m(&buff) else {
        return ret(n(0));
    };
    pl::method(it, &et, "SetFileType", vec![s("PFM"), s("image/x-pfm")])?;
    let tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![s("Image::ExifTool::Other::PFM")])?;
    pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), s("ColorSpace"), cap(&m, 1)])?;
    pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), s("ImageWidth"), cap(&m, 2)])?;
    pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), s("ImageHeight"), cap(&m, 3)])?;
    pl::method(it, &et, "HandleTag", vec![tag_table_ptr, s("ByteOrder"), cap(&m, 4)])?;
    // hack to set proper file description (extension is the same for Printer Font Metrics files)
    let static_vars = pl::pkg_hash(it, "Image::ExifTool::static_vars");
    static_vars.get_or_new_hash("OverrideFileDescription").set("PFM", s("Portable FloatMap"));
    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Other::ProcessPFM2", process_pfm2);
}
