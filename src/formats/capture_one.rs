//! CaptureOne EIP (Enhanced Image Package) reader.
//!
//! EIP is a ZIP file containing:
//! - An IIQ (Phase One TIFF) or other image file
//! - COS (CaptureOne Settings) XML files
//!
//! This mirrors ExifTool's CaptureOne.pm + ZIP.pm ProcessEIP function.

use crate::error::Result;
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

/// Helper: create a tag in the ZIP group.
fn zip_tag(name: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: "ZIP".into(),
            family1: "ZIP".into(),
            family2: "Other".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}

/// Helper: create a tag in the XML group (COS settings).
fn xml_tag(name: &str, value: String) -> Tag {
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: "XML".into(),
            family1: "XML".into(),
            family2: "Image".into(),
        },
        raw_value: Value::String(value.clone()),
        print_value: value,
        priority: 0,
    }
}

/// Helper: create a tag in the File group.
fn file_tag(name: &str, value: String) -> Tag {
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: "File".into(),
            family1: "File".into(),
            family2: "Other".into(),
        },
        raw_value: Value::String(value.clone()),
        print_value: value,
        priority: 0,
    }
}

/// A parsed entry from the ZIP central directory.
struct ZipEntry {
    filename: String,
    compression: u16,
    compressed_size: u32,
    uncompressed_size: u32,
    local_header_offset: u32,
    mod_time: u16,
    mod_date: u16,
    crc: u32,
    required_version: u16,
    bit_flag: u16,
}

/// Parse the ZIP central directory and return all entries.
fn parse_zip_central_directory(data: &[u8]) -> Option<Vec<ZipEntry>> {
    let len = data.len();
    if len < 22 {
        return None;
    }
    // Search for EOCD signature from end
    let search_start = len.saturating_sub(65557);
    let eocd_pos = {
        let mut found = None;
        let mut i = len.saturating_sub(22);
        loop {
            if i < search_start {
                break;
            }
            if data[i..i + 4] == [0x50, 0x4B, 0x05, 0x06] {
                found = Some(i);
                break;
            }
            if i == 0 {
                break;
            }
            i -= 1;
        }
        found?
    };

    let cd_entries = u16::from_le_bytes([data[eocd_pos + 10], data[eocd_pos + 11]]) as usize;
    let cd_size = u32::from_le_bytes([
        data[eocd_pos + 12],
        data[eocd_pos + 13],
        data[eocd_pos + 14],
        data[eocd_pos + 15],
    ]) as usize;
    let cd_offset = u32::from_le_bytes([
        data[eocd_pos + 16],
        data[eocd_pos + 17],
        data[eocd_pos + 18],
        data[eocd_pos + 19],
    ]) as usize;

    if cd_offset + cd_size > len {
        return None;
    }

    let mut entries = Vec::with_capacity(cd_entries);
    let mut pos = cd_offset;

    while pos + 46 <= cd_offset + cd_size && pos + 46 <= len {
        if data[pos..pos + 4] != [0x50, 0x4B, 0x01, 0x02] {
            break;
        }
        let required_version = u16::from_le_bytes([data[pos + 6], data[pos + 7]]);
        let bit_flag = u16::from_le_bytes([data[pos + 8], data[pos + 9]]);
        let compression = u16::from_le_bytes([data[pos + 10], data[pos + 11]]);
        let mod_time = u16::from_le_bytes([data[pos + 12], data[pos + 13]]);
        let mod_date = u16::from_le_bytes([data[pos + 14], data[pos + 15]]);
        let crc = u32::from_le_bytes([
            data[pos + 16],
            data[pos + 17],
            data[pos + 18],
            data[pos + 19],
        ]);
        let compressed_size = u32::from_le_bytes([
            data[pos + 20],
            data[pos + 21],
            data[pos + 22],
            data[pos + 23],
        ]);
        let uncompressed_size = u32::from_le_bytes([
            data[pos + 24],
            data[pos + 25],
            data[pos + 26],
            data[pos + 27],
        ]);
        let name_len = u16::from_le_bytes([data[pos + 28], data[pos + 29]]) as usize;
        let extra_len = u16::from_le_bytes([data[pos + 30], data[pos + 31]]) as usize;
        let comment_len = u16::from_le_bytes([data[pos + 32], data[pos + 33]]) as usize;
        let local_header_offset = u32::from_le_bytes([
            data[pos + 42],
            data[pos + 43],
            data[pos + 44],
            data[pos + 45],
        ]);

        let name_start = pos + 46;
        if name_start + name_len > len {
            break;
        }
        let filename =
            crate::encoding::decode_utf8_or_latin1(&data[name_start..name_start + name_len])
                .to_string();

        entries.push(ZipEntry {
            filename,
            compression,
            compressed_size,
            uncompressed_size,
            local_header_offset,
            mod_time,
            mod_date,
            crc,
            required_version,
            bit_flag,
        });

        pos = name_start + name_len + extra_len + comment_len;
    }

    Some(entries)
}

/// Get the raw (compressed) file data for a ZIP entry.
fn zip_entry_data<'a>(data: &'a [u8], entry: &ZipEntry) -> Option<&'a [u8]> {
    let lh_offset = entry.local_header_offset as usize;
    if lh_offset + 30 > data.len() {
        return None;
    }
    if data[lh_offset..lh_offset + 4] != [0x50, 0x4B, 0x03, 0x04] {
        return None;
    }
    let name_len = u16::from_le_bytes([data[lh_offset + 26], data[lh_offset + 27]]) as usize;
    let extra_len = u16::from_le_bytes([data[lh_offset + 28], data[lh_offset + 29]]) as usize;
    let data_start = lh_offset + 30 + name_len + extra_len;
    let data_end = data_start + entry.compressed_size as usize;
    if data_end > data.len() {
        return None;
    }
    Some(&data[data_start..data_end])
}

/// Decompress a ZIP entry (stored or deflated).
fn decompress_entry(data: &[u8], compression: u16, uncompressed_size: usize) -> Option<Vec<u8>> {
    match compression {
        0 => Some(data.to_vec()),
        8 => {
            use std::io::Read;
            let mut decoder = flate2::read::DeflateDecoder::new(data);
            let mut out = Vec::with_capacity(uncompressed_size.min(64 * 1024 * 1024));
            decoder.read_to_end(&mut out).ok()?;
            Some(out)
        }
        _ => None,
    }
}

/// Format DOS date/time as "YYYY:MM:DD HH:MM:SS".
fn format_dos_datetime(mod_date: u16, mod_time: u16) -> String {
    let year = ((mod_date >> 9) & 0x7F) as u32 + 1980;
    let month = ((mod_date >> 5) & 0x0F) as u32;
    let day = (mod_date & 0x1F) as u32;
    let hour = ((mod_time >> 11) & 0x1F) as u32;
    let minute = ((mod_time >> 5) & 0x3F) as u32;
    let second = ((mod_time & 0x1F) as u32) * 2;
    format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
        year, month, day, hour, minute, second
    )
}

/// ZIP compression method name.
fn compression_name(method: u16) -> &'static str {
    match method {
        0 => "None",
        1 => "Shrunk",
        2 => "Reduced with compression factor 1",
        3 => "Reduced with compression factor 2",
        4 => "Reduced with compression factor 3",
        5 => "Reduced with compression factor 4",
        6 => "Imploded",
        8 => "Deflated",
        9 => "Enhanced Deflate using Deflate64(tm)",
        12 => "BZIP2",
        14 => "LZMA (EFS)",
        _ => "Unknown",
    }
}

/// Extract K/V pairs from a COS XML string.
/// COS format: <E K="key" V="value"/> elements anywhere in the XML.
/// Later occurrences of the same key override earlier ones (AL overrides DL).
fn parse_cos_xml(xml: &str) -> Vec<(String, String)> {
    // We use an ordered map to preserve insertion order while allowing overrides.
    // Use a Vec of (key, value) where we track index by key.
    let mut order: Vec<String> = Vec::new();
    let mut map: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    let mut rest = xml;
    while let Some(e_pos) = rest.find("<E ") {
        let elem_start = &rest[e_pos..];
        // Find end of this element (either "/>" or ">")
        let elem_end = elem_start
            .find("/>")
            .map(|p| p + 2)
            .or_else(|| elem_start.find('>').map(|p| p + 1))
            .unwrap_or(elem_start.len());
        let elem = &elem_start[..elem_end];

        // Extract K attribute
        if let (Some(k), Some(v)) = (xml_attr(elem, "K"), xml_attr_v(elem)) {
            if !k.is_empty() {
                if !map.contains_key(&k) {
                    order.push(k.clone());
                }
                map.insert(k, v);
            }
        }

        rest = &rest[e_pos + 3..]; // advance past "<E "
        if rest.is_empty() {
            break;
        }
    }

    order
        .into_iter()
        .map(|k| {
            let v = map.remove(&k).unwrap_or_default();
            (k, v)
        })
        .collect()
}

/// Extract K attribute from an element string like `<E K="foo" V="bar"/>`.
fn xml_attr(elem: &str, attr: &str) -> Option<String> {
    let pattern = format!(" {}=\"", attr);
    let pos = elem.find(&pattern)?;
    let after = &elem[pos + pattern.len()..];
    let end = after.find('"')?;
    let val = xml_unescape(&after[..end]);
    Some(val)
}

/// Extract V attribute - handles both V="..." and V="" (empty value).
fn xml_attr_v(elem: &str) -> Option<String> {
    // Look for V=" pattern
    let pattern = " V=\"";
    let pos = elem.find(pattern)?;
    let after = &elem[pos + pattern.len()..];
    let end = after.find('"')?;
    let val = xml_unescape(&after[..end]);
    Some(val)
}

/// Unescape basic XML entities.
fn xml_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

/// Extract the highest-version manifest file name from manifest XML.
/// Returns (image_path, settings_path) if found.
fn parse_manifest(xml: &str) -> Option<(String, String)> {
    // Find <RawPath> and <SettingsPath>
    let raw_path = extract_xml_text(xml, "RawPath")?;
    let settings_path = extract_xml_text(xml, "SettingsPath")?;

    // Validate extensions
    let raw_lower = raw_path.to_ascii_lowercase();
    let settings_lower = settings_path.to_ascii_lowercase();

    if !raw_lower.ends_with(".iiq")
        && !raw_lower.ends_with(".tiff")
        && !raw_lower.ends_with(".tif")
        && !raw_lower.ends_with(".jpg")
        && !raw_lower.ends_with(".jpeg")
    {
        return None;
    }
    if !settings_lower.ends_with(".cos") {
        return None;
    }

    Some((raw_path, settings_path))
}

/// Extract text content of a simple XML element.
fn extract_xml_text(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let pos = xml.find(&open)?;
    let after = &xml[pos + open.len()..];
    let end = after.find(&close)?;
    Some(xml_unescape(after[..end].trim()))
}

/// Read a CaptureOne EIP file and return all tags.
pub fn read_eip(data: &[u8]) -> Result<Vec<Tag>> {
    let mut tags = Vec::new();

    // Parse the ZIP central directory
    let entries = parse_zip_central_directory(data).unwrap_or_default();
    if entries.is_empty() {
        return Ok(tags);
    }

    // Emit ZIP tags for the FIRST member (matching Perl ExifTool HandleMember behavior)
    // Perl's ProcessEIP calls HandleMember for every member but we just need the first one.
    // Actually Perl emits ZIP tags for each member with DOC_NUM, but the primary output
    // (without -G3) shows only the first set. We emit ZIP tags for the first entry.
    {
        let first = &entries[0];
        let bit_flag_str = if first.bit_flag != 0 {
            format!("0x{:04x}", first.bit_flag)
        } else {
            first.bit_flag.to_string()
        };
        tags.push(zip_tag(
            "ZipRequiredVersion",
            Value::U16(first.required_version),
        ));
        tags.push(zip_tag("ZipBitFlag", Value::String(bit_flag_str)));
        tags.push(zip_tag(
            "ZipCompression",
            Value::String(compression_name(first.compression).to_string()),
        ));
        tags.push(zip_tag(
            "ZipModifyDate",
            Value::String(format_dos_datetime(first.mod_date, first.mod_time)),
        ));
        tags.push(zip_tag(
            "ZipCRC",
            Value::String(format!("0x{:08x}", first.crc)),
        ));
        tags.push(zip_tag(
            "ZipCompressedSize",
            Value::U32(first.compressed_size),
        ));
        tags.push(zip_tag(
            "ZipUncompressedSize",
            Value::U32(first.uncompressed_size),
        ));
        tags.push(zip_tag(
            "ZipFileName",
            Value::String(first.filename.clone()),
        ));
    }

    // Find manifest files (manifest.xml, manifest50.xml, etc.)
    // Pick the highest-version manifest (lexicographically largest name).
    let mut best_manifest: Option<(&ZipEntry, String)> = None;
    for entry in &entries {
        let fname = &entry.filename;
        // Match manifest followed by optional digits and .xml
        let lower = fname.to_ascii_lowercase();
        if lower.starts_with("manifest") && lower.ends_with(".xml") {
            let stem = &lower[8..lower.len() - 4]; // digits between "manifest" and ".xml"
            if stem.chars().all(|c| c.is_ascii_digit()) {
                let is_better = match &best_manifest {
                    None => true,
                    Some((_, best_name)) => fname > best_name,
                };
                if is_better {
                    best_manifest = Some((entry, fname.clone()));
                }
            }
        }
    }

    // Try to parse the manifest to get image/settings paths
    let mut parse_files: std::collections::HashSet<String> = std::collections::HashSet::new();
    if let Some((manifest_entry, _)) = best_manifest {
        if let Some(raw) = zip_entry_data(data, manifest_entry) {
            if let Some(bytes) = decompress_entry(
                raw,
                manifest_entry.compression,
                manifest_entry.uncompressed_size as usize,
            ) {
                if let Ok(xml) = std::str::from_utf8(&bytes) {
                    if let Some((image_path, settings_path)) = parse_manifest(xml) {
                        parse_files.insert(image_path);
                        parse_files.insert(settings_path);
                    }
                }
            }
        }
    }

    // Process each relevant entry
    let mut iiq_data: Option<Vec<u8>> = None;
    let mut cos_data: Option<Vec<u8>> = None;

    for entry in &entries {
        let fname = &entry.filename;
        let lower = fname.to_ascii_lowercase();

        let should_parse = if !parse_files.is_empty() {
            parse_files.contains(fname.as_str())
        } else {
            // Fallback: look for image in root dir and .cos in CaptureOne/
            let is_root_image = !fname.contains('/')
                && (lower.ends_with(".iiq")
                    || lower.ends_with(".tif")
                    || lower.ends_with(".tiff")
                    || lower.ends_with(".jpg")
                    || lower.ends_with(".jpeg"));
            let is_cos = lower.starts_with("captureone/") && lower.ends_with(".cos");
            is_root_image || is_cos
        };

        if !should_parse {
            continue;
        }

        if let Some(raw) = zip_entry_data(data, entry) {
            if let Some(bytes) =
                decompress_entry(raw, entry.compression, entry.uncompressed_size as usize)
            {
                if lower.ends_with(".cos") {
                    // Keep the last .cos file found (highest version wins via manifest selection)
                    cos_data = Some(bytes);
                } else {
                    // Image file (IIQ/TIFF/JPEG)
                    iiq_data = Some(bytes);
                }
            }
        }
    }

    // Process IIQ/image file: extract EXIF/TIFF tags
    if let Some(ref iiq_bytes) = iiq_data {
        // Add ExifByteOrder to File group from the IIQ TIFF header
        if iiq_bytes.len() >= 2 {
            let bo_str = if iiq_bytes.starts_with(b"II") {
                Some("Little-endian (Intel, II)")
            } else if iiq_bytes.starts_with(b"MM") {
                Some("Big-endian (Motorola, MM)")
            } else {
                None
            };
            if let Some(bo) = bo_str {
                tags.push(file_tag("ExifByteOrder", bo.to_string()));
            }
        }

        // Parse as TIFF (the IIQ in EIP is a simple TIFF, not the full IIQ format)
        let iiq_tags = crate::formats::tiff::read_tiff(iiq_bytes).unwrap_or_default();
        // Filter out tags that shouldn't appear from EIP context:
        // - ExifByteOrder (we've already emitted it above)
        // - File group tags (FileName, FileSize, etc.) from the embedded file
        for t in iiq_tags {
            if t.name == "ExifByteOrder" {
                continue;
            }
            if t.group.family0 == "File" {
                continue;
            }
            tags.push(t);
        }
    }

    // Process COS XML settings file
    if let Some(ref cos_bytes) = cos_data {
        if let Ok(xml) = std::str::from_utf8(cos_bytes) {
            let pairs = parse_cos_xml(xml);
            for (key, val) in pairs {
                // ColorCorrections is shown as binary data per Perl CaptureOne.pm Hidden=>1
                if key == "ColorCorrections" {
                    let byte_count = val.len();
                    let display = format!(
                        "(Binary data {} bytes, use -b option to extract)",
                        byte_count
                    );
                    tags.push(Tag {
                        id: TagId::Text(key.clone()),
                        name: key.clone(),
                        description: key.clone(),
                        group: TagGroup {
                            family0: "XML".into(),
                            family1: "XML".into(),
                            family2: "Image".into(),
                        },
                        raw_value: Value::Binary(val.into_bytes()),
                        print_value: display,
                        priority: 0,
                    });
                } else {
                    tags.push(xml_tag(&key, val));
                }
            }
        }
    }

    Ok(tags)
}
