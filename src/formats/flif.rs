//! FLIF (Free Lossless Image Format) format reader.

use super::misc::mktag;
use crate::error::{Error, Result};
use crate::tag::Tag;
use crate::value::Value;

pub fn read_flif(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 6 || !data.starts_with(b"FLIF") {
        return Err(Error::InvalidData("not a FLIF file".into()));
    }

    let mut tags = Vec::new();
    let byte4 = data[4];
    // ExifTool FLIF tag 0: type char (determines interlaced, color mode)
    let type_char = byte4 as char;
    // ExifTool tag 1: bit depth char
    let bpc_char = data[5] as char;

    // ImageType: ExifTool maps the type byte directly
    let image_type = match type_char {
        '1' => "Grayscale (non-interlaced)",
        '3' => "RGB (non-interlaced)",
        '4' => "RGBA (non-interlaced)",
        'A' => "Grayscale (interlaced)",
        'C' => "RGB (interlaced)",
        'D' => "RGBA (interlaced)",
        'Q' => "Grayscale Animation (non-interlaced)",
        'S' => "RGB Animation (non-interlaced)",
        'T' => "RGBA Animation (non-interlaced)",
        'a' => "Grayscale Animation (interlaced)",
        'c' => "RGB Animation (interlaced)",
        'd' => "RGBA Animation (interlaced)",
        _ => "Unknown",
    };
    tags.push(mktag(
        "FLIF",
        "ImageType",
        "Image Type",
        Value::String(image_type.into()),
    ));

    // BitDepth
    let bit_depth = match bpc_char {
        '0' => "Custom",
        '1' => "8",
        '2' => "16",
        _ => "Unknown",
    };
    tags.push(mktag(
        "FLIF",
        "BitDepth",
        "Bit Depth",
        Value::String(bit_depth.into()),
    ));

    // Width and height are varint encoded starting at offset 6
    let mut pos = 6;
    if let Some((w, consumed)) = read_flif_varint(data, pos) {
        let width = (w + 1) as u32;
        tags.push(mktag(
            "FLIF",
            "ImageWidth",
            "Image Width",
            Value::U32(width),
        ));
        pos += consumed;
        if let Some((h, consumed2)) = read_flif_varint(data, pos) {
            let height = (h + 1) as u32;
            tags.push(mktag(
                "FLIF",
                "ImageHeight",
                "Image Height",
                Value::U32(height),
            ));
            pos += consumed2;

            // If animation type (byte4 > 'H' = 0x48), read frame count varint
            if byte4 > 0x48 {
                if let Some((frames, consumed3)) = read_flif_varint(data, pos) {
                    let frame_count = (frames + 2) as u32;
                    tags.push(mktag(
                        "FLIF",
                        "AnimationFrames",
                        "Animation Frames",
                        Value::U32(frame_count),
                    ));
                    pos += consumed3;
                }
            }
        }
    }

    // Parse metadata chunks: each chunk has a 4-byte tag, then varint size, then compressed data
    loop {
        if pos + 4 >= data.len() {
            break;
        }
        let chunk_tag = &data[pos..pos + 4];
        let first_byte = chunk_tag[0];
        // If first byte < 32, it's the start of image data
        if first_byte < 32 {
            // Encoding tag
            let encoding = match first_byte {
                0 => "FLIF16",
                _ => "Unknown",
            };
            tags.push(mktag(
                "FLIF",
                "Encoding",
                "Encoding",
                Value::String(encoding.into()),
            ));
            break;
        }
        pos += 4;
        let chunk_tag = std::str::from_utf8(chunk_tag).unwrap_or("").to_string();

        let size = match read_flif_varint(data, pos) {
            Some((s, consumed)) => {
                pos += consumed;
                s as usize
            }
            None => break,
        };

        if pos + size > data.len() {
            break;
        }
        let chunk_data = &data[pos..pos + size];
        pos += size;

        // Try to inflate (raw deflate)
        let inflated = flif_inflate(chunk_data);
        let payload = if let Some(ref d) = inflated {
            d.as_slice()
        } else {
            chunk_data
        };

        match chunk_tag.as_str() {
            "iCCP" => {
                // ICC profile
                if let Ok(icc_tags) = crate::formats::icc::read_icc(payload) {
                    tags.extend(icc_tags);
                }
            }
            "eXif" => {
                // EXIF: skip "Exif\0\0" header if present
                let exif_data = if payload.starts_with(b"Exif\x00\x00") {
                    &payload[6..]
                } else {
                    payload
                };
                if let Ok(exif_tags) = crate::metadata::ExifReader::read(exif_data) {
                    tags.extend(exif_tags);
                }
            }
            "eXmp" => {
                // XMP
                if let Ok(xmp_tags) = crate::metadata::XmpReader::read(payload) {
                    tags.extend(xmp_tags);
                }
            }
            _ => {}
        }
    }

    Ok(tags)
}

/// Try to inflate raw deflate-compressed data (FLIF metadata chunks).
/// FLIF uses raw deflate (no zlib/gzip header).
fn flif_inflate(data: &[u8]) -> Option<Vec<u8>> {
    use std::io::Read;
    // Try raw deflate first
    {
        use flate2::read::DeflateDecoder;
        let mut decoder = DeflateDecoder::new(data);
        let mut output = Vec::new();
        if decoder.read_to_end(&mut output).is_ok() && !output.is_empty() {
            return Some(output);
        }
    }
    // Fallback: try zlib
    {
        use flate2::read::ZlibDecoder;
        let mut decoder = ZlibDecoder::new(data);
        let mut output = Vec::new();
        if decoder.read_to_end(&mut output).is_ok() && !output.is_empty() {
            return Some(output);
        }
    }
    None
}

fn read_flif_varint(data: &[u8], mut pos: usize) -> Option<(u64, usize)> {
    let start = pos;
    let mut result = 0u64;
    loop {
        if pos >= data.len() {
            return None;
        }
        let byte = data[pos];
        result = (result << 7) | (byte & 0x7F) as u64;
        pos += 1;
        if byte & 0x80 == 0 {
            break;
        }
        if pos - start > 8 {
            return None;
        }
    }
    Some((result, pos - start))
}
