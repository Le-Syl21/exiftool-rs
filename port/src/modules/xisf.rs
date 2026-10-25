//! Image::ExifTool::XISF (Image/ExifTool/XISF.pm), ported sub by sub (see docs/port/PORTING.md).

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

/// Handle properties in XISF metadata structures
/// Inputs: 0) attribute list ref, 1) attr hash ref,
///         2) property name ref, 3) property value ref
/// Returns: true if value was changed
pub fn handle_xisf_attrs(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (attr_list, attrs, prop, val_pt) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    if attrs.get("id").is_undef() {
        return ret(n(0));
    }
    let mut changed = Value::Undef;
    // use "id" as the tag name, "value" as the value, and ignore "type"
    pl::set_deref(&prop, attrs.get("id"))?;
    let mut p = prop.deref();
    if re(r"^XISF:", "").subst(&mut p, false, |_| Vec::new()) > 0 {
        // remove XISF namespace
        pl::set_deref(&prop, p)?;
    }
    if !attrs.get("value").is_undef() {
        pl::set_deref(&val_pt, attrs.get("value"))?;
        changed = n(1);
    }
    let attrs_l = attr_list.items();
    if let Value::ArrayRef(r) = &attr_list {
        r.borrow_mut().clear();
    }
    for a in attrs_l {
        if a.eq_str("id") || a.eq_str("value") || a.eq_str("type") {
            attrs.delete(a.to_bytes());
        } else {
            attr_list.push(a);
        }
    }
    ret(changed)
}

/// Read information in a XISF document
/// Inputs: 0) ExifTool ref, 1) dirInfo ref
/// Returns: 1 on success, 0 if this wasn't a valid XISF file
pub fn process_xisf(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let buff = cell(Value::Undef);

    if !(pl::num_eq(&raf_read_cell(it, &raf, &buff, n(16))?, &n(16)) && re(r"^XISF0100", "").is_match(&buff.borrow())) {
        return ret(n(0));
    }
    pl::method(it, &et, "SetFileType", vec![])?;
    call(it, "SetByteOrder", vec![pl::s("II")])?;
    let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::XISF::Main")])?;
    let hdr_len = call(it, "Get32u", vec![rf(&buff), n(8)])?;
    if !pl::num_eq(&raf_read_cell(it, &raf, &buff, hdr_len.clone())?, &hdr_len) {
        pl::method(it, &et, "Warn", vec![pl::s("Error reading XISF header")])?;
        return ret(n(1));
    }
    let b = buff.borrow().clone();
    pl::method(it, &et, "FoundTag", vec![pl::s("XML"), b])?;
    let dir_info2 = Value::new_hash();
    dir_info2.set("DataPt", rf(&buff));
    let ignore = Value::new_hash();
    ignore.set("xisf", n(1));
    ignore.set("Metadata", n(1));
    ignore.set("Property", n(1));
    dir_info2.set("IgnoreProp", ignore);
    let opts = Value::new_hash();
    opts.set("AttrProc", Value::Code(it.sub_ref("Image::ExifTool::XISF::HandleXISFAttrs")));
    dir_info2.set("XMPParseOpts", opts);
    pl::call(it, "Image::ExifTool::XMP::ProcessXMP", vec![et.clone(), dir_info2, tag_table_ptr])?;
    let geo = et.path(&["VALUE", "ImageGeometry"]);
    if geo.truthy() {
        let p = pl::split(&re(":", ""), &geo, 4);
        let (w, h, nn) = (arg(&p, 0), arg(&p, 1), arg(&p, 2));
        pl::method(it, &et, "FoundTag", vec![pl::s("ImageWidth"), w])?;
        pl::method(it, &et, "FoundTag", vec![pl::s("ImageHeight"), h])?;
        pl::method(it, &et, "FoundTag", vec![pl::s("NumPlanes"), nn])?;
    }
    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::XISF::HandleXISFAttrs", handle_xisf_attrs);
    it.define_native("Image::ExifTool::XISF::ProcessXISF", process_xisf);
}
