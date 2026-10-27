//! Image::ExifTool::MPF (Image/ExifTool/MPF.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read Multi-Picture Format information

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, ret};

fn n(i: i64) -> Value {
    Value::Int(i)
}

fn s(x: &str) -> Value {
    Value::str(x)
}

/// `"$a$b..."`
fn cat(parts: &[&Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

/// Extract all MP images
/// Inputs: 0) ExifTool object ref
/// Returns: undef
pub fn extract_mp_images(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let et = arg(&a, 0);
    let ee = pl::method(it, &et, "Options", vec![s("ExtractEmbedded")])?;
    let save_binary = pl::method(it, &et, "Options", vec![s("Binary")])?;
    let mut did_preview = false;
    let mut xtra = Value::Undef;
    let mut i: i64 = 1;
    while xtra.truthy() || xtra.is_undef() {
        // run through MP images in the same order they were extracted
        xtra = if !et.get("VALUE").get(format!("MPImageStart ({i})")).is_undef() { s(&format!(" ({i})")) } else { s("") };
        let off = pl::method(it, &et, "GetValue", vec![cat(&[&s("MPImageStart"), &xtra]), s("ValueConv")])?;
        let len = pl::method(it, &et, "GetValue", vec![cat(&[&s("MPImageLength"), &xtra]), s("ValueConv")])?;
        if off.truthy() && len.truthy() {
            let typ = pl::method(it, &et, "GetValue", vec![cat(&[&s("MPImageType"), &xtra]), s("ValueConv")])?;
            let mut tag = s(&format!("MPImage{i}"));
            // store first "Large Thumbnail" as a PreviewImage
            if !did_preview && typ.truthy() && pl::num_eq(&ops::bit_and(&typ, &n(0x0f0000)), &n(0x010000)) {
                tag = s("PreviewImage");
                did_preview = true;
            }
            if ee.truthy() {
                pl::method(it, &et, "Options", vec![s("Binary"), n(1)])?;
            }
            let val = pl::call(it, "Image::ExifTool::Exif::ExtractImage", vec![et.clone(), off.clone(), len.clone(), tag.clone()])?;
            if ee.truthy() {
                pl::method(it, &et, "Options", vec![s("Binary"), save_binary.clone()])?;
            }
            if val.is_undef() {
                i += 1;
                continue;
            }
            let extra = pl::pkg_hash(it, "Image::ExifTool::Extra");
            if !extra.get(tag.to_bytes()).truthy() {
                let groups = Value::new_hash();
                groups.set("0", s("Composite"));
                groups.set("1", s("Composite"));
                groups.set("2", s("Preview"));
                let info = Value::new_hash();
                info.set("Name", tag.clone());
                info.set("Groups", groups);
                pl::call(it, "Image::ExifTool::AddTagToTable", vec![extra, tag.clone(), info])?;
            }
            let mut args = vec![tag.clone(), val.clone()];
            args.extend(pl::method_list(it, &et, "GetGroup", vec![cat(&[&s("MPImageStart"), &xtra])])?);
            pl::method(it, &et, "FoundTag", args)?;
            // extract information from MP images if ExtractEmbedded option used
            if ee.truthy() {
                let old_base = et.get("BASE");
                et.set("BASE", off.clone());
                et.set("DOC_NUM", n(i));
                let opts = Value::new_hash();
                opts.set("ReEntry", n(1));
                pl::method(it, &et, "ExtractInfo", vec![val.clone(), opts])?;
                et.delete("DOC_NUM");
                et.set("BASE", old_base);
            }
        }
        i += 1;
    }
    pl::ret_undef()
}

/// Process MP Entry list
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_mp_image_list(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let num = pl::int(&pl::div(&dir_info.get("DirLen"), &n(16))?); // (16 bytes per MP Entry)
    dir_info.set("DirLen", n(16));
    let mut success = Value::Undef;
    let old_g1 = et.get("SET_GROUP1");
    let mut i = n(0);
    while pl::lt(&i, &num) {
        et.set("SET_GROUP1", cat(&[&s("+"), &pl::add(&i, &n(1))]));
        success = pl::method(it, &et, "ProcessBinaryData", vec![dir_info.clone(), tag_table_ptr.clone()])?;
        dir_info.set("DirStart", pl::add(&dir_info.get("DirStart"), &n(16)));
        i = pl::add(&i, &n(1));
    }
    et.set("SET_GROUP1", old_g1);
    ret(success)
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::MPF::ExtractMPImages", extract_mp_images);
    it.define_native("Image::ExifTool::MPF::ProcessMPImageList", process_mp_image_list);
}
