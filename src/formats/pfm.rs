//! PFM (Printer Font Metrics / Portable Float Map) format reader.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_pfm(data: &[u8]) -> Result<Vec<Tag>> {
    // Detect Printer Font Metrics (PFM) vs Portable Float Map
    // PFM starts with 0x00 followed by 0x01 or 0x02
    if data.len() >= 2 && data[0] == 0x00 && (data[1] == 0x01 || data[1] == 0x02) {
        return read_printer_font_metrics(data);
    }
    super::ppm::read_ppm(data)
}

/// Read Printer Font Metrics (.pfm) binary format.
/// Little-endian fields at fixed offsets, as defined in Adobe's PFM spec.
fn read_printer_font_metrics(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 117 {
        return Err(Error::InvalidData("PFM file too short".into()));
    }
    let stored_size = u32::from_le_bytes([data[2], data[3], data[4], data[5]]) as usize;
    if stored_size != data.len() {
        return Err(Error::InvalidData("PFM file size mismatch".into()));
    }

    let mut tags: Vec<Tag> = Vec::new();

    // PFMVersion at offset 0: int16u LE, PrintConv: sprintf("%x.%.2x",$val>>8,$val&0xff)
    let pfm_ver = u16::from_le_bytes([data[0], data[1]]);
    let ver_str = format!("{:x}.{:02x}", pfm_ver >> 8, pfm_ver & 0xff);
    tags.push(mktag_font(
        "PFMVersion",
        "PFM Version",
        Value::String(ver_str),
    ));

    // Copyright at offset 6: string[60]
    let copyright = pfm_str(data, 6, 60);
    if !copyright.is_empty() {
        tags.push(mktag_font(
            "Copyright",
            "Copyright",
            Value::String(copyright),
        ));
    }

    // FontType at offset 66: int16u LE
    let font_type = u16::from_le_bytes([data[66], data[67]]);
    tags.push(mktag_font(
        "FontType",
        "Font Type",
        Value::String(format!("{}", font_type)),
    ));

    // PointSize at offset 68: int16u LE
    let point_size = u16::from_le_bytes([data[68], data[69]]);
    tags.push(mktag_font(
        "PointSize",
        "Point Size",
        Value::String(format!("{}", point_size)),
    ));

    // YResolution at offset 70: int16u LE
    let y_res = u16::from_le_bytes([data[70], data[71]]);
    tags.push(mktag_font(
        "YResolution",
        "Y Resolution",
        Value::String(format!("{}", y_res)),
    ));

    // XResolution at offset 72: int16u LE
    let x_res = u16::from_le_bytes([data[72], data[73]]);
    tags.push(mktag_font(
        "XResolution",
        "X Resolution",
        Value::String(format!("{}", x_res)),
    ));

    // Ascent at offset 74: int16u LE
    let ascent = u16::from_le_bytes([data[74], data[75]]);
    tags.push(mktag_font(
        "Ascent",
        "Ascent",
        Value::String(format!("{}", ascent)),
    ));

    // InternalLeading at offset 76: int16u LE
    let int_lead = u16::from_le_bytes([data[76], data[77]]);
    tags.push(mktag_font(
        "InternalLeading",
        "Internal Leading",
        Value::String(format!("{}", int_lead)),
    ));

    // ExternalLeading at offset 78: int16u LE
    let ext_lead = u16::from_le_bytes([data[78], data[79]]);
    tags.push(mktag_font(
        "ExternalLeading",
        "External Leading",
        Value::String(format!("{}", ext_lead)),
    ));

    // Italic at offset 80: int8u
    tags.push(mktag_font(
        "Italic",
        "Italic",
        Value::String(format!("{}", data[80])),
    ));

    // Underline at offset 81: int8u
    tags.push(mktag_font(
        "Underline",
        "Underline",
        Value::String(format!("{}", data[81])),
    ));

    // Strikeout at offset 82: int8u
    tags.push(mktag_font(
        "Strikeout",
        "Strikeout",
        Value::String(format!("{}", data[82])),
    ));

    // Weight at offset 83: int16u LE
    let weight = u16::from_le_bytes([data[83], data[84]]);
    tags.push(mktag_font(
        "Weight",
        "Weight",
        Value::String(format!("{}", weight)),
    ));

    // CharacterSet at offset 85: int8u
    tags.push(mktag_font(
        "CharacterSet",
        "Character Set",
        Value::String(format!("{}", data[85])),
    ));

    // PixWidth at offset 86: int16u LE
    let pix_w = u16::from_le_bytes([data[86], data[87]]);
    tags.push(mktag_font(
        "PixWidth",
        "Pix Width",
        Value::String(format!("{}", pix_w)),
    ));

    // PixHeight at offset 88: int16u LE
    let pix_h = u16::from_le_bytes([data[88], data[89]]);
    tags.push(mktag_font(
        "PixHeight",
        "Pix Height",
        Value::String(format!("{}", pix_h)),
    ));

    // PitchAndFamily at offset 90: int8u
    tags.push(mktag_font(
        "PitchAndFamily",
        "Pitch And Family",
        Value::String(format!("{}", data[90])),
    ));

    // AvgWidth at offset 91: int16u LE
    let avg_w = u16::from_le_bytes([data[91], data[92]]);
    tags.push(mktag_font(
        "AvgWidth",
        "Avg Width",
        Value::String(format!("{}", avg_w)),
    ));

    // MaxWidth at offset 93: int16u LE
    let max_w = u16::from_le_bytes([data[93], data[94]]);
    tags.push(mktag_font(
        "MaxWidth",
        "Max Width",
        Value::String(format!("{}", max_w)),
    ));

    // FirstChar at offset 95: int8u
    tags.push(mktag_font(
        "FirstChar",
        "First Char",
        Value::String(format!("{}", data[95])),
    ));

    // LastChar at offset 96: int8u
    tags.push(mktag_font(
        "LastChar",
        "Last Char",
        Value::String(format!("{}", data[96])),
    ));

    // DefaultChar at offset 97: int8u
    tags.push(mktag_font(
        "DefaultChar",
        "Default Char",
        Value::String(format!("{}", data[97])),
    ));

    // BreakChar at offset 98: int8u
    tags.push(mktag_font(
        "BreakChar",
        "Break Char",
        Value::String(format!("{}", data[98])),
    ));

    // WidthBytes at offset 99: int16u LE
    let width_bytes = u16::from_le_bytes([data[99], data[100]]);
    tags.push(mktag_font(
        "WidthBytes",
        "Width Bytes",
        Value::String(format!("{}", width_bytes)),
    ));

    // FontName and PostScriptFontName: offset to name string is at bytes 105..108 (int32u LE)
    // The name block contains: FontName\0PostScriptFontName\0
    if data.len() >= 109 {
        let name_off = u32::from_le_bytes([data[105], data[106], data[107], data[108]]) as usize;
        if name_off > 0 && name_off < data.len() {
            let rest = &data[name_off..];
            if let Some(null_pos) = rest.iter().position(|&b| b == 0) {
                let font_name: String = rest[..null_pos]
                    .iter()
                    .filter(|&&b| b >= 0x20)
                    .map(|&b| b as char)
                    .collect();
                if !font_name.is_empty() {
                    tags.push(mktag_font(
                        "FontName",
                        "Font Name",
                        Value::String(font_name),
                    ));
                }
                let rest2 = &rest[null_pos + 1..];
                if let Some(null_pos2) = rest2.iter().position(|&b| b == 0) {
                    let ps_name: String = rest2[..null_pos2]
                        .iter()
                        .filter(|&&b| b >= 0x20)
                        .map(|&b| b as char)
                        .collect();
                    if !ps_name.is_empty() {
                        tags.push(mktag_font(
                            "PostScriptFontName",
                            "PostScript Font Name",
                            Value::String(ps_name),
                        ));
                    }
                }
            }
        }
    }

    Ok(tags)
}

/// Read a null-terminated or fixed-length string from PFM data.
fn pfm_str(data: &[u8], offset: usize, max_len: usize) -> String {
    let end = (offset + max_len).min(data.len());
    if offset >= data.len() {
        return String::new();
    }
    let slice = &data[offset..end];
    let slice = if let Some(null_pos) = slice.iter().position(|&b| b == 0) {
        &slice[..null_pos]
    } else {
        slice
    };
    slice
        .iter()
        .filter(|&&b| b >= 0x20)
        .map(|&b| b as char)
        .collect()
}

/// Create a tag for Printer Font Metrics data.
fn mktag_font(name: &str, description: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "File".to_string(),
            family1: "Font".to_string(),
            family2: "Document".to_string(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}
