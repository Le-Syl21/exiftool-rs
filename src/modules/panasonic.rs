//! Image::ExifTool::Panasonic (Image/ExifTool/Panasonic.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Panasonic/Leica maker notes tags

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, perl_eval, raf_read_cell, substr};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, n, re, ret};

const MODULE: &str = "Image::ExifTool::Panasonic";

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

fn val(c: &Cell) -> Value {
    c.borrow().clone()
}

fn cat(parts: &[Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

//------------------------------------------------------------------------------
/// Inverse conversion for Leica M9 lens codes
/// Inputs: 0) value
/// Returns: Converted value, or undef on error
pub fn lens_type_conv_inv(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let v = arg(&a, 0);
    if let Some(c) = re(r"^(\d+) (\d+)$", "").m(&v) {
        ret(pl::add(&ops::shift_left(&cap(&c, 1), &n(2)), &ops::bit_and(&cap(&c, 2), &n(0x03))))
    } else if re(r"^\d+$", "").is_match(&v) {
        let bits = crate::runtime::lex_hash(it, MODULE, "frameSelectorBits").get(v.to_bytes());
        if bits.is_undef() {
            return pl::ret_undef();
        }
        ret(ops::bit_or(&ops::shift_left(&v, &n(2)), &bits))
    } else {
        pl::ret_undef()
    }
}

//------------------------------------------------------------------------------
/// Convert Leica Kelvin white balance
/// Inputs: 0) value, 1) flag to perform inverse conversion
/// Returns: Converted value, or undef on error
pub fn white_balance_conv(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (v, inv) = (arg(&a, 0), arg(&a, 1));
    if inv.truthy() {
        if let Some(c) = re(r"(\d+)", "").m(&v) {
            return ret(pl::add(&cap(&c, 1), &n(0x8000)));
        }
    } else if pl::gt(&v, &n(0x8000)) {
        return ret(pl::concat(&pl::sub(&v, &n(0x8000)), &pl::s(" Kelvin")));
    }
    pl::ret_undef()
}

//------------------------------------------------------------------------------
/// Process Leica makernotes in LEIC atom of MP4 videos (Leica T and X Vario)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_leica_leic(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let dir_start = or(dir_info.get("DirStart"), || n(0));
    let dir_len = dir_info.get("DirLen");
    let dir_len = if dir_len.truthy() {
        dir_len
    } else {
        pl::at(2752);
        pl::sub(&pl::length(&pl::sderef(&data_pt)?), &dir_start)
    };
    if pl::lt(&dir_len, &n(6)) {
        return ret(n(0));
    }
    call(it, "SetByteOrder", vec![pl::s("II")])?;
    let num_entries = call(it, "Get16u", vec![data_pt.clone(), dir_start.clone()])?;
    if pl::lt(&num_entries, &n(1)) || pl::gt(&num_entries, &n(255)) {
        return ret(n(0));
    }
    let size = call(it, "Get32u", vec![data_pt.clone(), pl::add(&dir_start, &n(2))])?;
    if pl::lt(&size, &pl::mul(&num_entries, &n(12))) || pl::gt(&pl::add(&size, &n(6)), &dir_len) {
        return ret(n(0));
    }
    // the Leica programmers want to make things difficult, so they store
    // the entry count before the directory size, making it impossible to
    // process as a standard TIFF IFD without a bit of reorganization...
    call(it, "Set16u", vec![num_entries, data_pt.clone(), pl::add(&dir_start, &n(4))])?;
    let dir_info2 = dir_info.hash_copy();
    dir_info2.set("DirStart", pl::add(&dir_start, &n(4)));
    dir_info2.set("DirLen", pl::sub(&size, &n(4)));
    dir_info2.set("DataPos", pl::sub(&dir_info2.get("DataPos"), &dir_start));
    dir_info2.set("Base", pl::add(&dir_info2.get("Base"), &dir_start));
    it.call_named("Image::ExifTool::Exif::ProcessExif", vec![et, dir_info2, tag_table_ptr], want)
}

//------------------------------------------------------------------------------
/// Process MakerNote trailer written by Leica S2
/// Inputs: 0) ExifTool object ref, 1) new absolute position of Leica trailer when writing
/// Returns: On success: 1 when reading, directory data when writing; otherwise undef
/// Notes:
/// - may be called twice for a file if the first call doesn't succeed
/// - must leave RAF position unchanged
/// - uses information from LeicaTrailer member:
///      TagInfo = tag info ref for MakerNote SubDirectory
///      Offset/Size = value offset/size from MakerNote IFD
///      TrailStart/TrailLen = actual JPEG trailer position/size (2nd call only)
/// - deletes LeicaTrailer member and sets LeicaTrailerPos when successful
pub fn process_leica_trailer(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, new_pos) = (arg(&a, 0), arg(&a, 1));
    let trail = et.get("LeicaTrailer");
    let raf = et.get("RAF");
    let trail_pos = trail.get("TrailPos");
    let pos = or(trail_pos.clone(), || trail.get("Offset"));
    let len = or(trail.get("TrailLen"), || trail.get("Size"));
    let buff = cell(Value::Undef);
    let result;
    let tag_ptr = Value::new_hash();

    if trail_pos.truthy() {
        et.delete("LeicaTrailer"); // done after this
    }
    if !pl::gt(&len, &n(0)) {
        if trail_pos.truthy() {
            pl::method(it, &et, "Warn", vec![pl::s("Missing Leica MakerNote trailer"), n(1)])?;
        }
        et.delete("LeicaTrailer");
        return pl::ret_undef();
    }
    let old_pos = pl::method(it, &raf, "Tell", vec![])?;
    let ok = pl::method(it, &raf, "Seek", vec![pos.clone(), n(0)])?.truthy() && pl::num_eq(&raf_read_cell(it, &raf, &buff, len.clone())?, &len);
    pl::method(it, &raf, "Seek", vec![old_pos, n(0)])?;
    if !ok {
        if trail_pos.truthy() {
            pl::method(it, &et, "Warn", vec![pl::s("Error reading Leica MakerNote trailer"), n(1)])?;
        }
        return pl::ret_undef();
    }
    // look for Leica MakerNote header (should be at start of
    // trailer, but allow up to 256 bytes of garbage just in case)
    // (the /g matches on $buff run in perl, so pos($buff) is kept as perl keeps it)
    let m = perl_eval(it, r"$buff =~ /^(.{0,256})LEICA\0..../sg ? ($1, pos($buff)) : ()", MODULE, &[("buff", &buff)], Want::List)?;
    if m.len() != 2 {
        let what = if trail_pos.truthy() { "trailer" } else { "offset" };
        pl::method(it, &et, "Warn", vec![pl::s(&format!("Invalid Leica MakerNote {what}")), n(1)])?;
        return pl::ret_undef();
    }
    let junk = m[0].clone();
    let start = pl::sub(&m[1], &n(10));
    if start.truthy() && !trail_pos.truthy() {
        pl::method(it, &et, "Warn", vec![pl::s("Invalid Leica MakerNote offset"), n(1)])?;
        return pl::ret_undef();
    }
    //
    // all checks passed -- go ahead and process the trailer now
    //
    let hdr_len = n(8);
    let dir_start = pl::add(&start, &hdr_len);
    let tag_info = trail.get("TagInfo");
    if et.get("HTML_DUMP").truthy() {
        let name = tag_info.get("Name");
        pl::method(it, &et, "HDump", vec![pl::add(&pos, &start), pl::sub(&len, &start), pl::concat(&name, &pl::s(" value")), pl::s("Leica MakerNote trailer"), n(4)])?;
        pl::method(it, &et, "HDump", vec![pl::add(&pos, &start), hdr_len.clone(), pl::s("MakerNotes header"), name])?;
    } else if pl::method(it, &et, "Options", vec![pl::s("Verbose")])?.truthy() {
        let where_ = pl::sprintf("at offset 0x%x", &[pos.clone()]);
        let msg = cat(&[pl::s("Leica MakerNote trailer ("), len.clone(), pl::s(" bytes "), where_, pl::s("):\n")]);
        pl::method(it, &et, "VPrint", vec![n(0), msg])?;
    }
    // delete LeicaTrailer member so we don't try to process it again
    et.delete("LeicaTrailer");
    et.set("LeicaTrailerPos", pl::add(&pos, &start)); // return actual start position of Leica trailer

    let old_order = call(it, "GetByteOrder", vec![])?;
    let num = call(it, "Get16u", vec![Value::ScalarRef(buff.clone()), dir_start.clone()])?; // get entry count
    if pl::gt(&ops::shift_right(&num, &n(8)), &ops::bit_and(&num, &n(0xff))) {
        call(it, "ToggleByteOrder", vec![])?; // set byte order
    }

    // use specialized algorithm to automatically fix offsets
    let val_start = pl::add(&pl::add(&pl::add(&dir_start, &n(2)), &pl::mul(&n(12), &num)), &n(4));
    let mut fix = n(0);
    if pl::lt(&val_start, &len) {
        let _val_block = pl::call(it, "Image::ExifTool::MakerNotes::GetValueBlocks", vec![Value::ScalarRef(buff.clone()), dir_start.clone(), tag_ptr.clone()])?;
        // find the minimum offset (excluding the PreviewImage tag 0x300 and 0x301)
        let mut min_ptr = Value::Undef;
        for k in tag_ptr.keys() {
            let kv = Value::bytes(&k[..]);
            let ptr = tag_ptr.get(&k[..]);
            if pl::num_eq(&kv, &n(0x300)) || pl::num_eq(&kv, &n(0x301)) || !ptr.truthy() || pl::num_eq(&ptr, &n(0xffffffff)) {
                continue;
            }
            if min_ptr.is_undef() || pl::gt(&min_ptr, &ptr) {
                min_ptr = ptr;
            }
        }
        if min_ptr.truthy() {
            let diff = pl::sub(&min_ptr, &pl::add(&val_start, &pos));
            let vs = cell(val_start.clone());
            let mut expect = Value::Undef;
            if et.get("Model").eq_str("S2") {
                // scan value data for the first non-zero byte
                let p = perl_eval(it, r"pos($buff) = $vs; $buff =~ /[^\0]/g ? (pos($buff)) : ()", MODULE, &[("buff", &buff), ("vs", &vs)], Want::List)?;
                if let Some(p) = p.first() {
                    let nn = pl::sub(&pl::sub(p, &n(1)), &val_start); // number of zero bytes
                    // S2 writes 282 bytes of zeros, exiftool writes none
                    expect = if pl::ge(&nn, &n(282)) { n(282) } else { n(0) };
                }
            } else {
                // M (Type 240)
                // scan for the lens type (M writes 114 bytes of garbage first)
                let p = perl_eval(it, r"pos($buff) = $vs; ($buff =~ /\G.{114}([\x20-\x7e]*\0*)/sg and length($1) >= 50) ? (1) : ()", MODULE, &[("buff", &buff), ("vs", &vs)], Want::List)?;
                if !p.is_empty() {
                    expect = n(114);
                }
            }
            let fix_base = pl::method(it, &et, "Options", vec![pl::s("FixBase")])?;
            if expect.is_undef() {
                pl::method(it, &et, "Warn", vec![pl::s("Unrecognized Leica trailer structure")])?;
            } else if !pl::num_eq(&diff, &expect) || !fix_base.is_undef() {
                fix = pl::sub(&expect, &diff);
                if !fix_base.is_undef() {
                    if !pl::str_eq(&fix_base, "") {
                        fix = fix_base.clone();
                    }
                    pl::method(it, &et, "Warn", vec![pl::concat(&pl::s("Adjusted MakerNotes base by "), &fix), n(1)])?;
                } else {
                    let msg = cat(&[pl::s("Possibly incorrect maker notes offsets (fixed by "), fix.clone(), pl::s(")")]);
                    pl::method(it, &et, "Warn", vec![msg, n(1)])?;
                }
            }
        }
    }
    // generate dirInfo for Leica MakerNote directory
    let dir_info = hash(vec![
        ("Name", tag_info.get("Name")),
        ("Base", fix.clone()),
        ("DataPt", Value::ScalarRef(buff.clone())),
        ("DataPos", pl::sub(&pos, &fix)),
        ("DataLen", len.clone()),
        ("DirStart", dir_start.clone()),
        ("DirLen", pl::sub(&len, &dir_start)),
        ("DirName", pl::s("MakerNotes")),
        ("Parent", pl::s("ExifIFD")),
        ("TagInfo", tag_info.clone()),
    ]);
    let tag_table_ptr = call(it, "GetTagTable", vec![tag_info.get("SubDirectory").get("TagTable")])?;
    if new_pos.truthy() {
        // are we writing?
        if !et.get("Model").eq_str("S2") {
            pl::method(it, &et, "Warn", vec![pl::s("Leica MakerNote trailer too messed up to edit.  Copying as a block"), n(1)])?;
            return ret(val(&buff));
        }
        // set position of new MakerNote IFD (+ 8 for Leica MakerNote header)
        dir_info.set("NewDataPos", pl::add(&pl::add(&new_pos, &start), &n(8)));
        let res = cell(pl::method(it, &et, "WriteDirectory", vec![dir_info.clone(), tag_table_ptr.clone()])?);
        // write preview image last if necessary and fix up the preview offsets
        let preview_info = et.get("PREVIEW_INFO");
        et.delete("PREVIEW_INFO");
        if val(&res).truthy() {
            if preview_info.truthy() {
                let fixup = preview_info.get("Fixup");
                // set preview offset (relative to start of makernotes, + 8 for makernote header)
                let l = pl::add(&pl::length(&val(&res)), &n(8));
                pl::method(it, &fixup, "SetMarkerPointers", vec![Value::ScalarRef(res.clone()), pl::s("PreviewImage"), l])?;
                let r = pl::concat(&val(&res), &preview_info.get("Data"));
                *res.borrow_mut() = r;
            }
            let hdr = substr(it, &val(&buff), &start, Some(&hdr_len))?;
            return ret(cat(&[junk, hdr, val(&res)]));
        }
        result = val(&res);
    } else {
        // extract information
        result = pl::method(it, &et, "ProcessDirectory", vec![dir_info.clone(), tag_table_ptr.clone()])?;
        // also extract as a block if necessary
        if pl::method(it, &et, "Options", vec![pl::s("MakerNotes")])?.truthy() || et.get("REQ_TAG_LOOKUP").get(pl::lc(&tag_info.get("Name")).to_bytes()).truthy() {
            // makernote header must be included in RebuildMakerNotes call
            dir_info.set("DirStart", pl::sub(&dir_info.get("DirStart"), &n(8)));
            dir_info.set("DirLen", pl::add(&dir_info.get("DirLen"), &n(8)));
            let bo = call(it, "GetByteOrder", vec![])?;
            et.set("MAKER_NOTE_BYTE_ORDER", bo);
            // rebuild maker notes (creates $$et{MAKER_NOTE_FIXUP})
            let mut v = pl::call(it, "Image::ExifTool::Exif::RebuildMakerNotes", vec![et.clone(), dir_info.clone(), tag_table_ptr.clone()])?;
            if v.is_undef() {
                if pl::gt(&len, &n(4)) {
                    pl::method(it, &et, "Warn", vec![pl::s("Error rebuilding maker notes (may be corrupt)")])?;
                }
                v = val(&buff);
            }
            let key = pl::method(it, &et, "FoundTag", vec![tag_info.clone(), v])?;
            pl::method(it, &et, "SetGroup", vec![key.clone(), pl::s("ExifIFD")])?;
            if et.get("MAKER_NOTE_FIXUP").truthy() {
                et.get_or_new_hash("TAG_EXTRA").get_or_new_hash(key.to_bytes()).set("Fixup", et.get("MAKER_NOTE_FIXUP"));
                et.delete("MAKER_NOTE_FIXUP");
            }
        }
    }
    call(it, "SetByteOrder", vec![old_order])?;
    ret(result)
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Panasonic::LensTypeConvInv", lens_type_conv_inv);
    it.define_native("Image::ExifTool::Panasonic::WhiteBalanceConv", white_balance_conv);
    it.define_native("Image::ExifTool::Panasonic::ProcessLeicaLEIC", process_leica_leic);
    it.define_native("Image::ExifTool::Panasonic::ProcessLeicaTrailer", process_leica_trailer);
}
