//! HEIF item tables: the parts of a `meta` box that say which items a file
//! holds and where their bytes are.
//!
//! A HEIF/HEIC/AVIF image keeps its metadata as *items*, not as boxes: an
//! `infe` entry in `iinf` gives an item its type (`Exif`, or `mime` with the
//! content type `application/rdf+xml` for XMP), `iloc` says where its bytes
//! are (in `mdat` for construction method 0, in `idat` for method 1), and a
//! `cdsc` reference in `iref` ties it to the image it describes. ExifTool
//! reads these in QuickTime.pm (`ParseItemLocation`, `ParseItemInfoEntry`,
//! `ParseContentDescribes`, `HandleItemInfo`) and rewrites them in
//! WriteQuickTime.pl (`WriteItemInfo`, `Handle_iloc`).
//!
//! This module is the shared model: the reader uses it to find the Exif and
//! XMP items, the HEIF writer to rebuild the tables. Every parser here is
//! strict and bounds-checked: on malformed input it returns an error rather
//! than guessing, because the writer must never rewrite offsets it did not
//! fully understand.

use std::ops::Range;

/// Largest item payload this module will assemble or inflate (256 MiB). A
/// metadata item is a few kilobytes; the cap only exists so that a corrupt
/// length cannot make us allocate the address space.
const MAX_ITEM_SIZE: u64 = 256 << 20;

/// One box: its four-character type and its byte range in the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoxRef {
    pub kind: [u8; 4],
    /// Offset of the box header.
    pub start: usize,
    /// Size of the header (8, or 16 with a 64-bit `largesize`).
    pub header_len: usize,
    /// Offset just past the box.
    pub end: usize,
    /// The box's size field was 0 ("extends to the end of the file").
    pub to_eof: bool,
}

impl BoxRef {
    /// Byte range of the box contents (after the header).
    pub fn payload(&self) -> Range<usize> {
        self.start + self.header_len..self.end
    }
}

/// Split `data[start..end]` into consecutive boxes. Fails on a box that runs
/// past `end` or has an impossible size.
pub fn parse_boxes(data: &[u8], start: usize, end: usize) -> Result<Vec<BoxRef>, String> {
    let end = end.min(data.len());
    let mut boxes = Vec::new();
    let mut pos = start;
    while pos < end {
        if end - pos < 8 {
            // Trailing padding shorter than a box header: ExifTool ignores it
            // too, as long as it is all zeros.
            if data[pos..end].iter().all(|&b| b == 0) {
                break;
            }
            return Err(format!("truncated box header at offset {pos}"));
        }
        let size32 = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
        let kind = [data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]];
        let (size, header_len, to_eof) = match size32 {
            0 => ((end - pos) as u64, 8, true),
            1 => {
                if end - pos < 16 {
                    return Err(format!("truncated 64-bit box size at offset {pos}"));
                }
                let mut b = [0u8; 8];
                b.copy_from_slice(&data[pos + 8..pos + 16]);
                (u64::from_be_bytes(b), 16, false)
            }
            n => (n as u64, 8, false),
        };
        if size < header_len as u64 || size > (end - pos) as u64 {
            return Err(format!(
                "box '{}' at offset {pos} has an invalid size ({size})",
                String::from_utf8_lossy(&kind)
            ));
        }
        let box_end = pos + size as usize;
        boxes.push(BoxRef {
            kind,
            start: pos,
            header_len,
            end: box_end,
            to_eof,
        });
        pos = box_end;
    }
    Ok(boxes)
}

/// A small big-endian reader over a box payload.
struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if n > self.data.len() - self.pos {
            return Err("truncated item table".into());
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, String> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Result<u32, String> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    /// An unsigned integer of `size` bytes (0, 4 or 8; 0 reads as 0).
    fn var(&mut self, size: u8) -> Result<u64, String> {
        match size {
            0 => Ok(0),
            4 => Ok(self.u32()? as u64),
            8 => {
                let b = self.take(8)?;
                let mut a = [0u8; 8];
                a.copy_from_slice(b);
                Ok(u64::from_be_bytes(a))
            }
            n => Err(format!("unsupported iloc field size {n}")),
        }
    }
    /// A NUL-terminated string; a string cut short by the end of the box is
    /// taken as it is (ExifTool's `GetString` does the same).
    fn cstr(&mut self) -> Vec<u8> {
        let rest = &self.data[self.pos..];
        match rest.iter().position(|&b| b == 0) {
            Some(n) => {
                self.pos += n + 1;
                rest[..n].to_vec()
            }
            None => {
                self.pos = self.data.len();
                rest.to_vec()
            }
        }
    }
    fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }
}

/// Append `v` as a `size`-byte big-endian integer (0, 4 or 8).
pub(crate) fn put_var(out: &mut Vec<u8>, v: u64, size: u8) {
    match size {
        0 => {}
        4 => out.extend_from_slice(&(v as u32).to_be_bytes()),
        _ => out.extend_from_slice(&v.to_be_bytes()),
    }
}

/// Whether `v` can be stored in a `size`-byte field.
pub(crate) fn fits(v: u64, size: u8) -> bool {
    match size {
        0 => v == 0,
        4 => v <= u32::MAX as u64,
        _ => true,
    }
}

// ── iloc ────────────────────────────────────────────────────────────────────

/// One extent of an item: `length` bytes at `base_offset + offset`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extent {
    pub index: u64,
    pub offset: u64,
    pub length: u64,
}

/// The `iloc` entry of one item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemLocation {
    pub item_id: u32,
    /// 0 = file offset, 1 = offset into `idat`, 2 = offset into another item.
    pub construction_method: u8,
    /// The other 12 bits of the 16-bit field that holds the construction
    /// method (reserved, kept as found).
    pub cm_reserved: u16,
    /// 0 = this file; otherwise an entry in `dinf/dref`.
    pub data_reference_index: u16,
    pub base_offset: u64,
    pub extents: Vec<Extent>,
}

/// The item location box (ISO/IEC 14496-12 §8.11.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemLocations {
    pub version: u8,
    pub flags: [u8; 3],
    pub offset_size: u8,
    pub length_size: u8,
    pub base_offset_size: u8,
    pub index_size: u8,
    pub items: Vec<ItemLocation>,
}

fn check_size(n: u8) -> Result<u8, String> {
    if matches!(n, 0 | 4 | 8) {
        Ok(n)
    } else {
        Err(format!("unsupported iloc field size {n}"))
    }
}

impl ItemLocations {
    /// Parse an `iloc` payload (starting at its version byte).
    pub fn parse(payload: &[u8]) -> Result<Self, String> {
        let mut c = Cursor::new(payload);
        let version = c.u8()?;
        if version > 2 {
            return Err(format!("unsupported iloc version {version}"));
        }
        let f = c.take(3)?;
        let flags = [f[0], f[1], f[2]];
        let sizes = c.u16()?;
        let offset_size = check_size((sizes >> 12) as u8)?;
        let length_size = check_size(((sizes >> 8) & 0x0f) as u8)?;
        let base_offset_size = check_size(((sizes >> 4) & 0x0f) as u8)?;
        // index_size only exists in versions 1 and 2 (reserved in version 0).
        let index_size = if version == 0 {
            0
        } else {
            check_size((sizes & 0x0f) as u8)?
        };
        let count = if version < 2 {
            c.u16()? as usize
        } else {
            c.u32()? as usize
        };
        // Each entry takes at least 6 bytes: guards the allocation below.
        if count > c.remaining() / 6 + 1 {
            return Err("iloc item count exceeds the box".into());
        }
        let mut items = Vec::with_capacity(count);
        for _ in 0..count {
            let item_id = if version < 2 {
                c.u16()? as u32
            } else {
                c.u32()?
            };
            let (construction_method, cm_reserved) = if version >= 1 {
                let v = c.u16()?;
                ((v & 0x0f) as u8, v & 0xfff0)
            } else {
                (0, 0)
            };
            let data_reference_index = c.u16()?;
            let base_offset = c.var(base_offset_size)?;
            let extent_count = c.u16()? as usize;
            let mut extents = Vec::with_capacity(extent_count.min(c.remaining()));
            for _ in 0..extent_count {
                let index = if version >= 1 { c.var(index_size)? } else { 0 };
                let offset = c.var(offset_size)?;
                let length = c.var(length_size)?;
                extents.push(Extent {
                    index,
                    offset,
                    length,
                });
            }
            items.push(ItemLocation {
                item_id,
                construction_method,
                cm_reserved,
                data_reference_index,
                base_offset,
                extents,
            });
        }
        Ok(Self {
            version,
            flags,
            offset_size,
            length_size,
            base_offset_size,
            index_size,
            items,
        })
    }

    /// Serialize back to an `iloc` payload (starting at its version byte).
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = vec![self.version];
        out.extend_from_slice(&self.flags);
        let sizes = ((self.offset_size as u16) << 12)
            | ((self.length_size as u16) << 8)
            | ((self.base_offset_size as u16) << 4)
            | if self.version >= 1 {
                self.index_size as u16
            } else {
                0
            };
        out.extend_from_slice(&sizes.to_be_bytes());
        if self.version < 2 {
            out.extend_from_slice(&(self.items.len() as u16).to_be_bytes());
        } else {
            out.extend_from_slice(&(self.items.len() as u32).to_be_bytes());
        }
        for it in &self.items {
            if self.version < 2 {
                out.extend_from_slice(&(it.item_id as u16).to_be_bytes());
            } else {
                out.extend_from_slice(&it.item_id.to_be_bytes());
            }
            if self.version >= 1 {
                let v = it.cm_reserved | it.construction_method as u16;
                out.extend_from_slice(&v.to_be_bytes());
            }
            out.extend_from_slice(&it.data_reference_index.to_be_bytes());
            put_var(&mut out, it.base_offset, self.base_offset_size);
            out.extend_from_slice(&(it.extents.len() as u16).to_be_bytes());
            for e in &it.extents {
                if self.version >= 1 {
                    put_var(&mut out, e.index, self.index_size);
                }
                put_var(&mut out, e.offset, self.offset_size);
                put_var(&mut out, e.length, self.length_size);
            }
        }
        out
    }

    pub fn get(&self, id: u32) -> Option<&ItemLocation> {
        self.items.iter().find(|i| i.item_id == id)
    }
}

// ── iinf / infe ─────────────────────────────────────────────────────────────

/// One `infe` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemInfoEntry {
    pub item_id: u32,
    pub version: u8,
    pub protection_index: u16,
    /// Item type (`hvc1`, `av01`, `Exif`, `mime`, ...); zeros for versions 0/1.
    pub item_type: [u8; 4],
    pub name: Vec<u8>,
    pub content_type: Vec<u8>,
    pub content_encoding: Vec<u8>,
    /// The whole `infe` box, header included, as found in the file.
    pub raw: Vec<u8>,
}

impl ItemInfoEntry {
    /// Parse one `infe` box (header included).
    pub fn parse(raw: &[u8], header_len: usize) -> Result<Self, String> {
        let mut c = Cursor::new(&raw[header_len.min(raw.len())..]);
        let version = c.u8()?;
        c.take(3)?;
        let mut e = ItemInfoEntry {
            item_id: 0,
            version,
            protection_index: 0,
            item_type: [0; 4],
            name: Vec::new(),
            content_type: Vec::new(),
            content_encoding: Vec::new(),
            raw: raw.to_vec(),
        };
        match version {
            0 | 1 => {
                e.item_id = c.u16()? as u32;
                e.protection_index = c.u16()?;
                e.name = c.cstr();
                e.content_type = c.cstr();
                e.content_encoding = c.cstr();
            }
            2 | 3 => {
                e.item_id = if version == 2 {
                    c.u16()? as u32
                } else {
                    c.u32()?
                };
                e.protection_index = c.u16()?;
                let t = c.take(4)?;
                e.item_type = [t[0], t[1], t[2], t[3]];
                e.name = c.cstr();
                if &e.item_type == b"mime" {
                    e.content_type = c.cstr();
                    e.content_encoding = c.cstr();
                }
            }
            v => return Err(format!("unsupported infe version {v}")),
        }
        Ok(e)
    }

    /// The item is an Exif block.
    pub fn is_exif(&self) -> bool {
        &self.item_type == b"Exif"
    }

    /// The item is an XMP packet (`mime` item of type application/rdf+xml).
    pub fn is_xmp(&self) -> bool {
        // ExifTool keys on `ContentType || Type`, so a version 0/1 entry with
        // that content type counts as well.
        self.content_type == b"application/rdf+xml"
    }
}

/// The item information box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemInfo {
    pub version: u8,
    pub flags: [u8; 3],
    pub entries: Vec<ItemInfoEntry>,
    /// The entry count the box declares (readers follow the boxes; the
    /// writer refuses a box where the two disagree).
    pub declared_count: usize,
    /// The box holds something other than `infe` boxes.
    pub has_foreign_boxes: bool,
}

impl ItemInfo {
    /// Parse an `iinf` payload (starting at its version byte).
    pub fn parse(payload: &[u8]) -> Result<Self, String> {
        let mut c = Cursor::new(payload);
        let version = c.u8()?;
        let f = c.take(3)?;
        let flags = [f[0], f[1], f[2]];
        let count = if version == 0 {
            c.u16()? as usize
        } else {
            c.u32()? as usize
        };
        let start = c.pos;
        let boxes = parse_boxes(payload, start, payload.len())?;
        let mut entries = Vec::new();
        for b in boxes.iter().filter(|b| &b.kind == b"infe") {
            entries.push(ItemInfoEntry::parse(
                &payload[b.start..b.end],
                b.header_len,
            )?);
        }
        Ok(Self {
            version,
            flags,
            entries,
            declared_count: count,
            has_foreign_boxes: boxes.iter().any(|b| &b.kind != b"infe"),
        })
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut out = vec![self.version];
        out.extend_from_slice(&self.flags);
        if self.version == 0 {
            out.extend_from_slice(&(self.entries.len() as u16).to_be_bytes());
        } else {
            out.extend_from_slice(&(self.entries.len() as u32).to_be_bytes());
        }
        for e in &self.entries {
            out.extend_from_slice(&e.raw);
        }
        out
    }

    pub fn get(&self, id: u32) -> Option<&ItemInfoEntry> {
        self.entries.iter().find(|e| e.item_id == id)
    }
}

// ── iref ────────────────────────────────────────────────────────────────────

/// One reference: item `from` has a `kind` relation to each item in `to`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemReference {
    pub kind: [u8; 4],
    pub from: u32,
    pub to: Vec<u32>,
}

/// The item reference box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemReferences {
    /// 0 = 16-bit item IDs, 1 = 32-bit.
    pub version: u8,
    pub flags: [u8; 3],
    pub refs: Vec<ItemReference>,
}

impl ItemReferences {
    /// Parse an `iref` payload (starting at its version byte).
    pub fn parse(payload: &[u8]) -> Result<Self, String> {
        let mut c = Cursor::new(payload);
        let version = c.u8()?;
        if version > 1 {
            return Err(format!("unsupported iref version {version}"));
        }
        let f = c.take(3)?;
        let flags = [f[0], f[1], f[2]];
        let mut refs = Vec::new();
        for b in parse_boxes(payload, 4, payload.len())? {
            let mut r = Cursor::new(&payload[b.payload()]);
            let id = |r: &mut Cursor| -> Result<u32, String> {
                if version == 0 {
                    Ok(r.u16()? as u32)
                } else {
                    r.u32()
                }
            };
            let from = id(&mut r)?;
            let count = r.u16()? as usize;
            let mut to = Vec::with_capacity(count.min(r.remaining()));
            for _ in 0..count {
                to.push(id(&mut r)?);
            }
            if r.remaining() != 0 {
                return Err("iref entry has trailing bytes".into());
            }
            refs.push(ItemReference {
                kind: b.kind,
                from,
                to,
            });
        }
        Ok(Self {
            version,
            flags,
            refs,
        })
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut out = vec![self.version];
        out.extend_from_slice(&self.flags);
        let id_len = if self.version == 0 { 2 } else { 4 };
        for r in &self.refs {
            let size = 8 + id_len + 2 + id_len * r.to.len();
            out.extend_from_slice(&(size as u32).to_be_bytes());
            out.extend_from_slice(&r.kind);
            let put = |out: &mut Vec<u8>, v: u32| {
                if id_len == 2 {
                    out.extend_from_slice(&(v as u16).to_be_bytes());
                } else {
                    out.extend_from_slice(&v.to_be_bytes());
                }
            };
            put(&mut out, r.from);
            out.extend_from_slice(&(r.to.len() as u16).to_be_bytes());
            for &t in &r.to {
                put(&mut out, t);
            }
        }
        out
    }
}

// ── ipma ────────────────────────────────────────────────────────────────────

/// The item property association box, kept as raw per-item entries: the
/// writer only ever drops an entry, it never edits one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemPropertyAssociations {
    pub version: u8,
    pub flags: [u8; 3],
    /// (item ID, the entry's bytes after the item ID).
    pub entries: Vec<(u32, Vec<u8>)>,
}

impl ItemPropertyAssociations {
    /// Parse an `ipma` payload (starting at its version byte).
    pub fn parse(payload: &[u8]) -> Result<Self, String> {
        let mut c = Cursor::new(payload);
        let version = c.u8()?;
        let f = c.take(3)?;
        let flags = [f[0], f[1], f[2]];
        let count = c.u32()? as usize;
        if count > c.remaining() / 3 + 1 {
            return Err("ipma entry count exceeds the box".into());
        }
        let wide = flags[2] & 1 != 0;
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            let id = if version < 1 {
                c.u16()? as u32
            } else {
                c.u32()?
            };
            let start = c.pos;
            let n = c.u8()? as usize;
            c.take(n * if wide { 2 } else { 1 })?;
            entries.push((id, payload[start..c.pos].to_vec()));
        }
        if c.remaining() != 0 {
            return Err("ipma has trailing bytes".into());
        }
        Ok(Self {
            version,
            flags,
            entries,
        })
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut out = vec![self.version];
        out.extend_from_slice(&self.flags);
        out.extend_from_slice(&(self.entries.len() as u32).to_be_bytes());
        for (id, rest) in &self.entries {
            if self.version < 1 {
                out.extend_from_slice(&(*id as u16).to_be_bytes());
            } else {
                out.extend_from_slice(&id.to_be_bytes());
            }
            out.extend_from_slice(rest);
        }
        out
    }
}

// ── meta ────────────────────────────────────────────────────────────────────

/// Which metadata item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataKind {
    Exif,
    Xmp,
}

/// A parsed `meta` box: its children plus the item tables among them.
#[derive(Debug, Clone)]
pub struct Meta {
    /// Range of the `meta` payload (version/flags included).
    pub payload: Range<usize>,
    /// The `meta` children, in file order (offsets are absolute).
    pub children: Vec<BoxRef>,
    /// Item ID from `pitm`.
    pub pitm: Option<u32>,
    pub iloc: Option<ItemLocations>,
    pub iinf: Option<ItemInfo>,
    pub iref: Option<ItemReferences>,
    /// Range of the `idat` payload.
    pub idat: Option<Range<usize>>,
}

impl Meta {
    /// Parse the `meta` box whose payload (version/flags first) is
    /// `data[payload]`.
    pub fn parse(data: &[u8], payload: Range<usize>) -> Result<Self, String> {
        if payload.end > data.len() || payload.start + 4 > payload.end {
            return Err("truncated meta box".into());
        }
        let children = parse_boxes(data, payload.start + 4, payload.end)?;
        let mut meta = Meta {
            payload,
            children,
            pitm: None,
            iloc: None,
            iinf: None,
            iref: None,
            idat: None,
        };
        let once = |seen: bool, name: &str| {
            if seen {
                Err(format!("more than one {name} box in meta"))
            } else {
                Ok(())
            }
        };
        for b in meta.children.clone() {
            let p = &data[b.payload()];
            match &b.kind {
                b"pitm" => {
                    once(meta.pitm.is_some(), "pitm")?;
                    let mut c = Cursor::new(p);
                    let v = c.u8()?;
                    c.take(3)?;
                    meta.pitm = Some(if v == 0 { c.u16()? as u32 } else { c.u32()? });
                }
                b"iloc" => {
                    once(meta.iloc.is_some(), "iloc")?;
                    meta.iloc = Some(ItemLocations::parse(p)?);
                }
                b"iinf" => {
                    once(meta.iinf.is_some(), "iinf")?;
                    meta.iinf = Some(ItemInfo::parse(p)?);
                }
                b"iref" => {
                    once(meta.iref.is_some(), "iref")?;
                    meta.iref = Some(ItemReferences::parse(p)?);
                }
                b"idat" => {
                    once(meta.idat.is_some(), "idat")?;
                    meta.idat = Some(b.payload());
                }
                _ => {}
            }
        }
        Ok(meta)
    }

    /// The primary item: `pitm`, or else the lowest item ID (ExifTool
    /// assumes the same when `pitm` is missing).
    pub fn primary(&self) -> Option<u32> {
        self.pitm.or_else(|| {
            self.iinf
                .as_ref()
                .and_then(|i| i.entries.iter().map(|e| e.item_id).min())
        })
    }

    /// The items `from` refers to, through the reference types ExifTool
    /// follows (`cdsc` and `dimg`, QuickTime.pm `ParseContentDescribes`).
    pub fn refers_to(&self, from: u32) -> Vec<u32> {
        let mut out = Vec::new();
        if let Some(iref) = &self.iref {
            for r in &iref.refs {
                if r.from == from && (&r.kind == b"cdsc" || &r.kind == b"dimg") {
                    out.extend_from_slice(&r.to);
                }
            }
        }
        out
    }

    /// The Exif or XMP item that describes the primary image — the one
    /// ExifTool edits ("only edit primary EXIF/XMP metadata").
    pub fn primary_metadata_item(&self, kind: MetadataKind) -> Option<&ItemInfoEntry> {
        let primary = self.primary()?;
        self.iinf.as_ref()?.entries.iter().find(|e| {
            let right_kind = match kind {
                MetadataKind::Exif => e.is_exif(),
                MetadataKind::Xmp => e.is_xmp(),
            };
            right_kind && self.refers_to(e.item_id).contains(&primary)
        })
    }

    /// The metadata items the reader extracts into the main document: those
    /// that describe the primary item, or that describe nothing at all
    /// (ExifTool gives a document number only to items that refer to some
    /// *other* item).
    pub fn main_metadata_items(&self) -> Vec<&ItemInfoEntry> {
        let Some(iinf) = &self.iinf else {
            return Vec::new();
        };
        let primary = self.primary();
        let mut items: Vec<&ItemInfoEntry> = iinf
            .entries
            .iter()
            .filter(|e| e.is_exif() || e.is_xmp())
            .filter(|e| {
                let to = self.refers_to(e.item_id);
                to.is_empty() || primary.is_some_and(|p| to.contains(&p))
            })
            .collect();
        items.sort_by_key(|e| e.item_id);
        items
    }

    /// The bytes of item `id`, assembled from its extents and inflated if the
    /// item is `deflate`-encoded. Construction methods 0 (file) and 1
    /// (`idat`) are supported; anything else, a protected item or one stored
    /// in another file is an error.
    pub fn item_data(&self, data: &[u8], id: u32) -> Result<Vec<u8>, String> {
        let raw = self.item_stored_bytes(data, id)?;
        let enc = self
            .iinf
            .as_ref()
            .and_then(|i| i.get(id))
            .map(|e| e.content_encoding.clone())
            .unwrap_or_default();
        decode_content(&raw, &enc)
    }

    /// The stored (possibly still compressed) bytes of item `id`.
    pub fn item_stored_bytes(&self, data: &[u8], id: u32) -> Result<Vec<u8>, String> {
        let loc = self
            .iloc
            .as_ref()
            .and_then(|l| l.get(id))
            .ok_or_else(|| format!("item {id} has no iloc entry"))?;
        if let Some(e) = self.iinf.as_ref().and_then(|i| i.get(id)) {
            if e.protection_index != 0 {
                return Err(format!("item {id} is protected"));
            }
        }
        if loc.data_reference_index != 0 {
            return Err(format!("item {id} is stored in another file"));
        }
        let area = match loc.construction_method {
            0 => data,
            1 => {
                let r = self
                    .idat
                    .clone()
                    .ok_or_else(|| format!("item {id} is in idat, but there is no idat box"))?;
                &data[r]
            }
            m => return Err(format!("item {id} uses construction method {m}")),
        };
        let mut out = Vec::new();
        for e in &loc.extents {
            let start = loc
                .base_offset
                .checked_add(e.offset)
                .ok_or("item extent offset overflows")?;
            // ISO/IEC 14496-12 reads a zero length as "the whole resource",
            // but ExifTool reads (and writes, for an emptied item) it as an
            // empty extent; files ExifTool edited rely on that.
            let len = e.length;
            let end = start.checked_add(len).ok_or("item extent overflows")?;
            if end > area.len() as u64 {
                return Err(format!("item {id} extends past the end of its data"));
            }
            if out.len() as u64 + len > MAX_ITEM_SIZE {
                return Err(format!("item {id} is too large"));
            }
            out.extend_from_slice(&area[start as usize..end as usize]);
        }
        Ok(out)
    }
}

/// Undo an item's `content_encoding` (only `deflate` exists in practice).
pub fn decode_content(raw: &[u8], encoding: &[u8]) -> Result<Vec<u8>, String> {
    match encoding {
        b"" => Ok(raw.to_vec()),
        b"deflate" => {
            use std::io::Read;
            let mut out = Vec::new();
            flate2::read::ZlibDecoder::new(raw)
                .take(MAX_ITEM_SIZE)
                .read_to_end(&mut out)
                .map_err(|e| format!("error inflating item: {e}"))?;
            Ok(out)
        }
        other => Err(format!(
            "can't decode {} encoded item",
            String::from_utf8_lossy(other)
        )),
    }
}

/// Apply an item's `content_encoding` to new content.
pub fn encode_content(plain: &[u8], encoding: &[u8]) -> Result<Vec<u8>, String> {
    match encoding {
        b"" => Ok(plain.to_vec()),
        b"deflate" => {
            use std::io::Write;
            let mut enc =
                flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            enc.write_all(plain)
                .and_then(|_| enc.finish())
                .map_err(|e| format!("error deflating item: {e}"))
        }
        other => Err(format!(
            "can't encode {} encoded item",
            String::from_utf8_lossy(other)
        )),
    }
}

/// Find the top-level `meta` box of a HEIF file and parse it.
pub fn top_level_meta(data: &[u8]) -> Result<(Vec<BoxRef>, usize, Meta), String> {
    let boxes = parse_boxes(data, 0, data.len())?;
    let mut metas = boxes.iter().enumerate().filter(|(_, b)| &b.kind == b"meta");
    let (idx, b) = metas.next().ok_or("no top-level meta box")?;
    if metas.next().is_some() {
        return Err("more than one top-level meta box".into());
    }
    let meta = Meta::parse(data, b.payload())?;
    Ok((boxes, idx, meta))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iloc_round_trips_every_version_and_size() {
        for version in 0..=2u8 {
            for &(o, l, b, i) in &[
                (4u8, 4u8, 0u8, 0u8),
                (8, 8, 8, 4),
                (0, 4, 4, 0),
                (4, 0, 8, 8),
            ] {
                let iloc = ItemLocations {
                    version,
                    flags: [0; 3],
                    offset_size: o,
                    length_size: l,
                    base_offset_size: b,
                    index_size: if version == 0 { 0 } else { i },
                    items: vec![ItemLocation {
                        item_id: 7,
                        construction_method: if version == 0 { 0 } else { 1 },
                        cm_reserved: 0,
                        data_reference_index: 0,
                        base_offset: if b == 0 { 0 } else { 99 },
                        extents: vec![Extent {
                            index: if version == 0 || i == 0 { 0 } else { 3 },
                            offset: if o == 0 { 0 } else { 12 },
                            length: if l == 0 { 0 } else { 34 },
                        }],
                    }],
                };
                let bytes = iloc.serialize();
                assert_eq!(ItemLocations::parse(&bytes).unwrap(), iloc);
            }
        }
    }

    #[test]
    fn iloc_rejects_bad_sizes_and_truncation() {
        // offset_size 2 is not a legal value.
        assert!(ItemLocations::parse(&[0, 0, 0, 0, 0x24, 0x40, 0, 0]).is_err());
        // Declares one item, holds nothing.
        assert!(ItemLocations::parse(&[0, 0, 0, 0, 0x44, 0x40, 0, 1]).is_err());
    }

    #[test]
    fn iref_round_trips() {
        for version in 0..=1u8 {
            let iref = ItemReferences {
                version,
                flags: [0; 3],
                refs: vec![
                    ItemReference {
                        kind: *b"cdsc",
                        from: 2,
                        to: vec![1],
                    },
                    ItemReference {
                        kind: *b"dimg",
                        from: 5,
                        to: vec![3, 4],
                    },
                ],
            };
            assert_eq!(ItemReferences::parse(&iref.serialize()).unwrap(), iref);
        }
    }

    #[test]
    fn deflate_round_trips() {
        let plain = b"<x:xmpmeta/>".repeat(20);
        let packed = encode_content(&plain, b"deflate").unwrap();
        assert_eq!(decode_content(&packed, b"deflate").unwrap(), plain);
        assert!(decode_content(&packed, b"gzip").is_err());
    }

    #[test]
    fn boxes_reject_overruns() {
        // A box claiming 64 bytes in a 16-byte buffer.
        let mut d = vec![0, 0, 0, 64];
        d.extend_from_slice(b"free");
        d.extend_from_slice(&[0; 8]);
        assert!(parse_boxes(&d, 0, d.len()).is_err());
        // Size 0 runs to the end.
        d[3] = 0;
        let b = parse_boxes(&d, 0, d.len()).unwrap();
        assert_eq!(b.len(), 1);
        assert!(b[0].to_eof);
        assert_eq!(b[0].end, 16);
    }
}
