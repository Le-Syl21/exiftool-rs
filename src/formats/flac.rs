//! FLAC file format reader.
//!
//! Parses FLAC metadata blocks: StreamInfo, VorbisComment, Picture.
//! Mirrors ExifTool's FLAC.pm.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_flac(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 8 || !data.starts_with(b"fLaC") {
        return Err(Error::InvalidData("not a FLAC file".into()));
    }

    let mut tags = Vec::new();
    let mut pos = 4;

    loop {
        if pos + 4 > data.len() {
            break;
        }

        let block_header = data[pos];
        let is_last = (block_header & 0x80) != 0;
        let block_type = block_header & 0x7F;
        let block_size = ((data[pos + 1] as usize) << 16)
            | ((data[pos + 2] as usize) << 8)
            | data[pos + 3] as usize;
        pos += 4;

        if pos + block_size > data.len() {
            break;
        }

        let block_data = &data[pos..pos + block_size];

        match block_type {
            // STREAMINFO
            0 => {
                if block_size >= 34 {
                    parse_stream_info(block_data, &mut tags);
                }
            }
            // VORBIS_COMMENT
            4 => {
                parse_vorbis_comments(block_data, &mut tags);
            }
            // PICTURE
            6 => {
                parse_flac_picture(block_data, &mut tags);
            }
            // APPLICATION (2), SEEKTABLE (3), CUESHEET (5): skip
            _ => {}
        }

        pos += block_size;
        if is_last {
            break;
        }
    }

    Ok(tags)
}

fn parse_stream_info(data: &[u8], tags: &mut Vec<Tag>) {
    // FLAC STREAMINFO block (from Perl FLAC.pm):
    // Bytes 0-1: BlockSizeMin (uint16 BE)
    // Bytes 2-3: BlockSizeMax (uint16 BE)
    // Bytes 4-6: FrameSizeMin (24 bits BE)
    // Bytes 7-9: FrameSizeMax (24 bits BE)
    // Bits 80-99: SampleRate (20 bits)
    // Bits 100-102: Channels - 1 (3 bits)
    // Bits 103-107: BitsPerSample - 1 (5 bits)
    // Bits 108-143: TotalSamples (36 bits)
    // Bytes 18-33: MD5Signature (16 bytes)

    let block_size_min = u16::from_be_bytes([data[0], data[1]]);
    let block_size_max = u16::from_be_bytes([data[2], data[3]]);
    let frame_size_min = ((data[4] as u32) << 16) | ((data[5] as u32) << 8) | data[6] as u32;
    let frame_size_max = ((data[7] as u32) << 16) | ((data[8] as u32) << 8) | data[9] as u32;

    let sample_rate = ((data[10] as u32) << 12)
        | ((data[11] as u32) << 4)
        | ((data[12] as u32) >> 4);

    let channels = ((data[12] >> 1) & 0x07) + 1;
    let bits_per_sample = (((data[12] & 0x01) as u16) << 4) | ((data[13] >> 4) as u16) + 1;

    let total_samples = (((data[13] & 0x0F) as u64) << 32)
        | ((data[14] as u64) << 24)
        | ((data[15] as u64) << 16)
        | ((data[16] as u64) << 8)
        | data[17] as u64;

    let md5 = if data.len() >= 34 {
        data[18..34].iter().map(|b| format!("{:02x}", b)).collect::<String>()
    } else {
        String::new()
    };

    tags.push(mk("BlockSizeMin", "Block Size Min", Value::U16(block_size_min)));
    tags.push(mk("BlockSizeMax", "Block Size Max", Value::U16(block_size_max)));
    tags.push(mk("FrameSizeMin", "Frame Size Min", Value::U32(frame_size_min)));
    tags.push(mk("FrameSizeMax", "Frame Size Max", Value::U32(frame_size_max)));
    tags.push(mk("SampleRate", "Sample Rate", Value::U32(sample_rate)));
    tags.push(mk("Channels", "Channels", Value::U8(channels)));
    tags.push(mk("BitsPerSample", "Bits Per Sample", Value::U16(bits_per_sample)));
    tags.push(mk("TotalSamples", "Total Samples", Value::String(total_samples.to_string())));
    if !md5.is_empty() {
        tags.push(mk("MD5Signature", "MD5 Signature", Value::String(md5)));
    }

    if total_samples > 0 && sample_rate > 0 {
        let duration = total_samples as f64 / sample_rate as f64;
        tags.push(mk(
            "Duration",
            "Duration",
            Value::String(format_duration(duration)),
        ));
    }
}

/// Parse Vorbis comments (shared format with OGG Vorbis).
pub fn parse_vorbis_comments(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 8 {
        return;
    }

    // Vendor string (little-endian length + data)
    let vendor_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let mut pos = 4;
    if pos + vendor_len > data.len() {
        return;
    }
    let vendor = String::from_utf8_lossy(&data[pos..pos + vendor_len]).to_string();
    pos += vendor_len;

    if !vendor.is_empty() {
        tags.push(mk("Vendor", "Encoder", Value::String(vendor)));
    }

    if pos + 4 > data.len() {
        return;
    }

    let num_comments = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
    pos += 4;

    for _ in 0..num_comments {
        if pos + 4 > data.len() {
            break;
        }
        let comment_len = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        pos += 4;

        if pos + comment_len > data.len() {
            break;
        }

        let comment = String::from_utf8_lossy(&data[pos..pos + comment_len]);
        pos += comment_len;

        if let Some(eq_pos) = comment.find('=') {
            let field = &comment[..eq_pos];
            let value = &comment[eq_pos + 1..];

            let (name, description) = vorbis_field_name(field);
            // If vorbis_field_name returned the raw field (unknown), try CamelCase conversion
            let (final_name, final_desc) = if name == field && field.contains(':') {
                // Handle NAMESPACE:KEY → NamespaceKey (from Perl Vorbis.pm)
                let parts: Vec<&str> = field.splitn(2, ':').collect();
                let ns = parts[0];
                let key = parts.get(1).unwrap_or(&"");
                let cc = format!("{}{}",
                    ns.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default().to_string()
                    + &ns[1..].to_lowercase(),
                    key.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default().to_string()
                    + &key[1..].to_lowercase());
                (cc.clone(), cc)
            } else if name == field {
                // Unknown field without namespace — just use CamelCase
                let cc = field.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default()
                    + &field[1..].to_lowercase();
                (cc.clone(), cc)
            } else {
                (name.to_string(), description.to_string())
            };
            tags.push(mk(&final_name, &final_desc, Value::String(value.to_string())));
        }
    }
}

fn parse_flac_picture(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 32 {
        return;
    }

    let pic_type = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    let mime_len = u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize;
    let mut pos = 8;

    if pos + mime_len > data.len() {
        return;
    }
    let mime = String::from_utf8_lossy(&data[pos..pos + mime_len]).to_string();
    pos += mime_len;

    if pos + 4 > data.len() {
        return;
    }
    let desc_len = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
    pos += 4 + desc_len;

    if pos + 16 > data.len() {
        return;
    }
    let width = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
    let height = u32::from_be_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]);
    pos += 16; // width + height + depth + num_colors

    if pos + 4 > data.len() {
        return;
    }
    let pic_data_len = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);

    let type_str = match pic_type {
        0 => "Other",
        3 => "Front Cover",
        4 => "Back Cover",
        _ => "Picture",
    };

    tags.push(mk(
        "Picture",
        "Picture",
        Value::String(format!("{} ({}x{}, {}, {} bytes)", type_str, width, height, mime, pic_data_len)),
    ));
}

/// Map Vorbis comment field names to canonical tag names.
fn vorbis_field_name(field: &str) -> (&str, &str) {
    match field.to_uppercase().as_str() {
        "TITLE" => ("Title", "Title"),
        "ARTIST" => ("Artist", "Artist"),
        "ALBUM" => ("Album", "Album"),
        "ALBUMARTIST" | "ALBUM ARTIST" => ("AlbumArtist", "Album Artist"),
        "TRACKNUMBER" | "TRACK" => ("TrackNumber", "Track Number"),
        "TRACKTOTAL" | "TOTALTRACKS" => ("TrackTotal", "Total Tracks"),
        "DISCNUMBER" | "DISC" => ("DiscNumber", "Disc Number"),
        "DISCTOTAL" | "TOTALDISCS" => ("DiscTotal", "Total Discs"),
        "DATE" => ("Date", "Date"),
        "GENRE" => ("Genre", "Genre"),
        "COMMENT" | "DESCRIPTION" => ("Comment", "Comment"),
        "COMPOSER" => ("Composer", "Composer"),
        "PERFORMER" => ("Performer", "Performer"),
        "LYRICIST" => ("Lyricist", "Lyricist"),
        "CONDUCTOR" => ("Conductor", "Conductor"),
        "PUBLISHER" | "LABEL" => ("Publisher", "Publisher"),
        "COPYRIGHT" => ("Copyright", "Copyright"),
        "LICENSE" => ("License", "License"),
        "ISRC" => ("ISRC", "ISRC"),
        "ENCODER" | "ENCODED-BY" => ("EncodedBy", "Encoded By"),
        "REPLAYGAIN_TRACK_GAIN" => ("ReplayGainTrackGain", "ReplayGain Track Gain"),
        "REPLAYGAIN_TRACK_PEAK" => ("ReplayGainTrackPeak", "ReplayGain Track Peak"),
        "REPLAYGAIN_ALBUM_GAIN" => ("ReplayGainAlbumGain", "ReplayGain Album Gain"),
        "REPLAYGAIN_ALBUM_PEAK" => ("ReplayGainAlbumPeak", "ReplayGain Album Peak"),
        "LANGUAGE" => ("Language", "Language"),
        "BPM" | "TEMPO" => ("BPM", "Beats Per Minute"),
        "COVERART" => ("CoverArt", "Cover Art"),
        "COVERARTMIME" => ("CoverArtMIMEType", "Cover Art MIME Type"),
        "METADATA_BLOCK_PICTURE" => ("Picture", "Picture"),
        _ => {
            // Handle namespace:key patterns (e.g. MEDIAJUKEBOX:DATE → MediajukeboxDate)
            if let Some(colon) = field.find(':') {
                let ns = &field[..colon];
                let key = &field[colon+1..];
                // Convert to CamelCase: MEDIAJUKEBOX → Mediajukebox, DATE → Date
                let ns_cc = ns.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default()
                    + &ns[1..].to_lowercase();
                let key_cc = key.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default()
                    + &key[1..].to_lowercase();
                // We can't return borrowed str for dynamic strings, so use field as-is
                // The caller handles this case separately
                return (field, field);
            }
            return (field, field);
        }
    }
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
            family0: "FLAC".into(),
            family1: "FLAC".into(),
            family2: "Audio".into(),
        },
        raw_value: value,
        print_value,
        priority: 0,
    }
}
