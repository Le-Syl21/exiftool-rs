//! HEIF/HEIC/AVIF metadata writer.
//!
//! A HEIF image keeps its EXIF and XMP as *items* of the top-level `meta`
//! box (see [`heif_items`](crate::formats::heif_items)): an `infe` entry
//! names the item, an `iloc` entry locates its bytes (in `mdat`, or in
//! `idat`), and a `cdsc` reference ties it to the primary image. Writing
//! metadata therefore means replacing an item's bytes, moving whatever
//! follows them, and rewriting every offset that pointed past them -- which
//! is what ExifTool's `WriteItemInfo` (WriteQuickTime.pl) does.
//!
//! [`rewrite`] does the same, with these choices:
//!
//! - New item data replaces the old data where it was; data that grows or
//!   shrinks shifts what follows, and the `iloc` entries of every other item
//!   (and the chunk offsets of any track in `moov`) are relocated. `iloc`
//!   field sizes 0, 4 and 8, `base_offset`, several extents, versions 0-2
//!   and construction methods 0 (`mdat`) and 1 (`idat`) are handled; a field
//!   that no longer fits its size is widened.
//! - An item whose metadata is deleted is removed: its `infe`, `iloc`,
//!   `iref` and `ipma` entries and its bytes. (ExifTool keeps such an item
//!   with a zero-length extent; both read back with no metadata.)
//! - A missing Exif or XMP item is created, as ExifTool creates it: a hidden
//!   `infe` with the next free item ID, a `cdsc` reference to the primary
//!   image (and to its `tmap` item, if any), and the data at the start of
//!   the first `mdat`.
//! - Image data and item properties are copied byte for byte.
//!
//! Anything the writer does not fully understand -- a protected or external
//! item, construction method 2, a fragmented file, an `iloc` inside a track
//! -- is an error, never a guess.

use crate::error::{Error, Result};
use crate::formats::heif_items::{
    self, encode_content, fits, BoxRef, Extent, ItemInfoEntry, ItemLocation, ItemReference,
    ItemReferences, MetadataKind,
};
use std::collections::BTreeMap;
use std::ops::Range;

/// The current Exif and XMP payloads of a HEIF file (decoded: an Exif
/// payload still starts with its 4-byte header offset).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetadataItems {
    pub exif: Option<Vec<u8>>,
    pub xmp: Option<Vec<u8>>,
}

/// What to do with one metadata item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemChange {
    Keep,
    /// Remove the item (a no-op when there is none).
    Remove,
    /// Replace the item's content (creating the item if needed). The content
    /// is given decoded; the item's own `content_encoding` is reapplied.
    Replace(Vec<u8>),
}

fn err(msg: impl Into<String>) -> Error {
    Error::InvalidData(format!("HEIF: {}", msg.into()))
}

/// Read the primary image's Exif and XMP payloads.
pub fn read_metadata(source: &[u8]) -> Result<MetadataItems> {
    let (_, _, meta) = heif_items::top_level_meta(source).map_err(err)?;
    let get = |kind| -> Result<Option<Vec<u8>>> {
        match meta.primary_metadata_item(kind) {
            Some(e) => meta.item_data(source, e.item_id).map(Some).map_err(err),
            None => Ok(None),
        }
    };
    Ok(MetadataItems {
        exif: get(MetadataKind::Exif)?,
        xmp: get(MetadataKind::Xmp)?,
    })
}

/// Write metadata to HEIF/HEIC/AVIF files.
///
/// Kept for API compatibility. `new_xmp` replaces (or creates) the XMP item.
/// HEIF has no iTunes-style item list, so `new_tags` must be empty: earlier
/// versions passed them to the MP4 writer, which wrote nothing a HEIF reader
/// looks at.
pub fn write_heif(
    source: &[u8],
    new_tags: &[(&[u8; 4], &str)],
    new_xmp: Option<&[u8]>,
) -> Result<Vec<u8>> {
    if !new_tags.is_empty() {
        return Err(Error::UnsupportedFileType(
            "HEIF has no QuickTime item list to write; use ExifTool::write_info".into(),
        ));
    }
    match new_xmp {
        Some(x) => rewrite(source, ItemChange::Keep, ItemChange::Replace(x.to_vec())),
        None => Ok(source.to_vec()),
    }
}

/// One planned change to an item's bytes.
#[derive(Debug, Clone)]
enum Plan {
    Remove(u32),
    Replace(u32, Vec<u8>),
    Create(MetadataKind, Vec<u8>),
}

/// Edits to a payload: an old byte range and what replaces it (new data for
/// an item, or nothing).
type Edits = Vec<(Range<usize>, Option<(u32, Vec<u8>)>)>;

/// A piece of the new file (or of a rebuilt `mdat`/`idat` payload).
#[derive(Debug, Clone)]
enum Piece {
    /// Old bytes, copied.
    Copy(Range<usize>),
    /// New data for item `id`.
    Item(u32, Vec<u8>),
}

/// Where an item's new data was placed: offset in the file (construction
/// method 0) or in the new `idat` payload (method 1).
#[derive(Debug, Clone, Copy)]
struct Placed {
    pos: u64,
    len: u64,
}

/// Lay out `pieces` starting at `at`; returns the copy runs (old range, new
/// start), the item placements and the end position.
fn layout(
    pieces: &[Piece],
    mut at: u64,
    runs: &mut Vec<(Range<usize>, u64)>,
    placed: &mut BTreeMap<u32, Placed>,
) -> u64 {
    for p in pieces {
        match p {
            Piece::Copy(r) => {
                runs.push((r.clone(), at));
                at += r.len() as u64;
            }
            Piece::Item(id, d) => {
                placed.insert(
                    *id,
                    Placed {
                        pos: at,
                        len: d.len() as u64,
                    },
                );
                at += d.len() as u64;
            }
        }
    }
    at
}

/// Map an old position (and length) through the copy runs.
fn map(runs: &[(Range<usize>, u64)], pos: u64, len: u64) -> Option<u64> {
    for (r, new) in runs {
        let (s, e) = (r.start as u64, r.end as u64);
        let end = pos.checked_add(len)?;
        if pos >= s && end <= e && (len > 0 || pos < e || s == e) {
            return Some(new + (pos - s));
        }
    }
    // A zero-length extent at the very end of a run.
    if len == 0 {
        for (r, new) in runs {
            if pos == r.end as u64 {
                return Some(new + r.len() as u64);
            }
        }
    }
    None
}

/// Rebuild the pieces of a payload (`mdat` or `idat`) with edits applied.
/// `edits` are (old range, replacement) with ranges inside `payload`.
fn edit_payload(payload: Range<usize>, mut edits: Edits) -> Vec<Piece> {
    edits.sort_by_key(|(r, _)| (r.start, r.end));
    let mut pieces = Vec::new();
    let mut cur = payload.start;
    for (r, rep) in edits {
        if r.start > cur {
            pieces.push(Piece::Copy(cur..r.start));
        }
        if let Some((id, d)) = rep {
            pieces.push(Piece::Item(id, d));
        }
        cur = cur.max(r.end);
    }
    if cur < payload.end {
        pieces.push(Piece::Copy(cur..payload.end));
    }
    pieces
}

fn box_bytes(kind: &[u8; 4], payload: &[u8]) -> Result<Vec<u8>> {
    let size = payload.len() + 8;
    if size > u32::MAX as usize {
        return Err(err(format!(
            "{} box too large",
            String::from_utf8_lossy(kind)
        )));
    }
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(&(size as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(payload);
    Ok(out)
}

/// The `infe` box ExifTool writes for a new item (WriteQuickTime.pl): version
/// 2 (3 for a 32-bit ID), flags 1 ("hidden"), no name.
fn new_infe(id: u32, kind: MetadataKind) -> Vec<u8> {
    let mut p = Vec::new();
    if id <= 0xffff {
        p.extend_from_slice(&[2, 0, 0, 1]);
        p.extend_from_slice(&(id as u16).to_be_bytes());
    } else {
        p.extend_from_slice(&[3, 0, 0, 1]);
        p.extend_from_slice(&id.to_be_bytes());
    }
    p.extend_from_slice(&[0, 0]); // protection index
    match kind {
        MetadataKind::Exif => p.extend_from_slice(b"Exif\0"),
        MetadataKind::Xmp => p.extend_from_slice(b"mime\0application/rdf+xml\0"),
    }
    box_bytes(b"infe", &p).expect("small box")
}

/// Is there a box of one of these kinds anywhere under `range`? Walks the
/// containers a track can hold.
fn contains_box(data: &[u8], range: Range<usize>, kinds: &[&[u8; 4]], depth: u32) -> bool {
    if depth > 16 {
        return true;
    }
    let Ok(boxes) = heif_items::parse_boxes(data, range.start, range.end) else {
        return false;
    };
    boxes.iter().any(|b| {
        if kinds.contains(&&b.kind) {
            return true;
        }
        let p = b.payload();
        match &b.kind {
            b"trak" | b"mdia" | b"minf" | b"stbl" | b"udta" | b"edts" | b"dinf" | b"mvex" => {
                contains_box(data, p, kinds, depth + 1)
            }
            b"meta" if p.len() >= 4 => contains_box(data, p.start + 4..p.end, kinds, depth + 1),
            _ => false,
        }
    })
}

/// Relocate the chunk offsets (`stco`/`co64`) of every track in `moov`,
/// in place in `out`, where the box now starts at `new_start`.
fn patch_chunk_offsets(
    data: &[u8],
    moov: &BoxRef,
    new_start: usize,
    out: &mut [u8],
    runs: &[(Range<usize>, u64)],
) -> Result<()> {
    fn walk(
        data: &[u8],
        range: Range<usize>,
        shift: isize,
        out: &mut [u8],
        runs: &[(Range<usize>, u64)],
        depth: u32,
    ) -> Result<()> {
        if depth > 16 {
            return Err(err("moov nests too deep"));
        }
        let boxes = heif_items::parse_boxes(data, range.start, range.end).map_err(err)?;
        for b in boxes {
            let p = b.payload();
            match &b.kind {
                b"trak" | b"mdia" | b"minf" | b"stbl" => {
                    walk(data, p, shift, out, runs, depth + 1)?
                }
                b"stco" | b"co64" => {
                    let wide = &b.kind == b"co64";
                    let d = &data[p.clone()];
                    if d.len() < 8 {
                        return Err(err("truncated chunk offset table"));
                    }
                    let n = u32::from_be_bytes([d[4], d[5], d[6], d[7]]) as usize;
                    let w = if wide { 8 } else { 4 };
                    if n.checked_mul(w).and_then(|x| x.checked_add(8)) > Some(d.len()) {
                        return Err(err("truncated chunk offset table"));
                    }
                    for i in 0..n {
                        let at = 8 + i * w;
                        let old = if wide {
                            let mut a = [0u8; 8];
                            a.copy_from_slice(&d[at..at + 8]);
                            u64::from_be_bytes(a)
                        } else {
                            u32::from_be_bytes([d[at], d[at + 1], d[at + 2], d[at + 3]]) as u64
                        };
                        let new = map(runs, old, 0)
                            .ok_or_else(|| err("a track chunk points into edited data"))?;
                        let dst = (p.start as isize + shift) as usize + at;
                        if wide {
                            out[dst..dst + 8].copy_from_slice(&new.to_be_bytes());
                        } else {
                            let v = u32::try_from(new)
                                .map_err(|_| err("a track chunk offset no longer fits stco"))?;
                            out[dst..dst + 4].copy_from_slice(&v.to_be_bytes());
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    let shift = new_start as isize - moov.start as isize;
    walk(data, moov.payload(), shift, out, runs, 0)
}

/// Apply `exif` and `xmp` to the primary image's metadata items.
pub fn rewrite(source: &[u8], exif: ItemChange, xmp: ItemChange) -> Result<Vec<u8>> {
    let (boxes, meta_idx, meta) = heif_items::top_level_meta(source).map_err(err)?;
    let (Some(iinf), Some(iloc)) = (meta.iinf.clone(), meta.iloc.clone()) else {
        return Err(err("the meta box has no item tables (iinf/iloc)"));
    };
    if iinf.declared_count != iinf.entries.len() || iinf.has_foreign_boxes {
        return Err(err("the iinf box is inconsistent"));
    }
    let primary = meta.primary().ok_or_else(|| err("no primary item"))?;

    // What to do with each item.
    let mut plans = Vec::new();
    for (kind, change) in [(MetadataKind::Exif, exif), (MetadataKind::Xmp, xmp)] {
        let existing = meta.primary_metadata_item(kind).map(|e| e.item_id);
        match (change, existing) {
            (ItemChange::Keep, _) | (ItemChange::Remove, None) => {}
            (ItemChange::Remove, Some(id)) => plans.push(Plan::Remove(id)),
            (ItemChange::Replace(d), Some(id)) => plans.push(Plan::Replace(id, d)),
            (ItemChange::Replace(d), None) => plans.push(Plan::Create(kind, d)),
        }
    }
    if plans.is_empty() {
        return Ok(source.to_vec());
    }
    for b in &boxes {
        if matches!(&b.kind, b"moof" | b"sidx" | b"ssix") {
            return Err(err("fragmented files can't be rewritten"));
        }
        if &b.kind == b"moov" && contains_box(source, b.payload(), &[b"iloc", b"saio"], 0) {
            return Err(err("a track holds its own item locations; not supported"));
        }
    }

    // Validate the edited items and encode their new data.
    let mut edited: BTreeMap<u32, Option<Vec<u8>>> = BTreeMap::new(); // id -> new stored data
    for plan in &plans {
        let id = match plan {
            Plan::Remove(id) | Plan::Replace(id, _) => *id,
            Plan::Create(..) => continue,
        };
        let e = iinf.get(id).expect("found through iinf");
        let loc = iloc
            .get(id)
            .ok_or_else(|| err(format!("item {id} has no iloc entry")))?;
        if e.protection_index != 0 {
            return Err(err(format!("item {id} is protected")));
        }
        if loc.data_reference_index != 0 {
            return Err(err(format!("item {id} is stored in another file")));
        }
        if loc.construction_method > 1 {
            return Err(err(format!(
                "item {id} uses construction method {}",
                loc.construction_method
            )));
        }
        let stored = match plan {
            Plan::Replace(_, d) => Some(encode_content(d, &e.content_encoding).map_err(err)?),
            _ => None,
        };
        edited.insert(id, stored);
    }

    // The old byte ranges of the edited items, checked against everything
    // else that uses the same bytes.
    let abs_extents = |loc: &ItemLocation| -> Result<Vec<Range<u64>>> {
        loc.extents
            .iter()
            .filter(|x| x.length > 0)
            .map(|x| {
                let s = loc
                    .base_offset
                    .checked_add(x.offset)
                    .ok_or_else(|| err("item offset overflows"))?;
                let e = s
                    .checked_add(x.length)
                    .ok_or_else(|| err("item length overflows"))?;
                Ok(s..e)
            })
            .collect()
    };
    let idat = meta.idat.clone();
    let mut mdat_edits: BTreeMap<usize, Edits> = BTreeMap::new(); // top-level box index -> edits
    let mut idat_edits: Edits = Vec::new();
    let mut pending: Vec<(u32, u8, Vec<u8>)> = Vec::new(); // (id, cm, data) with nowhere to go yet
    for (&id, stored) in &edited {
        let loc = iloc.get(id).expect("checked above");
        let ranges = abs_extents(loc)?;
        for other in &iloc.items {
            if other.item_id == id
                || other.data_reference_index != 0
                || other.construction_method != loc.construction_method
            {
                continue;
            }
            for o in abs_extents(other)? {
                if ranges.iter().any(|r| r.start < o.end && o.start < r.end) {
                    return Err(err(format!(
                        "item {id} shares its bytes with item {}",
                        other.item_id
                    )));
                }
            }
        }
        let mut first = true;
        for r in &ranges {
            let rep = if first {
                first = false;
                stored.clone().map(|d| (id, d))
            } else {
                None
            };
            if loc.construction_method == 0 {
                let k = boxes
                    .iter()
                    .position(|b| {
                        &b.kind == b"mdat"
                            && r.start >= b.payload().start as u64
                            && r.end <= b.end as u64
                    })
                    .ok_or_else(|| err(format!("item {id} is not inside an mdat box")))?;
                mdat_edits
                    .entry(k)
                    .or_default()
                    .push((r.start as usize..r.end as usize, rep));
            } else {
                let p = idat.clone().ok_or_else(|| err("no idat box"))?;
                if r.end > p.len() as u64 {
                    return Err(err(format!("item {id} runs past the idat box")));
                }
                idat_edits.push((p.start + r.start as usize..p.start + r.end as usize, rep));
            }
        }
        if ranges.is_empty() {
            if let Some(d) = stored {
                pending.push((id, loc.construction_method, d.clone()));
            }
        }
    }

    // New items: IDs after the highest one in use.
    let mut next_id = iinf
        .entries
        .iter()
        .map(|e| e.item_id)
        .chain(iloc.items.iter().map(|i| i.item_id))
        .max()
        .unwrap_or(0);
    let mut created: Vec<(u32, MetadataKind)> = Vec::new();
    for plan in &plans {
        if let Plan::Create(kind, d) = plan {
            next_id = next_id
                .checked_add(1)
                .ok_or_else(|| err("no free item ID"))?;
            created.push((next_id, *kind));
            pending.push((next_id, 0, d.clone()));
        }
    }
    // Data with no old position goes to the start of the first mdat (or of
    // idat for an emptied idat item), as ExifTool inserts it.
    let first_mdat = boxes.iter().position(|b| &b.kind == b"mdat");
    let mut new_mdat: Vec<Piece> = Vec::new();
    for (id, cm, d) in pending {
        if cm == 1 {
            let p = idat.clone().ok_or_else(|| err("no idat box"))?;
            idat_edits.push((p.start..p.start, Some((id, d))));
        } else if let Some(k) = first_mdat {
            let s = boxes[k].payload().start;
            mdat_edits.entry(k).or_default().push((s..s, Some((id, d))));
        } else {
            new_mdat.push(Piece::Item(id, d));
        }
    }
    if !new_mdat.is_empty() && boxes.last().is_some_and(|b| b.to_eof) {
        return Err(err(
            "the last box runs to the end of the file; can't append",
        ));
    }

    // The new item tables (all but iloc, whose values depend on the layout).
    let removed: Vec<u32> = plans
        .iter()
        .filter_map(|p| match p {
            Plan::Remove(id) => Some(*id),
            _ => None,
        })
        .collect();
    let mut new_iinf = iinf.clone();
    new_iinf.entries.retain(|e| !removed.contains(&e.item_id));
    for &(id, kind) in &created {
        let raw = new_infe(id, kind);
        new_iinf
            .entries
            .push(ItemInfoEntry::parse(&raw, 8).expect("well-formed infe"));
        if id > 0xffff && new_iinf.version == 0 {
            new_iinf.version = 1;
        }
    }
    let mut new_iref = meta.iref.clone();
    if !removed.is_empty() {
        if let Some(r) = &mut new_iref {
            r.refs.retain(|x| !removed.contains(&x.from));
            for x in &mut r.refs {
                x.to.retain(|t| !removed.contains(t));
            }
            r.refs.retain(|x| !x.to.is_empty());
        }
    }
    if !created.is_empty() {
        // The primary's tone-map item, which ExifTool also points new
        // metadata at.
        let tmap = iinf
            .entries
            .iter()
            .find(|e| &e.item_type == b"tmap" && meta.refers_to(e.item_id).contains(&primary))
            .map(|e| e.item_id);
        let r = new_iref.get_or_insert(ItemReferences {
            version: 0,
            flags: [0; 3],
            refs: Vec::new(),
        });
        for &(id, _) in &created {
            let mut to = vec![primary];
            to.extend(tmap);
            r.refs.push(ItemReference {
                kind: *b"cdsc",
                from: id,
                to,
            });
        }
    }
    if let Some(r) = &mut new_iref {
        let wide = r
            .refs
            .iter()
            .any(|x| x.from > 0xffff || x.to.iter().any(|&t| t > 0xffff));
        if wide {
            r.version = 1;
        }
    }
    let iref_payload = new_iref
        .as_ref()
        .filter(|r| !r.refs.is_empty())
        .map(|r| r.serialize());

    // ipma: drop the entries of removed items (rebuilding iprp around it).
    let mut iprp_bytes: Option<Vec<u8>> = None;
    if !removed.is_empty() {
        if let Some(iprp) = meta.children.iter().find(|b| &b.kind == b"iprp") {
            let kids =
                heif_items::parse_boxes(source, iprp.payload().start, iprp.end).map_err(err)?;
            let mut payload = Vec::new();
            let mut touched = false;
            for k in &kids {
                if &k.kind == b"ipma" {
                    let mut ipma =
                        heif_items::ItemPropertyAssociations::parse(&source[k.payload()])
                            .map_err(err)?;
                    let n = ipma.entries.len();
                    ipma.entries.retain(|(id, _)| !removed.contains(id));
                    if ipma.entries.len() != n {
                        touched = true;
                        payload.extend(box_bytes(b"ipma", &ipma.serialize())?);
                        continue;
                    }
                }
                payload.extend_from_slice(&source[k.start..k.end]);
            }
            if touched {
                iprp_bytes = Some(box_bytes(b"iprp", &payload)?);
            }
        }
    }

    // idat payload pieces.
    let idat_pieces = idat
        .clone()
        .map(|p| edit_payload(p, std::mem::take(&mut idat_edits)));

    // The iloc entries that survive, in order, plus the created ones.
    let mut new_iloc = iloc.clone();
    new_iloc.items.retain(|i| !removed.contains(&i.item_id));
    for &(id, _) in &created {
        new_iloc.items.push(ItemLocation {
            item_id: id,
            construction_method: 0,
            cm_reserved: 0,
            data_reference_index: 0,
            base_offset: 0,
            extents: vec![Extent {
                index: 0,
                offset: 0,
                length: 0,
            }],
        });
    }
    // An edited item ends up with one extent; giving it that shape now keeps
    // the size of the iloc box independent of the values laid out below.
    for item in new_iloc.items.iter_mut() {
        if edited.contains_key(&item.item_id) {
            let index = item.extents.first().map(|e| e.index).unwrap_or(0);
            item.extents = vec![Extent {
                index,
                offset: 0,
                length: 0,
            }];
        }
    }
    if new_iloc.items.iter().any(|i| i.item_id > 0xffff) || new_iloc.items.len() > 0xffff {
        new_iloc.version = 2;
    }

    // Lay out until the iloc field sizes are stable.
    for _ in 0..8 {
        // The meta box, with iloc values still to be filled in (its size
        // does not depend on them).
        let meta_box = &boxes[meta_idx];
        let build_meta = |iloc_payload: &[u8], idat_payload: Option<&[u8]>| -> Result<Vec<u8>> {
            let mut payload = source[meta.payload.start..meta.payload.start + 4].to_vec();
            let mut wrote_iref = false;
            for c in &meta.children {
                match &c.kind {
                    b"iloc" => payload.extend(box_bytes(b"iloc", iloc_payload)?),
                    b"iinf" => {
                        payload.extend(box_bytes(b"iinf", &new_iinf.serialize())?);
                        if meta.iref.is_none() {
                            if let Some(r) = &iref_payload {
                                payload.extend(box_bytes(b"iref", r)?);
                                wrote_iref = true;
                            }
                        }
                    }
                    b"iref" => {
                        if let Some(r) = &iref_payload {
                            payload.extend(box_bytes(b"iref", r)?);
                            wrote_iref = true;
                        }
                    }
                    b"iprp" if iprp_bytes.is_some() => {
                        payload.extend_from_slice(iprp_bytes.as_ref().unwrap())
                    }
                    b"idat" => match idat_payload {
                        Some(p) => payload.extend(box_bytes(b"idat", p)?),
                        None => payload.extend_from_slice(&source[c.start..c.end]),
                    },
                    _ => payload.extend_from_slice(&source[c.start..c.end]),
                }
            }
            if !wrote_iref {
                if let Some(r) = &iref_payload {
                    payload.extend(box_bytes(b"iref", r)?);
                }
            }
            let mut out = if meta_box.header_len == 16 {
                let mut h = vec![0, 0, 0, 1];
                h.extend_from_slice(b"meta");
                h.extend_from_slice(&((payload.len() + 16) as u64).to_be_bytes());
                h
            } else {
                box_bytes(b"meta", &[])?[..8].to_vec()
            };
            if meta_box.header_len != 16 {
                let size = u32::try_from(payload.len() + 8).map_err(|_| err("meta too large"))?;
                out[..4].copy_from_slice(&size.to_be_bytes());
            }
            out.extend(payload);
            Ok(out)
        };

        // idat layout (relative to its payload).
        let mut idat_runs: Vec<(Range<usize>, u64)> = Vec::new();
        let mut idat_placed: BTreeMap<u32, Placed> = BTreeMap::new();
        let idat_payload: Option<Vec<u8>> = idat_pieces.as_ref().map(|pieces| {
            let base = idat.as_ref().unwrap().start;
            let rel: Vec<Piece> = pieces
                .iter()
                .map(|p| match p {
                    Piece::Copy(r) => Piece::Copy(r.start - base..r.end - base),
                    other => other.clone(),
                })
                .collect();
            layout(&rel, 0, &mut idat_runs, &mut idat_placed);
            let mut out = Vec::new();
            for p in pieces {
                match p {
                    Piece::Copy(r) => out.extend_from_slice(&source[r.clone()]),
                    Piece::Item(_, d) => out.extend_from_slice(d),
                }
            }
            out
        });

        let dummy_iloc = new_iloc.serialize();
        let meta_len = build_meta(&dummy_iloc, idat_payload.as_deref())?.len();

        // File layout.
        let mut runs: Vec<(Range<usize>, u64)> = Vec::new();
        let mut placed: BTreeMap<u32, Placed> = BTreeMap::new();
        let mut at: u64 = 0;
        let mut mdat_headers: BTreeMap<usize, Vec<u8>> = BTreeMap::new();
        let mut mdat_pieces: BTreeMap<usize, Vec<Piece>> = BTreeMap::new();
        for (k, b) in boxes.iter().enumerate() {
            if k == meta_idx {
                at += meta_len as u64;
            } else if let Some(edits) = mdat_edits.get(&k) {
                let pieces = edit_payload(b.payload(), edits.clone());
                let body: u64 = pieces
                    .iter()
                    .map(|p| match p {
                        Piece::Copy(r) => r.len() as u64,
                        Piece::Item(_, d) => d.len() as u64,
                    })
                    .sum();
                let header = if b.to_eof {
                    let mut h = vec![0, 0, 0, 0];
                    h.extend_from_slice(b"mdat");
                    h
                } else if b.header_len == 16 || body + 8 > u32::MAX as u64 {
                    let mut h = vec![0, 0, 0, 1];
                    h.extend_from_slice(b"mdat");
                    h.extend_from_slice(&(body + 16).to_be_bytes());
                    h
                } else {
                    let mut h = ((body + 8) as u32).to_be_bytes().to_vec();
                    h.extend_from_slice(b"mdat");
                    h
                };
                at += header.len() as u64;
                at = layout(&pieces, at, &mut runs, &mut placed);
                mdat_headers.insert(k, header);
                mdat_pieces.insert(k, pieces);
            } else {
                runs.push((b.start..b.end, at));
                at += (b.end - b.start) as u64;
            }
        }
        // A new mdat goes right after the last box (before any padding).
        let appended_header = if new_mdat.is_empty() {
            None
        } else {
            let body: u64 = new_mdat
                .iter()
                .map(|p| match p {
                    Piece::Item(_, d) => d.len() as u64,
                    Piece::Copy(r) => r.len() as u64,
                })
                .sum();
            let size = u32::try_from(body + 8).map_err(|_| err("new mdat too large"))?;
            let mut h = size.to_be_bytes().to_vec();
            h.extend_from_slice(b"mdat");
            at += 8;
            at = layout(&new_mdat, at, &mut Vec::new(), &mut placed);
            Some(h)
        };

        let tail_start = boxes.last().map(|b| b.end).unwrap_or(0);
        if tail_start < source.len() {
            runs.push((tail_start..source.len(), at));
            at += (source.len() - tail_start) as u64;
        }
        // The iloc values.
        let mut iloc_out = new_iloc.clone();
        let mut grow: Option<&'static str> = None;
        for item in iloc_out.items.iter_mut() {
            let id = item.item_id;
            let new_place = match item.construction_method {
                0 if edited.contains_key(&id) || created.iter().any(|(c, _)| *c == id) => {
                    Some(placed.get(&id).copied())
                }
                1 if edited.contains_key(&id) => Some(idat_placed.get(&id).copied()),
                _ => None,
            };
            if let Some(p) = new_place {
                // An edited item: one extent at its new place.
                let p = p.unwrap_or(Placed { pos: 0, len: 0 });
                item.extents = vec![Extent {
                    index: item.extents.first().map(|e| e.index).unwrap_or(0),
                    offset: 0,
                    length: p.len,
                }];
                if iloc_out.offset_size > 0 || iloc_out.base_offset_size == 0 {
                    item.base_offset = 0;
                    item.extents[0].offset = p.pos;
                } else {
                    item.base_offset = p.pos;
                }
            } else if item.data_reference_index == 0 && item.construction_method <= 1 {
                let r = if item.construction_method == 0 {
                    &runs
                } else {
                    &idat_runs
                };
                if item.construction_method == 1 && idat_pieces.is_none() {
                    continue; // idat unchanged: method-1 offsets stay valid
                }
                let old_base = item.base_offset;
                let mut new_abs = Vec::with_capacity(item.extents.len());
                for x in &item.extents {
                    let old = old_base
                        .checked_add(x.offset)
                        .ok_or_else(|| err("item offset overflows"))?;
                    let n = match map(r, old, x.length) {
                        Some(n) => n,
                        None if x.length == 0 => 0,
                        None => {
                            return Err(err(format!(
                                "item {id} lies in a region this writer rewrites"
                            )))
                        }
                    };
                    new_abs.push(n);
                }
                if iloc_out.base_offset_size == 0 {
                    for (x, n) in item.extents.iter_mut().zip(&new_abs) {
                        x.offset = *n;
                    }
                } else if iloc_out.offset_size == 0 {
                    let first = new_abs.first().copied().unwrap_or(0);
                    if new_abs.iter().any(|&n| n != first) {
                        grow = Some("offset");
                    }
                    item.base_offset = first;
                } else if old_base == 0 {
                    for (x, n) in item.extents.iter_mut().zip(&new_abs) {
                        x.offset = *n;
                    }
                } else {
                    let old0 = old_base + item.extents.first().map(|x| x.offset).unwrap_or(0);
                    let new0 = new_abs.first().copied().unwrap_or(old0);
                    let mut base = (old_base as i128 + new0 as i128 - old0 as i128).max(0) as u64;
                    if new_abs.iter().any(|&n| n < base) {
                        base = new_abs.iter().copied().min().unwrap_or(0);
                    }
                    item.base_offset = base;
                    for (x, n) in item.extents.iter_mut().zip(&new_abs) {
                        x.offset = n - base;
                    }
                }
            }
            if !fits(item.base_offset, iloc_out.base_offset_size) {
                grow = grow.or(Some("base"));
            }
            for x in &item.extents {
                if !fits(x.offset, iloc_out.offset_size) {
                    grow = grow.or(Some("offset"));
                }
                if !fits(x.length, iloc_out.length_size) {
                    grow = grow.or(Some("length"));
                }
            }
        }
        if let Some(field) = grow {
            // A value outgrew its field: widen the field and lay out again.
            let widen = |s: &mut u8| *s = if *s == 0 { 4 } else { 8 };
            match field {
                "base" => widen(&mut new_iloc.base_offset_size),
                "offset" => widen(&mut new_iloc.offset_size),
                _ => widen(&mut new_iloc.length_size),
            }
            continue;
        }

        // Assemble.
        let iloc_payload = iloc_out.serialize();
        if iloc_payload.len() != dummy_iloc.len() {
            return Err(err("internal error: iloc size changed during layout"));
        }
        let meta_bytes = build_meta(&iloc_payload, idat_payload.as_deref())?;
        let mut out = Vec::with_capacity(at as usize);
        let mut moov_at: Vec<(usize, usize)> = Vec::new(); // (box index, new start)
        for (k, b) in boxes.iter().enumerate() {
            if k == meta_idx {
                out.extend_from_slice(&meta_bytes);
            } else if let Some(pieces) = mdat_pieces.get(&k) {
                out.extend_from_slice(&mdat_headers[&k]);
                for p in pieces {
                    match p {
                        Piece::Copy(r) => out.extend_from_slice(&source[r.clone()]),
                        Piece::Item(_, d) => out.extend_from_slice(d),
                    }
                }
            } else {
                if &b.kind == b"moov" {
                    moov_at.push((k, out.len()));
                }
                out.extend_from_slice(&source[b.start..b.end]);
            }
        }
        if let Some(h) = appended_header {
            out.extend_from_slice(&h);
            for p in &new_mdat {
                if let Piece::Item(_, d) = p {
                    out.extend_from_slice(d);
                }
            }
        }
        if tail_start < source.len() {
            out.extend_from_slice(&source[tail_start..]);
        }
        if out.len() as u64 != at {
            return Err(err("internal error: layout and output disagree"));
        }
        for (k, start) in moov_at {
            patch_chunk_offsets(source, &boxes[k], start, &mut out, &runs)?;
        }
        return Ok(out);
    }
    Err(err("could not lay out the item locations"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::heif_items::{decode_content, ItemLocations};

    const IMAGE: &[u8] = b"IMAGE-DATA-0123456789";

    struct Fixture {
        /// iloc (offset, length, base) sizes.
        sizes: (u8, u8, u8),
        version: u8,
        /// Exif in idat (construction method 1).
        exif_in_idat: bool,
        /// Image data before the metadata in mdat.
        image_first: bool,
        /// XMP stored deflated.
        deflate_xmp: bool,
    }

    const XMP: &[u8] = b"<x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'><rdf:Description rdf:about='' xmlns:dc='http://purl.org/dc/elements/1.1/' dc:format='image/heic'/></rdf:RDF></x:xmpmeta>";

    fn exif_payload() -> Vec<u8> {
        let mut e = vec![0, 0, 0, 6];
        e.extend_from_slice(b"Exif\0\0MM\0\x2a\0\0\0\x08\0\0\0\0\0\0");
        e
    }

    /// ftyp, meta (hdlr, pitm, iloc, iinf, iref, iprp, optional idat), mdat.
    /// Item 1 is the image (split in two extents when offsets exist), 2 the
    /// Exif, 3 the XMP.
    fn build(f: &Fixture) -> Vec<u8> {
        let exif = exif_payload();
        let xmp_stored = if f.deflate_xmp {
            encode_content(XMP, b"deflate").unwrap()
        } else {
            XMP.to_vec()
        };
        let ftyp = box_bytes(b"ftyp", b"heic\0\0\0\0mif1heic").unwrap();
        let hdlr = box_bytes(
            b"hdlr",
            &[0u8; 4]
                .repeat(2)
                .iter()
                .copied()
                .chain(*b"pict")
                .chain([0u8; 13])
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let pitm = box_bytes(b"pitm", &[0, 0, 0, 0, 0, 1]).unwrap();
        let infe = |id: u8, t: &[u8]| {
            let mut p = vec![2, 0, 0, 0, 0, id, 0, 0];
            p.extend_from_slice(t);
            box_bytes(b"infe", &p).unwrap()
        };
        let mut iinf_p = vec![0, 0, 0, 0, 0, 3];
        iinf_p.extend(infe(1, b"hvc1\0"));
        iinf_p.extend(infe(2, b"Exif\0"));
        if f.deflate_xmp {
            iinf_p.extend(infe(3, b"mime\0application/rdf+xml\0deflate\0"));
        } else {
            iinf_p.extend(infe(3, b"mime\0application/rdf+xml\0"));
        }
        let iinf = box_bytes(b"iinf", &iinf_p).unwrap();
        let cdsc = |from| ItemReference {
            kind: *b"cdsc",
            from,
            to: vec![1],
        };
        let iref = ItemReferences {
            version: 0,
            flags: [0; 3],
            refs: vec![cdsc(2), cdsc(3)],
        };
        let iref = box_bytes(b"iref", &iref.serialize()).unwrap();
        let ispe = box_bytes(b"ispe", &[0, 0, 0, 0, 0, 0, 0, 64, 0, 0, 0, 48]).unwrap();
        let ipco = box_bytes(b"ipco", &ispe).unwrap();
        // Item 1 and (oddly, but legally) item 3 are associated with ispe.
        let ipma = box_bytes(
            b"ipma",
            &[0, 0, 0, 0, 0, 0, 0, 2, 0, 1, 1, 0x81, 0, 3, 1, 0x01],
        )
        .unwrap();
        let iprp = box_bytes(b"iprp", &[ipco, ipma].concat()).unwrap();
        let idat = f
            .exif_in_idat
            .then(|| box_bytes(b"idat", &[b"pad!".to_vec(), exif.clone()].concat()).unwrap());

        let mut order: Vec<(u32, Vec<u8>)> = vec![(3, xmp_stored.clone())];
        if !f.exif_in_idat {
            order.insert(0, (2, exif.clone()));
        }
        if f.image_first {
            order.insert(0, (1, IMAGE.to_vec()));
        } else {
            order.push((1, IMAGE.to_vec()));
        }
        let (o, l, b) = f.sizes;
        let iloc = |pos: &BTreeMap<u32, u64>| {
            let mut items = Vec::new();
            for id in 1..=3u32 {
                let (cm, at, len) = match id {
                    2 if f.exif_in_idat => (1, 4, exif.len() as u64),
                    2 => (0, pos[&2], exif.len() as u64),
                    3 => (0, pos[&3], xmp_stored.len() as u64),
                    _ => (0, pos[&1], IMAGE.len() as u64),
                };
                let (base, off) = if o == 0 {
                    (at, 0)
                } else if b > 0 && id == 1 {
                    (at - 2, 2)
                } else {
                    (0, at)
                };
                // The image and the XMP are split in two extents.
                let extents = if (id == 1 || id == 3) && o > 0 {
                    let half = len / 2;
                    vec![
                        Extent {
                            index: 0,
                            offset: off,
                            length: half,
                        },
                        Extent {
                            index: 0,
                            offset: off + half,
                            length: len - half,
                        },
                    ]
                } else {
                    vec![Extent {
                        index: 0,
                        offset: off,
                        length: len,
                    }]
                };
                items.push(ItemLocation {
                    item_id: id,
                    construction_method: cm,
                    cm_reserved: 0,
                    data_reference_index: 0,
                    base_offset: base,
                    extents,
                });
            }
            let iloc = ItemLocations {
                version: f.version,
                flags: [0; 3],
                offset_size: o,
                length_size: l,
                base_offset_size: b,
                index_size: 0,
                items,
            };
            box_bytes(b"iloc", &iloc.serialize()).unwrap()
        };
        let meta = |iloc: Vec<u8>| {
            let mut p = vec![0, 0, 0, 0];
            for x in [&hdlr, &pitm, &iloc, &iinf, &iref, &iprp] {
                p.extend_from_slice(x);
            }
            if let Some(i) = &idat {
                p.extend_from_slice(i);
            }
            box_bytes(b"meta", &p).unwrap()
        };
        // The iloc's size does not depend on its values: measure, then fill.
        let dummy: BTreeMap<u32, u64> = (1..=3).map(|i| (i, 2)).collect();
        let meta_len = meta(iloc(&dummy)).len();
        let mut pos = BTreeMap::new();
        let mut at = (ftyp.len() + meta_len + 8) as u64;
        for (id, d) in &order {
            pos.insert(*id, at);
            at += d.len() as u64;
        }
        for id in 1..=3 {
            pos.entry(id).or_insert(2);
        }
        let mdat: Vec<u8> = order.iter().flat_map(|(_, d)| d.clone()).collect();
        let out = [ftyp, meta(iloc(&pos)), box_bytes(b"mdat", &mdat).unwrap()].concat();
        let md = read_metadata(&out).unwrap();
        assert_eq!(md.exif.as_deref(), Some(&exif[..]));
        assert_eq!(md.xmp.as_deref(), Some(XMP));
        check_image(&out);
        out
    }

    fn check_image(out: &[u8]) {
        let (_, _, m) = heif_items::top_level_meta(out).unwrap();
        assert_eq!(m.item_data(out, 1).unwrap(), IMAGE);
    }

    fn fixtures() -> Vec<Fixture> {
        let mut v = Vec::new();
        for (sizes, version) in [
            ((4, 4, 0), 0),
            ((4, 4, 4), 1),
            ((8, 8, 8), 2),
            ((0, 4, 4), 1),
            ((4, 8, 0), 0),
        ] {
            for image_first in [false, true] {
                v.push(Fixture {
                    sizes,
                    version,
                    exif_in_idat: false,
                    image_first,
                    deflate_xmp: false,
                });
                if version > 0 {
                    v.push(Fixture {
                        sizes,
                        version,
                        exif_in_idat: true,
                        image_first,
                        deflate_xmp: true,
                    });
                }
            }
        }
        v
    }

    #[test]
    fn removing_both_items_leaves_only_the_image() {
        for f in fixtures() {
            let src = build(&f);
            let out = rewrite(&src, ItemChange::Remove, ItemChange::Remove).unwrap();
            check_image(&out);
            let (_, _, m) = heif_items::top_level_meta(&out).unwrap();
            assert_eq!(m.iinf.as_ref().unwrap().entries.len(), 1);
            assert_eq!(m.iloc.as_ref().unwrap().items.len(), 1);
            assert!(m.iref.is_none());
            assert!(!out.windows(4).any(|w| w == b"Exif"), "{:?}", f.sizes);
            assert!(!out.windows(9).any(|w| w == b"x:xmpmeta"));
            assert_eq!(read_metadata(&out).unwrap(), MetadataItems::default());
            // The removed XMP item's ipma entry went with it.
            let iprp = m.children.iter().find(|b| &b.kind == b"iprp").unwrap();
            let kids = heif_items::parse_boxes(&out, iprp.payload().start, iprp.end).unwrap();
            let ipma = kids.iter().find(|b| &b.kind == b"ipma").unwrap();
            let ipma = heif_items::ItemPropertyAssociations::parse(&out[ipma.payload()]).unwrap();
            assert_eq!(ipma.entries.len(), 1);
        }
    }

    #[test]
    fn replacing_with_larger_and_smaller_data() {
        for f in fixtures() {
            let src = build(&f);
            for len in [3usize, 70_000] {
                let new = vec![b'z'; len];
                let out =
                    rewrite(&src, ItemChange::Keep, ItemChange::Replace(new.clone())).unwrap();
                check_image(&out);
                let md = read_metadata(&out).unwrap();
                assert_eq!(md.xmp.as_deref(), Some(&new[..]));
                assert_eq!(md.exif, Some(exif_payload()));
                let mut e = exif_payload();
                e.extend(std::iter::repeat_n(7u8, len));
                let out =
                    rewrite(&src, ItemChange::Replace(e.clone()), ItemChange::Remove).unwrap();
                check_image(&out);
                let md = read_metadata(&out).unwrap();
                assert_eq!(md.exif.as_deref(), Some(&e[..]));
                assert_eq!(md.xmp, None);
            }
        }
    }

    #[test]
    fn deflated_items_are_stored_deflated() {
        let f = fixtures().into_iter().find(|f| f.deflate_xmp).unwrap();
        let src = build(&f);
        let out = rewrite(
            &src,
            ItemChange::Keep,
            ItemChange::Replace(b"<new/>".to_vec()),
        )
        .unwrap();
        let (_, _, m) = heif_items::top_level_meta(&out).unwrap();
        let stored = m.item_stored_bytes(&out, 3).unwrap();
        assert_eq!(decode_content(&stored, b"deflate").unwrap(), b"<new/>");
    }

    #[test]
    fn creating_items_like_exiftool() {
        let src = build(&fixtures()[0]);
        let bare = rewrite(&src, ItemChange::Remove, ItemChange::Remove).unwrap();
        let out = rewrite(
            &bare,
            ItemChange::Replace(exif_payload()),
            ItemChange::Replace(b"<xmp/>".to_vec()),
        )
        .unwrap();
        check_image(&out);
        let md = read_metadata(&out).unwrap();
        assert_eq!(md.exif, Some(exif_payload()));
        assert_eq!(md.xmp.as_deref(), Some(&b"<xmp/>"[..]));
        let (_, _, m) = heif_items::top_level_meta(&out).unwrap();
        let entries = &m.iinf.as_ref().unwrap().entries;
        assert_eq!(
            entries.iter().map(|e| e.item_id).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        // Hidden, version 2, tied to the primary item.
        assert_eq!(&entries[1].raw[8..12], &[2, 0, 0, 1]);
        assert_eq!(m.refers_to(2), vec![1]);
        assert_eq!(m.refers_to(3), vec![1]);
    }

    #[test]
    fn creating_in_a_file_without_mdat_appends_one() {
        let src = build(&fixtures()[0]);
        let mut bare = rewrite(&src, ItemChange::Remove, ItemChange::Remove).unwrap();
        // Keep the image in a `free` box instead of `mdat`: there is then no
        // mdat to put new data in.
        let (boxes, _, _) = heif_items::top_level_meta(&bare).unwrap();
        let mdat = boxes.iter().find(|b| &b.kind == b"mdat").unwrap().start;
        bare[mdat + 4..mdat + 8].copy_from_slice(b"free");
        // And end it with a byte of padding, which the new mdat goes before.
        bare.push(0);
        check_image(&bare);
        let out = rewrite(
            &bare,
            ItemChange::Keep,
            ItemChange::Replace(b"<x/>".to_vec()),
        )
        .unwrap();
        check_image(&out);
        assert_eq!(
            read_metadata(&out).unwrap().xmp.as_deref(),
            Some(&b"<x/>"[..])
        );
        let (boxes, _, _) = heif_items::top_level_meta(&out).unwrap();
        assert_eq!(&boxes.last().unwrap().kind, b"mdat");
    }

    #[test]
    fn a_shared_extent_is_refused() {
        let mut src = build(&fixtures()[0]);
        // Point the XMP item at the image's bytes.
        let (_, _, m) = heif_items::top_level_meta(&src).unwrap();
        let mut iloc = m.iloc.clone().unwrap();
        let img = iloc.get(1).unwrap().extents[0].offset;
        iloc.items[2].extents[0].offset = img;
        let b = m
            .children
            .iter()
            .find(|b| &b.kind == b"iloc")
            .unwrap()
            .clone();
        src[b.payload()].copy_from_slice(&iloc.serialize());
        assert!(rewrite(&src, ItemChange::Keep, ItemChange::Remove).is_err());
    }

    #[test]
    fn nothing_to_do_returns_the_input() {
        let src = build(&fixtures()[0]);
        assert_eq!(
            rewrite(&src, ItemChange::Keep, ItemChange::Keep).unwrap(),
            src
        );
        let bare = rewrite(&src, ItemChange::Remove, ItemChange::Remove).unwrap();
        assert_eq!(
            rewrite(&bare, ItemChange::Remove, ItemChange::Remove).unwrap(),
            bare
        );
    }

    #[test]
    fn legacy_entry_point() {
        let src = build(&fixtures()[0]);
        assert!(write_heif(&src, &[(b"\xa9nam", "x")], None).is_err());
        assert_eq!(write_heif(&src, &[], None).unwrap(), src);
        let out = write_heif(&src, &[], Some(b"<y/>")).unwrap();
        assert_eq!(
            read_metadata(&out).unwrap().xmp.as_deref(),
            Some(&b"<y/>"[..])
        );
    }
}
