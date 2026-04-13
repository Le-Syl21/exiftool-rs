//! Palm Database / Mobipocket / Kindle file reader.
//!
//! Parses PDB/MOBI/AZW formats.
//! Mirrors ExifTool's Palm.pm.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

// Palm file type IDs (8 bytes: type+creator)
fn palm_file_type(type_creator: &[u8]) -> Option<&'static str> {
    let key = std::str::from_utf8(type_creator).unwrap_or("");
    match key {
        ".pdfADBE" => Some("Adobe Reader"),
        "TEXtREAd" => Some("PalmDOC"),
        "BVokBDIC" => Some("BDicty"),
        "DB99DBOS" => Some("DB"),
        "PNRdPPrs" => Some("eReader"),
        "DataPPrs" => Some("eReader"),
        "vIMGView" => Some("FireViewer"),
        "PmDBPmDB" => Some("HanDBase"),
        "InfoINDB" => Some("InfoView"),
        "ToGoToGo" => Some("iSilo"),
        "SDocSilX" => Some("iSilo 3"),
        "JbDbJBas" => Some("JFile"),
        "JfDbJFil" => Some("JFile Pro"),
        "DATALSdb" => Some("LIST"),
        "Mdb1Mdb1" => Some("MobileDB"),
        "BOOKMOBI" => Some("Mobipocket"),
        "DataPlkr" => Some("Plucker"),
        "DataSprd" => Some("QuickSheet"),
        "SM01SMem" => Some("SuperMemo"),
        "TEXtTlDc" => Some("TealDoc"),
        "InfoTlIf" => Some("TealInfo"),
        "DataTlMl" => Some("TealMeal"),
        "DataTlPt" => Some("TealPaint"),
        "dataTDBP" => Some("ThinkDB"),
        "TdatTide" => Some("Tides"),
        "ToRaTRPW" => Some("TomeRaider"),
        "zTXTGPlm" => Some("Weasel"),
        "BDOCWrdS" => Some("WordSmith"),
        _ => None,
    }
}

pub fn read_palm(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 86 {
        return Err(Error::InvalidData("file too small".into()));
    }

    // Type/creator at offset 60 (8 bytes)
    let type_creator = &data[60..68];
    if palm_file_type(type_creator).is_none() {
        return Err(Error::InvalidData("not a Palm file".into()));
    }

    let mut tags = Vec::new();
    let file_type = palm_file_type(type_creator).unwrap_or("Unknown");

    // DatabaseName: bytes 0-31 (null-terminated string)
    let db_name = read_cstr(&data[..32]);
    tags.push(mk("DatabaseName", "Database Name", Value::String(db_name)));

    // Dates at offsets 36, 40, 44 (big-endian uint32, seconds since 1904 or 1970)
    let create_ts = u32::from_be_bytes([data[36], data[37], data[38], data[39]]) as i64;
    let modify_ts = u32::from_be_bytes([data[40], data[41], data[42], data[43]]) as i64;
    let backup_ts = u32::from_be_bytes([data[44], data[45], data[46], data[47]]) as i64;
    let mod_num = u32::from_be_bytes([data[48], data[49], data[50], data[51]]);

    tags.push(mk(
        "CreateDate",
        "Create Date",
        Value::String(palm_date(create_ts)),
    ));
    tags.push(mk(
        "ModifyDate",
        "Modify Date",
        Value::String(palm_date(modify_ts)),
    ));
    tags.push(mk(
        "LastBackupDate",
        "Last Backup Date",
        Value::String(palm_date(backup_ts)),
    ));
    tags.push(mk(
        "ModificationNumber",
        "Modification Number",
        Value::U32(mod_num),
    ));

    // PalmFileType (type+creator formatted)
    tags.push(mk(
        "PalmFileType",
        "Palm File Type",
        Value::String(file_type.into()),
    ));

    // If this is Mobipocket, parse MOBI header
    if file_type == "Mobipocket" {
        // Number of records at offset 76 (uint16 big-endian)
        let num_records = u16::from_be_bytes([data[76], data[77]]) as usize;
        if num_records == 0 {
            return Ok(tags);
        }
        // First record offset at offset 78 (uint32 big-endian)
        let first_offset = u32::from_be_bytes([data[78], data[79], data[80], data[81]]) as usize;

        parse_mobi(data, first_offset, &mut tags);
    }

    Ok(tags)
}

fn parse_mobi(data: &[u8], offset: usize, tags: &mut Vec<Tag>) {
    if offset + 274 > data.len() {
        return;
    }

    let mobi_data = &data[offset..];

    // Check for PalmDOC header (starts at beginning of record)
    // Compression at bytes 0-1
    let compression = u16::from_be_bytes([mobi_data[0], mobi_data[1]]);
    let comp_str = match compression {
        1 => "None",
        2 => "PalmDOC",
        17480 => "HUFF/CDIC",
        _ => "Unknown",
    };
    tags.push(mk(
        "Compression",
        "Compression",
        Value::String(comp_str.into()),
    ));

    // Uncompressed text length at bytes 4-7
    let text_len = u32::from_be_bytes([mobi_data[4], mobi_data[5], mobi_data[6], mobi_data[7]]);
    tags.push(mk(
        "UncompressedTextLength",
        "Uncompressed Text Length",
        Value::String(convert_file_size(text_len as i64)),
    ));

    // Encryption at bytes 12-13
    let encryption = u16::from_be_bytes([mobi_data[12], mobi_data[13]]);
    let enc_str = match encryption {
        0 => "None",
        1 => "Old Mobipocket",
        2 => "Mobipocket",
        _ => "Unknown",
    };
    tags.push(mk(
        "Encryption",
        "Encryption",
        Value::String(enc_str.into()),
    ));

    // Check for MOBI header at offset 16
    if mobi_data.len() < 20 || &mobi_data[16..20] != b"MOBI" {
        return;
    }

    // MOBI header starts at 16
    let mobi_hdr = &mobi_data[16..];
    if mobi_hdr.len() < 24 {
        return;
    }

    // MobiType at offset 8 in MOBI header (= mobi_hdr[8..12])
    let mobi_type = u32::from_be_bytes([mobi_hdr[8], mobi_hdr[9], mobi_hdr[10], mobi_hdr[11]]);
    let type_str = match mobi_type {
        2 => "Mobipocket Book",
        3 => "PalmDoc Book",
        4 => "Audio",
        232 => "mobipocket? generated by kindlegen1.2",
        248 => "KF8: generated by kindlegen2",
        257 => "News",
        258 => "News_Feed",
        259 => "News_Magazine",
        513 => "PICS",
        514 => "WORD",
        515 => "XLS",
        516 => "PPT",
        517 => "TEXT",
        518 => "HTML",
        _ => "Unknown",
    };
    tags.push(mk("MobiType", "Mobi Type", Value::String(type_str.into())));

    // CodePage at offset 28 in MOBI header
    let code_page = u32::from_be_bytes([mobi_hdr[12], mobi_hdr[13], mobi_hdr[14], mobi_hdr[15]]);
    let cp_str = match code_page {
        1252 => "Windows Latin 1 (Western European)".to_string(),
        65001 => "Unicode (UTF-8)".to_string(),
        n => format!("{}", n),
    };
    tags.push(mk("CodePage", "Code Page", Value::String(cp_str)));

    // MobiVersion at offset 36 in MOBI header
    if mobi_hdr.len() >= 40 {
        let mobi_version =
            u32::from_be_bytes([mobi_hdr[20], mobi_hdr[21], mobi_hdr[22], mobi_hdr[23]]);
        tags.push(mk("MobiVersion", "Mobi Version", Value::U32(mobi_version)));
    }

    // BookName: offset at byte 84, length at byte 88 (relative to record start = mobi_data)
    // In mobi_hdr (= mobi_data[16..]), byte 84 = mobi_hdr[68], byte 88 = mobi_hdr[72]
    if mobi_data.len() >= 92 {
        let name_offset =
            u32::from_be_bytes([mobi_data[84], mobi_data[85], mobi_data[86], mobi_data[87]])
                as usize;
        let name_len =
            u32::from_be_bytes([mobi_data[88], mobi_data[89], mobi_data[90], mobi_data[91]])
                as usize;
        // name_offset is relative to the record start (offset in data)
        let abs_name_off = offset + name_offset;
        if abs_name_off + name_len <= data.len() && name_len > 0 {
            let book_name = crate::encoding::decode_utf8_or_latin1(
                &data[abs_name_off..abs_name_off + name_len],
            )
            .to_string();
            if !book_name.is_empty() {
                tags.push(mk("BookName", "Book Name", Value::String(book_name)));
            }
        }
    }

    // MinimumVersion at index 26 (byte 104) from start of record (mobi_data[104])
    if mobi_data.len() >= 108 {
        let min_version = u32::from_be_bytes([
            mobi_data[104],
            mobi_data[105],
            mobi_data[106],
            mobi_data[107],
        ]);
        tags.push(mk(
            "MinimumVersion",
            "Minimum Version",
            Value::U32(min_version),
        ));
    }

    // EXTH header flag at byte 128 from start of record (mobi_data[128])
    if mobi_data.len() < 132 {
        return;
    }
    let exth_flag = u32::from_be_bytes([
        mobi_data[128],
        mobi_data[129],
        mobi_data[130],
        mobi_data[131],
    ]);
    if exth_flag & 0x40 == 0 {
        return; // No EXTH
    }

    // MOBI header length at offset 20 in MOBI header (mobi_hdr[4..8])
    let mobi_hdr_len =
        u32::from_be_bytes([mobi_hdr[4], mobi_hdr[5], mobi_hdr[6], mobi_hdr[7]]) as usize;

    // EXTH starts at: offset (record start) + 16 (PalmDoc header) + mobi_hdr_len
    let exth_start = offset + 16 + mobi_hdr_len;
    if exth_start + 12 > data.len() {
        return;
    }

    let exth = &data[exth_start..];
    if &exth[..4] != b"EXTH" {
        return;
    }

    let exth_len = u32::from_be_bytes([exth[4], exth[5], exth[6], exth[7]]) as usize;
    let _exth_count = u32::from_be_bytes([exth[8], exth[9], exth[10], exth[11]]);

    if exth_start + exth_len > data.len() {
        return;
    }

    parse_exth(&exth[12..exth_len.min(exth.len())], tags);
}

fn parse_exth(data: &[u8], tags: &mut Vec<Tag>) {
    let mut pos = 0;
    while pos + 8 <= data.len() {
        let tag = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
        let len = u32::from_be_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]])
            as usize;
        if len < 8 || pos + len > data.len() {
            break;
        }
        let val_data = &data[pos + 8..pos + len];
        pos += len;

        match tag {
            100 => extract_str_tag(val_data, "Author", tags),
            101 => extract_str_tag(val_data, "Publisher", tags),
            102 => extract_str_tag(val_data, "Imprint", tags),
            103 => extract_str_tag(val_data, "Description", tags),
            104 => extract_str_tag(val_data, "ISBN", tags),
            108 => extract_str_tag(val_data, "Contributor", tags),
            204 => {
                if val_data.len() >= 4 {
                    let v =
                        u32::from_be_bytes([val_data[0], val_data[1], val_data[2], val_data[3]]);
                    let s = match v {
                        1 => "Mobigen".to_string(),
                        2 => "Mobipocket".to_string(),
                        200 => "Kindlegen (Windows)".to_string(),
                        201 => "Kindlegen (Linux)".to_string(),
                        202 => "Kindlegen (Mac)".to_string(),
                        n => format!("{}", n),
                    };
                    tags.push(mk("CreatorSoftware", "Creator Software", Value::String(s)));
                }
            }
            205 => {
                if val_data.len() >= 4 {
                    let v =
                        u32::from_be_bytes([val_data[0], val_data[1], val_data[2], val_data[3]]);
                    tags.push(mk(
                        "CreatorMajorVersion",
                        "Creator Major Version",
                        Value::U32(v),
                    ));
                }
            }
            206 => {
                if val_data.len() >= 4 {
                    let v =
                        u32::from_be_bytes([val_data[0], val_data[1], val_data[2], val_data[3]]);
                    tags.push(mk(
                        "CreatorMinorVersion",
                        "Creator Minor Version",
                        Value::U32(v),
                    ));
                }
            }
            207 => {
                if val_data.len() >= 4 {
                    let v =
                        u32::from_be_bytes([val_data[0], val_data[1], val_data[2], val_data[3]]);
                    tags.push(mk(
                        "CreatorBuildNumber",
                        "Creator Build Number",
                        Value::U32(v),
                    ));
                }
            }
            _ => {}
        }
    }
}

fn extract_str_tag(data: &[u8], name: &str, tags: &mut Vec<Tag>) {
    let s = crate::encoding::decode_utf8_or_latin1(data)
        .trim_end_matches('\0')
        .to_string();
    if !s.is_empty() {
        tags.push(mk(name, name, Value::String(s)));
    }
}

fn read_cstr(data: &[u8]) -> String {
    let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    crate::encoding::decode_utf8_or_latin1(&data[..end]).to_string()
}

/// Convert Palm timestamp to ExifTool date string.
/// Palm dates are seconds since Jan 1, 1904 (if >= offset) or Jan 1, 1970
fn palm_date(ts: i64) -> String {
    let mac_epoch_offset: i64 = (66 * 365 + 17) * 24 * 3600;
    let unix_ts = if ts >= mac_epoch_offset {
        ts - mac_epoch_offset
    } else {
        ts
    };

    // Get local timezone offset
    let utc_offset = get_local_utc_offset();
    let adjusted = unix_ts + utc_offset;

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
        if rem < dy {
            break;
        }
        rem -= dy;
        year += 1;
    }
    let leap = is_leap(year);
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
    for &dm in &month_days {
        if rem < dm {
            break;
        }
        rem -= dm;
        month += 1;
    }
    let day = rem + 1;

    let offset_hours = utc_offset / 3600;
    let offset_mins = (utc_offset.abs() % 3600) / 60;
    let sign = if utc_offset >= 0 { '+' } else { '-' };
    format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}{}{:02}:{:02}",
        year,
        month,
        day,
        hour,
        minute,
        second,
        sign,
        offset_hours.abs(),
        offset_mins
    )
}

fn get_local_utc_offset() -> i64 {
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

fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn convert_file_size(bytes: i64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.1} kB", bytes as f64 / 1_000.0)
    } else {
        format!("{} bytes", bytes)
    }
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "Palm".into(),
            family1: "Palm".into(),
            family2: "Document".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}
