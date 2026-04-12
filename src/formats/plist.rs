//! Apple PLIST parser (binary and XML formats).
//! Reads binary and XML property list formats used in Apple files.

use crate::error::Result;
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;
use std::collections::HashMap;

/// Parse a binary plist and return key-value pairs.
pub fn parse_binary_plist(data: &[u8]) -> Option<HashMap<String, PlistValue>> {
    if data.len() < 40 { return None; }

    // Check magic: "bplist00"
    if !data.starts_with(b"bplist0") { return None; }

    // Trailer: last 32 bytes
    let trailer = &data[data.len() - 32..];
    let _unused = &trailer[0..6];
    let int_size = trailer[6] as usize;
    let ref_size = trailer[7] as usize;
    let num_obj = u64::from_be_bytes([trailer[8], trailer[9], trailer[10], trailer[11],
        trailer[12], trailer[13], trailer[14], trailer[15]]) as usize;
    let top_obj = u64::from_be_bytes([trailer[16], trailer[17], trailer[18], trailer[19],
        trailer[20], trailer[21], trailer[22], trailer[23]]) as usize;
    let table_off = u64::from_be_bytes([trailer[24], trailer[25], trailer[26], trailer[27],
        trailer[28], trailer[29], trailer[30], trailer[31]]) as usize;

    if top_obj >= num_obj || int_size == 0 || ref_size == 0 { return None; }
    if table_off + int_size * num_obj > data.len() { return None; }

    // Read offset table
    let mut offsets = Vec::with_capacity(num_obj);
    for i in 0..num_obj {
        let off = read_int(data, table_off + i * int_size, int_size)?;
        offsets.push(off);
    }

    // Parse objects recursively starting from top_obj
    let result = parse_object(data, &offsets, ref_size, top_obj)?;

    // Convert to HashMap if it's a dict
    if let PlistValue::Dict(map) = result {
        Some(map)
    } else {
        None
    }
}

/// Plist value types.
#[derive(Debug, Clone)]
pub enum PlistValue {
    Int(i64),
    Real(f64),
    Bool(bool),
    String(String),
    Date(String),
    Data(Vec<u8>),
    Dict(HashMap<String, PlistValue>),
    Array(Vec<PlistValue>),
    Null,
}

fn parse_object(data: &[u8], offsets: &[usize], ref_size: usize, idx: usize) -> Option<PlistValue> {
    if idx >= offsets.len() { return None; }
    let off = offsets[idx];
    if off >= data.len() { return None; }

    let marker = data[off];
    let obj_type = marker >> 4;
    let obj_info = (marker & 0x0F) as usize;

    match obj_type {
        0x0 => {
            // Singleton: null, bool, fill
            match obj_info {
                0 => Some(PlistValue::Null),
                8 => Some(PlistValue::Bool(false)),
                9 => Some(PlistValue::Bool(true)),
                _ => Some(PlistValue::Null),
            }
        }
        0x1 => {
            // Int: 2^obj_info bytes
            let size = 1 << obj_info;
            let val = read_int_signed(data, off + 1, size)?;
            Some(PlistValue::Int(val))
        }
        0x2 => {
            // Real: 2^obj_info bytes
            let size = 1 << obj_info;
            if size == 4 && off + 5 <= data.len() {
                let bits = u32::from_be_bytes([data[off+1], data[off+2], data[off+3], data[off+4]]);
                Some(PlistValue::Real(f32::from_bits(bits) as f64))
            } else if size == 8 && off + 9 <= data.len() {
                let bits = u64::from_be_bytes([data[off+1], data[off+2], data[off+3], data[off+4],
                    data[off+5], data[off+6], data[off+7], data[off+8]]);
                Some(PlistValue::Real(f64::from_bits(bits)))
            } else { None }
        }
        0x3 => {
            // Date: 8-byte float64, seconds since Jan 1 2001 00:00:00 UTC
            if off + 9 > data.len() { return None; }
            let bits = u64::from_be_bytes([data[off+1], data[off+2], data[off+3], data[off+4],
                data[off+5], data[off+6], data[off+7], data[off+8]]);
            let secs_since_2001 = f64::from_bits(bits);
            // Convert to unix timestamp (2001-01-01 = 978307200 seconds since 1970-01-01)
            let unix_ts = secs_since_2001 as i64 + 978307200i64;
            let date_str = unix_ts_to_exif_date(unix_ts);
            Some(PlistValue::Date(date_str))
        }
        0x4 => {
            // Data: binary data
            let (len, extra) = if obj_info == 0x0F {
                let (l, ex) = read_length(data, off + 1)?;
                (l, 1 + ex)
            } else {
                (obj_info, 0)
            };
            let start = off + 1 + extra;
            if start + len > data.len() { return None; }
            Some(PlistValue::Data(data[start..start+len].to_vec()))
        }
        0x5 => {
            // ASCII string
            let len = if obj_info == 0x0F {
                let (l, _) = read_length(data, off + 1)?;
                l
            } else { obj_info };
            let start = if obj_info == 0x0F { off + 3 } else { off + 1 };
            if start + len > data.len() { return None; }
            Some(PlistValue::String(crate::encoding::decode_utf8_or_latin1(&data[start..start+len]).to_string()))
        }
        0x6 => {
            // UTF-16 string
            let len = if obj_info == 0x0F {
                let (l, _) = read_length(data, off + 1)?;
                l
            } else { obj_info };
            let start = if obj_info == 0x0F { off + 3 } else { off + 1 };
            let byte_len = len * 2;
            if start + byte_len > data.len() { return None; }
            let units: Vec<u16> = data[start..start+byte_len].chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
            Some(PlistValue::String(String::from_utf16_lossy(&units)))
        }
        0xA => {
            // Array
            let count = if obj_info == 0x0F {
                let (l, _) = read_length(data, off + 1)?;
                l
            } else { obj_info };
            let refs_start = if obj_info == 0x0F { off + 3 } else { off + 1 };
            let mut arr = Vec::new();
            for i in 0..count {
                let elem_ref = read_int(data, refs_start + i * ref_size, ref_size)?;
                if let Some(val) = parse_object(data, offsets, ref_size, elem_ref) {
                    arr.push(val);
                }
            }
            Some(PlistValue::Array(arr))
        }
        0xD => {
            // Dict
            let count = if obj_info == 0x0F {
                let (l, _) = read_length(data, off + 1)?;
                l
            } else { obj_info };
            let keys_start = if obj_info == 0x0F { off + 3 } else { off + 1 };
            let vals_start = keys_start + count * ref_size;

            let mut map = HashMap::new();
            for i in 0..count {
                let key_ref = read_int(data, keys_start + i * ref_size, ref_size)?;
                let val_ref = read_int(data, vals_start + i * ref_size, ref_size)?;
                if let Some(PlistValue::String(key)) = parse_object(data, offsets, ref_size, key_ref) {
                    if let Some(val) = parse_object(data, offsets, ref_size, val_ref) {
                        map.insert(key, val);
                    }
                }
            }
            Some(PlistValue::Dict(map))
        }
        _ => None,
    }
}

fn unix_ts_to_exif_date(ts: i64) -> String {
    // Get local UTC offset from TZ env var (same approach as psp.rs)
    let utc_offset = get_plist_utc_offset();
    let adjusted = ts + utc_offset;
    let secs_per_day = 86400i64;
    let days = adjusted / secs_per_day;
    let time_of_day = adjusted.rem_euclid(secs_per_day);
    let hour = time_of_day / 3600;
    let minute = (time_of_day % 3600) / 60;
    let second = time_of_day % 60;

    let mut year = 1970i32;
    let mut rem = days;
    loop {
        let dy = if plist_is_leap(year) { 366i64 } else { 365i64 };
        if rem < dy { break; }
        rem -= dy;
        year += 1;
    }
    let leap = plist_is_leap(year);
    let month_days = [31i64, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 1i32;
    for &dm in &month_days {
        if rem < dm { break; }
        rem -= dm;
        month += 1;
    }
    let day = rem + 1;
    let offset_hours = utc_offset / 3600;
    let offset_mins = (utc_offset.abs() % 3600) / 60;
    let sign = if utc_offset >= 0 { '+' } else { '-' };
    format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}{}{:02}:{:02}",
        year, month, day, hour, minute, second,
        sign, offset_hours.abs(), offset_mins)
}

fn get_plist_utc_offset() -> i64 {
    if let Ok(tz) = std::env::var("TZ") {
        let tz = tz.trim();
        if let Some(sign_pos) = tz.rfind(['+', '-']) {
            let sign: i64 = if &tz[sign_pos..sign_pos+1] == "+" { 1 } else { -1 };
            if let Ok(h) = tz[sign_pos+1..].parse::<i64>() {
                return -sign * h * 3600;
            }
        }
    }
    0
}

fn plist_is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn read_int(data: &[u8], off: usize, size: usize) -> Option<usize> {
    if off + size > data.len() { return None; }
    let mut val = 0usize;
    for i in 0..size { val = (val << 8) | data[off + i] as usize; }
    Some(val)
}

fn read_int_signed(data: &[u8], off: usize, size: usize) -> Option<i64> {
    if off + size > data.len() { return None; }
    let mut val = 0i64;
    for i in 0..size { val = (val << 8) | data[off + i] as i64; }
    Some(val)
}

fn read_length(data: &[u8], off: usize) -> Option<(usize, usize)> {
    if off >= data.len() { return None; }
    let marker = data[off];
    let size = 1 << (marker & 0x0F);
    let val = read_int(data, off + 1, size)?;
    Some((val, 1 + size))
}

// ============================================================================
// Public PLIST tag readers
// ============================================================================

/// Read a binary plist file and return ExifTool-compatible tags.
pub fn read_binary_plist_tags(data: &[u8]) -> Result<Vec<Tag>> {
    let map = match parse_binary_plist(data) {
        Some(m) => m,
        None => return Ok(Vec::new()),
    };

    let mut tags = Vec::new();
    let group = TagGroup {
        family0: "PLIST".into(),
        family1: "PLIST".into(),
        family2: "Document".into(),
    };

    flatten_plist_dict(&map, &[], &group, &mut tags);
    Ok(tags)
}

/// Flatten a plist dict (potentially nested) into tags.
/// `key_path` is the slice of ancestor keys leading to this dict.
fn flatten_plist_dict(
    map: &HashMap<String, PlistValue>,
    key_path: &[String],
    group: &TagGroup,
    tags: &mut Vec<Tag>,
) {
    for (key, val) in map {
        let mut path = key_path.to_vec();
        path.push(key.clone());
        flatten_plist_value(&path, val, group, tags);
    }
}

fn mk_plist_tag(name: String, raw_value: Value, group: &TagGroup) -> Tag {
    let print_value = raw_value.to_display_string();
    Tag {
        id: TagId::Text(name.clone()),
        name,
        description: String::new(),
        group: group.clone(),
        raw_value,
        print_value,
        priority: 0,
    }
}

fn flatten_plist_value(
    key_path: &[String],
    val: &PlistValue,
    group: &TagGroup,
    tags: &mut Vec<Tag>,
) {
    match val {
        PlistValue::Dict(inner) => {
            flatten_plist_dict(inner, key_path, group, tags);
        }
        PlistValue::Array(arr) => {
            // Join array of strings/values comma-separated
            let parts: Vec<String> = arr.iter().map(|v| plist_value_to_string(v)).collect();
            let tag_name = plist_key_path_to_tag_name(key_path);
            let tag_val = parts.join(", ");
            tags.push(mk_plist_tag(tag_name, Value::String(tag_val), group));
        }
        PlistValue::Bool(b) => {
            let tag_name = plist_key_path_to_tag_name(key_path);
            // Match ExifTool behavior: 0x08=False, 0x09=True in binary plist
            // But note: ExifTool Perl source has these inverted in the comment
            // The actual test output shows TestBoolean:False for bplist containing true bool
            // We use the standard spec: true=True, false=False
            let s = if *b { "True" } else { "False" };
            tags.push(mk_plist_tag(tag_name, Value::String(s.to_string()), group));
        }
        PlistValue::Int(i) => {
            let tag_name = plist_key_path_to_tag_name(key_path);
            tags.push(mk_plist_tag(tag_name, Value::String(i.to_string()), group));
        }
        PlistValue::Real(r) => {
            let tag_name = plist_key_path_to_tag_name(key_path);
            tags.push(mk_plist_tag(tag_name, Value::String(format_real(*r)), group));
        }
        PlistValue::String(s) => {
            let tag_name = plist_key_path_to_tag_name(key_path);
            tags.push(mk_plist_tag(tag_name, Value::String(s.clone()), group));
        }
        PlistValue::Date(s) => {
            let tag_name = plist_key_path_to_tag_name(key_path);
            tags.push(mk_plist_tag(tag_name, Value::String(s.clone()), group));
        }
        PlistValue::Data(bytes) => {
            let tag_name = plist_key_path_to_tag_name(key_path);
            tags.push(mk_plist_tag(tag_name, Value::Binary(bytes.clone()), group));
        }
        PlistValue::Null => {
            // Don't emit null values
        }
    }
}

fn plist_value_to_string(val: &PlistValue) -> String {
    match val {
        PlistValue::String(s) => s.clone(),
        PlistValue::Int(i) => i.to_string(),
        PlistValue::Real(r) => format_real(*r),
        PlistValue::Bool(b) => (if *b { "True" } else { "False" }).to_string(),
        PlistValue::Date(s) => s.clone(),
        _ => String::new(),
    }
}

fn format_real(r: f64) -> String {
    // Format like ExifTool: minimal decimal representation
    let s = format!("{}", r);
    s
}

/// Convert a key path like ["TestDict", "Author"] to tag name "TestDictAuthor".
/// Matches ExifTool's: $name =~ s/([^A-Za-z])([a-z])/$1\u$2/g; $name =~ tr/-_a-zA-Z0-9//dc; ucfirst($name)
fn plist_key_path_to_tag_name(path: &[String]) -> String {
    let tag_id = path.join("/");
    plist_tag_id_to_name(&tag_id)
}

pub fn plist_tag_id_to_name(tag_id: &str) -> String {
    // Capitalize words after non-alpha chars, remove illegal chars, ucfirst
    let mut name = String::new();
    let mut capitalize_next = false;
    for c in tag_id.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            if capitalize_next {
                for uc in c.to_uppercase() {
                    name.push(uc);
                }
                capitalize_next = false;
            } else {
                name.push(c);
            }
        } else {
            // Non-alphanumeric: remove but capitalize next letter
            capitalize_next = true;
        }
    }
    // ucfirst
    let mut result = String::new();
    let mut chars = name.chars();
    if let Some(first) = chars.next() {
        for uc in first.to_uppercase() {
            result.push(uc);
        }
        result.extend(chars);
    }
    result
}

// ============================================================================
// XML PLIST reader
// ============================================================================

/// Read an XML plist file and return ExifTool-compatible tags.
pub fn read_xml_plist(data: &[u8]) -> Result<Vec<Tag>> {
    let text = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return Ok(Vec::new()),
    };

    let mut tags = Vec::new();
    let group = TagGroup {
        family0: "PLIST".into(),
        family1: "XML".into(),
        family2: "Document".into(),
    };

    // Parse the plist dict recursively
    let mut pos = 0;
    // Skip to <dict> or <plist>
    if let Some(root) = parse_xml_plist_root(text, &mut pos) {
        flatten_plist_dict(&root, &[], &group, &mut tags);
    }

    Ok(tags)
}

/// Simple XML plist parser. Returns a dict (HashMap) representing the root dict.
fn parse_xml_plist_root(text: &str, pos: &mut usize) -> Option<HashMap<String, PlistValue>> {
    // Find the root <dict> element
    let dict_start = text.find("<dict>")?;
    *pos = dict_start + 6;
    Some(parse_xml_dict(text, pos))
}

fn parse_xml_dict(text: &str, pos: &mut usize) -> HashMap<String, PlistValue> {
    let mut map = HashMap::new();

    loop {
        skip_xml_whitespace(text, pos);
        if *pos >= text.len() { break; }

        // Check for </dict>
        if text[*pos..].starts_with("</dict>") {
            *pos += 7;
            break;
        }

        // Expect <key>...</key>
        if !text[*pos..].starts_with("<key>") {
            // Skip unknown element
            if let Some(end) = text[*pos..].find('>') {
                *pos += end + 1;
            } else {
                break;
            }
            continue;
        }
        *pos += 5; // skip "<key>"
        let key_end = match text[*pos..].find("</key>") {
            Some(e) => e,
            None => break,
        };
        let key = xml_unescape(&text[*pos..*pos + key_end]);
        *pos += key_end + 6; // skip key text + "</key>"

        skip_xml_whitespace(text, pos);
        if *pos >= text.len() { break; }

        // Parse value element
        if let Some(val) = parse_xml_value(text, pos) {
            map.insert(key, val);
        }
    }

    map
}

fn parse_xml_array(text: &str, pos: &mut usize) -> Vec<PlistValue> {
    let mut arr = Vec::new();

    loop {
        skip_xml_whitespace(text, pos);
        if *pos >= text.len() { break; }

        if text[*pos..].starts_with("</array>") {
            *pos += 8;
            break;
        }

        if let Some(val) = parse_xml_value(text, pos) {
            arr.push(val);
        } else {
            break;
        }
    }

    arr
}

fn parse_xml_value(text: &str, pos: &mut usize) -> Option<PlistValue> {
    skip_xml_whitespace(text, pos);
    if *pos >= text.len() { return None; }

    let rest = &text[*pos..];

    if rest.starts_with("<dict>") {
        *pos += 6;
        return Some(PlistValue::Dict(parse_xml_dict(text, pos)));
    }
    if rest.starts_with("<array>") {
        *pos += 7;
        return Some(PlistValue::Array(parse_xml_array(text, pos)));
    }
    if rest.starts_with("<string>") {
        *pos += 8;
        let end = text[*pos..].find("</string>")?;
        let s = xml_unescape(&text[*pos..*pos + end]);
        *pos += end + 9;
        return Some(PlistValue::String(s));
    }
    if rest.starts_with("<integer>") {
        *pos += 9;
        let end = text[*pos..].find("</integer>")?;
        let s = text[*pos..*pos + end].trim();
        let val = s.parse::<i64>().ok()?;
        *pos += end + 10;
        return Some(PlistValue::Int(val));
    }
    if rest.starts_with("<real>") {
        *pos += 6;
        let end = text[*pos..].find("</real>")?;
        let s = text[*pos..*pos + end].trim();
        let val = s.parse::<f64>().ok()?;
        *pos += end + 7;
        return Some(PlistValue::Real(val));
    }
    if rest.starts_with("<true/>") {
        *pos += 7;
        return Some(PlistValue::Bool(true));
    }
    if rest.starts_with("<false/>") {
        *pos += 8;
        return Some(PlistValue::Bool(false));
    }
    if rest.starts_with("<date>") {
        *pos += 6;
        let end = text[*pos..].find("</date>")?;
        let s = text[*pos..*pos + end].trim();
        // Convert ISO 8601 date to ExifTool format
        let date_str = convert_plist_date(s);
        *pos += end + 7;
        return Some(PlistValue::Date(date_str));
    }
    if rest.starts_with("<data>") {
        *pos += 6;
        let end = text[*pos..].find("</data>")?;
        let b64 = text[*pos..*pos + end].split_whitespace().collect::<String>();
        *pos += end + 7;
        let bytes = base64_decode_simple(&b64);
        return Some(PlistValue::Data(bytes));
    }

    // Skip unknown element
    if let Some(tag_end) = rest.find('>') {
        *pos += tag_end + 1;
    } else {
        *pos = text.len();
    }
    None
}

fn skip_xml_whitespace(text: &str, pos: &mut usize) {
    let bytes = text.as_bytes();
    while *pos < bytes.len() && (bytes[*pos] == b' ' || bytes[*pos] == b'\t'
        || bytes[*pos] == b'\n' || bytes[*pos] == b'\r') {
        *pos += 1;
    }
}

fn xml_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
     .replace("&lt;", "<")
     .replace("&gt;", ">")
     .replace("&quot;", "\"")
     .replace("&apos;", "'")
}

/// Convert ISO 8601 date "2013-02-22T12:49:10Z" to ExifTool format "2013:02:22 12:49:10Z"
fn convert_plist_date(s: &str) -> String {
    // Format: YYYY-MM-DDTHH:MM:SSZ or YYYY-MM-DDTHH:MM:SS+HH:MM
    if s.len() >= 19 {
        let date_part = &s[0..10].replace('-', ":");
        let time_part = &s[11..19];
        let tz_part = &s[19..];
        format!("{} {}{}", date_part, time_part, tz_part)
    } else {
        s.to_string()
    }
}

/// Simple base64 decode (no padding required)
fn base64_decode_simple(s: &str) -> Vec<u8> {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut table = [0u8; 256];
    for (i, &c) in alphabet.iter().enumerate() {
        table[c as usize] = i as u8;
    }

    let mut result = Vec::new();
    let bytes: Vec<u8> = s.bytes().filter(|&b| b != b'=' && b != b'\n' && b != b'\r' && b != b' ').collect();
    for chunk in bytes.chunks(4) {
        if chunk.len() < 2 { break; }
        let b0 = table[chunk[0] as usize];
        let b1 = table[chunk[1] as usize];
        result.push((b0 << 2) | (b1 >> 4));
        if chunk.len() >= 3 {
            let b2 = table[chunk[2] as usize];
            result.push((b1 << 4) | (b2 >> 2));
            if chunk.len() >= 4 {
                let b3 = table[chunk[3] as usize];
                result.push((b2 << 6) | b3);
            }
        }
    }
    result
}

/// Map known AAE binary plist key paths to ExifTool tag names.
fn aae_known_tag_name(key: &str) -> Option<&'static str> {
    match key {
        "slowMotion/regions/timeRange/start/flags" => Some("SlowMotionRegionsStartTimeFlags"),
        "slowMotion/regions/timeRange/start/value" => Some("SlowMotionRegionsStartTimeValue"),
        "slowMotion/regions/timeRange/start/timescale" => Some("SlowMotionRegionsStartTimeScale"),
        "slowMotion/regions/timeRange/start/epoch" => Some("SlowMotionRegionsStartTimeEpoch"),
        "slowMotion/regions/timeRange/duration/flags" => Some("SlowMotionRegionsDurationFlags"),
        "slowMotion/regions/timeRange/duration/value" => Some("SlowMotionRegionsDurationValue"),
        "slowMotion/regions/timeRange/duration/timescale" => Some("SlowMotionRegionsDurationTimeScale"),
        "slowMotion/regions/timeRange/duration/epoch" => Some("SlowMotionRegionsDurationEpoch"),
        "slowMotion/regions" => Some("SlowMotionRegions"),
        "slowMotion/rate" => Some("SlowMotionRate"),
        _ => None,
    }
}

/// Convert bitmask flags for SlowMotionRegions*Flags tags.
fn aae_flags_print_conv(val: i64) -> String {
    let bits = [
        (0, "Valid"),
        (1, "Has been rounded"),
        (2, "Positive infinity"),
        (3, "Negative infinity"),
        (4, "Indefinite"),
    ];
    let mut parts = Vec::new();
    for &(bit, name) in &bits {
        if val & (1 << bit) != 0 {
            parts.push(name);
        }
    }
    if parts.is_empty() {
        format!("{}", val)
    } else {
        parts.join(", ")
    }
}

/// Flatten a binary plist value recursively, collecting (path, value) pairs.
fn flatten_bplist_to_paths(val: &PlistValue, path: &str, out: &mut Vec<(String, PlistValue)>) {
    match val {
        PlistValue::Dict(map) => {
            for (k, v) in map {
                let child_path = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{}/{}", path, k)
                };
                flatten_bplist_to_paths(v, &child_path, out);
            }
        }
        PlistValue::Array(arr) => {
            // For arrays, flatten each element at the same path (like Perl's List => 1)
            // But also record the array at this path level
            out.push((path.to_string(), val.clone()));
            for item in arr {
                flatten_bplist_to_paths(item, path, out);
            }
        }
        _ => {
            out.push((path.to_string(), val.clone()));
        }
    }
}

/// Read AAE plist file (Apple Adjustments).
pub fn read_aae_plist(data: &[u8]) -> Result<Vec<Tag>> {
    if data.starts_with(b"bplist") {
        return read_binary_plist_tags(data);
    }

    let text = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return Ok(Vec::new()),
    };

    let mut tags = Vec::new();
    let group = TagGroup {
        family0: "PLIST".into(),
        family1: "XML".into(),
        family2: "Document".into(),
    };

    // Parse outer XML plist dict
    let mut pos = 0;
    let root = match parse_xml_plist_root(text, &mut pos) {
        Some(m) => m,
        None => return Ok(tags),
    };

    // Process keys — for most keys flatten normally, but for adjustmentData decode binary plist
    let mut sorted_keys: Vec<&String> = root.keys().collect();
    sorted_keys.sort();

    for key in &sorted_keys {
        let val = &root[*key];
        if key.as_str() == "adjustmentData" {
            // Parse adjustmentData as binary plist, extract nested tags
            if let PlistValue::Data(ref bytes) = val {
                let bplist_data = if bytes.starts_with(b"bplist") {
                    bytes.as_slice()
                } else {
                    continue;
                };
                if let Some(nested_map) = parse_binary_plist(bplist_data) {
                    // Flatten binary plist to path → value pairs
                    let mut paths: Vec<(String, PlistValue)> = Vec::new();
                    for (k, v) in &nested_map {
                        flatten_bplist_to_paths(v, k, &mut paths);
                    }
                    // Emit paths that match aae_known_tag_name
                    // Use a deterministic order matching ExifTool output
                    let order = [
                        "slowMotion/regions/timeRange/start/flags",
                        "slowMotion/regions/timeRange/start/value",
                        "slowMotion/regions/timeRange/start/timescale",
                        "slowMotion/regions/timeRange/start/epoch",
                        "slowMotion/regions/timeRange/duration/flags",
                        "slowMotion/regions/timeRange/duration/value",
                        "slowMotion/regions/timeRange/duration/timescale",
                        "slowMotion/regions/timeRange/duration/epoch",
                        "slowMotion/regions",
                        "slowMotion/rate",
                    ];
                    // Build a lookup map from the paths we got
                    let path_map: HashMap<String, PlistValue> = paths.into_iter().collect();
                    for &ordered_key in &order {
                        if let Some(nv) = path_map.get(ordered_key) {
                            if let Some(tag_name) = aae_known_tag_name(ordered_key) {
                                let raw_val = match nv {
                                    PlistValue::Int(i) => {
                                        if tag_name.ends_with("Flags") {
                                            Value::String(aae_flags_print_conv(*i))
                                        } else {
                                            Value::String(i.to_string())
                                        }
                                    }
                                    PlistValue::Real(r) => Value::String(format!("{}", r)),
                                    PlistValue::String(s) => Value::String(s.clone()),
                                    PlistValue::Bool(b) => Value::String(if *b { "True" } else { "False" }.to_string()),
                                    PlistValue::Array(_) => Value::String(String::new()),
                                    _ => Value::String(String::new()),
                                };
                                tags.push(mk_plist_tag(tag_name.to_string(), raw_val, &group));
                            }
                        }
                    }
                }
            }
            // Do NOT output AdjustmentData as a raw binary tag
        } else {
            let path = vec![(*key).clone()];
            flatten_plist_value(&path, val, &group, &mut tags);
        }
    }

    Ok(tags)
}
