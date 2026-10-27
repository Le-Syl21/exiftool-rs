//! Time::Piece, the part ExifTool uses: Time::Piece->strptime (its XS
//! _strptime, FreeBSD's strptime with Time::Piece's changes, and its
//! my_mini_mktime normalisation) and the accessors InverseDateTime calls.
//! Names of days and months are the C locale's, which use_locale() gives
//! in the "C" locale ExifTool runs in.

use std::rc::Rc;

use crate::interp::eval::die;
use crate::interp::Interp;
use crate::perl::Value;
use crate::pl::{arg, ret};

const WEEKDAY: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const WDAY: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTH: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const MON: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

#[derive(Clone, Copy)]
struct Tm {
    sec: i64,
    min: i64,
    hour: i64,
    mday: i64,
    mon: i64,
    year: i64,
    wday: i64,
    yday: i64,
    isdst: i64,
}

fn starts_ci(buf: &[u8], s: &str) -> bool {
    buf.len() >= s.len() && buf[..s.len()].eq_ignore_ascii_case(s.as_bytes())
}

/// Up to `len` digits (the first must be one).
fn digits(buf: &[u8], b: &mut usize, len: usize) -> Option<i64> {
    if !buf.get(*b).is_some_and(|c| c.is_ascii_digit()) {
        return None;
    }
    let mut i = 0i64;
    let mut n = len;
    while n > 0 && buf.get(*b).is_some_and(|c| c.is_ascii_digit()) {
        i = i * 10 + (buf[*b] - b'0') as i64;
        *b += 1;
        n -= 1;
    }
    Some(i)
}

/// After a number followed by white space, the format skips to its own
/// next white space (FreeBSD's quirk).
fn skip_fmt_word(buf: &[u8], b: usize, fmt: &[u8], p: &mut usize) {
    if buf.get(b).is_some_and(|c| c.is_ascii_whitespace()) {
        while *p < fmt.len() && !fmt[*p].is_ascii_whitespace() {
            *p += 1;
        }
    }
}

/// _strptime: the position after what was parsed, or None on a mismatch.
fn strptime(buf: &[u8], mut b: usize, fmt: &[u8], tm: &mut Tm, got_gmt: &mut bool) -> Option<usize> {
    let mut p = 0;
    while p < fmt.len() {
        if b >= buf.len() {
            break;
        }
        let c = fmt[p];
        p += 1;
        if c != b'%' {
            if c.is_ascii_whitespace() {
                while b < buf.len() && buf[b].is_ascii_whitespace() {
                    b += 1;
                }
            } else {
                if buf[b] != c {
                    return None;
                }
                b += 1;
            }
            continue;
        }
        let (mut e_alt, mut o_alt) = (false, false);
        loop {
            let c = fmt.get(p).copied().unwrap_or(0);
            p += 1;
            match c {
                0 | b'%' => {
                    if buf.get(b) != Some(&b'%') {
                        return None;
                    }
                    b += 1;
                }
                b'+' => b = strptime(buf, b, b"%c", tm, got_gmt)?,
                b'C' => {
                    let i = digits(buf, &mut b, 2)?;
                    if i < 19 {
                        return None;
                    }
                    tm.year = i * 100 - 1900;
                }
                b'c' => b = strptime(buf, b, b"%a %d %b %Y %I:%M:%S %p %Z", tm, got_gmt)?,
                b'D' => b = strptime(buf, b, b"%m/%d/%y", tm, got_gmt)?,
                b'E' | b'O' => {
                    if !(e_alt || o_alt) {
                        if c == b'E' {
                            e_alt = true;
                        } else {
                            o_alt = true;
                        }
                        continue;
                    }
                }
                b'F' => b = strptime(buf, b, b"%Y-%m-%d", tm, got_gmt)?,
                b'R' => b = strptime(buf, b, b"%H:%M", tm, got_gmt)?,
                b'r' | b'X' => b = strptime(buf, b, b"%I:%M:%S %p", tm, got_gmt)?,
                b'n' | b't' => {
                    if !buf.get(b).is_some_and(|c| c.is_ascii_whitespace()) {
                        return None;
                    }
                    while buf.get(b).is_some_and(|c| c.is_ascii_whitespace()) {
                        b += 1;
                    }
                }
                b'T' => b = strptime(buf, b, b"%H:%M:%S", tm, got_gmt)?,
                b'x' => b = strptime(buf, b, b"%a %d %b %Y", tm, got_gmt)?,
                b'j' => {
                    let i = digits(buf, &mut b, 3)?;
                    if !(1..=366).contains(&i) {
                        return None;
                    }
                    tm.yday = i - 1;
                    tm.mday = 0;
                }
                b'M' | b'S' => {
                    if b >= buf.len() || buf[b].is_ascii_whitespace() {
                        break;
                    }
                    let i = digits(buf, &mut b, 2)?;
                    if c == b'M' {
                        if i > 59 {
                            return None;
                        }
                        tm.min = i;
                    } else {
                        if i > 60 {
                            return None;
                        }
                        tm.sec = i;
                    }
                    skip_fmt_word(buf, b, fmt, &mut p);
                }
                b'H' | b'I' | b'k' | b'l' => {
                    let i = digits(buf, &mut b, 2)?;
                    if c == b'H' || c == b'k' {
                        if i > 23 {
                            return None;
                        }
                    } else if i > 12 {
                        return None;
                    }
                    tm.hour = i;
                    skip_fmt_word(buf, b, fmt, &mut p);
                }
                b'p' | b'P' => {
                    let rest = &buf[b..];
                    if starts_ci(rest, "AM") {
                        if tm.hour > 12 {
                            return None;
                        }
                        if tm.hour == 12 {
                            tm.hour = 0;
                        }
                        b += 2;
                    } else if starts_ci(rest, "PM") {
                        if tm.hour > 12 {
                            return None;
                        }
                        if tm.hour != 12 {
                            tm.hour += 12;
                        }
                        b += 2;
                    } else {
                        return None;
                    }
                }
                b'A' | b'a' => {
                    let names = if c == b'A' { &WEEKDAY } else { &WDAY };
                    let i = names.iter().position(|n| starts_ci(&buf[b..], n))?;
                    tm.wday = i as i64;
                    b += names[i].len();
                }
                b'U' | b'V' | b'W' => {
                    let i = digits(buf, &mut b, 2)?;
                    if i > 53 {
                        return None;
                    }
                    skip_fmt_word(buf, b, fmt, &mut p);
                }
                b'u' | b'w' => {
                    if !buf.get(b).is_some_and(|c| c.is_ascii_digit()) {
                        return None;
                    }
                    let mut i = (buf[b] - b'0') as i64;
                    if i > 6 + (c == b'u') as i64 {
                        return None;
                    }
                    if i == 7 {
                        i = 0;
                    }
                    tm.wday = i;
                    b += 1;
                    skip_fmt_word(buf, b, fmt, &mut p);
                }
                b'd' | b'e' => {
                    let i = digits(buf, &mut b, 2)?;
                    if i > 31 {
                        return None;
                    }
                    tm.mday = i;
                    skip_fmt_word(buf, b, fmt, &mut p);
                }
                b'B' | b'b' | b'h' => {
                    // (no alternative month names in the C locale)
                    let names = if c == b'B' { &MONTH } else { &MON };
                    let i = if o_alt && c != b'B' { None } else { names.iter().position(|n| starts_ci(&buf[b..], n)) }?;
                    tm.mon = i as i64;
                    b += names[i].len();
                }
                b'm' => {
                    let i = digits(buf, &mut b, 2)?;
                    if !(1..=12).contains(&i) {
                        return None;
                    }
                    tm.mon = i - 1;
                    skip_fmt_word(buf, b, fmt, &mut p);
                }
                b's' => {
                    // strtol: optional white space and sign, then digits
                    let mut q = b;
                    while buf.get(q).is_some_and(|c| c.is_ascii_whitespace()) {
                        q += 1;
                    }
                    let neg = buf.get(q) == Some(&b'-');
                    if matches!(buf.get(q), Some(b'-') | Some(b'+')) {
                        q += 1;
                    }
                    let start = q;
                    let mut n: i64 = 0;
                    while buf.get(q).is_some_and(|c| c.is_ascii_digit()) {
                        n = n.checked_mul(10)?.checked_add((buf[q] - b'0') as i64)?;
                        q += 1;
                    }
                    if q == start {
                        q = b; // no digits: strtol leaves the pointer
                    }
                    b = q;
                    let t = if neg { -n } else { n };
                    let bd = crate::interp::time::broken_down(t, *got_gmt);
                    *tm = Tm { sec: bd[0], min: bd[1], hour: bd[2], mday: bd[3], mon: bd[4], year: bd[5], wday: bd[6], yday: bd[7], isdst: bd[8] };
                }
                b'Y' | b'y' => {
                    if b >= buf.len() || buf[b].is_ascii_whitespace() {
                        break;
                    }
                    let mut i = digits(buf, &mut b, if c == b'Y' { 4 } else { 2 })?;
                    if c == b'Y' {
                        i -= 1900;
                    }
                    if c == b'y' && i < 69 {
                        i += 100;
                    }
                    if i < 0 {
                        return None;
                    }
                    tm.year = i;
                    skip_fmt_word(buf, b, fmt, &mut p);
                }
                b'Z' => {
                    let n = buf[b..].iter().take_while(|c| c.is_ascii_uppercase()).count();
                    if n > 0 {
                        if &buf[b..b + n] == b"GMT" {
                            *got_gmt = true;
                        }
                        if !*got_gmt {
                            return None;
                        }
                        b += n;
                    }
                }
                b'z' => {
                    let sign = match buf.get(b) {
                        Some(b'+') => 1,
                        Some(b'-') => -1,
                        _ => return None,
                    };
                    b += 1;
                    let mut i = 0;
                    for _ in 0..4 {
                        if !buf.get(b).is_some_and(|c| c.is_ascii_digit()) {
                            return None;
                        }
                        i = i * 10 + (buf[b] - b'0') as i64;
                        b += 1;
                    }
                    tm.hour -= sign * (i / 100);
                    tm.min -= sign * (i % 100);
                    *got_gmt = true;
                }
                _ => {}
            }
            break;
        }
    }
    Some(b)
}

/// my_mini_mktime: carry the fields over into a valid date and time (a
/// day of the year with no month and day sets the date), seconds up to 60
/// kept as a leap second.
fn mini_mktime(tm: &mut Tm) {
    let (mut month, mut mday, mut year) = (tm.mon, tm.mday, 1900 + tm.year);
    let mut jday = 0;
    if tm.yday >= 0 && mday <= 0 && month <= 0 {
        month = 0;
        mday = 0;
        jday = 1 + tm.yday;
    }
    let _ = &mut year;
    let mut secs = if (0..=60).contains(&tm.sec) {
        0
    } else {
        let s = tm.sec;
        tm.sec = 0;
        s
    };
    secs += 60 * tm.min + 3600 * tm.hour;
    // days from (year, month, mday + jday), month counted from 0
    let months = year * 12 + month;
    let (y, m) = (months.div_euclid(12), months.rem_euclid(12));
    let mut days = crate::interp::time::days_from_civil(y, m + 1, 1) + mday - 1 + jday;
    if mday <= 0 && jday == 0 {
        // mday 0 is the last day of the previous month, as the arithmetic gives
    }
    days += secs.div_euclid(86400);
    let secs = secs.rem_euclid(86400);
    tm.hour = secs / 3600;
    tm.min = (secs % 3600) / 60;
    tm.sec += secs % 60;
    let bd = crate::interp::time::broken_down(days * 86400, false);
    tm.mday = bd[3];
    tm.mon = bd[4];
    tm.year = bd[5];
    tm.wday = bd[6];
    tm.yday = bd[7];
}

/// The object: a hash blessed into Time::Piece holding perl's 11 fields
/// (the port blesses hashes only).
fn piece(tm: &Tm) -> Value {
    let fields = [tm.sec, tm.min, tm.hour, tm.mday, tm.mon, tm.year, tm.wday, tm.yday, tm.isdst.max(0), 0, 0];
    let h = Value::new_hash();
    if let Value::HashRef(r) = &h {
        r.borrow_mut().blessed = Some(Rc::from("Time::Piece"));
    }
    h.set("tm", Value::new_array(fields.iter().map(|&x| Value::Int(x)).collect()));
    h
}

fn field(a: &[Value], i: usize) -> Value {
    arg(a, 0).get("tm").at(i as i64)
}

pub fn register(it: &mut Interp) {
    it.set_scalar("Time::Piece::VERSION", Value::str("1.3401_01"));
    it.define_native("Time::Piece::strptime", |it, a, _| {
        let string = arg(&a, 1).to_bytes();
        let fmt = if a.len() > 2 { arg(&a, 2).to_bytes().to_vec() } else { b"%a, %d %b %Y %H:%M:%S %Z".to_vec() };
        let mut tm = Tm { sec: 0, min: 0, hour: 0, mday: 1, mon: 0, year: 70, wday: 4, yday: 0, isdst: -1 };
        let mut got_gmt = false;
        let Some(end) = strptime(&string, 0, &fmt, &mut tm, &mut got_gmt) else {
            return die("Error parsing time");
        };
        if end < string.len() {
            let rest = String::from_utf8_lossy(&string[end..]).into_owned();
            crate::pl::warn(it, &format!("Garbage at end of string in strptime: {rest}"))?;
            crate::pl::warn(it, "Perhaps a format flag did not match the actual input?")?;
        }
        mini_mktime(&mut tm);
        // _mktime takes the epoch from Time::Local::timegm, whose range
        // check refuses the leap second mini_mktime keeps
        if tm.sec == 60 {
            return die("Second '60' out of range 0..59");
        }
        ret(piece(&tm))
    });
    it.define_native("Time::Piece::use_locale", |_, _, _| Ok(vec![]));
    for (name, i) in [("sec", 0), ("second", 0), ("min", 1), ("minute", 1), ("hour", 2), ("mday", 3), ("day_of_month", 3), ("_mon", 4), ("_year", 5), ("_wday", 6), ("yday", 7), ("day_of_year", 7), ("isdst", 8), ("daylight_savings", 8)] {
        it.define_native(&format!("Time::Piece::{name}"), move |_, a, _| ret(field(&a, i)));
    }
    it.define_native("Time::Piece::mon", |_, a, _| ret(Value::Int(field(&a, 4).to_i64() + 1)));
    it.define_native("Time::Piece::year", |_, a, _| ret(Value::Int(field(&a, 5).to_i64() + 1900)));
    it.define_native("Time::Piece::epoch", |_, a, _| {
        let t = |i| field(&a, i).to_i64();
        let days = crate::interp::time::days_from_civil(t(5) + 1900, t(4) + 1, t(3));
        ret(Value::Int(days * 86400 + t(2) * 3600 + t(1) * 60 + t(0)))
    });
}
