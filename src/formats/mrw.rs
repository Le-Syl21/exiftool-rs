//! Minolta MRW file format reader.
//!
//! Parses MRW segments: PRD (dimensions), WBG (white balance), TTW (EXIF).
//! Mirrors ExifTool's MinoltaRaw.pm.

use crate::error::{Error, Result};
use crate::metadata::ExifReader;
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_mrw(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 8 || (data[0] != 0 || data[1] != b'M' || data[2] != b'R') {
        return Err(Error::InvalidData("not a MRW file".into()));
    }

    let mut tags = Vec::new();
    let _data_offset = u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize + 8;
    let mut pos = 8;

    while pos + 8 <= data.len() {
        let tag = &data[pos..pos + 4];
        let length = u32::from_be_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]) as usize;
        pos += 8;

        if pos + length > data.len() {
            break;
        }

        let seg_data = &data[pos..pos + length];

        match tag {
            b"\0TTW" => {
                // TIFF/EXIF data
                if let Ok(exif_tags) = ExifReader::read(seg_data) {
                    tags.extend(exif_tags);
                }
            }
            b"\0PRD" => {
                // Picture Raw Dimensions
                if length >= 16 {
                    let sensor_w = u16::from_be_bytes([seg_data[4], seg_data[5]]);
                    let sensor_h = u16::from_be_bytes([seg_data[2], seg_data[3]]);
                    let image_w = u16::from_be_bytes([seg_data[8], seg_data[9]]);
                    let image_h = u16::from_be_bytes([seg_data[6], seg_data[7]]);
                    let bit_depth = seg_data[10];

                    tags.push(mk("SensorWidth", "Sensor Width", Value::U16(sensor_w)));
                    tags.push(mk("SensorHeight", "Sensor Height", Value::U16(sensor_h)));
                    tags.push(mk("ImageWidth", "Image Width", Value::U16(image_w)));
                    tags.push(mk("ImageHeight", "Image Height", Value::U16(image_h)));
                    tags.push(mk("RawBitDepth", "Raw Bit Depth", Value::U8(bit_depth)));
                }
            }
            b"\0WBG" => {
                // White Balance Gains
                if length >= 8 {
                    let r = u16::from_be_bytes([seg_data[0], seg_data[1]]);
                    let g1 = u16::from_be_bytes([seg_data[2], seg_data[3]]);
                    let g2 = u16::from_be_bytes([seg_data[4], seg_data[5]]);
                    let b = u16::from_be_bytes([seg_data[6], seg_data[7]]);
                    tags.push(mk(
                        "WBScale",
                        "White Balance Scale",
                        Value::String(format!("R={} G1={} G2={} B={}", r, g1, g2, b)),
                    ));
                }
            }
            _ => {}
        }

        pos += length;
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
            family0: "MRW".into(),
            family1: "MRW".into(),
            family2: "Camera".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}
