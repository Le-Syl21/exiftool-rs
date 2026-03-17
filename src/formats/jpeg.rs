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
                let num_entries = rd16_afcp(afcp, 18) as usize; // at offset 12+4+2=18? No.
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
                    let digest = crate::md5::md5_hex(&iptc2_data);
                    if let Ok(iptc_tags) = IptcReader::read(&iptc2_data) {
                        tags.extend(iptc_tags);
                    }
                }
            }
        }
    }

    Ok(tags)
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
    let ver_be = u32::from_be_bytes([data[0x14], data[0x15], data[0x16], data[0x17]]);
    let ver_le = u32::from_le_bytes([data[0x14], data[0x15], data[0x16], data[0x17]]);
    let le = ver_le >= 100 && ver_le < 200;

    let rd32 = |off: usize| -> u32 {
        if off + 4 > data.len() { return 0; }
        if le { u32::from_le_bytes([data[off], data[off+1], data[off+2], data[off+3]]) }
        else { u32::from_be_bytes([data[off], data[off+1], data[off+2], data[off+3]]) }
    };
    let rd_f32 = |off: usize| -> f32 {
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
                        let cam_pn = String::from_utf8_lossy(&rec[244..276]).trim_end_matches('\0').to_string();
                        if !cam_pn.is_empty() { tags.push(mk("CameraPartNumber", cam_pn)); }
                        let cam_sn = String::from_utf8_lossy(&rec[276..308]).trim_end_matches('\0').to_string();
                        if !cam_sn.is_empty() { tags.push(mk("CameraSerialNumber", cam_sn)); }
                    }
                    if rec.len() >= 420 {
                        let cam_sw = String::from_utf8_lossy(&rec[308..340]).trim_end_matches('\0').to_string();
                        if !cam_sw.is_empty() { tags.push(mk("CameraSoftware", cam_sw)); }
                        let lens_model = String::from_utf8_lossy(&rec[340..372]).trim_end_matches('\0').to_string();
                        if !lens_model.is_empty() { tags.push(mk("LensModel", lens_model)); }
                        let lens_pn = String::from_utf8_lossy(&rec[372..404]).trim_end_matches('\0').to_string();
                        if !lens_pn.is_empty() { tags.push(mk("LensPartNumber", lens_pn)); }
                        let lens_sn = String::from_utf8_lossy(&rec[404..436]).trim_end_matches('\0').to_string();
                        if !lens_sn.is_empty() { tags.push(mk("LensSerialNumber", lens_sn)); }
                        let fov = rf(436);
                        if fov > 0.0 { tags.push(mk("FieldOfView", format!("{:.1} deg", fov))); }
                        let filter_model = String::from_utf8_lossy(&rec[492..524]).trim_end_matches('\0').to_string();
                        if !filter_model.is_empty() { tags.push(mk("FilterModel", filter_model)); }
                        let filter_pn = String::from_utf8_lossy(&rec[524..556]).trim_end_matches('\0').to_string();
                        if !filter_pn.is_empty() { tags.push(mk("FilterPartNumber", filter_pn)); }
                        let filter_sn = String::from_utf8_lossy(&rec[556..588]).trim_end_matches('\0').to_string();
                        if !filter_sn.is_empty() { tags.push(mk("FilterSerialNumber", filter_sn)); }
                    }
                    tags.push(mk("PeakSpectralSensitivity", format!("{:.1} um", rf(440))));
                    tags.push(mk("FocusStepCount", rd32(444).to_string()));
                    tags.push(mk("FocusDistance", format!("{:.1} m", rf(448))));
                    tags.push(mk("FrameRate", format!("{}", u16::from_le_bytes([rec[452], rec[453]]))));
                }
            }
            0x22 => {
                // PaletteInfo
                if rec.len() >= 50 {
                    let palette_colors = rd32(0) as usize;
                    tags.push(mk("PaletteColors", palette_colors.to_string()));
                    // Palette name, method, etc.
                    if rec.len() >= 128 {
                        let name = String::from_utf8_lossy(&rec[48..80]).trim_end_matches('\0').to_string();
                        if !name.is_empty() { tags.push(mk("PaletteName", name)); }
                        let fname = String::from_utf8_lossy(&rec[80..128]).trim_end_matches('\0').to_string();
                        if !fname.is_empty() { tags.push(mk("PaletteFileName", fname)); }
                    }
                }
            }
            0x01 => {
                // RawData — extract dimensions
                if rec.len() >= 32 {
                    let w = u16::from_le_bytes([rec[2], rec[3]]);
                    let h = u16::from_le_bytes([rec[4], rec[5]]);
                    tags.push(mk("RawThermalImageWidth", w.to_string()));
                    tags.push(mk("RawThermalImageHeight", h.to_string()));
                    let img_type = u16::from_le_bytes([rec[24], rec[25]]);
                    let type_str = match img_type {
                        0 => "TIFF", 1 => "PNG", 2 => "JPEG", 100 => "JP2", _ => "",
                    };
                    if !type_str.is_empty() { tags.push(mk("RawThermalImageType", type_str.into())); }
                    tags.push(mk("RawThermalImage", format!("(Binary data {} bytes)", rec.len())));
                }
            }
            _ => {}
        }
    }

    tags
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
        0x0425 => "IPTCDigest",
        0x0426 => "PrintScale",
        0x043C => "MeasurementScale",
        0x043D => "TimelineInfo",
        0x043E => "SheetDisclosure",
        0x043F => "DisplayInfo",
        0x0440 => "OnionSkins",
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
            // PrintScale at FORMAT index 10 = float at byte 10*2=20
            if data.len() >= 24 {
                let scale = f32::from_be_bytes([data[20], data[21], data[22], data[23]]);
                tags.push(mk("PrintScale", format!("{}", scale)));
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
