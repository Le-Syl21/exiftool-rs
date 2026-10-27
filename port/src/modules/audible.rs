//! Image::ExifTool::Audible (Image/ExifTool/Audible.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read metadata from Audible audio books

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell, substr};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

fn warn(it: &mut Interp, et: &Value, msg: Value) -> ER<()> {
    pl::method(it, et, "Warn", vec![msg])?;
    Ok(())
}

/// `{ Name => $name }`
fn name_hash(name: Value) -> Value {
    let h = Value::new_hash();
    h.set("Name", name);
    h
}

/// Process Audible 'meta' tags from M4B files (ref PH)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_audible_meta(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let data_pos = dir_info.get("DataPos");
    let data = data_pt.deref();
    let dir_len = pl::length(&data);
    if pl::lt(&dir_len, &n(4)) {
        return ret(n(0));
    }
    let num = call(it, "Get32u", vec![data_pt.clone(), n(0)])?;
    pl::method(it, &et, "VerboseDir", vec![pl::s("Audible_meta"), num.clone()])?;
    let mut pos = n(4);
    let mut index = n(0);
    while pl::lt(&index, &num) {
        if pl::gt(&pl::add(&pos, &n(3)), &dir_len) {
            break;
        }
        let unk = call(it, "Get8u", vec![data_pt.clone(), pos.clone()])?; // ? (0x80 or 0x00)
        if !(pl::num_eq(&unk, &n(0x80)) || pl::num_eq(&unk, &n(0x00))) {
            break;
        }
        let len = call(it, "Get16u", vec![data_pt.clone(), pl::add(&pos, &n(1))])?; // tag length
        pos = pl::add(&pos, &n(3));
        if pl::gt(&pl::add(&pl::add(&pos, &len), &n(6)), &dir_len) || !len.truthy() {
            break;
        }
        let tag = substr(it, &data, &pos, Some(&len))?; // tag ID
        let ver = call(it, "Get16u", vec![data_pt.clone(), pl::add(&pos, &len)])?; // version?
        if !pl::num_eq(&ver, &n(0x0001)) {
            break;
        }
        let size = call(it, "Get32u", vec![data_pt.clone(), pl::add(&pl::add(&pos, &len), &n(2))])?; // data size
        pos = pl::add(&pos, &pl::add(&len, &n(6)));
        if pl::gt(&pl::add(&pos, &size), &dir_len) {
            break;
        }
        let s = substr(it, &data, &pos, Some(&size))?;
        let val = pl::method(it, &et, "Decode", vec![s, pl::s("UTF8")])?;
        if !tag_table_ptr.get(tag.to_bytes()).truthy() {
            let t = if re("[a-z]", "").is_match(&tag) { tag.clone() } else { pl::lc(&tag) };
            let name = pl::call(it, "Image::ExifTool::MakeTagName", vec![t])?;
            call(it, "AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), name_hash(name)])?;
        }
        pl::method(
            it,
            &et,
            "HandleTag",
            vec![tag_table_ptr.clone(), tag, val, pl::s("DataPt"), data_pt.clone(), pl::s("DataPos"), data_pos.clone(), pl::s("Start"), pos.clone(), pl::s("Size"), size.clone(), pl::s("Index"), index.clone()],
        )?;
        pos = pl::add(&pos, &size);
        index = pl::add(&index, &n(1));
    }
    ret(n(1))
}

/// Process Audible 'cvrx' cover art atom from M4B files (ref PH)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_audible_cvrx(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let data_pos = dir_info.get("DataPos");
    let dir_len = pl::length(&data_pt.deref());
    if pl::gt(&n(0x0a), &dir_len) {
        return ret(n(0));
    }
    let len = call(it, "Get16u", vec![data_pt.clone(), n(0x08)])?;
    if pl::gt(&pl::add(&pl::add(&n(0x0a), &len), &n(6)), &dir_len) {
        return ret(n(0));
    }
    let size = call(it, "Get32u", vec![data_pt.clone(), pl::add(&pl::add(&n(0x0a), &len), &n(2))])?;
    if pl::gt(&pl::add(&pl::add(&pl::add(&n(0x0a), &len), &n(6)), &size), &dir_len) {
        return ret(n(0));
    }
    pl::method(it, &et, "VerboseDir", vec![pl::s("Audible_cvrx"), Value::Undef, dir_len])?;
    pl::method(
        it,
        &et,
        "HandleTag",
        vec![tag_table_ptr.clone(), pl::s("CoverArtType"), Value::Undef, pl::s("DataPt"), data_pt.clone(), pl::s("DataPos"), data_pos.clone(), pl::s("Start"), n(0x0a), pl::s("Size"), len.clone()],
    )?;
    pl::method(
        it,
        &et,
        "HandleTag",
        vec![tag_table_ptr.clone(), pl::s("CoverArt"), Value::Undef, pl::s("DataPt"), data_pt.clone(), pl::s("DataPos"), data_pos.clone(), pl::s("Start"), pl::add(&pl::add(&n(0x0a), &len), &n(6)), pl::s("Size"), size],
    )?;
    ret(n(1))
}

/// Read information from an Audible .AA file
/// Inputs: 0) ExifTool ref, 1) dirInfo ref
/// Returns: 1 on success, 0 if this wasn't a valid AA file
pub fn process_aa(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let (buff, toc) = (cell(Value::Undef), cell(Value::Undef));
    let buff_ref = Value::ScalarRef(buff.clone());
    let toc_ref = Value::ScalarRef(toc.clone());

    // check magic number
    if !(pl::num_eq(&raf_read_cell(it, &raf, &buff, n(16))?, &n(16)) && re(r"^.{4}\x57\x90\x75\x36", "s").is_match(&buff.borrow().clone())) {
        return ret(n(0));
    }
    // check file size
    let fsize = et.get("VALUE").get("FileSize");
    if !fsize.is_undef() {
        // first 4 bytes of the file should be the filesize
        if !pl::num_eq(&pl::unpack1("N", &buff.borrow().clone())?, &fsize) {
            return ret(n(0));
        }
    }
    pl::method(it, &et, "SetFileType", vec![])?;
    call(it, "SetByteOrder", vec![pl::s("MM")])?;
    let bytes = pl::mul(&n(12), &call(it, "Get32u", vec![buff_ref.clone(), n(8)])?); // table of contents size in bytes
    if pl::gt(&bytes, &n(0xc00)) {
        warn(it, &et, pl::s("Invalid TOC"))?;
        return ret(n(1));
    }
    // read the table of contents
    if !pl::num_eq(&raf_read_cell(it, &raf, &toc, bytes.clone())?, &bytes) {
        warn(it, &et, pl::s("Truncated TOC"))?;
        return ret(n(1));
    }
    let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::Audible::Main")])?;
    // parse table of contents (in $toc)
    let mut entry = n(0);
    'toc: while pl::lt(&entry, &bytes) {
        'next: {
            let type_ = call(it, "Get32u", vec![toc_ref.clone(), entry.clone()])?;
            if !(pl::num_eq(&type_, &n(2)) || pl::num_eq(&type_, &n(6)) || pl::num_eq(&type_, &n(11))) {
                break 'next;
            }
            let offset = call(it, "Get32u", vec![toc_ref.clone(), pl::add(&entry, &n(4))])?;
            let length = call(it, "Get32u", vec![toc_ref.clone(), pl::add(&entry, &n(8))])?;
            if !length.truthy() {
                break 'next;
            }
            let ty = type_.to_text();
            if !pl::method(it, &raf, "Seek", vec![offset.clone(), n(0)])?.truthy() {
                warn(it, &et, Value::from(format!("Chunk {ty} seek error")))?;
                break 'toc;
            }
            if pl::num_eq(&type_, &n(6)) {
                // offset table
                if pl::lt(&length, &n(4)) || !pl::num_eq(&raf_read_cell(it, &raf, &buff, n(4))?, &n(4)) {
                    break 'next; // only read the chapter count
                }
                let v = call(it, "Get32u", vec![buff_ref.clone(), n(0)])?;
                pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("_chapter_count"), v])?;
                break 'next;
            }
            // read the chunk
            if pl::gt(&length, &n(100000000)) {
                warn(it, &et, Value::from(format!("Chunk {ty} too big")))?;
                break 'next;
            }
            if !pl::num_eq(&raf_read_cell(it, &raf, &buff, length.clone())?, &length) {
                warn(it, &et, Value::from(format!("Chunk {ty} read error")))?;
                break 'toc;
            }
            if pl::num_eq(&type_, &n(11)) {
                // cover art
                if pl::lt(&length, &n(8)) {
                    break 'next;
                }
                let len = call(it, "Get32u", vec![buff_ref.clone(), n(0)])?;
                let off = call(it, "Get32u", vec![buff_ref.clone(), n(4)])?;
                if pl::lt(&off, &pl::add(&offset, &n(8))) || pl::gt(&pl::add(&pl::sub(&off, &offset), &len), &length) {
                    break 'next;
                }
                let v = substr(it, &buff.borrow().clone(), &pl::sub(&off, &offset), Some(&len))?;
                pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("_cover_art"), v])?;
                break 'next;
            }
            // parse metadata dictionary (in $buff)
            if pl::lt(&length, &n(4)) {
                warn(it, &et, pl::s("Bad dictionary"))?;
                break 'next;
            }
            let num = call(it, "Get32u", vec![buff_ref.clone(), n(0)])?;
            if pl::gt(&num, &n(0x200)) {
                warn(it, &et, pl::s("Bad dictionary count"))?;
                break 'next;
            }
            let mut pos = n(4); // dictionary starts immediately after count
            crate::runtime::require(it, "Image::ExifTool::HTML")?; // (for UnescapeHTML)
            pl::method(it, &et, "VerboseDir", vec![pl::s("Audible Metadata"), num.clone()])?;
            let mut i = n(0);
            while pl::lt(&i, &num) {
                let tag_pos = pl::add(&pos, &n(9)); // position of tag string
                if pl::gt(&tag_pos, &length) {
                    warn(it, &et, pl::s("Truncated dictionary"))?;
                    break;
                }
                // (1 unknown byte ignored at start of each dictionary entry)
                let tag_len = call(it, "Get32u", vec![buff_ref.clone(), pl::add(&pos, &n(1))])?; // tag string length
                let val_len = call(it, "Get32u", vec![buff_ref.clone(), pl::add(&pos, &n(5))])?; // value string length
                let val_pos = pl::add(&tag_pos, &tag_len); // position of value string
                let nxt_pos = pl::add(&val_pos, &val_len); // position of next entry
                if pl::gt(&nxt_pos, &length) {
                    warn(it, &et, pl::s("Bad dictionary entry"))?;
                    break;
                }
                let b = buff.borrow().clone();
                let tag = substr(it, &b, &tag_pos, Some(&tag_len))?;
                let val = substr(it, &b, &val_pos, Some(&val_len))?;
                if !tag_table_ptr.get(tag.to_bytes()).truthy() {
                    let mut name = pl::call(it, "Image::ExifTool::MakeTagName", vec![tag.clone()])?;
                    re("_(.)", "").subst(&mut name, true, |c| cap(c, 1).to_bytes().to_ascii_uppercase()); // change from underscore-separated to mixed case
                    call(it, "AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), name_hash(name)])?;
                }
                // unescape HTML character references and convert from UTF-8
                let u = pl::call(it, "Image::ExifTool::HTML::UnescapeHTML", vec![val])?;
                let val = pl::method(it, &et, "Decode", vec![u, pl::s("UTF8")])?;
                pl::method(
                    it,
                    &et,
                    "HandleTag",
                    vec![tag_table_ptr.clone(), tag, val, pl::s("DataPos"), offset.clone(), pl::s("DataPt"), buff_ref.clone(), pl::s("Start"), val_pos.clone(), pl::s("Size"), val_len.clone(), pl::s("Index"), i.clone()],
                )?;
                pos = nxt_pos; // step to next dictionary entry
                i = pl::add(&i, &n(1));
            }
        }
        entry = pl::add(&entry, &n(12));
    }
    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Audible::ProcessAudible_meta", process_audible_meta);
    it.define_native("Image::ExifTool::Audible::ProcessAudible_cvrx", process_audible_cvrx);
    it.define_native("Image::ExifTool::Audible::ProcessAA", process_aa);
}
