//! Image::ExifTool::GPS (GPS.pm): the conversions of GPS coordinates and
//! time stamps. The tag tables are data; these are the module's four subs.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::perl::Value;
use crate::pl::{self, arg, cap, re, ret};

fn n(i: i64) -> Value {
    Value::Int(i)
}

/// Convert GPS timestamp value
/// Inputs: 0) raw timestamp value string
/// Returns: EXIF-formatted time string
pub fn convert_time_stamp(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let val = arg(&a, 0);
    let p = pl::split_ws(&val);
    let (h, m, s) = (arg(&p, 0), arg(&p, 1), arg(&p, 2));
    let or0 = |v: &Value| if v.truthy() { v.clone() } else { n(0) };
    // (perl's warnings for non-numeric parts, in its order of evaluation)
    pl::at(463);
    pl::warn_num(it, &or0(&h), "", "multiplication (*)")?;
    pl::warn_num(it, &or0(&m), "", "addition (+)")?;
    pl::warn_num(it, &or0(&s), "", "addition (+)")?;
    let mut f = pl::add(&pl::mul(&pl::add(&pl::mul(&or0(&h), &n(60)), &or0(&m)), &n(60)), &or0(&s));
    let mut h = pl::int(&pl::div(&f, &n(3600))?);
    f = pl::sub(&f, &pl::mul(&h, &n(3600)));
    let mut m = pl::int(&pl::div(&f, &n(60))?);
    f = pl::sub(&f, &pl::mul(&m, &n(60)));
    let mut ss = pl::sprintf("%012.9f", &[f]);
    if pl::ge(&ss, &n(60)) {
        ss = pl::s("00");
        // ++$m >= 60 and $m -= 60, ++$h;
        m = pl::add(&m, &n(1));
        if pl::ge(&m, &n(60)) {
            m = pl::sub(&m, &n(60));
            h = pl::add(&h, &n(1));
        }
    } else {
        re(r"\.?0+$", "").subst(&mut ss, false, |_| Vec::new()); // trim trailing zeros + decimal
    }
    ret(pl::sprintf("%.2d:%.2d:%s", &[h, m, ss]))
}

/// Print GPS timestamp
/// Inputs: 0) EXIF-formatted time string
/// Returns: time rounded to the nearest microsecond
pub fn print_time_stamp(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut val = arg(&a, 0);
    let mut sec = Value::Undef;
    if re(r":(\d{2}\.\d+)$", "").subst(&mut val, false, |c| {
        sec = cap(c, 1);
        Vec::new()
    }) == 0
    {
        return ret(val);
    }
    let mut s = pl::div(&pl::int(&pl::add(&pl::mul(&sec, &n(1000000)), &Value::Num(0.5))), &n(1000000))?;
    if pl::lt(&s, &n(10)) {
        s = pl::concat(&pl::s("0"), &s);
    }
    ret(pl::concat(&pl::concat(&val, &pl::s(":")), &s))
}

/// Convert degrees to DMS, or whatever the current settings are
/// Inputs: 0) ExifTool reference, 1) Value in degrees,
///         2) format code (0=no format, 1=CoordFormat, 2=XMP format, 3=signed unformatted)
///         3) 'N' or 'E' if sign is significant and N/S/E/W should be added
/// Returns: DMS string
pub fn to_dms(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, mut val, mut do_print_conv, mut r) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    let mut fmt = Value::Undef;
    let mut fmts: Vec<Value> = Vec::new();
    let (mut sign, mut minus, mut neg) = (Value::Undef, Value::Undef, false);

    if pl::length(&val).to_i64() == 0 {
        // don't convert an empty value
        if do_print_conv.truthy() && pl::str_eq(&do_print_conv, "1") {
            return ret(val); // avoid hiding existing tag when extracting
        }
        return pl::ret_undef(); // avoid writing empty value
    }
    if r.truthy() {
        if pl::lt(&val, &n(0)) {
            val = pl::neg(&val);
            r = match &*r.to_bytes() {
                b"N" => pl::s("S"),
                b"E" => pl::s("W"),
                _ => Value::Undef,
            };
            sign = pl::s("-");
            minus = pl::s("-");
        } else {
            sign = pl::s("+");
            minus = pl::s("");
        }
        if !(do_print_conv.truthy() && pl::str_eq(&do_print_conv, "2")) {
            r = pl::concat(&pl::s(" "), &r);
        }
    } else {
        if do_print_conv.truthy() && pl::str_eq(&do_print_conv, "3") {
            neg = pl::lt(&val, &n(0));
            do_print_conv = n(0);
        }
        val = pl::abs(&val);
        r = pl::s("");
    }
    let num;
    if do_print_conv.truthy() {
        if pl::str_eq(&do_print_conv, "1") {
            fmt = pl::method(it, &et, "Options", vec![pl::s("CoordFormat")])?;
            if !fmt.truthy() {
                fmt = pl::concat(&pl::s("%d deg %d' %.2f\""), &r);
            } else if r.truthy() {
                // use signed value instead of reference direction if specified
                let (sg, mn) = (sign.to_bytes(), minus.to_bytes());
                let done = re(r"%\+", "").subst(&mut fmt, true, |_| [&sg[..], b"%"].concat()) > 0
                    || re(r"%-", "").subst(&mut fmt, true, |_| [&mn[..], b"%"].concat()) > 0;
                if !done {
                    fmt = pl::concat(&fmt, &r);
                }
            } else {
                re(r"%\+", "").subst(&mut fmt, true, |_| b"%".to_vec()); // don't know sign, so don't print it
            }
        } else {
            fmt = pl::concat(&pl::s("%d,%.8f"), &r); // use XMP format with 8 decimal minutes
        }
        // count (and capture) the format specifiers (max 3)
        for c in re(r"(%(%|[^%]*?[diouxXDOUeEfFgGcs]))", "").each(&fmt) {
            if pl::str_eq(&cap(&c, 1), "%%") {
                continue;
            }
            fmts.push(cap(&c, 1));
            if fmts.len() >= 3 {
                break;
            }
        }
        num = fmts.len();
    } else {
        num = 3;
    }
    let mut c: Vec<Value> = vec![val.clone()]; // coordinates (D) or (D,M) or (D,M,S)
    if num > 1 {
        c[0] = pl::int(&c[0]);
        c.push(pl::mul(&pl::sub(&val, &c[0]), &n(60)));
        if num > 2 {
            c[1] = pl::int(&c[1]);
            let t = pl::div(&c[1], &n(60))?;
            c.push(pl::mul(&pl::sub(&pl::sub(&val, &c[0]), &t), &n(3600)));
        }
        // handle round-off errors to ensure minutes and seconds are
        // less than 60 (eg. convert "72 59 60.00" to "73 0 0.00")
        let last = c.len() - 1;
        c[last] = if do_print_conv.truthy() { pl::sprintf_v(fmts.last().unwrap_or(&Value::Undef), &[c[last].clone()]) } else { pl::concat(&c[last], &pl::s("")) };
        if pl::ge(&c[last], &n(60)) {
            c[last] = pl::sub(&c[last], &n(60));
            // ($c[-2] += 1) >= 60 and $num > 2 and $c[-2] -= 60, $c[-3] += 1;
            c[last - 1] = pl::add(&c[last - 1], &n(1));
            if pl::ge(&c[last - 1], &n(60)) && num > 2 {
                c[last - 1] = pl::sub(&c[last - 1], &n(60));
                c[last - 2] = pl::add(&c[last - 2], &n(1));
            }
        }
    }
    let mut rtn;
    if do_print_conv.truthy() {
        rtn = pl::sprintf_v(&fmt, &c);
        // trim trailing zeros in XMP
        if pl::str_eq(&do_print_conv, "2") {
            let mut p = br"(\d)0+".to_vec();
            p.extend_from_slice(&r.to_bytes());
            p.push(b'$');
            let rb = r.to_bytes();
            pl::re_dyn(&p, "")?.subst(&mut rtn, false, |m| [&cap(m, 1).to_bytes()[..], &rb[..]].concat());
        }
    } else {
        if neg {
            for x in c.iter_mut() {
                *x = pl::mul(x, &n(-1));
            }
        }
        let joined: Vec<Vec<u8>> = c.iter().map(|x| x.to_bytes().to_vec()).collect();
        rtn = pl::concat(&Value::bytes(joined.join(&b' ')), &r);
    }
    ret(rtn)
}

/// Convert to decimal degrees
/// Inputs: 0) a string containing 1-3 decimal numbers and any amount of other garbage
///         1) true if value should be negative if coordinate ends in 'S' or 'W',
///         2) 'lat' or 'lon' to extract lat or lon from GPSCoordinates string
/// Returns: Coordinate in degrees, or '' on error
pub fn to_degrees(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (mut val, do_sign, coord) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if re(r"\b(inf|undef)\b", "").is_match(&val) {
        return ret(pl::s("")); // ignore invalid values
    }
    // use only lat or lon part of combined GPSCoordinates inputs
    if coord.truthy() && (pl::str_eq(&coord, "lat") || pl::str_eq(&coord, "lon")) {
        // (two formatted coordinate values with cardinal directions, separated by a comma)
        if let Some(m) = re(r"^(.*(?:N(?:orth)?|S(?:outh)?)),\s*(.*(?:E(?:ast)?|W(?:est)?))$", "i").m(&val) {
            val = if pl::str_eq(&coord, "lat") { cap(&m, 1) } else { cap(&m, 2) };
        }
    }
    // extract decimal or floating point values out of any other garbage
    let v = re(r"((?:[+-]?)(?=\d|\.\d)\d*(?:\.\d*)?(?:[Ee][+-]\d+)?)", "").all(&val);
    let (d, m, s) = (arg(&v, 0), arg(&v, 1), arg(&v, 2));
    if d.is_undef() {
        return ret(pl::s(""));
    }
    let or0 = |x: &Value| if x.truthy() { x.clone() } else { n(0) };
    let mut deg = pl::add(&d, &pl::div(&pl::add(&or0(&m), &pl::div(&or0(&s), &n(60))?), &n(60))?);
    // make negative if S or W coordinate
    let negative = if do_sign.truthy() { re(r"[^A-Z](S(outh)?|W(est)?)\s*$", "i").is_match(&val) } else { pl::lt(&deg, &n(0)) };
    if negative {
        deg = pl::neg(&deg);
    }
    ret(deg)
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::GPS::ConvertTimeStamp", convert_time_stamp);
    it.define_native("Image::ExifTool::GPS::PrintTimeStamp", print_time_stamp);
    it.define_native("Image::ExifTool::GPS::ToDMS", to_dms);
    it.define_native("Image::ExifTool::GPS::ToDegrees", to_degrees);
}
