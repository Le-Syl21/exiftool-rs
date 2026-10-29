//! Image::ExifTool::Samsung (Image/ExifTool/Samsung.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! The tag tables are data; these are the module's subs: the NX10
//! encryption, the MP4 INFO and trailer meta readers, the maker note IFD
//! fix-up, the "Sound & Shot" trailer reader/writer and the STMN writer.

use std::cell::RefCell;
use std::rc::Rc;

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::perl::Value;
use crate::pl::{self, arg, cap, re, ret};

const MODULE: &str = "Image::ExifTool::Samsung";

fn n(i: i64) -> Value {
    Value::Int(i)
}

fn s(x: &str) -> Value {
    pl::s(x)
}

// ---- local helpers ----

fn get16u(it: &mut Interp, data_pt: &Value, pos: Value) -> ER<Value> {
    pl::call(it, "Image::ExifTool::Get16u", vec![data_pt.clone(), pos])
}

fn get32u(it: &mut Interp, data_pt: &Value, pos: Value) -> ER<Value> {
    pl::call(it, "Image::ExifTool::Get32u", vec![data_pt.clone(), pos])
}

/// `$raf->Read($buff, $len) == $len`, `buff` updated.
fn raf_read_ok(it: &mut Interp, raf: &Value, buff: &mut Value, len: &Value) -> ER<bool> {
    let mut args = [buff.clone(), len.clone()];
    let r = pl::method_mut(it, raf, "Read", &mut args, Want::Scalar)?;
    *buff = args[0].clone();
    Ok(pl::num_eq(&r.last().cloned().unwrap_or_default(), len))
}

/// `$raf->Read($$cell, $len) == $len` for a buffer kept in a cell (`\$buf`
/// is handed out).
fn raf_read_cell(it: &mut Interp, raf: &Value, cell: &Value, len: &Value) -> ER<bool> {
    let mut b = cell.deref();
    let ok = raf_read_ok(it, raf, &mut b, len)?;
    pl::set_deref(cell, b)?;
    Ok(ok)
}

fn raf_seek(it: &mut Interp, raf: &Value, pos: Value, whence: i64) -> ER<bool> {
    Ok(pl::method(it, raf, "Seek", vec![pos, n(whence)])?.truthy())
}

fn raf_tell(it: &mut Interp, raf: &Value) -> ER<Value> {
    pl::method(it, raf, "Tell", vec![])
}

/// `substr($$ref, OFF, LEN) = VALUE`
fn substr_assign(r: &Value, off: i64, len: i64, v: &Value) -> ER<()> {
    let st = r.deref().to_bytes();
    let Some((a, b)) = crate::interp::eval::substr_range(st.len(), off, Some(len)) else {
        return crate::interp::eval::die("substr outside of string");
    };
    let mut out = st[..a].to_vec();
    out.extend_from_slice(&v.to_bytes());
    out.extend_from_slice(&st[b..]);
    pl::set_deref(r, Value::bytes(out))
}

//------------------------------------------------------------------------------
/// Encrypt/Decrypt NX10 information
/// Inputs: 0) ExifTool ref, 1) value as a string of integers,
///         2) tagInfo hash ref, 3-N) encryption salt values
/// Returns: encrypted/decrypted value
/// Notes:
/// 1) The encryption salt starts with '-' to reverse the encryption algorithm
/// 2) Additional salt values are provided when tag stores multiple arrays
///    (in which case the first value of the tag gives the array length)
pub fn crypt(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, val, tag_info) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let mut salt_list: Vec<Value> = a.iter().skip(3).cloned().collect();
    let key = et.get("EncryptionKey");
    if !key.truthy() {
        return pl::ret_undef();
    }
    let mut format = tag_info.get("Writable");
    if !format.truthy() {
        format = tag_info.get("Format");
    }
    if !format.truthy() {
        return pl::ret_undef();
    }
    let format_min_max = crate::runtime::lex_hash(it, MODULE, "formatMinMax");
    let mm = format_min_max.get(format.to_bytes());
    if !mm.truthy() {
        return pl::ret_undef();
    }
    let (min, max) = (mm.at(0), mm.at(1));
    let mut av = pl::split_ws(&val);
    let mut new_salt = n(if salt_list.len() > 1 { 1 } else { 0 }); // skip length entry if this is an array
    let (mut sign, mut salt, mut start) = (Value::Undef, Value::Undef, Value::Undef);
    let range = pl::add(&pl::sub(&max, &min), &n(1));
    let mut i = new_salt.clone();
    while pl::lt(&i, &n(av.len() as i64)) {
        if pl::num_eq(&i, &new_salt) {
            start = i.clone();
            salt = if salt_list.is_empty() { Value::Undef } else { salt_list.remove(0) };
            sign = if re(r"^-", "").subst(&mut salt, false, |_| Vec::new()) > 0 { n(-1) } else { n(1) };
            if !salt_list.is_empty() {
                new_salt = pl::add(&new_salt, &arg(&av, 0));
            }
        }
        pl::at(1596);
        let idx = pl::modulo(&pl::sub(&pl::add(&salt, &i), &start), &n(pl::aderef(&key)?.len() as i64))?;
        let k = key.at(idx.to_i64());
        let ii = i.to_i64() as usize;
        av[ii] = pl::add(&av[ii], &pl::mul(&sign, &k));
        // handle integer wrap-around
        if pl::gt(&sign, &n(0)) {
            if pl::gt(&av[ii], &max) {
                av[ii] = pl::sub(&av[ii], &range);
            }
        } else if pl::lt(&av[ii], &min) {
            av[ii] = pl::add(&av[ii], &range);
        }
        i = pl::add(&i, &n(1));
    }
    ret(pl::join(" ", &av))
}

//------------------------------------------------------------------------------
/// Process Samsung MP4 INFO data
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_info(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let mut pos = dir_info.get("DirStart");
    let len = dir_info.get("DirLen");
    let end = pl::add(&pos, &len);
    pl::method(it, &et, "VerboseDir", vec![s("INFO"), Value::Undef, len.clone()])?;
    while pl::le(&pl::add(&pos, &n(8)), &end) {
        pl::at(1620);
        let tag = pl::substr(&pl::sderef(&data_pt)?, pos.to_i64(), Some(4));
        let val = get32u(it, &data_pt, pl::add(&pos, &n(4)))?;
        if !tag_table_ptr.get(tag.to_bytes()).truthy() {
            let mut name = pl::concat(&s("Samsung_INFO_"), &tag);
            pl::tr(&mut name, "-_0-9a-zA-Z", "", "dc");
            if name.truthy() {
                let ti = Value::new_hash();
                ti.set("Name", name);
                pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), ti])?;
            }
        }
        pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tag, val])?;
        pos = pl::add(&pos, &n(8));
    }
    ret(n(1))
}

//------------------------------------------------------------------------------
/// Read Samsung Meta Info from trailer
/// Inputs: 0) ExifTool object ref, 1) source dirInfo ref, 2) tag table ref
/// Returns: true on success
pub fn process_samsung_meta(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let dir_name = dir_info.get("DirName");
    let data_pt = dir_info.get("DataPt");
    // ($pos is a single variable: a reference to it is taken below)
    let pos_cell = Rc::new(RefCell::new(dir_info.get("DirStart")));
    let pos = || pos_cell.borrow().clone();
    let set_pos = |v: Value| *pos_cell.borrow_mut() = v;
    let end = pl::add(&dir_info.get("DirLen"), &pos());
    if !(pl::le(&pl::add(&pos(), &n(8)), &end) && {
        pl::at(1644);
        pl::str_eq(&pl::substr(&pl::sderef(&data_pt)?, pos().to_i64(), Some(4)), "DOFS")
    }) {
        let msg = pl::concat(&pl::concat(&s("Unrecognized "), &dir_name), &s(" data"));
        pl::method(it, &et, "Warn", vec![msg, n(1)])?;
        return ret(n(0));
    }
    let ver = get32u(it, &data_pt, pl::add(&pos(), &n(4)))?;
    if pl::num_eq(&ver, &n(3)) {
        if !(pl::le(&pl::add(&pos(), &n(18)), &end) && pl::num_eq(&get32u(it, &data_pt, pl::add(&pos(), &n(12)))?, &dir_info.get("DirLen"))) {
            let msg = pl::concat(&pl::concat(&pl::concat(&pl::concat(&s("Unrecognized "), &dir_name), &s(" version ")), &ver), &s(" data"));
            pl::method(it, &et, "Warn", vec![msg])?;
            return ret(n(0));
        }
        let num = get16u(it, &data_pt, pl::add(&pos(), &n(16)))?;
        let dn = pl::concat(&pl::concat(&dir_name, &s(" version ")), &ver);
        pl::method(it, &et, "VerboseDir", vec![dn, num.clone()])?;
        set_pos(pl::add(&pos(), &n(18)));
        let mut val;
        let mut i = n(0);
        while pl::lt(&i, &num) {
            if pl::gt(&pl::add(&pos(), &n(2)), &end) {
                break;
            }
            let t = format!("x{}CC", pos().to_text());
            let xn = pl::unpack(&t, &data_pt.deref())?;
            let nn = arg(&xn, 1);
            set_pos(pl::add(&pos(), &n(2)));
            if pl::gt(&pl::add(&pl::add(&pos(), &nn), &n(2)), &end) {
                break;
            }
            let tag = pl::substr(&data_pt.deref(), pos().to_i64(), Some(nn.to_i64()));
            let len = get16u(it, &data_pt, pl::add(&pos(), &nn))?;
            set_pos(pl::add(&pos(), &pl::add(&nn, &n(2))));
            if pl::gt(&pl::add(&pos(), &len), &end) {
                break;
            }
            if pl::num_eq(&len, &n(4)) {
                val = get32u(it, &data_pt, pos())?;
            } else {
                let _tmp = pl::substr(&data_pt.deref(), pos().to_i64(), Some(len.to_i64()));
                val = Value::ScalarRef(pos_cell.clone());
            }
            pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tag, val])?;
            set_pos(pl::add(&pos(), &len));
            i = pl::add(&i, &n(1));
        }
        if pl::lt(&i, &num) {
            let msg = [&b"Unexpected end of "[..], &dir_name.to_bytes(), b" version ", &ver.to_bytes(), b" ", &i.to_bytes(), b" ", &num.to_bytes(), b" data"].concat();
            pl::method(it, &et, "Warn", vec![Value::bytes(msg)])?;
        }
    }
    ret(n(1))
}

//------------------------------------------------------------------------------
/// Inputs: 0) ExifTool object ref, 1) source dirInfo ref, 2) tag table ref
/// Returns: true on success
pub fn process_samsung_ifd(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let len = dir_info.get("DataLen");
    let pos = dir_info.get("DirStart");
    if !pl::lt(&pl::add(&pos, &n(4)), &len) {
        return ret(n(0));
    }
    let data_pt = dir_info.get("DataPt");
    pl::at(1691);
    let mut buff = pl::substr(&pl::sderef(&data_pt)?, pos.to_i64(), Some(4));
    // this is not an IFD for many models
    // (the string "Park Byeongchan" is often found here)
    let mut c1 = Value::Undef;
    if re(r"^([^\0])\0\0\0", "s").subst(&mut buff, false, |c| {
        c1 = cap(c, 1);
        let b = c1.to_bytes();
        [&b[..], b"\0", &b[..], b"\0"].concat()
    }) == 0
    {
        return ret(n(0));
    }
    let num_entries = n(c1.to_bytes().first().copied().unwrap_or(0) as i64);
    if et.get("HTML_DUMP").truthy() {
        let pt = pl::add(&pl::add(&dir_info.get("DirStart"), &dir_info.get("DataPos")), &dir_info.get("Base"));
        pl::method(it, &et, "HDump", vec![pl::sub(&pt, &n(44)), n(44), s("MakerNotes header"), s("Samsung")])?;
        let msg = pl::concat(&s("Format: int32u\nEntry count: "), &num_entries);
        pl::method(it, &et, "HDump", vec![pt, n(4), s("MakerNotes entries"), msg])?;
        dir_info.set("NoDumpEntryCount", n(1));
    }
    pl::at(1702);
    substr_assign(&data_pt, pos.to_i64(), 4, &buff)?; // insert bogus 2-byte entry count
    // offset base is at end of IFD
    let shift = pl::add(&pl::add(&pl::add(&dir_info.get("DirStart"), &n(4)), &pl::mul(&num_entries, &n(12))), &n(4));
    dir_info.set("Base", pl::add(&dir_info.get("Base"), &shift));
    dir_info.set("DataPos", pl::sub(&dir_info.get("DataPos"), &shift));
    dir_info.set("DirStart", pl::add(&dir_info.get("DirStart"), &n(2))); // start at bogus entry count
    dir_info.set("ZeroOffsetOK", n(1)); // disable check for zero offset
    et.delete("NO_UNKNOWN"); // (set for BinaryData, but not for EXIF IFD's)
    let rtn = pl::call(it, "Image::ExifTool::Exif::ProcessExif", vec![et.clone(), dir_info.clone(), tag_table_ptr])?;
    pl::at(1711);
    substr_assign(&data_pt, pos.to_i64() + 2, 1, &s("\0"))?; // remove bogus count
    ret(rtn)
}

//------------------------------------------------------------------------------
/// Read/write Samsung trailer (ie. "Sound & Shot" written by Galaxy Tab S (SM-T805))
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 not valid Samsung trailer, or -1 error writing
/// - updates DataPos to point to start of Samsung trailer
/// - updates DirLen to existing trailer length
pub fn process_samsung(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let mut raf = dir_info.get("RAF");
    let offset = {
        let o = dir_info.get("Offset");
        if o.truthy() {
            o
        } else {
            n(0)
        }
    };
    let outfile = dir_info.get("OutFile");
    let verbose = pl::method(it, &et, "Options", vec![s("Verbose")])?;
    let unknown = pl::method(it, &et, "Options", vec![s("Unknown")])?;
    let mut buff = Value::Undef;
    let buf2 = pl::sref(Value::Undef); // (\$buf2 is handed out)
    let (mut offset_pos, mut audio_n_off, mut audio_size) = (Value::Undef, Value::Undef, Value::Undef);

    if !raf.truthy() {
        raf = pl::method(it, &s("File::RandomAccess"), "new", vec![dir_info.get("DataPt")])?;
        pl::method(it, &et, "VerboseDir", vec![s("SamsungTrailer")])?;
    }
    if !(raf_seek(it, &raf, pl::sub(&n(-6), &offset), 2)?
        && raf_read_ok(it, &raf, &mut buff, &n(6))?
        && (pl::str_eq(&buff, "QDIOBS") || pl::str_eq(&buff, "\0\0SEFT")))
    {
        return ret(n(0));
    }
    let end_pos = raf_tell(it, &raf)?;
    if pl::str_eq(&buff, "QDIOBS") && !raf_seek(it, &raf, n(-2), 1)? {
        return ret(n(0)); // rewind to before 'BS'
    }
    let mut block_end = raf_tell(it, &raf)?;
    pl::call(it, "Image::ExifTool::SetByteOrder", vec![s("II")])?;

    // read blocks backward until we find the SEFH/SEFT block
    // (the only other block I have seen is QDIO/QDIO)
    'sam_block: loop {
        if !(raf_seek(it, &raf, pl::sub(&block_end, &n(8)), 0)? && raf_read_ok(it, &raf, &mut buff, &n(8))?) {
            break;
        }
        let typ = pl::substr(&buff, 4, None);
        if !re(r"^\w+$", "").is_match(&typ) {
            break;
        }
        let mut len = get32u(it, &pl::sref(buff.clone()), n(0))?;
        if !(pl::lt(&len, &n(0x10000)) && pl::ge(&len, &n(4)) && pl::lt(&pl::add(&len, &n(8)), &block_end)) {
            break;
        }
        if !(raf_seek(it, &raf, pl::sub(&n(-8), &len), 1)? && raf_read_ok(it, &raf, &mut buff, &len)?) {
            break;
        }
        block_end = pl::sub(&block_end, &pl::add(&len, &n(8)));
        if !pl::str_eq(&typ, "SEFT") {
            // look for directory block (ends with "SEFT")
            if !(outfile.truthy() && pl::str_eq(&typ, "QDIO")) {
                continue;
            }
            // QDIO block format:
            //   0 - 'QDIO'
            //   4 - int32u: 101 (version)
            //   8 - int32u: 1
            //  12 - int32u: absolute offset of audio file start (augh!!)
            //  16 - int32u: absolute offset of audio file end (augh!!)
            //  20 - int32u: 20 (QDIO block length minus 8)
            //  24 - 'QDIO'
            if pl::num_eq(&len, &n(20)) {
                // save position of audio file offset in QDIO block
                let t = raf_tell(it, &raf)?;
                offset_pos = pl::sub(&pl::add(&pl::sub(&end_pos, &t), &len), &n(12));
            } else {
                pl::method(it, &et, "Error", vec![s("Unsupported Samsung trailer QDIO block"), n(1)])?;
            }
            continue;
        }
        // validate SEFH header
        if !(re(r"^SEFH", "").is_match(&buff) && pl::ge(&len, &n(12))) {
            // tolerate extra junk written by Samsung Gallery
            let b = buff.to_bytes();
            let Some(p) = b.windows(6).position(|w| w == b"\0\0SEFT") else { break };
            pl::method(it, &et, "Warn", vec![s("Trailer likely corrupted by Samsung Gallery")])?;
            block_end = pl::add(&block_end, &n((p + 6) as i64));
            continue;
        }
        let dir_pos = pl::sub(&raf_tell(it, &raf)?, &len);
        // my $ver = Get32u(\$buff, 0x04);  # version (=101)
        let buff_pt = pl::sref(buff.clone());
        let count = get32u(it, &buff_pt, n(0x08))?;
        if pl::gt(&pl::add(&n(12), &pl::mul(&n(12), &count)), &len) {
            break;
        }
        let tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![s("Image::ExifTool::Samsung::Trailer")])?;

        // scan ahead quickly to look for the block where the data comes first
        // (have only seen this to be the first in the directory, but just in case)
        let mut first_block = n(0);
        let mut index = n(0);
        while pl::lt(&index, &count) {
            let entry = pl::add(&n(12), &pl::mul(&n(12), &index));
            let noff = get32u(it, &buff_pt, pl::add(&entry, &n(4)))?; // negative offset
            if pl::lt(&first_block, &noff) {
                first_block = noff;
            }
            index = pl::add(&index, &n(1));
        }
        // save trailer position and length
        let data_pos = pl::sub(&dir_pos, &first_block);
        dir_info.set("DataPos", data_pos.clone());
        let dir_len = pl::sub(&end_pos, &data_pos);
        dir_info.set("DirLen", dir_len.clone());
        if (verbose.truthy() || et.get("HTML_DUMP").truthy()) && !outfile.truthy() && dir_info.get("RAF").truthy() {
            pl::method(it, &et, "DumpTrailer", vec![dir_info.clone()])?;
            if et.get("HTML_DUMP").truthy() {
                return ret(n(1));
            }
        }
        // read through the SEFH/SEFT directory entries
        index = n(0);
        while pl::lt(&index, &count) {
            let entry = pl::add(&n(12), &pl::mul(&n(12), &index));
            index = pl::add(&index, &n(1));
            // first 2 bytes always 0 (may be part of block type)
            let typ = get16u(it, &buff_pt, pl::add(&entry, &n(2)))?; // block type
            let noff = get32u(it, &buff_pt, pl::add(&entry, &n(4)))?; // negative offset
            let size = get32u(it, &buff_pt, pl::add(&entry, &n(8)))?; // block size
            if pl::gt(&noff, &dir_pos) || pl::gt(&size, &noff) || pl::lt(&size, &n(8)) {
                break 'sam_block;
            }
            if pl::lt(&first_block, &noff) {
                first_block = noff.clone();
            }
            if outfile.truthy() {
                if !(pl::num_eq(&typ, &n(0x0100)) && !audio_n_off.truthy()) {
                    continue;
                }
                // save offset and length of first audio file for QDIO block
                if !(raf_seek(it, &raf, pl::sub(&dir_pos, &noff), 0)? && raf_read_cell(it, &raf, &buf2, &n(8))?) {
                    break;
                }
                len = get32u(it, &buf2, n(4))?;
                audio_n_off = pl::sub(&pl::sub(&noff, &n(8)), &len); // negative offset to start of audio data
                audio_size = pl::sub(&pl::sub(&size, &n(8)), &len);
                continue;
            }
            if !(raf_seek(it, &raf, pl::sub(&dir_pos, &noff), 0)? && raf_read_cell(it, &raf, &buf2, &size)?) {
                break;
            }
            // (could validate the first 4 bytes of the block because they
            // are the same as the first 4 bytes of the directory entry)
            len = get32u(it, &buf2, n(4))?;
            if pl::gt(&pl::add(&len, &n(8)), &size) {
                break;
            }
            let tag = pl::sprintf("0x%.4x", &[typ]);
            let tag_name = pl::concat(&tag, &s("-name"));
            // add unknown tags if necessary
            if !tag_table_ptr.get(tag.to_bytes()).truthy() {
                if !(unknown.truthy() || verbose.truthy()) {
                    continue;
                }
                let tag_info = Value::new_hash();
                tag_info.set("Name", pl::concat(&s("SamsungTrailer_"), &tag));
                tag_info.set("Description", pl::concat(&s("Samsung Trailer "), &tag));
                tag_info.set("Unknown", n(1));
                tag_info.set("Binary", n(1));
                pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), tag_info])?;
            }
            if !tag_table_ptr.get(tag_name.to_bytes()).truthy() {
                let tag_info2 = Value::new_hash();
                tag_info2.set("Name", pl::concat(&pl::concat(&s("SamsungTrailer_"), &tag), &s("Name")));
                tag_info2.set("Description", pl::concat(&pl::concat(&s("Samsung Trailer "), &tag), &s(" Name")));
                tag_info2.set("Unknown", n(1));
                pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_table_ptr.clone(), tag_name.clone(), tag_info2])?;
            }
            // set SamsungTagName ExifTool member for use in tag Condition
            et.set("SamsungTagName", pl::substr(&buf2.deref(), 8, Some(len.to_i64())));
            // extract tag name and value
            let dp = pl::sub(&dir_pos, &noff);
            pl::method(
                it,
                &et,
                "HandleTag",
                vec![tag_table_ptr.clone(), tag_name, Value::Undef, s("DataPt"), buf2.clone(), s("DataPos"), dp.clone(), s("Start"), n(8), s("Size"), len.clone()],
            )?;
            let st = pl::add(&n(8), &len);
            pl::method(
                it,
                &et,
                "HandleTag",
                vec![tag_table_ptr.clone(), tag, Value::Undef, s("DataPt"), buf2.clone(), s("DataPos"), dp, s("Start"), st.clone(), s("Size"), pl::sub(&size, &st)],
            )?;
            et.delete("SamsungTagName");
        }
        if outfile.truthy() {
            if !(raf_seek(it, &raf, data_pos.clone(), 0)? && raf_read_ok(it, &raf, &mut buff, &dir_len)?) {
                break;
            }
            // adjust the absolute offset in the QDIO block if necessary
            if offset_pos.truthy() && audio_n_off.truthy() {
                // initialize the audio file start/end position in the QDIO block
                let t = pl::call(it, "Image::ExifTool::Tell", vec![outfile.clone()])?;
                let new_pos = pl::sub(&pl::sub(&pl::add(&t, &dir_pos), &audio_n_off), &data_pos);
                let bp = pl::sref(buff.clone());
                let p1 = pl::sub(&pl::length(&bp.deref()), &offset_pos);
                pl::call(it, "Image::ExifTool::Set32u", vec![new_pos.clone(), bp.clone(), p1])?;
                let p2 = pl::add(&pl::sub(&pl::length(&bp.deref()), &offset_pos), &n(4));
                pl::call(it, "Image::ExifTool::Set32u", vec![pl::add(&new_pos, &audio_size), bp.clone(), p2])?;
                buff = bp.deref();
                // add a fixup so the calling routine can apply further shifts if necessary
                crate::runtime::require(it, "Image::ExifTool::Fixup")?;
                let mut fixup = dir_info.get("Fixup");
                if !fixup.truthy() {
                    fixup = pl::method(it, &s("Image::ExifTool::Fixup"), "new", vec![])?;
                    dir_info.set("Fixup", fixup.clone());
                }
                let f1 = pl::sub(&pl::length(&buff), &offset_pos);
                pl::method(it, &fixup, "AddFixup", vec![f1])?;
                let f2 = pl::add(&pl::sub(&pl::length(&buff), &offset_pos), &n(4));
                pl::method(it, &fixup, "AddFixup", vec![f2])?;
            }
            if verbose.truthy() {
                let msg = pl::concat(&pl::concat(&s("  Writing Samsung trailer ("), &dir_len), &s(" bytes)\n"));
                pl::method(it, &et, "VPrint", vec![n(0), msg])?;
            }
            if !pl::call(it, "Image::ExifTool::Write", vec![dir_info.get("OutFile"), buff.clone()])?.truthy() {
                return ret(n(-1));
            }
            return ret(n(1));
        }
        return ret(n(1));
    }
    pl::method(it, &et, "Warn", vec![s("Error processing Samsung trailer"), n(1)])?;
    ret(n(0))
}

//------------------------------------------------------------------------------
/// Write Samsung STMN maker notes
/// Inputs: 0) ExifTool object ref, 1) source dirInfo ref, 2) tag table ref
/// Returns: Binary data block or undefined on error
pub fn write_stmn(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    // create a Fixup for the PreviewImage
    let fixup = pl::method(it, &s("Image::ExifTool::Fixup"), "new", vec![])?;
    dir_info.set("Fixup", fixup);
    let val = pl::call(it, "Image::ExifTool::WriteBinaryData", vec![et.clone(), dir_info, tag_table_ptr])?;
    // force PreviewImage into the trailer even if it fits in EXIF segment
    if et.get("PREVIEW_INFO").truthy() {
        et.get("PREVIEW_INFO").set("IsTrailer", n(1));
    }
    ret(val)
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Samsung::Crypt", crypt);
    it.define_native("Image::ExifTool::Samsung::ProcessINFO", process_info);
    it.define_native("Image::ExifTool::Samsung::ProcessSamsungMeta", process_samsung_meta);
    it.define_native("Image::ExifTool::Samsung::ProcessSamsungIFD", process_samsung_ifd);
    it.define_native("Image::ExifTool::Samsung::ProcessSamsung", process_samsung);
    it.define_native("Image::ExifTool::Samsung::WriteSTMN", write_stmn);
}
