//! Font file reader (TrueType, OpenType, WOFF, WOFF2).
//!
//! Extracts font name table entries.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_font(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 12 {
        return Err(Error::InvalidData("file too small for font".into()));
    }

    let mut tags = Vec::new();

    // Detect font type
    if data.starts_with(b"wOFF") {
        return read_woff(data);
    }
    if data.starts_with(b"wOF2") {
        tags.push(mk("FontFormat", "Font Format", Value::String("WOFF2".into())));
        return Ok(tags);
    }
    if data.starts_with(b"OTTO") {
        tags.push(mk("FontFormat", "Font Format", Value::String("OpenType/CFF".into())));
    } else if data.starts_with(&[0x00, 0x01, 0x00, 0x00]) || data.starts_with(b"true") || data.starts_with(b"typ1") {
        tags.push(mk("FontFormat", "Font Format", Value::String("TrueType".into())));
    } else if data.starts_with(b"ttcf") {
        tags.push(mk("FontFormat", "Font Format", Value::String("TrueType Collection".into())));
        let num_fonts = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
        tags.push(mk("FontCount", "Font Count", Value::U32(num_fonts)));
        return Ok(tags);
    } else {
        return Err(Error::InvalidData("unknown font format".into()));
    }

    // Parse sfnt table directory
    let num_tables = u16::from_be_bytes([data[4], data[5]]) as usize;
    let mut pos = 12;

    for _ in 0..num_tables {
        if pos + 16 > data.len() {
            break;
        }
        let tag = &data[pos..pos + 4];
        let offset = u32::from_be_bytes([data[pos + 8], data[pos + 9], data[pos + 10], data[pos + 11]]) as usize;
        let length = u32::from_be_bytes([data[pos + 12], data[pos + 13], data[pos + 14], data[pos + 15]]) as usize;
        pos += 16;

        if tag == b"name" && offset + length <= data.len() {
            parse_name_table(&data[offset..offset + length], &mut tags);
        }
        if tag == b"head" && offset + 54 <= data.len() {
            let d = &data[offset..];
            let units_per_em = u16::from_be_bytes([d[18], d[19]]);
            tags.push(mk("UnitsPerEm", "Units Per Em", Value::U16(units_per_em)));
            // Created date (Mac epoch at offset 20, 8 bytes)
            let created = i64::from_be_bytes([d[20], d[21], d[22], d[23], d[24], d[25], d[26], d[27]]);
            if created > 0 {
                // Mac epoch = seconds since 1904-01-01
                tags.push(mk("FontCreated", "Font Created", Value::String(format!("(Mac epoch {})", created))));
            }
        }
    }

    Ok(tags)
}

fn read_woff(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 44 {
        return Err(Error::InvalidData("file too small for WOFF".into()));
    }

    let mut tags = Vec::new();
    tags.push(mk("FontFormat", "Font Format", Value::String("WOFF".into())));

    let flavor = &data[4..8];
    if flavor == b"OTTO" {
        tags.push(mk("FontFlavor", "Font Flavor", Value::String("OpenType/CFF".into())));
    } else {
        tags.push(mk("FontFlavor", "Font Flavor", Value::String("TrueType".into())));
    }

    let _num_tables = u16::from_be_bytes([data[12], data[13]]) as usize;
    let _total_size = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
    let major = u16::from_be_bytes([data[20], data[21]]);
    let minor = u16::from_be_bytes([data[22], data[23]]);

    tags.push(mk("Version", "Version", Value::String(format!("{}.{}", major, minor))));

    Ok(tags)
}

fn parse_name_table(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 6 {
        return;
    }

    let count = u16::from_be_bytes([data[2], data[3]]) as usize;
    let string_offset = u16::from_be_bytes([data[4], data[5]]) as usize;
    let mut pos = 6;

    for _ in 0..count {
        if pos + 12 > data.len() {
            break;
        }

        let platform_id = u16::from_be_bytes([data[pos], data[pos + 1]]);
        let _encoding_id = u16::from_be_bytes([data[pos + 2], data[pos + 3]]);
        let _language_id = u16::from_be_bytes([data[pos + 4], data[pos + 5]]);
        let name_id = u16::from_be_bytes([data[pos + 6], data[pos + 7]]);
        let length = u16::from_be_bytes([data[pos + 8], data[pos + 9]]) as usize;
        let offset = u16::from_be_bytes([data[pos + 10], data[pos + 11]]) as usize;
        pos += 12;

        let abs_offset = string_offset + offset;
        if abs_offset + length > data.len() {
            continue;
        }

        // Only process platform 3 (Windows) or platform 1 (Mac)
        let text = if platform_id == 3 {
            let units: Vec<u16> = data[abs_offset..abs_offset + length]
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16_lossy(&units)
        } else if platform_id == 1 {
            String::from_utf8_lossy(&data[abs_offset..abs_offset + length]).to_string()
        } else {
            continue;
        };

        if text.is_empty() {
            continue;
        }

        let (name, desc) = match name_id {
            0 => ("Copyright", "Copyright"),
            1 => ("FontFamily", "Font Family"),
            2 => ("FontSubFamily", "Font Sub-Family"),
            3 => ("FontUniqueID", "Unique ID"),
            4 => ("FontName", "Font Name"),
            5 => ("FontVersion", "Version"),
            6 => ("PostScriptName", "PostScript Name"),
            7 => ("Trademark", "Trademark"),
            8 => ("Manufacturer", "Manufacturer"),
            9 => ("Designer", "Designer"),
            11 => ("URLVendor", "Vendor URL"),
            12 => ("URLDesigner", "Designer URL"),
            13 => ("LicenseDescription", "License"),
            14 => ("LicenseURL", "License URL"),
            16 => ("TypographicFamily", "Typographic Family"),
            _ => continue,
        };

        // Only add if not already present (prefer Windows names)
        if !tags.iter().any(|t| t.name == name) {
            tags.push(mk(name, desc, Value::String(text)));
        }
    }
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup { family0: "Font".into(), family1: "Font".into(), family2: "Other".into() },
        raw_value: value, print_value: pv, priority: 0,
    }
}

/// Read PostScript Type 1 ASCII font (.pfa) file.
/// Mirrors ExifTool's PostScript.pm + Font.pm PSInfo handling.
pub fn read_pfa(data: &[u8]) -> Result<Vec<Tag>> {
    // Must start with %!PS-AdobeFont or similar (check bytes directly)
    if !data.starts_with(b"%!PS-AdobeFont") && !data.starts_with(b"%!FontType1") {
        return Err(Error::InvalidData("not a PFA file".into()));
    }

    // Take only the text portion (until we hit binary data)
    // Find the first non-text byte or use the full file if all text
    let text_end = data.iter().position(|&b| b == 0x80).unwrap_or(data.len());
    let text_data = &data[..text_end];
    let text = String::from_utf8_lossy(text_data);

    let mut tags = Vec::new();
    let mut comment_parts: Vec<String> = Vec::new();
    let mut in_font_info = false;
    let mut dsc_done = false;
    let mut comment_done = false;

    for line in text.lines() {
        // DSC comments: %% prefix
        if line.starts_with("%%") {
            dsc_done = false;
            if let Some(rest) = line.strip_prefix("%%Title: ") {
                tags.push(mk("Title", "Title", Value::String(rest.trim().to_string())));
            } else if let Some(rest) = line.strip_prefix("%%CreationDate: ") {
                tags.push(mk("CreateDate", "Create Date", Value::String(rest.trim().to_string())));
            } else if let Some(rest) = line.strip_prefix("%%Creator: ") {
                tags.push(mk("Creator", "Creator", Value::String(rest.trim().to_string())));
            } else if line.starts_with("%%EndComments") {
                dsc_done = true;
            }
            continue;
        }

        // Single % comment (only before EndComments / first non-comment)
        if line.starts_with('%') && !comment_done {
            let rest = &line[1..].trim_start();
            if !rest.is_empty() {
                comment_parts.push(rest.to_string());
            }
            continue;
        }

        // Non-comment line: stop accumulating comments if we haven't already
        if !line.starts_with('%') && !comment_done && !comment_parts.is_empty() {
            comment_done = true;
        }

        // Detect FontInfo begin/end
        if line.contains("FontInfo") && (line.contains("begin") || line.contains("dict begin")) {
            in_font_info = true;
        }
        if line.contains("currentdict end") || line.contains("end\n") || line.trim() == "end" {
            if in_font_info {
                in_font_info = false;
            }
        }

        // Parse /key value lines (both inside and outside FontInfo for top-level attrs)
        if line.contains('/') {
            let line_trimmed = line.trim();
            if let Some(rest) = line_trimmed.strip_prefix('/') {
                // Parse /Key value
                if let Some((key, val_part)) = rest.split_once(|c: char| c == ' ' || c == '\t') {
                    let val = val_part.trim();
                    let val_str = if val.starts_with('(') && val.contains(')') {
                        // PostScript string literal (value)
                        let inner = val.trim_start_matches('(');
                        if let Some(end) = inner.rfind(')') {
                            unescape_postscript(&inner[..end]).to_string()
                        } else {
                            inner.to_string()
                        }
                    } else if val.starts_with('/') {
                        // /Key /Value
                        val[1..].split_whitespace().next().unwrap_or("").to_string()
                    } else {
                        // /Key value (number, boolean)
                        val.split_whitespace().next().unwrap_or("").to_string()
                    };

                    // Map key to tag (PSInfo table)
                    match key {
                        "FontName" => tags.push(mk("FontName", "Font Name", Value::String(val_str))),
                        "FontType" => tags.push(mk("FontType", "Font Type", Value::String(val_str))),
                        "FullName" => tags.push(mk("FullName", "Full Name", Value::String(val_str))),
                        "FamilyName" => tags.push(mk("FontFamily", "Font Family", Value::String(val_str))),
                        "Weight" => tags.push(mk("Weight", "Weight", Value::String(val_str))),
                        "Notice" => tags.push(mk("Notice", "Notice", Value::String(val_str))),
                        "version" => tags.push(mk("Version", "Version", Value::String(val_str))),
                        "FSType" => tags.push(mk("FSType", "FS Type", Value::String(val_str))),
                        "ItalicAngle" => tags.push(mk("ItalicAngle", "Italic Angle", Value::String(val_str))),
                        "isFixedPitch" => tags.push(mk("IsFixedPitch", "Is Fixed Pitch", Value::String(val_str))),
                        "UnderlinePosition" => tags.push(mk("UnderlinePosition", "Underline Position", Value::String(val_str))),
                        "UnderlineThickness" => tags.push(mk("UnderlineThickness", "Underline Thickness", Value::String(val_str))),
                        _ => {}
                    }
                }
            }
        }
    }

    // Add accumulated comment
    if !comment_parts.is_empty() {
        let combined = comment_parts.join(".."); // ExifTool joins with ".." separator
        tags.push(mk("Comment", "Comment", Value::String(combined)));
    }

    Ok(tags)
}

fn unescape_postscript(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('r') => result.push('\r'),
                Some('t') => result.push('\t'),
                Some('b') => result.push('\x08'),
                Some('f') => result.push('\x0c'),
                Some('\\') => result.push('\\'),
                Some('(') => result.push('('),
                Some(')') => result.push(')'),
                Some(c) => { result.push('\\'); result.push(c); }
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Read Adobe Font Metrics (.afm) text file.
/// Mirrors ExifTool's Font.pm AFM handling.
pub fn read_afm(data: &[u8]) -> Result<Vec<Tag>> {
    let text = std::str::from_utf8(data).map_err(|_| Error::InvalidData("AFM not UTF-8".into()))?;

    // Must start with StartFontMetrics
    if !text.starts_with("StartFontMetrics") {
        return Err(Error::InvalidData("not an AFM file".into()));
    }

    let mut tags = Vec::new();
    let mut create_date: Option<String> = None;

    for line in text.lines() {
        // Comment lines: "Comment key: value" or "Comment text"
        if line.starts_with("Comment ") {
            let rest = &line[8..];
            // Check for "Comment Creation Date: ..."
            if let Some(stripped) = rest.strip_prefix("Creation Date: ") {
                create_date = Some(stripped.trim().to_string());
            } else if create_date.is_none() && !rest.is_empty() {
                // First non-date comment becomes Comment tag
                tags.push(mk("Comment", "Comment", Value::String(rest.trim().to_string())));
            }
            continue;
        }

        // Key value pairs separated by first whitespace
        if let Some((key, value)) = line.split_once(|c: char| c == ' ' || c == '\t') {
            let key = key.trim();
            let value = value.trim();

            // Map AFM keys to ExifTool tag names
            // ExifTool uses the Perl key directly (mostly same as AFM key)
            let tag_name = match key {
                "FontName" => Some(("FontName", "Font Name")),
                "FullName" => Some(("FullName", "Full Name")),
                "FamilyName" => Some(("FontFamily", "Font Family")),
                "Weight" => Some(("Weight", "Weight")),
                "Notice" => {
                    // Strip parentheses
                    let v = value.trim_start_matches('(').trim_end_matches(')');
                    tags.push(mk("Notice", "Notice", Value::String(v.to_string())));
                    None
                }
                "Version" => Some(("Version", "Version")),
                "EncodingScheme" => Some(("EncodingScheme", "Encoding Scheme")),
                "CapHeight" => {
                    if let Ok(n) = value.parse::<f64>() {
                        tags.push(mk("CapHeight", "Cap Height", Value::String(format!("{}", n))));
                    }
                    None
                }
                "XHeight" => {
                    if let Ok(n) = value.parse::<f64>() {
                        tags.push(mk("XHeight", "X Height", Value::String(format!("{}", n))));
                    }
                    None
                }
                "Ascender" => {
                    if let Ok(n) = value.parse::<f64>() {
                        tags.push(mk("Ascender", "Ascender", Value::String(format!("{}", n))));
                    }
                    None
                }
                "Descender" => {
                    if let Ok(n) = value.parse::<f64>() {
                        tags.push(mk("Descender", "Descender", Value::String(format!("{}", n))));
                    }
                    None
                }
                _ => None,
            };
            if let Some((name, desc)) = tag_name {
                tags.push(mk(name, desc, Value::String(value.to_string())));
            }
        }
    }

    // Add CreateDate from comments
    if let Some(date) = create_date {
        tags.push(mk("CreateDate", "Create Date", Value::String(date)));
    }

    Ok(tags)
}
