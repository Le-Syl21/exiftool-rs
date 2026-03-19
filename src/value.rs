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
                    if *n == 0 { "undef".to_string() } else { "inf".to_string() }
                } else if *n % *d == 0 {
                    (*n / *d).to_string()
                } else {
                    format!("{}/{}", n, d)
                }
            }
            Value::IRational(n, d) => {
                if *d == 0 {
                    if *n >= 0 { "inf".to_string() } else { "-inf".to_string() }
                } else if *n % *d == 0 {
                    (*n / *d).to_string()
                } else {
                    format!("{}/{}", n, d)
                }
            }
            Value::F32(v) => format!("{}", v),
            Value::F64(v) => format!("{}", v),
            Value::Binary(data) => format!("(Binary data {} bytes)", data.len()),
            Value::List(items) => {
                items
                    .iter()
                    .map(|v| v.to_display_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            }
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
