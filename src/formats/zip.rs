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
        let filename = String::from_utf8_lossy(&data[name_start..name_start + name_len]).to_string();

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

pub fn read_zip(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 30 || !data.starts_with(&[0x50, 0x4B, 0x03, 0x04]) {
        return Err(Error::InvalidData("not a ZIP file".into()));
    }

    // Check for iWork format by scanning filenames in the ZIP.
    // iWork files have index.xml/index.apxl but NOT [Content_Types].xml (OOXML).
    {
        let mut pos = 0usize;
        let mut has_index_xml = false;
        let mut has_content_types = false;
        let mut scan_count = 0usize;
        while pos + 30 <= data.len() && data[pos..pos + 4] == [0x50, 0x4B, 0x03, 0x04] && scan_count < 20 {
            let name_len = u16::from_le_bytes([data[pos + 26], data[pos + 27]]) as usize;
            let extra_len = u16::from_le_bytes([data[pos + 28], data[pos + 29]]) as usize;
            let compressed_size = u32::from_le_bytes([data[pos + 18], data[pos + 19], data[pos + 20], data[pos + 21]]) as usize;
            let name_start = pos + 30;
            if name_start + name_len > data.len() { break; }
            let filename = std::str::from_utf8(&data[name_start..name_start + name_len]).unwrap_or("");
            if filename == "index.xml" || filename == "index.apxl" {
                has_index_xml = true;
            }
            if filename == "[Content_Types].xml" {
                has_content_types = true;
                break; // definitely OOXML, not iWork
            }
            pos = name_start + name_len + extra_len + compressed_size;
            scan_count += 1;
        }
        if has_index_xml && !has_content_types {
            return read_iwork(data);
        }
    }

    // Pre-scan: check if this is an OpenDocument file (has "mimetype" as first file with oasis MIME)
    let is_opendoc = {
        let mut found = false;
        if data.len() > 30 && data[0..4] == [0x50, 0x4B, 0x03, 0x04] {
            let compression = u16::from_le_bytes([data[8], data[9]]);
            let compressed_size = u32::from_le_bytes([data[18], data[19], data[20], data[21]]) as usize;
            let name_len = u16::from_le_bytes([data[26], data[27]]) as usize;
            let extra_len = u16::from_le_bytes([data[28], data[29]]) as usize;
            let name_start = 30;
            if name_start + name_len <= data.len() {
                let filename = std::str::from_utf8(&data[name_start..name_start + name_len]).unwrap_or("");
                if filename == "mimetype" && compression == 0 {
                    let content_start = name_start + name_len + extra_len;
                    let content_end = (content_start + compressed_size).min(data.len());
                    if content_start < content_end {
                        let mime = std::str::from_utf8(&data[content_start..content_end]).unwrap_or("").trim();
                        if mime.starts_with("application/vnd.oasis.opendocument") {
                            found = true;
                        }
                    }
                }
            }
        }
        found
    };

    let mut tags = Vec::new();
    let mut file_count = 0u32;
    let mut total_size = 0u64;
    let mut has_content_types = false;
    let mut has_mimetype = false;
    let mut filenames = Vec::new();
    let mut first_file = true;

    // Parse local file headers
    let mut pos = 0;
    while pos + 30 <= data.len() && data[pos..pos + 4] == [0x50, 0x4B, 0x03, 0x04] {
        let bit_flag = u16::from_le_bytes([data[pos + 6], data[pos + 7]]);
        let compression = u16::from_le_bytes([data[pos + 8], data[pos + 9]]);
        let mod_time = u16::from_le_bytes([data[pos + 10], data[pos + 11]]);
        let mod_date = u16::from_le_bytes([data[pos + 12], data[pos + 13]]);
        let crc = u32::from_le_bytes([data[pos + 14], data[pos + 15], data[pos + 16], data[pos + 17]]);
        let compressed_size = u32::from_le_bytes([data[pos + 18], data[pos + 19], data[pos + 20], data[pos + 21]]) as usize;
        let uncompressed_size = u32::from_le_bytes([data[pos + 22], data[pos + 23], data[pos + 24], data[pos + 25]]) as u64;
        let name_len = u16::from_le_bytes([data[pos + 26], data[pos + 27]]) as usize;
        let extra_len = u16::from_le_bytes([data[pos + 28], data[pos + 29]]) as usize;
        let required_version = u16::from_le_bytes([data[pos + 4], data[pos + 5]]);

        let name_start = pos + 30;
        if name_start + name_len > data.len() {
            break;
        }
        let filename = String::from_utf8_lossy(&data[name_start..name_start + name_len]).to_string();

        // Emit per-file tags for the first file (matching Perl behavior)
        // Skip for OpenDocument files (they don't emit Zip* tags)
        let emit_zip_tags = first_file && !is_opendoc;
        if first_file { first_file = false; }
        if emit_zip_tags {
            tags.push(mk("ZipRequiredVersion", "Required Version", Value::U16(required_version)));
            let bit_flag_str = if bit_flag != 0 {
                format!("0x{:04x}", bit_flag)
            } else {
                bit_flag.to_string()
            };
            tags.push(mk("ZipBitFlag", "Bit Flag", Value::String(bit_flag_str)));
            let compression_str = zip_compression_name(compression);
            tags.push(mk("ZipCompression", "Compression", Value::String(compression_str)));
            // DOS date/time to EXIF-style string
            let year = ((mod_date >> 9) & 0x7F) as u32 + 1980;
            let month = ((mod_date >> 5) & 0x0F) as u32;
            let day = (mod_date & 0x1F) as u32;
            let hour = ((mod_time >> 11) & 0x1F) as u32;
            let minute = ((mod_time >> 5) & 0x3F) as u32;
            let second = ((mod_time & 0x1F) as u32) * 2;
            let date_str = format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
                year, month, day, hour, minute, second);
            tags.push(mk("ZipModifyDate", "Modify Date", Value::String(date_str)));
            let crc_str = format!("0x{:08x}", crc);
            tags.push(mk("ZipCRC", "CRC", Value::String(crc_str)));
            tags.push(mk("ZipCompressedSize", "Compressed Size", Value::U32(compressed_size as u32)));
            tags.push(mk("ZipUncompressedSize", "Uncompressed Size", Value::U32(uncompressed_size as u32)));
            tags.push(mk("ZipFileName", "File Name", Value::String(filename.clone())));
        }

        if filename == "[Content_Types].xml" {
            has_content_types = true;
        }
        if filename == "mimetype" {
            has_mimetype = true;
            let content_start = name_start + name_len + extra_len;
            let content_end = (content_start + compressed_size).min(data.len());
            if content_start < content_end && compression == 0 {
                let mime = String::from_utf8_lossy(&data[content_start..content_end])
                    .trim()
                    .to_string();
                tags.push(mk("MIMEType", "MIME Type", Value::String(mime)));
            }
        }

        // OpenDocument: parse meta.xml for metadata
        if filename == "meta.xml" {
            let content_start = name_start + name_len + extra_len;
            let content_end = (content_start + compressed_size).min(data.len());
            if content_start < content_end {
                if let Some(xml_bytes) = decompress_zip_entry(&data[content_start..content_end], compression, uncompressed_size as usize) {
                    parse_opendoc_meta(&xml_bytes, &mut tags);
                }
            }
        }

        // OpenDocument: thumbnail PNG
        if filename == "Thumbnails/thumbnail.png" {
            let content_start = name_start + name_len + extra_len;
            let content_end = (content_start + compressed_size).min(data.len());
            if content_start < content_end {
                if let Some(img_bytes) = decompress_zip_entry(&data[content_start..content_end], compression, uncompressed_size as usize) {
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
            let content_start = name_start + name_len + extra_len;
            let content_end = (content_start + compressed_size).min(data.len());
            if content_start < content_end {
                if let Some(xml_bytes) = decompress_zip_entry(&data[content_start..content_end], compression, uncompressed_size as usize) {
                    parse_ooxml_core(&xml_bytes, &mut tags);
                }
            }
        }

        if filename == "docProps/app.xml" {
            let content_start = name_start + name_len + extra_len;
            let content_end = (content_start + compressed_size).min(data.len());
            if content_start < content_end {
                if let Some(xml_bytes) = decompress_zip_entry(&data[content_start..content_end], compression, uncompressed_size as usize) {
                    parse_ooxml_app(&xml_bytes, &mut tags);
                }
            }
        }

        if filename == "docProps/custom.xml" {
            let content_start = name_start + name_len + extra_len;
            let content_end = (content_start + compressed_size).min(data.len());
            if content_start < content_end {
                if let Some(xml_bytes) = decompress_zip_entry(&data[content_start..content_end], compression, uncompressed_size as usize) {
                    parse_ooxml_custom(&xml_bytes, &mut tags);
                }
            }
        }

        // Extract PreviewImage from thumbnail
        if filename == "docProps/thumbnail.jpeg" || filename == "docProps/thumbnail.jpg"
            || filename == "docProps/thumbnail.png" {
            let content_start = name_start + name_len + extra_len;
            let content_end = (content_start + compressed_size).min(data.len());
            if content_start < content_end {
                if let Some(img_bytes) = decompress_zip_entry(&data[content_start..content_end], compression, uncompressed_size as usize) {
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

        if file_count < 50 {
            filenames.push(filename);
        }
        file_count += 1;
        total_size += uncompressed_size;

        pos = name_start + name_len + extra_len + compressed_size;
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

fn detect_ooxml_type(filenames: &[String]) -> String {
    for name in filenames {
        if name.starts_with("word/") { return "DOCX (Microsoft Word)".into(); }
        if name.starts_with("xl/") { return "XLSX (Microsoft Excel)".into(); }
        if name.starts_with("ppt/") { return "PPTX (Microsoft PowerPoint)".into(); }
        if name.starts_with("visio/") { return "VSDX (Microsoft Visio)".into(); }
    }
    "Office Open XML".into()
}

fn parse_ooxml_core(data: &[u8], tags: &mut Vec<Tag>) {
    let text = String::from_utf8_lossy(data);
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
    let text = String::from_utf8_lossy(data);
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
    let text = String::from_utf8_lossy(data);
    let mut rest = text.as_ref();

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

fn format_size(bytes: u64) -> String {
    if bytes < 1024 { format!("{} bytes", bytes) }
    else if bytes < 1024 * 1024 { format!("{:.1} kB", bytes as f64 / 1024.0) }
    else { format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0)) }
}

/// Parse OpenDocument meta.xml and extract metadata tags.
/// The meta.xml format uses namespaced XML elements.
fn parse_opendoc_meta(data: &[u8], tags: &mut Vec<Tag>) {
    let text = String::from_utf8_lossy(data);
    let xml = text.as_ref();

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
