//! Image::ExifTool::InDesign (Image/ExifTool/InDesign.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read/write meta information in Adobe InDesign files. The file's `my`
//! variables (%indMap and the three GUIDs) stay Perl data, read with
//! `lex_hash` / `lex_scalar`.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read, raf_read_cell};
use crate::perl::ops;
use crate::perl::Value;
use crate::pl::{self, arg, n, re, ret};
use crate::runtime;

/// `$raf->METHOD(...)` on an undefined `$raf` dies as perl does, at the
/// statement's location (the Read helpers run the call as an eval string,
/// which has a location of its own).
fn check_invocant(raf: &Value, method: &str) -> ER<()> {
    if raf.is_undef() {
        return crate::interp::eval::die(format!("Can't call method \"{method}\" on an undefined value"));
    }
    Ok(())
}

const MODULE: &str = "Image::ExifTool::InDesign";

fn s(x: &str) -> Value {
    pl::s(x)
}

fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

fn lex(it: &mut Interp, name: &str) -> Value {
    runtime::lex_scalar(it, MODULE, name).borrow().clone()
}

/// `$data =~ /^\Q$guid/`
fn starts_with(data: &Value, guid: &Value) -> bool {
    data.to_bytes().starts_with(&guid.to_bytes())
}

//------------------------------------------------------------------------------
// Read or write meta information in an InDesign file
// Inputs: 0) ExifTool object reference, 1) dirInfo reference
// Returns: 1 on success, 0 if this wasn't a valid InDesign file, or -1 on write error
pub fn process_ind(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let outfile = dir_info.get("OutFile");
    let mut hdr = Value::Undef;
    let buff = cell(Value::Undef); // (my $buff: \$buff is taken below)
    let buf2 = cell(Value::Undef);
    let mut err = Value::Undef;
    let mut write_len = Value::Undef;
    let mut found_xmp = false;
    let master_page_guid = lex(it, "masterPageGUID");
    let object_header_guid = lex(it, "objectHeaderGUID");
    let object_trailer_guid = lex(it, "objectTrailerGUID");
    let bget = |c: &crate::interp::eval::Cell| c.borrow().clone();

    // validate the InDesign file
    pl::at(41);
    check_invocant(&raf, "Read")?;
    if !pl::num_eq(&raf_read(it, &raf, &mut hdr, n(16))?, &n(16)) {
        return ret(n(0));
    }
    if hdr.to_bytes() != master_page_guid.to_bytes() {
        return ret(n(0));
    }
    if !pl::num_eq(&raf_read_cell(it, &raf, &buff, n(8))?, &n(8)) {
        return ret(n(0));
    }
    let ft = if pl::str_eq(&bget(&buff), "DOCUMENT") { "INDD" } else { "IND" };
    pl::method(it, &et, "SetFileType", vec![s(ft)])?; // set the FileType tag

    'done: {
        // read the master pages
        if !pl::method(it, &raf, "Seek", vec![n(0), n(0)])?.truthy() {
            err = s("Seek error");
            break 'done;
        }
        if !(pl::num_eq(&raf_read_cell(it, &raf, &buff, n(4096))?, &n(4096)) && pl::num_eq(&raf_read_cell(it, &raf, &buf2, n(4096))?, &n(4096))) {
            err = s("Unexpected end of file");
            break 'done; // (goto's can be our friend)
        }
        call(it, "SetByteOrder", vec![s("II")])?;
        if !starts_with(&bget(&buf2), &master_page_guid) {
            err = s("Second master page is invalid");
            break 'done;
        }
        let seq1 = call(it, "Get64u", vec![Value::ScalarRef(buff.clone()), n(264)])?;
        let seq2 = call(it, "Get64u", vec![Value::ScalarRef(buf2.clone()), n(264)])?;
        // take the most current master page
        let cur_page = if pl::gt(&seq2, &seq1) { Value::ScalarRef(buf2.clone()) } else { Value::ScalarRef(buff.clone()) };
        // byte order of stream data may be different than headers
        let stream_int32u;
        let sb = call(it, "Get8u", vec![cur_page.clone(), n(24)])?;
        if pl::num_eq(&sb, &n(1)) {
            stream_int32u = "V"; // little-endian int32u
        } else if pl::num_eq(&sb, &n(2)) {
            stream_int32u = "N"; // big-endian int32u
        } else {
            err = s("Invalid stream byte order");
            break 'done;
        }
        let pages = call(it, "Get32u", vec![cur_page.clone(), n(280)])?;
        if pl::lt(&pages, &n(2)) {
            err = s("Invalid page count");
            break 'done;
        }
        let pos = pl::mul(&pages, &n(4096));
        if pl::gt(&pos, &n(0x7fffffff)) {
            if !pl::method(it, &et, "Options", vec![s("LargeFileSupport")])?.truthy() {
                err = s("InDesign files larger than 2 GB not supported (LargeFileSupport not set)");
                break 'done;
            } else if pl::str_eq(&pl::method(it, &et, "Options", vec![s("LargeFileSupport")])?, "2") {
                pl::method(it, &et, "Warn", vec![s("Processing large file (LargeFileSupport is 2)")])?;
            }
        }
        if outfile.truthy() {
            // make XMP the preferred group for writing
            let ind_map = runtime::lex_hash(it, MODULE, "indMap");
            pl::method(it, &et, "InitWriteDirs", vec![ind_map, s("XMP")])?;

            if !call(it, "Write", vec![outfile.clone(), bget(&buff), bget(&buf2)])?.truthy() {
                err = n(1);
                break 'done;
            }
            let result = call(it, "CopyBlock", vec![raf.clone(), outfile.clone(), pl::sub(&pos, &n(8192))])?;
            if !result.truthy() {
                err = if !result.is_undef() { s("Error reading InDesign database") } else { n(1) };
                break 'done;
            }
            write_len = n(0);
        } else if !pl::method(it, &raf, "Seek", vec![pos.clone(), n(0)])?.truthy() {
            err = s("Seek error");
            break 'done;
        }
        // scan through the contiguous objects for XMP
        let verbose = pl::method(it, &et, "Options", vec![s("Verbose")])?;
        let out = pl::method(it, &et, "Options", vec![s("TextOut")])?;
        loop {
            if !raf_read(it, &raf, &mut hdr, n(32))?.truthy() {
                break;
            }
            if !(pl::length(&hdr).to_i64() == 32 && starts_with(&hdr, &object_header_guid)) {
                // this must be null padding or we have a possible error
                if re(r"^\0+$", "").is_match(&hdr) {
                    break;
                }
                // (could be up to 4095 bytes of non-null garbage plus 4095 null bytes from ExifTool)
                if raf_read_cell(it, &raf, &buff, n(8192))?.truthy() {
                    hdr = pl::concat(&hdr, &bget(&buff));
                }
                let nn = pl::length(&hdr).to_i64();
                re(r"\0+$", "").subst(&mut hdr, false, |_| Vec::new()); // remove trailing nulls
                if nn > 8190 || pl::length(&hdr).to_i64() > 4095 {
                    err = s("Corrupt file or unsupported InDesign version");
                    break;
                }
                let non = s("Non-null padding at end of file");
                if !outfile.truthy() {
                    pl::method(it, &et, "Warn", vec![non, n(1)])?;
                } else if !pl::method(it, &et, "Error", vec![non, n(1)])?.truthy() {
                    if !call(it, "Write", vec![outfile.clone(), hdr.clone()])?.truthy() {
                        err = n(1);
                    }
                    write_len = pl::add(&write_len, &pl::length(&hdr));
                }
                break;
            }
            let mut len = call(it, "Get32u", vec![pl::sref(hdr.clone()), n(24)])?;
            if verbose.truthy() {
                let t = pl::method(it, &raf, "Tell", vec![])?;
                let line = pl::sprintf("Contiguous object at offset 0x%x (%d bytes):\n", &[t, len.clone()]);
                pl::print_to(it, Some(&out), &[line])?;
                if pl::gt(&verbose, &n(2)) {
                    let len2 = if pl::lt(&len, &n(1024000)) { len.clone() } else { n(1024000) };
                    let r = raf_read_cell(it, &raf, &buff, len2)?;
                    if !pl::method(it, &raf, "Seek", vec![pl::neg(&r), n(1)])?.truthy() {
                        err = n(1);
                    }
                    let t = pl::method(it, &raf, "Tell", vec![])?;
                    pl::method(it, &et, "VerboseDump", vec![Value::ScalarRef(buff.clone()), s("Addr"), t])?;
                }
            }
            // check for XMP if stream data is long enough
            // (56 bytes is just enough for XMP header)
            if pl::gt(&len, &n(56)) {
                if !pl::num_eq(&raf_read_cell(it, &raf, &buff, n(56))?, &n(56)) {
                    err = s("Unexpected end of file");
                    break;
                }
                let m = re(r#"^(....)<\?xpacket begin=(['"])\xef\xbb\xbf\2 id=(['"])W5M0MpCehiHzreSzNTczkc9d\3"#, "s").m(&bget(&buff));
                if let Some(m) = m {
                    let len_word = pl::cap(&m, 1); // save length word for writing later
                    len = pl::sub(&len, &n(4)); // get length of XMP only
                    found_xmp = true;
                    // I have a sample where the XMP is 107 MB, and ActivePerl may run into
                    // memory troubles (with its apparent 1 GB limit) if the XMP is larger
                    // than about 400 MB, so guard against this
                    if pl::gt(&len, &n(300 * 1024 * 1024)) {
                        let msg = pl::sprintf("Insanely large XMP (%.0f MiB)", &[pl::div(&len, &n(1024 * 1024))?]);
                        if outfile.truthy() {
                            if pl::method(it, &et, "Error", vec![msg, n(2)])?.truthy() {
                                err = n(1);
                                break;
                            }
                        } else if pl::method(it, &et, "Options", vec![s("IgnoreMinorErrors")])?.truthy() {
                            pl::method(it, &et, "Warn", vec![msg])?;
                        } else {
                            pl::method(it, &et, "Warn", vec![pl::concat(&msg, &s(". Ignored.")), n(1)])?;
                            err = n(1);
                            break;
                        }
                    }
                    // load and parse the XMP data
                    if !(pl::method(it, &raf, "Seek", vec![n(-52), n(1)])?.truthy() && pl::num_eq(&raf_read_cell(it, &raf, &buff, len.clone())?, &len)) {
                        err = s("Error reading XMP stream");
                        break;
                    }
                    let dir_info2 = Value::new_hash();
                    dir_info2.set("DataPt", Value::ScalarRef(buff.clone()));
                    dir_info2.set("Parent", s("IND"));
                    dir_info2.set("NoDelete", n(1)); // do not allow this to be deleted when writing
                    // validate xmp data length (should be same as length in header - 4)
                    let xmp_len = pl::unpack1(stream_int32u, &len_word)?;
                    if !pl::num_eq(&xmp_len, &len) {
                        if pl::lt(&xmp_len, &len) {
                            dir_info2.set("DirLen", xmp_len.clone());
                        } else {
                            err = pl::concat(&pl::concat(&s("Truncated XMP stream (missing "), &pl::sub(&xmp_len, &len)), &s(" bytes)"));
                        }
                    }
                    let tag_table_ptr = call(it, "GetTagTable", vec![s("Image::ExifTool::XMP::Main")])?;
                    if outfile.truthy() {
                        if err.truthy() {
                            break;
                        }
                        // make sure that XMP is writable
                        let class_id = call(it, "Get32u", vec![pl::sref(hdr.clone()), n(20)])?;
                        if !ops::bit_and(&class_id, &n(0x40000000)).truthy() {
                            err = s("XMP stream is not writable");
                            break;
                        }
                        let xmp = pl::method(it, &et, "WriteDirectory", vec![dir_info2, tag_table_ptr])?;
                        if xmp.truthy() && pl::length(&xmp).truthy() {
                            // write new xmp with leading length word
                            let b = pl::concat(&pl::pack(stream_int32u, &[pl::length(&xmp)])?, &xmp);
                            *buff.borrow_mut() = b.clone();
                            // update header with new length and invalid checksum
                            let mut args = [pl::length(&b), pl::sref(hdr.clone()), n(24)];
                            hdr = set32u_hdr(it, &mut args)?;
                            let mut args = [n(0xffffffff), pl::sref(hdr.clone()), n(28)];
                            hdr = set32u_hdr(it, &mut args)?;
                        } else {
                            et.set("CHANGED", n(0)); // didn't change anything
                            if !xmp.is_undef() {
                                pl::method(it, &et, "Warn", vec![s("Can't delete XMP as a block from InDesign file")])?;
                            }
                            // put length word back at start of stream
                            let b = pl::concat(&len_word, &bget(&buff));
                            *buff.borrow_mut() = b;
                        }
                    } else {
                        pl::method(it, &et, "ProcessDirectory", vec![dir_info2, tag_table_ptr])?;
                    }
                    len = n(0); // we got the full stream (nothing left to read)
                } else {
                    len = pl::sub(&len, &n(56)); // we got 56 bytes of the stream
                }
            } else {
                *buff.borrow_mut() = s(""); // must reset this for writing later
            }
            if outfile.truthy() {
                // write object header and data
                if !call(it, "Write", vec![outfile.clone(), hdr.clone(), bget(&buff)])?.truthy() {
                    err = n(1);
                    break;
                }
                let result = call(it, "CopyBlock", vec![raf.clone(), outfile.clone(), len.clone()])?;
                if !result.truthy() {
                    err = if !result.is_undef() { s("Truncated stream data") } else { n(1) };
                    break;
                }
                write_len = pl::add(&pl::add(&pl::add(&write_len, &n(32)), &pl::length(&bget(&buff))), &len);
            } else if len.truthy() {
                // skip over remaining stream data
                if !pl::method(it, &raf, "Seek", vec![len.clone(), n(1)])?.truthy() {
                    err = s("Seek error");
                    break;
                }
            }
            if !pl::num_eq(&raf_read_cell(it, &raf, &buff, n(32))?, &n(32)) {
                err = s("Unexpected end of file");
                break;
            }
            if !starts_with(&bget(&buff), &object_trailer_guid) {
                err = s("Invalid object trailer");
                break;
            }
            if outfile.truthy() {
                // make sure object UID and ClassID are the same in the trailer
                if pl::substr(&hdr, 16, Some(8)).to_bytes() != pl::substr(&bget(&buff), 16, Some(8)).to_bytes() {
                    err = s("Non-matching object trailer");
                    break;
                }
                // write object trailer
                if !call(it, "Write", vec![outfile.clone(), object_trailer_guid.clone(), pl::substr(&hdr, 16, None)])?.truthy() {
                    err = n(1);
                    break;
                }
                write_len = pl::add(&write_len, &n(32));
            }
        }
        if outfile.truthy() {
            // write null padding if necessary
            // (InDesign files must be an even number of 4096-byte blocks)
            let part = pl::modulo(&write_len, &n(4096))?;
            if part.truthy() {
                let pad = Value::bytes(vec![0u8; (4096 - part.to_i64()).max(0) as usize]);
                if !call(it, "Write", vec![outfile.clone(), pad])?.truthy() {
                    err = n(1);
                }
            }
        }
    }
    // DONE:
    if !err.truthy() {
        if outfile.truthy() && !found_xmp {
            pl::method(it, &et, "Warn", vec![s("No XMP stream to edit")])?;
        }
        return ret(n(1)); // success!
    } else if !outfile.truthy() {
        // issue warning on read error
        if !pl::str_eq(&err, "1") {
            pl::method(it, &et, "Warn", vec![err])?;
        }
    } else if !pl::str_eq(&err, "1") {
        // set error and return success code
        pl::method(it, &et, "Error", vec![err])?;
    } else {
        return ret(n(-1)); // write error
    }
    ret(n(1))
}

/// `Set32u($val, \$hdr, $off)`: returns the updated `$hdr`.
fn set32u_hdr(it: &mut Interp, args: &mut [Value; 3]) -> ER<Value> {
    let hdr_ref = args[1].clone();
    call(it, "Set32u", args.to_vec())?;
    Ok(hdr_ref.deref())
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::InDesign::ProcessIND", process_ind);
}
