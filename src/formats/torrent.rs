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
                    Bencode::Bytes(b) => crate::encoding::decode_utf8_or_latin1(&b),
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
                    let dt = crate::formats::gzip::gzip_unix_to_datetime(*ts);
                    tags.push(mk("CreateDate", "Create Date", Value::String(dt)));
                }
            }
            "encoding" => {
                if let Some(s) = bencode_to_string(val) {
                    tags.push(mk("Encoding", "Encoding", Value::String(s)));
                }
            }
            "url-list" => match val {
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
            },
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
                    tags.push(mk(
                        "PieceLength",
                        "Piece Length",
                        Value::String(n.to_string()),
                    ));
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
                    tags.push(mk(
                        "File1Length",
                        "File 1 Length",
                        Value::String(convert_file_size(*n)),
                    ));
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
                                let path: Vec<String> =
                                    parts.iter().filter_map(bencode_to_string).collect();
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

/// Port of ExifTool ConvertFileSize (decimal units): %.1f below 10× a unit, %.0f above.
fn convert_file_size(bytes: i64) -> String {
    let v = bytes as f64;
    if bytes < 2000 {
        format!("{} bytes", bytes)
    } else if bytes < 10_000 {
        format!("{:.1} kB", v / 1000.0)
    } else if bytes < 2_000_000 {
        format!("{:.0} kB", v / 1000.0)
    } else if bytes < 10_000_000 {
        format!("{:.1} MB", v / 1_000_000.0)
    } else if bytes < 2_000_000_000 {
        format!("{:.0} MB", v / 1_000_000.0)
    } else if bytes < 10_000_000_000 {
        format!("{:.1} GB", v / 1_000_000_000.0)
    } else {
        format!("{:.0} GB", v / 1_000_000_000.0)
    }
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
