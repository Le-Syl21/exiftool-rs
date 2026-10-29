//! Image::ExifTool::OOXML (Image/ExifTool/OOXML.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read Office Open XML+ZIP files. The file's `my` variables (%isOOXML,
//! %fileType, %queuedAttrs, %queueAttrs, $vectorCount, @vectorVals) stay
//! Perl data, read and written with `lex_hash`, `lex_scalar` and
//! `lex_array`.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, perl_eval1};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};
use crate::runtime;

const MODULE: &str = "Image::ExifTool::OOXML";

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
// Returns: tagID and outtermost interesting namespace (or '' if no namespace)
fn get_tag_id_rs(props: &Value) -> ER<(Value, Value)> {
    let mut tag = Value::Undef;
    let mut namespace = Value::Undef;
    // (foreach autovivifies an undefined $props, the sub's own copy)
    pl::at(187);
    let list = if props.is_undef() { Vec::new() } else { pl::aderef(props)? };
    for prop in list {
        // split name into namespace and property name
        // (Note: namespace can be '' for property qualifiers)
        let (ns, nm) = match re(r"(.*?):(.*)", "").m(&prop) {
            Some(m) => (cap(&m, 1), cap(&m, 2)),
            None => (s(""), prop.clone()),
        };
        if pl::str_eq(&ns, "vt") {
            continue; // ignore 'vt' properties
        }
        if !tag.is_undef() {
            tag = pl::concat(&tag, &pl::ucfirst(&nm)); // add to tag name
        } else if !pl::str_eq(&prop, "Properties") && !pl::str_eq(&prop, "cp:coreProperties") && !pl::str_eq(&prop, "property") {
            tag = nm;
            // save namespace of first property to contribute to tag name
            if !namespace.truthy() {
                namespace = ns;
            }
        }
    }
    Ok((tag, if namespace.truthy() { namespace } else { s("") }))
}

pub fn get_tag_id(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (tag, ns) = get_tag_id_rs(&arg(&a, 0))?;
    Ok(vec![tag, ns])
}

//------------------------------------------------------------------------------
// We found an XMP property name/value
// Inputs: 0) ExifTool object ref, 1) tag table ref
//         2) reference to array of XMP property names (last is current property)
//         3) property value, 4) attribute hash ref (not used here)
// Returns: 1 if valid tag was found
pub fn found_tag(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, mut tag_table_ptr, props, mut val) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    pl::at(214);
    if pl::aderef(&props)?.is_empty() {
        return ret(n(0));
    }
    let verbose = pl::method(it, &et, "Options", vec![s("Verbose")])?;
    let queued_attrs = runtime::lex_hash(it, MODULE, "queuedAttrs");
    let queue_attrs = runtime::lex_hash(it, MODULE, "queueAttrs");
    let vector_count = runtime::lex_scalar(it, MODULE, "vectorCount");
    let vector_vals = runtime::lex_array(it, MODULE, "vectorVals");

    let mut tag = props.at(-1);
    if pl::gt(&verbose, &n(1)) {
        pl::method(it, &et, "VPrint", vec![n(0), s("  | - Tag '"), pl::join("/", &props.items()), s("'\n")])?;
    }

    // un-escape XML character entities
    val = pl::call(it, "Image::ExifTool::XMP::UnescapeXML", vec![val])?;
    // convert OOXML-escaped characters (eg. "_x0000d_" is a newline)
    let mut err = None;
    re(r"_x([0-9a-f]{4})_", "i").subst(&mut val, true, |m| {
        let h = u64::from_str_radix(&cap(m, 1).to_text(), 16).unwrap_or(0);
        match pl::call(it, "Image::ExifTool::PackUTF8", vec![n(h as i64)]) {
            Ok(v) => v.to_bytes().to_vec(),
            Err(e) => {
                err.get_or_insert(e);
                Vec::new()
            }
        }
    });
    if let Some(e) = err {
        return Err(e);
    }
    // convert from UTF8 to ExifTool Charset
    val = pl::method(it, &et, "Decode", vec![val, s("UTF8")])?;
    // queue this attribute for later if necessary
    if queue_attrs.get(tag.to_bytes()).truthy() {
        queued_attrs.set(tag.to_bytes(), val);
        return ret(n(0));
    }
    let ns;
    (tag, ns) = get_tag_id_rs(&props)?;

    if !tag.truthy() {
        // all properties are in ignored namespaces
        // so 'name' from our queued attributes for the tag
        let mut name = queued_attrs.get("name");
        if !name.truthy() {
            return ret(n(0));
        }
        re(r"(^| )([a-z])", "").subst(&mut name, true, |m| [&cap(m, 1).to_bytes()[..], &pl::uc(&cap(m, 2)).to_bytes()[..]].concat()); // start words with uppercase
        tag = name.clone();
        pl::tr(&mut tag, "-_a-zA-Z0-9", "", "dc");
        if pl::length(&tag).to_i64() == 0 {
            return ret(n(0));
        }
        if !tag_table_ptr.get(tag.to_bytes()).truthy() {
            let tag_info = Value::new_hash();
            tag_info.set("Name", tag.clone());
            tag_info.set("Description", name.clone());
            // format as a date/time value if type is 'vt:filetime'
            if pl::str_eq(&props.at(-1), "vt:filetime") {
                let g = Value::new_hash();
                g.set("2", s("Time"));
                tag_info.set("Groups", g);
                tag_info.set("Format", s("date"));
                tag_info.set("PrintConv", s("$self->ConvertDateTime($val)"));
            }
            if verbose.truthy() {
                pl::method(it, &et, "VPrint", vec![n(0), cat(&[s("  | [adding "), tag.clone(), s("]\n")])])?;
            }
            pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), tag_info])?;
        }
    } else if pl::str_eq(&tag, "xmlns") {
        // ignore namespaces (for now)
        return ret(n(0));
    } else if {
        let xmp_main = pl::pkg_hash(it, "Image::ExifTool::XMP::Main");
        let e = xmp_main.get(ns.to_bytes());
        pl::ref_of(&e) == "HASH" && e.get("SubDirectory").truthy()
    } {
        // use standard XMP table if it exists
        let xmp_main = pl::pkg_hash(it, "Image::ExifTool::XMP::Main");
        let table = xmp_main.get(ns.to_bytes()).get("SubDirectory").get("TagTable");
        // (no strict 'refs')
        if table.truthy() && !pl::pkg_hash(it, &table.to_text()).keys().is_empty() {
            tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![table])?;
        }
    } else if props.len() > 2 && props.items().iter().any(|p| re(r"^vt:vector$", "").is_match(p)) {
        // handle vector properties (accumulate as lists)
        if pl::str_eq(&props.at(-1), "vt:size") {
            *vector_count.borrow_mut() = val;
            if let Value::ArrayRef(v) = &vector_vals {
                v.borrow_mut().clear();
            }
            return ret(n(0));
        } else if pl::str_eq(&props.at(-1), "vt:baseType") {
            return ret(n(0)); // ignore baseType
        } else if vector_count.borrow().truthy() {
            let vc = pl::sub(&vector_count.borrow(), &n(1));
            *vector_count.borrow_mut() = vc.clone();
            if vc.truthy() {
                vector_vals.push(val);
                return ret(n(0));
            }
            if !vector_vals.is_empty() {
                let mut l = vector_vals.items();
                l.push(val);
                val = Value::new_array(l);
            }
            // Note: we will lose any improper-sized vector elements here
        }
    }
    // add any unknown tags to table
    if tag_table_ptr.get(tag.to_bytes()).truthy() {
        let tag_info = tag_table_ptr.get(tag.to_bytes());
        if pl::ref_of(&tag_info) == "HASH" {
            // reformat date/time values
            let mut fmt = tag_info.get("Format");
            if !fmt.truthy() {
                fmt = tag_info.get("Writable");
            }
            if !fmt.truthy() {
                fmt = s("");
            }
            if pl::str_eq(&fmt, "date") {
                val = pl::call(it, "Image::ExifTool::XMP::ConvertXMPDate", vec![val])?;
            }
        }
    } else {
        if verbose.truthy() {
            pl::method(it, &et, "VPrint", vec![n(0), cat(&[s("  [adding "), tag.clone(), s("]\n")])])?;
        }
        let info = Value::new_hash();
        info.set("Name", pl::ucfirst(&tag));
        pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), info])?;
    }
    // save the tag
    pl::method(it, &et, "HandleTag", vec![tag_table_ptr, tag, val])?;

    // start fresh for next tag
    *vector_count.borrow_mut() = Value::Undef;
    if let Value::HashRef(h) = &queued_attrs {
        h.borrow_mut().clear();
    }

    ret(n(1))
}

//------------------------------------------------------------------------------
// Extract information from an OOXML file
// Inputs: 0) ExifTool object reference, 1) dirInfo reference
// Returns: 1
// Notes: Upon entry to this routine, the file type has already been verified
// and the dirInfo hash contains 2 elements unique to this process proc:
//   MIME    - mime type of main document from "[Content_Types].xml"
//   ZIP     - reference to Archive::Zip object for this file
pub fn process_docx(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let zip = dir_info.get("ZIP");
    let tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![s("Image::ExifTool::OOXML::Main")])?;
    let mut mime = dir_info.get("MIME");
    if !mime.truthy() {
        mime = pl::pkg_hash(it, "Image::ExifTool::mimeType").get("DOCX");
    }
    let file_type_h = runtime::lex_hash(it, MODULE, "fileType");
    let is_ooxml = runtime::lex_hash(it, MODULE, "isOOXML");

    // set the file type ('DOCX' by default)
    let mut file_type = file_type_h.get(mime.to_bytes());
    if file_type.truthy() {
        // THMX is a special case because its contents.main MIME types is PPTX
        let ext = et.get("FILE_EXT");
        if pl::str_eq(&file_type, "PPTX") && ext.truthy() && pl::str_eq(&ext, "THMX") {
            file_type = s("THMX");
        }
    } else {
        pl::method(it, &et, "VPrint", vec![n(0), cat(&[s("Unrecognized MIME type: "), mime.clone(), s("\n")])])?;
        // get MIME type according to file extension
        file_type = et.get("FILE_EXT");
        // default to 'DOCX' if this isn't a known OOXML extension
        if !(file_type.truthy() && is_ooxml.get(file_type.to_bytes()).truthy()) {
            file_type = s("DOCX");
        }
    }
    pl::method(it, &et, "SetFileType", vec![file_type])?;

    // must catch all Archive::Zip warnings
    let handler = Value::Code(it.sub_ref("Image::ExifTool::ZIP::WarnProc"));
    pl::local_sig_warn(it, handler, |it| docx_members(it, &et, &zip, &tag_table_ptr))?;
    ret(n(1))
}

/// ProcessDOCX from `local $SIG{'__WARN__'} = \&Image::ExifTool::ZIP::WarnProc`
/// to its end.
fn docx_members(it: &mut Interp, et: &Value, zip: &Value, tag_table_ptr: &Value) -> ER<()> {
    // extract meta information from all files in ZIP "docProps" directory
    let mut doc_num: i64 = 0;
    pl::at(341);
    let members = pl::method_list(it, zip, "members", vec![])?;
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
        // process only XML and JPEG/WMF thumbnail images in "docProps" directory
        if !re(r"^docProps/(.*\.xml|(thumbnail\.(jpe?g|wmf)))$", "i").is_match(&file) {
            continue;
        }
        // get the file contents (CAREFUL! $buff MUST be local since we hand off a value ref)
        let r = pl::method_list(it, zip, "contents", vec![member.clone()])?;
        let (buff, status) = (arg(&r, 0), arg(&r, 1));
        if status.truthy() {
            pl::method(it, et, "Warn", vec![cat(&[s("Error extracting "), file.clone()])])?;
            continue;
        }
        let buff_c = cell(buff);
        let buff_ref = Value::ScalarRef(buff_c.clone());
        // extract docProps/thumbnail.(jpg|mwf) as PreviewImage|PreviewMWF
        if re(r"\.(jpe?g|wmf)$", "i").is_match(&file) {
            let tag = if re(r"\.wmf$", "i").is_match(&file) { "PreviewWMF" } else { "PreviewImage" };
            pl::method(it, et, "FoundTag", vec![s(tag), buff_ref])?;
            continue;
        }
        // process XML files (docProps/app.xml, docProps/core.xml, docProps/custom.xml)
        let len = pl::length(&buff_c.borrow());
        let di = Value::new_hash();
        di.set("DataPt", buff_ref.clone());
        di.set("DirLen", len.clone());
        // (skip over XML header if it exists)
        let start = perl_eval1(it, r"$buff =~ /<\?xml\s+.*?\?>/g ? pos($buff) : 0", MODULE, &[("buff", &buff_c)])?;
        di.set("DirStart", start);
        di.set("DataLen", len);
        let opts = Value::new_hash();
        opts.set("FoundProc", Value::Code(it.sub_ref("Image::ExifTool::OOXML::FoundTag")));
        di.set("XMPParseOpts", opts);
        pl::method(it, et, "ProcessDirectory", vec![di, tag_table_ptr.clone()])?;
        *buff_c.borrow_mut() = Value::Undef; // (free memory now)
    }
    et.delete("DOC_NUM");
    Ok(())
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::OOXML::GetTagID", get_tag_id);
    it.define_native("Image::ExifTool::OOXML::FoundTag", found_tag);
    it.define_native("Image::ExifTool::OOXML::ProcessDOCX", process_docx);
}
