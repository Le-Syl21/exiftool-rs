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
    /// `(...)`: a group, repeated by `count`.
    group: Option<Vec<Item>>,
}

fn parse_template(t: &[u8]) -> Result<Vec<Item>, String> {
    let mut i = 0;
    let items = parse_items(t, &mut i)?;
    if i < t.len() {
        return Err("Mismatched brackets in template".into());
    }
    Ok(items)
}

fn parse_items(t: &[u8], pos: &mut usize) -> Result<Vec<Item>, String> {
    let mut items = Vec::new();
    let mut i = *pos;
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
        if c == b')' {
            break;
        }
        // `%<bits>`: the checksum of the next item (unpack only).
        if c == b'%' {
            i += 1;
            let mut bits = 0usize;
            let d0 = i;
            while i < t.len() && t[i].is_ascii_digit() {
                bits = bits * 10 + (t[i] - b'0') as usize;
                i += 1;
            }
            if i == d0 {
                bits = 16;
            }
            items.push(Item { letter: b'%', endian: None, bang: false, count: Count::N(bits), slash: false, group: None });
            continue;
        }
        let mut group = None;
        if c == b'(' {
            i += 1;
            let g = parse_items(t, &mut i)?;
            if t.get(i) != Some(&b')') {
                return Err("No group ending character ')' found in template".into());
            }
            group = Some(g);
        } else if !c.is_ascii_alphabetic() && c != b'@' && c != b'.' {
            return Err(format!("Invalid type '{}' in pack", c as char));
        }
        i += 1;
        let mut item = Item { letter: c, endian: None, bang: false, count: Count::One, slash: false, group };
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
    *pos = i;
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
    WARNINGS.with(|w| w.borrow_mut().clear());
    // a template that starts with U works on characters (U0 mode)
    U0.with(|u| u.set(items.first().is_some_and(|i| i.letter == b'U')));
    unpack_items(&items, data, 0, &mut out)?;
    Ok(out)
}

thread_local! {
    /// The warnings of the last unpack (malformed UTF-8), for the caller to
    /// issue through $SIG{__WARN__}.
    static WARNINGS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    /// U0 mode: U reads characters (the bytes of a byte string), not UTF-8.
    static U0: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The warnings the last unpack gave.
pub fn take_warnings() -> Vec<String> {
    WARNINGS.with(|w| std::mem::take(&mut *w.borrow_mut()))
}

/// `\xNN` for each byte (perl's malformed-UTF-8 messages).
fn hex_bytes(s: &[u8]) -> String {
    s.iter().map(|c| format!("\\x{c:02x}")).collect()
}

/// The (perl-extended) UTF-8 encoding of a code point.
fn encode_utf8_ext(v: u64) -> Vec<u8> {
    if v < 0x80 {
        return vec![v as u8];
    }
    let len = match v {
        0..=0x7ff => 2,
        0x800..=0xffff => 3,
        0x10000..=0x1fffff => 4,
        0x200000..=0x3ffffff => 5,
        _ => 6,
    };
    let mut out = vec![0u8; len];
    let mut x = v;
    for i in (1..len).rev() {
        out[i] = 0x80 | (x & 0x3f) as u8;
        x >>= 6;
    }
    out[0] = ((0xff00u16 >> len) as u8) | x as u8;
    out
}

/// One UTF-8 character of a byte string, as unpack's C0 mode reads it:
/// (code point, bytes taken, warnings). A malformed sequence gives 0 and
/// perl's "Malformed UTF-8 character" messages, and takes its start byte and
/// the continuation bytes that follow it.
fn c0u_char(b: &[u8]) -> (u64, usize, Vec<String>) {
    let lead = b[0];
    if lead < 0x80 {
        return (lead as u64, 1, Vec::new());
    }
    let need = match lead {
        0x80..=0xbf => {
            return (0, 1, vec![format!("Malformed UTF-8 character: {} (unexpected continuation byte 0x{lead:02x}, with no preceding start byte) in unpack", hex_bytes(&b[..1]))]);
        }
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        0xf8..=0xfb => 5,
        0xfc..=0xfd => 6,
        0xfe => 7,
        _ => 13,
    };
    let avail = 1 + b[1..].iter().take(need - 1).take_while(|c| (0x80..0xc0).contains(*c)).count();
    if avail < need {
        let mut w = Vec::new();
        let got = &b[..avail];
        // perl reports a sequence cut by the end of the string, then a
        // non-continuation byte inside it
        if b.len() < need && b.len() > avail {
            w.push(format!("Malformed UTF-8 character: {} (too short; {} bytes available, need {need}) in unpack", hex_bytes(b), b.len()));
        }
        if b.len() > avail {
            w.push(format!(
                "Malformed UTF-8 character: {} (unexpected non-continuation byte 0x{:02x}, immediately after start byte 0x{lead:02x}; need {need} bytes, got {avail}) in unpack",
                hex_bytes(&b[..need.min(b.len())]),
                b[avail]
            ));
        } else {
            let s = if avail == 1 { "" } else { "s" };
            w.push(format!("Malformed UTF-8 character: {} (too short; {avail} byte{s} available, need {need}) in unpack", hex_bytes(got)));
        }
        let second = b.get(1).copied().filter(|_| avail > 1);
        let overlong_start = lead == 0xc0
            || lead == 0xc1
            || second.is_some_and(|c| (lead == 0xe0 && c < 0xa0) || (lead == 0xf0 && c < 0x90) || (lead == 0xf8 && c < 0x88) || (lead == 0xfc && c < 0x84) || (lead == 0xfe && c < 0x82));
        if overlong_start {
            w.push(format!(
                "Malformed UTF-8 character: {} (any UTF-8 sequence that starts with \"{}\" is overlong which can and should be represented with a different, shorter sequence) in unpack",
                hex_bytes(got),
                hex_bytes(got)
            ));
        }
        return (0, avail, w);
    }
    let mut v = if need >= 7 { 0 } else { (lead as u64) & (0x7f >> need) };
    for c in &b[1..need] {
        v = (v << 6) | (*c as u64 & 0x3f);
    }
    let min: u64 = match need {
        2 => 0x80,
        3 => 0x800,
        4 => 0x10000,
        5 => 0x200000,
        6 => 0x4000000,
        _ => 0x80000000,
    };
    if v < min {
        let mut digits = format!("{v:X}");
        while digits.len() < 2 || digits.len() % 2 == 1 {
            digits.insert(0, '0');
        }
        return (0, need, vec![format!("Malformed UTF-8 character: {} (overlong; instead use {} to represent U+{digits}) in unpack", hex_bytes(&b[..need]), hex_bytes(&encode_utf8_ext(v)))]);
    }
    (v, need, Vec::new())
}

fn unpack_items(items: &[Item], data: &[u8], start: usize, out: &mut Vec<Value>) -> Result<usize, String> {
    let mut pos = start;
    let mut pending_count: Option<usize> = None;
    let mut checksum: Option<(usize, usize)> = None; // (bits, values before the item)
    for item in items {
        if item.letter == b'%' {
            if let Count::N(bits) = item.count {
                checksum = Some((bits, out.len()));
            }
            continue;
        }
        if let Some((bits, from)) = checksum.take() {
            pos = unpack_items(std::slice::from_ref(item), data, pos, out)?;
            let vals: Vec<Value> = out.drain(from..).collect();
            out.push(checksum_of(&vals, bits));
            continue;
        }
        let count = match pending_count.take() {
            Some(n) => Count::N(n),
            None => item.count,
        };
        if let Some(g) = &item.group {
            let n = match count {
                Count::One => 1,
                Count::N(n) => n,
                Count::Star => usize::MAX,
            };
            let mut k = 0;
            while k < n && (count != Count::Star || pos < data.len()) {
                pos = unpack_items(g, data, pos, out)?;
                k += 1;
            }
            continue;
        }
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
            b'U' if count == Count::N(0) => U0.with(|u| u.set(true)),
            b'C' if count == Count::N(0) => U0.with(|u| u.set(false)),
            b'U' => {
                if U0.with(|u| u.get()) {
                    // U0 mode: the characters of a byte string are its bytes
                    let n = repeat(count, remaining);
                    for &c in &data[pos..pos + n] {
                        out.push(Value::Int(c as i64));
                    }
                    pos += n;
                } else {
                    // C0 mode: U reads UTF-8 characters from the bytes
                    if count == Count::One && pos >= data.len() {
                        return Err("U0 mode on a byte string".into());
                    }
                    let mut k = 0;
                    while pos < data.len() && (count == Count::Star || k < repeat(count, usize::MAX)) {
                        let (v, used, w) = c0u_char(&data[pos..]);
                        WARNINGS.with(|x| x.borrow_mut().extend(w));
                        out.push(crate::perl::ops::from_uv(v));
                        pos += used;
                        k += 1;
                    }
                }
            }
            b'u' => {
                // uudecode the rest: lines of a length char then 4-char groups.
                let mut res = Vec::new();
                while pos < data.len() {
                    let len = (data[pos].wrapping_sub(32) & 0x3f) as usize;
                    pos += 1;
                    if len == 0 {
                        // skip to the end of the line
                        while pos < data.len() && data[pos] != b'\n' {
                            pos += 1;
                        }
                        pos += 1;
                        continue;
                    }
                    let mut got = 0;
                    while got < len && pos < data.len() {
                        let mut q = [0u8; 4];
                        for k in 0..4 {
                            q[k] = data.get(pos + k).map(|c| c.wrapping_sub(32) & 0x3f).unwrap_or(0);
                        }
                        let b = [(q[0] << 2) | (q[1] >> 4), (q[1] << 4) | (q[2] >> 2), (q[2] << 6) | q[3]];
                        for &x in &b {
                            if got < len {
                                res.push(x);
                                got += 1;
                            }
                        }
                        pos += 4;
                    }
                    // skip the newline
                    while pos < data.len() && data[pos] != b'\n' {
                        pos += 1;
                    }
                    pos += 1;
                }
                out.push(Value::bytes(res));
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
    Ok(pos)
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
    let mut cur = Args { v: args, i: 0 };
    pack_items(&items, &mut cur, &mut out)?;
    Ok(out)
}

struct Args<'a> {
    v: &'a [Value],
    i: usize,
}

impl Args<'_> {
    fn next(&mut self) -> Value {
        let v = self.v.get(self.i).cloned().unwrap_or(Value::Undef);
        self.i += 1;
        v
    }

    fn remaining(&self) -> usize {
        self.v.len().saturating_sub(self.i)
    }
}

fn pack_items(items: &[Item], cur: &mut Args, out: &mut Vec<u8>) -> Result<(), String> {
    for item in items {
        if let Some(g) = &item.group {
            match item.count {
                Count::Star => {
                    while cur.remaining() > 0 {
                        pack_items(g, cur, out)?;
                    }
                }
                c => {
                    for _ in 0..repeat(c, 0) {
                        pack_items(g, cur, out)?;
                    }
                }
            }
            continue;
        }
        match item.letter {
            b'a' | b'A' | b'Z' => {
                let s = cur.next().to_bytes();
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
                let s = cur.next().to_bytes();
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
                let s = cur.next().to_bytes();
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
                    Count::Star => cur.remaining(),
                    c => repeat(c, 0),
                };
                for _ in 0..n {
                    let v = cur.next();
                    let cp = v.to_i64().max(0) as u32;
                    let ch = char::from_u32(cp).unwrap_or('\u{FFFD}');
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
            }
            b'u' => {
                // uuencode, 45 bytes per line
                let s = cur.next().to_bytes();
                let enc = |c: u8| if c == 0 { b'`' } else { c + 32 };
                for chunk in s.chunks(45) {
                    out.push(enc(chunk.len() as u8));
                    for g in chunk.chunks(3) {
                        let a = [g[0], *g.get(1).unwrap_or(&0), *g.get(2).unwrap_or(&0)];
                        out.push(enc(a[0] >> 2));
                        out.push(enc(((a[0] & 3) << 4) | (a[1] >> 4)));
                        out.push(enc(((a[1] & 0xf) << 2) | (a[2] >> 6)));
                        out.push(enc(a[2] & 0x3f));
                    }
                    out.push(b'\n');
                }
            }
            b'w' => {
                let n = match item.count {
                    Count::Star => cur.remaining(),
                    c => repeat(c, 0),
                };
                for _ in 0..n {
                    let mut v = crate::perl::ops::to_uv(&cur.next());
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
                    Count::Star => cur.remaining(),
                    c => repeat(c, 0),
                };
                for _ in 0..n {
                    let v = cur.next().to_f64();
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
                    Count::Star => cur.remaining(),
                    c => repeat(c, 0),
                };
                for _ in 0..n {
                    let u = crate::perl::ops::to_uv(&cur.next());
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
    Ok(())
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

/// The `%<bits>` checksum of unpacked values: their sum modulo 2**bits, as
/// perl computes it (integer sum, or a floating one when a value is not an
/// integer or bits is 64 or more).
fn checksum_of(vals: &[Value], bits: usize) -> Value {
    let all_int = vals.iter().all(|v| matches!(v, Value::Int(_)));
    if all_int && bits < 64 {
        let sum = vals.iter().fold(0u64, |a, v| a.wrapping_add(if let Value::Int(i) = v { *i as u64 } else { 0 }));
        let mask = if bits == 0 { 0 } else { (1u64 << bits) - 1 };
        return Value::Int((sum & mask) as i64);
    }
    let sum: f64 = vals.iter().map(|v| v.to_f64()).sum();
    let m = 2f64.powi(bits.min(1023) as i32);
    let r = sum - (sum / m).floor() * m;
    Value::Num(r)
}
