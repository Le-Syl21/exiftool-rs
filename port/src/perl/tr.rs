//! Perl's `tr///` (and `y///`): byte translation, deletion, counting.
//!
//! ExifTool uses it about 420 times: `tr/\0//d` to drop NULs,
//! `tr/a-zA-Z0-9//dc` to keep a set, `tr/,/ /` to replace, and in scalar
//! context to count (`($val =~ tr/,//) + 1`).

/// A compiled `tr/SEARCH/REPLACE/flags`.
pub struct Tr {
    map: [Action; 256],
}

#[derive(Clone, Copy, PartialEq)]
enum Action {
    Keep,
    Replace(u8),
    Delete,
}

/// Expand a tr list: escapes (`\n \t \r \0 \xHH \\ \- \/`, octal) and ranges.
fn expand(list: &[u8]) -> Vec<u8> {
    let mut chars = Vec::new();
    let mut i = 0;
    let mut lit = Vec::new();
    while i < list.len() {
        let (c, n) = if list[i] == b'\\' && i + 1 < list.len() {
            let e = list[i + 1];
            match e {
                b'n' => (b'\n', 2),
                b't' => (b'\t', 2),
                b'r' => (b'\r', 2),
                b'f' => (0x0c, 2),
                b'e' => (0x1b, 2),
                b'a' => (0x07, 2),
                b'x' => {
                    if list.get(i + 2) == Some(&b'{') {
                        let end = list[i + 3..].iter().position(|&b| b == b'}').map_or(list.len(), |p| i + 3 + p);
                        let v = u8::from_str_radix(std::str::from_utf8(&list[i + 3..end]).unwrap_or("0"), 16).unwrap_or(0);
                        (v, end + 1 - i)
                    } else {
                        let mut j = i + 2;
                        while j < list.len() && j < i + 4 && list[j].is_ascii_hexdigit() {
                            j += 1;
                        }
                        let v = u8::from_str_radix(std::str::from_utf8(&list[i + 2..j]).unwrap_or("0"), 16).unwrap_or(0);
                        (v, j - i)
                    }
                }
                b'0'..=b'7' => {
                    let mut j = i + 1;
                    while j < list.len() && j < i + 4 && (b'0'..=b'7').contains(&list[j]) {
                        j += 1;
                    }
                    let v = u32::from_str_radix(std::str::from_utf8(&list[i + 1..j]).unwrap(), 8).unwrap_or(0);
                    ((v & 0xff) as u8, j - i)
                }
                other => (other, 2),
            }
        } else {
            (list[i], 1)
        };
        // A literal `-` (escaped) must not form a range: mark it.
        let escaped_dash = list[i] == b'\\' && c == b'-';
        lit.push((c, escaped_dash));
        i += n;
    }
    let mut k = 0;
    while k < lit.len() {
        let (c, _) = lit[k];
        if k + 2 < lit.len() && lit[k + 1] == (b'-', false) {
            let (end, _) = lit[k + 2];
            if end >= c {
                chars.extend(c..=end);
            }
            k += 3;
        } else {
            chars.push(c);
            k += 1;
        }
    }
    chars
}

impl Tr {
    /// Build from the search and replace lists and the flags (`c`, `d`, `s`
    /// handled; `r` is the caller's).
    pub fn new(search: &[u8], replace: &[u8], flags: &str) -> Tr {
        let complement = flags.contains('c');
        let delete = flags.contains('d');
        let mut search = expand(search);
        let mut replace = expand(replace);
        if complement {
            let mut in_set = [false; 256];
            for &c in &search {
                in_set[c as usize] = true;
            }
            search = (0u16..256).filter(|&c| !in_set[c as usize]).map(|c| c as u8).collect();
        }
        if replace.is_empty() && !delete {
            replace = search.clone();
        }
        let mut map = [Action::Keep; 256];
        let mut seen = [false; 256];
        for (k, &c) in search.iter().enumerate() {
            if seen[c as usize] {
                continue; // first occurrence wins
            }
            seen[c as usize] = true;
            map[c as usize] = if k < replace.len() {
                Action::Replace(replace[k])
            } else if delete {
                Action::Delete
            } else {
                // Short replacement list: the last char repeats.
                Action::Replace(*replace.last().unwrap_or(&c))
            };
        }
        Tr { map }
    }

    /// Apply to `s`: the translated bytes and the number of matched bytes.
    pub fn apply(&self, s: &[u8], squeeze: bool) -> (Vec<u8>, usize) {
        let mut out = Vec::with_capacity(s.len());
        let mut count = 0;
        let mut last_replaced: Option<u8> = None;
        for &b in s {
            match self.map[b as usize] {
                Action::Keep => {
                    out.push(b);
                    last_replaced = None;
                }
                Action::Delete => count += 1,
                Action::Replace(r) => {
                    count += 1;
                    if squeeze && last_replaced == Some(r) {
                        continue;
                    }
                    out.push(r);
                    last_replaced = Some(r);
                }
            }
        }
        (out, count)
    }
}

/// `$s =~ tr/SEARCH/REPLACE/flags`: the new string and the count.
pub fn tr(s: &[u8], search: &[u8], replace: &[u8], flags: &str) -> (Vec<u8>, usize) {
    Tr::new(search, replace, flags).apply(s, flags.contains('s'))
}

/// `tr///` on a character string (its UTF-8 form), with lists of
/// characters below 256: those characters as for bytes; a character above
/// 0xFF is kept, or, with `c` (it is in the complement), deleted with `d`
/// or replaced by the last character of the replacement list.
pub fn tr_chars(s: &[u8], search: &[u8], replace: &[u8], flags: &str) -> (Vec<u8>, usize) {
    let t = Tr::new(search, replace, flags);
    let squeeze = flags.contains('s');
    let wide = if flags.contains('c') {
        if flags.contains('d') {
            Action::Delete
        } else {
            match expand(replace).last() {
                Some(&r) => Action::Replace(r),
                None => Action::Keep,
            }
        }
    } else {
        Action::Keep
    };
    let counted_wide = flags.contains('c');
    let mut out = Vec::with_capacity(s.len());
    let mut count = 0;
    let mut last_replaced: Option<u8> = None;
    let mut i = 0;
    while i < s.len() {
        let (cp, n) = crate::perl::utf8::decode_at(s, i);
        let action = if cp < 256 { t.map[cp as usize] } else { wide };
        match action {
            Action::Keep => {
                if cp >= 256 && counted_wide {
                    count += 1;
                }
                out.extend_from_slice(&s[i..i + n]);
                last_replaced = None;
            }
            Action::Delete => count += 1,
            Action::Replace(r) => {
                count += 1;
                if !(squeeze && last_replaced == Some(r)) {
                    crate::perl::utf8::push_cp(&mut out, r as u32);
                    last_replaced = Some(r);
                }
            }
        }
        i += n;
    }
    (out, count)
}

#[cfg(test)]
mod tests {
    use super::tr;

    #[test]
    fn forms() {
        assert_eq!(tr(b"a\0b\0", b"\\0", b"", "d").0, b"ab");
        assert_eq!(tr(b"Ab-1_x!", b"-_a-zA-Z0-9", b"", "dc").0, b"Ab-1_x");
        assert_eq!(tr(b"1,2,3", b",", b" ", "").0, b"1 2 3");
        assert_eq!(tr(b"1,2,3", b",", b"", "").1, 2);
        assert_eq!(tr(b"aabbcc", b"a-c", b"A-C", "s").0, b"ABC");
        assert_eq!(tr(b"hello", b"a-y", b"b-z", "").0, b"ifmmp");
        assert_eq!(tr(b"a-b", b"\\-", b"_", "").0, b"a_b");
    }
}
