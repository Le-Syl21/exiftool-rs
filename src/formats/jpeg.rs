//! JPEG file format reader.
//!
//! Parses JPEG APP segments to extract EXIF (APP1), XMP (APP1), IPTC (APP13),
//! and other metadata. Mirrors ExifTool's JPEG.pm.

use crate::error::{Error, Result};
use crate::metadata::{ExifReader, IptcReader, XmpReader};
use crate::tag::Tag;

/// JPEG marker constants.
const MARKER_SOI: u8 = 0xD8;
const MARKER_SOS: u8 = 0xDA;
const MARKER_APP1: u8 = 0xE1;
const MARKER_APP13: u8 = 0xED;
const MARKER_COM: u8 = 0xFE;

/// EXIF header in APP1: "Exif\0\0"
const EXIF_HEADER: &[u8] = b"Exif\0\0";
/// XMP header in APP1
const XMP_HEADER: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
/// Photoshop 3.0 header in APP13 (contains IPTC)
const PHOTOSHOP_HEADER: &[u8] = b"Photoshop 3.0\0";
/// IPTC resource type within Photoshop segment
const IPTC_RESOURCE_TYPE: u16 = 0x0404;

/// Extract all metadata tags from a JPEG file.
pub fn read_jpeg(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 2 || data[0] != 0xFF || data[1] != MARKER_SOI {
        return Err(Error::InvalidData("not a JPEG file".into()));
    }

    let mut tags = Vec::new();
    let mut pos = 2;

    while pos + 4 <= data.len() {
        // Find next marker
        if data[pos] != 0xFF {
            pos += 1;
            continue;
        }

        let marker = data[pos + 1];
        pos += 2;

        // Skip padding bytes (0xFF)
        if marker == 0xFF || marker == 0x00 {
            continue;
        }

        // SOS (Start of Scan) means we've reached image data - stop parsing
        if marker == MARKER_SOS {
            break;
        }

        // SOF markers (0xC0-0xCF except 0xC4 DHT and 0xCC DAC) — extract image dimensions
        if (0xC0..=0xCF).contains(&marker) && marker != 0xC4 && marker != 0xCC {
            if pos + 2 <= data.len() {
                let sof_len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
                if pos + sof_len <= data.len() && sof_len >= 8 {
                    let sof = &data[pos + 2..pos + sof_len];
                    let precision = sof[0];
                    let height = u16::from_be_bytes([sof[1], sof[2]]);
                    let width = u16::from_be_bytes([sof[3], sof[4]]);
                    let components = sof[5];

                    tags.push(crate::tag::Tag {
                        id: crate::tag::TagId::Text("ImageWidth".into()),
                        name: "ImageWidth".into(),
                        description: "Image Width".into(),
                        group: crate::tag::TagGroup { family0: "File".into(), family1: "File".into(), family2: "Image".into() },
                        raw_value: crate::value::Value::U16(width),
                        print_value: width.to_string(),
                        priority: 0,
                    });
                    tags.push(crate::tag::Tag {
                        id: crate::tag::TagId::Text("ImageHeight".into()),
                        name: "ImageHeight".into(),
                        description: "Image Height".into(),
                        group: crate::tag::TagGroup { family0: "File".into(), family1: "File".into(), family2: "Image".into() },
                        raw_value: crate::value::Value::U16(height),
                        print_value: height.to_string(),
                        priority: 0,
                    });
                    tags.push(crate::tag::Tag {
                        id: crate::tag::TagId::Text("BitsPerSample".into()),
                        name: "BitsPerSample".into(),
                        description: "Bits Per Sample".into(),
                        group: crate::tag::TagGroup { family0: "File".into(), family1: "File".into(), family2: "Image".into() },
                        raw_value: crate::value::Value::U8(precision),
                        print_value: precision.to_string(),
                        priority: 0,
                    });
                    tags.push(crate::tag::Tag {
                        id: crate::tag::TagId::Text("ColorComponents".into()),
                        name: "ColorComponents".into(),
                        description: "Color Components".into(),
                        group: crate::tag::TagGroup { family0: "File".into(), family1: "File".into(), family2: "Image".into() },
                        raw_value: crate::value::Value::U8(components),
                        print_value: components.to_string(),
                        priority: 0,
                    });

                    let enc_process = match marker {
                        0xC0 => "Baseline DCT, Huffman coding",
                        0xC1 => "Extended sequential DCT, Huffman coding",
                        0xC2 => "Progressive DCT, Huffman coding",
                        0xC3 => "Lossless, Huffman coding",
                        0xC9 => "Extended sequential DCT, arithmetic coding",
                        0xCA => "Progressive DCT, arithmetic coding",
                        0xCB => "Lossless, arithmetic coding",
                        _ => "Unknown",
                    };
                    tags.push(crate::tag::Tag {
                        id: crate::tag::TagId::Text("EncodingProcess".into()),
                        name: "EncodingProcess".into(),
                        description: "Encoding Process".into(),
                        group: crate::tag::TagGroup { family0: "File".into(), family1: "File".into(), family2: "Image".into() },
                        raw_value: crate::value::Value::U8(marker - 0xC0),
                        print_value: enc_process.to_string(),
                        priority: 0,
                    });

                    // YCbCr SubSampling (from component sampling factors)
                    if components >= 3 && sof.len() >= 6 + components as usize * 3 {
                        let h_sample = (sof[7] >> 4) & 0x0F;
                        let v_sample = sof[7] & 0x0F;
                        let subsampling = if h_sample == 2 && v_sample == 2 { "YCbCr4:2:0".to_string() }
                        else if h_sample == 2 && v_sample == 1 { "YCbCr4:2:2".to_string() }
                        else if h_sample == 1 && v_sample == 1 { "YCbCr4:4:4".to_string() }
                        else { format!("YCbCr {}:{}", h_sample, v_sample) };
                        tags.push(crate::tag::Tag {
                            id: crate::tag::TagId::Text("YCbCrSubSampling".into()),
                            name: "YCbCrSubSampling".into(),
                            description: "YCbCr Sub Sampling".into(),
                            group: crate::tag::TagGroup { family0: "File".into(), family1: "File".into(), family2: "Image".into() },
                            raw_value: crate::value::Value::String(format!("{} {}", h_sample, v_sample)),
                            print_value: subsampling,
                            priority: 0,
                        });
                    }
                }
            }
        }

        // Markers without payload
        if marker == MARKER_SOI || (0xD0..=0xD7).contains(&marker) {
            continue;
        }

        // Read segment length
        if pos + 2 > data.len() {
            break;
        }
        let seg_len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
        if seg_len < 2 || pos + seg_len > data.len() {
            break;
        }

        let seg_data = &data[pos + 2..pos + seg_len];
        pos += seg_len;

        match marker {
            MARKER_APP1 => {
                // EXIF data
                if seg_data.len() > EXIF_HEADER.len()
                    && seg_data.starts_with(EXIF_HEADER)
                {
                    let exif_data = &seg_data[EXIF_HEADER.len()..];
                    match ExifReader::read(exif_data) {
                        Ok(exif_tags) => tags.extend(exif_tags),
                        Err(_) => {} // silently skip malformed EXIF
                    }
                }
                // XMP data
                else if seg_data.len() > XMP_HEADER.len()
                    && seg_data.starts_with(XMP_HEADER)
                {
                    let xmp_data = &seg_data[XMP_HEADER.len()..];
                    match XmpReader::read(xmp_data) {
                        Ok(xmp_tags) => tags.extend(xmp_tags),
                        Err(_) => {}
                    }
                }
            }
            MARKER_APP13 => {
                // Photoshop / IPTC data
                if seg_data.len() > PHOTOSHOP_HEADER.len()
                    && seg_data.starts_with(PHOTOSHOP_HEADER)
                {
                    if let Some(iptc_data) = extract_iptc_from_photoshop(
                        &seg_data[PHOTOSHOP_HEADER.len()..],
                    ) {
                        match IptcReader::read(iptc_data) {
                            Ok(iptc_tags) => tags.extend(iptc_tags),
                            Err(_) => {}
                        }
                    }
                }
            }
            MARKER_COM => {
                // JPEG Comment
                let comment = String::from_utf8_lossy(seg_data)
                    .trim_end_matches('\0')
                    .to_string();
                if !comment.is_empty() {
                    tags.push(crate::tag::Tag {
                        id: crate::tag::TagId::Text("Comment".into()),
                        name: "Comment".into(),
                        description: "JPEG Comment".into(),
                        group: crate::tag::TagGroup {
                            family0: "File".into(),
                            family1: "Comment".into(),
                            family2: "Image".into(),
                        },
                        raw_value: crate::value::Value::String(comment.clone()),
                        print_value: comment,
                        priority: 0,
                    });
                }
            }
            _ => {
                // Skip unknown segments
            }
        }
    }

    Ok(tags)
}

/// Extract IPTC data from a Photoshop IRB (Image Resource Block) segment.
///
/// Photoshop IRB format:
///   - 4 bytes: resource type ("8BIM")
///   - 2 bytes: resource ID
///   - Pascal string: resource name (padded to even length)
///   - 4 bytes: resource data length
///   - N bytes: resource data (padded to even length)
fn extract_iptc_from_photoshop(data: &[u8]) -> Option<&[u8]> {
    let mut pos = 0;

    while pos + 12 <= data.len() {
        // Check for "8BIM" signature
        if &data[pos..pos + 4] != b"8BIM" {
            break;
        }
        pos += 4;

        // Resource ID
        let resource_id = u16::from_be_bytes([data[pos], data[pos + 1]]);
        pos += 2;

        // Pascal string (name) - first byte is length
        let name_len = data[pos] as usize;
        pos += 1;
        pos += name_len;
        // Pad to even offset
        if (name_len + 1) % 2 != 0 {
            pos += 1;
        }

        if pos + 4 > data.len() {
            break;
        }

        // Resource data length
        let data_len = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
            as usize;
        pos += 4;

        if pos + data_len > data.len() {
            break;
        }

        if resource_id == IPTC_RESOURCE_TYPE {
            return Some(&data[pos..pos + data_len]);
        }

        pos += data_len;
        // Pad to even offset
        if data_len % 2 != 0 {
            pos += 1;
        }
    }

    None
}
