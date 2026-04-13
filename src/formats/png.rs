//! PNG file format reader.
//!
//! Parses PNG chunks to extract metadata: tEXt, iTXt, zTXt, eXIf, iCCP.
//! Mirrors ExifTool's PNG.pm.

use crate::error::{Error, Result};
use crate::metadata::ExifReader;
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

/// PNG magic signature.
const PNG_SIGNATURE: &[u8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

/// Extract all metadata tags from a PNG file.
pub fn read_png(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 8 || !data.starts_with(PNG_SIGNATURE) {
        return Err(Error::InvalidData("not a PNG file".into()));
    }

    let mut tags = Vec::new();
    let mut pos = 8; // skip signature
    let mut found_idat = false;
    let mut text_after_idat_count = 0usize;

    while pos + 12 <= data.len() {
        let chunk_len =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let chunk_type = &data[pos + 4..pos + 8];
        let chunk_data_start = pos + 8;
        let chunk_data_end = chunk_data_start + chunk_len;

        if chunk_data_end + 4 > data.len() {
            break;
        }

        let chunk_data = &data[chunk_data_start..chunk_data_end];

        match chunk_type {
            // IDAT - image data, track when we've seen it
            b"IDAT" => {
                found_idat = true;
            }
            // IHDR - Image header (always first chunk)
            b"IHDR" if chunk_len >= 13 => {
                let width = u32::from_be_bytes([
                    chunk_data[0],
                    chunk_data[1],
                    chunk_data[2],
                    chunk_data[3],
                ]);
                let height = u32::from_be_bytes([
                    chunk_data[4],
                    chunk_data[5],
                    chunk_data[6],
                    chunk_data[7],
                ]);
                let bit_depth = chunk_data[8];
                let color_type = chunk_data[9];

                tags.push(make_png_tag("ImageWidth", "Image Width", Value::U32(width)));
                tags.push(make_png_tag(
                    "ImageHeight",
                    "Image Height",
                    Value::U32(height),
                ));
                tags.push(make_png_tag("BitDepth", "Bit Depth", Value::U8(bit_depth)));
                tags.push(make_png_tag(
                    "ColorType",
                    "Color Type",
                    Value::String(
                        match color_type {
                            0 => "Grayscale",
                            2 => "RGB",
                            3 => "Palette",
                            4 => "Grayscale with Alpha",
                            6 => "RGB with Alpha",
                            _ => "Unknown",
                        }
                        .to_string(),
                    ),
                ));
                // Compression (byte 10), Filter (byte 11), Interlace (byte 12) — from Perl PNG.pm
                let compression = match chunk_data[10] {
                    0 => "Deflate/Inflate",
                    _ => "Unknown",
                };
                tags.push(make_png_tag(
                    "Compression",
                    "Compression",
                    Value::String(compression.into()),
                ));
                let filter = match chunk_data[11] {
                    0 => "Adaptive",
                    _ => "Unknown",
                };
                tags.push(make_png_tag(
                    "Filter",
                    "Filter",
                    Value::String(filter.into()),
                ));
                let interlace = match chunk_data[12] {
                    0 => "Noninterlaced",
                    1 => "Adam7 Interlace",
                    _ => "Unknown",
                };
                tags.push(make_png_tag(
                    "Interlace",
                    "Interlace",
                    Value::String(interlace.into()),
                ));
            }

            // bKGD - Background color
            b"bKGD" if !chunk_data.is_empty() => {
                let bg = if chunk_data.len() >= 6 {
                    format!(
                        "{} {} {}",
                        u16::from_be_bytes([chunk_data[0], chunk_data[1]]),
                        u16::from_be_bytes([chunk_data[2], chunk_data[3]]),
                        u16::from_be_bytes([chunk_data[4], chunk_data[5]])
                    )
                } else if chunk_data.len() >= 2 {
                    u16::from_be_bytes([chunk_data[0], chunk_data[1]]).to_string()
                } else {
                    chunk_data[0].to_string()
                };
                tags.push(make_png_tag(
                    "BackgroundColor",
                    "Background Color",
                    Value::String(bg),
                ));
            }

            // tEXt - Uncompressed text
            b"tEXt" => {
                if found_idat {
                    text_after_idat_count += 1;
                }
                if let Some(null_pos) = chunk_data.iter().position(|&b| b == 0) {
                    let key = crate::encoding::decode_latin1(&chunk_data[..null_pos]);
                    let val = crate::encoding::decode_latin1(&chunk_data[null_pos + 1..]);
                    // Check for XMP in tEXt chunk
                    if key == "XML:com.adobe.xmp" {
                        if let Ok(xmp_tags) = crate::metadata::XmpReader::read(val.as_bytes()) {
                            tags.extend(xmp_tags);
                        }
                    } else {
                        tags.push(make_png_text_tag(&key, &val));
                    }
                }
            }

            // iTXt - International text (UTF-8)
            b"iTXt" => {
                if found_idat {
                    text_after_idat_count += 1;
                }
                parse_itxt_into(chunk_data, &mut tags);
            }

            // eXIf - EXIF data (PNG 1.5+)
            b"eXIf" => {
                if let Ok(exif_tags) = ExifReader::read(chunk_data) {
                    tags.extend(exif_tags);
                }
            }

            // pHYs - Physical pixel dimensions
            b"pHYs" if chunk_len >= 9 => {
                let ppux = u32::from_be_bytes([
                    chunk_data[0],
                    chunk_data[1],
                    chunk_data[2],
                    chunk_data[3],
                ]);
                let ppuy = u32::from_be_bytes([
                    chunk_data[4],
                    chunk_data[5],
                    chunk_data[6],
                    chunk_data[7],
                ]);
                let unit = chunk_data[8];

                let unit_str = match unit {
                    1 => "meters",
                    _ => "unknown",
                };
                tags.push(make_png_tag(
                    "PixelsPerUnitX",
                    "Pixels Per Unit X",
                    Value::U32(ppux),
                ));
                tags.push(make_png_tag(
                    "PixelsPerUnitY",
                    "Pixels Per Unit Y",
                    Value::U32(ppuy),
                ));
                tags.push(make_png_tag(
                    "PixelUnits",
                    "Pixel Units",
                    Value::String(unit_str.to_string()),
                ));
            }

            // tIME - Last modification time
            b"tIME" if chunk_len >= 7 => {
                let year = u16::from_be_bytes([chunk_data[0], chunk_data[1]]);
                let month = chunk_data[2];
                let day = chunk_data[3];
                let hour = chunk_data[4];
                let minute = chunk_data[5];
                let second = chunk_data[6];
                let date_str = format!(
                    "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
                    year, month, day, hour, minute, second
                );
                tags.push(make_png_tag(
                    "ModifyDate",
                    "Modify Date",
                    Value::String(date_str),
                ));
            }

            // sRGB chunk - sRGB rendering intent (1 byte)
            b"sRGB" if chunk_len >= 1 => {
                let intent = chunk_data[0];
                let intent_name = match intent {
                    0 => "Perceptual",
                    1 => "Relative Colorimetric",
                    2 => "Saturation",
                    3 => "Absolute Colorimetric",
                    _ => "Unknown",
                };
                tags.push(make_png_tag(
                    "SRGBRendering",
                    "sRGB Rendering",
                    Value::String(intent_name.to_string()),
                ));
            }

            // IEND - End of image
            b"IEND" => break,

            _ => {}
        }

        // Move past chunk data + 4-byte CRC
        pos = chunk_data_end + 4;
    }

    // Generate warning if text/EXIF chunks appeared after IDAT
    if text_after_idat_count > 0 {
        let warn_msg = if text_after_idat_count > 1 {
            format!("[minor] Text/EXIF chunk(s) found after PNG IDAT (may be ignored by some readers) [x{}]", text_after_idat_count)
        } else {
            "[minor] Text/EXIF chunk(s) found after PNG IDAT (may be ignored by some readers)"
                .to_string()
        };
        tags.push(Tag {
            id: TagId::Text("Warning".into()),
            name: "Warning".into(),
            description: "Warning".into(),
            group: TagGroup {
                family0: "ExifTool".into(),
                family1: "ExifTool".into(),
                family2: "Other".into(),
            },
            raw_value: Value::String(warn_msg.clone()),
            print_value: warn_msg,
            priority: 0,
        });
    }

    Ok(tags)
}

fn make_png_tag(name: &str, description: &str, value: Value) -> Tag {
    let print_value = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "PNG".to_string(),
            family1: "PNG".to_string(),
            family2: "Image".to_string(),
        },
        raw_value: value,
        print_value,
        priority: 0,
    }
}

fn make_png_text_tag(key: &str, value: &str) -> Tag {
    // Map standard PNG tEXt keys to Perl ExifTool tag names (from Perl PNG.pm TextualData)
    let mapped_name = match key.to_lowercase().as_str() {
        "comment" => "Comment",
        "author" => "Author",
        "copyright" => "Copyright",
        "creation time" => "CreationTime",
        "description" => "Description",
        "disclaimer" => "Disclaimer",
        "software" => "Creator",
        "source" => "Source",
        "title" => "Title",
        "warning" => "Warning",
        _ => key,
    };
    Tag {
        id: TagId::Text(mapped_name.to_string()),
        name: mapped_name.to_string(),
        description: mapped_name.to_string(),
        group: TagGroup {
            family0: "PNG".to_string(),
            family1: "PNG-tEXt".to_string(),
            family2: "Image".to_string(),
        },
        raw_value: Value::String(value.to_string()),
        print_value: value.to_string(),
        priority: 0,
    }
}

fn parse_itxt_into(data: &[u8], tags: &mut Vec<Tag>) {
    // iTXt: keyword\0 compression_flag\0 compression_method\0 language\0 translated_keyword\0 text
    let null_pos = match data.iter().position(|&b| b == 0) {
        Some(p) => p,
        None => return,
    };
    let key = crate::encoding::decode_utf8_or_latin1(&data[..null_pos]).to_string();

    let rest = &data[null_pos + 1..];
    if rest.len() < 2 {
        return;
    }

    let _compression_flag = rest[0];
    let _compression_method = rest[1];
    let rest = &rest[2..];

    // Skip language tag
    let null_pos = match rest.iter().position(|&b| b == 0) {
        Some(p) => p,
        None => return,
    };
    let rest = &rest[null_pos + 1..];

    // Skip translated keyword
    let null_pos = match rest.iter().position(|&b| b == 0) {
        Some(p) => p,
        None => return,
    };
    let text_slice = &rest[null_pos + 1..];

    let text = crate::encoding::decode_utf8_or_latin1(text_slice).to_string();

    // Check for XMP in iTXt
    if key == "XML:com.adobe.xmp" {
        if let Ok(xmp_tags) = crate::metadata::XmpReader::read(text.as_bytes()) {
            tags.extend(xmp_tags);
            return;
        }
    }

    tags.push(make_png_text_tag(&key, &text));
}
