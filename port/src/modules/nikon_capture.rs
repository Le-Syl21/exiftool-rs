//! Image::ExifTool::NikonCapture (Image/ExifTool/NikonCapture.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! The tag tables are data; these are the module's three subs: the writer
//! and the two readers of Nikon Capture data.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::perl::Value;
use crate::pl::{self, arg, ret};

fn n(i: i64) -> Value {
    Value::Int(i)
}

fn s(x: &str) -> Value {
    pl::s(x)
}

fn get32u(it: &mut Interp, data_pt: &Value, pos: Value) -> ER<Value> {
    pl::call(it, "Image::ExifTool::Get32u", vec![data_pt.clone(), pos])
}

fn set32u(it: &mut Interp, v: Value) -> ER<Value> {
    pl::call(it, "Image::ExifTool::Set32u", vec![v])
}

//------------------------------------------------------------------------------
/// write Nikon Capture data (ref 1)
/// Inputs: 0) ExifTool object reference, 1) reference to directory information
///         2) pointer to tag table
/// Returns: 1 on success
pub fn write_nikon_capture(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if !et.truthy() {
        return ret(n(1)); // allow dummy access to autoload this package
    }

    // no need to edit this information unless necessary
    let edit_dirs = et.get("EDIT_DIRS");
    if !(edit_dirs.get("MakerNotes").truthy() || edit_dirs.get("IPTC").truthy()) {
        return pl::ret_undef();
    }
    let data_pt = dir_info.get("DataPt");
    let dir_start = dir_info.get("DirStart");
    let mut dir_len = dir_info.get("DirLen");
    if pl::lt(&dir_len, &n(22)) {
        pl::method(it, &et, "Warn", vec![s("Short Nikon Capture Data"), n(1)])?;
        return pl::ret_undef();
    }
    // make sure the capture data is properly contained
    pl::call(it, "Image::ExifTool::SetByteOrder", vec![s("II")])?;
    let mut tag_id = get32u(it, &data_pt, dir_start.clone())?;
    // sometimes size includes 18 header bytes, and other times it doesn't (eg. ViewNX 2.1.1)
    let mut size = get32u(it, &data_pt, pl::add(&dir_start, &n(18)))?;
    let mut pad = pl::sub(&pl::sub(&dir_len, &size), &n(18));
    if !(pl::num_eq(&tag_id, &n(0x7a86a940)) && (pl::ge(&pad, &n(0)) || pl::num_eq(&pad, &n(-18)))) {
        pl::method(it, &et, "Warn", vec![s("Unrecognized Nikon Capture Data header")])?;
        return pl::ret_undef();
    }
    // determine if there is any data after this block
    if pl::gt(&pad, &n(0)) {
        pad = pl::substr(&pl::sderef(&data_pt)?, pl::add(&pl::add(&dir_start, &n(18)), &size).to_i64(), Some(pad.to_i64()));
        dir_len = pl::add(&size, &n(18));
    } else {
        pad = s("");
    }
    let mut out_buff = s("");
    let new_tags = pl::method(it, &et, "GetNewTagInfoHash", vec![tag_table_ptr.clone()])?;
    let dir_end = pl::add(&dir_start, &dir_len);

    // loop through all entries in the Nikon Capture data
    let mut pos = pl::add(&dir_start, &n(22));
    while pl::lt(&pl::add(&pos, &n(22)), &dir_end) {
        tag_id = get32u(it, &data_pt, pos.clone())?;
        size = pl::sub(&get32u(it, &data_pt, pl::add(&pos, &n(18)))?, &n(4));
        if pl::lt(&size, &n(0)) || pl::gt(&pl::add(&pl::add(&pos, &n(22)), &size), &dir_end) {
            break;
        }
        let tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag_id.clone()])?;
        let mut keep_existing = true;
        if tag_info.truthy() {
            let mut new_val = Value::Undef;
            if tag_info.get("SubDirectory").truthy() {
                // rewrite the subdirectory
                let subdir_info = Value::new_hash();
                subdir_info.set("DataPt", data_pt.clone());
                subdir_info.set("DirStart", pl::add(&pos, &n(22)));
                subdir_info.set("DirLen", size.clone());
                let sub_table = pl::call(it, "Image::ExifTool::GetTagTable", vec![tag_info.get("SubDirectory").get("TagTable")])?;
                // ignore minor errors in IPTC since there is typically trailing garbage
                let old_setting = pl::method(it, &et, "Options", vec![s("IgnoreMinorErrors")])?;
                if pl::re("IPTC", "").is_match(&tag_info.get("Name")) {
                    pl::method(it, &et, "Options", vec![s("IgnoreMinorErrors"), n(1)])?;
                }
                // rewrite the directory
                new_val = pl::method(it, &et, "WriteDirectory", vec![subdir_info, sub_table])?;
                // restore our original options
                pl::method(it, &et, "Options", vec![s("IgnoreMinorErrors"), old_setting])?;
            } else if new_tags.get(tag_id.to_bytes()).truthy() {
                // get new value for this tag if we are writing it
                let mut format = tag_info.get("Format");
                if !format.truthy() {
                    format = tag_info.get("Writable");
                }
                let old_val = pl::call(it, "Image::ExifTool::ReadValue", vec![data_pt.clone(), pl::add(&pos, &n(22)), format, n(1), size.clone()])?;
                let nv_hash = pl::method(it, &et, "GetNewValueHash", vec![tag_info.clone()])?;
                if pl::method(it, &et, "IsOverwriting", vec![nv_hash, old_val])?.truthy() {
                    let val = pl::method(it, &et, "GetNewValue", vec![tag_info.clone()])?;
                    if !val.is_undef() {
                        new_val = pl::call(it, "Image::ExifTool::WriteValue", vec![val, tag_info.get("Writable")])?;
                    }
                    if !new_val.is_undef() && pl::length(&new_val).truthy() {
                        et.set("CHANGED", pl::add(&et.get("CHANGED"), &n(1)));
                    } else {
                        new_val = Value::Undef;
                        let msg = pl::concat(&s("Can't delete "), &tag_info.get("Name"));
                        pl::method(it, &et, "Warn", vec![msg])?;
                    }
                }
            }
            if !new_val.is_undef() {
                keep_existing = false;
                if pl::length(&new_val).truthy() {
                    // write the new value
                    out_buff = pl::concat(&out_buff, &pl::substr(&pl::sderef(&data_pt)?, pos.to_i64(), Some(18)));
                    let l = set32u(it, pl::add(&pl::length(&new_val), &n(4)))?;
                    out_buff = pl::concat(&out_buff, &l);
                    out_buff = pl::concat(&out_buff, &new_val);
                }
                // (else don't write zero length information)
            }
        }
        if keep_existing {
            // rewrite the existing information
            out_buff = pl::concat(&out_buff, &pl::substr(&pl::sderef(&data_pt)?, pos.to_i64(), Some(pl::add(&n(22), &size).to_i64())));
        }
        pos = pl::add(&pos, &pl::add(&n(22), &size));
    }
    if !pl::num_eq(&pos, &dir_end) {
        if pl::num_eq(&pos, &pl::sub(&dir_end, &n(4))) {
            // it seems that sometimes (NX2) the main block size is wrong by 4 bytes
            // (did they forget to include the size word?)
            out_buff = pl::concat(&out_buff, &pl::substr(&pl::sderef(&data_pt)?, pos.to_i64(), Some(4)));
        } else {
            pl::method(it, &et, "Warn", vec![s("Nikon Capture Data improperly terminated"), n(1)])?;
            return pl::ret_undef();
        }
    }
    // add the header and return the new directory
    let head = pl::substr(&pl::sderef(&data_pt)?, dir_start.to_i64(), Some(18));
    let l = set32u(it, pl::add(&pl::length(&out_buff), &n(4)))?;
    ret(pl::concat(&pl::concat(&pl::concat(&head, &l), &out_buff), &pad))
}

//------------------------------------------------------------------------------
/// process Nikon Capture data (ref 1)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_nikon_capture_edit_versions(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let dir_start = dir_info.get("DirStart");
    let dir_len = dir_info.get("DirLen");
    let dir_end = pl::add(&dir_start, &dir_len);
    let verbose = pl::method(it, &et, "Options", vec![s("Verbose")])?;
    pl::call(it, "Image::ExifTool::SetByteOrder", vec![s("II")])?;
    if !pl::gt(&dir_len, &n(4)) {
        return ret(n(0));
    }
    let mut num = get32u(it, &data_pt, dir_start.clone())?;
    let mut pos = pl::add(&dir_start, &n(4));
    if verbose.truthy() {
        pl::method(it, &et, "VerboseDir", vec![s("NikonCaptureEditVersions"), num.clone()])?;
    }
    while num.truthy() {
        if pl::gt(&pl::add(&pos, &n(4)), &dir_end) {
            break;
        }
        let len = get32u(it, &data_pt, pos.clone())?;
        if pl::gt(&pl::add(&pl::add(&pos, &len), &n(4)), &dir_end) {
            break;
        }
        let dir_info = Value::new_hash();
        dir_info.set("DirName", s("NikonCapture"));
        dir_info.set("Parent", s("NikonCaptureEditVersions"));
        dir_info.set("DataPt", data_pt.clone());
        dir_info.set("DirStart", pl::add(&pos, &n(4)));
        dir_info.set("DirLen", len.clone());
        let c = pl::add(&et.get("DOC_COUNT"), &n(1));
        et.set("DOC_COUNT", c.clone());
        et.set("DOC_NUM", c);
        pl::method(it, &et, "ProcessDirectory", vec![dir_info, tag_table_ptr.clone()])?;
        num = pl::sub(&num, &n(1));
        pos = pl::add(&pos, &pl::add(&len, &n(4)));
    }
    et.delete("DOC_NUM");
    ret(n(1))
}

//------------------------------------------------------------------------------
/// process Nikon Capture data (ref 1)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_nikon_capture(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let dir_start = dir_info.get("DirStart");
    let dir_len = dir_info.get("DirLen");
    let dir_end = pl::add(&dir_start, &dir_len);
    let verbose = pl::method(it, &et, "Options", vec![s("Verbose")])?;
    let mut success = n(0);
    pl::call(it, "Image::ExifTool::SetByteOrder", vec![s("II")])?;
    if verbose.truthy() {
        pl::method(it, &et, "VerboseDir", vec![s("NikonCapture"), n(0), dir_len.clone()])?;
    }
    let mut pos = pl::add(&dir_start, &n(22));
    while pl::lt(&pl::add(&pos, &n(22)), &dir_end) {
        let tag_id = get32u(it, &data_pt, pos.clone())?;
        let mut size = pl::sub(&get32u(it, &data_pt, pl::add(&pos, &n(18)))?, &n(4));
        pos = pl::add(&pos, &n(22));
        if pl::lt(&size, &n(0)) || pl::gt(&pl::add(&pos, &size), &dir_end) {
            break;
        }
        let tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag_id.clone()])?;
        if tag_info.truthy() || verbose.truthy() {
            let (mut format, mut value) = (Value::Undef, Value::Undef);
            // (note that Writable will be 0 for Unknown tags)
            if tag_info.truthy() {
                format = tag_info.get("Format");
                if !format.truthy() {
                    format = tag_info.get("Writable");
                }
            }
            // generate a reasonable default format type for short values
            if !format.truthy() && (pl::num_eq(&size, &n(1)) || pl::num_eq(&size, &n(2)) || pl::num_eq(&size, &n(4))) {
                format = pl::concat(&pl::concat(&s("int"), &pl::mul(&size, &n(8))), &s("u"));
            }
            if format.truthy() {
                let mut count = n(1);
                if pl::str_eq(&format, "string") || pl::str_eq(&format, "undef") {
                    // patch Nikon bug in size of some values (HistogramXML)
                    if tag_info.truthy() && tag_info.get("AdjustSize").truthy() {
                        size = pl::add(&size, &tag_info.get("AdjustSize"));
                    }
                    count = size.clone();
                }
                value = pl::call(it, "Image::ExifTool::ReadValue", vec![data_pt.clone(), pos.clone(), format, count, size.clone()])?;
            } else if pl::num_eq(&size, &n(1)) {
                value = pl::substr(&pl::sderef(&data_pt)?, pos.to_i64(), Some(size.to_i64()));
            }
            let r = pl::method(
                it,
                &et,
                "HandleTag",
                vec![
                    tag_table_ptr.clone(),
                    tag_id,
                    value,
                    s("DataPt"),
                    data_pt.clone(),
                    s("DataPos"),
                    dir_info.get("DataPos"),
                    s("Base"),
                    dir_info.get("Base"),
                    s("Start"),
                    pos.clone(),
                    s("Size"),
                    size.clone(),
                ],
            )?;
            if r.truthy() {
                success = n(1);
            }
        }
        pos = pl::add(&pos, &size);
    }
    ret(success)
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::NikonCapture::WriteNikonCapture", write_nikon_capture);
    it.define_native("Image::ExifTool::NikonCapture::ProcessNikonCaptureEditVersions", process_nikon_capture_edit_versions);
    it.define_native("Image::ExifTool::NikonCapture::ProcessNikonCapture", process_nikon_capture);
}
