//! QuickTime/MP4/M4A/MOV file format reader.
//!
//! Parses ISO Base Media File Format (ISOBMFF) atom/box tree to extract
//! metadata from moov/udta/meta/ilst and embedded EXIF/XMP.
//! Mirrors ExifTool's QuickTime.pm.

use crate::error::{Error, Result};
use crate::metadata::XmpReader;
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_quicktime(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 8 {
        return Err(Error::InvalidData("file too small for QuickTime".into()));
    }

    let mut tags = Vec::new();

    // Check for ftyp
    if data.len() >= 12 && &data[4..8] == b"ftyp" {
        let size = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
        let brand = String::from_utf8_lossy(&data[8..12]).to_string();
        tags.push(mk("MajorBrand", "Major Brand", Value::String(brand)));
        if size >= 16 && data.len() >= 16 {
            let minor_ver = u32::from_be_bytes([data[12], data[13], data[14], data[15]]);
            tags.push(mk("MinorVersion", "Minor Version", Value::U32(minor_ver)));
        }
        // Compatible brands
        if size > 16 {
            let mut brands = Vec::new();
            let mut pos = 16;
            while pos + 4 <= size.min(data.len()) {
                let b = String::from_utf8_lossy(&data[pos..pos + 4]).trim().to_string();
                if !b.is_empty() {
                    brands.push(b);
                }
                pos += 4;
            }
            if !brands.is_empty() {
                tags.push(mk(
                    "CompatibleBrands",
                    "Compatible Brands",
                    Value::String(brands.join(", ")),
                ));
            }
        }
    }

    // Parse atom tree
    parse_atoms(data, 0, data.len(), &mut tags, 0);

    Ok(tags)
}

/// Recursively parse QuickTime atoms.
fn parse_atoms(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>, depth: u32) {
    if depth > 20 {
        return; // Prevent infinite recursion
    }

    let mut pos = start;

    while pos + 8 <= end {
        let mut size = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as u64;
        let atom_type = &data[pos + 4..pos + 8];
        let header_size;

        if size == 1 && pos + 16 <= end {
            // Extended size (64-bit)
            size = u64::from_be_bytes([
                data[pos + 8], data[pos + 9], data[pos + 10], data[pos + 11],
                data[pos + 12], data[pos + 13], data[pos + 14], data[pos + 15],
            ]);
            header_size = 16;
        } else if size == 0 {
            // Atom extends to end of file
            size = (end - pos) as u64;
            header_size = 8;
        } else {
            header_size = 8;
        }

        let atom_end = (pos as u64 + size) as usize;
        if atom_end > end || size < header_size as u64 {
            break;
        }

        let content_start = pos + header_size;
        let content_end = atom_end;

        match atom_type {
            // Container atoms - recurse into
            b"moov" | b"trak" | b"mdia" | b"minf" | b"stbl" | b"dinf" | b"edts" => {
                parse_atoms(data, content_start, content_end, tags, depth + 1);
            }
            b"udta" => {
                parse_atoms(data, content_start, content_end, tags, depth + 1);
            }
            // Metadata container
            b"meta" => {
                // meta has a 4-byte version/flags before sub-atoms
                if content_start + 4 <= content_end {
                    parse_atoms(data, content_start + 4, content_end, tags, depth + 1);
                }
            }
            // iTunes item list
            b"ilst" => {
                parse_ilst(data, content_start, content_end, tags);
            }
            // Movie header
            b"mvhd" => {
                parse_mvhd(data, content_start, content_end, tags);
            }
            // Track header
            b"tkhd" => {
                parse_tkhd(data, content_start, content_end, tags);
            }
            // Media header
            b"mdhd" => {
                parse_mdhd(data, content_start, content_end, tags);
            }
            // Handler reference
            b"hdlr" => {
                parse_hdlr(data, content_start, content_end, tags);
            }
            // Sample description
            b"stsd" => {
                parse_stsd(data, content_start, content_end, tags);
            }
            // XMP metadata (uuid box)
            b"uuid" => {
                // XMP UUID: BE7ACFCB97A942E89C71999491E3AFAC
                if content_end - content_start > 16 {
                    let uuid = &data[content_start..content_start + 16];
                    if uuid[0] == 0xBE && uuid[1] == 0x7A && uuid[2] == 0xCF && uuid[3] == 0xCB {
                        let xmp_data = &data[content_start + 16..content_end];
                        if let Ok(xmp_tags) = XmpReader::read(xmp_data) {
                            tags.extend(xmp_tags);
                        }
                    }
                }
            }
            // QuickTime text atoms (©xxx)
            _ if atom_type[0] == 0xA9 => {
                parse_qt_text_atom(atom_type, data, content_start, content_end, tags);
            }
            _ => {}
        }

        pos = atom_end;
    }
}

/// Parse movie header (mvhd) atom.
fn parse_mvhd(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>) {
    if start + 4 > end {
        return;
    }

    let version = data[start];
    let d = &data[start + 4..end];

    if version == 0 && d.len() >= 96 {
        let creation_time = u32::from_be_bytes([d[0], d[1], d[2], d[3]]);
        let modification_time = u32::from_be_bytes([d[4], d[5], d[6], d[7]]);
        let timescale = u32::from_be_bytes([d[8], d[9], d[10], d[11]]);
        let duration = u32::from_be_bytes([d[12], d[13], d[14], d[15]]);

        if let Some(dt) = mac_epoch_to_string(creation_time as u64) {
            tags.push(mk("CreateDate", "Create Date", Value::String(dt)));
        }
        if let Some(dt) = mac_epoch_to_string(modification_time as u64) {
            tags.push(mk("ModifyDate", "Modify Date", Value::String(dt)));
        }
        tags.push(mk("TimeScale", "Time Scale", Value::U32(timescale)));

        if timescale > 0 {
            let dur_secs = duration as f64 / timescale as f64;
            tags.push(mk(
                "Duration",
                "Duration",
                Value::String(format_duration(dur_secs)),
            ));
        }
    } else if version == 1 && d.len() >= 108 {
        let creation_time = u64::from_be_bytes([d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]]);
        let modification_time = u64::from_be_bytes([d[8], d[9], d[10], d[11], d[12], d[13], d[14], d[15]]);
        let timescale = u32::from_be_bytes([d[16], d[17], d[18], d[19]]);
        let duration = u64::from_be_bytes([d[20], d[21], d[22], d[23], d[24], d[25], d[26], d[27]]);

        if let Some(dt) = mac_epoch_to_string(creation_time) {
            tags.push(mk("CreateDate", "Create Date", Value::String(dt)));
        }
        if let Some(dt) = mac_epoch_to_string(modification_time) {
            tags.push(mk("ModifyDate", "Modify Date", Value::String(dt)));
        }
        tags.push(mk("TimeScale", "Time Scale", Value::U32(timescale)));

        if timescale > 0 {
            let dur_secs = duration as f64 / timescale as f64;
            tags.push(mk("Duration", "Duration", Value::String(format_duration(dur_secs))));
        }
    }
}

/// Parse track header (tkhd).
fn parse_tkhd(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>) {
    if start + 4 > end {
        return;
    }
    let version = data[start];
    let d = &data[start + 4..end];

    if version == 0 && d.len() >= 80 {
        // Width and height at offset 76 (fixed-point 16.16)
        let width_fp = u32::from_be_bytes([d[76], d[77], d[78], d[79]]);
        let height_fp = u32::from_be_bytes([d[80.min(d.len() - 4)], d[81.min(d.len() - 3)], d[82.min(d.len() - 2)], d[83.min(d.len() - 1)]]);
        let width = width_fp >> 16;
        let height = height_fp >> 16;

        if width > 0 && height > 0 {
            tags.push(mk("ImageWidth", "Image Width", Value::U32(width)));
            tags.push(mk("ImageHeight", "Image Height", Value::U32(height)));
        }
    }
}

/// Parse media header (mdhd).
fn parse_mdhd(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>) {
    if start + 4 > end {
        return;
    }
    let version = data[start];
    let d = &data[start + 4..end];

    if version == 0 && d.len() >= 20 {
        let timescale = u32::from_be_bytes([d[8], d[9], d[10], d[11]]);
        let duration = u32::from_be_bytes([d[12], d[13], d[14], d[15]]);
        let lang_code = u16::from_be_bytes([d[16], d[17]]);

        tags.push(mk("MediaTimeScale", "Media Time Scale", Value::U32(timescale)));

        if timescale > 0 && duration > 0 {
            let dur_secs = duration as f64 / timescale as f64;
            tags.push(mk("MediaDuration", "Media Duration", Value::String(format_duration(dur_secs))));
        }

        // Decode packed ISO 639-2 language code
        if lang_code != 0 && lang_code != 0x7FFF {
            let c1 = ((lang_code >> 10) & 0x1F) as u8 + 0x60;
            let c2 = ((lang_code >> 5) & 0x1F) as u8 + 0x60;
            let c3 = (lang_code & 0x1F) as u8 + 0x60;
            if c1.is_ascii_lowercase() && c2.is_ascii_lowercase() && c3.is_ascii_lowercase() {
                let lang = format!("{}{}{}", c1 as char, c2 as char, c3 as char);
                tags.push(mk("MediaLanguage", "Media Language", Value::String(lang)));
            }
        }
    }
}

/// Parse handler reference (hdlr).
fn parse_hdlr(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>) {
    if start + 12 > end {
        return;
    }
    let d = &data[start + 4..end];
    if d.len() >= 8 {
        let handler_type = String::from_utf8_lossy(&d[4..8]).trim().to_string();
        let handler_name = match handler_type.as_str() {
            "vide" => "Video",
            "soun" => "Audio",
            "text" | "sbtl" => "Subtitle",
            "meta" => "Metadata",
            "hint" => "Hint",
            "tmcd" => "Timecode",
            _ => &handler_type,
        };
        tags.push(mk("HandlerType", "Handler Type", Value::String(handler_name.to_string())));
    }
}

/// Parse sample description (stsd) for codec info.
fn parse_stsd(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>) {
    if start + 8 > end {
        return;
    }
    let d = &data[start..end];
    if d.len() < 16 {
        return;
    }
    // version (4) + entry count (4) + first entry
    let _entry_count = u32::from_be_bytes([d[4], d[5], d[6], d[7]]);

    // First sample entry: size(4) + format(4)
    if d.len() >= 16 {
        let codec = String::from_utf8_lossy(&d[12..16]).trim().to_string();
        if !codec.is_empty() {
            let codec_name = match codec.as_str() {
                "avc1" | "avc3" => "H.264/AVC",
                "hvc1" | "hev1" => "H.265/HEVC",
                "vp08" => "VP8",
                "vp09" => "VP9",
                "av01" => "AV1",
                "mp4a" => "AAC",
                "mp4v" => "MPEG-4 Visual",
                "ac-3" => "AC-3",
                "ec-3" => "E-AC-3",
                "alac" => "Apple Lossless",
                "Opus" => "Opus",
                ".mp3" => "MP3",
                "sowt" => "PCM (Little-endian)",
                "twos" => "PCM (Big-endian)",
                "lpcm" => "LPCM",
                "jpeg" => "JPEG",
                _ => &codec,
            };
            tags.push(mk("CompressorID", "Compressor ID", Value::String(codec_name.to_string())));
        }
    }
}

/// Parse iTunes metadata item list (ilst).
fn parse_ilst(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>) {
    let mut pos = start;

    while pos + 8 <= end {
        let item_size = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let item_type = &data[pos + 4..pos + 8];
        let item_end = pos + item_size;

        if item_size < 8 || item_end > end {
            break;
        }

        // Find the 'data' atom inside this item
        if let Some(value) = find_data_atom(data, pos + 8, item_end) {
            let (name, description) = ilst_tag_name(item_type);
            if !name.is_empty() {
                tags.push(mk(name, description, Value::String(value)));
            }
        }

        pos = item_end;
    }
}

/// Find and decode the 'data' atom inside an ilst item.
fn find_data_atom(data: &[u8], start: usize, end: usize) -> Option<String> {
    let mut pos = start;

    while pos + 16 <= end {
        let size = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let atom_type = &data[pos + 4..pos + 8];

        if size < 16 || pos + size > end {
            break;
        }

        if atom_type == b"data" {
            let data_type = u32::from_be_bytes([data[pos + 8], data[pos + 9], data[pos + 10], data[pos + 11]]);
            let value_data = &data[pos + 16..pos + size];

            return Some(match data_type & 0xFF {
                1 => {
                    // UTF-8
                    String::from_utf8_lossy(value_data).to_string()
                }
                2 => {
                    // UTF-16
                    let units: Vec<u16> = value_data
                        .chunks_exact(2)
                        .map(|c| u16::from_be_bytes([c[0], c[1]]))
                        .collect();
                    String::from_utf16_lossy(&units)
                }
                13 | 14 => {
                    // JPEG / PNG cover art
                    format!("(Binary data {} bytes)", value_data.len())
                }
                21 => {
                    // Signed integer
                    match value_data.len() {
                        1 => (value_data[0] as i8).to_string(),
                        2 => i16::from_be_bytes([value_data[0], value_data[1]]).to_string(),
                        4 => i32::from_be_bytes([value_data[0], value_data[1], value_data[2], value_data[3]]).to_string(),
                        8 => i64::from_be_bytes([
                            value_data[0], value_data[1], value_data[2], value_data[3],
                            value_data[4], value_data[5], value_data[6], value_data[7],
                        ]).to_string(),
                        _ => format!("(Signed {} bytes)", value_data.len()),
                    }
                }
                22 => {
                    // Unsigned integer
                    match value_data.len() {
                        1 => value_data[0].to_string(),
                        2 => u16::from_be_bytes([value_data[0], value_data[1]]).to_string(),
                        4 => u32::from_be_bytes([value_data[0], value_data[1], value_data[2], value_data[3]]).to_string(),
                        _ => format!("(Unsigned {} bytes)", value_data.len()),
                    }
                }
                0 => {
                    // Implicit (binary) - try to decode as track number etc.
                    if value_data.len() >= 4 {
                        // Track number format: 0x0000 + track(2) + total(2)
                        let track = u16::from_be_bytes([value_data[2], value_data[3]]);
                        if value_data.len() >= 6 {
                            let total = u16::from_be_bytes([value_data[4], value_data[5]]);
                            if total > 0 {
                                format!("{} of {}", track, total)
                            } else {
                                track.to_string()
                            }
                        } else {
                            track.to_string()
                        }
                    } else {
                        format!("(Binary {} bytes)", value_data.len())
                    }
                }
                _ => String::from_utf8_lossy(value_data).to_string(),
            });
        }

        pos += size;
    }

    None
}

/// Parse QuickTime text atom (©xxx at container level).
fn parse_qt_text_atom(
    atom_type: &[u8],
    data: &[u8],
    start: usize,
    end: usize,
    tags: &mut Vec<Tag>,
) {
    if start + 4 > end {
        return;
    }

    // QuickTime text: 2-byte text length + 2-byte language + text
    let text_len = u16::from_be_bytes([data[start], data[start + 1]]) as usize;
    let text_start = start + 4;

    if text_start + text_len <= end {
        let text = String::from_utf8_lossy(&data[text_start..text_start + text_len])
            .trim_end_matches('\0')
            .to_string();
        if !text.is_empty() {
            let key = String::from_utf8_lossy(&atom_type[1..4]).to_string();
            let (name, description) = qt_text_name(&key);
            tags.push(mk(name, description, Value::String(text)));
        }
    }
}

/// Map ilst item types to tag names.
fn ilst_tag_name(item_type: &[u8]) -> (&str, &str) {
    match item_type {
        b"\xa9nam" => ("Title", "Title"),
        b"\xa9ART" => ("Artist", "Artist"),
        b"\xa9alb" => ("Album", "Album"),
        b"\xa9day" => ("Year", "Year"),
        b"\xa9cmt" => ("Comment", "Comment"),
        b"\xa9gen" => ("Genre", "Genre"),
        b"\xa9wrt" => ("Composer", "Composer"),
        b"\xa9too" => ("Encoder", "Encoder"),
        b"\xa9grp" => ("Grouping", "Grouping"),
        b"\xa9lyr" => ("Lyrics", "Lyrics"),
        b"\xa9des" => ("Description", "Description"),
        b"trkn" => ("TrackNumber", "Track Number"),
        b"disk" => ("DiscNumber", "Disc Number"),
        b"tmpo" => ("BPM", "BPM"),
        b"cpil" => ("Compilation", "Compilation"),
        b"pgap" => ("Gapless", "Gapless Playback"),
        b"covr" => ("CoverArt", "Cover Art"),
        b"aART" => ("AlbumArtist", "Album Artist"),
        b"cprt" => ("Copyright", "Copyright"),
        b"desc" => ("Description", "Description"),
        b"ldes" => ("LongDescription", "Long Description"),
        b"tvsh" => ("TVShow", "TV Show"),
        b"tven" => ("TVEpisodeID", "TV Episode ID"),
        b"tvsn" => ("TVSeason", "TV Season"),
        b"tves" => ("TVEpisode", "TV Episode"),
        b"purd" => ("PurchaseDate", "Purchase Date"),
        b"stik" => ("MediaType", "Media Type"),
        b"rtng" => ("Rating", "Rating"),
        _ => {
            if item_type[0] == 0xA9 {
                // Unknown © tag
                return ("", "");
            }
            ("", "")
        }
    }
}

/// Map QuickTime text atom keys to tag names.
fn qt_text_name(key: &str) -> (&str, &str) {
    match key {
        "nam" => ("Title", "Title"),
        "ART" => ("Artist", "Artist"),
        "alb" => ("Album", "Album"),
        "day" => ("Year", "Year"),
        "cmt" => ("Comment", "Comment"),
        "gen" => ("Genre", "Genre"),
        "wrt" => ("Composer", "Composer"),
        "too" => ("Encoder", "Encoder"),
        "inf" => ("Information", "Information"),
        "req" => ("Requirements", "Requirements"),
        "fmt" => ("Format", "Format"),
        "dir" => ("Director", "Director"),
        "prd" => ("Producer", "Producer"),
        "prf" => ("Performer", "Performer"),
        "src" => ("Source", "Source"),
        "swr" => ("SoftwareVersion", "Software Version"),
        _ => (key, key),
    }
}

/// Convert Mac epoch (seconds since 1904-01-01) to date string.
fn mac_epoch_to_string(secs: u64) -> Option<String> {
    if secs == 0 {
        return None;
    }
    // Mac epoch is 66 years + 17 leap days before Unix epoch
    let unix_secs = secs as i64 - 2082844800;
    if unix_secs < 0 {
        return None;
    }

    // Simple date conversion (approximate, good enough for display)
    let days = unix_secs / 86400;
    let time_of_day = unix_secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Days since 1970-01-01
    let mut y = 1970i32;
    let mut remaining_days = days;

    loop {
        let days_in_year = if is_leap_year(y) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        y += 1;
    }

    let months = [31, if is_leap_year(y) { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 1;
    for &days_in_month in &months {
        if remaining_days < days_in_month {
            break;
        }
        remaining_days -= days_in_month;
        m += 1;
    }
    let d = remaining_days + 1;

    Some(format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}", y, m, d, hours, minutes, seconds))
}

fn is_leap_year(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn format_duration(seconds: f64) -> String {
    let hours = (seconds / 3600.0) as u32;
    let minutes = ((seconds % 3600.0) / 60.0) as u32;
    let secs = seconds % 60.0;

    if hours > 0 {
        format!("{}:{:02}:{:05.2}", hours, minutes, secs)
    } else {
        format!("{}:{:05.2}", minutes, secs)
    }
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let print_value = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "QuickTime".into(),
            family1: "QuickTime".into(),
            family2: "Video".into(),
        },
        raw_value: value,
        print_value,
        priority: 0,
    }
}
