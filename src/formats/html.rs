//! HTML metadata reader.
//!
//! Extracts `<meta>` tags, `<title>`, and embedded XMP from HTML files.

use crate::error::{Error, Result};
use crate::metadata::XmpReader;
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_html(data: &[u8]) -> Result<Vec<Tag>> {
    let text = String::from_utf8_lossy(data);
    let lower = text.to_lowercase();

    if !lower.contains("<html") && !lower.contains("<!doctype html") {
        return Err(Error::InvalidData("not an HTML file".into()));
    }

    let mut tags = Vec::new();

    // Extract <title>
    if let Some(start) = lower.find("<title") {
        let rest = &text[start..];
        if let Some(close) = rest.find('>') {
            let after = &rest[close + 1..];
            if let Some(end) = after.to_lowercase().find("</title") {
                let title = after[..end].trim().to_string();
                if !title.is_empty() {
                    tags.push(mk("Title", "Title", Value::String(title)));
                }
            }
        }
    }

    // Extract <meta> tags
    let mut search_pos = 0;
    while let Some(meta_pos) = lower[search_pos..].find("<meta") {
        let abs_pos = search_pos + meta_pos;
        let rest = &text[abs_pos..];
        let end = rest.find('>').unwrap_or(rest.len());
        let meta_tag = &rest[..end];

        let name = extract_attr(meta_tag, "name")
            .or_else(|| extract_attr(meta_tag, "property"))
            .or_else(|| extract_attr(meta_tag, "http-equiv"));
        let content = extract_attr(meta_tag, "content");

        if let (Some(name), Some(content)) = (name, content) {
            if !name.is_empty() && !content.is_empty() {
                let tag_name = name.replace([':', '.', '-'], "");
                tags.push(mk(&tag_name, &name, Value::String(content)));
            }
        }

        search_pos = abs_pos + end.max(5);
    }

    // Look for embedded XMP
    if let Some(xmp_start) = find_bytes(data, b"<?xpacket begin") {
        if let Some(xmp_end) = find_bytes(&data[xmp_start..], b"<?xpacket end") {
            let end = xmp_start + xmp_end + 20;
            if end <= data.len() {
                if let Ok(xmp_tags) = XmpReader::read(&data[xmp_start..end]) {
                    tags.extend(xmp_tags);
                }
            }
        }
    }

    Ok(tags)
}

fn extract_attr(tag: &str, attr_name: &str) -> Option<String> {
    let lower = tag.to_lowercase();
    let pattern = format!("{}=", attr_name);
    let pos = lower.find(&pattern)?;
    let rest = &tag[pos + pattern.len()..];
    let rest = rest.trim_start();

    if rest.starts_with('"') {
        let end = rest[1..].find('"')?;
        Some(rest[1..1 + end].to_string())
    } else if rest.starts_with('\'') {
        let end = rest[1..].find('\'')?;
        Some(rest[1..1 + end].to_string())
    } else {
        let end = rest.find(|c: char| c.is_whitespace() || c == '>' || c == '/').unwrap_or(rest.len());
        Some(rest[..end].to_string())
    }
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
        group: TagGroup { family0: "HTML".into(), family1: "HTML".into(), family2: "Document".into() },
        raw_value: value, print_value: pv, priority: 0,
    }
}
