//! BitTorrent metainfo file reader.
//!
//! Parses bencode format to extract torrent metadata.
//! Mirrors ExifTool's Torrent.pm.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

/// A parsed bencode value
enum Bencode {
    Int(i64),
    Bytes(Vec<u8>),
    List(Vec<Bencode>),
    Dict(Vec<(String, Bencode)>),
}

fn parse_bencode(data: &[u8], pos: &mut usize) -> Option<Bencode> {
    if *pos >= data.len() {
        return None;
    }
    match data[*pos] {
        b'i' => {
            *pos += 1;
            let start = *pos;
            while *pos < data.len() && data[*pos] != b'e' {
                *pos += 1;
            }
            if *pos >= data.len() {
                return None;
            }
            let s = std::str::from_utf8(&data[start..*pos]).ok()?;
            let n: i64 = s.parse().ok()?;
            *pos += 1; // skip 'e'
            Some(Bencode::Int(n))
        }
        b'd' => {
            *pos += 1;
            let mut dict = Vec::new();
            while *pos < data.len() && data[*pos] != b'e' {
                let key = parse_bencode(data, pos)?;
                let key_str = match key {
                    Bencode::Bytes(b) => String::from_utf8_lossy(&b).into_owned(),
                    _ => return None,
                };
                let val = parse_bencode(data, pos)?;
                dict.push((key_str, val));
            }
            if *pos < data.len() {
                *pos += 1; // skip 'e'
            }
            Some(Bencode::Dict(dict))
        }
        b'l' => {
            *pos += 1;
            let mut list = Vec::new();
            while *pos < data.len() && data[*pos] != b'e' {
                list.push(parse_bencode(data, pos)?);
            }
            if *pos < data.len() {
                *pos += 1; // skip 'e'
            }
            Some(Bencode::List(list))
        }
        b'0'..=b'9' => {
            let start = *pos;
            while *pos < data.len() && data[*pos] != b':' {
                *pos += 1;
            }
            if *pos >= data.len() {
                return None;
            }
            let len_str = std::str::from_utf8(&data[start..*pos]).ok()?;
            let len: usize = len_str.parse().ok()?;
            *pos += 1; // skip ':'
            if *pos + len > data.len() {
                return None;
            }
            let bytes = data[*pos..*pos + len].to_vec();
            *pos += len;
            Some(Bencode::Bytes(bytes))
        }
        _ => None,
    }
}

fn bencode_to_string(b: &Bencode) -> Option<String> {
    match b {
        Bencode::Bytes(bytes) => {
            // Try UTF-8 first
            if let Ok(s) = std::str::from_utf8(bytes) {
                Some(s.to_string())
            } else {
                None
            }
        }
        Bencode::Int(n) => Some(n.to_string()),
        _ => None,
    }
}

pub fn read_torrent(data: &[u8]) -> Result<Vec<Tag>> {
    if data.is_empty() || data[0] != b'd' {
        return Err(Error::InvalidData("not a torrent file".into()));
    }

    let mut pos = 0;
    let root = parse_bencode(data, &mut pos)
        .ok_or_else(|| Error::InvalidData("invalid bencode".into()))?;

    let dict = match &root {
        Bencode::Dict(d) => d,
        _ => return Err(Error::InvalidData("torrent root is not a dict".into())),
    };

    let mut tags = Vec::new();

    // Process top-level fields
    for (key, val) in dict {
        match key.as_str() {
            "announce" => {
                if let Some(s) = bencode_to_string(val) {
                    tags.push(mk("Announce", "Announce", Value::String(s)));
                }
            }
            "announce-list" => {
                if let Bencode::List(outer) = val {
                    let mut idx = 1usize;
                    for item in outer {
                        match item {
                            Bencode::List(inner) => {
                                for url in inner {
                                    if let Some(s) = bencode_to_string(url) {
                                        let name = format!("AnnounceList{}", idx);
                                        tags.push(mk(&name, &name, Value::String(s)));
                                        idx += 1;
                                    }
                                }
                            }
                            _ => {
                                if let Some(s) = bencode_to_string(item) {
                                    let name = format!("AnnounceList{}", idx);
                                    tags.push(mk(&name, &name, Value::String(s)));
                                    idx += 1;
                                }
                            }
                        }
                    }
                }
            }
            "comment" => {
                if let Some(s) = bencode_to_string(val) {
                    tags.push(mk("Comment", "Comment", Value::String(s)));
                }
            }
            "created by" => {
                if let Some(s) = bencode_to_string(val) {
                    tags.push(mk("Creator", "Creator", Value::String(s)));
                }
            }
            "creation date" => {
                if let Bencode::Int(ts) = val {
                    let dt = unix_to_exif_date(*ts);
                    tags.push(mk("CreateDate", "Create Date", Value::String(dt)));
                }
            }
            "encoding" => {
                if let Some(s) = bencode_to_string(val) {
                    tags.push(mk("Encoding", "Encoding", Value::String(s)));
                }
            }
            "url-list" => {
                match val {
                    Bencode::List(items) => {
                        for (i, item) in items.iter().enumerate() {
                            if let Some(s) = bencode_to_string(item) {
                                let name = format!("URLList{}", i + 1);
                                tags.push(mk(&name, &name, Value::String(s)));
                            }
                        }
                    }
                    _ => {
                        if let Some(s) = bencode_to_string(val) {
                            tags.push(mk("URLList1", "URLList1", Value::String(s)));
                        }
                    }
                }
            }
            "info" => {
                if let Bencode::Dict(info) = val {
                    process_info(info, &mut tags);
                }
            }
            _ => {}
        }
    }

    if tags.is_empty() {
        return Err(Error::InvalidData("no torrent data found".into()));
    }

    Ok(tags)
}

fn process_info(info: &[(String, Bencode)], tags: &mut Vec<Tag>) {
    // Collect files list
    let mut files_list: Option<&Vec<Bencode>> = None;

    for (key, val) in info {
        match key.as_str() {
            "name" => {
                if let Some(s) = bencode_to_string(val) {
                    tags.push(mk("Name", "Name", Value::String(s)));
                }
            }
            "piece length" => {
                if let Bencode::Int(n) = val {
                    tags.push(mk("PieceLength", "Piece Length", Value::String(n.to_string())));
                }
            }
            "pieces" => {
                if let Bencode::Bytes(b) = val {
                    tags.push(mk("Pieces", "Pieces", Value::Binary(b.clone())));
                }
            }
            "length" => {
                // Single-file torrent
                if let Bencode::Int(n) = val {
                    tags.push(mk("File1Length", "File 1 Length", Value::String(convert_file_size(*n))));
                }
            }
            "files" => {
                if let Bencode::List(list) = val {
                    files_list = Some(list);
                }
            }
            _ => {}
        }
    }

    // Process files
    if let Some(files) = files_list {
        for (i, file) in files.iter().enumerate() {
            let idx = i + 1;
            if let Bencode::Dict(fd) = file {
                for (fkey, fval) in fd {
                    match fkey.as_str() {
                        "length" => {
                            if let Bencode::Int(n) = fval {
                                let name = format!("File{}Length", idx);
                                tags.push(mk(&name, &name, Value::String(convert_file_size(*n))));
                            }
                        }
                        "path" => {
                            if let Bencode::List(parts) = fval {
                                let path: Vec<String> = parts
                                    .iter()
                                    .filter_map(bencode_to_string)
                                    .collect();
                                let path_str = path.join("/");
                                let name = format!("File{}Path", idx);
                                tags.push(mk(&name, &name, Value::String(path_str)));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

fn convert_file_size(bytes: i64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.1} kB", bytes as f64 / 1_000.0)
    } else {
        format!("{} bytes", bytes)
    }
}

fn unix_to_exif_date(ts: i64) -> String {
    // Detect local timezone offset (approximate) - ExifTool uses local time
    // We'll compute UTC and add the system timezone offset
    let utc_offset = get_local_utc_offset();

    let adjusted = ts + utc_offset;
    let secs_per_day = 86400i64;
    let mut days = adjusted / secs_per_day;
    let time_of_day = adjusted % secs_per_day;
    let (time_of_day, days) = if time_of_day < 0 {
        (time_of_day + secs_per_day, days - 1)
    } else {
        (time_of_day, days)
    };
    let hour = time_of_day / 3600;
    let minute = (time_of_day % 3600) / 60;
    let second = time_of_day % 60;

    let mut year = 1970i32;
    let mut rem = days;
    loop {
        let dy = if is_leap(year) { 366i64 } else { 365i64 };
        if rem < dy { break; }
        rem -= dy;
        year += 1;
    }
    let leap = is_leap(year);
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

fn get_local_utc_offset() -> i64 {
    // Use the TZ environment variable or /etc/localtime to determine offset
    // Simple fallback: read TZ
    if let Ok(tz) = std::env::var("TZ") {
        // Parse simple TZ like "CET-1" or "UTC+5"
        let tz = tz.trim();
        if let Some(sign_pos) = tz.rfind(['+', '-']) {
            let sign: i64 = if &tz[sign_pos..sign_pos+1] == "+" { 1 } else { -1 };
            if let Ok(h) = tz[sign_pos+1..].parse::<i64>() {
                return -sign * h * 3600; // TZ sign is inverted (CET-1 means UTC+1)
            }
        }
    }
    // Default: try to read system time difference (crude)
    // Use 0 (UTC) as fallback
    0
}

fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "Torrent".into(),
            family1: "Torrent".into(),
            family2: "Document".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}
