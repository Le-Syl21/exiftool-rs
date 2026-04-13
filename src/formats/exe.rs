//! Executable file reader (PE/ELF/Mach-O).
//!
//! Extracts basic header info from executable files.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_exe(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 4 {
        return Err(Error::InvalidData("file too small".into()));
    }

    // PE (MZ header)
    if data.starts_with(b"MZ") {
        return read_pe(data);
    }
    // ELF
    if data.starts_with(&[0x7F, b'E', b'L', b'F']) {
        return read_elf(data);
    }
    // Mach-O (32-bit)
    if data.starts_with(&[0xFE, 0xED, 0xFA, 0xCE]) || data.starts_with(&[0xCE, 0xFA, 0xED, 0xFE]) {
        return read_macho(data, false);
    }
    // Mach-O (64-bit)
    if data.starts_with(&[0xFE, 0xED, 0xFA, 0xCF]) || data.starts_with(&[0xCF, 0xFA, 0xED, 0xFE]) {
        return read_macho(data, true);
    }
    // Mach-O Universal/Fat
    if data.starts_with(&[0xCA, 0xFE, 0xBA, 0xBE]) {
        let mut tags = Vec::new();
        tags.push(mk(
            "ExeType",
            "Executable Type",
            Value::String("Mach-O Universal Binary".into()),
        ));
        if data.len() >= 8 {
            let num_arch = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
            tags.push(mk(
                "NumArchitectures",
                "Architectures",
                Value::U32(num_arch),
            ));
        }
        return Ok(tags);
    }
    // AR archive (.a files)
    if data.starts_with(b"!<arch>\n") {
        return read_ar(data);
    }

    Err(Error::InvalidData("unknown executable format".into()))
}

/// Parse AR (static library) archive. Extract CreateDate from first entry header
/// and Mach-O CPU tags from the first Mach-O member found.
/// Mirrors ExifTool's EXE.pm ProcessEXE handling of !<arch> files.
fn read_ar(data: &[u8]) -> Result<Vec<Tag>> {
    let mut tags = Vec::new();
    let mut pos = 8; // skip "!<arch>\n"
    let mut first_entry = true;
    let max_entries = 10;
    let mut entries_checked = 0;

    while entries_checked < max_entries && pos + 60 <= data.len() {
        // AR entry header: 60 bytes total
        // ar_name[16], ar_date[12], ar_uid[6], ar_gid[6], ar_mode[8], ar_size[10], terminator[2]
        let entry = &data[pos..pos + 60];
        if &entry[58..60] != b"`\n" {
            break;
        }

        let ar_size_str = std::str::from_utf8(&entry[48..58]).unwrap_or("").trim();
        let ar_size: usize = ar_size_str.parse().unwrap_or(0);
        let data_start = pos + 60;

        if first_entry {
            // Extract CreateDate from ar_date field (Unix timestamp string)
            let date_str = std::str::from_utf8(&entry[16..28])
                .unwrap_or("")
                .trim()
                .to_string();
            if let Ok(unix_ts) = date_str.parse::<i64>() {
                // Convert Unix timestamp to ExifTool date format
                let date = unix_to_exif_date(unix_ts);
                tags.push(mk("CreateDate", "Create Date", Value::String(date)));
            }
            first_entry = false;
        }

        // Determine actual data offset (BSD extended names: #1/N)
        let ar_name = std::str::from_utf8(&entry[0..16]).unwrap_or("").trim();
        let name_ext_len: usize = if let Some(rest) = ar_name.strip_prefix("#1/") {
            rest.trim().parse().unwrap_or(0)
        } else {
            0
        };
        let member_data_start = data_start + name_ext_len;

        // Try to extract Mach-O tags from this member
        if member_data_start + 4 <= data.len() {
            let member = &data[member_data_start..];
            let is_macho = member.starts_with(&[0xFE, 0xED, 0xFA, 0xCE])
                || member.starts_with(&[0xCE, 0xFA, 0xED, 0xFE])
                || member.starts_with(&[0xFE, 0xED, 0xFA, 0xCF])
                || member.starts_with(&[0xCF, 0xFA, 0xED, 0xFE]);
            if is_macho {
                let is_64 = member[3] == 0xCF || member[0] == 0xCF;
                if let Ok(mut macho_tags) = read_macho(member, is_64) {
                    // Remove ExeType if present (static lib doesn't need it)
                    macho_tags.retain(|t| {
                        t.name != "ExeType" && t.name != "ObjectFileType" && t.name != "ObjectFlags"
                    });
                    tags.extend(macho_tags);
                }
                break; // got what we need
            }
        }

        pos += 60 + ar_size;
        if pos & 1 != 0 {
            pos += 1; // align to even boundary
        }
        entries_checked += 1;
    }

    Ok(tags)
}

/// Convert Unix timestamp to ExifTool date format (YYYY:MM:DD HH:MM:SS+TZ)
fn unix_to_exif_date(ts: i64) -> String {
    // Simple Unix timestamp to date conversion (no external crate)
    // Using basic epoch calculation
    let secs_per_minute = 60i64;
    let secs_per_hour = 3600i64;
    let secs_per_day = 86400i64;

    // Days since epoch
    let mut days = ts / secs_per_day;
    let time_of_day = ts % secs_per_day;
    let hour = time_of_day / secs_per_hour;
    let minute = (time_of_day % secs_per_hour) / secs_per_minute;
    let second = time_of_day % secs_per_minute;

    // Calculate year/month/day from days since 1970-01-01
    let mut year = 1970i32;
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let leap = is_leap_year(year);
    let month_days = [
        31i64,
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
    let mut month = 1i32;
    for &md in &month_days {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }
    let day = days + 1;
    format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}+00:00",
        year, month, day, hour, minute, second
    )
}

fn is_leap_year(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn read_pe(data: &[u8]) -> Result<Vec<Tag>> {
    let mut tags = Vec::new();

    if data.len() < 64 {
        return Ok(tags);
    }

    // PE header offset at 0x3C
    let pe_offset = u32::from_le_bytes([data[0x3C], data[0x3D], data[0x3E], data[0x3F]]) as usize;
    if pe_offset + 24 > data.len() || &data[pe_offset..pe_offset + 4] != b"PE\0\0" {
        return Ok(tags);
    }

    let coff = &data[pe_offset + 4..];
    if coff.len() < 20 {
        return Ok(tags);
    }

    let machine = u16::from_le_bytes([coff[0], coff[1]]);
    let machine_str = match machine {
        0x0 => "Unknown",
        0x014c => "Intel 386 or later, and compatibles",
        0x014d => "Intel i860",
        0x0162 => "MIPS R3000",
        0x0166 => "MIPS little endian (R4000)",
        0x0168 => "MIPS R10000",
        0x0169 => "MIPS little endian WCI v2",
        0x0183 => "Alpha AXP (old)",
        0x0184 => "Alpha AXP",
        0x01a2 => "Hitachi SH3",
        0x01a6 => "Hitachi SH4",
        0x01a8 => "Hitachi SH5",
        0x01c0 => "ARM little endian",
        0x01c2 => "Thumb",
        0x01c4 => "Thumb 2 little endian",
        0x01f0 => "PowerPC little endian",
        0x01f1 => "PowerPC with floating point support",
        0x0200 => "Intel IA64",
        0x0266 => "MIPS16",
        0x0268 => "Motorola 68000 series",
        0x0284 => "Alpha AXP 64-bit",
        0x0366 => "MIPS with FPU",
        0x0466 => "MIPS16 with FPU",
        0x5032 => "RISC-V 32-bit",
        0x5064 => "RISC-V 64-bit",
        0x5128 => "RISC-V 128-bit",
        0x8664 => "AMD AMD64",
        0xaa64 => "ARM64 little endian",
        _ => "Unknown",
    };
    tags.push(mk(
        "MachineType",
        "Machine Type",
        Value::String(machine_str.into()),
    ));

    let num_sections = u16::from_le_bytes([coff[2], coff[3]]);
    let chars = u16::from_le_bytes([coff[18], coff[19]]);

    // Determine ExeType but don't emit as tag (Perl sets FileType instead)

    // TimeDateStamp
    let timestamp = u32::from_le_bytes([coff[4], coff[5], coff[6], coff[7]]);
    if timestamp > 0 {
        // Convert Unix timestamp to ExifTool date format
        let date = unix_to_exif_date(timestamp as i64);
        tags.push(mk("TimeStamp", "Time Stamp", Value::String(date)));
    }

    // ImageFileCharacteristics bitmask
    let chars_str = pe_image_file_chars(chars);
    tags.push(mk(
        "ImageFileCharacteristics",
        "Image File Characteristics",
        Value::String(chars_str),
    ));

    // Optional header starts at coff[20]
    let opt_size = u16::from_le_bytes([coff[16], coff[17]]) as usize;
    if coff.len() < 20 + opt_size {
        return Ok(tags);
    }

    let opt = &coff[20..20 + opt_size];
    if opt.len() < 2 {
        return Ok(tags);
    }

    let opt_magic = u16::from_le_bytes([opt[0], opt[1]]);
    let pe_type_str = match opt_magic {
        0x107 => "ROM Image",
        0x10b => "PE32",
        0x20b => "PE32+",
        _ => "Unknown",
    };
    tags.push(mk("PEType", "PE Type", Value::String(pe_type_str.into())));

    if opt.len() >= 4 {
        let linker_maj = opt[2];
        let linker_min = opt[3];
        tags.push(mk(
            "LinkerVersion",
            "Linker Version",
            Value::String(format!("{}.{}", linker_maj, linker_min)),
        ));
    }

    if opt.len() >= 12 {
        let code_size = u32::from_le_bytes([opt[4], opt[5], opt[6], opt[7]]);
        tags.push(mk("CodeSize", "Code Size", Value::U32(code_size)));
        let init_data = u32::from_le_bytes([opt[8], opt[9], opt[10], opt[11]]);
        tags.push(mk(
            "InitializedDataSize",
            "Initialized Data Size",
            Value::U32(init_data),
        ));
    }

    if opt.len() >= 20 {
        let uninit_data = u32::from_le_bytes([opt[12], opt[13], opt[14], opt[15]]);
        tags.push(mk(
            "UninitializedDataSize",
            "Uninitialized Data Size",
            Value::U32(uninit_data),
        ));
        let entry_point = u32::from_le_bytes([opt[16], opt[17], opt[18], opt[19]]);
        tags.push(mk(
            "EntryPoint",
            "Entry Point",
            Value::String(format!("0x{:x}", entry_point)),
        ));
    }

    if opt.len() >= 48 {
        let os_maj = u16::from_le_bytes([opt[40], opt[41]]);
        let os_min = u16::from_le_bytes([opt[42], opt[43]]);
        tags.push(mk(
            "OSVersion",
            "OS Version",
            Value::String(format!("{}.{}", os_maj, os_min)),
        ));
        let img_maj = u16::from_le_bytes([opt[44], opt[45]]);
        let img_min = u16::from_le_bytes([opt[46], opt[47]]);
        tags.push(mk(
            "ImageVersion",
            "Image Version",
            Value::String(format!("{}.{}", img_maj, img_min)),
        ));
    }

    if opt.len() >= 56 {
        let ss_maj = u16::from_le_bytes([opt[48], opt[49]]);
        let ss_min = u16::from_le_bytes([opt[50], opt[51]]);
        tags.push(mk(
            "SubsystemVersion",
            "Subsystem Version",
            Value::String(format!("{}.{}", ss_maj, ss_min)),
        ));
    }

    if opt.len() >= 72 {
        let subsystem = u16::from_le_bytes([opt[68], opt[69]]);
        let subsystem_str = match subsystem {
            0 => "Unknown",
            1 => "Native",
            2 => "Windows GUI",
            3 => "Windows command line",
            5 => "OS/2 command line",
            7 => "POSIX command line",
            9 => "Windows CE GUI",
            10 => "EFI application",
            11 => "EFI boot service",
            12 => "EFI runtime driver",
            13 => "EFI ROM",
            14 => "XBOX",
            _ => "Unknown",
        };
        tags.push(mk(
            "Subsystem",
            "Subsystem",
            Value::String(subsystem_str.into()),
        ));
    }

    // Parse sections to find .rsrc
    let sections_offset = pe_offset + 4 + 20 + opt_size;
    let sections_end = sections_offset + num_sections as usize * 40;
    if sections_end > data.len() {
        return Ok(tags);
    }

    // Build section table for VA->offset conversion
    let mut sections: Vec<(u32, u32, u32)> = Vec::new(); // (va, size, raw_offset)
    let mut rsrc_va: u32 = 0;
    let mut rsrc_raw: u32 = 0;
    let mut rsrc_size: u32 = 0;

    for i in 0..num_sections as usize {
        let s = sections_offset + i * 40;
        let va = u32::from_le_bytes([data[s + 12], data[s + 13], data[s + 14], data[s + 15]]);
        let size = u32::from_le_bytes([data[s + 16], data[s + 17], data[s + 18], data[s + 19]]);
        let raw = u32::from_le_bytes([data[s + 20], data[s + 21], data[s + 22], data[s + 23]]);
        sections.push((va, size, raw));

        let sname = &data[s..s + 8];
        if sname.starts_with(b".rsrc") {
            rsrc_va = va;
            rsrc_raw = raw;
            rsrc_size = size;
        }
    }

    if rsrc_raw == 0 || rsrc_size == 0 {
        return Ok(tags);
    }

    // Parse VS_VERSION_INFO from .rsrc section
    let rsrc_off = rsrc_raw as usize;
    let rsrc_end = (rsrc_off + rsrc_size as usize).min(data.len());
    if rsrc_off >= data.len() {
        return Ok(tags);
    }

    let rsrc = &data[rsrc_off..rsrc_end];

    // Find Version resource (type 16 = RT_VERSION)
    if let Some(vi_raw_off) = find_version_resource(rsrc, rsrc_va, &sections, data) {
        parse_vs_version_info(data, vi_raw_off, &mut tags);
    }

    Ok(tags)
}

fn va_to_offset(va: u32, sections: &[(u32, u32, u32)]) -> Option<usize> {
    for &(s_va, s_size, s_raw) in sections {
        if va >= s_va && va < s_va + s_size {
            return Some((va - s_va + s_raw) as usize);
        }
    }
    None
}

/// Find the raw file offset of the Version resource data
fn find_version_resource(
    rsrc: &[u8],
    _rsrc_va: u32,
    sections: &[(u32, u32, u32)],
    data: &[u8],
) -> Option<usize> {
    // Resource directory: 16-byte header + entries
    // entry: u32 name, u32 offset_or_data
    if rsrc.len() < 16 {
        return None;
    }
    let named = u16::from_le_bytes([rsrc[12], rsrc[13]]) as usize;
    let id_count = u16::from_le_bytes([rsrc[14], rsrc[15]]) as usize;
    let total = named + id_count;

    // Level 0: find RT_VERSION (type 16)
    for i in 0..total {
        let entry_off = 16 + i * 8;
        if entry_off + 8 > rsrc.len() {
            break;
        }
        let name = u32::from_le_bytes([
            rsrc[entry_off],
            rsrc[entry_off + 1],
            rsrc[entry_off + 2],
            rsrc[entry_off + 3],
        ]);
        let ptr = u32::from_le_bytes([
            rsrc[entry_off + 4],
            rsrc[entry_off + 5],
            rsrc[entry_off + 6],
            rsrc[entry_off + 7],
        ]);

        if name != 16 {
            continue; // not RT_VERSION
        }

        // Level 1: version directory
        if ptr & 0x80000000 == 0 {
            continue;
        }
        let l1_off = (ptr & 0x7fffffff) as usize;
        if l1_off + 16 > rsrc.len() {
            continue;
        }
        let l1_named = u16::from_le_bytes([rsrc[l1_off + 12], rsrc[l1_off + 13]]) as usize;
        let l1_id = u16::from_le_bytes([rsrc[l1_off + 14], rsrc[l1_off + 15]]) as usize;
        let l1_total = l1_named + l1_id;

        for j in 0..l1_total {
            let e1 = l1_off + 16 + j * 8;
            if e1 + 8 > rsrc.len() {
                break;
            }
            let ptr1 = u32::from_le_bytes([rsrc[e1 + 4], rsrc[e1 + 5], rsrc[e1 + 6], rsrc[e1 + 7]]);

            // Level 2: language directory
            if ptr1 & 0x80000000 == 0 {
                continue;
            }
            let l2_off = (ptr1 & 0x7fffffff) as usize;
            if l2_off + 16 > rsrc.len() {
                continue;
            }
            let l2_named = u16::from_le_bytes([rsrc[l2_off + 12], rsrc[l2_off + 13]]) as usize;
            let l2_id = u16::from_le_bytes([rsrc[l2_off + 14], rsrc[l2_off + 15]]) as usize;
            let l2_total = l2_named + l2_id;

            for k in 0..l2_total {
                let e2 = l2_off + 16 + k * 8;
                if e2 + 8 > rsrc.len() {
                    break;
                }
                let ptr2 =
                    u32::from_le_bytes([rsrc[e2 + 4], rsrc[e2 + 5], rsrc[e2 + 6], rsrc[e2 + 7]]);

                if ptr2 & 0x80000000 != 0 {
                    continue; // should be a leaf
                }

                // ptr2 points to RESOURCE_DATA_ENTRY within rsrc
                let data_entry_off = ptr2 as usize;
                if data_entry_off + 16 > rsrc.len() {
                    continue;
                }
                let data_va = u32::from_le_bytes([
                    rsrc[data_entry_off],
                    rsrc[data_entry_off + 1],
                    rsrc[data_entry_off + 2],
                    rsrc[data_entry_off + 3],
                ]);
                // Convert VA to file offset
                if let Some(file_off) = va_to_offset(data_va, sections) {
                    if file_off < data.len() {
                        return Some(file_off);
                    }
                }
            }
        }
    }
    None
}

/// Read a null-terminated UTF-16LE string, return (string, new_pos rounded to 4 bytes)
fn read_utf16_str(data: &[u8], start: usize, end: usize) -> (String, usize) {
    let mut pos = start;
    let mut chars: Vec<u16> = Vec::new();
    while pos + 2 <= end {
        let ch = u16::from_le_bytes([data[pos], data[pos + 1]]);
        pos += 2;
        if ch == 0 {
            break;
        }
        chars.push(ch);
    }
    // Round up to 4-byte boundary
    if pos & 3 != 0 {
        pos = (pos + 3) & !3;
    }
    (String::from_utf16_lossy(&chars), pos)
}

/// Parse VS_VERSION_INFO block
fn parse_vs_version_info(data: &[u8], start: usize, tags: &mut Vec<Tag>) {
    if start + 6 > data.len() {
        return;
    }
    let total_len = u16::from_le_bytes([data[start], data[start + 1]]) as usize;
    let val_len = u16::from_le_bytes([data[start + 2], data[start + 3]]) as usize;
    let _typ = u16::from_le_bytes([data[start + 4], data[start + 5]]);

    let end = (start + total_len).min(data.len());

    // Read key (should be "VS_VERSION_INFO")
    let (key, key_end) = read_utf16_str(data, start + 6, end);
    if key != "VS_VERSION_INFO" {
        return;
    }

    // Fixed version info (VS_FIXEDFILEINFO) at key_end, size = val_len
    if val_len >= 52 && key_end + 52 <= end {
        let ffi = &data[key_end..key_end + val_len];
        // Verify signature
        let sig = u32::from_le_bytes([ffi[0], ffi[1], ffi[2], ffi[3]]);
        if sig == 0xFEEF04BD {
            // FileVersionMS/LS at offset 8
            let fv_ms = u32::from_le_bytes([ffi[8], ffi[9], ffi[10], ffi[11]]);
            let fv_ls = u32::from_le_bytes([ffi[12], ffi[13], ffi[14], ffi[15]]);
            let file_ver = format!(
                "{}.{}.{}.{}",
                fv_ms >> 16,
                fv_ms & 0xffff,
                fv_ls >> 16,
                fv_ls & 0xffff
            );
            tags.push(mk(
                "FileVersionNumber",
                "File Version Number",
                Value::String(file_ver),
            ));

            // ProductVersionMS/LS at offset 16
            let pv_ms = u32::from_le_bytes([ffi[16], ffi[17], ffi[18], ffi[19]]);
            let pv_ls = u32::from_le_bytes([ffi[20], ffi[21], ffi[22], ffi[23]]);
            let prod_ver = format!(
                "{}.{}.{}.{}",
                pv_ms >> 16,
                pv_ms & 0xffff,
                pv_ls >> 16,
                pv_ls & 0xffff
            );
            tags.push(mk(
                "ProductVersionNumber",
                "Product Version Number",
                Value::String(prod_ver),
            ));

            // FileFlagsMask at offset 24
            let ff_mask = u32::from_le_bytes([ffi[24], ffi[25], ffi[26], ffi[27]]);
            tags.push(mk(
                "FileFlagsMask",
                "File Flags Mask",
                Value::String(format!("0x{:04x}", ff_mask)),
            ));

            // FileFlags at offset 28
            let ff = u32::from_le_bytes([ffi[28], ffi[29], ffi[30], ffi[31]]);
            let ff_str = pe_file_flags(ff as u16);
            tags.push(mk("FileFlags", "File Flags", Value::String(ff_str)));

            // FileOS at offset 32
            let fos = u32::from_le_bytes([ffi[32], ffi[33], ffi[34], ffi[35]]);
            let fos_str = match fos {
                0x00001 => "Win16",
                0x00002 => "PM-16",
                0x00003 => "PM-32",
                0x00004 => "Win32",
                0x10000 => "DOS",
                0x20000 => "OS/2 16-bit",
                0x30000 => "OS/2 32-bit",
                0x40000 => "Windows NT",
                0x10001 => "Windows 16-bit",
                0x10004 => "Windows 32-bit",
                0x20002 => "OS/2 16-bit PM-16",
                0x30003 => "OS/2 32-bit PM-32",
                0x40004 => "Windows NT 32-bit",
                _ => "",
            };
            if !fos_str.is_empty() {
                tags.push(mk("FileOS", "File OS", Value::String(fos_str.into())));
            } else {
                tags.push(mk(
                    "FileOS",
                    "File OS",
                    Value::String(format!("0x{:04x}", fos)),
                ));
            }

            // ObjectFileType at offset 36
            let oft = u32::from_le_bytes([ffi[36], ffi[37], ffi[38], ffi[39]]);
            let oft_str = match oft {
                0 => "Unknown",
                1 => "Executable application",
                2 => "Dynamic link library",
                3 => "Driver",
                4 => "Font",
                5 => "VxD",
                7 => "Static library",
                _ => "",
            };
            if !oft_str.is_empty() {
                tags.push(mk(
                    "ObjectFileType",
                    "Object File Type",
                    Value::String(oft_str.into()),
                ));
            }

            // FileSubtype at offset 40
            let fst = u32::from_le_bytes([ffi[40], ffi[41], ffi[42], ffi[43]]);
            tags.push(mk("FileSubtype", "File Subtype", Value::U32(fst)));
        }
    }

    // Walk StringFileInfo children
    let mut pos = key_end + val_len;
    pos = (pos + 3) & !3; // align to 4 bytes
    while pos + 6 <= end {
        let child_len = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        let child_val_len = u16::from_le_bytes([data[pos + 2], data[pos + 3]]) as usize;
        let child_end = (pos + child_len).min(end);

        let (child_key, child_key_end) = read_utf16_str(data, pos + 6, child_end);

        if child_key == "StringFileInfo" && child_val_len == 0 {
            parse_string_file_info(data, child_key_end, child_end, tags);
        }

        pos += child_len;
        pos = (pos + 3) & !3;
        if child_len == 0 {
            break;
        }
    }
}

/// Parse StringFileInfo block (contains StringTable)
fn parse_string_file_info(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>) {
    let pos = start;

    // First entry is the StringTable
    if pos + 6 > end {
        return;
    }
    let st_len = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
    if st_len == 0 {
        return;
    }
    let st_end = (pos + st_len).min(end);

    // Read language/charset key (e.g. "040904B0")
    let (lang_char, key_end) = read_utf16_str(data, pos + 6, st_end);

    // Extract LanguageCode and CharacterSet
    if lang_char.len() >= 4 {
        let lang_code = &lang_char[..4];
        let char_set = if lang_char.len() > 4 {
            &lang_char[4..]
        } else {
            ""
        };

        let lang_name = match lang_code.to_uppercase().as_str() {
            "0409" => "English (U.S.)",
            "0407" => "German",
            "040C" => "French",
            "0410" => "Italian",
            "0411" => "Japanese",
            "0412" => "Korean",
            "0413" => "Dutch",
            "0416" => "Portuguese (Brazilian)",
            "0419" => "Russian",
            "040A" => "Spanish (Castilian)",
            "0404" => "Chinese (Traditional)",
            "0804" => "Chinese (Simplified)",
            "0816" => "Portuguese (Standard)",
            "040E" => "Hungarian",
            "0415" => "Polish",
            "0405" => "Czech",
            "0406" => "Danish",
            "040B" => "Finnish",
            "040F" => "Icelandic",
            "0414" => "Norwegian (Bokmal)",
            "041D" => "Swedish",
            "1009" => "English (Canadian)",
            _ => lang_code,
        };
        tags.push(mk(
            "LanguageCode",
            "Language Code",
            Value::String(lang_name.to_string()),
        ));

        if !char_set.is_empty() {
            let cs_name = match char_set.to_uppercase().as_str() {
                "0000" => "ASCII",
                "03A4" => "Windows, Japan (Shift - JIS X-0208)",
                "03A8" => "Windows, Chinese (Simplified)",
                "03B5" => "Windows, Korea (Shift - KSC 5601)",
                "03B6" => "Windows, Taiwan (Big5)",
                "04B0" => "Unicode",
                "04E2" => "Windows, Latin2 (Eastern European)",
                "04E3" => "Windows, Cyrillic",
                "04E4" => "Windows, Latin1",
                "04E5" => "Windows, Greek",
                "04E6" => "Windows, Turkish",
                "04E7" => "Windows, Hebrew",
                "04E8" => "Windows, Arabic",
                _ => char_set,
            };
            tags.push(mk(
                "CharacterSet",
                "Character Set",
                Value::String(cs_name.to_string()),
            ));
        }
    }

    // Parse string entries
    let mut spos = key_end;
    while spos + 6 <= st_end {
        let s_len = u16::from_le_bytes([data[spos], data[spos + 1]]) as usize;
        let s_val_len = u16::from_le_bytes([data[spos + 2], data[spos + 3]]) as usize;
        if s_len == 0 {
            break;
        }
        let s_end = (spos + s_len).min(st_end);

        let (s_key, s_key_end) = read_utf16_str(data, spos + 6, s_end);

        let s_val = if s_val_len > 0 && s_key_end < s_end {
            let (v, _) = read_utf16_str(data, s_key_end, s_end);
            v
        } else {
            String::new()
        };

        // Map tag names to ExifTool names
        let tag_name = match s_key.as_str() {
            "OriginalFilename" => "OriginalFileName",
            other => other,
        };

        if !tag_name.is_empty() {
            tags.push(mk(tag_name, tag_name, Value::String(s_val)));
        }

        spos += s_len;
        spos = (spos + 3) & !3;
    }
}

fn pe_image_file_chars(flags: u16) -> String {
    let mut parts = Vec::new();
    if flags & (1 << 0) != 0 {
        parts.push("No relocs");
    }
    if flags & (1 << 1) != 0 {
        parts.push("Executable");
    }
    if flags & (1 << 2) != 0 {
        parts.push("No line numbers");
    }
    if flags & (1 << 3) != 0 {
        parts.push("No symbols");
    }
    if flags & (1 << 4) != 0 {
        parts.push("Aggressive working-set trim");
    }
    if flags & (1 << 5) != 0 {
        parts.push("Large address aware");
    }
    if flags & (1 << 7) != 0 {
        parts.push("Bytes reversed lo");
    }
    if flags & (1 << 8) != 0 {
        parts.push("32-bit");
    }
    if flags & (1 << 9) != 0 {
        parts.push("No debug");
    }
    if flags & (1 << 10) != 0 {
        parts.push("Removable run from swap");
    }
    if flags & (1 << 11) != 0 {
        parts.push("Net run from swap");
    }
    if flags & (1 << 12) != 0 {
        parts.push("System file");
    }
    if flags & (1 << 13) != 0 {
        parts.push("DLL");
    }
    if flags & (1 << 14) != 0 {
        parts.push("Uniprocessor only");
    }
    if flags & (1 << 15) != 0 {
        parts.push("Bytes reversed hi");
    }
    parts.join(", ")
}

fn pe_file_flags(flags: u16) -> String {
    let mut parts = Vec::new();
    if flags & (1 << 0) != 0 {
        parts.push("Debug");
    }
    if flags & (1 << 1) != 0 {
        parts.push("Pre-release");
    }
    if flags & (1 << 2) != 0 {
        parts.push("Patched");
    }
    if flags & (1 << 3) != 0 {
        parts.push("Private build");
    }
    if flags & (1 << 4) != 0 {
        parts.push("Info inferred");
    }
    if flags & (1 << 5) != 0 {
        parts.push("Special build");
    }
    if parts.is_empty() {
        "(none)".into()
    } else {
        parts.join(", ")
    }
}

fn read_elf(data: &[u8]) -> Result<Vec<Tag>> {
    let mut tags = Vec::new();

    if data.len() < 20 {
        return Ok(tags);
    }

    let class = match data[4] {
        1 => "32-bit",
        2 => "64-bit",
        _ => "Unknown",
    };
    let endian = match data[5] {
        1 => "Little-endian",
        2 => "Big-endian",
        _ => "Unknown",
    };
    let _os_abi = match data[7] {
        0 => "UNIX System V",
        3 => "Linux",
        6 => "Solaris",
        9 => "FreeBSD",
        _ => "Other",
    };

    tags.push(mk(
        "CPUType",
        "CPU Type",
        Value::String(format!("ELF {}", class)),
    ));
    tags.push(mk(
        "CPUByteOrder",
        "CPU Byte Order",
        Value::String(endian.into()),
    ));

    let is_le = data[5] == 1;
    let elf_type = if is_le {
        u16::from_le_bytes([data[16], data[17]])
    } else {
        u16::from_be_bytes([data[16], data[17]])
    };
    let type_str = match elf_type {
        1 => "Relocatable",
        2 => "Executable",
        3 => "Shared Object",
        4 => "Core Dump",
        _ => "Unknown",
    };
    tags.push(mk(
        "ObjectFileType",
        "Object File Type",
        Value::String(type_str.into()),
    ));

    let machine = if is_le {
        u16::from_le_bytes([data[18], data[19]])
    } else {
        u16::from_be_bytes([data[18], data[19]])
    };
    let machine_str = match machine {
        3 => "Intel 386",
        8 => "MIPS",
        20 => "PowerPC",
        40 => "ARM",
        62 => "AMD64",
        183 => "ARM64",
        _ => "Unknown",
    };
    tags.push(mk(
        "CPUArchitecture",
        "CPU Architecture",
        Value::String(machine_str.into()),
    ));

    Ok(tags)
}

fn read_macho(data: &[u8], is_64: bool) -> Result<Vec<Tag>> {
    let mut tags = Vec::new();

    // Determine byte order and bit depth from magic number
    // \xFE\xED\xFA\xCE = 32-bit big endian
    // \xCE\xFA\xED\xFE = 32-bit little endian
    // \xFE\xED\xFA\xCF = 64-bit big endian
    // \xCF\xFA\xED\xFE = 64-bit little endian
    let is_le = data[0] == 0xCE || data[0] == 0xCF;
    let arch_str = if is_64 { "64 bit" } else { "32 bit" };
    let order_str = if is_le { "Little endian" } else { "Big endian" };

    tags.push(mk(
        "CPUArchitecture",
        "CPU Architecture",
        Value::String(arch_str.into()),
    ));
    tags.push(mk(
        "CPUByteOrder",
        "CPU Byte Order",
        Value::String(order_str.into()),
    ));

    if data.len() < 28 {
        return Ok(tags);
    }

    // Mach header layout:
    //  0: magic (4 bytes)
    //  4: cputype (int32s)
    //  8: cpusubtype (int32s)
    // 12: filetype (int32u)
    // 16: ncmds (int32u)
    // 20: sizeofcmds (int32u)
    // 24: flags (int32u)

    let read_u32 = |offset: usize| -> u32 {
        if is_le {
            u32::from_le_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ])
        } else {
            u32::from_be_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ])
        }
    };
    let read_i32 = |offset: usize| -> i32 { read_u32(offset) as i32 };

    let cpu_type = read_i32(4);
    let cpu_subtype = read_i32(8);
    let file_type = read_u32(12);
    let flags = read_u32(24);

    // CPUType: strip 64-bit flag (0x1000000) for lookup, add "64-bit" suffix
    let cpu_base = cpu_type & 0x00FFFFFF;
    let has_64_flag = (cpu_type as u32) & 0x01000000 != 0;
    let cpu_name = match cpu_base {
        0xFFFFFF => "Any", // -1 & 0xFFFFFF
        1 => "VAX",
        2 => "ROMP",
        4 => "NS32032",
        5 => "NS32332",
        6 => "MC680x0",
        7 => "x86",
        8 => "MIPS",
        9 => "NS32532",
        10 => "MC98000",
        11 => "HPPA",
        12 => "ARM",
        13 => "MC88000",
        14 => "SPARC",
        15 => "i860 big endian",
        16 => "i860 little endian",
        17 => "RS6000",
        18 => "PowerPC",
        255 => "VEO",
        _ => "Unknown",
    };
    let cpu_type_str = if has_64_flag {
        format!("{} 64-bit", cpu_name)
    } else {
        cpu_name.to_string()
    };
    tags.push(mk("CPUType", "CPU Type", Value::String(cpu_type_str)));

    // CPUSubtype: lookup by "cputype subtype" key, adding "64-bit" suffix if high bit set
    let sub_base = cpu_subtype & 0x7FFFFFFF;
    let sub_has_64 = (cpu_subtype as u32) & 0x80000000 != 0;
    let lookup_key = format!("{} {}", cpu_base, sub_base);
    let subtype_name = macho_cpu_subtype(&lookup_key);
    let cpu_subtype_str = if sub_has_64 || (has_64_flag && !subtype_name.is_empty()) {
        format!(
            "{} 64-bit",
            if subtype_name.is_empty() {
                format!("Unknown ({} {})", cpu_type, cpu_subtype)
            } else {
                subtype_name.to_string()
            }
        )
    } else if subtype_name.is_empty() {
        format!("Unknown ({} {})", cpu_type, cpu_subtype)
    } else {
        subtype_name.to_string()
    };
    tags.push(mk(
        "CPUSubtype",
        "CPU Subtype",
        Value::String(cpu_subtype_str),
    ));

    // ObjectFileType
    let obj_type_str = match file_type {
        1 => "Relocatable object",
        2 => "Demand paged executable",
        3 => "Fixed VM shared library",
        4 => "Core",
        5 => "Preloaded executable",
        6 => "Dynamically bound shared library",
        7 => "Dynamic link editor",
        8 => "Dynamically bound bundle",
        9 => "Shared library stub for static linking",
        10 => "Debug information",
        11 => "x86_64 kexts",
        _ => "Unknown",
    };
    tags.push(mk(
        "ObjectFileType",
        "Object File Type",
        Value::String(obj_type_str.into()),
    ));

    // ObjectFlags: bitmask decoding
    let flag_names = [
        (0, "No undefs"),
        (1, "Incrementa link"),
        (2, "Dyld link"),
        (3, "Bind at load"),
        (4, "Prebound"),
        (5, "Split segs"),
        (6, "Lazy init"),
        (7, "Two level"),
        (8, "Force flat"),
        (9, "No multi defs"),
        (10, "No fix prebinding"),
        (11, "Prebindable"),
        (12, "All mods bound"),
        (13, "Subsections via symbols"),
        (14, "Canonical"),
        (15, "Weak defines"),
        (16, "Binds to weak"),
        (17, "Allow stack execution"),
        (18, "Dead strippable dylib"),
        (19, "Root safe"),
        (20, "No reexported dylibs"),
        (21, "Random address"),
    ];
    let mut flag_parts: Vec<&str> = Vec::new();
    for (bit, name) in &flag_names {
        if flags & (1 << bit) != 0 {
            flag_parts.push(name);
        }
    }
    let flags_str = if flag_parts.is_empty() {
        format!("0x{:x}", flags)
    } else {
        flag_parts.join(", ")
    };
    tags.push(mk("ObjectFlags", "Object Flags", Value::String(flags_str)));

    Ok(tags)
}

/// Lookup Mach-O CPU subtype name by "cputype subtype" key.
fn macho_cpu_subtype(key: &str) -> &'static str {
    match key {
        "1 0" => "VAX (all)",
        "1 1" => "VAX780",
        "1 2" => "VAX785",
        "1 3" => "VAX750",
        "1 4" => "VAX730",
        "1 5" => "UVAXI",
        "1 6" => "UVAXII",
        "1 7" => "VAX8200",
        "1 8" => "VAX8500",
        "1 9" => "VAX8600",
        "1 10" => "VAX8650",
        "1 11" => "VAX8800",
        "1 12" => "UVAXIII",
        "2 0" => "RT (all)",
        "2 1" => "RT PC",
        "2 2" => "RT APC",
        "2 3" => "RT 135",
        "4 0" => "NS32032 (all)",
        "4 1" => "NS32032 DPC (032 CPU)",
        "4 2" => "NS32032 SQT",
        "4 3" => "NS32032 APC FPU (32081)",
        "4 4" => "NS32032 APC FPA (Weitek)",
        "4 5" => "NS32032 XPC (532)",
        "5 0" => "NS32332 (all)",
        "5 1" => "NS32332 DPC (032 CPU)",
        "5 2" => "NS32332 SQT",
        "5 3" => "NS32332 APC FPU (32081)",
        "5 4" => "NS32332 APC FPA (Weitek)",
        "5 5" => "NS32332 XPC (532)",
        "6 1" => "MC680x0 (all)",
        "6 2" => "MC68040",
        "6 3" => "MC68030",
        "7 3" => "i386 (all)",
        "7 4" => "i486",
        "7 132" => "i486SX",
        "7 5" => "i586",
        "7 22" => "Pentium Pro",
        "7 54" => "Pentium II M3",
        "7 86" => "Pentium II M5",
        "7 103" => "Celeron",
        "7 119" => "Celeron Mobile",
        "7 8" => "Pentium III",
        "7 24" => "Pentium III M",
        "7 40" => "Pentium III Xeon",
        "7 9" => "Pentium M",
        "7 10" => "Pentium 4",
        "7 26" => "Pentium 4 M",
        "7 11" => "Itanium",
        "7 27" => "Itanium 2",
        "7 12" => "Xeon",
        "7 28" => "Xeon MP",
        "8 0" => "MIPS (all)",
        "8 1" => "MIPS R2300",
        "8 2" => "MIPS R2600",
        "8 3" => "MIPS R2800",
        "8 4" => "MIPS R2000a",
        "8 5" => "MIPS R2000",
        "8 6" => "MIPS R3000a",
        "8 7" => "MIPS R3000",
        "10 0" => "MC98000 (all)",
        "10 1" => "MC98601",
        "11 0" => "HPPA (all)",
        "11 1" => "HPPA 7100LC",
        "12 0" => "ARM (all)",
        "12 1" => "ARM A500 ARCH",
        "12 2" => "ARM A500",
        "12 3" => "ARM A440",
        "12 4" => "ARM M4",
        "12 5" => "ARM A680/V4T",
        "12 6" => "ARM V6",
        "12 7" => "ARM V5TEJ",
        "12 8" => "ARM XSCALE",
        "12 9" => "ARM V7",
        "13 0" => "MC88000 (all)",
        "13 1" => "MC88100",
        "13 2" => "MC88110",
        "14 0" => "SPARC (all)",
        "14 1" => "SUN 4/260",
        "14 2" => "SUN 4/110",
        "15 0" => "i860 (all)",
        "15 1" => "i860 860",
        "16 0" => "i860 little (all)",
        "16 1" => "i860 little",
        "17 0" => "RS6000 (all)",
        "17 1" => "RS6000",
        "18 0" => "PowerPC (all)",
        "18 1" => "PowerPC 601",
        "18 2" => "PowerPC 602",
        "18 3" => "PowerPC 603",
        "18 4" => "PowerPC 603e",
        "18 5" => "PowerPC 603ev",
        "18 6" => "PowerPC 604",
        "18 7" => "PowerPC 604e",
        "18 8" => "PowerPC 620",
        "18 9" => "PowerPC 750",
        "18 10" => "PowerPC 7400",
        "18 11" => "PowerPC 7450",
        "18 100" => "PowerPC 970",
        "255 1" => "VEO 1",
        "255 2" => "VEO 2",
        _ => "",
    }
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "EXE".into(),
            family1: "EXE".into(),
            family2: "Other".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}
