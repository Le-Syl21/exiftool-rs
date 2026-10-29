//! Perl's character strings: a string with the UTF-8 flag on holds
//! characters, stored as perl stores them, in (perl's extended) UTF-8.
//!
//! A byte string is a string of characters 0-255; joining it to a character
//! string upgrades it (each byte becomes the character of that code, the
//! Latin-1 reading), as perl does. These helpers convert between the two
//! forms and walk the characters of the UTF-8 form.

use crate::perl::Value;

/// Append the UTF-8 form of code point `cp` (perl's `uvchr_to_utf8`, which
/// also encodes surrogates and code points above 0x10FFFF).
pub fn push_cp(out: &mut Vec<u8>, cp: u32) {
    if cp < 0x80 {
        out.push(cp as u8);
    } else if cp < 0x800 {
        out.extend_from_slice(&[0xC0 | (cp >> 6) as u8, 0x80 | (cp & 0x3F) as u8]);
    } else if cp < 0x10000 {
        out.extend_from_slice(&[0xE0 | (cp >> 12) as u8, 0x80 | ((cp >> 6) & 0x3F) as u8, 0x80 | (cp & 0x3F) as u8]);
    } else if cp < 0x200000 {
        out.extend_from_slice(&[0xF0 | (cp >> 18) as u8, 0x80 | ((cp >> 12) & 0x3F) as u8, 0x80 | ((cp >> 6) & 0x3F) as u8, 0x80 | (cp & 0x3F) as u8]);
    } else if cp < 0x4000000 {
        out.extend_from_slice(&[0xF8 | (cp >> 24) as u8, 0x80 | ((cp >> 18) & 0x3F) as u8, 0x80 | ((cp >> 12) & 0x3F) as u8, 0x80 | ((cp >> 6) & 0x3F) as u8, 0x80 | (cp & 0x3F) as u8]);
    } else {
        out.extend_from_slice(&[0xFC | ((cp >> 30) & 1) as u8, 0x80 | ((cp >> 24) & 0x3F) as u8, 0x80 | ((cp >> 18) & 0x3F) as u8, 0x80 | ((cp >> 12) & 0x3F) as u8, 0x80 | ((cp >> 6) & 0x3F) as u8, 0x80 | (cp & 0x3F) as u8]);
    }
}

/// The length of the character that starts with byte `b` in the UTF-8 form.
#[inline]
pub fn char_len(b: u8) -> usize {
    match b {
        0..=0xBF => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        0xF8..=0xFB => 5,
        _ => 6,
    }
}

/// The character at byte `i` of a UTF-8 form, and its length in bytes.
pub fn decode_at(s: &[u8], i: usize) -> (u32, usize) {
    let b = s[i];
    let n = char_len(b).min(s.len() - i);
    if n == 1 {
        return (b as u32, 1);
    }
    let mask = match n {
        2 => 0x1F,
        3 => 0x0F,
        4 => 0x07,
        5 => 0x03,
        _ => 0x01,
    };
    let mut cp = (b & mask) as u32;
    for &c in &s[i + 1..i + n] {
        cp = (cp << 6) | (c & 0x3F) as u32;
    }
    (cp, n)
}

/// The characters of a UTF-8 form.
pub fn chars(s: &[u8]) -> impl Iterator<Item = u32> + '_ {
    let mut i = 0;
    std::iter::from_fn(move || {
        if i >= s.len() {
            return None;
        }
        let (c, n) = decode_at(s, i);
        i += n;
        Some(c)
    })
}

/// The number of characters of a UTF-8 form.
pub fn count(s: &[u8]) -> usize {
    s.iter().filter(|&&b| (b & 0xC0) != 0x80).count()
}

/// The byte offset of character `n` (the end when there are fewer).
pub fn byte_offset(s: &[u8], n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let mut k = 0;
    for (i, &b) in s.iter().enumerate() {
        if (b & 0xC0) != 0x80 {
            if k == n {
                return i;
            }
            k += 1;
        }
    }
    s.len()
}

/// The character offset of byte offset `b`.
pub fn char_offset(s: &[u8], b: usize) -> usize {
    count(&s[..b.min(s.len())])
}

/// A byte string upgraded: each byte is the character of that code.
pub fn upgrade(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len() + b.iter().filter(|&&c| c >= 0x80).count());
    for &c in b {
        if c < 0x80 {
            out.push(c);
        } else {
            out.extend_from_slice(&[0xC0 | (c >> 6), 0x80 | (c & 0x3F)]);
        }
    }
    out
}

/// A character string as bytes, when every character is below 256.
pub fn downgrade(s: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let b = s[i];
        if b < 0x80 {
            out.push(b);
            i += 1;
        } else if (b == 0xC2 || b == 0xC3) && i + 1 < s.len() {
            out.push(((b & 0x03) << 6) | (s[i + 1] & 0x3F));
            i += 2;
        } else {
            return None;
        }
    }
    Some(out)
}

/// Whether a UTF-8 form holds only characters below 256.
pub fn is_downgradable(s: &[u8]) -> bool {
    !s.iter().any(|&b| b >= 0xC4)
}

/// The length of the well-formed (lax: perl's, surrogates and large code
/// points allowed) UTF-8 sequence at `i`, or None.
fn lax_seq(s: &[u8], i: usize) -> Option<usize> {
    let b = s[i];
    if b < 0x80 {
        return Some(1);
    }
    if b < 0xC2 {
        return None;
    }
    let n = char_len(b);
    if i + n > s.len() || !s[i + 1..i + n].iter().all(|&c| (c & 0xC0) == 0x80) {
        return None;
    }
    // overlong forms of 3 bytes and more
    if (b == 0xE0 && s[i + 1] < 0xA0) || (b == 0xF0 && s[i + 1] < 0x90) || (b == 0xF8 && s[i + 1] < 0x88) || (b == 0xFC && s[i + 1] < 0x84) {
        return None;
    }
    Some(n)
}

/// Whether bytes are perl's well-formed UTF-8 (`utf8::decode`, `utf8::valid`).
pub fn is_lax_utf8(s: &[u8]) -> bool {
    let mut i = 0;
    while i < s.len() {
        match lax_seq(s, i) {
            Some(n) => i += n,
            None => return false,
        }
    }
    true
}

/// Strict UTF-8 decoding with the replacement character for each malformed
/// sequence, as `Encode::decode('UTF-8', $bytes)` (and `decode_utf8`) do.
/// The result is a UTF-8 form.
pub fn decode_strict(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        match std::str::from_utf8(&s[i..]) {
            Ok(t) => {
                out.extend_from_slice(t.as_bytes());
                break;
            }
            Err(e) => {
                let ok = e.valid_up_to();
                out.extend_from_slice(&s[i..i + ok]);
                out.extend_from_slice("\u{FFFD}".as_bytes());
                i += ok + e.error_len().unwrap_or(s.len() - i - ok);
            }
        }
    }
    out
}

/// A string being built from pieces of byte and character strings: bytes
/// until a character string joins it, then the UTF-8 form (the bytes so
/// far upgraded), as perl's concatenation does.
#[derive(Default)]
pub struct StrBuf {
    pub buf: Vec<u8>,
    pub utf8: bool,
}

impl StrBuf {
    pub fn new() -> StrBuf {
        StrBuf::default()
    }

    pub fn with_capacity(n: usize) -> StrBuf {
        StrBuf { buf: Vec::with_capacity(n), utf8: false }
    }

    /// Make the buffer a character string (its bytes upgraded).
    pub fn upgrade(&mut self) {
        if !self.utf8 {
            if !self.buf.is_ascii() {
                self.buf = upgrade(&self.buf);
            }
            self.utf8 = true;
        }
    }

    /// Append the bytes of a byte string.
    pub fn push_bytes(&mut self, b: &[u8]) {
        if self.utf8 && !b.is_ascii() {
            for &c in b {
                push_cp(&mut self.buf, c as u32);
            }
        } else {
            self.buf.extend_from_slice(b);
        }
    }

    /// Append the UTF-8 form of a character string.
    pub fn push_utf8(&mut self, b: &[u8]) {
        self.upgrade();
        self.buf.extend_from_slice(b);
    }

    /// Append a string form: UTF-8 when `utf8`, bytes otherwise.
    pub fn push_form(&mut self, b: &[u8], utf8: bool) {
        if utf8 {
            self.push_utf8(b)
        } else {
            self.push_bytes(b)
        }
    }

    /// Append a value's string.
    pub fn push_value(&mut self, v: &Value) {
        match v {
            Value::UStr(s) => self.push_utf8(s),
            Value::Str(s) => self.push_bytes(s),
            _ => self.push_bytes(&v.to_bytes()),
        }
    }

    /// Append one character (a byte when the buffer holds bytes and the
    /// code is below 256).
    pub fn push_char(&mut self, cp: u32) {
        if cp > 0xFF {
            self.upgrade();
        }
        if self.utf8 {
            push_cp(&mut self.buf, cp);
        } else {
            self.buf.push(cp as u8);
        }
    }

    pub fn into_value(self) -> Value {
        Value::string(self.buf, self.utf8)
    }
}

/// The case changes of `uc`, `lc`, `ucfirst`, `lcfirst`, `fc`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Case {
    Upper,
    Lower,
    UpperFirst,
    LowerFirst,
    Fold,
}

/// A case change of a string form: ASCII letters only in a byte string
/// (perl's rules without `use feature 'unicode_strings'`), Unicode's in a
/// character string.
pub fn change_case(b: &[u8], utf8: bool, case: Case) -> Vec<u8> {
    if !utf8 || b.is_ascii() {
        return match case {
            Case::Upper => b.to_ascii_uppercase(),
            Case::Lower | Case::Fold => b.to_ascii_lowercase(),
            Case::UpperFirst | Case::LowerFirst => {
                let mut o = b.to_vec();
                if let Some(f) = o.first_mut() {
                    *f = if case == Case::UpperFirst { f.to_ascii_uppercase() } else { f.to_ascii_lowercase() };
                }
                o
            }
        };
    }
    let mut out = Vec::with_capacity(b.len());
    let map = |out: &mut Vec<u8>, cp: u32, case: Case| match char::from_u32(cp) {
        None => push_cp(out, cp),
        Some(c) => match case {
            Case::Upper => c.to_uppercase().for_each(|u| push_cp(out, u as u32)),
            Case::Lower | Case::LowerFirst => c.to_lowercase().for_each(|u| push_cp(out, u as u32)),
            Case::Fold => match c {
                'ß' => out.extend_from_slice(b"ss"),
                _ => c.to_lowercase().for_each(|u| push_cp(out, u as u32)),
            },
            Case::UpperFirst => match titlecase(cp) {
                Some(t) => t.iter().for_each(|&u| push_cp(out, u)),
                None => c.to_uppercase().for_each(|u| push_cp(out, u as u32)),
            },
        },
    };
    match case {
        Case::Upper | Case::Lower | Case::Fold => {
            for cp in chars(b) {
                map(&mut out, cp, case);
            }
        }
        Case::UpperFirst | Case::LowerFirst => {
            if !b.is_empty() {
                let (cp, n) = decode_at(b, 0);
                map(&mut out, cp, case);
                out.extend_from_slice(&b[n..]);
            }
        }
    }
    out
}

/// The titlecase of the characters whose titlecase is not their uppercase.
fn titlecase(cp: u32) -> Option<&'static [u32]> {
    Some(match cp {
        0xDF => &[0x53, 0x73],
        0x1C4..=0x1C6 => &[0x1C5],
        0x1C7..=0x1C9 => &[0x1C8],
        0x1CA..=0x1CC => &[0x1CB],
        0x1F1..=0x1F3 => &[0x1F2],
        0xFB00 => &[0x46, 0x66],
        0xFB01 => &[0x46, 0x69],
        0xFB02 => &[0x46, 0x6C],
        _ => return None,
    })
}

/// A string's characters reversed (`scalar reverse`).
pub fn reverse_chars(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len());
    let mut end = b.len();
    while end > 0 {
        let mut s = end - 1;
        while s > 0 && (b[s] & 0xC0) == 0x80 {
            s -= 1;
        }
        out.extend_from_slice(&b[s..end]);
        end = s;
    }
    out
}

/// The length of the character at byte `i` (1 for a byte string).
#[inline]
pub fn step(b: &[u8], i: usize, utf8: bool) -> usize {
    if utf8 && i < b.len() {
        char_len(b[i]).min(b.len() - i)
    } else {
        1
    }
}

/// The byte range of characters `off..off+len` as `substr` takes them
/// (character offsets in, byte offsets out), for a UTF-8 form.
pub fn char_range_to_bytes(b: &[u8], start: usize, end: usize) -> (usize, usize) {
    let s = byte_offset(b, start);
    let e = s + byte_offset(&b[s..], end - start);
    (s, e)
}
