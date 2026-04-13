//! FlashPix / OLE Compound Document format reader.
//! Parses OLE2 compound documents (PPT, DOC, XLS, FPX) to extract
//! SummaryInformation and DocumentSummaryInformation property sets.
//! Mirrors ExifTool's FlashPix.pm ProcessFPX / ProcessProperties.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

fn mk(name: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: "FlashPix".into(),
            family1: "FlashPix".into(),
            family2: "Document".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}

fn mk_print(name: &str, value: Value, print: String) -> Tag {
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: "FlashPix".into(),
            family1: "FlashPix".into(),
            family2: "Document".into(),
        },
        raw_value: value,
        print_value: print,
        priority: 0,
    }
}

fn r16(data: &[u8], off: usize) -> u16 {
    if off + 2 > data.len() {
        return 0;
    }
    u16::from_le_bytes([data[off], data[off + 1]])
}

fn r32(data: &[u8], off: usize) -> u32 {
    if off + 4 > data.len() {
        return 0;
    }
    u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

fn r64(data: &[u8], off: usize) -> u64 {
    if off + 8 > data.len() {
        return 0;
    }
    u64::from_le_bytes([
        data[off],
        data[off + 1],
        data[off + 2],
        data[off + 3],
        data[off + 4],
        data[off + 5],
        data[off + 6],
        data[off + 7],
    ])
}

fn filetime_to_str(ft: u64) -> Option<String> {
    if ft == 0 {
        return None;
    }
    let secs = (ft / 10_000_000) as i64 - 11644473600;
    if secs < 0 {
        return None;
    }
    Some(unix_to_str(secs))
}

fn unix_to_str(unix: i64) -> String {
    if unix < 0 {
        return String::new();
    }
    let secs = unix % 86400;
    let days = unix / 86400;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    let (year, month, day) = days_to_date(days);
    format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
        year, month, day, h, m, s
    )
}

fn days_to_date(days: i64) -> (i32, u32, u32) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    (year as i32, month as u32, d as u32)
}

/// OLE VT_DATE: number of days since Dec 30, 1899
fn ole_date_to_str(days: f64) -> String {
    if days == 0.0 {
        return String::new();
    }
    let unix = ((days - 25569.0) * 86400.0) as i64;
    unix_to_str(unix)
}

/// Read a UTF-16LE string from data
fn read_utf16le(data: &[u8], count: usize) -> String {
    let words: Vec<u16> = (0..count.min(data.len() / 2))
        .map(|i| u16::from_le_bytes([data[i * 2], data[i * 2 + 1]]))
        .collect();
    let end = words.iter().position(|&w| w == 0).unwrap_or(words.len());
    String::from_utf16_lossy(&words[..end]).to_string()
}

/// Read a string from data (null-terminated or fixed length)
fn read_str_bytes(data: &[u8]) -> String {
    let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    crate::encoding::decode_utf8_or_latin1(&data[..end])
        .trim()
        .to_string()
}

/// OLE sector size (usually 512 bytes)
const HDR_SIZE: usize = 512;
const END_OF_CHAIN: u32 = 0xFFFFFFFE;
const FREESECT: u32 = 0xFFFFFFFF;

/// Read a chain of sectors from the FAT
fn read_sector_chain(data: &[u8], fat: &[u32], start_sector: u32, sector_size: usize) -> Vec<u8> {
    let mut result = Vec::new();
    let mut sector = start_sector;
    let mut count = 0;

    while sector != END_OF_CHAIN && sector != FREESECT && count < 10000 {
        let offset = HDR_SIZE + sector as usize * sector_size;
        if offset + sector_size > data.len() {
            break;
        }
        result.extend_from_slice(&data[offset..offset + sector_size]);
        if sector as usize >= fat.len() {
            break;
        }
        sector = fat[sector as usize];
        count += 1;
    }

    result
}

/// Parse the OLE FAT (File Allocation Table)
fn parse_fat(data: &[u8], sector_size: usize, difat: &[u32], fat_sector_count: u32) -> Vec<u32> {
    let mut fat = Vec::new();
    let entries_per_sector = sector_size / 4;
    let mut fat_sectors_read = 0;

    for &sect in difat {
        if sect == FREESECT || sect == END_OF_CHAIN {
            break;
        }
        if fat_sectors_read >= fat_sector_count as usize {
            break;
        }
        let off = HDR_SIZE + sect as usize * sector_size;
        if off + sector_size > data.len() {
            break;
        }
        for i in 0..entries_per_sector {
            fat.push(r32(data, off + i * 4));
        }
        fat_sectors_read += 1;
    }

    fat
}

/// Directory entry structure (128 bytes each)
struct DirEntry {
    name: String,
    entry_type: u8,
    start_sector: u32,
    size: u32,
}

fn parse_dir_entry(data: &[u8]) -> DirEntry {
    if data.len() < 128 {
        return DirEntry {
            name: String::new(),
            entry_type: 0,

            start_sector: FREESECT,
            size: 0,
        };
    }
    let name_len = r16(data, 64) as usize;
    let name = if name_len >= 2 {
        let name_bytes = &data[0..name_len.min(64)];
        read_utf16le(name_bytes, name_len / 2)
    } else {
        String::new()
    };
    DirEntry {
        name,
        entry_type: data[66],
        start_sector: r32(data, 116),
        size: r32(data, 120),
    }
}

/// Read all directory entries
fn parse_directory(dir_data: &[u8]) -> Vec<DirEntry> {
    let mut entries = Vec::new();
    let mut pos = 0;
    while pos + 128 <= dir_data.len() {
        entries.push(parse_dir_entry(&dir_data[pos..pos + 128]));
        pos += 128;
    }
    entries
}

/// Process an OLE property set stream
fn process_properties(data: &[u8], is_summary: bool, tags: &mut Vec<Tag>) {
    if data.len() < 28 {
        return;
    }

    // Property set header: byte order(2) + version(2) + SystemID(4) + CLSID(16) + reserved + num_property_sets(4)
    let byte_order = r16(data, 0);
    if byte_order != 0xFFFE {
        return;
    } // must be little-endian

    let num_sets = r32(data, 24) as usize;
    if num_sets == 0 {
        return;
    }

    // Process each property set
    for set_idx in 0..num_sets.min(2) {
        let hdr_off = 28 + set_idx * 20; // GUID(16) + offset(4)
        if hdr_off + 20 > data.len() {
            break;
        }
        let set_offset = r32(data, hdr_off + 16) as usize;
        if set_offset + 8 > data.len() {
            break;
        }

        let _set_size = r32(data, set_offset) as usize;
        let prop_count = r32(data, set_offset + 4) as usize;
        if prop_count > 1000 {
            continue;
        }

        // Read code page from property 0x01

        // First pass: find code page
        for i in 0..prop_count.min(500) {
            let off = set_offset + 8 + i * 8;
            if off + 8 > data.len() {
                break;
            }
            let prop_id = r32(data, off);
            let prop_off = r32(data, off + 4) as usize;
            let val_off = set_offset + prop_off;
            if val_off + 4 > data.len() {
                continue;
            }
            if prop_id == 1 {
                let vtype = r32(data, val_off) & 0xFFF;
                if vtype == 2 && val_off + 6 <= data.len() {}
                break;
            }
        }

        // Second pass: extract all properties
        for i in 0..prop_count.min(500) {
            let off = set_offset + 8 + i * 8;
            if off + 8 > data.len() {
                break;
            }
            let prop_id = r32(data, off);
            let prop_off = r32(data, off + 4) as usize;
            let val_off = set_offset + prop_off;
            if val_off + 4 > data.len() {
                continue;
            }

            let vtype_full = r32(data, val_off);
            let vtype = vtype_full & 0x0FFF;
            let is_vector = (vtype_full & 0x1000) != 0;

            let val_data = if val_off + 4 < data.len() {
                &data[val_off + 4..]
            } else {
                continue;
            };

            if is_vector {
                // VT_VECTOR: count (4 bytes) followed by elements
                if val_data.len() < 4 {
                    continue;
                }
                let count = r32(val_data, 0) as usize;
                process_vector_prop(
                    prop_id,
                    vtype,
                    &val_data[4..],
                    count,
                    set_idx,
                    is_summary,
                    tags,
                );
                continue;
            }

            let val = match vtype {
                0 | 1 => continue, // VT_EMPTY, VT_NULL
                2 => {
                    // VT_I2
                    if val_data.len() < 2 {
                        continue;
                    }
                    let v = r16(val_data, 0) as i16;
                    v.to_string()
                }
                3 | 10 => {
                    // VT_I4, VT_ERROR
                    if val_data.len() < 4 {
                        continue;
                    }
                    let v = r32(val_data, 0) as i32;
                    v.to_string()
                }
                4 => {
                    // VT_R4
                    if val_data.len() < 4 {
                        continue;
                    }
                    let v = f32::from_bits(r32(val_data, 0));
                    format!("{}", v)
                }
                5 => {
                    // VT_R8
                    if val_data.len() < 8 {
                        continue;
                    }
                    let v = f64::from_bits(r64(val_data, 0));
                    format!("{}", v)
                }
                7 => {
                    // VT_DATE (double, days since Dec 30, 1899)
                    if val_data.len() < 8 {
                        continue;
                    }
                    let v = f64::from_bits(r64(val_data, 0));
                    ole_date_to_str(v)
                }
                8 => {
                    // VT_BSTR
                    if val_data.len() < 4 {
                        continue;
                    }
                    let len = r32(val_data, 0) as usize;
                    if val_data.len() < 4 + len {
                        continue;
                    }
                    read_utf16le(&val_data[4..4 + len], len / 2)
                }
                11 => {
                    // VT_BOOL
                    if val_data.len() < 2 {
                        continue;
                    }
                    let v = r16(val_data, 0);
                    if v != 0 {
                        "1".to_string()
                    } else {
                        "0".to_string()
                    }
                }
                16 => {
                    // VT_I1
                    if val_data.is_empty() {
                        continue;
                    }
                    (val_data[0] as i8).to_string()
                }
                17 => {
                    // VT_UI1
                    if val_data.is_empty() {
                        continue;
                    }
                    val_data[0].to_string()
                }
                18 => {
                    // VT_UI2
                    if val_data.len() < 2 {
                        continue;
                    }
                    r16(val_data, 0).to_string()
                }
                19 => {
                    // VT_UI4
                    if val_data.len() < 4 {
                        continue;
                    }
                    r32(val_data, 0).to_string()
                }
                20 => {
                    // VT_I8
                    if val_data.len() < 8 {
                        continue;
                    }
                    let v = r64(val_data, 0) as i64;
                    v.to_string()
                }
                21 => {
                    // VT_UI8
                    if val_data.len() < 8 {
                        continue;
                    }
                    r64(val_data, 0).to_string()
                }
                30 => {
                    // VT_LPSTR
                    if val_data.len() < 4 {
                        continue;
                    }
                    let len = r32(val_data, 0) as usize;
                    if val_data.len() < 4 + len {
                        continue;
                    }
                    read_str_bytes(&val_data[4..4 + len])
                }
                31 => {
                    // VT_LPWSTR
                    if val_data.len() < 4 {
                        continue;
                    }
                    let word_count = r32(val_data, 0) as usize;
                    if val_data.len() < 4 + word_count * 2 {
                        continue;
                    }
                    read_utf16le(&val_data[4..], word_count)
                }
                64 => {
                    // VT_FILETIME
                    if val_data.len() < 8 {
                        continue;
                    }
                    let ft = r64(val_data, 0);
                    // Convert 100ns units to seconds
                    let secs = ft as f64 * 1e-7;
                    let one_year_secs = 365.0 * 24.0 * 3600.0;
                    if secs > one_year_secs {
                        // Treat as timestamp
                        match filetime_to_str(ft) {
                            Some(s) => s,
                            None => continue,
                        }
                    } else {
                        // Treat as time span (seconds) - pass as seconds string
                        // Use a special marker so process_*_prop can format it correctly
                        format!("TIMESPAN:{}", secs)
                    }
                }
                65 | 71 => {
                    // VT_BLOB, VT_CF
                    if val_data.len() < 4 {
                        continue;
                    }
                    let len = r32(val_data, 0) as usize;
                    format!("(Binary data {} bytes, use -b option to extract)", len)
                }
                72 => {
                    // VT_CLSID
                    if val_data.len() < 16 {
                        continue;
                    }
                    let g = val_data;
                    format!("{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
                        g[3],g[2],g[1],g[0], g[5],g[4], g[7],g[6],
                        g[8],g[9], g[10],g[11],g[12],g[13],g[14],g[15])
                }
                _ => continue,
            };

            if val.is_empty() {
                continue;
            }

            if is_summary {
                process_summary_prop(prop_id, val, tags);
            } else if set_idx == 0 {
                process_docinfo_prop(prop_id, val, tags);
            } else {
                // UserDefined properties
                process_userdefined_prop(prop_id, val, tags);
            }
        }
    }
}

fn process_vector_prop(
    prop_id: u32,
    elem_type: u32,
    data: &[u8],
    count: usize,
    set_idx: usize,
    is_summary: bool,
    tags: &mut Vec<Tag>,
) {
    let mut vals = Vec::new();
    let mut pos = 0;

    for _ in 0..count.min(100) {
        let val = match elem_type {
            2 | 18 => {
                if pos + 4 > data.len() {
                    break;
                }
                let v = r16(data, pos);
                pos += 4; // padded
                v.to_string()
            }
            3 | 10 => {
                if pos + 4 > data.len() {
                    break;
                }
                let v = r32(data, pos) as i32;
                pos += 4;
                v.to_string()
            }
            30 => {
                // VT_LPSTR
                if pos + 4 > data.len() {
                    break;
                }
                let len = r32(data, pos) as usize;
                pos += 4;
                if pos + len > data.len() {
                    break;
                }
                let s = read_str_bytes(&data[pos..pos + len]);
                pos += (len + 3) & !3;
                s
            }
            31 => {
                // VT_LPWSTR
                if pos + 4 > data.len() {
                    break;
                }
                let wcount = r32(data, pos) as usize;
                pos += 4;
                if pos + wcount * 2 > data.len() {
                    break;
                }
                let s = read_utf16le(&data[pos..], wcount);
                pos += (wcount * 2 + 3) & !3;
                s
            }
            12 => {
                // VT_VARIANT (4 bytes type + value)
                if pos + 4 > data.len() {
                    break;
                }
                let sub_type = r32(data, pos) & 0xFFF;
                pos += 4;
                match sub_type {
                    3 => {
                        if pos + 4 > data.len() {
                            break;
                        }
                        let v = r32(data, pos) as i32;
                        pos += 4;
                        v.to_string()
                    }
                    5 => {
                        // double
                        if pos + 8 > data.len() {
                            break;
                        }
                        let v = f64::from_bits(r64(data, pos));
                        pos += 8;
                        format!("{}", v)
                    }
                    7 => {
                        // VT_DATE
                        if pos + 8 > data.len() {
                            break;
                        }
                        let v = f64::from_bits(r64(data, pos));
                        pos += 8;
                        ole_date_to_str(v)
                    }
                    11 => {
                        // VT_BOOL
                        if pos + 4 > data.len() {
                            break;
                        }
                        let v = r16(data, pos);
                        pos += 4;
                        if v != 0 {
                            "1".into()
                        } else {
                            "0".into()
                        }
                    }
                    30 => {
                        // VT_LPSTR
                        if pos + 4 > data.len() {
                            break;
                        }
                        let len = r32(data, pos) as usize;
                        pos += 4;
                        if pos + len > data.len() {
                            break;
                        }
                        let s = read_str_bytes(&data[pos..pos + len]);
                        pos += (len + 3) & !3;
                        s
                    }
                    31 => {
                        // VT_LPWSTR
                        if pos + 4 > data.len() {
                            break;
                        }
                        let wcount = r32(data, pos) as usize;
                        pos += 4;
                        if pos + wcount * 2 > data.len() {
                            break;
                        }
                        let s = read_utf16le(&data[pos..], wcount);
                        pos += (wcount * 2 + 3) & !3;
                        s
                    }
                    64 => {
                        // FILETIME
                        if pos + 8 > data.len() {
                            break;
                        }
                        let ft = r64(data, pos);
                        pos += 8;
                        filetime_to_str(ft).unwrap_or_default()
                    }
                    _ => {
                        // Skip unknown variant
                        break;
                    }
                }
            }
            _ => break,
        };
        if !val.is_empty() {
            vals.push(val);
        }
    }

    if vals.is_empty() {
        return;
    }
    let combined = vals.join(", ");

    if is_summary {
        process_summary_prop(prop_id, combined, tags);
    } else if set_idx == 0 {
        process_docinfo_prop(prop_id, combined, tags);
    } else {
        process_userdefined_prop(prop_id, combined, tags);
    }
}

fn process_summary_prop(prop_id: u32, val: String, tags: &mut Vec<Tag>) {
    match prop_id {
        0x01 => {
            // CodePage
            let cp: u32 = val.parse().unwrap_or(0);
            let cp_name = codepage_name(cp);
            tags.push(mk_print("CodePage", Value::String(val), cp_name));
        }
        0x02 => tags.push(mk("Title", Value::String(val))),
        0x03 => tags.push(mk("Subject", Value::String(val))),
        0x04 => tags.push(mk("Author", Value::String(val))),
        0x05 => tags.push(mk("Keywords", Value::String(val))),
        0x06 => tags.push(mk("Comments", Value::String(val))),
        0x07 => tags.push(mk("Template", Value::String(val))),
        0x08 => tags.push(mk("LastModifiedBy", Value::String(val))),
        0x09 => tags.push(mk("RevisionNumber", Value::String(val))),
        0x0a => {
            // TotalEditTime as time span (seconds)
            let secs = if val.starts_with("TIMESPAN:") {
                val[9..].parse::<f64>().unwrap_or(0.0)
            } else {
                val.parse::<f64>().unwrap_or(0.0)
            };
            let print = convert_time_span(secs);
            let raw = format!("{}", secs as u64);
            tags.push(mk_print("TotalEditTime", Value::String(raw), print));
        }
        0x0c => tags.push(mk("CreateDate", Value::String(val))),
        0x0d => tags.push(mk("ModifyDate", Value::String(val))),
        0x0e => tags.push(mk("Pages", Value::String(val))),
        0x0f => tags.push(mk("Words", Value::String(val))),
        0x10 => tags.push(mk("Characters", Value::String(val))),
        0x12 => tags.push(mk("Software", Value::String(val))),
        _ => {}
    }
}

fn process_docinfo_prop(prop_id: u32, val: String, tags: &mut Vec<Tag>) {
    match prop_id {
        0x02 => tags.push(mk("Category", Value::String(val))),
        0x03 => tags.push(mk("PresentationTarget", Value::String(val))),
        0x04 => tags.push(mk("Bytes", Value::String(val))),
        0x05 => tags.push(mk("Lines", Value::String(val))),
        0x06 => tags.push(mk("Paragraphs", Value::String(val))),
        0x07 => tags.push(mk("Slides", Value::String(val))),
        0x08 => tags.push(mk("Notes", Value::String(val))),
        0x09 => tags.push(mk("HiddenSlides", Value::String(val))),
        0x0a => tags.push(mk("MMClips", Value::String(val))),
        0x0b => {
            let v = if val == "0" { "No" } else { "Yes" };
            tags.push(mk_print("ScaleCrop", Value::String(val), v.into()));
        }
        0x0c => tags.push(mk("HeadingPairs", Value::String(val))),
        0x0d => tags.push(mk("TitleOfParts", Value::String(val))),
        0x0e => tags.push(mk("Manager", Value::String(val))),
        0x0f => tags.push(mk("Company", Value::String(val))),
        0x10 => {
            let v = if val == "0" { "No" } else { "Yes" };
            tags.push(mk_print("LinksUpToDate", Value::String(val), v.into()));
        }
        0x13 => {
            let v = if val == "0" { "No" } else { "Yes" };
            tags.push(mk_print("SharedDoc", Value::String(val), v.into()));
        }
        0x16 => {
            let v = if val == "0" { "No" } else { "Yes" };
            tags.push(mk_print("HyperlinksChanged", Value::String(val), v.into()));
        }
        0x17 => {
            // AppVersion: upper 16 bits = major, lower 16 bits = minor
            let n: u32 = val.parse().unwrap_or(0);
            let ver = format!("{}.{:04}", n >> 16, n & 0xFFFF);
            tags.push(mk_print("AppVersion", Value::String(val), ver));
        }
        _ => {}
    }
}

/// Process UserDefined property set (custom properties)
fn process_userdefined_prop(prop_id: u32, val: String, tags: &mut Vec<Tag>) {
    // We skip this for now as it requires a string dictionary
    // The specific tags (CustomBoolean, CustomDate, CustomNumber, CustomText)
    // require the dictionary from prop_id=0 to map numeric IDs to names
    let _ = (prop_id, val, tags);
}

/// Process UserDefined properties with a dictionary
fn process_userdefined_with_dict(data: &[u8], set_offset: usize, tags: &mut Vec<Tag>) {
    if set_offset + 8 > data.len() {
        return;
    }
    let prop_count = r32(data, set_offset + 4) as usize;
    if prop_count > 500 {
        return;
    }

    // First find the dictionary (prop_id = 0)
    let mut dict: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
    for i in 0..prop_count.min(500) {
        let off = set_offset + 8 + i * 8;
        if off + 8 > data.len() {
            break;
        }
        let prop_id = r32(data, off);
        if prop_id != 0 {
            continue;
        }
        let prop_off = r32(data, off + 4) as usize;
        let val_off = set_offset + prop_off;
        if val_off + 4 > data.len() {
            break;
        }
        // Dictionary: count (4 bytes) + entries (id(4) + name_len(4) + name_bytes)
        let dict_count = r32(data, val_off) as usize;
        let mut pos = val_off + 4;
        for _ in 0..dict_count.min(100) {
            if pos + 8 > data.len() {
                break;
            }
            let id = r32(data, pos);
            let name_len = r32(data, pos + 4) as usize;
            pos += 8;
            if pos + name_len > data.len() {
                break;
            }
            let name = read_str_bytes(&data[pos..pos + name_len]);
            dict.insert(id, name);
            // OLE dictionary names are NOT padded to 4 bytes
            pos += name_len;
        }
        break;
    }

    // Now process actual properties
    for i in 0..prop_count.min(500) {
        let off = set_offset + 8 + i * 8;
        if off + 8 > data.len() {
            break;
        }
        let prop_id = r32(data, off);
        if prop_id == 0 || prop_id == 1 {
            continue;
        } // skip dictionary and codepage

        let name = match dict.get(&prop_id) {
            Some(n) => n.clone(),
            None => continue,
        };

        let prop_off = r32(data, off + 4) as usize;
        let val_off = set_offset + prop_off;
        if val_off + 4 > data.len() {
            continue;
        }
        let vtype = r32(data, val_off) & 0xFFF;
        let val_data = if val_off + 4 < data.len() {
            &data[val_off + 4..]
        } else {
            continue;
        };

        // Handle special system properties
        if name == "_PID_LINKBASE" {
            // VT_BLOB containing UTF-16LE string
            if vtype == 65 && val_data.len() >= 4 {
                // VT_BLOB
                let blob_size = r32(val_data, 0) as usize;
                if val_data.len() >= 4 + blob_size && blob_size >= 2 {
                    let blob = &val_data[4..4 + blob_size];
                    // Strip trailing null and decode UTF-16LE
                    let wcount = blob_size / 2;
                    let s = read_utf16le(blob, wcount);
                    if !s.is_empty() {
                        tags.push(mk("HyperlinkBase", Value::String(s)));
                    }
                }
            }
            continue;
        }

        if name == "_PID_HLINKS" {
            // VT_BLOB containing array of VT_VARIANT hyperlinks
            if vtype == 65 && val_data.len() >= 4 {
                // VT_BLOB
                let blob_size = r32(val_data, 0) as usize;
                if val_data.len() >= 4 + blob_size {
                    let blob = &val_data[4..4 + blob_size];
                    if let Some(links) = process_hyperlinks_blob(blob) {
                        if !links.is_empty() {
                            tags.push(mk("Hyperlinks", Value::String(links)));
                        }
                    }
                }
            }
            continue;
        }

        let val = match vtype {
            2 | 18 => {
                if val_data.len() < 2 {
                    continue;
                }
                r16(val_data, 0).to_string()
            }
            3 => {
                if val_data.len() < 4 {
                    continue;
                }
                (r32(val_data, 0) as i32).to_string()
            }
            5 => {
                if val_data.len() < 8 {
                    continue;
                }
                let v = f64::from_bits(r64(val_data, 0));
                format!("{}", v)
            }
            7 => {
                // VT_DATE
                if val_data.len() < 8 {
                    continue;
                }
                let v = f64::from_bits(r64(val_data, 0));
                ole_date_to_str(v)
            }
            11 => {
                // VT_BOOL
                if val_data.len() < 2 {
                    continue;
                }
                let v = r16(val_data, 0);
                if v != 0 {
                    "1".into()
                } else {
                    "0".into()
                }
            }
            30 => {
                // VT_LPSTR
                if val_data.len() < 4 {
                    continue;
                }
                let len = r32(val_data, 0) as usize;
                if val_data.len() < 4 + len {
                    continue;
                }
                read_str_bytes(&val_data[4..4 + len])
            }
            31 => {
                // VT_LPWSTR
                if val_data.len() < 4 {
                    continue;
                }
                let wcount = r32(val_data, 0) as usize;
                if val_data.len() < 4 + wcount * 2 {
                    continue;
                }
                read_utf16le(&val_data[4..], wcount)
            }
            64 => {
                // VT_FILETIME
                if val_data.len() < 8 {
                    continue;
                }
                let ft = r64(val_data, 0);
                match filetime_to_str(ft) {
                    Some(s) => s,
                    None => continue,
                }
            }
            _ => continue,
        };

        if val.is_empty() {
            continue;
        }

        // Build tag name from dictionary name
        // Remove leading "Custom " prefix if present
        let clean_name = if name.starts_with("Custom ") {
            &name[7..]
        } else {
            &name[..]
        };

        // Capitalize and form tag name
        let tag_name = {
            let mut chars = clean_name.chars();
            match chars.next() {
                None => format!("Custom{}", name),
                Some(f) => {
                    let mut s = f.to_uppercase().to_string();
                    s.extend(chars);
                    // Remove spaces for camelCase
                    let s = s.replace(' ', "");
                    format!("Custom{}", s)
                }
            }
        };
        tags.push(mk(&tag_name, Value::String(val)));
    }
}

/// Parse the _PID_HLINKS VT_BLOB as an array of VT_VARIANTs
/// Each hyperlink consists of 6 VT_VARIANTs: [type, flags, name, frame, address, subaddress]
fn process_hyperlinks_blob(blob: &[u8]) -> Option<String> {
    if blob.len() < 4 {
        return None;
    }
    let num_variants = r32(blob, 0) as usize;
    if num_variants == 0 {
        return None;
    }
    let mut pos = 4;
    let mut vals: Vec<String> = Vec::new();

    for _ in 0..num_variants.min(200) {
        if pos + 4 > blob.len() {
            break;
        }
        let vtype = r32(blob, pos) & 0xFFF;
        pos += 4;
        let val = match vtype {
            3 => {
                // VT_I4
                if pos + 4 > blob.len() {
                    break;
                }
                let v = r32(blob, pos) as i32;
                pos += 4;
                v.to_string()
            }
            31 => {
                // VT_LPWSTR - word count then UTF-16LE data
                if pos + 4 > blob.len() {
                    break;
                }
                let wcount = r32(blob, pos) as usize;
                pos += 4;
                if pos + wcount * 2 > blob.len() {
                    break;
                }
                let s = read_utf16le(&blob[pos..], wcount);
                // Pad to 4-byte boundary
                let byte_len = wcount * 2;
                pos += (byte_len + 3) & !3;
                s
            }
            _ => {
                // Skip 4 bytes for unknown simple types
                pos += 4;
                String::new()
            }
        };
        vals.push(val);
    }

    // Groups of 6: [type, flags, name, frame, address, subaddress]
    let mut links: Vec<String> = Vec::new();
    let mut i = 0;
    while i + 5 < vals.len() {
        let mut link = vals[i + 4].clone(); // address is at index 4
        let subaddr = &vals[i + 5]; // subaddress is at index 5
        if !subaddr.is_empty() {
            link.push('#');
            link.push_str(subaddr);
        }
        if !link.is_empty() {
            links.push(link);
        }
        i += 6;
    }

    if links.is_empty() {
        None
    } else {
        Some(links.join(", "))
    }
}

/// Mirror ExifTool's ConvertTimeSpan - format seconds as human-readable span
fn convert_time_span(secs: f64) -> String {
    if secs <= 0.0 {
        return secs.to_string();
    }
    if secs < 60.0 {
        format!("{} seconds", secs)
    } else if secs < 3600.0 {
        format!("{:.1} minutes", secs / 60.0)
    } else if secs < 86400.0 {
        format!("{:.1} hours", secs / 3600.0)
    } else {
        format!("{:.1} days", secs / 86400.0)
    }
}

fn codepage_name(cp: u32) -> String {
    match cp {
        437 => "DOS US",
        850 => "DOS Latin 1",
        1250 => "Windows Latin 2 (Central European)",
        1251 => "Windows Cyrillic",
        1252 => "Windows Latin 1 (Western European)",
        10000 => "Mac Roman (Western European)",
        65001 => "Unicode (UTF-8)",
        _ => return cp.to_string(),
    }
    .to_string()
}

/// Extract the "Current User" stream for PowerPoint
/// Based on ExifTool's ValueConv: skip 4 bytes, read size(4), pos(4), extract ASCII name
fn extract_current_user(data: &[u8]) -> Option<String> {
    if data.len() < 12 {
        return None;
    }
    // Skip first 4 bytes, then read size and pos
    let size = r32(data, 4) as usize;
    let pos = r32(data, 8) as usize;
    let len = size.checked_sub(pos)?.checked_sub(4)?;
    if data.len() < size + 8 {
        return None;
    }
    let name_start = 8 + pos;
    if name_start + len > data.len() {
        return None;
    }
    let name = read_str_bytes(&data[name_start..name_start + len]);
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// Read a mini-stream (for streams smaller than mini_stream_cutoff)
fn read_mini_stream_chain(
    mini_fat: &[u32],
    mini_container: &[u8],
    start: u32,
    size: u32,
    mini_sector_size: usize,
) -> Vec<u8> {
    let mut result = Vec::new();
    let mut sector = start;
    let mut count = 0;
    while sector != END_OF_CHAIN && sector != FREESECT && count < 10000 {
        let offset = sector as usize * mini_sector_size;
        if offset + mini_sector_size > mini_container.len() {
            break;
        }
        result.extend_from_slice(&mini_container[offset..offset + mini_sector_size]);
        if sector as usize >= mini_fat.len() {
            break;
        }
        sector = mini_fat[sector as usize];
        count += 1;
    }
    // Truncate to actual size
    result.truncate(size as usize);
    result
}

pub fn read_fpx(data: &[u8]) -> Result<Vec<Tag>> {
    // OLE2 Compound File magic: D0 CF 11 E0 A1 B1 1A E1
    if data.len() < HDR_SIZE || &data[0..8] != &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1] {
        return Err(Error::InvalidData("not an OLE2 compound file".into()));
    }

    let sector_size_exp = r16(data, 30) as u32;
    let sector_size = if sector_size_exp == 0 {
        512
    } else {
        1 << sector_size_exp
    };
    if sector_size < 64 || sector_size > 65536 {
        return Err(Error::InvalidData("invalid OLE sector size".into()));
    }

    let fat_sector_count = r32(data, 44);
    let dir_start_sector = r32(data, 48);
    let mini_sector_size_exp = r16(data, 32) as u32;
    let mini_sector_size = if mini_sector_size_exp == 0 {
        64
    } else {
        (1u32 << mini_sector_size_exp) as usize
    };
    let mini_stream_cutoff = r32(data, 56);
    let first_mini_fat_sector = r32(data, 60);
    let num_mini_fat_sectors = r32(data, 64);

    // DIFAT array from header (at offset 76, up to 109 entries)
    let mut difat = Vec::new();
    for i in 0..109usize {
        let off = 76 + i * 4;
        if off + 4 > data.len() {
            break;
        }
        let sect = r32(data, off);
        if sect == FREESECT {
            break;
        }
        difat.push(sect);
    }

    let fat = parse_fat(data, sector_size, &difat, fat_sector_count);

    // Parse Mini FAT (stored in regular sectors)
    let mini_fat = if first_mini_fat_sector != END_OF_CHAIN && first_mini_fat_sector != FREESECT {
        let mini_fat_sectors = &[first_mini_fat_sector]; // simplified: just first sector
        parse_fat(data, sector_size, mini_fat_sectors, num_mini_fat_sectors)
    } else {
        Vec::new()
    };

    // Read directory
    let dir_data = read_sector_chain(data, &fat, dir_start_sector, sector_size);
    let entries = parse_directory(&dir_data);

    // Get mini-stream container from root directory entry (entry 0, type=5=ROOT)
    let mini_container = if !entries.is_empty() && entries[0].entry_type == 5 {
        let root_start = entries[0].start_sector;
        let root_size = entries[0].size;
        if root_start != FREESECT && root_start != END_OF_CHAIN {
            let data = read_sector_chain(data, &fat, root_start, sector_size);
            let sz = root_size.min(data.len() as u32) as usize;
            data[..sz].to_vec()
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    let mut tags = Vec::new();

    // Find and process interesting streams
    for entry in &entries {
        if entry.entry_type != 2 {
            continue;
        } // only STREAM type
        if entry.start_sector == FREESECT {
            continue;
        }

        // Determine if stream is in mini-stream or regular stream
        let stream: Vec<u8> = if entry.size < mini_stream_cutoff
            && !mini_container.is_empty()
            && !mini_fat.is_empty()
        {
            // Read from mini-stream
            read_mini_stream_chain(
                &mini_fat,
                &mini_container,
                entry.start_sector,
                entry.size,
                mini_sector_size,
            )
        } else {
            // Read from regular sectors
            let s = read_sector_chain(data, &fat, entry.start_sector, sector_size);
            let sz = entry.size.min(s.len() as u32) as usize;
            s[..sz].to_vec()
        };

        match entry.name.as_str() {
            "\u{0005}SummaryInformation" => {
                process_properties(&stream, true, &mut tags);
            }
            "\u{0005}DocumentSummaryInformation" => {
                process_docinfo_stream(&stream, &mut tags);
            }
            "Current User" => {
                if let Some(name) = extract_current_user(&stream) {
                    if !name.is_empty() {
                        tags.push(mk("CurrentUser", Value::String(name)));
                    }
                }
            }
            _ => {}
        }
    }

    Ok(tags)
}

/// Process the DocumentSummaryInformation stream (which may have 2 sections)
fn process_docinfo_stream(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 28 {
        return;
    }

    let byte_order = r16(data, 0);
    if byte_order != 0xFFFE {
        return;
    }

    let num_sets = r32(data, 24) as usize;

    // Process first section (DocumentInfo)
    if num_sets >= 1 {
        let hdr_off = 28;
        if hdr_off + 20 <= data.len() {
            let set_offset = r32(data, hdr_off + 16) as usize;
            if set_offset + 8 <= data.len() {
                let prop_count = r32(data, set_offset + 4) as usize;
                if prop_count <= 1000 {
                    for i in 0..prop_count.min(500) {
                        let off = set_offset + 8 + i * 8;
                        if off + 8 > data.len() {
                            break;
                        }
                        let prop_id = r32(data, off);
                        let prop_off = r32(data, off + 4) as usize;
                        let val_off = set_offset + prop_off;
                        if val_off + 4 > data.len() {
                            continue;
                        }

                        let vtype_full = r32(data, val_off);
                        let vtype = vtype_full & 0x0FFF;
                        let is_vector = (vtype_full & 0x1000) != 0;
                        let val_data = &data[val_off + 4..];

                        if is_vector {
                            if val_data.len() < 4 {
                                continue;
                            }
                            let count = r32(val_data, 0) as usize;
                            process_vector_prop(
                                prop_id,
                                vtype,
                                &val_data[4..],
                                count,
                                0,
                                false,
                                tags,
                            );
                        } else {
                            let val = extract_prop_val(vtype, val_data);
                            if let Some(v) = val {
                                if !v.is_empty() {
                                    process_docinfo_prop(prop_id, v, tags);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Process second section (UserDefined properties)
    if num_sets >= 2 {
        let hdr_off = 28 + 20;
        if hdr_off + 20 <= data.len() {
            let set_offset = r32(data, hdr_off + 16) as usize;
            if set_offset + 8 <= data.len() {
                process_userdefined_with_dict(data, set_offset, tags);
            }
        }
    }
}

fn extract_prop_val(vtype: u32, val_data: &[u8]) -> Option<String> {
    match vtype {
        2 | 18 => {
            if val_data.len() < 2 {
                return None;
            }
            Some(r16(val_data, 0).to_string())
        }
        3 | 10 | 19 => {
            if val_data.len() < 4 {
                return None;
            }
            Some((r32(val_data, 0) as i32).to_string())
        }
        4 => {
            if val_data.len() < 4 {
                return None;
            }
            Some(format!("{}", f32::from_bits(r32(val_data, 0))))
        }
        5 => {
            if val_data.len() < 8 {
                return None;
            }
            Some(format!("{}", f64::from_bits(r64(val_data, 0))))
        }
        7 => {
            // VT_DATE
            if val_data.len() < 8 {
                return None;
            }
            let v = f64::from_bits(r64(val_data, 0));
            Some(ole_date_to_str(v))
        }
        8 => {
            // VT_BSTR
            if val_data.len() < 4 {
                return None;
            }
            let len = r32(val_data, 0) as usize;
            if val_data.len() < 4 + len {
                return None;
            }
            Some(read_utf16le(&val_data[4..4 + len], len / 2))
        }
        11 => {
            // VT_BOOL
            if val_data.len() < 2 {
                return None;
            }
            Some(if r16(val_data, 0) != 0 {
                "1".into()
            } else {
                "0".into()
            })
        }
        17 => {
            if val_data.is_empty() {
                return None;
            }
            Some(val_data[0].to_string())
        }
        30 => {
            // VT_LPSTR
            if val_data.len() < 4 {
                return None;
            }
            let len = r32(val_data, 0) as usize;
            if val_data.len() < 4 + len {
                return None;
            }
            Some(read_str_bytes(&val_data[4..4 + len]))
        }
        31 => {
            // VT_LPWSTR
            if val_data.len() < 4 {
                return None;
            }
            let wcount = r32(val_data, 0) as usize;
            if val_data.len() < 4 + wcount * 2 {
                return None;
            }
            Some(read_utf16le(&val_data[4..], wcount))
        }
        64 => {
            // VT_FILETIME
            if val_data.len() < 8 {
                return None;
            }
            let ft = r64(val_data, 0);
            filetime_to_str(ft)
        }
        _ => None,
    }
}
