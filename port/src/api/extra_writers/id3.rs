//! ID3 tag writer for MP3 and AIFF files.
//!
//! ExifTool reads ID3 but does not write it, so there is no reference
//! writer; this one edits the tags in place and keeps everything it is not
//! asked to change:
//!
//! - **ID3v2** (at the start of an MP3, in the `ID3 ` chunk of an AIFF):
//!   the frames are walked in the tag's own version (2.2, 2.3 or 2.4) and
//!   copied byte for byte, except the frames of a tag being written or
//!   deleted. A written text frame replaces every frame of that tag; a tag
//!   the file does not have is added. A file without ID3v2 gets an ID3v2.3
//!   tag. The new tag keeps the old tag's size (with padding) when it fits,
//!   so the audio does not move.
//! - **ID3v1** (the 128-byte `TAG` trailer of an MP3): edited when present
//!   (Title, Artist, Album, Year, Comment, Track, Genre), never created.
//!   Text is Latin-1, cut to the field's size, as every tagger does; a genre
//!   with no ID3v1 number is cleared there. A field cannot be deleted from
//!   ID3v1, only blanked (see [`edit_id3v1`]).
//!
//! The audio frames are copied unchanged. An unsynchronised ID3v2 tag is
//! refused (its frames cannot be walked without decoding it).

use crate::api::error::{Error, Result};

/// Which ID3 tag a request is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Id3Scope {
    Any,
    V1,
    V2,
}

impl Id3Scope {
    fn covers(self, s: Id3Scope) -> bool {
        self == Id3Scope::Any || self == s
    }

    /// The scope a write group names (`ID3`, `ID3v1`, `ID3v2_3`, ...).
    pub fn from_group(group: &str) -> Option<Id3Scope> {
        let g = group.to_ascii_lowercase();
        Some(match g.as_str() {
            "" | "*" | "all" | "id3" => Id3Scope::Any,
            "id3v1" => Id3Scope::V1,
            _ if g.starts_with("id3v2") => Id3Scope::V2,
            _ => return None,
        })
    }
}

/// The ID3 part of a write request.
#[derive(Debug, Clone, Default)]
pub struct Id3Changes {
    /// (scope, tag name, value).
    pub set: Vec<(Id3Scope, String, String)>,
    /// (scope, lower-case tag name).
    pub delete: Vec<(Id3Scope, String)>,
    /// Whole tags to remove.
    pub delete_all: Vec<Id3Scope>,
}

impl Id3Changes {
    pub fn is_empty(&self) -> bool {
        self.set.is_empty() && self.delete.is_empty() && self.delete_all.is_empty()
    }

    fn removes(&self, scope: Id3Scope) -> bool {
        self.delete_all.iter().any(|s| s.covers(scope))
    }

    fn deletes(&self, scope: Id3Scope, name: &str) -> bool {
        self.delete
            .iter()
            .any(|(s, t)| s.covers(scope) && t.eq_ignore_ascii_case(name))
    }

    /// The last value written to `name` in `scope`.
    fn value(&self, scope: Id3Scope, name: &str) -> Option<&str> {
        self.set
            .iter()
            .rev()
            .find(|(s, t, _)| s.covers(scope) && t.eq_ignore_ascii_case(name))
            .map(|(_, _, v)| v.as_str())
    }

    fn touches(&self, scope: Id3Scope) -> bool {
        self.removes(scope)
            || self.delete.iter().any(|(s, _)| s.covers(scope))
            || self.set.iter().any(|(s, _, _)| s.covers(scope))
    }
}

fn err(msg: impl Into<String>) -> Error {
    Error::InvalidData(format!("ID3: {}", msg.into()))
}

// ── ID3v2 ───────────────────────────────────────────────────────────────────

/// How a frame's value is laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameKind {
    Text,
    /// Comment-like: encoding, language, description, text.
    Comment,
}

/// Writable frames: tag name, kind, and the frame IDs of ID3v2.2, 2.3 and
/// 2.4 (ID3.pm `%ID3v2_2`, `%ID3v2_3`, `%ID3v2_4`). Further IDs a reader
/// gives the same name are listed after the third (`TDRC` is read as Year
/// in an ID3v2.3 tag too).
const FRAMES: &[(&str, FrameKind, &[&str])] = &[
    ("Title", FrameKind::Text, &["TT2", "TIT2", "TIT2"]),
    ("Artist", FrameKind::Text, &["TP1", "TPE1", "TPE1"]),
    ("Album", FrameKind::Text, &["TAL", "TALB", "TALB"]),
    ("Track", FrameKind::Text, &["TRK", "TRCK", "TRCK"]),
    (
        "Year",
        FrameKind::Text,
        &["TYE", "TYER", "TDRC", "TDRC", "TYER"],
    ),
    ("Genre", FrameKind::Text, &["TCO", "TCON", "TCON"]),
    ("Composer", FrameKind::Text, &["TCM", "TCOM", "TCOM"]),
    ("EncodedBy", FrameKind::Text, &["TEN", "TENC", "TENC"]),
    ("Grouping", FrameKind::Text, &["TT1", "TIT1", "TIT1"]),
    ("Band", FrameKind::Text, &["TP2", "TPE2", "TPE2"]),
    ("PartOfSet", FrameKind::Text, &["TPA", "TPOS", "TPOS"]),
    ("Compilation", FrameKind::Text, &["TCP", "TCMP", "TCMP"]),
    ("Subtitle", FrameKind::Text, &["TT3", "TIT3", "TIT3"]),
    ("Conductor", FrameKind::Text, &["TP3", "TPE3", "TPE3"]),
    ("Publisher", FrameKind::Text, &["TPB", "TPUB", "TPUB"]),
    ("Copyright", FrameKind::Text, &["TCR", "TCOP", "TCOP"]),
    ("BeatsPerMinute", FrameKind::Text, &["TBP", "TBPM", "TBPM"]),
    ("Comment", FrameKind::Comment, &["COM", "COMM", "COMM"]),
    ("Lyrics", FrameKind::Comment, &["ULT", "USLT", "USLT"]),
];

fn frame_for_name(
    name: &str,
) -> Option<&'static (&'static str, FrameKind, &'static [&'static str])> {
    FRAMES.iter().find(|(n, _, _)| n.eq_ignore_ascii_case(name))
}

fn name_for_frame(
    id: &[u8],
) -> Option<&'static (&'static str, FrameKind, &'static [&'static str])> {
    let id = std::str::from_utf8(id).ok()?;
    FRAMES.iter().find(|(_, _, ids)| ids.contains(&id))
}

/// Index into a FRAMES entry's IDs for a tag version.
fn id_index(version: u8) -> usize {
    match version {
        2 => 0,
        3 => 1,
        _ => 2,
    }
}

fn syncsafe(n: usize) -> Result<[u8; 4]> {
    if n >= 1 << 28 {
        return Err(err("tag too large"));
    }
    Ok([
        ((n >> 21) & 0x7F) as u8,
        ((n >> 14) & 0x7F) as u8,
        ((n >> 7) & 0x7F) as u8,
        (n & 0x7F) as u8,
    ])
}

fn read_syncsafe(b: &[u8]) -> usize {
    ((b[0] as usize & 0x7F) << 21)
        | ((b[1] as usize & 0x7F) << 14)
        | ((b[2] as usize & 0x7F) << 7)
        | (b[3] as usize & 0x7F)
}

/// Encode a string for a frame: (encoding byte, bytes). Latin-1 when it
/// fits, otherwise UTF-16 with a BOM (UTF-8 in ID3v2.4).
fn encode_text(value: &str, version: u8) -> (u8, Vec<u8>) {
    if value.chars().all(|c| (c as u32) < 0x100) {
        return (0, value.chars().map(|c| c as u8).collect());
    }
    if version >= 4 {
        return (3, value.as_bytes().to_vec());
    }
    let mut b = vec![0xFF, 0xFE];
    for u in value.encode_utf16() {
        b.extend_from_slice(&u.to_le_bytes());
    }
    (1, b)
}

/// The written form of a value: ID3 stores Compilation as 1/0.
fn stored_value<'a>(name: &str, value: &'a str) -> &'a str {
    if name.eq_ignore_ascii_case("Compilation") {
        if value.eq_ignore_ascii_case("yes") {
            return "1";
        }
        if value.eq_ignore_ascii_case("no") {
            return "0";
        }
    }
    value
}

fn frame_body(kind: FrameKind, value: &str, version: u8, lang: [u8; 3]) -> Vec<u8> {
    let (enc, text) = encode_text(value, version);
    let mut body = vec![enc];
    if kind == FrameKind::Comment {
        body.extend_from_slice(&lang);
        // An empty description, terminated.
        match enc {
            1 => body.extend_from_slice(&[0xFF, 0xFE, 0, 0]),
            2 => body.extend_from_slice(&[0, 0]),
            _ => body.push(0),
        }
    }
    body.extend(text);
    body
}

fn frame(id: &str, body: &[u8], version: u8) -> Result<Vec<u8>> {
    let mut f = Vec::with_capacity(body.len() + 10);
    f.extend_from_slice(id.as_bytes());
    match version {
        2 => {
            if body.len() >= 1 << 24 {
                return Err(err("frame too large"));
            }
            let n = body.len();
            f.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8, n as u8]);
        }
        3 => {
            let n = u32::try_from(body.len()).map_err(|_| err("frame too large"))?;
            f.extend_from_slice(&n.to_be_bytes());
            f.extend_from_slice(&[0, 0]);
        }
        _ => {
            f.extend_from_slice(&syncsafe(body.len())?);
            f.extend_from_slice(&[0, 0]);
        }
    }
    f.extend_from_slice(body);
    Ok(f)
}

/// Does a comment-like frame have an empty description?
fn comment_without_description(body: &[u8]) -> bool {
    if body.len() < 5 {
        return true;
    }
    let rest = &body[4..];
    match body[0] {
        1 => {
            rest.starts_with(&[0, 0])
                || rest.starts_with(&[0xFF, 0xFE, 0, 0])
                || rest.starts_with(&[0xFE, 0xFF, 0, 0])
        }
        2 => rest.starts_with(&[0, 0]),
        _ => rest.first() == Some(&0),
    }
}

/// Edit an ID3v2 tag (`None`: the file has none). Returns the new tag, or
/// `None` when it is removed or left without frames.
pub fn edit_id3v2(tag: Option<&[u8]>, changes: &Id3Changes) -> Result<Option<Vec<u8>>> {
    let scope = Id3Scope::V2;
    if changes.removes(scope) {
        return Ok(None);
    }
    let (version, revision, flags, old_total, frames_range) = match tag {
        Some(t) => {
            if t.len() < 10 || !t.starts_with(b"ID3") {
                return Err(err("invalid ID3v2 header"));
            }
            let version = t[3];
            if !(2..=4).contains(&version) {
                return Err(err(format!("can't write ID3v2.{version}")));
            }
            let flags = t[5];
            if flags & 0x80 != 0 {
                return Err(err("can't write an unsynchronised ID3v2 tag"));
            }
            if version == 2 && flags & 0x40 != 0 {
                return Err(err("can't write a compressed ID3v2.2 tag"));
            }
            let size = read_syncsafe(&t[6..10]);
            let end = (10 + size).min(t.len());
            let mut start = 10;
            if version >= 3 && flags & 0x40 != 0 {
                // The extended header (CRC, restrictions) is dropped: it
                // would no longer match.
                if t.len() < 14 {
                    return Err(err("truncated extended header"));
                }
                let n = if version == 4 {
                    read_syncsafe(&t[10..14])
                } else {
                    u32::from_be_bytes([t[10], t[11], t[12], t[13]]) as usize + 4
                };
                start += n;
            }
            if start > end {
                return Err(err("truncated ID3v2 tag"));
            }
            (version, t[4], flags & !0x50, 10 + size, start..end)
        }
        None => (3, 0, 0, 0, 0..0),
    };
    let src = tag.unwrap_or(&[]);

    let header_len = if version == 2 { 6 } else { 10 };
    let mut out_frames = Vec::new();
    let mut written: Vec<&str> = Vec::new();
    let mut pos = frames_range.start;
    while pos + header_len <= frames_range.end {
        let id = &src[pos..pos + if version == 2 { 3 } else { 4 }];
        if id[0] == 0 {
            break; // padding
        }
        let size = match version {
            2 => {
                (src[pos + 3] as usize) << 16 | (src[pos + 4] as usize) << 8 | src[pos + 5] as usize
            }
            3 => u32::from_be_bytes([src[pos + 4], src[pos + 5], src[pos + 6], src[pos + 7]])
                as usize,
            _ => read_syncsafe(&src[pos + 4..pos + 8]),
        };
        let body_start = pos + header_len;
        let end = body_start + size;
        if end > frames_range.end {
            return Err(err("truncated ID3v2 frame"));
        }
        let raw = &src[pos..end];
        pos = end;
        let Some(&(name, kind, ids)) = name_for_frame(id) else {
            out_frames.extend_from_slice(raw);
            continue;
        };
        if changes.deletes(scope, name) {
            continue;
        }
        let body = &src[body_start..end];
        // Comments with a description ("(iTunNORM) ...") are other tags.
        let replaceable = kind == FrameKind::Text || comment_without_description(body);
        let Some(value) = changes.value(scope, name).filter(|_| replaceable) else {
            out_frames.extend_from_slice(raw);
            continue;
        };
        if written.contains(&name) {
            continue; // one frame per written tag
        }
        written.push(name);
        let lang = if kind == FrameKind::Comment && body.len() >= 4 {
            [body[1], body[2], body[3]]
        } else {
            *b"eng"
        };
        let v = stored_value(name, value);
        let id = ids[id_index(version)];
        out_frames.extend(frame(id, &frame_body(kind, v, version, lang), version)?);
    }
    // Tags the file did not have.
    for (s, tag_name, value) in &changes.set {
        if !s.covers(scope) {
            continue;
        }
        let Some(&(name, kind, ids)) = frame_for_name(tag_name) else {
            continue;
        };
        if written.contains(&name) {
            continue;
        }
        written.push(name);
        let value = changes.value(scope, name).unwrap_or(value);
        let v = stored_value(name, value);
        out_frames.extend(frame(
            ids[id_index(version)],
            &frame_body(kind, v, version, *b"eng"),
            version,
        )?);
    }
    if out_frames.is_empty() {
        return Ok(None);
    }
    // Keep the old size (padding) when the frames still fit.
    let mut size = out_frames.len();
    if old_total >= 10 && size <= old_total - 10 {
        size = old_total - 10;
    }
    let mut out = Vec::with_capacity(size + 10);
    out.extend_from_slice(b"ID3");
    out.extend_from_slice(&[version, revision, flags]);
    out.extend_from_slice(&syncsafe(size)?);
    out.extend_from_slice(&out_frames);
    out.resize(size + 10, 0);
    Ok(Some(out))
}

// ── ID3v1 ───────────────────────────────────────────────────────────────────

fn latin1_field(value: &str, len: usize) -> Vec<u8> {
    let mut b: Vec<u8> = value
        .chars()
        .map(|c| if (c as u32) < 0x100 { c as u8 } else { b'?' })
        .take(len)
        .collect();
    b.resize(len, 0);
    b
}

/// Edit a 128-byte ID3v1 trailer. `None` when it is removed.
///
/// ID3v1 fields have a fixed place, so a field cannot be deleted, only
/// blanked (the genre becomes 255, "None"); ExifTool still lists a blank
/// field, with an empty value. A trailer left with nothing in it is
/// dropped.
pub fn edit_id3v1(trailer: &[u8], changes: &Id3Changes) -> Option<Vec<u8>> {
    let scope = Id3Scope::V1;
    if changes.removes(scope) {
        return None;
    }
    let mut t = trailer.to_vec();
    let v11 = t[125] == 0 && t[126] != 0;
    for (name, range) in [
        ("Title", 3..33),
        ("Artist", 33..63),
        ("Album", 63..93),
        ("Year", 93..97),
    ] {
        if changes.deletes(scope, name) {
            t[range.clone()].fill(0);
        }
        if let Some(v) = changes.value(scope, name) {
            let len = range.len();
            t[range].copy_from_slice(&latin1_field(v, len));
        }
    }
    let comment_len = if v11 { 28 } else { 30 };
    if changes.deletes(scope, "Comment") {
        t[97..97 + comment_len].fill(0);
    }
    if let Some(v) = changes.value(scope, "Comment") {
        t[97..97 + comment_len].copy_from_slice(&latin1_field(v, comment_len));
    }
    if changes.deletes(scope, "Track") && v11 {
        t[126] = 0;
    }
    if let Some(v) = changes.value(scope, "Track") {
        // ID3v1.1: the track number takes the last two comment bytes.
        let n = v.split('/').next().unwrap_or("").trim().parse::<u8>().ok();
        if let Some(n) = n.filter(|&n| n > 0) {
            t[125] = 0;
            t[126] = n;
        }
    }
    if changes.deletes(scope, "Genre") {
        t[127] = 255;
    }
    if let Some(v) = changes.value(scope, "Genre") {
        t[127] = GENRES
            .iter()
            .position(|g| g.eq_ignore_ascii_case(v.trim()))
            .filter(|&i| i < 255)
            .map_or(255, |i| i as u8);
    }
    let empty = t[3..127].iter().all(|&b| b == 0 || b == b' ') && t[127] == 255;
    (!empty).then_some(t)
}

// ── containers ──────────────────────────────────────────────────────────────

/// Rewrite an MP3 file (ID3v2 at the start, ID3v1 at the end).
pub fn write_mp3(source: &[u8], changes: &Id3Changes) -> Result<Vec<u8>> {
    if changes.is_empty() {
        return Ok(source.to_vec());
    }
    let v2_len = if source.len() >= 10 && source.starts_with(b"ID3") {
        let n = 10 + read_syncsafe(&source[6..10]);
        // An ID3v2.4 footer repeats the header.
        let n = if source[3] == 4 && source[5] & 0x10 != 0 {
            n + 10
        } else {
            n
        };
        if n > source.len() {
            return Err(err("truncated ID3v2 tag"));
        }
        n
    } else {
        0
    };
    let has_v1 =
        source.len() >= v2_len + 128 && &source[source.len() - 128..source.len() - 125] == b"TAG";
    let audio_end = if has_v1 {
        source.len() - 128
    } else {
        source.len()
    };

    let mut out = Vec::with_capacity(source.len() + 1024);
    if changes.touches(Id3Scope::V2) {
        let old = (v2_len > 0).then(|| &source[..v2_len]);
        if let Some(tag) = edit_id3v2(old, changes)? {
            out.extend(tag);
        }
    } else {
        out.extend_from_slice(&source[..v2_len]);
    }
    out.extend_from_slice(&source[v2_len..audio_end]);
    if has_v1 {
        if let Some(t) = edit_id3v1(&source[audio_end..], changes) {
            out.extend(t);
        }
    }
    Ok(out)
}

/// Rewrite an AIFF/AIFC file: the ID3v2 tag in its `ID3 ` chunk.
pub fn write_aiff(source: &[u8], changes: &Id3Changes) -> Result<Vec<u8>> {
    if source.len() < 12
        || !source.starts_with(b"FORM")
        || (&source[8..12] != b"AIFF" && &source[8..12] != b"AIFC")
    {
        return Err(err("not an AIFF file"));
    }
    if changes.is_empty() || !changes.touches(Id3Scope::V2) {
        return Ok(source.to_vec());
    }
    let form_end = (8 + u32::from_be_bytes([source[4], source[5], source[6], source[7]]) as usize)
        .min(source.len());
    let mut body = Vec::with_capacity(source.len());
    body.extend_from_slice(b"AIFF");
    body[..4].copy_from_slice(&source[8..12]);
    let mut pos = 12;
    let mut done = false;
    while pos + 8 <= form_end {
        let id = &source[pos..pos + 4];
        let size = u32::from_be_bytes([
            source[pos + 4],
            source[pos + 5],
            source[pos + 6],
            source[pos + 7],
        ]) as usize;
        let end = pos + 8 + size;
        if end > form_end {
            return Err(err("truncated AIFF chunk"));
        }
        let padded = (end + (size & 1)).min(form_end);
        if (id == b"ID3 " || id == b"id3 ") && !done && source[pos + 8..end].starts_with(b"ID3") {
            done = true;
            if let Some(tag) = edit_id3v2(Some(&source[pos + 8..end]), changes)? {
                push_chunk(&mut body, id, &tag)?;
            }
        } else {
            body.extend_from_slice(&source[pos..padded]);
        }
        pos = padded;
    }
    if !done {
        if let Some(tag) = edit_id3v2(None, changes)? {
            push_chunk(&mut body, b"ID3 ", &tag)?;
        }
    }
    let size = u32::try_from(body.len()).map_err(|_| err("file too large"))?;
    let mut out = Vec::with_capacity(body.len() + 8 + source.len() - form_end);
    out.extend_from_slice(b"FORM");
    out.extend_from_slice(&size.to_be_bytes());
    out.extend(body);
    out.extend_from_slice(&source[form_end..]);
    Ok(out)
}

fn push_chunk(out: &mut Vec<u8>, id: &[u8], data: &[u8]) -> Result<()> {
    let n = u32::try_from(data.len()).map_err(|_| err("chunk too large"))?;
    out.extend_from_slice(id);
    out.extend_from_slice(&n.to_be_bytes());
    out.extend_from_slice(data);
    if data.len() % 2 == 1 {
        out.push(0);
    }
    Ok(())
}

/// The ID3v1 genres (ID3.pm `%genre`), by number.
static GENRES: &[&str] = &[
    "Blues",
    "Classic Rock",
    "Country",
    "Dance",
    "Disco",
    "Funk",
    "Grunge",
    "Hip-Hop",
    "Jazz",
    "Metal",
    "New Age",
    "Oldies",
    "Other",
    "Pop",
    "R&B",
    "Rap",
    "Reggae",
    "Rock",
    "Techno",
    "Industrial",
    "Alternative",
    "Ska",
    "Death Metal",
    "Pranks",
    "Soundtrack",
    "Euro-Techno",
    "Ambient",
    "Trip-Hop",
    "Vocal",
    "Jazz+Funk",
    "Fusion",
    "Trance",
    "Classical",
    "Instrumental",
    "Acid",
    "House",
    "Game",
    "Sound Clip",
    "Gospel",
    "Noise",
    "Alt. Rock",
    "Bass",
    "Soul",
    "Punk",
    "Space",
    "Meditative",
    "Instrumental Pop",
    "Instrumental Rock",
    "Ethnic",
    "Gothic",
    "Darkwave",
    "Techno-Industrial",
    "Electronic",
    "Pop-Folk",
    "Eurodance",
    "Dream",
    "Southern Rock",
    "Comedy",
    "Cult",
    "Gangsta Rap",
    "Top 40",
    "Christian Rap",
    "Pop/Funk",
    "Jungle",
    "Native American",
    "Cabaret",
    "New Wave",
    "Psychedelic",
    "Rave",
    "Showtunes",
    "Trailer",
    "Lo-Fi",
    "Tribal",
    "Acid Punk",
    "Acid Jazz",
    "Polka",
    "Retro",
    "Musical",
    "Rock & Roll",
    "Hard Rock",
    "Folk",
    "Folk-Rock",
    "National Folk",
    "Swing",
    "Fast-Fusion",
    "Bebop",
    "Latin",
    "Revival",
    "Celtic",
    "Bluegrass",
    "Avantgarde",
    "Gothic Rock",
    "Progressive Rock",
    "Psychedelic Rock",
    "Symphonic Rock",
    "Slow Rock",
    "Big Band",
    "Chorus",
    "Easy Listening",
    "Acoustic",
    "Humour",
    "Speech",
    "Chanson",
    "Opera",
    "Chamber Music",
    "Sonata",
    "Symphony",
    "Booty Bass",
    "Primus",
    "Porn Groove",
    "Satire",
    "Slow Jam",
    "Club",
    "Tango",
    "Samba",
    "Folklore",
    "Ballad",
    "Power Ballad",
    "Rhythmic Soul",
    "Freestyle",
    "Duet",
    "Punk Rock",
    "Drum Solo",
    "A Cappella",
    "Euro-House",
    "Dance Hall",
    "Goa",
    "Drum & Bass",
    "Club-House",
    "Hardcore",
    "Terror",
    "Indie",
    "BritPop",
    "Afro-Punk",
    "Polsk Punk",
    "Beat",
    "Christian Gangsta Rap",
    "Heavy Metal",
    "Black Metal",
    "Crossover",
    "Contemporary Christian",
    "Christian Rock",
    "Merengue",
    "Salsa",
    "Thrash Metal",
    "Anime",
    "JPop",
    "Synthpop",
    "Abstract",
    "Art Rock",
    "Baroque",
    "Bhangra",
    "Big Beat",
    "Breakbeat",
    "Chillout",
    "Downtempo",
    "Dub",
    "EBM",
    "Eclectic",
    "Electro",
    "Electroclash",
    "Emo",
    "Experimental",
    "Garage",
    "Global",
    "IDM",
    "Illbient",
    "Industro-Goth",
    "Jam Band",
    "Krautrock",
    "Leftfield",
    "Lounge",
    "Math Rock",
    "New Romantic",
    "Nu-Breakz",
    "Post-Punk",
    "Post-Rock",
    "Psytrance",
    "Shoegaze",
    "Space Rock",
    "Trop Rock",
    "World Music",
    "Neoclassical",
    "Audiobook",
    "Audio Theatre",
    "Neue Deutsche Welle",
    "Podcast",
    "Indie Rock",
    "G-Funk",
    "Dubstep",
    "Garage Rock",
    "Psybient",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(file: &[u8]) -> Vec<(String, String, String)> {
        // (with duplicates: ID3v1 Title is one of ID3v2 Title)
        crate::api::ExifTool::with_options(crate::api::Options { duplicates: true, ..Default::default() })
            .extract_info_from_bytes(file, std::path::Path::new(""))
            .unwrap()
            .into_iter()
            .filter(|t| t.group.family0 == "ID3")
            .map(|t| {
                (
                    t.group.family1.clone(),
                    t.name.clone(),
                    t.print_value.clone(),
                )
            })
            .collect()
    }

    /// An ID3v2.2 tag (TT2 + TP1 + PIC) and an ID3v1 trailer around fake audio.
    fn sample() -> Vec<u8> {
        let mut frames = Vec::new();
        frames.extend(frame("TT2", b"\0Old title", 2).unwrap());
        frames.extend(frame("TP1", b"\0Old artist", 2).unwrap());
        frames.extend(frame("PIC", b"\0JPG\0\0PICTURE", 2).unwrap());
        let mut f = b"ID3\x02\0\0".to_vec();
        f.extend(syncsafe(frames.len() + 20).unwrap());
        f.extend(frames);
        f.extend([0u8; 20]);
        f.extend(b"\xff\xfbAUDIO");
        let mut v1 = b"TAG".to_vec();
        v1.extend(latin1_field("V1 title", 30));
        v1.extend(latin1_field("V1 artist", 30));
        v1.extend(latin1_field("V1 album", 30));
        v1.extend(latin1_field("2003", 4));
        v1.extend(latin1_field("V1 comment", 30));
        v1.push(7);
        f.extend(v1);
        f
    }

    #[test]
    fn set_edits_both_tags_and_keeps_other_frames() {
        let src = sample();
        let ch = Id3Changes {
            set: vec![(Id3Scope::Any, "Title".into(), "New".into())],
            ..Default::default()
        };
        let out = write_mp3(&src, &ch).unwrap();
        let t = tags(&out);
        assert!(t.contains(&("ID3v2_2".into(), "Title".into(), "New".into())), "{t:?}");
        assert!(t.contains(&("ID3v1".into(), "Title".into(), "New".into())), "{t:?}");
        assert!(t.contains(&("ID3v2_2".into(), "Artist".into(), "Old artist".into())), "{t:?}");
        assert!(t.iter().any(|(_, n, _)| n == "Picture"));
        // Same size: the frames fit the old padding.
        assert_eq!(out.len(), src.len());
        assert!(out.windows(7).any(|w| w == b"\xff\xfbAUDIO"));
    }

    #[test]
    fn delete_and_add() {
        let src = sample();
        let ch = Id3Changes {
            set: vec![(Id3Scope::Any, "Album".into(), "Ålbum".into())],
            delete: vec![(Id3Scope::Any, "artist".into())],
            ..Default::default()
        };
        let out = write_mp3(&src, &ch).unwrap();
        let t = tags(&out);
        // Gone from ID3v2; the fixed-width ID3v1 field is left empty, which
        // ExifTool reads as an empty Artist.
        assert!(!t.iter().any(|(_, n, v)| n == "Artist" && !v.is_empty()));
        assert!(t.contains(&("ID3v2_2".into(), "Album".into(), "Ålbum".into())), "{t:?}");
        // The ID3v1 field is blanked; the others stay.
        assert!(t.contains(&("ID3v1".into(), "Title".into(), "V1 title".into())), "{t:?}");
        let v1 = &out[out.len() - 128..];
        assert!(v1[33..63].iter().all(|&b| b == 0));
        // A trailer left empty goes.
        let ch = Id3Changes {
            delete: ["title", "artist", "album", "year", "comment", "genre"]
                .iter()
                .map(|n| (Id3Scope::V1, n.to_string()))
                .collect(),
            ..Default::default()
        };
        let out = write_mp3(&src, &ch).unwrap();
        assert!(out.ends_with(b"\xff\xfbAUDIO"));
        // Non-Latin-1 text goes to UTF-16.
        let ch = Id3Changes {
            set: vec![(Id3Scope::V2, "Title".into(), "Ωmega".into())],
            ..Default::default()
        };
        let out = write_mp3(&src, &ch).unwrap();
        assert!(tags(&out).contains(&("ID3v2_2".into(), "Title".into(), "Ωmega".into())));
    }

    #[test]
    fn whole_tags_and_new_tag() {
        let src = sample();
        let ch = Id3Changes {
            delete_all: vec![Id3Scope::Any],
            ..Default::default()
        };
        let out = write_mp3(&src, &ch).unwrap();
        assert_eq!(out, b"\xff\xfbAUDIO");
        let ch = Id3Changes {
            set: vec![(Id3Scope::Any, "Title".into(), "T".into())],
            ..Default::default()
        };
        let out2 = write_mp3(&out, &ch).unwrap();
        assert!(out2.starts_with(b"ID3\x03"));
        assert!(tags(&out2).contains(&("ID3v2_3".into(), "Title".into(), "T".into())));
        assert!(out2.ends_with(b"\xff\xfbAUDIO"));
    }

    #[test]
    fn aiff_chunk_is_rewritten() {
        let mut tag = b"ID3\x03\0\0".to_vec();
        let fr = frame("TIT2", b"\0Old", 3).unwrap();
        tag.extend(syncsafe(fr.len()).unwrap());
        tag.extend(fr);
        let mut body = b"AIFF".to_vec();
        push_chunk(&mut body, b"COMM", &[0u8; 18]).unwrap();
        push_chunk(&mut body, b"ID3 ", &tag).unwrap();
        let mut src = b"FORM".to_vec();
        src.extend((body.len() as u32).to_be_bytes());
        src.extend(body);
        let ch = Id3Changes {
            set: vec![(Id3Scope::Any, "Artist".into(), "Someone else".into())],
            ..Default::default()
        };
        let out = write_aiff(&src, &ch).unwrap();
        let t = crate::api::ExifTool::new().extract_info_from_bytes(&out, std::path::Path::new("")).unwrap();
        let get = |n: &str| {
            t.iter()
                .find(|x| x.name == n)
                .map(|x| x.print_value.clone())
        };
        assert_eq!(get("Artist").as_deref(), Some("Someone else"));
        assert_eq!(get("Title").as_deref(), Some("Old"));
        let form = u32::from_be_bytes(out[4..8].try_into().unwrap()) as usize;
        assert_eq!(form + 8, out.len());
    }

    #[test]
    fn unsynchronised_tag_is_refused() {
        let mut src = sample();
        src[5] = 0x80;
        let ch = Id3Changes {
            set: vec![(Id3Scope::Any, "Title".into(), "x".into())],
            ..Default::default()
        };
        assert!(write_mp3(&src, &ch).is_err());
    }
}
