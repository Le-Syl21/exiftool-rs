//! Image::ExifTool::XMP2 (Image/ExifTool/XMP2.pl), ported sub by sub (see docs/port/PORTING.md).
//!
//! Additional XMP namespace definitions. The tables are data; these are the
//! file's three subs (package Image::ExifTool::XMP).

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::perl::Value;
use crate::pl::{self, arg};

/// We found a SEAL property name/value
/// Inputs: 0) ExifTool ref, 1) tag table ref, 2) xmp property list ref
///         3) property value, 4) attribute hash ref
/// Returns: 1 if valid tag was found
pub fn found_seal(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, tag_table_ptr, props, val, attrs) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4));
    // remove 'seal' container property from name
    pl::at(1911);
    let mut seal_props = pl::aderef(&props)?;
    if !seal_props.is_empty() && pl::str_eq(&seal_props[0], "seal") {
        seal_props.remove(0);
    }
    it.call_named("Image::ExifTool::XMP::FoundXMP", vec![et, tag_table_ptr, Value::new_array(seal_props), val, attrs], want)
}

/// Process SEAL XML
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_seal(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let found = Value::Code(it.sub_ref("Image::ExifTool::XMP::FoundSEAL"));
    dir_info.get_or_new_hash("XMPParseOpts").set("FoundProc", found);
    it.call_named("Image::ExifTool::XMP::ProcessXMP", vec![et, dir_info, tag_table_ptr], want)
}

/// Generate crd tags
/// Inputs: 0) tag table ref
pub fn init_crd(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let tag_table_ptr = arg(&a, 0);
    // import tags from CRS namespace
    let crs_table = pl::call(it, "Image::ExifTool::GetTagTable", vec![pl::s("Image::ExifTool::XMP::crs")])?;
    for tag in pl::call_list(it, "Image::ExifTool::TagTableKeys", vec![crs_table.clone()])? {
        let crs_info = crs_table.get(tag.to_bytes());
        pl::at(1938);
        let tag_info = pl::hderef(&crs_info)?.hash_copy();
        tag_table_ptr.set(tag.to_bytes(), tag_info.clone());
        if crs_info.get("Groups").truthy() {
            let groups = Value::new_hash();
            groups.set("0", pl::s("XMP"));
            groups.set("1", pl::s("XMP-crd"));
            groups.set("2", crs_info.get("Groups").get("2"));
            tag_info.set("Groups", groups);
        }
    }
    pl::ret_nothing(want)
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::XMP::FoundSEAL", found_seal);
    it.define_native("Image::ExifTool::XMP::ProcessSEAL", process_seal);
    it.define_native("Image::ExifTool::XMP::Init_crd", init_crd);
}
