//! Canon CRW (CIFF) file format reader.
//!
//! Parses CIFF (Camera Image File Format) blocks used in Canon's legacy CRW files.
//! Mirrors ExifTool's CanonRaw.pm.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_crw(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 14 {
        return Err(Error::InvalidData("file too small for CRW".into()));
    }

    // Byte order (first 2 bytes)
    let is_le = data[0] == b'I' && data[1] == b'I';
    if !is_le && !(data[0] == b'M' && data[1] == b'M') {
        return Err(Error::InvalidData("invalid CRW byte order".into()));
    }

    // Header length
    let hlen = if is_le {
        u32::from_le_bytes([data[2], data[3], data[4], data[5]]) as usize
    } else {
        u32::from_be_bytes([data[2], data[3], data[4], data[5]]) as usize
    };

    // Validate HEAP signature
    if hlen < 14 || data.len() < hlen || &data[6..10] != b"HEAP" {
        return Err(Error::InvalidData("invalid CRW HEAP signature".into()));
    }

    let mut tags = Vec::new();

    // The root directory starts after the header and spans the rest of the file
    parse_ciff_dir(data, hlen, data.len(), is_le, &mut tags, 0);

    Ok(tags)
}

fn parse_ciff_dir(
    data: &[u8],
    block_start: usize,
    block_end: usize,
    is_le: bool,
    tags: &mut Vec<Tag>,
    depth: u32,
) {
    if depth > 10 || block_end <= block_start || block_end > data.len() {
        return;
    }

    // Last 4 bytes of block contain directory offset (relative to block_start)
    if block_end < block_start + 4 {
        return;
    }
    let dir_offset = read_u32(data, block_end - 4, is_le) as usize + block_start;

    if dir_offset + 2 > block_end {
        return;
    }

    let num_entries = read_u16(data, dir_offset, is_le) as usize;
    let mut pos = dir_offset + 2;

    for _ in 0..num_entries {
        if pos + 10 > block_end {
            break;
        }

        let tag_id = read_u16(data, pos, is_le);
        let size = read_u32(data, pos + 2, is_le) as usize;
        let value_offset = read_u32(data, pos + 6, is_le) as usize;
        pos += 10;

        let data_type = (tag_id >> 8) & 0x38;
        let abs_offset = value_offset + block_start;

        // Check if this is a subdirectory
        if data_type == 0x28 || data_type == 0x30 {
            if abs_offset + size <= block_end {
                parse_ciff_dir(data, abs_offset, abs_offset + size, is_le, tags, depth + 1);
            }
            continue;
        }

        // Extract value
        if abs_offset + size > data.len() {
            continue;
        }

        let value_data = &data[abs_offset..abs_offset + size];

        let (name, description) = crw_tag_name(tag_id);
        if name.is_empty() {
            continue;
        }

        let value = match data_type {
            0x00 => {
                // Raw bytes / string
                let s = String::from_utf8_lossy(value_data)
                    .trim_end_matches('\0')
                    .to_string();
                if s.chars().all(|c| c.is_ascii_graphic() || c.is_ascii_whitespace()) && !s.is_empty() {
                    Value::String(s)
                } else {
                    Value::Binary(value_data.to_vec())
                }
            }
            0x08 => {
                // ASCII string
                Value::String(
                    String::from_utf8_lossy(value_data)
                        .trim_end_matches('\0')
                        .to_string(),
                )
            }
            0x10 | 0x18 => {
                // int16 / int32
                if size == 2 {
                    Value::U16(read_u16(value_data, 0, is_le))
                } else if size == 4 {
                    Value::U32(read_u32(value_data, 0, is_le))
                } else {
                    Value::Binary(value_data.to_vec())
                }
            }
            _ => Value::Binary(value_data.to_vec()),
        };

        let print_value = value.to_display_string();
        tags.push(Tag {
            id: TagId::Numeric(tag_id),
            name: name.to_string(),
            description: description.to_string(),
            group: TagGroup {
                family0: "CanonRaw".into(),
                family1: "CanonRaw".into(),
                family2: "Camera".into(),
            },
            raw_value: value,
            print_value,
            priority: 0,
        });
    }
}

fn crw_tag_name(tag_id: u16) -> (&'static str, &'static str) {
    match tag_id & 0x3FFF {
        0x0005 => ("CanonRawMakeModel", "Make/Model"),
        0x0006 => ("TimeStamp", "Time Stamp"),
        0x000A => ("TargetImageType", "Target Image Type"),
        0x000B => ("ShutterReleaseMethod", "Shutter Release Method"),
        0x0010 => ("RawImageSize", "Raw Image Size"),
        0x0017 => ("CanonFileDescription", "File Description"),
        0x0018 => ("CanonFirmwareVersion", "Firmware Version"),
        0x001A => ("CanonFileNumber", "File Number"),
        0x001C => ("OwnerName", "Owner Name"),
        0x001E => ("CanonModelID", "Model ID"),
        0x0028 => ("CanonImageFormat", "Image Format"),
        0x002A => ("ShotInfo", "Shot Info"),
        0x002D => ("CameraSettings", "Camera Settings"),
        0x0035 => ("FocalLength", "Focal Length"),
        0x0038 => ("ShotInfo2", "Shot Info 2"),
        0x0093 => ("CanonRawSerialNumber", "Serial Number"),
        _ => ("", ""),
    }
}

fn read_u16(data: &[u8], offset: usize, is_le: bool) -> u16 {
    if is_le {
        u16::from_le_bytes([data[offset], data[offset + 1]])
    } else {
        u16::from_be_bytes([data[offset], data[offset + 1]])
    }
}

fn read_u32(data: &[u8], offset: usize, is_le: bool) -> u32 {
    if is_le {
        u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]])
    } else {
        u32::from_be_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]])
    }
}
