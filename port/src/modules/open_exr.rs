//! Image::ExifTool::OpenEXR (Image/ExifTool/OpenEXR.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read OpenEXR meta information. The file's `my %formatType` stays Perl
//! data, read with `lex_hash`.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read, raf_read_cell};
use crate::perl::ops;
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};
use crate::runtime;

const MODULE: &str = "Image::ExifTool::OpenEXR";

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

fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

//------------------------------------------------------------------------------
// Extract information from an OpenEXR file
// Inputs: 0) ExifTool object reference, 1) DirInfo reference
// Returns: 1 on success, 0 if this wasn't a valid OpenEXR file
pub fn process_exr(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let verbose = pl::method(it, &et, "Options", vec![s("Verbose")])?;
    let b = pl::method(it, &et, "Options", vec![s("Binary")])?;
    let binary = if b.truthy() { b } else { verbose.clone() };
    let mut buff = Value::Undef;
    let mut dim = Value::Undef;
    let format_type = runtime::lex_hash(it, MODULE, "formatType");

    // verify this is a valid RIFF file
    if !pl::num_eq(&raf_read(it, &raf, &mut buff, n(8))?, &n(8)) {
        return ret(n(0));
    }
    if !re(r"^\x76\x2f\x31\x01", "s").is_match(&buff) {
        return ret(n(0));
    }
    pl::method(it, &et, "SetFileType", vec![])?;
    call(it, "SetByteOrder", vec![s("II")])?;
    let tag_table_ptr = call(it, "GetTagTable", vec![s("Image::ExifTool::OpenEXR::Main")])?;

    // extract information from header
    let flags = pl::unpack1("x4V", &buff)?;
    pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), s("_ver"), ops::bit_and(&flags, &n(0xff))])?;
    pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), s("_flags"), ops::bit_and(&flags, &n(0xffffff00))])?;
    let max_len: i64 = if ops::bit_and(&flags, &n(0x400)).truthy() { 255 } else { 31 };
    let multi = ops::bit_and(&flags, &n(0x1000));
    let attr_re = pl::re_dyn(format!(r"^([^\0]{{1,{max_len}}})\0([^\0]{{1,{max_len}}})\0(.{{4}})").as_bytes(), "s")?;

    // extract attributes
    loop {
        if !raf_read(it, &raf, &mut buff, n((max_len + 1) * 2 + 5))?.truthy() {
            break;
        }
        if re(r"^\0", "").is_match(&buff) {
            if !(multi.truthy() && pl::method(it, &et, "Options", vec![s("ExtractEmbedded")])?.truthy()) {
                break;
            }
            // remove null and process the next frame header as a sub-document
            // (second null is end of all headers)
            let mut nulls = 0;
            let removed = re(r"^(\0+)", "").subst(&mut buff, false, |c| {
                nulls = cap(c, 1).to_bytes().len();
                Vec::new()
            });
            if removed > 0 && nulls > 1 {
                break;
            }
            let dc = pl::add(&et.get("DOC_COUNT"), &n(1));
            et.set("DOC_COUNT", dc.clone());
            et.set("DOC_NUM", dc);
        }
        let Some(m) = attr_re.m(&buff) else {
            pl::method(it, &et, "Warn", vec![s("EXR format error")])?;
            break;
        };
        // (pos($buff): the end of the match)
        let pos = cap(&m, 0).to_bytes().len() as i64;
        let (tag, type_, size) = (cap(&m, 1), cap(&m, 2), pl::unpack1("V", &cap(&m, 3))?);
        let back = pos - pl::length(&buff).to_i64();
        if !pl::method(it, &raf, "Seek", vec![n(back), n(1)])?.truthy() {
            pl::method(it, &et, "Warn", vec![s("Seek error")])?;
            break;
        }
        let mut tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
        if !tag_info.truthy() {
            let mut name = pl::ucfirst(&tag);
            re(r"([^a-zA-Z])([a-z])", "").subst(&mut name, true, |c| [&cap(c, 1).to_bytes()[..], &pl::uc(&cap(c, 2)).to_bytes()[..]].concat()); // capitalize first letter of each word
            pl::tr(&mut name, "-_a-zA-Z0-9", "", "dc");
            if pl::length(&name).to_i64() <= 1 {
                if pl::length(&name).truthy() {
                    name = pl::concat(&s("Tag"), &name);
                } else {
                    name = s("Invalid");
                }
            }
            tag_info = Value::new_hash();
            tag_info.set("Name", name);
            call(it, "AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), tag_info.clone()])?;
            pl::method(it, &et, "VPrint", vec![n(0), et.get("INDENT"), cat(&[s("[adding "), tag.clone(), s("]\n")])])?;
        }
        let mut val = Value::Undef;
        let mut success = false;
        let buf2 = cell(Value::Undef); // (my $buf2: \$buf2 is taken below)
        let buf2_pt = Value::ScalarRef(buf2.clone());
        let format = format_type.get(type_.to_bytes());
        let subdir = tag_info.get("SubDirectory");
        if format.truthy() || binary.truthy() || subdir.truthy() {
            if pl::num_eq(&raf_read_cell(it, &raf, &buf2, size.clone())?, &size) {
                success = true;
            }
            let b2 = buf2.borrow().clone();
            if subdir.truthy() {
                let pos = pl::sub(&pl::method(it, &raf, "Tell", vec![])?, &pl::length(&b2));
                pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tag.clone(), Value::Undef, s("DataPt"), buf2_pt.clone(), s("DataPos"), pos])?;
                if success {
                    continue;
                }
            } else if !format.truthy() {
                val = buf2_pt.clone(); // treat as undef binary data
            } else if !pl::str_eq(&format, "1") {
                // handle formats which map nicely into ExifTool format codes
                if let Some(m) = re(r"^(\w+)\[?(\d*)", "").m(&format) {
                    let (fmt, mut cnt) = (cap(&m, 1), cap(&m, 2));
                    if !cnt.truthy() {
                        cnt = if pl::str_eq(&fmt, "string") { size.clone() } else { n(1) };
                    }
                    val = call(it, "ReadValue", vec![buf2_pt.clone(), n(0), fmt, cnt, size.clone()])?;
                }
            // handle other format types
            } else if pl::str_eq(&type_, "tiledesc") {
                if pl::ge(&size, &n(9)) {
                    let x = call(it, "Get32u", vec![buf2_pt.clone(), n(0)])?;
                    let y = call(it, "Get32u", vec![buf2_pt.clone(), n(4)])?;
                    let mode = call(it, "Get8u", vec![buf2_pt.clone(), n(8)])?;
                    let lo = ops::bit_and(&mode, &n(0x0f));
                    let lvl = match lo.to_i64() {
                        0 => s("One Level"),
                        1 => s("MIMAP Levels"),
                        2 => s("RIPMAP Levels"),
                        _ => cat(&[s("Unknown Levels ("), ops::bit_and(&mode, &n(0xf)), s(")")]),
                    };
                    let hi = ops::shift_right(&mode, &n(4));
                    let rnd = match hi.to_i64() {
                        0 => s("Round Down"),
                        1 => s("Round Up"),
                        _ => cat(&[s("Unknown Rounding ("), hi.clone(), s(")")]),
                    };
                    val = cat(&[x, s("x"), y, s("; "), lvl, s("; "), rnd]);
                }
            } else if pl::str_eq(&type_, "chlist") {
                let list = Value::new_array(Vec::new());
                for c in re(r"\G([^\0]{1,31})\0(.{16})", "s").each(&b2) {
                    let (str_, dat) = (cap(&c, 1), cap(&c, 2));
                    let u = pl::unpack("VCx3VV", &dat)?;
                    let (pix, lin, x, y) = (arg(&u, 0), arg(&u, 1), arg(&u, 2), arg(&u, 3));
                    let pix = match &*pix.to_bytes() {
                        b"0" => s("int8u"),
                        b"1" => s("half"),
                        b"2" => s("float"),
                        _ => cat(&[s("unknown("), pix.clone(), s(")")]),
                    };
                    list.push(cat(&[str_, s(" "), pix, s(if lin.truthy() { " linear" } else { "" }), s(" "), x, s(" "), y]));
                }
                val = list;
            } else if pl::str_eq(&type_, "stringvector") {
                let list = Value::new_array(Vec::new());
                let mut pos = n(0);
                let l2 = pl::length(&b2);
                while pl::le(&pl::add(&pos, &n(4)), &l2) {
                    let len = call(it, "Get32u", vec![buf2_pt.clone(), pos.clone()])?;
                    if pl::gt(&pl::add(&pl::add(&pos, &n(4)), &len), &l2) {
                        break;
                    }
                    list.push(pl::substr(&b2, pl::add(&pos, &n(4)).to_i64(), Some(len.to_i64())));
                    pos = pl::add(&pl::add(&pos, &n(4)), &len);
                }
                val = list;
            } else {
                val = buf2_pt.clone(); // (shouldn't happen)
            }
        } else {
            // avoid loading binary data
            val = pl::sref(cat(&[s("Binary data "), size.clone(), s(" bytes")]));
            success = pl::method(it, &raf, "Seek", vec![size.clone(), n(1)])?.truthy();
        }
        if !success {
            pl::method(it, &et, "Warn", vec![s("Truncated or corrupted EXR file")])?;
            break;
        }
        if val.is_undef() {
            val = s("<bad>");
        }

        // take image dimensions from dataWindow (with displayWindow as backup)
        if pl::str_eq(&tag, "dataWindow") || (!dim.truthy() && pl::str_eq(&tag, "displayWindow")) {
            if let Some(m) = re(r"^(-?\d+) (-?\d+) (-?\d+) (-?\d+)$", "").m(&val) {
                if !et.get("DOC_NUM").truthy() {
                    dim = Value::new_array(vec![pl::add(&pl::sub(&cap(&m, 3), &cap(&m, 1)), &n(1)), pl::add(&pl::sub(&cap(&m, 4), &cap(&m, 2)), &n(1))]);
                }
            }
        }
        if verbose.truthy() {
            let data_pt = if pl::ref_of(&val) == "SCALAR" { val.clone() } else { buf2_pt.clone() };
            let addr = pl::sub(&pl::method(it, &raf, "Tell", vec![])?, &size);
            pl::method(
                it,
                &et,
                "VerboseInfo",
                vec![tag.clone(), tag_info.clone(), s("Table"), tag_table_ptr.clone(), s("Value"), val.clone(), s("Size"), size.clone(), s("Format"), type_.clone(), s("DataPt"), data_pt, s("Addr"), addr],
            )?;
        }
        pl::method(it, &et, "FoundTag", vec![tag_info, val])?;
    }
    et.delete("DOC_NUM");
    if dim.truthy() {
        pl::method(it, &et, "FoundTag", vec![s("ImageWidth"), dim.at(0)])?;
        pl::method(it, &et, "FoundTag", vec![s("ImageHeight"), dim.at(1)])?;
    }
    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::OpenEXR::ProcessEXR", process_exr);
}
