//! RED camera R3D format reader.
//! Mirrors ExifTool's Red.pm ProcessR3D.

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
            family0: "Red".into(),
            family1: "Red".into(),
            family2: "Camera".into(),
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
            family0: "Red".into(),
            family1: "Red".into(),
            family2: "Camera".into(),
        },
        raw_value: value,
        print_value: print,
        priority: 0,
    }
}

fn read_u16_be(data: &[u8], off: usize) -> u16 {
    if off + 2 > data.len() { return 0; }
    u16::from_be_bytes([data[off], data[off+1]])
}

fn read_u32_be(data: &[u8], off: usize) -> u32 {
    if off + 4 > data.len() { return 0; }
    u32::from_be_bytes([data[off], data[off+1], data[off+2], data[off+3]])
}

fn read_i32_be(data: &[u8], off: usize) -> i32 {
    read_u32_be(data, off) as i32
}

fn read_f32_be(data: &[u8], off: usize) -> f32 {
    if off + 4 > data.len() { return 0.0; }
    f32::from_bits(u32::from_be_bytes([data[off], data[off+1], data[off+2], data[off+3]]))
}

fn read_str(data: &[u8], off: usize, len: usize) -> String {
    if off + len > data.len() { return String::new(); }
    let s = &data[off..off+len];
    let end = s.iter().position(|&b| b == 0).unwrap_or(len);
    String::from_utf8_lossy(&s[..end]).trim().to_string()
}

fn convert_date(val: &str) -> String {
    // "YYYYMM" -> "YYYY:MM" or "YYYYMMDD" -> "YYYY:MM:DD"
    let v = val.replace('_', " ");
    if v.len() >= 8 {
        format!("{}:{}:{}", &v[0..4], &v[4..6], &v[6..8])
    } else if v.len() >= 6 {
        format!("{}:{}", &v[0..4], &v[4..6])
    } else {
        v
    }
}

fn convert_time(val: &str) -> String {
    // "HHMMSS" -> "HH:MM:SS" or "HHMM" -> "HH:MM"
    if val.len() >= 6 {
        format!("{}:{}:{}", &val[0..2], &val[2..4], &val[4..6])
    } else if val.len() >= 4 {
        format!("{}:{}", &val[0..2], &val[2..4])
    } else {
        val.to_string()
    }
}

/// Format a datetime from "YYYYMMDDHHMMSS"
fn convert_datetime(val: &str) -> String {
    if val.len() >= 14 {
        format!("{}:{}:{} {}:{}:{}", &val[0..4], &val[4..6], &val[6..8], &val[8..10], &val[10..12], &val[12..14])
    } else {
        val.to_string()
    }
}

/// Parse the RED directory entries starting at pos within buff
fn parse_red_dir(buff: &[u8], dir_start: usize, dir_end: usize, tags: &mut Vec<Tag>) {
    let mut pos = dir_start;

    while pos + 4 <= dir_end {
        let len = read_u16_be(buff, pos) as usize;
        if len < 4 || pos + len > dir_end { break; }
        let tag = read_u16_be(buff, pos + 2) as u32;
        let fmt_code = tag >> 12;
        let data = &buff[pos+4..pos+len];

        match fmt_code {
            1 => { // string
                let s = {
                    let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
                    String::from_utf8_lossy(&data[..end]).trim().to_string()
                };
                if !s.is_empty() {
                    match tag {
                        0x1000 => tags.push(mk("StartEdgeCode", Value::String(s))),
                        0x1001 => tags.push(mk("StartTimecode", Value::String(s))),
                        0x1005 => {
                            let dt = convert_datetime(&s);
                            tags.push(mk("DateTimeOriginal", Value::String(dt)));
                        }
                        0x1006 => tags.push(mk("SerialNumber", Value::String(s))),
                        0x1019 => tags.push(mk("CameraType", Value::String(s))),
                        0x101a => tags.push(mk("ReelNumber", Value::String(s))),
                        0x101b => tags.push(mk("Take", Value::String(s))),
                        0x1023 => {
                            let d = convert_date(&s);
                            tags.push(mk("DateCreated", Value::String(d)));
                        }
                        0x1024 => {
                            let t = convert_time(&s);
                            tags.push(mk("TimeCreated", Value::String(t)));
                        }
                        0x1025 => tags.push(mk("FirmwareVersion", Value::String(s))),
                        0x1029 => tags.push(mk("ReelTimecode", Value::String(s))),
                        0x102a => tags.push(mk("StorageType", Value::String(s))),
                        0x1030 => {
                            let d = convert_date(&s);
                            tags.push(mk("StorageFormatDate", Value::String(d)));
                        }
                        0x1031 => {
                            let t = convert_time(&s);
                            tags.push(mk("StorageFormatTime", Value::String(t)));
                        }
                        0x1032 => tags.push(mk("StorageSerialNumber", Value::String(s))),
                        0x1033 => tags.push(mk("StorageModel", Value::String(s))),
                        0x1036 => tags.push(mk("AspectRatio", Value::String(s))),
                        0x1056 => tags.push(mk("OriginalFileName", Value::String(s))),
                        0x106e => tags.push(mk("LensMake", Value::String(s))),
                        0x106f => tags.push(mk("LensNumber", Value::String(s))),
                        0x1070 => tags.push(mk("LensModel", Value::String(s))),
                        0x1071 => tags.push(mk("Model", Value::String(s))),
                        0x107c => tags.push(mk("CameraOperator", Value::String(s))),
                        0x1086 => tags.push(mk("VideoFormat", Value::String(s))),
                        0x1096 => tags.push(mk("Filter", Value::String(s))),
                        0x10a0 => tags.push(mk("Brain", Value::String(s))),
                        0x10a1 => tags.push(mk("Sensor", Value::String(s))),
                        0x10be => tags.push(mk("Quality", Value::String(s))),
                        _ => {}
                    }
                }
            }
            2 => { // float
                if data.len() >= 4 {
                    let f = read_f32_be(data, 0);
                    match tag {
                        0x200d => tags.push(mk("ColorTemperature", Value::F64(f as f64))),
                        0x204b => {
                            // RGBCurves: multiple floats
                            let count = data.len() / 4;
                            let mut vals = Vec::new();
                            for i in 0..count {
                                let v = read_f32_be(data, i*4);
                                vals.push(format!("{}", v));
                            }
                            tags.push(mk("RGBCurves", Value::String(vals.join(" "))));
                        }
                        0x2066 => {
                            let print = format!("{}", (f * 1000.0 + 0.5) as u32 as f32 / 1000.0);
                            tags.push(mk_print("OriginalFrameRate", Value::F64(f as f64), print));
                        }
                        _ => {}
                    }
                }
            }
            4 => { // int16u
                if data.len() >= 2 {
                    match tag {
                        0x4037 => {
                            // CropArea: 4 int16u values
                            if data.len() >= 8 {
                                let a = read_u16_be(data, 0);
                                let b = read_u16_be(data, 2);
                                let c = read_u16_be(data, 4);
                                let d2 = read_u16_be(data, 6);
                                tags.push(mk("CropArea", Value::String(format!("{} {} {} {}", a, b, c, d2))));
                            }
                        }
                        0x403b => {
                            let v = read_u16_be(data, 0);
                            tags.push(mk("ISO", Value::U16(v)));
                        }
                        0x406a => {
                            let v = read_u16_be(data, 0);
                            let f = v as f64 / 10.0;
                            tags.push(mk_print("FNumber", Value::F64(f), format!("{:.1}", f)));
                        }
                        0x406b => {
                            let v = read_u16_be(data, 0);
                            tags.push(mk("FocalLength", Value::U16(v)));
                        }
                        _ => {}
                    }
                }
            }
            6 => { // int32s
                if data.len() >= 4 {
                    match tag {
                        0x606c => {
                            let v = read_i32_be(data, 0);
                            let meters = v as f64 / 1000.0;
                            tags.push(mk_print("FocusDistance", Value::F64(meters), format!("{} m", meters)));
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }

        pos += len;
    }
}

pub fn read_r3d(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 8 {
        return Err(Error::InvalidData("file too small for R3D".into()));
    }

    // Validate: starts with \0\0..RED(1|2)
    if data[0] != 0 || data[1] != 0 {
        return Err(Error::InvalidData("not an R3D file".into()));
    }
    let ver = match &data[4..8] {
        b"RED1" => 1u8,
        b"RED2" => 2u8,
        _ => return Err(Error::InvalidData("not an R3D file".into())),
    };

    let mut tags = Vec::new();
    let block_size = read_u32_be(data, 0) as usize;

    if block_size < 8 || block_size > data.len() {
        return Err(Error::InvalidData("invalid R3D block size".into()));
    }

    // Extract header tags based on version
    match ver {
        1 => {
            // RED1 header
            if block_size >= 0x08 {
                // RedcodeVersion: offset 0x07, string[1]
                if 0x07 < block_size {
                    let rv = format!("{}", data[0x07] as char);
                    tags.push(mk("RedcodeVersion", Value::String(rv)));
                }
                // ImageWidth: offset 0x36, int16u
                if 0x36 + 2 <= block_size {
                    let w = read_u16_be(data, 0x36);
                    tags.push(mk("ImageWidth", Value::U16(w)));
                }
                // ImageHeight: offset 0x3a, int16u
                if 0x3a + 2 <= block_size {
                    let h = read_u16_be(data, 0x3a);
                    tags.push(mk("ImageHeight", Value::U16(h)));
                }
                // FrameRate: offset 0x3e, rational32u (numerator/denominator)
                if 0x42 + 4 <= block_size {
                    let num = read_u32_be(data, 0x3e);
                    let den = read_u32_be(data, 0x42);
                    if den > 0 {
                        let fr = num as f64 / den as f64;
                        let print = format!("{:.3}", fr);
                        // Trim trailing zeros
                        let print = print.trim_end_matches('0').trim_end_matches('.').to_string();
                        tags.push(mk_print("FrameRate", Value::F64(fr), print));
                    }
                }
                // OriginalFileName: offset 0x43, string[32]
                if 0x43 + 32 <= block_size {
                    let fn_str = read_str(data, 0x43, 32);
                    if !fn_str.is_empty() {
                        tags.push(mk("OriginalFileName", Value::String(fn_str)));
                    }
                }
            }

            // For version 1, read next block for directory
            let next_block_start = block_size;
            if next_block_start + 0x22 <= data.len() {
                let next_size = read_u32_be(data, next_block_start) as usize;
                if next_size >= 8 && next_block_start + next_size <= data.len() {
                    let buff = &data[next_block_start..next_block_start + next_size];
                    let dir_start = 0x22;
                    if dir_start + 2 <= buff.len() {
                        let dir_len = read_u16_be(buff, dir_start) as usize;
                        let dir_content_start = dir_start + 2;
                        let dir_end = dir_content_start + dir_len;
                        let dir_end = dir_end.min(buff.len());
                        parse_red_dir(buff, dir_content_start, dir_end, &mut tags);
                    }
                }
            }
        }
        2 => {
            // RED2 header
            if 0x07 < block_size {
                let rv = format!("{}", data[0x07] as char);
                tags.push(mk("RedcodeVersion", Value::String(rv)));
            }

            // rdi records
            let rdi_count = if 0x40 < block_size { data[0x40] as usize } else { 0 };
            let rda_count = if 0x41 < block_size { data[0x41] as usize } else { 0 };
            let rdx_count = if 0x42 < block_size { data[0x42] as usize } else { 0 };

            // First rdi record starts at 0x44
            let first_rdi = 0x44;
            if first_rdi + 0x18 <= block_size {
                // ImageWidth: offset 0x4c within block
                let w = read_u32_be(data, 0x4c);
                tags.push(mk("ImageWidth", Value::U32(w)));
                // ImageHeight: offset 0x50
                let h = read_u32_be(data, 0x50);
                tags.push(mk("ImageHeight", Value::U32(h)));
                // FrameRate: offset 0x56, int16u[3]
                // ValueConv: (a[1] * 0x10000 + a[2]) / a[0]
                if 0x56 + 6 <= block_size {
                    let a0 = read_u16_be(data, 0x56) as u32;
                    let a1 = read_u16_be(data, 0x58) as u32;
                    let a2 = read_u16_be(data, 0x5a) as u32;
                    if a0 > 0 {
                        let fr = (a1 * 0x10000 + a2) as f64 / a0 as f64;
                        let print = format!("{:.3}", fr);
                        let print = print.trim_end_matches('0').trim_end_matches('.').to_string();
                        tags.push(mk_print("FrameRate", Value::F64(fr), print));
                    }
                }
            }

            // Directory starts after header records
            let dir_start = 0x44
                + rdi_count * 0x18
                + rda_count * 0x14
                + rdx_count * 0x10;

            if dir_start + 2 <= block_size {
                let dir_len = read_u16_be(data, dir_start) as usize;
                let dir_content_start = dir_start + 2;
                let dir_end = dir_content_start + dir_len;
                let dir_end = dir_end.min(block_size);
                parse_red_dir(data, dir_content_start, dir_end, &mut tags);
            }
        }
        _ => {}
    }

    Ok(tags)
}
