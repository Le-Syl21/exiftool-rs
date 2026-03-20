//! GIMP XCF file format reader.
//!
//! Reads XCF image properties and embedded metadata (EXIF, XMP, ICC).
//! Mirrors ExifTool's GIMP.pm.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_xcf(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 26 || !data.starts_with(b"gimp xcf ") {
        return Err(Error::InvalidData("not a GIMP XCF file".into()));
    }

    let mut tags = Vec::new();

    // Header: "gimp xcf " + version(5 bytes) + \0 + width(4) + height(4) + colormode(4)
    let version_str = String::from_utf8_lossy(&data[9..14]).trim_end_matches('\0').to_string();
    let version_num = match version_str.as_str() {
        "file\0" | "file" => "0".to_string(),
        _ => {
            // "v001" -> "1", "v013" -> "13"
            let s = version_str.trim_start_matches('v').trim_start_matches('0');
            if s.is_empty() { "0".to_string() } else { s.to_string() }
        }
    };
    tags.push(mk("XCFVersion", "XCF Version", Value::String(version_num.clone())));

    let width = u32::from_be_bytes([data[14], data[15], data[16], data[17]]);
    let height = u32::from_be_bytes([data[18], data[19], data[20], data[21]]);
    let color_mode = u32::from_be_bytes([data[22], data[23], data[24], data[25]]);

    tags.push(mk("ImageWidth", "Image Width", Value::U32(width)));
    tags.push(mk("ImageHeight", "Image Height", Value::U32(height)));

    let color_mode_str = match color_mode {
        0 => "RGB Color",
        1 => "Grayscale",
        2 => "Indexed Color",
        _ => "Unknown",
    };
    tags.push(mk("ColorMode", "Color Mode", Value::String(color_mode_str.into())));

    // Skip precision for XCF version >= 4
    let version_int: u32 = version_num.parse().unwrap_or(0);
    let mut pos = 26;
    if version_int >= 4 {
        pos += 4; // skip precision field
    }

    // Read properties
    while pos + 8 <= data.len() {
        let prop_type = u32::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
        let prop_size = u32::from_be_bytes([data[pos+4], data[pos+5], data[pos+6], data[pos+7]]) as usize;
        pos += 8;

        if prop_type == 0 {
            break; // end of properties
        }

        if pos + prop_size > data.len() {
            break;
        }

        let prop_data = &data[pos..pos + prop_size];

        match prop_type {
            17 => {
                // Compression
                if prop_size >= 1 {
                    let comp = match prop_data[0] {
                        0 => "None",
                        1 => "RLE Encoding",
                        2 => "Zlib",
                        3 => "Fractal",
                        _ => "Unknown",
                    };
                    tags.push(mk("Compression", "Compression", Value::String(comp.into())));
                }
            }
            19 => {
                // Resolution (2 floats)
                if prop_size >= 8 {
                    let x_res = f32::from_be_bytes([prop_data[0], prop_data[1], prop_data[2], prop_data[3]]);
                    let y_res = f32::from_be_bytes([prop_data[4], prop_data[5], prop_data[6], prop_data[7]]);
                    tags.push(mk("XResolution", "X Resolution", Value::String(format!("{}", x_res as u32))));
                    tags.push(mk("YResolution", "Y Resolution", Value::String(format!("{}", y_res as u32))));
                }
            }
            20 => {
                // Tattoo
                if prop_size >= 4 {
                    let tattoo = u32::from_be_bytes([prop_data[0], prop_data[1], prop_data[2], prop_data[3]]);
                    tags.push(mk("Tattoo", "Tattoo", Value::U32(tattoo)));
                }
            }
            21 => {
                // Parasites — contains embedded EXIF, XMP, ICC, etc.
                parse_parasites(prop_data, &mut tags);
            }
            22 => {
                // Units
                if prop_size >= 4 {
                    let units = u32::from_be_bytes([prop_data[0], prop_data[1], prop_data[2], prop_data[3]]);
                    let units_str = match units {
                        1 => "Inches",
                        2 => "mm",
                        3 => "Points",
                        4 => "Picas",
                        _ => "Unknown",
                    };
                    tags.push(mk("Units", "Units", Value::String(units_str.into())));
                }
            }
            _ => {
                // Skip unknown properties
            }
        }

        pos += prop_size;
    }

    Ok(tags)
}

fn parse_parasites(data: &[u8], tags: &mut Vec<Tag>) {
    let mut pos = 0;
    while pos + 4 <= data.len() {
        // Name length (4 bytes)
        let name_len = u32::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
        pos += 4;

        if pos + name_len + 8 > data.len() {
            break;
        }

        // Name string
        let name_bytes = &data[pos..pos + name_len];
        let name = String::from_utf8_lossy(name_bytes).trim_end_matches('\0').to_string();
        pos += name_len;

        // Flags (4 bytes) — skip
        pos += 4;

        // Data size (4 bytes)
        if pos + 4 > data.len() {
            break;
        }
        let data_size = u32::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
        pos += 4;

        if pos + data_size > data.len() {
            break;
        }

        let parasite_data = &data[pos..pos + data_size];

        match name.as_str() {
            "exif-data" | "jpeg-exif-data" => {
                // EXIF data starts after "Exif\0\0" header (6 bytes)
                if parasite_data.len() > 6 && parasite_data.starts_with(b"Exif\0\0") {
                    let tiff_data = &parasite_data[6..];
                    if let Ok(exif_tags) = crate::metadata::ExifReader::read(tiff_data) {
                        tags.extend(exif_tags);
                    }
                }
            }
            "icc-profile" => {
                if let Ok(icc_tags) = crate::formats::icc::read_icc(parasite_data) {
                    tags.extend(icc_tags);
                }
            }
            "gimp-metadata" => {
                // XMP data starts after "GIMP_XMP_1" header (10 bytes)
                if parasite_data.len() > 10 {
                    let xmp_data = &parasite_data[10..];
                    if let Ok(xmp_tags) = crate::metadata::XmpReader::read(xmp_data) {
                        tags.extend(xmp_tags);
                    }
                }
            }
            "iptc-data" => {
                if let Ok(iptc_tags) = crate::metadata::IptcReader::read(parasite_data) {
                    tags.extend(iptc_tags);
                }
            }
            "gimp-comment" => {
                let comment = String::from_utf8_lossy(parasite_data).trim_end_matches('\0').to_string();
                if !comment.is_empty() {
                    tags.push(mk("Comment", "Comment", Value::String(comment)));
                }
            }
            _ => {
                // Skip unknown parasites
            }
        }

        pos += data_size;
    }
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "GIMP".into(),
            family1: "GIMP".into(),
            family2: "Image".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}
