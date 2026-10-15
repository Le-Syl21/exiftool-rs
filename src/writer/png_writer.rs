//! PNG metadata writer.
//!
//! Edits the textual chunks (tEXt, zTXt, iTXt), the XMP packet (the
//! `XML:com.adobe.xmp` iTXt chunk) and the EXIF block (eXIf) of a PNG,
//! following ExifTool's `ProcessPNG` (PNG.pm) and `AddChunks` (WritePNG.pl):
//!
//! - an existing text chunk whose keyword names a written tag is rewritten
//!   where it is, keeping its keyword as spelled (`comment` for Comment);
//!   one whose tag is deleted is dropped;
//! - text and eXIf chunks found after the image data are moved before it
//!   (up to a chunk nothing may be moved across), since some readers ignore
//!   them there;
//! - new chunks go right before the first image data chunk, text first (in
//!   tag-name order), then XMP, then EXIF.
//!
//! The image data chunks are copied unchanged.

use crate::error::{Error, Result};
use crate::writer::exif_edit::{self, ExifChanges, ExifOutcome};
use crate::writer::xmp_edit::{self, XmpChanges, XmpOutcome};

const SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const XMP_KEYWORD: &[u8] = b"XML:com.adobe.xmp";

/// The PNG textual part of a write request.
#[derive(Debug, Clone, Default)]
pub struct PngTextChanges {
    /// `PNG:all`: drop every textual tag (not the XMP or profile chunks).
    pub delete_all: bool,
    /// Tags to delete, by name.
    pub delete: Vec<String>,
    /// Tags to write: (name, value).
    pub set: Vec<(String, String)>,
}

impl PngTextChanges {
    pub fn is_empty(&self) -> bool {
        !self.delete_all && self.delete.is_empty() && self.set.is_empty()
    }
}

struct Chunk<'a> {
    typ: [u8; 4],
    data: &'a [u8],
    /// The chunk as found (length, type, data, CRC).
    raw: &'a [u8],
}

fn chunks(source: &[u8]) -> Result<(Vec<Chunk<'_>>, &[u8])> {
    let mut out = Vec::new();
    let mut pos = 8;
    loop {
        if pos + 12 > source.len() {
            return Err(Error::InvalidData("truncated PNG image".into()));
        }
        let len = u32::from_be_bytes([
            source[pos],
            source[pos + 1],
            source[pos + 2],
            source[pos + 3],
        ]) as usize;
        if len > 0x7fff_ffff || pos + 12 + len > source.len() {
            return Err(Error::InvalidData("corrupted PNG image".into()));
        }
        let mut typ = [0u8; 4];
        typ.copy_from_slice(&source[pos + 4..pos + 8]);
        let end = pos + 12 + len;
        out.push(Chunk {
            typ,
            data: &source[pos + 8..pos + 8 + len],
            raw: &source[pos..end],
        });
        pos = end;
        if &typ == b"IEND" {
            // Anything after IEND is a trailer, kept as is.
            return Ok((out, &source[pos..]));
        }
    }
}

fn is_text(t: &[u8; 4]) -> bool {
    matches!(t, b"tEXt" | b"zTXt" | b"iTXt")
}

fn is_data(t: &[u8; 4]) -> bool {
    matches!(t, b"IDAT" | b"JDAT" | b"JDAA")
}

/// Chunks nothing may be moved across (PNG.pm `%noLeapFrog`).
fn no_leap_frog(t: &[u8; 4]) -> bool {
    matches!(
        t,
        b"SAVE"
            | b"SEEK"
            | b"IHDR"
            | b"JHDR"
            | b"IEND"
            | b"MEND"
            | b"DHDR"
            | b"BASI"
            | b"CLON"
            | b"PAST"
            | b"SHOW"
            | b"MAGN"
    )
}

fn keyword(data: &[u8]) -> &[u8] {
    &data[..data.iter().position(|&b| b == 0).unwrap_or(data.len())]
}

/// Does chunk keyword `kw` name tag `name`? ExifTool looks the keyword up
/// as is and with its first letter capitalized, and tag names are matched
/// without regard to case.
fn names_tag(kw: &[u8], name: &str) -> bool {
    let kw = crate::encoding::decode_latin1(kw);
    kw.eq_ignore_ascii_case(name)
}

/// A keyword that holds a directory rather than a text value (XMP, raw
/// profiles): never touched by textual edits.
fn is_directory_keyword(kw: &[u8]) -> bool {
    kw == XMP_KEYWORD || kw.starts_with(b"Raw profile type ")
}

/// The data of a text chunk for `keyword` = `value` and its type: tEXt for
/// ASCII, iTXt (UTF-8) otherwise, as WritePNG.pl `BuildTextChunk` decides.
fn text_chunk(keyword: &[u8], value: &str) -> ([u8; 4], Vec<u8>) {
    let mut d = keyword.to_vec();
    d.push(0);
    if value.is_ascii() {
        d.extend_from_slice(value.as_bytes());
        (*b"tEXt", d)
    } else {
        // Not compressed, no language, no translated keyword.
        d.extend_from_slice(&[0, 0, 0, 0]);
        d.extend_from_slice(value.as_bytes());
        (*b"iTXt", d)
    }
}

fn xmp_chunk(xmp: &[u8]) -> Vec<u8> {
    let mut d = XMP_KEYWORD.to_vec();
    d.extend_from_slice(&[0, 0, 0, 0, 0]);
    d.extend_from_slice(xmp);
    d
}

/// The XMP packet of an iTXt chunk, when it is not compressed.
fn xmp_payload(data: &[u8]) -> Option<&[u8]> {
    let rest = data.strip_prefix(XMP_KEYWORD)?.strip_prefix(&[0u8][..])?;
    let (&flag, rest) = rest.split_first()?;
    let rest = rest.get(1..)?; // compression method
    if flag != 0 {
        return None;
    }
    // Language tag and translated keyword, each NUL-terminated.
    let a = rest.iter().position(|&b| b == 0)?;
    let rest = &rest[a + 1..];
    let b = rest.iter().position(|&b| b == 0)?;
    Some(&rest[b + 1..])
}

/// Rewrite a PNG with the requested text, XMP and EXIF changes.
pub fn write_png(
    source: &[u8],
    text: &PngTextChanges,
    exif: &ExifChanges,
    xmp: &XmpChanges,
) -> Result<Vec<u8>> {
    if source.len() < 8 || !source.starts_with(SIGNATURE) {
        return Err(Error::InvalidData("not a PNG file".into()));
    }
    let (list, trailer) = chunks(source)?;

    // Order: text and eXIf chunks after the first data chunk move before it,
    // until a chunk that may not be crossed.
    let first_data = list.iter().position(|c| is_data(&c.typ));
    let mut moved: Vec<usize> = Vec::new();
    if let Some(fd) = first_data {
        for (i, c) in list.iter().enumerate().skip(fd + 1) {
            if no_leap_frog(&c.typ) {
                break;
            }
            if is_text(&c.typ) || &c.typ == b"eXIf" {
                moved.push(i);
            }
        }
    }
    let mut order: Vec<usize> = Vec::with_capacity(list.len());
    for i in 0..list.len() {
        if Some(i) == first_data {
            order.extend(&moved);
        }
        if !moved.contains(&i) {
            order.push(i);
        }
    }

    let mut out = Vec::with_capacity(source.len() + 256);
    out.extend_from_slice(SIGNATURE);
    let mut written: Vec<bool> = vec![false; text.set.len()];
    let mut saw_xmp = false;
    let mut saw_exif = false;
    let mut added = false;

    for &i in &order {
        let c = &list[i];
        if !added && (is_data(&c.typ) || &c.typ == b"IEND") {
            added = true;
            add_new(&mut out, text, &written, exif, xmp, saw_xmp, saw_exif)?;
        }
        if is_text(&c.typ) {
            let kw = keyword(c.data);
            if kw == XMP_KEYWORD && &c.typ == b"iTXt" && !saw_xmp {
                saw_xmp = true;
                if xmp.is_empty() {
                    out.extend_from_slice(c.raw);
                    continue;
                }
                let Some(packet) = xmp_payload(c.data) else {
                    return Err(Error::InvalidData(
                        "compressed PNG XMP is not supported for writing".into(),
                    ));
                };
                match xmp_edit::edit_xmp(Some(packet), xmp)? {
                    XmpOutcome::Unchanged => out.extend_from_slice(c.raw),
                    XmpOutcome::Removed => {}
                    XmpOutcome::Rewritten(x) => write_chunk(&mut out, b"iTXt", &xmp_chunk(&x)),
                }
                continue;
            }
            if is_directory_keyword(kw) {
                out.extend_from_slice(c.raw);
                continue;
            }
            if text.delete_all || text.delete.iter().any(|n| names_tag(kw, n)) {
                // A tag both deleted and written comes back with its new
                // value (ExifTool deletes first, then writes).
                if let Some(j) = text.set.iter().position(|(n, _)| names_tag(kw, n)) {
                    if !written[j] {
                        written[j] = true;
                        let (t, d) = text_chunk(kw, &text.set[j].1);
                        write_chunk(&mut out, &t, &d);
                    }
                }
                continue;
            }
            if let Some(j) = text.set.iter().position(|(n, _)| names_tag(kw, n)) {
                written[j] = true;
                let (t, d) = text_chunk(kw, &text.set[j].1);
                write_chunk(&mut out, &t, &d);
                continue;
            }
            out.extend_from_slice(c.raw);
            continue;
        }
        if &c.typ == b"eXIf" && !saw_exif {
            saw_exif = true;
            match exif_edit::edit_exif(Some(c.data), exif)? {
                ExifOutcome::Unchanged => out.extend_from_slice(c.raw),
                ExifOutcome::Removed => {}
                ExifOutcome::Rewritten(t) => write_chunk(&mut out, b"eXIf", &t),
            }
            continue;
        }
        out.extend_from_slice(c.raw);
    }
    out.extend_from_slice(trailer);
    Ok(out)
}

/// The new chunks, written before the first image data chunk.
fn add_new(
    out: &mut Vec<u8>,
    text: &PngTextChanges,
    written: &[bool],
    exif: &ExifChanges,
    xmp: &XmpChanges,
    saw_xmp: bool,
    saw_exif: bool,
) -> Result<()> {
    // The text chunks still to add, in tag-name order (`sort keys`). A name
    // given twice is written once, with its last value.
    let mut pending: Vec<&(String, String)> = Vec::new();
    for (j, s) in text.set.iter().enumerate() {
        if written[j] {
            continue;
        }
        pending.retain(|p| !p.0.eq_ignore_ascii_case(&s.0));
        pending.push(s);
    }
    pending.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, value) in pending {
        let (t, d) = text_chunk(name.as_bytes(), value);
        write_chunk(out, &t, &d);
    }
    if !saw_xmp && !xmp.is_empty() {
        if let XmpOutcome::Rewritten(x) = xmp_edit::edit_xmp(None, xmp)? {
            write_chunk(out, b"iTXt", &xmp_chunk(&x));
        }
    }
    if !saw_exif && !exif.is_empty() {
        if let ExifOutcome::Rewritten(t) = exif_edit::edit_exif(None, exif)? {
            write_chunk(out, b"eXIf", &t);
        }
    }
    Ok(())
}

/// Write a PNG chunk with correct length and CRC.
fn write_chunk(output: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
    let len = data.len() as u32;
    output.extend_from_slice(&len.to_be_bytes());
    output.extend_from_slice(chunk_type);
    output.extend_from_slice(data);

    // CRC-32 over chunk_type + data
    let crc = crc32(chunk_type, data);
    output.extend_from_slice(&crc.to_be_bytes());
}

/// Calculate PNG CRC-32 (ISO 3309).
fn crc32(chunk_type: &[u8], data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;

    for &byte in chunk_type.iter().chain(data.iter()) {
        let index = ((crc ^ byte as u32) & 0xFF) as usize;
        crc = CRC_TABLE[index] ^ (crc >> 8);
    }

    crc ^ 0xFFFFFFFF
}

/// Pre-computed CRC-32 lookup table.
static CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            if c & 1 != 0 {
                c = 0xEDB88320 ^ (c >> 1);
            } else {
                c >>= 1;
            }
            k += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
};

#[cfg(test)]
mod tests {
    use super::*;

    /// IHDR, IDAT, then a lower-case `comment` tEXt after the image data,
    /// like the corpus PNG.png.
    fn sample() -> Vec<u8> {
        let mut v = SIGNATURE.to_vec();
        write_chunk(&mut v, b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0, 0, 0]);
        write_chunk(&mut v, b"IDAT", b"pixels");
        write_chunk(&mut v, b"tEXt", b"comment\0test comment");
        write_chunk(&mut v, b"IEND", b"");
        v
    }

    fn types(png: &[u8]) -> Vec<(String, Vec<u8>)> {
        chunks(png)
            .unwrap()
            .0
            .iter()
            .map(|c| {
                (
                    String::from_utf8_lossy(&c.typ).into_owned(),
                    c.data.to_vec(),
                )
            })
            .collect()
    }

    fn none() -> (ExifChanges, XmpChanges) {
        (ExifChanges::default(), XmpChanges::default())
    }

    #[test]
    fn replaces_the_comment_in_place_and_moves_it_before_idat() {
        // ExifTool 13.59 `-PNG:Comment=Set`: IHDR, tEXt "comment\0Set", IDAT.
        let (e, x) = none();
        let text = PngTextChanges {
            set: vec![("Comment".into(), "Set".into())],
            ..Default::default()
        };
        let out = write_png(&sample(), &text, &e, &x).unwrap();
        let t = types(&out);
        assert_eq!(t[1], ("tEXt".into(), b"comment\0Set".to_vec()));
        assert_eq!(t[2], ("IDAT".into(), b"pixels".to_vec()));
        assert_eq!(t.len(), 4);
    }

    #[test]
    fn deletes_and_adds() {
        // `-PNG:Title=Added -PNG:Comment=`: the comment goes, Title is added
        // before IDAT.
        let (e, x) = none();
        let text = PngTextChanges {
            set: vec![("Title".into(), "Added".into())],
            delete: vec!["Comment".into()],
            ..Default::default()
        };
        let out = write_png(&sample(), &text, &e, &x).unwrap();
        let t = types(&out);
        assert_eq!(t[1], ("tEXt".into(), b"Title\0Added".to_vec()));
        assert_eq!(t[2].0, "IDAT");
        assert!(!t.iter().any(|(_, d)| d.starts_with(b"comment")));
    }

    #[test]
    fn non_ascii_text_goes_to_itxt() {
        let (e, x) = none();
        let text = PngTextChanges {
            set: vec![("Author".into(), "Martín".into())],
            ..Default::default()
        };
        let out = write_png(&sample(), &text, &e, &x).unwrap();
        let t = types(&out);
        let mut want = b"Author\0\0\0\0\0".to_vec();
        want.extend_from_slice("Martín".as_bytes());
        assert!(t.contains(&("iTXt".into(), want)));
    }
}
