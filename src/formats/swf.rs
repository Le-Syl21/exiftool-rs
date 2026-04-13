//! SWF (Shockwave Flash) format reader.

use super::misc::mktag;
use crate::error::{Error, Result};
use crate::tag::Tag;
use crate::value::Value;

pub fn read_swf(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 8 {
        return Err(Error::InvalidData("not a SWF file".into()));
    }

    let compressed = match data[0] {
        b'F' => false,
        b'C' => true, // zlib compressed
        b'Z' => true, // LZMA compressed
        _ => return Err(Error::InvalidData("not a SWF file".into())),
    };

    if data[1] != b'W' || data[2] != b'S' {
        return Err(Error::InvalidData("not a SWF file".into()));
    }

    let mut tags = Vec::new();
    let version = data[3];
    let _file_length = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    tags.push(mktag(
        "SWF",
        "FlashVersion",
        "Flash Version",
        Value::U8(version),
    ));
    tags.push(mktag(
        "SWF",
        "Compressed",
        "Compressed",
        Value::String(if compressed { "True" } else { "False" }.into()),
    ));

    // Parse SWF body (starting at byte 8)
    // For uncompressed SWF: body starts at data[8]
    // For compressed: would need to decompress; we skip compression for now
    if !compressed && data.len() > 8 {
        parse_swf_body(&data[8..], &mut tags);
    }

    Ok(tags)
}

/// Parse the uncompressed SWF body starting after the 8-byte file header.
/// The body starts with a RECT structure (image dimensions), followed by
/// FrameRate (u16 LE, fixed 8.8) and FrameCount (u16 LE), then SWF tags.
fn parse_swf_body(body: &[u8], tags: &mut Vec<Tag>) {
    if body.is_empty() {
        return;
    }

    // RECT structure: first 5 bits = nBits (number of bits for each coordinate)
    // Then 4 values each nBits long: Xmin, Xmax, Ymin, Ymax (in twips, 1/20 pixel)
    let n_bits = (body[0] >> 3) as usize;
    let total_bits = 5 + n_bits * 4;
    let n_bytes = (total_bits + 7) / 8;

    if body.len() < n_bytes + 4 {
        return;
    }

    // Extract the bit-packed values
    // Read bit string
    let mut bit_str = 0u64;
    let bytes_to_read = n_bytes.min(8);
    for item in body.iter().take(bytes_to_read) {
        bit_str = (bit_str << 8) | *item as u64;
    }
    // Shift to align: the first 5 bits are nBits, then we have 4 * nBits values
    let total_64 = bytes_to_read * 8;
    let shift = total_64.saturating_sub(total_bits);
    bit_str >>= shift;

    // Extract values (from LSB side after the shift)
    let mask = if n_bits >= 64 {
        u64::MAX
    } else {
        (1u64 << n_bits) - 1
    };
    let ymax_raw = (bit_str & mask) as i32;
    let ymin_raw = ((bit_str >> n_bits) & mask) as i32;
    let xmax_raw = ((bit_str >> (n_bits * 2)) & mask) as i32;
    let xmin_raw = ((bit_str >> (n_bits * 3)) & mask) as i32;

    // Sign-extend if the high bit is set
    let sign_extend = |v: i32, bits: usize| -> i32 {
        if bits > 0 && bits < 32 && (v & (1 << (bits - 1))) != 0 {
            v | (!0i32 << bits)
        } else {
            v
        }
    };
    let xmin = sign_extend(xmin_raw, n_bits);
    let xmax = sign_extend(xmax_raw, n_bits);
    let ymin = sign_extend(ymin_raw, n_bits);
    let ymax = sign_extend(ymax_raw, n_bits);

    // Convert from twips to pixels (1/20 pixel per twip)
    let width = ((xmax - xmin) as f64) / 20.0;
    let height = ((ymax - ymin) as f64) / 20.0;

    if width >= 0.0 && height >= 0.0 {
        tags.push(mktag("SWF", "ImageWidth", "Image Width", Value::F64(width)));
        tags.push(mktag(
            "SWF",
            "ImageHeight",
            "Image Height",
            Value::F64(height),
        ));
    }

    // Frame rate (fixed point 8.8 little-endian) and frame count
    let fr_offset = n_bytes;
    if fr_offset + 4 > body.len() {
        return;
    }
    let frame_rate_raw = u16::from_le_bytes([body[fr_offset], body[fr_offset + 1]]);
    let frame_count = u16::from_le_bytes([body[fr_offset + 2], body[fr_offset + 3]]);
    let frame_rate = frame_rate_raw as f64 / 256.0;

    tags.push(mktag(
        "SWF",
        "FrameRate",
        "Frame Rate",
        Value::F64(frame_rate),
    ));
    tags.push(mktag(
        "SWF",
        "FrameCount",
        "Frame Count",
        Value::U16(frame_count),
    ));

    if frame_rate > 0.0 && frame_count > 0 {
        let duration = frame_count as f64 / frame_rate;
        tags.push(mktag(
            "SWF",
            "Duration",
            "Duration",
            Value::String(format!("{:.2} s", duration)),
        ));
    }

    // Scan SWF tags for metadata (tag 77 = Metadata/XMP)
    let mut tag_pos = fr_offset + 4;
    let mut found_attributes = false;
    while tag_pos + 2 <= body.len() {
        let code = u16::from_le_bytes([body[tag_pos], body[tag_pos + 1]]);
        let tag_type = code >> 6;
        let short_len = (code & 0x3F) as usize;
        tag_pos += 2;

        let tag_len = if short_len == 0x3F {
            if tag_pos + 4 > body.len() {
                break;
            }
            let l = u32::from_le_bytes([
                body[tag_pos],
                body[tag_pos + 1],
                body[tag_pos + 2],
                body[tag_pos + 3],
            ]) as usize;
            tag_pos += 4;
            l
        } else {
            short_len
        };

        if tag_pos + tag_len > body.len() {
            break;
        }

        match tag_type {
            69 => {
                // FileAttributes - check HasMetadata flag
                if tag_len >= 1 {
                    let flags = body[tag_pos];
                    found_attributes = true;
                    if flags & 0x10 == 0 {
                        break;
                    } // No metadata
                }
            }
            77 => {
                // Metadata tag (XMP)
                let xmp_data = &body[tag_pos..tag_pos + tag_len];
                // Parse XMP to extract Author and other tags
                if let Ok(xmp_tags) = crate::metadata::XmpReader::read(xmp_data) {
                    for t in xmp_tags {
                        // Only add tags not already present
                        if !tags.iter().any(|e| e.name == t.name) {
                            tags.push(t);
                        }
                    }
                }
                // Also store raw XMP
                tags.push(mktag(
                    "SWF",
                    "XMPToolkit",
                    "XMP Toolkit",
                    Value::String(extract_xmp_toolkit(xmp_data)),
                ));
                break;
            }
            _ => {}
        }

        tag_pos += tag_len;
    }
    let _ = found_attributes;
}

fn extract_xmp_toolkit(xmp: &[u8]) -> String {
    let text = crate::encoding::decode_utf8_or_latin1(xmp);
    // Look for xmp:CreatorTool or xmptk attribute
    if let Some(start) = text.find("xmptk=\"") {
        let after = &text[start + 7..];
        if let Some(end) = after.find('"') {
            return after[..end].to_string();
        }
    }
    if let Some(start) = text.find("<xmp:CreatorTool>") {
        let after = &text[start + 17..];
        if let Some(end) = after.find("</") {
            return after[..end].to_string();
        }
    }
    String::new()
}
