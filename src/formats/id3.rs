//! ID3 tag reader for MP3 and other audio files.
//!
//! Supports ID3v1, ID3v1.1, ID3v2.2, ID3v2.3, ID3v2.4.
//! Mirrors ExifTool's ID3.pm.

use crate::error::Result;
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

/// Read MP3 file: ID3v2 at start, ID3v1 at end, plus basic MPEG audio info.
pub fn read_mp3(data: &[u8]) -> Result<Vec<Tag>> {
    let mut tags = Vec::new();

    // Calculate ID3 size for ID3Size tag (matches Perl's $id3Len)
    let mut id3_len: usize = 0;

    // Try ID3v2 at start
    if data.len() >= 10 && data.starts_with(b"ID3") {
        let tag_size = syncsafe_u32(&data[6..10]) as usize;
        id3_len += tag_size + 10;
        let id3v2_tags = read_id3v2(data)?;
        tags.extend(id3v2_tags);
    }

    // Try ID3v1 at end (last 128 bytes)
    if data.len() >= 128 {
        let v1_start = data.len() - 128;
        if &data[v1_start..v1_start + 3] == b"TAG" {
            id3_len += 128;
            let id3v1_tags = read_id3v1(&data[v1_start..]);
            // Only add v1 tags not already present from v2
            for t in id3v1_tags {
                if !tags.iter().any(|existing| existing.name == t.name) {
                    tags.push(t);
                }
            }
        }
    }

    // Emit ID3Size (total size of ID3 metadata, like Perl's $id3Len)
    if id3_len > 0 {
        tags.push(mk("ID3Size", "ID3 Size", Value::U32(id3_len as u32)));
    }

    // Find MPEG audio frame header (after ID3v2 tag if present)
    let audio_start = if data.starts_with(b"ID3") && data.len() >= 10 {
        let size = syncsafe_u32(&data[6..10]);
        10 + size as usize
    } else {
        0
    };

    if let Some(mpeg_tags) = parse_mpeg_header(data, audio_start) {
        tags.extend(mpeg_tags);
    }

    // Duration composite: (FileSize - ID3Size) * 8 / AudioBitrate_bps
    // (approximate, matching Perl's MPEG::Composite Duration formula)
    {
        let audio_bitrate_bps = tags
            .iter()
            .find(|t| t.name == "AudioBitrate")
            .and_then(|t| {
                // print_value is like "128 kbps"
                t.print_value
                    .split_whitespace()
                    .next()
                    .and_then(|s| s.parse::<u64>().ok())
                    .map(|kbps| kbps * 1000)
            });
        if let Some(bps) = audio_bitrate_bps {
            if bps > 0 {
                let audio_bytes = (data.len() as u64).saturating_sub(id3_len as u64);
                let duration_secs = (8 * audio_bytes) as f64 / bps as f64;
                // Format like Perl: "0.02 s (approx)"
                let print = format!("{:.2} s (approx)", duration_secs);
                tags.push(mk("Duration", "Duration", Value::String(print)));
            }
        }
    }

    // DateTimeOriginal from Year (ID3 composite, like Perl ID3::Composite DateTimeOriginal)
    if !tags.iter().any(|t| t.name == "DateTimeOriginal") {
        if let Some(year) = tags
            .iter()
            .find(|t| t.name == "Year")
            .map(|t| t.print_value.clone())
        {
            if !year.is_empty() {
                tags.push(mk(
                    "DateTimeOriginal",
                    "Date/Time Original",
                    Value::String(year),
                ));
            }
        }
    }

    Ok(tags)
}

/// Parse ID3v2 header and frames.
fn read_id3v2(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 10 || !data.starts_with(b"ID3") {
        return Ok(Vec::new());
    }

    let version = data[3];
    let _revision = data[4];
    let _flags = data[5];
    let tag_size = syncsafe_u32(&data[6..10]) as usize;

    let mut tags = Vec::new();
    // Note: Perl does NOT emit ID3Version; we skip it to match Perl output.

    let end = (10 + tag_size).min(data.len());
    let mut pos = 10;

    // Skip extended header if present (flag bit 6)
    if _flags & 0x40 != 0 && pos + 4 <= end {
        let ext_size = if version == 4 {
            syncsafe_u32(&data[pos..pos + 4]) as usize
        } else {
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize
        };
        pos += ext_size;
    }

    while pos < end {
        if version == 2 {
            // ID3v2.2: 3-byte frame ID + 3-byte size
            if pos + 6 > end {
                break;
            }
            let frame_id = &data[pos..pos + 3];
            if frame_id[0] == 0 {
                break;
            }
            let frame_size = ((data[pos + 3] as usize) << 16)
                | ((data[pos + 4] as usize) << 8)
                | data[pos + 5] as usize;
            pos += 6;
            if frame_size == 0 || pos + frame_size > end {
                break;
            }
            let frame_data = &data[pos..pos + frame_size];
            let new_tags = decode_id3v2_frame_22(frame_id, frame_data);
            tags.extend(new_tags);
            pos += frame_size;
        } else {
            // ID3v2.3/v2.4: 4-byte frame ID + 4-byte size + 2-byte flags
            if pos + 10 > end {
                break;
            }
            let frame_id = &data[pos..pos + 4];
            if frame_id[0] == 0 {
                break;
            }
            let frame_size = if version == 4 {
                syncsafe_u32(&data[pos + 4..pos + 8]) as usize
            } else {
                u32::from_be_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]])
                    as usize
            };
            let _flags = u16::from_be_bytes([data[pos + 8], data[pos + 9]]);
            pos += 10;
            if frame_size == 0 || pos + frame_size > end {
                break;
            }
            let frame_data = &data[pos..pos + frame_size];
            if let Some(tag) = decode_id3v2_frame(frame_id, frame_data) {
                tags.push(tag);
            }
            pos += frame_size;
        }
    }

    Ok(tags)
}

/// Decode a sync-safe integer (7 bits per byte).
fn syncsafe_u32(data: &[u8]) -> u32 {
    ((data[0] as u32) << 21) | ((data[1] as u32) << 14) | ((data[2] as u32) << 7) | data[3] as u32
}

/// Decode ID3v2.2 frame (3-char IDs). Returns 0 or more tags (PIC returns 4).
fn decode_id3v2_frame_22(frame_id: &[u8], data: &[u8]) -> Vec<Tag> {
    let id = match std::str::from_utf8(frame_id) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    // Special frames that return multiple tags
    match id {
        "PIC" => return decode_pic_frame(data),
        "ULT" => {
            if let Some(tag) = decode_comment_frame("Lyrics", "Lyrics", data) {
                return vec![tag];
            }
            return Vec::new();
        }
        "COM" => {
            if let Some(tag) = decode_comment_frame("Comment", "Comment", data) {
                return vec![tag];
            }
            return Vec::new();
        }
        "RVA" => {
            if let Some(tag) = decode_rva_frame(data) {
                return vec![tag];
            }
            return Vec::new();
        }
        _ => {}
    }

    let (name, description) = match id {
        "TT2" => ("Title", "Title"),
        "TP1" => ("Artist", "Artist"),
        "TAL" => ("Album", "Album"),
        "TRK" => ("Track", "Track"),
        "TYE" => ("Year", "Year"),
        "TCO" => ("Genre", "Genre"),
        "TEN" => ("EncodedBy", "Encoded By"),
        "TCM" => ("Composer", "Composer"),
        "TT1" => ("Grouping", "Grouping"),
        "TP2" => ("Band", "Band"),
        "TPA" => ("PartOfSet", "Part of Set"),
        "TCP" => ("Compilation", "Compilation"),
        _ => return Vec::new(),
    };

    let text = match decode_id3_text(data) {
        Some(t) => t,
        None => return Vec::new(),
    };

    // Genre: resolve numeric genre codes like "(13)" or "13"
    if name == "Genre" {
        let resolved = resolve_genre(&text);
        return vec![mk(name, description, Value::String(resolved))];
    }

    // Compilation: map "0" → "No", "1" → "Yes"
    if name == "Compilation" {
        let val = match text.trim() {
            "0" => "No".to_string(),
            "1" => "Yes".to_string(),
            other => other.to_string(),
        };
        return vec![mk(name, description, Value::String(val))];
    }

    vec![mk(name, description, Value::String(text))]
}

/// Decode ID3v2.3/v2.4 frame (4-char IDs).
fn decode_id3v2_frame(frame_id: &[u8], data: &[u8]) -> Option<Tag> {
    let id = std::str::from_utf8(frame_id).ok()?;
    let (name, description) = match id {
        "TIT1" => ("Grouping", "Grouping"),
        "TIT2" => ("Title", "Title"),
        "TIT3" => ("Subtitle", "Subtitle"),
        "TPE1" => ("Artist", "Artist"),
        "TPE2" => ("Band", "Band"),
        "TPE3" => ("Conductor", "Conductor"),
        "TPE4" => ("InterpretedBy", "Interpreted By"),
        "TALB" => ("Album", "Album"),
        "TRCK" => ("Track", "Track"),
        "TPOS" => ("PartOfSet", "Part of Set"),
        "TYER" | "TDRC" => ("Year", "Year"),
        "TCON" => ("Genre", "Genre"),
        "TCOM" => ("Composer", "Composer"),
        "TENC" => ("EncodedBy", "Encoded By"),
        "TBPM" => ("BeatsPerMinute", "Beats Per Minute"),
        "TLEN" => ("Length", "Length"),
        "TPUB" => ("Publisher", "Publisher"),
        "TLAN" => ("Language", "Language"),
        "TCOP" => ("Copyright", "Copyright"),
        "TSSE" => ("EncoderSettings", "Encoder Settings"),
        "TSRC" => ("ISRC", "ISRC"),
        "TCMP" => ("Compilation", "Compilation"),
        "COMM" => return decode_comment_frame("Comment", "Comment", data),
        "USLT" => return decode_comment_frame("Lyrics", "Lyrics", data),
        "APIC" => {
            // APIC returns multiple tags; can't do it in single Option<Tag> path
            // Return only the Picture tag (main binary tag). The sub-tags are handled below.
            return decode_apic_primary(data);
        }
        "TXXX" => return decode_txxx_frame(data),
        "WOAR" => {
            let url = crate::encoding::decode_utf8_or_latin1(data)
                .trim_end_matches('\0')
                .to_string();
            return Some(mk("ArtistURL", "Artist URL", Value::String(url)));
        }
        "WXXX" => return decode_wxxx_frame(data),
        "PCNT" => {
            if data.len() >= 4 {
                let count = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                return Some(mk("PlayCounter", "Play Counter", Value::U32(count)));
            }
            return None;
        }
        "POPM" => return decode_popularity_frame(data),
        _ => return None,
    };

    let text = decode_id3_text(data)?;
    // Genre: resolve numeric genre codes like "(13)" or "13"
    if name == "Genre" {
        let resolved = resolve_genre(&text);
        return Some(mk(name, description, Value::String(resolved)));
    }
    // Compilation: map "0" → "No", "1" → "Yes"
    if name == "Compilation" {
        let val = match text.trim() {
            "0" => "No".to_string(),
            "1" => "Yes".to_string(),
            other => other.to_string(),
        };
        return Some(mk(name, description, Value::String(val)));
    }
    Some(mk(name, description, Value::String(text)))
}

/// Decode ID3 text with encoding byte.
fn decode_id3_text(data: &[u8]) -> Option<String> {
    if data.is_empty() {
        return None;
    }
    let encoding = data[0];
    let text_data = &data[1..];

    let text = match encoding {
        0 => {
            // Latin-1 / ISO-8859-1
            text_data.iter().map(|&b| b as char).collect::<String>()
        }
        1 => {
            // UTF-16 with BOM
            decode_utf16(text_data)
        }
        2 => {
            // UTF-16BE without BOM
            decode_utf16_be(text_data)
        }
        3 => {
            // UTF-8
            crate::encoding::decode_utf8_or_latin1(text_data).to_string()
        }
        _ => crate::encoding::decode_utf8_or_latin1(text_data).to_string(),
    };

    let trimmed = text.trim_end_matches('\0').to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn decode_utf16(data: &[u8]) -> String {
    if data.len() < 2 {
        return String::new();
    }
    let is_le = data[0] == 0xFF && data[1] == 0xFE;
    let text_data = &data[2..];
    if is_le {
        decode_utf16_le(text_data)
    } else {
        decode_utf16_be(text_data)
    }
}

fn decode_utf16_le(data: &[u8]) -> String {
    let units: Vec<u16> = data
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

fn decode_utf16_be(data: &[u8]) -> String {
    let units: Vec<u16> = data
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

/// Decode COMM/USLT/ULT frame (comment/lyrics with language).
fn decode_comment_frame(name: &str, description: &str, data: &[u8]) -> Option<Tag> {
    if data.len() < 5 {
        return None;
    }
    let encoding = data[0];
    let _language = &data[1..4]; // 3-byte ISO 639-2 code
    let rest = &data[4..];

    // Find null terminator separating short description from text
    let (_, text_part) = split_encoded_string(rest, encoding);
    let text = decode_raw_text(text_part, encoding);
    let trimmed = text.trim_end_matches('\0').to_string();

    if trimmed.is_empty() {
        None
    } else {
        Some(mk(name, description, Value::String(trimmed)))
    }
}

/// Decode ID3v2.2 PIC frame → 4 separate tags:
///   PictureFormat (3-char format like "JPG"), PictureType (int), PictureDescription, Picture (binary)
/// Format: encoding(1) + imageFormat(3) + pictureType(1) + description(encoded, null-term) + pictureData
fn decode_pic_frame(data: &[u8]) -> Vec<Tag> {
    if data.len() < 6 {
        return Vec::new();
    }
    let encoding = data[0];
    // 3-char image format (e.g., "JPG", "PNG")
    let image_format = crate::encoding::decode_utf8_or_latin1(&data[1..4]).to_string();
    let pic_type = data[4];
    let rest = &data[5..];

    // Description (encoded, null-terminated)
    let (desc_bytes, image_data) = split_encoded_string(rest, encoding);
    let description = decode_raw_text(desc_bytes, encoding);
    let description = description.trim_end_matches('\0').to_string();

    let pic_type_str = picture_type_str(pic_type);

    let mut tags = Vec::new();
    tags.push(mk(
        "PictureFormat",
        "Picture Format",
        Value::String(image_format),
    ));
    tags.push(mk(
        "PictureType",
        "Picture Type",
        Value::String(pic_type_str.to_string()),
    ));
    tags.push(mk(
        "PictureDescription",
        "Picture Description",
        Value::String(description),
    ));
    tags.push(mk("Picture", "Picture", Value::Binary(image_data.to_vec())));
    tags
}

/// Decode APIC frame → primary Picture tag only (the sub-tags PictureMIMEType/PictureType/PictureDescription
/// are handled separately in the frame dispatcher for v2.3+).
/// Returns the main "Picture" binary tag.
fn decode_apic_primary(data: &[u8]) -> Option<Tag> {
    if data.len() < 4 {
        return None;
    }
    let encoding = data[0];
    let rest = &data[1..];

    // MIME type (Latin-1, null-terminated)
    let null_pos = rest.iter().position(|&b| b == 0)?;
    let rest = &rest[null_pos + 1..];

    if rest.is_empty() {
        return None;
    }
    let _pic_type = rest[0];
    let rest = &rest[1..];

    // Description (encoded, null-terminated)
    let (_, image_data) = split_encoded_string(rest, encoding);

    Some(mk("Picture", "Picture", Value::Binary(image_data.to_vec())))
}

/// Decode RVA frame (ID3v2.2 relative volume adjustment).
/// Returns a RelativeVolumeAdjustment tag string like "+18.0% Right, +18.0% Left"
fn decode_rva_frame(data: &[u8]) -> Option<Tag> {
    if data.len() < 2 {
        return None;
    }
    let mut dat: Vec<u8> = data.to_vec();
    let flag = dat.remove(0) as u32;
    if dat.is_empty() {
        return None;
    }
    let bits = dat.remove(0) as u32;
    if bits == 0 {
        return None;
    }
    let bytes = ((bits + 7) / 8) as usize;

    // channels: (name, vol_idx, peak_idx, flag_bit)
    let channels: &[(&str, usize, usize, u32)] = &[
        ("Right", 0, 2, 0x01),
        ("Left", 1, 3, 0x02),
        ("Back-right", 4, 6, 0x04),
        ("Back-left", 5, 7, 0x08),
        ("Center", 8, 9, 0x10),
        ("Bass", 10, 11, 0x20),
    ];

    let mut parts = Vec::new();
    for &(name, vol_idx, peak_idx, flag_bit) in channels {
        let j = peak_idx * bytes;
        if dat.len() < j + bytes {
            break;
        }
        let i = vol_idx * bytes;
        let mut rel: i64 = 0;
        for b in 0..bytes {
            rel = rel * 256 + dat[i + b] as i64;
        }
        if flag & flag_bit == 0 {
            rel = -rel;
        }
        let max_val = ((1i64 << bits) - 1) as f64;
        let pct = 100.0 * rel as f64 / max_val;
        parts.push(format!("{:+.1}% {}", pct, name));
    }

    if parts.is_empty() {
        return None;
    }

    Some(mk(
        "RelativeVolumeAdjustment",
        "Relative Volume Adjustment",
        Value::String(parts.join(", ")),
    ))
}

/// Decode TXXX (user-defined text) frame.
fn decode_txxx_frame(data: &[u8]) -> Option<Tag> {
    if data.len() < 2 {
        return None;
    }
    let encoding = data[0];
    let rest = &data[1..];
    let (desc_bytes, value_bytes) = split_encoded_string(rest, encoding);
    let desc = decode_raw_text(desc_bytes, encoding);
    let value = decode_raw_text(value_bytes, encoding);
    let desc = desc.trim_end_matches('\0');
    let value = value.trim_end_matches('\0');

    if desc.is_empty() {
        return None;
    }

    Some(mk(desc, desc, Value::String(value.to_string())))
}

/// Decode WXXX (user-defined URL) frame.
fn decode_wxxx_frame(data: &[u8]) -> Option<Tag> {
    if data.len() < 2 {
        return None;
    }
    let encoding = data[0];
    let rest = &data[1..];
    let (desc_bytes, url_bytes) = split_encoded_string(rest, encoding);
    let desc = decode_raw_text(desc_bytes, encoding);
    let url = crate::encoding::decode_utf8_or_latin1(url_bytes)
        .trim_end_matches('\0')
        .to_string();
    let desc = desc.trim_end_matches('\0');
    let name = if desc.is_empty() { "UserURL" } else { desc };

    Some(mk(name, name, Value::String(url)))
}

/// Decode POPM (Popularimeter) frame.
fn decode_popularity_frame(data: &[u8]) -> Option<Tag> {
    let null_pos = data.iter().position(|&b| b == 0)?;
    let email = crate::encoding::decode_utf8_or_latin1(&data[..null_pos]).to_string();
    let rest = &data[null_pos + 1..];
    if rest.is_empty() {
        return None;
    }
    let rating = rest[0];
    let cnt: u32 = if rest.len() >= 5 {
        u32::from_be_bytes([rest[1], rest[2], rest[3], rest[4]])
    } else {
        0
    };
    Some(mk(
        "Popularimeter",
        "Popularimeter",
        Value::String(format!("{} {} {}", email, rating, cnt)),
    ))
}

/// Split encoded string at null terminator.
fn split_encoded_string(data: &[u8], encoding: u8) -> (&[u8], &[u8]) {
    if encoding == 1 || encoding == 2 {
        // UTF-16: look for double-null
        let mut i = 0;
        while i + 1 < data.len() {
            if data[i] == 0 && data[i + 1] == 0 {
                return (&data[..i], &data[i + 2..]);
            }
            i += 2;
        }
        (data, &[])
    } else {
        // Latin1/UTF-8: look for single null
        match data.iter().position(|&b| b == 0) {
            Some(pos) => (&data[..pos], &data[pos + 1..]),
            None => (data, &[]),
        }
    }
}

fn decode_raw_text(data: &[u8], encoding: u8) -> String {
    match encoding {
        0 => data.iter().map(|&b| b as char).collect(),
        1 => decode_utf16(data),
        2 => decode_utf16_be(data),
        3 => crate::encoding::decode_utf8_or_latin1(data).to_string(),
        _ => crate::encoding::decode_utf8_or_latin1(data).to_string(),
    }
}

/// Resolve ID3 genre: "(13)" → "Pop", "13" → "Pop", "Pop" → "Pop"
fn resolve_genre(text: &str) -> String {
    let text = text.trim();
    // Handle "(NN)" format
    let inner = if text.starts_with('(') && text.ends_with(')') {
        &text[1..text.len() - 1]
    } else {
        text
    };

    if let Ok(idx) = inner.parse::<usize>() {
        if idx < GENRES.len() {
            return GENRES[idx].to_string();
        }
    }
    text.to_string()
}

/// Map picture type byte to description string (matches Perl's %pictureType).
fn picture_type_str(pic_type: u8) -> &'static str {
    match pic_type {
        0 => "Other",
        1 => "32x32 PNG Icon",
        2 => "Other Icon",
        3 => "Front Cover",
        4 => "Back Cover",
        5 => "Leaflet",
        6 => "Media",
        7 => "Lead Artist",
        8 => "Artist",
        9 => "Conductor",
        10 => "Band",
        11 => "Composer",
        12 => "Lyricist",
        13 => "Recording Studio or Location",
        14 => "Recording Session",
        15 => "Performance",
        16 => "Capture from Movie or Video",
        17 => "Bright(ly) Colored Fish",
        18 => "Illustration",
        19 => "Band Logo",
        20 => "Publisher Logo",
        _ => "Unknown",
    }
}

/// Read ID3v1 tag (last 128 bytes of file).
fn read_id3v1(data: &[u8]) -> Vec<Tag> {
    let mut tags = Vec::new();
    if data.len() < 128 || &data[0..3] != b"TAG" {
        return tags;
    }

    let title = latin1_string(&data[3..33]);
    let artist = latin1_string(&data[33..63]);
    let album = latin1_string(&data[63..93]);
    let year = latin1_string(&data[93..97]);
    let comment = latin1_string(&data[97..127]);
    let genre_idx = data[127] as usize;

    if !title.is_empty() {
        tags.push(mk("Title", "Title", Value::String(title)));
    }
    if !artist.is_empty() {
        tags.push(mk("Artist", "Artist", Value::String(artist)));
    }
    if !album.is_empty() {
        tags.push(mk("Album", "Album", Value::String(album)));
    }
    if !year.is_empty() {
        tags.push(mk("Year", "Year", Value::String(year)));
    }

    // ID3v1.1: if byte 125 is 0 and byte 126 is non-zero, byte 126 is track number
    if data[125] == 0 && data[126] != 0 {
        tags.push(mk("Track", "Track", Value::U8(data[126])));
        let short_comment = latin1_string(&data[97..125]);
        if !short_comment.is_empty() {
            tags.push(mk("Comment", "Comment", Value::String(short_comment)));
        }
    } else if !comment.is_empty() {
        tags.push(mk("Comment", "Comment", Value::String(comment)));
    }

    if genre_idx < GENRES.len() {
        tags.push(mk(
            "Genre",
            "Genre",
            Value::String(GENRES[genre_idx].to_string()),
        ));
    }

    tags
}

fn latin1_string(data: &[u8]) -> String {
    data.iter()
        .map(|&b| b as char)
        .collect::<String>()
        .trim()
        .trim_end_matches('\0')
        .to_string()
}

/// Parse MPEG audio frame header to extract bitrate, sample rate, etc.
/// Matches Perl's MPEG.pm ProcessFrameHeader and MPEG::Audio tag table.
///
/// The 32-bit header word has bits numbered from MSB (bit 0) to LSB (bit 31):
///   Bits  0-10: sync (0xFFE)
///   Bits 11-12: MPEG version  (word >> 19 & 3)
///   Bits 13-14: Layer         (word >> 17 & 3)
///   Bit  15:    CRC protection
///   Bits 16-19: Bitrate index  (word >> 12 & 0xF)
///   Bits 20-21: Sample rate    (word >> 10 & 3)
///   Bit  22:    Padding
///   Bit  23:    Private
///   Bits 24-25: Channel mode   (word >> 6 & 3)
///   Bit  26:    MSStereo       (word >> 5 & 1)  [layer 3 only]
///   Bit  27:    IntensityStereo(word >> 4 & 1)  [layer 3 only]
///   Bits 28:    CopyrightFlag  (word >> 3 & 1)
///   Bits 29:    OriginalMedia  (word >> 2 & 1)
///   Bits 30-31: Emphasis       (word & 3)
fn parse_mpeg_header(data: &[u8], start: usize) -> Option<Vec<Tag>> {
    // Scan for MPEG sync word (11 bits: 0xFFE0)
    let mut pos = start;
    while pos + 4 <= data.len() {
        if data[pos] == 0xFF && (data[pos + 1] & 0xE0) == 0xE0 {
            break;
        }
        pos += 1;
        if pos > start + 4096 {
            return None; // Don't scan too far
        }
    }

    if pos + 4 > data.len() {
        return None;
    }

    let header = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
    let version = (header >> 19) & 3;
    let layer = (header >> 17) & 3;
    let bitrate_idx = ((header >> 12) & 0xF) as usize;
    let samplerate_idx = ((header >> 10) & 3) as usize;
    let channel_mode = (header >> 6) & 3;
    let ms_stereo = (header >> 5) & 1;
    let intensity_stereo = (header >> 4) & 1;
    let copyright_flag = (header >> 3) & 1;
    let original_media = (header >> 2) & 1;
    let emphasis = header & 3;

    let version_str = match version {
        0 => "2.5",
        2 => "2",
        3 => "1",
        _ => return None,
    };

    let layer_num = match layer {
        1 => 3u32,
        2 => 2,
        3 => 1,
        _ => return None,
    };

    // Perl emits MPEGAudioVersion as just the number (1, 2, or 2.5)
    let audio_version_str = version_str.to_string();

    // AudioLayer: the layer number (1, 2, or 3)
    let audio_layer = layer_num;

    // Bitrate table indexed by (version_bits, layer_bits, bitrate_idx)
    // Matching Perl's MPEG.pm Audio table exactly
    let bitrate = if version == 3 && layer == 3 {
        // MPEG1 Layer 1
        [
            0u32, 32000, 64000, 96000, 128000, 160000, 192000, 224000, 256000, 288000, 320000,
            352000, 384000, 416000, 448000, 0,
        ]
        .get(bitrate_idx)
        .copied()?
    } else if version == 3 && layer == 2 {
        // MPEG1 Layer 2
        [
            0u32, 32000, 48000, 56000, 64000, 80000, 96000, 112000, 128000, 160000, 192000, 224000,
            256000, 320000, 384000, 0,
        ]
        .get(bitrate_idx)
        .copied()?
    } else if version == 3 && layer == 1 {
        // MPEG1 Layer 3
        [
            0u32, 32000, 40000, 48000, 56000, 64000, 80000, 96000, 112000, 128000, 160000, 192000,
            224000, 256000, 320000, 0,
        ]
        .get(bitrate_idx)
        .copied()?
    } else if (version == 0 || version == 2) && layer == 3 {
        // MPEG2/2.5 Layer 1
        [
            0u32, 32000, 48000, 56000, 64000, 80000, 96000, 112000, 128000, 144000, 160000, 176000,
            192000, 224000, 256000, 0,
        ]
        .get(bitrate_idx)
        .copied()?
    } else if (version == 0 || version == 2) && (layer == 1 || layer == 2) {
        // MPEG2/2.5 Layer 2 or 3
        [
            0u32, 8000, 16000, 24000, 32000, 40000, 48000, 56000, 64000, 80000, 96000, 112000,
            128000, 144000, 160000, 0,
        ]
        .get(bitrate_idx)
        .copied()?
    } else {
        0
    };

    let sample_rate = if version == 3 {
        [44100u32, 48000, 32000, 0].get(samplerate_idx).copied()?
    } else if version == 2 {
        [22050u32, 24000, 16000, 0].get(samplerate_idx).copied()?
    } else {
        [11025u32, 12000, 8000, 0].get(samplerate_idx).copied()?
    };

    let channel_str = match channel_mode {
        0 => "Stereo",
        1 => "Joint Stereo",
        2 => "Dual Channel",
        3 => "Single Channel",
        _ => "Unknown",
    };

    let mut tags = Vec::new();

    // MPEGAudioVersion: Perl prints just the number (1, 2, or 2.5)
    tags.push(mk(
        "MPEGAudioVersion",
        "MPEG Audio Version",
        Value::String(audio_version_str),
    ));

    // AudioLayer: the layer number
    tags.push(mk("AudioLayer", "Audio Layer", Value::U32(audio_layer)));

    if bitrate > 0 {
        // Perl: "128 kbps" format via ConvertBitrate
        tags.push(mk(
            "AudioBitrate",
            "Audio Bitrate",
            Value::String(format!("{} kbps", bitrate / 1000)),
        ));
    }
    if sample_rate > 0 {
        tags.push(mk("SampleRate", "Sample Rate", Value::U32(sample_rate)));
    }
    tags.push(mk(
        "ChannelMode",
        "Channel Mode",
        Value::String(channel_str.into()),
    ));

    // MSStereo and IntensityStereo: only for layer 3 (layer_num == 3)
    if layer_num == 3 {
        tags.push(mk(
            "MSStereo",
            "MS Stereo",
            Value::String(if ms_stereo != 0 { "On" } else { "Off" }.into()),
        ));
        tags.push(mk(
            "IntensityStereo",
            "Intensity Stereo",
            Value::String(if intensity_stereo != 0 { "On" } else { "Off" }.into()),
        ));
    }

    // CopyrightFlag
    tags.push(mk(
        "CopyrightFlag",
        "Copyright Flag",
        Value::String(if copyright_flag != 0 { "True" } else { "False" }.into()),
    ));

    // OriginalMedia
    tags.push(mk(
        "OriginalMedia",
        "Original Media",
        Value::String(if original_media != 0 { "True" } else { "False" }.into()),
    ));

    // Emphasis
    let emphasis_str = match emphasis {
        0 => "None",
        1 => "50/15 ms",
        2 => "reserved",
        3 => "CCIT J.17",
        _ => "None",
    };
    tags.push(mk(
        "Emphasis",
        "Emphasis",
        Value::String(emphasis_str.into()),
    ));

    Some(tags)
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let print_value = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "ID3".into(),
            family1: "ID3".into(),
            family2: "Audio".into(),
        },
        raw_value: value,
        print_value,
        priority: 0,
    }
}

/// ID3v1 genre list (index 0-191). Matches Perl's %genre hash.
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
