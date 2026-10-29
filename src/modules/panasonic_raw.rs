//! Image::ExifTool::PanasonicRaw (Image/ExifTool/PanasonicRaw.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read/write Panasonic/Leica RAW/RW2/RWL meta information

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, substr};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, n, ret};

const MODULE: &str = "Image::ExifTool::PanasonicRaw";

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

fn hash(pairs: Vec<(&str, Value)>) -> Value {
    let h = Value::new_hash();
    for (k, v) in pairs {
        h.set(k, v);
    }
    h
}

fn or(a: Value, b: impl FnOnce() -> Value) -> Value {
    if a.truthy() {
        a
    } else {
        b()
    }
}

fn scalar(r: ER<Vec<Value>>) -> ER<Value> {
    Ok(r?.pop().unwrap_or_default())
}

//------------------------------------------------------------------------------
/// checksum algorithm for lens distortion correction information (ref 3)
/// Inputs: 0) data ref, 1) start position, 2) number of bytes, 3) incement
/// Returns: checksum value
pub fn checksum(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (data_pt, start, num, inc) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    let mut csum = n(0);
    let mut i = n(0);
    while pl::lt(&i, &num) {
        let b = call(it, "Get8u", vec![data_pt.clone(), pl::add(&start, &pl::mul(&i, &inc))])?;
        csum = pl::modulo(&pl::add(&pl::mul(&n(73), &csum), &b), &n(0xffef))?;
        i = pl::add(&i, &n(1));
    }
    ret(csum)
}

fn csum(it: &mut Interp, data_pt: &Value, start: Value, num: i64, inc: i64) -> ER<Value> {
    scalar(checksum(it, vec![data_pt.clone(), start, n(num), n(inc)], Want::Scalar))
}

//------------------------------------------------------------------------------
/// Read lens distortion information
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_distortion_info(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let start = or(dir_info.get("DirStart"), || n(0));
    let mut size = dir_info.get("DirLen");
    if !size.truthy() {
        pl::at(719);
        size = pl::sub(&pl::length(&pl::sderef(&data_pt)?), &start);
    }
    if pl::num_eq(&size, &n(32)) {
        // verify the checksums (ref 3)
        let csum1 = csum(it, &data_pt, pl::add(&start, &n(4)), 12, 1)?;
        let csum2 = csum(it, &data_pt, pl::add(&start, &n(16)), 12, 1)?;
        let csum3 = csum(it, &data_pt, pl::add(&start, &n(2)), 14, 2)?;
        let csum4 = csum(it, &data_pt, pl::add(&start, &n(3)), 14, 2)?;
        let g2 = call(it, "Get16u", vec![data_pt.clone(), pl::add(&start, &n(2))])?;
        let g28 = call(it, "Get16u", vec![data_pt.clone(), pl::add(&start, &n(28))])?;
        let g0 = call(it, "Get16u", vec![data_pt.clone(), pl::add(&start, &n(0))])?;
        let g30 = call(it, "Get16u", vec![data_pt.clone(), pl::add(&start, &n(30))])?;
        let mut res = ops::bit_xor(&csum1, &g2);
        for v in [csum2, g28, csum3, g0, csum4, g30] {
            res = ops::bit_xor(&res, &v);
        }
        if res.truthy() {
            pl::method(it, &et, "Warn", vec![pl::s("Invalid DistortionInfo checksum"), n(1)])?;
        }
    } else {
        pl::method(it, &et, "Warn", vec![pl::s("Invalid DistortionInfo"), n(1)])?;
    }
    it.call_method(&et, "ProcessBinaryData", vec![et.clone(), dir_info, tag_table_ptr], want)
}

//------------------------------------------------------------------------------
/// Write lens distortion information
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: updated distortion information or undef on error
pub fn write_distortion_info(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if !et.truthy() {
        return ret(n(1)); // (allow dummy access)
    }
    let dat = cell(pl::method(it, &et, "WriteBinaryData", vec![dir_info, tag_table_ptr])?);
    let d = Value::ScalarRef(dat.clone());
    if !dat.borrow().is_undef() && pl::num_eq(&pl::length(&dat.borrow()), &n(32)) {
        // fix checksums (ref 3)
        let c = csum(it, &d, n(4), 12, 1)?;
        call(it, "Set16u", vec![c, d.clone(), n(2)])?;
        let c = csum(it, &d, n(16), 12, 1)?;
        call(it, "Set16u", vec![c, d.clone(), n(28)])?;
        let c = csum(it, &d, n(2), 14, 2)?;
        call(it, "Set16u", vec![c, d.clone(), n(0)])?;
        let c = csum(it, &d, n(3), 14, 2)?;
        call(it, "Set16u", vec![c, d.clone(), n(30)])?;
    } else {
        pl::method(it, &et, "Warn", vec![pl::s("Error wriing DistortionInfo"), n(1)])?;
    }
    let v = dat.borrow().clone();
    ret(v)
}

//------------------------------------------------------------------------------
/// Patch for writing non-standard Panasonic RAW/RW2/RWL raw data
/// Inputs: 0) offset info ref, 1) raf ref, 2) IFD number
/// Returns: error string, or undef on success
/// OffsetInfo is a hash by tag ID of lists with the following elements:
///  0 - tag info ref
///  1 - pointer to int32u offset in IFD or value data
///  2 - value count
///  3 - reference to list of original offset values
///  4 - IFD format number
///  5 - (pointer to StripOffsets value added by this PatchRawDataOffset routine)
///  6 - flag set if this is a fixed offset (Panasonic GH6 fixed-offset hack)
pub fn patch_raw_data_offset(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (offset_info, raf, ifd) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let mut strip_offsets = offset_info.get("273");
    let mut strip_byte_counts = offset_info.get("279");
    let raw_data_offset = offset_info.get("280");
    let mut err = false;
    if !pl::num_eq(&ifd, &n(0)) {
        err = true;
    }
    if strip_offsets.truthy() || strip_byte_counts.truthy() {
        if !(strip_offsets.truthy() && strip_byte_counts.truthy() && pl::num_eq(&strip_offsets.at(2), &n(1))) {
            err = true;
        }
    } else {
        // the DC-GH6 and DC-GH5M2 write RawDataOffset with no Strip tags, so we need
        // to create fake StripByteCounts information for copying the data
        if offset_info.get("280").truthy() {
            // (just to be safe)
            let main117 = pl::pkg_hash(it, "PanasonicRaw::Main").get("279");
            strip_byte_counts = Value::new_array(vec![main117, n(0), n(1), Value::new_array(vec![n(0)]), n(4)]);
            offset_info.set("279", strip_byte_counts.clone());
            // set flag so the offset will be fixed (GH6 hack, see https://exiftool.org/forum/index.php?topic=13861.0)
            // (of course, fixing up the offset is now unnecessary, but continue to do this even
            // though the fixup adjustment will be 0 because this allows us to delete the following
            // line to remove the fix-offset restriction if Panasonic ever sees the light, but note
            // that in this case we should investigate the purpose of the seemily-duplicate raw
            // data offset contained within PanasonicRaw_0x0044)
            offset_info.get_or_new_array("280").set_at(6, n(1));
        }
    }
    if raw_data_offset.truthy() && !err {
        if !pl::num_eq(&raw_data_offset.at(2), &n(1)) {
            err = true;
        }
        if strip_offsets.truthy() && !(pl::num_eq(&strip_offsets.at(3).at(0), &n(0xffffffff)) || pl::num_eq(&strip_byte_counts.at(3).at(0), &n(0))) {
            err = true;
        }
    }
    if err {
        return ret(pl::s("Unsupported Panasonic/Leica RAW variant"));
    }
    if raw_data_offset.truthy() {
        // update StripOffsets along with this tag if it contains a reasonable value
        if strip_offsets.truthy() && !pl::num_eq(&strip_offsets.at(3).at(0), &n(0xffffffff)) {
            // save pointer to StripOffsets value for updating later
            raw_data_offset.push(strip_offsets.at(1));
        }
        // handle via RawDataOffset instead of StripOffsets
        strip_offsets = raw_data_offset.clone();
        offset_info.set("273", raw_data_offset.clone());
        offset_info.delete("280");
    }
    // determine the length of the raw data
    pl::at(812);
    let pos = pl::method(it, &raf, "Tell", vec![])?;
    pl::at(813);
    if !pl::method(it, &raf, "Seek", vec![n(0), n(2)])?.truthy() {
        err = true; // seek to end of file
    }
    pl::at(814);
    let len = pl::sub(&pl::method(it, &raf, "Tell", vec![])?, &strip_offsets.at(3).at(0));
    pl::at(815);
    pl::method(it, &raf, "Seek", vec![pos, n(0)])?;
    // quick check to be sure the raw data length isn't unreasonable
    // (the 22-byte length is for '<Dummy raw image data>' in our tests)
    if (pl::lt(&len, &n(1000)) && !pl::num_eq(&len, &n(22))) || ops::bit_and(&len, &n(0x80000000)).truthy() {
        err = true;
    }
    if err {
        return ret(pl::s("Error reading Panasonic raw data"));
    }
    // update StripByteCounts info with raw data length
    // (note that the original value is maintained in the file)
    strip_byte_counts.at(3).set_at(0, len);

    pl::ret_undef()
}

//------------------------------------------------------------------------------
/// Write meta information to Panasonic JpgFromRaw in RAW/RW2/RWL image
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: updated image data, or undef if nothing changed
pub fn write_jpg_from_raw(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, _tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let mut data_pt = dir_info.get("DataPt");
    let byte_order = call(it, "GetByteOrder", vec![])?;
    let file_type = et.get("TIFF_TYPE"); // RAW, RW2 or RWL
    let dir_start = dir_info.get("DirStart");
    if dir_start.truthy() {
        // DirStart is non-zero in DNG-converted RW2/RWL
        pl::at(839);
        let dir_len = ops::bit_or(&dir_info.get("DirLen"), &pl::sub(&pl::length(&pl::sderef(&data_pt)?), &dir_start));
        pl::at(840);
        let buff = substr(it, &pl::sderef(&data_pt)?, &dir_start, Some(&dir_len))?;
        data_pt = pl::sref(buff);
    }
    let raf = pl::method(it, &pl::s("File::RandomAccess"), "new", vec![data_pt.clone()])?;
    let outbuff = cell(Value::Undef);
    let dir_info2 = hash(vec![("RAF", raf), ("OutFile", Value::ScalarRef(outbuff.clone()))]);
    et.set("BASE", dir_info.get("DataPos"));
    et.set("TIFF_TYPE", pl::s("JPEG"));
    et.set("FILE_TYPE", pl::s("JPEG"));
    // use a specialized map so we don't write XMP or IPTC (or other junk) into the JPEG
    let edit_dirs = et.get("EDIT_DIRS");
    let add_dirs = et.get("ADD_DIRS");
    let map = crate::runtime::lex_hash(it, MODULE, "jpgFromRawMap");
    pl::method(it, &et, "InitWriteDirs", vec![map])?;
    // don't add XMP segment (IPTC won't get added because it is in Photoshop record)
    et.get("ADD_DIRS").delete("XMP");
    let result = pl::method(it, &et, "WriteJPEG", vec![dir_info2])?;
    // restore variables we changed
    et.set("BASE", n(0));
    et.set("FILE_TYPE", pl::s("TIFF"));
    et.set("TIFF_TYPE", file_type);
    et.set("EDIT_DIRS", edit_dirs);
    et.set("ADD_DIRS", add_dirs);
    call(it, "SetByteOrder", vec![byte_order])?;
    pl::at(865);
    let v = if pl::gt(&result, &n(0)) { outbuff.borrow().clone() } else { pl::sderef(&data_pt)? };
    ret(v)
}

//------------------------------------------------------------------------------
/// Extract meta information from an Panasonic JpgFromRaw
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid JpgFromRaw image
pub fn process_jpg_from_raw(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, _tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let mut data_pt = dir_info.get("DataPt");
    let byte_order = call(it, "GetByteOrder", vec![])?;
    let file_type = et.get("TIFF_TYPE"); // RAW, RW2 or RWL
    let tag_info = dir_info.get("TagInfo");
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    // (the outer $indent stays undef: the "my $indent" below is another variable)
    let indent = Value::Undef;
    let mut out = Value::Undef;
    if !tag_info.truthy() {
        pl::method(it, &et, "Warn", vec![pl::s("No tag info for Panasonic JpgFromRaw")])?;
        return ret(n(0));
    }
    let dir_start = dir_info.get("DirStart");
    if dir_start.truthy() {
        // DirStart is non-zero in DNG-converted RW2/RWL
        pl::at(884);
        let dir_len = ops::bit_or(&dir_info.get("DirLen"), &pl::sub(&pl::length(&pl::sderef(&data_pt)?), &dir_start));
        pl::at(885);
        let buff = substr(it, &pl::sderef(&data_pt)?, &dir_start, Some(&dir_len))?;
        data_pt = pl::sref(buff);
    }
    et.set("BASE", pl::add(&dir_info.get("DataPos"), &or(dir_start.clone(), || n(0))));
    et.set("TIFF_TYPE", pl::s("JPEG"));
    et.set("FILE_TYPE", pl::s("JPEG"));
    et.set("DOC_NUM", n(1));
    // extract information from embedded JPEG
    let raf = pl::method(it, &pl::s("File::RandomAccess"), "new", vec![data_pt.clone()])?;
    let dir_info2 = hash(vec![("Parent", pl::s("RAF")), ("RAF", raf)]);
    if verbose.truthy() {
        let _indent = et.get("INDENT");
        et.set("INDENT", pl::s("  "));
        out = pl::method(it, &et, "Options", vec![pl::s("TextOut")])?;
        let dashes = Value::bytes(vec![b'-'; 56]);
        pl::print_to(it, Some(&out), &[pl::s("--- DOC1:JpgFromRaw "), dashes, pl::s("\n")])?;
    }
    // fudge HtmlDump base offsets to show as a stand-alone JPEG
    et.set("BASE_FUDGE", et.get("BASE"));
    let rtn_val = pl::method(it, &et, "ProcessJPEG", vec![dir_info2])?;
    et.set("BASE_FUDGE", n(0));
    // restore necessary variables for continued RW2/RWL processing
    et.set("BASE", n(0));
    et.set("FILE_TYPE", pl::s("TIFF"));
    et.set("TIFF_TYPE", file_type);
    et.delete("DOC_NUM");
    call(it, "SetByteOrder", vec![byte_order])?;
    if verbose.truthy() {
        et.set("INDENT", indent);
        let dashes = Value::bytes(vec![b'-'; 76]);
        pl::print_to(it, Some(&out), &[dashes, pl::s("\n")])?;
    }
    ret(rtn_val)
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::PanasonicRaw::Checksum", checksum);
    it.define_native("Image::ExifTool::PanasonicRaw::ProcessDistortionInfo", process_distortion_info);
    it.define_native("Image::ExifTool::PanasonicRaw::WriteDistortionInfo", write_distortion_info);
    it.define_native("Image::ExifTool::PanasonicRaw::PatchRawDataOffset", patch_raw_data_offset);
    it.define_native("Image::ExifTool::PanasonicRaw::WriteJpgFromRaw", write_jpg_from_raw);
    it.define_native("Image::ExifTool::PanasonicRaw::ProcessJpgFromRaw", process_jpg_from_raw);
}
