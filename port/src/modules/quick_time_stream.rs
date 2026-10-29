//! Image::ExifTool::QuickTimeStream (Image/ExifTool/QuickTimeStream.pl), ported sub by sub (see docs/port/PORTING.md).
//!
//! Extract embedded information from QuickTime media data: timed metadata
//! samples, GPS tracks (freeGPS, NMEA, dashcam formats), Insta360 trailers.
//! The subs live in package Image::ExifTool::QuickTime; the tag tables are
//! data.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::exiftool::part2;
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, re, ret, Caps};
use crate::runtime;

const PKG: &str = "Image::ExifTool::QuickTime";
const MODULE: &str = "Image::ExifTool::QuickTimeStream";

// ---- local helpers (also used by the other ports of this group) ----

pub(crate) fn n(i: i64) -> Value {
    Value::Int(i)
}

pub(crate) fn f(x: f64) -> Value {
    Value::Num(x)
}

pub(crate) fn s(x: &str) -> Value {
    pl::s(x)
}

/// `$obj->Method(ARGS)` in scalar context.
pub(crate) fn m(it: &mut Interp, obj: &Value, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::method(it, obj, name, args)
}

pub(crate) fn scalar(r: ER<Vec<Value>>) -> ER<Value> {
    Ok(r?.pop().unwrap_or_default())
}

pub(crate) fn cat(parts: &[&Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

/// `$et->Options(NAME)`
pub(crate) fn opt(it: &mut Interp, et: &Value, name: &str) -> ER<Value> {
    m(it, et, "Options", vec![s(name)])
}

/// `$et->Warn(MSG)` / `$et->Warn(MSG, LEVEL)`
pub(crate) fn warn(it: &mut Interp, et: &Value, msg: Value) -> ER<Value> {
    m(it, et, "Warn", vec![msg])
}

pub(crate) fn warn_lvl(it: &mut Interp, et: &Value, msg: Value, lvl: i64) -> ER<Value> {
    m(it, et, "Warn", vec![msg, n(lvl)])
}

/// `$et->HandleTag($tbl, TAG => VAL, PARMS...)`
pub(crate) fn ht(it: &mut Interp, et: &Value, tbl: &Value, tag: &str, val: Value) -> ER<Value> {
    m(it, et, "HandleTag", vec![tbl.clone(), s(tag), val])
}

pub(crate) fn ht_v(it: &mut Interp, et: &Value, tbl: &Value, tag: Value, val: Value, parms: Vec<(&str, Value)>) -> ER<Value> {
    let mut a = vec![tbl.clone(), tag, val];
    for (k, v) in parms {
        a.push(s(k));
        a.push(v);
    }
    m(it, et, "HandleTag", a)
}

/// Dies as perl's `$$r` does under strict refs when `r` is not a SCALAR
/// reference (pl::sderef without copying the value).
pub(crate) fn check_sref(r: &Value) -> ER<()> {
    match r {
        Value::ScalarRef(_) | Value::LvRef(_) => Ok(()),
        _ => pl::sderef(r).map(|_| ()),
    }
}

/// The error perl gives for `$obj->NAME(...)` on an undefined value or an
/// unblessed reference, at the native's location, before a call made
/// through a helper that runs the method in a string eval (whose location
/// would be "(eval N) line 1"). Other invocants pass.
pub(crate) fn check_invocant(it: &mut Interp, obj: &Value, name: &str) -> ER<()> {
    let bad = match obj {
        Value::Undef => true,
        Value::HashRef(h) => h.borrow().blessed.is_none(),
        v => v.is_ref(),
    };
    if bad {
        // (call_method dies on such an invocant before calling anything)
        it.call_method(obj, name, vec![obj.clone()], Want::Scalar)?;
    }
    Ok(())
}

/// `$raf->Read($buff, $len)`: the count, and `buff` updated.
pub(crate) fn raf_read(it: &mut Interp, raf: &Value, buff: &mut Value, len: Value) -> ER<Value> {
    let mut a = [buff.clone(), len];
    let r = scalar(pl::method_mut(it, raf, "Read", &mut a, Want::Scalar))?;
    *buff = a[0].clone();
    Ok(r)
}

pub(crate) fn raf_seek(it: &mut Interp, raf: &Value, pos: Value, whence: i64) -> ER<bool> {
    Ok(m(it, raf, "Seek", vec![pos, n(whence)])?.truthy())
}

pub(crate) fn raf_tell(it: &mut Interp, raf: &Value) -> ER<Value> {
    m(it, raf, "Tell", vec![])
}

pub(crate) fn get8u(it: &mut Interp, dp: &Value, pos: Value) -> ER<Value> {
    scalar(part2::get8u(it, vec![dp.clone(), pos], Want::Scalar))
}

pub(crate) fn get16u(it: &mut Interp, dp: &Value, pos: Value) -> ER<Value> {
    scalar(part2::get16u(it, vec![dp.clone(), pos], Want::Scalar))
}

pub(crate) fn get16s(it: &mut Interp, dp: &Value, pos: Value) -> ER<Value> {
    scalar(part2::get16s(it, vec![dp.clone(), pos], Want::Scalar))
}

pub(crate) fn get32u(it: &mut Interp, dp: &Value, pos: Value) -> ER<Value> {
    scalar(part2::get32u(it, vec![dp.clone(), pos], Want::Scalar))
}

pub(crate) fn get32s(it: &mut Interp, dp: &Value, pos: Value) -> ER<Value> {
    scalar(part2::get32s(it, vec![dp.clone(), pos], Want::Scalar))
}

pub(crate) fn get_float(it: &mut Interp, dp: &Value, pos: Value) -> ER<Value> {
    scalar(part2::get_float(it, vec![dp.clone(), pos], Want::Scalar))
}

pub(crate) fn get_double(it: &mut Interp, dp: &Value, pos: Value) -> ER<Value> {
    scalar(part2::get_double(it, vec![dp.clone(), pos], Want::Scalar))
}

pub(crate) fn get64u(it: &mut Interp, dp: &Value, pos: Value) -> ER<Value> {
    pl::call(it, "Image::ExifTool::Get64u", vec![dp.clone(), pos])
}

pub(crate) fn get64s(it: &mut Interp, dp: &Value, pos: Value) -> ER<Value> {
    pl::call(it, "Image::ExifTool::Get64s", vec![dp.clone(), pos])
}

/// `ReadValue(DATAPT, OFFSET, FORMAT, COUNT, SIZE)` in scalar context.
pub(crate) fn read_value(it: &mut Interp, dp: &Value, off: Value, fmt: Value, count: Value, size: Value) -> ER<Value> {
    scalar(part2::read_value(it, vec![dp.clone(), off, fmt, count, size], Want::Scalar))
}

/// `ReadValue(...)` in list context.
pub(crate) fn read_value_list(it: &mut Interp, dp: &Value, off: Value, fmt: Value, count: Value, size: Value) -> ER<Vec<Value>> {
    part2::read_value(it, vec![dp.clone(), off, fmt, count, size], Want::List)
}

pub(crate) fn set_byte_order(it: &mut Interp, o: Value) -> ER<Value> {
    scalar(part2::set_byte_order(it, vec![o], Want::Scalar))
}

pub(crate) fn get_byte_order(it: &mut Interp) -> ER<Value> {
    scalar(part2::get_byte_order(it, vec![], Want::Scalar))
}

pub(crate) fn get_tag_table(it: &mut Interp, name: &str) -> ER<Value> {
    pl::call(it, "Image::ExifTool::GetTagTable", vec![s(name)])
}

pub(crate) fn add_tag_to_table(it: &mut Interp, tbl: &Value, tag: Value, info: Value) -> ER<Value> {
    pl::call(it, "Image::ExifTool::AddTagToTable", vec![tbl.clone(), tag, info])
}

/// `Image::ExifTool::ConvertUnixTime(ARGS)`
pub(crate) fn convert_unix_time(it: &mut Interp, args: Vec<Value>) -> ER<Value> {
    scalar(part2::convert_unix_time(it, args, Want::Scalar))
}

/// `{ KEY => VAL, ... }`
pub(crate) fn hash_of(pairs: Vec<(&str, Value)>) -> Value {
    let h = Value::new_hash();
    for (k, v) in pairs {
        h.set(k, v);
    }
    h
}

/// `$$et{KEY} = ++$$et{DOC_COUNT}` style increment: returns the new value.
pub(crate) fn pre_inc(h: &Value, key: &str) -> Value {
    let v = pl::add(&h.get(key), &n(1));
    h.set(key, v.clone());
    v
}

/// `$$et{DOC_NUM} = ++$$et{DOC_COUNT}`
pub(crate) fn new_doc(et: &Value) {
    let v = pre_inc(et, "DOC_COUNT");
    et.set("DOC_NUM", v);
}

/// `"@list"` (with the default `$"` of a space).
pub(crate) fn join_sp(v: &[Value]) -> Value {
    pl::join(" ", v)
}

/// `$s =~ /PAT/` captures.
pub(crate) fn mt(pat: &'static str, flags: &'static str, subject: &Value) -> Option<Caps> {
    re(pat, flags).m(subject)
}

/// `$s =~ /\G.../g` in scalar context from position `pos`: the captures
/// and the end of the match (perl's new `pos`).
pub(crate) fn match_from(pat: &str, flags: &str, subject: &Value, pos: usize) -> Option<(Caps, usize)> {
    let r = crate::perl::regex::compile(pat.as_bytes(), flags).unwrap_or_else(|e| panic!("bad pattern /{pat}/: {e}"));
    let b = subject.to_bytes();
    match r.match_at(&b, pos) {
        Ok(Some(sp)) => {
            let end = sp[0].map(|x| x.1).unwrap_or(pos);
            let caps = sp.iter().map(|g| g.map(|(a, e)| Value::bytes(&b[a..e]))).collect();
            Some((caps, end))
        }
        _ => None,
    }
}

/// `length $x` as a number (0 for undef).
pub(crate) fn len_of(v: &Value) -> i64 {
    v.to_bytes().len() as i64
}

/// `substr($s, OFF, LEN)` with integer arguments.
pub(crate) fn sub_str(v: &Value, off: i64, len: i64) -> Value {
    pl::substr(v, off, Some(len))
}

/// `pack 'C*', map { $_ ^ KEY } unpack 'C*', $s`
pub(crate) fn xor_bytes(v: &Value, key: u8) -> Value {
    Value::bytes(v.to_bytes().iter().map(|b| b ^ key).collect::<Vec<u8>>())
}

pub(crate) fn defined(v: &Value) -> bool {
    !v.is_undef()
}

/// `shift @$a` on an array reference.
pub(crate) fn shift(a: &Value) -> Value {
    match a {
        Value::ArrayRef(r) => {
            let mut v = r.borrow_mut();
            if v.is_empty() {
                Value::Undef
            } else {
                v.remove(0)
            }
        }
        _ => Value::Undef,
    }
}

/// `$x || DEFAULT`
pub(crate) fn or(v: Value, d: Value) -> Value {
    if v.truthy() {
        v
    } else {
        d
    }
}

// conversion factors
fn knots_to_kph() -> Value {
    Value::Num(1.852) // knots --> km/h
}
fn mps_to_kph() -> Value {
    Value::Num(3.6) // m/s   --> km/h
}
fn mph_to_kph() -> Value {
    Value::Num(1.60934) // mph   --> km/h
}

// typical (minimum?) size of freeGPS block
const GPS_BLOCK_SIZE: i64 = 0x8000;

/// A file lexical of QuickTimeStream.pl (`my %qtFmt`...).
fn lex_hash(it: &mut Interp, name: &str) -> Value {
    runtime::lex_hash(it, MODULE, name)
}

fn lex_array(it: &mut Interp, name: &str) -> Value {
    runtime::lex_array(it, MODULE, name)
}

/// `QuickTime::PrintableTagID(ARGS)`
fn printable_tag_id(it: &mut Interp, args: Vec<Value>) -> ER<Value> {
    pl::call(it, "Image::ExifTool::QuickTime::PrintableTagID", args)
}

/// `(($deg || 0) + $min/60) * SIGN`
fn deg_min(deg: &Value, min: &Value, sign: i64) -> ER<Value> {
    Ok(pl::mul(&pl::add(&or(deg.clone(), n(0)), &pl::div(min, &n(60))?), &n(sign)))
}

/// SignedInt32 on a value (the sub reads `$_`).
fn signed_int32_of(v: &Value) -> Value {
    if pl::lt(v, &n(0x80000000)) {
        v.clone()
    } else {
        pl::sub(v, &n(4294967296))
    }
}

// ---- the subs ----

/// Convert unsigned 32-bit integer to signed
/// Inputs: <none> (uses value in $_)
/// Returns: signed integer
pub fn signed_int32(it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let v = it.get_scalar("main::_");
    ret(signed_int32_of(&v))
}

/// Save information from keys in OtherSampleDesc directory for processing timed metadata
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
/// (ref "Timed Metadata Media" here:
///  https://developer.apple.com/library/content/documentation/QuickTime/QTFF/QTFFChap3/qtff3.html)
pub fn save_meta_keys(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| save_meta_keys_impl(it, &a))
}

fn save_meta_keys_impl(it: &mut Interp, a: &[Value]) -> ER<Vec<Value>> {
    let (et, dir_info, tag_tbl) = (arg(a, 0), arg(a, 1), arg(a, 2));
    let data_pt = dir_info.get("DataPt");
    pl::at(881);
    let data = pl::sderef(&data_pt)?;
    let dir_len = len_of(&data);
    if dir_len <= 8 {
        return ret(n(0));
    }
    let mut pos = 0i64;
    let verbose = et.get("OPTIONS").get("Verbose");
    let old_indent = et.get("INDENT");
    let mut ee = et.get("ee");
    if !ee.truthy() {
        ee = Value::new_hash();
        et.set("ee", ee.clone());
    }
    let qt_fmt = lex_hash(it, "qtFmt");

    if verbose.truthy() {
        m(it, &et, "VerboseDir", vec![dir_info.get("DirName"), Value::Undef, n(dir_len)])?;
    }

    // loop through metadata key table
    while pos + 8 < dir_len {
        let size = get32u(it, &data_pt, n(pos))?;
        let id = sub_str(&data, pos + 4, 4);
        let mut end = pl::add(&n(pos), &size);
        if pl::gt(&end, &n(dir_len)) {
            end = n(dir_len);
        }
        let end = end.to_i64();
        pos += 8;
        let (mut tag_id, mut format, mut pid) = (Value::Undef, Value::Undef, Value::Undef);
        if verbose.truthy() {
            pid = printable_tag_id(it, vec![id.clone(), n(1)])?;
            m(it, &et, "VPrint", vec![n(0), cat(&[&old_indent, &s("+ [Metadata Key entry, Local ID="), &pid, &s(", "), &size, &s(" bytes]\n")])])?;
            et.set("INDENT", cat(&[&et.get("INDENT"), &s("| ")]));
        }

        while pos + 4 < end {
            let mut len = pl::unpack1(&format!("x{pos}N"), &data)?.to_i64();
            if len < 8 || pos + len > end {
                break;
            }
            let tag = sub_str(&data, pos + 4, 4);
            pos += 8;
            len -= 8;
            let base = pl::add(&dir_info.get("Base"), &n(pos));
            let val = sub_str(&data, pos, len);
            pos += len;
            let mut st = Value::Undef;
            if tag.eq_str("keyd") {
                tag_id = val.clone();
                re(r"^(mdta|fiel)com\.apple\.quicktime\.", "").subst(&mut tag_id, false, |_| Vec::new());
                if !tag_id.truthy() {
                    tag_id = cat(&[&s("Tag_"), &val]);
                }
                if verbose.truthy() {
                    st = val.clone();
                    re(r"(.{4})", "").subst(&mut st, false, |c| [&cap(c, 1).to_bytes()[..], b" "].concat());
                }
            } else if tag.eq_str("dtyp") {
                if len_of(&val) < 4 {
                    continue;
                }
                if len_of(&val) >= 4 {
                    let ns = pl::unpack1("N", &val)?;
                    if pl::num_eq(&ns, &n(0)) {
                        if len_of(&val) < 8 {
                            warn(it, &et, s("Short dtyp data"))?;
                            continue;
                        }
                        st = pl::unpack1("x4N", &val)?;
                        format = or(qt_fmt.get(st.to_bytes()), s("undef"));
                    } else if pl::num_eq(&ns, &n(1)) {
                        st = pl::substr(&val, 4, None);
                        format = s("undef");
                    } else {
                        format = s("undef");
                    }
                    if verbose.truthy() && defined(&st) {
                        st = cat(&[&st, &s(" ("), &format, &s(")")]);
                    }
                }
            } else {
                ht_v(it, &et, &tag_tbl, tag.clone(), Value::Undef, vec![("DataPt", pl::sref(val.clone())), ("Base", base.clone())])?;
                continue;
            }
            if pl::gt(&verbose, &n(1)) {
                if defined(&st) {
                    pl::tr(&mut st, "\\x00-\\x1f\\x7f-\\xff", ".", "");
                    st = cat(&[&s(" = "), &st]);
                } else {
                    st = s("");
                }
                let p = printable_tag_id(it, vec![tag.clone(), n(2)])?;
                m(it, &et, "VPrint", vec![n(1), cat(&[&et.get("INDENT"), &s("- Tag '"), &p, &s("' ("), &n(len), &s(" bytes)"), &st, &s("\n")])])?;
                m(it, &et, "VerboseDump", vec![pl::sref(val.clone()), s("Base"), base.clone()])?;
            }
        }
        if defined(&tag_id) && defined(&format) {
            if verbose.truthy() {
                let t2 = printable_tag_id(it, vec![tag_id.clone()])?;
                m(it, &et, "VPrint", vec![n(0), cat(&[&et.get("INDENT"), &s("Added Local ID "), &pid, &s(" = "), &t2, &s(" ("), &format, &s(")\n")])])?;
            }
            ee.get_or_new_hash("keys").set(id.to_bytes(), hash_of(vec![("TagID", tag_id.clone()), ("Format", format.clone())]));
        }
        et.set("INDENT", old_indent.clone());
    }
    ret(n(1))
}

/// We found some tags for this sample, so set document number and save timing information
/// Inputs: 0) ExifTool ref, 1) tag table ref, 2) sample time, 3) sample duration
pub fn found_something(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, tag_tbl, time, dur) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    ret(found_some(it, &et, &tag_tbl, &time, &dur)?)
}

/// (returns the value of the last statement, as the sub does)
fn found_some(it: &mut Interp, et: &Value, tag_tbl: &Value, time: &Value, dur: &Value) -> ER<Value> {
    new_doc(et);
    if defined(time) {
        ht(it, et, tag_tbl, "SampleTime", time.clone())?;
    }
    if defined(dur) {
        return ht(it, et, tag_tbl, "SampleDuration", dur.clone());
    }
    Ok(s(""))
}

/// Approximate GPSDateTime value from sample time and CreateDate
/// Inputs: 0) ExifTool ref, 1) tag table ptr, 2) sample time (s)
///         3) true if CreateDate is UTC
/// Notes: Uses ExifTool CreateDateAtEnd as flag to subtract video duration
pub fn set_gps_date_time(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, tag_tbl, sample_time, is_utc) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    set_gps_dt(it, &et, &tag_tbl, sample_time, is_utc)?;
    pl::ret_nothing(want)
}

fn set_gps_dt(it: &mut Interp, et: &Value, tag_tbl: &Value, mut sample_time: Value, is_utc: Value) -> ER<()> {
    let value = et.get("VALUE");
    if defined(&sample_time) && value.get("CreateDate").truthy() {
        sample_time = pl::add(&sample_time, &value.get("CreateDate")); // adjust sample time to seconds since the epoch
        if et.get("CreateDateAtEnd").truthy() {
            // adjust if CreateDate is at end of video
            if !(value.get("TimeScale").truthy() && value.get("Duration").truthy()) {
                return Ok(());
            }
            pl::at(988);
            sample_time = pl::sub(&sample_time, &pl::div(&value.get("Duration"), &value.get("TimeScale"))?);
            warn_lvl(it, et, s("Approximating GPSDateTime as CreateDate - Duration + SampleTime"), 1)?;
        } else {
            warn_lvl(it, et, s("Approximating GPSDateTime as CreateDate + SampleTime"), 1)?;
        }
        let mut utc = opt(it, et, "QuickTimeUTC")?;
        if !defined(&utc) {
            utc = is_utc; // (allow QuickTimeUTC=0 to override $isUTC default)
        }
        if !utc.truthy() {
            let mut tz_off = et.get("tzOff"); // use previously calculated offset
            if !defined(&tz_off) {
                // adjust to UTC, assuming time is local
                let code = s("my @tm = localtime $t; my @gm = gmtime $t; Image::ExifTool::GetTimeZone(\\@tm, \\@gm) * 60");
                let saved = it.get_scalar("main::@");
                let (v, _) = pl::eval_site(it, &code, PKG, &[("t", value.get("CreateDate"))], Want::Scalar);
                let err = it.get_scalar("main::@");
                if err.truthy() {
                    return Err(crate::interp::Flow::Die(err));
                }
                it.set_scalar("main::@", saved);
                tz_off = v;
                et.set("tzOff", tz_off.clone());
            }
            sample_time = pl::sub(&sample_time, &tz_off); // shift from local time to UTC
        }
        et.set("SET_GROUP0", s("Composite"));
        let t = convert_unix_time(it, vec![sample_time, n(0), n(3)])?;
        ht(it, et, tag_tbl, "GPSDateTime", cat(&[&t, &s("Z")]))?;
        et.delete("SET_GROUP0");
    }
    Ok(())
}

/// Handle tags that we found in the subtitle 'text'
/// Inputs: 0) ExifTool ref, 1) tag table ref, 2) hash of tag names/values
pub fn handle_text_tags(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, tag_tbl, tags) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    handle_text(it, &et, &tag_tbl, &tags)?;
    ret(Value::Undef)
}

fn handle_text(it: &mut Interp, et: &Value, tag_tbl: &Value, tags: &Value) -> ER<()> {
    tags.delete("done");
    if tags.get("GPSDateTime").truthy() {
        tags.delete("GPSTimeStamp");
    }
    let mut keys = tags.keys();
    keys.sort();
    for tag in keys {
        let v = tags.get(&tag);
        m(it, et, "HandleTag", vec![tag_tbl.clone(), Value::Str(tag), v])?;
    }
    et.set("UnknownTextCount", n(0));
    if let Value::HashRef(h) = tags {
        h.borrow_mut().clear(); // clear the hash
    }
    Ok(())
}

/// Handle new time in NMEA stream and store queued tags if necessary
/// Inputs: 0) ExifTool ref, 1) time string, 2) tag table ref, 3) tags hash
pub fn handle_new_time(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, time, tag_tbl, tags) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    new_time(it, &et, &time, &tag_tbl, &tags)?;
    ret(et.get("LastDoc"))
}

fn new_time(it: &mut Interp, et: &Value, time: &Value, tag_tbl: &Value, tags: &Value) -> ER<()> {
    if et.get("LastTime").truthy() {
        if et.get("LastTime").to_bytes() == time.to_bytes() {
            // combine with the previous NMEA sentence
            et.set("DOC_NUM", et.get("LastDoc"));
        } else if {
            pl::at(1037);
            pl::hderef(tags)?.len() > 0
        } {
            // handle existing tags and start a new document
            // (see https://exiftool.org/forum/index.php?msg=75422)
            handle_text(it, et, tag_tbl, tags)?;
            // increment document number and update document count if necessary
            let dn = pre_inc(et, "DOC_NUM");
            if pl::lt(&et.get("DOC_COUNT"), &dn) {
                et.set("DOC_COUNT", dn);
            }
        }
    }
    et.set("LastTime", time.clone());
    et.set("LastDoc", et.get("DOC_NUM"));
    Ok(())
}

/// Convert latitude/longitude from DDDMM.MMMM format to decimal degrees
/// Inputs: 0) latitude, 1) longitude
/// Returns: lat/lon are changed in place
/// (note: this method works fine for negative coordinates)
pub fn convert_lat_lon_native(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (mut lat, mut lon) = (arg(&a, 0), arg(&a, 1));
    convert_lat_lon(&mut lat, &mut lon)?;
    it.set_arg(0, lat)?;
    it.set_arg(1, lon.clone())?;
    ret(lon)
}

pub(crate) fn convert_lat_lon(lat: &mut Value, lon: &mut Value) -> ER<()> {
    let mut deg = pl::int(&pl::div(lat, &n(100))?); // latitude
    *lat = pl::add(&deg, &pl::div(&pl::sub(lat, &pl::mul(&deg, &n(100))), &n(60))?);
    deg = pl::int(&pl::div(lon, &n(100))?); // longitude
    *lon = pl::add(&deg, &pl::div(&pl::sub(lon, &pl::mul(&deg, &n(100))), &n(60))?);
    Ok(())
}

/// Decrypt Lucky data
/// Inputs: 0) string to decrypt, 1) encryption key
/// Returns: decrypted string
pub fn decrypt_lucky(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    if arg(&a, 1).to_bytes().is_empty() {
        // ($i % length($key) with an empty key)
        pl::at(1619);
        pl::modulo(&n(0), &n(0))?;
    }
    ret(lucky(&arg(&a, 0), &arg(&a, 1)))
}

fn lucky(st: &Value, key: &Value) -> Value {
    let mut sv: Vec<u8> = st.to_bytes().to_vec();
    let kb = key.to_bytes();
    let mut enc: Vec<usize> = (0..256).collect();
    let (mut i, mut j) = (0usize, 0usize);
    loop {
        // (the keys used here are never empty, so the modulus is defined)
        let kv = if kb.is_empty() { 0 } else { kb[i % kb.len()] as usize };
        j = (j + enc[i] + kv) & 0xff;
        enc.swap(i, j);
        i += 1;
        if i >= 256 {
            break;
        }
    }
    let (mut i, mut j, mut k) = (0usize, 0usize, 0usize);
    loop {
        j = (j + 1) & 0xff;
        k = (k + enc[j]) & 0xff;
        enc.swap(j, k);
        let x = enc[(enc[j] + enc[k]) & 0xff] as u8;
        if i < sv.len() {
            sv[i] ^= x;
        } else {
            sv.push(x); // ($str[$i] ^= ... creates the element)
        }
        i += 1;
        if i >= sv.len() {
            break;
        }
    }
    Value::bytes(sv)
}

/// Append to `$tags{Text}` as Process_text does.
fn add_text(tags: &Value, txt: Value) {
    let t = tags.get("Text");
    tags.set("Text", if defined(&t) { cat(&[&t, &txt]) } else { txt });
}

/// Process subtitle 'text'
/// Inputs: 0) ExifTool ref, 1) data ref or dirInfo ref, 2) tag table ref
///         3) flag set if text was already stored
pub fn process_text(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, data_pt, tag_tbl, handled) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    match process_text_impl(it, &et, data_pt, &tag_tbl, &handled)? {
        Some(v) => ret(v),
        None => pl::ret_nothing(want),
    }
}

/// The result is what the sub returns: None for `return;`, or the value of
/// its last statement.
fn process_text_impl(it: &mut Interp, et: &Value, mut data_pt: Value, tag_tbl: &Value, handled: &Value) -> ER<Option<Value>> {
    let tags = Value::new_hash();

    if et.get("NoMoreTextDecoding").truthy() {
        return Ok(None);
    }

    if pl::ref_of(&data_pt) == "HASH" {
        let dir_name = data_pt.get("DirName");
        data_pt = data_pt.get("DataPt");
        pl::at(1063);
        let l = len_of(&pl::sderef(&data_pt)?);
        m(it, et, "VerboseDir", vec![dir_name, Value::Undef, n(l)])?;
    }
    pl::at(1066);
    let mut data = pl::sderef(&data_pt)?;

    for c in re(r"\$(\w+)([^\$\0]*)", "").each(&data) {
        let (tag, dat) = (cap(&c, 1), cap(&c, 2));
        if re(r"^[A-Z]{2}RMC$", "").is_match(&tag) {
            let Some(c) = mt(r"^,(\d{2})(\d{2})(\d+(?:\.\d*)),A?,(\d*?)(\d{1,2}\.\d+),([NS]),(\d*?)(\d{1,2}\.\d+),([EW]),(\d*\.?\d*),(\d*\.?\d*),(\d{2})(\d{2})(\d+)", "", &dat) else {
                if !handled.truthy() {
                    add_text(&tags, cat(&[&s("$"), &tag, &dat]));
                }
                if re(r"^,\d+\.?\d*,V,", "").is_match(&dat) {
                    et.set("UnknownTextCount", n(0)); // (allow any number of void fixes)
                }
                continue;
            };
            let time = cat(&[&cap(&c, 1), &s(":"), &cap(&c, 2), &s(":"), &cap(&c, 3)]);
            new_time(it, et, &time, tag_tbl, &tags)?;
            let c14 = cap(&c, 14);
            let year = pl::add(&c14, &n(if pl::ge(&c14, &n(70)) { 1900 } else { 2000 }));
            let date = pl::sprintf("%.4d:%.2d:%.2d", &[year, cap(&c, 13), cap(&c, 12)]);
            et.set("LastDate", date.clone());
            tags.set("GPSDateTime", cat(&[&date, &s(" "), &time, &s("Z")]));
            tags.set("GPSLatitude", deg_min(&cap(&c, 4), &cap(&c, 5), if cap(&c, 6).eq_str("N") { 1 } else { -1 })?);
            tags.set("GPSLongitude", deg_min(&cap(&c, 7), &cap(&c, 8), if cap(&c, 9).eq_str("E") { 1 } else { -1 })?);
            if len_of(&cap(&c, 10)) > 0 {
                tags.set("GPSSpeed", pl::mul(&cap(&c, 10), &knots_to_kph()));
            }
            if len_of(&cap(&c, 11)) > 0 {
                tags.set("GPSTrack", cap(&c, 11));
            }
        } else if let Some(c) = re(r"^[A-Z]{2}GGA$", "").is_match(&tag).then(|| mt(r"^,(\d{2})(\d{2})(\d+(?:\.\d*)?),(\d*?)(\d{1,2}\.\d+),([NS]),(\d*?)(\d{1,2}\.\d+),([EW]),[1-6]?,(\d+)?,(\.\d+|\d+\.?\d*)?,(-?\d+\.?\d*)?,M?", "s", &dat)).flatten() {
            let time = cat(&[&cap(&c, 1), &s(":"), &cap(&c, 2), &s(":"), &cap(&c, 3)]);
            new_time(it, et, &time, tag_tbl, &tags)?;
            tags.set("GPSTimeStamp", time);
            tags.set("GPSLatitude", deg_min(&cap(&c, 4), &cap(&c, 5), if cap(&c, 6).eq_str("N") { 1 } else { -1 })?);
            tags.set("GPSLongitude", deg_min(&cap(&c, 7), &cap(&c, 8), if cap(&c, 9).eq_str("E") { 1 } else { -1 })?);
            if defined(&cap(&c, 10)) {
                tags.set("GPSSatellites", cap(&c, 10));
            }
            if defined(&cap(&c, 11)) {
                tags.set("GPSDOP", cap(&c, 11));
            }
            if defined(&cap(&c, 12)) {
                tags.set("GPSAltitude", cap(&c, 12));
            }
        // ($G and $GS are ref https://exiftool.org/forum/index.php?topic=13115.msg71743#msg71743)
        } else if let Some(c) = tag.eq_str("G").then(|| mt(r":(\d{4})-(\d{2})-(\d{2}) (\d{2}:\d{2}:\d{2})-([NS])(\d+\.\d+)-([EW])(\d+\.\d+)-S(\d+)", "", &dat)).flatten() {
            tags.set("GPSDateTime", cat(&[&cap(&c, 1), &s(":"), &cap(&c, 2), &s(":"), &cap(&c, 3), &s(" "), &cap(&c, 4)]));
            tags.set("GPSLatitude", pl::mul(&cap(&c, 6), &n(if cap(&c, 5).eq_str("S") { -1 } else { 1 })));
            tags.set("GPSLongitude", pl::mul(&cap(&c, 8), &n(if cap(&c, 7).eq_str("W") { -1 } else { 1 })));
            tags.set("GPSSpeed", cap(&c, 9));
        } else if let Some(c) = tag.eq_str("GS").then(|| mt(r":([-+]?\d+),([-+]?\d+),([-+]?\d+)", "", &dat)).flatten() {
            // scale and re-arrange to match gsensor output from Win app (forum11665)
            let acc = [pl::div(&pl::add(&cap(&c, 2), &n(2432)), &n(1000))?, pl::div(&pl::add(&cap(&c, 3), &n(361)), &n(1000))?, pl::div(&pl::sub(&cap(&c, 1), &n(3708)), &n(1000))?];
            tags.set("Accelerometer", join_sp(&acc));
        } else if let Some(c) = tag.eq_str("BEGINGSENSOR").then(|| mt(r"^:([-+]\d+\.\d+):([-+]\d+\.\d+):([-+]\d+\.\d+)", "", &dat)).flatten() {
            tags.set("Accelerometer", cat(&[&cap(&c, 1), &s(" "), &cap(&c, 2), &s(" "), &cap(&c, 3)]));
        } else if let Some(c) = tag.eq_str("TIME").then(|| mt(r"^:(\d+)", "", &dat)).flatten() {
            tags.set("TimeCode", pl::div(&cap(&c, 1), &or(et.get("MediaTS"), n(1)))?);
        } else if tag.eq_str("BEGIN") {
            if len_of(&dat) > 0 {
                tags.set("Text", dat.clone());
            }
            tags.set("done", n(1));
        } else if !tag.eq_str("END") && !handled.truthy() {
            add_text(&tags, cat(&[&s("$"), &tag, &dat]));
        }
    }
    if !tags.is_empty() {
        if !tags.get("Accelerometer").truthy() {
            // (probably unnecessary test)
            // check for NextBase 622GW accelerometer data
            // Example data (leading 2-byte length word has been stripped by ProcessSamples):
            // 0000: 00 00 00 00 32 30 32 32 30 39 30 35 31 36 34 30 [....202209051640]
            // 0010: 33 33 00 00 29 00 ba ff 48 ff 18 00 f2 07 5a ff [33..)...H.....Z.]
            // 0020: 64 ff e8 ff 58 ff e8 ff c1 07 43 ff 41 ff d2 ff [d...X.....C.A...]
            // 0030: 58 ff ea ff dc 07 50 ff 30 ff e0 ff 72 ff d8 ff [X.....P.0...r...]
            // 0040: f5 07 51 ff 16 ff dc ff 6a ff ca ff 33 08 45 ff [..Q.....j...3.E.]
            if let Some(c) = mt(r"^\0{4}(\d{4})(\d{2})(\d{2})(\d{2})(\d{2})(\d{2})\0\0.{2}", "s", &data) {
                tags.set("DateTimeStamp", cat(&[&cap(&c, 1), &s(":"), &cap(&c, 2), &s(":"), &cap(&c, 2), &s(" "), &cap(&c, 4), &s(":"), &cap(&c, 5), &s(":"), &cap(&c, 6)]));
                let mut num = pl::unpack1("x20v", &data)?; // number of accelerometer readings
                if num.truthy() && pl::lt(&pl::add(&pl::mul(&num, &n(12)), &n(22)), &n(len_of(&data))) {
                    num = pl::mul(&num, &n(6));
                    let acc: Vec<Value> = pl::unpack(&format!("x22v{}", num.to_i64()), &data)?.into_iter().map(|v| if pl::ge(&v, &n(0x8000)) { pl::sub(&v, &n(0x10000)) } else { v }).collect();
                    tags.set("AccelerometerData", join_sp(&acc));
                }
            }
        }
        if tags.get("GPSTimeStamp").truthy() && !tags.get("GPSDateTime").truthy() && et.get("LastDate").truthy() {
            // hack to fill in missing date for NextBase 662GW
            // (note: this doesn't necessarily handle day rollover properly)
            tags.set("GPSDateTime", cat(&[&et.get("LastDate"), &s(" "), &tags.get("GPSTimeStamp"), &s("Z")]));
        }
        handle_text(it, et, tag_tbl, &tags)?;
        return Ok(None);
    }
    // check for enciphered binary GPS data
    // BlueSkySea:
    //   0000: 00 00 aa aa aa aa 54 54 98 9a 9b 93 9a 92 98 9a [......TT........]
    //   0010: 9a 9d 9f 9b 9f 9d aa aa aa aa aa aa aa aa aa aa [................]
    //   0020: aa aa aa aa aa a9 e4 9e 92 9f 9b 9f 92 9d 99 ef [................]
    //   0030: 9a 9a 98 9b 93 9d 9d 9c 93 aa aa aa aa aa 9a 99 [................]
    //   0040: 9b aa aa aa aa aa aa aa aa aa aa aa aa aa aa aa [................]
    //   [...]
    //  decrypted:
    //   0000: aa aa 00 00 00 00 fe fe 32 30 31 39 30 38 32 30 [........20190820]
    //   0010: 30 37 35 31 35 37 00 00 00 00 00 00 00 00 00 00 [075157..........]
    //   0020: 00 00 00 00 00 03 4e 34 38 35 31 35 38 37 33 45 [......N48515873E]
    //   0030: 30 30 32 31 39 37 37 36 39 00 00 00 00 00 30 33 [002197769.....03]
    //   0040: 31 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 [1...............]
    //   [...]
    // Ambarella A12:
    //   0000: 00 00 f2 e1 f0 ee 54 54 98 9a 9b 93 9b 9b 9b 9c [......TT........]
    //   0010: 9b 9a 9a 93 9a 9b a6 9a 9b 9b 93 9b 9a 9b 9c 9a [................]
    //   0020: 9d 9a 92 9f 93 a9 e4 9f 9f 9e 9f 9b 9b 9c 9d ef [................]
    //   0030: 9a 99 9d 9e 99 9a 9a 9e 9b 81 9a 9b 9f 9d 9a 9a [................]
    //   0040: 9a 87 9a 9a 9a 87 9a 98 99 87 9a 9a 99 87 9a 9a [................]
    //   [...]
    //  decrypted:
    //   0000: aa aa 58 4b 5a 44 fe fe 32 30 31 39 31 31 31 36 [..XKZD..20191116]
    //   0010: 31 30 30 39 30 31 0c 30 31 31 39 31 30 31 36 30 [100901.011910160]
    //   0020: 37 30 38 35 39 03 4e 35 35 34 35 31 31 36 37 45 [70859.N55451167E]
    //   0030: 30 33 37 34 33 30 30 34 31 2b 30 31 35 37 30 30 [037430041+015700]
    //   0040: 30 2d 30 30 30 2d 30 32 33 2d 30 30 33 2d 30 30 [0-000-023-003-00]
    //   [...]
    //   0100: aa 55 57 ed ed 45 58 54 44 00 01 30 30 30 30 31 [.UW..EXTD..00001]
    //   0110: 31 30 38 30 30 30 58 00 58 00 58 00 58 00 58 00 [108000X.X.X.X.X.]
    //   0120: 58 00 58 00 58 00 58 00 00 00 00 00 00 00 00 00 [X.X.X.X.........]
    //   0130: 00 00 00 00 00 00 00                            [.......]
    if re(r"^\0\0(..\xaa\xaa|\xf2\xe1\xf0\xee)", "s").is_match(&data) && len_of(&data) >= 282 {
        let mut val = xor_bytes(&sub_str(&data, 8, 14), 0xaa);
        if let Some(c) = mt(r"^(\d{4})(\d{2})(\d{2})(\d{2})(\d{2})(\d{2})$", "", &val) {
            tags.set("GPSDateTime", cat(&[&cap(&c, 1), &s(":"), &cap(&c, 2), &s(":"), &cap(&c, 3), &s(" "), &cap(&c, 4), &s(":"), &cap(&c, 5), &s(":"), &cap(&c, 6)]));
            val = xor_bytes(&sub_str(&data, 38, 9), 0xaa);
            if let Some(c) = mt(r"^([NS])(\d{2})(\d+$)$", "", &val) {
                let v = pl::add(&cap(&c, 2), &pl::div(&cap(&c, 3), &n(600000))?);
                tags.set("GPSLatitude", pl::mul(&v, &n(if cap(&c, 1).eq_str("S") { -1 } else { 1 })));
            }
            val = xor_bytes(&sub_str(&data, 47, 10), 0xaa);
            if let Some(c) = mt(r"^([EW])(\d{3})(\d+$)$", "", &val) {
                let v = pl::add(&cap(&c, 2), &pl::div(&cap(&c, 3), &n(600000))?);
                tags.set("GPSLongitude", pl::mul(&v, &n(if cap(&c, 1).eq_str("W") { -1 } else { 1 })));
            }
            val = xor_bytes(&sub_str(&data, 0x39, 5), 0xaa);
            if re(r"^[-+]\d+$", "").is_match(&val) {
                tags.set("GPSAltitude", pl::add(&val, &n(0)));
            }
            val = xor_bytes(&sub_str(&data, 0x3e, 3), 0xaa);
            if re(r"^\d+$", "").is_match(&val) {
                tags.set("GPSSpeed", pl::add(&val, &n(0)));
            }
            if re(r"^\0\0..\xaa\xaa", "s").is_match(&data) {
                // (BlueSkySea)
                val = xor_bytes(&sub_str(&data, 0xad, 12), 0xaa);
                // the first X,Y,Z accelerometer readings from the AccelerometerData
                if let Some(c) = mt(r"^([-+]\d{3})([-+]\d{3})([-+]\d{3})$", "", &val) {
                    tags.set("Accelerometer", cat(&[&cap(&c, 1), &s(" "), &cap(&c, 2), &s(" "), &cap(&c, 3)]));
                    val = xor_bytes(&sub_str(&data, 0xba, 96), 0xaa);
                    let order = get_byte_order(it)?;
                    set_byte_order(it, s("II"))?;
                    val = read_value(it, &pl::sref(val), n(0), s("float"), Value::Undef, Value::Undef)?;
                    set_byte_order(it, order)?;
                    tags.set("AccelerometerData", val);
                }
            } else {
                // (Ambarella)
                let mut acc: Vec<Value> = Vec::new();
                val = xor_bytes(&sub_str(&data, 0x41, 195), 0xaa);
                let mut p = 0;
                while let Some((c, e)) = match_from(r"\G([-+]\d{3})([-+]\d{3})([-+]\d{3})", "", &val, p) {
                    p = e;
                    acc.extend([cap(&c, 1), cap(&c, 2), cap(&c, 3)]);
                }
                if !acc.is_empty() {
                    tags.set("Accelerometer", join_sp(&acc));
                }
            }
        }
        if !tags.is_empty() {
            handle_text(it, et, tag_tbl, &tags)?;
            return Ok(None);
        }
    }

    // check for DJI telemetry data, eg:
    // "F/3.5, SS 1000, ISO 100, EV 0, GPS (8.6499, 53.1665, 18), D 24.26m,
    //  H 6.00m, H.S 2.10m/s, V.S 0.00m/s \n"
    if let Some(c) = mt(r"GPS \(([-+]?\d*\.\d+),\s*([-+]?\d*\.\d+)", "", &data) {
        et.set("CreateDateAtEnd", n(1)); // set flag indicating the file creation date is at the end
        tags.set("GPSLatitude", cap(&c, 2));
        tags.set("GPSLongitude", cap(&c, 1));
        if let Some(c) = mt(r",\s*H\s+([-+]?\d+\.?\d*)m", "", &data) {
            tags.set("GPSAltitude", cap(&c, 1));
        }
        if let Some(c) = mt(r",\s*H.S\s+([-+]?\d+\.?\d*)", "", &data) {
            tags.set("GPSSpeed", pl::mul(&cap(&c, 1), &mps_to_kph()));
        }
        if let Some(c) = mt(r",\s*D\s+(\d+\.?\d*)m", "", &data) {
            tags.set("Distance", pl::mul(&cap(&c, 1), &mps_to_kph()));
        }
        if let Some(c) = mt(r",\s*V.S\s+([-+]?\d+\.?\d*)", "", &data) {
            tags.set("VerticalSpeed", cap(&c, 1));
        }
        if let Some(c) = mt(r"\bF/(\d+\.?\d*)", "", &data) {
            tags.set("FNumber", cap(&c, 1));
        }
        if let Some(c) = mt(r"\bSS\s+(\d+\.?\d*)", "", &data) {
            tags.set("ExposureTime", pl::div(&n(1), &cap(&c, 1))?);
        }
        if let Some(c) = mt(r"\bEV\s+([-+]?\d+\.?\d*)(/\d+)?", "", &data) {
            tags.set("ExposureCompensation", pl::div(&cap(&c, 1), &or(cap(&c, 2), n(1)))?);
        }
        if let Some(c) = mt(r"\bISO\s+(\d+\.?\d*)", "", &data) {
            tags.set("ISO", cap(&c, 1));
        }
        handle_text(it, et, tag_tbl, &tags)?;
        return Ok(None);
    }

    // check for Mini 0806 dashcam GPS, eg:
    // "A,270519,201555.000,3356.8925,N,08420.2071,W,000.0,331.0M,+01.84,-09.80,-00.61;\n"
    if let Some(c) = mt(r"^A,(\d{2})(\d{2})(\d{2}),(\d{2})(\d{2})(\d{2}(\.\d+)?)", "", &data) {
        tags.set("GPSDateTime", cat(&[&s("20"), &cap(&c, 3), &s(":"), &cap(&c, 2), &s(":"), &cap(&c, 1), &s(" "), &cap(&c, 4), &s(":"), &cap(&c, 5), &s(":"), &cap(&c, 6), &s("Z")]));
        if let Some(c) = mt(r"^A,.*?,.*?,(\d{2})(\d+\.\d+),([NS])", "", &data) {
            let v = pl::add(&cap(&c, 1), &pl::div(&cap(&c, 2), &n(60))?);
            tags.set("GPSLatitude", pl::mul(&v, &n(if cap(&c, 3).eq_str("S") { -1 } else { 1 })));
        }
        if let Some(c) = mt(r"^A,.*?,.*?,.*?,.*?,(\d{3})(\d+\.\d+),([EW])", "", &data) {
            let v = pl::add(&cap(&c, 1), &pl::div(&cap(&c, 2), &n(60))?);
            tags.set("GPSLongitude", pl::mul(&v, &n(if cap(&c, 3).eq_str("W") { -1 } else { 1 })));
        }
        let av = pl::split(&re(",", ""), &data, 0);
        let at = |i: usize| av.get(i).cloned().unwrap_or_default();
        let a8 = at(8);
        if a8.truthy() {
            let mut v = a8;
            if re(r"M$", "").subst(&mut v, false, |_| Vec::new()) > 0 {
                tags.set("GPSAltitude", v);
            }
        }
        let a7 = at(7);
        if a7.truthy() && re(r"^\d+\.\d+$", "").is_match(&a7) {
            tags.set("GPSSpeed", a7); // (NC)
        }
        let a11 = at(11);
        if a11.truthy() {
            let mut v = a11;
            if re(r";\s*$", "").subst(&mut v, false, |_| Vec::new()) > 0 {
                tags.set("Accelerometer", cat(&[&at(9), &s(" "), &at(10), &s(" "), &v]));
            }
        }
        handle_text(it, et, tag_tbl, &tags)?;
        return Ok(None);
    }

    // check for Roadhawk dashcam text
    // ".;;;;D?JL;6+;;;D;R?;4;;;;DBB;;O;;;=D;L;;HO71G>F;-?=J-F:FNJJ;DPP-JF3F;;PL=DBRLBF0F;=?DNF-RD-PF;N;?=JF;;?D=F:*6F~"
    // decoded:
    // "X0000.2340Y-000.0720Z0000.9900G0001.0400$GPRMC,082138,A,5330.6683,N,00641.9749,W,012.5,87.86,050213,002.1,A"
    // (note: "002.1" is magnetic variation and is not decoded; it should have ",E" or ",W" afterward for direction)
    if re(r"\*[0-9A-F]{2}~$", "").is_match(&data) {
        // (ref https://reverseengineering.stackexchange.com/questions/11582/how-to-reverse-engineer-dash-cam-metadata)
        let decode = b"-I8XQWRVNZOYPUTA0B1C2SJ9K.L,M$D3E4F5G6H7";
        let mut chars: Vec<u8> = pl::substr(&data, 0, Some(-4)).to_bytes().to_vec();
        for ch in chars.iter_mut() {
            let nn = *ch as i64 - 43;
            if nn >= 0 && (nn as usize) < decode.len() {
                *ch = decode[nn as usize];
            }
        }
        let buff = Value::bytes(chars);
        if let Some(c) = mt(r"X(.*?)Y(.*?)Z(.*?)G(.*?)\$", "", &buff) {
            // yup. the decoding worked out
            tags.set("Accelerometer", cat(&[&cap(&c, 1), &s(" "), &cap(&c, 2), &s(" "), &cap(&c, 3), &s(" "), &cap(&c, 4)]));
            data = buff.clone();
            pl::set_deref(&data_pt, buff)?; // (process GPRMC below)
        }
    }

    // check for Thinkware format (and other NMEA RMC), eg:
    // "gsensori,4,512,-67,-12,100;GNRMC,161313.00,A,4529.87489,N,07337.01215,W,6.225,35.34,310819,,,A*52..;
    //  CAR,0,0,0,0.0,0,0,0,0,0,0,0,0"
    if let Some(c) = mt(r"[A-Z]{2}RMC,(\d{2})(\d{2})(\d+(\.\d*)?),A?,(\d*?)(\d{1,2}\.\d+),([NS]),(\d*?)(\d{1,2}\.\d+),([EW]),(\d*\.?\d*),(\d*\.?\d*),(\d{2})(\d{2})(\d+)", "", &data) {
        // do some basic sanity checks on the date
        if pl::le(&cap(&c, 13), &n(31)) && pl::le(&cap(&c, 14), &n(12)) && pl::le(&cap(&c, 15), &n(99)) {
            let c15 = cap(&c, 15);
            let year = pl::add(&c15, &n(if pl::ge(&c15, &n(70)) { 1900 } else { 2000 }));
            tags.set("GPSDateTime", pl::sprintf("%.4d:%.2d:%.2d %.2d:%.2d:%.2dZ", &[year, cap(&c, 14), cap(&c, 13), cap(&c, 1), cap(&c, 2), cap(&c, 3)]));
            tags.set("GPSLatitude", deg_min(&cap(&c, 5), &cap(&c, 6), if cap(&c, 7).eq_str("N") { 1 } else { -1 })?);
            tags.set("GPSLongitude", deg_min(&cap(&c, 8), &cap(&c, 9), if cap(&c, 10).eq_str("E") { 1 } else { -1 })?);
            if len_of(&cap(&c, 11)) > 0 {
                tags.set("GPSSpeed", pl::mul(&cap(&c, 11), &knots_to_kph()));
            }
            if len_of(&cap(&c, 12)) > 0 {
                tags.set("GPSTrack", cap(&c, 12));
            }
        }
    }
    if let Some(c) = mt(r"\bgsensori,(.*?)(;|$)", "", &data) {
        tags.set("GSensor", cap(&c, 1));
    }
    if let Some(c) = mt(r"\bCAR,(.*?)(;|$)", "", &data) {
        tags.set("Car", cap(&c, 1));
    }

    if !tags.is_empty() {
        handle_text(it, et, tag_tbl, &tags)?;
        Ok(Some(Value::Undef)) // (the value of "undef %$tags")
    } else {
        et.set("UnknownTextCount", pl::add(&or(et.get("UnknownTextCount"), n(0)), &n(1)));
        // give up trying to decode useful information if we haven't found anything for a while
        if pl::gt(&et.get("UnknownTextCount"), &n(100)) {
            et.set("NoMoreTextDecoding", n(1));
            Ok(Some(n(1)))
        } else {
            Ok(Some(s(""))) // (the value of the false condition)
        }
    }
}

/// Extract embedded metadata from media samples
/// Inputs: 0) ExifTool ref
/// Notes: Also accesses ExifTool RAF*, SET_GROUP1, HandlerType, MetaFormat,
///        ee*, and avcC elements (* = must exist)
/// - may be called either due to ExtractEmbedded option, or ImageDataHash requested
/// - hash includes only video and audio data
pub fn process_samples(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let et = arg(&a, 0);
    process_samples_impl(it, &et)?;
    pl::ret_nothing(want)
}

fn process_samples_impl(it: &mut Interp, et: &Value) -> ER<()> {
    let (raf, ee) = (et.get("RAF"), et.get("ee"));
    let (mut hdr_len, mut hdr_fmt) = (0i64, "C");
    let mut time: Vec<Value> = Vec::new();
    let mut dur: Vec<Value> = Vec::new();
    let mut old_indent = Value::Undef;
    let mut hash = Value::Undef;
    let (mut mdat_offset, mut mdat_size) = (Value::Undef, Value::Undef); // (for range-checking samples when hash is done)

    if !ee.truthy() {
        return Ok(());
    }
    et.delete("ee"); // use only once

    let ee_opt = or(opt(it, et, "ExtractEmbedded")?, n(0));
    let mut typ = or(et.get("HandlerType"), s(""));
    if typ.eq_str("vide") {
        // only process specific types of video streams
        hash = et.get("ImageDataHash");
        // only process specific video types if ExtractEmbedded was used
        // (otherwise we are only here to calculate the audio/video hash)
        if ee_opt.truthy() {
            if ee.get("avcC").truthy() {
                typ = s("avcC");
            } else if ee.get("JPEG").truthy() {
                typ = s("JPEG");
            } else if !hash.truthy() {
                return Ok(());
            }
        }
    } else if typ.eq_str("soun") {
        hash = et.get("ImageDataHash");
        if !hash.truthy() {
            return Ok(());
        }
    } else if !ee_opt.truthy() {
        return Ok(()); // (don't do hash on other types)
    }

    let mut hash_size = n(0);
    let (mut start, mut size) = (ee.get("start"), ee.get("size"));
    //
    // determine sample start offsets from chunk offsets (stco) and sample-to-chunk table (stsc),
    // and sample time/duration from time-to-sample (stts)
    //
    if !(start.truthy() && size.truthy()) {
        if !size.truthy() {
            return Ok(());
        }
        let (stco, stsc, stts) = (ee.get("stco"), ee.get("stsc"), ee.get("stts"));
        if !(stco.truthy() && stsc.truthy() && !stsc.is_empty()) {
            return Ok(());
        }
        start = Value::new_array(vec![]);
        let (mut next_chunk, mut i_chunk) = (n(0), n(1));
        let (mut samples_per_chunk, mut time_count, mut time_delta, mut tm) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef);
        if stts.truthy() && stts.len() > 1 {
            tm = n(0);
            time_count = shift(&stts);
            time_delta = shift(&stts);
        }
        let ts = or(et.get("MediaTS"), n(1));
        let mut chunk_size_list: Vec<Value> = Vec::new(); // total size of each chunk
        for chunk_start in stco.items() {
            if pl::ge(&i_chunk, &next_chunk) && !stsc.is_empty() {
                let e = shift(&stsc);
                samples_per_chunk = e.at(1);
                let _desc_idx = e.at(2);
                if !stsc.is_empty() {
                    next_chunk = stsc.at(0).at(0);
                }
            }
            if pl::lt(&n(size.len() as i64), &pl::add(&n(start.len() as i64), &samples_per_chunk)) {
                warn(it, et, s("Sample size error"))?;
                break;
            }
            if !(defined(&chunk_start) && len_of(&chunk_start) > 0) {
                break;
            }
            let mut sample_start = chunk_start.clone();
            let mut chunk_size = n(0);
            let mut i = n(0);
            'sample: loop {
                start.push(sample_start.clone());
                if defined(&tm) {
                    while !time_count.truthy() {
                        if stts.len() < 2 {
                            tm = Value::Undef;
                            break 'sample;
                        }
                        time_count = shift(&stts);
                        time_delta = shift(&stts);
                    }
                    time.push(pl::div(&tm, &ts)?);
                    dur.push(pl::div(&time_delta, &ts)?);
                    tm = pl::add(&tm, &time_delta);
                    time_count = pl::sub(&time_count, &n(1));
                }
                // (eventually should use the description indices: $descIdx)
                let last = size.at(start.len() as i64 - 1);
                chunk_size = pl::add(&chunk_size, &last);
                i = pl::add(&i, &n(1));
                if pl::ge(&i, &samples_per_chunk) {
                    break;
                }
                sample_start = pl::add(&sample_start, &size.at(start.len() as i64 - 1));
            }
            chunk_size_list.push(chunk_size);
            i_chunk = pl::add(&i_chunk, &n(1));
        }
        if start.len() != size.len() {
            warn(it, et, s("Incorrect sample start/size count"))?;
            return Ok(());
        }
        // process as chunks if we are only interested in calculating hash
        if typ.eq_str("soun") || typ.eq_str("vide") {
            start = stco;
            size = Value::new_array(chunk_size_list);
        }
    }
    //
    // extract and parse the sample data
    //
    let tag_tbl = get_tag_table(it, "Image::ExifTool::QuickTime::Stream")?;
    let verbose = opt(it, et, "Verbose")?;
    let meta_format = or(et.get("MetaFormat"), s(""));
    let tell = raf_tell(it, &raf)?;

    if verbose.truthy() {
        m(it, et, "VPrint", vec![n(0), s("---- Extract Embedded ----\n")])?;
        old_indent = et.get("INDENT");
        et.set("INDENT", s(""));
    }
    if hash.truthy() {
        mdat_size = et.get("MediaDataSize");
        if defined(&mdat_size) {
            mdat_offset = et.get("MediaDataOffset");
        }
    }
    // get required information from avcC box if parsing video data
    if typ.eq_str("avcC") {
        let b = get8u(it, &pl::sref(ee.get("avcC")), n(4))?;
        hdr_len = (b.to_i64() & 0x03) + 1;
        hdr_fmt = if hdr_len == 4 {
            "N"
        } else if hdr_len == 2 {
            "n"
        } else {
            "C"
        };
        runtime::require(it, "Image::ExifTool::H264")?;
    }
    let process_by_meta_format = lex_hash(it, "processByMetaFormat");

    // loop through all samples
    let mut i = 0i64;
    while i < start.len() as i64 && i < size.len() as i64 {
        let ii = i;
        i += 1;
        let (t_i, d_i) = (time.get(ii as usize).cloned().unwrap_or_default(), dur.get(ii as usize).cloned().unwrap_or_default());
        let start_i = start.at(ii);

        // initialize our flags for setting GPSDateTime
        et.delete("FoundGPSLatitude");
        et.delete("FoundGPSDateTime");

        // range check the sample data for hash if necessary
        let mut size = size.at(ii);
        if defined(&mdat_offset) {
            if pl::lt(&start_i, &mdat_offset) {
                warn(it, et, cat(&[&s("Sample "), &n(ii), &s(" for '"), &typ, &s("' data is before start of mdat")]))?;
            } else if pl::gt(&pl::add(&start_i, &size), &pl::add(&mdat_offset, &mdat_size)) {
                warn(it, et, cat(&[&s("Sample "), &n(ii), &s(" for '"), &typ, &s("' data runs off end of mdat")]))?;
                size = pl::sub(&pl::add(&mdat_offset, &mdat_size), &start_i);
                if pl::lt(&size, &n(0)) {
                    size = n(0);
                }
            }
        }
        // read the sample data
        if !raf_seek(it, &raf, start_i.clone(), 0)? {
            warn(it, et, cat(&[&s("Seek error in "), &typ, &s(" data")]))?;
            continue;
        }
        let mut buff = Value::Undef;
        let nr = raf_read(it, &raf, &mut buff, size.clone())?;
        if !pl::num_eq(&nr, &size) {
            warn(it, et, cat(&[&s("Error reading "), &typ, &s(" data")]))?;
            if !nr.truthy() {
                continue;
            }
            size = nr;
        }
        if hash.truthy() {
            m(it, &hash, "add", vec![buff.clone()])?;
            hash_size = pl::add(&hash_size, &n(len_of(&buff)));
        }
        if typ.eq_str("avcC") {
            if len_of(&buff) <= hdr_len {
                continue;
            }
            // scan through all NAL units and send them to ParseH264Video()
            let mut pos = 0i64;
            loop {
                let len = pl::unpack1(&format!("x{pos}{hdr_fmt}"), &buff)?;
                if pl::gt(&pl::add(&n(pos + hdr_len), &len), &n(len_of(&buff))) {
                    break;
                }
                let tmp = cat(&[&Value::bytes(b"\0\0\0\x01"), &pl::substr(&buff, pos + hdr_len, Some(len.to_i64()))]);
                pl::call(it, "Image::ExifTool::H264::ParseH264Video", vec![et.clone(), pl::sref(tmp)])?;
                pos = pl::add(&n(pos + hdr_len), &len).to_i64();
                if pos + hdr_len >= len_of(&buff) {
                    break;
                }
            }
            if et.get("GotNAL06").truthy() && pl::lt(&ee_opt, &n(3)) {
                break;
            }
            continue;
        }
        if pl::gt(&verbose, &n(1)) {
            let sg1 = et.get("SET_GROUP1");
            let hdr = if sg1.truthy() { cat(&[&sg1, &s(" Type='"), &typ, &s("' Format='"), &meta_format, &s("'")]) } else { cat(&[&s("Type='"), &typ, &s("'")]) };
            m(it, et, "VPrint", vec![n(1), cat(&[&hdr, &s(", Sample "), &n(ii + 1), &s(" of "), &n(start.len() as i64), &s(" ("), &size, &s(" bytes)\n")])])?;
            m(it, et, "VerboseDump", vec![pl::sref(buff.clone()), s("Addr"), start_i.clone()])?;
        }
        if typ.eq_str("text") ||
            // (PNDM is normally 'text', but was sbtl/tx3g in concatenated Garmin sample output_3videos.mp4)
            (typ.eq_str("sbtl") && meta_format.eq_str("tx3g") && re(r"^..PNDM", "s").is_match(&buff))
        {
            let mut handled = Value::Undef;
            found_some(it, et, &tag_tbl, &t_i, &d_i)?;
            if !re(r"^\$BEGIN", "").is_match(&buff) {
                // remove ending "encd" box if it exists
                if re(r"\0\0\0\x0cencd\0\0\x01\0$", "").subst(&mut buff, false, |_| Vec::new()) > 0 {
                    size = pl::sub(&size, &n(12));
                }
                // cameras such as the CanonPowerShotN100 store ASCII time codes with a
                // leading 2-byte integer giving the length of the string
                // (and chapter names start with a 2-byte integer too)
                if pl::ge(&size, &n(2)) && pl::num_eq(&pl::unpack1("n", &buff)?, &pl::sub(&size, &n(2))) {
                    if pl::num_eq(&size, &n(2)) {
                        continue;
                    }
                    buff = pl::substr(&buff, 2, None);
                }
                let mut val = Value::Undef;
                // check for encrypted GPS text as written by E-PRANCE B47FS camera
                let pndm = mt(r"^(\0.{3})?PNDM", "s", &buff);
                if re(r"^\0", "").is_match(&buff) && re(r"\x0a$", "").is_match(&buff) && len_of(&buff) > 5 {
                    // decode simple ASCII difference cipher,
                    // based on known value of 4th-last char = '*'
                    let dif = b'*' as i64 - pl::substr(&buff, -4, Some(1)).to_bytes().first().copied().unwrap_or(0) as i64;
                    let mid = pl::substr(&buff, 1, Some(-1)).to_bytes();
                    let tmp = Value::bytes(mid.iter().map(|&b| ((b as i64 + dif) & 0xff) as u8).collect::<Vec<u8>>());
                    if pl::gt(&verbose, &n(2)) {
                        m(it, et, "VPrint", vec![n(0), s("[decrypted text]\n")])?;
                        m(it, et, "VerboseDump", vec![pl::sref(tmp.clone())])?;
                    }
                    if let Some(c) = mt(r"^(.*?)(\$[A-Z]{2}RMC.*)", "s", &tmp) {
                        val = cap(&c, 1);
                        buff = cap(&c, 2);
                        pl::tr(&mut val, "\\t", " ", "");
                        if len_of(&val) > 0 {
                            ht(it, et, &tag_tbl, "RawGSensor", val.clone())?;
                        }
                    }
                } else if let Some(c) = pndm {
                    // Garmin Dashcam format (actually binary, not text)
                    let nn = if cap(&c, 1).truthy() { 4 } else { 0 }; // skip leading 4-byte size word if it exists
                    if len_of(&buff) < 20 + nn {
                        continue;
                    }
                    let bp = pl::sref(buff.clone());
                    let v = get32s(it, &bp, n(12 + nn))?;
                    ht(it, et, &tag_tbl, "GPSLatitude", pl::div(&pl::mul(&v, &n(180)), &n(0x80000000))?)?;
                    let v = get32s(it, &bp, n(16 + nn))?;
                    ht(it, et, &tag_tbl, "GPSLongitude", pl::div(&pl::mul(&v, &n(180)), &n(0x80000000))?)?;
                    let v = get16u(it, &bp, n(8 + nn))?;
                    ht(it, et, &tag_tbl, "GPSSpeed", pl::mul(&v, &mph_to_kph()))?;
                    set_gps_dt(it, et, &tag_tbl, t_i.clone(), n(1))?;
                    continue; // all done (don't store/process as text)
                }
                if !(defined(&val) || re(r"\0[^\0]", "").is_match(&buff)) {
                    // just store any other plain text
                    ht(it, et, &tag_tbl, "Text", buff.clone())?;
                    handled = n(1);
                }
            }
            process_text_impl(it, et, pl::sref(buff.clone()), &tag_tbl, &handled)?;
        } else if process_by_meta_format.get(typ.to_bytes()).truthy() {
            if tag_tbl.get(meta_format.to_bytes()).truthy() {
                let bp = pl::sref(buff.clone());
                let tag_info = m(it, et, "GetTagInfo", vec![tag_tbl.clone(), meta_format.clone(), bp.clone()])?;
                if tag_info.truthy() && (!tag_info.get("Unknown").truthy() || pl::ge(&et.get("OPTIONS").get("Unknown"), &tag_info.get("Unknown"))) {
                    found_some(it, et, &tag_tbl, &t_i, &d_i)?;
                    et.set("ee", ee.clone()); // need ee information for 'keys'
                    ht_v(it, et, &tag_tbl, meta_format.clone(), Value::Undef, vec![("DataPt", bp.clone()), ("Base", start_i.clone()), ("TagInfo", tag_info.clone())])?;
                    et.delete("ee");
                    // synthesize GPSDateTime if necessary for djmd metadata
                    if meta_format.eq_str("djmd") {
                        if defined(&et.get("GPSLatitude")) && defined(&et.get("GPSLongitude")) && !et.get("GPSDateTime").truthy() {
                            set_gps_dt(it, et, &tag_tbl, t_i.clone(), n(1))?; // (NC)
                        }
                        et.delete("GPSLatitude");
                        et.delete("GPSLongitude");
                        et.delete("GPSDateTime");
                    }
                } else if meta_format.eq_str("camm") && re(r"^X", "").is_match(&buff) {
                    // seen 'camm' metadata in this format (X/Y/Z acceleration and G force? + GPRMC + ?)
                    // "X0000.0000Y0000.0000Z0000.0000G0000.0000$GPRMC,000125,V,,,,,000.0,,280908,002.1,N*71~, 794021  \x0a"
                    found_some(it, et, &tag_tbl, &t_i, &d_i)?;
                    if let Some(c) = mt(r"X(.*?)Y(.*?)Z(.*?)G(.*?)\$", "", &buff) {
                        ht(it, et, &tag_tbl, "Accelerometer", cat(&[&cap(&c, 1), &s(" "), &cap(&c, 2), &s(" "), &cap(&c, 3), &s(" "), &cap(&c, 4)]))?;
                    }
                    process_text_impl(it, et, pl::sref(buff.clone()), &tag_tbl, &Value::Undef)?;
                }
            } else if verbose.truthy() {
                m(it, et, "VPrint", vec![n(0), cat(&[&s("Unknown "), &typ, &s(" format ("), &meta_format, &s(")")])])?;
            }
        } else if typ.eq_str("gps ") {
            // (ie. GPSDataList tag)
            if re(r"^....freeGPS ", "s").is_match(&buff) {
                // parse freeGPS data unless done already in brute-force scan
                // (some videos don't reference all freeGPS info from 'gps ' table, eg. INNOV,
                //  and some videos don't put 'gps ' data in mdat, eg XGODY 12" 4K Dashcam)
                if et.get("FoundGPSByScan").truthy() {
                    break;
                }
                // decode "freeGPS " data (Novatek and others)
                let di = hash_of(vec![("DataPt", pl::sref(buff.clone())), ("DataPos", start_i.clone()), ("SampleTime", t_i.clone()), ("SampleDuration", d_i.clone())]);
                process_free_gps(it, vec![et.clone(), di, tag_tbl.clone()], Want::Scalar)?;
            }
        } else if tag_tbl.get(typ.to_bytes()).truthy() {
            let bp = pl::sref(buff.clone());
            let tag_info = m(it, et, "GetTagInfo", vec![tag_tbl.clone(), typ.clone(), bp.clone()])?;
            if tag_info.truthy() {
                found_some(it, et, &tag_tbl, &t_i, &d_i)?;
                ht_v(it, et, &tag_tbl, typ.clone(), Value::Undef, vec![("DataPt", bp), ("Base", start_i.clone()), ("TagInfo", tag_info)])?;
            }
        }
        // generate approximate GPSDateTime if necessary
        if et.get("FoundGPSLatitude").truthy() && !et.get("FoundGPSDateTime").truthy() {
            set_gps_dt(it, et, &tag_tbl, t_i.clone(), Value::Undef)?;
        }
    }
    if verbose.truthy() {
        let st = if typ.eq_str("soun") { "Audio" } else { "Video" };
        if hash_size.truthy() {
            m(it, et, "VPrint", vec![n(0), cat(&[&et.get("INDENT"), &s("(ImageDataHash: "), &hash_size, &s(" bytes of "), &s(st), &s(" data)\n")])])?;
        }
        et.set("INDENT", old_indent);
        m(it, et, "VPrint", vec![n(0), s("--------------------------\n")])?;
    }
    // clean up
    raf_seek(it, &raf, tell, 0)?; // restore original file position
    et.delete("DOC_NUM");
    et.set("HandlerType", s(""));
    Ok(())
}

/// `map { SignedInt32 / DIV } LIST`
fn signed_div(v: Vec<Value>, div: Value) -> ER<Vec<Value>> {
    v.iter().map(|x| pl::div(&signed_int32_of(x), &div)).collect()
}

/// `$et->FoundTag(GPSType => N) if $debug`
fn gps_type(it: &mut Interp, et: &Value, debug: &Value, t: Value) -> ER<()> {
    if debug.truthy() {
        m(it, et, "FoundTag", vec![s("GPSType"), t])?;
    }
    Ok(())
}

/// The 6 values of `gmtime($t)` in list context, as perl gives them.
fn gmtime_list(it: &mut Interp, t: &Value) -> ER<Vec<Value>> {
    let saved = it.get_scalar("main::@");
    let (v, _) = pl::eval_site(it, &s("join ',', gmtime($t)"), "main", &[("t", t.clone())], Want::Scalar);
    it.set_scalar("main::@", saved);
    Ok(pl::split(&re(",", ""), &v, 0))
}

/// Process "freeGPS " data blocks
/// Inputs: 0) ExifTool ref, 1) dirInfo ref {DataPt,SampleTime,SampleDuration}, 2) tagTable ref
/// Returns: 1 on success (or 0 on unrecognized or "measurement-void" GPS data)
/// Notes:
pub fn process_free_gps(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_tbl) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    pl::at(1641);
    let mut data = pl::sderef(&data_pt)?;
    let dir_len = len_of(&data);
    let u = Value::Undef;
    let (mut yr, mut mon, mut day, mut hr, mut min, mut sec, mut lbl, mut ddd, mut done) = (u.clone(), u.clone(), u.clone(), u.clone(), u.clone(), u.clone(), u.clone(), u.clone(), u.clone());
    let (mut lat, mut lat_ref, mut lon, mut lon_ref, mut spd, mut trk, mut alt) = (u.clone(), u.clone(), u.clone(), u.clone(), u.clone(), u.clone(), u.clone());
    let mut acc: Vec<Value> = Vec::new();
    let mut xtra: Vec<Value> = Vec::new();
    let dp = |d: &Value| pl::sref(d.clone());

    if dir_len < 82 {
        return ret(n(0));
    }

    let debug = opt(it, &et, "Debug")?;
    let old_order = get_byte_order(it)?;
    set_byte_order(it, s("II"))?;
    et.set("FoundEmbedded", n(1));

    if sub_str(&data, 18, 8).to_bytes()[..] == b"\xaa\xaa\xf2\xe1\xf0\xee\x54\x54"[..] {
        gps_type(it, &et, &debug, n(1))?;
        // (this is very similar to the encrypted text format)
        // decode encrypted ASCII-based GPS (DashCam Azdome GS63H, ref 5)
        // header looks like this in my sample:
        //  0000: 00 00 80 00 66 72 65 65 47 50 53 20 05 01 00 00 [....freeGPS ....]
        //  0010: 01 03 aa aa f2 e1 f0 ee 54 54 98 9a 9b 92 9a 93 [........TT......]
        //  0020: 98 9e 98 98 9e 93 98 92 a6 9f 9f 9c 9d ed fa 8a [................]
        // decrypted (from byte 18):
        //  0000: 00 00 58 4b 5a 44 fe fe 32 30 31 38 30 39 32 34 [..XKZD..20180924]
        //  0010: 32 32 34 39 32 38 0c 35 35 36 37 47 50 20 20 20 [224928.5567GP   ]
        //  0020: 00 00 00 00 00 03 4e 34 30 34 36 34 33 35 30 57 [......N40464350W]
        //  0030: 30 30 37 30 34 30 33 30 38 30 30 30 30 30 30 30 [0070403080000000]
        //  0040: 37 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 [7...............]
        //  [...]
        //  00a0: 00 00 00 00 00 00 00 00 00 00 00 00 00 2b 30 39 [.............+09]
        //  00b0: 33 2d 30 30 33 2d 30 30 35 00 00 00 00 00 00 00 [3-003-005.......]
        // header looks like this for EEEkit gps:
        //  0000: 00 00 04 00 66 72 65 65 47 50 53 20 f0 03 00 00 [....freeGPS ....]
        //  0010: 01 03 aa aa f2 e1 f0 ee 54 54 98 9a 98 9a 9a 9f [........TT......]
        //  0020: 9b 93 9b 9c 98 99 99 9f a6 9a 9a 98 9a 9a 9f 9b [................]
        //  0030: 93 9b 9c 98 99 99 9c a9 e4 99 9d 9e 9f 98 9e 9b [................]
        //  0040: 9c fd 9b 98 98 98 9f 9f 9a 9a 93 81 9a 9b 9d 9f [................]
        // decrypted (from byte 18):
        //  0000: 00 00 58 4b 5a 44 fe fe 32 30 32 30 30 35 31 39 [..XKZD..20200519]
        //  0010: 31 36 32 33 33 35 0c 30 30 32 30 30 35 31 39 31 [162335.002005191]
        //  0020: 36 32 33 33 36 03 4e 33 37 34 35 32 34 31 36 57 [62336.N37452416W]
        //  0030: 31 32 32 32 35 35 30 30 39 2b 30 31 37 35 30 31 [122255009+017501]
        //  0040: 31 2b 30 31 34 2b 30 30 32 2b 30 32 36 2b 30 31 [1+014+002+026+01]
        let mut nn = dir_len - 18;
        if nn > 0x101 {
            nn = 0x101;
        }
        let buf2 = xor_bytes(&sub_str(&data, 18, nn), 0xaa);
        if pl::gt(&opt(it, &et, "Verbose")?, &n(1)) {
            m(it, &et, "VPrint", vec![n(1), s("[decrypted freeGPS data]")])?;
            m(it, &et, "VerboseDump", vec![dp(&buf2)])?;
        }
        // (extract longitude as 9 digits, not 8, ref PH)
        if let Some(c) = mt(r"^.{8}(\d{4})(\d{2})(\d{2})(\d{2})(\d{2})(\d{2}).(.{15})([NS])(\d{8})([EW])(\d{9})(\d{8})?", "s", &buf2) {
            yr = cap(&c, 1);
            mon = cap(&c, 2);
            day = cap(&c, 3);
            hr = cap(&c, 4);
            min = cap(&c, 5);
            sec = cap(&c, 6);
            lbl = cap(&c, 7);
            lat_ref = cap(&c, 8);
            lat = pl::div(&cap(&c, 9), &f(1e4))?;
            lon_ref = cap(&c, 10);
            lon = pl::div(&cap(&c, 11), &f(1e4))?;
            spd = cap(&c, 12);
            if defined(&spd) {
                // (Azdome)
                spd = pl::add(&spd, &n(0)); // remove leading 0's
            } else if let Some(c) = mt(r"^.{57}([-+]\d{4})(\d{3})", "s", &buf2) {
                // (EEEkit)
                // $alt = $1 + 0;  (doesn't look right for my sample, but the Ambarella A12 text has this)
                spd = pl::add(&cap(&c, 2), &n(0));
            }
        }
        // extract accelerometer data (ref PH)
        if let Some(c) = mt(r"^.{65}(([-+]\d{3})([-+]\d{3})([-+]\d{3})([-+]\d{3})*)", "s", &buf2) {
            let mut t = cap(&c, 1);
            acc = vec![pl::div(&cap(&c, 2), &n(100))?, pl::div(&cap(&c, 3), &n(100))?, pl::div(&cap(&c, 4), &n(100))?];
            re(r"([-+])", "").subst(&mut t, true, |c| [b" ", &cap(c, 1).to_bytes()[..]].concat());
            re(r"^ ", "").subst(&mut t, false, |_| Vec::new());
            it.set_scalar("main::_", t.clone()); // ($_ is assigned here)
            xtra.push(s("AccelerometerData"));
            xtra.push(t);
        } else if let Some(c) = mt(r"^.{173}([-+]\d{3})([-+]\d{3})([-+]\d{3})", "s", &buf2) {
            // (Azdome)
            // (Adzome may contain acc and date/time/label even if GPS doesn't exist)
            acc = vec![pl::div(&cap(&c, 1), &n(100))?, pl::div(&cap(&c, 2), &n(100))?, pl::div(&cap(&c, 3), &n(100))?];
            if !defined(&yr) {
                if let Some(c) = mt(r"^.{8}(\d{4})(\d{2})(\d{2})(\d{2})(\d{2})(\d{2}).(.{15})", "s", &buf2) {
                    yr = cap(&c, 1);
                    mon = cap(&c, 2);
                    day = cap(&c, 3);
                    hr = cap(&c, 4);
                    min = cap(&c, 5);
                    sec = cap(&c, 6);
                    lbl = cap(&c, 7);
                }
            }
        }
        if defined(&lbl) {
            re(r"\0.*", "s").subst(&mut lbl, false, |_| Vec::new());
            re(r"\s+$", "").subst(&mut lbl, false, |_| Vec::new()); // truncate at null and remove trailing spaces
            if len_of(&lbl) > 0 {
                xtra.push(s("UserLabel"));
                xtra.push(lbl.clone());
            }
        }
    } else if let Some(c) = mt(r"^.{52}(\d{4})(\d{2})(\d{2})(\d{2})(\d{2})(\d{2})", "s", &data) {
        gps_type(it, &et, &debug, n(2))?;
        // decode NMEA-format GPS data (Nextbase 512GW dashcam, ref PH)
        // header looks like this in my sample:
        //  0000: 00 00 80 00 66 72 65 65 47 50 53 20 40 01 00 00 [....freeGPS @...]
        //  0010: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 [................]
        //  0020: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 [................]
        //  0030: 00 00 00 00 32 30 31 38 30 39 31 39 31 30 30 39 [....201809191009]
        //  0040: 35 39 00 00 1c 01 00 00 06 00 00 00 ef ff ff ff [59..............]
        //  0050: 20 24 47 50 52 4d 43 2c 30 38 30 39 35 31 2e 30 [ $GPRMC,080951.0]
        //  0060: 30 30 2c 41 2c 35 32 30 37 2e 39 30 39 37 2c 4e [00,A,5207.9097,N]
        //  0070: 2c 30 30 35 30 35 2e 35 31 37 35 2c 45 2c 35 35 [,00505.5175,E,55]
        //  0080: 2e 31 31 2c 31 32 35 2e 35 38 2c 31 39 30 39 31 [.11,125.58,19091]
        //  0090: 38 2c 2c 2c 41 2a 35 39 0d 0a 00 00 00 00 00 00 [8,,,A*59........]
        xtra.push(s("CameraDateTime"));
        xtra.push(cat(&[&cap(&c, 1), &s(":"), &cap(&c, 2), &s(":"), &cap(&c, 3), &s(" "), &cap(&c, 4), &s(":"), &cap(&c, 5), &s(":"), &cap(&c, 6)]));
        if let Some(c) = mt(r"\$[A-Z]{2}RMC,(\d{2})(\d{2})(\d+(\.\d*)?),A?,(\d+\.\d+),([NS]),(\d+\.\d+),([EW]),(\d*\.?\d*),(\d*\.?\d*),(\d{2})(\d{2})(\d+)", "s", &data) {
            lat = cap(&c, 5);
            lat_ref = cap(&c, 6);
            lon = cap(&c, 7);
            lon_ref = cap(&c, 8);
            let c13 = cap(&c, 13);
            yr = pl::add(&c13, &n(if pl::ge(&c13, &n(70)) { 1900 } else { 2000 }));
            mon = cap(&c, 12);
            day = cap(&c, 11);
            hr = cap(&c, 1);
            min = cap(&c, 2);
            sec = cap(&c, 3);
            if len_of(&cap(&c, 9)) > 0 {
                spd = pl::mul(&cap(&c, 9), &knots_to_kph());
            }
            if len_of(&cap(&c, 10)) > 0 {
                trk = cap(&c, 10);
            }
        }
        if let Some(c) = mt(r"\$[A-Z]{2}GGA,(\d{2})(\d{2})(\d+(\.\d*)?),(\d+\.\d+),([NS]),(\d+\.\d+),([EW]),[1-6]?,(\d+)?,(\.\d+|\d+\.?\d*)?,(-?\d+\.?\d*)?,M?", "s", &data) {
            if !defined(&yr) {
                hr = cap(&c, 1);
                min = cap(&c, 2);
                sec = cap(&c, 3);
                lat = cap(&c, 5);
                lat_ref = cap(&c, 6);
                lon = cap(&c, 7);
                lon_ref = cap(&c, 8);
            }
            alt = cap(&c, 11);
            xtra.splice(0..0, [s("GPSSatellites"), cap(&c, 9)]);
            xtra.splice(0..0, [s("GPSDOP"), cap(&c, 10)]);
        }
        if defined(&lat) {
            // extract accelerometer readings if GPS was valid
            // and change to signed integer and divide by 256
            acc = signed_div(pl::unpack("x68V3", &data)?, n(256))?;
        }
    } else if let Some(c) = mt(r"^(.{37}|.{85})\0\0\0A([NS])([EW])\0", "s", &data) {
        if len_of(&cap(&c, 1)) == 85 {
            // Kenwood DRV-A510W (header is 48 bytes longer)
            // 0000: 00 00 40 00 66 72 65 65 47 50 53 20 f0 03 00 00 [..@.freeGPS ....]
            // 0010: 4d 4e 3a 44 52 56 2d 41 35 31 30 57 40 56 31 2e [MN:DRV-A510W@V1.]
            // 0020: 37 5f 42 44 5a 49 43 5a 5f 43 00 00 00 00 00 00 [7_BDZICZ_C......]
            // 0030: 00 00 00 00 00 00 00 00 3a 3a 73 74 61 72 74 40 [........::start@]
            // 0040: 0c 00 00 00 10 00 00 00 16 00 00 00 ea 07 00 00 [................]
            // 0050: 02 00 00 00 1c 00 00 00 41 4e 57 00 55 e7 a5 45 [........ANW.U..E]
            // 0060: d7 84 52 43 52 b8 d8 41 48 21 9a 43 f9 ff ff ff [..RCR..AH!.C....]
            // 0070: ff ff ff ff 01 00 00 00 00 00 00 00 00 00 00 00 [................]
            data = pl::substr(&data, 48, None);
            pl::set_deref(&data_pt, data.clone())?;
        }
        lat_ref = cap(&c, 2);
        lon_ref = cap(&c, 3);
        let v = pl::unpack("x16V6", &data)?;
        hr = arg(&v, 0);
        min = arg(&v, 1);
        sec = arg(&v, 2);
        yr = arg(&v, 3);
        mon = arg(&v, 4);
        day = arg(&v, 5);
        // test for base64-encoded and encrypted lucky gps strings
        let (mut not_enc, mut not_str, mut lt, mut ln) = (false, false, Value::Undef, Value::Undef);
        if len_of(&data) < 0x78 {
            not_enc = true;
            not_str = true;
        } else {
            lt = sub_str(&data, 0x2c, 20); // latitude
            ln = sub_str(&data, 0x40, 20); // longitude
            for x in [&lt, &ln] {
                if !re(r"^[A-Za-z0-9+/]{8,20}={0,2}\0*$", "").is_match(x) {
                    not_enc = true;
                    break;
                }
            }
            for x in [&lt, &ln] {
                if !re(r"^\d{1,5}\.\d+\0*$", "").is_match(x) {
                    not_str = true;
                    break;
                }
            }
        }
        if not_enc && not_str {
            gps_type(it, &et, &debug, n(3))?;
            // decode freeGPS from ViofoA119v3 dashcam (similar to Novatek GPS format)
            // 0000: 00 00 40 00 66 72 65 65 47 50 53 20 f0 03 00 00 [..@.freeGPS ....]
            // 0010: 05 00 00 00 2f 00 00 00 03 00 00 00 13 00 00 00 [..../...........]
            // 0020: 09 00 00 00 1b 00 00 00 41 4e 57 00 25 d1 99 45 [........ANW.%..E]
            // 0030: f1 47 40 46 66 66 d2 41 85 eb 83 41 00 00 00 00 [.G@Fff.A...A....]
            if pl::ge(&yr, &n(2000)) {
                // Kenwood dashcam sometimes stores absolute year and local time
                // (but sometimes year since 2000 and UTC time in same video!)
                runtime::require(it, "Time::Local")?;
                let time = pl::call(it, "Image::ExifTool::TimeLocal", vec![sec.clone(), min.clone(), hr.clone(), day.clone(), pl::sub(&mon, &n(1)), yr.clone()])?;
                let g = gmtime_list(it, &time)?;
                sec = arg(&g, 0);
                min = arg(&g, 1);
                hr = arg(&g, 2);
                day = arg(&g, 3);
                mon = arg(&g, 4);
                yr = arg(&g, 5);
                yr = pl::add(&yr, &n(1900));
                mon = pl::add(&mon, &n(1));
                warn_lvl(it, &et, s("Converting GPSDateTime to UTC based on local time zone"), 1)?;
            }
            lat = get_float(it, &data_pt, n(0x2c))?;
            lon = get_float(it, &data_pt, n(0x30))?;
            spd = pl::mul(&get_float(it, &data_pt, n(0x34))?, &knots_to_kph());
            trk = get_float(it, &data_pt, n(0x38))?;
            // (may be all zeros or int16u counting from 1 to 6 if not valid)
            let tmp = sub_str(&data, 60, 12);
            if tmp.to_bytes()[..] != b"\0\0\0\0\0\0\0\0\0\0\0\0"[..] && tmp.to_bytes()[..] != b"\x01\0\x02\0\x03\0\x04\0\x05\0\x06\0"[..] {
                acc = signed_div(pl::unpack("V3", &tmp)?, n(256))?;
            }
        } else {
            gps_type(it, &et, &debug, n(4))?;
            // decode freeGPS from E-ACE B44 dashcam
            // 0000: 00 00 40 00 66 72 65 65 47 50 53 20 f0 03 00 00 [..@.freeGPS ....]
            // 0010: 08 00 00 00 22 00 00 00 01 00 00 00 18 00 00 00 [...."...........]
            // 0020: 08 00 00 00 10 00 00 00 41 4e 45 00 67 4e 69 69 [........ANE.gNii]
            // 0030: 5a 38 4a 54 74 48 63 61 36 74 77 3d 00 00 00 00 [Z8JTtHca6tw=....]
            // 0040: 68 74 75 69 5a 4d 4a 53 73 58 55 58 37 4e 6f 3d [htuiZMJSsXUX7No=]
            // 0050: 00 00 00 00 64 3b ac 41 e1 3a 1d 43 2b 01 00 00 [....d;.A.:.C+...]
            // 0060: fd ff ff ff 43 00 00 00 32 4a 37 31 50 70 55 48 [....C...2J71PpUH]
            // 0070: 37 69 68 66 00 00 00 00 00 00 00 00 00 00 00 00 [7ihf............]
            // (16-byte string at 0x68 is base64 encoded and encrypted 'luckychip' string)
            spd = pl::mul(&get_float(it, &data_pt, n(0x54))?, &knots_to_kph());
            trk = get_float(it, &data_pt, n(0x58))?;
            acc = pl::unpack("x92V3", &data)?.iter().map(signed_int32_of).collect();
            // (accelerometer scaling is roughly 1G=250-300, but it varies depending on the axis,
            //  so leave the values as raw.  The axes are positive acceleration up,left,forward)
            if not_enc {
                // (not encrypted)
                lat = lt.clone();
                re(r"\0+$", "").subst(&mut lat, false, |_| Vec::new());
                lon = ln.clone();
                re(r"\0+$", "").subst(&mut lon, false, |_| Vec::new());
            } else {
                // decode base64 strings
                runtime::require(it, "Image::ExifTool::XMP")?;
                lt = pl::call(it, "Image::ExifTool::XMP::DecodeBase64", vec![lt])?.deref();
                ln = pl::call(it, "Image::ExifTool::XMP::DecodeBase64", vec![ln])?.deref();
                // try various keys to decrypt lat/lon
                let lucky_keys = lex_array(it, "luckyKeys");
                let (mut ch, mut key) = (b'a', lucky_keys.at(0));
                for i in 0..20 {
                    if i != 0 {
                        key = lucky_keys.at(1);
                        let c = ch;
                        re(r"#", "").subst(&mut key, true, |_| vec![c]);
                        ch += 1;
                    }
                    lat = lucky(&lt, &key);
                    if !re(r"^\d{1,4}\.\d+$", "").is_match(&lat) {
                        lat = Value::Undef;
                        continue;
                    }
                    lon = lucky(&ln, &key);
                    if !re(r"^\d{1,5}\.\d+$", "").is_match(&lon) {
                        lon = Value::Undef;
                        continue;
                    }
                    break;
                }
                if !lon.truthy() {
                    warn(it, &et, s("Unknown encryption for latitude/longitude"))?;
                }
            }
        }
    } else if let Some(c) = mt(r"^(.{16}|.{48}|.{80})LIGOGPSINFO\0", "s", &data).filter(|c| len_of(&data) >= len_of(&cap(c, 1)) + 0x84) {
        gps_type(it, &et, &debug, n(5))?;
        let pos = len_of(&cap(&c, 1));
        // iiway s1 dual dash cam - offset 16, encrypted and fuzzed with scale 1
        //  0000: 00 00 40 00 66 72 65 65 47 50 53 20 f0 03 00 00 [..@.freeGPS ....]
        //  0010: 4c 49 47 4f 47 50 53 49 4e 46 4f 00 00 00 00 05 [LIGOGPSINFO.....]
        //  0020: 0a 00 00 00 23 23 23 23 6a 00 00 00 c0 20 20 20 [....####j....   ]
        //  0030: 20 f0 12 10 12 22 e1 0e 10 12 2f 90 10 13 02 f2 [ ...."..../.....]
        // XGODY 12" 4K Dashcam - offset 16, encrypted and fuzzed with scale 1
        //  0000: 00 00 00 a8 66 72 65 65 47 50 53 20 98 00 00 00 [....freeGPS ....]
        //  0010: 4c 49 47 4f 47 50 53 49 4e 46 4f 00 00 00 00 05 [LIGOGPSINFO.....]
        //  0020: cd 61 00 00 23 23 23 23 6d 00 00 00 c1 ec 41 20 [.a..####m.....A ]
        //  0030: 20 f0 12 10 12 24 e5 0e 10 11 2f 92 10 12 00 f6 [ ....$..../.....]
        // ABASK A8 4K Dashcam - offset 16, encrypted and fuzzed with scale 3
        //  0000: 00 00 40 00 66 72 65 65 47 50 53 20 f0 03 00 00 [..@.freeGPS ....]
        //  0010: 4c 49 47 4f 47 50 53 49 4e 46 4f 00 00 00 00 05 [LIGOGPSINFO.....]
        //  0020: 00 00 00 00 23 23 23 23 69 00 00 00 c0 20 20 20 [....####i....   ]
        //  0030: 20 f0 12 10 12 23 e5 0e 10 12 2f 99 10 11 02 f2 [ ....#..../.....]
        // Unknown dashcam (forum16060) - offset 16, enciphered and fuzzed with scale 1
        //  0000: 00 00 40 00 66 72 65 65 47 50 53 20 98 00 00 00 [..@.freeGPS ....]
        //  0010: 4c 49 47 4f 47 50 53 49 4e 46 4f 00 00 00 00 0d [LIGOGPSINFO.....]
        //  0020: 0a 00 00 00 23 23 23 23 3b 00 a0 34 46 44 46 31 [....####;..4FDF1]
        //  0030: 2f 44 39 2f 45 38 20 44 3d 4c 47 4a 4c 39 38 20 [/D9/E8 D=LGJL98 ]
        // Rexing dashcam V1GW-4K - offset 48, encrypted and fuzzed with scale 1
        //  0000: 00 00 40 00 66 72 65 65 47 50 53 20 f0 03 00 00 [..@.freeGPS ....]
        //  0010: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 [................]
        //  0020: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 [................]
        //  0030: 4c 49 47 4f 47 50 53 49 4e 46 4f 00 00 00 00 05 [LIGOGPSINFO.....]
        //  0040: 01 00 00 00 23 23 23 23 73 00 00 00 c0 20 20 20 [....####s....   ]
        //  0050: 20 f0 12 10 12 23 e5 0e 10 12 2f 95 10 12 01 f3 [ ....#..../.....]
        // Kingslim D4 dashcam - offset 80, encrypted and fuzzed with scale 1
        //  0000: 0a 00 00 00 0b 00 00 00 07 00 00 00 e5 07 00 00 [................]
        //  0010: 06 00 00 00 03 00 00 00 41 4e 57 31 91 52 83 45 [........ANW1.R.E]
        //  0020: 15 70 fe c5 29 5c c3 41 ae c7 af 42 00 00 d1 be [.p..)\.A...B....]
        //  0030: 00 00 80 3b 00 00 2c 3e 00 00 00 00 00 00 00 00 [...;..,>........]
        //  0040: 00 00 00 00 00 00 00 00 00 00 00 00 26 26 26 26 [............&&&&]
        //  0050: 4c 49 47 4f 47 50 53 49 4e 46 4f 00 00 00 00 05 [LIGOGPSINFO.....]
        //  0060: 01 00 00 00 23 23 23 23 75 00 00 00 c0 22 20 20 [....####u...."  ]
        //  0070: 20 f0 12 10 12 21 e5 0e 10 12 2f 90 10 13 01 f2 [ ....!..../.....]
        let di = hash_of(vec![("DataPt", data_pt.clone()), ("DirStart", n(pos)), ("DirName", cat(&[&s("LigoGPS_"), &n(pos)]))]);
        // (this is weak, but the only difference I could find between these 2 headers)
        // (NOTE: ../testpics/gps_video/forum16229.mp4 uses this word for a counter!)
        if pos == 16 && re(r"^.{12}\xf0\x03\0\0.{16}\0{4}", "s").is_match(&data) {
            et.set("LigoGPSScale", n(3));
        }
        pl::call(it, "Image::ExifTool::LigoGPS::ProcessLigoGPS", vec![et.clone(), di, tag_tbl.clone()])?;
        done = n(1);

        // also... when offset is 0x50 (Kingslim), the GPS also exists in this format:
        // ($latRef, $lonRef) = ($1, $2);
        // ($hr,$min,$sec,$yr,$mon,$day) = unpack("V6", $$dataPt);
        // # lat/lon aren't decoded properly, but spd,trk,acc are
        // $lat = GetFloat($dataPt, 0x1c);
        // $lon = GetFloat($dataPt, 0x20);
        // $et->VPrint(0, sprintf("Raw lat/lon = %.9f %.9f\n", $lat, $lon));
        // $et->Warn('GPSLatitude/Longitude encryption is not yet known, so these will be wrong');
        // $lat = abs $lat;
        // $lon = abs $lon;
        // $spd = GetFloat($dataPt, 0x24) * $knotsToKph; # (convert knots to km/h)
        // $trk = GetFloat($dataPt, 0x28);
        // $acc[0] = GetFloat($dataPt, 0x2c);
        // $acc[1] = GetFloat($dataPt, 0x30);
        // $acc[2] = GetFloat($dataPt, 0x34);
    } else if let Some(c) = mt(r"^.{60}A\0{3}.{4}([NS])\0{3}.{4}([EW])\0{3}", "s", &data) {
        gps_type(it, &et, &debug, n(6))?;
        // decode freeGPS from Akaso dashcam
        // 0000: 00 00 80 00 66 72 65 65 47 50 53 20 60 00 00 00 [....freeGPS `...]
        // 0010: 78 2e 78 78 00 00 00 00 00 00 00 00 00 00 00 00 [x.xx............]
        // 0020: 30 30 30 30 30 00 00 00 00 00 00 00 00 00 00 00 [00000...........]
        // 0030: 12 00 00 00 2f 00 00 00 19 00 00 00 41 00 00 00 [..../.......A...]
        // 0040: 13 b3 ca 44 4e 00 00 00 29 92 fb 45 45 00 00 00 [...DN...)..EE...]
        // 0050: d9 ee b4 41 ec d1 d3 42 e4 07 00 00 01 00 00 00 [...A...B........]
        // 0060: 0c 00 00 00 01 00 00 00 05 00 00 00 00 00 00 00 [................]
        // (unknown dashcam, "Anticlock 2 2020_1125_1455_007.MOV"):
        // 0000: 00 00 80 00 66 72 65 65 47 50 53 20 68 00 00 00 [....freeGPS h...]
        // 0010: 32 30 31 33 30 33 32 35 41 00 00 00 00 00 00 00 [20130325A.......]
        // 0020: 41 70 72 20 20 36 20 32 30 31 36 2c 20 31 36 3a [Apr  6 2016, 16:]
        // 0030: 0e 00 00 00 38 00 00 00 22 00 00 00 41 00 00 00 [....8..."...A...]
        // 0040: 8a 63 24 45 53 00 00 00 9f e6 42 45 45 00 00 00 [.c$ES.....BEE...]
        // 0050: 59 c0 04 3f 52 b8 42 41 14 00 00 00 0b 00 00 00 [Y..?R.BA........]
        // 0060: 19 00 00 00 06 00 00 00 05 00 00 00 f6 ff ff ff [................]
        // 0070: 03 00 00 00 04 00 00 00 00 00 00 00 00 00 00 00 [................]
        lat_ref = cap(&c, 1);
        lon_ref = cap(&c, 2);
        let v = pl::unpack("x48V3x28V6", &data)?;
        hr = arg(&v, 0);
        min = arg(&v, 1);
        sec = arg(&v, 2);
        yr = arg(&v, 3);
        mon = arg(&v, 4);
        day = arg(&v, 5);
        acc = v.get(6..).map(|x| x.to_vec()).unwrap_or_default();
        lat = get_float(it, &data_pt, n(0x40))?;
        lon = get_float(it, &data_pt, n(0x48))?;
        spd = get_float(it, &data_pt, n(0x50))?;
        trk = get_float(it, &data_pt, n(0x54))?;
        if sub_str(&data, 16, 4).eq_str("x.xx") {
            trk = pl::add(&trk, &n(180)); // (why is this off by 180?)
            if pl::ge(&trk, &n(360)) {
                trk = pl::sub(&trk, &n(360));
            }
            acc.clear();
        } else {
            acc = signed_div(acc, n(1000))?; // (NC)
        }
    } else if re(r"^.{60}4W`b]S<", "s").is_match(&data) && len_of(&data) >= 140 {
        gps_type(it, &et, &debug, n(7))?;
        // 0000: 00 00 40 00 66 72 65 65 47 50 53 20 f0 01 00 00 [..@.freeGPS ....]
        // 0010: 5a 58 53 42 4e 58 59 53 00 00 00 00 00 00 00 00 [ZXSBNXYS........]
        // 0020: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 [................]
        // 0030: 00 00 00 00 00 00 00 00 00 00 00 00 34 57 60 62 [............4W`b]
        // 0040: 5d 53 3c 41 44 45 41 41 42 3e 40 40 3c 51 3c 45 []S<ADEAAB>@@<Q<E]
        // 0050: 41 40 43 3e 41 47 49 48 44 3c 5e 3c 40 41 46 43 [A@C>AGIHD<^<@AFC]
        // 0060: 42 3e 49 49 40 42 45 3c 55 3c 45 47 3e 45 43 41 [B>II@BE<U<EG>ECA]
        // decipher $GPRMC by subtracting 16 from each character value
        let raw = pl::unpack("x60C80", &data)?;
        let t = Value::bytes(raw.iter().map(|v| v.to_i64()).map(|b| if b >= 16 { (b - 16) as u8 } else { 0 }).collect::<Vec<u8>>());
        it.set_scalar("main::_", t.clone()); // ($_ is assigned here)
        if let Some(c) = mt(r"[A-Z]{2}RMC,(\d{2})(\d{2})(\d+(\.\d*)?),A?,(\d*?\d{1,2}\.\d+),([NS]),(\d*?\d{1,2}\.\d+),([EW]),(\d*\.?\d*),(\d*\.?\d*),(\d{2})(\d{2})(\d+)", "", &t) {
            yr = cap(&c, 13);
            mon = cap(&c, 12);
            day = cap(&c, 11);
            hr = cap(&c, 1);
            min = cap(&c, 2);
            sec = cap(&c, 3);
            lat = cap(&c, 5);
            lat_ref = cap(&c, 6);
            lon = cap(&c, 7);
            lon_ref = cap(&c, 8);
            yr = pl::add(&yr, &n(if pl::ge(&yr, &n(70)) { 1900 } else { 2000 }));
            if len_of(&cap(&c, 9)) > 0 {
                spd = pl::mul(&cap(&c, 9), &knots_to_kph());
            }
            if len_of(&cap(&c, 10)) > 0 {
                trk = cap(&c, 10);
            }
        } else {
            done = n(1);
        }
    } else if re(r"^.{64}[\x01-\x0c]\0{3}[\x01-\x1f]\0{3}A[NS][EW]\0{5}", "s").is_match(&data) {
        gps_type(it, &et, &debug, n(8))?;
        // Akaso V1 dascham
        //  0000: 00 00 80 00 66 72 65 65 47 50 53 20 78 00 00 00 [....freeGPS x...]
        //  0010: 59 6e 64 41 6b 61 73 6f 43 61 72 00 00 00 00 00 [YndAkasoCar.....]
        //  0020: 30 30 30 30 30 00 00 00 00 00 00 00 00 00 00 00 [00000...........]
        //  0030: 0e 00 00 00 27 00 00 00 2c 00 00 00 e3 07 00 00 [....'...,.......]
        //  0040: 05 00 00 00 1d 00 00 00 41 4e 45 00 00 00 00 00 [........ANE.....]
        //  0050: f1 4e 3e 3d 90 df ca 40 e3 50 bf 0b 0b 31 a0 40 [.N>=...@.P...1.@]
        //  0060: 4b dc c8 41 9a 79 a7 43 34 58 43 31 4f 37 31 35 [K..A.y.C4XC1O715]
        //  0070: 35 31 32 36 36 35 37 35 59 4e 44 53 0d e7 cc f9 [51266575YNDS....]
        //  0080: 00 00 00 00 05 00 00 00 00 00 00 00 00 00 00 00 [................]
        // Redtiger F7N dashcam
        //  0000: 00 00 40 00 66 72 65 65 47 50 53 20 f0 01 00 00 [..@.freeGPS ....]
        //  0010: 0a 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 [................]
        //  0020: 01 00 00 00 b0 56 50 01 7b 18 68 45 17 02 3f 46 [.....VP.{.hE..?F]
        //  0030: 13 00 00 00 01 00 00 00 06 00 00 00 15 00 00 00 [................]
        //  0040: 0c 00 00 00 1c 00 00 00 41 4e 57 00 00 00 00 00 [........ANW.....]
        //  0050: 80 d4 26 4e 36 11 b5 40 74 b5 15 7b cd 7b f3 40 [..&N6..@t..{.{.@]
        //  0060: 0a d7 a3 3d cd 4c 4e 43 38 34 37 41 45 48 31 36 [...=.LNC847AEH16]
        //  0070: 33 36 30 38 32 34 35 37 59 53 4b 4a 01 00 00 00 [36082457YSKJ....]
        //  0080: ec ff ff ff 00 00 00 00 0e 00 00 00 01 00 00 00 [................]
        //  0090: 0a 00 00 00 e5 07 00 00 0c 00 00 00 1c 00 00 00 [................]
        let v = pl::unpack("x48V6a1a1a1x1", &data)?;
        hr = arg(&v, 0);
        min = arg(&v, 1);
        sec = arg(&v, 2);
        yr = arg(&v, 3);
        mon = arg(&v, 4);
        day = arg(&v, 5);
        let _stat = arg(&v, 6);
        lat_ref = arg(&v, 7);
        lon_ref = arg(&v, 8);

        warn(it, &et, s("GPSLatitude/Longitude encryption is not yet known, so these will be wrong"))?;
        // (see https://exiftool.org/forum/index.php?topic=11320.0)

        spd = get_float(it, &data_pt, n(0x60))?;
        trk = pl::add(&get_float(it, &data_pt, n(0x64))?, &n(180)); // (why is this off by 180?)
        lat = get_double(it, &data_pt, n(0x50))?; // latitude is here, but encrypted somehow
        lon = get_double(it, &data_pt, n(0x58))?; // longitude is here, but encrypted somehow
        ddd = n(1); // don't convert until we know what the format is
        //my $serialNum = substr($$dataPt, 0x68, 20); # (confirmed)
    } else if re(r"^.{12}\xac\0\0\0.{44}(.{72})", "s").is_match(&data) {
        gps_type(it, &et, &debug, n(9))?;
        // EACHPAI dash cam
        //  0000: 00 00 80 00 66 72 65 65 47 50 53 20 ac 00 00 00 [....freeGPS ....]
        //  0010: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 [................]
        //  0020: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 [................]
        //  0030: 00 00 00 00 00 00 00 00 00 00 00 00 34 57 60 62 [............4W`b]
        //  0040: 5d 53 3c 41 47 45 45 42 42 3e 40 40 40 3c 51 3c []S<AGEEBB>@@@<Q<]
        //  0050: 44 42 44 40 3e 48 46 43 45 3c 5e 3c 40 48 43 41 [DBD@>HFCE<^<@HCA]
        //  0060: 42 3e 46 42 47 48 3c 67 3c 40 3e 40 42 3c 43 3e [B>FBGH<g<@>@B<C>]
        //  0070: 43 41 3c 40 42 40 46 42 40 3c 3c 3c 51 3a 47 46 [CA<@B@FB@<<<Q:GF]
        //  0080: 00 2a 36 35 00 00 00 00 00 00 00 00 00 00 00 00 [.*65............]

        warn_lvl(it, &et, s("Can't yet decrypt EACHPAI timed GPS"), 1)?;
        // (see https://exiftool.org/forum/index.php?topic=5095.msg61266#msg61266)
        done = n(1);

        //  my $time = pack 'C*', map { $_ ^= 0 } unpack 'C*', $1;
        //  # bytes 7-12 are the timestamp in ASCII HHMMSS after xor-ing with 0x70
        //  substr($time,7,6) = pack 'C*', map { $_ ^= 0x70 } unpack 'C*', substr($time,7,6);
        //  # (other values are currently unknown)
    } else if let Some(c) = mt(r"^.{64}A([NS])([EW])\0", "s", &data) {
        gps_type(it, &et, &debug, n(10))?;
        // Vantrue S1 dashcam
        //  0000: 00 00 80 00 66 72 65 65 47 50 53 20 78 00 00 00 [....freeGPS x...]
        //  0010: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 [................]
        //  0020: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 [................]
        //  0030: 68 6f 72 73 6f 6e 74 65 63 68 00 00 00 00 00 00 [horsontech......]
        //  0040: 41 4e 45 00 15 00 00 00 07 00 00 00 02 00 00 00 [ANE.............]
        //  0050: 03 00 00 00 35 00 00 00 05 00 00 00 4f 74 4c 44 [....5.......OtLD]
        //  0060: e2 77 a0 45 89 c1 98 42 71 bd ac 42 02 ab 0d 43 [.w.E...Bq..B...C]
        //  0070: 05 00 00 00 7f 00 00 00 07 01 00 00 00 00 00 00 [................]
        lat_ref = cap(&c, 1);
        lon_ref = cap(&c, 2);
        let v = pl::unpack("x68V6x20V3", &data)?;
        yr = arg(&v, 0);
        mon = arg(&v, 1);
        day = arg(&v, 2);
        hr = arg(&v, 3);
        min = arg(&v, 4);
        sec = arg(&v, 5);
        acc = v.get(6..).map(|x| x.to_vec()).unwrap_or_default();
        if pl::ge(&mon, &n(1)) && pl::le(&mon, &n(12)) && pl::ge(&day, &n(1)) && pl::le(&day, &n(31)) {
            // (not sure about acc scaling)
            acc = signed_div(acc, n(1000))?;
            lon = get_float(it, &data_pt, n(0x5c))?;
            lat = get_float(it, &data_pt, n(0x60))?;
            spd = pl::mul(&get_float(it, &data_pt, n(0x64))?, &knots_to_kph());
            trk = get_float(it, &data_pt, n(0x68))?;
            alt = get_float(it, &data_pt, n(0x6c))?;
        } else {
            done = n(1);
        }
    } else if sub_str(&data, 0x45, 3).eq_str("ATC") {
        gps_type(it, &et, &debug, n(11))?;
        // header looks like this: (sample 1)
        //  0000: 00 00 80 00 66 72 65 65 47 50 53 20 38 06 00 00 [....freeGPS 8...]
        //  0010: 49 51 53 32 30 31 33 30 33 30 36 42 00 00 00 00 [IQS20130306B....]
        //  0020: 4d 61 79 20 31 35 20 32 30 31 35 2c 20 31 39 3a [May 15 2015, 19:]
        // (sample 2)
        //  0000: 00 00 80 00 66 72 65 65 47 50 53 20 4c 06 00 00 [....freeGPS L...]
        //  0010: 32 30 31 33 30 33 31 38 2e 30 31 00 00 00 00 00 [20130318.01.....]
        //  0020: 4d 61 72 20 31 38 20 32 30 31 33 2c 20 31 34 3a [Mar 18 2013, 14:]

        let (mut last_rec_pos, mut found_new) = (Value::Undef, false);
        let verbose = opt(it, &et, "Verbose")?;
        let data_pos = dir_info.get("DataPos");
        let free_gps2 = et.get_or_new_hash("FreeGPS2");
        let mut then = free_gps2.get("Then");
        if !then.truthy() {
            then = Value::new_array(vec![n(0); 6]);
            free_gps2.set("Then", then.clone());
        }
        let date_max = lex_array(it, "dateMax");
        // Loop through records in the ATC-type GPS block until we find the most recent.
        // If we have already found one, then we only need to check the first record
        // (in case the buffer wrapped around), and the record after the position of
        // the last record we found, because the others will be old.  Odd, but this
        // is the way it is done...  I have only seen one new 52-byte record in the
        // entire 32 kB block, but the entire device ring buffer (containing 30
        // entries in my samples) is stored every time.  The code below allows for
        // the possibility of missing blocks and multiple new records in a single
        // block, but I have never seen this.  Note that there may be some earlier
        // GPS records at the end of the first block that we will miss decoding, but
        // these should (I believe) be before the start of the video
        let mut rec_pos = n(0x30);
        'atc_rec: while pl::lt(&pl::add(&rec_pos, &n(52)), &n(dir_len)) {
            'body: {
                let av = sub_str(&data, rec_pos.to_i64(), 52); // isolate a single record
                // decrypt record
                let mut ab: Vec<u8> = av.to_bytes().to_vec();
                let (key1, key2) = (ab[0x14], ab[0x1c]);
                for i in (0x00..=0x14).chain(0x18..=0x1b) {
                    ab[i] ^= key1;
                }
                for i in std::iter::once(0x1c).chain(0x20..=0x32) {
                    ab[i] ^= key2;
                }
                let b = Value::bytes(ab);
                // unpack and validate date/time
                let mut now = pl::unpack("x13C3x28vC2", &b)?; // (H-1,M,S,Y,m,d)
                now[0] = ops::bit_and(&pl::add(&now[0], &n(1)), &n(0xff)); // increment hour
                for i in 0..date_max.len() {
                    if pl::le(&now[i], &date_max.at(i as i64)) {
                        continue;
                    }
                    warn(it, &et, s("Invalid GPS date/time"))?;
                    rec_pos = pl::add(&rec_pos, &n(52));
                    continue 'atc_rec; // ignore this record
                }
                // look for next ATC record in temporal sequence
                for i in [3usize, 4, 5, 0, 1, 2] {
                    if pl::lt(&now[i], &then.at(i as i64)) {
                        if found_new {
                            break 'atc_rec;
                        }
                        break;
                    }
                    if pl::num_eq(&now[i], &then.at(i as i64)) {
                        continue;
                    }
                    // we found a more recent record -- extract it and remember its location
                    if verbose.truthy() {
                        m(it, &et, "VPrint", vec![n(2), s("  + [encrypted GPS record]\n")])?;
                        m(it, &et, "VerboseDump", vec![dp(&av), s("DataPos"), pl::add(&data_pos, &rec_pos)])?;
                        m(it, &et, "VPrint", vec![n(2), s("  + [decrypted GPS record]\n")])?;
                        m(it, &et, "VerboseDump", vec![dp(&b)])?;
                        //my @v = unpack 'H8VVC4V!CA3V!CA3VvvV!vCCCCH4', $b;
                        //$et->VPrint(2, "  + [unpacked: @v]\n");
                        // values unpacked above (ref PH):
                        //  0) 0x00 4 bytes - byte 0=1, 1=counts to 255, 2=record index, 3=0 (ref 3)
                        //  1) 0x04 4 bytes - int32u: bits 0-4=day, 5-8=mon, 9-19=year (ref 3)
                        //  2) 0x08 4 bytes - int32u: bits 0-5=sec, 6-11=min, 12-16=hour (ref 3)
                        //  3) 0x0c 1 byte  - seen values of 0,1,2 - GPS status maybe?
                        //  4) 0x0d 1 byte  - hour minus 1
                        //  5) 0x0e 1 byte  - minute
                        //  6) 0x0f 1 byte  - second
                        //  7) 0x10 4 bytes - int32s latitude * 1e7
                        //  8) 0x14 1 byte  - always 0 (used for decryption)
                        //  9) 0x15 3 bytes - always "ATC"
                        // 10) 0x18 4 bytes - int32s longitude * 1e7
                        // 11) 0x1c 1 byte  - always 0 (used for decryption)
                        // 12) 0x1d 3 bytes - always "001"
                        // 13) 0x20 4 bytes - int32s speed * 100 (m/s)
                        // 14) 0x24 2 bytes - int16u heading * 100 (-180 to 180 deg)
                        // 15) 0x26 2 bytes - always zero
                        // 16) 0x28 4 bytes - int32s altitude * 1000 (ref 3)
                        // 17) 0x2c 2 bytes - int16u year
                        // 18) 0x2e 1 byte  - month
                        // 19) 0x2f 1 byte  - day
                        // 20) 0x30 1 byte  - unknown
                        // 21) 0x31 1 byte  - always zero
                        // 22) 0x32 2 bytes - checksum ?
                    }
                    if let Value::ArrayRef(r) = &then {
                        *r.borrow_mut() = now.clone();
                    }
                    new_doc(&et);
                    let bp = dp(&b);
                    trk = pl::div(&get16s(it, &bp, n(0x24))?, &n(100))?;
                    if pl::lt(&trk, &n(0)) {
                        trk = pl::add(&trk, &n(360));
                    }
                    let time = pl::sprintf("%.4d:%.2d:%.2d %.2d:%.2d:%.2dZ", &[now[3].clone(), now[4].clone(), now[5].clone(), now[0].clone(), now[1].clone(), now[2].clone()]);
                    ht(it, &et, &tag_tbl, "GPSDateTime", time)?;
                    let v = get32s(it, &bp, n(0x10))?;
                    ht(it, &et, &tag_tbl, "GPSLatitude", pl::div(&v, &f(1e7))?)?;
                    let v = get32s(it, &bp, n(0x18))?;
                    ht(it, &et, &tag_tbl, "GPSLongitude", pl::div(&v, &f(1e7))?)?;
                    let v = get32s(it, &bp, n(0x20))?;
                    ht(it, &et, &tag_tbl, "GPSSpeed", pl::mul(&pl::div(&v, &n(100))?, &mps_to_kph()))?;
                    ht(it, &et, &tag_tbl, "GPSTrack", trk.clone())?;
                    let v = get32s(it, &bp, n(0x28))?;
                    ht(it, &et, &tag_tbl, "GPSAltitude", pl::div(&v, &n(1000))?)?;
                    last_rec_pos = rec_pos.clone();
                    found_new = true;
                    // don't skip to location of previous recent record in ring buffer
                    // since we found a more recent record here
                    free_gps2.delete("RecentRecPos");
                    break;
                }
                break 'body;
            }
            // skip older records
            let recent_rec_pos = free_gps2.get("RecentRecPos");
            if recent_rec_pos.truthy() && pl::lt(&rec_pos, &recent_rec_pos) {
                rec_pos = recent_rec_pos;
            }
            rec_pos = pl::add(&rec_pos, &n(52));
        }
        // save position of most recent record (needed when parsing the next freeGPS block)
        free_gps2.set("RecentRecPos", last_rec_pos);
        done = n(1);
    } else if let Some(c) = mt(r"^.{60}A\0.{10}([NS])\0.{14}([EW])\0", "s", &data).filter(|_| dir_len >= 0x88) {
        gps_type(it, &et, &debug, n(12))?;
        // header looks like this in my sample:
        //  0000: 00 00 80 00 66 72 65 65 47 50 53 20 08 01 00 00 [....freeGPS ....]
        //  0010: 32 30 31 33 30 38 31 35 2e 30 31 00 00 00 00 00 [20130815.01.....]
        //  0020: 4a 75 6e 20 31 30 20 32 30 31 37 2c 20 31 34 3a [Jun 10 2017, 14:]

        // Type 2 (ref PH):
        // 0x30 - int32u hour
        // 0x34 - int32u minute
        // 0x38 - int32u second
        // 0x3c - int32u GPS status ('A' or 'V')
        // 0x40 - double latitude  (DDMM.MMMMMM)
        // 0x48 - int32u latitude ref  ('N' or 'S')
        // 0x50 - double longitude (DDMM.MMMMMM)
        // 0x58 - int32u longitude ref ('E' or 'W')
        // 0x60 - double speed (knots)
        // 0x68 - double heading (deg)
        // 0x70 - int32u year - 2000
        // 0x74 - int32u month
        // 0x78 - int32u day
        // 0x7c - int32s[3] accelerometer * 1000
        lat_ref = cap(&c, 1);
        lon_ref = cap(&c, 2);
        let v = pl::unpack("x48V3x52V6", &data)?;
        hr = arg(&v, 0);
        min = arg(&v, 1);
        sec = arg(&v, 2);
        yr = arg(&v, 3);
        mon = arg(&v, 4);
        day = arg(&v, 5);
        acc = signed_div(v.get(6..).map(|x| x.to_vec()).unwrap_or_default(), n(1000))?;
        lat = get_double(it, &data_pt, n(0x40))?;
        lon = get_double(it, &data_pt, n(0x50))?;
        spd = pl::mul(&get_double(it, &data_pt, n(0x60))?, &knots_to_kph());
        trk = get_double(it, &data_pt, n(0x68))?;
    } else if re(r"^.{16}A([NS])([EW])\0", "s").is_match(&data) {
        gps_type(it, &et, &debug, n(13))?;
        // INNOVV MP4 video (same format as INNOVV TS)
        //  0000: 00 00 40 00 66 72 65 65 47 50 53 20 f0 03 00 00 [..@.freeGPS ....]
        //  0010: 41 4e 45 00 e4 56 96 45 86 b1 ca 44 5c 8f e2 40 [ANE..V.E...D\..@]
        //  0020: 33 33 58 43 c3 00 00 00 30 00 00 00 a0 fe ff ff [33XC....0.......]
        //  0030: 41 4e 45 00 e3 56 96 45 82 b1 ca 44 5c 8f fa 40 [ANE..V.E...D\..@]
        //  0040: c3 75 56 43 8c ff ff ff 8c 00 00 00 c3 fd ff ff [.uVC............]
        for c in re(r"(A[NS][EW]\0.{28})", "s").each(&data) {
            let dat = cap(&c, 1);
            let dpt = dp(&dat);
            lat = pl::abs(&get_float(it, &dpt, n(4))?); // (abs just to be safe)
            lon = pl::abs(&get_float(it, &dpt, n(8))?); // (abs just to be safe)
            spd = pl::mul(&get_float(it, &dpt, n(12))?, &knots_to_kph());
            trk = get_float(it, &dpt, n(16))?;
            acc = pl::unpack("x20V3", &dat)?.iter().map(signed_int32_of).collect();
            convert_lat_lon(&mut lat, &mut lon)?;
            new_doc(&et);
            ht(it, &et, &tag_tbl, "GPSLatitude", pl::mul(&lat, &n(if sub_str(&dat, 1, 1).eq_str("S") { -1 } else { 1 })))?;
            ht(it, &et, &tag_tbl, "GPSLongitude", pl::mul(&lon, &n(if sub_str(&dat, 2, 1).eq_str("W") { -1 } else { 1 })))?;
            ht(it, &et, &tag_tbl, "GPSSpeed", spd.clone())?;
            ht(it, &et, &tag_tbl, "GPSTrack", trk.clone())?;
            ht(it, &et, &tag_tbl, "Accelerometer", join_sp(&acc))?;
        }
        done = n(1);
    } else if re(r"^.{20}[\0-\x18][\0-\x3b]{2}[\0-\x09]A([NS])([EW])", "s").is_match(&data) {
        gps_type(it, &et, &debug, n(14))?;
        // XBHT motorcycle dashcam Model XB702
        //  0000: 00 00 40 00 66 72 65 65 47 50 53 20 f0 03 00 00 [..@.freeGPS ....]
        //  0010: 00 17 05 11 0d 25 18 00 41 4e 45 64 83 3f 00 00 [.....%..ANEd.?..]
        //  0020: 44 3d c5 02 48 6d ff 07 df 03 00 00 6b 00 00 00 [D=..Hm......k...]
        //  0030: 00 00 00 00 00 17 05 11 0d 25 18 01 41 4e 45 64 [.........%..ANEd]
        //  0040: 8b 3f 00 00 30 3d c5 02 50 6d ff 07 df 03 00 00 [.?..0=..Pm......]
        for c in re(r"(.{7}[\0-\x09]A[NS][EW].{25})", "s").each(&data) {
            let dat = cap(&c, 1);
            let v = pl::unpack("xC7xCCx5VVx4v", &dat)?;
            yr = arg(&v, 0);
            mon = arg(&v, 1);
            day = arg(&v, 2);
            hr = arg(&v, 3);
            min = arg(&v, 4);
            sec = arg(&v, 5);
            let ss = arg(&v, 6);
            lat_ref = arg(&v, 7);
            lon_ref = arg(&v, 8);
            lat = arg(&v, 9);
            lon = arg(&v, 10);
            spd = arg(&v, 11);
            yr = pl::add(&yr, &n(2000));
            lat = pl::div(&lat, &f(1e4))?;
            lon = pl::div(&lon, &f(1e4))?;
            convert_lat_lon(&mut lat, &mut lon)?;
            new_doc(&et);
            let time = pl::sprintf("%.4d:%.2d:%.2d %.2d:%.2d:%.2d.%d", &[yr.clone(), mon.clone(), day.clone(), hr.clone(), min.clone(), sec.clone(), ss]);
            ht(it, &et, &tag_tbl, "GPSDateTime", time)?;
            ht(it, &et, &tag_tbl, "GPSLatitude", pl::mul(&lat, &n(if lat_ref.eq_str("S") { -1 } else { 1 })))?;
            ht(it, &et, &tag_tbl, "GPSLongitude", pl::mul(&lon, &n(if lon_ref.eq_str("W") { -1 } else { 1 })))?;
            ht(it, &et, &tag_tbl, "GPSSpeed", spd.clone())?;
        }
        done = n(1);
    } else if let Some(c) = mt(r"^.{28}A.{11}([NS]).{15}([EW])", "s", &data) {
        gps_type(it, &et, &debug, n(15))?;
        // Vantrue N4 dashcam
        //  0000: 00 00 40 00 66 72 65 65 47 50 53 20 f0 03 00 00 [..@.freeGPS ....]
        //  0010: 0d 00 00 00 16 00 00 00 1e 00 00 00 41 00 00 00 [............A...]
        //  0020: 2c b7 b4 1a 5a 71 b2 40 4e 00 00 00 00 00 00 00 [,...Zq.@N.......]
        //  0030: fb ae 08 fe 77 f6 89 40 45 00 00 00 00 00 00 00 [....w..@E.......]
        //  0040: be 9f 1a 2f dd 84 36 40 5c 8f c2 f5 28 fc 68 40 [.../..6@\...(.h@]
        //  0050: 16 00 00 00 0c 00 00 00 0e 00 00 00 f2 fb ff ff [................]
        //  0060: 42 00 00 00 02 00 00 00 20 24 47 4e 52 4d 43 2c [B....... $GNRMC,]
        //  0070: 31 33 32 32 33 30 2e 30 30 30 2c 41 2c 34 37 32 [132230.000,A,472]
        //  0080: 31 2e 33 35 31 39 37 2c 4e 2c 30 30 38 33 30 2e [1.35197,N,00830.]
        //  0090: 38 30 38 35 39 2c 45 2c 32 32 2e 35 31 39 2c 31 [80859,E,22.519,1]
        //  00a0: 39 39 2e 38 38 2c 31 34 31 32 32 32 2c 2c 2c 41 [99.88,141222,,,A]
        //  00b0: 2a 37 35 0d 0a 00 00 00 00 00 00 00 00 00 00 00 [*75.............]
        lat_ref = cap(&c, 1);
        lon_ref = cap(&c, 2);
        let v = pl::unpack("x16V3x52V3V3", &data)?;
        hr = arg(&v, 0);
        min = arg(&v, 1);
        sec = arg(&v, 2);
        yr = arg(&v, 3);
        mon = arg(&v, 4);
        day = arg(&v, 5);
        acc = v.get(6..).map(|x| x.to_vec()).unwrap_or_default();
        lat = pl::abs(&get_double(it, &data_pt, n(32))?); // (abs just to be safe)
        lon = pl::abs(&get_double(it, &data_pt, n(48))?); // (abs just to be safe)
        spd = pl::mul(&get_double(it, &data_pt, n(64))?, &knots_to_kph());
        trk = get_double(it, &data_pt, n(72))?;
        acc = signed_div(acc, n(1000))?; // (NC)
        // (not necessary to read RMC sentence because we already have it all)
    } else if re(r"^.{72}A[NS][EW]\0", "s").is_match(&data) {
        // decode binary GPS format (Viofo A119S, ref 2)
        // header looks like this in my sample:
        //  0000: 00 00 80 00 66 72 65 65 47 50 53 20 4c 00 00 00 [....freeGPS L...]
        //  0010: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 [................]
        //  0020: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 [................]
        //  0030: 10 00 00 00 2d 00 00 00 14 00 00 00 11 00 00 00 [....-...........]
        //  0040: 0c 00 00 00 1f 00 00 00 41 4e 45 00 5d 9a a9 45 [........ANE.]..E]
        //  0050: ab 1e e5 44 ec 51 f0 40 b8 5e a5 43 00 00 00 00 [...D.Q.@.^.C....]
        // (records are same structure as Type 3 Novatek GPS:)
        // Type 3 (Novatek GPS, ref 2):
        // 0x30 - int32u hour
        // 0x34 - int32u minute
        // 0x38 - int32u second
        // 0x3c - int32u year - 2000
        // 0x40 - int32u month
        // 0x44 - int32u day
        // 0x48 - int8u  GPS status ('A' or 'V')
        // 0x49 - int8u  latitude ref  ('N' or 'S')
        // 0x4a - int8u  longitude ref ('E' or 'W')
        // 0x4b - 0
        // 0x4c - float  latitude  (DDMM.MMMMMM)
        // 0x50 - float  longitude (DDMM.MMMMMM)
        // 0x54 - float  speed (knots)
        // 0x58 - float  heading (deg)
        // Type 3b, same as above for 0x30-0x4a (ref PH)
        // 0x4c - int32s latitude (decimal degrees * 1e7)
        // 0x50 - int32s longitude (decimal degrees * 1e7)
        // 0x54 - int32s speed (m/s * 100)
        // 0x58 - float  altitude (m * 1000, NC)
        let v = pl::unpack("x48V6a1a1a1x1V4", &data)?;
        hr = arg(&v, 0);
        min = arg(&v, 1);
        sec = arg(&v, 2);
        yr = arg(&v, 3);
        mon = arg(&v, 4);
        day = arg(&v, 5);
        let _stat = arg(&v, 6);
        lat_ref = arg(&v, 7);
        lon_ref = arg(&v, 8);
        if sub_str(&data, 16, 3).eq_str("IQS") {
            gps_type(it, &et, &debug, n(16))?;
            // IQS variant (ref PH)
            // header looks like this in my sample:
            //  0000: 00 00 80 00 66 72 65 65 47 50 53 20 4c 00 00 00 [....freeGPS L...]
            //  0010: 49 51 53 5f 41 37 5f 32 30 31 35 30 34 31 37 00 [IQS_A7_20150417.]
            //  0020: 4d 61 72 20 32 39 20 32 30 31 37 2c 20 31 36 3a [Mar 29 2017, 16:]
            ddd = n(1);
            lat = pl::abs(&pl::div(&get32s(it, &data_pt, n(0x4c))?, &f(1e7))?);
            lon = pl::abs(&pl::div(&get32s(it, &data_pt, n(0x50))?, &f(1e7))?);
            spd = pl::mul(&pl::div(&get32s(it, &data_pt, n(0x54))?, &n(100))?, &mps_to_kph());
            alt = pl::div(&get_float(it, &data_pt, n(0x58))?, &n(1000))?; // (NC)
        } else {
            lat = get_float(it, &data_pt, n(0x4c))?;
            lon = get_float(it, &data_pt, n(0x50))?;
            spd = pl::mul(&get_float(it, &data_pt, n(0x54))?, &knots_to_kph());
            trk = get_float(it, &data_pt, n(0x58))?; // (NC, may be GPSImageDirection)
            // Rexing V1-4k dashcam scales the lat/lon
            // (recognize this dashcam by the KodakVersion, "3.01.054" for my sample)
            //  0000: 00 00 80 00 66 72 65 65 47 50 53 20 4c 00 00 00 [....freeGPS L...]
            //  0010: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 [................]
            //  0020: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 [................]
            //  0030: 0e 00 00 00 22 00 00 00 28 00 00 00 14 00 00 00 [...."...(.......]
            //  0040: 02 00 00 00 16 00 00 00 41 4e 57 00 e9 7e 90 43 [........ANW..~.C]
            //  0050: 48 76 17 45 0c 02 48 42 14 6e 85 43 00 00 00 00 [Hv.E..HB.n.C....]
            let kv = et.get("KodakVersion");
            if kv.truthy() && kv.eq_str("3.01.054") {
                gps_type(it, &et, &debug, s("17b"))?;
                lat = pl::div(&pl::sub(&lat, &f(187.982162849635)), &n(3))?;
                lon = pl::div(&pl::sub(&lon, &f(2199.19873715495)), &n(2))?;
                ddd = n(1);
            } else if pl::num_eq(&get32u(it, &data_pt, n(0))?, &n(0x400000)) && pl::le(&pl::abs(&lat), &n(90)) && pl::le(&pl::abs(&lon), &n(180)) {
                gps_type(it, &et, &debug, s("17c"))?;
                // Transcend Drive Body Camera 70
                //  0000: 00 00 40 00 66 72 65 65 47 50 53 20 4c 00 00 00 [..@.freeGPS L...]
                //  0010: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 [................]
                //  0020: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 [................]
                //  0030: 09 00 00 00 26 00 00 00 15 00 00 00 e9 07 00 00 [....&...........]
                //  0040: 05 00 00 00 10 00 00 00 41 53 45 00 6c 59 ee 41 [........ASE.lY.A]
                //  0050: 9f 1a f7 41 3c 6b 0f 41 9a 99 99 43 00 00 00 00 [...A<k.A...C....]
                ddd = n(1); // already in decimal degrees
                spd = pl::div(&spd, &knots_to_kph())?; // already in km/h
            } else {
                gps_type(it, &et, &debug, n(17))?;
            }
        }
        if dir_len >= 0xb0 {
            // lat/lon also stored as doubles by Transcend Driver Pro 230 (ref PH)
            let (lat2, lon2) = (get_double(it, &data_pt, n(0x70))?, get_double(it, &data_pt, n(0x80))?);
            // (0xa0 is altitude, don't know what 0x98 and 0xa8 are)
            if pl::lt(&pl::abs(&pl::sub(&lat2, &lat)), &f(0.001)) && pl::lt(&pl::abs(&pl::sub(&lon2, &lon)), &f(0.001)) {
                lat = lat2;
                lon = lon2;
                alt = get_double(it, &data_pt, n(0xa0))?;
            }
        }
    } else if let Some(c) = mt(r"^.{23}(\d{4})/(\d{2})/(\d{2}) (\d{2}):(\d{2}):(\d{2}) [N|S]", "s", &data) {
        gps_type(it, &et, &debug, n(18))?;
        // XGODY 12" 4K Dashcam
        //  0000: 00 00 00 a8 66 72 65 65 47 50 53 20 98 00 00 00 [....freeGPS ....]
        //  0010: 6e 6f 72 6d 61 6c 3a 32 30 32 34 2f 30 35 2f 32 [normal:2024/05/2]
        //  0020: 32 20 30 32 3a 35 34 3a 32 39 20 4e 3a 34 32 2e [2 02:54:29 N:42.]
        //  0030: 33 38 32 34 37 30 20 57 3a 38 33 2e 33 38 39 35 [382470 W:83.3895]
        //  0040: 37 30 20 35 33 2e 36 20 6b 6d 2f 68 20 78 3a 2d [70 53.6 km/h x:-]
        //  0050: 30 2e 30 32 20 79 3a 30 2e 39 39 20 7a 3a 30 2e [0.02 y:0.99 z:0.]
        //  0060: 31 30 20 41 3a 32 36 39 2e 32 20 48 3a 32 34 35 [10 A:269.2 H:245]
        //  0070: 2e 35 00 00 00 00 00 00 00 00 00 00 00 00 00 00 [.5..............]
        yr = cap(&c, 1);
        mon = cap(&c, 2);
        day = cap(&c, 3);
        hr = cap(&c, 4);
        min = cap(&c, 5);
        sec = cap(&c, 6);
        if re(r"\0+$", "").subst(&mut data, false, |_| Vec::new()) > 0 {
            pl::set_deref(&data_pt, data.clone())?; // remove trailing nulls
        }
        let av = pl::split_ws(&pl::substr(&data, 43, None));
        ddd = n(1);
        for item in av {
            let Some(c) = mt(r"^([A-Z]):([-+]?\d+(\.\d+)?)$", "i", &item) else {
                // (the "km/h" after spd is display units? because the value is stored in knots)
                if defined(&lon) && !defined(&spd) && re(r"^\d+\.\d+$", "").is_match(&item) {
                    spd = pl::mul(&item, &knots_to_kph());
                }
                continue;
            };
            let (c1, c2) = (cap(&c, 1), cap(&c, 2));
            if c1.eq_str("N") || c1.eq_str("S") {
                lat = c2;
                lat_ref = c1;
                continue;
            }
            if c1.eq_str("E") || c1.eq_str("W") {
                lon = c2;
                lon_ref = c1;
                continue;
            }
            if c1.eq_str("x") || c1.eq_str("y") || c1.eq_str("z") {
                acc.push(c2);
                continue;
            }
            if c1.eq_str("A") {
                trk = c2; // (verified, but why 'A'?)
                continue;
            }
            // seen 'H' - one might expect altitude ('H'eight), but it doesn't fit
            // the sample data, so save all other information as an "Unknown_X" tag
            if !tag_tbl.get(c1.to_bytes()).truthy() {
                add_tag_to_table(it, &tag_tbl, c1.clone(), hash_of(vec![("Name", cat(&[&s("Unknown_"), &c1])), ("Unknown", n(1))]))?;
            }
            xtra.push(c1);
            xtra.push(c2);
        }
    } else if re(r"^.{30}A.{20}VV", "").is_match(&data) {
        gps_type(it, &et, &debug, n(19))?;
        // 70mai A810 dashcam (note: no timestamps in the samples I have)
        //  0000: 00 00 40 00 66 72 65 65 47 50 53 20 ed 01 00 00 [..@.freeGPS ....]
        //  0010: 03 00 ed 01 00 00 00 0f 00 00 70 08 00 00 41 66 [..........p...Af]
        //  0020: 13 7d 1e 3c 11 dc 03 5d 01 00 00 01 00 00 00 23 [.}.<...].......#]
        //  0030: 00 00 00 56 56 00 00 00 00 00 00 00 00 00 00 00 [...VV...........]
        //  0040: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 [................]
        set_byte_order(it, s("II"))?;
        set_gps_dt(it, &et, &tag_tbl, dir_info.get("SampleTime"), Value::Undef)?;
        lat = pl::div(&get32s(it, &data_pt, n(31))?, &f(1e5))?;
        lon = pl::div(&get32s(it, &data_pt, n(35))?, &f(1e5))?;
        spd = get32s(it, &data_pt, n(43))?; // (seems to be km/h but not confirmed)
        // offset 475 - int16u=N string[N] - some sort of settings?:
        // eg. "\x15\x00{pA:V,rA:V,sF:0,tF:2}"
    } else {
        gps_type(it, &et, &debug, n(20))?;
        // (look for binary GPS as stored by Nextbase 512G, ref PH)
        //  0000: 00 00 80 00 66 72 65 65 47 50 53 20 78 01 00 00 [....freeGPS x...]
        //  0010: 78 2e 78 78 00 00 00 00 00 00 00 00 00 00 00 00 [x.xx............]
        //  0020: 30 30 30 30 30 00 00 00 00 00 00 00 00 00 00 00 [00000...........]
        //  0030: 24 53 02 79 d4 85 07 e2 0a 08 06 2a 01 d1 02 20 [$S.y.......*... ]
        //  0040: 14 98 ff ff 21 67 97 10 00 00 00 00 00 00 00 00 [....!g..........]
        //  0050: 24 53 02 a2 d4 42 07 e2 0a 08 06 2a 01 d2 02 20 [$S...B.....*... ]
        //  0060: 14 98 e3 ff 21 67 3b 10 00 00 00 00 00 00 00 00 [....!g;.........]
        // 32-byte record structure (big endian!):
        // 0x30 - int16u unknown (seen: 0x24 0x53 = "$S")
        // 0x32 - int16u speed (m/s * 100)
        // 0x34 - int16s heading (deg * 100) (or GPSImgDirection?)
        // 0x36 - int16u year
        // 0x38 - int8u  month
        // 0x39 - int8u  day
        // 0x3a - int8u  hour
        // 0x3b - int8u  min
        // 0x3c - int16u sec * 10
        // 0x3e - int8u  unknown (seen: 2)
        // 0x3f - int32s latitude (decimal degrees * 1e7)
        // 0x43 - int32s longitude (decimal degrees * 1e7)
        // 0x47 - int8u  unknown (seen: 16)
        // 0x48-0x4f -   all zero
        let mut pos = n(0x32);
        loop {
            let v = pl::unpack(&format!("x{}nnnCCCCnx1NN", pos.to_i64()), &data)?;
            spd = arg(&v, 0);
            trk = arg(&v, 1);
            yr = arg(&v, 2);
            mon = arg(&v, 3);
            day = arg(&v, 4);
            hr = arg(&v, 5);
            min = arg(&v, 6);
            sec = arg(&v, 7);
            lat = arg(&v, 8);
            lon = arg(&v, 9);
            // validate record using date/time
            if pl::lt(&yr, &n(2000)) || pl::gt(&yr, &n(2200)) || pl::lt(&mon, &n(1)) || pl::gt(&mon, &n(12)) || pl::lt(&day, &n(1)) || pl::gt(&day, &n(31)) || pl::gt(&hr, &n(59)) || pl::gt(&min, &n(59)) || pl::gt(&sec, &n(600)) {
                break;
            }
            // change lat/lon to signed integer and divide by 1e7
            lat = pl::div(&signed_int32_of(&lat), &f(1e7))?;
            lon = pl::div(&signed_int32_of(&lon), &f(1e7))?;
            if pl::ge(&trk, &n(0x8000)) {
                trk = pl::sub(&trk, &n(0x10000)); // make it signed
            }
            trk = pl::div(&trk, &n(100))?;
            if pl::lt(&trk, &n(0)) {
                trk = pl::add(&trk, &n(360));
            }
            let time = pl::sprintf("%.4d:%.2d:%.2d %.2d:%.2d:%04.1fZ", &[yr.clone(), mon.clone(), day.clone(), hr.clone(), min.clone(), pl::div(&sec, &n(10))?]);
            new_doc(&et);
            ht(it, &et, &tag_tbl, "GPSDateTime", time)?;
            ht(it, &et, &tag_tbl, "GPSLatitude", lat.clone())?;
            ht(it, &et, &tag_tbl, "GPSLongitude", lon.clone())?;
            ht(it, &et, &tag_tbl, "GPSSpeed", pl::mul(&pl::div(&spd, &n(100))?, &mps_to_kph()))?;
            ht(it, &et, &tag_tbl, "GPSTrack", trk.clone())?;
            // (last if $pos += 0x20 > length($$dataPt) - 0x1e: the comparison binds first)
            let cmp: Value = pl::gt(&n(0x20), &n(len_of(&data) - 0x1e)).into();
            pos = pl::add(&pos, &cmp);
            if pos.truthy() {
                break;
            }
        }
        done = n(1);
    }
    set_byte_order(it, old_order)?;
    if done.truthy() {
        return ret(n(if et.get("DOC_NUM").truthy() { 1 } else { 0 }));
    }
    if defined(&yr) && (pl::lt(&mon, &n(1)) || pl::gt(&mon, &n(12))) {
        return ret(n(0)); // quick sanity check
    }
    //
    // save tag values extracted by above code
    //
    found_some(it, &et, &tag_tbl, &dir_info.get("SampleTime"), &dir_info.get("SampleDuration"))?;
    if defined(&sec) && !re(r"^\d{2}", "").is_match(&sec) {
        sec = cat(&[&s("0"), &sec]); // pad integer part of seconds to 2 digits
    }
    if defined(&yr) {
        if pl::lt(&yr, &n(2000)) {
            yr = pl::add(&yr, &n(2000));
        }
        let time = pl::sprintf("%.4d:%.2d:%.2d %.2d:%.2d:%sZ", &[yr, mon, day, hr, min, sec]);
        ht(it, &et, &tag_tbl, "GPSDateTime", time)?;
    } else if defined(&hr) {
        let time = pl::sprintf("%.2d:%.2d:%sZ", &[hr, min, sec]);
        ht(it, &et, &tag_tbl, "GPSTimeStamp", time)?;
    }
    if defined(&lat) && defined(&lon) {
        // lat/long are in DDDMM.MMMM format unless $ddd is set
        if !ddd.truthy() {
            convert_lat_lon(&mut lat, &mut lon)?;
        }
        let ls = if lat_ref.truthy() && lat_ref.eq_str("S") { -1 } else { 1 };
        ht(it, &et, &tag_tbl, "GPSLatitude", pl::mul(&lat, &n(ls)))?;
        let ws = if lon_ref.truthy() && lon_ref.eq_str("W") { -1 } else { 1 };
        ht(it, &et, &tag_tbl, "GPSLongitude", pl::mul(&lon, &n(ws)))?;
    }
    if defined(&alt) {
        ht(it, &et, &tag_tbl, "GPSAltitude", alt)?;
    }
    if defined(&spd) {
        ht(it, &et, &tag_tbl, "GPSSpeed", spd)?;
    }
    if defined(&trk) {
        ht(it, &et, &tag_tbl, "GPSTrack", trk)?;
    }
    let mut xi = xtra.into_iter();
    while let Some(tag) = xi.next() {
        let v = xi.next().unwrap_or_default();
        m(it, &et, "HandleTag", vec![tag_tbl.clone(), tag, v])?;
    }
    if !acc.is_empty() {
        ht(it, &et, &tag_tbl, "Accelerometer", join_sp(&acc))?;
    }
    ret(n(1))
}

/// Extract embedded information referenced from a track
/// Inputs: 0) ExifTool ref, 1) tag name, 2) data ref
pub fn parse_tag(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| parse_tag_impl(it, &a))?;
    pl::ret_nothing(want)
}

fn parse_tag_impl(it: &mut Interp, a: &[Value]) -> ER<()> {
    let (et, tag, data_pt) = (arg(a, 0), arg(a, 1), arg(a, 2));
    pl::at(2493);
    let data = pl::sderef(&data_pt)?;
    let data_len = len_of(&data);
    let t = |x: &str| tag.eq_str(x);

    if t("stsz") || (t("stz2") && data_len > 12) {
        // read the sample sizes
        let u = pl::unpack("x4N2", &data)?;
        let (mut sz, num) = (arg(&u, 0), arg(&u, 1));
        let size = Value::new_array(vec![]);
        et.get_or_new_hash("ee").set("size", size.clone());
        let set = |v: Vec<Value>| {
            if let Value::ArrayRef(r) = &size {
                *r.borrow_mut() = v;
            }
        };
        if t("stsz") {
            if pl::num_eq(&sz, &n(0)) {
                set(read_value_list(it, &data_pt, n(12), s("int32u"), num.clone(), n(data_len - 12))?);
            } else {
                set(vec![sz.clone(); num.to_i64().max(0) as usize]);
            }
        } else {
            sz = ops::bit_and(&sz, &n(0xff));
            if pl::num_eq(&sz, &n(4)) {
                let cnt = pl::int(&pl::div(&pl::add(&num, &n(1)), &n(2))?);
                let tmp = read_value_list(it, &data_pt, n(12), s("int8u"), cnt, n(data_len - 12))?;
                for x in tmp {
                    size.push(ops::shift_right(&x, &n(4)));
                    size.push(ops::bit_and(&x, &n(0xff)));
                }
            } else if pl::num_eq(&sz, &n(8)) || pl::num_eq(&sz, &n(16)) {
                set(read_value_list(it, &data_pt, n(12), cat(&[&s("int"), &sz, &s("u")]), num.clone(), n(data_len - 12))?);
            }
        }
    } else if t("stco") || (t("co64") && data_len > 8) {
        // read the chunk offsets
        let num = pl::unpack1("x4N", &data)?;
        let fmt = if t("stco") { "int32u" } else { "int64u" };
        let v = read_value_list(it, &data_pt, n(8), s(fmt), num, n(data_len - 8))?;
        et.get_or_new_hash("ee").set("stco", Value::new_array(v));
    } else if t("stsc") && data_len > 8 {
        // read the sample-to-chunk box
        let num = pl::unpack1("x4N", &data)?.to_i64();
        if data_len >= 8 + num * 12 {
            let mut stsc = Vec::new();
            for i in 0..num {
                // list of (first-chunk, samples-per-chunk, sample-description-index)
                stsc.push(Value::new_array(pl::unpack(&format!("x{}N3", 8 + i * 12), &data)?));
            }
            et.get_or_new_hash("ee").set("stsc", Value::new_array(stsc));
        }
    } else if t("stts") && data_len > 8 {
        // read the time-to-sample box
        let num = pl::unpack1("x4N", &data)?.to_i64();
        if data_len >= 8 + num * 8 {
            let v = pl::unpack(&format!("x8N{}", num * 2), &data)?;
            et.get_or_new_hash("ee").set("stts", Value::new_array(v));
        }
    } else if t("avcC") {
        // read the AVC compressor configuration
        if data_len >= 7 {
            et.get_or_new_hash("ee").set("avcC", data.clone()); // (minimum length is 7)
        }
    } else if t("JPEG") {
        et.get_or_new_hash("ee").set("JPEG", data.clone());
    } else if t("gps ") && data_len > 8 {
        // decode Novatek 'gps ' box (ref 2)
        let mut num = get32u(it, &data_pt, n(4))?;
        if pl::gt(&pl::add(&pl::mul(&num, &n(8)), &n(8)), &n(data_len)) {
            num = n((data_len - 8) / 8);
        }
        let ee = et.get_or_new_hash("ee");
        let start = Value::new_array(vec![]);
        ee.set("start", start.clone());
        let size = Value::new_array(vec![]);
        ee.set("size", size.clone());
        let mut i = 0i64;
        while pl::lt(&n(i), &num) {
            start.push(get32u(it, &data_pt, n(8 + i * 8))?);
            size.push(get32u(it, &data_pt, n(12 + i * 8))?);
            i += 1;
        }
        et.set("HandlerType", tag.clone()); // fake handler type
        process_samples_impl(it, &et)?; // we have all we need to process sample data now
    } else if t("GPS ") {
        let mut pos = 0i64;
        let tag_tbl = get_tag_table(it, "Image::ExifTool::QuickTime::Stream")?;
        set_byte_order(it, s("II"))?;
        while pos + 36 < data_len {
            let dat = sub_str(&data, pos, 36);
            if dat.to_bytes()[..] == [0u8; 36][..] {
                break;
            }
            let av = pl::unpack("VVVVaVaV", &dat)?;
            new_doc(&et);
            // 0=1, 1=1, 2=secs, 3=?
            set_gps_dt(it, &et, &tag_tbl, arg(&av, 2), Value::Undef)?;
            let mut lat = pl::div(&arg(&av, 5), &f(1e3))?;
            let mut lon = pl::div(&arg(&av, 7), &f(1e3))?;
            convert_lat_lon(&mut lat, &mut lon)?;
            if arg(&av, 4).eq_str("S") {
                lat = pl::neg(&pl::abs(&lat));
            }
            if arg(&av, 6).eq_str("W") {
                lon = pl::neg(&pl::abs(&lon));
            }
            ht(it, &et, &tag_tbl, "GPSLatitude", lat)?;
            ht(it, &et, &tag_tbl, "GPSLongitude", lon)?;
            ht(it, &et, &tag_tbl, "GPSSpeed", pl::div(&arg(&av, 3), &f(1e3))?)?;
            pos += 36;
        }
        set_byte_order(it, s("MM"))?;
        et.delete("DOC_NUM");
    }
    Ok(())
}

/// Process Yuneec 'tx3g' and Autel sbtl metadata (ref PH)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_tx3g(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    pl::at(2591);
    let data = pl::sderef(&dir_info.get("DataPt"))?;
    if len_of(&data) < 2 {
        return ret(n(0));
    }
    m(it, &et, "VerboseDir", vec![s("tx3g"), Value::Undef, n(len_of(&data) - 2)])?;
    let text = pl::substr(&data, 2, None); // remove 2-byte length word
    ht(it, &et, &tag_table_ptr, "Text", text.clone())?;
    let tt = &tag_table_ptr;
    if re(r"^HOME\(", "").is_match(&text) {
        // --- sample text from Autel Evo II drone ---
        // HOME(W: 109.318642, N: 40.769371) 2023-09-12 10:28:07
        // GPS(W: 109.339287, N: 40.768574, 2371.76m)
        // HDR ISO:100 SHUTTER:1000 EV:-0.7 F-NUM:1.8
        // F.PRY (1.0\xc2\xb0, -3.7\xc2\xb0, -59.0\xc2\xb0), G.PRY (-51.1\xc2\xb0, 0.0\xc2\xb0, -58.9\xc2\xb0)
        for line in pl::split(&re(r"\x0a", ""), &text, 0) {
            if let Some(c) = mt(r"^HOME\(([EW]):\s*(\d+\.\d+),\s*([NS]):\s*(\d+\.\d+)\)\s*(.*)", "", &line) {
                let (mut lon, mut lat, mut time) = (cap(&c, 2), cap(&c, 4), cap(&c, 5));
                if cap(&c, 1).eq_str("W") {
                    lon = pl::neg(&lon);
                }
                if cap(&c, 3).eq_str("S") {
                    lat = pl::neg(&lat);
                }
                pl::tr(&mut time, "-", ":", ""); // (likely local time zone, but not confirmed)
                ht(it, &et, tt, "GPSDateTime", time)?;
                ht(it, &et, tt, "HomeLat", lat)?;
                ht(it, &et, tt, "HomeLon", lon)?;
            } else if let Some(c) = mt(r"^GPS\(([EW]):\s*(\d+\.\d+),\s*([NS]):\s*(\d+\.\d+),\s*(.*)m", "", &line) {
                let (mut lon, mut lat, alt) = (cap(&c, 2), cap(&c, 4), cap(&c, 5));
                if cap(&c, 1).eq_str("W") {
                    lon = pl::neg(&lon);
                }
                if cap(&c, 3).eq_str("S") {
                    lat = pl::neg(&lat);
                }
                ht(it, &et, tt, "Lat", lat)?;
                ht(it, &et, tt, "Lon", lon)?;
                ht(it, &et, tt, "Alt", alt)?;
            } else if let Some(c) = mt(r"^F\.PRY\s*\((-?[\d.]+)\xc2\xb0,\s*(-?[\d.]+)\xc2\xb0,\s*(-?[\d.]+)\xc2\xb0", "", &line) {
                ht(it, &et, tt, "Yaw", cap(&c, 1))?;
                ht(it, &et, tt, "Pitch", cap(&c, 2))?;
                ht(it, &et, tt, "Roll", cap(&c, 3))?;
                if let Some(c) = mt(r"G\.PRY\s*\((-?[\d.]+)\xc2\xb0,\s*(-?[\d.]+)\xc2\xb0,\s*(-?[\d.]+)\xc2\xb0", "", &line) {
                    ht(it, &et, tt, "GimYaw", cap(&c, 1))?;
                    ht(it, &et, tt, "GimPitch", cap(&c, 2))?;
                    ht(it, &et, tt, "GimRoll", cap(&c, 3))?;
                }
            } else {
                for c in re(r"([-\w]+):([^:]*[^:\s])(\s|$)", "s").each(&line) {
                    m(it, &et, "HandleTag", vec![tt.clone(), cap(&c, 1), cap(&c, 2)])?;
                }
            }
        }
    } else if let Some(c) = mt(r"^\w{3} (\d{4})-(\d{2})-(\d{2}) (\d{2}:\d{2}:\d{2}) ?([-+])(\d{2}):?(\d{2})$", "s", &text) {
        let v = cat(&[&cap(&c, 1), &s(":"), &cap(&c, 2), &s(":"), &cap(&c, 3), &s(" "), &cap(&c, 4), &cap(&c, 5), &cap(&c, 6), &s(":"), &cap(&c, 7)]);
        ht(it, &et, tt, "DateTime", v)?;
    } else {
        for c in re(r"(\w+):([^:]*[^:\s])(\s|$)", "s").each(&text) {
            m(it, &et, "HandleTag", vec![tt.clone(), cap(&c, 1), cap(&c, 2)])?;
        }
    }
    ret(n(1))
}

/// Process QuickTime 'mebx' timed metadata
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
/// - uses tag ID keys stored in the ExifTool ee data member by a previous call to SaveMetaKeys
pub fn process_mebx(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_tbl) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let ee = et.get("ee");
    if !ee.truthy() {
        return ret(n(0));
    }
    if !ee.get("keys").truthy() {
        return ret(n(0));
    }
    let data_pt = dir_info.get("DataPt");
    pl::at(2652);
    let data = pl::sderef(&data_pt)?;
    let dl = len_of(&data);

    // parse using information from 'keys' table (eg. Apple iPhone7+ hevc 'Core Media Data Handler')
    m(it, &et, "VerboseDir", vec![s("mebx"), Value::Undef, n(dl)])?;
    let mut pos = 0i64;
    while pos + 8 < dl {
        let len = get32u(it, &data_pt, n(pos))?.to_i64();
        if len < 8 || pos + len > dl {
            break;
        }
        let id = sub_str(&data, pos + 4, 4);
        let info = ee.get("keys").get(id.to_bytes());
        'rec: {
            if info.truthy() {
                let tag = info.get("TagID");
                if !tag_tbl.get(tag.to_bytes()).truthy() {
                    if !re(r"^[-\w.]+$", "").is_match(&tag) {
                        break 'rec;
                    }
                    // create info for tags with reasonable id's
                    let mut name = tag.clone();
                    re(r"[-.](.)", "").subst(&mut name, true, |c| cap(c, 1).to_bytes().to_ascii_uppercase());
                    add_tag_to_table(it, &tag_tbl, tag.clone(), hash_of(vec![("Name", pl::ucfirst(&name))]))?;
                }
                let val = read_value(it, &data_pt, n(pos + 8), info.get("Format"), Value::Undef, n(len - 8))?;
                ht_v(it, &et, &tag_tbl, tag, val, vec![("DataPt", data_pt.clone()), ("Base", dir_info.get("Base")), ("Start", n(pos + 8)), ("Size", n(len - 8))])?;
            } else {
                let p = printable_tag_id(it, vec![id.clone(), n(1)])?;
                warn(it, &et, cat(&[&s("No key information for mebx ID "), &p]))?;
            }
        }
        pos += len;
    }
    ret(n(1))
}

/// `EEWarn($et)` (QuickTime.pm)
fn ee_warn(it: &mut Interp, et: &Value) -> ER<Value> {
    pl::call(it, "Image::ExifTool::QuickTime::EEWarn", vec![et.clone()])
}

/// Process QuickTime '3gf' timed metadata (ref PH, Pittasoft Blackvue dashcam)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_3gf(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_tbl) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let mut dir_len = dir_info.get("DirLen");
    let rec_len = n(10); // 10-byte record length
    m(it, &et, "VerboseDir", vec![s("3gf"), Value::Undef, dir_len.clone()])?;
    if pl::gt(&dir_len, &rec_len) && !opt(it, &et, "ExtractEmbedded")?.truthy() {
        dir_len = rec_len.clone();
        ee_warn(it, &et)?;
    }
    let mut pos = n(0);
    while pl::le(&pl::add(&pos, &rec_len), &dir_len) {
        new_doc(&et);
        let tc = get32u(it, &data_pt, pos.clone())?;
        if pl::num_eq(&tc, &n(0xffffffff)) {
            break;
        }
        let x = pl::div(&get16s(it, &data_pt, pl::add(&pos, &n(4)))?, &n(10))?;
        let y = pl::div(&get16s(it, &data_pt, pl::add(&pos, &n(6)))?, &n(10))?;
        let z = pl::div(&get16s(it, &data_pt, pl::add(&pos, &n(8)))?, &n(10))?;
        ht(it, &et, &tag_tbl, "TimeCode", pl::div(&tc, &n(1000))?)?;
        ht(it, &et, &tag_tbl, "Accelerometer", join_sp(&[x, y, z]))?;
        pos = pl::add(&pos, &rec_len);
    }
    et.delete("DOC_NUM");
    ret(n(1))
}

/// Process DuDuBell M1 dashcam / VSYS M6L 'gps0' atom (ref PH)
/// (Lamax S9 dual dashcam also uses 'gps0' atom, but encrypted text format)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_gps0(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_tbl) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let data = data_pt.deref();
    let mut dir_len = dir_info.get("DirLen");
    m(it, &et, "VerboseDir", vec![s("gps0"), Value::Undef, dir_len.clone()])?;
    // check for encrypted format written by Lamax S9 dual dashcam
    // (similar to Ambarella A12, but in multiple 311-byte records)
    let enc = re(r"^.{2}\xf2\xe1\xf0\xeeTT\x98", "s");
    pl::at(2724);
    check_sref(&data_pt)?;
    if enc.is_match(&data) {
        let rec_len = 311;
        let mut pos = 0i64;
        while pl::le(&n(pos + rec_len), &dir_len) {
            let dat = sub_str(&data, pos, rec_len);
            if !enc.is_match(&dat) {
                break;
            }
            new_doc(&et);
            process_text_impl(it, &et, pl::sref(dat), &tag_tbl, &Value::Undef)?;
            pos += rec_len;
            pos += rec_len;
        }
        et.delete("DOC_NUM");
        return ret(n(1));
    }
    let rec_len = n(32); // 32-byte record length
    set_byte_order(it, s("II"))?;
    if pl::gt(&dir_len, &rec_len) && !opt(it, &et, "ExtractEmbedded")?.truthy() {
        dir_len = rec_len.clone();
        ee_warn(it, &et)?;
    }
    let mut pos = 0i64;
    while pl::le(&n(pos + 32), &dir_len) {
        'rec: {
            new_doc(&et);
            // lat/long are in DDDMM.MMMM format
            let mut lat = get_double(it, &data_pt, n(pos))?;
            let mut lon = get_double(it, &data_pt, n(pos + 8))?;
            if pl::gt(&pl::abs(&lat), &n(9000)) || pl::gt(&pl::abs(&lon), &n(18000)) {
                break 'rec;
            }
            convert_lat_lon(&mut lat, &mut lon)?;
            let mut av: Vec<Value> = sub_str(&data, pos + 22, 6).to_bytes().iter().map(|&b| n(b as i64)).collect(); // unpack date/time
            if av.is_empty() {
                av.push(Value::Undef);
            }
            av[0] = pl::add(&av[0], &n(2000));
            ht(it, &et, &tag_tbl, "GPSDateTime", pl::sprintf("%.4d:%.2d:%.2d %.2d:%.2d:%.2dZ", &av))?;
            ht(it, &et, &tag_tbl, "GPSLatitude", lat)?;
            ht(it, &et, &tag_tbl, "GPSLongitude", lon)?;
            let v = get16u(it, &data_pt, n(pos + 0x14))?;
            ht(it, &et, &tag_tbl, "GPSSpeed", v)?;
            let v = get8u(it, &data_pt, n(pos + 0x1c))?;
            ht(it, &et, &tag_tbl, "GPSTrack", pl::mul(&v, &n(2)))?; // (NC)
            let v = get32s(it, &data_pt, n(pos + 0x10))?;
            ht(it, &et, &tag_tbl, "GPSAltitude", v)?;
            // yet to be decoded:
            // 0x1d - int8u[3] seen: "1 1 0"
        }
        pos += 32;
    }
    et.delete("DOC_NUM");
    set_byte_order(it, s("MM"))?;
    ret(n(1))
}

/// Process DuDuBell M1 dashcam / VSYS M6L 'gsen' atom (ref PH)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_gsen(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_tbl) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let data = data_pt.deref();
    let mut dir_len = dir_info.get("DirLen");
    let rec_len = n(3); // 3-byte record length
    m(it, &et, "VerboseDir", vec![s("gsen"), Value::Undef, dir_len.clone()])?;
    if pl::gt(&dir_len, &rec_len) && !opt(it, &et, "ExtractEmbedded")?.truthy() {
        dir_len = rec_len.clone();
        ee_warn(it, &et)?;
    }
    let mut pos = 0i64;
    while pl::le(&n(pos + 3), &dir_len) {
        new_doc(&et);
        pl::at(2783);
        check_sref(&data_pt)?;
        let accv: Vec<Value> = pl::unpack(&format!("x{pos}c3"), &data)?.iter().map(|v| pl::div(v, &n(16))).collect::<ER<Vec<Value>>>()?;
        ht(it, &et, &tag_tbl, "Accelerometer", join_sp(&accv))?;
        // (there are no associated timestamps, but these are sampled at 5 Hz in my test video)
        pos += 3;
    }
    et.delete("DOC_NUM");
    ret(n(1))
}

/// Process 'gdat' atom Base64-encoded JSON-format timed GPS used by Nextbase software (ref PH)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_gdat(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_tbl) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if !et.get("OPTIONS").get("ExtractEmbedded").truthy() {
        warn_lvl(it, &et, s("Use the ExtractEmbedded option to extract timed GPSData"), 3)?;
        return ret(n(0));
    }
    let mut data_pt = dir_info.get("DataPt");
    runtime::require(it, "Image::ExifTool::XMP")?;
    // (an argument of a call: an undefined $dataPt autovivifies)
    pl::at(2804);
    let d = if data_pt.is_undef() { Value::Undef } else { pl::sderef(&data_pt)? };
    data_pt = pl::call(it, "Image::ExifTool::XMP::DecodeBase64", vec![d])?;
    let dbase = Value::new_hash();
    runtime::require(it, "Image::ExifTool::Import")?;
    pl::call(it, "Image::ExifTool::Import::ReadJSON", vec![data_pt, dbase.clone()])?;
    let info = dbase.get("*");
    if !info.truthy() {
        return ret(n(0));
    }
    if info.get("cameraModel").truthy() {
        ht(it, &et, &tag_tbl, "CameraModel", info.get("cameraModel"))?;
    }
    let gps = info.get("gpsData");
    if !gps.truthy() {
        return ret(n(0));
    }
    if pl::ref_of(&gps) != "ARRAY" {
        return ret(n(0));
    }
    for fix in gps.items() {
        if !(pl::ref_of(&fix) == "HASH" && fix.get("gpsStatus").truthy() && fix.get("gpsStatus").eq_str("A")) {
            continue;
        }
        new_doc(&et);
        if fix.get("datetime").truthy() {
            let mut d = fix.get("datetime");
            pl::tr(&mut d, "-T", ": ", "");
            fix.set("datetime", d.clone());
            ht(it, &et, &tag_tbl, "GPSDateTime", d)?;
        }
        if defined(&fix.get("lat")) {
            ht(it, &et, &tag_tbl, "GPSLatitude", fix.get("lat"))?;
        }
        if defined(&fix.get("lon")) {
            ht(it, &et, &tag_tbl, "GPSLongitude", fix.get("lon"))?;
        }
        if defined(&fix.get("speed")) {
            ht(it, &et, &tag_tbl, "GPSSpeed", pl::mul(&fix.get("speed"), &mph_to_kph()))?;
        }
        if defined(&fix.get("bearing")) {
            ht(it, &et, &tag_tbl, "GPSTrack", fix.get("bearing"))?;
        }
        if defined(&fix.get("xAcc")) && defined(&fix.get("yAcc")) && defined(&fix.get("zAcc")) {
            ht(it, &et, &tag_tbl, "Accelerometer", join_sp(&[fix.get("xAcc"), fix.get("yAcc"), fix.get("zAcc")]))?;
        }
    }
    et.delete("DOC_NUM");
    ret(n(1))
}

/// Extract GPS from Nextbase 'nbmt' atom
/// Inputs: 0) ExifTool ref, 1) data ref or dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_nbmt(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, data_pt, tag_tbl) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));

    if et.get("OPTIONS").get("ExtractEmbedded").truthy() {
        et.set("DOC_NUM", pl::add(&et.get("DOC_COUNT"), &n(1)));
        et.delete("UnknownTextCount");
        et.delete("NoMoreTextDecoding");
        et.set("SET_GROUP1", s("Nextbase"));
        process_text_impl(it, &et, data_pt, &tag_tbl, &n(1))?;
        et.delete("SET_GROUP1");
        et.delete("UnknownTextCount");
        et.delete("NoMoreTextDecoding");
        et.delete("DOC_NUM");
    } else {
        warn_lvl(it, &et, s("Use the ExtractEmbedded option to extract timed GPSData"), 3)?;
    }
    ret(n(1))
}

/// Process Kenwood drv-a301w dashcam 'udta' atom (ref PH)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
/// Sample data:
/// 0000: 56 49 44 45 4f 55 55 55 55 55 55 55 55 55 55 55 [VIDEOUUUUUUUUUUU]
/// 0010: 55 55 55 55 55 55 55 55 55 55 55 fe fe 32 30 32 [UUUUUUUUUUU..202]
/// 0020: 33 30 31 30 37 31 31 31 39 31 34 2e 32 30 32 33 [30107111914.2023]
/// 0030: 30 31 30 37 31 31 31 39 31 35 03 4e 34 37 33 37 [0107111915.N4737]
/// 0040: 37 30 35 33 57 31 32 32 30 39 39 30 31 34 2b 30 [7053W122099014+0]
/// 0050: 30 35 38 30 30 30 2b 30 30 36 2b 30 30 39 2b 30 [058000+006+009+0]
/// 0060: 30 34 2b 30 30 32 2b 30 30 39 2b 30 30 35 2b 30 [04+002+009+005+0]
pub fn process_kenwood(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_tbl) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    pl::at(2872);
    let data = pl::sderef(&dir_info.get("DataPt"))?;
    let _dir_len = dir_info.get("DirLen");
    for c in re(r"\xfe\xfe([^\xfe]+)", "").each(&data) {
        let dat = cap(&c, 1);
        let Some((c1, p1)) = match_from(r"^(\d{4})(\d{2})(\d{2})(\d{2})(\d{2})(\d{2}).", "s", &dat, 0) else { continue };
        let time = cat(&[&cap(&c1, 1), &s(":"), &cap(&c1, 2), &s(":"), &cap(&c1, 3), &s(" "), &cap(&c1, 4), &s(":"), &cap(&c1, 5), &s(":"), &cap(&c1, 6)]); // (likely local time zone, but not confirmed)
        // ignore second date (what is this for?)
        let Some((_, p2)) = match_from(r"\G(\d{4})(\d{2})(\d{2})(\d{2})(\d{2})(\d{2}).", "s", &dat, p1) else { continue };
        let Some((c3, p3)) = match_from(r"\G([NS])(\d+)([EW])(\d+)", "", &dat, p2) else { continue };
        let (mut lat, mut lon) = (pl::div(&cap(&c3, 2), &f(1e4))?, pl::div(&cap(&c3, 4), &f(1e4))?);
        convert_lat_lon(&mut lat, &mut lon)?;
        new_doc(&et);
        ht(it, &et, &tag_tbl, "GPSDateTime", time)?;
        ht(it, &et, &tag_tbl, "GPSLatitude", pl::mul(&lat, &n(if cap(&c3, 1).eq_str("S") { -1 } else { 1 })))?;
        ht(it, &et, &tag_tbl, "GPSLongitude", pl::mul(&lon, &n(if cap(&c3, 3).eq_str("W") { -1 } else { 1 })))?;
        let Some((c4, mut p)) = match_from(r"\G([-+]\d{4})(\d+)", "", &dat, p3) else { continue };
        ht(it, &et, &tag_tbl, "GPSAltitude", pl::add(&cap(&c4, 1), &n(0)))?; // (NC, educated guess)
        ht(it, &et, &tag_tbl, "GPSSpeed", cap(&c4, 2))?; // (km/h)
        let mut accv = Vec::new();
        while let Some((c5, e)) = match_from(r"\G([-+]\d+)([-+]\d+)([-+]\d+)", "", &dat, p) {
            p = e;
            accv.push(pl::div(&cap(&c5, 1), &n(1000))?);
            accv.push(pl::div(&cap(&c5, 2), &n(1000))?);
            accv.push(pl::div(&cap(&c5, 3), &n(1000))?);
        }
        if !accv.is_empty() {
            ht(it, &et, &tag_tbl, "Accelerometer", join_sp(&accv))?;
        }
        if !opt(it, &et, "ExtractEmbedded")?.truthy() {
            warn_lvl(it, &et, s("Use the ExtractEmbedded option to extract all timed GPS"), 3)?;
            break;
        }
    }
    et.delete("DOC_NUM");
    ret(n(1))
}

/// Process RIFF-format trailer written by Auto-Vox dashcam (ref PH)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
/// Note: This trailer is basically RIFF chunks added to a QuickTime-format file (augh!),
///       but there are differences in the record formats so we can't just call
///       ProcessRIFF to process the gps0 and gsen atoms using the routines above
pub fn process_riff_trailer(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_tbl) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let raf = dir_info.get("RAF");
    let verbose = opt(it, &et, "Verbose")?;
    let mut buff = Value::Undef;
    set_byte_order(it, s("II"))?;
    loop {
        pl::at(2917);
        if !pl::num_eq(&raf_read(it, &raf, &mut buff, n(8))?, &n(8)) {
            break;
        }
        let u = pl::unpack("a4V", &buff)?;
        let (tag, len) = (arg(&u, 0), arg(&u, 1));
        if tag.eq_str("\0\0\0\0") {
            break;
        }
        if !(re(r"^[\w ]{4}", "").is_match(&tag) && pl::lt(&len, &n(0x2000000))) {
            warn(it, &et, s("Bad RIFF trailer"))?;
            break;
        }
        if !pl::num_eq(&raf_read(it, &raf, &mut buff, len.clone())?, &len) {
            warn(it, &et, cat(&[&s("Truncated "), &tag, &s(" record in RIFF trailer")]))?;
            break;
        }
        if verbose.truthy() {
            m(it, &et, "VPrint", vec![n(0), cat(&[&s("  - RIFF trailer '"), &tag, &s("' ("), &len, &s(" bytes)\n")])])?;
            if pl::gt(&verbose, &n(2)) {
                let addr = pl::sub(&raf_tell(it, &raf)?, &len);
                m(it, &et, "VerboseDump", vec![pl::sref(buff.clone()), s("Addr"), addr])?;
            }
            et.set("INDENT", cat(&[&et.get("INDENT"), &s("| ")]));
            if re(r"^(gps0|gsen)$", "").is_match(&tag) {
                m(it, &et, "VerboseDir", vec![tag.clone(), Value::Undef, len.clone()])?;
            }
        }
        let bp = pl::sref(buff.clone());
        if tag.eq_str("gps0") {
            // (similar to record decoded in Process_gps0, but with some differences)
            // 0000: 41 49 54 47 74 46 94 f6 c6 c5 b4 40 34 a2 b4 37 [AITGtF.....@4..7]
            // 0010: f8 7b 8a 40 ff ff 00 00 38 00 77 0a 1a 0c 12 28 [.{.@....8.w....(]
            // 0020: 8d 01 02 40 29 07 00 00                         [...@)...]
            // 0x00 - undef[4] 'AITG'
            // 0x04 - double   latitude  (always positive)
            // 0x0c - double   longitude (always positive)
            // 0x14 - ?        seen hex "ff ff 00 00" (altitude in Process_gps0 record below)
            // 0x18 - int16u   speed in knots (different than km/hr in Process_gps0)
            // 0x1a - int8u[6] yr-1900,mon,day,hr,min,sec (different than -2000 in Process_gps0)
            // 0x20 - int8u    direction in degrees / 2
            // 0x21 - int8u    guessing that this is 1=N, 2=S - PH
            // 0x22 - int8u    guessing that this is 1=E, 2=W - PH
            // 0x23 - ?        seen hex "40"
            // 0x24 - in32u    time since start of video (ms)
            let rec_len = 0x28;
            let mut pos = 0i64;
            while pl::lt(&n(pos + rec_len), &len) {
                if !sub_str(&buff, pos, 4).eq_str("AITG") {
                    warn(it, &et, s("Unrecognized gps0 record"))?;
                    break;
                }
                new_doc(&et);
                // lat/long are in DDDMM.MMMM format
                let mut lat = get_double(it, &bp, n(pos + 4))?;
                let mut lon = get_double(it, &bp, n(pos + 12))?;
                if pl::gt(&pl::abs(&lat), &n(9000)) || pl::gt(&pl::abs(&lon), &n(18000)) {
                    if warn(it, &et, s("Bad gps0 record"))?.truthy() {
                        break;
                    }
                }
                convert_lat_lon(&mut lat, &mut lon)?;
                if pl::num_eq(&get8u(it, &bp, n(pos + 0x21))?, &n(2)) {
                    lat = pl::neg(&lat); // wild guess
                }
                if pl::num_eq(&get8u(it, &bp, n(pos + 0x22))?, &n(2)) {
                    lon = pl::neg(&lon); // wild guess
                }
                let mut av: Vec<Value> = sub_str(&buff, pos + 26, 6).to_bytes().iter().map(|&b| n(b as i64)).collect(); // unpack date/time
                if av.is_empty() {
                    av.push(Value::Undef);
                }
                av[0] = pl::add(&av[0], &n(1900)); // (different than Proces_gps0)
                let v = get32u(it, &bp, n(pos + 0x24))?;
                ht(it, &et, &tag_tbl, "SampleTime", pl::div(&v, &n(1000))?)?;
                ht(it, &et, &tag_tbl, "GPSDateTime", pl::sprintf("%.4d:%.2d:%.2d %.2d:%.2d:%.2dZ", &av))?;
                ht(it, &et, &tag_tbl, "GPSLatitude", lat)?;
                ht(it, &et, &tag_tbl, "GPSLongitude", lon)?;
                let v = get16u(it, &bp, n(pos + 0x18))?;
                ht(it, &et, &tag_tbl, "GPSSpeed", pl::mul(&v, &knots_to_kph()))?;
                let v = get8u(it, &bp, n(pos + 0x20))?;
                ht(it, &et, &tag_tbl, "GPSTrack", pl::mul(&v, &n(2)))?;
                pos += rec_len;
            }
        } else if tag.eq_str("gsen") {
            // (similar to record decoded in Process_gsen)
            // 0000: 41 49 54 53 1a 0d 05 ff c8 00 00 00 [AITS........]
            // 0x00 - undef[4] 'AITS'
            // 0x04 - int8s[3] accelerometer readings
            // 0x07 - ?        seen hex "ff"
            // 0x08 - in32u    time since start of video (ms)
            let rec_len = 0x0c;
            let mut pos = 0i64;
            while pl::lt(&n(pos + rec_len), &len) {
                if !sub_str(&buff, pos, 4).eq_str("AITS") {
                    warn(it, &et, s("Unrecognized gsen record"))?;
                    break;
                }
                new_doc(&et);
                let accv: Vec<Value> = pl::unpack(&format!("x{}c3", pos + 4), &buff)?.iter().map(|v| pl::div(v, &n(24))).collect::<ER<Vec<Value>>>()?;
                let v = get32u(it, &bp, n(pos + 8))?;
                ht(it, &et, &tag_tbl, "SampleTime", pl::div(&v, &n(1000))?)?;
                // 0=+Up, 1=+Right, 3=+Forward (calibration of 24 counts/g is a wild guess - PH)
                ht(it, &et, &tag_tbl, "Accelerometer", join_sp(&accv))?;
                pos += rec_len;
            }
        }
        // also seen, but not decoded:
        // gpsa (8 bytes): hex "01 20 00 00 08 03 02 08 "
        // gsea (20 bytes): all zeros
        if verbose.truthy() {
            et.set("INDENT", pl::substr(&et.get("INDENT"), 0, Some(-2)));
        }
    }
    et.delete("DOC_NUM");
    set_byte_order(it, s("MM"))?;
    ret(n(1))
}

/// Process Kenwood Dashcam trailer (forum16229)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
/// Sample data (chained 512-byte records starting like this):
/// 0000:  43 43 43 43 43 43 43 43 43 43 43 43 43 43 47 50 [CCCCCCCCCCCCCCGP]
/// 0010:  53 44 41 54 41 2d 2d 32 30 32 34 30 37 31 31 31 [SDATA--202407111]
/// 0020:  32 30 34 31 32 4e 35 30 2e 36 31 32 33 38 36 30 [20412N50.6123860]
/// 0030:  36 37 37 45 38 2e 37 30 32 37 31 38 30 39 38 39 [677E8.7027180989]
/// 0040:  35 33 33 2e 30 30 30 30 30 30 30 30 30 30 30 30 [533.000000000000]
/// 0050:  2e 30 30 30 30 30 30 30 30 30 30 30 30 30 2e 30 [.0000000000000.0]
/// 0060:  31 39 39 39 39 39 39 39 35 35 33 2d 30 2e 30 39 [19999999553-0.09]
/// 0070:  30 30 30 30 30 30 33 35 37 2d 30 2e 31 34 30 30 [000000357-0.1400]
/// 0080:  30 30 30 30 30 35 39 47 50 53 44 41 54 41 2d 2d [0000059GPSDATA--]
pub fn process_kenwood_trailer(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_tbl) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let raf = dir_info.get("RAF");
    let mut buff = Value::Undef;
    // current file position is 8 bytes into the 14 C's, so test the next 6:
    pl::at(3014);
    if !(raf_read(it, &raf, &mut buff, n(14))?.truthy() && buff.eq_str("CCCCCCCCCCCCCC")) {
        return ret(n(0));
    }
    m(it, &et, "VerboseDir", vec![s("Kenwood trailer"), Value::Undef, Value::Undef])?;
    if !et.get("OPTIONS").get("ExtractEmbedded").truthy() {
        warn_lvl(it, &et, s("Use the ExtractEmbedded option to extract timed GPSData from Kenwood trailer"), 3)?;
        return ret(n(1));
    }
    loop {
        if !raf_read(it, &raf, &mut buff, n(121))?.truthy() {
            break;
        }
        let Some(c) = mt(r"^GPSDATA--(\d{4})(\d{2})(\d{2})(\d{2})(\d{2})(\d{2})", "", &buff) else { break };
        found_some(it, &et, &tag_tbl, &Value::Undef, &Value::Undef)?;
        ht(it, &et, &tag_tbl, "GPSDateTime", cat(&[&cap(&c, 1), &s(":"), &cap(&c, 2), &s(":"), &cap(&c, 3), &s(" "), &cap(&c, 4), &s(":"), &cap(&c, 5), &s(":"), &cap(&c, 6)]))?;
        let mut i = 9 + 14;
        let mut accv: Vec<Value> = Vec::new();
        for tag in ["GPSLatitude", "GPSLongitude", "GPSSpeed", "unk", "acc", "acc", "acc"] {
            let mut val = sub_str(&buff, i, 14);
            i += 14;
            if tag == "unk" {
                continue;
            }
            let mut hemi = Value::Undef;
            re(r"^([NSEW])", "").subst(&mut val, false, |c| {
                hemi = cap(c, 1);
                Vec::new()
            });
            if !re(r"^[-+]?\d+\.\d+$", "").is_match(&val) {
                continue;
            }
            if tag == "acc" {
                accv.push(val);
                continue;
            }
            if hemi.truthy() && (hemi.eq_str("S") || hemi.eq_str("W")) {
                val = pl::neg(&val);
            }
            ht(it, &et, &tag_tbl, tag, val)?;
        }
        if accv.len() == 3 {
            ht(it, &et, &tag_tbl, "Accelerometer", join_sp(&accv))?;
        }
    }
    et.delete("DOC_NUM");
    ret(n(1))
}

/// Process 'gps ' atom containing NMEA from Pittasoft Blackvue dashcam (ref PH)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_nmea(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_tbl) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    ret(nmea(it, &et, &dir_info, &tag_tbl)?)
}

fn nmea(it: &mut Interp, et: &Value, dir_info: &Value, tag_tbl: &Value) -> ER<Value> {
    pl::at(3053);
    let data = pl::sderef(&dir_info.get("DataPt"))?;
    let rtn_val = Value::Undef;
    let fix = Value::new_hash();
    let mut p = 0usize; // pos($$dataPt)
    // parse only RMC and GGA sentence [with leading timecode] for now
    loop {
        let (mut tc, mut typ, mut tim) = (Value::Undef, Value::Undef, Value::Undef);
        if let Some((c, e)) = match_from(r"(?:\[(\d+)\])?\$[A-Z]{2}(RMC|GGA),(\d{2}\d{2}\d+(\.\d*)?),", "", &data, p) {
            p = e;
            tc = cap(&c, 1);
            typ = cap(&c, 2);
            tim = cap(&c, 3);
        } else {
            p = 0;
        }
        // write out last fix now if complete
        // (use the GPS timestamps because they may be different for the same timecode)
        if fix.get("tim").truthy() && (!tim.truthy() || !pl::num_eq(&fix.get("tim"), &tim)) {
            if fix.get("dat").truthy() && defined(&fix.get("lat")) && defined(&fix.get("lon")) {
                let mut sample_time = Value::Undef;
                if fix.get("tc").truthy() && et.get("StartTime").truthy() {
                    sample_time = pl::div(&pl::sub(&fix.get("tc"), &et.get("StartTime")), &n(1000))?;
                }
                found_some(it, et, tag_tbl, &sample_time, &Value::Undef)?;
                ht(it, et, tag_tbl, "GPSDateTime", fix.get("dat"))?;
                ht(it, et, tag_tbl, "GPSLatitude", fix.get("lat"))?;
                ht(it, et, tag_tbl, "GPSLongitude", fix.get("lon"))?;
                if defined(&fix.get("spd")) {
                    ht(it, et, tag_tbl, "GPSSpeed", pl::mul(&fix.get("spd"), &knots_to_kph()))?;
                }
                if defined(&fix.get("trk")) {
                    ht(it, et, tag_tbl, "GPSTrack", fix.get("trk"))?;
                }
                if defined(&fix.get("alt")) {
                    ht(it, et, tag_tbl, "GPSAltitude", fix.get("alt"))?;
                }
                if defined(&fix.get("nsats")) {
                    ht(it, et, tag_tbl, "GPSSatellites", pl::add(&fix.get("nsats"), &n(0)))?;
                }
                if defined(&fix.get("hdop")) {
                    ht(it, et, tag_tbl, "GPSDOP", fix.get("hdop"))?;
                }
            }
            if let Value::HashRef(h) = &fix {
                h.borrow_mut().clear();
            }
        }
        fix.set("tim", tim.clone());
        if !tim.truthy() {
            break;
        }
        let pos = p;
        p = pos - len_of(&tim) as usize - 1; // rewind to re-parse time
        // (parsing of NMEA strings copied from Geotag.pm)
        let rmc = if typ.eq_str("RMC") { match_from(r"\G(\d{2})(\d{2})(\d+(\.\d*)?),A?,(\d*?)(\d{1,2}\.\d+),([NS]),(\d*?)(\d{1,2}\.\d+),([EW]),(\d*\.?\d*),(\d*\.?\d*),(\d{2})(\d{2})(\d+)", "", &data, p) } else { None };
        if let Some((c, e)) = rmc {
            p = e;
            let c15 = cap(&c, 15);
            let year = pl::add(&c15, &n(if pl::ge(&c15, &n(70)) { 1900 } else { 2000 }));
            fix.set("tc", tc.clone()); // use timecode of RMC sentence
            fix.set("dat", pl::sprintf("%.4d:%.2d:%.2d %.2d:%.2d:%sZ", &[year, cap(&c, 14), cap(&c, 13), cap(&c, 1), cap(&c, 2), cap(&c, 3)]));
            fix.set("lat", deg_min(&cap(&c, 5), &cap(&c, 6), if cap(&c, 7).eq_str("N") { 1 } else { -1 })?);
            fix.set("lon", deg_min(&cap(&c, 8), &cap(&c, 9), if cap(&c, 10).eq_str("E") { 1 } else { -1 })?);
            if len_of(&cap(&c, 11)) > 0 {
                fix.set("spd", cap(&c, 11));
            }
            if len_of(&cap(&c, 12)) > 0 {
                fix.set("trk", cap(&c, 12));
            }
            continue;
        }
        let gga = if typ.eq_str("GGA") { match_from(r"\G(\d{2})(\d{2})(\d+(\.\d*)?),(\d*?)(\d{1,2}\.\d+),([NS]),(\d*?)(\d{1,2}\.\d+),([EW]),[1-6]?,(\d+)?,(\.\d+|\d+\.?\d*)?,(-?\d+\.?\d*)?,M?", "", &data, p) } else { None };
        if let Some((c, e)) = gga {
            p = e;
            fix.set("lat", deg_min(&cap(&c, 5), &cap(&c, 6), if cap(&c, 7).eq_str("N") { 1 } else { -1 })?);
            fix.set("lon", deg_min(&cap(&c, 8), &cap(&c, 9), if cap(&c, 10).eq_str("E") { 1 } else { -1 })?);
            fix.set("nsats", cap(&c, 11));
            fix.set("hdop", cap(&c, 12));
            fix.set("alt", cap(&c, 13));
        } else {
            p = pos; // continue searching from our last match
        }
    }
    et.delete("DOC_NUM");
    Ok(rtn_val)
}

/// Process 'gps ' or 'udat' atom possibly containing NMEA (ref PH)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_gps_log(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_tbl) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data = dir_info.get("DataPt").deref();
    let mut rtn_val = Value::Undef;

    // try NMEA format first
    if nmea(it, &et, &dir_info, &tag_tbl)?.truthy() {
        return ret(n(1));
    }

    // DENVER ACG-8050WMK2 format looks like this:
    // 210318073213[1][N][52200970][E][006362321][+00152][100][00140][C000000]+000+000+000+000+000+000+000+000+000+000+000+000+000+000+000+000+000+000
    // YYMMDDHHMMSS A? NS lat       EW lon         alt    kph  dir    kCal    accel
    for c in re(r"\b(\d{2})(\d{2})(\d{2})(\d{2})(\d{2})(\d{2})\[1\]\[([NS])\]\[(\d{8})\]\[([EW])\]\[(\d{9})\]\[([-+]?\d*)\]\[(\d*)\]\[(\d*)\]\[C?(\d*)\](([-+]\d{3})+)", "").each(&data) {
        let (c8, c10) = (cap(&c, 8), cap(&c, 10));
        let lat = pl::add(&pl::substr(&c8, 0, Some(2)), &pl::div(&pl::substr(&c8, 2, None), &n(600000))?);
        let lon = pl::add(&pl::substr(&c10, 0, Some(3)), &pl::div(&pl::substr(&c10, 3, None), &n(600000))?);
        new_doc(&et);
        ht(it, &et, &tag_tbl, "GPSDateTime", cat(&[&s("20"), &cap(&c, 1), &s(":"), &cap(&c, 2), &s(":"), &cap(&c, 3), &s(" "), &cap(&c, 4), &s(":"), &cap(&c, 5), &s(":"), &cap(&c, 6), &s("Z")]))?;
        ht(it, &et, &tag_tbl, "GPSLatitude", pl::mul(&lat, &n(if cap(&c, 7).eq_str("S") { -1 } else { 1 })))?;
        ht(it, &et, &tag_tbl, "GPSLongitude", pl::mul(&lon, &n(if cap(&c, 9).eq_str("W") { -1 } else { 1 })))?;
        if len_of(&cap(&c, 11)) > 0 {
            ht(it, &et, &tag_tbl, "GPSAltitude", pl::div(&cap(&c, 11), &n(10))?)?;
        }
        if len_of(&cap(&c, 12)) > 0 {
            ht(it, &et, &tag_tbl, "GPSSpeed", pl::add(&cap(&c, 12), &n(0)))?;
        }
        if len_of(&cap(&c, 13)) > 0 {
            ht(it, &et, &tag_tbl, "GPSTrack", pl::add(&cap(&c, 13), &n(0)))?;
        }
        if len_of(&cap(&c, 14)) > 0 {
            ht(it, &et, &tag_tbl, "KiloCalories", pl::div(&cap(&c, 14), &n(10))?)?;
        }
        if len_of(&cap(&c, 15)) > 0 {
            ht(it, &et, &tag_tbl, "Accelerometer", cap(&c, 15))?;
        }
        rtn_val = n(1);
    }
    et.delete("DOC_NUM");
    ret(rtn_val)
}

/// Process TomTom Bandit Action Cam TTAD atom (ref PH)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_ttad(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_tbl) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let dir_len = dir_info.get("DirLen");
    let mut pos = n(76);

    if pl::lt(&dir_len, &pos) {
        return ret(n(0));
    }

    m(it, &et, "VerboseDir", vec![s("TTAD"), Value::Undef, dir_len.clone()])?;
    set_byte_order(it, s("II"))?;

    let tt_len = lex_hash(it, "ttLen");
    let ee_opt = opt(it, &et, "ExtractEmbedded")?;
    let unknown = opt(it, &et, "Unknown")?;
    let mut found = 0i64;
    let mut sample_time = n(0);
    let mut resync = true;
    let mut skipped = 0i64;
    let mut warned = false;

    while pl::lt(&pos, &dir_len) {
        // get next record type
        let typ = get8u(it, &data_pt, pos.clone())?;
        pos = pl::add(&pos, &n(1));
        let ty = typ.to_i64();
        // resync if necessary by skipping data until next timecode record
        if resync && ty != 0xff {
            skipped += 1;
            if skipped > 0x100 && warn_lvl(it, &et, s("Unrecognized or bad TTAD data"), 1)?.truthy() {
                break;
            }
            continue;
        }
        let tl = tt_len.get(typ.to_bytes());
        if !tl.truthy() {
            // skip unknown records
            if !warned {
                warn_lvl(it, &et, cat(&[&s("Unknown TTAD record type "), &typ]), 1)?;
            }
            resync = true;
            warned = true;
            skipped += 1;
            continue;
        }
        if pl::gt(&pl::add(&pos, &tl), &dir_len) {
            break;
        }
        if ty == 0xff {
            // timecode?
            let tm = get32u(it, &data_pt, pos.clone())?;
            // validate timecode if skipping unknown data
            if resync {
                if pl::lt(&tm, &sample_time) || pl::gt(&tm, &pl::add(&sample_time, &n(250))) {
                    skipped += 1;
                    continue;
                }
                resync = false;
                skipped = 0;
            }
            pos = pl::add(&pos, &tl);
            sample_time = tm;
            continue;
        }
        if !ee_opt.truthy() {
            // only extract one of each type without -ee option
            if found & (1 << ty) != 0 {
                pos = pl::add(&pos, &tl);
                continue;
            }
            found |= 1 << ty;
        }
        if ty == 0 || ty == 3 {
            // (these are both just educated guesses - PH)
            found_some(it, &et, &tag_tbl, &pl::div(&sample_time, &n(1000))?, &Value::Undef)?;
            let mut av = Vec::new();
            for k in 0..3 {
                av.push(pl::div(&get32s(it, &data_pt, pl::add(&pos, &n(4 * k)))?, &n(1000))?);
            }
            ht(it, &et, &tag_tbl, if ty != 0 { "Accelerometer" } else { "AngularVelocity" }, join_sp(&av))?;
        } else if ty == 5 {
            // example records unpacked with 'dVddddVddddv*'
            // datetime                 ? spd  ele    lat        lon       ? trk   ?     ?      ?      ? ? ? ?     ? ?
            // 2019:03:05 07:52:58.999Z 3 0.02 242    48.0254203 7.8497567 0 45.69 13.34 17.218 17.218 0 0 0 32760 5 0
            // 2019:03:05 07:52:59.999Z 3 0.14 242    48.0254203 7.8497567 0 45.7  12.96 15.662 15.662 0 0 0 32760 5 0
            // 2019:03:05 07:53:00.999Z 3 0.67 243.78 48.0254584 7.8497907 0 50.93  9.16 10.84  10.84  0 0 0 32760 5 0
            // (I think "5" may be the number of satellites.  seen: 5,6,7 - PH)
            found_some(it, &et, &tag_tbl, &pl::div(&sample_time, &n(1000))?, &Value::Undef)?;
            let t = get_double(it, &data_pt, pos.clone())?;
            let d = convert_unix_time(it, vec![t, Value::Undef, n(3)])?;
            ht(it, &et, &tag_tbl, "GPSDateTime", cat(&[&d, &s("Z")]))?;
            let v = get_double(it, &data_pt, pl::add(&pos, &n(0x1c)))?;
            ht(it, &et, &tag_tbl, "GPSLatitude", v)?;
            let v = get_double(it, &data_pt, pl::add(&pos, &n(0x24)))?;
            ht(it, &et, &tag_tbl, "GPSLongitude", v)?;
            let v = get_double(it, &data_pt, pl::add(&pos, &n(0x14)))?;
            ht(it, &et, &tag_tbl, "GPSAltitude", v)?;
            let v = get_double(it, &data_pt, pl::add(&pos, &n(0x0c)))?;
            ht(it, &et, &tag_tbl, "GPSSpeed", pl::mul(&v, &mps_to_kph()))?;
            let v = get_double(it, &data_pt, pl::add(&pos, &n(0x30)))?;
            ht(it, &et, &tag_tbl, "GPSTrack", v)?;
            if unknown.truthy() {
                let mut av = Vec::new();
                for k in 0..3 {
                    av.push(get_double(it, &data_pt, pl::add(&pos, &n(0x38 + 8 * k)))?);
                }
                ht(it, &et, &tag_tbl, "Unknown03", join_sp(&av))?;
            }
        } else if ty < 3 {
            // as yet unknown:
            // 1 - int32s[1]? (values around 98k)
            // 2 - int32s[3] (values like "806 8124 4323" -- probably something * 1000 again)
            if unknown.truthy() {
                found_some(it, &et, &tag_tbl, &pl::div(&sample_time, &n(1000))?, &Value::Undef)?;
                let nn = if ty == 1 { 0 } else { 2 };
                let mut av = Vec::new();
                for k in 0..=nn {
                    av.push(get32s(it, &data_pt, pl::add(&pos, &n(4 * k)))?);
                }
                m(it, &et, "HandleTag", vec![tag_tbl.clone(), cat(&[&s("Unknown0"), &typ]), join_sp(&av)])?;
            }
        } else {
            warn_lvl(it, &et, cat(&[&s("Unknown TTAD record type "), &typ]), 1)?;
        }
        // without -ee, stop after we find types 0,3,5 (ie. bitmask 0x29)
        if !ee_opt.truthy() && (found & 0x29) == 0x29 {
            ee_warn(it, &et)?;
            break;
        }
        pos = pl::add(&pos, &tl);
    }
    set_byte_order(it, s("MM"))?;
    et.delete("DOC_NUM");
    ret(n(1))
}

/// Extract information from Insta360 trailer (INSV, INSP and MP4 files) or 'inst' box (ref PH)
/// Inputs: 0) ExifTool ref, 1) Optional dirInfo ref for returning trailer info
/// (dirInfo has Offset from end of trailer to end of file or DirEnd absolute end of trailer)
/// Returns: true on success
/// Notes: There looks to be some useful information by telemetry-parser, but
///        the code is cryptic:  https://github.com/AdrianEddy/telemetry-parser
pub fn process_insta360(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| process_insta360_impl(it, &a))
}

fn process_insta360_impl(it: &mut Interp, a: &[Value]) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(a, 0), arg(a, 1));
    let raf = et.get("RAF");
    let mut offset = if dir_info.truthy() { or(dir_info.get("Offset"), n(0)) } else { n(0) };
    let mut buff = Value::Undef;
    let (mut dir_table, mut dir_table_pos) = (Value::Undef, 0i64);

    if dir_info.truthy() && dir_info.get("DirEnd").truthy() {
        pl::at(3267);
        raf_seek(it, &raf, n(0), 2)?;
        offset = pl::sub(&raf_tell(it, &raf)?, &dir_info.get("DirEnd"));
    }
    pl::at(3270);
    let ok = raf_seek(it, &raf, pl::sub(&n(-78), &offset), 2)? && pl::num_eq(&raf_read(it, &raf, &mut buff, n(78))?, &n(78)) && pl::substr(&buff, -32, None).eq_str("8db42d694ccc418790edff439fe026bf"); // check magic number
    if !ok {
        return ret(n(0));
    }

    let verbose = opt(it, &et, "Verbose")?;
    let tag_tbl = get_tag_table(it, "Image::ExifTool::QuickTime::Stream")?;
    let trail_end = raf_tell(it, &raf)?;
    let trailer_len = pl::unpack1("x38V", &buff)?;
    if pl::gt(&trailer_len, &trail_end) {
        warn(it, &et, s("Bad Insta360 trailer size"))?;
        return ret(n(0));
    }
    if dir_info.truthy() {
        dir_info.set("DirLen", trailer_len.clone());
        dir_info.set("DataPos", pl::sub(&trail_end, &trailer_len));
        let out_file = dir_info.get("OutFile");
        if out_file.truthy() {
            if et.get_or_new_hash("DEL_GROUP").get("Insta360").truthy() {
                pre_inc(&et, "CHANGED");
                return ret(n(1));
            // just copy the trailer when writing
            } else if pl::gt(&trailer_len, &trail_end) || !raf_seek(it, &raf, dir_info.get("DataPos"), 0)? || {
                let mut ob = out_file.deref();
                let r = raf_read(it, &raf, &mut ob, trailer_len.clone())?;
                pl::set_deref(&out_file, ob)?;
                !pl::num_eq(&r, &trailer_len)
            } {
                return ret(n(0));
            } else {
                return ret(n(1));
            }
        }
        if verbose.truthy() || et.get("HTML_DUMP").truthy() {
            m(it, &et, "DumpTrailer", vec![dir_info.clone()])?;
        }
    }
    if !opt(it, &et, "ExtractEmbedded")?.truthy() {
        // can arrive here when reading Insta360 trailer on JPEG image (INSP file)
        warn_lvl(it, &et, s("Use ExtractEmbedded option to extract timed metadata from Insta360 trailer"), 3)?;
        return ret(n(1));
    }

    let unknown = opt(it, &et, "Unknown")?;
    // position relative to end of trailer (avoids using large offsets for files > 2 GB)
    let mut epos = n(-78);
    let insv_data_len = lex_hash(it, "insvDataLen");
    let insv_limit = lex_hash(it, "insvLimit");
    et.set("SET_GROUP0", s("Trailer"));
    et.set("SET_GROUP1", s("Insta360"));
    set_byte_order(it, s("II"))?;
    // loop through all records in the trailer, from last to first
    loop {
        let u = pl::unpack("vV", &buff)?;
        let (id, mut len) = (arg(&u, 0), arg(&u, 1));
        epos = pl::sub(&epos, &len);
        if pl::lt(&pl::add(&epos, &trailer_len), &n(0)) {
            break;
        }
        if !raf_seek(it, &raf, pl::sub(&epos, &offset), 2)? {
            break;
        }
        if verbose.truthy() {
            let off = pl::add(&trail_end, &epos);
            m(it, &et, "VPrint", vec![n(0), pl::sprintf("Insta360 Record 0x%x (offset 0x%x, %d bytes):\n", &[id.clone(), off, len.clone()])])?;
        }
        // there are 2 types of record 0x300:
        // 1. 56 byte records
        // 0000: 4a f7 02 00 00 00 00 00 00 00 00 00 00 1e e7 3f [J..............?]
        // 0010: 00 00 00 00 00 b2 ef bf 00 00 00 00 00 70 c1 bf [.............p..]
        // 0020: 00 00 00 e0 91 5c 8c bf 00 00 00 20 8f ff 87 bf [.....\..... ....]
        // 0030: 00 00 00 00 88 7f c9 bf
        // 2. 20 byte records
        // 0000: c1 d8 d9 0b 00 00 00 00 f5 83 14 80 df 7f fe 7f [................]
        // 0010: fe 7f 01 80
        let idk = id.to_bytes();
        let mut dlen = insv_data_len.get(&idk);
        let idn = id.to_i64();
        if defined(&dlen) && !dlen.truthy() {
            if idn == 0x300 {
                let m20 = pl::modulo(&len, &n(20))?.truthy();
                let m56 = pl::modulo(&len, &n(56))?.truthy();
                if m20 && !m56 {
                    dlen = n(56);
                } else if m56 && !m20 {
                    dlen = n(20);
                } else {
                    if pl::num_eq(&raf_read(it, &raf, &mut buff, n(20))?, &n(20)) {
                        if sub_str(&buff, 16, 3).eq_str("\0\0\0") {
                            dlen = n(56);
                        } else {
                            dlen = n(20);
                        }
                    }
                    if !raf_seek(it, &raf, pl::sub(&epos, &offset), 2)? {
                        break;
                    }
                }
            } else if idn == 0x200 {
                dlen = len.clone();
            }
        }
        // limit the number of records we read if necessary
        let lim = insv_limit.get(&idk);
        if dlen.truthy() && lim.truthy() && pl::gt(&len, &pl::mul(&lim.at(1), &dlen)) {
            let msg = cat(&[&s("Insta360 "), &lim.at(0), &s(" data is huge. Processing only the first "), &lim.at(1), &s(" records")]);
            if warn_lvl(it, &et, msg, 2)?.truthy() {
                len = pl::mul(&lim.at(1), &dlen);
            }
        }
        if !pl::num_eq(&raf_read(it, &raf, &mut buff, len.clone())?, &len) {
            break;
        }
        if pl::gt(&verbose, &n(2)) {
            m(it, &et, "VerboseDump", vec![pl::sref(buff.clone())])?;
        }
        let bp = pl::sref(buff.clone());
        if dlen.truthy() {
            let dl = dlen.to_i64();
            let ln = len.to_i64();
            if pl::modulo(&len, &dlen)?.truthy() && idn != 0x700 {
                // (have seen one 0x700 record which was expected format but not multiple of 53 bytes)
                warn(it, &et, pl::sprintf("Unexpected Insta360 record 0x%x length", &[id.clone()]))?;
            } else if idn == 0x200 {
                // there are 4 types of record 0x200
                // 1. JPEG preview (starts with ff d8 ff e1)
                // 2. TIFF preview (starts with 01 00 00 00, then record length)
                // 3. Unknown 1 (starts with 00 00 00 01)
                // 4. Unknown 2 (starts with 00 00 01 34)
                if re(r"^\xff\xd8\xff", "").is_match(&buff) {
                    m(it, &et, "FoundTag", vec![s("PreviewImage"), buff.clone()])?;
                } else if let Some(c) = mt(r"^\x01\0\0\0(.{4})\x01", "s", &buff).filter(|c| pl::unpack1("V", &cap(c, 1)).map(|v| pl::num_eq(&v, &dlen)).unwrap_or(false)) {
                    let _ = c;
                    let wh = pl::unpack("x16V2", &buff)?;
                    // build the TIFF image (could the 1 at byte 9 be the SamplesPerPixel?)
                    let hdr = pl::call(it, "Image::ExifTool::MakeTiffHeader", vec![arg(&wh, 0), arg(&wh, 1), n(1), n(8)])?;
                    m(it, &et, "FoundTag", vec![s("PreviewTIFF"), cat(&[&hdr, &pl::substr(&buff, 40, None)])])?;
                }
            } else if idn == 0x300 {
                let mut p = 0i64;
                while p < ln {
                    new_doc(&et);
                    let mut av: Vec<Value> = Vec::new();
                    if dl == 56 {
                        for k in 1..=6 {
                            av.push(get_double(it, &bp, n(p + 8 * k))?);
                        }
                    } else {
                        av = pl::unpack(&format!("x{p}x8v6"), &buff)?;
                        for x in av.iter_mut() {
                            *x = pl::div(&pl::sub(x, &n(0x8000)), &n(1000))?;
                        }
                    }
                    let tc = get64u(it, &bp, n(p))?;
                    ht(it, &et, &tag_tbl, "TimeCode", pl::sprintf("%.3f", &[pl::div(&tc, &n(1000))?]))?;
                    let sl = |r: std::ops::Range<usize>| -> Vec<Value> { r.map(|k| av.get(k).cloned().unwrap_or_default()).collect() };
                    let (a1, a2) = (sl(0..3), sl(3..6));
                    ht(it, &et, &tag_tbl, "Accelerometer", join_sp(&a1))?; // (NC)
                    ht(it, &et, &tag_tbl, "AngularVelocity", join_sp(&a2))?; // (NC)
                    p += dl;
                }
            } else if idn == 0x400 {
                let mut p = 0i64;
                while p < ln {
                    new_doc(&et);
                    let tc = get64u(it, &bp, n(p))?;
                    ht(it, &et, &tag_tbl, "TimeCode", pl::sprintf("%.3f", &[pl::div(&tc, &n(1000))?]))?;
                    let v = get_double(it, &bp, n(p + 8))?;
                    ht(it, &et, &tag_tbl, "ExposureTime", v)?; //6
                    p += dl;
                }
            } else if idn == 0x600 {
                //6
                let mut p = 0i64;
                while p < ln {
                    new_doc(&et);
                    let tc = get64u(it, &bp, n(p))?;
                    ht(it, &et, &tag_tbl, "VideoTimeStamp", pl::sprintf("%.3f", &[pl::div(&tc, &n(1000))?]))?;
                    p += dl;
                }
            } else if idn == 0x700 {
                let mut p = 0i64;
                'recs: while p + dl <= ln {
                    'rec: {
                        let tmp = sub_str(&buff, p, dl);
                        let mut av = pl::unpack("VVvaa8aa8aa8a8a8", &tmp)?;
                        av.resize(11, Value::Undef);
                        if !((av[5].eq_str("N") || av[5].eq_str("S")) && // (quick validation)
                            (av[7].eq_str("E") || av[7].eq_str("W") ||
                             // (odd, but I've seen "O" instead of "W".  Perhaps
                             //  when the language is french? ie. "Ouest"?)
                             av[7].eq_str("O")))
                        {
                            if av[3].eq_str("V") {
                                break 'rec; // void fixes don't have N/S E/W
                            }
                            warn(it, &et, s("Unrecognized INSV GPS format"))?;
                            break 'recs;
                        }
                        if !av[3].eq_str("A") {
                            break 'rec; // (ignore void fixes)
                        }
                        new_doc(&et);
                        for k in [4usize, 6, 8, 9, 10] {
                            av[k] = get_double(it, &pl::sref(av[k].clone()), n(0))?;
                        }
                        if av[5].eq_str("S") {
                            av[4] = pl::neg(&pl::abs(&av[4])); // (abs just in case it was already signed)
                        }
                        if !av[7].eq_str("E") {
                            av[6] = pl::neg(&pl::abs(&av[6]));
                        }
                        let mut ms = s("");
                        if av[2].truthy() {
                            ms = pl::sprintf(".%.3d", &[av[2].clone()]);
                            re(r"0+$", "").subst(&mut ms, false, |_| Vec::new());
                        }
                        let d = convert_unix_time(it, vec![av[0].clone()])?;
                        ht(it, &et, &tag_tbl, "GPSDateTime", cat(&[&d, &ms, &s("Z")]))?;
                        ht(it, &et, &tag_tbl, "GPSLatitude", av[4].clone())?;
                        ht(it, &et, &tag_tbl, "GPSLongitude", av[6].clone())?;
                        ht(it, &et, &tag_tbl, "GPSSpeed", pl::mul(&av[8], &mps_to_kph()))?;
                        ht(it, &et, &tag_tbl, "GPSTrack", av[9].clone())?;
                        ht(it, &et, &tag_tbl, "GPSAltitude", av[10].clone())?;
                        if unknown.truthy() {
                            ht(it, &et, &tag_tbl, "Unknown02", av[1].clone())?;
                        }
                    }
                    p += dl;
                }
            }
        } else if idn == 0x101 {
            let tag_table_ptr = get_tag_table(it, "Image::ExifTool::QuickTime::INSV_MakerNotes")?;
            let (mut i, mut p) = (0, 0i64);
            while i < 4 {
                if pl::gt(&n(p + 2), &len) {
                    break;
                }
                let tn = pl::unpack(&format!("x{p}CC"), &buff)?;
                let (t, nn) = (arg(&tn, 0), arg(&tn, 1).to_i64());
                if pl::gt(&n(p + 2 + nn), &len) {
                    break;
                }
                let val = sub_str(&buff, p + 2, nn);
                m(it, &et, "HandleTag", vec![tag_table_ptr.clone(), t, val])?;
                p += 2 + nn;
                i += 1;
            }
        } else if idn == 0x0 {
            if !len.truthy() {
                break;
            }
            // example directory table for record locations from Insta360AcePro MP4 video:
            //  vv vv                         - record ID
            //        vv vv vv vv             - record size
            //                    vv vv vv vv - offset from start of footer
            //  00 00 00 00 00 00 00 00 00 00
            //  01 01 82 04 00 00 1b 45 62 00
            //  02 00 28 46 05 00 ed fe 5c 00
            //  03 00 40 aa 24 00 ed fe 34 00
            //  04 00 00 c1 01 00 ed fe 30 00
            //  [...]
            if !dir_table.truthy() {
                dir_table = buff.clone();
                dir_table_pos = 0;
            }
        }
        // step through directory table instead of sequential scanning if possible
        if dir_table.truthy() {
            let mut ep = Value::Undef;
            loop {
                if dir_table_pos + 10 > len_of(&dir_table) {
                    break;
                }
                let v = pl::unpack(&format!("x{dir_table_pos}vVV"), &dir_table)?;
                let (id2, siz, off) = (arg(&v, 0), arg(&v, 1), arg(&v, 2));
                dir_table_pos += 10;
                if id2.truthy() && siz.truthy() && pl::lt(&pl::add(&off, &siz), &trailer_len) {
                    ep = pl::sub(&pl::add(&off, &siz), &trailer_len);
                    break;
                }
            }
            if !defined(&ep) {
                break;
            }
            epos = ep;
        } else {
            epos = pl::sub(&epos, &n(6));
            if pl::lt(&pl::add(&epos, &trailer_len), &n(0)) {
                break; // step back to previous record
            }
        }
        if !raf_seek(it, &raf, pl::sub(&epos, &offset), 2)? {
            break; // seek to start of next footer
        }
        if !pl::num_eq(&raf_read(it, &raf, &mut buff, n(6))?, &n(6)) {
            break; // read footer
        }
    }
    et.delete("DOC_NUM");
    set_byte_order(it, s("MM"))?;
    et.delete("SET_GROUP0");
    et.delete("SET_GROUP1");
    ret(n(1))
}

/// Process CAMM metadata (ref PH)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_camm(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, _tag_tbl) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let mut pos = or(dir_info.get("DirStart"), n(0));
    let dl = dir_info.get("DirLen");
    let end = pl::add(
        &pos,
        &if dl.truthy() {
            dl
        } else {
            pl::at(3489);
            pl::sub(&n(len_of(&pl::sderef(&data_pt)?)), &pos)
        },
    );
    // camm record size for each type, including 4-byte header
    let size_of = |t: i64| -> i64 {
        match t {
            1 => 12,
            2 => 16,
            3 => 16,
            4 => 16,
            5 => 28,
            6 => 60,
            7 => 16,
            _ => 0,
        }
    };
    let mut rtn_val = n(0);
    while pl::lt(&pl::add(&pos, &n(4)), &end) {
        let typ = get16u(it, &data_pt, pl::add(&pos, &n(2)))?;
        let size = if pl::num_eq(&typ, &pl::int(&typ)) { size_of(typ.to_i64()) } else { 0 };
        if size == 0 {
            warn(it, &et, cat(&[&s("Unknown camm record type "), &typ]))?;
            break;
        }
        if pl::gt(&pl::add(&pos, &n(size)), &end) {
            warn(it, &et, cat(&[&s("Truncated camm record "), &typ]))?;
            break;
        }
        let tbl = pl::call(it, "Image::ExifTool::GetTagTable", vec![cat(&[&s("Image::ExifTool::QuickTime::camm"), &typ])])?;
        dir_info.set("DirStart", pos.clone());
        dir_info.set("DirLen", n(size));
        if m(it, &et, "ProcessBinaryData", vec![dir_info.clone(), tbl])?.truthy() {
            rtn_val = n(1);
        }
        // not sure if this is according to specification, but I have seen multiple
        // camm records all in a single sample, so step forward to process the next one
        pos = pl::add(&pos, &n(size));
    }
    ret(rtn_val)
}

/// Process Garmin GPS 'uuid' atom (ref PH)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
/// Note: This format is used by the Garmin DriveAssist 51, but the DriveAssist 50
///       uses a completely different format.  :(
pub fn process_garmin_gps(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_tbl) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    pl::at(3518);
    let data_len = len_of(&pl::sderef(&data_pt)?);
    let mut pos = 33i64;
    let epoch = n((66 * 365 + 17) * 24 * 3600); // time is relative to Jan 1, 1904
    let scl = pl::div(&n(180), &n(32768 * 65536))?; // scaling factor for lat/lon
    m(it, &et, "VerboseDir", vec![s("GarminGPS")])?;
    et.set("SET_GROUP1", s("Garmin"));
    while pos + 20 <= data_len {
        new_doc(&et);
        let t = get32u(it, &data_pt, n(pos))?;
        let time = cat(&[&convert_unix_time(it, vec![pl::sub(&t, &epoch)])?, &s("Z")]);
        let lat = get32s(it, &data_pt, n(pos + 12))?;
        let lon = get32s(it, &data_pt, n(pos + 16))?;
        let spd = get16u(it, &data_pt, n(pos + 4))?; // (in mph)
        ht(it, &et, &tag_tbl, "GPSDateTime", time)?;
        // skip bad GPS fixes
        if !pl::num_eq(&lat, &n(-2147483648)) || !pl::num_eq(&lon, &n(-2147483648)) {
            ht(it, &et, &tag_tbl, "GPSLatitude", pl::mul(&lat, &scl))?;
            ht(it, &et, &tag_tbl, "GPSLongitude", pl::mul(&lon, &scl))?;
            ht(it, &et, &tag_tbl, "GPSSpeed", spd)?;
            ht(it, &et, &tag_tbl, "GPSSpeedRef", s("M"))?;
        }
        pos += 20;
    }
    et.delete("DOC_NUM");
    et.delete("SET_GROUP1");
    ret(n(1))
}

/// Process 360Fly 'uuid' atom containing sensor data
/// (ref https://github.com/JamesHeinrich/getID3/blob/master/getid3/module.audio-video.quicktime.php)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_360fly(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_tbl) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    pl::at(3554);
    let data = pl::sderef(&data_pt)?;
    let data_len = len_of(&data);
    let mut pos = 16i64;
    let mut last_time = n(-1);
    let stream_tbl = get_tag_table(it, "Image::ExifTool::QuickTime::Stream")?;
    while pos + 32 <= data_len {
        let typ = n(data.to_bytes()[pos as usize] as i64);
        let time = get64u(it, &data_pt, n(pos + 2))?; // (only valid for some types)
        if tag_tbl.get(typ.to_bytes()).truthy() && !pl::num_eq(&time, &last_time) {
            new_doc(&et);
            last_time = time.clone();
        }
        ht_v(it, &et, &tag_tbl, typ.clone(), Value::Undef, vec![("DataPt", data_pt.clone()), ("Start", n(pos)), ("Size", n(32))])?;
        // synthesize GPSDateTime from the timestamp for GPS records
        if pl::num_eq(&typ, &n(5)) {
            set_gps_dt(it, &et, &stream_tbl, pl::div(&time, &f(1e6))?, Value::Undef)?;
        }
        pos += 32;
    }
    et.delete("DOC_NUM");
    ret(n(1))
}

/// Process GPS from Vantrue N2S dashcam
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_fmas(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_tbl) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    pl::at(3584);
    let data = pl::sderef(&data_pt)?;
    if !(re(r"^FMAS\0\0\0\0.{72}SAMM.{36}A", "s").is_match(&data) && len_of(&data) >= 160) {
        return ret(n(0));
    }
    m(it, &et, "VerboseDir", vec![s("FMAS"), Value::Undef, n(len_of(&data))])?;
    // 0000: 46 4d 41 53 00 00 00 00 00 00 00 00 00 00 00 00 [FMAS............]
    // 0010: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 [................]
    // 0020: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 [................]
    // 0030: 02 08 01 08 06 08 02 04 07 02 06 00 00 00 00 00 [................]
    // 0040: 00 00 00 00 00 00 00 00 4f 46 4e 49 4d 4d 41 53 [........OFNIMMAS]
    // 0050: 53 41 4d 4d 01 00 00 00 00 00 00 00 00 00 00 00 [SAMM............]
    // 0060: e5 07 09 18 08 00 22 00 02 00 00 00 a1 82 8a bf [......".........]
    // 0070: 89 23 8e bd 0b 2c 30 bc 41 57 4e 51 16 00 a1 01 [.#...,0.AWNQ....]
    // 0080: 29 26 27 0c 4b 00 49 00 00 00 00 00 00 00 00 00 [)&'.K.I.........]
    // 0090: 00 00 00 00 00 00 00 00 00 52 00 00 00 00 00 00 [.........R......]
    let mut av = pl::unpack("x96vCCCCCCx16AAACCCvCCvvv", &data)?;
    av.resize(19, Value::Undef);
    set_byte_order(it, s("II"))?;
    let accv = read_value(it, &data_pt, n(0x6c), s("float"), n(3), Value::Undef)?; // (looks like Z comes first in my sample)
    let lon = pl::add(&av[10], &pl::div(&pl::add(&av[11], &pl::div(&av[13], &n(6000))?), &n(60))?); // (why zero byte at $a[12]?)
    let lat = pl::add(&av[14], &pl::div(&pl::add(&av[15], &pl::div(&av[16], &n(6000))?), &n(60))?);
    ht(it, &et, &tag_tbl, "GPSDateTime", pl::sprintf("%.4d:%.2d:%.2d %.2d:%.2d:%.2d", &av[0..6]))?;
    ht(it, &et, &tag_tbl, "GPSLatitude", pl::mul(&lat, &n(if av[9].eq_str("S") { -1 } else { 1 })))?;
    ht(it, &et, &tag_tbl, "GPSLongitude", pl::mul(&lon, &n(if av[8].eq_str("W") { -1 } else { 1 })))?;
    ht(it, &et, &tag_tbl, "GPSSpeed", pl::mul(&av[17], &mph_to_kph()))?; // convert mph -> kph
    ht(it, &et, &tag_tbl, "GPSTrack", av[18].clone())?;
    ht(it, &et, &tag_tbl, "Accelerometer", accv)?;
    set_byte_order(it, s("MM"))?;
    ret(n(1))
}

/// Process GPS from Wolfbox G900 Dashcam and Redtiger F9 4K
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_wolfbox(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_tbl) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    pl::at(3619);
    let data = pl::sderef(&data_pt)?;
    if len_of(&data) < 0xf8 {
        return ret(n(0));
    }
    m(it, &et, "VerboseDir", vec![s("Wolfbox"), Value::Undef, n(len_of(&data))])?;
    // 0000: 65 00 00 00 00 00 00 00 31 01 01 00 e3 ff 00 00 [e.......1.......]
    // 0010: 04 00 00 00 10 00 00 00 2a 00 00 00 00 00 00 00 [........*.......]
    // 0020: 01 00 00 00 00 00 00 00 8b 33 ff 51 00 00 00 00 [.........3.Q....]
    // 0030: a0 86 01 00 00 00 00 00 4d 5e 07 fa ff ff ff ff [........M^......]
    // 0040: a0 86 01 00 00 00 00 00 00 00 00 00 00 00 00 00 [................]
    // 0050: 64 00 00 00 00 00 00 00 90 21 00 00 00 00 00 00 [d........!......]
    // 0060: 64 00 00 00 00 00 00 00 18 00 00 00 03 00 00 00 [d...............]
    // 0070: e8 07 00 00 00 00 00 00 00 00 00 00 00 00 00 00 [................]
    // 0080: 00 00 00 00 00 00 00 00 30 30 30 30 30 30 30 30 [........00000000]
    // 0090: 30 30 30 30 30 30 30 30 48 59 54 48 00 00 00 00 [00000000HYTH....]
    // 00a0: 0c 00 00 00 10 00 00 00 2a 00 00 00 00 00 00 00 [........*.......]
    // 00b0: 4f 3f 0c 1f 00 00 00 00 a0 86 01 00 00 00 00 00 [O?..............]
    // 00c0: 7f cf 2d ff ff ff ff ff a0 86 01 00 00 00 00 00 [..-.............]
    // 00d0: 01 00 00 00 08 00 00 00 0a 00 00 00 00 00 00 00 [................]
    // 00e0: 0a 00 00 00 00 00 00 00 e8 03 00 00 00 00 00 00 [................]
    // 00f0: 0a 00 00 00 00 00 00 00 4d 00 00 00 00 00 00 00 [........M.......]
    // lat/lon at 0xb0/0xc0 and 0x128/0x138
    // h/m/s at 0x10 and 0xa0 and 0x148 (the first imprinted on the video, and
    // the latter 2 presumed UTC, but there is a 1 second offset for the Redtiger)
    // spd at 0x48, dir at 0x58, alt at 0xe8
    // Redtiger F9 4K Dual Front and Rear Mini Dash Cam
    // 0000: 01 00 00 00 00 00 00 00 f4 ff 5d fe 24 00 00 00 [..........].$...]
    // 0010: 10 00 00 00 2d 00 00 00 25 00 00 00 00 00 00 00 [....-...%.......]
    // 0020: 01 00 00 00 00 00 00 00 44 eb 8f 00 00 00 00 00 [........D.......]
    // 0030: 10 27 00 00 00 00 00 00 1b 94 8a 04 00 00 00 00 [.'..............]
    // 0040: 10 27 00 00 00 00 00 00 8c 69 00 00 00 00 00 00 [.'.......i......]
    // 0050: e8 03 00 00 00 00 00 00 ba 47 00 00 00 00 00 00 [.........G......]
    // 0060: 64 00 00 00 00 00 00 00 19 00 00 00 05 00 00 00 [d...............]
    // 0070: e9 07 00 00 00 00 00 00 00 00 00 00 00 00 00 00 [................]
    // 0080: 00 00 00 00 00 00 00 00 30 30 30 30 30 30 30 30 [........00000000]
    // 0090: 30 30 30 30 30 30 30 30 58 58 58 58 00 00 00 00 [00000000XXXX....]
    // 00a0: 08 00 00 00 2d 00 00 00 24 00 00 00 00 00 00 00 [....-...$.......]
    // 00b0: 90 eb 8f 00 00 00 00 00 10 27 00 00 00 00 00 00 [.........'......]
    // 00c0: 20 94 8a 04 00 00 00 00 10 27 00 00 00 00 00 00 [ ........'......]
    // 00d0: 01 00 00 00 11 00 00 00 40 00 00 00 00 00 00 00 [........@.......]
    // 00e0: 64 00 00 00 00 00 00 00 8a 00 00 00 00 00 00 00 [d...............]
    // 00f0: 0a 00 00 00 00 00 00 00 4d 00 00 00 00 00 00 00 [........M.......]
    set_byte_order(it, s("II"))?;
    let v = pl::unpack("x104V3x44V3", &data)?;
    let (d, mo, yr, h, mi, sc) = (arg(&v, 0), arg(&v, 1), arg(&v, 2), arg(&v, 3), arg(&v, 4), arg(&v, 5));
    let time = pl::sprintf("%.4d:%.2d:%.2d %.2d:%.2d:%.2dZ", &[yr, mo, d, h, mi, sc]);
    let mut av: Vec<Value> = Vec::new();
    //             0=spd 1=dir 2=lat 3=lon 4=alt
    for pos in [0x48, 0x58, 0xb0, 0xc0, 0xe8] {
        let val = get64s(it, &data_pt, n(pos))?;
        let scl = get64s(it, &data_pt, n(pos + 8))?;
        av.push(pl::div(&val, &or(scl, n(1)))?);
    }
    let (mut lat, mut lon) = (av[2].clone(), av[3].clone());
    convert_lat_lon(&mut lat, &mut lon)?;
    av[2] = lat;
    av[3] = lon;
    ht(it, &et, &tag_tbl, "GPSDateTime", time)?;
    ht(it, &et, &tag_tbl, "GPSLatitude", av[2].clone())?;
    ht(it, &et, &tag_tbl, "GPSLongitude", av[3].clone())?;
    ht(it, &et, &tag_tbl, "GPSSpeed", pl::mul(&av[0], &knots_to_kph()))?;
    ht(it, &et, &tag_tbl, "GPSTrack", av[1].clone())?;
    ht(it, &et, &tag_tbl, "GPSAltitude", av[4].clone())?;
    ret(n(1))
}

/// Scan media data for "freeGPS" and GoPro metadata if not found already (ref PH)
/// Inputs: 0) ExifTool ref
pub fn scan_media_data(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let et = arg(&a, 0);
    scan_media_data_impl(it, &et)?;
    pl::ret_nothing(want)
}

fn scan_media_data_impl(it: &mut Interp, et: &Value) -> ER<()> {
    let raf = et.get("RAF");
    if !raf.truthy() {
        return Ok(());
    }
    let (mut tag_tbl, mut verbose, mut buff, mut found) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef);
    let mut data_len;

    // don't rescan for freeGPS if we already found embedded metadata
    let data_pos = et.get("MediaDataOffset");
    if et.get("FoundEmbedded").truthy() || !data_pos.truthy() {
        return Ok(());
    }

    let (mut pos, mut buf2) = (n(0), s(""));
    let ee = opt(it, et, "ExtractEmbedded")?;
    if pl::gt(&ee, &n(2)) {
        // scan entire file from start of mdat if ExtractEmbedded > 2
        raf_seek(it, &raf, n(0), 2)?;
        data_len = pl::sub(&raf_tell(it, &raf)?, &et.get("MediaDataOffset"));
    } else {
        data_len = et.get("MediaDataSize");
    }
    if !(data_len.truthy() && m(it, &raf, "Seek", vec![data_pos.clone()])?.truthy()) {
        return Ok(());
    }

    // loop through 'mdat' media data looking for GPS information
    while data_len.truthy() {
        let mut nn = n(GPS_BLOCK_SIZE);
        if pl::gt(&pl::add(&nn, &pos), &data_len) {
            nn = pl::sub(&data_len, &pos);
        }
        if !pl::gt(&nn, &n(len_of(&buf2))) {
            break;
        }
        if !raf_read(it, &raf, &mut buff, pl::sub(&nn, &n(len_of(&buf2))))?.truthy() {
            break;
        }
        if len_of(&buf2) > 0 {
            buff = cat(&[&buf2, &buff]);
        }
        // look for "freeGPS " or GoPro record
        // (freeGPS found on an absolute 0x8000-byte boundary in all of my samples,
        //  but allow for any alignment when searching)
        let mm = match_from(r"(\0..\0freeGPS |GP\x06\0\0)", "s", &buff, 0);
        let Some((c, buff_pos_end)) = mm else {
            buf2 = pl::substr(&buff, -12, None);
            pos = pl::add(&pos, &n(len_of(&buff) - 12));
            // in all of my samples the first freeGPS block is within 2 MB of the start
            // of the mdat, so limit the scan to the first 20 MB to be fast and safe
            if found.truthy() || pl::lt(&pos, &f(20e6)) || pl::gt(&ee, &n(1)) {
                continue;
            }
            break;
        };
        let c1 = cap(&c, 1);
        if c1.to_bytes()[..] == b"GP\x06\0\0"[..] {
            // (GoPro GPS record header)
            // (found in Chigee Aio-5 Lite and some Insta360 videos)
            let buff_pos = n(buff_pos_end as i64);
            let file_pos = raf_tell(it, &raf)?;
            let start = pl::sub(&pl::add(&pl::sub(&file_pos, &n(len_of(&buff))), &buff_pos), &n(len_of(&c1)));
            if !m(it, &raf, "Seek", vec![start.clone()])?.truthy() {
                break;
            }
            if !defined(&found) {
                m(it, et, "VPrint", vec![n(0), s("---- Extract Embedded ----\n")])?;
                et.set("INDENT", cat(&[&et.get("INDENT"), &s("| ")]));
                found = n(0);
            }
            let max_len = pl::sub(&data_len, &pl::sub(&start, &et.get("MediaDataOffset")));
            runtime::require(it, "Image::ExifTool::GoPro")?;
            m(it, et, "VPrint", vec![n(0), pl::sprintf("Unreferenced GoPro record at 0x%x\n", &[file_pos.clone()])])?;
            let size = pl::call(it, "Image::ExifTool::GoPro::ProcessGP6", vec![et.clone(), hash_of(vec![("RAF", raf.clone()), ("DirLen", max_len)])])?;
            if size.truthy() {
                if !found.truthy() {
                    // scan entire file if we found a valid GoPro record
                    // (some records may exist in trailer)
                    if raf_seek(it, &raf, n(0), 2)? {
                        data_len = pl::sub(&raf_tell(it, &raf)?, &et.get("MediaDataOffset"));
                    }
                    found = n(2);
                }
                if !m(it, &raf, "Seek", vec![pl::add(&start, &size)])?.truthy() {
                    break;
                }
                pos = pl::sub(&pl::add(&start, &size), &et.get("MediaDataOffset"));
                buf2 = s("");
            } else {
                // (could have been a random match -- continue with search)
                if !m(it, &raf, "Seek", vec![file_pos.clone()])?.truthy() {
                    break;
                }
                buf2 = pl::substr(&buff, buff_pos.to_i64(), None);
                pos = pl::add(&pos, &buff_pos);
            }
            continue;
        }
        if len_of(&buff) < GPS_BLOCK_SIZE {
            break;
        }
        if !tag_tbl.truthy() {
            // initialize variables for extracting metadata from this block
            tag_tbl = get_tag_table(it, "Image::ExifTool::QuickTime::Stream")?;
            verbose = et.get("OPTIONS").get("Verbose");
            m(it, et, "VPrint", vec![n(0), s("---- Extract Embedded ----\n")])?;
            et.set("INDENT", cat(&[&et.get("INDENT"), &s("| ")]));
            found = n(1);
        }
        let bpos = buff_pos_end as i64;
        if bpos > 12 {
            pos = pl::add(&pos, &n(bpos - 12));
            buff = pl::substr(&buff, bpos - 12, None);
        }
        // make sure we have the full freeGPS record
        let mut len = pl::unpack1("N", &buff)?;
        if pl::lt(&len, &n(12)) {
            len = n(12);
        } else {
            let more = pl::sub(&len, &n(len_of(&buff)));
            if pl::gt(&more, &n(0)) {
                if !pl::num_eq(&raf_read(it, &raf, &mut buf2, more.clone())?, &more) {
                    break;
                }
                buff = cat(&[&buff, &buf2]);
            }
            if verbose.truthy() {
                m(it, et, "VerboseDir", vec![s("GPS"), Value::Undef, len.clone()])?;
                m(it, et, "VerboseDump", vec![pl::sref(buff.clone()), s("DataPos"), pl::add(&pos, &data_pos)])?;
            }
            let bref = pl::sref(buff.clone());
            let di = hash_of(vec![("DataPt", bref.clone()), ("DataPos", pl::add(&pos, &data_pos)), ("DirLen", len.clone())]);
            process_free_gps(it, vec![et.clone(), di, tag_tbl.clone()], Want::Scalar)?;
            buff = bref.deref();
            et.set("FoundGPSByScan", n(1));
        }
        pos = pl::add(&pos, &len);
        buf2 = pl::substr(&buff, len.to_i64(), None);
    }
    if found.truthy() {
        et.delete("DOC_NUM"); // reset DOC_NUM after extracting embedded metadata
        m(it, et, "VPrint", vec![n(0), s("--------------------------\n")])?;
        et.set("INDENT", pl::substr(&et.get("INDENT"), 0, Some(-2)));
    }
    Ok(())
}

/// Register the ported subs under their Perl names.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::QuickTime::SignedInt32", signed_int32);
    it.define_native("Image::ExifTool::QuickTime::SaveMetaKeys", save_meta_keys);
    it.define_native("Image::ExifTool::QuickTime::FoundSomething", found_something);
    it.define_native("Image::ExifTool::QuickTime::SetGPSDateTime", set_gps_date_time);
    it.define_native("Image::ExifTool::QuickTime::HandleTextTags", handle_text_tags);
    it.define_native("Image::ExifTool::QuickTime::HandleNewTime", handle_new_time);
    it.define_native("Image::ExifTool::QuickTime::Process_text", process_text);
    it.define_native("Image::ExifTool::QuickTime::ProcessSamples", process_samples);
    it.define_native("Image::ExifTool::QuickTime::ConvertLatLon", convert_lat_lon_native);
    it.define_native("Image::ExifTool::QuickTime::DecryptLucky", decrypt_lucky);
    it.define_native("Image::ExifTool::QuickTime::ProcessFreeGPS", process_free_gps);
    it.define_native("Image::ExifTool::QuickTime::ParseTag", parse_tag);
    it.define_native("Image::ExifTool::QuickTime::Process_tx3g", process_tx3g);
    it.define_native("Image::ExifTool::QuickTime::Process_mebx", process_mebx);
    it.define_native("Image::ExifTool::QuickTime::Process_3gf", process_3gf);
    it.define_native("Image::ExifTool::QuickTime::Process_gps0", process_gps0);
    it.define_native("Image::ExifTool::QuickTime::Process_gsen", process_gsen);
    it.define_native("Image::ExifTool::QuickTime::Process_gdat", process_gdat);
    it.define_native("Image::ExifTool::QuickTime::Process_nbmt", process_nbmt);
    it.define_native("Image::ExifTool::QuickTime::ProcessKenwood", process_kenwood);
    it.define_native("Image::ExifTool::QuickTime::ProcessRIFFTrailer", process_riff_trailer);
    it.define_native("Image::ExifTool::QuickTime::ProcessKenwoodTrailer", process_kenwood_trailer);
    it.define_native("Image::ExifTool::QuickTime::ProcessNMEA", process_nmea);
    it.define_native("Image::ExifTool::QuickTime::ProcessGPSLog", process_gps_log);
    it.define_native("Image::ExifTool::QuickTime::ProcessTTAD", process_ttad);
    it.define_native("Image::ExifTool::QuickTime::ProcessInsta360", process_insta360);
    it.define_native("Image::ExifTool::QuickTime::ProcessCAMM", process_camm);
    it.define_native("Image::ExifTool::QuickTime::ProcessGarminGPS", process_garmin_gps);
    it.define_native("Image::ExifTool::QuickTime::Process360Fly", process_360fly);
    it.define_native("Image::ExifTool::QuickTime::ProcessFMAS", process_fmas);
    it.define_native("Image::ExifTool::QuickTime::ProcessWolfbox", process_wolfbox);
    it.define_native("Image::ExifTool::QuickTime::ScanMediaData", scan_media_data);
}
