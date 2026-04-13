//! GIF file format reader.
//!
//! Parses GIF87a/GIF89a files to extract comments, XMP, animation info.
//! Mirrors ExifTool's GIF.pm.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_gif(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 13 || !data.starts_with(b"GIF8") {
        return Err(Error::InvalidData("not a GIF file".into()));
    }

    let mut tags = Vec::new();
    let version = crate::encoding::decode_utf8_or_latin1(&data[3..6]).to_string();
    tags.push(mk("GIFVersion", "GIF Version", Value::String(version)));

    // Logical Screen Descriptor (bytes 6-12)
    let width = u16::from_le_bytes([data[6], data[7]]);
    let height = u16::from_le_bytes([data[8], data[9]]);
    let packed = data[10];
    let has_gct = (packed & 0x80) != 0;
    let color_resolution = ((packed >> 4) & 0x07) + 1;
    let gct_size = if has_gct {
        3 * (1 << ((packed & 0x07) + 1))
    } else {
        0
    };
    let bg_color = data[11];
    let _aspect_ratio = data[12];

    tags.push(mk("ImageWidth", "Image Width", Value::U16(width)));
    tags.push(mk("ImageHeight", "Image Height", Value::U16(height)));
    tags.push(mk(
        "HasColorMap",
        "Has Color Map",
        Value::String(if has_gct { "Yes" } else { "No" }.into()),
    ));
    tags.push(mk(
        "ColorResolutionDepth",
        "Color Resolution Depth",
        Value::U8(color_resolution),
    ));
    tags.push(mk(
        "BitsPerPixel",
        "Bits Per Pixel",
        Value::U8((packed & 0x07) + 1),
    ));
    tags.push(mk(
        "BackgroundColor",
        "Background Color",
        Value::U8(bg_color),
    ));
    // PixelAspectRatio: 0 = square pixels (undef), otherwise (val+15)/64
    let aspect_ratio = data[12];
    if aspect_ratio != 0 {
        let par = (aspect_ratio as f64 + 15.0) / 64.0;
        tags.push(mk(
            "PixelAspectRatio",
            "Pixel Aspect Ratio",
            Value::String(format!("{:.4}", par)),
        ));
    } else {
        tags.push(mk("PixelAspectRatio", "Pixel Aspect Ratio", Value::U8(1)));
    }

    let mut pos = 13 + gct_size as usize;
    let mut frame_count: u32 = 0;
    let mut total_duration: f64 = 0.0;

    while pos < data.len() {
        match data[pos] {
            // Image Descriptor
            0x2C => {
                frame_count += 1;
                if pos + 10 > data.len() {
                    break;
                }
                let local_packed = data[pos + 9];
                let has_lct = (local_packed & 0x80) != 0;
                let lct_size = if has_lct {
                    3 * (1 << ((local_packed & 0x07) + 1))
                } else {
                    0
                };
                pos += 10 + lct_size;
                // Skip LZW minimum code size
                if pos >= data.len() {
                    break;
                }
                pos += 1;
                // Skip sub-blocks
                pos = skip_sub_blocks(data, pos);
            }
            // Extension
            0x21 => {
                if pos + 2 > data.len() {
                    break;
                }
                let label = data[pos + 1];
                pos += 2;

                match label {
                    // Comment Extension
                    0xFE => {
                        let (comment, new_pos) = read_sub_blocks(data, pos);
                        pos = new_pos;
                        if !comment.is_empty() {
                            let text = crate::encoding::decode_utf8_or_latin1(&comment).to_string();
                            // Normalize newlines to ".." to match ExifTool output format
                            let text = text.replace("\r\n", "..").replace(['\n', '\r'], "..");
                            tags.push(mk("Comment", "Comment", Value::String(text)));
                        }
                    }
                    // Graphic Control Extension
                    0xF9 => {
                        if pos + 5 <= data.len() && data[pos] == 4 {
                            let delay = u16::from_le_bytes([data[pos + 2], data[pos + 3]]);
                            total_duration += delay as f64 / 100.0;
                            let transparent_flag = (data[pos + 1] & 0x01) != 0;
                            if transparent_flag {
                                let transparent_idx = data[pos + 4];
                                tags.push(mk(
                                    "TransparentColor",
                                    "Transparent Color Index",
                                    Value::U8(transparent_idx),
                                ));
                            }
                        }
                        pos = skip_sub_blocks(data, pos);
                    }
                    // Application Extension
                    0xFF => {
                        if pos + 12 <= data.len() && data[pos] == 11 {
                            let app_id = &data[pos + 1..pos + 12];
                            pos += 12;

                            if app_id == b"NETSCAPE2.0" || app_id == b"ANIMEXTS1.0" {
                                // Animation loop count
                                if pos + 4 <= data.len() && data[pos] == 3 && data[pos + 1] == 1 {
                                    let loop_count =
                                        u16::from_le_bytes([data[pos + 2], data[pos + 3]]);
                                    tags.push(mk(
                                        "AnimationIterations",
                                        "Animation Iterations",
                                        Value::U16(if loop_count == 0 {
                                            u16::MAX
                                        } else {
                                            loop_count
                                        }),
                                    ));
                                }
                                pos = skip_sub_blocks(data, pos);
                            } else if &app_id[..8] == b"XMP Data" {
                                // XMP metadata — uses IncludeLengthBytes=2:
                                // sub-block length bytes are part of the data stream
                                // The raw XMP starts with a length byte followed by XMP content
                                // We need to read sub-blocks but include the length bytes in the stream
                                let (xmp_data, new_pos) = read_sub_blocks_include_len(data, pos);
                                pos = new_pos;
                                if !xmp_data.is_empty() {
                                    // Strip the 258-byte landing zone from the end
                                    // The landing zone ends with "\x01\x00" (last sub-block of 1 byte = 0x00)
                                    // Find the real end: last occurrence of "?xpacket end"
                                    let xmp_slice =
                                        if let Some(end_pos) = find_xpacket_end(&xmp_data) {
                                            &xmp_data[..end_pos]
                                        } else {
                                            &xmp_data
                                        };
                                    if let Ok(xmp_tags) =
                                        crate::metadata::XmpReader::read(xmp_slice)
                                    {
                                        tags.extend(xmp_tags);
                                    }
                                }
                            } else if &app_id[..8] == b"ICCRGBG1" {
                                // ICC Profile
                                let (icc_data, new_pos) = read_sub_blocks(data, pos);
                                pos = new_pos;
                                if !icc_data.is_empty() {
                                    if let Ok(icc_tags) = crate::formats::icc::read_icc(&icc_data) {
                                        tags.extend(icc_tags);
                                    }
                                }
                            } else {
                                pos = skip_sub_blocks(data, pos);
                            }
                        } else {
                            pos = skip_sub_blocks(data, pos);
                        }
                    }
                    // Plain Text Extension or unknown
                    _ => {
                        pos = skip_sub_blocks(data, pos);
                    }
                }
            }
            // Trailer
            0x3B => break,
            _ => {
                pos += 1;
            }
        }
    }

    if frame_count > 1 {
        tags.push(mk("FrameCount", "Frame Count", Value::U32(frame_count)));
    }
    if frame_count > 1 && total_duration > 0.0 {
        tags.push(mk(
            "Duration",
            "Duration",
            Value::String(format!("{:.2} s", total_duration)),
        ));
    }

    Ok(tags)
}

fn skip_sub_blocks(data: &[u8], mut pos: usize) -> usize {
    while pos < data.len() {
        let block_size = data[pos] as usize;
        pos += 1;
        if block_size == 0 {
            break;
        }
        pos += block_size;
    }
    pos
}

fn read_sub_blocks(data: &[u8], mut pos: usize) -> (Vec<u8>, usize) {
    let mut result = Vec::new();
    while pos < data.len() {
        let block_size = data[pos] as usize;
        pos += 1;
        if block_size == 0 {
            break;
        }
        if pos + block_size <= data.len() {
            result.extend_from_slice(&data[pos..pos + block_size]);
        }
        pos += block_size;
    }
    (result, pos)
}

/// Read sub-blocks and include the length bytes in the output (for XMP in GIF)
fn read_sub_blocks_include_len(data: &[u8], mut pos: usize) -> (Vec<u8>, usize) {
    let mut result = Vec::new();
    while pos < data.len() {
        let block_size = data[pos] as usize;
        result.push(data[pos]); // include length byte
        pos += 1;
        if block_size == 0 {
            break;
        }
        if pos + block_size <= data.len() {
            result.extend_from_slice(&data[pos..pos + block_size]);
        }
        pos += block_size;
    }
    (result, pos)
}

/// Find the end of the XMP xpacket (returns position after the closing "?>")
fn find_xpacket_end(data: &[u8]) -> Option<usize> {
    // Search for "?xpacket end=" from near the end
    let pattern = b"?xpacket end=";
    let start = if data.len() > 512 {
        data.len() - 512
    } else {
        0
    };
    let search_range = &data[start..];
    if let Some(rel_pos) = search_range
        .windows(pattern.len())
        .rposition(|w| w == pattern)
    {
        let abs_pos = start + rel_pos;
        // Find the closing "?>" after this position
        if let Some(end_rel) = data[abs_pos..].windows(2).position(|w| w == b"?>") {
            return Some(abs_pos + end_rel + 2);
        }
    }
    None
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let print_value = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "GIF".into(),
            family1: "GIF".into(),
            family2: "Image".into(),
        },
        raw_value: value,
        print_value,
        priority: 0,
    }
}
