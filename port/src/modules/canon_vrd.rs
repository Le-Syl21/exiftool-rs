//! Image::ExifTool::CanonVRD (Image/ExifTool/CanonVRD.pm), ported sub by sub (see docs/port/PORTING.md).

use std::cmp::Ordering;

use crate::interp::eval::{die, Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell, substr};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, n, re, ret};

/// `$raf->METHOD(...)` on an undefined `$raf` dies as perl does, at the
/// statement's location (the Read helpers run the call as an eval string,
/// which has a location of its own).
fn check_invocant(raf: &Value, method: &str) -> ER<()> {
    if raf.is_undef() {
        return crate::interp::eval::die(format!("Can't call method \"{method}\" on an undefined value"));
    }
    Ok(())
}

const MODULE: &str = "Image::ExifTool::CanonVRD";

/// `Image::ExifTool::NAME(ARGS)` (the functions CanonVRD.pm imports).
fn et(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

fn val(c: &Cell) -> Value {
    c.borrow().clone()
}

fn put(c: &Cell, v: Value) {
    *c.borrow_mut() = v;
}

/// `\$var` of a `my` variable kept in a cell.
fn rf(c: &Cell) -> Value {
    Value::ScalarRef(c.clone())
}

/// `$raf->Seek(POS, WHENCE)`
fn seek(it: &mut Interp, raf: &Value, pos: Value, whence: i64) -> ER<bool> {
    Ok(pl::method(it, raf, "Seek", vec![pos, n(whence)])?.truthy())
}

/// `$raf->Read($$ref, $len)`: reads into the scalar a reference points to.
fn read_into(it: &mut Interp, raf: &Value, r: &Value, len: &Value) -> ER<Value> {
    if let Value::ScalarRef(c) = r {
        return raf_read_cell(it, raf, c, len.clone());
    }
    let mut args = [r.deref(), len.clone()];
    let rv = pl::method_mut(it, raf, "Read", &mut args, Want::Scalar)?;
    pl::set_deref(r, args[0].clone())?;
    Ok(rv.into_iter().next().unwrap_or_default())
}

/// `substr($$ref, OFF, LEN) = NEW` (4-argument substr on the referenced scalar).
fn substr_set(r: &Value, off: &Value, len: &Value, new: &Value) -> ER<()> {
    let b = r.deref().to_bytes();
    let Some((s, e)) = crate::interp::eval::substr_range(b.len(), off.to_i64(), Some(len.to_i64())) else {
        return die("substr outside of string");
    };
    let mut out = b[..s].to_vec();
    out.extend_from_slice(&new.to_bytes());
    out.extend_from_slice(&b[e..]);
    pl::set_deref(r, Value::bytes(out))
}

/// `print $out STRING`
fn print_out(it: &mut Interp, out: &Value, s: Value) -> ER<()> {
    pl::print_to(it, Some(out), &[s])?;
    Ok(())
}

/// `$a eq $b` (references compare as their strings, as in perl).
fn str_eqv(a: &Value, b: &Value) -> bool {
    a.to_bytes() == b.to_bytes()
}

/// `hex $str`, as perl: an optional "0x" (or "x") prefix, underscores
/// ignored, stopping at the first non-hex character.
fn hex(v: &Value) -> Value {
    let b = v.to_bytes();
    let mut s: &[u8] = &b;
    if s.starts_with(b"0x") || s.starts_with(b"0X") {
        s = &s[2..];
    } else if s.starts_with(b"x") || s.starts_with(b"X") {
        s = &s[1..];
    }
    let mut x: f64 = 0.0;
    for &c in s {
        if c == b'_' {
            continue;
        }
        match (c as char).to_digit(16) {
            Some(d) => x = x * 16.0 + d as f64,
            None => break,
        }
    }
    if x <= u64::MAX as f64 {
        ops::from_uv(x as u64)
    } else {
        Value::Num(x)
    }
}

/// `$a <=> $b`
fn cmp(a: &Value, b: &Value) -> i64 {
    match ops::num_cmp(a, b) {
        Some(Ordering::Less) => -1,
        Some(Ordering::Equal) => 0,
        Some(Ordering::Greater) => 1,
        None => 0,
    }
}

/// sort DR4 tag ID's for the documentation
pub fn sort_dr4(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (av, bv) = (arg(&a, 0), arg(&a, 1));
    let (mut a_hex, mut a_dec, mut b_hex, mut b_dec) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef);
    let pat = re(r"^(0x[0-9a-f]+)?\.?(\d*?)$", "");
    if let Some(m) = pat.m(&av) {
        (a_hex, a_dec) = (cap(&m, 1), cap(&m, 2));
    }
    if let Some(m) = pat.m(&bv) {
        (b_hex, b_dec) = (cap(&m, 1), cap(&m, 2));
    }
    if a_hex.truthy() {
        if b_dec.is_undef() {
            return ret(n(1)); // $b is 'header';
        }
        if b_hex.truthy() {
            let c = cmp(&hex(&a_hex), &hex(&b_hex));
            return ret(n(if c != 0 { c } else { cmp(&a_dec, &b_dec) }));
        }
        let c = cmp(&hex(&a_hex), &b_dec);
        ret(n(if c != 0 { c } else { 1 }))
    } else if b_hex.truthy() {
        if a_dec.is_undef() {
            return ret(n(-1));
        }
        let c = cmp(&a_dec, &hex(&b_hex));
        ret(n(if c != 0 { c } else { -1 }))
    } else {
        if b_dec.is_undef() {
            return ret(n(1));
        }
        if a_dec.is_undef() {
            return ret(n(-1));
        }
        ret(n(cmp(&a_dec, &b_dec)))
    }
}

/// Tone curve print conversion
pub fn tone_curve_print(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut val = arg(&a, 0);
    let mut vals = pl::split_ws(&val);
    if vals.len() != 21 {
        return ret(val);
    }
    let mut nn = vals.remove(0);
    if !(pl::ge(&nn, &n(2)) && pl::le(&nn, &n(10))) {
        return ret(val);
    }
    val = pl::s("");
    let mut vals = std::collections::VecDeque::from(vals);
    loop {
        let t = nn.truthy();
        nn = pl::sub(&nn, &n(1));
        if !t {
            break;
        }
        if val.truthy() {
            val = pl::concat(&val, &pl::s(" "));
        }
        let x = vals.pop_front().unwrap_or_default();
        let y = vals.pop_front().unwrap_or_default();
        val = pl::concat(&val, &pl::concat(&pl::concat(&pl::concat(&pl::concat(&pl::s("("), &x), &pl::s(",")), &y), &pl::s(")")));
    }
    ret(val)
}

/// Inverse print conversion for tone curve
pub fn tone_curve_print_inv(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let val = arg(&a, 0);
    let mut vals = re(r"\((\d+),(\d+)\)", "").all(&val);
    if !(vals.len() >= 4 && vals.len() <= 20 && vals.len() & 0x01 == 0) {
        return pl::ret_undef();
    }
    let half = pl::div(&n(vals.len() as i64), &n(2))?;
    vals.insert(0, half);
    while vals.len() < 21 {
        vals.push(n(0));
    }
    ret(pl::join(" ", &vals))
}

/// Read/Write VRD edit data
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: Reading: 1 on success; Writing: modified edit data, or undef if nothing changed
pub fn process_edit_data(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et_, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if !et_.truthy() {
        return ret(n(1)); // allow dummy access
    }
    let mut data_pt = dir_info.get("DataPt");
    let mut pos = dir_info.get("DirStart");
    let mut data_pos = dir_info.get("DataPos");
    let outfile = dir_info.get("OutFile");
    let mut dir_len = dir_info.get("DirLen");
    let verbose = pl::method(it, &et_, "Options", vec![pl::s("Verbose")])?;
    let out = pl::method(it, &et_, "Options", vec![pl::s("TextOut")])?;
    let old_changed = et_.get("CHANGED");

    if !outfile.truthy() {
        pl::method(it, &et_, "VerboseDir", vec![pl::s("VRD Edit Data"), n(0), dir_len.clone()])?;
    }

    if outfile.truthy() {
        // make a copy for editing in place
        pl::at(1547);
        let buff = substr(it, &pl::sderef(&data_pt)?, &pos, Some(&dir_len))?;
        data_pt = pl::sref(buff);
        dir_info.set("DataPt", data_pt.clone());
        data_pos = pl::add(&data_pos, &pos);
        pos = n(0);
        dir_info.set("DirStart", n(0));
    }
    let mut dir_end = pl::add(&pos, &dir_len);

    // loop through all records in the edit data
    let mut rec_num = n(0);
    let mut rec_len;
    let mut err = false;
    loop {
        if pl::gt(&pl::add(&pos, &n(4)), &dir_end) {
            if pl::num_eq(&pos, &dir_end) {
                break; // all done if we arrived at end
            }
            rec_len = n(0); // just reset record size (will exit loop on test below)
        } else {
            rec_len = et(it, "Get32u", vec![data_pt.clone(), pos.clone()])?;
            // (DR4 has a null terminator)
            if pl::num_eq(&rec_len, &n(0)) && pl::num_eq(&pl::add(&pos, &n(4)), &dir_end) {
                break;
            }
        }
        pos = pl::add(&pos, &n(4)); // move to start of record
        if pl::gt(&pl::add(&pos, &rec_len), &dir_end) {
            pl::method(it, &et_, "Warn", vec![pl::s("Possibly corrupt CanonVRD Edit record")])?;
            err = true;
            break;
        }
        let save_rec_len = rec_len.clone();
        if pl::gt(&verbose, &n(1)) && !outfile.truthy() {
            let fmt = pl::concat(&pl::concat(&pl::concat(&et_.get("INDENT"), &pl::s("CanonVRD Edit record (")), &rec_len), &pl::s(" bytes at offset 0x%x)\n"));
            let line = pl::sprintf_v(&fmt, &[pl::add(&pos, &data_pos)]);
            print_out(it, &out, line)?;
            if rec_num.truthy() {
                pl::method(it, &et_, "VerboseDump", vec![data_pt.clone(), pl::s("Len"), rec_len.clone(), pl::s("Start"), pos.clone(), pl::s("Addr"), pl::add(&pos, &data_pos)])?;
            }
        }

        // our edit information is the 0th record, so don't process the others
        if !rec_num.truthy() {
            // process VRD edit information
            let sub_table_ptr = tag_table_ptr.clone();
            let subdir_info = Value::new_hash();
            subdir_info.set("DataPt", data_pt.clone());
            subdir_info.set("DataPos", data_pos.clone());
            subdir_info.set("DirStart", pos.clone());
            subdir_info.set("DirLen", rec_len.clone());
            subdir_info.set("OutFile", outfile.clone());
            let mut sub_start = n(0);
            // loop through various sections of the VRD edit data
            let mut index = n(0);
            loop {
                let tag_info = sub_table_ptr.get(index.to_bytes());
                if !tag_info.truthy() {
                    break;
                }
                let mut sub_len;
                let max_len = pl::sub(&rec_len, &sub_start);
                if tag_info.get("Size").truthy() {
                    sub_len = tag_info.get("Size");
                } else if !tag_info.get("Size").is_undef() {
                    // get size from int32u at $subStart
                    if !pl::le(&pl::add(&sub_start, &n(4)), &rec_len) {
                        break;
                    }
                    sub_len = et(it, "Get32u", vec![data_pt.clone(), pl::add(&sub_start, &pos)])?;
                    sub_start = pl::add(&sub_start, &n(4)); // skip the length word
                } else {
                    sub_len = max_len.clone();
                }
                if pl::gt(&sub_len, &max_len) {
                    sub_len = max_len;
                }
                if sub_len.truthy() {
                    let sub_table = et(it, "GetTagTable", vec![tag_info.path(&["SubDirectory", "TagTable"])])?;
                    let sub_name = tag_info.get("Name");
                    subdir_info.set("DirStart", pl::add(&sub_start, &pos));
                    subdir_info.set("DirLen", sub_len.clone());
                    subdir_info.set("DirName", sub_name.clone());
                    if outfile.truthy() {
                        // rewrite this section of the VRD edit information
                        if verbose.truthy() {
                            print_out(it, &out, pl::concat(&pl::concat(&pl::s("  Rewriting Canon "), &sub_name), &pl::s("\n")))?;
                        }
                        let new_val = pl::method(it, &et_, "WriteDirectory", vec![subdir_info.clone(), sub_table])?;
                        if new_val.truthy() {
                            let size_diff = pl::sub(&pl::length(&new_val), &sub_len);
                            pl::at(1620);
                            substr_set(&data_pt, &pl::add(&pos, &sub_start), &sub_len, &new_val)?;
                            if size_diff.truthy() {
                                sub_len = pl::length(&new_val);
                                rec_len = pl::add(&rec_len, &size_diff);
                                dir_end = pl::add(&dir_end, &size_diff);
                                dir_len = pl::add(&dir_len, &size_diff);
                            }
                        }
                    } else {
                        let msg = pl::concat(&pl::concat(&et_.get("INDENT"), &sub_name), &pl::s(" (SubDirectory) -->\n"));
                        pl::method(it, &et_, "VPrint", vec![n(0), msg])?;
                        pl::method(
                            it,
                            &et_,
                            "VerboseDump",
                            vec![data_pt.clone(), pl::s("Start"), pl::add(&pos, &sub_start), pl::s("Addr"), pl::add(&pl::add(&data_pos, &pos), &sub_start), pl::s("Len"), sub_len.clone()],
                        )?;
                        // extract tags from this section of the VRD edit information
                        pl::method(it, &et_, "ProcessDirectory", vec![subdir_info.clone(), sub_table])?;
                    }
                }
                // next section starts at the end of this one
                sub_start = pl::add(&sub_start, &sub_len);
                index = pl::add(&index, &n(1));
            }
            if outfile.truthy() && save_rec_len.to_bytes() != rec_len.to_bytes() {
                // update record length if necessary
                et(it, "Set32u", vec![rec_len.clone(), data_pt.clone(), pl::sub(&pos, &n(4))])?;
            }
        }
        rec_num = pl::add(&rec_num, &n(1));
        pos = pl::add(&pos, &rec_len);
    }
    if outfile.truthy() {
        if pl::num_eq(&old_changed, &et_.get("CHANGED")) {
            return pl::ret_undef();
        }
        return ret(substr(it, &data_pt.deref(), &dir_info.get("DirStart"), Some(&dir_len))?);
    }
    ret(if err { n(0) } else { n(1) })
}

/// Process VRD IHL data
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_ihl(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et_, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let data_pos = dir_info.get("DataPos");
    let mut pos = dir_info.get("DirStart");
    let dir_len = dir_info.get("DirLen");
    let dir_end = pl::add(&pos, &dir_len);

    pl::method(it, &et_, "VerboseDir", vec![pl::s("VRD IHL"), n(0), dir_len.clone()])?;

    et(it, "SetByteOrder", vec![pl::s("II")])?; // (make up your mind, Canon!)
    while pl::le(&pl::add(&pos, &n(48)), &dir_end) {
        pl::at(1671);
        let hdr = substr(it, &pl::sderef(&data_pt)?, &pos, Some(&n(48)))?;
        if !re(r"^IHL Created Optional Item Data\0\0", "").is_match(&hdr) {
            pl::method(it, &et_, "Warn", vec![pl::s("Possibly corrupted VRD IHL data")])?;
            break;
        }
        let tag = et(it, "Get32u", vec![data_pt.clone(), pl::add(&pos, &n(36))])?;
        let size = et(it, "Get32u", vec![data_pt.clone(), pl::add(&pos, &n(40))])?; // size of data in IHL record
        let next = et(it, "Get32u", vec![data_pt.clone(), pl::add(&pos, &n(44))])?; // size of complete IHL record
        if pl::gt(&size, &next) || pl::gt(&pl::add(&pl::add(&pos, &n(48)), &next), &dir_end) {
            pl::method(it, &et_, "Warn", vec![pl::sprintf("Bad size for VRD IHL tag 0x%.4x", &[tag])])?;
            break;
        }
        pos = pl::add(&pos, &n(48));
        let v = substr(it, &data_pt.deref(), &pos, Some(&size))?;
        pl::method(it, &et_, "HandleTag", vec![tag_table_ptr.clone(), tag, v, pl::s("DataPt"), data_pt.clone(), pl::s("DataPos"), data_pos.clone(), pl::s("Start"), pos.clone(), pl::s("Size"), size])?;
        pos = pl::add(&pos, &next);
    }
    ret(n(1))
}

/// Process VRD IHL EXIF data
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_ihl_exif(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et_, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    et_.set("DOC_NUM", n(1));
    // the IHL-edited maker notes may look messed up, but the offsets should be OK
    let old_fix = pl::method(it, &et_, "Options", vec![pl::s("FixBase"), n(0)])?;
    let rtn_val = pl::method(it, &et_, "ProcessTIFF", vec![dir_info, tag_table_ptr])?;
    pl::method(it, &et_, "Options", vec![pl::s("FixBase"), old_fix])?;
    et_.delete("DOC_NUM");
    ret(rtn_val)
}

fn wrap_dr4_impl(it: &mut Interp, v: &Value) -> ER<Value> {
    let nn = pl::length(v);
    let old_order = et(it, "GetByteOrder", vec![])?;
    et(it, "SetByteOrder", vec![pl::s("MM")])?;
    let blank_header = val(&crate::runtime::lex_scalar(it, MODULE, "blankHeader"));
    let blank_footer = val(&crate::runtime::lex_scalar(it, MODULE, "blankFooter"));
    let a = et(it, "Set32u", vec![pl::add(&nn, &n(8))])?;
    let b = et(it, "Set32u", vec![nn.clone()])?;
    let joined = [&blank_header.to_bytes()[..], b"\xff\xff\0\xf7", &a.to_bytes()[..], &b.to_bytes()[..], &v.to_bytes()[..], b"\0\0\0\0", &blank_footer.to_bytes()[..]].concat();
    let c = cell(Value::bytes(joined));
    // update the new VRD length in the header/footer
    et(it, "Set32u", vec![pl::add(&nn, &n(16)), rf(&c), n(0x18)])?; // (extra 16 bytes for the edit record wrapper)
    let l = pl::sub(&pl::length(&val(&c)), &n(0x2c));
    et(it, "Set32u", vec![pl::add(&nn, &n(16)), rf(&c), l])?;
    et(it, "SetByteOrder", vec![old_order])?;
    Ok(val(&c))
}

/// Wrap DR4 data with the VRD header/footer and edit record
/// Inputs: 0) DR4 record
/// Returns: VRD[Edit[DR4]] data
pub fn wrap_dr4(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(wrap_dr4_impl(it, &arg(&a, 0))?)
}

/// Read/Write DPP version 4 edit data or DR4 file
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns:
///   Reading from memory (not RAF and not IsWriting): 1 on success
///   Editing from memory (not RAF and IsWriting): modified edit data, or undef if nothing changed
///   Reading file (RAF and not OutFile): 1 if a valid DR4 file, 0 if not
///   Writing file (RAF and OutFile): 1 if valid DR4 file, 0 if not, -1 on write error
/// (serves me right for not having a consistent interface for the various modes of operation)
pub fn process_dr4(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et_, dir_info, mut tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if !et_.truthy() {
        return ret(n(1)); // allow dummy access
    }
    let mut data_pt = dir_info.get("DataPt");
    let raf = dir_info.get("RAF");
    let outfile = dir_info.get("OutFile");
    let is_writing = if outfile.truthy() { outfile.clone() } else { dir_info.get("IsWriting") };
    let dp = dir_info.get("DataPos");
    let data_pos = if dp.truthy() { dp } else { n(0) };
    let _verbose = pl::method(it, &et_, "Options", vec![pl::s("Verbose")])?;
    let _unknown = pl::method(it, &et_, "Options", vec![pl::s("Unknown")])?;
    let (pos, dir_len);
    let mut num_entries = Value::Undef;
    let mut err = false;
    let mut new_tags = Value::Undef;

    // write CanonDR4 as a block if specified
    if is_writing.truthy() {
        let nv_hash = cell(Value::Undef);
        let new_val = pl::method(it, &et_, "GetNewValue", vec![pl::s("CanonDR4"), rf(&nv_hash)])?;
        if new_val.truthy() {
            pl::method(it, &et_, "VPrint", vec![n(0), pl::s("  Writing CanonDR4 as a block\n")])?;
            et_.set("DidCanonVRD", n(1)); // set flag so we don't add this twice
            et_.set("CHANGED", pl::add(&et_.get("CHANGED"), &n(1)));
            if outfile.truthy() {
                if !et(it, "Write", vec![dir_info.get("OutFile"), new_val])?.truthy() {
                    return ret(n(-1));
                }
                return ret(n(1));
            } else {
                return ret(new_val);
            }
        } else if !data_pt.truthy() && (val(&nv_hash).truthy() || et_.path(&["DEL_GROUP", "CanonVRD"]).truthy()) {
            pl::method(it, &et_, "Error", vec![pl::s("Can't delete all CanonDR4 information from a DR4 file")])?;
            return ret(n(1));
        }
    }
    if data_pt.truthy() {
        let ds = dir_info.get("DirStart");
        pos = if ds.truthy() { ds } else { n(0) };
        let dl = dir_info.get("DirLen");
        dir_len = if dl.truthy() {
            dl
        } else {
            pl::at(1773);
            pl::sub(&pl::length(&pl::sderef(&data_pt)?), &pos)
        };
    } else {
        // load DR4 file into memory
        let buff = cell(Value::Undef);
        pl::at(1777);
        check_invocant(&raf, "Read")?;
        let ok = pl::num_eq(&raf_read_cell(it, &raf, &buff, n(8))?, &n(8)) && re(r"^IIII[\x04|\x05]\0\x04\0", "").is_match(&val(&buff));
        if !ok {
            return ret(n(0));
        }
        pl::method(it, &et_, "SetFileType", vec![])?;
        if !seek(it, &raf, n(0), 2)? {
            return ret(n(1)); // (return $err = 1)
        }
        dir_len = pl::method(it, &raf, "Tell", vec![])?;
        if !seek(it, &raf, n(0), 0)? {
            return ret(n(1)); // (return $err = 1)
        }
        if !pl::num_eq(&raf_read_cell(it, &raf, &buff, dir_len.clone())?, &dir_len) {
            err = true;
        }
        if err {
            pl::method(it, &et_, "Warn", vec![pl::s("Error reading DR4 file")])?;
            return ret(n(1));
        }
        tag_table_ptr = et(it, "GetTagTable", vec![pl::s("Image::ExifTool::CanonVRD::DR4")])?;
        data_pt = rf(&buff);
        pos = n(0);
    }
    let dir_end = pl::add(&pos, &dir_len);

    if (et_.get("TAGS_FROM_FILE").truthy() && !et_.path(&["EXCL_TAG_LOOKUP", "canondr4"]).truthy()) || et_.path(&["REQ_TAG_LOOKUP", "canondr4"]).truthy() {
        // extract CanonDR4 block if copying tags, or if requested
        pl::at(1794);
        let blk = substr(it, &pl::sderef(&data_pt)?, &pos, Some(&dir_len))?;
        pl::method(it, &et_, "FoundTag", vec![pl::s("CanonDR4"), blk])?;
    }

    // version 4 header is 32 bytes (int32u[8])
    if pl::lt(&dir_len, &n(32)) {
        err = true;
    } else {
        pl::at(1801);
        let bo = substr(it, &pl::sderef(&data_pt)?, &pos, Some(&n(2)))?;
        if !et(it, "SetByteOrder", vec![bo])?.truthy() {
            err = true;
        }
        // process the DR4 header
        let hdr_info = Value::new_hash();
        hdr_info.set("DataPt", data_pt.clone());
        hdr_info.set("DirStart", pos.clone());
        hdr_info.set("DirLen", n(32));
        hdr_info.set("DirName", pl::s("DR4Header"));
        let hdr_table = et(it, "GetTagTable", vec![pl::s("Image::ExifTool::CanonVRD::DR4Header")])?;
        if outfile.truthy() {
            let hdr = pl::method(it, &et_, "WriteDirectory", vec![hdr_info, hdr_table])?;
            if hdr.truthy() && pl::num_eq(&pl::length(&hdr), &n(32)) {
                pl::at(1812);
                substr_set(&data_pt, &pos, &n(32), &hdr)?;
            }
        } else {
            pl::method(it, &et_, "VerboseDir", vec![pl::s("DR4Header"), Value::Undef, n(32)])?;
            pl::method(it, &et_, "ProcessDirectory", vec![hdr_info, hdr_table])?;
        }
        // number of entries in the DR4 directory
        num_entries = et(it, "Get32u", vec![data_pt.clone(), pl::add(&pos, &n(28))])?;
        if pl::lt(&dir_len, &pl::add(&n(36), &pl::mul(&n(28), &num_entries))) {
            err = true;
        }
    }
    if err {
        pl::method(it, &et_, "Warn", vec![pl::s("Invalid DR4 directory")])?;
        return if outfile.truthy() { pl::ret_undef() } else { ret(n(0)) };
    }

    if outfile.truthy() {
        new_tags = pl::method(it, &et_, "GetNewTagInfoHash", vec![tag_table_ptr.clone()])?;
    } else {
        pl::method(it, &et_, "VerboseDir", vec![pl::s("DR4"), num_entries.clone(), dir_len.clone()])?;
    }

    let vrd_format = crate::runtime::lex_hash(it, MODULE, "vrdFormat");
    let mut index = n(0);
    'idx: while pl::lt(&index, &num_entries) {
        'entry: {
            let mut v = Value::Undef;
            let mut flg: Vec<Value> = Vec::new();
            let entry = pl::add(&pl::add(&pos, &n(36)), &pl::mul(&n(28), &index));
            if pl::gt(&pl::add(&entry, &n(28)), &dir_end) {
                break 'idx;
            }
            let tag = et(it, "Get32u", vec![data_pt.clone(), entry.clone()])?;
            let fmt = et(it, "Get32u", vec![data_pt.clone(), pl::add(&entry, &n(4))])?;
            flg.push(et(it, "Get32u", vec![data_pt.clone(), pl::add(&entry, &n(8))])?);
            flg.push(et(it, "Get32u", vec![data_pt.clone(), pl::add(&entry, &n(12))])?);
            flg.push(et(it, "Get32u", vec![data_pt.clone(), pl::add(&entry, &n(16))])?);
            let mut off = pl::add(&et(it, "Get32u", vec![data_pt.clone(), pl::add(&entry, &n(20))])?, &pos);
            let mut len = et(it, "Get32u", vec![data_pt.clone(), pl::add(&entry, &n(24))])?;
            if pl::ge(&pl::add(&off, &len), &dir_end) {
                break 'entry;
            }
            let mut format = vrd_format.get(fmt.to_bytes());
            if !format.truthy() {
                let raw = substr(it, &data_pt.deref(), &off, Some(&len))?;
                v = pl::unpack1("H*", &raw)?;
                format = pl::s("undef");
            } else if pl::str_eq(&format, "double") && pl::num_eq(&len, &n(8)) {
                // avoid teeny weeny values
                v = et(it, "ReadValue", vec![data_pt.clone(), off.clone(), format.clone(), Value::Undef, len.clone()])?;
                if pl::lt(&pl::abs(&v), &Value::Num(1e-100)) {
                    v = n(0);
                }
            }
            if outfile.truthy() {
                // write (binary data) subdirectory if it exists
                let mut tag_info = pl::method(it, &et_, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
                if tag_info.truthy() && tag_info.get("SubDirectory").truthy() {
                    let subdir_info = Value::new_hash();
                    subdir_info.set("DataPt", data_pt.clone());
                    subdir_info.set("DirStart", off.clone());
                    subdir_info.set("DirLen", len.clone());
                    subdir_info.set("DirName", tag_info.get("Name"));
                    let sub_table_ptr = et(it, "GetTagTable", vec![tag_info.path(&["SubDirectory", "TagTable"])])?;
                    let save_changed = et_.get("CHANGED");
                    let dat = pl::method(it, &et_, "WriteDirectory", vec![subdir_info, sub_table_ptr])?;
                    if !dat.is_undef() && pl::num_eq(&pl::length(&dat), &len) {
                        pl::at(1865);
                        substr_set(&data_pt, &off, &len, &dat)?;
                    } else {
                        et_.set("CHANGED", save_changed); // didn't change anything after all
                    }
                } else {
                    // loop through main tag and flags (don't yet consider flag 2)
                    let mut i = n(-1);
                    while pl::lt(&i, &n(2)) {
                        'flag: {
                            let key = if pl::ge(&i, &n(0)) { pl::sprintf("0x%x.%d", &[tag.clone(), i.clone()]) } else { tag.clone() };
                            tag_info = new_tags.get(key.to_bytes());
                            if !tag_info.truthy() {
                                break 'flag;
                            }
                            if pl::ge(&i, &n(0)) {
                                off = pl::add(&pl::add(&entry, &n(8)), &pl::mul(&n(4), &i));
                                format = pl::s("int32u");
                                len = n(4);
                                v = Value::Undef;
                            }
                            if v.is_undef() {
                                v = et(it, "ReadValue", vec![data_pt.clone(), off.clone(), format.clone(), Value::Undef, len.clone()])?;
                            }
                            let nv_hash = cell(Value::Undef);
                            let new_val = pl::method(it, &et_, "GetNewValue", vec![tag_info.clone(), rf(&nv_hash)])?;
                            if pl::method(it, &et_, "IsOverwriting", vec![val(&nv_hash), v.clone()])?.truthy() && !new_val.is_undef() {
                                let fs = et(it, "FormatSize", vec![format.clone()])?;
                                pl::at(1884);
                                let count = pl::int(&pl::div(&len, &fs)?);
                                let rtn_val = et(it, "WriteValue", vec![new_val.clone(), format.clone(), count, data_pt.clone(), off.clone()])?;
                                if !rtn_val.is_undef() {
                                    let name = tag_info.get("Name");
                                    pl::method(it, &et_, "VerboseValue", vec![pl::concat(&pl::s("- CanonVRD:"), &name), v.clone()])?;
                                    pl::method(it, &et_, "VerboseValue", vec![pl::concat(&pl::s("+ CanonVRD:"), &name), new_val])?;
                                    et_.set("CHANGED", pl::add(&et_.get("CHANGED"), &n(1)));
                                }
                            }
                        }
                        i = pl::add(&i, &n(1));
                    }
                }
                break 'entry;
            }
            let extra = pl::concat(&pl::concat(&pl::concat(&pl::s(", fmt="), &fmt), &pl::s(" flags=")), &pl::join(",", &flg));
            pl::method(
                it,
                &et_,
                "HandleTag",
                vec![
                    tag_table_ptr.clone(),
                    tag.clone(),
                    v,
                    pl::s("DataPt"),
                    data_pt.clone(),
                    pl::s("DataPos"),
                    data_pos.clone(),
                    pl::s("Start"),
                    off,
                    pl::s("Size"),
                    len,
                    pl::s("Index"),
                    index.clone(),
                    pl::s("Format"),
                    format,
                    // $flg[0] is on/off flag
                    // $flg[1] "is default" flag?
                    // $flg[2] changed to 0 when some unsharp mask settings were changed
                    pl::s("Extra"),
                    extra,
                ],
            )?;
            for i in 0..=2 {
                let flag_id = pl::sprintf("0x%x.%d", &[tag.clone(), n(i)]);
                if tag_table_ptr.get(flag_id.to_bytes()).truthy() {
                    pl::method(it, &et_, "HandleTag", vec![tag_table_ptr.clone(), flag_id, flg[i as usize].clone()])?;
                }
            }
        }
        index = pl::add(&index, &n(1));
    }
    finish_dr4(it, &is_writing, &raf, &data_pt, &pos, &dir_len, &outfile)
}

/// The end of ProcessDR4, after the entry loop.
fn finish_dr4(it: &mut Interp, is_writing: &Value, raf: &Value, data_pt: &Value, pos: &Value, dir_len: &Value, outfile: &Value) -> ER<Vec<Value>> {
    if !is_writing.truthy() {
        return ret(n(1));
    }
    if !raf.truthy() {
        return ret(substr(it, &data_pt.deref(), pos, Some(dir_len))?);
    }
    let blk = substr(it, &data_pt.deref(), pos, Some(dir_len))?;
    if et(it, "Write", vec![outfile.clone(), blk])?.truthy() {
        return ret(n(1));
    }
    ret(n(-1))
}

/// Read/write Canon VRD file
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 if this was a Canon VRD file, 0 otherwise, -1 on write error
pub fn process_vrd(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et_, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let buff = cell(Value::Undef);
    let num = raf_read_cell(it, &raf, &buff, n(0x1c))?;

    // initialize write directories if necessary
    if dir_info.get("OutFile").truthy() {
        let vrd_map = crate::runtime::lex_hash(it, MODULE, "vrdMap");
        pl::method(it, &et_, "InitWriteDirs", vec![vrd_map, pl::s("XMP")])?;
    }

    if !num.truthy() && dir_info.get("OutFile").truthy() {
        // create new VRD file from scratch
        let new_val = pl::method(it, &et_, "GetNewValue", vec![pl::s("CanonVRD")])?;
        if new_val.truthy() {
            pl::method(it, &et_, "VPrint", vec![n(0), pl::s("  Writing CanonVRD as a block\n")])?;
            if !et(it, "Write", vec![dir_info.get("OutFile"), new_val])?.truthy() {
                return ret(n(-1));
            }
            et_.set("DidCanonVRD", n(1));
            et_.set("CHANGED", pl::add(&et_.get("CHANGED"), &n(1)));
        } else {
            // allow VRD to be created from individual tags
            if et_.path(&["ADD_DIRS", "CanonVRD"]).truthy() {
                let new_val = cell(pl::s(""));
                let d = Value::new_hash();
                d.set("OutFile", rf(&new_val));
                let r = process_canon_vrd(it, vec![et_.clone(), d], Want::Scalar)?;
                if pl::gt(&arg(&r, 0), &n(0)) {
                    if !et(it, "Write", vec![dir_info.get("OutFile"), val(&new_val)])?.truthy() {
                        return ret(n(-1));
                    }
                    et_.set("CHANGED", pl::add(&et_.get("CHANGED"), &n(1)));
                    return ret(n(1));
                }
            }
            pl::method(it, &et_, "Error", vec![pl::s("No CanonVRD information to write")])?;
        }
    } else {
        if !pl::num_eq(&num, &n(0x1c)) {
            return ret(n(0));
        }
        if !re(r"^CANON OPTIONAL DATA\0", "").is_match(&val(&buff)) {
            return ret(n(0));
        }
        pl::method(it, &et_, "SetFileType", vec![])?;
        dir_info.set("DirName", pl::s("CanonVRD")); // set directory name for verbose output
        let result = arg(&process_canon_vrd(it, vec![et_.clone(), dir_info.clone()], Want::Scalar)?, 0);
        if pl::lt(&result, &n(0)) {
            return ret(result);
        }
        if !result.truthy() {
            pl::method(it, &et_, "Warn", vec![pl::s("Format error in VRD file")])?;
        }
    }
    ret(n(1))
}

/// Write VRD data record as a block
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: VRD data block (may be empty if no VRD data)
/// Notes: Increments ExifTool CHANGED flag if changed
pub fn write_canon_vrd(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let et_ = arg(&a, 0);
    if !et_.truthy() {
        return ret(n(1)); // allow dummy access
    }
    let extra = pl::pkg_hash(it, "Image::ExifTool::Extra").get("CanonVRD");
    let nv_hash = pl::method(it, &et_, "GetNewValueHash", vec![extra])?;
    let mut v = pl::method(it, &et_, "GetNewValue", vec![nv_hash.clone()])?;
    if v.is_undef() {
        v = pl::s("");
    }
    if !pl::method(it, &et_, "IsOverwriting", vec![nv_hash, v.clone()])?.truthy() {
        return pl::ret_undef();
    }
    et_.set("CHANGED", pl::add(&et_.get("CHANGED"), &n(1)));
    ret(v)
}

/// Write DR4-type CanonVRD edit record
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: VRD data block (may be empty if deleted, of undef on error)
pub fn write_canon_dr4(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et_, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if !et_.truthy() {
        return ret(n(1)); // allow dummy access
    }
    let extra = pl::pkg_hash(it, "Image::ExifTool::Extra").get("CanonDR4");
    let nv_hash = pl::method(it, &et_, "GetNewValueHash", vec![extra])?;
    let v = pl::method(it, &et_, "GetNewValue", vec![nv_hash.clone()])?;
    if !v.is_undef() {
        if !pl::method(it, &et_, "IsOverwriting", vec![nv_hash, v.clone()])?.truthy() {
            return pl::ret_undef();
        }
        pl::method(it, &et_, "VPrint", vec![n(0), pl::s("  Writing CanonDR4 as a block\n")])?;
        et_.set("CHANGED", pl::add(&et_.get("CHANGED"), &n(1)));
        return ret(wrap_dr4_impl(it, &v)?);
    }
    let buff = cell(pl::s(""));
    dir_info.set("OutFile", rf(&buff));
    let r = arg(&process_canon_vrd(it, vec![et_, dir_info, tag_table_ptr], Want::Scalar)?, 0);
    if pl::gt(&r, &n(0)) {
        return ret(val(&buff));
    }
    pl::ret_undef()
}

/// Read/write CanonVRD information (from VRD file or VRD trailer)
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 not valid VRD, or -1 error writing
/// - updates DataPos to point to start of CanonVRD information
/// - updates DirLen to existing trailer length
pub fn process_canon_vrd(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et_, dir_info) = (arg(&a, 0), arg(&a, 1));
    let mut raf = dir_info.get("RAF");
    let o = dir_info.get("Offset");
    let offset = if o.truthy() { o } else { n(0) };
    let outfile = dir_info.get("OutFile");
    let mut data_pt = dir_info.get("DataPt");
    let verbose = pl::method(it, &et_, "Options", vec![pl::s("Verbose")])?;
    let out = pl::method(it, &et_, "Options", vec![pl::s("TextOut")])?;
    let buff = cell(Value::Undef);
    let mut created = false;
    let mut err = false;
    let mut block_len;
    let mut block_type = Value::Undef;
    let did_dir = Value::new_hash();
    let mut from_file = false;
    //
    // The CanonVRD trailer has a 0x1c-byte header and a 0x40-byte footer,
    // each beginning with "CANON OPTIONAL DATA\0" and containing an int32u
    // giving the size of the contained data (at byte 0x18 and 0x14 respectively)
    //
    if raf.truthy() {
        from_file = true;
    } else {
        if !data_pt.truthy() {
            if !outfile.truthy() {
                return ret(n(1));
            }
            // create blank VRD data from scratch
            let blank_header = val(&crate::runtime::lex_scalar(it, MODULE, "blankHeader"));
            let blank_footer = val(&crate::runtime::lex_scalar(it, MODULE, "blankFooter"));
            let blank = pl::concat(&blank_header, &blank_footer);
            data_pt = pl::sref(blank);
            if verbose.truthy() {
                print_out(it, &out, pl::s("  Creating CanonVRD trailer\n"))?;
            }
            created = true;
        }
        raf = pl::method(it, &pl::s("File::RandomAccess"), "new", vec![data_pt.clone()])?;
    }
    // read and validate the footer
    if !seek(it, &raf, pl::sub(&n(-0x40), &offset), 2)? {
        return ret(n(0));
    }
    if !pl::num_eq(&raf_read_cell(it, &raf, &buff, n(0x40))?, &n(0x40)) {
        return ret(n(0));
    }
    let Some(m) = re(r"^CANON OPTIONAL DATA\0(.{4})", "s").m(&val(&buff)) else { return ret(n(0)) };
    let mut dir_len = pl::add(&pl::unpack1("N", &cap(&m, 1))?, &n(0x5c)); // size including header+footer

    // read and validate the header
    let ok = pl::lt(&dir_len, &Value::Num(2147483648.0))
        && seek(it, &raf, pl::neg(&dir_len), 1)?
        && pl::num_eq(&raf_read_cell(it, &raf, &buff, n(0x1c))?, &n(0x1c))
        && re(r"^CANON OPTIONAL DATA\0", "").is_match(&val(&buff))
        && seek(it, &raf, n(-0x1c), 1)?;
    if !ok {
        pl::method(it, &et_, "Warn", vec![pl::s("Bad CanonVRD trailer")])?;
        return ret(n(0));
    }
    // set variables returned in dirInfo hash
    let tell = pl::method(it, &raf, "Tell", vec![])?;
    dir_info.set("DataPos", tell);
    dir_info.set("DirLen", dir_len.clone());

    if outfile.truthy() && pl::ref_of(&outfile) == "SCALAR" && pl::length(&outfile.deref()).to_i64() == 0 {
        // write directly to outfile to avoid duplicating data in memory
        if !from_file {
            pl::set_deref(&outfile, data_pt.deref())?;
        }
        // TRICKY! -- copy to outfile memory buffer and edit in place
        // (so we must disable all Write() calls for this case)
        data_pt = outfile.clone();
    }
    if from_file || dir_info.get("DirStart").truthy() {
        if !data_pt.truthy() {
            data_pt = rf(&buff);
        }
        // read VRD data into memory if necessary
        if !pl::num_eq(&read_into(it, &raf, &data_pt, &dir_len)?, &dir_len) {
            if outfile.truthy() && str_eqv(&outfile, &data_pt) {
                pl::set_deref(&data_pt, pl::s(""))?;
            }
            pl::method(it, &et_, "Warn", vec![pl::s("Error reading CanonVRD data")])?;
            return ret(n(0));
        }
    }
    let mut vrd_type = pl::s("VRD");

    if outfile.truthy() {
        if verbose.truthy() && !created {
            print_out(it, &out, pl::s("  Rewriting CanonVRD trailer\n"))?;
        }
        // exit quickly if writing and no CanonVRD tags are being edited
        if !et_.get("EDIT_DIRS").exists("CanonVRD") {
            if verbose.truthy() {
                print_out(it, &out, pl::concat(&et_.get("INDENT"), &pl::s("  [nothing changed in CanonVRD]\n")))?;
            }
            if str_eqv(&outfile, &data_pt) {
                return ret(n(1));
            }
            return ret(if et(it, "Write", vec![outfile.clone(), data_pt.deref()])?.truthy() { n(1) } else { n(-1) });
        }
        // delete CanonVRD information if specified
        let mut do_del = et_.path(&["DEL_GROUP", "CanonVRD"]).truthy();
        if !do_del {
            if et_.path(&["DEL_GROUP", "Trailer"]).truthy() && !et_.get("FILE_TYPE").eq_str("VRD") {
                do_del = true;
            }
            if !do_del {
                // also delete if writing as a block (will get added back again later)
                let extra = pl::pkg_hash(it, "Image::ExifTool::Extra");
                let new_value = et_.get("NEW_VALUE");
                let v4 = re(r"^.{28}\xff\xff\0\xf7", "s");
                if new_value.get(extra.get("CanonVRD").to_bytes()).truthy() {
                    // delete if this isn't version 4
                    if !v4.is_match(&data_pt.deref()) {
                        do_del = true;
                    }
                }
                if new_value.get(extra.get("CanonDR4").to_bytes()).truthy() && !do_del {
                    // delete if this is version 4
                    if v4.is_match(&data_pt.deref()) {
                        do_del = true;
                    }
                }
            }
        }
        if do_del {
            if et_.get("FILE_TYPE").eq_str("VRD") {
                let new_val = pl::method(it, &et_, "GetNewValue", vec![pl::s("CanonVRD")])?;
                if new_val.truthy() {
                    if verbose.truthy() {
                        print_out(it, &out, pl::s("  Writing CanonVRD as a block\n"))?;
                    }
                    if str_eqv(&outfile, &data_pt) {
                        pl::set_deref(&outfile, new_val)?;
                    } else if !et(it, "Write", vec![outfile.clone(), new_val])?.truthy() {
                        return ret(n(-1));
                    }
                    et_.set("DidCanonVRD", n(1));
                    et_.set("CHANGED", pl::add(&et_.get("CHANGED"), &n(1)));
                } else {
                    pl::method(it, &et_, "Error", vec![pl::s("Can't delete all CanonVRD information from a VRD file")])?;
                }
            } else {
                if verbose.truthy() {
                    print_out(it, &out, pl::s("  Deleting CanonVRD trailer\n"))?;
                }
                if str_eqv(&outfile, &data_pt) {
                    pl::set_deref(&outfile, pl::s(""))?;
                }
                et_.set("CHANGED", pl::add(&et_.get("CHANGED"), &n(1)));
            }
            return ret(n(1));
        }
        // write now and return if CanonVRD was set as a block
        let mut v = pl::method(it, &et_, "GetNewValue", vec![pl::s("CanonVRD")])?;
        if !v.truthy() {
            v = pl::method(it, &et_, "GetNewValue", vec![pl::s("CanonDR4")])?;
            if v.truthy() {
                vrd_type = pl::s("DR4");
            }
        }
        if v.truthy() {
            if verbose.truthy() {
                print_out(it, &out, pl::concat(&pl::concat(&pl::s("  Writing Canon"), &vrd_type), &pl::s(" as a block\n")))?;
            }
            // must wrap DR4 data with the VRD header/footer and edit record
            if vrd_type.eq_str("DR4") {
                v = wrap_dr4_impl(it, &v)?;
            }
            if str_eqv(&outfile, &data_pt) {
                pl::set_deref(&outfile, v)?;
            } else if !et(it, "Write", vec![outfile.clone(), v])?.truthy() {
                return ret(n(-1));
            }
            et_.set("DidCanonVRD", n(1));
            et_.set("CHANGED", pl::add(&et_.get("CHANGED"), &n(1)));
            return ret(n(1));
        }
    } else if (verbose.truthy() || et_.get("HTML_DUMP").truthy()) && dir_info.get("RAF").truthy() {
        pl::method(it, &et_, "DumpTrailer", vec![dir_info.clone()])?;
    }

    let tag_table_ptr = et(it, "GetTagTable", vec![pl::s("Image::ExifTool::CanonVRD::Main")])?;

    // validate VRD trailer and get position and length of edit record
    et(it, "SetByteOrder", vec![pl::s("MM")])?; // VRD header/footer is big-endian
    let mut pos = n(0x1c); // start at end of header

    // loop through the VRD blocks
    loop {
        let end = pl::sub(&dir_len, &n(0x40)); // end of last VRD block (and start of footer)
        if pl::gt(&pl::add(&pos, &n(8)), &end) {
            if pl::num_eq(&pos, &end) {
                break;
            }
            block_len = end.clone(); // mark as invalid
        } else {
            block_type = et(it, "Get32u", vec![data_pt.clone(), pos.clone()])?;
            block_len = et(it, "Get32u", vec![data_pt.clone(), pl::add(&pos, &n(4))])?;
        }
        if pl::num_eq(&block_type, &n(0xffff00f7)) {
            vrd_type = pl::s("DR4");
        }
        pos = pl::add(&pos, &n(8)); // move to start of block
        if pl::gt(&pl::add(&pos, &block_len), &end) {
            pl::method(it, &et_, "Warn", vec![pl::s("Possibly corrupt CanonVRD block")])?;
            break;
        }
        if pl::gt(&verbose, &n(1)) && !outfile.truthy() {
            let fmt = pl::concat(&pl::concat(&pl::s("  CanonVRD block 0x%.8x ("), &block_len), &pl::s(" bytes at offset 0x%x)\n"));
            let line = pl::sprintf_v(&fmt, &[block_type.clone(), pl::add(&pos, &dir_info.get("DataPos"))]);
            print_out(it, &out, line)?;
            pl::method(it, &et_, "VerboseDump", vec![data_pt.clone(), pl::s("Len"), block_len.clone(), pl::s("Start"), pos.clone(), pl::s("Addr"), pl::add(&pos, &dir_info.get("DataPos"))])?;
        }
        let mut tag_info = tag_table_ptr.get(block_type.to_bytes());
        if !tag_info.truthy() {
            if !pl::method(it, &et_, "Options", vec![pl::s("Unknown")])?.truthy() {
                pos = pl::add(&pos, &block_len); // step to next block
                continue;
            }
            let name = pl::sprintf("CanonVRD_0x%.8x", &[block_type.clone()]);
            let mut desc = name.clone();
            pl::tr(&mut desc, "_", " ", "");
            tag_info = Value::new_hash();
            tag_info.set("Name", name);
            tag_info.set("Description", desc);
            tag_info.set("Binary", n(1));
            et(it, "AddTagToTable", vec![tag_table_ptr.clone(), block_type.clone(), tag_info.clone()])?;
        }
        if tag_info.get("SubDirectory").truthy() {
            let sub_table_ptr = et(it, "GetTagTable", vec![tag_info.path(&["SubDirectory", "TagTable"])])?;
            let subdir_info = Value::new_hash();
            subdir_info.set("DataPt", data_pt.clone());
            subdir_info.set("DataLen", pl::length(&data_pt.deref()));
            subdir_info.set("DataPos", dir_info.get("DataPos"));
            subdir_info.set("DirStart", pos.clone());
            subdir_info.set("DirLen", block_len.clone());
            subdir_info.set("DirName", tag_info.get("Name"));
            subdir_info.set("Parent", pl::s("CanonVRD"));
            subdir_info.set("OutFile", outfile.clone());
            if outfile.truthy() {
                // set flag indicating we did this directory
                did_dir.set(tag_info.get("Name").to_bytes(), n(1));
                let mut dat;
                if et_.get("NEW_VALUE").get(tag_info.to_bytes()).truthy() {
                    // write as a block
                    let msg = pl::concat(&pl::concat(&pl::s("Writing "), &tag_info.get("Name")), &pl::s(" as a block\n"));
                    pl::method(it, &et_, "VPrint", vec![n(0), msg])?;
                    dat = pl::method(it, &et_, "GetNewValue", vec![tag_info.clone()])?;
                    if dat.is_undef() {
                        dat = pl::s("");
                    }
                    et_.set("CHANGED", pl::add(&et_.get("CHANGED"), &n(1)));
                } else {
                    dat = pl::method(it, &et_, "WriteDirectory", vec![subdir_info, sub_table_ptr])?;
                }
                // update data with new directory
                if !dat.is_undef() {
                    if pl::length(&dat).to_i64() != 0 || !re(r"^(CRW|VRD)$", "").is_match(&et_.get("FILE_TYPE")) {
                        // replace with new block (updating the block length word)
                        let new = pl::concat(&et(it, "Set32u", vec![pl::length(&dat)])?, &dat);
                        pl::at(2217);
                        substr_set(&data_pt, &pl::sub(&pos, &n(4)), &pl::add(&block_len, &n(4)), &new)?;
                    } else {
                        // remove block totally (CRW/VRD files only)
                        pl::at(2220);
                        substr_set(&data_pt, &pl::sub(&pos, &n(8)), &pl::add(&block_len, &n(8)), &pl::s(""))?;
                    }
                    // make necessary adjustments if block changes length
                    let diff = pl::sub(&pl::length(&data_pt.deref()), &dir_len);
                    if !pl::num_eq(&diff, &n(0)) {
                        pos = pl::add(&pos, &diff);
                        dir_len = pl::add(&dir_len, &diff);
                        // update the new VRD length in the header/footer
                        et(it, "Set32u", vec![pl::sub(&dir_len, &n(0x5c)), data_pt.clone(), n(0x18)])?;
                        et(it, "Set32u", vec![pl::sub(&dir_len, &n(0x5c)), data_pt.clone(), pl::sub(&dir_len, &n(0x2c))])?;
                    }
                }
            } else {
                // extract as a block if requested
                pl::method(it, &et_, "ProcessDirectory", vec![subdir_info, sub_table_ptr])?;
            }
        } else {
            let v = substr(it, &data_pt.deref(), &pos, Some(&block_len))?;
            pl::method(it, &et_, "HandleTag", vec![tag_table_ptr.clone(), block_type.clone(), v])?;
        }
        pos = pl::add(&pos, &block_len); // step to next block
    }
    if outfile.truthy() {
        // create XMP block if necessary (CRW/VRD files only)
        if et_.path(&["ADD_DIRS", "CanonVRD"]).truthy() && !did_dir.get("XMP").truthy() {
            let sub_table_ptr = et(it, "GetTagTable", vec![pl::s("Image::ExifTool::XMP::Main")])?;
            let d = Value::new_hash();
            d.set("Parent", pl::s("CanonVRD"));
            let dat = pl::method(it, &et_, "WriteDirectory", vec![d, sub_table_ptr])?;
            if dat.truthy() {
                let _block_len = pl::length(&dat);
                let a = et(it, "Set32u", vec![n(0xffff00f6)])?;
                let b = et(it, "Set32u", vec![pl::length(&dat)])?;
                let new = pl::concat(&pl::concat(&a, &b), &dat);
                pl::at(2247);
                substr_set(&data_pt, &n(-0x40), &n(0), &new)?;
                dir_len = pl::length(&data_pt.deref());
                // update the new VRD length in the header/footer
                et(it, "Set32u", vec![pl::sub(&dir_len, &n(0x5c)), data_pt.clone(), n(0x18)])?;
                et(it, "Set32u", vec![pl::sub(&dir_len, &n(0x5c)), data_pt.clone(), pl::sub(&dir_len, &n(0x2c))])?;
            }
        }
        // write CanonVRD trailer unless it is empty
        if pl::length(&data_pt.deref()).to_i64() != 0 {
            if !str_eqv(&outfile, &data_pt) && !et(it, "Write", vec![outfile.clone(), data_pt.deref()])?.truthy() {
                err = true;
            }
        } else if verbose.truthy() {
            print_out(it, &out, pl::s("  Deleting CanonVRD trailer\n"))?;
        }
    } else if vrd_type.eq_str("VRD") && ((et_.get("TAGS_FROM_FILE").truthy() && !et_.path(&["EXCL_TAG_LOOKUP", "canonvrd"]).truthy()) || et_.path(&["REQ_TAG_LOOKUP", "canonvrd"]).truthy()) {
        // extract CanonVRD block if copying tags, or if requested (and not DR4 info)
        pl::method(it, &et_, "FoundTag", vec![pl::s("CanonVRD"), val(&buff)])?;
    }
    put(&buff, Value::Undef);
    ret(if err { n(-1) } else { n(1) })
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::CanonVRD::SortDR4", sort_dr4);
    it.define_native("Image::ExifTool::CanonVRD::ToneCurvePrint", tone_curve_print);
    it.define_native("Image::ExifTool::CanonVRD::ToneCurvePrintInv", tone_curve_print_inv);
    it.define_native("Image::ExifTool::CanonVRD::ProcessEditData", process_edit_data);
    it.define_native("Image::ExifTool::CanonVRD::ProcessIHL", process_ihl);
    it.define_native("Image::ExifTool::CanonVRD::ProcessIHLExif", process_ihl_exif);
    it.define_native("Image::ExifTool::CanonVRD::WrapDR4", wrap_dr4);
    it.define_native("Image::ExifTool::CanonVRD::ProcessDR4", process_dr4);
    it.define_native("Image::ExifTool::CanonVRD::ProcessVRD", process_vrd);
    it.define_native("Image::ExifTool::CanonVRD::WriteCanonVRD", write_canon_vrd);
    it.define_native("Image::ExifTool::CanonVRD::WriteCanonDR4", write_canon_dr4);
    it.define_native("Image::ExifTool::CanonVRD::ProcessCanonVRD", process_canon_vrd);
}
