//! Matroska/WebM metadata writer.
//!
//! ExifTool reads Matroska but does not write it. This writer edits the
//! two places a segment keeps its descriptive metadata:
//!
//! - **Info/Title** (`Matroska:Title`): replaced, added or removed in the
//!   segment `Info` element.
//! - **Tags/Tag/SimpleTag** (any other name): a SimpleTag whose `TagName`
//!   is the tag (case aside) gets the new `TagString`, or is removed; a tag
//!   that is not there is added as a SimpleTag of a new whole-file `Tag`
//!   (no `Targets`, which means TargetTypeValue 50). A `Tags` element is
//!   created before the first Cluster when the segment has none.
//!
//! Every other element, and the media in the Clusters, is copied byte for
//! byte. Like `mkvpropedit`, the writer keeps the positions the file
//! refers to: an element that shrinks is followed by an `EBMLVoid` (or
//! re-encodes its size one byte wider), one that grows first takes the
//! space of an `EBMLVoid` that follows it. Only when that is not enough do
//! the later elements move, and then every position that points past the
//! change is relocated: `SeekPosition` in the SeekHead, `CueClusterPosition`
//! and `CueCodecState` in the Cues, `Position` in each Cluster, and the
//! Segment size. A `CRC-32` element of a rewritten or patched master is
//! recomputed. A position that no longer fits its field, or an unknown-size
//! element ahead of the Tags to edit, is an error.

use crate::api::error::{Error, Result};
use std::ops::Range;

const ID_EBML: u32 = 0x1A45_DFA3;
const ID_SEGMENT: u32 = 0x1853_8067;
const ID_SEEKHEAD: u32 = 0x114D_9B74;
const ID_SEEK: u32 = 0x4DBB;
const ID_SEEKPOSITION: u32 = 0x53AC;
const ID_INFO: u32 = 0x1549_A966;
const ID_TITLE: u32 = 0x7BA9;
const ID_TAGS: u32 = 0x1254_C367;
const ID_TAG: u32 = 0x7373;
const ID_SIMPLETAG: u32 = 0x67C8;
const ID_TAGNAME: u32 = 0x45A3;
const ID_TAGSTRING: u32 = 0x4487;
const ID_CLUSTER: u32 = 0x1F43_B675;
const ID_CLUSTER_POSITION: u32 = 0xA7;
const ID_CUES: u32 = 0x1C53_BB6B;
const ID_CUEPOINT: u32 = 0xBB;
const ID_CUETRACKPOSITIONS: u32 = 0xB7;
const ID_CUECLUSTERPOSITION: u32 = 0xF1;
const ID_CUECODECSTATE: u32 = 0xEA;
const ID_VOID: u32 = 0xEC;
const ID_CRC32: u32 = 0xBF;

/// The Matroska part of a write request.
#[derive(Debug, Clone, Default)]
pub struct MkvChanges {
    /// (tag name, value).
    pub set: Vec<(String, String)>,
    /// Lower-case tag names.
    pub delete: Vec<String>,
    /// `Matroska:all`: the Title and every SimpleTag.
    pub delete_all: bool,
}

impl MkvChanges {
    pub fn is_empty(&self) -> bool {
        self.set.is_empty() && self.delete.is_empty() && !self.delete_all
    }

    fn deletes(&self, name: &str) -> bool {
        self.delete_all || self.delete.iter().any(|d| d.eq_ignore_ascii_case(name))
    }

    fn value(&self, name: &str) -> Option<&str> {
        self.set
            .iter()
            .rev()
            .find(|(t, _)| t.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    fn is_title(name: &str) -> bool {
        name.eq_ignore_ascii_case("Title")
    }

    /// Requests for SimpleTags (everything but Title).
    fn touches_tags(&self) -> bool {
        self.delete_all
            || self.delete.iter().any(|d| !Self::is_title(d))
            || self.set.iter().any(|(t, _)| !Self::is_title(t))
    }

    fn touches_title(&self) -> bool {
        self.deletes("Title") || self.value("Title").is_some()
    }
}

fn err(msg: impl Into<String>) -> Error {
    Error::InvalidData(format!("Matroska: {}", msg.into()))
}

// ── EBML primitives ─────────────────────────────────────────────────────────

/// One element: ID, header length, payload range (`unknown`: size field
/// says "unknown", payload runs to the end of its parent).
#[derive(Debug, Clone)]
struct Elem {
    id: u32,
    start: usize,
    /// Width of the size field.
    size_len: usize,
    payload: Range<usize>,
    unknown: bool,
}

impl Elem {
    fn end(&self) -> usize {
        self.payload.end
    }
}

fn read_id(d: &[u8], pos: usize) -> Option<(u32, usize)> {
    let first = *d.get(pos)?;
    if first == 0 {
        return None;
    }
    let len = first.leading_zeros() as usize + 1;
    if len > 4 || pos + len > d.len() {
        return None;
    }
    let mut v = first as u32;
    for i in 1..len {
        v = (v << 8) | d[pos + i] as u32;
    }
    Some((v, len))
}

/// A size field: (value, width, unknown).
fn read_size(d: &[u8], pos: usize) -> Option<(u64, usize, bool)> {
    let first = *d.get(pos)?;
    if first == 0 {
        return None;
    }
    let len = first.leading_zeros() as usize + 1;
    if pos + len > d.len() {
        return None;
    }
    let mut v = (first as u64) & ((1u64 << (8 - len)) - 1);
    for i in 1..len {
        v = (v << 8) | d[pos + i] as u64;
    }
    let unknown = v == (1u64 << (7 * len)) - 1;
    Some((v, len, unknown))
}

fn read_elem(d: &[u8], pos: usize, parent_end: usize) -> Result<Elem> {
    let (id, il) = read_id(d, pos).ok_or_else(|| err(format!("bad element ID at {pos}")))?;
    let (size, sl, unknown) =
        read_size(d, pos + il).ok_or_else(|| err(format!("bad element size at {pos}")))?;
    let start = pos + il + sl;
    let end = if unknown {
        parent_end
    } else {
        usize::try_from(size)
            .ok()
            .and_then(|s| start.checked_add(s))
            .filter(|&e| e <= parent_end)
            .ok_or_else(|| err(format!("element {id:X} at {pos} runs past its parent")))?
    };
    Ok(Elem {
        id,
        start: pos,
        size_len: sl,
        payload: start..end,
        unknown,
    })
}

fn children(d: &[u8], range: Range<usize>) -> Result<Vec<Elem>> {
    let mut out = Vec::new();
    let mut pos = range.start;
    while pos < range.end {
        let e = read_elem(d, pos, range.end)?;
        pos = e.end();
        let unknown = e.unknown;
        out.push(e);
        if unknown {
            break;
        }
    }
    Ok(out)
}

fn id_bytes(id: u32) -> Vec<u8> {
    let b = id.to_be_bytes();
    let skip = b.iter().position(|&x| x != 0).unwrap_or(3);
    b[skip..].to_vec()
}

/// A size field of at least `min` bytes.
fn size_bytes(n: usize, min: usize) -> Result<Vec<u8>> {
    let mut w = min.max(1);
    while w <= 8 && (n as u64) >= (1u64 << (7 * w)) - 1 {
        w += 1;
    }
    if w > 8 {
        return Err(err("element too large"));
    }
    let mut v = (n as u64) | (1u64 << (7 * w));
    let mut out = vec![0u8; w];
    for i in (0..w).rev() {
        out[i] = v as u8;
        v >>= 8;
    }
    Ok(out)
}

fn element(id: u32, payload: &[u8], min_size_len: usize) -> Result<Vec<u8>> {
    let mut out = id_bytes(id);
    out.extend(size_bytes(payload.len(), min_size_len)?);
    out.extend_from_slice(payload);
    Ok(out)
}

/// An `EBMLVoid` of exactly `total` bytes (at least 2).
fn void(total: usize) -> Option<Vec<u8>> {
    (1..=8).find_map(|w| {
        let n = total.checked_sub(1 + w)?;
        if (n as u64) < (1u64 << (7 * w)) - 1 {
            let mut out = vec![0xEC];
            out.extend(size_bytes(n, w).ok()?);
            out.resize(total, 0);
            Some(out)
        } else {
            None
        }
    })
}

/// IEEE CRC-32 (the EBML `CRC-32` element; stored little-endian).
fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let t = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, e) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            *e = c;
        }
        t
    });
    !data.iter().fold(!0u32, |c, &b| {
        t[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8)
    })
}

/// Recompute the `CRC-32` child of a master payload, in place.
fn fix_crc(buf: &mut [u8], payload: Range<usize>) -> Result<()> {
    let Ok(kids) = children(buf, payload.clone()) else {
        return Ok(());
    };
    if let Some(c) = kids
        .first()
        .filter(|c| c.id == ID_CRC32 && c.payload.len() == 4)
    {
        let crc = crc32(&buf[c.end()..payload.end]);
        let p = c.payload.start;
        buf[p..p + 4].copy_from_slice(&crc.to_le_bytes());
    }
    Ok(())
}

/// A master's payload with a `CRC-32` first child refreshed.
fn with_crc(payload: Vec<u8>) -> Vec<u8> {
    let mut p = payload;
    let n = p.len();
    let _ = fix_crc(&mut p, 0..n);
    p
}

fn uint_bytes(v: u64, width: usize) -> Option<Vec<u8>> {
    if width < 8 && v >= 1u64 << (8 * width) {
        return None;
    }
    Some(v.to_be_bytes()[8 - width..].to_vec())
}

fn read_uint(d: &[u8]) -> u64 {
    d.iter().fold(0u64, |v, &b| (v << 8) | b as u64)
}

// ── edits ───────────────────────────────────────────────────────────────────

fn simple_tag(name: &str, value: &str) -> Result<Vec<u8>> {
    let mut p = element(ID_TAGNAME, name.as_bytes(), 1)?;
    // (no TagLanguage: its default, "und", is what an absent one means;
    // ExifTool names a tag with one by its language, "Artist-und")
    p.extend(element(ID_TAGSTRING, value.as_bytes(), 1)?);
    element(ID_SIMPLETAG, &p, 1)
}

/// The SimpleTag field name for a tag (`TagName`s are upper case).
fn tag_name_for(tag: &str) -> String {
    let mut out = String::new();
    for (i, c) in tag.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_uppercase());
    }
    out
}

/// Does a SimpleTag `TagName` name this tag?
fn names_tag(tag_name: &str, tag: &str) -> bool {
    tag_name.eq_ignore_ascii_case(tag) || tag_name.eq_ignore_ascii_case(&tag_name_for(tag))
}

fn edit_info(d: &[u8], info: &Elem, ch: &MkvChanges) -> Result<Vec<u8>> {
    let mut p = Vec::new();
    let mut done = false;
    for c in children(d, info.payload.clone())? {
        if c.id == ID_TITLE {
            if ch.deletes("Title") || done {
                continue;
            }
            if let Some(v) = ch.value("Title") {
                done = true;
                p.extend(element(ID_TITLE, v.as_bytes(), 1)?);
                continue;
            }
        }
        p.extend_from_slice(&d[c.start..c.end()]);
    }
    if !done {
        if let Some(v) = ch.value("Title") {
            p.extend(element(ID_TITLE, v.as_bytes(), 1)?);
        }
    }
    element(ID_INFO, &with_crc(p), info.size_len)
}

/// Edit one `Tags` element; `None` when nothing is left in it. Marks the
/// tags it wrote in `written`.
fn edit_tags(
    d: &[u8],
    tags: &Elem,
    ch: &MkvChanges,
    written: &mut Vec<String>,
) -> Result<Option<Vec<u8>>> {
    let mut tp = Vec::new();
    let mut any_tag = false;
    for t in children(d, tags.payload.clone())? {
        if t.id != ID_TAG {
            tp.extend_from_slice(&d[t.start..t.end()]);
            continue;
        }
        let mut p = Vec::new();
        let mut simple = 0;
        for s in children(d, t.payload.clone())? {
            if s.id != ID_SIMPLETAG {
                p.extend_from_slice(&d[s.start..s.end()]);
                continue;
            }
            let fields = children(d, s.payload.clone())?;
            let name = fields
                .iter()
                .find(|f| f.id == ID_TAGNAME)
                .map(|f| String::from_utf8_lossy(&d[f.payload.clone()]).into_owned())
                .unwrap_or_default();
            let target = ch
                .set
                .iter()
                .rev()
                .find(|(tag, _)| !MkvChanges::is_title(tag) && names_tag(&name, tag));
            let deleted = ch.delete_all
                || ch
                    .delete
                    .iter()
                    .any(|t| !MkvChanges::is_title(t) && names_tag(&name, t));
            if deleted && target.is_none() {
                continue;
            }
            match target {
                Some((tag, v)) => {
                    let lc = tag.to_ascii_lowercase();
                    if written.contains(&lc) {
                        continue;
                    }
                    written.push(lc);
                    let mut sp = Vec::new();
                    let mut has_string = false;
                    for f in &fields {
                        match f.id {
                            ID_TAGSTRING => {
                                has_string = true;
                                sp.extend(element(ID_TAGSTRING, v.as_bytes(), 1)?);
                            }
                            // A binary value is replaced by the string.
                            0x4485 => {}
                            _ => sp.extend_from_slice(&d[f.start..f.end()]),
                        }
                    }
                    if !has_string {
                        sp.extend(element(ID_TAGSTRING, v.as_bytes(), 1)?);
                    }
                    p.extend(element(ID_SIMPLETAG, &sp, 1)?);
                }
                None => p.extend_from_slice(&d[s.start..s.end()]),
            }
            simple += 1;
        }
        if simple > 0 {
            any_tag = true;
            tp.extend(element(ID_TAG, &with_crc(p), t.size_len)?);
        }
    }
    if !any_tag {
        return Ok(None);
    }
    Ok(Some(element(ID_TAGS, &with_crc(tp), tags.size_len)?))
}

/// A `Tag` holding the SimpleTags still to be created.
fn new_tag(ch: &MkvChanges, written: &mut Vec<String>) -> Result<Option<Vec<u8>>> {
    let mut p = Vec::new();
    for (tag, _) in &ch.set {
        let lc = tag.to_ascii_lowercase();
        if MkvChanges::is_title(tag) || written.contains(&lc) {
            continue;
        }
        written.push(lc);
        let v = ch.value(tag).unwrap_or_default();
        p.extend(simple_tag(&tag_name_for(tag), v)?);
    }
    if p.is_empty() {
        return Ok(None);
    }
    Ok(Some(element(ID_TAG, &p, 1)?))
}

/// What becomes of one segment child.
enum Piece {
    Copy,
    Drop,
    Replace(Vec<u8>),
}

/// Rewrite a Matroska file with `changes` applied.
pub fn write_matroska(source: &[u8], changes: &MkvChanges) -> Result<Vec<u8>> {
    if source.len() < 4 || !source.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return Err(Error::InvalidData("not a Matroska file".into()));
    }
    if changes.is_empty() {
        return Ok(source.to_vec());
    }
    let header = read_elem(source, 0, source.len())?;
    if header.id != ID_EBML || header.unknown {
        return Err(err("invalid EBML header"));
    }
    // The Segment; a truncated file (size past the end) is edited as far
    // as it goes.
    let (sid, il) = read_id(source, header.end()).ok_or_else(|| err("no Segment"))?;
    if sid != ID_SEGMENT {
        return Err(err("no Segment after the EBML header"));
    }
    let (seg_size, seg_sl, seg_unknown) =
        read_size(source, header.end() + il).ok_or_else(|| err("bad Segment size"))?;
    let seg_start = header.end() + il + seg_sl;
    let seg_end = if seg_unknown {
        source.len()
    } else {
        usize::try_from(seg_size)
            .ok()
            .and_then(|s| seg_start.checked_add(s))
            .map_or(source.len(), |e| e.min(source.len()))
    };
    let kids = children(source, seg_start..seg_end)?;
    let opaque_from = kids.iter().position(|k| k.unknown && k.id != ID_CLUSTER);
    if opaque_from.is_some() {
        return Err(err("unknown-size element in the Segment"));
    }

    // Plan each child.
    let mut pieces: Vec<Piece> = kids.iter().map(|_| Piece::Copy).collect();
    let mut written: Vec<String> = Vec::new();
    let mut have_info = false;
    let mut first_tags: Option<usize> = None;
    for (i, k) in kids.iter().enumerate() {
        match k.id {
            ID_INFO if !have_info => {
                have_info = true;
                if changes.touches_title() || changes.delete_all {
                    let new = edit_info(source, k, changes)?;
                    if new != source[k.start..k.end()] {
                        pieces[i] = Piece::Replace(new);
                    }
                }
            }
            ID_TAGS if changes.touches_tags() => {
                first_tags.get_or_insert(i);
                pieces[i] = match edit_tags(source, k, changes, &mut written)? {
                    Some(b) if b == source[k.start..k.end()] => Piece::Copy,
                    Some(b) => Piece::Replace(b),
                    None => Piece::Drop,
                };
            }
            _ => {}
        }
        if k.unknown && changes.touches_tags() && first_tags.is_none() {
            return Err(err("Tags after an unknown-size Cluster"));
        }
    }
    if changes.value("Title").is_some() && !have_info {
        return Err(err("no segment Info to hold the Title"));
    }
    // New SimpleTags: into the first Tags, or a new Tags before the first
    // Cluster (or at the end of the segment).
    let mut insert_tags: Option<(usize, Vec<u8>)> = None;
    if let Some(tag) = new_tag(changes, &mut written)? {
        match first_tags {
            Some(i) => {
                let k = &kids[i];
                let mut payload = match &pieces[i] {
                    Piece::Replace(b) => {
                        let e = read_elem(b, 0, b.len())?;
                        b[e.payload.clone()].to_vec()
                    }
                    Piece::Drop => Vec::new(),
                    Piece::Copy => source[k.payload.clone()].to_vec(),
                };
                payload.extend(tag);
                pieces[i] = Piece::Replace(element(ID_TAGS, &with_crc(payload), k.size_len)?);
            }
            None => {
                let at = kids
                    .iter()
                    .position(|k| k.id == ID_CLUSTER)
                    .unwrap_or(kids.len());
                insert_tags = Some((at, element(ID_TAGS, &tag, 1)?));
            }
        }
    }

    // Lay the segment out, keeping positions where the space allows.
    let mut body: Vec<u8> = Vec::with_capacity(source.len() - seg_start + 256);
    // (old range relative to the segment, new start relative to it)
    let mut runs: Vec<(Range<usize>, usize)> = Vec::new();
    let mut rewritten: Vec<(usize, usize)> = Vec::new(); // (old rel start, new rel start)
    let mut skip_void = false;
    for (i, k) in kids.iter().enumerate() {
        let (os, oe) = (k.start - seg_start, k.end() - seg_start);
        if let Some((at, t)) = &insert_tags {
            if *at == i {
                body.extend_from_slice(t);
            }
        }
        if skip_void {
            skip_void = false;
            if k.id == ID_VOID {
                continue;
            }
        }
        match &pieces[i] {
            Piece::Copy => {
                runs.push((os..oe, body.len()));
                body.extend_from_slice(&source[k.start..k.end()]);
            }
            Piece::Drop | Piece::Replace(_) => {
                let new: &[u8] = match &pieces[i] {
                    Piece::Replace(b) => b,
                    _ => &[],
                };
                rewritten.push((os, body.len()));
                // The room this element may use: its own, plus a Void that
                // follows it.
                let next_void = kids
                    .get(i + 1)
                    .filter(|n| {
                        n.id == ID_VOID && !matches!(insert_tags, Some((at, _)) if at == i + 1)
                    })
                    .map(|n| n.end() - n.start);
                let room = (oe - os) + next_void.unwrap_or(0);
                if new.len() <= room {
                    let pad = room - new.len();
                    if pad == 1 {
                        // One byte: widen the size field instead.
                        let e = read_elem(new, 0, new.len())?;
                        let widened = element(e.id, &new[e.payload.clone()], e.size_len + 1)?;
                        body.extend(widened);
                    } else {
                        body.extend_from_slice(new);
                        if pad >= 2 {
                            body.extend(void(pad).ok_or_else(|| err("can't pad"))?);
                        }
                    }
                    if next_void.is_some() {
                        skip_void = true;
                    }
                } else {
                    body.extend_from_slice(new);
                }
            }
        }
    }
    if let Some((at, t)) = &insert_tags {
        if *at == kids.len() {
            body.extend_from_slice(t);
        }
    }
    let old_len = seg_end - seg_start;
    let delta = body.len() as i64 - old_len as i64;
    let moved = delta != 0 || runs.iter().any(|(r, n)| r.start != *n) || insert_tags.is_some();

    if moved {
        // Positions are relative to the segment payload.
        let map = |p: u64| -> Option<u64> {
            let p_us = usize::try_from(p).ok()?;
            for (r, n) in &runs {
                if p_us >= r.start && p_us < r.end {
                    return Some((n + (p_us - r.start)) as u64);
                }
            }
            for (os, ns) in &rewritten {
                if p_us == *os {
                    return Some(*ns as u64);
                }
            }
            if p_us >= old_len {
                // Past what this (truncated) file holds: shift with the end.
                return u64::try_from(p as i64 + delta).ok();
            }
            None
        };
        let n = body.len();
        relocate(&mut body, 0..n, &map, 0)?;
    }

    let mut out = Vec::with_capacity(seg_start + body.len());
    out.extend_from_slice(&source[..header.end()]);
    out.extend(id_bytes(ID_SEGMENT));
    if seg_unknown {
        out.extend_from_slice(&source[header.end() + il..seg_start]);
    } else {
        let new_size =
            u64::try_from(seg_size as i64 + delta).map_err(|_| err("bad Segment size"))?;
        if new_size >= (1u64 << (7 * seg_sl)) - 1 {
            return Err(err("the Segment size no longer fits its field"));
        }
        let mut v = new_size | (1u64 << (7 * seg_sl));
        let mut f = vec![0u8; seg_sl];
        for i in (0..seg_sl).rev() {
            f[i] = v as u8;
            v >>= 8;
        }
        out.extend(f);
    }
    out.extend(body);
    // Whatever followed the segment (or the truncated rest) stays.
    if !seg_unknown && seg_end < source.len() {
        out.extend_from_slice(&source[seg_end..]);
    }
    Ok(out)
}

/// Patch the positions in the segment body (`buf`, relative offsets).
fn relocate(
    buf: &mut [u8],
    range: Range<usize>,
    map: &dyn Fn(u64) -> Option<u64>,
    depth: u32,
) -> Result<()> {
    if depth > 8 {
        return Ok(());
    }
    let kids = children(buf, range)?;
    for k in kids {
        let patch_here = match (depth, k.id) {
            (0, ID_SEEKHEAD) | (1, ID_SEEK) => {
                relocate(buf, k.payload.clone(), map, depth + 1)?;
                true
            }
            (0, ID_CUES) | (1, ID_CUEPOINT) | (2, ID_CUETRACKPOSITIONS) => {
                relocate(buf, k.payload.clone(), map, depth + 1)?;
                true
            }
            (0, ID_CLUSTER) => {
                // Only the Cluster's own Position child.
                if let Ok(cs) = children(buf, k.payload.clone()) {
                    for c in cs.iter().filter(|c| c.id == ID_CLUSTER_POSITION) {
                        patch_uint(buf, c.payload.clone(), map)?;
                    }
                }
                true
            }
            (2, ID_SEEKPOSITION) | (3, ID_CUECLUSTERPOSITION) | (3, ID_CUECODECSTATE) => {
                patch_uint(buf, k.payload.clone(), map)?;
                false
            }
            _ => false,
        };
        if patch_here && k.id != ID_CLUSTER {
            fix_crc(buf, k.payload.clone())?;
        }
    }
    Ok(())
}

fn patch_uint(buf: &mut [u8], r: Range<usize>, map: &dyn Fn(u64) -> Option<u64>) -> Result<()> {
    let old = read_uint(&buf[r.clone()]);
    let Some(new) = map(old) else {
        // Points into a rewritten element: nothing there to follow.
        return Ok(());
    };
    let b = uint_bytes(new, r.len()).ok_or_else(|| err("a position no longer fits its field"))?;
    buf[r].copy_from_slice(&b);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn el(id: u32, p: &[u8]) -> Vec<u8> {
        element(id, p, 1).unwrap()
    }

    /// EBML header, Segment { SeekHead(->Info, ->Cluster), Info, Cluster }.
    fn sample(void_after_info: usize) -> (Vec<u8>, usize) {
        let header = el(ID_EBML, &el(0x4282, b"matroska"));
        let info = el(ID_INFO, &el(0x4D80, b"mux"));
        let seek = |id: u32, pos: u32| {
            let mut p = el(0x53AB, &id_bytes(id));
            p.extend(el(ID_SEEKPOSITION, &pos.to_be_bytes()));
            el(ID_SEEK, &p)
        };
        let build = |info_at: u32, cluster_at: u32| {
            let sh = el(
                ID_SEEKHEAD,
                &[seek(ID_INFO, info_at), seek(ID_CLUSTER, cluster_at)].concat(),
            );
            (sh.len(), sh)
        };
        let (shl, _) = build(0, 0);
        let info_at = shl as u32;
        let mut body_wo_sh = info.clone();
        if void_after_info > 0 {
            body_wo_sh.extend(void(void_after_info).unwrap());
        }
        let cluster_at = info_at + body_wo_sh.len() as u32;
        let cluster = el(ID_CLUSTER, &[el(0xE7, &[0]), el(0xA3, b"FRAME")].concat());
        let (_, sh) = build(info_at, cluster_at);
        let body = [sh, body_wo_sh, cluster].concat();
        let mut f = header;
        f.extend(id_bytes(ID_SEGMENT));
        f.extend(size_bytes(body.len(), 8).unwrap());
        let seg_start = f.len();
        f.extend(body);
        (f, seg_start)
    }

    fn read(file: &[u8]) -> Vec<(String, String)> {
        crate::api::ExifTool::new()
            .extract_info_from_bytes(file, std::path::Path::new(""))
            .unwrap()
            .into_iter()
            .map(|t| (t.name, t.print_value))
            .collect()
    }

    /// The Cluster seek entry still points at the Cluster.
    fn check_seek(file: &[u8], seg_start: usize) {
        let i = file
            .windows(4)
            .rposition(|w| w == [0x1F, 0x43, 0xB6, 0x75])
            .unwrap();
        // The second SeekPosition (4 bytes) is the Cluster's.
        let positions: Vec<usize> = file
            .windows(3)
            .enumerate()
            .filter(|(_, w)| *w == [0x53, 0xAC, 0x84])
            .map(|(j, _)| u32::from_be_bytes(file[j + 3..j + 7].try_into().unwrap()) as usize)
            .collect();
        assert_eq!(positions[1] + seg_start, i);
    }

    fn set(tag: &str, v: &str) -> MkvChanges {
        MkvChanges {
            set: vec![(tag.into(), v.into())],
            ..Default::default()
        }
    }

    #[test]
    fn title_grows_and_relocates() {
        let (src, seg) = sample(0);
        check_seek(&src, seg);
        let out = write_matroska(&src, &set("Title", "A new title")).unwrap();
        assert!(read(&out).contains(&("Title".into(), "A new title".into())));
        check_seek(&out, seg);
        assert!(out.windows(5).any(|w| w == b"FRAME"));
        let size = read_size(&out, seg - 8).unwrap().0 as usize;
        assert_eq!(seg + size, out.len());
        // Deleting it again shrinks Info and pads with a Void: nothing moves.
        let ch = MkvChanges {
            delete: vec!["title".into()],
            ..Default::default()
        };
        let back = write_matroska(&out, &ch).unwrap();
        assert!(!read(&back).iter().any(|(n, _)| n == "Title"));
        assert_eq!(back.len(), out.len());
        check_seek(&back, seg);
    }

    #[test]
    fn title_uses_the_following_void() {
        let (src, seg) = sample(40);
        let out = write_matroska(&src, &set("Title", "Fits")).unwrap();
        assert_eq!(out.len(), src.len());
        assert!(read(&out).contains(&("Title".into(), "Fits".into())));
        check_seek(&out, seg);
    }

    #[test]
    fn simple_tags_add_edit_delete() {
        let (src, seg) = sample(0);
        let out = write_matroska(&src, &set("Artist", "Me")).unwrap();
        assert!(read(&out).contains(&("Artist".into(), "Me".into())));
        check_seek(&out, seg);
        let out2 = write_matroska(&out, &set("Artist", "You")).unwrap();
        assert!(read(&out2).contains(&("Artist".into(), "You".into())));
        let ch = MkvChanges {
            delete: vec!["artist".into()],
            ..Default::default()
        };
        let out3 = write_matroska(&out2, &ch).unwrap();
        assert!(!read(&out3).iter().any(|(n, _)| n == "Artist"));
        check_seek(&out3, seg);
    }

    #[test]
    fn void_sizes() {
        for n in 2..300 {
            let v = void(n).unwrap();
            assert_eq!(v.len(), n);
            let e = read_elem(&v, 0, v.len()).unwrap();
            assert_eq!(e.end(), n);
        }
        assert!(void(1).is_none());
    }

    #[test]
    fn crc32_is_ieee() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }
}
