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
        // Stop at the first embedded document or end of comments section
        if line.starts_with("%%EndComments") || line.starts_with("%%BeginDocument")
            || line.starts_with("%%BeginProlog") || line.starts_with("%%BeginSetup")
        {
            break;
        }

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
        } else if let Some(_rest) = line.strip_prefix("%%HiResBoundingBox:") {
            // Perl doesn't emit HiResBoundingBox
        } else if let Some(rest) = line.strip_prefix("%%Pages:") {
            tags.push(mk("Pages", "Pages", Value::String(rest.trim().to_string())));
        } else if let Some(rest) = line.strip_prefix("%%LanguageLevel:") {
            tags.push(mk("LanguageLevel", "Language Level", Value::String(rest.trim().to_string())));
        } else if let Some(rest) = line.strip_prefix("%%DocumentData:") {
            tags.push(mk("DocumentData", "Document Data", Value::String(rest.trim().to_string())));
        } else if line.starts_with("%!PS-Adobe-") {
            // Perl stores version internally but doesn't emit PSVersion/EPSVersion directly
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

    // Look for %BeginPhotoshop blocks (Photoshop IRB data encoded as hex)
    let full_text = String::from_utf8_lossy(&data[offset..]);
    let full_text = full_text.replace('\r', "\n");
    parse_photoshop_blocks(&full_text, &mut tags);

    // Parse %ImageData: for image dimensions
    parse_image_data_comment(&full_text, &mut tags);

    Ok(tags)
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
            if first { first = false; continue; } // skip header line
            let line = line.trim();
            if line.starts_with("% ") {
                let hex_part = &line[2..];
                hex_str.push_str(hex_part);
            }
        }

        if !hex_str.is_empty() {
            let irb_data = decode_hex(&hex_str);
            parse_photoshop_irb(&irb_data, tags);
        }

        let advance = start + end + 13; // skip past %EndPhotoshop
        if advance >= search.len() { break; }
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
        let name_total = if name_total % 2 != 0 { name_total + 1 } else { name_total };
        let data_start = pos + 6 + name_total;
        if data_start + 4 > data.len() {
            break;
        }
        let data_size = u32::from_be_bytes([
            data[data_start], data[data_start + 1],
            data[data_start + 2], data[data_start + 3],
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
                tags.push(mk("CurrentIPTCDigest", "Current IPTC Digest", Value::String(digest)));
                if let Ok(iptc_tags) = crate::metadata::IptcReader::read(block_data) {
                    tags.extend(iptc_tags);
                }
            }
            0x0425 => {
                // IPTCDigest (stored as raw 16-byte MD5)
                if block_data.len() >= 16 {
                    let digest = block_data[..16].iter().map(|b| format!("{:02x}", b)).collect::<String>();
                    tags.push(mk("IPTCDigest", "IPTC Digest", Value::String(digest)));
                }
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
            let parts: Vec<&str> = rest.trim().split_whitespace().collect();
            if parts.len() >= 2 {
                if let Ok(w) = parts[0].parse::<u32>() {
                    tags.push(mk("ImageWidth", "Image Width", Value::U32(w)));
                }
                if let Ok(h) = parts[1].parse::<u32>() {
                    tags.push(mk("ImageHeight", "Image Height", Value::U32(h)));
                }
                // Build the ImageData string
                let img_data_str = rest.trim().to_string();
                tags.push(mk("ImageData", "Image Data", Value::String(img_data_str)));
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
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}
