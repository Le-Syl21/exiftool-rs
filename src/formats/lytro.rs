//! Lytro Light Field Picture (LFP) format reader.
//!
//! Reads Lytro LFP files, which contain JSON metadata blocks and embedded
//! JPEG images. Mirrors ExifTool's Lytro.pm.
//!
//! File format: fixed 16-byte header, then segments each with a 16-byte
//! header (type + size) and 80-byte SHA-1 identifier, followed by data.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

/// Magic bytes for LFP files: "\x89LFP\x0d\x0a\x1a\x0a"
pub const LFP_MAGIC: &[u8] = &[0x89, 0x4C, 0x46, 0x50, 0x0D, 0x0A, 0x1A, 0x0A];

pub fn read_lfp(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 16 || !data.starts_with(LFP_MAGIC) {
        return Err(Error::InvalidData("Not a Lytro LFP file".into()));
    }

    let mut tags: Vec<Tag> = Vec::new();
    let mut json_blocks: Vec<String> = Vec::new();

    let mut pos = 16usize;

    while pos + 16 <= data.len() {
        // Each segment starts with a 16-byte header
        let hdr = &data[pos..pos + 16];

        // Must start with \x89LF
        if hdr[0] != 0x89 || hdr[1] != b'L' || hdr[2] != b'F' {
            break;
        }

        // Size is big-endian u32 at offset 12
        if pos + 16 > data.len() {
            break;
        }
        let size = u32::from_be_bytes([hdr[12], hdr[13], hdr[14], hdr[15]]) as usize;
        pos += 16;

        // Skip 80-byte SHA-1 identifier
        if pos + 80 > data.len() {
            break;
        }
        pos += 80;

        // Read segment data
        if size > 0 {
            if pos + size > data.len() {
                break;
            }
            let segment = &data[pos..pos + size];

            // Check if it's a JSON metadata block
            if looks_like_json(segment) {
                if let Ok(s) = std::str::from_utf8(segment) {
                    json_blocks.push(s.to_string());
                }
            }
            // Check if it's an embedded JPEG
            // (We skip EmbeddedImage for now; it's binary data)

            pos += size;
        }

        // Skip padding to align to 16-byte boundary
        let pad = 16 - (size % 16);
        if pad != 16 {
            pos += pad;
        }
    }

    // Emit JSONMetadata tag (list of all JSON blocks as binary)
    for block in &json_blocks {
        tags.push(mk_lytro(
            "JSONMetadata",
            "JSON Metadata",
            Value::String(block.clone()),
        ));
    }

    // Process each JSON block: extract tags
    for block in &json_blocks {
        extract_tags_from_json(block.as_str(), &mut tags);
    }

    // Perl doesn't emit FocalPlaneYResolution for Lytro (note says "Y same as X")
    tags.retain(|t| t.name != "FocalPlaneYResolution");

    Ok(tags)
}

fn looks_like_json(data: &[u8]) -> bool {
    // Trim leading whitespace and check for '{'
    let s = data.iter().take_while(|&&b| b == b' ' || b == b'\t' || b == b'\r' || b == b'\n');
    let start = s.count();
    start < data.len() && data[start] == b'{'
}

/// Recursively extract tags from a JSON block, mirroring Perl's ExtractTags.
fn extract_tags_from_json(json: &str, tags: &mut Vec<Tag>) {
    // The top level is an object; start with empty prefix
    let chars: Vec<char> = json.chars().collect();
    let mut pos = 0;
    // Skip to opening {
    while pos < chars.len() && chars[pos] != '{' {
        pos += 1;
    }
    if pos >= chars.len() {
        return;
    }
    extract_object(&chars, &mut pos, "", tags);
}

/// Parse a JSON object at the current position (which must point to '{').
/// For each key-value pair, build a tag path and emit tags.
/// `parent` is the accumulated tag path so far (CamelCase).
fn extract_object(chars: &[char], pos: &mut usize, parent: &str, tags: &mut Vec<Tag>) {
    if *pos >= chars.len() || chars[*pos] != '{' {
        return;
    }
    *pos += 1; // skip '{'

    loop {
        // Skip whitespace and commas
        skip_ws_comma(chars, pos);
        if *pos >= chars.len() || chars[*pos] == '}' {
            if *pos < chars.len() {
                *pos += 1;
            }
            break;
        }

        // Read key
        if chars[*pos] != '"' {
            // Unexpected character, skip to end
            while *pos < chars.len() && chars[*pos] != '}' {
                *pos += 1;
            }
            if *pos < chars.len() {
                *pos += 1;
            }
            break;
        }
        let key = read_json_string(chars, pos);

        // Skip whitespace and colon
        skip_ws_comma(chars, pos);
        if *pos < chars.len() && chars[*pos] == ':' {
            *pos += 1;
        }
        skip_ws_comma(chars, pos);

        if *pos >= chars.len() {
            break;
        }

        // Build the accumulated tag path: parent + ucfirst(key)
        let tag_path = build_tag_path(parent, &key);

        // Read value
        match chars[*pos] {
            '{' => {
                // Nested object: recurse
                extract_object(chars, pos, &tag_path, tags);
            }
            '[' => {
                // Array: iterate elements
                extract_array(chars, pos, &tag_path, tags);
            }
            '"' => {
                let val = read_json_string(chars, pos);
                emit_tag(&tag_path, val, tags);
            }
            't' => {
                // true
                *pos += 4;
                emit_tag(&tag_path, "true".to_string(), tags);
            }
            'f' => {
                // false
                *pos += 5;
                emit_tag(&tag_path, "false".to_string(), tags);
            }
            'n' => {
                // null
                *pos += 4;
                // skip null values
            }
            _ => {
                // Number or other scalar
                let num = read_number(chars, pos);
                if !num.is_empty() {
                    emit_tag(&tag_path, num, tags);
                }
            }
        }
    }
}

/// Parse a JSON array at the current position (which must point to '[').
fn extract_array(chars: &[char], pos: &mut usize, parent: &str, tags: &mut Vec<Tag>) {
    if *pos >= chars.len() || chars[*pos] != '[' {
        return;
    }
    *pos += 1; // skip '['

    loop {
        skip_ws_comma(chars, pos);
        if *pos >= chars.len() || chars[*pos] == ']' {
            if *pos < chars.len() {
                *pos += 1;
            }
            break;
        }

        match chars[*pos] {
            '{' => {
                // Array of objects: recurse into each object with same parent
                extract_object(chars, pos, parent, tags);
            }
            '[' => {
                // Nested array: skip
                let end = find_matching(chars, *pos, '[', ']');
                *pos = end + 1;
            }
            '"' => {
                let val = read_json_string(chars, pos);
                emit_tag(parent, val, tags);
            }
            't' => {
                *pos += 4;
                emit_tag(parent, "true".to_string(), tags);
            }
            'f' => {
                *pos += 5;
                emit_tag(parent, "false".to_string(), tags);
            }
            'n' => {
                *pos += 4;
            }
            _ => {
                let num = read_number(chars, pos);
                if !num.is_empty() {
                    emit_tag(parent, num, tags);
                }
            }
        }
    }
}

/// Build the tag path from parent and key: parent + ucfirst(key).
fn build_tag_path(parent: &str, key: &str) -> String {
    if key.is_empty() {
        return parent.to_string();
    }
    let uc = ucfirst(key);
    format!("{}{}", parent, uc)
}

/// Uppercase first character of a string.
fn ucfirst(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().to_string() + c.as_str(),
    }
}

/// Convert a raw tag path (like `DevicesLensFNumber`) to a tag Name,
/// applying Perl's transformations:
/// 1. Replace non-alphanumeric chars (except - and _): remove char, uppercase next
/// 2. Strip `ParametersVendorContentComLytroTags`
/// 3. Strip leading `Devices`
///
/// Returns (name, is_devices) where is_devices means it started with "Devices".
fn tag_path_to_name(tag_path: &str) -> (String, bool) {
    // Step 1: apply s/[^-_a-zA-Z0-9](.?)/\U$1/g
    // This removes non-alnum/dash/underscore chars and uppercases the following char
    let cleaned = clean_non_alnum(tag_path);

    // Step 2: strip ParametersVendorContentComLytroTags
    let cleaned = cleaned.replace("ParametersVendorContentComLytroTags", "");

    // Step 3: check and strip leading Devices
    if cleaned.starts_with("Devices") {
        let stripped = &cleaned["Devices".len()..];
        (stripped.to_string(), true)
    } else {
        (cleaned, false)
    }
}

/// Replace non-alphanumeric (non -_) characters: remove the char and uppercase the next.
/// Mirrors Perl: s/[^-_a-zA-Z0-9](.?)/\U$1/g
fn clean_non_alnum(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_alphanumeric() || c == '-' || c == '_' {
            result.push(c);
            i += 1;
        } else {
            // Remove this char, uppercase the next
            i += 1;
            if i < chars.len() {
                for uc in chars[i].to_uppercase() {
                    result.push(uc);
                }
                i += 1;
            }
        }
    }
    result
}

/// Emit a tag, applying name transformations and print conversions.
fn emit_tag(tag_path: &str, json_value: String, tags: &mut Vec<Tag>) {
    let (name, is_devices) = tag_path_to_name(tag_path);
    if name.is_empty() {
        return;
    }

    // Apply special name mappings and print conversions
    let (final_name, raw_str, print_value) = apply_tag_mapping(&name, &json_value);

    let family2 = if is_devices { "Camera" } else { "Image" };

    // Check if tag with this name already exists; if so update it (last-write-wins for arrays)
    if let Some(existing) = tags.iter_mut().find(|t| t.name == final_name) {
        existing.raw_value = Value::String(raw_str.clone());
        existing.print_value = print_value.clone();
        return;
    }

    tags.push(Tag {
        id: TagId::Text(final_name.clone()),
        name: final_name.clone(),
        description: final_name.clone(),
        group: TagGroup {
            family0: "Lytro".into(),
            family1: "Lytro".into(),
            family2: family2.into(),
        },
        raw_value: Value::String(raw_str),
        print_value,
        priority: 0,
    });
}

/// Apply special tag name mappings (from Perl's %Main table) and print conversions.
/// Returns (final_name, raw_value, print_value).
/// The raw_value is the numeric/converted value used for composite calculations.
/// The print_value is the human-readable string.
fn apply_tag_mapping(name: &str, raw: &str) -> (String, String, String) {
    match name {
        // Explicit name remappings from Perl's tag table
        "Type" => ("CameraType".into(), raw.to_string(), raw.to_string()),
        "CameraMake" => ("Make".into(), raw.to_string(), raw.to_string()),
        "CameraModel" => ("Model".into(), raw.to_string(), raw.to_string()),
        "CameraSerialNumber" => ("SerialNumber".into(), raw.to_string(), raw.to_string()),
        "CameraFirmware" => ("FirmwareVersion".into(), raw.to_string(), raw.to_string()),
        "AccelerometerSampleArrayTime" => ("AccelerometerTime".into(), raw.to_string(), raw.to_string()),
        "AccelerometerSampleArrayX" => ("AccelerometerX".into(), raw.to_string(), raw.to_string()),
        "AccelerometerSampleArrayY" => ("AccelerometerY".into(), raw.to_string(), raw.to_string()),
        "AccelerometerSampleArrayZ" => ("AccelerometerZ".into(), raw.to_string(), raw.to_string()),
        "ClockZuluTime" => {
            // ValueConv: convert XMP date format to ExifTool format
            let converted = convert_xmp_date(raw);
            ("DateTimeOriginal".into(), converted.clone(), converted)
        }
        "LensFNumber" => {
            // Raw value stays numeric for composites (e.g., Aperture)
            let pv = format_fnumber(raw);
            ("FNumber".into(), raw.to_string(), pv)
        }
        "LensFocalLength" => {
            // ValueConv: $val * 1000 (metres to mm)
            // Raw stores mm value as string; PrintConv: "X.X mm"
            if let Ok(v) = raw.parse::<f64>() {
                let mm = v * 1000.0;
                let mm_str = format!("{}", mm);
                let pv = format!("{:.1} mm", mm);
                ("FocalLength".into(), mm_str, pv)
            } else {
                ("FocalLength".into(), raw.to_string(), raw.to_string())
            }
        }
        "LensTemperature" => {
            let pv = format_temperature(raw);
            ("LensTemperature".into(), raw.to_string(), pv)
        }
        "SocTemperature" => {
            let pv = format_temperature(raw);
            ("SocTemperature".into(), raw.to_string(), pv)
        }
        "ShutterFrameExposureDuration" => {
            let pv = format_exposure_time(raw);
            ("FrameExposureTime".into(), raw.to_string(), pv)
        }
        "ShutterPixelExposureDuration" => {
            // Raw stays as numeric seconds for composites (ShutterSpeed)
            let pv = format_exposure_time(raw);
            ("ExposureTime".into(), raw.to_string(), pv)
        }
        "SensorPixelPitch" => {
            // ValueConv: 25.4 / $val / 1000 (metres to pixels/inch)
            // Store converted numeric value in raw for composite calculations
            if let Ok(v) = raw.parse::<f64>() {
                let ppi = 25.4 / v / 1000.0;
                let s = format!("{}", ppi);
                ("FocalPlaneXResolution".into(), s.clone(), s)
            } else {
                ("FocalPlaneXResolution".into(), raw.to_string(), raw.to_string())
            }
        }
        "SensorSensorSerial" => ("SensorSerialNumber".into(), raw.to_string(), raw.to_string()),
        "SensorIso" => ("ISO".into(), raw.to_string(), raw.to_string()),
        "ImageLimitExposureBias" => {
            let pv = format_exposure_bias(raw);
            ("ImageLimitExposureBias".into(), raw.to_string(), pv)
        }
        "ImageModulationExposureBias" => {
            let pv = format_exposure_bias(raw);
            ("ImageModulationExposureBias".into(), raw.to_string(), pv)
        }
        "ImageOrientation" => {
            let pv = match raw {
                "1" => "Horizontal (normal)".to_string(),
                _ => raw.to_string(),
            };
            ("Orientation".into(), raw.to_string(), pv)
        }
        _ => (name.to_string(), raw.to_string(), raw.to_string()),
    }
}

// ============================================================================
// Print conversion helpers
// ============================================================================

/// Convert XMP date (2012-04-12T14:10:55.000Z) to ExifTool format
/// (2012:04:12 14:10:55.000Z)
fn convert_xmp_date(val: &str) -> String {
    // Replace first '-' and second '-' with ':'
    // Replace 'T' with ' '
    let mut result = String::with_capacity(val.len());
    let bytes = val.as_bytes();
    let mut dash_count = 0;
    for &b in bytes {
        if b == b'-' && dash_count < 2 {
            result.push(':');
            dash_count += 1;
        } else if b == b'T' && dash_count == 2 {
            result.push(' ');
        } else {
            result.push(b as char);
        }
    }
    result
}

/// Format FNumber (e.g., 1.9099... → "1.9")
fn format_fnumber(val: &str) -> String {
    if let Ok(f) = val.parse::<f64>() {
        // Perl PrintFNumber: format to minimal significant digits (usually 1 decimal)
        // Round to 1 decimal place
        format!("{:.1}", f)
    } else {
        val.to_string()
    }
}

/// Format temperature as "XX.X C".
fn format_temperature(val: &str) -> String {
    if let Ok(f) = val.parse::<f64>() {
        format!("{:.1} C", f)
    } else {
        val.to_string()
    }
}

/// Format exposure time as fraction (e.g., 0.004 → "1/250").
fn format_exposure_time(val: &str) -> String {
    if let Ok(f) = val.parse::<f64>() {
        if f <= 0.0 {
            return val.to_string();
        }
        if f >= 1.0 {
            return format!("{}", f);
        }
        // Express as 1/N
        let n = (1.0 / f).round() as u64;
        format!("1/{}", n)
    } else {
        val.to_string()
    }
}

/// Format exposure bias with sign: "+0.0" or "-1.2"
fn format_exposure_bias(val: &str) -> String {
    if let Ok(f) = val.parse::<f64>() {
        format!("{:+.1}", f)
    } else {
        val.to_string()
    }
}

// ============================================================================
// JSON parsing utilities
// ============================================================================

fn skip_ws_comma(chars: &[char], pos: &mut usize) {
    while *pos < chars.len() {
        let c = chars[*pos];
        if c.is_whitespace() || c == ',' {
            *pos += 1;
        } else {
            break;
        }
    }
}

fn read_json_string(chars: &[char], pos: &mut usize) -> String {
    if *pos >= chars.len() || chars[*pos] != '"' {
        return String::new();
    }
    *pos += 1; // skip opening quote
    let mut result = String::new();
    while *pos < chars.len() {
        let c = chars[*pos];
        if c == '\\' && *pos + 1 < chars.len() {
            *pos += 1;
            match chars[*pos] {
                '"' => result.push('"'),
                '\\' => result.push('\\'),
                '/' => result.push('/'),
                'n' => result.push('\n'),
                'r' => result.push('\r'),
                't' => result.push('\t'),
                'b' => result.push('\x08'),
                'f' => result.push('\x0C'),
                'u' => {
                    // Unicode escape \uXXXX
                    if *pos + 4 < chars.len() {
                        let hex: String = chars[*pos + 1..*pos + 5].iter().collect();
                        if let Ok(n) = u32::from_str_radix(&hex, 16) {
                            if let Some(ch) = char::from_u32(n) {
                                result.push(ch);
                            }
                        }
                        *pos += 4;
                    }
                }
                other => {
                    result.push('\\');
                    result.push(other);
                }
            }
            *pos += 1;
        } else if c == '"' {
            *pos += 1; // skip closing quote
            break;
        } else {
            result.push(c);
            *pos += 1;
        }
    }
    result
}

fn read_number(chars: &[char], pos: &mut usize) -> String {
    let start = *pos;
    // Handle negative sign
    if *pos < chars.len() && chars[*pos] == '-' {
        *pos += 1;
    }
    while *pos < chars.len() {
        let c = chars[*pos];
        if c.is_ascii_digit() || c == '.' || c == 'e' || c == 'E' || c == '+' || c == '-' {
            *pos += 1;
        } else {
            break;
        }
    }
    chars[start..*pos].iter().collect()
}

fn find_matching(chars: &[char], start: usize, open: char, close: char) -> usize {
    let mut depth = 0;
    let mut i = start;
    let mut in_string = false;
    while i < chars.len() {
        if in_string {
            if chars[i] == '\\' {
                i += 1; // skip escaped char
            } else if chars[i] == '"' {
                in_string = false;
            }
        } else {
            if chars[i] == '"' {
                in_string = true;
            } else if chars[i] == open {
                depth += 1;
            } else if chars[i] == close {
                depth -= 1;
                if depth == 0 {
                    return i;
                }
            }
        }
        i += 1;
    }
    chars.len().saturating_sub(1)
}

/// Create a Lytro-group tag.
fn mk_lytro(name: &str, description: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "Lytro".into(),
            family1: "Lytro".into(),
            family2: "Camera".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}
