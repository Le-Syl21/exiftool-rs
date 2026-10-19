//! PNG metadata writer.
//!
//! Edits the textual chunks (tEXt, zTXt, iTXt), the writable PNG chunks
//! (tIME, gAMA, sRGB), the XMP packet (the `XML:com.adobe.xmp` text chunk)
//! and the EXIF block (eXIf) of a PNG, following ExifTool's `ProcessPNG`,
//! `FoundPNG` (PNG.pm) and `AddChunks` (WritePNG.pl):
//!
//! - an existing text chunk whose keyword names a written tag is rewritten
//!   where it is, keeping its keyword as spelled (`comment` for Comment);
//!   one whose tag is deleted is dropped; the same goes for tIME
//!   (`ModifyDate`), gAMA (`Gamma`) and sRGB (`SRGBRendering`);
//! - `PNG:all` drops every textual tag and those three chunks, keeping the
//!   ones that hold a directory (XMP, raw profiles), the ICC profile, pHYs and
//!   the image data; a tag also written comes back as a new chunk;
//! - text and eXIf chunks found after the image data are moved before it
//!   (up to a chunk nothing may be moved across), since some readers ignore
//!   them there;
//! - new chunks go right before the first image data chunk: gAMA and sRGB
//!   first (before PLTE when there is one), then the text chunks and tIME
//!   (in tag-ID order), then XMP, then EXIF.
//!
//! The image data chunks are copied unchanged.

use crate::error::{Error, Result};
use crate::writer::exif_edit::{self, ExifChanges, ExifOutcome};
use crate::writer::xmp_edit::{self, XmpChanges, XmpOutcome};

const SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const XMP_KEYWORD: &[u8] = b"XML:com.adobe.xmp";

/// The PNG part of a write request: the PNG tags themselves (text chunks,
/// `ModifyDate`, `Gamma`, `SRGBRendering`) and the PNG-only groups.
#[derive(Debug, Clone, Default)]
pub struct PngTextChanges {
    /// `PNG:all`: drop every textual tag and the tIME, gAMA and sRGB chunks
    /// (not the XMP, raw profile, iCCP or pHYs chunks).
    pub delete_all: bool,
    /// Tags to delete, by name.
    pub delete: Vec<String>,
    /// Tags to write: (name, value).
    pub set: Vec<(String, String)>,
    /// `ICC_Profile:all`: drop the iCCP chunk and the ICC raw profiles.
    pub delete_icc_profile: bool,
    /// `PNG-pHYs:all`: drop the pHYs chunk.
    pub delete_physical_pixel: bool,
    /// `all`: also drop what only a full deletion reaches here: every raw
    /// profile, the SEAL chunk and the data after IEND.
    pub delete_everything: bool,
}

impl PngTextChanges {
    pub fn is_empty(&self) -> bool {
        !self.delete_all
            && self.delete.is_empty()
            && self.set.is_empty()
            && !self.delete_icc_profile
            && !self.delete_physical_pixel
            && !self.delete_everything
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

/// The language tag of an iTXt chunk (empty for tEXt and zTXt).
fn language<'a>(typ: &[u8; 4], data: &'a [u8]) -> &'a [u8] {
    if typ != b"iTXt" {
        return &[];
    }
    let rest = data.get(keyword(data).len() + 3..).unwrap_or_default();
    keyword(rest)
}

/// The PNG chunks ExifTool writes as tags (PNG.pm `Main` entries with
/// `Writable`): tag name and chunk type.
const CHUNK_TAGS: &[(&str, &[u8; 4])] = &[
    ("ModifyDate", b"tIME"),
    ("Gamma", b"gAMA"),
    ("SRGBRendering", b"sRGB"),
];

fn chunk_tag(name: &str) -> Option<[u8; 4]> {
    CHUNK_TAGS
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, t)| **t)
}

/// The data of a tIME, gAMA or sRGB chunk for a written value, through the
/// tag's inverse conversions; `None` where ExifTool warns and writes nothing.
fn encode_chunk_tag(typ: &[u8; 4], value: &str) -> Option<Vec<u8>> {
    match typ {
        b"tIME" => {
            // PrintConvInv `InverseDateTime`, then ValueConvInv
            // `/^(\d+):(\d+):(\d+)\s+(\d+):(\d+):(\d+)/` and `pack('nC5')`.
            let v = crate::writer::xmp_tags::inverse_date_time(value)?;
            let (date, time) = v.split_once(' ')?;
            let mut n: Vec<u32> = Vec::with_capacity(6);
            for part in date.split(':').chain(time.split(':')).take(6) {
                let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
                n.push(digits.parse().ok()?);
            }
            if n.len() != 6 {
                return None;
            }
            let mut d = (n[0] as u16).to_be_bytes().to_vec();
            d.extend(n[1..].iter().map(|&x| x as u8));
            Some(d)
        }
        b"gAMA" => {
            // ValueConvInv `pack("N", int(1e5/$val+0.5))`.
            let g: f64 = value.trim().parse().ok()?;
            let n = (1e5 / g + 0.5).trunc();
            if !n.is_finite() || !(0.0..=f64::from(u32::MAX)).contains(&n) {
                return None;
            }
            Some((n as u32).to_be_bytes().to_vec())
        }
        b"sRGB" => {
            const NAMES: [&str; 4] = [
                "Perceptual",
                "Relative Colorimetric",
                "Saturation",
                "Absolute Colorimetric",
            ];
            let i = NAMES
                .iter()
                .position(|n| n.eq_ignore_ascii_case(value.trim()))?;
            Some(vec![i as u8])
        }
        _ => None,
    }
}

/// Does a text chunk (keyword `kw`, language `lang`) hold tag `name`? Tag
/// names are matched without regard to case. A known keyword holds the tag
/// `TextualData` names (`Creation Time` is `CreationTime`), with a language
/// code that tag in that language (`Title-fr`); an unknown keyword can only
/// be one this writer was asked to create, matched as spelled. The names of
/// the tIME, gAMA and sRGB tags never reach a text chunk.
fn names_tag(kw: &[u8], lang: &[u8], name: &str) -> bool {
    if chunk_tag(name).is_some() {
        return false;
    }
    let kw = crate::encoding::decode_latin1(kw);
    match crate::formats::png::text_tag_name(&kw) {
        Some(tag) if lang.is_empty() => tag.eq_ignore_ascii_case(name),
        Some(tag) => {
            let lang = crate::encoding::decode_latin1(lang);
            name.eq_ignore_ascii_case(&format!("{tag}-{lang}"))
        }
        None => lang.is_empty() && kw.eq_ignore_ascii_case(name),
    }
}

/// The keyword and language of a new text chunk for tag `name`: a known
/// tag's `TextualData` keyword, with the language of a `Tag-lang` name;
/// any other name as given.
fn new_keyword(name: &str) -> (String, String) {
    let known = |n: &str| {
        crate::formats::png::TEXT_TAGS
            .iter()
            .find(|(_, t)| t.eq_ignore_ascii_case(n))
            .map(|(kw, _)| kw.to_string())
    };
    if let Some(kw) = known(name) {
        return (kw, String::new());
    }
    if let Some((tag, lang)) = name.split_once('-') {
        if let Some(kw) = known(tag) {
            return (kw, lang.to_string());
        }
    }
    (name.to_string(), String::new())
}

/// The directories a text keyword can hold (PNG.pm `TextualData`): the XMP
/// packet and the raw profiles. With a language code (iTXt), the keyword is
/// an ordinary, unknown text tag instead.
#[derive(Clone, Copy, PartialEq)]
enum Directory {
    Xmp,
    /// `Raw profile type exif` and `APP1`.
    ExifProfile,
    /// `Raw profile type icc` and `icm`.
    IccProfile,
    /// `Raw profile type xmp`.
    XmpProfile,
    /// `Raw profile type iptc` and `8bim`.
    PhotoshopProfile,
}

fn directory(kw: &[u8], lang: &[u8]) -> Option<Directory> {
    if !lang.is_empty() {
        return None;
    }
    if kw == XMP_KEYWORD {
        return Some(Directory::Xmp);
    }
    match kw.strip_prefix(b"Raw profile type ")? {
        b"exif" | b"APP1" => Some(Directory::ExifProfile),
        b"icc" | b"icm" => Some(Directory::IccProfile),
        b"xmp" => Some(Directory::XmpProfile),
        b"iptc" | b"8bim" => Some(Directory::PhotoshopProfile),
        _ => None,
    }
}

/// The data of a text chunk for `keyword` = `value` and its type, as
/// WritePNG.pl `BuildTextChunk` decides: iTXt (UTF-8) for a value in a
/// language or with non-ASCII characters, tEXt otherwise.
fn text_chunk(keyword: &[u8], lang: &[u8], value: &str) -> ([u8; 4], Vec<u8>) {
    let mut d = keyword.to_vec();
    d.push(0);
    if value.is_ascii() && lang.is_empty() {
        d.extend_from_slice(value.as_bytes());
        (*b"tEXt", d)
    } else {
        // Not compressed, the language, no translated keyword.
        d.extend_from_slice(&[0, 0]);
        d.extend_from_slice(lang);
        d.extend_from_slice(&[0, 0]);
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

/// A zlib stream, inflated.
fn inflate(data: &[u8]) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(data)
        .read_to_end(&mut out)
        .ok()?;
    Some(out)
}

/// The XMP packet of an `XML:com.adobe.xmp` text chunk of any of the three
/// types, inflated when it is compressed; `None` when it cannot be.
fn xmp_payload(typ: &[u8; 4], data: &[u8]) -> Option<Vec<u8>> {
    let rest = data.strip_prefix(XMP_KEYWORD)?.strip_prefix(&[0u8][..])?;
    match typ {
        b"tEXt" => Some(rest.to_vec()),
        b"zTXt" => inflate(rest.get(1..)?),
        _ => {
            let (&flag, rest) = rest.split_first()?;
            let rest = rest.get(1..)?; // compression method
                                       // Language tag and translated keyword, each NUL-terminated.
            let a = rest.iter().position(|&b| b == 0)?;
            let rest = &rest[a + 1..];
            let b = rest.iter().position(|&b| b == 0)?;
            let text = &rest[b + 1..];
            if flag != 0 {
                inflate(text)
            } else {
                Some(text.to_vec())
            }
        }
    }
}

/// A tIME, gAMA or sRGB value to write, resolved from the request.
struct ChunkSet {
    typ: [u8; 4],
    data: Vec<u8>,
    written: bool,
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

    // The tIME/gAMA/sRGB part of the request; the rest is text. A value that
    // does not convert is not written (ExifTool warns and skips it).
    let mut chunk_sets: Vec<ChunkSet> = Vec::new();
    for (name, value) in &text.set {
        if let Some(typ) = chunk_tag(name) {
            chunk_sets.retain(|c| c.typ != typ);
            if let Some(data) = encode_chunk_tag(&typ, value) {
                chunk_sets.push(ChunkSet {
                    typ,
                    data,
                    written: false,
                });
            }
        }
    }
    let chunk_deleted = |typ: &[u8; 4]| {
        text.delete_all
            || text
                .delete
                .iter()
                .any(|n| chunk_tag(n).as_ref() == Some(typ))
    };
    let text_sets: Vec<&(String, String)> = text
        .set
        .iter()
        .filter(|(n, _)| chunk_tag(n).is_none())
        .collect();

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
    let mut written: Vec<bool> = vec![false; text_sets.len()];
    let mut saw_xmp = false;
    let mut saw_exif = false;
    let mut added_early = false;
    let mut added = false;

    for &i in &order {
        let c = &list[i];
        let data_or_end = is_data(&c.typ) || &c.typ == b"IEND";
        if !added_early && (data_or_end || &c.typ == b"PLTE") {
            // WritePNG.pl `Add_iCCP`: new sRGB and gAMA chunks go before PLTE.
            added_early = true;
            add_chunk_tags(&mut out, &mut chunk_sets, |t| t != b"tIME");
        }
        if !added && data_or_end {
            added = true;
            add_new(
                &mut out,
                &text_sets,
                &written,
                &mut chunk_sets,
                exif,
                xmp,
                saw_xmp,
                saw_exif,
            )?;
        }
        if CHUNK_TAGS.iter().any(|(_, t)| **t == c.typ) {
            if text.delete_all {
                // Deleted with the group; a new value is added back with the
                // new chunks.
                continue;
            }
            if let Some(s) = chunk_sets.iter_mut().find(|s| s.typ == c.typ) {
                s.written = true;
                write_chunk(&mut out, &c.typ, &s.data);
                continue;
            }
            if !chunk_deleted(&c.typ) {
                out.extend_from_slice(c.raw);
            }
            continue;
        }
        match &c.typ {
            b"iCCP" if text.delete_icc_profile => continue,
            b"pHYs" if text.delete_physical_pixel => continue,
            b"seAl" if text.delete_everything => continue,
            _ => {}
        }
        if is_text(&c.typ) {
            let kw = keyword(c.data);
            let lang = language(&c.typ, c.data);
            match directory(kw, lang) {
                Some(Directory::Xmp) => {
                    saw_xmp = true;
                    if xmp.is_empty() {
                        out.extend_from_slice(c.raw);
                        continue;
                    }
                    let Some(packet) = xmp_payload(&c.typ, c.data) else {
                        // Not inflatable: deleted with the group, otherwise
                        // kept as it is (and not added again).
                        if !xmp.delete_all {
                            out.extend_from_slice(c.raw);
                        }
                        continue;
                    };
                    match xmp_edit::edit_xmp(Some(&packet), xmp)? {
                        XmpOutcome::Unchanged => out.extend_from_slice(c.raw),
                        XmpOutcome::Removed => {}
                        // Always written back as an uncompressed iTXt chunk
                        // (`BuildTextChunk`, whatever the chunk was).
                        XmpOutcome::Rewritten(x) => write_chunk(&mut out, b"iTXt", &xmp_chunk(&x)),
                    }
                    continue;
                }
                Some(dir) => {
                    let deleted = text.delete_everything
                        || match dir {
                            Directory::ExifProfile => exif.delete_all,
                            Directory::IccProfile => text.delete_icc_profile,
                            Directory::XmpProfile => xmp.delete_all,
                            _ => false,
                        };
                    if !deleted {
                        out.extend_from_slice(c.raw);
                    }
                    continue;
                }
                None => {}
            }
            if text.delete_all {
                // The tag goes with the group; a new value for it is added
                // back with the new chunks (ExifTool keeps it in `ADD_PNG`).
                continue;
            }
            if text.delete.iter().any(|n| names_tag(kw, lang, n)) {
                // A tag both deleted and written gets its new value in place.
                if let Some(j) = text_sets.iter().position(|(n, _)| names_tag(kw, lang, n)) {
                    if !written[j] {
                        written[j] = true;
                        let (t, d) = text_chunk(kw, lang, &text_sets[j].1);
                        write_chunk(&mut out, &t, &d);
                    }
                }
                continue;
            }
            if let Some(j) = text_sets.iter().position(|(n, _)| names_tag(kw, lang, n)) {
                written[j] = true;
                let (t, d) = text_chunk(kw, lang, &text_sets[j].1);
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
    if !text.delete_everything {
        out.extend_from_slice(trailer);
    }
    Ok(out)
}

/// Write the pending tIME/gAMA/sRGB values that `pick` selects, in chunk
/// type order (`sort keys`).
fn add_chunk_tags(out: &mut Vec<u8>, sets: &mut [ChunkSet], pick: impl Fn(&[u8; 4]) -> bool) {
    let mut pending: Vec<&mut ChunkSet> = sets
        .iter_mut()
        .filter(|s| !s.written && pick(&s.typ))
        .collect();
    pending.sort_by_key(|s| s.typ);
    for s in pending {
        s.written = true;
        write_chunk(out, &s.typ, &s.data);
    }
}

/// The new chunks, written before the first image data chunk.
#[allow(clippy::too_many_arguments)]
fn add_new(
    out: &mut Vec<u8>,
    text_sets: &[&(String, String)],
    written: &[bool],
    chunk_sets: &mut [ChunkSet],
    exif: &ExifChanges,
    xmp: &XmpChanges,
    saw_xmp: bool,
    saw_exif: bool,
) -> Result<()> {
    // The text chunks and tIME still to add, in tag-ID order (`sort keys`):
    // a text tag's ID is its keyword (`Title-fr` in a language), tIME's is
    // the chunk type. A name given twice is written once, with its last value.
    let mut pending: Vec<(Vec<u8>, [u8; 4], Vec<u8>)> = Vec::new();
    let mut names: Vec<&str> = Vec::new();
    for (j, s) in text_sets.iter().enumerate() {
        if written[j] {
            continue;
        }
        if let Some(k) = names.iter().position(|n| n.eq_ignore_ascii_case(&s.0)) {
            names.remove(k);
            pending.remove(k);
        }
        let (kw, lang) = new_keyword(&s.0);
        let (t, d) = text_chunk(kw.as_bytes(), lang.as_bytes(), &s.1);
        let id = if lang.is_empty() {
            kw
        } else {
            format!("{kw}-{lang}")
        };
        names.push(&s.0);
        pending.push((id.into_bytes(), t, d));
    }
    for s in chunk_sets.iter_mut().filter(|s| !s.written) {
        s.written = true;
        pending.push((s.typ.to_vec(), s.typ, s.data.clone()));
    }
    pending.sort_by(|a, b| a.0.cmp(&b.0));
    for (_, t, d) in pending {
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
