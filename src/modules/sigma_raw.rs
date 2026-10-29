//! Image::ExifTool::SigmaRaw (Image/ExifTool/SigmaRaw.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read Sigma/Foveon RAW (X3F) meta information

use crate::interp::eval::{die, Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell, substr};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, n, re, ret};

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

fn val(c: &Cell) -> Value {
    c.borrow().clone()
}

fn rf(c: &Cell) -> Value {
    Value::ScalarRef(c.clone())
}

fn cat(parts: &[Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

/// `$raf->Read($buff, $len) == $len`
fn read_eq(it: &mut Interp, raf: &Value, buff: &Cell, len: &Value) -> ER<bool> {
    Ok(pl::num_eq(&raf_read_cell(it, raf, buff, len.clone())?, len))
}

fn seek(it: &mut Interp, raf: &Value, pos: Value, whence: i64) -> ER<bool> {
    Ok(pl::method(it, raf, "Seek", vec![pos, n(whence)])?.truthy())
}

fn write(it: &mut Interp, outfile: &Value, data: Vec<Value>) -> ER<bool> {
    let mut a = vec![outfile.clone()];
    a.extend(data);
    Ok(call(it, "Write", a)?.truthy())
}

fn err(msg: &str) -> ER<Vec<Value>> {
    ret(pl::s(msg))
}

/// `File::RandomAccess->new(\$buff)`
fn new_raf(it: &mut Interp, data_ref: Value) -> ER<Value> {
    pl::method(it, &pl::s("File::RandomAccess"), "new", vec![data_ref])
}

//------------------------------------------------------------------------------
/// Extract null-terminated unicode string from list of characters
/// Inputs: 0) ExifTool ref, 1) list ref, 2) position in list
/// Returns: Converted string
pub fn extract_unicode_string(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, chars, pos) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    // ($i<@$chars dies when $chars is not an array reference)
    pl::at(278);
    let list = pl::aderef(&chars)?;
    let mut i = pos.to_i64();
    while i < list.len() as i64 {
        if !chars.at(i).truthy() {
            break;
        }
        i += 1;
    }
    // @$chars[$pos..$i-1]
    let slice: Vec<Value> = (pos.to_i64()..i).map(|k| chars.at(k)).collect();
    let buff = pl::pack("v*", &slice)?;
    ret(pl::method(it, &et, "Decode", vec![buff, pl::s("UTF16"), pl::s("II")])?)
}

//------------------------------------------------------------------------------
/// Process an X3F header
/// Inputs: 0) ExifTool ref, 1) DirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_x3f_header(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, mut tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let hdr_len = dir_info.get("DirLen");

    // process the static header structure first
    pl::method(it, &et, "ProcessBinaryData", vec![dir_info.clone(), tag_table_ptr.clone()])?;

    // process extended data if available
    pl::at(299);
    if pl::ge(&pl::sub(&pl::length(&pl::sderef(&data_pt)?), &hdr_len), &n(160)) {
        let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
        if verbose.truthy() {
            pl::method(it, &et, "VerboseDir", vec![pl::s("X3F HeaderExt"), n(32)])?;
            pl::method(it, &et, "VerboseDump", vec![data_pt.clone(), pl::s("Start"), hdr_len.clone()])?;
        }
        tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::SigmaRaw::HeaderExt")])?;
        let tags = pl::unpack(&format!("x{}C32", hdr_len.to_text()), &data_pt.deref())?;
        let mut unused = n(0);
        for i in 0..32i64 {
            let t = arg(&tags, i as usize);
            if !t.truthy() {
                unused = pl::add(&unused, &n(1));
                continue;
            }
            pl::method(
                it,
                &et,
                "HandleTag",
                vec![tag_table_ptr.clone(), t, Value::Undef, pl::s("Index"), n(i), pl::s("DataPt"), data_pt.clone(), pl::s("Start"), pl::add(&pl::add(&hdr_len, &n(32)), &n(i * 4)), pl::s("Size"), n(4)],
            )?;
        }
        let msg = cat(&[et.get("INDENT"), pl::s("("), unused, pl::s(" entries unused)\n")]);
        pl::method(it, &et, "VPrint", vec![n(0), msg])?;
    }
    ret(n(1))
}

//------------------------------------------------------------------------------
/// Process an X3F properties
/// Inputs: 0) ExifTool ref, 1) DirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_x3f_properties(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    pl::at(331);
    let data = pl::sderef(&data_pt)?;
    let size = pl::length(&data);
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    let unknown = pl::method(it, &et, "Options", vec![pl::s("Unknown")])?;

    if !(pl::ge(&size, &n(24)) && re(r"^SECp", "").is_match(&data)) {
        pl::method(it, &et, "Warn", vec![pl::s("Bad properties header")])?;
        return ret(n(0));
    }
    let u = pl::unpack("x8V2x4V", &data)?;
    let (entries, fmt, len) = (arg(&u, 0), arg(&u, 1), arg(&u, 2));
    if !pl::ge(&size, &pl::add(&pl::add(&n(24), &pl::mul(&n(8), &entries)), &len)) {
        pl::method(it, &et, "Warn", vec![pl::s("Truncated Property directory")])?;
        return ret(n(0));
    }
    if verbose.truthy() {
        pl::method(it, &et, "VerboseDir", vec![pl::s("Properties"), entries.clone()])?;
    }
    if !pl::num_eq(&fmt, &n(0)) {
        pl::method(it, &et, "Warn", vec![pl::concat(&pl::s("Unsupported character format "), &fmt)])?;
        return ret(n(0));
    }
    let char_pos = pl::add(&n(24), &pl::mul(&n(8), &entries));
    let chars = pl::unpack("v*", &substr(it, &data, &char_pos, Some(&pl::mul(&len, &n(2))))?)?;
    let nchars = n(chars.len() as i64);
    let chars_ref = Value::new_array(chars);
    let mut index = n(0);
    while pl::lt(&index, &entries) {
        let p = pl::unpack("V2", &substr(it, &data, &pl::add(&pl::mul(&index, &n(8)), &n(24)), Some(&n(8)))?)?;
        let (name_pos, val_pos) = (arg(&p, 0), arg(&p, 1));
        if pl::ge(&name_pos, &nchars) || pl::ge(&val_pos, &nchars) {
            pl::method(it, &et, "Warn", vec![pl::s("Bad Property pointer")])?;
            return ret(n(0));
        }
        let tag = extract_unicode_string(it, vec![et.clone(), chars_ref.clone(), name_pos], Want::Scalar)?.pop().unwrap_or_default();
        let v = extract_unicode_string(it, vec![et.clone(), chars_ref.clone(), val_pos.clone()], Want::Scalar)?.pop().unwrap_or_default();
        if !tag_table_ptr.get(tag.to_bytes()).truthy() && unknown.truthy() && re(r"^\w+$", "").is_match(&tag) {
            let desc = pl::call(it, "Image::ExifTool::MakeDescription", vec![pl::s("SigmaRaw"), tag.clone()])?;
            let tag_info = hash(vec![("Name", pl::concat(&pl::s("SigmaRaw_"), &tag)), ("Description", desc), ("Unknown", n(1)), ("Writable", n(0))]); // can't write unknown tags
            // add tag information to table
            call(it, "AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), tag_info])?;
        }

        let sz = pl::mul(&n(2), &pl::add(&pl::length(&v), &n(1)));
        pl::method(
            it,
            &et,
            "HandleTag",
            vec![tag_table_ptr.clone(), tag, v, pl::s("Index"), index.clone(), pl::s("DataPt"), data_pt.clone(), pl::s("Start"), pl::add(&char_pos, &pl::mul(&n(2), &val_pos)), pl::s("Size"), sz],
        )?;
        index = pl::add(&index, &n(1));
    }
    ret(n(1))
}

/// `substr($s, $pos, $len) = $new`
fn substr_assign(s: &Value, pos: i64, len: i64, new: &Value) -> ER<Value> {
    let b = s.to_bytes();
    let Some((a, e)) = crate::interp::eval::substr_range(b.len(), pos, Some(len)) else {
        return die("substr outside of string");
    };
    let mut v = b[..a].to_vec();
    v.extend_from_slice(&new.to_bytes());
    v.extend_from_slice(&b[e..]);
    Ok(Value::bytes(v))
}

//------------------------------------------------------------------------------
/// Write an X3F file
/// Inputs: 0) ExifTool ref, 1) DirInfo ref (DirStart = directory offset)
/// Returns: error string, undef on success, or -1 on write error
/// Notes: Writes metadata to embedded JpgFromRaw image
pub fn write_x3f(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let outfile = dir_info.get("OutFile");
    let (hdr, buff, dir) = (cell(Value::Undef), cell(Value::Undef), cell(Value::Undef));
    let mut out_pos = Value::Undef;
    let mut did_contain = false;
    let order = Value::new_hash();

    pl::at(390);
    if !seek(it, &raf, dir_info.get("DirStart"), 0)? {
        return err("Error seeking to directory start");
    }

    // read the X3F directory header (will be copied directly to output)
    if !read_eq(it, &raf, &hdr, &n(12))? {
        return err("Truncated X3F image");
    }
    if !re(r"^SECd", "").is_match(&val(&hdr)) {
        return err("Bad section header");
    }
    let u = pl::unpack("x4V2", &val(&hdr))?;
    let (_ver, entries) = (arg(&u, 0), arg(&u, 1));

    // do sanity check on number of entries in directory
    if !(pl::gt(&entries, &n(2)) && pl::lt(&entries, &n(20))) {
        return err("Invalid X3F directory count");
    }
    // read the directory entries
    if !read_eq(it, &raf, &dir, &pl::mul(&entries, &n(12)))? {
        return err("Truncated X3F directory");
    }
    // do a quick scan to determine the offset of the first data subsection,
    // and the order in which the actual data is stored in the file
    let ne = entries.to_i64();
    for index in 0..ne {
        let pos = index * 12;
        let e = pl::unpack(&format!("x{pos}V2a4"), &val(&dir))?;
        let offset = arg(&e, 0);
        // remember position of first data subsection
        if out_pos.is_undef() || pl::gt(&out_pos, &offset) {
            out_pos = offset.clone();
        }
        // save the order of the data
        if !order.get(offset.to_bytes()).is_undef() {
            order.set("BAD", n(1));
        }
        order.set(offset.to_bytes(), n(index));
    }
    // copy the file header up to the start of the first data subsection
    if !(seek(it, &raf, n(0), 0)? && read_eq(it, &raf, &buff, &out_pos)?) {
        return err("Error reading X3F header");
    }
    if !write(it, &outfile, vec![val(&buff)])? {
        return ret(n(-1));
    }

    // this is a bit tricky/unfortunate:  the current version of Sigma Photo Pro
    // (2022-10-18) is sensitive to the order of the data sections, and these may
    // differ from the order of their respective entries in the footer.  To patch
    // this, instead of looping through the footer sections in order, we process
    // them in the order of the offsets they contain, writing their referenced data
    // sequentially as we go.  This preserves both the order of the data sections
    // and the order of the footer entries.  (Note that the upcoming release of
    // Sigma Photo Pro will fix this issue at their end, but this patch will remain
    // to maintain backward compatibilty with older SPP versions.)
    let order_list: Vec<i64> = if order.get("BAD").truthy() {
        // (this could perhaps happen if any of the sections is ever zero-length)
        pl::method(it, &et, "Error", vec![pl::s("Double-referenced data in footer directory!"), n(1)])?;
        (0..ne).collect()
    } else {
        let mut keys: Vec<Value> = order.keys().iter().map(Value::bytes).collect();
        keys.sort_by(|x, y| ops::num_cmp(x, y).unwrap_or(std::cmp::Ordering::Equal));
        keys.iter().map(|k| order.get(k.to_bytes()).to_i64()).collect()
    };

    // loop through footer directory, rewriting each section
    for index in order_list {
        let pos = index * 12;
        let e = pl::unpack(&format!("x{pos}V2a4"), &val(&dir))?;
        let (offset, mut len, tag) = (arg(&e, 0), arg(&e, 1), arg(&e, 2));
        if !seek(it, &raf, offset, 0)? {
            return err("Bad data offset");
        }

        if pl::str_eq(&tag, "IMA2") && pl::gt(&len, &n(28)) {
            // check subsection header (28 bytes) to see if this is a JPEG preview image
            if !read_eq(it, &raf, &buff, &n(28))? {
                return err("Error reading PreviewImage header");
            }
            if !write(it, &outfile, vec![val(&buff)])? {
                return ret(n(-1));
            }
            len = pl::sub(&len, &n(28));

            // only rewrite full-sized JpgFromRaw (version 2.0, type 2, format 18)
            if re(r"^SECi\0\0\x02\0\x02\0\0\0\x12\0\0\0", "").is_match(&val(&buff)) {
                if !read_eq(it, &raf, &buff, &len)? {
                    return err("Error reading JpgFromRaw");
                }
                if re(r"^\xff\xd8\xff\xe1", "").is_match(&val(&buff)) {
                    // does this preview contain EXIF?
                    // use same write directories as JPEG
                    pl::method(it, &et, "InitWriteDirs", vec![pl::s("JPEG")])?;
                    // make sure we don't add APP0 JFIF because it would mess up our preview identification
                    et.get("ADD_DIRS").delete("APP0");
                    et.get("ADD_DIRS").delete("JFIF");
                    // rewrite the embedded JPEG in memory
                    let new_data = cell(Value::Undef);
                    let jraf = new_raf(it, rf(&buff))?;
                    let jpeg_info = hash(vec![("Parent", pl::s("X3F")), ("RAF", jraf), ("OutFile", rf(&new_data))]);
                    et.set("FILE_TYPE", pl::s("JPEG"));
                    let success = pl::method(it, &et, "WriteJPEG", vec![jpeg_info])?;
                    et.set("FILE_TYPE", pl::s("X3F"));
                    call(it, "SetByteOrder", vec![pl::s("II")])?;
                    if !(success.truthy() && val(&new_data).truthy()) {
                        return err("Error writing X3F JpgFromRaw");
                    }
                    if pl::lt(&success, &n(0)) {
                        return ret(n(-1));
                    }
                    // (this shouldn't happen unless someone tries to delete the EXIF...)
                    if !re(r"^\xff\xd8\xff\xe1", "").is_match(&val(&new_data)) {
                        return err("EXIF segment must come first in X3F JpgFromRaw");
                    }
                    // trim off any extra null bytes (since section length includes padding -- silly Sigma)
                    let mut nd = val(&new_data);
                    if re(r"\0+$", "").subst(&mut nd, false, |_| Vec::new()) > 0 {
                        *new_data.borrow_mut() = nd;
                    }
                    // write new data if anything changed, otherwise copy old image
                    let out_pt = if et.get("CHANGED").truthy() { &new_data } else { &buff };
                    if !write(it, &outfile, vec![val(out_pt)])? {
                        return ret(n(-1));
                    }
                    // set $len to the total subsection data length
                    len = pl::length(&val(out_pt));
                    did_contain = true;
                } else if !write(it, &outfile, vec![val(&buff)])? {
                    return ret(n(-1));
                }
            } else {
                // copy original image data
                if !pl::call(it, "Image::ExifTool::CopyBlock", vec![raf.clone(), outfile.clone(), len.clone()])?.truthy() {
                    return err("Corrupted X3F image");
                }
            }
            len = pl::add(&len, &n(28)); // add back header length
        } else {
            // copy data for this subsection
            if !pl::call(it, "Image::ExifTool::CopyBlock", vec![raf.clone(), outfile.clone(), len.clone()])?.truthy() {
                return err("Corrupted X3F directory");
            }
        }
        // pad data to an even 4-byte boundary
        // (stored length includes padding! ref Sigma engineer Yuki Miyahara)
        let low = ops::bit_and(&len, &n(0x03));
        if low.truthy() {
            let pad = pl::sub(&n(4), &low);
            if !write(it, &outfile, vec![Value::bytes(vec![0u8; pad.to_i64() as usize])])? {
                return ret(n(-1));
            }
            len = pl::add(&len, &pad);
        }
        // update footer entry with new offset/size
        let packed = pl::pack("V2", &[out_pos.clone(), len.clone()])?;
        let nd = substr_assign(&val(&dir), pos, 8, &packed)?;
        *dir.borrow_mut() = nd;
        out_pos = pl::add(&out_pos, &len);
    }
    // warn if we couldn't add metadata to this image (should only be SD9 or SD10)
    if !did_contain {
        pl::method(it, &et, "Warn", vec![pl::s("Can't yet write SD9 or SD10 X3F images")])?;
    }
    // write out the directory and the directory pointer, and we are done
    let p = pl::pack("V", &[out_pos])?;
    if !write(it, &outfile, vec![val(&hdr), val(&dir), p])? {
        return ret(n(-1));
    }
    pl::ret_undef()
}

//------------------------------------------------------------------------------
/// Process an X3F directory
/// Inputs: 0) ExifTool ref, 1) DirInfo ref, 2) tag table ref
/// Returns: error string or undef on success
pub fn process_x3f_directory(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let raf = dir_info.get("RAF");
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;

    pl::at(522);
    if !seek(it, &raf, dir_info.get("DirStart"), 0)? {
        return err("Error seeking to directory start");
    }

    // parse the X3F directory structure
    let (buff, dir) = (cell(Value::Undef), cell(Value::Undef));
    if !read_eq(it, &raf, &buff, &n(12))? {
        return err("Truncated X3F image");
    }
    if !re(r"^SECd", "").is_match(&val(&buff)) {
        return err("Bad section header");
    }
    let u = pl::unpack("x4V2", &val(&buff))?;
    let (_ver, entries) = (arg(&u, 0), arg(&u, 1));
    if verbose.truthy() {
        pl::method(it, &et, "VerboseDir", vec![pl::s("X3F Subsection"), entries.clone()])?;
    }
    if !read_eq(it, &raf, &dir, &pl::mul(&entries, &n(12)))? {
        return err("Truncated X3F directory");
    }
    let mut index = n(0);
    while pl::lt(&index, &entries) {
        'entry: {
            let pos = pl::mul(&index, &n(12));
            let e = pl::unpack(&format!("x{}V2a4", pos.to_text()), &val(&dir))?;
            let (mut offset, mut len, tag) = (arg(&e, 0), arg(&e, 1), arg(&e, 2));
            let mut tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
            if verbose.truthy() {
                let msg = cat(&[et.get("INDENT"), index.clone(), pl::s(") "), tag.clone(), pl::s(" Subsection ("), len.clone(), pl::s(" bytes):\n")]);
                pl::method(it, &et, "VPrint", vec![n(0), msg])?;
                if pl::gt(&verbose, &n(2)) {
                    if !seek(it, &raf, offset.clone(), 0)? {
                        return err("Error seeking");
                    }
                    if !read_eq(it, &raf, &buff, &len)? {
                        return err("Truncated image");
                    }
                    pl::method(it, &et, "VerboseDump", vec![rf(&buff)])?;
                }
            }
            if !tag_info.truthy() {
                break 'entry;
            }
            if !seek(it, &raf, offset.clone(), 0)? {
                return ret(cat(&[pl::s("Error seeking for "), tag_info.get("Name")]));
            }
            if pl::str_eq(&tag_info.get("Name"), "PreviewImage") {
                // check image header to see if this is a JPEG preview image
                if !read_eq(it, &raf, &buff, &n(28))? {
                    return err("Error reading PreviewImage header");
                }
                offset = pl::add(&offset, &n(28));
                len = pl::sub(&len, &n(28));
                // ignore all image data but JPEG compressed (version 2.0, type 2, format 18)
                if !re(r"^SECi\0\0\x02\0\x02\0\0\0\x12\0\0\0", "").is_match(&val(&buff)) {
                    // do hash on non-preview data if requested
                    if et.get("ImageDataHash").truthy() && !pl::str_eq(&substr(it, &val(&buff), &n(8), Some(&n(1)))?, "\x02") {
                        pl::method(it, &et, "ImageDataHash", vec![raf.clone(), len.clone(), pl::s("SigmaRaw IMAG")])?;
                    }
                    break 'entry;
                }
                if !read_eq(it, &raf, &buff, &len)? {
                    return err("Error reading PreviewImage data");
                }
                // check fore EXIF segment, and extract this image as the JpgFromRaw
                if re(r"^\xff\xd8\xff\xe1", "").is_match(&val(&buff)) {
                    et.set("IsJpgFromRaw", n(1));
                    tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
                    et.delete("IsJpgFromRaw");
                }
            } else if !read_eq(it, &raf, &buff, &len)? {
                return ret(cat(&[pl::s("Error reading "), tag_info.get("Name"), pl::s(" data")]));
            }
            let subdir = tag_info.get("SubDirectory");
            if subdir.truthy() {
                let dir_info = hash(vec![("DataPt", rf(&buff))]);
                let sub_table = call(it, "GetTagTable", vec![subdir.get("TagTable")])?;
                pl::method(it, &et, "ProcessDirectory", vec![dir_info, sub_table])?;
            } else {
                // extract metadata from JpgFromRaw
                if pl::str_eq(&tag_info.get("Name"), "JpgFromRaw") {
                    let jraf = new_raf(it, rf(&buff))?;
                    let dir_info = hash(vec![("Parent", pl::s("X3F")), ("RAF", jraf)]);
                    et.set("BASE", pl::add(&et.get("BASE"), &offset));
                    pl::method(it, &et, "ProcessJPEG", vec![dir_info])?;
                    et.set("BASE", pl::sub(&et.get("BASE"), &offset));
                    call(it, "SetByteOrder", vec![pl::s("II")])?;
                }
                pl::method(it, &et, "FoundTag", vec![tag_info, val(&buff)])?;
            }
        }
        index = pl::add(&index, &n(1));
    }
    pl::ret_undef()
}

//------------------------------------------------------------------------------
/// Read/write information from a Sigma raw (X3F) image
/// Inputs: 0) ExifTool ref, 1) DirInfo ref
/// Returns: 1 on success, 0 if this wasn't a valid X3F image, or -1 on write error
pub fn process_x3f(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let outfile = dir_info.get("OutFile");
    let raf = dir_info.get("RAF");
    let warn_name = if outfile.truthy() { "Image::ExifTool::Error" } else { "Image::ExifTool::Warn" };
    let warn = |it: &mut Interp, args: Vec<Value>| -> ER<()> {
        let mut a = vec![et.clone()];
        a.extend(args);
        pl::call(it, warn_name, a)?;
        Ok(())
    };
    let buff = cell(Value::Undef);
    let mut hdr_len = Value::Undef;

    pl::at(603);
    if !read_eq(it, &raf, &buff, &n(40))? {
        return ret(n(0));
    }
    if !re(r"^FOVb", "").is_match(&val(&buff)) {
        return ret(n(0));
    }

    call(it, "SetByteOrder", vec![pl::s("II")])?;
    pl::method(it, &et, "SetFileType", vec![])?;

    // check version number
    let mut ver = pl::unpack1("x4V", &val(&buff))?;
    ver = cat(&[ops::shift_right(&ver, &n(16)), pl::s("."), ops::bit_and(&ver, &n(0xffff))]);
    if pl::gt(&ver, &n(5)) {
        warn(it, vec![cat(&[pl::s("Untested X3F version ("), ver.clone(), pl::s("). Please submit sample for testing")]), n(1)])?;
    }
    // read version 2.1/2.2/2.3 extended header
    if pl::gt(&ver, &n(2)) {
        let extra;
        let buf2 = cell(Value::Undef);
        if pl::ge(&ver, &n(4)) {
            hdr_len = n(0x300);
            extra = n(0);
        } else {
            hdr_len = if pl::gt(&ver, &Value::Num(2.2)) { n(104) } else { n(72) }; // SceneCaptureType string added in 2.3
            extra = n(160); // (extended header is 160 bytes)
        }
        let more = pl::add(&pl::sub(&hdr_len, &pl::length(&val(&buff))), &extra);
        if !read_eq(it, &raf, &buf2, &more)? {
            warn(it, vec![pl::s("Error reading X3F header")])?;
            return ret(n(1));
        }
        let b = pl::concat(&val(&buff), &val(&buf2));
        *buff.borrow_mut() = b;
    }
    let (_wid_pos, hdr_type) = if pl::lt(&ver, &n(4)) { (28, "Header") } else { (40, "Header4") };
    // process header information
    let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::SigmaRaw::Main")])?;
    if !outfile.truthy() {
        pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s(hdr_type), val(&buff), pl::s("DataPt"), rf(&buff), pl::s("Size"), hdr_len.clone()])?;
    }
    // read the directory pointer
    if !seek(it, &raf, n(-4), 2)? {
        warn(it, vec![pl::s("Seek error")])?;
        return ret(n(1));
    }
    if !read_eq(it, &raf, &buff, &n(4))? {
        warn(it, vec![pl::s("Error reading X3F dir pointer")])?;
        return ret(n(1));
    }
    let offset = pl::unpack1("V", &val(&buff))?;
    let dir_info = hash(vec![("RAF", raf.clone()), ("DirStart", offset)]);
    let error;
    if outfile.truthy() {
        dir_info.set("OutFile", outfile.clone());
        error = write_x3f(it, vec![et.clone(), dir_info], Want::Scalar)?.pop().unwrap_or_default();
        if error.truthy() && pl::str_eq(&error, "-1") {
            return ret(n(-1));
        }
    } else {
        // process the X3F subsections
        error = pl::method(it, &et, "ProcessDirectory", vec![dir_info, tag_table_ptr])?;
    }
    if error.truthy() {
        warn(it, vec![error])?;
    }
    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::SigmaRaw::ExtractUnicodeString", extract_unicode_string);
    it.define_native("Image::ExifTool::SigmaRaw::ProcessX3FHeader", process_x3f_header);
    it.define_native("Image::ExifTool::SigmaRaw::ProcessX3FProperties", process_x3f_properties);
    it.define_native("Image::ExifTool::SigmaRaw::WriteX3F", write_x3f);
    it.define_native("Image::ExifTool::SigmaRaw::ProcessX3FDirectory", process_x3f_directory);
    it.define_native("Image::ExifTool::SigmaRaw::ProcessX3F", process_x3f);
}
