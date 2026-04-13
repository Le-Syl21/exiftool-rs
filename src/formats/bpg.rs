//! BPG (Better Portable Graphics) format reader.

use super::misc::mktag;
use crate::error::{Error, Result};
use crate::tag::Tag;
use crate::value::Value;

pub fn read_bpg(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 6 || !data.starts_with(&[0x42, 0x50, 0x47, 0xFB]) {
        return Err(Error::InvalidData("not a BPG file".into()));
    }

    let mut tags = Vec::new();

    // Bytes 4-5 are a big-endian 16-bit word containing multiple bit fields
    // Layout matches Perl BPG::Main ProcessBinaryData at offset 4, Format int16u:
    //   bits 15-13 (mask 0xe000): PixelFormat
    //   bits 12,2  (mask 0x1004): Alpha
    //   bits 11-8  (mask 0x0f00): BitDepth (value + 8)
    //   bits 7-4   (mask 0x00f0): ColorSpace
    //   bits 3,1,0 (mask 0x000b): Flags
    let word = u16::from_be_bytes([data[4], data[5]]);

    let pixel_format = (word & 0xe000) >> 13;
    let alpha_raw = word & 0x1004;
    let bit_depth = ((word & 0x0f00) >> 8) + 8;
    let flags = word & 0x000b;

    let pf_name = match pixel_format {
        0 => "Grayscale",
        1 => "4:2:0 (chroma at 0.5, 0.5)",
        2 => "4:2:2 (chroma at 0.5, 0)",
        3 => "4:4:4",
        4 => "4:2:0 (chroma at 0, 0.5)",
        5 => "4:2:2 (chroma at 0, 0)",
        _ => "Unknown",
    };
    tags.push(mktag(
        "BPG",
        "PixelFormat",
        "Pixel Format",
        Value::String(pf_name.into()),
    ));

    let alpha_name = match alpha_raw {
        0x1000 => "Alpha Exists (color not premultiplied)",
        0x1004 => "Alpha Exists (color premultiplied)",
        0x0004 => "Alpha Exists (W color component)",
        _ => "No Alpha Plane",
    };
    tags.push(mktag(
        "BPG",
        "Alpha",
        "Alpha",
        Value::String(alpha_name.into()),
    ));

    tags.push(mktag(
        "BPG",
        "BitDepth",
        "Bit Depth",
        Value::U32(bit_depth as u32),
    ));

    // Flags: bitmask (bit 0=Animation, bit 1=Limited Range, bit 3=Extension Present)
    let mut flag_parts: Vec<&str> = Vec::new();
    if flags & 0x0001 != 0 {
        flag_parts.push("Animation");
    }
    if flags & 0x0002 != 0 {
        flag_parts.push("Limited Range");
    }
    if flags & 0x0008 != 0 {
        flag_parts.push("Extension Present");
    }
    let flags_str = flag_parts.join(", ");
    tags.push(mktag("BPG", "Flags", "Flags", Value::String(flags_str)));

    // Width, height, and image length are ue7-encoded starting at offset 6
    let mut pos = 6;
    if let Some((w, consumed)) = read_bpg_ue(data, pos) {
        tags.push(mktag(
            "BPG",
            "ImageWidth",
            "Image Width",
            Value::U32(w as u32),
        ));
        pos += consumed;
        if let Some((h, consumed)) = read_bpg_ue(data, pos) {
            tags.push(mktag(
                "BPG",
                "ImageHeight",
                "Image Height",
                Value::U32(h as u32),
            ));
            pos += consumed;
            if let Some((img_len, consumed)) = read_bpg_ue(data, pos) {
                tags.push(mktag(
                    "BPG",
                    "ImageLength",
                    "Image Length",
                    Value::U32(img_len as u32),
                ));
                pos += consumed;

                // Parse extension blocks if the Extension Present flag is set
                if flags & 0x0008 != 0 {
                    if let Some((ext_size, n)) = read_bpg_ue(data, pos) {
                        pos += n;
                        let ext_end = pos + ext_size as usize;
                        if ext_end <= data.len() {
                            bpg_parse_extensions(data, pos, ext_end, &mut tags);
                        }
                    }
                }
            }
        }
    }

    Ok(tags)
}

fn bpg_parse_extensions(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>) {
    let mut pos = start;
    while pos < end {
        if pos >= data.len() {
            break;
        }
        let ext_type = data[pos];
        pos += 1;
        let (ext_len, n) = match read_bpg_ue(data, pos) {
            Some(v) => v,
            None => break,
        };
        pos += n;
        let ext_len = ext_len as usize;
        if pos + ext_len > end {
            break;
        }
        let ext_data = &data[pos..pos + ext_len];
        pos += ext_len;

        match ext_type {
            1 => {
                // EXIF: raw TIFF data (no "Exif\0\0" prefix).
                // libbpg sometimes adds an extra padding byte before the TIFF header.
                let exif_data = if ext_len > 3 {
                    let b0 = ext_data[0];
                    let b1 = ext_data[1];
                    let b2 = ext_data[2];
                    // Check for extra byte before II or MM TIFF header
                    if b0 != b'I' && b0 != b'M' && (b1 == b'I' || b1 == b'M') && b1 == b2 {
                        tags.push(mktag(
                            "ExifTool",
                            "Warning",
                            "Warning",
                            Value::String(
                                "[minor] Ignored extra byte at start of EXIF extension".into(),
                            ),
                        ));
                        &ext_data[1..]
                    } else {
                        ext_data
                    }
                } else {
                    ext_data
                };
                if let Ok(exif_tags) = crate::metadata::ExifReader::read(exif_data) {
                    tags.extend(exif_tags);
                }
            }
            2 => {
                // ICC Profile
                if let Ok(icc_tags) = crate::formats::icc::read_icc(ext_data) {
                    tags.extend(icc_tags);
                }
            }
            3 => {
                // XMP
                if let Ok(xmp_tags) = crate::metadata::XmpReader::read(ext_data) {
                    tags.extend(xmp_tags);
                }
            }
            _ => {
                // Extension types 4 (ThumbnailBPG) and 5 (AnimationControl) are binary/unknown
            }
        }
    }
}

fn read_bpg_ue(data: &[u8], mut pos: usize) -> Option<(u64, usize)> {
    // BPG uses a simple ue7 varint: MSB is continuation bit
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
