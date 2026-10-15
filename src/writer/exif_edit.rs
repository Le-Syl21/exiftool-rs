//! Edit an existing EXIF (TIFF) block without rebuilding it from scratch.
//!
//! [`exif_writer::build_exif`](crate::writer::exif_writer::build_exif) writes
//! a fresh IFD0/ExifIFD/GPS block from a list of entries, which loses
//! everything it does not model (IFD1 and its thumbnail, the Interop IFD,
//! maker notes and their private offsets). This module edits a block in
//! place instead, the way ExifTool's `WriteExif` preserves what it is not
//! asked to change:
//!
//! - an edited directory is rewritten at its old offset when it still fits,
//!   and moved to the end of the block when it grows;
//! - a new or longer value goes to the end of the block, a shorter one stays
//!   where it was;
//! - everything else -- maker note, thumbnail, unknown tags, values of
//!   untouched entries -- keeps its offset, so internal pointers stay valid;
//! - the bytes of a deleted entry, value or directory are zeroed, so removed
//!   metadata cannot be recovered from the file.
//!
//! The group semantics follow ExifTool 13.59: `EXIF:all` and `IFD0:all` drop
//! the whole block, `ExifIFD:all`, `GPS:all`, `InteropIFD:all` and
//! `IFD1:all` drop that directory, and a directory left holding only its
//! mandatory tags after a deletion is dropped too (WriteExif.pl
//! `%mandatory`), which is how deleting the last real tag of an image
//! removes its EXIF entirely.

use std::collections::HashMap;
use std::ops::Range;

use crate::error::{Error, Result};
use crate::metadata::exif::ByteOrderMark;

/// The EXIF directories this editor understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ifd {
    Ifd0,
    Ifd1,
    ExifIfd,
    Gps,
    Interop,
}

impl Ifd {
    /// ExifTool's family-1 group name for the directory.
    pub fn name(self) -> &'static str {
        match self {
            Ifd::Ifd0 => "IFD0",
            Ifd::Ifd1 => "IFD1",
            Ifd::ExifIfd => "ExifIFD",
            Ifd::Gps => "GPS",
            Ifd::Interop => "InteropIFD",
        }
    }

    /// The directory a group name designates (case-insensitive).
    pub fn from_group(group: &str) -> Option<Ifd> {
        match group.to_ascii_lowercase().as_str() {
            "ifd0" => Some(Ifd::Ifd0),
            "ifd1" => Some(Ifd::Ifd1),
            "exififd" => Some(Ifd::ExifIfd),
            "gps" => Some(Ifd::Gps),
            "interopifd" => Some(Ifd::Interop),
            _ => None,
        }
    }

    /// The directory holding the pointer to this one, and the pointer's tag.
    fn pointer(self) -> Option<(Ifd, u16)> {
        match self {
            Ifd::ExifIfd => Some((Ifd::Ifd0, 0x8769)),
            Ifd::Gps => Some((Ifd::Ifd0, 0x8825)),
            Ifd::Interop => Some((Ifd::ExifIfd, 0xA005)),
            Ifd::Ifd0 | Ifd::Ifd1 => None,
        }
    }

    /// Children first: the order in which directories are written, so that
    /// a parent knows where its children ended up.
    const BOTTOM_UP: [Ifd; 5] = [Ifd::Interop, Ifd::ExifIfd, Ifd::Gps, Ifd::Ifd1, Ifd::Ifd0];
}

const POINTER_TAGS: [u16; 3] = [0x8769, 0x8825, 0xA005];

/// A value to write: tag `tag` in `ifd`, TIFF type `format`, encoded in
/// both byte orders (the block's own order is only known when it is read).
#[derive(Debug, Clone)]
pub struct SetValue {
    pub ifd: Ifd,
    pub tag: u16,
    pub format: u16,
    pub big_endian: Vec<u8>,
    pub little_endian: Vec<u8>,
}

/// The EXIF part of a write request.
#[derive(Debug, Clone, Default)]
pub struct ExifChanges {
    /// `EXIF:all` (or `IFD0:all`, or `all`): drop the whole block first.
    pub delete_all: bool,
    /// Directories to drop (`GPS:all`, ...).
    pub delete_dirs: Vec<Ifd>,
    /// `MakerNotes:all`.
    pub delete_maker_notes: bool,
    /// Tags to delete: optional directory, lower-case tag name.
    pub delete_tags: Vec<(Option<Ifd>, String)>,
    /// Tags to write.
    pub set: Vec<SetValue>,
}

impl ExifChanges {
    pub fn is_empty(&self) -> bool {
        !self.delete_all
            && self.delete_dirs.is_empty()
            && !self.delete_maker_notes
            && self.delete_tags.is_empty()
            && self.set.is_empty()
    }
}

/// What became of the EXIF block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExifOutcome {
    /// Nothing was changed.
    Unchanged,
    /// No EXIF is left.
    Removed,
    /// The new TIFF block.
    Rewritten(Vec<u8>),
}

fn type_size(t: u16) -> Option<usize> {
    match t {
        1 | 2 | 6 | 7 => Some(1),
        3 | 8 => Some(2),
        4 | 9 | 11 | 13 => Some(4),
        5 | 10 | 12 => Some(8),
        _ => None,
    }
}

#[derive(Debug, Clone)]
struct Entry {
    tag: u16,
    typ: u16,
    count: u32,
    /// The 4-byte value/offset field.
    field: [u8; 4],
    /// Where an out-of-line value lives in the block.
    value: Option<Range<usize>>,
    /// A new value to place when the directory is written.
    new_value: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
struct Dir {
    /// Offset of the directory in the block (None for a new one).
    offset: Option<usize>,
    orig_count: usize,
    entries: Vec<Entry>,
    next: u32,
    dirty: bool,
    /// An entry was removed (feeds the mandatory-tags rule).
    lost_entries: bool,
    /// A tag was written here (disables the mandatory-tags rule).
    written: bool,
}

impl Dir {
    fn new() -> Self {
        Dir {
            offset: None,
            orig_count: 0,
            entries: Vec::new(),
            next: 0,
            dirty: true,
            lost_entries: false,
            written: false,
        }
    }

    fn table_len(n: usize) -> usize {
        2 + 12 * n + 4
    }
}

struct Block {
    buf: Vec<u8>,
    bo: ByteOrderMark,
    dirs: HashMap<Ifd, Dir>,
    /// Byte ranges that held removed data and must be zeroed.
    freed: Vec<Range<usize>>,
    ifd0_moved: bool,
}

impl Block {
    fn u16(&self, p: usize) -> u16 {
        let b = [self.buf[p], self.buf[p + 1]];
        match self.bo {
            ByteOrderMark::LittleEndian => u16::from_le_bytes(b),
            ByteOrderMark::BigEndian => u16::from_be_bytes(b),
        }
    }
    fn u32(&self, p: usize) -> u32 {
        let b = [
            self.buf[p],
            self.buf[p + 1],
            self.buf[p + 2],
            self.buf[p + 3],
        ];
        match self.bo {
            ByteOrderMark::LittleEndian => u32::from_le_bytes(b),
            ByteOrderMark::BigEndian => u32::from_be_bytes(b),
        }
    }
    fn p16(&self, v: u16) -> [u8; 2] {
        match self.bo {
            ByteOrderMark::LittleEndian => v.to_le_bytes(),
            ByteOrderMark::BigEndian => v.to_be_bytes(),
        }
    }
    fn p32(&self, v: u32) -> [u8; 4] {
        match self.bo {
            ByteOrderMark::LittleEndian => v.to_le_bytes(),
            ByteOrderMark::BigEndian => v.to_be_bytes(),
        }
    }
    fn field_u32(&self, f: [u8; 4]) -> u32 {
        match self.bo {
            ByteOrderMark::LittleEndian => u32::from_le_bytes(f),
            ByteOrderMark::BigEndian => u32::from_be_bytes(f),
        }
    }

    /// Read the directory at `off`; `None` when it does not fit the block.
    fn read_dir(&self, off: usize) -> Option<Dir> {
        if off < 8 || off.checked_add(2)? > self.buf.len() {
            return None;
        }
        let n = self.u16(off) as usize;
        if off + Dir::table_len(n) > self.buf.len() {
            return None;
        }
        let mut entries = Vec::with_capacity(n);
        for i in 0..n {
            let e = off + 2 + 12 * i;
            let tag = self.u16(e);
            let typ = self.u16(e + 2);
            let count = self.u32(e + 4);
            let field = [
                self.buf[e + 8],
                self.buf[e + 9],
                self.buf[e + 10],
                self.buf[e + 11],
            ];
            let value = type_size(typ).and_then(|s| {
                let size = s.checked_mul(count as usize)?;
                if size <= 4 {
                    return None;
                }
                let start = self.field_u32(field) as usize;
                let end = start.checked_add(size)?;
                (end <= self.buf.len()).then_some(start..end)
            });
            entries.push(Entry {
                tag,
                typ,
                count,
                field,
                value,
                new_value: None,
            });
        }
        let next = self.u32(off + 2 + 12 * n);
        Some(Dir {
            offset: Some(off),
            orig_count: n,
            entries,
            next,
            dirty: false,
            lost_entries: false,
            written: false,
        })
    }

    fn pointer_value(&self, dir: &Dir, tag: u16) -> Option<usize> {
        dir.entries
            .iter()
            .find(|e| e.tag == tag && matches!(e.typ, 4 | 13) && e.count == 1)
            .map(|e| self.field_u32(e.field) as usize)
    }

    /// The data ranges an IFD1 points at (thumbnail, strips): freed with it.
    fn image_data_ranges(&self, dir: &Dir) -> Vec<Range<usize>> {
        let mut out = Vec::new();
        let values = |tag: u16| -> Vec<usize> {
            let Some(e) = dir.entries.iter().find(|e| e.tag == tag) else {
                return Vec::new();
            };
            let Some(sz) = type_size(e.typ).filter(|&s| s == 2 || s == 4) else {
                return Vec::new();
            };
            let (start, n) = match &e.value {
                Some(r) => (r.start, e.count as usize),
                None => (usize::MAX, e.count as usize),
            };
            (0..n.min(4096))
                .map(|i| {
                    if start == usize::MAX {
                        // Inline: up to 4 bytes in the field.
                        let mut f = [0u8; 4];
                        f.copy_from_slice(&e.field);
                        if sz == 2 {
                            let b = [f[i * 2], f[i * 2 + 1]];
                            (match self.bo {
                                ByteOrderMark::LittleEndian => u16::from_le_bytes(b),
                                ByteOrderMark::BigEndian => u16::from_be_bytes(b),
                            }) as usize
                        } else {
                            self.field_u32(f) as usize
                        }
                    } else if sz == 2 {
                        self.u16(start + i * 2) as usize
                    } else {
                        self.u32(start + i * 4) as usize
                    }
                })
                .collect()
        };
        for (off_tag, len_tag) in [(0x0201u16, 0x0202u16), (0x0111, 0x0117)] {
            let offs = values(off_tag);
            let lens = values(len_tag);
            for (o, l) in offs.iter().zip(lens.iter()) {
                if let Some(end) = o.checked_add(*l) {
                    if *o >= 8 && end <= self.buf.len() {
                        out.push(*o..end);
                    }
                }
            }
        }
        out
    }

    /// Append `data` at the end of the block (word-aligned) and return its
    /// offset.
    fn append(&mut self, data: &[u8]) -> usize {
        if self.buf.len() % 2 == 1 {
            self.buf.push(0);
        }
        let at = self.buf.len();
        self.buf.extend_from_slice(data);
        at
    }
}

/// The mandatory tags of a directory and their values, in the block's byte
/// order (WriteExif.pl `%mandatory`; XResolution and friends are optional
/// since EXIF 3.0 and so are not in IFD0's list).
fn mandatory(ifd: Ifd, bo: ByteOrderMark) -> Vec<(u16, u16, Vec<u8>)> {
    let s = |v: u16| match bo {
        ByteOrderMark::LittleEndian => v.to_le_bytes().to_vec(),
        ByteOrderMark::BigEndian => v.to_be_bytes().to_vec(),
    };
    let r = |n: u32, d: u32| {
        let (a, b) = match bo {
            ByteOrderMark::LittleEndian => (n.to_le_bytes(), d.to_le_bytes()),
            ByteOrderMark::BigEndian => (n.to_be_bytes(), d.to_be_bytes()),
        };
        [a, b].concat()
    };
    match ifd {
        Ifd::Ifd0 => vec![(0x0213, 3, s(1))],
        Ifd::Ifd1 => vec![
            (0x0103, 3, s(6)),
            (0x011a, 5, r(72, 1)),
            (0x011b, 5, r(72, 1)),
            (0x0128, 3, s(2)),
        ],
        Ifd::ExifIfd => vec![
            (0x9000, 7, b"0232".to_vec()),
            (0x9101, 7, vec![1, 2, 3, 0]),
            (0xa001, 3, s(0xffff)),
        ],
        Ifd::Gps => vec![(0x0000, 1, vec![2, 3, 0, 0])],
        Ifd::Interop => vec![(0x0002, 7, b"0100".to_vec())],
    }
}

/// ExifTool's name for tag `id` in directory `ifd`.
pub fn tag_name(ifd: Ifd, id: u16) -> Option<&'static str> {
    crate::tags::exif::lookup(ifd.name(), id)
        .map(|t| t.name)
        .or_else(|| crate::tags::exif::lookup_generated(id).map(|(n, _)| n))
}

/// Apply `changes` to the TIFF block `tiff` (`None` when the file has no
/// EXIF yet, in which case only writes can do anything).
pub fn edit_exif(tiff: Option<&[u8]>, changes: &ExifChanges) -> Result<ExifOutcome> {
    if changes.is_empty() {
        return Ok(ExifOutcome::Unchanged);
    }
    let deleting_everything = changes.delete_all || changes.delete_dirs.contains(&Ifd::Ifd0);
    let had_exif = tiff.is_some_and(|t| !t.is_empty());
    // Deleting the block and writing tags in the same request leaves only
    // the new tags (ExifTool deletes first, then writes).
    let tiff = if deleting_everything { None } else { tiff };
    let mut block = match tiff {
        Some(t) => open_block(t)?,
        None => {
            if changes.set.is_empty() {
                return Ok(if deleting_everything && had_exif {
                    ExifOutcome::Removed
                } else {
                    ExifOutcome::Unchanged
                });
            }
            // A new block is big-endian, as ExifTool writes it.
            Block {
                buf: b"MM\0\x2a\0\0\0\0".to_vec(),
                bo: ByteOrderMark::BigEndian,
                dirs: HashMap::new(),
                freed: Vec::new(),
                ifd0_moved: true,
            }
        }
    };
    let mut changed = false;

    // Whole directories.
    let mut doomed: Vec<Ifd> = changes.delete_dirs.clone();
    if doomed.contains(&Ifd::ExifIfd) {
        doomed.push(Ifd::Interop);
    }
    for ifd in doomed {
        if remove_dir(&mut block, ifd) {
            changed = true;
        }
    }

    // Maker notes and individual tags.
    let mut targets: Vec<(Option<Ifd>, String)> = changes.delete_tags.clone();
    if changes.delete_maker_notes {
        targets.push((Some(Ifd::ExifIfd), "makernote".into()));
    }
    if !targets.is_empty() {
        for ifd in Ifd::BOTTOM_UP {
            let Some(dir) = block.dirs.get_mut(&ifd) else {
                continue;
            };
            let before = dir.entries.len();
            let mut freed = Vec::new();
            dir.entries.retain(|e| {
                if POINTER_TAGS.contains(&e.tag) {
                    return true;
                }
                let name = match (ifd, e.tag) {
                    (Ifd::ExifIfd, 0x927c) => "makernote".to_string(),
                    _ => match tag_name(ifd, e.tag) {
                        Some(n) => n.to_ascii_lowercase(),
                        None => return true,
                    },
                };
                let hit = targets
                    .iter()
                    .any(|(g, n)| g.is_none_or(|g| g == ifd) && *n == name);
                if hit {
                    if let Some(r) = &e.value {
                        freed.push(r.clone());
                    }
                }
                !hit
            });
            if dir.entries.len() != before {
                dir.dirty = true;
                dir.lost_entries = true;
                changed = true;
                block.freed.extend(freed);
            }
        }
    }

    // Writes.
    for sv in &changes.set {
        if sv.ifd == Ifd::Ifd1 {
            // Creating or editing IFD1 means handling a thumbnail; not
            // supported, so the request is left unapplied.
            continue;
        }
        ensure_dir(&mut block, sv.ifd);
        let data = match block.bo {
            ByteOrderMark::BigEndian => sv.big_endian.clone(),
            ByteOrderMark::LittleEndian => sv.little_endian.clone(),
        };
        let Some(sz) = type_size(sv.format) else {
            continue;
        };
        let count = (data.len() / sz) as u32;
        let dir = block.dirs.get_mut(&sv.ifd).expect("directory just ensured");
        dir.written = true;
        dir.dirty = true;
        changed = true;
        let new = Entry {
            tag: sv.tag,
            typ: sv.format,
            count,
            field: [0; 4],
            value: None,
            new_value: Some(data),
        };
        match dir.entries.iter_mut().find(|e| e.tag == sv.tag) {
            Some(e) => {
                let old = e.value.clone();
                *e = Entry { value: old, ..new };
            }
            None => {
                let at = dir
                    .entries
                    .iter()
                    .position(|e| e.tag > sv.tag)
                    .unwrap_or(dir.entries.len());
                dir.entries.insert(at, new);
            }
        }
    }

    if !changed {
        return Ok(ExifOutcome::Unchanged);
    }

    // A directory that lost entries and now holds only mandatory tags goes
    // too, children first; IFD0 only goes when no IFD1 follows it.
    for ifd in Ifd::BOTTOM_UP {
        let Some(dir) = block.dirs.get(&ifd) else {
            continue;
        };
        let only_mandatory = {
            let mand = mandatory(ifd, block.bo);
            dir.entries.iter().all(|e| {
                let bytes = match (&e.new_value, &e.value) {
                    (Some(v), _) => v.clone(),
                    (None, Some(r)) => block.buf[r.clone()].to_vec(),
                    (None, None) => {
                        let n = type_size(e.typ).unwrap_or(4) * e.count as usize;
                        e.field[..n.min(4)].to_vec()
                    }
                };
                mand.iter()
                    .any(|(t, ty, v)| *t == e.tag && *ty == e.typ && *v == bytes)
            })
        };
        let empty = dir.entries.is_empty();
        let next_ifd = ifd == Ifd::Ifd0 && block.dirs.contains_key(&Ifd::Ifd1);
        let prune = (empty || (only_mandatory && dir.lost_entries && !dir.written)) && !next_ifd;
        if prune {
            remove_dir(&mut block, ifd);
        }
    }
    if !block.dirs.contains_key(&Ifd::Ifd0) {
        return Ok(if had_exif {
            ExifOutcome::Removed
        } else {
            ExifOutcome::Unchanged
        });
    }

    write_block(&mut block)?;
    Ok(ExifOutcome::Rewritten(block.buf))
}

fn open_block(tiff: &[u8]) -> Result<Block> {
    if tiff.len() < 8 {
        return Err(Error::InvalidExif("EXIF block too short".into()));
    }
    let bo = match &tiff[..4] {
        b"MM\0\x2a" => ByteOrderMark::BigEndian,
        b"II\x2a\0" => ByteOrderMark::LittleEndian,
        _ => return Err(Error::InvalidExif("not a TIFF header".into())),
    };
    let mut block = Block {
        buf: tiff.to_vec(),
        bo,
        dirs: HashMap::new(),
        freed: Vec::new(),
        ifd0_moved: false,
    };
    let ifd0_off = block.u32(4) as usize;
    let ifd0 = block
        .read_dir(ifd0_off)
        .ok_or_else(|| Error::InvalidExif("IFD0 is outside the EXIF block".into()))?;
    let mut seen = vec![ifd0_off];
    let mut add = |block: &mut Block, ifd: Ifd, off: usize| -> Result<()> {
        if seen.contains(&off) {
            return Err(Error::InvalidExif(format!("{} loops back", ifd.name())));
        }
        seen.push(off);
        match block.read_dir(off) {
            Some(d) => {
                block.dirs.insert(ifd, d);
                Ok(())
            }
            None => Err(Error::InvalidExif(format!(
                "{} is outside the EXIF block",
                ifd.name()
            ))),
        }
    };
    let ifd1_off = ifd0.next as usize;
    let exif_off = block.pointer_value(&ifd0, 0x8769);
    let gps_off = block.pointer_value(&ifd0, 0x8825);
    block.dirs.insert(Ifd::Ifd0, ifd0);
    if ifd1_off != 0 {
        add(&mut block, Ifd::Ifd1, ifd1_off)?;
    }
    if let Some(o) = exif_off {
        add(&mut block, Ifd::ExifIfd, o)?;
        let interop = block.pointer_value(&block.dirs[&Ifd::ExifIfd], 0xA005);
        if let Some(o) = interop {
            add(&mut block, Ifd::Interop, o)?;
        }
    }
    if let Some(o) = gps_off {
        add(&mut block, Ifd::Gps, o)?;
    }
    Ok(block)
}

/// Drop directory `ifd` (and the pointer to it). Returns whether it existed.
fn remove_dir(block: &mut Block, ifd: Ifd) -> bool {
    let Some(dir) = block.dirs.remove(&ifd) else {
        return false;
    };
    if let Some(off) = dir.offset {
        block.freed.push(off..off + Dir::table_len(dir.orig_count));
    }
    for e in &dir.entries {
        if let Some(r) = &e.value {
            block.freed.push(r.clone());
        }
    }
    if ifd == Ifd::Ifd1 {
        let ranges = block.image_data_ranges(&dir);
        block.freed.extend(ranges);
        if let Some(d0) = block.dirs.get_mut(&Ifd::Ifd0) {
            d0.next = 0;
            d0.dirty = true;
        }
    }
    if ifd == Ifd::ExifIfd {
        // Interop hangs off ExifIFD.
        remove_dir(block, Ifd::Interop);
    }
    if let Some((parent, tag)) = ifd.pointer() {
        if let Some(p) = block.dirs.get_mut(&parent) {
            p.entries.retain(|e| e.tag != tag);
            p.dirty = true;
            p.lost_entries = true;
        }
    }
    true
}

/// Make sure directory `ifd` exists, creating it (and its parents) with
/// their mandatory tags, as ExifTool does when it creates a directory.
fn ensure_dir(block: &mut Block, ifd: Ifd) {
    if block.dirs.contains_key(&ifd) {
        return;
    }
    if let Some((parent, tag)) = ifd.pointer() {
        ensure_dir(block, parent);
        let p = block.dirs.get_mut(&parent).expect("parent just ensured");
        let at = p
            .entries
            .iter()
            .position(|e| e.tag > tag)
            .unwrap_or(p.entries.len());
        p.entries.insert(
            at,
            Entry {
                tag,
                typ: 4,
                count: 1,
                field: [0; 4],
                value: None,
                new_value: None,
            },
        );
        p.dirty = true;
    }
    let mut dir = Dir::new();
    for (tag, typ, v) in mandatory(ifd, block.bo) {
        let n = (v.len() / type_size(typ).unwrap_or(1)) as u32;
        dir.entries.push(Entry {
            tag,
            typ,
            count: n,
            field: [0; 4],
            value: None,
            new_value: Some(v),
        });
    }
    if ifd == Ifd::Ifd0 {
        block.ifd0_moved = true;
    }
    block.dirs.insert(ifd, dir);
}

/// Lay the edited directories out in the block and zero what was freed.
fn write_block(block: &mut Block) -> Result<()> {
    let mut placed: HashMap<Ifd, usize> = HashMap::new();
    for ifd in Ifd::BOTTOM_UP {
        let Some(mut dir) = block.dirs.remove(&ifd) else {
            continue;
        };
        // A pointer to a child that moved makes this directory dirty.
        for child in [Ifd::ExifIfd, Ifd::Gps, Ifd::Interop] {
            if let Some((parent, tag)) = child.pointer() {
                if parent != ifd {
                    continue;
                }
                if let Some(&at) = placed.get(&child) {
                    if let Some(e) = dir.entries.iter_mut().find(|e| e.tag == tag) {
                        let f = block.p32(at as u32);
                        if e.field != f {
                            e.field = f;
                            e.typ = if e.typ == 13 { 13 } else { 4 };
                            e.count = 1;
                            dir.dirty = true;
                        }
                    }
                }
            }
        }
        if ifd == Ifd::Ifd0 {
            let next = placed
                .get(&Ifd::Ifd1)
                .map(|&o| o as u32)
                .unwrap_or(dir.next);
            if next != dir.next {
                dir.next = next;
                dir.dirty = true;
            }
        }
        if !dir.dirty {
            placed.insert(ifd, dir.offset.expect("untouched directory has an offset"));
            block.dirs.insert(ifd, dir);
            continue;
        }
        // Values first.
        for e in dir.entries.iter_mut() {
            let Some(v) = e.new_value.take() else {
                continue;
            };
            if v.len() <= 4 {
                let mut f = [0u8; 4];
                f[..v.len()].copy_from_slice(&v);
                e.field = f;
                if let Some(r) = e.value.take() {
                    block.freed.push(r);
                }
            } else {
                let at = match e.value.take() {
                    Some(r) if v.len() <= r.len() => {
                        block.buf[r.start..r.start + v.len()].copy_from_slice(&v);
                        if v.len() < r.len() {
                            block.freed.push(r.start + v.len()..r.end);
                        }
                        r.start
                    }
                    Some(r) => {
                        block.freed.push(r);
                        block.append(&v)
                    }
                    None => block.append(&v),
                };
                e.value = Some(at..at + v.len());
                e.field = block.p32(at as u32);
            }
        }
        // Then the table.
        let n = dir.entries.len();
        if n > 0xffff {
            return Err(Error::InvalidExif("too many entries in one IFD".into()));
        }
        let mut table = Vec::with_capacity(Dir::table_len(n));
        table.extend_from_slice(&block.p16(n as u16));
        for e in &dir.entries {
            table.extend_from_slice(&block.p16(e.tag));
            table.extend_from_slice(&block.p16(e.typ));
            table.extend_from_slice(&block.p32(e.count));
            table.extend_from_slice(&e.field);
        }
        table.extend_from_slice(&block.p32(dir.next));
        let at = match dir.offset {
            Some(off) if n <= dir.orig_count => {
                block.buf[off..off + table.len()].copy_from_slice(&table);
                let old_end = off + Dir::table_len(dir.orig_count);
                if off + table.len() < old_end {
                    block.freed.push(off + table.len()..old_end);
                }
                off
            }
            Some(off) => {
                block.freed.push(off..off + Dir::table_len(dir.orig_count));
                block.append(&table)
            }
            None => block.append(&table),
        };
        if ifd == Ifd::Ifd0 && dir.offset != Some(at) {
            block.ifd0_moved = true;
        }
        dir.offset = Some(at);
        dir.orig_count = n;
        dir.dirty = false;
        placed.insert(ifd, at);
        block.dirs.insert(ifd, dir);
    }
    if block.ifd0_moved {
        let at = placed[&Ifd::Ifd0] as u32;
        let f = block.p32(at);
        block.buf[4..8].copy_from_slice(&f);
    }
    // A pointer field of a directory written above may still point at a
    // child placed later in the same pass (IFD0 is last, so this only
    // concerns nothing today); the bottom-up order guarantees it.
    scrub(block);
    Ok(())
}

/// Zero every freed byte that no live structure still uses.
fn scrub(block: &mut Block) {
    let mut live: Vec<Range<usize>> = Vec::new();
    for dir in block.dirs.values() {
        if let Some(off) = dir.offset {
            live.push(off..off + Dir::table_len(dir.entries.len()));
        }
        for e in &dir.entries {
            if let Some(r) = &e.value {
                live.push(r.clone());
            }
        }
    }
    if let Some(d1) = block.dirs.get(&Ifd::Ifd1) {
        live.extend(block.image_data_ranges(d1));
    }
    live.push(0..8);
    for r in std::mem::take(&mut block.freed) {
        let end = r.end.min(block.buf.len());
        for p in r.start..end {
            if !live.iter().any(|l| l.contains(&p)) {
                block.buf[p] = 0;
            }
        }
    }
}

// ── Value encoding ──────────────────────────────────────────────────────────

/// Where a writable tag goes and how it is stored.
#[derive(Debug, Clone, Copy)]
pub struct TagSpec {
    pub ifd: Ifd,
    pub tag: u16,
    /// TIFF type.
    pub format: u16,
}

/// The tags this crate can write to EXIF, by (case-insensitive) name.
pub fn tag_spec(name: &str) -> Option<TagSpec> {
    let (ifd, tag, format) = match name.to_ascii_lowercase().as_str() {
        "imagedescription" => (Ifd::Ifd0, 0x010E, 2),
        "make" => (Ifd::Ifd0, 0x010F, 2),
        "model" => (Ifd::Ifd0, 0x0110, 2),
        "orientation" => (Ifd::Ifd0, 0x0112, 3),
        "xresolution" => (Ifd::Ifd0, 0x011A, 5),
        "yresolution" => (Ifd::Ifd0, 0x011B, 5),
        "resolutionunit" => (Ifd::Ifd0, 0x0128, 3),
        "software" => (Ifd::Ifd0, 0x0131, 2),
        "modifydate" | "datetime" => (Ifd::Ifd0, 0x0132, 2),
        "artist" => (Ifd::Ifd0, 0x013B, 2),
        "copyright" => (Ifd::Ifd0, 0x8298, 2),
        "datetimeoriginal" => (Ifd::ExifIfd, 0x9003, 2),
        "createdate" | "datetimedigitized" => (Ifd::ExifIfd, 0x9004, 2),
        "usercomment" => (Ifd::ExifIfd, 0x9286, 7),
        "imageuniqueid" => (Ifd::ExifIfd, 0xA420, 2),
        "ownername" | "cameraownername" => (Ifd::ExifIfd, 0xA430, 2),
        "serialnumber" | "bodyserialnumber" => (Ifd::ExifIfd, 0xA431, 2),
        "lensmake" => (Ifd::ExifIfd, 0xA433, 2),
        "lensmodel" => (Ifd::ExifIfd, 0xA434, 2),
        "lensserialnumber" => (Ifd::ExifIfd, 0xA435, 2),
        "gpsversionid" => (Ifd::Gps, 0x0000, 1),
        "gpslatituderef" => (Ifd::Gps, 0x0001, 2),
        "gpslatitude" => (Ifd::Gps, 0x0002, 5),
        "gpslongituderef" => (Ifd::Gps, 0x0003, 2),
        "gpslongitude" => (Ifd::Gps, 0x0004, 5),
        "gpsaltituderef" => (Ifd::Gps, 0x0005, 1),
        "gpsaltitude" => (Ifd::Gps, 0x0006, 5),
        _ => return None,
    };
    Some(TagSpec { ifd, tag, format })
}

/// Encode `value` for `spec` in byte order `bo`, applying the inverse
/// conversions ExifTool applies to the same input. `None` when the value is
/// not valid for the tag.
pub fn encode_value(spec: &TagSpec, value: &str, bo: ByteOrderMark) -> Option<Vec<u8>> {
    let u16b = |v: u16| match bo {
        ByteOrderMark::LittleEndian => v.to_le_bytes().to_vec(),
        ByteOrderMark::BigEndian => v.to_be_bytes().to_vec(),
    };
    let u32b = |v: u32| match bo {
        ByteOrderMark::LittleEndian => v.to_le_bytes(),
        ByteOrderMark::BigEndian => v.to_be_bytes(),
    };
    let rat = |v: f64| -> Vec<u8> {
        let (n, d) = rationalize_unsigned(v);
        [u32b(n), u32b(d)].concat()
    };
    let v = value.trim();
    match (spec.ifd, spec.tag) {
        // GPSVersionID: "2.3.0.0" or "2 3 0 0".
        (Ifd::Gps, 0x0000) => {
            let parts: Vec<u8> = v
                .split(|c: char| c == '.' || c.is_whitespace())
                .filter(|s| !s.is_empty())
                .map(|s| s.parse().ok())
                .collect::<Option<_>>()?;
            (parts.len() == 4).then_some(parts)
        }
        // GPSLatitudeRef / GPSLongitudeRef (GPS.pm %printConvLatRef/LonRef).
        (Ifd::Gps, 0x0001) => coordinate_ref(v, 'N', 'S').map(|c| vec![c as u8, 0]),
        (Ifd::Gps, 0x0003) => coordinate_ref(v, 'E', 'W').map(|c| vec![c as u8, 0]),
        // GPSLatitude / GPSLongitude: ToDegrees, then ToDMS, then rationals.
        (Ifd::Gps, 0x0002) | (Ifd::Gps, 0x0004) => {
            let deg = to_degrees(v)?.abs();
            let d = deg.trunc();
            let m = ((deg - d) * 60.0).trunc();
            let mut s = (deg - d - m / 60.0) * 3600.0;
            // ExifTool stringifies the seconds (15 significant digits)
            // before rationalizing them.
            s = format!("{:.*e}", 14, s).parse().unwrap_or(s);
            let (mut d, mut m) = (d, m);
            if s >= 60.0 {
                s -= 60.0;
                m += 1.0;
                if m >= 60.0 {
                    m -= 60.0;
                    d += 1.0;
                }
            }
            Some([rat(d), rat(m), rat(s)].concat())
        }
        // GPSAltitudeRef: 0/1, "Above/Below Sea Level", or a signed number.
        (Ifd::Gps, 0x0005) => {
            let l = v.to_ascii_lowercase();
            if l == "above sea level" {
                Some(vec![0])
            } else if l == "below sea level" || l.starts_with('-') {
                Some(vec![1])
            } else if l.starts_with(|c: char| c.is_ascii_digit() || c == '+') {
                let n: f64 = l.trim_start_matches('+').parse().ok()?;
                Some(vec![if n == 1.0 { 1 } else { 0 }])
            } else {
                None
            }
        }
        // GPSAltitude: an unsigned number, "m" suffix allowed.
        (Ifd::Gps, 0x0006) => {
            let n: f64 = v.trim_end_matches('m').trim().parse().ok()?;
            Some(rat(n.abs()))
        }
        _ => match spec.format {
            2 => {
                let mut d = v.as_bytes().to_vec();
                d.push(0);
                Some(d)
            }
            3 => {
                let n: u16 = v.parse().ok()?;
                Some(u16b(n))
            }
            5 => {
                if let Some((a, b)) = v.split_once('/') {
                    let n: u32 = a.trim().parse().ok()?;
                    let d: u32 = b.trim().parse().ok()?;
                    Some([u32b(n), u32b(d)].concat())
                } else {
                    let f: f64 = v.parse().ok()?;
                    (f >= 0.0).then(|| rat(f))
                }
            }
            7 => {
                // UserComment: character code + text.
                let mut d = b"ASCII\0\0\0".to_vec();
                d.extend_from_slice(v.as_bytes());
                Some(d)
            }
            _ => None,
        },
    }
}

/// The cardinal letter a coordinate reference designates (`pos` for a
/// positive number, `neg` for a negative one).
fn coordinate_ref(v: &str, pos: char, neg: char) -> Option<char> {
    let l = v.to_ascii_uppercase();
    let full = |c: char| match c {
        'N' => "NORTH",
        'S' => "SOUTH",
        'E' => "EAST",
        _ => "WEST",
    };
    for c in [pos, neg] {
        if l == c.to_string() || l == full(c) {
            return Some(c);
        }
    }
    // "... N" at the end of a formatted coordinate.
    if let Some(last) = l.split_whitespace().last() {
        for c in [pos, neg] {
            if last == c.to_string() || last == full(c) {
                return Some(c);
            }
        }
    }
    let first = l.chars().next()?;
    if first == '-' {
        Some(neg)
    } else if first == '+' || first.is_ascii_digit() {
        Some(pos)
    } else {
        None
    }
}

/// GPS.pm `ToDegrees`: up to three numbers (degrees, minutes, seconds) out
/// of any surrounding text.
fn to_degrees(v: &str) -> Option<f64> {
    let mut nums = Vec::new();
    let b = v.as_bytes();
    let mut i = 0;
    while i < b.len() && nums.len() < 3 {
        let start = i;
        let mut j = i;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        let digits_start = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j < b.len() && b[j] == b'.' {
            j += 1;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
        }
        let has_digit = b[digits_start..j].iter().any(|c| c.is_ascii_digit());
        if has_digit {
            nums.push(v[start..j].parse::<f64>().ok()?);
            i = j;
        } else {
            i = start + 1;
        }
    }
    let d = *nums.first()?;
    let m = nums.get(1).copied().unwrap_or(0.0);
    let s = nums.get(2).copied().unwrap_or(0.0);
    Some(d + (m + s / 60.0) / 60.0)
}

/// Writer.pl `Rationalize` with the unsigned maximum used for rational64u.
pub fn rationalize_unsigned(val: f64) -> (u32, u32) {
    if val == 0.0 || !val.is_finite() {
        return (0, 1);
    }
    let val = val.abs();
    let max = u32::MAX as f64;
    let mut fracs: Vec<f64> = Vec::new();
    let mut frac = val;
    let mut best: Option<(f64, f64)> = None;
    for _ in 0..64 {
        // AssembleRational(int(frac + 0.5), 1, @fracs)
        let (mut n, mut d) = ((frac + 0.5).trunc(), 1.0);
        for &f in &fracs {
            let nn = f * n + d;
            d = n;
            n = nn;
        }
        if n > max || d > max {
            if best.is_some() {
                break;
            }
            return if val < 1.0 {
                (1, u32::MAX)
            } else {
                (u32::MAX, 1)
            };
        }
        best = Some((n, d));
        let err = (n / d - val) / val;
        if err.abs() < 1e-8 {
            break;
        }
        let int = frac.trunc();
        fracs.insert(0, int);
        frac -= int;
        if frac == 0.0 {
            break;
        }
        frac = 1.0 / frac;
    }
    let (n, d) = best.unwrap_or((0.0, 1.0));
    (n as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(name: &str, value: &str) -> SetValue {
        let spec = tag_spec(name).unwrap();
        SetValue {
            ifd: spec.ifd,
            tag: spec.tag,
            format: spec.format,
            big_endian: encode_value(&spec, value, ByteOrderMark::BigEndian).unwrap(),
            little_endian: encode_value(&spec, value, ByteOrderMark::LittleEndian).unwrap(),
        }
    }

    fn read(tiff: &[u8]) -> Vec<(String, String, String)> {
        crate::metadata::ExifReader::read(tiff)
            .unwrap()
            .into_iter()
            .filter(|t| t.name != "ExifByteOrder")
            .map(|t| (t.group.family1, t.name, t.print_value))
            .collect()
    }

    fn sample(bo: ByteOrderMark) -> Vec<u8> {
        let changes = ExifChanges {
            set: vec![
                set("Make", "Maker"),
                set("Model", "A rather long model name"),
                set("Artist", "Somebody"),
                set("DateTimeOriginal", "2024:01:02 03:04:05"),
                set("GPSLatitudeRef", "N"),
                set("GPSLatitude", "48.8584"),
            ],
            ..Default::default()
        };
        let mut start = match bo {
            ByteOrderMark::BigEndian => b"MM\0\x2a\0\0\0\x08".to_vec(),
            ByteOrderMark::LittleEndian => b"II\x2a\0\x08\0\0\0".to_vec(),
        };
        // An IFD0 with a single YCbCrPositioning entry.
        let p16 = |v: u16| match bo {
            ByteOrderMark::BigEndian => v.to_be_bytes(),
            ByteOrderMark::LittleEndian => v.to_le_bytes(),
        };
        let p32 = |v: u32| match bo {
            ByteOrderMark::BigEndian => v.to_be_bytes(),
            ByteOrderMark::LittleEndian => v.to_le_bytes(),
        };
        start.extend_from_slice(&p16(1));
        start.extend_from_slice(&p16(0x0213));
        start.extend_from_slice(&p16(3));
        start.extend_from_slice(&p32(1));
        start.extend_from_slice(&p16(1));
        start.extend_from_slice(&[0, 0]);
        start.extend_from_slice(&p32(0));
        match edit_exif(Some(&start), &changes).unwrap() {
            ExifOutcome::Rewritten(b) => b,
            o => panic!("{o:?}"),
        }
    }

    #[test]
    fn writes_into_every_directory_in_both_byte_orders() {
        for bo in [ByteOrderMark::BigEndian, ByteOrderMark::LittleEndian] {
            let tags = read(&sample(bo));
            let has = |g: &str, n: &str, v: &str| {
                tags.iter()
                    .any(|(tg, tn, tv)| tg == g && tn == n && tv == v)
            };
            assert!(has("IFD0", "Make", "Maker"), "{tags:?}");
            assert!(has("IFD0", "Model", "A rather long model name"));
            assert!(has("ExifIFD", "DateTimeOriginal", "2024:01:02 03:04:05"));
            // Creating ExifIFD adds its mandatory tags, as ExifTool does.
            assert!(has("ExifIFD", "ExifVersion", "0232"), "{tags:?}");
            assert!(has("GPS", "GPSVersionID", "2.3.0.0"));
            assert!(has("GPS", "GPSLatitude", "48 deg 51' 30.24\""), "{tags:?}");
            assert!(has("GPS", "GPSLatitudeRef", "North"));
        }
    }

    #[test]
    fn deleting_the_last_real_tags_drops_mandatory_only_directories() {
        let tiff = sample(ByteOrderMark::BigEndian);
        let changes = ExifChanges {
            delete_tags: ["make", "model", "artist", "datetimeoriginal", "gpslatitude"]
                .iter()
                .map(|n| (None, n.to_string()))
                .chain(std::iter::once((Some(Ifd::Gps), "gpslatituderef".into())))
                .collect(),
            ..Default::default()
        };
        assert_eq!(
            edit_exif(Some(&tiff), &changes).unwrap(),
            ExifOutcome::Removed
        );
    }

    #[test]
    fn deleting_a_directory_keeps_the_rest_and_zeroes_the_data() {
        let tiff = sample(ByteOrderMark::LittleEndian);
        let changes = ExifChanges {
            delete_dirs: vec![Ifd::Gps],
            ..Default::default()
        };
        let ExifOutcome::Rewritten(out) = edit_exif(Some(&tiff), &changes).unwrap() else {
            panic!()
        };
        let tags = read(&out);
        assert!(tags.iter().all(|(g, _, _)| g != "GPS"), "{tags:?}");
        assert!(tags.iter().any(|(_, n, v)| n == "Make" && v == "Maker"));
        // The GPS rationals are gone from the bytes, not just unlinked.
        let lat = encode_value(
            &tag_spec("GPSLatitude").unwrap(),
            "48.8584",
            ByteOrderMark::LittleEndian,
        )
        .unwrap();
        assert!(!out.windows(lat.len()).any(|w| w == lat.as_slice()));
    }

    #[test]
    fn group_and_tag_deletions_leave_unrelated_tags_alone() {
        let tiff = sample(ByteOrderMark::BigEndian);
        let changes = ExifChanges {
            delete_tags: vec![(Some(Ifd::Ifd0), "model".into())],
            ..Default::default()
        };
        let ExifOutcome::Rewritten(out) = edit_exif(Some(&tiff), &changes).unwrap() else {
            panic!()
        };
        let tags = read(&out);
        assert!(!tags.iter().any(|(_, n, _)| n == "Model"));
        assert!(!out.windows(8).any(|w| w == b"A rather"));
        assert_eq!(tags.len(), read(&tiff).len() - 1);
        // A tag qualified with another directory is not touched.
        let changes = ExifChanges {
            delete_tags: vec![(Some(Ifd::Gps), "make".into())],
            ..Default::default()
        };
        assert_eq!(
            edit_exif(Some(&tiff), &changes).unwrap(),
            ExifOutcome::Unchanged
        );
    }

    #[test]
    fn rewriting_a_longer_value_moves_it_and_a_shorter_one_stays() {
        let tiff = sample(ByteOrderMark::BigEndian);
        for v in ["X", "A much, much longer model name than before"] {
            let changes = ExifChanges {
                set: vec![set("Model", v)],
                ..Default::default()
            };
            let ExifOutcome::Rewritten(out) = edit_exif(Some(&tiff), &changes).unwrap() else {
                panic!()
            };
            let tags = read(&out);
            assert!(tags.iter().any(|(_, n, tv)| n == "Model" && tv == v));
            assert_eq!(tags.len(), read(&tiff).len());
        }
    }

    #[test]
    fn rationalize_matches_exiftool() {
        assert_eq!(rationalize_unsigned(72.0), (72, 1));
        assert_eq!(rationalize_unsigned(0.5), (1, 2));
        assert_eq!(rationalize_unsigned(30.24), (756, 25));
        assert_eq!(rationalize_unsigned(1.2345678901), (100, 81));
    }

    #[test]
    fn coordinate_inputs() {
        assert_eq!(
            to_degrees("51 30 2.52"),
            Some(51.0 + 30.0 / 60.0 + 2.52 / 3600.0)
        );
        assert_eq!(to_degrees("51 deg 30' 0.00\" N"), Some(51.5));
        assert_eq!(coordinate_ref("North", 'N', 'S'), Some('N'));
        assert_eq!(coordinate_ref("-12.5", 'N', 'S'), Some('S'));
        assert_eq!(coordinate_ref("0 deg 7' 28.56\" W", 'E', 'W'), Some('W'));
        assert_eq!(coordinate_ref("nowhere", 'E', 'W'), None);
    }

    #[test]
    fn malformed_blocks_are_refused() {
        assert!(edit_exif(
            Some(b"MM\0\x2a\0\0\0\x40"),
            &ExifChanges {
                delete_all: false,
                delete_tags: vec![(None, "make".into())],
                ..Default::default()
            }
        )
        .is_err());
    }
}
