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
    // Bits 0-15: min block size
    // Bits 16-31: max block size
    // Bits 32-55: min frame size (24 bits)
    // Bits 56-79: max frame size (24 bits)
    // Bits 80-99: sample rate (20 bits)
    // Bits 100-102: channels - 1 (3 bits)
    // Bits 103-107: bits per sample - 1 (5 bits)
    // Bits 108-143: total samples (36 bits)

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

    tags.push(mk("SampleRate", "Sample Rate", Value::U32(sample_rate)));
    tags.push(mk("NumChannels", "Number of Channels", Value::U8(channels)));
    tags.push(mk("BitsPerSample", "Bits Per Sample", Value::U16(bits_per_sample)));

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
            tags.push(mk(name, description, Value::String(value.to_string())));
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
        "TRACKNUMBER" | "TRACK" => ("Track", "Track Number"),
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
        _ => {
            // Return as-is for unknown fields
            // We leak here but it's acceptable for static-like behavior
            // In practice we return the field name directly
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
