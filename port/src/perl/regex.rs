//! Perl regular expressions on bytes, through PCRE2.
//!
//! ExifTool matches Perl patterns against raw file data (`/s` everywhere,
//! backreferences, lookahead, `\G` with `pos()`). PCRE2 implements Perl's
//! syntax and semantics; this module only adapts its API to Perl's: flags,
//! a start offset for `\G` and `/g`, and captures. Compiled patterns are
//! cached, as perl caches a constant pattern.

use std::cell::RefCell;
type HashMap<K, V> = rustc_hash::FxHashMap<K, V>;
use std::rc::Rc;

/// A compiled pattern.
pub struct PerlRegex {
    re: pcre2::bytes::Regex,
    /// Number of capture groups (without group 0).
    pub groups: usize,
    /// Named groups and their index, for `%+`.
    pub names: Vec<Option<String>>,
    /// Compiled in UTF mode, for a character string subject (given in its
    /// UTF-8 form): offsets are still byte offsets in that form.
    pub utf: bool,
    /// The pattern and flags it was compiled from (to compile it again in
    /// UTF mode for a character string).
    pub source: Vec<u8>,
    pub flags: String,
    /// The match data of the last `match_at`, kept for the next one: PCRE2
    /// match data (and its match context) made for each match were a
    /// malloc and a free each.
    locs: RefCell<Option<pcre2::bytes::CaptureLocations>>,
}

impl PerlRegex {
    /// The same pattern for a character string subject: UTF mode.
    pub fn for_chars(self: &Rc<Self>) -> Rc<PerlRegex> {
        if self.utf {
            return self.clone();
        }
        compile_utf(&self.source, false, &self.flags).unwrap_or_else(|_| self.clone())
    }
}

/// The spans of a match: group 0 then each capture group (`None` when the
/// group did not take part).
pub type Spans = Vec<Option<(usize, usize)>>;

thread_local! {
    /// The compiled patterns, by pattern then flags (looked up without
    /// copying the pattern).
    static CACHE: RefCell<HashMap<Vec<u8>, Vec<(String, Rc<PerlRegex>)>>> = RefCell::new(HashMap::default());
    /// The patterns compiled in UTF mode, by pattern, then whether the
    /// pattern is a character string (its UTF-8 form) and flags.
    static UTF_CACHE: RefCell<HashMap<Vec<u8>, Vec<(bool, String, Rc<PerlRegex>)>>> = RefCell::new(HashMap::default());
    /// The patterns of `compile_few`, by pattern then flags.
    static FEW_CACHE: RefCell<HashMap<Vec<u8>, Vec<(String, Rc<PerlRegex>)>>> = RefCell::new(HashMap::default());
}

/// `compile` without JIT, for a pattern built at run time that is matched
/// a few times only (one per XMP property name): JIT compiling it cost
/// more than the matches it would speed up.
pub fn compile_few(pattern: &[u8], flags: &str) -> Result<Rc<PerlRegex>, String> {
    let hit = FEW_CACHE.with(|c| c.borrow().get(pattern).and_then(|v| v.iter().find(|(f, _)| f == flags).map(|(_, re)| re.clone())));
    if let Some(re) = hit {
        return Ok(re);
    }
    let compiled = build_jit(pattern, false, flags, false, false)?;
    FEW_CACHE.with(|c| c.borrow_mut().entry(pattern.to_vec()).or_default().push((flags.to_string(), compiled.clone())));
    Ok(compiled)
}

/// Compile `pattern` with Perl modifiers (`i`, `m`, `s`, `x`; others such as
/// `g`, `e`, `o`, `r` are the caller's business and ignored here).
pub fn compile(pattern: &[u8], flags: &str) -> Result<Rc<PerlRegex>, String> {
    let hit = CACHE.with(|c| c.borrow().get(pattern).and_then(|v| v.iter().find(|(f, _)| f == flags).map(|(_, re)| re.clone())));
    if let Some(re) = hit {
        return Ok(re);
    }
    let compiled = build(pattern, false, flags, false)?;
    CACHE.with(|c| c.borrow_mut().entry(pattern.to_vec()).or_default().push((flags.to_string(), compiled.clone())));
    Ok(compiled)
}

/// Compile a pattern for a character string subject (perl's matching on
/// a string with the UTF-8 flag): UTF mode, with Unicode's classes and
/// case folding, as perl's rules for such strings. `pat_utf8`: the
/// pattern is itself a character string, given in its UTF-8 form.
pub fn compile_utf(pattern: &[u8], pat_utf8: bool, flags: &str) -> Result<Rc<PerlRegex>, String> {
    let hit = UTF_CACHE.with(|c| c.borrow().get(pattern).and_then(|v| v.iter().find(|(u, f, _)| *u == pat_utf8 && f == flags).map(|(_, _, re)| re.clone())));
    if let Some(re) = hit {
        return Ok(re);
    }
    let compiled = build(pattern, pat_utf8, flags, true)?;
    UTF_CACHE.with(|c| c.borrow_mut().entry(pattern.to_vec()).or_default().push((pat_utf8, flags.to_string(), compiled.clone())));
    Ok(compiled)
}

/// Whether a pattern (bytes) names a character above 0xFF (`\x{100}`,
/// `\N{U+...}`), which makes perl match in Unicode (the subject upgraded).
pub fn needs_utf(pattern: &[u8]) -> bool {
    let mut i = 0;
    while i + 1 < pattern.len() {
        if pattern[i] == b'\\' {
            let e = pattern[i + 1];
            if (e == b'x' || e == b'o') && pattern.get(i + 2) == Some(&b'{') {
                if let Some(end) = pattern[i + 3..].iter().position(|&b| b == b'}') {
                    let t = String::from_utf8_lossy(&pattern[i + 3..i + 3 + end]).replace('_', "");
                    let v = u32::from_str_radix(t.trim(), if e == b'x' { 16 } else { 8 }).unwrap_or(0);
                    if v > 0xFF {
                        return true;
                    }
                }
            } else if e == b'N' && pattern.get(i + 2) == Some(&b'{') {
                return true;
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    false
}

fn build(pattern: &[u8], pat_utf8: bool, flags: &str, utf: bool) -> Result<Rc<PerlRegex>, String> {
    build_jit(pattern, pat_utf8, flags, utf, true)
}

fn build_jit(pattern: &[u8], pat_utf8: bool, flags: &str, utf: bool, jit: bool) -> Result<Rc<PerlRegex>, String> {
    let fixed = class_range_escapes(&never_quantifiers(pattern));
    // what perl allows and PCRE2 only with an option the crate does not
    // expose, set inline: duplicate group names (perl accepts
    // (?<n>a)|(?<n>b)) and /xx (spaces and tabs ignored in classes too)
    let mut prefix = String::new();
    if [&b"(?<"[..], b"(?'", b"(?P<"].iter().any(|n| fixed.windows(n.len()).any(|w| w == *n)) {
        prefix.push_str("(?J)");
    }
    if flags.matches('x').count() >= 2 {
        prefix.push_str("(?xx)");
    }
    let text = format!("{prefix}{}", to_pcre_pattern(&fixed, pat_utf8));
    let mut b = pcre2::bytes::RegexBuilder::new();
    b.caseless(flags.contains('i'))
        .multi_line(flags.contains('m'))
        .dotall(flags.contains('s'))
        .extended(flags.contains('x'))
        .utf(utf)
        .ucp(utf)
        .jit_if_available(jit);
    let re = b.build(&text).map_err(|e| perl_message_at(pattern, &e, prefix.len()))?;
    let names = re.capture_names().to_vec();
    let groups = re.captures_len().saturating_sub(1);
    Ok(Rc::new(PerlRegex { re, groups, names, utf, source: pattern.to_vec(), flags: flags.to_string(), locs: RefCell::new(None) }))
}

/// In a character class, perl takes a `-` next to a class escape
/// (`[a-\d]`, `[\d-z]`, `[\w-\s]`) as a literal hyphen (warning under
/// -w); PCRE2 refuses the range: the hyphen is escaped.
fn class_range_escapes(p: &[u8]) -> Vec<u8> {
    // a class escape: \d \D \w \W \s \S \h \H \v \V, or [:name:]
    let is_class_escape = |p: &[u8], i: usize| -> Option<usize> {
        match (p.get(i), p.get(i + 1)) {
            (Some(b'\\'), Some(c)) if b"dDwWsShHvV".contains(c) => Some(2),
            (Some(b'['), Some(b':')) => p[i + 2..].windows(2).position(|w| w == b":]").map(|k| k + 4),
            _ => None,
        }
    };
    let mut out = Vec::with_capacity(p.len() + 4);
    let (mut i, mut class) = (0, false);
    // the previous item in the class was a class escape
    let mut after_escape = false;
    while i < p.len() {
        let c = p[i];
        if !class {
            if c == b'\\' && i + 1 < p.len() {
                out.extend_from_slice(&p[i..i + 2]);
                i += 2;
                continue;
            }
            if c == b'[' {
                class = true;
                after_escape = false;
                out.push(c);
                i += 1;
                // a ] (or ^]) first is literal
                if p.get(i) == Some(&b'^') {
                    out.push(b'^');
                    i += 1;
                }
                if p.get(i) == Some(&b']') {
                    out.push(b']');
                    i += 1;
                }
                continue;
            }
            out.push(c);
            i += 1;
            continue;
        }
        if c == b']' {
            class = false;
            out.push(c);
            i += 1;
            continue;
        }
        if let Some(n) = is_class_escape(p, i) {
            out.extend_from_slice(&p[i..i + n]);
            i += n;
            after_escape = true;
            continue;
        }
        if c == b'-' && p.get(i + 1) != Some(&b']') && (after_escape || is_class_escape(p, i + 1).is_some()) {
            out.extend_from_slice(b"\\-");
            i += 1;
            after_escape = false;
            continue;
        }
        if c == b'\\' && i + 1 < p.len() {
            out.extend_from_slice(&p[i..i + 2]);
            i += 2;
        } else {
            out.push(c);
            i += 1;
        }
        after_escape = false;
    }
    out
}

/// perl accepts `X{n,m}` with n > m (a quantifier that cannot match, a
/// warning under -w); PCRE2 refuses it: it becomes `X{0}(?!)`.
fn never_quantifiers(p: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(p.len());
    let (mut i, mut class) = (0, false);
    while i < p.len() {
        let c = p[i];
        if c == b'\\' && i + 1 < p.len() {
            out.extend_from_slice(&p[i..i + 2]);
            i += 2;
            continue;
        }
        if class {
            if c == b']' {
                class = false;
            }
        } else if c == b'[' {
            class = true;
            if p.get(i + 1) == Some(&b']') || (p.get(i + 1) == Some(&b'^') && p.get(i + 2) == Some(&b']')) {
                // a ] first in the class is literal
                let skip = if p[i + 1] == b'^' { 3 } else { 2 };
                out.extend_from_slice(&p[i..i + skip]);
                i += skip;
                continue;
            }
        } else if c == b'{' {
            let rest = &p[i + 1..];
            let close = rest.iter().position(|&b| b == b'}');
            if let Some(e) = close {
                let inner = &rest[..e];
                if let Some(k) = inner.iter().position(|&b| b == b',') {
                    let (a, b) = (&inner[..k], &inner[k + 1..]);
                    let num = |x: &[u8]| (!x.is_empty() && x.iter().all(u8::is_ascii_digit)).then(|| String::from_utf8_lossy(x).parse::<u64>().ok()).flatten();
                    if let (Some(n), Some(m)) = (num(a), num(b)) {
                        if n > m {
                            out.extend_from_slice(b"{0}(?!)");
                            i += e + 2;
                            continue;
                        }
                    }
                }
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// `perl_message` for a pattern compiled with `shift` bytes of inline
/// options before it.
fn perl_message_at(pattern: &[u8], e: &pcre2::Error, shift: usize) -> String {
    perl_message_off(pattern, e, e.offset().map(|o| o.saturating_sub(shift)))
}

/// perl's message for a pattern PCRE2 refuses: perl's wording and its
/// "<-- HERE" mark where perl puts it, for the common errors; PCRE2's
/// wording at PCRE2's position otherwise.
pub fn perl_message(pattern: &[u8], e: &pcre2::Error) -> String {
    perl_message_off(pattern, e, e.offset())
}

fn perl_message_off(pattern: &[u8], e: &pcre2::Error, offset: Option<usize>) -> String {
    let pat = String::from_utf8_lossy(pattern).into_owned();
    let len = pattern.len();
    // the PCRE2 message, from the Display text ("PCRE2: error compiling
    // pattern at offset N: MESSAGE")
    let full = e.to_string();
    let msg = full.splitn(2, ": ").nth(1).map(|r| r.split_once(": ").map(|(_, m)| m.to_string()).unwrap_or_else(|| r.to_string())).unwrap_or(full.clone());
    let off = offset.unwrap_or(len).min(len);
    let here = |at: usize, what: &str| {
        let at = at.min(len);
        format!("{what} in regex; marked by <-- HERE in m/{} <-- HERE {}/", String::from_utf8_lossy(&pattern[..at]), String::from_utf8_lossy(&pattern[at..]))
    };
    // the first group or class left open, scanning as perl's parser does
    let unclosed = |open: u8| -> Option<usize> {
        let (mut i, mut class, mut stack) = (0, None::<usize>, Vec::new());
        while i < len {
            let c = pattern[i];
            if c == b'\\' {
                i += 2;
                continue;
            }
            match class {
                Some(_) if c == b']' => class = None,
                Some(_) => {}
                None if c == b'[' => {
                    class = Some(i);
                    if matches!(pattern.get(i + 1), Some(b']')) {
                        i += 1;
                    }
                }
                None if c == b'(' => stack.push(i),
                None if c == b')' => {
                    stack.pop();
                }
                None => {}
            }
            i += 1;
        }
        if open == b'[' { class } else { stack.first().copied() }
    };
    let prev_quant = off > 0 && matches!(pattern.get(off - 1), Some(b'*' | b'+' | b'?' | b'}'));
    if msg.contains("missing closing parenthesis") {
        return here(unclosed(b'(').map(|p| p + 1).unwrap_or(off), "Unmatched (");
    }
    if msg.contains("unmatched closing parenthesis") {
        return here(off + 1, "Unmatched )");
    }
    if msg.contains("missing terminating ] for character class") {
        return here(unclosed(b'[').map(|p| p + 1).unwrap_or(off), "Unmatched [");
    }
    if msg.contains("quantifier does not follow a repeatable item") {
        return here(off + 1, if prev_quant { "Nested quantifiers" } else { "Quantifier follows nothing" });
    }
    if msg.contains("\\ at end of pattern") {
        return format!("Trailing \\ in regex m/{pat}/");
    }
    if msg.contains("range out of order in character class") {
        let a = off.saturating_sub(2);
        return here(off + 1, &format!("Invalid [] range \"{}\"", String::from_utf8_lossy(&pattern[a..(off + 1).min(len)])));
    }
    if msg.contains("unrecognized character after (? or (?-") {
        let ch = pattern.get(off).map(|&c| c as char).unwrap_or(' ');
        return here(off + 1, &format!("Sequence (?{ch}...) not recognized"));
    }
    if msg.contains("missing ) after (?# comment") {
        return format!("Sequence (?#... not terminated in regex m/{pat}/");
    }
    if msg.contains("subpattern name") && msg.contains("terminator") {
        return here(len, "Sequence (?<... not terminated");
    }
    if msg.contains("reference to non-existent subpattern") {
        let named = pat.contains("\\k");
        if named {
            // perl marks the end of the name
            let end = pattern[off..].iter().position(|c| matches!(c, b'>' | b'}' | b'\'')).map(|k| off + k).unwrap_or(len);
            return here(end, "Reference to nonexistent named group");
        }
        // perl marks the end of the group number
        let mut end = off;
        while end < len && !pattern[end].is_ascii_digit() {
            end += 1;
        }
        while end < len && pattern[end].is_ascii_digit() {
            end += 1;
        }
        return here(end, "Reference to nonexistent group");
    }
    if msg.contains("unknown POSIX class name") {
        let end = pattern[off..].windows(2).position(|w| w == b":]").map(|k| off + k + 2).unwrap_or(off);
        let start = pattern[..end.saturating_sub(2)].windows(2).rposition(|w| w == b"[:").unwrap_or(off);
        return here(end, &format!("POSIX class {} unknown", String::from_utf8_lossy(&pattern[start..end])));
    }
    if msg.contains("lookbehind assertion is not fixed length") {
        return format!("Lookbehind longer than 255 not implemented in regex m/{pat}/");
    }
    here(off, &msg)
}

/// PCRE2's builder takes text: bytes that are not valid UTF-8 (raw binary in
/// magic numbers, Latin-1 in patterns) become `\x{hh}` escapes, which mean
/// the same byte in non-UTF mode (and the same character in UTF mode). The
/// characters of a pattern that is a character string (`utf8`: its UTF-8
/// form) become `\x{...}` escapes too.
fn to_pcre_pattern(pattern: &[u8], utf8: bool) -> String {
    if let Ok(s) = std::str::from_utf8(pattern) {
        if !s.contains('\0') && s.is_ascii() {
            return s.to_string();
        }
    }
    let mut out = String::with_capacity(pattern.len() + 8);
    let mut i = 0;
    while i < pattern.len() {
        let b = pattern[i];
        if utf8 && b >= 0x80 {
            let (cp, n) = crate::perl::utf8::decode_at(pattern, i);
            out.push_str(&format!("\\x{{{cp:x}}}"));
            i += n;
            continue;
        }
        if utf8 && b == b'\\' && i + 1 < pattern.len() && pattern[i + 1] >= 0x80 {
            let (cp, n) = crate::perl::utf8::decode_at(pattern, i + 1);
            out.push_str(&format!("\\x{{{cp:x}}}"));
            i += 1 + n;
            continue;
        }
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
        let mut locs = self.locs.take().unwrap_or_else(|| self.re.capture_locations());
        let r = match self.re.captures_read_at(&mut locs, subject, start) {
            Ok(Some(_)) => {
                let spans = (0..=self.groups).map(|g| locs.get(g)).collect();
                Ok(Some(spans))
            }
            Ok(None) => Ok(None),
            // A match limit is reached on a pathological pattern; perl would
            // keep going, PCRE2 gives up. Report no match, as the safest.
            Err(e) => Err(format!("{e}")),
        };
        self.locs.replace(Some(locs));
        r
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
                start = s + crate::perl::utf8::step(subject, s, self.utf);
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
                start += crate::perl::utf8::step(subject, start, self.utf);
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

/// `split` with perl's rules (pp_split): a match must end past the current
/// position, captures are returned, trailing empty fields are removed when
/// there is no limit. `awk` is `split ' '` (leading whitespace skipped).
pub fn split(re: &PerlRegex, awk: bool, subject: &[u8], limit: i64) -> Result<Vec<crate::perl::Value>, String> {
    use crate::perl::Value;
    let mut fields: Vec<Value> = Vec::new();
    let mut s = 0usize;
    if awk {
        s = subject.iter().take_while(|c| c.is_ascii_whitespace()).count();
    }
    let len = subject.len();
    let mut remaining = if limit > 0 { limit } else { i64::MAX };
    while s < len {
        remaining -= 1;
        if remaining == 0 {
            break;
        }
        // The match must end after s.
        let mut from = s;
        let spans = loop {
            match re.match_at(subject, from)? {
                Some(sp) => {
                    let (_, e) = sp[0].unwrap();
                    if e > s {
                        break Some(sp);
                    }
                    from += crate::perl::utf8::step(subject, from, re.utf);
                    if from > len {
                        break None;
                    }
                }
                None => break None,
            }
        };
        let Some(spans) = spans else { break };
        let (ms, me) = spans[0].unwrap();
        fields.push(Value::string(&subject[s..ms], re.utf));
        for g in &spans[1..] {
            fields.push(match g {
                Some((a, b)) => Value::string(&subject[*a..*b], re.utf),
                None => Value::Undef,
            });
        }
        s = me;
    }
    if s < len || (!fields.is_empty() && limit != 0) {
        fields.push(Value::string(&subject[s.min(len)..], re.utf));
    }
    if limit == 0 {
        while fields.last().is_some_and(|v| v.is_undef() || v.to_bytes().is_empty()) {
            fields.pop();
        }
    }
    Ok(fields)
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

/// `quotemeta` of a character string (its UTF-8 form): ASCII as for bytes;
/// a character above 0x7F is quoted when it is pattern syntax, white
/// space, a control or default-ignorable (perl's rule for UTF-8 strings).
pub fn quotemeta_chars(s: &[u8]) -> Vec<u8> {
    let quoted = |c: u32| {
        matches!(c,
            0x80..=0xA7 | 0xA9 | 0xAB | 0xAC | 0xAD | 0xAE | 0xB0 | 0xB1 | 0xB6 | 0xBB | 0xBF | 0xD7 | 0xF7
            | 0x34F | 0x61C | 0x115F | 0x1160 | 0x1680 | 0x17B4 | 0x17B5 | 0x180B..=0x180F
            | 0x2000..=0x203E | 0x2041..=0x2053 | 0x2055..=0x206F | 0x2190..=0x245F | 0x2500..=0x2775 | 0x2794..=0x2BFF
            | 0x2E00..=0x2E7F | 0x3000..=0x3003 | 0x3008..=0x3020 | 0x3030 | 0x3164 | 0xFD3E | 0xFD3F
            | 0xFE00..=0xFE0F | 0xFE45 | 0xFE46 | 0xFEFF | 0xFFA0 | 0xFFF0..=0xFFF8 | 0x1BCA0..=0x1BCA3 | 0x1D173..=0x1D17A | 0xE0000..=0xE0FFF)
    };
    let mut out = Vec::with_capacity(s.len() * 2);
    let mut i = 0;
    while i < s.len() {
        let (c, n) = crate::perl::utf8::decode_at(s, i);
        if c < 0x80 {
            if !(s[i].is_ascii_alphanumeric() || s[i] == b'_') {
                out.push(b'\\');
            }
        } else if quoted(c) {
            out.push(b'\\');
        }
        out.extend_from_slice(&s[i..i + n]);
        i += n;
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
