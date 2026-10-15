//! PSD/PSB metadata writer: the image resources (IRB) section.
//!
//! Follows ExifTool's `WritePhotoshop` (WritePhotoshop.pl): the resource
//! blocks are rewritten in their order, keeping each block's type and name;
//! an edited resource is replaced where it is, a removed one is dropped,
//! and a new one is appended (resources are added in ID order). The header,
//! color mode data, layers and image data are copied unchanged.

use crate::error::{Error, Result};

/// The IPTC-IIM resource.
pub const IPTC: u16 = 0x0404;
/// The XMP resource.
pub const XMP: u16 = 0x0424;

/// What becomes of one resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceChange {
    Keep,
    Remove,
    Replace(Vec<u8>),
}

struct Block<'a> {
    /// Type, ID and name, as found (`8BIM` + ID + padded Pascal string).
    head: &'a [u8],
    id: u16,
    is_8bim: bool,
    data: &'a [u8],
    /// The block as found, padding included.
    raw: &'a [u8],
}

/// The bounds of the image resources section: (start of the blocks, end).
fn irb_section(source: &[u8]) -> Result<(usize, usize)> {
    let bad = || Error::InvalidData("truncated PSD".into());
    if source.len() < 26 || !source.starts_with(b"8BPS") {
        return Err(Error::InvalidData("not a PSD file".into()));
    }
    let u32_at = |p: usize| -> Result<usize> {
        let b = source.get(p..p + 4).ok_or_else(bad)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize)
    };
    let irb = 26 + 4 + u32_at(26)?;
    let start = irb + 4;
    let end = start + u32_at(irb)?;
    if end > source.len() {
        return Err(bad());
    }
    Ok((start, end))
}

fn blocks(source: &[u8], start: usize, end: usize) -> Result<Vec<Block<'_>>> {
    let bad = || Error::InvalidData("bad Photoshop IRB resource".into());
    let mut out = Vec::new();
    let mut pos = start;
    while pos + 8 < end {
        let sig = &source[pos..pos + 4];
        if !matches!(sig, b"8BIM" | b"PHUT" | b"DCSR" | b"AgHg" | b"MeSa") {
            return Err(bad());
        }
        let id = u16::from_be_bytes([source[pos + 4], source[pos + 5]]);
        let mut name_len = 1 + source[pos + 6] as usize;
        name_len += name_len & 1;
        let size_at = pos + 6 + name_len;
        let size = source
            .get(size_at..size_at + 4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize)
            .ok_or_else(bad)?;
        let data_at = size_at + 4;
        if data_at + size > end {
            return Err(bad());
        }
        let next = (data_at + size + (size & 1)).min(end);
        out.push(Block {
            head: &source[pos..size_at],
            id,
            is_8bim: sig == b"8BIM",
            data: &source[data_at..data_at + size],
            raw: &source[pos..next],
        });
        pos = next;
    }
    Ok(out)
}

/// The data of the first `8BIM` resource `id`, if the file has one.
pub fn resource(source: &[u8], id: u16) -> Result<Option<&[u8]>> {
    let (start, end) = irb_section(source)?;
    Ok(blocks(source, start, end)?
        .into_iter()
        .find(|b| b.is_8bim && b.id == id)
        .map(|b| b.data))
}

fn put_block(out: &mut Vec<u8>, head: &[u8], data: &[u8]) {
    out.extend_from_slice(head);
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(data);
    if data.len() % 2 == 1 {
        out.push(0);
    }
}

/// Rewrite the PSD with the given resource changes, `(id, change)`.
pub fn write_psd(source: &[u8], changes: &[(u16, ResourceChange)]) -> Result<Vec<u8>> {
    let (start, end) = irb_section(source)?;
    if changes.iter().all(|(_, c)| *c == ResourceChange::Keep) {
        return Ok(source.to_vec());
    }
    let list = blocks(source, start, end)?;
    let mut irb = Vec::with_capacity(end - start + 1024);
    let mut done: Vec<u16> = Vec::new();
    for b in &list {
        let change = changes
            .iter()
            .find(|(id, _)| b.is_8bim && *id == b.id && !done.contains(id))
            .map(|(_, c)| c);
        match change {
            None | Some(ResourceChange::Keep) => irb.extend_from_slice(b.raw),
            Some(ResourceChange::Remove) => done.push(b.id),
            Some(ResourceChange::Replace(d)) => {
                done.push(b.id);
                put_block(&mut irb, b.head, d);
            }
        }
    }
    let mut new: Vec<&(u16, ResourceChange)> = changes
        .iter()
        .filter(|(id, c)| matches!(c, ResourceChange::Replace(_)) && !done.contains(id))
        .collect();
    new.sort_by_key(|(id, _)| *id);
    for (id, c) in new {
        if let ResourceChange::Replace(d) = c {
            let mut head = b"8BIM".to_vec();
            head.extend_from_slice(&id.to_be_bytes());
            head.extend_from_slice(&[0, 0]); // empty name, padded
            put_block(&mut irb, &head, d);
        }
    }

    let mut out = Vec::with_capacity(source.len() + 1024);
    out.extend_from_slice(&source[..start - 4]);
    out.extend_from_slice(&(irb.len() as u32).to_be_bytes());
    out.extend_from_slice(&irb);
    out.extend_from_slice(&source[end..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn psd(resources: &[(u16, &[u8])]) -> Vec<u8> {
        let mut f = b"8BPS\0\x01\0\0\0\0\0\0\0\x03\0\0\0\x01\0\0\0\x01\0\x08\0\x03".to_vec();
        f.extend_from_slice(&0u32.to_be_bytes()); // color mode data
        let mut irb = Vec::new();
        for (id, d) in resources {
            let mut head = b"8BIM".to_vec();
            head.extend_from_slice(&id.to_be_bytes());
            head.extend_from_slice(&[0, 0]);
            put_block(&mut irb, &head, d);
        }
        f.extend_from_slice(&(irb.len() as u32).to_be_bytes());
        f.extend_from_slice(&irb);
        f.extend_from_slice(b"layers+image");
        f
    }

    #[test]
    fn replaces_removes_and_adds_resources() {
        let src = psd(&[(0x03ed, b"res"), (IPTC, b"old iptc"), (0x0425, b"digest")]);
        let out = write_psd(
            &src,
            &[
                (IPTC, ResourceChange::Remove),
                (XMP, ResourceChange::Replace(b"<x/>".to_vec())),
            ],
        )
        .unwrap();
        assert_eq!(
            out,
            psd(&[(0x03ed, b"res"), (0x0425, b"digest"), (XMP, b"<x/>")])
        );
        assert_eq!(resource(&out, XMP).unwrap(), Some(&b"<x/>"[..]));
        assert!(out.ends_with(b"layers+image"));
    }
}
