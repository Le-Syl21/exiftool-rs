//! Windows Shell Link (.lnk) and URL shortcut (.url) file reader.
//! Mirrors ExifTool's LNK.pm ProcessLNK() and ProcessINI().

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

fn mk_str(name: &str, val: &str) -> Tag {
    mk(name, Value::String(val.to_string()))
}

fn mk_time(name: &str, val: &str) -> Tag {
    let mut t = mk_str(name, val);
    t.group.family2 = "Time".into();
    t
}

fn get_url_tz_offset() -> i64 {
    if let Ok(tz) = std::env::var("TZ") {
        let tz = tz.trim();
        if let Some(sign_pos) = tz.rfind(['+', '-']) {
            let sign: i64 = if &tz[sign_pos..sign_pos + 1] == "+" {
                1
            } else {
                -1
            };
            if let Ok(h) = tz[sign_pos + 1..].parse::<i64>() {
                return -sign * h * 3600;
            }
        }
    }
    0
}

/// Convert Windows FILETIME (100-ns intervals since 1601-01-01) to ExifTool datetime
fn filetime_to_datetime(lo: u32, hi: u32, tz_offset_hours: i64) -> Option<String> {
    let filetime = (hi as u64) * 4294967296u64 + (lo as u64);
    if filetime == 0 {
        return None;
    }
    let unix_secs = (filetime as i64) / 10_000_000 - 11_644_473_600;
    let secs = unix_secs + tz_offset_hours * 3600;
    let (year, month, day, h, m, s) = unix_to_components(secs);
    let sign = if tz_offset_hours >= 0 { '+' } else { '-' };
    let abs_h = tz_offset_hours.unsigned_abs();
    Some(format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}{}{:02}:00",
        year, month, day, h, m, s, sign, abs_h
    ))
}

/// Convert a 64-bit FILETIME value to ExifTool datetime (UTC)
fn filetime64_to_datetime(val: u64) -> Option<String> {
    if val == 0 {
        return None;
    }
    let unix_secs = (val as i64) / 10_000_000 - 11_644_473_600;
    let (year, month, day, h, m, s) = unix_to_components(unix_secs);
    Some(format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
        year, month, day, h, m, s
    ))
}

fn unix_to_components(secs: i64) -> (i32, u32, u32, u32, u32, u32) {
    let s = (secs % 60 + 60) as u32 % 60;
    let secs2 = if secs % 60 < 0 {
        secs - secs % 60 - 60
    } else {
        secs - secs % 60
    };
    let mins = secs2 / 60;
    let m = (mins % 60 + 60) as u32 % 60;
    let mins2 = if mins % 60 < 0 {
        mins - mins % 60 - 60
    } else {
        mins - mins % 60
    };
    let hours = mins2 / 60;
    let h = (hours % 24 + 24) as u32 % 24;
    let hours2 = if hours % 24 < 0 {
        hours - hours % 24 - 24
    } else {
        hours - hours % 24
    };
    let days = hours2 / 24;
    let (year, month, day) = days_to_date(days);
    (year, month, day, h, m, s)
}

fn days_to_date(days: i64) -> (i32, u32, u32) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

/// Convert LNK DOSTime 32-bit value to ExifTool datetime string.
/// ExifTool's LNK.pm DOSTime() treats the whole 32-bit value as:
///   bits 31-27 = hour, bits 26-21 = minute, bits 20-15 = second/2,
///   bits 14-9 = year-1980, bits 8-5 = month, bits 4-0 = day
fn dos_time(val: u32) -> Option<String> {
    if val == 0 {
        return None;
    }
    let year = ((val >> 9) & 0x7f) as i32 + 1980;
    let month = (val >> 5) & 0x0f;
    let day = val & 0x1f;
    let hour = (val >> 27) & 0x1f;
    let min = (val >> 21) & 0x3f;
    let sec = ((val >> 15) & 0x3e) as u32; // 2-second resolution, bits 20-15
    if month == 0 || day == 0 {
        return None;
    }
    Some(format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
        year, month, day, hour, min, sec
    ))
}

fn read_u16_le(data: &[u8], off: usize) -> Option<u16> {
    if off + 2 <= data.len() {
        Some(u16::from_le_bytes([data[off], data[off + 1]]))
    } else {
        None
    }
}

fn read_u32_le(data: &[u8], off: usize) -> Option<u32> {
    if off + 4 <= data.len() {
        Some(u32::from_le_bytes([
            data[off],
            data[off + 1],
            data[off + 2],
            data[off + 3],
        ]))
    } else {
        None
    }
}

fn read_u64_le(data: &[u8], off: usize) -> Option<u64> {
    if off + 8 <= data.len() {
        Some(u64::from_le_bytes([
            data[off],
            data[off + 1],
            data[off + 2],
            data[off + 3],
            data[off + 4],
            data[off + 5],
            data[off + 6],
            data[off + 7],
        ]))
    } else {
        None
    }
}

fn read_cstring(data: &[u8], off: usize) -> Option<String> {
    if off >= data.len() {
        return None;
    }
    let end = data[off..]
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(data.len() - off);
    Some(crate::encoding::decode_utf8_or_latin1(&data[off..off + end]).to_string())
}

fn read_utf16le_string(data: &[u8], off: usize) -> Option<String> {
    if off + 2 > data.len() {
        return None;
    }
    let mut chars = Vec::new();
    let mut i = off;
    while i + 1 < data.len() {
        let ch = u16::from_le_bytes([data[i], data[i + 1]]);
        if ch == 0 {
            break;
        }
        chars.push(ch);
        i += 2;
    }
    Some(String::from_utf16_lossy(&chars))
}

/// Read a UTF-16LE string of known byte length (no null terminator search)
fn read_utf16le_fixed(data: &[u8], off: usize, byte_len: usize) -> Option<String> {
    if off + byte_len > data.len() {
        return None;
    }
    let chars: Vec<u16> = data[off..off + byte_len]
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    let s = String::from_utf16_lossy(&chars);
    // Trim trailing nulls
    Some(s.trim_end_matches('\0').to_string())
}

fn flags_to_string(val: u32, bits: &[(u32, &str)]) -> String {
    let parts: Vec<&str> = bits
        .iter()
        .filter(|(mask, _)| val & (1 << mask) != 0)
        .map(|(_, name)| *name)
        .collect();
    if parts.is_empty() {
        format!("0x{:08x}", val)
    } else {
        parts.join(", ")
    }
}

/// Convert 16 bytes of binary GUID data to the standard string format
/// Mirrors ExifTool's ASF::GetGUID: VvvNN -> NnnNN repack + hex + dashes + uppercase
fn format_guid(data: &[u8]) -> Option<String> {
    if data.len() < 16 {
        return None;
    }
    let d1 = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    let d2 = u16::from_le_bytes([data[4], data[5]]);
    let d3 = u16::from_le_bytes([data[6], data[7]]);
    // d4 and d5 are big-endian (no swap needed)
    let d4 = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    let d5 = u32::from_be_bytes([data[12], data[13], data[14], data[15]]);
    Some(
        format!("{:08X}-{:04X}-{:04X}-{:08X}{:08X}", d1, d2, d3, d4, d5).replacen(
            &format!("{:08X}{:08X}", d4, d5),
            &{
                let s = format!("{:08X}{:08X}", d4, d5);
                format!("{}-{}", &s[0..4], &s[4..])
            },
            1,
        ),
    )
}

/// Lookup a GUID string in the known GUID table
fn guid_lookup(guid: &str) -> Option<&'static str> {
    // Try exact match first (uppercase), then case-insensitive
    let upper = guid.to_uppercase();
    GUID_TABLE
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(&upper))
        .map(|(_, v)| *v)
}

/// Format a GUID and optionally resolve its name
fn format_guid_with_name(data: &[u8]) -> Option<String> {
    let guid = format_guid(data)?;
    if let Some(name) = guid_lookup(&guid) {
        Some(format!("{} ({})", guid, name))
    } else {
        Some(guid)
    }
}

// ============================================================
// GUID lookup table (matches ExifTool's %guidLookup)
// ============================================================
static GUID_TABLE: &[(&str, &str)] = &[
    // ref https://learn.microsoft.com/en-us/windows/win32/shell/knownfolderid
    (
        "008CA0B1-55B4-4C56-B8A8-4DE4B299D3BE",
        "Account Pictures (per-user)",
    ),
    ("DE61D971-5EBC-4F02-A3A9-6C82895E5C04", "Add New Programs"),
    (
        "724EF170-A42D-4FEF-9F26-B60E846FBA4F",
        "Administrative Tools (per-user)",
    ),
    (
        "D0384E7D-BAC3-4797-8F14-CBA229B392B5",
        "Administrative Tools",
    ),
    ("1E87508D-89C2-42F0-8A7E-645A0F50CA58", "Applications"),
    (
        "A3918781-E5F2-4890-B3D9-A7E54332328C",
        "Application Shortcuts (per-user)",
    ),
    ("A305CE99-F527-492B-8B1A-7E76FA98D6E4", "Installed Updates"),
    (
        "9E52AB10-F80D-49DF-ACB8-4330F5687855",
        "Assemblies (per-user)",
    ),
    ("1FE35CDE-B250-4B6A-A1C4-8164E91E8EC7", "Assemblies"),
    (
        "56784854-C6CB-462B-8169-88E350ACB882",
        "Contacts (per-user)",
    ),
    ("B4BFCC3A-DB2C-424C-B029-7FE99A87C641", "Desktop (per-user)"),
    ("C4AA340D-F20F-4863-AFEF-F87EF2E6BA25", "Desktop"),
    (
        "5CE4A5E9-E4EB-479D-B89F-130C02886155",
        "Device Metadata Store",
    ),
    ("7B0DB17D-9CD2-4A93-9733-46CC89022E7C", "Documents Library"),
    (
        "FDD39AD0-238F-46AF-ADB4-6C85480369C7",
        "Documents (per-user)",
    ),
    ("ED4824AF-DCE4-45A8-81E2-FC7965083634", "Documents"),
    (
        "3B193882-D3AD-4EAB-965A-69829D1FB59F",
        "Downloads (per-user)",
    ),
    (
        "374DE290-123F-4565-9164-39C4925E467B",
        "Downloads (per-user) [2]",
    ),
    (
        "7D1D3A04-DEBB-4115-95CF-2F29DA2920DA",
        "Saved Searches (per-user)",
    ),
    ("1AC14E77-02E7-4E5D-B744-2EB1AE5198B7", "System"),
    ("D65231B0-B2F1-4857-A4CE-A8E7C6EA7D27", "System32\\x86"),
    ("0762D272-C50A-4BB0-A382-697DCD729B80", "User Profiles"),
    (
        "5CD7AEE2-2219-4A67-B85D-6C9CE15660CB",
        "Programs (per-user)",
    ),
    ("BCBD3057-CA5C-4622-B42D-BC56DB0AE516", "Programs"),
    (
        "4C5C32FF-BB9D-43B0-B5B4-2D72E54EAAA4",
        "Saved Games (per-user)",
    ),
    (
        "7C5A40EF-A0FB-4BFC-874A-C0F2E0B9FA8E",
        "Programs (per-user Start Menu)",
    ),
    (
        "A4115719-D62E-491D-AA7C-E74B8BE3B067",
        "Programs (Start Menu)",
    ),
    (
        "625B53C3-AB48-4EC1-BA1F-A1EF4146FC19",
        "Start Menu (per-user)",
    ),
    ("A77F5D77-2E2B-44C3-A6A2-ABA601054A51", "Start Menu"),
    ("B97D20BB-F46A-4C97-BA10-5E3608430854", "Startup (per-user)"),
    ("82A5EA35-D9CD-47C5-9629-E15D2F714E6E", "Startup"),
    ("43668BF8-C14E-49B2-97C9-747784D784B7", "Sync Manager"),
    ("289A9A43-BE44-4057-A41B-587A76D7E7F9", "Sync Results"),
    ("0F214138-B1D3-4A90-BBA9-27CBC0C5389A", "Sync Setup"),
    ("1F3427C8-5C10-4210-AA03-2EE45287D668", "User Pinned"),
    ("F3CE0F7C-4901-4ACC-8648-D5D44B04EF8F", "Users Files"),
    (
        "A52BBA46-E9E1-435F-B3D9-28DAA648C0F6",
        "OneDrive (per-user)",
    ),
    ("DFDF76A2-C82A-4D63-906A-5644AC457385", "Public (per-user)"),
    ("C4900540-2379-4C75-844B-64E6FAF8716B", "Public"),
    ("ED4824AF-DCE4-45A8-81E2-FC7965083634", "Public Documents"),
    ("3D644C9B-1FB8-4F30-9B45-F670235F79C0", "Public Downloads"),
    (
        "DEBF2536-E1A8-4C59-B6A2-414586476AEA",
        "Public Game Explorer",
    ),
    ("48DAF80B-E6CF-4F4E-B800-0E69D84EE384", "Public Libraries"),
    ("3214FAB5-9757-4298-BB61-92A9DEAA44FF", "Public Music"),
    ("B6EBFB86-6907-413C-9AF7-4FC2ABF07CC5", "Public Pictures"),
    ("2400183A-6185-49FB-A2D8-4A392A602BA3", "Public Videos"),
    ("52A4F021-7B75-48A9-9F6B-4B87A210BC8F", "Quick Launch"),
    (
        "AE50C081-EBD2-438A-8655-8A092E34987A",
        "Recent Items (per-user)",
    ),
    ("B7534046-3ECB-4C18-BE4E-64CD4CB7D6AC", "Recycle Bin"),
    (
        "8AD10C31-2ADB-4296-A8F7-E4701232C972",
        "Resources (per-user)",
    ),
    (
        "C870044B-F49E-4126-A9C3-B52A1FF411E8",
        "Ringtones (per-user)",
    ),
    ("E555AB60-153B-4D17-9F04-A5FE99FC15EC", "Ringtones"),
    ("3EB685DB-65F9-4CF6-A03A-E3EF65729F3D", "Roaming (per-user)"),
    (
        "AAA8D5A5-F1D6-4259-BAA8-78E7EF60835E",
        "Roaming Tiles (per-user)",
    ),
    (
        "B250C668-F532-474D-A4B0-6A08CB4F0F74",
        "Sample Music (obsolete)",
    ),
    (
        "C4900540-2379-4C75-844B-64E6FAF8716B",
        "Sample Pictures (obsolete)",
    ),
    (
        "15CA69B3-30EE-49C1-ACE1-6B5EC372AFB5",
        "Sample Playlists (obsolete)",
    ),
    (
        "859EAD94-2E85-48AD-A71A-0969CB56A6CD",
        "Sample Videos (obsolete)",
    ),
    (
        "4C5C32FF-BB9D-43B0-B5B4-2D72E54EAAA4",
        "Saved Games (per-user)",
    ),
    ("EE32E446-31CA-4ABA-814F-A5EBD2FD6D5E", "Offline Files"),
    (
        "98EC0E18-2098-4D44-8644-66979315A281",
        "Microsoft Office Outlook (per-user)",
    ),
    (
        "190337D1-B8CA-4121-A639-6D472D16972A",
        "Searches (per-user)",
    ),
    ("8983036C-27C0-404B-8F08-102D10DCFD74", "SendTo (per-user)"),
    (
        "A3918781-E5F2-4890-B3D9-A7E54332328C",
        "Application Shortcuts (per-user)",
    ),
    (
        "AB5FB87B-7CE2-4F83-915D-550846C9537B",
        "Camera Roll (per-user)",
    ),
    (
        "B7BEDE81-DF94-4682-A7D8-57A52620B86F",
        "Screenshots (per-user)",
    ),
    (
        "2B20D9D9-7BBE-4C7B-A2DA-B76FAADCC677",
        "3D Objects (per-user)",
    ),
    (
        "767E6811-49CB-4273-87C2-20F355E1085B",
        "Camera Roll (per-user)",
    ),
    ("2112AB0A-C86A-4FFE-A368-0DE96E47012E", "Music (per-user)"),
    (
        "339719B5-8C47-4894-94C2-D8F77ADD44A6",
        "Pictures (per-user)",
    ),
    (
        "33E28130-4E1E-4676-835A-98395C3BC3BB",
        "Pictures (per-user)",
    ),
    (
        "A302545D-DEFF-464B-ABE8-61C8648D939B",
        "Libraries (virtual)",
    ),
    (
        "18989B1D-99B5-455B-841C-AB7C74E4DDFC",
        "MyVideos (per-user)",
    ),
    ("491E922F-5643-4AF4-A7EB-4E7A138D8174", "Videos (per-user)"),
    // ref Google AI and from samples
    (
        "00021401-0000-0000-C000-000000000046",
        "Shell Link Class Identifier",
    ),
    ("20D04FE0-3AEA-1069-A2D8-08002B30309D", "My Computer"),
    ("450D8FBA-AD25-11D0-A2A8-0800361B3003", "My Documents"),
    ("D8B0C1EE-DA91-44CB-A0F8-6851F14ECBC7", "OneDrive"),
    (
        "B4FB3F98-C1EA-428D-A78A-D1F5659CBA93",
        "My Documents (Home)",
    ),
    ("F02C1A0D-BE21-4350-88B0-7367FC96EF3C", "Network"),
    ("871C5380-42A0-1069-A2EA-08002B30301D", "Internet Explorer"),
    ("645FF040-5081-101B-9F08-00AA002F954E", "Recycle Bin"),
    ("B4FB3F98-C1EA-428D-A78A-D1F5659CBA93", "HomeGroup"),
    ("9E395ED8-512D-4315-9960-9110B74616C8", "Recent Items"),
    (
        "21EC2020-3AEA-1069-A2DD-08002B30309D",
        "Control Panel Items",
    ),
    (
        "7007ACC7-3202-11D1-AAD2-00805FC1270E",
        "Network Connections",
    ),
    // New in 13.53
    ("26EE0668-A00A-44D7-9371-BEB064C98683", "Control Panel"),
    (
        "2559A1F1-21D7-11D4-BDAF-00C04F60B9F0",
        "Windows Help and Support",
    ),
    ("031E4825-7B94-4DC3-B131-E946B44C8DD5", "Libraries"),
    ("22877A6D-37A1-461A-91B0-DBDA5AAEBC99", "Recent Items"),
    ("2559A1F3-21D7-11D4-BDAF-00C04F60B9F0", "Run Dialog Box"),
    ("3080F90D-D7AD-11D9-BD98-0000947B0257", "Desktop"),
    ("3080F90E-D7AD-11D9-BD98-0000947B0257", "Task View"),
    (
        "4336A54D-038B-4685-AB02-99BB52D3FB8B",
        "Public User Root Folder",
    ),
    ("5399E694-6CE5-4D6C-8FCE-1D8870FDCBA0", "Control Panel"),
    ("59031A47-3F72-44A7-89C5-5595FE6B30EE", "User Profile"),
    ("871C5380-42A0-1069-A2EA-08002B30309D", "Internet"),
    ("ED228FDF-9EA8-4870-83B1-96B02CFE0D52", "Game Explorer"),
    ("A8CDFF1C-4878-43BE-B5FD-F8091C1C60D0", "Documents"),
    ("3ADD1653-EB32-4CB0-BBD7-DFA0ABB5ACCA", "My Pictures"),
    // ref https://github.com/EricZimmerman/GuidMapping
    ("0C39A5CF-1A7A-40C8-BA74-8900E6DF5FCD", "Recent Items"),
    // ref libfwsi
    ("5E591A74-DF96-48D3-8D67-1733BCEE28BA", "Delegate GUID"),
    ("04731B67-D933-450A-90E6-4ACD2E9408FE", "Search Folder"),
    ("DFFACDC5-679F-4156-8947-C5C76BC0B67F", "Users Files"),
    ("289AF617-1CC3-42A6-926C-E6A863F0E3BA", "My Computer"),
    ("3134EF9C-6B18-4996-AD04-ED5912E00EB5", "Recent Files"),
    ("35786D3C-B075-49B9-88DD-029876E11C01", "Portable Devices"),
    ("3936E9E4-D92C-4EEE-A85A-BC16D5EA0819", "Frequent Places"),
    ("59031A47-3F72-44A7-89C5-5595FE6B30EE", "Shared Documents"),
    (
        "640167B4-59B0-47A6-B335-A6B3C0695AEA",
        "Portable Media Devices",
    ),
    ("896664F7-12E1-490F-8782-C0835AFD98FC", "Libraries"),
    (
        "9113A02D-00A3-46B9-BC5F-9C04DADDD5D7",
        "Enhanced Storage Data Source",
    ),
    ("9DB7A13C-F208-4981-8353-73CC61AE2783", "Previous Versions"),
    ("B155BDF8-02F0-451E-9A26-AE317CFD7779", "NetHood"),
    ("C2B136E2-D50E-405C-8784-363C582BF43E", "Wireless Devices"),
    ("D34A6CA6-62C2-4C34-8A7C-14709C1AD938", "Common Places"),
    ("ED50FC29-B964-48A9-AFB3-15EBB9B97F36", "PrintHood"),
    ("F5FB2C77-0E2F-4A16-A381-3E560C68BC83", "Removable Drives"),
    // ref https://learn.microsoft.com/en-us/windows/win32/wpd_sdk/supporting-autoplay
    ("80E170D2-1055-4A3E-B952-82CC4F8A8689", "Content Type All"),
    (
        "0FED060E-8793-4B1E-90C9-48AC389AC631",
        "Content Type Appointment",
    ),
    ("4AD2C85E-5E2D-45E5-8864-4F229E3C6CF0", "Content Type Audio"),
    (
        "AA18737E-5009-48FA-AE21-85F24383B4E6",
        "Content Type Audio Album",
    ),
    (
        "A1FD5967-6023-49A0-9DF1-F8060BE751B0",
        "Content Type Calendar",
    ),
    (
        "DC3876E8-A948-4060-9050-CBD77E8A3D87",
        "Content Type Certificate",
    ),
    (
        "EABA8313-4525-4707-9F0E-87C6808E9435",
        "Content Type Contact",
    ),
    (
        "346B8932-4C36-40D8-9415-1828291F9DE9",
        "Content Type Contact Group",
    ),
    (
        "680ADF52-950A-4041-9B41-65E393648155",
        "Content Type Document",
    ),
    ("8038044A-7E51-4F8F-883D-1D0623D14533", "Content Type Email"),
    (
        "27E2E392-A111-48E0-AB0C-E17705A05F85",
        "Content Type Folder",
    ),
    (
        "99ED0160-17FF-4C44-9D98-1D7A6F941921",
        "Content Type Functional Object",
    ),
    (
        "0085E0A6-8D34-45D7-BC5C-447E59C73D48",
        "Content Type Generic File",
    ),
    (
        "E80EAAF8-B2DB-4133-B67E-1BEF4B4A6E5F",
        "Content Type Generic Message",
    ),
    ("EF2107D5-A52A-4243-A26B-62D4176D7603", "Content Type Image"),
    (
        "75793148-15F5-4A30-A813-54ED8A37E226",
        "Content Type Image Album",
    ),
    (
        "5E88B3CC-3E65-4E62-BFFF-229495253AB0",
        "Content Type Media Cast",
    ),
    ("9CD20ECF-3B50-414F-A641-E473FFE45751", "Content Type Memo"),
    (
        "00F0C3AC-A593-49AC-9219-24ABCA5A2563",
        "Content Type Mixed Content Album",
    ),
    (
        "031DA7EE-18C8-4205-847E-89A11261D0F3",
        "Content Type Network Association",
    ),
    (
        "1A33F7E4-AF13-48F5-994E-77369DFE04A3",
        "Content Type Playlist",
    ),
    (
        "D269F96A-247C-4BFF-98FB-97F3C49220E6",
        "Content Type Program",
    ),
    (
        "821089F5-1D91-4DC9-BE3C-BBB1B35B18CE",
        "Content Type Section",
    ),
    ("63252F2C-887F-4CB6-B1AC-D29855DCEF6C", "Content Type Task"),
    (
        "60A169CF-F2AE-4E21-9375-9677F11C1C6E",
        "Content Type Television",
    ),
    (
        "28D8D31E-249C-454E-AABC-34883168E634",
        "Content Type Unspecified",
    ),
    ("9261B03C-3D78-4519-85E3-02C5E1F50BB9", "Content Type Video"),
    (
        "012B0DB7-D4C1-45D6-B081-94B87779614F",
        "Content Type Video Album",
    ),
    (
        "0BAC070A-9F5F-4DA4-A8F6-3DE44D68FD6C",
        "Content Type Wireless Profile",
    ),
];

/// Process the LNK main header
fn process_lnk_header(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 0x48 {
        return;
    }

    // Flags at 0x14
    if let Some(flags) = read_u32_le(data, 0x14) {
        let flag_bits = [
            (0u32, "IDList"),
            (1, "LinkInfo"),
            (2, "Description"),
            (3, "RelativePath"),
            (4, "WorkingDir"),
            (5, "CommandArgs"),
            (6, "IconFile"),
            (7, "Unicode"),
            (8, "NoLinkInfo"),
            (9, "ExpString"),
            (10, "SeparateProc"),
            (12, "DarwinID"),
            (13, "RunAsUser"),
            (14, "ExpIcon"),
            (15, "NoPidAlias"),
            (17, "RunWithShim"),
            (18, "NoLinkTrack"),
            (19, "TargetMetadata"),
            (20, "NoLinkPathTracking"),
            (21, "NoKnownFolderTracking"),
            (22, "NoKnownFolderAlias"),
            (23, "LinkToLink"),
            (24, "UnaliasOnSave"),
            (25, "PreferEnvPath"),
            (26, "KeepLocalIDList"),
        ];
        let s = flags_to_string(flags, &flag_bits);
        tags.push(mk_str("Flags", &s));
    }

    // FileAttributes at 0x18
    if let Some(attrs) = read_u32_le(data, 0x18) {
        let attr_bits = [
            (0u32, "Read-only"),
            (1, "Hidden"),
            (2, "System"),
            (4, "Directory"),
            (5, "Archive"),
            (7, "Normal"),
            (8, "Temporary"),
            (9, "Sparse"),
            (10, "Reparse point"),
            (11, "Compressed"),
            (12, "Offline"),
            (13, "Not indexed"),
            (14, "Encrypted"),
        ];
        let s = if attrs == 0 {
            "(none)".to_string()
        } else if attrs & 0x80 != 0 {
            "Normal".to_string()
        } else {
            let parts: Vec<&str> = attr_bits
                .iter()
                .filter(|(bit, _)| attrs & (1 << bit) != 0)
                .map(|(_, name)| *name)
                .collect();
            if parts.is_empty() {
                format!("0x{:08x}", attrs)
            } else {
                parts.join(", ")
            }
        };
        tags.push(mk_str("FileAttributes", &s));
    }

    // CreateDate at 0x1c (8 bytes, FILETIME)
    if let (Some(lo), Some(hi)) = (read_u32_le(data, 0x1c), read_u32_le(data, 0x20)) {
        if let Some(dt) = filetime_to_datetime(lo, hi, 0) {
            tags.push(mk_time("CreateDate", &dt));
        }
    }

    // AccessDate at 0x24 (8 bytes, FILETIME)
    if let (Some(lo), Some(hi)) = (read_u32_le(data, 0x24), read_u32_le(data, 0x28)) {
        if let Some(dt) = filetime_to_datetime(lo, hi, 0) {
            tags.push(mk_time("AccessDate", &dt));
        }
    }

    // ModifyDate at 0x2c (8 bytes, FILETIME)
    if let (Some(lo), Some(hi)) = (read_u32_le(data, 0x2c), read_u32_le(data, 0x30)) {
        if let Some(dt) = filetime_to_datetime(lo, hi, 0) {
            tags.push(mk_time("ModifyDate", &dt));
        }
    }

    // TargetFileSize at 0x34
    if let Some(sz) = read_u32_le(data, 0x34) {
        tags.push(mk("TargetFileSize", Value::U32(sz)));
    }

    // IconIndex at 0x38
    if let Some(idx) = read_u32_le(data, 0x38) {
        let s = if idx == 0 {
            "(none)".to_string()
        } else {
            format!("{}", idx)
        };
        tags.push(mk_str("IconIndex", &s));
    }

    // RunWindow at 0x3c
    if let Some(rw) = read_u32_le(data, 0x3c) {
        let s = match rw {
            0 => "Hide",
            1 => "Normal",
            2 => "Show Minimized",
            3 => "Show Maximized",
            4 => "Show No Activate",
            5 => "Show",
            6 => "Minimized",
            7 => "Show Minimized No Activate",
            8 => "Show NA",
            9 => "Restore",
            10 => "Show Default",
            _ => "Unknown",
        };
        tags.push(mk_str("RunWindow", s));
    }

    // HotKey at 0x40
    if let Some(hk) = read_u32_le(data, 0x40) {
        let s = if hk == 0 {
            "(none)".to_string()
        } else {
            let ch = hk & 0xff;
            let mut key = if ch >= 0x30 && ch <= 0x39 {
                format!("{}", (ch as u8) as char)
            } else if ch >= 0x41 && ch <= 0x5a {
                format!("{}", (ch as u8) as char)
            } else if ch >= 0x70 && ch <= 0x87 {
                format!("F{}", ch - 0x6f)
            } else if ch == 0x90 {
                "Num Lock".to_string()
            } else if ch == 0x91 {
                "Scroll Lock".to_string()
            } else {
                format!("Unknown (0x{:x})", ch)
            };
            if hk & 0x400 != 0 {
                key = format!("Alt-{}", key);
            }
            if hk & 0x200 != 0 {
                key = format!("Control-{}", key);
            }
            if hk & 0x100 != 0 {
                key = format!("Shift-{}", key);
            }
            key
        };
        tags.push(mk_str("HotKey", &s));
    }
}

/// Process ItemID list to extract target file info
fn process_item_id(data: &[u8], tags: &mut Vec<Tag>) {
    let mut pos = 0;
    while pos + 2 <= data.len() {
        let size = match read_u16_le(data, pos) {
            Some(s) => s as usize,
            None => break,
        };
        if size == 0 {
            break;
        }
        if size < 4 {
            break;
        }
        let actual_size = if pos + size > data.len() {
            data.len() - pos
        } else {
            size
        };
        let item_data = &data[pos..pos + actual_size];

        let item_type = data[pos + 2];

        // Find 0xbeef extension block offset within item data
        let beef_start = find_beef_offset(item_data);

        // Determine effective data length for the item (before beef extensions)
        let item_len = if let Some(bs) = beef_start {
            bs
        } else {
            actual_size
        };

        // Resolve tag ranges: 0x20-0x2f -> MyComputer, 0x30-0x3f -> ShellFSFolder, 0x40-0x4f -> NetworkLocation
        let effective_type = resolve_item_type(item_type);

        match effective_type {
            0x00 => process_item_00(&item_data[..item_len], tags),
            0x01 => process_control_panel_info(&item_data[..item_len], tags),
            0x1e | 0x1f => process_root_folder(&item_data[..item_len], tags),
            0x2e => process_volume_guid(&item_data[..item_len], tags),
            0x2f => {
                // VolumeName: extract ASCII name from offset 3
                if item_len > 5 {
                    if let Some(name) = read_cstring(item_data, 5) {
                        if !name.is_empty() {
                            tags.push(mk_str("VolumeName", &name));
                        }
                    }
                }
            }
            0x31 => process_target_info(&item_data[..item_len], tags),
            0x40 => process_network_location(&item_data[..item_len], tags),
            0x61 => process_uri_item(&item_data[..item_len], tags),
            0x70 | 0x71 => {
                // ControlPanelShellItem - GUID at offset 14
                if item_len >= 30 {
                    if let Some(s) = format_guid_with_name(&item_data[16..32]) {
                        tags.push(mk_str("ControlPanelShellItem", &s));
                    }
                }
            }
            0x74 => process_users_files_folder(&item_data[..item_len], tags),
            0xff => process_vendor_data(&item_data[..item_len], tags),
            _ => {}
        }

        // Process 0xbeef extension blocks
        if let Some(bs) = beef_start {
            process_beef_extensions(item_data, bs, tags);
        }

        pos += actual_size;
    }
}

/// Find 0xbeef extension block start within item data.
/// The Perl code searches for `.{5}\0\xef\xbe` (an 8-byte pattern), then uses pos-8
/// as the extension block offset. The extension block layout is:
///   [0-1] size, [2-3] version, [4-7] signature (e.g. LE 0xbeef0004 = 04 00 ef be)
/// The last 2 bytes of the item data contain the offset pointer to verify.
fn find_beef_offset(data: &[u8]) -> Option<usize> {
    if data.len() < 10 {
        return None;
    }
    // Search for the 0xbeef signature pattern: any byte, 0x00, 0xef, 0xbe at offsets +4..+7
    // So the beef block starts 4 bytes before the signature byte
    for i in 4..data.len().saturating_sub(3) {
        // Check signature at the u32 starting at offset i: XX 00 EF BE
        if data[i + 1] == 0x00 && data[i + 2] == 0xef && data[i + 3] == 0xbe {
            let block_start = i - 4; // the extension block starts 4 bytes before signature
            if block_start < 5 {
                continue;
            } // must be after at least some item data
              // Verify: the last 2 bytes of the item should point to this offset
            let off2 = read_u16_le(data, data.len() - 2).unwrap_or(0) as usize;
            if off2 == block_start {
                return Some(block_start);
            }
        }
    }
    None
}

/// Resolve item type ranges to canonical types
fn resolve_item_type(t: u8) -> u8 {
    match t {
        // 0x20-0x2f: MyComputer range (even have GUID, odd have name)
        0x20..=0x2d => 0x2e, // even IDs -> VolumeGUID-like
        0x2e => 0x2e,
        0x2f => 0x2f,
        // 0x30-0x3f: ShellFSFolder
        0x30..=0x3f => 0x31,
        // 0x40-0x4f: NetworkLocation
        0x40..=0x4f => 0x40,
        other => other,
    }
}

/// Process item type 0x00 variants
fn process_item_00(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 8 {
        return;
    }

    // Check for ControlPanelCPL (special ID 0xffffffxx)
    if data.len() > 7 && data[5] == 0xff && data[6] == 0xff && data[7] == 0xff {
        if let Some(special_type) = read_u32_le(data, 4) {
            tags.push(mk_str(
                "Item00SpecialType",
                &format!("0x{:08x} (ControlPanelCPL)", special_type),
            ));
        }
        // Extract CPL file path strings
        if data.len() > 14 {
            extract_item00_strings(data, 14, tags, "CPLFilePath");
        }
        return;
    }

    // Check for GameFolderInfo (ID 0x49534647 = "GFSI")
    if data.len() > 7 && &data[4..8] == b"GFSI" {
        tags.push(mk_str("Item00SpecialType", "0x49534647 (GameFolder)"));
        return;
    }

    // Check for PropertyStore (ID 0x23febbee)
    if data.len() > 9 && data[6] == 0xee && data[7] == 0xbb && data[8] == 0xfe && data[9] == 0x23 {
        // PropertyStoreGUID at offset 14 (16 bytes, but skip first 2)
        if data.len() >= 30 {
            if let Some(s) = format_guid_with_name(&data[14..30]) {
                tags.push(mk_str("PropertyStoreGUID", &s));
            }
        }
        return;
    }

    // Check for MTPType2 (ID 0x10312005)
    if data.len() > 9 && data[6] == 0x05 && data[7] == 0x20 && data[8] == 0x31 && data[9] == 0x10 {
        process_mtp_type2(data, tags);
        return;
    }

    // Generic Item00Info
    if data.len() >= 10 {
        if let Some(item_type) = read_u32_le(data, 6) {
            tags.push(mk_str("Item00Type", &format!("0x{:08x}", item_type)));
        }
    }

    // Try to extract property strings
    if data.len() >= 26 {
        if let (Some(prop1_len), Some(prop2_len)) = (read_u16_le(data, 20), read_u16_le(data, 22)) {
            let expected_size = 24 + 2 * (prop1_len as usize + prop2_len as usize);
            if expected_size == data.len() && prop1_len > 0 {
                let off1 = 24;
                let byte_len1 = prop1_len as usize * 2;
                if let Some(s) = read_utf16le_fixed(data, off1, byte_len1) {
                    if !s.is_empty() {
                        tags.push(mk_str("PropertyString1", &s));
                    }
                }
                if prop2_len > 0 {
                    let off2 = off1 + byte_len1;
                    let byte_len2 = prop2_len as usize * 2;
                    if let Some(s) = read_utf16le_fixed(data, off2, byte_len2) {
                        if !s.is_empty() {
                            tags.push(mk_str("PropertyString2", &s));
                        }
                    }
                }
            }
        }
    }
}

/// Extract strings (ASCII or Unicode) from item00 data for CPLFilePath
fn extract_item00_strings(data: &[u8], off: usize, tags: &mut Vec<Tag>, tag_name: &str) {
    if off >= data.len() {
        return;
    }
    let sub = &data[off..];
    // Check if it looks like Unicode (ASCII char followed by 0x00)
    if sub.len() >= 3
        && sub[0] >= 0x20
        && sub[0] <= 0x7f
        && sub[1] == 0x00
        && sub[2] >= 0x20
        && sub[2] <= 0x7f
    {
        // Unicode strings
        let mut strings = Vec::new();
        let mut pos = 0;
        while pos + 1 < sub.len() {
            if let Some(s) = read_utf16le_string(sub, pos) {
                if !s.is_empty() {
                    let byte_len = s.encode_utf16().count() * 2 + 2;
                    strings.push(s);
                    pos += byte_len;
                } else {
                    pos += 2;
                }
            } else {
                break;
            }
        }
        if !strings.is_empty() {
            tags.push(mk_str(tag_name, &strings.join(", ")));
        }
    } else {
        // ASCII strings
        let mut strings = Vec::new();
        for segment in sub.split(|&b| b == 0) {
            let s: String = segment
                .iter()
                .filter(|&&b| b >= 0x20 && b <= 0x7f)
                .map(|&b| b as char)
                .collect();
            if !s.is_empty() {
                strings.push(s);
            }
        }
        if !strings.is_empty() {
            tags.push(mk_str(tag_name, &strings.join(", ")));
        }
    }
}

/// Process MTPType2 item
fn process_mtp_type2(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 54 {
        return;
    }
    let storage_name_len = read_u32_le(data, 38).unwrap_or(0) as usize;
    let storage_id_len = read_u32_le(data, 42).unwrap_or(0) as usize;
    let fs_name_len = read_u32_le(data, 46).unwrap_or(0) as usize;
    let num_guids = read_u32_le(data, 50).unwrap_or(0) as usize;

    let mut off = 54;
    // MTPStorageName
    let byte_len = storage_name_len * 2;
    if off + byte_len <= data.len() {
        if let Some(s) = read_utf16le_fixed(data, off, byte_len) {
            if !s.is_empty() {
                tags.push(mk_str("MTPStorageName", &s));
            }
        }
    }
    off += byte_len;

    // MTPStorageID
    let byte_len = storage_id_len * 2;
    if off + byte_len <= data.len() {
        if let Some(s) = read_utf16le_fixed(data, off, byte_len) {
            if !s.is_empty() {
                tags.push(mk_str("MTPStorageID", &s));
            }
        }
    }
    off += byte_len;

    // MTPFileSystem
    let byte_len = fs_name_len * 2;
    if off + byte_len <= data.len() {
        if let Some(s) = read_utf16le_fixed(data, off, byte_len) {
            if !s.is_empty() {
                tags.push(mk_str("MTPFileSystem", &s));
            }
        }
    }
    off += byte_len;

    // MTP GUIDs (72 bytes each = 36 UTF-16 chars representing a GUID string)
    let max_guids = num_guids.min(8);
    for i in 0..max_guids {
        if off + 72 > data.len() {
            break;
        }
        if let Some(s) = read_utf16le_fixed(data, off, 72) {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                let name = format!("MTP_GUID{}", i + 1);
                if let Some(lookup) = guid_lookup(trimmed) {
                    tags.push(mk_str(&name, &format!("{} ({})", trimmed, lookup)));
                } else {
                    tags.push(mk_str(&name, trimmed));
                }
            }
        }
        off += 78; // 72 bytes + padding to next entry (78 bytes stride per Perl: offsets 56,134,212,290,368,446,524,602)
    }
}

/// Process ControlPanelInfo (item type 0x01)
fn process_control_panel_info(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 12 {
        return;
    }
    if let Some(cat) = read_u32_le(data, 8) {
        let s = match cat {
            0 => "All Control Panel Items",
            1 => "Appearance and Personalization",
            2 => "Hardware and Sound",
            3 => "Network and Internet",
            4 => "Sounds, Speech, and Audio Devices",
            5 => "System and Security",
            6 => "Clock, Language, and Region",
            7 => "Ease of Access",
            8 => "Programs",
            9 => "User Accounts",
            10 => "Security Center",
            11 => "Mobile PC",
            _ => "",
        };
        if s.is_empty() {
            tags.push(mk_str("ControlPanelCategory", &format!("{}", cat)));
        } else {
            tags.push(mk_str("ControlPanelCategory", s));
        }
    }
}

/// Process RootFolder (item types 0x1e, 0x1f)
fn process_root_folder(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 5 {
        return;
    }

    // SortIndex at offset 3
    let sort_idx = data[3];
    let sort_name = match sort_idx {
        0x00 => "Internet Explorer",
        0x42 => "Libraries",
        0x44 => "Users",
        0x48 => "My Documents",
        0x4c => "Public Folder",
        0x50 => "My Computer",
        0x54 => "Users Libraries",
        0x58 => "My Network Places/Network",
        0x60 => "Recycle Bin",
        0x68 => "Internet Explorer",
        0x70 => "Control Panel",
        0x78 => "Recycle Bin",
        0x80 => "My Games",
        _ => "",
    };
    if !sort_name.is_empty() {
        tags.push(mk_str(
            "SortIndex",
            &format!("0x{:02x} ({})", sort_idx, sort_name),
        ));
    } else {
        tags.push(mk_str("SortIndex", &format!("0x{:02x}", sort_idx)));
    }

    // RootFolderGUID at offset 4 (16 bytes)
    if data.len() >= 20 {
        if let Some(s) = format_guid_with_name(&data[4..20]) {
            tags.push(mk_str("RootFolderGUID", &s));
        }
    }
}

/// Process VolumeGUID (item type 0x2e)
fn process_volume_guid(data: &[u8], tags: &mut Vec<Tag>) {
    // GUID is last 16 bytes (seen both 0x14 and 0x32 bytes long)
    if data.len() < 20 {
        return;
    }
    let guid_off = data.len() - 16;
    if let Some(s) = format_guid_with_name(&data[guid_off..guid_off + 16]) {
        tags.push(mk_str("VolumeGUID", &s));
    }
}

/// Process target info (item types 0x30-0x3f / ShellFSFolder)
fn process_target_info(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 14 {
        return;
    }

    // Offset 8: TargetFileModifyDate (LNK DOS time format)
    if let Some(val) = read_u32_le(data, 8) {
        if val != 0 {
            if let Some(dt) = dos_time(val) {
                tags.push(mk_time("TargetFileModifyDate", &dt));
            }
        }
    }

    // Offset 12: TargetFileAttributes (int16u)
    if let Some(attrs) = read_u16_le(data, 12) {
        let attr_bits = [
            (0u32, "Read-only"),
            (1, "Hidden"),
            (2, "System"),
            (4, "Directory"),
            (5, "Archive"),
            (7, "Normal"),
        ];
        let s = if attrs & 0x80 != 0 {
            "Normal".to_string()
        } else {
            let parts: Vec<&str> = attr_bits
                .iter()
                .filter(|(bit, _)| (attrs as u32) & (1 << bit) != 0)
                .map(|(_, name)| *name)
                .collect();
            if parts.is_empty() {
                "(none)".to_string()
            } else {
                parts.join(", ")
            }
        };
        tags.push(mk_str("TargetFileAttributes", &s));
    }

    // Offset 14: TargetFileDOSName
    // Could be ASCII or Unicode - check if it looks like Unicode
    if data.len() > 16 {
        if data[14] >= 0x20
            && data[14] <= 0x7f
            && data[15] == 0x00
            && data.len() > 16
            && data[16] >= 0x20
            && data[16] <= 0x7f
        {
            // Unicode
            if let Some(name) = read_utf16le_string(data, 14) {
                if !name.is_empty() {
                    tags.push(mk_str("TargetFileDOSName", &name));
                }
            }
        } else {
            // ASCII
            if let Some(name) = read_cstring(data, 14) {
                if !name.is_empty() {
                    tags.push(mk_str("TargetFileDOSName", &name));
                }
            }
        }
    }
}

/// Process NetworkLocation (item types 0x40-0x4f)
fn process_network_location(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 8 {
        return;
    }
    // Extract ASCII strings from offset 6 onwards
    let sub = &data[6..];
    let mut strings = Vec::new();
    for segment in sub.split(|&b| b == 0) {
        let s: String = segment
            .iter()
            .filter(|&&b| b >= 0x20 && b <= 0x7f)
            .map(|&b| b as char)
            .collect();
        if !s.is_empty() {
            strings.push(s);
        }
    }
    if !strings.is_empty() {
        tags.push(mk_str("NetworkLocation", &strings.join(", ")));
    }
}

/// Process URI shell item (type 0x61)
fn process_uri_item(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 8 {
        return;
    }
    let uri_flags = data[3];
    let uri_data_size = read_u16_le(data, 4).unwrap_or(0);
    let is_unicode = uri_flags & 0x80 != 0;

    if uri_data_size == 0 {
        // Simple URI: string from offset 8 to end
        if data.len() > 8 {
            let s = if is_unicode {
                read_utf16le_string(data, 8).unwrap_or_default()
            } else {
                read_cstring(data, 8).unwrap_or_default()
            };
            if !s.is_empty() {
                tags.push(mk_str("URI", &s));
            }
        }
    } else {
        // URI with FTP data
        // Offset 14: TimeStamp (FILETIME, 8 bytes)
        if data.len() >= 22 {
            if let Some(ft) = read_u64_le(data, 14) {
                if let Some(dt) = filetime64_to_datetime(ft) {
                    tags.push(mk_time("TimeStamp", &dt));
                }
            }
        }

        // FTP fields: host, username, password, then URI
        let mut off = 42;
        let ftp_fields = ["FTPHost", "FTPUserName", "FTPPassword"];
        for field_name in &ftp_fields {
            if off + 4 > data.len() {
                break;
            }
            let field_len = read_u32_le(data, off).unwrap_or(0) as usize;
            off += 4;
            if field_len > 0 && off + field_len <= data.len() {
                let s = if is_unicode {
                    read_utf16le_fixed(data, off, field_len).unwrap_or_default()
                } else {
                    crate::encoding::decode_utf8_or_latin1(&data[off..off + field_len])
                        .trim_end_matches('\0')
                        .to_string()
                };
                if !s.is_empty() {
                    tags.push(mk_str(field_name, &s));
                }
            }
            off += field_len;
        }

        // Remaining data is the URI
        if off < data.len() {
            let s = if is_unicode {
                read_utf16le_string(data, off).unwrap_or_default()
            } else {
                read_cstring(data, off).unwrap_or_default()
            };
            if !s.is_empty() {
                tags.push(mk_str("URI", &s));
            }
        }
    }
}

/// Process UsersFilesFolder (item type 0x74)
fn process_users_files_folder(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 6 {
        return;
    }
    // Offset 4: inner data size (skip it)
    let inner_size = read_u16_le(data, 4).unwrap_or(0) as usize;
    let off = 6 + inner_size;

    // DelegateClassGUID at offset off (16 bytes)
    if off + 16 <= data.len() {
        if let Some(s) = format_guid_with_name(&data[off..off + 16]) {
            tags.push(mk_str("DelegateClassGUID", &s));
        }
    }

    // DelegateFolderGUID at offset off+16 (16 bytes)
    if off + 32 <= data.len() {
        if let Some(s) = format_guid_with_name(&data[off + 16..off + 32]) {
            tags.push(mk_str("DelegateFolderGUID", &s));
        }
    }
}

/// Process VendorData (item type 0xff)
fn process_vendor_data(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 4 {
        return;
    }
    // Extract Unicode and ASCII strings (min 3 chars, null terminated)
    let mut strings = Vec::new();

    // Try Unicode first
    let mut i = 2;
    while i + 5 < data.len() {
        if data[i] >= 0x20 && data[i] <= 0x7f && data[i + 1] == 0x00 {
            if let Some(s) = read_utf16le_string(data, i) {
                if s.len() >= 3 {
                    strings.push(s.clone());
                    i += (s.encode_utf16().count() * 2) + 2;
                    continue;
                }
            }
        }
        i += 1;
    }

    // Also try ASCII
    if strings.is_empty() {
        for segment in data[2..].split(|&b| b == 0) {
            let s: String = segment
                .iter()
                .filter(|&&b| b >= 0x20 && b <= 0x7e)
                .map(|&b| b as char)
                .collect();
            if s.len() >= 3 {
                strings.push(s);
            }
        }
    }

    if !strings.is_empty() {
        tags.push(mk_str("VendorData", &strings.join(", ")));
    }
}

/// Process 0xbeef extension blocks within an item
fn process_beef_extensions(data: &[u8], start: usize, tags: &mut Vec<Tag>) {
    let end = data.len();
    let mut off = start;

    while off + 8 <= end {
        let len = read_u16_le(data, off).unwrap_or(0) as usize;
        if len < 4 {
            break;
        }
        if off + len > end {
            break;
        }

        let beef_id = read_u32_le(data, off + 4).unwrap_or(0);
        if beef_id & 0xffff0000 != 0xbeef0000 {
            break;
        }

        let block = &data[off..off + len];

        match beef_id {
            0xbeef0003 => process_beef0003(block, tags),
            0xbeef0004 => process_beef0004(block, tags),
            0xbeef0014 => process_beef0014(block, tags),
            0xbeef0025 => process_beef0025(block, tags),
            0xbeef0026 => process_beef0026(block, tags),
            _ => {}
        }

        // Align to 2-byte boundary
        let padded_len = if len & 1 != 0 { len + 1 } else { len };
        off += padded_len;
    }
}

/// Process beef0003 extension (UnknownGUID)
fn process_beef0003(data: &[u8], tags: &mut Vec<Tag>) {
    // GUID at offset 8 (16 bytes)
    if data.len() >= 24 {
        if let Some(s) = format_guid_with_name(&data[8..24]) {
            tags.push(mk_str("UnknownGUID", &s));
        }
    }
}

/// Process beef0004 extension (TargetInfo extensions)
fn process_beef0004(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 12 {
        return;
    }

    let version = read_u16_le(data, 2).unwrap_or(0);

    // TargetFileCreateDate at offset 8 (DOSTime)
    if let Some(val) = read_u32_le(data, 8) {
        if val != 0 {
            if let Some(dt) = dos_time(val) {
                tags.push(mk_time("TargetFileCreateDate", &dt));
            }
        }
    }

    // TargetFileAccessDate at offset 12 (DOSTime)
    if data.len() >= 16 {
        if let Some(val) = read_u32_le(data, 12) {
            if val != 0 {
                if let Some(dt) = dos_time(val) {
                    tags.push(mk_time("TargetFileAccessDate", &dt));
                }
            }
        }
    }

    // OperatingSystem at offset 16 (int16u), then variable offsets
    if data.len() >= 18 {
        if let Some(os_val) = read_u16_le(data, 16) {
            let os_name = match os_val {
                0x14 => "Windows XP, 2003",
                0x26 => "Windows Vista",
                0x2a => "Windows 2008, 7, 8",
                0x2e => "Windows 8.1, 10",
                _ => "",
            };
            if !os_name.is_empty() {
                tags.push(mk_str("OperatingSystem", os_name));
            } else {
                tags.push(mk_str("OperatingSystem", &format!("0x{:04x}", os_val)));
            }
        }
    }

    // TargetFileName: variable offset depends on version
    // Calculate varSize like Perl does
    let mut var_size: usize = 0;
    if version >= 7 {
        var_size += 18;
    }
    if version >= 3 {
        var_size += 2;
    }
    if version >= 9 {
        var_size += 4;
    }
    if version >= 8 {
        var_size += 4;
    }

    let name_off = 18 + var_size;
    if name_off + 4 < data.len() {
        let name_len = data.len().saturating_sub(name_off + 2); // drop offset word after name
        if name_len >= 2 {
            // Extract UTF-16 strings (one or two null-terminated)
            let name_data = &data[name_off..name_off + name_len];
            let mut strings = Vec::new();
            let mut pos = 0;
            while pos + 1 < name_data.len() {
                if let Some(s) = read_utf16le_string(name_data, pos) {
                    if !s.is_empty() {
                        let byte_len = s.encode_utf16().count() * 2 + 2;
                        strings.push(s);
                        pos += byte_len;
                    } else {
                        pos += 2;
                    }
                } else {
                    break;
                }
            }
            if strings.len() == 1 {
                tags.push(mk_str("TargetFileName", &strings[0]));
            } else if strings.len() > 1 {
                tags.push(mk_str("TargetFileName", &strings.join(", ")));
            }
        }
    }
}

/// Process beef0014 extension (URI data)
fn process_beef0014(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 56 {
        return;
    }

    let num = read_u32_le(data, 52).unwrap_or(0) as usize;
    let mut off = 56;

    let uri_tag_names = [
        "AbsoluteURI",
        "URIAuthority",
        "DisplayURI",
        "URIDomain",
        "URIExtension",
        "URIFragment",
        "URIHost",
        "URIPassword",
        "URIPath",
        "URIPathAndQuery",
        "URIQuery",
        "RawURI",
        "URISchemeName",
        "URIUserInfo",
        "URIUserName",
        "URIHostType",
        "URIPort",
        "URIScheme",
        "URIZone",
    ];

    for _i in 0..num {
        if off + 8 > data.len() {
            break;
        }
        let tag_id = read_u32_le(data, off).unwrap_or(0) as usize;
        let size = read_u32_le(data, off + 4).unwrap_or(0) as usize;
        off += 8;
        if size == 0 {
            continue;
        }
        if off + size > data.len() {
            break;
        }

        let val = read_utf16le_fixed(data, off, size).unwrap_or_default();
        if !val.is_empty() {
            let name = if tag_id < uri_tag_names.len() {
                uri_tag_names[tag_id]
            } else {
                "UnknownURI"
            };
            tags.push(mk_str(name, &val));
        }
        off += size;
    }
}

/// Process beef0025 extension (FileTime1, FileTime2)
fn process_beef0025(data: &[u8], tags: &mut Vec<Tag>) {
    // FileTime1 at offset 0x0c (8 bytes)
    if data.len() >= 0x14 {
        if let Some(ft) = read_u64_le(data, 0x0c) {
            if let Some(dt) = filetime64_to_datetime(ft) {
                tags.push(mk_time("FileTime1", &dt));
            }
        }
    }
    // FileTime2 at offset 0x14 (8 bytes)
    if data.len() >= 0x1c {
        if let Some(ft) = read_u64_le(data, 0x14) {
            if let Some(dt) = filetime64_to_datetime(ft) {
                tags.push(mk_time("FileTime2", &dt));
            }
        }
    }
}

/// Process beef0026 extension (CreateDate, ModifyDate, LastAccessDate)
fn process_beef0026(data: &[u8], tags: &mut Vec<Tag>) {
    // Check condition: byte at offset 8 should be one of 0x11, 0x10, 0x12, 0x34, 0x31
    if data.len() < 9 {
        return;
    }
    let marker = data[8];
    if marker != 0x11 && marker != 0x10 && marker != 0x12 && marker != 0x34 && marker != 0x31 {
        return;
    }

    // CreateDate at offset 0x0c (8 bytes FILETIME)
    if data.len() >= 0x14 {
        if let Some(ft) = read_u64_le(data, 0x0c) {
            if let Some(dt) = filetime64_to_datetime(ft) {
                tags.push(mk_time("CreateDate", &dt));
            }
        }
    }
    // ModifyDate at offset 0x14
    if data.len() >= 0x1c {
        if let Some(ft) = read_u64_le(data, 0x14) {
            if let Some(dt) = filetime64_to_datetime(ft) {
                tags.push(mk_time("ModifyDate", &dt));
            }
        }
    }
    // LastAccessDate at offset 0x1c
    if data.len() >= 0x24 {
        if let Some(ft) = read_u64_le(data, 0x1c) {
            if let Some(dt) = filetime64_to_datetime(ft) {
                tags.push(mk_time("LastAccessDate", &dt));
            }
        }
    }
}

/// Process LinkInfo structure
fn process_link_info(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 0x1c {
        return;
    }
    let hdr_len = read_u32_le(data, 4).unwrap_or(0x1c) as usize;
    let lif = read_u32_le(data, 8).unwrap_or(0);

    if lif & 0x01 != 0 {
        // Volume ID info
        let vol_off = read_u32_le(data, 0x0c).unwrap_or(0) as usize;
        if vol_off != 0 && vol_off + 0x14 <= data.len() {
            // DriveType at vol_off + 4
            if let Some(dt) = read_u32_le(data, vol_off + 4) {
                let s = match dt {
                    0 => "Unknown",
                    1 => "Invalid Root Path",
                    2 => "Removable Media",
                    3 => "Fixed Disk",
                    4 => "Remote Drive",
                    5 => "CD-ROM",
                    6 => "Ram Disk",
                    _ => "Unknown",
                };
                tags.push(mk_str("DriveType", s));
            }
            // DriveSerialNumber at vol_off + 8
            if let Some(sn) = read_u32_le(data, vol_off + 8) {
                let s = format!("{:04X}-{:04X}", (sn >> 16) & 0xffff, sn & 0xffff);
                tags.push(mk_str("DriveSerialNumber", &s));
            }
            // VolumeLabel
            if vol_off + 0x10 <= data.len() {
                let lbl_off_rel = read_u32_le(data, vol_off + 0x0c).unwrap_or(0) as usize;
                let (lbl_str, unicode) = if lbl_off_rel == 0x14 && vol_off + 0x14 <= data.len() {
                    // Unicode offset
                    let uni_off = read_u32_le(data, vol_off + 0x10).unwrap_or(0) as usize;
                    (read_utf16le_string(data, vol_off + uni_off), true)
                } else {
                    (read_cstring(data, vol_off + lbl_off_rel), false)
                };
                let _ = unicode;
                if let Some(lbl) = lbl_str {
                    tags.push(mk_str("VolumeLabel", &lbl));
                }
            }
        }

        // Local base path
        let lbp_off = if hdr_len >= 0x24 && data.len() >= 0x24 {
            read_u32_le(data, 0x1c).unwrap_or(0) as usize
        } else {
            read_u32_le(data, 0x10).unwrap_or(0) as usize
        };
        let unicode_path = hdr_len >= 0x24;
        let lbp = if unicode_path {
            read_utf16le_string(data, lbp_off)
        } else {
            read_cstring(data, lbp_off)
        };
        if let Some(path) = lbp {
            if !path.is_empty() {
                tags.push(mk_str("LocalBasePath", &path));
            }
        }
    }

    // CommonPathSuffix at 0x18
    let cps_off = read_u32_le(data, 0x18).unwrap_or(0) as usize;
    if cps_off != 0 && cps_off < data.len() {
        let cps = read_cstring(data, cps_off).unwrap_or_default();
        tags.push(mk_str("CommonPathSuffix", &cps));
    }
}

/// Process ConsoleData extra data block
fn process_console_data(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 0x90 {
        return;
    }

    // FillAttributes at 0x08
    if let Some(v) = read_u16_le(data, 0x08) {
        tags.push(mk_str("FillAttributes", &format!("0x{:02x}", v)));
    }
    // PopupFillAttributes at 0x0a
    if let Some(v) = read_u16_le(data, 0x0a) {
        tags.push(mk_str("PopupFillAttributes", &format!("0x{:02x}", v)));
    }
    // ScreenBufferSize at 0x0c (2 x int16u)
    if let (Some(x), Some(y)) = (read_u16_le(data, 0x0c), read_u16_le(data, 0x0e)) {
        tags.push(mk_str("ScreenBufferSize", &format!("{} x {}", x, y)));
    }
    // WindowSize at 0x10
    if let (Some(x), Some(y)) = (read_u16_le(data, 0x10), read_u16_le(data, 0x12)) {
        tags.push(mk_str("WindowSize", &format!("{} x {}", x, y)));
    }
    // WindowOrigin at 0x14
    if let (Some(x), Some(y)) = (read_u16_le(data, 0x14), read_u16_le(data, 0x16)) {
        tags.push(mk_str("WindowOrigin", &format!("{} x {}", x, y)));
    }
    // FontSize at 0x20
    if let (Some(x), Some(y)) = (read_u16_le(data, 0x20), read_u16_le(data, 0x22)) {
        tags.push(mk_str("FontSize", &format!("{} x {}", x, y)));
    }
    // FontFamily at 0x24 (with Mask 0xf0 applied, values divided by 0x10)
    if let Some(ff) = read_u32_le(data, 0x24) {
        let s = match (ff & 0xf0) >> 4 {
            0x0 => "Don't Care",
            0x1 => "Roman",
            0x2 => "Swiss",
            0x3 => "Modern",
            0x4 => "Script",
            0x5 => "Decorative",
            _ => "Unknown",
        };
        tags.push(mk_str("FontFamily", s));
    }
    // FontWeight at 0x28
    if let Some(fw) = read_u32_le(data, 0x28) {
        tags.push(mk("FontWeight", Value::U32(fw)));
    }
    // FontName at 0x2c (64 bytes UTF-16LE)
    if data.len() >= 0x6c {
        let fn_data = &data[0x2c..0x2c + 64];
        let chars: Vec<u16> = fn_data
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .take_while(|&c| c != 0)
            .collect();
        let name = String::from_utf16_lossy(&chars);
        if !name.is_empty() {
            tags.push(mk_str("FontName", &name));
        }
    }
    // CursorSize at 0x6c
    if let Some(cs) = read_u32_le(data, 0x6c) {
        tags.push(mk("CursorSize", Value::U32(cs)));
    }
    // FullScreen at 0x70
    if let Some(v) = read_u32_le(data, 0x70) {
        tags.push(mk_str("FullScreen", if v != 0 { "Yes" } else { "No" }));
    }
    // QuickEdit at 0x74
    if let Some(v) = read_u32_le(data, 0x74) {
        tags.push(mk_str("QuickEdit", if v != 0 { "Yes" } else { "No" }));
    }
    // InsertMode at 0x78
    if let Some(v) = read_u32_le(data, 0x78) {
        tags.push(mk_str("InsertMode", if v != 0 { "Yes" } else { "No" }));
    }
    // WindowOriginAuto at 0x7c
    if let Some(v) = read_u32_le(data, 0x7c) {
        tags.push(mk_str(
            "WindowOriginAuto",
            if v != 0 { "Yes" } else { "No" },
        ));
    }
    // HistoryBufferSize at 0x80
    if let Some(v) = read_u32_le(data, 0x80) {
        tags.push(mk("HistoryBufferSize", Value::U32(v)));
    }
    // NumHistoryBuffers at 0x84
    if let Some(v) = read_u32_le(data, 0x84) {
        tags.push(mk("NumHistoryBuffers", Value::U32(v)));
    }
    // RemoveHistoryDuplicates at 0x88
    if let Some(v) = read_u32_le(data, 0x88) {
        tags.push(mk_str(
            "RemoveHistoryDuplicates",
            if v != 0 { "Yes" } else { "No" },
        ));
    }
}

/// Process TrackerData extra data block
fn process_tracker_data(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 0x20 {
        return;
    }
    // MachineID at 0x10 (null-terminated string)
    if let Some(id) = read_cstring(data, 0x10) {
        if !id.is_empty() {
            tags.push(mk_str("MachineID", &id));
        }
    }
}

/// Process ConsoleFEData extra data block
fn process_console_fe_data(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 0x0c {
        return;
    }
    // CodePage at 0x08
    if let Some(cp) = read_u32_le(data, 0x08) {
        tags.push(mk("CodePage", Value::U32(cp)));
    }
}

/// Read a binary Windows .lnk (Shell Link) file
pub fn read_lnk(data: &[u8]) -> crate::error::Result<Vec<Tag>> {
    if data.len() < 0x4c {
        return Ok(Vec::new());
    }

    // Check LNK magic: header size 0x4c, CLSID starts at offset 4
    let hdr_size = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    if hdr_size < 0x4c {
        return Ok(Vec::new());
    }

    // Check CLSID: 01 14 02 00 00 00 00 00 C0 00 00 00 00 00 00 46
    let clsid_ok = &data[4..20]
        == &[
            0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x46,
        ];
    if !clsid_ok {
        return Ok(Vec::new());
    }

    let mut tags = Vec::new();
    let flags = read_u32_le(data, 0x14).unwrap_or(0);

    // Process header
    process_lnk_header(data, &mut tags);

    let mut pos = hdr_size;

    // IDList (flag bit 0)
    if flags & 0x01 != 0 {
        if pos + 2 > data.len() {
            return Ok(tags);
        }
        let list_len = read_u16_le(data, pos).unwrap_or(0) as usize;
        pos += 2;
        if pos + list_len > data.len() {
            return Ok(tags);
        }
        let id_data = &data[pos..pos + list_len];
        process_item_id(id_data, &mut tags);
        pos += list_len;
    }

    // LinkInfo (flag bit 1)
    if flags & 0x02 != 0 {
        if pos + 4 > data.len() {
            return Ok(tags);
        }
        let li_len = read_u32_le(data, pos).unwrap_or(0) as usize;
        if pos + li_len > data.len() {
            return Ok(tags);
        }
        let li_data = &data[pos..pos + li_len];
        process_link_info(li_data, &mut tags);
        pos += li_len;
    }

    // String data: Description, RelativePath, WorkingDirectory, CommandLineArguments, IconFileName
    let string_names = [
        "Description",
        "RelativePath",
        "WorkingDirectory",
        "CommandLineArguments",
        "IconFileName",
    ];
    let string_flag_masks = [0x04u32, 0x08, 0x10, 0x20, 0x40];
    let is_unicode = (flags & 0x80) != 0;

    for (i, (&mask, &name)) in string_flag_masks
        .iter()
        .zip(string_names.iter())
        .enumerate()
    {
        if flags & mask == 0 {
            continue;
        }
        if pos + 2 > data.len() {
            break;
        }
        let char_count = read_u16_le(data, pos).unwrap_or(0) as usize;
        pos += 2;
        if char_count == 0 {
            continue;
        }
        // Limit description length to 260 chars (except CommandLineArguments)
        let limit = if i != 3 { 260 } else { usize::MAX };
        let actual_count = char_count.min(limit);
        let byte_len = if is_unicode {
            actual_count * 2
        } else {
            actual_count
        };
        if pos + byte_len > data.len() {
            break;
        }
        let s = if is_unicode {
            let chars: Vec<u16> = data[pos..pos + byte_len]
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect();
            String::from_utf16_lossy(&chars).to_string()
        } else {
            crate::encoding::decode_utf8_or_latin1(&data[pos..pos + byte_len]).to_string()
        };
        let full_byte_len = if is_unicode {
            char_count * 2
        } else {
            char_count
        };
        pos += full_byte_len.min(data.len() - pos);
        if !s.is_empty() {
            tags.push(mk_str(name, &s));
        }
    }

    // Extra data blocks
    while pos + 4 <= data.len() {
        let block_len = read_u32_le(data, pos).unwrap_or(0) as usize;
        if block_len < 4 {
            break;
        }
        if pos + block_len > data.len() {
            break;
        }
        let block_data = &data[pos..pos + block_len];
        if block_data.len() < 8 {
            pos += block_len;
            continue;
        }
        let block_sig = read_u32_le(block_data, 4).unwrap_or(0);

        match block_sig {
            0xa0000002 => process_console_data(block_data, &mut tags),
            0xa0000003 => process_tracker_data(block_data, &mut tags),
            0xa0000004 => process_console_fe_data(block_data, &mut tags),
            _ => {}
        }
        pos += block_len;
    }

    Ok(tags)
}

/// Read a Windows .url file (INI-format)
pub fn read_url(data: &[u8]) -> crate::error::Result<Vec<Tag>> {
    // Check if it's a binary LNK (magic check)
    if data.len() >= 20 {
        let clsid_ok = data.len() >= 20
            && &data[4..20]
                == &[
                    0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00,
                    0x00, 0x00, 0x46,
                ];
        if clsid_ok {
            return read_lnk(data);
        }
    }

    let text = crate::encoding::decode_utf8_or_latin1(data);
    let mut tags = Vec::new();

    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.starts_with('[') {
            continue;
        }
        if let Some(eq) = line.find('=') {
            let key = &line[..eq];
            let val = &line[eq + 1..];
            match key {
                "URL" | "IconFile" | "IconIndex" | "WorkingDirectory" | "HotKey" | "Author"
                | "WhatsNew" | "Comment" | "Desc" | "Roamed" | "IDList" => {
                    tags.push(mk_str(key, val));
                }
                "Modified" => {
                    // Hex-encoded 8-byte FILETIME (little-endian uint32 lo + hi)
                    let hex = val.trim();
                    if hex.len() >= 16 {
                        let bytes: Vec<u8> = (0..16)
                            .step_by(2)
                            .filter_map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
                            .collect();
                        if bytes.len() >= 8 {
                            let lo = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                            let hi = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
                            // Get local UTC offset
                            let tz_offset = get_url_tz_offset();
                            if let Some(dt) = filetime_to_datetime(lo, hi, tz_offset / 3600) {
                                let mut t = mk_str("Modified", &dt);
                                t.group.family1 = "LNK".into();
                                tags.push(t);
                            }
                        }
                    }
                }
                "ShowCommand" => {
                    let pval = match val.trim() {
                        "1" => "Normal",
                        "2" => "Minimized",
                        "3" => "Maximized",
                        v => v,
                    };
                    let mut t = mk_str("ShowCommand", val);
                    t.print_value = pval.to_string();
                    tags.push(t);
                }
                _ => {}
            }
        }
    }

    Ok(tags)
}
