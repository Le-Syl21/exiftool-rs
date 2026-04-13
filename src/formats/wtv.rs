//! WTV (Windows Recorded TV) file format reader.
//!
//! Parses WTV metadata stored as name-value pairs in a sector-based structure.
//! Mirrors ExifTool's WTV.pm ProcessWTV / ProcessMetadata.
//!
//! References:
//!   https://wiki.multimedia.cx/index.php?title=WTV

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

// WTV file magic (first 16 bytes)
const WTV_MAGIC: [u8; 16] = [
    0xb7, 0xd8, 0x00, 0x20, 0x37, 0x49, 0xda, 0x11, 0xa6, 0x4e, 0x00, 0x07, 0xe9, 0x5e, 0xad, 0x8d,
];

// GUID for WTV directory entries
const DIR_ENTRY_GUID: [u8; 16] = [
    0x92, 0xb7, 0x74, 0x91, 0x59, 0x70, 0x70, 0x44, 0x88, 0xdf, 0x06, 0x3b, 0x82, 0xcc, 0x21, 0x3d,
];

// GUID sentinel for metadata entries
const METADATA_ENTRY_GUID: [u8; 16] = [
    0x5a, 0xfe, 0xd7, 0x6d, 0xc8, 0x1d, 0x8f, 0x4a, 0x99, 0x22, 0xfa, 0xb1, 0x1c, 0x38, 0x14, 0x53,
];

fn mk_wtv(name: &str, value: Value, print: String) -> Tag {
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: "WTV".into(),
            family1: "WTV".into(),
            family2: "Video".into(),
        },
        raw_value: value,
        print_value: print,
        priority: 0,
    }
}

fn read_u32_le(data: &[u8], off: usize) -> Option<u32> {
    if off + 4 > data.len() {
        return None;
    }
    Some(u32::from_le_bytes([
        data[off],
        data[off + 1],
        data[off + 2],
        data[off + 3],
    ]))
}

fn read_i32_le(data: &[u8], off: usize) -> Option<i32> {
    read_u32_le(data, off).map(|v| v as i32)
}

fn read_u64_le(data: &[u8], off: usize) -> Option<u64> {
    if off + 8 > data.len() {
        return None;
    }
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
}

/// Decode a UTF-16-LE byte slice to a String.
fn decode_utf16le(bytes: &[u8]) -> String {
    let words: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    String::from_utf16_lossy(&words)
        .trim_end_matches('\0')
        .to_string()
}

/// Read sectors referenced by a sector table.
///
/// `file`: the complete file data.
/// `sec_table`: the sector table bytes (each 4-byte entry is a sector number).
/// `pos`: start offset within `sec_table`.
/// `sec_size`: size of each sector in bytes.
///
/// Mirrors Perl's ReadSectors().
fn read_sectors(file: &[u8], sec_table: &[u8], mut pos: usize, sec_size: usize) -> Option<Vec<u8>> {
    let mut result: Vec<u8> = Vec::new();
    while pos + 4 <= sec_table.len() {
        let sec = u32::from_le_bytes([
            sec_table[pos],
            sec_table[pos + 1],
            sec_table[pos + 2],
            sec_table[pos + 3],
        ]) as usize;
        if sec == 0xffff {
            return None;
        }
        if sec == 0 {
            // null marks end of sector table
            break;
        }
        let offset = sec * sec_size;
        if offset + sec_size > file.len() {
            return None;
        }
        result.extend_from_slice(&file[offset..offset + sec_size]);
        pos += 4;
    }
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

/// Returns true if the raw tag name is marked Unknown=1 in the Perl tag table.
/// These tags are not output by default (Perl ExifTool hides them unless -u flag is used).
fn is_unknown_tag(raw: &str) -> bool {
    matches!(
        raw,
        "WM/WMRVBitrate"
            | "WM/WMRVExpirationDate"
            | "WM/WMRVExpirationSpan"
            | "WM/MediaThumbTimeStamp"
    )
}

/// Map a raw WTV metadata tag name to the ExifTool tag name.
///
/// Mirrors the tag table in WTV.pm:
///   - 'Duration' -> 'Duration'
///   - 'Title' -> 'Title'
///   - 'WM/Genre' -> 'Genre'
///   - 'WM/...' -> strip 'WM/' and optionally 'WMRV'
fn map_tag_name(raw: &str) -> String {
    // Explicit mappings from the Perl tag table
    match raw {
        "WM/Genre" => return "Genre".into(),
        "WM/Language" => return "Language".into(),
        "WM/MediaClassPrimaryID" => return "MediaClassPrimaryID".into(),
        "WM/MediaClassSecondaryID" => return "MediaClassSecondaryID".into(),
        "WM/MediaCredits" => return "MediaCredits".into(),
        "WM/MediaIsDelay" => return "MediaIsDelay".into(),
        "WM/MediaIsFinale" => return "MediaIsFinale".into(),
        "WM/MediaIsLive" => return "MediaIsLive".into(),
        "WM/MediaIsMovie" => return "MediaIsMovie".into(),
        "WM/MediaIsPremiere" => return "MediaIsPremiere".into(),
        "WM/MediaIsRepeat" => return "MediaIsRepeat".into(),
        "WM/MediaIsSAP" => return "MediaIsSAP".into(),
        "WM/MediaIsSport" => return "MediaIsSport".into(),
        "WM/MediaIsStereo" => return "MediaIsStereo".into(),
        "WM/MediaIsSubtitled" => return "MediaIsSubtitled".into(),
        "WM/MediaIsTape" => return "MediaIsTape".into(),
        "WM/MediaNetworkAffiliation" => return "MediaNetworkAffiliation".into(),
        "WM/MediaOriginalBroadcastDateTime" => return "MediaOriginalBroadcastDateTime".into(),
        "WM/MediaOriginalChannel" => return "MediaOriginalChannel".into(),
        "WM/MediaOriginalChannelSubNumber" => return "MediaOriginalChannelSubNumber".into(),
        "WM/MediaOriginalRunTime" => return "MediaOriginalRunTime".into(),
        "WM/MediaStationCallSign" => return "MediaStationCallSign".into(),
        "WM/MediaStationName" => return "MediaStationName".into(),
        "WM/MediaThumbAspectRatioX" => return "MediaThumbAspectRatioX".into(),
        "WM/MediaThumbAspectRatioY" => return "MediaThumbAspectRatioY".into(),
        "WM/MediaThumbHeight" => return "MediaThumbHeight".into(),
        "WM/MediaThumbRatingAttributes" => return "MediaThumbRatingAttributes".into(),
        "WM/MediaThumbRatingLevel" => return "MediaThumbRatingLevel".into(),
        "WM/MediaThumbRatingSystem" => return "MediaThumbRatingSystem".into(),
        "WM/MediaThumbRet" => return "MediaThumbRet".into(),
        "WM/MediaThumbStride" => return "MediaThumbStride".into(),
        "WM/MediaThumbWidth" => return "MediaThumbWidth".into(),
        "WM/OriginalReleaseTime" => return "OriginalReleaseTime".into(),
        "WM/ParentalRating" => return "ParentalRating".into(),
        "WM/ParentalRatingReason" => return "ParentalRatingReason".into(),
        "WM/Provider" => return "Provider".into(),
        "WM/ProviderCopyright" => return "ProviderCopyright".into(),
        "WM/ProviderRating" => return "ProviderRating".into(),
        "WM/SubTitle" => return "Subtitle".into(),
        "WM/SubTitleDescription" => return "SubtitleDescription".into(),
        "WM/VideoClosedCaptioning" => return "VideoClosedCaptioning".into(),
        "WM/WMRVATSCContent" => return "ATSCContent".into(),
        "WM/WMRVActualSoftPostPadding" => return "ActualSoftPostPadding".into(),
        "WM/WMRVActualSoftPrePadding" => return "ActualSoftPrePadding".into(),
        "WM/WMRVBrandingImageID" => return "BrandingImageID".into(),
        "WM/WMRVBrandingName" => return "BrandingName".into(),
        "WM/WMRVContentProtected" => return "ContentProtected".into(),
        "WM/WMRVContentProtectedPercent" => return "ContentProtectedPercent".into(),
        "WM/WMRVDTVContent" => return "DTVContent".into(),
        "WM/WMRVEncodeTime" => return "EncodeTime".into(),
        "WM/WMRVEndTime" => return "EndTime".into(),
        "WM/WMRVHDContent" => return "HDContent".into(),
        "WM/WMRVHardPostPadding" => return "HardPostPadding".into(),
        "WM/WMRVHardPrePadding" => return "HardPrePadding".into(),
        "WM/WMRVInBandRatingAttributes" => return "InBandRatingAttributes".into(),
        "WM/WMRVInBandRatingLevel" => return "InBandRatingLevel".into(),
        "WM/WMRVInBandRatingSystem" => return "InBandRatingSystem".into(),
        "WM/WMRVKeepUntil" => return "KeepUntil".into(),
        "WM/WMRVOriginalSoftPostPadding" => return "OriginalSoftPostPadding".into(),
        "WM/WMRVOriginalSoftPrePadding" => return "OriginalSoftPrePadding".into(),
        "WM/WMRVProgramID" => return "ProgramID".into(),
        "WM/WMRVQuality" => return "Quality".into(),
        "WM/WMRVRequestID" => return "RequestID".into(),
        "WM/WMRVScheduleItemID" => return "ScheduleItemID".into(),
        "WM/WMRVSeriesUID" => return "SeriesUID".into(),
        "WM/WMRVServiceID" => return "ServiceID".into(),
        "WM/WMRVWatched" => return "Watched".into(),
        _ => {}
    }

    // Unknown tags: strip leading 'WTV_Metadata_' or 'WM/WMRV' or 'WM/'
    // (mirrors: $name =~ s{^(WTV_Metadata_)?WM/(WMRV)?}{};)
    let mut name = raw;
    if let Some(rest) = name.strip_prefix("WTV_Metadata_") {
        name = rest;
    }
    if let Some(rest) = name.strip_prefix("WM/WMRV") {
        return rest.to_string();
    }
    if let Some(rest) = name.strip_prefix("WM/") {
        return rest.to_string();
    }
    name.to_string()
}

/// Convert a boolean int32 value to "Yes"/"No".
fn bool_print(val: i32) -> String {
    if val == 0 {
        "No".to_string()
    } else {
        "Yes".to_string()
    }
}

/// Convert 100ns-since-0001-01-01 to "YYYY:MM:DD HH:MM:SSZ".
///
/// Mirrors: $val / 1e7 - 719162*24*3600 -> unix time -> ConvertUnixTime
/// ConvertUnixTime rounds the fractional seconds (if frac >= 0.5, adds 1 to integer time).
fn convert_time_100ns(val: u64) -> String {
    // 719162 days from 0001-01-01 to 1970-01-01
    const EPOCH_OFFSET_SECS: i64 = 719162 * 24 * 3600;
    let float_secs = val as f64 / 1e7 - EPOCH_OFFSET_SECS as f64;
    let int_secs = float_secs.floor() as i64;
    let frac = float_secs - float_secs.floor();
    // Mirrors Perl ConvertUnixTime: sprintf('%.0f', frac) => '1' means round up
    let rounded_secs = if frac >= 0.5 { int_secs + 1 } else { int_secs };
    unix_secs_to_datetime(rounded_secs)
}

fn unix_secs_to_datetime(secs: i64) -> String {
    // Simple conversion without external deps
    // Based on: https://www.researchgate.net/publication/316558298
    if secs < 0 {
        // Before epoch - for WTV the Perl code outputs e.g. "0001:01:01 00:00:00Z" but
        // dates that far back are unusual. We'll compute properly.
        let pos = (-secs) as u64;
        let (y, mo, d, h, mi, s) = secs_to_ymd_hms(-(pos as i64));
        return format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}Z", y, mo, d, h, mi, s);
    }
    let (y, mo, d, h, mi, s) = secs_to_ymd_hms(secs);
    format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}Z", y, mo, d, h, mi, s)
}

fn secs_to_ymd_hms(secs: i64) -> (i32, u32, u32, u32, u32, u32) {
    // Compute time-of-day
    let s = secs.rem_euclid(60) as u32;
    let mins = secs.div_euclid(60);
    let mi = mins.rem_euclid(60) as u32;
    let hours = mins.div_euclid(60);
    let h = hours.rem_euclid(24) as u32;
    let days = hours.div_euclid(24); // days since 1970-01-01

    // Convert days to year/month/day using the algorithm from:
    // http://howardhinnant.github.io/date_algorithms.html (civil_from_days)
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if mo <= 2 { y + 1 } else { y } as i32;

    (year, mo, d, h, mi, s)
}

/// Convert duration in 100ns units to seconds, then format.
/// Mirrors: $val/1e7 -> ConvertDuration
fn convert_duration_100ns(val: u64) -> String {
    let total_secs = val as f64 / 1e7;
    convert_duration(total_secs)
}

/// Convert duration in seconds to a human-readable string.
/// Mirrors ExifTool's ConvertDuration():
///   - < 30s: "X.XX s"
///   - >= 30s: "H:MM:SS" (always includes hours component)
fn convert_duration(secs: f64) -> String {
    if secs == 0.0 {
        return "0 s".to_string();
    }
    let (sign, secs) = if secs < 0.0 { ("-", -secs) } else { ("", secs) };
    if secs < 30.0 {
        return format!("{}{:.2} s", sign, secs);
    }
    // Round to nearest second
    let secs = (secs + 0.5) as u64;
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = secs / 3600;
    format!("{}{}:{:02}:{:02}", sign, h, m, s)
}

/// Process WTV metadata chunk data.
/// Mirrors Perl's ProcessMetadata().
fn process_metadata(data: &[u8], tags: &mut Vec<Tag>) {
    let end = data.len();
    let mut pos = 0;

    while pos + 0x18 < end {
        // Check sentinel GUID
        if &data[pos..pos + 16] != &METADATA_ENTRY_GUID {
            break;
        }

        let fmt = match read_u32_le(data, pos + 0x10) {
            Some(v) => v,
            None => break,
        };
        let len = match read_u32_le(data, pos + 0x14) {
            Some(v) => v as usize,
            None => break,
        };

        pos += 0x18;

        // Read null-terminated UTF-16-LE name
        let mut name_bytes: Vec<u8> = Vec::new();
        loop {
            if pos + 2 > end {
                return; // truncated
            }
            let ch = &data[pos..pos + 2];
            pos += 2;
            if ch == [0, 0] {
                break;
            }
            name_bytes.extend_from_slice(ch);
        }

        if pos + len > end {
            break;
        }

        let raw_name = decode_utf16le(&name_bytes);

        // Skip tags marked as Unknown=1 in the Perl tag table (not output by default)
        if is_unknown_tag(&raw_name) {
            pos += len;
            continue;
        }

        let tag_name = map_tag_name(&raw_name);
        let value_bytes = &data[pos..pos + len];

        let tag = match fmt {
            // fmt 0 = int32u, fmt 3 = boolean32
            0 | 3 => {
                if len < 4 {
                    pos += len;
                    continue;
                }
                let int_val = match read_i32_le(value_bytes, 0) {
                    Some(v) => v,
                    None => {
                        pos += len;
                        continue;
                    }
                };
                let print = if fmt == 3 {
                    bool_print(int_val)
                } else {
                    int_val.to_string()
                };
                mk_wtv(&tag_name, Value::I32(int_val), print)
            }
            // fmt 1 = string (UTF-16-LE)
            1 => {
                let raw_s = decode_utf16le(value_bytes);
                // Apply ValueConv transformations for specific tags
                // 'WM/MediaOriginalBroadcastDateTime' and 'WM/OriginalReleaseTime':
                //   Perl: $val =~ tr/-T/: /; $val
                //   Converts ISO date "0001-01-01T00:00:00Z" -> "0001:01:01 00:00:00Z"
                let s = match tag_name.as_str() {
                    "MediaOriginalBroadcastDateTime" | "OriginalReleaseTime" => {
                        raw_s.replace('-', ":").replace('T', " ")
                    }
                    _ => raw_s,
                };
                mk_wtv(&tag_name, Value::String(s.clone()), s)
            }
            // fmt 4 = int64u (time values use this)
            4 => {
                if len < 8 {
                    pos += len;
                    continue;
                }
                let u64_val = match read_u64_le(value_bytes, 0) {
                    Some(v) => v,
                    None => {
                        pos += len;
                        continue;
                    }
                };
                // Apply value conversions for known time/duration tags
                let print = match tag_name.as_str() {
                    "Duration" => {
                        let secs = u64_val as f64 / 1e7;
                        convert_duration(secs)
                    }
                    "EncodeTime" | "EndTime" => convert_time_100ns(u64_val),
                    "MediaOriginalRunTime" => convert_duration_100ns(u64_val),
                    _ => u64_val.to_string(),
                };
                // For Duration, store float value; for int64 use Int
                let raw_val = match tag_name.as_str() {
                    "Duration" => Value::F64(u64_val as f64 / 1e7),
                    _ => Value::String(u64_val.to_string()),
                };
                mk_wtv(&tag_name, raw_val, print)
            }
            // fmt 6 = GUID
            6 => {
                let hex = value_bytes
                    .iter()
                    .map(|b| format!("{:02x}", b))
                    .collect::<String>();
                mk_wtv(&tag_name, Value::String(hex.clone()), hex)
            }
            // unknown formats - skip
            _ => {
                pos += len;
                continue;
            }
        };

        tags.push(tag);
        pos += len;
    }
}

/// Main entry point: parse a WTV file and return extracted tags.
pub fn read_wtv(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 0x60 {
        return Err(Error::InvalidData("WTV file too small".into()));
    }
    if &data[..16] != &WTV_MAGIC {
        return Err(Error::InvalidData("not a WTV file".into()));
    }

    // Sector size is at offset 0x28; constrain to 0x1000 or 0x100
    let raw_sec_size =
        u32::from_le_bytes([data[0x28], data[0x29], data[0x2a], data[0x2b]]) as usize;
    let sec_size = if raw_sec_size == 0x1000 || raw_sec_size == 0x100 {
        raw_sec_size
    } else {
        0x1000
    };

    // Read the WTV directory: sector table starts at offset 0x38 in the header
    let header = &data[..0x60.min(data.len())];
    let directory = match read_sectors(data, header, 0x38, sec_size) {
        Some(d) => d,
        None => return Err(Error::InvalidData("failed to read WTV directory".into())),
    };

    let mut tags = Vec::new();

    // Parse directory entries to find 'table.0.entries.legacy_attrib'
    let target = "table.0.entries.legacy_attrib";
    let mut pos = 0;

    while pos + 0x28 < directory.len() {
        // Check directory entry GUID
        if &directory[pos..pos + 16] != &DIR_ENTRY_GUID {
            if pos > 0 {
                break; // no more entries
            }
            // First entry doesn't match - invalid
            break;
        }

        let entry_len = match read_u32_le(&directory, pos + 0x10) {
            Some(v) => v as usize,
            None => break,
        };
        if entry_len < 0x28 || pos + entry_len > directory.len() {
            break;
        }

        let n = match read_u32_le(&directory, pos + 0x20) {
            Some(v) => v as usize,
            None => break,
        };

        // Validate
        if 0x28 + n * 2 + 8 > entry_len {
            break;
        }

        let name_end = pos + 0x28 + n * 2;
        if name_end > directory.len() {
            break;
        }
        let tag_name = decode_utf16le(&directory[pos + 0x28..name_end]);
        let ptr = name_end;

        let sec_num = match read_u32_le(&directory, ptr) {
            Some(v) => v as usize,
            None => break,
        };
        let flag = match read_u32_le(&directory, ptr + 4) {
            Some(v) => v,
            None => break,
        };

        if tag_name == target && (flag == 0 || flag == 1) {
            // Read the data for this entry
            let sec_bytes = (sec_num as u32).to_le_bytes();
            if let Some(level1) = read_sectors(data, &sec_bytes, 0, sec_size) {
                let metadata_data = if flag == 1 {
                    // flag=1: it's a sector table, read again
                    read_sectors(data, &level1, 0, sec_size)
                } else {
                    Some(level1)
                };

                if let Some(md) = metadata_data {
                    process_metadata(&md, &mut tags);
                }
            }
        }

        pos += entry_len;
    }

    Ok(tags)
}
