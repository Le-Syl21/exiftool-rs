//! JPEG 2000 / JPEG XL metadata writer: the XMP packet.
//!
//! Follows ExifTool's `ProcessJpeg2000Box` (Jpeg2000.pm). The XMP lives in
//! a `uuid` box (the XMP UUID) in a JP2, and in an `xml ` box -- or a
//! Brotli-compressed `brob` box holding one -- in a JXL. (A JP2 `xml ` box
//! is plain XML, not XMP, and is left alone.) The packet is edited in place
//! with [`xmp_edit`]: a rewritten box stays where it was (a `brob` box is
//! compressed again), a deleted one is dropped, and a new one is added at
//! the end of the file, or before a last box that runs to the end of the
//! file. Every other box, the codestream included, is copied unchanged.

use crate::error::{Error, Result};
use crate::writer::xmp_edit::{self, XmpChanges, XmpOutcome};

const UUID_XMP: [u8; 16] = [
    0xBE, 0x7A, 0xCF, 0xCB, 0x97, 0xA9, 0x42, 0xE8, 0x9C, 0x71, 0x99, 0x94, 0x91, 0xE3, 0xAF, 0xAC,
];

struct Box<'a> {
    typ: [u8; 4],
    /// The box as found, header included.
    raw: &'a [u8],
    /// The box content.
    content: &'a [u8],
    /// The size field was 0: the box runs to the end of the file.
    to_end: bool,
}

fn boxes(source: &[u8]) -> Result<Vec<Box<'_>>> {
    let bad = || Error::InvalidData("corrupted JPEG 2000 box".into());
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < source.len() {
        let hdr = source.get(pos..pos + 8).ok_or_else(bad)?;
        let size = u32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) as u64;
        let mut typ = [0u8; 4];
        typ.copy_from_slice(&hdr[4..8]);
        let (hlen, len, to_end) = match size {
            0 => (8, (source.len() - pos) as u64, true),
            1 => {
                let b = source.get(pos + 8..pos + 16).ok_or_else(bad)?;
                let mut a = [0u8; 8];
                a.copy_from_slice(b);
                (16, u64::from_be_bytes(a), false)
            }
            n => (8, n, false),
        };
        let len = usize::try_from(len).map_err(|_| bad())?;
        if len < hlen || pos + len > source.len() {
            return Err(bad());
        }
        out.push(Box {
            typ,
            raw: &source[pos..pos + len],
            content: &source[pos + hlen..pos + len],
            to_end,
        });
        pos += len;
    }
    Ok(out)
}

fn put_box(out: &mut Vec<u8>, typ: &[u8; 4], parts: &[&[u8]]) {
    let len: usize = 8 + parts.iter().map(|p| p.len()).sum::<usize>();
    out.extend_from_slice(&(len as u32).to_be_bytes());
    out.extend_from_slice(typ);
    for p in parts {
        out.extend_from_slice(p);
    }
}

fn brotli_decompress(data: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    brotli::BrotliDecompress(&mut std::io::Cursor::new(data), &mut out)
        .map_err(|e| Error::InvalidData(format!("bad Brotli data in brob box: {e}")))?;
    Ok(out)
}

fn brotli_compress(data: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let params = brotli::enc::BrotliEncoderParams::default();
    brotli::BrotliCompress(&mut std::io::Cursor::new(data), &mut out, &params)
        .map_err(|e| Error::InvalidData(format!("Brotli compression failed: {e}")))?;
    Ok(out)
}

/// Does this file use the JPEG XL box layout (rather than JPEG 2000's)?
fn is_jxl(source: &[u8]) -> bool {
    source.starts_with(b"\0\0\0\x0cJXL \r\n\x87\n")
}

/// Apply the XMP changes to a JP2/JPX/JXL file.
pub fn write_jp2(source: &[u8], xmp: &XmpChanges) -> Result<Vec<u8>> {
    if xmp.is_empty() {
        return Ok(source.to_vec());
    }
    if source.starts_with(&[0xFF, 0x0A]) {
        // ExifTool would first wrap the codestream in an ISO BMFF container.
        return Err(Error::UnsupportedFileType(
            "a bare JPEG XL codestream has no box to hold XMP".into(),
        ));
    }
    let jxl = is_jxl(source);
    let list = boxes(source)?;
    let mut out = Vec::with_capacity(source.len() + 1024);
    let mut done = false;
    for (i, b) in list.iter().enumerate() {
        let last = i + 1 == list.len();
        if !done && b.to_end && last {
            // New boxes go before a box that runs to the end of the file.
            add_new(&mut out, xmp, jxl)?;
            done = true;
        }
        let is_uuid_xmp = &b.typ == b"uuid" && b.content.starts_with(&UUID_XMP);
        let is_xml = jxl && &b.typ == b"xml ";
        let is_brob_xml = jxl && &b.typ == b"brob" && b.content.len() >= 4 && {
            let t = &b.content[..4];
            t.eq_ignore_ascii_case(b"xml ")
        };
        if done || !(is_uuid_xmp || is_xml || is_brob_xml) {
            out.extend_from_slice(b.raw);
            continue;
        }
        done = true;
        let packet = if is_uuid_xmp {
            b.content[16..].to_vec()
        } else if is_xml {
            b.content.to_vec()
        } else {
            brotli_decompress(&b.content[4..])?
        };
        match xmp_edit::edit_xmp(Some(&packet), xmp)? {
            XmpOutcome::Unchanged => out.extend_from_slice(b.raw),
            XmpOutcome::Removed => {}
            XmpOutcome::Rewritten(x) => {
                if is_uuid_xmp {
                    put_box(&mut out, b"uuid", &[&UUID_XMP, &x]);
                } else if is_xml {
                    put_box(&mut out, b"xml ", &[&x]);
                } else {
                    let c = brotli_compress(&x)?;
                    put_box(&mut out, b"brob", &[b"xml ", &c]);
                }
            }
        }
    }
    if !done {
        add_new(&mut out, xmp, jxl)?;
    }
    Ok(out)
}

/// A new XMP box: `uuid` in a JP2, `xml ` in a JXL (uncompressed, as
/// ExifTool writes it without the Compress option).
fn add_new(out: &mut Vec<u8>, xmp: &XmpChanges, jxl: bool) -> Result<()> {
    if let XmpOutcome::Rewritten(x) = xmp_edit::edit_xmp(None, xmp)? {
        if jxl {
            put_box(out, b"xml ", &[&x]);
        } else {
            put_box(out, b"uuid", &[&UUID_XMP, &x]);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer::xmp_edit::{XmpSet, XmpValueKind};

    fn set_description(v: &str) -> XmpChanges {
        XmpChanges {
            set: vec![XmpSet {
                uri: "http://purl.org/dc/elements/1.1/".into(),
                prefix: "dc".into(),
                property: "description".into(),
                values: vec![v.into()],
                kind: XmpValueKind::LangAlt,
            }],
            toolkit: "test".into(),
            ..Default::default()
        }
    }

    fn jxl_with(meta: &[u8]) -> Vec<u8> {
        let mut f = b"\0\0\0\x0cJXL \r\n\x87\n".to_vec();
        put_box(&mut f, b"ftyp", &[b"jxl \0\0\0\0jxl "]);
        f.extend_from_slice(meta);
        put_box(&mut f, b"jxlc", &[b"\xff\x0acodestream"]);
        f
    }

    fn read_xmp_description(file: &[u8]) -> Option<String> {
        let tags = crate::formats::jp2::read_jp2(file).ok()?;
        tags.into_iter()
            .find(|t| t.name == "Description")
            .map(|t| t.print_value)
    }

    #[test]
    fn edits_brotli_xmp_in_place() {
        let mut first = Vec::new();
        let XmpOutcome::Rewritten(x) = xmp_edit::edit_xmp(None, &set_description("old")).unwrap()
        else {
            panic!("new packet");
        };
        put_box(
            &mut first,
            b"brob",
            &[b"xml ", &brotli_compress(&x).unwrap()],
        );
        let src = jxl_with(&first);
        let out = write_jp2(&src, &set_description("new")).unwrap();
        assert_eq!(read_xmp_description(&out).as_deref(), Some("new"));
        // Still compressed, and the codestream is intact at the end.
        let b = boxes(&out).unwrap();
        assert_eq!(&b[2].typ, b"brob");
        assert!(out.ends_with(b"\xff\x0acodestream"));
    }

    #[test]
    fn deletes_the_xmp_box() {
        let mut first = Vec::new();
        let XmpOutcome::Rewritten(x) = xmp_edit::edit_xmp(None, &set_description("old")).unwrap()
        else {
            panic!("new packet");
        };
        put_box(&mut first, b"xml ", &[&x]);
        let src = jxl_with(&first);
        let del = XmpChanges {
            delete_all: true,
            ..Default::default()
        };
        let out = write_jp2(&src, &del).unwrap();
        assert_eq!(out, jxl_with(&[]));
    }

    #[test]
    fn bare_codestream_is_refused() {
        assert!(write_jp2(b"\xff\x0a\0\0", &set_description("x")).is_err());
    }
}
