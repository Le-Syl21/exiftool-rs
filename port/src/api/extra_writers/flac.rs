//! FLAC metadata writer, and the Vorbis comment editor shared with the Ogg
//! writer.
//!
//! ExifTool reads Vorbis comments but does not write them. The comment
//! list is edited in place: the vendor string and every comment not asked
//! about are kept in their order; a written tag replaces all comments of
//! that tag (one comment, where the first one was); a deleted tag loses all
//! of them; `Vorbis:all` empties the list and the vendor string (the field
//! stays, as the format requires it). Tag names are matched the way
//! ExifTool names the fields (Vorbis.pm `ProcessComments`: `TITLE` is Title
//! from its table, `MEDIAJUKEBOX:TOOL NAME` is Mediajukebox:toolName made
//! a tag name, MediajukeboxToolName), and a new comment takes the field
//! of ExifTool's table for the name, or the name in capitals.
//!
//! In a FLAC file the `VORBIS_COMMENT` block is replaced (one is added
//! after `STREAMINFO` when there is none, and `Vorbis:all` removes it); the
//! other blocks and the audio frames are copied unchanged.

use crate::api::error::{Error, Result};

/// The Vorbis comment part of a write request.
#[derive(Debug, Clone, Default)]
pub struct VorbisChanges {
    /// (tag name, value).
    pub set: Vec<(String, String)>,
    /// Lower-case tag names.
    pub delete: Vec<String>,
    pub delete_all: bool,
}

impl VorbisChanges {
    pub fn is_empty(&self) -> bool {
        self.set.is_empty() && self.delete.is_empty() && !self.delete_all
    }

    fn value(&self, name: &str) -> Option<&str> {
        self.set
            .iter()
            .rev()
            .find(|(t, _)| t.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

fn err(msg: impl Into<String>) -> Error {
    Error::InvalidData(format!("Vorbis: {}", msg.into()))
}

/// The fields of Vorbis.pm `%Image::ExifTool::Vorbis::Comments` and their
/// tag names.
const FIELDS: &[(&str, &str)] = &[
    ("vendor", "Vendor"),
    ("TITLE", "Title"),
    ("VERSION", "Version"),
    ("ALBUM", "Album"),
    ("TRACKNUMBER", "TrackNumber"),
    ("ARTIST", "Artist"),
    ("PERFORMER", "Performer"),
    ("COPYRIGHT", "Copyright"),
    ("LICENSE", "License"),
    ("ORGANIZATION", "Organization"),
    ("DESCRIPTION", "Description"),
    ("GENRE", "Genre"),
    ("DATE", "Date"),
    ("LOCATION", "Location"),
    ("CONTACT", "Contact"),
    ("ISRC", "ISRCNumber"),
    ("COVERARTMIME", "CoverArtMIMEType"),
    ("COVERART", "CoverArt"),
    ("REPLAYGAIN_TRACK_PEAK", "ReplayGainTrackPeak"),
    ("REPLAYGAIN_TRACK_GAIN", "ReplayGainTrackGain"),
    ("REPLAYGAIN_ALBUM_PEAK", "ReplayGainAlbumPeak"),
    ("REPLAYGAIN_ALBUM_GAIN", "ReplayGainAlbumGain"),
    ("ENCODED_USING", "EncodedUsing"),
    ("ENCODED_BY", "EncodedBy"),
    ("COMMENT", "Comment"),
    ("DIRECTOR", "Director"),
    ("PRODUCER", "Producer"),
    ("COMPOSER", "Composer"),
    ("ACTOR", "Actor"),
    ("ENCODER", "Encoder"),
    ("ENCODER_OPTIONS", "EncoderOptions"),
    ("METADATA_BLOCK_PICTURE", "Picture"),
];

/// The tag name ExifTool gives a comment field (Vorbis.pm
/// `ProcessComments`): the table's, or the field made a tag name.
fn vorbis_tag_name(field: &str) -> String {
    let id = field.to_uppercase();
    if let Some((_, name)) = FIELDS.iter().find(|(f, _)| *f == id) {
        return name.to_string();
    }
    // ucfirst(lc), then `s/[^\w-]+(.?)/\U$1/sg` and
    // `s/([a-z0-9])_([a-z])/$1\U$2/g`
    let lower: Vec<char> = id.to_lowercase().chars().collect();
    let mut name = String::new();
    let mut i = 0;
    let word = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
    while i < lower.len() {
        let c = lower[i];
        if word(c) {
            name.push(if i == 0 { c.to_uppercase().next().unwrap_or(c) } else { c });
            i += 1;
        } else {
            while i < lower.len() && !word(lower[i]) {
                i += 1;
            }
            if i < lower.len() {
                name.extend(lower[i].to_uppercase());
                i += 1;
            }
        }
    }
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '_' && i > 0 && (chars[i - 1].is_ascii_lowercase() || chars[i - 1].is_ascii_digit()) && chars.get(i + 1).is_some_and(|n| n.is_ascii_lowercase()) {
            out.push(chars[i + 1].to_ascii_uppercase());
            i += 2;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// The field written for a new comment: that of ExifTool's table for the
/// tag name, or the name in capitals (which ExifTool reads back as the
/// same name, case aside).
fn field_for_tag(tag: &str) -> String {
    FIELDS
        .iter()
        .find(|(_, t)| t.eq_ignore_ascii_case(tag))
        .map_or_else(|| tag.to_ascii_uppercase(), |(f, _)| f.to_string())
}

fn le32(d: &[u8], at: usize) -> Option<usize> {
    let b = d.get(at..at + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
}

/// Edit a Vorbis comment structure (vendor, count, comments; no framing
/// bit). Returns the new structure and the length of the old one, so the
/// caller keeps whatever followed it.
pub fn edit_vorbis_comment(data: &[u8], changes: &VorbisChanges) -> Result<(Vec<u8>, usize)> {
    let vendor_len = le32(data, 0).ok_or_else(|| err("truncated comment header"))?;
    let mut pos = 4usize
        .checked_add(vendor_len)
        .filter(|&p| p <= data.len())
        .ok_or_else(|| err("truncated vendor string"))?;
    let vendor = &data[4..pos];
    let count = le32(data, pos).ok_or_else(|| err("truncated comment list"))?;
    pos += 4;
    let mut comments: Vec<&[u8]> = Vec::new();
    for _ in 0..count {
        let n = le32(data, pos).ok_or_else(|| err("truncated comment list"))?;
        pos += 4;
        let c = data
            .get(pos..pos + n)
            .ok_or_else(|| err("truncated comment"))?;
        comments.push(c);
        pos += n;
    }

    let mut out_comments: Vec<Vec<u8>> = Vec::new();
    let mut written: Vec<String> = Vec::new();
    if !changes.delete_all {
        for c in comments {
            let field = c.iter().position(|&b| b == b'=').map(|i| &c[..i]);
            let name = field.map(|f| vorbis_tag_name(&String::from_utf8_lossy(f)));
            let Some(name) = name else {
                out_comments.push(c.to_vec());
                continue;
            };
            if changes.delete.iter().any(|d| d.eq_ignore_ascii_case(&name)) {
                continue;
            }
            match changes.value(&name) {
                Some(v) => {
                    let lc = name.to_ascii_lowercase();
                    if written.contains(&lc) {
                        continue;
                    }
                    written.push(lc);
                    let mut nc = field.unwrap_or_default().to_vec();
                    nc.push(b'=');
                    nc.extend_from_slice(v.as_bytes());
                    out_comments.push(nc);
                }
                None => out_comments.push(c.to_vec()),
            }
        }
    }
    for (tag, _) in &changes.set {
        let lc = tag.to_ascii_lowercase();
        if written.contains(&lc) {
            continue;
        }
        written.push(lc);
        let v = changes.value(tag).unwrap_or_default();
        out_comments.push(format!("{}={v}", field_for_tag(tag)).into_bytes());
    }

    // `Vorbis:all` takes the vendor string too (it reads as the Vendor
    // tag); the field itself stays, as the format requires it.
    let vendor: &[u8] = if changes.delete_all { &[] } else { vendor };
    let mut out = Vec::new();
    out.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    out.extend_from_slice(vendor);
    out.extend_from_slice(&(out_comments.len() as u32).to_le_bytes());
    for c in &out_comments {
        let n = u32::try_from(c.len()).map_err(|_| err("comment too long"))?;
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(c);
    }
    Ok((out, pos))
}

/// A new comment structure (vendor string, no comments) to edit.
pub fn empty_vorbis_comment() -> Vec<u8> {
    let vendor = format!("exiftool-rs {}", env!("CARGO_PKG_VERSION"));
    let mut out = Vec::new();
    out.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    out.extend_from_slice(vendor.as_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out
}

fn block_header(kind: u8, last: bool, len: usize) -> Result<[u8; 4]> {
    if len >= 1 << 24 {
        return Err(err("metadata block too large"));
    }
    Ok([
        kind | if last { 0x80 } else { 0 },
        (len >> 16) as u8,
        (len >> 8) as u8,
        len as u8,
    ])
}

/// Rewrite a FLAC file with its Vorbis comments edited.
pub fn write_flac(source: &[u8], changes: &VorbisChanges) -> Result<Vec<u8>> {
    if source.len() < 8 || !source.starts_with(b"fLaC") {
        return Err(Error::InvalidData("not a FLAC file".into()));
    }
    if changes.is_empty() {
        return Ok(source.to_vec());
    }
    // The metadata blocks: (type, payload range).
    let mut blocks: Vec<(u8, std::ops::Range<usize>)> = Vec::new();
    let mut pos = 4;
    loop {
        if pos + 4 > source.len() {
            return Err(err("truncated FLAC metadata"));
        }
        let h = source[pos];
        let len = (source[pos + 1] as usize) << 16
            | (source[pos + 2] as usize) << 8
            | source[pos + 3] as usize;
        let start = pos + 4;
        if start + len > source.len() {
            return Err(err("truncated FLAC metadata block"));
        }
        blocks.push((h & 0x7F, start..start + len));
        pos = start + len;
        if h & 0x80 != 0 {
            break;
        }
    }
    let audio = pos;

    let mut new_blocks: Vec<(u8, Vec<u8>)> = Vec::new();
    let mut done = false;
    for (kind, r) in &blocks {
        if *kind == 4 && !done {
            done = true;
            // `Vorbis:all`: a FLAC file needs no comment block at all.
            if changes.delete_all && changes.set.is_empty() {
                continue;
            }
            let (c, _) = edit_vorbis_comment(&source[r.clone()], changes)?;
            new_blocks.push((4, c));
        } else {
            new_blocks.push((*kind, source[r.clone()].to_vec()));
        }
    }
    if !done {
        if changes.set.is_empty() {
            // Nothing to delete in a file without comments.
            return Ok(source.to_vec());
        }
        let (c, _) = edit_vorbis_comment(&empty_vorbis_comment(), changes)?;
        let at = usize::from(!new_blocks.is_empty() && new_blocks[0].0 == 0);
        new_blocks.insert(at, (4, c));
    }
    let mut out = Vec::with_capacity(source.len() + 256);
    out.extend_from_slice(b"fLaC");
    let n = new_blocks.len();
    for (i, (kind, data)) in new_blocks.iter().enumerate() {
        out.extend_from_slice(&block_header(*kind, i + 1 == n, data.len())?);
        out.extend_from_slice(data);
    }
    out.extend_from_slice(&source[audio..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comment(vendor: &str, items: &[&str]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend((vendor.len() as u32).to_le_bytes());
        out.extend(vendor.as_bytes());
        out.extend((items.len() as u32).to_le_bytes());
        for c in items {
            out.extend((c.len() as u32).to_le_bytes());
            out.extend(c.as_bytes());
        }
        out
    }

    fn flac(comments: &[u8]) -> Vec<u8> {
        let mut f = b"fLaC".to_vec();
        f.extend(block_header(0, false, 34).unwrap());
        f.extend([0u8; 34]);
        f.extend(block_header(4, false, comments.len()).unwrap());
        f.extend(comments);
        f.extend(block_header(1, true, 8).unwrap());
        f.extend([0u8; 8]);
        f.extend(b"\xff\xf8AUDIO");
        f
    }

    fn read(file: &[u8]) -> Vec<(String, String)> {
        crate::api::ExifTool::new()
            .extract_info_from_bytes(file, std::path::Path::new(""))
            .unwrap()
            .into_iter()
            .filter(|t| t.group.family0 == "Vorbis")
            .map(|t| (t.name, t.print_value))
            .collect()
    }

    #[test]
    fn set_delete_keep_vendor_and_others() {
        let src = flac(&comment(
            "ref",
            &["TITLE=Old", "ARTIST=A", "title=Dup", "GENRE=G"],
        ));
        let ch = VorbisChanges {
            set: vec![
                ("Title".into(), "New".into()),
                ("Album".into(), "Alb".into()),
            ],
            delete: vec!["genre".into()],
            ..Default::default()
        };
        let out = write_flac(&src, &ch).unwrap();
        assert_eq!(
            read(&out),
            vec![
                ("Vendor".into(), "ref".into()),
                ("Title".into(), "New".into()),
                ("Artist".into(), "A".into()),
                ("Album".into(), "Alb".into()),
            ]
        );
        assert!(out.ends_with(b"\xff\xf8AUDIO"));
        let ch = VorbisChanges {
            delete_all: true,
            ..Default::default()
        };
        let out = write_flac(&src, &ch).unwrap();
        assert!(read(&out).is_empty());
        assert!(out.ends_with(b"\xff\xf8AUDIO"));
    }

    #[test]
    fn names_are_exiftools() {
        assert_eq!(vorbis_tag_name("title"), "Title");
        assert_eq!(vorbis_tag_name("ISRC"), "ISRCNumber");
        assert_eq!(vorbis_tag_name("MEDIAJUKEBOX:TOOL NAME"), "MediajukeboxToolName");
        assert_eq!(vorbis_tag_name("ALBUM ARTIST"), "AlbumArtist");
        assert_eq!(vorbis_tag_name("TRACKTOTAL"), "Tracktotal");
        assert_eq!(vorbis_tag_name("MUSICBRAINZ_ALBUMID"), "MusicbrainzAlbumid");
        assert_eq!(field_for_tag("ISRCNumber"), "ISRC");
        assert_eq!(field_for_tag("AlbumArtist"), "ALBUMARTIST");
    }

    #[test]
    fn adds_a_comment_block() {
        let mut f = b"fLaC".to_vec();
        f.extend(block_header(0, true, 34).unwrap());
        f.extend([0u8; 34]);
        f.extend(b"\xff\xf8AUDIO");
        let ch = VorbisChanges {
            set: vec![("Artist".into(), "Me".into())],
            ..Default::default()
        };
        let out = write_flac(&f, &ch).unwrap();
        assert!(read(&out).contains(&("Artist".into(), "Me".into())));
        // STREAMINFO lost its last-block flag to the new block.
        assert_eq!(out[4], 0);
        assert_eq!(out[4 + 4 + 34] & 0x80, 0x80);
        assert!(out.ends_with(b"\xff\xf8AUDIO"));
    }
}
