//! JPEG 2000 (JP2/JPX/J2C) and JPEG XL (JXL) box-based format reader.
//!
//! Parses JP2 boxes to extract image header, color spec, and embedded EXIF/XMP/IPTC.
//! Mirrors ExifTool's Jpeg2000.pm.

use crate::error::{Error, Result};
use crate::metadata::{ExifReader, XmpReader};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

/// UUID for EXIF in JP2 containers
const UUID_EXIF: [u8; 16] = [
    0x4A, 0x46, 0x49, 0x46, 0x00, 0x11, 0x00, 0x10,
    0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
];

/// UUID for XMP in JP2 containers
const UUID_XMP: [u8; 16] = [
    0xBE, 0x7A, 0xCF, 0xCB, 0x97, 0xA9, 0x42, 0xE8,
    0x9C, 0x71, 0x99, 0x94, 0x91, 0xE3, 0xAF, 0xAC,
];

pub fn read_jp2(data: &[u8]) -> Result<Vec<Tag>> {
    // JP2 signature box: 0000000C 6A502020 0D0A870A
    if data.len() < 12 {
        return Err(Error::InvalidData("file too small for JP2".into()));
    }

    let mut tags = Vec::new();
    let mut pos = 0;

    // Check for JP2 signature
    if data.starts_with(&[0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20]) {
        pos = 12; // Skip signature box
    }

    parse_boxes(data, pos, data.len(), &mut tags, 0)?;
    Ok(tags)
}

pub fn read_jxl(data: &[u8]) -> Result<Vec<Tag>> {
    let mut tags = Vec::new();

    // JXL bare codestream: FF 0A
    if data.len() >= 2 && data[0] == 0xFF && data[1] == 0x0A {
        tags.push(mk("JXLFormat", "JXL Format", Value::String("Codestream".into())));
        // Parse JXL codestream header for basic image info
        if data.len() >= 10 {
            parse_jxl_codestream(&data[2..], &mut tags);
        }
        return Ok(tags);
    }

    // JXL container (ISOBMFF boxes)
    if data.len() >= 12 && data.starts_with(&[0x00, 0x00, 0x00, 0x0C, 0x4A, 0x58, 0x4C, 0x20]) {
        tags.push(mk("JXLFormat", "JXL Format", Value::String("Container".into())));
        parse_boxes(data, 12, data.len(), &mut tags, 0)?;
        return Ok(tags);
    }

    Err(Error::InvalidData("not a JXL file".into()))
}

fn parse_boxes(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>, depth: u32) -> Result<()> {
    if depth > 10 {
        return Ok(());
    }

    let mut pos = start;

    while pos + 8 <= end {
        let box_size = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as u64;
        let box_type = &data[pos + 4..pos + 8];

        let (header_size, actual_size) = if box_size == 1 && pos + 16 <= end {
            let ext_size = u64::from_be_bytes([
                data[pos + 8], data[pos + 9], data[pos + 10], data[pos + 11],
                data[pos + 12], data[pos + 13], data[pos + 14], data[pos + 15],
            ]);
            (16usize, ext_size)
        } else if box_size == 0 {
            (8usize, (end - pos) as u64)
        } else {
            (8usize, box_size)
        };

        let content_start = pos + header_size;
        let content_end = (pos as u64 + actual_size) as usize;
        if content_end > end || actual_size < header_size as u64 {
            break;
        }

        match box_type {
            // JP2 Header superbox
            b"jp2h" => {
                parse_boxes(data, content_start, content_end, tags, depth + 1)?;
            }
            // Image Header box
            b"ihdr" => {
                if content_end - content_start >= 14 {
                    let cd = &data[content_start..content_end];
                    let height = u32::from_be_bytes([cd[0], cd[1], cd[2], cd[3]]);
                    let width = u32::from_be_bytes([cd[4], cd[5], cd[6], cd[7]]);
                    let num_components = u16::from_be_bytes([cd[8], cd[9]]);
                    let bit_depth = (cd[10] & 0x7F) + 1;
                    let _compression = cd[11];

                    tags.push(mk("ImageHeight", "Image Height", Value::U32(height)));
                    tags.push(mk("ImageWidth", "Image Width", Value::U32(width)));
                    tags.push(mk("NumberOfComponents", "Number of Components", Value::U16(num_components)));
                    tags.push(mk("BitsPerComponent", "Bits Per Component", Value::U8(bit_depth)));
                }
            }
            // Color Specification box
            b"colr" => {
                if content_end - content_start >= 3 {
                    let method = data[content_start];
                    if method == 1 && content_end - content_start >= 7 {
                        let enum_cs = u32::from_be_bytes([
                            data[content_start + 3], data[content_start + 4],
                            data[content_start + 5], data[content_start + 6],
                        ]);
                        let cs_name = match enum_cs {
                            16 => "sRGB",
                            17 => "Grayscale",
                            18 => "sYCC",
                            _ => "Unknown",
                        };
                        tags.push(mk("ColorSpace", "Color Space", Value::String(cs_name.into())));
                    }
                }
            }
            // Resolution box
            b"res " => {
                parse_boxes(data, content_start, content_end, tags, depth + 1)?;
            }
            b"resc" | b"resd" => {
                if content_end - content_start >= 10 {
                    let cd = &data[content_start..content_end];
                    let vr_n = u16::from_be_bytes([cd[0], cd[1]]);
                    let vr_d = u16::from_be_bytes([cd[2], cd[3]]);
                    let hr_n = u16::from_be_bytes([cd[4], cd[5]]);
                    let hr_d = u16::from_be_bytes([cd[6], cd[7]]);
                    let vr_e = cd[8] as i8;
                    let hr_e = cd[9] as i8;

                    if vr_d > 0 {
                        let vres = (vr_n as f64 / vr_d as f64) * 10f64.powi(vr_e as i32);
                        tags.push(mk("YResolution", "Y Resolution", Value::String(format!("{:.0}", vres))));
                    }
                    if hr_d > 0 {
                        let hres = (hr_n as f64 / hr_d as f64) * 10f64.powi(hr_e as i32);
                        tags.push(mk("XResolution", "X Resolution", Value::String(format!("{:.0}", hres))));
                    }
                }
            }
            // UUID box (EXIF, XMP, IPTC)
            b"uuid" => {
                if content_end - content_start > 16 {
                    let uuid = &data[content_start..content_start + 16];
                    let payload = &data[content_start + 16..content_end];

                    if uuid == &UUID_XMP {
                        if let Ok(xmp_tags) = XmpReader::read(payload) {
                            tags.extend(xmp_tags);
                        }
                    } else if uuid == &UUID_EXIF {
                        if let Ok(exif_tags) = ExifReader::read(payload) {
                            tags.extend(exif_tags);
                        }
                    }
                }
            }
            // XML box (XMP)
            b"xml " => {
                let payload = &data[content_start..content_end];
                if let Ok(xmp_tags) = XmpReader::read(payload) {
                    tags.extend(xmp_tags);
                }
            }
            // JXL codestream
            b"jxlc" | b"jxlp" => {
                if content_end - content_start > 2 {
                    let cs_data = &data[content_start..content_end];
                    // jxlp has 4-byte sequence number prefix
                    let offset = if box_type == b"jxlp" { 4 } else { 0 };
                    if cs_data.len() > offset {
                        parse_jxl_codestream(&cs_data[offset..], tags);
                    }
                }
            }
            // Exif box (JXL)
            b"Exif" => {
                if content_end - content_start > 4 {
                    // 4-byte offset prefix
                    let exif_data = &data[content_start + 4..content_end];
                    if let Ok(exif_tags) = ExifReader::read(exif_data) {
                        tags.extend(exif_tags);
                    }
                }
            }
            _ => {}
        }

        pos = content_end;
    }

    Ok(())
}

fn parse_jxl_codestream(data: &[u8], tags: &mut Vec<Tag>) {
    // JXL codestream SizeHeader is bit-packed; simplified extraction
    if data.len() < 4 {
        return;
    }
    // First bit: small (1) or not (0)
    let bits = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    let small = bits & 1;

    if small == 1 {
        let height_div8 = ((bits >> 1) & 0x1F) + 1;
        let ratio = (bits >> 6) & 0x07;
        let height = height_div8 * 8;
        let width = match ratio {
            0 => height, // 1:1
            1 => (height * 12 + 9) / 10,
            2 => (height * 4 + 2) / 3,
            3 => (height * 3 + 1) / 2,
            4 => (height * 16 + 8) / 9,
            5 => (height * 5 + 2) / 4,
            6 => height * 2,
            _ => height,
        };
        tags.push(mk("ImageWidth", "Image Width", Value::U32(width)));
        tags.push(mk("ImageHeight", "Image Height", Value::U32(height)));
    }
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let print_value = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "JP2".into(),
            family1: "JP2".into(),
            family2: "Image".into(),
        },
        raw_value: value,
        print_value,
        priority: 0,
    }
}
