//! Image::ExifTool::DICOM (Image/ExifTool/DICOM.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read DICOM and ACR-NEMA medical images. The file's `my` hashes
//! (%dicomFormat, %vr32, %implicitVR) stay Perl data, read with
//! `lex_hash`; `%uid` is a package variable.

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell};
use crate::perl::ops;
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};
use crate::runtime;

const MODULE: &str = "Image::ExifTool::DICOM";

fn s(x: &str) -> Value {
    pl::s(x)
}

fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// `$raf->Read($buff, $len) == $len`
fn read_eq(it: &mut Interp, raf: &Value, buff: &Cell, len: &Value) -> ER<bool> {
    Ok(pl::num_eq(&raf_read_cell(it, raf, buff, len.clone())?, len))
}

/// `s/PAT/REPL/` where REPL interpolates the captures given as `$N` in
/// `parts` (bytes) and capture numbers.
fn subst_caps(val: &mut Value, pat: &str, flags: &str, parts: &[Result<&str, usize>]) -> bool {
    re(pat, flags).subst(val, false, |c| {
        let mut r = Vec::new();
        for p in parts {
            match p {
                Ok(t) => r.extend_from_slice(t.as_bytes()),
                Err(i) => r.extend_from_slice(&cap(c, *i).to_bytes()),
            }
        }
        r
    }) > 0
}

//------------------------------------------------------------------------------
// Extract information from a DICOM (DCM) image
// Inputs: 0) ExifTool object reference, 1) DirInfo reference
// Returns: 1 on success, 0 if this wasn't a valid DICOM file
pub fn process_dicom(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let mut raf = dir_info.get("RAF");
    let unknown = pl::method(it, &et, "Options", vec![s("Unknown")])?;
    let verbose = pl::method(it, &et, "Options", vec![s("Verbose")])?;
    let hdr = cell(Value::Undef);
    let buff = cell(Value::Undef); // (my $buff: \$buff is taken below)
    let buff_pt = Value::ScalarRef(buff.clone());
    let hdr_pt = Value::ScalarRef(hdr.clone());
    let mut implicit = Value::Undef;
    let (mut vr, mut len): (Value, Value);
    let dicom_format = runtime::lex_hash(it, MODULE, "dicomFormat");
    let vr32 = runtime::lex_hash(it, MODULE, "vr32");
    let implicit_vr = runtime::lex_hash(it, MODULE, "implicitVR");
    let uid = pl::pkg_hash(it, "Image::ExifTool::DICOM::uid");
    let bget = |c: &Cell| c.borrow().clone();
    //
    // identify the DICOM or ACR-NEMA file
    //
    if !read_eq(it, &raf, &hdr, &n(12))? {
        return ret(n(0)); // save for ACR identification later
    }
    if !pl::method(it, &raf, "Seek", vec![n(128), n(0)])?.truthy() {
        return ret(n(0)); // skip to end of DICM header
    }
    if !read_eq(it, &raf, &buff, &n(4))? {
        return ret(n(0)); // read signature
    }
    if pl::str_eq(&bget(&buff), "DICM") {
        // file meta information transfer syntax is explicit little endian
        call(it, "SetByteOrder", vec![s("II")])?;
        pl::method(it, &et, "SetFileType", vec![s("DICOM")])?;
    } else {
        // test for a RAW DCM image (ACR-NEMA format, ie. no header)
        for order in ["II", "MM", ""] {
            if order.is_empty() {
                return ret(n(0)); // no luck identifying the syntax
            }
            call(it, "SetByteOrder", vec![s(order)])?;
            let g = call(it, "Get16u", vec![hdr_pt.clone(), n(0)])?;
            // expect group number to be small and even
            if pl::lt(&g, &n(2)) || pl::gt(&g, &n(8)) || ops::bit_and(&g, &n(0x01)).truthy() {
                continue;
            }
            let e = call(it, "Get16u", vec![hdr_pt.clone(), n(2)])?;
            if pl::gt(&e, &n(0x20)) {
                continue; // expect a low element number at start
            }
            vr = pl::substr(&bget(&hdr), 4, Some(2)); // look for explicit VR
            if re(r"^[A-Z]{2}$", "").is_match(&vr) {
                implicit = n(0);
                if vr32.get(vr.to_bytes()).truthy() {
                    if !pl::num_eq(&call(it, "Get16u", vec![hdr_pt.clone(), n(6)])?, &n(0)) {
                        continue; // must be 2 zero bytes
                    }
                    len = call(it, "Get32u", vec![hdr_pt.clone(), n(8)])?;
                } else {
                    if pl::num_eq(&e, &n(0)) && !pl::str_eq(&vr, "UL") {
                        continue; // group length must be UL
                    }
                    len = call(it, "Get16u", vec![hdr_pt.clone(), n(6)])?;
                }
            } else {
                implicit = n(1);
                len = call(it, "Get32u", vec![hdr_pt.clone(), n(4)])?;
            }
            if pl::num_eq(&e, &n(0)) && !pl::num_eq(&len, &n(4)) {
                continue; // group length value must be 4 bytes
            }
            if pl::gt(&len, &n(64)) {
                continue; // first element shouldn't be too long
            }
            break; // success!
        }
        if !pl::method(it, &raf, "Seek", vec![n(0), n(0)])?.truthy() {
            return ret(n(0)); // rewind to start of file
        }
        pl::method(it, &et, "SetFileType", vec![s("ACR")])?;
    }
    //
    // process the meta information
    //
    let tag_table_ptr = call(it, "GetTagTable", vec![s("Image::ExifTool::DICOM::Main")])?;
    let mut pos = pl::method(it, &raf, "Tell", vec![])?;
    let mut err = true;
    let (mut transfer_syntax, mut group2end) = (Value::Undef, Value::Undef);
    loop {
        if !read_eq(it, &raf, &buff, &n(8))? {
            err = false;
            break;
        }
        pos = pl::add(&pos, &n(8));
        let mut group = call(it, "Get16u", vec![buff_pt.clone(), n(0)])?;
        // implement the transfer syntax at the end of the group 2 data
        if transfer_syntax.truthy() && (!pl::num_eq(&group, &n(0x0002)) || (group2end.truthy() && pl::gt(&pos, &group2end))) {
            // 1.2.840.10008.1.2   = implicit VR little endian
            // 1.2.840.10008.1.2.2 = explicit VR big endian
            // 1.2.840.10008.1.2.x = explicit VR little endian
            // 1.2.840.10008.1.2.1.99 = deflated
            let Some(m) = re(r"^1\.2\.840\.10008\.1\.2(\.\d+)?(\.\d+)?", "").m(&transfer_syntax) else {
                let msg = pl::concat(&s("Unrecognized transfer syntax "), &transfer_syntax);
                pl::method(it, &et, "Warn", vec![msg])?;
                break;
            };
            let (c1, c2) = (cap(&m, 1), cap(&m, 2));
            if !c1.truthy() {
                implicit = n(1);
            } else if pl::str_eq(&c1, ".2") {
                call(it, "SetByteOrder", vec![s("MM")])?;
                group = call(it, "Get16u", vec![buff_pt.clone(), n(0)])?; // must get group again
            } else if pl::str_eq(&c1, ".1") && c2.truthy() && pl::str_eq(&c2, ".99") {
                // inflate compressed data stream
                if pl::eval_block(it, |it| runtime::require(it, "Compress::Zlib"))?.is_some() {
                    // must use undocumented zlib feature to disable zlib header information
                    // because DICOM deflated data doesn't have the zlib header (ref 3)
                    let wbits = pl::neg(&pl::call(it, "Compress::Zlib::MAX_WBITS", vec![])?);
                    let inflate = pl::call(it, "Compress::Zlib::inflateInit", vec![s("-WindowBits"), wbits])?;
                    if inflate.truthy() {
                        if !pl::method(it, &raf, "Seek", vec![n(-8), n(1)])?.truthy() {
                            break;
                        }
                        let mut data = s("");
                        while raf_read_cell(it, &raf, &buff, n(65536))?.truthy() {
                            let r = pl::method_list(it, &inflate, "inflate", vec![bget(&buff)])?;
                            let (buf, stat) = (arg(&r, 0), arg(&r, 1));
                            let z_ok = pl::call(it, "Compress::Zlib::Z_OK", vec![])?;
                            let z_end = pl::call(it, "Compress::Zlib::Z_STREAM_END", vec![])?;
                            if pl::num_eq(&stat, &z_ok) || pl::num_eq(&stat, &z_end) {
                                data = pl::concat(&data, &buf);
                                if pl::num_eq(&stat, &z_end) {
                                    break;
                                }
                            } else {
                                pl::method(it, &et, "Warn", vec![s("Error inflating compressed data stream")])?;
                                return ret(n(1));
                            }
                        }
                        if pl::lt(&pl::length(&data), &n(8)) {
                            break;
                        }
                        // create new RAF object from inflated data stream
                        raf = pl::method(it, &s("File::RandomAccess"), "new", vec![pl::sref(data)])?;
                        // re-read start of stream (now decompressed)
                        if !read_eq(it, &raf, &buff, &n(8))? {
                            break;
                        }
                        group = call(it, "Get16u", vec![buff_pt.clone(), n(0)])?;
                    } else {
                        pl::method(it, &et, "Warn", vec![s("Error initializing inflation")])?;
                        return ret(n(1));
                    }
                } else {
                    pl::method(it, &et, "Warn", vec![s("Install Compress::Zlib to decode compressed data stream")])?;
                    return ret(n(1));
                }
            }
            transfer_syntax = Value::Undef;
        }
        let element = call(it, "Get16u", vec![buff_pt.clone(), n(2)])?;
        let mut tag = pl::sprintf("%.4X,%.4X", &[group.clone(), element.clone()]);

        if implicit.truthy() || implicit_vr.get(tag.to_bytes()).truthy() {
            // treat everything as string if implicit VR because it
            // isn't worth it to generate the necessary VR lookup tables
            // for the thousands of defined data elements
            vr = s(""); // no VR (treat everything as string)
            len = call(it, "Get32u", vec![buff_pt.clone(), n(4)])?;
        } else {
            vr = pl::substr(&bget(&buff), 4, Some(2));
            if !re(r"^[A-Z]{2}$", "").is_match(&vr) {
                break;
            }
            if vr32.get(vr.to_bytes()).truthy() {
                if !read_eq(it, &raf, &buff, &n(4))? {
                    break;
                }
                pos = pl::add(&pos, &n(4));
                len = call(it, "Get32u", vec![buff_pt.clone(), n(0)])?;
                if pl::str_eq(&vr, "SQ") {
                    len = n(0); // just recurse into sequences
                }
            } else {
                len = call(it, "Get16u", vec![buff_pt.clone(), n(6)])?;
            }
        }
        if pl::num_eq(&len, &n(0xffffffff)) {
            len = n(0); // don't read value if undefined length
            if verbose.truthy() {
                // start list of items in verbose output
                let msg = pl::concat(&et.get("INDENT"), &s("+ [List of items]\n"));
                pl::method(it, &et, "VPrint", vec![n(0), msg])?;
                et.set("INDENT", pl::concat(&et.get("INDENT"), &s("| ")));
            }
        }
        // read the element value
        if len.truthy() {
            if !read_eq(it, &raf, &buff, &len)? {
                break;
            }
            pos = pl::add(&pos, &len);
        } else {
            *buff.borrow_mut() = s("");
        }

        // handle tags not found in the table
        let mut tag_info = tag_table_ptr.get(tag.to_bytes());
        if !tag_info.truthy() {
            // accept tag ID's with "x" for a wildcard in the following patterns:
            // '60xx,1203', '0020,31xx', '0028,04x2', '1000,xxx0', '1010,xxxx'
            let try_xx = |pat: &str, parts: &[Result<&str, usize>]| -> Option<Value> {
                let mut xx = tag.clone();
                if subst_caps(&mut xx, pat, "", parts) && tag_table_ptr.get(xx.to_bytes()).truthy() {
                    Some(xx)
                } else {
                    None
                }
            };
            let found = try_xx(r"^(..)..", &[Err(1), Ok("xx")])
                .or_else(|| try_xx(r"..$", &[Ok("xx")]))
                .or_else(|| try_xx(r".(.)$", &[Ok("x"), Err(1)]))
                .or_else(|| try_xx(r"...(.)$", &[Ok("xxx"), Err(1)]))
                .or_else(|| try_xx(r"....$", &[Ok("xxxx")]));
            if let Some(xx) = found {
                tag_info = tag_table_ptr.get(xx.to_bytes());
                tag = xx;
            } else if unknown.truthy() {
                // create tag info hash for unknown elements
                tag_info = Value::new_hash();
                if pl::num_eq(&element, &n(0)) {
                    // element zero is group length
                    tag_info.set("Name", pl::sprintf("Group%.4X_Length", &[group.clone()]));
                    tag_info.set("Description", pl::sprintf("Group %.4X Length", &[group.clone()]));
                } else {
                    tag_info.set("Name", pl::sprintf("DICOM_%.4X_%.4X", &[group.clone(), element.clone()]));
                    tag_info.set("Description", pl::sprintf("DICOM %.4X,%.4X", &[group.clone(), element.clone()]));
                }
                tag_info.set("Unknown", n(1));
                call(it, "AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), tag_info.clone()])?;
            }
        }
        // get VR from our tag information if implicit
        if tag_info.truthy() && !vr.truthy() {
            let v = tag_info.get("VR");
            vr = if v.truthy() { v } else { s("  ") };
        }

        if pl::num_eq(&element, &n(0)) {
            vr = s("UL"); // group length element is always unsigned long
        }
        let mut val;
        let mut format = dicom_format.get(vr.to_bytes());
        // remove trailing space used to pad to an even number of characters
        if !(format.truthy() || pl::length(&bget(&buff)).to_i64() & 0x01 != 0) {
            let mut b = bget(&buff);
            if re(r" $", "").subst(&mut b, false, |_| Vec::new()) > 0 {
                *buff.borrow_mut() = b;
            }
        }
        if pl::gt(&len, &n(1024)) {
            // treat large data elements as binary data
            let bin_data;
            let lc_tag = if tag_info.truthy() { pl::lc(&tag_info.get("Name")) } else { s("unknown") };
            if et.get("REQ_TAG_LOOKUP").get(lc_tag.to_bytes()).truthy()
                || (et.path(&["OPTIONS", "Binary"]).truthy() && !et.get("EXCL_TAG_LOOKUP").get(lc_tag.to_bytes()).truthy())
            {
                bin_data = bget(&buff); // must make a copy
            } else {
                bin_data = pl::concat(&pl::concat(&s("Binary data "), &len), &s(" bytes"));
            }
            val = pl::sref(bin_data);
        } else if format.truthy() {
            val = call(it, "ReadValue", vec![buff_pt.clone(), n(0), format.clone(), Value::Undef, len.clone()])?;
        } else {
            val = bget(&buff);
            format = s("string");
            if pl::str_eq(&vr, "DA") {
                // format date values
                subst_caps(&mut val, r"^ *(\d{4})(\d{2})(\d{2})", "", &[Err(1), Ok(":"), Err(2), Ok(":"), Err(3)]);
            } else if pl::str_eq(&vr, "TM") {
                // format time values
                subst_caps(&mut val, r"^ *(\d{2})(\d{2})(\d{2}[^ ]*)", "", &[Err(1), Ok(":"), Err(2), Ok(":"), Err(3)]);
            } else if pl::str_eq(&vr, "DT") {
                // format date/time values
                subst_caps(
                    &mut val,
                    r"^ *(\d{4})(\d{2})(\d{2})(\d{2})(\d{2})(\d{2}[^ ]*)",
                    "",
                    &[Err(1), Ok(":"), Err(2), Ok(":"), Err(3), Ok(" "), Err(4), Ok(":"), Err(5), Ok(":"), Err(6)],
                );
            } else if pl::str_eq(&vr, "AT") && pl::num_eq(&len, &n(4)) {
                // convert attribute tag ID to hex format
                let g = call(it, "Get16u", vec![buff_pt.clone(), n(0)])?;
                let e = call(it, "Get16u", vec![buff_pt.clone(), n(2)])?;
                val = pl::sprintf("%.4X,%.4X", &[g, e]);
            } else if pl::str_eq(&vr, "UI") {
                // add PrintConv to translate registered UID's
                re(r"\0.*", "s").subst(&mut val, false, |_| Vec::new()); // truncate at null
                if tag_info.truthy() {
                    if uid.get(val.to_bytes()).truthy() {
                        tag_info.set("PrintConv", uid.clone());
                    }
                    if !tag_info.exists("Hidden") || tag_info.get("Hidden").is_undef() {
                        tag_info.set("Hidden", s("")); // (don't add PrintConv to -listx output)
                    }
                }
            } else if re(r"^(AE|CS|DS|IS|LO|PN|SH)$", "").is_match(&vr) {
                re(r" +$", "").subst(&mut val, false, |_| Vec::new()); // leading/trailing spaces not significant
                re(r"^ +", "").subst(&mut val, false, |_| Vec::new());
            } else if re(r"^(LT|ST|UT)$", "").is_match(&vr) {
                re(r" +$", "").subst(&mut val, false, |_| Vec::new()); // trailing spaces not significant
            }
        }
        // save the group 2 end position and transfer syntax
        if pl::num_eq(&group, &n(0x0002)) {
            if pl::num_eq(&element, &n(0x0000)) {
                group2end = pl::add(&pos, &val);
            }
            if pl::num_eq(&element, &n(0x0010)) {
                transfer_syntax = val.clone();
            }
        }

        // handle the new tag information
        pl::method(
            it,
            &et,
            "HandleTag",
            vec![
                tag_table_ptr.clone(),
                tag.clone(),
                val,
                s("DataPt"),
                buff_pt.clone(),
                s("DataPos"),
                pl::sub(&pos, &len),
                s("Format"),
                format,
                s("Start"),
                n(0),
                s("Size"),
                len.clone(),
                s("Extra"),
                pl::concat(&pl::concat(&s(" ("), &vr), &s(")")),
            ],
        )?;

        // stop indenting for list if we reached EndOfItems tag
        if verbose.truthy() && pl::str_eq(&tag, "FFFE,E00D") {
            let mut indent = et.get("INDENT");
            re(r"..$", "").subst(&mut indent, false, |_| Vec::new());
            et.set("INDENT", indent);
        }
    }
    if err {
        pl::method(it, &et, "Warn", vec![s("Error reading DICOM file (corrupted?)")])?;
    }
    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::DICOM::ProcessDICOM", process_dicom);
}
