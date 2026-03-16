//! PostScript/EPS/AI file format reader.
//!
//! Parses DSC (Document Structuring Convention) comments for metadata.
//! Mirrors ExifTool's PostScript.pm.

use crate::error::{Error, Result};
use crate::metadata::XmpReader;
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_postscript(data: &[u8]) -> Result<Vec<Tag>> {
    let mut tags = Vec::new();
    let mut offset = 0;

    // DOS EPS binary header: C5 D0 D3 C6
    if data.len() >= 30 && data.starts_with(&[0xC5, 0xD0, 0xD3, 0xC6]) {
        let ps_offset = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
        let ps_length = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize;

        if ps_offset + ps_length <= data.len() {
            offset = ps_offset;
        }
        tags.push(mk("EPSFormat", "EPS Format", Value::String("DOS Binary".into())));
    }

    // Check for PS magic
    if offset + 4 > data.len() || (!data[offset..].starts_with(b"%!PS") && !data[offset..].starts_with(b"%!Ad")) {
        return Err(Error::InvalidData("not a PostScript file".into()));
    }

    // Parse DSC comments line by line (handle \r, \n, and \r\n)
    let text = String::from_utf8_lossy(&data[offset..data.len().min(offset + 65536)]);
    let text = text.replace('\r', "\n");

    for line in text.lines() {
        if !line.starts_with("%%") && !line.starts_with("%!") {
            // Stop at first non-comment non-DSC line (after header section)
            if !line.starts_with('%') && !line.is_empty() {
                break;
            }
            continue;
        }

        let line = line.trim();

        if let Some(rest) = line.strip_prefix("%%Title:") {
            tags.push(mk("Title", "Title", Value::String(rest.trim().trim_matches('(').trim_matches(')').to_string())));
        } else if let Some(rest) = line.strip_prefix("%%Creator:") {
            tags.push(mk("Creator", "Creator", Value::String(rest.trim().trim_matches('(').trim_matches(')').to_string())));
        } else if let Some(rest) = line.strip_prefix("%%CreationDate:") {
            tags.push(mk("CreateDate", "Create Date", Value::String(rest.trim().trim_matches('(').trim_matches(')').to_string())));
        } else if let Some(rest) = line.strip_prefix("%%For:") {
            tags.push(mk("Author", "Author", Value::String(rest.trim().trim_matches('(').trim_matches(')').to_string())));
        } else if let Some(rest) = line.strip_prefix("%%BoundingBox:") {
            tags.push(mk("BoundingBox", "Bounding Box", Value::String(rest.trim().to_string())));
        } else if let Some(rest) = line.strip_prefix("%%HiResBoundingBox:") {
            tags.push(mk("HiResBoundingBox", "HiRes Bounding Box", Value::String(rest.trim().to_string())));
        } else if let Some(rest) = line.strip_prefix("%%Pages:") {
            tags.push(mk("Pages", "Pages", Value::String(rest.trim().to_string())));
        } else if let Some(rest) = line.strip_prefix("%%LanguageLevel:") {
            tags.push(mk("LanguageLevel", "Language Level", Value::String(rest.trim().to_string())));
        } else if let Some(rest) = line.strip_prefix("%%DocumentData:") {
            tags.push(mk("DocumentData", "Document Data", Value::String(rest.trim().to_string())));
        } else if line.starts_with("%!PS-Adobe-") {
            let version = line.strip_prefix("%!PS-Adobe-").unwrap_or("").trim();
            tags.push(mk("PSVersion", "PostScript Version", Value::String(version.to_string())));
            // Check for EPS
            if version.contains("EPSF") {
                tags.push(mk("EPSVersion", "EPS Version", Value::String(version.to_string())));
            }
        }
    }

    // Look for embedded XMP
    if let Some(xmp_start) = find_bytes(&data[offset..], b"<?xpacket begin") {
        let xmp_data = &data[offset + xmp_start..];
        if let Some(xmp_end) = find_bytes(xmp_data, b"<?xpacket end") {
            let end = xmp_end + 20; // Include the end tag
            if let Ok(xmp_tags) = XmpReader::read(&xmp_data[..end.min(xmp_data.len())]) {
                tags.extend(xmp_tags);
            }
        }
    }

    Ok(tags)
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "PostScript".into(),
            family1: "PostScript".into(),
            family2: "Document".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}
