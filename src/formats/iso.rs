//! ISO 9660 disc image reader.
//!
//! Reads volume descriptor metadata from ISO images.
//! Mirrors ExifTool's ISO.pm ProcessISO().

use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

fn mk(name: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: "ISO".into(),
            family1: "ISO".into(),
            family2: "Other".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}

fn mk_with_print(name: &str, raw: Value, print: String) -> Tag {
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: "ISO".into(),
            family1: "ISO".into(),
            family2: "Other".into(),
        },
        raw_value: raw,
        print_value: print,
        priority: 0,
    }
}

fn trim_spaces(s: &str) -> &str {
    s.trim_end_matches(' ')
}

/// Parse ISO 9660 7.1.3 date/time (17 bytes: YYYYMMDDHHmmssss_tz)
fn parse_iso_datetime(data: &[u8]) -> Option<String> {
    if data.len() < 17 { return None; }
    // Check if non-zero
    if data[..16].iter().all(|&b| b == b'0' || b == 0 || b == b' ') { return None; }
    let s = std::str::from_utf8(&data[..16]).ok()?;
    if s.chars().all(|c| c == '0' || c == ' ') { return None; }

    let year = &s[0..4];
    let month = &s[4..6];
    let day = &s[6..8];
    let hour = &s[8..10];
    let min = &s[10..12];
    let sec = &s[12..14];
    let csec = &s[14..16];

    // Timezone: signed byte, 15-minute intervals
    let tz_byte = data[16] as i8;
    let tz_mins = tz_byte as i32 * 15;
    let tz_str = if tz_mins == 0 {
        "+00:00".to_string()
    } else {
        let sign = if tz_mins >= 0 { "+" } else { "-" };
        let abs = tz_mins.abs();
        format!("{}{:02}:{:02}", sign, abs / 60, abs % 60)
    };

    Some(format!("{}:{}:{} {}:{}:{}.{}{}", year, month, day, hour, min, sec, csec, tz_str))
}

/// Parse ISO 9660 "short" date (7 bytes: YYMMDDHHMMSS_tz)
fn parse_short_datetime(data: &[u8]) -> Option<String> {
    if data.len() < 7 { return None; }
    let year = data[0] as u32 + 1900;
    let month = data[1];
    let day = data[2];
    let hour = data[3];
    let min = data[4];
    let sec = data[5];
    let tz_byte = data[6] as i8;
    let tz_mins = tz_byte as i32 * 15;
    let tz_str = if tz_mins == 0 {
        "+00:00".to_string()
    } else {
        let sign = if tz_mins >= 0 { "+" } else { "-" };
        let abs = tz_mins.abs();
        format!("{}{:02}:{:02}", sign, abs / 60, abs % 60)
    };
    Some(format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}{}", year, month, day, hour, min, sec, tz_str))
}

/// Format file size like Perl ConvertFileSize (e.g., "391 MB")
fn format_file_size(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.0} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.0} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} kB", bytes as f64 / 1024.0)
    } else {
        format!("{} bytes", bytes)
    }
}

fn read_le_u32(data: &[u8], off: usize) -> u32 {
    if off + 4 > data.len() { return 0; }
    u32::from_le_bytes([data[off], data[off+1], data[off+2], data[off+3]])
}

fn read_le_u16(data: &[u8], off: usize) -> u16 {
    if off + 2 > data.len() { return 0; }
    u16::from_le_bytes([data[off], data[off+1]])
}

pub fn read_iso(data: &[u8]) -> crate::error::Result<Vec<Tag>> {
    // ISO 9660: volume descriptors start at sector 16 (32768 bytes)
    if data.len() < 32768 + 2048 {
        return Ok(Vec::new());
    }

    let mut tags = Vec::new();
    let mut offset = 32768usize;

    while offset + 2048 <= data.len() {
        let sector = &data[offset..offset + 2048];

        // Check magic: type byte + "CD001"
        let vol_type = sector[0];
        if &sector[1..6] != b"CD001" { break; }

        match vol_type {
            0 => {
                // Boot Record
                let boot_system = trim_spaces(
                    std::str::from_utf8(&sector[7..39]).unwrap_or("")
                );
                // Always extract BootSystem even if empty (indicates bootable)
                tags.push(mk("BootSystem", Value::String(boot_system.to_string())));
            }
            1 => {
                // Primary Volume Descriptor
                // VolumeName at offset 40, 32 bytes
                let vol_name = trim_spaces(
                    std::str::from_utf8(&sector[40..72]).unwrap_or("").trim_end_matches('\0')
                );
                if !vol_name.is_empty() {
                    tags.push(mk("VolumeName", Value::String(vol_name.to_string())));
                }

                // VolumeBlockCount at offset 80, little-endian u32
                let block_count = read_le_u32(sector, 80);
                tags.push(mk("VolumeBlockCount", Value::U32(block_count)));

                // VolumeBlockSize at offset 128, little-endian u16
                let block_size = read_le_u16(sector, 128);
                tags.push(mk("VolumeBlockSize", Value::U16(block_size)));

                // RootDirectoryCreateDate at offset 174, 7 bytes
                if sector.len() > 181 {
                    if let Some(dt) = parse_short_datetime(&sector[174..181]) {
                        tags.push(mk("RootDirectoryCreateDate", Value::String(dt)));
                    }
                }

                // VolumeSetName at offset 190, 128 bytes
                let set_name = trim_spaces(
                    std::str::from_utf8(&sector[190..318]).unwrap_or("").trim_end_matches('\0')
                );
                if !set_name.is_empty() {
                    tags.push(mk("VolumeSetName", Value::String(set_name.to_string())));
                }

                // Publisher at offset 318, 128 bytes
                let publisher = trim_spaces(
                    std::str::from_utf8(&sector[318..446]).unwrap_or("").trim_end_matches('\0')
                );
                if !publisher.is_empty() {
                    tags.push(mk("Publisher", Value::String(publisher.to_string())));
                }

                // DataPreparer at offset 446, 128 bytes
                let preparer = trim_spaces(
                    std::str::from_utf8(&sector[446..574]).unwrap_or("").trim_end_matches('\0')
                );
                if !preparer.is_empty() {
                    tags.push(mk("DataPreparer", Value::String(preparer.to_string())));
                }

                // Software at offset 574, 128 bytes
                let software = trim_spaces(
                    std::str::from_utf8(&sector[574..702]).unwrap_or("").trim_end_matches('\0')
                );
                if !software.is_empty() {
                    tags.push(mk("Software", Value::String(software.to_string())));
                }

                // CopyrightFileName at offset 702, 38 bytes
                let copyright_fn = trim_spaces(
                    std::str::from_utf8(&sector[702..740]).unwrap_or("").trim_end_matches('\0')
                );
                if !copyright_fn.is_empty() {
                    tags.push(mk("CopyrightFileName", Value::String(copyright_fn.to_string())));
                }

                // AbstractFileName at offset 740, 36 bytes
                let abstract_fn = trim_spaces(
                    std::str::from_utf8(&sector[740..776]).unwrap_or("").trim_end_matches('\0')
                );
                if !abstract_fn.is_empty() {
                    tags.push(mk("AbstractFileName", Value::String(abstract_fn.to_string())));
                }

                // BibligraphicFileName at offset 776, 37 bytes
                let biblio_fn = trim_spaces(
                    std::str::from_utf8(&sector[776..813]).unwrap_or("").trim_end_matches('\0')
                );
                if !biblio_fn.is_empty() {
                    tags.push(mk("BibligraphicFileName", Value::String(biblio_fn.to_string())));
                }

                // VolumeCreateDate at offset 813, 17 bytes
                if sector.len() > 830 {
                    if let Some(dt) = parse_iso_datetime(&sector[813..830]) {
                        tags.push(mk("VolumeCreateDate", Value::String(dt)));
                    }
                }

                // VolumeModifyDate at offset 830, 17 bytes
                if sector.len() > 847 {
                    if let Some(dt) = parse_iso_datetime(&sector[830..847]) {
                        tags.push(mk("VolumeModifyDate", Value::String(dt)));
                    }
                }

                // VolumeSize composite: block_count * block_size
                let total_bytes = block_count as u64 * block_size as u64;
                tags.push(mk_with_print("VolumeSize", Value::String(total_bytes.to_string()), format_file_size(total_bytes)));
            }
            255 => {
                // Terminator
                break;
            }
            _ => {}
        }

        offset += 2048;
    }

    Ok(tags)
}
