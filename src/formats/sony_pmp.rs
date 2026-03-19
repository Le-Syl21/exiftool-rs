//! Sony PMP (DSC-F1) video format reader.
//!
//! Parses Sony proprietary PMP files with embedded JPEG.
//! Mirrors ExifTool's Sony.pm ProcessPMP.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_sony_pmp(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 128 {
        return Err(Error::InvalidData("file too small for PMP".into()));
    }

    // Validate: bytes 8-11 should be 0x00 0x00 0x00 0x7C (header length = 124)
    // and last 4 bytes of the 128-byte header should be FF D8 FF DB (JPEG SOI)
    if data[8] != 0x00 || data[9] != 0x00 || data[10] != 0x00 || data[11] != 0x7C {
        return Err(Error::InvalidData("invalid PMP header".into()));
    }
    if data[124] != 0xFF || data[125] != 0xD8 || data[126] != 0xFF || data[127] != 0xDB {
        return Err(Error::InvalidData("PMP missing JPEG signature".into()));
    }

    let mut tags = Vec::new();

    // Make and Model are always Sony DSC-F1
    tags.push(mk("Make", "Make", Value::String("Sony".into())));
    tags.push(mk("Model", "Model", Value::String("DSC-F1".into())));

    // JpgFromRawStart at offset 8 (int32u BE)
    let jpg_start = u32::from_be_bytes([data[8], data[9], data[10], data[11]]) as usize;
    tags.push(mk("JpgFromRawStart", "Jpg From Raw Start", Value::U32(jpg_start as u32)));

    // JpgFromRawLength at offset 12
    let jpg_len = u32::from_be_bytes([data[12], data[13], data[14], data[15]]) as usize;
    tags.push(mk("JpgFromRawLength", "Jpg From Raw Length", Value::U32(jpg_len as u32)));

    // SonyImageWidth at offset 22 (int16u BE)
    let sony_w = u16::from_be_bytes([data[22], data[23]]);
    tags.push(mk("SonyImageWidth", "Sony Image Width", Value::U16(sony_w)));

    // SonyImageHeight at offset 24
    let sony_h = u16::from_be_bytes([data[24], data[25]]);
    tags.push(mk("SonyImageHeight", "Sony Image Height", Value::U16(sony_h)));

    // Orientation at offset 27
    let orientation = data[27];
    let orient_str = match orientation {
        0 => "Horizontal (normal)",
        1 => "Rotate 270 CW",
        2 => "Rotate 180",
        3 => "Rotate 90 CW",
        _ => "Unknown",
    };
    tags.push(mk("Orientation", "Orientation", Value::String(orient_str.into())));

    // ImageQuality at offset 29
    let quality = data[29];
    let qual_str = match quality {
        8 => "Snap Shot",
        23 => "Standard",
        51 => "Fine",
        n => return {
            // Just add unknown
            tags.push(mk("ImageQuality", "Image Quality", Value::U8(n)));
            parse_rest(&data, jpg_start, jpg_len, &mut tags);
            Ok(tags)
        },
    };
    tags.push(mk("ImageQuality", "Image Quality", Value::String(qual_str.into())));

    parse_rest(data, jpg_start, jpg_len, &mut tags);

    Ok(tags)
}

fn parse_rest(data: &[u8], jpg_start: usize, jpg_len: usize, tags: &mut Vec<Tag>) {
    // Comment at offset 52 (string[19])
    if data.len() > 71 {
        let comment = read_null_str(&data[52..71]);
        if !comment.is_empty() {
            tags.push(mk("Comment", "Comment", Value::String(comment)));
        }
    }

    // DateTimeOriginal at offset 76 (6 bytes: yy mm dd hh mm ss)
    if data.len() >= 82 {
        let y = data[76] as i32 + if data[76] < 70 { 2000 } else { 1900 };
        let dt = format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
            y, data[77], data[78], data[79], data[80], data[81]);
        tags.push(mk("DateTimeOriginal", "Date/Time Original", Value::String(dt)));
    }

    // ModifyDate at offset 84
    if data.len() >= 90 {
        let y = data[84] as i32 + if data[84] < 70 { 2000 } else { 1900 };
        let dt = format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
            y, data[85], data[86], data[87], data[88], data[89]);
        tags.push(mk("ModifyDate", "Modify Date", Value::String(dt)));
    }

    // ExposureTime at offset 102 (int16s BE)
    if data.len() >= 104 {
        let et_raw = i16::from_be_bytes([data[102], data[103]]);
        if et_raw > 0 {
            let exp = 2.0f64.powf(-(et_raw as f64) / 100.0);
            let exp_str = if exp < 1.0 {
                let denom = (1.0 / exp).round() as i64;
                format!("1/{}", denom)
            } else {
                format!("{:.1}", exp)
            };
            tags.push(mk("ExposureTime", "Exposure Time", Value::String(exp_str)));
        }
    }

    // FNumber at offset 106 (int16s BE)
    if data.len() >= 108 {
        let fn_raw = i16::from_be_bytes([data[106], data[107]]);
        if fn_raw > 0 {
            let fnum = fn_raw as f64 / 100.0;
            tags.push(mk("FNumber", "F Number", Value::String(format!("{:.1}", fnum))));
        }
    }

    // Flash at offset 118
    if data.len() >= 119 {
        let flash = data[118];
        let flash_str = match flash {
            0 => "No Flash",
            1 => "Fired",
            _ => "Unknown",
        };
        tags.push(mk("Flash", "Flash", Value::String(flash_str.into())));
    }

    // Parse the embedded JPEG for additional EXIF data
    if jpg_start > 0 && jpg_start < data.len() {
        let jpg_end = (jpg_start + jpg_len).min(data.len());
        let jpg_data = &data[jpg_start..jpg_end];
        if jpg_data.len() >= 3 && jpg_data[0] == 0xFF && jpg_data[1] == 0xD8 {
            if let Ok(jpeg_tags) = crate::formats::jpeg::read_jpeg(jpg_data) {
                // Only take EXIF/IPTC tags, not file-level tags
                for tag in jpeg_tags {
                    if tag.group.family0 != "File" {
                        tags.push(tag);
                    }
                }
            }
        }
    }
}

fn read_null_str(data: &[u8]) -> String {
    let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    String::from_utf8_lossy(&data[..end]).to_string()
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "MakerNotes".into(),
            family1: "Sony".into(),
            family2: "Image".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}
