//! Fujifilm RAF file format reader.
//!
//! Parses RAF header and embedded JPEG/EXIF data.
//! Mirrors ExifTool's FujiFilm.pm ProcessRAF.
//!
//! RAF directory format (big-endian):
//!   4 bytes: entry count
//!   Per entry:
//!     2 bytes: tag_id
//!     2 bytes: data_len
//!     data_len bytes: raw data

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_raf(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 100 || !data.starts_with(b"FUJIFILMCCD-RAW") {
        return Err(Error::InvalidData("not a Fujifilm RAF file".into()));
    }

    let mut tags = Vec::new();

    // Version at offset 0x3C (4 bytes ASCII, e.g., "0106")
    let version = crate::encoding::decode_utf8_or_latin1(&data[0x3C..0x40]).to_string();
    tags.push(mk("RAFVersion", "RAF Version", Value::String(version)));

    // Camera model (bytes 0x1C-0x3C, null-terminated)
    let model_end = data[0x1C..0x3C]
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(0x20);
    let model = crate::encoding::decode_utf8_or_latin1(&data[0x1C..0x1C + model_end]).to_string();
    if !model.is_empty() {
        tags.push(mk("Model", "Camera Model", Value::String(model)));
    }

    // RAFCompression at 0x6c (if the first byte is 0x00, it's a valid compression tag)
    if data.len() >= 0x70 && data[0x6c] == 0 {
        let compression = u32::from_be_bytes([data[0x6c], data[0x6d], data[0x6e], data[0x6f]]);
        let comp_str = match compression {
            0 => "Uncompressed",
            2 => "Lossless",
            3 => "Lossy",
            _ => "Unknown",
        };
        tags.push(mk_loc(
            "RAFCompression",
            "RAF Compression",
            Value::U32(compression),
            comp_str.to_string(),
        ));
    }

    // JPEG offset at 0x54 (uint32 BE) and length at 0x58
    let jpeg_offset;
    let jpeg_length;
    if data.len() >= 0x5C {
        jpeg_offset = u32::from_be_bytes([data[0x54], data[0x55], data[0x56], data[0x57]]) as usize;
        jpeg_length = u32::from_be_bytes([data[0x58], data[0x59], data[0x5A], data[0x5B]]) as usize;
    } else {
        jpeg_offset = 0;
        jpeg_length = 0;
    }

    // Add PreviewImage tag (binary data of embedded JPEG)
    if jpeg_offset > 0 && jpeg_offset + jpeg_length <= data.len() && jpeg_length > 0 {
        let jpeg_data = &data[jpeg_offset..jpeg_offset + jpeg_length];
        // Add PreviewImage tag
        tags.push(Tag {
            id: TagId::Text("PreviewImage".into()),
            name: "PreviewImage".into(),
            description: "Preview Image".into(),
            group: TagGroup {
                family0: "RAF".into(),
                family1: "RAF".into(),
                family2: "Preview".into(),
            },
            raw_value: Value::Binary(jpeg_data.to_vec()),
            print_value: format!("(Binary data {} bytes)", jpeg_length),
            priority: 0,
        });

        // Try to extract EXIF from embedded JPEG
        if jpeg_data.starts_with(&[0xFF, 0xD8, 0xFF]) {
            if let Ok(jpeg_tags) = crate::formats::jpeg::read_jpeg(jpeg_data) {
                tags.extend(jpeg_tags);
            }
        }
    }

    // RAF directory offset at 0x5C and length at 0x60
    if data.len() >= 0x64 {
        let dir_offset =
            u32::from_be_bytes([data[0x5C], data[0x5D], data[0x5E], data[0x5F]]) as usize;
        let dir_length =
            u32::from_be_bytes([data[0x60], data[0x61], data[0x62], data[0x63]]) as usize;

        if dir_offset > 0 && dir_offset + dir_length <= data.len() {
            parse_raf_directory(&data[dir_offset..dir_offset + dir_length], &mut tags);
        }
    }

    Ok(tags)
}

/// Parse the RAF proprietary directory.
/// Format: 4-byte entry count (BE), then per entry: 2-byte tag_id, 2-byte data_len, data_len bytes of data.
fn parse_raf_directory(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 4 {
        return;
    }

    let num_entries = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
    if num_entries > 256 {
        return; // Sanity check
    }

    let mut pos = 4;
    // FujiLayout flag: set by tag 0x130 if (first_byte & 0x80) != 0
    let mut fuji_layout = false;

    // First pass: determine FujiLayout from tag 0x130
    {
        let mut scan_pos = 4;
        for _ in 0..num_entries {
            if scan_pos + 4 > data.len() {
                break;
            }
            let tag_id = u16::from_be_bytes([data[scan_pos], data[scan_pos + 1]]);
            let data_len = u16::from_be_bytes([data[scan_pos + 2], data[scan_pos + 3]]) as usize;
            scan_pos += 4;
            if scan_pos + data_len > data.len() {
                break;
            }
            if tag_id == 0x130 && data_len >= 1 {
                fuji_layout = (data[scan_pos] & 0x80) != 0;
            }
            scan_pos += data_len;
        }
    }

    for _ in 0..num_entries {
        if pos + 4 > data.len() {
            break;
        }

        let tag_id = u16::from_be_bytes([data[pos], data[pos + 1]]);
        let data_len = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
        pos += 4;

        if pos + data_len > data.len() {
            break;
        }

        let val_data = &data[pos..pos + data_len];
        pos += data_len;

        if let Some(tag) = decode_raf_tag(tag_id, data_len, val_data, fuji_layout) {
            tags.push(tag);
        }
    }
}

/// Decode a single RAF tag into a Tag struct.
fn decode_raf_tag(tag_id: u16, data_len: usize, val_data: &[u8], fuji_layout: bool) -> Option<Tag> {
    match tag_id {
        // RawImageFullSize: int16u[2], stored height-width, display width-height
        0x100 if data_len >= 4 => {
            let height = u16::from_be_bytes([val_data[0], val_data[1]]) as u32;
            let width = u16::from_be_bytes([val_data[2], val_data[3]]) as u32;
            let s = format!("{}x{}", width, height);
            Some(mk_loc(
                "RawImageFullSize",
                "Raw Image Full Size",
                Value::String(s.clone()),
                s,
            ))
        }
        // RawImageCropTopLeft: int16u[2] (top_margin, left_margin)
        0x110 if data_len >= 4 => {
            let top = u16::from_be_bytes([val_data[0], val_data[1]]);
            let left = u16::from_be_bytes([val_data[2], val_data[3]]);
            let s = format!("{} {}", top, left);
            Some(mk_loc(
                "RawImageCropTopLeft",
                "Raw Image Crop Top Left",
                Value::String(s.clone()),
                s,
            ))
        }
        // RawImageCroppedSize: int16u[2], stored height-width, display width-height
        0x111 if data_len >= 4 => {
            let height = u16::from_be_bytes([val_data[0], val_data[1]]) as u32;
            let width = u16::from_be_bytes([val_data[2], val_data[3]]) as u32;
            let s = format!("{}x{}", width, height);
            Some(mk_loc(
                "RawImageCroppedSize",
                "Raw Image Cropped Size",
                Value::String(s.clone()),
                s,
            ))
        }
        // RawImageSize: int16u[2], height then width, with FujiLayout adjustment
        0x121 if data_len >= 4 => {
            let mut height = u16::from_be_bytes([val_data[0], val_data[1]]) as u32;
            let mut width = u16::from_be_bytes([val_data[2], val_data[3]]) as u32;
            if fuji_layout {
                width /= 2;
                height *= 2;
            }
            let s = format!("{}x{}", width, height);
            Some(mk_loc(
                "RawImageSize",
                "Raw Image Size",
                Value::String(s.clone()),
                s,
            ))
        }
        // FujiLayout: int8u[4]
        0x130 => {
            let bytes: Vec<u8> = val_data[..data_len.min(4)].to_vec();
            let s = bytes
                .iter()
                .map(|b| b.to_string())
                .collect::<Vec<_>>()
                .join(" ");
            Some(mk_loc(
                "FujiLayout",
                "Fuji Layout",
                Value::String(s.clone()),
                s,
            ))
        }
        // WB_GRGBLevelsAuto: int16u[4] (take first 4 values only)
        0x2000 if data_len >= 8 => Some(decode_wb_grgb(
            val_data,
            "WB_GRGBLevelsAuto",
            "WB GRGB Levels Auto",
        )),
        // WB_GRGBLevelsDaylight
        0x2100 if data_len >= 8 => Some(decode_wb_grgb(
            val_data,
            "WB_GRGBLevelsDaylight",
            "WB GRGB Levels Daylight",
        )),
        // WB_GRGBLevelsCloudy
        0x2200 if data_len >= 8 => Some(decode_wb_grgb(
            val_data,
            "WB_GRGBLevelsCloudy",
            "WB GRGB Levels Cloudy",
        )),
        // WB_GRGBLevelsDaylightFluor
        0x2300 if data_len >= 8 => Some(decode_wb_grgb(
            val_data,
            "WB_GRGBLevelsDaylightFluor",
            "WB GRGB Levels Daylight Fluor",
        )),
        // WB_GRGBLevelsDayWhiteFluor
        0x2301 if data_len >= 8 => Some(decode_wb_grgb(
            val_data,
            "WB_GRGBLevelsDayWhiteFluor",
            "WB GRGB Levels Day White Fluor",
        )),
        // WB_GRGBLevelsWhiteFluorescent
        0x2302 if data_len >= 8 => Some(decode_wb_grgb(
            val_data,
            "WB_GRGBLevelsWhiteFluorescent",
            "WB GRGB Levels White Fluorescent",
        )),
        // WB_GRGBLevelsWarmWhiteFluor
        0x2310 if data_len >= 8 => Some(decode_wb_grgb(
            val_data,
            "WB_GRGBLevelsWarmWhiteFluor",
            "WB GRGB Levels Warm White Fluor",
        )),
        // WB_GRGBLevelsLivingRoomWarmWhiteFluor
        0x2311 if data_len >= 8 => Some(decode_wb_grgb(
            val_data,
            "WB_GRGBLevelsLivingRoomWarmWhiteFluor",
            "WB GRGB Levels Living Room Warm White Fluor",
        )),
        // WB_GRGBLevelsTungsten
        0x2400 if data_len >= 8 => Some(decode_wb_grgb(
            val_data,
            "WB_GRGBLevelsTungsten",
            "WB GRGB Levels Tungsten",
        )),
        // WB_GRGBLevels
        0x2ff0 if data_len >= 8 => {
            Some(decode_wb_grgb(val_data, "WB_GRGBLevels", "WB GRGB Levels"))
        }
        // RelativeExposure: rational32s = int16s numerator + int16s denominator (4 bytes total)
        // ValueConv: log($val) / log(2); PrintConv: sprintf("%+.1f",$val) or 0
        0x9200 if data_len >= 4 => {
            let n = i16::from_be_bytes([val_data[0], val_data[1]]) as f64;
            let d = i16::from_be_bytes([val_data[2], val_data[3]]) as f64;
            if d != 0.0 {
                let ratio = n / d;
                let value = if ratio > 0.0 {
                    ratio.ln() / 2.0_f64.ln()
                } else if ratio == 0.0 {
                    0.0
                } else {
                    return None;
                };
                let print = if value == 0.0 {
                    "0".to_string()
                } else {
                    format!("{:+.1}", value)
                };
                Some(mk_loc(
                    "RelativeExposure",
                    "Relative Exposure",
                    Value::F64(value),
                    print,
                ))
            } else {
                None
            }
        }
        // RawExposureBias: rational32s = int16s/int16s (4 bytes)
        // PrintConv: sprintf("%+.1f",$val) or 0
        0x9650 if data_len >= 4 => {
            let n = i16::from_be_bytes([val_data[0], val_data[1]]) as f64;
            let d = i16::from_be_bytes([val_data[2], val_data[3]]) as f64;
            if d != 0.0 {
                let value = n / d;
                let print = if value == 0.0 {
                    "0".to_string()
                } else {
                    format!("{:+.1}", value)
                };
                Some(mk_loc(
                    "RawExposureBias",
                    "Raw Exposure Bias",
                    Value::F64(value),
                    print,
                ))
            } else {
                None
            }
        }
        _ => None, // Unknown or unhandled tag
    }
}

/// Decode a WB_GRGB tag from int16u[4] (big-endian).
/// Only takes the first 4 u16 values (G, R, G, B).
fn decode_wb_grgb(val_data: &[u8], name: &str, description: &str) -> Tag {
    let g1 = u16::from_be_bytes([val_data[0], val_data[1]]);
    let r = u16::from_be_bytes([val_data[2], val_data[3]]);
    let g2 = u16::from_be_bytes([val_data[4], val_data[5]]);
    let b = u16::from_be_bytes([val_data[6], val_data[7]]);
    let s = format!("{} {} {} {}", g1, r, g2, b);
    mk_loc(name, description, Value::String(s.clone()), s)
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

fn mk_loc(name: &str, description: &str, value: Value, print: String) -> Tag {
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
        print_value: print,
        priority: 0,
    }
}
