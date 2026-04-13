//! Kyocera Contax N Digital RAW format reader.

use super::misc::mktag;
use crate::error::{Error, Result};
use crate::tag::Tag;
use crate::value::Value;

pub fn read_kyocera_raw(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 156 {
        return Err(Error::InvalidData("too short for Kyocera RAW".into()));
    }
    // Validate: "ARECOYK" at offset 0x19
    if &data[0x19..0x20] != b"ARECOYK" {
        return Err(Error::InvalidData("not a Kyocera RAW file".into()));
    }

    let mut tags = Vec::new();
    let group = "KyoceraRaw";

    // FirmwareVersion at 0x01, 10 bytes, reversed string
    let fw_bytes: Vec<u8> = data[0x01..0x0b].iter().rev().copied().collect();
    let fw = crate::encoding::decode_utf8_or_latin1(&fw_bytes)
        .trim_matches('\0')
        .to_string();
    if !fw.is_empty() {
        tags.push(mktag(
            group,
            "FirmwareVersion",
            "Firmware Version",
            Value::String(fw),
        ));
    }

    // Model at 0x0c, 12 bytes, reversed string
    let model_bytes: Vec<u8> = data[0x0c..0x18].iter().rev().copied().collect();
    let model = crate::encoding::decode_utf8_or_latin1(&model_bytes)
        .trim_matches('\0')
        .to_string();
    if !model.is_empty() {
        tags.push(mktag(
            group,
            "Model",
            "Camera Model Name",
            Value::String(model),
        ));
    }

    // Make at 0x19, 7 bytes, reversed string
    let make_bytes: Vec<u8> = data[0x19..0x20].iter().rev().copied().collect();
    let make = crate::encoding::decode_utf8_or_latin1(&make_bytes)
        .trim_matches('\0')
        .to_string();
    if !make.is_empty() {
        tags.push(mktag(group, "Make", "Camera Make", Value::String(make)));
    }

    // DateTimeOriginal at 0x21, 20 bytes, reversed string
    let dt_bytes: Vec<u8> = data[0x21..0x35].iter().rev().copied().collect();
    let dt_str = crate::encoding::decode_utf8_or_latin1(&dt_bytes)
        .trim_matches('\0')
        .to_string();
    if !dt_str.is_empty() {
        tags.push(mktag(
            group,
            "DateTimeOriginal",
            "Date/Time Original",
            Value::String(dt_str),
        ));
    }

    // ISO at 0x34, int32u (big-endian, index into table)
    if data.len() >= 0x38 {
        let iso_idx = u32::from_be_bytes([data[0x34], data[0x35], data[0x36], data[0x37]]);
        let iso_val = kyocera_iso(iso_idx);
        if iso_val > 0 {
            let mut t = mktag(group, "ISO", "ISO", Value::String(iso_idx.to_string()));
            t.print_value = iso_val.to_string();
            tags.push(t);
        }
    }

    // ExposureTime at 0x38, int32u: 2^(val/8)/16000
    if data.len() >= 0x3c {
        let et_idx = u32::from_be_bytes([data[0x38], data[0x39], data[0x3a], data[0x3b]]);
        let et_val = f64::powf(2.0, et_idx as f64 / 8.0) / 16000.0;
        let print_val = format_exposure_time(et_val);
        let mut t = mktag(
            group,
            "ExposureTime",
            "Exposure Time",
            Value::String(format!("{:.10}", et_val)),
        );
        t.print_value = print_val;
        tags.push(t);
    }

    // WB_RGGBLevels at 0x3c, int32u[4]
    if data.len() >= 0x4c {
        let r = u32::from_be_bytes([data[0x3c], data[0x3d], data[0x3e], data[0x3f]]);
        let g1 = u32::from_be_bytes([data[0x40], data[0x41], data[0x42], data[0x43]]);
        let g2 = u32::from_be_bytes([data[0x44], data[0x45], data[0x46], data[0x47]]);
        let b = u32::from_be_bytes([data[0x48], data[0x49], data[0x4a], data[0x4b]]);
        let wb_str = format!("{} {} {} {}", r, g1, g2, b);
        tags.push(mktag(
            group,
            "WB_RGGBLevels",
            "WB RGGB Levels",
            Value::String(wb_str),
        ));
    }

    // FNumber at 0x58, int32u: 2^(val/16)
    if data.len() >= 0x5c {
        let fn_idx = u32::from_be_bytes([data[0x58], data[0x59], data[0x5a], data[0x5b]]);
        let fn_val = f64::powf(2.0, fn_idx as f64 / 16.0);
        let print_val = format!("{}", (fn_val * 10000.0).round() / 10000.0);
        let mut t = mktag(
            group,
            "FNumber",
            "F Number",
            Value::String(format!("{}", fn_val)),
        );
        t.print_value = print_val;
        tags.push(t);
    }

    // MaxAperture at 0x68, int32u: 2^(val/16)
    if data.len() >= 0x6c {
        let ma_idx = u32::from_be_bytes([data[0x68], data[0x69], data[0x6a], data[0x6b]]);
        let ma_val = f64::powf(2.0, ma_idx as f64 / 16.0);
        let print_val = format!("{}", (ma_val * 100.0).round() / 100.0);
        let mut t = mktag(
            group,
            "MaxAperture",
            "Max Aperture Value",
            Value::String(format!("{}", ma_val)),
        );
        t.print_value = print_val;
        tags.push(t);
    }

    // FocalLength at 0x70, int32u
    if data.len() >= 0x74 {
        let fl = u32::from_be_bytes([data[0x70], data[0x71], data[0x72], data[0x73]]);
        let mut t = mktag(
            group,
            "FocalLength",
            "Focal Length",
            Value::String(fl.to_string()),
        );
        t.print_value = format!("{} mm", fl);
        tags.push(t);
    }

    // Lens at 0x7c, string[32]
    if data.len() >= 0x9c {
        let lens_bytes = &data[0x7c..0x9c];
        let lens = crate::encoding::decode_utf8_or_latin1(lens_bytes)
            .trim_matches('\0')
            .to_string();
        if !lens.is_empty() {
            tags.push(mktag(group, "Lens", "Lens", Value::String(lens)));
        }
    }

    Ok(tags)
}

fn kyocera_iso(idx: u32) -> u32 {
    match idx {
        7 => 25,
        8 => 32,
        9 => 40,
        10 => 50,
        11 => 64,
        12 => 80,
        13 => 100,
        14 => 125,
        15 => 160,
        16 => 200,
        17 => 250,
        18 => 320,
        19 => 400,
        _ => 0,
    }
}

fn format_exposure_time(val: f64) -> String {
    if val == 0.0 {
        return "0".to_string();
    }
    if val >= 1.0 {
        format!("{}", val)
    } else {
        let recip = (1.0 / val).round() as u32;
        format!("1/{}", recip)
    }
}
