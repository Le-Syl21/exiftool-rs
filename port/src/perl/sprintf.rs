//! Perl's `sprintf`, as `sv_vcatpvfn` implements it.
//!
//! ExifTool formats about 1,250 values with `sprintf`, and its output is the
//! test's expected output, so the details matter: `%d` truncates a float,
//! `%u` and `%x` read a negative number as unsigned, `%g` follows C, a
//! missing argument formats as undef, `%v` joins the ordinals of a string.

use crate::perl::value::{Number, Value};

/// `sprintf($fmt, @args)`.
pub fn sprintf(fmt: &[u8], args: &[Value]) -> Vec<u8> {
    let mut out = Vec::with_capacity(fmt.len() + 16);
    let mut next_arg = 0usize;
    let mut i = 0;
    let take = |next: &mut usize, explicit: Option<usize>| -> Value {
        let idx = match explicit {
            Some(n) => n.saturating_sub(1),
            None => {
                let n = *next;
                *next += 1;
                n
            }
        };
        args.get(idx).cloned().unwrap_or(Value::Undef)
    };
    while i < fmt.len() {
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
            let w = take(&mut next_arg, None).to_i64();
            if w < 0 {
                spec.left = true;
            }
            spec.width = Some(w.unsigned_abs() as usize);
        } else {
            let s = i;
            while i < fmt.len() && fmt[i].is_ascii_digit() {
                i += 1;
            }
            if i > s {
                spec.width = std::str::from_utf8(&fmt[s..i]).ok().and_then(|t| t.parse().ok());
            }
        }
        // Precision.
        if i < fmt.len() && fmt[i] == b'.' {
            i += 1;
            if i < fmt.len() && fmt[i] == b'*' {
                i += 1;
                let p = take(&mut next_arg, None).to_i64();
                spec.prec = if p < 0 { None } else { Some(p as usize) };
            } else {
                let s = i;
                while i < fmt.len() && fmt[i].is_ascii_digit() {
                    i += 1;
                }
                spec.prec = Some(
                    std::str::from_utf8(&fmt[s..i]).ok().and_then(|t| t.parse().ok()).unwrap_or(0),
                );
            }
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
            let bytes = v.to_bytes();
            let parts: Vec<Vec<u8>> = bytes
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
        match format_one(conv, &spec, &v) {
            Some(s) => out.extend(s),
            None => out.extend_from_slice(&fmt[spec_start..i]),
        }
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
            if ch < 256 {
                vec![ch as u8]
            } else {
                char::from_u32(ch).unwrap_or('\u{FFFD}').to_string().into_bytes()
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

fn float_string(conv: u8, n: f64, spec: &Spec) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Inf".into() } else { "-Inf".into() };
    }
    let upper = conv.is_ascii_uppercase();
    let s = match conv.to_ascii_lowercase() {
        b'f' => format!("{:.*}", spec.prec.unwrap_or(6), n),
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
    let s = format!("{:.*e}", prec, n);
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
            format!("{:.*}", p - 1, n)
        } else if n.is_sign_negative() {
            "-0".into()
        } else {
            "0".into()
        };
    }
    // The exponent after rounding to P significant digits.
    let e_repr = format!("{:.*e}", p - 1, n);
    let exp: i32 = e_repr.split_once('e').map_or(0, |(_, e)| e.parse().unwrap_or(0));
    let mut s = if exp < -4 || exp >= p as i32 {
        format_e(n, p - 1, alt)
    } else {
        format!("{:.*}", (p as i32 - 1 - exp).max(0) as usize, n)
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
    if body.len() >= w {
        return body;
    }
    let fill = w - body.len();
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
