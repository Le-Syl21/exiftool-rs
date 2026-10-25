//! Image::ExifTool::Apple (Image/ExifTool/Apple.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::perl::Value;
use crate::pl::{self, arg, n, ret};

//------------------------------------------------------------------------------
/// Convert from binary PLIST format to a tag value we can use
/// Inputs: 0) binary plist data, 1) ExifTool ref
/// Returns: converted value
pub fn convert_plist(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (val, et) = (arg(&a, 0), arg(&a, 1));
    let dir_info = Value::new_hash();
    dir_info.set("DataPt", pl::sref(val));
    dir_info.set("NoVerboseDir", n(1));
    let old_order = pl::method(it, &et, "GetByteOrder", vec![])?;
    crate::runtime::require(it, "Image::ExifTool::PLIST")?;
    pl::call(it, "Image::ExifTool::PLIST::ProcessBinaryPLIST", vec![et.clone(), dir_info.clone()])?;
    let mut val = dir_info.get("Value");
    if pl::ref_of(&val) == "HASH" && !pl::method(it, &et, "Options", vec![pl::s("Struct")])?.truthy() {
        crate::runtime::require(it, "Image/ExifTool/XMPStruct.pl")?;
        val = pl::call(it, "Image::ExifTool::XMP::SerializeStruct", vec![et.clone(), val])?;
    }
    pl::method(it, &et, "SetByteOrder", vec![old_order])?;
    ret(val)
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Apple::ConvertPLIST", convert_plist);
}
