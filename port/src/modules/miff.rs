//! Image::ExifTool::MIFF (Image/ExifTool/MIFF.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read Magick Image File Format meta information.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read, raf_read_cell, raf_read_line};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

fn s(x: &str) -> Value {
    pl::s(x)
}

fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
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
// Extract meta information from a MIFF image
// Inputs: 0) ExifTool object reference, 1) dirInfo reference
// Returns: 1 on success, 0 if this wasn't a valid MIFF image
pub fn process_miff(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let verbose = et.path(&["OPTIONS", "Verbose"]);
    let mut hdr = Value::Undef;

    // validate the MIFF file (note: MIFF files _may_ begin with other
    // characters, but this starting sequence is strongly suggested.)
    if !pl::num_eq(&raf_read(it, &raf, &mut hdr, n(14))?, &n(14)) {
        return ret(n(0));
    }
    if !pl::str_eq(&hdr, "id=ImageMagick") {
        return ret(n(0));
    }
    pl::method(it, &et, "SetFileType", vec![])?; // set the FileType tag

    // set end-of-line character sequence to read to end of the TEXT
    // section for new-type MIFF files (text ends with Colon+Ctrl-Z)
    // Old MIFF files end with Colon+Linefeed, so this will likely
    // slurp those entire files, which will be slower, but will work
    // OK except that the profile information won't be decoded
    pl::local_global(it, b'$', "main::/", |it| {
        it.set_scalar("main::/", s(":\x1a"));
        process_miff_body(it, &et, &raf, hdr, verbose)
    })
}

/// ProcessMIFF after `local $/ = ":\x1a"`.
fn process_miff_body(it: &mut Interp, et: &Value, raf: &Value, hdr: Value, verbose: Value) -> ER<Vec<Value>> {
    let buff = cell(Value::Undef);
    let mut mode = s("");
    let mut profiles: Vec<(Value, Value)> = Vec::new();
    let mut line = Value::Undef;
    if raf_read_line(it, raf, &mut line)?.truthy() {
        // chomp $buff: remove end-of-line chars
        let lb = line.to_bytes();
        if lb.ends_with(b":\x1a") {
            line = Value::bytes(&lb[..lb.len() - 2]);
        }
        let tag_table_ptr = call(it, "GetTagTable", vec![s("Image::ExifTool::MIFF::Main")])?;
        let mut entries = pl::split_ws(&line);
        entries.insert(0, hdr); // put the ID back in
        let (mut tag, mut val) = (Value::Undef, Value::Undef);
        for e in &entries {
            if pl::str_eq(&mode, "com") {
                if re(r"\}$", "").is_match(e) {
                    mode = s("");
                }
                continue;
            } else if re(r"^\{", "").is_match(e) {
                mode = s("com"); // read to the end of the comment
                continue;
            }
            if pl::str_eq(&mode, "val") {
                val = cat(&[val, s(" "), e.clone()]); // join back together with a space
                if !re(r"\}$", "").is_match(e) {
                    continue;
                }
                mode = s("");
                re(r"(^\{|\}$)", "").subst(&mut val, true, |_| Vec::new()); // remove braces
            } else if let Some(m) = re(r"(.+)=(.+)", "").m(e) {
                (tag, val) = (cap(&m, 1), cap(&m, 2));
                if re(r"^\{", "").is_match(&val) {
                    mode = s("val"); // read to the end of the value data
                    continue;
                }
            } else if re(r"^:", "").is_match(e) {
                // this could be the end of an old-style MIFF file
                break;
            } else {
                // something we don't recognize -- stop parsing here
                pl::method(it, et, "Warn", vec![s("Unrecognized MIFF data")])?;
                break;
            }
            let mut tag_info = pl::method(it, et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
            if !tag_info.truthy() {
                tag_info = Value::new_hash();
                tag_info.set("Name", tag.clone());
                call(it, "AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), tag_info.clone()])?;
            }
            if verbose.truthy() {
                pl::method(it, et, "VerboseInfo", vec![tag.clone(), tag_info.clone(), s("Table"), tag_table_ptr.clone(), s("DataPt"), pl::sref(val.clone())])?;
            }
            // handle profile tags specially
            if let Some(m) = re(r"^profile-(.*)", "").m(&tag) {
                profiles.push((cap(&m, 1), val.clone()));
            } else {
                pl::method(it, et, "FoundTag", vec![tag_info, val.clone()])?;
            }
        }
    }

    // process profile information
    for (type_, len) in profiles {
        if !re(r"^\d+$", "").is_match(&len) {
            pl::method(it, et, "Warn", vec![cat(&[s("Invalid length for "), type_.clone(), s(" profile")])])?;
            break; // don't try to read the rest
        }
        if !pl::num_eq(&raf_read_cell(it, raf, &buff, len.clone())?, &len) {
            pl::method(it, et, "Warn", vec![cat(&[s("Error reading "), type_.clone(), s(" profile ("), len.clone(), s(" bytes)")])])?;
            continue;
        }
        let mut processed = n(0);
        let dir_info = Value::new_hash();
        dir_info.set("Parent", s("PNG"));
        dir_info.set("DataPt", Value::ScalarRef(buff.clone()));
        dir_info.set("DataPos", pl::sub(&pl::method(it, raf, "Tell", vec![])?, &len));
        dir_info.set("DataLen", len.clone());
        dir_info.set("DirStart", n(0));
        dir_info.set("DirLen", len.clone());
        let b = buff.borrow().clone();
        if pl::str_eq(&type_, "icc") {
            // ICC Profile information
            let tag_table_ptr = call(it, "GetTagTable", vec![s("Image::ExifTool::ICC_Profile::Main")])?;
            processed = pl::method(it, et, "ProcessDirectory", vec![dir_info.clone(), tag_table_ptr])?;
        } else if pl::str_eq(&type_, "iptc") {
            if re(r"^8BIM", "").is_match(&b) {
                // Photoshop information
                let tag_table_ptr = call(it, "GetTagTable", vec![s("Image::ExifTool::Photoshop::Main")])?;
                processed = pl::method(it, et, "ProcessDirectory", vec![dir_info.clone(), tag_table_ptr])?;
            }
        // I haven't seen 'exif' or 'xmp' profile types yet, but I have seen them
        // in newer PNG files so presumably they are possible here as well - PH
        } else if pl::str_eq(&type_, "APP1") || pl::str_eq(&type_, "exif") || pl::str_eq(&type_, "xmp") {
            let exif_hdr = it.get_scalar("Image::ExifTool::exifAPP1hdr");
            let xmp_hdr = it.get_scalar("Image::ExifTool::xmpAPP1hdr");
            let pat = |h: &Value| [b"^".as_slice(), &h.to_bytes()].concat();
            if pl::re_dyn(&pat(&exif_hdr), "")?.is_match(&b) {
                // APP1 EXIF
                let hdr_len = pl::length(&exif_hdr);
                dir_info.set("DirStart", pl::add(&dir_info.get("DirStart"), &hdr_len));
                dir_info.set("DirLen", pl::sub(&dir_info.get("DirLen"), &hdr_len));
                // use the usual position for EXIF data: 12 bytes from start of file
                // (this may be wrong, but I can't see where the PNG stores this information)
                dir_info.set("Base", n(12)); // this is the usual value
                processed = pl::method(it, et, "ProcessTIFF", vec![dir_info.clone()])?;
            } else if pl::re_dyn(&pat(&xmp_hdr), "")?.is_match(&b) {
                // APP1 XMP
                let hdr_len = pl::length(&xmp_hdr);
                let tag_table_ptr = call(it, "GetTagTable", vec![s("Image::ExifTool::XMP::Main")])?;
                dir_info.set("DirStart", pl::add(&dir_info.get("DirStart"), &hdr_len));
                dir_info.set("DirLen", pl::sub(&dir_info.get("DirLen"), &hdr_len));
                processed = pl::method(it, et, "ProcessDirectory", vec![dir_info.clone(), tag_table_ptr])?;
            }
        }
        if !processed.truthy() {
            pl::method(it, et, "Warn", vec![cat(&[s("Unknown MIFF "), type_.clone(), s(" profile data")])])?;
            if verbose.truthy() {
                pl::method(it, et, "VerboseDir", vec![type_.clone(), n(0), len.clone()])?;
                pl::method(it, et, "VerboseDump", vec![Value::ScalarRef(buff.clone())])?;
            }
        }
    }
    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::MIFF::ProcessMIFF", process_miff);
}
