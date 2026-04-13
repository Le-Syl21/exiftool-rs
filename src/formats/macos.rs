//! macOS XAttr (._) sidecar file reader.

use super::misc::mktag;
use crate::error::{Error, Result};
use crate::tag::Tag;
use crate::value::Value;

/// Parse MacOS AppleDouble sidecar (._) files containing XAttr data.
/// Mirrors ExifTool's MacOS.pm ProcessMacOS and ProcessATTR.
pub fn read_macos(data: &[u8]) -> Result<Vec<Tag>> {
    // Check header: \0\x05\x16\x07\0\x02\0\0Mac OS X
    if data.len() < 26 || data[0] != 0x00 || data[1] != 0x05 || data[2] != 0x16 || data[3] != 0x07 {
        return Err(Error::InvalidData("not a MacOS sidecar file".into()));
    }
    let ver = data[5];
    if ver != 2 {
        return Ok(Vec::new());
    }

    let entries = u16::from_be_bytes([data[24], data[25]]) as usize;
    if 26 + entries * 12 > data.len() {
        return Ok(Vec::new());
    }

    let mut tags = Vec::new();

    for i in 0..entries {
        let pos = 26 + i * 12;
        let tag_id = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
        let off = u32::from_be_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]])
            as usize;
        let len = u32::from_be_bytes([data[pos + 8], data[pos + 9], data[pos + 10], data[pos + 11]])
            as usize;

        if tag_id == 9 && off + len <= data.len() {
            // ATTR block
            let entry_data = &data[off..off + len];
            parse_attr_block(data, entry_data, &mut tags);
        }
    }

    Ok(tags)
}

/// Parse an ATTR (extended attributes) block from a MacOS sidecar file.
/// entry_data is the ATTR block, full_data is the entire file (for absolute offsets).
fn parse_attr_block(full_data: &[u8], entry_data: &[u8], tags: &mut Vec<Tag>) {
    if entry_data.len() < 70 {
        return;
    }
    // Check for ATTR signature at offset 34
    if &entry_data[34..38] != b"ATTR" {
        return;
    }

    let xattr_entries = u32::from_be_bytes([
        entry_data[66],
        entry_data[67],
        entry_data[68],
        entry_data[69],
    ]) as usize;
    let mut pos = 70;

    for _i in 0..xattr_entries {
        if pos + 11 > entry_data.len() {
            break;
        }
        let off = u32::from_be_bytes([
            entry_data[pos],
            entry_data[pos + 1],
            entry_data[pos + 2],
            entry_data[pos + 3],
        ]) as usize;
        let len = u32::from_be_bytes([
            entry_data[pos + 4],
            entry_data[pos + 5],
            entry_data[pos + 6],
            entry_data[pos + 7],
        ]) as usize;
        let n = entry_data[pos + 10] as usize;

        if pos + 11 + n > entry_data.len() {
            break;
        }
        let name_bytes = &entry_data[pos + 11..pos + 11 + n];
        let name = crate::encoding::decode_utf8_or_latin1(name_bytes)
            .trim_end_matches('\0')
            .to_string();

        // Offsets are absolute file offsets
        let val_data = if off + len <= full_data.len() {
            &full_data[off..off + len]
        } else {
            pos += ((11 + n + 3) & !3).max(1);
            continue;
        };

        // Convert xattr name to ExifTool tag name
        let tag_name = xattr_name_to_tag(&name);

        // Process value
        if val_data.starts_with(b"bplist0") {
            // Parse simple binary plist (arrays, strings, dates)
            if let Some(value) = parse_simple_bplist(val_data) {
                tags.push(mktag("MacOS", &tag_name, &tag_name, Value::String(value)));
            } else {
                // Just mark as binary
                tags.push(mktag(
                    "MacOS",
                    &tag_name,
                    &tag_name,
                    Value::Binary(val_data.to_vec()),
                ));
            }
        } else if len > 100 || val_data.contains(&0u8) && !val_data.starts_with(b"0082") {
            // Binary data
            tags.push(mktag(
                "MacOS",
                &tag_name,
                &tag_name,
                Value::Binary(val_data.to_vec()),
            ));
        } else {
            let s = crate::encoding::decode_utf8_or_latin1(val_data)
                .trim_end_matches('\0')
                .to_string();
            // Handle quarantine string: format "0082;TIME;APP;"
            let display = if name == "com.apple.quarantine" {
                format_quarantine(&s)
            } else {
                s
            };
            if !display.is_empty() {
                tags.push(mktag("MacOS", &tag_name, &tag_name, Value::String(display)));
            }
        }

        // Advance to next entry (aligned to 4 bytes)
        pos += ((11 + n + 3) & !3).max(4);
    }
}

/// Convert xattr attribute name to ExifTool tag name.
/// Mirrors Perl: com.apple.metadata:kMDItemXxx → XAttrMDItemXxx etc.
fn xattr_name_to_tag(name: &str) -> String {
    // Check known names first (from ExifTool's XAttr table)
    let known = match name {
        "com.apple.quarantine" => Some("XAttrQuarantine"),
        "com.apple.lastuseddate#PS" => Some("XAttrLastUsedDate"),
        "com.apple.metadata:kMDItemDownloadedDate" => Some("XAttrMDItemDownloadedDate"),
        "com.apple.metadata:kMDItemWhereFroms" => Some("XAttrMDItemWhereFroms"),
        "com.apple.metadata:kMDLabel" => Some("XAttrMDLabel"),
        "com.apple.metadata:kMDItemFinderComment" => Some("XAttrMDItemFinderComment"),
        "com.apple.metadata:_kMDItemUserTags" => Some("XAttrMDItemUserTags"),
        _ => None,
    };
    // For non-apple names: strip separators and capitalize words
    if name.starts_with("org.")
        || name.starts_with("net.")
        || (!name.starts_with("com.apple.") && name.contains(':'))
    {
        // Apply MakeTagName-style conversion
        let mut tag = String::from("XAttr");
        let mut cap_next = true;
        for c in name.chars() {
            if c == '.' || c == ':' || c == '_' || c == '-' {
                cap_next = true;
            } else if cap_next {
                for uc in c.to_uppercase() {
                    tag.push(uc);
                }
                cap_next = false;
            } else {
                tag.push(c);
            }
        }
        return tag;
    }
    if let Some(n) = known {
        return n.to_string();
    }

    // Remove random ID after kMDLabel_
    let name = if let Some(p) = name.find("kMDLabel_") {
        &name[..p + 8] // keep up to kMDLabel
    } else {
        name
    };

    // Apply Perl transformation
    let basename = if let Some(rest) = name.strip_prefix("com.apple.") {
        // s/^metadata:_?k//
        let rest = if let Some(r) = rest.strip_prefix("metadata:k") {
            r
        } else if let Some(r) = rest.strip_prefix("metadata:_k") {
            r
        } else if let Some(r) = rest.strip_prefix("metadata:") {
            r
        } else {
            rest
        };
        rest.to_string()
    } else {
        name.to_string()
    };

    // ucfirst then s/[.:_]([a-z])/\U$1/g
    let base_ucfirst = ucfirst_str_misc(&basename);
    let mut result = String::from("XAttr");

    let chars: Vec<char> = base_ucfirst.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if (c == '.' || c == ':' || c == '_' || c == '#')
            && i + 1 < chars.len()
            && chars[i + 1].is_ascii_lowercase()
        {
            result.push(chars[i + 1].to_ascii_uppercase());
            i += 2;
        } else if c == '.' || c == ':' || c == '_' || c == '#' {
            i += 1; // skip separator with no following lowercase
        } else {
            result.push(c);
            i += 1;
        }
    }
    result
}

fn ucfirst_str_misc(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

/// Format quarantine string to ExifTool format.
fn format_quarantine(s: &str) -> String {
    // Format: "FLAGS;HEX_TIME;APP;" or similar
    // ExifTool shows: "Flags=0082 set at 2020:11:12 12:27:26 by Safari"
    let parts: Vec<&str> = s.split(';').collect();
    if parts.len() >= 3 {
        let flags = parts[0];
        let time_hex = parts[1];
        let app = parts[2];

        // Try to parse time_hex as hex timestamp
        let time_str = if let Ok(ts) = i64::from_str_radix(time_hex, 16) {
            // Mac HFS+ time: seconds since 2001-01-01 or 1904-01-01
            // QuickTime epoch (2001) is used for Apple timestamps
            // Actually quarantine uses Unix epoch
            // Let's just show the raw value
            format!("(ts={})", ts)
        } else {
            time_hex.to_string()
        };

        if !app.is_empty() {
            return format!("Flags={} set at {} by {}", flags, time_str, app);
        }
        return format!("Flags={} set at {}", flags, time_str);
    }
    s.to_string()
}

/// Parse a simple binary plist to extract string, array of strings, or date values.
fn parse_simple_bplist(data: &[u8]) -> Option<String> {
    if data.len() < 32 || !data.starts_with(b"bplist00") {
        return None;
    }

    // Read trailer: last 32 bytes
    let trailer_start = data.len() - 32;
    let trailer = &data[trailer_start..];
    let offset_int_size = trailer[6] as usize;
    let obj_ref_size = trailer[7] as usize;
    let num_objects = u64::from_be_bytes([
        trailer[8],
        trailer[9],
        trailer[10],
        trailer[11],
        trailer[12],
        trailer[13],
        trailer[14],
        trailer[15],
    ]) as usize;
    let top_object = u64::from_be_bytes([
        trailer[16],
        trailer[17],
        trailer[18],
        trailer[19],
        trailer[20],
        trailer[21],
        trailer[22],
        trailer[23],
    ]) as usize;
    let offset_table_offset = u64::from_be_bytes([
        trailer[24],
        trailer[25],
        trailer[26],
        trailer[27],
        trailer[28],
        trailer[29],
        trailer[30],
        trailer[31],
    ]) as usize;

    if offset_int_size == 0 || offset_int_size > 8 || num_objects == 0 {
        return None;
    }

    let mut objects_offset = Vec::with_capacity(num_objects);
    for i in 0..num_objects {
        let ot_pos = offset_table_offset + i * offset_int_size;
        if ot_pos + offset_int_size > data.len() {
            return None;
        }
        let mut off: usize = 0;
        for j in 0..offset_int_size {
            off = (off << 8) | data[ot_pos + j] as usize;
        }
        objects_offset.push(off);
    }

    let read_object = |obj_idx: usize| -> Option<String> {
        let off = *objects_offset.get(obj_idx)?;
        if off >= data.len() {
            return None;
        }
        let marker = data[off];
        let type_nibble = (marker & 0xF0) >> 4;
        let info_nibble = marker & 0x0F;

        match type_nibble {
            0x5 => {
                // ASCII string
                let len = info_nibble as usize;
                if off + 1 + len > data.len() {
                    return None;
                }
                Some(
                    crate::encoding::decode_utf8_or_latin1(&data[off + 1..off + 1 + len])
                        .to_string(),
                )
            }
            0x6 => {
                // Unicode string (UTF-16BE)
                let len = info_nibble as usize;
                let byte_len = len * 2;
                if off + 1 + byte_len > data.len() {
                    return None;
                }
                let chars: Vec<u16> = data[off + 1..off + 1 + byte_len]
                    .chunks_exact(2)
                    .map(|c| u16::from_be_bytes([c[0], c[1]]))
                    .collect();
                String::from_utf16(&chars).ok()
            }
            0x3 => {
                // Date (64-bit float, seconds since 2001-01-01)
                if off + 9 > data.len() {
                    return None;
                }
                let bits = u64::from_be_bytes([
                    data[off + 1],
                    data[off + 2],
                    data[off + 3],
                    data[off + 4],
                    data[off + 5],
                    data[off + 6],
                    data[off + 7],
                    data[off + 8],
                ]);
                let secs = f64::from_bits(bits);
                // Convert from Apple epoch (2001-01-01) to Unix epoch (1970-01-01)
                let unix_secs = secs as i64 + 978307200;
                // Format as date string
                let days = unix_secs / 86400;
                let time = unix_secs % 86400;
                let hour = time / 3600;
                let min = (time % 3600) / 60;
                let sec = time % 60;
                let mut year = 1970i32;
                let mut rem_days = days;
                loop {
                    let dy = if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
                        366
                    } else {
                        365
                    };
                    if rem_days < dy {
                        break;
                    }
                    rem_days -= dy;
                    year += 1;
                }
                let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
                let month_days = [
                    31i64,
                    if leap { 29 } else { 28 },
                    31,
                    30,
                    31,
                    30,
                    31,
                    31,
                    30,
                    31,
                    30,
                    31,
                ];
                let mut month = 1i32;
                for &md in &month_days {
                    if rem_days < md {
                        break;
                    }
                    rem_days -= md;
                    month += 1;
                }
                let day = rem_days + 1;
                Some(format!(
                    "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
                    year, month, day, hour, min, sec
                ))
            }
            0xA => {
                // Array: collect items
                let count = if info_nibble == 0xF {
                    // extended length
                    if off + 2 > data.len() {
                        return None;
                    }
                    let ext_marker = data[off + 1];
                    (1 << (ext_marker & 0xF)) as usize
                } else {
                    info_nibble as usize
                };
                Some(format!("({} items)", count))
            }
            _ => None,
        }
    };

    // Get top object
    let result = read_object(top_object)?;

    // If it's an array, try to read its elements
    if let Some(off) = objects_offset.get(top_object) {
        let off = *off;
        if off < data.len() {
            let marker = data[off];
            let type_nibble = (marker & 0xF0) >> 4;
            if type_nibble == 0xA {
                // Array: read elements
                let count = (marker & 0x0F) as usize;
                let mut items = Vec::new();
                for j in 0..count {
                    let ref_pos = off + 1 + j * obj_ref_size;
                    if ref_pos + obj_ref_size > data.len() {
                        break;
                    }
                    let mut obj_ref: usize = 0;
                    for k in 0..obj_ref_size {
                        obj_ref = (obj_ref << 8) | data[ref_pos + k] as usize;
                    }
                    if let Some(item_val) = read_object(obj_ref) {
                        items.push(item_val);
                    }
                }
                if !items.is_empty() {
                    return Some(items.join(", "));
                }
            }
        }
    }

    Some(result)
}
