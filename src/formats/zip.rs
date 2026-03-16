//! ZIP archive and ZIP-based document reader.
//!
//! Handles ZIP archives, and Office Open XML (DOCX/XLSX/PPTX)
//! and OpenDocument (ODS/ODT) formats that are ZIP containers.
//! Mirrors ExifTool's ZIP.pm, OOXML.pm, and OpenDocument.pm.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_zip(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 30 || !data.starts_with(&[0x50, 0x4B, 0x03, 0x04]) {
        return Err(Error::InvalidData("not a ZIP file".into()));
    }

    let mut tags = Vec::new();
    let mut file_count = 0u32;
    let mut total_size = 0u64;
    let mut has_content_types = false;
    let mut has_mimetype = false;
    let mut filenames = Vec::new();

    // Parse local file headers
    let mut pos = 0;
    while pos + 30 <= data.len() && data[pos..pos + 4] == [0x50, 0x4B, 0x03, 0x04] {
        let compressed_size = u32::from_le_bytes([data[pos + 18], data[pos + 19], data[pos + 20], data[pos + 21]]) as usize;
        let uncompressed_size = u32::from_le_bytes([data[pos + 22], data[pos + 23], data[pos + 24], data[pos + 25]]) as u64;
        let name_len = u16::from_le_bytes([data[pos + 26], data[pos + 27]]) as usize;
        let extra_len = u16::from_le_bytes([data[pos + 28], data[pos + 29]]) as usize;

        let name_start = pos + 30;
        if name_start + name_len > data.len() {
            break;
        }
        let filename = String::from_utf8_lossy(&data[name_start..name_start + name_len]).to_string();

        if filename == "[Content_Types].xml" {
            has_content_types = true;
        }
        if filename == "mimetype" {
            has_mimetype = true;
            // Read mimetype content (usually stored uncompressed as first file)
            let content_start = name_start + name_len + extra_len;
            let content_end = (content_start + compressed_size).min(data.len());
            if content_start < content_end {
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
            // Only parse if stored (not compressed) - compression method at offset 8
            let method = u16::from_le_bytes([data[pos + 8], data[pos + 9]]);
            if method == 0 && content_start < content_end {
                parse_ooxml_core(&data[content_start..content_end], &mut tags);
            }
        }

        if filename == "docProps/app.xml" {
            let content_start = name_start + name_len + extra_len;
            let content_end = (content_start + compressed_size).min(data.len());
            let method = u16::from_le_bytes([data[pos + 8], data[pos + 9]]);
            if method == 0 && content_start < content_end {
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
        // OOXML document
        let doc_type = detect_ooxml_type(&filenames);
        tags.push(mk("ZipSubType", "Document Type", Value::String(doc_type)));
    } else if has_mimetype {
        tags.push(mk("ZipSubType", "Document Type", Value::String("OpenDocument".into())));
    }

    tags.push(mk("ZipFileCount", "File Count", Value::U32(file_count)));
    tags.push(mk("ZipTotalSize", "Total Uncompressed Size", Value::String(format_size(total_size))));

    Ok(tags)
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
