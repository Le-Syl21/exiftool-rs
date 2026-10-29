//! Image::ExifTool::WriteQuickTime (Image/ExifTool/WriteQuickTime.pl), ported sub by sub (see docs/port/PORTING.md).
//!
//! Write XMP (and the other metadata) to QuickTime (MOV and MP4) files. The
//! file is autoloaded in package Image::ExifTool::QuickTime; its `my`
//! variables (`%dirMap`, `%qtFormat`, `$undLang`...) live under
//! `Image::ExifTool::QuickTime::<my:WriteQuickTime.pl>::NAME`. Its
//! module-level code (marking the Preferred tags) is
//! `init_write_quick_time`, which the module's dump calls once loaded.
//!
//! The lists ExifTool keeps of chunk offsets and mdat chunks are Perl arrays
//! here too (WriteItemInfo's edit list is shared through `$$et{mdatEdit}`).

use std::cmp::Ordering;

use crate::interp::eval::{substr_range, ER};
use crate::interp::{Binding, Interp, Want};
use crate::modules::exiftool::part2;
use crate::modules::write_xmp::call_mut;
use crate::modules::xmp::{cat, hh};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, n, re, ret};

const PKG: &str = "Image::ExifTool::QuickTime";

// ---- local helpers ----

fn s(x: &str) -> Value {
    pl::s(x)
}

fn m(it: &mut Interp, obj: &Value, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::method(it, obj, name, args)
}

/// `Image::ExifTool::NAME(ARGS)` in scalar context.
fn etc(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// `NAME(ARGS)` of package QuickTime (QuickTime.pm).
fn qt(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("{PKG}::{name}"), args)
}

type Native = fn(&mut Interp, Vec<Value>, Want) -> ER<Vec<Value>>;

fn da(it: &mut Interp, f: Native, args: Vec<Value>) -> ER<Value> {
    Ok(f(it, args, Want::Scalar)?.pop().unwrap_or_default())
}

fn get8u(it: &mut Interp, dp: &Value, pos: Value) -> ER<Value> {
    da(it, part2::get8u, vec![dp.clone(), pos])
}

fn get16u(it: &mut Interp, dp: &Value, pos: Value) -> ER<Value> {
    da(it, part2::get16u, vec![dp.clone(), pos])
}

fn get32u(it: &mut Interp, dp: &Value, pos: Value) -> ER<Value> {
    da(it, part2::get32u, vec![dp.clone(), pos])
}

fn get64u(it: &mut Interp, dp: &Value, pos: Value) -> ER<Value> {
    etc(it, "Get64u", vec![dp.clone(), pos])
}

/// `Set8u($val)` (the packed value)
fn set8u(it: &mut Interp, v: Value) -> ER<Value> {
    da(it, part2::set8u, vec![v])
}

fn set16u(it: &mut Interp, v: Value) -> ER<Value> {
    da(it, part2::set16u, vec![v])
}

fn set32u(it: &mut Interp, v: Value) -> ER<Value> {
    da(it, part2::set32u, vec![v])
}

/// `Set16u($val, $dataPt, $pos)` (written in place)
fn set16u_at(it: &mut Interp, v: Value, dp: &Value, pos: Value) -> ER<()> {
    da(it, part2::set16u, vec![v, dp.clone(), pos])?;
    Ok(())
}

fn set32u_at(it: &mut Interp, v: Value, dp: &Value, pos: Value) -> ER<()> {
    da(it, part2::set32u, vec![v, dp.clone(), pos])?;
    Ok(())
}

fn set64u_at(it: &mut Interp, v: Value, dp: &Value, pos: Value) -> ER<()> {
    etc(it, "Set64u", vec![v, dp.clone(), pos])?;
    Ok(())
}

fn set_byte_order(it: &mut Interp, o: &str) -> ER<()> {
    da(it, part2::set_byte_order, vec![s(o)])?;
    Ok(())
}

fn get_tag_table(it: &mut Interp, name: Value) -> ER<Value> {
    etc(it, "GetTagTable", vec![name])
}

/// `$raf->Read($buff, $len)`: the count, and `buff` updated.
fn raf_read(it: &mut Interp, raf: &Value, buff: &mut Value, len: Value) -> ER<Value> {
    let mut a = [buff.clone(), len];
    let r = pl::method_mut(it, raf, "Read", &mut a, Want::Scalar)?.pop().unwrap_or_default();
    *buff = a[0].clone();
    Ok(r)
}

/// `$v || ''`
fn or_empty(v: Value) -> Value {
    if v.truthy() {
        v
    } else {
        s("")
    }
}

fn raf_seek(it: &mut Interp, raf: &Value, pos: Value, whence: i64) -> ER<bool> {
    Ok(m(it, raf, "Seek", vec![pos, n(whence)])?.truthy())
}

fn raf_tell(it: &mut Interp, raf: &Value) -> ER<Value> {
    m(it, raf, "Tell", vec![])
}

/// `Write($outfile, LIST)`: true on success.
fn write(it: &mut Interp, outfile: &Value, items: &[Value]) -> ER<bool> {
    let mut args = vec![outfile.clone()];
    args.extend_from_slice(items);
    Ok(etc(it, "Write", args)?.truthy())
}

fn error(it: &mut Interp, et: &Value, msg: Value) -> ER<Value> {
    m(it, et, "Error", vec![msg])
}

fn warn(it: &mut Interp, et: &Value, msg: Value) -> ER<Value> {
    m(it, et, "Warn", vec![msg])
}

fn vprint(it: &mut Interp, et: &Value, level: i64, msg: Value) -> ER<()> {
    m(it, et, "VPrint", vec![n(level), msg])?;
    Ok(())
}

fn inc_changed(et: &Value) {
    et.set("CHANGED", pl::add(&et.get("CHANGED"), &n(1)));
}

fn lex_name(name: &str) -> String {
    format!("{PKG}::<my:WriteQuickTime.pl>::{name}")
}

/// A hash `my` variable of WriteQuickTime.pl.
fn lex_h(it: &mut Interp, name: &str) -> Value {
    match it.global(b'%', &lex_name(name)) {
        Binding::Hash(h) => Value::HashRef(h),
        _ => Value::new_hash(),
    }
}

/// A scalar `my` variable of WriteQuickTime.pl.
fn lex_s(it: &mut Interp, name: &str) -> Value {
    match it.global(b'$', &lex_name(name)) {
        Binding::Scalar(c) => c.borrow().clone(),
        _ => Value::Undef,
    }
}

/// A package hash of QuickTime.pm (`%avType`, `%stringEncoding`...).
fn pkg_h(it: &mut Interp, name: &str) -> Value {
    pl::pkg_hash(it, &format!("{PKG}::{name}"))
}

fn len_of(v: &Value) -> Value {
    n(v.to_bytes().len() as i64)
}

/// `length $$ref`
fn dlen(r: &Value) -> Value {
    len_of(&r.deref())
}

fn keys(h: &Value) -> Vec<Value> {
    h.keys().into_iter().map(Value::Str).collect()
}

fn sorted_keys(h: &Value) -> Vec<Value> {
    let mut k = keys(h);
    k.sort_by(|a, b| a.to_bytes().cmp(&b.to_bytes()));
    k
}

fn num_cmp(a: &Value, b: &Value) -> Ordering {
    ops::num_cmp(a, b).unwrap_or(Ordering::Equal)
}

/// `sort { $a <=> $b } keys %$h`
fn sorted_num_keys(h: &Value) -> Vec<Value> {
    let mut k = keys(h);
    k.sort_by(num_cmp);
    k
}

fn arr(v: Vec<Value>) -> Value {
    Value::new_array(v)
}

/// `$$a[i] += $d`
fn add_at(a: &Value, i: usize, d: &Value) {
    a.set_at(i, pl::add(&a.at(i as i64), d));
}

/// `substr($$ref, $pos, 0) = $ins`
fn insert_at(r: &Value, pos: i64, ins: &Value) -> ER<()> {
    let b = r.deref().to_bytes();
    if let Some((a, _)) = substr_range(b.len(), pos, Some(0)) {
        let mut o = b[..a].to_vec();
        o.extend_from_slice(&ins.to_bytes());
        o.extend_from_slice(&b[a..]);
        pl::set_deref(r, Value::bytes(o))?;
    }
    Ok(())
}

/// `substr($s, $off, $len) = $repl` on a value.
fn substr_set(v: &Value, off: i64, len: i64, repl: &Value) -> Value {
    let b = v.to_bytes();
    match substr_range(b.len(), off, Some(len)) {
        Some((a, e)) => {
            let mut o = b[..a].to_vec();
            o.extend_from_slice(&repl.to_bytes());
            o.extend_from_slice(&b[e..]);
            Value::bytes(o)
        }
        None => v.clone(),
    }
}

/// `GetVarInt($dataPt, $pos, $n)`: the value, with `pos` updated.
fn get_var_int(it: &mut Interp, data_pt: &Value, pos: &mut Value, nn: &Value) -> ER<Value> {
    let mut args = [data_pt.clone(), pos.clone(), nn.clone()];
    let r = call_mut(it, "Image::ExifTool::QuickTime::GetVarInt", &mut args, Want::Scalar)?.pop().unwrap_or_default();
    *pos = args[1].clone();
    Ok(r)
}

/// Pack the language and country codes of a LangCode (`$1`, `$2` of
/// `/^([A-Z]{3})?[-_]?([A-Z]{2})?$/i`) as the two writers do.
fn pack_lang(c1: &Value, c2: &Value) -> ER<(Value, Value)> {
    let (mut ctry, mut lang) = (n(0), n(0));
    if c1.truthy() && !pl::str_eq(c1, "und") {
        for ch in pl::unpack("C*", &pl::lc(c1))? {
            lang = ops::bit_or(&ops::shift_left(&lang, &n(5)), &pl::sub(&ch, &n(0x60)));
        }
    }
    if c2.truthy() && !pl::str_eq(c2, "ZZ") {
        ctry = pl::unpack1("n", &pl::pack("a2", &[pl::uc(c2)])?)?;
    }
    Ok((ctry, lang))
}

// ---- the subs of WriteQuickTime.pl ----

/// Format GPSCoordinates for writing
/// Inputs: 0) PrintConv value
/// Returns: ValueConv value
pub fn print_inv_gps_coordinates(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let val = arg(&a, 0);
    let mut v = pl::split(&re(", *", ""), &val, 0);
    if v.len() == 2 || v.len() == 3 {
        let below = v.len() > 2 && v[2].truthy() && re("below", "i").is_match(&v[2]);
        v[0] = pl::call(it, "Image::ExifTool::GPS::ToDegrees", vec![v[0].clone(), n(1)])?;
        v[1] = pl::call(it, "Image::ExifTool::GPS::ToDegrees", vec![v[1].clone(), n(1)])?;
        if v.len() == 3 {
            let f = etc(it, "ToFloat", vec![v[2].clone()])?;
            v[2] = pl::mul(&f, &n(if below { -1 } else { 1 }));
        }
        return ret(pl::join(" ", &v));
    }
    if re(r"^([-+]?\d+(\.\d*)?)\s+([-+]?\d+(\.\d*)?)$", "").is_match(&val) {
        return ret(val); // already 2 floats?
    }
    if re(r"^([-+]\d+(\.\d*)?){2,3}(CRS.*)?/?$", "").is_match(&val) {
        return ret(val); // already in ISO6709 format?
    }
    pl::ret_undef()
}

/// Convert GPS coordinates back to ISO6709 format
/// Inputs: 0) ValueConv value
/// Returns: ISO6709 coordinates
pub fn conv_inv_iso6709(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| {
        let val = arg(&a, 0);
        let mut av = pl::split_ws(&val);
        if av.len() == 2 || av.len() == 3 {
            // latitude must have 2 digits before the decimal, and longitude 3,
            // and all values must start with a "+" or "-", and Google Photos
            // requires at least 3 digits after the decimal point
            // (and as of Apr 2021, Google Photos doesn't accept coordinats
            //  with more than 5 digits after the decimal place:
            //  https://exiftool.org/forum/index.php?topic=11055.msg67171#msg67171
            //  still a problem Apr 2024: https://exiftool.org/forum/index.php?msg=85761)
            let mut fmt: Vec<&str> = vec!["%s%02d.%s%s", "%s%03d.%s%s", "%s%d.%s%s"];
            let mut limit: Vec<i64> = vec![90, 180];
            for x in av.iter_mut() {
                if !etc(it, "IsFloat", vec![x.clone()])?.truthy() {
                    return pl::ret_undef();
                }
                let lim = if limit.is_empty() { None } else { Some(limit.remove(0)) };
                if let Some(l) = lim {
                    if pl::gt(&pl::abs(x), &n(l)) {
                        pl::warn(it, &format!("{}itude out of range\n", if limit.is_empty() { "Long" } else { "Lat" }))?;
                    }
                }
                re(r"^([-+]?)(\d+)\.?(\d*)", "").subst(x, false, |c| {
                    let f = if fmt.is_empty() { "" } else { fmt.remove(0) };
                    let c1 = cap(c, 1);
                    let c3 = cap(c, 3);
                    let l3 = c3.to_bytes().len();
                    let pad = if l3 < 3 { "0".repeat(3 - l3) } else { String::new() };
                    pl::sprintf(f, &[if c1.truthy() { c1 } else { s("+") }, cap(c, 2), c3, s(&pad)]).to_bytes().to_vec()
                });
            }
            av.push(s("/"));
            return ret(pl::join("", &av));
        }
        if re(r"^([-+]\d+(\.\d*)?){2,3}(CRS.*)?/?$", "").is_match(&val) {
            return ret(val); // already in ISO6709 format?
        }
        pl::ret_undef()
    })
}

/// Handle offsets in iloc (ItemLocation) atom when writing (ref ISO 14496-12:2015 pg.79)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) data ref, 3) output buffer ref
/// Returns: true on success
/// Notes: see also ParseItemLocation() in QuickTime.pm
/// (variable names with underlines correspond to names in ISO 14496-12)
pub fn handle_iloc(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(n(handle_iloc_v(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2), &arg(&a, 3))?))
}

fn handle_iloc_v(it: &mut Interp, et: &Value, dir_info: &Value, data_pt: &Value, outfile: &Value) -> ER<i64> {
    let off = dir_info.get("ChunkOffset");
    pl::at(208);
    let len = len_of(&pl::sderef(data_pt)?);
    if pl::lt(&len, &n(8)) {
        return Ok(0);
    }
    let ver = get8u(it, data_pt, n(0))?;
    let siz = get16u(it, data_pt, n(4))?;
    let noff = ops::shift_right(&siz, &n(12));
    let nlen = ops::bit_and(&ops::shift_right(&siz, &n(8)), &n(0x0f));
    let nbas = ops::bit_and(&ops::shift_right(&siz, &n(4)), &n(0x0f));
    let nind = ops::bit_and(&siz, &n(0x0f));
    let ok = |v: &Value| matches!(v.to_i64(), 0 | 4 | 8);
    if !(ok(&noff) && ok(&nlen) && ok(&nbas) && ok(&nind)) {
        return Ok(0);
    }
    // piggy-back on existing code to fix up stco/co64 4/8-byte offsets
    let tag = if pl::num_eq(&noff, &n(4)) { s("stco_iloc") } else { s("co64_iloc") };
    let (num, mut pos);
    if pl::lt(&ver, &n(2)) {
        num = get16u(it, data_pt, n(6))?;
        pos = n(8);
    } else {
        if pl::lt(&len, &n(10)) {
            return Ok(0);
        }
        num = get32u(it, data_pt, n(6))?;
        pos = n(10);
    }
    let mut i = n(0);
    while pl::lt(&i, &num) {
        let id;
        if pl::lt(&ver, &n(2)) {
            if pl::gt(&pl::add(&pos, &n(2)), &len) {
                return Ok(0);
            }
            id = get16u(it, data_pt, pos.clone())?;
            pos = pl::add(&pos, &n(2));
        } else {
            if pl::gt(&pl::add(&pos, &n(4)), &len) {
                return Ok(0);
            }
            id = get32u(it, data_pt, pos.clone())?;
            pos = pl::add(&pos, &n(4));
        }
        let mut const_off = Value::Undef;
        let mut off_base: Vec<Value> = Vec::new();
        let mut off_item: Vec<Value> = Vec::new();
        let mut min_offset = Value::Undef;
        if pl::num_eq(&ver, &n(1)) || pl::num_eq(&ver, &n(2)) {
            if pl::gt(&pl::add(&pos, &n(2)), &len) {
                return Ok(0);
            }
            // offsets are absolute only if ConstructionMethod is 0, otherwise
            // the relative offsets are constant as far as we are concerned
            const_off = ops::bit_and(&get16u(it, data_pt, pos.clone())?, &n(0x0f));
            pos = pl::add(&pos, &n(2));
        }
        if pl::gt(&pl::add(&pos, &n(2)), &len) {
            return Ok(0);
        }
        let dref_idx = get16u(it, data_pt, pos.clone())?;
        if dref_idx.truthy() {
            let qdr = et.get("QtDataRef");
            let di = pl::sub(&dref_idx, &n(1)).to_i64();
            if qdr.truthy() && qdr.at(di).truthy() {
                let dref = qdr.at(di);
                // these offsets are constant unless the data is in this file
                if !(pl::num_eq(&dref.at(1), &n(1)) && !pl::str_eq(&dref.at(0), "rsrc")) {
                    const_off = n(1);
                }
            } else {
                error(it, et, cat!("No data reference for iloc entry ", i))?;
                return Ok(0);
            }
        }
        pos = pl::add(&pos, &n(2));
        // get base offset and save its location if in this file
        let base_offset = get_var_int(it, data_pt, &mut pos, &nbas)?;
        if base_offset.truthy() && !const_off.truthy() {
            let tg = cat!(if pl::num_eq(&nbas, &n(4)) { "stco" } else { "co64" }, "_iloc");
            let p = pl::sub(&pl::add(&pl::add(&dlen(outfile), &n(8)), &pos), &nbas);
            off_base.push(arr(vec![tg, p, nbas.clone(), n(0), id.clone()]));
        }
        if pl::gt(&pl::add(&pos, &n(2)), &len) {
            return Ok(0);
        }
        let ext_num = get16u(it, data_pt, pos.clone())?;
        pos = pl::add(&pos, &n(2));
        // run through the item list to get offset locations and the minimum offset in this file
        let mut j = n(0);
        while pl::lt(&j, &ext_num) {
            if pl::num_eq(&ver, &n(1)) || pl::num_eq(&ver, &n(2)) {
                pos = pl::add(&pos, &nind);
            }
            let extent_offset = get_var_int(it, data_pt, &mut pos, &noff)?;
            if extent_offset.is_undef() {
                return Ok(0);
            }
            if !const_off.truthy() {
                if noff.truthy() {
                    let p = pl::sub(&pl::add(&pl::add(&dlen(outfile), &n(8)), &pos), &noff);
                    off_item.push(arr(vec![tag.clone(), p, noff.clone(), n(0), id.clone()]));
                }
                if min_offset.is_undef() || pl::gt(&min_offset, &extent_offset) {
                    min_offset = extent_offset.clone();
                }
            }
            if pl::gt(&pl::add(&pos, &nlen), &dlen(data_pt)) {
                return Ok(0);
            }
            pos = pl::add(&pos, &nlen);
            j = pl::add(&j, &n(1));
        }
        // decide whether to fix up the base offset or individual item offsets
        // (adjust the one that is larger)
        if !min_offset.is_undef() && pl::gt(&min_offset, &base_offset) {
            for e in &off_item {
                e.set_at(3, base_offset.clone());
            }
            for e in off_item {
                off.push(e);
            }
        } else {
            for e in &off_base {
                e.set_at(3, min_offset.clone());
            }
            for e in off_base {
                off.push(e);
            }
        }
        i = pl::add(&i, &n(1));
    }
    Ok(1)
}

/// Get localized version of tagInfo hash
/// Inputs: 0) tagInfo hash ref, 1) language code (eg. "fra-FR")
/// Returns: new tagInfo hash ref, or undef if invalid or no language code
pub fn get_lang_info(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(get_lang_info_v(it, &arg(&a, 0), arg(&a, 1))?)
}

fn get_lang_info_v(it: &mut Interp, tag_info: &Value, mut lang_code: Value) -> ER<Value> {
    if !lang_code.truthy() {
        return Ok(Value::Undef);
    }
    // only allow alternate language tags in lang-alt lists
    let mut writable = tag_info.get("Writable");
    if writable.is_undef() {
        writable = hh(tag_info, "Table").get("WRITABLE");
    }
    if !writable.truthy() {
        return Ok(Value::Undef);
    }
    pl::tr(&mut lang_code, "_", "-", ""); // RFC 3066 specifies '-' as a separator
    let lang_info = etc(it, "GetLangInfo", vec![tag_info.clone(), lang_code])?;
    Ok(lang_info)
}

/// validate raw values for writing
/// Inputs: 0) ExifTool ref, 1) tagInfo hash ref, 2) raw value ref
/// Returns: error string or undef (and possibly changes value) on success
pub fn check_qt_value(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (tag_info, val_ptr) = (arg(&a, 1), arg(&a, 2));
    let mut format = tag_info.get("Format");
    if !format.truthy() {
        format = tag_info.get("Writable");
    }
    if !format.truthy() {
        format = hh(&tag_info, "Table").get("FORMAT");
    }
    if !format.truthy() {
        return pl::ret_undef();
    }
    ret(etc(it, "CheckValue", vec![val_ptr, format, tag_info.get("Count")])?)
}

/// Format QuickTime value for writing
/// Inputs: 0) ExifTool ref, 1) value ref, 2) tagInfo ref, 3) Format (or undef)
/// Returns: Flags for QT data type, and reformats value as required (sets to undef on error)
pub fn format_qt_value(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(format_qt_value_v(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2), arg(&a, 3))?)
}

fn format_qt_value_v(it: &mut Interp, et: &Value, val_pt: &Value, tag_info: &Value, mut format: Value) -> ER<Value> {
    let writable = tag_info.get("Writable");
    let count = tag_info.get("Count");
    let flags;
    if !format.truthy() {
        format = tag_info.get("Format");
    }
    let qt_format = lex_h(it, "qtFormat");
    let mut val_pt = val_pt.clone();
    let v;
    if (format.truthy() && !pl::str_eq(&format, "string")) || (!format.truthy() && writable.truthy() && !pl::str_eq(&writable, "string")) {
        // ($$valPt as a sub argument and an assignment target autovivifies
        // the lexical $valPt; a string dies under strict refs)
        pl::at(335);
        if val_pt.is_undef() {
            val_pt = pl::sref(Value::Undef);
        }
        v = pl::sderef(&val_pt)?;
        let val_pt = &val_pt;
        let f = if format.truthy() { format.clone() } else { writable.clone() };
        let w = etc(it, "WriteValue", vec![v, f, count])?;
        pl::set_deref(val_pt, w)?;
        if writable.truthy() && qt_format.get(writable.to_bytes()).truthy() {
            flags = qt_format.get(writable.to_bytes());
        } else {
            let k = if format.truthy() { format.clone() } else { n(0) };
            let f = qt_format.get(k.to_bytes());
            flags = if f.truthy() { f } else { n(0) };
        }
    } else if {
        pl::at(341);
        v = pl::sderef(&val_pt)?;
        re(r"^\xff\xd8\xff", "").is_match(&v)
    } {
        flags = n(0x0d); // JPG
    } else if re(r"^(\x89P|\x8aM|\x8bJ)NG\r\n\x1a\n", "").is_match(&v) {
        flags = n(0x0e); // PNG
    } else if re(r"^BM.{15}\0", "s").is_match(&v) {
        flags = n(0x1b); // BMP
    } else {
        flags = n(0x01); // UTF8
        let e = m(it, et, "Encode", vec![v, s("UTF8")])?;
        pl::set_deref(&val_pt, e)?;
    }
    if val_pt.deref().is_undef() {
        warn(it, et, cat!("Error converting value for ", tag_info.get("Name")))?;
    }
    Ok(flags)
}

/// Set variable-length integer (used by WriteItemInfo)
/// Inputs: 0) value, 1) integer size in bytes (0, 4 or 8),
/// Returns: packed integer
pub fn set_var_int(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(set_var_int_v(it, arg(&a, 0), &arg(&a, 1))?)
}

fn set_var_int_v(it: &mut Interp, val: Value, nn: &Value) -> ER<Value> {
    if pl::num_eq(nn, &n(4)) {
        return set32u(it, val);
    } else if pl::num_eq(nn, &n(8)) {
        return etc(it, "Set64u", vec![val]);
    }
    Ok(s(""))
}

/// Write Nextbase infi atom (ref PH)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: updated infi data
pub fn write_nextbase(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    if !et.truthy() {
        return ret(n(1));
    }
    if hh(&et, "DEL_GROUP").get("Nextbase").truthy() {
        inc_changed(&et);
        return ret(s(""));
    }
    pl::at(379);
    ret(pl::sderef(&dir_info.get("DataPt"))?)
}

/// Write Meta Keys to add/delete entries as necessary ('mdta' handler) (ref PH)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: updated keys box data
/// Note: Residual entries may be left in the 'keys' directory when deleting tags
///       with language codes because the language code(s) are not known until the
///       corresponding ItemList entry(s) are processed
pub fn write_keys(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if !et.truthy() {
        return ret(n(1)); // allow dummy access to autoload this package
    }
    let data_pt = dir_info.get("DataPt");
    pl::at(394);
    let mut dir_len = len_of(&pl::sderef(&data_pt)?);
    let done = Value::new_hash();
    let remap = Value::new_hash();
    let info = Value::new_hash();
    let add = Value::new_hash();

    let av_type = pkg_h(it, "avType");
    let at = av_type.get(et.get("MediaType").to_bytes());
    let keys_grp = if at.truthy() { cat!(at, "Keys") } else { s("Keys") };
    if pl::lt(&dir_len, &n(8)) {
        warn(it, &et, s("Short Keys box"))?;
        dir_len = n(8);
        pl::set_deref(&data_pt, Value::bytes(vec![0u8; 8]))?;
    }
    if hh(&et, "DEL_GROUP").get(keys_grp.to_bytes()).truthy() {
        dir_len = n(8); // delete all existing keys
        // deleted keys are identified by a zero entry in the Remap lookup
        let nn = get32u(it, &data_pt, n(4))?;
        let mut i = n(1);
        while pl::le(&i, &nn) {
            remap.set(i.to_bytes(), n(0));
            i = pl::add(&i, &n(1));
        }
        vprint(it, &et, 0, cat!("  [deleting ", nn, " ", keys_grp, " entr", if pl::num_eq(&nn, &n(1)) { "y" } else { "ies" }, "]\n"))?;
        inc_changed(&et);
    }
    let mut pos = n(8);
    let new_tags = m(it, &et, "GetNewTagInfoHash", vec![tag_table_ptr.clone()])?;
    let mut new_data = pl::substr(&data_pt.deref(), 0, Some(pos.to_i64()));

    let mut new_index = n(1);
    let mut index = n(1);
    while pl::lt(&pos, &pl::sub(&dir_len, &n(4))) {
        let d = data_pt.deref();
        let len = pl::unpack1(&format!("x{}N", pos.to_text()), &d)?;
        if pl::lt(&len, &n(8)) || pl::gt(&pl::add(&pos, &len), &dir_len) {
            break;
        }
        let ns = pl::substr(&d, pl::add(&pos, &n(4)).to_i64(), Some(4));
        let mut tag = pl::substr(&d, pl::add(&pos, &n(8)).to_i64(), Some(pl::sub(&len, &n(8)).to_i64()));
        re(r"\0.*", "s").subst(&mut tag, false, |_| Vec::new()); // truncate at null
        if pl::str_eq(&ns, "mdta") {
            re(r"^com\.apple\.quicktime\.", "").subst(&mut tag, false, |_| Vec::new()); // remove apple quicktime domain
        }
        if !tag.truthy() {
            tag = cat!("Tag_", ns);
        }
        done.set(tag.to_bytes(), n(1)); // set flag to avoid creating this tag
        let tag_info = m(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
        if tag_info.truthy() {
            info.set(index.to_bytes(), tag_info.clone());
            if new_tags.get(tag.to_bytes()).truthy() {
                let nv_hash = m(it, &et, "GetNewValueHash", vec![tag_info.clone()])?;
                // drop this tag if it is being deleted
                if nv_hash.truthy() && pl::gt(&m(it, &et, "IsOverwriting", vec![nv_hash.clone()])?, &n(0)) && m(it, &et, "GetNewValue", vec![nv_hash.clone()])?.is_undef() {
                    // don't delete this key if we could be writing any alternate-language version of this tag
                    let mut dont_delete = false;
                    for t in keys(&new_tags) {
                        let nt = new_tags.get(t.to_bytes());
                        if !(nt.get("SrcTagInfo").truthy() && nt.get("SrcTagInfo").to_bytes() == tag_info.to_bytes()) {
                            continue;
                        }
                        let nv = m(it, &et, "GetNewValueHash", vec![nt])?;
                        if !(m(it, &et, "IsOverwriting", vec![nv.clone()])?.truthy() && !m(it, &et, "GetNewValue", vec![nv])?.is_undef()) {
                            continue;
                        }
                        dont_delete = true;
                        break;
                    }
                    if !dont_delete {
                        // delete this key
                        vprint(it, &et, 1, cat!(et.get("INDENT"), "[deleting ", keys_grp, " entry ", index, " '", tag, "']\n"))?;
                        pos = pl::add(&pos, &len);
                        remap.set(index.to_bytes(), n(0));
                        index = pl::add(&index, &n(1));
                        inc_changed(&et);
                        continue;
                    }
                }
            }
        }
        // add to the Keys box data
        new_data = pl::concat(&new_data, &pl::substr(&d, pos.to_i64(), Some(len.to_i64())));
        remap.set(index.to_bytes(), new_index.clone());
        index = pl::add(&index, &n(1));
        new_index = pl::add(&new_index, &n(1));
        pos = pl::add(&pos, &len);
    }
    // add keys for any tags we need to create
    let full_keys_id = lex_h(it, "fullKeysID");
    for tag in sorted_keys(&new_tags) {
        let tag_info = new_tags.get(tag.to_bytes());
        let id = if tag_info.get("LangCode").truthy() && tag_info.get("SrcTagInfo").truthy() { tag_info.get("SrcTagInfo").get("TagID") } else { tag.clone() };
        if done.get(id.to_bytes()).truthy() {
            continue;
        }
        let nv_hash = m(it, &et, "GetNewValueHash", vec![tag_info.clone()])?;
        if !(nv_hash.get("IsCreating").truthy() && m(it, &et, "IsOverwriting", vec![nv_hash.clone()])?.truthy() && !m(it, &et, "GetNewValue", vec![nv_hash.clone()])?.is_undef()) {
            continue;
        }
        // add new entry to 'keys' data
        let mut val = id.clone();
        let full = match re(r"^(.*?)\.", "").m(&val) {
            Some(c) => full_keys_id.get(cap(&c, 1).to_bytes()).truthy(),
            None => false,
        };
        if !full {
            val = cat!("com.apple.quicktime.", val);
        }
        let s32 = set32u(it, pl::add(&n(8), &len_of(&val)))?;
        new_data = cat!(new_data, s32, "mdta", val);
        vprint(it, &et, 1, cat!(et.get("INDENT"), "[adding ", keys_grp, " entry ", new_index, " '", id, "']\n"))?;
        add.set(new_index.to_bytes(), tag_info);
        new_index = pl::add(&new_index, &n(1));
        inc_changed(&et);
    }
    let num = pl::sub(&new_index, &n(1));
    if num.truthy() {
        let r = pl::sref(new_data);
        set32u_at(it, num.clone(), &r, n(4))?; // update count in header
        new_data = r.deref();
    } else {
        new_data = s(""); // delete empty Keys box
    }
    // save temporary variables for use when writing ItemList:
    //   Remap - lookup for remapping Keys ID numbers (0 if item is deleted)
    //   Info  - Keys tag information, based on old index value
    //   Add   - Keys items deleted, based on old index value
    //   Num   - Number of items in edited Keys box
    let h = Value::new_hash();
    h.set("Remap", remap);
    h.set("Info", info);
    h.set("Add", add);
    h.set("Num", num);
    et.set(keys_grp.to_bytes(), h);

    ret(new_data) // return updated Keys box
}

/// Write ItemInformation in HEIC files
/// Inputs: 0) ExifTool ref, 1) dirInfo ref (with BoxPos entry), 2) output buffer ref
/// Returns: mdat edit list ref (empty if nothing changed)
pub fn write_item_info(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    match write_item_info_v(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2))? {
        Some(v) => ret(v),
        None => pl::ret_nothing(want),
    }
}

/// WriteItemInfo: `None` for `return ()`.
fn write_item_info_v(it: &mut Interp, et: &Value, dir_info: &Value, outfile: &Value) -> ER<Option<Value>> {
    let box_pos = dir_info.get("BoxPos"); // hash of [position,length,irefVer(iref only)] for box in $outfile
    let raf = et.get("RAF");
    let items = et.get("ItemInfo");
    let did = Value::new_hash();
    let mut mdat_edit: Vec<Value> = Vec::new();
    let mut tmap = Value::Undef;

    if !(items.truthy() && raf.truthy()) {
        return Ok(None);
    }

    // extract information from EXIF/XMP metadata items
    let mut primary = et.get("PrimaryItem");
    let cur_pos = raf_tell(it, &raf)?;
    let mut last_id = n(0);
    for id in sorted_num_keys(&items) {
        last_id = id.clone();
        if primary.is_undef() {
            primary = id.clone(); // assume primary is lowest-number item if pitm missing
        }
        let item = items.get(id.to_bytes());
        // only edit primary EXIF/XMP metadata
        if !(item.get("RefersTo").truthy() && item.get("RefersTo").get(primary.to_bytes()).truthy()) {
            continue;
        }
        let mut type_ = item.get("ContentType");
        if !type_.truthy() {
            type_ = item.get("Type");
        }
        if !type_.truthy() {
            continue;
        }
        if pl::str_eq(&type_, "tmap") {
            tmap = id.clone(); // save ID of primary 'tmap' item (tone-mapped image)
        }
        // get ExifTool name for this item
        let name = match &*type_.to_bytes() {
            b"Exif" => "EXIF",
            b"application/rdf+xml" => "XMP",
            _ => continue, // only care about EXIF and XMP
        };
        if !hh(et, "EDIT_DIRS").get(name).truthy() {
            continue;
        }
        did.set(name, n(1)); // set flag to prevent creating this metadata
        let mut warn_ = Value::Undef;
        let mut buff = Value::Undef;
        let mut edit: Vec<Value> = Vec::new();
        if !box_pos.get("iloc").truthy() {
            warn_ = s("Missing iloc box");
        }
        if !(item.get("Extents").truthy() && !item.get("Extents").is_empty()) {
            warn_ = cat!("No Extents for ", type_, " item");
        }
        if item.get("ContentEncoding").truthy() {
            if !pl::str_eq(&item.get("ContentEncoding"), "deflate") {
                warn_ = cat!("Can't currently decode ", item.get("ContentEncoding"), " encoded ", type_, " metadata");
            } else if pl::eval_block(it, |it| crate::runtime::require(it, "Compress::Zlib"))?.is_none() {
                warn_ = cat!("Install Compress::Zlib to decode deflated ", type_, " metadata");
            }
        }
        if item.get("ProtectionIndex").truthy() {
            warn_ = cat!("Can't currently decode protected ", type_, " metadata");
        }
        if item.get("ConstructionMethod").truthy() {
            warn_ = cat!("Can't currently extract ", type_, " with construction method ", item.get("ConstructionMethod"));
        }
        if item.get("DataReferenceIndex").truthy() {
            warn_ = cat!(type_, " metadata is not in this file");
        }
        if warn_.truthy() {
            warn(it, et, warn_)?;
            continue;
        }
        let base = if item.get("BaseOffset").truthy() { item.get("BaseOffset") } else { n(0) };
        let mut val = s("");
        for extent in item.get("Extents").items() {
            if !buff.is_undef() {
                val = pl::concat(&val, &buff);
            }
            let pos = pl::add(&extent.at(1), &base);
            if extent.at(2).truthy() {
                if !raf_seek(it, &raf, pos.clone(), 0)? {
                    break;
                }
                if !raf_read(it, &raf, &mut buff, extent.at(2))?.truthy() {
                    break;
                }
            } else {
                buff = s("");
            }
            edit.push(arr(vec![pos.clone(), pl::add(&pos, &extent.at(2))])); // replace or delete this if changed
        }
        if buff.is_undef() {
            continue;
        }
        if pl::length(&val).truthy() {
            buff = pl::concat(&val, &buff);
        }
        let comp = m(it, et, "Options", vec![s("Compress")])?;
        if !comp.is_undef() && (comp.truthy() ^ item.get("ContentEncoding").truthy()) {
            //TODO: add ability to edit infe entry in iinf to change encoding according to Compress option if set
            m(it, et, "Warn", vec![cat!("Can't currently change compression when rewriting ", name, " in HEIC"), n(1)])?;
        }
        let mut was_deflated = false;
        if item.get("ContentEncoding").truthy() {
            let (mut v2, mut stat) = (Value::Undef, Value::Undef);
            let inflate = pl::call(it, "Compress::Zlib::inflateInit", vec![])?;
            if inflate.truthy() {
                let mut args = [buff.clone()];
                let r = pl::method_mut(it, &inflate, "inflate", &mut args, Want::List)?;
                // ($buff, passed as an alias, is replaced or skipped below)
                v2 = arg(&r, 0);
                stat = arg(&r, 1);
            }
            vprint(it, et, 0, cat!("  (Inflating stored ", name, " metadata)\n"))?;
            if inflate.truthy() && pl::num_eq(&stat, &pl::call(it, "Compress::Zlib::Z_STREAM_END", vec![])?) {
                buff = v2;
                was_deflated = true;
            } else {
                warn(it, et, cat!("Error inflating ", name, " metadata"))?;
                continue;
            }
        }
        let (mut hdr, sub_table, mut proc_);
        proc_ = Value::Undef;
        let more_dir_info = Value::new_hash(); // %moreDirInfo
        let new_val2 = pl::sref(Value::Undef); // \$newVal2
        let mut strt = n(0);
        if name == "EXIF" {
            if !pl::length(&buff).truthy() {
                // create EXIF from scratch
                hdr = Value::bytes(b"\0\0\0\x06Exif\0\0");
            } else if re(r"^(MM\0\x2a|II\x2a\0)", "").is_match(&buff) {
                warn(it, et, s("Missing Exif header"))?;
                hdr = s("");
            } else if pl::ge(&len_of(&buff), &n(4)) && pl::ge(&len_of(&buff), &pl::add(&n(4), &pl::unpack1("N", &buff)?)) {
                hdr = pl::substr(&buff, 0, Some(pl::add(&n(4), &pl::unpack1("N", &buff)?).to_i64()));
                strt = len_of(&hdr);
                if pl::num_eq(&len_of(&hdr), &n(14)) && &pl::substr(&hdr, 4, Some(2)).to_bytes()[..] == b"\xff\xe1" {
                    proc_ = Value::Code(it.sub_ref("Image::ExifTool::WriteJPEG"));
                    // (use EXIF table so the group names will be correct for deleting as a group)
                    more_dir_info.set("XiaomiMess", n(1));
                    more_dir_info.set("OutFile", new_val2.clone());
                    let class = s("File::RandomAccess");
                    let new_raf = it.call_method(&class, "new", vec![class.clone(), pl::sref(buff.clone())], Want::Scalar)?.pop().unwrap_or_default();
                    more_dir_info.set("RAF", new_raf);
                    pl::set_deref(&new_val2, s(""))?;
                    et.get_or_new_hash("EDIT_DIRS").set("APP1", s("JPEG")); // must parse into JPEG APP1
                    raf_seek(it, &raf, n(4), 0)?; // (skip size)
                    hdr = s("");
                }
            } else {
                warn(it, et, s("Invalid Exif header"))?;
                continue;
            }
            sub_table = get_tag_table(it, s("Image::ExifTool::Exif::Main"))?;
            if !proc_.truthy() {
                proc_ = Value::Code(it.sub_ref("Image::ExifTool::WriteTIFF"));
            }
        } else {
            hdr = s("");
            sub_table = get_tag_table(it, s("Image::ExifTool::XMP::Main"))?;
        }
        let d_info = Value::new_hash();
        d_info.set("DataPt", pl::sref(buff.clone()));
        d_info.set("DataLen", len_of(&buff));
        d_info.set("DirStart", strt.clone());
        d_info.set("DirLen", pl::sub(&len_of(&buff), &strt));
        for k in more_dir_info.keys() {
            d_info.set(&k[..], more_dir_info.get(&k[..]));
        }
        let changed = et.get("CHANGED");
        et.set("DemoteErrors", n(1));
        let mut new_val = m(it, et, "WriteDirectory", vec![d_info.clone(), sub_table, proc_])?;
        if pl::gt(&et.get("DemoteErrors"), &n(1)) {
            // just copy existing subdirectory if a non-quicktime error occurred
            new_val = Value::Undef;
            et.set("CHANGED", changed.clone()); // nothing changed
        }
        et.set("DemoteErrors", Value::Undef);
        if more_dir_info.get("OutFile").truthy() && !new_val.is_undef() {
            if pl::str_eq(&new_val, "1") {
                new_val = new_val2.deref(); // return OutFile buffer (from WriteJPEG)
            } else if pl::str_eq(&new_val, "0") || pl::str_eq(&new_val, "-1") {
                new_val = Value::Undef; // (shouldn't happen)
            }
        }
        if !new_val.is_undef() && changed.to_bytes() != et.get("CHANGED").to_bytes() &&
            // nothing changed if deleting an empty directory
            (d_info.get("DirLen").truthy() || pl::length(&new_val).truthy())
        {
            if pl::length(&hdr).truthy() && pl::length(&new_val).truthy() {
                new_val = pl::concat(&hdr, &new_val);
            }
            if was_deflated {
                let deflate = pl::call(it, "Compress::Zlib::deflateInit", vec![])?;
                if deflate.truthy() {
                    vprint(it, et, 0, cat!("  (Re-deflating new ", name, " metadata)\n"))?;
                    buff = m(it, &deflate, "deflate", vec![new_val.clone()])?;
                    if !buff.is_undef() {
                        let f = m(it, &deflate, "flush", vec![])?;
                        buff = pl::concat(&buff, &f);
                        new_val = buff.clone();
                    }
                }
            }
            if !edit.is_empty() {
                edit[0].set_at(2, pl::sref(new_val.clone())); // replace the old chunk with the new data
                edit[0].set_at(3, id.clone()); // mark this chunk with the item ID
            }
            mdat_edit.extend(edit.iter().cloned());
            // update item extent_length
            let mut nn = len_of(&new_val);
            for extent in item.get("Extents").items() {
                let (nlen, len_pt) = (extent.at(3), extent.at(4));
                let p = pl::add(&pl::add(&box_pos.get("iloc").at(0), &n(8)), &len_pt);
                if pl::num_eq(&nlen, &n(8)) {
                    set64u_at(it, nn.clone(), outfile, p)?;
                } else if pl::le(&nn, &Value::Int(0xffffffff)) {
                    set32u_at(it, nn.clone(), outfile, p)?;
                } else {
                    error(it, et, s("Can't yet promote iloc length to 64 bits"))?;
                    return Ok(None);
                }
                nn = n(0);
            }
            if item.get("Extents").len() != 1 {
                error(it, et, cat!("Can't yet handle ", name, " in multiple parts. Please submit sample for testing"))?;
            }
        }
        et.set("CHANGED", changed); // (will set this later if successful in editing mdat)
    }
    raf_seek(it, &raf, cur_pos, 0)?; // seek back to original position

    // add necessary metadata types if they didn't already exist
    let mut count_new = Value::Undef;
    let add = Value::new_hash();
    let used_id = Value::new_hash();
    for name in ["EXIF", "XMP"] {
        if did.get(name).truthy() || !hh(et, "ADD_DIRS").get(name).truthy() {
            continue;
        }
        let mut missing: Vec<Value> = Vec::new();
        for b in ["iinf", "iloc"] {
            if !box_pos.get(b).truthy() {
                missing.push(s(b));
            }
        }
        if !missing.is_empty() {
            let str_ = if missing.len() > 1 { cat!(pl::join(" and ", &missing), " boxes") } else { cat!(pl::join(" ", &missing), " box") };
            warn(it, et, cat!("Can't create ", name, ". Missing expected ", str_))?;
            break;
        }
        if et.get("PrimaryItem").is_undef() {
            if primary.is_undef() {
                warn(it, et, cat!("Can't create ", name, ". No items to reference"))?;
                break;
            }
            // add new primary item reference box after hdrl box
            if pl::lt(&primary, &n(0x10000)) {
                add.set("hdlr", pl::pack("Na4Nn", &[n(14), s("pitm"), n(0), primary.clone()])?);
            } else {
                add.set("hdlr", pl::pack("Na4CCCCN", &[n(16), s("pitm"), n(1), n(0), n(0), n(0), primary.clone()])?);
            }
            m(it, et, "Warn", vec![cat!("Added missing PrimaryItemReference (for item ", primary, ")"), n(1)])?;
        }
        let buff = s("");
        let (hdr, sub_table, proc_);
        if name == "EXIF" {
            hdr = Value::bytes(b"\0\0\0\x06Exif\0\0");
            sub_table = get_tag_table(it, s("Image::ExifTool::Exif::Main"))?;
            proc_ = Value::Code(it.sub_ref("Image::ExifTool::WriteTIFF"));
        } else {
            hdr = s("");
            sub_table = get_tag_table(it, s("Image::ExifTool::XMP::Main"))?;
            proc_ = Value::Undef;
        }
        let d_info = Value::new_hash();
        d_info.set("DataPt", pl::sref(buff));
        d_info.set("DataLen", n(0));
        d_info.set("DirStart", n(0));
        d_info.set("DirLen", n(0));
        let changed = et.get("CHANGED");
        let mut new_val = m(it, et, "WriteDirectory", vec![d_info, sub_table, proc_])?;
        if !new_val.is_undef() && changed.to_bytes() != et.get("CHANGED").to_bytes() {
            let iref_ver;
            if box_pos.get("iref").truthy() {
                iref_ver = get8u(it, outfile, pl::add(&box_pos.get("iref").at(0), &n(8)))?;
            } else {
                // create iref box after end of iinf box (and save version in boxPos list)
                iref_ver = n(if pl::lt(&primary, &n(0x10000)) { 0 } else { 1 });
                let iinf = box_pos.get("iinf");
                box_pos.set("iref", arr(vec![pl::add(&iinf.at(0), &iinf.at(1)), n(0), iref_ver.clone()]));
            }
            if pl::length(&hdr).truthy() {
                new_val = pl::concat(&hdr, &new_val);
            }
            // add new infe to iinf
            if add.get("iinf").is_undef() {
                add.set("iinf", s(""));
                add.set("iref", s(""));
                add.set("iloc", s(""));
            }
            let (type_, mime);
            let mut enc = s("");
            if name == "XMP" {
                type_ = Value::bytes(b"mime\0");
                mime = Value::bytes(b"application/rdf+xml\0");
                // write compressed XMP if Compress option is set
                if m(it, et, "Options", vec![s("Compress")])?.truthy() && pl::length(&new_val).truthy() {
                    if pl::eval_block(it, |it| crate::runtime::require(it, "Compress::Zlib"))?.is_none() {
                        warn(it, et, s("Install Compress::Zlib to write compressed metadata"))?;
                    } else {
                        let deflate = pl::call(it, "Compress::Zlib::deflateInit", vec![])?;
                        if deflate.truthy() {
                            vprint(it, et, 0, cat!("  (Deflating new ", name, " metadata)\n"))?;
                            let b = m(it, &deflate, "deflate", vec![new_val.clone()])?;
                            if !b.is_undef() {
                                let f = m(it, &deflate, "flush", vec![])?;
                                new_val = pl::concat(&b, &f);
                                enc = Value::bytes(b"deflate\0");
                            }
                        }
                    }
                }
            } else {
                type_ = Value::bytes(b"Exif\0");
                mime = s("");
            }
            last_id = pl::add(&last_id, &n(1));
            let id = last_id.clone(); // use next highest available ID (so ID's in iinf are in order)
            // add new infe entry to iinf box
            let mut nn = pl::add(&pl::add(&pl::add(&len_of(&type_), &len_of(&mime)), &len_of(&enc)), &n(16));
            if pl::lt(&id, &n(0x10000)) {
                let p = pl::pack("Na4CCCCnn", &[nn.clone(), s("infe"), n(2), n(0), n(0), n(1), id.clone(), n(0)])?;
                add.set("iinf", cat!(add.get("iinf"), p, type_, mime, enc));
            } else {
                nn = pl::add(&nn, &n(2));
                let p = pl::pack("Na4CCCCNn", &[nn.clone(), s("infe"), n(3), n(0), n(0), n(1), id.clone(), n(0)])?;
                add.set("iinf", cat!(add.get("iinf"), p, type_, mime, enc));
            }
            // add new cdsc to iref (also refer to primary 'tmap' if it exists)
            if iref_ver.truthy() {
                let (fmt, siz, num) = if !tmap.is_undef() { ("N", 22, 2) } else { ("", 18, 1) };
                let p = pl::pack(&format!("Na4NnN{fmt}"), &[n(siz), s("cdsc"), id.clone(), n(num), primary.clone(), tmap.clone()])?;
                add.set("iref", pl::concat(&add.get("iref"), &p));
            } else {
                let (fmt, siz, num) = if !tmap.is_undef() { ("n", 16, 2) } else { ("", 14, 1) };
                let p = pl::pack(&format!("Na4nnn{fmt}"), &[n(siz), s("cdsc"), id.clone(), n(num), primary.clone(), tmap.clone()])?;
                add.set("iref", pl::concat(&add.get("iref"), &p));
            }
            // add new entry to iloc table (see ISO14496-12:2015 pg.79)
            let iloc = box_pos.get("iloc");
            let iloc_ver = get8u(it, outfile, pl::add(&iloc.at(0), &n(8)))?;
            let siz = get16u(it, outfile, pl::add(&iloc.at(0), &n(12)))?; // get size information
            let noff = ops::shift_right(&siz, &n(12));
            let nlen = ops::bit_and(&ops::shift_right(&siz, &n(8)), &n(0x0f));
            let nbas = ops::bit_and(&ops::shift_right(&siz, &n(4)), &n(0x0f));
            let nind = ops::bit_and(&siz, &n(0x0f));
            let (pbas, poff);
            let nvl = len_of(&new_val);
            if pl::num_eq(&iloc_ver, &n(0)) {
                // set offset to 0 as flag that this is a new idat chunk being added
                pbas = pl::add(&len_of(&add.get("iloc")), &n(4));
                poff = pl::add(&pl::add(&pbas, &nbas), &n(2));
                let p = cat!(pl::pack("nn", &[id.clone(), n(0)])?, set_var_int_v(it, n(0), &nbas)?, set16u(it, n(1))?, set_var_int_v(it, n(0), &noff)?, set_var_int_v(it, nvl.clone(), &nlen)?);
                add.set("iloc", pl::concat(&add.get("iloc"), &p));
            } else if pl::num_eq(&iloc_ver, &n(1)) {
                pbas = pl::add(&len_of(&add.get("iloc")), &n(6));
                poff = pl::add(&pl::add(&pl::add(&pbas, &nbas), &n(2)), &nind);
                let p = cat!(pl::pack("nnn", &[id.clone(), n(0), n(0)])?, set_var_int_v(it, n(0), &nbas)?, set16u(it, n(1))?, set_var_int_v(it, n(0), &nind)?, set_var_int_v(it, n(0), &noff)?, set_var_int_v(it, nvl.clone(), &nlen)?);
                add.set("iloc", pl::concat(&add.get("iloc"), &p));
            } else if pl::num_eq(&iloc_ver, &n(2)) {
                pbas = pl::add(&len_of(&add.get("iloc")), &n(8));
                poff = pl::add(&pl::add(&pl::add(&pbas, &nbas), &n(2)), &nind);
                let p = cat!(pl::pack("Nnn", &[id.clone(), n(0), n(0)])?, set_var_int_v(it, n(0), &nbas)?, set16u(it, n(1))?, set_var_int_v(it, n(0), &nind)?, set_var_int_v(it, n(0), &noff)?, set_var_int_v(it, nvl.clone(), &nlen)?);
                add.set("iloc", pl::concat(&add.get("iloc"), &p));
            } else {
                warn(it, et, cat!("Can't create ", name, ". Unsupported iloc version ", iloc_ver))?;
                break;
            }
            // add new ChunkOffset entry to update this new offset
            let off = dir_info.get("ChunkOffset");
            if !off.truthy() {
                warn(it, et, s("Internal error. Missing ChunkOffset"))?;
                break;
            }
            let iend = pl::add(&iloc.at(0), &iloc.at(1));
            let new_off;
            if pl::num_eq(&noff, &n(4)) {
                new_off = arr(vec![s("stco_iloc"), pl::add(&iend, &poff), noff.clone(), n(0), id.clone()]);
            } else if pl::num_eq(&noff, &n(8)) {
                new_off = arr(vec![s("co64_iloc"), pl::add(&iend, &poff), noff.clone(), n(0), id.clone()]);
            } else if pl::num_eq(&noff, &n(0)) {
                // offset_size is zero, so store the offset in base_offset instead
                if pl::num_eq(&nbas, &n(4)) {
                    new_off = arr(vec![s("stco_iloc"), pl::add(&iend, &pbas), nbas.clone(), n(0), id.clone()]);
                } else if pl::num_eq(&nbas, &n(8)) {
                    new_off = arr(vec![s("co64_iloc"), pl::add(&iend, &pbas), nbas.clone(), n(0), id.clone()]);
                } else {
                    warn(it, et, cat!("Can't create ", name, ". Invalid iloc offset+base size"))?;
                    break;
                }
            } else {
                warn(it, et, cat!("Can't create ", name, ". Invalid iloc offset size"))?;
                break;
            }
            // add directory as a new mdat chunk
            off.push(new_off);
            mdat_edit.push(arr(vec![n(0), n(0), pl::sref(new_val), id.clone()]));
            used_id.set(id.to_bytes(), n(1));
            count_new = pl::add(&if count_new.truthy() { count_new.clone() } else { n(0) }, &n(1));
            et.set("CHANGED", changed); // set this later if successful in editing mdat
        }
    }
    if count_new.truthy() {
        // insert new entries into iinf, iref and iloc boxes,
        // and add new pitm box after hdlr if necessary
        let mut added = n(0);
        let mut tags = keys(&box_pos);
        tags.sort_by(|a, b| num_cmp(&box_pos.get(a.to_bytes()).at(0), &box_pos.get(b.to_bytes()).at(0)));
        for tag in tags {
            let bp = box_pos.get(tag.to_bytes());
            add_at(&bp, 0, &added);
            if !add.get(tag.to_bytes()).truthy() {
                continue;
            }
            let pos = bp.at(0);
            if !bp.at(1).truthy() {
                if !pl::str_eq(&tag, "iref") {
                    error(it, et, s("Internal error adding iref box"))?;
                    break;
                }
                // create new iref box
                let a_ = add.get(tag.to_bytes());
                let h = cat!(set32u(it, pl::add(&n(12), &len_of(&a_)))?, tag, set8u(it, bp.at(2))?, b"\0\0\0"[..], a_);
                add.set(tag.to_bytes(), h);
            } else if !pl::str_eq(&tag, "hdlr") {
                let nn = pl::add(&get32u(it, outfile, pos.clone())?, &len_of(&add.get(tag.to_bytes())));
                set32u_at(it, nn, outfile, pos.clone())?; // increase box size
            }
            if pl::str_eq(&tag, "iinf") {
                let iinf_ver = get8u(it, outfile, pl::add(&pos, &n(8)))?;
                if pl::num_eq(&iinf_ver, &n(0)) {
                    let nn = pl::add(&get16u(it, outfile, pl::add(&pos, &n(12)))?, &count_new);
                    if pl::gt(&nn, &n(0xffff)) {
                        error(it, et, s("Can't currently handle rollover to long item count"))?;
                        return Ok(Some(Value::Undef));
                    }
                    set16u_at(it, nn, outfile, pl::add(&pos, &n(12)))?; // incr count
                } else {
                    let nn = pl::add(&get32u(it, outfile, pl::add(&pos, &n(12)))?, &count_new);
                    set32u_at(it, nn, outfile, pl::add(&pos, &n(12)))?; // incr count
                }
            } else if pl::str_eq(&tag, "iref") {
                // nothing more to do
            } else if pl::str_eq(&tag, "iloc") {
                let iloc_ver = get8u(it, outfile, pl::add(&pos, &n(8)))?;
                if pl::lt(&iloc_ver, &n(2)) {
                    let nn = pl::add(&get16u(it, outfile, pl::add(&pos, &n(14)))?, &count_new);
                    set16u_at(it, nn.clone(), outfile, pl::add(&pos, &n(14)))?; // incr count
                    if pl::gt(&nn, &n(0xffff)) {
                        error(it, et, s("Can't currently handle rollover to long item count"))?;
                        return Ok(Some(Value::Undef));
                    }
                } else {
                    let nn = pl::add(&get32u(it, outfile, pl::add(&pos, &n(14)))?, &count_new);
                    set32u_at(it, nn, outfile, pl::add(&pos, &n(14)))?; // incr count
                }
                // must also update pointer locations in this box
                if added.truthy() {
                    for e in dir_info.get("ChunkOffset").items() {
                        add_at(&e, 1, &added);
                    }
                }
            } else if !pl::str_eq(&tag, "hdlr") {
                continue;
            }
            // add new entries to this box (or add pitm after hdlr)
            let a_ = add.get(tag.to_bytes());
            insert_at(outfile, pl::add(&pos, &bp.at(1)).to_i64(), &a_)?;
            add_at(&bp, 1, &len_of(&a_));
            added = pl::add(&added, &len_of(&a_)); // positions are shifted by length of new entries
        }
    }
    et.delete("ItemInfo");
    Ok(Some(if mdat_edit.is_empty() { Value::Undef } else { arr(mdat_edit) }))
}

/// Write a series of QuickTime atoms from file or in memory
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: A) if dirInfo contains DataPt: new directory data
///          B) otherwise: true on success, 0 if a write error occurred
///             (true but sets an Error on a file format error)
/// Notes: Yes, this is a real mess.  Just like the QuickTime metadata situation.
pub fn write_quick_time(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let r = pl::local_topic(it, |it| write_quick_time_v(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2)))?;
    ret(r)
}

// (some assignments are dead in the Perl too, eg. $flags before FormatQTValue)
#[allow(unused_assignments)]
fn write_quick_time_v(it: &mut Interp, et: &Value, dir_info: &Value, tag_table_ptr: &Value) -> ER<Value> {
    if !et.truthy() {
        return Ok(n(1)); // allow dummy access to autoload this package
    }
    let mut mdat: Vec<Value> = Vec::new();
    let mut mdat_edit: Vec<Value> = Vec::new();
    let mut track = Value::Undef;
    let mut term = Value::Undef;
    let mut del_count = Value::Undef;
    let lang_tags = Value::new_hash();
    let mut can_create = Value::Undef;
    let mut del_grp = Value::Undef;
    let box_pos = Value::new_hash();
    let did_dir = Value::new_hash();
    let write_last = Value::new_hash(); // %writeLast
    let mut err = false;
    let mut atom_count = Value::Undef;
    let (mut last_pos, mut err_str, mut trailer, mut buf2) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef);
    let (mut keys_grp, mut keys_path);
    let mut item_index: Option<Value> = None;
    let outfile0 = dir_info.get("OutFile");
    if !outfile0.truthy() {
        return Ok(n(0));
    }
    let mut raf = dir_info.get("RAF"); // (will be null for lower-level atoms)
    let data_pt = dir_info.get("DataPt"); // (will be null for top-level atoms)
    let dir_name = dir_info.get("DirName");
    let dir_start = if dir_info.get("DirStart").truthy() { dir_info.get("DirStart") } else { n(0) };
    let parent = dir_info.get("Parent");
    let add_dirs = et.get("ADD_DIRS");
    let did_tag = et.get("DidTag");
    let mut new_tags = Value::new_hash();
    let mut create_keys = n(0);
    let (mut rtn_val, rtn_err) = if data_pt.truthy() { (Value::Undef, Value::Undef) } else { (n(1), n(0)) };
    let av_type = pkg_h(it, "avType");
    let max_read_len = lex_s(it, "maxReadLen");
    let und_lang = lex_s(it, "undLang");
    let qt_format = lex_h(it, "qtFormat");

    // check for trailer at end of file
    if raf.truthy() {
        trailer = qt(it, "IdentifyTrailers", vec![raf.clone()])?;
        if trailer.truthy() && !trailer.is_ref() {
            error(it, et, trailer)?;
            return Ok(n(1));
        }
    }
    if data_pt.truthy() {
        raf = pl::call(it, "File::RandomAccess::new", vec![s("File::RandomAccess"), data_pt.clone()])?;
    } else if !raf.truthy() {
        return Ok(n(0));
    }
    // use buffered output for everything but 'mdat' atoms
    let outfile = pl::sref(s(""));

    if dir_start.truthy() {
        raf_seek(it, &raf, dir_start.clone(), 1)?; // skip header if it exists
    }

    let at = av_type.get(et.get("MediaType").to_bytes());
    if at.truthy() {
        // (note: these won't be correct now if we haven't yet processed the Media box,
        // but in this case they won't be needed until after we set them properly below)
        keys_grp = cat!(at, "Keys");
        keys_path = s("MOV-Movie-Track");
    } else {
        keys_grp = s("Keys");
        keys_path = s("MOV-Movie");
    }
    let cur_path = pl::join("-", &et.get("PATH").items());
    let mut write_path = dir_name.clone();
    let mut dir = dir_name.clone();
    loop {
        dir = hh(et, "DirMap").get(dir.to_bytes());
        if dir.is_undef() {
            break;
        }
        write_path = cat!(dir, "-", write_path);
    }
    // hack to create Keys directories if necessary (its containing Meta is in a different location)
    if add_dirs.get("Keys").truthy() && re(r"^MOV-Movie(-Meta)?$", "").is_match(&cur_path) {
        create_keys = n(1); // create new Keys directories
    } else if (add_dirs.get("AudioKeys").truthy() || add_dirs.get("VideoKeys").truthy()) && re(r"^MOV-Movie-Track(-Meta)?$", "").is_match(&cur_path) {
        create_keys = n(-1); // (must wait until MediaType is known)
    } else if pl::str_eq(&cur_path, "MOV-Movie-Meta-ItemList") || (pl::str_eq(&cur_path, "MOV-Movie-Track-Meta-ItemList") && at.truthy()) {
        create_keys = n(2); // create new Keys tags
        let keys_ = et.get(keys_grp.to_bytes());
        if keys_.truthy() {
            // add new tag entries for existing Keys tags, now that we know their ID's
            // - first make lookup to convert Keys tagInfo ref to index number
            let keys_info = Value::new_hash();
            for index in keys(&hh(&keys_, "Info")) {
                if hh(&keys_, "Remap").get(index.to_bytes()).truthy() {
                    keys_info.set(hh(&keys_, "Info").get(index.to_bytes()).to_bytes(), index.clone());
                }
            }
            let keys_table = get_tag_table(it, cat!("Image::ExifTool::QuickTime::", keys_grp))?;
            let new_keys_tags = m(it, et, "GetNewTagInfoHash", vec![keys_table])?;
            for k in keys(&new_keys_tags) {
                let tag_info = new_keys_tags.get(k.to_bytes());
                let mut index = keys_info.get(tag_info.to_bytes());
                if !index.truthy() {
                    let sti = tag_info.get("SrcTagInfo");
                    index = if sti.truthy() { keys_info.get(sti.to_bytes()) } else { sti };
                }
                if !index.truthy() {
                    continue;
                }
                let mut id = set32u(it, index)?;
                if tag_info.get("LangCode").truthy() {
                    // add to lookup of language tags we are writing with this ID
                    if !lang_tags.get(id.to_bytes()).truthy() {
                        lang_tags.set(id.to_bytes(), Value::new_hash());
                    }
                    lang_tags.get(id.to_bytes()).set(k.to_bytes(), tag_info.clone());
                    id = cat!(id, "-", tag_info.get("LangCode"));
                }
                new_tags.set(id.to_bytes(), tag_info);
            }
        }
    } else {
        // get hash of new tags to edit/create in this directory
        new_tags = m(it, et, "GetNewTagInfoHash", vec![tag_table_ptr.clone()])?;
        // make lookup of language tags for each ID
        for k in keys(&new_tags) {
            let nt = new_tags.get(k.to_bytes());
            if !(nt.get("LangCode").truthy() && nt.get("SrcTagInfo").truthy()) {
                continue;
            }
            let id = nt.get("SrcTagInfo").get("TagID");
            if !lang_tags.get(id.to_bytes()).truthy() {
                lang_tags.set(id.to_bytes(), Value::new_hash());
            }
            lang_tags.get(id.to_bytes()).set(k.to_bytes(), nt);
        }
    }
    if cur_path.to_bytes() == write_path.to_bytes() || create_keys.truthy() {
        can_create = n(1);
        // (must check the appropriate Keys delete flag if this is a Keys ItemList)
        del_grp = hh(et, "DEL_GROUP").get(if create_keys.truthy() { keys_grp.to_bytes() } else { dir_name.to_bytes() });
    }
    if tag_table_ptr.get("VARS").truthy() {
        atom_count = tag_table_ptr.get("VARS").get("ATOM_COUNT");
    }

    let mut tag = s("");
    let mut last_tag = s("");
    if pl::str_eq(&dir_name, "ItemPropertyContainer") {
        item_index = Some(n(0));
    }

    // read ahead to parse item property associations if this is 'iprp' ItemProperties
    // (necessary to determine which properties belong to primary item in HEIC file)
    if pl::str_eq(&dir_name, "ItemProperties") {
        let pos = raf_tell(it, &raf)?;
        loop {
            if !pl::num_eq(&raf_read(it, &raf, &mut buf2, n(8))?, &n(8)) {
                break;
            }
            let size = pl::sub(&get32u(it, &pl::sref(buf2.clone()), n(0))?, &n(8)); // (atom size without 8-byte header)
            tag = pl::substr(&buf2, 4, Some(4));
            if pl::lt(&size, &n(0)) {
                break;
            }
            if !pl::str_eq(&tag, "ipma") {
                raf_seek(it, &raf, size, 1)?;
                continue;
            }
            if pl::num_eq(&raf_read(it, &raf, &mut buf2, size.clone())?, &size) {
                qt(it, "ParseItemPropAssoc", vec![buf2.clone(), et.clone()])?;
            }
            break;
        }
        m(it, &raf, "Seek", vec![pos])?;
    }
    let max_len_ok = |size: &Value| pl::gt(size, &max_read_len);
    'atoms: loop {
        // loop through all atoms at this level
        if let Some(ii) = item_index.as_mut() {
            *ii = pl::add(ii, &n(1));
        }
        last_pos = raf_tell(it, &raf)?;
        // stop processing if we reached a known trailer
        if trailer.truthy() && pl::ge(&last_pos, &trailer.at(1)) {
            if !pl::num_eq(&last_pos, &trailer.at(1)) {
                err_str = cat!("Corrupted ", trailer.at(0), " trailer");
            }
            break;
        }
        if tag_table_ptr.get(tag.to_bytes()).truthy() {
            last_tag = tag.clone(); // keep track of last known tag
        }
        if !atom_count.is_undef() {
            atom_count = pl::sub(&atom_count, &n(1));
            if pl::lt(&atom_count, &n(0)) && data_pt.truthy() {
                // stop processing now and just copy the rest of the atom
                let t = raf_tell(it, &raf)?;
                if !write(it, &outfile, &[pl::substr(&data_pt.deref(), t.to_i64(), None)])? {
                    rtn_val = rtn_err.clone();
                    err = true;
                }
                break;
            }
        }
        let mut hdr = Value::Undef;
        let mut buff = Value::Undef;
        let mut keys_index;
        let nread = raf_read(it, &raf, &mut hdr, n(8))?;
        if !pl::num_eq(&nread, &n(8)) {
            if pl::num_eq(&nread, &n(4)) && pl::str_eq(&hdr, "\0\0\0\0") {
                // "for historical reasons" the udta is optionally terminated by 4 zeros (ref 1)
                // --> hold this terminator to the end
                term = hdr.clone();
            } else if !pl::num_eq(&nread, &n(0)) {
                // warn unless this is 1-3 pad bytes
                if pl::gt(&nread, &n(3)) || hdr.to_bytes()[..] != vec![0u8; nread.to_i64().max(0) as usize][..] {
                    m(it, et, "Error", vec![cat!("Unknown ", nread, " bytes at end of file"), n(1)])?;
                }
            }
            break;
        }
        let mut size = pl::sub(&get32u(it, &pl::sref(hdr.clone()), n(0))?, &n(8)); // (atom size without 8-byte header)
        tag = pl::substr(&hdr, 4, Some(4));
        if pl::num_eq(&size, &n(-7)) {
            // read the extended size
            if !pl::num_eq(&raf_read(it, &raf, &mut buff, n(8))?, &n(8)) {
                err_str = s("Truncated extended atom");
                break;
            }
            hdr = pl::concat(&hdr, &buff);
            let u = pl::unpack("NN", &buff)?;
            let (hi, lo) = (arg(&u, 0), arg(&u, 1));
            if hi.truthy() || pl::gt(&lo, &n(0x7fffffff)) {
                if pl::gt(&hi, &n(0x7fffffff)) {
                    err_str = s("Invalid atom size");
                    break;
                } else if !m(it, et, "Options", vec![s("LargeFileSupport")])?.truthy() {
                    error(it, et, s("End of processing at large atom (LargeFileSupport not enabled)"))?;
                    break;
                } else if pl::str_eq(&m(it, et, "Options", vec![s("LargeFileSupport")])?, "2") {
                    warn(it, et, s("Processing large atom (LargeFileSupport is 2)"))?;
                }
            }
            size = pl::sub(&pl::add(&pl::mul(&hi, &Value::Int(4294967296)), &lo), &n(16));
            if pl::lt(&size, &n(0)) {
                err_str = s("Invalid extended atom size");
                break;
            }
        } else if pl::num_eq(&size, &n(-8)) {
            if data_pt.truthy() {
                if pl::str_eq(&dir_info.get("DirName"), "CanonCNTH") {
                    break; // (this is normal for Canon CNTH atom)
                }
                let pos = pl::sub(&raf_tell(it, &raf)?, &n(4));
                raf_seek(it, &raf, n(0), 2)?;
                let str_ = cat!(dir_info.get("DirName"), " with ", pl::sub(&raf_tell(it, &raf)?, &pos), " bytes");
                m(it, et, "Error", vec![cat!("Terminator found in ", str_, " remaining"), n(1)])?;
            } else {
                // size of zero is only valid for top-level atom, and
                // indicates the atom extends to the end of file
                // (save in mdat list to write later; with zero end position to copy rest of file)
                let t = raf_tell(it, &raf)?;
                mdat.push(arr(vec![t, n(0), hdr.clone()]));
            }
            break;
        } else if pl::lt(&size, &n(0)) {
            err_str = s("Invalid atom size");
            break;
        }

        // keep track of 'mdat' atom locations for writing later
        if pl::str_eq(&tag, "mdat") {
            if data_pt.truthy() {
                error(it, et, s("'mdat' not at top level"))?;
                break;
            }
            let t = raf_tell(it, &raf)?;
            mdat.push(arr(vec![t.clone(), pl::add(&t, &size), hdr.clone()]));
            if !raf_seek(it, &raf, size.clone(), 1)? {
                error(it, et, s("Seek error in mdat atom"))?;
                return Ok(rtn_val);
            }
            continue;
        } else if pl::str_eq(&tag, "cmov") {
            error(it, et, s("Can't yet write compressed movie metadata"))?;
            return Ok(rtn_val);
        } else if pl::str_eq(&tag, "wide") {
            if size.truthy() {
                warn(it, et, cat!("Incorrect size for 'wide' atom (", size, " bytes)"))?;
                if !raf_seek(it, &raf, size.clone(), 1)? {
                    error(it, et, s("Truncated wide atom"))?;
                }
            }
            continue; // drop 'wide' tag
        }

        // read the atom data
        let got;
        if !size.truthy() {
            buff = s("");
            got = n(0);
        } else {
            // read the atom data (but only first 64kB if data is huge)
            let l = if max_len_ok(&size) { n(0x10000) } else { size.clone() };
            got = raf_read(it, &raf, &mut buff, l)?;
        }
        if !pl::num_eq(&got, &size) {
            // ignore up to 256 bytes of garbage at end of file
            let mut type_ = Value::Undef;
            let c1 = pl::le(&got, &n(256)) && pl::ge(&size, &n(1024)) && !pl::str_eq(&tag, "mdat");
            let c2 = !c1 && pl::lt(&got, &n(3000)) && re(r"^<b[r>]", "").is_match(&pl::pack("N", &[size.clone()])?) && {
                type_ = s("extraneous HTML");
                true
            };
            if c1 || c2 {
                let bytes = pl::add(&got, &len_of(&hdr));
                if !type_.truthy() {
                    type_ = s("garbage");
                }
                if hh(et, "OPTIONS").get("IgnoreMinorErrors").truthy() {
                    warn(it, et, cat!("Deleted ", type_, " at end of file (", bytes, " bytes)"))?;
                    buff = s("");
                    hdr = s("");
                } else {
                    m(it, et, "Error", vec![cat!("Possible ", type_, " at end of file (", bytes, " bytes)"), n(1)])?;
                    return Ok(rtn_val);
                }
            } else {
                tag = qt(it, "PrintableTagID", vec![tag.clone(), n(3)])?;
                if max_len_ok(&size) && pl::num_eq(&got, &n(0x10000)) {
                    let mb = pl::int(&pl::add(&pl::div(&size, &n(0x100000))?, &Value::Num(0.5)));
                    err_str = cat!("'", tag, "' atom is too large for rewriting (", mb, " MB)");
                } else {
                    err_str = cat!("Truncated '", tag, "' atom");
                }
                break;
            }
        }
        // save the handler type of the track media
        let path = et.get("PATH");
        if pl::str_eq(&tag, "hdlr") && pl::ge(&len_of(&buff), &n(12)) && !path.is_empty() && pl::str_eq(&path.at(-1), "Media") {
            et.set("MediaType", pl::substr(&buff, 8, Some(4)));
        }
        // if this atom stores offsets, save its location so we can fix up offsets later
        // (are there any other atoms that may store absolute file offsets?)
        if re(r"^(stco|co64|iloc|mfra|moof|sidx|saio|gps |CTBO|uuid)$", "").is_match(&tag) {
            // (note that we only need to do this if the media data is stored in this file)
            let flg = et.get("QtDataFlg");
            if pl::str_eq(&tag, "mfra") || pl::str_eq(&tag, "moof") {
                error(it, et, s("Can't yet handle movie fragments when writing"))?;
                return Ok(rtn_val);
            } else if pl::str_eq(&tag, "sidx") || pl::str_eq(&tag, "saio") {
                error(it, et, cat!("Can't yet handle ", tag, " box when writing"))?;
                return Ok(rtn_val);
            } else if pl::str_eq(&tag, "iloc") {
                let r = pl::sref(buff.clone());
                if handle_iloc_v(it, et, dir_info, &r, &outfile)? == 0 {
                    error(it, et, s("Error parsing iloc atom"))?;
                }
                buff = r.deref();
            } else if pl::str_eq(&tag, "gps ") {
                // (only care about the 'gps ' box in 'moov')
                if dir_info.get("DirID").truthy() && pl::str_eq(&dir_info.get("DirID"), "moov") && pl::gt(&len_of(&buff), &n(8)) {
                    let off = dir_info.get("ChunkOffset");
                    let mut num = get32u(it, &pl::sref(buff.clone()), n(4))?;
                    if pl::gt(&pl::add(&pl::mul(&num, &n(8)), &n(8)), &len_of(&buff)) {
                        num = pl::int(&pl::div(&pl::sub(&len_of(&buff), &n(8)), &n(8))?);
                    }
                    let mut i = n(0);
                    while pl::lt(&i, &num) {
                        let p = pl::add(&pl::add(&pl::add(&dlen(&outfile), &len_of(&hdr)), &n(8)), &pl::mul(&i, &n(8)));
                        off.push(arr(vec![s("stco_gps "), p, n(4)]));
                        i = pl::add(&i, &n(1));
                    }
                }
            } else if pl::str_eq(&tag, "CTBO") || pl::str_eq(&tag, "uuid") {
                // hack for updating CR3 CTBO offsets
                dir_info.get_or_new_array("ChunkOffset").push(arr(vec![tag.clone(), dlen(&outfile), pl::add(&len_of(&hdr), &size)]));
            } else if !flg.truthy() || pl::num_eq(&flg, &n(1)) {
                // assume "1" if stsd is yet to be read
                if !flg.truthy() {
                    et.set("AssumedDataRef", n(1));
                }
                // must update offsets since the data is in this file
                dir_info.get_or_new_array("ChunkOffset").push(arr(vec![tag.clone(), pl::add(&dlen(&outfile), &len_of(&hdr)), size.clone()]));
            } else if pl::num_eq(&flg, &n(3)) {
                error(it, et, s("Can't write files with mixed internal/external media data"))?;
                return Ok(rtn_val);
            }
        }

        // rewrite this atom
        let mut tag_info = m(it, et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone(), pl::sref(buff.clone())])?;

        // call write hook if it exists
        if tag_info.truthy() && tag_info.get("WriteHook").truthy() {
            pl::call_code(it, &tag_info.get("WriteHook"), vec![buff.clone(), et.clone()], Want::Scalar)?;
        }

        // allow numerical tag ID's (ItemList entries defined by Keys)
        if !tag_info.truthy() && pl::str_eq(&dir_name, "ItemList") && et.get(keys_grp.to_bytes()).truthy() {
            keys_index = pl::unpack1("N", &tag)?;
            let kg = et.get(keys_grp.to_bytes());
            let new_index = hh(&kg, "Remap").get(keys_index.to_bytes());
            if !new_index.is_undef() {
                tag_info = hh(&kg, "Info").get(keys_index.to_bytes());
                if !new_index.truthy() {
                    if tag_info.truthy() {
                        vprint(it, et, 1, cat!("    - Keys:", tag_info.get("Name")))?;
                    } else {
                        del_count = pl::add(&if del_count.truthy() { del_count.clone() } else { n(0) }, &n(1));
                    }
                    inc_changed(et);
                    continue;
                }
                // use the new Keys index of this item if it changed
                if !pl::num_eq(&keys_index, &new_index) {
                    tag = set32u(it, new_index)?;
                    hdr = substr_set(&hdr, 4, 4, &tag);
                }
            } else {
                keys_index = Value::Undef;
            }
            let _ = keys_index;
        }
        // delete all ItemList tags when deleting group, but take care not to delete UserData Meta
        if del_grp.truthy() {
            if pl::str_eq(&dir_name, "ItemList") {
                del_count = pl::add(&if del_count.truthy() { del_count.clone() } else { n(0) }, &n(1));
                inc_changed(et);
                continue;
            } else if pl::str_eq(&dir_name, "UserData") && (!tag_info.truthy() || !tag_info.get("SubDirectory").truthy()) {
                del_count = pl::add(&if del_count.truthy() { del_count.clone() } else { n(0) }, &n(1));
                inc_changed(et);
                continue;
            }
        }
        if tag_info.truthy() && tag_info.get("AddedUnknown").truthy() {
            tag_info = Value::Undef;
        }

        // don't write this tag unless associated with the primary item
        // (Note: This relies on iinf and dimg coming before iprp)
        if let (Some(ii), true) = (item_index.clone(), et.get("ItemInfo").truthy()) {
            let items = et.get("ItemInfo");
            let mut is_primary = false;
            let mut primary = et.get("PrimaryItem");
            if primary.is_undef() {
                if et.get("ItemInfo").truthy() {
                    primary = sorted_num_keys(&et.get("ItemInfo")).into_iter().next().unwrap_or_default();
                }
                if primary.is_undef() {
                    primary = n(0);
                }
            }
            let mut pitem = items.get(primary.to_bytes());
            if !pitem.truthy() {
                pitem = Value::new_hash();
            }
            if !pitem.get("RefersTo").truthy() {
                pitem.set("RefersTo", Value::new_hash());
            }
            let dont_inherit = pkg_h(it, "dontInherit");
            let mut ids = sorted_num_keys(&items);
            ids.reverse();
            'item_id2: for id in ids {
                if !items.get(id.to_bytes()).get("Association").truthy() {
                    continue;
                }
                let item = items.get(id.to_bytes());
                for prop in item.get("Association").items() {
                    if !pl::num_eq(&prop, &ii) {
                        continue;
                    }
                    let d = dont_inherit.get(tag.to_bytes());
                    let dont = if d.truthy() { d } else { n(0) };
                    if !(pl::num_eq(&id, &primary)
                        || (!dont.truthy() && item.get("RefersTo").truthy() && item.get("RefersTo").get(primary.to_bytes()).truthy())
                        // special case to assume this property belongs to the primary
                        // image if it belongs to an item referred to by the primary
                        || (!pl::num_eq(&dont, &n(1)) && hh(&pitem, "RefersTo").get(id.to_bytes()).truthy()))
                    {
                        break;
                    }
                    is_primary = true;
                    break 'item_id2;
                }
            }
            if !is_primary {
                tag_info = Value::Undef;
            }
        }

        if tag_info.truthy() && (tag_info.get("Writable").is_undef() || tag_info.get("Writable").truthy()) {
            let subdir = tag_info.get("SubDirectory");
            let mut new_data = Value::Undef;
            let chunk_offset = arr(Vec::new());

            if subdir.truthy() {
                // process atoms in this container from a buffer in memory

                if pl::str_eq(&tag, "trak") {
                    et.set("MediaType", s("")); // init media type for this track
                    et.delete("AssumedDataRef");
                }
                let sdn = subdir.get("DirName");
                let sub_name = if sdn.truthy() { sdn } else { tag_info.get("Name") };
                let start = if subdir.get("Start").truthy() { subdir.get("Start") } else { n(0) };
                let b0 = if dir_info.get("Base").truthy() { dir_info.get("Base") } else { n(0) };
                let mut base = pl::sub(&pl::add(&b0, &raf_tell(it, &raf)?), &size);
                let mut d_pos = n(0);
                let mut hdr_len = start.clone();
                // handle case where known trailer is the payload of a "DontRead" box (eg. 'inst')
                if tag_info.get("DontRead").truthy() && trailer.truthy() && pl::num_eq(&base, &trailer.at(1)) {
                    trailer = trailer.at(3);
                }
                if subdir.get("Base").truthy() {
                    //#### eval Base ($start, $base, $dPos, $hdrLen, $size, $tag, $buff, $hdr...)
                    let scope = crate::runtime::file_scope(it, PKG);
                    let vars = [
                        ("et", et.clone()),
                        ("dirInfo", dir_info.clone()),
                        ("tagTablePtr", tag_table_ptr.clone()),
                        ("raf", raf.clone()),
                        ("dataPt", data_pt.clone()),
                        ("dirName", dir_name.clone()),
                        ("tag", tag.clone()),
                        ("hdr", hdr.clone()),
                        ("buff", buff.clone()),
                        ("size", size.clone()),
                        ("tagInfo", tag_info.clone()),
                        ("subdir", subdir.clone()),
                        ("subName", sub_name.clone()),
                        ("start", start.clone()),
                        ("base", base.clone()),
                        ("dPos", d_pos.clone()),
                        ("hdrLen", hdr_len.clone()),
                    ];
                    let (local_base, _) = pl::eval_site_in(it, &subdir.get("Base"), PKG, Some(scope), &vars, Want::Scalar);
                    d_pos = pl::sub(&d_pos, &local_base);
                    base = pl::sub(&base, &d_pos);
                    // get length of header before base offset
                    if pl::le(&local_base, &hdr_len) {
                        hdr_len = pl::sub(&hdr_len, &local_base);
                    }
                }
                let subdir_info = Value::new_hash();
                subdir_info.set("Parent", dir_name.clone());
                subdir_info.set("DirName", sub_name.clone());
                subdir_info.set("Name", tag_info.get("Name"));
                subdir_info.set("TagInfo", tag_info.clone());
                subdir_info.set("DirID", tag.clone());
                let buff_ref = pl::sref(buff.clone());
                subdir_info.set("DataPt", buff_ref.clone());
                subdir_info.set("DataLen", size.clone());
                subdir_info.set("DataPos", d_pos.clone());
                subdir_info.set("DirStart", start.clone());
                subdir_info.set("DirLen", pl::sub(&size, &start));
                subdir_info.set("Base", base.clone());
                subdir_info.set("HasData", subdir.get("HasData"));
                subdir_info.set("Multi", subdir.get("Multi")); // necessary?
                subdir_info.set("OutFile", outfile.clone());
                subdir_info.set("NoRefTest", n(1)); // don't check directory references
                subdir_info.set("WriteGroup", tag_info.get("WriteGroup"));
                subdir_info.set("Permanent", tag_info.get("Permanent"));
                // initialize array to hold details about chunk offset table
                // (each entry has 3-5 items: 0=atom type, 1=table offset, 2=table size,
                //  3=optional base offset, 4=optional item ID)
                subdir_info.set("ChunkOffset", chunk_offset.clone());
                // set InPlace flag so XMP will be padded properly when
                // QuickTimePad is used if this is an XMP directory
                if m(it, et, "Options", vec![s("QuickTimePad")])?.truthy() {
                    subdir_info.set("InPlace", n(2));
                }
                // pass the header pointer if necessary (for EXIF IFD's
                // where the Base offset is at the end of the header)
                if hdr_len.truthy() && pl::lt(&hdr_len, &size) {
                    let header = pl::substr(&buff, 0, Some(hdr_len.to_i64()));
                    subdir_info.set("HeaderPtr", pl::sref(header));
                }
                if subdir.get("ByteOrder").truthy() && re(r"^Little", "").is_match(&subdir.get("ByteOrder")) {
                    set_byte_order(it, "II")?;
                }
                let old_write_group = et.get("CUR_WRITE_GROUP");
                if pl::str_eq(&sub_name, "Track") {
                    if !track.truthy() {
                        track = n(0);
                    }
                    track = pl::add(&track, &n(1));
                    et.set("CUR_WRITE_GROUP", cat!("Track", track));
                }
                let sub_table = get_tag_table(it, subdir.get("TagTable"))?;
                // demote non-QuickTime errors to warnings
                if !pl::str_eq(&hh(&sub_table, "GROUPS").get("0"), "QuickTime") {
                    et.set("DemoteErrors", n(1));
                }
                let old_changed = et.get("CHANGED");
                new_data = m(it, et, "WriteDirectory", vec![subdir_info.clone(), sub_table, subdir.get("WriteProc")])?;
                buff = buff_ref.deref();
                if et.get("DemoteErrors").truthy() {
                    // just copy existing subdirectory if a non-quicktime error occurred
                    if pl::gt(&et.get("DemoteErrors"), &n(1)) {
                        et.set("CHANGED", old_changed.clone());
                    }
                    et.delete("DemoteErrors");
                }
                if !new_data.is_undef() && !pl::length(&new_data).truthy() && (tag_info.get("Permanent").truthy() || (tag_table_ptr.get("PERMANENT").truthy() && tag_info.get("Permanent").is_undef())) {
                    // do nothing if trying to delete tag from a PERMANENT table
                    et.set("CHANGED", old_changed);
                    new_data = Value::Undef;
                }
                if pl::str_eq(&tag, "trak") {
                    et.set("MediaType", s("")); // reset media type at end of track
                    if et.get("AssumedDataRef").truthy() {
                        let cwg = et.get("CUR_WRITE_GROUP");
                        let grp = if cwg.truthy() { cwg } else { dir_name.clone() };
                        error(it, et, cat!("Can't locate data reference to update offsets for ", grp))?;
                        et.delete("AssumedDataRef");
                    }
                }
                et.set("CUR_WRITE_GROUP", old_write_group);
                set_byte_order(it, "MM")?;
                // add back header if necessary
                if start.truthy() && !new_data.is_undef() && (pl::length(&new_data).truthy() || (!tag_info.get("Permanent").is_undef() && !tag_info.get("Permanent").truthy())) {
                    new_data = pl::concat(&pl::substr(&buff, 0, Some(start.to_i64())), &new_data);
                    for e in chunk_offset.items() {
                        add_at(&e, 1, &start);
                    }
                }
                // the directory exists, so we don't need to add it
                if cur_path.to_bytes() == write_path.to_bytes() && add_dirs.get(sub_name.to_bytes()).truthy() && add_dirs.get(sub_name.to_bytes()).to_bytes() == dir_name.to_bytes() {
                    add_dirs.delete(sub_name.to_bytes());
                }
                did_dir.set(tag.to_bytes(), n(1)); // (note: keyed by tag ID)
            } else {
                // modify existing QuickTime tags in various formats

                let mut nv_hash = m(it, et, "GetNewValueHash", vec![tag_info.clone()])?;
                if nv_hash.truthy() || lang_tags.get(tag.to_bytes()).truthy() || del_grp.truthy() {
                    let nv_hash_no_lang = nv_hash.clone();
                    let (mut val, mut len, mut lang, mut type_, mut flags, mut ctry) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef, Value::Undef, Value::Undef);
                    let mut charset_quick_time = Value::Undef;
                    let mut format = tag_info.get("Format");
                    let has_data = dir_info.get("HasData").truthy() && re(r"\0...data\0", "s").is_match(&buff);
                    let mut lang_info = tag_info.clone();
                    let string_encoding = pkg_h(it, "stringEncoding");
                    if has_data {
                        let mut pos = n(0);
                        loop {
                            {
                                if pl::gt(&pl::add(&pos, &n(16)), &size) {
                                    // add any new alternate language tags now
                                    if lang_tags.get(tag.to_bytes()).truthy() {
                                        let mut tgs = vec![s("")];
                                        tgs.extend(sorted_keys(&lang_tags.get(tag.to_bytes())));
                                        for tg in tgs {
                                            let ti = if tg.truthy() { lang_tags.get(tag.to_bytes()).get(tg.to_bytes()) } else { nv_hash_no_lang.clone() };
                                            nv_hash = m(it, et, "GetNewValueHash", vec![ti.clone()])?;
                                            if !(nv_hash.truthy() && !did_tag.get(nv_hash.to_bytes()).truthy()) {
                                                continue;
                                            }
                                            did_tag.set(nv_hash.to_bytes(), n(1));
                                            if !(nv_hash.get("IsCreating").truthy() && m(it, et, "IsOverwriting", vec![nv_hash.clone()])?.truthy()) {
                                                continue;
                                            }
                                            let new_val = m(it, et, "GetNewValue", vec![nv_hash.clone()])?;
                                            if new_val.is_undef() {
                                                continue;
                                            }
                                            let pr_val = new_val.clone();
                                            let nvr = pl::sref(new_val);
                                            let flags = format_qt_value_v(it, et, &nvr, &tag_info, format.clone())?;
                                            let new_val = nvr.deref();
                                            if new_val.is_undef() {
                                                continue;
                                            }
                                            let (mut ctry, mut lang) = (n(0), n(0));
                                            if ti.get("LangCode").truthy() {
                                                let Some(c) = re(r"^([A-Z]{3})?[-_]?([A-Z]{2})?$", "i").m(&ti.get("LangCode")) else {
                                                    warn(it, et, cat!("Invalid language code for ", ti.get("Name")))?;
                                                    continue;
                                                };
                                                // pack language and country codes
                                                (ctry, lang) = pack_lang(&cap(&c, 1), &cap(&c, 2))?;
                                            }
                                            if !lang.truthy() && et.get("OPTIONS").get("QuickTimeUndLang").truthy() {
                                                lang = und_lang.clone();
                                            }
                                            if new_data.is_undef() {
                                                new_data = pl::substr(&buff, 0, Some(pos.to_i64()));
                                            }
                                            let p = pl::pack("Na4Nnn", &[pl::add(&n(16), &len_of(&new_val)), s("data"), flags, ctry, lang])?;
                                            new_data = cat!(new_data, p, new_val);
                                            let grp = m(it, et, "GetGroup", vec![ti.clone(), n(1)])?;
                                            m(it, et, "VerboseValue", vec![cat!("+ ", grp, ":", ti.get("Name")), pr_val])?;
                                            inc_changed(et);
                                        }
                                    }
                                    break;
                                }
                                let u = pl::unpack(&format!("x{}Na4Nnn", pos.to_text()), &buff)?;
                                len = arg(&u, 0);
                                type_ = arg(&u, 1);
                                flags = arg(&u, 2);
                                ctry = arg(&u, 3);
                                lang = arg(&u, 4);
                                if !lang.truthy() {
                                    lang = und_lang.clone(); // treat both 0 and 'und' as 'und'
                                }
                                lang_info = tag_info.clone();
                                let mut del_tag = del_grp.truthy();
                                let mut new_val = Value::Undef;
                                let mut lang_code = qt(it, "GetLangCode", vec![lang.clone(), ctry.clone(), n(1)])?;
                                loop {
                                    lang_info = get_lang_info_v(it, &tag_info, lang_code.clone())?;
                                    nv_hash = m(it, et, "GetNewValueHash", vec![lang_info.clone()])?;
                                    if nv_hash.truthy() || !ctry.truthy() || lang.to_bytes() != und_lang.to_bytes() || pl::num_eq(&pl::length(&lang_code), &n(2)) {
                                        break;
                                    }
                                    // check to see if tag was written with a 2-char country code only
                                    lang_code = pl::lc(&pl::unpack1("a2", &pl::pack("n", &[ctry.clone()])?)?);
                                }
                                // set flag to delete language tag when writing default
                                // (except for a default-language Keys entry)
                                if !nv_hash.truthy() && nv_hash_no_lang.truthy() {
                                    if lang.to_bytes() == und_lang.to_bytes() && !ctry.truthy() && !did_tag.get(nv_hash_no_lang.to_bytes()).truthy() {
                                        nv_hash = nv_hash_no_lang.clone(); // write existing default
                                    } else {
                                        del_tag = true; // delete tag
                                    }
                                }
                                if pl::gt(&pl::add(&pos, &len), &size) {
                                    break;
                                }
                                if pl::str_eq(&type_, "data") && pl::ge(&len, &n(16)) {
                                    pos = pl::add(&pos, &n(16));
                                    len = pl::sub(&len, &n(16));
                                    val = pl::substr(&buff, pos.to_i64(), Some(len.to_i64()));
                                    // decode value (see QuickTime.pm for an explanation)
                                    if string_encoding.get(flags.to_bytes()).truthy() {
                                        val = m(it, et, "Decode", vec![val, string_encoding.get(flags.to_bytes())])?;
                                        if !tag_info.get("Binary").truthy() {
                                            re(r"\0$", "").subst(&mut val, false, |_| Vec::new());
                                        }
                                        flags = n(0x01); // write all strings as UTF-8
                                    } else {
                                        if format.truthy() {
                                            // update flags for the format we are writing
                                            let w = tag_info.get("Writable");
                                            if w.truthy() && qt_format.get(w.to_bytes()).truthy() {
                                                flags = qt_format.get(w.to_bytes());
                                            } else if qt_format.get(format.to_bytes()).truthy() {
                                                flags = qt_format.get(format.to_bytes());
                                            }
                                        } else {
                                            format = qt(it, "QuickTimeFormat", vec![flags.clone(), len.clone()])?;
                                        }
                                        if format.truthy() {
                                            val = da(it, part2::read_value, vec![pl::sref(val), n(0), format.clone(), tag_info.get("Count"), len.clone()])?;
                                        }
                                    }
                                    if (nv_hash.truthy() && m(it, et, "IsOverwriting", vec![nv_hash.clone(), val.clone()])?.truthy()) || del_tag {
                                        if !nv_hash.is_undef() {
                                            new_val = m(it, et, "GetNewValue", vec![nv_hash.clone()])?;
                                        }
                                        if del_tag || new_val.is_undef() || did_tag.get(nv_hash.to_bytes()).truthy() {
                                            // delete the tag
                                            let grp = m(it, et, "GetGroup", vec![lang_info.clone(), n(1)])?;
                                            m(it, et, "VerboseValue", vec![cat!("- ", grp, ":", lang_info.get("Name")), val.clone()])?;
                                            // copy data up to start of this tag to delete this value
                                            if new_data.is_undef() {
                                                new_data = pl::substr(&buff, 0, Some(pl::sub(&pos, &n(16)).to_i64()));
                                            }
                                            inc_changed(et);
                                            pos = pl::add(&pos, &len);
                                            continue;
                                        }
                                        let pr_val = new_val.clone();
                                        // format new value for writing (and get new flags)
                                        let nvr = pl::sref(new_val);
                                        flags = format_qt_value_v(it, et, &nvr, &tag_info, format.clone())?;
                                        new_val = nvr.deref();
                                        if new_val.is_undef() {
                                            pos = pl::add(&pos, &len);
                                            continue;
                                        }
                                        let grp = m(it, et, "GetGroup", vec![lang_info.clone(), n(1)])?;
                                        m(it, et, "VerboseValue", vec![cat!("- ", grp, ":", lang_info.get("Name")), val.clone()])?;
                                        m(it, et, "VerboseValue", vec![cat!("+ ", grp, ":", lang_info.get("Name")), pr_val])?;
                                        if new_data.is_undef() {
                                            new_data = pl::substr(&buff, 0, Some(pl::sub(&pos, &n(16)).to_i64()));
                                        }
                                        let mut w_lang = lang.clone();
                                        if w_lang.to_bytes() == und_lang.to_bytes() && !et.get("OPTIONS").get("QuickTimeUndLang").truthy() {
                                            w_lang = n(0);
                                        }
                                        let p = pl::pack("Na4Nnn", &[pl::add(&len_of(&new_val), &n(16)), type_.clone(), flags.clone(), ctry.clone(), w_lang])?;
                                        new_data = cat!(new_data, p, new_val);
                                        inc_changed(et);
                                    } else if !new_data.is_undef() {
                                        new_data = pl::concat(&new_data, &pl::substr(&buff, pl::sub(&pos, &n(16)).to_i64(), Some(pl::add(&len, &n(16)).to_i64())));
                                    }
                                } else if !new_data.is_undef() {
                                    new_data = pl::concat(&new_data, &pl::substr(&buff, pos.to_i64(), Some(len.to_i64())));
                                }
                                if nv_hash.truthy() {
                                    did_tag.set(nv_hash.to_bytes(), n(1));
                                }
                            }
                            // (the loop increment: $pos += $len)
                            pos = pl::add(&pos, &len);
                        }
                        if !new_data.is_undef() && pl::lt(&pos, &size) {
                            new_data = pl::concat(&new_data, &pl::substr(&buff, pos.to_i64(), None));
                        }
                        val = Value::Undef; // (already constructed $newData)
                    } else if format.truthy() {
                        val = da(it, part2::read_value, vec![pl::sref(buff.clone()), n(0), format.clone(), Value::Undef, size.clone()])?;
                    } else if (re(r"^\xa9", "").is_match(&tag) || tag_info.get("IText").truthy()) && pl::ge(&size, &if tag_info.get("IText").truthy() { tag_info.get("IText") } else { n(4) }) {
                        let mut hdr2 = Value::Undef;
                        let itext = tag_info.get("IText");
                        if itext.truthy() && pl::ge(&itext, &n(6)) {
                            let pos = pl::sub(&itext, &n(2));
                            lang = pl::unpack1(&format!("x{}n", pos.to_text()), &buff)?;
                            hdr2 = pl::substr(&buff, 4, Some(pl::sub(&itext, &n(6)).to_i64()));
                            len = pl::sub(&size, &itext);
                            val = pl::substr(&buff, itext.to_i64(), Some(len.to_i64()));
                        } else {
                            let u = pl::unpack("nn", &buff)?;
                            len = arg(&u, 0);
                            lang = arg(&u, 1);
                            if pl::gt(&pl::add(&n(4), &len), &size) {
                                len = pl::sub(&len, &n(4)); // (see QuickTime.pm for explanation)
                            }
                            if pl::gt(&len, &pl::sub(&size, &n(4))) || pl::lt(&len, &n(0)) {
                                len = pl::sub(&size, &n(4));
                            }
                            val = pl::substr(&buff, 4, Some(len.to_i64()));
                        }
                        if !lang.truthy() {
                            lang = und_lang.clone(); // treat both 0 and 'und' as 'und'
                        }
                        let enc;
                        if pl::lt(&lang, &n(0x400)) && !re(r"^\xfe\xff", "").is_match(&val) {
                            charset_quick_time = m(it, et, "Options", vec![s("CharsetQuickTime")])?;
                            enc = charset_quick_time.clone();
                        } else {
                            enc = if re(r"^\xfe\xff", "").subst(&mut val, false, |_| Vec::new()) > 0 { s("UTF16") } else { s("UTF8") };
                        }
                        if !tag_info.get("NoDecode").truthy() {
                            val = m(it, et, "Decode", vec![val, enc])?;
                            re(r"\0+$", "").subst(&mut val, false, |_| Vec::new()); // remove trailing nulls if they exist
                        }
                        if !hdr2.is_undef() {
                            val = pl::concat(&hdr2, &val);
                        }
                        let lang_code = qt(it, "UnpackLang", vec![lang.clone(), n(1)])?;
                        lang_info = get_lang_info_v(it, &tag_info, lang_code)?;
                        nv_hash = m(it, et, "GetNewValueHash", vec![lang_info.clone()])?;
                        if !nv_hash.truthy() && nv_hash_no_lang.truthy() {
                            if lang.to_bytes() == und_lang.to_bytes() && !did_tag.get(nv_hash_no_lang.to_bytes()).truthy() {
                                nv_hash = nv_hash_no_lang.clone();
                            } else if can_create.truthy() {
                                // delete other languages when writing default
                                let grp = m(it, et, "GetGroup", vec![lang_info.clone(), n(1)])?;
                                m(it, et, "VerboseValue", vec![cat!("- ", grp, ":", lang_info.get("Name")), val.clone()])?;
                                inc_changed(et);
                                continue 'atoms;
                            }
                        }
                    } else {
                        val = buff.clone();
                        if re(r"^\xa9", "").is_match(&tag) || tag_info.get("IText").truthy() {
                            warn(it, et, cat!("Corrupted ", tag_info.get("Name"), " value"))?;
                        }
                    }
                    if nv_hash.truthy() && !val.is_undef() {
                        if m(it, et, "IsOverwriting", vec![nv_hash.clone(), val.clone()])?.truthy() {
                            new_data = m(it, et, "GetNewValue", vec![nv_hash.clone()])?;
                            if new_data.is_undef() && !can_create.truthy() {
                                new_data = s("");
                            }
                            inc_changed(et);
                            let grp = m(it, et, "GetGroup", vec![lang_info.clone(), n(1)])?;
                            m(it, et, "VerboseValue", vec![cat!("- ", grp, ":", lang_info.get("Name")), val.clone()])?;
                            if !(!new_data.is_undef() && !did_tag.get(nv_hash.to_bytes()).truthy()) {
                                // must not delete items from iprp because it will mess up the ordering
                                if item_index.is_none() {
                                    continue 'atoms;
                                }
                            }
                            m(it, et, "VerboseValue", vec![cat!("+ ", grp, ":", lang_info.get("Name")), new_data.clone()])?;
                            // add back necessary header and encode as necessary
                            if !lang.is_undef() {
                                let itext = if tag_info.get("IText").truthy() { tag_info.get("IText") } else { n(0) };
                                let mut hdr2 = Value::Undef;
                                if pl::gt(&itext, &n(6)) {
                                    let k = pl::sub(&itext, &n(6));
                                    if pl::lt(&len_of(&new_data), &k) {
                                        new_data = pl::concat(&new_data, &s(&" ".repeat(k.to_i64().max(0) as usize)));
                                    }
                                    hdr2 = pl::substr(&new_data, 0, Some(k.to_i64()));
                                    new_data = pl::substr(&new_data, k.to_i64(), None);
                                }
                                if !tag_info.get("NoDecode").truthy() {
                                    let e = if pl::lt(&lang, &n(0x400)) { charset_quick_time.clone() } else { s("UTF8") };
                                    new_data = m(it, et, "Encode", vec![new_data, e])?;
                                }
                                let mut w_lang = lang.clone();
                                        if w_lang.to_bytes() == und_lang.to_bytes() && !et.get("OPTIONS").get("QuickTimeUndLang").truthy() {
                                            w_lang = n(0);
                                        }
                                if pl::lt(&itext, &n(6)) {
                                    new_data = pl::concat(&pl::pack("nn", &[len_of(&new_data), w_lang])?, &new_data);
                                } else if pl::num_eq(&itext, &n(6)) {
                                    new_data = cat!(pl::pack("Nn", &[n(0), w_lang])?, new_data, b"\0"[..]);
                                } else {
                                    new_data = cat!(b"\0\0\0\0"[..], hdr2, pl::pack("n", &[w_lang])?, new_data, b"\0"[..]);
                                }
                            } else if !format.truthy() || (re(r"^string", "").is_match(&format) && !tag_info.get("Binary").truthy() && !tag_info.get("ValueConv").truthy()) {
                                // write all strings as UTF-8
                                new_data = m(it, et, "Encode", vec![new_data, s("UTF8")])?;
                            } else if format.truthy() && !tag_info.get("Binary").truthy() {
                                // format new value for writing
                                new_data = etc(it, "WriteValue", vec![new_data, format.clone(), tag_info.get("Count")])?;
                            }
                        }
                        did_tag.set(nv_hash.to_bytes(), n(1)); // set flag so we don't add this tag again
                    }
                }
            }
            // write the new atom if it was modified
            if !new_data.is_undef() {
                let size_diff = pl::sub(&len_of(&buff), &len_of(&new_data));
                // pad to original size if specified, otherwise give verbose message about the changed size
                if pl::gt(&size_diff, &n(0)) && tag_info.get("PreservePadding").truthy() && m(it, et, "Options", vec![s("QuickTimePad")])?.truthy() {
                    new_data = pl::concat(&new_data, &Value::bytes(vec![0u8; size_diff.to_i64() as usize]));
                    vprint(it, et, 1, cat!("    (", tag_info.get("Name"), " padded to original size)"))?;
                } else if size_diff.truthy() {
                    vprint(it, et, 1, cat!("    (", tag_info.get("Name"), " changed size)"))?;
                }
                let len = pl::add(&len_of(&new_data), &n(8));
                if pl::gt(&len, &Value::Int(0x7fffffff)) {
                    error(it, et, cat!(tag_info.get("Name"), " to large to write"))?;
                    break;
                }
                // update size in ChunkOffset list for modified 'uuid' atom
                if pl::str_eq(&tag, "uuid") {
                    dir_info.get_or_new_array("ChunkOffset").at(-1).set_at(2, len.clone());
                }
                if !pl::gt(&len, &n(8)) {
                    continue; // don't write empty atom header
                }
                // maintain pointer to chunk offsets if necessary
                if !chunk_offset.is_empty() {
                    let d = pl::add(&n(8), &dlen(&outfile));
                    for e in chunk_offset.items() {
                        add_at(&e, 1, &d);
                    }
                    let co = dir_info.get_or_new_array("ChunkOffset");
                    for e in chunk_offset.items() {
                        co.push(e);
                    }
                }
                if tag_info.get("WriteLast").truthy() {
                    let s32 = set32u(it, len.clone())?;
                    let ord = tag_info.get("WriteLast");
                    write_last.set(ord.to_bytes(), cat!(or_empty(write_last.get(ord.to_bytes())), s32, tag, new_data));
                } else {
                    box_pos.set(tag.to_bytes(), arr(vec![dlen(&outfile), pl::add(&len_of(&new_data), &n(8))]));
                    // write the updated directory with its atom header
                    let s32 = set32u(it, len)?;
                    if !write(it, &outfile, &[s32, tag.clone(), new_data])? {
                        rtn_val = rtn_err.clone();
                        err = true;
                        break;
                    }
                }
                continue;
            }
        }
        // keep track of data references in this track
        if pl::str_eq(&tag, "dinf") {
            et.set("QtDataRef", arr(Vec::new())); // initialize list of data references
        } else if pl::str_eq(&parent, "DataInfo") && pl::ge(&len_of(&buff), &n(4)) {
            // save data reference type and version/flags
            let v = get32u(it, &pl::sref(buff.clone()), n(0))?;
            et.get_or_new_array("QtDataRef").push(arr(vec![tag.clone(), v]));
        } else if pl::str_eq(&tag, "stsd") && pl::ge(&len_of(&buff), &n(8)) {
            let br = pl::sref(buff.clone());
            let nn = get32u(it, &br, n(4))?; // get number of sample descriptions in table
            let (mut pos, mut flg) = (n(8), n(0));
            let mut msg = Value::Undef;
            let mut i = n(0);
            while pl::lt(&i, &nn) {
                // loop through sample descriptions
                if !pl::le(&pl::add(&pos, &n(16)), &len_of(&buff)) {
                    msg = s("Truncated sample table");
                    break;
                }
                let siz = get32u(it, &br, pos.clone())?;
                if !pl::le(&pl::add(&pos, &siz), &len_of(&buff)) {
                    msg = s("Truncated sample table");
                    break;
                }
                let dref_idx = get16u(it, &br, pl::add(&pos, &n(14)))?;
                let dref_tbl = et.get("QtDataRef");
                let di = pl::sub(&dref_idx, &n(1)).to_i64();
                if !dref_idx.truthy() {
                    flg = ops::bit_or(&flg, &n(0x01)); // in this file if data reference index is 0 (if like iloc)
                } else if dref_tbl.truthy() && dref_tbl.at(di).truthy() {
                    let dref = dref_tbl.at(di);
                    // $flg = 0x01-in this file, 0x02-in some other file
                    let f = if pl::num_eq(&dref.at(1), &n(1)) && !pl::str_eq(&dref.at(0), "rsrc") { 0x01 } else { 0x02 };
                    flg = ops::bit_or(&flg, &n(f));
                } else {
                    msg = cat!("No data reference for sample description ", i);
                    break;
                }
                pos = pl::add(&pos, &siz);
                i = pl::add(&i, &n(1));
            }
            if msg.truthy() {
                // (allow empty sample description for non-audio/video handler types, eg. 'url ', 'meta')
                // (also, incorrectly written 'mett' SampleEntry by Google phones,
                //  see https://exiftool.org/forum/index.php?msg=91158)
                if av_type.get(et.get("MediaType").to_bytes()).truthy() {
                    let cwg = et.get("CUR_WRITE_GROUP");
                    let grp = if cwg.truthy() { cwg } else { parent.clone() };
                    error(it, et, cat!(msg, " for ", grp))?;
                    return Ok(rtn_err);
                }
                flg = n(1); // (this seems to be the case)
            }
            et.set("QtDataFlg", flg.clone());
            if et.get("AssumedDataRef").truthy() {
                if !pl::num_eq(&flg, &et.get("AssumedDataRef")) {
                    let cwg = et.get("CUR_WRITE_GROUP");
                    let grp = if cwg.truthy() { cwg } else { parent.clone() };
                    error(it, et, cat!("Assumed incorrect data reference for ", grp, " (was ", flg, ")"))?;
                }
                et.delete("AssumedDataRef");
            }
        }
        if tag_info.truthy() && tag_info.get("WriteLast").truthy() {
            let ord = tag_info.get("WriteLast");
            write_last.set(ord.to_bytes(), cat!(or_empty(write_last.get(ord.to_bytes())), hdr, buff));
        } else {
            // save position of this box in the output buffer
            box_pos.set(tag.to_bytes(), arr(vec![dlen(&outfile), pl::add(&len_of(&hdr), &len_of(&buff))]));
            // copy the existing atom
            if !write(it, &outfile, &[hdr, buff])? {
                rtn_val = rtn_err.clone();
                err = true;
                break;
            }
        }
    }
    // ($errStr is set if there was an error that could possibly be due to an unknown trailer)
    if err_str.truthy() {
        let tt = tag_table_ptr.get(tag.to_bytes());
        if (pl::str_eq(&last_tag, "mdat") || pl::str_eq(&last_tag, "moov")) && !data_pt.truthy() && (!tt.truthy() || (tt.ref_type() == "HASH" && tt.get("Unknown").truthy())) {
            // identify other known trailers from their first bytes
            buf2 = s("");
            if raf_seek(it, &raf, last_pos.clone(), 0)? {
                raf_read(it, &raf, &mut buf2, n(8))?;
            }
            let type_ = if pl::str_eq(&buf2, "CCCCCCCC") {
                s("Kenwood")
            } else if re(r"^(gpsa|gps0|gsen|gsea)...\0", "s").is_match(&buf2) {
                s("RIFF")
            } else {
                s("Unknown")
            };
            // determine length of this trailer
            let len;
            if trailer.truthy() {
                len = pl::sub(&trailer.at(1), &last_pos); // runs to start of next trailer
            } else {
                if !raf_seek(it, &raf, n(0), 2)? {
                    error(it, et, s("Seek error"))?;
                    return Ok(if data_pt.truthy() { Value::Undef } else { n(1) });
                }
                len = pl::sub(&raf_tell(it, &raf)?, &last_pos); // runs to end of file
            }
            // add to start of linked list of trailers
            trailer = arr(vec![type_, last_pos.clone(), len, trailer]);
        } else {
            error(it, et, err_str)?;
            return Ok(if data_pt.truthy() { Value::Undef } else { n(1) });
        }
    }
    if del_count.truthy() {
        vprint(it, et, 0, cat!("  [deleting ", del_count, " ", dir_name, " tag", if pl::num_eq(&del_count, &n(1)) { "" } else { "s" }, "]\n"))?;
    }

    // can finally set necessary variables for creating Video/AudioKeys tags
    if pl::lt(&create_keys, &n(0)) {
        let at = av_type.get(et.get("MediaType").to_bytes());
        if at.truthy() {
            create_keys = n(1);
            keys_grp = cat!(at, "Keys");
            keys_path = s("MOV-Movie-Track");
        } else {
            can_create = n(0);
        }
    }
    if !add_dirs.get(keys_grp.to_bytes()).truthy() {
        create_keys = ops::bit_and(&create_keys, &ops::bit_not(&n(0x01))); // (Keys may have been written)
    }

    // add new directories/tags at this level if necessary
    if can_create.truthy() && (hh(et, "EDIT_DIRS").exists(dir_name.to_bytes()) || create_keys.truthy()) {
        // get a hash of tagInfo references to add to this directory
        let mut dirs = m(it, et, "GetAddDirHash", vec![tag_table_ptr.clone(), dir_name.clone()])?;
        // make sorted list of new tags to be added
        let mut add_tags: Vec<Value> = keys(&dirs);
        add_tags.extend(keys(&new_tags));
        add_tags.sort_by(|a, b| a.to_bytes().cmp(&b.to_bytes()));
        // add Keys tags if necessary
        if create_keys.truthy() {
            if cur_path.to_bytes() == keys_path.to_bytes() {
                // add Meta for Keys if necessary
                if !did_dir.get("meta").truthy() {
                    let mm = pl::pkg_hash(it, "Image::ExifTool::QuickTime::Movie").get("meta");
                    dirs.set("meta", mm);
                    add_tags.push(s("meta"));
                }
            } else if cur_path.to_bytes() == cat!(keys_path, "-Meta").to_bytes() {
                // special case for Keys Meta -- reset directories and start again
                add_tags.clear();
                dirs = Value::new_hash();
                for k in ["keys", "ilst"] {
                    if did_dir.get(k).truthy() {
                        continue; // don't add again
                    }
                    let mm = pl::pkg_hash(it, "Image::ExifTool::QuickTime::Meta").get(k);
                    dirs.set(k, mm);
                    add_tags.push(s(k));
                }
            } else if cur_path.to_bytes() == cat!(keys_path, "-Meta-ItemList").to_bytes() && et.get(keys_grp.to_bytes()).truthy() {
                let kadd = hh(&et.get(keys_grp.to_bytes()), "Add");
                for index in sorted_num_keys(&kadd) {
                    let id = set32u(it, index.clone())?;
                    new_tags.set(id.to_bytes(), kadd.get(index.to_bytes()));
                    add_tags.push(id);
                }
            } else {
                dirs = m(it, et, "GetAddDirHash", vec![tag_table_ptr.clone(), dir_name.clone()])?;
                add_tags.extend(sorted_keys(&dirs));
            }
        }
        // (note that $tag may be a binary Keys index here)
        for mut tag in add_tags {
            let mut tag_info = dirs.get(tag.to_bytes());
            if !tag_info.truthy() {
                tag_info = new_tags.get(tag.to_bytes());
            }
            if tag_info.ref_type() != "HASH" {
                // (shouldn't happen, but somehow there is forum17260)
                // (also can happen if Meta exists but Keys does not since 'keys' is an array ref.
                //  SonyPMW-EX1R.mp4 has a Movie-Meta atom with XML and no Keys that triggers this
                //  issue.  Note that in this case the Meta HandlerType is 'meta' instead of 'mdta',
                //  which isn't a problem for ExifTool, but may be for other software?)
                if tag_info.ref_type() != "ARRAY" {
                    continue;
                }
                tag_info = m(it, et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
                if !tag_info.truthy() {
                    continue;
                }
            }
            if !tag_info.get("CanCreate").is_undef() && !tag_info.get("CanCreate").truthy() {
                continue;
            }
            if !tag_info.get("MediaType").is_undef() && et.get("MediaType").to_bytes() != tag_info.get("MediaType").to_bytes() {
                continue;
            }
            let subdir = tag_info.get("SubDirectory");
            if !subdir.truthy() {
                let nv_hash = m(it, et, "GetNewValueHash", vec![tag_info.clone()])?;
                if !(nv_hash.truthy() && !did_tag.get(nv_hash.to_bytes()).truthy()) {
                    continue;
                }
                if !(nv_hash.get("IsCreating").truthy() && m(it, et, "IsOverwriting", vec![nv_hash.clone()])?.truthy()) {
                    continue;
                }
                let new_val = m(it, et, "GetNewValue", vec![nv_hash.clone()])?;
                if new_val.is_undef() {
                    continue;
                }
                let pr_val = new_val.clone();
                let nvr = pl::sref(new_val);
                let flags = format_qt_value_v(it, et, &nvr, &tag_info, Value::Undef)?;
                let mut new_val = nvr.deref();
                if new_val.is_undef() {
                    continue;
                }
                let (mut ctry, mut lang) = (n(0), n(0));
                // handle alternate languages
                if tag_info.get("LangCode").truthy() {
                    tag = pl::substr(&tag, 0, Some(4)); // strip language code from tag ID
                    let Some(c) = re(r"^([A-Z]{3})?[-_]?([A-Z]{2})?$", "i").m(&tag_info.get("LangCode")) else {
                        warn(it, et, cat!("Invalid language code for ", tag_info.get("Name")))?;
                        continue;
                    };
                    // pack language and country codes
                    (ctry, lang) = pack_lang(&cap(&c, 1), &cap(&c, 2))?;
                }
                if !lang.truthy() && et.get("OPTIONS").get("QuickTimeUndLang").truthy() {
                    lang = und_lang.clone();
                }
                if dir_info.get("HasData").truthy() {
                    // add 'data' header
                    new_val = cat!(pl::pack("Na4Nnn", &[pl::add(&n(16), &len_of(&new_val)), s("data"), flags, ctry, lang])?, new_val);
                } else if re(r"^\xa9", "").is_match(&tag) || tag_info.get("IText").truthy() {
                    if ctry.truthy() {
                        let grp = m(it, et, "GetGroup", vec![tag_info.clone(), n(1)])?;
                        warn(it, et, cat!("Can't use country code for ", grp, ":", tag_info.get("Name")))?;
                        continue;
                    } else if tag_info.get("IText").truthy() && pl::ge(&tag_info.get("IText"), &n(6)) {
                        // add 6-byte langText header and trailing null
                        // (with extra junk before language code if IText > 6)
                        let k = pl::sub(&tag_info.get("IText"), &n(6)).to_i64();
                        if (len_of(&new_val).to_i64()) < k {
                            new_val = pl::concat(&new_val, &s(&" ".repeat(k.max(0) as usize)));
                        }
                        new_val = cat!(b"\0\0\0\0"[..], pl::substr(&new_val, 0, Some(k)), pl::pack("n", &[lang.clone()])?, pl::substr(&new_val, k, None), b"\0"[..]);
                    } else {
                        // add IText header
                        new_val = cat!(pl::pack("nn", &[len_of(&new_val), lang])?, new_val);
                    }
                } else if ctry.truthy() || (lang.truthy() && lang.to_bytes() != und_lang.to_bytes()) {
                    let grp = m(it, et, "GetGroup", vec![tag_info.clone(), n(1)])?;
                    warn(it, et, cat!("Can't use language code for ", grp, ":", tag_info.get("Name")))?;
                    continue;
                }
                let s32 = set32u(it, pl::add(&n(8), &len_of(&new_val)))?;
                if tag_info.get("WriteLast").truthy() {
                    let ord = tag_info.get("WriteLast");
                    write_last.set(ord.to_bytes(), cat!(or_empty(write_last.get(ord.to_bytes())), s32, tag, new_val));
                } else {
                    box_pos.set(tag.to_bytes(), arr(vec![dlen(&outfile), pl::add(&n(8), &len_of(&new_val))]));
                    if !write(it, &outfile, &[s32, tag.clone(), new_val])? {
                        rtn_val = rtn_err.clone();
                        err = true;
                    }
                }
                let grp = m(it, et, "GetGroup", vec![tag_info.clone(), n(1)])?;
                m(it, et, "VerboseValue", vec![cat!("+ ", grp, ":", tag_info.get("Name")), pr_val])?;
                did_tag.set(nv_hash.to_bytes(), n(1));
                inc_changed(et);
                continue;
            }
            let sdn = subdir.get("DirName");
            let sub_name = if sdn.truthy() { sdn } else { tag_info.get("Name") };
            // QuickTime hierarchy is complex, so check full directory path before adding
            if create_keys.truthy() && cur_path.to_bytes() == keys_path.to_bytes() && pl::str_eq(&sub_name, "Meta") {
                vprint(it, et, 0, s("  Creating Meta with mdta Handler and Keys\n"))?;
                // init Meta box for Keys tags with mdta Handler and empty Keys+ItemList
                buf2 = Value::bytes(b"\0\0\0\x20hdlr\0\0\0\0\0\0\0\0mdta\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\x10keys\0\0\0\0\0\0\0\0\0\0\0\x08ilst");
            } else if create_keys.truthy() && cur_path.to_bytes() == cat!(keys_path, "-Meta").to_bytes() {
                buf2 = if pl::str_eq(&sub_name, "Keys") { Value::bytes(vec![0u8; 8]) } else { s("") };
            } else if pl::str_eq(&sub_name, "Meta") && hh(et, "OPTIONS").get("QuickTimeHandler").truthy() {
                vprint(it, et, 0, s("  Creating Meta with mdir Handler\n"))?;
                // init Meta box for ItemList tags with mdir Handler
                buf2 = Value::bytes(b"\0\0\0\x20hdlr\0\0\0\0\0\0\0\0mdir\0\0\0\0\0\0\0\0\0\0\0\0");
            } else {
                if !(cur_path.to_bytes() == write_path.to_bytes() && add_dirs.get(sub_name.to_bytes()).truthy() && add_dirs.get(sub_name.to_bytes()).to_bytes() == dir_name.to_bytes()) {
                    continue;
                }
                buf2 = s(""); // write from scratch
            }
            let subdir_info = Value::new_hash();
            subdir_info.set("Parent", dir_name.clone());
            subdir_info.set("DirName", sub_name.clone());
            subdir_info.set("DataPt", pl::sref(buf2.clone()));
            subdir_info.set("DirStart", n(0));
            subdir_info.set("HasData", subdir.get("HasData"));
            subdir_info.set("OutFile", outfile.clone());
            subdir_info.set("ChunkOffset", arr(Vec::new())); // (just to be safe)
            subdir_info.set("WriteGroup", tag_info.get("WriteGroup"));
            let sub_table = get_tag_table(it, subdir.get("TagTable"))?;
            let new_data = m(it, et, "WriteDirectory", vec![subdir_info, sub_table, subdir.get("WriteProc")])?;
            if new_data.truthy() && pl::le(&len_of(&new_data), &Value::Int(0x7ffffff7)) {
                let mut prefix = s("");
                // add atom version or ID if necessary
                if subdir.get("Start").truthy() {
                    if pl::num_eq(&subdir.get("Start"), &n(4)) {
                        prefix = Value::bytes(b"\0\0\0\0"); // a simple version number
                    } else {
                        // get UUID from Condition expression
                        let cond = tag_info.get("Condition");
                        if cond.truthy() {
                            if let Some(c) = re(r"=~\s*/\^(.*)/", "").m(&cond) {
                                let code = cat!("\"", cap(&c, 1), "\"");
                                let (p, _) = { let fs = crate::runtime::file_scope(it, PKG); pl::eval_site_in(it, &code, PKG, Some(fs), &[("et", et.clone()), ("dirInfo", dir_info.clone()), ("tagInfo", tag_info.clone()), ("cond", cond.clone())], Want::Scalar) };
                                prefix = p;
                            }
                        }
                        if !pl::num_eq(&len_of(&prefix), &subdir.get("Start")) {
                            error(it, et, s("Internal UUID error"))?;
                        }
                    }
                }
                let new_hdr = cat!(set32u(it, pl::add(&pl::add(&n(8), &len_of(&new_data)), &len_of(&prefix)))?, tag, prefix);
                if tag_info.get("WriteLast").truthy() {
                    let ord = tag_info.get("WriteLast");
                    write_last.set(ord.to_bytes(), cat!(or_empty(write_last.get(ord.to_bytes())), new_hdr, new_data));
                } else {
                    if pl::str_eq(&tag, "uuid") {
                        // add offset for new uuid (needed for CR3 CTBO offsets)
                        let off = dir_info.get_or_new_array("ChunkOffset");
                        off.push(arr(vec![tag.clone(), dlen(&outfile), pl::add(&len_of(&new_hdr), &len_of(&new_data))]));
                    }
                    box_pos.set(tag.to_bytes(), arr(vec![dlen(&outfile), pl::add(&len_of(&new_hdr), &len_of(&new_data))]));
                    if !write(it, &outfile, &[new_hdr, new_data])? {
                        rtn_val = rtn_err.clone();
                        err = true;
                    }
                }
            }
            // add only once (must delete _after_ call to WriteDirectory())
            // (Keys tags are a special case, and are handled separately)
            if !create_keys.truthy() {
                add_dirs.delete(sub_name.to_bytes());
            }
        }
    }
    // write HEIC metadata after top-level 'meta' box has been processed if editing this information
    if pl::str_eq(&cur_path, "MOV-Meta") && hh(et, "EDIT_DIRS").get("ItemInformation").truthy() {
        dir_info.set("BoxPos", box_pos.clone());
        let mdat_edit_ = write_item_info_v(it, et, dir_info, &outfile)?.unwrap_or_default();
        if mdat_edit_.truthy() {
            if et.get("mdatEdit").truthy() {
                error(it, et, s("Multiple top-level Meta containers"))?;
            }
            et.set("mdatEdit", mdat_edit_);
        }
    }
    // write out any necessary terminator
    if term.truthy() && pl::length(&outfile.deref()).truthy() && !write(it, &outfile, &[term.clone()])? {
        rtn_val = rtn_err.clone();
        err = true;
    }

    // delete temporary Keys variables after Meta is processed
    if pl::str_eq(&dir_name, "Meta") {
        // delete any Meta box with no useful information (ie. only 'hdlr','keys','lang','ctry')
        let empty_meta = lex_h(it, "emptyMeta");
        let mut is_empty = true;
        for k in keys(&box_pos) {
            if !empty_meta.get(k.to_bytes()).truthy() {
                is_empty = false;
                break;
            }
        }
        if is_empty {
            if !box_pos.is_empty() {
                let mut names: Vec<Value> = keys(&box_pos).iter().map(|k| empty_meta.get(k.to_bytes())).collect();
                names.sort_by(|a, b| a.to_bytes().cmp(&b.to_bytes()));
                vprint(it, et, 0, cat!("  Deleting ", pl::join("+", &names)))?;
            }
            pl::set_deref(&outfile, s(""))?;
            // (could report a file if editing nothing when it contained an empty Meta atom)
            // ++$$et{CHANGED};
        }
        if cur_path.to_bytes() == cat!(keys_path, "-Meta").to_bytes() {
            add_dirs.delete(keys_grp.to_bytes()); // prevent creation of another Meta for Keys tags
            et.delete(keys_grp.to_bytes());
        }
    }

    // return now if writing subdirectory
    if data_pt.truthy() {
        if write_last.len() > 0 {
            error(it, et, s("Internal error: WriteLast not on top-level atom!\n"))?;
        }
        return Ok(if err { Value::Undef } else { outfile.deref() });
    }

    // issue minor error if we didn't find an 'mdat' atom
    let off = dir_info.get("ChunkOffset");
    if mdat.is_empty() {
        for co in off.items() {
            if pl::str_eq(&co.at(0), "uuid") {
                continue;
            }
            error(it, et, s("Media data referenced but not found"))?;
            return Ok(rtn_val);
        }
        m(it, et, "Warn", vec![s("No media data"), n(1)])?;
    }

    // edit mdat blocks as required
    // (0=old pos [0 if creating], 1=old end [0 if creating], 2=new data ref or undef to delete,
    //  3=new data item id)
    if et.get("mdatEdit").truthy() {
        mdat_edit = et.get("mdatEdit").items();
        et.delete("mdatEdit");
    }
    for edit in &mdat_edit {
        let mut this_mdat: Vec<Value> = Vec::new();
        let mut new_mdat: Vec<Value> = Vec::new();
        let mut changed = false;
        for md in &mdat {
            // keep track of all chunks for the mdat with this header
            if pl::length(&md.at(2)).truthy() {
                new_mdat.append(&mut this_mdat);
            }
            this_mdat.push(md.clone());
            // is this edit inside this mdat chunk?
            // - $$edit[0] and $$edit[1] will both be zero if we are creating a new chunk
            // - $$mdat[1] is zero if mdat runs to end of file
            // - $$edit[0] == $$edit[1] == $$mdat[0] if reviving a deleted chunk
            // - $$mdat[5] is defined if this was a newly added/edited chunk
            if !md.at(5).is_undef() || changed {
                continue; // don't replace a newly added chunk
            }
            let (e0, e1) = (edit.at(0), edit.at(1));
            let (m0, m1) = (md.at(0), md.at(1));
            if !e0.truthy() ||    // (newly created chunk)
                // (edit is inside chunk)
                (((pl::lt(&e0, &m1) || !m1.truthy()) && pl::gt(&e1, &m0)) ||
                // (edit inserted at start or end of chunk)
                (pl::num_eq(&e0, &e1) && (pl::num_eq(&e0, &m0) || pl::num_eq(&e0, &m1))))
            {
                if !edit.at(0).truthy() {
                    edit.set_at(0, md.at(0)); // insert at start of this mdat
                    edit.set_at(1, md.at(0));
                } else if pl::lt(&edit.at(0), &md.at(0)) || (pl::gt(&edit.at(1), &md.at(1)) && md.at(1).truthy()) {
                    error(it, et, s("ItemInfo runs across mdat boundary"))?;
                    return Ok(rtn_val);
                }
                let hdr_chunk = this_mdat[0].clone();
                if !hdr_chunk.truthy() {
                    error(it, et, s("Internal error finding mdat header"))?;
                    return Ok(rtn_val);
                }
                // calculate difference in mdat size
                let new_len = if edit.at(2).truthy() { dlen(&edit.at(2)) } else { n(0) };
                let diff = pl::sub(&new_len, &pl::sub(&edit.at(1), &edit.at(0)));
                // edit size of mdat in header if necessary
                if diff.truthy() {
                    let hl = len_of(&hdr_chunk.at(2));
                    if pl::num_eq(&hl, &n(8)) {
                        let r = pl::sref(hdr_chunk.at(2));
                        let mut size = get32u(it, &r, n(0))?;
                        if size.truthy() {
                            // (0 size = extends to end of file)
                            size = pl::add(&size, &diff);
                            if pl::gt(&size, &Value::Int(0xffffffff)) {
                                error(it, et, s("Can't yet grow mdat across 4GB boundary"))?;
                                return Ok(rtn_val);
                            }
                            set32u_at(it, size, &r, n(0))?;
                            hdr_chunk.set_at(2, r.deref());
                        }
                    } else if pl::num_eq(&hl, &n(16)) {
                        let r = pl::sref(hdr_chunk.at(2));
                        let mut size = get64u(it, &r, n(8))?;
                        if size.truthy() {
                            size = pl::add(&size, &diff);
                            set64u_at(it, size, &r, n(8))?;
                            hdr_chunk.set_at(2, r.deref());
                        }
                    } else {
                        error(it, et, s("Internal error. Invalid mdat header"))?;
                        return Ok(rtn_val);
                    }
                }
                changed = true;
                // remove the edited section of this chunk (if any) and replace with new data (if any)
                if pl::gt(&edit.at(0), &md.at(0)) {
                    if edit.at(2).truthy() {
                        this_mdat.push(arr(vec![edit.at(0), edit.at(1), s(""), n(0), edit.at(2), edit.at(3)]));
                    }
                    // add remaining data after edit (or empty stub in case it is referenced by an offset)
                    this_mdat.push(arr(vec![edit.at(1), md.at(1), s("")]));
                    md.set_at(1, edit.at(0)); // now ends at start of edit
                } else {
                    if edit.at(2).truthy() {
                        // insert the new chunk before this chunk, moving the header to the new chunk
                        let at_ = this_mdat.len() - 1;
                        this_mdat.insert(at_, arr(vec![edit.at(0), edit.at(1), md.at(2), n(0), edit.at(2), edit.at(3)]));
                        md.set_at(2, s("")); // (header was moved to new chunk)
                        // initialize ChunkOffset pointer if necessary
                        if edit.at(3).truthy() {
                            let mut nn = 0;
                            for co in off.items() {
                                if !(!co.at(4).is_undef() && pl::num_eq(&co.at(4), &edit.at(3))) {
                                    continue;
                                }
                                nn += 1;
                                if pl::str_eq(&co.at(0), "stco_iloc") {
                                    set32u_at(it, md.at(0), &outfile, co.at(1))?;
                                } else {
                                    set64u_at(it, md.at(0), &outfile, co.at(1))?;
                                }
                            }
                            if nn != 1 {
                                error(it, et, s("Internal error updating chunk offsets"))?;
                            }
                        }
                    }
                    md.set_at(0, edit.at(1)); // remove old data
                }
            }
        }
        if changed {
            new_mdat.append(&mut this_mdat);
            mdat = new_mdat;
            inc_changed(et);
        } else {
            error(it, et, s("Internal error modifying mdat"))?;
        }
    }

    // determine our new mdat positions
    // (0=old pos, 1=old end, 2=mdat header, 3=new pos, 4=new data ref if changed, 5=new item ID)
    let mut pos = dlen(&outfile);
    for md in &mdat {
        pos = pl::add(&pos, &len_of(&md.at(2)));
        md.set_at(3, pos.clone());
        let d = if md.at(4).truthy() { dlen(&md.at(4)) } else { pl::sub(&md.at(1), &md.at(0)) };
        pos = pl::add(&pos, &d);
    }

    // fix up offsets for new mdat position(s) (and uuid positions in CR3 images)
    let ctbo_id = lex_h(it, "ctboID");
    for co in off.items() {
        let (mut type_, mut ptr, mut len, mut base, id) = (co.at(0), co.at(1), co.at(2), co.at(3), co.at(4));
        if !base.truthy() {
            base = n(0);
        }
        let Some(tc) = re(r"^(stco|co64)_?(.*)$", "").m(&type_) else {
            if pl::str_eq(&type_, "uuid") {
                continue;
            }
            if !pl::str_eq(&type_, "CTBO") {
                error(it, et, s("Internal error fixing offsets"))?;
                break;
            }
            // update 'CTBO' item offsets/sizes in Canon CR3 images
            if !pl::gt(&co.at(2), &n(12)) {
                error(it, et, s("Invalid CTBO atom"))?;
                break;
            }
            if mdat.is_empty() {
                error(it, et, s("Missing CR3 image data"))?;
                break;
            }
            let nn = get32u(it, &outfile, pl::add(&co.at(1), &n(8)))?;
            if pl::lt(&co.at(2), &pl::add(&pl::mul(&nn, &n(20)), &n(12))) {
                error(it, et, s("Truncated CTBO atom"))?;
                break;
            }
            let ctbo_off = Value::new_hash();
            // determine uuid types, and build an offset lookup based on CTBO ID number
            for e in off.items() {
                if !(pl::str_eq(&e.at(0), "uuid") && pl::ge(&e.at(2), &n(24))) {
                    continue; // (ignore undersized and deleted uuid boxes)
                }
                let mut p = e.at(1);
                if pl::gt(&pl::add(&p, &n(24)), &dlen(&outfile)) {
                    continue; // (will happen for WriteLast uuid tags)
                }
                let siz = get32u(it, &outfile, p.clone())?; // get size of uuid atom
                if pl::num_eq(&siz, &n(1)) {
                    // check for extended (8-byte) size
                    if !pl::ge(&e.at(2), &n(32)) {
                        continue;
                    }
                    p = pl::add(&p, &n(8));
                }
                // get CTBO entry ID based on 16-byte UUID identifier
                let uid = pl::substr(&outfile.deref(), pl::add(&p, &n(8)).to_i64(), Some(16));
                let cid = ctbo_id.get(uid.to_bytes());
                if !cid.is_undef() {
                    ctbo_off.set(cid.to_bytes(), e.clone());
                }
            }
            // calculate new offset for the first mdat (size of -1 indicates it didn't change)
            ctbo_off.set("3", arr(vec![s("mdat"), pl::sub(&mdat[0].at(3), &len_of(&mdat[0].at(2))), n(-1)]));
            let mut i = n(0);
            while pl::lt(&i, &nn) {
                let p = pl::add(&pl::add(&co.at(1), &n(12)), &pl::mul(&i, &n(20)));
                i = pl::add(&i, &n(1));
                let cid = get32u(it, &outfile, p.clone())?;
                // ignore if size is zero unless we can add this entry
                // (note: can't yet add/delete PreviewImage, but leave this possibility open)
                if !(get64u(it, &outfile, pl::add(&p, &n(12)))?.truthy() || pl::num_eq(&cid, &n(1)) || pl::num_eq(&cid, &n(2))) {
                    continue;
                }
                if ctbo_off.get(cid.to_bytes()).is_undef() {
                    if !(pl::num_eq(&cid, &n(1)) || pl::num_eq(&cid, &n(2))) {
                        error(it, et, cat!("Can't handle CR3 CTBO ID number ", cid))?;
                        break;
                    }
                    // XMP or PreviewImage was deleted -- set offset and size to zero
                    ctbo_off.set(cid.to_bytes(), arr(vec![s("uuid"), n(0), n(0)]));
                }
                // update the new offset and size of this entry
                let e = ctbo_off.get(cid.to_bytes());
                set64u_at(it, e.at(1), &outfile, pl::add(&p, &n(4)))?;
                if !pl::lt(&e.at(2), &n(0)) {
                    set64u_at(it, e.at(2), &outfile, pl::add(&p, &n(12)))?;
                }
            }
            continue;
        };
        let (t1, t2) = (cap(&tc, 1), cap(&tc, 2));
        let siz = n(if pl::str_eq(&t1, "co64") { 8 } else { 4 });
        let (nn, tag);
        if t2.truthy() {
            // is this an offset in an iloc or 'gps ' atom?
            nn = n(1);
            type_ = t1;
            tag = t2;
        } else {
            // this is an stco or co84 atom
            if pl::lt(&len, &n(8)) {
                continue;
            }
            nn = get32u(it, &outfile, pl::add(&ptr, &n(4)))?; // get number of entries in table
            ptr = pl::add(&ptr, &n(8));
            len = pl::sub(&len, &n(8));
            tag = t1;
        }
        let end = pl::add(&ptr, &pl::mul(&nn, &siz));
        if pl::gt(&end, &pl::add(&ptr, &len)) {
            error(it, et, cat!("Invalid ", tag, " table"))?;
            return Ok(rtn_val);
        }
        while pl::lt(&ptr, &end) {
            let mut ok = false;
            let mut val = if pl::str_eq(&type_, "co64") { get64u(it, &outfile, ptr.clone())? } else { get32u(it, &outfile, ptr.clone())? };
            let mut i = 0;
            while i < mdat.len() {
                let md = mdat[i].clone();
                let pos = pl::add(&val, &base);
                if !md.at(5).is_undef() {
                    // is this chunk associated with an item we edited?
                    // set offset only for the corresponding new chunk
                    if !(!id.is_undef() && pl::num_eq(&id, &md.at(5))) {
                        // could have pointed to empty chunk before inserted chunk
                        if !(pl::num_eq(&pos, &md.at(0)) && !pl::num_eq(&md.at(0), &md.at(1))) {
                            i += 1;
                            continue;
                        }
                    }
                } else {
                    // (have seen $pos == $$mdat[1], which is a real PITA)
                    if !(pl::ge(&pos, &md.at(0)) && (pl::le(&pos, &md.at(1)) || !md.at(1).truthy())) {
                        i += 1;
                        continue;
                    }
                    // step to next chunk if contiguous and at the end of this one
                    if pl::num_eq(&pos, &md.at(1)) && i + 1 < mdat.len() && pl::num_eq(&pos, &mdat[i + 1].at(0)) {
                        i += 1;
                        continue;
                    }
                }
                val = pl::add(&val, &pl::sub(&md.at(3), &md.at(0)));
                if pl::lt(&val, &n(0)) {
                    error(it, et, cat!("Error fixing up ", tag, " offset"))?;
                    return Ok(rtn_val);
                }
                if pl::str_eq(&type_, "co64") {
                    set64u_at(it, val.clone(), &outfile, ptr.clone())?;
                } else if pl::le(&val, &Value::Int(0xffffffff)) {
                    set32u_at(it, val.clone(), &outfile, ptr.clone())?;
                } else {
                    error(it, et, cat!("Can't yet promote ", tag, " offset to 64 bits"))?;
                    return Ok(rtn_val);
                }
                ok = true;
                break;
            }
            if !ok {
                error(it, et, cat!("Chunk offset in ", tag, " atom is outside media data"))?;
                return Ok(rtn_val);
            }
            ptr = pl::add(&ptr, &siz);
        }
    }

    // switch back to actual output file
    let outfile_real = dir_info.get("OutFile");

    // write the metadata
    if !write(it, &outfile_real, &[outfile.deref()])? {
        rtn_val = n(0);
    }

    // write the media data
    for md in &mdat {
        if !write(it, &outfile_real, &[md.at(2)])? {
            rtn_val = n(0); // write mdat header
        }
        if md.at(4).truthy() {
            if !write(it, &outfile_real, &[md.at(4).deref()])? {
                rtn_val = n(0);
            }
        } else {
            if !raf_seek(it, &raf, md.at(0), 0)? {
                error(it, et, s("Seek error"))?;
                break;
            }
            if md.at(1).truthy() {
                let result = etc(it, "CopyBlock", vec![raf.clone(), outfile_real.clone(), pl::sub(&md.at(1), &md.at(0))])?;
                if result.is_undef() {
                    rtn_val = n(0);
                    break;
                }
                if !result.truthy() {
                    error(it, et, s("Truncated mdat atom"))?;
                    break;
                }
            } else {
                // mdat continues to end of file
                while raf_read(it, &raf, &mut buf2, n(65536))?.truthy() {
                    if !write(it, &outfile_real, &[buf2.clone()])? {
                        rtn_val = n(0);
                        break;
                    }
                }
            }
        }
    }

    // write the stuff that must come last
    let mut last_keys = write_last.keys();
    last_keys.sort();
    for k in last_keys {
        if !write(it, &outfile_real, &[write_last.get(&k[..])])? {
            rtn_val = n(0);
        }
    }

    // copy trailers if necessary
    while rtn_val.truthy() && trailer.truthy() {
        // are we deleting the trailers?
        let et_trailer = pl::pkg_hash(it, "Image::ExifTool::Extra").get("Trailer");
        let nv_trail = m(it, et, "GetNewValueHash", vec![et_trailer])?;
        let dg = hh(et, "DEL_GROUP");
        if dg.get("Trailer").truthy() || dg.get(trailer.at(0).to_bytes()).truthy() || (nv_trail.truthy() && !(nv_trail.get("Value").truthy() && nv_trail.get("Value").at(0).truthy())) {
            m(it, et, "Warn", vec![cat!("Deleted ", trailer.at(0), " trailer"), n(1)])?;
            inc_changed(et);
            trailer = trailer.at(3);
            continue;
        }
        if !raf_seek(it, &raf, trailer.at(1), 0)? {
            rtn_val = n(0);
            break;
        }
        if pl::str_eq(&trailer.at(0), "MIE") {
            crate::runtime::require(it, "Image::ExifTool::MIE")?;
            let d_info = Value::new_hash();
            d_info.set("RAF", raf.clone());
            d_info.set("OutFile", outfile_real.clone());
            let result = pl::call(it, "Image::ExifTool::MIE::ProcessMIE", vec![et.clone(), d_info])?;
            if !pl::gt(&result, &n(0)) {
                error(it, et, s("Error writing MIE trailer"))?;
                rtn_val = n(0);
                break;
            }
        } else {
            let msg = pl::sprintf("Copying %s trailer from offset 0x%x (%d bytes)", &[trailer.at(0), trailer.at(1), trailer.at(2)]);
            m(it, et, "Warn", vec![msg, n(1)])?;
            let mut len = trailer.at(2);
            while len.truthy() {
                let nn = if pl::gt(&len, &n(65536)) { n(65536) } else { len.clone() };
                if !(pl::num_eq(&raf_read(it, &raf, &mut buf2, nn.clone())?, &nn) && write(it, &outfile_real, &[buf2.clone()])?) {
                    rtn_val = n(0);
                    break;
                }
                len = pl::sub(&len, &nn);
            }
            if !rtn_val.truthy() {
                error(it, et, cat!("Error copying ", trailer.at(0), " trailer"))?;
                break;
            }
        }
        trailer = trailer.at(3); // step to next trailer in linked list
    }
    Ok(rtn_val)
}

/// Write QuickTime-format MOV/MP4 file
/// Inputs: 0) ExifTool ref, 1) dirInfo ref
/// Returns: 1 on success, 0 if this wasn't a valid QuickTime file,
///          or -1 if a write error occurred
pub fn write_mov(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    if !et.truthy() {
        return ret(n(1)); // allow dummy access to autoload this package
    }
    let raf = dir_info.get("RAF");
    if !raf.truthy() {
        return ret(n(0));
    }
    let mut buff = Value::Undef;
    let ftype;

    // read the first atom header
    if !pl::num_eq(&raf_read(it, &raf, &mut buff, n(8))?, &n(8)) {
        return ret(n(0));
    }
    let u = pl::unpack("Na4", &buff)?;
    let (size, tag) = (arg(&u, 0), arg(&u, 1));
    if pl::lt(&size, &n(8)) && !pl::num_eq(&size, &n(1)) {
        return ret(n(0));
    }

    // validate the file format
    let tag_table_ptr = get_tag_table(it, s("Image::ExifTool::QuickTime::Main"))?;
    if !tag_table_ptr.get(tag.to_bytes()).truthy() {
        return ret(n(0));
    }

    // determine the file type (by default, assume MP4 if 'ftyp' exists
    // without 'qt  ' as a compatible brand, but HEIC is an exception)
    if pl::str_eq(&tag, "ftyp")
        && pl::ge(&size, &n(12))
        && pl::lt(&size, &n(100000))
        && pl::num_eq(&raf_read(it, &raf, &mut buff, pl::sub(&size, &n(8)))?, &pl::sub(&size, &n(8)))
        && !re(r"^(....)+(qt  )", "s").is_match(&buff)
    {
        if re(r"^crx ", "").is_match(&buff) {
            ftype = "CR3";
        } else if re(r"^(heic|mif1|msf1|heix|hevc|hevx|avif)", "").is_match(&buff) {
            ftype = "HEIC";
        } else {
            ftype = "MP4";
        }
    } else {
        ftype = "MOV";
    }
    m(it, &et, "SetFileType", vec![s(ftype)])?; // need to set "FileType" tag for a Condition
    let dir_map = lex_h(it, "dirMap").get(ftype);
    if ftype == "HEIC" {
        // EXIF is preferred in HEIC files
        m(it, &et, "InitWriteDirs", vec![dir_map.clone(), s("EXIF"), s("QuickTime")])?;
    } else {
        m(it, &et, "InitWriteDirs", vec![dir_map.clone(), s("XMP"), s("QuickTime")])?;
    }
    et.set("DirMap", dir_map); // need access to directory map when writing
    // track tags globally to avoid creating multiple tags in the case of duplicate directories
    et.set("DidTag", Value::new_hash());
    set_byte_order(it, "MM")?;
    raf_seek(it, &raf, n(0), 0)?;

    // write the file
    et.set("MediaType", s(""));
    dir_info.set("Parent", s(""));
    dir_info.set("DirName", s("MOV"));
    dir_info.set("ChunkOffset", arr(Vec::new())); // (just to be safe)
    let r = pl::local_topic(it, |it| write_quick_time_v(it, &et, &dir_info, &tag_table_ptr))?;
    ret(n(if r.truthy() { 1 } else { -1 }))
}

/// WriteQuickTime.pl's module-level code:
/// mark UserData tags that don't have ItemList counterparts as Preferred
/// - and set Preferred to 0 for any Avoid-ed tag
/// - also, for now, set Writable to 0 for any tag with a RawConv and no RawConvInv
fn init_write_quick_time(it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let item_list = match it.global(b'%', &format!("{PKG}::ItemList")) {
        Binding::Hash(h) => Value::HashRef(h),
        _ => return ret(Value::Undef),
    };
    let user_data = match it.global(b'%', &format!("{PKG}::UserData")) {
        Binding::Hash(h) => Value::HashRef(h),
        _ => return ret(Value::Undef),
    };
    let mut pref = std::collections::HashSet::new();
    for tag in pl::call_list(it, "Image::ExifTool::TagTableKeys", vec![item_list.clone()])? {
        let mut tag_info = item_list.get(tag.to_bytes());
        if !tag_info.is_hash() {
            if tag_info.is_ref() {
                continue;
            }
            let h = Value::new_hash();
            h.set("Name", tag_info);
            item_list.set(tag.to_bytes(), h.clone());
            tag_info = h;
        } else {
            if tag_info.get("RawConv").truthy() && !tag_info.get("RawConvInv").truthy() {
                tag_info.set("Writable", n(0));
            }
            if tag_info.get("Avoid").truthy() {
                tag_info.set("Preferred", n(0));
                continue;
            }
            let p = tag_info.get("Preferred");
            if !p.is_undef() && !p.truthy() {
                continue;
            }
        }
        pref.insert(tag_info.get("Name").to_bytes().to_vec());
    }
    for tag in pl::call_list(it, "Image::ExifTool::TagTableKeys", vec![user_data.clone()])? {
        let mut tag_info = user_data.get(tag.to_bytes());
        if !tag_info.is_hash() {
            if tag_info.is_ref() {
                continue;
            }
            let h = Value::new_hash();
            h.set("Name", tag_info);
            user_data.set(tag.to_bytes(), h.clone());
            tag_info = h;
        } else {
            if tag_info.get("RawConv").truthy() && !tag_info.get("RawConvInv").truthy() {
                tag_info.set("Writable", n(0));
            }
            if tag_info.get("Avoid").truthy() {
                tag_info.set("Preferred", n(0));
                continue;
            }
            if !tag_info.get("Preferred").is_undef() || pref.contains(&tag_info.get("Name").to_bytes().to_vec()) {
                continue;
            }
        }
        tag_info.set("Preferred", n(1));
    }
    ret(Value::Undef)
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::QuickTime::InitWriteQuickTime", init_write_quick_time);
    it.define_native("Image::ExifTool::QuickTime::PrintInvGPSCoordinates", print_inv_gps_coordinates);
    it.define_native("Image::ExifTool::QuickTime::ConvInvISO6709", conv_inv_iso6709);
    it.define_native("Image::ExifTool::QuickTime::Handle_iloc", handle_iloc);
    it.define_native("Image::ExifTool::QuickTime::GetLangInfo", get_lang_info);
    it.define_native("Image::ExifTool::QuickTime::CheckQTValue", check_qt_value);
    it.define_native("Image::ExifTool::QuickTime::FormatQTValue", format_qt_value);
    it.define_native("Image::ExifTool::QuickTime::SetVarInt", set_var_int);
    it.define_native("Image::ExifTool::QuickTime::WriteNextbase", write_nextbase);
    it.define_native("Image::ExifTool::QuickTime::WriteKeys", write_keys);
    it.define_native("Image::ExifTool::QuickTime::WriteItemInfo", write_item_info);
    it.define_native("Image::ExifTool::QuickTime::WriteQuickTime", write_quick_time);
    it.define_native("Image::ExifTool::QuickTime::WriteMOV", write_mov);
}
