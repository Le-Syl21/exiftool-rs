//! The trailers after the EOI of a JPEG, edited as ExifTool's
//! `ProcessTrailers` (ExifTool.pm) does.
//!
//! ExifTool finds the trailers from the end of the file backwards: each one
//! is identified by its last bytes (`IdentifyTrailer`), its own module gives
//! its length, and the next one ends where it starts. The walk stops at the
//! first block it cannot identify; what lies between the EOI and the first
//! trailer it found (MPF images, unknown data) is copied as it is.
//!
//! What is edited, as each trailer's module does it:
//!
//! - **AFCP** (AFCP.pm): the IPTC entry, and the absolute offsets of the
//!   directory, rewritten for where the trailer ends up.
//! - **FotoStation** (FotoStation.pm): the IPTC record.
//! - **MIE** (MIE.pm): the `EXIF`, `IPTC` and `XMP` directories of the
//!   `Meta` group, edited where they are and created when a request writes
//!   tags of their family (ExifTool writes the MIE trailer with
//!   `InitWriteDirs(\%mieMap, 'MIE')`, so a new EXIF, IPTC or XMP directory
//!   goes into MIE-Meta as well).
//! - **Samsung** (Samsung.pm): the absolute offsets of the embedded audio
//!   in the QDIO block, rewritten for where the trailer ends up.
//!
//! The others (PhotoMechanic, whose records are PhotoMechanic soft-edit
//! tags only, CanonVRD, Vivo, Insta360, NikonApp) are copied.

use crate::error::{Error, Result};
use crate::writer::exif_edit::{self, ExifChanges, ExifOutcome};
use crate::writer::iptc_writer;
use crate::writer::photoshop_irb::IptcChanges;
use crate::writer::xmp_edit::{self, XmpChanges, XmpOutcome};

/// The edits that may reach a trailer.
pub struct TrailerEdits<'a> {
    pub exif: &'a ExifChanges,
    pub iptc: &'a IptcChanges,
    pub xmp: &'a XmpChanges,
}

impl TrailerEdits<'_> {
    fn is_empty(&self) -> bool {
        self.exif.is_empty() && self.iptc.is_empty() && self.xmp.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Afcp,
    FotoStation,
    PhotoMechanic,
    CanonVrd,
    Mie,
    Samsung,
    Insta360,
    NikonApp,
    Vivo,
}

#[derive(Debug, Clone)]
struct Trailer {
    kind: Kind,
    start: usize,
    end: usize,
    /// Samsung: where the audio offsets of the QDIO block are, relative to
    /// the trailer start, the audio data's offset relative to the trailer
    /// start, and its size.
    samsung_audio: Option<(usize, usize, u32)>,
}

fn be32(d: &[u8], p: usize) -> u32 {
    u32::from_be_bytes([d[p], d[p + 1], d[p + 2], d[p + 3]])
}

fn le32(d: &[u8], p: usize) -> u32 {
    u32::from_le_bytes([d[p], d[p + 1], d[p + 2], d[p + 3]])
}

/// Where the trailers may start: past the first EOI after the start of
/// scan (Writer.pl `WriteJPEG`, `TrailerStart`).
fn trailer_start(jpeg: &[u8]) -> Option<usize> {
    let mut pos = 2;
    while pos + 4 <= jpeg.len() {
        if jpeg[pos] != 0xFF {
            return None;
        }
        let marker = jpeg[pos + 1];
        if marker == 0xFF {
            pos += 1;
            continue;
        }
        if marker == 0x00 || marker == 0x01 || (0xD0..=0xD8).contains(&marker) {
            pos += 2;
            continue;
        }
        if marker == 0xD9 {
            return None;
        }
        let len = u16::from_be_bytes([jpeg[pos + 2], jpeg[pos + 3]]) as usize;
        if len < 2 {
            return None;
        }
        pos += 2 + len;
        if marker == 0xDA {
            let scan = jpeg.get(pos..)?;
            return scan
                .windows(2)
                .position(|w| w == [0xFF, 0xD9])
                .map(|i| pos + i + 2);
        }
    }
    None
}

/// `IdentifyTrailer`: the kind of trailer ending at `end`, from up to 64
/// bytes before it. `None` also for the trailers whose extent ExifTool
/// takes to be everything back to the EOI when writing (OnePlus, Google):
/// the walk stops there and the rest is copied.
fn identify(d: &[u8], end: usize) -> Option<Kind> {
    let b = &d[end.saturating_sub(64)..end];
    let n = b.len();
    let mie_term = |at: usize, len_bytes: usize| {
        // `~\0\x04\0zmie` then the extended terminator of the group.
        let sig = 8 + 4 + len_bytes + 2;
        n >= sig && {
            let s = &b[n - sig..];
            s.starts_with(b"~\0\x04\0zmie~\0\0")
                && s[11] == at as u8
                && matches!(s[sig - 2], 0x10 | 0x18)
                && s[sig - 1] == len_bytes as u8
        }
    };
    if n >= 12 && &b[n - 12..n - 9] == b"AXS" && matches!(b[n - 9], b'!' | b'*') {
        Some(Kind::Afcp)
    } else if b.ends_with(&[0xA1, 0xB2, 0xC3, 0xD4]) {
        Some(Kind::FotoStation)
    } else if b.ends_with(b"cbipcbbl") {
        Some(Kind::PhotoMechanic)
    } else if n == 64 && b.starts_with(b"CANON OPTIONAL DATA\0") {
        Some(Kind::CanonVrd)
    } else if mie_term(0x06, 4) || mie_term(0x0a, 8) {
        Some(Kind::Mie)
    } else if b.ends_with(b"\0\0QDIOBS") || b.ends_with(b"\0\0SEFT") {
        Some(Kind::Samsung)
    } else if b.ends_with(b"8db42d694ccc418790edff439fe026bf") {
        Some(Kind::Insta360)
    } else if b.ends_with(b"\0\0\0\0\0\0/NIKON APP") {
        Some(Kind::NikonApp)
    } else if b.ends_with(b"\xff\xff\xff\xff\x1b*9HWfu\x84\x93\xa2\xb1") {
        Some(Kind::Vivo)
    } else {
        None
    }
}

/// Where the trailer of kind `kind` ending at `end` starts, as its module
/// finds it (`ts` is where trailers may start).
fn extent(d: &[u8], ts: usize, end: usize, kind: Kind) -> Option<Trailer> {
    let mut t = Trailer {
        kind,
        start: 0,
        end,
        samsung_audio: None,
    };
    t.start = match kind {
        Kind::Afcp => {
            let e = end.checked_sub(12)?;
            let hdr = &d[e..e + 4];
            let big = hdr[3] == b'!';
            let rec = if big { be32(d, e + 4) } else { le32(d, e + 4) } as usize;
            if rec >= ts && rec + 12 <= e && &d[rec..rec + 4] == hdr {
                rec
            } else {
                // AFCP.pm `ScanForTrailer`: the first header after the EOI.
                ts + d[ts..e].windows(4).position(|w| w == hdr)?
            }
        }
        Kind::FotoStation => {
            let mut p = end;
            while p >= ts + 10 {
                let f = &d[p - 10..p];
                let size = be32(f, 2) as usize;
                if f[6..10] != [0xA1, 0xB2, 0xC3, 0xD4] || size < 10 || size > p - ts {
                    break;
                }
                p -= size;
            }
            (p < end).then_some(p)?
        }
        Kind::PhotoMechanic => {
            let size = be32(d, end - 12);
            if size & 0x8000_0000 != 0 {
                return None;
            }
            (end - 12).checked_sub(size as usize)?
        }
        Kind::CanonVrd => {
            let size = be32(d, end - 64 + 0x14) as usize;
            let s = end.checked_sub(64 + 0x1c)?.checked_sub(size)?;
            d[s..].starts_with(b"CANON OPTIONAL DATA\0").then_some(s)?
        }
        Kind::Mie => {
            let mut p = end;
            let mut start = None;
            while p >= 10 {
                let b = &d[p - 10..p];
                let be = b[8] == 0x10;
                let len = if b.starts_with(b"~\0\0\x06") && matches!(b[8], 0x10 | 0x18) && b[9] == 4
                {
                    if be { be32(b, 4) } else { le32(b, 4) }.into()
                } else if matches!(b[8], 0x10 | 0x18) && b[9] == 8 {
                    let w: [u8; 8] = b[..8].try_into().ok()?;
                    if be {
                        u64::from_be_bytes(w)
                    } else {
                        u64::from_le_bytes(w)
                    }
                } else {
                    break;
                };
                let Ok(len) = usize::try_from(len) else {
                    break;
                };
                if len < 12 || len > p {
                    break;
                }
                if b[9] == 8 && (len < 14 || &d[p - 14..p - 10] != b"~\0\0\x0a") {
                    break;
                }
                p -= len;
                start = Some(p);
            }
            start.filter(|&s| s >= ts)?
        }
        Kind::Samsung => {
            let (s, audio) = samsung_extent(d, end)?;
            t.samsung_audio = audio;
            s
        }
        Kind::Insta360 => {
            let len = le32(d, end.checked_sub(78)? + 38) as usize;
            end.checked_sub(len)?
        }
        Kind::NikonApp => {
            let len = be32(d, end.checked_sub(20)?) as usize;
            end.checked_sub(len)?
        }
        Kind::Vivo => {
            // Trailer.pm `ProcessVivo`: from the first `streamdata` or
            // `vivo{"` after the EOI.
            let area = &d[ts..end];
            let at = (0..area.len()).find(|&i| {
                area[i..].starts_with(b"streamdata") || area[i..].starts_with(b"vivo{\"")
            })?;
            ts + at
        }
    };
    (t.start >= ts && t.start < end).then_some(t)
}

/// Samsung.pm `ProcessSamsung`: the start of the trailer ending at `end`,
/// and where its audio offsets are (see [`Trailer::samsung_audio`]).
#[allow(clippy::type_complexity)]
fn samsung_extent(d: &[u8], end: usize) -> Option<(usize, Option<(usize, usize, u32)>)> {
    let mut block_end = if d[..end].ends_with(b"QDIOBS") {
        end - 2
    } else {
        end
    };
    let mut qdio_field: Option<usize> = None;
    loop {
        if block_end < 8 {
            return None;
        }
        let h = &d[block_end - 8..block_end];
        let typ = &h[4..8];
        if !typ.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_') {
            return None;
        }
        let len = le32(h, 0) as usize;
        if !(4..0x10000).contains(&len) || len + 8 >= block_end {
            return None;
        }
        let data_at = block_end - 8 - len;
        block_end = data_at;
        if typ != b"SEFT" {
            if typ == b"QDIO" && len == 20 {
                qdio_field = Some(data_at + 12);
            }
            continue;
        }
        let dir = &d[data_at..data_at + len];
        if !dir.starts_with(b"SEFH") || len < 12 {
            return None;
        }
        let count = le32(dir, 8) as usize;
        if 12 + 12 * count > len {
            return None;
        }
        let dir_pos = data_at;
        let mut first = 0usize;
        let mut audio = None;
        for i in 0..count {
            let e = 12 + 12 * i;
            let typ = u16::from_le_bytes([dir[e + 2], dir[e + 3]]);
            let noff = le32(dir, e + 4) as usize;
            let size = le32(dir, e + 8) as usize;
            if noff > dir_pos || size > noff || size < 8 {
                return None;
            }
            first = first.max(noff);
            if typ == 0x0100 && audio.is_none() {
                let at = dir_pos - noff;
                let name_len = le32(d, at + 4) as usize;
                let a_noff = noff.checked_sub(8 + name_len)?;
                let a_size = size.checked_sub(8 + name_len)?;
                audio = Some((a_noff, a_size));
            }
        }
        let start = dir_pos - first;
        let audio = match (qdio_field, audio) {
            (Some(f), Some((a_noff, a_size))) => {
                Some((f - start, dir_pos - a_noff - start, a_size as u32))
            }
            _ => None,
        };
        return Some((start, audio));
    }
}

/// The trailers of a JPEG, in file order, and where trailers may start.
fn chain(d: &[u8]) -> Option<(usize, Vec<Trailer>)> {
    let ts = trailer_start(d)?;
    let mut out = Vec::new();
    let mut end = d.len();
    while end > ts {
        let Some(t) = identify(d, end).and_then(|k| extent(d, ts, end, k)) else {
            break;
        };
        end = t.start;
        out.push(t);
    }
    out.reverse();
    Some((ts, out))
}

/// Apply the edits to the trailers of `jpeg` (the file with its segments
/// already rewritten), and fix the offsets of the trailers that moved.
pub fn rewrite(jpeg: Vec<u8>, edits: &TrailerEdits) -> Result<Vec<u8>> {
    let Some((_, trailers)) = chain(&jpeg) else {
        return Ok(jpeg);
    };
    let Some(first) = trailers.first() else {
        return Ok(jpeg);
    };
    let mut out = jpeg[..first.start].to_vec();
    let mut changed = false;
    for t in &trailers {
        let old = &jpeg[t.start..t.end];
        let pos = out.len();
        let new = match t.kind {
            Kind::Afcp => edit_afcp(&jpeg, t, pos, edits.iptc)?,
            Kind::FotoStation => edit_fotostation(old, edits.iptc)?,
            Kind::Mie if !edits.is_empty() => edit_mie(old, edits)?,
            Kind::Samsung => t.samsung_audio.and_then(|(field, audio, size)| {
                let start = u32::try_from(pos + audio).ok()?;
                let mut b = old.to_vec();
                b[field..field + 4].copy_from_slice(&start.to_le_bytes());
                b[field + 4..field + 8].copy_from_slice(&(start + size).to_le_bytes());
                (b != old).then_some(b)
            }),
            _ => None,
        };
        match new {
            Some(b) => {
                changed = true;
                out.extend_from_slice(&b);
            }
            None => out.extend_from_slice(old),
        }
    }
    Ok(if changed { out } else { jpeg })
}

// ── AFCP ────────────────────────────────────────────────────────────────────

/// AFCP.pm `ProcessAFCP` in write mode: the trailer `t` of `d`, written at
/// `pos` in the output, with the IPTC edits applied. `None` when it stays
/// as it is.
fn edit_afcp(d: &[u8], t: &Trailer, pos: usize, iptc: &IptcChanges) -> Result<Option<Vec<u8>>> {
    let start = t.start;
    let e = t.end - 12;
    let big = d[start + 3] == b'!';
    let rd16 = |p: usize| {
        let w = [d[p], d[p + 1]];
        if big {
            u16::from_be_bytes(w)
        } else {
            u16::from_le_bytes(w)
        }
    };
    let rd32 = |p: usize| if big { be32(d, p) } else { le32(d, p) };
    let put32 = |v: u32| {
        if big {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
    };
    let put16 = |v: u16| {
        if big {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
    };
    let recorded = rd32(e + 4) as i64;
    // How far the trailer moved since its offsets were written.
    let fix = start as i64 - recorded;
    let count = rd16(start + 6) as usize;
    let dir_end = start + 12 + 12 * count;
    let mut entries = Vec::with_capacity(count);
    let mut valid = dir_end <= e;
    for i in 0..count {
        if !valid {
            break;
        }
        let p = start + 12 + 12 * i;
        let size = rd32(p + 4) as i64;
        let off = rd32(p + 8) as i64 + fix;
        if off < dir_end as i64 || off + size > e as i64 {
            valid = false;
            break;
        }
        let tag: [u8; 4] = d[p..p + 4].try_into().expect("4 bytes");
        entries.push((tag, off as usize..(off + size) as usize));
    }
    let edits_iptc = !iptc.is_empty() && entries.iter().any(|(t, _)| t == b"IPTC");
    if !valid {
        if !iptc.is_empty() {
            return Err(Error::InvalidData("bad AFCP directory".into()));
        }
        return Ok(None);
    }
    if !edits_iptc && fix == 0 && pos == start {
        return Ok(None);
    }
    let val_pos = pos + 12 + 12 * entries.len();
    let mut dir = Vec::with_capacity(12 * entries.len());
    let mut vals = Vec::new();
    for (tag, range) in &entries {
        let data = &d[range.clone()];
        let new_data;
        let data = if tag == b"IPTC" && !iptc.is_empty() {
            let old = if iptc.delete_all { &[][..] } else { data };
            new_data = iptc_writer::edit_iptc(old, &iptc.tags)?;
            &new_data[..]
        } else {
            data
        };
        let at = u32::try_from(val_pos + vals.len())
            .map_err(|_| Error::InvalidData("AFCP offset out of range".into()))?;
        dir.extend_from_slice(tag);
        dir.extend_from_slice(&put32(data.len() as u32));
        dir.extend_from_slice(&put32(at));
        vals.extend_from_slice(data);
    }
    let pos32 =
        u32::try_from(pos).map_err(|_| Error::InvalidData("AFCP offset out of range".into()))?;
    let hdr = &d[start..start + 4];
    let mut out = Vec::with_capacity(t.end - t.start);
    out.extend_from_slice(hdr);
    out.extend_from_slice(&d[start + 4..start + 6]); // version
    out.extend_from_slice(&put16(entries.len() as u16));
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&dir);
    out.extend_from_slice(&vals);
    out.extend_from_slice(hdr);
    out.extend_from_slice(&put32(pos32));
    out.extend_from_slice(&[0; 4]);
    Ok(Some(out))
}

// ── FotoStation ─────────────────────────────────────────────────────────────

/// FotoStation.pm `ProcessFotoStation` in write mode: records read back from
/// the end, each followed by a footer `<tag:u16> <size:u32, footer
/// included> A1B2C3D4`; tag 1 is IPTC. ExifTool keeps an IPTC record even
/// when it is left empty.
fn edit_fotostation(t: &[u8], iptc: &IptcChanges) -> Result<Option<Vec<u8>>> {
    if iptc.is_empty() {
        return Ok(None);
    }
    let mut end = t.len();
    let mut records: Vec<(u16, std::ops::Range<usize>)> = Vec::new();
    while end >= 10 {
        let f = &t[end - 10..end];
        let tag = u16::from_be_bytes([f[0], f[1]]);
        let size = be32(f, 2) as usize;
        if f[6..10] != [0xA1, 0xB2, 0xC3, 0xD4] || size < 10 || size > end {
            break;
        }
        records.push((tag, end - size..end - 10));
        end -= size;
    }
    if !records.iter().any(|(t, _)| *t == 1) {
        return Ok(None);
    }
    let mut out = t[..end].to_vec();
    for (tag, range) in records.iter().rev() {
        let old = &t[range.clone()];
        let new;
        let data = if *tag == 1 {
            new = iptc_writer::edit_iptc(if iptc.delete_all { &[] } else { old }, &iptc.tags)?;
            &new[..]
        } else {
            old
        };
        out.extend_from_slice(data);
        out.extend_from_slice(&tag.to_be_bytes());
        out.extend_from_slice(&((data.len() + 10) as u32).to_be_bytes());
        out.extend_from_slice(&[0xA1, 0xB2, 0xC3, 0xD4]);
    }
    Ok((out != t).then_some(out))
}

// ── MIE ─────────────────────────────────────────────────────────────────────

/// One MIE element (MIE.pm): `~ <format> <tagLen> <valLen> <tag>
/// [<extended valLen>] <value>`. An uncompressed group (format 0x10 or
/// 0x18) has no value: its elements follow, up to a terminator element
/// (tagLen 0).
#[derive(Debug)]
struct Elem {
    start: usize,
    /// Start of the value, or of the group's elements.
    body: usize,
    end: usize,
    format: u8,
    tag: Vec<u8>,
    /// The elements and the terminator of a group.
    group: Option<(Vec<Elem>, std::ops::Range<usize>)>,
}

fn mie_err(why: &str) -> Error {
    Error::InvalidData(format!("bad MIE trailer: {why}"))
}

/// The element header at `pos`: format, tag length, value length and the
/// position past the header. The extended value length is in `be` order.
fn mie_header(d: &[u8], pos: usize, be: bool) -> Result<(u8, usize, usize, usize)> {
    if pos + 4 > d.len() || d[pos] != b'~' {
        return Err(mie_err("sync byte expected"));
    }
    let format = d[pos + 1];
    let tag_len = d[pos + 2] as usize;
    let vl = d[pos + 3] as usize;
    let mut p = pos + 4 + tag_len;
    let len = if vl > 252 {
        let n = 1usize << (256 - vl);
        let b = d.get(p..p + n).ok_or_else(|| mie_err("truncated"))?;
        p += n;
        let mut v: u64 = 0;
        for i in 0..n {
            let byte = if be { b[i] } else { b[n - 1 - i] };
            v = (v << 8) | u64::from(byte);
        }
        usize::try_from(v).map_err(|_| mie_err("element too large"))?
    } else {
        vl
    };
    if p > d.len() {
        return Err(mie_err("truncated"));
    }
    Ok((format, tag_len, len, p))
}

fn is_group(format: u8) -> bool {
    format == 0x10 || format == 0x18
}

fn parse_elem(d: &[u8], pos: usize, be: bool, depth: usize) -> Result<Elem> {
    let (format, tag_len, len, body) = mie_header(d, pos, be)?;
    let tag = d[pos + 4..pos + 4 + tag_len].to_vec();
    if is_group(format) {
        if depth > 16 {
            return Err(mie_err("groups nested too deep"));
        }
        let (kids, term) = parse_group(d, body, format & 0x08 == 0, depth + 1)?;
        Ok(Elem {
            start: pos,
            body,
            end: term.end,
            format,
            tag,
            group: Some((kids, term)),
        })
    } else {
        let end = body
            .checked_add(len)
            .filter(|&e| e <= d.len())
            .ok_or_else(|| mie_err("truncated"))?;
        Ok(Elem {
            start: pos,
            body,
            end,
            format,
            tag,
            group: None,
        })
    }
}

/// The elements of a group starting at `pos`, and its terminator.
fn parse_group(
    d: &[u8],
    mut pos: usize,
    be: bool,
    depth: usize,
) -> Result<(Vec<Elem>, std::ops::Range<usize>)> {
    let mut kids = Vec::new();
    loop {
        if pos + 4 > d.len() {
            return Err(mie_err("missing group terminator"));
        }
        if d[pos + 2] == 0 {
            let (_, _, len, body) = mie_header(d, pos, be)?;
            let end = body
                .checked_add(len)
                .filter(|&e| e <= d.len())
                .ok_or_else(|| mie_err("truncated"))?;
            return Ok((kids, pos..end));
        }
        let e = parse_elem(d, pos, be, depth)?;
        pos = e.end;
        kids.push(e);
    }
}

/// A new MIE element with an `undef` (format 0) value.
fn mie_element(tag: &[u8], value: &[u8], be: bool) -> Vec<u8> {
    let mut out = vec![b'~', 0x00, tag.len() as u8];
    let len = value.len();
    let ext: Vec<u8> = if len < 253 {
        out.push(len as u8);
        Vec::new()
    } else if len < 65536 {
        out.push(255);
        if be {
            (len as u16).to_be_bytes().to_vec()
        } else {
            (len as u16).to_le_bytes().to_vec()
        }
    } else {
        out.push(254);
        if be {
            (len as u32).to_be_bytes().to_vec()
        } else {
            (len as u32).to_le_bytes().to_vec()
        }
    };
    out.extend_from_slice(tag);
    out.extend_from_slice(&ext);
    out.extend_from_slice(value);
    out
}

/// What the edits make of the `EXIF`, `IPTC` or `XMP` directory of MIE-Meta
/// (`old`: its current value, `None` when there is none).
fn edit_mie_dir(tag: &[u8], old: Option<&[u8]>, edits: &TrailerEdits) -> Result<XmpOutcome> {
    Ok(match tag {
        b"EXIF" => match exif_edit::edit_exif(old, edits.exif)? {
            ExifOutcome::Unchanged => XmpOutcome::Unchanged,
            ExifOutcome::Removed => XmpOutcome::Removed,
            ExifOutcome::Rewritten(b) => XmpOutcome::Rewritten(b),
        },
        b"IPTC" => {
            let iptc = edits.iptc;
            if iptc.is_empty() || (old.is_none() && !iptc.creates()) {
                XmpOutcome::Unchanged
            } else {
                let base = if iptc.delete_all {
                    &[][..]
                } else {
                    old.unwrap_or_default()
                };
                let new = iptc_writer::edit_iptc(base, &iptc.tags)?;
                if new.is_empty() {
                    if old.is_some() {
                        XmpOutcome::Removed
                    } else {
                        XmpOutcome::Unchanged
                    }
                } else if Some(&new[..]) == old {
                    XmpOutcome::Unchanged
                } else {
                    XmpOutcome::Rewritten(new)
                }
            }
        }
        b"XMP" => xmp_edit::edit_xmp(old, edits.xmp)?,
        _ => XmpOutcome::Unchanged,
    })
}

/// The elements of MIE-Meta with the edits applied (`kids`: the existing
/// ones). `None` when nothing changes.
fn edit_meta(d: &[u8], kids: &[Elem], be: bool, edits: &TrailerEdits) -> Result<Option<Vec<u8>>> {
    // MIE.pm `WriteMIEGroup`: the directories being edited, in sort order,
    // are merged with the (sorted) existing elements.
    let mut pending: Vec<&[u8]> = vec![b"EXIF", b"IPTC", b"XMP"];
    let mut out = Vec::new();
    let mut changed = false;
    let create = |tag: &[u8], out: &mut Vec<u8>, changed: &mut bool| -> Result<()> {
        if let XmpOutcome::Rewritten(b) = edit_mie_dir(tag, None, edits)? {
            out.extend_from_slice(&mie_element(tag, &b, be));
            *changed = true;
        }
        Ok(())
    };
    for k in kids {
        while let Some(&p) = pending.first() {
            if p >= &k.tag[..] {
                break;
            }
            create(p, &mut out, &mut changed)?;
            pending.remove(0);
        }
        let raw = &d[k.start..k.end];
        let Some(i) = pending.iter().position(|p| *p == &k.tag[..]) else {
            out.extend_from_slice(raw);
            continue;
        };
        let tag = pending.remove(i);
        if k.format & 0xf3 == 0x10 {
            // A group or compressed group by this name: not a directory
            // ExifTool edits ("not expected type").
            out.extend_from_slice(raw);
            continue;
        }
        let old = &d[k.body..k.end];
        let outcome = edit_mie_dir(tag, Some(old), edits)?;
        if k.format & 0x04 != 0 && outcome != XmpOutcome::Unchanged {
            return Err(Error::UnsupportedFileType(
                "editing a compressed MIE directory is not supported".into(),
            ));
        }
        match outcome {
            XmpOutcome::Unchanged => out.extend_from_slice(raw),
            XmpOutcome::Removed => changed = true,
            XmpOutcome::Rewritten(b) => {
                out.extend_from_slice(&mie_element(tag, &b, be));
                changed = true;
            }
        }
    }
    for p in pending {
        create(p, &mut out, &mut changed)?;
    }
    Ok(changed.then_some(out))
}

/// MIE.pm `ProcessMIE` in write mode, on a MIE trailer: the first document
/// with the edits applied to its `Meta` group (created when a new directory
/// needs it). `None` when nothing changes.
fn edit_mie(t: &[u8], edits: &TrailerEdits) -> Result<Option<Vec<u8>>> {
    if t.len() < 8 || !matches!(t[1], 0x10 | 0x18) || &t[2..3] != b"\x04" || &t[4..8] != b"0MIE" {
        return Err(mie_err("no MIE document"));
    }
    let be = t[1] == 0x10;
    let doc = parse_elem(t, 0, be, 0)?;
    let (kids, _) = doc.group.as_ref().expect("a group");
    let mut body = Vec::new();
    let mut changed = false;
    let meta = kids
        .iter()
        .position(|k| k.tag == b"Meta" && is_group(k.format));
    match meta {
        Some(i) => {
            let m = &kids[i];
            let (mkids, mterm) = m.group.as_ref().expect("a group");
            let mbe = m.format & 0x08 == 0;
            for (j, k) in kids.iter().enumerate() {
                if j != i {
                    body.extend_from_slice(&t[k.start..k.end]);
                    continue;
                }
                match edit_meta(t, mkids, mbe, edits)? {
                    Some(new) => {
                        changed = true;
                        body.extend_from_slice(&t[m.start..m.body]);
                        body.extend_from_slice(&new);
                        body.extend_from_slice(&t[mterm.clone()]);
                    }
                    None => body.extend_from_slice(&t[k.start..k.end]),
                }
            }
        }
        None => {
            let new = edit_meta(t, &[], be, edits)?;
            let mut placed = false;
            for k in kids {
                if let (Some(n), false) = (&new, placed) {
                    if &k.tag[..] > b"Meta".as_slice() {
                        push_meta(&mut body, n, be);
                        placed = true;
                    }
                }
                body.extend_from_slice(&t[k.start..k.end]);
            }
            if let Some(n) = &new {
                if !placed {
                    push_meta(&mut body, n, be);
                }
                changed = true;
            }
        }
    }
    if !changed {
        return Ok(None);
    }
    let mut out = t[..doc.body].to_vec();
    out.extend_from_slice(&body);
    // The extended terminator of the document gives its length.
    let len = u32::try_from(out.len() + 10).map_err(|_| mie_err("too large"))?;
    out.extend_from_slice(b"~\0\0\x06");
    out.extend_from_slice(&if be {
        len.to_be_bytes()
    } else {
        len.to_le_bytes()
    });
    out.push(if be { 0x10 } else { 0x18 });
    out.push(0x04);
    out.extend_from_slice(&t[doc.end..]);
    Ok(Some(out))
}

/// A new `Meta` group holding `kids`.
fn push_meta(out: &mut Vec<u8>, kids: &[u8], be: bool) {
    out.extend_from_slice(&[b'~', if be { 0x10 } else { 0x18 }, 4, 0]);
    out.extend_from_slice(b"Meta");
    out.extend_from_slice(kids);
    out.extend_from_slice(b"~\0\0\0");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer::iptc_writer::{check_value, lookup_tag, IptcRequest};

    fn iptc_set(tag: &str, v: &str) -> IptcChanges {
        let tag = lookup_tag(tag).unwrap();
        IptcChanges {
            delete_all: false,
            tags: vec![IptcRequest {
                tag,
                values: vec![check_value(tag, v).unwrap()],
            }],
        }
    }

    /// A minimal JPEG followed by `trailers`.
    fn jpeg_with(trailers: &[&[u8]]) -> Vec<u8> {
        let mut out = vec![0xFF, 0xD8, 0xFF, 0xDA, 0x00, 0x02, 0x11, 0x22, 0xFF, 0xD9];
        for t in trailers {
            out.extend_from_slice(t);
        }
        out
    }

    fn fotostation(iptc: &[u8]) -> Vec<u8> {
        let mut t = iptc.to_vec();
        t.extend_from_slice(&1u16.to_be_bytes());
        t.extend_from_slice(&((iptc.len() + 10) as u32).to_be_bytes());
        t.extend_from_slice(&[0xA1, 0xB2, 0xC3, 0xD4]);
        t
    }

    fn photomechanic() -> Vec<u8> {
        let mut t = b"\x1c\x02\xdd\x00\x04\x00\x00\x00\x01".to_vec();
        t.extend_from_slice(&(t.len() as u32).to_be_bytes());
        t.extend_from_slice(b"cbipcbbl");
        t
    }

    /// The MIE trailer of t/images/ExifTool.jpg: Meta/Document/Copyright.
    fn mie() -> Vec<u8> {
        let mut t = b"~\x10\x04\xfe0MIE\0\0\0\0~\x10\x04\0Meta~\x10\x08\0Document".to_vec();
        t.extend_from_slice(b"~\x28\x09\x13Copyright\xc2\xa9 2006 Phil Harvey");
        t.extend_from_slice(b"~\0\0\0~\0\0\0~\0\x04\0zmie");
        let len = (t.len() + 10) as u32;
        t.extend_from_slice(b"~\0\0\x06");
        t.extend_from_slice(&len.to_be_bytes());
        t.extend_from_slice(b"\x10\x04");
        t
    }

    #[test]
    fn walks_the_chain_from_the_end() {
        let fs = fotostation(b"\x1c\x02\x00\x00\x02\x00\x02");
        let src = jpeg_with(&[&fs, &photomechanic(), &mie()]);
        let (ts, chain) = chain(&src).unwrap();
        assert_eq!(ts, 10);
        let kinds: Vec<Kind> = chain.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, [Kind::FotoStation, Kind::PhotoMechanic, Kind::Mie]);
        assert_eq!(chain[0].start, 10);
        assert_eq!(chain[2].end, src.len());
    }

    /// Perl: `exiftool -IPTC:By-line=Set` on t/images/ExifTool.jpg edits the
    /// FotoStation IPTC record although other trailers follow it, and adds
    /// a MIE-Meta IPTC directory holding the new tag and
    /// ApplicationRecordVersion 4.
    #[test]
    fn iptc_reaches_a_fotostation_record_before_other_trailers_and_mie() {
        let fs = fotostation(b"\x1c\x02\x00\x00\x02\x00\x02");
        let src = jpeg_with(&[&fs, &photomechanic(), &mie()]);
        let iptc = iptc_set("By-line", "Set");
        let edits = TrailerEdits {
            exif: &ExifChanges::default(),
            iptc: &iptc,
            xmp: &XmpChanges::default(),
        };
        let out = rewrite(src, &edits).unwrap();
        let (_, chain) = chain(&out).unwrap();
        assert_eq!(chain.len(), 3);
        let fs = &out[chain[0].start..chain[0].end];
        assert_eq!(
            &fs[..15],
            b"\x1c\x02\x00\x00\x02\x00\x02\x1c\x02\x50\x00\x03Set"
        );
        let m = &out[chain[2].start..chain[2].end];
        let doc = parse_elem(m, 0, true, 0).unwrap();
        let meta = &doc.group.as_ref().unwrap().0[0];
        let tags: Vec<&[u8]> = meta
            .group
            .as_ref()
            .unwrap()
            .0
            .iter()
            .map(|e| &e.tag[..])
            .collect();
        assert_eq!(tags, [&b"Document"[..], b"IPTC"]);
        let iptc_el = &meta.group.as_ref().unwrap().0[1];
        assert_eq!(
            &m[iptc_el.body..iptc_el.end],
            b"\x1c\x02\x50\x00\x03Set\x1c\x02\x00\x00\x02\x00\x04"
        );
        // The extended terminator carries the new length.
        assert_eq!(be32(m, m.len() - 6) as usize, m.len());
    }

    #[test]
    fn iptc_deletion_empties_the_fotostation_record() {
        let fs = fotostation(b"\x1c\x02\x00\x00\x02\x00\x02");
        let src = jpeg_with(&[&fs, &mie()]);
        let iptc = IptcChanges {
            delete_all: true,
            tags: Vec::new(),
        };
        let edits = TrailerEdits {
            exif: &ExifChanges::default(),
            iptc: &iptc,
            xmp: &XmpChanges::default(),
        };
        let out = rewrite(src.clone(), &edits).unwrap();
        let (_, chain) = chain(&out).unwrap();
        // An empty IPTC record stays (10-byte footer only); MIE unchanged.
        assert_eq!(chain[0].end - chain[0].start, 10);
        assert_eq!(&out[chain[1].start..], &mie()[..]);
    }
}
