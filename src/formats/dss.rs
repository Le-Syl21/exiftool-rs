//! Digital Speech Standard format reader.

use super::misc::mktag;
use crate::error::{Error, Result};
use crate::tag::Tag;
use crate::value::Value;

/// Parse DSS (Digital Speech Standard) voice recorder files.
/// Mirrors ExifTool's Olympus::ProcessDSS().
pub fn read_dss(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 68 {
        return Err(Error::InvalidData("DSS file too small".into()));
    }
    // Magic: \x02dss or \x03ds2
    if !(data[0] == 0x02 || data[0] == 0x03)
        || data[1] != b'd'
        || data[2] != b's'
        || (data[3] != b's' && data[3] != b'2')
    {
        return Err(Error::InvalidData("not a DSS/DS2 file".into()));
    }

    let mut tags = Vec::new();

    // Offset 12: Model, string[16]
    if data.len() >= 28 {
        let model_bytes = &data[12..28];
        let model = crate::encoding::decode_utf8_or_latin1(model_bytes)
            .trim_end_matches('\0')
            .trim()
            .to_string();
        if !model.is_empty() {
            tags.push(mktag(
                "Olympus",
                "Model",
                "Camera Model Name",
                Value::String(model),
            ));
        }
    }

    // Offset 38: StartTime, string[12] — format YYMMDDHHMMSS
    if data.len() >= 50 {
        let st_bytes = &data[38..50];
        let st_str = crate::encoding::decode_utf8_or_latin1(st_bytes);
        if let Some(dt) = parse_dss_time(&st_str) {
            tags.push(mktag(
                "Olympus",
                "StartTime",
                "Start Time",
                Value::String(dt),
            ));
        }
    }

    // Offset 50: EndTime, string[12]
    if data.len() >= 62 {
        let et_bytes = &data[50..62];
        let et_str = crate::encoding::decode_utf8_or_latin1(et_bytes);
        if let Some(dt) = parse_dss_time(&et_str) {
            tags.push(mktag("Olympus", "EndTime", "End Time", Value::String(dt)));
        }
    }

    // Offset 62: Duration, string[6] — format HHMMSS
    if data.len() >= 68 {
        let dur_bytes = &data[62..68];
        let dur_str = crate::encoding::decode_utf8_or_latin1(dur_bytes);
        if let Some(dur_secs) = parse_dss_duration(&dur_str) {
            let dur_display = dss_convert_duration(dur_secs);
            tags.push(mktag(
                "Olympus",
                "Duration",
                "Duration",
                Value::String(dur_display),
            ));
        }
    }

    Ok(tags)
}

/// Parse DSS time string YYMMDDHHMMSS → "20YY:MM:DD HH:MM:SS"
fn parse_dss_time(s: &str) -> Option<String> {
    let s = s.trim_matches('\0');
    if s.len() < 12 {
        return None;
    }
    let yy = &s[0..2];
    let mm = &s[2..4];
    let dd = &s[4..6];
    let hh = &s[6..8];
    let mi = &s[8..10];
    let ss = &s[10..12];
    // Validate digits
    if !yy.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(format!("20{}:{}:{} {}:{}:{}", yy, mm, dd, hh, mi, ss))
}

/// Parse DSS duration string HHMMSS → seconds
fn parse_dss_duration(s: &str) -> Option<f64> {
    let s = s.trim_matches('\0');
    if s.len() < 6 {
        return None;
    }
    let hh: u64 = s[0..2].parse().ok()?;
    let mm: u64 = s[2..4].parse().ok()?;
    let ss: u64 = s[4..6].parse().ok()?;
    Some(((hh * 60 + mm) * 60 + ss) as f64)
}

/// Convert duration in seconds to display string (mirrors ExifTool's ConvertDuration).
fn dss_convert_duration(secs: f64) -> String {
    if secs == 0.0 {
        return "0 s".to_string();
    }
    if secs < 30.0 {
        return format!("{:.2} s", secs);
    }
    let secs_u = (secs + 0.5) as u64;
    let h = secs_u / 3600;
    let m = (secs_u % 3600) / 60;
    let s = secs_u % 60;
    format!("{}:{:02}:{:02}", h, m, s)
}
