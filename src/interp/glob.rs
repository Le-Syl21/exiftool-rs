//! File name globbing as perl's File::Glob does it: `glob` (CORE::glob,
//! csh_glob: words split on white space, double quotes group a word) and
//! `File::Glob::bsd_glob` (one pattern), both with GLOB_CSH: braces, `~`,
//! backslash quoting, a pattern without wildcards returned as it is, and
//! each pattern's matches sorted case-insensitively.

use std::path::Path;

/// `glob EXPR`: the words of the pattern, each globbed.
pub fn csh_glob(pat: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for w in split_words(pat) {
        out.extend(bsd_glob(&w));
    }
    out
}

fn split_words(s: &[u8]) -> Vec<Vec<u8>> {
    let mut words = Vec::new();
    let mut cur = Vec::new();
    let (mut in_q, mut have) = (false, false);
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c == b'\\' && i + 1 < s.len() {
            cur.push(c);
            cur.push(s[i + 1]);
            have = true;
            i += 2;
            continue;
        }
        if c == b'"' {
            in_q = !in_q;
            have = true;
        } else if c.is_ascii_whitespace() && !in_q {
            if have {
                words.push(std::mem::take(&mut cur));
                have = false;
            }
        } else {
            // inside quotes, wildcards are literal
            if in_q && matches!(c, b'*' | b'?' | b'[' | b'{' | b'~') {
                cur.push(b'\\');
            }
            cur.push(c);
            have = true;
        }
        i += 1;
    }
    if have {
        words.push(cur);
    }
    words
}

/// `File::Glob::bsd_glob($pattern)`.
pub fn bsd_glob(pat: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for p in expand_braces(pat) {
        let p = expand_tilde(&p);
        if !has_magic(&p) {
            out.push(unquote(&p));
            continue;
        }
        let mut found = Vec::new();
        let abs = p.first() == Some(&b'/');
        let comps: Vec<&[u8]> = p.split(|&c| c == b'/').filter(|c| !c.is_empty()).collect();
        walk(if abs { b"/".to_vec() } else { Vec::new() }, &comps, &mut found);
        found.sort_by(|a, b| a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase()).then(a.cmp(b)));
        out.extend(found);
    }
    out
}

fn has_magic(p: &[u8]) -> bool {
    let mut i = 0;
    while i < p.len() {
        match p[i] {
            b'\\' => i += 1,
            b'*' | b'?' | b'[' => return true,
            _ => {}
        }
        i += 1;
    }
    false
}

fn unquote(p: &[u8]) -> Vec<u8> {
    let mut o = Vec::with_capacity(p.len());
    let mut i = 0;
    while i < p.len() {
        if p[i] == b'\\' && i + 1 < p.len() {
            i += 1;
        }
        o.push(p[i]);
        i += 1;
    }
    o
}

fn expand_tilde(p: &[u8]) -> Vec<u8> {
    if p.first() != Some(&b'~') || !(p.len() == 1 || p[1] == b'/') {
        return p.to_vec();
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let mut o = home.into_bytes();
    o.extend_from_slice(&p[1..]);
    o
}

/// `a{b,c}d` → `abd`, `acd` (nested braces included; `{}` is literal).
fn expand_braces(p: &[u8]) -> Vec<Vec<u8>> {
    let mut i = 0;
    while i < p.len() {
        match p[i] {
            b'\\' => i += 1,
            b'{' if p.get(i + 1) != Some(&b'}') => {
                let mut depth = 0;
                let mut j = i;
                let mut alts = Vec::new();
                let mut start = i + 1;
                while j < p.len() {
                    match p[j] {
                        b'\\' => j += 1,
                        b'{' => depth += 1,
                        b'}' => {
                            depth -= 1;
                            if depth == 0 {
                                alts.push(&p[start..j]);
                                let mut out = Vec::new();
                                for a in alts {
                                    let mut s = p[..i].to_vec();
                                    s.extend_from_slice(a);
                                    s.extend_from_slice(&p[j + 1..]);
                                    out.extend(expand_braces(&s));
                                }
                                return out;
                            }
                        }
                        b',' if depth == 1 => {
                            alts.push(&p[start..j]);
                            start = j + 1;
                        }
                        _ => {}
                    }
                    j += 1;
                }
                return vec![p.to_vec()];
            }
            _ => {}
        }
        i += 1;
    }
    vec![p.to_vec()]
}

fn walk(base: Vec<u8>, comps: &[&[u8]], out: &mut Vec<Vec<u8>>) {
    let Some((first, rest)) = comps.split_first() else {
        out.push(base);
        return;
    };
    let join = |name: &[u8]| {
        let mut p = base.clone();
        if !p.is_empty() && p.last() != Some(&b'/') {
            p.push(b'/');
        }
        p.extend_from_slice(name);
        p
    };
    if !has_magic(first) {
        let p = join(&unquote(first));
        if rest.is_empty() {
            if Path::new(&*String::from_utf8_lossy(&p)).symlink_metadata().is_ok() {
                out.push(p);
            }
        } else {
            walk(p, rest, out);
        }
        return;
    }
    let dir = if base.is_empty() { ".".to_string() } else { String::from_utf8_lossy(&base).into_owned() };
    let Ok(rd) = std::fs::read_dir(&dir) else { return };
    // readdir also lists "." and ".."
    let mut names: Vec<Vec<u8>> = vec![b".".to_vec(), b"..".to_vec()];
    names.extend(rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned().into_bytes()));
    for n in names {
        if n.first() == Some(&b'.') && first.first() != Some(&b'.') {
            continue;
        }
        if fnmatch(first, &n) {
            let p = join(&n);
            if rest.is_empty() {
                out.push(p);
            } else if Path::new(&*String::from_utf8_lossy(&p)).is_dir() {
                walk(p, rest, out);
            }
        }
    }
}

/// `*`, `?`, `[...]` (with `!` or `^` negation and ranges), `\` quoting.
fn fnmatch(p: &[u8], s: &[u8]) -> bool {
    let (mut pi, mut si) = (0, 0);
    let (mut star_p, mut star_s) = (usize::MAX, 0);
    while si < s.len() {
        if pi < p.len() {
            match p[pi] {
                b'*' => {
                    star_p = pi;
                    star_s = si;
                    pi += 1;
                    continue;
                }
                b'?' => {
                    pi += 1;
                    si += 1;
                    continue;
                }
                b'[' => {
                    if let Some((ok, end)) = class(p, pi, s[si]) {
                        if ok {
                            pi = end;
                            si += 1;
                            continue;
                        }
                    } else if s[si] == b'[' {
                        pi += 1;
                        si += 1;
                        continue;
                    }
                }
                b'\\' if pi + 1 < p.len() && p[pi + 1] == s[si] => {
                    pi += 2;
                    si += 1;
                    continue;
                }
                c if c == s[si] && c != b'\\' => {
                    pi += 1;
                    si += 1;
                    continue;
                }
                _ => {}
            }
        }
        if star_p != usize::MAX {
            pi = star_p + 1;
            star_s += 1;
            si = star_s;
            continue;
        }
        return false;
    }
    while pi < p.len() && p[pi] == b'*' {
        pi += 1;
    }
    pi == p.len()
}

/// A `[...]` class at p[i]: (matches c, index after the class).
fn class(p: &[u8], i: usize, c: u8) -> Option<(bool, usize)> {
    let mut j = i + 1;
    let neg = matches!(p.get(j), Some(b'!') | Some(b'^'));
    if neg {
        j += 1;
    }
    let mut hit = false;
    let mut first = true;
    while j < p.len() {
        if p[j] == b']' && !first {
            return Some((hit != neg, j + 1));
        }
        first = false;
        let lo = if p[j] == b'\\' && j + 1 < p.len() {
            j += 1;
            p[j]
        } else {
            p[j]
        };
        if p.get(j + 1) == Some(&b'-') && p.get(j + 2).is_some_and(|&x| x != b']') {
            let hi = p[j + 2];
            if lo <= c && c <= hi {
                hit = true;
            }
            j += 3;
        } else {
            if lo == c {
                hit = true;
            }
            j += 1;
        }
    }
    None
}
