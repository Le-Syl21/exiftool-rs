//! PDF metadata writer: the Info dictionary.
//!
//! Follows ExifTool's `WritePDF` (WritePDF.pl): the file is never rewritten,
//! an *incremental update* is appended instead -- the new Info dictionary
//! as a new revision of its object (or a new object when there was none), a
//! cross-reference section for it, and a trailer pointing back at the
//! previous one. Everything before the update stays byte for byte, so the
//! update can be reverted (ExifTool's `-PDF-update:all=`), and it is framed
//! with the same `%BeginExifToolUpdate` / `%EndExifToolUpdate` comments.
//!
//! Encrypted files, and files whose Info dictionary sits in a compressed
//! object stream, are refused rather than written wrong.

use crate::error::{Error, Result};

const BEGIN_COMMENT: &[u8] = b"%BeginExifToolUpdate";
const END_COMMENT: &[u8] = b"%EndExifToolUpdate ";

/// The PDF Info part of a write request.
#[derive(Debug, Clone, Default)]
pub struct PdfChanges {
    /// `PDF:all`: drop the whole Info dictionary.
    pub delete_all: bool,
    /// Info keys to delete (`Title`, ...).
    pub delete: Vec<String>,
    /// Info keys to write, with their text value.
    pub set: Vec<(String, String)>,
}

impl PdfChanges {
    pub fn is_empty(&self) -> bool {
        !self.delete_all && self.delete.is_empty() && self.set.is_empty()
    }
}

/// The Info key a tag name writes, and how its value is formatted.
fn info_key(tag: &str) -> Option<&'static str> {
    Some(match tag.to_ascii_lowercase().as_str() {
        "title" => "Title",
        "author" => "Author",
        "subject" => "Subject",
        "keywords" => "Keywords",
        "creator" => "Creator",
        "producer" => "Producer",
        _ => return None,
    })
}

/// Can `tag` be written to the PDF Info dictionary?
pub fn is_info_tag(tag: &str) -> bool {
    info_key(tag).is_some()
}

// ── lexing ───────────────────────────────────────────────────────────────────

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b'\x0c' | 0)
}

fn is_delim(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

fn skip_ws(d: &[u8], mut p: usize) -> usize {
    while p < d.len() {
        if is_ws(d[p]) {
            p += 1;
        } else if d[p] == b'%' {
            while p < d.len() && d[p] != b'\r' && d[p] != b'\n' {
                p += 1;
            }
        } else {
            break;
        }
    }
    p
}

/// The end of the object (value) starting at `p`.
fn object_end(d: &[u8], p: usize) -> Option<usize> {
    let c = *d.get(p)?;
    match c {
        b'(' => {
            let mut depth = 0usize;
            let mut i = p;
            while i < d.len() {
                match d[i] {
                    b'\\' => i += 1,
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(i + 1);
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            None
        }
        b'<' if d.get(p + 1) == Some(&b'<') => {
            let mut i = skip_ws(d, p + 2);
            loop {
                if d.get(i..i + 2)? == b">>" {
                    return Some(i + 2);
                }
                i = skip_ws(d, object_end(d, i)?);
            }
        }
        b'<' => Some(p + d[p..].iter().position(|&b| b == b'>')? + 1),
        b'[' => {
            let mut i = skip_ws(d, p + 1);
            loop {
                if *d.get(i)? == b']' {
                    return Some(i + 1);
                }
                i = skip_ws(d, object_end(d, i)?);
            }
        }
        b'/' => {
            let mut i = p + 1;
            while i < d.len() && !is_ws(d[i]) && !is_delim(d[i]) {
                i += 1;
            }
            Some(i)
        }
        b')' | b'>' | b']' | b'{' | b'}' => None,
        _ => {
            // Number or keyword; `N G R` is one object.
            let word = |i: usize| -> usize {
                let mut j = i;
                while j < d.len() && !is_ws(d[j]) && !is_delim(d[j]) {
                    j += 1;
                }
                j
            };
            let e = word(p);
            if e == p {
                return None;
            }
            let digits = |a: usize, b: usize| a < b && d[a..b].iter().all(u8::is_ascii_digit);
            if digits(p, e) {
                let g = skip_ws(d, e);
                let ge = word(g);
                if digits(g, ge) {
                    let r = skip_ws(d, ge);
                    if d.get(r) == Some(&b'R')
                        && d.get(r + 1).is_none_or(|&b| is_ws(b) || is_delim(b))
                    {
                        return Some(r + 1);
                    }
                }
            }
            Some(e)
        }
    }
}

/// A dictionary: its entries as raw (key, value) text, in file order.
type Dict = Vec<(Vec<u8>, Vec<u8>)>;

/// Parse the dictionary at `p`: its entries and where it ends.
fn parse_dict(d: &[u8], p: usize) -> Option<(Dict, usize)> {
    if d.get(p..p + 2)? != b"<<" {
        return None;
    }
    let mut out = Vec::new();
    let mut i = skip_ws(d, p + 2);
    loop {
        if d.get(i..i + 2)? == b">>" {
            return Some((out, i + 2));
        }
        if d[i] != b'/' {
            return None;
        }
        let ke = object_end(d, i)?;
        let key = d[i + 1..ke].to_vec();
        let v = skip_ws(d, ke);
        let ve = object_end(d, v)?;
        out.push((key, d[v..ve].to_vec()));
        i = skip_ws(d, ve);
    }
}

fn get<'a>(dict: &'a [(Vec<u8>, Vec<u8>)], key: &[u8]) -> Option<&'a [u8]> {
    dict.iter()
        .find(|(k, _)| k.as_slice() == key)
        .map(|(_, v)| v.as_slice())
}

fn set(dict: &mut Vec<(Vec<u8>, Vec<u8>)>, key: &[u8], value: Vec<u8>) {
    match dict.iter_mut().find(|(k, _)| k.as_slice() == key) {
        Some(e) => e.1 = value,
        None => dict.push((key.to_vec(), value)),
    }
}

/// `N G R` → (N, G).
fn parse_ref(v: &[u8]) -> Option<(u32, u32)> {
    let s = std::str::from_utf8(v).ok()?;
    let mut it = s.split_ascii_whitespace();
    let n = it.next()?.parse().ok()?;
    let g = it.next()?.parse().ok()?;
    (it.next()? == "R").then_some((n, g))
}

fn parse_usize(v: &[u8]) -> Option<usize> {
    std::str::from_utf8(v).ok()?.trim().parse().ok()
}

/// The offset of the last `N G obj` header (at a token boundary).
fn find_object(d: &[u8], n: u32, g: u32) -> Option<usize> {
    let pat = format!("{n} {g} obj");
    let pat = pat.as_bytes();
    let mut end = d.len();
    while let Some(p) = d[..end].windows(pat.len()).rposition(|w| w == pat) {
        let before_ok = p == 0 || is_ws(d[p - 1]) || is_delim(d[p - 1]);
        let after_ok = d
            .get(p + pat.len())
            .is_none_or(|&b| is_ws(b) || is_delim(b));
        if before_ok && after_ok {
            return Some(p);
        }
        end = p;
    }
    None
}

// ── value encoding (WritePDF.pl WritePDFValue / EncodeString) ───────────────

fn encode_string(value: &str) -> Vec<u8> {
    let raw: Vec<u8> = if value.is_ascii() {
        value.as_bytes().to_vec()
    } else {
        // UCS-2 big-endian with a byte order mark.
        let mut v = vec![0xFE, 0xFF];
        for u in value.encode_utf16() {
            v.extend_from_slice(&u.to_be_bytes());
        }
        v
    };
    if raw
        .iter()
        .any(|&b| matches!(b, 0x00..=0x08 | 0x0a..=0x1f | 0x7f | 0xff))
    {
        let mut s = b"<".to_vec();
        for (i, chunk) in raw.chunks(40).enumerate() {
            if i > 0 {
                s.push(b'\n');
            }
            for b in chunk {
                s.extend_from_slice(format!("{b:02x}").as_bytes());
            }
        }
        s.push(b'>');
        s
    } else {
        let mut s = b"(".to_vec();
        for &b in &raw {
            if matches!(b, b'(' | b')' | b'\\') {
                s.push(b'\\');
            }
            s.push(b);
        }
        s.push(b')');
        s
    }
}

// ── writing ─────────────────────────────────────────────────────────────────

fn write_dict(out: &mut Vec<u8>, nl: &[u8], dict: &[(Vec<u8>, Vec<u8>)]) {
    out.extend_from_slice(nl);
    out.extend_from_slice(b"<<");
    for (k, v) in dict {
        out.extend_from_slice(nl);
        out.push(b'/');
        out.extend_from_slice(k);
        out.push(b' ');
        out.extend_from_slice(v);
    }
    out.extend_from_slice(nl);
    out.extend_from_slice(b">>");
}

/// The trailer of the last revision: its dictionary, where its xref is,
/// and whether that xref is a stream.
struct Trailer {
    dict: Vec<(Vec<u8>, Vec<u8>)>,
    startxref: usize,
    xref_stream: bool,
}

fn last_trailer(d: &[u8], base: usize) -> Result<Trailer> {
    let bad = |m: &str| Error::InvalidData(format!("PDF: {m}"));
    let tail_start = d.len().saturating_sub(1024);
    let rel = d[tail_start..]
        .windows(9)
        .rposition(|w| w == b"startxref")
        .ok_or_else(|| bad("no startxref"))?;
    let p = skip_ws(d, tail_start + rel + 9);
    let e = object_end(d, p).ok_or_else(|| bad("bad startxref"))?;
    let startxref = parse_usize(&d[p..e]).ok_or_else(|| bad("bad startxref"))?;
    let x = skip_ws(d, startxref + base);
    if d.get(x..x + 4) == Some(b"xref") {
        // Classic table: the trailer keyword follows it.
        let t = d[x..]
            .windows(7)
            .position(|w| w == b"trailer")
            .map(|r| x + r)
            .ok_or_else(|| bad("no trailer"))?;
        let (dict, _) =
            parse_dict(d, skip_ws(d, t + 7)).ok_or_else(|| bad("bad trailer dictionary"))?;
        return Ok(Trailer {
            dict,
            startxref,
            xref_stream: false,
        });
    }
    // A cross-reference stream: `N G obj << /Type /XRef ... >> stream`.
    let mut i = x;
    for _ in 0..3 {
        i = skip_ws(d, object_end(d, i).ok_or_else(|| bad("bad xref"))?);
    }
    let (dict, _) = parse_dict(d, i).ok_or_else(|| bad("bad xref stream dictionary"))?;
    if get(&dict, b"Type") != Some(b"/XRef") {
        return Err(bad("startxref does not point at a cross-reference section"));
    }
    Ok(Trailer {
        dict,
        startxref,
        xref_stream: true,
    })
}

/// Append an incremental update with the Info changes. Returns the input
/// unchanged when the request changes nothing.
pub fn write_pdf(source: &[u8], changes: &PdfChanges) -> Result<Vec<u8>> {
    let start = source
        .windows(5)
        .position(|w| w == b"%PDF-")
        .filter(|&p| p < 1024 && source[..p].iter().all(|&b| is_ws(b)))
        .ok_or_else(|| Error::InvalidData("not a PDF file".into()))?;
    if changes.is_empty() {
        return Ok(source.to_vec());
    }
    let base = start;
    let trailer = last_trailer(source, base)?;
    if get(&trailer.dict, b"Encrypt").is_some() {
        return Err(Error::UnsupportedFileType(
            "writing encrypted PDF files is not supported".into(),
        ));
    }
    let size = get(&trailer.dict, b"Size")
        .and_then(parse_usize)
        .ok_or_else(|| Error::InvalidData("PDF trailer has no Size".into()))?;

    // The current Info dictionary.
    let info_ref = get(&trailer.dict, b"Info").map(|v| {
        parse_ref(v).ok_or_else(|| Error::InvalidData("PDF Info is not an indirect object".into()))
    });
    let info_ref = info_ref.transpose()?;
    let mut info: Vec<(Vec<u8>, Vec<u8>)> = match info_ref {
        Some((n, g)) => {
            let p = find_object(source, n, g).ok_or_else(|| {
                Error::UnsupportedFileType(
                    "the PDF Info dictionary is in a compressed object stream".into(),
                )
            })?;
            let mut i = p;
            for _ in 0..3 {
                i = skip_ws(source, object_end(source, i).unwrap_or(source.len()));
            }
            parse_dict(source, i)
                .ok_or_else(|| Error::InvalidData("bad PDF Info dictionary".into()))?
                .0
        }
        None => Vec::new(),
    };
    let before = info.clone();

    // Apply the changes: delete first, then write; new keys go last, in
    // alphabetical order, as ExifTool writes them.
    if changes.delete_all {
        info.clear();
    }
    for t in &changes.delete {
        if let Some(k) = info_key(t) {
            info.retain(|(key, _)| key.as_slice() != k.as_bytes());
        }
    }
    let mut new_keys: Vec<(&str, &str)> = Vec::new();
    for (t, v) in &changes.set {
        let Some(k) = info_key(t) else { continue };
        if info.iter().any(|(key, _)| key.as_slice() == k.as_bytes()) {
            set(&mut info, k.as_bytes(), encode_string(v));
        } else {
            new_keys.retain(|(nk, _)| *nk != k);
            new_keys.push((k, v));
        }
    }
    new_keys.sort();
    for (k, v) in new_keys {
        info.push((k.as_bytes().to_vec(), encode_string(v)));
    }
    if info == before {
        return Ok(source.to_vec());
    }

    // The line separator of the file's last cross-reference section.
    let nl: &[u8] = {
        let after = &source[(trailer.startxref + base).min(source.len())..];
        match after.iter().position(|&b| b == b'\r' || b == b'\n') {
            Some(i) if after[i] == b'\r' && after.get(i + 1) == Some(&b'\n') => b"\r\n",
            Some(i) if after[i] == b'\r' => b"\r",
            _ => b"\n",
        }
    };

    let mut out = source.to_vec();
    let old_eof = out.len() - base;
    out.extend_from_slice(BEGIN_COMMENT);

    // xref entries: (object, offset, generation, in use).
    let mut entries: Vec<(usize, usize, u32, bool)> = vec![(0, 0, 65535, false)];
    let mut next = size;
    let mut main = trailer.dict.clone();
    if info.is_empty() {
        // Nothing left: the Info object is freed and unlinked.
        main.retain(|(k, _)| k.as_slice() != b"Info");
        if let Some((n, g)) = info_ref {
            entries.push((n as usize, 0, g.saturating_add(1).min(65535), false));
        }
    } else {
        let (n, g) = match info_ref {
            Some(r) => r,
            None => {
                next += 1;
                (size as u32, 0)
            }
        };
        out.extend_from_slice(nl);
        entries.push((n as usize, out.len() - base, g, true));
        out.extend_from_slice(format!("{n} {g} obj").as_bytes());
        write_dict(&mut out, nl, &info);
        out.extend_from_slice(nl);
        out.extend_from_slice(b"endobj");
        set(&mut main, b"Info", format!("{n} {g} R").into_bytes());
    }
    entries.sort_by_key(|e| e.0);
    // Free-list links: each free entry points at the next free object.
    let free: Vec<usize> = entries.iter().filter(|e| !e.3).map(|e| e.0).collect();
    for e in entries.iter_mut().filter(|e| !e.3) {
        let pos = free.iter().position(|&f| f == e.0).unwrap_or(0);
        e.1 = free.get(pos + 1).copied().unwrap_or(0);
    }

    set(
        &mut main,
        b"Prev",
        trailer.startxref.to_string().into_bytes(),
    );
    bump_id(&mut main);

    if trailer.xref_stream {
        // The update's cross-reference is a stream too, uncompressed.
        let id = next;
        next += 1;
        out.extend_from_slice(nl);
        entries.push((id, out.len() - base, 0, true));
        entries.sort_by_key(|e| e.0);
        let startxref = out.len() - base;
        let wide = entries.iter().any(|e| e.1 > 0xffff_ffff);
        let mut stream = Vec::new();
        let mut index: Vec<String> = Vec::new();
        let mut k = 0;
        while k < entries.len() {
            let first = entries[k].0;
            let mut n = 0;
            while k < entries.len() && entries[k].0 == first + n {
                let (_, off, gen, used) = entries[k];
                stream.push(u8::from(used));
                if wide {
                    stream.extend_from_slice(&(off as u64).to_be_bytes());
                } else {
                    stream.extend_from_slice(&(off as u32).to_be_bytes());
                }
                stream.extend_from_slice(&(gen as u16).to_be_bytes());
                k += 1;
                n += 1;
            }
            index.push(format!("{first} {n}"));
        }
        main.retain(|(k, _)| {
            !matches!(
                k.as_slice(),
                b"W" | b"Index" | b"Length" | b"Filter" | b"DecodeParms" | b"DL" | b"XRefStm"
            )
        });
        set(&mut main, b"Size", next.to_string().into_bytes());
        let w = if wide { "[ 1 8 2 ]" } else { "[ 1 4 2 ]" };
        set(&mut main, b"W", w.as_bytes().to_vec());
        set(
            &mut main,
            b"Index",
            format!("[ {} ]", index.join(" ")).into_bytes(),
        );
        set(&mut main, b"Length", stream.len().to_string().into_bytes());
        out.extend_from_slice(format!("{id} 0 obj").as_bytes());
        write_dict(&mut out, nl, &main);
        out.extend_from_slice(nl);
        out.extend_from_slice(b"stream\r\n");
        out.extend_from_slice(&stream);
        out.extend_from_slice(nl);
        out.extend_from_slice(b"endstream");
        out.extend_from_slice(nl);
        out.extend_from_slice(b"endobj");
        finish(&mut out, nl, old_eof, startxref);
    } else {
        main.retain(|(k, _)| k.as_slice() != b"XRefStm");
        set(&mut main, b"Size", next.to_string().into_bytes());
        out.extend_from_slice(nl);
        let startxref = out.len() - base;
        out.extend_from_slice(b"xref");
        out.extend_from_slice(nl);
        // Entries are exactly 20 bytes: pad a one-byte newline.
        let endl: Vec<u8> = if nl.len() == 1 {
            [b" ", nl].concat()
        } else {
            nl.to_vec()
        };
        let mut k = 0;
        while k < entries.len() {
            let first = entries[k].0;
            let mut j = k;
            while j + 1 < entries.len() && entries[j + 1].0 == entries[j].0 + 1 {
                j += 1;
            }
            out.extend_from_slice(format!("{first} {}", j - k + 1).as_bytes());
            out.extend_from_slice(nl);
            for e in &entries[k..=j] {
                let t = if e.3 { 'n' } else { 'f' };
                out.extend_from_slice(format!("{:010} {:05} {t}", e.1, e.2).as_bytes());
                out.extend_from_slice(&endl);
            }
            k = j + 1;
        }
        out.extend_from_slice(b"trailer");
        write_dict(&mut out, nl, &main);
        finish(&mut out, nl, old_eof, startxref);
    }
    Ok(out)
}

fn finish(out: &mut Vec<u8>, nl: &[u8], old_eof: usize, startxref: usize) {
    out.extend_from_slice(nl);
    out.extend_from_slice(END_COMMENT);
    out.extend_from_slice(old_eof.to_string().as_bytes());
    out.extend_from_slice(nl);
    out.extend_from_slice(b"startxref");
    out.extend_from_slice(nl);
    out.extend_from_slice(startxref.to_string().as_bytes());
    out.extend_from_slice(nl);
    out.extend_from_slice(b"%%EOF");
    out.extend_from_slice(nl);
}

/// A new revision gets a new second file identifier: ExifTool increments
/// its first byte.
fn bump_id(main: &mut [(Vec<u8>, Vec<u8>)]) {
    let Some((_, v)) = main.iter_mut().find(|(k, _)| k.as_slice() == b"ID") else {
        return;
    };
    let d = v.clone();
    if d.first() != Some(&b'[') {
        return;
    }
    let first = skip_ws(&d, 1);
    let Some(e1) = object_end(&d, first) else {
        return;
    };
    let second = skip_ws(&d, e1);
    if d.get(second) == Some(&b'<') && d.get(second + 1) != Some(&b'<') {
        let h = d
            .get(second + 1..second + 3)
            .and_then(|h| std::str::from_utf8(h).ok());
        if let Some(byte) = h.and_then(|h| u8::from_str_radix(h, 16).ok()) {
            let new = format!("{:02x}", byte.wrapping_add(1));
            v[second + 1..second + 3].copy_from_slice(new.as_bytes());
        }
    } else if d.get(second) == Some(&b'(') {
        if let Some(&c) = d.get(second + 1) {
            if !matches!(c, b'\\' | b')' | b'(') {
                let mut n = c.wrapping_add(1);
                if matches!(n, b'(' | b')' | b'\\' | 0x00..=0x08 | 0x0a..=0x1f | 0x7f | 0xff) {
                    n = b'a';
                }
                v[second + 1] = n;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal classic PDF with an Info dictionary (object 3).
    fn sample() -> Vec<u8> {
        let mut f = b"%PDF-1.4\n".to_vec();
        let o1 = f.len();
        f.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        let o2 = f.len();
        f.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n");
        let o3 = f.len();
        f.extend_from_slice(b"3 0 obj\n<< /Producer (Old \\(p\\)) /Title (T) >>\nendobj\n");
        let x = f.len();
        f.extend_from_slice(
            format!(
                "xref\n0 4\n0000000000 65535 f \n{o1:010} 00000 n \n{o2:010} 00000 n \n{o3:010} 00000 n \ntrailer\n<< /Size 4 /Root 1 0 R /Info 3 0 R >>\nstartxref\n{x}\n%%EOF\n"
            )
            .as_bytes(),
        );
        f
    }

    fn info_of(pdf: &[u8]) -> Vec<(String, String)> {
        let t = last_trailer(pdf, 0).unwrap();
        let Some((n, g)) = get(&t.dict, b"Info").and_then(parse_ref) else {
            return Vec::new();
        };
        let mut i = find_object(pdf, n, g).unwrap();
        for _ in 0..3 {
            i = skip_ws(pdf, object_end(pdf, i).unwrap());
        }
        parse_dict(pdf, i)
            .unwrap()
            .0
            .into_iter()
            .map(|(k, v)| (String::from_utf8(k).unwrap(), String::from_utf8(v).unwrap()))
            .collect()
    }

    #[test]
    fn appends_an_incremental_update() {
        let src = sample();
        let ch = PdfChanges {
            set: vec![("Author".into(), "Me (x)".into())],
            delete: vec!["Producer".into()],
            ..Default::default()
        };
        let out = write_pdf(&src, &ch).unwrap();
        // The original file is untouched.
        assert!(out.starts_with(&src));
        assert_eq!(
            info_of(&out),
            [
                ("Title".to_string(), "(T)".to_string()),
                ("Author".to_string(), "(Me \\(x\\))".to_string())
            ]
        );
        // The new xref points at the new revision of object 3.
        let t = last_trailer(&out, 0).unwrap();
        let at = t.startxref;
        let table = std::str::from_utf8(&out[at..]).unwrap();
        let line = table
            .lines()
            .find(|l| l.trim_end().ends_with("00000 n"))
            .unwrap();
        let off: usize = line[..10].parse().unwrap();
        assert!(out[off..].starts_with(b"3 0 obj"));
        let prev = last_trailer(&src, 0).unwrap().startxref.to_string();
        assert_eq!(get(&t.dict, b"Prev"), Some(prev.as_bytes()));
    }

    #[test]
    fn creates_info_when_missing_and_drops_it_when_emptied() {
        let src = sample();
        let del = PdfChanges {
            delete_all: true,
            ..Default::default()
        };
        let out = write_pdf(&src, &del).unwrap();
        let t = last_trailer(&out, 0).unwrap();
        assert!(get(&t.dict, b"Info").is_none());
        let add = PdfChanges {
            set: vec![("Title".into(), "Ünïcode".into())],
            ..Default::default()
        };
        let out2 = write_pdf(&out, &add).unwrap();
        let t2 = last_trailer(&out2, 0).unwrap();
        // A new object, numbered from the trailer's Size.
        assert_eq!(get(&t2.dict, b"Info"), Some(&b"4 0 R"[..]));
        assert_eq!(get(&t2.dict, b"Size"), Some(&b"5"[..]));
        let title = &info_of(&out2)[0].1;
        assert!(title.starts_with("<feff00dc"), "{title}");
    }
}
