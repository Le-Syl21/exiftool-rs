//! Image::ExifTool::iWork (Image/ExifTool/iWork.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read Apple iWork '09 XML+ZIP files. The file's `my` hashes
//! (%iWorkType, %mimeType) stay Perl data, read with `lex_hash`.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, perl_eval};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};
use crate::runtime;

const MODULE: &str = "Image::ExifTool::iWork";

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
// Generate a tag ID for this XML tag
// Inputs: 0) tag property name list ref
// Returns: tagID
fn get_tag_id_rs(props: &Value) -> Value {
    if re(r"^\w+:ID$", "").is_match(&props.at(-1)) {
        return n(0); // ignore ID tags
    }
    match re(r"^.*?:(.*)", "").m(&props.at(0)) {
        Some(m) => cap(&m, 1),
        None => props.at(0),
    }
}

pub fn get_tag_id(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(get_tag_id_rs(&arg(&a, 0)))
}

//------------------------------------------------------------------------------
// We found an XMP property name/value
// Inputs: 0) ExifTool object ref, 1) tag table ref
//         2) reference to array of XMP property names (last is current property)
//         3) property value, 4) attribute hash ref (not used here)
// Returns: 1 if valid tag was found
pub fn found_tag(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, tag_table_ptr, props, mut val) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    if props.is_empty() {
        return ret(n(0));
    }
    let verbose = pl::method(it, &et, "Options", vec![s("Verbose")])?;

    if pl::gt(&verbose, &n(1)) {
        pl::method(it, &et, "VPrint", vec![n(0), s("  | - Tag '"), pl::join("/", &props.items()), s("'\n")])?;
    }

    // un-escape XML character entities
    val = pl::call(it, "Image::ExifTool::XMP::UnescapeXML", vec![val])?;
    // convert from UTF8 to ExifTool Charset
    val = pl::method(it, &et, "Decode", vec![val, s("UTF8")])?;
    let tag = get_tag_id_rs(&props);
    if !tag.truthy() {
        return ret(n(0));
    }

    // add any unknown tags to table
    if !tag_table_ptr.get(tag.to_bytes()).truthy() {
        if verbose.truthy() {
            pl::method(it, &et, "VPrint", vec![n(0), cat(&[s("  [adding "), tag.clone(), s("]\n")])])?;
        }
        let info = Value::new_hash();
        info.set("Name", pl::ucfirst(&tag));
        pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), info])?;
    }
    // save the tag
    pl::method(it, &et, "HandleTag", vec![tag_table_ptr, tag, val])?;

    ret(n(1))
}

//------------------------------------------------------------------------------
// Extract information from an iWork file
// Inputs: 0) ExifTool object reference, 1) dirInfo reference
// Returns: 1
// Notes: Upon entry to this routine, the file type has already been verified
// as ZIP and the dirInfo hash contains a 'ZIP' Archive::Zip object reference
pub fn process_i_work(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    // try to determine the file type
    let handler = Value::Code(it.sub_ref("Image::ExifTool::ZIP::WarnProc"));
    pl::local_sig_warn(it, handler, |it| process_body(it, &et, &dir_info))?;
    ret(n(1))
}

/// Process_iWork with `local $SIG{'__WARN__'} = \&Image::ExifTool::ZIP::WarnProc`.
fn process_body(it: &mut Interp, et: &Value, dir_info: &Value) -> ER<()> {
    let zip = dir_info.get("ZIP");
    let mut type_ = Value::Undef;
    // (my $index: a scalar the index file's contents go into)
    let index = cell(Value::Undef);
    let mut index_file = Value::Undef;
    let i_work_type = runtime::lex_hash(it, MODULE, "iWorkType");
    let mime_type = runtime::lex_hash(it, MODULE, "mimeType");

    // trust type given by file extension if available
    let ext = et.get("FILE_EXT");
    if ext.truthy() {
        type_ = i_work_type.get(ext.to_bytes());
    }
    if !type_.truthy() {
        // read the index file
        let members = pl::method_list(it, &zip, "membersMatching", vec![s(r"^index\.(xml|apxl)$")])?;
        if !members.is_empty() {
            let r = pl::method_list(it, &zip, "contents", vec![members[0].clone()])?;
            *index.borrow_mut() = arg(&r, 0);
            let status = arg(&r, 1);
            if !status.truthy() {
                index_file = pl::method(it, &members[0], "fileName", vec![])?;
                let idx = index.borrow().clone();
                if let Some(m) = re(r"^\s*<\?xml version=[^<]+<(\w+:\w+)", "s").m(&idx) {
                    let t = i_work_type.get(cap(&m, 1).to_bytes());
                    if t.truthy() {
                        type_ = t;
                    }
                }
            }
        } else {
            let members = pl::method_list(it, &zip, "membersMatching", vec![s(r"(?i)^.*\.(pages|numbers|key)/Index.*")])?;
            if !members.is_empty() {
                let tmp = pl::method(it, &members[0], "fileName", vec![])?;
                if let Some(m) = re(r"\.(pages|numbers|key)", "i").m(&tmp) {
                    type_ = i_work_type.get(pl::uc(&cap(&m, 1)).to_bytes());
                }
            }
        }
        if !type_.truthy() {
            type_ = s("ZIP"); // assume ZIP by default
        }
    }
    let mt = mime_type.get(type_.to_bytes());
    pl::method(it, et, "SetFileType", vec![type_.clone(), mt])?;

    let members = pl::method_list(it, &zip, "members", vec![])?;
    let mut doc_num: i64 = 0;
    for member in members {
        // get filename of this ZIP member
        let file = pl::method(it, &member, "fileName", vec![])?;
        if file.is_undef() {
            continue;
        }
        pl::method(it, et, "VPrint", vec![n(0), cat(&[s("File: "), file.clone(), s("\n")])])?;
        // set the document number and extract ZIP tags
        doc_num += 1;
        et.set("DOC_NUM", n(doc_num));
        pl::call(it, "Image::ExifTool::ZIP::HandleMember", vec![et.clone(), member.clone()])?;

        // process only the index XML and JPEG thumbnail/preview files
        if !re(r"^(index\.(xml|apxl)|QuickLook/Thumbnail\.jpg|[^/]+/preview(-micro|-web)?.jpg)$", "i").is_match(&file) {
            continue;
        }
        // get the file contents if necessary
        // (CAREFUL! $buff MUST be local since we hand off a value ref to PreviewImage)
        let buff_pt: Value;
        if index_file.truthy() && index_file.to_bytes() == file.to_bytes() {
            // use the index file we already loaded
            buff_pt = Value::ScalarRef(index.clone());
        } else {
            let r = pl::method_list(it, &zip, "contents", vec![member.clone()])?;
            let (buff, status) = (arg(&r, 0), arg(&r, 1));
            if status.truthy() {
                pl::method(it, et, "Warn", vec![cat(&[s("Error extracting "), file.clone()])])?;
                continue;
            }
            buff_pt = pl::sref(buff);
        }
        // extract JPEG as PreviewImage (should only be QuickLook/Thumbnail.jpg)
        if re(r"\.jpg$", "").is_match(&file) {
            let t = match re(r"preview-(\w+)", "").m(&file) {
                Some(m) => {
                    if pl::str_eq(&cap(&m, 1), "web") {
                        "Other"
                    } else {
                        "Thumbnail"
                    }
                }
                None => "Preview",
            };
            pl::method(it, et, "FoundTag", vec![s(&format!("{t}Image")), buff_pt.clone()])?;
            continue;
        }
        // process "metadata" section of XML index file
        let b = cell(buff_pt.clone());
        let m1 = perl_eval(it, r"$$b =~ /<(\w+):metadata>/g ? ($1, pos $$b) : ()", MODULE, &[("b", &b)], Want::List)?;
        if m1.is_empty() {
            continue;
        }
        let ns = cell(arg(&m1, 0));
        let p1 = arg(&m1, 1);
        let m2 = perl_eval(it, r"$$b =~ m{</${ns}:metadata>}g ? (pos $$b) : ()", MODULE, &[("b", &b), ("ns", &ns)], Want::List)?;
        if m2.is_empty() {
            continue;
        }
        let p2 = arg(&m2, 0);
        // construct XML data from "metadata" section only
        let body = pl::substr(&buff_pt.deref(), p1.to_i64(), Some(p2.to_i64() - p1.to_i64()));
        pl::set_deref(&buff_pt, pl::concat(&s("<?xml version=\"1.0\"?>"), &body))?;
        let di = Value::new_hash();
        di.set("DataPt", buff_pt.clone());
        let len = pl::length(&buff_pt.deref());
        di.set("DirLen", len.clone());
        di.set("DataLen", len);
        let opts = Value::new_hash();
        opts.set("FoundProc", Value::Code(it.sub_ref("Image::ExifTool::iWork::FoundTag")));
        di.set("XMPParseOpts", opts);
        let tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![s("Image::ExifTool::iWork::Main")])?;
        pl::method(it, et, "ProcessDirectory", vec![di, tag_table_ptr])?;
        pl::set_deref(&buff_pt, Value::Undef)?; // (free memory now)
    }
    et.delete("DOC_NUM");
    Ok(())
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::iWork::GetTagID", get_tag_id);
    it.define_native("Image::ExifTool::iWork::FoundTag", found_tag);
    it.define_native("Image::ExifTool::iWork::Process_iWork", process_i_work);
}
