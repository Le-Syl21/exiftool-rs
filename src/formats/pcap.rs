//! PCAP and PCAPNG packet capture format reader.

use super::misc::mktag;
use crate::error::Result;
use crate::tag::Tag;
use crate::value::Value;

pub fn read_pcap(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 24 {
        return Err(crate::error::Error::InvalidData("not a PCAP file".into()));
    }

    let is_le = data[0] == 0xD4 && data[1] == 0xC3;
    let r16 = |d: &[u8], o: usize| -> u16 {
        if o + 2 > d.len() {
            return 0;
        }
        if is_le {
            u16::from_le_bytes([d[o], d[o + 1]])
        } else {
            u16::from_be_bytes([d[o], d[o + 1]])
        }
    };
    let r32 = |d: &[u8], o: usize| -> u32 {
        if o + 4 > d.len() {
            return 0;
        }
        if is_le {
            u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
        } else {
            u32::from_be_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
        }
    };

    let maj = r16(data, 4);
    let min = r16(data, 6);
    let link_type = r32(data, 20);

    let mut tags = Vec::new();
    let bo_str = if is_le {
        "Little-endian (Intel, II)"
    } else {
        "Big-endian (Motorola, MM)"
    };
    tags.push(mktag(
        "PCAP",
        "ByteOrder",
        "Byte Order",
        Value::String(bo_str.into()),
    ));
    tags.push(mktag(
        "PCAP",
        "PCAPVersion",
        "PCAP Version",
        Value::String(format!("PCAP {}.{}", maj, min)),
    ));
    tags.push(mktag(
        "PCAP",
        "LinkType",
        "Link Type",
        Value::String(pcap_link_type_name(link_type)),
    ));

    Ok(tags)
}

// ============================================================================
// PCAPNG (pcap next generation) format reader
// ============================================================================

pub fn read_pcapng(data: &[u8]) -> Result<Vec<Tag>> {
    // Section Header Block: 0x0A0D0D0A
    if data.len() < 28 || data[0] != 0x0A || data[1] != 0x0D || data[2] != 0x0D || data[3] != 0x0A {
        return Err(crate::error::Error::InvalidData("not a PCAPNG file".into()));
    }

    // Block length at offset 4 (4 bytes)
    // Byte order magic at offset 8: 0x1A2B3C4D (LE) or 0x4D3C2B1A (BE)
    let bo_magic_le = data.len() >= 12
        && data[8] == 0x4D
        && data[9] == 0x3C
        && data[10] == 0x2B
        && data[11] == 0x1A;
    let is_le = bo_magic_le;

    let r16 = |d: &[u8], o: usize| -> u16 {
        if o + 2 > d.len() {
            return 0;
        }
        if is_le {
            u16::from_le_bytes([d[o], d[o + 1]])
        } else {
            u16::from_be_bytes([d[o], d[o + 1]])
        }
    };
    let r32 = |d: &[u8], o: usize| -> u32 {
        if o + 4 > d.len() {
            return 0;
        }
        if is_le {
            u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
        } else {
            u32::from_be_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
        }
    };

    let maj = r16(data, 12);
    let min = r16(data, 14);
    let blk_len = r32(data, 4) as usize;

    let mut tags = Vec::new();
    let bo_str = if is_le {
        "Little-endian (Intel, II)"
    } else {
        "Big-endian (Motorola, MM)"
    };
    tags.push(mktag(
        "PCAP",
        "ByteOrder",
        "Byte Order",
        Value::String(bo_str.into()),
    ));
    tags.push(mktag(
        "PCAP",
        "PCAPVersion",
        "PCAP Version",
        Value::String(format!("PCAPNG {}.{}", maj, min)),
    ));

    // SHB structure: block_type(4) + block_len(4) + bo_magic(4) + major(2) + minor(2) + section_len(8)
    // Options start at offset 24 (after the 8-byte section_length field)
    let opt_start = 24usize;
    let opt_end = if blk_len > 4 && blk_len <= data.len() {
        blk_len - 4
    } else {
        data.len()
    };
    parse_pcapng_options(data, opt_start, opt_end, is_le, &mut tags, "shb");

    // Parse Interface Description Block (IDB) right after the SHB
    let idb_start = if blk_len < data.len() {
        blk_len
    } else {
        return Ok(tags);
    };
    if idb_start + 20 <= data.len() {
        let idb_type = r32(data, idb_start);
        if idb_type == 1 {
            // IDB: block type(4) + block_len(4) + link_type(2) + reserved(2) + snap_len(4) = 16 bytes
            let idb_len = r32(data, idb_start + 4) as usize;
            let link_type = r32(data, idb_start + 8) & 0xFFFF;
            let link_name = pcap_link_type_name(link_type);
            tags.push(mktag(
                "PCAP",
                "LinkType",
                "Link Type",
                Value::String(link_name),
            ));

            // Parse IDB options (starting at offset idb_start + 16)
            let idb_opt_start = idb_start + 16;
            let idb_opt_end = if idb_start + idb_len > 4 && idb_start + idb_len <= data.len() {
                idb_start + idb_len - 4
            } else {
                data.len()
            };
            parse_pcapng_options(data, idb_opt_start, idb_opt_end, is_le, &mut tags, "idb");

            // Parse EPB/SPB blocks to find TimeStamp
            let epb_start = idb_start + idb_len;
            parse_pcapng_blocks(data, epb_start, is_le, &mut tags);
        }
    }

    Ok(tags)
}

fn parse_pcapng_options(
    data: &[u8],
    start: usize,
    end: usize,
    is_le: bool,
    tags: &mut Vec<Tag>,
    ctx: &str,
) {
    let r16 = |d: &[u8], o: usize| -> u16 {
        if o + 2 > d.len() {
            return 0;
        }
        if is_le {
            u16::from_le_bytes([d[o], d[o + 1]])
        } else {
            u16::from_be_bytes([d[o], d[o + 1]])
        }
    };

    let mut pos = start;
    while pos + 4 <= end.min(data.len()) {
        let opt_code = r16(data, pos);
        let opt_len = r16(data, pos + 2) as usize;
        pos += 4;
        if opt_code == 0 {
            break;
        } // opt_endofopt
        let padded_len = (opt_len + 3) & !3;
        if pos + opt_len > data.len() {
            break;
        }

        let opt_data = &data[pos..pos + opt_len];

        match (ctx, opt_code) {
            ("shb", 2) => {
                // shb_hardware
                let s = crate::encoding::decode_utf8_or_latin1(opt_data).to_string();
                tags.push(mktag("PCAP", "Hardware", "Hardware", Value::String(s)));
            }
            ("shb", 3) => {
                // shb_os
                let s = crate::encoding::decode_utf8_or_latin1(opt_data).to_string();
                tags.push(mktag(
                    "PCAP",
                    "OperatingSystem",
                    "Operating System",
                    Value::String(s),
                ));
            }
            ("shb", 4) => {
                // shb_userappl
                let s = crate::encoding::decode_utf8_or_latin1(opt_data).to_string();
                tags.push(mktag(
                    "PCAP",
                    "UserApplication",
                    "User Application",
                    Value::String(s),
                ));
            }
            ("idb", 2) => {
                // if_name
                let s = crate::encoding::decode_utf8_or_latin1(opt_data).to_string();
                tags.push(mktag("PCAP", "DeviceName", "Device Name", Value::String(s)));
            }
            ("idb", 9) => {
                // if_tsresol: timestamp resolution
                if opt_len >= 1 {
                    let tsresol = opt_data[0];
                    let resolution = if tsresol & 0x80 != 0 {
                        // Power of 2
                        let exp = tsresol & 0x7F;
                        format!("2^-{}", exp)
                    } else {
                        // Power of 10
                        let exp = tsresol & 0x7F;
                        format!("1e-{:02}", exp)
                    };
                    tags.push(mktag(
                        "PCAP",
                        "TimeStampResolution",
                        "Time Stamp Resolution",
                        Value::String(resolution),
                    ));
                }
            }
            ("idb", 12) => {
                // if_os
                let s = crate::encoding::decode_utf8_or_latin1(opt_data).to_string();
                if !tags.iter().any(|t| t.name == "OperatingSystem") {
                    tags.push(mktag(
                        "PCAP",
                        "OperatingSystem",
                        "Operating System",
                        Value::String(s),
                    ));
                }
            }
            _ => {}
        }

        pos += padded_len;
    }
}

fn parse_pcapng_blocks(data: &[u8], start: usize, is_le: bool, tags: &mut Vec<Tag>) {
    let r32 = |d: &[u8], o: usize| -> u32 {
        if o + 4 > d.len() {
            return 0;
        }
        if is_le {
            u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
        } else {
            u32::from_be_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
        }
    };
    let _r64 = |d: &[u8], o: usize| -> u64 {
        if o + 8 > d.len() {
            return 0;
        }
        if is_le {
            u64::from_le_bytes([
                d[o],
                d[o + 1],
                d[o + 2],
                d[o + 3],
                d[o + 4],
                d[o + 5],
                d[o + 6],
                d[o + 7],
            ])
        } else {
            u64::from_be_bytes([
                d[o],
                d[o + 1],
                d[o + 2],
                d[o + 3],
                d[o + 4],
                d[o + 5],
                d[o + 6],
                d[o + 7],
            ])
        }
    };

    let mut pos = start;
    while pos + 8 <= data.len() {
        let block_type = r32(data, pos);
        let block_len = r32(data, pos + 4) as usize;
        if block_len < 12 || pos + block_len > data.len() {
            break;
        }

        // EPB (Enhanced Packet Block) = type 6
        if block_type == 6 && block_len >= 28 {
            let ts_hi = r32(data, pos + 12) as u64;
            let ts_lo = r32(data, pos + 16) as u64;
            let ts_raw = (ts_hi << 32) | ts_lo;
            // Default resolution is 1e-6 (microseconds)
            let ts_secs = ts_raw / 1_000_000;
            let ts_usecs = ts_raw % 1_000_000;
            // Format as ExifTool does: YYYY:MM:DD HH:MM:SS.ssssss+ZZ:ZZ
            if let Some(dt) = format_unix_timestamp(ts_secs as i64, ts_usecs) {
                tags.push(mktag("PCAP", "TimeStamp", "Time Stamp", Value::String(dt)));
            }
            break; // Only need first packet timestamp
        }

        pos += block_len;
    }
}

fn format_unix_timestamp(secs: i64, usecs: u64) -> Option<String> {
    // Simple Unix timestamp to datetime conversion
    // This is a basic implementation - timezone from local offset
    // For now, use UTC + known local offset from Perl output
    // Perl shows: 2020:10:13 16:12:07.025764+02:00
    // We'll use UTC for simplicity but format it correctly

    // Get local timezone offset using system time
    let tz_offset_secs = get_local_tz_offset();

    let adjusted = secs + tz_offset_secs as i64;

    // Compute Y/M/D H:M:S from Unix timestamp
    let (y, mo, d, h, mi, s) = unix_to_datetime(adjusted);
    let tz_hours = tz_offset_secs / 3600;
    let tz_mins = (tz_offset_secs.abs() % 3600) / 60;
    let tz_sign = if tz_offset_secs >= 0 { '+' } else { '-' };

    Some(format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}.{:06}{}{:02}:{:02}",
        y,
        mo,
        d,
        h,
        mi,
        s,
        usecs,
        tz_sign,
        tz_hours.abs(),
        tz_mins
    ))
}

fn get_local_tz_offset() -> i32 {
    // Try to get timezone offset from system
    // This uses a simple method: compare local time to UTC

    // For now return 0 (UTC) - the test data shows +02:00 but we can't easily detect this
    // without platform-specific code
    0
}

pub(crate) fn unix_to_datetime(secs: i64) -> (i32, u32, u32, u32, u32, u32) {
    // Basic implementation of Unix timestamp to calendar date
    const SECS_PER_DAY: i64 = 86400;
    const DAYS_PER_400Y: i64 = 146097;

    let (days, rem) = if secs >= 0 {
        (secs / SECS_PER_DAY, secs % SECS_PER_DAY)
    } else {
        let d = (secs + 1) / SECS_PER_DAY - 1;
        let r = secs - d * SECS_PER_DAY;
        (d, r)
    };

    let h = (rem / 3600) as u32;
    let m = ((rem % 3600) / 60) as u32;
    let s = (rem % 60) as u32;

    // Days since 1970-01-01
    // Adjust to days since 2000-03-01 for easier calculation
    let z = days + 719468; // days from 0000-03-01
    let era = if z >= 0 { z } else { z - DAYS_PER_400Y + 1 } / DAYS_PER_400Y;
    let doe = z - era * DAYS_PER_400Y;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };

    (y as i32, mo as u32, d as u32, h, m, s)
}

fn pcap_link_type_name(link_type: u32) -> String {
    match link_type {
        0 => "BSD Loopback".into(),
        1 => "IEEE 802.3 Ethernet".into(),
        9 => "PPP".into(),
        105 => "IEEE 802.11".into(),
        108 => "OpenBSD Loopback".into(),
        113 => "Linux SLL".into(),
        127 => "IEEE 802.11 Radiotap".into(),
        _ => format!("{}", link_type),
    }
}
