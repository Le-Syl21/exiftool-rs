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

    // Try ID3v2 at start
    if data.len() >= 10 && data.starts_with(b"ID3") {
        let id3v2_tags = read_id3v2(data)?;
        tags.extend(id3v2_tags);
    }

    // Try ID3v1 at end (last 128 bytes)
    if data.len() >= 128 {
        let v1_start = data.len() - 128;
        if &data[v1_start..v1_start + 3] == b"TAG" {
            let id3v1_tags = read_id3v1(&data[v1_start..]);
            // Only add v1 tags not already present from v2
            for t in id3v1_tags {
                if !tags.iter().any(|existing| existing.name == t.name) {
                    tags.push(t);
                }
            }
        }
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
    tags.push(mk(
        "ID3Version",
        "ID3 Version",
        Value::String(format!("2.{}.{}", version, _revision)),
    ));

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
            let frame_size =
                ((data[pos + 3] as usize) << 16) | ((data[pos + 4] as usize) << 8) | data[pos + 5] as usize;
            pos += 6;
            if frame_size == 0 || pos + frame_size > end {
                break;
            }
            let frame_data = &data[pos..pos + frame_size];
            if let Some(tag) = decode_id3v2_frame_22(frame_id, frame_data) {
                tags.push(tag);
            }
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
                u32::from_be_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]) as usize
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

/// Decode ID3v2.2 frame (3-char IDs).
fn decode_id3v2_frame_22(frame_id: &[u8], data: &[u8]) -> Option<Tag> {
    let id = std::str::from_utf8(frame_id).ok()?;
    let (name, description) = match id {
        "TT2" => ("Title", "Title"),
        "TP1" => ("Artist", "Artist"),
        "TAL" => ("Album", "Album"),
        "TRK" => ("Track", "Track"),
        "TYE" => ("Year", "Year"),
        "TCO" => ("Genre", "Genre"),
        "COM" => ("Comment", "Comment"),
        "TEN" => ("EncodedBy", "Encoded By"),
        "TCM" => ("Composer", "Composer"),
        "TT1" => ("ContentGroup", "Content Group"),
        "TP2" => ("AlbumArtist", "Album Artist"),
        "TPA" => ("PartOfSet", "Part of Set"),
        _ => return None,
    };

    if id == "COM" {
        return decode_comment_frame(name, description, data);
    }

    let text = decode_id3_text(data)?;
    Some(mk(name, description, Value::String(text)))
}

/// Decode ID3v2.3/v2.4 frame (4-char IDs).
fn decode_id3v2_frame(frame_id: &[u8], data: &[u8]) -> Option<Tag> {
    let id = std::str::from_utf8(frame_id).ok()?;
    let (name, description) = match id {
        "TIT1" => ("ContentGroup", "Content Group"),
        "TIT2" => ("Title", "Title"),
        "TIT3" => ("Subtitle", "Subtitle"),
        "TPE1" => ("Artist", "Artist"),
        "TPE2" => ("AlbumArtist", "Album Artist"),
        "TPE3" => ("Conductor", "Conductor"),
        "TPE4" => ("InterpretedBy", "Interpreted By"),
        "TALB" => ("Album", "Album"),
        "TRCK" => ("Track", "Track"),
        "TPOS" => ("PartOfSet", "Part of Set"),
        "TYER" | "TDRC" => ("Year", "Year"),
        "TCON" => ("Genre", "Genre"),
        "TCOM" => ("Composer", "Composer"),
        "TENC" => ("EncodedBy", "Encoded By"),
        "TBPM" => ("BeatsPerMinute", "BPM"),
        "TLEN" => ("Duration", "Duration"),
        "TPUB" => ("Publisher", "Publisher"),
        "TLAN" => ("Language", "Language"),
        "TCOP" => ("Copyright", "Copyright"),
        "TSSE" => ("EncoderSettings", "Encoder Settings"),
        "TSRC" => ("ISRC", "ISRC"),
        "COMM" => return decode_comment_frame("Comment", "Comment", data),
        "USLT" => return decode_comment_frame("Lyrics", "Lyrics", data),
        "APIC" => return decode_picture_frame(data),
        "TXXX" => return decode_txxx_frame(data),
        "WOAR" => {
            let url = String::from_utf8_lossy(data).trim_end_matches('\0').to_string();
            return Some(mk("ArtistURL", "Artist URL", Value::String(url)));
        }
        "WXXX" => return decode_wxxx_frame(data),
        "PCNT" => {
            if data.len() >= 4 {
                let count = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                return Some(mk("PlayCount", "Play Count", Value::U32(count)));
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
            String::from_utf8_lossy(text_data).to_string()
        }
        _ => String::from_utf8_lossy(text_data).to_string(),
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

/// Decode COMM/USLT frame (comment/lyrics with language).
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

/// Decode APIC picture frame.
fn decode_picture_frame(data: &[u8]) -> Option<Tag> {
    if data.len() < 4 {
        return None;
    }
    let encoding = data[0];
    let rest = &data[1..];

    // MIME type (Latin-1, null-terminated)
    let null_pos = rest.iter().position(|&b| b == 0)?;
    let mime = String::from_utf8_lossy(&rest[..null_pos]).to_string();
    let rest = &rest[null_pos + 1..];

    if rest.is_empty() {
        return None;
    }
    let pic_type = rest[0];
    let rest = &rest[1..];

    // Description (encoded)
    let (_, image_data) = split_encoded_string(rest, encoding);

    let pic_type_str = match pic_type {
        0 => "Other",
        1 => "32x32 Icon",
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
        13 => "Recording Location",
        14 => "During Recording",
        15 => "During Performance",
        16 => "Movie Capture",
        17 => "Bright Fish",
        18 => "Illustration",
        19 => "Band Logo",
        20 => "Publisher Logo",
        _ => "Unknown",
    };

    Some(mk(
        "Picture",
        "Picture",
        Value::String(format!(
            "({}, {}, {} bytes)",
            pic_type_str,
            mime,
            image_data.len()
        )),
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
    let url = String::from_utf8_lossy(url_bytes).trim_end_matches('\0').to_string();
    let desc = desc.trim_end_matches('\0');
    let name = if desc.is_empty() { "UserURL" } else { desc };

    Some(mk(name, name, Value::String(url)))
}

/// Decode POPM (Popularimeter) frame.
fn decode_popularity_frame(data: &[u8]) -> Option<Tag> {
    let null_pos = data.iter().position(|&b| b == 0)?;
    let email = String::from_utf8_lossy(&data[..null_pos]).to_string();
    let rest = &data[null_pos + 1..];
    if rest.is_empty() {
        return None;
    }
    let rating = rest[0];
    Some(mk(
        "Popularimeter",
        "Popularimeter",
        Value::String(format!("{}: {}/255", email, rating)),
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
        3 => String::from_utf8_lossy(data).to_string(),
        _ => String::from_utf8_lossy(data).to_string(),
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

    if !title.is_empty() { tags.push(mk("Title", "Title", Value::String(title))); }
    if !artist.is_empty() { tags.push(mk("Artist", "Artist", Value::String(artist))); }
    if !album.is_empty() { tags.push(mk("Album", "Album", Value::String(album))); }
    if !year.is_empty() { tags.push(mk("Year", "Year", Value::String(year))); }

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
        tags.push(mk("Genre", "Genre", Value::String(GENRES[genre_idx].to_string())));
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

    let version_str = match version {
        0 => "2.5",
        2 => "2",
        3 => "1",
        _ => return None,
    };

    let layer_str = match layer {
        1 => "3",
        2 => "2",
        3 => "1",
        _ => return None,
    };

    // Bitrate table for MPEG1 Layer 3
    let bitrate = if version == 3 && layer == 1 {
        // MPEG1 Layer 3
        [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0]
            .get(bitrate_idx)
            .copied()?
    } else if (version == 0 || version == 2) && layer == 1 {
        // MPEG2/2.5 Layer 3
        [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0]
            .get(bitrate_idx)
            .copied()?
    } else {
        0
    };

    let sample_rate = if version == 3 {
        [44100, 48000, 32000, 0].get(samplerate_idx).copied()?
    } else if version == 2 {
        [22050, 24000, 16000, 0].get(samplerate_idx).copied()?
    } else {
        [11025, 12000, 8000, 0].get(samplerate_idx).copied()?
    };

    let channel_str = match channel_mode {
        0 => "Stereo",
        1 => "Joint Stereo",
        2 => "Dual Channel",
        3 => "Mono",
        _ => "Unknown",
    };

    let mut tags = Vec::new();
    tags.push(mk(
        "MPEGAudioVersion",
        "MPEG Audio Version",
        Value::String(format!("MPEG{} Layer {}", version_str, layer_str)),
    ));
    if bitrate > 0 {
        tags.push(mk(
            "AudioBitrate",
            "Audio Bitrate",
            Value::String(format!("{} kbps", bitrate)),
        ));
    }
    if sample_rate > 0 {
        tags.push(mk(
            "SampleRate",
            "Sample Rate",
            Value::U32(sample_rate),
        ));
    }
    tags.push(mk(
        "ChannelMode",
        "Channel Mode",
        Value::String(channel_str.into()),
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

/// ID3v1 genre list (index 0-191).
static GENRES: &[&str] = &[
    "Blues", "Classic Rock", "Country", "Dance", "Disco", "Funk", "Grunge",
    "Hip-Hop", "Jazz", "Metal", "New Age", "Oldies", "Other", "Pop", "R&B",
    "Rap", "Reggae", "Rock", "Techno", "Industrial", "Alternative", "Ska",
    "Death Metal", "Pranks", "Soundtrack", "Euro-Techno", "Ambient",
    "Trip-Hop", "Vocal", "Jazz+Funk", "Fusion", "Trance", "Classical",
    "Instrumental", "Acid", "House", "Game", "Sound Clip", "Gospel", "Noise",
    "AlternRock", "Bass", "Soul", "Punk", "Space", "Meditative",
    "Instrumental Pop", "Instrumental Rock", "Ethnic", "Gothic", "Darkwave",
    "Techno-Industrial", "Electronic", "Pop-Folk", "Eurodance", "Dream",
    "Southern Rock", "Comedy", "Cult", "Gangsta", "Top 40", "Christian Rap",
    "Pop/Funk", "Jungle", "Native American", "Cabaret", "New Wave",
    "Psychedelic", "Rave", "Showtunes", "Trailer", "Lo-Fi", "Tribal",
    "Acid Punk", "Acid Jazz", "Polka", "Retro", "Musical", "Rock & Roll",
    "Hard Rock", "Folk", "Folk-Rock", "National Folk", "Swing", "Fast Fusion",
    "Bebop", "Latin", "Revival", "Celtic", "Bluegrass", "Avantgarde",
    "Gothic Rock", "Progressive Rock", "Psychedelic Rock", "Symphonic Rock",
    "Slow Rock", "Big Band", "Chorus", "Easy Listening", "Acoustic", "Humour",
    "Speech", "Chanson", "Opera", "Chamber Music", "Sonata", "Symphony",
    "Booty Bass", "Primus", "Porn Groove", "Satire", "Slow Jam", "Club",
    "Tango", "Samba", "Folklore", "Ballad", "Power Ballad", "Rhythmic Soul",
    "Freestyle", "Duet", "Punk Rock", "Drum Solo", "A capella", "Euro-House",
    "Dance Hall", "Goa", "Drum & Bass", "Club-House", "Hardcore Techno",
    "Terror", "Indie", "BritPop", "Negerpunk", "Polsk Punk", "Beat",
    "Christian Gangsta Rap", "Heavy Metal", "Black Metal", "Crossover",
    "Contemporary Christian", "Christian Rock", "Merengue", "Salsa",
    "Thrash Metal", "Anime", "JPop", "Synthpop", "Abstract", "Art Rock",
    "Baroque", "Bhangra", "Big Beat", "Breakbeat", "Chillout", "Downtempo",
    "Dub", "EBM", "Eclectic", "Electro", "Electroclash", "Emo", "Experimental",
    "Garage", "Global", "IDM", "Illbient", "Industro-Goth", "Jam Band",
    "Krautrock", "Leftfield", "Lounge", "Math Rock", "New Romantic",
    "Nu-Breakz", "Post-Punk", "Post-Rock", "Psytrance", "Shoegaze",
    "Space Rock", "Trop Rock", "World Music", "Neoclassical", "Audiobook",
    "Audio Theatre", "Neue Deutsche Welle", "Podcast", "Indie Rock",
    "G-Funk", "Dubstep", "Garage Rock", "Psybient",
];
