//! MOI (camcorder info file) format reader.

use super::misc::mktag;
use crate::error::{Error, Result};
use crate::tag::Tag;
use crate::value::Value;

/// Parse MOI (camcorder info) files. Mirrors ExifTool's MOI.pm.
pub fn read_moi(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 256 || !data.starts_with(b"V6") {
        return Err(Error::InvalidData("not a MOI file".into()));
    }

    let mut tags = Vec::new();

    // 0x00: MOIVersion (string[2])
    let version = crate::encoding::decode_utf8_or_latin1(&data[0..2]).to_string();
    tags.push(mktag(
        "MOI",
        "MOIVersion",
        "MOI Version",
        Value::String(version),
    ));

    // 0x06: DateTimeOriginal (undef[8]) = unpack 'nCCCCn'
    // year(u16), month(u8), day(u8), hour(u8), min(u8), ms*1000(u16)
    if data.len() >= 14 {
        let year = u16::from_be_bytes([data[6], data[7]]);
        let month = data[8];
        let day = data[9];
        let hour = data[10];
        let min = data[11];
        let ms = u16::from_be_bytes([data[12], data[13]]);
        let sec_f = ms as f64 / 1000.0;
        let dt = format!(
            "{:04}:{:02}:{:02} {:02}:{:02}:{:06.3}",
            year, month, day, hour, min, sec_f
        );
        tags.push(mktag(
            "MOI",
            "DateTimeOriginal",
            "Date/Time Original",
            Value::String(dt),
        ));
    }

    // 0x0e: Duration (int32u, ms)
    if data.len() >= 0x12 {
        let dur_ms = u32::from_be_bytes([data[0x0e], data[0x0f], data[0x10], data[0x11]]);
        let dur_s = dur_ms as f64 / 1000.0;
        let dur_str = format!("{:.2} s", dur_s);
        tags.push(mktag("MOI", "Duration", "Duration", Value::String(dur_str)));
    }

    // 0x80: AspectRatio (int8u)
    if data.len() > 0x80 {
        let aspect = data[0x80];
        let lo = aspect & 0x0F;
        let hi = aspect >> 4;
        let aspect_str = match lo {
            0 | 1 => "4:3",
            4 | 5 => "16:9",
            _ => "Unknown",
        };
        let sys_str = match hi {
            4 => " NTSC",
            5 => " PAL",
            _ => "",
        };
        let full = format!("{}{}", aspect_str, sys_str);
        tags.push(mktag(
            "MOI",
            "AspectRatio",
            "Aspect Ratio",
            Value::String(full),
        ));
    }

    // 0x84: AudioCodec (int16u)
    if data.len() > 0x86 {
        let ac = u16::from_be_bytes([data[0x84], data[0x85]]);
        let codec = match ac {
            0x00c1 => "AC3",
            0x4001 => "MPEG",
            _ => "Unknown",
        };
        tags.push(mktag(
            "MOI",
            "AudioCodec",
            "Audio Codec",
            Value::String(codec.into()),
        ));
    }

    // 0x86: AudioBitrate (int8u, val * 16000 + 48000)
    if data.len() > 0x86 {
        let ab = data[0x86];
        let bitrate = ab as u32 * 16000 + 48000;
        let bitrate_str = format!("{} kbps", bitrate / 1000);
        tags.push(mktag(
            "MOI",
            "AudioBitrate",
            "Audio Bitrate",
            Value::String(bitrate_str),
        ));
    }

    // 0xda: VideoBitrate (int16u with lookup)
    if data.len() > 0xdc {
        let vb = u16::from_be_bytes([data[0xda], data[0xdb]]);
        let vbps: Option<u32> = match vb {
            0x5896 => Some(8500000),
            0x813d => Some(5500000),
            _ => None,
        };
        if let Some(bps) = vbps {
            let vb_str = format!("{:.1} Mbps", bps as f64 / 1_000_000.0);
            tags.push(mktag(
                "MOI",
                "VideoBitrate",
                "Video Bitrate",
                Value::String(vb_str),
            ));
        }
    }

    Ok(tags)
}
