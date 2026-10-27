//! Image::ExifTool::WriteExif (Image/ExifTool/WriteExif.pl), ported sub by sub (see docs/port/PORTING.md).
//!
//! The first section holds the local helpers shared by the ports of the
//! Write*.pl files (WriteExif, WriteIPTC, WritePhotoshop, WriteCanonRaw,
//! WritePDF, WritePostScript): Perl operations the shared helpers do not
//! cover, built on the public Interp API only.

use std::rc::Rc;

use crate::interp::eval::{die, Cell, ER};
use crate::interp::{Interp, Scope, Want};
use crate::modules::exiftool::part2;
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, re, ret};

pub(crate) use crate::modules::random_access::{cell, substr};

// ---- local helpers (shared by the Write*.pl ports) ----

pub(crate) fn n(i: i64) -> Value {
    Value::Int(i)
}

/// The value of a `my` variable kept in a cell.
pub(crate) fn get(c: &Cell) -> Value {
    c.borrow().clone()
}

/// `$var = v` for a `my` variable kept in a cell.
pub(crate) fn put(c: &Cell, v: Value) {
    *c.borrow_mut() = v;
}

/// `\$var` of a `my` variable kept in a cell.
pub(crate) fn rf(c: &Cell) -> Value {
    Value::ScalarRef(c.clone())
}

/// `Image::ExifTool::NAME(ARGS)` in scalar context.
pub(crate) fn etf(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// `$et->Options(NAME)`
pub(crate) fn opt(it: &mut Interp, et: &Value, name: &str) -> ER<Value> {
    pl::method(it, et, "Options", vec![pl::s(name)])
}

/// `$raf->Seek(POS, WHENCE)`
pub(crate) fn seek(it: &mut Interp, raf: &Value, pos: Value, whence: i64) -> ER<bool> {
    Ok(pl::method(it, raf, "Seek", vec![pos, n(whence)])?.truthy())
}

/// `$raf->Read($buff, LEN)`: the number of bytes read.
pub(crate) fn read(it: &mut Interp, raf: &Value, buff: &Cell, len: Value) -> ER<Value> {
    crate::modules::random_access::raf_read_cell(it, raf, buff, len)
}

/// `$raf->Read($buff, LEN) == LEN`
pub(crate) fn read_eq(it: &mut Interp, raf: &Value, buff: &Cell, len: &Value) -> ER<bool> {
    let r = read(it, raf, buff, len.clone())?;
    Ok(pl::num_eq(&r, len))
}

/// `$raf->Tell()`
pub(crate) fn tell(it: &mut Interp, raf: &Value) -> ER<Value> {
    pl::method(it, raf, "Tell", vec![])
}

/// `Write($outfile, LIST)`: true on success.
pub(crate) fn write(it: &mut Interp, outfile: &Value, items: Vec<Value>) -> ER<bool> {
    let mut a = vec![outfile.clone()];
    a.extend(items);
    Ok(pl::call(it, "Image::ExifTool::Write", a)?.truthy())
}

fn sc(r: ER<Vec<Value>>) -> ER<Value> {
    Ok(r?.pop().unwrap_or_default())
}

/// `Get8u($dataPt, $pos)`
pub(crate) fn g8u(it: &mut Interp, pt: &Value, pos: Value) -> ER<Value> {
    sc(part2::get8u(it, vec![pt.clone(), pos], Want::Scalar))
}

/// `Get16u($dataPt, $pos)`
pub(crate) fn g16u(it: &mut Interp, pt: &Value, pos: Value) -> ER<Value> {
    sc(part2::get16u(it, vec![pt.clone(), pos], Want::Scalar))
}

/// `Get32u($dataPt, $pos)`
pub(crate) fn g32u(it: &mut Interp, pt: &Value, pos: Value) -> ER<Value> {
    sc(part2::get32u(it, vec![pt.clone(), pos], Want::Scalar))
}

/// `Set16u($val)`: the packed value.
pub(crate) fn s16u(it: &mut Interp, v: Value) -> ER<Value> {
    sc(part2::set16u(it, vec![v], Want::Scalar))
}

/// `Set32u($val)`: the packed value.
pub(crate) fn s32u(it: &mut Interp, v: Value) -> ER<Value> {
    sc(part2::set32u(it, vec![v], Want::Scalar))
}

/// `Set16u($val, $dataPt, $pos)`
pub(crate) fn s16u_at(it: &mut Interp, v: Value, pt: &Value, pos: Value) -> ER<()> {
    part2::set16u(it, vec![v, pt.clone(), pos], Want::Scalar)?;
    Ok(())
}

/// `Set32u($val, $dataPt, $pos)`
pub(crate) fn s32u_at(it: &mut Interp, v: Value, pt: &Value, pos: Value) -> ER<()> {
    part2::set32u(it, vec![v, pt.clone(), pos], Want::Scalar)?;
    Ok(())
}

/// `GetByteOrder()`
pub(crate) fn byte_order(it: &mut Interp) -> ER<Value> {
    sc(part2::get_byte_order(it, vec![], Want::Scalar))
}

/// `SetByteOrder($order)`
pub(crate) fn set_byte_order(it: &mut Interp, order: Value) -> ER<Value> {
    sc(part2::set_byte_order(it, vec![order], Want::Scalar))
}

/// `ReadValue(ARGS)` in scalar context.
pub(crate) fn read_value(it: &mut Interp, args: Vec<Value>) -> ER<Value> {
    etf(it, "ReadValue", args)
}

/// `WriteValue(ARGS)` in scalar context.
pub(crate) fn write_value(it: &mut Interp, args: Vec<Value>) -> ER<Value> {
    etf(it, "WriteValue", args)
}

/// `++$$h{key}`
pub(crate) fn inc(h: &Value, key: &str) {
    h.set(key, pl::add(&h.get(key), &n(1)));
}

/// `$a eq $b`
pub(crate) fn seq(a: &Value, b: &Value) -> bool {
    a.to_bytes() == b.to_bytes()
}

/// `length $x` as a number (undef is 0).
pub(crate) fn len(v: &Value) -> Value {
    n(v.to_bytes().len() as i64)
}

/// `$a . $b . ...`
pub(crate) fn cat(items: &[Value]) -> Value {
    let mut v = Vec::new();
    for i in items {
        v.extend_from_slice(&i.to_bytes());
    }
    Value::bytes(v)
}

/// `$s x $n`
pub(crate) fn rep(s: &[u8], count: &Value) -> Value {
    let c = count.to_i64().max(0) as usize;
    Value::bytes(s.repeat(c))
}

/// `substr($s, OFF, LEN) = REPL` (or 4-argument substr): the new string.
pub(crate) fn substr_repl(s: &Value, off: &Value, len: Option<&Value>, repl: &Value) -> ER<Value> {
    let b = s.to_bytes();
    let Some((x, y)) = crate::interp::eval::substr_range(b.len(), off.to_i64(), len.map(|l| l.to_i64())) else {
        return die("substr outside of string");
    };
    let mut out = b[..x].to_vec();
    out.extend_from_slice(&repl.to_bytes());
    out.extend_from_slice(&b[y..]);
    Ok(Value::bytes(out))
}

/// `chr($n)` as the interpreter's builtin.
pub(crate) fn chr(v: &Value) -> Value {
    let f = v.to_f64();
    let c = if f < 0.0 || f.is_nan() { 0xFFFD } else { f as u32 };
    if c < 256 {
        Value::bytes([c as u8])
    } else {
        Value::bytes(char::from_u32(c).unwrap_or('\u{FFFD}').to_string().into_bytes())
    }
}

/// `hex($s)` as the interpreter's builtin.
pub(crate) fn hex(v: &Value) -> Value {
    let b = v.to_bytes();
    let mut s: &[u8] = &b;
    if s.starts_with(b"0x") || s.starts_with(b"0X") {
        s = &s[2..];
    } else if s.starts_with(b"x") || s.starts_with(b"X") {
        s = &s[1..];
    }
    let (mut u, mut f, mut overflow) = (0u64, 0f64, false);
    for &c in s {
        if c == b'_' {
            continue;
        }
        let Some(d) = (c as char).to_digit(16) else { break };
        if !overflow {
            match u.checked_mul(16).and_then(|x| x.checked_add(d as u64)) {
                Some(x) => u = x,
                None => {
                    overflow = true;
                    f = u as f64 * 16.0 + d as f64;
                }
            }
        } else {
            f = f * 16.0 + d as f64;
        }
    }
    if overflow {
        Value::Num(f)
    } else {
        ops::from_uv(u)
    }
}

/// `ord($s)`
pub(crate) fn ord(v: &Value) -> Value {
    n(v.to_bytes().first().copied().unwrap_or(0) as i64)
}

/// `sort { $a <=> $b } LIST` (stable, as perl's merge sort).
pub(crate) fn sort_num(v: &mut [Value]) {
    v.sort_by(|a, b| ops::num_cmp(a, b).unwrap_or(std::cmp::Ordering::Equal));
}

/// `sort LIST` (string order).
pub(crate) fn sort_str(v: &mut [Value]) {
    v.sort_by(ops::str_cmp);
}

/// `keys %$h` as values.
pub(crate) fn keys(h: &Value) -> Vec<Value> {
    h.keys().into_iter().map(Value::from).collect()
}

/// `$a & $b`
pub(crate) fn band(a: &Value, b: i64) -> Value {
    ops::bit_and(a, &n(b))
}

/// `print $out STRING`
pub(crate) fn print_out(it: &mut Interp, out: &Value, s: Value) -> ER<()> {
    pl::print_to(it, Some(out), &[s])?;
    Ok(())
}

/// A scope with the file lexicals of a Write*.pl file (`my` variables at
/// its top level), as an eval site in that file sees them.
pub(crate) fn wscope(it: &Interp, module: &str, file: &str) -> Rc<Scope> {
    let scope = Scope::new(None);
    let prefix = format!("{module}::<my:{file}>::");
    for (sigil, name, b) in it.globals_with_prefix(&prefix) {
        scope.declare(sigil, &name[prefix.len()..], b);
    }
    scope
}

/// A file lexical of a Write*.pl file (`my $x` at its top level).
pub(crate) fn wlex_scalar(it: &mut Interp, module: &str, file: &str, name: &str) -> Cell {
    match it.global(b'$', &format!("{module}::<my:{file}>::{name}")) {
        crate::interp::Binding::Scalar(c) => c,
        _ => unreachable!(),
    }
}

/// `%x` file lexical of a Write*.pl file.
pub(crate) fn wlex_hash(it: &mut Interp, module: &str, file: &str, name: &str) -> Value {
    match it.global(b'%', &format!("{module}::<my:{file}>::{name}")) {
        crate::interp::Binding::Hash(h) => Value::HashRef(h),
        _ => unreachable!(),
    }
}

/// `eval $code` at an eval site of a Write*.pl file, with the variables the
/// site has in scope (see `pl::eval_site_in`).
pub(crate) fn eval_at(it: &mut Interp, code: &Value, module: &str, file: &str, vars: &[(&str, Value)], want: Want) -> (Value, Vec<Value>) {
    let scope = wscope(it, module, file);
    pl::eval_site_in(it, code, module, Some(scope), vars, want)
}

/// `$$h{k}` as an array, autovivified (`@{$$h{k}}` in an lvalue context
/// such as grep or foreach).
pub(crate) fn viv_array(h: &Value, key: &str) -> Value {
    h.get_or_new_array(key)
}

const MODULE: &str = "Image::ExifTool::Exif";
const FILE: &str = "WriteExif.pl";

/// `Image::ExifTool::Fixup->new`
fn fixup_new(it: &mut Interp) -> ER<Value> {
    pl::method(it, &pl::s("Image::ExifTool::Fixup"), "new", vec![])
}

/// `$$h{k} += v`
fn add_to(h: &Value, k: &str, v: &Value) {
    h.set(k, pl::add(&h.get(k), v));
}

/// `"@list"` (joined with `$"`)
fn interp_list(it: &mut Interp, items: &[Value]) -> Value {
    let sep = it.get_scalar("main::\"");
    let parts: Vec<Vec<u8>> = items.iter().map(|v| v.to_bytes().to_vec()).collect();
    Value::bytes(parts.join(&sep.to_bytes()[..]))
}

/// Inverse print conversion for OffsetTime tags
/// Inputs: 0) input time zone or date/time value, 1) ExifTool ref
/// Returns: Time zone string for writing to EXIF
pub fn inverse_offset_time(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (mut val, et) = (arg(&a, 0), arg(&a, 1));
    if pl::lc(&val).eq_str("now") {
        val = pl::method(it, &et, "TimeNow", vec![])?;
    }
    if re(r"Z$", "").is_match(&val) {
        return ret(pl::s("+00:00"));
    }
    if let Some(c) = re(r"([-+])(\d{1,2}):?(\d{2})", "").m(&val) {
        return ret(pl::sprintf("%s%.2d:%.2d", &[cap(&c, 1), cap(&c, 2), cap(&c, 3)]));
    }
    pl::ret_undef()
}

/// Inverse print conversion for LensInfo
/// Inputs: 0) lens info string
/// Returns: PrintConvInv of string
pub fn convert_lens_info(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let val = arg(&a, 0);
    let v = pl::call_list(it, "Image::ExifTool::Exif::GetLensInfo", vec![val.clone(), n(1)])?; // (allow unknown "?" values)
    ret(if !v.is_empty() { pl::join(" ", &v) } else { val })
}

/// Get binary CFA Pattern from a text string
/// Inputs: Print-converted CFA pattern (eg. '[Blue,Green][Green,Red]')
/// Returns: CFA pattern as a string of numbers
pub fn get_cfa_pattern(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let val = arg(&a, 0);
    let rows = pl::split(&re(r"\]\s*\[", ""), &val, 0);
    if rows.is_empty() {
        pl::warn(it, "Rows not properly bracketed by '[]'\n")?;
        return pl::ret_undef();
    }
    let mut cols = pl::split(&re(r",", ""), &rows[0], 0);
    if cols.is_empty() {
        pl::warn(it, "Colors not separated by ','\n")?;
        return pl::ret_undef();
    }
    let ny = cols.len();
    let mut av = vec![n(rows.len() as i64), n(cols.len() as i64)];
    let lookup = |s: &[u8]| -> Option<i64> {
        Some(match s {
            b"red" => 0,
            b"green" => 1,
            b"blue" => 2,
            b"cyan" => 3,
            b"magenta" => 4,
            b"yellow" => 5,
            b"white" => 6,
            _ => return None,
        })
    };
    for row in &rows {
        cols = pl::split(&re(r",", ""), row, 0);
        if cols.len() != ny {
            pl::warn(it, "Inconsistent number of colors in each row\n")?;
            return pl::ret_undef();
        }
        for col in cols.iter_mut() {
            pl::tr(col, " \\]\\[", "", "d"); // remove remaining brackets and any spaces
            let Some(c) = lookup(&pl::lc(col).to_bytes()) else {
                pl::warn(it, &format!("Unknown color '{}'\n", col.to_text()))?;
                return pl::ret_undef();
            };
            av.push(n(c));
        }
    }
    ret(interp_list(it, &av))
}

/// validate raw values for writing
/// Inputs: 0) ExifTool ref, 1) tagInfo hash ref, 2) raw value ref
/// Returns: error string or undef (and possibly changes value) on success
pub fn check_exif(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (_et, tag_info, val_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let mut format = tag_info.get("Format");
    if !format.truthy() {
        format = tag_info.get("Writable");
    }
    if !format.truthy() {
        format = tag_info.get("Table").get("WRITABLE");
    }
    if !format.truthy() || format.eq_str("1") {
        if tag_info.get("Groups").get("0").eq_str("MakerNotes") {
            return pl::ret_undef(); // OK to have no format for makernotes
        } else {
            return ret(pl::s("No writable format"));
        }
    }
    ret(etf(it, "CheckValue", vec![val_ptr, format, tag_info.get("Count")])?)
}

/// encode exif ASCII/Unicode text from UTF8 or Latin
/// Inputs: 0) ExifTool ref, 1) text string
/// Returns: encoded string
/// Note: MUST be called Raw conversion time so the EXIF byte order is known!
pub fn encode_exif_text(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, val) = (arg(&a, 0), arg(&a, 1));
    // does the string contain special characters?
    if re(r"[\x80-\xff]", "").is_match(&val) {
        let order = pl::method(it, &et, "GetNewValue", vec![pl::s("ExifUnicodeByteOrder")])?;
        let e = pl::method(it, &et, "Encode", vec![val, pl::s("UTF16"), order])?;
        ret(pl::concat(&pl::s("UNICODE\0"), &e))
    } else {
        ret(pl::concat(&pl::s("ASCII\0\0\0"), &val))
    }
}

/// rebuild maker notes to properly contain all value data
/// (some manufacturers put value data outside maker notes!!)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: new maker note data (and creates MAKER_NOTE_FIXUP), or undef on error
pub fn rebuild_maker_notes(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let dir_start = dir_info.get("DirStart");
    let dir_len = dir_info.get("DirLen");
    let data_pt = dir_info.get("DataPt");
    let dp = dir_info.get("DataPos");
    let data_pos = if dp.truthy() { dp } else { n(0) };
    let mut rtn_value = Value::Undef;
    let subdir_info = dir_info.hash_copy();

    et.delete("MAKER_NOTE_FIXUP");

    // don't need to rebuild text, BinaryData or PreviewImage maker notes
    let tag_info = dir_info.get("TagInfo");
    let subdir = tag_info.get("SubDirectory");
    let mut proc = subdir.get("ProcessProc");
    if !proc.truthy() {
        proc = tag_table_ptr.get("PROCESS_PROC");
    }
    if !proc.truthy() {
        proc = Value::Code(it.sub_ref("Image::ExifTool::Exif::ProcessExif"));
    }
    let process_exif = Value::Code(it.sub_ref("Image::ExifTool::Exif::ProcessExif"));
    let pbd = Value::Code(it.sub_ref("Image::ExifTool::ProcessBinaryData"));
    if (!seq(&proc, &process_exif) && re(r"Text", "").is_match(&tag_info.get("Name")))
        || seq(&proc, &pbd)
        || (tag_info.get("PossiblePreview").truthy() && pl::gt(&dir_len, &n(6)) && substr(it, &data_pt.deref(), &dir_start, Some(&n(3)))?.to_bytes()[..] == b"\xff\xd8\xff"[..])
    {
        return ret(substr(it, &data_pt.deref(), &dir_start, Some(&dir_len))?);
    }
    let save_order = byte_order(it)?;
    let loc = pl::call(it, "Image::ExifTool::MakerNotes::LocateIFD", vec![et.clone(), subdir_info.clone()])?;
    if !loc.is_undef() {
        let maker_fixup = fixup_new(it)?;
        subdir_info.set("Fixup", maker_fixup.clone());
        // create new exiftool object to rewrite the directory without changing it
        let new_tool = pl::method(it, &pl::s("Image::ExifTool"), "new", vec![])?;
        let opts = et.get("OPTIONS");
        pl::method(it, &new_tool, "Options", vec![pl::s("IgnoreMinorErrors"), opts.get("IgnoreMinorErrors"), pl::s("FixBase"), opts.get("FixBase")])?;
        pl::method(it, &new_tool, "Init", vec![])?; // must do this before calling WriteDirectory()!
        // don't copy over preview image
        pl::method(it, &new_tool, "SetNewValue", vec![pl::s("PreviewImage"), pl::s("")])?;
        // copy all transient members over in case they are used for writing
        // (Make, Model, etc)
        for k in et.keys() {
            if k.iter().any(|c| c.is_ascii_lowercase()) {
                new_tool.set(&k, et.get(&k));
            }
        }
        // fix base offsets if specified
        let fb = opt(it, &et, "FixBase")?;
        pl::method(it, &new_tool, "Options", vec![pl::s("FixBase"), fb])?;
        // set GENERATE_PREVIEW_INFO flag so PREVIEW_INFO will be generated
        new_tool.set("GENERATE_PREVIEW_INFO", n(1));
        // drop any large tags
        new_tool.set("DropTags", n(1));
        // initialize other necessary data members
        new_tool.set("FILE_TYPE", et.get("FILE_TYPE"));
        new_tool.set("TIFF_TYPE", et.get("TIFF_TYPE"));
        // rewrite maker notes
        rtn_value = pl::method(it, &new_tool, "WriteDirectory", vec![subdir_info.clone(), tag_table_ptr.clone()])?;
        if !rtn_value.is_undef() && len(&rtn_value).truthy() {
            let rv = cell(rtn_value.clone());
            // add the dummy/empty preview image if necessary
            if new_tool.get("PREVIEW_INFO").truthy() {
                let l = len(&get(&rv));
                pl::method(it, &maker_fixup, "SetMarkerPointers", vec![rf(&rv), pl::s("PreviewImage"), l])?;
                put(&rv, pl::concat(&get(&rv), &new_tool.get("PREVIEW_INFO").get("Data")));
                new_tool.delete("PREVIEW_INFO");
            }
            // add makernote header
            if loc.truthy() {
                let mut hdr = substr(it, &data_pt.deref(), &dir_start, Some(&loc))?;
                // special case: convert Pentax/Samsung DNG maker notes to JPEG style
                // (in JPEG, Pentax makernotes are absolute and start with "AOC\0" for some
                // models, but in DNG images they are stored in tag 0xc634 of IFD0 and
                // start with either "PENTAX \0" or "SAMSUNG\0")
                if dir_info.get("Parent").eq_str("IFD0") && re(r"^(PENTAX |SAMSUNG)\0", "").is_match(&hdr) {
                    // convert to JPEG-style AOC maker notes if used by this model
                    // (Note: this expression also appears in Exif.pm)
                    if re(r"\b(K(-[57mrx]|(10|20|100|110|200)D|2000)|GX(10|20))\b", "").is_match(&et.get("Model")) {
                        re(r"^(PENTAX |SAMSUNG)\0", "").subst(&mut hdr, false, |_| b"AOC\0".to_vec());
                        // save fixup because AOC maker notes have absolute offsets
                        et.set("MAKER_NOTE_FIXUP", maker_fixup.clone());
                    }
                }
                put(&rv, pl::concat(&hdr, &get(&rv)));
                // adjust fixup for shift in start position
                add_to(&maker_fixup, "Start", &len(&hdr));
            }
            // shift offsets according to original position of maker notes,
            // and relative to the makernotes Base
            let sh = pl::sub(&pl::add(&pl::add(&data_pos, &dir_start), &dir_info.get("Base")), &subdir_info.get("Base"));
            add_to(&maker_fixup, "Shift", &sh);
            // repair incorrect offsets if offsets were fixed
            let fixed_by = subdir_info.get("FixedBy");
            add_to(&maker_fixup, "Shift", &if fixed_by.truthy() { fixed_by } else { n(0) });
            // fix up pointers to the specified offset
            pl::method(it, &maker_fixup, "ApplyFixup", vec![rf(&rv)])?;
            // save fixup information unless offsets were relative
            if !subdir_info.get("Relative").truthy() {
                // set shift so offsets are all relative to start of maker notes
                add_to(&maker_fixup, "Shift", &pl::neg(&pl::add(&data_pos, &dir_start)));
                et.set("MAKER_NOTE_FIXUP", maker_fixup.clone()); // save fixup for later
            }
            rtn_value = get(&rv);
        }
    }
    set_byte_order(it, save_order)?;

    ret(rtn_value)
}

/// SortIFD on a data reference.
fn sort_ifd_v(it: &mut Interp, data_pt: &Value, dir_start: &Value, num_entries: &Value, allow_zero: &Value) -> ER<()> {
    let entries = Value::new_hash();
    // split the directory into separate entries
    let mut index = n(0);
    while pl::lt(&index, num_entries) {
        let entry = pl::add(&pl::add(dir_start, &n(2)), &pl::mul(&n(12), &index));
        let mut tag_id = g16u(it, data_pt, entry.clone())?;
        let entry_data = substr(it, &data_pt.deref(), &entry, Some(&n(12)))?;
        // silly software can pad directories with zero entries -- put these at the end
        if !(tag_id.truthy() || pl::num_eq(&index, &n(0)) || allow_zero.truthy()) {
            tag_id = n(0x10000);
        }
        // add new entry (allow for duplicate tag ID's, which shouldn't normally happen)
        let k = tag_id.to_bytes();
        if entries.get(&k).truthy() {
            entries.set(&k, pl::concat(&entries.get(&k), &entry_data));
        } else {
            entries.set(&k, entry_data);
        }
        index = pl::add(&index, &n(1));
    }
    // sort the directory entries
    let mut sorted_tags = keys(&entries);
    sort_num(&mut sorted_tags);
    // generate the sorted IFD
    let mut new_dir = pl::s("");
    for t in sorted_tags {
        new_dir = pl::concat(&new_dir, &entries.get(t.to_bytes()));
    }
    // replace original directory with new, sorted one
    let v = substr_repl(&data_pt.deref(), &pl::add(dir_start, &n(2)), Some(&pl::mul(&n(12), num_entries)), &new_dir)?;
    pl::set_deref(data_pt, v)?;
    Ok(())
}

/// Sort IFD directory entries
/// Inputs: 0) data reference, 1) directory start, 2) number of entries,
///         3) flag to treat 0 as a valid tag ID (as opposed to an empty IFD entry)
pub fn sort_ifd(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    sort_ifd_v(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2), &arg(&a, 3))?;
    ret(Value::Undef)
}

/// ValidateIFD on values.
fn validate_ifd_v(it: &mut Interp, dir_info: &Value, dir_start: Value) -> ER<bool> {
    let raf = dir_info.get("RAF");
    if !raf.truthy() {
        return Ok(false);
    }
    let base = dir_info.get("Base");
    let dir_start = if dir_start.is_undef() {
        let d = dir_info.get("DirStart");
        if d.truthy() { d } else { n(0) }
    } else {
        dir_start
    };
    let dp = dir_info.get("DataPos");
    let offset = pl::add(&dir_start, &if dp.truthy() { dp } else { n(0) });
    let buff = cell(Value::Undef);
    if !(seek(it, &raf, pl::add(&offset, &base), 0)? && read_eq(it, &raf, &buff, &n(2))?) {
        return Ok(false);
    }
    let num_entries = g16u(it, &rf(&buff), n(0))?;
    if !(pl::gt(&num_entries, &n(1)) && pl::lt(&num_entries, &n(64))) {
        return Ok(false);
    }
    let l = pl::mul(&n(12), &num_entries);
    if !read_eq(it, &raf, &buff, &l)? {
        return Ok(false);
    }
    let mut last_id = n(-1);
    let mut index = n(0);
    while pl::lt(&index, &num_entries) {
        let entry = pl::mul(&n(12), &index);
        let tag_id = g16u(it, &rf(&buff), entry.clone())?;
        if !(pl::gt(&tag_id, &last_id) || dir_info.get("AllowOutOfOrderTags").truthy()) {
            return Ok(false);
        }
        let format = g16u(it, &rf(&buff), pl::add(&entry, &n(2)))?;
        if !(pl::gt(&format, &n(0)) && pl::le(&format, &n(13))) {
            return Ok(false);
        }
        let count = g32u(it, &rf(&buff), pl::add(&entry, &n(4)))?;
        if !pl::gt(&count, &n(0)) {
            return Ok(false);
        }
        last_id = tag_id;
        index = pl::add(&index, &n(1));
    }
    Ok(true)
}

/// Validate IFD entries (strict validation to test possible chained IFD's)
/// Inputs: 0) dirInfo ref (must have RAF set), 1) optional DirStart
/// Returns: true if IFD looks OK
pub fn validate_ifd(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let ok = validate_ifd_v(it, &arg(&a, 0), arg(&a, 1))?;
    ret(n(ok as i64))
}

/// GetOffList on values: the offset list and the offset hash.
fn get_off_list_v(it: &mut Interp, data_pt: &Value, dir_start: &Value, data_pos: &Value, num_entries: &Value, tag_table_ptr: &Value) -> ER<(Value, Value)> {
    let format_size = pl::pkg_array(it, "Image::ExifTool::Exif::formatSize");
    let ifd_end = pl::add(&pl::add(&pl::add(dir_start, &n(2)), &pl::mul(&n(12), num_entries)), data_pos);
    let off_hash = Value::new_hash();
    let mut index = n(0);
    while pl::lt(&index, num_entries) {
        let entry = pl::add(&pl::add(dir_start, &n(2)), &pl::mul(&n(12), &index));
        index = pl::add(&index, &n(1));
        let format = g16u(it, data_pt, pl::add(&entry, &n(2)))?;
        if pl::lt(&format, &n(1)) || pl::gt(&format, &n(13)) {
            continue;
        }
        let count = g16u(it, data_pt, pl::add(&entry, &n(4)))?;
        let size = pl::mul(&format_size.at(format.to_i64()), &count);
        if pl::le(&size, &n(4)) {
            let tag_id = g16u(it, data_pt, entry.clone())?;
            let ti = tag_table_ptr.get(tag_id.to_bytes());
            if !(pl::ref_of(&ti) == "HASH" && ti.get("FixCount").truthy()) {
                continue;
            }
        }
        let offset = g16u(it, data_pt, pl::add(&entry, &n(8)))?;
        if pl::ge(&offset, &ifd_end) {
            off_hash.set(offset.to_bytes(), n(1));
        }
    }
    // set offset hash values to indices in list
    let mut off_list = keys(&off_hash);
    sort_str(&mut off_list);
    for (i, offset) in off_list.iter().enumerate() {
        off_hash.set(offset.to_bytes(), n(i as i64));
    }
    Ok((Value::new_array(off_list), off_hash))
}

/// Get sorted list of offsets used in IFD
/// Inputs: 0) data ref, 1) directory start, 2) dataPos, 3) IFD entries, 4) tag table ref
/// Returns: 0) sorted list of offsets (only offsets after the end of the IFD)
///          1) hash of list indices keyed by offset value
/// Notes: This is used in a patch to fix the count for tags in Kodak SubIFD3
pub fn get_off_list(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (l, h) = get_off_list_v(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2), &arg(&a, 3), &arg(&a, 4))?;
    Ok(vec![l, h])
}

fn update_tiff_end_v(et: &Value, end: &Value) {
    if !et.get("TIFF_END").is_undef() && pl::lt(&et.get("TIFF_END"), end) {
        et.set("TIFF_END", end.clone());
    }
}

/// Update TIFF_END member if defined
/// Inputs: 0) ExifTool ref, 1) end of valid TIFF data
pub fn update_tiff_end(_it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    update_tiff_end_v(&arg(&a, 0), &arg(&a, 1));
    pl::ret_nothing(want)
}

fn validate_image_data_v(it: &mut Interp, et: &Value, v_info: &Value, dir_name: &Value, err_flag: &Value) -> ER<()> {
    pl::local_topic(it, |it| {
        let vi = |k: i64| v_info.get(k.to_string());
        // determine the expected size of the image data for an uncompressed image
        // (0x102 BitsPerSample, 0x103 Compression and 0x115 SamplesPerPixel
        //  all default to a value of 1 if they don't exist)
        if (vi(0x103).is_undef() || vi(0x103).eq_str("1")) && vi(0x100).truthy() && vi(0x101).truthy() && (vi(0x117).truthy() || vi(0x145).truthy()) {
            let spp = vi(0x115);
            let samples_per_pix = if spp.truthy() { spp } else { n(1) };
            let mut bits_per_sample = if vi(0x102).truthy() { pl::split_ws(&vi(0x102)) } else { vec![n(1); samples_per_pix.to_i64().max(0) as usize] };
            let byte_count_info = if vi(0x117).truthy() { vi(0x117) } else { vi(0x145) };
            let byte_counts = byte_count_info.at(1);
            let mut total_bytes = n(0);
            for x in pl::split_ws(&byte_counts) {
                it.set_scalar("main::_", x.clone());
                total_bytes = pl::add(&total_bytes, &x);
            }
            let mut minor = Value::Undef;
            if et.get("DOC_NUM").truthy() || !et.get("FILE_TYPE").eq_str("TIFF") {
                minor = n(1);
            }
            if !pl::num_eq(&n(bits_per_sample.len() as i64), &samples_per_pix) {
                if !(et.get("FILE_TYPE").eq_str("EPS") && bits_per_sample.len() == 1) {
                    // (just a warning for this problem)
                    let s = if samples_per_pix.eq_str("1") { "" } else { "s" };
                    let msg = cat(&[dir_name.clone(), pl::s(" BitsPerSample should have "), samples_per_pix.clone(), pl::s(&format!(" value{s}"))]);
                    pl::method(it, et, "Warn", vec![msg, minor.clone()])?;
                }
                while pl::lt(&n(bits_per_sample.len() as i64), &samples_per_pix) {
                    let first = bits_per_sample.first().cloned().unwrap_or_default();
                    bits_per_sample.push(first);
                }
                let first = bits_per_sample.first().cloned().unwrap_or_default();
                for b in &bits_per_sample {
                    it.set_scalar("main::_", b.clone());
                    if !seq(b, &first) {
                        pl::method(it, et, "Warn", vec![pl::concat(dir_name, &pl::s(" BitsPerSample values are different")), minor.clone()])?;
                    }
                    if pl::lt(b, &n(1)) || pl::gt(b, &n(32)) {
                        pl::method(it, et, "Warn", vec![cat(&[pl::s("Invalid "), dir_name.clone(), pl::s(" BitsPerSample value")]), minor.clone()])?;
                    }
                }
            }
            let mut bits_per_pixel = n(0);
            for b in &bits_per_sample {
                it.set_scalar("main::_", b.clone());
                bits_per_pixel = pl::add(&bits_per_pixel, b);
            }
            let expected_bytes = pl::int(&pl::div(&pl::add(&pl::mul(&pl::mul(&vi(0x100), &vi(0x101)), &bits_per_pixel), &n(7)), &n(8))?);
            if !pl::num_eq(&expected_bytes, &total_bytes) &&
                // (this problem seems normal for certain types of RAW files...)
                !re(r"^(K25|KDC|MEF|ORF|SRF)$", "").is_match(&et.get("TIFF_TYPE"))
            {
                let (adj, minor);
                if pl::gt(&expected_bytes, &total_bytes) {
                    adj = "Under"; // undersized is a bigger problem because we may lose data
                    minor = if err_flag.truthy() { Value::Undef } else { n(0) };
                } else {
                    adj = "Over";
                    minor = n(1);
                }
                let msg = cat(&[pl::s(adj), pl::s("sized "), dir_name.clone(), pl::s(" "), byte_count_info.at(0).get("Name"), pl::s(" ("), total_bytes.clone(), pl::s(" bytes, but expected "), expected_bytes.clone(), pl::s(")")]);
                if minor.is_undef() {
                    // this is a serious error if we are writing the file and there
                    // is a chance that we may not copy all of the image data
                    // (but make it minor to allow the file to be written anyway)
                    pl::method(it, et, "Error", vec![msg, n(1)])?;
                } else {
                    pl::method(it, et, "Warn", vec![msg, minor])?;
                }
            }
        }
        Ok(())
    })
}

/// Validate image data size
/// Inputs: 0) ExifTool ref, 1) validate info hash ref,
///         2) flag to issue error (ie. we're writing)
/// - issues warning or error if problems found
pub fn validate_image_data(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    validate_image_data_v(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2), &arg(&a, 3))?;
    pl::ret_nothing(want)
}

/// Add specified image data to ImageDataHash hash
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) lookup for [tagInfo,value] based on tagID
pub fn add_image_data_hash(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, dir_info, offset_info) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));

    let verbose = opt(it, &et, "Verbose")?;
    let _hash = et.get("ImageDataHash");
    let raf = dir_info.get("RAF");

    let mut ids = keys(&offset_info);
    sort_str(&mut ids);
    for tag_id in ids {
        let oi = offset_info.get(tag_id.to_bytes());
        if pl::ref_of(&oi) != "ARRAY" {
            continue; // ignore scalar tag values used for Validate
        }
        let tag_info = oi.at(0);
        if !tag_info.get("IsImageData").truthy() {
            continue; // only consider image data
        }
        let size_id = tag_info.get("OffsetPair");
        let mut sizes;
        if tag_info.get("NotRealPair").truthy() {
            sizes = vec![n(999999999)]; // (Panasonic hack: raw data runs to end of file)
        } else if size_id.truthy() && offset_info.get(size_id.to_bytes()).truthy() {
            sizes = pl::split_ws(&offset_info.get(size_id.to_bytes()).at(1));
        } else {
            continue;
        }
        let offsets = pl::split_ws(&oi.at(1));
        if tag_info.get("NotRealPair").truthy() {
            if sizes.is_empty() {
                sizes.push(Value::Undef);
            }
            sizes[0] = n(999999999);
        }
        let mut total = n(0);
        for offset in offsets {
            let size = if sizes.is_empty() { Value::Undef } else { sizes.remove(0) };
            if !(re(r"^\d+$", "").is_match(&offset) && size.truthy() && re(r"^\d+$", "").is_match(&size) && size.truthy()) {
                continue;
            }
            if !seek(it, &raf, offset, 0)? {
                continue; // (offset is absolute)
            }
            let h = pl::method(it, &et, "ImageDataHash", vec![raf.clone(), size])?;
            total = pl::add(&total, &h);
        }
        if verbose.truthy() {
            let mut name = cat(&[dir_info.get("DirName"), pl::s(":"), tag_info.get("Name")]);
            re(r"Offsets?|Start$", "").subst(&mut name, false, |_| Vec::new());
            let msg = cat(&[et.get("INDENT"), pl::s("(ImageDataHash: "), total, pl::s(" bytes of "), name, pl::s(" data)\n")]);
            pl::method(it, &et, "VPrint", vec![n(0), msg])?;
        }
    }
    pl::ret_nothing(want)
}

fn exif_err_v(it: &mut Interp, et: &Value, err_str: &Value, tag_table_ptr: &Value) -> ER<Value> {
    // MakerNote errors are minor by default
    let mut minor = tag_table_ptr.get("GROUPS").get("0").eq_str("MakerNotes") || et.get("FILE_TYPE").eq_str("MOV");
    if tag_table_ptr.get("VARS").truthy() && tag_table_ptr.get("VARS").get("MINOR_ERRORS").truthy() {
        if minor && pl::method(it, et, "Warn", vec![pl::concat(err_str, &pl::s(". IFD dropped."))])?.truthy() {
            return Ok(pl::s(""));
        }
        minor = true;
    }
    if pl::method(it, et, "Error", vec![err_str.clone(), n(minor as i64)])?.truthy() {
        return Ok(Value::Undef);
    }
    Ok(pl::s(""))
}

/// Handle error while writing EXIF
/// Inputs: 0) ExifTool ref, 1) error string, 2) tag table ref
/// Returns: undef on fatal error, or '' if minor error is ignored
pub fn exif_err(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(exif_err_v(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2))?)
}

/// Read/Write IFD with TIFF-like header (used by DNG 1.2)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: Reading: 1 on success, otherwise returns 0 and sets a Warning
///          Writing: new data block or undef on error
pub fn process_tiff_ifd(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if !et.truthy() {
        return ret(n(1)); // allow dummy access
    }
    let raf = dir_info.get("RAF");
    let b = dir_info.get("Base");
    let base = if b.truthy() { b } else { n(0) };
    let dir_name = dir_info.get("DirName");
    let m = dir_info.get("Subdir").get("Magic");
    let magic = if m.truthy() { m } else { n(0x002a) };
    let buff = cell(Value::Undef);

    // structured with a TIFF-like header and relative offsets
    if !(seek(it, &raf, base.clone(), 0)? && read_eq(it, &raf, &buff, &n(8))?) {
        return ret(n(0));
    }
    let hdr = substr(it, &get(&buff), &n(0), Some(&n(2)))?;
    if !(set_byte_order(it, hdr)?.truthy() && pl::num_eq(&g16u(it, &rf(&buff), n(2))?, &magic)) {
        let msg = cat(&[pl::s("Invalid "), dir_name.clone(), pl::s(" header")]);
        if dir_info.get("IsWriting").truthy() {
            pl::method(it, &et, "Error", vec![msg])?;
            return pl::ret_undef();
        } else {
            pl::method(it, &et, "Warn", vec![msg])?;
            return ret(n(0));
        }
    }
    let offset = g32u(it, &rf(&buff), n(4))?;
    let di = Value::new_hash();
    di.set("DirName", dir_info.get("DirName"));
    di.set("Parent", dir_info.get("Parent"));
    di.set("Base", base.clone());
    di.set("DataPt", rf(&buff));
    di.set("DataLen", len(&get(&buff)));
    di.set("DataPos", n(0));
    di.set("DirStart", offset.clone());
    di.set("DirLen", pl::sub(&len(&get(&buff)), &offset));
    di.set("RAF", raf.clone());
    di.set("NewDataPos", n(8));
    if dir_info.get("IsWriting").truthy() {
        // rewrite the Camera Profile IFD
        let new_dir = write_exif_v(it, &et, &di, &tag_table_ptr)?;
        // don't add header if error writing directory ($newDir is undef)
        // or if directory is being deleted ($newDir is empty)
        if !new_dir.truthy() {
            return ret(new_dir);
        }
        // return directory with TIFF-like header
        let bo = byte_order(it)?;
        return ret(cat(&[bo, s16u(it, magic)?, s32u(it, n(8))?, new_dir]));
    }
    if et.get("HTML_DUMP").truthy() {
        let le = if byte_order(it)?.eq_str("II") { "Little" } else { "Big" };
        let tip = pl::sprintf("Byte order: %s endian\nIdentifier: 0x%.4x\n%s offset: 0x%.4x", &[pl::s(le), magic.clone(), dir_name.clone(), offset.clone()]);
        pl::method(it, &et, "HDump", vec![base, n(8), pl::concat(&dir_name, &pl::s(" header")), tip, n(0)])?;
    }
    ret(pl::call(it, "Image::ExifTool::Exif::ProcessExif", vec![et, di, tag_table_ptr])?)
}

/// `$x || 0`
fn or0(v: Value) -> Value {
    if v.truthy() {
        v
    } else {
        n(0)
    }
}

/// `@$a = LIST`
fn set_items(a: &Value, v: Vec<Value>) {
    if let Value::ArrayRef(r) = a {
        *r.borrow_mut() = v;
    }
}

/// `%$h = ()`
fn clear_hash(h: &Value) {
    if let Value::HashRef(r) = h {
        r.borrow_mut().clear();
    }
}

/// `$$h{k}` read where perl autovivifies `$h` when it is undef (an
/// rvalue element access through an undefined variable).
fn autoviv(h: &mut Value) {
    if h.is_undef() {
        *h = Value::new_hash();
    }
}

/// Write EXIF directory
/// Inputs: 0) ExifTool object ref, 1) source dirInfo ref, 2) tag table ref
/// Returns: Exif data block (may be empty if no Exif data) or undef on error
/// Notes: Increments ExifTool CHANGED flag for each tag changed.  Also updates
///        TIFF_END if defined with location of end of original TIFF image.
/// Returns IFD data in the following order:
///   1. IFD0 directory followed by its data
///   2. SubIFD directory followed by its data, thumbnail and image
///   3. GlobalParameters, EXIF, GPS, Interop IFD's each with their data
///   4. IFD1,IFD2,... directories each followed by their data
///   5. Thumbnail and/or image data for each IFD, with IFD0 image last
pub fn write_exif(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if !et.truthy() {
        return ret(n(1)); // allow dummy access to autoload this package
    }
    ret(write_exif_v(it, &et, &dir_info, &tag_table_ptr)?)
}

// (the Perl `my` variables are declared up front, as in the Perl)
#[allow(unused_assignments)]
fn write_exif_v(it: &mut Interp, et: &Value, dir_info0: &Value, tag_table_ptr: &Value) -> ER<Value> {
    let et = et.clone();
    let tag_table_ptr = tag_table_ptr.clone();
    let orig_dir_info = dir_info0.clone(); // save original dirInfo
    let mut dir_info = dir_info0.clone();
    let mut data_pt = dir_info.get("DataPt");
    if !data_pt.truthy() {
        data_pt = pl::sref(pl::s(""));
    }
    let mut data_pos = or0(dir_info.get("DataPos"));
    let mut dir_start = or0(dir_info.get("DirStart"));
    let dl = dir_info.get("DataLen");
    let mut data_len = if dl.truthy() { dl } else { len(&data_pt.deref()) };
    let dl = dir_info.get("DirLen");
    let mut dir_len = if dl.truthy() { dl } else { pl::sub(&data_len, &dir_start) };
    let mut base = or0(dir_info.get("Base"));
    let first_base = base.clone();
    let mut raf = dir_info.get("RAF");
    let dn = dir_info.get("DirName");
    let mut dir_name = if dn.truthy() { dn } else { pl::s("unknown") };
    let fx = dir_info.get("Fixup");
    let fixup = if fx.truthy() { fx } else { fixup_new(it)? };
    let idf = dir_info.get("ImageData");
    let image_data_flag = if idf.truthy() { idf } else { pl::s("") };
    let verbose = opt(it, &et, "Verbose")?;
    let out = opt(it, &et, "TextOut")?;
    let no_mandatory = opt(it, &et, "NoMandatory")?;
    let mut next_ifd_pos = Value::Undef;
    let offset_data = Value::new_hash();
    let mut in_maker_notes = false;
    let offset_info_list = Value::new_array(vec![]); // @offsetInfo
    let validate_info = Value::new_hash();
    let x_delete = Value::new_hash();
    let mut str_enc = Value::Undef;
    let mut delete_all = n(0);
    let new_data = cell(pl::s("")); // initialize buffer to receive new directory data
    let image_data = Value::new_array(vec![]); // image data blocks to copy later if requested
    let mut name = dir_info.get("Name");
    if !(name.truthy() && dir_name.eq_str("MakerNotes") && !re(r"^MakerNote", "").is_match(&name)) {
        name = dir_name.clone();
    }

    let format_size = pl::pkg_array(it, "Image::ExifTool::Exif::formatSize");
    let format_name = pl::pkg_array(it, "Image::ExifTool::Exif::formatName");
    let format_number = pl::pkg_hash(it, "Image::ExifTool::Exif::formatNumber");
    let int_format = pl::pkg_hash(it, "Image::ExifTool::Exif::intFormat");
    let save_for_validate = pl::pkg_hash(it, "Image::ExifTool::Exif::saveForValidate");
    let exif_main = pl::pkg_hash(it, "Image::ExifTool::Exif::Main");
    let cross_delete = wlex_hash(it, MODULE, FILE, "crossDelete");
    let mandatory_h = wlex_hash(it, MODULE, FILE, "mandatory");
    let fsz = |f: &Value| format_size.at(f.to_i64());
    let fnum = |f: &Value| format_number.get(f.to_bytes());

    // save byte order of existing EXIF
    if dir_name.eq_str("IFD0") || dir_name.eq_str("ExifIFD") {
        let bo = byte_order(it)?;
        et.set("SaveExifByteOrder", bo);
    }

    // set encoding for strings
    if tag_table_ptr.get("GROUPS").get("0").eq_str("EXIF") {
        str_enc = opt(it, &et, "CharsetEXIF")?;
    }

    // allow multiple IFD's in IFD0-IFD1-IFD2... chain
    if re(r"^(IFD0|SubIFD)$", "").is_match(&dir_name) && dir_info.get("Multi").is_undef() {
        dir_info.set("Multi", n(1));
    }
    if tag_table_ptr.get("GROUPS").get("0").eq_str("MakerNotes") {
        in_maker_notes = true;
    }
    let mut ifd = n(0);

    //~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
    // loop through each IFD
    //
    'ifd: loop {
        'ifd_body: {
            // make sure that Compression and SubfileType are defined for this IFD (for Condition's)
            et.set("Compression", pl::s(""));
            et.set("SubfileType", pl::s(""));

            // save pointer to start of this IFD within the newData
            let new_start = len(&get(&new_data));
            let mut subdirs: Vec<Value> = Vec::new(); // list of subdirectory data and tag table pointers
            // determine if directory is contained within our data
            let mut must_read = false;
            if pl::lt(&dir_start, &n(0)) || pl::gt(&dir_start, &pl::sub(&data_len, &n(2))) {
                must_read = true;
            } else if pl::ge(&dir_len, &n(2)) {
                let l = pl::add(&n(2), &pl::mul(&n(12), &g16u(it, &data_pt, dir_start.clone())?));
                if pl::gt(&pl::add(&dir_start, &l), &data_len) {
                    must_read = true;
                }
            }
            // read IFD from file if necessary
            if must_read {
                if raf.truthy() {
                    // read the count of entries in this IFD
                    let offset = pl::add(&dir_start, &data_pos);
                    let (buff, buf2) = (cell(Value::Undef), cell(Value::Undef));
                    if !(seek(it, &raf, pl::add(&offset, &base), 0)? && read_eq(it, &raf, &buff, &n(2))?) {
                        return exif_err_v(it, &et, &pl::concat(&pl::s("Bad IFD or truncated file in "), &name), &tag_table_ptr);
                    }
                    let mut l = pl::mul(&n(12), &g16u(it, &rf(&buff), n(0))?);
                    // (also read next IFD pointer if available)
                    if !pl::ge(&read(it, &raf, &buf2, pl::add(&l, &n(4)))?, &l) {
                        return exif_err_v(it, &et, &pl::concat(&pl::s("Error reading "), &name), &tag_table_ptr);
                    }
                    put(&buff, pl::concat(&get(&buff), &get(&buf2)));
                    // make copy of dirInfo since we're going to modify it
                    dir_info = dir_info.hash_copy();
                    // update directory parameters for the newly loaded IFD
                    data_pt = rf(&buff);
                    dir_info.set("DataPt", data_pt.clone());
                    dir_start = n(0);
                    dir_info.set("DirStart", n(0));
                    data_pos = offset.clone();
                    dir_info.set("DataPos", offset.clone());
                    data_len = len(&get(&buff));
                    dir_info.set("DataLen", data_len.clone());
                    dir_len = data_len.clone();
                    dir_info.set("DirLen", data_len.clone());
                    // only account for nextIFD pointer if we are going to use it
                    if pl::num_eq(&data_len, &pl::add(&l, &n(6))) && (dir_info.get("Multi").truthy() || re(r"\0{4}$", "").is_match(&get(&buff))) {
                        l = pl::add(&l, &n(4));
                    }
                    update_tiff_end_v(&et, &pl::add(&pl::add(&pl::add(&offset, &base), &n(2)), &l));
                } else if dir_len.truthy() && pl::ge(&pl::add(&dir_start, &n(4)), &data_len) {
                    // error if we can't load IFD (unless we are creating
                    // from scratch, in which case dirLen will be zero)
                    let s = if opt(it, &et, "IgnoreMinorErrors")?.truthy() { "Deleted bad" } else { "Bad" };
                    pl::method(it, &et, "Error", vec![cat(&[pl::s(s), pl::s(" "), name.clone(), pl::s(" directory")]), n(1)])?;
                }
            }
            let mut index;
            let mut dir_end;
            let mut num_entries;
            let has_old_id = Value::new_hash();
            let mut unsorted = false;
            if pl::lt(&pl::add(&dir_start, &n(4)), &data_len) {
                num_entries = g16u(it, &data_pt, dir_start.clone())?;
                dir_end = pl::add(&pl::add(&dir_start, &n(2)), &pl::mul(&n(12), &num_entries));
                if pl::gt(&dir_end, &data_len) {
                    let nn = pl::int(&pl::div(&pl::sub(&pl::sub(&data_len, &dir_start), &n(2)), &n(12))?);
                    let rtn = exif_err_v(it, &et, &cat(&[pl::s("Truncated "), name.clone(), pl::s(" directory")]), &tag_table_ptr)?;
                    if !(nn.truthy() && !rtn.is_undef()) {
                        return Ok(Value::Undef);
                    }
                    num_entries = nn; // continue processing the entries we have
                }
                // create lookup for existing tag ID's and determine if directory is sorted
                let mut last_id = n(-1);
                index = n(0);
                while pl::lt(&index, &num_entries) {
                    let tag_id = g16u(it, &data_pt, pl::add(&pl::add(&dir_start, &n(2)), &pl::mul(&n(12), &index)))?;
                    has_old_id.set(tag_id.to_bytes(), n(1));
                    // check for proper sequence (but ignore null entries at end)
                    if pl::lt(&tag_id, &last_id) && (tag_id.truthy() || tag_table_ptr.get("0").truthy()) {
                        unsorted = true;
                    }
                    last_id = tag_id;
                    index = pl::add(&index, &n(1));
                }
                // sort entries if out-of-order (but not in maker notes IFDs or RAW files)
                if unsorted && !(in_maker_notes || pl::method(it, &et, "IsRawType", vec![])?.truthy()) {
                    sort_ifd_v(it, &data_pt, &dir_start, &num_entries, &tag_table_ptr.get("0"))?;
                    pl::method(it, &et, "Warn", vec![cat(&[pl::s("Entries in "), name.clone(), pl::s(" were out of sequence. Fixed.")]), n(1)])?;
                    unsorted = false;
                }
            } else {
                num_entries = n(0);
                dir_end = dir_start.clone();
            }

            // loop through new values and accumulate all information for this IFD
            let set = Value::new_hash();
            let may_delete = Value::new_hash();
            let has_new_id = Value::new_hash();
            let wrong_dir = cross_delete.get(dir_name.to_bytes());
            let new_tag_info = pl::method_list(it, &et, "GetNewTagInfoList", vec![tag_table_ptr.clone()])?;
            for ti in &new_tag_info {
                let tag_id = ti.get("TagID");
                has_new_id.set(tag_id.to_bytes(), n(1));
                // must evaluate Condition later when we have all DataMember's available
                let v = if pl::ref_of(&tag_table_ptr.get(tag_id.to_bytes())) == "ARRAY" || ti.get("Condition").truthy() { pl::s("") } else { ti.clone() };
                set.set(tag_id.to_bytes(), v);
            }

            // fix base offsets (some cameras incorrectly write maker notes in IFD0)
            if dir_name.eq_str("MakerNotes")
                && re(r"^(ExifIFD|IFD0)$", "").is_match(&dir_info.get("Parent"))
                && !re(r"^(ARW|SR2)$", "").is_match(&et.get("TIFF_TYPE"))
                && !et.get("LeicaTrailerPos").truthy()
                && pl::call(it, "Image::ExifTool::MakerNotes::FixBase", vec![et.clone(), dir_info.clone()])?.truthy()
            {
                // update local variables from fixed values
                base = dir_info.get("Base");
                data_pos = dir_info.get("DataPos");
                // changed if ForceWrite tag was was set to "FixBase"
                if et.get("FORCE_WRITE").get("FixBase").truthy() {
                    inc(&et, "CHANGED");
                }
                if et.get("TIFF_TYPE").eq_str("SRW") && et.get("Make").eq_str("SAMSUNG") && et.get("Model").eq_str("EK-GN120") {
                    pl::method(it, &et, "Error", vec![pl::s("EK-GN120 SRW files are too buggy to write")])?;
                }
            }

            // initialize variables to handle mandatory tags
            let (mut mandatory, mut all_mandatory, mut add_mandatory) = (Value::Undef, Value::Undef, Value::Undef);
            if !no_mandatory.truthy() {
                mandatory = mandatory_h.get(dir_name.to_bytes());
            }
            if mandatory.truthy() {
                // use X/Y resolution values from JFIF if available
                if dir_name.eq_str("IFD0") && !et.get("JFIFYResolution").is_undef() {
                    let ifd0_vals = mandatory.hash_copy();
                    ifd0_vals.set("282", et.get("JFIFXResolution"));
                    ifd0_vals.set("283", et.get("JFIFYResolution"));
                    ifd0_vals.set("296", pl::add(&et.get("JFIFResolutionUnit"), &n(1)));
                    mandatory = ifd0_vals;
                }
                all_mandatory = n(0); // initialize to zero
                add_mandatory = n(0);
                // add mandatory tags if creating a new directory
                if !num_entries.truthy() {
                    for k in mandatory.keys() {
                        if set.get(&k).is_undef() {
                            set.set(&k, tag_table_ptr.get(&k));
                        }
                    }
                }
            } else {
                delete_all = Value::Undef; // don't remove directory (no mandatory entries)
            }
            let add_dirs;
            let mut new_tags: Vec<Value> = Vec::new();
            if in_maker_notes {
                add_dirs = Value::new_hash(); // can't currently add new directories in MakerNotes
                // allow non-permanent makernotes tags to be added
                // (note: we may get into trouble if there are too many of these
                //  because we allow out-of-order tags in MakerNote IFD's but our
                //  logic to add new tags relies on ordered entries)
                for k in set.keys() {
                    let s = set.get(&k);
                    if !s.truthy() {
                        continue;
                    }
                    let perm = s.get("Permanent");
                    if !perm.is_undef() && !perm.truthy() {
                        new_tags.push(Value::from(k.clone()));
                    }
                }
                if new_tags.len() > 1 {
                    sort_num(&mut new_tags);
                }
            } else {
                // get a hash of directories we will be writing in this one
                add_dirs = pl::method(it, &et, "GetAddDirHash", vec![tag_table_ptr.clone(), dir_name.clone()])?;
                // make a union of tags & dirs (can set whole dirs, like MakerNotes)
                let all_tags = set.hash_copy();
                for k in add_dirs.keys() {
                    all_tags.set(&k, add_dirs.get(&k));
                }
                // make sorted list of new tags to be added
                new_tags = keys(&all_tags);
                sort_num(&mut new_tags);
            }
            let dir_buff = cell(pl::s("")); // buffer for directory data
            let mut val_buff = pl::s(""); // buffer for value data
            let mut val_fixups: Vec<Value> = Vec::new(); // list of fixups for offsets in valBuff
            // fixup for offsets in dirBuff
            let mut dir_fixup = fixup_new(it)?;
            let mut entry_based_fixup = Value::Undef;
            let mut last_tag_id = n(-1);
            let (mut old_info, mut old_format, mut old_form_name, mut old_count, mut old_size, mut old_image_data) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef, Value::Undef, Value::Undef);
            let old_value = cell(Value::Undef);
            let (mut read_format, mut read_form_name, mut read_count) = (Value::Undef, Value::Undef, Value::Undef); // format for reading old value(s)
            let (mut entry, mut value_data_pt, mut value_data_pos, mut value_data_len, mut value_ptr, mut val_end) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef, Value::Undef, Value::Undef);
            let (mut off_list, mut off_hash, mut ignore_count, mut fix_count) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef);
            let mut old_id = n(-1);
            let mut new_id = n(-1);

            // patch for Canon EOS 40D firmware 1.0.4 bug (incorrect directory counts)
            if in_maker_notes && et.get("Model").eq_str("Canon EOS 40D") {
                let p = pl::add(&pl::add(&pl::add(&dir_start, &n(2)), &pl::mul(&n(12), &pl::sub(&num_entries, &n(1)))), &n(2));
                let fmt = g16u(it, &data_pt, p)?;
                if pl::lt(&fmt, &n(1)) || pl::gt(&fmt, &n(13)) {
                    // adjust the number of directory entries
                    num_entries = pl::sub(&num_entries, &n(1));
                    dir_end = pl::sub(&dir_end, &n(12));
                    ignore_count = n(1);
                }
            }
            //..............................................................................
            // loop through entries in new directory
            //
            index = n(0);
            'entry: loop {
                if !old_id.is_undef() && pl::num_eq(&old_id, &new_id) {
                    //
                    // read next entry from existing directory
                    //
                    if pl::lt(&index, &num_entries) {
                        entry = pl::add(&pl::add(&dir_start, &n(2)), &pl::mul(&n(12), &index));
                        old_id = g16u(it, &data_pt, entry.clone())?;
                        old_format = g16u(it, &data_pt, pl::add(&entry, &n(2)))?;
                        read_format = old_format.clone();
                        old_count = g32u(it, &data_pt, pl::add(&entry, &n(4)))?;
                        read_count = old_count.clone();
                        old_image_data = Value::Undef;
                        if (pl::lt(&old_format, &n(1)) || pl::gt(&old_format, &n(13))) && !pl::num_eq(&old_format, &n(129)) && !(pl::num_eq(&old_format, &n(16)) && et.get("Make").eq_str("Apple") && in_maker_notes) {
                            let msg = cat(&[pl::s("Bad format ("), old_format.clone(), pl::s(") for "), name.clone(), pl::s(" entry "), index.clone()]);
                            // patch to preserve invalid directory entries in SubIFD3 of
                            // various Kodak Z-series cameras (Z812, Z1085IS, Z1275)
                            // and some Sony cameras such as the DSC-P10
                            if dir_name.eq_str("MakerNotes")
                                && ((re(r"KODAK", "i").is_match(&et.get("Make")) && dir_info.get("Name").truthy() && dir_info.get("Name").eq_str("SubIFD3"))
                                    || (pl::num_eq(&num_entries, &n(12)) && et.get("Make").eq_str("SONY") && pl::ge(&index, &n(8))))
                            {
                                let e = substr(it, &data_pt.deref(), &entry, Some(&n(12)))?;
                                put(&dir_buff, pl::concat(&get(&dir_buff), &e));
                                index = pl::add(&index, &n(1));
                                new_id = old_id.clone(); // we wrote this
                                pl::method(it, &et, "Warn", vec![msg, n(1)])?;
                                continue 'entry;
                            }
                            // don't write out null directory entry
                            if pl::num_eq(&old_format, &n(0)) && index.truthy() && pl::num_eq(&old_count, &n(0)) {
                                ignore_count = pl::add(&or0(ignore_count.clone()), &n(1));
                                // must keep same directory size to avoid messing up our fixed offsets
                                if dir_info.get("FixBase").truthy() {
                                    put(&dir_buff, pl::concat(&get(&dir_buff), &rep(b"\0", &n(12))));
                                }
                                index = pl::add(&index, &n(1));
                                new_id = old_id.clone(); // pretend we wrote this
                                continue 'entry;
                            }
                            return exif_err_v(it, &et, &msg, &tag_table_ptr);
                        }
                        old_form_name = format_name.at(old_format.to_i64());
                        read_form_name = old_form_name.clone();
                        value_data_pt = data_pt.clone();
                        value_data_pos = data_pos.clone();
                        value_data_len = data_len.clone();
                        value_ptr = pl::add(&entry, &n(8));
                        // try direct method first for speed
                        old_info = tag_table_ptr.get(old_id.to_bytes());
                        if pl::ref_of(&old_info) != "HASH" || old_info.get("Condition").truthy() {
                            // must get unknown tags too
                            // (necessary so we don't miss a tag we want to Drop)
                            let unk = pl::method(it, &et, "Options", vec![pl::s("Unknown"), n(1)])?;
                            old_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), old_id.clone()])?;
                            pl::method(it, &et, "Options", vec![pl::s("Unknown"), unk])?;
                        }
                        // patch incorrect count in Kodak SubIFD3 tags
                        if pl::lt(&old_count, &n(2)) && old_info.truthy() && old_info.get("FixCount").truthy() {
                            if !off_list.truthy() {
                                let (l, h) = get_off_list_v(it, &data_pt, &dir_start, &data_pos, &num_entries, &tag_table_ptr)?;
                                off_list = l;
                                off_hash = h;
                            }
                            let i = off_hash.get(g32u(it, &data_pt, value_ptr.clone())?.to_bytes());
                            if !i.is_undef() && pl::lt(&i, &n(off_list.len() as i64 - 1)) {
                                let d = pl::sub(&off_list.at(i.to_i64() + 1), &off_list.at(i.to_i64()));
                                old_count = pl::int(&pl::div(&d, &fsz(&old_format))?);
                                if !pl::num_eq(&old_count, &read_count) {
                                    fix_count = pl::add(&or0(fix_count.clone()), &n(1));
                                }
                            }
                        }
                        old_size = pl::mul(&old_count, &fsz(&old_format));
                        let mut read_from_file = false;
                        if pl::gt(&old_size, &n(4)) {
                            value_ptr = g32u(it, &data_pt, value_ptr.clone())?;
                            // fix valuePtr if necessary
                            if dir_info.get("FixOffsets").truthy() {
                                if !val_end.truthy() {
                                    val_end = pl::add(&pl::add(&pl::add(&pl::add(&data_pos, &dir_start), &n(2)), &pl::mul(&n(12), &num_entries)), &n(4));
                                }
                                let (tag_id, size, w_flag) = (old_id.clone(), old_size.clone(), n(1));
                                //### eval FixOffsets ($valuePtr, $valEnd, $size, $tagID, $wFlag)
                                let vars = [("valuePtr", value_ptr.clone()), ("valEnd", val_end.clone()), ("size", size), ("tagID", tag_id), ("wFlag", w_flag), ("et", et.clone()), ("dirInfo", dir_info.clone())];
                                let (_, after) = eval_at(it, &dir_info.get("FixOffsets"), MODULE, FILE, &vars, Want::Scalar);
                                value_ptr = after[0].clone();
                                val_end = after[1].clone();
                                if value_ptr.is_undef() {
                                    if !et.get("DropTags").truthy() {
                                        let tag_str = if old_info.truthy() { old_info.get("Name") } else { pl::sprintf("tag 0x%.4x", &[old_id.clone()]) };
                                        if pl::method(it, &et, "Error", vec![cat(&[pl::s("Bad "), name.clone(), pl::s(" offset for "), tag_str]), n(in_maker_notes as i64)])?.truthy() {
                                            return Ok(Value::Undef);
                                        }
                                    }
                                    index = pl::add(&index, &n(1));
                                    old_id = new_id.clone();
                                    continue 'entry; // drop this tag
                                }
                            }
                            // offset shouldn't point into TIFF or IFD header
                            let mut suspect = pl::lt(&value_ptr, &n(8));
                            // convert offset to pointer in $$dataPt
                            let tt = tag_table_ptr.get(old_id.to_bytes());
                            if dir_info.get("EntryBased").truthy() || (pl::ref_of(&tt) == "HASH" && tt.get("EntryBased").truthy()) {
                                value_ptr = pl::add(&value_ptr, &entry);
                            } else {
                                value_ptr = pl::sub(&value_ptr, &data_pos);
                            }
                            // value shouldn't overlap our directory
                            if pl::lt(&value_ptr, &dir_end) && pl::gt(&pl::add(&value_ptr, &old_size), &dir_start) {
                                suspect = true;
                            }
                            // get value by seeking in file if we are allowed
                            if pl::lt(&value_ptr, &n(0)) || pl::gt(&pl::add(&value_ptr, &old_size), &data_len) {
                                let (mut pos, mut tag_str, mut invalid_preview, mut tmp_info, mut leica_trailer) = (Value::Undef, Value::Undef, n(0), Value::Undef, Value::Undef);
                                if old_info.truthy() {
                                    tag_str = old_info.get("Name");
                                    leica_trailer = old_info.get("LeicaTrailer");
                                } else if !old_info.is_undef() {
                                    tmp_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), old_id.clone(), pl::sref(pl::s("")), old_form_name.clone(), old_count.clone()])?;
                                    if tmp_info.truthy() {
                                        tag_str = tmp_info.get("Name");
                                        leica_trailer = tmp_info.get("LeicaTrailer");
                                    }
                                }
                                if !tag_str.truthy() {
                                    tag_str = pl::sprintf("tag 0x%.4x", &[old_id.clone()]);
                                }
                                // allow PreviewImage to run outside EXIF segment in JPEG images
                                if !raf.truthy() {
                                    if tag_str.eq_str("PreviewImage") {
                                        raf = et.get("RAF");
                                        if raf.truthy() {
                                            pos = tell(it, &raf)?;
                                            if old_info.truthy() && old_info.get("ChangeBase").truthy() {
                                                // adjust base offset for this tag only
                                                //### eval ChangeBase ($dirStart,$dataPos)
                                                let vars = [("dirStart", dir_start.clone()), ("dataPos", data_pos.clone()), ("et", et.clone()), ("dirInfo", dir_info.clone())];
                                                let (new_base, _) = eval_at(it, &old_info.get("ChangeBase"), MODULE, FILE, &vars, Want::Scalar);
                                                value_ptr = pl::add(&value_ptr, &new_base);
                                            }
                                        } else {
                                            invalid_preview = n(1);
                                        }
                                    } else if leica_trailer.truthy() {
                                        // save information about Leica makernote trailer
                                        let lt = Value::new_hash();
                                        lt.set("TagInfo", if old_info.truthy() { old_info.clone() } else { tmp_info.clone() });
                                        lt.set("Offset", pl::add(&pl::add(&base, &value_ptr), &data_pos));
                                        lt.set("Size", old_size.clone());
                                        lt.set("Fixup", fixup_new(it)?);
                                        et.set("LeicaTrailer", lt);
                                        invalid_preview = n(2);
                                        // remove SubDirectory to prevent processing (for now)
                                        let copy = if old_info.truthy() { old_info.hash_copy() } else { tmp_info.hash_copy() };
                                        copy.delete("SubDirectory");
                                        copy.delete("MakerNotes");
                                        old_info = copy;
                                    }
                                }
                                let bdl = pl::call(it, "Image::ExifTool::Exif::BINARY_DATA_LIMIT", vec![])?;
                                if pl::gt(&old_size, &bdl)
                                    && orig_dir_info.get("ImageData").truthy()
                                    && (old_info.is_undef() || (old_info.truthy() && (!old_info.get("SubDirectory").truthy() || old_info.get("ReadFromRAF").truthy())))
                                {
                                    // copy huge data blocks later instead of loading into memory
                                    put(&old_value, pl::s("")); // dummy empty value
                                    // copy this value later unless writing a new value
                                    if set.get(old_id.to_bytes()).is_undef() {
                                        let pad = if band(&old_size, 0x01).truthy() { n(1) } else { n(0) };
                                        // save block information to copy later (set directory offset later)
                                        old_image_data = Value::new_array(vec![pl::add(&pl::add(&base, &value_ptr), &data_pos), old_size.clone(), pad]);
                                    }
                                } else if raf.truthy() {
                                    let mut success = seek(it, &raf, pl::add(&pl::add(&base, &value_ptr), &data_pos), 0)? && read_eq(it, &raf, &old_value, &old_size)?;
                                    if !pos.is_undef() {
                                        seek(it, &raf, pos.clone(), 0)?;
                                        raf = Value::Undef;
                                        // (sony A700 has 32-byte header on PreviewImage)
                                        if !(success && re(r"^(\xff\xd8\xff|(.|.{33})\xd8\xff\xdb)", "s").is_match(&get(&old_value))) {
                                            invalid_preview = n(1);
                                            success = true; // continue writing directory anyway
                                        }
                                    }
                                    if !success {
                                        let mut wrn = pl::sprintf(&format!("Error reading value for {} entry {}, ID 0x%.4x", name.to_text().replace('%', "%%"), index.to_text()), &[old_id.clone()]);
                                        let mut trunc_ok = Value::Undef;
                                        if old_info.truthy() && !old_info.get("Unknown").truthy() {
                                            wrn = cat(&[wrn, pl::s(" "), old_info.get("Name")]);
                                            trunc_ok = old_info.get("TruncateOK");
                                        }
                                        let minor = if in_maker_notes { n(1) } else { trunc_ok.clone() };
                                        if pl::method(it, &et, "Error", vec![wrn, minor])?.truthy() {
                                            return Ok(Value::Undef);
                                        }
                                        if !trunc_ok.truthy() {
                                            index = pl::add(&index, &n(1));
                                            old_id = new_id.clone();
                                            continue 'entry; // drop this tag
                                        }
                                    }
                                } else if !invalid_preview.truthy() {
                                    if pl::method(it, &et, "Error", vec![cat(&[pl::s("Bad "), name.clone(), pl::s(" offset for "), tag_str.clone()]), n(in_maker_notes as i64)])?.truthy() {
                                        return Ok(Value::Undef);
                                    }
                                    index = pl::add(&index, &n(1));
                                    old_id = new_id.clone();
                                    continue 'entry; // drop this tag
                                }
                                if invalid_preview.truthy() {
                                    // set value for invalid preview
                                    if et.get("FILE_TYPE").eq_str("JPEG") {
                                        // define dummy value for preview (or Leica MakerNote) to write later
                                        // (value must be larger than 4 bytes to generate PREVIEW_INFO,
                                        // and an even number of bytes so it won't be padded)
                                        put(&old_value, pl::s("LOAD_PREVIEW"));
                                    } else {
                                        put(&old_value, pl::s("none"));
                                        old_size = len(&get(&old_value));
                                    }
                                    value_ptr = n(0);
                                } else {
                                    update_tiff_end_v(&et, &pl::add(&pl::add(&pl::add(&base, &value_ptr), &data_pos), &old_size));
                                }
                                // update pointers for value just read from file
                                value_data_pt = rf(&old_value);
                                value_data_pos = pl::add(&value_ptr, &data_pos);
                                value_data_len = old_size.clone();
                                value_ptr = n(0);
                                read_from_file = true;
                            }
                            if suspect {
                                let tag_str = if old_info.truthy() { old_info.get("Name") } else { pl::sprintf("tag 0x%.4x", &[old_id.clone()]) };
                                let s = cat(&[pl::s("Suspicious "), name.clone(), pl::s(" offset for "), tag_str]);
                                if in_maker_notes {
                                    pl::method(it, &et, "Warn", vec![s, n(1)])?;
                                } else if pl::method(it, &et, "Error", vec![s, n(1)])?.truthy() {
                                    return Ok(Value::Undef);
                                }
                            }
                        }
                        // read value if we haven't already
                        if !read_from_file {
                            let v = substr(it, &value_data_pt.deref(), &value_ptr, Some(&old_size))?;
                            put(&old_value, v);
                        }
                        // get tagInfo using value if necessary
                        if !old_info.is_undef() && !old_info.truthy() {
                            let unk = pl::method(it, &et, "Options", vec![pl::s("Unknown"), n(1)])?;
                            old_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), old_id.clone(), rf(&old_value), old_form_name.clone(), old_count.clone()])?;
                            pl::method(it, &et, "Options", vec![pl::s("Unknown"), unk])?;
                            // now that we have the value, we can resolve the Condition to finally
                            // determine whether we want to delete this tag or not
                            if may_delete.get(old_id.to_bytes()).truthy() && old_info.truthy() && (new_tags.is_empty() || !pl::num_eq(&new_tags[0], &old_id)) {
                                let mut nv_hash = pl::method(it, &et, "GetNewValueHash", vec![old_info.clone(), dir_name.clone()])?;
                                if !nv_hash.truthy() && wrong_dir.truthy() {
                                    // delete from wrong directory if necessary
                                    nv_hash = pl::method(it, &et, "GetNewValueHash", vec![old_info.clone(), wrong_dir.clone()])?;
                                    if nv_hash.truthy() {
                                        x_delete.set(old_id.to_bytes(), n(1));
                                    }
                                }
                                if nv_hash.truthy() {
                                    // we want to delete this tag after all, so insert it into our list
                                    set.set(old_id.to_bytes(), old_info.clone());
                                    new_tags.insert(0, old_id.clone());
                                }
                            }
                        }
                        // make sure we are handling the 'ifd' format properly
                        if (pl::num_eq(&old_format, &n(13)) || pl::num_eq(&old_format, &n(18))) && (!old_info.truthy() || !old_info.get("SubIFD").truthy()) {
                            let s = pl::sprintf("%s tag 0x%.4x IFD format not handled", &[name.clone(), old_id.clone()]);
                            pl::method(it, &et, "Error", vec![s, n(in_maker_notes as i64)])?;
                        }
                        // override format we use to read the value if specified
                        if old_info.truthy() {
                            // check for tags which must be integers
                            if (old_info.get("IsOffset").truthy() || old_info.get("SubIFD").truthy()) && !int_format.get(old_form_name.to_bytes()).truthy() {
                                let msg = cat(&[pl::s("Invalid format ("), old_form_name.clone(), pl::s(") for "), name.clone(), pl::s(" "), old_info.get("Name")]);
                                pl::method(it, &et, "Error", vec![msg, n(in_maker_notes as i64)])?;
                                index = pl::add(&index, &n(1));
                                old_id = new_id.clone();
                                continue 'entry; // drop this tag
                            }
                            if old_info.get("Drop").truthy() && et.get("DropTags").truthy() && (pl::num_eq(&old_info.get("Drop"), &n(1)) || pl::lt(&old_info.get("Drop"), &old_size)) {
                                index = pl::add(&index, &n(1));
                                old_id = new_id.clone();
                                continue 'entry; // drop this tag
                            }
                            if old_info.get("Format").truthy() {
                                read_form_name = old_info.get("Format");
                                read_format = fnum(&read_form_name);
                                if !read_format.truthy() {
                                    // we aren't reading in a standard EXIF format, so rewrite in old format
                                    read_form_name = old_form_name.clone();
                                    read_format = old_format.clone();
                                }
                                if old_info.get("FixedSize").truthy() {
                                    if old_info.get("FixedSize").truthy() {
                                        old_size = old_info.get("FixedSize");
                                    }
                                    let v = substr(it, &value_data_pt.deref(), &value_ptr, Some(&old_size))?;
                                    put(&old_value, v);
                                }
                                // adjust number of items to read if format size changed
                                read_count = pl::div(&old_size, &fsz(&read_format))?;
                            }
                        }
                        if pl::le(&old_id, &last_tag_id) && !(in_maker_notes || pl::method(it, &et, "IsRawType", vec![])?.truthy()) {
                            let s = if old_info.truthy() { pl::concat(&old_info.get("Name"), &pl::s(" tag")) } else { pl::sprintf("tag 0x%x", &[old_id.clone()]) };
                            if pl::num_eq(&old_id, &last_tag_id) {
                                pl::method(it, &et, "Warn", vec![cat(&[pl::s("Duplicate "), s, pl::s(" in "), name.clone()])])?;
                                // put this tag back into the newTags list if necessary
                                if !set.get(old_id.to_bytes()).is_undef() {
                                    new_tags.insert(0, old_id.clone());
                                }
                            } else {
                                pl::method(it, &et, "Warn", vec![cat(&[pl::ucfirst(&s), pl::s(" out of sequence in "), name.clone()])])?;
                            }
                        }
                        last_tag_id = old_id.clone();
                        index = pl::add(&index, &n(1)); // increment index for next time
                    } else {
                        old_id = Value::Undef; // no more existing entries
                    }
                }
                //
                // write out all new tags, up to and including this one
                //
                new_id = new_tags.first().cloned().unwrap_or_default();
                let mut is_new: i64; // -1=tag is old, 0=tag same as existing, 1=tag is new
                if old_id.is_undef() {
                    if new_id.is_undef() {
                        break 'entry;
                    }
                    is_new = 1;
                } else if new_id.is_undef() {
                    // maker notes will have no new tags defined
                    if !set.get(old_id.to_bytes()).is_undef() {
                        new_id = old_id.clone();
                        is_new = 0;
                    } else {
                        is_new = -1;
                    }
                } else {
                    is_new = match ops::num_cmp(&old_id, &new_id) {
                        Some(std::cmp::Ordering::Less) => -1,
                        Some(std::cmp::Ordering::Greater) => 1,
                        _ => 0,
                    };
                    // special logic needed if directory has out-of-order entries
                    if unsorted && is_new != 0 {
                        if is_new > 0 && has_old_id.get(new_id.to_bytes()).truthy() {
                            // we wanted to create the new tag, but an old tag
                            // does exist with this ID, so defer writing the new tag
                            is_new = -1;
                        }
                        if is_new < 0 && has_new_id.get(old_id.to_bytes()).truthy() {
                            // we wanted to write the old tag, but we have
                            // a new tag with this ID, so move it up in the order
                            let mut tmp_tags = vec![old_id.clone()];
                            for t in &new_tags {
                                if !pl::num_eq(t, &old_id) {
                                    tmp_tags.push(t.clone());
                                }
                            }
                            new_tags = tmp_tags;
                            new_id = old_id.clone();
                            is_new = 0;
                        }
                    }
                }
                let mut new_info = old_info.clone();
                let mut new_format = old_format.clone();
                let mut new_form_name = old_form_name.clone();
                let mut new_count = old_count.clone();
                let mut ifd_form_name = Value::Undef;
                let new_value = cell(Value::Undef);
                let mut new_value_pt = if is_new >= 0 { rf(&new_value) } else { rf(&old_value) };
                let mut is_overwriting = Value::Undef;

                if is_new >= 0 {
                    // add, edit or delete this tag
                    if !new_tags.is_empty() {
                        new_tags.remove(0); // remove from list
                    }
                    let mut cur_info = set.get(new_id.to_bytes());
                    // don't allow MakerNotes to be added to ExifIFD of CR3 file
                    if pl::num_eq(&new_id, &n(0x927c)) && is_new > 0 && et.get("FileType").eq_str("CR3") {
                        continue 'entry;
                    }
                    if !(cur_info.truthy() || add_dirs.get(new_id.to_bytes()).truthy()) {
                        // we can finally get the specific tagInfo reference for this tag
                        // (because we can now evaluate the Condition statement since all
                        //  DataMember's have been obtained for tags up to this one)
                        cur_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), new_id.clone()])?;
                        if !cur_info.is_undef() && !cur_info.truthy() {
                            // need value to evaluate the condition
                            // (tricky because we need the tagInfo ref to get the value,
                            //  so we must loop through all new tagInfo's...)
                            for ti in &new_tag_info {
                                if !pl::num_eq(&ti.get("TagID"), &new_id) {
                                    continue;
                                }
                                let mut val = pl::method(it, &et, "GetNewValue", vec![ti.clone()])?;
                                if val.is_undef() {
                                    may_delete.set(new_id.to_bytes(), n(1));
                                    continue;
                                }
                                // must convert to binary for evaluating in Condition
                                let mut fmt = ti.get("Format");
                                if !fmt.truthy() {
                                    fmt = ti.get("Writable");
                                }
                                if fmt.truthy() {
                                    val = write_value(it, vec![val, fmt, ti.get("Count")])?;
                                    if val.is_undef() {
                                        may_delete.set(new_id.to_bytes(), n(1));
                                        continue;
                                    }
                                }
                                cur_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), new_id.clone(), pl::sref(val), old_form_name.clone(), old_count.clone()])?;
                                if cur_info.truthy() {
                                    if seq(&cur_info, ti) {
                                        break;
                                    }
                                    cur_info = Value::Undef;
                                }
                            }
                            // may want to delete this, but we need to see the old value first
                            if !cur_info.truthy() {
                                may_delete.set(new_id.to_bytes(), n(1));
                            }
                        }
                        // don't set this tag unless valid for the current condition
                        if cur_info.truthy() && et.get("NEW_VALUE").get(cur_info.to_bytes()).truthy() {
                            set.set(new_id.to_bytes(), cur_info.clone());
                        } else {
                            if is_new > 0 {
                                continue 'entry;
                            }
                            is_new = -1;
                            cur_info = Value::Undef;
                        }
                    }
                    let mut no_write = false;
                    if cur_info.truthy() {
                        'cur: {
                            if cur_info.get("WriteCondition").truthy() {
                                let self_ = et.clone(); // set $self to be used in eval
                                //### eval WriteCondition ($self)
                                let vars = [("self", self_), ("et", et.clone())];
                                let (r, _) = eval_at(it, &cur_info.get("WriteCondition"), MODULE, FILE, &vars, Want::Scalar);
                                if !r.truthy() {
                                    let err = it.get_scalar("main::@");
                                    if err.truthy() {
                                        it.warn(err)?;
                                    }
                                    no_write = true; // GOTO !
                                    break 'cur;
                                }
                            }
                            let mut nv_hash = Value::Undef;
                            if is_new >= 0 {
                                nv_hash = pl::method(it, &et, "GetNewValueHash", vec![cur_info.clone(), dir_name.clone()])?;
                            }
                            if !(nv_hash.truthy() || (!mandatory.get(new_id.to_bytes()).is_undef() && !no_mandatory.truthy())) {
                                if !wrong_dir.truthy() {
                                    no_write = true; // GOTO !
                                    break 'cur;
                                }
                                // delete stuff from the wrong directory if setting somewhere else
                                nv_hash = pl::method(it, &et, "GetNewValueHash", vec![cur_info.clone(), wrong_dir.clone()])?;
                                // don't cross delete if not overwriting
                                if !pl::method(it, &et, "IsOverwriting", vec![nv_hash.clone()])?.truthy() {
                                    no_write = true; // GOTO !
                                    break 'cur;
                                }
                                // don't cross delete if specifically deleting from the other directory
                                // (Note: don't call GetValue() here because it shouldn't be called
                                //  if IsOverwriting returns < 0 -- eg. when shifting)
                                if nv_hash.get("Value").is_undef() && nv_hash.get("WantGroup").truthy() && seq(&pl::lc(&nv_hash.get("WantGroup")), &pl::lc(&wrong_dir)) {
                                    no_write = true; // GOTO !
                                    break 'cur;
                                } else {
                                    // remove this tag if found in this IFD
                                    x_delete.set(new_id.to_bytes(), n(1));
                                }
                            }
                        }
                    } else if !add_dirs.get(new_id.to_bytes()).truthy() {
                        no_write = true;
                    }
                    if no_write {
                        // NoWrite:
                        if is_new > 0 {
                            continue 'entry;
                        }
                        set.delete(new_id.to_bytes());
                        is_new = -1;
                    }
                    if set.get(new_id.to_bytes()).truthy() {
                        //
                        // set the new tag value (or 'next' if deleting tag)
                        //
                        new_info = set.get(new_id.to_bytes());
                        new_count = new_info.get("Count");
                        let mut val = Value::Undef;
                        let new_val = cell(Value::Undef);
                        let mut nv_hash = pl::method(it, &et, "GetNewValueHash", vec![new_info.clone(), dir_name.clone()])?;
                        if is_new > 0 {
                            // don't create new entry unless requested
                            if nv_hash.truthy() {
                                if !nv_hash.get("IsCreating").truthy() {
                                    continue 'entry;
                                }
                                if new_info.get("IsOverwriting").truthy() {
                                    let proc = new_info.get("IsOverwriting");
                                    is_overwriting = pl::call_code(it, &proc, vec![et.clone(), nv_hash.clone(), val.clone(), rf(&new_val)], Want::Scalar)?.pop().unwrap_or_default();
                                } else {
                                    is_overwriting = pl::method(it, &et, "IsOverwriting", vec![nv_hash.clone()])?;
                                }
                            } else {
                                if x_delete.get(new_id.to_bytes()).truthy() {
                                    continue 'entry; // don't create if cross deleting
                                }
                                put(&new_val, mandatory.get(new_id.to_bytes())); // get value for mandatory tag
                                is_overwriting = n(1);
                            }
                            // convert using new format
                            if new_info.get("Format").truthy() {
                                new_form_name = new_info.get("Format");
                                // use Writable flag to specify IFD format code
                                ifd_form_name = new_info.get("Writable");
                            } else {
                                new_form_name = new_info.get("Writable");
                                if !new_form_name.truthy() {
                                    pl::warn(it, &format!("No format for {} {}\n", name.to_text(), new_info.get("Name").to_text()))?;
                                    continue 'entry;
                                }
                            }
                            new_format = fnum(&new_form_name);
                        } else if nv_hash.truthy() || x_delete.get(new_id.to_bytes()).truthy() {
                            if !nv_hash.truthy() {
                                nv_hash = pl::method(it, &et, "GetNewValueHash", vec![new_info.clone(), wrong_dir.clone()])?;
                            }
                            // read value
                            if pl::ge(&len(&get(&old_value)), &old_size) {
                                val = read_value(it, vec![rf(&old_value), n(0), read_form_name.clone(), read_count.clone(), old_size.clone()])?;
                            } else {
                                val = pl::s("");
                            }
                            // determine write format (by default, use 'Writable' format)
                            let mut writable = new_info.get("Writable");
                            // (or use existing format if 'Writable' not specified)
                            if !(writable.truthy() && !writable.eq_str("1")) {
                                writable = old_form_name.clone();
                            }
                            // (and override write format with 'Format' if specified)
                            let f = new_info.get("Format");
                            let write_form = if f.truthy() { f } else { writable.clone() };
                            if !seq(&write_form, &new_form_name) {
                                // write in specified format
                                new_form_name = write_form;
                                new_format = fnum(&new_form_name);
                                // use different IFD format code if necessary
                                if in_maker_notes {
                                    // always preserve IFD format in maker notes
                                    ifd_form_name = old_form_name.clone();
                                } else if !seq(&writable, &new_form_name) {
                                    // use specified IFD format
                                    ifd_form_name = writable.clone();
                                }
                            }
                            if in_maker_notes && !read_form_name.eq_str("string") && !read_form_name.eq_str("undef") {
                                // keep same size in maker notes unless string or binary
                                new_count = pl::div(&pl::mul(&old_count, &fsz(&old_format)), &fsz(&new_format))?;
                            }
                            if new_info.get("IsOverwriting").truthy() {
                                let proc = new_info.get("IsOverwriting");
                                is_overwriting = pl::call_code(it, &proc, vec![et.clone(), nv_hash.clone(), val.clone(), rf(&new_val)], Want::Scalar)?.pop().unwrap_or_default();
                            } else {
                                is_overwriting = pl::method(it, &et, "IsOverwriting", vec![nv_hash.clone(), val.clone()])?;
                            }
                        }
                        let mut no_overwrite = false;
                        if is_overwriting.truthy() {
                            'ow: {
                                if get(&new_val).is_undef() {
                                    put(&new_val, pl::method(it, &et, "GetNewValue", vec![nv_hash.clone()])?);
                                }
                                // value undefined if deleting this tag
                                // (also delete tag if cross-deleting and this isn't a date/time shift)
                                if get(&new_val).is_undef() || (x_delete.get(new_id.to_bytes()).truthy() && nv_hash.get("Shift").is_undef()) {
                                    if get(&new_val).is_undef() && new_info.get("RawConvInv").truthy() && !nv_hash.get("Value").is_undef() {
                                        // error in RawConvInv, so rewrite existing tag
                                        no_overwrite = true; // GOTO!
                                        break 'ow;
                                    }
                                    if is_new == 0 {
                                        inc(&et, "CHANGED");
                                        pl::method(it, &et, "VerboseValue", vec![cat(&[pl::s("- "), dir_name.clone(), pl::s(":"), new_info.get("Name")]), val.clone()])?;
                                    }
                                    continue 'entry;
                                }
                                if new_count.truthy() && pl::lt(&new_count, &n(0)) {
                                    // set count to number of values if variable
                                    let vals = pl::split_ws(&get(&new_val));
                                    new_count = n(vals.len() as i64);
                                }
                                // convert to binary format
                                put(&new_value, write_value(it, vec![get(&new_val), new_form_name.clone(), new_count.clone()])?);
                                if get(&new_value).is_undef() {
                                    pl::method(it, &et, "Warn", vec![cat(&[pl::s("Invalid value for "), dir_name.clone(), pl::s(":"), new_info.get("Name")])])?;
                                    no_overwrite = true; // GOTO!
                                    break 'ow;
                                }
                                if len(&get(&new_value)).truthy() {
                                    // limit maximum value length in JPEG images
                                    // (max segment size is 65533 bytes and the min EXIF size is 96 incl an additional IFD entry)
                                    if et.get("FILE_TYPE").eq_str("JPEG") && pl::gt(&len(&get(&new_value)), &n(65436)) && !new_info.get("Name").eq_str("PreviewImage") {
                                        let nm = if new_info.get("MakerNotes").truthy() { pl::s("MakerNotes") } else { new_info.get("Name") };
                                        pl::method(it, &et, "Warn", vec![pl::concat(&pl::s("Writing large value for "), &nm), n(1)])?;
                                    }
                                    // re-code if necessary
                                    if new_form_name.eq_str("utf8") {
                                        let e = pl::method(it, &et, "Encode", vec![get(&new_value), pl::s("UTF8")])?;
                                        put(&new_value, e);
                                    } else if str_enc.truthy() && new_form_name.eq_str("string") {
                                        let e = pl::method(it, &et, "Encode", vec![get(&new_value), str_enc.clone()])?;
                                        put(&new_value, e);
                                    }
                                } else {
                                    pl::method(it, &et, "Warn", vec![cat(&[pl::s("Can't write zero length "), new_info.get("Name"), pl::s(" in "), tag_table_ptr.get("GROUPS").get("1")])])?;
                                    no_overwrite = true; // GOTO!
                                    break 'ow;
                                }
                                if is_new >= 0 {
                                    new_count = pl::div(&len(&get(&new_value)), &fsz(&new_format))?;
                                    inc(&et, "CHANGED");
                                    if !all_mandatory.is_undef() {
                                        // not all mandatory if we are writing any tag specifically
                                        if nv_hash.truthy() {
                                            all_mandatory = Value::Undef;
                                            delete_all = Value::Undef;
                                        } else {
                                            add_mandatory = pl::add(&add_mandatory, &n(1)); // count mandatory tags that we added
                                        }
                                    }
                                    if pl::gt(&verbose, &n(1)) {
                                        if is_new == 0 {
                                            pl::method(it, &et, "VerboseValue", vec![cat(&[pl::s("- "), dir_name.clone(), pl::s(":"), new_info.get("Name")]), val.clone()])?;
                                        }
                                        if new_info.get("OffsetPair").truthy() && get(&new_val).eq_str("4277010157") {
                                            // (0xfeedfeed)
                                            let o = et.get("OPTIONS").get("TextOut");
                                            print_out(it, &o, cat(&[pl::s("    + "), dir_name.clone(), pl::s(":"), new_info.get("Name"), pl::s(" = <tbd>\n")]))?;
                                        } else {
                                            let s = pl::s(if nv_hash.truthy() { "" } else { " (mandatory)" });
                                            pl::method(it, &et, "VerboseValue", vec![cat(&[pl::s("+ "), dir_name.clone(), pl::s(":"), new_info.get("Name")]), get(&new_val), s])?;
                                        }
                                    }
                                }
                            }
                        } else {
                            no_overwrite = true;
                        }
                        if no_overwrite {
                            // NoOverwrite:
                            if is_new > 0 {
                                continue 'entry;
                            }
                            is_new = -1; // rewrite existing tag
                        }
                        // set format for EXIF IFD if different than conversion format
                        if ifd_form_name.truthy() {
                            new_form_name = ifd_form_name.clone();
                            new_format = fnum(&new_form_name);
                        }
                    } else if is_new > 0 {
                        //
                        // create new subdirectory
                        //
                        // newInfo may not be defined if we try to add a mandatory tag
                        // to a directory that doesn't support it (eg. IFD1 in RW2 images)
                        new_info = add_dirs.get(new_id.to_bytes());
                        if !new_info.truthy() {
                            continue 'entry;
                        }
                        // make sure we don't try to generate a new MakerNotes directory
                        // or a SubIFD
                        if new_info.get("MakerNotes").truthy() || new_info.get("Name").eq_str("SubIFD") {
                            continue 'entry;
                        }
                        let sub_table = if new_info.get("SubDirectory").get("TagTable").truthy() {
                            etf(it, "GetTagTable", vec![new_info.get("SubDirectory").get("TagTable")])?
                        } else {
                            tag_table_ptr.clone()
                        };
                        // create empty source directory
                        let source_dir = Value::new_hash();
                        source_dir.set("Parent", dir_name.clone());
                        source_dir.set("Fixup", fixup_new(it)?);
                        if new_info.get("SubIFD").truthy() {
                            source_dir.set("DirName", new_info.get("Groups").get("1"));
                        }
                        put(&new_value, pl::method(it, &et, "WriteDirectory", vec![source_dir.clone(), sub_table.clone()])?);
                        // only add new directory if it isn't empty
                        if !(!get(&new_value).is_undef() && len(&get(&new_value)).truthy()) {
                            continue 'entry;
                        }
                        // set the fixup start location
                        if new_info.get("SubIFD").truthy() {
                            // subdirectory is referenced by an offset in value buffer
                            let subdir = cell(get(&new_value));
                            put(&new_value, s32u(it, n(0xfeedf00d))?);
                            let sd = Value::new_hash();
                            sd.set("DataPt", rf(&subdir));
                            sd.set("Table", sub_table.clone());
                            sd.set("Fixup", source_dir.get("Fixup"));
                            sd.set("Offset", pl::add(&len(&get(&dir_buff)), &n(8)));
                            sd.set("Where", pl::s("dirBuff"));
                            subdirs.push(sd);
                            new_form_name = pl::s("int32u");
                            new_format = fnum(&new_form_name);
                        } else {
                            // subdirectory goes directly into value buffer
                            add_to(&source_dir.get("Fixup"), "Start", &len(&val_buff));
                            // use Writable to set format, otherwise 'undef'
                            new_form_name = new_info.get("Writable");
                            if !(new_form_name.truthy() && fnum(&new_form_name).truthy()) {
                                new_form_name = pl::s("undef");
                            }
                            new_format = fnum(&new_form_name);
                            val_fixups.push(source_dir.get("Fixup"));
                        }
                    } else if {
                        autoviv(&mut new_info);
                        new_info.get("Format").truthy() && new_info.get("Writable").truthy() && !new_info.get("Writable").eq_str("1")
                    } {
                        // use specified write format
                        new_form_name = new_info.get("Writable");
                        new_format = fnum(&new_form_name);
                    } else if add_dirs.get(new_id.to_bytes()).truthy() && !seq(&new_info, &add_dirs.get(new_id.to_bytes())) {
                        // this can happen if we are trying to add a directory that doesn't exist
                        // in this type of file (eg. try adding a SubIFD tag to an A100 image)
                        is_new = -1;
                    }
                }
                if is_new < 0 {
                    // just rewrite existing tag
                    new_id = old_id.clone();
                    put(&new_value, get(&old_value));
                    new_format = old_format.clone(); // (just in case it changed)
                    new_form_name = old_form_name.clone();
                    // set offset of this entry in the directory so we can update the pointer
                    // and save block information to copy this large block later
                    if old_image_data.truthy() {
                        old_image_data.set_at(3, pl::add(&pl::add(&new_start, &len(&get(&dir_buff))), &n(2)));
                        image_data.push(old_image_data.clone());
                        orig_dir_info.set("ImageData", image_data.clone());
                    }
                }
                if new_info.truthy() {
                    {
                        //
                        // load necessary data for this tag (thumbnail image, etc)
                        //
                        if new_info.get("DataTag").truthy() && is_new >= 0 {
                            let data_tag = new_info.get("DataTag");
                            // load data for this tag
                            if !(!offset_data.get(data_tag.to_bytes()).is_undef() || data_tag.eq_str("LeicaTrailer")) {
                                // prefer tag from Composite table if it exists (otherwise
                                // PreviewImage data would be taken from Extra tag)
                                let comp_info = pl::call(it, "Image::ExifTool::GetCompositeTagInfo", vec![data_tag.clone()])?;
                                let v = pl::method(it, &et, "GetNewValue", vec![if comp_info.truthy() { comp_info } else { data_tag.clone() }])?;
                                offset_data.set(data_tag.to_bytes(), v);
                                let mut err = Value::Undef;
                                if !offset_data.get(data_tag.to_bytes()).is_undef() {
                                    let l = len(&offset_data.get(data_tag.to_bytes()));
                                    if data_tag.eq_str("PreviewImage") {
                                        // must set DEL_PREVIEW flag now if preview fit into IFD
                                        if pl::le(&l, &n(4)) {
                                            et.set("DEL_PREVIEW", n(1));
                                        }
                                    }
                                } else {
                                    err = pl::concat(&data_tag, &pl::s(" not found"));
                                }
                                if err.truthy() {
                                    if new_info.get("IsOffset").truthy() {
                                        pl::method(it, &et, "Warn", vec![err])?;
                                    }
                                    set.delete(new_id.to_bytes()); // remove from list of tags we are setting
                                    continue 'entry;
                                }
                            }
                        }
                        //
                        // write maker notes
                        //
                        if new_info.get("MakerNotes").truthy() {
                            // don't write new makernotes if we are deleting this group
                            let dgm = et.get("DEL_GROUP").get("MakerNotes");
                            if dgm.truthy() && (!pl::num_eq(&dgm, &n(2)) || is_new <= 0) {
                                let rt = pl::method(it, &et, "IsRawType", vec![])?;
                                if rt.truthy() && !(pl::num_eq(&rt, &n(2)) && dir_name.eq_str("ExifIFD")) {
                                    pl::method(it, &et, "Warn", vec![pl::concat(&pl::s("Can't delete MakerNotes from "), &et.get("FileType")), n(1)])?;
                                } else {
                                    if is_new <= 0 {
                                        inc(&et, "CHANGED");
                                        if verbose.truthy() {
                                            print_out(it, &out, pl::s("  Deleting MakerNotes\n"))?;
                                        }
                                    }
                                    continue 'entry;
                                }
                            }
                            let save_order = byte_order(it)?;
                            if is_new >= 0 && !set.get(new_id.to_bytes()).is_undef() {
                                // we are writing a whole new maker note block
                                // --> add fixup information if necessary
                                let nv_hash = pl::method(it, &et, "GetNewValueHash", vec![new_info.clone(), dir_name.clone()])?;
                                if nv_hash.truthy() && nv_hash.get("MAKER_NOTE_FIXUP").truthy() {
                                    // must clone fixup because we will be shifting it
                                    let maker_fixup = pl::method(it, &nv_hash.get("MAKER_NOTE_FIXUP"), "Clone", vec![])?;
                                    let val_len = len(&val_buff);
                                    add_to(&maker_fixup, "Start", &val_len);
                                    val_fixups.push(maker_fixup);
                                }
                            } else {
                                // update maker notes if possible
                                let subdir_info = Value::new_hash();
                                subdir_info.set("Base", base.clone());
                                subdir_info.set("DataPt", value_data_pt.clone());
                                subdir_info.set("DataPos", value_data_pos.clone());
                                subdir_info.set("DataLen", value_data_len.clone());
                                subdir_info.set("DirStart", value_ptr.clone());
                                subdir_info.set("DirLen", old_size.clone());
                                subdir_info.set("DirName", pl::s("MakerNotes"));
                                subdir_info.set("Name", new_info.get("Name"));
                                subdir_info.set("Parent", dir_name.clone());
                                subdir_info.set("TagInfo", new_info.clone());
                                subdir_info.set("RAF", raf.clone());
                                let (mut sub_table, mut subdir, mut loc, mut write_proc, mut not_ifd) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef, Value::Undef);
                                if new_info.get("SubDirectory").truthy() {
                                    let sub = new_info.get("SubDirectory");
                                    if sub.get("FixBase").truthy() {
                                        subdir_info.set("FixBase", n(1));
                                    }
                                    subdir_info.set("FixOffsets", sub.get("FixOffsets"));
                                    subdir_info.set("EntryBased", sub.get("EntryBased"));
                                    if !sub.get("Base").is_undef() {
                                        subdir_info.set("NoFixBase", n(1));
                                    }
                                    subdir_info.set("AutoFix", sub.get("AutoFix"));
                                    if sub.get("ByteOrder").truthy() {
                                        set_byte_order(it, sub.get("ByteOrder"))?;
                                    }
                                }
                                // get the proper tag table for these maker notes
                                if old_info.truthy() && old_info.get("SubDirectory").truthy() {
                                    sub_table = old_info.get("SubDirectory").get("TagTable");
                                    if sub_table.truthy() {
                                        sub_table = etf(it, "GetTagTable", vec![sub_table])?;
                                    }
                                    write_proc = old_info.get("SubDirectory").get("WriteProc");
                                    not_ifd = old_info.get("NotIFD");
                                } else {
                                    pl::method(it, &et, "Warn", vec![pl::s("Internal problem getting maker notes tag table")])?;
                                }
                                if sub_table.truthy() && !write_proc.truthy() {
                                    write_proc = sub_table.get("WRITE_PROC");
                                }
                                if !sub_table.truthy() {
                                    sub_table = tag_table_ptr.clone();
                                }
                                let wuop = Value::Code(it.sub_ref("Image::ExifTool::MakerNotes::WriteUnknownOrPreview"));
                                let pbd = Value::Code(it.sub_ref("Image::ExifTool::ProcessBinaryData"));
                                if write_proc.truthy() && seq(&write_proc, &wuop) && re(r"^\xff\xd8\xff", "").is_match(&get(&old_value)) {
                                    loc = n(0);
                                } else if !not_ifd.truthy() {
                                    // look for IFD-style maker notes
                                    loc = pl::call(it, "Image::ExifTool::MakerNotes::LocateIFD", vec![et.clone(), subdir_info.clone()])?;
                                }
                                if !loc.is_undef() {
                                    // we need fixup data for this subdirectory
                                    subdir_info.set("Fixup", fixup_new(it)?);
                                    // rewrite maker notes
                                    let changed = et.get("CHANGED");
                                    subdir = pl::method(it, &et, "WriteDirectory", vec![subdir_info.clone(), sub_table.clone(), write_proc.clone()])?;
                                    if pl::num_eq(&changed, &et.get("CHANGED")) && pl::method(it, &subdir_info.get("Fixup"), "IsEmpty", vec![])?.truthy() {
                                        // return original data if nothing changed and no fixups
                                        subdir = Value::Undef;
                                    }
                                } else if sub_table.get("PROCESS_PROC").truthy() && seq(&sub_table.get("PROCESS_PROC"), &pbd) {
                                    let sub = old_info.get("SubDirectory");
                                    if !sub.get("Start").is_undef() {
                                        //### eval Start ($valuePtr)
                                        let vars = [("valuePtr", value_ptr.clone()), ("et", et.clone()), ("dirInfo", dir_info.clone())];
                                        let (start, _) = eval_at(it, &sub.get("Start"), MODULE, FILE, &vars, Want::Scalar);
                                        loc = pl::sub(&start, &value_ptr);
                                        subdir_info.set("DirStart", start);
                                        subdir_info.set("DirLen", pl::sub(&subdir_info.get("DirLen"), &loc));
                                    } else {
                                        loc = n(0);
                                    }
                                    // rewrite maker notes
                                    subdir = pl::method(it, &et, "WriteDirectory", vec![subdir_info.clone(), sub_table.clone()])?;
                                } else if not_ifd.truthy() {
                                    if write_proc.truthy() {
                                        loc = n(0);
                                        subdir = pl::method(it, &et, "WriteDirectory", vec![subdir_info.clone(), sub_table.clone()])?;
                                    }
                                } else {
                                    let msg = pl::s("Maker notes could not be parsed");
                                    if et.get("FILE_TYPE").eq_str("JPEG") {
                                        pl::method(it, &et, "Warn", vec![msg, n(1)])?;
                                    } else {
                                        pl::method(it, &et, "Error", vec![msg, n(1)])?;
                                    }
                                }
                                if !subdir.is_undef() {
                                    if !len(&subdir).truthy() {
                                        set_byte_order(it, save_order.clone())?;
                                        continue 'entry;
                                    }
                                    let val_len = len(&val_buff);
                                    // restore existing header and substitute the new
                                    // maker notes for the old value
                                    let h = substr(it, &get(&old_value), &n(0), Some(&loc))?;
                                    put(&new_value, pl::concat(&h, &subdir));
                                    let mut maker_fixup = subdir_info.get("Fixup");
                                    autoviv(&mut maker_fixup); // (perl autovivifies it at $$makerFixup{Start})
                                    let preview_info = et.get("PREVIEW_INFO");
                                    if subdir_info.get("Relative").truthy() {
                                        // apply a one-time fixup to $loc since offsets are relative
                                        add_to(&maker_fixup, "Start", &loc);
                                        // shift all offsets to be relative to new base
                                        let base_shift = pl::sub(&pl::add(&pl::add(&value_data_pos, &value_ptr), &base), &subdir_info.get("Base"));
                                        add_to(&maker_fixup, "Shift", &base_shift);
                                        pl::method(it, &maker_fixup, "ApplyFixup", vec![rf(&new_value)])?;
                                        if preview_info.truthy() {
                                            // remove all but PreviewImage fixup (since others shouldn't change)
                                            let ptrs = maker_fixup.get("Pointers");
                                            for k in ptrs.keys() {
                                                if !re(r"_PreviewImage$", "").is_match(&Value::from(k.clone())) {
                                                    ptrs.delete(&k);
                                                }
                                            }
                                            // zero pointer so we can see how it gets shifted later
                                            pl::method(it, &maker_fixup, "SetMarkerPointers", vec![rf(&new_value), pl::s("PreviewImage"), n(0)])?;
                                            // set the pointer to the start of the EXIF information
                                            // add preview image fixup to list of value fixups
                                            add_to(&maker_fixup, "Start", &val_len);
                                            val_fixups.push(maker_fixup.clone());
                                            preview_info.set("BaseShift", base_shift);
                                            preview_info.set("Relative", n(1));
                                        }
                                    // don't shift anything if relative flag set to zero (Pentax patch)
                                    } else if subdir_info.get("Relative").is_undef() {
                                        // shift offset base if shifted in the original image or if FixBase
                                        // was used, but be careful of automatic FixBase with negative shifts
                                        // since they may lead to negative (invalid) offsets (casio_edit_problem.jpg)
                                        let mut base_shift = pl::sub(&base, &subdir_info.get("Base"));
                                        if subdir_info.get("AutoFix").truthy() {
                                            base_shift = n(0);
                                        } else if subdir_info.get("FixBase").truthy() && pl::lt(&base_shift, &n(0)) &&
                                            // allow negative base shift if offsets are bigger (PentaxOptioWP.jpg)
                                            (!subdir_info.get("MinOffset").truthy() || pl::lt(&pl::add(&subdir_info.get("MinOffset"), &base_shift), &n(0)))
                                        {
                                            let fix_base = opt(it, &et, "FixBase")?;
                                            if fix_base.is_undef() {
                                                let s = if opt(it, &et, "IgnoreMinorErrors")?.truthy() { "ignored" } else { "fix or ignore?" };
                                                pl::method(it, &et, "Error", vec![pl::s(&format!("MakerNotes offsets may be incorrect ({s})")), n(1)])?;
                                            } else if fix_base.eq_str("") {
                                                pl::method(it, &et, "Warn", vec![pl::s("Fixed incorrect MakerNotes offsets")])?;
                                                base_shift = n(0);
                                            }
                                        }
                                        add_to(&maker_fixup, "Start", &pl::add(&val_len, &loc));
                                        add_to(&maker_fixup, "Shift", &base_shift);
                                        // permanently fix makernote offset errors
                                        add_to(&maker_fixup, "Shift", &or0(subdir_info.get("FixedBy")));
                                        val_fixups.push(maker_fixup.clone());
                                        if preview_info.truthy() && !preview_info.get("NoBaseShift").truthy() {
                                            preview_info.set("BaseShift", base_shift);
                                        }
                                    }
                                    new_value_pt = rf(&new_value); // write new value
                                }
                            }
                            set_byte_order(it, save_order)?;

                        // process existing subdirectory unless we are overwriting it entirely
                        } else if new_info.get("SubDirectory").truthy() && is_new <= 0 && !is_overwriting.truthy()
                            // don't edit directory if Writable is set to 0
                            && (new_info.get("Writable").is_undef() || new_info.get("Writable").truthy())
                            && !new_info.get("ReadFromRAF").truthy()
                        {
                            let subdir = new_info.get("SubDirectory");
                            if new_info.get("SubIFD").truthy() {
                                //
                                // rewrite existing sub IFD's
                                //
                                let mut sub_table = tag_table_ptr.clone();
                                if subdir.get("TagTable").truthy() {
                                    sub_table = etf(it, "GetTagTable", vec![subdir.get("TagTable")])?;
                                }
                                // determine directory name for this IFD
                                let g1 = new_info.get("Groups").get("1");
                                let mut subdir_name = if g1.truthy() { g1 } else { new_info.get("Name") };
                                // all makernotes directory names must be 'MakerNotes'
                                if sub_table.get("GROUPS").get("0").eq_str("MakerNotes") {
                                    subdir_name = pl::s("MakerNotes");
                                }
                                // must handle sub-IFD's specially since the values
                                // are actually offsets to subdirectories
                                if !read_count.truthy() {
                                    // can't have zero count
                                    if pl::method(it, &et, "Error", vec![cat(&[name.clone(), pl::s(" entry "), index.clone(), pl::s(" has zero count")]), n(2)])?.truthy() {
                                        return Ok(Value::Undef);
                                    }
                                    continue 'entry;
                                }
                                let mut write_count = 0i64;
                                put(&new_value, pl::s("")); // reset value because we regenerate it below
                                let mut i = n(0);
                                'subifd: while pl::lt(&i, &read_count) {
                                    let i_cur = i.clone();
                                    i = pl::add(&i, &n(1));
                                    let i = i_cur;
                                    let off = pl::mul(&i, &fsz(&read_format));
                                    let val = read_value(it, vec![value_data_pt.clone(), pl::add(&value_ptr, &off), read_form_name.clone(), n(1), pl::sub(&old_size, &off)])?;
                                    let mut subdir_start = pl::sub(&val, &data_pos);
                                    let mut subdir_base = base.clone();
                                    let mut hdr_len = Value::Undef;
                                    if !subdir.get("Start").is_undef() {
                                        //### eval Start ($val)
                                        let vars = [("val", val.clone()), ("et", et.clone()), ("dirInfo", dir_info.clone())];
                                        let (mut new_start, _) = eval_at(it, &subdir.get("Start"), MODULE, FILE, &vars, Want::Scalar);
                                        if !pl::call(it, "Image::ExifTool::IsInt", vec![new_start.clone()])?.truthy() {
                                            pl::method(it, &et, "Error", vec![pl::concat(&pl::s("Bad subdirectory start for "), &new_info.get("Name"))])?;
                                            continue 'subifd;
                                        }
                                        new_start = pl::sub(&new_start, &data_pos);
                                        hdr_len = pl::sub(&new_start, &subdir_start);
                                        subdir_start = new_start;
                                    }
                                    if subdir.get("Base").truthy() {
                                        let start = pl::add(&subdir_start, &data_pos);
                                        //### eval Base ($start,$base)
                                        let vars = [("start", start), ("base", base.clone()), ("et", et.clone()), ("dirInfo", dir_info.clone())];
                                        let (b, _) = eval_at(it, &subdir.get("Base"), MODULE, FILE, &vars, Want::Scalar);
                                        subdir_base = pl::add(&subdir_base, &b);
                                    }
                                    // add IFD number if more than one
                                    if i.truthy() {
                                        let ib = i.to_bytes();
                                        re(r"\d*$", "").subst(&mut subdir_name, false, |_| ib.to_vec());
                                    }
                                    let subdir_info = Value::new_hash();
                                    subdir_info.set("Base", subdir_base.clone());
                                    subdir_info.set("DataPt", data_pt.clone());
                                    subdir_info.set("DataPos", pl::add(&pl::sub(&data_pos, &subdir_base), &base));
                                    subdir_info.set("DataLen", data_len.clone());
                                    subdir_info.set("DirStart", subdir_start.clone());
                                    subdir_info.set("DirName", subdir_name.clone());
                                    subdir_info.set("Name", new_info.get("Name"));
                                    subdir_info.set("TagInfo", new_info.clone());
                                    subdir_info.set("Parent", dir_name.clone());
                                    subdir_info.set("Fixup", fixup_new(it)?);
                                    subdir_info.set("RAF", raf.clone());
                                    subdir_info.set("Subdir", subdir.clone());
                                    // set ImageData only for 1st level SubIFD's
                                    subdir_info.set("ImageData", if image_data_flag.eq_str("Main") { pl::s("SubIFD") } else { Value::Undef });
                                    // pass on header pointer only for certain sub IFD's
                                    if pl::num_eq(&new_info.get("SubIFD"), &n(2)) {
                                        subdir_info.set("HeaderPtr", dir_info.get("HeaderPtr"));
                                    }
                                    if subdir.get("RelativeBase").truthy() {
                                        // apply one-time fixup if offsets are relative (Sony IDC hack)
                                        subdir_info.delete("Fixup");
                                        subdir_info.delete("ImageData");
                                    }
                                    // is the subdirectory outside our current data?
                                    if pl::lt(&subdir_start, &n(0)) || pl::gt(&pl::add(&subdir_start, &n(2)), &data_len) {
                                        if raf.truthy() {
                                            // reset SubDirectory buffer (we will load it later)
                                            subdir_info.set("DataPt", pl::sref(pl::s("")));
                                            subdir_info.set("DataLen", n(0));
                                        } else {
                                            let err0 = cat(&[pl::s("Can't read "), subdir_name.clone(), pl::s(" data")]);
                                            if sub_table.get("VARS").truthy() && sub_table.get("VARS").get("MINOR_ERRORS").truthy() {
                                                pl::method(it, &et, "Warn", vec![pl::concat(&err0, &pl::s(". Ignored."))])?;
                                            } else if pl::method(it, &et, "Error", vec![err0, n(in_maker_notes as i64)])?.truthy() {
                                                return Ok(Value::Undef);
                                            }
                                            continue 'entry; // don't write this directory
                                        }
                                    }
                                    let mut subdir_data = pl::method(it, &et, "WriteDirectory", vec![subdir_info.clone(), sub_table.clone(), subdir.get("WriteProc")])?;
                                    if subdir_data.is_undef() {
                                        // WriteDirectory should have issued an error, but check just in case
                                        if !et.get("VALUE").get("Error").truthy() {
                                            pl::method(it, &et, "Error", vec![pl::concat(&pl::s("Error writing "), &subdir_name)])?;
                                        }
                                        return Ok(Value::Undef);
                                    }
                                    // add back original header if necessary (eg. Ricoh GR)
                                    if hdr_len.truthy() && pl::gt(&hdr_len, &n(0)) && pl::le(&subdir_start, &data_len) {
                                        let h = substr(it, &data_pt.deref(), &pl::sub(&subdir_start, &hdr_len), Some(&hdr_len))?;
                                        subdir_data = pl::concat(&h, &subdir_data);
                                        add_to(&subdir_info.get_or_new_hash("Fixup"), "Start", &hdr_len); // (autovivified as in perl)
                                    }
                                    if !len(&subdir_data).truthy() {
                                        if !in_maker_notes {
                                            continue 'subifd;
                                        }
                                        // don't delete MakerNote Sub-IFD's, write empty IFD instead
                                        subdir_data = rep(b"\0", &n(6));
                                        // reset SubIFD ImageData and Fixup just to be safe
                                        subdir_info.delete("ImageData");
                                        subdir_info.delete("Fixup");
                                    }
                                    // handle data blocks that we will transfer later
                                    if subdir_info.get("ImageData").is_ref() {
                                        for b in subdir_info.get("ImageData").items() {
                                            image_data.push(b);
                                        }
                                        orig_dir_info.set("ImageData", image_data.clone());
                                    }
                                    // temporarily set value to subdirectory index
                                    // (will set to actual offset later when we know what it is)
                                    let f = s32u(it, n(0xfeedf00d))?;
                                    put(&new_value, pl::concat(&get(&new_value), &f));
                                    let (offset, where_);
                                    if pl::gt(&read_count, &n(1)) {
                                        offset = pl::add(&len(&val_buff), &pl::mul(&i, &n(4)));
                                        where_ = "valBuff";
                                    } else {
                                        offset = pl::add(&len(&get(&dir_buff)), &n(8));
                                        where_ = "dirBuff";
                                    }
                                    // add to list of subdirectories we will append later
                                    let sd = Value::new_hash();
                                    sd.set("DataPt", pl::sref(subdir_data));
                                    sd.set("Table", sub_table.clone());
                                    sd.set("Fixup", subdir_info.get("Fixup"));
                                    sd.set("Offset", offset);
                                    sd.set("Where", pl::s(where_));
                                    sd.set("ImageData", subdir_info.get("ImageData"));
                                    subdirs.push(sd);
                                    write_count += 1; // count number of subdirs written
                                }
                                if !len(&get(&new_value)).truthy() {
                                    continue 'entry;
                                }
                                // must change location of subdir offset if we deleted
                                // a directory and only one remains
                                if pl::lt(&n(write_count), &read_count) && write_count == 1 {
                                    let last = subdirs.last().cloned().unwrap_or_default();
                                    last.set("Where", pl::s("dirBuff"));
                                    last.set("Offset", pl::add(&len(&get(&dir_buff)), &n(8)));
                                }
                                // set new format to int32u for IFD
                                let ff = new_info.get("FixFormat");
                                new_form_name = if ff.truthy() { ff } else { pl::s("int32u") };
                                new_format = fnum(&new_form_name);
                                new_value_pt = rf(&new_value);
                            } else if (subdir.get("Start").is_undef() || re(r"\$valuePtr", "").is_match(&subdir.get("Start"))) && subdir.get("TagTable").truthy() {
                                //
                                // rewrite other existing subdirectories ('$valuePtr' type only)
                                //
                                // set subdirectory Start and Base
                                let mut subdir_start = value_ptr.clone();
                                if subdir.get("Start").truthy() {
                                    //### eval Start ($valuePtr)
                                    let vars = [("valuePtr", value_ptr.clone()), ("et", et.clone()), ("dirInfo", dir_info.clone())];
                                    subdir_start = eval_at(it, &subdir.get("Start"), MODULE, FILE, &vars, Want::Scalar).0;
                                    // must adjust directory size if start changed
                                    old_size = pl::sub(&old_size, &pl::sub(&subdir_start, &value_ptr));
                                }
                                let mut subdir_base = base.clone();
                                if subdir.get("Base").truthy() {
                                    let start = pl::add(&subdir_start, &value_data_pos);
                                    //### eval Base ($start,$base)
                                    let vars = [("start", start), ("base", base.clone()), ("et", et.clone()), ("dirInfo", dir_info.clone())];
                                    let (b, _) = eval_at(it, &subdir.get("Base"), MODULE, FILE, &vars, Want::Scalar);
                                    subdir_base = pl::add(&subdir_base, &b);
                                }
                                let sub_fixup = fixup_new(it)?;
                                let subdir_info = Value::new_hash();
                                subdir_info.set("Base", subdir_base.clone());
                                subdir_info.set("DataPt", value_data_pt.clone());
                                subdir_info.set("DataPos", pl::add(&pl::sub(&value_data_pos, &subdir_base), &base));
                                subdir_info.set("DataLen", value_data_len.clone());
                                subdir_info.set("DirStart", subdir_start.clone());
                                subdir_info.set("DirName", subdir.get("DirName"));
                                subdir_info.set("DirLen", old_size.clone());
                                subdir_info.set("Parent", dir_name.clone());
                                subdir_info.set("Fixup", sub_fixup.clone());
                                subdir_info.set("RAF", raf.clone());
                                subdir_info.set("TagInfo", new_info.clone());
                                if !old_size.truthy() {
                                    // replace with dummy data if empty to prevent WriteDirectory
                                    // routines from accessing data they shouldn't
                                    subdir_info.set("DataPt", pl::sref(pl::s("")));
                                    subdir_info.set("DataLen", n(0));
                                    subdir_info.set("DirStart", n(0));
                                    add_to(&subdir_info, "DataPos", &subdir_start);
                                }
                                let sub_table = etf(it, "GetTagTable", vec![subdir.get("TagTable")])?;
                                let old_order = byte_order(it)?;
                                if subdir.get("ByteOrder").truthy() {
                                    set_byte_order(it, subdir.get("ByteOrder"))?;
                                }
                                put(&new_value, pl::method(it, &et, "WriteDirectory", vec![subdir_info.clone(), sub_table, subdir.get("WriteProc")])?);
                                set_byte_order(it, old_order)?;
                                if !get(&new_value).is_undef() {
                                    let hdr_len = pl::sub(&subdir_start, &value_ptr);
                                    if hdr_len.truthy() {
                                        let h = substr(it, &value_data_pt.deref(), &value_ptr, Some(&hdr_len))?;
                                        put(&new_value, pl::concat(&h, &get(&new_value)));
                                        add_to(&sub_fixup, "Start", &hdr_len);
                                    }
                                    new_value_pt = rf(&new_value);
                                } else {
                                    new_value_pt = rf(&old_value);
                                }
                                if !len(&new_value_pt.deref()).truthy() {
                                    // don't delete a previously empty makernote directory
                                    if old_size.truthy() || !in_maker_notes {
                                        continue 'entry;
                                    }
                                }
                                if sub_fixup.get("Pointers").truthy() && pl::num_eq(&subdir_info.get("Base"), &base) {
                                    add_to(&sub_fixup, "Start", &len(&val_buff));
                                    val_fixups.push(sub_fixup);
                                } else {
                                    // apply fixup in case we added a header ($hdrLen above)
                                    pl::method(it, &sub_fixup, "ApplyFixup", vec![rf(&new_value)])?;
                                }
                            }
                        } else if new_info.get("OffsetPair").truthy() {
                            //
                            // keep track of offsets
                            //
                            let dt = new_info.get("DataTag");
                            let data_tag = if dt.truthy() { dt } else { pl::s("") };
                            if data_tag.eq_str("CanonVRD") {
                                // must decide now if we will write CanonVRD information
                                let has_vrd;
                                let extra = pl::pkg_hash(it, "Image::ExifTool::Extra");
                                if et.get("NEW_VALUE").get(extra.get("CanonVRD").to_bytes()).truthy() {
                                    // adding or deleting as a block
                                    has_vrd = pl::method(it, &et, "GetNewValue", vec![pl::s("CanonVRD")])?.truthy();
                                } else if et.get("DEL_GROUP").get("CanonVRD").truthy() || et.get("DEL_GROUP").get("Trailer").truthy() {
                                    has_vrd = false; // deleting as a group
                                } else {
                                    has_vrd = !new_value_pt.deref().eq_str("\0\0\0\0");
                                }
                                if has_vrd {
                                    // add a fixup, and set this offset later
                                    pl::method(it, &dir_fixup, "AddFixup", vec![pl::add(&len(&get(&dir_buff)), &n(8)), data_tag.clone()])?;
                                } else {
                                    // there is (or will soon be) no VRD information, so set pointer to zero
                                    put(&new_value, rep(b"\0", &len(&new_value_pt.deref())));
                                    new_value_pt = rf(&new_value);
                                }
                            } else if data_tag.eq_str("OriginalDecisionData") {
                                // handle Canon OriginalDecisionData (no associated length tag)
                                // - I'm going out of my way here to preserve data which is
                                //   invalidated anyway by our edits
                                let mut odd = Value::Undef;
                                let odd_info = pl::call(it, "Image::ExifTool::GetCompositeTagInfo", vec![pl::s("OriginalDecisionData")])?;
                                if odd_info.truthy() && et.get("NEW_VALUE").get(odd_info.to_bytes()).truthy() {
                                    odd = pl::method(it, &et, "GetNewValue", vec![data_tag.clone()])?;
                                    if pl::gt(&verbose, &n(1)) {
                                        if !new_value_pt.deref().eq_str("\0\0\0\0") {
                                            print_out(it, &out, cat(&[pl::s("    - "), dir_name.clone(), pl::s(":"), data_tag.clone(), pl::s("\n")]))?;
                                        }
                                        if odd.truthy() {
                                            print_out(it, &out, cat(&[pl::s("    + "), dir_name.clone(), pl::s(":"), data_tag.clone(), pl::s("\n")]))?;
                                        }
                                    }
                                    inc(&et, "CHANGED");
                                } else if !new_value_pt.deref().eq_str("\0\0\0\0") {
                                    if pl::num_eq(&len(&new_value_pt.deref()), &n(4)) {
                                        crate::runtime::require(it, "Image::ExifTool::Canon")?;
                                        let mut offset = g32u(it, &new_value_pt, n(0))?;
                                        // absolute offset in JPEG images only
                                        if !et.get("FILE_TYPE").eq_str("JPEG") {
                                            offset = pl::add(&offset, &base);
                                        }
                                        odd = pl::call(it, "Image::ExifTool::Canon::ReadODD", vec![et.clone(), offset])?;
                                        if odd.is_ref() {
                                            odd = odd.deref();
                                        }
                                    } else {
                                        pl::method(it, &et, "Error", vec![pl::concat(&pl::s("Invalid "), &new_info.get("Name")), n(1)])?;
                                    }
                                }
                                if odd.truthy() {
                                    let mut new_offset = len(&val_buff);
                                    // (ODD offset is absolute in JPEG, so add base offset!)
                                    if et.get("FILE_TYPE").eq_str("JPEG") {
                                        new_offset = pl::add(&new_offset, &base);
                                    }
                                    put(&new_value, s32u(it, new_offset)?);
                                    pl::method(it, &dir_fixup, "AddFixup", vec![pl::add(&len(&get(&dir_buff)), &n(8)), data_tag.clone()])?;
                                    val_buff = pl::concat(&val_buff, &odd); // add original decision data
                                } else {
                                    put(&new_value, pl::s("\0\0\0\0"));
                                }
                                new_value_pt = rf(&new_value);
                            } else {
                                let mut offset_info = offset_info_list.at(ifd.to_i64());
                                // save original values (for updating TIFF_END later)
                                let vals = Value::new_array(vec![]);
                                if is_new <= 0 {
                                    let old_order = byte_order(it)?;
                                    // Minolta A200 stores these in the wrong byte order!
                                    if new_info.get("ByteOrder").truthy() {
                                        set_byte_order(it, new_info.get("ByteOrder"))?;
                                    }
                                    let v = pl::call_list(it, "Image::ExifTool::ReadValue", vec![rf(&old_value), n(0), read_form_name.clone(), read_count.clone(), old_size.clone()])?;
                                    set_items(&vals, v);
                                    set_byte_order(it, old_order)?;
                                    if !new_info.get("IsOffset").truthy() {
                                        validate_info.set(new_id.to_bytes(), Value::new_array(vec![new_info.clone(), pl::join(" ", &vals.items())]));
                                    }
                                }
                                // only support int32 pointers (for now)
                                if !pl::num_eq(&fsz(&new_format), &n(4)) && new_info.get("IsOffset").truthy() {
                                    if is_new > 0 {
                                        pl::warn(it, "Internal error (Offset not int32)")?;
                                        return Ok(Value::Undef);
                                    }
                                    if !pl::num_eq(&new_count, &read_count) {
                                        pl::warn(it, "Wrong count!")?;
                                        return Ok(Value::Undef);
                                    }
                                    // change to int32
                                    new_form_name = pl::s("int32u");
                                    new_format = fnum(&new_form_name);
                                    put(&new_value, write_value(it, vec![pl::join(" ", &vals.items()), new_form_name.clone(), new_count.clone()])?);
                                    if get(&new_value).is_undef() {
                                        pl::warn(it, &format!("Internal error writing offsets for {}\n", new_info.get("Name").to_text()))?;
                                        return Ok(Value::Undef);
                                    }
                                    new_value_pt = rf(&new_value);
                                }
                                if !offset_info.truthy() {
                                    offset_info = Value::new_hash();
                                    offset_info_list.set_at(ifd.to_i64() as usize, offset_info.clone());
                                }
                                // save location of valuePtr in new directory
                                // (notice we add 10 instead of 8 for valuePtr because
                                // we will put a 2-byte count at start of directory later)
                                let ptr = pl::add(&pl::add(&new_start, &len(&get(&dir_buff))), &n(10));
                                if !new_count.truthy() {
                                    new_count = n(1); // make sure count is set for offsetInfo
                                }
                                // save value pointer and value count for each tag
                                offset_info.set(new_id.to_bytes(), Value::new_array(vec![new_info.clone(), ptr, new_count.clone(), vals, new_format.clone()]));
                            }
                        } else if new_info.get("DataMember").truthy() {
                            // save any necessary data members (Make, Model, etc)
                            let mut format_str = new_form_name.clone();
                            let mut count = new_count.clone();
                            // change to specified format if necessary
                            if new_info.get("Format").truthy() && !seq(&new_info.get("Format"), &format_str) {
                                format_str = new_info.get("Format");
                                let format = fnum(&format_str);
                                // adjust number of items for new format size
                                if format.truthy() {
                                    count = pl::int(&pl::div(&len(&new_value_pt.deref()), &fsz(&format))?);
                                }
                            }
                            let val = read_value(it, vec![new_value_pt.clone(), n(0), format_str, count, len(&new_value_pt.deref())])?;
                            let conv = new_info.get("RawConv");
                            if conv.truthy() {
                                // let the RawConv store the (possibly converted) data member
                                if pl::ref_of(&conv) == "CODE" {
                                    pl::call_code(it, &conv, vec![val, et.clone()], Want::Scalar)?;
                                } else {
                                    let (self_, tag, tag_info) = (et.clone(), new_info.get("Name"), new_info.clone());
                                    //### eval RawConv ($self, $val, $tag, $tagInfo, $priority, @grps)
                                    let vars = [("self", self_), ("val", val), ("tag", tag), ("tagInfo", tag_info), ("priority", Value::Undef), ("@grps", Value::new_array(vec![])), ("et", et.clone())];
                                    eval_at(it, &conv, MODULE, FILE, &vars, Want::Scalar);
                                }
                            } else {
                                et.set(new_info.get("DataMember").to_bytes(), val);
                            }
                        }
                    }
                }
                //
                // write out the directory entry
                //
                let mut new_size = len(&new_value_pt.deref());
                let fsize = fsz(&new_format);
                let mut offset_val;
                // set proper count
                if !(old_info.truthy() && old_info.get("FixedSize").truthy()) {
                    new_count = pl::int(&pl::div(&pl::sub(&pl::add(&new_size, &fsize), &n(1)), &fsize)?);
                }
                if save_for_validate.get(new_id.to_bytes()).truthy() && seq(&tag_table_ptr, &exif_main) {
                    let vals = pl::call_list(it, "Image::ExifTool::ReadValue", vec![rf(&new_value), n(0), new_form_name.clone(), new_count.clone(), new_size.clone()])?;
                    validate_info.set(new_id.to_bytes(), pl::join(" ", &vals));
                }
                if pl::gt(&new_size, &n(4)) {
                    // zero-pad to an even number of bytes (required by EXIF standard)
                    // and make sure we are a multiple of the format size
                    while band(&new_size, 0x01).truthy() || pl::lt(&new_size, &pl::mul(&new_count, &fsize)) {
                        pl::set_deref(&new_value_pt, pl::concat(&new_value_pt.deref(), &pl::s("\0")))?;
                        new_size = pl::add(&new_size, &n(1));
                    }
                    let mut entry_based = false;
                    if dir_info.get("EntryBased").truthy() || (new_info.truthy() && new_info.get("EntryBased").truthy()) {
                        entry_based = true;
                        offset_val = s32u(it, pl::sub(&len(&val_buff), &len(&get(&dir_buff))))?;
                    } else {
                        offset_val = s32u(it, len(&val_buff))?;
                    }
                    let (mut data_tag, mut put_first) = (Value::Undef, Value::Undef);
                    if new_info.truthy() {
                        data_tag = new_info.get("DataTag");
                        put_first = new_info.get("PutFirst");
                    }
                    if data_tag.truthy() {
                        if data_tag.eq_str("PreviewImage") && (et.get("FILE_TYPE").eq_str("JPEG") || et.get("GENERATE_PREVIEW_INFO").truthy()) {
                            // hold onto the PreviewImage until we can determine if it fits
                            if !et.get("PREVIEW_INFO").truthy() {
                                let pi = Value::new_hash();
                                pi.set("Data", new_value_pt.deref());
                                pi.set("Fixup", fixup_new(it)?);
                                et.set("PREVIEW_INFO", pi);
                            }
                            if new_info.get("ChangeBase").truthy() {
                                et.get("PREVIEW_INFO").set("ChangeBase", n(1));
                            }
                            if new_info.get("IsOffset").truthy() && new_info.get("IsOffset").eq_str("2") {
                                et.get("PREVIEW_INFO").set("NoBaseShift", n(1));
                            }
                            // use original preview size if we will attempt to load it later
                            if new_value_pt.deref().eq_str("LOAD_PREVIEW") {
                                new_count = old_count.clone();
                            }
                            pl::set_deref(&new_value_pt, pl::s(""))?;
                        } else if data_tag.eq_str("LeicaTrailer") && et.get("LeicaTrailer").truthy() {
                            pl::set_deref(&new_value_pt, pl::s(""))?;
                        }
                    }
                    if put_first.truthy() && dir_info.get("HeaderPtr").truthy() {
                        let hdr_ptr = dir_info.get("HeaderPtr");
                        // place this value immediately after the TIFF header (eg. IIQ maker notes)
                        offset_val = s32u(it, len(&hdr_ptr.deref()))?;
                        pl::set_deref(&hdr_ptr, pl::concat(&hdr_ptr.deref(), &new_value_pt.deref()))?;
                    } else {
                        val_buff = pl::concat(&val_buff, &new_value_pt.deref()); // add value data to buffer
                        // must save a fixup pointer for every pointer in the directory
                        if entry_based {
                            if !entry_based_fixup.truthy() {
                                entry_based_fixup = fixup_new(it)?;
                            }
                            pl::method(it, &entry_based_fixup, "AddFixup", vec![pl::add(&len(&get(&dir_buff)), &n(8)), data_tag.clone()])?;
                        } else {
                            pl::method(it, &dir_fixup, "AddFixup", vec![pl::add(&len(&get(&dir_buff)), &n(8)), data_tag.clone()])?;
                        }
                    }
                } else {
                    offset_val = new_value_pt.deref(); // save value in offset if 4 bytes or less
                    // must pad value with zeros if less than 4 bytes
                    if pl::lt(&new_size, &n(4)) {
                        offset_val = pl::concat(&offset_val, &rep(b"\0", &pl::sub(&n(4), &new_size)));
                    }
                }
                // write the directory entry
                let e = cat(&[s16u(it, new_id.clone())?, s16u(it, new_format.clone())?, s32u(it, new_count.clone())?, offset_val]);
                put(&dir_buff, pl::concat(&get(&dir_buff), &e));
                // update flag to keep track of mandatory tags
                while !all_mandatory.is_undef() {
                    if !mandatory.get(new_id.to_bytes()).is_undef() {
                        // values must correspond to mandatory values
                        autoviv(&mut new_info);
                        let f = new_info.get("Format");
                        let form = if f.truthy() { f } else { new_form_name.clone() };
                        let mand_val = write_value(it, vec![mandatory.get(new_id.to_bytes()), form, new_count.clone()])?;
                        if !mand_val.is_undef() && seq(&mand_val, &new_value_pt.deref()) {
                            all_mandatory = pl::add(&all_mandatory, &n(1)); // count mandatory tags
                            break;
                        }
                    }
                    delete_all = Value::Undef;
                    all_mandatory = Value::Undef;
                }
            }
            if !validate_info.is_empty() {
                validate_image_data_v(it, &et, &validate_info, &dir_name, &n(1))?;
                clear_hash(&validate_info);
            }
            if ignore_count.truthy() {
                let y = if pl::gt(&ignore_count, &n(1)) { "ies" } else { "y" };
                let verb = if dir_info.get("FixBase").truthy() { "Ignored" } else { "Removed" };
                pl::method(it, &et, "Warn", vec![cat(&[pl::s(verb), pl::s(" "), ignore_count.clone(), pl::s(&format!(" invalid entr{y} from ")), name.clone()]), n(1)])?;
            }
            if fix_count.truthy() {
                let s = if pl::gt(&fix_count, &n(1)) { "s" } else { "" };
                pl::method(it, &et, "Warn", vec![cat(&[pl::s(&format!("Fixed invalid count{s} for ")), fix_count.clone(), pl::s(" "), name.clone(), pl::s(&format!(" tag{s}"))]), n(1)])?;
            }
            //..............................................................................
            // write directory counts and nextIFD pointer and add value data to end of IFD
            //
            // determine now if there is or will be another IFD after this one
            let next_ifd_offset = if pl::le(&pl::add(&dir_end, &n(4)), &data_len) { g32u(it, &data_pt, dir_end.clone())? } else { n(0) };
            let is_next_ifd = dir_info.get("Multi").truthy()
                && (next_ifd_offset.truthy() ||
                // account for the case where we will create the next IFD
                // (IFD1 only, but not in TIFF-format images)
                (dir_name.eq_str("IFD0") && et.get("ADD_DIRS").get("IFD1").truthy() && !et.get("FILE_TYPE").eq_str("TIFF")));
            // calculate number of entries in new directory
            let mut new_entries = pl::div(&len(&get(&dir_buff)), &n(12))?;
            // delete entire directory if we deleted a tag and only mandatory tags remain or we
            // attempted to create a directory with only mandatory tags and there is no nextIFD
            if all_mandatory.truthy() && !is_next_ifd && (pl::lt(&new_entries, &num_entries) || pl::num_eq(&num_entries, &n(0))) {
                new_entries = n(0);
                put(&dir_buff, pl::s(""));
                val_buff = pl::s("");
                dir_fixup = Value::Undef; // no fixups in this directory
                if !delete_all.is_undef() {
                    delete_all = pl::add(&delete_all, &n(1));
                }
                if pl::gt(&verbose, &n(1)) {
                    print_out(it, &out, cat(&[pl::s("    - "), all_mandatory.clone(), pl::s(" mandatory tag(s)\n")]))?;
                }
                et.set("CHANGED", pl::sub(&et.get("CHANGED"), &add_mandatory)); // didn't change these after all
            }
            if ifd.truthy() && !new_entries.truthy() {
                if verbose.truthy() {
                    print_out(it, &out, pl::s("  Deleting IFD1\n"))?;
                }
                break 'ifd; // don't write IFD1 if empty
            }
            // apply one-time fixup for entry-based offsets
            if entry_based_fixup.truthy() {
                entry_based_fixup.set("Shift", pl::add(&len(&get(&dir_buff)), &n(4)));
                pl::method(it, &entry_based_fixup, "ApplyFixup", vec![rf(&dir_buff)])?;
                entry_based_fixup = Value::Undef;
            }
            let _ = entry_based_fixup;
            // initialize next IFD pointer to zero
            let mut next_ifd = s32u(it, n(0))?;
            // some cameras use a different amount of padding after the makernote IFD
            if dir_name.eq_str("MakerNotes") && re(r"^(ExifIFD|IFD0)$", "").is_match(&dir_info.get("Parent")) {
                let r = pl::call_list(it, "Image::ExifTool::MakerNotes::GetMakerNoteOffset", vec![et.clone()])?;
                let pad = arg(&r, 1);
                if !pad.is_undef() && (pl::num_eq(&pad, &n(0)) || (pl::gt(&pad, &n(4)) && pl::le(&pad, &n(32)))) {
                    next_ifd = rep(b"\0", &pad);
                }
            }
            // add directory entry count to start of IFD and next IFD pointer to end
            let e = cat(&[s16u(it, new_entries)?, get(&dir_buff), next_ifd.clone()]);
            put(&new_data, pl::concat(&get(&new_data), &e));
            // get position of value data in newData
            let val_pos = len(&get(&new_data));
            // go back now and set next IFD pointer if this isn't the first IFD
            if next_ifd_pos.truthy() {
                // set offset to next IFD
                s32u_at(it, new_start.clone(), &rf(&new_data), next_ifd_pos.clone())?;
                pl::method(it, &fixup, "AddFixup", vec![next_ifd_pos.clone(), pl::s("NextIFD")])?; // add fixup for this offset in newData
            }
            // remember position of 'next IFD' pointer so we can set it next time around
            next_ifd_pos = if len(&next_ifd).truthy() { pl::sub(&val_pos, &len(&next_ifd)) } else { Value::Undef };
            // add value data after IFD
            put(&new_data, pl::concat(&get(&new_data), &val_buff));
            //
            // add any subdirectories, adding fixup information
            //
            for subdir in &subdirs {
                let l = len(&get(&new_data)); // position of subdirectory in data
                let subdir_fixup = subdir.get("Fixup");
                if subdir_fixup.truthy() {
                    add_to(&subdir_fixup, "Start", &l);
                    pl::method(it, &fixup, "AddFixup", vec![subdir_fixup.clone()])?;
                }
                let image_data_s = subdir.get("ImageData");
                let mut block_size = n(0);
                // must also update start position for ImageData fixups
                if image_data_s.is_ref() {
                    for block_info in image_data_s.items() {
                        let (_pos, size, pad, _entry, sub_fix) = (block_info.at(0), block_info.at(1), block_info.at(2), block_info.at(3), block_info.at(4));
                        if sub_fix.truthy() {
                            add_to(&sub_fix, "Start", &l);
                            // save expected image data offset for calculating shift later
                            sub_fix.set("BlockLen", pl::add(&len(&subdir.get("DataPt").deref()), &block_size));
                        }
                        block_size = pl::add(&block_size, &pl::add(&size, &pad));
                    }
                }
                put(&new_data, pl::concat(&get(&new_data), &subdir.get("DataPt").deref())); // add subdirectory to our data
                pl::set_deref(&subdir.get("DataPt"), Value::Undef)?; // free memory now
                // set the pointer
                let mut offset = subdir.get("Offset");
                // if offset is in valBuff, it was added to the end of dirBuff
                // (plus 4 bytes for nextIFD pointer)
                if subdir.get("Where").eq_str("valBuff") {
                    offset = pl::add(&offset, &pl::add(&len(&get(&dir_buff)), &n(4)));
                }
                offset = pl::add(&offset, &pl::add(&new_start, &n(2))); // get offset in newData
                // check to be sure we got the right offset
                if !pl::num_eq(&g32u(it, &rf(&new_data), offset.clone())?, &n(0xfeedf00d)) {
                    pl::method(it, &et, "Error", vec![pl::concat(&pl::s("Internal error while rewriting "), &name)])?;
                    return Ok(Value::Undef);
                }
                // set the offset to the subdirectory data
                s32u_at(it, l, &rf(&new_data), offset.clone())?;
                pl::method(it, &fixup, "AddFixup", vec![offset])?; // add fixup for this offset in newData
            }
            // add fixup for all offsets in directory according to value data position
            // (which is at the end of this directory)
            if dir_fixup.truthy() {
                dir_fixup.set("Start", pl::add(&new_start, &n(2)));
                dir_fixup.set("Shift", pl::sub(&val_pos, &dir_fixup.get("Start")));
                pl::method(it, &fixup, "AddFixup", vec![dir_fixup.clone()])?;
            }
            // add valueData fixups, adjusting for position of value data
            for val_fixup in &val_fixups {
                add_to(val_fixup, "Start", &val_pos);
                pl::method(it, &fixup, "AddFixup", vec![val_fixup.clone()])?;
            }
            // stop if no next IFD pointer
            if !is_next_ifd {
                break 'ifd; // stop unless scanning for multiple IFD's
            }
            if next_ifd_offset.truthy() {
                // continue with next IFD
                dir_start = pl::sub(&next_ifd_offset, &data_pos);
            } else {
                // create IFD1 if necessary
                if verbose.truthy() {
                    print_out(it, &out, pl::s("  Creating IFD1\n"))?;
                }
                data_pt = pl::sref(rep(b"\0", &n(2))); // empty IFD1 data (zero entry count)
                dir_start = n(0);
                data_len = n(2);
                dir_len = n(2);
            }
            // increment IFD name
            let mut ifd_num = n(0);
            {
                let mut c1 = Value::Undef;
                if re(r"(\d+)$", "").subst(&mut dir_name, false, |c| {
                    c1 = cap(c, 1);
                    Vec::new()
                }) > 0
                {
                    ifd_num = c1;
                }
            }
            dir_name = pl::concat(&dir_name, &pl::add(&ifd_num, &n(1)));
            re(r"\d+$", "").subst(&mut name, false, |_| Vec::new());
            name = pl::concat(&name, &pl::add(&ifd_num, &n(1)));
            et.set("DIR_NAME", dir_name.clone());
            let path = et.get("PATH");
            let pl_len = path.len();
            if pl_len > 0 {
                path.set_at(pl_len - 1, dir_name.clone());
            } else {
                return die("Modification of non-creatable array value attempted, subscript -1");
            }
            if !next_ifd_offset.truthy() {
                break 'ifd_body;
            }

            // guard against writing the same directory twice
            let addr = pl::add(&next_ifd_offset, &base);
            if et.get("PROCESSED").get(addr.to_bytes()).truthy() {
                let msg = cat(&[name.clone(), pl::s(" pointer references previous "), et.get("PROCESSED").get(addr.to_bytes()), pl::s(" directory")]);
                pl::method(it, &et, "Error", vec![msg, n(1)])?;
                break 'ifd;
            }
            et.get_or_new_hash("PROCESSED").set(addr.to_bytes(), name.clone());

            if dir_name.eq_str("SubIFD1") && !validate_ifd_v(it, &dir_info, dir_start.clone())? {
                if et.get("TIFF_TYPE").eq_str("TIFF") {
                    pl::method(it, &et, "Error", vec![pl::s("Ignored bad IFD linked from SubIFD"), n(1)])?;
                } else if verbose.truthy() {
                    pl::method(it, &et, "Warn", vec![pl::s("Ignored bad IFD linked from SubIFD")])?;
                }
                break 'ifd; // don't write bad IFD
            }
            if et.get("DEL_GROUP").get(dir_name.to_bytes()).truthy() {
                if verbose.truthy() {
                    print_out(it, &out, cat(&[pl::s("  Deleting "), dir_name.clone(), pl::s("\n")]))?;
                }
                if raf.truthy() {
                    pl::method(it, &et, "Error", vec![cat(&[pl::s("Deleting "), dir_name.clone(), pl::s(" also deletes subsequent IFD's and possibly image data")]), n(1)])?;
                }
                inc(&et, "CHANGED");
                if pl::num_eq(&et.get("DEL_GROUP").get(dir_name.to_bytes()), &n(2)) && et.get("ADD_DIRS").get(dir_name.to_bytes()).truthy() {
                    data_pt = pl::sref(rep(b"\0", &n(2))); // start with empty IFD
                    dir_start = n(0);
                    data_len = n(2);
                    dir_len = n(2);
                } else {
                    break 'ifd; // don't write this IFD (or any subsequent IFD)
                }
            } else if verbose.truthy() {
                print_out(it, &out, cat(&[pl::s("  Rewriting "), name.clone(), pl::s("\n")]))?;
            }
        }
        ifd = pl::add(&ifd, &n(1));
    }
    //~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

    // do our fixups now so we can more easily calculate offsets below
    pl::method(it, &fixup, "ApplyFixup", vec![rf(&new_data)])?;
    // write Sony HiddenData now if this is an ARW file
    if et.get("HiddenData").truthy() && !dir_info.get("Fixup").truthy() && et.get("FILE_TYPE").eq_str("TIFF") {
        let l = len(&get(&new_data));
        pl::method(it, &fixup, "SetMarkerPointers", vec![rf(&new_data), pl::s("HiddenData"), l])?;
        let hbuf = cell(Value::Undef);
        let hd = et.get("HiddenData");
        if seek(it, &raf, hd.get("Offset"), 0)? && read_eq(it, &raf, &hbuf, &hd.get("Size"))? && re(r"^\x55\x26\x11\x05\0", "").is_match(&get(&hbuf)) {
            put(&new_data, pl::concat(&get(&new_data), &get(&hbuf)));
        } else {
            pl::method(it, &et, "Error", vec![pl::s("Error copying hidden data"), n(1)])?;
        }
    }
    //
    // determine total block size for deferred data
    //
    let mut num_blocks = image_data.len() as i64; // save this so we scan only existing blocks later
    let mut block_size = n(0); // total size of blocks to copy later
    for block_info in image_data.items() {
        block_size = pl::add(&block_size, &pl::add(&block_info.at(1), &block_info.at(2)));
    }
    //
    // copy over image data for IFD's, starting with the last IFD first
    //
    if !offset_info_list.is_empty() {
        let mut ttw_len = Value::Undef; // length of MRW TTW segment
        let mut write_later: Vec<Value> = Vec::new(); // write image data last
        let mut ifd = offset_info_list.len() as i64 - 1;
        while ifd >= -1 {
            let ifd_cur = ifd;
            ifd -= 1;
            let ifd = ifd_cur;
            // build list of offsets to process
            let mut offset_list: Vec<Value> = Vec::new();
            if ifd >= 0 {
                let dn = dir_info.get("DirName");
                dir_name = if dn.truthy() { dn } else { pl::s("unknown") };
                if ifd != 0 {
                    re(r"\d+$", "").subst(&mut dir_name, false, |_| Vec::new());
                    dir_name = pl::concat(&dir_name, &n(ifd));
                }
                let offset_info = offset_info_list.at(ifd);
                if !offset_info.truthy() {
                    continue;
                }
                if offset_info.get("273").truthy() && offset_info.get("324").truthy() {
                    // SubIFD may contain double-referenced data as both strips and tiles
                    // for Sony ARW files when SonyRawFileType is "Lossless Compressed RAW 2"
                    if dir_name.eq_str("SubIFD")
                        && et.get("TIFF_TYPE").eq_str("ARW")
                        && offset_info.get("279").truthy()
                        && offset_info.get("325").truthy()
                        && pl::num_eq(&offset_info.get("273").at(2), &n(1))
                    {
                        // (must be a single strip or the tile offsets could get out of sync)
                        // check the start offsets to see if they are the same
                        if pl::num_eq(&offset_info.get("273").at(3).at(0), &offset_info.get("324").at(3).at(0)) {
                            // some Sony ARW images contain double-referenced raw data stored as both strips
                            // and tiles.  Copy the data using only the strip tags, but store the TileOffets
                            // information for updating later (see PanasonicRaw:PatchRawDataOffset for a
                            // description of offsetInfo elements)
                            offset_info.get("273").set_at(5, offset_info.get("324")); // hack to save TileOffsets
                            // delete tile information from offsetInfo because we will copy as strips
                            offset_info.delete("324");
                            offset_info.delete("325");
                        }
                    } else {
                        pl::method(it, &et, "Error", vec![cat(&[pl::s("TIFF "), dir_name.clone(), pl::s(" contains both strip and tile data")])])?;
                    }
                }
                // patch Panasonic RAW/RW2 StripOffsets/StripByteCounts if necessary
                let strip_offsets = offset_info.get("273");
                let raw_data_offset = offset_info.get("280");
                if (strip_offsets.truthy() && strip_offsets.at(0).get("PanasonicHack").truthy()) || (raw_data_offset.truthy() && raw_data_offset.at(0).get("PanasonicHack").truthy()) {
                    crate::runtime::require(it, "Image::ExifTool::PanasonicRaw")?;
                    let err = pl::call(it, "Image::ExifTool::PanasonicRaw::PatchRawDataOffset", vec![offset_info.clone(), raf.clone(), n(ifd)])?;
                    if err.truthy() {
                        pl::method(it, &et, "Error", vec![err])?;
                    }
                }
                // loop through all tags in reverse numerical order so we save thumbnail
                // data before main image data if both exist in the same IFD
                let mut ids = keys(&offset_info);
                sort_num(&mut ids);
                ids.reverse();
                for tag_id in ids {
                    let tag_info = offset_info.get(tag_id.to_bytes()).at(0);
                    if !tag_info.get("IsOffset").truthy() {
                        continue; // handle byte counts with offsets
                    }
                    let size_info = offset_info.get(tag_info.get("OffsetPair").to_bytes());
                    if !size_info.truthy() {
                        pl::method(it, &et, "Error", vec![cat(&[pl::s("No size tag for "), dir_name.clone(), pl::s(":"), tag_info.get("Name")])])?;
                        continue;
                    }
                    let data_tag = tag_info.get("DataTag");
                    // write TIFF image data (strips or tiles) later if requested
                    if raf.truthy()
                        && !orig_dir_info.get("ImageData").is_undef()
                        && (pl::num_eq(&tag_id, &n(0x111)) || pl::num_eq(&tag_id, &n(0x144)) ||
                            // also defer writing of other big data such as JpgFromRaw in NEF
                            (size_info.at(3).at(0).truthy() &&
                             // (calculate approximate combined size of all blocks)
                             pl::gt(&pl::mul(&size_info.at(3).at(0), &n(size_info.at(3).len() as i64)), &n(1000000))))
                        &&
                        // but don't defer writing if replacing with new value
                        (data_tag.is_undef() || offset_data.get(data_tag.to_bytes()).is_undef())
                    {
                        write_later.push(Value::new_array(vec![offset_info.get(tag_id.to_bytes()), size_info]));
                    } else {
                        offset_list.push(Value::new_array(vec![offset_info.get(tag_id.to_bytes()), size_info]));
                    }
                }
            } else {
                if write_later.is_empty() {
                    break;
                }
                // finally, copy all deferred data
                offset_list = write_later.clone();
            }
            for offset_pair in &offset_list {
                let p0 = offset_pair.at(0);
                let p1 = offset_pair.at(1);
                let (tag_info, mut offsets, count, old_offset) = (p0.at(0), p0.at(1), p0.at(2), p0.at(3));
                let (cnt_info, mut byte_counts, count2, old_size, format) = (p1.at(0), p1.at(1), p1.at(2), p1.at(3), p1.at(4));
                // must be the same number of offset and byte count values
                if !pl::num_eq(&count, &count2) {
                    pl::method(it, &et, "Error", vec![pl::concat(&pl::s("Offsets/ByteCounts disagree on count for "), &tag_info.get("Name"))])?;
                    return Ok(Value::Undef);
                }
                let format_str = format_name.at(format.to_i64());
                // follow pointer to value data if necessary
                if pl::gt(&count, &n(1)) {
                    offsets = g32u(it, &rf(&new_data), offsets.clone())?;
                }
                let nn = pl::mul(&count, &fsz(&format));
                if pl::gt(&nn, &n(4)) {
                    byte_counts = g32u(it, &rf(&new_data), byte_counts.clone())?;
                }
                if pl::lt(&byte_counts, &n(0)) || pl::gt(&pl::add(&byte_counts, &nn), &len(&get(&new_data))) {
                    pl::method(it, &et, "Error", vec![cat(&[pl::s("Error reading "), tag_info.get("Name"), pl::s(" byte counts")])])?;
                    return Ok(Value::Undef);
                }
                // get offset base and data pos (abnormal for some preview images)
                let (mut dbase, mut dpos, wrong_base);
                let mut sub_ifd_data_fixup = Value::Undef;
                if tag_info.get("IsOffset").eq_str("2") {
                    dbase = first_base.clone();
                    dpos = pl::sub(&pl::add(&data_pos, &base), &first_base);
                } else {
                    dbase = base.clone();
                    dpos = data_pos.clone();
                }
                // use different base if necessary for some offsets (Minolta A200)
                if tag_info.get("WrongBase").truthy() {
                    let self_ = et.clone();
                    //### eval WrongBase ($self)
                    let vars = [("self", self_), ("et", et.clone())];
                    let (w, _) = eval_at(it, &tag_info.get("WrongBase"), MODULE, FILE, &vars, Want::Scalar);
                    wrong_base = or0(w);
                    dbase = pl::add(&dbase, &wrong_base);
                    dpos = pl::sub(&dpos, &wrong_base);
                } else {
                    wrong_base = n(0);
                }
                let old_order = byte_order(it)?;
                let data_tag = tag_info.get("DataTag");
                // use different byte order for values of this offset pair if required (Minolta A200)
                if tag_info.get("ByteOrder").truthy() {
                    set_byte_order(it, tag_info.get("ByteOrder"))?;
                }
                // transfer the data referenced by all offsets of this tag
                let mut nn = n(0);
                'blk: while pl::lt(&nn, &count) {
                    let n_cur = nn.clone();
                    nn = pl::add(&nn, &n(1));
                    let nn = n_cur;
                    let (mut old_end, mut size) = (Value::Undef, Value::Undef);
                    if !old_offset.is_empty() && !old_size.is_empty() {
                        // calculate end offset of this block
                        old_end = pl::add(&old_offset.at(nn.to_i64()), &old_size.at(nn.to_i64()));
                        // update TIFF_END as if we read this data from file
                        update_tiff_end_v(&et, &pl::add(&old_end, &dbase));
                    }
                    let offset_pos = pl::add(&offsets, &pl::mul(&nn, &n(4)));
                    let byte_count_pos = pl::add(&byte_counts, &pl::mul(&nn, &fsz(&format)));
                    if tag_info.get("PanasonicHack").truthy() {
                        // use actual raw data length (may be different than StripByteCounts!)
                        size = old_size.at(nn.to_i64());
                    } else {
                        // use size of new data
                        size = read_value(it, vec![rf(&new_data), byte_count_pos.clone(), format_str.clone(), n(1), n(4)])?;
                    }
                    let mut offset = old_offset.at(nn.to_i64());
                    if !offset.is_undef() {
                        offset = pl::sub(&offset, &dpos);
                    } else if !pl::num_eq(&size, &n(0xfeedfeed)) {
                        pl::method(it, &et, "Error", vec![pl::s("Internal error (no offset)")])?;
                        return Ok(Value::Undef);
                    }
                    let mut new_offset = pl::sub(&len(&get(&new_data)), &wrong_base);
                    let buff = cell(Value::Undef);
                    // look for 'feed' code to use our new data
                    if pl::num_eq(&size, &n(0xfeedfeed)) {
                        if data_tag.is_undef() {
                            pl::method(it, &et, "Error", vec![pl::concat(&pl::s("No DataTag defined for "), &tag_info.get("Name"))])?;
                            return Ok(Value::Undef);
                        }
                        if offset_data.get(data_tag.to_bytes()).is_undef() {
                            pl::method(it, &et, "Error", vec![cat(&[pl::s("Internal error (no "), data_tag.clone(), pl::s(")")])])?;
                            return Ok(Value::Undef);
                        }
                        if pl::gt(&count, &n(1)) {
                            pl::method(it, &et, "Error", vec![cat(&[pl::s("Can't modify "), tag_info.get("Name"), pl::s(" with count "), count.clone()])])?;
                            return Ok(Value::Undef);
                        }
                        put(&buff, offset_data.get(data_tag.to_bytes()));
                        if !pl::num_eq(&fsz(&format), &n(4)) {
                            pl::method(it, &et, "Error", vec![pl::concat(&cnt_info.get("Name"), &pl::s(" is not int32"))])?;
                            return Ok(Value::Undef);
                        }
                        // set the data size
                        size = len(&get(&buff));
                        s32u_at(it, size.clone(), &rf(&new_data), byte_count_pos.clone())?;
                    } else if ifd < 0 {
                        // hack for fixed-offset data (Panasonic GH6)
                        if p0.at(6).truthy() {
                            if pl::gt(&count, &n(1)) {
                                pl::method(it, &et, "Error", vec![pl::s("Can't handle fixed offsets with count > 1")])?;
                            } else {
                                let fixed_offset = g32u(it, &rf(&new_data), offsets.clone())?;
                                let mut pad_to_fixed_offset = pl::sub(&fixed_offset, &pl::add(&new_offset, &dpos));
                                // account for blocks that come before this (Pansonic DC-S1RM2, forum17319)
                                for b in image_data.items() {
                                    pad_to_fixed_offset = pl::sub(&pad_to_fixed_offset, &pl::add(&b.at(1), &b.at(2)));
                                }
                                if pl::lt(&pad_to_fixed_offset, &n(0)) {
                                    pl::method(it, &et, "Error", vec![pl::s("Metadata too large to fit before fixed-offset image data")])?;
                                } else {
                                    // add necessary padding before raw data
                                    image_data.push(Value::new_array(vec![pl::add(&pl::add(&offset, &dbase), &dpos), n(0), pad_to_fixed_offset.clone()]));
                                    new_offset = pl::add(&new_offset, &pad_to_fixed_offset);
                                    pl::method(it, &et, "Warn", vec![cat(&[pl::s("Adding "), pad_to_fixed_offset, pl::s(" bytes of padding before fixed-offset image data")]), n(1)])?;
                                }
                            }
                        }
                        // pad if necessary (but don't pad contiguous image blocks)
                        let mut pad = n(0);
                        let n1 = pl::add(&nn, &n(1));
                        if band(&pl::add(&block_size, &size), 0x01).truthy() && (pl::ge(&n1, &count) || !old_end.truthy() || !pl::num_eq(&old_end, &old_offset.at(n1.to_i64()))) {
                            pad = pl::add(&pad, &n(1));
                        }
                        // preserve original image padding if specified
                        if orig_dir_info.get("PreserveImagePadding").truthy() && pl::lt(&n1, &count) && old_end.truthy() && pl::gt(&old_offset.at(n1.to_i64()), &old_end) {
                            pad = pl::sub(&old_offset.at(n1.to_i64()), &old_end);
                        }
                        // copy data later
                        image_data.push(Value::new_array(vec![pl::add(&pl::add(&offset, &dbase), &dpos), size.clone(), pad.clone()]));
                        new_offset = pl::add(&new_offset, &block_size); // data comes after other deferred data
                        // create fixup for SubIFD ImageData
                        if image_data_flag.eq_str("SubIFD") && !sub_ifd_data_fixup.truthy() {
                            sub_ifd_data_fixup = fixup_new(it)?;
                            image_data.at(image_data.len() as i64 - 1).set_at(4, sub_ifd_data_fixup.clone());
                        }
                        size = pl::add(&size, &pad); // account for pad byte if necessary
                        // return ImageData list
                        orig_dir_info.set("ImageData", image_data.clone());
                    } else if pl::ge(&offset, &n(0)) && pl::le(&pl::add(&offset, &size), &data_len) {
                        // take data from old dir data buffer
                        put(&buff, substr(it, &data_pt.deref(), &offset, Some(&size))?);
                    } else if et.get("TIFF_TYPE").eq_str("MRW") {
                        // TTW segment must be an even 4 bytes long, so pad now if necessary
                        let ln = len(&get(&new_data));
                        let r = band(&ln, 0x03);
                        put(&buff, if r.truthy() { rep(b"\0", &pl::sub(&n(4), &r)) } else { pl::s("") });
                        size = len(&get(&buff));
                        // data exists after MRW TTW segment
                        if ttw_len.is_undef() {
                            ttw_len = pl::add(&len(&get(&new_data)), &size);
                        }
                        new_offset = pl::sub(&pl::add(&pl::add(&offset, &dpos), &ttw_len), &data_len);
                    } else if raf.truthy() && seek(it, &raf, pl::add(&pl::add(&offset, &dbase), &dpos), 0)? && read_eq(it, &raf, &buff, &size)? {
                        // (data was read OK)
                        // patch incorrect ThumbnailOffset in Sony A100 1.00 ARW images
                        if et.get("TIFF_TYPE").eq_str("ARW") && tag_info.get("Name").eq_str("ThumbnailOffset") && et.get("Model").eq_str("DSLR-A100") && !re(r"^\xff\xd8\xff", "").is_match(&get(&buff)) {
                            let pos = pl::add(&pl::add(&offset, &dbase), &dpos);
                            let try_ = cell(Value::Undef);
                            if pl::lt(&pos, &n(0x10000)) && seek(it, &raf, pl::add(&pos, &n(0x10000)), 0)? && read_eq(it, &raf, &try_, &size)? && re(r"^\xff\xd8\xff", "").is_match(&get(&try_)) {
                                put(&buff, get(&try_));
                                pl::method(it, &et, "Warn", vec![pl::s("Adjusted incorrect A100 ThumbnailOffset"), n(1)])?;
                            } else {
                                pl::method(it, &et, "Error", vec![pl::s("Invalid ThumbnailImage")])?;
                            }
                        }
                    } else if tag_info.get("Name").eq_str("ThumbnailOffset") && pl::ge(&offset, &n(0)) && pl::lt(&offset, &data_len) {
                        // Grrr.  The Canon 350D writes the thumbnail with an incorrect byte count
                        let diff = pl::sub(&pl::add(&offset, &size), &data_len);
                        pl::method(it, &et, "Warn", vec![cat(&[pl::s("ThumbnailImage runs outside EXIF data by "), diff.clone(), pl::s(" bytes (truncated)")]), n(1)])?;
                        // set the size to the available data
                        size = pl::sub(&size, &diff);
                        if !write_value(it, vec![size.clone(), format_str.clone(), n(1), rf(&new_data), byte_count_pos.clone()])?.truthy() {
                            pl::warn(it, "Internal error writing thumbnail size")?;
                        }
                        // get the truncated image
                        put(&buff, substr(it, &data_pt.deref(), &offset, Some(&size))?);
                    } else if tag_info.get("Name").eq_str("PreviewImageStart") && et.get("FILE_TYPE").eq_str("JPEG") {
                        // try to load the preview image using the specified offset
                        put(&buff, Value::Undef);
                        let r = et.get("RAF");
                        if r.truthy() && !raf.truthy() {
                            let tl = tell(it, &r)?;
                            // read and validate
                            if !(seek(it, &r, pl::add(&pl::add(&offset, &base), &data_pos), 0)? && read_eq(it, &r, &buff, &size)? && re(r"^.\xd8\xff[\xc4\xdb\xe0-\xef]", "s").is_match(&get(&buff))) {
                                put(&buff, Value::Undef);
                            }
                            if !seek(it, &r, tl, 0)? {
                                pl::method(it, &et, "Error", vec![pl::s("Seek error")])?;
                                return Ok(Value::Undef);
                            }
                        }
                        // set flag if we must load PreviewImage
                        if get(&buff).is_undef() {
                            put(&buff, pl::s("LOAD_PREVIEW"));
                        }
                    } else {
                        let data_name = if data_tag.truthy() { data_tag.clone() } else { tag_info.get("Name") };
                        if pl::method(it, &et, "Error", vec![cat(&[pl::s("Error reading "), data_name, pl::s(" data in "), name.clone()]), n(in_maker_notes as i64)])?.truthy() {
                            return Ok(Value::Undef);
                        }
                        put(&buff, pl::s(""));
                    }
                    if tag_info.get("Name").eq_str("PreviewImageStart") {
                        if et.get("FILE_TYPE").eq_str("JPEG") && !tag_info.get("MakerPreview").truthy() {
                            if size.truthy() {
                                // hold onto the PreviewImage until we can determine if it fits
                                if !et.get("PREVIEW_INFO").truthy() {
                                    let pi = Value::new_hash();
                                    pi.set("Data", get(&buff));
                                    pi.set("Fixup", fixup_new(it)?);
                                    et.set("PREVIEW_INFO", pi);
                                }
                                if tag_info.get("IsOffset").truthy() && tag_info.get("IsOffset").eq_str("2") {
                                    et.get("PREVIEW_INFO").set("NoBaseShift", n(1));
                                }
                                if pl::ge(&offset, &n(0)) && pl::le(&pl::add(&offset, &size), &data_len) {
                                    // set flag indicating this preview wasn't in a trailer
                                    et.get("PREVIEW_INFO").set("WasContained", n(1));
                                }
                            }
                            put(&buff, pl::s(""));
                        } else if et.get("TIFF_TYPE").eq_str("ARW") && et.get("Model").eq_str("DSLR-A100") {
                            // the A100 double-references the same preview, so ignore the
                            // second one (the offset and size will be patched later)
                            if et.get("A100PreviewLength").truthy() {
                                continue 'blk;
                            }
                            if !get(&buff).is_undef() {
                                et.set("A100PreviewLength", len(&get(&buff)));
                            }
                        }
                    }
                    // update offset accordingly and add to end of new data
                    s32u_at(it, new_offset.clone(), &rf(&new_data), offset_pos.clone())?;
                    // add a pointer to fix up this offset value (marked with DataTag name)
                    pl::method(it, &fixup, "AddFixup", vec![offset_pos.clone(), data_tag.clone()])?;
                    // also add to subIfdDataFixup if necessary
                    if sub_ifd_data_fixup.truthy() {
                        pl::method(it, &sub_ifd_data_fixup, "AddFixup", vec![offset_pos.clone(), data_tag.clone()])?;
                    }
                    // must also (sometimes) update StripOffsets in Panasonic RW2 images
                    // and TileOffsets in Sony ARW images
                    let other_pos = p0.at(5);
                    if other_pos.truthy() {
                        if tag_info.get("PanasonicHack").truthy() {
                            s32u_at(it, new_offset.clone(), &rf(&new_data), other_pos.clone())?;
                            pl::method(it, &fixup, "AddFixup", vec![other_pos.clone(), data_tag.clone()])?;
                        } else if pl::ref_of(&other_pos) == "ARRAY" {
                            // the image data was copied as one large strip, and is double-referenced
                            // as tile data, so all we need to do now is properly update the tile offsets
                            let old_raw_data_offset = p0.at(3).at(0);
                            let cnt = other_pos.at(2);
                            // point to offsets in value data if more than one pointer
                            if pl::gt(&cnt, &n(1)) {
                                let v = g32u(it, &rf(&new_data), other_pos.at(1))?;
                                other_pos.set_at(1, v);
                            }
                            let mut i = n(0);
                            while pl::lt(&i, &cnt) {
                                let old_tile_offset = other_pos.at(3).at(i.to_i64());
                                let ptr_pos = pl::add(&other_pos.at(1), &pl::mul(&n(4), &i));
                                s32u_at(it, pl::sub(&pl::add(&new_offset, &old_tile_offset), &old_raw_data_offset), &rf(&new_data), ptr_pos.clone())?;
                                pl::method(it, &fixup, "AddFixup", vec![ptr_pos.clone(), data_tag.clone()])?;
                                if sub_ifd_data_fixup.truthy() {
                                    pl::method(it, &sub_ifd_data_fixup, "AddFixup", vec![ptr_pos, data_tag.clone()])?;
                                }
                                i = pl::add(&i, &n(1));
                            }
                        }
                    }
                    if ifd >= 0 {
                        // buff length must be even (Note: may have changed since $size was set)
                        if band(&len(&get(&buff)), 0x01).truthy() {
                            put(&buff, pl::concat(&get(&buff), &pl::s("\0")));
                        }
                        put(&new_data, pl::concat(&get(&new_data), &get(&buff))); // add this strip to the data
                    } else {
                        block_size = pl::add(&block_size, &size); // keep track of total size
                    }
                }
                set_byte_order(it, old_order)?;
            }
        }
        // verify that nothing else got written after determining TTW length
        if !ttw_len.is_undef() && !pl::num_eq(&ttw_len, &len(&get(&new_data))) {
            pl::method(it, &et, "Error", vec![pl::s("Internal error writing MRW TTW")])?;
        }
    }
    //
    // set offsets and generate fixups for tag values which were too large for memory
    //
    block_size = n(0);
    for block_info in image_data.items() {
        let (_pos, size, pad, entry, sub_fix) = (block_info.at(0), block_info.at(1), block_info.at(2), block_info.at(3), block_info.at(4));
        if !entry.is_undef() {
            let format = g16u(it, &rf(&new_data), pl::add(&entry, &n(2)))?;
            if pl::lt(&format, &n(1)) || pl::gt(&format, &n(13)) {
                pl::method(it, &et, "Error", vec![pl::s("Internal error copying huge value")])?;
                break;
            } else {
                // set count and offset in directory entry
                s32u_at(it, pl::div(&size, &fsz(&format))?, &rf(&new_data), pl::add(&entry, &n(4)))?;
                s32u_at(it, pl::add(&len(&get(&new_data)), &block_size), &rf(&new_data), pl::add(&entry, &n(8)))?;
                pl::method(it, &fixup, "AddFixup", vec![pl::add(&entry, &n(8))])?;
                // create special fixup for SubIFD data
                if image_data_flag.eq_str("SubIFD") {
                    let sub_ifd_data_fixup = fixup_new(it)?;
                    pl::method(it, &sub_ifd_data_fixup, "AddFixup", vec![pl::add(&entry, &n(8))])?;
                    // save fixup in imageData list
                    block_info.set_at(4, sub_ifd_data_fixup);
                }
                // must reset entry pointer so we don't use it again in a parent IFD!
                block_info.set_at(3, Value::Undef);
            }
        }
        // apply additional shift required for contained SubIFD image data offsets
        if sub_fix.truthy() && !sub_fix.get("BlockLen").is_undef() && num_blocks > 0 {
            // our offset expects the data at the end of the SubIFD block (BlockLen + Start),
            // but it will actually be at length($newData) + $blockSize.  So adjust
            // accordingly (and subtract an extra Start because this shift is applied later)
            let sh = pl::add(&pl::sub(&pl::sub(&len(&get(&new_data)), &sub_fix.get("BlockLen")), &pl::mul(&n(2), &sub_fix.get("Start"))), &block_size);
            add_to(&sub_fix, "Shift", &sh);
            pl::method(it, &sub_fix, "ApplyFixup", vec![rf(&new_data)])?;
        }
        block_size = pl::add(&block_size, &pl::add(&size, &pad));
        num_blocks -= 1;
    }
    //
    // apply final shift to new data position if this is the top level IFD
    //
    if !dir_info.get("Fixup").truthy() {
        let hdr_ptr = dir_info.get("HeaderPtr");
        let new_data_pos = if hdr_ptr.truthy() { len(&hdr_ptr.deref()) } else { or0(dir_info.get("NewDataPos")) };
        // adjust CanonVRD offset to point to end of regular TIFF if necessary
        // (NOTE: This will be incorrect if multiple trailers exist,
        //  but it is unlikely that it could ever be correct in this case anyway.
        //  Also, this doesn't work for JPEG images (but CanonDPP doesn't set
        //  this when editing JPEG images anyway))
        let l = pl::add(&len(&get(&new_data)), &block_size);
        pl::method(it, &fixup, "SetMarkerPointers", vec![rf(&new_data), pl::s("CanonVRD"), l])?;
        if new_data_pos.truthy() {
            add_to(&fixup, "Shift", &new_data_pos);
            pl::method(it, &fixup, "ApplyFixup", vec![rf(&new_data)])?;
        }
        // save fixup for adjusting Leica trailer and Sony HiddenData offsets if necessary
        if et.get("LeicaTrailer").truthy() {
            pl::method(it, &et.get("LeicaTrailer").get("Fixup"), "AddFixup", vec![fixup.clone()])?;
        }
        if et.get("HiddenData").truthy() {
            pl::method(it, &et.get("HiddenData").get("Fixup"), "AddFixup", vec![fixup.clone()])?;
        }
        // save fixup for PreviewImage in JPEG file if necessary
        let preview_info = et.get("PREVIEW_INFO");
        if preview_info.truthy() {
            let pt = preview_info.get("Data"); // image data or 'LOAD_PREVIEW' flag
            // now that we know the size of the EXIF data, first test to see if our new image fits
            // inside the EXIF segment (remember about the TIFF and EXIF headers: 8+6 bytes)
            if (!pt.eq_str("LOAD_PREVIEW") && pl::le(&pl::add(&pl::add(&len(&pt), &len(&get(&new_data))), &n(14)), &n(0xfffd)) && !preview_info.get("IsTrailer").truthy()) || preview_info.get("IsShort").truthy() {
                // must fit in this segment if using short pointers
                // It fits! (or must exist in EXIF segment), so fixup the
                // PreviewImage pointers and stuff the preview image in here
                let mut new_pos = pl::add(&len(&get(&new_data)), &new_data_pos);
                new_pos = pl::add(&new_pos, &or0(preview_info.get("BaseShift")));
                if preview_info.get("Relative").truthy() {
                    // calculate our base by looking at how far the pointer got shifted
                    let g = pl::method(it, &fixup, "GetMarkerPointers", vec![rf(&new_data), pl::s("PreviewImage")])?;
                    new_pos = pl::sub(&new_pos, &or0(g));
                }
                pl::method(it, &fixup, "SetMarkerPointers", vec![rf(&new_data), pl::s("PreviewImage"), new_pos])?;
                put(&new_data, pl::concat(&get(&new_data), &pt));
                // set flag to delete old preview unless it was contained in the EXIF
                if !et.get("PREVIEW_INFO").get("WasContained").truthy() {
                    et.set("DEL_PREVIEW", n(1));
                }
                et.delete("PREVIEW_INFO"); // done with our preview data
            } else {
                // Doesn't fit, or we still don't know, so save fixup information
                // and put the preview at the end of the file
                if !preview_info.get("Fixup").truthy() {
                    preview_info.set("Fixup", fixup_new(it)?);
                }
                pl::method(it, &preview_info.get("Fixup"), "AddFixup", vec![fixup.clone()])?;
            }
        } else if !get(&new_data).is_undef() && delete_all.truthy() {
            put(&new_data, pl::s("")); // delete both IFD0 and IFD1 since only mandatory tags remain
        } else if et.get("A100PreviewLength").truthy() {
            // save preview image start for patching A100 quirks later
            let g = pl::method(it, &fixup, "GetMarkerPointers", vec![rf(&new_data), pl::s("PreviewImage")])?;
            et.set("A100PreviewStart", g);
        }
        // save location of last IFD for use in Canon RAW header
        if pl::num_eq(&new_data_pos, &n(16)) {
            let mut ifd_pos = pl::method_list(it, &fixup, "GetMarkerPointers", vec![rf(&new_data), pl::s("NextIFD")])?;
            orig_dir_info.set("LastIFD", ifd_pos.pop().unwrap_or_default());
        }
        // recrypt SR2 SubIFD data if necessary
        let key = et.get("SR2SubIFDKey");
        if key.truthy() {
            let start = pl::method(it, &fixup, "GetMarkerPointers", vec![rf(&new_data), pl::s("SR2SubIFDOffset")])?;
            let l = et.get("SR2SubIFDLength");
            // (must subtract 8 for size of TIFF header)
            if start.truthy() && pl::le(&pl::add(&pl::sub(&start, &n(8)), &l), &len(&get(&new_data))) {
                crate::runtime::require(it, "Image::ExifTool::Sony")?;
                pl::call(it, "Image::ExifTool::Sony::Decrypt", vec![rf(&new_data), pl::sub(&start, &n(8)), l, key])?;
            }
        }
    }
    // return empty string if no entries in directory
    // (could be up to 10 bytes and still be empty)
    if !get(&new_data).is_undef() && pl::lt(&len(&get(&new_data)), &n(12)) {
        put(&new_data, pl::s(""));
    }

    // set changed if ForceWrite tag was set to "EXIF"
    if !get(&new_data).is_undef() && len(&get(&new_data)).truthy() && et.get("FORCE_WRITE").get("EXIF").truthy() {
        inc(&et, "CHANGED");
    }

    Ok(get(&new_data)) // return our directory data
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Exif::InverseOffsetTime", inverse_offset_time);
    it.define_native("Image::ExifTool::Exif::ConvertLensInfo", convert_lens_info);
    it.define_native("Image::ExifTool::Exif::GetCFAPattern", get_cfa_pattern);
    it.define_native("Image::ExifTool::Exif::CheckExif", check_exif);
    it.define_native("Image::ExifTool::Exif::EncodeExifText", encode_exif_text);
    it.define_native("Image::ExifTool::Exif::RebuildMakerNotes", rebuild_maker_notes);
    it.define_native("Image::ExifTool::Exif::SortIFD", sort_ifd);
    it.define_native("Image::ExifTool::Exif::ValidateIFD", validate_ifd);
    it.define_native("Image::ExifTool::Exif::GetOffList", get_off_list);
    it.define_native("Image::ExifTool::Exif::UpdateTiffEnd", update_tiff_end);
    it.define_native("Image::ExifTool::Exif::ValidateImageData", validate_image_data);
    it.define_native("Image::ExifTool::Exif::AddImageDataHash", add_image_data_hash);
    it.define_native("Image::ExifTool::Exif::ExifErr", exif_err);
    it.define_native("Image::ExifTool::Exif::ProcessTiffIFD", process_tiff_ifd);
    it.define_native("Image::ExifTool::Exif::WriteExif", write_exif);
}
