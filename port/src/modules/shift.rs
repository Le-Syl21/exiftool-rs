//! Image::ExifTool::Shift (Image/ExifTool/Shift.pl), ported sub by sub (see docs/port/PORTING.md).
//!
//! ExifTool time shifting routines (package Image::ExifTool).

use std::rc::Rc;

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::perl::regex::{self as pre, PerlRegex};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, re, ret};

fn n(i: i64) -> Value {
    Value::Int(i)
}

// ---- local helpers (also used by the Geotag and Geolocation ports) ----

/// `$x || 0`
pub(crate) fn or0(v: &Value) -> Value {
    if v.truthy() {
        v.clone()
    } else {
        n(0)
    }
}

/// `$a <=> $b` as a number (undef for NaN).
pub(crate) fn spaceship(a: &Value, b: &Value) -> Value {
    match ops::num_cmp(a, b) {
        Some(std::cmp::Ordering::Less) => n(-1),
        Some(std::cmp::Ordering::Equal) => n(0),
        Some(std::cmp::Ordering::Greater) => n(1),
        None => Value::Undef,
    }
}

/// `$a ** $b`
pub(crate) fn pow(a: &Value, b: &Value) -> Value {
    ops::pow(a, b)
}

/// `defined $a[$i]` on a Vec used as a perl array.
pub(crate) fn defined_at(v: &[Value], i: usize) -> bool {
    v.get(i).is_some_and(|x| !x.is_undef())
}

/// `$a[$i]` on a Vec used as a perl array (undef past the end).
pub(crate) fn at(v: &[Value], i: usize) -> Value {
    v.get(i).cloned().unwrap_or_default()
}

/// `$a[$i] = $x` on a Vec used as a perl array (extending it).
pub(crate) fn put_at(v: &mut Vec<Value>, i: usize, x: Value) {
    if i >= v.len() {
        v.resize(i + 1, Value::Undef);
    }
    v[i] = x;
}

/// `shift @a` on a Vec.
pub(crate) fn vshift(v: &mut Vec<Value>) -> Value {
    if v.is_empty() {
        Value::Undef
    } else {
        v.remove(0)
    }
}

/// `@$ref = LIST` on an array reference (in place).
pub(crate) fn set_items(r: &Value, items: Vec<Value>) {
    if let Value::ArrayRef(a) = r {
        *a.borrow_mut() = items;
    }
}

/// A compiled pattern for matches that keep `pos()` (scalar `m//g`).
pub(crate) fn rx(pat: &str, flags: &str) -> Rc<PerlRegex> {
    pre::compile(pat.as_bytes(), flags).unwrap_or_else(|e| panic!("bad pattern /{pat}/: {e}"))
}

/// `$s =~ /re/g` in scalar context from `pos`: the captures and the new
/// pos, or None (perl then resets pos).
pub(crate) fn match_g(r: &PerlRegex, subject: &Value, pos: usize) -> Option<(pl::Caps, usize)> {
    let b = subject.to_bytes();
    match r.match_at(&b, pos) {
        Ok(Some(sp)) => {
            let end = sp[0].unwrap().1;
            Some((sp.iter().map(|g| g.map(|(s, e)| Value::bytes(&b[s..e]))).collect(), end))
        }
        _ => None,
    }
}

/// `gmtime($t)` / `localtime($t)` in list context, as the builtin.
pub(crate) fn time_list(it: &mut Interp, t: &Value, local: bool) -> Vec<Value> {
    if local {
        crate::interp::time::sync_tz(it);
    }
    let tm = crate::interp::time::broken_down(t.to_f64().floor() as i64, local);
    tm.iter().map(|&x| Value::Int(x)).collect()
}

/// `localtime` (now) in list context.
pub(crate) fn localtime_now(it: &mut Interp) -> Vec<Value> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
    time_list(it, &Value::Num(now), true)
}

/// `sprintf` with a format built at run time.
fn sprintf_s(fmt: &[u8], args: &[Value]) -> Value {
    Value::bytes(crate::perl::sprintf::sprintf(fmt, args))
}

// ---- the subs ----

/// apply shift to value in new value hash
/// Inputs: 0) ExifTool ref, 1) shift type, 2) shift string, 3) raw date/time value,
///         4) new value hash ref
/// Returns: error string or undef on success and updates value in new value hash
pub fn apply_shift(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, func, mut shift, mut val, nv_hash) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4));

    // get shift direction from first character in shift string
    let mut c1 = Value::Undef;
    let pre_ = if re(r"^(\+|-)", "").subst(&mut shift, false, |c| {
        c1 = cap(c, 1);
        Vec::new()
    }) > 0
    {
        c1
    } else {
        pl::s("+")
    };
    let dir = if pl::str_eq(&pre_, "+") { n(1) } else { n(-1) };
    let tag_info = nv_hash.get("TagInfo");
    let tag = tag_info.get("Name");
    let shift_offset;
    if nv_hash.get("ShiftOffset").truthy() {
        shift_offset = nv_hash.get("ShiftOffset");
    } else {
        shift_offset = Value::new_hash();
        nv_hash.set("ShiftOffset", shift_offset.clone());
    }

    // initialize handler for eval warnings
    let handler = Value::Code(it.sub_ref("Image::ExifTool::SetWarning"));
    pl::local_sig_warn(it, handler, |it| {
        pl::call(it, "Image::ExifTool::SetWarning", vec![Value::Undef])?;

        // shift is applied to ValueConv value, so we must ValueConv-Shift-ValueConvInv
        let mut err;
        for type_ in ["ValueConv", "Shift", "ValueConvInv"] {
            if type_ == "Shift" {
                //### eval ShiftXxx function
                let code = pl::concat(&pl::concat(&pl::s("Shift"), &func), &pl::s("($val, $shift, $dir, $shiftOffset)"));
                let (r, after) = pl::eval_site(it, &code, "Image::ExifTool", &[("val", val.clone()), ("shift", shift.clone()), ("dir", dir.clone()), ("shiftOffset", shift_offset.clone())], Want::Scalar);
                err = r;
                val = after[0].clone();
                shift = after[1].clone();
            } else if tag_info.get(type_).truthy() {
                let conv = tag_info.get(type_);
                if conv.is_code() {
                    val = pl::call_code(it, &conv, vec![val.clone(), self_.clone()], Want::Scalar)?.pop().unwrap_or_default();
                } else {
                    if tag_info.get(type_).is_ref() {
                        return ret(pl::s(&format!("Can't handle {type_} for {} in ApplyShift()", tag.to_text())));
                    }
                    //### eval ValueConv/ValueConvInv ($val, $self)
                    let (r, _) = pl::eval_site(it, &tag_info.get(type_), "Image::ExifTool", &[("val", val.clone()), ("self", self_.clone())], Want::Scalar);
                    val = r;
                }
                err = Value::Undef;
            } else {
                continue;
            }
            // handle errors
            if err.truthy() {
                return ret(err);
            }
            let at = it.get_scalar("main::@");
            if at.truthy() {
                pl::call(it, "Image::ExifTool::SetWarning", vec![at])?;
            }
            if pl::call(it, "Image::ExifTool::GetWarning", vec![])?.truthy() {
                return ret(pl::call(it, "Image::ExifTool::CleanWarning", vec![])?);
            }
        }
        // update value in new value hash
        nv_hash.set("Value", Value::new_array(vec![val.clone()]));
        pl::ret_undef() // success
    })
}

/// Check date/time shift
/// Inputs: 0) shift type, 1) shift string (without sign)
/// Returns: updated shift string, or undef on error (and may update shift)
pub fn check_shift(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (type_, mut shift) = (arg(&a, 0), arg(&a, 1));
    let err;
    if pl::str_eq(&type_, "Time") {
        let mut c1 = Value::Undef;
        if re(r"^(\+|-)", "").subst(&mut shift, false, |c| {
            c1 = cap(c, 1);
            Vec::new()
        }) == 0
        {
            return ret(pl::s("No shift direction"));
        }
        // do a test shift to validate the shift string
        let test_time = pl::s("2005:11:02 09:00:13.25-04:00");
        let d = if pl::str_eq(&c1, "+") { n(1) } else { n(-1) };
        err = shift_time_impl(it, &[test_time, shift, d])?.0;
    } else {
        err = pl::s(&format!("Unknown shift type ({})", type_.to_text()));
    }
    ret(err)
}

/// return the number of days in a month
/// Inputs: 0) month number (Jan=1, may be outside range), 1) year
/// Returns: number of days in month
pub fn days_in_month(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(days_in_month_v(&arg(&a, 0), &arg(&a, 1))?)
}

fn days_in_month_v(mon: &Value, year: &Value) -> ER<Value> {
    let (mut mon, mut year) = (mon.clone(), year.clone());
    let days = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    // adjust to the range [0,11]
    while pl::lt(&mon, &n(1)) {
        mon = pl::add(&mon, &n(12));
        year = pl::sub(&year, &n(1));
    }
    while pl::gt(&mon, &n(12)) {
        mon = pl::sub(&mon, &n(12));
        year = pl::add(&year, &n(1));
    }
    // return standard number of days unless february on a leap year
    if !(pl::num_eq(&mon, &n(2)) && !pl::modulo(&year, &n(4))?.truthy()) {
        let idx = pl::sub(&mon, &n(1)).to_i64();
        let i = if idx < 0 { idx + 12 } else { idx };
        return Ok(days.get(i as usize).map(|&d| n(d)).unwrap_or_default());
    }
    // leap years don't occur on even centuries except every 400 years
    if pl::modulo(&year, &n(100))?.truthy() || !pl::modulo(&year, &n(400))?.truthy() {
        return Ok(n(29));
    }
    Ok(n(28))
}

/// split times into corresponding components: YYYY mm dd HH MM SS tzh tzm
/// Inputs: 0) date/time or shift string 1) reference to list for returned components
///         2) optional reference to list of time components (if shift string)
/// Returns: true on success
/// Returned components are 0-Y, 1-M, 2-D, 3-hr, 4-min, 5-sec, 6-tzhr, 7-tzmin
pub fn split_time(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (mut val, vals, time) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let time_at = |i: i64| time.at(i);
    // insert zeros if missing in shift string
    if time.truthy() {
        re(r"(^|[-+:\s]):", "").subst(&mut val, true, |c| [&cap(c, 1).to_bytes()[..], b"0:"].concat());
        re(r":([:\s]|$)", "").subst(&mut val, true, |c| [&b":0"[..], &cap(c, 1).to_bytes()[..]].concat());
    }
    // change dashes to colons in date (for XMP dates)
    if re(r"^(\d{4})-(\d{2})-(\d{2})", "").subst(&mut val, false, |c| pl::join(":", &[cap(c, 1), cap(c, 2), cap(c, 3)]).to_bytes().to_vec()) > 0 {
        pl::tr(&mut val, "T", " ", ""); // change 'T' separator to ' '
    }
    // add space before timezone to split it into a separate word
    re(r"(\+|-)", "").subst(&mut val, false, |c| [&b" "[..], &cap(c, 1).to_bytes()[..]].concat());
    let mut words = pl::split_ws(&val);
    let mut err = true;
    let mut v: Vec<Value> = Vec::new();
    loop {
        let word = vshift(&mut words);
        if word.is_undef() {
            break;
        }
        // split word into separate numbers (allow decimal points but no signs)
        let mut vals_ = re(r"(?=\d|\.\d)\d*(?:\.\d*)?", "").all(&word);
        if vals_.is_empty() {
            break;
        }
        if let Some(c) = re(r"^(\+|-)", "").m(&word) {
            // this is the timezone
            if defined_at(&v, 6) || vals_.len() > 2 {
                err = true;
                break;
            }
            let sign = if !pl::str_eq(&cap(&c, 1), "-") { n(1) } else { n(-1) };
            // apply sign to both minutes and seconds
            let x = vshift(&mut vals_);
            put_at(&mut v, 6, pl::mul(&sign, &x));
            let y = vshift(&mut vals_);
            put_at(&mut v, 7, pl::mul(&sign, &or0(&y)));
        } else if (!words.is_empty() && re(r"^\d+", "").is_match(&words[0])) // there is a time word to follow
            || (!time.truthy() && re(r"^\d{3}", "").is_match(&at(&vals_, 0))) // first value is year (3 or more digits)
            || (time.truthy() && time_at(3).is_undef() && !defined_at(&v, 0))
        // we don't have a time
        {
            // this is a date (must come first)
            if !v.is_empty() || vals_.len() > 3 {
                err = true;
                break;
            }
            if !time.truthy() && vals_.len() != 3 {
                err = true;
                break;
            }
            let x = vals_.pop().unwrap_or_default(); // take day first if only one specified
            put_at(&mut v, 2, x);
            let x = vals_.pop().unwrap_or_default();
            put_at(&mut v, 1, or0(&x));
            let x = vals_.pop().unwrap_or_default();
            put_at(&mut v, 0, or0(&x));
        } else {
            // this is a time (can't come after timezone)
            if defined_at(&v, 3) || defined_at(&v, 6) || vals_.len() > 3 {
                err = true;
                break;
            }
            if !time.truthy() && vals_.len() != 3 && vals_.len() != 2 {
                err = true;
                break;
            }
            let x = vshift(&mut vals_); // take hour first if only one specified
            put_at(&mut v, 3, x);
            let x = vshift(&mut vals_);
            put_at(&mut v, 4, or0(&x));
            let x = vshift(&mut vals_);
            put_at(&mut v, 5, or0(&x));
        }
        err = false;
    }
    if err || v.is_empty() {
        return ret(n(0));
    }
    if time.truthy() {
        // zero any required shift entries which aren't yet defined
        if !time_at(0).is_undef() && !defined_at(&v, 0) {
            for i in 0..3 {
                put_at(&mut v, i, n(0));
            }
        }
        if !time_at(3).is_undef() && !defined_at(&v, 3) {
            for i in 3..6 {
                put_at(&mut v, i, n(0));
            }
        }
        if !time_at(6).is_undef() && !defined_at(&v, 6) {
            for i in 6..8 {
                put_at(&mut v, i, n(0));
            }
        }
    }
    set_items(&vals, v); // return split time components
    ret(n(1))
}

fn split_time_v(it: &mut Interp, val: &Value, vals: &Value, time: Option<&Value>) -> ER<bool> {
    let mut a = vec![val.clone(), vals.clone()];
    if let Some(t) = time {
        a.push(t.clone());
    }
    Ok(split_time(it, a, Want::Scalar)?.pop().unwrap_or_default().truthy())
}

/// shift date/time by components
/// Inputs: 0) split date/time list ref, 1) split shift list ref,
///         2) shift direction, 3) reference to output list of shifted components
///         4) number of decimal points in seconds
///         5) reference to return time difference due to rounding
/// Returns: error string or undef on success
pub fn shift_components(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (time, shift, dir, to_time, dec, rnd_pt) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4), arg(&a, 5));
    // min/max for Y, M, D, h, m, s
    let min = [0, 1, 1, 0, 0, 0];
    let max = [10000, 12, 28, 24, 60, 60];
    let mx = |i: usize| max.get(i).map(|&x| n(x)).unwrap_or_default();
    let mn = |i: usize| min.get(i).map(|&x| n(x)).unwrap_or_default();
    //
    // apply the shift
    //
    let mut c = n(0);
    let mut i = 0usize;
    while i < time.len() {
        let mut v = pl::add(&pl::add(&or0(&time.at(i as i64)), &pl::mul(&dir, &or0(&shift.at(i as i64)))), &c);
        // handle fractional values by propagating remainders downwards
        if !pl::num_eq(&v, &pl::int(&v)) && i < 5 {
            let iv = pl::int(&v);
            c = pl::mul(&pl::sub(&v, &iv), &mx(i + 1));
            v = iv;
        } else {
            c = n(0);
        }
        to_time.set_at(i, v);
        i += 1;
    }
    // round off seconds to the required number of decimal points
    let sec = to_time.at(5);
    if !sec.is_undef() && !pl::num_eq(&sec, &pl::int(&sec)) {
        let mult = pow(&n(10), &dec);
        let rnd_sec = pl::div(&pl::int(&pl::add(&pl::mul(&sec, &mult), &pl::mul(&Value::Num(0.5), &spaceship(&sec, &n(0))))), &mult)?;
        if rnd_pt.truthy() {
            pl::set_deref(&rnd_pt, pl::sub(&sec, &rnd_sec))?;
        }
        to_time.set_at(5, rnd_sec);
    }
    //
    // handle overflows, starting with least significant number first (seconds)
    //
    c = n(0);
    for i in (0..=5usize).rev() {
        if time.at(i as i64).is_undef() {
            c = n(0);
            continue;
        }
        // apply shift and adjust for previous overflow
        let mut v = pl::add(&to_time.at(i as i64), &c);
        c = n(0); // set carry to zero
        // adjust for over/underflow
        let (min, max) = (mn(i), mx(i));
        if pl::lt(&v, &min) {
            if i == 2 {
                // 2 = day of month
                loop {
                    // add number of days in previous month
                    c = pl::sub(&c, &n(1));
                    let mon = pl::add(&to_time.at(i as i64 - 1), &c);
                    v = pl::add(&v, &days_in_month_v(&mon, &to_time.at(i as i64 - 2))?);
                    if !pl::lt(&v, &n(1)) {
                        break;
                    }
                }
            } else {
                let fc = pl::div(&pl::sub(&v, &min), &max)?;
                // carry ($c) must be largest integer equal to or less than $fc
                c = pl::int(&fc);
                if pl::gt(&c, &fc) {
                    c = pl::sub(&c, &n(1));
                }
                v = pl::sub(&v, &pl::mul(&c, &max));
            }
        } else if pl::ge(&v, &pl::add(&max, &min)) {
            if i == 2 {
                loop {
                    // test against number of days in current month
                    let mon = pl::add(&to_time.at(i as i64 - 1), &c);
                    let days = days_in_month_v(&mon, &to_time.at(i as i64 - 2))?;
                    if pl::le(&v, &days) {
                        break;
                    }
                    v = pl::sub(&v, &days);
                    c = pl::add(&c, &n(1));
                    if pl::le(&v, &n(28)) {
                        break;
                    }
                }
            } else {
                let fc = pl::div(&pl::sub(&pl::sub(&v, &max), &min), &max)?;
                // carry ($c) must be smallest integer greater than $fc
                c = pl::int(&fc);
                if pl::le(&c, &fc) {
                    c = pl::add(&c, &n(1));
                }
                v = pl::sub(&v, &pl::mul(&c, &max));
            }
        }
        to_time.set_at(i, v); // save the new value
    }
    // handle overflows in timezone
    if !to_time.at(6).is_undef() {
        let mut m = pl::add(&pl::mul(&to_time.at(6), &n(60)), &to_time.at(7));
        m = pl::add(&m, &pl::mul(&Value::Num(0.5), &spaceship(&m, &n(0)))); // avoid round-off errors
        to_time.set_at(6, pl::int(&pl::div(&m, &n(60))?));
        to_time.set_at(7, pl::int(&pl::sub(&m, &pl::mul(&to_time.at(6), &n(60)))));
    }
    pl::ret_undef() // success
}

/// Shift an integer or floating-point number
/// Inputs: 0) date/time string, 1) shift string, 2) shift direction (+1 or -1)
///         3) (unused)
/// Returns: undef and updates input value
pub fn shift_number(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (val, shift, dir) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    it.set_arg(0, pl::add(&val, &pl::mul(&shift, &dir)))?; // return shifted value
    pl::ret_undef() // success!
}

/// Shift date/time string
/// Inputs: 0) date/time string, 1) shift string, 2) shift direction (+1 or -1),
///            or 0 or undef to take shift direction from sign of shift,
///         3) reference to ShiftOffset hash (with Date, DateTime, Time, Timezone keys)
///   or    0) shift string (and operates on $_)
/// Returns: error string or undef on success and date/time string is updated
pub fn shift_time(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (err, new_val) = shift_time_impl(it, &a)?;
    if let Some(v) = new_val {
        if a.len() == 1 {
            it.set_scalar("main::_", v); // set $_ to the returned value
        } else {
            it.set_arg(0, v)?; // return shifted value
        }
    }
    ret(err)
}

/// The body of ShiftTime: the returned error, and the shifted value when
/// it succeeded (written to `$_[0]` or `$_` by the caller).
fn shift_time_impl(it: &mut Interp, a: &[Value]) -> ER<(Value, Option<Value>)> {
    let (mut val, mut shift, mut dir, shift_offset);
    let time = Value::new_array(vec![]);
    let shift_l = Value::new_array(vec![]);
    let to_time = Value::new_array(vec![]);
    let mode;
    let mut need_shift_offset = false;
    let dec;

    if a.len() == 1 {
        // single argument form of ShiftTime()?
        val = it.get_scalar("main::_");
        shift = arg(a, 0);
        dir = Value::Undef;
        shift_offset = Value::Undef;
    } else {
        (val, shift, dir, shift_offset) = (arg(a, 0), arg(a, 1), arg(a, 2), arg(a, 3));
    }
    if !dir.truthy() {
        let mut c1 = Value::Undef;
        let m = re(r"^(\+|-)", "").subst(&mut shift, false, |c| {
            c1 = cap(c, 1);
            Vec::new()
        }) > 0;
        dir = if m && pl::str_eq(&c1, "-") { n(-1) } else { n(1) };
    }
    //
    // figure out what we are dealing with (time, date or date/time)
    //
    if !split_time_v(it, &val, &time, None)? {
        return Ok((pl::s(&format!("Invalid time string ({})", val.to_text())), None));
    }
    let t = |i: i64| time.at(i);
    if !t(0).is_undef() {
        if pl::str_eq(&t(0), "0000") {
            return Ok((pl::s("Can't shift from year 0000"), None));
        }
        mode = if !t(3).is_undef() { "DateTime" } else { "Date" };
    } else if !t(3).is_undef() {
        mode = "Time";
    } else {
        mode = "";
    }
    // get number of digits after the seconds decimal point
    let m5 = if t(5).is_undef() { None } else { re(r"\.(\d+)", "").m(&t(5)) };
    if let Some(c) = m5 {
        dec = n(cap(&c, 1).to_bytes().len() as i64);
    } else {
        dec = n(0);
    }
    if shift_offset.truthy() {
        if shift_offset.get(mode).is_undef() {
            need_shift_offset = true;
        }
        if !t(6).is_undef() && shift_offset.get("Timezone").is_undef() {
            need_shift_offset = true;
        }
    } else {
        need_shift_offset = true;
    }
    if need_shift_offset {
        //
        // apply date/time shift the hard way
        //
        if !split_time_v(it, &shift, &shift_l, Some(&time))? {
            return Ok((pl::s(&format!("Invalid shift string ({})", shift.to_text())), None));
        }

        // change 'Z' timezone to '+00:00' only if necessary
        if shift_l.len() > 6 && time.len() <= 6 && re(r"Z$", "").subst(&mut val, false, |_| b"+00:00".to_vec()) > 0 {
            time.set_at(6, n(0));
            time.set_at(7, n(0));
        }
        let rnd_diff = pl::sref(Value::Undef);
        let err = shift_components(it, vec![time.clone(), shift_l.clone(), dir.clone(), to_time.clone(), dec.clone(), rnd_diff.clone()], Want::Scalar)?.pop().unwrap_or_default();
        if err.truthy() {
            return Ok((err, None));
        }
        let rnd_diff = rnd_diff.deref();
        //
        // calculate and save the shift offsets for next time
        //
        if shift_offset.truthy() {
            if !t(0).is_undef() || !t(3).is_undef() {
                let mut tm1: Vec<Value> = [0, 0, 0, 1, 0, 2000].iter().map(|&x| n(x)).collect();
                let mut tm2 = tm1.clone();
                let tt = |i: i64| to_time.at(i);
                if !t(0).is_undef() {
                    (tm1[3], tm1[4], tm1[5]) = (t(2), t(1), t(0));
                    (tm2[3], tm2[4], tm2[5]) = (tt(2), tt(1), tt(0));
                    tm1[4] = pl::sub(&tm1[4], &n(1)); // month should start from 0
                    tm2[4] = pl::sub(&tm2[4], &n(1));
                }
                let mut diff = n(0);
                if !t(3).is_undef() {
                    (tm1[0], tm1[1], tm1[2]) = (t(5), t(4), t(3));
                    (tm2[0], tm2[1], tm2[2]) = (tt(5), tt(4), tt(3));
                    // handle fractional seconds separately
                    diff = pl::sub(&pl::sub(&tm2[0], &pl::int(&tm2[0])), &pl::sub(&tm1[0], &pl::int(&tm1[0])));
                    if !rnd_diff.is_undef() {
                        diff = pl::add(&diff, &rnd_diff); // un-do rounding
                    }
                    tm1[0] = pl::int(&tm1[0]);
                    tm2[0] = pl::int(&tm2[0]);
                }
                let r = pl::eval_block(it, |it| {
                    crate::runtime::require(it, "Time::Local")?;
                    let g2 = pl::call(it, "Time::Local::timegm", tm2.clone())?;
                    let g1 = pl::call(it, "Time::Local::timegm", tm1.clone())?;
                    Ok(pl::sub(&g2, &g1))
                })?;
                if let Some(d) = r {
                    diff = pl::add(&diff, &d);
                }
                // not a problem if we failed here since we'll just try again next time,
                // so don't return error message
                if !it.get_scalar("main::@").truthy() {
                    let mode = if !t(0).is_undef() {
                        if !t(3).is_undef() {
                            "DateTime"
                        } else {
                            "Date"
                        }
                    } else {
                        "Time"
                    };
                    shift_offset.set(mode, diff);
                }
            }
            if !t(6).is_undef() {
                let tz = pl::sub(&pl::add(&pl::mul(&pl::sub(&to_time.at(6), &t(6)), &n(60)), &to_time.at(7)), &t(7));
                shift_offset.set("Timezone", tz);
            }
        }
    } else {
        //
        // apply shift from previously calculated offsets
        //
        if shift_offset.get("Timezone").truthy() && time.len() <= 6 {
            // change 'Z' timezone to '+00:00' only if necessary
            if re(r"Z$", "").subst(&mut val, false, |_| b"+00:00".to_vec()) > 0 {
                time.set_at(6, n(0));
                time.set_at(7, n(0));
            }
        }
        // apply the previous date/time shift if necessary
        if !mode.is_empty() {
            let mut tm: Vec<Value> = [0, 0, 0, 1, 0, 2000].iter().map(|&x| n(x)).collect();
            if !t(0).is_undef() {
                (tm[3], tm[4], tm[5]) = (t(2), t(1), t(0));
                tm[4] = pl::sub(&tm[4], &n(1)); // month should start from 0
            }
            if !t(3).is_undef() {
                (tm[0], tm[1], tm[2]) = (t(5), t(4), t(3));
            }
            // save fractional seconds
            let mut frac = pl::sub(&tm[0], &pl::int(&tm[0]));
            tm[0] = pl::int(&tm[0]);
            let tmv = tm.clone();
            let f = frac.clone();
            let r = pl::eval_block(it, |it| {
                crate::runtime::require(it, "Time::Local")?;
                Ok(pl::add(&pl::call(it, "Time::Local::timegm", tmv)?, &f))
            })?;
            let at = it.get_scalar("main::@");
            if at.truthy() {
                return Ok((pl::call(it, "Image::ExifTool::CleanWarning", vec![at])?, None));
            }
            let mut tmx = r.unwrap_or_default();
            tmx = pl::add(&tmx, &shift_offset.get(mode)); // apply the shift
            // save fractional seconds in shifted time
            frac = pl::sub(&tmx, &pl::int(&tmx));
            if frac.truthy() {
                tmx = pl::int(&tmx);
                // must account for any rounding that could occur
                if pl::ge(&pl::add(&frac, &pl::mul(&Value::Num(0.5), &pow(&n(10), &pl::neg(&dec)))), &n(1)) {
                    tmx = pl::add(&tmx, &n(1));
                    frac = n(0);
                }
            }
            let tm = time_list(it, &tmx, false);
            let mut tt: Vec<Value> = tm[0..6].iter().rev().cloned().collect();
            tt[0] = pl::add(&tt[0], &n(1900));
            tt[1] = pl::add(&tt[1], &n(1));
            tt[5] = pl::add(&tt[5], &frac); // add the fractional seconds back in
            set_items(&to_time, tt);
        }
        // apply the previous timezone shift if necessary
        if !t(6).is_undef() {
            let mut m = pl::add(&pl::mul(&t(6), &n(60)), &t(7));
            m = pl::add(&m, &shift_offset.get("Timezone"));
            m = pl::add(&m, &pl::mul(&Value::Num(0.5), &spaceship(&m, &n(0)))); // avoid round-off errors
            to_time.set_at(6, pl::int(&pl::div(&m, &n(60))?));
            to_time.set_at(7, pl::int(&pl::sub(&m, &pl::mul(&to_time.at(6), &n(60)))));
        }
    }
    //
    // insert shifted time components back into original string
    //
    let re_num = rx(r"((?=\d|\.\d)\d*(\.\d*)?)", "");
    let re_tz = rx(r"((?:\+|-)(?=\d|\.\d)\d*(\.\d*)?)", "");
    let mut pos = 0usize;
    let mut i = 0usize;
    while i < to_time.len() {
        let ii = i as i64;
        if t(ii).is_undef() || to_time.at(ii).is_undef() {
            i += 1;
            continue;
        }
        let (v, d, s);
        if i != 6 {
            // not timezone hours
            let Some((c, p)) = match_g(&re_num, &val, pos) else { break };
            pos = p;
            if pl::num_eq(&to_time.at(ii), &t(ii)) {
                i += 1;
                continue;
            }
            v = cap(&c, 1); // value
            d = cap(&c, 2); // decimal part of value
            s = ""; // no sign
        } else {
            if pl::num_eq(&t(ii), &to_time.at(ii)) && pl::num_eq(&t(ii + 1), &to_time.at(ii + 1)) {
                break;
            }
            let Some((c, p)) = match_g(&re_tz, &val, pos) else { break };
            pos = p;
            v = cap(&c, 1);
            d = cap(&c, 2);
            if pl::ge(&to_time.at(6), &n(0)) && pl::ge(&to_time.at(7), &n(0)) {
                s = "+";
            } else {
                s = "-";
                to_time.set_at(6, pl::neg(&to_time.at(6)));
                to_time.set_at(7, pl::neg(&to_time.at(7)));
            }
        }
        let nv = to_time.at(ii);
        let len = v.to_bytes().len();
        let sig = len as i64 - s.len() as i64;
        let dec = if d.truthy() { d.to_bytes().len() as i64 - 1 } else { 0 };
        let fmt = if dec != 0 { format!("{s}%0{sig}.{dec}f") } else { format!("{s}%0{sig}d") };
        let new_num = sprintf_s(fmt.as_bytes(), &[nv]);
        let b = val.to_bytes();
        let start = pos - len;
        let mut out = b[..start].to_vec();
        out.extend_from_slice(&new_num.to_bytes());
        out.extend_from_slice(&b[pos..]);
        val = Value::bytes(out);
        pos = pos + new_num.to_bytes().len() - len;
        i += 1;
    }
    Ok((Value::Undef, Some(val))) // success!
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::ApplyShift", apply_shift);
    it.define_native("Image::ExifTool::CheckShift", check_shift);
    it.define_native("Image::ExifTool::DaysInMonth", days_in_month);
    it.define_native("Image::ExifTool::SplitTime", split_time);
    it.define_native("Image::ExifTool::ShiftComponents", shift_components);
    it.define_native("Image::ExifTool::ShiftNumber", shift_number);
    it.define_native("Image::ExifTool::ShiftTime", shift_time);
}
