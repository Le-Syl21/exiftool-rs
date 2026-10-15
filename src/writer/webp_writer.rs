//! WebP metadata writer: the EXIF and XMP chunks.
//!
//! Follows ExifTool's `WriteRIFF` (WriteRIFF.pl) for WebP:
//!
//! - an existing `EXIF` or `XMP ` chunk is edited in place ([`exif_edit`],
//!   [`xmp_edit`]), keeping a non-standard `Exif\0\0` header if the chunk
//!   has one, and dropped when nothing is left in it;
//! - a new EXIF chunk goes before an existing XMP chunk (EXIF must come
//!   first), otherwise new chunks are appended, EXIF then XMP;
//! - the VP8X flags for EXIF and XMP follow what the file ends up with, a
//!   VP8X chunk is created when an extended feature needs one, and dropped
//!   when nothing needs it any more;
//! - the image chunks are copied unchanged.

use crate::error::{Error, Result};
use crate::writer::exif_edit::{self, ExifChanges, ExifOutcome};
use crate::writer::xmp_edit::{self, XmpChanges, XmpOutcome};

struct Chunk<'a> {
    id: [u8; 4],
    data: &'a [u8],
    /// Header, data and pad byte, as found.
    raw: &'a [u8],
}

fn chunks(source: &[u8]) -> Result<Vec<Chunk<'_>>> {
    let mut out = Vec::new();
    let mut pos = 12;
    while pos + 8 <= source.len() {
        let mut id = [0u8; 4];
        id.copy_from_slice(&source[pos..pos + 4]);
        let len = u32::from_le_bytes([
            source[pos + 4],
            source[pos + 5],
            source[pos + 6],
            source[pos + 7],
        ]) as usize;
        let end = pos + 8 + len;
        if end > source.len() {
            return Err(Error::InvalidData(format!(
                "truncated '{}' chunk",
                String::from_utf8_lossy(&id)
            )));
        }
        let padded = (end + (len & 1)).min(source.len());
        out.push(Chunk {
            id,
            data: &source[pos + 8..end],
            raw: &source[pos..padded],
        });
        pos = padded;
    }
    Ok(out)
}

fn put_chunk(out: &mut Vec<u8>, id: &[u8; 4], parts: &[&[u8]]) {
    let len: usize = parts.iter().map(|p| p.len()).sum();
    out.extend_from_slice(id);
    out.extend_from_slice(&(len as u32).to_le_bytes());
    for p in parts {
        out.extend_from_slice(p);
    }
    if len % 2 == 1 {
        out.push(0);
    }
}

/// The new content of an EXIF chunk: `None` to drop it, the chunk as is
/// when unchanged.
fn edit_exif_chunk(data: &[u8], exif: &ExifChanges) -> Result<Option<Vec<u8>>> {
    let (hdr, tiff) = match data.strip_prefix(b"Exif\0\0") {
        // ExifTool drops the improper header when the whole EXIF goes.
        Some(t) => (&data[..6], t),
        None => (&data[..0], data),
    };
    Ok(match exif_edit::edit_exif(Some(tiff), exif)? {
        ExifOutcome::Unchanged => Some(data.to_vec()),
        ExifOutcome::Removed => None,
        ExifOutcome::Rewritten(t) => Some([hdr, &t].concat()),
    })
}

fn edit_xmp_chunk(data: Option<&[u8]>, xmp: &XmpChanges) -> Result<Option<Vec<u8>>> {
    Ok(match xmp_edit::edit_xmp(data, xmp)? {
        XmpOutcome::Unchanged => data.map(<[u8]>::to_vec),
        XmpOutcome::Removed => None,
        XmpOutcome::Rewritten(x) => Some(x),
    })
}

/// The canvas size from a VP8 or VP8L chunk (for a VP8X created here).
fn image_size(c: &Chunk) -> Option<(u32, u32)> {
    let d = c.data;
    match &c.id {
        b"VP8 " if d.len() >= 10 && d[3..6] == [0x9d, 0x01, 0x2a] => Some((
            u32::from(u16::from_le_bytes([d[6], d[7]]) & 0x3fff),
            u32::from(u16::from_le_bytes([d[8], d[9]]) & 0x3fff),
        )),
        b"VP8L" if d.len() >= 5 && d[0] == 0x2f => {
            let w = u32::from_le_bytes([d[1], d[2], d[3], d[4]]);
            Some(((w & 0x3fff) + 1, ((w >> 14) & 0x3fff) + 1))
        }
        _ => None,
    }
}

/// Rewrite a WebP file with the EXIF and XMP changes applied.
pub fn write_webp(source: &[u8], exif: &ExifChanges, xmp: &XmpChanges) -> Result<Vec<u8>> {
    if source.len() < 12 || !source.starts_with(b"RIFF") || &source[8..12] != b"WEBP" {
        return Err(Error::InvalidData("not a WebP file".into()));
    }
    if exif.is_empty() && xmp.is_empty() {
        return Ok(source.to_vec());
    }
    let list = chunks(source)?;
    let has = |id: &[u8; 4]| list.iter().any(|c| &c.id == id);

    // New chunk contents (EXIF, XMP): what each one becomes.
    let old_exif = list.iter().find(|c| &c.id == b"EXIF");
    let old_xmp = list.iter().find(|c| &c.id == b"XMP ");
    let new_exif = match old_exif {
        Some(c) => edit_exif_chunk(c.data, exif)?,
        None => match exif_edit::edit_exif(None, exif)? {
            ExifOutcome::Rewritten(t) => Some(t),
            _ => None,
        },
    };
    let new_xmp = edit_xmp_chunk(old_xmp.map(|c| c.data), xmp)?;
    if new_exif.as_deref() == old_exif.map(|c| c.data)
        && new_xmp.as_deref() == old_xmp.map(|c| c.data)
    {
        return Ok(source.to_vec());
    }

    // Body: the chunks with the metadata edited, a new EXIF before the XMP.
    let mut body = Vec::with_capacity(source.len() + 1024);
    let mut exif_done = false;
    let mut xmp_done = false;
    for c in &list {
        match &c.id {
            b"VP8X" => {} // written with its final flags below
            b"EXIF" if !exif_done => {
                exif_done = true;
                if let Some(e) = &new_exif {
                    put_chunk(&mut body, b"EXIF", &[e]);
                }
            }
            b"XMP " if !xmp_done => {
                xmp_done = true;
                if !exif_done {
                    exif_done = true;
                    if let Some(e) = &new_exif {
                        put_chunk(&mut body, b"EXIF", &[e]);
                    }
                }
                if let Some(x) = &new_xmp {
                    put_chunk(&mut body, b"XMP ", &[x]);
                }
            }
            _ => body.extend_from_slice(c.raw),
        }
    }
    if !exif_done {
        if let Some(e) = &new_exif {
            put_chunk(&mut body, b"EXIF", &[e]);
        }
    }
    if !xmp_done {
        if let Some(x) = &new_xmp {
            put_chunk(&mut body, b"XMP ", &[x]);
        }
    }

    // VP8X: flags reflect the final EXIF/XMP; created or dropped as needed.
    let (with_exif, with_xmp) = (new_exif.is_some(), new_xmp.is_some());
    let needs_vp8x = has(b"ANIM") || has(b"ALPH") || has(b"ICCP") || with_exif || with_xmp;
    let mut vp8x: Option<Vec<u8>> = None;
    if let Some(c) = list.iter().find(|c| &c.id == b"VP8X") {
        if needs_vp8x {
            let mut d = c.data.to_vec();
            if d.len() < 10 {
                return Err(Error::InvalidData("truncated VP8X chunk".into()));
            }
            d[0] &= !0x2c;
            if with_xmp {
                d[0] |= 0x04;
            }
            if with_exif {
                d[0] |= 0x08;
            }
            if has(b"ICCP") {
                d[0] |= 0x20;
            }
            vp8x = Some(d);
        }
    } else if needs_vp8x {
        let (w, h) = list
            .iter()
            .find_map(image_size)
            .ok_or_else(|| Error::InvalidData("cannot get the WebP image size for VP8X".into()))?;
        let mut flags = 0u32;
        if has(b"ANIM") {
            flags |= 0x02;
        }
        if with_xmp {
            flags |= 0x04;
        }
        if with_exif {
            flags |= 0x08;
        }
        if has(b"ALPH") {
            flags |= 0x10;
        }
        if has(b"ICCP") {
            flags |= 0x20;
        }
        let mut d = flags.to_le_bytes().to_vec();
        let (w1, h1) = (w.saturating_sub(1), h.saturating_sub(1));
        d.extend_from_slice(&(w1 | ((h1 & 0xff) << 24)).to_le_bytes());
        d.extend_from_slice(&((h1 >> 8) as u16).to_le_bytes());
        vp8x = Some(d);
    }

    let mut out = Vec::with_capacity(body.len() + 32);
    out.extend_from_slice(b"RIFF\0\0\0\0WEBP");
    if let Some(d) = &vp8x {
        put_chunk(&mut out, b"VP8X", &[d]);
    }
    // ICCP must follow VP8X; it is among the body chunks already, in its
    // original place, which is right after VP8X in a valid file.
    out.extend_from_slice(&body);
    let size = (out.len() - 8) as u32;
    out[4..8].copy_from_slice(&size.to_le_bytes());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A simple (lossy, VP8) WebP: 3x2 canvas, no VP8X.
    fn simple() -> Vec<u8> {
        let mut body = Vec::new();
        let vp8 = [0x50, 0x01, 0x00, 0x9d, 0x01, 0x2a, 3, 0, 2, 0, 0xAA, 0xBB];
        put_chunk(&mut body, b"VP8 ", &[&vp8]);
        let mut f = b"RIFF".to_vec();
        f.extend_from_slice(&((body.len() + 4) as u32).to_le_bytes());
        f.extend_from_slice(b"WEBP");
        f.extend_from_slice(&body);
        f
    }

    fn set_artist() -> ExifChanges {
        let spec = exif_edit::tag_spec("Artist").unwrap();
        ExifChanges {
            set: vec![exif_edit::SetValue {
                ifd: spec.ifd,
                tag: spec.tag,
                format: spec.format,
                big_endian: b"me\0".to_vec(),
                little_endian: b"me\0".to_vec(),
            }],
            ..Default::default()
        }
    }

    #[test]
    fn adding_exif_creates_vp8x_and_keeps_the_image() {
        let out = write_webp(&simple(), &set_artist(), &XmpChanges::default()).unwrap();
        let list = chunks(&out).unwrap();
        let ids: Vec<&[u8; 4]> = list.iter().map(|c| &c.id).collect();
        assert_eq!(ids, [b"VP8X", b"VP8 ", b"EXIF"]);
        // EXIF flag, canvas 3x2 stored minus one.
        assert_eq!(list[0].data, &[0x08, 0, 0, 0, 2, 0, 0, 1, 0, 0]);
        assert!(list[1].data.ends_with(&[0xAA, 0xBB]));
        assert_eq!(
            u32::from_le_bytes([out[4], out[5], out[6], out[7]]) as usize,
            out.len() - 8
        );
    }

    #[test]
    fn deleting_the_last_metadata_drops_vp8x() {
        let with = write_webp(&simple(), &set_artist(), &XmpChanges::default()).unwrap();
        let del = ExifChanges {
            delete_all: true,
            ..Default::default()
        };
        let out = write_webp(&with, &del, &XmpChanges::default()).unwrap();
        assert_eq!(out, simple());
    }
}
