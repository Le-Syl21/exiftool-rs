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
    let version = crate::encoding::decode_utf8_or_latin1(&data[5..header_end]).trim().to_string();
    tags.push(mk("PDFVersion", "PDF Version", Value::String(version)));

    // Find startxref (near end of file)
    let search_start = if data.len() > 1024 { data.len() - 1024 } else { 0 };
    let tail = &data[search_start..];

    // Find "startxref" marker
    let _xref_offset = find_bytes(tail, b"startxref").and_then(|rel| {
        let line_start = rel + 9; // skip "startxref"
        let offset_str = crate::encoding::decode_utf8_or_latin1(&tail[line_start..])
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
        let ref_str = crate::encoding::decode_utf8_or_latin1(rest);
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
/// Works on raw bytes to preserve UTF-16BE and PDFDocEncoding data.
fn parse_info_dict(dict: &[u8], tags: &mut Vec<Tag>) {
    let fields: &[(&[u8], &str, &str)] = &[
        (b"/Title", "Title", "Title"),
        (b"/Author", "Author", "Author"),
        (b"/Subject", "Subject", "Subject"),
        (b"/Keywords", "Keywords", "Keywords"),
        (b"/Creator", "Creator", "Creator Application"),
        (b"/Producer", "Producer", "PDF Producer"),
        (b"/CreationDate", "CreateDate", "Create Date"),
        (b"/ModDate", "ModifyDate", "Modify Date"),
    ];

    for (key, name, description) in fields {
        if let Some(value) = extract_pdf_string_value_bytes(dict, key) {
            let value = if name.contains("Date") {
                convert_pdf_date(&value)
            } else {
                value
            };
            if !value.is_empty() {
                tags.push(mk(name, description, Value::String(value)));
            }
        }
    }
}

/// Extract a string value after a PDF key from raw bytes.
fn extract_pdf_string_value_bytes(dict: &[u8], key: &[u8]) -> Option<String> {
    let key_pos = find_bytes(dict, key)?;
    let rest = &dict[key_pos + key.len()..];
    // Skip whitespace
    let start = rest.iter().position(|&b| b != b' ' && b != b'\t' && b != b'\r' && b != b'\n')?;
    let rest = &rest[start..];

    if rest.first() == Some(&b'(') {
        // Literal string — find matching close paren on raw bytes
        let mut depth = 0i32;
        let mut end = 0;
        let mut i = 0;
        while i < rest.len() {
            match rest[i] {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i;
                        break;
                    }
                }
                b'\\' => { i += 1; } // skip escaped byte
                _ => {}
            }
            i += 1;
        }
        if end > 1 {
            let raw = &rest[1..end];
            return Some(decode_pdf_literal_bytes(raw));
        }
    } else if rest.first() == Some(&b'<') {
        // Hex string
        if let Some(close) = rest.iter().position(|&b| b == b'>') {
            let hex = &rest[1..close];
            // Hex content is always ASCII, safe to convert
            let hex_str = crate::encoding::decode_utf8_or_latin1(hex);
            return Some(decode_pdf_hex_string(&hex_str));
        }
    }

    None
}

/// Decode PDF literal string from raw bytes: process escape sequences,
/// then detect UTF-16BE BOM or fall back to PDFDocEncoding.
fn decode_pdf_literal_bytes(raw: &[u8]) -> String {
    // First pass: decode escape sequences into raw bytes
    let mut bytes = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'\\' && i + 1 < raw.len() {
            i += 1;
            match raw[i] {
                b'n' => bytes.push(b'\n'),
                b'r' => bytes.push(b'\r'),
                b't' => bytes.push(b'\t'),
                b'b' => bytes.push(0x08),
                b'f' => bytes.push(0x0C),
                b'(' => bytes.push(b'('),
                b')' => bytes.push(b')'),
                b'\\' => bytes.push(b'\\'),
                b'0'..=b'7' => {
                    let mut val = (raw[i] - b'0') as u8;
                    if i + 1 < raw.len() && raw[i + 1] >= b'0' && raw[i + 1] <= b'7' {
                        i += 1;
                        val = val * 8 + (raw[i] - b'0');
                        if i + 1 < raw.len() && raw[i + 1] >= b'0' && raw[i + 1] <= b'7' {
                            i += 1;
                            val = val * 8 + (raw[i] - b'0');
                        }
                    }
                    bytes.push(val);
                }
                c => {
                    bytes.push(b'\\');
                    bytes.push(c);
                }
            }
        } else {
            bytes.push(raw[i]);
        }
        i += 1;
    }

    decode_pdf_text_bytes(&bytes)
}

/// Decode raw PDF text bytes: UTF-16BE (if BOM present), UTF-8 (if BOM present), or PDFDocEncoding.
fn decode_pdf_text_bytes(bytes: &[u8]) -> String {
    // UTF-16BE BOM: 0xFE 0xFF
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    // UTF-8 BOM: 0xEF 0xBB 0xBF
    if bytes.len() >= 3 && bytes[0] == 0xEF && bytes[1] == 0xBB && bytes[2] == 0xBF {
        return crate::encoding::decode_utf8_or_latin1(&bytes[3..]).to_string();
    }
    // PDFDocEncoding (superset of Latin-1 with special chars at 0x80-0x9F)
    decode_pdf_doc_encoding(bytes)
}

/// PDFDocEncoding lookup for bytes 0x80–0xAD that differ from Unicode.
/// Bytes 0x00–0x7F map to Unicode directly (ASCII).
/// Bytes 0xAE–0xFF map to the same Unicode code point (Latin-1).
fn pdf_doc_encoding_char(b: u8) -> char {
    match b {
        0x80 => '\u{2022}', // BULLET
        0x81 => '\u{2020}', // DAGGER
        0x82 => '\u{2021}', // DOUBLE DAGGER
        0x83 => '\u{2026}', // HORIZONTAL ELLIPSIS
        0x84 => '\u{2014}', // EM DASH
        0x85 => '\u{2013}', // EN DASH
        0x86 => '\u{0192}', // LATIN SMALL LETTER F WITH HOOK
        0x87 => '\u{2044}', // FRACTION SLASH
        0x88 => '\u{2039}', // SINGLE LEFT-POINTING ANGLE QUOTATION MARK
        0x89 => '\u{203A}', // SINGLE RIGHT-POINTING ANGLE QUOTATION MARK
        0x8A => '\u{2212}', // MINUS SIGN
        0x8B => '\u{2030}', // PER MILLE SIGN
        0x8C => '\u{201E}', // DOUBLE LOW-9 QUOTATION MARK
        0x8D => '\u{201C}', // LEFT DOUBLE QUOTATION MARK
        0x8E => '\u{201D}', // RIGHT DOUBLE QUOTATION MARK
        0x8F => '\u{2018}', // LEFT SINGLE QUOTATION MARK
        0x90 => '\u{2019}', // RIGHT SINGLE QUOTATION MARK
        0x91 => '\u{201A}', // SINGLE LOW-9 QUOTATION MARK
        0x92 => '\u{2122}', // TRADE MARK SIGN
        0x93 => '\u{FB01}', // LATIN SMALL LIGATURE FI
        0x94 => '\u{FB02}', // LATIN SMALL LIGATURE FL
        0x95 => '\u{0141}', // LATIN CAPITAL LETTER L WITH STROKE
        0x96 => '\u{0152}', // LATIN CAPITAL LIGATURE OE
        0x97 => '\u{0160}', // LATIN CAPITAL LETTER S WITH CARON
        0x98 => '\u{0178}', // LATIN CAPITAL LETTER Y WITH DIAERESIS
        0x99 => '\u{017D}', // LATIN CAPITAL LETTER Z WITH CARON
        0x9A => '\u{0131}', // LATIN SMALL LETTER DOTLESS I
        0x9B => '\u{0142}', // LATIN SMALL LETTER L WITH STROKE
        0x9C => '\u{0153}', // LATIN SMALL LIGATURE OE
        0x9D => '\u{0161}', // LATIN SMALL LETTER S WITH CARON
        0x9E => '\u{017E}', // LATIN SMALL LETTER Z WITH CARON
        0xA0 => '\u{20AC}', // EURO SIGN
        0xA1 => '\u{00A1}', // INVERTED EXCLAMATION MARK
        0xA2 => '\u{00A2}', // CENT SIGN
        0xA3 => '\u{00A3}', // POUND SIGN
        0xA4 => '\u{00A4}', // CURRENCY SIGN
        0xA5 => '\u{00A5}', // YEN SIGN
        0xA6 => '\u{00A6}', // BROKEN BAR
        0xA7 => '\u{00A7}', // SECTION SIGN
        0xA8 => '\u{00A8}', // DIAERESIS
        0xA9 => '\u{00A9}', // COPYRIGHT SIGN
        0xAA => '\u{00AA}', // FEMININE ORDINAL INDICATOR
        0xAB => '\u{00AB}', // LEFT-POINTING DOUBLE ANGLE QUOTATION MARK
        0xAC => '\u{00AC}', // NOT SIGN
        0xAD => '\u{00AD}', // SOFT HYPHEN
        // 0xAE–0xFF: same as Unicode code point (Latin-1 supplement)
        _ => b as char,
    }
}

/// Decode a byte slice as PDFDocEncoding to a String.
fn decode_pdf_doc_encoding(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len());
    for &b in bytes {
        if b < 0x80 {
            result.push(b as char);
        } else {
            result.push(pdf_doc_encoding_char(b));
        }
    }
    result
}

/// Decode PDF hex string.
fn decode_pdf_hex_string(hex: &str) -> String {
    let hex = hex.replace(char::is_whitespace, "");
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
    decode_pdf_text_bytes(&bytes)
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
    let text = crate::encoding::decode_utf8_or_latin1(data);
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
