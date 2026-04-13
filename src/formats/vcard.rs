//! VCard (.vcf) and iCalendar (.ics) format reader.
//! Mirrors ExifTool's VCard.pm ProcessVCard.

use crate::error::Result;
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

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
                Some(c2) => {
                    result.push('\\');
                    result.push(c2);
                }
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

/// Normalize a vCard tag name
fn normalize_vcard_tag(raw_tag: &str) -> String {
    let tag_id = ucfirst_lower(raw_tag);

    match tag_id.as_str() {
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
            if let Some(stripped) = tag_id.strip_prefix("X-") {
                ucfirst_lower(stripped)
            } else {
                tag_id
            }
        }
    }
}

fn ucfirst_lower(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => {
            let mut result = first.to_uppercase().to_string();
            result.extend(chars.map(|c| c.to_ascii_lowercase()));
            result
        }
    }
}

fn ucfirst(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => {
            let mut result = first.to_uppercase().to_string();
            result.extend(chars);
            result
        }
    }
}

/// Normalize an iCalendar tag name
fn normalize_ical_tag(raw_tag: &str) -> String {
    let tag_id = ucfirst_lower(raw_tag);

    match tag_id.as_str() {
        "Version" => "VCalendarVersion".into(),
        "Calscale" => "CalendarScale".into(),
        "Prodid" => "Software".into(),
        "Attach" => "Attachment".into(),
        "Class" => "Classification".into(),
        "Geo" => "Geolocation".into(),
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
        "X-apple-calendar-color" => "CalendarColor".into(),
        "X-apple-default-alarm" => "DefaultAlarm".into(),
        "X-apple-local-default-alarm" => "LocalDefaultAlarm".into(),
        "X-wr-caldesc" => "CalendarDescription".into(),
        "X-wr-calname" => "CalendarName".into(),
        "X-wr-relcalid" => "CalendarID".into(),
        "X-wr-alarmuid" => "AlarmUID".into(),
        "X-wr-timezone" => "TimeZone2".into(),
        "Last-modified" => "ModifyDate".into(),
        "Recurrence-id" => "RecurrenceID".into(),
        "Exdate" => "ExceptionDateTimes".into(),
        "Rdate" => "RecurrenceDateTimes".into(),
        "Rrule" => "RecurrenceRule".into(),
        _ => {
            if tag_id.starts_with("X-microsoft-") {
                ucfirst(&raw_tag[12..])
            } else if let Some(stripped) = tag_id.strip_prefix("X-") {
                ucfirst_lower(stripped)
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
    matches!(
        name,
        "DateTimeCompleted"
            | "DateTimeEnd"
            | "DateTimeDue"
            | "DateTimeStart"
            | "DateCreated"
            | "DateTimeStamp"
            | "ModifyDate"
            | "ExceptionDateTimes"
            | "RecurrenceDateTimes"
            | "Acknowledged"
            | "RecurrenceID"
    )
}

/// Parsed vCard/iCal line
struct ParsedLine {
    tag: String,
    types: Vec<String>,
    language: Option<String>,
    geo: Option<String>,
    label: Option<String>,
    tzid: Option<String>,
    extra_types: Vec<String>, // for inline base64 data types (e.g., "ImageJpeg")
    value: String,
}

/// Parse a single unfolded vCard line
fn parse_vcard_line(line: &str) -> Option<ParsedLine> {
    if line.is_empty() {
        return None;
    }

    // Parse tag name (up to first ';' or ':')
    let mut pos;
    let bytes = line.as_bytes();

    // Skip group prefix (e.g., "item1.")
    let tag_start = if let Some(dot) = line.find('.') {
        // Only skip if before the colon and semicolon
        let first_sep = line.find([':', ';']).unwrap_or(line.len());
        if dot < first_sep {
            dot + 1
        } else {
            0
        }
    } else {
        0
    };

    pos = tag_start;
    let tag_name_start = pos;

    // Read tag name
    while pos < bytes.len() && bytes[pos] != b';' && bytes[pos] != b':' {
        pos += 1;
    }

    if pos >= bytes.len() {
        return None;
    }
    let raw_tag = &line[tag_name_start..pos];
    if raw_tag.is_empty() {
        return None;
    }

    // Parse parameters
    let mut types = Vec::new();
    let mut encoding = None;
    let mut language = None;
    let mut geo: Option<String> = None;
    let mut label: Option<String> = None;
    let mut tzid: Option<String> = None;

    while pos < bytes.len() && bytes[pos] == b';' {
        pos += 1; // skip ';'

        // Read parameter name
        let param_start = pos;
        while pos < bytes.len() && bytes[pos] != b'=' && bytes[pos] != b':' && bytes[pos] != b';' {
            pos += 1;
        }
        let param_name = &line[param_start..pos];
        let param_name_lower = param_name.to_ascii_lowercase();

        if pos < bytes.len() && bytes[pos] == b'=' {
            pos += 1; // skip '='

            // Read parameter value (may be quoted)
            let mut param_val = String::new();
            while pos < bytes.len() && bytes[pos] != b':' && bytes[pos] != b';' {
                if bytes[pos] == b'"' {
                    // Quoted string
                    pos += 1;
                    while pos < bytes.len() && bytes[pos] != b'"' {
                        param_val.push(bytes[pos] as char);
                        pos += 1;
                    }
                    if pos < bytes.len() {
                        pos += 1;
                    } // skip closing quote
                      // Skip optional comma after quoted value
                    if pos < bytes.len() && bytes[pos] == b',' {
                        pos += 1;
                    }
                } else {
                    // Unquoted - read until , ; :
                    let val_start = pos;
                    while pos < bytes.len()
                        && bytes[pos] != b','
                        && bytes[pos] != b':'
                        && bytes[pos] != b';'
                    {
                        pos += 1;
                    }
                    param_val.push_str(&line[val_start..pos]);
                    if pos < bytes.len() && bytes[pos] == b',' {
                        pos += 1; // skip comma between multi-values
                    } else {
                        break;
                    }
                }
            }

            match param_name_lower.as_str() {
                "type" => {
                    // May be comma-separated list
                    for tv in param_val.split(',') {
                        let tv = tv.trim().trim_matches('"');
                        if !tv.is_empty() {
                            types.push(ucfirst_lower(tv));
                        }
                    }
                }
                "encoding" => {
                    encoding = Some(param_val.to_ascii_lowercase());
                }
                "language" => {
                    language = Some(param_val);
                }
                "geo" => {
                    // Remove "geo:" prefix if present
                    let v = if let Some(stripped) = param_val.strip_prefix("geo:") {
                        stripped.to_string()
                    } else {
                        param_val
                    };
                    // Convert comma to ", " for display
                    let v = v.replace(',', ", ");
                    geo = Some(v);
                }
                "label" => {
                    let v = unescape_vcard(&param_val);
                    label = Some(v);
                }
                "tzid" => {
                    tzid = Some(param_val);
                }
                _ => {} // ignore other params
            }
        } else {
            // Bare parameter (old vCard 2.x style) - treat as TYPE
            if !param_name.is_empty() && bytes[pos] != b':' {
                types.push(ucfirst_lower(param_name));
            } else if !param_name.is_empty() {
                // Check if it's a known encoding
                let pn_lower = param_name.to_ascii_lowercase();
                match pn_lower.as_str() {
                    "quoted-printable" => {
                        encoding = Some("quoted-printable".into());
                    }
                    "base64" | "b" => {
                        encoding = Some("base64".into());
                    }
                    _ => {
                        types.push(ucfirst_lower(param_name));
                    }
                }
            }
        }
    }

    // Now we should be at ':'
    if pos >= bytes.len() || bytes[pos] != b':' {
        return None;
    }
    pos += 1; // skip ':'

    let value_str = &line[pos..];

    // Check for inline base64 data: "data:type/subtype;base64,"
    let mut extra_types = Vec::new();

    if let Some(rest) = value_str.strip_prefix("data:") {
        if let Some(semi) = rest.find(';') {
            let mime_type = &rest[..semi];
            if rest[semi..].starts_with(";base64,") {
                // Extract type/subtype and set encoding
                if let Some(slash) = mime_type.find('/') {
                    let t1 = ucfirst(&mime_type[..slash]);
                    let t2 = ucfirst(&mime_type[slash + 1..]);
                    extra_types.push(format!("{}{}", t1, t2));
                }
                encoding = Some("base64".into());
                // The actual base64 data comes after "data:type/sub;base64,"
                // (value computed below)
                // Note: we'll replace this with binary indicator below
            }
        }
    }

    // Apply encoding
    let final_value = match encoding.as_deref() {
        Some("base64") | Some("b") => {
            // For PHOTO/LOGO/SOUND - just indicate binary
            "(Binary data, use -b option to extract)".to_string()
        }
        Some("quoted-printable") => decode_qp(value_str),
        _ => unescape_vcard(value_str),
    };

    Some(ParsedLine {
        tag: raw_tag.to_string(),
        types,
        language,
        geo,
        label,
        tzid,
        extra_types,
        value: final_value,
    })
}

/// Decode quoted-printable encoding
fn decode_qp(s: &str) -> String {
    let mut result = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'=' && i + 2 < bytes.len() {
            let h1 = bytes[i + 1];
            let h2 = bytes[i + 2];
            if h1 == b'\r' || h1 == b'\n' {
                // Soft line break - skip
                i += 2;
                if i < bytes.len() && bytes[i] == b'\n' {
                    i += 1;
                }
                continue;
            }
            if let (Some(n1), Some(n2)) = (hex_val(h1), hex_val(h2)) {
                result.push((n1 << 4) | n2);
                i += 3;
                continue;
            }
        }
        result.push(bytes[i]);
        i += 1;
    }
    crate::encoding::decode_utf8_or_latin1(&result).to_string()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Parse all unfolded lines from vCard/iCal text
fn parse_lines_raw(text: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();

    for raw_line in text.split('\n') {
        let line = raw_line.trim_end_matches('\r');
        if line.starts_with(' ') || line.starts_with('\t') {
            // Continuation line (folded)
            current.push_str(&line[1..]);
        } else {
            if !current.is_empty() {
                result.push(current.clone());
            }
            current = line.to_string();
        }
    }
    if !current.is_empty() {
        result.push(current);
    }
    result
}

/// iCalendar isComponent set (top-level components that set the family1 group name)
/// These component names are WITHOUT the V prefix (VEVENT→Event, VTIMEZONE→Timezone).
fn is_ical_component(name: &str) -> bool {
    matches!(
        name,
        "Event" | "Todo" | "Journal" | "Freebusy" | "Timezone" | "Alarm"
    )
}

pub fn read_vcard(data: &[u8]) -> crate::error::Result<Vec<Tag>> {
    let text = crate::encoding::decode_utf8_or_latin1(data);

    let is_vcalendar = text.starts_with("BEGIN:VCALENDAR")
        || text.contains("\nBEGIN:VCALENDAR")
        || text.to_ascii_uppercase().contains("BEGIN:VCALENDAR");

    let raw_lines = parse_lines_raw(&text);
    let mut tags = Vec::new();

    // iCalendar component tracking (mirrors Perl @count + $component logic):
    //
    // top_component: currently active top-level isComponent (Event, Timezone, etc.), None outside
    // sub_stack: stack of (obj_name, index) for nested sub-components WITHIN a top-level component
    //   e.g. VALARM(1) inside VEVENT → sub_stack = [("Alarm", 1)]
    //   e.g. DAYLIGHT(1) inside VTIMEZONE → sub_stack = [("Daylight", 1)]
    // sub_count_stack: per-nesting-level counters for sub-component types
    //   sub_count_stack[0] = counts at current depth (reset when entering new top-level component)
    //   sub_count_stack[1] = counts within the first sub-component level, etc.
    //
    // Tag prefix for tags inside sub-components: "Alarm1", "Daylight1", etc.
    // Tags directly inside a top-level component (VEVENT, VTIMEZONE) are FLAT (no prefix).
    let mut top_component: Option<String> = None;
    let mut sub_stack: Vec<(String, u32)> = Vec::new();
    let mut sub_count_stack: Vec<std::collections::HashMap<String, u32>> =
        vec![std::collections::HashMap::new()];

    for line in &raw_lines {
        let upper = line.to_ascii_uppercase();

        // Handle BEGIN/END
        if upper.starts_with("BEGIN:") || upper.starts_with("END:") {
            let is_begin = upper.starts_with("BEGIN:");
            let what_raw = if is_begin { &line[6..] } else { &line[4..] };
            let what_lower = what_raw.to_ascii_lowercase();
            // Capitalize first letter (preserving rest as-is doesn't matter, we lowercase first)
            let what_cap = ucfirst(&what_lower);

            // Is this VCARD/VCALENDAR/VNOTE (outermost container)?
            let is_outer = matches!(what_cap.as_str(), "Vcard" | "Vcalendar" | "Vnote");

            // Strip optional "V" prefix for isComponent check (VEVENT→Event, VALARM→Alarm, etc.)
            // but DAYLIGHT, STANDARD don't have "V" prefix so they stay as-is
            let what_no_v = if what_cap.starts_with('V') && what_cap.len() > 1 {
                ucfirst(&what_cap[1..])
            } else {
                what_cap.clone()
            };

            if is_begin {
                if is_outer {
                    // New VCALENDAR/VCARD/VNOTE: reset all tracking
                    top_component = None;
                    sub_stack.clear();
                    sub_count_stack = vec![std::collections::HashMap::new()];
                } else if is_ical_component(&what_no_v) && top_component.is_none() {
                    // New top-level isComponent (VEVENT, VTIMEZONE, etc.) while not in one
                    top_component = Some(what_no_v);
                    // Reset sub-component tracking for each new top-level component
                    sub_stack.clear();
                    sub_count_stack = vec![std::collections::HashMap::new()];
                } else {
                    // Sub-component (VALARM, DAYLIGHT, STANDARD, etc.)
                    // In Perl: `$count[-1]{$what}++ if $v` where $v is non-empty only for V-prefixed names
                    // (VALARM→$v="V", Alarm; DAYLIGHT→$v="", Daylight)
                    // The $what used is WITHOUT the V prefix (Alarm, Daylight, Standard)
                    let has_v_prefix = what_cap.starts_with('V') && what_cap.len() > 1;
                    let obj_name = what_no_v.clone(); // component name without V prefix

                    let idx = if has_v_prefix {
                        // Only V-prefixed components get a count (e.g. VALARM → Alarm1)
                        let cnt = sub_count_stack
                            .last_mut()
                            .unwrap()
                            .entry(obj_name.clone())
                            .or_insert(0);
                        *cnt += 1;
                        *cnt
                    } else {
                        // Non-V components (DAYLIGHT, STANDARD) don't get a count → use 0 (displayed as empty)
                        0
                    };
                    sub_stack.push((obj_name, idx));
                    sub_count_stack.push(std::collections::HashMap::new());
                }
            } else {
                // END
                if is_outer {
                    // nothing special
                } else if is_ical_component(&what_no_v)
                    && top_component.as_deref() == Some(what_no_v.as_str())
                {
                    // Ending the top-level component
                    top_component = None;
                    sub_stack.clear();
                    sub_count_stack = vec![std::collections::HashMap::new()];
                } else if !sub_stack.is_empty() {
                    sub_stack.pop();
                    sub_count_stack.pop();
                }
            }
            continue;
        }

        let parsed = match parse_vcard_line(line) {
            Some(p) => p,
            None => continue,
        };

        let raw_upper = parsed.tag.to_ascii_uppercase();
        if raw_upper == "BEGIN" || raw_upper == "END" {
            continue;
        }

        if is_vcalendar {
            // Build tag prefix from sub_stack only (top-level component tags are flat)
            // Index 0 means no number suffix (for non-V-prefixed components like DAYLIGHT)
            let prefix: String = sub_stack
                .iter()
                .map(|(name, idx)| {
                    if *idx == 0 {
                        name.clone()
                    } else {
                        format!("{}{}", name, idx)
                    }
                })
                .collect();
            emit_ical_tag(&parsed, &prefix, &mut tags);
        } else {
            emit_vcard_tag(&parsed, &mut tags);
        }
    }

    Ok(tags)
}

fn emit_vcard_tag(parsed: &ParsedLine, tags: &mut Vec<Tag>) {
    let base_name = normalize_vcard_tag(&parsed.tag);

    // Build type suffix
    let type_suffix: String = parsed.types.iter().cloned().collect();

    // Build extra suffix from inline data types
    let extra_suffix: String = parsed.extra_types.join("");

    // Full name: base + type_suffix + extra_suffix
    let mut full_name = format!("{}{}{}", base_name, type_suffix, extra_suffix);

    // Apply language suffix: Note-fr
    if let Some(ref lang) = parsed.language {
        full_name = format!("{}-{}", full_name, lang);
    }

    let val = parsed.value.clone();

    // Convert datetime if needed
    let display_val = if is_time_tag_vcard(&base_name) {
        convert_datetime(&val)
    } else {
        val.clone()
    };

    tags.push(mk(&full_name, display_val));

    // Emit extra parameter tags (GEO, LABEL, TZID)
    if let Some(ref geo_val) = parsed.geo {
        let geo_tag = format!("{}{}Geolocation", base_name, type_suffix);
        tags.push(mk(&geo_tag, geo_val.clone()));
    }
    if let Some(ref lbl_val) = parsed.label {
        let lbl_tag = format!("{}{}Label", base_name, type_suffix);
        tags.push(mk(&lbl_tag, lbl_val.clone()));
    }
}

fn emit_ical_tag(parsed: &ParsedLine, component_prefix: &str, tags: &mut Vec<Tag>) {
    let base_name = normalize_ical_tag(&parsed.tag);

    let full_name = if component_prefix.is_empty() {
        base_name.clone()
    } else {
        format!("{}{}", component_prefix, base_name)
    };

    let val = parsed.value.clone();

    let display_val = if is_time_tag_ical(&base_name) || is_time_tag_ical(&full_name) {
        convert_datetime(&val)
    } else {
        val.clone()
    };

    let mut tag = mk_ical(&full_name, display_val);
    tag.group.family0 = "VCalendar".into();
    tag.group.family1 = "VCalendar".into();
    tags.push(tag);

    // TZID parameter
    if let Some(ref tzid_val) = parsed.tzid {
        let tzid_tag = format!("{}TimezoneID", full_name);
        let mut t = mk_ical(&tzid_tag, tzid_val.clone());
        t.group.family0 = "VCalendar".into();
        t.group.family1 = "VCalendar".into();
        tags.push(t);
    }
}

/// Read a VCF (vCard) file
pub fn read_vcf(data: &[u8]) -> Result<Vec<Tag>> {
    read_vcard(data)
}

/// Read an ICS (iCalendar) file
pub fn read_ics(data: &[u8]) -> Result<Vec<Tag>> {
    read_vcard(data)
}
