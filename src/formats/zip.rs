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
        if first_file {
            first_file = false;
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

        // For OOXML: parse docProps/core.xml
        if filename == "docProps/core.xml" {
            let content_start = name_start + name_len + extra_len;
            let content_end = (content_start + compressed_size).min(data.len());
            if compression == 0 && content_start < content_end {
                parse_ooxml_core(&data[content_start..content_end], &mut tags);
            }
        }

        if filename == "docProps/app.xml" {
            let content_start = name_start + name_len + extra_len;
            let content_end = (content_start + compressed_size).min(data.len());
            if compression == 0 && content_start < content_end {
                parse_ooxml_app(&data[content_start..content_end], &mut tags);
            }
        }

        if file_count < 50 {
            filenames.push(filename);
        }
        file_count += 1;
        total_size += uncompressed_size;

        pos = name_start + name_len + extra_len + compressed_size;
    }

    // Determine document type
    if has_content_types {
        let doc_type = detect_ooxml_type(&filenames);
        tags.push(mk("ZipSubType", "Document Type", Value::String(doc_type)));
    } else if has_mimetype {
        tags.push(mk("ZipSubType", "Document Type", Value::String("OpenDocument".into())));
    }

    Ok(tags)
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
    let fields = [
        ("dc:title", "Title"),
        ("dc:creator", "Creator"),
        ("dc:subject", "Subject"),
        ("dc:description", "Description"),
        ("cp:keywords", "Keywords"),
        ("cp:lastModifiedBy", "LastModifiedBy"),
        ("cp:revision", "Revision"),
        ("dcterms:created", "CreateDate"),
        ("dcterms:modified", "ModifyDate"),
        ("cp:category", "Category"),
    ];

    for (xml_tag, name) in &fields {
        if let Some(value) = extract_xml_value(&text, xml_tag) {
            tags.push(mk(name, name, Value::String(value)));
        }
    }
}

fn parse_ooxml_app(data: &[u8], tags: &mut Vec<Tag>) {
    let text = String::from_utf8_lossy(data);
    let fields = [
        ("Application", "Application"),
        ("AppVersion", "AppVersion"),
        ("Company", "Company"),
        ("Pages", "Pages"),
        ("Words", "Words"),
        ("Characters", "Characters"),
        ("Slides", "Slides"),
        ("TotalTime", "TotalEditingTime"),
    ];

    for (xml_tag, name) in &fields {
        if let Some(value) = extract_xml_value(&text, xml_tag) {
            tags.push(mk(name, name, Value::String(value)));
        }
    }
}

fn extract_xml_value(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}", tag);
    let close = format!("</{}", tag);

    let start = xml.find(&open)?;
    let after_open = &xml[start..];
    let content_start = after_open.find('>')? + 1;
    let content = &after_open[content_start..];
    let end = content.find(&close)?;
    let value = content[..end].trim().to_string();

    if value.is_empty() { None } else { Some(value) }
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 { format!("{} bytes", bytes) }
    else if bytes < 1024 * 1024 { format!("{:.1} kB", bytes as f64 / 1024.0) }
    else { format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0)) }
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
