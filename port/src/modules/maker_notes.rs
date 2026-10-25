//! Image::ExifTool::MakerNotes (Image/ExifTool/MakerNotes.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! The conditional list of maker notes (@Image::ExifTool::MakerNotes::Main)
//! is data; these are the module's subs: maker note offsets, base fixing,
//! IFD location in unknown maker notes, and the special processors.

use std::rc::Rc;

use crate::interp::eval::{Binding, ER};
use crate::interp::{Code, Interp, Scope, Want};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, re, ret};

const PKG: &str = "Image::ExifTool::MakerNotes";

fn n(i: i64) -> Value {
    Value::Int(i)
}

// ---- local helpers ----

/// `substr($s, $off, $len)` (undef past the end, as perl).
fn substr(s: &Value, off: i64, len: Option<i64>) -> Value {
    let b = s.to_bytes();
    match crate::interp::eval::substr_range(b.len(), off, len) {
        Some((a, e)) => Value::bytes(&b[a..e]),
        None => Value::Undef,
    }
}

/// `\$val`: a new reference to a copy of a scalar.
fn new_ref(v: Value) -> Value {
    Value::ScalarRef(Rc::new(std::cell::RefCell::new(v)))
}

/// `$x || $default`
fn or(v: Value, d: Value) -> Value {
    if v.truthy() {
        v
    } else {
        d
    }
}

fn get16u(it: &mut Interp, data_pt: &Value, pos: Value) -> ER<Value> {
    pl::call(it, "Image::ExifTool::Get16u", vec![data_pt.clone(), pos])
}

fn get32u(it: &mut Interp, data_pt: &Value, pos: Value) -> ER<Value> {
    pl::call(it, "Image::ExifTool::Get32u", vec![data_pt.clone(), pos])
}

fn get_byte_order(it: &mut Interp) -> ER<Value> {
    pl::call(it, "Image::ExifTool::GetByteOrder", vec![])
}

/// `$Image::ExifTool::Exif::formatSize[$format]`
fn format_size(it: &mut Interp, format: &Value) -> Value {
    match it.global(b'@', "Image::ExifTool::Exif::formatSize") {
        Binding::Array(a) => Value::ArrayRef(a).at(format.to_i64()),
        _ => Value::Undef,
    }
}

/// `$debug`, the file's debugging switch.
fn debug(it: &mut Interp) -> bool {
    crate::runtime::lex_scalar(it, PKG, "debug").borrow().truthy()
}

/// `print $out $str` (or to STDOUT when `$out` is undef), through Perl so
/// the output goes where ExifTool's own prints go.
fn print_to(it: &mut Interp, out: Option<Value>, s: Value) -> ER<()> {
    let scope = Scope::new(None);
    scope.scalar("s", s);
    let code: &[u8] = match out {
        Some(o) => {
            scope.scalar("out", o);
            b"print $out $s"
        }
        None => b"print $s",
    };
    match it.eval_string(code, PKG, &scope, Want::Void) {
        Ok(_) => Ok(()),
        Err(e) => Err(crate::interp::Flow::Die(e)),
    }
}

/// `eval $code` at an eval site, with its variables; undef on error (with
/// `$@` set, as perl).
fn eval_site(it: &mut Interp, code: &Value, vars: &[(&str, Value)]) -> Value {
    let scope = Scope::new(None);
    for (name, v) in vars {
        scope.scalar(name, v.clone());
    }
    match it.eval_string(&code.to_bytes(), PKG, &scope, Want::Scalar) {
        Ok(v) => v.last().cloned().unwrap_or_default(),
        Err(_) => Value::Undef,
    }
}

/// `$code eq \&Image::ExifTool::MakerNotes::NAME`
fn is_sub(it: &mut Interp, code: &Value, name: &str) -> bool {
    let r: Rc<Code> = it.sub_ref(&format!("{PKG}::{name}"));
    code.same_ref(&Value::Code(r))
}

/// Keys of a hash sorted numerically (`sort { $a <=> $b } keys %$h`).
fn sorted_num_keys(h: &Value) -> Vec<Value> {
    let mut k: Vec<Value> = h.keys().into_iter().map(Value::from).collect();
    k.sort_by(|a, b| ops::num_cmp(a, b).unwrap_or(std::cmp::Ordering::Equal));
    k
}

//------------------------------------------------------------------------------
/// Get normal offset of value data from end of maker note IFD
/// Inputs: 0) ExifTool object reference
/// Returns: Array: 0) relative flag (undef for no change)
///                 1) normal offset from end of IFD to first value (empty if unknown)
///                 2-N) other possible offsets used by some models
/// Notes: Directory size should be validated before calling this routine
pub fn get_maker_note_offset(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let et = arg(&a, 0);
    let _ = it;
    // figure out where we expect the value data based on camera type
    let make = et.get("Make");
    let model = et.get("Model");
    let mut relative = Value::Undef;
    let mut offsets: Vec<Value> = Vec::new();

    // normally value data starts 4 bytes after end of directory, so this is the default.
    // offsets of 0 and 4 are always allowed even if not specified,
    // but the first offset specified is the one used when writing
    if re(r"^Canon", "").is_match(&make) {
        offsets.push(if re(r"\b(20D|350D|REBEL XT|Kiss Digital N)\b", "").is_match(&model) { n(6) } else { n(4) });
        // some Canon models (FV-M30, Optura50, Optura60) leave 24 unused bytes
        // at the end of the IFD (2 spare IFD entries?)
        if re(r"\b(FV\b|OPTURA)", "").is_match(&model) {
            offsets.push(n(28));
        }
        // some Canon PowerShot models leave 12 unused bytes
        if re(r"(PowerShot|IXUS|IXY)", "").is_match(&model) {
            offsets.push(n(16));
        }
    } else if re(r"^CASIO", "").is_match(&make) {
        // Casio AVI and MOV images use no padding, and their JPEG's use 4,
        // except some models like the EX-S770,Z65,Z70,Z75 and Z700 which use 16,
        // and the EX-Z35 which uses 2 (grrrr...)
        if re(r"^(RIFF|MOV)$", "").is_match(&et.get("FILE_TYPE")) {
            offsets.push(n(0));
        } else {
            offsets.extend([n(4), n(16), n(2)]);
        }
    } else if re(r"^(General Imaging Co.|GEDSC IMAGING CORP.)", "i").is_match(&make) {
        offsets.push(n(0));
    } else if re(r"^KYOCERA", "").is_match(&make) {
        offsets.push(n(12));
    } else if re(r"^Leica Camera AG", "").is_match(&make) {
        if pl::str_eq(&model, "S2") {
            // lots of empty space before first value in S2 images
            offsets.extend([n(4), if pl::str_eq(&et.get("FILE_TYPE"), "JPEG") { n(286) } else { n(274) }]);
        } else if pl::str_eq(&model, "LEICA M MONOCHROM (Typ 246)") {
            offsets.extend([n(4), n(130)]);
        } else if pl::str_eq(&model, "LEICA M (Typ 240)") {
            offsets.extend([n(4), n(118)]);
        } else if re(r"^(R8|R9|M8)\b", "").is_match(&model) {
            offsets.push(n(6));
        } else {
            offsets.push(n(4));
        }
    } else if re(r"^OLYMPUS", "").is_match(&make) && re(r"^E-(1|300|330)\b", "").is_match(&model) {
        offsets.push(n(16));
    } else if re(r"^OLYMPUS", "").is_match(&make) &&
        // these Olympus models are just weird
        re(r"^(C2500L|C-1Z?|C-5000Z|X-2|C720UZ|C725UZ|C150|C2Z|E-10|E-20|FerrariMODEL2003|u20D|u10D)\b", "").is_match(&model)
    {
        // no expected offset --> determine offset empirically via FixBase()
    } else if re(r"^(Panasonic|JVC)\b", "").is_match(&make) {
        offsets.push(n(0));
    } else if re(r"^SONY", "").is_match(&make) {
        // earlier DSLR and "PREMI" models use an offset of 4
        if re(r"^(DSLR-.*|SLT-A(33|35|55V)|NEX-(3|5|C3|VG10E))$", "").is_match(&model) || et.get("OlympusCAMER").truthy() {
            offsets.push(n(4));
        } else {
            offsets.push(n(0));
        }
    } else if pl::str_eq(&et.get("TIFF_TYPE"), "SRW") && pl::str_eq(&make, "SAMSUNG") && pl::str_eq(&model, "EK-GN120") {
        offsets.push(n(40)); // patch to read most of the maker notes, but breaks PreviewIFD
    } else if pl::str_eq(&make, "FUJIFILM") {
        // some models have offset of 6, so allow that too (A345,A350,A360,A370)
        offsets.extend([n(4), n(6)]);
    } else if re(r"^TOSHIBA", "").is_match(&make) {
        // similar to Canon, can also have 24 bytes of padding
        offsets.extend([n(0), n(24)]);
    } else if re(r"^PENTAX", "").is_match(&make) {
        offsets.push(n(4));
        // the Pentax addressing mode is determined automatically, but
        // sometimes the algorithm gets it wrong, but Pentax always uses
        // absolute addressing, so force it to be absolute
        relative = n(0);
    } else if re(r"^Konica Minolta", "i").is_match(&make) {
        // patch for DiMAGE X50, Xg, Z2 and Z10
        offsets.extend([n(4), n(-16)]);
    } else if re(r"^Minolta", "").is_match(&make) {
        // patch for DiMAGE 7, X20 and Z1
        offsets.extend([n(4), n(-8), n(-12)]);
    } else {
        offsets.push(n(4)); // the normal offset
    }
    // (in scalar context, the list's last element is @offsets: its count)
    if want != Want::List {
        return ret(n(offsets.len() as i64));
    }
    let mut r = vec![relative];
    r.extend(offsets);
    Ok(r)
}

//------------------------------------------------------------------------------
/// Get hash of value offsets / block sizes
/// Inputs: 0) Data pointer, 1) offset to start of directory,
///         2) hash ref to return value pointers based on tag ID
/// Returns: 0) hash reference: keys are offsets, values are block sizes
///          1) same thing, but with keys adjusted for value-based offsets
/// Notes: Directory size should be validated before calling this routine
/// - calculates MIN and MAX offsets in entry-based hash
pub fn get_value_blocks(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (data_pt, dir_start, tag_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let (vb, vba) = get_value_blocks_rs(it, &data_pt, &dir_start, &tag_ptr)?;
    Ok(vec![vb, vba])
}

fn get_value_blocks_rs(it: &mut Interp, data_pt: &Value, dir_start: &Value, tag_ptr: &Value) -> ER<(Value, Value)> {
    let num_entries = get16u(it, data_pt, dir_start.clone())?;
    let val_block = Value::new_hash();
    let val_blk_adj = Value::new_hash();
    let mut index = n(0);
    while pl::lt(&index, &num_entries) {
        let entry = pl::add(&pl::add(dir_start, &n(2)), &pl::mul(&n(12), &index));
        let format = get16u(it, data_pt, pl::add(&entry, &n(2)))?;
        if pl::lt(&format, &n(1)) || pl::gt(&format, &n(13)) {
            break;
        }
        let count = get32u(it, data_pt, pl::add(&entry, &n(4)))?;
        let size = pl::mul(&count, &format_size(it, &format));
        if pl::le(&size, &n(4)) {
            index = pl::add(&index, &n(1));
            continue;
        }
        let mut val_ptr = get32u(it, data_pt, pl::add(&entry, &n(8)))?;
        if tag_ptr.truthy() {
            let tag = get16u(it, data_pt, entry.clone())?;
            tag_ptr.set(tag.to_bytes(), val_ptr.clone());
        }
        // save location and size of longest block at this offset
        let k = val_ptr.to_bytes();
        if !(val_block.exists(&k) && !val_block.get(&k).is_undef() && pl::gt(&val_block.get(&k), &size)) {
            val_block.set(&k, size.clone());
        }
        // adjust for case of value-based offsets
        val_ptr = pl::add(&val_ptr, &pl::mul(&n(12), &index));
        let k = val_ptr.to_bytes();
        if !(!val_blk_adj.get(&k).is_undef() && pl::gt(&val_blk_adj.get(&k), &size)) {
            val_blk_adj.set(&k, size.clone());
            let end = pl::add(&val_ptr, &size);
            if !val_blk_adj.get("MIN").is_undef() {
                // save minimum only if it has a value of 12 or greater
                if pl::lt(&val_blk_adj.get("MIN"), &n(12)) || pl::gt(&val_blk_adj.get("MIN"), &val_ptr) {
                    val_blk_adj.set("MIN", val_ptr.clone());
                }
                if pl::gt(&val_blk_adj.get("MAX"), &end) {
                    val_blk_adj.set("MAX", end);
                }
            } else {
                val_blk_adj.set("MIN", val_ptr.clone());
                val_blk_adj.set("MAX", end);
            }
        }
        index = pl::add(&index, &n(1));
    }
    Ok((val_block, val_blk_adj))
}

//------------------------------------------------------------------------------
/// Fix base for value offsets in maker notes IFD (if necessary)
/// Inputs: 0) ExifTool object ref, 1) DirInfo hash ref
/// Return: amount of base shift (and $dirInfo Base and DataPos are updated,
///         FixedBy is set if offsets fixed, and Relative or EntryBased may be set)
pub fn fix_base(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    // don't fix base if fixing offsets individually or if we don't want to fix them
    if dir_info.get("FixOffsets").truthy() || dir_info.get("NoFixBase").truthy() {
        return ret(n(0));
    }

    let data_pt = dir_info.get("DataPt");
    let data_pos = dir_info.get("DataPos");
    let dir_start = or(dir_info.get("DirStart"), n(0));
    let mut entry_based = dir_info.get("EntryBased");
    let dir_name = dir_info.get("DirName");
    let mut fix_base = pl::method(it, &et, "Options", vec![pl::s("FixBase")])?;
    let set_base = !fix_base.is_undef() && !pl::str_eq(&fix_base, "");
    let mut fix;
    let mut fixed_by = Value::Undef;
    let tag_ptr = Value::new_hash();

    // get hash of value block positions
    let (val_block, val_blk_adj) = get_value_blocks_rs(it, &data_pt, &dir_start, &tag_ptr)?;
    if val_block.is_empty() {
        return ret(n(0));
    }
    // get sorted list of value offsets
    let val_ptrs = sorted_num_keys(&val_block);
    let warn = |it: &mut Interp, msg: Value| -> ER<()> {
        pl::method(it, &et, "Warn", vec![msg, n(1)])?;
        Ok(())
    };
//
// handle special case of Canon maker notes with TIFF footer containing original offset
//
    if re(r"^Canon", "").is_match(&et.get("Make")) && pl::gt(&dir_info.get("DirLen"), &n(8)) {
        let footer_pos = pl::sub(&pl::add(&dir_start, &dir_info.get("DirLen")), &n(8));
        let footer = substr(&data_pt.deref(), footer_pos.to_i64(), Some(8));
        if re(r"^(II\x2a\0|MM\0\x2a)", "").is_match(&footer) &&  // check for TIFF footer
            substr(&footer, 0, Some(2)).to_bytes() == get_byte_order(it)?.to_bytes() // validate byte ordering
        {
            let old_offset = get32u(it, &new_ref(footer.clone()), n(4))?;
            let new_offset = pl::add(&dir_start, &data_pos);
            if set_base {
                fix = fix_base.clone();
            } else {
                fix = pl::sub(&new_offset, &old_offset);
                if !fix.truthy() {
                    return ret(n(0));
                }
                // Picasa and ACDSee have a bug where they update other offsets without
                // updating the TIFF footer (PH - 2009/02/25), so test for this case:
                // validate Canon maker note footer fix by checking offset of last value
                let last = val_ptrs.last().unwrap();
                let max_pt = pl::add(last, &val_block.get(last.to_bytes()));
                // compare to end of maker notes, taking 8-byte footer into account
                let end_diff = pl::sub(&pl::sub(&pl::add(&dir_start, &dir_info.get("DirLen")), &pl::sub(&max_pt, &data_pos)), &n(8));
                // ignore footer offset only if end difference is exactly correct
                // (allow for possible padding byte, although I have never seen this)
                if !end_diff.truthy() || pl::num_eq(&end_diff, &n(1)) {
                    warn(it, pl::s("Canon maker note footer may be invalid (ignored)"))?;
                    return ret(n(0));
                }
            }
            warn(it, pl::concat(&pl::concat(&pl::concat(&pl::s("Adjusted "), &dir_name), &pl::s(" base by ")), &fix))?;
            dir_info.set("FixedBy", fix.clone());
            dir_info.set("Base", pl::add(&dir_info.get("Base"), &fix));
            dir_info.set("DataPos", pl::sub(&dir_info.get("DataPos"), &fix));
            return ret(fix);
        }
    }
//
// analyze value offsets to see if they need fixing.  The first task is to determine
// the minimum valid offset used (this is tricky, because we have to weed out bad
// offsets written by some cameras)
//
    let mut min_pt = val_ptrs[0].clone(); // if life were simple, this would be it
    dir_info.set("MinOffset", min_pt.clone());
    let ifd_len = pl::add(&n(2), &pl::mul(&n(12), &get16u(it, &dir_info.get("DataPt"), dir_start.clone())?));
    let ifd_end = pl::add(&dir_start, &ifd_len);
    let mno = get_maker_note_offset(it, vec![et.clone()], Want::List)?;
    let mut relative = arg(&mno, 0);
    let mut offsets: Vec<Value> = mno[1..].to_vec();
    let mut make_diff = arg(&offsets, 0);
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    let mut diff = Value::Undef;
    let mut shift = Value::Undef;

    // calculate expected minimum value offset
    let expected = pl::add(&pl::add(&data_pos, &ifd_end), &if !make_diff.is_undef() { make_diff.clone() } else { n(4) });
    if debug(it) {
        print_to(it, None, pl::concat(&expected, &pl::s(" expected\n")))?;
    }

    // zero our counters
    let (mut count_neg12, mut count_zero, mut count_overlap) = (0i64, 0i64, 0i64);
    let mut last = Value::Undef;
    for val_ptr in &val_ptrs {
        if debug(it) {
            let s = pl::sprintf("%d - %d (%d)\n", &[val_ptr.clone(), pl::add(val_ptr, &val_block.get(val_ptr.to_bytes())), pl::sub(val_ptr, &or(last.clone(), n(0)))]);
            print_to(it, None, s)?;
        }
        if !last.is_undef() {
            let gap = pl::sub(val_ptr, &last);
            if pl::num_eq(&gap, &n(0)) || pl::num_eq(&gap, &n(1)) {
                count_zero += 1;
            } else if pl::num_eq(&gap, &n(-12)) && !entry_based.truthy() {
                // you get this when value offsets are relative to the IFD entry
                count_neg12 += 1;
            } else if pl::lt(&gap, &n(0)) {
                // any other negative difference indicates overlapping values
                if val_ptr.truthy() {
                    count_overlap += 1; // (but ignore zero value pointers)
                }
            } else if pl::ge(&gap, &ifd_len) {
                // ignore previous minimum if we took a jump to near the expected value
                // (some values can be stored before the IFD)
                if pl::le(&pl::abs(&pl::sub(val_ptr, &expected)), &n(4)) {
                    min_pt = val_ptr.clone();
                }
            }
            // an offset less than 12 is surely garbage, so ignore it
            if pl::lt(&min_pt, &n(12)) {
                min_pt = val_ptr.clone();
            }
        }
        last = pl::add(val_ptr, &val_block.get(val_ptr.to_bytes()));
    }
    // could this IFD be using entry-based offsets?
    let adj_min = val_blk_adj.get("MIN");
    if ((count_neg12 > count_zero && pl::ge(&adj_min, &pl::sub(&ifd_len, &n(2))))
        || (pl::num_eq(&adj_min, &pl::sub(&ifd_len, &n(2))) || pl::num_eq(&adj_min, &pl::add(&ifd_len, &n(2)))))
        && pl::le(&val_blk_adj.get("MAX"), &pl::sub(&dir_info.get("DirLen"), &n(2)))
    {
        // looks like these offsets are entry-based, so use the offsets
        // which have been correcting for individual entry position
        entry_based = n(1);
        if verbose.truthy() {
            warn(it, pl::concat(&dir_name, &pl::s(" offsets are entry-based")))?;
        }
    } else {
        // calculate offset difference from end of IFD to first value
        diff = pl::sub(&pl::sub(&min_pt, &data_pos), &ifd_end);
        shift = n(0);
        if count_overlap != 0 {
            warn(it, pl::concat(&pl::s("Overlapping "), &pl::concat(&dir_name, &pl::s(" values"))))?;
        }
        if entry_based.truthy() {
            warn(it, pl::concat(&dir_name, &pl::s(" offsets do NOT look entry-based")))?;
            entry_based = Value::Undef;
            relative = Value::Undef;
        }
        // use PrintIM tag to do special check for correct absolute offsets
        if tag_ptr.get("3584").truthy() {
            let ptr = pl::sub(&tag_ptr.get("3584"), &data_pos);
            let d = data_pt.deref();
            if pl::gt(&ptr, &n(0))
                && pl::le(&ptr, &pl::sub(&pl::length(&d), &n(8)))
                && substr(&d, ptr.to_i64(), Some(8)).to_bytes()[..] == b"PrintIM\0"[..]
            {
                return ret(n(0));
            }
        }
        // allow a range of reasonable differences for Unknown maker notes
        if dir_info.get("FixBase").truthy() && pl::num_eq(&dir_info.get("FixBase"), &n(2)) && pl::ge(&diff, &n(0)) && pl::le(&diff, &n(24)) {
            return ret(n(0));
        }
        // ******** (used for testing to extract differences) ********
        // $et->FoundTag('Diff', $diff);
        // $et->FoundTag('MakeDiff',$makeDiff);
    }
//
// handle entry-based offsets
//
    if entry_based.truthy() {
        if debug(it) {
            print_to(it, None, pl::s("--> entry-based!\n"))?;
        }
        // most of my entry-based samples have first value immediately
        // after last IFD entry (ie. no padding or next IFD pointer)
        make_diff = n(0);
        offsets.push(n(4)); // but some do have a next IFD pointer
        // corrected entry-based offsets are relative to start of first entry
        let expected = pl::mul(&n(12), &get16u(it, &dir_info.get("DataPt"), dir_start.clone())?);
        diff = pl::sub(&val_blk_adj.get("MIN"), &expected);
        // set up directory to read values with entry-based offsets
        // (ignore everything and set base to start of first entry)
        shift = pl::add(&pl::add(&data_pos, &dir_start), &n(2));
        dir_info.set("Base", pl::add(&dir_info.get("Base"), &shift));
        dir_info.set("DataPos", pl::sub(&dir_info.get("DataPos"), &shift));
        dir_info.set("EntryBased", n(1));
        dir_info.set("Relative", n(1)); // entry-based offsets are relative
        dir_info.delete("FixBase"); // no automatic base fix
        if !set_base {
            fix_base = Value::Undef;
        }
    }
//
// return without doing shift if offsets look OK
//
    if !set_base {
        // don't try to fix offsets for whacky cameras
        if make_diff.is_undef() {
            return ret(shift);
        }
        // normal value data starts 4 bytes after IFD, but allow 0 or 4...
        if pl::num_eq(&diff, &n(0)) || pl::num_eq(&diff, &n(4)) {
            return ret(shift);
        }
        // also check for allowed make-specific differences
        for o in &offsets {
            if pl::num_eq(&diff, o) {
                return ret(shift);
            }
        }
    }
//
// apply the fix, or issue a warning
//
    // use default padding of 4 bytes unless already specified
    if make_diff.is_undef() {
        make_diff = n(4);
    }
    fix = pl::sub(&make_diff, &diff); // assume standard diff for this make

    if dir_info.get("FixBase").truthy() {
        // set flag if offsets are relative (base is at or above directory start)
        if pl::le(&pl::add(&pl::sub(&data_pos, &fix), &dir_start), &n(0)) {
            dir_info.set("Relative", if !relative.is_undef() { relative.clone() } else { n(1) });
        }
        if set_base {
            fixed_by = fix_base.clone();
            fix = pl::add(&fix, &fix_base);
        }
    } else if !fix_base.is_undef() {
        if !pl::str_eq(&fix_base, "") {
            fix = fix_base.clone();
        }
        fixed_by = fix.clone();
    } else {
        // print warning unless difference looks reasonable
        if pl::lt(&diff, &n(0)) || pl::gt(&diff, &n(16)) || ops::bit_and(&diff, &n(0x01)).truthy() {
            warn(it, pl::concat(&pl::concat(&pl::s("Possibly incorrect maker notes offsets (fix by "), &fix), &pl::s("?)")))?;
        }
        // don't do the fix (but we already adjusted base if entry-based)
        return ret(shift);
    }
    if !fixed_by.is_undef() {
        warn(it, pl::concat(&pl::concat(&pl::concat(&pl::s("Adjusted "), &dir_name), &pl::s(" base by ")), &fixed_by))?;
        dir_info.set("FixedBy", fixed_by);
    }
    dir_info.set("Base", pl::add(&dir_info.get("Base"), &fix));
    dir_info.set("DataPos", pl::sub(&dir_info.get("DataPos"), &fix));
    ret(pl::add(&fix, &shift))
}

//------------------------------------------------------------------------------
/// Find start of IFD in unknown maker notes
/// Inputs: 0) reference to directory information
/// Returns: offset to IFD on success, undefined otherwise
/// - dirInfo may contain TagInfo reference for tag associated with directory
/// - on success, updates DirStart, DirLen, Base and DataPos in dirInfo
/// - also sets Relative flag in dirInfo if offsets are relative to IFD
/// Note: Changes byte ordering!
pub fn locate_ifd(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    ret(locate_ifd_rs(it, &et, &dir_info)?)
}

fn locate_ifd_rs(it: &mut Interp, et: &Value, dir_info: &Value) -> ER<Value> {
    let data_pt = dir_info.get("DataPt");
    let dir_start = or(dir_info.get("DirStart"), n(0));
    // (ignore MakerNotes DirLen since sometimes this is incorrect)
    let size = pl::sub(&dir_info.get("DataLen"), &dir_start);
    let dir_len = if !dir_info.get("DirLen").is_undef() { dir_info.get("DirLen") } else { size.clone() };
    let tag_info = dir_info.get("TagInfo");
    let mut ifd_offset_pos = Value::Undef;
    // the IFD should be within the first 32 bytes
    // (Kyocera sets the current record at 22 bytes)
    let (mut first_try, mut last_try) = (n(0), n(32));

    // make sure Base and DataPos are defined
    if !dir_info.get("Base").truthy() {
        dir_info.set("Base", n(0));
    }
    if !dir_info.get("DataPos").truthy() {
        dir_info.set("DataPos", n(0));
    }
//
// use tag information (if provided) to determine directory location
//
    if tag_info.truthy() && tag_info.get("SubDirectory").truthy() {
        let subdir = tag_info.get("SubDirectory");
        let pp = subdir.get("ProcessProc");
        if !(pp.truthy() && (is_sub(it, &pp, "ProcessUnknown") || is_sub(it, &pp, "ProcessUnknownOrPreview"))) {
            // look for the IFD at the "Start" specified in our SubDirectory information
            let value_ptr = dir_start.clone();
            let mut new_start = dir_start.clone();
            if !subdir.get("Start").is_undef() {
                //### eval Start ($valuePtr)
                new_start = eval_site(it, &subdir.get("Start"), &[("valuePtr", value_ptr.clone())]);
            }
            if subdir.get("Base").truthy() {
                // calculate subdirectory start relative to $base for eval
                let start = pl::add(&new_start, &dir_info.get("DataPos"));
                let base = or(dir_info.get("Base"), n(0));
                //### eval Base ($start,$base)
                let base_shift = eval_site(it, &subdir.get("Base"), &[("start", start), ("base", base)]);
                // shift directory base (note: we may do this again below
                // if an OffsetPt is defined, but that doesn't matter since
                // the base shift is relative to DataPos, which we set too)
                dir_info.set("Base", pl::add(&dir_info.get("Base"), &base_shift));
                dir_info.set("DataPos", pl::sub(&dir_info.get("DataPos"), &base_shift));
                // this is a relative directory if Base depends on $start
                if re(r"\$start\b", "").is_match(&subdir.get("Base")) {
                    dir_info.set("Relative", n(1));
                    // hack to fix Leica quirk
                    if pp.truthy() && is_sub(it, &pp, "FixLeicaBase") {
                        let old_start = dir_info.get("DirStart");
                        dir_info.set("DirStart", new_start.clone());
                        fix_leica_base_rs(it, et, dir_info, &Value::Undef)?;
                        dir_info.set("DirStart", old_start);
                    }
                }
            }
            // add offset to the start of the directory if necessary
            if subdir.get("OffsetPt").truthy() {
                if re(r"^Little", "i").is_match(&subdir.get("ByteOrder")) {
                    pl::call(it, "Image::ExifTool::SetByteOrder", vec![pl::s("II")])?;
                } else if re(r"^Big", "i").is_match(&subdir.get("ByteOrder")) {
                    pl::call(it, "Image::ExifTool::SetByteOrder", vec![pl::s("MM")])?;
                } else {
                    pl::warn(it, "Can't have variable byte ordering for SubDirectories using OffsetPt\n")?;
                    return Ok(Value::Undef);
                }
                //### eval OffsetPt ($valuePtr)
                ifd_offset_pos = pl::sub(&eval_site(it, &subdir.get("OffsetPt"), &[("valuePtr", value_ptr.clone())]), &dir_start);
            }
            // pinpoint position to look for this IFD
            first_try = pl::sub(&new_start, &dir_start);
            last_try = first_try.clone();
        }
    }
//
// scan for something that looks like an IFD
//
    if pl::ge(&dir_len, &pl::add(&n(14), &first_try)) {
        // minimum size for an IFD
        let mut offset = first_try.clone();
        'ifd_try: while pl::le(&offset, &last_try) {
            let this = offset.clone();
            offset = pl::add(&offset, &n(2));
            let offset = this;
            if pl::gt(&pl::add(&offset, &n(14)), &dir_len) {
                break; // 14 bytes is minimum size for an IFD
            }
            let pos = pl::add(&dir_start, &offset);
//
// look for a standard TIFF header (Nikon uses it, others may as well),
//
            if pl::call(it, "Image::ExifTool::SetByteOrder", vec![substr(&data_pt.deref(), pos.to_i64(), Some(2))])?.truthy()
                && pl::num_eq(&get16u(it, &data_pt, pl::add(&pos, &n(2)))?, &n(0x2a))
            {
                ifd_offset_pos = n(4);
            }
            if !ifd_offset_pos.is_undef() {
                // get pointer to IFD
                let ptr = get32u(it, &data_pt, pl::add(&pos, &ifd_offset_pos))?;
                if pl::ge(&ptr, &pl::add(&ifd_offset_pos, &n(4))) && pl::le(&pl::add(&pl::add(&ptr, &offset), &n(14)), &dir_len) {
                    // shift directory start and shorten dirLen accordingly
                    let po = pl::add(&ptr, &offset);
                    dir_info.set("DirStart", pl::add(&dir_info.get("DirStart"), &po));
                    dir_info.set("DirLen", pl::sub(&dir_info.get("DirLen"), &po));
                    // shift pointer base to the start of the TIFF header
                    let shift = pl::add(&pl::add(&dir_info.get("DataPos"), &dir_start), &offset);
                    dir_info.set("Base", pl::add(&dir_info.get("Base"), &shift));
                    dir_info.set("DataPos", pl::sub(&dir_info.get("DataPos"), &shift));
                    dir_info.set("Relative", n(1)); // set "relative offsets" flag
                    return Ok(po);
                }
                ifd_offset_pos = Value::Undef;
            }
//
// look for a standard IFD (starts with 2-byte entry count)
//
            let mut num = get16u(it, &data_pt, pos.clone())?;
            if !num.truthy() {
                continue;
            }
            // number of entries in an IFD should be between 1 and 255
            if !ops::bit_and(&num, &n(0xff)).truthy() {
                // lower byte is zero -- byte order could be wrong
                pl::call(it, "Image::ExifTool::ToggleByteOrder", vec![])?;
                num = ops::shift_right(&num, &n(8));
            } else if ops::bit_and(&num, &n(0xff00)).truthy() {
                // upper byte isn't zero -- not an IFD
                continue;
            }
            let bytes_from_end = pl::sub(&size, &pl::add(&pl::add(&offset, &n(2)), &pl::mul(&n(12), &num)));
            if pl::lt(&bytes_from_end, &n(4)) && !(pl::num_eq(&bytes_from_end, &n(2)) || pl::num_eq(&bytes_from_end, &n(0))) {
                continue;
            }
            // do a quick validation of all format types
            let mut index = n(0);
            while pl::lt(&index, &num) {
                let cur = index.clone();
                index = pl::add(&index, &n(1));
                let index = cur;
                let entry = pl::add(&pl::add(&pos, &n(2)), &pl::mul(&n(12), &index));
                let format = get16u(it, &data_pt, pl::add(&entry, &n(2)))?;
                let count = get32u(it, &data_pt, pl::add(&entry, &n(4)))?;
                if !format.truthy() {
                    // patch for buggy Samsung NX200 JPEG MakerNotes entry count
                    if pl::num_eq(&num, &n(23)) && pl::num_eq(&index, &n(21)) && pl::str_eq(&et.get("Make"), "SAMSUNG") {
                        pl::call(it, "Image::ExifTool::Set16u", vec![n(21), data_pt.clone(), pos.clone()])?; // really 21 IFD entries!
                        pl::method(it, et, "Warn", vec![pl::s("Fixed incorrect Makernote entry count"), n(1)])?;
                        break;
                    }
                    // allow everything to be zero if not first entry
                    // because some manufacturers pad with null entries
                    if !(count.truthy() || pl::num_eq(&index, &n(0))) {
                        continue;
                    }
                    // patch for Canon EOS 40D firmware 1.0.4 bug: allow zero format for last entry
                    if pl::num_eq(&index, &pl::sub(&num, &n(1))) && re(r"EOS 40D", "").is_match(&et.get("Model")) {
                        continue;
                    }
                }
                // patch for Sony cameras like the DSC-P10 that have invalid MakerNote entries
                if pl::num_eq(&num, &n(12)) && pl::str_eq(&et.get("Make"), "SONY") && pl::ge(&index, &n(8)) {
                    continue;
                }
                // patch for Apple ProRaw DNG which uses format 16 in the maker notes
                if pl::num_eq(&format, &n(16)) && pl::str_eq(&et.get("Make"), "Apple") {
                    continue;
                }
                // (would like to verify tag ID, but some manufactures don't
                //  sort entries in order of tag ID so we don't have much of
                //  a handle to verify this field)
                // verify format
                if pl::lt(&format, &n(1)) || pl::gt(&format, &n(13)) {
                    continue 'ifd_try;
                }
                // count must be reasonable (can't test for zero count because
                // cameras like the 1DmkIII use this value)
                if ops::bit_and(&count, &n(0xff000000)).truthy() {
                    continue 'ifd_try;
                }
                // extra tests to avoid mis-identifying Samsung makernotes (GT-I9000, etc)
                if !pl::num_eq(&num, &n(1)) {
                    continue;
                }
                let value_size = pl::mul(&count, &format_size(it, &format));
                if pl::gt(&value_size, &n(4)) {
                    if pl::gt(&value_size, &size) {
                        continue 'ifd_try;
                    }
                    let value_ptr = get32u(it, &data_pt, pl::add(&entry, &n(8)))?;
                    if pl::gt(&value_ptr, &n(0x10000)) {
                        continue 'ifd_try;
                    }
                }
            }
            dir_info.set("DirStart", pl::add(&dir_info.get("DirStart"), &offset)); // update directory start
            dir_info.set("DirLen", pl::sub(&dir_info.get("DirLen"), &offset));
            return Ok(offset); // success!!
        }
    }
    Ok(Value::Undef)
}

//------------------------------------------------------------------------------
/// Fix base offset for Leica maker notes
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success, and updates $dirInfo if necessary for new directory
pub fn fix_leica_base(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    ret(fix_leica_base_rs(it, &et, &dir_info, &tag_table_ptr)?)
}

fn fix_leica_base_rs(it: &mut Interp, et: &Value, dir_info: &Value, tag_table_ptr: &Value) -> ER<Value> {
    let data_pt = dir_info.get("DataPt");
    let dir_start = or(dir_info.get("DirStart"), n(0));
    // get hash of value block positions
    let (val_block, _val_blk_adj) = get_value_blocks_rs(it, &data_pt, &dir_start, &Value::Undef)?;
    if !val_block.is_empty() {
        // get sorted list of value offsets
        let val_ptrs = sorted_num_keys(&val_block);
        let num_entries = get16u(it, &data_pt, dir_start.clone())?;
        let diff = pl::sub(&val_ptrs[0], &pl::add(&pl::mul(&num_entries, &n(12)), &n(4)));
        if pl::gt(&diff, &n(8)) {
            dir_info.set("Base", pl::sub(&dir_info.get("Base"), &n(8)));
            dir_info.set("DataPos", pl::add(&dir_info.get("DataPos"), &n(8)));
        }
    }
    let mut success = n(1);
    if tag_table_ptr.truthy() {
        success = pl::call(it, "Image::ExifTool::Exif::ProcessExif", vec![et.clone(), dir_info.clone(), tag_table_ptr.clone()])?;
    }
    Ok(success)
}

//------------------------------------------------------------------------------
/// Process Canon maker notes
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_canon(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    // identify Canon MakerNote footer in HtmlDump
    // (this code moved from FixBase so it also works for Adobe MakN in DNG images)
    if et.get("HTML_DUMP").truthy() && pl::gt(&dir_info.get("DirLen"), &n(8)) {
        let data_pos = dir_info.get("DataPos");
        let dir_start = or(dir_info.get("DirStart"), n(0));
        let footer_pos = pl::sub(&pl::add(&dir_start, &dir_info.get("DirLen")), &n(8));
        let footer = substr(&dir_info.get("DataPt").deref(), footer_pos.to_i64(), Some(8));
        if re(r"^(II\x2a\0|MM\0\x2a)", "").is_match(&footer) && substr(&footer, 0, Some(2)).to_bytes() == get_byte_order(it)?.to_bytes() {
            let old_offset = get32u(it, &new_ref(footer.clone()), n(4))?;
            let new_offset = pl::add(&dir_start, &data_pos);
            let mut str = pl::sprintf("Original maker note offset: 0x%.4x", &[old_offset.clone()]);
            if !pl::num_eq(&old_offset, &new_offset) {
                str = pl::concat(&str, &pl::sprintf("\nCurrent maker note offset: 0x%.4x", &[new_offset]));
            }
            let file_pos = pl::add(&pl::add(&or(dir_info.get("Base"), n(0)), &data_pos), &footer_pos);
            pl::method(it, &et, "HDump", vec![file_pos, n(8), pl::s("[Canon MakerNotes footer]"), str])?;
        }
    }
    // process as normal
    it.call_named("Image::ExifTool::Exif::ProcessExif", vec![et, dir_info, tag_table_ptr], want)
}

//------------------------------------------------------------------------------
/// Process GE type 2 maker notes
/// Inputs: 0) ExifTool object ref, 1) DirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_ge2(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    if !data_pt.truthy() {
        return ret(n(0));
    }
    let dir_start = or(dir_info.get("DirStart"), n(0));

    // these maker notes are missing the IFD entry count, but they
    // always have 25 entries, so write the entry count manually
    pl::call(it, "Image::ExifTool::Set16u", vec![n(25), data_pt, dir_start])?;
    it.call_named("Image::ExifTool::Exif::ProcessExif", vec![et, dir_info, tag_table_ptr], want)
}

//------------------------------------------------------------------------------
/// Process broken Kodak type 8b maker notes
/// Inputs: 0) ExifTool object ref, 1) DirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_kodak_patch(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    if !data_pt.truthy() {
        return ret(n(0));
    }
    let dir_start = or(dir_info.get("DirStart"), n(0));

    // these maker notes have an 2 extra null bytes either before or after the entry count,
    // so fix this by making a 2-byte entry count just before the first IFD entry
    if !pl::ge(&dir_info.get("DirLen"), &n(4)) {
        return ret(n(0));
    }
    let t1 = get16u(it, &data_pt, dir_start.clone())?;
    let t2 = get16u(it, &data_pt, pl::add(&dir_start, &n(2)))?;
    pl::call(it, "Image::ExifTool::Set16u", vec![or(t1, t2), data_pt, pl::add(&dir_start, &n(2))])?;
    dir_info.set("DirStart", pl::add(&dir_info.get("DirStart"), &n(2)));
    it.call_named("Image::ExifTool::Exif::ProcessExif", vec![et, dir_info, tag_table_ptr], want)
}

//------------------------------------------------------------------------------
/// Process unknown maker notes or PreviewImage
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success, and updates $dirInfo if necessary for new directory
pub fn process_unknown_or_preview(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let dir_start = dir_info.get("DirStart");
    let dir_len = dir_info.get("DirLen");
    // check to see if this is a preview image
    if pl::gt(&dir_len, &n(6)) && substr(&data_pt.deref(), dir_start.to_i64(), Some(3)).to_bytes()[..] == b"\xff\xd8\xff"[..] {
        pl::method(it, &et, "VerboseDir", vec![pl::s("PreviewImage")])?;
        if et.get("HTML_DUMP").truthy() {
            let pos = pl::add(&pl::add(&dir_info.get("DataPos"), &dir_info.get("Base")), &dir_start);
            let msg = pl::concat(&pl::concat(&pl::s("Size: "), &dir_len), &pl::s(" bytes"));
            pl::method(it, &et, "HDump", vec![pos, dir_len.clone(), pl::s("(MakerNotes:PreviewImage data)"), msg])?;
        }
        let v = substr(&data_pt.deref(), dir_start.to_i64(), Some(dir_len.to_i64()));
        pl::method(it, &et, "FoundTag", vec![pl::s("PreviewImage"), v])?;
        return ret(n(1));
    }
    ret(process_unknown_rs(it, &et, &dir_info, &tag_table_ptr)?)
}

//------------------------------------------------------------------------------
/// Write unknown maker notes or PreviewImage
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: directory data, '' to delete, or undef on error
pub fn write_unknown_or_preview(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let dir_start = dir_info.get("DirStart");
    let dir_len = dir_info.get("DirLen");
    let new_val;
    // check to see if this is a preview image
    if pl::gt(&dir_len, &n(6)) && substr(&data_pt.deref(), dir_start.to_i64(), Some(3)).to_bytes()[..] == b"\xff\xd8\xff"[..] {
        let extra = match it.global(b'%', "Image::ExifTool::Extra") {
            Binding::Hash(h) => Value::HashRef(h),
            _ => unreachable!(),
        };
        if et.get("NEW_VALUE").get(extra.get("PreviewImage").to_bytes()).truthy() {
            // write or delete new preview (if deleted, it can't currently be added back again)
            new_val = or(pl::method(it, &et, "GetNewValue", vec![pl::s("PreviewImage")])?, pl::s(""));
            if pl::gt(&pl::method(it, &et, "Options", vec![pl::s("Verbose")])?, &n(1)) {
                let old = substr(&data_pt.deref(), dir_start.to_i64(), Some(dir_len.to_i64()));
                pl::method(it, &et, "VerboseValue", vec![pl::s("- MakerNotes:PreviewImage"), old])?;
                if new_val.truthy() {
                    pl::method(it, &et, "VerboseValue", vec![pl::s("+ MakerNotes:PreviewImage"), new_val.clone()])?;
                }
            }
            et.set("CHANGED", pl::add(&et.get("CHANGED"), &n(1)));
        } else {
            new_val = substr(&data_pt.deref(), dir_start.to_i64(), Some(dir_len.to_i64()));
        }
    } else {
        // rewrite MakerNote IFD
        new_val = pl::call(it, "Image::ExifTool::Exif::WriteExif", vec![et, dir_info, tag_table_ptr])?;
    }
    ret(new_val)
}

//------------------------------------------------------------------------------
/// Process unknown maker notes assuming it is in EXIF IFD format
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success, and updates $dirInfo if necessary for new directory
pub fn process_unknown(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    ret(process_unknown_rs(it, &et, &dir_info, &tag_table_ptr)?)
}

fn process_unknown_rs(it: &mut Interp, et: &Value, dir_info: &Value, tag_table_ptr: &Value) -> ER<Value> {
    let mut success = n(0);

    let loc = locate_ifd_rs(it, et, dir_info)?;
    if !loc.is_undef() {
        let bo = get_byte_order(it)?;
        et.set("UnknownByteOrder", bo);
        if pl::gt(&pl::method(it, et, "Options", vec![pl::s("Verbose")])?, &n(1)) {
            let out = pl::method(it, et, "Options", vec![pl::s("TextOut")])?;
            let mut indent = et.get("INDENT");
            re(r"\| $", "").subst(&mut indent, false, |_| b"  ".to_vec());
            let addr = pl::add(&pl::add(&dir_info.get("DirStart"), &dir_info.get("DataPos")), &dir_info.get("Base"));
            let mut fmt = indent.to_bytes().to_vec();
            fmt.extend_from_slice(b"Found IFD at offset 0x%.4x in maker notes:\n");
            print_to(it, Some(out), pl::sprintf_v(&Value::bytes(fmt), &[addr]))?;
        }
        success = pl::call(it, "Image::ExifTool::Exif::ProcessExif", vec![et.clone(), dir_info.clone(), tag_table_ptr.clone()])?;
    } else {
        et.set("UnknownByteOrder", pl::s("")); // indicates we tried but didn't set byte order
        pl::method(it, et, "Warn", vec![pl::concat(&pl::concat(&pl::s("Unrecognized "), &dir_info.get("DirName")), &pl::s("")), n(1)])?;
    }
    Ok(success)
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::MakerNotes::GetMakerNoteOffset", get_maker_note_offset);
    it.define_native("Image::ExifTool::MakerNotes::GetValueBlocks", get_value_blocks);
    it.define_native("Image::ExifTool::MakerNotes::FixBase", fix_base);
    it.define_native("Image::ExifTool::MakerNotes::LocateIFD", locate_ifd);
    it.define_native("Image::ExifTool::MakerNotes::FixLeicaBase", fix_leica_base);
    it.define_native("Image::ExifTool::MakerNotes::ProcessCanon", process_canon);
    it.define_native("Image::ExifTool::MakerNotes::ProcessGE2", process_ge2);
    it.define_native("Image::ExifTool::MakerNotes::ProcessKodakPatch", process_kodak_patch);
    it.define_native("Image::ExifTool::MakerNotes::ProcessUnknownOrPreview", process_unknown_or_preview);
    it.define_native("Image::ExifTool::MakerNotes::WriteUnknownOrPreview", write_unknown_or_preview);
    it.define_native("Image::ExifTool::MakerNotes::ProcessUnknown", process_unknown);
}
