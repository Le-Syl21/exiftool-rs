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
    // Extended XMP chunk accumulator: (total_size, chunks sorted by offset)
    let mut ext_xmp_chunks: Vec<(u32, Vec<u8>)> = Vec::new();
    let mut ext_xmp_total: u32 = 0;

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
            // APP0 - JFIF
            0xE0 => {
                if seg_data.len() >= 5 && seg_data.starts_with(b"JFIF\0") {
                    let major = seg_data[5] as u16;
                    let minor = if seg_data.len() > 6 { seg_data[6] as u16 } else { 0 };
                    tags.push(crate::tag::Tag {
                        id: crate::tag::TagId::Text("JFIFVersion".into()),
                        name: "JFIFVersion".into(),
                        description: "JFIF Version".into(),
                        group: crate::tag::TagGroup { family0: "JFIF".into(), family1: "JFIF".into(), family2: "Image".into() },
                        raw_value: crate::value::Value::String(format!("{}.{:02}", major, minor)),
                        print_value: format!("{}.{:02}", major, minor),
                        priority: 0,
                    });
                }
            }
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
                // XMP data (standard)
                else if seg_data.len() > XMP_HEADER.len()
                    && seg_data.starts_with(XMP_HEADER)
                {
                    let xmp_data = &seg_data[XMP_HEADER.len()..];
                    match XmpReader::read(xmp_data) {
                        Ok(xmp_tags) => tags.extend(xmp_tags),
                        Err(_) => {}
                    }
                }
                // Casio QVCI APP1 segment
                else if seg_data.starts_with(b"QVCI\0") && seg_data.len() > 0x80 {
                    let d = seg_data;
                    let mk = |name: &str, val: String| -> crate::tag::Tag {
                        crate::tag::Tag {
                            id: crate::tag::TagId::Text(name.into()),
                            name: name.into(), description: name.into(),
                            group: crate::tag::TagGroup { family0: "MakerNotes".into(), family1: "Casio".into(), family2: "Camera".into() },
                            raw_value: crate::value::Value::String(val.clone()), print_value: val, priority: 0,
                        }
                    };
                    // CasioQuality at 0x2C
                    let quality = match d[0x2C] {
                        1 => "Economy", 2 => "Normal", 3 => "Fine", 4 => "Super Fine", _ => "",
                    };
                    if !quality.is_empty() { tags.push(mk("CasioQuality", quality.into())); }
                    // DateTimeOriginal at 0x4D (20 bytes string)
                    if d.len() > 0x61 {
                        let dt = String::from_utf8_lossy(&d[0x4D..0x61]).trim_end_matches('\0').replace('.', ":").to_string();
                        if !dt.is_empty() { tags.push(mk("DateTimeOriginal", dt)); }
                    }
                    // ModelType at 0x62 (4 bytes)
                    if d.len() > 0x66 {
                        let mt = u32::from_le_bytes([d[0x62], d[0x63], d[0x64], d[0x65]]);
                        tags.push(mk("ModelType", mt.to_string()));
                    }
                    // ManufactureIndex at 0x76, ManufactureCode at 0x7A
                    if d.len() > 0x7E {
                        let mi = u32::from_le_bytes([d[0x76], d[0x77], d[0x78], d[0x79]]);
                        let mc = u32::from_le_bytes([d[0x7A], d[0x7B], d[0x7C], d[0x7D]]);
                        tags.push(mk("ManufactureIndex", mi.to_string()));
                        tags.push(mk("ManufactureCode", mc.to_string()));
                    }
                    // XResolution, YResolution, ResolutionUnit from TIFF-like structure
                    // (these may be in the EXIF already)
                }
                // Extended XMP: accumulate chunks for later assembly
                else if seg_data.len() > 75
                    && seg_data.starts_with(b"http://ns.adobe.com/xmp/extension/\0")
                {
                    let rest = &seg_data[35..];
                    if rest.len() >= 40 {
                        let total = u32::from_be_bytes([rest[32], rest[33], rest[34], rest[35]]);
                        let offset = u32::from_be_bytes([rest[36], rest[37], rest[38], rest[39]]);
                        let chunk = &rest[40..];
                        ext_xmp_total = total;
                        ext_xmp_chunks.push((offset, chunk.to_vec()));
                    }
                }
            }
            // APP2 — ICC_Profile
            0xE2 => {
                if seg_data.starts_with(b"ICC_PROFILE\0") && seg_data.len() > 14 {
                    // ICC_PROFILE header: "ICC_PROFILE\0" + chunk_num(1) + total_chunks(1) + data
                    let icc_data = &seg_data[14..];
                    let icc_tags = crate::formats::icc::parse_icc_tags(icc_data);
                    tags.extend(icc_tags);
                }
            }
            // APP14 — Adobe
            0xEE => {
                if seg_data.starts_with(b"Adobe") && seg_data.len() >= 12 {
                    let d = &seg_data[5..]; // skip "Adobe"
                    let mk = |name: &str, val: String| crate::tag::Tag {
                        id: crate::tag::TagId::Text(name.into()),
                        name: name.into(), description: name.into(),
                        group: crate::tag::TagGroup { family0: "APP14".into(), family1: "Adobe".into(), family2: "Image".into() },
                        raw_value: crate::value::Value::String(val.clone()), print_value: val, priority: 0,
                    };
                    if d.len() >= 2 {
                        tags.push(mk("DCTEncodeVersion", u16::from_be_bytes([d[0], d[1]]).to_string()));
                    }
                    if d.len() >= 4 {
                        tags.push(mk("APP14Flags0", u16::from_be_bytes([d[2], d[3]]).to_string()));
                    }
                    if d.len() >= 6 {
                        tags.push(mk("APP14Flags1", u16::from_be_bytes([d[4], d[5]]).to_string()));
                    }
                    if d.len() >= 7 {
                        let ct = match d[6] { 0 => "Unknown", 1 => "YCbCr", 2 => "YCCK", _ => "" };
                        if !ct.is_empty() { tags.push(mk("ColorTransform", ct.into())); }
                    }
                }
            }
            MARKER_APP13 => {
                // Photoshop / IPTC data + all IRBs
                if seg_data.len() > PHOTOSHOP_HEADER.len()
                    && seg_data.starts_with(PHOTOSHOP_HEADER)
                {
                    let (iptc_data, irb_tags) = extract_photoshop_irbs(
                        &seg_data[PHOTOSHOP_HEADER.len()..],
                    );
                    tags.extend(irb_tags);
                    if let Some(iptc_data) = iptc_data {
                        match IptcReader::read(&iptc_data) {
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

    // Assemble and parse Extended XMP chunks (Perl: after SOS, reassemble by offset)
    if !ext_xmp_chunks.is_empty() {
        ext_xmp_chunks.sort_by_key(|(off, _)| *off);
        let mut assembled = Vec::with_capacity(ext_xmp_total as usize);
        for (_, chunk) in &ext_xmp_chunks {
            assembled.extend_from_slice(chunk);
        }
        if let Ok(ext_tags) = XmpReader::read(&assembled) {
            tags.extend(ext_tags);
        }
    }

    Ok(tags)
}

/// Extract all Photoshop IRBs, returning IPTC data and IRB tags.
fn extract_photoshop_irbs(data: &[u8]) -> (Option<Vec<u8>>, Vec<crate::tag::Tag>) {
    let mut iptc = None;
    let mut tags = Vec::new();
    let mut pos = 0;

    while pos + 12 <= data.len() {
        if &data[pos..pos + 4] != b"8BIM" { break; }
        pos += 4;
        let resource_id = u16::from_be_bytes([data[pos], data[pos + 1]]);
        pos += 2;
        let name_len = data[pos] as usize;
        pos += 1 + name_len;
        if (name_len + 1) % 2 != 0 { pos += 1; }
        if pos + 4 > data.len() { break; }
        let data_len = u32::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
        pos += 4;
        if pos + data_len > data.len() { break; }
        let irb_data = &data[pos..pos + data_len];

        if resource_id == 0x0404 {
            iptc = Some(irb_data.to_vec());
        } else {
            // Extract known Photoshop IRB tags
            let name = photoshop_irb_name(resource_id);
            if !name.is_empty() && data_len <= 256 {
                let value = decode_photoshop_irb(resource_id, irb_data);
                if !value.is_empty() {
                    tags.push(crate::tag::Tag {
                        id: crate::tag::TagId::Numeric(resource_id),
                        name: name.to_string(),
                        description: name.to_string(),
                        group: crate::tag::TagGroup {
                            family0: "Photoshop".into(),
                            family1: "Photoshop".into(),
                            family2: "Image".into(),
                        },
                        raw_value: crate::value::Value::String(value.clone()),
                        print_value: value,
                        priority: 0,
                    });
                }
            }
        }

        pos += data_len;
        if data_len % 2 != 0 { pos += 1; }
    }

    (iptc, tags)
}

fn photoshop_irb_name(id: u16) -> &'static str {
    match id {
        0x03ED => "ResolutionInfo",
        0x03F3 => "PrintFlags",
        0x0406 => "JPEG_Quality",
        0x0408 => "GridGuidesInfo",
        0x040A => "CopyrightFlag",
        0x040B => "URL",
        0x040C => "ThumbnailImage",
        0x0414 => "DocumentSpecificIDs",
        0x0419 => "GlobalAltitude",
        0x041A => "ICC_Profile",
        0x041E => "URLList",
        0x0421 => "VersionInfo",
        0x0425 => "CaptionDigest",
        0x0426 => "PrintScale",
        0x043C => "MeasurementScale",
        0x043D => "TimelineInfo",
        0x043E => "SheetDisclosure",
        0x043F => "DisplayInfo",
        0x0440 => "OnionSkins",
        0x0BBD => "IPTCDigest",
        0x2710 => "PrintInfo2",
        _ => match id {
            0x03F3 => "PrintFlags",
            0x041B => "SpotHalftone",
            0x041D => "AlphaIdentifiers",
            0x041F => "PrintFlagsInfo",
            _ => "",
        },
    }
}

fn decode_photoshop_irb(id: u16, data: &[u8]) -> String {
    match id {
        0x040A => {
            // CopyrightFlag: 1 byte
            if !data.is_empty() {
                if data[0] == 0 { "False".into() } else { "True".into() }
            } else { String::new() }
        }
        0x0419 => {
            // GlobalAltitude: int32u BE
            if data.len() >= 4 {
                u32::from_be_bytes([data[0], data[1], data[2], data[3]]).to_string()
            } else { String::new() }
        }
        0x0406 => {
            // JPEG_Quality: structured
            if data.len() >= 4 {
                let quality = u16::from_be_bytes([data[0], data[1]]);
                let format = u16::from_be_bytes([data[2], data[3]]);
                let q_str = match quality { 1..=3 => "Low", 4..=6 => "Medium", 7..=9 => "High", 10..=12 => "Maximum", _ => "" };
                let f_str = match format { 0 => "Standard", 1 => "Optimized", 2 => "Progressive", _ => "" };
                format!("{} ({})", q_str, f_str)
            } else { String::new() }
        }
        0x0BBD => {
            // IPTCDigest: 16-byte MD5
            if data.len() >= 16 {
                data[..16].iter().map(|b| format!("{:02x}", b)).collect()
            } else { String::new() }
        }
        _ => {
            // Generic: try as string
            if data.iter().all(|&b| b >= 0x20 && b < 0x7F || b == 0) {
                String::from_utf8_lossy(data).trim_end_matches('\0').to_string()
            } else if data.len() <= 4 {
                format!("{}", u32::from_be_bytes({
                    let mut buf = [0u8; 4];
                    buf[4-data.len()..].copy_from_slice(data);
                    buf
                }))
            } else {
                String::new()
            }
        }
    }
}

/// Extract IPTC data from a Photoshop IRB (for backward compat).
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
