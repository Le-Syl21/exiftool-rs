//! EXIF/TIFF IFD metadata reader.
//!
//! Implements reading of TIFF IFD structures used in EXIF, GPS, and Interop metadata.
//! Mirrors the core logic of ExifTool's Exif.pm ProcessExif function.

use byteorder::{BigEndian, ByteOrder, LittleEndian};

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::tags::exif as exif_tags;
use crate::value::Value;

/// Byte order of the TIFF data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteOrderMark {
    LittleEndian,
    BigEndian,
}

/// Parsed TIFF header.
#[derive(Debug)]
pub struct TiffHeader {
    pub byte_order: ByteOrderMark,
    pub ifd0_offset: u32,
}

/// EXIF IFD entry as read from the file.
#[derive(Debug)]
struct IfdEntry {
    tag: u16,
    data_type: u16,
    count: u32,
    value_offset: u32,
    /// For values that fit in 4 bytes, the raw 4 bytes
    inline_data: [u8; 4],
}

/// Size in bytes for each TIFF data type.
fn type_size(data_type: u16) -> Option<usize> {
    match data_type {
        1 => Some(1),  // BYTE
        2 => Some(1),  // ASCII
        3 => Some(2),  // SHORT
        4 => Some(4),  // LONG
        5 => Some(8),  // RATIONAL
        6 => Some(1),  // SBYTE
        7 => Some(1),  // UNDEFINED
        8 => Some(2),  // SSHORT
        9 => Some(4),  // SLONG
        10 => Some(8), // SRATIONAL
        11 => Some(4), // FLOAT
        12 => Some(8), // DOUBLE
        13 => Some(4), // IFD
        _ => None,
    }
}

/// Parse a TIFF header from raw bytes.
pub fn parse_tiff_header(data: &[u8]) -> Result<TiffHeader> {
    if data.len() < 8 {
        return Err(Error::InvalidTiffHeader);
    }

    let byte_order = match (data[0], data[1]) {
        (b'I', b'I') => ByteOrderMark::LittleEndian,
        (b'M', b'M') => ByteOrderMark::BigEndian,
        _ => return Err(Error::InvalidTiffHeader),
    };

    let magic = match byte_order {
        ByteOrderMark::LittleEndian => LittleEndian::read_u16(&data[2..4]),
        ByteOrderMark::BigEndian => BigEndian::read_u16(&data[2..4]),
    };

    if magic != 42 {
        return Err(Error::InvalidTiffHeader);
    }

    let ifd0_offset = match byte_order {
        ByteOrderMark::LittleEndian => LittleEndian::read_u32(&data[4..8]),
        ByteOrderMark::BigEndian => BigEndian::read_u32(&data[4..8]),
    };

    Ok(TiffHeader {
        byte_order,
        ifd0_offset,
    })
}

/// EXIF metadata reader.
pub struct ExifReader;

impl ExifReader {
    /// Parse EXIF data from a byte slice (starting at the TIFF header).
    pub fn read(data: &[u8]) -> Result<Vec<Tag>> {
        let header = parse_tiff_header(data)?;
        let mut tags = Vec::new();

        // Emit ExifByteOrder tag
        let bo_str = match header.byte_order {
            ByteOrderMark::LittleEndian => "Little-endian (Intel, II)",
            ByteOrderMark::BigEndian => "Big-endian (Motorola, MM)",
        };
        tags.push(Tag {
            id: TagId::Text("ExifByteOrder".to_string()),
            name: "ExifByteOrder".to_string(),
            description: "Exif Byte Order".to_string(),
            group: TagGroup {
                family0: "EXIF".to_string(),
                family1: "IFD0".to_string(),
                family2: "ExifTool".to_string(),
            },
            raw_value: Value::String(bo_str.to_string()),
            print_value: bo_str.to_string(),
            priority: 0,
        });

        // Detect CR2: "CR" at offset 8 in TIFF data
        let is_cr2 = data.len() > 10 && &data[8..10] == b"CR";

        // Read IFD0 (main image)
        Self::read_ifd(data, &header, header.ifd0_offset, "IFD0", &mut tags)?;

        // For CR2 files, rename IFD0 StripOffsets→PreviewImageStart and
        // StripByteCounts→PreviewImageLength, then construct PreviewImage.
        if is_cr2 {
            // Rename tags in-place
            for tag in tags.iter_mut() {
                if tag.group.family1 == "IFD0" {
                    if tag.name == "StripOffsets" {
                        tag.name = "PreviewImageStart".to_string();
                        tag.description = "Preview Image Start".to_string();
                        tag.id = TagId::Text("PreviewImageStart".to_string());
                    } else if tag.name == "StripByteCounts" {
                        tag.name = "PreviewImageLength".to_string();
                        tag.description = "Preview Image Length".to_string();
                        tag.id = TagId::Text("PreviewImageLength".to_string());
                    }
                }
            }
            // Construct PreviewImage from PreviewImageStart + PreviewImageLength
            let preview_start = tags.iter()
                .find(|t| t.name == "PreviewImageStart" && t.group.family1 == "IFD0")
                .and_then(|t| t.raw_value.as_u64())
                .map(|v| v as usize);
            let preview_len = tags.iter()
                .find(|t| t.name == "PreviewImageLength" && t.group.family1 == "IFD0")
                .and_then(|t| t.raw_value.as_u64())
                .map(|v| v as usize);
            if let (Some(start), Some(len)) = (preview_start, preview_len) {
                if len > 0 && start + len <= data.len() {
                    let img_data = data[start..start+len].to_vec();
                    let pv = format!("(Binary data {} bytes, use -b option to extract)", len);
                    tags.push(Tag {
                        id: TagId::Text("PreviewImage".to_string()),
                        name: "PreviewImage".to_string(),
                        description: "Preview Image".to_string(),
                        group: TagGroup {
                            family0: "EXIF".to_string(),
                            family1: "IFD0".to_string(),
                            family2: "Preview".to_string(),
                        },
                        raw_value: Value::Binary(img_data),
                        print_value: pv,
                        priority: 0,
                    });
                }
            }
        }

        // Extract Make + Model for MakerNotes detection and sub-table dispatch
        let make = tags
            .iter()
            .find(|t| t.name == "Make")
            .map(|t| t.print_value.clone())
            .unwrap_or_default();

        let model = tags
            .iter()
            .find(|t| t.name == "Model")
            .map(|t| t.print_value.clone())
            .unwrap_or_default();

        // Store model for sub-table dispatch
        let make_and_model = if model.is_empty() { make.clone() } else { model };

        // Find and parse MakerNotes
        // Look for the MakerNote tag (0x927C) that was stored as Undefined
        let mn_info: Option<(usize, usize)> = {
            // Re-scan ExifIFD for MakerNote offset/size
            let mut result = None;
            Self::find_makernote(data, &header, &mut result);
            result
        };

        if let Some((mn_offset, mn_size)) = mn_info {
            let mn_tags = crate::metadata::makernotes::parse_makernotes(
                data, mn_offset, mn_size, &make, &make_and_model, header.byte_order,
            );
            // Remove the raw MakerNote tag and replace with parsed tags
            tags.retain(|t| t.name != "MakerNote");
            // In Perl ExifTool, MakerNotes tags with equal/higher priority overwrite EXIF tags.
            // Tags in the EXIF-primary list: EXIF wins (skip MakerNotes duplicate).
            // Other tags: MakerNotes wins (remove EXIF version, add MakerNotes version).
            {
                // Tags where EXIF takes priority over MakerNotes (structural/authoritative EXIF)
                let exif_primary: &[&str] = &[
                    "ThumbnailOffset", "ThumbnailLength", "ThumbnailImage",
                    "StripOffsets", "StripByteCounts",
                    "PreviewImageStart", "PreviewImageLength", "PreviewImage",
                    "ImageWidth", "ImageHeight", "BitsPerSample", "Compression",
                    "PhotometricInterpretation", "SamplesPerPixel", "RowsPerStrip",
                    "PlanarConfiguration", "XResolution", "YResolution", "ResolutionUnit",
                    "Orientation", "Make", "Model", "Software",
                    "ExifByteOrder", "CR2CFAPattern", "RawImageSegmentation",
                    "ColorSpace", "ExifVersion", "FlashpixVersion",
                    "ExifImageWidth", "ExifImageHeight", "InteropIndex", "InteropVersion",
                    "DateTimeOriginal", "CreateDate", "ModifyDate", "DateTime",
                    "FocalPlaneXResolution", "FocalPlaneYResolution", "FocalPlaneResolutionUnit",
                    "CustomRendered", "ExposureMode", "SceneCaptureType",
                    "Flash", "FocalLength", "ISO", "ExposureTime", "ExposureProgram",
                    "FNumber", "ShutterSpeedValue", "ApertureValue", "ComponentsConfiguration",
                    "UserComment",
                ];
                let mn_name_set: std::collections::HashSet<String> = mn_tags.iter()
                    .map(|t| t.name.clone())
                    .collect();
                let exif_has: std::collections::HashSet<String> = tags.iter()
                    .map(|t| t.name.clone())
                    .collect();
                // Remove EXIF non-primary tags when MakerNotes provides them (MakerNotes wins)
                tags.retain(|t| {
                    !mn_name_set.contains(&t.name)
                    || exif_primary.contains(&t.name.as_str())
                });
                // Add MakerNotes tags, but skip EXIF-primary tags that EXIF already provides
                for mn_tag in mn_tags {
                    if exif_primary.contains(&mn_tag.name.as_str())
                        && exif_has.contains(&mn_tag.name) {
                        // EXIF wins - don't add MakerNotes version
                        continue;
                    }
                    tags.push(mn_tag);
                }
            }
        }

        // Parse IPTC data embedded in TIFF (tag 0x83BB "IPTC-NAA")
        // The raw tag stores IPTC data as undefined bytes or a list of u32 values
        {
            let iptc_data: Option<Vec<u8>> = tags.iter()
                .find(|t| t.name == "IPTC-NAA")
                .and_then(|t| {
                    match &t.raw_value {
                        Value::Undefined(bytes) => Some(bytes.clone()),
                        Value::Binary(bytes) => Some(bytes.clone()),
                        Value::List(items) => {
                            // IPTC-NAA stored as uint32 list - convert back to bytes (big-endian)
                            let mut bytes = Vec::with_capacity(items.len() * 4);
                            for item in items {
                                match item {
                                    Value::U32(v) => bytes.extend_from_slice(&v.to_be_bytes()),
                                    _ => {}
                                }
                            }
                            if bytes.is_empty() { None } else { Some(bytes) }
                        }
                        _ => None,
                    }
                });

            if let Some(iptc_bytes) = iptc_data {
                // Compute MD5 of the raw IPTC data for CurrentIPTCDigest
                let md5_hex = crate::md5::md5_hex(&iptc_bytes);

                if let Ok(iptc_tags) = crate::metadata::IptcReader::read(&iptc_bytes) {
                    // Replace raw IPTC-NAA tag with parsed IPTC tags
                    tags.retain(|t| t.name != "IPTC-NAA");
                    tags.extend(iptc_tags);
                }

                // Add CurrentIPTCDigest tag
                tags.push(crate::tag::Tag {
                    id: crate::tag::TagId::Text("CurrentIPTCDigest".into()),
                    name: "CurrentIPTCDigest".into(),
                    description: "Current IPTC Digest".into(),
                    group: crate::tag::TagGroup {
                        family0: "IPTC".into(),
                        family1: "IPTC".into(),
                        family2: "Other".into(),
                    },
                    raw_value: Value::String(md5_hex.clone()),
                    print_value: md5_hex,
                    priority: 0,
                });
            }
        }

        // Parse ICC_Profile data embedded in TIFF (tag 0x8773)
        {
            let icc_data: Option<Vec<u8>> = tags.iter()
                .find(|t| t.name == "ICC_Profile")
                .and_then(|t| {
                    match &t.raw_value {
                        Value::Undefined(bytes) => Some(bytes.clone()),
                        Value::Binary(bytes) => Some(bytes.clone()),
                        _ => None,
                    }
                });

            if let Some(icc_bytes) = icc_data {
                if let Ok(icc_tags) = crate::formats::icc::read_icc(&icc_bytes) {
                    // Replace raw ICC_Profile tag with parsed ICC tags
                    tags.retain(|t| t.name != "ICC_Profile");
                    tags.extend(icc_tags);
                }
            }
        }

        // Process GeoTIFF key directory if present
        process_geotiff_keys(&mut tags);

        // Final deduplication: within MakerNotes, if the same tag name appears multiple times
        // (e.g., from different sub-tables), keep the last occurrence.
        // Only deduplicate MakerNotes tags (family0 == "MakerNotes") to avoid affecting
        // structural EXIF/IFD tags.
        {
            // Find MakerNotes tags that have duplicates
            let mn_tags_start = tags.iter().position(|t| t.group.family0 == "MakerNotes")
                .unwrap_or(tags.len());
            if mn_tags_start < tags.len() {
                // For each MakerNotes tag name, find the last occurrence index
                let mut last_idx: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
                for (i, t) in tags[mn_tags_start..].iter().enumerate() {
                    last_idx.insert(t.name.as_str(), i + mn_tags_start);
                }
                // Retain only the last occurrence of each MakerNotes duplicate
                let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
                // Process in reverse, keeping only the last (= rightmost = first in reverse)
                let mut keep = vec![false; tags.len()];
                for (i, t) in tags.iter().enumerate().rev() {
                    if t.group.family0 != "MakerNotes" {
                        keep[i] = true;
                        continue;
                    }
                    if seen.insert(t.name.as_str()) {
                        keep[i] = true; // first seen in reverse = last occurrence
                    }
                }
                let mut iter = keep.iter();
                tags.retain(|_| *iter.next().unwrap_or(&true));
            }
        }

        Ok(tags)
    }

    /// Find MakerNote (tag 0x927C) offset and size in ExifIFD.
    fn find_makernote(data: &[u8], header: &TiffHeader, result: &mut Option<(usize, usize)>) {
        // First find ExifIFD offset from IFD0
        let ifd0_offset = header.ifd0_offset as usize;
        if ifd0_offset + 2 > data.len() {
            return;
        }
        let entry_count = read_u16(data, ifd0_offset, header.byte_order) as usize;
        let entries_start = ifd0_offset + 2;

        for i in 0..entry_count {
            let eoff = entries_start + i * 12;
            if eoff + 12 > data.len() { break; }
            let tag = read_u16(data, eoff, header.byte_order);
            if tag == 0x8769 {
                // ExifIFD pointer
                let exif_offset = read_u32(data, eoff + 8, header.byte_order) as usize;
                Self::find_makernote_in_ifd(data, header, exif_offset, result);
                break;
            }
        }
    }

    fn find_makernote_in_ifd(data: &[u8], header: &TiffHeader, ifd_offset: usize, result: &mut Option<(usize, usize)>) {
        if ifd_offset + 2 > data.len() {
            return;
        }
        let entry_count = read_u16(data, ifd_offset, header.byte_order) as usize;
        let entries_start = ifd_offset + 2;

        for i in 0..entry_count {
            let eoff = entries_start + i * 12;
            if eoff + 12 > data.len() { break; }
            let tag = read_u16(data, eoff, header.byte_order);
            if tag == 0x927C {
                let data_type = read_u16(data, eoff + 2, header.byte_order);
                let count = read_u32(data, eoff + 4, header.byte_order) as usize;
                let type_size = match data_type { 1 | 2 | 6 | 7 => 1, 3 | 8 => 2, 4 | 9 | 11 | 13 => 4, 5 | 10 | 12 => 8, _ => 1 };
                let total_size = type_size * count;

                if total_size <= 4 {
                    // Inline - too small for real MakerNotes
                    break;
                }
                let offset = read_u32(data, eoff + 8, header.byte_order) as usize;
                if offset + total_size <= data.len() {
                    *result = Some((offset, total_size));
                }
                break;
            }
        }
    }

    /// Parse EXIF data from a byte slice with an explicit byte order and offset.
    fn read_ifd(
        data: &[u8],
        header: &TiffHeader,
        offset: u32,
        ifd_name: &str,
        tags: &mut Vec<Tag>,
    ) -> Result<Option<u32>> {
        let offset = offset as usize;
        if offset + 2 > data.len() {
            return Err(Error::InvalidExif(format!(
                "{} offset {} beyond data length {}",
                ifd_name,
                offset,
                data.len()
            )));
        }

        let entry_count = read_u16(data, offset, header.byte_order) as usize;
        let entries_start = offset + 2;
        let _entries_end = entries_start + entry_count * 12;

        // Validate: at minimum, first entry must fit
        if entries_start + 12 > data.len() && entry_count > 0 {
            return Err(Error::InvalidExif(format!(
                "{} entries extend beyond data (need {}, have {})",
                ifd_name,
                entries_start + 12,
                data.len()
            )));
        }
        // Clamp entry count if IFD extends beyond data
        let entry_count = entry_count.min((data.len().saturating_sub(entries_start)) / 12);
        let entries_end = entries_start + entry_count * 12;

        for i in 0..entry_count {
            let entry_offset = entries_start + i * 12;
            let entry = parse_ifd_entry(data, entry_offset, header.byte_order);

            // Check for sub-IFDs (ExifIFD, GPS, Interop)
            match entry.tag {
                0x8769 => {
                    // ExifIFD
                    let sub_offset = entry.value_offset;
                    if (sub_offset as usize) < data.len() {
                        let _ = Self::read_ifd(data, header, sub_offset, "ExifIFD", tags);
                    }
                    continue;
                }
                0x8825 => {
                    // GPS IFD
                    let sub_offset = entry.value_offset;
                    if (sub_offset as usize) < data.len() {
                        let _ = Self::read_ifd(data, header, sub_offset, "GPS", tags);
                    }
                    continue;
                }
                0xA005 => {
                    // Interop IFD
                    let sub_offset = entry.value_offset;
                    if (sub_offset as usize) < data.len() {
                        let _ = Self::read_ifd(data, header, sub_offset, "InteropIFD", tags);
                    }
                    continue;
                }
                // PrintIM tag: extract version from "PrintIM" + 4-byte version
                0xC4A5 => {
                    let total_size = match entry.data_type {
                        1 | 2 | 6 | 7 => entry.count as usize,
                        _ => 0,
                    };
                    if total_size > 11 {
                        let off = entry.value_offset as usize;
                        if off + 11 <= data.len() && &data[off..off+7] == b"PrintIM" {
                            let ver = String::from_utf8_lossy(&data[off+7..off+11]).to_string();
                            tags.push(Tag {
                                id: TagId::Text("PrintIMVersion".into()),
                                name: "PrintIMVersion".into(),
                                description: "PrintIM Version".into(),
                                group: TagGroup { family0: "PrintIM".into(), family1: "PrintIM".into(), family2: "Printing".into() },
                                raw_value: Value::String(ver.clone()),
                                print_value: ver,
                                priority: 0,
                            });
                        }
                    }
                    continue; // Suppress raw PrintIM tag
                }
                // Suppress GPS tag 0x0006 (GPSAltitude) when value is 0/0
                0x0006 if ifd_name == "GPS" => {
                    if let Some(val) = read_ifd_value(data, &entry, header.byte_order) {
                        if let Value::URational(0, 0) = val {
                            continue;
                        }
                    }
                }
                // In CR2 IFD2 (preview JPEG), suppress StripOffsets/StripByteCounts
                // because IFD3 has the correct values for the raw data.
                // Also suppress tags that duplicate IFD0 content (ImageWidth, ImageHeight,
                // BitsPerSample, Compression) since the first (IFD0) value is preferred.
                0x0100 | 0x0101 | 0x0102 | 0x0103 | 0x0111 | 0x0117
                    if ifd_name == "IFD2" => {
                    continue;
                }
                // In CR2 IFD3 (raw data), suppress Compression (IFD0 value is preferred).
                0x0103 if ifd_name == "IFD3" => {
                    continue;
                }
                _ => {}
            }

            if let Some(mut value) = read_ifd_value(data, &entry, header.byte_order) {
                // GPS TimeStamp (0x0007): convert 0/0 rationals to 0/1 so it displays as "0, 0, 0"
                // (Perl treats 0/0 as 0 for GPS time, enabling GPSDateTime composite)
                if ifd_name == "GPS" && entry.tag == 0x0007 {
                    if let Value::List(ref mut items) = value {
                        for item in items.iter_mut() {
                            if matches!(item, Value::URational(0, 0)) {
                                *item = Value::URational(0, 1);
                            }
                        }
                    }
                }
                let tag_info = exif_tags::lookup(ifd_name, entry.tag);
                let (name, description, family2) = match tag_info {
                    Some(info) => (
                        info.name.to_string(),
                        info.description.to_string(),
                        info.family2.to_string(),
                    ),
                    None => {
                        // Skip known SubDirectory/internal tags that Perl doesn't emit
                        if matches!(entry.tag,
                            0x014A | // SubIFD pointers
                            0x02BC | // ApplicationNotes (XMP SubDirectory)
                            0x9216 | // NikonEncryption
                            0xC634   // DNG PrivateData
                        ) {
                            continue;
                        }
                        // Fallback to generated tags
                        match exif_tags::lookup_generated(entry.tag) {
                            Some((n, d)) => (n.to_string(), d.to_string(), "Other".to_string()),
                            None => {
                                // Perl doesn't emit unknown EXIF tags by default
                                continue;
                            },
                        }
                    }
                };

                // Suppress known SubDirectory/internal tags that Perl decodes but doesn't emit as raw
                if matches!(name.as_str(),
                    "ApplicationNotes" | // XMP data — should be parsed, not emitted raw
                    "MinSampleValue" | "MaxSampleValue" | // Not emitted by Perl for raw formats
                    "ProcessingSoftware" | // Protected tag, not always emitted
                    "PanasonicTitle" | "PanasonicTitle2" // DNG tags, wrong match for RW2
                ) {
                    continue;
                }

                let print_value =
                    exif_tags::print_conv(ifd_name, entry.tag, &value)
                        .or_else(|| {
                            // Fallback to generated print conversions
                            value.as_u64()
                                .and_then(|v| crate::tags::print_conv_generated::print_conv_by_name(&name, v as i64))
                                .map(|s| s.to_string())
                        })
                        .unwrap_or_else(|| value.to_display_string());

                tags.push(Tag {
                    id: TagId::Numeric(entry.tag),
                    name,
                    description,
                    group: TagGroup {
                        family0: "EXIF".to_string(),
                        family1: ifd_name.to_string(),
                        family2,
                    },
                    raw_value: value,
                    print_value,
                    priority: 0,
                });
            }
        }

        // Read next IFD offset
        let next_ifd_offset = if entries_end + 4 <= data.len() {
            read_u32(data, entries_end, header.byte_order)
        } else { 0 };
        if next_ifd_offset != 0 && ifd_name == "IFD0" {
            // IFD1 = thumbnail
            let ifd1_start_idx = tags.len();
            let ifd1_next = Self::read_ifd(data, header, next_ifd_offset, "IFD1", tags)
                .ok().flatten();
            // Suppress IFD1 tags that duplicate IFD0 tags (only keep thumbnail-specific ones)
            // In Perl, IFD1 (thumbnail) tags are secondary and don't appear in output if IFD0 has them.
            {
                let ifd0_names: std::collections::HashSet<String> = tags[..ifd1_start_idx].iter()
                    .map(|t| t.name.clone())
                    .collect();
                let thumbnail_tags = ["ThumbnailOffset", "ThumbnailLength", "ThumbnailImage",
                    "Compression", "PhotometricInterpretation", "JPEGInterchangeFormat",
                    "JPEGInterchangeFormatLength"];
                tags.retain(|t| {
                    if t.group.family1 != "IFD1" { return true; }
                    // Keep thumbnail-specific tags
                    if thumbnail_tags.contains(&t.name.as_str()) { return true; }
                    // Suppress IFD1 tags that IFD0 already has
                    !ifd0_names.contains(&t.name)
                });
            }

            // Create ThumbnailImage tag if offset+length are present
            let thumb_offset = tags.iter()
                .find(|t| t.name == "ThumbnailOffset" && t.group.family1 == "IFD1")
                .and_then(|t| t.raw_value.as_u64());
            let thumb_length = tags.iter()
                .find(|t| t.name == "ThumbnailLength" && t.group.family1 == "IFD1")
                .and_then(|t| t.raw_value.as_u64());

            if let (Some(off), Some(len)) = (thumb_offset, thumb_length) {
                let off = off as usize;
                let len = len as usize;
                if off + len <= data.len() && len > 0 {
                    tags.push(Tag {
                        id: TagId::Text("ThumbnailImage".into()),
                        name: "ThumbnailImage".into(),
                        description: "Thumbnail Image".into(),
                        group: TagGroup { family0: "EXIF".into(), family1: "IFD1".into(), family2: "Image".into() },
                        raw_value: Value::Binary(data[off..off+len].to_vec()),
                        print_value: format!("(Binary data {} bytes)", len),
                        priority: 0,
                    });
                }
            }

            // CR2 files have additional IFDs (IFD2, IFD3) following IFD1 in the chain.
            // CR2 is identified by "CR" bytes at offset 8 in the TIFF data.
            let is_cr2 = data.len() > 10 && &data[8..10] == b"CR";
            if is_cr2 {
                if let Some(ifd2_offset) = ifd1_next {
                    // IFD2 = preview JPEG image data (emit selected tags)
                    let ifd2_next = Self::read_ifd(data, header, ifd2_offset, "IFD2", tags)
                        .ok().flatten();
                    // IFD3 = raw image data (emit CR2CFAPattern, RawImageSegmentation, StripOffsets, StripByteCounts)
                    if let Some(ifd3_offset) = ifd2_next {
                        let _ = Self::read_ifd(data, header, ifd3_offset, "IFD3", tags);
                    }
                }
            }
        }

        Ok(if next_ifd_offset != 0 {
            Some(next_ifd_offset)
        } else {
            None
        })
    }
}

fn parse_ifd_entry(data: &[u8], offset: usize, byte_order: ByteOrderMark) -> IfdEntry {
    let tag = read_u16(data, offset, byte_order);
    let data_type = read_u16(data, offset + 2, byte_order);
    let count = read_u32(data, offset + 4, byte_order);
    let value_offset = read_u32(data, offset + 8, byte_order);
    let mut inline_data = [0u8; 4];
    inline_data.copy_from_slice(&data[offset + 8..offset + 12]);

    IfdEntry {
        tag,
        data_type,
        count,
        value_offset,
        inline_data,
    }
}

fn read_ifd_value(data: &[u8], entry: &IfdEntry, byte_order: ByteOrderMark) -> Option<Value> {
    let elem_size = type_size(entry.data_type)?;
    let total_size = elem_size * entry.count as usize;

    let value_data = if total_size <= 4 {
        &entry.inline_data[..total_size]
    } else {
        let offset = entry.value_offset as usize;
        if offset + total_size > data.len() {
            return None;
        }
        &data[offset..offset + total_size]
    };

    // IPTC-NAA (0x83BB): always read as raw binary regardless of declared type
    // Perl reads "int32u[17] as undef[68]" — the raw bytes contain IPTC records
    if entry.tag == 0x83BB {
        return Some(Value::Binary(value_data.to_vec()));
    }

    match entry.data_type {
        // BYTE
        1 => {
            if entry.count == 1 {
                Some(Value::U8(value_data[0]))
            } else {
                Some(Value::List(value_data.iter().map(|&b| Value::U8(b)).collect()))
            }
        }
        // ASCII
        2 => {
            let s = String::from_utf8_lossy(value_data);
            Some(Value::String(s.trim_end_matches('\0').to_string()))
        }
        // SHORT
        3 => {
            if entry.count == 1 {
                Some(Value::U16(read_u16(value_data, 0, byte_order)))
            } else {
                let vals: Vec<Value> = (0..entry.count as usize)
                    .map(|i| Value::U16(read_u16(value_data, i * 2, byte_order)))
                    .collect();
                Some(Value::List(vals))
            }
        }
        // LONG
        4 | 13 => {
            if entry.count == 1 {
                Some(Value::U32(read_u32(value_data, 0, byte_order)))
            } else {
                let vals: Vec<Value> = (0..entry.count as usize)
                    .map(|i| Value::U32(read_u32(value_data, i * 4, byte_order)))
                    .collect();
                Some(Value::List(vals))
            }
        }
        // RATIONAL (unsigned)
        5 => {
            if entry.count == 1 {
                let n = read_u32(value_data, 0, byte_order);
                let d = read_u32(value_data, 4, byte_order);
                Some(Value::URational(n, d))
            } else {
                let vals: Vec<Value> = (0..entry.count as usize)
                    .map(|i| {
                        let n = read_u32(value_data, i * 8, byte_order);
                        let d = read_u32(value_data, i * 8 + 4, byte_order);
                        Value::URational(n, d)
                    })
                    .collect();
                Some(Value::List(vals))
            }
        }
        // SBYTE
        6 => {
            if entry.count == 1 {
                Some(Value::I16(value_data[0] as i8 as i16))
            } else {
                let vals: Vec<Value> = value_data
                    .iter()
                    .map(|&b| Value::I16(b as i8 as i16))
                    .collect();
                Some(Value::List(vals))
            }
        }
        // UNDEFINED
        7 => Some(Value::Undefined(value_data.to_vec())),
        // SSHORT
        8 => {
            if entry.count == 1 {
                Some(Value::I16(read_i16(value_data, 0, byte_order)))
            } else {
                let vals: Vec<Value> = (0..entry.count as usize)
                    .map(|i| Value::I16(read_i16(value_data, i * 2, byte_order)))
                    .collect();
                Some(Value::List(vals))
            }
        }
        // SLONG
        9 => {
            if entry.count == 1 {
                Some(Value::I32(read_i32(value_data, 0, byte_order)))
            } else {
                let vals: Vec<Value> = (0..entry.count as usize)
                    .map(|i| Value::I32(read_i32(value_data, i * 4, byte_order)))
                    .collect();
                Some(Value::List(vals))
            }
        }
        // SRATIONAL
        10 => {
            if entry.count == 1 {
                let n = read_i32(value_data, 0, byte_order);
                let d = read_i32(value_data, 4, byte_order);
                Some(Value::IRational(n, d))
            } else {
                let vals: Vec<Value> = (0..entry.count as usize)
                    .map(|i| {
                        let n = read_i32(value_data, i * 8, byte_order);
                        let d = read_i32(value_data, i * 8 + 4, byte_order);
                        Value::IRational(n, d)
                    })
                    .collect();
                Some(Value::List(vals))
            }
        }
        // FLOAT
        11 => {
            if entry.count == 1 {
                let bits = read_u32(value_data, 0, byte_order);
                Some(Value::F32(f32::from_bits(bits)))
            } else {
                let vals: Vec<Value> = (0..entry.count as usize)
                    .map(|i| {
                        let bits = read_u32(value_data, i * 4, byte_order);
                        Value::F32(f32::from_bits(bits))
                    })
                    .collect();
                Some(Value::List(vals))
            }
        }
        // DOUBLE
        12 => {
            if entry.count == 1 {
                let bits = read_u64(value_data, 0, byte_order);
                Some(Value::F64(f64::from_bits(bits)))
            } else {
                let vals: Vec<Value> = (0..entry.count as usize)
                    .map(|i| {
                        let bits = read_u64(value_data, i * 8, byte_order);
                        Value::F64(f64::from_bits(bits))
                    })
                    .collect();
                Some(Value::List(vals))
            }
        }
        _ => None,
    }
}

// Byte-order-aware read helpers
fn read_u16(data: &[u8], offset: usize, bo: ByteOrderMark) -> u16 {
    match bo {
        ByteOrderMark::LittleEndian => LittleEndian::read_u16(&data[offset..]),
        ByteOrderMark::BigEndian => BigEndian::read_u16(&data[offset..]),
    }
}

fn read_u32(data: &[u8], offset: usize, bo: ByteOrderMark) -> u32 {
    match bo {
        ByteOrderMark::LittleEndian => LittleEndian::read_u32(&data[offset..]),
        ByteOrderMark::BigEndian => BigEndian::read_u32(&data[offset..]),
    }
}

fn read_u64(data: &[u8], offset: usize, bo: ByteOrderMark) -> u64 {
    match bo {
        ByteOrderMark::LittleEndian => LittleEndian::read_u64(&data[offset..]),
        ByteOrderMark::BigEndian => BigEndian::read_u64(&data[offset..]),
    }
}

fn read_i16(data: &[u8], offset: usize, bo: ByteOrderMark) -> i16 {
    match bo {
        ByteOrderMark::LittleEndian => LittleEndian::read_i16(&data[offset..]),
        ByteOrderMark::BigEndian => BigEndian::read_i16(&data[offset..]),
    }
}

fn read_i32(data: &[u8], offset: usize, bo: ByteOrderMark) -> i32 {
    match bo {
        ByteOrderMark::LittleEndian => LittleEndian::read_i32(&data[offset..]),
        ByteOrderMark::BigEndian => BigEndian::read_i32(&data[offset..]),
    }
}

/// Process GeoTIFF key directory (tag GeoTiffDirectory / GeoKeyDirectory)
/// and replace raw directory/ascii/double params with named GeoTIFF tags.
fn process_geotiff_keys(tags: &mut Vec<Tag>) {
    // Extract GeoTiffDirectory values
    let dir_vals: Option<Vec<u16>> = tags.iter()
        .find(|t| t.name == "GeoTiffDirectory")
        .and_then(|t| {
            match &t.raw_value {
                Value::List(items) => {
                    let vals: Vec<u16> = items.iter().filter_map(|v| {
                        match v {
                            Value::U16(x) => Some(*x),
                            Value::U32(x) => Some(*x as u16),
                            _ => None,
                        }
                    }).collect();
                    if vals.is_empty() { None } else { Some(vals) }
                }
                _ => None,
            }
        });

    let dir_vals = match dir_vals {
        Some(v) => v,
        None => return,
    };

    if dir_vals.len() < 4 {
        return;
    }

    let version = dir_vals[0];
    let revision = dir_vals[1];
    let minor_rev = dir_vals[2];
    let num_entries = dir_vals[3] as usize;

    if dir_vals.len() < 4 + num_entries * 4 {
        return;
    }

    // Extract ASCII params
    let ascii_params: Option<String> = tags.iter()
        .find(|t| t.name == "GeoTiffAsciiParams")
        .map(|t| t.print_value.clone());

    // Extract double params
    let double_params: Option<Vec<f64>> = tags.iter()
        .find(|t| t.name == "GeoTiffDoubleParams")
        .and_then(|t| {
            match &t.raw_value {
                Value::List(items) => {
                    let vals: Vec<f64> = items.iter().filter_map(|v| {
                        match v {
                            Value::F64(x) => Some(*x),
                            Value::F32(x) => Some(*x as f64),
                            _ => None,
                        }
                    }).collect();
                    if vals.is_empty() { None } else { Some(vals) }
                }
                _ => None,
            }
        });

    let mut new_tags = Vec::new();

    // Version tag
    new_tags.push(Tag {
        id: TagId::Text("GeoTiffVersion".to_string()),
        name: "GeoTiffVersion".to_string(),
        description: "GeoTiff Version".to_string(),
        group: TagGroup { family0: "EXIF".into(), family1: "IFD0".into(), family2: "Location".into() },
        raw_value: Value::String(format!("{}.{}.{}", version, revision, minor_rev)),
        print_value: format!("{}.{}.{}", version, revision, minor_rev),
        priority: 0,
    });

    // Process each GeoKey
    for i in 0..num_entries {
        let base = 4 + i * 4;
        let key_id = dir_vals[base];
        let location = dir_vals[base + 1];
        let count = dir_vals[base + 2] as usize;
        let value_or_offset = dir_vals[base + 3];

        let raw_val: Option<String> = match location {
            0 => {
                // Value stored inline in value_or_offset
                Some(format!("{}", value_or_offset))
            }
            34737 => {
                // ASCII params
                if let Some(ref ascii) = ascii_params {
                    let off = value_or_offset as usize;
                    let end = (off + count).min(ascii.len());
                    if off <= end {
                        let s = &ascii[off..end];
                        // Remove trailing '|' separators
                        let s = s.trim_end_matches('|').trim().to_string();
                        Some(s)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            34736 => {
                // Double params
                if let Some(ref doubles) = double_params {
                    let off = value_or_offset as usize;
                    if count == 1 && off < doubles.len() {
                        Some(format!("{}", doubles[off]))
                    } else if count > 1 {
                        let vals: Vec<String> = doubles.iter().skip(off).take(count)
                            .map(|v| format!("{}", v)).collect();
                        Some(vals.join(" "))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            _ => None,
        };

        let val_str = match raw_val {
            Some(v) => v,
            None => continue,
        };

        // Map GeoKey ID to tag name and print value
        let (tag_name, print_val) = geotiff_key_to_tag(key_id, &val_str);
        if tag_name.is_empty() { continue; }

        new_tags.push(Tag {
            id: TagId::Text(tag_name.clone()),
            name: tag_name.clone(),
            description: tag_name.clone(),
            group: TagGroup { family0: "EXIF".into(), family1: "IFD0".into(), family2: "Location".into() },
            raw_value: Value::String(val_str),
            print_value: print_val,
            priority: 0,
        });
    }

    if !new_tags.is_empty() {
        // Remove raw GeoTIFF tags
        tags.retain(|t| t.name != "GeoTiffDirectory" && t.name != "GeoTiffAsciiParams" && t.name != "GeoTiffDoubleParams");
        tags.extend(new_tags);
    }
}

/// Map a GeoKey ID to (tag_name, print_value).
fn geotiff_key_to_tag(key_id: u16, value: &str) -> (String, String) {
    let val_u16: Option<u16> = value.parse().ok();

    match key_id {
        // Section 6.2.1: GeoTIFF Configuration Keys
        0x0001 => return ("GeoTiffVersion".to_string(), value.to_string()), // not used here
        0x0400 => { // GTModelType
            let print = match val_u16 {
                Some(1) => "Projected".to_string(),
                Some(2) => "Geographic".to_string(),
                Some(3) => "Geocentric".to_string(),
                Some(32767) => "User Defined".to_string(),
                _ => value.to_string(),
            };
            return ("GTModelType".to_string(), print);
        }
        0x0401 => { // GTRasterType
            let print = match val_u16 {
                Some(1) => "Pixel Is Area".to_string(),
                Some(2) => "Pixel Is Point".to_string(),
                Some(32767) => "User Defined".to_string(),
                _ => value.to_string(),
            };
            return ("GTRasterType".to_string(), print);
        }
        0x0402 => return ("GTCitation".to_string(), value.to_string()),

        // Section 6.2.2: Geographic CS Parameter Keys
        0x0800 => return ("GeographicType".to_string(), geotiff_pcs_name(val_u16.unwrap_or(0), value)),
        0x0801 => return ("GeogCitation".to_string(), value.to_string()),
        0x0802 => return ("GeogGeodeticDatum".to_string(), value.to_string()),
        0x0803 => return ("GeogPrimeMeridian".to_string(), value.to_string()),
        0x0804 => return ("GeogLinearUnits".to_string(), geotiff_linear_unit_name(val_u16.unwrap_or(0), value)),
        0x0805 => return ("GeogLinearUnitSize".to_string(), value.to_string()),
        0x0806 => return ("GeogAngularUnits".to_string(), value.to_string()),
        0x0807 => return ("GeogAngularUnitSize".to_string(), value.to_string()),
        0x0808 => return ("GeogEllipsoid".to_string(), value.to_string()),
        0x0809 => return ("GeogSemiMajorAxis".to_string(), value.to_string()),
        0x080a => return ("GeogSemiMinorAxis".to_string(), value.to_string()),
        0x080b => return ("GeogInvFlattening".to_string(), value.to_string()),
        0x080c => return ("GeogAzimuthUnits".to_string(), value.to_string()),
        0x080d => return ("GeogPrimeMeridianLong".to_string(), value.to_string()),

        // Section 6.2.3: Projected CS Parameter Keys
        0x0C00 => { // ProjectedCSType
            return ("ProjectedCSType".to_string(), geotiff_pcs_name(val_u16.unwrap_or(0), value));
        }
        0x0C01 => return ("PCSCitation".to_string(), value.to_string()),
        0x0C02 => return ("Projection".to_string(), value.to_string()),
        0x0C03 => return ("ProjCoordTrans".to_string(), value.to_string()),
        0x0C04 => return ("ProjLinearUnits".to_string(), geotiff_linear_unit_name(val_u16.unwrap_or(0), value)),
        0x0C05 => return ("ProjLinearUnitSize".to_string(), value.to_string()),
        0x0C06 => return ("ProjStdParallel1".to_string(), value.to_string()),
        0x0C07 => return ("ProjStdParallel2".to_string(), value.to_string()),
        0x0C08 => return ("ProjNatOriginLong".to_string(), value.to_string()),
        0x0C09 => return ("ProjNatOriginLat".to_string(), value.to_string()),
        0x0C0a => return ("ProjFalseEasting".to_string(), value.to_string()),
        0x0C0b => return ("ProjFalseNorthing".to_string(), value.to_string()),
        0x0C0c => return ("ProjFalseOriginLong".to_string(), value.to_string()),
        0x0C0d => return ("ProjFalseOriginLat".to_string(), value.to_string()),
        0x0C0e => return ("ProjFalseOriginEasting".to_string(), value.to_string()),
        0x0C0f => return ("ProjFalseOriginNorthing".to_string(), value.to_string()),
        0x0C10 => return ("ProjCenterLong".to_string(), value.to_string()),
        0x0C11 => return ("ProjCenterLat".to_string(), value.to_string()),
        0x0C12 => return ("ProjCenterEasting".to_string(), value.to_string()),
        0x0C13 => return ("ProjCenterNorthing".to_string(), value.to_string()),
        0x0C14 => return ("ProjScaleAtNatOrigin".to_string(), value.to_string()),
        0x0C15 => return ("ProjScaleAtCenter".to_string(), value.to_string()),
        0x0C16 => return ("ProjAzimuthAngle".to_string(), value.to_string()),
        0x0C17 => return ("ProjStraightVertPoleLong".to_string(), value.to_string()),

        // Section 6.2.4: Vertical CS Keys
        0x1000 => return ("VerticalCSType".to_string(), value.to_string()),
        0x1001 => return ("VerticalCitation".to_string(), value.to_string()),
        0x1002 => return ("VerticalDatum".to_string(), value.to_string()),
        0x1003 => return ("VerticalUnits".to_string(), geotiff_linear_unit_name(val_u16.unwrap_or(0), value)),

        _ => {}
    }
    (String::new(), String::new())
}

fn geotiff_linear_unit_name(val: u16, fallback: &str) -> String {
    match val {
        9001 => "Linear Meter".to_string(),
        9002 => "Linear Foot".to_string(),
        9003 => "Linear Foot US Survey".to_string(),
        9004 => "Linear Foot Modified American".to_string(),
        9005 => "Linear Foot Clarke".to_string(),
        9006 => "Linear Foot Indian".to_string(),
        9007 => "Linear Link".to_string(),
        9008 => "Linear Link Benoit".to_string(),
        9009 => "Linear Link Sears".to_string(),
        9010 => "Linear Chain Benoit".to_string(),
        9011 => "Linear Chain Sears".to_string(),
        9012 => "Linear Yard Sears".to_string(),
        9013 => "Linear Yard Indian".to_string(),
        9014 => "Linear Fathom".to_string(),
        9015 => "Linear Mile International Nautical".to_string(),
        _ => fallback.to_string(),
    }
}

fn geotiff_pcs_name(val: u16, fallback: &str) -> String {
    // Common PCS codes - just return the code with description for common ones
    match val {
        26918 => "NAD83 UTM zone 18N".to_string(),
        26919 => "NAD83 UTM zone 19N".to_string(),
        32618 => "WGS84 UTM zone 18N".to_string(),
        32619 => "WGS84 UTM zone 19N".to_string(),
        4326 => "WGS 84".to_string(),
        4269 => "NAD83".to_string(),
        4267 => "NAD27".to_string(),
        32767 => "User Defined".to_string(),
        _ => fallback.to_string(),
    }
}
