//! VCard (.vcf) and iCalendar (.ics) format reader.
//! Mirrors ExifTool's VCard.pm ProcessVCard.

use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;
use crate::error::Result;

fn mk(name: &str, value: String) -> Tag {
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: "VCard".into(),
            family1: "VCard".into(),
            family2: "Document".into(),
        },
        raw_value: Value::String(value.clone()),
        print_value: value,
        priority: 0,
    }
}

fn mk_ical(name: &str, value: String) -> Tag {
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: "VCalendar".into(),
            family1: "VCalendar".into(),
            family2: "Document".into(),
        },
        raw_value: Value::String(value.clone()),
        print_value: value,
        priority: 0,
    }
}

/// Unescape vCard special sequences
fn unescape_vcard(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('\\') => result.push('\\'),
                Some(',') => result.push(','),
                Some('n') | Some('N') => result.push('\n'),
                Some(c2) => { result.push('\\'); result.push(c2); }
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Convert date/time format from vCard/iCal to EXIF style
fn convert_datetime(val: &str) -> String {
    let mut s = val.to_string();
    // YYYYMMDDTHHMMSSZ -> YYYY:MM:DD HH:MM:SSZ
    // Use regex-like replacement
    let s_bytes = s.as_bytes();
    if s_bytes.len() >= 15 && s_bytes[8] == b'T' {
        let year = &s[0..4];
        let month = &s[4..6];
        let day = &s[6..8];
        let hour = &s[9..11];
        let min = &s[11..13];
        let sec = &s[13..15];
        let tz = if s.len() > 15 { &s[15..] } else { "" };
        s = format!("{}:{}:{} {}:{}:{}{}", year, month, day, hour, min, sec, tz);
    } else if s_bytes.len() == 8 && s_bytes.iter().all(|b| b.is_ascii_digit()) {
        // YYYYMMDD -> YYYY:MM:DD
        let year = &s[0..4];
        let month = &s[4..6];
        let day = &s[6..8];
        s = format!("{}:{}:{}", year, month, day);
    }
    // YYYY-MM-DD -> YYYY:MM:DD
    if s.len() >= 10 && s.as_bytes()[4] == b'-' && s.as_bytes()[7] == b'-' {
        s = format!("{}:{}:{}{}", &s[0..4], &s[5..7], &s[8..10], &s[10..]);
    }
    s
}

/// Normalize a vCard tag name:
/// - lowercase with uppercase first letter
/// - lookup table for known name mappings
fn normalize_vcard_tag(raw_tag: &str, types: &[String]) -> (String, String) {
    // The tag ID used internally (lowercase with uppercase first)
    let tag_id = {
        let mut chars = raw_tag.chars();
        match chars.next() {
            None => String::new(),
            Some(first) => {
                let mut s = first.to_uppercase().to_string();
                s.extend(chars.map(|c| c.to_ascii_lowercase()));
                s
            }
        }
    };

    // Known name mappings from VCard.pm
    let base_name = match tag_id.as_str() {
        "Version" => "VCardVersion".into(),
        "Fn" => "FormattedName".into(),
        "N" => "Name".into(),
        "Bday" => "Birthday".into(),
        "Tz" => "TimeZone".into(),
        "Adr" => "Address".into(),
        "Geo" => "Geolocation".into(),
        "Impp" => "IMPP".into(),
        "Lang" => "Language".into(),
        "Org" => "Organization".into(),
        "Photo" => "Photo".into(),
        "Prodid" => "Software".into(),
        "Rev" => "Revision".into(),
        "Tel" => "Telephone".into(),
        "Title" => "JobTitle".into(),
        "Uid" => "UID".into(),
        "Url" => "URL".into(),
        "X-ablabel" => "ABLabel".into(),
        "X-abdate" => "ABDate".into(),
        "X-aim" => "AIM".into(),
        "X-icq" => "ICQ".into(),
        "X-abuid" => "AB_UID".into(),
        "X-abrelatednames" => "ABRelatedNames".into(),
        "X-socialprofile" => "SocialProfile".into(),
        _ => {
            // Remove X- prefix for custom tags
            if tag_id.starts_with("X-") || tag_id.starts_with("x-") {
                let stripped = &tag_id[2..];
                if !stripped.is_empty() {
                    let mut chars = stripped.chars();
                    match chars.next() {
                        None => tag_id.clone(),
                        Some(first) => {
                            let mut s = first.to_uppercase().to_string();
                            s.extend(chars.map(|c| c.to_ascii_lowercase()));
                            s
                        }
                    }
                } else {
                    tag_id.clone()
                }
            } else {
                tag_id.clone()
            }
        }
    };

    // Append type parameters to the name
    let mut name = base_name;
    for t in types {
        let t_upper: String = {
            let mut chars = t.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => {
                    let mut s = first.to_uppercase().to_string();
                    s.extend(chars.map(|c| c.to_ascii_lowercase()));
                    s
                }
            }
        };
        name.push_str(&t_upper);
    }

    (tag_id, name)
}

/// Normalize an iCalendar tag name
fn normalize_ical_tag(raw_tag: &str) -> String {
    let tag_id = {
        let mut chars = raw_tag.chars();
        match chars.next() {
            None => String::new(),
            Some(first) => {
                let mut s = first.to_uppercase().to_string();
                s.extend(chars.map(|c| c.to_ascii_lowercase()));
                s
            }
        }
    };

    match tag_id.as_str() {
        "Version" => "VCalendarVersion".into(),
        "Calscale" => "CalendarScale".into(),
        "Prodid" => "Software".into(),
        "Attach" => "Attachment".into(),
        "Class" => "Classification".into(),
        "Geo" => "Geolocation".into(),
        "Location" => "Location".into(),
        "Completed" => "DateTimeCompleted".into(),
        "Dtend" => "DateTimeEnd".into(),
        "Due" => "DateTimeDue".into(),
        "Dtstart" => "DateTimeStart".into(),
        "Freebusy" => "FreeBusyTime".into(),
        "Transp" => "TimeTransparency".into(),
        "Tzid" => "TimezoneID".into(),
        "Tzname" => "TimezoneName".into(),
        "Tzoffsetfrom" => "TimezoneOffsetFrom".into(),
        "Tzoffsetto" => "TimezoneOffsetTo".into(),
        "Tzurl" => "TimeZoneURL".into(),
        "Uid" => "UID".into(),
        "Url" => "URL".into(),
        "Created" => "DateCreated".into(),
        "Dtstamp" => "DateTimeStamp".into(),
        "Sequence" => "SequenceNumber".into(),
        "Acknowledged" => "Acknowledged".into(),
        "X-apple-calendar-color" => "CalendarColor".into(),
        "X-apple-default-alarm" => "DefaultAlarm".into(),
        "X-apple-local-default-alarm" => "LocalDefaultAlarm".into(),
        "X-wr-caldesc" => "CalendarDescription".into(),
        "X-wr-calname" => "CalendarName".into(),
        "X-wr-relcalid" => "CalendarID".into(),
        "X-wr-alarmuid" => "AlarmUID".into(),
        _ => {
            if tag_id.starts_with("X-microsoft-") {
                let stripped = &raw_tag[12..];
                let mut chars = stripped.chars();
                match chars.next() {
                    None => tag_id,
                    Some(first) => {
                        let mut s = first.to_uppercase().to_string();
                        s.extend(chars.map(|c| c.to_ascii_lowercase()));
                        s
                    }
                }
            } else if tag_id.starts_with("X-") || tag_id.starts_with("x-") {
                let stripped = &tag_id[2..];
                if !stripped.is_empty() {
                    let mut chars = stripped.chars();
                    match chars.next() {
                        None => tag_id,
                        Some(first) => {
                            let mut s = first.to_uppercase().to_string();
                            s.extend(chars.map(|c| c.to_ascii_lowercase()));
                            s
                        }
                    }
                } else {
                    tag_id
                }
            } else {
                tag_id
            }
        }
    }
}

/// Check if this is a time tag in vCard
fn is_time_tag_vcard(name: &str) -> bool {
    matches!(name, "Birthday" | "ABDate" | "TimeZone")
}

/// Check if this is a time tag in iCalendar
fn is_time_tag_ical(name: &str) -> bool {
    matches!(name,
        "DateTimeCompleted" | "DateTimeEnd" | "DateTimeDue" | "DateTimeStart" |
        "DateCreated" | "DateTimeStamp" | "ModifyDate" | "DateCreated" |
        "ExceptionDateTimes" | "RecurrenceDateTimes" | "Acknowledged"
    )
}

/// Parse vCard/iCalendar line into (tag, params, value)
/// Handles folded lines (continuation lines start with space/tab)
fn parse_lines(text: &str) -> Vec<(String, Vec<String>, String)> {
    let mut result = Vec::new();
    let mut current_line = String::new();

    // Handle both CRLF and LF line endings, and folded lines
    for raw_line in text.split('\n') {
        let line = raw_line.trim_end_matches('\r');
        if line.starts_with(' ') || line.starts_with('\t') {
            // Continuation line
            current_line.push_str(&line[1..]);
        } else {
            if !current_line.is_empty() {
                // Process previous line
                process_vcard_line(&current_line, &mut result);
            }
            current_line = line.to_string();
        }
    }
    if !current_line.is_empty() {
        process_vcard_line(&current_line, &mut result);
    }

    result
}

fn process_vcard_line(line: &str, result: &mut Vec<(String, Vec<String>, String)>) {
    // Skip empty lines or structural lines
    if line.is_empty() { return; }

    // Find the colon separating tag from value
    // But first parse the tag name and parameters
    let colon_pos = line.find(':');
    if colon_pos.is_none() { return; }
    let colon_pos = colon_pos.unwrap();

    let tag_part = &line[..colon_pos];
    let value = &line[colon_pos+1..];

    // Parse tag and parameters
    // tag_part = "TAGNAME;PARAM1=VAL;PARAM2=VAL;..."
    let semicolons: Vec<&str> = tag_part.splitn(100, ';').collect();
    if semicolons.is_empty() { return; }

    let raw_tag = semicolons[0];

    // Remove group prefix (e.g. "item1.EMAIL" -> "EMAIL")
    let raw_tag = if let Some(dot_pos) = raw_tag.find('.') {
        &raw_tag[dot_pos+1..]
    } else {
        raw_tag
    };

    // Parse TYPE parameters
    let mut types = Vec::new();
    let mut encoding = None;
    for param in &semicolons[1..] {
        let param_lower = param.to_ascii_lowercase();
        if param_lower.starts_with("type=") {
            let type_vals = &param[5..];
            for tv in type_vals.split(',') {
                let tv = tv.trim().trim_matches('"');
                if !tv.is_empty() {
                    types.push(tv.to_string());
                }
            }
        } else if param_lower.starts_with("encoding=") {
            encoding = Some(param[9..].to_ascii_lowercase());
        } else if !param.contains('=') {
            // Old vCard 2.x format: bare type parameter
            types.push(param.to_string());
        }
    }

    // Handle base64 data in value
    let mut value_str = value.to_string();
    if let Some(enc) = &encoding {
        if enc == "base64" || enc == "b" {
            // Just store as-is (binary data indicator)
            value_str = format!("(Binary data, use -b option to extract)");
        } else if enc == "quoted-printable" {
            // Decode quoted-printable
            let mut decoded = String::new();
            let mut chars = value_str.chars().peekable();
            while let Some(c) = chars.next() {
                if c == '=' {
                    let h1 = chars.next();
                    let h2 = chars.next();
                    if let (Some(h1), Some(h2)) = (h1, h2) {
                        let hex_str = format!("{}{}", h1, h2);
                        if let Ok(byte) = u8::from_str_radix(&hex_str, 16) {
                            decoded.push(byte as char);
                        }
                    }
                } else {
                    decoded.push(c);
                }
            }
            value_str = decoded;
        }
    } else {
        value_str = unescape_vcard(&value_str);
    }

    result.push((raw_tag.to_string(), types, value_str));
}

pub fn read_vcard(data: &[u8]) -> crate::error::Result<Vec<Tag>> {
    let text = String::from_utf8_lossy(data);

    // Detect type
    let is_vcalendar = text.starts_with("BEGIN:VCALENDAR") ||
        text.contains("\nBEGIN:VCALENDAR") ||
        text.starts_with("BEGIN:vcalendar");

    let lines = parse_lines(&text);
    let mut tags = Vec::new();

    // Track component nesting for iCalendar
    let mut component_stack: Vec<(String, u32)> = Vec::new(); // (name, count)
    let mut component_prefix = String::new();

    // Count for indexed component prefix (e.g. Alarm1, Alarm2)
    let mut alarm_count = 0u32;

    for (raw_tag, types, value) in &lines {
        let raw_upper = raw_tag.to_ascii_uppercase();

        // Handle BEGIN/END structural lines
        if raw_upper == "BEGIN" || raw_upper == "END" {
            let what = value.to_ascii_lowercase();
            let what = {
                let mut chars = what.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => {
                        let mut s = first.to_uppercase().to_string();
                        s.extend(chars);
                        s
                    }
                }
            };

            if raw_upper == "BEGIN" {
                if what == "Vcard" || what == "Vcalendar" || what == "Vnote" {
                    // top level, reset
                    component_stack.clear();
                    component_prefix.clear();
                    alarm_count = 0;
                } else {
                    if what == "Alarm" {
                        alarm_count += 1;
                        component_stack.push((what.clone(), alarm_count));
                    } else {
                        component_stack.push((what.clone(), 0));
                    }
                    // Rebuild prefix
                    component_prefix = component_stack.iter()
                        .map(|(name, idx)| {
                            if *idx > 0 {
                                format!("{}{}", name, idx)
                            } else {
                                name.clone()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("");
                }
            } else { // END
                if !component_stack.is_empty() {
                    component_stack.pop();
                    component_prefix = component_stack.iter()
                        .map(|(name, idx)| {
                            if *idx > 0 {
                                format!("{}{}", name, idx)
                            } else {
                                name.clone()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("");
                }
            }
            continue;
        }

        let val_str = value.clone();

        if is_vcalendar {
            let base_name = normalize_ical_tag(raw_tag);

            // Apply component prefix if inside a component
            let full_name = if component_prefix.is_empty() {
                base_name.clone()
            } else {
                format!("{}{}", component_prefix, base_name)
            };

            // Handle date/time conversions
            let display_val = if is_time_tag_ical(&base_name) || is_time_tag_ical(&full_name) {
                convert_datetime(&val_str)
            } else {
                val_str.clone()
            };

            // Also handle Last-modified specially
            let full_name = if raw_tag.to_ascii_lowercase() == "last-modified" {
                if component_prefix.is_empty() { "ModifyDate".into() } else { format!("{}ModifyDate", component_prefix) }
            } else {
                full_name
            };

            // Handle TZID parameter (timezone ID for datetime)
            // Add TZID as a separate tag
            let mut tag = mk_ical(&full_name, display_val);
            tag.group.family0 = "VCalendar".into();
            tag.group.family1 = "VCalendar".into();
            tags.push(tag);

        } else {
            // vCard
            let (_, base_name) = normalize_vcard_tag(raw_tag, types);

            let display_val = if is_time_tag_vcard(&base_name) {
                convert_datetime(&val_str)
            } else {
                val_str.clone()
            };

            // Handle PHOTO with base64
            let display_val = if raw_tag.to_ascii_uppercase() == "PHOTO" && val_str.contains("base64,") {
                // Extract just type info
                let img_type = if val_str.contains("image/jpeg") || types.iter().any(|t| t.to_ascii_uppercase() == "JPEG") {
                    "Jpeg"
                } else if val_str.contains("image/png") {
                    "Png"
                } else {
                    "Unknown"
                };
                format!("(Binary data, use -b option to extract)")
            } else {
                display_val
            };

            let tag = mk(&base_name, display_val);
            tags.push(tag);
        }
    }

    Ok(tags)
}

/// Read a VCF (vCard) file
pub fn read_vcf(data: &[u8]) -> Result<Vec<Tag>> {
    read_vcard(data)
}

/// Read an ICS (iCalendar) file
pub fn read_ics(data: &[u8]) -> Result<Vec<Tag>> {
    read_vcard(data)
}
