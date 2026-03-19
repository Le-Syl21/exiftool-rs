//! Windows URL shortcut (.url) and LNK file reader.
//!
//! .url files are INI-format text files with key=value pairs.
//! Mirrors ExifTool's LNK.pm ProcessINI() for URL files.

use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

fn mk(name: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: "LNK".into(),
            family1: "LNK".into(),
            family2: "Other".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}

/// Convert Windows FILETIME (100-ns intervals since 1601-01-01) hex string to ExifTool datetime
fn filetime_hex_to_datetime(hex: &str) -> Option<String> {
    // Parse 16-char hex string as little-endian 64-bit
    let bytes = hex::decode(hex).ok()?;
    if bytes.len() < 8 {
        return None;
    }
    let lo = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as u64;
    let hi = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as u64;
    let filetime = hi * 4294967296 + lo;
    if filetime == 0 {
        return None;
    }
    // Convert from 100-ns intervals since 1601-01-01 to Unix time
    // Offset: 11644473600 seconds between 1601-01-01 and 1970-01-01
    let unix_secs = (filetime as f64) / 1e7 - 11644473600.0;
    if unix_secs < 0.0 {
        return None;
    }
    // Format as ExifTool datetime
    let unix_secs_i64 = unix_secs as i64;
    Some(format_unix_time(unix_secs_i64))
}

fn format_unix_time(unix: i64) -> String {
    // Simple conversion: days since epoch
    let secs = unix % 86400;
    let days = unix / 86400;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;

    // Calculate year/month/day from days since 1970-01-01
    let (year, month, day) = days_to_date(days);
    format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}", year, month, day, h, m, s)
}

fn days_to_date(days: i64) -> (i32, u32, u32) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

/// ShowCommand print conversion (matches Perl: 1=>Normal, 2=>Minimized, 3=>Maximized)
fn show_command_print(val: &str) -> String {
    match val.trim() {
        "1" => "Normal".to_string(),
        "2" => "Minimized".to_string(),
        "3" => "Maximized".to_string(),
        v => v.to_string(),
    }
}

/// Read a Windows .url file (INI-format)
pub fn read_url(data: &[u8]) -> crate::error::Result<Vec<Tag>> {
    let text = String::from_utf8_lossy(data);
    let mut tags = Vec::new();

    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.starts_with('[') {
            continue; // section header
        }
        if let Some(eq) = line.find('=') {
            let key = &line[..eq];
            let val = &line[eq + 1..];

            match key {
                "URL" => tags.push(mk("URL", Value::String(val.to_string()))),
                "IconFile" => tags.push(mk("IconFile", Value::String(val.to_string()))),
                "IconIndex" => tags.push(mk("IconIndex", Value::String(val.to_string()))),
                "WorkingDirectory" => tags.push(mk("WorkingDirectory", Value::String(val.to_string()))),
                "HotKey" => tags.push(mk("HotKey", Value::String(val.to_string()))),
                "ShowCommand" => {
                    let pval = show_command_print(val);
                    let mut t = mk("ShowCommand", Value::String(val.to_string()));
                    t.print_value = pval;
                    tags.push(t);
                }
                "Modified" => {
                    // Modified is a FILETIME hex string
                    if let Some(dt) = filetime_hex_to_datetime(val) {
                        let mut t = mk("Modified", Value::String(val.to_string()));
                        t.print_value = dt;
                        tags.push(t);
                    } else {
                        tags.push(mk("Modified", Value::String(val.to_string())));
                    }
                }
                "Author" => tags.push(mk("Author", Value::String(val.to_string()))),
                "WhatsNew" => tags.push(mk("WhatsNew", Value::String(val.to_string()))),
                "Comment" => tags.push(mk("Comment", Value::String(val.to_string()))),
                "Desc" => tags.push(mk("Desc", Value::String(val.to_string()))),
                "Roamed" => tags.push(mk("Roamed", Value::String(val.to_string()))),
                "IDList" => tags.push(mk("IDList", Value::String(val.to_string()))),
                _ => {} // unknown keys ignored
            }
        }
    }

    Ok(tags)
}
