//! WordPerfect Graphics format reader.

use super::misc::mktag;
use crate::error::{Error, Result};
use crate::tag::Tag;
use crate::value::Value;

pub fn read_wpg(data: &[u8]) -> Result<Vec<Tag>> {
    // WPG magic: FF 57 50 43
    if data.len() < 16 || &data[0..4] != b"\xff\x57\x50\x43" {
        return Err(Error::InvalidData("not a WPG file".into()));
    }

    let mut tags = Vec::new();

    // Offset to first record (little-endian u32 at bytes 4-7)
    let offset = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
    // Version at bytes 10-11
    let ver = data[10];
    let rev = data[11];
    tags.push(mktag(
        "WPG",
        "WPGVersion",
        "WPG Version",
        Value::String(format!("{}.{}", ver, rev)),
    ));

    if !(1..=2).contains(&ver) {
        return Ok(tags);
    }

    // Determine start position
    let mut pos = if offset > 16 { offset } else { 16 };
    if pos > data.len() {
        pos = data.len();
    }

    let mut records: Vec<String> = Vec::new();
    let mut last_type: Option<u32> = None;
    let mut count = 0usize;
    let mut image_width_inches: Option<f64> = None;
    let mut image_height_inches: Option<f64> = None;

    // WPG v1 record map
    let v1_map: std::collections::HashMap<u32, &str> = [
        (0x01, "Fill Attributes"),
        (0x02, "Line Attributes"),
        (0x03, "Marker Attributes"),
        (0x04, "Polymarker"),
        (0x05, "Line"),
        (0x06, "Polyline"),
        (0x07, "Rectangle"),
        (0x08, "Polygon"),
        (0x09, "Ellipse"),
        (0x0a, "Reserved"),
        (0x0b, "Bitmap (Type 1)"),
        (0x0c, "Graphics Text (Type 1)"),
        (0x0d, "Graphics Text Attributes"),
        (0x0e, "Color Map"),
        (0x0f, "Start WPG (Type 1)"),
        (0x10, "End WPG"),
        (0x11, "PostScript Data (Type 1)"),
        (0x12, "Output Attributes"),
        (0x13, "Curved Polyline"),
        (0x14, "Bitmap (Type 2)"),
        (0x15, "Start Figure"),
        (0x16, "Start Chart"),
        (0x17, "PlanPerfect Data"),
        (0x18, "Graphics Text (Type 2)"),
        (0x19, "Start WPG (Type 2)"),
        (0x1a, "Graphics Text (Type 3)"),
        (0x1b, "PostScript Data (Type 2)"),
    ]
    .iter()
    .cloned()
    .collect();

    // WPG v2 record map
    let v2_map: std::collections::HashMap<u32, &str> = [
        (0x00, "End Marker"),
        (0x01, "Start WPG"),
        (0x02, "End WPG"),
        (0x03, "Form Settings"),
        (0x04, "Ruler Settings"),
        (0x05, "Grid Settings"),
        (0x06, "Layer"),
        (0x08, "Pen Style Definition"),
        (0x09, "Pattern Definition"),
        (0x0a, "Comment"),
        (0x0b, "Color Transfer"),
        (0x0c, "Color Palette"),
        (0x0d, "DP Color Palette"),
        (0x0e, "Bitmap Data"),
        (0x0f, "Text Data"),
        (0x10, "Chart Style"),
        (0x11, "Chart Data"),
        (0x12, "Object Image"),
        (0x15, "Polyline"),
        (0x16, "Polyspline"),
        (0x17, "Polycurve"),
        (0x18, "Rectangle"),
        (0x19, "Arc"),
        (0x1a, "Compound Polygon"),
        (0x1b, "Bitmap"),
        (0x1c, "Text Line"),
        (0x1d, "Text Block"),
        (0x1e, "Text Path"),
        (0x1f, "Chart"),
        (0x20, "Group"),
        (0x21, "Object Capsule"),
        (0x22, "Font Settings"),
        (0x25, "Pen Fore Color"),
        (0x26, "DP Pen Fore Color"),
        (0x27, "Pen Back Color"),
        (0x28, "DP Pen Back Color"),
        (0x29, "Pen Style"),
        (0x2a, "Pen Pattern"),
        (0x2b, "Pen Size"),
        (0x2c, "DP Pen Size"),
        (0x2d, "Line Cap"),
        (0x2e, "Line Join"),
        (0x2f, "Brush Gradient"),
        (0x30, "DP Brush Gradient"),
        (0x31, "Brush Fore Color"),
        (0x32, "DP Brush Fore Color"),
        (0x33, "Brush Back Color"),
        (0x34, "DP Brush Back Color"),
        (0x35, "Brush Pattern"),
        (0x36, "Horizontal Line"),
        (0x37, "Vertical Line"),
        (0x38, "Poster Settings"),
        (0x39, "Image State"),
        (0x3a, "Envelope Definition"),
        (0x3b, "Envelope"),
        (0x3c, "Texture Definition"),
        (0x3d, "Brush Texture"),
        (0x3e, "Texture Alignment"),
        (0x3f, "Pen Texture "),
    ]
    .iter()
    .cloned()
    .collect();

    let mut safety = 0;
    loop {
        if pos >= data.len() || safety > 10000 {
            break;
        }
        safety += 1;

        let (record_type, len, get_size) = if ver == 1 {
            if pos >= data.len() {
                break;
            }
            let rtype = data[pos] as u32;
            pos += 1;
            // Read var-int length
            let (l, advance) = read_wpg_varint(data, pos);
            pos += advance;
            let gs = rtype == 0x0f; // Start WPG (Type 1)
            (rtype, l, gs)
        } else {
            // Version 2: read 2 bytes for flags+type
            if pos + 1 >= data.len() {
                break;
            }
            let rtype = data[pos + 1] as u32;
            pos += 2;
            // Skip extensions (var-int)
            let (_, adv) = read_wpg_varint(data, pos);
            pos += adv;
            // Read record length (var-int)
            let (l, adv2) = read_wpg_varint(data, pos);
            pos += adv2;
            let gs = rtype == 0x01; // Start WPG
            let rtype_opt = if rtype > 0x3f { u32::MAX } else { rtype };
            (rtype_opt, l, gs)
        };

        if record_type == u32::MAX {
            // Skip unknown v2 record
            pos += len;
            continue;
        }

        if get_size {
            // Read Start record to get image dimensions
            let rec_end = pos + len;
            if rec_end > data.len() {
                break;
            }
            let rec = &data[pos..rec_end];
            pos = rec_end;

            if ver == 1 && rec.len() >= 6 {
                // v1: skip 2 bytes, then u16 width, u16 height
                let w = u16::from_le_bytes([rec[2], rec[3]]) as f64;
                let h = u16::from_le_bytes([rec[4], rec[5]]) as f64;
                image_width_inches = Some(w / 1200.0);
                image_height_inches = Some(h / 1200.0);
            } else if ver == 2 && rec.len() >= 21 {
                // v2: xres(u16), yres(u16), precision(u8), then coordinates
                let xres = u16::from_le_bytes([rec[0], rec[1]]) as f64;
                let yres = u16::from_le_bytes([rec[2], rec[3]]) as f64;
                let precision = rec[4];
                let (x1, y1, x2, y2) = if precision == 0 && rec.len() >= 21 {
                    // int16s x4 at offset 13
                    let x1 = i16::from_le_bytes([rec[13], rec[14]]) as f64;
                    let y1 = i16::from_le_bytes([rec[15], rec[16]]) as f64;
                    let x2 = i16::from_le_bytes([rec[17], rec[18]]) as f64;
                    let y2 = i16::from_le_bytes([rec[19], rec[20]]) as f64;
                    (x1, y1, x2, y2)
                } else if precision == 1 && rec.len() >= 29 {
                    // int32s x4 at offset 13
                    let x1 = i32::from_le_bytes([rec[13], rec[14], rec[15], rec[16]]) as f64;
                    let y1 = i32::from_le_bytes([rec[17], rec[18], rec[19], rec[20]]) as f64;
                    let x2 = i32::from_le_bytes([rec[21], rec[22], rec[23], rec[24]]) as f64;
                    let y2 = i32::from_le_bytes([rec[25], rec[26], rec[27], rec[28]]) as f64;
                    (x1, y1, x2, y2)
                } else {
                    pos += 0; // skip
                              // Emit last_type
                    if let Some(lt) = last_type.take() {
                        let _val = if count > 1 {
                            format!("{}x{}", lt, count)
                        } else {
                            format!("{}", lt)
                        };
                        records.push(format_wpg_record(
                            lt,
                            count,
                            if ver == 1 { &v1_map } else { &v2_map },
                        ));
                    }
                    last_type = Some(record_type);
                    count = 1;
                    continue;
                };
                let w = (x2 - x1).abs();
                let h = (y2 - y1).abs();
                let xres_div = if xres == 0.0 { 1200.0 } else { xres };
                let yres_div = if yres == 0.0 { 1200.0 } else { yres };
                image_width_inches = Some(w / xres_div);
                image_height_inches = Some(h / yres_div);
            }
        } else {
            pos += len;
        }

        // Accumulate records (collapse sequential identical types)
        if last_type == Some(record_type) {
            count += 1;
        } else {
            if let Some(lt) = last_type.take() {
                records.push(format_wpg_record(
                    lt,
                    count,
                    if ver == 1 { &v1_map } else { &v2_map },
                ));
            }
            if record_type == 0 && ver == 2 {
                break;
            } // End Marker
            last_type = Some(record_type);
            count = 1;
        }
    }
    // Emit last record
    if let Some(lt) = last_type.take() {
        records.push(format_wpg_record(
            lt,
            count,
            if ver == 1 { &v1_map } else { &v2_map },
        ));
    }

    if let Some(w) = image_width_inches {
        tags.push(mktag(
            "WPG",
            "ImageWidthInches",
            "Image Width Inches",
            Value::String(format!("{:.2}", w)),
        ));
    }
    if let Some(h) = image_height_inches {
        tags.push(mktag(
            "WPG",
            "ImageHeightInches",
            "Image Height Inches",
            Value::String(format!("{:.2}", h)),
        ));
    }
    if !records.is_empty() {
        let joined = records.join(", ");
        tags.push(mktag("WPG", "Records", "Records", Value::String(joined)));
    }

    Ok(tags)
}

fn format_wpg_record(
    rtype: u32,
    count: usize,
    map: &std::collections::HashMap<u32, &str>,
) -> String {
    let name = map
        .get(&rtype)
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("Unknown (0x{:02x})", rtype));
    if count > 1 {
        format!("{} x {}", name, count)
    } else {
        name
    }
}

fn read_wpg_varint(data: &[u8], pos: usize) -> (usize, usize) {
    if pos >= data.len() {
        return (0, 0);
    }
    let first = data[pos] as usize;
    if first != 0xFF {
        return (first, 1);
    }
    // 0xFF → read 2 more bytes as u16 LE
    if pos + 2 >= data.len() {
        return (0, 1);
    }
    let val = u16::from_le_bytes([data[pos + 1], data[pos + 2]]) as usize;
    if val & 0x8000 != 0 {
        // Read 2 more bytes
        if pos + 4 >= data.len() {
            return (val & 0x7FFF, 3);
        }
        let hi = u16::from_le_bytes([data[pos + 3], data[pos + 4]]) as usize;
        let full = ((val & 0x7FFF) << 16) | hi;
        return (full, 5);
    }
    (val, 3)
}
