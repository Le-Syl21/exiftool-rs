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

fn get_url_tz_offset() -> i64 {
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
    Some(format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}{}{:02}:00",
        year, month, day, h, m, s, sign, abs_h))
}

fn unix_to_components(secs: i64) -> (i32, u32, u32, u32, u32, u32) {
    let s = (secs % 60 + 60) as u32 % 60;
    let secs2 = if secs % 60 < 0 { secs - secs % 60 - 60 } else { secs - secs % 60 };
    let mins = secs2 / 60;
    let m = (mins % 60 + 60) as u32 % 60;
    let mins2 = if mins % 60 < 0 { mins - mins % 60 - 60 } else { mins - mins % 60 };
    let hours = mins2 / 60;
    let h = (hours % 24 + 24) as u32 % 24;
    let hours2 = if hours % 24 < 0 { hours - hours % 24 - 24 } else { hours - hours % 24 };
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
    if val == 0 { return None; }
    let year = ((val >> 9) & 0x7f) as i32 + 1980;
    let month = (val >> 5) & 0x0f;
    let day = val & 0x1f;
    let hour = (val >> 27) & 0x1f;
    let min = (val >> 21) & 0x3f;
    let sec = ((val >> 15) & 0x3e) as u32; // 2-second resolution, bits 20-15
    if month == 0 || day == 0 { return None; }
    Some(format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}", year, month, day, hour, min, sec))
}

fn read_u16_le(data: &[u8], off: usize) -> Option<u16> {
    if off + 2 <= data.len() {
        Some(u16::from_le_bytes([data[off], data[off+1]]))
    } else { None }
}

fn read_u32_le(data: &[u8], off: usize) -> Option<u32> {
    if off + 4 <= data.len() {
        Some(u32::from_le_bytes([data[off], data[off+1], data[off+2], data[off+3]]))
    } else { None }
}

fn read_cstring(data: &[u8], off: usize) -> Option<String> {
    if off >= data.len() { return None; }
    let end = data[off..].iter().position(|&b| b == 0).unwrap_or(data.len() - off);
    Some(String::from_utf8_lossy(&data[off..off+end]).to_string())
}

fn read_utf16le_string(data: &[u8], off: usize) -> Option<String> {
    if off + 2 > data.len() { return None; }
    let mut chars = Vec::new();
    let mut i = off;
    while i + 1 < data.len() {
        let ch = u16::from_le_bytes([data[i], data[i+1]]);
        if ch == 0 { break; }
        chars.push(ch);
        i += 2;
    }
    Some(String::from_utf16_lossy(&chars))
}

fn flags_to_string(val: u32, bits: &[(u32, &str)]) -> String {
    let parts: Vec<&str> = bits.iter()
        .filter(|(mask, _)| val & (1 << mask) != 0)
        .map(|(_, name)| *name)
        .collect();
    if parts.is_empty() {
        format!("0x{:08x}", val)
    } else {
        parts.join(", ")
    }
}

/// Process the LNK main header
fn process_lnk_header(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 0x48 { return; }

    // Flags at 0x14
    if let Some(flags) = read_u32_le(data, 0x14) {
        let flag_bits = [
            (0u32, "IDList"), (1, "LinkInfo"), (2, "Description"), (3, "RelativePath"),
            (4, "WorkingDir"), (5, "CommandArgs"), (6, "IconFile"), (7, "Unicode"),
            (8, "NoLinkInfo"), (9, "ExpString"), (10, "SeparateProc"), (12, "DarwinID"),
            (13, "RunAsUser"), (14, "ExpIcon"), (15, "NoPidAlias"), (17, "RunWithShim"),
            (18, "NoLinkTrack"), (19, "TargetMetadata"), (20, "NoLinkPathTracking"),
            (21, "NoKnownFolderTracking"), (22, "NoKnownFolderAlias"), (23, "LinkToLink"),
            (24, "UnaliasOnSave"), (25, "PreferEnvPath"), (26, "KeepLocalIDList"),
        ];
        let s = flags_to_string(flags, &flag_bits);
        tags.push(mk_str("Flags", &s));
    }

    // FileAttributes at 0x18
    if let Some(attrs) = read_u32_le(data, 0x18) {
        let attr_bits = [
            (0u32, "Read-only"), (1, "Hidden"), (2, "System"), (4, "Directory"),
            (5, "Archive"), (7, "Normal"), (8, "Temporary"), (9, "Sparse"),
            (10, "Reparse point"), (11, "Compressed"), (12, "Offline"),
            (13, "Not indexed"), (14, "Encrypted"),
        ];
        let s = if attrs == 0 { "(none)".to_string() } else if attrs & 0x80 != 0 { "Normal".to_string() } else {
            let parts: Vec<&str> = attr_bits.iter()
                .filter(|(bit, _)| attrs & (1 << bit) != 0)
                .map(|(_, name)| *name)
                .collect();
            if parts.is_empty() { format!("0x{:08x}", attrs) } else { parts.join(", ") }
        };
        tags.push(mk_str("FileAttributes", &s));
    }

    // CreateDate at 0x1c (8 bytes, FILETIME)
    if let (Some(lo), Some(hi)) = (read_u32_le(data, 0x1c), read_u32_le(data, 0x20)) {
        if let Some(dt) = filetime_to_datetime(lo, hi, 0) {
            // ExifTool shows these with timezone +02:00 in the test, but that's system-dependent
            // For comparison we just use UTC
            tags.push(mk_str("CreateDate", &dt));
        }
    }

    // AccessDate at 0x24 (8 bytes, FILETIME)
    if let (Some(lo), Some(hi)) = (read_u32_le(data, 0x24), read_u32_le(data, 0x28)) {
        if let Some(dt) = filetime_to_datetime(lo, hi, 0) {
            tags.push(mk_str("AccessDate", &dt));
        }
    }

    // ModifyDate at 0x2c (8 bytes, FILETIME)
    if let (Some(lo), Some(hi)) = (read_u32_le(data, 0x2c), read_u32_le(data, 0x30)) {
        if let Some(dt) = filetime_to_datetime(lo, hi, 0) {
            tags.push(mk_str("ModifyDate", &dt));
        }
    }

    // TargetFileSize at 0x34
    if let Some(sz) = read_u32_le(data, 0x34) {
        tags.push(mk("TargetFileSize", Value::U32(sz)));
    }

    // IconIndex at 0x38
    if let Some(idx) = read_u32_le(data, 0x38) {
        let s = if idx == 0 { "(none)".to_string() } else { format!("{}", idx) };
        tags.push(mk_str("IconIndex", &s));
    }

    // RunWindow at 0x3c
    if let Some(rw) = read_u32_le(data, 0x3c) {
        let s = match rw {
            0 => "Hide", 1 => "Normal", 2 => "Show Minimized", 3 => "Show Maximized",
            4 => "Show No Activate", 5 => "Show", 6 => "Minimized",
            7 => "Show Minimized No Activate", 8 => "Show NA", 9 => "Restore",
            10 => "Show Default", _ => "Unknown",
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
            if hk & 0x400 != 0 { key = format!("Alt-{}", key); }
            if hk & 0x200 != 0 { key = format!("Control-{}", key); }
            if hk & 0x100 != 0 { key = format!("Shift-{}", key); }
            key
        };
        tags.push(mk_str("HotKey", &s));
    }
}

/// Process ItemID list to extract target file info
fn process_item_id(data: &[u8], tags: &mut Vec<Tag>) {
    let mut pos = 0;
    while pos + 3 < data.len() {
        let size = match read_u16_le(data, pos) {
            Some(s) => s as usize,
            None => break,
        };
        if size < 3 || pos + size > data.len() { break; }

        let item_type = data[pos + 2];
        let item_data = &data[pos..pos + size];

        match item_type {
            0x32 | 0x35 | 0x36 => {
                // File/target entry
                process_target_info(item_data, tags, item_type);
            }
            _ => {}
        }

        pos += size;
    }
}

fn process_target_info(data: &[u8], tags: &mut Vec<Tag>, item_type: u8) {
    if data.len() < 14 { return; }

    // Offset 8: TargetFileModifyDate (LNK DOS time format)
    if let Some(val) = read_u32_le(data, 8) {
        if let Some(dt) = dos_time(val) {
            tags.push(mk_str("TargetFileModifyDate", &dt));
        }
    }

    // Offset 12: TargetFileAttributes (int16u)
    if let Some(attrs) = read_u16_le(data, 12) {
        let attr_bits = [
            (0u32, "Read-only"), (1, "Hidden"), (2, "System"), (4, "Directory"),
            (5, "Archive"), (7, "Normal"),
        ];
        let s = if attrs & 0x80 != 0 { "Normal".to_string() } else {
            let parts: Vec<&str> = attr_bits.iter()
                .filter(|(bit, _)| (attrs as u32) & (1 << bit) != 0)
                .map(|(_, name)| *name)
                .collect();
            if parts.is_empty() { "(none)".to_string() } else { parts.join(", ") }
        };
        tags.push(mk_str("TargetFileAttributes", &s));
    }

    // Offset 14: TargetFileDOSName (null-terminated ASCII string)
    let dos_name = if let Some(name) = read_cstring(data, 14) { name } else { String::new() };
    if !dos_name.is_empty() {
        tags.push(mk_str("TargetFileDOSName", &dos_name));
    }

    // Find extension block start: after DOS name, aligned to 2-byte boundary
    // DOS name is at offset 14, null-terminated
    let dos_name_len = dos_name.len();
    // Position after null terminator: 14 + dos_name_len + 1
    let after_null = 14 + dos_name_len + 1;
    // Align to next 2-byte boundary
    let ext_start = (after_null + 1) & !1;

    // Extension block structure:
    // [0-1]: ext_size (int16u)
    // [2-3]: ext_version (int16u)
    // [4-5]: unknown (int16u, e.g. 0x0004)
    // [6-7]: magic = 0xBEEF (ef be in LE)
    // [8-11]: CreateDate (LNK DOSTime)
    // [12-15]: AccessDate (LNK DOSTime)
    // [16-19]: unknown
    // [20..]: Unicode filename (UTF-16LE, null-terminated)
    if ext_start + 8 <= data.len() {
        let magic = read_u16_le(data, ext_start + 6).unwrap_or(0);
        if magic == 0xbeef {
            // CreateDate
            if let Some(val) = read_u32_le(data, ext_start + 8) {
                if let Some(dt) = dos_time(val) {
                    tags.push(mk_str("TargetFileCreateDate", &dt));
                }
            }
            // AccessDate
            if ext_start + 16 <= data.len() {
                if let Some(val) = read_u32_le(data, ext_start + 12) {
                    if let Some(dt) = dos_time(val) {
                        tags.push(mk_str("TargetFileAccessDate", &dt));
                    }
                }
            }
            // Unicode TargetFileName
            let uni_off = ext_start + 20;
            if uni_off + 2 <= data.len() {
                if let Some(name) = read_utf16le_string(data, uni_off) {
                    if !name.is_empty() {
                        tags.push(mk_str("TargetFileName", &name));
                    }
                }
            }
        }
    }
    // For type 0x35 (DirInfo) and 0x36 (FileInfo2), Unicode name at offset 14
    if item_type == 0x35 || item_type == 0x36 {
        if let Some(name) = read_utf16le_string(data, 14) {
            if !name.is_empty() {
                tags.push(mk_str("TargetFileName", &name));
            }
        }
    }
}

/// Process LinkInfo structure
fn process_link_info(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 0x1c { return; }
    let hdr_len = read_u32_le(data, 4).unwrap_or(0x1c) as usize;
    let lif = read_u32_le(data, 8).unwrap_or(0);

    if lif & 0x01 != 0 {
        // Volume ID info
        let vol_off = read_u32_le(data, 0x0c).unwrap_or(0) as usize;
        if vol_off != 0 && vol_off + 0x14 <= data.len() {
            // DriveType at vol_off + 4
            if let Some(dt) = read_u32_le(data, vol_off + 4) {
                let s = match dt {
                    0 => "Unknown", 1 => "Invalid Root Path", 2 => "Removable Media",
                    3 => "Fixed Disk", 4 => "Remote Drive", 5 => "CD-ROM", 6 => "Ram Disk",
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
    if data.len() < 0x90 { return; }

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
    // FontSize at 0x20 (int16u x, then int16u y? or int32u?)
    // According to LNK spec, FontSize is int32u but ExifTool shows "10 x 20"
    // so it's likely two int16u values
    if let (Some(x), Some(y)) = (read_u16_le(data, 0x20), read_u16_le(data, 0x22)) {
        tags.push(mk_str("FontSize", &format!("{} x {}", x, y)));
    }
    // FontFamily at 0x24
    if let Some(ff) = read_u32_le(data, 0x24) {
        let s = match ff & 0xf0 {
            0x00 => "Don't Care", 0x10 => "Roman", 0x20 => "Swiss",
            0x30 => "Modern", 0x40 => "Script", 0x50 => "Decorative",
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
        let fn_data = &data[0x2c..0x2c+64];
        let chars: Vec<u16> = fn_data.chunks_exact(2)
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
        tags.push(mk_str("WindowOriginAuto", if v != 0 { "Yes" } else { "No" }));
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
        tags.push(mk_str("RemoveHistoryDuplicates", if v != 0 { "Yes" } else { "No" }));
    }
}

/// Process TrackerData extra data block
fn process_tracker_data(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 0x20 { return; }
    // MachineID at 0x10 (null-terminated string)
    if let Some(id) = read_cstring(data, 0x10) {
        if !id.is_empty() {
            tags.push(mk_str("MachineID", &id));
        }
    }
}

/// Process ConsoleFEData extra data block
fn process_console_fe_data(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 0x0c { return; }
    // CodePage at 0x08
    if let Some(cp) = read_u32_le(data, 0x08) {
        tags.push(mk("CodePage", Value::U32(cp)));
    }
}

/// Read a binary Windows .lnk (Shell Link) file
pub fn read_lnk(data: &[u8]) -> crate::error::Result<Vec<Tag>> {
    if data.len() < 0x4c { return Ok(Vec::new()); }

    // Check LNK magic: header size 0x4c, CLSID starts at offset 4
    let hdr_size = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    if hdr_size < 0x4c { return Ok(Vec::new()); }

    // Check CLSID: 01 14 02 00 00 00 00 00 C0 00 00 00 00 00 00 46
    let clsid_ok = &data[4..20] == &[0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00,
                                      0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46];
    if !clsid_ok { return Ok(Vec::new()); }

    let mut tags = Vec::new();
    let flags = read_u32_le(data, 0x14).unwrap_or(0);

    // Process header
    process_lnk_header(data, &mut tags);

    let mut pos = hdr_size;

    // IDList (flag bit 0)
    if flags & 0x01 != 0 {
        if pos + 2 > data.len() { return Ok(tags); }
        let list_len = read_u16_le(data, pos).unwrap_or(0) as usize;
        pos += 2;
        if pos + list_len > data.len() { return Ok(tags); }
        let id_data = &data[pos..pos + list_len];
        process_item_id(id_data, &mut tags);
        pos += list_len;
    }

    // LinkInfo (flag bit 1)
    if flags & 0x02 != 0 {
        if pos + 4 > data.len() { return Ok(tags); }
        let li_len = read_u32_le(data, pos).unwrap_or(0) as usize;
        if pos + li_len > data.len() { return Ok(tags); }
        let li_data = &data[pos..pos + li_len];
        process_link_info(li_data, &mut tags);
        pos += li_len;
    }

    // String data: Description, RelativePath, WorkingDirectory, CommandLineArguments, IconFileName
    let string_names = ["Description", "RelativePath", "WorkingDirectory", "CommandLineArguments", "IconFileName"];
    let string_flag_masks = [0x04u32, 0x08, 0x10, 0x20, 0x40];
    let is_unicode = (flags & 0x80) != 0;

    for (i, (&mask, &name)) in string_flag_masks.iter().zip(string_names.iter()).enumerate() {
        if flags & mask == 0 { continue; }
        if pos + 2 > data.len() { break; }
        let char_count = read_u16_le(data, pos).unwrap_or(0) as usize;
        pos += 2;
        if char_count == 0 { continue; }
        // Limit description length to 260 chars (except CommandLineArguments)
        let limit = if i != 3 { 260 } else { usize::MAX };
        let actual_count = char_count.min(limit);
        let byte_len = if is_unicode { actual_count * 2 } else { actual_count };
        if pos + byte_len > data.len() { break; }
        let s = if is_unicode {
            let chars: Vec<u16> = data[pos..pos + byte_len].chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect();
            String::from_utf16_lossy(&chars).to_string()
        } else {
            String::from_utf8_lossy(&data[pos..pos + byte_len]).to_string()
        };
        let full_byte_len = if is_unicode { char_count * 2 } else { char_count };
        pos += full_byte_len.min(data.len() - pos);
        if !s.is_empty() {
            tags.push(mk_str(name, &s));
        }
    }

    // Extra data blocks
    while pos + 4 <= data.len() {
        let block_len = read_u32_le(data, pos).unwrap_or(0) as usize;
        if block_len < 4 { break; }
        if pos + block_len > data.len() { break; }
        let block_data = &data[pos..pos + block_len];
        if block_data.len() < 8 { pos += block_len; continue; }
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
        let clsid_ok = data.len() >= 20 && &data[4..20] == &[0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00,
                                                                0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46];
        if clsid_ok {
            return read_lnk(data);
        }
    }

    let text = String::from_utf8_lossy(data);
    let mut tags = Vec::new();

    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.starts_with('[') { continue; }
        if let Some(eq) = line.find('=') {
            let key = &line[..eq];
            let val = &line[eq + 1..];
            match key {
                "URL" | "IconFile" | "IconIndex" | "WorkingDirectory" | "HotKey" |
                "Author" | "WhatsNew" | "Comment" | "Desc" | "Roamed" | "IDList" => {
                    tags.push(mk_str(key, val));
                }
                "Modified" => {
                    // Hex-encoded 8-byte FILETIME (little-endian uint32 lo + hi)
                    let hex = val.trim();
                    if hex.len() >= 16 {
                        let bytes: Vec<u8> = (0..16).step_by(2)
                            .filter_map(|i| u8::from_str_radix(&hex[i..i+2], 16).ok())
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
                        "1" => "Normal", "2" => "Minimized", "3" => "Maximized", v => v,
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
