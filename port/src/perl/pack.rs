//! Perl's `pack` and `unpack`, for the templates ExifTool uses.
//!
//! ExifTool calls them about 1,400 times: integers in both byte orders
//! (`n N v V s S l L q Q`, `<` `>` `!`), bytes (`C c`), strings (`a A Z`),
//! hex and bit strings (`H h B b`), floats (`f d`), UTF-8 (`U`), BER
//! integers (`w`) and positioning (`x X @`). Native order is little-endian,
//! as on the machines ExifTool's expected outputs come from.

use crate::perl::ops::from_uv;
use crate::perl::value::Value;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Count {
    One,
    N(usize),
    Star,
}

#[derive(Clone, Debug)]
struct Item {
    letter: u8,
    /// `<` little-endian, `>` big-endian, or native.
    endian: Option<u8>,
    /// `!` modifier.
    bang: bool,
    count: Count,
    /// `letter/...`: this item gives the count of the next one.
    slash: bool,
}

fn parse_template(t: &[u8]) -> Result<Vec<Item>, String> {
    let mut items = Vec::new();
    let mut i = 0;
    while i < t.len() {
        let c = t[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c == b'#' {
            while i < t.len() && t[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == b'(' || c == b')' {
            return Err("pack groups are not supported".into());
        }
        if !c.is_ascii_alphabetic() && c != b'@' && c != b'.' {
            return Err(format!("Invalid type '{}' in pack", c as char));
        }
        i += 1;
        let mut item = Item { letter: c, endian: None, bang: false, count: Count::One, slash: false };
        while i < t.len() && matches!(t[i], b'<' | b'>' | b'!') {
            match t[i] {
                b'!' => item.bang = true,
                e => item.endian = Some(e),
            }
            i += 1;
        }
        if i < t.len() && t[i] == b'*' {
            item.count = Count::Star;
            i += 1;
        } else if i < t.len() && t[i] == b'[' {
            let s = i + 1;
            while i < t.len() && t[i] != b']' {
                i += 1;
            }
            let n = std::str::from_utf8(&t[s..i]).ok().and_then(|x| x.parse().ok()).ok_or("bad count")?;
            item.count = Count::N(n);
            i += 1;
        } else {
            let s = i;
            while i < t.len() && t[i].is_ascii_digit() {
                i += 1;
            }
            if i > s {
                item.count = Count::N(std::str::from_utf8(&t[s..i]).unwrap().parse().map_err(|_| "bad count")?);
            }
        }
        if i < t.len() && t[i] == b'/' {
            item.slash = true;
            i += 1;
        }
        items.push(item);
    }
    Ok(items)
}

/// Size in bytes of a fixed-size numeric letter, and whether it is signed.
fn int_size(letter: u8, bang: bool) -> Option<(usize, bool)> {
    Some(match letter {
        b'c' => (1, true),
        b'C' | b'W' => (1, false),
        b's' => (2, true),
        b'S' => (2, false),
        b'l' => (4, true),
        b'L' => (4, false),
        b'q' | b'j' => (8, true),
        b'Q' | b'J' => (8, false),
        b'i' => (4, true),
        b'I' => (4, false),
        b'n' | b'v' => (2, bang),
        b'N' | b'V' => (4, bang),
        _ => return None,
    })
}

fn big_endian(item: &Item) -> bool {
    match item.letter {
        b'n' | b'N' => true,
        b'v' | b'V' => false,
        _ => item.endian == Some(b'>'),
    }
}

/// `unpack(TEMPLATE, EXPR)` in list context.
pub fn unpack(template: &[u8], data: &[u8]) -> Result<Vec<Value>, String> {
    let items = parse_template(template)?;
    let mut out = Vec::new();
    let mut pos = 0usize;
    let mut pending_count: Option<usize> = None;
    for item in &items {
        let count = match pending_count.take() {
            Some(n) => Count::N(n),
            None => item.count,
        };
        let remaining = data.len().saturating_sub(pos);
        match item.letter {
            b'a' | b'A' | b'Z' => {
                let n = match count {
                    Count::One => 1,
                    Count::N(n) => n,
                    Count::Star => remaining,
                }
                .min(remaining);
                let mut s = &data[pos..pos + n];
                pos += n;
                match item.letter {
                    b'A' => {
                        let end = s.iter().rposition(|&b| !matches!(b, b' ' | 0 | b'\t' | b'\n' | b'\r' | 0x0c)).map_or(0, |p| p + 1);
                        s = &s[..end];
                    }
                    b'Z' => {
                        if let Some(z) = s.iter().position(|&b| b == 0) {
                            s = &s[..z];
                        }
                    }
                    _ => {}
                }
                out.push(Value::bytes(s));
            }
            b'H' | b'h' => {
                let nyb = match count {
                    Count::One => 1,
                    Count::N(n) => n,
                    Count::Star => remaining * 2,
                }
                .min(remaining * 2);
                let bytes = nyb.div_ceil(2);
                let mut s = String::with_capacity(nyb);
                for k in 0..nyb {
                    let b = data[pos + k / 2];
                    let v = if (k % 2 == 0) == (item.letter == b'H') { b >> 4 } else { b & 0xf };
                    s.push(char::from_digit(v as u32, 16).unwrap());
                }
                pos += bytes;
                out.push(Value::str(&s));
            }
            b'B' | b'b' => {
                let bits = match count {
                    Count::One => 1,
                    Count::N(n) => n,
                    Count::Star => remaining * 8,
                }
                .min(remaining * 8);
                let mut s = String::with_capacity(bits);
                for k in 0..bits {
                    let b = data[pos + k / 8];
                    let bit = if item.letter == b'B' { (b >> (7 - k % 8)) & 1 } else { (b >> (k % 8)) & 1 };
                    s.push(if bit == 1 { '1' } else { '0' });
                }
                pos += bits.div_ceil(8);
                out.push(Value::str(&s));
            }
            b'x' => {
                let n = match count {
                    Count::One => 1,
                    Count::N(n) => n,
                    Count::Star => 0,
                };
                if item.bang {
                    if n > 0 && pos % n != 0 {
                        pos += n - pos % n;
                    }
                } else {
                    if n > remaining {
                        return Err("'x' outside of string in unpack".into());
                    }
                    pos += n;
                }
            }
            b'X' => {
                let n = match count {
                    Count::One => 1,
                    Count::N(n) => n,
                    Count::Star => 0,
                };
                if n > pos {
                    return Err("'X' outside of string in unpack".into());
                }
                pos -= n;
            }
            b'@' => {
                let n = match count {
                    Count::One => 0,
                    Count::N(n) => n,
                    Count::Star => data.len(),
                };
                if n > data.len() {
                    return Err("'@' outside of string in unpack".into());
                }
                pos = n;
            }
            b'f' | b'd' | b'F' => {
                let size = if item.letter == b'f' { 4 } else { 8 };
                let n = repeat(count, remaining / size);
                for _ in 0..n {
                    if pos + size > data.len() {
                        break;
                    }
                    let mut b = data[pos..pos + size].to_vec();
                    if item.endian == Some(b'>') {
                        b.reverse();
                    }
                    let v = if size == 4 {
                        f32::from_le_bytes(b.try_into().unwrap()) as f64
                    } else {
                        f64::from_le_bytes(b.try_into().unwrap())
                    };
                    out.push(Value::Num(v));
                    pos += size;
                }
            }
            b'U' => {
                // On a byte string, U reads UTF-8 characters.
                let text = String::from_utf8_lossy(&data[pos..]).into_owned();
                let n = repeat(count, text.chars().count());
                let mut consumed = 0;
                for ch in text.chars().take(n) {
                    out.push(Value::Int(ch as i64));
                    consumed += ch.len_utf8();
                }
                pos += consumed;
            }
            b'w' => {
                let n = repeat(count, usize::MAX);
                for _ in 0..n {
                    if pos >= data.len() {
                        break;
                    }
                    // Beyond 64 bits perl carries on in decimal, as a string.
                    let mut v: u64 = 0;
                    let mut big: Option<Vec<u8>> = None;
                    loop {
                        let Some(&b) = data.get(pos) else { return Err("Unterminated compressed integer in unpack".into()) };
                        pos += 1;
                        let d = (b & 0x7f) as u32;
                        match &mut big {
                            Some(digits) => decimal_mul_add(digits, 128, d),
                            None => match v.checked_mul(128).and_then(|x| x.checked_add(d as u64)) {
                                Some(x) => v = x,
                                None => {
                                    let mut digits: Vec<u8> = v.to_string().into_bytes();
                                    decimal_mul_add(&mut digits, 128, d);
                                    big = Some(digits);
                                }
                            },
                        }
                        if b & 0x80 == 0 {
                            break;
                        }
                    }
                    out.push(match big {
                        Some(digits) => Value::bytes(digits),
                        None => from_uv(v),
                    });
                }
            }
            letter => {
                let (size, signed) = int_size(letter, item.bang).ok_or_else(|| format!("Invalid type '{}' in unpack", letter as char))?;
                let n = repeat(count, remaining / size);
                for _ in 0..n {
                    if pos + size > data.len() {
                        break;
                    }
                    let b = &data[pos..pos + size];
                    let mut u: u64 = 0;
                    if big_endian(item) {
                        for &x in b {
                            u = (u << 8) | x as u64;
                        }
                    } else {
                        for &x in b.iter().rev() {
                            u = (u << 8) | x as u64;
                        }
                    }
                    let v = if signed {
                        let shift = 64 - size * 8;
                        Value::Int(((u << shift) as i64) >> shift)
                    } else {
                        from_uv(u)
                    };
                    pos += size;
                    if item.slash {
                        pending_count = Some(v.to_i64().max(0) as usize);
                    } else {
                        out.push(v);
                    }
                }
            }
        }
    }
    Ok(out)
}

/// `digits = digits * mul + add`, on a decimal string.
fn decimal_mul_add(digits: &mut Vec<u8>, mul: u32, add: u32) {
    let mut carry = add;
    for d in digits.iter_mut().rev() {
        let x = (*d - b'0') as u32 * mul + carry;
        *d = b'0' + (x % 10) as u8;
        carry = x / 10;
    }
    while carry > 0 {
        digits.insert(0, b'0' + (carry % 10) as u8);
        carry /= 10;
    }
}

fn repeat(count: Count, star: usize) -> usize {
    match count {
        Count::One => 1,
        Count::N(n) => n,
        Count::Star => star,
    }
}

/// `pack(TEMPLATE, LIST)`.
pub fn pack(template: &[u8], args: &[Value]) -> Result<Vec<u8>, String> {
    let items = parse_template(template)?;
    let mut out = Vec::new();
    let mut a = args.iter();
    let mut next = || a.next().cloned().unwrap_or(Value::Undef);
    for item in &items {
        match item.letter {
            b'a' | b'A' | b'Z' => {
                let s = next().to_bytes();
                let pad = if item.letter == b'A' { b' ' } else { 0 };
                match item.count {
                    Count::Star => {
                        out.extend_from_slice(&s);
                        if item.letter == b'Z' {
                            out.push(0);
                        }
                    }
                    c => {
                        let n = repeat(c, 0);
                        let mut field = s.to_vec();
                        let keep = if item.letter == b'Z' && n > 0 { n - 1 } else { n };
                        field.truncate(keep);
                        while field.len() < n {
                            field.push(pad);
                        }
                        out.extend(field);
                    }
                }
            }
            b'H' | b'h' => {
                let s = next().to_bytes();
                let nyb = match item.count {
                    Count::Star => s.len(),
                    c => repeat(c, 0),
                };
                let mut byte = 0u8;
                for k in 0..nyb {
                    let ch = s.get(k).copied().unwrap_or(b'0');
                    let v = if ch.is_ascii_alphabetic() { (ch.to_ascii_lowercase() - b'a' + 10) & 0xf } else { ch.wrapping_sub(b'0') & 0xf };
                    if item.letter == b'H' {
                        if k % 2 == 0 {
                            byte = v << 4;
                        } else {
                            byte |= v;
                        }
                    } else if k % 2 == 0 {
                        byte = v;
                    } else {
                        byte |= v << 4;
                    }
                    if k % 2 == 1 || k + 1 == nyb {
                        out.push(byte);
                        byte = 0;
                    }
                }
            }
            b'B' | b'b' => {
                let s = next().to_bytes();
                let bits = match item.count {
                    Count::Star => s.len(),
                    c => repeat(c, 0),
                };
                let mut byte = 0u8;
                for k in 0..bits {
                    let on = s.get(k).is_some_and(|&c| c & 1 == 1);
                    if on {
                        byte |= if item.letter == b'B' { 0x80 >> (k % 8) } else { 1 << (k % 8) };
                    }
                    if k % 8 == 7 || k + 1 == bits {
                        out.push(byte);
                        byte = 0;
                    }
                }
            }
            b'x' => {
                let n = repeat(item.count, 0);
                if item.bang {
                    if n > 0 && out.len() % n != 0 {
                        let pad = n - out.len() % n;
                        out.extend(std::iter::repeat(0).take(pad));
                    }
                } else {
                    out.extend(std::iter::repeat(0).take(n));
                }
            }
            b'X' => {
                let n = repeat(item.count, 0).min(out.len());
                out.truncate(out.len() - n);
            }
            b'@' => {
                let n = repeat(item.count, 0);
                out.resize(n, 0);
            }
            b'U' => {
                let n = match item.count {
                    Count::Star => args.len(),
                    c => repeat(c, 0),
                };
                for _ in 0..n {
                    let v = next();
                    let cp = v.to_i64().max(0) as u32;
                    let ch = char::from_u32(cp).unwrap_or('\u{FFFD}');
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
            }
            b'w' => {
                let n = match item.count {
                    Count::Star => args.len(),
                    c => repeat(c, 0),
                };
                for _ in 0..n {
                    let mut v = crate::perl::ops::to_uv(&next());
                    let mut bytes = vec![(v & 0x7f) as u8];
                    v >>= 7;
                    while v > 0 {
                        bytes.push((v & 0x7f) as u8 | 0x80);
                        v >>= 7;
                    }
                    bytes.reverse();
                    out.extend(bytes);
                }
            }
            b'f' | b'd' | b'F' => {
                let n = match item.count {
                    Count::Star => args.len(),
                    c => repeat(c, 0),
                };
                for _ in 0..n {
                    let v = next().to_f64();
                    let mut b = if item.letter == b'f' { (v as f32).to_le_bytes().to_vec() } else { v.to_le_bytes().to_vec() };
                    if item.endian == Some(b'>') {
                        b.reverse();
                    }
                    out.extend(b);
                }
            }
            letter => {
                let (size, _) = int_size(letter, item.bang).ok_or_else(|| format!("Invalid type '{}' in pack", letter as char))?;
                let n = match item.count {
                    Count::Star => args.len(),
                    c => repeat(c, 0),
                };
                for _ in 0..n {
                    let u = crate::perl::ops::to_uv(&next());
                    let le = u.to_le_bytes();
                    let mut b = le[..size].to_vec();
                    if big_endian(item) {
                        b.reverse();
                    }
                    out.extend(b);
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn up(t: &str, d: &[u8]) -> Vec<String> {
        unpack(t.as_bytes(), d).unwrap().iter().map(Value::to_text).collect()
    }

    #[test]
    fn unpack_basics() {
        assert_eq!(up("n", &[1, 2]), ["258"]);
        assert_eq!(up("v", &[1, 2]), ["513"]);
        assert_eq!(up("N", &[0xff, 0xff, 0xff, 0xff]), ["4294967295"]);
        assert_eq!(up("n!", &[0xff, 0xfe]), ["-2"]);
        assert_eq!(up("C*", &[1, 2, 3]), ["1", "2", "3"]);
        assert_eq!(up("x2C", &[1, 2, 3]), ["3"]);
        assert_eq!(up("H*", &[0xab, 0x01]), ["ab01"]);
        assert_eq!(up("H2", &[0xab, 0x01]), ["ab"]);
        assert_eq!(up("A4", b"ab  \0"), ["ab"]);
        assert_eq!(up("Z*", b"ab\0cd"), ["ab"]);
        assert_eq!(up("a3", b"ab\0cd"), ["ab\0"]);
        assert_eq!(up("B8", &[0x81]), ["10000001"]);
        assert_eq!(up("s<", &[0xfe, 0xff]), ["-2"]);
        assert_eq!(up("l>", &[0xff, 0xff, 0xff, 0xfe]), ["-2"]);
        assert_eq!(up("C/a", &[2, b'h', b'i', b'x']), ["hi"]);
    }

    #[test]
    fn pack_basics() {
        assert_eq!(pack(b"n", &[Value::Int(258)]).unwrap(), [1, 2]);
        assert_eq!(pack(b"V", &[Value::Int(1)]).unwrap(), [1, 0, 0, 0]);
        assert_eq!(pack(b"H*", &[Value::str("ab01")]).unwrap(), [0xab, 0x01]);
        assert_eq!(pack(b"A4", &[Value::str("ab")]).unwrap(), b"ab  ");
        assert_eq!(pack(b"a4", &[Value::str("ab")]).unwrap(), b"ab\0\0");
        assert_eq!(pack(b"C*", &[Value::Int(1), Value::Int(300)]).unwrap(), [1, 44]);
        assert_eq!(pack(b"U*", &[Value::Int(0xe9)]).unwrap(), "é".as_bytes());
    }
}
