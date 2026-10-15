//! Photoshop image resource blocks (IRB), as ExifTool's `WritePhotoshop`
//! (WritePhotoshop.pl) rewrites them.
//!
//! A JPEG APP13 segment and a PSD image-resource section hold the same list
//! of resources: `8BIM <id:u16> <pascal name, even> <size:u32> <data, even>`.
//! Editing IPTC touches resource `0x0404` only: every other resource
//! (resolution, slices, `IPTCDigest`, ...) is copied as it was, and so is
//! the resource's name. ExifTool does not update `IPTCDigest` unless asked
//! to (`-IPTCDigest=new`), so neither does this.

use crate::error::{Error, Result};
use crate::writer::iptc_writer::{edit_iptc, IptcRequest};

/// Photoshop resource ID of the IPTC-IIM block.
pub const IPTC_RESOURCE: u16 = 0x0404;

/// The IPTC part of a write request.
#[derive(Debug, Clone, Default)]
pub struct IptcChanges {
    /// `IPTC:all`: delete the IPTC block (before any `set`).
    pub delete_all: bool,
    pub tags: Vec<IptcRequest>,
}

impl IptcChanges {
    pub fn is_empty(&self) -> bool {
        !self.delete_all && self.tags.is_empty()
    }
    /// Will this edit create IPTC where there is none?
    fn creates(&self) -> bool {
        self.tags.iter().any(|t| !t.values.is_empty())
    }
}

struct Resource<'a> {
    kind: &'a [u8],
    id: u16,
    /// The padded pascal-string name, as stored.
    name: &'a [u8],
    data: &'a [u8],
}

fn parse(data: &[u8]) -> Result<Vec<Resource<'_>>> {
    let bad = |why: &str| Error::InvalidData(format!("bad Photoshop IRB resource: {why}"));
    let mut out = Vec::new();
    let mut pos = 0;
    while pos + 8 < data.len() {
        // Each entry starts on an even offset.
        if pos & 1 == 1 {
            pos += 1;
        }
        if pos + 8 > data.len() {
            break;
        }
        let kind = &data[pos..pos + 4];
        if !matches!(kind, b"8BIM" | b"PHUT" | b"DCSR" | b"AgHg" | b"MeSa") {
            return Err(bad("unknown resource type"));
        }
        let id = u16::from_be_bytes([data[pos + 4], data[pos + 5]]);
        let mut name_len = 1 + data[pos + 6] as usize;
        if name_len & 1 == 1 {
            name_len += 1;
        }
        if pos + name_len + 10 > data.len() {
            return Err(bad("truncated header"));
        }
        let name = &data[pos + 6..pos + 6 + name_len];
        let s = pos + 6 + name_len;
        let size = u32::from_be_bytes([data[s], data[s + 1], data[s + 2], data[s + 3]]) as usize;
        let start = s + 4;
        if start + size > data.len() {
            return Err(bad("data size past the end"));
        }
        out.push(Resource {
            kind,
            id,
            name,
            data: &data[start..start + size],
        });
        pos = start + size;
    }
    Ok(out)
}

fn push_resource(out: &mut Vec<u8>, kind: &[u8], id: u16, name: &[u8], data: &[u8]) {
    out.extend_from_slice(kind);
    out.extend_from_slice(&id.to_be_bytes());
    out.extend_from_slice(name);
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(data);
    if data.len() & 1 == 1 {
        out.push(0);
    }
}

/// The data of resource `id` of a resource list, if there is one.
pub fn find_resource(irb: &[u8], id: u16) -> Option<&[u8]> {
    parse(irb)
        .ok()?
        .into_iter()
        .find(|r| r.id == id && r.kind == b"8BIM")
        .map(|r| r.data)
}

/// The IPTC-IIM block of a resource list, if there is one.
pub fn find_iptc(irb: &[u8]) -> Option<&[u8]> {
    find_resource(irb, IPTC_RESOURCE)
}

/// Apply `changes` to the resource list `irb` (empty when there is none).
/// Returns `None` when nothing changed, else the new list (empty when no
/// resource is left).
pub fn edit_irb(irb: &[u8], changes: &IptcChanges) -> Result<Option<Vec<u8>>> {
    if changes.is_empty() {
        return Ok(None);
    }
    let resources = parse(irb)?;
    let mut out = Vec::with_capacity(irb.len() + 64);
    let mut changed = false;
    let mut done_iptc = false;
    for r in &resources {
        if r.id == IPTC_RESOURCE && r.kind == b"8BIM" && !done_iptc {
            done_iptc = true;
            let old = if changes.delete_all { &[][..] } else { r.data };
            let new = edit_iptc(old, &changes.tags)?;
            if new.as_slice() != r.data {
                changed = true;
            }
            if new.is_empty() {
                continue; // no IPTC left: drop the resource
            }
            push_resource(&mut out, r.kind, r.id, r.name, &new);
        } else {
            push_resource(&mut out, r.kind, r.id, r.name, r.data);
        }
    }
    if !done_iptc && changes.creates() {
        let new = edit_iptc(&[], &changes.tags)?;
        if !new.is_empty() {
            // ExifTool appends the resources it creates.
            push_resource(&mut out, b"8BIM", IPTC_RESOURCE, b"\0\0", &new);
            changed = true;
        }
    }
    Ok(changed.then_some(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer::iptc_writer::{check_value, lookup_tag};

    fn set(name: &str, v: &str) -> IptcChanges {
        let tag = lookup_tag(name).unwrap();
        IptcChanges {
            delete_all: false,
            tags: vec![IptcRequest {
                tag,
                values: vec![check_value(tag, v).unwrap()],
            }],
        }
    }

    #[test]
    fn other_resources_survive_an_iptc_edit() {
        let mut irb = Vec::new();
        push_resource(&mut irb, b"8BIM", 0x03ED, b"\0\0", &[1, 2, 3, 4]);
        push_resource(&mut irb, b"8BIM", 0x0404, b"\0\0", b"\x1c\x02\x78\x00\x01a");
        push_resource(&mut irb, b"8BIM", 0x0425, b"\0\0", &[9; 16]);
        let out = edit_irb(&irb, &set("City", "Paris")).unwrap().unwrap();
        let res = parse(&out).unwrap();
        assert_eq!(
            res.iter().map(|r| r.id).collect::<Vec<_>>(),
            [0x03ED, 0x0404, 0x0425]
        );
        assert_eq!(res[0].data, &[1, 2, 3, 4]);
        assert_eq!(res[2].data, &[9; 16]);
        assert!(res[1].data.ends_with(b"Paris\x1c\x02\x00\x00\x02\x00\x04"));
    }

    #[test]
    fn delete_all_drops_only_the_iptc_resource() {
        let mut irb = Vec::new();
        push_resource(&mut irb, b"8BIM", 0x0404, b"\0\0", b"\x1c\x02\x78\x00\x01a");
        push_resource(&mut irb, b"8BIM", 0x03ED, b"\0\0", &[1, 2]);
        let del = IptcChanges {
            delete_all: true,
            tags: vec![],
        };
        let out = edit_irb(&irb, &del).unwrap().unwrap();
        let res = parse(&out).unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].id, 0x03ED);
    }
}
