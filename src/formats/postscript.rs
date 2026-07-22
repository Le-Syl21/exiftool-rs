//! PostScript/EPS/AI file format reader.
//!
//! Parses DSC (Document Structuring Convention) comments for metadata.
//! Mirrors ExifTool's PostScript.pm.

use crate::error::{Error, Result};
use crate::metadata::XmpReader;
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

/// Decode hex string (ignoring spaces) to bytes
fn decode_hex(s: &str) -> Vec<u8> {
    let s: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    (0..s.len() / 2)
        .filter_map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok())
        .collect()
}

/// `extract_embedded` is ExifTool's ExtractEmbedded level: at 0 the contents of
/// `%%BeginDocument`/`%%EndDocument` blocks are skipped, exactly like
/// `ProcessPS`'s `next unless $embedded` (PostScript.pm:606).
pub fn read_postscript(data: &[u8], extract_embedded: u8) -> Result<Vec<Tag>> {
    let mut tags = Vec::new();
    let mut offset = 0;

    // DOS EPS binary header: C5 D0 D3 C6
    if data.len() >= 30 && data.starts_with(&[0xC5, 0xD0, 0xD3, 0xC6]) {
        let ps_offset = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
        let ps_length = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize;

        if ps_offset + ps_length <= data.len() {
            offset = ps_offset;
        }
        tags.push(mk(
            "EPSFormat",
            "EPS Format",
            Value::String("DOS Binary".into()),
        ));
    }

    // Check for PS magic
    if offset + 4 > data.len()
        || (!data[offset..].starts_with(b"%!PS") && !data[offset..].starts_with(b"%!Ad"))
    {
        return Err(Error::InvalidData("not a PostScript file".into()));
    }

    // Scan every line of the document, the way ProcessPS does
    // (PostScript.pm:513-643): there is no "header section" in ExifTool -- the
    // whole file is walked and every `%%Tag: value` comment whose name is in the
    // Main table is extracted, wherever it appears.
    scan_dsc(&data[offset..], extract_embedded, &mut tags);

    // The Photoshop IRB and the XMP packet are read in the order they appear in
    // the file, because that is the only order ExifTool knows: ProcessPS reads
    // the document line by line (PostScript.pm:560-640), handling
    // `%BeginPhotoshop` and a stray `<?xpacket begin` wherever each turns up.
    // The two carry the same IPTC fields (Category, Credit, Headline, Source,
    // SupplementalCategories, Urgency), so document order alone decides which
    // one is stored last — and last wins.
    let xmp_at = find_bytes(&data[offset..], b"<?xpacket begin");
    let irb_at = find_bytes(&data[offset..], b"%BeginPhotoshop:");
    let xmp_first = match (xmp_at, irb_at) {
        (Some(x), Some(i)) => x < i,
        _ => true,
    };

    let full_text = crate::encoding::decode_utf8_or_latin1(&data[offset..]);
    let full_text = full_text.replace('\r', "\n");

    let read_xmp = |tags: &mut Vec<Tag>| {
        if let Some(xmp_start) = xmp_at {
            let xmp_data = &data[offset + xmp_start..];
            if let Some(xmp_end) = find_bytes(xmp_data, b"<?xpacket end") {
                let end = xmp_end + 20; // Include the end tag
                if let Ok(xmp_tags) = XmpReader::read(&xmp_data[..end.min(xmp_data.len())]) {
                    tags.extend(xmp_tags);
                }
            }
        }
    };

    if xmp_first {
        read_xmp(&mut tags);
        parse_photoshop_blocks(&full_text, &mut tags);
    } else {
        parse_photoshop_blocks(&full_text, &mut tags);
        read_xmp(&mut tags);
    }

    // Parse %ImageData: for image dimensions
    parse_image_data_comment(&full_text, &mut tags);

    Ok(tags)
}

/// The `PostScript::Main` tag table, restricted to the DSC comments that carry a
/// value (PostScript.pm:32-57 and :97-124). Maps the comment name to the
/// ExifTool tag name and its description. Names absent from this table -- e.g.
/// `HiResBoundingBox`, `LanguageLevel`, `DocumentProcessColors` -- are not
/// tags in ExifTool and must not be emitted.
fn dsc_tag(name: &str) -> Option<(&'static str, &'static str)> {
    Some(match name {
        "Author" => ("Author", "Author"),
        "BoundingBox" => ("BoundingBox", "Bounding Box"),
        "Copyright" => ("Copyright", "Copyright"),
        "CreationDate" => ("CreateDate", "Create Date"),
        "Creator" => ("Creator", "Creator"),
        "ImageData" => ("ImageData", "Image Data"),
        "For" => ("For", "For"),
        "Keywords" => ("Keywords", "Keywords"),
        "ModDate" => ("ModifyDate", "Modify Date"),
        "Pages" => ("Pages", "Pages"),
        "Routing" => ("Routing", "Routing"),
        "Subject" => ("Subject", "Subject"),
        "Title" => ("Title", "Title"),
        "Version" => ("Version", "Version"),
        // AI metadata (PostScript.pm:97-124)
        "AI9_ColorModel" => ("AIColorModel", "AI Color Model"),
        "AI3_ColorUsage" => ("AIColorUsage", "AI Color Usage"),
        "AI5_RulerUnits" => ("AIRulerUnits", "AI Ruler Units"),
        "AI5_TargetResolution" => ("AITargetResolution", "AI Target Resolution"),
        "AI5_NumLayers" => ("AINumLayers", "AI Num Layers"),
        "AI5_FileFormat" => ("AIFileFormat", "AI File Format"),
        "AI8_CreatorVersion" => ("AICreatorVersion", "AI Creator Version"),
        "AI12_BuildNumber" => ("AIBuildNumber", "AI Build Number"),
        _ => return None,
    })
}

/// PrintConv for the two AI tags that have one (PostScript.pm:99-118).
fn dsc_print_conv(tag: &str, val: &str) -> Option<&'static str> {
    Some(match (tag, val.trim()) {
        ("AIColorModel", "1") => "RGB",
        ("AIColorModel", "2") => "CMYK",
        ("AIRulerUnits", "0") => "Inches",
        ("AIRulerUnits", "1") => "Millimeters",
        ("AIRulerUnits", "2") => "Points",
        ("AIRulerUnits", "3") => "Picas",
        ("AIRulerUnits", "4") => "Centimeters",
        ("AIRulerUnits", "6") => "Pixels",
        _ => return None,
    })
}

/// Splits a PostScript file into lines the way ProcessPS does: a line ends at
/// any CR, LF or CR+LF (PostScript.pm:373-393, `SplitLine`). Returns
/// `(start, end)` byte ranges of the line bodies, terminator excluded.
fn ps_lines(data: &[u8]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < data.len() {
        if data[i] == b'\n' || data[i] == b'\r' {
            out.push((start, i));
            if data[i] == b'\r' && i + 1 < data.len() && data[i + 1] == b'\n' {
                i += 1;
            }
            i += 1;
            start = i;
        } else {
            i += 1;
        }
    }
    if start < data.len() {
        out.push((start, data.len()));
    }
    out
}

fn starts_with_ci(line: &str, prefix: &str) -> bool {
    line.len() >= prefix.len() && line[..prefix.len()].eq_ignore_ascii_case(prefix)
}

/// `$docNum =~ s/-?(\d+)$//` (PostScript.pm:578, :583). Removes the trailing
/// nesting level and returns the number that was removed.
fn pop_doc_level(doc_num: &mut String) -> u32 {
    let digits_at = doc_num
        .rfind(|c: char| !c.is_ascii_digit())
        .map_or(0, |p| p + 1);
    if digits_at == doc_num.len() {
        return 0;
    }
    let num = doc_num[digits_at..].parse::<u32>().unwrap_or(0);
    let cut = if digits_at > 0 && doc_num.as_bytes()[digits_at - 1] == b'-' {
        digits_at - 1
    } else {
        digits_at
    };
    doc_num.truncate(cut);
    num
}

/// `^(%{1,2})(Begin)(_xml_packet|Photoshop|ICCProfile|Document|Binary)` with the
/// /i flag (PostScript.pm:586). Returns `(percent, begin, keyword)` exactly as
/// they were spelled, because the end token is built from them.
fn match_begin(line: &str) -> Option<(&str, &str, &str)> {
    let pct_len = if line.starts_with("%%") {
        2
    } else if line.starts_with('%') {
        1
    } else {
        return None;
    };
    let rest = &line[pct_len..];
    if rest.len() < 5 || !rest[..5].eq_ignore_ascii_case("Begin") {
        return None;
    }
    let after = &rest[5..];
    for kw in [
        "_xml_packet",
        "Photoshop",
        "ICCProfile",
        "Document",
        "Binary",
    ] {
        if after.len() >= kw.len() && after[..kw.len()].eq_ignore_ascii_case(kw) {
            return Some((&line[..pct_len], &rest[..5], &after[..kw.len()]));
        }
    }
    None
}

/// `^%%?(\w+): ?(.*)` (PostScript.pm:637). Returns the comment name and the
/// raw value, plus the number of leading `%`.
fn match_dsc_comment(line: &str) -> Option<(usize, &str, &str)> {
    let pct_len = if line.starts_with("%%") {
        2
    } else if line.starts_with('%') {
        1
    } else {
        return None;
    };
    let rest = &line[pct_len..];
    let colon = rest.find(':')?;
    let name = &rest[..colon];
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    let mut val = &rest[colon + 1..];
    // ` ?` -- at most one space is part of the separator.
    val = val.strip_prefix(' ').unwrap_or(val);
    Some((pct_len, name, val))
}

/// `DecodeComment` (PostScript.pm:308-370), minus the continuation-line reading
/// which the caller does: strips the enclosing brackets of a literal string,
/// splits a sequence of bracketed strings, and decodes the escape sequences.
fn decode_comment(val: &str) -> String {
    let val = val.trim_end_matches(['\r', '\n']);
    if !(val.starts_with('(') && val.ends_with(')') && val.len() >= 2) {
        return val.to_string();
    }
    let inner = &val[1..val.len() - 1];
    let mut vals: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut nesting = 1usize;
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                cur.push('\\');
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            '(' => {
                nesting += 1;
                cur.push(c);
            }
            ')' => {
                nesting -= 1;
                if nesting == 0 {
                    vals.push(std::mem::take(&mut cur));
                    // `++$nesting if $val =~ s/\s*\(//` -- start the next string.
                    while chars.peek().is_some_and(|c| c.is_whitespace()) {
                        chars.next();
                    }
                    if chars.peek() == Some(&'(') {
                        chars.next();
                        nesting = 1;
                    }
                } else {
                    cur.push(c);
                }
            }
            _ => cur.push(c),
        }
    }
    vals.push(cur);
    let decoded: Vec<String> = vals.iter().map(|v| unescape_ps(v)).collect();
    decoded.join(", ")
}

/// Escape-sequence decoding shared by `DecodeComment` and `UnescapePostScript`
/// (PostScript.pm:350-366).
fn unescape_ps(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            None => break,
            Some(d) if d.is_digit(8) => {
                let mut oct = d.to_digit(8).unwrap();
                for _ in 0..2 {
                    match chars.peek() {
                        Some(e) if e.is_digit(8) => {
                            oct = oct * 8 + e.to_digit(8).unwrap();
                            chars.next();
                        }
                        _ => break,
                    }
                }
                out.push((oct & 0xff) as u8 as char);
            }
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('b') => out.push('\u{8}'),
            Some('f') => out.push('\u{c}'),
            Some(d) => out.push(d),
        }
    }
    out
}

/// The `ProcessPS` line loop (PostScript.pm:513-643).
///
/// A single flat pass over the file driving a small state machine: data blocks
/// (`%BeginPhotoshop`, `%begin_xml_packet`, `%BeginICCProfile`) are skipped here
/// because the caller extracts them by byte scan, `%%BeginDocument` blocks open
/// a numbered sub-document, and every other line that looks like a DSC comment
/// whose name is a Main tag is emitted.
fn scan_dsc(data: &[u8], extract_embedded: u8, tags: &mut Vec<Tag>) {
    let embedded = extract_embedded > 0;
    let lines = ps_lines(data);

    // State machine variables, named after ProcessPS's.
    let mut mode: Option<&'static str> = None;
    let mut end_token: Option<String> = None;
    let mut begin_token = String::new();
    let mut doc_num = String::new();
    let mut sub_doc_num: u32 = 0;
    let mut doc_count: u32 = 0;
    let mut end_doc: Option<String> = None;
    let mut skip_to: usize = 0;

    let mut i = 0;
    while i < lines.len() {
        let (start, end) = lines[i];
        i += 1;
        if start < skip_to {
            continue;
        }
        let line = crate::encoding::decode_utf8_or_latin1(&data[start..end]);
        let line = line.as_str();

        if let Some(m) = mode {
            match &end_token {
                // `not $endToken` -- a stray XMP packet, ends at `<?xpacket end`.
                None => {
                    if !line.contains("<?xpacket end") {
                        continue;
                    }
                    mode = None;
                    continue;
                }
                Some(et) => {
                    if !starts_with_ci(line, et) {
                        if m == "Document" {
                            // Not extracting: only track the nesting level.
                            if starts_with_ci(line, &begin_token) {
                                doc_num.push_str("-1");
                            }
                        }
                        continue;
                    }
                    if m == "Document" {
                        pop_doc_level(&mut doc_num);
                        if doc_num.is_empty() {
                            mode = None;
                        }
                        continue;
                    }
                    // A data block we do not decode here; drop it and carry on.
                    mode = None;
                    end_token = None;
                    continue;
                }
            }
        }

        if let Some(ed) = end_doc.clone() {
            if starts_with_ci(line, &ed) {
                sub_doc_num = pop_doc_level(&mut doc_num);
                if doc_num.is_empty() {
                    end_doc = None;
                }
                continue;
            }
        }

        if let Some((pct, begin, kw)) = match_begin(line) {
            let kind = match kw.to_ascii_lowercase().as_str() {
                "_xml_packet" => "XMP",
                "photoshop" => "Photoshop",
                "iccprofile" => "ICC_Profile",
                "document" => "Document",
                _ => {
                    // BeginBinary: skip the announced byte count.
                    if let Some((_, name, val)) = match_dsc_comment(line) {
                        if name.eq_ignore_ascii_case("BeginBinary") {
                            if let Ok(n) = val.trim().parse::<usize>() {
                                skip_to = end + n;
                            }
                        }
                    }
                    continue;
                }
            };
            let bt = format!("{pct}{begin}{kw}");
            let et = format!("{pct}{}{kw}", if begin == "begin" { "end" } else { "End" });
            begin_token = bt.clone();
            end_token = Some(et.clone());
            if kind != "Document" {
                mode = Some(kind);
                continue;
            }
            // This is either the 1st sub-document or the Nth document.
            if doc_num.is_empty() {
                doc_count += 1;
                doc_num = doc_count.to_string();
            } else {
                sub_doc_num += 1;
                doc_num.push('-');
                doc_num.push_str(&sub_doc_num.to_string());
            }
            sub_doc_num = 0;
            if !embedded {
                mode = Some("Document");
                continue;
            }
            end_doc = Some(et);
            end_token = None;
            mode = None;
            // Save the document name if available:
            // `^$beginToken:\s+([^\n\r]+)` (PostScript.pm:614).
            if let Some(rest) = line.get(bt.len()..).and_then(|r| r.strip_prefix(':')) {
                let name = rest.trim_start();
                if name.len() < rest.len() && !name.is_empty() {
                    let name = if name.starts_with('(') && name.ends_with(')') {
                        &name[1..name.len() - 1]
                    } else {
                        name
                    };
                    tags.push(mk_doc(
                        "EmbeddedFileName",
                        "Embedded File Name",
                        Value::String(name.to_string()),
                        &doc_num,
                    ));
                }
            }
            continue;
        }

        if line.starts_with("<?xpacket begin") && line.contains("W5M0MpCehiHzreSzNTczkc9d") {
            if !line.contains("<?xpacket end") {
                mode = Some("XMP");
                end_token = None;
            }
            continue;
        }

        let Some((pct_len, name, raw)) = match_dsc_comment(line) else {
            continue;
        };
        let Some((tag_name, desc)) = dsc_tag(name) else {
            continue;
        };
        // Only `ImageData` and the AI tags may have a single leading '%'
        // (PostScript.pm:639).
        if pct_len == 1 && tag_name != "ImageData" && !name.starts_with("AI") {
            continue;
        }
        // Continuation lines: `%%+` (PostScript.pm:319).
        let mut val = raw.to_string();
        while i < lines.len() {
            let (cs, ce) = lines[i];
            let cont = crate::encoding::decode_utf8_or_latin1(&data[cs..ce]);
            if !cont.starts_with("%%+") {
                break;
            }
            val.push_str(&cont[3..]);
            i += 1;
        }
        let val = decode_comment(&val);
        let print = dsc_print_conv(tag_name, &val).map(str::to_string);
        let mut tag = mk_doc(tag_name, desc, Value::String(val), &doc_num);
        if let Some(p) = print {
            tag.print_value = p;
        }
        tags.push(tag);
    }
}

/// Parse %BeginPhotoshop ... %EndPhotoshop blocks
fn parse_photoshop_blocks(text: &str, tags: &mut Vec<Tag>) {
    let mut search: &str = text;
    while let Some(start) = search.find("%BeginPhotoshop:") {
        let block = &search[start..];
        let end = block.find("%EndPhotoshop").unwrap_or(block.len());
        let block = &block[..end];

        // Collect hex data from continuation lines
        let mut hex_str = String::new();
        let mut first = true;
        for line in block.lines() {
            if first {
                first = false;
                continue;
            } // skip header line
            let line = line.trim();
            if let Some(hex_part) = line.strip_prefix("% ") {
                hex_str.push_str(hex_part);
            }
        }

        if !hex_str.is_empty() {
            let irb_data = decode_hex(&hex_str);
            parse_photoshop_irb(&irb_data, tags);
        }

        let advance = start + end + 13; // skip past %EndPhotoshop
        if advance >= search.len() {
            break;
        }
        search = &search[advance..];
    }
}

/// Parse Photoshop Image Resource Blocks (8BIM format)
fn parse_photoshop_irb(data: &[u8], tags: &mut Vec<Tag>) {
    let mut pos = 0;
    while pos + 12 <= data.len() {
        if &data[pos..pos + 4] != b"8BIM" {
            break;
        }
        let res_type = u16::from_be_bytes([data[pos + 4], data[pos + 5]]);

        // Pascal string at pos+6: 1 byte length + string data, padded to even
        let name_len = data[pos + 6] as usize;
        let name_total = 1 + name_len;
        let name_total = if name_total % 2 != 0 {
            name_total + 1
        } else {
            name_total
        };
        let data_start = pos + 6 + name_total;
        if data_start + 4 > data.len() {
            break;
        }
        let data_size = u32::from_be_bytes([
            data[data_start],
            data[data_start + 1],
            data[data_start + 2],
            data[data_start + 3],
        ]) as usize;
        let data_end = data_start + 4 + data_size;
        if data_end > data.len() {
            break;
        }
        let block_data = &data[data_start + 4..data_end];

        match res_type {
            0x0404 => {
                // IPTC-NAA: compute CurrentIPTCDigest as MD5 of the data
                let digest = crate::md5::md5_hex(block_data);
                tags.push(mk(
                    "CurrentIPTCDigest",
                    "Current IPTC Digest",
                    Value::String(digest),
                ));
                if let Ok(iptc_tags) = crate::metadata::IptcReader::read(block_data) {
                    tags.extend(iptc_tags);
                }
            }
            0x0425
                // IPTCDigest (stored as raw 16-byte MD5)
                if block_data.len() >= 16 => {
                    let digest = block_data[..16]
                        .iter()
                        .map(|b| format!("{:02x}", b))
                        .collect::<String>();
                    tags.push(mk("IPTCDigest", "IPTC Digest", Value::String(digest)));
                }
            _ => {}
        }

        pos = data_end;
        if pos % 2 != 0 {
            pos += 1;
        }
    }
}

/// Parse %ImageData: comment for image dimensions
fn parse_image_data_comment(text: &str, tags: &mut Vec<Tag>) {
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("%ImageData:") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() >= 2 {
                if let Ok(w) = parts[0].parse::<u32>() {
                    tags.push(mk("ImageWidth", "Image Width", Value::U32(w)));
                }
                if let Ok(h) = parts[1].parse::<u32>() {
                    tags.push(mk("ImageHeight", "Image Height", Value::U32(h)));
                }
            }
            break;
        }
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
        group: TagGroup {
            family0: "PostScript".into(),
            family1: "PostScript".into(),
            family2: "Document".into(),
            family3: "Main".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}

/// Same as [`mk`], but tagged with the family-3 document number ProcessPS was
/// at when the tag was found (`$$et{DOC_NUM} = $docNum`, PostScript.pm:610).
fn mk_doc(name: &str, description: &str, value: Value, doc_num: &str) -> Tag {
    let mut tag = mk(name, description, value);
    if !doc_num.is_empty() {
        tag.group.family3 = format!("Doc{doc_num}");
    }
    tag
}
