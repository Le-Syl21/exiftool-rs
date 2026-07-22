//! PDF file format reader.
//!
//! Parses PDF Info dictionary and embedded XMP metadata stream.
//! Mirrors ExifTool's PDF.pm.

use std::cell::Cell;
use std::io::Read;

use crate::error::{Error, Result};
use crate::formats::psd;
use crate::metadata::XmpReader;
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

thread_local! {
    static PROCESS_COMPRESSED: Cell<bool> = const { Cell::new(false) };
}

/// Set whether to process compressed data (used by -z option).
pub fn set_process_compressed(enabled: bool) {
    PROCESS_COMPRESSED.with(|c| c.set(enabled));
}

/// Get whether compressed data processing is enabled.
fn get_process_compressed() -> bool {
    PROCESS_COMPRESSED.with(|c| c.get())
}

/// Read a PDF. `extract_embedded` is ExifTool's ExtractEmbedded level: the
/// XObject image dictionaries are only walked when it is non-zero
/// (PDF.pm:1836, `my $embedded = (... $et->Options('ExtractEmbedded'))`).
pub fn read_pdf(data: &[u8], extract_embedded: u8) -> Result<Vec<Tag>> {
    if data.len() < 8 || !data.starts_with(b"%PDF-") {
        return Err(Error::InvalidData("not a PDF file".into()));
    }

    let mut tags = Vec::new();

    // PDF version from header
    let header_end = data
        .iter()
        .position(|&b| b == b'\n' || b == b'\r')
        .unwrap_or(20)
        .min(20);
    let version = crate::encoding::decode_utf8_or_latin1(&data[5..header_end])
        .trim()
        .to_string();
    tags.push(mk("PDFVersion", "PDF Version", Value::String(version)));

    // Find startxref (near end of file)
    let search_start = if data.len() > 1024 {
        data.len() - 1024
    } else {
        0
    };
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

    // The XMP stream and the Photoshop IRB hold the same IPTC fields (Category,
    // Credit, Headline, Source, SupplementalCategories, Urgency), so whichever
    // is read last wins. ExifTool walks the document catalog key by key in the
    // order the file writes them — `ProcessDict` takes its tag list straight
    // from the parsed dictionary (`my @tags = @{$$dict{_tags}}`, PDF.pm:1837) —
    // and the catalog reaches the XMP through `/Metadata` and the Photoshop IRB
    // through `/Pages` (→ Page → PieceInfo → AdobePhotoshop → Private →
    // ImageResources). So the two catalog keys decide the order.
    if catalog_lists_metadata_first(data) {
        scan_for_xmp(data, &mut tags);
        scan_for_photoshop_irbs(data, &mut tags);
    } else {
        scan_for_photoshop_irbs(data, &mut tags);
        scan_for_xmp(data, &mut tags);
    }

    // Extract MediaBox from page dictionary (only if found within a /Type /Page dict)
    if let Some(media_box) = extract_media_box_from_page(data) {
        // MediaBox is a 4-element numeric list; keep it as a List so JSON emits an
        // array. Elements are kept as strings so to_display_string re-joins with
        // ", " (matching Perl's text form) while each stays a JSON number.
        tags.push(mk(
            "MediaBox",
            "Media Box",
            Value::List(media_box.into_iter().map(Value::String).collect()),
        ));
    }

    // Count pages (look for /Type /Page entries)
    let page_count = count_pattern(data, b"/Type /Page") + count_pattern(data, b"/Type/Page");
    // Subtract catalog /Type /Pages entries
    let pages_count = count_pattern(data, b"/Type /Pages") + count_pattern(data, b"/Type/Pages");
    let actual_pages = if page_count > pages_count {
        page_count - pages_count
    } else {
        page_count
    };
    if actual_pages > 0 {
        tags.push(mk(
            "PageCount",
            "Page Count",
            Value::U32(actual_pages as u32),
        ));
    }

    // Linearized? Perl always emits "Yes" or "No"
    // A linearized PDF has /Linearized key in its first object dict
    let is_linearized = find_bytes(&data[..data.len().min(4096)], b"/Linearized").is_some();
    tags.push(mk(
        "Linearized",
        "Linearized",
        Value::String(if is_linearized { "Yes" } else { "No" }.into()),
    ));

    // Encrypted?
    if find_bytes(&data[..data.len().min(8192)], b"/Encrypt").is_some() {
        tags.push(mk("Encryption", "Encryption", Value::String("Yes".into())));
    }

    if extract_embedded > 0 {
        embedded::read_embedded_images(data, &mut tags);
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
    let start = rest
        .iter()
        .position(|&b| b != b' ' && b != b'\t' && b != b'\r' && b != b'\n')?;
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
                b'\\' => {
                    i += 1;
                } // skip escaped byte
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
                    let mut val = raw[i] - b'0';
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

/// Convert a PDF date to ExifTool's form. Port of `ConvertPDFDate`
/// (PDF.pm:634-658): the `D:` prefix is dropped, the date is padded from
/// `00000101000000`, then a trailing `Z` is kept as-is and a `[-+]HH'mm'`
/// offset becomes `[-+]HH:mm` (an absent minute field reads `00`).
fn convert_pdf_date(s: &str) -> String {
    let s = s.trim_start_matches("D:");
    let default = "00000101000000";
    let mut date = s.to_string();
    if date.len() < default.len() {
        date.push_str(&default[date.len()..]);
    }
    if !date.is_char_boundary(14) || !date[..14].bytes().all(|b| b.is_ascii_digit()) {
        return s.to_string();
    }
    let mut out = format!(
        "{}:{}:{} {}:{}:{}",
        &date[0..4],
        &date[4..6],
        &date[6..8],
        &date[8..10],
        &date[10..12],
        &date[12..14]
    );
    let tz = date[14..].trim_start();
    if tz.starts_with('Z') || tz.starts_with('z') {
        out.push('Z');
    } else if let Some(sign) = tz.chars().next().filter(|c| *c == '-' || *c == '+') {
        let rest = tz[1..].trim_start();
        let hours: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !hours.is_empty() {
            // Perl's separator class is [': ]+; without one the offset is dropped.
            let rest = &rest[hours.len()..];
            let sep_len = rest
                .chars()
                .take_while(|c| *c == '\'' || *c == ':' || *c == ' ')
                .count();
            if sep_len > 0 {
                let minutes: String = rest[sep_len..]
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                out.push(sign);
                out.push_str(&hours);
                out.push(':');
                out.push_str(if minutes.is_empty() { "00" } else { &minutes });
            }
        }
    }
    out
}

/// Whether the document catalog names `/Metadata` before `/Pages`, i.e. whether
/// ExifTool reaches the XMP stream before the Photoshop image resources.
///
/// A catalog without a `/Metadata` key (or without a catalog at all) answers
/// false, which leaves the Photoshop IRB first — the same result ExifTool gets
/// when there is no XMP to compete with.
fn catalog_lists_metadata_first(data: &[u8]) -> bool {
    let mut pos = 0;
    while pos < data.len() {
        let hit = match find_bytes(&data[pos..], b"/Type /Catalog")
            .or_else(|| find_bytes(&data[pos..], b"/Type/Catalog"))
        {
            Some(p) => pos + p,
            None => return false,
        };
        // The catalog dictionary ends at its closing `>>`; a 4 KiB window is
        // ample for a catalog, which holds only references.
        let end = (hit + 4096).min(data.len());
        let dict = &data[hit..end];
        let dict = match find_bytes(dict, b">>") {
            Some(e) => &dict[..e],
            None => dict,
        };
        let metadata = find_bytes(dict, b"/Metadata");
        let pages = find_bytes(dict, b"/Pages");
        match (metadata, pages) {
            (Some(m), Some(p)) => return m < p,
            (Some(_), None) => return true,
            _ => pos = hit + 1,
        }
    }
    false
}

/// Scan file for the XMP metadata stream.
fn scan_for_xmp(data: &[u8], tags: &mut Vec<Tag>) {
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

                // Check if this stream uses FlateDecode (compressed)
                let header_region = &data[abs_pos..(abs_pos + stream_pos).min(data.len())];
                let is_flate = find_bytes(header_region, b"/FlateDecode").is_some();

                // Find "endstream"
                if let Some(end_pos) = find_bytes(&data[stream_start..], b"endstream") {
                    let raw_data = &data[stream_start..stream_start + end_pos];

                    // Try raw data first, then decompress if -z is set
                    if find_bytes(raw_data, b"<x:xmpmeta").is_some()
                        || find_bytes(raw_data, b"<?xpacket").is_some()
                    {
                        if let Ok(xmp_tags) = XmpReader::read(raw_data) {
                            tags.extend(xmp_tags);
                        }
                    } else if is_flate && get_process_compressed() {
                        // Attempt zlib decompression for FlateDecode streams
                        if let Some(decompressed) = try_zlib_decompress(raw_data) {
                            if find_bytes(&decompressed, b"<x:xmpmeta").is_some()
                                || find_bytes(&decompressed, b"<?xpacket").is_some()
                            {
                                if let Ok(xmp_tags) = XmpReader::read(&decompressed) {
                                    tags.extend(xmp_tags);
                                }
                            }
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
fn extract_media_box_from_page(data: &[u8]) -> Option<Vec<String>> {
    let text = crate::encoding::decode_utf8_or_latin1(data);
    // Find /Type /Pages or /Type/Pages dictionaries and look for /MediaBox within them
    let mut search_start = 0;
    while search_start < text.len() {
        // Find the next /Type /Pages (with optional spaces)
        let pages_pos = text[search_start..]
            .find("/Type /Pages")
            .or_else(|| text[search_start..].find("/Type/Pages"));
        let pages_pos = match pages_pos {
            Some(p) => search_start + p,
            None => break,
        };
        // Find the dictionary bounds (<< ... >>) containing this /Type /Pages
        // Search backward for <<
        let dict_start = text[..pages_pos].rfind("<<").unwrap_or(0);
        // Search forward for >>
        let dict_end = text[pages_pos..]
            .find(">>")
            .map(|p| pages_pos + p + 2)
            .unwrap_or(text.len());
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
                        let formatted: Vec<String> = nums[..4]
                            .iter()
                            .map(|s| {
                                if let Ok(i) = s.parse::<i64>() {
                                    i.to_string()
                                } else if let Ok(f) = s.parse::<f64>() {
                                    format!("{}", f)
                                } else {
                                    s.to_string()
                                }
                            })
                            .collect();
                        return Some(formatted);
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
                tags.extend(
                    irb_tags
                        .into_iter()
                        .filter(|t| t.name != "CurrentIPTCDigest"),
                );
                return; // Only parse once
            }
            search_pos = abs_pos + 4;
        } else {
            break;
        }
    }
}

/// Try to decompress zlib (FlateDecode) data, returning None on failure.
fn try_zlib_decompress(data: &[u8]) -> Option<Vec<u8>> {
    // PDF FlateDecode uses zlib format (not raw deflate)
    let mut decoder = flate2::read::ZlibDecoder::new(data);
    let mut buf = Vec::new();
    // Limit decompressed size to 64 MB to avoid memory issues
    decoder
        .by_ref()
        .take(64 * 1024 * 1024)
        .read_to_end(&mut buf)
        .ok()?;
    if buf.is_empty() {
        // Try raw deflate as fallback (some PDFs omit the zlib header)
        let mut decoder = flate2::read::DeflateDecoder::new(data);
        let mut buf2 = Vec::new();
        decoder
            .by_ref()
            .take(64 * 1024 * 1024)
            .read_to_end(&mut buf2)
            .ok()?;
        if buf2.is_empty() {
            return None;
        }
        return Some(buf2);
    }
    Some(buf)
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
            family3: "Main".into(),
        },
        raw_value: value,
        print_value,
        priority: 0,
    }
}

/// Embedded images: the `/XObject` image dictionaries reachable from the
/// document catalogue, and the metadata of the images themselves.
///
/// ExifTool walks Root → Pages → Kids → Page → Resources → XObject
/// (PDF.pm:170-322) and, when ExtractEmbedded is on, treats every `Im#` entry of
/// the XObject dictionary as a sub-document (PDF.pm:367-400 defines the `Im`
/// table and PDF.pm:1836 the `$embedded` gate). Width, Height, Filter and
/// ColorSpace are reported as `EmbeddedImage*`; the stream itself is reported as
/// `EmbeddedImage` and re-read as a file of its own, but only for the two image
/// filters ExifTool understands (PDF.pm:2164, `if ($filter eq '/DCTDecode' or
/// $filter eq '/JPXDecode')`).
mod embedded {
    use std::collections::HashMap;

    use super::{find_bytes, mk};
    use crate::tag::{Tag, TagGroup, TagId};
    use crate::value::Value;

    /// A PDF object, parsed only as far as this walk needs.
    #[derive(Debug, Clone)]
    enum Obj {
        /// Dictionary with its keys in file order — PDF.pm's `_tags`, which is
        /// the order ProcessDict emits them in (PDF.pm:1837).
        Dict(Vec<(String, Obj)>),
        Array(Vec<Obj>),
        /// A `/Name`, stored without its leading slash (ReadPDFValue, PDF.pm:914).
        Name(String),
        /// A number, boolean, null or string token, kept as written.
        Token(String),
        /// An `N G R` indirect reference.
        Ref(u32),
    }

    impl Obj {
        fn get(&self, key: &str) -> Option<&Obj> {
            match self {
                Obj::Dict(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
                _ => None,
            }
        }
        fn as_name(&self) -> Option<&str> {
            match self {
                Obj::Name(n) => Some(n),
                _ => None,
            }
        }
        fn as_usize(&self) -> Option<usize> {
            match self {
                Obj::Token(t) => t.parse().ok(),
                _ => None,
            }
        }
    }

    fn is_delim(b: u8) -> bool {
        matches!(
            b,
            b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
        )
    }

    fn is_ws(b: u8) -> bool {
        matches!(b, b'\0' | b'\t' | b'\n' | 0x0c | b'\r' | b' ')
    }

    struct Parser<'a> {
        d: &'a [u8],
        p: usize,
    }

    impl<'a> Parser<'a> {
        fn skip_ws(&mut self) {
            while self.p < self.d.len() {
                let b = self.d[self.p];
                if is_ws(b) {
                    self.p += 1;
                } else if b == b'%' {
                    while self.p < self.d.len()
                        && self.d[self.p] != b'\n'
                        && self.d[self.p] != b'\r'
                    {
                        self.p += 1;
                    }
                } else {
                    break;
                }
            }
        }

        fn regular_token(&mut self) -> String {
            let start = self.p;
            while self.p < self.d.len() && !is_ws(self.d[self.p]) && !is_delim(self.d[self.p]) {
                self.p += 1;
            }
            String::from_utf8_lossy(&self.d[start..self.p]).into_owned()
        }

        /// Skip a literal `(...)` string, honouring nesting and backslash escapes.
        fn skip_literal_string(&mut self) {
            let mut depth = 0i32;
            while self.p < self.d.len() {
                match self.d[self.p] {
                    b'\\' => self.p += 1,
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            self.p += 1;
                            return;
                        }
                    }
                    _ => {}
                }
                self.p += 1;
            }
        }

        fn parse(&mut self, depth: u32) -> Option<Obj> {
            if depth > 32 {
                return None;
            }
            self.skip_ws();
            let b = *self.d.get(self.p)?;
            match b {
                b'<' if self.d.get(self.p + 1) == Some(&b'<') => {
                    self.p += 2;
                    let mut entries = Vec::new();
                    loop {
                        self.skip_ws();
                        if self.p >= self.d.len() {
                            break;
                        }
                        if self.d[self.p] == b'>' {
                            self.p += 1;
                            if self.d.get(self.p) == Some(&b'>') {
                                self.p += 1;
                            }
                            break;
                        }
                        if self.d[self.p] != b'/' {
                            // Not a key: the dictionary is malformed, stop here.
                            break;
                        }
                        self.p += 1;
                        let key = self.regular_token();
                        let value = self.parse(depth + 1)?;
                        entries.push((key, value));
                    }
                    Some(Obj::Dict(entries))
                }
                b'<' => {
                    // Hex string.
                    let start = self.p;
                    while self.p < self.d.len() && self.d[self.p] != b'>' {
                        self.p += 1;
                    }
                    self.p = (self.p + 1).min(self.d.len());
                    Some(Obj::Token(
                        String::from_utf8_lossy(&self.d[start..self.p]).into_owned(),
                    ))
                }
                b'[' => {
                    self.p += 1;
                    let mut items = Vec::new();
                    loop {
                        self.skip_ws();
                        if self.p >= self.d.len() {
                            break;
                        }
                        if self.d[self.p] == b']' {
                            self.p += 1;
                            break;
                        }
                        items.push(self.parse(depth + 1)?);
                    }
                    Some(Obj::Array(items))
                }
                b'(' => {
                    let start = self.p;
                    self.skip_literal_string();
                    Some(Obj::Token(
                        String::from_utf8_lossy(&self.d[start..self.p]).into_owned(),
                    ))
                }
                b'/' => {
                    self.p += 1;
                    Some(Obj::Name(self.regular_token()))
                }
                _ if is_delim(b) => None,
                _ => {
                    let token = self.regular_token();
                    if token.is_empty() {
                        return None;
                    }
                    // `N G R` is an indirect reference; anything else is a plain token.
                    if let Ok(num) = token.parse::<u32>() {
                        let save = self.p;
                        self.skip_ws();
                        let gen = self.regular_token();
                        if gen.parse::<u32>().is_ok() {
                            self.skip_ws();
                            let r = self.regular_token();
                            if r == "R" {
                                return Some(Obj::Ref(num));
                            }
                        }
                        self.p = save;
                    }
                    Some(Obj::Token(token))
                }
            }
        }
    }

    /// Map every `N G obj` in the file to the offset just past the `obj` keyword.
    /// ExifTool uses the cross-reference table; a full scan finds the same
    /// objects and also survives a stale xref.
    fn build_object_index(data: &[u8]) -> HashMap<u32, usize> {
        let mut index = HashMap::new();
        let mut pos = 0usize;
        while let Some(rel) = find_bytes(&data[pos..], b"obj") {
            let at = pos + rel;
            pos = at + 3;
            // `obj` must be a token of its own.
            if at + 3 < data.len() && !is_ws(data[at + 3]) && !is_delim(data[at + 3]) {
                continue;
            }
            // Walk back over "<gen> <num> " to the object number.
            let mut i = at;
            let back = |i: &mut usize, want_digits: bool| -> Option<(usize, usize)> {
                while *i > 0 && is_ws(data[*i - 1]) {
                    *i -= 1;
                }
                let end = *i;
                while *i > 0 && data[*i - 1].is_ascii_digit() {
                    *i -= 1;
                }
                if want_digits && *i == end {
                    return None;
                }
                Some((*i, end))
            };
            let gen = match back(&mut i, true) {
                Some(r) => r,
                None => continue,
            };
            if gen.0 == gen.1 {
                continue;
            }
            let num = match back(&mut i, true) {
                Some(r) => r,
                None => continue,
            };
            if let Ok(n) = String::from_utf8_lossy(&data[num.0..num.1]).parse::<u32>() {
                index.entry(n).or_insert(at + 3);
            }
        }
        index
    }

    /// Fetch an indirect object: its parsed body plus, when it carries one, the
    /// byte range of its stream data.
    fn fetch(data: &[u8], index: &HashMap<u32, usize>, num: u32) -> Option<(Obj, Option<usize>)> {
        let start = *index.get(&num)?;
        let mut parser = Parser { d: data, p: start };
        let obj = parser.parse(0)?;
        let mut stream_start = None;
        let mut probe = Parser {
            d: data,
            p: parser.p,
        };
        probe.skip_ws();
        if data[probe.p..].starts_with(b"stream") {
            let mut s = probe.p + 6;
            if data.get(s) == Some(&b'\r') {
                s += 1;
            }
            if data.get(s) == Some(&b'\n') {
                s += 1;
            }
            stream_start = Some(s);
        }
        Some((obj, stream_start))
    }

    /// Resolve one level of indirection.
    fn resolve(data: &[u8], index: &HashMap<u32, usize>, obj: &Obj) -> Option<Obj> {
        match obj {
            Obj::Ref(n) => fetch(data, index, *n).map(|(o, _)| o),
            other => Some(other.clone()),
        }
    }

    /// The document catalogue: the trailer's `/Root`, or failing that the first
    /// object declaring `/Type /Catalog`.
    fn find_catalog(data: &[u8], index: &HashMap<u32, usize>) -> Option<Obj> {
        let mut pos = 0usize;
        let mut root: Option<u32> = None;
        while let Some(rel) = find_bytes(&data[pos..], b"trailer") {
            let at = pos + rel;
            pos = at + 7;
            let mut parser = Parser { d: data, p: pos };
            if let Some(Obj::Dict(entries)) = parser.parse(0) {
                if let Some((_, Obj::Ref(n))) = entries.iter().find(|(k, _)| k == "Root") {
                    root = Some(*n);
                }
            }
        }
        if let Some(n) = root {
            if let Some((obj, _)) = fetch(data, index, n) {
                return Some(obj);
            }
        }
        let mut nums: Vec<u32> = index.keys().copied().collect();
        nums.sort_unstable();
        for n in nums {
            if let Some((obj, _)) = fetch(data, index, n) {
                if obj.get("Type").and_then(Obj::as_name) == Some("Catalog") {
                    return Some(obj);
                }
            }
        }
        None
    }

    /// Collect the page dictionaries in page-tree order, each paired with the
    /// `/Resources` in force for it (a Page inherits its parent's).
    fn collect_pages(
        data: &[u8],
        index: &HashMap<u32, usize>,
        node: &Obj,
        inherited: Option<&Obj>,
        depth: u32,
        out: &mut Vec<Obj>,
    ) {
        if depth > 16 || out.len() > 256 {
            return;
        }
        let resources = node
            .get("Resources")
            .and_then(|r| resolve(data, index, r))
            .or_else(|| inherited.cloned());
        match node.get("Type").and_then(Obj::as_name) {
            Some("Page") => {
                if let Some(r) = resources {
                    out.push(r);
                }
            }
            _ => {
                let kids = match node.get("Kids").and_then(|k| resolve(data, index, k)) {
                    Some(Obj::Array(items)) => items,
                    _ => return,
                };
                for kid in &kids {
                    if let Some(child) = resolve(data, index, kid) {
                        collect_pages(data, index, &child, resources.as_ref(), depth + 1, out);
                    }
                }
            }
        }
    }

    /// `Im0`, `Im12`, … — the XObject keys the `Im` table of PDF.pm:369-377
    /// matches through ProcessDict's `/^(.*?)(\d+)$/` (PDF.pm:1875).
    fn is_image_key(key: &str) -> bool {
        let digits = key.trim_start_matches("Im");
        key.len() > 2
            && key.starts_with("Im")
            && !digits.is_empty()
            && digits.bytes().all(|b| b.is_ascii_digit())
    }

    /// Values of a PDF value that ExifTool would report for a List tag: a name,
    /// or every non-reference element of an array (the `Im` ColorSpace RawConv
    /// is `ref $val ? undef : $val`, PDF.pm:387).
    fn list_values(obj: &Obj) -> Vec<String> {
        match obj {
            Obj::Name(n) => vec![n.clone()],
            Obj::Token(t) => vec![t.clone()],
            Obj::Array(items) => items
                .iter()
                .filter_map(|i| match i {
                    Obj::Name(n) => Some(n.clone()),
                    Obj::Token(t) => Some(t.clone()),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    fn mk_doc(name: &str, description: &str, value: Value, family2: &str, doc: u32) -> Tag {
        let mut t = mk(name, description, value);
        t.description = description.to_string();
        t.group.family2 = family2.to_string();
        t.group.family3 = format!("Doc{doc}");
        t
    }

    pub(super) fn read_embedded_images(data: &[u8], tags: &mut Vec<Tag>) {
        let index = build_object_index(data);
        if index.is_empty() {
            return;
        }
        let catalog = match find_catalog(data, &index) {
            Some(c) => c,
            None => return,
        };
        let pages = match catalog.get("Pages").and_then(|p| resolve(data, &index, p)) {
            Some(p) => p,
            None => return,
        };
        let mut resources = Vec::new();
        collect_pages(data, &index, &pages, None, 0, &mut resources);

        let mut doc = 0u32;
        let mut seen: Vec<u32> = Vec::new();
        for res in &resources {
            let xobject = match res.get("XObject").and_then(|x| resolve(data, &index, x)) {
                Some(x) => x,
                None => continue,
            };
            let entries = match &xobject {
                Obj::Dict(e) => e.clone(),
                _ => continue,
            };
            for (key, value) in entries {
                if !is_image_key(&key) {
                    continue;
                }
                // ExifTool fetches each object once (`$fetched{$$val}`, PDF.pm:1877).
                let num = match value {
                    Obj::Ref(n) => {
                        if seen.contains(&n) {
                            continue;
                        }
                        seen.push(n);
                        n
                    }
                    _ => continue,
                };
                let (image, stream_start) = match fetch(data, &index, num) {
                    Some(r) => r,
                    None => continue,
                };
                let image_entries = match &image {
                    Obj::Dict(e) => e,
                    _ => continue,
                };
                doc += 1;
                let mut image_tags = Vec::new();
                for (key, value) in image_entries {
                    let value = match resolve(data, &index, value) {
                        Some(v) => v,
                        None => continue,
                    };
                    match key.as_str() {
                        "Width" | "Height" => {
                            if let Some(n) = value.as_usize() {
                                let name = format!("EmbeddedImage{key}");
                                image_tags.push(mk_doc(
                                    &name,
                                    &format!("Embedded Image {key}"),
                                    Value::U32(n as u32),
                                    "Other",
                                    doc,
                                ));
                            }
                        }
                        "Filter" | "ColorSpace" => {
                            let values = list_values(&value);
                            if values.is_empty() {
                                continue;
                            }
                            let name = format!("EmbeddedImage{key}");
                            let description = if key == "Filter" {
                                "Embedded Image Filter"
                            } else {
                                "Embedded Image Color Space"
                            };
                            let value = if values.len() == 1 {
                                Value::String(values[0].clone())
                            } else {
                                Value::List(values.into_iter().map(Value::String).collect())
                            };
                            image_tags.push(mk_doc(&name, description, value, "Other", doc));
                        }
                        _ => {}
                    }
                }

                // Only the two image filters ExifTool can hand on get their stream
                // extracted and re-read (PDF.pm:2164). The filter tested is the
                // LAST of the filter chain (PDF.pm:2160).
                let filter = image
                    .get("Filter")
                    .and_then(|f| resolve(data, &index, f))
                    .map(|f| list_values(&f))
                    .and_then(|v| v.last().cloned())
                    .unwrap_or_default();
                if filter != "DCTDecode" && filter != "JPXDecode" {
                    tags.append(&mut image_tags);
                    continue;
                }
                let stream = match stream_start.and_then(|s| {
                    let len = image
                        .get("Length")
                        .and_then(|l| resolve(data, &index, l))
                        .and_then(|l| l.as_usize())
                        .or_else(|| find_bytes(&data[s..], b"endstream"))?;
                    data.get(s..s + len)
                }) {
                    Some(s) => s,
                    None => {
                        tags.append(&mut image_tags);
                        continue;
                    }
                };
                image_tags.push(mk_doc(
                    "EmbeddedImage",
                    "Embedded Image",
                    Value::Binary(stream.to_vec()),
                    "Preview",
                    doc,
                ));
                tags.append(&mut image_tags);
                read_embedded_file(stream, doc, tags);
            }
        }
    }

    /// Re-read an extracted image stream as a file of its own — ExifTool's
    /// `$et->ExtractInfo(\$$dict{_stream}, { ReEntry => 1 })` (PDF.pm:2169).
    fn read_embedded_file(stream: &[u8], doc: u32, tags: &mut Vec<Tag>) {
        if !stream.starts_with(&[0xff, 0xd8]) {
            return;
        }
        let jpeg_tags = match crate::formats::jpeg::read_jpeg(stream) {
            Ok(t) => t,
            Err(_) => return,
        };
        // SetFileType on re-entry gives the sub-document its own File pseudo-tags.
        for (name, description, value) in [
            ("FileType", "File Type", "JPEG"),
            ("FileTypeExtension", "File Type Extension", "jpg"),
            ("MIMEType", "MIME Type", "image/jpeg"),
        ] {
            tags.push(Tag {
                id: TagId::Text(name.to_string()),
                name: name.to_string(),
                description: description.to_string(),
                group: TagGroup {
                    family0: "File".into(),
                    family1: "File".into(),
                    family2: "Other".into(),
                    family3: format!("Doc{doc}"),
                },
                raw_value: Value::String(value.to_string()),
                print_value: value.to_string(),
                priority: 0,
            });
        }
        for mut t in jpeg_tags {
            // Composites are derived once, over the whole file, by the caller.
            if t.group.family0 == "Composite" {
                continue;
            }
            t.group.family3 = format!("Doc{doc}");
            tags.push(t);
        }
    }
}
