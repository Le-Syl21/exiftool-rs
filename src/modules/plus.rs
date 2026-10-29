//! Image::ExifTool::PLUS (Image/ExifTool/PLUS.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, perl_eval1};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

/// `chr($c)`: a byte below 256, else perl's own builtin (a wide character).
fn chr(it: &mut Interp, c: &Value) -> ER<Value> {
    let i = pl::int(c).to_i64();
    if (0..256).contains(&i) {
        return Ok(Value::bytes([i as u8]));
    }
    let cc = cell(c.clone());
    perl_eval1(it, "chr($c)", "Image::ExifTool::PLUS", &[("c", &cc)])
}

/// Validate Media Summary Code
/// Inputs: 0) Media Usage Code
/// Returns: true if OK, false on severe error
/// - issues warning for detected format problems
/// - repairs some repairable problems
pub fn validate_media_summary(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let val = arg(&a, 0);

    let mut a = pl::split(&re(r"\|", ""), &val, 0);
    if !(a.len() >= 4 && arg(&a, 0).to_bytes().is_empty()) {
        pl::warn(it, "Not a valid Media Summary Code\n")?;
        return ret(n(0));
    }
    if !arg(&a, 1).eq_str("PLUS") {
        pl::warn(it, "Unrecognized Media Usage standard\n")?;
        return ret(n(0));
    }
    if !re(r"^V(\d+)", "").is_match(&arg(&a, 2)) {
        pl::warn(it, "Unrecognized Media Usage version\n")?;
    }
    let Some(m) = re(r"^U(\d+)", "").m(&arg(&a, 3)) else {
        pl::warn(it, "Invalid Media Usage count\n")?;
        return ret(n(0));
    };
    let mut num_usages = cap(&m, 1);
    if !pl::num_eq(&num_usages, &n(a.len() as i64 - 4)) {
        pl::warn(it, "Fixed incorrect number of Media Usages\n")?;
        num_usages = n(a.len() as i64 - 4);
        a[3] = pl::sprintf("U%.3d", &[num_usages.clone()]);
    }
    let mut i = n(1);
    while pl::le(&i, &num_usages) {
        let ii = i.to_i64() as usize;
        let usage = arg(&a, ii + 3);
        let Some(m) = re(r"^1I([A-Z])([A-Z])", "").m(&usage) else {
            pl::warn(it, &format!("Missing Media Usage {} item count\n", i.to_text()))?;
            return ret(n(0));
        };
        let ulen = pl::length(&usage);
        if pl::modulo(&ulen, &n(4))?.truthy() {
            pl::warn(it, &format!("Incorrect Media Usage {} length\n", i.to_text()))?;
            return ret(n(0));
        }
        let ord = |v: &Value| n(v.to_bytes().first().copied().unwrap_or(0) as i64);
        let mut num_items = pl::add(&pl::sub(&pl::add(&pl::mul(&pl::sub(&ord(&cap(&m, 1)), &n(65)), &n(26)), &ord(&cap(&m, 2))), &n(65)), &n(1));
        if !pl::num_eq(&ulen, &pl::mul(&n(4), &pl::add(&num_items, &n(1)))) {
            num_items = pl::sub(&pl::div(&ulen, &n(4))?, &n(1));
            pl::warn(it, &format!("Fixed incorrect Media Usage {} item count\n", i.to_text()))?;
            let c1 = chr(it, &pl::add(&n(65), &pl::int(&pl::div(&num_items, &n(26))?)))?;
            let c2 = chr(it, &pl::modulo(&num_items, &n(26))?)?;
            let rest = pl::substr(&usage, 4, None);
            a[ii + 3] = pl::concat(&pl::concat(&pl::concat(&pl::s("1I"), &c1), &c2), &rest);
        }
        let mut j = n(1);
        while pl::le(&j, &num_items) {
            let item = pl::substr(&usage, pl::mul(&j, &n(4)).to_i64(), Some(4));
            if !re(r"^\d[A-Z]{3}$", "").is_match(&item) {
                let msg = pl::concat(&pl::concat(&pl::concat(&pl::concat(&pl::s("Invalid item \""), &item), &pl::s("\" for Media Usage ")), &i), &pl::s("\n"));
                it.warn(msg)?;
                return ret(n(0));
            }
            j = pl::add(&j, &n(1));
        }
        i = pl::add(&i, &n(1));
    }
    if it.get_scalar("Image::ExifTool::evalWarning").truthy() {
        // $_[0] = join('|', @a) . '|': after the `shift`, $_[0] is a new
        // element of @_, not the caller's variable, so this changes nothing
        // outside the sub (as in the Perl)
        let _ = pl::concat(&pl::join("|", &a), &pl::s("|"));
    }
    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::PLUS::ValidateMediaSummary", validate_media_summary);
}
