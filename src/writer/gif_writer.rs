//! GIF metadata writer: the comment extension (File:Comment).
//!
//! Follows ExifTool's `ProcessGIF` (GIF.pm): the GIF is walked block by
//! block, so image data is copied unchanged and never mistaken for an
//! extension. Writing a comment replaces the first comment extension and
//! drops the others; when there is none before the first image, the new one
//! goes right before it. Deleting the comment drops every comment extension.

use crate::error::{Error, Result};

/// What to do with the comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GifComment<'a> {
    Keep,
    Delete,
    Set(&'a str),
}

/// Read a chain of data sub-blocks starting at `pos`: returns the end
/// (past the zero terminator) and the concatenated data.
fn sub_blocks(src: &[u8], mut pos: usize) -> Result<(usize, Vec<u8>)> {
    let mut data = Vec::new();
    loop {
        let n = *src
            .get(pos)
            .ok_or_else(|| Error::InvalidData("truncated GIF block".into()))?
            as usize;
        pos += 1;
        if n == 0 {
            return Ok((pos, data));
        }
        let chunk = src
            .get(pos..pos + n)
            .ok_or_else(|| Error::InvalidData("truncated GIF block".into()))?;
        data.extend_from_slice(chunk);
        pos += n;
    }
}

fn write_comment(out: &mut Vec<u8>, comment: &[u8]) {
    out.extend_from_slice(&[0x21, 0xFE]);
    for chunk in comment.chunks(255) {
        out.push(chunk.len() as u8);
        out.extend_from_slice(chunk);
    }
    out.push(0);
}

pub fn write_gif(source: &[u8], comment: GifComment) -> Result<Vec<u8>> {
    if source.len() < 13 || !source.starts_with(b"GIF8") {
        return Err(Error::InvalidData("not a GIF file".into()));
    }
    if comment == GifComment::Keep {
        return Ok(source.to_vec());
    }
    let mut out = Vec::with_capacity(source.len() + 64);
    // Header; a new comment needs GIF89a.
    if matches!(comment, GifComment::Set(_)) {
        out.extend_from_slice(b"GIF89a");
    } else {
        out.extend_from_slice(&source[..6]);
    }
    out.extend_from_slice(&source[6..13]);
    let mut pos = 13;
    let flags = source[10];
    if flags & 0x80 != 0 {
        let n = 3 * (2usize << (flags & 0x07));
        let table = source
            .get(pos..pos + n)
            .ok_or_else(|| Error::InvalidData("truncated GIF color table".into()))?;
        out.extend_from_slice(table);
        pos += n;
    }

    // The comment still to write; once written (or deleted), any further
    // comment extension is dropped.
    let mut pending: Option<&[u8]> = match comment {
        GifComment::Set(c) => Some(c.as_bytes()),
        _ => None,
    };
    loop {
        let Some(&b) = source.get(pos) else {
            return Err(Error::InvalidData("GIF has no trailer".into()));
        };
        if b != 0x21 {
            if let Some(c) = pending.take() {
                write_comment(&mut out, c);
            }
        }
        match b {
            0x2C => {
                // Image descriptor, optional local color table, LZW code
                // size, then the image data sub-blocks: copied as they are.
                let start = pos;
                let desc = source
                    .get(pos..pos + 10)
                    .ok_or_else(|| Error::InvalidData("truncated GIF image".into()))?;
                let mut p = pos + 10;
                if desc[9] & 0x80 != 0 {
                    p += 3 * (2usize << (desc[9] & 0x07));
                }
                p += 1; // LZW minimum code size
                let (end, _) = sub_blocks(source, p)?;
                out.extend_from_slice(&source[start..end]);
                pos = end;
            }
            0x21 => {
                let label = *source
                    .get(pos + 1)
                    .ok_or_else(|| Error::InvalidData("truncated GIF extension".into()))?;
                let (end, _) = sub_blocks(source, pos + 2)?;
                if label == 0xFE {
                    // The first one is replaced by the new comment; the
                    // others (and all of them when deleting) are dropped.
                    if let Some(c) = pending.take() {
                        write_comment(&mut out, c);
                    }
                } else {
                    out.extend_from_slice(&source[pos..end]);
                }
                pos = end;
            }
            _ => {
                // The trailer (0x3B), and whatever follows it.
                out.extend_from_slice(&source[pos..]);
                return Ok(out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1x1 GIF89a whose image data contains the bytes 0x21 0xFE, with a
    /// comment before the image.
    fn sample() -> Vec<u8> {
        let mut g = b"GIF89a\x01\0\x01\0\0\0\0".to_vec();
        write_comment(&mut g, b"old comment");
        g.extend_from_slice(&[0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0]);
        g.extend_from_slice(&[2, 3, 0x21, 0xFE, 0x01, 0]);
        g.push(0x3B);
        g
    }

    #[test]
    fn replaces_the_comment_and_keeps_image_data() {
        let out = write_gif(&sample(), GifComment::Set("new")).unwrap();
        let mut want = b"GIF89a\x01\0\x01\0\0\0\0".to_vec();
        write_comment(&mut want, b"new");
        want.extend_from_slice(&[0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0]);
        want.extend_from_slice(&[2, 3, 0x21, 0xFE, 0x01, 0]);
        want.push(0x3B);
        assert_eq!(out, want);
    }

    #[test]
    fn deletes_the_comment() {
        let out = write_gif(&sample(), GifComment::Delete).unwrap();
        let mut want = b"GIF89a\x01\0\x01\0\0\0\0".to_vec();
        want.extend_from_slice(&[0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0]);
        want.extend_from_slice(&[2, 3, 0x21, 0xFE, 0x01, 0]);
        want.push(0x3B);
        assert_eq!(out, want);
    }
}
