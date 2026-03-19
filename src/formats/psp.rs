//! Paint Shop Pro (PSP) file reader.
//!
//! Parses PSP/PSPIMAGE files.
//! Mirrors ExifTool's PSP.pm.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_psp(data: &[u8]) -> Result<Vec<Tag>> {
    // Magic: "Paint Shop Pro Image File\x0a\x1a\0\0\0\0\0" (32 bytes)
    if data.len() < 32 || !data.starts_with(b"Paint Shop Pro Image File\x0a\x1a\0\0\0\0\0") {
        return Err(Error::InvalidData("not a PSP file".into()));
    }

    let mut tags = Vec::new();

    // File version at offset 32 (4 bytes: major.minor as int16u[2])
    if data.len() < 36 {
        return Ok(tags);
    }

    let major = u16::from_le_bytes([data[32], data[33]]);
    let minor = u16::from_le_bytes([data[34], data[35]]);
    tags.push(mk("FileVersion", "File Version", Value::String(format!("{}.{}", major, minor))));

    // Block header length depends on version:
    // version > 3: 10 bytes; version <= 3: 14 bytes
    let hlen: usize = if major > 3 { 10 } else { 14 };

    // Parse blocks
    let mut pos = 36;
    while pos + hlen <= data.len() {
        // Block marker: "~BK\0"
        if &data[pos..pos + 4] != b"~BK\0" {
            break;
        }

        let block_type = u16::from_le_bytes([data[pos + 4], data[pos + 5]]);
        let block_len = u32::from_le_bytes([
            data[pos + hlen - 4], data[pos + hlen - 3],
            data[pos + hlen - 2], data[pos + hlen - 1],
        ]) as usize;

        pos += hlen;

        if pos + block_len > data.len() {
            break;
        }

        let block_data = &data[pos..pos + block_len];
        pos += block_len;

        match block_type {
            0 => {
                // Image info block
                let start = if major > 3 { 4usize } else { 0usize };
                parse_image_block(&block_data[start.min(block_data.len())..], &mut tags);
            }
            1 => {
                // Creator info block
                parse_creator_block(block_data, &mut tags);
            }
            10 => {
                // Extended data block
                parse_ext_block(block_data, &mut tags);
            }
            _ => {}
        }
    }

    Ok(tags)
}

fn parse_image_block(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 4 {
        return;
    }
    // ImageWidth (int32u) at 0
    let width = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    tags.push(mk("ImageWidth", "Image Width", Value::U32(width)));
    if data.len() < 8 {
        return;
    }
    // ImageHeight (int32u) at 4
    let height = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    tags.push(mk("ImageHeight", "Image Height", Value::U32(height)));
    if data.len() < 16 {
        return;
    }
    // ImageResolution (double) at 8
    let res_bytes: [u8; 8] = data[8..16].try_into().unwrap_or([0; 8]);
    let resolution = f64::from_le_bytes(res_bytes);
    if resolution > 0.0 {
        tags.push(mk("ImageResolution", "Image Resolution", Value::String(format!("{}", resolution))));
    }
    if data.len() < 17 {
        return;
    }
    // ResolutionUnit (int8u) at 16
    let res_unit = data[16];
    let unit_str = match res_unit {
        0 => "None",
        1 => "inches",
        2 => "cm",
        _ => "Unknown",
    };
    tags.push(mk("ResolutionUnit", "Resolution Unit", Value::String(unit_str.into())));
    if data.len() < 19 {
        return;
    }
    // Compression (int16u) at 17
    let compression = u16::from_le_bytes([data[17], data[18]]);
    let comp_str = match compression {
        0 => "None",
        1 => "RLE",
        2 => "LZ77",
        3 => "JPEG",
        _ => "Unknown",
    };
    tags.push(mk("Compression", "Compression", Value::String(comp_str.into())));
    if data.len() < 21 {
        return;
    }
    // BitsPerSample (int16u) at 19
    let bps = u16::from_le_bytes([data[19], data[20]]);
    tags.push(mk("BitsPerSample", "Bits Per Sample", Value::U16(bps)));
    if data.len() < 23 {
        return;
    }
    // Planes (int16u) at 21
    let planes = u16::from_le_bytes([data[21], data[22]]);
    tags.push(mk("Planes", "Planes", Value::U16(planes)));
    if data.len() < 27 {
        return;
    }
    // NumColors (int32u) at 23
    let num_colors = u32::from_le_bytes([data[23], data[24], data[25], data[26]]);
    tags.push(mk("NumColors", "Number of Colors", Value::U32(num_colors)));

    // XResolution and YResolution (same value, stored as resolution)
    if resolution > 0.0 {
        tags.push(mk("XResolution", "X Resolution", Value::String(format!("{}", resolution))));
        tags.push(mk("YResolution", "Y Resolution", Value::String(format!("{}", resolution))));
    }
}

fn parse_creator_block(data: &[u8], tags: &mut Vec<Tag>) {
    // Sub-blocks: "~FL\0" + tag(uint16) + len(uint32)
    let mut pos = 0;
    while pos + 10 <= data.len() {
        if &data[pos..pos + 4] != b"~FL\0" {
            break;
        }
        let tag = u16::from_le_bytes([data[pos + 4], data[pos + 5]]);
        let len = u32::from_le_bytes([data[pos + 6], data[pos + 7], data[pos + 8], data[pos + 9]]) as usize;
        pos += 10;

        if pos + len > data.len() {
            break;
        }
        let val_data = &data[pos..pos + len];
        pos += len;

        match tag {
            0 => {
                // Title
                let s = read_null_terminated_or_all(val_data);
                if !s.is_empty() {
                    tags.push(mk("Title", "Title", Value::String(s)));
                }
            }
            1 => {
                // CreateDate (int32u unix timestamp)
                if val_data.len() >= 4 {
                    let ts = u32::from_le_bytes([val_data[0], val_data[1], val_data[2], val_data[3]]) as i64;
                    let dt = unix_to_exif_date(ts);
                    tags.push(mk("CreateDate", "Create Date", Value::String(dt)));
                }
            }
            2 => {
                // ModifyDate
                if val_data.len() >= 4 {
                    let ts = u32::from_le_bytes([val_data[0], val_data[1], val_data[2], val_data[3]]) as i64;
                    let dt = unix_to_exif_date(ts);
                    tags.push(mk("ModifyDate", "Modify Date", Value::String(dt)));
                }
            }
            3 => {
                let s = read_null_terminated_or_all(val_data);
                if !s.is_empty() {
                    tags.push(mk("Artist", "Artist", Value::String(s)));
                }
            }
            4 => {
                let s = read_null_terminated_or_all(val_data);
                if !s.is_empty() {
                    tags.push(mk("Copyright", "Copyright", Value::String(s)));
                }
            }
            5 => {
                let s = read_null_terminated_or_all(val_data);
                if !s.is_empty() {
                    tags.push(mk("Description", "Description", Value::String(s)));
                }
            }
            6 => {
                // CreatorAppID
                if val_data.len() >= 4 {
                    let id = u32::from_le_bytes([val_data[0], val_data[1], val_data[2], val_data[3]]);
                    let name = match id {
                        0 => "Unknown".to_string(),
                        1 => "Paint Shop Pro".to_string(),
                        n => format!("{}", n),
                    };
                    tags.push(mk("CreatorAppID", "Creator App ID", Value::String(name)));
                }
            }
            7 => {
                // CreatorAppVersion (4 bytes little-endian, reversed)
                if val_data.len() >= 4 {
                    let v = format!("{}.{}.{}.{}", val_data[3], val_data[2], val_data[1], val_data[0]);
                    tags.push(mk("CreatorAppVersion", "Creator App Version", Value::String(v)));
                }
            }
            _ => {}
        }
    }
}

fn parse_ext_block(data: &[u8], tags: &mut Vec<Tag>) {
    // Same structure as creator block, but tag 3 contains EXIF data
    let mut pos = 0;
    while pos + 10 <= data.len() {
        if &data[pos..pos + 4] != b"~FL\0" {
            break;
        }
        let tag = u16::from_le_bytes([data[pos + 4], data[pos + 5]]);
        let len = u32::from_le_bytes([data[pos + 6], data[pos + 7], data[pos + 8], data[pos + 9]]) as usize;
        pos += 10;

        if pos + len > data.len() {
            break;
        }
        let val_data = &data[pos..pos + len];
        pos += len;

        if tag == 3 && val_data.len() > 14 && &val_data[..6] == b"Exif\0\0" {
            // EXIF block: starts with "Exif\0\0" then byte order + TIFF header
            let exif_data = &val_data[6..];
            if let Ok(exif_tags) = crate::metadata::exif::ExifReader::read(exif_data) {
                tags.extend(exif_tags);
            }
        }
    }
}

fn read_null_terminated_or_all(data: &[u8]) -> String {
    let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    String::from_utf8_lossy(&data[..end]).to_string()
}

fn unix_to_exif_date(ts: i64) -> String {
    let utc_offset = get_local_utc_offset();
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
            family0: "PSP".into(),
            family1: "PSP".into(),
            family2: "Image".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}
