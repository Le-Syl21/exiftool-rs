//! MRC (Medical Research Council) image format reader.
//! Mirrors ExifTool's MRC.pm ProcessMRC.
//!
//! Reference: https://www.ccpem.ac.uk/mrc_format/mrc2014.php

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;
use crate::value::format_g15;

fn mk(name: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: "File".into(),
            family1: "File".into(),
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
            family0: "File".into(),
            family1: "File".into(),
            family2: "Image".into(),
        },
        raw_value: value,
        print_value: print,
        priority: 0,
    }
}

fn mk_time(name: &str, value: Value, print: String) -> Tag {
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: "File".into(),
            family1: "File".into(),
            family2: "Time".into(),
        },
        raw_value: value,
        print_value: print,
        priority: 0,
    }
}

fn read_u32_le(data: &[u8], offset: usize) -> u32 {
    if offset + 4 > data.len() { return 0; }
    u32::from_le_bytes([data[offset], data[offset+1], data[offset+2], data[offset+3]])
}

fn read_i32_le(data: &[u8], offset: usize) -> i32 {
    if offset + 4 > data.len() { return 0; }
    i32::from_le_bytes([data[offset], data[offset+1], data[offset+2], data[offset+3]])
}

fn read_f32_le(data: &[u8], offset: usize) -> f32 {
    if offset + 4 > data.len() { return 0.0; }
    f32::from_bits(u32::from_le_bytes([data[offset], data[offset+1], data[offset+2], data[offset+3]]))
}

fn read_f64_le(data: &[u8], offset: usize) -> f64 {
    if offset + 8 > data.len() { return 0.0; }
    f64::from_bits(u64::from_le_bytes([
        data[offset], data[offset+1], data[offset+2], data[offset+3],
        data[offset+4], data[offset+5], data[offset+6], data[offset+7],
    ]))
}

fn read_u64_le(data: &[u8], offset: usize) -> u64 {
    if offset + 8 > data.len() { return 0; }
    u64::from_le_bytes([
        data[offset], data[offset+1], data[offset+2], data[offset+3],
        data[offset+4], data[offset+5], data[offset+6], data[offset+7],
    ])
}

fn read_str(data: &[u8], offset: usize, len: usize) -> String {
    if offset + len > data.len() { return String::new(); }
    let s = &data[offset..offset+len];
    let end = s.iter().position(|&b| b == 0).unwrap_or(len);
    String::from_utf8_lossy(&s[..end]).trim_end().to_string()
}

/// Convert Unix timestamp (seconds since 1970-01-01) to ExifTool datetime string.
fn unix_to_exif_datetime(secs: i64) -> String {
    if secs < 0 { return String::new(); }
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let h = time_secs / 3600;
    let m = (time_secs % 3600) / 60;
    let s = time_secs % 60;
    let mut y = 1970i32;
    let mut rem = days;
    loop {
        let dy = if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 { 366 } else { 365 };
        if rem < dy { break; }
        rem -= dy;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let months = [31i64, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut mo = 1;
    for &dm in &months {
        if rem < dm { break; }
        rem -= dm;
        mo += 1;
    }
    format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}", y, mo, rem + 1, h, m, s)
}

/// Convert OLE Automation date (days since Dec 30, 1899) to ExifTool datetime string.
/// Perl: ValueConv => 'ConvertUnixTime(($val-25569)*24*3600)'
/// Perl's ConvertUnixTime rounds fractional seconds (adds 1 if frac >= 0.5).
fn ole_date_to_datetime(ole_days: f64) -> String {
    let unix_secs_f = (ole_days - 25569.0) * 86400.0;
    // Round fractional seconds (Perl ConvertUnixTime behavior)
    let itime = unix_secs_f.floor() as i64;
    let frac = unix_secs_f - itime as f64;
    // If frac rounds to 1 (i.e., >= 0.5), increment itime
    let rounded = if frac >= 0.5 { itime + 1 } else { itime };
    unix_to_exif_datetime(rounded)
}

/// Convert microseconds-since-Unix-epoch to ExifTool datetime string.
/// Perl: ValueConv => 'ConvertUnixTime($val / 1e6, 1, 6)'
fn usecs_to_datetime(usecs: u64) -> String {
    let secs = usecs / 1_000_000;
    let frac_usecs = usecs % 1_000_000;
    let base = unix_to_exif_datetime(secs as i64);
    if frac_usecs == 0 {
        format!("{}Z", base)
    } else {
        format!("{}.{:06}Z", base, frac_usecs)
    }
}

/// Process MRC main header (1024 bytes, little-endian int32u fields).
fn process_mrc_header(data: &[u8], tags: &mut Vec<Tag>) -> (u32, u32, String) {
    // All offsets are in int32u units (4 bytes), little-endian.
    // The header is always 1024 bytes.

    // Field 0 (offset 0): ImageWidth
    let image_width = read_u32_le(data, 0);
    tags.push(mk("ImageWidth", Value::U32(image_width)));

    // Field 1 (offset 4): ImageHeight
    let image_height = read_u32_le(data, 4);
    tags.push(mk("ImageHeight", Value::U32(image_height)));

    // Field 2 (offset 8): ImageDepth
    let image_depth = read_u32_le(data, 8);
    tags.push(mk("ImageDepth", Value::U32(image_depth)));

    // Field 3 (offset 12): ImageMode
    let image_mode = read_u32_le(data, 12);
    let mode_str = match image_mode {
        0 => "8-bit signed integer",
        1 => "16-bit signed integer",
        2 => "32-bit signed real",
        3 => "complex 16-bit integer",
        4 => "complex 32-bit real",
        6 => "16-bit unsigned integer",
        _ => "Unknown",
    };
    tags.push(mk_print("ImageMode", Value::U32(image_mode), mode_str.to_string()));

    // Field 4 (offset 16): StartPoint — int32u[3]
    {
        let a = read_i32_le(data, 16);
        let b = read_i32_le(data, 20);
        let c = read_i32_le(data, 24);
        let v = Value::List(vec![Value::I32(a), Value::I32(b), Value::I32(c)]);
        let print = format!("{} {} {}", a, b, c);
        tags.push(mk_print("StartPoint", v, print));
    }

    // Field 7 (offset 28): GridSize — int32u[3]
    {
        let a = read_u32_le(data, 28);
        let b = read_u32_le(data, 32);
        let c = read_u32_le(data, 36);
        let v = Value::List(vec![Value::U32(a), Value::U32(b), Value::U32(c)]);
        let print = format!("{} {} {}", a, b, c);
        tags.push(mk_print("GridSize", v, print));
    }

    // Field 10 (offset 40): CellWidth — float
    {
        let v = read_f32_le(data, 40) as f64;
        tags.push(mk_print("CellWidth", Value::F64(v), format_g15(v)));
    }

    // Field 11 (offset 44): CellHeight — float
    {
        let v = read_f32_le(data, 44) as f64;
        tags.push(mk_print("CellHeight", Value::F64(v), format_g15(v)));
    }

    // Field 12 (offset 48): CellDepth — float
    {
        let v = read_f32_le(data, 48) as f64;
        tags.push(mk_print("CellDepth", Value::F64(v), format_g15(v)));
    }

    // Field 13 (offset 52): CellAlpha — float
    {
        let v = read_f32_le(data, 52) as f64;
        tags.push(mk_print("CellAlpha", Value::F64(v), format_g15(v)));
    }

    // Field 14 (offset 56): CellBeta — float
    {
        let v = read_f32_le(data, 56) as f64;
        tags.push(mk_print("CellBeta", Value::F64(v), format_g15(v)));
    }

    // Field 15 (offset 60): CellGamma — float
    {
        let v = read_f32_le(data, 60) as f64;
        tags.push(mk_print("CellGamma", Value::F64(v), format_g15(v)));
    }

    // Field 16 (offset 64): ImageWidthAxis — int32u, PrintConv {1=>X, 2=>Y, 3=>Z}
    {
        let v = read_u32_le(data, 64);
        let print = match v { 1 => "X", 2 => "Y", 3 => "Z", _ => "Unknown" };
        tags.push(mk_print("ImageWidthAxis", Value::U32(v), print.to_string()));
    }

    // Field 17 (offset 68): ImageHeightAxis
    {
        let v = read_u32_le(data, 68);
        let print = match v { 1 => "X", 2 => "Y", 3 => "Z", _ => "Unknown" };
        tags.push(mk_print("ImageHeightAxis", Value::U32(v), print.to_string()));
    }

    // Field 18 (offset 72): ImageDepthAxis
    {
        let v = read_u32_le(data, 72);
        let print = match v { 1 => "X", 2 => "Y", 3 => "Z", _ => "Unknown" };
        tags.push(mk_print("ImageDepthAxis", Value::U32(v), print.to_string()));
    }

    // Field 19 (offset 76): DensityMin — float
    {
        let v = read_f32_le(data, 76) as f64;
        tags.push(mk_print("DensityMin", Value::F64(v), format_g15(v)));
    }

    // Field 20 (offset 80): DensityMax — float
    {
        let v = read_f32_le(data, 80) as f64;
        tags.push(mk_print("DensityMax", Value::F64(v), format_g15(v)));
    }

    // Field 21 (offset 84): DensityMean — float
    {
        let v = read_f32_le(data, 84) as f64;
        tags.push(mk_print("DensityMean", Value::F64(v), format_g15(v)));
    }

    // Field 22 (offset 88): SpaceGroupNumber — int32u
    {
        let v = read_u32_le(data, 88);
        tags.push(mk("SpaceGroupNumber", Value::U32(v)));
    }

    // Field 23 (offset 92): ExtendedHeaderSize — int32u
    let ext_hdr_size = read_u32_le(data, 92);
    tags.push(mk("ExtendedHeaderSize", Value::U32(ext_hdr_size)));

    // Field 26 (offset 104): ExtendedHeaderType — string[4]
    let ext_hdr_type = read_str(data, 104, 4);
    tags.push(mk("ExtendedHeaderType", Value::String(ext_hdr_type.clone())));

    // Field 27 (offset 108): MRCVersion — int32u
    {
        let v = read_u32_le(data, 108);
        tags.push(mk("MRCVersion", Value::U32(v)));
    }

    // Field 49 (offset 196): Origin — float[3]
    {
        let a = read_f32_le(data, 196) as f64;
        let b = read_f32_le(data, 200) as f64;
        let c = read_f32_le(data, 204) as f64;
        let v = Value::List(vec![Value::F64(a), Value::F64(b), Value::F64(c)]);
        let print = format!("{} {} {}", format_g15(a), format_g15(b), format_g15(c));
        tags.push(mk_print("Origin", v, print));
    }

    // Field 53 (offset 212): MachineStamp — int8u[4]
    // PrintConv => 'sprintf("0x%.2x 0x%.2x 0x%.2x 0x%.2x", split " ", $val)'
    if data.len() >= 216 {
        let b0 = data[212];
        let b1 = data[213];
        let b2 = data[214];
        let b3 = data[215];
        let print = format!("0x{:02x} 0x{:02x} 0x{:02x} 0x{:02x}", b0, b1, b2, b3);
        let v = Value::List(vec![Value::U8(b0), Value::U8(b1), Value::U8(b2), Value::U8(b3)]);
        tags.push(mk_print("MachineStamp", v, print));
    }

    // Field 54 (offset 216): RMSDeviation — float
    {
        let v = read_f32_le(data, 216) as f64;
        tags.push(mk_print("RMSDeviation", Value::F64(v), format_g15(v)));
    }

    // Field 55 (offset 220): NumberOfLabels — int32u
    let n_lab = read_u32_le(data, 220);
    tags.push(mk("NumberOfLabels", Value::U32(n_lab)));

    // Fields 56-236: Labels (string[80] each, 20 int32u words each = 80 bytes)
    // Each label starts at offset 224 + (label_num * 80)
    // Field 56 (offset 224 = 56*4): Label0 if n_lab > 0
    // Field 76 (offset 304 = 76*4): Label1 if n_lab > 1, etc.
    let label_names = ["Label0","Label1","Label2","Label3","Label4","Label5","Label6","Label7","Label8","Label9"];
    for (i, &label_name) in label_names.iter().enumerate() {
        if n_lab as usize > i {
            let offset = 224 + i * 80;
            let s = read_str(data, offset, 80);
            if !s.is_empty() {
                tags.push(mk(label_name, Value::String(s)));
            }
        }
    }

    (ext_hdr_size, image_depth, ext_hdr_type)
}

/// Process FEI1/FEI2 extended header section.
fn process_fei12_header(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 4 {
        return;
    }

    // Offset 0: MetadataSize — int32u
    let metadata_size = read_u32_le(data, 0);
    tags.push(mk("MetadataSize", Value::U32(metadata_size)));

    // Offset 4: MetadataVersion — int32u
    let metadata_version = read_u32_le(data, 4);
    tags.push(mk("MetadataVersion", Value::U32(metadata_version)));

    // Offset 8: Bitmask1 — int32u
    let bitmask1 = read_u32_le(data, 8);
    let bm1_print = format!("0x{:08x}", bitmask1);
    tags.push(mk_print("Bitmask1", Value::U32(bitmask1), bm1_print));
    let mut bitm = bitmask1;

    // Offset 12: TimeStamp — double, if bitm & 0x01
    // OLE automation date: days since Dec 30, 1899
    if bitm & 0x01 != 0 {
        let v = read_f64_le(data, 12);
        let dt = ole_date_to_datetime(v);
        tags.push(mk_time("TimeStamp", Value::F64(v), dt));
    }

    // Offset 20: MicroscopeType — string[16], if bitm & 0x02
    if bitm & 0x02 != 0 {
        let s = read_str(data, 20, 16);
        tags.push(mk("MicroscopeType", Value::String(s)));
    }

    // Offset 36: MicroscopeID — string[16], if bitm & 0x04
    if bitm & 0x04 != 0 {
        let s = read_str(data, 36, 16);
        tags.push(mk("MicroscopeID", Value::String(s)));
    }

    // Offset 52: Application — string[16], if bitm & 0x08
    if bitm & 0x08 != 0 {
        let s = read_str(data, 52, 16);
        tags.push(mk("Application", Value::String(s)));
    }

    // Offset 68: AppVersion — string[16], if bitm & 0x10
    if bitm & 0x10 != 0 {
        let s = read_str(data, 68, 16);
        tags.push(mk("AppVersion", Value::String(s)));
    }

    // Offset 84: HighTension — double (volts), if bitm & 0x20
    if bitm & 0x20 != 0 {
        let v = read_f64_le(data, 84);
        tags.push(mk_print("HighTension", Value::F64(v), format_g15(v)));
    }

    // Offset 92: Dose — double (electrons/m2), if bitm & 0x40
    if bitm & 0x40 != 0 {
        let v = read_f64_le(data, 92);
        tags.push(mk_print("Dose", Value::F64(v), format_g15(v)));
    }

    // Offset 100: AlphaTilt — double, if bitm & 0x80
    if bitm & 0x80 != 0 {
        let v = read_f64_le(data, 100);
        tags.push(mk_print("AlphaTilt", Value::F64(v), format_g15(v)));
    }

    // Offset 108: BetaTilt — double, if bitm & 0x100
    if bitm & 0x100 != 0 {
        let v = read_f64_le(data, 108);
        tags.push(mk_print("BetaTilt", Value::F64(v), format_g15(v)));
    }

    // Offset 116: XStage — double, if bitm & 0x200
    if bitm & 0x200 != 0 {
        let v = read_f64_le(data, 116);
        tags.push(mk_print("XStage", Value::F64(v), format_g15(v)));
    }

    // Offset 124: YStage — double, if bitm & 0x400
    if bitm & 0x400 != 0 {
        let v = read_f64_le(data, 124);
        tags.push(mk_print("YStage", Value::F64(v), format_g15(v)));
    }

    // Offset 132: ZStage — double, if bitm & 0x800
    if bitm & 0x800 != 0 {
        let v = read_f64_le(data, 132);
        tags.push(mk_print("ZStage", Value::F64(v), format_g15(v)));
    }

    // Offset 140: TiltAxisAngle — double, if bitm & 0x1000
    if bitm & 0x1000 != 0 {
        let v = read_f64_le(data, 140);
        tags.push(mk_print("TiltAxisAngle", Value::F64(v), format_g15(v)));
    }

    // Offset 148: DualAxisRot — double, if bitm & 0x2000
    if bitm & 0x2000 != 0 {
        let v = read_f64_le(data, 148);
        tags.push(mk_print("DualAxisRot", Value::F64(v), format_g15(v)));
    }

    // Offset 156: PixelSizeX — double, if bitm & 0x4000
    if bitm & 0x4000 != 0 {
        let v = read_f64_le(data, 156);
        tags.push(mk_print("PixelSizeX", Value::F64(v), format_g15(v)));
    }

    // Offset 164: PixelSizeY — double, if bitm & 0x8000
    if bitm & 0x8000 != 0 {
        let v = read_f64_le(data, 164);
        tags.push(mk_print("PixelSizeY", Value::F64(v), format_g15(v)));
    }

    // Offset 220: Defocus — double, if bitm & 0x400000
    if bitm & 0x400000 != 0 {
        let v = read_f64_le(data, 220);
        tags.push(mk_print("Defocus", Value::F64(v), format_g15(v)));
    }

    // Offset 228: STEMDefocus — double, if bitm & 0x800000
    if bitm & 0x800000 != 0 {
        let v = read_f64_le(data, 228);
        tags.push(mk_print("STEMDefocus", Value::F64(v), format_g15(v)));
    }

    // Offset 236: AppliedDefocus — double, if bitm & 0x1000000
    if bitm & 0x1000000 != 0 {
        let v = read_f64_le(data, 236);
        tags.push(mk_print("AppliedDefocus", Value::F64(v), format_g15(v)));
    }

    // Offset 244: InstrumentMode — int32u, if bitm & 0x2000000, PrintConv {1=>TEM, 2=>STEM}
    if bitm & 0x2000000 != 0 {
        let v = read_u32_le(data, 244);
        let print = match v { 1 => "TEM", 2 => "STEM", _ => "Unknown" };
        tags.push(mk_print("InstrumentMode", Value::U32(v), print.to_string()));
    }

    // Offset 248: ProjectionMode — int32u, if bitm & 0x4000000
    // PrintConv {1=>Diffraction, 2=>Imaging}
    if bitm & 0x4000000 != 0 {
        let v = read_u32_le(data, 248);
        let print = match v { 1 => "Diffraction", 2 => "Imaging", _ => "Unknown" };
        tags.push(mk_print("ProjectionMode", Value::U32(v), print.to_string()));
    }

    // Offset 252: ObjectiveLens — string[16], if bitm & 0x8000000
    if bitm & 0x8000000 != 0 {
        let s = read_str(data, 252, 16);
        tags.push(mk("ObjectiveLens", Value::String(s)));
    }

    // Offset 268: HighMagnificationMode — string[16], if bitm & 0x10000000
    if bitm & 0x10000000 != 0 {
        let s = read_str(data, 268, 16);
        tags.push(mk("HighMagnificationMode", Value::String(s)));
    }

    // Offset 284: ProbeMode — int32u, if bitm & 0x20000000, PrintConv {1=>Nano, 2=>Micro}
    if bitm & 0x20000000 != 0 {
        let v = read_u32_le(data, 284);
        let print = match v { 1 => "Nano", 2 => "Micro", _ => "Unknown" };
        tags.push(mk_print("ProbeMode", Value::U32(v), print.to_string()));
    }

    // Offset 288: EFTEMOn — int8u, if bitm & 0x40000000, PrintConv {0=>No, 1=>Yes}
    if bitm & 0x40000000 != 0 {
        let v = if data.len() > 288 { data[288] } else { 0 };
        let print = if v == 0 { "No" } else { "Yes" };
        tags.push(mk_print("EFTEMOn", Value::U8(v), print.to_string()));
    }

    // Offset 289: Magnification — double, if bitm & 0x80000000
    if bitm & 0x80000000 != 0 {
        let v = read_f64_le(data, 289);
        tags.push(mk_print("Magnification", Value::F64(v), format_g15(v)));
    }

    // Offset 297: Bitmask2 — int32u (replaces bitm)
    if data.len() >= 301 {
        let bitmask2 = read_u32_le(data, 297);
        let bm2_print = format!("0x{:08x}", bitmask2);
        tags.push(mk_print("Bitmask2", Value::U32(bitmask2), bm2_print));
        bitm = bitmask2;

        // Offset 301: CameraLength — double, if bitm & 0x01
        if bitm & 0x01 != 0 {
            let v = read_f64_le(data, 301);
            tags.push(mk_print("CameraLength", Value::F64(v), format_g15(v)));
        }

        // Offset 309: SpotIndex — int32u, if bitm & 0x02
        if bitm & 0x02 != 0 {
            let v = read_u32_le(data, 309);
            tags.push(mk("SpotIndex", Value::U32(v)));
        }

        // Offset 313: IlluminationArea — double, if bitm & 0x04
        if bitm & 0x04 != 0 {
            let v = read_f64_le(data, 313);
            tags.push(mk_print("IlluminationArea", Value::F64(v), format_g15(v)));
        }

        // Offset 321: Intensity — double, if bitm & 0x08
        if bitm & 0x08 != 0 {
            let v = read_f64_le(data, 321);
            tags.push(mk_print("Intensity", Value::F64(v), format_g15(v)));
        }

        // Offset 329: ConvergenceAngle — double, if bitm & 0x10
        if bitm & 0x10 != 0 {
            let v = read_f64_le(data, 329);
            tags.push(mk_print("ConvergenceAngle", Value::F64(v), format_g15(v)));
        }

        // Offset 337: IlluminationMode — string[16], if bitm & 0x20
        if bitm & 0x20 != 0 {
            let s = read_str(data, 337, 16);
            tags.push(mk("IlluminationMode", Value::String(s)));
        }

        // Offset 353: WideConvergenceAngleRange — int8u bool, if bitm & 0x40
        if bitm & 0x40 != 0 {
            if data.len() > 353 {
                let v = data[353];
                let print = if v == 0 { "No" } else { "Yes" };
                tags.push(mk_print("WideConvergenceAngleRange", Value::U8(v), print.to_string()));
            }
        }

        // Offset 354: SlitInserted — int8u bool, if bitm & 0x80
        if bitm & 0x80 != 0 {
            if data.len() > 354 {
                let v = data[354];
                let print = if v == 0 { "No" } else { "Yes" };
                tags.push(mk_print("SlitInserted", Value::U8(v), print.to_string()));
            }
        }

        // Offset 355: SlitWidth — double, if bitm & 0x100
        if bitm & 0x100 != 0 {
            let v = read_f64_le(data, 355);
            tags.push(mk_print("SlitWidth", Value::F64(v), format_g15(v)));
        }

        // Offset 363: AccelVoltOffset — double, if bitm & 0x200
        if bitm & 0x200 != 0 {
            let v = read_f64_le(data, 363);
            tags.push(mk_print("AccelVoltOffset", Value::F64(v), format_g15(v)));
        }

        // Offset 371: DriftTubeVolt — double, if bitm & 0x400
        if bitm & 0x400 != 0 {
            let v = read_f64_le(data, 371);
            tags.push(mk_print("DriftTubeVolt", Value::F64(v), format_g15(v)));
        }

        // Offset 379: EnergyShift — double, if bitm & 0x800
        if bitm & 0x800 != 0 {
            let v = read_f64_le(data, 379);
            tags.push(mk_print("EnergyShift", Value::F64(v), format_g15(v)));
        }

        // Offset 387: ShiftOffsetX — double, if bitm & 0x1000
        if bitm & 0x1000 != 0 {
            let v = read_f64_le(data, 387);
            tags.push(mk_print("ShiftOffsetX", Value::F64(v), format_g15(v)));
        }

        // Offset 395: ShiftOffsetY — double, if bitm & 0x2000
        if bitm & 0x2000 != 0 {
            let v = read_f64_le(data, 395);
            tags.push(mk_print("ShiftOffsetY", Value::F64(v), format_g15(v)));
        }

        // Offset 403: ShiftX — double, if bitm & 0x4000
        if bitm & 0x4000 != 0 {
            let v = read_f64_le(data, 403);
            tags.push(mk_print("ShiftX", Value::F64(v), format_g15(v)));
        }

        // Offset 411: ShiftY — double, if bitm & 0x8000
        if bitm & 0x8000 != 0 {
            let v = read_f64_le(data, 411);
            tags.push(mk_print("ShiftY", Value::F64(v), format_g15(v)));
        }

        // Offset 419: IntegrationTime — double, if bitm & 0x10000
        if bitm & 0x10000 != 0 {
            let v = read_f64_le(data, 419);
            tags.push(mk_print("IntegrationTime", Value::F64(v), format_g15(v)));
        }

        // Offset 427: BinningWidth — int32u, if bitm & 0x20000
        if bitm & 0x20000 != 0 {
            let v = read_u32_le(data, 427);
            tags.push(mk("BinningWidth", Value::U32(v)));
        }

        // Offset 431: BinningHeight — int32u, if bitm & 0x40000
        if bitm & 0x40000 != 0 {
            let v = read_u32_le(data, 431);
            tags.push(mk("BinningHeight", Value::U32(v)));
        }

        // Offset 435: CameraName — string[16], if bitm & 0x80000
        if bitm & 0x80000 != 0 {
            let s = read_str(data, 435, 16);
            tags.push(mk("CameraName", Value::String(s)));
        }

        // Offset 451: ReadoutAreaLeft — int32u, if bitm & 0x100000
        if bitm & 0x100000 != 0 {
            let v = read_u32_le(data, 451);
            tags.push(mk("ReadoutAreaLeft", Value::U32(v)));
        }

        // Offset 455: ReadoutAreaTop — int32u, if bitm & 0x200000
        if bitm & 0x200000 != 0 {
            let v = read_u32_le(data, 455);
            tags.push(mk("ReadoutAreaTop", Value::U32(v)));
        }

        // Offset 459: ReadoutAreaRight — int32u, if bitm & 0x400000
        if bitm & 0x400000 != 0 {
            let v = read_u32_le(data, 459);
            tags.push(mk("ReadoutAreaRight", Value::U32(v)));
        }

        // Offset 463: ReadoutAreaBottom — int32u, if bitm & 0x800000
        if bitm & 0x800000 != 0 {
            let v = read_u32_le(data, 463);
            tags.push(mk("ReadoutAreaBottom", Value::U32(v)));
        }

        // Offset 467: CetaNoiseReduct — int8u bool, if bitm & 0x1000000
        if bitm & 0x1000000 != 0 {
            if data.len() > 467 {
                let v = data[467];
                let print = if v == 0 { "No" } else { "Yes" };
                tags.push(mk_print("CetaNoiseReduct", Value::U8(v), print.to_string()));
            }
        }

        // Offset 468: CetaFramesSummed — int32u, if bitm & 0x2000000
        if bitm & 0x2000000 != 0 {
            let v = read_u32_le(data, 468);
            tags.push(mk("CetaFramesSummed", Value::U32(v)));
        }

        // Offset 472: DirectDetElectronCounting — int8u bool, if bitm & 0x4000000
        if bitm & 0x4000000 != 0 {
            if data.len() > 472 {
                let v = data[472];
                let print = if v == 0 { "No" } else { "Yes" };
                tags.push(mk_print("DirectDetElectronCounting", Value::U8(v), print.to_string()));
            }
        }

        // Offset 473: DirectDetAlignFrames — int8u bool, if bitm & 0x8000000
        if bitm & 0x8000000 != 0 {
            if data.len() > 473 {
                let v = data[473];
                let print = if v == 0 { "No" } else { "Yes" };
                tags.push(mk_print("DirectDetAlignFrames", Value::U8(v), print.to_string()));
            }
        }
    }

    // Offset 490: Bitmask3 — int32u
    if data.len() >= 494 {
        let bitmask3 = read_u32_le(data, 490);
        let bm3_print = format!("0x{:08x}", bitmask3);
        tags.push(mk_print("Bitmask3", Value::U32(bitmask3), bm3_print));
        bitm = bitmask3;

        // Offset 518: PhasePlate — int8u bool, if bitm & 0x40
        if bitm & 0x40 != 0 {
            if data.len() > 518 {
                let v = data[518];
                let print = if v == 0 { "No" } else { "Yes" };
                tags.push(mk_print("PhasePlate", Value::U8(v), print.to_string()));
            }
        }

        // Offset 519: STEMDetectorName — string[16], if bitm & 0x80
        if bitm & 0x80 != 0 {
            let s = read_str(data, 519, 16);
            tags.push(mk("STEMDetectorName", Value::String(s)));
        }

        // Offset 535: Gain — double, if bitm & 0x100
        if bitm & 0x100 != 0 {
            let v = read_f64_le(data, 535);
            tags.push(mk_print("Gain", Value::F64(v), format_g15(v)));
        }

        // Offset 543: Offset — double, if bitm & 0x200
        if bitm & 0x200 != 0 {
            let v = read_f64_le(data, 543);
            tags.push(mk_print("Offset", Value::F64(v), format_g15(v)));
        }

        // Offset 571: DwellTime — double, if bitm & 0x8000
        if bitm & 0x8000 != 0 {
            let v = read_f64_le(data, 571);
            tags.push(mk_print("DwellTime", Value::F64(v), format_g15(v)));
        }

        // Offset 579: FrameTime — double, if bitm & 0x10000
        if bitm & 0x10000 != 0 {
            let v = read_f64_le(data, 579);
            tags.push(mk_print("FrameTime", Value::F64(v), format_g15(v)));
        }

        // Offset 587: ScanSizeLeft — int32u, if bitm & 0x20000
        if bitm & 0x20000 != 0 {
            let v = read_u32_le(data, 587);
            tags.push(mk("ScanSizeLeft", Value::U32(v)));
        }

        // Offset 591: ScanSizeTop — int32u, if bitm & 0x40000
        if bitm & 0x40000 != 0 {
            let v = read_u32_le(data, 591);
            tags.push(mk("ScanSizeTop", Value::U32(v)));
        }

        // Offset 595: ScanSizeRight — int32u, if bitm & 0x80000
        if bitm & 0x80000 != 0 {
            let v = read_u32_le(data, 595);
            tags.push(mk("ScanSizeRight", Value::U32(v)));
        }

        // Offset 599: ScanSizeBottom — int32u, if bitm & 0x100000
        if bitm & 0x100000 != 0 {
            let v = read_u32_le(data, 599);
            tags.push(mk("ScanSizeBottom", Value::U32(v)));
        }

        // Offset 603: FullScanFOV_X — double, if bitm & 0x200000
        if bitm & 0x200000 != 0 {
            let v = read_f64_le(data, 603);
            tags.push(mk_print("FullScanFOV_X", Value::F64(v), format_g15(v)));
        }

        // Offset 611: FullScanFOV_Y — double, if bitm & 0x400000
        if bitm & 0x400000 != 0 {
            let v = read_f64_le(data, 611);
            tags.push(mk_print("FullScanFOV_Y", Value::F64(v), format_g15(v)));
        }

        // Offset 619: Element — string[16], if bitm & 0x800000
        if bitm & 0x800000 != 0 {
            let s = read_str(data, 619, 16);
            tags.push(mk("Element", Value::String(s)));
        }

        // Offset 635: EnergyIntervalLower — double, if bitm & 0x1000000
        if bitm & 0x1000000 != 0 {
            let v = read_f64_le(data, 635);
            tags.push(mk_print("EnergyIntervalLower", Value::F64(v), format_g15(v)));
        }

        // Offset 643: EnergyIntervalHigher — double, if bitm & 0x2000000
        if bitm & 0x2000000 != 0 {
            let v = read_f64_le(data, 643);
            tags.push(mk_print("EnergyIntervalHigher", Value::F64(v), format_g15(v)));
        }

        // Offset 651: Method — int32u, if bitm & 0x4000000
        if bitm & 0x4000000 != 0 {
            let v = read_u32_le(data, 651);
            tags.push(mk("Method", Value::U32(v)));
        }

        // Offset 655: IsDoseFraction — int8u bool, if bitm & 0x8000000
        if bitm & 0x8000000 != 0 {
            if data.len() > 655 {
                let v = data[655];
                let print = if v == 0 { "No" } else { "Yes" };
                tags.push(mk_print("IsDoseFraction", Value::U8(v), print.to_string()));
            }
        }

        // Offset 656: FractionNumber — int32u, if bitm & 0x10000000
        if bitm & 0x10000000 != 0 {
            let v = read_u32_le(data, 656);
            tags.push(mk("FractionNumber", Value::U32(v)));
        }

        // Offset 660: StartFrame — int32u, if bitm & 0x20000000
        if bitm & 0x20000000 != 0 {
            let v = read_u32_le(data, 660);
            tags.push(mk("StartFrame", Value::U32(v)));
        }

        // Offset 664: EndFrame — int32u, if bitm & 0x40000000
        if bitm & 0x40000000 != 0 {
            let v = read_u32_le(data, 664);
            tags.push(mk("EndFrame", Value::U32(v)));
        }

        // Offset 668: InputStackFilename — string[80], if bitm & 0x80000000
        if bitm & 0x80000000 != 0 {
            let s = read_str(data, 668, 80);
            tags.push(mk("InputStackFilename", Value::String(s)));
        }
    }

    // Offset 748: Bitmask4 — int32u
    if data.len() >= 752 {
        let bitmask4 = read_u32_le(data, 748);
        let bm4_print = format!("0x{:08x}", bitmask4);
        tags.push(mk_print("Bitmask4", Value::U32(bitmask4), bm4_print));
        bitm = bitmask4;

        // Offset 752: AlphaTiltMin — double, if bitm & 0x01
        if bitm & 0x01 != 0 {
            let v = read_f64_le(data, 752);
            tags.push(mk_print("AlphaTiltMin", Value::F64(v), format_g15(v)));
        }

        // Offset 760: AlphaTiltMax — double, if bitm & 0x02
        if bitm & 0x02 != 0 {
            let v = read_f64_le(data, 760);
            tags.push(mk_print("AlphaTiltMax", Value::F64(v), format_g15(v)));
        }

        // FEI2 header starts here
        // Offset 768: ScanRotation — double, if bitm & 0x04
        if bitm & 0x04 != 0 {
            let v = read_f64_le(data, 768);
            tags.push(mk_print("ScanRotation", Value::F64(v), format_g15(v)));
        }

        // Offset 776: DiffractionPatternRotation — double, if bitm & 0x08
        if bitm & 0x08 != 0 {
            let v = read_f64_le(data, 776);
            tags.push(mk_print("DiffractionPatternRotation", Value::F64(v), format_g15(v)));
        }

        // Offset 784: ImageRotation — double, if bitm & 0x10
        if bitm & 0x10 != 0 {
            let v = read_f64_le(data, 784);
            tags.push(mk_print("ImageRotation", Value::F64(v), format_g15(v)));
        }

        // Offset 792: ScanModeEnumeration — int32u, if bitm & 0x20
        if bitm & 0x20 != 0 {
            let v = read_u32_le(data, 792);
            let print = match v { 0 => "Other", 1 => "Raster", 2 => "Serpentine", _ => "Unknown" };
            tags.push(mk_print("ScanModeEnumeration", Value::U32(v), print.to_string()));
        }

        // Offset 796: AcquisitionTimeStamp — int64u microseconds since Unix epoch, if bitm & 0x40
        if bitm & 0x40 != 0 {
            let v = read_u64_le(data, 796);
            let dt = usecs_to_datetime(v);
            tags.push(mk_time("AcquisitionTimeStamp", Value::U32(v as u32), dt));
        }

        // Offset 804: DetectorCommercialName — string[16], if bitm & 0x80
        if bitm & 0x80 != 0 {
            let s = read_str(data, 804, 16);
            tags.push(mk("DetectorCommercialName", Value::String(s)));
        }

        // Offset 820: StartTiltAngle — double, if bitm & 0x100
        if bitm & 0x100 != 0 {
            let v = read_f64_le(data, 820);
            tags.push(mk_print("StartTiltAngle", Value::F64(v), format_g15(v)));
        }

        // Offset 828: EndTiltAngle — double, if bitm & 0x200
        if bitm & 0x200 != 0 {
            let v = read_f64_le(data, 828);
            tags.push(mk_print("EndTiltAngle", Value::F64(v), format_g15(v)));
        }

        // Offset 836: TiltPerImage — double, if bitm & 0x400
        if bitm & 0x400 != 0 {
            let v = read_f64_le(data, 836);
            tags.push(mk_print("TiltPerImage", Value::F64(v), format_g15(v)));
        }

        // Offset 844: TitlSpeed — double, if bitm & 0x800
        if bitm & 0x800 != 0 {
            let v = read_f64_le(data, 844);
            tags.push(mk_print("TitlSpeed", Value::F64(v), format_g15(v)));
        }

        // Offset 852: BeamCenterX — int32u, if bitm & 0x1000
        if bitm & 0x1000 != 0 {
            let v = read_u32_le(data, 852);
            tags.push(mk("BeamCenterX", Value::U32(v)));
        }

        // Offset 856: BeamCenterY — int32u, if bitm & 0x2000
        if bitm & 0x2000 != 0 {
            let v = read_u32_le(data, 856);
            tags.push(mk("BeamCenterY", Value::U32(v)));
        }

        // Offset 860: CFEGFlashTimeStamp — int64u microseconds, if bitm & 0x4000
        if bitm & 0x4000 != 0 {
            let v = read_u64_le(data, 860);
            let dt = usecs_to_datetime(v);
            tags.push(mk_time("CFEGFlashTimeStamp", Value::U32(v as u32), dt));
        }

        // Offset 868: PhasePlatePosition — int32u, if bitm & 0x8000
        if bitm & 0x8000 != 0 {
            let v = read_u32_le(data, 868);
            tags.push(mk("PhasePlatePosition", Value::U32(v)));
        }

        // Offset 872: ObjectiveAperture — string[16], if bitm & 0x10000
        if bitm & 0x10000 != 0 {
            let s = read_str(data, 872, 16);
            tags.push(mk("ObjectiveAperture", Value::String(s)));
        }
    }
}

/// Validate and read an MRC file.
pub fn read_mrc(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 1024 {
        return Err(Error::InvalidData("MRC file too small".into()));
    }

    // Validate: axes at offsets 64-75 (three int32u, each value 1/2/3)
    // and "MAP" at offset 208 followed by machine stamp bytes
    let ax1 = read_u32_le(data, 64);
    let ax2 = read_u32_le(data, 68);
    let ax3 = read_u32_le(data, 72);
    if !(1..=3).contains(&ax1) || !(1..=3).contains(&ax2) || !(1..=3).contains(&ax3) {
        return Err(Error::InvalidData("Invalid MRC axis values".into()));
    }
    if &data[208..211] != b"MAP" {
        return Err(Error::InvalidData("Missing MRC MAP signature".into()));
    }
    // Machine stamp check: offset 212 must be 0x44/0x44 (LE), 0x44/0x41, or 0x11/0x11
    let ms0 = data[212];
    let ms1 = data[213];
    let valid_stamp = (ms0 == 0x44 && ms1 == 0x44)
        || (ms0 == 0x44 && ms1 == 0x41)
        || (ms0 == 0x11 && ms1 == 0x11);
    if !valid_stamp {
        return Err(Error::InvalidData("Invalid MRC machine stamp".into()));
    }

    let mut tags = Vec::new();

    // Process main 1024-byte header
    let (ext_hdr_size, image_depth, ext_hdr_type) =
        process_mrc_header(&data[..1024], &mut tags);

    // Process extended header (FEI1 or FEI2)
    if ext_hdr_size > 0 && (ext_hdr_type.starts_with("FEI1") || ext_hdr_type.starts_with("FEI2")) {
        let ext_start = 1024;
        let ext_end = ext_start + ext_hdr_size as usize;
        if ext_end <= data.len() {
            // Read size from first 4 bytes of extended header
            let section_size = read_u32_le(data, ext_start) as usize;
            if section_size > 0 && section_size <= ext_hdr_size as usize {
                // Check: size * ImageDepth <= ExtendedHeaderSize (from Perl validation)
                if section_size * (image_depth as usize) <= ext_hdr_size as usize {
                    let section_data = &data[ext_start..ext_start + section_size.min(data.len() - ext_start)];
                    process_fei12_header(section_data, &mut tags);
                    // Perl warns: 'Use the ExtractEmbedded option to read metadata for all frames'
                    // if there are more frames (we only read the first)
                    if image_depth > 1 {
                        tags.push(mk_print(
                            "Warning",
                            Value::String("[minor] Use the ExtractEmbedded option to read metadata for all frames".into()),
                            "[minor] Use the ExtractEmbedded option to read metadata for all frames".into(),
                        ));
                    }
                } else {
                    tags.push(mk_print(
                        "Warning",
                        Value::String("Corrupted extended header".into()),
                        "Corrupted extended header".into(),
                    ));
                }
            }
        } else {
            tags.push(mk_print(
                "Warning",
                Value::String("Error reading extended header".into()),
                "Error reading extended header".into(),
            ));
        }
    }

    Ok(tags)
}
