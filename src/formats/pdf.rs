//! PDF file format reader.
//!
//! Parses PDF Info dictionary and embedded XMP metadata stream.
//! Mirrors ExifTool's PDF.pm.

use crate::error::{Error, Result};
use crate::formats::psd;
use crate::metadata::XmpReader;
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_pdf(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 8 || !data.starts_with(b"%PDF-") {
        return Err(Error::InvalidData("not a PDF file".into()));
    }

    let mut tags = Vec::new();

    // PDF version from header
    let header_end = data.iter().position(|&b| b == b'\n' || b == b'\r').unwrap_or(20).min(20);
    let version = String::from_utf8_lossy(&data[5..header_end]).trim().to_string();
    tags.push(mk("PDFVersion", "PDF Version", Value::String(version)));

    // Find startxref (near end of file)
    let search_start = if data.len() > 1024 { data.len() - 1024 } else { 0 };
    let tail = &data[search_start..];

    // Find "startxref" marker
    let _xref_offset = find_bytes(tail, b"startxref").and_then(|rel| {
        let line_start = rel + 9; // skip "startxref"
        let offset_str = String::from_utf8_lossy(&tail[line_start..])
            .trim()
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        offset_str.parse::<usize>().ok()
    });

    // Try to find and parse trailer dictionary
    if let Some(trailer_start) = find_bytes(tail, b"trailer") {
        let trailer_data = &tail[trailer_start..];
        if let Some(dict_start) = find_bytes(trailer_data, b"<<") {
            let dict_str = &trailer_data[dict_start..];
            parse_trailer_info(data, dict_str, &mut tags);
        }
    }

    // Scan for Info dictionary objects and XMP streams
    scan_for_info_and_xmp(data, &mut tags);

    // Scan for embedded Photoshop IRBs (IPTC, EXIF, ICC etc.)
    scan_for_photoshop_irbs(data, &mut tags);

    // Extract MediaBox from page dictionary (only if found within a /Type /Page dict)
    if let Some(media_box) = extract_media_box_from_page(data) {
        tags.push(mk("MediaBox", "Media Box", Value::String(media_box)));
    }

    // Count pages (look for /Type /Page entries)
    let page_count = count_pattern(data, b"/Type /Page") + count_pattern(data, b"/Type/Page");
    // Subtract catalog /Type /Pages entries
    let pages_count = count_pattern(data, b"/Type /Pages") + count_pattern(data, b"/Type/Pages");
    let actual_pages = if page_count > pages_count { page_count - pages_count } else { page_count };
    if actual_pages > 0 {
        tags.push(mk("PageCount", "Page Count", Value::U32(actual_pages as u32)));
    }

    // Linearized? Perl always emits "Yes" or "No"
    // A linearized PDF has /Linearized key in its first object dict
    let is_linearized = find_bytes(&data[..data.len().min(4096)], b"/Linearized").is_some();
    tags.push(mk("Linearized", "Linearized", Value::String(if is_linearized { "Yes" } else { "No" }.into())));

    // Encrypted?
    if find_bytes(&data[..data.len().min(8192)], b"/Encrypt").is_some() {
        tags.push(mk("Encryption", "Encryption", Value::String("Yes".into())));
    }

    Ok(tags)
}

/// Parse trailer dictionary for /Info reference, then find the Info object.
fn parse_trailer_info(data: &[u8], trailer: &[u8], tags: &mut Vec<Tag>) {
    // Look for /Info N N R pattern
    if let Some(info_pos) = find_bytes(trailer, b"/Info") {
        let rest = &trailer[info_pos + 5..];
        // Try to parse object reference: "N 0 R"
        let ref_str = String::from_utf8_lossy(rest);
        let parts: Vec<&str> = ref_str.trim().splitn(4, char::is_whitespace).collect();
        if parts.len() >= 3 && parts[2].starts_with('R') {
            if let Ok(obj_num) = parts[0].parse::<u32>() {
                // Find this object in the file
                find_and_parse_info_object(data, obj_num, tags);
            }
        }
    }
}

/// Find an indirect object by number and parse its Info dictionary.
fn find_and_parse_info_object(data: &[u8], obj_num: u32, tags: &mut Vec<Tag>) {
    let pattern = format!("{} 0 obj", obj_num);
    let pattern_bytes = pattern.as_bytes();

    if let Some(pos) = find_bytes(data, pattern_bytes) {
        let obj_data = &data[pos + pattern_bytes.len()..];
        if let Some(dict_start) = find_bytes(obj_data, b"<<") {
            if let Some(dict_end) = find_bytes(&obj_data[dict_start..], b">>") {
                let dict = &obj_data[dict_start..dict_start + dict_end + 2];
                parse_info_dict(dict, tags);
            }
        }
    }
}

/// Parse a PDF Info dictionary for standard metadata keys.
fn parse_info_dict(dict: &[u8], tags: &mut Vec<Tag>) {
    let dict_str = String::from_utf8_lossy(dict);

    let fields = [
        ("/Title", "Title", "Title"),
        ("/Author", "Author", "Author"),
        ("/Subject", "Subject", "Subject"),
        ("/Keywords", "Keywords", "Keywords"),
        ("/Creator", "Creator", "Creator Application"),
        ("/Producer", "Producer", "PDF Producer"),
        ("/CreationDate", "CreateDate", "Create Date"),
        ("/ModDate", "ModifyDate", "Modify Date"),
    ];

    for (key, name, description) in &fields {
        if let Some(value) = extract_pdf_string_value(&dict_str, key) {
            let value = if name.contains("Date") {
                convert_pdf_date(&value)
            } else {
                value
            };
            // Don't add empty values
            if !value.is_empty() {
                tags.push(mk(name, description, Value::String(value)));
            }
        }
    }
}

/// Extract a string value after a PDF key like /Title from a dictionary.
fn extract_pdf_string_value(dict: &str, key: &str) -> Option<String> {
    let key_pos = dict.find(key)?;
    let rest = &dict[key_pos + key.len()..];
    let rest = rest.trim_start();

    if rest.starts_with('(') {
        // Literal string
        let mut depth = 0;
        let mut end = 0;
        let bytes = rest.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            match b {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i;
                        break;
                    }
                }
                b'\\' => { /* skip next */ }
                _ => {}
            }
        }
        if end > 1 {
            let raw = &rest[1..end];
            return Some(decode_pdf_string(raw));
        }
    } else if rest.starts_with('<') {
        // Hex string
        if let Some(close) = rest.find('>') {
            let hex = &rest[1..close];
            return Some(decode_pdf_hex_string(hex));
        }
    }

    None
}

/// Decode PDF string escape sequences.
fn decode_pdf_string(s: &str) -> String {
    let mut result = String::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            i += 1;
            match bytes[i] {
                b'n' => result.push('\n'),
                b'r' => result.push('\r'),
                b't' => result.push('\t'),
                b'b' => result.push('\u{08}'),
                b'f' => result.push('\u{0C}'),
                b'(' => result.push('('),
                b')' => result.push(')'),
                b'\\' => result.push('\\'),
                b'0'..=b'7' => {
                    // Octal
                    let mut val = (bytes[i] - b'0') as u32;
                    if i + 1 < bytes.len() && bytes[i + 1] >= b'0' && bytes[i + 1] <= b'7' {
                        i += 1;
                        val = val * 8 + (bytes[i] - b'0') as u32;
                        if i + 1 < bytes.len() && bytes[i + 1] >= b'0' && bytes[i + 1] <= b'7' {
                            i += 1;
                            val = val * 8 + (bytes[i] - b'0') as u32;
                        }
                    }
                    if let Some(c) = char::from_u32(val) {
                        result.push(c);
                    }
                }
                c => {
                    result.push('\\');
                    result.push(c as char);
                }
            }
        } else {
            result.push(bytes[i] as char);
        }
        i += 1;
    }
    result
}

/// Decode PDF hex string.
fn decode_pdf_hex_string(hex: &str) -> String {
    let hex = hex.replace(char::is_whitespace, "");
    // Check for UTF-16 BOM (FEFF)
    if hex.starts_with("FEFF") || hex.starts_with("feff") {
        let bytes: Vec<u8> = (0..hex.len())
            .step_by(2)
            .filter_map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
            .collect();
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .skip(1) // skip BOM
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }

    // ASCII hex
    let bytes: Vec<u8> = (0..hex.len())
        .step_by(2)
        .filter_map(|i| {
            if i + 2 <= hex.len() {
                u8::from_str_radix(&hex[i..i + 2], 16).ok()
            } else {
                None
            }
        })
        .collect();
    String::from_utf8_lossy(&bytes).to_string()
}

/// Convert PDF date format "D:YYYYMMDDHHmmSS" to standard format.
fn convert_pdf_date(s: &str) -> String {
    let s = s.trim_start_matches("D:");
    if s.len() >= 14 {
        format!(
            "{}:{}:{} {}:{}:{}",
            &s[0..4], &s[4..6], &s[6..8], &s[8..10], &s[10..12], &s[12..14]
        )
    } else if s.len() >= 8 {
        format!("{}:{}:{}", &s[0..4], &s[4..6], &s[6..8])
    } else {
        s.to_string()
    }
}

/// Scan file for Info dictionary and XMP metadata stream.
fn scan_for_info_and_xmp(data: &[u8], tags: &mut Vec<Tag>) {
    // Look for XMP metadata stream: /Type /Metadata /Subtype /XML
    let mut search_pos = 0;
    while search_pos < data.len() {
        if let Some(pos) = find_bytes(&data[search_pos..], b"/Type /Metadata") {
            let abs_pos = search_pos + pos;
            // Look for the stream keyword nearby (within 512 bytes)
            let search_end = (abs_pos + 512).min(data.len());
            if let Some(stream_pos) = find_bytes(&data[abs_pos..search_end], b"stream") {
                let stream_start = abs_pos + stream_pos + 6;
                // Skip \r\n or \n after "stream"
                let stream_start = if stream_start < data.len() && data[stream_start] == b'\r' {
                    if stream_start + 1 < data.len() && data[stream_start + 1] == b'\n' {
                        stream_start + 2
                    } else {
                        stream_start + 1
                    }
                } else if stream_start < data.len() && data[stream_start] == b'\n' {
                    stream_start + 1
                } else {
                    stream_start
                };

                // Find "endstream"
                if let Some(end_pos) = find_bytes(&data[stream_start..], b"endstream") {
                    let xmp_data = &data[stream_start..stream_start + end_pos];
                    // Verify it looks like XMP
                    if find_bytes(xmp_data, b"<x:xmpmeta").is_some()
                        || find_bytes(xmp_data, b"<?xpacket").is_some()
                    {
                        if let Ok(xmp_tags) = XmpReader::read(xmp_data) {
                            tags.extend(xmp_tags);
                        }
                    }
                }
            }
            search_pos = abs_pos + 1;
        } else {
            break;
        }
    }
}

/// Find /MediaBox in a /Type /Pages dictionary (page tree root, not individual pages).
/// Perl only reads MediaBox from the Pages node, not from individual Page objects.
fn extract_media_box_from_page(data: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(data);
    // Find /Type /Pages or /Type/Pages dictionaries and look for /MediaBox within them
    let mut search_start = 0;
    while search_start < text.len() {
        // Find the next /Type /Pages (with optional spaces)
        let pages_pos = text[search_start..].find("/Type /Pages")
            .or_else(|| text[search_start..].find("/Type/Pages"));
        let pages_pos = match pages_pos {
            Some(p) => search_start + p,
            None => break,
        };
        // Find the dictionary bounds (<< ... >>) containing this /Type /Pages
        // Search backward for <<
        let dict_start = text[..pages_pos].rfind("<<").unwrap_or(0);
        // Search forward for >>
        let dict_end = text[pages_pos..].find(">>").map(|p| pages_pos + p + 2).unwrap_or(text.len());
        let dict = &text[dict_start..dict_end];
        // Look for /MediaBox within this dict
        if let Some(mb_pos) = dict.find("/MediaBox") {
            let rest = &dict[mb_pos + 9..];
            let rest_trimmed = rest.trim_start();
            if rest_trimmed.starts_with('[') {
                if let Some(end) = rest_trimmed.find(']') {
                    let inner = &rest_trimmed[1..end];
                    let nums: Vec<&str> = inner.split_whitespace().collect();
                    if nums.len() >= 4 {
                        let formatted: Vec<String> = nums[..4].iter().map(|s| {
                            if let Ok(i) = s.parse::<i64>() {
                                i.to_string()
                            } else if let Ok(f) = s.parse::<f64>() {
                                format!("{}", f)
                            } else {
                                s.to_string()
                            }
                        }).collect();
                        return Some(formatted.join(", "));
                    }
                }
            }
        }
        search_start = pages_pos + 12;
    }
    None
}

/// Scan PDF data for embedded Photoshop 8BIM resource blocks.
fn scan_for_photoshop_irbs(data: &[u8], tags: &mut Vec<Tag>) {
    // Look for the start of 8BIM sequences - find first 8BIM that is at the start of a block
    // Typically in a PDF stream object
    let mut search_pos = 0;
    while search_pos + 4 < data.len() {
        if let Some(pos) = find_bytes(&data[search_pos..], b"8BIM") {
            let abs_pos = search_pos + pos;
            // Check if this looks like a real Photoshop IRB block (preceded by binary stream data)
            // Walk backward a bit to find if there's a "stream\n" before this area
            let block_start = abs_pos;

            // Only parse if we can find a sequence of 8BIM blocks
            // Parse from this block start
            let end = data.len();
            let mut irb_tags = Vec::new();
            psd::read_irb_resources(data, block_start, end, &mut irb_tags);
            if !irb_tags.is_empty() {
                // Perl doesn't emit CurrentIPTCDigest for PDF files
                tags.extend(irb_tags.into_iter().filter(|t| t.name != "CurrentIPTCDigest"));
                return; // Only parse once
            }
            search_pos = abs_pos + 4;
        } else {
            break;
        }
    }
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn count_pattern(data: &[u8], pattern: &[u8]) -> usize {
    let mut count = 0;
    let mut pos = 0;
    while pos + pattern.len() <= data.len() {
        if let Some(found) = find_bytes(&data[pos..], pattern) {
            count += 1;
            pos += found + pattern.len();
        } else {
            break;
        }
    }
    count
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let print_value = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "PDF".into(),
            family1: "PDF".into(),
            family2: "Document".into(),
        },
        raw_value: value,
        print_value,
        priority: 0,
    }
}
