//! ICC Color Profile reader.
//!
//! Parses ICC profile header for color space and rendering intent info.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_icc(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 128 || &data[36..40] != b"acsp" {
        return Err(Error::InvalidData("not an ICC profile".into()));
    }

    let mut tags = Vec::new();

    let profile_size = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    tags.push(mk("ProfileSize", "Profile Size", Value::U32(profile_size)));

    let preferred_cmm = String::from_utf8_lossy(&data[4..8]).trim().to_string();
    if !preferred_cmm.is_empty() && preferred_cmm != "\0\0\0\0" {
        tags.push(mk("PreferredCMM", "Preferred CMM", Value::String(preferred_cmm)));
    }

    let major = data[8];
    let minor = (data[9] >> 4) & 0x0F;
    let patch = data[9] & 0x0F;
    tags.push(mk("ProfileVersion", "Profile Version", Value::String(format!("{}.{}.{}", major, minor, patch))));

    let device_class = String::from_utf8_lossy(&data[12..16]).to_string();
    let class_name = match device_class.trim() {
        "scnr" => "Input Device",
        "mntr" => "Display Device",
        "prtr" => "Output Device",
        "link" => "Device Link",
        "spac" => "Color Space Conversion",
        "abst" => "Abstract",
        "nmcl" => "Named Color",
        _ => &device_class,
    };
    tags.push(mk("DeviceClass", "Device Class", Value::String(class_name.to_string())));

    let color_space = String::from_utf8_lossy(&data[16..20]).trim().to_string();
    let cs_name = match color_space.as_str() {
        "XYZ" => "XYZ",
        "Lab" => "Lab",
        "Luv" => "Luv",
        "YCbr" => "YCbCr",
        "Yxy" => "Yxy",
        "RGB" => "RGB",
        "GRAY" => "Grayscale",
        "HSV" => "HSV",
        "HLS" => "HLS",
        "CMYK" => "CMYK",
        "CMY" => "CMY",
        _ => &color_space,
    };
    tags.push(mk("ColorSpaceData", "Color Space", Value::String(cs_name.to_string())));

    let pcs = String::from_utf8_lossy(&data[20..24]).trim().to_string();
    tags.push(mk("ProfileConnectionSpace", "Connection Space", Value::String(pcs)));

    // Creation date (bytes 24-35): year(2), month(2), day(2), hour(2), minute(2), second(2)
    let year = u16::from_be_bytes([data[24], data[25]]);
    let month = u16::from_be_bytes([data[26], data[27]]);
    let day = u16::from_be_bytes([data[28], data[29]]);
    let hour = u16::from_be_bytes([data[30], data[31]]);
    let min = u16::from_be_bytes([data[32], data[33]]);
    let sec = u16::from_be_bytes([data[34], data[35]]);
    tags.push(mk(
        "ProfileDateTime",
        "Profile Date/Time",
        Value::String(format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}", year, month, day, hour, min, sec)),
    ));

    // Primary platform (bytes 40-43)
    let platform = String::from_utf8_lossy(&data[40..44]).trim().to_string();
    let platform_name = match platform.as_str() {
        "APPL" => "Apple",
        "MSFT" => "Microsoft",
        "SGI " => "SGI",
        "SUNW" => "Sun Microsystems",
        _ => &platform,
    };
    if !platform_name.is_empty() {
        tags.push(mk("PrimaryPlatform", "Primary Platform", Value::String(platform_name.to_string())));
    }

    // Rendering intent (byte 67)
    let intent = u32::from_be_bytes([data[64], data[65], data[66], data[67]]);
    let intent_name = match intent {
        0 => "Perceptual",
        1 => "Media-Relative Colorimetric",
        2 => "Saturation",
        3 => "ICC-Absolute Colorimetric",
        _ => "Unknown",
    };
    tags.push(mk("RenderingIntent", "Rendering Intent", Value::String(intent_name.to_string())));

    // Device manufacturer (bytes 48-51) and model (52-55)
    let manufacturer = String::from_utf8_lossy(&data[48..52]).trim().to_string();
    if !manufacturer.is_empty() && manufacturer.bytes().any(|b| b > 0x20) {
        tags.push(mk("DeviceManufacturer", "Device Manufacturer", Value::String(manufacturer)));
    }

    // Profile description tag - search in tag table
    if data.len() >= 132 {
        let tag_count = u32::from_be_bytes([data[128], data[129], data[130], data[131]]) as usize;
        let mut tpos = 132;
        for _ in 0..tag_count.min(100) {
            if tpos + 12 > data.len() {
                break;
            }
            let sig = &data[tpos..tpos + 4];
            let offset = u32::from_be_bytes([data[tpos + 4], data[tpos + 5], data[tpos + 6], data[tpos + 7]]) as usize;
            let size = u32::from_be_bytes([data[tpos + 8], data[tpos + 9], data[tpos + 10], data[tpos + 11]]) as usize;
            tpos += 12;

            if sig == b"desc" && offset + size <= data.len() && size > 12 {
                // 'desc' type: 4 bytes signature + 4 reserved + 4 bytes string length + string
                let d = &data[offset..offset + size];
                if d.len() >= 12 && &d[0..4] == b"desc" {
                    let str_len = u32::from_be_bytes([d[8], d[9], d[10], d[11]]) as usize;
                    if 12 + str_len <= d.len() {
                        let desc = String::from_utf8_lossy(&d[12..12 + str_len])
                            .trim_end_matches('\0')
                            .to_string();
                        if !desc.is_empty() {
                            tags.push(mk("ProfileDescription", "Profile Description", Value::String(desc)));
                        }
                    }
                }
            }

            if sig == b"cprt" && offset + size <= data.len() && size > 8 {
                let d = &data[offset..offset + size];
                if d.len() >= 8 && &d[0..4] == b"text" {
                    let text = String::from_utf8_lossy(&d[8..])
                        .trim_end_matches('\0')
                        .to_string();
                    if !text.is_empty() {
                        tags.push(mk("ProfileCopyright", "Profile Copyright", Value::String(text)));
                    }
                }
            }
        }
    }

    Ok(tags)
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "ICC_Profile".into(),
            family1: "ICC_Profile".into(),
            family2: "Image".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}
