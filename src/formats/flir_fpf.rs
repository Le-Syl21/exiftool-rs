//! FLIR Public image Format (FPF) reader.
//! Mirrors ExifTool's FLIR.pm ProcessFPF / %Image::ExifTool::FLIR::FPF.
//!
//! Reference: http://support.flir.com/DocDownload/Assets/62/English/1557488%24A.pdf
//!            http://code.google.com/p/dvelib/source/browse/trunk/flirPublicFormat/fpfConverter/Fpfimg.h

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
            family0: "FLIR".into(),
            family1: "FLIR".into(),
            family2: "Image".into(),
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
            family0: "FLIR".into(),
            family1: "FLIR".into(),
            family2: "Image".into(),
        },
        raw_value: value,
        print_value: print,
        priority: 0,
    }
}

fn read_u16_le(data: &[u8], offset: usize) -> u16 {
    if offset + 2 > data.len() {
        return 0;
    }
    u16::from_le_bytes([data[offset], data[offset + 1]])
}

fn read_u32_le(data: &[u8], offset: usize) -> u32 {
    if offset + 4 > data.len() {
        return 0;
    }
    u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

fn read_f32_le(data: &[u8], offset: usize) -> f32 {
    if offset + 4 > data.len() {
        return 0.0;
    }
    f32::from_bits(u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ]))
}

fn read_str(data: &[u8], offset: usize, len: usize) -> String {
    if offset + len > data.len() {
        return String::new();
    }
    let s = &data[offset..offset + len];
    let end = s.iter().position(|&b| b == 0).unwrap_or(len);
    crate::encoding::decode_utf8_or_latin1(&s[..end])
        .trim()
        .to_string()
}

/// Convert float Kelvin to Celsius: subtract 273.15, format "%.1f C"
fn kelvin_to_celsius(k: f32) -> String {
    let c = k as f64 - 273.15;
    let c = if c == 0.0 { 0.0 } else { c }; // normalize -0.0
    format!("{:.1} C", c)
}

pub fn read_fpf(data: &[u8]) -> Result<Vec<Tag>> {
    // Magic: "FPF Public Image Format\0" at offset 0, header is 892 bytes
    if data.len() < 892 {
        return Err(Error::InvalidData("FPF file too small".into()));
    }
    if !data.starts_with(b"FPF Public Image Format\0") {
        return Err(Error::InvalidData("not a FPF file".into()));
    }

    // Perl: SetByteOrder('II'); ToggleByteOrder() unless Get32u(\$buff, 0x20) & 0xffff;
    // i.e., it's little-endian unless the low 16 bits of the version word are 0
    // (in practice all known samples are little-endian)
    let version_raw = read_u32_le(data, 0x20);
    if version_raw & 0xffff == 0 {
        // Big-endian file — we don't have a sample but handle gracefully
        return Err(Error::InvalidData("FPF big-endian not supported".into()));
    }

    let mut tags: Vec<Tag> = Vec::new();

    // 0x20: FPFVersion (int32u)
    let fpf_version = read_u32_le(data, 0x20);
    tags.push(mk("FPFVersion", Value::U32(fpf_version)));

    // 0x24: ImageDataOffset (int32u)
    let image_data_offset = read_u32_le(data, 0x24);
    tags.push(mk("ImageDataOffset", Value::U32(image_data_offset)));

    // 0x28: ImageType (int16u)
    let image_type = read_u16_le(data, 0x28);
    let image_type_str = match image_type {
        0 => "Temperature",
        1 => "Temperature Difference",
        2 => "Object Signal",
        3 => "Object Signal Difference",
        _ => "Unknown",
    };
    tags.push(mk_print(
        "ImageType",
        Value::U16(image_type),
        image_type_str.to_string(),
    ));

    // 0x2a: ImagePixelFormat (int16u)
    let pixel_fmt = read_u16_le(data, 0x2a);
    let pixel_fmt_str = match pixel_fmt {
        0 => "2-byte short integer",
        1 => "4-byte long integer",
        2 => "4-byte float",
        3 => "8-byte double",
        _ => "Unknown",
    };
    tags.push(mk_print(
        "ImagePixelFormat",
        Value::U16(pixel_fmt),
        pixel_fmt_str.to_string(),
    ));

    // 0x2c: ImageWidth (int16u)
    let width = read_u16_le(data, 0x2c);
    tags.push(mk("ImageWidth", Value::U16(width)));

    // 0x2e: ImageHeight (int16u)
    let height = read_u16_le(data, 0x2e);
    tags.push(mk("ImageHeight", Value::U16(height)));

    // 0x30: ExternalTriggerCount (int32u)
    let ext_trig = read_u32_le(data, 0x30);
    tags.push(mk("ExternalTriggerCount", Value::U32(ext_trig)));

    // 0x34: SequenceFrameNumber (int32u)
    let seq_frame = read_u32_le(data, 0x34);
    tags.push(mk("SequenceFrameNumber", Value::U32(seq_frame)));

    // 0x78: CameraModel (string[32])
    let camera_model = read_str(data, 0x78, 32);
    tags.push(mk("CameraModel", Value::String(camera_model)));

    // 0x98: CameraPartNumber (string[32])
    let camera_part = read_str(data, 0x98, 32);
    tags.push(mk("CameraPartNumber", Value::String(camera_part)));

    // 0xb8: CameraSerialNumber (string[32])
    let camera_serial = read_str(data, 0xb8, 32);
    tags.push(mk("CameraSerialNumber", Value::String(camera_serial)));

    // 0xd8: CameraTemperatureRangeMin (float, Kelvin -> Celsius)
    let cam_temp_min = read_f32_le(data, 0xd8);
    tags.push(mk_print(
        "CameraTemperatureRangeMin",
        Value::F32(cam_temp_min),
        kelvin_to_celsius(cam_temp_min),
    ));

    // 0xdc: CameraTemperatureRangeMax (float, Kelvin -> Celsius)
    let cam_temp_max = read_f32_le(data, 0xdc);
    tags.push(mk_print(
        "CameraTemperatureRangeMax",
        Value::F32(cam_temp_max),
        kelvin_to_celsius(cam_temp_max),
    ));

    // 0xe0: LensModel (string[32])
    let lens_model = read_str(data, 0xe0, 32);
    tags.push(mk("LensModel", Value::String(lens_model)));

    // 0x100: LensPartNumber (string[32])
    let lens_part = read_str(data, 0x100, 32);
    tags.push(mk("LensPartNumber", Value::String(lens_part)));

    // 0x120: LensSerialNumber (string[32])
    let lens_serial = read_str(data, 0x120, 32);
    tags.push(mk("LensSerialNumber", Value::String(lens_serial)));

    // 0x140: FilterModel (string[32])
    // Note: Perl says string[32] but next field (FilterPartNumber) is at 0x150 which is only 16 bytes
    // away; ref 4 says FilterModel is at 0x140. We use 16 bytes to be consistent with spacing.
    let filter_model = read_str(data, 0x140, 16);
    tags.push(mk("FilterModel", Value::String(filter_model)));

    // 0x150: FilterPartNumber (string[32])
    // (0x180 - 0x150 = 0x30 = 48 bytes, but Perl says string[32] — use 32)
    let filter_part = read_str(data, 0x150, 32);
    tags.push(mk("FilterPartNumber", Value::String(filter_part)));

    // 0x180: FilterSerialNumber (string[32])
    // (0x1e0 - 0x180 = 0x60 = 96 bytes available, but Perl says string[32])
    let filter_serial = read_str(data, 0x180, 32);
    tags.push(mk("FilterSerialNumber", Value::String(filter_serial)));

    // 0x1e0: Emissivity (float, %.2f)
    let emissivity = read_f32_le(data, 0x1e0);
    tags.push(mk_print(
        "Emissivity",
        Value::F32(emissivity),
        format!("{:.2}", emissivity),
    ));

    // 0x1e4: ObjectDistance (float, "%.2f m")
    let obj_dist = read_f32_le(data, 0x1e4);
    tags.push(mk_print(
        "ObjectDistance",
        Value::F32(obj_dist),
        format!("{:.2} m", obj_dist),
    ));

    // 0x1e8: ReflectedApparentTemperature (float, Kelvin -> Celsius)
    let refl_temp = read_f32_le(data, 0x1e8);
    tags.push(mk_print(
        "ReflectedApparentTemperature",
        Value::F32(refl_temp),
        kelvin_to_celsius(refl_temp),
    ));

    // 0x1ec: AtmosphericTemperature (float, Kelvin -> Celsius)
    let atm_temp = read_f32_le(data, 0x1ec);
    tags.push(mk_print(
        "AtmosphericTemperature",
        Value::F32(atm_temp),
        kelvin_to_celsius(atm_temp),
    ));

    // 0x1f0: RelativeHumidity (float, "%.1f %%" * 100)
    let rel_hum = read_f32_le(data, 0x1f0);
    tags.push(mk_print(
        "RelativeHumidity",
        Value::F32(rel_hum),
        format!("{:.1} %", (rel_hum as f64) * 100.0),
    ));

    // 0x1f4: ComputedAtmosphericTrans (float, %.2f)
    let comp_atm = read_f32_le(data, 0x1f4);
    tags.push(mk_print(
        "ComputedAtmosphericTrans",
        Value::F32(comp_atm),
        format!("{:.2}", comp_atm),
    ));

    // 0x1f8: EstimatedAtmosphericTrans (float, %.2f)
    let est_atm = read_f32_le(data, 0x1f8);
    tags.push(mk_print(
        "EstimatedAtmosphericTrans",
        Value::F32(est_atm),
        format!("{:.2}", est_atm),
    ));

    // 0x1fc: ReferenceTemperature (float, Kelvin -> Celsius)
    let ref_temp = read_f32_le(data, 0x1fc);
    tags.push(mk_print(
        "ReferenceTemperature",
        Value::F32(ref_temp),
        kelvin_to_celsius(ref_temp),
    ));

    // 0x200: IRWindowTemperature (float, Kelvin -> Celsius)
    let irwin_temp = read_f32_le(data, 0x200);
    tags.push(mk_print(
        "IRWindowTemperature",
        Value::F32(irwin_temp),
        kelvin_to_celsius(irwin_temp),
    ));

    // 0x204: IRWindowTransmission (float, %.2f)
    let irwin_trans = read_f32_le(data, 0x204);
    tags.push(mk_print(
        "IRWindowTransmission",
        Value::F32(irwin_trans),
        format!("{:.2}", irwin_trans),
    ));

    // 0x248: DateTimeOriginal (int32u[7])
    // Format: year, month, day, hour, min, sec, millisec
    // ValueConv: sprintf("%.4d:%.2d:%.2d %.2d:%.2d:%.2d.%.3d", split(" ", $val))
    if data.len() >= 0x248 + 7 * 4 {
        let year = read_u32_le(data, 0x248);
        let month = read_u32_le(data, 0x24c);
        let day = read_u32_le(data, 0x250);
        let hour = read_u32_le(data, 0x254);
        let min = read_u32_le(data, 0x258);
        let sec = read_u32_le(data, 0x25c);
        let ms = read_u32_le(data, 0x260);
        let dt = format!(
            "{:04}:{:02}:{:02} {:02}:{:02}:{:02}.{:03}",
            year, month, day, hour, min, sec, ms
        );
        tags.push(mk("DateTimeOriginal", Value::String(dt)));
    }

    // 0x2a4: CameraScaleMin (float, %.1f)
    let cam_scale_min = read_f32_le(data, 0x2a4);
    tags.push(mk_print(
        "CameraScaleMin",
        Value::F32(cam_scale_min),
        format!("{:.1}", cam_scale_min),
    ));

    // 0x2a8: CameraScaleMax (float, %.1f)
    let cam_scale_max = read_f32_le(data, 0x2a8);
    tags.push(mk_print(
        "CameraScaleMax",
        Value::F32(cam_scale_max),
        format!("{:.1}", cam_scale_max),
    ));

    // 0x2ac: CalculatedScaleMin (float, %.1f)
    let calc_scale_min = read_f32_le(data, 0x2ac);
    tags.push(mk_print(
        "CalculatedScaleMin",
        Value::F32(calc_scale_min),
        format!("{:.1}", calc_scale_min),
    ));

    // 0x2b0: CalculatedScaleMax (float, %.1f)
    let calc_scale_max = read_f32_le(data, 0x2b0);
    tags.push(mk_print(
        "CalculatedScaleMax",
        Value::F32(calc_scale_max),
        format!("{:.1}", calc_scale_max),
    ));

    // 0x2b4: ActualScaleMin (float, %.1f)
    let act_scale_min = read_f32_le(data, 0x2b4);
    tags.push(mk_print(
        "ActualScaleMin",
        Value::F32(act_scale_min),
        format!("{:.1}", act_scale_min),
    ));

    // 0x2b8: ActualScaleMax (float, %.1f)
    let act_scale_max = read_f32_le(data, 0x2b8);
    tags.push(mk_print(
        "ActualScaleMax",
        Value::F32(act_scale_max),
        format!("{:.1}", act_scale_max),
    ));

    Ok(tags)
}
