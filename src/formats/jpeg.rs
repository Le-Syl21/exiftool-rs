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
const MARKER_APP3: u8 = 0xE3;
const MARKER_APP4: u8 = 0xE4;
const MARKER_APP5: u8 = 0xE5;
const MARKER_APP7: u8 = 0xE7;
const MARKER_APP12: u8 = 0xEC;
const MARKER_APP13: u8 = 0xED;
const MARKER_COM: u8 = 0xFE;

/// CanonVRD trailer signature
const CANON_VRD_SIG: &[u8] = b"CANON OPTIONAL DATA\0";

/// EXIF header in APP1: "Exif\0\0"
const EXIF_HEADER: &[u8] = b"Exif\0\0";
/// XMP header in APP1
const XMP_HEADER: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
/// Photoshop 3.0 header in APP13 (contains IPTC)
const PHOTOSHOP_HEADER: &[u8] = b"Photoshop 3.0\0";


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
    // InfiRay flag: set when APP2 IJPEG header is detected
    let mut is_infray = false;
    // FlashPix FPXR accumulator: contents list + stream data per index
    let mut fpxr_contents: Vec<FpxrEntry> = Vec::new();
    let mut fpxr_seen = false;

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
                    let jfif_mk = |name: &str, val: String| crate::tag::Tag {
                        id: crate::tag::TagId::Text(name.into()),
                        name: name.into(), description: name.into(),
                        group: crate::tag::TagGroup { family0: "JFIF".into(), family1: "JFIF".into(), family2: "Image".into() },
                        raw_value: crate::value::Value::String(val.clone()), print_value: val, priority: 0,
                    };
                    tags.push(jfif_mk("JFIFVersion", format!("{}.{:02}", major, minor)));
                    // ResolutionUnit at byte 7
                    if seg_data.len() > 7 {
                        let unit = match seg_data[7] { 0 => "None", 1 => "inches", 2 => "cm", _ => "" };
                        if !unit.is_empty() { tags.push(jfif_mk("ResolutionUnit", unit.into())); }
                    }
                    // XResolution at bytes 8-9, YResolution at 10-11 (int16u BE)
                    if seg_data.len() > 11 {
                        let xres = u16::from_be_bytes([seg_data[8], seg_data[9]]);
                        let yres = u16::from_be_bytes([seg_data[10], seg_data[11]]);
                        tags.push(jfif_mk("XResolution", xres.to_string()));
                        tags.push(jfif_mk("YResolution", yres.to_string()));
                    }
                } else if seg_data.len() >= 5 && seg_data.starts_with(b"JFXX\0") {
                    // JFXX APP0: thumbnail extension (from Perl JFIF.pm)
                    // Format: 'JFXX\0' + extension_code(1) + thumbnail data
                    if seg_data.len() > 5 {
                        let ext_code = seg_data[5];
                        let thumb_data = &seg_data[6..];
                        if !thumb_data.is_empty() {
                            let (mime, fmt) = match ext_code {
                                0x10 => ("image/jpeg", "JPEG"),
                                0x11 => ("image/x-rgb", "RGB"),
                                0x13 => ("image/x-rgb", "RGB Palette"),
                                _ => ("image/jpeg", "JPEG"),
                            };
                            let _ = mime;
                            let _ = fmt;
                            tags.push(crate::tag::Tag {
                                id: crate::tag::TagId::Text("ThumbnailImage".into()),
                                name: "ThumbnailImage".into(),
                                description: "Thumbnail Image".into(),
                                group: crate::tag::TagGroup { family0: "JFIF".into(), family1: "JFIF".into(), family2: "Image".into() },
                                raw_value: crate::value::Value::Binary(thumb_data.to_vec()),
                                print_value: format!("(Binary data {} bytes, use -b option to extract)", thumb_data.len()),
                                priority: 0,
                            });
                        }
                    }
                } else if seg_data.len() >= 14 && {
                    // CIFF check: (II|MM) + 4 bytes + HEAPJPGM
                    (seg_data.starts_with(b"II") || seg_data.starts_with(b"MM"))
                    && seg_data.len() > 10 && &seg_data[6..10] == b"HEAP"
                } {
                    // Canon CIFF data embedded in APP0 (from Perl JPEG.pm CIFF condition)
                    if let Ok(ciff_tags) = crate::formats::canon_raw::read_crw(seg_data) {
                        tags.extend(ciff_tags);
                    }
                    // Supplementary: extract FreeBytes (tag 0x0001) which canon_raw skips
                    tags.extend(extract_ciff_freebytes(seg_data));
                } else if seg_data.starts_with(b"AVI1") && seg_data.len() > 4 {
                    // AVI1 APP0: from AVI JPEG frames (from Perl JPEG.pm/JPEG::AVI1)
                    // Data after "AVI1" (4 bytes): index 0 (int8u) = InterleavedField
                    let d = &seg_data[4..];
                    if !d.is_empty() {
                        let val = d[0];
                        let print_val = match val {
                            0 => "Not Interleaved",
                            1 => "Odd",
                            2 => "Even",
                            _ => "",
                        };
                        if !print_val.is_empty() {
                            tags.push(crate::tag::Tag {
                                id: crate::tag::TagId::Numeric(0),
                                name: "InterleavedField".into(),
                                description: "Interleaved Field".into(),
                                group: crate::tag::TagGroup { family0: "APP0".into(), family1: "AVI1".into(), family2: "Image".into() },
                                raw_value: crate::value::Value::U8(val),
                                print_value: print_val.into(),
                                priority: 0,
                            });
                        }
                    }
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
                // FLIR thermal data: "FLIR\0" + segment_num(1) + total_segments(1) + FFF data
                else if seg_data.starts_with(b"FLIR\0") && seg_data.len() > 0x48 {
                    let fff_start = 8; // "FLIR\0" + seg_num + total + padding
                    let flir_data = &seg_data[fff_start..];
                    if flir_data.starts_with(b"FFF\0") || flir_data.starts_with(b"AFF\0") {
                        tags.extend(decode_flir_fff(flir_data));
                    }
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
            // APP2 — ICC_Profile, InfiRay IJPEG, MPF, or FPXR
            0xE2 => {
                // InfiRay: "....IJPEG\0" at offset 4
                if seg_data.len() > 10 && &seg_data[4..10] == b"IJPEG\0" {
                    is_infray = true;
                    tags.extend(decode_infray_version(seg_data));
                }
                // FPXR: "FPXR\0" header (FlashPix Ready)
                else if seg_data.starts_with(b"FPXR\0") && seg_data.len() > 7 {
                    fpxr_seen = true;
                    accumulate_fpxr(seg_data, &mut fpxr_contents);
                }
                // MPF: "MPF\0" header (Multi-Picture Format)
                else if seg_data.starts_with(b"MPF\0") {
                    tags.extend(parse_mpf(seg_data, data));
                }
                else if seg_data.starts_with(b"ICC_PROFILE\0") && seg_data.len() > 14 {
                    // ICC_PROFILE header: "ICC_PROFILE\0" + chunk_num(1) + total_chunks(1) + data
                    let icc_data = &seg_data[14..];
                    let icc_tags = crate::formats::icc::parse_icc_tags(icc_data);
                    tags.extend(icc_tags);
                }
            }
            // APP3 — Kodak Meta IFD or InfiRay ImagingData
            MARKER_APP3 => {
                if is_infray && !seg_data.is_empty() {
                    tags.push(crate::tag::Tag {
                        id: crate::tag::TagId::Text("ImagingData".into()),
                        name: "ImagingData".into(), description: "Imaging Data".into(),
                        group: crate::tag::TagGroup { family0: "APP3".into(), family1: "InfiRay".into(), family2: "Image".into() },
                        raw_value: crate::value::Value::Binary(seg_data.to_vec()),
                        print_value: format!("(Binary data {} bytes, use -b option to extract)", seg_data.len()),
                        priority: 0,
                    });
                }
                // JPS (JPEG Stereo): starts with "_JPSJPS_"
                else if seg_data.starts_with(b"_JPSJPS_") && seg_data.len() >= 14 {
                    tags.extend(parse_jps(seg_data));
                }
                // Kodak Meta IFD: starts with "Meta\0\0", "META\0\0", or "Exif\0\0"
                else if seg_data.len() > 8
                    && (seg_data.starts_with(b"Meta\0\0")
                        || seg_data.starts_with(b"META\0\0")
                        || seg_data.starts_with(b"Exif\0\0"))
                {
                    let meta_data = &seg_data[6..];
                    tags.extend(parse_meta_ifd(meta_data));
                }
            }
            // APP4 — InfiRay Factory
            MARKER_APP4 => {
                if is_infray {
                    tags.extend(decode_infray_factory(seg_data));
                }
            }
            // APP5 — Ricoh RMETA or InfiRay Picture
            MARKER_APP5 => {
                if is_infray {
                    tags.extend(decode_infray_picture(seg_data));
                } else if seg_data.starts_with(b"RMETA\0") && seg_data.len() > 6 {
                    tags.extend(parse_ricoh_rmeta(&seg_data[6..]));
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
                // Adobe_CM segment: starts with "Adobe_CM", int16u at offset 8 = AdobeCMType
                if seg_data.starts_with(b"Adobe_CM") && seg_data.len() >= 10 {
                    let val = u16::from_be_bytes([seg_data[8], seg_data[9]]);
                    tags.push(crate::tag::Tag {
                        id: crate::tag::TagId::Numeric(0),
                        name: "AdobeCMType".into(),
                        description: "Adobe CM Type".into(),
                        group: crate::tag::TagGroup { family0: "APP13".into(), family1: "AdobeCM".into(), family2: "Image".into() },
                        raw_value: crate::value::Value::U16(val),
                        print_value: val.to_string(),
                        priority: 0,
                    });
                }
                // Photoshop / IPTC data + all IRBs
                else if seg_data.len() > PHOTOSHOP_HEADER.len()
                    && seg_data.starts_with(PHOTOSHOP_HEADER)
                {
                    let (iptc_data, irb_tags) = extract_photoshop_irbs(
                        &seg_data[PHOTOSHOP_HEADER.len()..],
                    );
                    tags.extend(irb_tags);
                    if let Some(iptc_data) = iptc_data {
                        // CurrentIPTCDigest = MD5 of raw IPTC block (from Perl IPTC.pm)
                        let digest = crate::md5::md5_hex(&iptc_data);
                        tags.push(crate::tag::Tag {
                            id: crate::tag::TagId::Text("CurrentIPTCDigest".into()),
                            name: "CurrentIPTCDigest".into(),
                            description: "Current IPTC Digest".into(),
                            group: crate::tag::TagGroup { family0: "Photoshop".into(), family1: "Photoshop".into(), family2: "Other".into() },
                            raw_value: crate::value::Value::String(digest.clone()),
                            print_value: digest, priority: 0,
                        });
                        match IptcReader::read(&iptc_data) {
                            Ok(iptc_tags) => tags.extend(iptc_tags),
                            Err(_) => {}
                        }
                    }
                }
            }
            // APP6 — GoPro GPMF, InfiRay MixMode, NITF, or EPPIM
            0xE6 => {
                if is_infray {
                    tags.extend(decode_infray_mixmode(seg_data));
                } else if seg_data.starts_with(b"GoPro\0") && seg_data.len() > 6 {
                    tags.extend(parse_gopro_gpmf(&seg_data[6..]));
                } else if seg_data.starts_with(b"NITF\0") || seg_data.starts_with(b"NTIF\0") {
                    tags.extend(process_nitf(&seg_data[5..]));
                } else if seg_data.starts_with(b"EPPIM\0") && seg_data.len() > 6 {
                    // EPPIM: Canon's "Extension of PrintIM" APP6 tag (from Perl JPEG.pm).
                    // Format: "EPPIM\0" + mini-TIFF with IFD0 containing tag 0xC4A5 (PrintIM data).
                    let tiff_data = &seg_data[6..];
                    tags.extend(process_eppim(tiff_data));
                }
            }
            // APP7 — InfiRay OpMode or Qualcomm Camera Attributes
            MARKER_APP7 => {
                if is_infray {
                    tags.extend(decode_infray_opmode(seg_data));
                } else if seg_data.len() > 27
                    && seg_data[0] == 0x1a
                    && seg_data[1..27].starts_with(b"Qualcomm Camera Attributes")
                {
                    // Qualcomm APP7 metadata (from Perl Qualcomm.pm / ExifTool.pm)
                    // Header: \x1a + "Qualcomm Camera Attributes" (27 bytes)
                    // Data starts at offset 27
                    tags.extend(parse_qualcomm(&seg_data[27..]));
                }
            }
            // APP8 — SPIFF or InfiRay Isothermal
            0xE8 => {
                if is_infray {
                    tags.extend(decode_infray_isothermal(seg_data));
                } else if seg_data.starts_with(b"SPIFF\0") {
                    tags.extend(process_spiff(&seg_data[6..]));
                }
            }
            // APP9 — InfiRay Sensor or Media Jukebox (XML metadata)
            0xE9 => {
                if is_infray {
                    tags.extend(decode_infray_sensor(seg_data));
                } else if seg_data.starts_with(b"Media Jukebox\0") {
                    // Skip "Media Jukebox\0" (14 bytes) + version(2) + type(1) = 17 bytes, then XML
                    let xml_start = seg_data.iter().position(|&b| b == b'<').unwrap_or(seg_data.len());
                    if xml_start < seg_data.len() {
                        tags.extend(process_media_jukebox_xml(&seg_data[xml_start..]));
                    }
                }
            }
            // APP11 — JPEG-HDR or JUMBF
            0xEB => {
                if seg_data.starts_with(b"HDR_RI ") {
                    tags.extend(process_jpeg_hdr(seg_data));
                } else if seg_data.len() >= 2 && seg_data.starts_with(b"JP") {
                    // JUMBF: APP11 with 'JP' prefix (from Perl: Jpeg2000::Main table)
                    // Format: 'JP'(2) + Z(uint16) + box_instance(uint32) + packet_seq(uint32) + jumb boxes
                    // The jumb box starts at offset 8 in seg_data
                    // (after 'JP'(2) + Z(2) + box_inst(4) = 8 bytes? or different layout)
                    // From analysis: seg_data[8..] contains the JUMBF box chain
                    // First box: LBox(4)+'jumb'(4) + jumd_box(LBox+4) + ...
                    tags.extend(process_jumbf_app11(seg_data));
                }
            }
            // APP12 — Ducky (Photoshop "Save for Web") or PictureInfo (Agfa/Olympus text format)
            MARKER_APP12 => {
                if seg_data.starts_with(b"Ducky") {
                    tags.extend(process_ducky(&seg_data[5..]));
                } else {
                    tags.extend(process_app12_picture_info(seg_data));
                }
            }
            // APP15 — GraphicConverter quality
            0xEF => {
                tags.extend(process_graphicconverter(seg_data));
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

    // Process accumulated FPXR segments (FlashPix Ready)
    if fpxr_seen {
        tags.extend(process_fpxr_segments(&fpxr_contents));
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

    // AFCP Trailer: scan end of file for "AXS!" or "AXS*" (from Perl AFCP.pm)
    if data.len() > 24 {
        let trailer_check = &data[data.len().saturating_sub(12)..];
        if trailer_check.starts_with(b"AXS!") || trailer_check.starts_with(b"AXS*") {
            let le = trailer_check[3] == b'*';
            let rd32_afcp = |d: &[u8], off: usize| -> u32 {
                if le { u32::from_le_bytes([d[off], d[off+1], d[off+2], d[off+3]]) }
                else { u32::from_be_bytes([d[off], d[off+1], d[off+2], d[off+3]]) }
            };
            let rd16_afcp = |d: &[u8], off: usize| -> u16 {
                if le { u16::from_le_bytes([d[off], d[off+1]]) }
                else { u16::from_be_bytes([d[off], d[off+1]]) }
            };

            let start_pos = rd32_afcp(trailer_check, 4) as usize;
            if start_pos + 18 < data.len() {
                let afcp = &data[start_pos..];
                // AXS header (12 bytes: "AXS!" + start + reserved)
                // Then: version(4) + numEntries(2)
                let _num_entries = rd16_afcp(afcp, 18) as usize; // at offset 12+4+2=18? No.
                // Actually: AXS!(4) + version_info(4) + num_entries(2) = offset 10
                // Perl: vers=substr(buff,4,2), numEntries=Get16u(buff,6)
                // buff is the first 8 bytes after seeking to start: AXS! header is 4+4+4=12
                // Wait — Perl reads: $raf->Read($buff, 8) after the AXS header (12 bytes)
                // So buff = 8 bytes at start_pos+12: version(2) + padding(2) + numEntries(2) + ...
                // Actually Perl does: seek(startPos), read(buff,12) = AXS header
                // then read(buff,8) = version info: vers=bytes[4..6], numEntries=Get16u(bytes,6)
                // So after AXS header (12 bytes), there's 8 bytes of version/numEntries
                // numEntries at offset 12+6=18 from start
                // AXS header: tag(4) + version(2) + numEntries(2) + reserved(4) = 12 bytes
                // Perl: numEntries = Get16u(buff, 6)
                let num_entries = rd16_afcp(&data, start_pos + 6) as usize;

                // Directory: 12 bytes each, starts right after the 12-byte header
                let dir_start = start_pos + 12;
                for i in 0..num_entries.min(20) {
                    let eoff = dir_start + i * 12;
                    if eoff + 12 > data.len() { break; }
                    let tag = &data[eoff..eoff + 4];
                    let size = rd32_afcp(&data, eoff + 4) as usize;
                    let offset = rd32_afcp(&data, eoff + 8) as usize;

                    if tag == b"IPTC" && offset + size <= data.len() {
                        let iptc_raw = &data[offset..offset + size];
                        let iptc_start = iptc_raw.iter().position(|&b| b == 0x1C).unwrap_or(0);
                        if let Ok(iptc_tags) = IptcReader::read(&iptc_raw[iptc_start..]) {
                            tags.extend(iptc_tags);
                        }
                    }
                }
            }
        }
    }

    // PhotoMechanic trailer: "cbipcbbl" signature anywhere after SOS (from Perl PhotoMechanic.pm)
    // The trailer can be followed by other trailers (CanonVRD, Samsung, etc.)
    // so we scan the whole file forward for the "cbipcbbl" signature.
    if let Some(pm_sig_pos) = data.windows(8).position(|w| w == b"cbipcbbl") {
        // Layout: pm_data(size bytes) + size(4 BE) + "cbipcbbl"(8)
        if pm_sig_pos >= 12 {
            let size = u32::from_be_bytes([
                data[pm_sig_pos-4], data[pm_sig_pos-3], data[pm_sig_pos-2], data[pm_sig_pos-1]
            ]) as usize;
            if size > 0 && pm_sig_pos >= 4 + size {
                let pm_data = &data[pm_sig_pos - 4 - size..pm_sig_pos - 4];
            // PhotoMechanic data is in IPTC format (record 2, datasets 209+)
            // But also contains standard IPTC records
            if let Some(start) = pm_data.iter().position(|&b| b == 0x1C) {
                if let Ok(iptc_tags) = IptcReader::read(&pm_data[start..]) {
                    // Map PM-specific datasets to tag names
                    for tag in &iptc_tags {
                        tags.push(tag.clone());
                    }
                }
                // Also extract PM-specific tags with custom names
                let mut pos = start;
                while pos + 5 <= pm_data.len() {
                    if pm_data[pos] != 0x1C { break; }
                    let rec = pm_data[pos + 1];
                    let ds = pm_data[pos + 2];
                    let len = u16::from_be_bytes([pm_data[pos+3], pm_data[pos+4]]) as usize;
                    pos += 5;
                    if pos + len > pm_data.len() { break; }
                    let val_bytes = &pm_data[pos..pos+len];
                    let name = match (rec, ds) {
                        (2, 216) => "Rotation",
                        (2, 217) => "CropLeft",
                        (2, 218) => "CropTop",
                        (2, 219) => "CropRight",
                        (2, 220) => "CropBottom",
                        (2, 221) => "Tagged",
                        (2, 222) => "ColorClass",
                        _ => { pos += len; continue; },
                    };
                    let raw_int = if len == 4 {
                        i32::from_be_bytes([val_bytes[0], val_bytes[1], val_bytes[2], val_bytes[3]])
                    } else if len == 2 {
                        i16::from_be_bytes([val_bytes[0], val_bytes[1]]) as i32
                    } else { 0 };
                    let raw_val = raw_int.to_string();
                    // Apply print conversions (from Perl PhotoMechanic.pm)
                    let print_val = match (rec, ds) {
                        (2, 221) => match raw_int { // Tagged: 0=No, 1=Yes
                            0 => "No".to_string(),
                            1 => "Yes".to_string(),
                            _ => raw_val.clone(),
                        },
                        (2, 222) => match raw_int { // ColorClass
                            0 => "0 (None)".to_string(),
                            1 => "1 (Winner)".to_string(),
                            2 => "2 (Winner alt)".to_string(),
                            3 => "3 (Superior)".to_string(),
                            4 => "4 (Superior alt)".to_string(),
                            5 => "5 (Typical)".to_string(),
                            6 => "6 (Typical alt)".to_string(),
                            7 => "7 (Extras)".to_string(),
                            8 => "8 (Trash)".to_string(),
                            _ => raw_val.clone(),
                        },
                        _ => raw_val.clone(),
                    };
                    tags.push(crate::tag::Tag {
                        id: crate::tag::TagId::Text(name.into()),
                        name: name.into(), description: name.into(),
                        group: crate::tag::TagGroup { family0: "PhotoMechanic".into(), family1: "PhotoMechanic".into(), family2: "Image".into() },
                        raw_value: crate::value::Value::String(raw_val), print_value: print_val, priority: 0,
                    });
                    pos += len;
                }
            }
            }
        }
    }

    // FotoStation trailer: 0xa1b2c3d4 signature (from Perl FotoStation.pm)
    // Blocks can appear anywhere in file (other trailers may follow them).
    // Each block footer: tag(2) + size(4) + sig(4). The sig is the LAST 4 bytes of each block.
    // We scan the entire file for all occurrences of the signature.
    {
        let fs_sig = [0xa1u8, 0xb2, 0xc3, 0xd4];
        let mut search_start = 0usize;
        while search_start + 4 <= data.len() {
            let found = data[search_start..].windows(4).position(|w| w == fs_sig);
            let sig_pos = match found {
                Some(p) => search_start + p,
                None => break,
            };
            search_start = sig_pos + 4; // next search starts after this sig

            // Footer is the 10 bytes ending at sig_pos+4: tag(2)+size(4)+sig(4)
            // footer starts at sig_pos-6
            if sig_pos < 6 { continue; }
            let footer_start = sig_pos - 6;
            let tag = u16::from_be_bytes([data[footer_start], data[footer_start+1]]);
            let size = u32::from_be_bytes([data[footer_start+2], data[footer_start+3],
                                           data[footer_start+4], data[footer_start+5]]) as usize;
            // size includes the 10-byte footer. data portion = size - 10.
            if size < 10 { continue; }
            let block_end = sig_pos + 4; // end of this block
            if block_end < size { continue; }
            let block_start = block_end - size;
            let rec_data = &data[block_start..block_start + size - 10];

            match tag {
                0x01 => {
                    // IPTC data
                    if let Some(start) = rec_data.iter().position(|&b| b == 0x1C) {
                        if let Ok(iptc_tags) = IptcReader::read(&rec_data[start..]) {
                            tags.extend(iptc_tags);
                        }
                    }
                }
                0x02 => {
                    // SoftEdit: binary data with crop/rotation info (from Perl FotoStation::SoftEdit)
                    // FORMAT=int32s, big-endian
                    let mk = |name: &str, val: String| crate::tag::Tag {
                        id: crate::tag::TagId::Text(name.into()),
                        name: name.into(), description: name.into(),
                        group: crate::tag::TagGroup { family0: "FotoStation".into(), family1: "FotoStation".into(), family2: "Image".into() },
                        raw_value: crate::value::Value::String(val.clone()), print_value: val, priority: 0,
                    };
                    // rd32s reads signed int32 BE at index*4
                    let rd32s = |idx: usize| -> i32 {
                        let off = idx * 4;
                        if off + 4 > rec_data.len() { return 0; }
                        i32::from_be_bytes([rec_data[off], rec_data[off+1], rec_data[off+2], rec_data[off+3]])
                    };
                    if rec_data.len() >= 16 {
                        tags.push(mk("OriginalImageWidth", rd32s(0).to_string()));
                        tags.push(mk("OriginalImageHeight", rd32s(1).to_string()));
                        tags.push(mk("ColorPlanes", rd32s(2).to_string()));
                    }
                    if rec_data.len() >= 20 {
                        // XYResolution: val / 1000 (ValueConv)
                        let xy_raw = rd32s(3);
                        let xy_val = xy_raw as f64 / 1000.0;
                        let xy_str = if xy_val == xy_val.floor() && xy_val.fract() == 0.0 {
                            format!("{}", xy_val as i64)
                        } else {
                            format!("{}", xy_val)
                        };
                        tags.push(mk("XYResolution", xy_str));
                    }
                    if rec_data.len() >= 24 {
                        // Rotation: $val ? 360 - $val / 100 : 0
                        let rot_raw = rd32s(4);
                        let rot_val = if rot_raw != 0 { 360.0 - rot_raw as f64 / 100.0 } else { 0.0 };
                        let rot_str = if rot_val == rot_val.floor() {
                            format!("{}", rot_val as i64)
                        } else {
                            format!("{}", rot_val)
                        };
                        tags.push(mk("Rotation", rot_str));
                    }
                    if rec_data.len() >= 40 {
                        // CropLeft/Top/Right/Bottom: val/1000, PrintConv adds "%"
                        let fmt_crop = |v: i32| -> String {
                            let f = v as f64 / 1000.0;
                            // Trim trailing zeros like Perl does
                            let s = format!("{}", f);
                            format!("{}%", s)
                        };
                        tags.push(mk("CropLeft",   fmt_crop(rd32s(6))));
                        tags.push(mk("CropTop",    fmt_crop(rd32s(7))));
                        tags.push(mk("CropRight",  fmt_crop(rd32s(8))));
                        tags.push(mk("CropBottom", fmt_crop(rd32s(9))));
                    }
                    if rec_data.len() >= 48 {
                        // CropRotation: -val / 100 (raw stored as int, not float)
                        let cr_raw = rd32s(11);
                        let cr_val = -(cr_raw as f64) / 100.0;
                        let cr_str = if cr_val == cr_val.floor() && cr_val.fract() == 0.0 {
                            format!("{}", cr_val as i64)
                        } else {
                            format!("{}", cr_val)
                        };
                        tags.push(mk("CropRotation", cr_str));
                    }
                }
                _ => {}
            }
        }
    }

    // FotoStation/PhotoMechanic trailers: scan for Photoshop segments after SOS
    // These are APP13 segments embedded after the image data
    {
        let sos_pos = data.windows(2).position(|w| w == [0xFF, 0xDA]);
        if let Some(sp) = sos_pos {
            // Scan rest of file for additional Photoshop segments
            let rest = &data[sp..];
            // Look for "Photoshop 3.0\0" or "cbipcbbl" markers
            if let Some(ps_pos) = rest.windows(14).position(|w| w == PHOTOSHOP_HEADER) {
                let ps_data = &rest[ps_pos + PHOTOSHOP_HEADER.len()..];
                let (iptc2, irb2) = extract_photoshop_irbs(ps_data);
                tags.extend(irb2);
                if let Some(iptc2_data) = iptc2 {
                    let _digest = crate::md5::md5_hex(&iptc2_data);
                    if let Ok(iptc_tags) = IptcReader::read(&iptc2_data) {
                        tags.extend(iptc_tags);
                    }
                }
            }
        }
    }

    // CanonVRD trailer: search for "CANON OPTIONAL DATA\0" footer (from Perl CanonVRD.pm)
    // The trailer has a 0x1c-byte header and 0x40-byte footer, both starting with the sig.
    // In JPEG the VRD may not be the last thing in the file; other trailers can follow it.
    // We scan backwards for the signature, treating each 0x40-byte candidate as a footer.
    // Footer bytes 20-23 (BE uint32) = contained data size; total = contained + 0x5c.
    {
        let sig = CANON_VRD_SIG;
        let sig_len = sig.len(); // 20 bytes
        let mut search_end = data.len();
        'vrd_scan: while search_end >= sig_len + 0x40 {
            let found = data[..search_end].windows(sig_len)
                .rposition(|w| w == sig);
            let candidate = match found {
                Some(p) => p,
                None => break,
            };
            search_end = candidate; // advance backwards for next iteration
            let footer_end = candidate + 0x40;
            if footer_end > data.len() { continue; }
            let footer = &data[candidate..footer_end];
            if footer.len() < 24 { continue; }
            let contained_len = u32::from_be_bytes([
                footer[20], footer[21], footer[22], footer[23],
            ]) as usize;
            let total_len = contained_len.saturating_add(0x5c);
            if total_len < 0x60 || total_len > 0x800000 { continue; }
            if footer_end < total_len { continue; }
            let vrd_start = footer_end - total_len;
            if !data[vrd_start..].starts_with(sig) { continue; }
            // Verify: header at vrd_start, footer at vrd_start + 0x1c + contained_len
            if vrd_start + 0x1c + contained_len != candidate { continue; }
            // Found valid VRD
            let vrd_data = &data[vrd_start..footer_end];
            tags.extend(parse_canon_vrd(vrd_data, total_len));
            break 'vrd_scan;
        }
    }

    // Samsung trailer: "QDIOBS" or "\0\0SEFT" signature (from Perl Samsung.pm::ProcessSamsung).
    // Format: data blocks + SEFH directory + [QDIO block] + "QDIOBS" terminator.
    // SEFH directory: "SEFH" + u32le_version + u32le_count + (count * 12-byte entries).
    // Each entry: u16_padding + u16le_type + u32le_noff + u32le_size.
    // Each block: u32le_type_marker + u32le_namelen + name + data.
    if let Some(qdiobs_pos) = data.windows(6).rposition(|w| w == b"QDIOBS") {
        // JSON data follows "QDIOBSvivo" or similar prefix in the Vivo-style trailer block.
        let after = &data[qdiobs_pos + 6..];
        if let Some(json_start) = after.iter().position(|&b| b == b'{') {
            let json_data = &after[json_start..];
            let mut depth = 0usize;
            let mut json_end = None;
            for (i, &b) in json_data.iter().enumerate() {
                match b {
                    b'{' => depth += 1,
                    b'}' => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 { json_end = Some(i + 1); break; }
                    }
                    _ => {}
                }
            }
            if let Some(end) = json_end {
                let json_str = String::from_utf8_lossy(&json_data[..end]).to_string();
                tags.push(crate::tag::Tag {
                    id: crate::tag::TagId::Text("JSONInfo".into()),
                    name: "JSONInfo".into(),
                    description: "JSON Info".into(),
                    group: crate::tag::TagGroup { family0: "Trailer".into(), family1: "Samsung".into(), family2: "Other".into() },
                    raw_value: crate::value::Value::String(json_str.clone()),
                    print_value: json_str, priority: 0,
                });
            }
        }

        // Parse SEFT/SEFH directory structure backward from QDIOBS.
        // Walk backward through {data}{4-byte size LE}{4-char type} blocks.
        // QDIOBS: rewind 2 bytes (before 'BS'), so effective end = qdiobs_pos + 4.
        let block_end = qdiobs_pos + 4; // position of "QDIOB" end = position of 'BS'
        tags.extend(parse_samsung_seft(data, block_end));
    }

    // MIE trailer: scan for "~\x10\x04\xfe" outer MIE group signature (from Perl MIE.pm).
    if let Some(mie_pos) = data.windows(4).rposition(|w| w == b"\x7e\x10\x04\xfe") {
        let mie_data = &data[mie_pos..];
        tags.extend(parse_mie_trailer(mie_data));
    }

    // IPTCDigest Warning: compare stored IPTCDigest with CurrentIPTCDigest
    {
        let stored = tags.iter().find(|t| t.name == "IPTCDigest").map(|t| t.print_value.clone());
        let current = tags.iter().find(|t| t.name == "CurrentIPTCDigest").map(|t| t.print_value.clone());
        if let (Some(stored_val), Some(current_val)) = (stored, current) {
            if stored_val != current_val {
                tags.push(crate::tag::Tag {
                    id: crate::tag::TagId::Text("Warning".into()),
                    name: "Warning".into(),
                    description: "Warning".into(),
                    group: crate::tag::TagGroup {
                        family0: "ExifTool".into(),
                        family1: "ExifTool".into(),
                        family2: "Other".into(),
                    },
                    raw_value: crate::value::Value::String("IPTCDigest is not current".into()),
                    print_value: "IPTCDigest is not current".into(),
                    priority: 0,
                });
            }
        }
    }

    // FLIR post-processing: fix MakerNote tag misidentification.
    // When Make contains "FLIR", the FLIR MakerNote IFD is parsed as Unknown manufacturer,
    // causing FLIR tag 0x0002 (ImageTemperatureMin) to be named "CPUVersions" (JVC table),
    // and tag 0x0003 (Emissivity) to be named "Quality".  Tag 0x0001 (ImageTemperatureMax)
    // is dropped entirely (no entry in any fallback table).
    // Also, composite LensID is generated from LensModel "FOL7" which Perl suppresses because
    // it doesn't match the /(mm|\d\/F)/ pattern required by LensID-2 composite.
    {
        let is_flir = tags.iter().any(|t| t.name == "Make"
            && t.print_value.to_uppercase().contains("FLIR"));
        if is_flir {
            // Rename "CPUVersions" (from MakerNotes) → "ImageTemperatureMin"
            for t in tags.iter_mut() {
                if t.name == "CPUVersions" && t.group.family0 == "MakerNotes" {
                    t.id = crate::tag::TagId::Text("ImageTemperatureMin".into());
                    t.name = "ImageTemperatureMin".into();
                    t.description = "ImageTemperatureMin".into();
                }
            }
            // Remove "Quality" from MakerNotes (FLIR tag 0x0003 = Emissivity, already in FFF)
            // Remove "LensID" composite (LensModel "FOL7" doesn't match Perl's /(mm|\d\/F)/ pattern)
            tags.retain(|t| {
                !(t.name == "Quality" && t.group.family0 == "MakerNotes")
                && !(t.name == "LensID" && t.group.family0 == "Composite")
            });
            // Add ImageTemperatureMax (FLIR MakerNote tag 0x0001, rational64s) by parsing
            // the EXIF APP1 MakerNote IFD directly, since it's dropped by the generic decoder.
            if !tags.iter().any(|t| t.name == "ImageTemperatureMax") {
                let (max_t, _) = read_flir_makernote_temps(data);
                if let Some(v) = max_t {
                    tags.insert(0, crate::tag::Tag {
                        id: crate::tag::TagId::Text("ImageTemperatureMax".into()),
                        name: "ImageTemperatureMax".into(),
                        description: "ImageTemperatureMax".into(),
                        group: crate::tag::TagGroup {
                            family0: "MakerNotes".into(),
                            family1: "FLIR".into(),
                            family2: "Camera".into(),
                        },
                        raw_value: crate::value::Value::String(v.to_string()),
                        print_value: v.to_string(),
                        priority: 0,
                    });
                }
            }
        }
    }

    Ok(tags)
}

/// Parse NITF APP6 segment (National Imagery Transmission Format).
/// Data is the content after the "NITF\0" or "NTIF\0" header (5 bytes).
fn process_nitf(data: &[u8]) -> Vec<crate::tag::Tag> {
    let mut tags = Vec::new();
    if data.len() < 14 { return tags; }

    let mk = |name: &str, val: String| crate::tag::Tag {
        id: crate::tag::TagId::Text(name.into()),
        name: name.into(), description: name.into(),
        group: crate::tag::TagGroup { family0: "APP6".into(), family1: "NITF".into(), family2: "Image".into() },
        raw_value: crate::value::Value::String(val.clone()), print_value: val, priority: 0,
    };

    tags.push(mk("NITFVersion", format!("{}.{:02}", data[0], data[1])));
    let fmt_byte = data[2] & 0xFF;
    tags.push(mk("ImageFormat", if fmt_byte == b'B' { "IMode B".into() } else { format!("{}", fmt_byte as char) }));
    if data.len() > 4 {
        tags.push(mk("BlocksPerRow", u16::from_be_bytes([data[3], data[4]]).to_string()));
    }
    if data.len() > 6 {
        tags.push(mk("BlocksPerColumn", u16::from_be_bytes([data[5], data[6]]).to_string()));
    }
    if data.len() > 7 {
        tags.push(mk("ImageColor", match data[7] { 0 => "Monochrome".into(), v => v.to_string() }));
    }
    if data.len() > 8 { tags.push(mk("BitDepth", data[8].to_string())); }
    if data.len() > 9 {
        tags.push(mk("ImageClass", match data[9] {
            0 => "General Purpose".into(), 4 => "Tactical Imagery".into(), v => v.to_string(),
        }));
    }
    if data.len() > 10 {
        tags.push(mk("JPEGProcess", match data[10] {
            1 => "Baseline sequential DCT, Huffman coding, 8-bit samples".into(),
            4 => "Extended sequential DCT, Huffman coding, 12-bit samples".into(),
            v => v.to_string(),
        }));
    }
    if data.len() > 12 {
        tags.push(mk("StreamColor", match data[12] { 0 => "Monochrome".into(), v => v.to_string() }));
    }
    if data.len() > 13 { tags.push(mk("StreamBitDepth", data[13].to_string())); }
    if data.len() > 17 {
        let flags = u32::from_be_bytes([data[14], data[15], data[16], data[17]]);
        tags.push(mk("Flags", format!("0x{:x}", flags)));
    }

    tags
}

/// Parse JUMBF APP11 segment (JPEG Universal Metadata Box Format).
/// From Perl: JPEG.pm APP11 handler and Jpeg2000.pm JUMD table.
/// seg_data starts with 'JP' (2 bytes), followed by Z(2)+box_inst(4)+packet_seq(4)
/// then JUMBF box chain starting at offset 8.
fn process_jumbf_app11(seg_data: &[u8]) -> Vec<crate::tag::Tag> {
    let mut tags = Vec::new();
    // Header: 'JP'(2) + Z(2) + box_instance(4) = 8 bytes; boxes start at offset 8
    // But from file analysis the outermost jumb box LBox is at seg_data[8]
    // Actually: 'JP'(2) + type16(2) + box_instance_num(4) = 8 bytes header
    // Then: LBox(4) + TBox(4) + content  -- the 'jumb' box
    if seg_data.len() < 12 { return tags; }
    let boxes_data = &seg_data[8..]; // skip JP header
    parse_jumbf_boxes(boxes_data, &mut tags);
    tags
}

/// Parse a sequence of JUMBF boxes from data.
/// Each box: LBox(4 BE) + TBox(4) + content(LBox-8 bytes)
fn parse_jumbf_boxes(data: &[u8], tags: &mut Vec<crate::tag::Tag>) {
    let mut pos = 0;
    while pos + 8 <= data.len() {
        let lbox = u32::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
        let tbox = &data[pos+4..pos+8];
        if lbox < 8 { break; }
        let content_end = pos + lbox;
        if content_end > data.len() { break; }
        let content = &data[pos+8..content_end];

        if tbox == b"jumb" {
            // JUMBF container box: recursively parse contents
            // Contents: jumd (description) box + content box(es)
            parse_jumbf_boxes_jumd(content, tags);
        }
        // (other box types like 'bfdb', 'bidb', 'json' etc. are not extracted here)

        pos += lbox;
        if pos >= data.len() { break; }
    }
}

/// Parse the contents of a 'jumb' box: extract JUMDType/JUMDLabel from 'jumd' sub-box.
fn parse_jumbf_boxes_jumd(data: &[u8], tags: &mut Vec<crate::tag::Tag>) {
    let mut pos = 0;
    while pos + 8 <= data.len() {
        let lbox = u32::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
        let tbox = &data[pos+4..pos+8];
        if lbox < 8 { break; }
        let content_end = pos + lbox;
        if content_end > data.len() { break; }
        let content = &data[pos+8..content_end];

        if tbox == b"jumd" {
            // JUMD description box (from Perl Jpeg2000::JUMD table)
            // type(16) + toggles(1) + label(null-terminated)
            if content.len() >= 17 {
                let type_bytes = &content[..16];
                let _toggles = content[16];
                // label: null-terminated string after toggles
                let label_data = &content[17..];
                let null_pos = label_data.iter().position(|&b| b == 0).unwrap_or(label_data.len());
                let label = String::from_utf8_lossy(&label_data[..null_pos]).to_string();

                // JUMDType: raw=hex string, print=formatted with dashes
                let type_hex = type_bytes.iter().map(|b| format!("{:02x}", b)).collect::<String>();
                // PrintConv: split into 8-4-4-16, and if first 4 bytes are printable ASCII, show as (ascii)
                let print_type = {
                    let a0 = &type_hex[..8];
                    let a1 = &type_hex[8..12];
                    let a2 = &type_hex[12..16];
                    let a3 = &type_hex[16..32];
                    let ascii4 = &type_bytes[..4];
                    let is_printable_alpha = ascii4.iter().all(|&b| b.is_ascii_alphanumeric());
                    if is_printable_alpha {
                        let ascii_str = String::from_utf8_lossy(ascii4);
                        format!("({})-{}-{}-{}", ascii_str, a1, a2, a3)
                    } else {
                        format!("{}-{}-{}-{}", a0, a1, a2, a3)
                    }
                };

                let jumbf_group = crate::tag::TagGroup {
                    family0: "JUMBF".into(),
                    family1: "JUMBF".into(),
                    family2: "Image".into(),
                };

                tags.push(crate::tag::Tag {
                    id: crate::tag::TagId::Text("JUMDType".into()),
                    name: "JUMDType".into(),
                    description: "JUMD Type".into(),
                    group: jumbf_group.clone(),
                    raw_value: crate::value::Value::String(type_hex),
                    print_value: print_type,
                    priority: 0,
                });

                if !label.is_empty() {
                    tags.push(crate::tag::Tag {
                        id: crate::tag::TagId::Text("JUMDLabel".into()),
                        name: "JUMDLabel".into(),
                        description: "JUMD Label".into(),
                        group: jumbf_group,
                        raw_value: crate::value::Value::String(label.clone()),
                        print_value: label,
                        priority: 0,
                    });
                }
            }
            // Only extract the outermost (first) JUMD for now
            return;
        } else if tbox == b"jumb" {
            // Nested jumb box
            parse_jumbf_boxes_jumd(content, tags);
        }

        pos += lbox;
        if pos >= data.len() { break; }
    }
}

/// Parse EPPIM APP6 segment (Extension of PrintIM).
/// From Perl: JPEG.pm APP6 EPPIM handler.
/// tiff_data is the mini-TIFF after the "EPPIM\0" header (6 bytes).
fn process_eppim(tiff_data: &[u8]) -> Vec<crate::tag::Tag> {
    let mut tags = Vec::new();
    if tiff_data.len() < 8 { return tags; }
    let is_le = tiff_data[0] == b'I' && tiff_data[1] == b'I';
    if !is_le && !(tiff_data[0] == b'M' && tiff_data[1] == b'M') {
        return tags;
    }
    let r16 = |d: &[u8], off: usize| -> u16 {
        if off + 2 > d.len() { return 0; }
        if is_le { u16::from_le_bytes([d[off], d[off+1]]) } else { u16::from_be_bytes([d[off], d[off+1]]) }
    };
    let r32 = |d: &[u8], off: usize| -> u32 {
        if off + 4 > d.len() { return 0; }
        if is_le { u32::from_le_bytes([d[off], d[off+1], d[off+2], d[off+3]]) } else { u32::from_be_bytes([d[off], d[off+1], d[off+2], d[off+3]]) }
    };
    let ifd0 = r32(tiff_data, 4) as usize;
    if ifd0 + 2 > tiff_data.len() { return tags; }
    let n = r16(tiff_data, ifd0) as usize;
    for i in 0..n {
        let off = ifd0 + 2 + i * 12;
        if off + 12 > tiff_data.len() { break; }
        let tag = r16(tiff_data, off);
        let dt = r16(tiff_data, off + 2);
        let count = r32(tiff_data, off + 4) as usize;
        let voff_raw = r32(tiff_data, off + 8) as usize;

        if tag == 0xC4A5 {
            // PrintIM data: undef[46] starting with 'PrintIM\0' + 4-byte version
            let voff = voff_raw;
            let size = match dt { 1 | 6 | 7 => count, 2 => count, _ => 0 };
            if size >= 11 && voff + size <= tiff_data.len() {
                let pm = &tiff_data[voff..voff + size];
                if pm.starts_with(b"PrintIM") {
                    let ver = String::from_utf8_lossy(&pm[7..11]).trim_end_matches('\0').to_string();
                    tags.push(crate::tag::Tag {
                        id: crate::tag::TagId::Text("PrintIMVersion".into()),
                        name: "PrintIMVersion".into(),
                        description: "PrintIM Version".into(),
                        group: crate::tag::TagGroup {
                            family0: "PrintIM".into(),
                            family1: "PrintIM".into(),
                            family2: "Printing".into(),
                        },
                        raw_value: crate::value::Value::String(ver.clone()),
                        print_value: ver,
                        priority: 0,
                    });
                }
            }
        }
    }
    tags
}

/// Parse SPIFF APP8 segment (Still Picture Interchange File Format).
/// Data is the content after the "SPIFF\0" header (6 bytes).
fn process_spiff(data: &[u8]) -> Vec<crate::tag::Tag> {
    let mut tags = Vec::new();
    if data.len() < 2 { return tags; }

    let mk = |name: &str, val: String| crate::tag::Tag {
        id: crate::tag::TagId::Text(name.into()),
        name: name.into(), description: name.into(),
        group: crate::tag::TagGroup { family0: "APP8".into(), family1: "SPIFF".into(), family2: "Image".into() },
        raw_value: crate::value::Value::String(val.clone()), print_value: val, priority: 0,
    };

    tags.push(mk("SPIFFVersion", format!("{}.{}", data[0], data[1])));
    if data.len() > 14 {
        let cs = match data[14] {
            0 => "Bi-level", 1 => "YCbCr, ITU-R BT 709, video",
            2 => "No color space specified", 3 => "YCbCr, ITU-R BT 601-1, RGB",
            4 => "YCbCr, ITU-R BT 601-1, video", 8 => "Gray-scale",
            9 => "PhotoYCC", 10 => "RGB", 11 => "CMY", 12 => "CMYK",
            13 => "YCCK", 14 => "CIELab", _ => "",
        };
        if !cs.is_empty() { tags.push(mk("ColorSpace", cs.into())); }
    }
    if data.len() > 16 {
        let comp = match data[16] {
            0 => "Uncompressed, interleaved, 8 bits per sample", 1 => "Modified Huffman",
            2 => "Modified READ", 3 => "Modified Modified READ", 4 => "JBIG", 5 => "JPEG", _ => "",
        };
        if !comp.is_empty() { tags.push(mk("Compression", comp.into())); }
    }
    if data.len() > 17 {
        let ru = match data[17] { 0 => "None", 1 => "inches", 2 => "cm", _ => "" };
        if !ru.is_empty() { tags.push(mk("ResolutionUnit", ru.into())); }
    }
    if data.len() > 21 {
        tags.push(mk("YResolution", u32::from_be_bytes([data[18], data[19], data[20], data[21]]).to_string()));
    }
    if data.len() > 25 {
        tags.push(mk("XResolution", u32::from_be_bytes([data[22], data[23], data[24], data[25]]).to_string()));
    }

    tags
}

/// Parse Media Jukebox APP9 XML metadata (from Perl JPEG::MediaJukebox).
/// Data starts at the first '<' of the XML (e.g. `<MJMD>`).
fn process_media_jukebox_xml(data: &[u8]) -> Vec<crate::tag::Tag> {
    let mut tags = Vec::new();
    let xml = String::from_utf8_lossy(data);

    let mk_xml = |name: &str, val: String| crate::tag::Tag {
        id: crate::tag::TagId::Text(name.into()),
        name: name.into(), description: name.into(),
        group: crate::tag::TagGroup { family0: "XML".into(), family1: "MediaJukebox".into(), family2: "Image".into() },
        raw_value: crate::value::Value::String(val.clone()), print_value: val, priority: 0,
    };

    let extract_xml_tag = |xml: &str, tag: &str| -> Option<String> {
        let open = format!("<{}>", tag);
        let close = format!("</{}>", tag);
        let start = xml.find(&open)?;
        let after_open = start + open.len();
        let end = xml[after_open..].find(&close)? + after_open;
        if after_open <= end { Some(xml[after_open..end].trim().to_string()) } else { None }
    };

    for tag_name in &["Tool_Name", "Tool_Version", "People", "Places", "Album", "Name"] {
        if let Some(val) = extract_xml_tag(&xml, tag_name) {
            if !val.is_empty() { tags.push(mk_xml(tag_name, val)); }
        }
    }

    // Date: days since Dec 30, 1899 to ExifTool datetime.
    // Perl: ConvertUnixTime(($val - 25569) * 86400)
    if let Some(date_str) = extract_xml_tag(&xml, "Date") {
        if let Ok(days) = date_str.parse::<f64>() {
            let unix_secs = ((days - 25569.0) * 86400.0) as i64;
            let formatted = unix_to_exiftool_datetime(unix_secs);
            let mut dt = mk_xml("Date", formatted);
            dt.group.family2 = "Time".into();
            tags.push(dt);
        }
    }

    tags
}

/// Convert a Unix timestamp to ExifTool datetime string "YYYY:MM:DD HH:MM:SS" (UTC).
fn unix_to_exiftool_datetime(unix_secs: i64) -> String {
    let secs_per_day = 86400i64;
    let days = unix_secs.div_euclid(secs_per_day);
    let tod = unix_secs.rem_euclid(secs_per_day);
    let (h, m, s) = (tod / 3600, (tod % 3600) / 60, tod % 60);

    // Civil date from days since Unix epoch (proleptic Gregorian calendar)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y0 = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let yr = if mo <= 2 { y0 + 1 } else { y0 };

    format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}", yr, mo, d, h, m, s)
}

/// Parse JPEG-HDR APP11 segment (from Perl ProcessJPEG_HDR in JPEG.pm).
/// Format: "HDR_RI " (7 bytes) + text key=value pairs + "~\0" + binary ratio image.
fn process_jpeg_hdr(data: &[u8]) -> Vec<crate::tag::Tag> {
    let mut tags = Vec::new();
    if data.len() < 9 { return tags; }

    let mk = |name: &str, val: String| crate::tag::Tag {
        id: crate::tag::TagId::Text(name.into()),
        name: name.into(), description: name.into(),
        group: crate::tag::TagGroup { family0: "APP11".into(), family1: "JPEG-HDR".into(), family2: "Image".into() },
        raw_value: crate::value::Value::String(val.clone()), print_value: val, priority: 0,
    };

    // Find ~\0 delimiter
    let tilde_pos = match data.windows(2).position(|w| w == b"~\x00") {
        Some(p) => p, None => return tags,
    };

    // Perl: $meta = substr($$dataPt, 7, $pos-9)
    // where $pos = Perl pos() after /~\0/g = tilde_pos + 2
    // so meta length = tilde_pos + 2 - 9 = tilde_pos - 7
    let meta_len = tilde_pos.saturating_sub(7);
    let meta = String::from_utf8_lossy(&data[7..7 + meta_len]);
    let meta_bytes = meta.as_bytes();

    // Parse /(\w+)=([^,\s]*)/g
    let mut i = 0usize;
    while i < meta_bytes.len() {
        if !meta_bytes[i].is_ascii_alphanumeric() && meta_bytes[i] != b'_' { i += 1; continue; }
        let key_start = i;
        while i < meta_bytes.len() && (meta_bytes[i].is_ascii_alphanumeric() || meta_bytes[i] == b'_') { i += 1; }
        let key = std::str::from_utf8(&meta_bytes[key_start..i]).unwrap_or("");
        if i >= meta_bytes.len() || meta_bytes[i] != b'=' { continue; }
        i += 1;
        let val_start = i;
        while i < meta_bytes.len() && meta_bytes[i] != b',' && !meta_bytes[i].is_ascii_whitespace() { i += 1; }
        let val = std::str::from_utf8(&meta_bytes[val_start..i]).unwrap_or("");

        let tag_name = match key {
            "ver" => "JPEG-HDRVersion",
            "ln0" => "Ln0", "ln1" => "Ln1", "s2n" => "S2n",
            "alp" => "Alpha", "bet" => "Beta", "cor" => "CorrectionMethod",
            other => other,
        };
        tags.push(mk(tag_name, val.to_string()));
    }

    // RatioImage: binary data after ~\0
    let ratio_data = &data[tilde_pos + 2..];
    if !ratio_data.is_empty() {
        let display = format!("(Binary data {} bytes, use -b option to extract)", ratio_data.len());
        let mut t = mk("RatioImage", display);
        t.raw_value = crate::value::Value::Binary(ratio_data.to_vec());
        t.group.family2 = "Preview".into();
        tags.push(t);
    }

    tags
}

/// Parse GraphicConverter APP15 quality segment (from Perl JPEG::GraphConv).
/// Format: "Q <number>" — stores JPEG quality value.
fn process_graphicconverter(data: &[u8]) -> Vec<crate::tag::Tag> {
    let mut tags = Vec::new();
    if data.is_empty() || data[0] != b'Q' { return tags; }
    let rest = String::from_utf8_lossy(&data[1..]);
    let trimmed = rest.trim_start();
    let num_end = trimmed.find(|c: char| !c.is_ascii_digit()).unwrap_or(trimmed.len());
    let quality_str = trimmed[..num_end].to_string();
    if quality_str.is_empty() { return tags; }

    tags.push(crate::tag::Tag {
        id: crate::tag::TagId::Text("Quality".into()),
        name: "Quality".into(),
        description: "Quality".into(),
        group: crate::tag::TagGroup { family0: "APP15".into(), family1: "GraphConv".into(), family2: "Image".into() },
        raw_value: crate::value::Value::String(quality_str.clone()),
        print_value: quality_str,
        priority: 0,
    });

    tags
}

/// Parse MIE trailer data (from Perl MIE.pm ProcessMIEGroup).
/// MIE element: sync(0x7E) + format(1) + nameLen(1) + valLen(1) + name + value.
/// Extended valLen: 253 => next 2 bytes, 254 => next 4 bytes, 255 => next 8 bytes.
/// Parse Samsung SEFT/SEFH trailer (from Perl Samsung.pm::ProcessSamsung).
/// Walks backward from block_end through {data}{u32le_size}{4-char_type} blocks.
/// When "SEFT" block found, parses SEFH directory to extract named data blocks.
/// Each block: [u32le_type_marker][u32le_namelen][name bytes][data bytes]
fn parse_samsung_seft(data: &[u8], block_end: usize) -> Vec<crate::tag::Tag> {
    let mut tags = Vec::new();
    if block_end < 8 { return tags; }

    let get_u32le = |d: &[u8], off: usize| -> u32 {
        if off + 4 > d.len() { return 0; }
        u32::from_le_bytes([d[off], d[off+1], d[off+2], d[off+3]])
    };
    let get_u16le = |d: &[u8], off: usize| -> u16 {
        if off + 2 > d.len() { return 0; }
        u16::from_le_bytes([d[off], d[off+1]])
    };

    // dirPos = absolute position of SEFH block start in data
    let mut cur_end = block_end;
    let mut dir_pos: Option<usize> = None;

    // Walk blocks backward until we find "SEFT"
    for _ in 0..10 { // limit iterations
        if cur_end < 8 { break; }
        let footer_pos = cur_end - 8;
        let size = get_u32le(data, footer_pos) as usize;
        let type_bytes = &data[footer_pos+4..footer_pos+8];
        if footer_pos < size { break; }
        let block_start = footer_pos - size;
        if type_bytes == b"SEFT" {
            dir_pos = Some(block_start);
            break;
        }
        // Skip other blocks (e.g., "QDIO")
        cur_end = block_start;
    }

    let dir_start = match dir_pos { Some(p) => p, None => return tags };

    // SEFH block content starts at dir_start
    // Check for "SEFH" magic at some offset (may have prefix bytes)
    let sefh_off = if data.len() > dir_start + 4 {
        // Find "SEFH" within the block
        let block_data = &data[dir_start..cur_end.saturating_sub(8)];
        block_data.windows(4).position(|w| w == b"SEFH").map(|p| dir_start + p)
    } else {
        None
    };

    let sefh_abs = match sefh_off { Some(p) => p, None => return tags };

    // SEFH header: "SEFH"(4) + version(4) + count(4) = 12 bytes
    if sefh_abs + 12 > data.len() { return tags; }
    let count = get_u32le(data, sefh_abs + 8) as usize;
    if count > 100 { return tags; }

    let mk_sam = |name: &str, raw: crate::value::Value, print: String| -> crate::tag::Tag {
        crate::tag::Tag {
            id: crate::tag::TagId::Text(name.into()),
            name: name.into(),
            description: name.into(),
            group: crate::tag::TagGroup {
                family0: "MakerNotes".into(),
                family1: "Samsung".into(),
                family2: "Other".into(),
            },
            raw_value: raw,
            print_value: print,
            priority: 0,
        }
    };

    for i in 0..count {
        let entry = sefh_abs + 12 + i * 12;
        if entry + 12 > data.len() { break; }
        let entry_type = get_u16le(data, entry + 2);
        let noff = get_u32le(data, entry + 4) as usize;
        let size = get_u32le(data, entry + 8) as usize;
        // Block data is at dirPos - noff (where dirPos is the SEFT block start)
        if noff > dir_start || size < 8 { continue; }
        let block_abs = dir_start - noff;
        if block_abs + size > data.len() { continue; }
        let buf2 = &data[block_abs..block_abs + size];
        let name_len = get_u32le(buf2, 4) as usize;
        if 8 + name_len > size { continue; }
        let name_bytes = &buf2[8..8 + name_len];
        let name_str = String::from_utf8_lossy(name_bytes).trim_end_matches('\0').to_string();
        let value_bytes = &buf2[8 + name_len..];

        match entry_type {
            0x0100 => {
                // EmbeddedAudioFileName (name field) + EmbeddedAudioFile (data field)
                if !name_str.is_empty() {
                    tags.push(mk_sam(
                        "EmbeddedAudioFileName",
                        crate::value::Value::String(name_str.clone()),
                        name_str,
                    ));
                }
                if !value_bytes.is_empty() {
                    tags.push(mk_sam(
                        "EmbeddedAudioFile",
                        crate::value::Value::Binary(value_bytes.to_vec()),
                        format!("(Binary data {} bytes, use -b option to extract)", value_bytes.len()),
                    ));
                }
            }
            _ => {} // Skip other types (0x0800 = metadata, etc.)
        }
    }

    tags
}

fn parse_mie_trailer(data: &[u8]) -> Vec<crate::tag::Tag> {
    // Parse MIE elements linearly, treating groups as transparent containers.
    // Key: groups with val_len=0 have inline sub-elements (stream-based, not length-delimited).
    // Groups with val_len>0 have their content embedded in the value bytes.
    let mut tags = Vec::new();
    let mut pos = 0usize;
    parse_mie_elements(data, &mut pos, &mut tags, 0);
    tags
}

/// Parse MIE elements from `data[pos..]`, recursing into groups.
/// Returns when an end-of-group terminator (tagLen=0) is found or data is exhausted.
///
/// MIE element layout per MIE.pm:
///   1. Header: sync(0x7E) + format(1) + tagLen(1) + raw_valLen(1)
///   2. Tag name: tagLen bytes
///   3. Extended valLen (if raw_valLen > 252): 2/4/8 bytes
///   4. Value: valLen bytes
fn parse_mie_elements(data: &[u8], pos: &mut usize, tags: &mut Vec<crate::tag::Tag>, depth: usize) {
    if depth > 8 { return; } // prevent infinite recursion

    while *pos + 4 <= data.len() {
        if data[*pos] != 0x7E { *pos += 1; continue; } // skip non-sync bytes
        *pos += 1;
        if *pos + 3 > data.len() { break; }

        let format = data[*pos]; *pos += 1;
        let name_len = data[*pos] as usize; *pos += 1;
        let raw_vlen = data[*pos] as usize; *pos += 1;

        // Step 1: Read tag name (BEFORE decoding extended val len — per MIE spec)
        if name_len == 0 {
            // End-of-group: tagLen=0, decode val_len and skip it, then return
            let val_len: usize = if raw_vlen <= 252 {
                raw_vlen
            } else {
                let extra = 1usize << (256 - raw_vlen);
                if *pos + extra > data.len() { break; }
                let mut v = 0usize;
                for k in 0..extra { v = (v << 8) | (data[*pos + k] as usize); }
                *pos += extra;
                v
            };
            *pos += val_len;
            return; // end of this group level
        }

        if *pos + name_len > data.len() { break; }
        let name = String::from_utf8_lossy(&data[*pos..*pos + name_len]).to_string();
        *pos += name_len;

        // Step 2: Decode extended value length (AFTER reading name — per MIE spec)
        let val_len: usize = if raw_vlen <= 252 {
            raw_vlen
        } else {
            let extra = 1usize << (256 - raw_vlen);
            if *pos + extra > data.len() { break; }
            let mut v = 0usize;
            for k in 0..extra { v = (v << 8) | (data[*pos + k] as usize); }
            *pos += extra;
            v
        };

        let type_nibble = (format >> 4) & 0x0F;

        if type_nibble == 0x1 {
            // MIE group element
            if val_len == 0 {
                // Inline group: sub-elements follow immediately in the stream
                parse_mie_elements(data, pos, tags, depth + 1);
            } else {
                // Embedded group: content is the next val_len bytes
                if *pos + val_len > data.len() { break; }
                let sub_data = &data[*pos..*pos + val_len];
                *pos += val_len;
                let mut sub_pos = 0usize;
                parse_mie_elements(sub_data, &mut sub_pos, tags, depth + 1);
            }
        } else {
            // Leaf value
            if *pos + val_len > data.len() { break; }
            let val_bytes = &data[*pos..*pos + val_len];
            *pos += val_len;

            // Skip internal MIE metadata names starting with digit (e.g. "0MIE", "0Type")
            if name.starts_with(|c: char| c.is_ascii_digit()) { continue; }
            if name.is_empty() { continue; }

            // Map known MIE tag name aliases
            let name = match name.as_str() {
                "zmie" => "TrailerSignature".to_string(),
                _ => name,
            };

            let val_str = String::from_utf8_lossy(val_bytes).trim_end_matches('\0').to_string();
            tags.push(crate::tag::Tag {
                id: crate::tag::TagId::Text(name.clone()),
                name: name.clone(),
                description: name.clone(),
                group: crate::tag::TagGroup {
                    family0: "MIE".into(), family1: "MIE".into(), family2: "Other".into(),
                },
                raw_value: crate::value::Value::String(val_str.clone()),
                print_value: val_str,
                priority: 0,
            });
        }
    }
}

/// Decode InfiRay IJPEG APP2 data (from Perl InfiRay.pm).
fn decode_infray_version(data: &[u8]) -> Vec<crate::tag::Tag> {
    let mut tags = Vec::new();
    if data.len() < 0x50 { return tags; }

    let mk = |name: &str, val: String| crate::tag::Tag {
        id: crate::tag::TagId::Text(name.into()),
        name: name.into(), description: name.into(),
        group: crate::tag::TagGroup { family0: "APP2".into(), family1: "InfiRay".into(), family2: "Camera".into() },
        raw_value: crate::value::Value::String(val.clone()), print_value: val, priority: 0,
    };

    let ru16 = |off: usize| u16::from_le_bytes([data[off], data[off+1]]);
    let _ru32 = |off: usize| u32::from_le_bytes([data[off], data[off+1], data[off+2], data[off+3]]);
    let _rf32 = |off: usize| f32::from_le_bytes([data[off], data[off+1], data[off+2], data[off+3]]);

    tags.push(mk("IJPEGVersion", format!("{}.{}.{}.{}", data[0], data[1], data[2], data[3])));
    if data.len() > 0x11 {
        tags.push(mk("IJPEGOrgType", data[0x0C].to_string()));
        tags.push(mk("IJPEGDispType", data[0x0D].to_string()));
        tags.push(mk("IJPEGRotate", data[0x0E].to_string()));
        tags.push(mk("IJPEGMirrorFlip", data[0x0F].to_string()));
        tags.push(mk("ImageColorSwitchable", data[0x10].to_string()));
        tags.push(mk("ThermalColorPalette", ru16(0x11).to_string()));
    }
    if data.len() > 0x30 {
        tags.push(mk("IRDataSize", format!("{}", u64::from_le_bytes([data[0x20],data[0x21],data[0x22],data[0x23],data[0x24],data[0x25],data[0x26],data[0x27]]))));
        tags.push(mk("IRDataFormat", ru16(0x28).to_string()));
        tags.push(mk("IRImageWidth", ru16(0x2A).to_string()));
        tags.push(mk("IRImageHeight", ru16(0x2C).to_string()));
        tags.push(mk("IRImageBpp", data[0x2E].to_string()));
    }
    if data.len() > 0x48 {
        tags.push(mk("TempDataSize", format!("{}", u64::from_le_bytes([data[0x30],data[0x31],data[0x32],data[0x33],data[0x34],data[0x35],data[0x36],data[0x37]]))));
        tags.push(mk("TempDataFormat", ru16(0x38).to_string()));
        tags.push(mk("TempImageWidth", ru16(0x3A).to_string()));
        tags.push(mk("TempImageHeight", ru16(0x3C).to_string()));
        tags.push(mk("TempImageBpp", data[0x3E].to_string()));
    }
    if data.len() > 0x4E {
        tags.push(mk("VisibleDataSize", format!("{}", u64::from_le_bytes([data[0x40],data[0x41],data[0x42],data[0x43],data[0x44],data[0x45],data[0x46],data[0x47]]))));
        tags.push(mk("VisibleDataFormat", ru16(0x48).to_string()));
        tags.push(mk("VisibleImageWidth", ru16(0x4A).to_string()));
        tags.push(mk("VisibleImageHeight", ru16(0x4C).to_string()));
        tags.push(mk("VisibleImageBpp", data[0x4E].to_string()));
    }
    // IJPEGTempVersion at 0x50
    if data.len() > 0x54 {
        tags.push(mk("IJPEGTempVersion", format!("{}.{}.{}.{}", data[0x50], data[0x51], data[0x52], data[0x53])));
    }

    tags
}

/// Decode FLIR FFF data (from Perl FLIR.pm ProcessFLIR).
fn decode_flir_fff(data: &[u8]) -> Vec<crate::tag::Tag> {
    let mut tags = Vec::new();
    if data.len() < 0x40 { return tags; }

    let mk = |name: &str, val: String| crate::tag::Tag {
        id: crate::tag::TagId::Text(name.into()),
        name: name.into(), description: name.into(),
        group: crate::tag::TagGroup { family0: "APP1".into(), family1: "FLIR".into(), family2: "Camera".into() },
        raw_value: crate::value::Value::String(val.clone()), print_value: val, priority: 0,
    };

    // Detect byte order from version at offset 0x14
    let _ver_be = u32::from_be_bytes([data[0x14], data[0x15], data[0x16], data[0x17]]);
    let ver_le = u32::from_le_bytes([data[0x14], data[0x15], data[0x16], data[0x17]]);
    let le = ver_le >= 100 && ver_le < 200;

    let rd32 = |off: usize| -> u32 {
        if off + 4 > data.len() { return 0; }
        if le { u32::from_le_bytes([data[off], data[off+1], data[off+2], data[off+3]]) }
        else { u32::from_be_bytes([data[off], data[off+1], data[off+2], data[off+3]]) }
    };
    let _rd_f32 = |off: usize| -> f32 {
        if off + 4 > data.len() { return 0.0; }
        let bits = rd32(off);
        f32::from_bits(bits)
    };

    // Read directory
    let dir_offset = rd32(0x18) as usize;
    let num_entries = rd32(0x1C) as usize;

    tags.push(mk("CreatorSoftware", String::from_utf8_lossy(&data[4..20]).trim_end_matches('\0').to_string()));

    for i in 0..num_entries.min(50) {
        let entry_off = dir_offset + i * 0x20;
        if entry_off + 0x20 > data.len() { break; }

        let rec_type = if le { u16::from_le_bytes([data[entry_off], data[entry_off + 1]]) } else { u16::from_be_bytes([data[entry_off], data[entry_off + 1]]) };
        let rec_offset = rd32(entry_off + 0x0C) as usize;
        let rec_size = rd32(entry_off + 0x10) as usize;

        if rec_offset + rec_size > data.len() { continue; }
        let rec = &data[rec_offset..rec_offset + rec_size];

        match rec_type {
            0x20 => {
                // CameraInfo (from Perl FLIR::CameraInfo)
                if rec.len() >= 200 {
                    let ci_le = rec.len() > 2 && rec[0] == 2; // byte order from first int16u
                    let rf = |off: usize| -> f32 {
                        if off + 4 > rec.len() { return 0.0; }
                        let bits = if ci_le {
                            u32::from_le_bytes([rec[off], rec[off+1], rec[off+2], rec[off+3]])
                        } else {
                            u32::from_be_bytes([rec[off], rec[off+1], rec[off+2], rec[off+3]])
                        };
                        f32::from_bits(bits)
                    };
                    tags.push(mk("Emissivity", format!("{:.2}", rf(32))));
                    tags.push(mk("ObjectDistance", format!("{:.2} m", rf(36))));
                    tags.push(mk("ReflectedApparentTemperature", format!("{:.1} C", rf(40) - 273.15)));
                    tags.push(mk("AtmosphericTemperature", format!("{:.1} C", rf(44) - 273.15)));
                    tags.push(mk("IRWindowTemperature", format!("{:.1} C", rf(48) - 273.15)));
                    tags.push(mk("IRWindowTransmission", format!("{:.2}", rf(52))));
                    tags.push(mk("RelativeHumidity", format!("{:.2}", rf(60))));
                    tags.push(mk("PlanckR1", format!("{}", rf(88))));
                    tags.push(mk("PlanckB", format!("{}", rf(92))));
                    tags.push(mk("PlanckF", format!("{}", rf(96))));
                    tags.push(mk("AtmosphericTransAlpha1", format!("{}", rf(112))));
                    tags.push(mk("AtmosphericTransAlpha2", format!("{}", rf(116))));
                    tags.push(mk("AtmosphericTransBeta1", format!("{}", rf(120))));
                    tags.push(mk("AtmosphericTransBeta2", format!("{}", rf(124))));
                    tags.push(mk("AtmosphericTransX", format!("{}", rf(128))));
                    let max_temp = rf(144) - 273.15;
                    let min_temp = rf(148) - 273.15;
                    tags.push(mk("CameraTemperatureRangeMax", format!("{:.1} C", max_temp)));
                    tags.push(mk("CameraTemperatureRangeMin", format!("{:.1} C", min_temp)));
                    tags.push(mk("CameraTemperatureMaxClip", format!("{:.1} C", rf(152) - 273.15)));
                    tags.push(mk("CameraTemperatureMinClip", format!("{:.1} C", rf(156) - 273.15)));
                    tags.push(mk("CameraTemperatureMaxSaturated", format!("{:.1} C", rf(160) - 273.15)));
                    tags.push(mk("CameraTemperatureMinSaturated", format!("{:.1} C", rf(164) - 273.15)));
                    tags.push(mk("CameraTemperatureMaxWarn", format!("{:.1} C", rf(168) - 273.15)));
                    tags.push(mk("CameraTemperatureMinWarn", format!("{:.1} C", rf(172) - 273.15)));
                    // Strings at fixed offsets
                    if rec.len() >= 260 {
                        let cam_model = String::from_utf8_lossy(&rec[212..244]).trim_end_matches('\0').to_string();
                        if !cam_model.is_empty() { tags.push(mk("CameraModel", cam_model)); }
                        let cam_pn = String::from_utf8_lossy(&rec[244..260]).trim_end_matches('\0').to_string();
                        if !cam_pn.is_empty() { tags.push(mk("CameraPartNumber", cam_pn)); }
                        let cam_sn = String::from_utf8_lossy(&rec[260..276]).trim_end_matches('\0').to_string();
                        if !cam_sn.is_empty() { tags.push(mk("CameraSerialNumber", cam_sn)); }
                    }
                    if rec.len() >= 572 {
                        let cam_sw = String::from_utf8_lossy(&rec[276..292]).trim_end_matches('\0').to_string();
                        if !cam_sw.is_empty() { tags.push(mk("CameraSoftware", cam_sw)); }
                        let lens_model = String::from_utf8_lossy(&rec[368..400]).trim_end_matches('\0').to_string();
                        if !lens_model.is_empty() { tags.push(mk("LensModel", lens_model)); }
                        let lens_pn = String::from_utf8_lossy(&rec[400..416]).trim_end_matches('\0').to_string();
                        tags.push(mk("LensPartNumber", lens_pn));
                        let lens_sn = String::from_utf8_lossy(&rec[416..432]).trim_end_matches('\0').to_string();
                        tags.push(mk("LensSerialNumber", lens_sn));
                        let fov = rf(436);
                        if fov > 0.0 { tags.push(mk("FieldOfView", format!("{:.1} deg", fov))); }
                        // FilterModel: string[16] at 0x1ec=492 (Perl: Format => 'string[16]')
                        let filter_model = String::from_utf8_lossy(&rec[492..508]).trim_end_matches('\0').to_string();
                        tags.push(mk("FilterModel", filter_model));
                        // FilterPartNumber: string[32] at 0x1fc=508
                        let filter_pn = String::from_utf8_lossy(&rec[508..540]).trim_end_matches('\0').to_string();
                        tags.push(mk("FilterPartNumber", filter_pn));
                        // FilterSerialNumber: string[32] at 0x21c=540
                        let filter_sn = String::from_utf8_lossy(&rec[540..572]).trim_end_matches('\0').to_string();
                        tags.push(mk("FilterSerialNumber", filter_sn));
                    }
                    tags.push(mk("PeakSpectralSensitivity", format!("{:.1} um", rf(440))));
                    tags.push(mk("FocusStepCount", rd32(444).to_string()));
                    tags.push(mk("FocusDistance", format!("{:.1} m", rf(448))));
                    // PlanckO (int32s) and PlanckR2 (float)
                    if rec.len() >= 784 {
                        let planck_o = if ci_le { i32::from_le_bytes([rec[776], rec[777], rec[778], rec[779]]) } else { i32::from_be_bytes([rec[776], rec[777], rec[778], rec[779]]) };
                        tags.push(mk("PlanckO", planck_o.to_string()));
                        tags.push(mk("PlanckR2", format!("{}", rf(780))));
                    }
                    tags.push(mk("FrameRate", format!("{}", u16::from_le_bytes([rec[452], rec[453]]))));

                    // Additional CameraInfo fields (from Perl FLIR::CameraInfo)
                    if rec.len() >= 830 {
                        // RawValue stats (int16u)
                        let ru16 = |off: usize| -> u16 {
                            if ci_le { u16::from_le_bytes([rec[off], rec[off+1]]) }
                            else { u16::from_be_bytes([rec[off], rec[off+1]]) }
                        };
                        tags.push(mk("RawValueRangeMin", ru16(784).to_string()));
                        tags.push(mk("RawValueRangeMax", ru16(786).to_string()));
                        tags.push(mk("RawValueMedian", ru16(824).to_string()));
                        tags.push(mk("RawValueRange", ru16(828).to_string()));
                    }
                    // Note: ImageTemperatureMax/Min come from FLIR MakerNotes IFD (tags 0x0001/0x0002),
                    // not from CameraInfo. They are handled in post-processing below.
                }
            }
            0x22 => {
                // PaletteInfo (from Perl FLIR::PaletteInfo)
                if rec.len() >= 28 {
                    tags.push(mk("PaletteColors", rec[0].to_string()));
                    // Colors at fixed offsets (3 bytes each: R,G,B)
                    let color = |off: usize| -> String {
                        if off + 3 <= rec.len() { format!("{} {} {}", rec[off], rec[off+1], rec[off+2]) }
                        else { String::new() }
                    };
                    tags.push(mk("AboveColor", color(6)));
                    tags.push(mk("BelowColor", color(9)));
                    tags.push(mk("OverflowColor", color(12)));
                    tags.push(mk("UnderflowColor", color(15)));
                    tags.push(mk("Isotherm1Color", color(18)));
                    tags.push(mk("Isotherm2Color", color(21)));
                    tags.push(mk("PaletteMethod", rec[26].to_string()));
                    tags.push(mk("PaletteStretch", rec[27].to_string()));
                    if rec.len() >= 128 {
                        let fname = String::from_utf8_lossy(&rec[48..80]).trim_end_matches('\0').to_string();
                        if !fname.is_empty() { tags.push(mk("PaletteFileName", fname)); }
                        let pname = String::from_utf8_lossy(&rec[80..112]).trim_end_matches('\0').to_string();
                        if !pname.is_empty() { tags.push(mk("PaletteName", pname)); }
                    }
                    // Palette data
                    let pc = rec[0] as usize;
                    if pc > 0 && 112 + pc * 3 <= rec.len() {
                        tags.push(mk("Palette", format!("(Binary data {} bytes)", pc * 3)));
                    }
                }
            }
            0x01 => {
                // RawData — extract dimensions and image type (Perl FLIR::RawData)
                // FORMAT => 'int16u', FIRST_ENTRY => 0
                // Entry 0 (bytes 0-1): byte order check (should be 0x0002)
                // Entry 1 (bytes 2-3): RawThermalImageWidth
                // Entry 2 (bytes 4-5): RawThermalImageHeight
                // Entry 16 (bytes 32+): image data starting at offset 0x20
                if rec.len() >= 34 {
                    // Determine record byte order from first int16u (should be 0x0002)
                    let rec_le = u16::from_le_bytes([rec[0], rec[1]]) == 0x0002;
                    let rw = |off: usize| -> u16 {
                        if rec_le { u16::from_le_bytes([rec[off], rec[off+1]]) }
                        else { u16::from_be_bytes([rec[off], rec[off+1]]) }
                    };
                    let w = rw(2);
                    let h = rw(4);
                    tags.push(mk("RawThermalImageWidth", w.to_string()));
                    tags.push(mk("RawThermalImageHeight", h.to_string()));
                    // Image data starts at 0x20 (entry 16 * 2 bytes per int16u)
                    // GetImageType checks magic bytes to determine format (Perl FLIR::GetImageType)
                    let img_data = &rec[0x20..];
                    let type_str = if img_data.starts_with(b"\x89PNG\r\n\x1a\n") {
                        "PNG"
                    } else if img_data.starts_with(b"\xff\xd8\xff") {
                        "JPG"
                    } else if img_data.len() == (w as usize) * (h as usize) * 2 {
                        "TIFF"
                    } else {
                        "DAT"
                    };
                    tags.push(mk("RawThermalImageType", type_str.into()));
                    tags.push(mk("RawThermalImage", format!("(Binary data {} bytes)", rec.len())));
                }
            }
            _ => {}
        }
    }

    tags
}

/// Extract FLIR MakerNote tags from raw JPEG EXIF data.
/// The FLIR MakerNote IFD contains ImageTemperatureMax (0x0001) and ImageTemperatureMin (0x0002)
/// as rational64s values (Perl FLIR::Main table).
/// Returns (max_temp, min_temp) as raw rational numerator/denominator values.
fn read_flir_makernote_temps(jpeg_data: &[u8]) -> (Option<i64>, Option<i64>) {
    let mut pos = 2usize;
    while pos + 4 <= jpeg_data.len() {
        if jpeg_data[pos] != 0xFF { pos += 1; continue; }
        let marker = jpeg_data[pos + 1];
        pos += 2;
        if marker == 0xD8 || (0xD0..=0xD7).contains(&marker) { continue; }
        if pos + 2 > jpeg_data.len() { break; }
        let seg_len = u16::from_be_bytes([jpeg_data[pos], jpeg_data[pos + 1]]) as usize;
        if seg_len < 2 || pos + seg_len > jpeg_data.len() { break; }
        let seg = &jpeg_data[pos + 2..pos + seg_len];
        pos += seg_len;

        if marker == 0xE1 && seg.len() > 6 && seg.starts_with(b"Exif\0\0") {
            let exif = &seg[6..];
            if exif.len() < 8 { break; }
            let le = &exif[..2] == b"II";
            let ru16 = |d: &[u8], off: usize| -> usize {
                if off + 2 > d.len() { return 0; }
                if le { u16::from_le_bytes([d[off], d[off+1]]) as usize }
                else { u16::from_be_bytes([d[off], d[off+1]]) as usize }
            };
            let ru32 = |d: &[u8], off: usize| -> usize {
                if off + 4 > d.len() { return 0; }
                if le { u32::from_le_bytes([d[off], d[off+1], d[off+2], d[off+3]]) as usize }
                else { u32::from_be_bytes([d[off], d[off+1], d[off+2], d[off+3]]) as usize }
            };
            let ri32 = |d: &[u8], off: usize| -> i64 {
                if off + 4 > d.len() { return 0; }
                if le { i32::from_le_bytes([d[off], d[off+1], d[off+2], d[off+3]]) as i64 }
                else { i32::from_be_bytes([d[off], d[off+1], d[off+2], d[off+3]]) as i64 }
            };
            // IFD0
            let ifd0_off = ru32(exif, 4);
            if ifd0_off + 2 > exif.len() { break; }
            let ifd0_count = ru16(exif, ifd0_off);
            // Find ExifIFD (0x8769)
            let mut exif_ifd_off = 0usize;
            for i in 0..ifd0_count {
                let eoff = ifd0_off + 2 + i * 12;
                if eoff + 12 > exif.len() { break; }
                let tag = ru16(exif, eoff);
                if tag == 0x8769 {
                    exif_ifd_off = ru32(exif, eoff + 8);
                    break;
                }
            }
            if exif_ifd_off == 0 || exif_ifd_off + 2 > exif.len() { break; }
            let exif_count = ru16(exif, exif_ifd_off);
            // Find MakerNote (0x927C)
            let mut mn_off = 0usize;
            let mut mn_count = 0usize;
            for j in 0..exif_count {
                let etoff = exif_ifd_off + 2 + j * 12;
                if etoff + 12 > exif.len() { break; }
                let tag = ru16(exif, etoff);
                if tag == 0x927C {
                    mn_count = ru32(exif, etoff + 4);
                    mn_off = ru32(exif, etoff + 8);
                    break;
                }
            }
            if mn_off == 0 || mn_off + mn_count > exif.len() { break; }
            let mn = &exif[mn_off..mn_off + mn_count];
            if mn.len() < 2 { break; }
            // FLIR MakerNote is a plain IFD (no header), same byte order as EXIF
            let mn_entries = ru16(mn, 0);
            if mn_entries > 500 { break; }
            let mut max_t: Option<i64> = None;
            let mut min_t: Option<i64> = None;
            for k in 0..mn_entries {
                let koff = 2 + k * 12;
                if koff + 12 > mn.len() { break; }
                let ktag = ru16(mn, koff);
                let ktype = ru16(mn, koff + 2);
                let kcount_val = ru32(mn, koff + 4);
                // Only handle rational64u/s (type 5 or 10), count=1
                if (ktype == 5 || ktype == 10) && kcount_val == 1 {
                    let val_off = ru32(mn, koff + 8); // offset into full EXIF data
                    if val_off + 8 <= exif.len() {
                        let num = ri32(exif, val_off);
                        let den = ri32(exif, val_off + 4);
                        let rational_val = if den != 0 { num / den } else { 0 };
                        match ktag {
                            0x0001 => max_t = Some(rational_val),
                            0x0002 => min_t = Some(rational_val),
                            _ => {}
                        }
                    }
                }
                if max_t.is_some() && min_t.is_some() { break; }
            }
            return (max_t, min_t);
        }
    }
    (None, None)
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
            // Decode IRBs with sub-tags (ResolutionInfo, etc.)
            decode_photoshop_irb_subtags(resource_id, irb_data, &mut tags);

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
        0x03ED => "ResolutionInfo", // decoded specially below
        0x03F3 => "PrintFlags",
        // 0x0406 => JPEG_Quality — suppressed (decoded via subtags)
        0x0408 => "GridGuidesInfo",
        0x040A => "CopyrightFlag",
        0x040B => "URL",
        0x040C => "ThumbnailImage",
        // 0x0414 => DocumentSpecificIDs — suppressed
        0x0419 => "GlobalAltitude",
        0x041A => "ICC_Profile",
        // 0x041E => URLList — suppressed (decoded via subtags)
        0x0421 => "VersionInfo",
        0x0425 => "IPTCDigest",
        0x0426 => "PrintScale",
        0x043C => "MeasurementScale",
        0x043D => "TimelineInfo",
        0x043E => "SheetDisclosure",
        0x043F => "DisplayInfo",
        0x0440 => "OnionSkins",
        0x2710 => "PrintInfo2",
        0x041B => "SpotHalftone",
        0x041D => "AlphaIdentifiers",
        0x041F => "PrintFlagsInfo",
        _ => "",
    }
}

fn decode_photoshop_irb_subtags(id: u16, data: &[u8], tags: &mut Vec<crate::tag::Tag>) {
    let mk = |name: &str, val: String| crate::tag::Tag {
        id: crate::tag::TagId::Text(name.into()),
        name: name.into(), description: name.into(),
        group: crate::tag::TagGroup { family0: "Photoshop".into(), family1: "Photoshop".into(), family2: "Image".into() },
        raw_value: crate::value::Value::String(val.clone()), print_value: val, priority: 0,
    };

    match id {
        0x03ED if data.len() >= 14 => {
            // ResolutionInfo (from Perl Photoshop::Resolution)
            let xres = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as f64 / 65536.0;
            tags.push(mk("XResolution", format!("{}", (xres * 100.0).round() / 100.0)));
            let units_x = match u16::from_be_bytes([data[4], data[5]]) { 1 => "inches", 2 => "cm", _ => "" };
            if !units_x.is_empty() { tags.push(mk("DisplayedUnitsX", units_x.into())); }
            // Bytes 6-7: WidthUnit (not commonly used)
            let yres = u32::from_be_bytes([data[8], data[9], data[10], data[11]]) as f64 / 65536.0;
            tags.push(mk("YResolution", format!("{}", (yres * 100.0).round() / 100.0)));
            let units_y = match u16::from_be_bytes([data[12], data[13]]) { 1 => "inches", 2 => "cm", _ => "" };
            if !units_y.is_empty() { tags.push(mk("DisplayedUnitsY", units_y.into())); }
        }
        0x0406 if data.len() >= 4 => {
            // JPEG_Quality (from Perl Photoshop::JPEG_Quality)
            let quality = i16::from_be_bytes([data[0], data[1]]);
            tags.push(mk("PhotoshopQuality", (quality + 4).to_string()));
            let format = i16::from_be_bytes([data[2], data[3]]);
            let fmt_str = match format { 0 => "Standard", 1 => "Optimized", 0x101 => "Progressive", _ => "" };
            if !fmt_str.is_empty() { tags.push(mk("PhotoshopFormat", fmt_str.into())); }
        }
        0x040D if data.len() >= 4 => {
            // GlobalAngle
            let angle = i32::from_be_bytes([data[0], data[1], data[2], data[3]]);
            tags.push(mk("GlobalAngle", angle.to_string()));
        }
        0x0419 if data.len() >= 4 => {
            // GlobalAltitude
            let alt = i32::from_be_bytes([data[0], data[1], data[2], data[3]]);
            tags.push(mk("GlobalAltitude", alt.to_string()));
        }
        0x041A if data.len() >= 28 => {
            // SliceInfo (from Perl Photoshop::SliceInfo)
            // Offset 20: SlicesGroupName (var_ustr32 = len(4) + UTF-16 string)
            if data.len() > 24 {
                let name_len = u32::from_be_bytes([data[20], data[21], data[22], data[23]]) as usize;
                if 24 + name_len * 2 <= data.len() {
                    let units: Vec<u16> = data[24..24 + name_len * 2].chunks_exact(2)
                        .map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
                    let name = String::from_utf16_lossy(&units).trim_end_matches('\0').to_string();
                    if !name.is_empty() { tags.push(mk("SlicesGroupName", name)); }
                }
                let num_off = 24 + name_len * 2;
                if num_off + 4 <= data.len() {
                    let num = u32::from_be_bytes([data[num_off], data[num_off+1], data[num_off+2], data[num_off+3]]);
                    tags.push(mk("NumSlices", num.to_string()));
                }
            }
        }
        0x0421 if data.len() >= 5 => {
            // VersionInfo (from Perl Photoshop::VersionInfo)
            let has_merged = if data[4] != 0 { "Yes" } else { "No" };
            tags.push(mk("HasRealMergedData", has_merged.into()));
            // WriterName at offset 5 (var_ustr32)
            if data.len() > 9 {
                let wname_len = u32::from_be_bytes([data[5], data[6], data[7], data[8]]) as usize;
                if 9 + wname_len * 2 <= data.len() {
                    let units: Vec<u16> = data[9..9 + wname_len * 2].chunks_exact(2)
                        .map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
                    let wname = String::from_utf16_lossy(&units).trim_end_matches('\0').to_string();
                    if !wname.is_empty() { tags.push(mk("WriterName", wname)); }
                    let rname_off = 9 + wname_len * 2;
                    if rname_off + 4 <= data.len() {
                        let rname_len = u32::from_be_bytes([data[rname_off], data[rname_off+1], data[rname_off+2], data[rname_off+3]]) as usize;
                        if rname_off + 4 + rname_len * 2 <= data.len() {
                            let units: Vec<u16> = data[rname_off+4..rname_off+4+rname_len*2].chunks_exact(2)
                                .map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
                            let rname = String::from_utf16_lossy(&units).trim_end_matches('\0').to_string();
                            if !rname.is_empty() { tags.push(mk("ReaderName", rname)); }
                        }
                    }
                }
            }
        }
        0x0426 if data.len() >= 14 => {
            // PrintScaleInfo (from Perl Photoshop::PrintScaleInfo)
            let style = match u16::from_be_bytes([data[0], data[1]]) {
                0 => "Centered", 1 => "Size to Fit", 2 => "User Defined", _ => "",
            };
            if !style.is_empty() { tags.push(mk("PrintStyle", style.into())); }
            let x = f32::from_be_bytes([data[2], data[3], data[4], data[5]]);
            let y = f32::from_be_bytes([data[6], data[7], data[8], data[9]]);
            tags.push(mk("PrintPosition", format!("{} {}", x, y)));
            let scale = f32::from_be_bytes([data[10], data[11], data[12], data[13]]);
            tags.push(mk("PrintScale", format!("{}", scale)));
        }
        0x041E if data.len() >= 4 => {
            // URLList (from Perl Photoshop::URLList)
            let count = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
            // Perl always emits URL_List (even when empty)
            tags.push(mk("URL_List", String::new()));
            let mut upos = 4;
            for _ in 0..count.min(20) {
                if upos + 12 > data.len() { break; }
                upos += 8;
                let slen = u32::from_be_bytes([data[upos], data[upos+1], data[upos+2], data[upos+3]]) as usize;
                upos += 4;
                if upos + slen * 2 > data.len() { break; }
                let units: Vec<u16> = data[upos..upos+slen*2].chunks_exact(2)
                    .map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
                let url = String::from_utf16_lossy(&units).trim_end_matches('\0').to_string();
                if !url.is_empty() { tags.push(mk("URL", url)); }
                upos += slen * 2;
            }
        }
        _ => {}
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
        0x0425 => {
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

// ── APP12: Ducky and PictureInfo ──────────────────────────────────────────────

/// Process APP12 Ducky segment (Photoshop "Save for Web").
/// Format: 5-byte "Ducky" header (already stripped) followed by TLV records:
///   2 bytes: tag (big-endian), 2 bytes: length, N bytes: value
/// Tag 0 = end, 1 = Quality (4-byte BE u32), 2 = Comment (4-byte count + UTF-16 BE),
/// 3 = Copyright (4-byte count + UTF-16 BE).
fn process_ducky(data: &[u8]) -> Vec<crate::tag::Tag> {
    let mut tags = Vec::new();
    let mut pos = 0;
    let mk = |name: &str, val: String, family2: &str| -> crate::tag::Tag {
        crate::tag::Tag {
            id: crate::tag::TagId::Text(name.into()),
            name: name.into(),
            description: name.into(),
            group: crate::tag::TagGroup {
                family0: "Ducky".into(),
                family1: "Ducky".into(),
                family2: family2.into(),
            },
            raw_value: crate::value::Value::String(val.clone()),
            print_value: val,
            priority: 0,
        }
    };
    while pos + 4 <= data.len() {
        let tag = u16::from_be_bytes([data[pos], data[pos + 1]]);
        let len = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
        pos += 4;
        if tag == 0 {
            break;
        }
        if pos + len > data.len() {
            break;
        }
        let val_bytes = &data[pos..pos + len];
        pos += len;
        match tag {
            1 => {
                // Quality: 4-byte big-endian integer → "N%"
                if val_bytes.len() >= 4 {
                    let q = u32::from_be_bytes([val_bytes[0], val_bytes[1], val_bytes[2], val_bytes[3]]);
                    tags.push(mk("Quality", format!("{}%", q), "Image"));
                }
            }
            2 => {
                // Comment: 4-byte char count + UTF-16 BE string
                if val_bytes.len() >= 4 {
                    let s = decode_utf16be(&val_bytes[4..]);
                    if !s.is_empty() {
                        tags.push(mk("Comment", s, "Image"));
                    }
                }
            }
            3 => {
                // Copyright: 4-byte char count + UTF-16 BE string
                if val_bytes.len() >= 4 {
                    let s = decode_utf16be(&val_bytes[4..]);
                    if !s.is_empty() {
                        tags.push(mk("Copyright", s, "Author"));
                    }
                }
            }
            _ => {}
        }
    }
    tags
}

/// Decode a UTF-16 big-endian byte slice to a Rust String (null-terminated).
fn decode_utf16be(bytes: &[u8]) -> String {
    let words: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .take_while(|&w| w != 0)
        .collect();
    String::from_utf16_lossy(&words).to_string()
}

/// Convert an APP12 raw tag key to a proper tag name following ExifTool's MakeTagName logic:
/// 1. Remove illegal characters (keep only [-_a-zA-Z0-9])
/// 2. ucfirst (capitalize first letter)
/// 3. Prefix with "Tag" if length < 2 or starts with [-0-9]
fn make_app12_tag_name(raw: &str) -> String {
    let cleaned: String = raw.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if cleaned.is_empty() {
        return String::new();
    }
    let mut name = String::new();
    let mut chars = cleaned.chars();
    if let Some(first) = chars.next() {
        name.push(first.to_ascii_uppercase());
        name.extend(chars);
    }
    if name.len() < 2 || name.starts_with(|c: char| c.is_ascii_digit() || c == '-') {
        name = format!("Tag{}", name);
    }
    name
}

/// Determine group2 for an APP12 tag given the current section name.
fn app12_group2(tag_name: &str, section: &str) -> &'static str {
    match tag_name {
        "CameraType" | "SerialNumber" | "Version" | "ID" => "Camera",
        _ if section.to_ascii_lowercase().contains("camera") => "Camera",
        _ => "Image",
    }
}

/// Process APP12 PictureInfo segment (text key=value format from Agfa/Olympus cameras).
///
/// Format: ASCII text with optional section headers like "[picture info]" and
/// key=value pairs separated by CR/LF or NUL bytes.
fn process_app12_picture_info(data: &[u8]) -> Vec<crate::tag::Tag> {
    let text = String::from_utf8_lossy(data);
    let mut tags = Vec::new();
    let mut section = String::new();

    for line in text.split(|c| c == '\r' || c == '\n' || c == '\0') {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Section header like "[picture info]"
        if line.starts_with('[') {
            if let Some(end) = line.find(']') {
                section = line[1..end].to_string();
            }
            continue;
        }
        parse_app12_kv_line(line, &section, &mut tags);
    }

    tags
}

/// Parse a single line of APP12 text for key=value pairs (possibly multiple per line,
/// separated by spaces when two key= tokens appear on the same line).
fn parse_app12_kv_line(line: &str, section: &str, tags: &mut Vec<crate::tag::Tag>) {
    let mut remaining = line;
    loop {
        let eq_pos = match app12_find_key_start(remaining) {
            Some(p) => p,
            None => break,
        };
        let kv = &remaining[eq_pos..];
        let eq = match kv.find('=') {
            Some(p) => p,
            None => break,
        };
        let raw_key = &kv[..eq];
        let after_eq = &kv[eq + 1..];

        // Value ends where the next "key=" starts, or at end of string
        let val_end = app12_find_key_start(after_eq).unwrap_or(after_eq.len());
        let raw_val = after_eq[..val_end].trim_end();

        remaining = &after_eq[val_end..];

        if !raw_key.is_empty() && !raw_val.is_empty() {
            emit_app12_tag(raw_key, raw_val, section, tags);
        }
    }
}

/// Find the byte offset of the start of the next "key=" token in `s`.
/// A key is [a-zA-Z0-9_#-]+ immediately followed by '='.
fn app12_find_key_start(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        if app12_is_key_char(bytes[i]) {
            let start = i;
            while i < len && app12_is_key_char(bytes[i]) {
                i += 1;
            }
            if i < len && bytes[i] == b'=' {
                return Some(start);
            }
        } else {
            i += 1;
        }
    }
    None
}

#[inline]
fn app12_is_key_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'#' || b == b'-'
}

/// Apply tag-specific transformations (print conversions) and push the tag.
fn emit_app12_tag(raw_key: &str, raw_val: &str, section: &str, tags: &mut Vec<crate::tag::Tag>) {
    let (tag_name, print_val, group2): (String, String, &str) = match raw_key {
        // Shutter (microseconds) → ExposureTime formatted as fraction
        "Shutter" | "shtr" => {
            let micros: f64 = raw_val.parse().unwrap_or(0.0);
            let secs = micros * 1e-6_f64;
            ("ExposureTime".to_string(), app12_format_exposure_time(secs), "Image")
        }
        // Type → CameraType
        "Type" => ("CameraType".to_string(), raw_val.to_string(), "Camera"),
        // Serial# → SerialNumber
        "Serial#" => ("SerialNumber".to_string(), raw_val.to_string(), "Camera"),
        // Macro: 0→Off, 1→On
        "Macro" => {
            let print = match raw_val { "0" => "Off", "1" => "On", _ => raw_val };
            ("Macro".to_string(), print.to_string(), "Image")
        }
        // Flash: 0→Off, 1→On
        "Flash" => {
            let print = match raw_val { "0" => "Off", "1" => "On", _ => raw_val };
            ("Flash".to_string(), print.to_string(), "Image")
        }
        // FNumber: strip leading alpha chars (e.g. "F11" → "11.0")
        "FNumber" => {
            let stripped: String = raw_val.chars().skip_while(|c| !c.is_ascii_digit()).collect();
            let print = stripped.parse::<f64>().map(|v| format!("{:.1}", v)).unwrap_or(stripped);
            ("FNumber".to_string(), print, "Image")
        }
        // TimeDate → DateTimeOriginal (unix timestamp)
        "TimeDate" => {
            let unix: i64 = raw_val.parse().unwrap_or(0);
            ("DateTimeOriginal".to_string(), app12_unix_to_datetime(unix), "Time")
        }
        // ExpBias → ExposureCompensation
        "ExpBias" => ("ExposureCompensation".to_string(), raw_val.to_string(), "Image"),
        // FWare → FirmwareVersion
        "FWare" => ("FirmwareVersion".to_string(), raw_val.to_string(), "Camera"),
        // Ytarget → YTarget
        "Ytarget" => ("YTarget".to_string(), raw_val.to_string(), "Image"),
        // ylevel → YLevel
        "ylevel" => ("YLevel".to_string(), raw_val.to_string(), "Image"),
        // ImageSize: replace '-' with 'x'
        "ImageSize" => {
            ("ImageSize".to_string(), raw_val.replace('-', "x"), "Image")
        }
        // All other tags: apply MakeTagName logic
        _ => {
            let name = make_app12_tag_name(raw_key);
            if name.is_empty() {
                return;
            }
            let g2 = app12_group2(&name, section);
            (name, raw_val.to_string(), g2)
        }
    };

    tags.push(crate::tag::Tag {
        id: crate::tag::TagId::Text(tag_name.clone()),
        name: tag_name.clone(),
        description: tag_name,
        group: crate::tag::TagGroup {
            family0: "APP12".into(),
            family1: "APP12".into(),
            family2: group2.into(),
        },
        raw_value: crate::value::Value::String(raw_val.to_string()),
        print_value: print_val,
        priority: 0,
    });
}

/// Format exposure time in seconds as a fraction string (e.g. "1/155").
fn app12_format_exposure_time(secs: f64) -> String {
    if secs <= 0.0 {
        return "0".to_string();
    }
    if secs >= 1.0 {
        return format!("{:.0}", secs);
    }
    let denom = (1.0 / secs).round() as u64;
    format!("1/{}", denom)
}

/// Convert Unix timestamp to ExifTool datetime string "YYYY:MM:DD HH:MM:SS".
fn app12_unix_to_datetime(unix: i64) -> String {
    let secs_per_day = 86400i64;
    let time_of_day = unix.rem_euclid(secs_per_day);
    let days = unix.div_euclid(secs_per_day);
    let h = time_of_day / 3600;
    let mi = (time_of_day % 3600) / 60;
    let s = time_of_day % 60;
    let (year, month, day) = app12_days_to_ymd(days);
    format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}", year, month, day, h, mi, s)
}

/// Civil calendar: days since Unix epoch (1970-01-01) → (year, month, day).
fn app12_days_to_ymd(days: i64) -> (i64, u32, u32) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    (y, mo as u32, d as u32)
}

// ── MPF (Multi-Picture Format) parser ─────────────────────────────────────────

/// Parse MPF APP2 segment (CIPA DC-007 Multi-Picture Format).
///
/// `seg_data`  — APP2 segment payload (starts with "MPF\0")
/// `jpeg_data` — full JPEG file bytes (needed for PreviewImage extraction)
///
/// MPImageStart offsets in MPF entries are relative to the start of the TIFF
/// header within the MPF block (= offset 4 within seg_data).
fn parse_mpf(seg_data: &[u8], jpeg_data: &[u8]) -> Vec<crate::tag::Tag> {
    parse_mpf_inner(seg_data, jpeg_data).unwrap_or_default()
}

fn parse_mpf_inner(
    seg_data: &[u8],
    jpeg_data: &[u8],
) -> Option<Vec<crate::tag::Tag>> {
    let mut tags = Vec::new();

    // "MPF\0" is 4 bytes; the TIFF-like block follows immediately.
    if seg_data.len() < 8 {
        return None;
    }
    let mpf = &seg_data[4..]; // TIFF block; all IFD offsets are relative to this slice

    // MPImageStart offsets within MP Entries are relative to the start of the
    // TIFF block (DC-007 §5.2.3.3.3).  Compute that position in the full JPEG.
    let tiff_base = seg_data.as_ptr() as usize - jpeg_data.as_ptr() as usize + 4;

    // Byte order mark
    let big_endian = match (mpf.get(0)?, mpf.get(1)?) {
        (b'M', b'M') => true,
        (b'I', b'I') => false,
        _ => return None,
    };

    let ru16 = |data: &[u8], off: usize| -> Option<u16> {
        let b = data.get(off..off + 2)?;
        Some(if big_endian {
            u16::from_be_bytes([b[0], b[1]])
        } else {
            u16::from_le_bytes([b[0], b[1]])
        })
    };
    let ru32 = |data: &[u8], off: usize| -> Option<u32> {
        let b = data.get(off..off + 4)?;
        Some(if big_endian {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        })
    };

    let magic = ru16(mpf, 2)?;
    if magic != 42 {
        return None;
    }
    let ifd0_off = ru32(mpf, 4)? as usize;

    // Use family1 = "MPF" so that our `-G` output matches Perl's `-G` (family0).
    // Perl's family0 and family1 for MPF IFD tags are "MPF" and "MPF0" respectively.
    // Our `-G` flag displays family1, so we set family1 = "MPF" here.
    let mk_ifd_tag = |name: &str, raw: crate::value::Value, print: String| crate::tag::Tag {
        id: crate::tag::TagId::Text(name.into()),
        name: name.into(),
        description: name.into(),
        group: crate::tag::TagGroup {
            family0: "MPF".into(),
            family1: "MPF".into(),
            family2: "Image".into(),
        },
        raw_value: raw,
        print_value: print,
        priority: 0,
    };

    // --- Parse IFD0 entries ---
    let entry_count = ru16(mpf, ifd0_off)? as usize;

    let mut mp_list_offset: usize = 0;
    let mut mp_list_byte_count: usize = 0;

    for i in 0..entry_count {
        let eoff = ifd0_off + 2 + i * 12;
        if eoff + 12 > mpf.len() {
            break;
        }
        let tag_id = ru16(mpf, eoff)?;
        let count  = ru32(mpf, eoff + 4)? as usize;
        let val32  = ru32(mpf, eoff + 8)?;

        match tag_id {
            0xb000 => {
                // MPFVersion — 4 UNDEFINED bytes stored inline
                let bytes = mpf.get(eoff + 8..eoff + 12).unwrap_or(&[]);
                let ver = String::from_utf8_lossy(bytes).into_owned();
                tags.push(mk_ifd_tag(
                    "MPFVersion",
                    crate::value::Value::String(ver.clone()),
                    ver,
                ));
            }
            0xb001 => {
                // NumberOfImages — LONG (int32u)
                tags.push(mk_ifd_tag(
                    "NumberOfImages",
                    crate::value::Value::U32(val32),
                    val32.to_string(),
                ));
            }
            0xb002 => {
                // MPImageList — UNDEFINED blob; 16 bytes per MP Entry
                // val32 is offset within the mpf TIFF block
                let off = val32 as usize;
                if off + count <= mpf.len() {
                    mp_list_offset = off;
                    mp_list_byte_count = count;
                }
            }
            _ => {} // TotalFrames, pan/stereo tags, etc. — ignore for now
        }
    }

    // --- Parse MP Entries (16 bytes each) ---
    let num_entries = mp_list_byte_count / 16;
    for idx in 0..num_entries {
        let eoff = mp_list_offset + idx * 16;
        if eoff + 16 > mpf.len() {
            break;
        }

        let attr    = ru32(mpf, eoff)?;
        let img_len = ru32(mpf, eoff + 4)?;
        let img_off = ru32(mpf, eoff + 8)?;
        let dep1    = ru16(mpf, eoff + 12)?;
        let dep2    = ru16(mpf, eoff + 14)?;

        // The first MP entry with MPImageStart == 0 is the primary (current) image.
        // Perl ExifTool does not emit individual tags for it — only for embedded images.
        if img_off == 0 && idx == 0 {
            continue;
        }

        // Bit-field extraction from the 32-bit attribute word
        let flags_raw = (attr >> 27) & 0x1F; // bits 31..27
        let fmt_raw   = (attr >> 24) & 0x07; // bits 26..24
        let type_raw  =  attr        & 0x00FF_FFFF; // bits 23..0

        // Use family1 = "MPF" so that our `-G` output matches Perl's `-G` (family0 = "MPF").
        let mk = |name: &str, raw: crate::value::Value, print: String| crate::tag::Tag {
            id: crate::tag::TagId::Text(name.into()),
            name: name.into(),
            description: name.into(),
            group: crate::tag::TagGroup {
                family0: "MPF".into(),
                family1: "MPF".into(),
                family2: "Image".into(),
            },
            raw_value: raw,
            print_value: print,
            priority: 0,
        };

        // MPImageFlags — bitmask of bits 2,3,4 within flags_raw
        let flags_print = {
            let mut parts: Vec<&str> = Vec::new();
            if flags_raw & (1 << 2) != 0 { parts.push("Representative image"); }
            if flags_raw & (1 << 3) != 0 { parts.push("Dependent child image"); }
            if flags_raw & (1 << 4) != 0 { parts.push("Dependent parent image"); }
            if parts.is_empty() {
                flags_raw.to_string()
            } else {
                parts.join(", ")
            }
        };
        tags.push(mk("MPImageFlags", crate::value::Value::U32(flags_raw), flags_print));

        // MPImageFormat
        let fmt_print = match fmt_raw {
            0 => "JPEG".to_string(),
            _ => fmt_raw.to_string(),
        };
        tags.push(mk("MPImageFormat", crate::value::Value::U32(fmt_raw), fmt_print));

        // MPImageType
        let type_print = match type_raw {
            0x000000 => "Undefined".to_string(),
            0x010001 => "Large Thumbnail (VGA equivalent)".to_string(),
            0x010002 => "Large Thumbnail (full HD equivalent)".to_string(),
            0x010003 => "Large Thumbnail (4K equivalent)".to_string(),
            0x010004 => "Large Thumbnail (8K equivalent)".to_string(),
            0x010005 => "Large Thumbnail (16K equivalent)".to_string(),
            0x020001 => "Multi-frame Panorama".to_string(),
            0x020002 => "Multi-frame Disparity".to_string(),
            0x020003 => "Multi-angle".to_string(),
            0x030000 => "Baseline MP Primary Image".to_string(),
            0x040000 => "Original Preservation Image".to_string(),
            0x050000 => "Gain Map Image".to_string(),
            _ => format!("0x{:06X}", type_raw),
        };
        tags.push(mk("MPImageType", crate::value::Value::U32(type_raw), type_print));

        // MPImageLength
        tags.push(mk(
            "MPImageLength",
            crate::value::Value::U32(img_len),
            img_len.to_string(),
        ));

        // MPImageStart — offset in the MPEntry is relative to the start of the
        // TIFF block within the MPF segment (tiff_base).
        let abs_start = tiff_base as u64 + img_off as u64;
        tags.push(mk(
            "MPImageStart",
            crate::value::Value::U32(abs_start as u32),
            abs_start.to_string(),
        ));

        // DependentImage1EntryNumber
        tags.push(mk(
            "DependentImage1EntryNumber",
            crate::value::Value::U16(dep1),
            dep1.to_string(),
        ));

        // DependentImage2EntryNumber
        tags.push(mk(
            "DependentImage2EntryNumber",
            crate::value::Value::U16(dep2),
            dep2.to_string(),
        ));

        // PreviewImage — emit for "Large Thumbnail" type images (high nibble of type == 0x01).
        // Always create the tag (even when the file is truncated) to match Perl behavior.
        if (type_raw & 0x0F_0000) == 0x01_0000 && img_len > 0 {
            let start = tiff_base + img_off as usize;
            let end   = start + img_len as usize;
            // Read available data (may be empty if file is truncated).
            let preview = if start < jpeg_data.len() {
                jpeg_data[start..end.min(jpeg_data.len())].to_vec()
            } else {
                Vec::new()
            };
            let print = format!(
                "(Binary data {} bytes, use -b option to extract)",
                img_len
            );
            tags.push(crate::tag::Tag {
                id: crate::tag::TagId::Text("PreviewImage".into()),
                name: "PreviewImage".into(),
                description: "Preview Image".into(),
                group: crate::tag::TagGroup {
                    family0: "MPF".into(),
                    family1: "MPF".into(),
                    family2: "Image".into(),
                },
                raw_value: crate::value::Value::Binary(preview),
                print_value: print,
                priority: 0,
            });
        }
    }

    Some(tags)
}

// ── Kodak APP3 Meta IFD ───────────────────────────────────────────────────────

/// Parse Kodak APP3 "Meta" IFD from a TIFF-like structure (ref Kodak.pm).
///
/// The Meta segment payload (after the 6-byte "Meta\0\0" header) starts with a
/// standard TIFF header (II/MM + magic 42 + IFD0 offset), followed by an IFD
/// using Kodak-specific tag IDs in the 0xC350–0xC46E range.
fn parse_meta_ifd(data: &[u8]) -> Vec<crate::tag::Tag> {
    use crate::metadata::exif::{parse_tiff_header, ByteOrderMark};

    let mut tags = Vec::new();

    let header = match parse_tiff_header(data) {
        Ok(h) => h,
        Err(_) => return tags,
    };

    let ifd_offset = header.ifd0_offset as usize;
    if ifd_offset + 2 > data.len() {
        return tags;
    }

    let read_u16 = |d: &[u8], off: usize| -> u16 {
        if off + 2 > d.len() { return 0; }
        match header.byte_order {
            ByteOrderMark::LittleEndian => u16::from_le_bytes([d[off], d[off+1]]),
            ByteOrderMark::BigEndian => u16::from_be_bytes([d[off], d[off+1]]),
        }
    };
    let read_u32 = |d: &[u8], off: usize| -> u32 {
        if off + 4 > d.len() { return 0; }
        match header.byte_order {
            ByteOrderMark::LittleEndian => u32::from_le_bytes([d[off], d[off+1], d[off+2], d[off+3]]),
            ByteOrderMark::BigEndian => u32::from_be_bytes([d[off], d[off+1], d[off+2], d[off+3]]),
        }
    };

    let entry_count = read_u16(data, ifd_offset) as usize;
    let entries_start = ifd_offset + 2;
    let entry_count = entry_count.min((data.len().saturating_sub(entries_start)) / 12);

    for i in 0..entry_count {
        let eoff = entries_start + i * 12;
        if eoff + 12 > data.len() { break; }

        let tag_id = read_u16(data, eoff);
        let data_type = read_u16(data, eoff + 2);
        let count = read_u32(data, eoff + 4);

        let elem_size: usize = match data_type {
            1 | 2 | 6 | 7 => 1,
            3 | 8 => 2,
            4 | 9 | 11 | 13 => 4,
            5 | 10 | 12 => 8,
            _ => continue,
        };
        let total_size = elem_size * count as usize;

        let val_data: &[u8] = if total_size <= 4 {
            let end = (eoff + 8 + total_size).min(eoff + 12);
            &data[eoff + 8..end]
        } else {
            let offset = read_u32(data, eoff + 8) as usize;
            if offset + total_size > data.len() { continue; }
            &data[offset..offset + total_size]
        };

        let name: &str = match tag_id {
            0xC350 => "FilmProductCode",
            0xC351 => "ImageSourceEK",
            0xC352 => "CaptureConditionsPAR",
            0xC353 => "CameraOwner",
            0xC354 => "SerialNumber",
            0xC355 => "UserSelectGroupTitle",
            0xC356 => "DealerIDNumber",
            0xC357 => "CaptureDeviceFID",
            0xC358 => "EnvelopeNumber",
            0xC359 => "FrameNumber",
            0xC35A => "FilmCategory",
            0xC35B => "FilmGencode",
            0xC35C => "ModelAndVersion",
            0xC35D => "FilmSize",
            0xC35E => "SBA_RGBShifts",
            0xC35F => "SBAInputImageColorspace",
            0xC360 => "SBAInputImageBitDepth",
            0xC361 => "SBAExposureRecord",
            0xC362 => "UserAdjSBA_RGBShifts",
            0xC363 => "ImageRotationStatus",
            0xC364 => "RollGuidElements",
            0xC365 => "MetadataNumber",
            0xC366 => "EditTagArray",
            0xC367 => "Magnification",
            0xC36C => "NativeXResolution",
            0xC36D => "NativeYResolution",
            0xC36E => "KodakEffectsIFD",
            0xC36F => "KodakBordersIFD",
            0xC37A => "NativeResolutionUnit",
            0xC418 => "SourceImageDirectory",
            0xC419 => "SourceImageFileName",
            0xC41A => "SourceImageVolumeName",
            0xC46C => "PrintQuality",
            0xC46E => "ImagePrintStatus",
            _ => continue,
        };

        // Skip sub-IFD pointer tags
        if name == "KodakEffectsIFD" || name == "KodakBordersIFD" {
            continue;
        }

        let is_binary = matches!(name, "SBAExposureRecord" | "UserAdjSBA_RGBShifts");

        let print_value = if is_binary {
            format!("(Binary data {} bytes, use -b option to extract)", total_size)
        } else {
            meta_ifd_value_string(data_type, count, val_data, header.byte_order)
        };

        tags.push(crate::tag::Tag {
            id: crate::tag::TagId::Numeric(tag_id),
            name: name.into(),
            description: name.into(),
            group: crate::tag::TagGroup {
                family0: "Meta".into(),
                family1: "MetaIFD".into(),
                family2: "Image".into(),
            },
            raw_value: crate::value::Value::String(print_value.clone()),
            print_value,
            priority: 0,
        });
    }

    tags
}

/// Format Meta IFD entry value as a display string.
fn meta_ifd_value_string(
    data_type: u16,
    count: u32,
    val_data: &[u8],
    byte_order: crate::metadata::exif::ByteOrderMark,
) -> String {
    use crate::metadata::exif::ByteOrderMark;

    let ru16 = |off: usize| -> u16 {
        if off + 2 > val_data.len() { return 0; }
        match byte_order {
            ByteOrderMark::LittleEndian => u16::from_le_bytes([val_data[off], val_data[off+1]]),
            ByteOrderMark::BigEndian => u16::from_be_bytes([val_data[off], val_data[off+1]]),
        }
    };
    let _ri16 = |off: usize| -> i16 { ru16(off) as i16 };
    let ru32 = |off: usize| -> u32 {
        if off + 4 > val_data.len() { return 0; }
        match byte_order {
            ByteOrderMark::LittleEndian => u32::from_le_bytes([val_data[off], val_data[off+1], val_data[off+2], val_data[off+3]]),
            ByteOrderMark::BigEndian => u32::from_be_bytes([val_data[off], val_data[off+1], val_data[off+2], val_data[off+3]]),
        }
    };

    match data_type {
        1 => { // BYTE (uint8)
            let n = count as usize;
            if n == 1 { val_data[0].to_string() }
            else { val_data[..n.min(val_data.len())].iter().map(|b| b.to_string()).collect::<Vec<_>>().join(" ") }
        }
        2 => { // ASCII
            String::from_utf8_lossy(val_data).trim_end_matches('\0').to_string()
        }
        3 => { // SHORT (uint16)
            let n = count as usize;
            if n == 1 { ru16(0).to_string() }
            else { (0..n).map(|i| ru16(i * 2).to_string()).collect::<Vec<_>>().join(" ") }
        }
        4 | 13 => { // LONG (uint32)
            let n = count as usize;
            if n == 1 { ru32(0).to_string() }
            else { (0..n).map(|i| ru32(i * 4).to_string()).collect::<Vec<_>>().join(" ") }
        }
        5 => { // RATIONAL
            let n = count as usize;
            (0..n).map(|i| {
                let num = ru32(i * 8);
                let den = ru32(i * 8 + 4);
                if den == 0 { "0".into() } else { format!("{}", num as f64 / den as f64) }
            }).collect::<Vec<_>>().join(" ")
        }
        7 => { // UNDEFINED — render as ASCII if printable
            let s = String::from_utf8_lossy(val_data);
            let trimmed = s.trim_end_matches('\0');
            if trimmed.chars().all(|c| c.is_ascii_graphic() || c == ' ') {
                trimmed.to_string()
            } else {
                val_data.iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().concat()
            }
        }
        8 => { // SSHORT
            let n = count as usize;
            if n == 1 { (ru16(0) as i16).to_string() }
            else { (0..n).map(|i| (ru16(i * 2) as i16).to_string()).collect::<Vec<_>>().join(" ") }
        }
        9 => { // SLONG
            let n = count as usize;
            if n == 1 { (ru32(0) as i32).to_string() }
            else { (0..n).map(|i| (ru32(i * 4) as i32).to_string()).collect::<Vec<_>>().join(" ") }
        }
        _ => String::new(),
    }
}

// ── Canon VRD trailer ─────────────────────────────────────────────────────────

/// Parse Canon VRD trailer (from Perl CanonVRD.pm).
///
/// Layout: 0x1c-byte header + VRD blocks + 0x40-byte footer.
/// Block format: type(4 BE) + length(4 BE) + data[length], big-endian throughout.
/// Block 0xffff00f4 = EditData containing VRD1 (0x272 bytes fixed) + StampTool + VRD2.
fn parse_canon_vrd(data: &[u8], total_len: usize) -> Vec<crate::tag::Tag> {
    let mut tags = Vec::new();

    let header_len = 0x1c_usize;
    let footer_len = 0x40_usize;
    if total_len < header_len + footer_len || total_len > data.len() {
        return tags;
    }
    let footer_start = total_len - footer_len;

    let _ru16be = |d: &[u8], off: usize| -> u16 {
        if off + 2 > d.len() { return 0; }
        u16::from_be_bytes([d[off], d[off+1]])
    };
    let ru32be = |d: &[u8], off: usize| -> u32 {
        if off + 4 > d.len() { return 0; }
        u32::from_be_bytes([d[off], d[off+1], d[off+2], d[off+3]])
    };

    let mk = |name: &str, val: String| -> crate::tag::Tag {
        crate::tag::Tag {
            id: crate::tag::TagId::Text(name.into()),
            name: name.into(),
            description: name.into(),
            group: crate::tag::TagGroup {
                family0: "CanonVRD".into(),
                family1: "CanonVRD".into(),
                family2: "Image".into(),
            },
            raw_value: crate::value::Value::String(val.clone()),
            print_value: val,
            priority: 0,
        }
    };

    let mut pos = header_len;
    let blocks_end = footer_start;

    while pos + 8 <= blocks_end {
        let block_type = ru32be(data, pos);
        let block_len = ru32be(data, pos + 4) as usize;
        pos += 8;

        if pos + block_len > blocks_end {
            break;
        }

        let block = &data[pos..pos + block_len];

        if block_type == 0xffff00f4 {
            // EditData: record 0 is the VRD edit sections
            if block.len() >= 4 {
                let rec0_len = ru32be(block, 0) as usize;
                if 4 + rec0_len <= block.len() {
                    let rec0 = &block[4..4 + rec0_len];
                    // Section 0: VRD1 (fixed 0x272 bytes)
                    let vrd1_size = 0x272_usize;
                    if rec0.len() >= vrd1_size {
                        let vrd1 = &rec0[..vrd1_size];
                        tags.extend(parse_vrd1(vrd1, &mk));
                    }
                }
            }
        }

        pos += block_len;
    }

    tags
}

/// Parse VRD version 1 binary data (fixed 0x272 bytes, big-endian).
/// Ref: %Image::ExifTool::CanonVRD::Ver1
fn parse_vrd1(
    d: &[u8],
    mk: &impl Fn(&str, String) -> crate::tag::Tag,
) -> Vec<crate::tag::Tag> {
    let mut tags = Vec::new();
    if d.len() < 0x272 { return tags; }

    let ru16 = |off: usize| -> u16 {
        if off + 2 > d.len() { return 0; }
        u16::from_be_bytes([d[off], d[off+1]])
    };
    let ri16 = |off: usize| -> i16 { ru16(off) as i16 };
    let ru32 = |off: usize| -> u32 {
        if off + 4 > d.len() { return 0; }
        u32::from_be_bytes([d[off], d[off+1], d[off+2], d[off+3]])
    };
    let ri32 = |off: usize| -> i32 { ru32(off) as i32 };
    let rf32 = |off: usize| -> f32 {
        if off + 4 > d.len() { return 0.0; }
        f32::from_be_bytes([d[off], d[off+1], d[off+2], d[off+3]])
    };

    // 0x002: VRDVersion (int16u) -> "x.y.z"
    let ver_raw = ru16(0x002);
    let ver_str = {
        let s = ver_raw.to_string();
        if s.len() >= 3 {
            format!("{}.{}.{}", &s[..s.len()-2], &s[s.len()-2..s.len()-1], &s[s.len()-1..])
        } else {
            s
        }
    };
    tags.push(mk("VRDVersion", ver_str));

    // 0x006: WBAdjRGGBLevels (int16u[4])
    let wba: Vec<String> = (0..4).map(|i| ru16(0x006 + i*2).to_string()).collect();
    tags.push(mk("WBAdjRGGBLevels", wba.join(" ")));

    // 0x018: WhiteBalanceAdj (int16u)
    let wb_adj = match ru16(0x018) {
        0 => "Auto", 1 => "Daylight", 2 => "Cloudy", 3 => "Tungsten",
        4 => "Fluorescent", 5 => "Flash", 8 => "Shade", 9 => "Kelvin",
        30 => "Manual (Click)", 31 => "Shot Settings", _ => "",
    };
    if !wb_adj.is_empty() { tags.push(mk("WhiteBalanceAdj", wb_adj.into())); }

    // 0x01a: WBAdjColorTemp (int16u)
    tags.push(mk("WBAdjColorTemp", ru16(0x01a).to_string()));

    // 0x024: WBFineTuneActive (int16u)
    tags.push(mk("WBFineTuneActive", if ru16(0x024) == 0 { "No" } else { "Yes" }.into()));

    // 0x028: WBFineTuneSaturation (int16u)
    tags.push(mk("WBFineTuneSaturation", ru16(0x028).to_string()));

    // 0x02c: WBFineTuneTone (int16u)
    tags.push(mk("WBFineTuneTone", ru16(0x02c).to_string()));

    // 0x02e: RawColorAdj (int16u)
    let raw_color = match ru16(0x02e) {
        0 => "Shot Settings", 1 => "Faithful", 2 => "Custom", _ => "",
    };
    if !raw_color.is_empty() { tags.push(mk("RawColorAdj", raw_color.into())); }

    // 0x030: RawCustomSaturation (int32s)
    tags.push(mk("RawCustomSaturation", ri32(0x030).to_string()));

    // 0x034: RawCustomTone (int32s)
    tags.push(mk("RawCustomTone", ri32(0x034).to_string()));

    // 0x038: RawBrightnessAdj (int32s / 6000, %.2f)
    tags.push(mk("RawBrightnessAdj", format!("{:.2}", ri32(0x038) as f64 / 6000.0)));

    // 0x03c: ToneCurveProperty (int16u)
    let tcp = match ru16(0x03c) {
        0 => "Shot Settings", 1 => "Linear", 2 => "Custom 1", 3 => "Custom 2",
        4 => "Custom 3", 5 => "Custom 4", 6 => "Custom 5", _ => "",
    };
    if !tcp.is_empty() { tags.push(mk("ToneCurveProperty", tcp.into())); }

    // 0x07a: DynamicRangeMin (int16u)
    tags.push(mk("DynamicRangeMin", ru16(0x07a).to_string()));

    // 0x07c: DynamicRangeMax (int16u)
    tags.push(mk("DynamicRangeMax", ru16(0x07c).to_string()));

    // 0x110: ToneCurveActive (int16u)
    tags.push(mk("ToneCurveActive", if ru16(0x110) == 0 { "No" } else { "Yes" }.into()));

    // 0x113: ToneCurveMode (byte: 0=RGB, 1=Luminance)
    tags.push(mk("ToneCurveMode", if d[0x113] == 0 { "RGB" } else { "Luminance" }.into()));

    // 0x114: BrightnessAdj (int8s)
    tags.push(mk("BrightnessAdj", (d[0x114] as i8).to_string()));

    // 0x115: ContrastAdj (int8s)
    tags.push(mk("ContrastAdj", (d[0x115] as i8).to_string()));

    // 0x116: SaturationAdj (int16s)
    tags.push(mk("SaturationAdj", ri16(0x116).to_string()));

    // 0x11e: ColorToneAdj (int32s)
    tags.push(mk("ColorToneAdj", ri32(0x11e).to_string()));

    // Tone curve: int16u[21] (42 bytes).
    // vals[0] = count of control points (2..=10)
    // vals[1..2] = (x,y) for point 1, etc.
    // (From Perl CanonVRD::ToneCurvePrint)
    let tone_curve_str = |off: usize| -> String {
        if off + 42 > d.len() { return String::new(); }
        let count = ru16(off) as usize;
        if count < 2 || count > 10 { return String::new(); }
        let mut parts = Vec::new();
        for i in 0..count {
            let x = ru16(off + 2 + i * 4);
            let y = ru16(off + 2 + i * 4 + 2);
            parts.push(format!("({},{})", x, y));
        }
        parts.join(" ")
    };
    // Curve limits: int16u[4]
    let curve_limits = |off: usize| -> String {
        (0..4).map(|i| ru16(off + i*2).to_string()).collect::<Vec<_>>().join(" ")
    };

    tags.push(mk("LuminanceCurvePoints", tone_curve_str(0x126)));
    tags.push(mk("LuminanceCurveLimits", curve_limits(0x150)));
    tags.push(mk("ToneCurveInterpolation", if d[0x159] == 0 { "Curve" } else { "Straight" }.into()));
    tags.push(mk("RedCurvePoints", tone_curve_str(0x160)));
    tags.push(mk("RedCurveLimits", curve_limits(0x18a)));
    tags.push(mk("GreenCurvePoints", tone_curve_str(0x19a)));
    tags.push(mk("GreenCurveLimits", curve_limits(0x1c4)));
    tags.push(mk("BlueCurvePoints", tone_curve_str(0x1d4)));
    tags.push(mk("BlueCurveLimits", curve_limits(0x1fe)));
    tags.push(mk("RGBCurvePoints", tone_curve_str(0x20e)));
    tags.push(mk("RGBCurveLimits", curve_limits(0x238)));

    // 0x244: CropActive (int16u)
    tags.push(mk("CropActive", if ru16(0x244) == 0 { "No" } else { "Yes" }.into()));

    // 0x24a: CropWidth, 0x24c: CropHeight (int16u)
    tags.push(mk("CropWidth", ru16(0x24a).to_string()));
    tags.push(mk("CropHeight", ru16(0x24c).to_string()));

    // 0x25a: SharpnessAdj (int16u)
    tags.push(mk("SharpnessAdj", ru16(0x25a).to_string()));

    // 0x260: CropAspectRatio (int16u)
    let car = match ru16(0x260) {
        0 => "Free", 1 => "3:2", 2 => "2:3", 3 => "4:3", 4 => "3:4",
        5 => "A-size Landscape", 6 => "A-size Portrait",
        7 => "Letter-size Landscape", 8 => "Letter-size Portrait",
        9 => "4:5", 10 => "5:4", 11 => "1:1", 12 => "Circle",
        65535 => "Custom", _ => "",
    };
    if !car.is_empty() { tags.push(mk("CropAspectRatio", car.into())); }

    // 0x262: ConstrainedCropWidth (float, %.7g — removes trailing zeros)
    {
        let v = rf32(0x262);
        let s = if v == v.trunc() && v.abs() < 1e7 {
            format!("{}", v as i64)
        } else {
            // Simulate %.7g: up to 7 significant digits, no trailing zeros
            let _s7 = format!("{:.7e}", v);
            // Parse back and use the shorter representation
            format!("{:.7}", v).trim_end_matches('0').trim_end_matches('.').to_string()
        };
        tags.push(mk("ConstrainedCropWidth", s));
    }

    // 0x266: ConstrainedCropHeight (float, %.7g)
    {
        let v = rf32(0x266);
        let s = if v == v.trunc() && v.abs() < 1e7 {
            format!("{}", v as i64)
        } else {
            format!("{:.7}", v).trim_end_matches('0').trim_end_matches('.').to_string()
        };
        tags.push(mk("ConstrainedCropHeight", s));
    }

    // 0x26a: CheckMark (int16u: 0=Clear, else numeric)
    let cm = match ru16(0x26a) {
        0 => "Clear".to_string(),
        v => v.to_string(),
    };
    tags.push(mk("CheckMark", cm));

    // 0x270: WorkColorSpace (int16u)
    let wcs = match ru16(0x270) {
        0 => "sRGB", 1 => "Adobe RGB", 2 => "Wide Gamut RGB",
        3 => "Apple RGB", 4 => "ColorMatch RGB", _ => "",
    };
    if !wcs.is_empty() { tags.push(mk("WorkColorSpace", wcs.into())); }

    tags
}

// ── GoPro GPMF parser ─────────────────────────────────────────────────────────

/// GPMF data type sizes (from GoPro.pm %goProFmt).
#[allow(dead_code)]
fn gpmf_format_size(fmt: u8) -> usize {
    match fmt {
        0x62 | 0x42 => 1,          // int8s / int8u ('b'/'B')
        0x63 => 1,                  // string ('c')
        0x73 | 0x53 => 2,          // int16s / int16u ('s'/'S')
        0x6c | 0x4c | 0x66 => 4,   // int32s / int32u / float ('l'/'L'/'f')
        0x64 => 8,                  // double ('d')
        0x46 => 4,                  // 4-char ID ('F')
        0x47 | 0x55 => 16,         // 16-byte uuid / date ('G'/'U')
        0x6a | 0x4a => 8,          // int64s / int64u ('j'/'J')
        0x71 | 0x51 => 4,          // fixed32s / fixed64s ('q'/'Q')
        _ => 0,
    }
}

/// GPMF tag name lookup (from GoPro.pm %GPMF tag table).
fn gpmf_tag_name(tag: &[u8; 4]) -> &'static str {
    match tag {
        b"CASN" => "CameraSerialNumber",
        b"FMWR" => "FirmwareVersion",
        b"MINF" => "Model",
        b"DVNM" => "DeviceName",
        b"SIUN" | b"UNIT" => "",  // internal: units
        b"SCAL" => "",            // internal: scale factor
        b"TYPE" => "",            // internal: data type
        b"TSMP" => "",            // internal: total samples
        b"TICK" => "",            // internal
        b"TOCK" => "",            // internal
        b"EMPT" => "",            // empty
        b"MTRX" => "",            // internal matrix
        b"ORIN" => "",            // internal orientation
        b"ORIO" => "",            // internal orientation
        b"ACCL" => "Accelerometer",
        b"GYRO" => "Gyroscope",
        b"MAGN" => "Magnetometer",
        b"ISOE" => "ISOSpeeds",
        b"ISOG" => "ISO",
        b"SHUT" => "ShutterSpeed",
        b"WBAL" => "WhiteBalance",
        b"WRGB" => "WhiteBalanceRGB",
        b"FACE" => "FaceDetected",
        b"FCNM" => "FaceNumbers",
        b"GPSF" => "GPSMeasureMode",
        b"GPSP" => "GPSHPositioningError",
        b"GPSU" => "GPSDateTime",
        b"GPS5" => "GPSInfo",
        b"CDAT" => "CreationDate",
        b"MDAT" => "ModifyDate",
        b"EISA" => "ElectronicImageStabilization",
        b"EISE" => "ElectronicStabilizationOn",
        b"YAVG" => "AverageY",
        b"HUES" => "HueCount",
        b"UNIF" => "Uniformity",
        b"SCEN" => "SceneClassification",
        b"SROT" => "SensorReadOutTime",
        b"MWET" => "WaterDetected",
        b"AALP" => "AudioLevel",
        b"APTS" => "AudioPTS",
        b"MUID" => "MediaUniqueID",
        b"EXPT" => "ExposureType",
        // GoPro Settings tags (from Perl GoPro.pm)
        b"OREN" => "AutoRotation",
        b"DZOM" => "DigitalZoomOn",
        b"SMTR" => "SpotMeter",
        b"PRTN" => "Protune",
        b"PIMX" => "AutoISOMax",
        b"PIMN" => "AutoISOMin",
        b"RATE" => "Rate",
        b"PRES" => "PhotoResolution",
        b"PHDR" => "HDRSetting",
        b"PTEV" => "ExposureCompensation",
        b"PTCL" => "ColorMode",
        b"PTSH" => "Sharpness",
        b"ZMKF" => "ZoomModePinch",
        b"FWVS" => "OtherFirmware",
        b"KBAT" => "BatteryStatus",
        b"STMP" => "",  // internal timestamp
        b"STRM" => "",  // stream container
        b"DEVC" => "",  // device container
        b"DZMX" => "DigitalZoomAmount",
        b"DZST" => "DigitalZoom",
        b"ABSC" => "AutoBoostScore",
        b"ALLD" => "AutoLowLightDuration",
        b"AUDO" => "AudioSetting",
        b"BITR" => "BitrateSetting",
        b"MMOD" => "MediaMode",
        b"LOGS" => "HealthLogs",
        _ => "",
    }
}

/// Parse GoPro GPMF data from APP6 segment (after "GoPro\0" header).
/// GPMF is a binary record format: 4-byte tag + 1-byte format + 1-byte size + 2-byte count.
fn parse_gopro_gpmf(data: &[u8]) -> Vec<crate::tag::Tag> {
    let mut tags = Vec::new();
    parse_gpmf_records(data, &mut tags, 0);
    tags
}

/// Recursively parse GPMF records (from GoPro.pm ProcessGoPro).
fn parse_gpmf_records(data: &[u8], tags: &mut Vec<crate::tag::Tag>, depth: usize) {
    if depth > 8 { return; }
    let mut pos = 0;
    while pos + 8 <= data.len() {
        let tag_bytes: [u8; 4] = [data[pos], data[pos+1], data[pos+2], data[pos+3]];
        // Stop at null tag
        if tag_bytes == [0, 0, 0, 0] { break; }
        // Validate tag chars
        if !tag_bytes.iter().all(|&b| b == b'-' || b == b'_' || b == b' ' || b.is_ascii_alphanumeric()) {
            break;
        }
        let fmt = data[pos + 4];
        let sample_size = data[pos + 5] as usize;
        let count = u16::from_be_bytes([data[pos + 6], data[pos + 7]]) as usize;
        let size = sample_size * count;
        pos += 8;
        if pos + size > data.len() { break; }

        let val_data = &data[pos..pos + size];
        let padded = (size + 3) & !3;
        pos += padded;

        // Container (format 0): recurse into sub-records
        if fmt == 0 {
            parse_gpmf_records(val_data, tags, depth + 1);
            continue;
        }

        let name = gpmf_tag_name(&tag_bytes);
        if name.is_empty() { continue; }

        // Decode value based on format
        let val_str = if fmt == 0x63 {
            // string
            String::from_utf8_lossy(val_data).trim_end_matches('\0').to_string()
        } else if fmt == 0x55 && val_data.len() >= 16 {
            // date: "yymmddhhmmss.sss" format
            String::from_utf8_lossy(&val_data[..16]).trim_end_matches('\0').to_string()
        } else if (fmt == 0x42 || fmt == 0x62) && size == 1 {
            val_data[0].to_string()
        } else if (fmt == 0x53 || fmt == 0x73) && size >= 2 {
            if count == 1 {
                let v = if fmt == 0x73 {
                    (i16::from_be_bytes([val_data[0], val_data[1]])).to_string()
                } else {
                    u16::from_be_bytes([val_data[0], val_data[1]]).to_string()
                };
                v
            } else {
                format!("(Binary data {} bytes)", size)
            }
        } else if (fmt == 0x4c || fmt == 0x6c) && size >= 4 {
            if count == 1 {
                if fmt == 0x6c {
                    i32::from_be_bytes([val_data[0], val_data[1], val_data[2], val_data[3]]).to_string()
                } else {
                    u32::from_be_bytes([val_data[0], val_data[1], val_data[2], val_data[3]]).to_string()
                }
            } else {
                format!("(Binary data {} bytes)", size)
            }
        } else if fmt == 0x66 && size >= 4 {
            if count == 1 {
                let v = f32::from_be_bytes([val_data[0], val_data[1], val_data[2], val_data[3]]);
                format!("{}", v)
            } else {
                format!("(Binary data {} bytes)", size)
            }
        } else if fmt == 0x64 && size >= 8 {
            if count == 1 {
                let v = f64::from_be_bytes([val_data[0], val_data[1], val_data[2], val_data[3],
                    val_data[4], val_data[5], val_data[6], val_data[7]]);
                format!("{}", v)
            } else {
                format!("(Binary data {} bytes)", size)
            }
        } else if size > 256 {
            format!("(Binary data {} bytes)", size)
        } else {
            String::from_utf8_lossy(val_data).trim_end_matches('\0').to_string()
        };

        // Emit even empty values (Perl does: ExposureType with empty value)
        {
            tags.push(crate::tag::Tag {
                id: crate::tag::TagId::Text(name.into()),
                name: name.into(),
                description: name.into(),
                group: crate::tag::TagGroup {
                    family0: "APP6".into(),
                    family1: "GoPro".into(),
                    family2: "Camera".into(),
                },
                raw_value: crate::value::Value::String(val_str.clone()),
                print_value: val_str,
                priority: 0,
            });
        }
    }
}

// ── Ricoh RMETA parser ────────────────────────────────────────────────────────

/// Parse Ricoh RMETA APP5 data (after "RMETA\0" header).
/// From Perl ProcessRicohRMETA: binary directory with tag names, string values,
/// and numerical values in separate sections.
fn parse_ricoh_rmeta(data: &[u8]) -> Vec<crate::tag::Tag> {
    let mut tags = Vec::new();
    if data.len() < 20 { return tags; }

    // Byte order from first 2 bytes
    let big_endian = match (data[0], data[1]) {
        (b'M', b'M') => true,
        (b'I', b'I') => false,
        _ => true, // default
    };

    let ru16 = |off: usize| -> u16 {
        if off + 2 > data.len() { return 0; }
        if big_endian { u16::from_be_bytes([data[off], data[off+1]]) }
        else { u16::from_le_bytes([data[off], data[off+1]]) }
    };

    // RMETA segment number at offset 4
    let rmeta_num = ru16(4);
    if rmeta_num != 0 {
        // Non-zero segment: barcode or audio data; skip for now
        return tags;
    }

    // Directory start offset at offset 8
    let dir_offset = ru16(8) as usize;
    if dir_offset + 2 > data.len() { return tags; }
    let num_entries = ru16(dir_offset) as usize;
    if num_entries > 100 { return tags; }

    // Parse sections: type(2) + size(2), then data
    let mut section_tags: Vec<String> = Vec::new();
    let mut section_vals: Vec<String> = Vec::new();
    let mut section_nums: Vec<u16> = Vec::new();

    let mut spos = dir_offset + 10; // start of first section
    while spos + 4 <= data.len() {
        let sec_type = ru16(spos);
        let sec_size = ru16(spos + 2) as usize;
        if sec_size == 0 { break; }
        spos += 4;
        let actual_size = sec_size.saturating_sub(2);
        if actual_size == 0 || spos + actual_size > data.len() { break; }

        let sec_data = &data[spos..spos + actual_size];

        if sec_type == 1 {
            // Section 1: tag names (null-delimited)
            section_tags = String::from_utf8_lossy(sec_data)
                .split('\0')
                .take(num_entries + 1)
                .map(|s| s.to_string())
                .collect();
        } else if sec_type == 2 || sec_type == 18 {
            // Section 2/18: string values (null-delimited)
            section_vals = String::from_utf8_lossy(sec_data)
                .split('\0')
                .take(num_entries + 1)
                .map(|s| s.to_string())
                .collect();
        } else if sec_type == 3 {
            // Section 3: numerical values (int16u)
            for i in 0..num_entries.min(actual_size / 2) {
                section_nums.push(ru16(spos + i * 2));
            }
        }

        spos += actual_size;
    }

    let mk = |name: &str, val: String| crate::tag::Tag {
        id: crate::tag::TagId::Text(name.into()),
        name: name.into(),
        description: name.into(),
        group: crate::tag::TagGroup {
            family0: "APP5".into(),
            family1: "RMETA".into(),
            family2: "Image".into(),
        },
        raw_value: crate::value::Value::String(val.clone()),
        print_value: val,
        priority: 0,
    };

    // Combine tags and values
    for i in 0..num_entries {
        let tag = section_tags.get(i).cloned().unwrap_or_default();
        let val = section_vals.get(i).cloned().unwrap_or_default();
        let num = section_nums.get(i).copied();

        if tag.is_empty() && val.is_empty() { continue; }

        // Capitalize tag name words
        let name = if tag.is_empty() {
            "RMETA_Unknown".to_string()
        } else {
            tag.split_whitespace()
                .map(|w| {
                    let mut c = w.chars();
                    match c.next() {
                        None => String::new(),
                        Some(f) => f.to_uppercase().chain(c).collect(),
                    }
                })
                .collect::<Vec<_>>()
                .join("")
        };

        // Use string value if available, otherwise numerical
        let display = if !val.is_empty() {
            val
        } else if let Some(n) = num {
            n.to_string()
        } else {
            continue;
        };

        tags.push(mk(&name, display));
    }

    tags
}

// ── InfiRay APP3-APP9 parsers ─────────────────────────────────────────────────

/// Decode InfiRay Factory data (APP4).
fn decode_infray_factory(data: &[u8]) -> Vec<crate::tag::Tag> {
    let mut tags = Vec::new();
    if data.len() < 4 { return tags; }

    let mk = |name: &str, val: String| crate::tag::Tag {
        id: crate::tag::TagId::Text(name.into()),
        name: name.into(), description: name.into(),
        group: crate::tag::TagGroup { family0: "APP4".into(), family1: "InfiRay".into(), family2: "Image".into() },
        raw_value: crate::value::Value::String(val.clone()), print_value: val, priority: 0,
    };

    let ri16 = |off: usize| -> i16 { if off + 2 > data.len() { 0 } else { i16::from_le_bytes([data[off], data[off+1]]) } };
    let ri32 = |off: usize| -> i32 { if off + 4 > data.len() { 0 } else { i32::from_le_bytes([data[off], data[off+1], data[off+2], data[off+3]]) } };
    let ri8 = |off: usize| -> i8 { if off < data.len() { data[off] as i8 } else { 0 } };

    tags.push(mk("IJPEGTempVersion", format!("{}.{}.{}.{}", data[0], data[1], data[2], data[3])));
    if data.len() > 0x05 { tags.push(mk("FactDefEmissivity", ri8(0x04).to_string())); }
    if data.len() > 0x06 { tags.push(mk("FactDefTau", ri8(0x05).to_string())); }
    if data.len() > 0x08 { tags.push(mk("FactDefTa", ri16(0x06).to_string())); }
    if data.len() > 0x0A { tags.push(mk("FactDefTu", ri16(0x08).to_string())); }
    if data.len() > 0x0C { tags.push(mk("FactDefDist", ri16(0x0A).to_string())); }
    if data.len() > 0x10 { tags.push(mk("FactDefA0", ri32(0x0C).to_string())); }
    if data.len() > 0x14 { tags.push(mk("FactDefB0", ri32(0x10).to_string())); }
    if data.len() > 0x18 { tags.push(mk("FactDefA1", ri32(0x14).to_string())); }
    if data.len() > 0x1C { tags.push(mk("FactDefB1", ri32(0x18).to_string())); }
    if data.len() > 0x20 { tags.push(mk("FactDefP0", ri32(0x1C).to_string())); }
    if data.len() > 0x24 { tags.push(mk("FactDefP1", ri32(0x20).to_string())); }
    if data.len() > 0x28 { tags.push(mk("FactDefP2", ri32(0x24).to_string())); }
    if data.len() > 0x46 { tags.push(mk("FactRelSensorTemp", ri16(0x44).to_string())); }
    if data.len() > 0x48 { tags.push(mk("FactRelShutterTemp", ri16(0x46).to_string())); }
    if data.len() > 0x4A { tags.push(mk("FactRelLensTemp", ri16(0x48).to_string())); }
    if data.len() > 0x65 { tags.push(mk("FactStatusGain", ri8(0x64).to_string())); }
    if data.len() > 0x66 { tags.push(mk("FactStatusEnvOK", ri8(0x65).to_string())); }
    if data.len() > 0x67 { tags.push(mk("FactStatusDistOK", ri8(0x66).to_string())); }
    if data.len() > 0x68 { tags.push(mk("FactStatusTempMap", ri8(0x67).to_string())); }

    tags
}

/// Decode InfiRay Picture temperature info (APP5).
fn decode_infray_picture(data: &[u8]) -> Vec<crate::tag::Tag> {
    let mut tags = Vec::new();
    if data.len() < 4 { return tags; }

    let mk = |name: &str, val: String| crate::tag::Tag {
        id: crate::tag::TagId::Text(name.into()),
        name: name.into(), description: name.into(),
        group: crate::tag::TagGroup { family0: "APP5".into(), family1: "InfiRay".into(), family2: "Image".into() },
        raw_value: crate::value::Value::String(val.clone()), print_value: val, priority: 0,
    };

    let rf32 = |off: usize| -> f32 {
        if off + 4 > data.len() { 0.0 } else { f32::from_le_bytes([data[off], data[off+1], data[off+2], data[off+3]]) }
    };

    tags.push(mk("EnvironmentTemp", format!("{:.2} C", rf32(0x00))));
    tags.push(mk("Distance", format!("{:.2} m", rf32(0x04))));
    tags.push(mk("Emissivity", format!("{:.2}", rf32(0x08))));
    tags.push(mk("Humidity", format!("{:.1} %", rf32(0x0C) * 100.0)));
    if data.len() > 0x14 { tags.push(mk("ReferenceTemp", format!("{:.2} C", rf32(0x10)))); }
    if data.len() > 0x21 { tags.push(mk("TempUnit", data[0x20].to_string())); }
    if data.len() > 0x22 { tags.push(mk("ShowCenterTemp", data[0x21].to_string())); }
    if data.len() > 0x23 { tags.push(mk("ShowMaxTemp", data[0x22].to_string())); }
    if data.len() > 0x24 { tags.push(mk("ShowMinTemp", data[0x23].to_string())); }
    if data.len() > 0x26 {
        let count = u16::from_le_bytes([data[0x24], data[0x25]]);
        tags.push(mk("TempMeasureCount", count.to_string()));
    }

    tags
}

/// Decode InfiRay MixMode data (APP6).
fn decode_infray_mixmode(data: &[u8]) -> Vec<crate::tag::Tag> {
    let mut tags = Vec::new();
    if data.is_empty() { return tags; }

    let mk = |name: &str, val: String| crate::tag::Tag {
        id: crate::tag::TagId::Text(name.into()),
        name: name.into(), description: name.into(),
        group: crate::tag::TagGroup { family0: "APP6".into(), family1: "InfiRay".into(), family2: "Image".into() },
        raw_value: crate::value::Value::String(val.clone()), print_value: val, priority: 0,
    };

    let rf32 = |off: usize| -> f32 {
        if off + 4 > data.len() { 0.0 } else { f32::from_le_bytes([data[off], data[off+1], data[off+2], data[off+3]]) }
    };

    tags.push(mk("MixMode", data[0].to_string()));
    if data.len() > 5 { tags.push(mk("FusionIntensity", format!("{:.1} %", rf32(0x01) * 100.0))); }
    if data.len() > 9 { tags.push(mk("OffsetAdjustment", format!("{}", rf32(0x05)))); }
    // CorrectionAsix: 30 floats at offset 0x09 (from Perl InfiRay::MixMode)
    if data.len() >= 0x09 + 30 * 4 {
        let vals: Vec<String> = (0..30).map(|i| format!("{}", rf32(0x09 + i * 4))).collect();
        tags.push(mk("CorrectionAsix", vals.join(" ")));
    }

    tags
}

/// Decode InfiRay OpMode data (APP7).
fn decode_infray_opmode(data: &[u8]) -> Vec<crate::tag::Tag> {
    let mut tags = Vec::new();
    if data.is_empty() { return tags; }

    let mk = |name: &str, val: String| crate::tag::Tag {
        id: crate::tag::TagId::Text(name.into()),
        name: name.into(), description: name.into(),
        group: crate::tag::TagGroup { family0: "APP7".into(), family1: "InfiRay".into(), family2: "Image".into() },
        raw_value: crate::value::Value::String(val.clone()), print_value: val, priority: 0,
    };

    let ru32 = |off: usize| -> u32 {
        if off + 4 > data.len() { 0 } else { u32::from_le_bytes([data[off], data[off+1], data[off+2], data[off+3]]) }
    };
    let rf32 = |off: usize| -> f32 {
        if off + 4 > data.len() { 0.0 } else { f32::from_le_bytes([data[off], data[off+1], data[off+2], data[off+3]]) }
    };

    tags.push(mk("WorkingMode", data[0].to_string()));
    if data.len() > 5 { tags.push(mk("IntegralTime", ru32(0x01).to_string())); }
    if data.len() > 9 { tags.push(mk("IntegratTimeHdr", ru32(0x05).to_string())); }
    if data.len() > 0x0A { tags.push(mk("GainStable", data[0x09].to_string())); }
    if data.len() > 0x0B { tags.push(mk("TempControlEnable", data[0x0A].to_string())); }
    if data.len() > 0x0F { tags.push(mk("DeviceTemp", format!("{:.2} C", rf32(0x0B)))); }

    tags
}

/// Decode InfiRay Isothermal data (APP8).
fn decode_infray_isothermal(data: &[u8]) -> Vec<crate::tag::Tag> {
    let mut tags = Vec::new();
    if data.len() < 16 { return tags; }

    let mk = |name: &str, val: String| crate::tag::Tag {
        id: crate::tag::TagId::Text(name.into()),
        name: name.into(), description: name.into(),
        group: crate::tag::TagGroup { family0: "APP8".into(), family1: "InfiRay".into(), family2: "Image".into() },
        raw_value: crate::value::Value::String(val.clone()), print_value: val, priority: 0,
    };

    let rf32 = |off: usize| -> f32 {
        f32::from_le_bytes([data[off], data[off+1], data[off+2], data[off+3]])
    };

    tags.push(mk("IsothermalMax", format!("{}", rf32(0x00))));
    tags.push(mk("IsothermalMin", format!("{}", rf32(0x04))));
    tags.push(mk("ChromaBarMax", format!("{}", rf32(0x08))));
    tags.push(mk("ChromaBarMin", format!("{}", rf32(0x0C))));

    tags
}

/// Decode InfiRay Sensor info (APP9).
fn decode_infray_sensor(data: &[u8]) -> Vec<crate::tag::Tag> {
    let mut tags = Vec::new();
    if data.len() < 0x100 { return tags; }

    let mk = |name: &str, val: String| crate::tag::Tag {
        id: crate::tag::TagId::Text(name.into()),
        name: name.into(), description: name.into(),
        group: crate::tag::TagGroup { family0: "APP9".into(), family1: "InfiRay".into(), family2: "Image".into() },
        raw_value: crate::value::Value::String(val.clone()), print_value: val, priority: 0,
    };

    let read_str = |off: usize, len: usize| -> String {
        if off + len > data.len() { return String::new(); }
        String::from_utf8_lossy(&data[off..off+len]).trim_end_matches('\0').to_string()
    };

    let rf32 = |off: usize| -> f32 {
        if off + 4 > data.len() { 0.0 } else { f32::from_le_bytes([data[off], data[off+1], data[off+2], data[off+3]]) }
    };

    let v = read_str(0x000, 12); if !v.is_empty() { tags.push(mk("IRSensorManufacturer", v)); }
    let v = read_str(0x040, 12); if !v.is_empty() { tags.push(mk("IRSensorName", v)); }
    let v = read_str(0x080, 32); if !v.is_empty() { tags.push(mk("IRSensorPartNumber", v)); }
    let v = read_str(0x0C0, 32); if !v.is_empty() { tags.push(mk("IRSensorSerialNumber", v)); }
    if data.len() > 0x10C {
        let v = read_str(0x100, 12); if !v.is_empty() { tags.push(mk("IRSensorFirmware", v)); }
    }
    if data.len() > 0x144 { tags.push(mk("IRSensorAperture", format!("{:.2}", rf32(0x140)))); }
    if data.len() > 0x148 { tags.push(mk("IRFocalLength", format!("{:.2}", rf32(0x144)))); }
    if data.len() > 0x18C {
        let v = read_str(0x180, 12); if !v.is_empty() { tags.push(mk("VisibleSensorManufacturer", v)); }
    }
    if data.len() > 0x1CC {
        let v = read_str(0x1C0, 12); if !v.is_empty() { tags.push(mk("VisibleSensorName", v)); }
    }
    if data.len() > 0x220 {
        let v = read_str(0x200, 32); if !v.is_empty() { tags.push(mk("VisibleSensorPartNumber", v)); }
    }
    if data.len() > 0x260 {
        let v = read_str(0x240, 32); if !v.is_empty() { tags.push(mk("VisibleSensorSerialNumber", v)); }
    }
    if data.len() > 0x28C {
        let v = read_str(0x280, 12); if !v.is_empty() { tags.push(mk("VisibleSensorFirmware", v)); }
    }
    if data.len() > 0x2C4 { tags.push(mk("VisibleSensorAperture", format!("{}", rf32(0x2C0)))); }
    if data.len() > 0x2C8 { tags.push(mk("VisibleFocalLength", format!("{}", rf32(0x2C4)))); }

    tags
}

// ── FlashPix FPXR parser ──────────────────────────────────────────────────────

/// FPXR contents entry.
struct FpxrEntry {
    name: String,
    #[allow(dead_code)]
    size: u32,
    stream: Vec<u8>,
}

/// Accumulate FPXR APP2 segment data.
/// seg_data starts with "FPXR\0" followed by version(1) + type(1) + payload.
fn accumulate_fpxr(seg_data: &[u8], contents: &mut Vec<FpxrEntry>) {
    if seg_data.len() < 7 { return; }
    let seg_type = seg_data[6];

    if seg_type == 1 {
        // Contents List segment
        if seg_data.len() < 9 { return; }
        let num_entries = u16::from_be_bytes([seg_data[7], seg_data[8]]) as usize;
        let mut pos = 9;
        contents.clear();
        for _ in 0..num_entries.min(50) {
            if pos + 5 > seg_data.len() { break; }
            let size = u32::from_be_bytes([seg_data[pos], seg_data[pos+1], seg_data[pos+2], seg_data[pos+3]]);
            let _default = seg_data[pos + 4];
            pos += 5;

            // Stream name: little-endian UTF-16, starting with '/', terminated by double null
            let name_start = pos;
            let mut found_end = false;
            while pos + 2 <= seg_data.len() {
                let w = u16::from_le_bytes([seg_data[pos], seg_data[pos+1]]);
                pos += 2;
                if w == 0 { found_end = true; break; }
            }
            if !found_end { break; }

            // Decode name as little-endian UTF-16
            let name_bytes = &seg_data[name_start..pos.saturating_sub(2)];
            let units: Vec<u16> = name_bytes.chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            let mut name = String::from_utf16_lossy(&units);
            // Remove directory specification, keep only filename
            if let Some(slash_pos) = name.rfind('/') {
                name = name[slash_pos+1..].to_string();
            }

            // Read storage class ID if size == 0xffffffff
            if size == 0xFFFFFFFF {
                if pos + 16 > seg_data.len() { break; }
                pos += 16; // skip 16-byte class ID
            }

            contents.push(FpxrEntry {
                name,
                size,
                stream: Vec::new(),
            });
        }
    } else if seg_type == 2 {
        // Stream Data segment
        if seg_data.len() < 13 { return; }
        let index = u16::from_be_bytes([seg_data[7], seg_data[8]]) as usize;
        let _offset = u32::from_be_bytes([seg_data[9], seg_data[10], seg_data[11], seg_data[12]]);
        if index < contents.len() {
            let stream_data = &seg_data[13..];
            contents[index].stream.extend_from_slice(stream_data);
        }
    }
    // type 3 = Reserved, ignore
}

/// Process accumulated FPXR segments.
fn process_fpxr_segments(contents: &[FpxrEntry]) -> Vec<crate::tag::Tag> {
    let mut tags = Vec::new();

    for entry in contents {
        if entry.stream.is_empty() { continue; }
        let name = &entry.name;

        // Screen Nail stream → ScreenNail binary tag (strip 0x1c header)
        if name.contains("Screen Nail") {
            let payload = if entry.stream.len() > 0x1c { &entry.stream[0x1c..] } else { &entry.stream };
            tags.push(crate::tag::Tag {
                id: crate::tag::TagId::Text("ScreenNail".into()),
                name: "ScreenNail".into(), description: "Screen Nail".into(),
                group: crate::tag::TagGroup { family0: "FlashPix".into(), family1: "FlashPix".into(), family2: "Other".into() },
                raw_value: crate::value::Value::Binary(payload.to_vec()),
                print_value: format!("(Binary data {} bytes, use -b option to extract)", payload.len()),
                priority: 0,
            });
            continue;
        }

        // Audio Stream → AudioStream binary tag (strip 0x1c header)
        if name.contains("Audio") && name.contains("Stream") {
            let payload = if entry.stream.len() > 0x1c { &entry.stream[0x1c..] } else { &entry.stream };
            tags.push(crate::tag::Tag {
                id: crate::tag::TagId::Text("AudioStream".into()),
                name: "AudioStream".into(), description: "Audio Stream".into(),
                group: crate::tag::TagGroup { family0: "FlashPix".into(), family1: "FlashPix".into(), family2: "Other".into() },
                raw_value: crate::value::Value::Binary(payload.to_vec()),
                print_value: format!("(Binary data {} bytes, use -b option to extract)", payload.len()),
                priority: 0,
            });
            continue;
        }

        // Extension List → parse OLE property set for FlashPix extension tags
        if name.contains("Extension List") {
            tags.extend(parse_fpxr_extension_list(&entry.stream));
            continue;
        }

        // Audio Info → parse OLE property set for CodePage etc.
        if name.contains("Audio") && name.contains("Info") {
            tags.extend(parse_fpxr_audio_info(&entry.stream));
            continue;
        }
    }

    tags
}

/// Parse OLE property set from FlashPix Extension List stream.
fn parse_fpxr_extension_list(data: &[u8]) -> Vec<crate::tag::Tag> {
    parse_ole_props(data, "FlashPix", &|id| match id & 0x0000ffff {
        0x0001 => Some("ExtensionName"),
        0x0002 => Some("ExtensionClassID"),
        0x0003 => Some("ExtensionPersistence"),
        0x0004 => Some("ExtensionCreateDate"),
        0x0005 => Some("ExtensionModifyDate"),
        0x0006 => Some("CreatingApplication"),
        0x0007 => Some("ExtensionDescription"),
        0x1000 => Some("Storage-StreamPathname"),
        _ => match id {
            0x10000000 => Some("UsedExtensionNumbers"),
            _ => None,
        }
    })
}

/// Parse OLE property set from FlashPix Audio Info stream.
fn parse_fpxr_audio_info(data: &[u8]) -> Vec<crate::tag::Tag> {
    parse_ole_props(data, "FlashPix", &|id| match id {
        0x01 => Some("CodePage"),
        _ => None,
    })
}

/// Minimal OLE property set parser (from Perl FlashPix::ProcessProperties).
fn parse_ole_props<'a>(data: &[u8], family: &str, tag_map: &dyn Fn(u32) -> Option<&'a str>) -> Vec<crate::tag::Tag> {
    let mut tags = Vec::new();
    if data.len() < 28 { return tags; }
    let le = data[0] == 0xFE && data[1] == 0xFF;
    let ru32 = |off: usize| -> u32 {
        if off + 4 > data.len() { return 0; }
        if le { u32::from_le_bytes([data[off], data[off+1], data[off+2], data[off+3]]) }
        else { u32::from_be_bytes([data[off], data[off+1], data[off+2], data[off+3]]) }
    };
    let ru16 = |off: usize| -> u16 {
        if off + 2 > data.len() { return 0; }
        if le { u16::from_le_bytes([data[off], data[off+1]]) }
        else { u16::from_be_bytes([data[off], data[off+1]]) }
    };
    let section_count = ru32(24) as usize;
    if section_count == 0 || data.len() < 28 + section_count * 20 { return tags; }
    for s in 0..section_count.min(4) {
        let sec_offset = ru32(28 + 16 + s * 20) as usize;
        if sec_offset + 8 > data.len() { continue; }
        let prop_count = ru32(sec_offset + 4) as usize;
        if prop_count > 500 { continue; }
        for i in 0..prop_count.min(500) {
            let entry_off = sec_offset + 8 + i * 8;
            if entry_off + 8 > data.len() { break; }
            let prop_id = ru32(entry_off);
            let prop_offset = ru32(entry_off + 4) as usize;
            let val_off = sec_offset + prop_offset;
            if val_off + 4 > data.len() { continue; }
            let vtype = ru32(val_off) & 0xFFF;
            let tag_name = if prop_id == 1 { Some("CodePage") } else { tag_map(prop_id) };
            let tag_name: &str = match tag_name { Some(n) => n, None => continue };
            let val_str = match vtype {
                2 | 18 => { // VT_I2 / VT_UI2
                    if val_off + 6 > data.len() { continue; }
                    let v = ru16(val_off + 4) as i16;
                    if tag_name == "CodePage" {
                        match v as u16 { 1200 => "Unicode UTF-16, little endian".into(), 1252 => "Windows Latin 1".into(), _ => v.to_string() }
                    } else if tag_name == "ExtensionPersistence" {
                        match v { 0 => "Always Valid".into(), 1 => "Invalidated By Modification".into(), _ => v.to_string() }
                    } else { v.to_string() }
                }
                3 => { if val_off + 8 > data.len() { continue; } ru32(val_off + 4).to_string() }
                30 => { // VT_LPSTR
                    if val_off + 8 > data.len() { continue; }
                    let slen = ru32(val_off + 4) as usize;
                    if val_off + 8 + slen > data.len() { continue; }
                    String::from_utf8_lossy(&data[val_off+8..val_off+8+slen]).trim_end_matches('\0').to_string()
                }
                31 => { // VT_LPWSTR
                    if val_off + 8 > data.len() { continue; }
                    let chars = ru32(val_off + 4) as usize;
                    if val_off + 8 + chars * 2 > data.len() { continue; }
                    let u16s: Vec<u16> = (0..chars).map(|j| ru16(val_off + 8 + j * 2)).collect();
                    String::from_utf16_lossy(&u16s).trim_end_matches('\0').to_string()
                }
                64 => { // VT_FILETIME
                    if val_off + 12 > data.len() { continue; }
                    let lo = ru32(val_off + 4) as u64;
                    let hi = ru32(val_off + 8) as u64;
                    let ft = (hi << 32) | lo;
                    if ft == 0 { "0000:00:00 00:00:00".into() } else {
                        let secs = ft / 10_000_000;
                        let unix = secs.wrapping_sub(11644473600) as i64;
                        let s = (unix.rem_euclid(60)) as u32; let m = ((unix / 60).rem_euclid(60)) as u32;
                        let h = ((unix / 3600).rem_euclid(24)) as u32;
                        let days = unix.div_euclid(86400);
                        let d = days + 719468;
                        let era = if d >= 0 { d } else { d - 146096 } / 146097;
                        let doe = (d - era * 146097) as u32;
                        let yoe = (doe - doe/1460 + doe/36524 - doe/146096) / 365;
                        let y = yoe as i64 + era * 400;
                        let doy = doe - (365*yoe + yoe/4 - yoe/100);
                        let mp = (5*doy + 2) / 153;
                        let dd = doy - (153*mp + 2)/5 + 1;
                        let mm = if mp < 10 { mp + 3 } else { mp - 9 };
                        let y = if mm <= 2 { y + 1 } else { y };
                        format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}", y, mm, dd, h, m, s)
                    }
                }
                0x1002 | 0x1003 | 0x1012 | 0x1013 => { // VT_VECTOR|VT_I2/UI2/I4/UI4
                    if val_off + 8 > data.len() { continue; }
                    let count = ru32(val_off + 4) as usize;
                    let esz = if vtype == 0x1002 || vtype == 0x1012 { 2usize } else { 4 };
                    (0..count.min(100)).filter_map(|j| {
                        let eoff = val_off + 8 + j * esz;
                        if eoff + esz > data.len() { return None; }
                        Some(if esz == 2 { ru16(eoff).to_string() } else { ru32(eoff).to_string() })
                    }).collect::<Vec<_>>().join(", ")
                }
                72 | 65 => { // VT_CLSID (0x48=72)
                    if val_off + 20 > data.len() { continue; }
                    let d1 = ru32(val_off+4); let d2 = ru16(val_off+8); let d3 = ru16(val_off+10);
                    let d4 = &data[val_off+12..val_off+20];
                    format!("{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
                        d1, d2, d3, d4[0], d4[1], d4[2], d4[3], d4[4], d4[5], d4[6], d4[7])
                }
                _ => continue,
            };
            tags.push(crate::tag::Tag {
                id: crate::tag::TagId::Text(tag_name.into()),
                name: tag_name.into(), description: tag_name.into(),
                group: crate::tag::TagGroup { family0: family.into(), family1: family.into(), family2: "Other".into() },
                raw_value: crate::value::Value::String(val_str.clone()), print_value: val_str, priority: 0,
            });
        }
    }
    tags
}

/// Parse Qualcomm Camera Attributes data from APP7 segment.
/// Format per entry: valLen(u16 LE) + tagLen(u8) + tag(tagLen bytes) + fmt(u8) + cnt1(u16) + cnt2(u16) + value(valLen bytes)
/// Based on Perl Qualcomm.pm ProcessQualcomm.
fn parse_qualcomm(data: &[u8]) -> Vec<crate::tag::Tag> {
    let mut tags = Vec::new();
    let mut pos = 0;
    let end = data.len();

    while pos + 3 < end {
        let val_len = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        let tag_len = data[pos + 2] as usize;
        // Check bounds: 3 (header) + tag_len + 5 (fmt+cnt1+cnt2) + val_len
        if pos + 3 + tag_len + 5 + val_len > end {
            break;
        }
        let tag_bytes = &data[pos + 3..pos + 3 + tag_len];
        let tag_str = String::from_utf8_lossy(tag_bytes).to_string();
        pos += 3 + tag_len; // now at format byte
        let fmt = data[pos];
        // skip fmt(1) + cnt1(2) + cnt2(2) = 5 bytes
        pos += 5;
        let value_data = &data[pos..pos + val_len];
        pos += val_len;

        // Decode value based on format (from Perl @qualcommFormat)
        let (val_str, raw_value) = match fmt {
            0 => { // int8u
                let v = if !value_data.is_empty() { value_data[0] as u64 } else { 0 };
                (v.to_string(), crate::value::Value::U32(v as u32))
            }
            1 => { // int8s
                let v = if !value_data.is_empty() { value_data[0] as i8 as i64 } else { 0 };
                (v.to_string(), crate::value::Value::String(v.to_string()))
            }
            2 => { // int16u
                if value_data.len() >= 2 {
                    let v = u16::from_le_bytes([value_data[0], value_data[1]]);
                    (v.to_string(), crate::value::Value::U16(v))
                } else { continue; }
            }
            3 => { // int16s
                if value_data.len() >= 2 {
                    let v = i16::from_le_bytes([value_data[0], value_data[1]]);
                    (v.to_string(), crate::value::Value::String(v.to_string()))
                } else { continue; }
            }
            4 => { // int32u
                if value_data.len() >= 4 {
                    let v = u32::from_le_bytes([value_data[0], value_data[1], value_data[2], value_data[3]]);
                    (v.to_string(), crate::value::Value::U32(v))
                } else { continue; }
            }
            5 => { // int32s
                if value_data.len() >= 4 {
                    let v = i32::from_le_bytes([value_data[0], value_data[1], value_data[2], value_data[3]]);
                    (v.to_string(), crate::value::Value::String(v.to_string()))
                } else { continue; }
            }
            6 => { // float
                if value_data.len() >= 4 {
                    let v = f32::from_le_bytes([value_data[0], value_data[1], value_data[2], value_data[3]]);
                    (v.to_string(), crate::value::Value::String(v.to_string()))
                } else { continue; }
            }
            7 => { // double
                if value_data.len() >= 8 {
                    let v = f64::from_le_bytes([value_data[0], value_data[1], value_data[2], value_data[3],
                                                value_data[4], value_data[5], value_data[6], value_data[7]]);
                    (v.to_string(), crate::value::Value::String(v.to_string()))
                } else { continue; }
            }
            _ => continue, // unknown format
        };

        // Convert tag name from snake_case to CamelCase using Perl MakeNameAndDesc logic
        let name = qualcomm_tag_to_name(&tag_str);
        if name.is_empty() { continue; }

        tags.push(crate::tag::Tag {
            id: crate::tag::TagId::Text(tag_str.clone()),
            name: name.clone(),
            description: name.clone(),
            group: crate::tag::TagGroup {
                family0: "Qualcomm".into(),
                family1: "Qualcomm".into(),
                family2: "Camera".into(),
            },
            raw_value,
            print_value: val_str,
            priority: 0,
        });
    }
    tags
}

/// Convert Qualcomm snake_case tag ID to CamelCase tag name.
/// Based on Perl Qualcomm.pm MakeNameAndDesc.
fn qualcomm_tag_to_name(tag: &str) -> String {
    let mut s = tag.to_string();

    // Step 1: capitalize leading acronyms/patterns, or just first letter
    // Perl: s/^(asf|awb|aec|afr|af_|la_|r2_tl|tl)/\U$1/ or $_ = ucfirst
    let prefixes = ["asf", "awb", "aec", "afr", "af_", "la_", "r2_tl", "tl"];
    let mut matched = false;
    for pfx in &prefixes {
        if s.starts_with(pfx) {
            let upper = pfx.to_uppercase();
            s = format!("{}{}", upper, &s[pfx.len()..]);
            matched = true;
            break;
        }
    }
    if !matched {
        // ucfirst
        let mut chars = s.chars();
        if let Some(c) = chars.next() {
            s = format!("{}{}", c.to_uppercase(), chars.as_str());
        }
    }

    // Step 2: capitalize first letter of each word after underscore
    // Perl: s/_([a-z])/_\u$1/g
    let mut result = String::new();
    let mut prev_underscore = false;
    for c in s.chars() {
        if c == '_' {
            prev_underscore = true;
            result.push('_');
        } else if prev_underscore && c.is_ascii_lowercase() {
            result.push(c.to_ascii_uppercase());
            prev_underscore = false;
        } else {
            result.push(c);
            prev_underscore = false;
        }
    }
    s = result;

    // Step 3: handle bracket subscripts [N] -> _NN  (2-digit)
    // Perl: s/\[(\d+)\]$/sprintf("_%.2d",$1)/e
    if let Some(bracket_pos) = s.find('[') {
        if s.ends_with(']') {
            let inner = &s[bracket_pos + 1..s.len() - 1];
            if let Ok(n) = inner.parse::<u32>() {
                s = format!("{}_{:02}", &s[..bracket_pos], n);
            }
        }
    }

    // Step 4: delete invalid characters (keep only alphanumeric, dash, underscore)
    // Perl: tr/-_a-zA-Z0-9//dc
    s.retain(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');

    // Step 5: build description (with underscores as spaces), then remove unnecessary underscores
    // For tag name: remove underscores between letter transitions
    // Perl: s/_([A-Z][a-z])/$1/g; s/([a-z0-9])_([A-Z])/$1$2/g; s/([A-Za-z])_(\d)/$1$2/g
    let has_underscore = s.contains('_');
    if has_underscore {
        // Remove underscore before uppercase+lowercase: _Xx -> Xx
        let mut out = String::new();
        let chars: Vec<char> = s.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '_' && i + 2 < chars.len()
                && chars[i + 1].is_ascii_uppercase()
                && chars[i + 2].is_ascii_lowercase()
            {
                // skip the underscore
                i += 1;
                continue;
            }
            out.push(chars[i]);
            i += 1;
        }
        s = out;

        // Remove underscore between lowercase/digit and uppercase: xX -> xX
        let mut out2 = String::new();
        let chars: Vec<char> = s.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '_' && i > 0 && i + 1 < chars.len()
                && (chars[i - 1].is_ascii_lowercase() || chars[i - 1].is_ascii_digit())
                && chars[i + 1].is_ascii_uppercase()
            {
                i += 1;
                continue;
            }
            out2.push(chars[i]);
            i += 1;
        }
        s = out2;

        // Remove underscore between letter and digit: x_1 -> x1
        let mut out3 = String::new();
        let chars: Vec<char> = s.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '_' && i > 0 && i + 1 < chars.len()
                && chars[i - 1].is_ascii_alphabetic()
                && chars[i + 1].is_ascii_digit()
            {
                i += 1;
                continue;
            }
            out3.push(chars[i]);
            i += 1;
        }
        s = out3;
    }

    s
}

/// Extract FreeBytes (CIFF tag 0x0001) from CIFF data that canon_raw::read_crw skips.
/// Walks the CIFF directory structure to find tag 0x0001 entries.
fn extract_ciff_freebytes(data: &[u8]) -> Vec<crate::tag::Tag> {
    if data.len() < 14 {
        return Vec::new();
    }
    let is_le = data[0] == b'I' && data[1] == b'I';
    if !is_le && !(data[0] == b'M' && data[1] == b'M') {
        return Vec::new();
    }
    let hlen = if is_le {
        u32::from_le_bytes([data[2], data[3], data[4], data[5]]) as usize
    } else {
        u32::from_be_bytes([data[2], data[3], data[4], data[5]]) as usize
    };
    if hlen < 14 || data.len() < hlen || &data[6..10] != b"HEAP" {
        return Vec::new();
    }
    let mut tags = Vec::new();
    ciff_find_freebytes(data, hlen, data.len(), is_le, &mut tags, 0);
    tags
}

fn ciff_find_freebytes(
    data: &[u8], block_start: usize, block_end: usize,
    is_le: bool, tags: &mut Vec<crate::tag::Tag>, depth: u32,
) {
    if depth > 10 || block_end <= block_start + 4 || block_end > data.len() {
        return;
    }
    let ru16 = |off: usize| -> u16 {
        if is_le { u16::from_le_bytes([data[off], data[off + 1]]) }
        else { u16::from_be_bytes([data[off], data[off + 1]]) }
    };
    let ru32 = |off: usize| -> u32 {
        if is_le { u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]) }
        else { u32::from_be_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]) }
    };

    let dir_offset = ru32(block_end - 4) as usize + block_start;
    if dir_offset + 2 > block_end { return; }
    let num_entries = ru16(dir_offset) as usize;
    let mut pos = dir_offset + 2;

    for _ in 0..num_entries {
        if pos + 10 > block_end { break; }
        let raw_tag = ru16(pos);
        let size_field = ru32(pos + 2) as usize;
        let value_offset = ru32(pos + 6) as usize;
        let entry_pos = pos;
        pos += 10;

        if (raw_tag & 0x8000) != 0 { continue; }
        let tag_id = raw_tag & 0x3FFF;
        let data_type = (raw_tag >> 8) & 0x38;
        let value_in_dir = (raw_tag & 0x4000) != 0;

        // Recurse into subdirectories
        if (data_type == 0x28 || data_type == 0x30) && !value_in_dir {
            let abs_offset = value_offset + block_start;
            if abs_offset + size_field <= block_end {
                ciff_find_freebytes(data, abs_offset, abs_offset + size_field, is_le, tags, depth + 1);
            }
            continue;
        }

        // Only look for FreeBytes (tag 0x0001)
        if tag_id != 0x0001 { continue; }

        let value_data = if value_in_dir {
            if entry_pos + 10 > data.len() { continue; }
            &data[entry_pos + 2..entry_pos + 10]
        } else {
            let abs_offset = value_offset + block_start;
            if abs_offset + size_field > data.len() { continue; }
            &data[abs_offset..abs_offset + size_field]
        };

        tags.push(crate::tag::Tag {
            id: crate::tag::TagId::Numeric(0x0001),
            name: "FreeBytes".into(),
            description: "Free Bytes".into(),
            group: crate::tag::TagGroup {
                family0: "CanonRaw".into(),
                family1: "CanonRaw".into(),
                family2: "Camera".into(),
            },
            raw_value: crate::value::Value::Binary(value_data.to_vec()),
            print_value: format!("(Binary data {} bytes, use -b option to extract)", value_data.len()),
            priority: 0,
        });
        return; // only need the first FreeBytes
    }
}

/// Parse JPS (JPEG Stereo) APP3 segment.
/// Data starts with "_JPSJPS_" (8 bytes), followed by the block.
/// Mirrors ExifTool's JPEG::JPS table (JPEG.pm).
fn parse_jps(data: &[u8]) -> Vec<Tag> {
    use crate::tag::{TagId, TagGroup};
    use crate::value::Value;

    let mut tags = Vec::new();

    let mk = |name: &str, description: &str, val: Value| -> Tag {
        let print_value = val.to_display_string();
        Tag {
            id: TagId::Text(name.to_string()),
            name: name.to_string(),
            description: description.to_string(),
            group: TagGroup {
                family0: "APP3".into(),
                family1: "JPS".into(),
                family2: "Image".into(),
            },
            raw_value: val,
            print_value,
            priority: 0,
        }
    };

    // HdrLength at offset 0x08 (int16u)
    let hdr_length = if data.len() >= 10 {
        u16::from_be_bytes([data[8], data[9]]) as usize
    } else {
        return tags;
    };

    if data.len() < 14 {
        return tags;
    }

    // JPSSeparation and MediaType from int32u at offset 0x0a
    let sep_raw = u32::from_be_bytes([data[10], data[11], data[12], data[13]]);
    let media_type = sep_raw & 0xff;
    let separation = (sep_raw >> 24) & 0xff;

    if media_type == 1 {
        // Stereo only: emit JPSSeparation
        tags.push(mk("JPSSeparation", "JPS Separation", Value::U32(separation)));
    }

    // JPSFlags at offset 0x0b
    if data.len() > 11 {
        let flags = data[11];
        let mut flag_strs = Vec::new();
        if flags & (1 << 0) != 0 { flag_strs.push("Half height"); }
        if flags & (1 << 1) != 0 { flag_strs.push("Half width"); }
        if flags & (1 << 2) != 0 { flag_strs.push("Left field first"); }
        let flag_str = if flag_strs.is_empty() {
            String::new()
        } else {
            flag_strs.join(", ")
        };
        tags.push(mk("JPSFlags", "JPS Flags", Value::String(flag_str)));
    }

    // JPSLayout at offset 0x0c
    if data.len() > 12 {
        let layout = data[12];
        let layout_str = if media_type == 0 {
            // Mono
            match layout {
                0 => "Both Eyes",
                1 => "Left Eye",
                2 => "Right Eye",
                _ => "Unknown",
            }
        } else {
            // Stereo
            match layout {
                1 => "Interleaved",
                2 => "Side By Side",
                3 => "Over Under",
                4 => "Anaglyph",
                _ => "Unknown",
            }
        };
        tags.push(mk("JPSLayout", "JPS Layout", Value::String(layout_str.to_string())));
    }

    // JPSType at offset 0x0d
    if data.len() > 13 {
        let jtype = data[13];
        let type_str = match jtype {
            0 => "Mono",
            1 => "Stereo",
            _ => "Unknown",
        };
        tags.push(mk("JPSType", "JPS Type", Value::String(type_str.to_string())));
    }

    // JPSComment: starts at offset 0x10, adjusted by HdrLength - 4
    // "Hook => $varSize += $$self{HdrLength} - 4" means comment is at 0x10 + (HdrLength - 4)
    let comment_offset = 0x10 + (hdr_length.saturating_sub(4));
    if data.len() > comment_offset {
        let comment_bytes = &data[comment_offset..];
        // Strip null bytes
        let comment = String::from_utf8_lossy(comment_bytes)
            .trim_end_matches('\0')
            .to_string();
        if !comment.is_empty() {
            tags.push(mk("JPSComment", "JPS Comment", Value::String(comment)));
        }
    }

    tags
}
