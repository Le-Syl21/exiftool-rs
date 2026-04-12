//! TNEF (Transport Neutral Encapsulation Format) reader.
//! Mirrors ExifTool's TNEF.pm ProcessTNEF.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

fn mk(name: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: "File".into(),
            family1: "File".into(),
            family2: "Other".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}

fn mk_print(name: &str, value: Value, print: String) -> Tag {
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: "File".into(),
            family1: "File".into(),
            family2: "Other".into(),
        },
        raw_value: value,
        print_value: print,
        priority: 0,
    }
}

fn read_u16_le(data: &[u8], off: usize) -> u16 {
    if off + 2 > data.len() { return 0; }
    u16::from_le_bytes([data[off], data[off+1]])
}

fn read_u32_le(data: &[u8], off: usize) -> u32 {
    if off + 4 > data.len() { return 0; }
    u32::from_le_bytes([data[off], data[off+1], data[off+2], data[off+3]])
}

fn read_u64_le(data: &[u8], off: usize) -> u64 {
    if off + 8 > data.len() { return 0; }
    u64::from_le_bytes([data[off], data[off+1], data[off+2], data[off+3],
                        data[off+4], data[off+5], data[off+6], data[off+7]])
}

fn read_i32_le(data: &[u8], off: usize) -> i32 {
    read_u32_le(data, off) as i32
}

/// Convert FILETIME (100ns intervals since 1601-01-01) to datetime string
fn filetime_to_datetime(ft: u64) -> Option<String> {
    if ft == 0 { return None; }
    // Unix time = (ft / 10_000_000) - 11644473600
    let secs = (ft / 10_000_000) as i64 - 11644473600;
    if secs < 0 { return None; }
    Some(unix_time_to_str(secs))
}

fn unix_time_to_str(unix: i64) -> String {
    let secs = unix % 86400;
    let days = unix / 86400;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    let (year, month, day) = days_to_date(days);
    format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}", year, month, day, h, m, s)
}

fn days_to_date(days: i64) -> (i32, u32, u32) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    (year as i32, month as u32, d as u32)
}

/// Parse a TNEF date/time (12 bytes: year(2) month(2) day(2) hour(2) min(2) sec(2))
fn parse_tnef_date(data: &[u8]) -> Option<String> {
    if data.len() < 12 { return None; }
    let year = read_u16_le(data, 0);
    let month = read_u16_le(data, 2);
    let day = read_u16_le(data, 4);
    let hour = read_u16_le(data, 6);
    let min = read_u16_le(data, 8);
    let sec = read_u16_le(data, 10);
    Some(format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}", year, month, day, hour, min, sec))
}

/// Read string from bytes (null-terminated or full length)
fn read_str(data: &[u8]) -> String {
    let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    crate::encoding::decode_utf8_or_latin1(&data[..end]).trim().to_string()
}

/// Decode UTF-16LE string
fn read_utf16le(data: &[u8]) -> String {
    let word_count = data.len() / 2;
    let words: Vec<u16> = (0..word_count)
        .map(|i| u16::from_le_bytes([data[i*2], data[i*2+1]]))
        .collect();
    // Strip null terminator
    let end = words.iter().position(|&w| w == 0).unwrap_or(words.len());
    String::from_utf16_lossy(&words[..end]).to_string()
}

/// Microsoft code page lookup
fn codepage_name(cp: u32) -> String {
    match cp {
        437 => "DOS US",
        850 => "DOS Latin 1",
        1250 => "Windows Latin 2 (Central European)",
        1251 => "Windows Cyrillic",
        1252 => "Windows Latin 1 (Western European)",
        1253 => "Windows Greek",
        1254 => "Windows Latin 5 (Turkish)",
        1255 => "Windows Hebrew",
        1256 => "Windows Arabic",
        1257 => "Windows Baltic",
        1258 => "Windows Vietnamese",
        65001 => "Unicode (UTF-8)",
        _ => return cp.to_string(),
    }.to_string()
}

/// Extract GUID bytes as a short string (first 4 bytes big-endian hex, after stripping common suffix)
/// Returns format like "00062008" from GUID 00062008-0000-0000-C000-000000000046
fn guid_prefix(data: &[u8]) -> String {
    if data.len() < 16 { return String::new(); }
    // GUID is in mixed-endian format: {DWORD}-{WORD}-{WORD}-{BYTE[2]}-{BYTE[6]}
    // Little-endian DWORD at bytes 0..4
    format!("{:02x}{:02x}{:02x}{:02x}",
        data[3], data[2], data[1], data[0])
}

/// Process TNEF message properties block (MAPI properties)
fn process_props(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 4 { return; }
    let entries = read_u32_le(data, 0) as usize;
    let mut pos = 4;

    for _i in 0..entries {
        if pos + 4 > data.len() { break; }
        let prop_type = read_u16_le(data, pos);
        let prop_tag = read_u16_le(data, pos + 2);
        pos += 4;

        // Handle named properties (bit 0x8000 set in tag)
        // Named property: GUID(16) + id_type(4) + num_or_len(4) [+ string_data if id_type==1]
        let named_key: Option<String> = if prop_tag & 0x8000 != 0 {
            if pos + 24 > data.len() { break; }
            let guid = guid_prefix(&data[pos..pos+16]);
            let id_type = read_u32_le(data, pos + 16);
            let num = read_u32_le(data, pos + 20);
            pos += 24;

            if id_type == 0 {
                // Numeric named property: key = GUID_NUMHEX
                Some(format!("{}_{:08x}", guid, num))
            } else if id_type == 1 {
                // String named property: key = GUID_name
                let slen = num as usize;
                if pos + slen > data.len() { break; }
                let name = if slen >= 2 {
                    read_utf16le(&data[pos..pos + slen - 2])
                } else {
                    String::new()
                };
                pos += (slen + 3) & !3; // pad to 4 bytes
                Some(format!("{}_{}", guid, name))
            } else {
                break; // unknown id_type
            }
        } else {
            None
        };

        // Handle multi-value
        let (is_multi, eff_prop_type) = if prop_type & 0x1000 != 0 {
            (true, prop_type & 0x0FFF)
        } else {
            (false, prop_type)
        };

        let count = if is_multi {
            if pos + 4 > data.len() { break; }
            let c = read_u32_le(data, pos) as usize;
            pos += 4;
            c
        } else {
            1
        };

        for _j in 0..count {
            let result = read_prop_value_ex(data, &mut pos, eff_prop_type, is_multi);
            let val = match result {
                Some(v) => v,
                None => break,
            };

            // Determine tag name from either named key or numeric tag
            let tag_name: Option<(&'static str, String)> = if let Some(ref key) = named_key {
                match key.as_str() {
                    "00062008_00008554" => Some(("AppVersion", val.clone())),
                    _ => None,
                }
            } else {
                match prop_tag {
                    0x0002 => Some(("AlternateRecipientAllowed", format_bool(&val))),
                    0x0039 => Some(("ClientSubmitTime", val.clone())),
                    0x0040 => Some(("ReceivedByName", val.clone())),
                    0x0044 => Some(("ReceivedRepresentingName", val.clone())),
                    0x007F => Some(("CorrelationKey", val.clone())),
                    0x0070 => Some(("Subject", val.clone())),
                    0x0075 => Some(("ReceivedByAddressType", val.clone())),
                    0x0076 => Some(("ReceivedByEmailAddress", val.clone())),
                    0x0077 => Some(("ReceivedRepresentingAddressType", val.clone())),
                    0x0078 => Some(("ReceivedRepresentingEmailAddress", val.clone())),
                    0x0C1A => Some(("SenderName", val.clone())),
                    0x0E1D => Some(("NormalizedSubject", val.clone())),
                    0x1000 => None, // MessageBodyText - binary, skip
                    0x1007 => Some(("SyncBodyCount", val.clone())),
                    0x1008 => Some(("SyncBodyData", val.clone())),
                    0x1009 => Some(("MessageBodyRTF", val.clone())),
                    0x1035 => Some(("InternetMessageID", val.clone())),
                    0x10F4 => Some(("Hidden", format_bool(&val))),
                    0x10F6 => Some(("ReadOnly", format_bool(&val))),
                    0x3007 => Some(("CreateDate", val.clone())),
                    0x3008 => Some(("ModifyDate", val.clone())),
                    0x3FDE => Some(("InternetCodePage", val.clone())),
                    0x3FF1 => Some(("LocalUserID", val.clone())),
                    0x3FF8 => Some(("CreatorName", val.clone())),
                    0x3FFA => Some(("LastModifierName", val.clone())),
                    0x3FFD => Some(("MessageCodePage", val.clone())),
                    0x4076 => Some(("SpamConfidenceLevel", val.clone())),
                    _ => None,
                }
            };

            if let Some((name, pval)) = tag_name {
                let tag = mk_print(name, Value::String(val), pval);
                tags.push(tag);
            }
        }
    }
}

fn format_bool(val: &str) -> String {
    match val.trim() {
        "0" => "False".into(),
        "1" | "-1" => "True".into(),
        _ => val.to_string(),
    }
}


fn read_prop_value_ex(data: &[u8], pos: &mut usize, prop_type: u16, is_multi: bool) -> Option<String> {
    match prop_type {
        0x01 => { // null
            Some(String::new())
        }
        0x02 => { // int16s
            if *pos + 4 > data.len() { return None; }
            let v = read_u16_le(data, *pos) as i16;
            *pos += 4; // padded
            Some(v.to_string())
        }
        0x03 | 0x0A => { // int32s
            if *pos + 4 > data.len() { return None; }
            let v = read_i32_le(data, *pos);
            *pos += 4;
            Some(v.to_string())
        }
        0x04 => { // float
            if *pos + 4 > data.len() { return None; }
            let v = f32::from_bits(read_u32_le(data, *pos));
            *pos += 4;
            Some(format!("{}", v))
        }
        0x05 => { // double
            if *pos + 8 > data.len() { return None; }
            let bits = read_u64_le(data, *pos);
            let v = f64::from_bits(bits);
            *pos += 8;
            Some(format!("{}", v))
        }
        0x06 => { // currency (int64s / 10000)
            if *pos + 8 > data.len() { return None; }
            let v = read_u64_le(data, *pos) as i64;
            *pos += 8;
            Some(format!("{}", v as f64 / 10000.0))
        }
        0x07 => { // OLE date (double, days since Dec 30, 1899)
            if *pos + 8 > data.len() { return None; }
            let bits = read_u64_le(data, *pos);
            let v = f64::from_bits(bits);
            *pos += 8;
            // Convert to Unix time: (v - 25569) * 86400
            let unix = ((v - 25569.0) * 86400.0) as i64;
            Some(unix_time_to_str(unix))
        }
        0x0B => { // boolean
            if *pos + 4 > data.len() { return None; }
            let v = read_u16_le(data, *pos);
            *pos += 4;
            Some(if v != 0 { "True".into() } else { "False".into() })
        }
        0x14 => { // int64s
            if *pos + 8 > data.len() { return None; }
            let v = read_u64_le(data, *pos) as i64;
            *pos += 8;
            Some(v.to_string())
        }
        0x1E => { // string (null-terminated)
            // For non-multi: skip 4-byte count prefix before the size
            if !is_multi { *pos += 4; }
            if *pos + 4 > data.len() { return None; }
            let len = read_u32_le(data, *pos) as usize;
            *pos += 4;
            if *pos + len > data.len() { return None; }
            let s = read_str(&data[*pos..*pos+len]);
            *pos += (len + 3) & !3; // pad to 4 bytes
            Some(s)
        }
        0x1F => { // Unicode string (null-terminated UTF-16LE)
            // For non-multi: skip 4-byte count prefix before the size
            if !is_multi { *pos += 4; }
            if *pos + 4 > data.len() { return None; }
            let len = read_u32_le(data, *pos) as usize;
            *pos += 4;
            if *pos + len > data.len() { return None; }
            let s = read_utf16le(&data[*pos..*pos+len]);
            *pos += (len + 3) & !3;
            Some(s)
        }
        0x40 => { // SYSTIME (FILETIME)
            if *pos + 8 > data.len() { return None; }
            let ft = read_u64_le(data, *pos);
            *pos += 8;
            Some(filetime_to_datetime(ft).unwrap_or_default())
        }
        0x48 => { // GUID (16 bytes)
            if *pos + 16 > data.len() { return None; }
            let g = &data[*pos..*pos+16];
            *pos += 16;
            let guid = format!("{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
                g[3],g[2],g[1],g[0], g[5],g[4], g[7],g[6],
                g[8],g[9], g[10],g[11],g[12],g[13],g[14],g[15]);
            Some(guid)
        }
        0x102 => { // binary blob
            // For non-multi: skip 4-byte count prefix before the size
            if !is_multi { *pos += 4; }
            if *pos + 4 > data.len() { return None; }
            let len = read_u32_le(data, *pos) as usize;
            *pos += 4;
            if *pos + len > data.len() { return None; }
            let s = format!("(Binary data {} bytes, use -b option to extract)", len);
            *pos += (len + 3) & !3;
            Some(s)
        }
        _ => {
            // Unknown variable-length type: skip count + size + data
            if !is_multi { *pos += 4; } // skip count prefix
            if *pos + 4 > data.len() { return None; }
            let len = read_u32_le(data, *pos) as usize;
            if len > 1024*1024 { return None; } // sanity
            *pos += 4;
            if *pos + len > data.len() { return None; }
            *pos += (len + 3) & !3;
            None
        }
    }
}

pub fn read_tnef(data: &[u8]) -> Result<Vec<Tag>> {
    // TNEF signature: 0x78 0x9F 0x3E 0x22
    if data.len() < 0x15 || &data[0..4] != b"\x78\x9f\x3e\x22" {
        return Err(Error::InvalidData("not a TNEF file".into()));
    }

    // Verify the continuation: bytes 6-10 should match
    if &data[6..11] != b"\x01\x06\x90\x08\x00" {
        // Try more lenient check - just check signature
    }

    let mut tags = Vec::new();

    // CodePage from first few bytes
    // TNEFVersion is at offset 9 (4 bytes) after signature
    let _version_bytes = if data.len() >= 10 {
        read_u32_le(data, 6)
    } else { 0 };

    // Read TNEFVersion from attribute at pos 6 (tag=0x089006, len=4)
    // First, parse actual tag stream starting at offset 6
    let mut pos = 6;

    while pos + 9 <= data.len() {
        // attrLevel (1) + tag (4) + len (4) = 9 bytes header
        let _attr_level = data[pos];
        let tag = read_u32_le(data, pos + 1);
        let len = read_u32_le(data, pos + 5) as usize;
        pos += 9;

        if pos + len > data.len() { break; }
        let attr_data = &data[pos..pos + len];

        match tag {
            0x089006 => {
                // TNEFVersion (4 bytes little-endian)
                if attr_data.len() >= 4 {
                    let v = read_u32_le(attr_data, 0);
                    let ver = format!("{}.{}.{}.{}", (v >> 24) & 0xFF, (v >> 16) & 0xFF, (v >> 8) & 0xFF, v & 0xFF);
                    tags.push(mk("TNEFVersion", Value::String(ver)));
                }
            }
            0x069007 => {
                // CodePage
                if attr_data.len() >= 4 {
                    let cp = read_u32_le(attr_data, 0);
                    let cp_name = codepage_name(cp);
                    tags.push(mk_print("CodePage", Value::U32(cp), cp_name));
                }
            }
            0x078008 => {
                // MessageClass
                let s = read_str(attr_data);
                if !s.is_empty() {
                    tags.push(mk("MessageClass", Value::String(s)));
                }
            }
            0x018009 => {
                // MessageID
                let s = read_str(attr_data);
                if !s.is_empty() {
                    tags.push(mk("MessageID", Value::String(s)));
                }
            }
            0x018004 => {
                // Subject
                let s = read_str(attr_data);
                if !s.is_empty() {
                    tags.push(mk("Subject", Value::String(s)));
                }
            }
            0x038005 => {
                // SentDate
                if let Some(dt) = parse_tnef_date(attr_data) {
                    tags.push(mk("SentDate", Value::String(dt)));
                }
            }
            0x038006 => {
                // ReceivedDate
                if let Some(dt) = parse_tnef_date(attr_data) {
                    tags.push(mk("ReceivedDate", Value::String(dt)));
                }
            }
            0x04800D => {
                // Priority
                if attr_data.len() >= 2 {
                    let p = read_u16_le(attr_data, 0);
                    let p_str = match p {
                        0 => "Low",
                        1 => "Normal",
                        2 => "High",
                        _ => "Unknown",
                    };
                    tags.push(mk_print("Priority", Value::U16(p), p_str.into()));
                }
            }
            0x038020 => {
                // MessageModifyDate
                if let Some(dt) = parse_tnef_date(attr_data) {
                    tags.push(mk("MessageModifyDate", Value::String(dt)));
                }
            }
            0x069003 => {
                // MessageProps - process MAPI properties
                process_props(attr_data, &mut tags);
            }
            0x069002 => {
                // AttachRenderingData (start of attachment)
            }
            0x069005 => {
                // AttachInfo (end of attachment)
                // process_props(attr_data, &mut tags); // attachment props
            }
            _ => {}
        }

        pos += len;
        pos += 2; // skip checksum
    }

    Ok(tags)
}
