//! OpenEXR image format reader.
//!
//! Reads header attributes from EXR files.
//! Mirrors ExifTool's OpenEXR.pm ProcessEXR().

use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

fn mk(name: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: "OpenEXR".into(),
            family1: "OpenEXR".into(),
            family2: "Image".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}

fn mk_with_print(name: &str, raw: Value, print: String) -> Tag {
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: "OpenEXR".into(),
            family1: "OpenEXR".into(),
            family2: "Image".into(),
        },
        raw_value: raw,
        print_value: print,
        priority: 0,
    }
}

fn read_le_u32(data: &[u8], off: usize) -> Option<u32> {
    if off + 4 > data.len() { return None; }
    Some(u32::from_le_bytes([data[off], data[off+1], data[off+2], data[off+3]]))
}



fn read_le_u8(data: &[u8], off: usize) -> Option<u8> {
    data.get(off).copied()
}

/// Read null-terminated string from data starting at pos, max_len bytes
fn read_cstr(data: &[u8], pos: usize, max_len: usize) -> Option<(&str, usize)> {
    let end = data[pos..].iter().take(max_len + 1).position(|&b| b == 0)?;
    let s = std::str::from_utf8(&data[pos..pos + end]).ok()?;
    Some((s, pos + end + 1))
}

pub fn read_openexr(data: &[u8]) -> crate::error::Result<Vec<Tag>> {
    if data.len() < 8 {
        return Ok(Vec::new());
    }
    // Magic: 0x762f3101 (little-endian)
    if &data[0..4] != &[0x76, 0x2f, 0x31, 0x01] {
        return Ok(Vec::new());
    }

    let flags = read_le_u32(data, 4).unwrap_or(0);
    let version = flags & 0xff;
    let flags_bits = flags & 0xffffff00;
    let max_name_len = if flags & 0x400 != 0 { 255 } else { 31 };

    let mut tags = Vec::new();

    // EXRVersion
    tags.push(mk("EXRVersion", Value::String(version.to_string())));

    // Flags
    {
        let mut flag_parts = Vec::new();
        if flags_bits & (1 << 9) != 0 { flag_parts.push("Tiled"); }
        if flags_bits & (1 << 10) != 0 { flag_parts.push("Long names"); }
        if flags_bits & (1 << 11) != 0 { flag_parts.push("Deep data"); }
        if flags_bits & (1 << 12) != 0 { flag_parts.push("Multipart"); }
        let flag_str = if flag_parts.is_empty() { "(none)".to_string() } else { flag_parts.join(", ") };
        tags.push(mk_with_print("Flags", Value::U32(flags_bits), flag_str));
    }

    let mut pos = 8;
    let mut data_window: Option<[i32; 4]> = None;
    let mut display_window: Option<[i32; 4]> = None;

    // Parse attributes
    loop {
        if pos >= data.len() { break; }
        if data[pos] == 0 { break; } // end of header

        // Read attribute name
        let (attr_name, next) = match read_cstr(data, pos, max_name_len) {
            Some(v) => v,
            None => break,
        };
        let attr_name = attr_name.to_string();
        pos = next;

        if pos >= data.len() { break; }

        // Read attribute type
        let (attr_type, next) = match read_cstr(data, pos, max_name_len) {
            Some(v) => v,
            None => break,
        };
        let _attr_type = attr_type.to_string();
        pos = next;

        // Read size
        let size = match read_le_u32(data, pos) {
            Some(v) => v as usize,
            None => break,
        };
        pos += 4;

        if pos + size > data.len() { break; }
        let val_data = &data[pos..pos + size];
        pos += size;

        // Process the attribute
        match attr_name.as_str() {
            "channels" => {
                // chlist: null-terminated channel entries
                let mut channels = Vec::new();
                let mut cp = 0;
                while cp < val_data.len() {
                    if val_data[cp] == 0 { break; }
                    // Read channel name (null-terminated, max 31)
                    let end = val_data[cp..].iter().take(32).position(|&b| b == 0);
                    let end = match end { Some(e) => e, None => break };
                    let ch_name = std::str::from_utf8(&val_data[cp..cp + end]).unwrap_or("?");
                    cp += end + 1;
                    // Read pixel type (4), linear (1), x sampling (4), y sampling (4) = 16 bytes total from name end, but ptype is first
                    if cp + 16 > val_data.len() { break; }
                    let pix_type = u32::from_le_bytes([val_data[cp], val_data[cp+1], val_data[cp+2], val_data[cp+3]]);
                    let linear = val_data[cp + 4];
                    // skip 3 reserved bytes
                    let x_samp = u32::from_le_bytes([val_data[cp+8], val_data[cp+9], val_data[cp+10], val_data[cp+11]]);
                    let y_samp = u32::from_le_bytes([val_data[cp+12], val_data[cp+13], val_data[cp+14], val_data[cp+15]]);
                    cp += 16;
                    let pix_str = match pix_type {
                        0 => "int8u",
                        1 => "half",
                        2 => "float",
                        _ => "unknown",
                    };
                    let lin_str = if linear != 0 { " linear" } else { "" };
                    channels.push(format!("{} {}{} {} {}", ch_name, pix_str, lin_str, x_samp, y_samp));
                }
                let ch_str = channels.join(", ");
                tags.push(mk("Channels", Value::String(ch_str)));
            }
            "compression" => {
                if let Some(v) = read_le_u8(val_data, 0) {
                    let comp_str = match v {
                        0 => "None",
                        1 => "RLE",
                        2 => "ZIPS",
                        3 => "ZIP",
                        4 => "PIZ",
                        5 => "PXR24",
                        6 => "B44",
                        7 => "B44A",
                        8 => "DWAA",
                        9 => "DWAB",
                        _ => "Unknown",
                    };
                    tags.push(mk_with_print("Compression", Value::U8(v), comp_str.to_string()));
                }
            }
            "dataWindow" => {
                if val_data.len() >= 16 {
                    let x1 = i32::from_le_bytes([val_data[0], val_data[1], val_data[2], val_data[3]]);
                    let y1 = i32::from_le_bytes([val_data[4], val_data[5], val_data[6], val_data[7]]);
                    let x2 = i32::from_le_bytes([val_data[8], val_data[9], val_data[10], val_data[11]]);
                    let y2 = i32::from_le_bytes([val_data[12], val_data[13], val_data[14], val_data[15]]);
                    data_window = Some([x1, y1, x2, y2]);
                    tags.push(mk("DataWindow", Value::String(format!("{} {} {} {}", x1, y1, x2, y2))));
                }
            }
            "displayWindow" => {
                if val_data.len() >= 16 {
                    let x1 = i32::from_le_bytes([val_data[0], val_data[1], val_data[2], val_data[3]]);
                    let y1 = i32::from_le_bytes([val_data[4], val_data[5], val_data[6], val_data[7]]);
                    let x2 = i32::from_le_bytes([val_data[8], val_data[9], val_data[10], val_data[11]]);
                    let y2 = i32::from_le_bytes([val_data[12], val_data[13], val_data[14], val_data[15]]);
                    if display_window.is_none() {
                        display_window = Some([x1, y1, x2, y2]);
                    }
                    tags.push(mk("DisplayWindow", Value::String(format!("{} {} {} {}", x1, y1, x2, y2))));
                }
            }
            "lineOrder" => {
                if let Some(v) = read_le_u8(val_data, 0) {
                    let lo_str = match v {
                        0 => "Increasing Y",
                        1 => "Decreasing Y",
                        2 => "Random Y",
                        _ => "Unknown",
                    };
                    tags.push(mk_with_print("LineOrder", Value::U8(v), lo_str.to_string()));
                }
            }
            "pixelAspectRatio" => {
                if val_data.len() >= 4 {
                    let v = f32::from_bits(u32::from_le_bytes([val_data[0], val_data[1], val_data[2], val_data[3]]));
                    // Format like Perl: integer if whole, else float
                    let s = if v.fract() == 0.0 && v >= 0.0 && v < 1e9 {
                        format!("{}", v as i64)
                    } else {
                        format!("{}", v)
                    };
                    tags.push(mk("PixelAspectRatio", Value::String(s)));
                }
            }
            "screenWindowCenter" => {
                if val_data.len() >= 8 {
                    let x = f32::from_bits(u32::from_le_bytes([val_data[0], val_data[1], val_data[2], val_data[3]]));
                    let y = f32::from_bits(u32::from_le_bytes([val_data[4], val_data[5], val_data[6], val_data[7]]));
                    let xs = format_float(x);
                    let ys = format_float(y);
                    tags.push(mk("ScreenWindowCenter", Value::String(format!("{} {}", xs, ys))));
                }
            }
            "screenWindowWidth" => {
                if val_data.len() >= 4 {
                    let v = f32::from_bits(u32::from_le_bytes([val_data[0], val_data[1], val_data[2], val_data[3]]));
                    tags.push(mk("ScreenWindowWidth", Value::String(format_float(v))));
                }
            }
            _ => {
                // Skip unknown attributes
            }
        }
    }

    // Calculate image dimensions from dataWindow (fallback to displayWindow)
    let dim = data_window.or(display_window);
    if let Some([x1, y1, x2, y2]) = dim {
        let w = (x2 - x1 + 1) as u32;
        let h = (y2 - y1 + 1) as u32;
        tags.push(mk("ImageWidth", Value::U32(w)));
        tags.push(mk("ImageHeight", Value::U32(h)));
    }

    Ok(tags)
}

fn format_float(v: f32) -> String {
    if v.fract() == 0.0 && v.abs() < 1e9 {
        format!("{}", v as i64)
    } else {
        format!("{}", v)
    }
}
