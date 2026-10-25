//! Perl regular expressions on bytes, through PCRE2.
//!
//! ExifTool matches Perl patterns against raw file data (`/s` everywhere,
//! backreferences, lookahead, `\G` with `pos()`). PCRE2 implements Perl's
//! syntax and semantics; this module only adapts its API to Perl's: flags,
//! a start offset for `\G` and `/g`, and captures. Compiled patterns are
//! cached, as perl caches a constant pattern.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// A compiled pattern.
pub struct PerlRegex {
    re: pcre2::bytes::Regex,
    /// Number of capture groups (without group 0).
    pub groups: usize,
    /// Named groups and their index, for `%+`.
    pub names: Vec<Option<String>>,
}

/// The spans of a match: group 0 then each capture group (`None` when the
/// group did not take part).
pub type Spans = Vec<Option<(usize, usize)>>;

thread_local! {
    static CACHE: RefCell<HashMap<(Vec<u8>, String), Rc<PerlRegex>>> = RefCell::new(HashMap::new());
}

/// Compile `pattern` with Perl modifiers (`i`, `m`, `s`, `x`; others such as
/// `g`, `e`, `o`, `r` are the caller's business and ignored here).
pub fn compile(pattern: &[u8], flags: &str) -> Result<Rc<PerlRegex>, String> {
    let key = (pattern.to_vec(), flags.to_string());
    if let Some(re) = CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return Ok(re);
    }
    let text = to_pcre_pattern(pattern);
    let mut b = pcre2::bytes::RegexBuilder::new();
    b.caseless(flags.contains('i'))
        .multi_line(flags.contains('m'))
        .dotall(flags.contains('s'))
        .extended(flags.contains('x'))
        .utf(false)
        .ucp(false)
        .jit_if_available(true);
    let re = b.build(&text).map_err(|e| format!("{e}"))?;
    let names = re.capture_names().to_vec();
    let groups = re.captures_len().saturating_sub(1);
    let compiled = Rc::new(PerlRegex { re, groups, names });
    CACHE.with(|c| c.borrow_mut().insert(key, compiled.clone()));
    Ok(compiled)
}

/// PCRE2's builder takes text: bytes that are not valid UTF-8 (raw binary in
/// magic numbers, Latin-1 in patterns) become `\x{hh}` escapes, which mean
/// the same byte in non-UTF mode.
fn to_pcre_pattern(pattern: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(pattern) {
        if !s.contains('\0') && s.is_ascii() {
            return s.to_string();
        }
    }
    let mut out = String::with_capacity(pattern.len() + 8);
    let mut i = 0;
    while i < pattern.len() {
        let b = pattern[i];
        // An escaped raw byte (quotemeta puts a backslash before NUL and
        // high bytes): the \x{hh} form replaces both.
        if b == b'\\' && i + 1 < pattern.len() && (pattern[i + 1] == 0 || pattern[i + 1] >= 0x80) {
            out.push_str(&format!("\\x{{{:02x}}}", pattern[i + 1]));
            i += 2;
            continue;
        }
        if b == b'\\' && i + 1 < pattern.len() {
            out.push('\\');
            out.push(pattern[i + 1] as char);
            i += 2;
            continue;
        }
        if b == 0 || b >= 0x80 {
            out.push_str(&format!("\\x{{{b:02x}}}"));
        } else {
            out.push(b as char);
        }
        i += 1;
    }
    out
}

impl PerlRegex {
    /// Match starting the search at `start` (lookbehind and `\G` see the
    /// text before it, as with perl's `pos()`).
    pub fn match_at(&self, subject: &[u8], start: usize) -> Result<Option<Spans>, String> {
        if start > subject.len() {
            return Ok(None);
        }
        let mut locs = self.re.capture_locations();
        match self.re.captures_read_at(&mut locs, subject, start) {
            Ok(Some(_)) => {
                let spans = (0..=self.groups).map(|g| locs.get(g)).collect();
                Ok(Some(spans))
            }
            Ok(None) => Ok(None),
            // A match limit is reached on a pathological pattern; perl would
            // keep going, PCRE2 gives up. Report no match, as the safest.
            Err(e) => Err(format!("{e}")),
        }
    }

    pub fn is_match(&self, subject: &[u8]) -> bool {
        self.re.is_match(subject).unwrap_or(false)
    }

    /// All non-overlapping matches from `start`, as perl's `/g` in list
    /// context: after an empty match the next search starts one byte later.
    pub fn match_all(&self, subject: &[u8], mut start: usize) -> Result<Vec<Spans>, String> {
        let mut all = Vec::new();
        let mut last_empty_at: Option<usize> = None;
        while start <= subject.len() {
            let Some(spans) = self.match_at(subject, start)? else { break };
            let (s, e) = spans[0].unwrap();
            if s == e && last_empty_at == Some(s) {
                // perl does not return a second empty match at the same place.
                start = s + 1;
                last_empty_at = None;
                continue;
            }
            all.push(spans);
            if s == e {
                last_empty_at = Some(e);
                start = e;
                if start >= subject.len() {
                    break;
                }
                start += 1;
                // Retry at e+1 is what perl does after an empty match only if
                // no non-empty match starts at e; PCRE2 with NOTEMPTY_ATSTART
                // would be exact. ExifTool's patterns never match empty in /g.
            } else {
                start = e;
                last_empty_at = None;
            }
        }
        Ok(all)
    }
}

/// `quotemeta`: backslash every byte that is not a word character.
pub fn quotemeta(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 2);
    for &b in s {
        if !(b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80) {
            out.push(b'\\');
        }
        out.push(b);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_and_flags() {
        let re = compile(b"^\xff\xd8\xff", "s").unwrap();
        assert!(re.match_at(b"\xff\xd8\xff\xe0", 0).unwrap().is_some());
        let re = compile(b"^II\\x2a\\0", "s").unwrap();
        assert!(re.match_at(b"II*\0\x08", 0).unwrap().is_some());
        let re = compile(b"eos 5d", "i").unwrap();
        assert!(re.is_match(b"Canon EOS 5D Mark II"));
        let re = compile(b"(\\d+)-(\\d+)", "").unwrap();
        let m = re.match_at(b"ab 12-34", 0).unwrap().unwrap();
        assert_eq!(m[1], Some((3, 5)));
        assert_eq!(m[2], Some((6, 8)));
        // Backreference and lookahead.
        let re = compile(b"(.)\\1(?=x)", "").unwrap();
        assert!(re.is_match(b"aax"));
        assert!(!re.is_match(b"aay"));
        // $ matches before a trailing newline, as in perl.
        let re = compile(b"abc$", "").unwrap();
        assert!(re.is_match(b"abc\n"));
        // \G anchors at the start offset.
        let re = compile(b"\\G(\\w)", "").unwrap();
        assert_eq!(re.match_at(b"ab cd", 3).unwrap().unwrap()[1], Some((3, 4)));
        assert!(re.match_at(b"ab cd", 2).unwrap().is_none());
    }

    #[test]
    fn global() {
        let re = compile(b"\\d+", "").unwrap();
        let all = re.match_all(b"1 22 333", 0).unwrap();
        assert_eq!(all.len(), 3);
    }
}
