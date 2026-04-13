use std::fmt;

/// Represents a metadata tag value, which can be of various types.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// ASCII/UTF-8 string
    String(String),
    /// Unsigned 8-bit integer
    U8(u8),
    /// Unsigned 16-bit integer
    U16(u16),
    /// Unsigned 32-bit integer
    U32(u32),
    /// Signed 16-bit integer
    I16(i16),
    /// Signed 32-bit integer
    I32(i32),
    /// Unsigned rational (numerator/denominator)
    URational(u32, u32),
    /// Signed rational (numerator/denominator)
    IRational(i32, i32),
    /// 32-bit float
    F32(f32),
    /// 64-bit float
    F64(f64),
    /// Raw binary data
    Binary(Vec<u8>),
    /// A list of values (e.g., GPS coordinates, color space arrays)
    List(Vec<Value>),
    /// Undefined/opaque bytes with a semantic type hint
    Undefined(Vec<u8>),
}

impl Value {
    /// Convert to string representation (PrintConv equivalent).
    pub fn to_display_string(&self) -> String {
        match self {
            Value::String(s) => s.clone(),
            Value::U8(v) => v.to_string(),
            Value::U16(v) => v.to_string(),
            Value::U32(v) => v.to_string(),
            Value::I16(v) => v.to_string(),
            Value::I32(v) => v.to_string(),
            Value::URational(n, d) => {
                if *d == 0 {
                    if *n == 0 {
                        "undef".to_string()
                    } else {
                        "inf".to_string()
                    }
                } else if *n % *d == 0 {
                    (*n / *d).to_string()
                } else {
                    format!("{}/{}", n, d)
                }
            }
            Value::IRational(n, d) => {
                if *d == 0 {
                    if *n >= 0 {
                        "inf".to_string()
                    } else {
                        "-inf".to_string()
                    }
                } else if *n % *d == 0 {
                    (*n / *d).to_string()
                } else {
                    format!("{}/{}", n, d)
                }
            }
            Value::F32(v) => format!("{}", v),
            Value::F64(v) => format!("{}", v),
            Value::Binary(data) => format!("(Binary data {} bytes)", data.len()),
            Value::List(items) => items
                .iter()
                .map(|v| v.to_display_string())
                .collect::<Vec<_>>()
                .join(", "),
            Value::Undefined(data) => format!("(Undefined {} bytes)", data.len()),
        }
    }

    /// Try to interpret the value as a float.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::U8(v) => Some(*v as f64),
            Value::U16(v) => Some(*v as f64),
            Value::U32(v) => Some(*v as f64),
            Value::I16(v) => Some(*v as f64),
            Value::I32(v) => Some(*v as f64),
            Value::F32(v) => Some(*v as f64),
            Value::F64(v) => Some(*v),
            Value::URational(n, d) if *d != 0 => Some(*n as f64 / *d as f64),
            Value::IRational(n, d) if *d != 0 => Some(*n as f64 / *d as f64),
            _ => None,
        }
    }

    /// Try to interpret the value as a string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    /// Try to interpret the value as an unsigned integer.
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::U8(v) => Some(*v as u64),
            Value::U16(v) => Some(*v as u64),
            Value::U32(v) => Some(*v as u64),
            _ => None,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_display_string())
    }
}

/// Format a float with Perl-style %.15g precision (15 significant digits, trailing zeros stripped).
/// This matches ExifTool's default `%s` formatting for floating-point values.
pub fn format_g15(v: f64) -> String {
    format_g_prec(v, 15)
}

/// Format a float with Perl-style %.Ng precision (N significant digits, trailing zeros stripped).
/// Mirrors C sprintf's %g: uses exponential if exponent < -4 or >= precision.
pub fn format_g_prec(v: f64, prec: usize) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    let abs_v = v.abs();
    let exp = abs_v.log10().floor() as i32;
    if exp >= -4 && exp < prec as i32 {
        // Fixed-point: need (prec-1 - exp) decimal places
        let decimal_places = ((prec as i32 - 1 - exp).max(0)) as usize;
        let s = format!("{:.prec$}", v, prec = decimal_places);
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s
        }
    } else {
        // Exponential format: prec-1 decimal places
        let decimal_places = prec - 1;
        let s = format!("{:.prec$e}", v, prec = decimal_places);
        // Rust produces e.g. "3.51360899930879e20", need "3.51360899930879e+20"
        // and "-1.5e-6" → "-1.5e-06" (at least 2 digits in exponent)
        // First strip trailing zeros from mantissa
        let (mantissa_part, exp_part) = if let Some(e_pos) = s.find('e') {
            (&s[..e_pos], &s[e_pos + 1..])
        } else {
            return s;
        };
        let mantissa_trimmed = if mantissa_part.contains('.') {
            mantissa_part.trim_end_matches('0').trim_end_matches('.')
        } else {
            mantissa_part
        };
        // Parse exponent and reformat with sign and minimum 2 digits
        let exp_val: i32 = exp_part.parse().unwrap_or(0);
        let exp_str = if exp_val >= 0 {
            format!("e+{:02}", exp_val)
        } else {
            format!("e-{:02}", -exp_val)
        };
        format!("{}{}", mantissa_trimmed, exp_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── to_display_string ──────────────────────────────────────────

    #[test]
    fn display_string() {
        assert_eq!(Value::String("hello".into()).to_display_string(), "hello");
    }

    #[test]
    fn display_u8() {
        assert_eq!(Value::U8(42).to_display_string(), "42");
    }

    #[test]
    fn display_u16() {
        assert_eq!(Value::U16(1024).to_display_string(), "1024");
    }

    #[test]
    fn display_u32() {
        assert_eq!(Value::U32(100_000).to_display_string(), "100000");
    }

    #[test]
    fn display_i16() {
        assert_eq!(Value::I16(-123).to_display_string(), "-123");
    }

    #[test]
    fn display_i32() {
        assert_eq!(Value::I32(-999_999).to_display_string(), "-999999");
    }

    #[test]
    fn display_urational_exact_division() {
        assert_eq!(Value::URational(100, 10).to_display_string(), "10");
    }

    #[test]
    fn display_urational_non_exact() {
        assert_eq!(Value::URational(1, 3).to_display_string(), "1/3");
    }

    #[test]
    fn display_urational_zero_zero() {
        assert_eq!(Value::URational(0, 0).to_display_string(), "undef");
    }

    #[test]
    fn display_urational_n_over_zero() {
        assert_eq!(Value::URational(5, 0).to_display_string(), "inf");
    }

    #[test]
    fn display_irational_exact() {
        assert_eq!(Value::IRational(-10, 5).to_display_string(), "-2");
    }

    #[test]
    fn display_irational_non_exact() {
        assert_eq!(Value::IRational(7, 3).to_display_string(), "7/3");
    }

    #[test]
    fn display_irational_positive_inf() {
        assert_eq!(Value::IRational(1, 0).to_display_string(), "inf");
    }

    #[test]
    fn display_irational_zero_inf() {
        // n=0 d=0 → n >= 0, so "inf"
        assert_eq!(Value::IRational(0, 0).to_display_string(), "inf");
    }

    #[test]
    fn display_irational_negative_inf() {
        assert_eq!(Value::IRational(-3, 0).to_display_string(), "-inf");
    }

    #[test]
    fn display_f32() {
        let s = Value::F32(3.14).to_display_string();
        assert!(s.starts_with("3.14"), "got: {}", s);
    }

    #[test]
    fn display_f64() {
        assert_eq!(Value::F64(2.5).to_display_string(), "2.5");
    }

    #[test]
    fn display_binary() {
        assert_eq!(
            Value::Binary(vec![0, 1, 2]).to_display_string(),
            "(Binary data 3 bytes)"
        );
    }

    #[test]
    fn display_list() {
        let list = Value::List(vec![Value::U16(640), Value::U16(480)]);
        assert_eq!(list.to_display_string(), "640, 480");
    }

    #[test]
    fn display_undefined() {
        assert_eq!(
            Value::Undefined(vec![0xAB; 5]).to_display_string(),
            "(Undefined 5 bytes)"
        );
    }

    // ── as_f64 ─────────────────────────────────────────────────────

    #[test]
    fn as_f64_u8() {
        assert_eq!(Value::U8(10).as_f64(), Some(10.0));
    }

    #[test]
    fn as_f64_u16() {
        assert_eq!(Value::U16(300).as_f64(), Some(300.0));
    }

    #[test]
    fn as_f64_u32() {
        assert_eq!(Value::U32(70_000).as_f64(), Some(70_000.0));
    }

    #[test]
    fn as_f64_i16() {
        assert_eq!(Value::I16(-50).as_f64(), Some(-50.0));
    }

    #[test]
    fn as_f64_i32() {
        assert_eq!(Value::I32(-1_000_000).as_f64(), Some(-1_000_000.0));
    }

    #[test]
    fn as_f64_f32() {
        let val = Value::F32(1.5).as_f64().unwrap();
        assert!((val - 1.5).abs() < 1e-6);
    }

    #[test]
    fn as_f64_f64() {
        assert_eq!(Value::F64(9.99).as_f64(), Some(9.99));
    }

    #[test]
    fn as_f64_urational() {
        let val = Value::URational(1, 4).as_f64().unwrap();
        assert!((val - 0.25).abs() < 1e-10);
    }

    #[test]
    fn as_f64_urational_zero_denom() {
        assert_eq!(Value::URational(5, 0).as_f64(), None);
    }

    #[test]
    fn as_f64_irational() {
        let val = Value::IRational(-3, 2).as_f64().unwrap();
        assert!((val - -1.5).abs() < 1e-10);
    }

    #[test]
    fn as_f64_irational_zero_denom() {
        assert_eq!(Value::IRational(-1, 0).as_f64(), None);
    }

    #[test]
    fn as_f64_string_none() {
        assert_eq!(Value::String("hi".into()).as_f64(), None);
    }

    #[test]
    fn as_f64_binary_none() {
        assert_eq!(Value::Binary(vec![1]).as_f64(), None);
    }

    #[test]
    fn as_f64_undefined_none() {
        assert_eq!(Value::Undefined(vec![1]).as_f64(), None);
    }

    // ── as_str ─────────────────────────────────────────────────────

    #[test]
    fn as_str_string() {
        assert_eq!(Value::String("test".into()).as_str(), Some("test"));
    }

    #[test]
    fn as_str_non_string() {
        assert_eq!(Value::U8(1).as_str(), None);
        assert_eq!(Value::Binary(vec![]).as_str(), None);
        assert_eq!(Value::F64(1.0).as_str(), None);
    }

    // ── as_u64 ─────────────────────────────────────────────────────

    #[test]
    fn as_u64_unsigned_types() {
        assert_eq!(Value::U8(255).as_u64(), Some(255));
        assert_eq!(Value::U16(65535).as_u64(), Some(65535));
        assert_eq!(Value::U32(0xFFFFFFFF).as_u64(), Some(0xFFFFFFFF));
    }

    #[test]
    fn as_u64_signed_none() {
        assert_eq!(Value::I16(1).as_u64(), None);
        assert_eq!(Value::I32(1).as_u64(), None);
    }

    #[test]
    fn as_u64_other_none() {
        assert_eq!(Value::String("42".into()).as_u64(), None);
        assert_eq!(Value::F64(1.0).as_u64(), None);
        assert_eq!(Value::Binary(vec![]).as_u64(), None);
        assert_eq!(Value::Undefined(vec![]).as_u64(), None);
    }

    // ── Display trait ──────────────────────────────────────────────

    #[test]
    fn display_trait_delegates() {
        let v = Value::URational(1, 3);
        assert_eq!(format!("{}", v), "1/3");
    }

    // ── format_g15 / format_g_prec ─────────────────────────────────

    #[test]
    fn format_g15_zero() {
        assert_eq!(format_g15(0.0), "0");
    }

    #[test]
    fn format_g15_integer() {
        assert_eq!(format_g15(42.0), "42");
    }

    #[test]
    fn format_g15_simple_decimal() {
        assert_eq!(format_g15(3.5), "3.5");
    }

    #[test]
    fn format_g15_negative() {
        assert_eq!(format_g15(-1.25), "-1.25");
    }

    #[test]
    fn format_g15_large_value_scientific() {
        // 1e+20 should use exponential
        let s = format_g15(1e20);
        assert!(s.contains("e+"), "expected scientific notation, got: {}", s);
    }

    #[test]
    fn format_g15_small_value_scientific() {
        // 1e-5 is < 1e-4, should use exponential
        let s = format_g15(1e-5);
        assert!(s.contains("e-"), "expected scientific notation, got: {}", s);
    }

    #[test]
    fn format_g15_borderline_fixed() {
        // 0.0001 = 1e-4 → exp = -4, which is >= -4, so fixed format
        let s = format_g15(0.0001);
        assert_eq!(s, "0.0001");
    }

    #[test]
    fn format_g_prec_low_precision() {
        // 3 significant digits for pi
        let s = format_g_prec(std::f64::consts::PI, 3);
        assert_eq!(s, "3.14");
    }

    #[test]
    fn format_g_prec_one_digit() {
        let s = format_g_prec(7.7, 1);
        assert_eq!(s, "8");
    }

    #[test]
    fn format_g15_trailing_zeros_stripped() {
        // 1.5 should not produce "1.500000..."
        let s = format_g15(1.5);
        assert!(!s.ends_with('0'), "trailing zeros not stripped: {}", s);
    }

    #[test]
    fn format_g15_very_large() {
        let s = format_g15(1.23456789e+100);
        assert!(s.starts_with("1.23456789"), "got: {}", s);
        assert!(s.contains("e+100"), "got: {}", s);
    }

    #[test]
    fn format_g15_very_small_negative() {
        let s = format_g15(-5.5e-10);
        assert!(s.starts_with("-5.5e-"), "got: {}", s);
    }
}
