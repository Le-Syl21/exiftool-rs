//! DjVu file format reader.
//!
//! Parses IFF-based DjVu files to extract metadata.
//! Mirrors ExifTool's DjVu.pm.

use crate::error::{Error, Result};
use crate::metadata::XmpReader;
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_djvu(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 16 || !data.starts_with(b"AT&TFORM") {
        return Err(Error::InvalidData("not a DjVu file".into()));
    }

    let mut tags = Vec::new();
    let form_type = &data[12..16];

    // Determine subfile type
    let subfile_type = match form_type {
        b"DJVU" => "Single-page image",
        b"DJVM" => "Multi-page document",
        b"PM44" => "Color IW44",
        b"BM44" => "Grayscale IW44",
        b"DJVI" => "Shared component",
        b"THUM" => "Thumbnail image",
        _ => "",
    };
    if !subfile_type.is_empty() {
        tags.push(mk("SubfileType", "Subfile Type", Value::String(subfile_type.into())));
    }

    // Parse chunks
    let pos = 16;
    parse_chunks(data, pos, data.len(), &mut tags);

    Ok(tags)
}

fn parse_chunks(data: &[u8], mut pos: usize, end: usize, tags: &mut Vec<Tag>) {
    while pos + 8 <= end {
        let chunk_id = &data[pos..pos + 4];
        let chunk_size = u32::from_be_bytes([
            data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7],
        ]) as usize;
        pos += 8;

        let chunk_end = pos + chunk_size;
        if chunk_end > end {
            break;
        }

        let chunk_data = &data[pos..chunk_end];

        match chunk_id {
            b"INFO" => parse_info(chunk_data, tags),
            b"FORM" => {
                // FORM chunk contains a type and nested chunks
                if chunk_data.len() >= 4 {
                    let sub_type = &chunk_data[..4];
                    // SubfileType from form type
                    let subfile_str = match sub_type {
                        b"DJVU" => "Single-page image",
                        b"DJVM" => "Multi-page document",
                        b"PM44" => "Color IW44",
                        b"BM44" => "Grayscale IW44",
                        b"DJVI" => "Shared component",
                        b"THUM" => "Thumbnail image",
                        _ => "",
                    };
                    if !subfile_str.is_empty() {
                        tags.push(mk("SubfileType", "Subfile Type", Value::String(subfile_str.into())));
                    }
                    parse_chunks(data, pos + 4, chunk_end, tags);
                }
            }
            b"ANTa" => parse_ant(chunk_data, tags),
            b"ANTz" => {
                // BZZ compressed annotation - we don't have a BZZ decompressor
                // so we skip this for now
            }
            b"INCL" => {
                // Included file ID
                let id = String::from_utf8_lossy(chunk_data).trim_end_matches('\0').to_string();
                if !id.is_empty() {
                    tags.push(mk("IncludedFileID", "Included File ID", Value::String(id)));
                }
            }
            b"NDIR" => {
                // Bundled multi-page document directory - skip
            }
            _ => {}
        }

        pos = chunk_end;
        if chunk_size % 2 != 0 {
            pos += 1;
        }
    }
}

fn parse_info(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 10 {
        return;
    }
    let width = u16::from_be_bytes([data[0], data[1]]);
    let height = u16::from_be_bytes([data[2], data[3]]);

    // DjVu version: bytes 4 and 5 (minor, major)
    let minor = data[4];
    let major = data[5];
    let version_str = format!("{}.{}", major, minor);

    // Spatial resolution: little-endian uint16 at offset 6
    let dpi = u16::from_le_bytes([data[6], data[7]]);

    // Gamma at offset 8: uint8, value = gamma * 10
    let gamma = data[8] as f64 / 10.0;

    // Orientation at offset 9: lower 3 bits
    let orientation = if data.len() > 9 { data[9] & 0x07 } else { 0 };

    tags.push(mk("ImageWidth", "Image Width", Value::U16(width)));
    tags.push(mk("ImageHeight", "Image Height", Value::U16(height)));
    tags.push(mk("DjVuVersion", "DjVu Version", Value::String(version_str)));
    tags.push(mk("SpatialResolution", "Spatial Resolution", Value::U16(dpi)));

    if gamma > 0.0 {
        tags.push(mk("Gamma", "Gamma", Value::String(format!("{:.1}", gamma))));
    }

    let orient_str = match orientation {
        1 => "Horizontal (normal)",
        2 => "Rotate 180",
        5 => "Rotate 90 CW",
        6 => "Rotate 270 CW",
        _ => "Unknown (0)",
    };
    tags.push(mk("Orientation", "Orientation", Value::String(orient_str.into())));
}

/// Parse DjVu ANTa annotation chunk (s-expression format)
fn parse_ant(data: &[u8], tags: &mut Vec<Tag>) {
    let text = String::from_utf8_lossy(data);
    let text = text.as_ref();

    // Look for (metadata ...) block
    if let Some(meta_start) = find_sexpr(text, "metadata") {
        parse_meta_sexpr(&text[meta_start..], tags);
    }

    // Look for (xmp ...) block
    if let Some(xmp_start) = find_sexpr(text, "xmp") {
        if let Some(xmp_content) = extract_sexpr_string(&text[xmp_start..]) {
            if let Ok(xmp_tags) = XmpReader::read(xmp_content.as_bytes()) {
                tags.extend(xmp_tags);
            }
        }
    }

    // Look for (url ...) blocks
    let mut search = text;
    let mut url_start = 0;
    while let Some(pos) = search.find("(url ") {
        let from = url_start + pos;
        if let Some(url) = extract_sexpr_value(&text[from..], "url") {
            tags.push(mk("URL", "URL", Value::String(url)));
        }
        let advance = pos + 5;
        if advance >= search.len() {
            break;
        }
        search = &search[advance..];
        url_start += advance;
    }
}

fn find_sexpr(text: &str, name: &str) -> Option<usize> {
    let search = format!("({}", name);
    let mut pos = 0;
    while let Some(p) = text[pos..].find(&search) {
        let abs = pos + p;
        // Check that after the name comes a space, (, or "
        let after = abs + search.len();
        if after >= text.len() || matches!(text.as_bytes()[after], b' ' | b'\t' | b'\n' | b'\r' | b'"' | b'(') {
            return Some(abs);
        }
        pos = abs + 1;
    }
    None
}

fn extract_sexpr_value(text: &str, name: &str) -> Option<String> {
    let search = format!("({} ", name);
    if let Some(p) = text.find(&search) {
        let after = p + search.len();
        extract_sexpr_string(&text[after..])
    } else {
        None
    }
}

fn extract_sexpr_string(text: &str) -> Option<String> {
    let text = text.trim_start();
    if text.starts_with('"') {
        // Parse quoted string
        let mut result = String::new();
        let mut chars = text[1..].chars();
        loop {
            match chars.next() {
                None => break,
                Some('"') => return Some(result),
                Some('\\') => {
                    match chars.next() {
                        Some('n') => result.push('\n'),
                        Some('r') => result.push('\r'),
                        Some('t') => result.push('\t'),
                        Some('"') => result.push('"'),
                        Some('\\') => result.push('\\'),
                        Some(c) => { result.push('\\'); result.push(c); }
                        None => break,
                    }
                }
                Some(c) => result.push(c),
            }
        }
        Some(result)
    } else {
        // Unquoted token - read until whitespace or )
        let end = text.find(|c: char| c.is_whitespace() || c == ')').unwrap_or(text.len());
        if end > 0 {
            Some(text[..end].to_string())
        } else {
            None
        }
    }
}

fn parse_meta_sexpr(text: &str, tags: &mut Vec<Tag>) {
    // Find (metadata ...)
    // Parse inner s-expressions as key/value pairs
    // Format: (metadata (key "value") (key "value") ...)
    let search = "(metadata";
    if let Some(p) = text.find(search) {
        let after = p + search.len();
        // skip whitespace after "metadata"
        let inner = &text[after..];
        // Parse each (key "value") pair
        parse_meta_pairs(inner, tags);
    }
}

fn parse_meta_pairs(text: &str, tags: &mut Vec<Tag>) {
    let mut pos = 0;
    let bytes = text.as_bytes();

    while pos < bytes.len() {
        // Skip whitespace
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= bytes.len() {
            break;
        }
        if bytes[pos] == b')' {
            break; // End of metadata block
        }
        if bytes[pos] != b'(' {
            pos += 1;
            continue;
        }
        pos += 1; // skip '('

        // Read key
        let key_start = pos;
        while pos < bytes.len() && !bytes[pos].is_ascii_whitespace() && bytes[pos] != b')' {
            pos += 1;
        }
        let key = &text[key_start..pos];
        if key.is_empty() {
            continue;
        }

        // Skip whitespace
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }

        // Read value (could be quoted or unquoted)
        if pos >= bytes.len() {
            break;
        }

        let value = if bytes[pos] == b'"' {
            pos += 1;
            let mut s = String::new();
            loop {
                if pos >= bytes.len() {
                    break;
                }
                if bytes[pos] == b'"' {
                    pos += 1;
                    break;
                }
                if bytes[pos] == b'\\' && pos + 1 < bytes.len() {
                    pos += 1;
                    match bytes[pos] {
                        b'n' => s.push('\n'),
                        b'r' => s.push('\r'),
                        b't' => s.push('\t'),
                        b'"' => s.push('"'),
                        b'\\' => s.push('\\'),
                        c => { s.push('\\'); s.push(c as char); }
                    }
                } else {
                    s.push(bytes[pos] as char);
                }
                pos += 1;
            }
            s
        } else {
            let vstart = pos;
            while pos < bytes.len() && bytes[pos] != b')' && !bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }
            text[vstart..pos].to_string()
        };

        // Skip to closing ')'
        while pos < bytes.len() && bytes[pos] != b')' {
            pos += 1;
        }
        if pos < bytes.len() {
            pos += 1; // skip ')'
        }

        // Map key to tag name
        let tag_name = djvu_meta_tag_name(key);
        if !value.is_empty() && !tag_name.is_empty() {
            tags.push(mk(&tag_name, &tag_name, Value::String(value)));
        }
    }
}

fn djvu_meta_tag_name(key: &str) -> String {
    match key {
        "author" => "Author".into(),
        "title" => "Title".into(),
        "subject" => "Subject".into(),
        "keywords" => "Keywords".into(),
        "creator" | "Creator" => "Creator".into(),
        "producer" | "Producer" => "Producer".into(),
        "CreationDate" => "CreateDate".into(),
        "ModDate" => "ModifyDate".into(),
        "note" => "Note".into(),
        "notes" | "Notes" => "Notes".into(),
        "annote" => "Annotation".into(),
        "year" => "Year".into(),
        "publisher" => "Publisher".into(),
        "journal" => "Journal".into(),
        "booktitle" => "BookTitle".into(),
        "url" => "URL".into(),
        "description" | "Description" => "Description".into(),
        "rights" | "Rights" => "Rights".into(),
        "Trapped" => "Trapped".into(),
        "CreatorTool" => "CreatorTool".into(),
        // PDF-style tags (capitalized)
        "Title" => "Title".into(),
        "Author" => "Author".into(),
        "Subject" => "Subject".into(),
        "Keywords" => "Keywords".into(),
        "ModifyDate" => "ModifyDate".into(),
        "CreateDate" => "CreateDate".into(),
        // Fallback: capitalize first letter
        s => {
            let mut chars = s.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().to_string() + chars.as_str(),
            }
        }
    }
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "DjVu".into(),
            family1: "DjVu".into(),
            family2: "Image".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}
