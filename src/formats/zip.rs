//! ZIP archive and ZIP-based document reader.
//!
//! Handles ZIP archives, and Office Open XML (DOCX/XLSX/PPTX)
//! and OpenDocument (ODS/ODT) formats that are ZIP containers.
//! Mirrors ExifTool's ZIP.pm, OOXML.pm, and OpenDocument.pm.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

/// Read Apple iWork ZIP-based document (Numbers, Pages, Keynote).
/// Extracts metadata from index.xml and PreviewImage from QuickLook/Thumbnail.jpg.
pub fn read_iwork(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 30 || !data.starts_with(&[0x50, 0x4B, 0x03, 0x04]) {
        return Err(Error::InvalidData("not a ZIP file".into()));
    }

    let mut tags = Vec::new();
    let mut first_file = true;

    // First pass: collect ZIP directory entries (filename → data range)
    let mut pos = 0usize;
    while pos + 30 <= data.len() && data[pos..pos + 4] == [0x50, 0x4B, 0x03, 0x04] {
        let compression = u16::from_le_bytes([data[pos + 8], data[pos + 9]]);
        let mod_time = u16::from_le_bytes([data[pos + 10], data[pos + 11]]);
        let mod_date = u16::from_le_bytes([data[pos + 12], data[pos + 13]]);
        let crc = u32::from_le_bytes([data[pos + 14], data[pos + 15], data[pos + 16], data[pos + 17]]);
        let compressed_size = u32::from_le_bytes([data[pos + 18], data[pos + 19], data[pos + 20], data[pos + 21]]) as usize;
        let uncompressed_size = u32::from_le_bytes([data[pos + 22], data[pos + 23], data[pos + 24], data[pos + 25]]) as u32;
        let name_len = u16::from_le_bytes([data[pos + 26], data[pos + 27]]) as usize;
        let extra_len = u16::from_le_bytes([data[pos + 28], data[pos + 29]]) as usize;
        let required_version = u16::from_le_bytes([data[pos + 4], data[pos + 5]]);
        let bit_flag = u16::from_le_bytes([data[pos + 6], data[pos + 7]]);

        let name_start = pos + 30;
        if name_start + name_len > data.len() { break; }
        let filename = crate::encoding::decode_utf8_or_latin1(&data[name_start..name_start + name_len]).to_string();

        if first_file {
            first_file = false;
            tags.push(mk("ZipRequiredVersion", "Required Version", Value::U16(required_version)));
            let bit_flag_str = if bit_flag != 0 { format!("0x{:04x}", bit_flag) } else { bit_flag.to_string() };
            tags.push(mk("ZipBitFlag", "Bit Flag", Value::String(bit_flag_str)));
            let compression_str = zip_compression_name(compression);
            tags.push(mk("ZipCompression", "Compression", Value::String(compression_str)));
            let year = ((mod_date >> 9) & 0x7F) as u32 + 1980;
            let month = ((mod_date >> 5) & 0x0F) as u32;
            let day = (mod_date & 0x1F) as u32;
            let hour = ((mod_time >> 11) & 0x1F) as u32;
            let minute = ((mod_time >> 5) & 0x3F) as u32;
            let second = ((mod_time & 0x1F) as u32) * 2;
            let date_str = format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}", year, month, day, hour, minute, second);
            tags.push(mk("ZipModifyDate", "Modify Date", Value::String(date_str)));
            let crc_str = format!("0x{:08x}", crc);
            tags.push(mk("ZipCRC", "CRC", Value::String(crc_str)));
            tags.push(mk("ZipCompressedSize", "Compressed Size", Value::U32(compressed_size as u32)));
            tags.push(mk("ZipUncompressedSize", "Uncompressed Size", Value::U32(uncompressed_size)));
            tags.push(mk("ZipFileName", "File Name", Value::String(filename.clone())));
        }

        let data_start = name_start + name_len + extra_len;

        // QuickLook/Thumbnail.jpg -> PreviewImage
        if filename.eq_ignore_ascii_case("QuickLook/Thumbnail.jpg") && compression == 0 {
            let file_data_end = (data_start + compressed_size).min(data.len());
            if data_start < file_data_end {
                let n = file_data_end - data_start;
                let preview_str = format!("(Binary data {} bytes, use -b option to extract)", n);
                tags.push(Tag {
                    id: TagId::Text("PreviewImage".into()),
                    name: "PreviewImage".into(),
                    description: "Preview Image".into(),
                    group: TagGroup { family0: "ZIP".into(), family1: "ZIP".into(), family2: "Preview".into() },
                    raw_value: Value::Binary(data[data_start..file_data_end].to_vec()),
                    print_value: preview_str,
                    priority: 0,
                });
            }
        }

        // index.xml or index.apxl -> extract metadata
        if filename == "index.xml" || filename == "index.apxl" {
            let file_data_end = (data_start + compressed_size).min(data.len());
            if data_start < file_data_end {
                let raw = &data[data_start..file_data_end];
                let xml_string: Option<String> = if compression == 0 {
                    std::str::from_utf8(raw).ok().map(|s| s.to_string())
                } else if compression == 8 {
                    // Deflate compressed - decompress
                    use std::io::Read;
                    let mut decoder = flate2::read::DeflateDecoder::new(raw);
                    let mut decompressed = Vec::new();
                    decoder.read_to_end(&mut decompressed).ok();
                    std::str::from_utf8(&decompressed).ok().map(|s| s.to_string())
                } else {
                    None
                };
                if let Some(text) = xml_string {
                    parse_iwork_metadata(&text, &mut tags);
                }
            }
        }

        pos = data_start + compressed_size;
    }

    Ok(tags)
}

/// Parse iWork index.xml metadata section.
fn parse_iwork_metadata(xml: &str, tags: &mut Vec<Tag>) {
    // Find the <ns:metadata>...</ns:metadata> section
    let meta_start = if let Some(p) = xml.find(":metadata>") {
        p + ":metadata>".len()
    } else {
        return;
    };
    let meta_section = &xml[meta_start..];
    let meta_end = if let Some(_p) = meta_section.find(":metadata>") {
        // Find the '</' before ':metadata>'
        let search = "</";
        if let Some(close_pos) = meta_section[.._p].rfind(search) {
            close_pos
        } else {
            return;
        }
    } else {
        return;
    };
    let metadata_xml = &meta_section[..meta_end];

    let fields = [
        ("sf:authors", "Author"),
        ("sf:copyright", "Copyright"),
        ("sf:title", "Title"),
        ("sf:keywords", "Keywords"),
        ("sf:comment", "Comment"),
        ("sf:projects", "Projects"),
    ];

    for (sf_tag, name) in &fields {
        let open = format!("<{}>", sf_tag);
        let close = format!("</{}>", sf_tag);
        if let Some(start) = metadata_xml.find(&open) {
            let inner = &metadata_xml[start + open.len()..];
            if let Some(end) = inner.find(&close) {
                let block = &inner[..end];
                let values = extract_sfa_strings(block);
                if !values.is_empty() {
                    let combined = values.join(", ");
                    let val = Value::String(combined.clone());
                    tags.push(Tag {
                        id: TagId::Text(name.to_string()),
                        name: name.to_string(),
                        description: name.to_string(),
                        group: TagGroup { family0: "XML".into(), family1: "XML".into(), family2: "Document".into() },
                        raw_value: val,
                        print_value: combined,
                        priority: 0,
                    });
                }
            }
        }
    }
}

/// Extract all sfa:string="..." values from an XML fragment.
fn extract_sfa_strings(xml: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut rest = xml;
    while let Some(pos) = rest.find("sfa:string=\"") {
        let after = &rest[pos + "sfa:string=\"".len()..];
        if let Some(end) = after.find('"') {
            let value = &after[..end];
            let unescaped = value
                .replace("&amp;", "&")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&quot;", "\"")
                .replace("&apos;", "'");
            values.push(unescaped);
            rest = &after[end + 1..];
        } else {
            break;
        }
    }
    values
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
/// Returns None if no valid EOCD is found.
fn parse_zip_central_directory(data: &[u8]) -> Option<Vec<ZipEntry>> {
    let len = data.len();
    if len < 22 {
        return None;
    }
    // Search for EOCD signature from end of file (allow up to 65535 bytes comment)
    let search_start = if len > 65557 { len - 65557 } else { 0 };
    let eocd_pos = {
        let mut found = None;
        let mut i = len.saturating_sub(22);
        loop {
            if i < search_start { break; }
            if data[i..i + 4] == [0x50, 0x4B, 0x05, 0x06] {
                found = Some(i);
                break;
            }
            if i == 0 { break; }
            i -= 1;
        }
        found?
    };

    let cd_entries = u16::from_le_bytes([data[eocd_pos + 10], data[eocd_pos + 11]]) as usize;
    let cd_size = u32::from_le_bytes([data[eocd_pos + 12], data[eocd_pos + 13],
                                      data[eocd_pos + 14], data[eocd_pos + 15]]) as usize;
    let cd_offset = u32::from_le_bytes([data[eocd_pos + 16], data[eocd_pos + 17],
                                        data[eocd_pos + 18], data[eocd_pos + 19]]) as usize;

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
        let crc = u32::from_le_bytes([data[pos + 16], data[pos + 17], data[pos + 18], data[pos + 19]]);
        let compressed_size = u32::from_le_bytes([data[pos + 20], data[pos + 21], data[pos + 22], data[pos + 23]]);
        let uncompressed_size = u32::from_le_bytes([data[pos + 24], data[pos + 25], data[pos + 26], data[pos + 27]]);
        let name_len = u16::from_le_bytes([data[pos + 28], data[pos + 29]]) as usize;
        let extra_len = u16::from_le_bytes([data[pos + 30], data[pos + 31]]) as usize;
        let comment_len = u16::from_le_bytes([data[pos + 32], data[pos + 33]]) as usize;
        let local_header_offset = u32::from_le_bytes([data[pos + 42], data[pos + 43], data[pos + 44], data[pos + 45]]);

        let name_start = pos + 46;
        if name_start + name_len > len { break; }
        let filename = crate::encoding::decode_utf8_or_latin1(&data[name_start..name_start + name_len]).to_string();

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

/// Get the file data for a ZIP entry using its local header offset.
/// Reads the local file header to find the actual data start.
fn zip_entry_data<'a>(data: &'a [u8], entry: &ZipEntry) -> Option<&'a [u8]> {
    let lh_offset = entry.local_header_offset as usize;
    if lh_offset + 30 > data.len() { return None; }
    if data[lh_offset..lh_offset + 4] != [0x50, 0x4B, 0x03, 0x04] { return None; }
    let name_len = u16::from_le_bytes([data[lh_offset + 26], data[lh_offset + 27]]) as usize;
    let extra_len = u16::from_le_bytes([data[lh_offset + 28], data[lh_offset + 29]]) as usize;
    let data_start = lh_offset + 30 + name_len + extra_len;
    let data_end = data_start + entry.compressed_size as usize;
    if data_end > data.len() { return None; }
    Some(&data[data_start..data_end])
}

pub fn read_zip(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 30 || !data.starts_with(&[0x50, 0x4B, 0x03, 0x04]) {
        return Err(Error::InvalidData("not a ZIP file".into()));
    }

    // Parse the central directory to get reliable file entries
    let entries = parse_zip_central_directory(data).unwrap_or_default();

    // Check for iWork format
    {
        let mut has_index_xml = false;
        let mut has_content_types = false;
        for entry in &entries {
            if entry.filename == "index.xml" || entry.filename == "index.apxl" {
                has_index_xml = true;
            }
            if entry.filename == "[Content_Types].xml" {
                has_content_types = true;
                break;
            }
        }
        if has_index_xml && !has_content_types {
            return read_iwork(data);
        }
    }

    // Check if this is an OpenDocument file
    let is_opendoc = entries.first()
        .map(|e| e.filename == "mimetype" && e.compression == 0)
        .unwrap_or(false) && {
        // Read mimetype content
        entries.first().and_then(|e| zip_entry_data(data, e))
            .and_then(|d| std::str::from_utf8(d).ok())
            .map(|s| s.trim().starts_with("application/vnd.oasis.opendocument"))
            .unwrap_or(false)
    };

    let mut tags = Vec::new();
    let mut has_content_types = false;
    let mut has_mimetype = false;
    let mut first_entry = true;

    for entry in &entries {
        let filename = &entry.filename;

        // Emit per-file tags for the first file (matching Perl behavior)
        // Skip for OpenDocument files
        if first_entry && !is_opendoc {
            let bit_flag_str = if entry.bit_flag != 0 {
                format!("0x{:04x}", entry.bit_flag)
            } else {
                entry.bit_flag.to_string()
            };
            tags.push(mk("ZipRequiredVersion", "Required Version", Value::U16(entry.required_version)));
            tags.push(mk("ZipBitFlag", "Bit Flag", Value::String(bit_flag_str)));
            tags.push(mk("ZipCompression", "Compression", Value::String(zip_compression_name(entry.compression))));
            let year = ((entry.mod_date >> 9) & 0x7F) as u32 + 1980;
            let month = ((entry.mod_date >> 5) & 0x0F) as u32;
            let day = (entry.mod_date & 0x1F) as u32;
            let hour = ((entry.mod_time >> 11) & 0x1F) as u32;
            let minute = ((entry.mod_time >> 5) & 0x3F) as u32;
            let second = ((entry.mod_time & 0x1F) as u32) * 2;
            let date_str = format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}", year, month, day, hour, minute, second);
            tags.push(mk("ZipModifyDate", "Modify Date", Value::String(date_str)));
            tags.push(mk("ZipCRC", "CRC", Value::String(format!("0x{:08x}", entry.crc))));
            tags.push(mk("ZipCompressedSize", "Compressed Size", Value::U32(entry.compressed_size)));
            tags.push(mk("ZipUncompressedSize", "Uncompressed Size", Value::U32(entry.uncompressed_size)));
            tags.push(mk("ZipFileName", "File Name", Value::String(filename.clone())));
        }
        if first_entry { first_entry = false; }

        if filename == "[Content_Types].xml" {
            has_content_types = true;
        }

        if filename == "mimetype" {
            has_mimetype = true;
            if !is_opendoc {
                if let Some(raw) = zip_entry_data(data, entry) {
                    if entry.compression == 0 {
                        let mime = crate::encoding::decode_utf8_or_latin1(raw).trim().to_string();
                        tags.push(mk("MIMEType", "MIME Type", Value::String(mime)));
                    }
                }
            }
        }

        // OpenDocument: parse meta.xml for metadata
        if filename == "meta.xml" {
            if let Some(raw) = zip_entry_data(data, entry) {
                if let Some(xml_bytes) = decompress_zip_entry(raw, entry.compression, entry.uncompressed_size as usize) {
                    parse_opendoc_meta(&xml_bytes, &mut tags);
                }
            }
        }

        // OpenDocument: thumbnail PNG
        if filename == "Thumbnails/thumbnail.png" {
            if let Some(raw) = zip_entry_data(data, entry) {
                if let Some(img_bytes) = decompress_zip_entry(raw, entry.compression, entry.uncompressed_size as usize) {
                    let n = img_bytes.len();
                    let preview_str = format!("(Binary data {} bytes, use -b option to extract)", n);
                    tags.push(Tag {
                        id: TagId::Text("PreviewPNG".into()),
                        name: "PreviewPNG".into(),
                        description: "Preview PNG".into(),
                        group: TagGroup { family0: "ZIP".into(), family1: "ZIP".into(), family2: "Preview".into() },
                        raw_value: Value::Binary(img_bytes),
                        print_value: preview_str,
                        priority: 0,
                    });
                }
            }
        }

        // For OOXML: parse docProps/core.xml
        if filename == "docProps/core.xml" {
            if let Some(raw) = zip_entry_data(data, entry) {
                if let Some(xml_bytes) = decompress_zip_entry(raw, entry.compression, entry.uncompressed_size as usize) {
                    parse_ooxml_core(&xml_bytes, &mut tags);
                }
            }
        }

        if filename == "docProps/app.xml" {
            if let Some(raw) = zip_entry_data(data, entry) {
                if let Some(xml_bytes) = decompress_zip_entry(raw, entry.compression, entry.uncompressed_size as usize) {
                    parse_ooxml_app(&xml_bytes, &mut tags);
                }
            }
        }

        if filename == "docProps/custom.xml" {
            if let Some(raw) = zip_entry_data(data, entry) {
                if let Some(xml_bytes) = decompress_zip_entry(raw, entry.compression, entry.uncompressed_size as usize) {
                    parse_ooxml_custom(&xml_bytes, &mut tags);
                }
            }
        }

        // Extract PreviewImage from thumbnail
        if filename == "docProps/thumbnail.jpeg" || filename == "docProps/thumbnail.jpg"
            || filename == "docProps/thumbnail.png" {
            if let Some(raw) = zip_entry_data(data, entry) {
                if let Some(img_bytes) = decompress_zip_entry(raw, entry.compression, entry.uncompressed_size as usize) {
                    let n = img_bytes.len();
                    let preview_str = format!("(Binary data {} bytes, use -b option to extract)", n);
                    tags.push(Tag {
                        id: TagId::Text("PreviewImage".into()),
                        name: "PreviewImage".into(),
                        description: "Preview Image".into(),
                        group: TagGroup { family0: "ZIP".into(), family1: "ZIP".into(), family2: "Preview".into() },
                        raw_value: Value::Binary(img_bytes),
                        print_value: preview_str,
                        priority: 0,
                    });
                }
            }
        }
    }

    // Note: ZipSubType is not emitted as it doesn't match Perl ExifTool output
    let _ = has_content_types;
    let _ = has_mimetype;

    Ok(tags)
}

/// Decompress a ZIP entry if needed (supports stored and deflated).
fn decompress_zip_entry(data: &[u8], compression: u16, uncompressed_size: usize) -> Option<Vec<u8>> {
    match compression {
        0 => Some(data.to_vec()), // Stored
        8 => {
            // Deflated
            use std::io::Read;
            let mut decoder = flate2::read::DeflateDecoder::new(data);
            let mut out = Vec::with_capacity(uncompressed_size.min(10 * 1024 * 1024));
            decoder.read_to_end(&mut out).ok()?;
            Some(out)
        }
        _ => None,
    }
}

fn zip_compression_name(method: u16) -> String {
    match method {
        0 => "None".into(),
        1 => "Shrunk".into(),
        2 => "Reduced with compression factor 1".into(),
        3 => "Reduced with compression factor 2".into(),
        4 => "Reduced with compression factor 3".into(),
        5 => "Reduced with compression factor 4".into(),
        6 => "Imploded".into(),
        8 => "Deflated".into(),
        9 => "Enhanced Deflate using Deflate64(tm)".into(),
        12 => "BZIP2".into(),
        14 => "LZMA (EFS)".into(),
        _ => format!("{}", method),
    }
}


fn parse_ooxml_core(data: &[u8], tags: &mut Vec<Tag>) {
    let text = crate::encoding::decode_utf8_or_latin1(data);
    let fields: &[(&str, &str)] = &[
        ("dc:title", "Title"),
        ("dc:creator", "Creator"),
        ("dc:subject", "Subject"),
        ("dc:description", "Description"),
        ("cp:keywords", "Keywords"),
        ("cp:lastModifiedBy", "LastModifiedBy"),
        ("cp:revision", "RevisionNumber"),
        ("dcterms:created", "CreateDate"),
        ("dcterms:modified", "ModifyDate"),
        ("cp:category", "Category"),
        ("cp:contentStatus", "ContentStatus"),
        ("cp:language", "Language"),
        ("cp:version", "Version"),
    ];

    for (xml_tag, name) in fields {
        if let Some(value) = extract_xml_value(&text, xml_tag) {
            let value = if name.contains("Date") {
                convert_ooxml_date(&value)
            } else {
                value
            };
            tags.push(mk(name, name, Value::String(value)));
        }
    }
}

fn parse_ooxml_app(data: &[u8], tags: &mut Vec<Tag>) {
    let text = crate::encoding::decode_utf8_or_latin1(data);
    let fields: &[(&str, &str)] = &[
        ("Template", "Template"),
        ("TotalTime", "TotalEditTime"),
        ("Pages", "Pages"),
        ("Words", "Words"),
        ("Characters", "Characters"),
        ("Application", "Application"),
        ("DocSecurity", "DocSecurity"),
        ("Lines", "Lines"),
        ("Paragraphs", "Paragraphs"),
        ("ScaleCrop", "ScaleCrop"),
        ("Manager", "Manager"),
        ("Company", "Company"),
        ("LinksUpToDate", "LinksUpToDate"),
        ("CharactersWithSpaces", "CharactersWithSpaces"),
        ("SharedDoc", "SharedDoc"),
        ("HyperlinkBase", "HyperlinkBase"),
        ("HyperlinksChanged", "HyperlinksChanged"),
        ("AppVersion", "AppVersion"),
        ("Slides", "Slides"),
        ("Notes", "Notes"),
        ("HiddenSlides", "HiddenSlides"),
        ("MMClips", "MMClips"),
        ("PresentationFormat", "PresentationFormat"),
    ];

    for (xml_tag, name) in fields {
        if let Some(value) = extract_xml_value(&text, xml_tag) {
            // Convert boolean strings to Yes/No, and DocSecurity to text
            let value = convert_ooxml_value(xml_tag, name, &value);
            tags.push(mk(name, name, Value::String(value)));
        }
    }

    // Parse HeadingPairs (vt:vector with variants)
    if let Some(hp) = extract_xml_value(&text, "HeadingPairs") {
        let pairs = parse_vt_vector_pairs(&hp);
        if !pairs.is_empty() {
            tags.push(mk("HeadingPairs", "HeadingPairs", Value::String(pairs)));
        }
    }

    // Parse TitlesOfParts (vt:vector of lpstr)
    if let Some(tp) = extract_xml_value(&text, "TitlesOfParts") {
        let titles = parse_vt_vector_strings(&tp);
        if !titles.is_empty() {
            tags.push(mk("TitlesOfParts", "TitlesOfParts", Value::String(titles)));
        }
    }
}

/// Convert OOXML-specific values (booleans, security flags, etc.)
fn convert_ooxml_value(_xml_tag: &str, tag_name: &str, value: &str) -> String {
    match tag_name {
        "ScaleCrop" | "LinksUpToDate" | "HyperlinksChanged" | "SharedDoc" => {
            match value.to_lowercase().as_str() {
                "true" => "Yes".to_string(),
                "false" => "No".to_string(),
                _ => value.to_string(),
            }
        }
        "DocSecurity" => {
            match value {
                "0" => "None".to_string(),
                "1" => "Password protected".to_string(),
                "2" => "Read-only recommended".to_string(),
                "4" => "Read-only enforced".to_string(),
                "8" => "Locked for annotations".to_string(),
                _ => value.to_string(),
            }
        }
        "TotalEditTime" => {
            // TotalTime is in minutes
            if let Ok(mins) = value.parse::<u64>() {
                if mins == 1 {
                    "1 minute".to_string()
                } else {
                    format!("{} minutes", mins)
                }
            } else {
                value.to_string()
            }
        }
        _ => value.to_string(),
    }
}

/// Parse vt:vector with alternating lpstr/i4 values -> "key, value, key, value, ..."
fn parse_vt_vector_pairs(xml: &str) -> String {
    let mut results = Vec::new();
    let mut rest = xml;
    // Extract lpstr values
    let mut strs = Vec::new();
    while let Some(p) = rest.find("<vt:lpstr>") {
        let after = &rest[p + 10..];
        if let Some(end) = after.find("</vt:lpstr>") {
            strs.push(after[..end].trim().to_string());
            rest = &after[end + 11..];
        } else {
            break;
        }
    }
    // Extract i4 values
    rest = xml;
    let mut nums = Vec::new();
    while let Some(p) = rest.find("<vt:i4>") {
        let after = &rest[p + 7..];
        if let Some(end) = after.find("</vt:i4>") {
            nums.push(after[..end].trim().to_string());
            rest = &after[end + 8..];
        } else {
            break;
        }
    }
    // Interleave
    for (s, n) in strs.iter().zip(nums.iter()) {
        results.push(format!("{}", s));
        results.push(format!("{}", n));
    }
    results.join(", ")
}

/// Parse vt:vector of lpstr -> comma-joined
fn parse_vt_vector_strings(xml: &str) -> String {
    let mut results = Vec::new();
    let mut rest = xml;
    while let Some(p) = rest.find("<vt:lpstr>") {
        let after = &rest[p + 10..];
        if let Some(end) = after.find("</vt:lpstr>") {
            results.push(after[..end].trim().to_string());
            rest = &after[end + 11..];
        } else {
            break;
        }
    }
    results.join(", ")
}

/// Parse docProps/custom.xml for custom properties.
fn parse_ooxml_custom(data: &[u8], tags: &mut Vec<Tag>) {
    let text = crate::encoding::decode_utf8_or_latin1(data);
    let mut rest = text.as_str();

    // Each property: <property ... name="PropName"><vt:TYPE>value</vt:TYPE></property>
    while let Some(p) = rest.find("<property ") {
        let prop_start = &rest[p..];
        let end = prop_start.find("</property>").map(|e| e + 11).unwrap_or(prop_start.len());
        let prop_xml = &prop_start[..end];

        // Extract name attribute
        if let Some(name) = extract_xml_attr(prop_xml, "name") {
            // Extract value from any vt: element
            if let Some(value) = extract_vt_value(prop_xml) {
                // Convert name to ExifTool-style tag name
                let tag_name = name.split_whitespace()
                    .map(|w| {
                        let mut chars = w.chars();
                        match chars.next() {
                            None => String::new(),
                            Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
                        }
                    })
                    .collect::<String>();
                if !tag_name.is_empty() && !value.is_empty() {
                    // Convert date values
                    let value = if prop_xml.contains("vt:filetime") {
                        convert_ooxml_date(&value)
                    } else {
                        value
                    };
                    tags.push(mk(&tag_name, &name, Value::String(value)));
                }
            }
        }

        rest = &rest[p + 10..]; // advance past "<property "
        if rest.len() < 10 { break; }
    }
}

/// Extract an XML attribute value (simple, non-namespace-aware).
fn extract_xml_attr(xml: &str, attr: &str) -> Option<String> {
    let pattern = format!("{}=\"", attr);
    let start = xml.find(&pattern)? + pattern.len();
    let end = xml[start..].find('"')?;
    Some(xml[start..start + end].to_string())
}

/// Extract value from vt:lpwstr, vt:lpstr, vt:i4, vt:r8, vt:bool, vt:filetime, etc.
fn extract_vt_value(xml: &str) -> Option<String> {
    let vt_types = ["lpwstr", "lpstr", "i4", "r8", "bool", "filetime", "date",
                    "i1", "i2", "i8", "ui1", "ui2", "ui4", "ui8", "decimal", "array"];
    for vt in &vt_types {
        let open = format!("<vt:{}>", vt);
        let close = format!("</vt:{}>", vt);
        if let Some(start) = xml.find(&open) {
            let after = &xml[start + open.len()..];
            if let Some(end) = after.find(&close) {
                let val = after[..end].trim().to_string();
                if !val.is_empty() {
                    return Some(val);
                }
            }
        }
    }
    None
}

/// Convert OOXML date "2009-10-24T01:41:00Z" -> "2009:10:24 01:41:00Z"
fn convert_ooxml_date(s: &str) -> String {
    if s.len() >= 19 && s.chars().nth(4) == Some('-') {
        let date = s[..10].replace('-', ":");
        let time_part = &s[11..];
        format!("{} {}", date, time_part)
    } else if s.len() >= 10 && s.chars().nth(4) == Some('-') {
        s[..10].replace('-', ":")
    } else {
        s.to_string()
    }
}

fn extract_xml_value(xml: &str, tag: &str) -> Option<String> {
    // Match exact tag: <tag> or <tag /> or <ns:tag> - require '>' or '/' or ' ' after tag name
    let open_exact = format!("<{}>", tag);
    let open_attr = format!("<{} ", tag);
    let _open_ns_end = format!("<{}/>", tag);
    let close = format!("</{}>", tag);

    let start = if let Some(p) = xml.find(&open_exact) {
        p + open_exact.len() - 1 // point to '>'
    } else if let Some(p) = xml.find(&open_attr) {
        p
    } else {
        return None;
    };

    let after = &xml[start..];
    let content_start = after.find('>')? + 1;
    let content = &after[content_start..];
    let end = content.find(&close)?;
    let value = content[..end].trim().to_string();

    if value.is_empty() { None } else { Some(value) }
}


/// Parse OpenDocument meta.xml and extract metadata tags.
/// The meta.xml format uses namespaced XML elements.
fn parse_opendoc_meta(data: &[u8], tags: &mut Vec<Tag>) {
    let text = crate::encoding::decode_utf8_or_latin1(data);
    let xml = text.as_str();

    // Extract grddl:transformation attribute from root element
    if let Some(trans) = xml_attr(xml, "grddl:transformation") {
        tags.push(mk("Transformation", "Transformation", Value::String(trans)));
    }

    // Parse office:meta section
    // Simple tag mappings: element local-name → ExifTool tag name
    let simple_tags = [
        ("meta:initial-creator", "Initial-creator"),
        ("meta:creation-date", "Creation-date"),
        ("meta:keyword", "Keyword"),
        ("meta:editing-duration", "Editing-duration"),
        ("meta:editing-cycles", "Editing-cycles"),
        ("meta:generator", "Generator"),
        ("dc:title", "Title"),
        ("dc:subject", "Subject"),
        ("dc:description", "Description"),
        ("dc:date", "Date"),
        ("dc:creator", "Creator"),
    ];

    for (elem, tag_name) in &simple_tags {
        if let Some(val) = xml_element_text(xml, elem) {
            // Convert ISO dates to ExifTool format
            let val = if tag_name.ends_with("-date") || *tag_name == "Date" {
                convert_iso_date(&val)
            } else {
                val
            };
            tags.push(mk(tag_name, tag_name, Value::String(val)));
        }
    }

    // meta:document-statistic attributes
    if let Some(stat_pos) = xml.find("<meta:document-statistic") {
        let stat_part = &xml[stat_pos..];
        let elem_end = stat_part.find('>').unwrap_or(stat_part.len());
        let elem_str = &stat_part[..elem_end];

        let stat_attrs = [
            ("meta:table-count", "Document-statisticTable-count"),
            ("meta:cell-count", "Document-statisticCell-count"),
            ("meta:object-count", "Document-statisticObject-count"),
            ("meta:page-count", "Document-statisticPage-count"),
            ("meta:word-count", "Document-statisticWord-count"),
            ("meta:character-count", "Document-statisticCharacter-count"),
        ];
        for (attr, tag_name) in &stat_attrs {
            if let Some(val) = xml_attr(elem_str, attr) {
                tags.push(mk(tag_name, tag_name, Value::String(val)));
            }
        }
    }

    // meta:user-defined elements
    let mut search = xml;
    while let Some(pos) = search.find("<meta:user-defined") {
        let part = &search[pos..];
        // Get meta:name attribute
        let elem_end = part.find('>').unwrap_or(part.len());
        let elem_str = &part[..elem_end];
        let name = xml_attr(elem_str, "meta:name").unwrap_or_default();
        // Get text content
        let content_start = part.find('>').map(|p| p + 1).unwrap_or(part.len());
        let content_end = part.find("</meta:user-defined>").unwrap_or(part.len());
        let value = if content_start < content_end {
            xml_decode_entities(part[content_start..content_end].trim())
        } else {
            String::new()
        };
        if !name.is_empty() {
            tags.push(mk("User-definedName", "User-defined Name", Value::String(name)));
        }
        if !value.is_empty() {
            tags.push(mk("User-defined", "User-defined", Value::String(value)));
        }
        let advance = pos + 19;
        if advance >= search.len() { break; }
        search = &search[advance..];
    }
}

/// Get attribute value from an XML element string.
fn xml_attr(xml: &str, attr: &str) -> Option<String> {
    let search = format!("{}=\"", attr);
    let pos = xml.find(&search)?;
    let after = &xml[pos + search.len()..];
    let end = after.find('"')?;
    Some(xml_decode_entities(&after[..end]))
}

/// Get text content of first matching XML element.
fn xml_element_text(xml: &str, elem: &str) -> Option<String> {
    let open = format!("<{}", elem);
    let close = format!("</{}>", elem);
    let pos = xml.find(&open)?;
    let after_open = &xml[pos..];
    // Find end of opening tag
    let tag_end = after_open.find('>')?;
    // Check for self-closing tag
    if after_open[..tag_end].ends_with('/') {
        return None;
    }
    let content_start = tag_end + 1;
    let close_pos = after_open[content_start..].find(&close)?;
    let content = &after_open[content_start..content_start + close_pos];
    let trimmed = content.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(xml_decode_entities(trimmed))
    }
}

/// Decode basic XML entities.
fn xml_decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
     .replace("&lt;", "<")
     .replace("&gt;", ">")
     .replace("&quot;", "\"")
     .replace("&apos;", "'")
}

/// Convert ISO 8601 date "2010-04-19T11:16:49.13" to ExifTool format "2010:04:19 11:16:49.13"
fn convert_iso_date(s: &str) -> String {
    // Replace first two dashes with colons, and T with space
    let mut result = String::with_capacity(s.len());
    let mut dash_count = 0;
    let mut in_time = false;
    for c in s.chars() {
        if !in_time && c == '-' {
            dash_count += 1;
            if dash_count <= 2 {
                result.push(':');
            } else {
                result.push(c);
            }
        } else if c == 'T' {
            in_time = true;
            result.push(' ');
        } else {
            result.push(c);
        }
    }
    result
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
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
