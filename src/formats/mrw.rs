//! Minolta MRW file format reader.
//!
//! Parses MRW segments: PRD (dimensions), WBG (white balance), TTW (EXIF), RIF (image format).
//! Mirrors ExifTool's MinoltaRaw.pm.

use crate::error::{Error, Result};
use crate::metadata::ExifReader;
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_mrw(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 8 || data[0] != 0 || data[1] != b'M' || data[2] != b'R' {
        return Err(Error::InvalidData("not a MRW file".into()));
    }

    let mut tags = Vec::new();
    let data_offset = u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize + 8;
    let mut pos = 8;

    while pos + 8 <= data_offset.min(data.len()) {
        let seg_tag = &data[pos..pos + 4];
        let length = u32::from_be_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]) as usize;
        pos += 8;

        if pos + length > data.len() {
            break;
        }

        let seg_data = &data[pos..pos + length];

        match seg_tag {
            b"\0TTW" => {
                // TIFF/EXIF data
                if let Ok(exif_tags) = ExifReader::read(seg_data) {
                    tags.extend(exif_tags);
                }
            }
            b"\0PRD" => {
                // Picture Raw Dimensions — from Perl MinoltaRaw::PRD (binary data, big-endian)
                // Offset 0:  FirmwareID (string[8])
                // Offset 8:  SensorHeight (int16u)
                // Offset 10: SensorWidth  (int16u)
                // Offset 12: ImageHeight  (int16u)
                // Offset 14: ImageWidth   (int16u)
                // Offset 16: RawDepth     (int8u)
                // Offset 17: BitDepth     (int8u)
                // Offset 18: StorageMethod (int8u)
                // Offset 23: BayerPattern  (int8u)
                if length >= 8 {
                    let fw = crate::encoding::decode_utf8_or_latin1(&seg_data[0..8.min(length)]).trim_end_matches('\0').to_string();
                    if !fw.is_empty() {
                        tags.push(mk_str("FirmwareID", "Firmware ID", fw.clone()));
                    }
                }
                if length >= 12 {
                    let sensor_h = u16::from_be_bytes([seg_data[8], seg_data[9]]);
                    let sensor_w = u16::from_be_bytes([seg_data[10], seg_data[11]]);
                    tags.push(mk_u16("SensorHeight", "Sensor Height", sensor_h));
                    tags.push(mk_u16("SensorWidth", "Sensor Width", sensor_w));
                }
                if length >= 16 {
                    let image_h = u16::from_be_bytes([seg_data[12], seg_data[13]]);
                    let image_w = u16::from_be_bytes([seg_data[14], seg_data[15]]);
                    tags.push(mk_u16("ImageHeight", "Image Height", image_h));
                    tags.push(mk_u16("ImageWidth", "Image Width", image_w));
                }
                if length >= 17 {
                    let raw_depth = seg_data[16];
                    tags.push(mk_u8("RawDepth", "Raw Depth", raw_depth));
                }
                if length >= 18 {
                    let bit_depth = seg_data[17];
                    tags.push(mk_u8("BitDepth", "Bit Depth", bit_depth));
                }
                if length >= 19 {
                    let storage = seg_data[18];
                    let storage_str = match storage {
                        82 => "Padded".to_string(),
                        89 => "Linear".to_string(),
                        v  => v.to_string(),
                    };
                    tags.push(mk_str("StorageMethod", "Storage Method", storage_str));
                }
                if length >= 24 {
                    let bayer = seg_data[23];
                    let bayer_str = match bayer {
                        1 => "RGGB".to_string(),
                        4 => "GBRG".to_string(),
                        v => v.to_string(),
                    };
                    tags.push(mk_str("BayerPattern", "Bayer Pattern", bayer_str));
                }
            }
            b"\0WBG" => {
                // White Balance Gains — from Perl MinoltaRaw::WBG
                // Offset 0: WBScale (int8u[4])
                // Offset 4: WB_RGGBLevels (int16u[4]) — for most models
                if length >= 4 {
                    let w0 = seg_data[0];
                    let w1 = seg_data[1];
                    let w2 = seg_data[2];
                    let w3 = seg_data[3];
                    tags.push(mk_str("WBScale", "White Balance Scale",
                        format!("{} {} {} {}", w0, w1, w2, w3)));
                }
                if length >= 12 {
                    let r  = u16::from_be_bytes([seg_data[4],  seg_data[5]]);
                    let g1 = u16::from_be_bytes([seg_data[6],  seg_data[7]]);
                    let g2 = u16::from_be_bytes([seg_data[8],  seg_data[9]]);
                    let b  = u16::from_be_bytes([seg_data[10], seg_data[11]]);
                    tags.push(mk_str("WB_RGGBLevels", "WB RGGB Levels",
                        format!("{} {} {} {}", r, g1, g2, b)));
                    // Also compute Red/Blue balance as float
                    if g1 > 0 {
                        let red_bal = r as f64 / g1 as f64;
                        tags.push(mk_str("RedBalance", "Red Balance",
                            format!("{:.6}", red_bal)));
                    }
                    if g2 > 0 {
                        let blue_bal = b as f64 / g2 as f64;
                        tags.push(mk_str("BlueBalance", "Blue Balance",
                            format!("{:.6}", blue_bal)));
                    }
                }
            }
            b"\0RIF" => {
                // Requested Image Format — from Perl MinoltaRaw::RIF (binary data, big-endian)
                // Offset 1:  Saturation   (int8s)
                // Offset 2:  Contrast     (int8s)
                // Offset 3:  Sharpness    (int8s)
                // Offset 4:  WBMode       (int8u, special encoding)
                // Offset 5:  ProgramMode  (int8u)
                // Offset 6:  ISOSetting   (int8u, formula: 2^((val-48)/8)*100)
                // Offset 7:  ColorMode    (int8u)
                // Offset 8:  WB_RBLevelsTungsten (int16u[2])
                // Offset 12: WB_RBLevelsDaylight (int16u[2])
                // Offset 16: WB_RBLevelsCloudy   (int16u[2])
                // Offset 20: WB_RBLevelsCoolWhiteF (int16u[2])
                // Offset 24: WB_RBLevelsFlash    (int16u[2])
                // Offset 28: WB_RBLevelsCustom   (int16u[2])
                // Offset 56: ColorFilter  (int8s)
                // Offset 57: BWFilter     (int8u)
                // Offset 58: ZoneMatching (int8u)
                // Offset 59: Hue          (int8s)
                if length > 1 {
                    let sat = seg_data[1] as i8;
                    tags.push(mk_str("Saturation", "Saturation", sat.to_string()));
                }
                if length > 2 {
                    let con = seg_data[2] as i8;
                    tags.push(mk_str("Contrast", "Contrast", con.to_string()));
                }
                if length > 3 {
                    let sharp = seg_data[3] as i8;
                    tags.push(mk_str("Sharpness", "Sharpness", sharp.to_string()));
                }
                if length > 4 {
                    let wbmode = seg_data[4];
                    tags.push(mk_str("WBMode", "WB Mode", convert_wb_mode(wbmode)));
                }
                if length > 5 {
                    let prog = seg_data[5];
                    let prog_str = match prog {
                        0 => "None".to_string(),
                        1 => "Portrait".to_string(),
                        2 => "Text".to_string(),
                        3 => "Night Portrait".to_string(),
                        4 => "Sunset".to_string(),
                        5 => "Sports".to_string(),
                        v => v.to_string(),
                    };
                    tags.push(mk_str("ProgramMode", "Program Mode", prog_str));
                }
                if length > 6 {
                    let iso_raw = seg_data[6];
                    if iso_raw != 255 {
                        let iso_str = match iso_raw {
                            0   => "Auto".to_string(),
                            174 => "80 (Zone Matching Low)".to_string(),
                            184 => "200 (Zone Matching High)".to_string(),
                            v   => {
                                let iso_val = (2f64.powf((v as f64 - 48.0) / 8.0) * 100.0 + 0.5) as u32;
                                iso_val.to_string()
                            }
                        };
                        tags.push(mk_str("ISOSetting", "ISO Setting", iso_str));
                    }
                }
                if length > 7 {
                    let cm = seg_data[7];
                    let cm_str = convert_minolta_color_mode(cm as u32);
                    tags.push(mk_str("ColorMode", "Color Mode", cm_str));
                }
                // WB_RBLevels (only for Minolta PRD models — always true for MRW files)
                for (name, offset) in &[
                    ("WB_RBLevelsTungsten",  8usize),
                    ("WB_RBLevelsDaylight",  12),
                    ("WB_RBLevelsCloudy",    16),
                    ("WB_RBLevelsCoolWhiteF",20),
                    ("WB_RBLevelsFlash",     24),
                    ("WB_RBLevelsCustom",    28),
                ] {
                    if length >= offset + 4 {
                        let r = u16::from_be_bytes([seg_data[*offset],     seg_data[*offset + 1]]);
                        let b = u16::from_be_bytes([seg_data[*offset + 2], seg_data[*offset + 3]]);
                        tags.push(mk_str(name, name, format!("{} {}", r, b)));
                    }
                }
                if length > 56 {
                    let cf = seg_data[56] as i8;
                    tags.push(mk_str("ColorFilter", "Color Filter", cf.to_string()));
                }
                if length > 57 {
                    tags.push(mk_str("BWFilter", "BW Filter", seg_data[57].to_string()));
                }
                if length > 58 {
                    let zm = seg_data[58];
                    let zm_str = match zm {
                        0 => "ISO Setting Used".to_string(),
                        1 => "High Key".to_string(),
                        2 => "Low Key".to_string(),
                        v => v.to_string(),
                    };
                    tags.push(mk_str("ZoneMatching", "Zone Matching", zm_str));
                }
                if length > 59 {
                    let hue = seg_data[59] as i8;
                    tags.push(mk_str("Hue", "Hue", hue.to_string()));
                }
            }
            _ => {}
        }

        pos += length;
    }

    Ok(tags)
}

/// Convert WBMode byte to string (matches Perl MinoltaRaw::ConvertWBMode).
fn convert_wb_mode(val: u8) -> String {
    let wb_map: &[(u8, &str)] = &[
        (0, "Auto"), (1, "Daylight"), (2, "Cloudy"), (3, "Tungsten"),
        (4, "Flash/Fluorescent"), (5, "Fluorescent"), (6, "Shade"),
        (7, "User 1"), (8, "User 2"), (9, "User 3"), (10, "Temperature"),
    ];
    let lo = val & 0x0f;
    let hi = val >> 4;
    let base = wb_map.iter().find(|&&(k, _)| k == lo)
        .map(|&(_, v)| v)
        .unwrap_or("Unknown");
    let mut s = base.to_string();
    if hi >= 6 && hi <= 12 {
        s.push_str(&format!(" ({})", hi as i8 - 8));
    }
    s
}

/// Convert Minolta ColorMode value to string (matches Perl %minoltaColorMode).
fn convert_minolta_color_mode(val: u32) -> String {
    match val {
        0  => "Natural color".to_string(),
        1  => "Black & White".to_string(),
        2  => "Vivid color".to_string(),
        3  => "Solarization".to_string(),
        4  => "Adobe RGB".to_string(),
        13 => "Natural sRGB".to_string(),
        14 => "Natural+ sRGB".to_string(),
        v  => v.to_string(),
    }
}

fn mk_str(name: &str, description: &str, value: String) -> Tag {
    let pv = value.clone();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "MRW".into(),
            family1: "MinoltaRaw".into(),
            family2: "Camera".into(),
        },
        raw_value: Value::String(value),
        print_value: pv,
        priority: 0,
    }
}

fn mk_u16(name: &str, description: &str, value: u16) -> Tag {
    let pv = value.to_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "MRW".into(),
            family1: "MinoltaRaw".into(),
            family2: "Camera".into(),
        },
        raw_value: Value::U16(value),
        print_value: pv,
        priority: 0,
    }
}

fn mk_u8(name: &str, description: &str, value: u8) -> Tag {
    let pv = value.to_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "MRW".into(),
            family1: "MinoltaRaw".into(),
            family2: "Camera".into(),
        },
        raw_value: Value::U8(value),
        print_value: pv,
        priority: 0,
    }
}
