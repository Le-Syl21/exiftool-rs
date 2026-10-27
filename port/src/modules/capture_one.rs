//! Image::ExifTool::CaptureOne (Image/ExifTool/CaptureOne.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read Capture One EIP and COS files.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

fn s(x: &str) -> Value {
    pl::s(x)
}

/// String interpolation: the parts concatenated.
fn cat(parts: &[Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

//------------------------------------------------------------------------------
// We found an XMP property name/value
// Inputs: 0) attribute list ref, 1) attr hash ref,
//         2) property name ref, 3) property value ref
// Returns: true if value was changed
pub fn handle_cos_attrs(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (attr_list, attrs, prop, val_pt) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    let mut changed = Value::Undef;
    if !pl::length(&val_pt.deref()).truthy() && !attrs.get("K").is_undef() && !attrs.get("V").is_undef() {
        pl::set_deref(&prop, attrs.get("K"))?;
        pl::set_deref(&val_pt, attrs.get("V"))?;
        // remove these attributes from the list
        let attrs_list = attr_list.items();
        if let Value::ArrayRef(r) = &attr_list {
            r.borrow_mut().clear();
        }
        for a in attrs_list {
            if pl::str_eq(&a, "K") || pl::str_eq(&a, "V") {
                attrs.delete(a.to_bytes());
            } else {
                attr_list.push(a);
            }
        }
        changed = n(1);
    }
    ret(changed)
}

//------------------------------------------------------------------------------
// We found a COS property name/value
// Inputs: 0) ExifTool object ref, 1) tag table ref
//         2) reference to array of XMP property names (last is current property)
//         3) property value, 4) attribute hash ref (not used here)
// Returns: 1 if valid tag was found
pub fn found_cos(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, tag_table_ptr, props, mut val) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));

    let tag = props.at(-1);
    if !tag_table_ptr.get(tag.to_bytes()).truthy() {
        pl::method(it, &et, "VPrint", vec![n(0), cat(&[s("  | [adding "), tag.clone(), s("]\n")])])?;
        let mut name = pl::ucfirst(&tag);
        pl::tr(&mut name, "-_a-zA-Z0-9", "", "dc");
        if !pl::length(&tag).truthy() {
            return ret(n(0));
        }
        let tag_info = Value::new_hash();
        tag_info.set("Name", tag.clone());
        // try formatting any tag with "Date" in the name as a date
        // (shouldn't affect non-date tags)
        if re(r"Date(?![a-z])", "").is_match(&name) {
            let g = Value::new_hash();
            g.set("2", s("Time"));
            tag_info.set("Groups", g);
            tag_info.set("ValueConv", s("Image::ExifTool::XMP::ConvertXMPDate($val,1)"));
            tag_info.set("PrintConv", s("$self->ConvertDateTime($val)"));
        }
        pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), tag_info])?;
    }
    // convert from UTF8 to ExifTool Charset
    val = pl::method(it, &et, "Decode", vec![val, s("UTF8")])?;
    // un-escape XML character entities
    val = pl::call(it, "Image::ExifTool::XMP::UnescapeXML", vec![val])?;
    pl::method(it, &et, "HandleTag", vec![tag_table_ptr, tag, val])?;
    ret(n(0))
}

//------------------------------------------------------------------------------
// Extract information from a COS file
// Inputs: 0) ExifTool object reference, 1) dirInfo reference
// Returns: 1 on success, 0 if this wasn't a valid XML file
pub fn process_cos(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));

    // process using XMP module, but override handling of attributes and tags
    let opts = Value::new_hash();
    opts.set("AttrProc", Value::Code(it.sub_ref("Image::ExifTool::CaptureOne::HandleCOSAttrs")));
    opts.set("FoundProc", Value::Code(it.sub_ref("Image::ExifTool::CaptureOne::FoundCOS")));
    dir_info.set("XMPParseOpts", opts);
    let tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![s("Image::ExifTool::CaptureOne::Main")])?;
    let success = pl::method(it, &et, "ProcessDirectory", vec![dir_info.clone(), tag_table_ptr])?;
    dir_info.delete("XMLParseArgs");
    ret(success)
}

//------------------------------------------------------------------------------
// Extract information from a CaptureOne EIP file
// Inputs: 0) ExifTool object reference, 1) dirInfo reference
// Returns: 1
// Notes: Upon entry to this routine, the file type has already been verified
// and the dirInfo hash contains a ZIP element unique to this process proc:
//   ZIP     - reference to Archive::Zip object for this file
pub fn process_eip(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let zip = dir_info.get("ZIP");

    pl::method(it, &et, "SetFileType", vec![s("EIP")])?;

    // must catch all Archive::Zip warnings
    let handler = Value::Code(it.sub_ref("Image::ExifTool::ZIP::WarnProc"));
    pl::local_sig_warn(it, handler, |it| process_eip_body(it, &et, &zip))
}

/// ProcessEIP after `local $SIG{'__WARN__'} = \&Image::ExifTool::ZIP::WarnProc`.
fn process_eip_body(it: &mut Interp, et: &Value, zip: &Value) -> ER<Vec<Value>> {
    let (mut file, mut member) = (Value::Undef, Value::Undef);
    let mut parse_file: Vec<Value> = Vec::new(); // (keys of %parseFile, all set to 1)
    // find all manifest files
    let mut members = pl::method_list(it, zip, "membersMatching", vec![s(r"^manifest\d*.xml$")])?;
    // and choose the one with the highest version number (any better ideas?)
    while !members.is_empty() {
        let m = members.remove(0);
        let f = pl::method(it, &m, "fileName", vec![])?;
        if file.truthy() && file.to_bytes() > f.to_bytes() {
            continue;
        }
        member = m;
        file = f;
    }
    // get file names from our chosen manifest file
    if member.truthy() {
        let r = pl::method_list(it, zip, "contents", vec![member.clone()])?;
        let (buff, status) = (arg(&r, 0), arg(&r, 1));
        if !status.truthy() {
            let mut found_image = false;
            for c in re(r"<(RawPath|SettingsPath)>(.*?)</\1>", "s").each(&buff) {
                file = cap(&c, 2);
                if !re(r"\.(cos|iiq|jpe?g|tiff?)$", "i").is_match(&file) {
                    continue;
                }
                // set flag to parse this file
                if !parse_file.iter().any(|f| f.to_bytes() == file.to_bytes()) {
                    parse_file.push(file.clone());
                }
                if !re(r"\.cos$", "i").is_match(&file) {
                    found_image = true;
                }
            }
            // ignore manifest unless it contained a valid image
            if !found_image {
                parse_file.clear();
            }
        }
    }
    // extract meta information from embedded files
    let mut doc_num: i64 = 0;
    let members = pl::method_list(it, zip, "members", vec![])?; // get all members
    for member in members {
        // get filename of this ZIP member
        file = pl::method(it, &member, "fileName", vec![])?;
        if file.is_undef() {
            continue;
        }
        pl::method(it, et, "VPrint", vec![n(0), cat(&[s("File: "), file.clone(), s("\n")])])?;
        // set the document number and extract ZIP tags
        doc_num += 1;
        et.set("DOC_NUM", n(doc_num));
        pl::call(it, "Image::ExifTool::ZIP::HandleMember", vec![et.clone(), member.clone()])?;
        if !parse_file.is_empty() {
            if !parse_file.iter().any(|f| f.to_bytes() == file.to_bytes()) {
                continue;
            }
        } else {
            // reading the manifest didn't work, so look for image files in the
            // root directory and .cos files in the CaptureOne directory
            if !re(r"^([^/]+\.(iiq|jpe?g|tiff?)|CaptureOne/.*\.cos)$", "i").is_match(&file) {
                continue;
            }
        }
        // extract the contents of the file
        // Note: this could use a LOT of memory here for RAW images...
        let r = pl::method_list(it, zip, "contents", vec![member.clone()])?;
        let (buff, status) = (arg(&r, 0), arg(&r, 1));
        if status.truthy() {
            pl::method(it, et, "Warn", vec![cat(&[s("Error extracting "), file.clone()])])?;
            continue;
        }
        if re(r"\.cos$", "i").is_match(&file) {
            // process Capture One Settings files
            let len = pl::length(&buff);
            let di = Value::new_hash();
            di.set("DataPt", pl::sref(buff));
            di.set("DirLen", len.clone());
            di.set("DataLen", len);
            process_cos(it, vec![et.clone(), di], Want::Scalar)?;
        } else {
            // set HtmlDump error if necessary because it doesn't work with embedded files
            let hd = et.get("HTML_DUMP");
            if hd.truthy() {
                hd.set("Error", s("Sorry, can't dump images embedded in ZIP files"));
            }
            // process IIQ, JPEG and TIFF images
            let opts = Value::new_hash();
            opts.set("ReEntry", n(1));
            pl::method(it, et, "ExtractInfo", vec![pl::sref(buff), opts])?;
        }
    }
    et.delete("DOC_NUM");
    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::CaptureOne::HandleCOSAttrs", handle_cos_attrs);
    it.define_native("Image::ExifTool::CaptureOne::FoundCOS", found_cos);
    it.define_native("Image::ExifTool::CaptureOne::ProcessCOS", process_cos);
    it.define_native("Image::ExifTool::CaptureOne::ProcessEIP", process_eip);
}
