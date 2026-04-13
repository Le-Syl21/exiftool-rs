//! RAR archive format reader.

use super::misc::mktag;
use crate::error::{Error, Result};
use crate::tag::Tag;
use crate::value::Value;

/// Read a ULEB128 (unsigned LEB128) integer from data at pos, advancing pos.
fn read_uleb128(data: &[u8], pos: &mut usize) -> Option<u64> {
    let mut result: u64 = 0;
    let mut shift = 0u32;
    loop {
        if *pos >= data.len() {
            return None;
        }
        let byte = data[*pos];
        *pos += 1;
        result |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 64 {
            return None;
        }
    }
    Some(result)
}

pub fn read_rar(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 8 || !data.starts_with(b"Rar!\x1A\x07") {
        return Err(Error::InvalidData("not a RAR file".into()));
    }

    let mut tags = Vec::new();

    if data[6] == 0x00 {
        // RAR v4
        tags.push(mktag(
            "ZIP",
            "FileVersion",
            "File Version",
            Value::String("RAR v4".into()),
        ));
        read_rar4_entries(data, &mut tags);
    } else if data[6] == 0x01 && data[7] == 0x00 {
        // RAR v5
        tags.push(mktag(
            "ZIP",
            "FileVersion",
            "File Version",
            Value::String("RAR v5".into()),
        ));
        read_rar5_entries(data, &mut tags);
    }

    Ok(tags)
}

fn read_rar5_entries(data: &[u8], tags: &mut Vec<Tag>) {
    // After 8-byte signature, iterate blocks:
    // each block: 4 bytes CRC32, then ULEB128 headSize, then headSize bytes header
    let mut pos = 8;

    loop {
        // skip 4-byte CRC
        if pos + 4 > data.len() {
            break;
        }
        pos += 4;

        let head_size = match read_uleb128(data, &mut pos) {
            Some(v) if v > 0 => v as usize,
            _ => break,
        };

        if pos + head_size > data.len() {
            break;
        }

        let header = &data[pos..pos + head_size];
        pos += head_size;

        let mut hpos = 0;
        let head_type = match read_uleb128(header, &mut hpos) {
            Some(v) => v,
            None => break,
        };

        // Skip headFlags
        let head_flag = match read_uleb128(header, &mut hpos) {
            Some(v) => v,
            None => break,
        };

        // head_type 2 = file header, 3 = service header
        if head_type != 2 && head_type != 3 {
            // Skip data section if present
            if head_flag & 0x0002 != 0 {
                // read extra data size to skip
                if let Some(data_size) = read_uleb128(data, &mut pos) {
                    pos += data_size as usize;
                }
            }
            continue;
        }

        // skip extraSize
        let _extra_size = read_uleb128(header, &mut hpos);

        let data_size: u64 = if head_flag & 0x0002 != 0 {
            match read_uleb128(header, &mut hpos) {
                Some(v) => v,
                None => break,
            }
        } else {
            0
        };

        if head_type == 3 {
            // service header - skip its data
            pos += data_size as usize;
            continue;
        }

        // File header
        if head_type == 2 {
            tags.push(mktag(
                "ZIP",
                "CompressedSize",
                "Compressed Size",
                Value::U32(data_size as u32),
            ));
        }

        let file_flag = match read_uleb128(header, &mut hpos) {
            Some(v) => v,
            None => {
                pos += data_size as usize;
                continue;
            }
        };
        let uncompressed_size = match read_uleb128(header, &mut hpos) {
            Some(v) => v,
            None => {
                pos += data_size as usize;
                continue;
            }
        };
        if file_flag & 0x0008 == 0 {
            tags.push(mktag(
                "ZIP",
                "UncompressedSize",
                "Uncompressed Size",
                Value::U32(uncompressed_size as u32),
            ));
        }

        // skip file attributes
        let _attrs = read_uleb128(header, &mut hpos);

        // optional mtime (4 bytes)
        if file_flag & 0x0002 != 0 {
            hpos += 4;
        }
        // optional CRC (4 bytes)
        if file_flag & 0x0004 != 0 {
            hpos += 4;
        }

        // skip compressionInfo
        let _comp_info = read_uleb128(header, &mut hpos);

        // OS
        if let Some(os_val) = read_uleb128(header, &mut hpos) {
            let os_name = match os_val {
                0 => "Win32",
                1 => "Unix",
                _ => "Unknown",
            };
            tags.push(mktag(
                "ZIP",
                "OperatingSystem",
                "Operating System",
                Value::String(os_name.into()),
            ));
        }

        // filename: 1-byte length then name bytes
        if hpos < header.len() {
            let name_len = header[hpos] as usize;
            hpos += 1;
            if hpos + name_len <= header.len() {
                let name = crate::encoding::decode_utf8_or_latin1(&header[hpos..hpos + name_len])
                    .trim_end_matches('\0')
                    .to_string();
                if !name.is_empty() {
                    tags.push(mktag(
                        "ZIP",
                        "ArchivedFileName",
                        "Archived File Name",
                        Value::String(name),
                    ));
                }
            }
        }

        pos += data_size as usize;
    }
}

fn read_rar4_entries(data: &[u8], tags: &mut Vec<Tag>) {
    // RAR v4: little-endian blocks after 7-byte signature
    let mut pos = 7;

    loop {
        if pos + 7 > data.len() {
            break;
        }
        // Block header: CRC(2) Type(1) Flags(2) Size(2)
        let block_type = data[pos + 2];
        let flags = u16::from_le_bytes([data[pos + 3], data[pos + 4]]);
        let mut size = u16::from_le_bytes([data[pos + 5], data[pos + 6]]) as usize;
        size = size.saturating_sub(7);

        if flags & 0x8000 != 0 {
            if pos + 11 > data.len() {
                break;
            }
            let add_size =
                u32::from_le_bytes([data[pos + 7], data[pos + 8], data[pos + 9], data[pos + 10]])
                    as usize;
            size = size.saturating_add(add_size).saturating_sub(4);
        }

        pos += 7;

        if block_type == 0x74 && size > 0 {
            // File block
            let n = size.min(4096).min(data.len() - pos);
            if n >= 16 {
                let file_data = &data[pos..pos + n];
                let compressed =
                    u32::from_le_bytes([file_data[0], file_data[1], file_data[2], file_data[3]])
                        as u64;
                let uncompressed =
                    u32::from_le_bytes([file_data[4], file_data[5], file_data[6], file_data[7]])
                        as u64;
                let os_byte = file_data[14];
                let name_len = u16::from_le_bytes([file_data[10], file_data[11]]) as usize;
                // name starts after 25-byte base header
                if n >= 25 + name_len {
                    let name =
                        crate::encoding::decode_utf8_or_latin1(&file_data[25..25 + name_len])
                            .to_string();
                    tags.push(mktag(
                        "ZIP",
                        "CompressedSize",
                        "Compressed Size",
                        Value::U32(compressed as u32),
                    ));
                    tags.push(mktag(
                        "ZIP",
                        "UncompressedSize",
                        "Uncompressed Size",
                        Value::U32(uncompressed as u32),
                    ));
                    let os_name = match os_byte {
                        0 => "MS-DOS",
                        1 => "OS/2",
                        2 => "Win32",
                        3 => "Unix",
                        _ => "Unknown",
                    };
                    tags.push(mktag(
                        "ZIP",
                        "OperatingSystem",
                        "Operating System",
                        Value::String(os_name.into()),
                    ));
                    tags.push(mktag(
                        "ZIP",
                        "ArchivedFileName",
                        "Archived File Name",
                        Value::String(name),
                    ));
                }
            }
        }

        if size == 0 {
            break;
        }
        pos += size;
    }
}
