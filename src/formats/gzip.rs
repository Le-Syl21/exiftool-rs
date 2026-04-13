//! GZIP format reader.

use super::misc::mktag;
use crate::error::{Error, Result};
use crate::tag::Tag;
use crate::value::Value;

pub fn read_gzip(data: &[u8]) -> Result<Vec<Tag>> {
    // RFC 1952: magic=1F 8B, method=08 (deflate)
    if data.len() < 10 || data[0] != 0x1F || data[1] != 0x8B || data[2] != 0x08 {
        return Err(Error::InvalidData("not a GZIP file".into()));
    }

    let mut tags = Vec::new();
    let method = data[2];
    let flags = data[3];
    let xflags = data[8];
    let os_byte = data[9];

    // Compression (byte 2)
    let compress_str = if method == 8 { "Deflated" } else { "Unknown" };
    tags.push(mktag(
        "GZIP",
        "Compression",
        "Compression",
        Value::String(compress_str.into()),
    ));

    // Flags (byte 3) — bitmask
    let flag_names = [
        (0, "Text"),
        (1, "CRC16"),
        (2, "ExtraFields"),
        (3, "FileName"),
        (4, "Comment"),
    ];
    let mut flag_parts: Vec<&str> = Vec::new();
    for (bit, name) in &flag_names {
        if flags & (1 << bit) != 0 {
            flag_parts.push(name);
        }
    }
    let flags_str = if flag_parts.is_empty() {
        "(none)".to_string()
    } else {
        flag_parts.join(", ")
    };
    tags.push(mktag("GZIP", "Flags", "Flags", Value::String(flags_str)));

    // ModifyDate (bytes 4-7, Unix timestamp, local time)
    let mtime = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    if mtime > 0 {
        let dt = gzip_unix_to_datetime(mtime as i64);
        tags.push(mktag(
            "GZIP",
            "ModifyDate",
            "Modify Date",
            Value::String(dt),
        ));
    }

    // ExtraFlags (byte 8)
    let extra_flags_str = match xflags {
        0 => "(none)".to_string(),
        2 => "Maximum Compression".to_string(),
        4 => "Fastest Algorithm".to_string(),
        _ => format!("{}", xflags),
    };
    tags.push(mktag(
        "GZIP",
        "ExtraFlags",
        "Extra Flags",
        Value::String(extra_flags_str),
    ));

    // OperatingSystem (byte 9)
    let os_str = match os_byte {
        0 => "FAT filesystem (MS-DOS, OS/2, NT/Win32)",
        1 => "Amiga",
        2 => "VMS (or OpenVMS)",
        3 => "Unix",
        4 => "VM/CMS",
        5 => "Atari TOS",
        6 => "HPFS filesystem (OS/2, NT)",
        7 => "Macintosh",
        8 => "Z-System",
        9 => "CP/M",
        10 => "TOPS-20",
        11 => "NTFS filesystem (NT)",
        12 => "QDOS",
        13 => "Acorn RISCOS",
        255 => "unknown",
        _ => "Other",
    };
    tags.push(mktag(
        "GZIP",
        "OperatingSystem",
        "Operating System",
        Value::String(os_str.into()),
    ));

    // Extract file name and comment if flag bits set
    let mut pos = 10usize;
    if flags & 0x18 != 0 {
        // Skip FEXTRA (bit 2) if present
        if flags & 0x04 != 0 && pos + 2 <= data.len() {
            let xlen = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
            pos += 2 + xlen;
        }

        // ArchivedFileName (bit 3)
        if flags & 0x08 != 0 && pos < data.len() {
            let name_end = data[pos..]
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(data.len() - pos);
            let filename =
                crate::encoding::decode_utf8_or_latin1(&data[pos..pos + name_end]).to_string();
            if !filename.is_empty() {
                tags.push(mktag(
                    "GZIP",
                    "ArchivedFileName",
                    "Archived File Name",
                    Value::String(filename),
                ));
            }
            pos += name_end + 1;
        }

        // Comment (bit 4)
        if flags & 0x10 != 0 && pos < data.len() {
            let comment_end = data[pos..]
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(data.len() - pos);
            let comment =
                crate::encoding::decode_utf8_or_latin1(&data[pos..pos + comment_end]).to_string();
            if !comment.is_empty() {
                tags.push(mktag("GZIP", "Comment", "Comment", Value::String(comment)));
            }
        }
    } else {
        // No FEXTRA flag, but FNAME might still be set
        if flags & 0x04 != 0 && pos + 2 <= data.len() {
            let xlen = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
            pos += 2 + xlen;
        }
        if flags & 0x08 != 0 && pos < data.len() {
            let name_end = data[pos..]
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(data.len() - pos);
            let filename =
                crate::encoding::decode_utf8_or_latin1(&data[pos..pos + name_end]).to_string();
            if !filename.is_empty() {
                tags.push(mktag(
                    "GZIP",
                    "ArchivedFileName",
                    "Archived File Name",
                    Value::String(filename),
                ));
            }
        }
    }

    Ok(tags)
}

/// Convert Unix timestamp to "YYYY:MM:DD HH:MM:SS+HH:00" (local time).
/// Mirrors Perl's ConvertUnixTime($val, 1).
pub(crate) fn gzip_unix_to_datetime(secs: i64) -> String {
    // Get timezone offset from system (DST-aware for the specific timestamp)
    let tz_offset = get_local_tz_offset_for_timestamp(secs);
    let local_secs = secs + tz_offset;
    let days = local_secs / 86400;
    let time = local_secs % 86400;
    let (time, days) = if time < 0 {
        (time + 86400, days - 1)
    } else {
        (time, days)
    };
    let h = time / 3600;
    let m = (time % 3600) / 60;
    let s = time % 60;
    let mut y = 1970i32;
    let mut rem = days;
    loop {
        let dy: i64 = if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
            366
        } else {
            365
        };
        if rem < dy {
            break;
        }
        rem -= dy;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let months: [i64; 12] = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut mo = 1;
    for &dm in &months {
        if rem < dm {
            break;
        }
        rem -= dm;
        mo += 1;
    }
    let tz_h = tz_offset / 3600;
    let tz_m = (tz_offset.abs() % 3600) / 60;
    let tz_sign = if tz_offset >= 0 { "+" } else { "-" };
    format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}{}{:02}:{:02}",
        y,
        mo,
        rem + 1,
        h,
        m,
        s,
        tz_sign,
        tz_h.abs(),
        tz_m
    )
}

/// Get local timezone offset in seconds for a specific Unix timestamp (DST-aware).
/// Uses libc's localtime_r via raw syscall to account for DST.
fn get_local_tz_offset_for_timestamp(ts: i64) -> i64 {
    #[cfg(target_os = "linux")]
    {
        // Use libc localtime via /proc/self/fd - actually let's use libc directly
        // since the binary is on Linux we can use the C library through FFI
        use std::mem;
        extern "C" {
            fn localtime_r(timep: *const LibcTimeT, result: *mut TmStruct) -> *mut TmStruct;
        }
        type LibcTimeT = i64;
        #[repr(C)]
        struct TmStruct {
            tm_sec: i32,
            tm_min: i32,
            tm_hour: i32,
            tm_mday: i32,
            tm_mon: i32,
            tm_year: i32,
            tm_wday: i32,
            tm_yday: i32,
            tm_isdst: i32,
            tm_gmtoff: i64,
            tm_zone: *const i8,
        }
        unsafe {
            let mut tm: TmStruct = mem::zeroed();
            let t = ts;
            if !localtime_r(&t, &mut tm).is_null() {
                return tm.tm_gmtoff;
            }
        }
    }
    // Fallback: try to detect from /etc/timezone
    if let Ok(tz) = std::fs::read_to_string("/etc/timezone") {
        let tz = tz.trim();
        if tz == "UTC" || tz == "UTC0" {
            return 0;
        }
    }
    if let Ok(link) = std::fs::read_link("/etc/localtime") {
        let path = link.to_string_lossy();
        if path.contains("UTC") || path.ends_with("/UTC") {
            return 0;
        }
        if path.contains("Europe/") || path.contains("/CET") {
            return 3600;
        }
        if path.contains("America/New_York") {
            return -5 * 3600;
        }
        if path.contains("America/Los_Angeles") {
            return -8 * 3600;
        }
        if path.contains("America/Chicago") {
            return -6 * 3600;
        }
        if path.contains("Asia/Tokyo") {
            return 9 * 3600;
        }
    }
    0
}
