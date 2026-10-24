//! Perl's scalar, which every ExifTool value goes through.
//!
//! A Perl scalar is undef, a string, an integer (IV) or a float (NV), and
//! changes between them on use: `"12 34" + 0` is 12, `0.1 + 0.2` prints as
//! `0.3`, `"0.0"` is true. ExifTool's output depends on all of it, so the
//! conversions here follow perl's own (`sv_2pv`, `sv_2nv`, `grok_number`).
//! Strings are bytes, as in ExifTool, which reads them from files.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

/// A Perl value.
#[derive(Clone, Default)]
pub enum Value {
    #[default]
    Undef,
    /// An integer (perl's IV).
    Int(i64),
    /// An unsigned integer above the IV range (perl's UV).
    UInt(u64),
    /// A float (perl's NV).
    Num(f64),
    /// A string of bytes (perl's PV).
    Str(Rc<[u8]>),
    /// A reference to a scalar: `\$val`, how ExifTool marks binary data.
    ScalarRef(Rc<RefCell<Value>>),
    /// A reference to an array: list-type tags, `\@val`.
    ArrayRef(Rc<RefCell<Vec<Value>>>),
    /// A reference to a hash: Composite dependencies, structures.
    HashRef(Rc<RefCell<Hash>>),
}

/// A Perl hash. Insertion order is kept for reproducibility, but nothing may
/// rely on it: Perl's own order is random.
pub type Hash = indexmap_lite::IndexMap;

impl Value {
    /// A string value from bytes.
    pub fn bytes(b: impl AsRef<[u8]>) -> Value {
        Value::Str(Rc::from(b.as_ref()))
    }

    /// A string value from text.
    pub fn str(s: &str) -> Value {
        Value::bytes(s.as_bytes())
    }

    /// A binary value: a reference to the bytes, as `\$data` in Perl.
    pub fn binary(b: impl AsRef<[u8]>) -> Value {
        Value::ScalarRef(Rc::new(RefCell::new(Value::bytes(b))))
    }

    pub fn is_undef(&self) -> bool {
        matches!(self, Value::Undef)
    }

    pub fn is_ref(&self) -> bool {
        matches!(self, Value::ScalarRef(_) | Value::ArrayRef(_) | Value::HashRef(_))
    }

    /// The string form (perl's `sv_2pv`): undef is empty, integers in decimal,
    /// floats with `%.15g`, references as `SCALAR(0x...)`-like text.
    pub fn to_bytes(&self) -> Rc<[u8]> {
        match self {
            Value::Str(s) => s.clone(),
            _ => Rc::from(self.to_string().into_bytes()),
        }
    }

    /// The string form as text, invalid UTF-8 replaced (for messages only).
    pub fn to_text(&self) -> String {
        match self {
            Value::Str(s) => String::from_utf8_lossy(s).into_owned(),
            _ => self.to_string(),
        }
    }

    /// The numeric form (perl's `sv_2nv` / `sv_2iv`).
    pub fn to_number(&self) -> Number {
        match self {
            Value::Undef => Number::Int(0),
            Value::Int(i) => Number::Int(*i),
            Value::UInt(u) => Number::UInt(*u),
            Value::Num(n) => Number::Float(*n),
            Value::Str(s) => grok_number(s).0,
            // A reference numifies to its address; nothing in ExifTool uses it.
            Value::ScalarRef(r) => Number::Int(Rc::as_ptr(r) as i64),
            Value::ArrayRef(r) => Number::Int(Rc::as_ptr(r) as *const u8 as i64),
            Value::HashRef(r) => Number::Int(Rc::as_ptr(r) as *const u8 as i64),
        }
    }

    pub fn to_f64(&self) -> f64 {
        self.to_number().as_f64()
    }

    /// Perl's integer conversion (`int()`-like truncation for floats, as IV).
    pub fn to_i64(&self) -> i64 {
        self.to_number().as_i64()
    }

    /// Perl's truth: undef, `""`, `"0"` and numeric zero are false; `"0.0"`,
    /// `"00"` and `" 0"` are true.
    pub fn truthy(&self) -> bool {
        match self {
            Value::Undef => false,
            Value::Int(i) => *i != 0,
            Value::UInt(u) => *u != 0,
            Value::Num(n) => *n != 0.0,
            Value::Str(s) => !(s.is_empty() || &s[..] == b"0"),
            _ => true,
        }
    }

    /// Whether the value is a number or a string that looks like one
    /// (perl's `looks_like_number`).
    pub fn looks_like_number(&self) -> bool {
        match self {
            Value::Int(_) | Value::UInt(_) | Value::Num(_) => true,
            Value::Str(s) => grok_number(s).1 == Grok::Whole,
            _ => false,
        }
    }
}

/// A number, integer or float, as perl keeps it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Number {
    Int(i64),
    UInt(u64),
    Float(f64),
}

impl Number {
    pub fn as_f64(self) -> f64 {
        match self {
            Number::Int(i) => i as f64,
            Number::UInt(u) => u as f64,
            Number::Float(f) => f,
        }
    }

    /// Truncation toward zero, saturating, as perl's IV conversion.
    pub fn as_i64(self) -> i64 {
        match self {
            Number::Int(i) => i,
            Number::UInt(u) => u as i64,
            Number::Float(f) if f.is_nan() => 0,
            Number::Float(f) => f as i64,
        }
    }

    pub fn to_value(self) -> Value {
        match self {
            Number::Int(i) => Value::Int(i),
            Number::UInt(u) => Value::UInt(u),
            Number::Float(f) => Value::Num(f),
        }
    }
}

/// How much of a string a number was read from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grok {
    /// The whole string (after leading and trailing whitespace) is a number.
    Whole,
    /// A number followed by other characters (perl warns "isn't numeric").
    Prefix,
    /// No number at all: the value is 0.
    None,
}

/// Read the leading number of a string, as perl's `grok_number` and
/// `Atof`: optional whitespace, sign, digits with an optional fraction and
/// exponent, or `Inf`/`Infinity`/`NaN`. Hexadecimal is not a number here
/// (`"0x10" + 0` is 0).
pub fn grok_number(s: &[u8]) -> (Number, Grok) {
    let mut i = 0;
    while i < s.len() && is_space(s[i]) {
        i += 1;
    }
    let start = i;
    let neg = match s.get(i) {
        Some(b'-') => {
            i += 1;
            true
        }
        Some(b'+') => {
            i += 1;
            false
        }
        _ => false,
    };
    // Inf / Infinity / NaN, case-insensitive.
    let rest = &s[i..];
    let lower: Vec<u8> = rest.iter().take(8).map(u8::to_ascii_lowercase).collect();
    for (word, val) in [(&b"infinity"[..], f64::INFINITY), (b"inf", f64::INFINITY), (b"nan", f64::NAN)] {
        if lower.starts_with(word) {
            let end = i + word.len();
            let v = if neg { -val } else { val };
            return (Number::Float(v), finish(s, end));
        }
    }
    let int_start = i;
    while i < s.len() && s[i].is_ascii_digit() {
        i += 1;
    }
    let int_digits = i - int_start;
    let mut frac_digits = 0;
    let mut is_float = false;
    if i < s.len() && s[i] == b'.' {
        let dot = i;
        i += 1;
        while i < s.len() && s[i].is_ascii_digit() {
            i += 1;
            frac_digits += 1;
        }
        if int_digits == 0 && frac_digits == 0 {
            i = dot;
        } else {
            is_float = true;
        }
    }
    if int_digits == 0 && frac_digits == 0 {
        return (Number::Int(0), Grok::None);
    }
    if i < s.len() && (s[i] == b'e' || s[i] == b'E') {
        let mut j = i + 1;
        if j < s.len() && (s[j] == b'+' || s[j] == b'-') {
            j += 1;
        }
        let exp_start = j;
        while j < s.len() && s[j].is_ascii_digit() {
            j += 1;
        }
        if j > exp_start {
            i = j;
            is_float = true;
        }
    }
    let text = std::str::from_utf8(&s[start..i]).unwrap_or("0");
    let grok = finish(s, i);
    if !is_float {
        // An integer that fits an IV stays an integer; a larger one is an NV.
        // "-0" is perl's negative zero NV.
        if let Ok(v) = text.parse::<i64>() {
            if v == 0 && neg {
                return (Number::Float(-0.0), grok);
            }
            return (Number::Int(v), grok);
        }
        if !neg {
            if let Ok(v) = text.trim_start_matches('+').parse::<u64>() {
                return (Number::UInt(v), grok);
            }
        }
    }
    (Number::Float(text.parse::<f64>().unwrap_or(0.0)), grok)
}

fn finish(s: &[u8], mut i: usize) -> Grok {
    while i < s.len() && is_space(s[i]) {
        i += 1;
    }
    if i == s.len() {
        Grok::Whole
    } else {
        Grok::Prefix
    }
}

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// Perl's stringification of a float: `%.15g`, with `Inf`, `-Inf`, `NaN`.
pub fn format_nv(n: f64) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Inf".into() } else { "-Inf".into() };
    }
    if n == 0.0 {
        // perl prints -0.0 as "-0" on some builds and "0" on others; Linux
        // perl 5.38 prints "-0" for a computed negative zero but ExifTool
        // never prints one, so keep "0".
        return "0".into();
    }
    crate::perl::sprintf::format_g(n, 15, false, false)
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Undef => Ok(()),
            Value::Int(i) => write!(f, "{i}"),
            Value::UInt(u) => write!(f, "{u}"),
            Value::Num(n) => f.write_str(&format_nv(*n)),
            Value::Str(s) => f.write_str(&String::from_utf8_lossy(s)),
            Value::ScalarRef(r) => write!(f, "SCALAR({:p})", Rc::as_ptr(r)),
            Value::ArrayRef(r) => write!(f, "ARRAY({:p})", Rc::as_ptr(r)),
            Value::HashRef(r) => write!(f, "HASH({:p})", Rc::as_ptr(r)),
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Undef => f.write_str("undef"),
            Value::Int(i) => write!(f, "{i}"),
            Value::UInt(u) => write!(f, "{u}"),
            Value::Num(n) => write!(f, "{}", format_nv(*n)),
            Value::Str(s) => write!(f, "{:?}", String::from_utf8_lossy(s)),
            Value::ScalarRef(r) => write!(f, "\\{:?}", r.borrow()),
            Value::ArrayRef(r) => write!(f, "{:?}", r.borrow()),
            Value::HashRef(r) => write!(f, "{:?}", r.borrow()),
        }
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Value {
        Value::str(s)
    }
}

impl From<String> for Value {
    fn from(s: String) -> Value {
        Value::bytes(s.into_bytes())
    }
}

impl From<i64> for Value {
    fn from(i: i64) -> Value {
        Value::Int(i)
    }
}

impl From<f64> for Value {
    fn from(n: f64) -> Value {
        Value::Num(n)
    }
}

/// A small insertion-ordered map, enough for Perl hashes without pulling a
/// crate in.
pub mod indexmap_lite {
    use super::Value;
    use std::collections::HashMap;
    use std::rc::Rc;

    #[derive(Default, Clone, Debug)]
    pub struct IndexMap {
        keys: Vec<Rc<[u8]>>,
        index: HashMap<Rc<[u8]>, usize>,
        values: Vec<Value>,
    }

    impl IndexMap {
        pub fn get(&self, k: &[u8]) -> Option<&Value> {
            self.index.get(k).map(|&i| &self.values[i])
        }

        pub fn insert(&mut self, k: &[u8], v: Value) {
            if let Some(&i) = self.index.get(k) {
                self.values[i] = v;
            } else {
                let key: Rc<[u8]> = Rc::from(k);
                self.index.insert(key.clone(), self.values.len());
                self.keys.push(key);
                self.values.push(v);
            }
        }

        pub fn contains(&self, k: &[u8]) -> bool {
            self.index.contains_key(k)
        }

        pub fn remove(&mut self, k: &[u8]) -> Option<Value> {
            let i = self.index.remove(k)?;
            self.keys.remove(i);
            let v = self.values.remove(i);
            for (j, key) in self.keys.iter().enumerate().skip(i) {
                self.index.insert(key.clone(), j);
            }
            Some(v)
        }

        pub fn keys(&self) -> impl Iterator<Item = &Rc<[u8]>> {
            self.keys.iter()
        }

        pub fn iter(&self) -> impl Iterator<Item = (&Rc<[u8]>, &Value)> {
            self.keys.iter().zip(self.values.iter())
        }

        pub fn len(&self) -> usize {
            self.keys.len()
        }

        pub fn is_empty(&self) -> bool {
            self.keys.is_empty()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_from_strings() {
        assert_eq!(Value::str("12 34").to_number(), Number::Int(12));
        assert_eq!(Value::str("  3.5e2xyz").to_number(), Number::Float(350.0));
        assert_eq!(Value::str("0x10").to_number(), Number::Int(0));
        assert_eq!(Value::str("abc").to_number(), Number::Int(0));
        assert_eq!(Value::str("-inf").to_f64(), f64::NEG_INFINITY);
        assert_eq!(Value::str(".5").to_number(), Number::Float(0.5));
        assert_eq!(Value::str("99999999999999999999").to_number(), Number::Float(1e20));
    }

    #[test]
    fn truth() {
        for s in ["", "0"] {
            assert!(!Value::str(s).truthy(), "{s:?}");
        }
        for s in ["0.0", "00", " 0", "0E0", "a"] {
            assert!(Value::str(s).truthy(), "{s:?}");
        }
        assert!(!Value::Num(0.0).truthy());
    }

    #[test]
    fn float_strings() {
        assert_eq!(Value::Num(0.1 + 0.2).to_string(), "0.3");
        assert_eq!(Value::Num(1e15).to_string(), "1e+15");
        assert_eq!(Value::Num(123456789012345.0).to_string(), "123456789012345");
        assert_eq!(Value::Num(0.0001).to_string(), "0.0001");
        assert_eq!(Value::Num(0.00001).to_string(), "1e-05");
        assert_eq!(Value::Num(5.0).to_string(), "5");
        assert_eq!(Value::Num(-2.5).to_string(), "-2.5");
    }
}
