//! Image::ExifTool::WritePNG (Image/ExifTool/WritePNG.pl), ported sub by sub (see docs/port/PORTING.md).
//!
//! Write PNG meta information. The file is autoloaded in package
//! Image::ExifTool::PNG; its `my @crcTable` lives under
//! `Image::ExifTool::PNG::<my:WritePNG.pl>::crcTable`.

use crate::interp::eval::ER;
use crate::interp::{Binding, Interp, Want};
use crate::modules::xmp::{cat, hh};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, n, re, ret};

const MODULE: &str = "Image::ExifTool::PNG";

/// `Image::ExifTool::NAME(ARGS)` (imported functions).
fn et_call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// `Write($outfile, LIST)`: true on success.
fn write(it: &mut Interp, outfile: &Value, items: &[Value]) -> ER<bool> {
    let mut args = vec![outfile.clone()];
    args.extend_from_slice(items);
    Ok(et_call(it, "Write", args)?.truthy())
}

/// `%stdCase` of PNG.pm (a package variable).
fn std_case(it: &mut Interp, key: &str) -> Value {
    pl::pkg_hash(it, "Image::ExifTool::PNG::stdCase").get(key)
}

/// `eval { require Compress::Zlib }`
fn have_zlib(it: &mut Interp) -> ER<bool> {
    Ok(pl::eval_block(it, |it| crate::runtime::require(it, "Compress::Zlib"))?.is_some())
}

/// The file's `my @crcTable`.
fn crc_table(it: &mut Interp) -> Value {
    match it.global(b'@', &format!("{MODULE}::<my:WritePNG.pl>::crcTable")) {
        Binding::Array(a) => Value::ArrayRef(a),
        _ => Value::new_array(Vec::new()),
    }
}

/// Calculate CRC or update running CRC (ref 1)
/// Inputs: 0) data reference, 1) running crc to update (undef initially)
///         2) data position (undef for 0), 3) data length (undef for all data),
/// Returns: updated CRC
pub fn calculate_crc(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(calculate_crc_v(it, &arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3))?)
}

pub(crate) fn calculate_crc_v(it: &mut Interp, data_pt: &Value, mut crc: Value, mut pos: Value, mut len: Value) -> ER<Value> {
    if crc.is_undef() {
        crc = n(0);
    }
    if pos.is_undef() {
        pos = n(0);
    }
    let data = data_pt.deref();
    if len.is_undef() {
        len = pl::sub(&pl::length(&data), &pos);
    }
    let mut c = ops::to_uv(&crc) ^ 0xffffffff; // undo 1's complement
    // build lookup table unless done already
    let tbl = crc_table(it);
    if tbl.is_empty() {
        for k in 0..256u64 {
            let mut cc = k;
            for _ in 0..8 {
                cc = if cc & 1 != 0 { 0xedb88320 ^ (cc >> 1) } else { cc >> 1 };
            }
            tbl.set_at(k as usize, ops::from_uv(cc));
        }
    }
    let table: Vec<u64> = tbl.items().iter().map(ops::to_uv).collect();
    // calculate the CRC
    let bytes = data.to_bytes();
    // foreach (unpack("x${pos}C$len", $$dataPt)): the bytes directly when the
    // template is plain, else perl's unpack (and its errors)
    let simple = |v: &Value| matches!(v, Value::Int(i) if *i >= 0);
    let list: Vec<u64> = if simple(&pos) && simple(&len) && (pos.to_i64() as usize) <= bytes.len() {
        let p = pos.to_i64() as usize;
        let e = (p + len.to_i64() as usize).min(bytes.len());
        bytes[p..e].iter().map(|&b| b as u64).collect()
    } else {
        // (perl's unpack dies on an 'x' past the end before it looks at the
        // count; the shared unpack does not, so the check is made here)
        if pl::gt(&pos, &n(bytes.len() as i64)) {
            return crate::interp::eval::die("'x' outside of string in unpack");
        }
        let t = cat!("x", pos, "C", len);
        pl::unpack(&t.to_text(), &data)?.iter().map(ops::to_uv).collect()
    };
    for b in list {
        c = table.get(((c ^ b) & 0xff) as usize).copied().unwrap_or(0) ^ (c >> 8);
    }
    Ok(ops::from_uv(c ^ 0xffffffff)) // return 1's complement
}

/// Encode data in ASCII Hex
/// Inputs: 0) input data reference
/// Returns: Hex-encoded data (max 72 chars per line)
pub fn hex_encode(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(hex_encode_v(&arg(&a, 0))?)
}

fn hex_encode_v(data_pt: &Value) -> ER<Value> {
    let data = data_pt.deref();
    let len = pl::length(&data).to_i64();
    let mut hex = Vec::new();
    let mut pos = 0;
    while pos < len {
        let mut nn = len - pos;
        if nn > 36 {
            nn = 36;
        }
        hex.extend_from_slice(&pl::unpack1("H*", &pl::substr(&data, pos, Some(nn)))?.to_bytes());
        hex.push(b'\n');
        pos += 36;
    }
    Ok(Value::bytes(hex))
}

/// Write profile chunk (possibly compressed if Zlib is available)
/// Inputs: 0) outfile, 1) Raw profile type (SCALAR ref for iCCP name), 2) data ref
///         3) profile header type (undef if not a text profile)
/// Returns: 1 on success
pub fn write_profile(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(write_profile_v(it, &arg(&a, 0), &arg(&a, 1), arg(&a, 2), &arg(&a, 3))?)
}

fn write_profile_v(it: &mut Interp, outfile: &Value, raw_type: &Value, mut data_pt: Value, profile: &Value) -> ER<Value> {
    let (mut buff, prefix, chunk);
    let mut deflate = Value::Undef;
    let exif = std_case(it, "exif");
    if (raw_type.is_ref() || raw_type.to_bytes() != exif.to_bytes()) && have_zlib(it)? {
        deflate = pl::call(it, "Compress::Zlib::deflateInit", vec![])?;
    }
    if profile.is_undef() {
        // write ICC profile as compressed iCCP chunk if possible
        if raw_type.is_ref() {
            if !deflate.truthy() {
                return Ok(n(0));
            }
            chunk = pl::s("iCCP");
            prefix = cat!(raw_type.deref(), b"\0\0"[..]);
        } else {
            chunk = raw_type.clone();
            if raw_type.to_bytes() == std_case(it, "zxif").to_bytes() {
                prefix = cat!(b"\0"[..], pl::pack("N", &[pl::length(&data_pt.deref())])?); // (proposed compressed EXIF)
            } else {
                prefix = pl::s(""); // standard EXIF
            }
        }
        if deflate.truthy() {
            buff = pl::method(it, &deflate, "deflate", vec![data_pt.deref()])?;
            if buff.is_undef() {
                return Ok(n(0));
            }
            let f = pl::method(it, &deflate, "flush", vec![])?;
            buff = pl::concat(&buff, &f);
            data_pt = pl::sref(buff);
        }
    } else {
        // write as ASCII-hex encoded profile in tEXt or zTXt chunk
        let fmt = cat!("\n", profile, " profile\n%8d\n");
        let txt_hdr = pl::sprintf_v(&fmt, &[pl::length(&data_pt.deref())]);
        buff = pl::concat(&txt_hdr, &hex_encode_v(&data_pt)?);
        let mut ch = pl::s("tEXt"); // write as tEXt if deflate not available
        let mut pre = cat!("Raw profile type ", raw_type, b"\0"[..]);
        data_pt = pl::sref(buff.clone());
        // write profile as zTXt chunk if possible
        if deflate.truthy() {
            let buf2 = pl::method(it, &deflate, "deflate", vec![buff.clone()])?;
            if !buf2.is_undef() {
                let f = pl::method(it, &deflate, "flush", vec![])?;
                data_pt = pl::sref(pl::concat(&buf2, &f));
                ch = pl::s("zTXt");
                pre = pl::concat(&pre, &pl::s("\0")); // compression type byte (0=deflate)
            }
        }
        chunk = ch;
        prefix = pre;
    }
    let data = data_pt.deref();
    let hdr = pl::concat(&pl::pack("Na4", &[pl::add(&pl::length(&prefix), &pl::length(&data)), chunk])?, &prefix);
    let crc = calculate_crc_v(it, &pl::sref(hdr.clone()), Value::Undef, n(4), Value::Undef)?;
    let crc = calculate_crc_v(it, &data_pt, crc, Value::Undef, Value::Undef)?;
    let w = write(it, outfile, &[hdr, data, pl::pack("N", &[crc])?])?;
    Ok(Value::from(w))
}

/// Add iCCP-related chunks to the PNG image if necessary (must come before PLTE and IDAT)
/// Inputs: 0) ExifTool object ref, 1) output file or scalar ref
/// Returns: true on success
pub fn add_iccp(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, outfile) = (arg(&a, 0), arg(&a, 1));
    if hh(&et, "ADD_DIRS").get("ICC_Profile").truthy() {
        // write new ICC data
        let tag_table_ptr = et_call(it, "GetTagTable", vec![pl::s("Image::ExifTool::ICC_Profile::Main")])?;
        let dir_info = Value::new_hash();
        dir_info.set("Parent", pl::s("PNG"));
        dir_info.set("DirName", pl::s("ICC_Profile"));
        let buff = pl::method(it, &et, "WriteDirectory", vec![dir_info, tag_table_ptr])?;
        if !buff.is_undef() && pl::length(&buff).truthy() {
            let ti = pl::pkg_hash(it, "Image::ExifTool::PNG::Main").get("iCCP-name");
            let mut profile_name = pl::method(it, &et, "GetNewValue", vec![ti])?;
            if profile_name.is_undef() {
                profile_name = pl::s("icm");
            }
            if write_profile_v(it, &outfile, &pl::sref(profile_name.clone()), pl::sref(buff), &Value::Undef)?.truthy() {
                pl::method(it, &et, "VPrint", vec![n(0), cat!("Created ICC profile with name '", profile_name, "'\n")])?;
                hh(&et, "ADD_DIRS").delete("ICC_Profile"); // don't add it again
            }
        }
    }
    // must also add sRGB and gAMA before PLTE and IDAT
    if et.get("ADD_PNG").truthy() {
        let add_before_plte = Value::new_hash();
        for tag in ["sRGB", "gAMA"] {
            let add_png = hh(&et, "ADD_PNG");
            if !add_png.get(tag).truthy() {
                continue;
            }
            add_before_plte.set(tag, add_png.get(tag));
            add_png.delete(tag);
        }
        if !add_before_plte.is_empty() {
            let save = et.get("ADD_PNG");
            et.set("ADD_PNG", add_before_plte);
            add_chunks_v(it, &et, &outfile, Vec::new())?;
            et.set("ADD_PNG", save);
        }
    }
    ret(n(1))
}

/// This routine is called after we edit an existing directory
/// Inputs: 0) ExifTool ref, 1) dir name, 2) output data ref
///         3) flag set if location is non-standard (to update, but not create from scratch)
/// - on return, $$outBuff is set to '' if the directory is to be deleted
pub fn done_dir(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, mut dir, out_buff, non_standard) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    let save_dir = dir.clone();
    if pl::str_eq(&dir, "IFD0") {
        dir = pl::s("EXIF");
    }
    // don't add this directory again unless this is in a non-standard location
    if !non_standard.truthy() {
        hh(&et, "ADD_DIRS").delete(dir.to_bytes());
        if pl::str_eq(&dir, "EXIF") {
            hh(&et, "ADD_DIRS").delete("IFD0");
        }
    } else if hh(&et, "DEL_GROUP").get(dir.to_bytes()).truthy() || hh(&et, "DEL_GROUP").get(save_dir.to_bytes()).truthy() {
        pl::method(it, &et, "VPrint", vec![n(0), cat!("  Deleting non-standard ", dir, "\n")])?;
        pl::set_deref(&out_buff, pl::s(""))?;
    }
    pl::ret_undef()
}

/// Generate tEXt, zTXt or iTXt data for writing
/// Inputs: 0) ExifTool ref, 1) tagID, 2) tagInfo ref, 3) value string, 4) language code
/// Returns: chunk data (not including 8-byte chunk header)
/// Notes: Sets ExifTool TextChunkType member to the type of chunk written
pub fn build_text_chunk(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(build_text_chunk_v(it, &arg(&a, 0), arg(&a, 1), &arg(&a, 2), arg(&a, 3), &arg(&a, 4))?)
}

fn build_text_chunk_v(it: &mut Interp, et: &Value, mut tag: Value, tag_info: &Value, mut val: Value, lang: &Value) -> ER<Value> {
    let (mut comp_val, mut i_txt, mut comp) = (Value::Undef, Value::Undef, Value::Undef);
    if tag_info.get("SubDirectory").truthy() {
        if pl::str_eq(&tag_info.get("Name"), "XMP") {
            i_txt = n(2); // write as iTXt but flag to avoid encoding
            // (never compress XMP)
        } else {
            comp = n(2); // compress raw profile if possible
        }
    } else {
        // compress if specified
        if pl::method(it, et, "Options", vec![pl::s("Compress")])?.truthy() {
            comp = n(1);
        }
        if lang.truthy() {
            i_txt = n(1); // write as iTXt if it has a language code
            let mut p = b"-".to_vec();
            p.extend_from_slice(&lang.to_bytes());
            p.push(b'$');
            pl::re_dyn(&p, "")?.subst(&mut tag, false, |_| Vec::new()); // remove language code from tagID
        } else if !pl::str_eq(&hh(et, "OPTIONS").get("Charset"), "Latin") && re(r"[\x80-\xff]", "").is_match(&val) {
            i_txt = n(1); // write as iTXt if it contains non-Latin special characters
        } else if tag_info.get("iTXt").truthy() {
            i_txt = n(1); // write as iTXt if specified in user-defined tag
        }
    }
    if comp.truthy() {
        let mut warn = Value::Undef;
        if have_zlib(it)? {
            let deflate = pl::call(it, "Compress::Zlib::deflateInit", vec![])?;
            if deflate.truthy() {
                comp_val = pl::method(it, &deflate, "deflate", vec![val.clone()])?;
            }
            if !comp_val.is_undef() {
                let f = pl::method(it, &deflate, "flush", vec![])?;
                comp_val = pl::concat(&comp_val, &f);
                // only compress if it actually saves space
                if !pl::lt(&pl::length(&comp_val), &pl::length(&val)) {
                    comp_val = Value::Undef;
                    warn = pl::s("uncompressed data is smaller");
                }
            } else {
                warn = pl::s("deflate error");
            }
        } else {
            warn = pl::s("Compress::Zlib not available");
        }
        // warn if any user-specified compression fails
        if warn.truthy() && pl::num_eq(&comp, &n(1)) {
            pl::method(it, et, "Warn", vec![cat!("PNG:", tag_info.get("Name"), " not compressed (", warn, ")"), n(1)])?;
        }
    }
    // decide whether to write as iTXt, zTXt or tEXt
    let xtra;
    if i_txt.truthy() {
        et.set("TextChunkType", pl::s("iTXt"));
        let l = if lang.truthy() { lang.clone() } else { pl::s("") };
        xtra = cat!(if comp_val.is_undef() { &b"\0\0"[..] } else { &b"\x01\0"[..] }, l, b"\0\0"[..]);
        // iTXt is encoded as UTF-8 (but note that XMP is already UTF-8)
        if pl::num_eq(&i_txt, &n(1)) {
            val = pl::method(it, et, "Encode", vec![val, pl::s("UTF8")])?;
        }
    } else if !comp_val.is_undef() {
        et.set("TextChunkType", pl::s("zTXt"));
        xtra = pl::s("\0");
    } else {
        et.set("TextChunkType", pl::s("tEXt"));
        xtra = pl::s("");
    }
    Ok(cat!(tag, b"\0"[..], xtra, if comp_val.is_undef() { val } else { comp_val }))
}

/// Add any outstanding new chunks to the PNG image
/// Inputs: 0) ExifTool object ref, 1) output file or scalar ref
///         2-N) dirs to add (empty to add all except EXIF 'IFD0', including PNG tags)
/// Returns: true on success
pub fn add_chunks(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let add = a.iter().skip(2).cloned().collect();
    ret(add_chunks_v(it, &arg(&a, 0), &arg(&a, 1), add)?)
}

fn sorted_keys(h: &Value) -> Vec<Value> {
    let mut k: Vec<Vec<u8>> = h.keys().iter().map(|k| k.to_vec()).collect();
    k.sort();
    k.into_iter().map(Value::bytes).collect()
}

fn crc_n(it: &mut Interp, buff: &Value) -> ER<Value> {
    let c = calculate_crc_v(it, &pl::sref(buff.clone()), Value::Undef, Value::Undef, Value::Undef)?;
    pl::pack("N", &[c])
}

fn add_chunks_v(it: &mut Interp, et: &Value, outfile: &Value, mut add: Vec<Value>) -> ER<Value> {
    let add_tags;
    let mut err = false;
    let mut specified = false;
    if !add.is_empty() {
        add_tags = Value::new_hash(); // don't add any PNG tags
        specified = true;
    } else {
        let mut t = et.get("ADD_PNG"); // add all PNG tags...
        et.delete("ADD_PNG"); // ...once
        if t.is_undef() {
            t = Value::new_hash(); // (keys %$addTags autovivifies)
        }
        add_tags = t;
        // add all directories
        add = sorted_keys(&hh(et, "ADD_DIRS"));
    }
    // write any outstanding PNG tags
    let textual = pl::pkg_hash(it, "Image::ExifTool::PNG::TextualData");
    for tag in sorted_keys(&add_tags) {
        let tag_info = add_tags.get(tag.to_bytes());
        if tag_info.get("FakeTag").truthy() {
            continue; // (iCCP-name)
        }
        let nv_hash = pl::method(it, et, "GetNewValueHash", vec![tag_info.clone()])?;
        // (native PNG information is always preferred, so don't rely on just IsCreating)
        if !(nv_hash.get("IsCreating").truthy() || pl::gt(&pl::method(it, et, "IsOverwriting", vec![nv_hash.clone()])?, &n(0))) {
            continue;
        }
        let val = pl::method(it, et, "GetNewValue", vec![nv_hash.clone()])?;
        if !val.is_undef() {
            if nv_hash.get("EditOnly").truthy() {
                continue;
            }
            let data;
            if tag_info.get("Table").same_ref(&textual) {
                let d = build_text_chunk_v(it, et, tag.clone(), &tag_info, val.clone(), &tag_info.get("LangCode"))?;
                data = pl::concat(&et.get("TextChunkType"), &d);
                et.delete("TextChunkType");
            } else {
                data = pl::concat(&tag, &val);
            }
            let hdr = pl::pack("N", &[pl::sub(&pl::length(&data), &n(4))])?;
            let cbuf = crc_n(it, &data)?;
            if !write(it, outfile, &[hdr, data, cbuf])? {
                err = true;
            }
            pl::method(it, et, "VerboseValue", vec![cat!("+ PNG:", tag_info.get("Name")), val])?;
            et.set("CHANGED", pl::add(&et.get("CHANGED"), &n(1)));
        }
    }
    // create any necessary directories
    for dir in add {
        if !hh(et, "ADD_DIRS").get(dir.to_bytes()).truthy() {
            continue; // make sure we want to add it first
        }
        let buff;
        let dir_info = Value::new_hash();
        dir_info.set("Parent", pl::s("PNG"));
        dir_info.set("DirName", dir.clone());
        if pl::str_eq(&dir, "IFD0") {
            if !specified {
                continue; // wait until specifically asked to write EXIF 'IFD0'
            }
            let chunk = std_case(it, "exif");
            // (zxIf was not adopted)
            pl::method(it, et, "VPrint", vec![n(0), cat!("Creating ", chunk, " chunk:\n")])?;
            et.set("TIFF_TYPE", pl::s("APP1"));
            let tag_table_ptr = et_call(it, "GetTagTable", vec![pl::s("Image::ExifTool::Exif::Main")])?;
            let wt = Value::Code(it.sub_ref("Image::ExifTool::WriteTIFF"));
            buff = pl::method(it, et, "WriteDirectory", vec![dir_info, tag_table_ptr, wt])?;
            if !buff.is_undef() && pl::length(&buff).truthy() && !write_profile_v(it, outfile, &chunk, pl::sref(buff), &Value::Undef)?.truthy() {
                err = true;
            }
        } else if pl::str_eq(&dir, "XMP") {
            pl::method(it, et, "VPrint", vec![n(0), pl::s("Creating XMP iTXt chunk:\n")])?;
            let tag_table_ptr = et_call(it, "GetTagTable", vec![pl::s("Image::ExifTool::XMP::Main")])?;
            dir_info.set("ReadOnly", n(1));
            let mut b = pl::method(it, et, "WriteDirectory", vec![dir_info, tag_table_ptr])?;
            if !b.is_undef() && pl::length(&b).truthy() && {
                // the packet is read-only (because of CRC)
                let r = pl::sref(b.clone());
                let ok = pl::call(it, "Image::ExifTool::XMP::ValidateXMP", vec![r.clone(), pl::s("r")])?.truthy();
                b = r.deref();
                ok
            } {
                // (now write XMP iTXt chunk according to XMP specification)
                b = pl::concat(&pl::s("iTXtXML:com.adobe.xmp\0\0\0\0\0"), &b);
                let hdr = pl::pack("N", &[pl::sub(&pl::length(&b), &n(4))])?;
                let cbuf = crc_n(it, &b)?;
                if !write(it, outfile, &[hdr, b, cbuf])? {
                    err = true;
                }
            }
        } else if pl::str_eq(&dir, "IPTC") {
            pl::method(it, et, "Warn", vec![pl::s("Creating non-standard IPTC in PNG"), n(1)])?;
            pl::method(it, et, "VPrint", vec![n(0), pl::s("Creating IPTC profile:\n")])?;
            // write new IPTC data (stored in a Photoshop directory)
            dir_info.set("DirName", pl::s("Photoshop"));
            let tag_table_ptr = et_call(it, "GetTagTable", vec![pl::s("Image::ExifTool::Photoshop::Main")])?;
            buff = pl::method(it, et, "WriteDirectory", vec![dir_info, tag_table_ptr])?;
            if !buff.is_undef() && pl::length(&buff).truthy() && !write_profile_v(it, outfile, &pl::s("iptc"), pl::sref(buff), &pl::s("IPTC"))?.truthy() {
                err = true;
            }
        } else if pl::str_eq(&dir, "ICC_Profile") {
            pl::method(it, et, "VPrint", vec![n(0), pl::s("Creating ICC profile:\n")])?;
            // write new ICC data (only done if we couldn't create iCCP chunk)
            let tag_table_ptr = et_call(it, "GetTagTable", vec![pl::s("Image::ExifTool::ICC_Profile::Main")])?;
            buff = pl::method(it, et, "WriteDirectory", vec![dir_info, tag_table_ptr])?;
            if !buff.is_undef() && pl::length(&buff).truthy() {
                if !write_profile_v(it, outfile, &pl::s("icm"), pl::sref(buff), &pl::s("ICC"))?.truthy() {
                    err = true;
                }
                pl::method(it, et, "Warn", vec![pl::s("Wrote ICC as a raw profile (no Compress::Zlib)")])?;
            }
        } else if pl::str_eq(&dir, "PNG-pHYs") {
            pl::method(it, et, "VPrint", vec![n(0), pl::s("Creating pHYs chunk (default 2834 pixels per meter):\n")])?;
            let tag_table_ptr = et_call(it, "GetTagTable", vec![pl::s("Image::ExifTool::PNG::PhysicalPixel")])?;
            let blank = Value::bytes(b"\0\0\x0b\x12\0\0\x0b\x12\x01"); // 2834 pixels per meter (72 dpi)
            dir_info.set("DataPt", pl::sref(blank));
            let mut b = pl::method(it, et, "WriteDirectory", vec![dir_info, tag_table_ptr])?;
            if !b.is_undef() && pl::length(&b).truthy() {
                b = pl::concat(&pl::s("pHYs"), &b); // CRC includes chunk name
                let hdr = pl::pack("N", &[pl::sub(&pl::length(&b), &n(4))])?;
                let cbuf = crc_n(it, &b)?;
                if !write(it, outfile, &[hdr, b, cbuf])? {
                    err = true;
                }
            }
        } else {
            continue;
        }
        hh(et, "ADD_DIRS").delete(dir.to_bytes()); // don't add again
    }
    Ok(Value::from(!err))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::PNG::CalculateCRC", calculate_crc);
    it.define_native("Image::ExifTool::PNG::HexEncode", hex_encode);
    it.define_native("Image::ExifTool::PNG::WriteProfile", write_profile);
    it.define_native("Image::ExifTool::PNG::Add_iCCP", add_iccp);
    it.define_native("Image::ExifTool::PNG::DoneDir", done_dir);
    it.define_native("Image::ExifTool::PNG::BuildTextChunk", build_text_chunk);
    it.define_native("Image::ExifTool::PNG::AddChunks", add_chunks);
}
