//! ICO/CUR file format reader.
//!
//! Parses Windows icon/cursor files.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_ico(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 6 || data[0] != 0 || data[1] != 0 || (data[2] != 1 && data[2] != 2) {
        return Err(Error::InvalidData("not an ICO/CUR file".into()));
    }

    let mut tags = Vec::new();
    let ico_type = if data[2] == 1 { "Icon" } else { "Cursor" };
    let num_images = u16::from_le_bytes([data[4], data[5]]);

    tags.push(mk("IconType", "Icon Type", Value::String(ico_type.into())));
    tags.push(mk("ImageCount", "Image Count", Value::U16(num_images)));

    // Parse directory entries (16 bytes each, starting at offset 6)
    let mut pos = 6;
    for i in 0..num_images.min(16) {
        if pos + 16 > data.len() {
            break;
        }
        let w = if data[pos] == 0 { 256u16 } else { data[pos] as u16 };
        let h = if data[pos + 1] == 0 { 256u16 } else { data[pos + 1] as u16 };
        let _colors = data[pos + 2] as u16;
        let bit_count = u16::from_le_bytes([data[pos + 6], data[pos + 7]]);
        let _img_size = u32::from_le_bytes([data[pos + 8], data[pos + 9], data[pos + 10], data[pos + 11]]);

        if i == 0 {
            tags.push(mk("ImageWidth", "Image Width", Value::U16(w)));
            tags.push(mk("ImageHeight", "Image Height", Value::U16(h)));
            if bit_count > 0 {
                tags.push(mk("BitDepth", "Bit Depth", Value::U16(bit_count)));
            }
        }

        pos += 16;
    }

    Ok(tags)
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "ICO".into(),
            family1: "ICO".into(),
            family2: "Image".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}
