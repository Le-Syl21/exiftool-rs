//! BMP file format reader.
//!
//! Parses BMP headers (V3/V4/V5, OS/2) to extract image dimensions, color info.
//! Mirrors ExifTool's BMP.pm.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_bmp(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 18 || !data.starts_with(b"BM") {
        return Err(Error::InvalidData("not a BMP file".into()));
    }

    let mut tags = Vec::new();

    let _file_size = u32::from_le_bytes([data[2], data[3], data[4], data[5]]);
    let _data_offset = u32::from_le_bytes([data[10], data[11], data[12], data[13]]);
    let header_size = u32::from_le_bytes([data[14], data[15], data[16], data[17]]);

    let version = match header_size {
        12 | 16 => "OS/2 V1",
        40 => "Windows V3",
        52 => "Windows V3 (Adobe)",
        56 => "Windows V3 (Adobe+Alpha)",
        64 => "OS/2 V2",
        108 => "Windows V4",
        124 => "Windows V5",
        _ => "Unknown",
    };

    tags.push(mk("BMPVersion", "BMP Version", Value::String(version.into())));

    if header_size == 12 && data.len() >= 26 {
        // OS/2 V1: 16-bit width/height
        let width = u16::from_le_bytes([data[18], data[19]]);
        let height = u16::from_le_bytes([data[20], data[21]]);
        let planes = u16::from_le_bytes([data[22], data[23]]);
        let bit_depth = u16::from_le_bytes([data[24], data[25]]);

        tags.push(mk("ImageWidth", "Image Width", Value::U16(width)));
        tags.push(mk("ImageHeight", "Image Height", Value::U16(height)));
        tags.push(mk("Planes", "Planes", Value::U16(planes)));
        tags.push(mk("BitDepth", "Bit Depth", Value::U16(bit_depth)));
    } else if header_size >= 40 && data.len() >= 54 {
        // Windows V3+: 32-bit width/height
        let width = i32::from_le_bytes([data[18], data[19], data[20], data[21]]);
        let height = i32::from_le_bytes([data[22], data[23], data[24], data[25]]);
        let planes = u16::from_le_bytes([data[26], data[27]]);
        let bit_depth = u16::from_le_bytes([data[28], data[29]]);
        let compression = u32::from_le_bytes([data[30], data[31], data[32], data[33]]);
        let image_size = u32::from_le_bytes([data[34], data[35], data[36], data[37]]);
        let x_ppm = i32::from_le_bytes([data[38], data[39], data[40], data[41]]);
        let y_ppm = i32::from_le_bytes([data[42], data[43], data[44], data[45]]);
        let colors_used = u32::from_le_bytes([data[46], data[47], data[48], data[49]]);
        let colors_important = u32::from_le_bytes([data[50], data[51], data[52], data[53]]);

        tags.push(mk("ImageWidth", "Image Width", Value::I32(width)));
        tags.push(mk("ImageHeight", "Image Height", Value::I32(height.abs())));
        tags.push(mk("Planes", "Planes", Value::U16(planes)));
        tags.push(mk("BitDepth", "Bit Depth", Value::U16(bit_depth)));

        let compression_str = match compression {
            0 => "None",
            1 => "8-Bit RLE",
            2 => "4-Bit RLE",
            3 => "Bitfields",
            4 => "JPEG",
            5 => "PNG",
            6 => "Alphabitfields",
            _ => "Unknown",
        };
        tags.push(mk(
            "Compression",
            "Compression",
            Value::String(compression_str.into()),
        ));

        if image_size > 0 {
            tags.push(mk("ImageLength", "Image Data Size", Value::U32(image_size)));
        }

        // PixelsPerMeterX/Y (raw value in pixels per meter)
        tags.push(mk("PixelsPerMeterX", "Pixels Per Meter X", Value::I32(x_ppm)));
        tags.push(mk("PixelsPerMeterY", "Pixels Per Meter Y", Value::I32(y_ppm)));
        if colors_used > 0 {
            tags.push(mk("NumColors", "Number of Colors", Value::U32(colors_used)));
        }
        if colors_important > 0 {
            tags.push(mk(
                "NumImportantColors",
                "Important Colors",
                Value::U32(colors_important),
            ));
        }

        // V4+ color space info (offset 70)
        if header_size >= 108 && data.len() >= 14 + 108 {
            let cs_type = u32::from_le_bytes([data[70], data[71], data[72], data[73]]);
            let cs_name = match cs_type {
                0 => "Calibrated RGB",
                0x73524742 => "sRGB",           // 'sRGB'
                0x57696E20 => "Windows Color Space", // 'Win '
                0x4C494E4B => "Linked Profile",  // 'LINK'
                0x4D424544 => "Embedded Profile", // 'MBED'
                _ => "Unknown",
            };
            tags.push(mk(
                "ColorSpace",
                "Color Space",
                Value::String(cs_name.into()),
            ));

            // Rendering intent (offset 108)
            if header_size >= 112 && data.len() >= 14 + 112 {
                let intent = u32::from_le_bytes([data[122], data[123], data[124], data[125]]);
                let intent_str = match intent {
                    1 => "Business (Saturation)",
                    2 => "Relative Colorimetric",
                    4 => "Perceptual",
                    8 => "Absolute Colorimetric",
                    _ => "Unknown",
                };
                tags.push(mk(
                    "RenderingIntent",
                    "Rendering Intent",
                    Value::String(intent_str.into()),
                ));
            }
        }
    }

    Ok(tags)
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let print_value = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "BMP".into(),
            family1: "BMP".into(),
            family2: "Image".into(),
        },
        raw_value: value,
        print_value,
        priority: 0,
    }
}
