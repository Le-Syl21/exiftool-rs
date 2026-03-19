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
    let is_ico = data[2] == 1;
    let num_images = u16::from_le_bytes([data[4], data[5]]);

    tags.push(mk("ImageCount", "Image Count", Value::U16(num_images)));

    // Parse directory entries (16 bytes each, starting at offset 6)
    // Only extract tags from first image entry (like Perl does)
    let pos = 6;
    if num_images > 0 && pos + 16 <= data.len() {
        let w = if data[pos] == 0 { 256u16 } else { data[pos] as u16 };
        let h = if data[pos + 1] == 0 { 256u16 } else { data[pos + 1] as u16 };
        let num_colors = data[pos + 2] as u16;
        // byte 3 is reserved
        let color_planes = u16::from_le_bytes([data[pos + 4], data[pos + 5]]);
        let bits_per_pixel = u16::from_le_bytes([data[pos + 6], data[pos + 7]]);
        let img_size = u32::from_le_bytes([data[pos + 8], data[pos + 9], data[pos + 10], data[pos + 11]]);

        tags.push(mk("ImageWidth", "Image Width", Value::U16(w)));
        tags.push(mk("ImageHeight", "Image Height", Value::U16(h)));
        tags.push(mk("NumColors", "Num Colors", Value::U16(num_colors)));
        if is_ico {
            tags.push(mk("ColorPlanes", "Color Planes", Value::U16(color_planes)));
            tags.push(mk("BitsPerPixel", "Bits Per Pixel", Value::U16(bits_per_pixel)));
        }
        tags.push(mk("ImageLength", "Image Length", Value::U32(img_size)));
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
