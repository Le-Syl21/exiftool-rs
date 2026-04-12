//! MIFF (Magick Image File Format) reader.
//!
//! Parses MIFF headers (text key=value pairs) and embedded profiles
//! (IPTC/Photoshop 8BIM blocks, EXIF APP1, XMP APP1).
//! Mirrors ExifTool's MIFF.pm ProcessMIFF().

use crate::error::{Error, Result};
use crate::metadata::{ExifReader, XmpReader};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

/// XMP APP1 header as used in MIFF (and PNG): "http://ns.adobe.com/xap/1.0/\0"
const XMP_APP1_HDR: &[u8] = b"http://ns.adobe.com/xap/1.0/\x00";

/// EXIF APP1 header: "Exif\0\0"
const EXIF_APP1_HDR: &[u8] = b"Exif\x00\x00";

/// Map a lower-case MIFF header key to an ExifTool tag name.
/// Returns None for profile-* keys (handled separately) and unknown keys
/// (which are dynamically named).
fn miff_tag_name(key: &str) -> Option<&'static str> {
    match key {
        "background-color" => Some("BackgroundColor"),
        "blue-primary" => Some("BluePrimary"),
        "border-color" => Some("BorderColor"),
        "matt-color" => Some("MattColor"),
        "class" => Some("Class"),
        "colors" => Some("Colors"),
        "colorspace" => Some("ColorSpace"),
        "columns" => Some("ImageWidth"),
        "compression" => Some("Compression"),
        "delay" => Some("Delay"),
        "depth" => Some("Depth"),
        "dispose" => Some("Dispose"),
        "gamma" => Some("Gamma"),
        "green-primary" => Some("GreenPrimary"),
        "id" => Some("ID"),
        "iterations" => Some("Iterations"),
        "label" => Some("Label"),
        "matte" => Some("Matte"),
        "montage" => Some("Montage"),
        "packets" => Some("Packets"),
        "page" => Some("Page"),
        "red-primary" => Some("RedPrimary"),
        "rendering-intent" => Some("RenderingIntent"),
        "resolution" => Some("Resolution"),
        "rows" => Some("ImageHeight"),
        "scene" => Some("Scene"),
        "signature" => Some("Signature"),
        "units" => Some("Units"),
        "white-point" => Some("WhitePoint"),
        _ => None,
    }
}

fn make_miff_tag(name: &str, value: String) -> Tag {
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: "MIFF".into(),
            family1: "MIFF".into(),
            family2: "Image".into(),
        },
        raw_value: Value::String(value.clone()),
        print_value: value,
        priority: 0,
    }
}

/// Extract all metadata tags from a MIFF file.
pub fn read_miff(data: &[u8]) -> Result<Vec<Tag>> {
    // MIFF files must start with "id=ImageMagick"
    if data.len() < 14 || &data[..14] != b"id=ImageMagick" {
        return Err(Error::InvalidData("not a MIFF file".into()));
    }

    let mut tags = Vec::new();

    // Find end of header: ":\x1a" (new-style) or ":\n" (old-style)
    // The Perl code reads until ":\x1a" (Colon+Ctrl-Z).
    // For old-style files it may just use ":\n".
    let header_end = find_header_end(data);
    if header_end.is_none() {
        return Err(Error::InvalidData("MIFF header end marker not found".into()));
    }
    let (header_data_end, profile_data_start) = header_end.unwrap();

    // Parse the header text
    let header_bytes = &data[..header_data_end];
    let header_str = crate::encoding::decode_utf8_or_latin1(header_bytes);

    // Collect profiles: list of (profile_type, byte_length)
    let mut profiles: Vec<(String, usize)> = Vec::new();

    // Parse header: lines of "key=value" pairs separated by whitespace/newlines.
    // The Perl code splits on whitespace: split ' ', $buff
    // This means all tokens (split by any whitespace) are processed.
    // Multi-word values are enclosed in braces: key={value with spaces}
    // Comments are in braces starting with {.
    parse_header_tokens(&header_str, &mut tags, &mut profiles);

    // Process profile data
    let mut pos = profile_data_start;
    for (profile_type, profile_len) in &profiles {
        if pos + profile_len > data.len() {
            break;
        }
        let profile_data = &data[pos..pos + profile_len];
        pos += profile_len;

        match profile_type.as_str() {
            "iptc" => {
                // IPTC profile: if starts with "8BIM", process as Photoshop IRB blocks
                if profile_data.starts_with(b"8BIM") {
                    crate::formats::psd::read_irb_resources(
                        profile_data,
                        0,
                        profile_data.len(),
                        &mut tags,
                    );
                    // Perl ExifTool does not emit CurrentIPTCDigest for MIFF files
                    tags.retain(|t| t.name != "CurrentIPTCDigest");
                }
            }
            "APP1" | "exif" => {
                if profile_data.starts_with(EXIF_APP1_HDR) {
                    // APP1 EXIF: skip "Exif\0\0" header, then parse TIFF
                    let exif_data = &profile_data[EXIF_APP1_HDR.len()..];
                    if let Ok(exif_tags) = ExifReader::read(exif_data) {
                        tags.extend(exif_tags);
                    }
                } else if profile_data.starts_with(XMP_APP1_HDR) {
                    // APP1 XMP: skip header, then parse XMP
                    let xmp_data = &profile_data[XMP_APP1_HDR.len()..];
                    if let Ok(xmp_tags) = XmpReader::read(xmp_data) {
                        tags.extend(xmp_tags);
                    }
                }
            }
            "xmp" => {
                if let Ok(xmp_tags) = XmpReader::read(profile_data) {
                    tags.extend(xmp_tags);
                }
            }
            "icc" => {
                if let Ok(icc_tags) = crate::formats::icc::read_icc(profile_data) {
                    tags.extend(icc_tags);
                }
            }
            _ => {}
        }
    }

    // Perl ExifTool doesn't emit CurrentIPTCDigest for MIFF
    tags.retain(|t| t.name != "CurrentIPTCDigest");
    Ok(tags)
}

/// Find the end of the MIFF header.
/// Returns (offset of end-of-header-content, offset of first profile byte).
/// The header ends with ":\x1a" or (old-style) ":\n" as terminator.
fn find_header_end(data: &[u8]) -> Option<(usize, usize)> {
    // Look for ":\x1a" first (new-style)
    if let Some(idx) = find_bytes(data, b":\x1a") {
        return Some((idx, idx + 2));
    }
    // Old-style: look for standalone ":\n"
    // The Perl code: local $/ = ":\x1a"; but old files end with ":\n"
    // We'll just use the entire file as header in that case (no profile data)
    if let Some(idx) = find_bytes(data, b":\n") {
        return Some((idx, idx + 2));
    }
    None
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
}

/// Parse MIFF header tokens and populate tags and profiles lists.
/// The Perl code: split ' ', $buff — splits on any whitespace.
/// Then iterates through tokens, building key=value pairs.
fn parse_header_tokens(
    header: &str,
    tags: &mut Vec<Tag>,
    profiles: &mut Vec<(String, usize)>,
) {
    // Tokenize: split on any whitespace (space, tab, newline, CR, form-feed)
    let tokens: Vec<&str> = header.split_whitespace().collect();

    let mut i = 0;
    let mut mode = ""; // "" normal, "com" in comment, "val" in multi-word value
    let mut current_tag: Option<String> = None;
    let mut current_val = String::new();

    while i < tokens.len() {
        let token = tokens[i];
        i += 1;

        if mode == "com" {
            if token.ends_with('}') {
                mode = "";
            }
            continue;
        }

        if token.starts_with('{') && current_tag.is_none() {
            // A comment block
            if !token.ends_with('}') {
                mode = "com";
            }
            continue;
        }

        if mode == "val" {
            // Continuation of a brace-enclosed value
            current_val.push(' ');
            current_val.push_str(token);
            if token.ends_with('}') {
                mode = "";
                // Remove surrounding braces from accumulated value
                if let Some(ref tag) = current_tag {
                    let val = current_val
                        .trim_start_matches('{')
                        .trim_end_matches('}')
                        .to_string();
                    emit_tag(tag, val, tags, profiles);
                }
                current_tag = None;
                current_val.clear();
            }
            continue;
        }

        // Normal token: should be "key=value"
        if let Some(eq_pos) = token.find('=') {
            let key = &token[..eq_pos];
            let val = &token[eq_pos + 1..];

            if val.starts_with('{') {
                if val.ends_with('}') && val.len() > 1 {
                    // Single-token brace value
                    let inner = &val[1..val.len() - 1];
                    emit_tag(key, inner.to_string(), tags, profiles);
                } else {
                    // Multi-token brace value
                    mode = "val";
                    current_tag = Some(key.to_string());
                    current_val = val.to_string();
                }
            } else {
                emit_tag(key, val.to_string(), tags, profiles);
            }
        } else if token.starts_with(':') {
            // End of old-style MIFF
            break;
        }
        // Unknown token: ignore (Perl warns but continues)
    }
}

fn emit_tag(
    key: &str,
    val: String,
    tags: &mut Vec<Tag>,
    profiles: &mut Vec<(String, usize)>,
) {
    // Check for profile-* keys
    if let Some(profile_type) = key.strip_prefix("profile-") {
        // val is the length as a string
        if let Ok(len) = val.parse::<usize>() {
            profiles.push((profile_type.to_string(), len));
        }
        return;
    }

    // Map key to ExifTool tag name
    let tag_name = if let Some(mapped) = miff_tag_name(key) {
        mapped.to_string()
    } else {
        // Dynamic tag: use the key as-is (Perl adds it to the tag table)
        key.to_string()
    };

    tags.push(make_miff_tag(&tag_name, val));
}
