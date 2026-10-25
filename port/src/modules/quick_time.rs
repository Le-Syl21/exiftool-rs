//! Image::ExifTool::QuickTime (Image/ExifTool/QuickTime.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read QuickTime and MP4 meta information. The tag tables are data; these
//! are the module's subs: the atom handlers (ProcessMOV and the others), the
//! HEIF item parsers and the value conversions.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::exiftool::part2;
use crate::modules::random_access::substr as substr_w;
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, re, ret};
use crate::runtime;

const PKG: &str = "Image::ExifTool::QuickTime";

// ---- local helpers ----

fn n(i: i64) -> Value {
    Value::Int(i)
}

fn s(x: &str) -> Value {
    pl::s(x)
}

/// `$obj->Method(ARGS)` in scalar context.
fn m(it: &mut Interp, obj: &Value, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::method(it, obj, name, args)
}

/// `Image::ExifTool::Func(ARGS)` in scalar context.
fn etc(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

fn scalar(r: ER<Vec<Value>>) -> ER<Value> {
    Ok(r?.pop().unwrap_or_default())
}

fn get8u(it: &mut Interp, dp: &Value, pos: Value) -> ER<Value> {
    scalar(part2::get8u(it, vec![dp.clone(), pos], Want::Scalar))
}

fn get16u(it: &mut Interp, dp: &Value, pos: Value) -> ER<Value> {
    scalar(part2::get16u(it, vec![dp.clone(), pos], Want::Scalar))
}

fn get32u(it: &mut Interp, dp: &Value, pos: Value) -> ER<Value> {
    scalar(part2::get32u(it, vec![dp.clone(), pos], Want::Scalar))
}

fn get64u(it: &mut Interp, dp: &Value, pos: Value) -> ER<Value> {
    etc(it, "Get64u", vec![dp.clone(), pos])
}

/// `ReadValue(DATAPT, OFFSET, FORMAT, COUNT, SIZE)` in scalar context.
fn read_value(it: &mut Interp, dp: &Value, off: Value, fmt: Value, count: Value, size: Value) -> ER<Value> {
    scalar(part2::read_value(it, vec![dp.clone(), off, fmt, count, size], Want::Scalar))
}

fn get_tag_table(it: &mut Interp, name: &str) -> ER<Value> {
    etc(it, "GetTagTable", vec![s(name)])
}

fn add_tag_to_table(it: &mut Interp, tbl: &Value, tag: &Value, info: &Value) -> ER<Value> {
    etc(it, "AddTagToTable", vec![tbl.clone(), tag.clone(), info.clone()])
}

fn set_byte_order(it: &mut Interp, o: &str) -> ER<Value> {
    scalar(part2::set_byte_order(it, vec![s(o)], Want::Scalar))
}

/// `$raf->Read($buff, $len)`: the count, and `buff` updated.
fn raf_read(it: &mut Interp, raf: &Value, buff: &mut Value, len: Value) -> ER<Value> {
    let mut a = [buff.clone(), len];
    let r = scalar(pl::method_mut(it, raf, "Read", &mut a, Want::Scalar))?;
    *buff = a[0].clone();
    Ok(r)
}

fn raf_seek(it: &mut Interp, raf: &Value, pos: Value, whence: Value) -> ER<bool> {
    Ok(m(it, raf, "Seek", vec![pos, whence])?.truthy())
}

fn raf_tell(it: &mut Interp, raf: &Value) -> ER<Value> {
    m(it, raf, "Tell", vec![])
}

/// A file lexical of QuickTime.pm (`my %ftypLookup`...).
fn lex_hash(it: &mut Interp, name: &str) -> Value {
    runtime::lex_hash(it, PKG, name)
}

/// A package hash of QuickTime.pm (`%eeBox`, `%stringEncoding`...).
fn pkg_hash(it: &mut Interp, name: &str) -> Value {
    pl::pkg_hash(it, &format!("{PKG}::{name}"))
}

fn cat(parts: &[&Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

/// `scalar(@a)` as a number.
fn count(v: &[Value]) -> Value {
    n(v.len() as i64)
}

/// The numerical sort of hash keys (`sort { $a <=> $b } keys %$h`).
fn sorted_num_keys(h: &Value) -> Vec<Value> {
    let mut k: Vec<Value> = h.keys().into_iter().map(Value::Str).collect();
    k.sort_by(|a, b| ops::num_cmp(a, b).unwrap_or(std::cmp::Ordering::Equal));
    k
}

/// Whether a value written back by a callee differs from the original.
fn changed(a: &Value, b: &Value) -> bool {
    if a.is_ref() || b.is_ref() {
        return !a.same_ref(b);
    }
    a.is_undef() != b.is_undef() || a.to_bytes() != b.to_bytes()
}

/// Call `name` with `@_` aliased to our own arguments (`&$AUTOLOAD(@_)`,
/// `DoAutoLoad($AUTOLOAD, @_)`): what the callee writes to them reaches
/// our caller. `skip` is the number of leading arguments that are not ours.
fn call_aliased(it: &mut Interp, name: &str, mut args: Vec<Value>, orig: &[Value], skip: usize, want: Want) -> ER<Vec<Value>> {
    let r = pl::call_mut(it, name, &mut args, want)?;
    for (i, (new, old)) in args[skip..].iter().zip(orig.iter()).enumerate() {
        if changed(new, old) {
            it.set_arg(i, new.clone())?;
        }
    }
    Ok(r)
}

// ---- the subs ----

/// AutoLoad our routines when necessary
pub fn autoload(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let autoload = it.get_scalar("Image::ExifTool::QuickTime::AUTOLOAD");
    // (Note: no need to autoload routines in QuickTimeStream that use Stream table)
    if autoload.eq_str("Image::ExifTool::QuickTime::Process_mebx") {
        runtime::require(it, "Image/ExifTool/QuickTimeStream.pl")?;
        call_aliased(it, &autoload.to_text(), a.clone(), &a, 0, want)
    } else {
        let mut args = vec![autoload];
        args.extend(a.iter().cloned());
        call_aliased(it, "Image::ExifTool::DoAutoLoad", args, &a, 1, want)
    }
}


/// Get rotation matrix
/// Inputs: 0) angle in degrees
/// Returns: 9-element rotation matrix as a string (with 0 x/y offsets)
pub fn get_rotation_matrix(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let ang = pl::div(&pl::mul(&Value::Num(3.14159265358979323846264), &arg(&a, 0)), &n(180))?;
    let mut cos = Value::Num(ang.to_f64().cos());
    let mut sin = Value::Num(ang.to_f64().sin());
    // round to zero
    if pl::lt(&pl::abs(&cos), &Value::Num(1e-12)) {
        cos = n(0);
    }
    if pl::lt(&pl::abs(&sin), &Value::Num(1e-12)) {
        sin = n(0);
    }
    let msn = pl::neg(&sin);
    ret(pl::join(" ", &[cos.clone(), sin, n(0), msn, cos, n(0), n(0), n(0), n(1)]))
}

/// Get rotation angle from a matrix
/// Inputs: 0) rotation matrix as a string
/// Return: positive rotation angle in degrees rounded to 3 decimal points,
///         or undef on error
pub fn get_rotation_angle(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(get_rotation_angle_impl(&arg(&a, 0))?)
}

fn get_rotation_angle_impl(rot_matrix: &Value) -> ER<Value> {
    let a = pl::split_ws(rot_matrix);
    if pl::num_eq(&arg(&a, 0), &n(0)) && pl::num_eq(&arg(&a, 1), &n(0)) {
        return Ok(Value::Undef);
    }
    // calculate the rotation angle (assume uniform rotation)
    let at = Value::Num(arg(&a, 1).to_f64().atan2(arg(&a, 0).to_f64()));
    let mut angle = pl::div(&pl::mul(&at, &n(180)), &Value::Num(3.14159))?;
    if pl::lt(&angle, &n(0)) {
        angle = pl::add(&angle, &n(360));
    }
    pl::div(&pl::int(&pl::add(&pl::mul(&angle, &n(1000)), &Value::Num(0.5))), &n(1000))
}

/// Calculate rotation of video track
/// Inputs: 0) ExifTool object ref
/// Returns: rotation angle or undef
pub fn calc_rotation(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let et = arg(&a, 0);
    let value = et.get("VALUE");
    let mut track = Value::Undef;
    // get the video track family 1 group (eg. "Track1");
    let mut i = 0i64;
    loop {
        let idx = if i != 0 { format!(" ({i})") } else { String::new() };
        let tag = s(&format!("HandlerType{idx}"));
        if !value.get(tag.to_bytes()).truthy() {
            break;
        }
        if !value.get(tag.to_bytes()).eq_str("vide") {
            i += 1;
            continue;
        }
        track = m(it, &et, "GetGroup", vec![tag, n(1)])?;
        break;
    }
    if !track.truthy() {
        return pl::ret_undef();
    }
    // get the video track matrix
    let mut i = 0i64;
    loop {
        let idx = if i != 0 { format!(" ({i})") } else { String::new() };
        let tag = s(&format!("MatrixStructure{idx}"));
        if !value.get(tag.to_bytes()).truthy() {
            break;
        }
        let g = m(it, &et, "GetGroup", vec![tag.clone(), n(1)])?;
        if g.to_bytes() != track.to_bytes() {
            i += 1;
            continue;
        }
        return ret(get_rotation_angle_impl(&value.get(tag.to_bytes()))?);
    }
    pl::ret_undef()
}

/// Get MatrixStructure for a given rotation angle
/// Inputs: 0) rotation angle (deg), 1) ExifTool ref
/// Returns: matrix structure as a string, or undef if it can't be rotated
/// - requires ImageSizeLookahead to determine the video image size, and doesn't
///   rotate matrix unless image size is valid
pub fn get_matrix_structure(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (val, et) = (arg(&a, 0), arg(&a, 1));
    let mut av = pl::split_ws(&val);
    // pass straight through if it already has an offset
    if !(pl::num_eq(&arg(&av, 6), &n(0)) && pl::num_eq(&arg(&av, 7), &n(0))) {
        return ret(val);
    }
    let sv = pl::split_ws(&et.get("ImageSizeLookahead"));
    let (mut w, mut h) = (arg(&sv, 12), arg(&sv, 13));
    if !(w.truthy() && h.truthy()) {
        return pl::ret_undef(); // don't rotate 0-sized track
    }
    w = fix_wrong_format_impl(&w)?;
    h = fix_wrong_format_impl(&h)?;
    // apply necessary offsets for the standard rotations
    let angle = get_rotation_angle_impl(&val)?;
    if angle.is_undef() {
        return pl::ret_undef();
    }
    let set67 = |av: &mut Vec<Value>, x: Value, y: Value| {
        if av.len() < 8 {
            av.resize(8, Value::Undef);
        }
        av[6] = x;
        av[7] = y;
    };
    if pl::num_eq(&angle, &n(90)) {
        set67(&mut av, h, n(0));
    } else if pl::num_eq(&angle, &n(180)) {
        set67(&mut av, w, h);
    } else if pl::num_eq(&angle, &n(270)) {
        set67(&mut av, n(0), w);
    }
    ret(pl::join(" ", &av))
}

/// Determine the average sample rate from a time-to-sample table
/// Inputs: 0) ExifTool object ref, 1) time-to-sample table data ref
/// Returns: average sample rate (in Hz)
pub fn calc_sample_rate(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, val_pt) = (arg(&a, 0), arg(&a, 1));
    let dat = pl::unpack("N*", &val_pt.deref())?;
    let (mut num, mut dur) = (n(0), n(0));
    let mut i = 2usize;
    while (i as i64) < dat.len() as i64 - 1 {
        num = pl::add(&num, &dat[i]); // total number of samples
        dur = pl::add(&dur, &pl::mul(&dat[i], &dat[i + 1])); // total sample duration
        i += 2;
    }
    if !(num.truthy() && dur.truthy() && et.get("MediaTS").truthy()) {
        return pl::ret_undef();
    }
    ret(pl::div(&pl::mul(&num, &et.get("MediaTS")), &dur)?)
}

/// Fix incorrect format for ImageWidth/Height as written by Pentax
pub fn fix_wrong_format(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(fix_wrong_format_impl(&arg(&a, 0))?)
}

fn fix_wrong_format_impl(val: &Value) -> ER<Value> {
    if !val.truthy() {
        return Ok(Value::Undef);
    }
    if ops::bit_and(val, &n(0xfff00000)).truthy() {
        pl::unpack1("n", &pl::pack("N", &[val.clone()])?)
    } else {
        Ok(val.clone())
    }
}

/// Convert ISO 6709 string to standard lag/lon format
/// Inputs: 0) ISO 6709 string (lat, lon, and optional alt)
/// Returns: position in decimal degrees with altitude if available
/// Notes: Wikipedia indicates altitude may be in feet -- how is this specified?
pub fn convert_iso6709(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut val = arg(&a, 0);
    let z = n(0);
    if let Some(c) = re(r"^([-+]\d{1,2}(?:\.\d*)?)([-+]\d{1,3}(?:\.\d*)?)([-+]\d+(?:\.\d*)?)?", "").m(&val) {
        // +DD.DDD+DDD.DDD+AA.AAA
        val = cat(&[&pl::add(&cap(&c, 1), &z), &s(" "), &pl::add(&cap(&c, 2), &z)]);
        if cap(&c, 3).truthy() {
            val = cat(&[&val, &s(" "), &pl::add(&cap(&c, 3), &z)]);
        }
    } else if let Some(c) = re(r"^([-+])(\d{2})(\d{2}(?:\.\d*)?)([-+])(\d{3})(\d{2}(?:\.\d*)?)([-+]\d+(?:\.\d*)?)?", "").m(&val) {
        // +DDMM.MMM+DDDMM.MMM+AA.AAA
        let mut lat = pl::add(&cap(&c, 2), &pl::div(&cap(&c, 3), &n(60))?);
        if pl::str_eq(&cap(&c, 1), "-") {
            lat = pl::neg(&lat);
        }
        let mut lon = pl::add(&cap(&c, 5), &pl::div(&cap(&c, 6), &n(60))?);
        if pl::str_eq(&cap(&c, 4), "-") {
            lon = pl::neg(&lon);
        }
        val = cat(&[&lat, &s(" "), &lon]);
        if cap(&c, 7).truthy() {
            val = cat(&[&val, &s(" "), &pl::add(&cap(&c, 7), &z)]);
        }
    } else if let Some(c) = re(r"^([-+])(\d{2})(\d{2})(\d{2}(?:\.\d*)?)([-+])(\d{3})(\d{2})(\d{2}(?:\.\d*)?)([-+]\d+(?:\.\d*)?)?", "").m(&val) {
        // +DDMMSS.SSS+DDDMMSS.SSS+AA.AAA
        let mut lat = pl::add(&pl::add(&cap(&c, 2), &pl::div(&cap(&c, 3), &n(60))?), &pl::div(&cap(&c, 4), &n(3600))?);
        if pl::str_eq(&cap(&c, 1), "-") {
            lat = pl::neg(&lat);
        }
        let mut lon = pl::add(&pl::add(&cap(&c, 6), &pl::div(&cap(&c, 7), &n(60))?), &pl::div(&cap(&c, 8), &n(3600))?);
        if pl::str_eq(&cap(&c, 5), "-") {
            lon = pl::neg(&lon);
        }
        val = cat(&[&lat, &s(" "), &lon]);
        if cap(&c, 9).truthy() {
            val = cat(&[&val, &s(" "), &pl::add(&cap(&c, 9), &z)]);
        }
    }
    ret(val)
}

/// Convert Nero chapter list (ref ffmpeg libavformat/movenc.c)
/// Inputs: 0) binary chpl data
/// Returns: chapter list
pub fn convert_chapter_list(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let val = arg(&a, 0);
    let size = val.to_bytes().len() as i64;
    if size < 9 {
        return ret(s("<invalid>"));
    }
    let vp = pl::sref(val.clone());
    let num = get8u(it, &vp, n(8))?;
    let mut chapters = Vec::new();
    let mut pos = 9i64;
    let mut i = n(0);
    while pl::lt(&i, &num) {
        if pos + 9 > size {
            break;
        }
        let dur = pl::div(&get64u(it, &vp, n(pos))?, &n(10000000))?;
        let len = get8u(it, &vp, n(pos + 8))?.to_i64();
        if pos + 9 + len > size {
            break;
        }
        let title = substr_w(it, &val, &n(pos + 9), Some(&n(len)))?;
        pos += 9 + len;
        chapters.push(cat(&[&dur, &s(" "), &title]));
        i = pl::add(&i, &n(1));
    }
    ret(Value::new_array(chapters)) // return as a list
}

/// Print conversion for a Nero chapter list item
/// Inputs: 0) ValueConv chapter string
/// Returns: formatted chapter string
pub fn print_chapter(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let val = arg(&a, 0);
    let Some(c) = re(r"^(\S+) (.*)", "").m(&val) else {
        return ret(val);
    };
    let (mut dur, title) = (cap(&c, 1), cap(&c, 2));
    let mut h = pl::int(&pl::div(&dur, &n(3600))?);
    dur = pl::sub(&dur, &pl::mul(&h, &n(3600)));
    let mut mm = pl::int(&pl::div(&dur, &n(60))?);
    let sec = pl::sub(&dur, &pl::mul(&mm, &n(60)));
    let mut ss = pl::sprintf("%06.3f", &[sec]);
    if pl::ge(&ss, &n(60)) {
        ss = s("00.000");
        // ++$m >= 60 and $m -= 60, ++$h;
        mm = pl::add(&mm, &n(1));
        if pl::ge(&mm, &n(60)) {
            mm = pl::sub(&mm, &n(60));
            h = pl::add(&h, &n(1));
        }
    }
    ret(pl::sprintf("[%d:%.2d:%s] %s", &[h, mm, ss, title]))
}

/// Format GPSCoordinates for printing
/// Inputs: 0) string with numerical lat, lon and optional alt, separated by spaces
///         1) ExifTool object reference
/// Returns: PrintConv value
pub fn print_gps_coordinates(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (val, et) = (arg(&a, 0), arg(&a, 1));
    let v = pl::split_ws(&val);
    let lat = pl::call(it, "Image::ExifTool::GPS::ToDMS", vec![et.clone(), arg(&v, 0), n(1), s("N")])?;
    let lon = pl::call(it, "Image::ExifTool::GPS::ToDMS", vec![et.clone(), arg(&v, 1), n(1), s("E")])?;
    let mut prt = cat(&[&lat, &s(", "), &lon]);
    if v.len() > 2 && !v[2].is_undef() {
        let alt = if pl::lt(&v[2], &n(0)) { cat(&[&pl::neg(&v[2]), &s(" m Below")]) } else { cat(&[&v[2], &s(" m Above")]) };
        prt = cat(&[&prt, &s(", "), &alt, &s(" Sea Level")]);
    }
    ret(prt)
}

/// Unpack packed ISO 639/T language code
/// Inputs: 0) packed language code (or undef/0), 1) true to not treat 'und' and 'eng' as default
/// Returns: language code, or undef/0 for default language, or 'err' for format error
pub fn unpack_lang(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(unpack_lang_impl(&arg(&a, 0), &arg(&a, 1))?)
}

fn unpack_lang_impl(lang: &Value, no_def: &Value) -> ER<Value> {
    let mut lang = lang.clone();
    if lang.truthy() {
        // language code is packed in 5-bit characters
        let c: Vec<Value> = [10, 5, 0].iter().map(|sh| pl::add(&ops::bit_and(&ops::shift_right(&lang, &n(*sh)), &n(0x1f)), &n(0x60))).collect();
        lang = pl::pack("C*", &c)?;
        // validate language code
        if re(r"^[a-z]+$", "").is_match(&lang) {
            // treat 'eng' or 'und' as the default language
            if (lang.eq_str("und") || lang.eq_str("eng")) && !no_def.truthy() {
                lang = Value::Undef;
            }
        } else {
            lang = s("err"); // invalid language code
        }
    }
    Ok(lang)
}

/// Get language code string given QuickTime language and country codes
/// Inputs: 0) numerical language code, 1) numerical country code, 2) no defaults
/// Returns: language code string (ie. "fra-FR") or undef for default language
/// ex) 0x15c7 0x0000 is 'eng' with no country (ie. returns 'und' unless $noDef)
///     0x15c7 0x5553 is 'eng-US'
///     0x1a41 0x4652 is 'fra-FR'
///     0x55c4 is 'und'
pub fn get_lang_code(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(get_lang_code_impl(&arg(&a, 0), &arg(&a, 1), &arg(&a, 2))?)
}

fn get_lang_code_impl(lang: &Value, ctry: &Value, no_def: &Value) -> ER<Value> {
    let (mut lang, mut ctry) = (lang.clone(), ctry.clone());
    // ignore country ('ctry') and language lists ('lang') for now
    if ctry.truthy() && pl::le(&ctry, &n(255)) {
        ctry = Value::Undef;
    }
    if lang.truthy() && pl::le(&lang, &n(255)) {
        lang = Value::Undef;
    }
    let mut lang_code = unpack_lang_impl(&lang, no_def)?;
    // add country code if specified
    if ctry.truthy() {
        ctry = pl::unpack1("a2", &pl::pack("n", &[ctry])?)?; // unpack as ISO 3166-1
        // treat 'ZZ' like a default country (see ref 12)
        if ctry.eq_str("ZZ") {
            ctry = Value::Undef;
        }
        if ctry.truthy() && re(r"^[A-Z]{2}$", "").is_match(&ctry) {
            if !lang_code.truthy() {
                lang_code = unpack_lang_impl(&lang, &n(1))?;
                if !lang_code.truthy() {
                    lang_code = s("und");
                }
            }
            lang_code = cat(&[&lang_code, &s("-"), &ctry]);
        }
    }
    Ok(lang_code)
}

/// Get langInfo hash and save details about alt-lang tags
/// Inputs: 0) ExifTool ref, 1) tagInfo hash ref, 2) locale code
/// Returns: new tagInfo hash ref, or undef if invalid
pub fn get_lang_info_qt(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(get_lang_info_qt_impl(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2))?)
}

fn get_lang_info_qt_impl(it: &mut Interp, et: &Value, tag_info: &Value, lang_code: &Value) -> ER<Value> {
    let lang_info = etc(it, "GetLangInfo", vec![tag_info.clone(), lang_code.clone()])?;
    if lang_info.truthy() {
        if !et.get("QTLang").truthy() {
            et.set("QTLang", Value::new_array(vec![]));
        }
        et.get_or_new_array("QTLang").push(lang_info.get("Name"));
    }
    Ok(lang_info)
}

/// Get variable-length integer from data (used by ParseItemLocation)
/// Inputs: 0) data ref, 1) start position, 2) integer size in bytes (0, 4 or 8),
///         3) default value
/// Returns: integer value, and updates current position
pub fn get_var_int(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut pos = arg(&a, 1);
    let r = get_var_int_impl(it, &arg(&a, 0), &mut pos, &arg(&a, 2), &arg(&a, 3));
    it.set_arg(1, pos)?;
    ret(r?)
}

fn get_var_int_impl(it: &mut Interp, data_pt: &Value, pos_arg: &mut Value, nn: &Value, default: &Value) -> ER<Value> {
    let pos = pos_arg.clone();
    let len = n(data_pt.deref().to_bytes().len() as i64);
    *pos_arg = pl::add(&pos, nn); // update current position
    if pl::gt(&pl::add(&pos, nn), &len) {
        return Ok(Value::Undef);
    }
    if pl::num_eq(nn, &n(0)) {
        return Ok(if default.truthy() { default.clone() } else { n(0) });
    } else if pl::num_eq(nn, &n(4)) {
        return get32u(it, data_pt, pos);
    } else if pl::num_eq(nn, &n(8)) {
        return get64u(it, data_pt, pos);
    }
    Ok(Value::Undef)
}

/// Get null-terminated string from binary data (used by ParseItemInfoEntry)
/// Inputs: 0) data ref, 1) start position
/// Returns: string, and updates current position
pub fn get_string(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut pos = arg(&a, 1);
    let r = get_string_impl(it, &arg(&a, 0), &mut pos);
    it.set_arg(1, pos)?;
    ret(r?)
}

fn get_string_impl(_it: &mut Interp, data_pt: &Value, pos_arg: &mut Value) -> ER<Value> {
    let data = data_pt.deref().to_bytes();
    let len = n(data.len() as i64);
    let mut pos = pos_arg.clone();
    let mut out = Vec::new();
    while pl::lt(&pos, &len) {
        let ch = data[pos.to_i64() as usize];
        pos = pl::add(&pos, &n(1));
        if ch == 0 {
            break;
        }
        out.push(ch);
    }
    *pos_arg = pos; // update current position
    Ok(Value::bytes(out))
}

/// Get a printable version of the tag ID
/// Inputs: 0) tag ID, 1) Flag: 0x01 - print as 4- or 8-digit hex value if necessary
///                             0x02 - put leading backslash before escaped character
/// Returns: Printable tag ID
pub fn printable_tag_id(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(printable_tag_id_impl(&arg(&a, 0), &arg(&a, 1))?)
}

fn hex_of(c: &pl::Caps) -> Vec<u8> {
    let b = cap(c, 1).to_bytes();
    b.iter().flat_map(|x| format!("{x:02x}").into_bytes()).collect()
}

fn printable_tag_id_impl(tag0: &Value, flag: &Value) -> ER<Value> {
    let mut tag = tag0.clone();
    let nn = re(r"([^-_a-zA-Z0-9])", "").subst(&mut tag, true, |c| {
        let mut v = b"x".to_vec();
        v.extend(hex_of(c));
        v
    });
    if nn > 0 && flag.truthy() {
        if nn > 2 && ops::bit_and(flag, &n(0x01)).truthy() {
            tag = cat(&[&s("0x"), &pl::unpack1("H8", tag0)?]);
            re(r"^0x0000", "").subst(&mut tag, false, |_| b"0x".to_vec());
        } else if ops::bit_and(flag, &n(0x02)).truthy() {
            tag = tag0.clone();
            re(r"([^-_a-zA-Z0-9])", "").subst(&mut tag, true, |c| {
                let mut v = b"\\x".to_vec();
                v.extend(hex_of(c));
                v
            });
        }
    }
    Ok(tag)
}

/// Parse item location (iloc) box (ref ISO 14496-12:2015 pg.79)
/// Inputs: 0) iloc data, 1) ExifTool ref
/// Returns: undef, and fills in ExifTool ItemInfo hash
/// Notes: see also Handle_iloc() in WriteQuickTime.pl
pub fn parse_item_location(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (val, et) = (arg(&a, 0), arg(&a, 1));
    let mut id;
    let mut extent_index = Value::Undef;
    let (mut extent_offset, mut extent_length);

    let verbose = if et.get("IsWriting").truthy() { n(0) } else { m(it, &et, "Options", vec![s("Verbose")])? };
    let mut items = et.get("ItemInfo");
    if !items.truthy() {
        items = Value::new_hash();
        et.set("ItemInfo", items.clone());
    }
    let len = n(val.to_bytes().len() as i64);
    if pl::lt(&len, &n(8)) {
        return pl::ret_undef();
    }
    let vp = pl::sref(val.clone());
    let ver = get8u(it, &vp, n(0))?;
    let siz = get16u(it, &vp, n(4))?;
    let noff = ops::shift_right(&siz, &n(12));
    let nlen = ops::bit_and(&ops::shift_right(&siz, &n(8)), &n(0x0f));
    let nbas = ops::bit_and(&ops::shift_right(&siz, &n(4)), &n(0x0f));
    let nind = ops::bit_and(&siz, &n(0x0f));
    let num;
    let mut pos;
    if pl::lt(&ver, &n(2)) {
        num = get16u(it, &vp, n(6))?;
        pos = n(8);
    } else {
        if pl::lt(&len, &n(10)) {
            return pl::ret_undef();
        }
        num = get32u(it, &vp, n(6))?;
        pos = n(10);
    }
    let ver12 = pl::num_eq(&ver, &n(1)) || pl::num_eq(&ver, &n(2));
    let mut i = n(0);
    while pl::lt(&i, &num) {
        if pl::lt(&ver, &n(2)) {
            if pl::gt(&pl::add(&pos, &n(2)), &len) {
                return pl::ret_undef();
            }
            id = get16u(it, &vp, pos.clone())?;
            pos = pl::add(&pos, &n(2));
        } else {
            if pl::gt(&pl::add(&pos, &n(4)), &len) {
                return pl::ret_undef();
            }
            id = get32u(it, &vp, pos.clone())?;
            pos = pl::add(&pos, &n(4));
        }
        if ver12 {
            if pl::gt(&pl::add(&pos, &n(2)), &len) {
                return pl::ret_undef();
            }
            let cm = ops::bit_and(&get16u(it, &vp, pos.clone())?, &n(0x0f));
            items.get_or_new_hash(id.to_bytes()).set("ConstructionMethod", cm);
            pos = pl::add(&pos, &n(2));
        }
        if pl::gt(&pl::add(&pos, &n(2)), &len) {
            return pl::ret_undef();
        }
        let item = items.get_or_new_hash(id.to_bytes());
        item.set("DataReferenceIndex", get16u(it, &vp, pos.clone())?);
        pos = pl::add(&pos, &n(2));
        let bo = get_var_int_impl(it, &vp, &mut pos, &nbas, &Value::Undef)?;
        item.set("BaseOffset", bo);
        if pl::gt(&pl::add(&pos, &n(2)), &len) {
            return pl::ret_undef();
        }
        let ext_num = get16u(it, &vp, pos.clone())?;
        pos = pl::add(&pos, &n(2));
        let mut extents = Vec::new();
        let mut j = n(0);
        while pl::lt(&j, &ext_num) {
            if ver12 {
                extent_index = get_var_int_impl(it, &vp, &mut pos, &nind, &n(1))?;
            }
            extent_offset = get_var_int_impl(it, &vp, &mut pos, &noff, &Value::Undef)?;
            extent_length = get_var_int_impl(it, &vp, &mut pos, &nlen, &Value::Undef)?;
            if extent_length.is_undef() {
                return pl::ret_undef();
            }
            if verbose.truthy() {
                let cm = item.get("ConstructionMethod");
                let cm = if cm.is_undef() { s("") } else { cm };
                let tail = pl::sprintf(" base=0x%x offset=0x%x len=0x%x\n", &[item.get("BaseOffset"), extent_offset.clone(), extent_length.clone()]);
                let head = cat(&[&et.get("INDENT"), &s("  Item "), &id, &s(": const_meth=")]);
                m(it, &et, "VPrint", vec![n(1), head, cm, tail])?;
            }
            extents.push(Value::new_array(vec![extent_index.clone(), extent_offset.clone(), extent_length.clone(), nlen.clone(), pl::sub(&pos, &nlen)]));
            j = pl::add(&j, &n(1));
        }
        // save item location information keyed on 1-based item ID:
        item.set("Extents", Value::new_array(extents));
        i = pl::add(&i, &n(1));
    }
    pl::ret_undef()
}

/// Parse content describes entry (cdsc) box
/// Inputs: 0) cdsc data, 1) ExifTool ref
/// Returns: undef, and fills in ExifTool ItemInfo hash
pub fn parse_content_describes(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (val, et) = (arg(&a, 0), arg(&a, 1));
    let mut v;
    if et.get("ItemRefVersion").truthy() {
        if val.to_bytes().len() < 10 {
            return pl::ret_undef();
        }
        v = pl::unpack("NnN*", &val)?;
    } else {
        if val.to_bytes().len() < 6 {
            return pl::ret_undef();
        }
        v = pl::unpack("nnn*", &val)?;
    }
    let id = if v.is_empty() { Value::Undef } else { v.remove(0) };
    let cnt = if v.is_empty() { Value::Undef } else { v.remove(0) };
    let mut to = v;
    if pl::gt(&cnt, &count(&to)) {
        let msg = s("Missing values in ContentDescribes box");
        if et.get("IsWriting").truthy() {
            m(it, &et, "Error", vec![msg])?;
        } else {
            m(it, &et, "Warn", vec![msg])?;
        }
    } else if pl::lt(&cnt, &count(&to)) {
        m(it, &et, "Warn", vec![s("Ignored extra values in ContentDescribes box"), n(1)])?;
        to = vec![cnt.clone()];
    }
    // add all referenced item ID's to a "RefersTo" lookup
    for t in &to {
        let refers = et.get_or_new_hash("ItemInfo").get_or_new_hash(id.to_bytes()).get_or_new_hash("RefersTo");
        refers.set(t.to_bytes(), n(1));
    }
    pl::ret_undef()
}

/// Parse item information entry (infe) box (ref ISO 14496-12:2015 pg.82)
/// Inputs: 0) infe data, 1) ExifTool ref
/// Returns: undef, and fills in ExifTool ItemInfo hash
pub fn parse_item_info_entry(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (val, et) = (arg(&a, 0), arg(&a, 1));
    let mut id = Value::Undef;

    let verbose = if et.get("IsWriting").truthy() { n(0) } else { m(it, &et, "Options", vec![s("Verbose")])? };
    let mut items = et.get("ItemInfo");
    if !items.truthy() {
        items = Value::new_hash();
        et.set("ItemInfo", items.clone());
    }
    let len = n(val.to_bytes().len() as i64);
    if pl::lt(&len, &n(4)) {
        return pl::ret_undef();
    }
    let vp = pl::sref(val.clone());
    let ver = get8u(it, &vp, n(0))?;
    let mut pos = n(4);
    if pl::gt(&pl::add(&pos, &n(4)), &len) {
        return pl::ret_undef();
    }
    if pl::num_eq(&ver, &n(0)) || pl::num_eq(&ver, &n(1)) {
        id = get16u(it, &vp, pos.clone())?;
        let item = items.get_or_new_hash(id.to_bytes());
        item.set("ProtectionIndex", get16u(it, &vp, pl::add(&pos, &n(2)))?);
        pos = pl::add(&pos, &n(4));
        let v = get_string_impl(it, &vp, &mut pos)?;
        item.set("Name", v);
        let v = get_string_impl(it, &vp, &mut pos)?;
        item.set("ContentType", v);
        let v = get_string_impl(it, &vp, &mut pos)?;
        item.set("ContentEncoding", v);
    } else {
        if pl::num_eq(&ver, &n(2)) {
            id = get16u(it, &vp, pos.clone())?;
            pos = pl::add(&pos, &n(2));
        } else if pl::num_eq(&ver, &n(3)) {
            id = get32u(it, &vp, pos.clone())?;
            pos = pl::add(&pos, &n(4));
        }
        if pl::gt(&pl::add(&pos, &n(6)), &len) {
            return pl::ret_undef();
        }
        let item = items.get_or_new_hash(id.to_bytes());
        item.set("ProtectionIndex", get16u(it, &vp, pos.clone())?);
        let typ = substr_w(it, &val, &pl::add(&pos, &n(2)), Some(&n(4)))?;
        item.set("Type", typ.clone());
        pos = pl::add(&pos, &n(6));
        let v = get_string_impl(it, &vp, &mut pos)?;
        item.set("Name", v);
        if typ.eq_str("mime") {
            let v = get_string_impl(it, &vp, &mut pos)?;
            item.set("ContentType", v);
            let v = get_string_impl(it, &vp, &mut pos)?;
            item.set("ContentEncoding", v);
        } else if typ.eq_str("uri ") {
            let v = get_string_impl(it, &vp, &mut pos)?;
            item.set("URI", v);
        }
    }
    //[retracted] # save raw infe box when writing in case we need to sort items later
    //[retracted] $$items{$id}{infe} = pack('N', length($val)+8) . 'infe' . $val if $$et{IsWriting};
    if pl::gt(&verbose, &n(1)) {
        let item = items.get_or_new_hash(id.to_bytes());
        let or_e = |v: Value| if v.truthy() { v } else { s("") };
        let prim = et.get("PrimaryItem");
        let p = if prim.truthy() && pl::num_eq(&prim, &id) { s(" (PrimaryItem)") } else { s("") };
        m(it, &et, "VPrint", vec![n(1), cat(&[&et.get("INDENT"), &s("  Item "), &id, &s(": Type=")]), or_e(item.get("Type")), s(" Name="), or_e(item.get("Name")), s(" ContentType="), or_e(item.get("ContentType")), p, s("\n")])?;
    }
    if !pl::gt(&id, &et.get("LastItemID")) {
        m(it, &et, "Warn", vec![s("Item info entries are out of order")])?; //[retracted] unless $$et{IsWriting};
        //[retracted] $$et{ItemsNotSorted} = 1;   # set flag indicating the items weren't sorted
    }
    et.set("LastItemID", id);
    pl::ret_undef()
}

/// Parse item property association (ipma) box (ref https://github.com/gpac/gpac/blob/master/src/isomedia/iff.c)
/// Inputs: 0) ipma data, 1) ExifTool ref
/// Returns: undef, and fills in ExifTool ItemInfo hash
pub fn parse_item_prop_assoc(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (val, et) = (arg(&a, 0), arg(&a, 1));
    let mut id;

    let verbose = if et.get("IsWriting").truthy() { n(0) } else { m(it, &et, "Options", vec![s("Verbose")])? };
    let mut items = et.get("ItemInfo");
    if !items.truthy() {
        items = Value::new_hash();
        et.set("ItemInfo", items.clone());
    }
    let len = n(val.to_bytes().len() as i64);
    if pl::lt(&len, &n(8)) {
        return pl::ret_undef();
    }
    let vp = pl::sref(val.clone());
    let ver = get8u(it, &vp, n(0))?;
    let flg = get32u(it, &vp, n(0))?;
    let num = get32u(it, &vp, n(4))?;
    let mut pos = n(8);
    let mut last_id = n(-1);
    let mut i = n(0);
    while pl::lt(&i, &num) {
        if pl::num_eq(&ver, &n(0)) {
            if pl::gt(&pl::add(&pos, &n(3)), &len) {
                return pl::ret_undef();
            }
            id = get16u(it, &vp, pos.clone())?;
            pos = pl::add(&pos, &n(2));
        } else {
            if pl::gt(&pl::add(&pos, &n(5)), &len) {
                return pl::ret_undef();
            }
            id = get32u(it, &vp, pos.clone())?;
            pos = pl::add(&pos, &n(4));
        }
        let nn = get8u(it, &vp, pos.clone())?;
        pos = pl::add(&pos, &n(1));
        let (mut association, mut essential) = (Vec::new(), Vec::new());
        if ops::bit_and(&flg, &n(0x01)).truthy() {
            if pl::gt(&pl::add(&pos, &pl::mul(&nn, &n(2))), &len) {
                return pl::ret_undef();
            }
            let mut j = n(0);
            while pl::lt(&j, &nn) {
                let tmp = get16u(it, &vp, pl::add(&pos, &pl::mul(&j, &n(2))))?;
                association.push(ops::bit_and(&tmp, &n(0x7fff)));
                essential.push(if ops::bit_and(&tmp, &n(0x8000)).truthy() { n(1) } else { n(0) });
                j = pl::add(&j, &n(1));
            }
            pos = pl::add(&pos, &pl::mul(&nn, &n(2)));
        } else {
            if pl::gt(&pl::add(&pos, &nn), &len) {
                return pl::ret_undef();
            }
            let mut j = n(0);
            while pl::lt(&j, &nn) {
                let tmp = get8u(it, &vp, pl::add(&pos, &j))?;
                association.push(ops::bit_and(&tmp, &n(0x7f)));
                essential.push(if ops::bit_and(&tmp, &n(0x80)).truthy() { n(1) } else { n(0) });
                j = pl::add(&j, &n(1));
            }
            pos = pl::add(&pos, &nn);
        }
        let item = items.get_or_new_hash(id.to_bytes());
        let assoc_str = pl::join(" ", &association);
        item.set("Association", Value::new_array(association));
        item.set("Essential", Value::new_array(essential));
        if pl::gt(&verbose, &n(1)) {
            m(it, &et, "VPrint", vec![n(1), cat(&[&et.get("INDENT"), &s("  Item "), &id, &s(" properties: "), &assoc_str, &s("\n")])])?;
        }
        // (according to ISO/IEC 23008-12, these entries must be sorted by item ID)
        if !pl::gt(&id, &last_id) {
            m(it, &et, "Warn", vec![s("Item property association entries are out of order")])?;
        }
        last_id = id;
        i = pl::add(&i, &n(1));
    }
    pl::ret_undef()
}

/// Process item information now
/// Inputs: 0) ExifTool ref
pub fn handle_item_info(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    handle_item_info_impl(it, &arg(&a, 0))?;
    pl::ret_nothing(_w)
}

fn handle_item_info_impl(it: &mut Interp, et: &Value) -> ER<()> {
    let et = et.clone();
    let raf = et.get("RAF");
    let items = et.get("ItemInfo");
    let verbose = m(it, &et, "Options", vec![s("Verbose")])?;
    let mut buff: Value;

    // extract information from EXIF/XMP metadata items
    if items.truthy() && raf.truthy() {
        et.get_or_new_array("PATH").push(s("ItemInformation"));
        let cur_pos = raf_tell(it, &raf)?;
        let primary = et.get("PrimaryItem");
        m(it, &et, "VerboseDir", vec![s("Processing items from ItemInformation"), n(items.len() as i64)])?;
        for id in sorted_num_keys(&items) {
            let item = items.get(id.to_bytes());
            let mut typ = item.get("ContentType");
            if !typ.truthy() {
                typ = item.get("Type");
            }
            if !typ.truthy() {
                continue;
            }
            let extents = item.get("Extents");
            let has_extents = extents.truthy() && !extents.is_empty();
            if verbose.truthy() {
                // add up total length of this item for the verbose output
                let mut len = n(0);
                if has_extents {
                    for e in extents.items() {
                        len = pl::add(&len, &e.at(2));
                    }
                }
                let ce = item.get("ContentEncoding");
                let enc = if ce.truthy() { cat(&[&s(", "), &ce, &s(" encoded")]) } else { s("") };
                m(it, &et, "VPrint", vec![n(0), cat(&[&et.get("INDENT"), &s("Item "), &id, &s(") '"), &typ, &s("' ("), &len, &s(" bytes"), &enc, &s(")\n")])])?;
            }
            // get ExifTool name for this item
            let name = match &*typ.to_bytes() {
                b"Exif" => s("EXIF"),
                b"application/rdf+xml" => s("XMP"),
                b"jpeg" => s("PreviewImage"),
                b"uri " => s("PLIST"),
                _ => s(""),
            };
            let mut warn = Value::Undef;
            let ce = item.get("ContentEncoding");
            if ce.truthy() {
                if !ce.eq_str("deflate") {
                    // (other possible values are 'gzip' and 'compress', but I don't have samples of these)
                    warn = cat(&[&s("Can't currently decode "), &ce, &s(" encoded "), &typ, &s(" metadata")]);
                } else if pl::eval_block(it, |it| runtime::require(it, "Compress::Zlib"))?.is_none() {
                    warn = cat(&[&s("Install Compress::Zlib to decode deflated "), &typ, &s(" metadata")]);
                }
            }
            if item.get("ProtectionIndex").truthy() {
                warn = cat(&[&s("Can't currently decode protected "), &typ, &s(" metadata")]);
            }
            // Note: In HEIC's, these seem to indicate data in 'idat' instead of 'mdat'
            let mut const_meth = item.get("ConstructionMethod");
            if !const_meth.truthy() {
                const_meth = n(0);
            }
            if pl::gt(&const_meth, &n(1)) {
                warn = cat(&[&s("Can't currently extract "), &typ, &s(" with construction method "), &const_meth]);
            }
            if pl::num_eq(&const_meth, &n(1)) && !et.get("MediaDataInfo").truthy() {
                warn = cat(&[&s("No 'idat' for "), &typ, &s(" object with construction method 1")]);
            }
            if warn.truthy() && name.truthy() {
                m(it, &et, "Warn", vec![warn.clone()])?;
            }
            if item.get("DataReferenceIndex").truthy() {
                warn = s("Not this file"); // (can only extract from "this file")
            }
            if !(has_extents || warn.truthy()) {
                warn = cat(&[&s("No Extents for "), &typ, &s(" item")]);
                if name.truthy() {
                    m(it, &et, "Warn", vec![warn.clone()])?;
                }
            }
            if warn.truthy() {
                if pl::gt(&verbose, &n(2)) {
                    m(it, &et, "VPrint", vec![n(0), cat(&[&et.get("INDENT"), &s("    [not extracted]  ("), &warn, &s(")\n")])])?;
                }
                continue;
            }
            let bo = item.get("BaseOffset");
            let base = pl::add(&if bo.truthy() { bo } else { n(0) }, &if const_meth.truthy() { et.get("MediaDataInfo").at(0) } else { n(0) });
            if pl::gt(&verbose, &n(2)) {
                // do verbose hex dump
                let mut len = n(0);
                buff = Value::Undef;
                let mut val = s("");
                let max_len = if pl::gt(&verbose, &n(3)) { n(2048) } else { n(96) };
                for extent in extents.items() {
                    let nn = extent.at(2);
                    let mut more = pl::sub(&max_len, &len);
                    if pl::gt(&more, &n(0)) && nn.truthy() {
                        if pl::gt(&more, &nn) {
                            more = nn.clone();
                        }
                        if !buff.is_undef() {
                            val = pl::concat(&val, &buff);
                        }
                        if !raf_seek(it, &raf, pl::add(&extent.at(1), &base), n(0))? {
                            break;
                        }
                        if !raf_read(it, &raf, &mut buff, more)?.truthy() {
                            break;
                        }
                    }
                    len = pl::add(&len, &nn);
                }
                if !buff.is_undef() {
                    if !val.to_bytes().is_empty() {
                        buff = pl::concat(&val, &buff);
                    }
                    m(it, &et, "VerboseDump", vec![pl::sref(buff.clone()), s("DataPos"), pl::add(&extents.at(0).at(1), &base)])?;
                    let snip = pl::sub(&len, &n(buff.to_bytes().len() as i64));
                    if snip.truthy() {
                        m(it, &et, "VPrint", vec![n(0), cat(&[&et.get("INDENT"), &s("    [snip "), &snip, &s(" bytes]\n")])])?;
                    }
                }
            }
            // do hash of AVIF "av01" and HEIC image data
            if lex_hash(it, "isImageData").get(typ.to_bytes()).truthy() && et.get("ImageDataHash").truthy() {
                let mut tot = n(0);
                for extent in extents.items() {
                    if !raf_seek(it, &raf, pl::add(&extent.at(1), &base), n(0))? {
                        m(it, &et, "Warn", vec![cat(&[&s("Seek error in "), &typ, &s(" image data")])])?;
                        break;
                    }
                    let r = m(it, &et, "ImageDataHash", vec![raf.clone(), extent.at(2), cat(&[&typ, &s(" image")]), n(1)])?;
                    tot = pl::add(&tot, &r);
                }
                if tot.truthy() {
                    m(it, &et, "VPrint", vec![n(0), cat(&[&et.get("INDENT"), &s("(ImageDataHash: "), &tot, &s(" bytes of "), &typ, &s(" data)\n")])])?;
                }
            }
            if !name.truthy() {
                continue;
            }
            // assemble the data for this item
            buff = Value::Undef;
            let mut val = s("");
            for extent in extents.items() {
                if !buff.is_undef() {
                    val = pl::concat(&val, &buff);
                }
                if !raf_seek(it, &raf, pl::add(&extent.at(1), &base), n(0))? {
                    break;
                }
                if !raf_read(it, &raf, &mut buff, extent.at(2))?.truthy() {
                    break;
                }
            }
            if buff.is_undef() {
                continue;
            }
            if !val.to_bytes().is_empty() {
                buff = pl::concat(&val, &buff);
            }
            if buff.to_bytes().is_empty() {
                continue; // ignore empty directories
            }
            if item.get("ContentEncoding").truthy() {
                let inflate = pl::call(it, "Compress::Zlib::inflateInit", vec![])?;
                let (mut v2, mut stat) = (Value::Undef, Value::Undef);
                if inflate.truthy() {
                    let mut ab = [buff.clone()];
                    let r = pl::method_mut(it, &inflate, "inflate", &mut ab, Want::List)?;
                    v2 = arg(&r, 0);
                    stat = arg(&r, 1);
                }
                let z_stream_end = pl::call(it, "Compress::Zlib::Z_STREAM_END", vec![])?;
                if inflate.truthy() && pl::num_eq(&stat, &z_stream_end) {
                    buff = v2;
                    let len = n(buff.to_bytes().len() as i64);
                    m(it, &et, "VPrint", vec![n(0), cat(&[&et.get("INDENT"), &s("Inflated Item "), &id, &s(") '"), &typ, &s("' ("), &len, &s(" bytes)\n")])])?;
                    m(it, &et, "VerboseDump", vec![pl::sref(buff.clone())])?;
                } else {
                    let warn = cat(&[&s("Error inflating "), &name, &s(" metadata")]);
                    m(it, &et, "Warn", vec![warn.clone()])?;
                    if pl::gt(&verbose, &n(2)) {
                        m(it, &et, "VPrint", vec![n(0), cat(&[&et.get("INDENT"), &s("    [not extracted]  ("), &warn, &s(")\n")])])?;
                    }
                    continue;
                }
            }
            let mut start = Value::Undef;
            let sub_table;
            let mut proc = Value::Undef;
            let pos = pl::add(&extents.at(0).at(1), &base);
            if name.eq_str("EXIF") && buff.to_bytes().len() >= 4 {
                if re(r"^(MM\0\x2a|II\x2a\0)", "").is_match(&buff) {
                    m(it, &et, "Warn", vec![s("Missing Exif header")])?;
                } else if re(r"^Exif\0\0", "").is_match(&buff) {
                    // (haven't seen this yet, but it is just a matter of time
                    //  until someone screws it up like this)
                    m(it, &et, "Warn", vec![s("Missing Exif header size")])?;
                    start = n(6);
                } else {
                    let nn = pl::unpack1("N", &buff)?;
                    start = pl::add(&n(4), &nn); // skip "Exif\0\0" header if it exists
                    if pl::gt(&start, &n(buff.to_bytes().len() as i64)) {
                        m(it, &et, "Warn", vec![s("Invalid EXIF header")])?;
                        continue;
                    }
                    if et.get("HTML_DUMP").truthy() {
                        m(it, &et, "HDump", vec![pos.clone(), n(4), s("Exif header length"), cat(&[&s("Value: "), &nn])])?;
                        if nn.truthy() {
                            m(it, &et, "HDump", vec![pl::add(&pos, &n(4)), pl::sub(&start, &n(4)), s("Exif header")])?;
                        }
                    }
                }
                sub_table = get_tag_table(it, "Image::ExifTool::Exif::Main")?;
                proc = Value::Code(it.sub_ref("Image::ExifTool::ProcessTIFF"));
            } else if name.eq_str("PreviewImage") {
                // take a quick stab at determining the size of the image
                // (based on JPEG previews found in Fuji X-H2S HIF images)
                let mut typ = s("PreviewImage");
                if let Some(c) = re(r"^.{556}\xff\xc0\0\x11.(.{4})", "s").m(&buff) {
                    let hw = pl::unpack("n2", &cap(&c, 1))?;
                    let (h, w) = (arg(&hw, 0), arg(&hw, 1));
                    // (not sure if $h is ever the long dimension, but test it just in case)
                    if pl::num_eq(&w, &n(160)) || pl::num_eq(&h, &n(160)) {
                        typ = s("ThumbnailImage");
                    } else if pl::num_eq(&w, &n(1920)) || pl::num_eq(&h, &n(1920)) {
                        typ = s("OtherImage"); // (large preview)
                    } // (PreviewImage is 640x480)
                }
                m(it, &et, "FoundTag", vec![typ, buff.clone()])?;
                continue;
            } else if name.eq_str("PLIST") {
                // extract PLIST information from 'uri ' if available
                if !re(r"^bplist00", "").is_match(&buff) {
                    continue;
                }
                sub_table = get_tag_table(it, "Image::ExifTool::PLIST::Main")?;
            } else {
                sub_table = get_tag_table(it, "Image::ExifTool::XMP::Main")?;
            }
            if !start.truthy() {
                start = n(0);
            }
            let blen = n(buff.to_bytes().len() as i64);
            let dir_info = Value::new_hash();
            dir_info.set("DataPt", pl::sref(buff.clone()));
            dir_info.set("DataLen", blen.clone());
            dir_info.set("DirStart", start.clone());
            dir_info.set("DirLen", pl::sub(&blen, &start));
            dir_info.set("DataPos", pos.clone());
            dir_info.set("Base", pl::add(&pos, &start)); // (needed for HtmlDump and IsOffset tags in binary data)
            // handle processing of metadata for sub-documents
            let refers = item.get("RefersTo");
            if !primary.is_undef() && refers.truthy() && !refers.get(primary.to_bytes()).truthy() {
                // set document number if this doesn't refer to the primary document
                let dc = pl::add(&et.get("DOC_COUNT"), &n(1));
                et.set("DOC_COUNT", dc.clone());
                et.set("DOC_NUM", dc);
                // associate this document number with the lowest item index
                let lowest = sorted_num_keys(&refers).into_iter().next().unwrap_or_default();
                items.get_or_new_hash(lowest.to_bytes()).set("DocNum", et.get("DOC_NUM"));
            }
            m(it, &et, "ProcessDirectory", vec![dir_info, sub_table, proc])?;
            et.delete("DOC_NUM");
        }
        if !raf_seek(it, &raf, cur_pos, n(0))? {
            m(it, &et, "Warn", vec![s("Seek error")])?; // seek back to original position
        }
        let path = et.get("PATH");
        if let Value::ArrayRef(p) = &path {
            p.borrow_mut().pop();
        }
    }
    // process the item properties now that we should know their associations and document numbers
    let ipc = et.get("ItemPropertyContainer");
    if ipc.truthy() {
        let (dir_info, sub_table, proc) = (ipc.at(0), ipc.at(1), ipc.at(2));
        et.set("IsItemProperty", n(1)); // set item property flag
        m(it, &et, "ProcessDirectory", vec![dir_info, sub_table, proc])?;
        et.delete("ItemPropertyContainer");
        et.delete("IsItemProperty");
        et.delete("DOC_NUM");
    }
    et.delete("ItemInfo");
    et.delete("MediaDataInfo");
    Ok(())
}

/// Warn if ExtractEmbedded option isn't used
/// Inputs: 0) ExifTool ref
pub fn ee_warn(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(ee_warn_impl(it, &arg(&a, 0))?)
}

fn ee_warn_impl(it: &mut Interp, et: &Value) -> ER<Value> {
    m(it, et, "Warn", vec![s("The ExtractEmbedded option may find more tags in the media data"), n(3)])
}

/// Get quicktime format from flags word
/// Inputs: 0) quicktime atom flags, 1) data length
/// Returns: ExifTool format string
pub fn quick_time_format(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(quick_time_format_impl(&arg(&a, 0), &arg(&a, 1)))
}

fn quick_time_format_impl(flags: &Value, len: &Value) -> Value {
    let mut format = Value::Undef;
    let lk = |len: &Value, tbl: &[(&str, &str)]| -> Value {
        let k = len.to_bytes();
        tbl.iter().find(|(a, _)| a.as_bytes() == &k[..]).map(|(_, b)| s(b)).unwrap_or_default()
    };
    if pl::num_eq(flags, &n(0x15)) || pl::num_eq(flags, &n(0x16)) {
        format = lk(len, &[("1", "int8"), ("2", "int16"), ("4", "int32"), ("8", "int64")]);
        if format.truthy() {
            format = pl::concat(&format, &s(if pl::num_eq(flags, &n(0x15)) { "s" } else { "u" }));
        }
    } else if pl::num_eq(flags, &n(0x17)) {
        format = s("float");
    } else if pl::num_eq(flags, &n(0x18)) {
        format = s("double");
    } else if pl::num_eq(flags, &n(0x00)) {
        format = lk(len, &[("1", "int8u"), ("2", "int16u")]);
    }
    format
}

/// Process MPEG-4 MTDT atom (ref 11)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_meta_data(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let data = data_pt.deref();
    let dir_len = n(data.to_bytes().len() as i64);
    let verbose = m(it, &et, "Options", vec![s("Verbose")])?;
    if !pl::ge(&dir_len, &n(2)) {
        return ret(n(0));
    }
    let cnt = get16u(it, &data_pt, n(0))?;
    if verbose.truthy() {
        m(it, &et, "VerboseDir", vec![s("MetaData"), cnt.clone()])?;
    }
    let mut pos = n(2);
    let mut i = n(0);
    while pl::lt(&i, &cnt) {
        if pl::gt(&pl::add(&pos, &n(10)), &dir_len) {
            break;
        }
        let size = get16u(it, &data_pt, pos.clone())?;
        if pl::lt(&size, &n(10)) || pl::gt(&pl::add(&size, &pos), &dir_len) {
            break;
        }
        let tag = get32u(it, &data_pt, pl::add(&pos, &n(2)))?;
        let mut lang = get16u(it, &data_pt, pl::add(&pos, &n(6)))?;
        let enc = get16u(it, &data_pt, pl::add(&pos, &n(8)))?;
        let mut val = substr_w(it, &data_pt.deref(), &pl::add(&pos, &n(10)), Some(&pl::sub(&size, &n(10))))?;
        let mut tag_info = m(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
        if tag_info.truthy() {
            // convert language code to ASCII (ignore read-only bit)
            lang = unpack_lang_impl(&lang, &Value::Undef)?;
            // handle alternate languages
            if lang.truthy() {
                let lang_info = get_lang_info_qt_impl(it, &et, &tag_info, &lang)?;
                if lang_info.truthy() {
                    tag_info = lang_info;
                }
            }
            if verbose.truthy() {
                m(it, &et, "VerboseInfo", vec![tag.clone(), tag_info.clone(), s("Value"), val.clone(), s("DataPt"), data_pt.clone(), s("Start"), pl::add(&pos, &n(10)), s("Size"), pl::sub(&size, &n(10))])?;
            }
            // convert from UTF-16 BE if necessary
            if pl::num_eq(&enc, &n(1)) {
                val = m(it, &et, "Decode", vec![val, s("UTF16")])?;
            }
            if pl::num_eq(&enc, &n(0)) && tag_info.get("Unknown").truthy() {
                // binary data
                m(it, &et, "FoundTag", vec![tag_info.clone(), pl::sref(val)])?;
            } else {
                m(it, &et, "FoundTag", vec![tag_info.clone(), val])?;
            }
        }
        pos = pl::add(&pos, &size);
        i = pl::add(&i, &n(1));
    }
    ret(n(1))
}

/// Process sample description table
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
/// (ref https://developer.apple.com/library/content/documentation/QuickTime/QTFF/QTFFChap2/qtff2.html#//apple_ref/doc/uid/TP40000939-CH204-25691)
pub fn process_sample_desc(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let ds = dir_info.get("DirStart");
    let mut pos = if ds.truthy() { ds } else { n(0) };
    let dl = dir_info.get("DirLen");
    let dir_len = if dl.truthy() { dl } else { pl::sub(&n(data_pt.deref().to_bytes().len() as i64), &pos) };
    if pl::gt(&pl::add(&pos, &n(8)), &dir_len) {
        return ret(n(0));
    }

    let num = get32u(it, &data_pt, n(4))?; // get number of sample entries in table
    pos = pl::add(&pos, &n(8));
    let mut err = false;
    let mut i = n(0);
    while pl::lt(&i, &num) {
        // loop through sample entries
        if pl::gt(&pl::add(&pos, &n(8)), &dir_len) {
            err = true;
            break;
        }
        let size = get32u(it, &data_pt, pos.clone())?;
        if pl::gt(&pl::add(&pos, &size), &dir_len) {
            err = true;
            break;
        }
        dir_info.set("DirStart", pos.clone());
        dir_info.set("DirLen", size.clone());
        process_hybrid(it, vec![et.clone(), dir_info.clone(), tag_table_ptr.clone()], Want::Scalar)?;
        pos = pl::add(&pos, &size);
        i = pl::add(&i, &n(1));
    }
    if err && et.get("HandlerType").truthy() {
        let mut grp = et.get("SET_GROUP1");
        if !grp.truthy() {
            grp = dir_info.get("Parent");
        }
        if !grp.truthy() {
            grp = s("unknown");
        }
        m(it, &et, "Warn", vec![cat(&[&s("Truncated "), &et.get("HandlerType"), &s(" sample table for "), &grp])])?;
    }
    ret(n(1))
}

/// Process hybrid binary data + QuickTime container (ref PH)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_hybrid(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    // brute-force search for child atoms after first 8 bytes of binary data
    let data_pt = dir_info.get("DataPt");
    let ds = dir_info.get("DirStart");
    let dir_start = if ds.truthy() { ds } else { n(0) };
    let dl = dir_info.get("DirLen");
    let dir_len = if dl.truthy() { dl } else { pl::sub(&n(data_pt.deref().to_bytes().len() as i64), &dir_start) };
    let end = pl::add(&dir_start, &dir_len);
    let mut pos = pl::add(&dir_start, &n(8)); // skip length/version
    let mut try_ = pos.clone();
    let mut child_pos = Value::Undef;

    while pl::le(&pos, &pl::sub(&end, &n(8))) {
        let tag = substr_w(it, &data_pt.deref(), &pl::add(&try_, &n(4)), Some(&n(4)))?;
        // look only for well-behaved tag ID's
        if re(r"[^\w ]", "").is_match(&tag) {
            pos = pl::add(&pos, &n(1));
            try_ = pos.clone();
            continue;
        }
        let size = get32u(it, &data_pt, try_.clone())?;
        if pl::num_eq(&pl::add(&size, &try_), &end) {
            // the atom ends exactly at the end of the parent -- this must be it
            child_pos = pos.clone();
            dir_info.set("DirLen", pos.clone()); // the binary data ends at the first child atom
            break;
        }
        if pl::lt(&size, &n(8)) || pl::gt(&pl::add(&size, &try_), &pl::sub(&end, &n(8))) {
            pos = pl::add(&pos, &n(1));
            try_ = pos.clone(); // fail.  try next position
        } else {
            try_ = pl::add(&try_, &size); // could be another atom following this
        }
    }
    // process binary data
    dir_info.set("MixedTags", n(1)); // ignore non-integer tag ID's
    m(it, &et, "ProcessBinaryData", vec![dir_info.clone(), tag_table_ptr.clone()])?;
    // process child atoms if found
    if child_pos.truthy() {
        dir_info.set("DirStart", child_pos.clone());
        dir_info.set("DirLen", pl::sub(&end, &child_pos));
        process_mov(it, vec![et.clone(), dir_info.clone(), tag_table_ptr.clone()], Want::Scalar)?;
    }
    ret(n(1))
}

/// Process iTunes 'righ' atom (ref PH)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_rights(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let data_pos = dir_info.get("Base");
    let data = data_pt.deref();
    let dir_len = n(data.to_bytes().len() as i64);
    let opts = et.get("OPTIONS");
    let mut unknown = opts.get("Unknown");
    if !unknown.truthy() {
        unknown = opts.get("Verbose");
    }
    m(it, &et, "VerboseDir", vec![s("righ"), pl::div(&dir_len, &n(8))?])?;
    let mut pos = n(0);
    while pl::le(&pl::add(&pos, &n(8)), &dir_len) {
        let tag = substr_w(it, &data, &pos, Some(&n(4)))?;
        if tag.eq_str("\0\0\0\0") {
            break;
        }
        let mut val = substr_w(it, &data, &pl::add(&pos, &n(4)), Some(&n(4)))?;
        let mut tag_info = m(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
        if !tag_info.truthy() {
            if !unknown.truthy() {
                pos = pl::add(&pos, &n(8));
                continue;
            }
            let name = printable_tag_id_impl(&tag, &Value::Undef)?;
            tag_info = Value::new_hash();
            tag_info.set("Name", cat(&[&s("Unknown_"), &name]));
            tag_info.set("Description", cat(&[&s("Unknown "), &name]));
            tag_info.set("Unknown", n(1));
            add_tag_to_table(it, &tag_table_ptr, &tag, &tag_info)?;
        }
        if !tag_info.get("Format").truthy() {
            val = cat(&[&s("0x"), &pl::unpack1("H*", &val)?]);
        }
        m(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tag, val, s("DataPt"), data_pt.clone(), s("DataPos"), data_pos.clone(), s("Start"), pl::add(&pos, &n(4)), s("Size"), n(4)])?;
        pos = pl::add(&pos, &n(8));
    }
    ret(n(1))
}

/// Process Nextbase 'infi' atom (ref PH)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_nextbase(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_tbl) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let data = data_pt.deref();
    m(it, &et, "VerboseDir", vec![s("Nextbase"), Value::Undef, n(data.to_bytes().len() as i64)])?;
    for c in re(r"(.*?): +(.*)\x0d", "").each(&data) {
        let (id, val) = (cap(&c, 1), cap(&c, 2));
        if !tag_tbl.get(id.to_bytes()).truthy() {
            let info = Value::new_hash();
            info.set("Name", etc(it, "MakeTagName", vec![id.clone()])?);
            add_tag_to_table(it, &tag_tbl, &id, &info)?;
        }
        let len = n(val.to_bytes().len() as i64);
        m(it, &et, "HandleTag", vec![tag_tbl.clone(), id, val, s("Size"), len])?;
    }
    ret(n(1))
}

/// Process iTunes Encoding Params (ref PH)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_encoding_params(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let data = data_pt.deref();
    let dir_len = data.to_bytes().len() as i64;
    m(it, &et, "VerboseDir", vec![s("Encoding Params"), pl::div(&n(dir_len), &n(8))?])?;
    let mut pos = 0i64;
    while pos + 8 <= dir_len {
        let v = pl::unpack(&format!("x{pos}a4N"), &data)?;
        m(it, &et, "HandleTag", vec![tag_table_ptr.clone(), arg(&v, 0), arg(&v, 1)])?;
        pos += 8;
    }
    ret(n(1))
}

/// Read Meta Keys and add tags to ItemList table ('mdta' handler) (ref PH)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_keys(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| process_keys_impl(it, &a))
}

fn process_keys_impl(it: &mut Interp, a: &[Value]) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(a, 0), arg(a, 1), arg(a, 2));
    let data_pt = dir_info.get("DataPt");
    let data = data_pt.deref();
    let dir_len = data.to_bytes().len() as i64;
    let mut out = Value::Undef;
    if m(it, &et, "Options", vec![s("Verbose")])?.truthy() {
        m(it, &et, "VerboseDir", vec![s("Keys")])?;
        out = m(it, &et, "Options", vec![s("TextOut")])?;
    }
    let mut pos = 8i64;
    let mut index = 1i64;
    et.set("KeysCount", pl::add(&et.get("KeysCount"), &n(1))); // increment key count for this directory
    let item_list = get_tag_table(it, "Image::ExifTool::QuickTime::ItemList")?;
    let user_data = get_tag_table(it, "Image::ExifTool::QuickTime::UserData")?;
    while pos < dir_len - 4 {
        let len = pl::unpack1(&format!("x{pos}N"), &data)?;
        if pl::lt(&len, &n(8)) || pl::gt(&pl::add(&n(pos), &len), &n(dir_len)) {
            break;
        }
        tag_table_ptr.delete(n(index).to_bytes());
        let ns = substr_w(it, &data, &n(pos + 4), Some(&n(4)))?;
        let mut tag = substr_w(it, &data, &n(pos + 8), Some(&pl::sub(&len, &n(8))))?;
        re(r"\0.*", "s").subst(&mut tag, false, |_| Vec::new()); // truncate at null
        let full = tag.clone();
        if ns.eq_str("mdta") {
            re(r"^com\.(apple\.quicktime\.)?", "").subst(&mut tag, false, |_| Vec::new()); // remove apple quicktime domain
        }
        if !tag.truthy() {
            tag = cat(&[&s("Tag_"), &ns]);
        }
        let short = tag.clone();
        let mut tag_info;
        loop {
            tag_info = m(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
            if tag_info.truthy() {
                break;
            }
            // also try ItemList and UserData tables
            tag_info = m(it, &et, "GetTagInfo", vec![item_list.clone(), tag.clone()])?;
            if tag_info.truthy() {
                break;
            }
            tag_info = m(it, &et, "GetTagInfo", vec![user_data.clone(), tag.clone()])?;
            if tag_info.truthy() {
                break;
            }
            // (I have some samples where the tag is a reversed ItemList or UserData tag ID)
            if re(r"^\w{3}\xa9$", "").is_match(&tag) {
                tag = pl::pack("N", &[pl::unpack1("V", &tag)?])?;
                tag_info = m(it, &et, "GetTagInfo", vec![item_list.clone(), tag.clone()])?;
                if tag_info.truthy() {
                    break;
                }
                tag_info = m(it, &et, "GetTagInfo", vec![user_data.clone(), tag.clone()])?;
                break;
            }
            if tag.to_bytes() == full.to_bytes() {
                tag = short.clone();
                break;
            }
            tag = full.clone();
        }
        let mut new_info = Value::Undef;
        let mut msg = Value::Undef;
        if tag_info.truthy() {
            // copy tag information into new Keys tag
            new_info = Value::new_hash();
            for k in ["Name", "Format", "ValueConv", "ValueConvInv", "PrintConv", "PrintConvInv"] {
                new_info.set(k, tag_info.get(k));
            }
            let w = tag_info.get("Writable");
            new_info.set("Writable", if w.is_undef() { n(1) } else { w });
            new_info.set("SubDirectory", tag_info.get("SubDirectory"));
            let groups = tag_info.get("Groups");
            new_info.set("Groups", if groups.truthy() { groups.hash_copy() } else { Value::new_hash() });
            let ng = new_info.get("Groups");
            for g in ["0", "1", "2"] {
                if !ng.get(g).truthy() {
                    ng.set(g, tag_table_ptr.get("GROUPS").get(g));
                }
            }
            // set Keys group.  This is necessary for logic when reading the associated ItemList entry,
            // but note that the group name will be overridden by TAG_EXTRA G1 for tags in a track
            ng.set("1", s("Keys"));
        } else if re(r"^[-\w. ]+$", "").is_match(&tag) || re(r"\w{4}", "").is_match(&tag) {
            // create info for tags with reasonable id's
            let mut name = pl::ucfirst(&tag);
            pl::tr(&mut name, "-0-9a-zA-Z_. ", "", "dc");
            re(r"[. ]+(.?)", "").subst(&mut name, true, |c| cap(c, 1).to_bytes().to_ascii_uppercase());
            re(r"_([a-z])", "").subst(&mut name, true, |c| [b"_".to_vec(), cap(c, 1).to_bytes().to_ascii_uppercase()].concat());
            re(r"([a-z])_([A-Z])", "").subst(&mut name, true, |c| [cap(c, 1).to_bytes().to_vec(), cap(c, 2).to_bytes().to_vec()].concat());
            if name.to_bytes().len() < 2 {
                name = cat(&[&s("Tag_"), &name]);
            }
            new_info = Value::new_hash();
            new_info.set("Name", name);
            let g = Value::new_hash();
            g.set("1", s("Keys"));
            new_info.set("Groups", g);
            msg = s(" (Unknown)");
            m(it, &et, "VPrint", vec![n(0), et.get("INDENT"), cat(&[&s("[adding Keys:"), &tag, &s("]\n")])])?;
        }
        // substitute this tag in the ItemList table with the given index
        let id = cat(&[&et.get("KeysCount"), &s("."), &n(index)]);
        let old_info = item_list.get(id.to_bytes());
        if pl::ref_of(&old_info) == "HASH" {
            // delete other languages too if they exist
            let other = old_info.get("OtherLang");
            if other.truthy() {
                for k in other.items() {
                    item_list.delete(k.to_bytes());
                }
            }
            item_list.delete(id.to_bytes());
        }
        if new_info.truthy() {
            new_info.set("KeysID", tag.clone()); // save original ID for use in family 7 group name
            add_tag_to_table(it, &item_list, &id, &new_info)?;
            if !msg.truthy() {
                msg = s("");
            }
            if out.truthy() {
                let line = cat(&[&et.get("INDENT"), &s("Added ItemList Tag "), &id, &s(" = ("), &ns, &s(") "), &full, &msg, &s("\n")]);
                pl::print_to(it, Some(&out), &[line])?;
            }
        }
        pos += len.to_i64();
        index += 1;
    }
    ret(n(1))
}

/// Process keys in MetaSampleDesc directory
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_meta_keys(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    // save this information to decode timed metadata samples when ExtractEmbedded is used
    if et.get("OPTIONS").get("ExtractEmbedded").truthy() {
        pl::call(it, "Image::ExifTool::QuickTime::SaveMetaKeys", vec![et, dir_info, tag_table_ptr])?;
    }
    ret(n(1))
}

/// Identify trailers at specified offset from end of file
/// Inputs: 0) RAF reference, 1) Offset from end of file
/// Returns: Array ref to first trailer in linked list: 0) name of trailer,
///          1) absolute offset to start of this trailer, 2) trailer length,
///          3) ref to next trailer. Or undef if no trailer found, or error string on error
/// - file position is returned to its original location
pub fn identify_trailers(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(identify_trailers_impl(it, &arg(&a, 0))?)
}

fn identify_trailers_impl(it: &mut Interp, raf: &Value) -> ER<Value> {
    let (mut trailer, mut next_trail, mut buff) = (Value::Undef, Value::Undef, Value::Undef);
    let (mut typ, mut len);
    let pos = raf_tell(it, raf)?;
    let mut offset = n(0); // positive offset back from end of file
    while raf_seek(it, raf, pl::sub(&n(-40), &offset), n(2))? && pl::num_eq(&raf_read(it, raf, &mut buff, n(40))?, &n(40)) {
        if pl::substr(&buff, 8, None).eq_str("8db42d694ccc418790edff439fe026bf") {
            typ = s("Insta360");
            len = pl::unpack1("V", &buff)?;
        } else if re(r"\&\&\&\&(.{4})$", "").is_match(&buff) {
            typ = s("LigoGPS");
            len = get32u(it, &pl::sref(buff.clone()), n(36))?;
        } else if let Some(c) = re(r"~\0\x04\0zmie~\0\0\x06.{4}([\x10\x18])(\x04)$", "s").m(&buff).or_else(|| re(r"~\0\x04\0zmie~\0\0\x0a.{8}([\x10\x18])(\x08)$", "s").m(&buff)) {
            let old_order = scalar(part2::get_byte_order(it, vec![], Want::Scalar))?;
            set_byte_order(it, if cap(&c, 1).eq_str("\x10") { "MM" } else { "II" })?;
            typ = s("MIE");
            let bp = pl::sref(buff.clone());
            len = if cap(&c, 2).eq_str("\x04") { get32u(it, &bp, n(34))? } else { get64u(it, &bp, n(30))? };
            scalar(part2::set_byte_order(it, vec![old_order], Want::Scalar))?;
        } else {
            break;
        }
        // 0) trailer type, 1) trailer start, 2) trailer length, 3) pointer to next trailer
        let start = pl::sub(&raf_tell(it, raf)?, &len);
        trailer = Value::new_array(vec![typ, start, len.clone(), next_trail.clone()]);
        next_trail = trailer.clone();
        offset = pl::add(&offset, &len);
    }
    if !raf_seek(it, raf, pos, n(0))? {
        return Ok(s("Seek error"));
    }
    Ok(trailer)
}

/// `QuickTime::Func(ARGS)` (a sub of QuickTimeStream.pl, or autoloaded).
fn qt(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("{PKG}::{name}"), args)
}

/// `{ RAF => $raf, ... }`
fn hash_of(pairs: Vec<(&str, Value)>) -> Value {
    let h = Value::new_hash();
    for (k, v) in pairs {
        h.set(k, v);
    }
    h
}

/// Process a QuickTime atom
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) optional tag table ref
/// Returns: 1 on success
pub fn process_mov(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| process_mov_impl(it, &a))
}

fn process_mov_impl(it: &mut Interp, a: &[Value]) -> ER<Vec<Value>> {
    let (et, dir_info, mut tag_table_ptr) = (arg(a, 0), arg(a, 1), arg(a, 2));
    let mut raf = dir_info.get("RAF");
    let data_pt = dir_info.get("DataPt");
    let verbose = m(it, &et, "Options", vec![s("Verbose")])?;
    let validate = et.get("OPTIONS").get("Validate");
    let db = dir_info.get("Base");
    let dir_base = if db.truthy() { db } else { n(0) };
    let mut data_pos = dir_base.clone();
    let di = dir_info.get("DirID");
    let dir_id = if di.truthy() { di } else { s("") };
    let charset_quick_time = m(it, &et, "Options", vec![s("CharsetQuickTime")])?;
    let mut buff = Value::Undef;
    let (mut tag, mut size);
    let mut track = Value::Undef;
    let mut is_user_data = false;
    let triplet = Value::new_hash();
    let mut do_default_lang = false;
    let mut index = Value::Undef;
    let mut dir_end = Value::Undef;
    let mut unk_opt = Value::Undef;
    let mut save_options: Vec<(Value, Value)> = Vec::new();
    let mut atom_count = Value::Undef;
    let mut warn_str = Value::Undef;
    let mut trailer = Value::Undef;

    let top_level = !et.get("InQuickTime").truthy();
    et.set("InQuickTime", n(1));
    if top_level {
        et.set("MediaType", s(""));
        et.set("MetaFormat", s(""));
        et.set("HandlerType", s(""));
    }

    if et.get("KeysCount").is_undef() {
        et.set("KeysCount", n(0)); // initialize ItemList key directory count
        do_default_lang = true; // flag to generate default language tags
    }
    // more convenient to package data as a RandomAccess file
    if !raf.truthy() {
        raf = m(it, &s("File::RandomAccess"), "new", vec![data_pt.clone()])?;
        if dir_info.get("DirLen").truthy() {
            let ds = dir_info.get("DirStart");
            dir_end = pl::add(&pl::add(&data_pos, &dir_info.get("DirLen")), &if ds.truthy() { ds } else { n(0) });
        }
    }
    // skip leading bytes if necessary
    if dir_info.get("DirStart").truthy() {
        if !raf_seek(it, &raf, dir_info.get("DirStart"), n(1))? {
            return ret(n(0));
        }
        data_pos = pl::add(&data_pos, &dir_info.get("DirStart"));
    }
    // read size/tag name atom header
    if !pl::num_eq(&raf_read(it, &raf, &mut buff, n(8))?, &n(8)) {
        return ret(n(0));
    }
    data_pos = pl::add(&data_pos, &n(8));
    if tag_table_ptr.truthy() {
        is_user_data = tag_table_ptr.same_ref(&pkg_hash(it, "UserData"));
    } else {
        tag_table_ptr = get_tag_table(it, "Image::ExifTool::QuickTime::Main")?;
    }
    let st = pl::unpack("Na4", &buff)?;
    size = arg(&st, 0);
    tag = arg(&st, 1);
    let fs = et.get("OPTIONS").get("FastScan");
    let fast = if fs.truthy() { fs } else { n(0) };
    // check for Insta360, LIGOGPSINFO or MIE trailer
    if top_level && !fast.truthy() {
        trailer = identify_trailers_impl(it, &raf)?;
        if trailer.truthy() && !trailer.is_ref() {
            m(it, &et, "Warn", vec![trailer])?;
            return ret(n(0));
        }
    }
    if data_pt.truthy() {
        if verbose.truthy() {
            m(it, &et, "VerboseDir", vec![dir_info.get("DirName")])?;
        }
    } else {
        // check on file type if called with a RAF
        if !tag_table_ptr.get(tag.to_bytes()).truthy() {
            return ret(n(0));
        }
        let mut file_type = Value::Undef;
        if tag.eq_str("ftyp") && pl::ge(&size, &n(12)) {
            // read ftyp atom to see what type of file this is
            let sz8 = pl::sub(&size, &n(8));
            if pl::num_eq(&raf_read(it, &raf, &mut buff, sz8.clone())?, &sz8) {
                if !raf_seek(it, &raf, pl::neg(&sz8), n(1))? {
                    m(it, &et, "Warn", vec![s("Seek error")])?;
                    return ret(n(0));
                }
                let typ = substr_w(it, &buff, &n(0), Some(&n(4)))?;
                et.set("save_ftyp", typ.clone());
                // see if we know the extension for this file type
                let fl = lex_hash(it, "ftypLookup").get(typ.to_bytes());
                let fm = if fl.truthy() { re(r"\(\.(\w+)", "").m(&fl) } else { None };
                if let Some(c) = fm {
                    file_type = cap(&c, 1);
                // check compatible brands
                } else if re(r"^.{8}(.{4})+(mp41|mp42|avc1)", "s").is_match(&buff) {
                    file_type = s("MP4");
                } else if re(r"^.{8}(.{4})+(f4v )", "s").is_match(&buff) {
                    file_type = s("F4V");
                } else if re(r"^.{8}(.{4})+(qt  )", "s").is_match(&buff) {
                    file_type = s("MOV");
                }
            }
            if !file_type.truthy() {
                file_type = s("MP4"); // default to MP4
            }
            // set file type from extension if appropriate
            let ext = et.get("FILE_EXT");
            if ext.truthy() {
                let ue = lex_hash(it, "useExt").get(ext.to_bytes());
                if ue.truthy() && file_type.to_bytes() == ue.to_bytes() {
                    file_type = ext;
                }
            }
            let mime = lex_hash(it, "mimeLookup").get(file_type.to_bytes());
            m(it, &et, "SetFileType", vec![file_type.clone(), if mime.truthy() { mime } else { s("video/mp4") }])?;
            // temporarily set ExtractEmbedded option for CRX files
            if file_type.eq_str("CRX") {
                let old = m(it, &et, "Options", vec![s("ExtractEmbedded"), n(1)])?;
                save_options.push((s("ExtractEmbedded"), old));
            }
        } else {
            m(it, &et, "SetFileType", vec![])?; // MOV
        }
        set_byte_order(it, "MM")?;
        // have XMP take priority except for HEIC
        if !(file_type.truthy() && file_type.eq_str("HEIC")) {
            et.set("PRIORITY_DIR", s("XMP"));
        }
    }
    if fast.truthy() {
        raf.set("NoBuffer", n(1)); // disable buffering in FastScan mode
    }

    let eo = et.get("OPTIONS").get("ExtractEmbedded");
    let ee = if eo.truthy() { eo } else { n(0) };
    let hash = et.get("ImageDataHash");
    if ee.truthy() || hash.truthy() {
        unk_opt = et.get("OPTIONS").get("Unknown");
        runtime::require(it, "Image/ExifTool/QuickTimeStream.pl")?;
    }
    if tag_table_ptr.get("VARS").truthy() {
        index = tag_table_ptr.get("VARS").get("START_INDEX");
        atom_count = tag_table_ptr.get("VARS").get("ATOM_COUNT");
    }
    let mut last_tag = s("");
    let mut last_pos = n(0);
    loop {
        let mut ee_tag = false;
        let mut ignore = false;
        if !atom_count.is_undef() {
            atom_count = pl::sub(&atom_count, &n(1));
            if pl::lt(&atom_count, &n(0)) {
                break;
            }
        }
        if pl::lt(&size, &n(8)) {
            if pl::num_eq(&size, &n(0)) {
                if data_pt.truthy() {
                    // a zero size isn't legal for contained atoms, but Canon uses it to
                    // terminate the CNTH atom (eg. CanonEOS100D.mov), so tolerate it here
                    let pos = pl::sub(&raf_tell(it, &raf)?, &n(4));
                    if !raf_seek(it, &raf, n(0), n(2))? {
                        m(it, &et, "Warn", vec![s("Seek error")])?;
                        return ret(n(0));
                    }
                    let rem = pl::sub(&raf_tell(it, &raf)?, &pos);
                    let st = cat(&[&dir_info.get("DirName"), &s(" with "), &rem, &s(" bytes")]);
                    m(it, &et, "VPrint", vec![n(0), cat(&[&et.get("INDENT"), &s("[Terminator found in "), &st, &s(" remaining]")])])?;
                } else {
                    let t = printable_tag_id_impl(&tag, &n(2))?;
                    m(it, &et, "VPrint", vec![n(0), cat(&[&et.get("INDENT"), &s("Tag '"), &t, &s("' extends to end of file")])])?;
                    let tsize = cat(&[&tag, &s("-size")]);
                    if tag_table_ptr.get(tsize.to_bytes()).truthy() {
                        let pos = raf_tell(it, &raf)?;
                        if !fast.truthy() {
                            if !raf_seek(it, &raf, n(0), n(2))? {
                                m(it, &et, "Warn", vec![s("Seek error")])?;
                                return ret(n(0));
                            }
                            let sz = pl::sub(&raf_tell(it, &raf)?, &pos);
                            m(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tsize, sz])?;
                        }
                        let toff = cat(&[&tag, &s("-offset")]);
                        if tag_table_ptr.get(toff.to_bytes()).truthy() {
                            m(it, &et, "HandleTag", vec![tag_table_ptr.clone(), toff, pos])?;
                        }
                    }
                }
                break;
            }
            if !pl::num_eq(&size, &n(1)) {
                warn_str = s("Invalid atom size");
                break;
            }
            // read extended atom size
            if !pl::num_eq(&raf_read(it, &raf, &mut buff, n(8))?, &n(8)) {
                warn_str = s("Truncated atom header");
                break;
            }
            data_pos = pl::add(&data_pos, &n(8));
            let hl = pl::unpack("NN", &buff)?;
            let (hi, lo) = (arg(&hl, 0), arg(&hl, 1));
            if hi.truthy() || pl::gt(&lo, &n(0x7fffffff)) {
                if pl::gt(&hi, &n(0x7fffffff)) {
                    warn_str = s("Invalid atom size");
                    break;
                } else if !m(it, &et, "Options", vec![s("LargeFileSupport")])?.truthy() {
                    warn_str = s("End of processing at large atom (LargeFileSupport not enabled)");
                    break;
                } else if m(it, &et, "Options", vec![s("LargeFileSupport")])?.eq_str("2") {
                    m(it, &et, "Warn", vec![s("Processing large atom (LargeFileSupport is 2)")])?;
                }
            }
            size = pl::sub(&pl::add(&pl::mul(&hi, &n(4294967296)), &lo), &n(16));
            if pl::lt(&size, &n(0)) {
                warn_str = s("Invalid extended size");
                break;
            }
        } else {
            size = pl::sub(&size, &n(8));
        }
        if validate.truthy() {
            if tag.eq_str("wide") && size.truthy() {
                m(it, &et, "Warn", vec![s("Invalid 'wide' atom size")])?;
            }
            if !et.get("ValidatePath").truthy() {
                et.set("ValidatePath", Value::new_hash());
            }
            let mut parts = et.get("PATH").items();
            parts.push(tag.clone());
            let mut path = pl::join("-", &parts);
            let sg1 = et.get("SET_GROUP1");
            if sg1.truthy() {
                let rep = cat(&[&s("-"), &sg1, &s("-")]).to_bytes().to_vec();
                re(r"-Track-", "").subst(&mut path, false, |_| rep.clone());
            }
            if et.get("ValidatePath").get(path.to_bytes()).truthy() && !lex_hash(it, "dupTagOK").get(tag.to_bytes()).truthy() && !lex_hash(it, "dupDirOK").get(dir_id.to_bytes()).truthy() {
                let i = get32u(it, &pl::sref(tag.clone()), n(0))?;
                let st = if pl::lt(&i, &n(255)) { cat(&[&s("index "), &i]) } else { cat(&[&s("tag '"), &printable_tag_id_impl(&tag, &n(2))?, &s("'")]) };
                re(r"-[^-+]$", "").subst(&mut path, false, |_| Vec::new()); // remove tag name
                m(it, &et, "Warn", vec![cat(&[&s("Duplicate "), &st, &s(" at "), &path])])?;
                if path.eq_str("MOV-moov") {
                    et.set("ValidatePath", Value::new_hash()); // avoid warnings for all contained dups
                }
            }
            et.get("ValidatePath").set(path.to_bytes(), n(1));
        }
        if is_user_data && et.get("SET_GROUP1").truthy() {
            let tag_info = m(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
            if !(tag_info.truthy() && tag_info.get("SubDirectory").truthy()) {
                // add track name to UserData tags inside tracks
                tag = cat(&[&et.get("SET_GROUP1"), &tag]);
                if !tag_table_ptr.get(tag.to_bytes()).truthy() && tag_info.truthy() {
                    let new_info = tag_info.hash_copy();
                    for k in ["Name", "Description"] {
                        if !tag_info.get(k).truthy() {
                            continue;
                        }
                        let mut v = cat(&[&et.get("SET_GROUP1"), &tag_info.get(k)]);
                        re(r"^(Track\d+)Track", "").subst(&mut v, false, |c| cap(c, 1).to_bytes().to_vec()); // remove duplicate "Track" in name
                        new_info.set(k, v);
                    }
                    add_tag_to_table(it, &tag_table_ptr, &tag, &new_info)?;
                }
            }
        }
        // set flag to store additional information for ExtractEmbedded option
        let handler_type = et.get("HandlerType");
        let ee_box = pkg_hash(it, "eeBox").get(handler_type.to_bytes());
        let ee_box2 = lex_hash(it, "eeBox2").get(handler_type.to_bytes());
        let hash_box = lex_hash(it, "hashBox").get(handler_type.to_bytes());
        if ee_box.truthy() && ee_box.get(tag.to_bytes()).truthy() {
            if ee.truthy() || hash.truthy() {
                // (there is another 'gps ' box with a track log that doesn't contain offsets)
                if !tag.eq_str("gps ") || ee_box.get(tag.to_bytes()).to_bytes() == dir_id.to_bytes() {
                    ee_tag = true;
                    et.get("OPTIONS").set("Unknown", n(1)); // temporarily enable "Unknown" option
                }
            }
        } else if pl::gt(&ee, &n(1)) && ee_box2.truthy() && ee_box2.get(tag.to_bytes()).truthy() {
            ee_tag = true;
            et.get("OPTIONS").set("Unknown", n(1));
        } else if hash.truthy() && hash_box.truthy() && hash_box.get(tag.to_bytes()).truthy() {
            ee_tag = true;
            et.get("OPTIONS").set("Unknown", n(1));
        }
        let mut tag_info = m(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;

        if ee_tag {
            et.get("OPTIONS").set("Unknown", unk_opt.clone()); // restore Unknown option
        }

        // allow numerical tag ID's
        if !tag_info.truthy() {
            let id = cat(&[&et.get("KeysCount"), &s("."), &pl::unpack1("N", &tag)?]);
            if tag_table_ptr.get(id.to_bytes()).truthy() {
                tag_info = m(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), id.clone()])?;
                tag = id;
            }
        }
        // generate tagInfo if Unknown option set
        if tag_info.is_undef() && (et.get("OPTIONS").get("Unknown").truthy() || verbose.truthy() || re(r"^\xa9", "").is_match(&tag)) {
            let name = printable_tag_id_impl(&tag, &n(1))?;
            tag_info = Value::new_hash();
            if let Some(c) = re(r"^xa9(.*)", "").m(&name) {
                tag_info.set("Name", cat(&[&s("UserData_"), &cap(&c, 1)]));
                tag_info.set("Description", cat(&[&s("User Data "), &cap(&c, 1)]));
            } else {
                tag_info.set("Name", cat(&[&s("Unknown_"), &name]));
                tag_info.set("Description", cat(&[&s("Unknown "), &name]));
                let ui = lex_hash(it, "unknownInfo");
                for k in ui.keys() {
                    tag_info.set(&k[..], ui.get(&k[..]));
                }
            }
            add_tag_to_table(it, &tag_table_ptr, &tag, &tag_info)?;
        }
        // save required tag sizes
        let tsize = cat(&[&tag, &s("-size")]);
        if tag_table_ptr.get(tsize.to_bytes()).truthy() {
            m(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tsize, size.clone()])?;
            let toff = cat(&[&tag, &s("-offset")]);
            if tag_table_ptr.get(toff.to_bytes()).truthy() {
                let o = pl::add(&raf_tell(it, &raf)?, &dir_base);
                m(it, &et, "HandleTag", vec![tag_table_ptr.clone(), toff, o])?;
            }
        }
        // save position/size of 'idat'
        if tag.eq_str("idat") {
            let o = pl::add(&raf_tell(it, &raf)?, &dir_base);
            et.set("MediaDataInfo", Value::new_array(vec![o, size.clone()]));
        }
        // stop processing at mdat/idat if -fast2 is used
        if pl::gt(&fast, &n(1)) && (tag.eq_str("mdat") || (tag.eq_str("idat") && !pl::str_eq(&et.get("FileType"), "HEIC"))) {
            break;
        }
        // load values only if associated with a tag (or verbose) and not too big
        if pl::gt(&size, &n(0x2000000)) {
            // start to get worried above 32 MiB
            // check for RIFF trailer (written by Auto-Vox dashcam)
            if re(r"^(gpsa|gps0|gsen|gsea)...\0", "s").is_match(&buff) {
                // (yet seen only gpsa as first record)
                m(it, &et, "VPrint", vec![n(0), pl::sprintf("Found RIFF trailer at offset 0x%x", &[last_pos.clone()])])?;
                if ee.truthy() {
                    if !raf_seek(it, &raf, n(-8), n(1))? {
                        break; // seek back to start of trailer
                    }
                    let tbl = get_tag_table(it, "Image::ExifTool::QuickTime::Stream")?;
                    qt(it, "ProcessRIFFTrailer", vec![et.clone(), hash_of(vec![("RAF", raf.clone())]), tbl])?;
                } else {
                    ee_warn_impl(it, &et)?;
                }
                break;
            } else if buff.eq_str("CCCCCCCC") {
                m(it, &et, "VPrint", vec![n(0), pl::sprintf("Found Kenwood trailer at offset 0x%x", &[last_pos.clone()])])?;
                let tbl = get_tag_table(it, "Image::ExifTool::QuickTime::Stream")?;
                qt(it, "ProcessKenwoodTrailer", vec![et.clone(), hash_of(vec![("RAF", raf.clone())]), tbl])?;
                break;
            }
            if !tag_info.truthy() || tag_info.get("Unknown").truthy() {
                ignore = true;
            } else if pl::gt(&size, &n(0x8000000)) {
                let t = printable_tag_id_impl(&tag, &n(2))?;
                if m(it, &et, "Warn", vec![cat(&[&s("Skipping '"), &t, &s("' atom > 128 MiB")]), n(if ee_tag { 2 } else { 1 })])?.truthy() {
                    ignore = true;
                }
            } else if !ee_tag {
                let t = printable_tag_id_impl(&tag, &n(2))?;
                if m(it, &et, "Warn", vec![cat(&[&s("Skipping '"), &t, &s("' atom > 32 MiB")]), n(2)])?.truthy() {
                    ignore = true;
                }
            }
        }
        if !tag_info.is_undef() && !ignore && !(tag_info.truthy() && tag_info.get("DontRead").truthy()) {
            // set document number for this item property if necessary
            if et.get("IsItemProperty").truthy() {
                let items = et.get("ItemInfo");
                let (mut doc_num, mut lowest) = (Value::Undef, Value::Undef);
                let pr = et.get("PrimaryItem");
                let primary = if pr.truthy() { pr } else { n(0) };
                let pi = items.get(primary.to_bytes());
                let pitem = if pi.truthy() { pi } else { Value::new_hash() };
                if !pitem.get("RefersTo").truthy() {
                    pitem.set("RefersTo", Value::new_hash());
                }
                let mut ids = sorted_num_keys(&items);
                ids.reverse();
                'item_id: for id in ids {
                    if !items.get(id.to_bytes()).get("Association").truthy() {
                        continue;
                    }
                    let item = items.get(id.to_bytes());
                    for prop in item.get("Association").items() {
                        if !pl::num_eq(&prop, &index) {
                            continue;
                        }
                        let d = pkg_hash(it, "dontInherit").get(tag.to_bytes());
                        let dont = if d.truthy() { d } else { n(0) };
                        let refers = item.get("RefersTo");
                        if pl::num_eq(&id, &primary)
                            || (!dont.truthy() && (refers.truthy() && refers.get(primary.to_bytes()).truthy()))
                            // special case to assume this property belongs to the primary
                            // image if it belongs to an item referred to by the primary
                            || (!pl::num_eq(&dont, &n(1)) && pitem.get("RefersTo").get(id.to_bytes()).truthy())
                        {
                            // this is associated with the primary item or an item describing
                            // the primary item, so consider this part of the main document
                            doc_num = Value::Undef;
                            lowest = Value::Undef;
                            break 'item_id;
                        } else if item.get("DocNum").truthy() {
                            // this property is already associated with an item that has
                            // an ExifTool document number, so use the lowest associated DocNum
                            if doc_num.is_undef() || pl::gt(&doc_num, &item.get("DocNum")) {
                                doc_num = item.get("DocNum");
                            }
                        } else {
                            // keep track of the lowest associated item ID
                            lowest = id.clone();
                        }
                    }
                }
                if doc_num.is_undef() && !lowest.is_undef() {
                    // this is the first time we've seen metadata from this item,
                    // so use a new document number
                    doc_num = pl::add(&et.get("DOC_COUNT"), &n(1));
                    et.set("DOC_COUNT", doc_num.clone());
                    items.get_or_new_hash(lowest.to_bytes()).set("DocNum", doc_num.clone());
                }
                et.set("DOC_NUM", doc_num);
            }
            let mut val = Value::Undef;
            let missing = pl::sub(&size, &raf_read(it, &raf, &mut val, size.clone())?);
            if missing.truthy() {
                let t = printable_tag_id_impl(&tag, &n(2))?;
                warn_str = cat(&[&s("Truncated '"), &t, &s("' data (missing "), &missing, &s(" bytes)")]);
                break;
            }
            // use value to get tag info if necessary
            if !tag_info.truthy() {
                tag_info = m(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone(), pl::sref(val.clone())])?;
            }
            let has_data = dir_info.get("HasData").truthy() && re(r"^....data\0", "s").is_match(&val);
            if verbose.truthy() && !val.is_undef() && !has_data {
                let mut tval = Value::Undef;
                if tag_info.truthy() && tag_info.get("Format").truthy() {
                    let vl = n(val.to_bytes().len() as i64);
                    tval = read_value(it, &pl::sref(val.clone()), n(0), tag_info.get("Format"), tag_info.get("Count"), vl)?;
                }
                let fmt = if tag_info.truthy() { tag_info.get("Format") } else { Value::Undef };
                m(it, &et, "VerboseInfo", vec![tag.clone(), tag_info.clone(), s("Value"), tval, s("DataPt"), pl::sref(val.clone()), s("DataPos"), data_pos.clone(), s("Size"), size.clone(), s("Format"), fmt, s("Index"), index.clone()])?;
                // print iref item ID numbers
                if dir_id.eq_str("iref") {
                    let mut v: Vec<Value> = Vec::new();
                    if et.get("ItemRefVersion").truthy() {
                        if val.to_bytes().len() >= 10 {
                            v = pl::unpack("NnN*", &val)?;
                        }
                    } else if val.to_bytes().len() >= 6 {
                        v = pl::unpack("nnn*", &val)?;
                    }
                    let mut id = if v.is_empty() { Value::Undef } else { v.remove(0) };
                    let mut cnt = if v.is_empty() { Value::Undef } else { v.remove(0) };
                    let mut to = v;
                    if id.is_undef() {
                        id = s("<err>");
                        cnt = n(0);
                    }
                    if !pl::num_eq(&cnt, &count(&to)) {
                        id = cat(&[&id, &s(" (wrong count: "), &cnt, &s(")")]);
                    }
                    // convert sequential numbers to a range
                    let mut i = 1usize;
                    while i < to.len() {
                        let ok = match re(r"(\d+)$", "").m(&to[i - 1]) {
                            Some(c) => pl::num_eq(&to[i], &pl::add(&cap(&c, 1), &n(1))),
                            None => false,
                        };
                        if !ok {
                            i += 1;
                            continue;
                        }
                        let rep = cat(&[&s("-"), &to[i]]).to_bytes().to_vec();
                        re(r"(-.*)?$", "").subst(&mut to[i - 1], false, |_| rep.clone());
                        to.remove(i);
                    }
                    m(it, &et, "VPrint", vec![n(1), cat(&[&et.get("INDENT"), &s("  Item "), &id, &s(" refers to: ")]), pl::join(",", &to), s("\n")])?;
                }
            }
            // extract metadata from stream if ExtractEmbedded option is enabled
            if ee_tag {
                let vr = pl::sref(val.clone());
                qt(it, "ParseTag", vec![et.clone(), tag.clone(), vr.clone()])?;
                val = vr.deref();
                // forget this tag if we generated it only for ExtractEmbedded
                if tag_info.truthy() && tag_info.get("Unknown").truthy() && !unk_opt.truthy() {
                    tag_info = Value::Undef;
                }
            }

            // handle iTunesInfo mean/name/data triplets
            if tag_info.truthy() && tag_info.get("Triplet").truthy() {
                if tag.eq_str("data") && triplet.get("mean").truthy() && triplet.get("name").truthy() {
                    tag = triplet.get("name");
                    // add 'mean' to name unless it is 'com.apple.iTunes'
                    if !triplet.get("mean").eq_str("com.apple.iTunes") {
                        tag = cat(&[&triplet.get("mean"), &s("/"), &tag]);
                    }
                    tag_info = m(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone(), pl::sref(val.clone())])?;
                    if !tag_info.truthy() {
                        let mut name = triplet.get("name");
                        let mut desc = name.clone();
                        pl::tr(&mut name, "-_a-zA-Z0-9", "", "dc");
                        pl::tr(&mut desc, "_", " ", "");
                        tag_info = Value::new_hash();
                        tag_info.set("Name", name.clone());
                        tag_info.set("Description", desc);
                        m(it, &et, "VPrint", vec![n(0), et.get("INDENT"), cat(&[&s("[adding QuickTime:"), &name, &s("]\n")])])?;
                        add_tag_to_table(it, &tag_table_ptr, &tag, &tag_info)?;
                    }
                    // ignore 8-byte header
                    if val.to_bytes().len() >= 8 {
                        val = pl::substr(&val, 8, None);
                    }
                    if !(tag_info.get("Format").truthy() || tag_info.get("SubDirectory").truthy()) {
                        // extract as binary if it contains any non-ASCII or control characters
                        if re(r"[^\x20-\x7e]", "").is_match(&val) {
                            val = pl::sref(val.clone());
                        }
                    }
                    tag_info.set("List", n(1)); // (allow any of these tags to have multiple data elements)
                    if verbose.truthy() {
                        m(it, &et, "VerboseInfo", vec![tag.clone(), tag_info.clone(), s("Value"), val.clone()])?;
                    }
                } else {
                    if val.to_bytes().len() > 4 {
                        triplet.set(tag.to_bytes(), pl::substr(&val, 4, None));
                    }
                    tag_info = Value::Undef; // don't store this tag
                }
            }
            if tag_info.truthy() {
                let mut found: Vec<Value> = Vec::new();
                let subdir = tag_info.get("SubDirectory");
                if subdir.truthy() {
                    let st = subdir.get("Start");
                    let start = if st.truthy() { st } else { n(0) };
                    let (mut base, mut d_pos) = (data_pos.clone(), n(0));
                    if subdir.get("Base").truthy() {
                        let scope = runtime::file_scope(it, PKG);
                        let vars = [("et", et.clone()), ("dirInfo", dir_info.clone()), ("tagTablePtr", tag_table_ptr.clone()), ("raf", raf.clone()), ("dataPt", data_pt.clone()), ("tag", tag.clone()), ("size", size.clone()), ("tagInfo", tag_info.clone()), ("val", val.clone()), ("subdir", subdir.clone()), ("start", start.clone()), ("base", base.clone()), ("dPos", d_pos.clone()), ("dataPos", data_pos.clone())];
                        let (r, _) = pl::eval_site_in(it, &subdir.get("Base"), PKG, Some(scope), &vars, Want::Scalar);
                        d_pos = pl::sub(&d_pos, &r);
                        base = pl::sub(&base, &d_pos);
                    }
                    let sdn = subdir.get("DirName");
                    let sub_dir_info = hash_of(vec![
                        ("DataPt", pl::sref(val.clone())),
                        ("DataLen", size.clone()),
                        ("DirStart", start.clone()),
                        ("DirLen", pl::sub(&size, &start)),
                        ("DirName", if sdn.truthy() { sdn } else { tag_info.get("Name") }),
                        ("DirID", tag.clone()),
                        ("HasData", subdir.get("HasData")),
                        ("Multi", subdir.get("Multi")),
                        ("IgnoreProp", subdir.get("IgnoreProp")), // (XML hack)
                        ("DataPos", d_pos.clone()),
                        ("Base", base.clone()), // (needed for IsOffset tags in binary data)
                    ]);
                    if tag_info.get("BlockExtract").truthy() {
                        sub_dir_info.set("BlockInfo", tag_info.clone());
                    }
                    let bo = subdir.get("ByteOrder");
                    if bo.truthy() && re(r"^Little", "").is_match(&bo) {
                        set_byte_order(it, "II")?;
                    }
                    let old_group1 = et.get("SET_GROUP1");
                    let sd = tag_info.get("SubDirectory");
                    if sd.truthy() && sd.get("TagTable").truthy() && sd.get("TagTable").eq_str("Image::ExifTool::QuickTime::Track") {
                        if !track.truthy() {
                            track = n(0);
                        }
                        track = pl::add(&track, &n(1));
                        et.set("SET_GROUP1", cat(&[&s("Track"), &track]));
                    }
                    let sub_table = etc(it, "GetTagTable", vec![subdir.get("TagTable")])?;
                    let mut proc = subdir.get("ProcessProc");
                    // make ProcessMOV() the default processing procedure for subdirectories
                    if !(proc.truthy() || sub_table.get("PROCESS_PROC").truthy()) {
                        proc = Value::Code(it.sub_ref("Image::ExifTool::QuickTime::ProcessMOV"));
                    }
                    if pl::gt(&size, &start) {
                        // delay processing of ipco box until after all other boxes
                        if tag.eq_str("ipco") && !et.get("IsItemProperty").truthy() {
                            et.set("ItemPropertyContainer", Value::new_array(vec![sub_dir_info.clone(), sub_table.clone(), proc.clone()]));
                            m(it, &et, "VPrint", vec![n(0), cat(&[&et.get("INDENT"), &s("[Process ipco box later]")])])?;
                        } else if pl::lt(&fast, &n(2)) || !tag_info.get("MakerNotes").truthy() {
                            m(it, &et, "ProcessDirectory", vec![sub_dir_info.clone(), sub_table.clone(), proc.clone()])?;
                        }
                    }
                    if tag.eq_str("stbl") {
                        // process sample data when exiting SampleTable box if extracting embedded
                        if ee.truthy() || hash.truthy() {
                            qt(it, "ProcessSamples", vec![et.clone()])?;
                        }
                    } else if tag.eq_str("minf") {
                        et.set("HandlerType", s("")); // reset handler type at end of media info box
                    }
                    et.set("SET_GROUP1", old_group1);
                    set_byte_order(it, "MM")?;
                } else if has_data {
                    // handle atoms containing 'data' tags
                    // (currently ignore contained atoms: 'itif', 'name', etc.)
                    let mut pos = n(0);
                    let string_encoding = pkg_hash(it, "stringEncoding");
                    let qt_flags = lex_hash(it, "qtFlags");
                    loop {
                        if pl::gt(&pl::add(&pos, &n(16)), &size) {
                            break;
                        }
                        let u = pl::unpack(&format!("x{}Na4Nnn", pos.to_i64()), &val)?;
                        let (mut len, typ, flags, ctry, lang) = (arg(&u, 0), arg(&u, 1), arg(&u, 2), arg(&u, 3), arg(&u, 4));
                        if pl::gt(&pl::add(&pos, &len), &size) || !len.truthy() {
                            break;
                        }
                        let mut value = Value::Undef;
                        let mut lang_info = Value::Undef;
                        let mut old_dir = Value::Undef;
                        let mut format = tag_info.get("Format");
                        if typ.eq_str("data") && pl::ge(&len, &n(16)) {
                            pos = pl::add(&pos, &n(16));
                            len = pl::sub(&len, &n(16));
                            value = substr_w(it, &val, &pos, Some(&len))?;
                            // format flags (ref 12):
                            // 0x0=binary, 0x1=UTF-8, 0x2=UTF-16, 0x3=ShiftJIS,
                            // 0x4=UTF-8  0x5=UTF-16, 0xd=JPEG, 0xe=PNG,
                            // 0x15=signed int, 0x16=unsigned int, 0x17=float,
                            // 0x18=double, 0x1b=BMP, 0x1c='meta' atom
                            let se = string_encoding.get(flags.to_bytes());
                            if se.truthy() {
                                // handle all string formats
                                value = m(it, &et, "Decode", vec![value, se])?;
                                // (shouldn't be null terminated, but some software writes it anyway)
                                if !tag_info.get("Binary").truthy() {
                                    re(r"\0$", "").subst(&mut value, false, |_| Vec::new());
                                }
                            } else {
                                if !format.truthy() {
                                    format = quick_time_format_impl(&flags, &len);
                                } else if let Some(c) = re(r"^int\d+([us])$", "").m(&format) {
                                    // adjust integer to available length (but not int64)
                                    let fmt = match &*len.to_bytes() {
                                        b"1" => Some("int8"),
                                        b"2" => Some("int16"),
                                        b"4" => Some("int32"),
                                        _ => None,
                                    };
                                    if let Some(f) = fmt {
                                        format = cat(&[&s(f), &cap(&c, 1)]);
                                    }
                                }
                                if format.truthy() {
                                    value = read_value(it, &pl::sref(value.clone()), n(0), format.clone(), tag_info.get("Count"), len.clone())?;
                                } else if !tag_info.get("ValueConv").truthy() {
                                    // make binary data a scalar reference unless a ValueConv exists
                                    value = pl::sref(value.clone());
                                }
                            }
                        }
                        if ctry.truthy() || lang.truthy() {
                            let lang_code = get_lang_code_impl(&lang, &ctry, &Value::Undef)?;
                            if lang_code.truthy() {
                                // get tagInfo for other language
                                lang_info = get_lang_info_qt_impl(it, &et, &tag_info, &lang_code)?;
                                // save other language tag ID's so we can delete later if necessary
                                if lang_info.truthy() {
                                    if !tag_info.get("OtherLang").truthy() {
                                        tag_info.set("OtherLang", Value::new_array(vec![]));
                                    }
                                    tag_info.get("OtherLang").push(lang_info.get("TagID"));
                                }
                            }
                        }
                        if !lang_info.truthy() {
                            lang_info = tag_info.clone();
                        }
                        let qf = qt_flags.get(flags.to_bytes());
                        let st = if qf.truthy() { cat(&[&s(" ("), &qf, &s(")")]) } else { s("") };
                        if verbose.truthy() {
                            let efmt = cat(&[&s(", Type='"), &typ, &s("', Flags=0x%x%s, Lang=0x%.4x")]);
                            let extra = pl::sprintf_v(&efmt, &[flags.clone(), st, lang.clone()]);
                            let vv = if value.is_ref() { value.deref() } else { value.clone() };
                            m(it, &et, "VerboseInfo", vec![tag.clone(), lang_info.clone(), s("Value"), vv, s("DataPt"), pl::sref(val.clone()), s("DataPos"), data_pos.clone(), s("Start"), pos.clone(), s("Size"), len.clone(), s("Format"), format.clone(), s("Index"), index.clone(), s("Extra"), extra])?;
                        }
                        if !value.is_undef() {
                            // use "Keys" in path instead of ItemList if this was defined by a Keys tag
                            // (the only reason for this is to have "Keys" in the family 5 group name)
                            // Note that the Keys group is specifically set by the ProcessKeys routine,
                            // even though this tag would be in the ItemList table
                            let g = tag_info.get("Groups");
                            let is_keys = g.truthy() && g.get("1").truthy() && g.get("1").eq_str("Keys");
                            let path = et.get("PATH");
                            if is_keys {
                                old_dir = path.at(-1);
                                let l = path.len();
                                if l > 0 {
                                    path.set_at(l - 1, s("Keys"));
                                }
                            }
                            found.extend(pl::method_list(it, &et, "FoundTag", vec![lang_info.clone(), value.clone()])?);
                            if is_keys {
                                let l = path.len();
                                if l > 0 {
                                    path.set_at(l - 1, old_dir.clone());
                                }
                            }
                        }
                        pos = pl::add(&pos, &len);
                    }
                } else if re(r"^\xa9", "").is_match(&tag) || tag_info.get("IText").truthy() {
                    // parse international text to extract all languages
                    let mut pos = n(0);
                    if tag_info.get("Format").truthy() {
                        let vl = n(val.to_bytes().len() as i64);
                        // (ReadValue in list context, as an argument list)
                        let rv = part2::read_value(it, vec![pl::sref(val.clone()), n(0), tag_info.get("Format"), Value::Undef, vl], Want::List)?;
                        let mut fa = vec![tag_info.clone()];
                        fa.extend(rv);
                        found.extend(pl::method_list(it, &et, "FoundTag", fa)?);
                        pos = size.clone();
                    }
                    let itext = tag_info.get("IText");
                    loop {
                        let (mut len, mut lang);
                        if itext.truthy() && pl::ge(&itext, &n(6)) {
                            if pl::gt(&pl::add(&pos, &itext), &size) {
                                break;
                            }
                            pos = pl::add(&pos, &pl::sub(&itext, &n(2)));
                            lang = pl::unpack1(&format!("x{}n", pos.to_i64()), &val)?;
                            pos = pl::add(&pos, &n(2));
                            len = pl::sub(&size, &pos);
                        } else {
                            if pl::gt(&pl::add(&pos, &n(4)), &size) {
                                break;
                            }
                            let u = pl::unpack(&format!("x{}nn", pos.to_i64()), &val)?;
                            len = arg(&u, 0);
                            lang = arg(&u, 1);
                            pos = pl::add(&pos, &n(4));
                            // according to the QuickTime spec (ref 12), $len should include
                            // 4 bytes for length and type words, but nobody (including
                            // Apple, Pentax and Kodak) seems to add these in, so try
                            // to allow for either
                            if pl::gt(&pl::add(&pos, &len), &size) {
                                len = pl::sub(&len, &n(4));
                                if pl::gt(&pl::add(&pos, &len), &size) || pl::lt(&len, &n(0)) {
                                    break;
                                }
                            }
                        }
                        // ignore any empty entries (or null padding) after the first
                        if !len.truthy() && pos.truthy() {
                            continue;
                        }
                        let mut st = substr_w(it, &val, &pos, Some(&len))?;
                        let mut lang_info = Value::Undef;
                        let mut enc = Value::Undef;
                        if (pl::lt(&lang, &n(0x400)) || pl::num_eq(&lang, &n(0x7fff))) && !re(r"^\xfe\xff", "").is_match(&st) {
                            // this is a Macintosh language code
                            // a language code of 0 is Macintosh english, so treat as default
                            if lang.truthy() {
                                if pl::num_eq(&lang, &n(0x7fff)) {
                                    // technically, ISO 639-2 doesn't have a 2-character
                                    // equivalent for 'und', but use 'un' anyway
                                    lang = s("un");
                                } else {
                                    // use Font.pm to look up language string
                                    runtime::require(it, "Image::ExifTool::Font")?;
                                    lang = pl::pkg_hash(it, "Image::ExifTool::Font::ttLang").get("Macintosh").get(lang.to_bytes());
                                }
                            } else {
                                // for the default language code of 0x0000, use UTF-8 instead
                                // of the CharsetQuickTime setting if obviously UTF8
                                if pl::gt(&etc(it, "IsUTF8", vec![pl::sref(st.clone())])?, &n(0)) {
                                    enc = s("UTF8");
                                }
                            }
                            // the spec says only "Macintosh text encoding", but
                            // allow this to be configured by the user
                            if !enc.truthy() {
                                enc = charset_quick_time.clone();
                            }
                        } else {
                            // convert language code to ASCII (ignore read-only bit)
                            lang = unpack_lang_impl(&lang, &Value::Undef)?;
                            // may be either UTF-8 or UTF-16BE
                            enc = if re(r"^\xfe\xff", "").subst(&mut st, false, |_| Vec::new()) > 0 { s("UTF16") } else { s("UTF8") };
                        }
                        if !tag_info.get("NoDecode").truthy() {
                            st = m(it, &et, "Decode", vec![st, enc])?;
                            re(r"\0+$", "").subst(&mut st, false, |_| Vec::new()); // remove any trailing nulls (eg. 3gp tags)
                        }
                        if itext.truthy() && pl::gt(&itext, &n(6)) {
                            let nn = pl::sub(&itext, &n(6));
                            // add back extra bytes (eg. 'rtng' box)
                            let extra = substr_w(it, &val, &pl::sub(&pl::sub(&pos, &nn), &n(2)), Some(&nn))?;
                            st = pl::concat(&extra, &st);
                        }
                        if lang.truthy() {
                            lang_info = get_lang_info_qt_impl(it, &et, &tag_info, &lang)?;
                        }
                        let ti = if lang_info.truthy() { lang_info } else { tag_info.clone() };
                        found.extend(pl::method_list(it, &et, "FoundTag", vec![ti, st])?);
                        pos = pl::add(&pos, &len);
                    }
                } else {
                    let format = tag_info.get("Format");
                    if format.truthy() {
                        let vl = n(val.to_bytes().len() as i64);
                        val = read_value(it, &pl::sref(val.clone()), n(0), format.clone(), tag_info.get("Count"), vl)?;
                    }
                    let mut old_base = Value::Undef;
                    if tag_info.get("SetBase").truthy() {
                        old_base = et.get("BASE");
                        et.set("BASE", data_pos.clone());
                    }
                    let key = m(it, &et, "FoundTag", vec![tag_info.clone(), val.clone()])?;
                    found.push(key.clone());
                    if !old_base.is_undef() {
                        et.set("BASE", old_base);
                    }
                    // decode if necessary (NOTE: must be done after RawConv)
                    let vh = et.get("VALUE");
                    if !key.is_undef()
                        && (!format.truthy() || re(r"^string", "").is_match(&format))
                        && !tag_info.get("Unknown").truthy()
                        && !tag_info.get("ValueConv").truthy()
                        && !tag_info.get("Binary").truthy()
                        && !vh.get(key.to_bytes()).is_undef()
                        && !val.is_ref()
                    {
                        let vp = pl::elem_ref(&vh, &key.to_bytes());
                        let cur = vp.deref();
                        if !cur.is_ref() && cur.to_bytes().len() <= 65536 && re(r"[\x80-\xff]", "").is_match(&cur) {
                            // the encoding of this is not specified, so use CharsetQuickTime
                            // unless the string is valid UTF-8
                            let enc = if pl::gt(&etc(it, "IsUTF8", vec![vp.clone()])?, &n(0)) { s("UTF8") } else { charset_quick_time.clone() };
                            let cur = vp.deref();
                            let d = m(it, &et, "Decode", vec![cur, enc])?;
                            pl::set_deref(&vp, d)?;
                        }
                    }
                }
                // tweak family 1 group names for Keys/ItemList/UserData tags in a track
                if et.get("SET_GROUP1").truthy() && (dir_id.eq_str("ilst") || dir_id.eq_str("udta")) && !found.is_empty() {
                    let typ = lex_hash(it, "trackPath").get(pl::join("-", &et.get("PATH").items()).to_bytes());
                    if typ.truthy() {
                        let av = pkg_hash(it, "avType").get(et.get("MediaType").to_bytes());
                        let grp = cat(&[&if av.truthy() { av } else { et.get("SET_GROUP1") }, &typ]);
                        for f in &found {
                            if !f.is_undef() {
                                m(it, &et, "SetGroup", vec![f.clone(), grp.clone()])?;
                            }
                        }
                    }
                }
            }
        } else {
            if verbose.truthy() {
                let extra = pl::sprintf(" at offset 0x%.4x", &[raf_tell(it, &raf)?]);
                m(it, &et, "VerboseInfo", vec![tag.clone(), tag_info.clone(), s("Size"), size.clone(), s("Extra"), extra])?;
            }
            let seek_to = pl::add(&raf_tell(it, &raf)?, &size);
            if tag_info.truthy() && tag_info.get("DontRead").truthy() && tag_info.get("SubDirectory").truthy() {
                // ignore first trailer if it is the payload of this box
                if trailer.truthy() && pl::num_eq(&trailer.at(1), &raf_tell(it, &raf)?) {
                    trailer = trailer.at(3);
                }
                let subdir = tag_info.get("SubDirectory");
                let sub_dir_info = hash_of(vec![("RAF", raf.clone()), ("DirName", tag_info.get("Name")), ("DirID", tag.clone()), ("DirEnd", seek_to.clone())]);
                let sub_table = etc(it, "GetTagTable", vec![subdir.get("TagTable")])?;
                let mut proc = subdir.get("ProcessProc");
                // make ProcessMOV() the default processing procedure for subdirectories
                if !(proc.truthy() || sub_table.get("PROCESS_PROC").truthy()) {
                    proc = Value::Code(it.sub_ref("Image::ExifTool::QuickTime::ProcessMOV"));
                }
                m(it, &et, "ProcessDirectory", vec![sub_dir_info, sub_table, proc])?;
                m(it, &raf, "Seek", vec![seek_to.clone()])?;
            }
            let ok = m(it, &raf, "Seek", vec![pl::sub(&seek_to, &n(1))])?.truthy() &&pl::num_eq(&raf_read(it, &raf, &mut buff, n(1))?, &n(1));
            if !ok {
                if re(r"^<b[r>]", "").is_match(&pl::pack("N", &[size.clone()])?) {
                    // check for corrupted HEIC file downloaded from heic.digital
                    warn_str = pl::sprintf("Extraneous HTML text appended to file at offset 0x%x", &[last_pos.clone()]);
                } else {
                    let t = printable_tag_id_impl(&tag, &n(2))?;
                    warn_str = pl::sprintf_v(&cat(&[&s("Truncated '"), &t, &s("' data at offset 0x%x")]), &[last_pos.clone()]);
                }
                break;
            }
        }
        if tag.eq_str("trak") {
            et.set("MediaType", s("")); // reset track type at end of track
        }
        data_pos = pl::add(&data_pos, &pl::add(&size, &n(8))); // point to start of next atom data
        if dir_end.truthy() && pl::ge(&data_pos, &dir_end) {
            break; // (note: ignores last value if 0 bytes)
        }
        last_pos = pl::add(&raf_tell(it, &raf)?, &dir_base);
        if trailer.truthy() && pl::ge(&last_pos, &trailer.at(1)) {
            let msg = pl::sprintf("%s trailer at offset 0x%x (%d bytes)", &[trailer.at(0), trailer.at(1), trailer.at(2)]);
            m(it, &et, "Warn", vec![msg, n(1)])?;
            break;
        }
        if !pl::num_eq(&raf_read(it, &raf, &mut buff, n(8))?, &n(8)) {
            break;
        }
        if tag_table_ptr.get(tag.to_bytes()).truthy() && !tag.eq_str("free") {
            last_tag = tag.clone(); // (Insta360 sometimes puts free block before trailer)
        }
        let st = pl::unpack("Na4", &buff)?;
        size = arg(&st, 0);
        tag = arg(&st, 1);
        if !index.is_undef() {
            index = pl::add(&index, &n(1));
        }
    }
    if warn_str.truthy() {
        // assume this is an unknown trailer if it comes immediately after
        // mdat or moov and has a tag name we don't recognize
        let te = tag_table_ptr.get(tag.to_bytes());
        if (last_tag.eq_str("mdat") || last_tag.eq_str("moov")) && (!te.truthy() || (pl::ref_of(&te) == "HASH" && te.get("Unknown").truthy())) {
            m(it, &et, "Warn", vec![cat(&[&s("Unknown trailer with "), &pl::lcfirst(&warn_str)])])?;
        } else {
            m(it, &et, "Warn", vec![warn_str])?;
        }
    }
    // tweak file type based on track content ("iso*" and "dash" ftyp only ["mp42" added in 13.39])
    if top_level && et.get("FileType").truthy() && et.get("FileType").eq_str("MP4") && et.get("save_ftyp").truthy() && et.get("HasHandler").truthy() && re(r"^(iso|dash|mp42)", "").is_match(&et.get("save_ftyp")) && et.get("HasHandler").get("soun").truthy() && !et.get("HasHandler").get("vide").truthy() {
        m(it, &et, "OverrideFileType", vec![s("M4A"), s("audio/mp4")])?;
    }
    // fill in missing defaults for alternate language tags
    // (the first language is taken as the default)
    if do_default_lang && et.get("QTLang").truthy() {
        'qt_lang: for tag in et.get("QTLang").items() {
            if et.get("VALUE").get(tag.to_bytes()).is_undef() {
                continue;
            }
            let lang_info = et.get("TAG_INFO").get(tag.to_bytes());
            if !lang_info.truthy() {
                continue;
            }
            let tag_info = lang_info.get("SrcTagInfo");
            if !tag_info.truthy() {
                continue;
            }
            let info_hash = et.get("TAG_INFO");
            let name = tag_info.get("Name");
            // loop through all instances of this tag name and generate the default-language
            // version only if we don't already have a QuickTime tag with this name
            let mut i = 0i64;
            let mut key = name.clone();
            while info_hash.get(key.to_bytes()).truthy() {
                if m(it, &et, "GetGroup", vec![key.clone(), n(0)])?.eq_str("QuickTime") {
                    continue 'qt_lang;
                }
                i += 1;
                key = cat(&[&name, &s(&format!(" ({i})"))]);
            }
            let old_raw_conv = tag_info.get("RawConv");
            if !old_raw_conv.is_undef() {
                tag_info.delete("RawConv"); // (avoid doing RawConv twice)
            }
            let key = m(it, &et, "FoundTag", vec![tag_info.clone(), et.get("VALUE").get(tag.to_bytes())])?;
            if !old_raw_conv.is_undef() {
                tag_info.set("RawConv", old_raw_conv);
            }
            // copy extra tag information (groups, etc) to the synthetic tag
            let te = et.get_or_new_hash("TAG_EXTRA");
            te.set(key.to_bytes(), te.get(tag.to_bytes()));
            m(it, &et, "VPrint", vec![n(0), cat(&[&s("(synthesized default-language tag for QuickTime:"), &tag_info.get("Name"), &s(")")])])?;
        }
        et.delete("QTLang");
    }
    // process item information now that we are done processing its 'meta' container
    if top_level || dir_id.eq_str("meta") {
        handle_item_info_impl(it, &et)?;
    }

    // process linked list of trailers
    while trailer.truthy() {
        if pl::gt(&last_pos, &trailer.at(1)) {
            trailer = trailer.at(3); // skip if we have already processed this as an atom
            continue;
        }
        if !raf_seek(it, &raf, trailer.at(1), n(0))? {
            break;
        }
        let ligo = trailer.at(0).eq_str("LigoGPS") && pl::num_eq(&raf_read(it, &raf, &mut buff, n(8))?, &n(8)) && re(r"skip$", "i").is_match(&buff);
        if ligo {
            if !ee.truthy() {
                m(it, &et, "Warn", vec![s("Use the ExtractEmbedded option to decode timed GPS"), n(3)])?;
                trailer = trailer.at(3);
                continue;
            }
            let len = pl::sub(&get32u(it, &pl::sref(buff.clone()), n(0))?, &n(16));
            let ok = pl::gt(&len, &n(0)) && pl::num_eq(&raf_read(it, &raf, &mut buff, len.clone())?, &len) && re(r"^LIGOGPSINFO\0", "").is_match(&buff);
            if ok {
                let tbl = get_tag_table(it, "Image::ExifTool::QuickTime::Stream")?;
                let bp = pl::sref(buff.clone());
                let di = hash_of(vec![("DataPt", bp.clone()), ("DataPos", pl::add(&trailer.at(1), &n(8))), ("DirName", s("LigoGPSTrailer"))]);
                m(it, &et, "VerboseDump", vec![bp, s("DataPos"), di.get("DataPos")])?;
                pl::call(it, "Image::ExifTool::LigoGPS::ProcessLigoGPS", vec![et.clone(), di, tbl])?;
            } else {
                m(it, &et, "Warn", vec![s("Unrecognized data in LigoGPS trailer")])?;
            }
        } else if trailer.at(0).eq_str("Insta360") && ee.truthy() {
            // process Insta360 trailer if it exists
            if !raf_seek(it, &raf, n(0), n(2))? {
                m(it, &et, "Warn", vec![s("Seek error")])?;
                break;
            }
            let offset = pl::sub(&pl::sub(&raf_tell(it, &raf)?, &trailer.at(1)), &trailer.at(2));
            qt(it, "ProcessInsta360", vec![et.clone(), hash_of(vec![("RAF", raf.clone()), ("DirName", trailer.at(0)), ("Offset", offset)])])?;
        } else if trailer.at(0).eq_str("MIE") {
            runtime::require(it, "Image::ExifTool::MIE")?;
            pl::call(it, "Image::ExifTool::MIE::ProcessMIE", vec![et.clone(), hash_of(vec![("RAF", raf.clone()), ("DirName", s("MIE")), ("Trailer", n(1))])])?;
        }
        trailer = trailer.at(3);
    }
    // brute force scan for metadata embedded in media data
    // (and process Insta360 trailer if it exists)
    if ee.truthy() && top_level && !et.get("OPTIONS").get("FastScan").truthy() {
        qt(it, "ScanMediaData", vec![et.clone()])?;
    }

    // restore any changed options
    for (k, v) in save_options {
        m(it, &et, "Options", vec![k, v])?;
    }
    ret(n(1))
}

/// Process a QuickTime Image File
/// Inputs: 0) ExifTool object reference, 1) directory information reference
/// Returns: 1 on success
pub fn process_qtif(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let table = get_tag_table(it, "Image::ExifTool::QuickTime::ImageFile")?;
    process_mov(it, vec![et, dir_info, table], want)
}

/// Autoload LigoGPS module if necessary (Image::ExifTool::LigoGPS::AUTOLOAD)
pub fn ligo_gps_autoload(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let autoload = it.get_scalar("Image::ExifTool::LigoGPS::AUTOLOAD");
    runtime::require(it, "Image::ExifTool::LigoGPS")?;
    let name = autoload.to_text();
    if it.sub(&name).is_none() {
        // (caller(0) as the interpreter gives it)
        // reproduce Perl's standard 'undefined subroutine' message:
        return crate::interp::eval::die(format!("Undefined subroutine {name} called at (eval) line 1\n"));
    }
    call_aliased(it, &name, a.clone(), &a, 0, want) // call the function
}

/// Register the ported subs under their Perl names.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::QuickTime::AUTOLOAD", autoload);
    it.define_native("Image::ExifTool::QuickTime::GetRotationMatrix", get_rotation_matrix);
    it.define_native("Image::ExifTool::QuickTime::GetRotationAngle", get_rotation_angle);
    it.define_native("Image::ExifTool::QuickTime::CalcRotation", calc_rotation);
    it.define_native("Image::ExifTool::QuickTime::GetMatrixStructure", get_matrix_structure);
    it.define_native("Image::ExifTool::QuickTime::CalcSampleRate", calc_sample_rate);
    it.define_native("Image::ExifTool::QuickTime::FixWrongFormat", fix_wrong_format);
    it.define_native("Image::ExifTool::QuickTime::ConvertISO6709", convert_iso6709);
    it.define_native("Image::ExifTool::QuickTime::ConvertChapterList", convert_chapter_list);
    it.define_native("Image::ExifTool::QuickTime::PrintChapter", print_chapter);
    it.define_native("Image::ExifTool::QuickTime::PrintGPSCoordinates", print_gps_coordinates);
    it.define_native("Image::ExifTool::QuickTime::UnpackLang", unpack_lang);
    it.define_native("Image::ExifTool::QuickTime::GetLangCode", get_lang_code);
    it.define_native("Image::ExifTool::QuickTime::GetLangInfoQT", get_lang_info_qt);
    it.define_native("Image::ExifTool::QuickTime::GetVarInt", get_var_int);
    it.define_native("Image::ExifTool::QuickTime::GetString", get_string);
    it.define_native("Image::ExifTool::QuickTime::PrintableTagID", printable_tag_id);
    it.define_native("Image::ExifTool::QuickTime::ParseItemLocation", parse_item_location);
    it.define_native("Image::ExifTool::QuickTime::ParseContentDescribes", parse_content_describes);
    it.define_native("Image::ExifTool::QuickTime::ParseItemInfoEntry", parse_item_info_entry);
    it.define_native("Image::ExifTool::QuickTime::ParseItemPropAssoc", parse_item_prop_assoc);
    it.define_native("Image::ExifTool::QuickTime::HandleItemInfo", handle_item_info);
    it.define_native("Image::ExifTool::QuickTime::EEWarn", ee_warn);
    it.define_native("Image::ExifTool::QuickTime::QuickTimeFormat", quick_time_format);
    it.define_native("Image::ExifTool::QuickTime::ProcessMetaData", process_meta_data);
    it.define_native("Image::ExifTool::QuickTime::ProcessSampleDesc", process_sample_desc);
    it.define_native("Image::ExifTool::QuickTime::ProcessHybrid", process_hybrid);
    it.define_native("Image::ExifTool::QuickTime::ProcessRights", process_rights);
    it.define_native("Image::ExifTool::QuickTime::ProcessNextbase", process_nextbase);
    it.define_native("Image::ExifTool::QuickTime::ProcessEncodingParams", process_encoding_params);
    it.define_native("Image::ExifTool::QuickTime::ProcessKeys", process_keys);
    it.define_native("Image::ExifTool::QuickTime::ProcessMetaKeys", process_meta_keys);
    it.define_native("Image::ExifTool::QuickTime::IdentifyTrailers", identify_trailers);
    it.define_native("Image::ExifTool::QuickTime::ProcessMOV", process_mov);
    it.define_native("Image::ExifTool::QuickTime::ProcessQTIF", process_qtif);
    it.define_native("Image::ExifTool::LigoGPS::AUTOLOAD", ligo_gps_autoload);
}
