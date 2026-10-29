//! Image::ExifTool::BigTIFF (Image/ExifTool/BigTIFF.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read Big TIFF meta information

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::cell;
use crate::modules::write_exif::{etf, g16u, get, len, read_eq, read_value, seek, set_byte_order, tell};
use crate::perl::Value;
use crate::pl::{self, arg, cap, re, ret};

fn n(i: i64) -> Value {
    Value::Int(i)
}

fn s(x: &str) -> Value {
    Value::str(x)
}

/// `$maxOffset`: currently supported maximum data offset/size
fn max_offset() -> Value {
    n(0x7fffffff)
}

/// `Image::ExifTool::Get64u($dataPt, $pos)`
fn g64u(it: &mut Interp, pt: &Value, pos: Value) -> ER<Value> {
    etf(it, "Get64u", vec![pt.clone(), pos])
}

/// `"$a$b..."`
fn cat(parts: &[&Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

/// `$et->Warn(MSG)`
fn warn(it: &mut Interp, et: &Value, msg: Value) -> ER<()> {
    pl::method(it, et, "Warn", vec![msg])?;
    Ok(())
}

/// Process Big IFD directory
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success, otherwise returns 0 and sets a Warning
pub fn process_big_ifd(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let raf = dir_info.get("RAF");
    let mut verbose = et.get("OPTIONS").get("Verbose");
    let html_dump = et.get("HTML_DUMP");
    let mut dir_name = dir_info.get("DirName");
    let mut dir_start = dir_info.get("DirStart");
    let (mut off_name, mut next_off_name) = (Value::Undef, Value::Undef);
    if html_dump.truthy() {
        verbose = n(-1); // mix htmlDump into verbose so we can test for both at once
        off_name = dir_info.get("OffsetName");
    }
    let format_size = pl::pkg_array(it, "Image::ExifTool::Exif::formatSize");
    let format_name = pl::pkg_array(it, "Image::ExifTool::Exif::formatName");
    // loop through IFD chain
    loop {
        if pl::gt(&dir_start, &max_offset()) && !pl::method(it, &et, "Options", vec![s("LargeFileSupport")])?.truthy() {
            warn(it, &et, s("Huge offsets not supported (LargeFileSupport not set)"))?;
            break;
        }
        if !seek(it, &raf, dir_start.clone(), 0)? {
            warn(it, &et, cat(&[&s("Bad "), &dir_name, &s(" offset")]))?;
            return ret(n(0));
        }
        let dir_buff = cell(Value::Undef);
        if !read_eq(it, &raf, &dir_buff, &n(8))? {
            warn(it, &et, cat(&[&s("Truncated "), &dir_name, &s(" count")]))?;
            return ret(n(0));
        }
        let num_entries = g64u(it, &Value::ScalarRef(dir_buff.clone()), n(0))?;
        if pl::gt(&verbose, &n(0)) {
            pl::method(it, &et, "VerboseDir", vec![dir_name.clone(), num_entries.clone()])?;
        }
        let bsize = pl::mul(&num_entries, &n(20));
        if pl::gt(&bsize, &max_offset()) {
            warn(it, &et, s("Huge directory counts not yet supported"))?;
            break;
        }
        let buf_pos = tell(it, &raf)?;
        if !read_eq(it, &raf, &dir_buff, &bsize)? {
            warn(it, &et, cat(&[&s("Truncated "), &dir_name, &s(" directory")]))?;
            return ret(n(0));
        }
        let next_ifd_c = cell(Value::Undef);
        if !read_eq(it, &raf, &next_ifd_c, &n(8))? {
            *next_ifd_c.borrow_mut() = Value::Undef; // try to read next IFD pointer
        }
        let next_ifd = get(&next_ifd_c);
        let dir_buff_pt = Value::ScalarRef(dir_buff.clone());
        if html_dump.truthy() {
            pl::method(it, &et, "HDump", vec![pl::sub(&buf_pos, &n(8)), n(8), cat(&[&dir_name, &s(" entries")]), cat(&[&s("Entry count: "), &num_entries]), Value::Undef, off_name.clone()])?;
            if !next_ifd.is_undef() {
                let off = g64u(it, &Value::ScalarRef(next_ifd_c.clone()), n(0))?;
                let tip = pl::sprintf("Offset: 0x%.8x", &[off.clone()]);
                let mut id = off_name.clone();
                if off.truthy() {
                    let r = pl::call_list(it, "Image::ExifTool::Exif::NextOffsetName", vec![et.clone(), id.clone()])?;
                    next_off_name = arg(&r, 0);
                    id = arg(&r, 1);
                }
                pl::method(it, &et, "HDump", vec![pl::add(&buf_pos, &pl::mul(&n(20), &num_entries)), n(8), s("Next IFD"), tip, n(0), id])?;
            }
        }
        // loop through all entries in this BigTIFF IFD
        let mut index = n(0);
        while pl::lt(&index, &num_entries) {
            let entry = pl::mul(&n(20), &index);
            let tag_id = g16u(it, &dir_buff_pt, entry.clone())?;
            let format = g16u(it, &dir_buff_pt, pl::add(&entry, &n(2)))?;
            let count = g64u(it, &dir_buff_pt, pl::add(&entry, &n(4)))?;
            let fsz = format_size.at(format.to_i64());
            if fsz.is_undef() {
                pl::method(it, &et, "HDump", vec![pl::add(&buf_pos, &entry), n(20), s("[invalid IFD entry]"), cat(&[&s("Bad format value: "), &format]), n(1), off_name.clone()])?;
                // warn unless the IFD was just padded with zeros
                let fmt = cat(&[&s("Unknown format ("), &format, &s(") for "), &dir_name, &s(" tag 0x%x")]);
                let m = pl::sprintf_v(&fmt, &[tag_id.clone()]);
                warn(it, &et, m)?;
                return ret(n(0)); // assume corrupted IFD
            }
            let format_str = format_name.at(format.to_i64());
            let size = pl::mul(&count, &fsz);
            let mut tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag_id.clone()])?;
            if !(!tag_info.is_undef() || verbose.truthy()) {
                index = pl::add(&index, &n(1));
                continue;
            }
            let mut value_ptr = pl::add(&entry, &n(12));
            let val_buff = cell(Value::Undef);
            let (val_base, mut sub_off_name);
            sub_off_name = Value::Undef;
            if pl::gt(&size, &n(8)) {
                if pl::gt(&size, &max_offset()) {
                    warn(it, &et, cat(&[&s("Can't handle "), &dir_name, &s(" entry "), &index, &s(" (huge size)")]))?;
                    index = pl::add(&index, &n(1));
                    continue;
                }
                value_ptr = g64u(it, &dir_buff_pt, value_ptr)?;
                if pl::gt(&value_ptr, &max_offset()) && !pl::method(it, &et, "Options", vec![s("LargeFileSupport")])?.truthy() {
                    warn(it, &et, cat(&[&s("Can't handle "), &dir_name, &s(" entry "), &index, &s(" (LargeFileSupport not set)")]))?;
                    index = pl::add(&index, &n(1));
                    continue;
                }
                if !(seek(it, &raf, value_ptr.clone(), 0)? && read_eq(it, &raf, &val_buff, &size)?) {
                    warn(it, &et, cat(&[&s("Error reading "), &dir_name, &s(" entry "), &index]))?;
                    index = pl::add(&index, &n(1));
                    continue;
                }
                val_base = n(0);
            } else {
                *val_buff.borrow_mut() = crate::modules::write_exif::substr(it, &get(&dir_buff), &value_ptr, Some(&size))?;
                val_base = buf_pos.clone();
            }
            let val_buff_pt = Value::ScalarRef(val_buff.clone());
            if !tag_info.is_undef() && !tag_info.truthy() {
                // GetTagInfo() required the value for a Condition
                tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag_id.clone(), val_buff_pt.clone()])?;
            }
            let rational = cell(Value::Undef);
            let val = read_value(it, vec![val_buff_pt.clone(), n(0), format_str.clone(), count.clone(), size.clone(), Value::ScalarRef(rational.clone())])?;
            if html_dump.truthy() {
                let mut tval = val.clone();
                // show numerator/denominator separately for rational numbers
                let rat = get(&rational);
                if !rat.is_undef() {
                    tval = cat(&[&tval, &s(" ("), &rat, &s(")")]);
                }
                let tag_name;
                let col_name;
                if pl::num_eq(&tag_id, &n(0x927c)) && dir_name.eq_str("ExifIFD") {
                    tag_name = s("MakerNotes");
                } else if tag_info.truthy() {
                    tag_name = tag_info.get("Name");
                } else {
                    tag_name = pl::sprintf("Tag 0x%.4x", &[tag_id.clone()]);
                }
                let dname = pl::sprintf_v(&cat(&[&dir_name, &s("-%.2d")]), &[index.clone()]);
                // build our tool tip
                let mut tip = cat(&[&pl::sprintf("Tag ID: 0x%.4x\n", &[tag_id.clone()]), &s("Format: "), &format_str, &s("["), &count, &s("]\nSize: "), &size, &s(" bytes\n")]);
                if pl::gt(&size, &n(8)) {
                    tip = cat(&[&tip, &pl::sprintf("Value offset: 0x%.8x\n", &[value_ptr.clone()])]);
                    col_name = cat(&[&s("<span class=H>"), &tag_name, &s("</span>")]);
                } else {
                    col_name = tag_name.clone();
                }
                if len(&tval).to_i64() > 32 {
                    tval = cat(&[&pl::substr(&tval, 0, Some(28)), &s("[...]")]);
                }
                if re(r"^(string|undef|binary)", "").is_match(&format_str) {
                    // translate non-printable characters
                    pl::tr(&mut tval, "\\x00-\\x1f\\x7f-\\xff", ".", "");
                } else if tag_info.truthy() && pl::call(it, "Image::ExifTool::IsInt", vec![tval.clone()])?.truthy() {
                    if tag_info.get("IsOffset").truthy() {
                        tval = pl::sprintf("0x%.4x", &[tval]);
                    } else if tag_info.get("PrintHex").truthy() {
                        tval = pl::sprintf("0x%x", &[tval]);
                    }
                }
                tip = cat(&[&tip, &s("Value: "), &tval]);
                let mut sid = Value::Undef;
                let id = if tag_info.truthy() && tag_info.get("SubIFD").truthy() {
                    let r = pl::call_list(it, "Image::ExifTool::Exif::NextOffsetName", vec![et.clone(), off_name.clone()])?;
                    sub_off_name = arg(&r, 0);
                    sid = arg(&r, 2);
                    arg(&r, 1)
                } else {
                    off_name.clone()
                };
                pl::method(it, &et, "HDump", vec![pl::add(&entry, &buf_pos), n(20), cat(&[&dname, &s(" "), &col_name]), tip, n(1), id])?;
                if pl::gt(&size, &n(8)) {
                    // add value data block
                    let flg = if tag_info.truthy() && tag_info.get("SubDirectory").truthy() && tag_info.get("MakerNotes").truthy() { n(4) } else { n(0) };
                    pl::method(it, &et, "HDump", vec![value_ptr.clone(), size.clone(), cat(&[&tag_name, &s(" value")]), s("SAME"), flg, sid])?;
                }
            }
            if tag_info.truthy() && tag_info.get("SubIFD").truthy() {
                // process all SubIFD's as BigTIFF
                if pl::gt(&verbose, &n(0)) {
                    pl::method(
                        it,
                        &et,
                        "VerboseInfo",
                        vec![
                            tag_id.clone(),
                            tag_info.clone(),
                            s("Table"),
                            tag_table_ptr.clone(),
                            s("Index"),
                            index.clone(),
                            s("Value"),
                            val.clone(),
                            s("DataPt"),
                            val_buff_pt.clone(),
                            s("DataPos"),
                            pl::add(&val_base, &value_ptr),
                            s("Start"),
                            n(0),
                            s("Size"),
                            size.clone(),
                            s("Format"),
                            format_str.clone(),
                            s("Count"),
                            count.clone(),
                        ],
                    )?;
                }
                let offsets = pl::split_ws(&val);
                let proc_ref = Value::Code(it.sub_ref("Image::ExifTool::BigTIFF::ProcessBigIFD"));
                for (i, off) in offsets.iter().enumerate() {
                    let mut subdir_name = tag_info.get("Name");
                    if i > 0 {
                        subdir_name = cat(&[&subdir_name, &n(i as i64)]);
                    }
                    let subdir_info = Value::new_hash();
                    subdir_info.set("RAF", raf.clone());
                    subdir_info.set("DataPos", n(0));
                    subdir_info.set("DirStart", off.clone());
                    subdir_info.set("DirName", subdir_name);
                    subdir_info.set("Parent", dir_name.clone());
                    subdir_info.set("OffsetName", sub_off_name.clone());
                    pl::method(it, &et, "ProcessDirectory", vec![subdir_info, tag_table_ptr.clone(), proc_ref.clone()])?;
                }
            } else {
                let tag_key = pl::method(
                    it,
                    &et,
                    "HandleTag",
                    vec![
                        tag_table_ptr.clone(),
                        tag_id.clone(),
                        val.clone(),
                        s("Index"),
                        index.clone(),
                        s("DataPt"),
                        val_buff_pt.clone(),
                        s("DataPos"),
                        pl::add(&val_base, &value_ptr),
                        s("Start"),
                        n(0),
                        s("Size"),
                        size.clone(),
                        s("Format"),
                        format_str.clone(),
                        s("TagInfo"),
                        tag_info.clone(),
                        s("RAF"),
                        raf.clone(),
                    ],
                )?;
                if tag_key.truthy() {
                    pl::method(it, &et, "SetGroup", vec![tag_key, dir_name.clone()])?;
                }
            }
            index = pl::add(&index, &n(1));
        }
        let Some(c) = re(r"^(IFD|SubIFD)(\d*)$", "").m(&dir_name) else { break };
        let c2 = cap(&c, 2);
        dir_name = cat(&[&cap(&c, 1), &pl::add(&(if c2.truthy() { c2 } else { n(0) }), &n(1))]);
        if next_ifd.is_undef() {
            warn(it, &et, cat(&[&s("Bad "), &dir_name, &s(" pointer")]))?;
            return ret(n(0));
        }
        dir_start = g64u(it, &Value::ScalarRef(next_ifd_c.clone()), n(0))?;
        if !dir_start.truthy() {
            break;
        }
        off_name = next_off_name.clone();
        // protect against infinite loop
        let processed = et.get_or_new_hash("PROCESSED");
        let ds = dir_start.to_bytes();
        if processed.get(&ds).truthy() {
            warn(it, &et, cat(&[&dir_name, &s(" pointer references previous "), &processed.get(&ds), &s(" directory")]))?;
            break;
        } else {
            processed.set(&ds, dir_name.clone());
        }
    }
    ret(n(1))
}

/// Extract meta information from a BigTIFF image
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid BigTIFF image
pub fn process_btf(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let buff = cell(Value::Undef);
    if !read_eq(it, &raf, &buff, &n(16))? {
        return ret(n(0));
    }
    if !re(r"^(MM\0\x2b\0\x08\0\0|II\x2b\0\x08\0\0\0)", "").is_match(&get(&buff)) {
        return ret(n(0));
    }
    if dir_info.get("OutFile").truthy() {
        pl::method(it, &et, "Error", vec![s("ExifTool does not support writing of BigTIFF images")])?;
        return ret(n(1));
    }
    pl::method(it, &et, "SetFileType", vec![s("BTF")])?; // set the FileType tag
    set_byte_order(it, pl::substr(&get(&buff), 0, Some(2)))?;
    let offset = g64u(it, &Value::ScalarRef(buff.clone()), n(8))?;
    if et.get("HTML_DUMP").truthy() {
        let o = if crate::modules::write_exif::byte_order(it)?.eq_str("II") { s("Little") } else { s("Big") };
        pl::method(it, &et, "HDump", vec![n(0), n(8), s("BigTIFF header"), cat(&[&s("Byte order: "), &o, &s(" endian")]), n(0)])?;
        pl::method(it, &et, "HDump", vec![n(8), n(8), s("IFD0 pointer"), pl::sprintf("Offset: 0x%.8x", &[offset.clone()]), n(0)])?;
    }
    let dir_info2 = Value::new_hash();
    dir_info2.set("RAF", raf);
    dir_info2.set("DataPos", n(0));
    dir_info2.set("DirStart", offset);
    dir_info2.set("DirName", s("IFD0"));
    dir_info2.set("Parent", s("BigTIFF"));
    let tag_table_ptr = etf(it, "GetTagTable", vec![s("Image::ExifTool::Exif::Main")])?;
    let proc_ref = Value::Code(it.sub_ref("Image::ExifTool::BigTIFF::ProcessBigIFD"));
    pl::method(it, &et, "ProcessDirectory", vec![dir_info2, tag_table_ptr, proc_ref])?;
    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::BigTIFF::ProcessBigIFD", process_big_ifd);
    it.define_native("Image::ExifTool::BigTIFF::ProcessBTF", process_btf);
}
