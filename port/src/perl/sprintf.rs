//! Perl's `sprintf`, as `sv_vcatpvfn` implements it.
//!
//! ExifTool formats about 1,250 values with `sprintf`, and its output is the
//! test's expected output, so the details matter: `%d` truncates a float,
//! `%u` and `%x` read a negative number as unsigned, `%g` follows C, a
//! missing argument formats as undef, `%v` joins the ordinals of a string.

use crate::perl::value::{Number, Value};

/// How a conversion of `sprintf` used an argument, for perl's warnings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ArgUse {
    /// The argument (its index) was read as a string.
    Str(usize),
    /// The argument was read as a number.
    Num(usize),
    /// A conversion had no argument left ("Missing argument").
    Missing,
    /// Arguments were left over ("Redundant argument").
    Redundant,
}

/// `sprintf($fmt, @args)`.
pub fn sprintf(fmt: &[u8], args: &[Value]) -> Vec<u8> {
    sprintf_traced(fmt, args, None)
}

thread_local! {
    /// The sprintf running makes a character string: the format is its
    /// UTF-8 form, `%s` and `%c` give characters, widths count them.
    static UTF: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// A `%c` above 0xFF was met in a byte string (perl upgrades it).
    static WIDE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The error the last sprintf stopped on (see `take_error`).
    static ERROR: std::cell::Cell<Option<Error>> = const { std::cell::Cell::new(None) };
}

/// Why a `sprintf` stopped: perl croaks on these.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Error {
    /// A width or precision beyond perl's limit (a quarter of the address
    /// space): "Integer overflow in format string for sprintf".
    Overflow,
    /// A float conversion whose result would pass INT_MAX: "Numeric format
    /// result too large".
    TooLarge,
    /// A field longer than the port builds (`BUILD_MAX`). Perl tries, and
    /// ends with "Out of memory!" when the system says no; the port dies.
    OutOfMemory,
}

impl Error {
    /// The message perl dies with, for `op` (`sprintf` or `printf`).
    pub fn message(self, op: &str) -> String {
        match self {
            Error::Overflow => format!("Integer overflow in format string for {op}"),
            Error::TooLarge => "Numeric format result too large".to_string(),
            Error::OutOfMemory => format!("Out of memory in {op}"),
        }
    }
}

/// The error the last `sprintf` call stopped on, if any (it returns what
/// it had formatted so far): the caller dies with it, as perl does.
pub fn take_error() -> Option<Error> {
    ERROR.with(|e| e.take())
}

/// Stop a sprintf on a perl error (`take_error` gives it back).
fn stop(e: Error, out: Vec<u8>) -> Vec<u8> {
    ERROR.with(|c| c.set(Some(e)));
    out
}

/// Perl's limit on a width or precision (sv.c, `expect_number` and
/// `sprintf_arg_num_val`): beyond it the format "overflows".
const WIDTH_MAX: u64 = (usize::MAX / 4) as u64;

/// The longest field the port builds. Perl allocates any width up to
/// `WIDTH_MAX` (and runs out of memory past a few GB); a format that asks
/// for more than this is refused instead of exhausting the memory.
pub const BUILD_MAX: usize = 1 << 28;

/// A width or precision written in the format: perl croaks when one more
/// digit could pass `WIDTH_MAX`.
fn format_number(digits: &[u8]) -> Result<usize, Error> {
    let mut n: u64 = 0;
    for (k, &d) in digits.iter().enumerate() {
        if k > 0 && n > (WIDTH_MAX - 9) / 10 {
            return Err(Error::Overflow);
        }
        n = n * 10 + (d - b'0') as u64;
    }
    Ok(n as usize)
}

/// Perl's SvIV of a `*` argument: a float past the IV range goes through
/// a UV, so that 1e19 is negative and 1e300 or Inf -1 (as `sprintf("%*d",
/// 1e300, 5)` shows: a width of 1, left-justified).
fn sv_iv(v: &Value) -> i64 {
    match v.to_number() {
        Number::Float(f) if f.is_nan() => 0,
        Number::Float(f) if f >= 9.223372036854775808e18 => crate::perl::ops::to_uv(v) as i64,
        Number::UInt(u) => u as i64,
        _ => v.to_i64(),
    }
}

/// A width or precision given as an argument (`*`), and whether it is
/// negative.
fn argument_number(n: i64) -> Result<(usize, bool), Error> {
    if n.unsigned_abs() > WIDTH_MAX {
        return Err(Error::Overflow);
    }
    Ok((n.unsigned_abs() as usize, n < 0))
}

/// `sprintf` with perl's character strings: the result is a character
/// string when the format or an argument is one (or a `%c` is above 0xFF).
pub fn sprintf_value(fmt: &Value, args: &[Value], mut trace: Option<&mut Vec<ArgUse>>) -> Value {
    let utf = fmt.is_utf8() || args.iter().any(Value::is_utf8);
    if !utf {
        WIDE.with(|w| w.set(false));
        let out = sprintf_traced(&fmt.to_bytes(), args, trace.as_deref_mut());
        if !WIDE.with(|w| w.get()) {
            return Value::bytes(out);
        }
    }
    UTF.with(|u| u.set(true));
    let out = sprintf_traced(&fmt.utf8_bytes(), args, trace);
    UTF.with(|u| u.set(false));
    Value::chars(out)
}

fn utf_mode() -> bool {
    UTF.with(|u| u.get())
}

/// `sprintf`, recording in `trace` how each conversion used its argument.
pub fn sprintf_traced(fmt: &[u8], args: &[Value], trace: Option<&mut Vec<ArgUse>>) -> Vec<u8> {
    ERROR.with(|e| e.set(None));
    let mut out = Vec::with_capacity(fmt.len() + 16);
    let mut next_arg = 0usize;
    let mut i = 0;
    let tracing = trace.is_some();
    let uses = std::cell::RefCell::new(Vec::new());
    let explicit_used = std::cell::Cell::new(false);
    // (the index taken is recorded; whether it is read as a number is
    // set by the conversion)
    let take = |next: &mut usize, explicit: Option<usize>| -> Value {
        let idx = match explicit {
            Some(n) => {
                explicit_used.set(true);
                n.saturating_sub(1)
            }
            None => {
                let n = *next;
                *next += 1;
                n
            }
        };
        if tracing {
            uses.borrow_mut().push(if idx < args.len() { ArgUse::Num(idx) } else { ArgUse::Missing });
        }
        args.get(idx).cloned().unwrap_or(Value::Undef)
    };
    let as_str = || {
        if tracing {
            let last = uses.borrow().last().copied();
            if let Some(ArgUse::Num(i)) = last {
                *uses.borrow_mut().last_mut().unwrap() = ArgUse::Str(i);
            }
        }
    };
    while i < fmt.len() {
        if out.len() > BUILD_MAX {
            return stop(Error::OutOfMemory, out);
        }
        let c = fmt[i];
        if c != b'%' {
            out.push(c);
            i += 1;
            continue;
        }
        let spec_start = i;
        i += 1;
        if i < fmt.len() && fmt[i] == b'%' {
            out.push(b'%');
            i += 1;
            continue;
        }
        // %N$ explicit argument index.
        let mut explicit = None;
        let mut j = i;
        while j < fmt.len() && fmt[j].is_ascii_digit() {
            j += 1;
        }
        if j > i && j < fmt.len() && fmt[j] == b'$' {
            explicit = std::str::from_utf8(&fmt[i..j]).ok().and_then(|s| s.parse().ok());
            i = j + 1;
        }
        let mut spec = Spec::default();
        // Flags.
        while i < fmt.len() {
            match fmt[i] {
                b'-' => spec.left = true,
                b'+' => spec.plus = true,
                b' ' => spec.space = true,
                b'0' => spec.zero = true,
                b'#' => spec.alt = true,
                _ => break,
            }
            i += 1;
        }
        // Vector flag: %vd joins the formatted ordinals of a string with ".".
        let mut vector = false;
        if i < fmt.len() && fmt[i] == b'v' {
            vector = true;
            i += 1;
        }
        // Width.
        if i < fmt.len() && fmt[i] == b'*' {
            i += 1;
            let (w, neg) = match argument_number(sv_iv(&take(&mut next_arg, None))) {
                Ok(w) => w,
                Err(e) => return stop(e, out),
            };
            if neg {
                spec.left = true;
            }
            spec.width = Some(w);
        } else {
            let s = i;
            while i < fmt.len() && fmt[i].is_ascii_digit() {
                i += 1;
            }
            if i > s {
                match format_number(&fmt[s..i]) {
                    Ok(w) => spec.width = Some(w),
                    Err(e) => return stop(e, out),
                }
            }
        }
        // Precision.
        if i < fmt.len() && fmt[i] == b'.' {
            i += 1;
            if i < fmt.len() && fmt[i] == b'*' {
                i += 1;
                let (p, neg) = match argument_number(sv_iv(&take(&mut next_arg, None))) {
                    Ok(p) => p,
                    Err(e) => return stop(e, out),
                };
                spec.prec = if neg { None } else { Some(p) };
            } else {
                let s = i;
                while i < fmt.len() && fmt[i].is_ascii_digit() {
                    i += 1;
                }
                // (leading zeros add nothing: "%.007f")
                let digits = &fmt[s..i];
                let start = digits.iter().position(|&d| d != b'0').unwrap_or(digits.len());
                match format_number(&digits[start..]) {
                    Ok(p) => spec.prec = Some(p),
                    Err(e) => return stop(e, out),
                }
            }
        }
        // A float's field past INT_MAX is perl's own error (its width, or
        // its precision and a few characters). Otherwise a precision makes
        // digits for a number; it only cuts a string.
        let conv = fmt[i..].iter().find(|c| !b"hlqLVztj".contains(c));
        let float = conv.is_some_and(|c| b"eEfFgGaA".contains(c));
        let digits = float || conv.is_some_and(|c| b"diuoxXbB".contains(c));
        if float && spec.width.unwrap_or(0).max(spec.prec.unwrap_or(0).saturating_add(20)) > i32::MAX as usize {
            return stop(Error::TooLarge, out);
        }
        if spec.width.unwrap_or(0) > BUILD_MAX || (digits && spec.prec.unwrap_or(0) > BUILD_MAX) {
            return stop(Error::OutOfMemory, out);
        }
        // Size modifiers, ignored: perl's IV and NV are already the widest.
        while i < fmt.len() && matches!(fmt[i], b'h' | b'l' | b'q' | b'L' | b'V' | b'z' | b't' | b'j') {
            i += 1;
        }
        let Some(&conv) = fmt.get(i) else {
            // An incomplete spec is copied as is ("Invalid conversion").
            out.extend_from_slice(&fmt[spec_start..]);
            break;
        };
        i += 1;
        if vector {
            let v = take(&mut next_arg, explicit);
            as_str();
            let ords: Vec<u32> = if utf_mode() { crate::perl::utf8::chars(&v.utf8_bytes()).collect() } else { v.to_bytes().iter().map(|&b| b as u32).collect() };
            let parts: Vec<Vec<u8>> = ords
                .iter()
                .map(|&b| format_one(conv, &spec, &Value::Int(b as i64)).unwrap_or_default())
                .collect();
            out.extend(parts.join(&b'.'));
            continue;
        }
        if !b"csdiuoxXbBeEfFgGaA".contains(&conv) {
            // Unknown conversion: perl prints it verbatim.
            out.extend_from_slice(&fmt[spec_start..i]);
            continue;
        }
        let v = take(&mut next_arg, explicit);
        if conv == b's' {
            as_str();
        }
        match format_one(conv, &spec, &v) {
            Some(s) => out.extend(s),
            None => out.extend_from_slice(&fmt[spec_start..i]),
        }
    }
    if let Some(t) = trace {
        let mut u = uses.into_inner();
        if !explicit_used.get() && next_arg < args.len() {
            u.push(ArgUse::Redundant);
        }
        *t = u;
    }
    out
}

#[derive(Default, Clone)]
struct Spec {
    left: bool,
    plus: bool,
    space: bool,
    zero: bool,
    alt: bool,
    width: Option<usize>,
    prec: Option<usize>,
}

fn format_one(conv: u8, spec: &Spec, v: &Value) -> Option<Vec<u8>> {
    let body: Vec<u8> = match conv {
        b'c' => {
            let n = v.to_i64();
            let ch = if n < 0 { 0xFFFD } else { n as u32 };
            if utf_mode() {
                let mut o = Vec::new();
                crate::perl::utf8::push_cp(&mut o, ch);
                o
            } else if ch < 256 {
                vec![ch as u8]
            } else {
                WIDE.with(|w| w.set(true));
                char::from_u32(ch).unwrap_or('\u{FFFD}').to_string().into_bytes()
            }
        }
        b's' if utf_mode() => {
            let s = v.utf8_bytes();
            match spec.prec {
                Some(p) => s[..crate::perl::utf8::byte_offset(&s, p)].to_vec(),
                _ => s.to_vec(),
            }
        }
        b's' => {
            let s = v.to_bytes();
            match spec.prec {
                Some(p) if p < s.len() => s[..p].to_vec(),
                _ => s.to_vec(),
            }
        }
        b'd' | b'i' | b'u' | b'o' | b'x' | b'X' | b'b' | b'B' => {
            // Inf and NaN print as text in the integer formats, padded like a
            // string (perl: "00Inf" for %05d).
            if let Number::Float(f) = v.to_number() {
                if !f.is_finite() {
                    let mut t = if f.is_nan() {
                        "NaN".to_string()
                    } else if f < 0.0 {
                        "-Inf".to_string()
                    } else {
                        "Inf".to_string()
                    };
                    if f > 0.0 && (spec.plus || spec.space) {
                        t.insert(0, '+');
                    }
                    let mut sp = spec.clone();
                    sp.prec = None;
                    return Some(pad(&sp, t.into_bytes(), false));
                }
            }
            return Some(if matches!(conv, b'd' | b'i') {
                pad_number(spec, int_string(v, spec), true)
            } else {
                pad_number(spec, unsigned_string(conv, v, spec), false)
            });
        }
        b'e' | b'E' | b'f' | b'F' | b'g' | b'G' | b'a' | b'A' => {
            let n = v.to_f64();
            let mut s = float_string(conv, n, spec);
            if !s.starts_with('-') && !n.is_nan() {
                if spec.plus {
                    s.insert(0, '+');
                } else if spec.space {
                    s.insert(0, ' ');
                }
            }
            let numeric = n.is_finite();
            return Some(pad(spec, s.into_bytes(), numeric));
        }
        _ => return None,
    };
    Some(pad(spec, body, false))
}

/// `%d`: the integer value, a float truncated toward zero; a float beyond the
/// IV range prints with `%.0f`, as perl does.
fn int_string(v: &Value, spec: &Spec) -> String {
    let digits_and_sign = match v.to_number() {
        Number::Int(i) => (i.unsigned_abs().to_string(), i < 0),
        // %d reads a UV as an IV: 2**64-1 prints as -1.
        Number::UInt(u) => {
            let i = u as i64;
            (i.unsigned_abs().to_string(), i < 0)
        }
        Number::Float(f) => {
            // perl's I_V cast: below IV_MIN saturates to IV_MIN; above IV_MAX
            // goes through UV (saturating at UV_MAX), then is read as an IV,
            // so 1e20 prints as -1.
            let i = if f < -9.223372036854776e18 {
                i64::MIN
            } else if f < 9.223372036854776e18 {
                f.trunc() as i64
            } else if f < 1.8446744073709552e19 {
                (f as u64) as i64
            } else {
                -1
            };
            (i.unsigned_abs().to_string(), i < 0)
        }
    };
    let (mut digits, neg) = digits_and_sign;
    if let Some(p) = spec.prec {
        if p == 0 && digits == "0" {
            digits.clear();
        }
        while digits.len() < p {
            digits.insert(0, '0');
        }
    }
    let sign = if neg {
        "-"
    } else if spec.plus {
        "+"
    } else if spec.space {
        " "
    } else {
        ""
    };
    format!("{sign}{digits}")
}

/// `%u %o %x %X %b %B`: the value as an unsigned 64-bit integer (a negative
/// IV wraps, as a C cast; a float is truncated, then wrapped).
fn unsigned_string(conv: u8, v: &Value, spec: &Spec) -> String {
    let u: u64 = match v.to_number() {
        Number::Int(i) => i as u64,
        Number::UInt(u) => u,
        Number::Float(f) => {
            if f < 0.0 {
                (f.trunc() as i64) as u64
            } else if f >= 1.8446744073709552e19 {
                u64::MAX
            } else {
                f as u64
            }
        }
    };
    let mut digits = match conv {
        b'u' => u.to_string(),
        b'o' => format!("{u:o}"),
        b'x' => format!("{u:x}"),
        b'X' => format!("{u:X}"),
        _ => format!("{u:b}"),
    };
    if let Some(p) = spec.prec {
        if p == 0 && u == 0 {
            digits.clear();
        }
        while digits.len() < p {
            digits.insert(0, '0');
        }
    }
    if spec.alt && u != 0 {
        let prefix = match conv {
            b'o' if !digits.starts_with('0') => "0",
            b'x' => "0x",
            b'X' => "0X",
            b'b' => "0b",
            b'B' => "0B",
            _ => "",
        };
        digits.insert_str(0, prefix);
    }
    digits
}

/// Digits past which a double's exact decimal expansion is all zeros (its
/// fraction has at most 1074 digits, its significand at most 767): Rust
/// formats a precision of up to 65535 only, perl any.
const EXACT_DIGITS: usize = 1100;

/// `%.Pf`, for any P.
fn fixed(n: f64, prec: usize) -> String {
    if prec <= EXACT_DIGITS {
        return format!("{:.*}", prec, n);
    }
    let mut s = format!("{:.*}", EXACT_DIGITS, n);
    s.extend(std::iter::repeat('0').take(prec - EXACT_DIGITS));
    s
}

/// Rust's `{:.Pe}`, for any P.
fn sci(n: f64, prec: usize) -> String {
    if prec <= EXACT_DIGITS {
        return format!("{:.*e}", prec, n);
    }
    let s = format!("{:.*e}", EXACT_DIGITS, n);
    let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
    let mut out = String::with_capacity(prec + 8);
    out.push_str(m);
    out.extend(std::iter::repeat('0').take(prec - EXACT_DIGITS));
    out.push('e');
    out.push_str(e);
    out
}

fn float_string(conv: u8, n: f64, spec: &Spec) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Inf".into() } else { "-Inf".into() };
    }
    let upper = conv.is_ascii_uppercase();
    let s = match conv.to_ascii_lowercase() {
        b'f' => fixed(n, spec.prec.unwrap_or(6)),
        b'e' => format_e(n, spec.prec.unwrap_or(6), spec.alt),
        b'g' => format_g(n, spec.prec.unwrap_or(6), spec.alt, true),
        _ => format_e(n, spec.prec.unwrap_or(13), spec.alt), // %a: not used by ExifTool
    };
    let s = if spec.alt && conv.eq_ignore_ascii_case(&b'f') && !s.contains('.') {
        format!("{s}.")
    } else {
        s
    };
    if upper {
        s.to_uppercase()
    } else {
        s
    }
}

/// C's `%.Pe`: one digit, a point, P digits, `e`, a sign and at least two
/// exponent digits.
pub fn format_e(n: f64, prec: usize, alt: bool) -> String {
    let s = sci(n, prec);
    let (mantissa, exp) = s.split_once('e').unwrap_or((&s, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let mantissa = if alt && prec == 0 { format!("{mantissa}.") } else { mantissa.to_string() };
    let sign = if exp < 0 { '-' } else { '+' };
    format!("{mantissa}e{sign}{:02}", exp.unsigned_abs())
}

/// C's `%.Pg`: `%e` when the exponent is below -4 or at least P, `%f`
/// otherwise; trailing zeros removed unless `#`. `c_style` is unused (kept
/// for the perl stringification caller, which is `%.15g`).
pub fn format_g(n: f64, prec: usize, alt: bool, _c_style: bool) -> String {
    let p = if prec == 0 { 1 } else { prec };
    if n == 0.0 {
        return if alt {
            fixed(n, p - 1)
        } else if n.is_sign_negative() {
            "-0".into()
        } else {
            "0".into()
        };
    }
    // The exponent after rounding to P significant digits.
    let e_repr = sci(n, p - 1);
    let exp: i32 = e_repr.split_once('e').map_or(0, |(_, e)| e.parse().unwrap_or(0));
    let mut s = if exp < -4 || exp >= p as i32 {
        format_e(n, p - 1, alt)
    } else {
        fixed(n, (p as i32 - 1 - exp).max(0) as usize)
    };
    if !alt {
        if let Some(epos) = s.find('e') {
            let (m, e) = s.split_at(epos);
            let m = if m.contains('.') { m.trim_end_matches('0').trim_end_matches('.') } else { m };
            s = format!("{m}{e}");
        } else if s.contains('.') {
            s = s.trim_end_matches('0').trim_end_matches('.').to_string();
        }
    }
    s
}

/// Width padding for a numeric conversion: zeros go after the sign (and a
/// `0x` prefix), unless a precision was given for an integer.
fn pad_number(spec: &Spec, s: String, _signed: bool) -> Vec<u8> {
    let zero_ok = spec.prec.is_none();
    let mut sp = spec.clone();
    sp.zero = spec.zero && zero_ok;
    pad(&sp, s.into_bytes(), true)
}

fn pad(spec: &Spec, body: Vec<u8>, numeric: bool) -> Vec<u8> {
    let Some(w) = spec.width else { return body };
    // (a character string's width counts characters)
    let len = if utf_mode() { crate::perl::utf8::count(&body) } else { body.len() };
    if len >= w {
        return body;
    }
    let fill = w - len;
    if spec.left {
        let mut b = body;
        b.extend(std::iter::repeat(b' ').take(fill));
        return b;
    }
    if spec.zero && numeric {
        // Zeros after a sign or a 0x/0b prefix.
        let mut prefix = 0;
        if matches!(body.first(), Some(b'-' | b'+' | b' ')) {
            prefix = 1;
        }
        if body.len() >= prefix + 2 && body[prefix] == b'0' && matches!(body[prefix + 1], b'x' | b'X' | b'b' | b'B') {
            prefix += 2;
        }
        let mut b = body[..prefix].to_vec();
        b.extend(std::iter::repeat(b'0').take(fill));
        b.extend_from_slice(&body[prefix..]);
        return b;
    }
    if spec.zero && !numeric {
        // perl pads strings with zeros too when 0 is given.
        let mut b = vec![b'0'; fill];
        b.extend(body);
        return b;
    }
    let mut b = vec![b' '; fill];
    b.extend(body);
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(fmt: &str, args: &[Value]) -> String {
        String::from_utf8(sprintf(fmt.as_bytes(), args)).unwrap()
    }

    #[test]
    fn integers() {
        assert_eq!(f("%d", &[Value::Num(-3.9)]), "-3");
        assert_eq!(f("%05d", &[Value::Int(-5)]), "-0005");
        assert_eq!(f("%.3d", &[Value::Int(7)]), "007");
        assert_eq!(f("%u", &[Value::Int(-1)]), "18446744073709551615");
        assert_eq!(f("%x %X %#x", &[Value::Int(255), Value::Int(255), Value::Int(255)]), "ff FF 0xff");
        assert_eq!(f("%.4x", &[Value::Int(0x1a)]), "001a");
        assert_eq!(f("%d", &[Value::Num(1e20)]), "-1"); // perl casts through UV
        assert_eq!(f("%+d", &[Value::Int(3)]), "+3");
        assert_eq!(f("%d", &[Value::str("12abc")]), "12");
        assert_eq!(f("%b", &[Value::Int(5)]), "101");
    }

    #[test]
    fn floats() {
        assert_eq!(f("%.2f", &[Value::Num(2.675)]), "2.67"); // binary 2.67499...
        assert_eq!(f("%.1f", &[Value::Num(0.25)]), "0.2");
        assert_eq!(f("%e", &[Value::Num(12345.678)]), "1.234568e+04");
        assert_eq!(f("%g", &[Value::Num(0.0001)]), "0.0001");
        assert_eq!(f("%g", &[Value::Num(0.00001)]), "1e-05");
        assert_eq!(f("%g", &[Value::Num(1234567.0)]), "1.23457e+06");
        assert_eq!(f("%.3g", &[Value::Num(3.14159)]), "3.14");
        assert_eq!(f("%#.3g", &[Value::Num(1.0)]), "1.00");
        assert_eq!(f("%8.3f|%-8.2f|", &[Value::Num(3.14159), Value::Num(2.5)]), "   3.142|2.50    |");
        assert_eq!(f("%f", &[Value::Num(f64::INFINITY)]), "Inf");
        assert_eq!(f("%.0f", &[Value::Num(0.5)]), "0");
        assert_eq!(f("%.0f", &[Value::Num(1.5)]), "2");
    }

    #[test]
    fn strings_and_args() {
        assert_eq!(f("%s-%s", &[Value::str("a")]), "a-");
        assert_eq!(f("%.2s", &[Value::str("abcd")]), "ab");
        assert_eq!(f("%5s|%-5s|", &[Value::str("ab"), Value::str("cd")]), "   ab|cd   |");
        assert_eq!(f("%2$s %1$s", &[Value::str("a"), Value::str("b")]), "b a");
        assert_eq!(f("%*d", &[Value::Int(4), Value::Int(7)]), "   7");
        assert_eq!(f("%vd", &[Value::str("1.2.3")]), "49.46.50.46.51");
        assert_eq!(f("%c%c", &[Value::Int(65), Value::Int(66)]), "AB");
        assert_eq!(f("100%%", &[]), "100%");
        assert_eq!(f("%y", &[Value::Int(1)]), "%y");
    }
}
