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

    let preferred_cmm = crate::encoding::decode_utf8_or_latin1(&data[4..8])
        .trim()
        .to_string();
    if !preferred_cmm.is_empty() && preferred_cmm != "\0\0\0\0" {
        tags.push(mk(
            "ProfileCMMType",
            "Profile CMM Type",
            Value::String(preferred_cmm),
        ));
    }

    let major = data[8];
    let minor = (data[9] >> 4) & 0x0F;
    let patch = data[9] & 0x0F;
    tags.push(mk(
        "ProfileVersion",
        "Profile Version",
        Value::String(format!("{}.{}.{}", major, minor, patch)),
    ));

    let device_class = crate::encoding::decode_utf8_or_latin1(&data[12..16]).to_string();
    let class_name = match device_class.trim() {
        "scnr" => "Input Device Profile",
        "mntr" => "Display Device Profile",
        "prtr" => "Output Device Profile",
        "link" => "DeviceLink Profile",
        "spac" => "ColorSpace Conversion Profile",
        "abst" => "Abstract Profile",
        "nmcl" => "Named Color Profile",
        _ => &device_class,
    };
    tags.push(mk(
        "ProfileClass",
        "Profile Class",
        Value::String(class_name.to_string()),
    ));

    let color_space = crate::encoding::decode_utf8_or_latin1(&data[16..20])
        .trim()
        .to_string();
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
    tags.push(mk(
        "ColorSpaceData",
        "Color Space",
        Value::String(cs_name.to_string()),
    ));

    let pcs = crate::encoding::decode_utf8_or_latin1(&data[20..24])
        .trim()
        .to_string();
    tags.push(mk(
        "ProfileConnectionSpace",
        "Connection Space",
        Value::String(pcs),
    ));

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
        Value::String(format!(
            "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
            year, month, day, hour, min, sec
        )),
    ));

    // Primary platform (bytes 40-43)
    let platform = crate::encoding::decode_utf8_or_latin1(&data[40..44])
        .trim()
        .to_string();
    let platform_name = match platform.as_str() {
        "APPL" => "Apple",
        "MSFT" => "Microsoft",
        "SGI " => "SGI",
        "SUNW" => "Sun Microsystems",
        _ => &platform,
    };
    if !platform_name.is_empty() {
        tags.push(mk(
            "PrimaryPlatform",
            "Primary Platform",
            Value::String(platform_name.to_string()),
        ));
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
    tags.push(mk(
        "RenderingIntent",
        "Rendering Intent",
        Value::String(intent_name.to_string()),
    ));

    // CMMFlags (bytes 44-47)
    let flags = u32::from_be_bytes([data[44], data[45], data[46], data[47]]);
    tags.push(mk(
        "CMMFlags",
        "CMM Flags",
        Value::String(format!("0x{:08X}", flags)),
    ));

    // DeviceAttributes (bytes 56-63)
    let attr = u64::from_be_bytes([
        data[56], data[57], data[58], data[59], data[60], data[61], data[62], data[63],
    ]);
    tags.push(mk(
        "DeviceAttributes",
        "Device Attributes",
        Value::String(format!("{}", attr)),
    ));

    // Device manufacturer (bytes 48-51) and model (52-55)
    let manufacturer = crate::encoding::decode_utf8_or_latin1(&data[48..52])
        .trim()
        .to_string();
    if !manufacturer.is_empty() && manufacturer.bytes().any(|b| b > 0x20) {
        tags.push(mk(
            "DeviceManufacturer",
            "Device Manufacturer",
            Value::String(manufacturer),
        ));
    }
    // DeviceModel: always emit (may be empty string), from ICC profile header bytes 52-55
    let dev_model_raw = &data[52..56];
    let dev_model = if dev_model_raw.iter().all(|&b| b == 0) {
        String::new()
    } else {
        crate::encoding::decode_utf8_or_latin1(dev_model_raw)
            .trim_end_matches('\0')
            .trim()
            .to_string()
    };
    tags.push(mk("DeviceModel", "Device Model", Value::String(dev_model)));

    // ProfileFileSignature (bytes 36-39, should be "acsp")
    tags.push(mk(
        "ProfileFileSignature",
        "Profile File Signature",
        Value::String("acsp".into()),
    ));

    // ConnectionSpaceIlluminant (bytes 68-79, XYZ)
    if data.len() >= 80 {
        let x = i32::from_be_bytes([data[68], data[69], data[70], data[71]]) as f64 / 65536.0;
        let y = i32::from_be_bytes([data[72], data[73], data[74], data[75]]) as f64 / 65536.0;
        let z = i32::from_be_bytes([data[76], data[77], data[78], data[79]]) as f64 / 65536.0;
        tags.push(mk(
            "ConnectionSpaceIlluminant",
            "Connection Space Illuminant",
            Value::String(format!("{:.6} {:.6} {:.6}", x, y, z)),
        ));
    }

    // ProfileCreator (bytes 80-83) - always emit (may be empty)
    if data.len() >= 84 {
        let raw = &data[80..84];
        let creator = if raw.iter().all(|&b| b == 0) {
            String::new()
        } else {
            crate::encoding::decode_utf8_or_latin1(raw)
                .trim_end_matches('\0')
                .trim()
                .to_string()
        };
        tags.push(mk(
            "ProfileCreator",
            "Profile Creator",
            Value::String(creator),
        ));
    }

    // ProfileID (bytes 84-99, MD5)
    if data.len() >= 100 {
        let id: String = data[84..100].iter().map(|b| format!("{:02x}", b)).collect();
        tags.push(mk("ProfileID", "Profile ID", Value::String(id)));
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
            let offset = u32::from_be_bytes([
                data[tpos + 4],
                data[tpos + 5],
                data[tpos + 6],
                data[tpos + 7],
            ]) as usize;
            let size = u32::from_be_bytes([
                data[tpos + 8],
                data[tpos + 9],
                data[tpos + 10],
                data[tpos + 11],
            ]) as usize;
            tpos += 12;

            if sig == b"desc" && offset + size <= data.len() && size > 12 {
                // 'desc' type: 4 bytes signature + 4 reserved + 4 bytes string length + string
                let d = &data[offset..offset + size];
                if d.len() >= 12 && &d[0..4] == b"desc" {
                    let str_len = u32::from_be_bytes([d[8], d[9], d[10], d[11]]) as usize;
                    if 12 + str_len <= d.len() {
                        let desc = crate::encoding::decode_utf8_or_latin1(&d[12..12 + str_len])
                            .trim_end_matches('\0')
                            .to_string();
                        if !desc.is_empty() {
                            tags.push(mk(
                                "ProfileDescription",
                                "Profile Description",
                                Value::String(desc),
                            ));
                        }
                    }
                }
            }

            if sig == b"cprt" && offset + size <= data.len() && size > 8 {
                let d = &data[offset..offset + size];
                if d.len() >= 8 && &d[0..4] == b"text" {
                    let text = crate::encoding::decode_utf8_or_latin1(&d[8..])
                        .trim_end_matches('\0')
                        .to_string();
                    if !text.is_empty() {
                        tags.push(mk(
                            "ProfileCopyright",
                            "Profile Copyright",
                            Value::String(text),
                        ));
                    }
                }
            }

            // Map ICC tag signatures to names (from Perl ICC_Profile.pm)
            if offset + size <= data.len() && size >= 8 {
                let d = &data[offset..offset + size];
                let tag_name = match sig {
                    b"rXYZ" => "RedMatrixColumn",
                    b"gXYZ" => "GreenMatrixColumn",
                    b"bXYZ" => "BlueMatrixColumn",
                    b"wtpt" => "MediaWhitePoint",
                    b"bkpt" => "MediaBlackPoint",
                    b"lumi" => "Luminance",
                    b"rTRC" => "RedTRC",
                    b"gTRC" => "GreenTRC",
                    b"bTRC" => "BlueTRC",
                    b"tech" => "Technology",
                    b"dmnd" => "DeviceMfgDesc",
                    b"dmdd" => "DeviceModelDesc",
                    b"vued" => "ViewingCondDesc",
                    b"view" => "ViewingConditions",
                    b"meas" => "MeasurementInfo",
                    b"chad" => "ChromaticAdaptation",
                    _ => "",
                };
                if !tag_name.is_empty() {
                    let type_sig = &d[0..4];
                    let value = match type_sig {
                        b"XYZ " if d.len() >= 20 => {
                            // XYZ type: 3 x s15Fixed16
                            let x = i32::from_be_bytes([d[8], d[9], d[10], d[11]]) as f64 / 65536.0;
                            let y =
                                i32::from_be_bytes([d[12], d[13], d[14], d[15]]) as f64 / 65536.0;
                            let z =
                                i32::from_be_bytes([d[16], d[17], d[18], d[19]]) as f64 / 65536.0;
                            format!("{:.6} {:.6} {:.6}", x, y, z)
                        }
                        b"curv" if d.len() >= 12 => {
                            let count = u32::from_be_bytes([d[8], d[9], d[10], d[11]]);
                            if count == 0 {
                                "Linear".into()
                            } else if count == 1 && d.len() >= 14 {
                                let gamma = u16::from_be_bytes([d[12], d[13]]) as f64 / 256.0;
                                format!("{:.1}", gamma)
                            } else {
                                format!("(Binary data {} entries)", count)
                            }
                        }
                        b"desc" if d.len() >= 12 => {
                            let len = u32::from_be_bytes([d[8], d[9], d[10], d[11]]) as usize;
                            if 12 + len <= d.len() {
                                crate::encoding::decode_utf8_or_latin1(&d[12..12 + len])
                                    .trim_end_matches('\0')
                                    .to_string()
                            } else {
                                String::new()
                            }
                        }
                        b"mluc" if d.len() >= 20 => {
                            // multiLocalizedUnicode
                            let rec_count = u32::from_be_bytes([d[8], d[9], d[10], d[11]]) as usize;
                            if rec_count > 0 && d.len() >= 20 {
                                let str_off =
                                    u32::from_be_bytes([d[20], d[21], d[22], d[23]]) as usize;
                                let str_len =
                                    u32::from_be_bytes([d[16], d[17], d[18], d[19]]) as usize;
                                if str_off + str_len <= d.len() {
                                    let units: Vec<u16> = d[str_off..str_off + str_len]
                                        .chunks_exact(2)
                                        .map(|c| u16::from_be_bytes([c[0], c[1]]))
                                        .collect();
                                    String::from_utf16_lossy(&units)
                                        .trim_end_matches('\0')
                                        .to_string()
                                } else {
                                    String::new()
                                }
                            } else {
                                String::new()
                            }
                        }
                        b"sig " if d.len() >= 12 => {
                            crate::encoding::decode_utf8_or_latin1(&d[8..12])
                                .trim()
                                .to_string()
                        }
                        b"meas" if d.len() >= 36 => {
                            // measurement type
                            let observer = match u32::from_be_bytes([d[8], d[9], d[10], d[11]]) {
                                1 => "CIE 1931",
                                2 => "CIE 1964",
                                _ => "Unknown",
                            };
                            tags.push(mk(
                                "MeasurementObserver",
                                "Measurement Observer",
                                Value::String(observer.into()),
                            ));
                            let geometry = match u32::from_be_bytes([d[24], d[25], d[26], d[27]]) {
                                1 => "0/45 or 45/0",
                                2 => "0/d or d/0",
                                _ => "Unknown",
                            };
                            tags.push(mk(
                                "MeasurementGeometry",
                                "Measurement Geometry",
                                Value::String(geometry.into()),
                            ));
                            let illum = match u32::from_be_bytes([d[32], d[33], d[34], d[35]]) {
                                1 => "D50",
                                2 => "D65",
                                3 => "D93",
                                4 => "F2",
                                5 => "D55",
                                6 => "A",
                                7 => "E",
                                8 => "F8",
                                _ => "Unknown",
                            };
                            tags.push(mk(
                                "MeasurementIlluminant",
                                "Measurement Illuminant",
                                Value::String(illum.into()),
                            ));
                            // Backing and flare
                            let backing_x =
                                i32::from_be_bytes([d[12], d[13], d[14], d[15]]) as f64 / 65536.0;
                            let backing_y =
                                i32::from_be_bytes([d[16], d[17], d[18], d[19]]) as f64 / 65536.0;
                            let backing_z =
                                i32::from_be_bytes([d[20], d[21], d[22], d[23]]) as f64 / 65536.0;
                            tags.push(mk(
                                "MeasurementBacking",
                                "Measurement Backing",
                                Value::String(format!(
                                    "{:.6} {:.6} {:.6}",
                                    backing_x, backing_y, backing_z
                                )),
                            ));
                            let flare =
                                u32::from_be_bytes([d[28], d[29], d[30], d[31]]) as f64 / 65536.0;
                            tags.push(mk(
                                "MeasurementFlare",
                                "Measurement Flare",
                                Value::String(format!("{:.4}%", flare * 100.0)),
                            ));
                            String::new() // sub-tags already pushed
                        }
                        b"view" if d.len() >= 28 => {
                            let x = i32::from_be_bytes([d[8], d[9], d[10], d[11]]) as f64 / 65536.0;
                            let y =
                                i32::from_be_bytes([d[12], d[13], d[14], d[15]]) as f64 / 65536.0;
                            let z =
                                i32::from_be_bytes([d[16], d[17], d[18], d[19]]) as f64 / 65536.0;
                            tags.push(mk(
                                "ViewingCondIlluminant",
                                "Viewing Cond Illuminant",
                                Value::String(format!("{:.5} {:.5} {:.5}", x, y, z)),
                            ));
                            let sx =
                                i32::from_be_bytes([d[20], d[21], d[22], d[23]]) as f64 / 65536.0;
                            let sy =
                                i32::from_be_bytes([d[24], d[25], d[26], d[27]]) as f64 / 65536.0;
                            let sz =
                                i32::from_be_bytes([d[28], d[29], d[30], d[31]]) as f64 / 65536.0;
                            tags.push(mk(
                                "ViewingCondSurround",
                                "Viewing Cond Surround",
                                Value::String(format!("{:.5} {:.5} {:.5}", sx, sy, sz)),
                            ));
                            if d.len() >= 36 {
                                let illum_type =
                                    match u32::from_be_bytes([d[32], d[33], d[34], d[35]]) {
                                        1 => "D50",
                                        2 => "D65",
                                        3 => "D93",
                                        4 => "F2",
                                        5 => "D55",
                                        6 => "A",
                                        7 => "E",
                                        8 => "F8",
                                        _ => "Unknown",
                                    };
                                tags.push(mk(
                                    "ViewingCondIlluminantType",
                                    "Viewing Cond Illuminant Type",
                                    Value::String(illum_type.into()),
                                ));
                            }
                            String::new()
                        }
                        _ => String::new(),
                    };
                    if !value.is_empty() {
                        tags.push(mk(tag_name, tag_name, Value::String(value)));
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

/// Parse ICC profile tags from raw profile data embedded in JPEG APP2.
pub fn parse_icc_tags(data: &[u8]) -> Vec<Tag> {
    read_icc(data).unwrap_or_default()
}
