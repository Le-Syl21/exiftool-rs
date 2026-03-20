//! PGF (Progressive Graphics File) reader.
//!
//! Parses PGF header and extracts embedded PNG metadata.
//! Mirrors ExifTool's PGF.pm.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_pgf(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 24 || !data.starts_with(b"PGF") {
        return Err(Error::InvalidData("not a PGF file".into()));
    }

    let mut tags = Vec::new();

    // Version byte at offset 3
    let version = data[3];
    tags.push(mk("PGFVersion", "PGF Version", Value::String(format!("0x{:02x}", version))));

    if version != 0x36 {
        // Unsupported version, return what we have
        return Ok(tags);
    }

    // Post-header data length at offset 4 (little-endian uint32)
    let post_len = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;

    // PGF header fields (little-endian)
    // offset 8: ImageWidth (int32u)
    if data.len() >= 12 {
        let width = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        tags.push(mk("ImageWidth", "Image Width", Value::U32(width)));
    }
    // offset 12: ImageHeight (int32u)
    if data.len() >= 16 {
        let height = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
        tags.push(mk("ImageHeight", "Image Height", Value::U32(height)));
    }
    // offset 16: PyramidLevels (int8u)
    if data.len() >= 17 {
        tags.push(mk("PyramidLevels", "Pyramid Levels", Value::U8(data[16])));
    }
    // offset 17: Quality (int8u)
    if data.len() >= 18 {
        tags.push(mk("Quality", "Quality", Value::U8(data[17])));
    }
    // offset 18: BitsPerPixel (int8u)
    if data.len() >= 19 {
        tags.push(mk("BitsPerPixel", "Bits Per Pixel", Value::U8(data[18])));
    }
    // offset 19: ColorComponents (int8u)
    if data.len() >= 20 {
        tags.push(mk("ColorComponents", "Color Components", Value::U8(data[19])));
    }
    // offset 20: ColorMode (int8u)
    if data.len() >= 21 {
        let color_mode = data[20];
        let mode_str = match color_mode {
            0 => "Bitmap",
            1 => "Grayscale",
            2 => "Indexed",
            3 => "RGB",
            4 => "CMYK",
            7 => "Multichannel",
            8 => "Duotone",
            9 => "Lab",
            _ => "Unknown",
        };
        tags.push(mk("ColorMode", "Color Mode", Value::String(mode_str.into())));
        // offset 21-23: BackgroundColor (int8u[3])
        if data.len() >= 24 {
            let bg = format!("{} {} {}", data[21], data[22], data[23]);
            tags.push(mk("BackgroundColor", "Background Color", Value::String(bg)));
        }
    }

    // The embedded metadata is a PNG image that starts after the PGF header
    // Header is 24 bytes, then post_len bytes of post-header data
    // But if color mode is Indexed (2), skip 1024 byte color table
    let color_mode = if data.len() >= 21 { data[20] } else { 255 };
    let skip = if color_mode == 2 { 1024usize } else { 0usize };
    let meta_start = 24 + skip;

    // The post_len includes the metadata PNG
    let effective_len = post_len.saturating_sub(16 + skip); // 16 = size of fixed header fields
    let _meta_end = meta_start + effective_len.min(data.len().saturating_sub(meta_start));

    if meta_start < data.len() && meta_start + 8 < data.len() {
        let meta = &data[meta_start..data.len()];
        // Look for embedded PNG
        if let Some(png_pos) = find_bytes(meta, &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
            let png_data = &meta[png_pos..];
            if let Ok(png_tags) = crate::formats::png::read_png(png_data) {
                // Add PNG tags, but skip dimension tags (PGF header has the correct ones)
                for mut tag in png_tags {
                    match tag.name.as_str() {
                        "ImageWidth" | "ImageHeight" | "ImageSize" | "Megapixels"
                        | "FileType" | "FileTypeExtension" | "MIMEType" => continue,
                        _ => {}
                    }
                    tag.priority = 2; // higher priority than default
                    tags.push(tag);
                }
            }
        }
    }

    Ok(tags)
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "PGF".into(),
            family1: "PGF".into(),
            family2: "Image".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 2,
    }
}
