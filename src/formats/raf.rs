//! Fujifilm RAF file format reader.
//!
//! Parses RAF header and embedded JPEG/EXIF data.
//! Mirrors ExifTool's FujiFilm.pm ProcessRAF.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_raf(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 100 || !data.starts_with(b"FUJIFILMCCD-RAW") {
        return Err(Error::InvalidData("not a Fujifilm RAF file".into()));
    }

    let mut tags = Vec::new();

    // Version at offset 0x3C (4 bytes ASCII, e.g., "0201")
    let version = String::from_utf8_lossy(&data[0x3C..0x40]).to_string();
    tags.push(mk("RAFVersion", "RAF Version", Value::String(version)));

    // Camera model (bytes 0x1C-0x3C, null-terminated)
    let model_end = data[0x1C..0x3C].iter().position(|&b| b == 0).unwrap_or(0x20);
    let model = String::from_utf8_lossy(&data[0x1C..0x1C + model_end]).to_string();
    if !model.is_empty() {
        tags.push(mk("Model", "Camera Model", Value::String(model)));
    }

    // JPEG offset at 0x54 (uint32 BE) and length at 0x58
    if data.len() >= 0x5C {
        let jpeg_offset = u32::from_be_bytes([data[0x54], data[0x55], data[0x56], data[0x57]]) as usize;
        let jpeg_length = u32::from_be_bytes([data[0x58], data[0x59], data[0x5A], data[0x5B]]) as usize;

        if jpeg_offset > 0 && jpeg_offset + jpeg_length <= data.len() {
            let jpeg_data = &data[jpeg_offset..jpeg_offset + jpeg_length];
            // Try to extract EXIF from embedded JPEG
            if jpeg_data.starts_with(&[0xFF, 0xD8, 0xFF]) {
                if let Ok(jpeg_tags) = crate::formats::jpeg::read_jpeg(jpeg_data) {
                    tags.extend(jpeg_tags);
                }
            }
        }
    }

    // RAF directory offset at 0x5C and length at 0x60
    if data.len() >= 0x64 {
        let dir_offset = u32::from_be_bytes([data[0x5C], data[0x5D], data[0x5E], data[0x5F]]) as usize;
        let dir_length = u32::from_be_bytes([data[0x60], data[0x61], data[0x62], data[0x63]]) as usize;

        if dir_offset > 0 && dir_offset + dir_length <= data.len() {
            parse_raf_directory(&data[dir_offset..dir_offset + dir_length], &mut tags);
        }
    }

    Ok(tags)
}

fn parse_raf_directory(data: &[u8], _tags: &mut Vec<Tag>) {
    if data.len() < 4 {
        return;
    }

    let num_entries = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let mut pos = 4;

    for _ in 0..num_entries {
        if pos + 12 > data.len() {
            break;
        }

        let _tag_id = u16::from_be_bytes([data[pos], data[pos + 1]]);
        let entry_size = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
        // offset and length within data block
        let value_offset = u32::from_be_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]) as usize;
        let value_length = u32::from_be_bytes([data[pos + 8], data[pos + 9], data[pos + 10], data[pos + 11]]) as usize;

        pos += 4 + entry_size.max(8);

        if value_offset + value_length > data.len() || value_length == 0 {
            continue;
        }

        // We could decode specific RAF tags here but most useful metadata
        // comes from the embedded JPEG/EXIF
    }
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "RAF".into(),
            family1: "RAF".into(),
            family2: "Camera".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}
