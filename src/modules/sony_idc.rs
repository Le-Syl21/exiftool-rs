//! Image::ExifTool::SonyIDC (Image/ExifTool/SonyIDC.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

//------------------------------------------------------------------------------
/// Extract all IDC preview images
/// Inputs: 0) ExifTool object ref
/// Returns: data for "IDCPreviewImage" tag (which I have never seen),
///          or undef if there was no preview in the SonyIDC IFD
pub fn extract_previews(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let et = arg(&a, 0);
    let mut i = n(1);
    let mut xtra = pl::s(" (1)");
    let mut preview = Value::Undef;
    // loop through all available IDC preview images in the order they were found
    loop {
        let key = pl::concat(&pl::s("IDCPreviewStart"), &xtra);
        if et.get("VALUE").get(key.to_bytes()).is_undef() {
            if !xtra.truthy() {
                break;
            }
            xtra = pl::s(""); // do the last tag extracted last
            continue;
        }
        // run through IDC preview images in the same order they were extracted
        let off = pl::method(it, &et, "GetValue", vec![key.clone(), pl::s("ValueConv")])?;
        if !off.truthy() {
            break;
        }
        let len = pl::method(it, &et, "GetValue", vec![pl::concat(&pl::s("IDCPreviewLength"), &xtra), pl::s("ValueConv")])?;
        if !len.truthy() {
            break;
        }
        // get stack version from number in group 1 name
        let grp1 = pl::method(it, &et, "GetGroup", vec![key.clone(), n(1)])?;
        if let Some(m) = re(r"(\d+)$", "").m(&grp1) {
            let tag = pl::concat(&pl::s("IDCPreviewImage"), &cap(&m, 1));
            let extra = pl::pkg_hash(it, "Image::ExifTool::Extra");
            if !extra.get(tag.to_bytes()).truthy() {
                let groups = Value::new_hash();
                groups.set("0", pl::s("Composite"));
                groups.set("1", pl::s("Composite"));
                groups.set("2", pl::s("Preview"));
                let ti = Value::new_hash();
                ti.set("Name", tag.clone());
                ti.set("Groups", groups);
                pl::call(it, "Image::ExifTool::AddTagToTable", vec![extra, tag.clone(), ti])?;
            }
            let v = pl::call(it, "Image::ExifTool::Exif::ExtractImage", vec![et.clone(), off, len, tag.clone()])?;
            let g = pl::method_list(it, &et, "GetGroup", vec![key])?;
            let mut args = vec![tag, v];
            args.extend(g);
            pl::method(it, &et, "FoundTag", args)?;
        } else {
            preview = pl::call(it, "Image::ExifTool::Exif::ExtractImage", vec![et.clone(), off, len, pl::s("IDCPreviewImage")])?;
        }
        // step to next set of tags unless we are done
        if !xtra.truthy() {
            break;
        }
        i = pl::add(&i, &n(1));
        xtra = pl::concat(&pl::concat(&pl::s(" ("), &i), &pl::s(")"));
    }
    ret(preview)
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::SonyIDC::ExtractPreviews", extract_previews);
}
