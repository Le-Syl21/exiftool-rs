//! The Photoshop "Ducky" APP12 segment of a JPEG, edited as ExifTool's
//! `WriteDucky` (APP12.pm) does: `Ducky`, then blocks `<tag:u16> <len:u16>
//! <data>` ended by a zero tag. Tag 1 is Quality (int32u), 2 Comment and 3
//! Copyright (a u32 length, then UTF-16BE text).
//!
//! The three tags are `Avoid`: an ungrouped request only replaces a tag
//! the segment has, and only `Ducky:Tag` creates one.

use crate::error::Result;

/// One request for a Ducky tag.
#[derive(Debug, Clone)]
pub struct DuckyEdit {
    /// 1 Quality, 2 Comment, 3 Copyright.
    pub tag: u16,
    /// The new value; `None` deletes the tag.
    pub value: Option<String>,
    /// Create the tag when the segment lacks it (a `Ducky:` request).
    pub create: bool,
}

/// The edits to the Ducky segment.
#[derive(Debug, Clone, Default)]
pub struct DuckyChanges {
    /// `Ducky:all` (or `-all=`): the segment goes.
    pub delete_all: bool,
    pub edits: Vec<DuckyEdit>,
}

impl DuckyChanges {
    pub fn is_empty(&self) -> bool {
        !self.delete_all && self.edits.is_empty()
    }
}

/// The Ducky tag number of an ExifTool tag name.
pub fn tag_id(name: &str) -> Option<u16> {
    match name.to_ascii_lowercase().as_str() {
        "quality" => Some(1),
        "comment" => Some(2),
        "copyright" => Some(3),
        _ => None,
    }
}

/// The stored form of a value (`ValueConvInv` after `PrintConvInv`), or
/// `None` when ExifTool rejects it.
fn encode(tag: u16, value: &str) -> Option<Vec<u8>> {
    if tag == 1 {
        // PrintConvInv: $val=~/(\d+)/ ? $1 : undef; ValueConvInv: pack("N").
        let digits: String = value
            .chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(char::is_ascii_digit)
            .collect();
        let n: u64 = digits.parse().ok()?;
        return Some((n as u32).to_be_bytes().to_vec());
    }
    // pack("N", length $val) . Encode($val, "UTF16", "MM"): the length is
    // that of ExifTool's UTF-8 byte string.
    let mut out = (value.len() as u32).to_be_bytes().to_vec();
    for u in value.encode_utf16() {
        out.extend_from_slice(&u.to_be_bytes());
    }
    Some(out)
}

/// Can every value of `changes` be stored?
pub fn valid(changes: &DuckyChanges) -> bool {
    changes.edits.iter().all(|e| {
        e.value
            .as_deref()
            .is_none_or(|v| encode(e.tag, v).is_some())
    })
}

/// The new payload of a Ducky segment (`data`: past the `Ducky` header, or
/// `None` to create one). `Ok(None)`: unchanged; `Ok(Some(empty))`: the
/// segment goes.
pub fn edit(data: Option<&[u8]>, changes: &DuckyChanges) -> Result<Option<Vec<u8>>> {
    if changes.delete_all {
        return Ok(data.map(|_| Vec::new()));
    }
    let d = data.unwrap_or_default();
    let mut blocks: Vec<(u16, Vec<u8>)> = Vec::new();
    let mut p = 0;
    while p + 4 <= d.len() {
        let tag = u16::from_be_bytes([d[p], d[p + 1]]);
        if tag == 0 {
            break;
        }
        let len = u16::from_be_bytes([d[p + 2], d[p + 3]]) as usize;
        if p + 4 + len > d.len() {
            return Err(crate::error::Error::InvalidData(
                "invalid Ducky block length".into(),
            ));
        }
        blocks.push((tag, d[p + 4..p + 4 + len].to_vec()));
        p += 4 + len;
    }
    let mut changed = false;
    for e in &changes.edits {
        let at = blocks.iter().position(|(t, _)| *t == e.tag);
        match (&e.value, at) {
            (None, Some(i)) => {
                blocks.remove(i);
                changed = true;
            }
            (None, None) => {}
            (Some(v), at) => {
                let Some(bytes) = encode(e.tag, v) else {
                    continue;
                };
                match at {
                    Some(i) if blocks[i].1 != bytes => {
                        blocks[i].1 = bytes;
                        changed = true;
                    }
                    Some(_) => {}
                    None if e.create => {
                        // Added after the existing blocks, highest tag first
                        // (`pop @addTags`).
                        blocks.push((e.tag, bytes));
                        changed = true;
                    }
                    None => {}
                }
            }
        }
    }
    if !changed {
        return Ok(None);
    }
    let mut out = Vec::new();
    for (tag, v) in &blocks {
        out.extend_from_slice(&tag.to_be_bytes());
        out.extend_from_slice(&(v.len() as u16).to_be_bytes());
        out.extend_from_slice(v);
    }
    if !out.is_empty() {
        out.extend_from_slice(&[0, 0]);
    }
    Ok(Some(out))
}

/// Apply `changes` to the Ducky APP12 segment of a JPEG (creating one after
/// the segments ExifTool writes before it when a `Ducky:` request needs it).
pub fn rewrite(jpeg: Vec<u8>, changes: &DuckyChanges) -> Result<Vec<u8>> {
    if changes.is_empty() {
        return Ok(jpeg);
    }
    // The segments before the scan.
    let mut pos = 2;
    let mut found: Option<(usize, usize)> = None;
    let mut sos = jpeg.len();
    while pos + 4 <= jpeg.len() && jpeg[pos] == 0xFF {
        let m = jpeg[pos + 1];
        if m == 0xFF {
            pos += 1;
            continue;
        }
        if m == 0xDA || m == 0xD9 {
            sos = pos;
            break;
        }
        if m == 0x01 || (0xD0..=0xD8).contains(&m) {
            pos += 2;
            continue;
        }
        let len = u16::from_be_bytes([jpeg[pos + 2], jpeg[pos + 3]]) as usize;
        if len < 2 || pos + 2 + len > jpeg.len() {
            break;
        }
        if m == 0xEC && found.is_none() && jpeg[pos + 4..pos + 2 + len].starts_with(b"Ducky") {
            found = Some((pos, pos + 2 + len));
        }
        pos += 2 + len;
    }
    let (start, end, old) = match found {
        Some((s, e)) => (s, e, Some(&jpeg[s + 9..e])),
        None => (sos, sos, None),
    };
    let Some(new) = edit(old, changes)? else {
        return Ok(jpeg);
    };
    let mut seg = Vec::new();
    if !new.is_empty() {
        let len = u16::try_from(new.len() + 7)
            .map_err(|_| crate::error::Error::InvalidData("Ducky segment too large".into()))?;
        seg.extend_from_slice(&[0xFF, 0xEC]);
        seg.extend_from_slice(&len.to_be_bytes());
        seg.extend_from_slice(b"Ducky");
        seg.extend_from_slice(&new);
    }
    let mut out = jpeg[..start].to_vec();
    out.extend_from_slice(&seg);
    out.extend_from_slice(&jpeg[end..]);
    let shift = out.len() as i64 - jpeg.len() as i64;
    if shift != 0 && sos < jpeg.len() {
        let out_sos = (sos as i64 + shift) as usize;
        crate::writer::jpeg_writer::fix_offsets_after_scan(&jpeg, &mut out, sos + 2, out_sos + 2);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Ducky segment of t/images/ExifTool.jpg: Quality 50, Copyright.
    fn ducky() -> Vec<u8> {
        let mut d = vec![0, 1, 0, 4, 0, 0, 0, 50];
        let c = encode(3, "Copyright 2004 Phil Harvey").unwrap();
        d.extend_from_slice(&[0, 3]);
        d.extend_from_slice(&(c.len() as u16).to_be_bytes());
        d.extend_from_slice(&c);
        d.extend_from_slice(&[0, 0]);
        d
    }

    #[test]
    fn ungrouped_requests_only_edit_and_delete() {
        let ch = DuckyChanges {
            delete_all: false,
            edits: vec![
                DuckyEdit {
                    tag: 3,
                    value: Some("New".into()),
                    create: false,
                },
                DuckyEdit {
                    tag: 2,
                    value: Some("Not created".into()),
                    create: false,
                },
            ],
        };
        let out = edit(Some(&ducky()), &ch).unwrap().unwrap();
        assert_eq!(&out[8..12], &[0, 3, 0, 10]);
        assert_eq!(&out[12..22], b"\0\0\0\x03\0N\0e\0w");
        assert_eq!(out.len(), 24);
        // Deleting both tags leaves nothing: the segment goes.
        let del = |tag| DuckyEdit {
            tag,
            value: None,
            create: false,
        };
        let ch = DuckyChanges {
            delete_all: false,
            edits: vec![del(1), del(3)],
        };
        assert!(edit(Some(&ducky()), &ch).unwrap().unwrap().is_empty());
    }
}
