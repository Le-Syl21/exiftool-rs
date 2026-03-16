//! ASF/WMV/WMA file format reader.
//!
//! Parses GUID-based ASF objects for metadata.
//! Mirrors ExifTool's ASF.pm.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

// ASF Header Object GUID
const ASF_HEADER_GUID: [u8; 16] = [
    0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11,
    0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62, 0xCE, 0x6C,
];

pub fn read_asf(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 30 || &data[..16] != ASF_HEADER_GUID {
        return Err(Error::InvalidData("not an ASF file".into()));
    }

    let mut tags = Vec::new();
    let header_size = u64::from_le_bytes([
        data[16], data[17], data[18], data[19],
        data[20], data[21], data[22], data[23],
    ]) as usize;
    let _num_headers = u32::from_le_bytes([data[24], data[25], data[26], data[27]]);

    let mut pos = 30; // After ASF header object header
    let end = header_size.min(data.len());

    while pos + 24 <= end {
        let guid = &data[pos..pos + 16];
        let obj_size = u64::from_le_bytes([
            data[pos + 16], data[pos + 17], data[pos + 18], data[pos + 19],
            data[pos + 20], data[pos + 21], data[pos + 22], data[pos + 23],
        ]) as usize;

        if obj_size < 24 || pos + obj_size > end {
            break;
        }

        let obj_data = &data[pos + 24..pos + obj_size];

        // File Properties Object
        if guid_matches(guid, &[0xA1, 0xDC, 0xAB, 0x8C, 0x47, 0xA9, 0xCF, 0x11, 0x8E, 0xE4, 0x00, 0xC0, 0x0C, 0x20, 0x53, 0x65]) {
            parse_file_properties(obj_data, &mut tags);
        }
        // Content Description Object
        else if guid_matches(guid, &[0x33, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62, 0xCE, 0x6C]) {
            parse_content_description(obj_data, &mut tags);
        }
        // Extended Content Description Object
        else if guid_matches(guid, &[0x40, 0xA4, 0xD0, 0xD2, 0x07, 0xE3, 0xD2, 0x11, 0x97, 0xF0, 0x00, 0xA0, 0xC9, 0x5E, 0xA8, 0x50]) {
            parse_extended_content(obj_data, &mut tags);
        }
        // Stream Properties Object
        else if guid_matches(guid, &[0x91, 0x07, 0xDC, 0xB7, 0xB7, 0xA9, 0xCF, 0x11, 0x8E, 0xE6, 0x00, 0xC0, 0x0C, 0x20, 0x53, 0x65]) {
            parse_stream_properties(obj_data, &mut tags);
        }

        pos += obj_size;
    }

    Ok(tags)
}

fn parse_file_properties(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 80 {
        return;
    }

    // File size at offset 16 (uint64)
    let _file_size = u64::from_le_bytes([
        data[16], data[17], data[18], data[19],
        data[20], data[21], data[22], data[23],
    ]);

    // Creation date at offset 24 (FILETIME - 100ns since 1601-01-01)
    let create_time = u64::from_le_bytes([
        data[24], data[25], data[26], data[27],
        data[28], data[29], data[30], data[31],
    ]);

    // Duration at offset 40 (100ns units)
    let duration_100ns = u64::from_le_bytes([
        data[40], data[41], data[42], data[43],
        data[44], data[45], data[46], data[47],
    ]);

    // Preroll at offset 56 (milliseconds)
    let preroll_ms = u64::from_le_bytes([
        data[56], data[57], data[58], data[59],
        data[60], data[61], data[62], data[63],
    ]);

    if create_time > 0 {
        if let Some(dt) = filetime_to_string(create_time) {
            tags.push(mk("CreateDate", "Create Date", Value::String(dt)));
        }
    }

    let duration_secs = (duration_100ns as f64 / 10_000_000.0) - (preroll_ms as f64 / 1000.0);
    if duration_secs > 0.0 {
        let mins = (duration_secs / 60.0) as u32;
        let secs = duration_secs % 60.0;
        tags.push(mk("Duration", "Duration", Value::String(format!("{}:{:05.2}", mins, secs))));
    }

    // Max bitrate at offset 72
    let max_bitrate = u32::from_le_bytes([data[72], data[73], data[74], data[75]]);
    if max_bitrate > 0 {
        tags.push(mk("MaxBitrate", "Max Bitrate", Value::String(format!("{} kbps", max_bitrate / 1000))));
    }
}

fn parse_content_description(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 10 {
        return;
    }

    let title_len = u16::from_le_bytes([data[0], data[1]]) as usize;
    let author_len = u16::from_le_bytes([data[2], data[3]]) as usize;
    let copyright_len = u16::from_le_bytes([data[4], data[5]]) as usize;
    let desc_len = u16::from_le_bytes([data[6], data[7]]) as usize;
    let _rating_len = u16::from_le_bytes([data[8], data[9]]) as usize;

    let mut pos = 10;
    let fields = [
        (title_len, "Title", "Title"),
        (author_len, "Author", "Author"),
        (copyright_len, "Copyright", "Copyright"),
        (desc_len, "Description", "Description"),
    ];

    for (len, name, desc) in &fields {
        if pos + len > data.len() {
            break;
        }
        let text = decode_utf16le(&data[pos..pos + len]);
        if !text.is_empty() {
            tags.push(mk(name, desc, Value::String(text)));
        }
        pos += len;
    }
}

fn parse_extended_content(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 2 {
        return;
    }
    let count = u16::from_le_bytes([data[0], data[1]]) as usize;
    let mut pos = 2;

    for _ in 0..count {
        if pos + 6 > data.len() {
            break;
        }
        let name_len = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2;
        if pos + name_len > data.len() {
            break;
        }
        let name = decode_utf16le(&data[pos..pos + name_len]);
        pos += name_len;

        if pos + 4 > data.len() {
            break;
        }
        let val_type = u16::from_le_bytes([data[pos], data[pos + 1]]);
        let val_len = u16::from_le_bytes([data[pos + 2], data[pos + 3]]) as usize;
        pos += 4;

        if pos + val_len > data.len() {
            break;
        }

        let value = match val_type {
            0 => decode_utf16le(&data[pos..pos + val_len]),       // Unicode string
            1 => format!("(Binary {} bytes)", val_len),            // Binary
            2 => {                                                  // Bool
                if val_len >= 4 {
                    let v = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
                    if v != 0 { "True".into() } else { "False".into() }
                } else { String::new() }
            }
            3 => {                                                  // DWORD
                if val_len >= 4 {
                    u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]).to_string()
                } else { String::new() }
            }
            4 | 5 => {                                              // QWORD / WORD
                if val_len >= 8 {
                    u64::from_le_bytes([
                        data[pos], data[pos + 1], data[pos + 2], data[pos + 3],
                        data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7],
                    ]).to_string()
                } else { String::new() }
            }
            _ => String::new(),
        };
        pos += val_len;

        if !value.is_empty() && !name.is_empty() {
            let clean_name = name.trim_start_matches("WM/");
            tags.push(mk(clean_name, clean_name, Value::String(value)));
        }
    }
}

fn parse_stream_properties(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 54 {
        return;
    }

    let stream_guid = &data[0..16];

    // Audio stream GUID
    if guid_matches(stream_guid, &[0x40, 0x9E, 0x69, 0xF8, 0x4D, 0x5B, 0xCF, 0x11, 0xA8, 0xFD, 0x00, 0x80, 0x5F, 0x5C, 0x44, 0x2B]) {
        if data.len() >= 54 + 18 {
            let type_specific = &data[54..];
            let _format_tag = u16::from_le_bytes([type_specific[0], type_specific[1]]);
            let channels = u16::from_le_bytes([type_specific[2], type_specific[3]]);
            let sample_rate = u32::from_le_bytes([type_specific[4], type_specific[5], type_specific[6], type_specific[7]]);
            let avg_bitrate = u32::from_le_bytes([type_specific[8], type_specific[9], type_specific[10], type_specific[11]]);
            let bits_per_sample = u16::from_le_bytes([type_specific[14], type_specific[15]]);

            tags.push(mk("AudioChannels", "Audio Channels", Value::U16(channels)));
            tags.push(mk("AudioSampleRate", "Audio Sample Rate", Value::U32(sample_rate)));
            tags.push(mk("AudioBitrate", "Audio Bitrate", Value::String(format!("{} kbps", avg_bitrate * 8 / 1000))));
            if bits_per_sample > 0 {
                tags.push(mk("AudioBitsPerSample", "Audio Bits/Sample", Value::U16(bits_per_sample)));
            }
        }
    }
    // Video stream GUID
    else if guid_matches(stream_guid, &[0xC0, 0xEF, 0x19, 0xBC, 0x4D, 0x5B, 0xCF, 0x11, 0xA8, 0xFD, 0x00, 0x80, 0x5F, 0x5C, 0x44, 0x2B]) {
        if data.len() >= 54 + 40 {
            let type_specific = &data[54..];
            let width = u32::from_le_bytes([type_specific[4], type_specific[5], type_specific[6], type_specific[7]]);
            let height = u32::from_le_bytes([type_specific[8], type_specific[9], type_specific[10], type_specific[11]]);

            tags.push(mk("ImageWidth", "Image Width", Value::U32(width)));
            tags.push(mk("ImageHeight", "Image Height", Value::U32(height)));
        }
    }
}

fn guid_matches(a: &[u8], b: &[u8; 16]) -> bool {
    a.len() >= 16 && &a[..16] == b
}

fn decode_utf16le(data: &[u8]) -> String {
    let units: Vec<u16> = data
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units)
        .trim_end_matches('\0')
        .to_string()
}

fn filetime_to_string(ft: u64) -> Option<String> {
    if ft == 0 {
        return None;
    }
    // FILETIME: 100ns intervals since 1601-01-01
    // Unix epoch starts 11644473600 seconds later
    let unix_secs = (ft / 10_000_000) as i64 - 11644473600;
    if unix_secs < 0 {
        return None;
    }

    let days = unix_secs / 86400;
    let time = unix_secs % 86400;
    let h = time / 3600;
    let m = (time % 3600) / 60;
    let s = time % 60;

    let mut y = 1970i32;
    let mut rem = days;
    loop {
        let dy = if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 { 366 } else { 365 };
        if rem < dy { break; }
        rem -= dy;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let months = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut mo = 1;
    for &dm in &months {
        if rem < dm { break; }
        rem -= dm;
        mo += 1;
    }

    Some(format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}", y, mo, rem + 1, h, m, s))
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "ASF".into(),
            family1: "ASF".into(),
            family2: "Video".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}
