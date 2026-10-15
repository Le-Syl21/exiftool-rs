//! QuickTime/MP4 metadata writer (MOV, MP4, M4A, 3GP, F4V).
//!
//! Follows ExifTool's `WriteQuickTime` (WriteQuickTime.pl) for the three
//! places QuickTime keeps text metadata, and for XMP:
//!
//! - **ItemList** -- iTunes-style items in `moov/udta/meta/ilst`. A written
//!   tag replaces the item's data with one UTF-8 `data` atom; a tag that is
//!   not there yet is created here (the default location for QuickTime
//!   tags), with `udta`, `meta` (a full box with an `mdir` handler) and
//!   `ilst` created as needed.
//! - **UserData** -- `\xa9xxx` text atoms directly in `moov/udta` (2-byte
//!   length, 2-byte language, text). Existing atoms are rewritten keeping
//!   their language; the text is UTF-8 for language 0 or an ISO 639 code and
//!   MacRoman for another Macintosh language code, as ExifTool encodes it.
//!   A tag ExifTool has no ItemList entry for is created here.
//! - **Keys** -- `moov/meta` with a `keys` table and an index-numbered
//!   `ilst`. Existing entries are edited; a deleted key is removed from the
//!   table and the following indexes are renumbered. Keys entries are only
//!   created when the `Keys` group is named.
//! - **XMP** -- `moov/udta/XMP_` in a MOV file, a top-level `uuid` box
//!   (BE7ACFCB-97A9-42E8-9C71-999491E3AFAC) in an MP4 file, both decided
//!   from `ftyp` the way ExifTool's `WriteMOV` decides. The packet is edited
//!   in place with [`xmp_edit`](crate::writer::xmp_edit); an existing packet
//!   is edited where it is.
//!
//! A tag without a group (or with the `QuickTime` group) is edited in every
//! location that has it, deleted from all of them, and created in ItemList
//! when ItemList does not have it -- what ExifTool does. A container left
//! empty by a deletion (an `ilst`, a `meta` with only a handler, a `udta`)
//! is removed, as ExifTool removes it.
//!
//! Media data is copied byte for byte. When `moov` (or the XMP box) changes
//! size ahead of the media data, every chunk offset (`stco`, `co64`, and the
//! `gps ` table) is relocated. Files whose layout cannot be relocated
//! safely -- movie fragments, `sidx`/`saio` boxes, item locations, a
//! compressed movie header -- are refused with an error, as ExifTool refuses
//! them.

use crate::error::{Error, Result};
use crate::formats::heif_items::{parse_boxes, BoxRef};
use crate::formats::quicktime::{ilst_tag_name, keys_tag_name, qt_text_name, MAC_ROMAN_HIGH};
use crate::writer::xmp_edit::{edit_xmp, XmpChanges, XmpOutcome};
use std::collections::HashSet;
use std::ops::Range;

/// The `uuid` of an XMP box.
pub const XMP_UUID: [u8; 16] = [
    0xBE, 0x7A, 0xCF, 0xCB, 0x97, 0xA9, 0x42, 0xE8, 0x9C, 0x71, 0x99, 0x94, 0x91, 0xE3, 0xAF, 0xAC,
];

/// Where a QuickTime tag lives (family-1 group), or `Any` for all of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QtScope {
    Any,
    ItemList,
    UserData,
    Keys,
}

impl QtScope {
    fn covers(self, loc: QtScope) -> bool {
        self == QtScope::Any || self == loc
    }

    /// The scope a write group names (`QuickTime`, `ItemList`, ...), or
    /// `None` for a group that is not a QuickTime metadata location.
    pub fn from_group(group: &str) -> Option<QtScope> {
        Some(match group.to_ascii_lowercase().as_str() {
            "" | "*" | "all" | "quicktime" => QtScope::Any,
            "itemlist" => QtScope::ItemList,
            "userdata" => QtScope::UserData,
            "keys" => QtScope::Keys,
            _ => return None,
        })
    }
}

/// A tag to write.
#[derive(Debug, Clone)]
pub struct QtSet {
    pub scope: QtScope,
    pub tag: String,
    pub value: String,
}

/// The QuickTime and XMP part of a write request.
#[derive(Debug, Clone, Default)]
pub struct QtChanges {
    pub set: Vec<QtSet>,
    /// (scope, lower-case tag name).
    pub delete: Vec<(QtScope, String)>,
    /// Whole locations to empty (`ItemList:all`; `QuickTime:all` gives
    /// `Any`, all three).
    pub delete_groups: Vec<QtScope>,
    pub xmp: XmpChanges,
}

impl QtChanges {
    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
            && self.delete.is_empty()
            && self.delete_groups.is_empty()
            && self.xmp.is_empty()
    }
}

fn err(msg: impl Into<String>) -> Error {
    Error::InvalidData(format!("QuickTime: {}", msg.into()))
}

// ── tag tables ──────────────────────────────────────────────────────────────

/// ItemList atoms written as UTF-8 text, and the atom ExifTool creates for a
/// tag (QuickTime.pm `%ItemList`; `desc` is `Avoid`, so `\xa9des` is the
/// one created for Description).
const ITEM_LIST_TEXT: &[(&str, &[u8; 4])] = &[
    ("Title", b"\xa9nam"),
    ("Artist", b"\xa9ART"),
    ("Album", b"\xa9alb"),
    ("Comment", b"\xa9cmt"),
    ("Genre", b"\xa9gen"),
    ("Composer", b"\xa9wrt"),
    ("Encoder", b"\xa9too"),
    ("Grouping", b"\xa9grp"),
    ("Lyrics", b"\xa9lyr"),
    ("Description", b"\xa9des"),
    ("AlbumArtist", b"aART"),
    ("Copyright", b"cprt"),
    ("LongDescription", b"ldes"),
    ("TVShow", b"tvsh"),
    ("TVEpisodeID", b"tven"),
];

/// Other ItemList text atoms that are edited when present.
const ITEM_LIST_TEXT_EDIT: &[&[u8; 4]] = &[b"desc"];

/// UserData text atoms, and the one ExifTool creates for a tag
/// (QuickTime.pm `%UserData`; `\xa9wrt` is `Avoid` there, `\xa9com` wins).
const USER_DATA_TEXT: &[(&str, &[u8; 4])] = &[
    ("Title", b"\xa9nam"),
    ("Artist", b"\xa9ART"),
    ("Album", b"\xa9alb"),
    ("Comment", b"\xa9cmt"),
    ("Genre", b"\xa9gen"),
    ("Composer", b"\xa9com"),
    ("Encoder", b"\xa9too"),
    ("Grouping", b"\xa9grp"),
    ("Lyrics", b"\xa9lyr"),
    ("Information", b"\xa9inf"),
    ("Requirements", b"\xa9req"),
    ("Format", b"\xa9fmt"),
    ("Director", b"\xa9dir"),
    ("Producer", b"\xa9prd"),
    ("Performers", b"\xa9prf"),
    ("SourceCredits", b"\xa9src"),
    ("SoftwareVersion", b"\xa9swr"),
    ("Make", b"\xa9mak"),
    ("Model", b"\xa9mod"),
    ("Copyright", b"\xa9cpy"),
];

/// Keys entries written as UTF-8 text (QuickTime.pm `%Keys`).
const KEYS_TEXT: &[(&str, &str)] = &[
    ("Title", "com.apple.quicktime.title"),
    ("Artist", "com.apple.quicktime.artist"),
    ("Album", "com.apple.quicktime.album"),
    ("Comment", "com.apple.quicktime.comment"),
    ("Description", "com.apple.quicktime.description"),
    ("Copyright", "com.apple.quicktime.copyright"),
    ("Author", "com.apple.quicktime.author"),
    ("DisplayName", "com.apple.quicktime.displayname"),
    ("Keywords", "com.apple.quicktime.keywords"),
    ("Make", "com.apple.quicktime.make"),
    ("Model", "com.apple.quicktime.model"),
    ("Software", "com.apple.quicktime.software"),
];

fn lookup<'a, T>(table: &'a [(&str, T)], tag: &str) -> Option<&'a T> {
    table
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(tag))
        .map(|(_, v)| v)
}

/// The name our reader (and ExifTool) gives an ItemList atom.
fn item_list_name(kind: &[u8; 4]) -> &'static str {
    ilst_tag_name(kind).0
}

/// The name of a UserData atom (`\xa9xxx` only).
fn user_data_name(kind: &[u8; 4]) -> &'static str {
    if kind[0] != 0xA9 {
        return "";
    }
    match std::str::from_utf8(&kind[1..]) {
        Ok(k) => qt_text_name(k).0,
        Err(_) => "",
    }
}

fn keys_name(key: &[u8]) -> &'static str {
    std::str::from_utf8(key)
        .ok()
        .and_then(keys_tag_name)
        .map_or("", |(n, _)| n)
}

/// Is this ItemList atom one whose value is text?
fn item_list_is_text(kind: &[u8; 4]) -> bool {
    ITEM_LIST_TEXT.iter().any(|(_, k)| *k == kind) || ITEM_LIST_TEXT_EDIT.contains(&kind)
}

// ── box helpers ─────────────────────────────────────────────────────────────

fn boxed(kind: &[u8; 4], payload: &[u8]) -> Result<Vec<u8>> {
    let size = payload.len() + 8;
    let size = u32::try_from(size).map_err(|_| err("box too large"))?;
    let mut out = Vec::with_capacity(payload.len() + 8);
    out.extend_from_slice(&size.to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(payload);
    Ok(out)
}

fn children(data: &[u8], range: Range<usize>) -> Result<Vec<BoxRef>> {
    parse_boxes(data, range.start, range.end).map_err(err)
}

/// Offset of the children of a `meta` box payload: 0 for the QuickTime
/// form (a plain container), 4 for the ISO full box. Same test as the
/// reader: QuickTime form when `hdlr` (or another known child) comes first.
fn meta_children_offset(payload: &[u8]) -> usize {
    if payload.len() >= 8
        && matches!(
            &payload[4..8],
            b"hdlr" | b"keys" | b"ilst" | b"free" | b"lang" | b"ctry"
        )
    {
        0
    } else {
        4
    }
}

/// A `data` atom holding UTF-8 text (type 1, no country or language).
fn text_data_atom(value: &str) -> Vec<u8> {
    let mut p = Vec::with_capacity(8 + value.len());
    p.extend_from_slice(&[0, 0, 0, 1, 0, 0, 0, 0]);
    p.extend_from_slice(value.as_bytes());
    boxed(b"data", &p).expect("small box")
}

/// The `hdlr` ExifTool writes in a new `meta` (32 bytes, no name).
fn handler(kind: &[u8; 4]) -> Vec<u8> {
    let mut p = vec![0u8; 8];
    p.extend_from_slice(kind);
    p.extend_from_slice(&[0u8; 12]);
    boxed(b"hdlr", &p).expect("small box")
}

/// Encode a UserData text value for its language code.
fn encode_lang_text(value: &str, lang: u16) -> Vec<u8> {
    if lang == 0 || lang >= 0x400 {
        return value.as_bytes().to_vec();
    }
    value
        .chars()
        .map(|c| {
            if (c as u32) < 0x80 {
                c as u8
            } else {
                MAC_ROMAN_HIGH
                    .iter()
                    .position(|&m| m == c)
                    .map_or(b'?', |i| 0x80 + i as u8)
            }
        })
        .collect()
}

fn user_data_text_atom(kind: &[u8; 4], value: &str, lang: u16) -> Result<Vec<u8>> {
    let text = encode_lang_text(value, lang);
    let len = u16::try_from(text.len()).map_err(|_| err("UserData text too long"))?;
    let mut p = Vec::with_capacity(4 + text.len());
    p.extend_from_slice(&len.to_be_bytes());
    p.extend_from_slice(&lang.to_be_bytes());
    p.extend_from_slice(&text);
    boxed(kind, &p)
}

// ── the edit plan ───────────────────────────────────────────────────────────

/// Where XMP is now and what becomes of it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum XmpPlace {
    /// No change.
    Keep,
    Remove,
    Write(Vec<u8>),
}

struct Plan<'a> {
    changes: &'a QtChanges,
    /// Sets created per location (indexes into `changes.set`).
    create_item_list: Vec<usize>,
    create_user_data: Vec<usize>,
    create_keys: Vec<usize>,
    /// XMP in `moov/udta/XMP_`.
    udta_xmp: XmpPlace,
    /// Create `udta/XMP_` (MOV) with this packet.
    udta_xmp_new: Option<Vec<u8>>,
}

impl Plan<'_> {
    fn deletes(&self, loc: QtScope, name: &str) -> bool {
        if name.is_empty() {
            return false;
        }
        self.changes.delete_groups.iter().any(|s| s.covers(loc))
            || self
                .changes
                .delete
                .iter()
                .any(|(s, t)| s.covers(loc) && t.eq_ignore_ascii_case(name))
    }

    fn group_deleted(&self, loc: QtScope) -> bool {
        self.changes.delete_groups.iter().any(|s| s.covers(loc))
    }

    /// The value to write to an existing tag in `loc`.
    fn set_value(&self, loc: QtScope, name: &str) -> Option<&str> {
        if name.is_empty() {
            return None;
        }
        self.changes
            .set
            .iter()
            .rev()
            .find(|s| s.scope.covers(loc) && s.tag.eq_ignore_ascii_case(name))
            .map(|s| s.value.as_str())
    }
}

/// The tags each location already has (lower-case names).
#[derive(Default)]
struct Existing {
    item_list: HashSet<String>,
    user_data: HashSet<String>,
    keys: HashSet<String>,
    udta_xmp: Option<Range<usize>>,
}

fn scan_ilst_names(data: &[u8], ilst: &BoxRef, into: &mut HashSet<String>) -> Result<()> {
    for item in children(data, ilst.payload())? {
        let n = item_list_name(&item.kind);
        if !n.is_empty() {
            into.insert(n.to_ascii_lowercase());
        }
    }
    Ok(())
}

/// Parsed `keys` table: the key strings (namespace stripped) with their
/// namespaces.
struct KeysTable {
    entries: Vec<([u8; 4], Vec<u8>)>,
}

impl KeysTable {
    fn parse(payload: &[u8]) -> Result<Self> {
        if payload.len() < 8 {
            return Err(err("truncated keys box"));
        }
        let count = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]) as usize;
        let mut entries = Vec::new();
        let mut pos = 8;
        for _ in 0..count {
            if pos + 8 > payload.len() {
                return Err(err("truncated keys box"));
            }
            let size = u32::from_be_bytes([
                payload[pos],
                payload[pos + 1],
                payload[pos + 2],
                payload[pos + 3],
            ]) as usize;
            if size < 8 || pos + size > payload.len() {
                return Err(err("invalid keys entry"));
            }
            let mut ns = [0u8; 4];
            ns.copy_from_slice(&payload[pos + 4..pos + 8]);
            entries.push((ns, payload[pos + 8..pos + size].to_vec()));
            pos += size;
        }
        Ok(KeysTable { entries })
    }

    fn serialize(&self, version_flags: &[u8]) -> Vec<u8> {
        let mut p = version_flags.to_vec();
        p.extend_from_slice(&(self.entries.len() as u32).to_be_bytes());
        for (ns, key) in &self.entries {
            p.extend_from_slice(&((key.len() + 8) as u32).to_be_bytes());
            p.extend_from_slice(ns);
            p.extend_from_slice(key);
        }
        boxed(b"keys", &p).expect("small box")
    }
}

fn is_keys_meta(data: &[u8], meta: &BoxRef) -> Result<bool> {
    let p = meta.payload();
    let off = meta_children_offset(&data[p.clone()]);
    Ok(children(data, p.start + off..p.end)?
        .iter()
        .any(|b| &b.kind == b"keys"))
}

fn scan_moov(data: &[u8], moov: &BoxRef) -> Result<Existing> {
    let mut ex = Existing::default();
    for c in children(data, moov.payload())? {
        match &c.kind {
            b"udta" => {
                for u in children(data, c.payload())? {
                    match &u.kind {
                        b"meta" => {
                            let p = u.payload();
                            let off = meta_children_offset(&data[p.clone()]);
                            for m in children(data, p.start + off..p.end)? {
                                if &m.kind == b"ilst" {
                                    scan_ilst_names(data, &m, &mut ex.item_list)?;
                                }
                            }
                        }
                        b"XMP_" => ex.udta_xmp = Some(u.payload()),
                        k => {
                            let n = user_data_name(k);
                            if !n.is_empty() {
                                ex.user_data.insert(n.to_ascii_lowercase());
                            }
                        }
                    }
                }
            }
            b"meta" => {
                let p = c.payload();
                let off = meta_children_offset(&data[p.clone()]);
                let kids = children(data, p.start + off..p.end)?;
                if let Some(k) = kids.iter().find(|b| &b.kind == b"keys") {
                    let table = KeysTable::parse(&data[k.payload()])?;
                    for (_, key) in &table.entries {
                        let n = keys_name(key);
                        if !n.is_empty() {
                            ex.keys.insert(n.to_ascii_lowercase());
                        }
                    }
                } else {
                    for m in kids.iter().filter(|b| &b.kind == b"ilst") {
                        scan_ilst_names(data, m, &mut ex.item_list)?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(ex)
}

/// ExifTool's MOV/MP4 decision (`WriteMOV`): MP4 when the file starts with
/// `ftyp` and no brand after the major one is `qt  `.
fn is_mov_layout(data: &[u8], top: &[BoxRef]) -> bool {
    match top.first() {
        Some(b) if &b.kind == b"ftyp" => {
            let p = &data[b.payload()];
            p.chunks_exact(4).skip(1).any(|c| c == b"qt  ")
        }
        _ => true,
    }
}

// ── rewriting ───────────────────────────────────────────────────────────────

/// Rewrite an ItemList `ilst` payload. Returns the new payload.
fn rewrite_item_list(data: &[u8], ilst: &BoxRef, plan: &Plan, create: bool) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for item in children(data, ilst.payload())? {
        let name = item_list_name(&item.kind);
        if plan.group_deleted(QtScope::ItemList) || plan.deletes(QtScope::ItemList, name) {
            continue;
        }
        if item_list_is_text(&item.kind) {
            if let Some(v) = plan.set_value(QtScope::ItemList, name) {
                out.extend(boxed(&item.kind, &text_data_atom(v))?);
                continue;
            }
        }
        out.extend_from_slice(&data[item.start..item.end]);
    }
    if create {
        for &i in &plan.create_item_list {
            let s = &plan.changes.set[i];
            if let Some(kind) = lookup(ITEM_LIST_TEXT, &s.tag) {
                out.extend(boxed(kind, &text_data_atom(&s.value))?);
            }
        }
    }
    Ok(out)
}

/// A new ItemList `meta` (ISO full box, `mdir` handler) holding `ilst`.
fn new_item_list_meta(ilst_payload: &[u8]) -> Result<Vec<u8>> {
    let mut p = vec![0u8; 4];
    p.extend(handler(b"mdir"));
    p.extend(boxed(b"ilst", ilst_payload)?);
    boxed(b"meta", &p)
}

/// Boxes ExifTool counts as an empty `meta` (WriteQuickTime.pl `%emptyMeta`).
fn only_empty_meta_boxes(body: &[u8]) -> bool {
    match parse_boxes(body, 0, body.len()) {
        Ok(kids) => kids
            .iter()
            .all(|b| matches!(&b.kind, b"hdlr" | b"keys" | b"lang" | b"ctry" | b"free")),
        Err(_) => false,
    }
}

/// Rewrite a `meta` holding an ItemList. `None` when it is left empty.
fn rewrite_item_list_meta(
    data: &[u8],
    meta: &BoxRef,
    plan: &Plan,
    create: bool,
) -> Result<Option<Vec<u8>>> {
    let p = meta.payload();
    let off = meta_children_offset(&data[p.clone()]);
    let mut body = Vec::new();
    let mut saw_ilst = false;
    let mut removed = false;
    for c in children(data, p.start + off..p.end)? {
        if &c.kind == b"ilst" && !saw_ilst {
            saw_ilst = true;
            let new = rewrite_item_list(data, &c, plan, create)?;
            if new.is_empty() {
                removed = true;
            } else if new.as_slice() == &data[c.payload()] {
                body.extend_from_slice(&data[c.start..c.end]);
            } else {
                body.extend(boxed(b"ilst", &new)?);
            }
        } else {
            body.extend_from_slice(&data[c.start..c.end]);
        }
    }
    if !saw_ilst && create && !plan.create_item_list.is_empty() {
        let fake = BoxRef {
            kind: *b"ilst",
            start: 0,
            header_len: 0,
            end: 0,
            to_eof: false,
        };
        let new = rewrite_item_list(&[], &fake, plan, true)?;
        if !new.is_empty() {
            body.extend(boxed(b"ilst", &new)?);
        }
    }
    if removed && only_empty_meta_boxes(&body) {
        return Ok(None);
    }
    let mut payload = data[p.start..p.start + off].to_vec();
    payload.extend(body);
    if payload.as_slice() == &data[p.clone()] {
        return Ok(Some(data[meta.start..meta.end].to_vec()));
    }
    Ok(Some(boxed(b"meta", &payload)?))
}

/// Rewrite the Keys `meta`. `None` when no key is left.
fn rewrite_keys_meta(data: &[u8], meta: &BoxRef, plan: &Plan) -> Result<Option<Vec<u8>>> {
    let p = meta.payload();
    let off = meta_children_offset(&data[p.clone()]);
    let kids = children(data, p.start + off..p.end)?;
    let keys_box = kids
        .iter()
        .find(|b| &b.kind == b"keys")
        .ok_or_else(|| err("Keys meta without keys"))?;
    let kp = keys_box.payload();
    let mut table = KeysTable::parse(&data[kp.clone()])?;
    let version_flags = data[kp.start..kp.start + 4].to_vec();

    // Old index (1-based) -> new index, or 0 when deleted.
    let mut remap = Vec::with_capacity(table.entries.len() + 1);
    remap.push(0u32);
    let mut kept = Vec::new();
    for (ns, key) in table.entries.drain(..) {
        let name = keys_name(&key);
        if plan.group_deleted(QtScope::Keys) || plan.deletes(QtScope::Keys, name) {
            remap.push(0);
        } else {
            kept.push((ns, key));
            remap.push(kept.len() as u32);
        }
    }
    table.entries = kept;
    // Keys to create, with their new index.
    let mut created: Vec<(u32, &str)> = Vec::new();
    for &i in &plan.create_keys {
        let s = &plan.changes.set[i];
        if let Some(key) = lookup(KEYS_TEXT, &s.tag) {
            table.entries.push((*b"mdta", key.as_bytes().to_vec()));
            created.push((table.entries.len() as u32, s.value.as_str()));
        }
    }

    let mut body = Vec::new();
    for c in &kids {
        match &c.kind {
            b"keys" => body.extend(table.serialize(&version_flags)),
            b"ilst" => {
                let mut items = Vec::new();
                for item in children(data, c.payload())? {
                    let old = u32::from_be_bytes(item.kind) as usize;
                    let Some(&new) = remap.get(old).filter(|_| old > 0) else {
                        // An item for no key: kept as it is (the reader
                        // ignores it as ExifTool does).
                        items.extend_from_slice(&data[item.start..item.end]);
                        continue;
                    };
                    if new == 0 {
                        continue;
                    }
                    let key = &table.entries[new as usize - 1].1;
                    let name = keys_name(key);
                    let idx = new.to_be_bytes();
                    match plan.set_value(QtScope::Keys, name) {
                        Some(v) if lookup(KEYS_TEXT, name).is_some() => {
                            items.extend(boxed(&idx, &text_data_atom(v))?)
                        }
                        _ => {
                            let mut b = data[item.start..item.end].to_vec();
                            b[4..8].copy_from_slice(&idx);
                            items.extend(b);
                        }
                    }
                }
                for (idx, v) in &created {
                    items.extend(boxed(&idx.to_be_bytes(), &text_data_atom(v))?);
                }
                if !items.is_empty() {
                    body.extend(boxed(b"ilst", &items)?);
                }
            }
            _ => body.extend_from_slice(&data[c.start..c.end]),
        }
    }
    if !kids.iter().any(|b| &b.kind == b"ilst") && !created.is_empty() {
        let mut items = Vec::new();
        for (idx, v) in &created {
            items.extend(boxed(&idx.to_be_bytes(), &text_data_atom(v))?);
        }
        body.extend(boxed(b"ilst", &items)?);
    }
    if table.entries.is_empty() {
        return Ok(None);
    }
    let mut payload = data[p.start..p.start + off].to_vec();
    payload.extend(body);
    if payload.as_slice() == &data[p.clone()] {
        return Ok(Some(data[meta.start..meta.end].to_vec()));
    }
    Ok(Some(boxed(b"meta", &payload)?))
}

/// A new Keys `meta` (QuickTime form, `mdta` handler), as ExifTool writes it.
fn new_keys_meta(plan: &Plan) -> Result<Option<Vec<u8>>> {
    let mut table = KeysTable {
        entries: Vec::new(),
    };
    let mut items = Vec::new();
    for &i in &plan.create_keys {
        let s = &plan.changes.set[i];
        if let Some(key) = lookup(KEYS_TEXT, &s.tag) {
            table.entries.push((*b"mdta", key.as_bytes().to_vec()));
            let idx = (table.entries.len() as u32).to_be_bytes();
            items.extend(boxed(&idx, &text_data_atom(&s.value))?);
        }
    }
    if table.entries.is_empty() {
        return Ok(None);
    }
    let mut p = handler(b"mdta");
    p.extend(table.serialize(&[0, 0, 0, 0]));
    p.extend(boxed(b"ilst", &items)?);
    Ok(Some(boxed(b"meta", &p)?))
}

/// UserData atoms to create.
fn new_user_data_atoms(plan: &Plan) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for &i in &plan.create_user_data {
        let s = &plan.changes.set[i];
        if let Some(kind) = lookup(USER_DATA_TEXT, &s.tag) {
            out.extend(user_data_text_atom(kind, &s.value, 0)?);
        }
    }
    Ok(out)
}

/// Rewrite `udta`. `None` when a deletion left it empty.
fn rewrite_udta(data: &[u8], udta: &BoxRef, plan: &Plan) -> Result<Option<Vec<u8>>> {
    let kids = children(data, udta.payload())?;
    let mut body = Vec::new();
    let mut saw_item_list = false;
    for c in &kids {
        match &c.kind {
            b"meta" if !saw_item_list => {
                saw_item_list = true;
                if let Some(m) = rewrite_item_list_meta(data, c, plan, true)? {
                    body.extend(m);
                }
            }
            b"XMP_" => match &plan.udta_xmp {
                XmpPlace::Keep => body.extend_from_slice(&data[c.start..c.end]),
                XmpPlace::Remove => {}
                XmpPlace::Write(x) => body.extend(boxed(b"XMP_", x)?),
            },
            k if k[0] == 0xA9 => {
                let name = user_data_name(k);
                if plan.group_deleted(QtScope::UserData) || plan.deletes(QtScope::UserData, name) {
                    continue;
                }
                match plan.set_value(QtScope::UserData, name) {
                    Some(v) => {
                        let p = &data[c.payload()];
                        let lang = if p.len() >= 4 {
                            u16::from_be_bytes([p[2], p[3]])
                        } else {
                            0
                        };
                        body.extend(user_data_text_atom(k, v, lang)?);
                    }
                    None => body.extend_from_slice(&data[c.start..c.end]),
                }
            }
            _ => body.extend_from_slice(&data[c.start..c.end]),
        }
    }
    if let Some(x) = &plan.udta_xmp_new {
        body.extend(boxed(b"XMP_", x)?);
    }
    if !saw_item_list && !plan.create_item_list.is_empty() {
        let fake = BoxRef {
            kind: *b"ilst",
            start: 0,
            header_len: 0,
            end: 0,
            to_eof: false,
        };
        let ilst = rewrite_item_list(&[], &fake, plan, true)?;
        if !ilst.is_empty() {
            body.extend(new_item_list_meta(&ilst)?);
        }
    }
    body.extend(new_user_data_atoms(plan)?);
    if body.is_empty() && !kids.is_empty() {
        return Ok(None);
    }
    if body.as_slice() == &data[udta.payload()] {
        return Ok(Some(data[udta.start..udta.end].to_vec()));
    }
    Ok(Some(boxed(b"udta", &body)?))
}

/// A new `udta` for what the plan creates, if anything.
fn new_udta(plan: &Plan) -> Result<Option<Vec<u8>>> {
    let empty = BoxRef {
        kind: *b"udta",
        start: 0,
        header_len: 0,
        end: 0,
        to_eof: false,
    };
    match rewrite_udta(&[], &empty, plan)? {
        Some(b) if b.len() > 8 => Ok(Some(b)),
        _ => Ok(None),
    }
}

fn rewrite_moov(data: &[u8], moov: &BoxRef, plan: &Plan) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    let mut saw_udta = false;
    let mut saw_keys = false;
    for c in children(data, moov.payload())? {
        match &c.kind {
            b"cmov" => return Err(err("can't yet write compressed movie metadata")),
            b"udta" if !saw_udta => {
                saw_udta = true;
                if let Some(u) = rewrite_udta(data, &c, plan)? {
                    body.extend(u);
                }
            }
            b"meta" => {
                if is_keys_meta(data, &c)? {
                    if saw_keys {
                        body.extend_from_slice(&data[c.start..c.end]);
                        continue;
                    }
                    saw_keys = true;
                    if let Some(m) = rewrite_keys_meta(data, &c, plan)? {
                        body.extend(m);
                    }
                } else if let Some(m) = rewrite_item_list_meta(data, &c, plan, false)? {
                    body.extend(m);
                }
            }
            _ => body.extend_from_slice(&data[c.start..c.end]),
        }
    }
    if !saw_udta {
        if let Some(u) = new_udta(plan)? {
            body.extend(u);
        }
    }
    if !saw_keys {
        if let Some(m) = new_keys_meta(plan)? {
            body.extend(m);
        }
    }
    boxed(b"moov", &body)
}

/// Map an old file position through the copied runs (old range, new start).
fn map_pos(runs: &[(Range<usize>, usize)], pos: u64) -> Option<u64> {
    runs.iter()
        .find(|(r, _)| pos >= r.start as u64 && pos < r.end as u64)
        .map(|(r, new)| *new as u64 + (pos - r.start as u64))
        .or_else(|| {
            runs.iter()
                .find(|(r, _)| pos == r.end as u64)
                .map(|(r, new)| (*new + r.len()) as u64)
        })
}

/// Relocate the chunk offsets in the new `moov` (at `out[moov]`).
fn patch_offsets(
    out: &mut [u8],
    range: Range<usize>,
    runs: &[(Range<usize>, usize)],
    first_change: u64,
    depth: u32,
) -> Result<()> {
    if depth > 16 {
        return Err(err("moov nests too deep"));
    }
    // An offset into a box copied as it was follows that box. One that
    // points into the metadata boxes being rewritten, or past the end of
    // the file, addresses no media data of this file (a stripped or
    // truncated sample); it is kept as it was, where ExifTool gives up
    // ("Chunk offset ... is outside media data").
    let relocate = |old: u64| -> Result<u64> {
        if old < first_change {
            return Ok(old);
        }
        Ok(map_pos(runs, old).unwrap_or(old))
    };
    let kids = parse_boxes(out, range.start, range.end).map_err(err)?;
    for b in kids {
        let p = b.payload();
        match &b.kind {
            b"trak" | b"mdia" | b"minf" | b"stbl" => {
                patch_offsets(out, p, runs, first_change, depth + 1)?
            }
            b"saio" => return Err(err("can't yet handle saio box when writing")),
            b"stco" | b"co64" | b"gps " => {
                let (w, step, first) = match &b.kind {
                    b"stco" => (4, 4, 8),
                    b"co64" => (8, 8, 8),
                    _ => (4, 8, 8),
                };
                if p.len() < 8 {
                    return Err(err("truncated chunk offset table"));
                }
                let d = &out[p.clone()];
                let mut n = u32::from_be_bytes([d[4], d[5], d[6], d[7]]) as usize;
                if &b.kind == b"gps " {
                    n = n.min((p.len() - 8) / 8);
                }
                if n.checked_mul(step).and_then(|x| x.checked_add(first)) > Some(p.len()) {
                    return Err(err("truncated chunk offset table"));
                }
                for i in 0..n {
                    let at = p.start + first + i * step;
                    let old = if w == 8 {
                        let mut a = [0u8; 8];
                        a.copy_from_slice(&out[at..at + 8]);
                        u64::from_be_bytes(a)
                    } else {
                        u32::from_be_bytes([out[at], out[at + 1], out[at + 2], out[at + 3]]) as u64
                    };
                    let new = relocate(old)?;
                    if w == 8 {
                        out[at..at + 8].copy_from_slice(&new.to_be_bytes());
                    } else {
                        let v = u32::try_from(new)
                            .map_err(|_| err("a chunk offset no longer fits stco"))?;
                        out[at..at + 4].copy_from_slice(&v.to_be_bytes());
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Rewrite a QuickTime-based file with `changes` applied.
pub fn write_mp4(source: &[u8], changes: &QtChanges) -> Result<Vec<u8>> {
    if changes.is_empty() {
        return Ok(source.to_vec());
    }
    let top = parse_boxes(source, 0, source.len()).map_err(err)?;
    let moov_idx = match top.iter().position(|b| &b.kind == b"moov") {
        Some(i) => i,
        None => return Err(err("no moov box")),
    };
    if top.iter().filter(|b| &b.kind == b"moov").count() > 1 {
        return Err(err("more than one moov box"));
    }
    let moov = &top[moov_idx];
    let is_mov = is_mov_layout(source, &top);
    let existing = scan_moov(source, moov)?;

    // XMP: edit the packet where it is; create it where ExifTool does.
    let top_xmp = top
        .iter()
        .position(|b| &b.kind == b"uuid" && source[b.payload()].starts_with(&XMP_UUID));
    let current: Option<&[u8]> = match (top_xmp, &existing.udta_xmp) {
        (Some(i), _) => Some(&source[top[i].payload().start + 16..top[i].end]),
        (None, Some(r)) => Some(&source[r.clone()]),
        (None, None) => None,
    };
    let xmp_outcome = if changes.xmp.is_empty() {
        XmpOutcome::Unchanged
    } else {
        edit_xmp(current, &changes.xmp)?
    };
    let xmp_place = match &xmp_outcome {
        XmpOutcome::Unchanged => XmpPlace::Keep,
        XmpOutcome::Removed => XmpPlace::Remove,
        XmpOutcome::Rewritten(x) => XmpPlace::Write(x.clone()),
    };
    let (mut udta_xmp, mut udta_xmp_new, mut top_xmp_place, mut top_xmp_new) =
        (XmpPlace::Keep, None, XmpPlace::Keep, None);
    match (top_xmp, &existing.udta_xmp) {
        (Some(_), _) => top_xmp_place = xmp_place,
        (None, Some(_)) => udta_xmp = xmp_place,
        (None, None) => {
            if let XmpPlace::Write(x) = xmp_place {
                if is_mov {
                    udta_xmp_new = Some(x);
                } else {
                    top_xmp_new = Some(x);
                }
            }
        }
    }

    // Where new tags go.
    let mut plan = Plan {
        changes,
        create_item_list: Vec::new(),
        create_user_data: Vec::new(),
        create_keys: Vec::new(),
        udta_xmp,
        udta_xmp_new,
    };
    for (i, s) in changes.set.iter().enumerate() {
        // The last request for a tag in a scope wins.
        if changes.set[i + 1..]
            .iter()
            .any(|t| t.scope == s.scope && t.tag.eq_ignore_ascii_case(&s.tag))
        {
            continue;
        }
        let lc = s.tag.to_ascii_lowercase();
        let in_item_list = lookup(ITEM_LIST_TEXT, &s.tag).is_some();
        let in_user_data = lookup(USER_DATA_TEXT, &s.tag).is_some();
        match s.scope {
            QtScope::ItemList if in_item_list && !existing.item_list.contains(&lc) => {
                plan.create_item_list.push(i)
            }
            QtScope::UserData if in_user_data && !existing.user_data.contains(&lc) => {
                plan.create_user_data.push(i)
            }
            QtScope::Keys
                if lookup(KEYS_TEXT, &s.tag).is_some() && !existing.keys.contains(&lc) =>
            {
                plan.create_keys.push(i)
            }
            QtScope::Any => {
                if in_item_list {
                    if !existing.item_list.contains(&lc) {
                        plan.create_item_list.push(i);
                    }
                } else if in_user_data
                    && !existing.user_data.contains(&lc)
                    && !existing.keys.contains(&lc)
                {
                    plan.create_user_data.push(i);
                }
            }
            _ => {}
        }
    }

    let new_moov = rewrite_moov(source, moov, &plan)?;
    let moov_changed = new_moov.as_slice() != &source[moov.start..moov.end];

    // Assemble the file, recording where every copied box went.
    let mut out = Vec::with_capacity(source.len() + 4096);
    let mut runs: Vec<(Range<usize>, usize)> = Vec::new();
    let mut first_change: Option<usize> = None;
    let mut moov_at = 0..0;
    for (i, b) in top.iter().enumerate() {
        if i == moov_idx {
            if moov_changed {
                first_change.get_or_insert(b.start);
            } else {
                runs.push((b.start..b.end, out.len()));
            }
            let at = out.len();
            out.extend_from_slice(&new_moov);
            moov_at = at..out.len();
            if let Some(x) = &top_xmp_new {
                first_change.get_or_insert(b.end);
                let mut p = XMP_UUID.to_vec();
                p.extend_from_slice(x);
                out.extend(boxed(b"uuid", &p)?);
            }
        } else if Some(i) == top_xmp && top_xmp_place != XmpPlace::Keep {
            first_change.get_or_insert(b.start);
            if let XmpPlace::Write(x) = &top_xmp_place {
                let mut p = XMP_UUID.to_vec();
                p.extend_from_slice(x);
                out.extend(boxed(b"uuid", &p)?);
            }
        } else {
            runs.push((b.start..b.end, out.len()));
            out.extend_from_slice(&source[b.start..b.end]);
        }
    }
    let Some(first_change) = first_change else {
        return Ok(out);
    };

    // Anything that addresses the file by absolute offset past the first
    // change must be relocated; refuse the layouts that cannot be.
    let shifted = runs
        .iter()
        .any(|(r, new)| r.start >= first_change && *new != r.start);
    if shifted {
        for b in &top {
            if b.start < first_change {
                continue;
            }
            match &b.kind {
                b"moof" | b"mfra" => {
                    return Err(err("can't yet handle movie fragments when writing"))
                }
                b"sidx" => return Err(err("can't yet handle sidx box when writing")),
                _ => {}
            }
        }
        for b in &top {
            if &b.kind == b"meta" {
                let p = b.payload();
                let off = meta_children_offset(&source[p.clone()]);
                if children(source, p.start + off..p.end)?
                    .iter()
                    .any(|c| &c.kind == b"iloc")
                {
                    return Err(err("can't relocate the item locations of this file"));
                }
            }
        }
    }
    let moov_payload = moov_at.start + 8..moov_at.end;
    patch_offsets(&mut out, moov_payload, &runs, first_change as u64, 0)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bx(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        boxed(kind, payload).unwrap()
    }

    /// ftyp + moov(trak/stbl/stco -> mdat, udta/meta/ilst) + mdat.
    fn sample(brand: &[u8; 4], with_udta: bool) -> Vec<u8> {
        let ftyp = bx(b"ftyp", &[&brand[..], &[0, 0, 0, 0], &brand[..]].concat());
        let build = |mdat_at: u32| -> Vec<u8> {
            let stco = bx(
                b"stco",
                &[
                    &[0u8, 0, 0, 0, 0, 0, 0, 1][..],
                    &(mdat_at + 8).to_be_bytes(),
                ]
                .concat(),
            );
            let stbl = bx(b"stbl", &stco);
            let minf = bx(b"minf", &stbl);
            let mdia = bx(b"mdia", &minf);
            let trak = bx(b"trak", &mdia);
            let mut moov_body = trak;
            if with_udta {
                let nam = bx(b"\xa9nam", &text_data_atom("Old title"));
                let art = bx(b"\xa9ART", &text_data_atom("Old artist"));
                let ilst = bx(b"ilst", &[nam, art].concat());
                let meta = bx(b"meta", &[&[0u8; 4][..], &handler(b"mdir"), &ilst].concat());
                let ud = user_data_text_atom(b"\xa9ART", "Old artist", 0).unwrap();
                moov_body.extend(bx(b"udta", &[meta, ud].concat()));
            }
            bx(b"moov", &moov_body)
        };
        let moov_len = build(0).len() as u32;
        let moov = build(ftyp.len() as u32 + moov_len);
        [ftyp, moov, bx(b"mdat", b"MEDIA-DATA")].concat()
    }

    fn chunk_offset(file: &[u8]) -> u32 {
        let i = file.windows(4).position(|w| w == b"stco").unwrap();
        u32::from_be_bytes(file[i + 12..i + 16].try_into().unwrap())
    }

    fn media_at(file: &[u8]) -> usize {
        file.windows(10).position(|w| w == b"MEDIA-DATA").unwrap()
    }

    fn set(scope: QtScope, tag: &str, value: &str) -> QtChanges {
        QtChanges {
            set: vec![QtSet {
                scope,
                tag: tag.into(),
                value: value.into(),
            }],
            ..Default::default()
        }
    }

    fn read(file: &[u8]) -> Vec<(String, String, String)> {
        crate::formats::quicktime::read_quicktime(file)
            .unwrap()
            .into_iter()
            .map(|t| {
                (
                    t.group.family1.clone(),
                    t.name.clone(),
                    t.print_value.clone(),
                )
            })
            .collect()
    }

    fn values(file: &[u8], name: &str) -> Vec<(String, String)> {
        read(file)
            .into_iter()
            .filter(|(_, n, _)| n == name)
            .map(|(g, _, v)| (g, v))
            .collect()
    }

    #[test]
    fn set_keeps_other_items_and_relocates_chunks() {
        let src = sample(b"mp42", true);
        assert_eq!(chunk_offset(&src) as usize, media_at(&src));
        let out = write_mp4(&src, &set(QtScope::Any, "Title", "A much longer new title")).unwrap();
        assert_eq!(chunk_offset(&out) as usize, media_at(&out));
        assert_eq!(
            values(&out, "Title"),
            vec![("ItemList".into(), "A much longer new title".into())]
        );
        // ExifTool edits Artist in UserData and ItemList alike.
        let out = write_mp4(&src, &set(QtScope::Any, "Artist", "New")).unwrap();
        let mut a = values(&out, "Artist");
        a.sort();
        assert_eq!(
            a,
            vec![
                ("ItemList".into(), "New".into()),
                ("UserData".into(), "New".into())
            ]
        );
        assert_eq!(
            values(&out, "Title"),
            vec![("ItemList".into(), "Old title".into())]
        );
    }

    #[test]
    fn delete_removes_every_instance_and_empty_containers() {
        let src = sample(b"mp42", true);
        let ch = QtChanges {
            delete: vec![
                (QtScope::Any, "artist".into()),
                (QtScope::Any, "title".into()),
            ],
            ..Default::default()
        };
        let out = write_mp4(&src, &ch).unwrap();
        assert!(values(&out, "Artist").is_empty());
        assert!(values(&out, "Title").is_empty());
        // udta, meta and ilst all went away with their last tag.
        assert!(!out.windows(4).any(|w| w == b"udta"));
        assert_eq!(chunk_offset(&out) as usize, media_at(&out));
        // Deleting only from ItemList leaves UserData alone.
        let ch = QtChanges {
            delete: vec![(QtScope::ItemList, "artist".into())],
            ..Default::default()
        };
        let out = write_mp4(&src, &ch).unwrap();
        assert_eq!(
            values(&out, "Artist"),
            vec![("UserData".into(), "Old artist".into())]
        );
    }

    #[test]
    fn creates_item_list_from_scratch() {
        let src = sample(b"mp42", false);
        let out = write_mp4(&src, &set(QtScope::Any, "Album", "Album X")).unwrap();
        assert_eq!(
            values(&out, "Album"),
            vec![("ItemList".into(), "Album X".into())]
        );
        assert_eq!(chunk_offset(&out) as usize, media_at(&out));
        assert!(out.ends_with(b"MEDIA-DATA"));
    }

    #[test]
    fn keys_create_edit_delete_renumbers() {
        let src = sample(b"mp42", false);
        let mut ch = set(QtScope::Keys, "Artist", "K artist");
        ch.set.push(QtSet {
            scope: QtScope::Keys,
            tag: "Album".into(),
            value: "K album".into(),
        });
        let out = write_mp4(&src, &ch).unwrap();
        assert_eq!(
            values(&out, "Artist"),
            vec![("Keys".into(), "K artist".into())]
        );
        assert_eq!(
            values(&out, "Album"),
            vec![("Keys".into(), "K album".into())]
        );
        // Deleting the first key renumbers the second.
        let ch = QtChanges {
            delete: vec![(QtScope::Any, "artist".into())],
            ..Default::default()
        };
        let out2 = write_mp4(&out, &ch).unwrap();
        assert!(values(&out2, "Artist").is_empty());
        assert_eq!(
            values(&out2, "Album"),
            vec![("Keys".into(), "K album".into())]
        );
        // And deleting the last key drops the Keys meta.
        let ch = QtChanges {
            delete_groups: vec![QtScope::Keys],
            ..Default::default()
        };
        let out3 = write_mp4(&out2, &ch).unwrap();
        assert!(!out3.windows(4).any(|w| w == b"keys"));
        assert_eq!(chunk_offset(&out3) as usize, media_at(&out3));
    }

    #[test]
    fn user_data_mac_language_is_macroman() {
        assert_eq!(encode_lang_text("é", 0), "é".as_bytes());
        assert_eq!(encode_lang_text("é", 1), vec![0x8E]);
        assert_eq!(encode_lang_text("é", 0x55c4), "é".as_bytes());
    }

    #[test]
    fn mov_layout_follows_ftyp() {
        let mp4 = sample(b"mp42", false);
        let top = parse_boxes(&mp4, 0, mp4.len()).unwrap();
        assert!(!is_mov_layout(&mp4, &top));
        let mov = sample(b"qt  ", false);
        let top = parse_boxes(&mov, 0, mov.len()).unwrap();
        assert!(is_mov_layout(&mov, &top));
    }

    #[test]
    fn refuses_fragmented_files_when_offsets_move() {
        let mut src = sample(b"mp42", false);
        src.extend(bx(b"moof", &[0u8; 8]));
        assert!(write_mp4(&src, &set(QtScope::Any, "Title", "x")).is_err());
    }
}
