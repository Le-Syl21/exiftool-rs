//! Image::ExifTool::Nikon (Image/ExifTool/Nikon.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! The tag tables are data; these are the module's subs: the AF point and
//! Picture Control conversions, the lens ID fallback, the decryption of the
//! encrypted maker note blocks (bit-exact with the Perl), and the Nikon
//! directory/trailer processors.

use std::cmp::Ordering;

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, re, ret};

const MODULE: &str = "Image::ExifTool::Nikon";

fn n(i: i64) -> Value {
    Value::Int(i)
}

fn s(x: &str) -> Value {
    pl::s(x)
}

// ---- local helpers ----

/// `$x || 0`
fn or0(v: Value) -> Value {
    if v.truthy() {
        v
    } else {
        n(0)
    }
}

fn get16u(it: &mut Interp, data_pt: &Value, pos: Value) -> ER<Value> {
    pl::call(it, "Image::ExifTool::Get16u", vec![data_pt.clone(), pos])
}

fn get32u(it: &mut Interp, data_pt: &Value, pos: Value) -> ER<Value> {
    pl::call(it, "Image::ExifTool::Get32u", vec![data_pt.clone(), pos])
}

fn set_byte_order(it: &mut Interp, order: Value) -> ER<Value> {
    pl::call(it, "Image::ExifTool::SetByteOrder", vec![order])
}

fn options(it: &mut Interp, et: &Value, name: &str) -> ER<Value> {
    pl::method(it, et, "Options", vec![s(name)])
}

/// `$raf->Read($buff, $len)`: the number of bytes read, `buff` updated.
fn raf_read(it: &mut Interp, raf: &Value, buff: &mut Value, len: Value) -> ER<Value> {
    let mut args = [buff.clone(), len];
    let r = pl::method_mut(it, raf, "Read", &mut args, Want::Scalar)?;
    *buff = args[0].clone();
    Ok(r.last().cloned().unwrap_or_default())
}

/// `$raf->Read($buff, $len) == $len`
fn raf_read_ok(it: &mut Interp, raf: &Value, buff: &mut Value, len: &Value) -> ER<bool> {
    let r = raf_read(it, raf, buff, len.clone())?;
    Ok(pl::num_eq(&r, len))
}

/// `$raf->Seek($pos, $whence)`
fn raf_seek(it: &mut Interp, raf: &Value, pos: Value, whence: i64) -> ER<bool> {
    Ok(pl::method(it, raf, "Seek", vec![pos, n(whence)])?.truthy())
}

/// `substr(STR, OFF, LEN)` as the builtin (warns outside the string).
fn substr_w(it: &mut Interp, st: &Value, off: &Value, len: Option<&Value>) -> ER<Value> {
    crate::modules::random_access::substr(it, st, off, len)
}

/// `chr($c)`: a byte below 256, otherwise perl's wide character.
fn chr(it: &mut Interp, c: &Value) -> ER<Value> {
    let i = c.to_i64();
    if (0..256).contains(&i) {
        return Ok(Value::bytes([i as u8]));
    }
    let (v, _) = pl::eval_site(it, &s("chr($c)"), MODULE, &[("c", c.clone())], Want::Scalar);
    Ok(v)
}

/// `$a cmp $b`
fn cmp(a: &Value, b: &Value) -> Ordering {
    ops::str_cmp(a, b)
}

/// `$a <=> $b` (NaN compares equal here, as perl's sort treats it).
fn num_cmp(a: &Value, b: &Value) -> Ordering {
    ops::num_cmp(a, b).unwrap_or(Ordering::Equal)
}

//------------------------------------------------------------------------------
/// Process Nikon AVI tags (D5000 videos)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_nikon_avi(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let mut pos = or0(dir_info.get("DirStart"));
    let dir_end = pl::add(&pos, &dir_info.get("DirLen"));
    pl::method(it, &et, "VerboseDir", vec![dir_info.clone(), Value::Undef, dir_info.get("DirLen")])?;
    set_byte_order(it, s("II"))?;
    while pl::le(&pl::add(&pos, &n(4)), &dir_end) {
        let tag = get16u(it, &data_pt, pos.clone())?;
        let size = get16u(it, &data_pt, pl::add(&pos, &n(2)))?;
        pos = pl::add(&pos, &n(4));
        if pl::gt(&pl::add(&pos, &size), &dir_end) {
            break;
        }
        pl::method(
            it,
            &et,
            "HandleTag",
            vec![tag_table_ptr.clone(), tag, Value::Undef, s("DataPt"), data_pt.clone(), s("Start"), pos.clone(), s("Size"), size.clone()],
        )?;
        pos = pl::add(&pos, &size);
    }
    ret(n(1))
}

//------------------------------------------------------------------------------
/// Print conversion for Nikon AF points
/// Inputs: 0) value to convert (as a string of hex bytes),
///         1) lookup for AF point bit number (starting at 1), or array ref
pub fn print_af_points(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (mut val, af_points) = (arg(&a, 0), arg(&a, 1));
    let mut points: Vec<Value> = Vec::new();
    pl::tr(&mut val, " ", "", "d"); // remove spaces from hex string
    let dat = pl::unpack("C*", &pl::pack("H*", &[val])?)?; // convert to array of bytes
    // loop through all bytes to find active AF points
    let is_hash = pl::ref_of(&af_points) == "HASH";
    let mut i = 0;
    while i <= dat.len() {
        let d = arg(&dat, i);
        if d.truthy() {
            for j in 0..8 {
                if !ops::bit_and(&d, &n(1 << j)).truthy() {
                    continue;
                }
                let point = if is_hash { af_points.get((i as i64 * 8 + j + 1).to_string()) } else { af_points.at(i as i64 * 8 + j) };
                if !point.is_undef() {
                    points.push(point);
                }
            }
        }
        i += 1;
    }
    if points.is_empty() {
        return ret(s("(none)"));
    }
    // sort the points and return as comma-separated string
    points.sort_by(|a, b| {
        let (la, lb) = (pl::length(a), pl::length(b));
        if pl::num_eq(&la, &lb) {
            return cmp(a, b);
        }
        let pad = |x: &Value| pl::concat(&pl::concat(&pl::substr(x, 0, Some(1)), &s("0")), &pl::substr(x, 1, Some(1)));
        if pl::num_eq(&la, &n(2)) {
            return cmp(&pad(a), b);
        }
        cmp(a, &pad(b))
    });
    ret(pl::join(",", &points))
}

//------------------------------------------------------------------------------
/// Inverse print conversion for AF points
/// Inputs: 0) AF point string, 1) AF point hash or array ref
/// Returns: AF point data as a string of hex bytes
pub fn print_af_points_inv(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (val, af_points) = (arg(&a, 0), arg(&a, 1));
    let points = re(r"[A-Za-z]\d+", "").all(&val);
    let is_hash = pl::ref_of(&af_points) == "HASH";
    let num = if is_hash { af_points.keys().len() } else { af_points.len() };
    let size = (num + 7) / 8;
    let mut dat: Vec<Value> = vec![n(0); size];
    if !points.is_empty() {
        let bit_num_h = Value::new_hash();
        if is_hash {
            for k in af_points.keys() {
                // build reverse lookup
                bit_num_h.set(af_points.get(&k).to_bytes(), Value::Str(k));
            }
        } else {
            for (i, p) in af_points.items().iter().enumerate() {
                bit_num_h.set(p.to_bytes(), n(i as i64 + 1));
            }
        }
        for point in &points {
            let bit_num = bit_num_h.get(pl::uc(point).to_bytes());
            if !bit_num.truthy() {
                continue;
            }
            let bm1 = pl::sub(&bit_num, &n(1));
            let byte = pl::int(&pl::div(&bm1, &n(8))?).to_i64();
            if byte < 0 {
                continue;
            }
            let byte = byte as usize;
            if byte >= dat.len() {
                dat.resize(byte + 1, Value::Undef);
            }
            dat[byte] = ops::bit_or(&dat[byte], &ops::shift_left(&n(1), &pl::modulo(&bm1, &n(8))?));
        }
    }
    let packed = pl::pack("C*", &dat)?;
    let hex = pl::unpack(&"H2".repeat(size), &packed)?;
    ret(pl::join(" ", &hex))
}

//------------------------------------------------------------------------------
/// Get AF point name for grid-type AF
/// Inputs: 0) AF point number, 1) number of columns, 2) true for inverse conversion
/// Returns: AF point name, or undef
pub fn get_af_point_grid(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(get_af_point_grid_rs(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2))?)
}

fn get_af_point_grid_rs(it: &mut Interp, val: &Value, ncol: &Value, inv: &Value) -> ER<Value> {
    if inv.truthy() {
        let Some(m) = re(r"^([A-J])(\d+)$", "i").m(val) else { return Ok(Value::Undef) };
        let o = n(pl::uc(&cap(&m, 1)).to_bytes()[0] as i64 - 65);
        Ok(pl::sub(&pl::add(&pl::mul(&o, ncol), &cap(&m, 2)), &n(1)))
    } else {
        let row = ops::bit_and(&pl::int(&pl::div(&pl::add(val, &Value::Num(0.5)), ncol)?), &n(0xff));
        let col = pl::add(&pl::sub(val, &pl::mul(ncol, &row)), &n(1));
        let c = chr(it, &pl::add(&n(65), &row))?;
        Ok(pl::concat(&c, &col))
    }
}

//------------------------------------------------------------------------------
/// Print conversion for grid-type AF points
/// Inputs: 0) value to convert (as a string of hex bytes),
///         1) number of columns in grid
pub fn print_af_points_grid(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (mut val, ncol) = (arg(&a, 0), arg(&a, 1));
    let mut points: Vec<Value> = Vec::new();
    pl::tr(&mut val, " ", "", "d"); // remove spaces from hex string
    let dat = pl::unpack("C*", &pl::pack("H*", &[val])?)?; // convert to array of bytes
    // loop through all bytes to find active AF points
    for (i, d) in dat.iter().enumerate() {
        if !d.truthy() {
            continue;
        }
        for j in 0..8 {
            if !ops::bit_and(d, &n(1 << j)).truthy() {
                continue;
            }
            let point = get_af_point_grid_rs(it, &n(i as i64 * 8 + j), &ncol, &Value::Undef)?;
            if !point.is_undef() {
                points.push(point);
            }
        }
    }
    if points.is_empty() {
        return ret(s("(none)"));
    }
    ret(pl::join(",", &points)) // return as comma-separated string
}

//------------------------------------------------------------------------------
/// Inverse print conversion for AF points
/// Inputs: 0) AF point string, 1) number of columns, 2) size of data
/// Returns: AF point data as a string of hex bytes
pub fn print_af_points_grid_inv(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (val, ncol, size) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let points = re(r"[A-Za-z]\d+", "").all(&val);
    let size_n = size.to_i64().max(0) as usize;
    let mut dat: Vec<Value> = vec![n(0); size_n];
    for p in &points {
        let nn = get_af_point_grid_rs(it, p, &ncol, &n(1))?;
        if nn.is_undef() {
            continue;
        }
        let byte = pl::int(&pl::div(&nn, &n(8))?);
        if pl::gt(&byte, &size) {
            continue;
        }
        let b = byte.to_i64();
        if b < 0 {
            continue;
        }
        let b = b as usize;
        if b >= dat.len() {
            dat.resize(b + 1, Value::Undef);
        }
        dat[b] = ops::bit_or(&dat[b], &ops::shift_left(&n(1), &pl::sub(&nn, &pl::mul(&byte, &n(8)))));
    }
    let packed = pl::pack("C*", &dat)?;
    let hex = pl::unpack(&"H2".repeat(size_n), &packed)?;
    ret(pl::join(" ", &hex))
}

//------------------------------------------------------------------------------
/// Print conversion for relative Left/Right AF points (ref 28)
/// Inputs: 0) column, 1) number of columns
/// Returns: AF point data as a string (e.g. '2L of Center' or 'C' or '3R of Center')
pub fn print_af_points_left_right(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (col, ncol) = (arg(&a, 0), arg(&a, 1));
    let center = pl::div(&pl::add(&ncol, &n(1)), &n(2))?;
    if pl::num_eq(&col, &n(0)) {
        return ret(s("n/a")); //out of focus
    }
    if pl::num_eq(&col, &center) {
        return ret(s("C"));
    }
    if pl::lt(&col, &center) {
        return ret(pl::concat(&pl::sprintf("%d", &[pl::sub(&center, &col)]), &s("L of Center")));
    }
    if pl::gt(&col, &center) {
        return ret(pl::concat(&pl::sprintf("%d", &[pl::sub(&col, &center)]), &s("R of Center")));
    }
    ret(s(""))
}

//------------------------------------------------------------------------------
/// Print conversion for relative Up/Down AF points (ref 28)
/// Inputs: 0) row, 1) number of rows
/// Returns: AF point data as a string (e.g. '2U from Center' or 'C' or '3D from Center')
pub fn print_af_points_up_down(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (row, nrow) = (arg(&a, 0), arg(&a, 1));
    let center = pl::div(&pl::add(&nrow, &n(1)), &n(2))?;
    if pl::num_eq(&row, &n(0)) {
        return ret(s("n/a")); //out of focus
    }
    if pl::num_eq(&row, &center) {
        return ret(s("C"));
    }
    if pl::lt(&row, &center) {
        return ret(pl::concat(&pl::sprintf("%d", &[pl::sub(&center, &row)]), &s("U from Center")));
    }
    if pl::gt(&row, &center) {
        return ret(pl::concat(&pl::sprintf("%d", &[pl::sub(&row, &center)]), &s("D from Center")));
    }
    ret(s(""))
}

//------------------------------------------------------------------------------
/// Print PictureControl value
/// Inputs: 0) value (with 0x80 subtracted),
///         1) 'Normal' (0 value) string (default 'Normal')
///         2) format string for numbers (default '%+d'), 3) v2 divisor
/// Returns: PrintConv value
pub fn print_pc(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (val, norm, fmt, div) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    if pl::num_eq(&val, &n(0)) {
        return ret(if norm.truthy() { norm } else { s("Normal") });
    }
    if pl::num_eq(&val, &n(0x7f)) {
        return ret(s("n/a"));
    }
    if pl::num_eq(&val, &n(-128)) {
        return ret(s("Auto"));
    }
    // -127 = custom curve created in Camera Control Pro (show as "User" by D3) - ref 28
    if pl::num_eq(&val, &n(-127)) {
        return ret(s("User")); //28
    }
    let fmt = if fmt.truthy() { fmt } else { s("%+d") };
    let div = if div.truthy() { div } else { n(1) };
    ret(pl::sprintf_v(&fmt, &[pl::div(&val, &div)?]))
}

//------------------------------------------------------------------------------
/// Inverse of PrintPC
/// Inputs: 0) PrintConv value (after subracting 0x80 from raw value), 1) v2 divisor
/// Returns: unconverted value
/// Notes: raw values: 0=Auto, 1=User, 0xff=n/a, ... 0x7f=-1, 0x80=0, 0x81=1, ...
pub fn print_pc_inv(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (val, div) = (arg(&a, 0), arg(&a, 1));
    if re(r"^[-+]?\d+(\.\d+)?$", "").is_match(&val) {
        return ret(pl::mul(&val, &if div.truthy() { div } else { n(1) }));
    }
    if re(r"n\/a", "i").is_match(&val) {
        return ret(n(0x7f));
    }
    if re(r"auto", "i").is_match(&val) {
        return ret(n(-128));
    }
    if re(r"user", "i").is_match(&val) {
        return ret(n(-127)); //28
    }
    ret(n(0))
}

//------------------------------------------------------------------------------
/// Convert unknown LensID values
/// Inputs: 0) value, 1) flag for inverse conversion, 2) PrintConv hash ref
pub fn lens_id_conv(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (val, inv, conv) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if inv.truthy() {
        return pl::ret_undef();
    }
    let key = |i: i64| pl::concat(&pl::concat(&val, &s(".")), &n(i)).to_bytes();
    // multiple lenses with the same LensID are distinguished by decimal values
    if conv.get(key(1)).truthy() {
        let (mut vals, mut user): (Vec<Value>, Vec<Value>) = (Vec::new(), Vec::new());
        let user_lens = pl::pkg_hash(it, "Image::ExifTool::userLens");
        let mut i = 1;
        loop {
            let lens = conv.get(key(i));
            if !lens.truthy() {
                break;
            }
            if user_lens.get(lens.to_bytes()).truthy() {
                user.push(lens);
            } else {
                vals.push(lens);
            }
            i += 1;
        }
        if !user.is_empty() {
            return ret(pl::join(" or ", &user));
        }
        return ret(pl::join(" or ", &vals));
    }
    // Sigma has been changing the LensIDNumber on some new lenses
    // and with some Sigma lenses the LensFStops changes! (argh!)
    // Also, older cameras my not set bits 4-7 of LensType
    let mut pat = val.clone();
    re(r"^\w+ \w+", "").subst(&mut pat, false, |_| b".. ..".to_vec()); // ignore LensIDNumber and LensFStops
    re(r"\w(\w)$", "").subst(&mut pat, false, |c| [b".", &cap(c, 1).to_bytes()[..]].concat()); // ignore bits 4-7 of LensType
    let anchored = |p: &Value| -> ER<pl::Re> {
        let mut b = b"^".to_vec();
        b.extend_from_slice(&p.to_bytes());
        b.push(b'$');
        pl::re_dyn(&b, "")
    };
    let rx = anchored(&pat)?;
    let mut ids: Vec<Value> = conv.keys().into_iter().map(Value::Str).filter(|k| rx.is_match(k)).collect();
    ids.sort_by(cmp);
    if !ids.is_empty() {
        // first try different LensFStops (2nd value)
        pat = val.clone();
        re(r" \w+", "").subst(&mut pat, false, |_| b" ..".to_vec());
        let rx = anchored(&pat)?;
        let good: Vec<&Value> = ids.iter().filter(|k| rx.is_match(k)).collect();
        if let Some(g) = good.first() {
            return ret(conv.get(g.to_bytes()));
        }
        // then try different LensIDNumber (1st value)
        pat = val.clone();
        re(r"^\w+", "").subst(&mut pat, false, |_| b"..".to_vec());
        let rx = anchored(&pat)?;
        let good: Vec<&Value> = ids.iter().filter(|k| rx.is_match(k)).collect();
        if let Some(g) = good.first() {
            return ret(unknown_lens(&val, &conv.get(g.to_bytes())));
        }
        // older cameras may not set bits 4-7 of LensType
        pat = val.clone();
        re(r"\w(\w)$", "").subst(&mut pat, false, |c| [b".", &cap(c, 1).to_bytes()[..]].concat());
        let rx = anchored(&pat)?;
        let good: Vec<&Value> = ids.iter().filter(|k| rx.is_match(k)).collect();
        if let Some(g) = good.first() {
            return ret(unknown_lens(&val, &conv.get(g.to_bytes())));
        }
    }
    pl::ret_undef()
}

/// `"Unknown ($val) $$conv{$good[0]} ?"`
fn unknown_lens(val: &Value, lens: &Value) -> Value {
    let mut v = b"Unknown (".to_vec();
    v.extend_from_slice(&val.to_bytes());
    v.extend_from_slice(b") ");
    v.extend_from_slice(&lens.to_bytes());
    v.extend_from_slice(b" ?");
    Value::bytes(v)
}

//------------------------------------------------------------------------------
/// Clean up formatting of string values
/// Inputs: 0) string value, 1) ExifTool ref
/// Returns: formatted string value
/// - removes trailing spaces and changes case to something more sensible
pub fn format_string(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (mut st, et) = (arg(&a, 0), arg(&a, 1));
    // limit string length (can be very long for some unknown tags)
    let lim = options(it, &et, "LimitLongValues")?;
    if pl::gt(&pl::length(&st), &lim) && pl::ge(&lim, &n(5)) {
        st = pl::concat(&pl::substr(&st, 0, Some(pl::sub(&lim, &n(5)).to_i64())), &s("[...]"));
    } else {
        re(r"\s+$", "").subst(&mut st, false, |_| Vec::new()); // remove trailing white space
        // Don't change case of non-words (no vowels)
        if re(r"[AEIOUY]", "").is_match(&st) {
            // change all letters but the first to lower case,
            // but only in words containing a vowel
            let lower = |c: &pl::Caps| [&cap(c, 1).to_bytes()[..], &cap(c, 2).to_bytes().to_ascii_lowercase()[..]].concat();
            if re(r"\b([AEIOUY])([A-Z]+)", "").subst(&mut st, true, lower) > 0 {
                re(r"\bAf\b", "").subst(&mut st, false, |_| b"AF".to_vec()); // patch for "AF"
                // patch for a number of models that write improper string
                // terminator for ImageStabilization (VR-OFF, VR-ON)
                re(r"  +.$", "s").subst(&mut st, false, |_| Vec::new());
            }
            if re(r"\b([A-Z])([A-Z]*[AEIOUY][A-Z]*)", "").subst(&mut st, true, lower) > 0 {
                re(r"\bRaw\b", "").subst(&mut st, false, |_| b"RAW".to_vec()); // patch for "RAW"
            }
        }
    }
    ret(st)
}

//------------------------------------------------------------------------------
// decryption parameters: the file lexicals ($ci0, $cj0, $ck0, $decryptStart)
// and the decoding tables from ref 4 (@xlat), shared with the Perl

/// Decrypt Nikon data block (ref 4)
/// Inputs: 0) reference to data block, 1) optional start offset (default 0)
///         2) optional number of bytes to decode (default to the end of the data)
///         3) optional serial number key (undef to continue previous decryption)
///         4) optional shutter count key
/// Returns: data block with specified data decrypted
/// Notes: The first time this is called for a given encrypted data block the serial/count
///        keys must be defined, and $start must be the offset for initialization of the
///        decryption parameters (ie. the beginning of the encrypted data, which isn't
///        necessarily inside the data block if $len is zero).  Subsequent calls for
///        the same data block do not specify the serial/count keys, and may be used
///        to decrypt data at any start point within the full data block.
pub fn decrypt(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(decrypt_rs(it, &arg(&a, 0), arg(&a, 1), arg(&a, 2), &arg(&a, 3), &arg(&a, 4))?)
}

fn decrypt_rs(it: &mut Interp, data_pt: &Value, mut start: Value, mut len: Value, serial: &Value, count: &Value) -> ER<Value> {
    let ci0 = crate::runtime::lex_scalar(it, MODULE, "ci0");
    let cj0 = crate::runtime::lex_scalar(it, MODULE, "cj0");
    let ck0 = crate::runtime::lex_scalar(it, MODULE, "ck0");
    let decrypt_start = crate::runtime::lex_scalar(it, MODULE, "decryptStart");
    let (cj, ck);

    if !start.truthy() {
        start = n(0);
    }
    let data = data_pt.deref();
    let max_len = pl::sub(&pl::length(&data), &start);
    if len.is_undef() || pl::gt(&len, &max_len) {
        len = max_len;
    }
    if !serial.is_undef() && !count.is_undef() {
        // initialize decryption parameters
        let mut key = n(0);
        for i in 0..4 {
            key = ops::bit_xor(&key, &ops::bit_and(&ops::shift_right(count, &n(i * 8)), &n(0xff)));
        }
        let xlat = crate::runtime::lex_array(it, MODULE, "xlat");
        *ci0.borrow_mut() = xlat.at(0).at(ops::bit_and(serial, &n(0xff)).to_i64());
        *cj0.borrow_mut() = xlat.at(1).at(key.to_i64());
        *ck0.borrow_mut() = n(0x60);
        *decrypt_start.borrow_mut() = Value::Undef;
    }
    let (ci0, cj0, ck0) = (ci0.borrow().clone(), cj0.borrow().clone(), ck0.borrow().clone());
    let ds = decrypt_start.borrow().clone();
    if !ds.is_undef() {
        // initialize decryption parameters for this start position
        let nn = pl::sub(&start, &ds);
        let t = pl::add(&pl::mul(&nn, &ck0), &pl::div(&pl::mul(&nn, &pl::sub(&nn, &n(1))), &n(2))?);
        cj = ops::bit_and(&pl::add(&cj0, &pl::mul(&ci0, &t)), &n(0xff));
        ck = ops::bit_and(&pl::add(&ck0, &nn), &n(0xff));
    } else {
        *decrypt_start.borrow_mut() = start.clone();
        (cj, ck) = (cj0, ck0);
    }
    if pl::le(&len, &n(0)) {
        return Ok(data);
    }
    let (off, l) = (start.to_i64(), len.to_i64());
    let mut bytes = pl::substr(&data, off, Some(l)).to_bytes().to_vec();
    // (the parameters are bytes from here: $cj and $ck are masked with 0xff
    // and $ci0 comes from the table, so integer arithmetic is exact)
    let (ci0, mut cj, mut ck) = (ci0.to_i64(), cj.to_i64(), ck.to_i64());
    for ch in bytes.iter_mut() {
        cj = (cj + ci0 * ck) & 0xff;
        ck = (ck + 1) & 0xff;
        *ch ^= cj as u8;
    }
    let mut out = pl::substr(&data, 0, Some(off)).to_bytes().to_vec();
    out.extend_from_slice(&bytes);
    out.extend_from_slice(&pl::substr(&data, off + l, None).to_bytes());
    Ok(Value::bytes(out))
}

//------------------------------------------------------------------------------
/// Get serial number for use as a decryption key
/// Inputs: 0) ExifTool object ref, 1) serial number string
/// Returns: serial key integer or undef if no serial number provided
pub fn serial_key(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(serial_key_rs(&arg(&a, 0), &arg(&a, 1)))
}

fn serial_key_rs(et: &Value, serial: &Value) -> Value {
    // use serial number as key if integral
    if serial.is_undef() || re(r"^\d+$", "").is_match(serial) {
        return serial.clone();
    }
    if re(r"\bD50$", "").is_match(&et.get("Model")) {
        return n(0x22); // D50 (ref 8)
    }
    n(0x60) // D200 (ref 10), D40X (ref PH), etc
}

//------------------------------------------------------------------------------
/// Extract information from "NIKON APP" trailer (ref PH)
/// Inputs: 0) ExifTool ref, 1) Optional dirInfo ref for returning trailer info
/// Returns: true on success
pub fn process_nikon_app(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| process_nikon_app_body(it, arg(&a, 0), arg(&a, 1)))
}

fn process_nikon_app_body(it: &mut Interp, et: Value, dir_info: Value) -> ER<Vec<Value>> {
    let raf = et.get("RAF");
    let offset = if dir_info.truthy() { or0(dir_info.get("Offset")) } else { n(0) };
    let mut buff = Value::Undef;

    if !(raf_seek(it, &raf, pl::sub(&n(-20), &offset), 2)?
        && raf_read_ok(it, &raf, &mut buff, &n(20))?
        && pl::str_eq(&pl::substr(&buff, -16, None), "\0\0\0\0\0\0/NIKON APP"))
    {
        return ret(n(0)); // check magic number
    }
    let verbose = options(it, &et, "Verbose")?;
    let file_end = pl::method(it, &raf, "Tell", vec![])?;
    let trailer_len = pl::unpack1("N", &buff)?;
    if pl::gt(&trailer_len, &file_end) {
        pl::method(it, &et, "Warn", vec![s("Bad NikonApp trailer size")])?;
        return ret(n(0));
    }
    if dir_info.truthy() {
        dir_info.set("DirLen", trailer_len.clone());
        dir_info.set("DataPos", pl::sub(&file_end, &trailer_len));
        if dir_info.get("OutFile").truthy() {
            if et.get("DEL_GROUP").get("NikonApp").truthy() {
                let msg = pl::concat(&pl::concat(&s("  Deleting NikonApp trailer ("), &trailer_len), &s(" bytes)\n"));
                pl::method(it, &et, "VPrint", vec![n(0), msg])?;
                et.set("CHANGED", pl::add(&et.get("CHANGED"), &n(1)));
            // just copy the trailer when writing (read directly into output buffer)
            } else if pl::gt(&trailer_len, &file_end) || !raf_seek(it, &raf, dir_info.get("DataPos"), 0)? || {
                let out = dir_info.get("OutFile");
                let mut b = out.deref();
                let r = raf_read(it, &raf, &mut b, trailer_len.clone())?;
                pl::set_deref(&out, b)?;
                !pl::num_eq(&r, &trailer_len)
            } {
                return ret(n(0));
            }
            return ret(n(1));
        }
        if verbose.truthy() || et.get("HTML_DUMP").truthy() {
            pl::method(it, &et, "DumpTrailer", vec![dir_info.clone()])?;
        }
    }
    if !(pl::ge(&trailer_len, &n(0x40))
        && raf_seek(it, &raf, pl::sub(&file_end, &trailer_len), 0)?
        && raf_read_ok(it, &raf, &mut buff, &n(0x40))?
        && re(r"NIKON APP\0", "").is_match(&buff))
    {
        pl::method(it, &et, "Warn", vec![s("Error reading NikonApp trailer")])?;
        return ret(n(0));
    }
    et.set("SET_GROUP0", s("NikonApp"));
    loop {
        let r = raf_read(it, &raf, &mut buff, n(8))?;
        if !pl::num_eq(&r, &n(8)) {
            break;
        }
        let v = pl::unpack("N2", &buff)?;
        let (id, len) = (arg(&v, 0), arg(&v, 1));
        if ops::bit_and(&len, &n(0x80000000)).truthy() {
            pl::method(it, &et, "Warn", vec![s("Invalid NikonApp record length")])?;
            break;
        }
        if pl::num_eq(&id, &n(0)) && pl::num_eq(&len, &n(0)) {
            break;
        }
        if !raf_read_ok(it, &raf, &mut buff, &len)? {
            pl::method(it, &et, "Warn", vec![s("Truncated NikonApp record")])?;
            break;
        }
        if pl::num_eq(&id, &n(1)) {
            crate::runtime::require(it, "Image::ExifTool::XMP")?;
            let d = Value::new_hash();
            d.set("DataPt", pl::sref(buff.clone()));
            pl::call(it, "Image::ExifTool::XMP::ProcessXMP", vec![et.clone(), d])?;
        } else {
            // (haven't seen any other types of records)
            let msg = pl::concat(&s("Unknown NikonApp record "), &id);
            pl::method(it, &et, "Warn", vec![msg])?;
            break;
        }
    }
    et.delete("SET_GROUP0");
    ret(n(1))
}

//------------------------------------------------------------------------------
/// Read Nikon NCTG tags in MOV videos
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_nikon_mov(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let data_pos = dir_info.get("DataPos");
    let need_tags = Value::new_hash(); // needed for decryption
    need_tags.set("17867825", n(0)); // 0x110a431
    need_tags.set("33554599", Value::Undef); // 0x20000a7
    pl::method(it, &et, "VerboseDir", vec![dir_info.get("DirName"), n(0), dir_info.get("DirLen")])?;
    let format_name = pl::pkg_array(it, "Image::ExifTool::Exif::formatName");
    let format_size = pl::pkg_array(it, "Image::ExifTool::Exif::formatSize");
    // do two passes so we can pre-scan for necessary decryption keys
    for pass in 0..2 {
        let mut pos = dir_info.get("DirStart");
        let end = pl::add(&pos, &dir_info.get("DirLen"));
        while pl::lt(&pl::add(&pos, &n(8)), &end) {
            let tag = get32u(it, &data_pt, pos.clone())?;
            let fmt = get16u(it, &data_pt, pl::add(&pos, &n(4)))?; // (same format code as EXIF)
            let count = get16u(it, &data_pt, pl::add(&pos, &n(6)))?;
            pos = pl::add(&pos, &n(8));
            let fmt_str = format_name.at(fmt.to_i64());
            let dir_name = dir_info.get("DirName");
            if !fmt_str.truthy() {
                if pass != 0 {
                    let f = [&b"Unknown format ("[..], &fmt.to_bytes(), b") for ", &dir_name.to_bytes(), b" tag 0x%x"].concat();
                    let msg = pl::sprintf_v(&Value::bytes(f), &[tag.clone()]);
                    pl::method(it, &et, "Warn", vec![msg])?;
                }
                break;
            }
            let size = pl::mul(&count, &format_size.at(fmt.to_i64()));
            if pl::gt(&pl::add(&pos, &size), &end) {
                if pass != 0 {
                    let f = [&b"Truncated data for "[..], &dir_name.to_bytes(), b" tag 0x%x"].concat();
                    let msg = pl::sprintf_v(&Value::bytes(f), &[tag.clone()]);
                    pl::method(it, &et, "Warn", vec![msg])?;
                }
                break;
            }
            if pass != 0 {
                let rational = pl::sref(Value::Undef);
                let val = pl::call(it, "Image::ExifTool::ReadValue", vec![data_pt.clone(), pos.clone(), fmt_str.clone(), count.clone(), size.clone(), rational.clone()])?;
                let key = pl::method(
                    it,
                    &et,
                    "HandleTag",
                    vec![
                        tag_table_ptr.clone(),
                        tag.clone(),
                        val,
                        s("DataPt"),
                        data_pt.clone(),
                        s("DataPos"),
                        data_pos.clone(),
                        s("Format"),
                        fmt_str.clone(),
                        s("Start"),
                        pos.clone(),
                        s("Size"),
                        size.clone(),
                        s("Base"),
                        dir_info.get("Base"),
                    ],
                )?;
                if key.truthy() {
                    let rational = rational.deref();
                    if rational.truthy() {
                        et.get_or_new_hash("TAG_EXTRA").get_or_new_hash(key.to_bytes()).set("Rational", rational);
                    }
                    if et.get("OPTIONS").get("SaveBin").truthy() {
                        let bin = pl::substr(&data_pt.deref(), pos.to_i64(), Some(size.to_i64()));
                        et.get_or_new_hash("TAG_EXTRA").get_or_new_hash(key.to_bytes()).set("BinVal", bin);
                    }
                }
            } else if need_tags.exists(tag.to_bytes()) {
                let v = pl::call(it, "Image::ExifTool::ReadValue", vec![data_pt.clone(), pos.clone(), fmt_str.clone(), count.clone(), size.clone()])?;
                need_tags.set(tag.to_bytes(), v);
                et.set("NikonSerialKey", serial_key_rs(&et, &need_tags.get("17867825")));
                et.set("NikonCountKey", need_tags.get("33554599"));
            }
            pos = pl::add(&pos, &size); // is this padded to an even offset????
        }
    }
    ret(n(1))
}

//------------------------------------------------------------------------------
/// Get offset of end-of-data for a tag
/// Inputs: 0) tag table ref, 1) tag ID, 2) true to not calculate end for a SubDirectory
/// Returns: offset of tag value end, undef if it can't be determined
pub fn get_tag_end(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(get_tag_end_rs(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2))?)
}

fn get_tag_end_rs(it: &mut Interp, tag_table_ptr: &Value, tag_id: &Value, ignore_subdir: &Value) -> ER<Value> {
    let mut tag_info = tag_table_ptr.get(tag_id.to_bytes());
    if pl::ref_of(&tag_info) == "ARRAY" {
        tag_info = tag_info.at(0);
    }
    // (can't pre-determine position of offset-based subdirectories)
    if ignore_subdir.truthy() && tag_info.get("SubDirectory").truthy() {
        return Ok(Value::Undef);
    }
    let mut fmt = tag_info.get("Format");
    if !fmt.truthy() {
        fmt = tag_table_ptr.get("FORMAT");
    }
    if !fmt.truthy() {
        fmt = s("int8u");
    }
    let mut nm_cap = None;
    let nm = if re(r"\[(\d+)\]$", "").subst(&mut fmt, false, |c| {
        nm_cap = Some(cap(c, 1));
        Vec::new()
    }) > 0
    {
        nm_cap.unwrap_or_default()
    } else {
        n(1)
    };
    let sz = pl::call(it, "Image::ExifTool::FormatSize", vec![fmt])?;
    if !sz.truthy() {
        return Ok(Value::Undef);
    }
    Ok(pl::add(&pl::int(tag_id), &pl::mul(&sz, &nm)))
}

//------------------------------------------------------------------------------
/// Initialize SubDirectory KnownStart/KnownEnd limits of known tags (used in decryption)
/// Inputs: 0) tagInfo ref containing this SubDirectory, 2) tag table ref for encrypted subdir
/// Notes: KnownStart/KnownEnd are relative to the SubDirectory Start.  If KnownStart/KnownEnd
///        aren't set then the entire data is decrypted, so all of this effort is just for speed.
pub fn init_encrypted_subdir(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    init_encrypted_subdir_rs(it, &arg(&a, 0), &arg(&a, 1))?;
    pl::ret_nothing(Want::Scalar)
}

fn tag_table_keys(it: &mut Interp, table: &Value) -> ER<Vec<Value>> {
    pl::call_list(it, "Image::ExifTool::TagTableKeys", vec![table.clone()])
}

fn init_encrypted_subdir_rs(it: &mut Interp, tag_info: &Value, tag_table_ptr: &Value) -> ER<()> {
    //
    // for encrypted NIKON_OFFSETS tables we loop through all SubDirectory tags in this table
    // and set the KnownEnd for each of these according to the last tag in the child tables
    //
    let mut vars = tag_table_ptr.get("VARS");
    if !vars.truthy() {
        vars = Value::new_hash();
        tag_table_ptr.set("VARS", vars.clone());
    }
    if vars.get("NIKON_OFFSETS").truthy() && !vars.get("NIKON_INITIALIZED").truthy() {
        vars.set("NIKON_INITIALIZED", n(1));
        for tag_id in tag_table_keys(it, tag_table_ptr)? {
            let tag_info = tag_table_ptr.get(tag_id.to_bytes());
            if pl::ref_of(&tag_info) != "HASH" {
                continue;
            }
            let subdir = tag_info.get("SubDirectory");
            if !subdir.truthy() {
                continue;
            }
            let tbl = pl::call(it, "Image::ExifTool::GetTagTable", vec![subdir.get("TagTable")])?;
            let mut keys = tag_table_keys(it, &tbl)?;
            keys.sort_by(|a, b| num_cmp(b, a)); // (reverse sort)
            let last = arg(&keys, 0);
            let end = get_tag_end_rs(it, &tbl, &last, &n(1))?;
            subdir.set("KnownEnd", end);
        }
    }
    //
    // for other encrypted Nikon tables we set the KnownStart/KnownEnd entries in the
    // SubDirectory of the parent tag
    //
    if !tag_info.get("NikonInitialized").truthy() {
        tag_info.set("NikonInitialized", n(1));
        let subdir = tag_info.get("SubDirectory");
        let start = or0(subdir.get("DecryptStart"));
        let mut off = subdir.get("DirOffset");
        let mut tag_ids = tag_table_keys(it, tag_table_ptr)?;
        tag_ids.sort_by(num_cmp);
        if !off.is_undef() {
            off = pl::add(&off, &start); // (DirOffset, if specified, is relative to DecryptStart)
        } else {
            // ignore tags that come before the start of encryption
            while !tag_ids.is_empty() && pl::lt(&tag_ids[0], &start) {
                tag_ids.remove(0);
            }
            off = n(0);
        }
        if !tag_ids.is_empty() {
            let (first, last) = (tag_ids[0].clone(), tag_ids[tag_ids.len() - 1].clone());
            let mut last_info = tag_table_ptr.get(last.to_bytes());
            if pl::ref_of(&last_info) == "ARRAY" {
                last_info = last_info.at(0);
            }
            let _ = last_info;
            if pl::gt(&pl::add(&first, &off), &start) {
                subdir.set("KnownStart", pl::add(&pl::int(&first), &off));
            }
            let end = get_tag_end_rs(it, tag_table_ptr, &last, &Value::Undef)?;
            subdir.set("KnownEnd", end);
            if !subdir.get("KnownEnd").is_undef() {
                subdir.set("KnownEnd", pl::add(&subdir.get("KnownEnd"), &off));
            } else {
                let msg = pl::concat(&pl::concat(&s("Internal error setting KnownEnd for "), &tag_table_ptr.get("SHORT_NAME")), &s("\n"));
                pl::warn(it, &msg.to_text())?;
            }
        } else {
            subdir.set("KnownStart", start.clone());
            subdir.set("KnownEnd", start);
        }
    }
    Ok(())
}

//------------------------------------------------------------------------------
/// Prepare to process NIKON_OFFSETS directory and decrypt necessary data
/// Inputs: 0) ExifTool ref, 1) data ref, 2) tag table ref, 3) decrypt start,
///         4) decrypt mode (0=piecewise, 1=continuous to end of last known section, 2=all)
/// Returns: end of decrypted data (or undef for piecewise decryption)
pub fn prepare_nikon_offsets(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(prepare_nikon_offsets_rs(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2), &arg(&a, 3), &arg(&a, 4))?)
}

fn prepare_nikon_offsets_rs(it: &mut Interp, et: &Value, data_pt: &Value, tag_table_ptr: &Value, start: &Value, decrypt_mode: &Value) -> ER<Value> {
    let offset = tag_table_ptr.get("VARS").get("NIKON_OFFSETS");
    let data_len = pl::length(&data_pt.deref());
    if pl::gt(&pl::add(&offset, &n(4)), &data_len) || pl::lt(&offset, start) {
        return Ok(Value::Undef);
    }
    let serial = et.get("NikonSerialKey");
    let count = et.get("NikonCountKey");
    let mut dpos = pl::add(&offset, &n(4)); // decrypt up to NumberOffsets
    let d = decrypt_rs(it, data_pt, start.clone(), pl::sub(&dpos, start), &serial, &count)?;
    pl::set_deref(data_pt, d)?;
    let num_offsets = get32u(it, data_pt, offset.clone())?;
    let more = pl::mul(&num_offsets, &n(4)); // more bytes to decrypt entire offsets table
    if pl::gt(&pl::add(&pl::add(&offset, &n(4)), &more), &data_len) {
        return Ok(Value::Undef);
    }
    let d = decrypt_rs(it, data_pt, dpos.clone(), more.clone(), &Value::Undef, &Value::Undef)?;
    pl::set_deref(data_pt, d)?;
    dpos = pl::add(&dpos, &more);
    let unknown = options(it, et, "Unknown")?;
    let mut off_info: Vec<[Value; 3]> = Vec::new();
    let mut end = Value::Undef;
    // extract non-zero offsets and create unknown subdirectories if Unknown > 1
    let mut i = n(0);
    while pl::lt(&i, &num_offsets) {
        let pos = pl::add(&pl::add(&offset, &n(4)), &pl::mul(&n(4), &i));
        i = pl::add(&i, &n(1));
        let off = get32u(it, data_pt, pos.clone())?;
        if !off.truthy() {
            continue;
        }
        let mut tag_info = tag_table_ptr.get(pos.to_bytes());
        let mut known = n(0);
        if tag_info.truthy() {
            if pl::ref_of(&tag_info) != "HASH" || !tag_info.get("Unknown").truthy() {
                known = n(1);
            }
        } else if pl::gt(&unknown, &n(1)) {
            // create new table for unknown information
            let tbl = pl::sprintf("Image::ExifTool::Nikon::UnknownInfo%.2x", &[pos.clone()]);
            let h = pl::pkg_hash(it, &tbl.to_text());
            if h.is_empty() {
                if let Value::HashRef(hr) = &h {
                    let attrs = crate::runtime::lex_hash(it, MODULE, "binaryDataAttrs");
                    let mut hm = hr.borrow_mut();
                    hm.clear();
                    for k in attrs.keys() {
                        hm.insert(&k, attrs.get(&k));
                    }
                    let groups = Value::new_hash();
                    groups.set("0", s("MakerNotes"));
                    groups.set("2", s("Unknown"));
                    hm.insert(b"GROUPS", groups);
                }
                pl::call(it, "Image::ExifTool::GetTagTable", vec![tbl.clone()])?;
            }
            // add unknown entry in offset table for this subdirectory
            let ti = Value::new_hash();
            ti.set("Name", pl::sprintf("UnknownOffset%.2x", &[pos.clone()]));
            ti.set("Format", s("int32u"));
            let sd = Value::new_hash();
            sd.set("TagTable", tbl);
            ti.set("SubDirectory", sd);
            ti.set("Unknown", n(2));
            tag_info = pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_table_ptr.clone(), pos.clone(), ti])?;
        }
        let _ = tag_info;
        off_info.push([pos, off, known]); // save parameters for non-zero offsets
    }
    // sort offsets in ascending order, and use the differences to calculate
    // directory lengths and update the SubDirectory DirLen's accordingly
    let mut sorted = off_info;
    sorted.sort_by(|a, b| num_cmp(&a[1], &b[1]).then_with(|| num_cmp(&a[0], &b[0])));
    sorted.push([n(0), pl::length(&data_pt.deref()), n(0)]);
    for i in 0..sorted.len() - 1 {
        let pos = sorted[i][0].clone();
        let len = pl::sub(&sorted[i + 1][1], &sorted[i][1]);
        let mut tag_info = tag_table_ptr.get(pos.to_bytes());
        if tag_info.truthy() && !(pl::ref_of(&tag_info) == "HASH" && tag_info.get("AlwaysDecrypt").truthy()) {
            tag_info = pl::method(it, et, "GetTagInfo", vec![tag_table_ptr.clone(), pos.clone()])?;
        }
        // set DirLen in SubDirectory entry
        let mut subdir = Value::Undef;
        if pl::ref_of(&tag_info) == "HASH" && {
            subdir = tag_info.get("SubDirectory");
            !subdir.is_undef()
        } {
            subdir.set("DirLen", len.clone());
        }
        if decrypt_mode.truthy() {
            // keep track of end of last known directory
            if sorted[i][2].truthy() {
                end = sorted[i + 1][1].clone();
            }
        } else if tag_info.truthy() && (pl::ref_of(&tag_info) != "HASH" || !tag_info.get("Unknown").truthy()) {
            // decrypt data piecewise as necessary
            let mut nn = len.clone();
            if subdir.truthy() && subdir.get("KnownEnd").truthy() {
                nn = subdir.get("KnownEnd");
                if pl::gt(&nn, &len) {
                    if !tag_info.get("AlwaysDecrypt").truthy() {
                        let msg = pl::concat(&s("Data too short for "), &tag_info.get("Name"));
                        pl::method(it, et, "Warn", vec![msg, n(1)])?;
                    }
                    nn = len;
                }
            }
            let d = decrypt_rs(it, data_pt, sorted[i][1].clone(), nn, &Value::Undef, &Value::Undef)?;
            pl::set_deref(data_pt, d)?;
        }
    }
    if decrypt_mode.truthy() {
        // decrypt the remaining required data
        if pl::num_eq(decrypt_mode, &n(2)) || !end.truthy() || pl::lt(&end, &dpos) {
            end = pl::length(&data_pt.deref());
        }
        let d = decrypt_rs(it, data_pt, dpos.clone(), pl::sub(&end, &dpos), &Value::Undef, &Value::Undef)?;
        pl::set_deref(data_pt, d)?;
    }
    Ok(end)
}

//------------------------------------------------------------------------------
/// Read/Write Nikon Encrypted data block
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success when reading, or new directory when writing (IsWriting set)
pub fn process_nikon_encrypted(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if !et.truthy() {
        return ret(n(1)); // allow dummy access
    }
    let mut serial = et.get("NikonSerialKey");
    let mut count = et.get("NikonCountKey");
    let digits = re(r"^\d+$", "");
    if !(!serial.is_undef() && !count.is_undef() && digits.is_match(&serial) && digits.is_match(&count)) {
        if !serial.is_undef() || !count.is_undef() {
            let msg = if !serial.is_undef() && !count.is_undef() {
                if digits.is_match(&serial) {
                    "invalid ShutterCount"
                } else {
                    "invalid SerialNumber"
                }
            } else if !serial.is_undef() {
                "no ShutterCount"
            } else {
                "no SerialNumber"
            };
            pl::method(it, &et, "Warn", vec![s(&format!("Can't decrypt Nikon information ({msg} key)"))])?;
        }
        et.delete("NikonSerialKey");
        et.delete("NikonCountKey");
        return ret(n(0));
    }
    let old_order = pl::call(it, "Image::ExifTool::GetByteOrder", vec![])?;
    let is_writing = dir_info.get("IsWriting");
    let verbose = if is_writing.truthy() { n(0) } else { options(it, &et, "Verbose")? };
    let tag_info = dir_info.get("TagInfo");
    let dir_start = dir_info.get("DirStart");
    let dl = dir_info.get("DirLen");
    let data0 = substr_w(it, &dir_info.get("DataPt").deref(), &dir_start, Some(&dl))?;
    let data = pl::sref(data0); // (\$data)

    let (mut start, offset);
    let (mut len, mut recrypt, mut new_serial, mut new_count, mut did_decrypt) = (Value::Undef, false, Value::Undef, Value::Undef, false);

    // must re-encrypt when writing if serial number or shutter count changes
    if is_writing.truthy() {
        if et.get("NewNikonSerialKey").truthy() {
            new_serial = et.get("NewNikonSerialKey");
            recrypt = true;
        }
        if et.get("NewNikonCountKey").truthy() {
            new_count = et.get("NewNikonCountKey");
            recrypt = true;
        }
    }
    if tag_info.truthy() && tag_info.get("SubDirectory").truthy() {
        // initialize SubDirectory entries used in encryption (KnownStart, KnownEnd)
        init_encrypted_subdir_rs(it, &tag_info, &tag_table_ptr)?;
        let subdir = tag_info.get("SubDirectory");
        start = or0(subdir.get("DecryptStart"));
        // DirOffset, if specified, is the offset to the start of the
        // directory relative to start of encrypted data
        offset = if !subdir.get("DirOffset").is_undef() { pl::add(&subdir.get("DirOffset"), &start) } else { n(0) };
        // must set byte ordering before calling PrepareNikonOffsets()
        if subdir.get("ByteOrder").truthy() {
            set_byte_order(it, subdir.get("ByteOrder"))?;
        }
        // prepare for processing NIKON_OFFSETS directory if necessary
        let unknown = pl::gt(&verbose, &n(2)) || pl::gt(&options(it, &et, "Unknown")?, &n(1));
        // decrypt mode: 0=piecewise, 1=continuous to end of last known section, 2=all
        let d_mode = if is_writing.truthy() {
            if recrypt {
                2
            } else {
                1
            }
        } else if unknown {
            2
        } else {
            0
        };
        if tag_table_ptr.get("VARS").get("NIKON_OFFSETS").truthy() {
            len = prepare_nikon_offsets_rs(it, &et, &data, &tag_table_ptr, &start, &n(d_mode))?;
            did_decrypt = true;
        } else if d_mode < 2 {
            if d_mode == 0 && subdir.get("KnownStart").truthy() {
                // initialize decryption parameters for address DecryptStart address
                decrypt_rs(it, &data, start.clone(), n(0), &serial, &count)?;
                // reset serial/count keys so we don't re-initialize below
                serial = Value::Undef;
                count = Value::Undef;
                // change decryption start to skip unnecessary data
                start = subdir.get("KnownStart");
            }
            if subdir.get("KnownEnd").truthy() {
                len = pl::sub(&subdir.get("KnownEnd"), &start);
            }
        }
    } else {
        start = n(0);
        offset = n(0);
    }
    let max_len = pl::sub(&pl::length(&data.deref()), &start);
    // decrypt all the data unless the length was specified
    if !(len.truthy() && pl::lt(&len, &max_len)) {
        len = max_len;
    }

    if !did_decrypt {
        let d = decrypt_rs(it, &data, start.clone(), len.clone(), &serial, &count)?;
        pl::set_deref(&data, d)?;
    }

    if pl::gt(&verbose, &n(2)) {
        pl::method(it, &et, "VerboseDir", vec![pl::concat(&s("Decrypted "), &tag_info.get("Name"))])?;
        pl::method(
            it,
            &et,
            "VerboseDump",
            vec![
                data.clone(),
                s("Prefix"),
                pl::concat(&et.get("INDENT"), &s("  ")),
                // (remove this because it is useful to have decrypted offsets start at 0)
                //DataPos => $dirStart + $$dirInfo{DataPos} + ($$dirInfo{Base} || 0),
            ],
        )?;
    }
    // process the decrypted information
    let subdir_info = Value::new_hash();
    subdir_info.set("DataPt", data.clone());
    subdir_info.set("DirStart", offset.clone());
    subdir_info.set("DirLen", pl::sub(&pl::length(&data.deref()), &offset));
    subdir_info.set("DirName", dir_info.get("DirName"));
    subdir_info.set("DataPos", pl::add(&dir_info.get("DataPos"), &dir_start));
    subdir_info.set("Base", dir_info.get("Base"));
    let mut rtn_val;
    if is_writing.truthy() {
        let changed = et.get("CHANGED");
        rtn_val = pl::method(it, &et, "WriteBinaryData", vec![subdir_info, tag_table_ptr.clone()])?;
        // must re-encrypt if serial number or shutter count changes
        if recrypt {
            if !new_serial.is_undef() {
                serial = new_serial;
            }
            if !new_count.is_undef() {
                count = new_count;
            }
            et.set("CHANGED", pl::add(&et.get("CHANGED"), &n(1)));
        }
        if pl::num_eq(&changed, &et.get("CHANGED")) {
            rtn_val = Value::Undef; // nothing changed so use original data
        } else {
            // add back any un-encrypted data at start
            if offset.truthy() {
                rtn_val = pl::concat(&pl::substr(&data.deref(), 0, Some(offset.to_i64())), &rtn_val);
            }
            // re-encrypt data (symmetrical algorithm)
            rtn_val = decrypt_rs(it, &pl::sref(rtn_val), start.clone(), len.clone(), &serial, &count)?;
            let msg = pl::concat(&pl::concat(&s("  [recrypted "), &tag_info.get("Name")), &s("]"));
            pl::method(it, &et, "VPrint", vec![n(2), et.get("INDENT"), msg])?;
        }
    } else {
        rtn_val = pl::method(it, &et, "ProcessBinaryData", vec![subdir_info, tag_table_ptr.clone()])?;
    }
    set_byte_order(it, old_order)?;
    ret(rtn_val)
}

//------------------------------------------------------------------------------
/// Pre-scan EXIF directory to extract specific tags
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) required tagID hash ref
/// Returns: 1 if directory was scanned successfully
pub fn prescan_exif(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(prescan_exif_rs(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2))?)
}

fn prescan_exif_rs(it: &mut Interp, _et: &Value, dir_info: &Value, tag_hash: &Value) -> ER<Value> {
    let mut data_pt = dir_info.get("DataPt");
    let mut data_pos = or0(dir_info.get("DataPos"));
    let mut data_len = dir_info.get("DataLen");
    let mut dir_start = or0(dir_info.get("DirStart"));
    let base = or0(dir_info.get("Base"));
    let raf = dir_info.get("RAF");
    let mut num_entries = Value::Undef;
    let (mut data, mut buff) = (Value::Undef, Value::Undef);

    // get number of entries in IFD
    if pl::ge(&dir_start, &n(0)) && pl::le(&dir_start, &pl::sub(&data_len, &n(2))) {
        num_entries = get16u(it, &data_pt, dir_start.clone())?;
        // reset $numEntries to read from file if necessary
        if pl::gt(&pl::add(&pl::add(&dir_start, &n(2)), &pl::mul(&n(12), &num_entries)), &data_len) {
            num_entries = Value::Undef;
        }
    }
    // read IFD from file if necessary
    if !num_entries.truthy() {
        if !raf.truthy() {
            return Ok(n(0));
        }
        data_pos = pl::add(&data_pos, &dir_start); // read data from the start of the directory
        if !(raf_seek(it, &raf, pl::add(&data_pos, &base), 0)? && raf_read_ok(it, &raf, &mut data, &n(2))?) {
            return Ok(n(0));
        }
        num_entries = get16u(it, &pl::sref(data.clone()), n(0))?;
        let len = pl::mul(&n(12), &num_entries);
        if !raf_read_ok(it, &raf, &mut buff, &len)? {
            return Ok(n(0));
        }
        data = pl::concat(&data, &buff);
        // update variables for the newly loaded IFD (already updated dataPos)
        data_len = pl::length(&data);
        data_pt = pl::sref(data);
        dir_start = n(0);
    }
    let format_name = pl::pkg_array(it, "Image::ExifTool::Exif::formatName");
    let format_size = pl::pkg_array(it, "Image::ExifTool::Exif::formatSize");
    // loop through Nikon MakerNote IFD entries
    let mut index = n(0);
    while pl::lt(&index, &num_entries) {
        let entry = pl::add(&pl::add(&dir_start, &n(2)), &pl::mul(&n(12), &index));
        index = pl::add(&index, &n(1));
        let tag_id = get16u(it, &data_pt, entry.clone())?;
        if !tag_hash.exists(tag_id.to_bytes()) {
            continue; // only extract required tags
        }
        let format = get16u(it, &data_pt, pl::add(&entry, &n(2)))?;
        if pl::lt(&format, &n(1)) || pl::gt(&format, &n(13)) {
            continue;
        }
        let count = get32u(it, &data_pt, pl::add(&entry, &n(4)))?;
        let size = pl::mul(&count, &format_size.at(format.to_i64()));
        let format_str = format_name.at(format.to_i64());
        let mut value_ptr = pl::add(&entry, &n(8)); // pointer to value within $$dataPt
        if pl::gt(&size, &n(4)) {
            if pl::gt(&size, &n(0x1000000)) {
                continue; // set a reasonable limit on data size (16MB)
            }
            value_ptr = get32u(it, &data_pt, value_ptr)?;
            // convert offset to pointer in $$dataPt
            // (don't yet handle EntryBased or FixOffsets)
            value_ptr = pl::sub(&value_ptr, &data_pos);
            if pl::lt(&value_ptr, &n(0)) || pl::gt(&pl::add(&value_ptr, &size), &data_len) {
                if !(raf.truthy() && raf_seek(it, &raf, pl::add(&pl::add(&base, &value_ptr), &data_pos), 0)? && raf_read_ok(it, &raf, &mut buff, &size)?) {
                    continue;
                }
                let v = pl::call(it, "Image::ExifTool::ReadValue", vec![pl::sref(buff.clone()), n(0), format_str, count, size])?;
                tag_hash.set(tag_id.to_bytes(), v);
                continue;
            }
        }
        let v = pl::call(it, "Image::ExifTool::ReadValue", vec![data_pt.clone(), value_ptr, format_str, count, size])?;
        tag_hash.set(tag_id.to_bytes(), v);
    }
    Ok(n(1))
}

//------------------------------------------------------------------------------
/// Process Nikon Capture history data
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_nikon_capture_edit_versions(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    crate::runtime::require(it, "Image::ExifTool::NikonCapture")?;
    ret(pl::call(it, "Image::ExifTool::NikonCapture::ProcessNikonCaptureEditVersions", vec![et, dir_info, tag_table_ptr])?)
}

//------------------------------------------------------------------------------
/// Process Nikon Capture Offsets IFD (ref PH)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
/// Notes: This isn't a normal IFD, but is close...
pub fn process_nikon_capture_offsets(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let dir_start = dir_info.get("DirStart");
    let dir_len = dir_info.get("DirLen");
    let mut success = n(0);
    if !pl::gt(&dir_len, &n(2)) {
        return ret(n(0));
    }
    let count = get16u(it, &data_pt, dir_start.clone())?;
    if !(count.truthy() && pl::le(&pl::add(&pl::mul(&count, &n(12)), &n(2)), &dir_len)) {
        return ret(n(0));
    }
    if options(it, &et, "Verbose")?.truthy() {
        pl::method(it, &et, "VerboseDir", vec![s("NikonCaptureOffsets"), count.clone()])?;
    }
    let mut index = n(0);
    while pl::lt(&index, &count) {
        let pos = pl::add(&pl::add(&dir_start, &pl::mul(&n(12), &index)), &n(2));
        let tag_id = get32u(it, &data_pt, pos.clone())?;
        let value = get32u(it, &data_pt, pl::add(&pos, &n(4)))?;
        let r = pl::method(
            it,
            &et,
            "HandleTag",
            vec![tag_table_ptr.clone(), tag_id, value, s("Index"), index.clone(), s("DataPt"), data_pt.clone(), s("Start"), pos, s("Size"), n(12)],
        )?;
        if r.truthy() {
            success = n(1);
        }
        index = pl::add(&index, &n(1));
    }
    ret(success)
}

//------------------------------------------------------------------------------
/// Read Nikon NKA file
/// Inputs: 0) ExifTool ref, 1) dirInfo ref
/// Returns: 1 on success
pub fn process_nka(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let et = arg(&a, 0);
    let raf = et.get("RAF");
    let mut buff = Value::Undef;
    if !raf_read_ok(it, &raf, &mut buff, &n(0x35))? {
        return ret(n(0));
    }
    let len = pl::unpack1("x49V", &buff)?;
    if !raf_read_ok(it, &raf, &mut buff, &len)? {
        return ret(n(0));
    }
    pl::method(it, &et, "SetFileType", vec![s("NKA"), s("application/x-nikon-nxstudio")])?;
    let dir_info = Value::new_hash();
    dir_info.set("DataPt", pl::sref(buff));
    dir_info.set("DataPos", n(0x35));
    let tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![s("Image::ExifTool::XMP::XML")])?;
    ret(pl::method(it, &et, "ProcessDirectory", vec![dir_info, tag_table_ptr])?)
}

//------------------------------------------------------------------------------
/// Read/write Nikon MakerNotes directory
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success, otherwise returns 0 and sets a Warning when reading
///          or new directory when writing (IsWriting set in dirInfo)
pub fn process_nikon(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if !et.truthy() {
        return ret(n(1)); // allow dummy access
    }

    // pre-scan IFD to get SerialNumber (0x001d) and ShutterCount (0x00a7) for use in decryption
    let need_tags = Value::new_hash();
    need_tags.set("29", n(0));
    need_tags.set("167", Value::Undef);
    prescan_exif_rs(it, &et, &dir_info, &need_tags)?;
    et.set("NikonSerialKey", serial_key_rs(&et, &need_tags.get("29")));
    et.set("NikonCountKey", need_tags.get("167"));

    // process Nikon makernotes
    let rtn_val;
    if dir_info.get("IsWriting").truthy() {
        // get new decryptino keys if they are being changed
        let main = pl::pkg_hash(it, "Image::ExifTool::Nikon::Main");
        let serial = pl::method(it, &et, "GetNewValue", vec![main.get("29")])?;
        let count = pl::method(it, &et, "GetNewValue", vec![main.get("167")])?;
        et.set("NewNikonSerialKey", serial_key_rs(&et, &serial));
        et.set("NewNikonCountKey", count);
        rtn_val = pl::call(it, "Image::ExifTool::Exif::WriteExif", vec![et.clone(), dir_info, tag_table_ptr])?;
        et.delete("NewNikonSerialKey");
        et.delete("NewNikonCountKey");
    } else {
        rtn_val = pl::call(it, "Image::ExifTool::Exif::ProcessExif", vec![et.clone(), dir_info, tag_table_ptr])?;
    }
    et.delete("NikonSerialKey");
    et.delete("NikonCountKey");
    ret(rtn_val)
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Nikon::ProcessNikonAVI", process_nikon_avi);
    it.define_native("Image::ExifTool::Nikon::PrintAFPoints", print_af_points);
    it.define_native("Image::ExifTool::Nikon::PrintAFPointsInv", print_af_points_inv);
    it.define_native("Image::ExifTool::Nikon::GetAFPointGrid", get_af_point_grid);
    it.define_native("Image::ExifTool::Nikon::PrintAFPointsGrid", print_af_points_grid);
    it.define_native("Image::ExifTool::Nikon::PrintAFPointsGridInv", print_af_points_grid_inv);
    it.define_native("Image::ExifTool::Nikon::PrintAFPointsLeftRight", print_af_points_left_right);
    it.define_native("Image::ExifTool::Nikon::PrintAFPointsUpDown", print_af_points_up_down);
    it.define_native("Image::ExifTool::Nikon::PrintPC", print_pc);
    it.define_native("Image::ExifTool::Nikon::PrintPCInv", print_pc_inv);
    it.define_native("Image::ExifTool::Nikon::LensIDConv", lens_id_conv);
    it.define_native("Image::ExifTool::Nikon::FormatString", format_string);
    it.define_native("Image::ExifTool::Nikon::Decrypt", decrypt);
    it.define_native("Image::ExifTool::Nikon::SerialKey", serial_key);
    it.define_native("Image::ExifTool::Nikon::ProcessNikonApp", process_nikon_app);
    it.define_native("Image::ExifTool::Nikon::ProcessNikonMOV", process_nikon_mov);
    it.define_native("Image::ExifTool::Nikon::GetTagEnd", get_tag_end);
    it.define_native("Image::ExifTool::Nikon::InitEncryptedSubdir", init_encrypted_subdir);
    it.define_native("Image::ExifTool::Nikon::PrepareNikonOffsets", prepare_nikon_offsets);
    it.define_native("Image::ExifTool::Nikon::ProcessNikonEncrypted", process_nikon_encrypted);
    it.define_native("Image::ExifTool::Nikon::PrescanExif", prescan_exif);
    it.define_native("Image::ExifTool::Nikon::ProcessNikonCaptureEditVersions", process_nikon_capture_edit_versions);
    it.define_native("Image::ExifTool::Nikon::ProcessNikonCaptureOffsets", process_nikon_capture_offsets);
    it.define_native("Image::ExifTool::Nikon::ProcessNKA", process_nka);
    it.define_native("Image::ExifTool::Nikon::ProcessNikon", process_nikon);
}
