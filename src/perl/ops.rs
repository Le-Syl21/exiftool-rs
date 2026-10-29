//! Perl's arithmetic and comparison operators.
//!
//! Perl keeps integers exact where it can: `pp_add`, `pp_subtract` and
//! `pp_multiply` use integer arithmetic when both operands are integers, or
//! floats with an integral value in the IV range, and fall back to floats on
//! overflow. That is why `0 + "1e15"` prints `1000000000000000` while the
//! constant `1e15` prints `1e+15`.

use crate::perl::value::{Number, Value};

/// An operand as perl's arithmetic sees it: an integer when it can be one.
fn iv_able(n: Number) -> Option<i64> {
    match n {
        Number::Int(i) => Some(i),
        Number::UInt(_) => None,
        Number::Float(f) => {
            if f.fract() == 0.0 && f >= -9.223372036854776e18 && f < 9.223372036854776e18 {
                Some(f as i64)
            } else {
                None
            }
        }
    }
}

fn numeric(v: &Value) -> Number {
    v.to_number()
}

/// An operand as perl's integer arithmetic sees it (IV, UV, or a float
/// with an integral value in their range), widened to i128.
fn int_able(n: Number) -> Option<i128> {
    match n {
        Number::Int(i) => Some(i as i128),
        Number::UInt(u) => Some(u as i128),
        Number::Float(f) => {
            if f.fract() == 0.0 && f >= -9.223372036854776e18 && f < 1.8446744073709552e19 {
                Some(f as i128)
            } else {
                None
            }
        }
    }
}

/// An integer result: IV when it fits, UV when positive and it fits, NV
/// otherwise (perl's pp_add and friends).
fn from_i128(r: i128) -> Value {
    if r >= i64::MIN as i128 && r <= i64::MAX as i128 {
        Value::Int(r as i64)
    } else if r >= 0 && r <= u64::MAX as i128 {
        Value::UInt(r as u64)
    } else {
        Value::Num(r as f64)
    }
}

fn int_op(a: &Value, b: &Value, op: fn(i128, i128) -> Option<i128>, float: fn(f64, f64) -> f64) -> Value {
    let (x, y) = (numeric(a), numeric(b));
    // An undefined operand is not integer-valued for perl (SvIV_please
    // fails): the float path runs.
    if a.is_undef() || b.is_undef() {
        return Value::Num(float(x.as_f64(), y.as_f64()));
    }
    if let (Some(i), Some(j)) = (int_able(x), int_able(y)) {
        if let Some(r) = op(i, j) {
            return from_i128(r);
        }
    }
    Value::Num(float(x.as_f64(), y.as_f64()))
}

pub fn add(a: &Value, b: &Value) -> Value {
    int_op(a, b, |i, j| i.checked_add(j), |x, y| x + y)
}

pub fn sub(a: &Value, b: &Value) -> Value {
    int_op(a, b, |i, j| i.checked_sub(j), |x, y| x - y)
}

pub fn mul(a: &Value, b: &Value) -> Value {
    int_op(a, b, |i, j| i.checked_mul(j), |x, y| x * y)
}

/// `/`: always a float division, but an integral result is kept exact when
/// both operands are integers and divide evenly (perl's `pp_divide`).
/// Division by zero is perl's fatal "Illegal division by zero".
pub fn div(a: &Value, b: &Value) -> Result<Value, String> {
    let (x, y) = (numeric(a), numeric(b));
    if y.as_f64() == 0.0 {
        return Err("Illegal division by zero".into());
    }
    // perl tries an exact integer division first on 64-bit builds
    // (PERL_TRY_UV_DIVIDE), so large integers keep every digit.
    if !a.is_undef() && !b.is_undef() {
        if let (Some(i), Some(j)) = (int_able(x), int_able(y)) {
            if j != 0 && i % j == 0 {
                return Ok(from_i128(i / j));
            }
        }
    }
    Ok(Value::Num(x.as_f64() / y.as_f64()))
}

/// `%`: integer modulus with the sign of the right operand, as perl's
/// `pp_modulo` computes it: on the magnitudes as UVs (operands truncated),
/// or with fmod when one of them is past the UV range (an infinity, 1e30;
/// the right one then rounded, as perl does).
pub fn modulo(a: &Value, b: &Value) -> Result<Value, String> {
    const UV_MAX_P1: f64 = 18446744073709551616.0;
    let (x, y) = (numeric(a), numeric(b));
    let mut use_double = false;
    let (mut right, mut dright, mut dright_valid) = (0u64, 0f64, false);
    let right_neg = match y {
        Number::Int(j) => {
            right = j.unsigned_abs();
            j < 0
        }
        Number::UInt(u) => {
            right = u;
            false
        }
        Number::Float(f) => {
            dright = f.abs();
            if dright < UV_MAX_P1 {
                right = dright as u64;
                dright_valid = true;
            } else {
                use_double = true;
            }
            f < 0.0
        }
    };
    let (mut left, mut dleft) = (0u64, 0f64);
    let left_neg = match x {
        Number::Int(i) if !use_double => {
            left = i.unsigned_abs();
            i < 0
        }
        Number::UInt(u) if !use_double => {
            left = u;
            false
        }
        _ => {
            let f = x.as_f64();
            dleft = f.abs();
            if !use_double {
                if dleft < UV_MAX_P1 {
                    left = dleft as u64;
                } else {
                    use_double = true;
                    dright = if dright_valid { (dright + 0.5).floor() } else { right as f64 };
                }
            }
            f < 0.0
        }
    };
    if use_double {
        if dright == 0.0 {
            return Err("Illegal modulus zero".into());
        }
        let mut ans = dleft % dright;
        if left_neg != right_neg && ans != 0.0 {
            ans = dright - ans;
        }
        return Ok(Value::Num(if right_neg { -ans } else { ans }));
    }
    if right == 0 {
        return Err("Illegal modulus zero".into());
    }
    let mut ans = left % right;
    if left_neg != right_neg && ans != 0 {
        ans = right - ans;
    }
    Ok(if !right_neg {
        from_uv(ans)
    } else if ans <= 1 << 63 {
        Value::Int((!ans).wrapping_add(1) as i64)
    } else {
        Value::Num(-(ans as f64))
    })
}

/// `**`: integer when both are integers and the result is exact.
pub fn pow(a: &Value, b: &Value) -> Value {
    let (x, y) = (numeric(a), numeric(b));
    if let (Some(i), Some(j)) = (iv_able(x), iv_able(y)) {
        if (0..64).contains(&j) {
            if let Some(r) = i.checked_pow(j as u32) {
                return Value::Int(r);
            }
        }
    }
    Value::Num(x.as_f64().powf(y.as_f64()))
}

/// Unary minus as perl does it on any value (S_negate_string): -"foo" is
/// "-foo", -"+x" is "-x", -"-foo" is "+foo" when it is not a number.
pub fn negate(v: &Value) -> Value {
    if let Value::Str(s) | Value::UStr(s) = v {
        let utf8 = v.is_utf8();
        if !s.is_empty() {
            if s[0].is_ascii_alphabetic() || s[0] == b'_' {
                let mut o = b"-".to_vec();
                o.extend_from_slice(s);
                return Value::string(o, utf8);
            }
            if s[0] == b'+' || (s[0] == b'-' && !v.looks_like_number()) {
                let mut o = s.to_vec();
                o[0] = if s[0] == b'-' { b'+' } else { b'-' };
                return Value::string(o, utf8);
            }
        }
    }
    neg(v)
}

pub fn neg(a: &Value) -> Value {
    // perl's unary minus on a string that does not look like a number
    // prefixes "-" (`-"foo"` is "-foo"); ExifTool only negates numbers.
    match numeric(a) {
        Number::Int(i) if i != i64::MIN => Value::Int(-i),
        n => Value::Num(-n.as_f64()),
    }
}

/// `<=>`: -1, 0, 1, or undef when either side is NaN.
pub fn num_cmp(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    let (x, y) = (numeric(a), numeric(b));
    match (x, y) {
        (Number::Int(i), Number::Int(j)) => Some(i.cmp(&j)),
        (Number::UInt(i), Number::UInt(j)) => Some(i.cmp(&j)),
        (Number::UInt(_), Number::Int(_)) => Some(std::cmp::Ordering::Greater),
        (Number::Int(_), Number::UInt(_)) => Some(std::cmp::Ordering::Less),
        _ => x.as_f64().partial_cmp(&y.as_f64()),
    }
}

/// `cmp`: byte-wise string comparison.
/// `$a . $b`: a character string when either is one (the other upgraded).
pub fn concat(a: &Value, b: &Value) -> Value {
    if a.is_utf8() || b.is_utf8() {
        let mut s = a.utf8_bytes().to_vec();
        s.extend_from_slice(&b.utf8_bytes());
        return Value::chars(s);
    }
    let mut s = a.to_bytes().to_vec();
    s.extend_from_slice(&b.to_bytes());
    Value::bytes(s)
}

/// `$a x $n` for a scalar.
pub fn repeat(a: &Value, n: usize) -> Value {
    match a {
        Value::UStr(s) => Value::chars(s.repeat(n)),
        _ => Value::bytes(a.to_bytes().repeat(n)),
    }
}

pub fn str_cmp(a: &Value, b: &Value) -> std::cmp::Ordering {
    // a character string against a byte string: the bytes upgraded (the
    // UTF-8 forms order as the code points do)
    if a.is_utf8() || b.is_utf8() {
        return a.utf8_bytes()[..].cmp(&b.utf8_bytes()[..]);
    }
    a.to_bytes()[..].cmp(&b.to_bytes()[..])
}

/// Integer bitwise operators: operands as unsigned 64-bit integers (perl
/// without `use integer`).
pub fn to_uv(v: &Value) -> u64 {
    match numeric(v) {
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
    }
}

/// A UV result as perl stores it: an IV when it fits, otherwise a UV.
pub fn from_uv(u: u64) -> Value {
    if u <= i64::MAX as u64 {
        Value::Int(u as i64)
    } else {
        Value::UInt(u)
    }
}

pub fn bit_and(a: &Value, b: &Value) -> Value {
    from_uv(to_uv(a) & to_uv(b))
}

pub fn bit_or(a: &Value, b: &Value) -> Value {
    from_uv(to_uv(a) | to_uv(b))
}

pub fn bit_xor(a: &Value, b: &Value) -> Value {
    from_uv(to_uv(a) ^ to_uv(b))
}

pub fn shift_left(a: &Value, b: &Value) -> Value {
    let n = b.to_i64();
    if !(0..64).contains(&n) {
        return Value::Int(0);
    }
    from_uv(to_uv(a) << n)
}

pub fn shift_right(a: &Value, b: &Value) -> Value {
    let n = b.to_i64();
    if !(0..64).contains(&n) {
        return Value::Int(0);
    }
    from_uv(to_uv(a) >> n)
}

pub fn bit_not(a: &Value) -> Value {
    from_uv(!to_uv(a))
}

/// `int()`: truncation toward zero, keeping integers.
pub fn int(a: &Value) -> Value {
    match numeric(a) {
        Number::Int(i) => Value::Int(i),
        Number::UInt(u) => Value::UInt(u),
        Number::Float(f) if f.is_finite() => {
            let t = f.trunc();
            match iv_able(Number::Float(t)) {
                Some(i) => Value::Int(i),
                None => Value::Num(t),
            }
        }
        Number::Float(f) => Value::Num(f),
    }
}

pub fn abs(a: &Value) -> Value {
    match numeric(a) {
        Number::UInt(u) => Value::UInt(u),
        Number::Int(i) if i != i64::MIN => Value::Int(i.abs()),
        n => Value::Num(n.as_f64().abs()),
    }
}
