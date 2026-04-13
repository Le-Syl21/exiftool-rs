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
    0x4A, 0x46, 0x49, 0x46, 0x00, 0x11, 0x00, 0x10, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
];

/// UUID for XMP in JP2 containers
const UUID_XMP: [u8; 16] = [
    0xBE, 0x7A, 0xCF, 0xCB, 0x97, 0xA9, 0x42, 0xE8, 0x9C, 0x71, 0x99, 0x94, 0x91, 0xE3, 0xAF, 0xAC,
];

/// Parse J2C codestream (raw JPEG 2000 codestream, no box wrapper).
/// Mirrors how ExifTool uses ProcessJPEG with j2cMarker table for .j2c files.
pub fn read_j2c(data: &[u8]) -> Result<Vec<Tag>> {
    // Must start with FF 4F (SOC) FF 51 (SIZ)
    if data.len() < 4 || data[0] != 0xFF || data[1] != 0x4F {
        return Err(Error::InvalidData("not a J2C codestream".into()));
    }

    let mut tags = Vec::new();
    let mut pos = 2; // skip SOC marker (FF 4F has no length)
    let mut got_size = false;

    while pos + 4 <= data.len() {
        if data[pos] != 0xFF {
            break;
        }
        let marker = data[pos + 1];
        // Markers with no length: SOC (4F), SOD (93), EPH (92)
        if marker == 0x4F || marker == 0x93 || marker == 0x92 {
            pos += 2;
            continue;
        }
        if pos + 4 > data.len() {
            break;
        }
        let seg_len = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
        if seg_len < 2 || pos + 2 + seg_len > data.len() {
            break;
        }
        let seg_data = &data[pos + 4..pos + 2 + seg_len];

        match marker {
            0x51 => {
                // SIZ: Rsiz(2) Xsiz(4) Ysiz(4) ...
                // Perl: unpack('x2N2') => (Xsiz, Ysiz) => (width, height)
                if seg_data.len() >= 10 && !got_size {
                    let w =
                        u32::from_be_bytes([seg_data[2], seg_data[3], seg_data[4], seg_data[5]]);
                    let h =
                        u32::from_be_bytes([seg_data[6], seg_data[7], seg_data[8], seg_data[9]]);
                    got_size = true;
                    tags.push(mk("ImageWidth", "Image Width", Value::U32(w)));
                    tags.push(mk("ImageHeight", "Image Height", Value::U32(h)));
                }
            }
            0x64 => {
                // CME: comment and extension
                if seg_data.len() >= 2 {
                    let _reg = u16::from_be_bytes([seg_data[0], seg_data[1]]);
                    let val = &seg_data[2..];
                    if !val.is_empty() {
                        let comment = crate::encoding::decode_utf8_or_latin1(val);
                        tags.push(mk("Comment", "Comment", Value::String(comment)));
                    }
                }
            }
            _ => {}
        }

        pos += 2 + seg_len;
    }

    Ok(tags)
}

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
        // Parse JXL codestream header for image dimensions
        parse_jxl_codestream(data, &mut tags);
        return Ok(tags);
    }

    // JXL container (ISOBMFF boxes)
    if data.len() >= 12 && data.starts_with(&[0x00, 0x00, 0x00, 0x0C, 0x4A, 0x58, 0x4C, 0x20]) {
        parse_boxes(data, 12, data.len(), &mut tags, 0)?;
        return Ok(tags);
    }

    Err(Error::InvalidData("not a JXL file".into()))
}

fn parse_boxes(
    data: &[u8],
    start: usize,
    end: usize,
    tags: &mut Vec<Tag>,
    depth: u32,
) -> Result<()> {
    if depth > 10 {
        return Ok(());
    }

    let mut pos = start;

    while pos + 8 <= end {
        let box_size =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as u64;
        let box_type = &data[pos + 4..pos + 8];

        let (header_size, actual_size) = if box_size == 1 && pos + 16 <= end {
            let ext_size = u64::from_be_bytes([
                data[pos + 8],
                data[pos + 9],
                data[pos + 10],
                data[pos + 11],
                data[pos + 12],
                data[pos + 13],
                data[pos + 14],
                data[pos + 15],
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
                    let bpc_raw = cd[10];
                    let compression_raw = cd[11];

                    tags.push(mk("ImageHeight", "Image Height", Value::U32(height)));
                    tags.push(mk("ImageWidth", "Image Width", Value::U32(width)));
                    tags.push(mk(
                        "NumberOfComponents",
                        "Number of Components",
                        Value::U16(num_components),
                    ));

                    // BitsPerComponent: bit7 = signed, bits 0-6 = depth-1
                    let bpc_str = if bpc_raw == 0xff {
                        "Variable".to_string()
                    } else {
                        let sign = if (bpc_raw & 0x80) != 0 {
                            "Signed"
                        } else {
                            "Unsigned"
                        };
                        let depth = (bpc_raw & 0x7f) + 1;
                        format!("{} Bits, {}", depth, sign)
                    };
                    tags.push(mk(
                        "BitsPerComponent",
                        "Bits Per Component",
                        Value::String(bpc_str),
                    ));

                    // Compression
                    let comp_str = match compression_raw {
                        0 => "Uncompressed".to_string(),
                        1 => "Modified Huffman".to_string(),
                        2 => "Modified READ".to_string(),
                        3 => "Modified Modified READ".to_string(),
                        4 => "JBIG".to_string(),
                        5 => "JPEG".to_string(),
                        6 => "JPEG-LS".to_string(),
                        7 => "JPEG 2000".to_string(),
                        8 => "JBIG2".to_string(),
                        _ => format!("{}", compression_raw),
                    };
                    tags.push(mk("Compression", "Compression", Value::String(comp_str)));
                }
            }
            // Color Specification box
            b"colr" => {
                if content_end - content_start >= 3 {
                    let cd = &data[content_start..content_end];
                    let method = cd[0];
                    let precedence = cd[1] as i8;
                    let approximation = cd[2];

                    // ColorSpecMethod
                    let method_str = match method {
                        1 => "Enumerated".to_string(),
                        2 => "Restricted ICC".to_string(),
                        3 => "Any ICC".to_string(),
                        4 => "Vendor Color".to_string(),
                        _ => format!("{}", method),
                    };
                    tags.push(mk(
                        "ColorSpecMethod",
                        "Color Spec Method",
                        Value::String(method_str),
                    ));
                    tags.push(mk(
                        "ColorSpecPrecedence",
                        "Color Spec Precedence",
                        Value::String(format!("{}", precedence)),
                    ));

                    // ColorSpecApproximation
                    let approx_str = match approximation {
                        0 => "Not Specified".to_string(),
                        1 => "Accurate".to_string(),
                        2 => "Exceptional Quality".to_string(),
                        3 => "Reasonable Quality".to_string(),
                        4 => "Poor Quality".to_string(),
                        _ => format!("{}", approximation),
                    };
                    tags.push(mk(
                        "ColorSpecApproximation",
                        "Color Spec Approximation",
                        Value::String(approx_str),
                    ));

                    if method == 1 && content_end - content_start >= 7 {
                        let enum_cs = u32::from_be_bytes([cd[3], cd[4], cd[5], cd[6]]);
                        let cs_name = match enum_cs {
                            16 => "sRGB",
                            17 => "Grayscale",
                            18 => "sYCC",
                            _ => "Unknown",
                        };
                        tags.push(mk(
                            "ColorSpace",
                            "Color Space",
                            Value::String(cs_name.into()),
                        ));
                    } else if method == 2 || method == 3 {
                        // ICC profile follows at offset 3
                        if content_end - content_start > 3 {
                            let icc_data = &data[content_start + 3..content_end];
                            if let Ok(icc_tags) = crate::formats::icc::read_icc(icc_data) {
                                tags.extend(icc_tags);
                            }
                        }
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
                        tags.push(mk(
                            "YResolution",
                            "Y Resolution",
                            Value::String(format!("{:.0}", vres)),
                        ));
                    }
                    if hr_d > 0 {
                        let hres = (hr_n as f64 / hr_d as f64) * 10f64.powi(hr_e as i32);
                        tags.push(mk(
                            "XResolution",
                            "X Resolution",
                            Value::String(format!("{:.0}", hres)),
                        ));
                    }
                }
            }
            // UUID box (EXIF, XMP, IPTC, GeoJP2)
            b"uuid" => {
                if content_end - content_start > 16 {
                    let uuid = &data[content_start..content_start + 16];
                    let payload = &data[content_start + 16..content_end];

                    if uuid == UUID_XMP {
                        // XMP by UUID
                        if let Ok(xmp_tags) = XmpReader::read(payload) {
                            tags.extend(xmp_tags);
                        }
                    } else if uuid == b"JpgTiffExif->JP2" {
                        // EXIF: UUID is literally "JpgTiffExif->JP2", payload is TIFF
                        if let Ok(exif_tags) = ExifReader::read(payload) {
                            tags.extend(exif_tags);
                        }
                    } else if uuid == UUID_EXIF {
                        // Alternative EXIF UUID (from our constant)
                        if let Ok(exif_tags) = ExifReader::read(payload) {
                            tags.extend(exif_tags);
                        }
                    } else {
                        // GeoJP2: UUID b14bf8bd-083d-4b43-a5ae-8cd7d5a6ce03
                        const UUID_GEOJP2: [u8; 16] = [
                            0xb1, 0x4b, 0xf8, 0xbd, 0x08, 0x3d, 0x4b, 0x43, 0xa5, 0xae, 0x8c, 0xd7,
                            0xd5, 0xa6, 0xce, 0x03,
                        ];
                        if uuid == UUID_GEOJP2 {
                            // GeoTIFF data: TIFF file
                            if let Ok(geo_tags) = ExifReader::read(payload) {
                                tags.extend(geo_tags);
                            }
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
            // ISOBMFF File Type box
            b"ftyp" => {
                let cd = &data[content_start..content_end];
                if cd.len() >= 4 {
                    let major_brand = &cd[0..4];
                    let brand_str = crate::encoding::decode_utf8_or_latin1(major_brand);
                    let brand_desc = match major_brand {
                        b"jp2 " => "JPEG 2000 Image (.JP2)",
                        b"jpm " => "JPEG 2000 Compound Image (.JPM)",
                        b"jpx " => "JPEG 2000 with extensions (.JPX)",
                        b"jxl " => "JPEG XL Image (.JXL)",
                        b"jph " => "High-throughput JPEG 2000 (.JPH)",
                        _ => "",
                    };
                    let brand_display = if brand_desc.is_empty() {
                        brand_str.trim().to_string()
                    } else {
                        brand_desc.to_string()
                    };
                    tags.push(mk(
                        "MajorBrand",
                        "Major Brand",
                        Value::String(brand_display),
                    ));
                }
                if cd.len() >= 8 {
                    let mv = &cd[4..8];
                    let minor = format!(
                        "{:x}.{:x}.{:x}",
                        u16::from_be_bytes([mv[0], mv[1]]),
                        mv[2],
                        mv[3]
                    );
                    tags.push(mk("MinorVersion", "Minor Version", Value::String(minor)));
                }
                if cd.len() >= 12 {
                    // CompatibleBrands: 4-char groups starting at byte 8
                    let compat_data = &cd[8..];
                    let mut brands: Vec<String> = Vec::new();
                    for chunk in compat_data.chunks(4) {
                        if chunk.len() == 4 && !chunk.contains(&0u8) {
                            let b = crate::encoding::decode_utf8_or_latin1(chunk).to_string();
                            brands.push(b);
                        }
                    }
                    if !brands.is_empty() {
                        tags.push(mk(
                            "CompatibleBrands",
                            "Compatible Brands",
                            Value::String(brands.join(", ")),
                        ));
                    }
                }
            }
            // Brotli-encoded metadata box (JXL)
            b"brob" => {
                let cd = &data[content_start..content_end];
                if cd.len() >= 4 {
                    let inner_type = &cd[0..4];
                    let brotli_data = &cd[4..];
                    // Decompress Brotli
                    use std::io::Cursor;
                    let mut input = Cursor::new(brotli_data);
                    let mut output: Vec<u8> = Vec::new();
                    let decomp_ok = brotli::BrotliDecompress(&mut input, &mut output).is_ok();
                    if decomp_ok && !output.is_empty() {
                        match inner_type {
                            b"Exif" | b"exif" => {
                                // Skip 4-byte offset if present (like the Exif box)
                                let exif_payload = if output.len() > 4 {
                                    &output[4..]
                                } else {
                                    &output[..]
                                };
                                if let Ok(exif_tags) = ExifReader::read(exif_payload) {
                                    tags.extend(exif_tags);
                                }
                            }
                            b"xml " | b"XML " => {
                                if let Ok(xmp_tags) = XmpReader::read(&output) {
                                    tags.extend(xmp_tags);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }

        pos = content_end;
    }

    Ok(())
}

/// Parse JXL codestream header to extract ImageWidth and ImageHeight.
/// Mirrors Perl's ProcessJXLCodestream.
/// data: codestream starting at FF 0A
fn parse_jxl_codestream(data: &[u8], tags: &mut Vec<Tag>) {
    // unpack 'x2C12' — skip 2 bytes (FF 0A), then read 12 bytes
    if data.len() < 14 {
        return;
    }
    // Build mutable array @a of 12 bytes starting at offset 2
    let mut a: [u8; 12] = [0u8; 12];
    let start = if data.len() >= 2 && data[0] == 0xFF && data[1] == 0x0A {
        2
    } else {
        0
    };
    let src = &data[start..];
    let len = src.len().min(12);
    a[..len].copy_from_slice(&src[..len]);

    // GetBits: reads n bits LSB-first from the shared array
    let mut bits_state = a;

    let get_bits = |state: &mut [u8; 12], n: u32| -> u32 {
        let mut v: u32 = 0;
        let mut bit: u32 = 1;
        for _ in 0..n {
            for i in 0..12 {
                let set = state[i] & 1;
                state[i] >>= 1;
                if i > 0 {
                    if set != 0 {
                        state[i - 1] |= 0x80;
                    }
                } else {
                    if set != 0 {
                        v |= bit;
                    }
                    bit <<= 1;
                }
            }
        }
        v
    };

    let small = get_bits(&mut bits_state, 1);
    let (x, y);

    if small == 1 {
        y = (get_bits(&mut bits_state, 5) + 1) * 8;
        let ratio = get_bits(&mut bits_state, 3);
        if ratio == 0 {
            x = (get_bits(&mut bits_state, 5) + 1) * 8;
        } else {
            let (num, den) = match ratio {
                1 => (12u32, 10u32),
                2 => (4, 3),
                3 => (3, 2),
                4 => (16, 9),
                5 => (5, 4),
                6 => (2, 1),
                _ => (1, 1),
            };
            x = y * num / den;
        }
    } else {
        // Non-small: read 2-bit size selector to determine number of bits for dimension
        let size_bits = [9u32, 13, 18, 30];
        let sel = get_bits(&mut bits_state, 2) as usize;
        let nbits_y = size_bits[sel.min(3)];
        y = get_bits(&mut bits_state, nbits_y) + 1;

        let ratio = get_bits(&mut bits_state, 3);
        if ratio == 0 {
            let sel2 = get_bits(&mut bits_state, 2) as usize;
            let nbits_x = size_bits[sel2.min(3)];
            x = get_bits(&mut bits_state, nbits_x) + 1;
        } else {
            let (num, den) = match ratio {
                1 => (12u32, 10u32),
                2 => (4, 3),
                3 => (3, 2),
                4 => (16, 9),
                5 => (5, 4),
                6 => (2, 1),
                _ => (1, 1),
            };
            x = y * num / den;
        }
    };

    tags.push(mk("ImageWidth", "Image Width", Value::U32(x)));
    tags.push(mk("ImageHeight", "Image Height", Value::U32(y)));
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
