//! Font file reader (TrueType, OpenType, WOFF, WOFF2).
//!
//! Extracts font name table entries.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_font(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 12 {
        return Err(Error::InvalidData("file too small for font".into()));
    }

    let mut tags = Vec::new();

    // Detect font type
    if data.starts_with(b"wOFF") {
        return read_woff(data);
    }
    if data.starts_with(b"wOF2") {
        tags.push(mk("FontFormat", "Font Format", Value::String("WOFF2".into())));
        return Ok(tags);
    }
    if data.starts_with(b"OTTO") {
        tags.push(mk("FontFormat", "Font Format", Value::String("OpenType/CFF".into())));
    } else if data.starts_with(&[0x00, 0x01, 0x00, 0x00]) || data.starts_with(b"true") || data.starts_with(b"typ1") {
        tags.push(mk("FontFormat", "Font Format", Value::String("TrueType".into())));
    } else if data.starts_with(b"ttcf") {
        tags.push(mk("FontFormat", "Font Format", Value::String("TrueType Collection".into())));
        let num_fonts = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
        tags.push(mk("FontCount", "Font Count", Value::U32(num_fonts)));
        return Ok(tags);
    } else {
        return Err(Error::InvalidData("unknown font format".into()));
    }

    // Parse sfnt table directory
    let num_tables = u16::from_be_bytes([data[4], data[5]]) as usize;
    let mut pos = 12;

    for _ in 0..num_tables {
        if pos + 16 > data.len() {
            break;
        }
        let tag = &data[pos..pos + 4];
        let offset = u32::from_be_bytes([data[pos + 8], data[pos + 9], data[pos + 10], data[pos + 11]]) as usize;
        let length = u32::from_be_bytes([data[pos + 12], data[pos + 13], data[pos + 14], data[pos + 15]]) as usize;
        pos += 16;

        if tag == b"name" && offset + length <= data.len() {
            parse_name_table(&data[offset..offset + length], &mut tags);
        }
        if tag == b"head" && offset + 54 <= data.len() {
            let d = &data[offset..];
            let units_per_em = u16::from_be_bytes([d[18], d[19]]);
            tags.push(mk("UnitsPerEm", "Units Per Em", Value::U16(units_per_em)));
            // Created date (Mac epoch at offset 20, 8 bytes)
            let created = i64::from_be_bytes([d[20], d[21], d[22], d[23], d[24], d[25], d[26], d[27]]);
            if created > 0 {
                // Mac epoch = seconds since 1904-01-01
                tags.push(mk("FontCreated", "Font Created", Value::String(format!("(Mac epoch {})", created))));
            }
        }
    }

    Ok(tags)
}

fn read_woff(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 44 {
        return Err(Error::InvalidData("file too small for WOFF".into()));
    }

    let mut tags = Vec::new();
    tags.push(mk("FontFormat", "Font Format", Value::String("WOFF".into())));

    let flavor = &data[4..8];
    if flavor == b"OTTO" {
        tags.push(mk("FontFlavor", "Font Flavor", Value::String("OpenType/CFF".into())));
    } else {
        tags.push(mk("FontFlavor", "Font Flavor", Value::String("TrueType".into())));
    }

    let _num_tables = u16::from_be_bytes([data[12], data[13]]) as usize;
    let _total_size = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
    let major = u16::from_be_bytes([data[20], data[21]]);
    let minor = u16::from_be_bytes([data[22], data[23]]);

    tags.push(mk("Version", "Version", Value::String(format!("{}.{}", major, minor))));

    Ok(tags)
}

fn parse_name_table(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 6 {
        return;
    }

    let count = u16::from_be_bytes([data[2], data[3]]) as usize;
    let string_offset = u16::from_be_bytes([data[4], data[5]]) as usize;
    let mut pos = 6;

    for _ in 0..count {
        if pos + 12 > data.len() {
            break;
        }

        let platform_id = u16::from_be_bytes([data[pos], data[pos + 1]]);
        let _encoding_id = u16::from_be_bytes([data[pos + 2], data[pos + 3]]);
        let _language_id = u16::from_be_bytes([data[pos + 4], data[pos + 5]]);
        let name_id = u16::from_be_bytes([data[pos + 6], data[pos + 7]]);
        let length = u16::from_be_bytes([data[pos + 8], data[pos + 9]]) as usize;
        let offset = u16::from_be_bytes([data[pos + 10], data[pos + 11]]) as usize;
        pos += 12;

        let abs_offset = string_offset + offset;
        if abs_offset + length > data.len() {
            continue;
        }

        // Only process platform 3 (Windows) or platform 1 (Mac)
        let text = if platform_id == 3 {
            let units: Vec<u16> = data[abs_offset..abs_offset + length]
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16_lossy(&units)
        } else if platform_id == 1 {
            String::from_utf8_lossy(&data[abs_offset..abs_offset + length]).to_string()
        } else {
            continue;
        };

        if text.is_empty() {
            continue;
        }

        let (name, desc) = match name_id {
            0 => ("Copyright", "Copyright"),
            1 => ("FontFamily", "Font Family"),
            2 => ("FontSubFamily", "Font Sub-Family"),
            3 => ("FontUniqueID", "Unique ID"),
            4 => ("FontName", "Font Name"),
            5 => ("FontVersion", "Version"),
            6 => ("PostScriptName", "PostScript Name"),
            7 => ("Trademark", "Trademark"),
            8 => ("Manufacturer", "Manufacturer"),
            9 => ("Designer", "Designer"),
            11 => ("URLVendor", "Vendor URL"),
            12 => ("URLDesigner", "Designer URL"),
            13 => ("LicenseDescription", "License"),
            14 => ("LicenseURL", "License URL"),
            16 => ("TypographicFamily", "Typographic Family"),
            _ => continue,
        };

        // Only add if not already present (prefer Windows names)
        if !tags.iter().any(|t| t.name == name) {
            tags.push(mk(name, desc, Value::String(text)));
        }
    }
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup { family0: "Font".into(), family1: "Font".into(), family2: "Other".into() },
        raw_value: value, print_value: pv, priority: 0,
    }
}
