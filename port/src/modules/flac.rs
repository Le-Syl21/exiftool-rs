//! Image::ExifTool::FLAC (Image/ExifTool/FLAC.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read Free Lossless Audio Codec information

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// `$$ref` under `use strict`: undef is fatal.
fn deref(r: &Value) -> ER<Value> {
    if r.is_undef() {
        return crate::interp::eval::die("Can't use an undefined value as a SCALAR reference");
    }
    Ok(r.deref())
}

/// Process information in a bit stream
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Notes: Byte order is used to determine the ordering of bits in the stream:
/// 'MM' = bit 0 is most significant, 'II' = bit 0 is least significant
/// - can handle arbitrarily wide values (eg. 8-byte or larger integers)
pub fn process_bit_stream(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let data_pos = dir_info.get("DataPos");
    let ds = dir_info.get("DirStart");
    let dir_start = if ds.truthy() { ds } else { n(0) };
    let dl = dir_info.get("DirLen");
    let dir_len = if dl.truthy() { dl } else { pl::sub(&pl::length(&deref(&data_pt)?), &dir_start) };
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    let byte_order = call(it, "GetByteOrder", vec![])?;

    if verbose.truthy() {
        let msg = format!("  + [BitStream directory, {} bytes, '{}' order]\n", dir_len.to_text(), byte_order.to_text());
        pl::method(it, &et, "VPrint", vec![n(0), Value::from(msg)])?;
    }
    let mut keys: Vec<Value> = tag_table_ptr.keys().into_iter().map(Value::from).collect();
    keys.sort_by(|x, y| x.to_bytes().cmp(&y.to_bytes()));
    for tag in keys {
        let Some(m) = re(r"^Bit(\d+)-?(\d+)?", "").m(&tag) else { continue };
        let b1 = cap(&m, 1);
        let b2 = { let c = cap(&m, 2); if c.truthy() { c } else { b1.clone() } }; // start/end bit numbers in stream
        let (i1, i2) = (pl::int(&pl::div(&b1, &n(8))?), pl::int(&pl::div(&b2, &n(8))?)); // start/end byte numbers
        let (f1, f2) = (pl::modulo(&b1, &n(8))?.to_i64(), pl::modulo(&b2, &n(8))?.to_i64()); // start/end bit numbers within each byte
        if pl::ge(&i2, &dir_len) {
            break;
        }
        let (mut val, mut extra) = (Value::Undef, Value::Undef);
        // if Format is unspecified, convert the specified number of bits to an unsigned integer,
        // otherwise allow HandleTag to convert whole bytes the normal way (via undefined $val)
        let ti = tag_table_ptr.get(tag.to_bytes());
        if pl::ref_of(&ti) != "HASH" || !ti.get("Format").truthy() {
            let mut mask: i64 = 0;
            val = n(0);
            if verbose.truthy() && (f1 != 0 || f2 != 7) {
                extra = pl::s(", Mask=0x");
            }
            let (i1n, i2n) = (i1.to_i64(), i2.to_i64());
            if byte_order.eq_str("MM") {
                // loop from high byte to low byte
                let mut i = i1n;
                while i <= i2n {
                    mask = 0xff;
                    if i == i1n && f1 != 0 {
                        // mask off high bits in first word (0 is high bit)
                        for b in (8 - f1)..=7 {
                            mask ^= 1 << b;
                        }
                    }
                    if i == i2n && f2 < 7 {
                        // mask off low bits in last word (7 is low bit)
                        for b in 0..=(6 - f2) {
                            mask ^= 1 << b;
                        }
                    }
                    let byte = call(it, "Get8u", vec![data_pt.clone(), pl::add(&n(i), &dir_start)])?;
                    val = pl::add(&pl::mul(&val, &n(256)), &n(mask & byte.to_i64()));
                    if extra.truthy() {
                        extra = pl::concat(&extra, &pl::sprintf("%.2x", &[n(mask)]));
                    }
                    i += 1;
                }
            } else {
                // (FLAC is big-endian, but support little-endian bit streams
                //  so this routine can be used by other modules)
                // loop from high byte to low byte
                let mut i = i2n;
                while i >= i1n {
                    mask = 0xff;
                    if i == i1n && f1 != 0 {
                        // mask off low bits in first word (0 is low bit)
                        for b in 0..=(f1 - 1) {
                            mask ^= 1 << b;
                        }
                    }
                    if i == i2n && f2 < 7 {
                        // mask off high bits in last word (7 is high bit)
                        for b in (f2 + 1)..=7 {
                            mask ^= 1 << b;
                        }
                    }
                    let byte = call(it, "Get8u", vec![data_pt.clone(), pl::add(&n(i), &dir_start)])?;
                    val = pl::add(&pl::mul(&val, &n(256)), &n(mask & byte.to_i64()));
                    if extra.truthy() {
                        extra = pl::concat(&extra, &pl::sprintf("%.2x", &[n(mask)]));
                    }
                    i -= 1;
                }
            }
            // shift word down until low bit is in position 0
            while mask & 0x01 == 0 {
                val = pl::div(&val, &n(2))?;
                mask >>= 1;
            }
        }
        pl::method(
            it,
            &et,
            "HandleTag",
            vec![
                tag_table_ptr.clone(),
                tag.clone(),
                val,
                pl::s("DataPt"),
                data_pt.clone(),
                pl::s("DataPos"),
                data_pos.clone(),
                pl::s("Start"),
                pl::add(&dir_start, &i1),
                pl::s("Size"),
                pl::add(&pl::sub(&i2, &i1), &n(1)),
                pl::s("Extra"),
                extra,
            ],
        )?;
    }
    ret(n(1))
}

/// Extract information from an Ogg FLAC file
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid Ogg FLAC file
pub fn process_flac(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));

    // must first check for leading/trailing ID3 information
    if !et.get("DoneID3").truthy() {
        crate::runtime::require(it, "Image::ExifTool::ID3")?;
        if pl::call(it, "Image::ExifTool::ID3::ProcessID3", vec![et.clone(), dir_info.clone()])?.truthy() {
            return ret(n(1));
        }
    }
    let raf = dir_info.get("RAF");
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    let out = pl::method(it, &et, "Options", vec![pl::s("TextOut")])?;
    let buff = cell(Value::Undef);
    let mut err = false;

    // check FLAC signature
    if !(pl::num_eq(&raf_read_cell(it, &raf, &buff, n(4))?, &n(4)) && buff.borrow().eq_str("fLaC")) {
        return ret(n(0));
    }
    pl::method(it, &et, "SetFileType", vec![])?;
    call(it, "SetByteOrder", vec![pl::s("MM")])?;
    let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::FLAC::Main")])?;
    loop {
        // read next metadata block header
        if !pl::num_eq(&raf_read_cell(it, &raf, &buff, n(4))?, &n(4)) {
            break;
        }
        let b = buff.borrow().clone();
        let flag = pl::unpack1("C", &b)?.to_i64();
        let size = n(pl::unpack1("N", &b)?.to_i64() & 0x00ffffff);
        if !pl::num_eq(&raf_read_cell(it, &raf, &buff, size.clone())?, &size) {
            err = true;
            break;
        }
        let last = flag & 0x80; // last-metadata-block flag
        let tag = flag & 0x7f; // tag bits
        if verbose.truthy() {
            pl::print_to(it, Some(&out), &[Value::from(format!("FLAC metadata block, type {tag}:\n"))])?;
            let tell = pl::method(it, &raf, "Tell", vec![])?;
            pl::method(it, &et, "VerboseDump", vec![Value::ScalarRef(buff.clone()), pl::s("DataPos"), pl::sub(&tell, &size)])?;
        }
        let tell = pl::method(it, &raf, "Tell", vec![])?;
        let b = buff.borrow().clone();
        pl::method(
            it,
            &et,
            "HandleTag",
            vec![tag_table_ptr.clone(), n(tag), b, pl::s("DataPt"), Value::ScalarRef(buff.clone()), pl::s("DataPos"), pl::sub(&tell, &size), pl::s("Start"), n(0), pl::s("Size"), size.clone()],
        )?;
        if last != 0 {
            break; // all done if  is set
        }
    }
    if err {
        pl::method(it, &et, "Warn", vec![pl::s("Format error in FLAC file")])?;
    }
    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::FLAC::ProcessBitStream", process_bit_stream);
    it.define_native("Image::ExifTool::FLAC::ProcessFLAC", process_flac);
}
