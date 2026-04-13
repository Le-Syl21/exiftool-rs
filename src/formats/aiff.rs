//! AIFF/AIFC audio file format reader.
//!
//! Parses IFF chunks: COMM (audio info), NAME, AUTH, comments.
//! Mirrors ExifTool's AIFF.pm.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_aiff(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 12
        || !data.starts_with(b"FORM")
        || (&data[8..12] != b"AIFF" && &data[8..12] != b"AIFC")
    {
        return Err(Error::InvalidData("not an AIFF file".into()));
    }

    let mut tags = Vec::new();
    let is_compressed = &data[8..12] == b"AIFC";

    let mut pos = 12;

    while pos + 8 <= data.len() {
        let chunk_id = &data[pos..pos + 4];
        let chunk_size =
            u32::from_be_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]])
                as usize;
        pos += 8;

        if pos + chunk_size > data.len() {
            break;
        }

        let cd = &data[pos..pos + chunk_size];

        match chunk_id {
            // Common chunk
            b"COMM" => {
                if cd.len() >= 18 {
                    let channels = i16::from_be_bytes([cd[0], cd[1]]);
                    let num_frames = u32::from_be_bytes([cd[2], cd[3], cd[4], cd[5]]);
                    let bits_per_sample = i16::from_be_bytes([cd[6], cd[7]]);
                    let sample_rate = decode_ieee_extended(&cd[8..18]);

                    tags.push(mk(
                        "NumChannels",
                        "Number of Channels",
                        Value::I16(channels),
                    ));
                    tags.push(mk(
                        "NumSampleFrames",
                        "Number of Sample Frames",
                        Value::U32(num_frames),
                    ));
                    tags.push(mk("SampleSize", "Sample Size", Value::I16(bits_per_sample)));
                    tags.push(mk(
                        "SampleRate",
                        "Sample Rate",
                        Value::U32(sample_rate as u32),
                    ));

                    if sample_rate > 0.0 && num_frames > 0 {
                        let duration = num_frames as f64 / sample_rate;
                        // Perl ConvertDuration: < 30s → "{:.2} s", else "h:mm:ss"
                        let dur_str = if duration < 30.0 {
                            format!("{:.2} s", duration)
                        } else {
                            let dur_u = (duration + 0.5) as u64;
                            let h = dur_u / 3600;
                            let m = (dur_u % 3600) / 60;
                            let s = dur_u % 60;
                            format!("{}:{:02}:{:02}", h, m, s)
                        };
                        tags.push(mk("Duration", "Duration", Value::String(dur_str)));
                    }

                    // AIFC compression type
                    if is_compressed && cd.len() >= 22 {
                        let comp_type = crate::encoding::decode_utf8_or_latin1(&cd[18..22])
                            .trim()
                            .to_string();
                        let comp_name = match comp_type.as_str() {
                            "NONE" | "none" => "None",
                            "sowt" => "Little-endian PCM",
                            "fl32" | "FL32" => "32-bit Float",
                            "fl64" | "FL64" => "64-bit Float",
                            "alaw" | "ALAW" => "A-Law",
                            "ulaw" | "ULAW" => "mu-Law",
                            "ima4" | "IMA4" => "IMA ADPCM",
                            _ => &comp_type,
                        };
                        tags.push(mk(
                            "Compression",
                            "Compression",
                            Value::String(comp_name.to_string()),
                        ));
                    }
                }
            }
            b"NAME" => {
                let name = crate::encoding::decode_utf8_or_latin1(cd)
                    .trim_end_matches('\0')
                    .to_string();
                if !name.is_empty() {
                    tags.push(mk("Name", "Name", Value::String(name)));
                }
            }
            b"AUTH" => {
                let author = crate::encoding::decode_utf8_or_latin1(cd)
                    .trim_end_matches('\0')
                    .to_string();
                if !author.is_empty() {
                    tags.push(mk("Author", "Author", Value::String(author)));
                }
            }
            b"(c) " => {
                let copyright = crate::encoding::decode_utf8_or_latin1(cd)
                    .trim_end_matches('\0')
                    .to_string();
                if !copyright.is_empty() {
                    tags.push(mk("Copyright", "Copyright", Value::String(copyright)));
                }
            }
            b"ANNO" => {
                let annotation = crate::encoding::decode_utf8_or_latin1(cd)
                    .trim_end_matches('\0')
                    .to_string();
                if !annotation.is_empty() {
                    tags.push(mk("Annotation", "Annotation", Value::String(annotation)));
                }
            }
            // COMT: Comment chunk with timestamp
            b"COMT" => {
                if cd.len() >= 2 {
                    let num_comments = u16::from_be_bytes([cd[0], cd[1]]) as usize;
                    let mut p = 2;
                    for _ in 0..num_comments {
                        if p + 8 > cd.len() {
                            break;
                        }
                        let ts = u32::from_be_bytes([cd[p], cd[p + 1], cd[p + 2], cd[p + 3]]);
                        // marker ID at p+4..p+6 (skipped)
                        let size = u16::from_be_bytes([cd[p + 6], cd[p + 7]]) as usize;
                        p += 8;
                        // CommentTime: Mac epoch (seconds since 1904-01-01)
                        // ValueConv: ConvertUnixTime($val - ((66 * 365 + 17) * 24 * 3600))
                        let mac_offset: u64 = (66 * 365 + 17) * 24 * 3600;
                        if ts as u64 >= mac_offset {
                            let unix_ts = ts as u64 - mac_offset;
                            let dt = aiff_unix_to_datetime(unix_ts as i64);
                            tags.push(mk("CommentTime", "Comment Time", Value::String(dt)));
                        }
                        if p + size <= cd.len() && size > 0 {
                            let comment = crate::encoding::decode_utf8_or_latin1(&cd[p..p + size])
                                .trim_end_matches('\0')
                                .to_string();
                            if !comment.is_empty() {
                                tags.push(mk("Comment", "Comment", Value::String(comment)));
                            }
                        }
                        let size_padded = size + (size & 1);
                        p += size_padded;
                    }
                }
            }
            // ID3 tags embedded in AIFF
            b"ID3 " => {
                if cd.starts_with(b"ID3") {
                    if let Ok(id3_tags) = crate::formats::id3::read_mp3(cd) {
                        tags.extend(id3_tags);
                    }
                }
            }
            _ => {}
        }

        pos += chunk_size;
        // Pad to even boundary
        if chunk_size % 2 != 0 {
            pos += 1;
        }
    }

    Ok(tags)
}

/// Decode 80-bit IEEE 754 extended precision float (10 bytes, big-endian).
fn decode_ieee_extended(data: &[u8]) -> f64 {
    if data.len() < 10 {
        return 0.0;
    }

    let exponent = (((data[0] as u16) & 0x7F) << 8) | data[1] as u16;
    let sign = if data[0] & 0x80 != 0 { -1.0 } else { 1.0 };
    let mantissa = ((data[2] as u64) << 56)
        | ((data[3] as u64) << 48)
        | ((data[4] as u64) << 40)
        | ((data[5] as u64) << 32)
        | ((data[6] as u64) << 24)
        | ((data[7] as u64) << 16)
        | ((data[8] as u64) << 8)
        | data[9] as u64;

    if exponent == 0 && mantissa == 0 {
        return 0.0;
    }

    let f = mantissa as f64 / (1u64 << 63) as f64;
    sign * f * 2.0_f64.powi(exponent as i32 - 16383)
}

/// Convert Unix timestamp to "YYYY:MM:DD HH:MM:SS" (UTC, without timezone suffix).
/// Mirrors Perl's ConvertUnixTime($val) for AIFF CommentTime.
fn aiff_unix_to_datetime(secs: i64) -> String {
    let days = secs / 86400;
    let time = secs % 86400;
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
    let mut mo = 1i32;
    for &dm in &months {
        if rem < dm {
            break;
        }
        rem -= dm;
        mo += 1;
    }
    format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
        y,
        mo,
        rem + 1,
        h,
        m,
        s
    )
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let print_value = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "AIFF".into(),
            family1: "AIFF".into(),
            family2: "Audio".into(),
        },
        raw_value: value,
        print_value,
        priority: 0,
    }
}
