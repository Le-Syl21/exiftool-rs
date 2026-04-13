//! 7-Zip (7Z) archive format reader.

use super::misc::mktag;
use super::pcap::unix_to_datetime;
use crate::error::{Error, Result};
use crate::tag::Tag;
use crate::value::Value;

pub fn read_7z(data: &[u8]) -> Result<Vec<Tag>> {
    // 7z signature: 37 7A BC AF 27 1C
    if data.len() < 32 || !data.starts_with(b"7z\xBC\xAF\x27\x1C") {
        return Err(Error::InvalidData("not a 7z file".into()));
    }

    let mut tags = Vec::new();

    // Version: bytes 6-7 (major, minor)
    let major = data[6];
    let minor = data[7];
    tags.push(mktag(
        "ZIP",
        "FileVersion",
        "File Version",
        Value::String(format!("7z v{}.{:02}", major, minor)),
    ));

    // Start Header: skip CRC (4 bytes at offset 8)
    // NextHeaderOffset (8 bytes at offset 12), NextHeaderSize (8 bytes at offset 20)
    // NextHeaderCRC (4 bytes at offset 28)
    let next_header_offset = u64::from_le_bytes([
        data[12], data[13], data[14], data[15], data[16], data[17], data[18], data[19],
    ]) as usize;
    let next_header_size = u64::from_le_bytes([
        data[20], data[21], data[22], data[23], data[24], data[25], data[26], data[27],
    ]) as usize;

    // Next header starts after the 32-byte start header
    let header_start = 32 + next_header_offset;
    if header_start >= data.len() || header_start + next_header_size > data.len() {
        return Ok(tags);
    }

    let header = &data[header_start..header_start + next_header_size];
    if header.is_empty() {
        return Ok(tags);
    }

    let pid = header[0];
    if pid == 0x01 {
        // Normal (uncompressed) header — parse file info
        sevenz_extract_header(&header[1..], &mut tags);
    }
    // pid == 0x17 (23) = encoded header — requires LZMA decompression, skip

    Ok(tags)
}

/// Read a 7z variable-length encoded uint64.
/// Returns (value, bytes_consumed) or None if data is insufficient.
fn sevenz_read_uint64(data: &[u8]) -> Option<(u64, usize)> {
    if data.is_empty() {
        return None;
    }
    let b = data[0];
    if b == 0xFF {
        // Full 8-byte value follows
        if data.len() < 9 {
            return None;
        }
        let v = u64::from_le_bytes([
            data[1], data[2], data[3], data[4], data[5], data[6], data[7], data[8],
        ]);
        return Some((v, 9));
    }

    let thresholds: &[u8] = &[0x7F, 0xBF, 0xDF, 0xEF, 0xF7, 0xFB, 0xFD, 0xFE];
    let mut mask: u8 = 0x80;
    let mut extra_bytes = 0usize;

    for (i, &threshold) in thresholds.iter().enumerate() {
        if b <= threshold {
            extra_bytes = i;
            break;
        }
        mask >>= 1;
        if i == thresholds.len() - 1 {
            extra_bytes = 8;
        }
    }

    if extra_bytes == 0 {
        return Some(((b & (mask - 1)) as u64, 1));
    }

    if data.len() < 1 + extra_bytes {
        return None;
    }

    let mut buf = [0u8; 8];
    buf[..extra_bytes].copy_from_slice(&data[1..1 + extra_bytes]);
    let value = u64::from_le_bytes(buf);
    let high = (b & mask.wrapping_sub(1)) as u64;
    Some((value + (high << (extra_bytes * 8)), 1 + extra_bytes))
}

/// Read booleans from 7z bitfield.
fn sevenz_read_booleans(data: &[u8], pos: &mut usize, count: usize, check_all: bool) -> Vec<bool> {
    let mut result = Vec::with_capacity(count);
    if check_all {
        if *pos >= data.len() {
            return vec![false; count];
        }
        let all_defined = data[*pos];
        *pos += 1;
        if all_defined != 0 {
            return vec![true; count];
        }
    }

    let mut b = 0u8;
    let mut mask = 0u8;
    for _ in 0..count {
        if mask == 0 {
            if *pos >= data.len() {
                result.push(false);
                continue;
            }
            b = data[*pos];
            *pos += 1;
            mask = 0x80;
        }
        result.push((b & mask) != 0);
        mask >>= 1;
    }
    result
}

/// Skip through the streams info section (PackInfo + UnpackInfo + SubstreamsInfo).
fn sevenz_skip_streams_info(data: &[u8], pos: &mut usize) -> bool {
    // We need to skip the entire StreamsInfo block to get to FilesInfo.
    // StreamsInfo = PackInfo? UnpackInfo? SubstreamsInfo? End
    if *pos >= data.len() {
        return false;
    }
    let mut pid = data[*pos];
    *pos += 1;

    // PackInfo (id=6)
    if pid == 0x06 {
        if !sevenz_skip_pack_info(data, pos) {
            return false;
        }
        if *pos >= data.len() {
            return false;
        }
        pid = data[*pos];
        *pos += 1;
    }

    // UnpackInfo (id=7)
    if pid == 0x07 {
        if !sevenz_skip_unpack_info(data, pos) {
            return false;
        }
        if *pos >= data.len() {
            return false;
        }
        pid = data[*pos];
        *pos += 1;
    }

    // SubstreamsInfo (id=8)
    if pid == 0x08 {
        if !sevenz_skip_to_end(data, pos) {
            return false;
        }
        if *pos >= data.len() {
            return false;
        }
        pid = data[*pos];
        *pos += 1;
    }

    pid == 0x00 // End marker
}

/// Skip PackInfo section.
fn sevenz_skip_pack_info(data: &[u8], pos: &mut usize) -> bool {
    // packPos (uint64)
    if let Some((_, n)) = sevenz_read_uint64(&data[*pos..]) {
        *pos += n;
    } else {
        return false;
    }
    // numPackStreams (uint64)
    let num_streams = if let Some((v, n)) = sevenz_read_uint64(&data[*pos..]) {
        *pos += n;
        v as usize
    } else {
        return false;
    };

    // Properties until End
    loop {
        if *pos >= data.len() {
            return false;
        }
        let pid = data[*pos];
        *pos += 1;
        if pid == 0x00 {
            return true;
        }
        if pid == 0x09 {
            // Size: read numStreams uint64 values
            for _ in 0..num_streams {
                if let Some((_, n)) = sevenz_read_uint64(&data[*pos..]) {
                    *pos += n;
                } else {
                    return false;
                }
            }
        } else if pid == 0x0A {
            // CRC
            let defined = sevenz_read_booleans(data, pos, num_streams, true);
            for d in &defined {
                if *d {
                    *pos += 4; // uint32 CRC
                }
            }
        } else {
            return false;
        }
    }
}

/// Skip UnpackInfo section.
fn sevenz_skip_unpack_info(data: &[u8], pos: &mut usize) -> bool {
    if *pos >= data.len() {
        return false;
    }
    let pid = data[*pos];
    *pos += 1;
    if pid != 0x0B {
        // Folder id expected
        return false;
    }

    let num_folders = if let Some((v, n)) = sevenz_read_uint64(&data[*pos..]) {
        *pos += n;
        v as usize
    } else {
        return false;
    };

    if *pos >= data.len() {
        return false;
    }
    let external = data[*pos];
    *pos += 1;

    if external != 0 {
        return false; // External data not supported
    }

    // Read folders to count total output streams
    let mut total_out = 0usize;
    for _ in 0..num_folders {
        let out = sevenz_skip_folder(data, pos);
        if out == 0 {
            return false;
        }
        total_out += out;
    }

    // CodersUnpackSize (id=0x0C)
    if *pos >= data.len() {
        return false;
    }
    let pid2 = data[*pos];
    *pos += 1;
    if pid2 != 0x0C {
        return false;
    }
    // Read total_out uint64 values
    for _ in 0..total_out {
        if let Some((_, n)) = sevenz_read_uint64(&data[*pos..]) {
            *pos += n;
        } else {
            return false;
        }
    }

    // Optional CRC + End
    loop {
        if *pos >= data.len() {
            return false;
        }
        let pid3 = data[*pos];
        *pos += 1;
        if pid3 == 0x00 {
            return true;
        }
        if pid3 == 0x0A {
            // CRC
            let defined = sevenz_read_booleans(data, pos, num_folders, true);
            for d in &defined {
                if *d {
                    *pos += 4;
                }
            }
        } else {
            return false;
        }
    }
}

/// Skip a folder definition, return total output stream count (0 on error).
fn sevenz_skip_folder(data: &[u8], pos: &mut usize) -> usize {
    let num_coders = if let Some((v, n)) = sevenz_read_uint64(&data[*pos..]) {
        *pos += n;
        v as usize
    } else {
        return 0;
    };

    let mut total_in = 0usize;
    let mut total_out = 0usize;

    for _ in 0..num_coders {
        if *pos >= data.len() {
            return 0;
        }
        let b = data[*pos];
        *pos += 1;
        let method_size = (b & 0x0F) as usize;
        let is_complex = (b & 0x10) != 0;
        let has_attributes = (b & 0x20) != 0;

        *pos += method_size; // skip method ID

        let (num_in, num_out) = if is_complex {
            let ni = if let Some((v, n)) = sevenz_read_uint64(&data[*pos..]) {
                *pos += n;
                v as usize
            } else {
                return 0;
            };
            let no = if let Some((v, n)) = sevenz_read_uint64(&data[*pos..]) {
                *pos += n;
                v as usize
            } else {
                return 0;
            };
            (ni, no)
        } else {
            (1, 1)
        };
        total_in += num_in;
        total_out += num_out;

        if has_attributes {
            let prop_len = if let Some((v, n)) = sevenz_read_uint64(&data[*pos..]) {
                *pos += n;
                v as usize
            } else {
                return 0;
            };
            *pos += prop_len;
        }
    }

    // BindPairs
    let num_bind_pairs = total_out.saturating_sub(1);
    for _ in 0..num_bind_pairs {
        // Two uint64 per bind pair
        for _ in 0..2 {
            if let Some((_, n)) = sevenz_read_uint64(&data[*pos..]) {
                *pos += n;
            } else {
                return 0;
            }
        }
    }

    // PackedIndices
    let num_packed = total_in.saturating_sub(num_bind_pairs);
    if num_packed != 1 {
        for _ in 0..num_packed {
            if let Some((_, n)) = sevenz_read_uint64(&data[*pos..]) {
                *pos += n;
            } else {
                return 0;
            }
        }
    }

    total_out
}

/// Skip properties until End marker (0x00).
fn sevenz_skip_to_end(data: &[u8], pos: &mut usize) -> bool {
    #[allow(clippy::never_loop)]
    loop {
        if *pos >= data.len() {
            return false;
        }
        let pid = data[*pos];
        *pos += 1;
        if pid == 0x00 {
            return true;
        }
        // For SubstreamsInfo properties (13=NumUnpackStream, 9=Size, 10=CRC),
        // we don't know the exact count, so skip by reading the remaining as
        // generic property-id + data. Since these use variable-length encoding,
        // we skip by scanning for the End marker (0x00) after the last known property.
        // Simplified: skip entire remaining sub-block by scanning for 0x00 end.
        // This works because the format guarantees an End marker.

        // For known property IDs, try to skip properly
        if pid == 0x0D || pid == 0x09 || pid == 0x0A {
            // These have variable-length content we can't easily skip without
            // knowing the folder count. Use a simple heuristic: scan for End marker.
            // Back up one byte and scan forward.
            *pos -= 1;
            // Scan forward for End marker (0x00) at a plausible position
            while *pos < data.len() {
                if data[*pos] == 0x00 {
                    *pos += 1;
                    return true;
                }
                *pos += 1;
            }
            return false;
        }

        // Unknown property ID — can't skip without knowing its size
        return false;
    }
}

/// Extract header info from a normal (uncompressed) 7z header.
fn sevenz_extract_header(data: &[u8], tags: &mut Vec<Tag>) {
    let mut pos = 0;
    if pos >= data.len() {
        return;
    }

    let mut pid = data[pos];
    pos += 1;

    // MainStreamsInfo (id=4)
    if pid == 0x04 {
        if !sevenz_skip_streams_info(data, &mut pos) {
            return;
        }
        if pos >= data.len() {
            return;
        }
        pid = data[pos];
        pos += 1;
    }

    // FilesInfo (id=5)
    if pid == 0x05 {
        sevenz_read_files_info(data, &mut pos, tags);
    }
}

/// Read the FilesInfo section and extract file names and modify dates.
fn sevenz_read_files_info(data: &[u8], pos: &mut usize, tags: &mut Vec<Tag>) {
    let num_files = if let Some((v, n)) = sevenz_read_uint64(&data[*pos..]) {
        *pos += n;
        v as usize
    } else {
        return;
    };

    if num_files == 0 || num_files > 100_000 {
        return;
    }

    let mut filenames: Vec<Option<String>> = vec![None; num_files];
    let mut modify_dates: Vec<Option<i64>> = vec![None; num_files];

    loop {
        if *pos >= data.len() {
            break;
        }
        let prop = data[*pos];
        *pos += 1;

        if prop == 0x00 {
            // End
            break;
        }

        let size = if let Some((v, n)) = sevenz_read_uint64(&data[*pos..]) {
            *pos += n;
            v as usize
        } else {
            break;
        };

        if *pos + size > data.len() {
            break;
        }

        let prop_data = &data[*pos..*pos + size];

        match prop {
            0x11 => {
                // Names (property 17)
                sevenz_read_names(prop_data, num_files, &mut filenames);
            }
            0x14 => {
                // LastWriteTime (property 20)
                sevenz_read_times(prop_data, num_files, &mut modify_dates);
            }
            0x19 => {
                // Dummy (property 25) — skip
            }
            _ => {
                // Skip other properties (EmptyStream=14, EmptyFile=15, Attributes=21, etc.)
            }
        }

        *pos += size;
    }

    // Emit tags per file (using doc_num like Perl ExifTool)
    let mut doc_num = 0u32;
    for i in 0..num_files {
        let has_name = filenames[i].is_some();
        let has_date = modify_dates[i].is_some();
        if !has_name && !has_date {
            continue;
        }
        doc_num += 1;

        if let Some(ref name) = filenames[i] {
            let mut tag = mktag(
                "ZIP",
                "ArchivedFileName",
                "Archived File Name",
                Value::String(name.clone()),
            );
            tag.group.family2 = "Other".into();
            if doc_num > 1 {
                tag.name = format!("ArchivedFileName ({})", doc_num);
            }
            tags.push(tag);
        }
        if let Some(unix_secs) = modify_dates[i] {
            let (y, mo, d, h, m, s) = unix_to_datetime(unix_secs);
            let dt_str = format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}Z", y, mo, d, h, m, s);
            let mut tag = mktag("ZIP", "ModifyDate", "Modify Date", Value::String(dt_str));
            tag.group.family2 = "Time".into();
            if doc_num > 1 {
                tag.name = format!("ModifyDate ({})", doc_num);
            }
            tags.push(tag);
        }
    }
}

/// Read UTF-16LE file names from the Names property.
fn sevenz_read_names(data: &[u8], num_files: usize, filenames: &mut [Option<String>]) {
    if data.is_empty() {
        return;
    }
    // First byte: external flag
    let external = data[0];
    if external != 0 {
        return;
    }

    let mut pos = 1;
    for item in filenames.iter_mut().take(num_files) {
        let mut utf16_units = Vec::new();
        loop {
            if pos + 2 > data.len() {
                return;
            }
            let ch = u16::from_le_bytes([data[pos], data[pos + 1]]);
            pos += 2;
            if ch == 0 {
                break;
            }
            utf16_units.push(ch);
        }
        if !utf16_units.is_empty() {
            *item = Some(String::from_utf16_lossy(&utf16_units));
        }
    }
}

/// Read Windows FILETIME timestamps from a Times property.
fn sevenz_read_times(data: &[u8], num_files: usize, times: &mut [Option<i64>]) {
    let mut pos = 0;
    let defined = sevenz_read_booleans(data, &mut pos, num_files, true);

    if pos >= data.len() {
        return;
    }
    let external = data[pos];
    pos += 1;
    if external != 0 {
        return;
    }

    for i in 0..num_files {
        if i < defined.len() && defined[i] {
            if pos + 8 > data.len() {
                return;
            }
            let filetime = u64::from_le_bytes([
                data[pos],
                data[pos + 1],
                data[pos + 2],
                data[pos + 3],
                data[pos + 4],
                data[pos + 5],
                data[pos + 6],
                data[pos + 7],
            ]);
            pos += 8;
            // Convert Windows FILETIME to Unix timestamp
            // FILETIME is 100-nanosecond intervals since 1601-01-01
            // Integer division for exact seconds
            let unix_secs = (filetime / 10_000_000).wrapping_sub(11_644_473_600) as i64;
            times[i] = Some(unix_secs);
        }
    }
}
