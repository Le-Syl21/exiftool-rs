//! Digital Video (DV) format reader.
//!
//! Reads metadata from raw DV files.
//! Mirrors ExifTool's DV.pm ProcessDV().

use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

fn mk(name: &str, value: Value, print: String) -> Tag {
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: "DV".into(),
            family1: "DV".into(),
            family2: "Video".into(),
        },
        raw_value: value.clone(),
        print_value: print,
        priority: 0,
    }
}

fn mk_str(name: &str, s: &str) -> Tag {
    mk(name, Value::String(s.to_string()), s.to_string())
}

struct DvProfile {
    dsf: u8,
    video_stype: u8,
    frame_size: u32,
    video_format: &'static str,
    colorimetry: &'static str,
    frame_rate: f64,
    image_height: u32,
    image_width: u32,
}

const DV_PROFILES: &[DvProfile] = &[
    DvProfile {
        dsf: 0,
        video_stype: 0x0,
        frame_size: 120000,
        video_format: "IEC 61834, SMPTE-314M - 525/60 (NTSC)",
        colorimetry: "4:1:1",
        frame_rate: 30000.0 / 1001.0,
        image_height: 480,
        image_width: 720,
    },
    DvProfile {
        dsf: 1,
        video_stype: 0x0,
        frame_size: 144000,
        video_format: "IEC 61834 - 625/50 (PAL)",
        colorimetry: "4:2:0",
        frame_rate: 25.0,
        image_height: 576,
        image_width: 720,
    },
    DvProfile {
        dsf: 1,
        video_stype: 0x0,
        frame_size: 144000,
        video_format: "SMPTE-314M - 625/50 (PAL)",
        colorimetry: "4:1:1",
        frame_rate: 25.0,
        image_height: 576,
        image_width: 720,
    },
    DvProfile {
        dsf: 0,
        video_stype: 0x4,
        frame_size: 240000,
        video_format: "DVCPRO50: SMPTE-314M - 525/60 (NTSC) 50 Mbps",
        colorimetry: "4:2:2",
        frame_rate: 30000.0 / 1001.0,
        image_height: 480,
        image_width: 720,
    },
    DvProfile {
        dsf: 1,
        video_stype: 0x4,
        frame_size: 288000,
        video_format: "DVCPRO50: SMPTE-314M - 625/50 (PAL) 50 Mbps",
        colorimetry: "4:2:2",
        frame_rate: 25.0,
        image_height: 576,
        image_width: 720,
    },
    DvProfile {
        dsf: 0,
        video_stype: 0x14,
        frame_size: 480000,
        video_format: "DVCPRO HD: SMPTE-370M - 1080i60 100 Mbps",
        colorimetry: "4:2:2",
        frame_rate: 30000.0 / 1001.0,
        image_height: 1080,
        image_width: 1280,
    },
    DvProfile {
        dsf: 1,
        video_stype: 0x14,
        frame_size: 576000,
        video_format: "DVCPRO HD: SMPTE-370M - 1080i50 100 Mbps",
        colorimetry: "4:2:2",
        frame_rate: 25.0,
        image_height: 1080,
        image_width: 1440,
    },
    DvProfile {
        dsf: 0,
        video_stype: 0x18,
        frame_size: 240000,
        video_format: "DVCPRO HD: SMPTE-370M - 720p60 100 Mbps",
        colorimetry: "4:2:2",
        frame_rate: 60000.0 / 1001.0,
        image_height: 720,
        image_width: 960,
    },
    DvProfile {
        dsf: 1,
        video_stype: 0x18,
        frame_size: 288000,
        video_format: "DVCPRO HD: SMPTE-370M - 720p50 100 Mbps",
        colorimetry: "4:2:2",
        frame_rate: 50.0,
        image_height: 720,
        image_width: 960,
    },
    DvProfile {
        dsf: 1,
        video_stype: 0x1,
        frame_size: 144000,
        video_format: "IEC 61883-5 - 625/50 (PAL)",
        colorimetry: "4:2:0",
        frame_rate: 25.0,
        image_height: 576,
        image_width: 720,
    },
];

/// Format bitrate in the way Perl ConvertBitrate does (e.g., "28.8 Mbps")
fn format_bitrate(bps: f64) -> String {
    if bps >= 1_000_000.0 {
        let mbps = bps / 1_000_000.0;
        // Round to 1 decimal
        format!("{} Mbps", format_decimal(mbps, 1))
    } else if bps >= 1_000.0 {
        format!("{} kbps", format_decimal(bps / 1_000.0, 1))
    } else {
        format!("{} bps", bps as u64)
    }
}

fn format_decimal(v: f64, decimals: usize) -> String {
    let factor = 10f64.powi(decimals as i32);
    let rounded = (v * factor).round() / factor;
    if rounded.fract() == 0.0 {
        format!("{}", rounded as i64)
    } else {
        format!("{:.1$}", rounded, decimals)
    }
}

/// Format duration in Perl ConvertDuration style (e.g., "0.00 s")
fn format_duration(secs: f64) -> String {
    if secs < 60.0 {
        format!("{:.2} s", secs)
    } else if secs < 3600.0 {
        format!("{}:{:05.2}", secs as u64 / 60, secs % 60.0)
    } else {
        format!(
            "{}:{:02}:{:05.2}",
            secs as u64 / 3600,
            (secs as u64 % 3600) / 60,
            secs % 60.0
        )
    }
}

pub fn read_dv(data: &[u8], file_size: u64) -> crate::error::Result<Vec<Tag>> {
    if data.len() < 480 {
        // Need at least 6 DIF blocks (6 * 80 bytes) after start offset
        return Ok(Vec::new());
    }

    // Find the DIF header start
    let start = find_dif_start(data)?;
    let start = match start {
        Some(s) => s,
        None => return Ok(Vec::new()),
    };

    if start + 80 * 6 > data.len() {
        return Ok(Vec::new());
    }

    let dsf = (data[start + 3] & 0x80) >> 7;
    let stype = data[start + 80 * 5 + 48 + 3] & 0x1f;

    // 576i50 25Mbps 4:1:1 is a special case
    let profile = if dsf == 1 && stype == 0 && (data[start + 4] & 0x07) != 0 {
        &DV_PROFILES[2]
    } else {
        let found = DV_PROFILES
            .iter()
            .find(|p| p.dsf == dsf && p.video_stype == stype);
        match found {
            Some(p) => p,
            None => return Ok(Vec::new()),
        }
    };

    let mut tags = Vec::new();

    // Extract video metadata from VAUX DIFs
    let mut date: Option<String> = None;
    let mut time: Option<String> = None;
    let mut is16_9: Option<bool> = None;
    let mut interlace: Option<bool> = None;

    let mut pos = start;
    for _i in 1..6usize {
        pos += 80;
        if pos >= data.len() {
            break;
        }
        let block_type = data[pos];
        if (block_type & 0xf0) != 0x50 {
            continue;
        } // VAUX type

        for j in 0..15usize {
            let p = pos + j * 5 + 3;
            if p + 4 >= data.len() {
                break;
            }
            let t = data[p];
            if t == 0x61 {
                // video control
                let apt = data[start + 4] & 0x07;
                let tv = data[p + 2];
                let aspect = (tv & 0x07) == 0x02 || (apt == 0 && (tv & 0x07) == 0x07);
                is16_9 = Some(aspect);
                interlace = Some((data[p + 3] & 0x10) != 0);
            } else if t == 0x62 {
                // date
                let _d0 = data[p + 1];
                let d1 = data[p + 2];
                let d2 = data[p + 3];
                let d3 = data[p + 4];
                let year_bcd = d3;
                let month_bcd = d2 & 0x1f;
                let day_bcd = d1 & 0x3f;
                let year_str = format!("{:02x}", year_bcd);
                let month_str = format!("{:02x}", month_bcd);
                let day_str = format!("{:02x}", day_bcd);
                let date_raw = format!("{}:{}:{}", year_str, month_str, day_str);
                // check for invalid (BCD digits > 9)
                if date_raw.chars().any(|c| c.is_ascii_lowercase()) {
                    date = None;
                } else {
                    let year_prefix = if year_str.as_str() < "90" { "20" } else { "19" };
                    date = Some(format!("{}{}", year_prefix, date_raw));
                }
                time = None;
            } else if t == 0x63 && date.is_some() {
                // time: bytes at p+1..p+4 are [frames, seconds, minutes, hours] (BCD)
                // Perl: $t[3]=p+4=hours & 0x3f, $t[2]=p+3=minutes & 0x7f, $t[1]=p+2=seconds & 0x7f
                let hours = data[p + 4] & 0x3f;
                let minutes = data[p + 3] & 0x7f;
                let seconds = data[p + 2] & 0x7f;
                time = Some(format!("{:02x}:{:02x}:{:02x}", hours, minutes, seconds));
                break;
            } else {
                time = None;
            }
        }
    }

    if let (Some(d), Some(ti)) = (&date, &time) {
        tags.push(mk_str("DateTimeOriginal", &format!("{} {}", d, ti)));
    }

    tags.push(mk(
        "ImageWidth",
        Value::U32(profile.image_width),
        profile.image_width.to_string(),
    ));
    tags.push(mk(
        "ImageHeight",
        Value::U32(profile.image_height),
        profile.image_height.to_string(),
    ));

    // Calculate duration and bitrate
    let byte_rate = profile.frame_size as f64 * profile.frame_rate;
    let total_bitrate = 8.0 * byte_rate;
    let duration = file_size as f64 / byte_rate;

    tags.push(mk(
        "Duration",
        Value::String(format!("{:.10}", duration)),
        format_duration(duration),
    ));
    tags.push(mk(
        "TotalBitrate",
        Value::String(format!("{}", total_bitrate as u64)),
        format_bitrate(total_bitrate),
    ));
    tags.push(mk_str("VideoFormat", profile.video_format));

    // VideoScanType and AspectRatio (only if date/time was found)
    if date.is_some() && time.is_some() {
        if let Some(il) = interlace {
            let scan = if il { "Interlaced" } else { "Progressive" };
            tags.push(mk_str("VideoScanType", scan));
        }
    }

    // FrameRate
    let fr = profile.frame_rate;
    let fr_rounded = (fr * 1000.0 + 0.5) as i64 as f64 / 1000.0;
    let fr_str = if fr_rounded.fract() == 0.0 {
        format!("{}", fr_rounded as i64)
    } else {
        format!("{}", fr_rounded)
    };
    tags.push(mk("FrameRate", Value::String(fr_str.clone()), fr_str));

    // AspectRatio (only if date/time was found)
    if date.is_some() && time.is_some() {
        if let Some(is16) = is16_9 {
            let ar = if is16 { "16:9" } else { "4:3" };
            tags.push(mk_str("AspectRatio", ar));
        }
    }

    tags.push(mk_str("Colorimetry", profile.colorimetry));

    // Audio info from AAUX DIF
    let audio_pos = start + 80 * 6 + 80 * 16 * 3 + 3;
    if audio_pos + 4 < data.len() && data[audio_pos] == 0x50 {
        let _smpls = data[audio_pos + 1];
        let freq = (data[audio_pos + 4] >> 3) & 0x07;
        let atype = data[audio_pos + 3] & 0x1f;
        let quant = data[audio_pos + 4] & 0x07;

        if freq < 3 {
            let sample_rate = match freq {
                0 => 48000u32,
                1 => 44100,
                2 => 32000,
                _ => unreachable!(),
            };
            tags.push(mk(
                "AudioSampleRate",
                Value::U32(sample_rate),
                sample_rate.to_string(),
            ));
        }

        let atype2 = if atype == 0 && quant != 0 && freq == 2 {
            2
        } else {
            atype
        };
        if atype2 < 4 {
            let channels = match atype2 {
                0 => 2u32,
                1 => 0,
                2 => 4,
                3 => 8,
                _ => 0,
            };
            tags.push(mk(
                "AudioChannels",
                Value::U32(channels),
                channels.to_string(),
            ));
        }

        let bits = if quant != 0 { 12u32 } else { 16 };
        tags.push(mk("AudioBitsPerSample", Value::U32(bits), bits.to_string()));
    }

    Ok(tags)
}

fn find_dif_start(data: &[u8]) -> crate::error::Result<Option<usize>> {
    // Try pattern 1: \x1f\x07\x00[\x3f\xbf]
    for i in 0..data.len().saturating_sub(4) {
        if data[i] == 0x1f
            && data[i + 1] == 0x07
            && data[i + 2] == 0x00
            && (data[i + 3] == 0x3f || data[i + 3] == 0xbf)
        {
            return Ok(Some(i.saturating_sub(i % 80)));
        }
    }

    // Try pattern 2: look for sync sequence
    let pat1 = [0x3f, 0x07, 0x00];
    for i in 0..data.len().saturating_sub(167) {
        let matches = (data[i] == 0x00 || data[i] == 0xff)
            && data[i + 1..i + 4] == pat1
            && i + 167 < data.len()
            && data[i + 81..i + 85] == [0xff, 0x3f, 0x07, 0x01];
        if matches {
            let start = i.saturating_sub(163);
            if start + 163 < data.len() {
                return Ok(Some(start));
            }
        }
    }

    Ok(None)
}
