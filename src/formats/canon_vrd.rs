//! Canon VRD and DR4 recipe data format reader.
//!
//! VRD files start with "CANON OPTIONAL DATA\0" (header + footer) and contain
//! blocks of adjustment data. DR4 files start with "IIII\x04\x00\x04\x00" and
//! use a directory-based format with typed entries.
//!
//! Mirrors ExifTool's CanonVRD.pm.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

// ============================================================================
// Helpers
// ============================================================================

fn mktag(group: &str, name: &str, raw: Value, print: String) -> Tag {
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: group.to_string(),
            family1: group.to_string(),
            family2: "Image".to_string(),
        },
        raw_value: raw,
        print_value: print,
        priority: 5,
    }
}

fn read_u8(data: &[u8], off: usize) -> u8 {
    data[off]
}

fn read_i8(data: &[u8], off: usize) -> i8 {
    data[off] as i8
}

fn read_u16_le(data: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([data[off], data[off + 1]])
}

fn read_i16_le(data: &[u8], off: usize) -> i16 {
    i16::from_le_bytes([data[off], data[off + 1]])
}

fn read_u16_be(data: &[u8], off: usize) -> u16 {
    u16::from_be_bytes([data[off], data[off + 1]])
}

fn read_i16_be(data: &[u8], off: usize) -> i16 {
    i16::from_be_bytes([data[off], data[off + 1]])
}

fn read_i32_be(data: &[u8], off: usize) -> i32 {
    i32::from_be_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

fn read_f32_be(data: &[u8], off: usize) -> f32 {
    f32::from_be_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

fn read_u32_le(data: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

fn read_i32_le(data: &[u8], off: usize) -> i32 {
    i32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

fn read_u32_be(data: &[u8], off: usize) -> u32 {
    u32::from_be_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

fn read_f32_le(data: &[u8], off: usize) -> f32 {
    f32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

fn read_f64_le(data: &[u8], off: usize) -> f64 {
    f64::from_le_bytes([
        data[off],
        data[off + 1],
        data[off + 2],
        data[off + 3],
        data[off + 4],
        data[off + 5],
        data[off + 6],
        data[off + 7],
    ])
}

fn no_yes(v: u32) -> String {
    if v == 0 { "No".to_string() } else { "Yes".to_string() }
}

// ============================================================================
// DR4 format reader
// ============================================================================

/// DR4 format codes → (element_size, is_signed, is_float, is_string)
fn dr4_format_size(fmt: u32) -> usize {
    match fmt {
        1 => 4,   // int32u
        2 => 1,   // string (bytes)
        8 => 4,   // int32u
        9 => 4,   // int32s
        13 => 8,  // double
        24 => 4,  // int32s (rectangle coordinates)
        33 => 4,  // int32u (array)
        38 => 8,  // double (array)
        255 => 1, // undef
        _ => 1,
    }
}

/// Read a DR4 value and return it as a Value
fn read_dr4_value(data: &[u8], off: usize, len: usize, fmt: u32) -> Value {
    if off + len > data.len() || len == 0 {
        return Value::Binary(Vec::new());
    }
    let elem_size = dr4_format_size(fmt);
    let count = if elem_size > 0 { len / elem_size } else { 0 };
    match fmt {
        2 => {
            // string
            let s = String::from_utf8_lossy(&data[off..off + len])
                .trim_end_matches('\0')
                .to_string();
            Value::String(s)
        }
        1 | 8 => {
            // int32u
            if len >= 4 {
                if count == 1 {
                    Value::U32(read_u32_le(data, off))
                } else {
                    let vals: Vec<Value> = (0..count)
                        .map(|i| Value::U32(read_u32_le(data, off + i * 4)))
                        .collect();
                    Value::List(vals)
                }
            } else {
                Value::Binary(data[off..off + len].to_vec())
            }
        }
        9 | 24 => {
            // int32s
            if len >= 4 {
                if count == 1 {
                    Value::I32(read_i32_le(data, off))
                } else {
                    let vals: Vec<Value> = (0..count)
                        .map(|i| Value::I32(read_i32_le(data, off + i * 4)))
                        .collect();
                    Value::List(vals)
                }
            } else {
                Value::Binary(data[off..off + len].to_vec())
            }
        }
        13 | 38 => {
            // double
            if len >= 8 {
                if count == 1 {
                    let v = read_f64_le(data, off);
                    let v = if v.abs() < 1e-100 { 0.0 } else { v };
                    Value::F64(v)
                } else {
                    let vals: Vec<Value> = (0..count)
                        .map(|i| {
                            let v = read_f64_le(data, off + i * 8);
                            let v = if v.abs() < 1e-100 { 0.0 } else { v };
                            Value::F64(v)
                        })
                        .collect();
                    Value::List(vals)
                }
            } else {
                Value::Binary(data[off..off + len].to_vec())
            }
        }
        _ => Value::Binary(data[off..off + len].to_vec()),
    }
}

/// Format a Value as a print string
fn value_to_print(val: &Value) -> String {
    match val {
        Value::String(s) => s.clone(),
        Value::U32(v) => v.to_string(),
        Value::I32(v) => v.to_string(),
        Value::F64(v) => format!("{}", v),
        Value::List(vals) => {
            vals.iter()
                .map(|v| value_to_print(v))
                .collect::<Vec<_>>()
                .join(" ")
        }
        Value::Binary(b) => format!("(binary {} bytes)", b.len()),
        _ => val.to_display_string(),
    }
}

/// Canon model ID lookup
fn canon_model_id(id: u32) -> &'static str {
    match id {
        0x80000001 => "EOS-1D",
        0x80000167 => "EOS-1DS",
        0x80000168 => "EOS 10D",
        0x80000169 => "EOS-1D Mark III",
        0x80000170 => "EOS Digital Rebel / 300D / Kiss Digital",
        0x80000174 => "EOS-1D Mark II",
        0x80000175 => "EOS 20D",
        0x80000176 => "EOS Digital Rebel XSi / 450D / Kiss X2",
        0x80000188 => "EOS-1Ds Mark II",
        0x80000189 => "EOS Digital Rebel XT / 350D / Kiss Digital N",
        0x80000190 => "EOS 40D",
        0x80000213 => "EOS 5D",
        0x80000215 => "EOS-1Ds Mark III",
        0x80000218 => "EOS 5D Mark II",
        0x80000232 => "EOS-1D Mark II N",
        0x80000234 => "EOS 30D",
        0x80000236 => "EOS Digital Rebel XTi / 400D / Kiss Digital X",
        0x80000250 => "EOS 7D",
        0x80000252 => "EOS Rebel T1i / 500D / Kiss X3",
        0x80000254 => "EOS Rebel XS / 1000D / Kiss F",
        0x80000261 => "EOS 50D",
        0x80000269 => "EOS-1D X",
        0x80000270 => "EOS Rebel T2i / 550D / Kiss X4",
        0x80000281 => "EOS-1D Mark IV",
        0x80000285 => "EOS 5D Mark III",
        0x80000286 => "EOS Rebel T3i / 600D / Kiss X5",
        0x80000287 => "EOS 60D",
        0x80000288 => "EOS Rebel T3 / 1100D / Kiss X50",
        0x80000289 => "EOS 7D Mark II",
        0x80000301 => "EOS Rebel T4i / 650D / Kiss X6i",
        0x80000302 => "EOS 6D",
        0x80000324 => "EOS-1D C",
        0x80000325 => "EOS 70D",
        0x80000326 => "EOS Rebel T5i / 700D / Kiss X7i",
        0x80000327 => "EOS Rebel T5 / 1200D / Kiss X70 / Hi",
        0x80000328 => "EOS-1D X Mark II",
        0x80000331 => "EOS M",
        0x80000346 => "EOS Rebel SL1 / 100D / Kiss X7",
        0x80000347 => "EOS Rebel T6s / 760D / 8000D",
        0x80000349 => "EOS 5D Mark IV",
        0x80000350 => "EOS 80D",
        0x80000355 => "EOS M2",
        0x80000382 => "EOS 5DS",
        0x80000393 => "EOS Rebel T6i / 750D / Kiss X8i",
        0x80000401 => "EOS 5DS R",
        0x80000404 => "EOS Rebel T6 / 1300D / Kiss X80",
        0x80000405 => "EOS Rebel T7i / 800D / Kiss X9i",
        0x80000406 => "EOS 6D Mark II",
        0x80000408 => "EOS 77D / 9000D",
        0x80000417 => "EOS Rebel SL2 / 200D / Kiss X9",
        0x80000421 => "EOS R5",
        0x80000422 => "EOS Rebel T100 / 4000D / 3000D",
        0x80000424 => "EOS R",
        0x80000428 => "EOS-1D X Mark III",
        0x80000432 => "EOS Rebel T7 / 2000D / 1500D / Kiss X90",
        0x80000433 => "EOS RP",
        0x80000435 => "EOS Rebel T8i / 850D / X10i",
        0x80000436 => "EOS SL3 / 250D / Kiss X10",
        0x80000437 => "EOS 90D",
        0x80000450 => "EOS R3",
        0x80000453 => "EOS R6",
        0x80000464 => "EOS R7",
        0x80000465 => "EOS R10",
        0x80000468 => "EOS M50 Mark II / Kiss M2",
        0x80000480 => "EOS R50",
        0x80000481 => "EOS R6 Mark II",
        0x80000487 => "EOS R8",
        0x80000495 => "EOS R1",
        0x80000496 => "EOS R5 Mark II",
        0x80000498 => "EOS R100",
        _ => "",
    }
}

/// Tone curve print conversion: interpret 21-element array
fn tone_curve_print(vals: &[u32]) -> String {
    if vals.len() != 21 { return vals.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" "); }
    let n = vals[0] as usize;
    if n < 2 || n > 10 { return vals.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" "); }
    let mut result = String::new();
    for i in 0..n {
        if i > 0 { result.push(' '); }
        result.push('(');
        result.push_str(&vals[1 + i * 2].to_string());
        result.push(',');
        result.push_str(&vals[2 + i * 2].to_string());
        result.push(')');
    }
    result
}

/// Parse the ToneCurve subdirectory (tag 0x20400) which is int32u[...] binary data
fn parse_dr4_tone_curve(data: &[u8], off: usize, len: usize, tags: &mut Vec<Tag>) {
    // ToneCurve subdirectory: int32u values
    // 0x00: ToneCurveColorSpace
    // 0x01: ToneCurveShape
    // 0x03: ToneCurveInputRange (2 int32u)
    // 0x05: ToneCurveOutputRange (2 int32u)
    // 0x07: RGBCurvePoints (21 int32u)
    // 0x0a: ToneCurveX (1 int32u)
    // 0x0b: ToneCurveY (1 int32u)
    // 0x2d: RedCurvePoints (21 int32u)
    // 0x53: GreenCurvePoints (21 int32u)
    // 0x79: BlueCurvePoints (21 int32u)
    if off + len > data.len() { return; }

    let read_u32_at = |idx: usize| -> Option<u32> {
        let byte_off = off + idx * 4;
        if byte_off + 4 <= off + len { Some(read_u32_le(data, byte_off)) } else { None }
    };
    let read_u32s_at = |idx: usize, count: usize| -> Option<Vec<u32>> {
        let byte_off = off + idx * 4;
        if byte_off + count * 4 <= off + len {
            Some((0..count).map(|i| read_u32_le(data, byte_off + i * 4)).collect())
        } else { None }
    };

    // ToneCurveColorSpace
    if let Some(v) = read_u32_at(0x00) {
        let print = match v { 0 => "RGB".to_string(), 1 => "Luminance".to_string(), _ => v.to_string() };
        tags.push(mktag("CanonDR4", "ToneCurveColorSpace", Value::U32(v), print));
    }
    // ToneCurveShape
    if let Some(v) = read_u32_at(0x01) {
        let print = match v { 0 => "Curve".to_string(), 1 => "Straight".to_string(), _ => v.to_string() };
        tags.push(mktag("CanonDR4", "ToneCurveShape", Value::U32(v), print));
    }
    // ToneCurveInputRange (0x03..0x04)
    if let Some(vals) = read_u32s_at(0x03, 2) {
        let print = format!("{} {}", vals[0], vals[1]);
        let raw = Value::List(vals.iter().map(|&v| Value::U32(v)).collect());
        tags.push(mktag("CanonDR4", "ToneCurveInputRange", raw, print));
    }
    // ToneCurveOutputRange (0x05..0x06)
    if let Some(vals) = read_u32s_at(0x05, 2) {
        let print = format!("{} {}", vals[0], vals[1]);
        let raw = Value::List(vals.iter().map(|&v| Value::U32(v)).collect());
        tags.push(mktag("CanonDR4", "ToneCurveOutputRange", raw, print));
    }
    // RGBCurvePoints (0x07..0x1b = 21 elements)
    if let Some(vals) = read_u32s_at(0x07, 21) {
        let print = tone_curve_print(&vals);
        let raw = Value::List(vals.iter().map(|&v| Value::U32(v)).collect());
        tags.push(mktag("CanonDR4", "RGBCurvePoints", raw, print));
    }
    // ToneCurveX (0x0a)
    if let Some(v) = read_u32_at(0x0a) {
        tags.push(mktag("CanonDR4", "ToneCurveX", Value::U32(v), v.to_string()));
    }
    // ToneCurveY (0x0b)
    if let Some(v) = read_u32_at(0x0b) {
        tags.push(mktag("CanonDR4", "ToneCurveY", Value::U32(v), v.to_string()));
    }
    // RedCurvePoints (0x2d..0x41 = 21 elements)
    if let Some(vals) = read_u32s_at(0x2d, 21) {
        let print = tone_curve_print(&vals);
        let raw = Value::List(vals.iter().map(|&v| Value::U32(v)).collect());
        tags.push(mktag("CanonDR4", "RedCurvePoints", raw, print));
    }
    // GreenCurvePoints (0x53..0x67)
    if let Some(vals) = read_u32s_at(0x53, 21) {
        let print = tone_curve_print(&vals);
        let raw = Value::List(vals.iter().map(|&v| Value::U32(v)).collect());
        tags.push(mktag("CanonDR4", "GreenCurvePoints", raw, print));
    }
    // BlueCurvePoints (0x79..0x8d)
    if let Some(vals) = read_u32s_at(0x79, 21) {
        let print = tone_curve_print(&vals);
        let raw = Value::List(vals.iter().map(|&v| Value::U32(v)).collect());
        tags.push(mktag("CanonDR4", "BlueCurvePoints", raw, print));
    }
}

/// Parse GammaInfo subdirectory (tag 0x20a00) which is f64 values
fn parse_dr4_gamma_info(data: &[u8], off: usize, len: usize, tags: &mut Vec<Tag>) {
    if off + len > data.len() { return; }
    let read_f64_at = |idx: usize| -> Option<f64> {
        let byte_off = off + idx * 8;
        if byte_off + 8 <= off + len {
            let v = read_f64_le(data, byte_off);
            Some(if v.abs() < 1e-100 { 0.0 } else { v })
        } else { None }
    };
    let read_f64s_at = |idx: usize, count: usize| -> Option<Vec<f64>> {
        let byte_off = off + idx * 8;
        if byte_off + count * 8 <= off + len {
            Some((0..count).map(|i| {
                let v = read_f64_le(data, byte_off + i * 8);
                if v.abs() < 1e-100 { 0.0 } else { v }
            }).collect())
        } else { None }
    };

    if let Some(v) = read_f64_at(0x02) {
        tags.push(mktag("CanonDR4", "GammaContrast", Value::F64(v), format!("{}", v)));
    }
    if let Some(v) = read_f64_at(0x03) {
        tags.push(mktag("CanonDR4", "GammaColorTone", Value::F64(v), format!("{}", v)));
    }
    if let Some(v) = read_f64_at(0x04) {
        tags.push(mktag("CanonDR4", "GammaSaturation", Value::F64(v), format!("{}", v)));
    }
    if let Some(v) = read_f64_at(0x05) {
        tags.push(mktag("CanonDR4", "GammaUnsharpMaskStrength", Value::F64(v), format!("{}", v)));
    }
    if let Some(v) = read_f64_at(0x06) {
        tags.push(mktag("CanonDR4", "GammaUnsharpMaskFineness", Value::F64(v), format!("{}", v)));
    }
    if let Some(v) = read_f64_at(0x07) {
        tags.push(mktag("CanonDR4", "GammaUnsharpMaskThreshold", Value::F64(v), format!("{}", v)));
    }
    if let Some(v) = read_f64_at(0x08) {
        tags.push(mktag("CanonDR4", "GammaSharpnessStrength", Value::F64(v), format!("{}", v)));
    }
    if let Some(v) = read_f64_at(0x09) {
        tags.push(mktag("CanonDR4", "GammaShadow", Value::F64(v), format!("{}", v)));
    }
    if let Some(v) = read_f64_at(0x0a) {
        tags.push(mktag("CanonDR4", "GammaHighlight", Value::F64(v), format!("{}", v)));
    }
    // GammaBlackPoint: complex conversion
    if let Some(v) = read_f64_at(0x0c) {
        let conv = if v <= 0.0 {
            0.0
        } else {
            let r = (v / 4.6875_f64).ln() / 2.0_f64.ln() + 1.0;
            if r.abs() > 1e-10 { r } else { 0.0 }
        };
        tags.push(mktag("CanonDR4", "GammaBlackPoint", Value::F64(conv), format!("{:+.3}", conv)));
    }
    // GammaWhitePoint: complex conversion
    if let Some(v) = read_f64_at(0x0d) {
        let conv = if v <= 0.0 {
            0.0
        } else {
            let r = (v / 4.6875_f64).ln() / 2.0_f64.ln() - 11.77109325169954;
            if r.abs() > 1e-10 { r } else { 0.0 }
        };
        tags.push(mktag("CanonDR4", "GammaWhitePoint", Value::F64(conv), format!("{:+.3}", conv)));
    }
    // GammaMidPoint: complex conversion
    if let Some(v) = read_f64_at(0x0e) {
        let conv = if v <= 0.0 {
            0.0
        } else {
            let r = (v / 4.6875_f64).ln() / 2.0_f64.ln() - 8.0;
            if r.abs() > 1e-10 { r } else { 0.0 }
        };
        tags.push(mktag("CanonDR4", "GammaMidPoint", Value::F64(conv), format!("{:+.3}", conv)));
    }
    // GammaCurveOutputRange: 2 doubles at 0x0f
    if let Some(vals) = read_f64s_at(0x0f, 2) {
        let print = format!("{} {}", vals[0] as i64, vals[1] as i64);
        let raw = Value::List(vals.iter().map(|&v| Value::F64(v)).collect());
        tags.push(mktag("CanonDR4", "GammaCurveOutputRange", raw, print));
    }
}

/// Parse CropInfo subdirectory (tag 0xf0100)
fn parse_dr4_crop_info(data: &[u8], off: usize, len: usize, tags: &mut Vec<Tag>) {
    if off + len > data.len() { return; }
    let read_i32_at = |idx: usize| -> Option<i32> {
        let byte_off = off + idx * 4;
        if byte_off + 4 <= off + len { Some(read_i32_le(data, byte_off)) } else { None }
    };

    // 0: CropActive
    if let Some(v) = read_i32_at(0) {
        let print = no_yes(v as u32);
        tags.push(mktag("CanonDR4", "CropActive", Value::I32(v), print));
    }
    // 1: CropRotatedOriginalWidth
    if let Some(v) = read_i32_at(1) {
        tags.push(mktag("CanonDR4", "CropRotatedOriginalWidth", Value::I32(v), v.to_string()));
    }
    // 2: CropRotatedOriginalHeight
    if let Some(v) = read_i32_at(2) {
        tags.push(mktag("CanonDR4", "CropRotatedOriginalHeight", Value::I32(v), v.to_string()));
    }
    // 3: CropX
    if let Some(v) = read_i32_at(3) {
        tags.push(mktag("CanonDR4", "CropX", Value::I32(v), v.to_string()));
    }
    // 4: CropY
    if let Some(v) = read_i32_at(4) {
        tags.push(mktag("CanonDR4", "CropY", Value::I32(v), v.to_string()));
    }
    // 5: CropWidth
    if let Some(v) = read_i32_at(5) {
        tags.push(mktag("CanonDR4", "CropWidth", Value::I32(v), v.to_string()));
    }
    // 6: CropHeight
    if let Some(v) = read_i32_at(6) {
        tags.push(mktag("CanonDR4", "CropHeight", Value::I32(v), v.to_string()));
    }
    // 7: CropRotation
    if let Some(v) = read_i32_at(7) {
        tags.push(mktag("CanonDR4", "CropRotation", Value::I32(v), v.to_string()));
    }
    // 8-9: CropAngle (double at byte offset 32)
    let angle_off = off + 8 * 4;
    if angle_off + 8 <= off + len {
        let v = read_f64_le(data, angle_off);
        let print = format!("{:.7}", v).trim_end_matches('0').trim_end_matches('.').to_string();
        // Perl does sprintf("%.7g",$val)
        let print = format_g(v, 7);
        tags.push(mktag("CanonDR4", "CropAngle", Value::F64(v), print));
    }
    // 10: CropOriginalWidth (at index 10 counting i32 = byte 40, but after the double at 8-9)
    // Layout: 0..7 are i32 (4 bytes each = 32 bytes), then double at 32 (8 bytes), then i32 at 40, 44
    let orig_w_off = off + 32 + 8; // byte 40
    if orig_w_off + 4 <= off + len {
        let v = read_i32_le(data, orig_w_off);
        tags.push(mktag("CanonDR4", "CropOriginalWidth", Value::I32(v), v.to_string()));
    }
    let orig_h_off = off + 32 + 8 + 4; // byte 44
    if orig_h_off + 4 <= off + len {
        let v = read_i32_le(data, orig_h_off);
        tags.push(mktag("CanonDR4", "CropOriginalHeight", Value::I32(v), v.to_string()));
    }
}

/// Parse StampInfo subdirectory (tag 0xf0510)
fn parse_dr4_stamp_info(data: &[u8], off: usize, len: usize, tags: &mut Vec<Tag>) {
    // FORMAT => 'int32u', 0x02 => 'StampToolCount'
    let byte_off = off + 0x02 * 4;
    if byte_off + 4 <= off + len {
        let v = read_u32_le(data, byte_off);
        tags.push(mktag("CanonDR4", "StampToolCount", Value::U32(v), v.to_string()));
    }
}

/// Format a f64 with %g semantics (up to `sig` significant digits)
fn format_g(v: f64, sig: usize) -> String {
    if v == 0.0 { return "0".to_string(); }
    let formatted = format!("{:.prec$e}", v, prec = sig.saturating_sub(1));
    // Parse to determine if exponential or fixed notation is better
    // Use Rust's default Display which already does something like %g
    let s = format!("{:.*}", sig, v);
    // Actually, use a simpler approach: format with enough precision, then strip trailing zeros
    let s = format!("{}", v);
    s
}

/// HSL tag print conversion: 3 doubles formatted with up to 4 significant digits
fn format_hsl(data: &[u8], off: usize, len: usize) -> String {
    if off + len > data.len() || len < 24 { return String::new(); }
    let v0 = read_f64_le(data, off);
    let v1 = read_f64_le(data, off + 8);
    let v2 = read_f64_le(data, off + 16);
    // Perl uses sprintf "%g", $val which gives 6 significant digits max
    format!("{} {} {}", fmt_g(v0), fmt_g(v1), fmt_g(v2))
}

fn fmt_g(v: f64) -> String {
    if v == 0.0 { return "0".to_string(); }
    // Use Rust's default formatting which approximates %g
    let s = format!("{}", v);
    s
}

/// Process DR4 directory entries and emit tags
fn process_dr4_entries(data: &[u8], pos: usize, num_entries: usize, tags: &mut Vec<Tag>) {
    let dir_start = pos;

    for index in 0..num_entries {
        let entry = dir_start + 36 + 28 * index;
        if entry + 28 > data.len() { break; }

        let tag = read_u32_le(data, entry);
        let fmt = read_u32_le(data, entry + 4);
        let flg0 = read_u32_le(data, entry + 8);
        let flg1 = read_u32_le(data, entry + 12);
        let _flg2 = read_u32_le(data, entry + 16);
        let off = read_u32_le(data, entry + 20) as usize + dir_start;
        let len = read_u32_le(data, entry + 24) as usize;

        if off + len > data.len() { continue; }

        // Process the main tag value
        process_dr4_tag(data, tag, fmt, flg0, flg1, off, len, entry, tags);
    }
}

fn process_dr4_tag(
    data: &[u8],
    tag: u32,
    fmt: u32,
    flg0: u32,
    flg1: u32,
    off: usize,
    len: usize,
    entry: usize,
    tags: &mut Vec<Tag>,
) {
    // Handle subdirectory tags
    match tag {
        0x20400 => {
            // ToneCurve subdirectory
            parse_dr4_tone_curve(data, off, len, tags);
            // Also emit ToneCurveOriginal flag (.1)
            let print = no_yes(flg1);
            tags.push(mktag("CanonDR4", "ToneCurveOriginal", Value::U32(flg1), print));
            return;
        }
        0x20a00 => {
            // GammaInfo subdirectory
            parse_dr4_gamma_info(data, off, len, tags);
            return;
        }
        0xf0100 => {
            // CropInfo subdirectory
            parse_dr4_crop_info(data, off, len, tags);
            return;
        }
        0xf0510 => {
            // StampInfo subdirectory
            parse_dr4_stamp_info(data, off, len, tags);
            return;
        }
        _ => {}
    }

    // Read value
    let val = read_dr4_value(data, off, len, fmt);

    // Process flag tags first (stored in directory entry at fixed offsets)
    match tag {
        0x20310 => {
            // SharpnessAdjOn (flag 0)
            let flag_val = read_u32_le(data, entry + 8);
            let print = no_yes(flag_val);
            tags.push(mktag("CanonDR4", "SharpnessAdjOn", Value::U32(flag_val), print));
        }
        0x20500 => {
            // AutoLightingOptimizerOn (flag 0)
            let flag_val = read_u32_le(data, entry + 8);
            let print = no_yes(flag_val);
            tags.push(mktag("CanonDR4", "AutoLightingOptimizerOn", Value::U32(flag_val), print));
        }
        0x20670 => {
            // ColorMoireReductionOn (flag 0)
            let flag_val = read_u32_le(data, entry + 8);
            let print = no_yes(flag_val);
            tags.push(mktag("CanonDR4", "ColorMoireReductionOn", Value::U32(flag_val), print));
        }
        0x20702 => {
            // PeripheralIlluminationOn (flag 0)
            let flag_val = read_u32_le(data, entry + 8);
            let print = no_yes(flag_val);
            tags.push(mktag("CanonDR4", "PeripheralIlluminationOn", Value::U32(flag_val), print));
        }
        0x20703 => {
            // ChromaticAberrationOn (flag 0)
            let flag_val = read_u32_le(data, entry + 8);
            let print = no_yes(flag_val);
            tags.push(mktag("CanonDR4", "ChromaticAberrationOn", Value::U32(flag_val), print));
        }
        0x20705 => {
            // DistortionCorrectionOn (flag 0)
            let flag_val = read_u32_le(data, entry + 8);
            let print = no_yes(flag_val);
            tags.push(mktag("CanonDR4", "DistortionCorrectionOn", Value::U32(flag_val), print));
        }
        0x20706 => {
            // DLOOn (flag 0)
            let flag_val = read_u32_le(data, entry + 8);
            let print = no_yes(flag_val);
            tags.push(mktag("CanonDR4", "DLOOn", Value::U32(flag_val), print));
        }
        _ => {}
    }

    // Now process the main tag
    let (name, print) = dr4_tag_name_and_print(data, tag, fmt, &val, off, len);
    if name.is_empty() { return; }

    tags.push(mktag("CanonDR4", name, val, print));
}

/// Extract an integer from a Value (handles both U32 and I32)
fn val_as_i32(val: &Value) -> Option<i32> {
    match val {
        Value::U32(v) => Some(*v as i32),
        Value::I32(v) => Some(*v),
        _ => None,
    }
}

fn val_as_u32(val: &Value) -> Option<u32> {
    match val {
        Value::U32(v) => Some(*v),
        Value::I32(v) => Some(*v as u32),
        _ => None,
    }
}

/// Return tag name and print value for a DR4 tag
fn dr4_tag_name_and_print<'a>(
    data: &[u8],
    tag: u32,
    fmt: u32,
    val: &Value,
    off: usize,
    len: usize,
) -> (&'a str, String) {
    match tag {
        // Header tags (processed separately)
        // 0x10002 => Rotation
        0x10002 => {
            let v = match val_as_u32(val) { Some(x) => x, None => return ("", String::new()) };
            let print = match v { 0 => "0", 1 => "90", 2 => "180", 3 => "270", _ => "" };
            ("Rotation", if print.is_empty() { v.to_string() } else { print.to_string() })
        }
        // 0x10003 => AngleAdj
        0x10003 => {
            let v = match val { Value::F64(x) => *x, Value::I32(x) => *x as f64, Value::U32(x) => *x as f64, _ => return ("", String::new()) };
            ("AngleAdj", format!("{:.2}", v))
        }
        0x10021 => {
            let print = val.to_display_string();
            ("CustomPictureStyle", print)
        }
        0x10100 => {
            let v = match val_as_u32(val) { Some(x) => x, None => return ("", String::new()) };
            let print = match v {
                0 => "Unrated".to_string(),
                1 => "1".to_string(),
                2 => "2".to_string(),
                3 => "3".to_string(),
                4 => "4".to_string(),
                5 => "5".to_string(),
                0xFFFFFFFF => "Rejected".to_string(),
                _ => v.to_string(),
            };
            ("Rating", print)
        }
        0x10101 => {
            let v = match val_as_u32(val) { Some(x) => x, None => return ("", String::new()) };
            let print = match v {
                0 => "Clear".to_string(),
                1..=5 => v.to_string(),
                _ => v.to_string(),
            };
            ("CheckMark", print)
        }
        0x10200 => {
            let v = match val_as_u32(val) { Some(x) => x, None => return ("", String::new()) };
            let print = match v {
                1 => "sRGB",
                2 => "Adobe RGB",
                3 => "Wide Gamut RGB",
                4 => "Apple RGB",
                5 => "ColorMatch RGB",
                _ => "",
            };
            ("WorkColorSpace", if print.is_empty() { v.to_string() } else { print.to_string() })
        }
        0x20001 => {
            let print = val.to_display_string();
            ("RawBrightnessAdj", print)
        }
        0x20101 => {
            let v = match val_as_i32(val) { Some(x) => x, None => return ("", String::new()) };
            let print = match v {
                -1 => "Manual (Click)",
                0 => "Auto",
                1 => "Daylight",
                2 => "Cloudy",
                3 => "Tungsten",
                4 => "Fluorescent",
                5 => "Flash",
                8 => "Shade",
                9 => "Kelvin",
                255 => "Shot Settings",
                _ => "",
            };
            ("WhiteBalanceAdj", if print.is_empty() { v.to_string() } else { print.to_string() })
        }
        0x20102 => ("WBAdjColorTemp", val.to_display_string()),
        0x20105 => ("WBAdjMagentaGreen", val.to_display_string()),
        0x20106 => ("WBAdjBlueAmber", val.to_display_string()),
        0x20125 => {
            // WBAdjRGGBLevels: remove first integer
            let print = match val {
                Value::List(vals) => {
                    let s: Vec<String> = vals.iter().map(|v| v.to_display_string()).collect();
                    // Remove first element per Perl: '$val =~ s/^\d+ //; $val'
                    if s.len() > 1 { s[1..].join(" ") } else { s.join(" ") }
                }
                _ => val.to_display_string(),
            };
            ("WBAdjRGGBLevels", print)
        }
        0x20200 => {
            let v = match val_as_u32(val) { Some(x) => x, None => return ("", String::new()) };
            ("GammaLinear", no_yes(v))
        }
        0x20301 => {
            let v = match val_as_u32(val) { Some(x) => x, None => return ("", String::new()) };
            let print = match v {
                0x81 => "Standard",
                0x82 => "Portrait",
                0x83 => "Landscape",
                0x84 => "Neutral",
                0x85 => "Faithful",
                0x86 => "Monochrome",
                0x87 => "Auto",
                0x88 => "Fine Detail",
                0xf0 => "Shot Settings",
                0xff => "Custom",
                _ => "",
            };
            ("PictureStyle", if print.is_empty() { format!("0x{:x}", v) } else { print.to_string() })
        }
        0x20303 => ("ContrastAdj", val.to_display_string()),
        0x20304 => {
            // ColorToneAdj: double, format with 1 decimal if needed
            let print = match val {
                Value::F64(v) => {
                    if *v == v.floor() {
                        format!("{}", *v as i64)
                    } else {
                        // Use %g style
                        fmt_g(*v)
                    }
                }
                _ => val.to_display_string(),
            };
            ("ColorToneAdj", print)
        }
        0x20305 => {
            let print = match val {
                Value::F64(v) => fmt_g(*v),
                _ => val.to_display_string(),
            };
            ("ColorSaturationAdj", print)
        }
        0x20306 => {
            let v = match val_as_u32(val) { Some(x) => x, None => return ("", String::new()) };
            let print = match v {
                0 => "None",
                1 => "Sepia",
                2 => "Blue",
                3 => "Purple",
                4 => "Green",
                _ => "",
            };
            ("MonochromeToningEffect", if print.is_empty() { v.to_string() } else { print.to_string() })
        }
        0x20307 => {
            let v = match val_as_u32(val) { Some(x) => x, None => return ("", String::new()) };
            let print = match v {
                0 => "None",
                1 => "Yellow",
                2 => "Orange",
                3 => "Red",
                4 => "Green",
                _ => "",
            };
            ("MonochromeFilterEffect", if print.is_empty() { v.to_string() } else { print.to_string() })
        }
        0x20308 => ("UnsharpMaskStrength", val.to_display_string()),
        0x20309 => ("UnsharpMaskFineness", val.to_display_string()),
        0x2030a => {
            let print = match val {
                Value::F64(v) => fmt_g(*v),
                _ => val.to_display_string(),
            };
            ("UnsharpMaskThreshold", print)
        }
        0x2030b => {
            let print = match val {
                Value::F64(v) => fmt_g(*v),
                _ => val.to_display_string(),
            };
            ("ShadowAdj", print)
        }
        0x2030c => {
            let print = match val {
                Value::F64(v) => fmt_g(*v),
                _ => val.to_display_string(),
            };
            ("HighlightAdj", print)
        }
        0x20310 => {
            let v = match val_as_u32(val) { Some(x) => x, None => return ("", String::new()) };
            let print = match v {
                0 => "Sharpness",
                1 => "Unsharp Mask",
                _ => "",
            };
            ("SharpnessAdj", if print.is_empty() { v.to_string() } else { print.to_string() })
        }
        0x20311 => ("SharpnessStrength", val.to_display_string()),
        // 0x20400: ToneCurve - handled as subdirectory above
        0x20410 => ("ToneCurveBrightness", val.to_display_string()),
        0x20411 => ("ToneCurveContrast", val.to_display_string()),
        0x20500 => {
            let v = match val_as_u32(val) { Some(x) => x, None => return ("", String::new()) };
            let print = match v {
                0 => "Low",
                1 => "Standard",
                2 => "Strong",
                _ => "",
            };
            ("AutoLightingOptimizer", if print.is_empty() { v.to_string() } else { print.to_string() })
        }
        0x20600 => {
            let print = match val {
                Value::F64(v) => fmt_g(*v),
                _ => val.to_display_string(),
            };
            ("LuminanceNoiseReduction", print)
        }
        0x20601 => {
            let print = match val {
                Value::F64(v) => fmt_g(*v),
                _ => val.to_display_string(),
            };
            ("ChrominanceNoiseReduction", print)
        }
        0x20670 => {
            let print = match val {
                Value::F64(v) => fmt_g(*v),
                _ => val.to_display_string(),
            };
            ("ColorMoireReduction", print)
        }
        0x20701 => {
            // ShootingDistance: val/10, formatted as "%"
            let print = match val {
                Value::F64(v) => format!("{:.0}%", v * 100.0),
                Value::I32(v) => {
                    let fv = *v as f64 / 10.0;
                    format!("{:.0}%", fv * 100.0)
                }
                Value::U32(v) => {
                    let fv = *v as f64 / 10.0;
                    format!("{:.0}%", fv * 100.0)
                }
                _ => val.to_display_string(),
            };
            ("ShootingDistance", print)
        }
        0x20702 => {
            let print = match val {
                Value::F64(v) => fmt_g(*v),
                _ => val.to_display_string(),
            };
            ("PeripheralIllumination", print)
        }
        0x20703 => {
            let print = match val {
                Value::F64(v) => fmt_g(*v),
                _ => val.to_display_string(),
            };
            ("ChromaticAberration", print)
        }
        0x20704 => {
            let v = match val_as_u32(val) { Some(x) => x, None => return ("", String::new()) };
            ("ColorBlurOn", no_yes(v))
        }
        0x20705 => {
            let print = match val {
                Value::F64(v) => fmt_g(*v),
                _ => val.to_display_string(),
            };
            ("DistortionCorrection", print)
        }
        0x20706 => ("DLOSetting", val.to_display_string()),
        0x20707 => {
            let print = match val {
                Value::F64(v) => fmt_g(*v),
                _ => val.to_display_string(),
            };
            ("ChromaticAberrationRed", print)
        }
        0x20708 => {
            let print = match val {
                Value::F64(v) => fmt_g(*v),
                _ => val.to_display_string(),
            };
            ("ChromaticAberrationBlue", print)
        }
        0x20900 => ("ColorHue", val.to_display_string()),
        0x20901 => ("SaturationAdj", val.to_display_string()),
        0x20910 => {
            ("RedHSL", format_hsl(data, off, len))
        }
        0x20911 => {
            ("OrangeHSL", format_hsl(data, off, len))
        }
        0x20912 => {
            ("YellowHSL", format_hsl(data, off, len))
        }
        0x20913 => {
            ("GreenHSL", format_hsl(data, off, len))
        }
        0x20914 => {
            ("AquaHSL", format_hsl(data, off, len))
        }
        0x20915 => {
            ("BlueHSL", format_hsl(data, off, len))
        }
        0x20916 => {
            ("PurpleHSL", format_hsl(data, off, len))
        }
        0x20917 => {
            ("MagentaHSL", format_hsl(data, off, len))
        }
        0x30101 => {
            let v = match val_as_u32(val) { Some(x) => x, None => return ("", String::new()) };
            let print = match v {
                0 => "Free",
                1 => "Custom",
                2 => "1:1",
                3 => "3:2",
                4 => "2:3",
                5 => "4:3",
                6 => "3:4",
                7 => "5:4",
                8 => "4:5",
                9 => "16:9",
                10 => "9:16",
                _ => "",
            };
            ("CropAspectRatio", if print.is_empty() { v.to_string() } else { print.to_string() })
        }
        0x30102 => {
            // CropAspectRatioCustom: 2 int32u values
            let print = match val {
                Value::List(vals) => vals.iter().map(|v| v.to_display_string()).collect::<Vec<_>>().join(" "),
                _ => val.to_display_string(),
            };
            ("CropAspectRatioCustom", print)
        }
        0xf0512 => ("LensFocalLength", val.to_display_string()),
        _ => ("", String::new()),
    }
}

/// Read DR4 file
pub fn read_dr4(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 32 {
        return Err(Error::InvalidData("DR4 file too small".into()));
    }
    // Check magic: "IIII" + 04/05 00 04 00
    if &data[0..4] != b"IIII"
        || !((data[4] == 0x04 || data[4] == 0x05) && data[5] == 0x00 && data[6] == 0x04 && data[7] == 0x00)
    {
        return Err(Error::InvalidData("not a DR4 file".into()));
    }

    let mut tags = Vec::new();

    // DR4Header: int32u[8] at offset 0
    // [3]: DR4CameraModel
    // [7]: number of entries
    let camera_model_id = read_u32_le(data, 12); // index 3
    let model_name = canon_model_id(camera_model_id);
    let print = if model_name.is_empty() {
        format!("0x{:x}", camera_model_id)
    } else {
        model_name.to_string()
    };
    tags.push(mktag("CanonDR4", "DR4CameraModel", Value::U32(camera_model_id), print));

    let num_entries = read_u32_le(data, 28) as usize; // index 7

    if data.len() < 36 + 28 * num_entries {
        return Err(Error::InvalidData("DR4 directory truncated".into()));
    }

    // Process all directory entries
    process_dr4_entries(data, 0, num_entries, &mut tags);

    Ok(tags)
}

// ============================================================================
// VRD format reader (Ver1 + Ver2/3)
// ============================================================================

/// Read VRD file: "CANON OPTIONAL DATA\0" header + blocks + footer
pub fn read_vrd(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 0x1c + 0x40 {
        return Err(Error::InvalidData("VRD file too small".into()));
    }
    if &data[0..20] != b"CANON OPTIONAL DATA\0" {
        return Err(Error::InvalidData("not a VRD file".into()));
    }

    // VRD outer structure is big-endian
    // Header is 0x1c bytes, footer is 0x40 bytes
    // Blocks between header and footer: each block = type(4BE) + len(4BE) + data
    let dir_len = data.len(); // total file size
    let footer_start = dir_len - 0x40;

    let mut tags = Vec::new();
    let mut pos = 0x1c; // start after header

    while pos + 8 <= footer_start {
        if pos + 8 > footer_start { break; }
        let block_type = read_u32_be(data, pos);
        let block_len = read_u32_be(data, pos + 4) as usize;
        pos += 8;

        if pos + block_len > footer_start {
            break;
        }

        match block_type {
            0xffff00f4 => {
                // EditData: VRD version 1/2/3 edit information
                parse_vrd_edit_data(&data[pos..pos + block_len], &mut tags);
            }
            0xffff00f7 => {
                // Edit4Data: DR4-style edit information embedded in VRD
                // Inner data uses ProcessEditData format: 4-byte length + DR4 data
                if block_len >= 8 {
                    let inner_len = read_u32_be(data, pos) as usize;
                    let inner_start = pos + 4;
                    if inner_start + inner_len <= pos + block_len {
                        // The DR4 data inside VRD
                        if inner_len >= 32 {
                            process_dr4_entries(&data[inner_start..inner_start + inner_len], 0,
                                read_u32_le(&data[inner_start..inner_start + inner_len], 28) as usize,
                                &mut tags);
                        }
                    }
                }
            }
            _ => {}
        }

        pos += block_len;
    }

    Ok(tags)
}

/// Parse VRD edit data sections (within the 0xffff00f4 block)
fn parse_vrd_edit_data(edit_data: &[u8], tags: &mut Vec<Tag>) {
    // ProcessEditData: reads one record (prefixed by 4-byte length, big-endian)
    // Then inside the record, there are 3 sections:
    //   0: VRD1 (fixed 0x272 bytes)
    //   1: VRDStampTool (variable length: 4-byte count prefix)
    //   2: VRD2 (rest)

    if edit_data.len() < 4 { return; }
    let rec_len = read_u32_be(edit_data, 0) as usize;
    if rec_len + 4 > edit_data.len() { return; }
    let rec = &edit_data[4..4 + rec_len];

    // Section 0: VRD1 (0x272 bytes)
    let vrd1_size = 0x272usize;
    if rec.len() >= vrd1_size {
        parse_vrd_ver1(&rec[..vrd1_size], tags);
    }

    // Section 1: VRDStampTool (4-byte length prefix)
    let mut sub_pos = vrd1_size;
    if sub_pos + 4 > rec.len() { return; }
    let stamp_len = read_u32_be(rec, sub_pos) as usize;
    sub_pos += 4;
    if sub_pos + stamp_len > rec.len() { return; }
    if stamp_len >= 4 {
        let count = read_u32_le(&rec[sub_pos..], 0);
        tags.push(mktag("CanonVRD", "StampToolCount", Value::U32(count), count.to_string()));
    }
    sub_pos += stamp_len;

    // Section 2: VRD2 (rest of rec)
    if sub_pos < rec.len() {
        parse_vrd_ver2(&rec[sub_pos..], tags);
    }
}

/// Parse VRD Version 1 binary data (0x272 bytes, little-endian)
fn parse_vrd_ver1(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 0x272 { return; }

    // VRD inner data is big-endian (SetByteOrder('MM') in Perl)

    // 0x002: VRDVersion (int16u)
    let vrd_ver = read_u16_be(data, 0x002);
    // Format: 3 digits → X.Y.Z
    let v_str = format!("{}.{}.{}", vrd_ver / 100, (vrd_ver / 10) % 10, vrd_ver % 10);
    tags.push(mktag("CanonVRD", "VRDVersion", Value::U16(vrd_ver), v_str));

    // 0x006: WBAdjRGGBLevels (int16u[4])
    {
        let a = read_u16_be(data, 0x006);
        let b = read_u16_be(data, 0x008);
        let c = read_u16_be(data, 0x00a);
        let d = read_u16_be(data, 0x00c);
        let print = format!("{} {} {} {}", a, b, c, d);
        let raw = Value::List(vec![Value::U16(a), Value::U16(b), Value::U16(c), Value::U16(d)]);
        tags.push(mktag("CanonVRD", "WBAdjRGGBLevels", raw, print));
    }

    // 0x018: WhiteBalanceAdj (int16u)
    {
        let v = read_u16_be(data, 0x018);
        let print = match v {
            0 => "Auto",
            1 => "Daylight",
            2 => "Cloudy",
            3 => "Tungsten",
            4 => "Fluorescent",
            5 => "Flash",
            8 => "Shade",
            9 => "Kelvin",
            30 => "Manual (Click)",
            31 => "Shot Settings",
            _ => "",
        };
        tags.push(mktag("CanonVRD", "WhiteBalanceAdj", Value::U16(v),
            if print.is_empty() { v.to_string() } else { print.to_string() }));
    }

    // 0x01a: WBAdjColorTemp (int16u)
    {
        let v = read_u16_be(data, 0x01a);
        tags.push(mktag("CanonVRD", "WBAdjColorTemp", Value::U16(v), v.to_string()));
    }

    // 0x024: WBFineTuneActive (int16u)
    {
        let v = read_u16_be(data, 0x024);
        tags.push(mktag("CanonVRD", "WBFineTuneActive", Value::U16(v), no_yes(v as u32)));
    }

    // 0x028: WBFineTuneSaturation (int16u)
    {
        let v = read_u16_be(data, 0x028);
        tags.push(mktag("CanonVRD", "WBFineTuneSaturation", Value::U16(v), v.to_string()));
    }

    // 0x02c: WBFineTuneTone (int16u)
    {
        let v = read_u16_be(data, 0x02c);
        tags.push(mktag("CanonVRD", "WBFineTuneTone", Value::U16(v), v.to_string()));
    }

    // 0x02e: RawColorAdj (int16u)
    {
        let v = read_u16_be(data, 0x02e);
        let print = match v { 0 => "Shot Settings", 1 => "Faithful", 2 => "Custom", _ => "" };
        tags.push(mktag("CanonVRD", "RawColorAdj", Value::U16(v),
            if print.is_empty() { v.to_string() } else { print.to_string() }));
    }

    // 0x030: RawCustomSaturation (int32s)
    {
        let v = read_i32_be(data, 0x030);
        tags.push(mktag("CanonVRD", "RawCustomSaturation", Value::I32(v), v.to_string()));
    }

    // 0x034: RawCustomTone (int32s)
    {
        let v = read_i32_be(data, 0x034);
        tags.push(mktag("CanonVRD", "RawCustomTone", Value::I32(v), v.to_string()));
    }

    // 0x038: RawBrightnessAdj (int32s / 6000)
    {
        let v = read_i32_be(data, 0x038);
        let fv = v as f64 / 6000.0;
        let print = format!("{:.2}", fv);
        tags.push(mktag("CanonVRD", "RawBrightnessAdj", Value::I32(v), print));
    }

    // 0x03c: ToneCurveProperty (int16u)
    {
        let v = read_u16_be(data, 0x03c);
        let print = match v {
            0 => "Shot Settings",
            1 => "Linear",
            2 => "Custom 1",
            3 => "Custom 2",
            4 => "Custom 3",
            5 => "Custom 4",
            6 => "Custom 5",
            _ => "",
        };
        tags.push(mktag("CanonVRD", "ToneCurveProperty", Value::U16(v),
            if print.is_empty() { v.to_string() } else { print.to_string() }));
    }

    // 0x07a: DynamicRangeMin (int16u)
    {
        let v = read_u16_be(data, 0x07a);
        tags.push(mktag("CanonVRD", "DynamicRangeMin", Value::U16(v), v.to_string()));
    }

    // 0x07c: DynamicRangeMax (int16u)
    {
        let v = read_u16_be(data, 0x07c);
        tags.push(mktag("CanonVRD", "DynamicRangeMax", Value::U16(v), v.to_string()));
    }

    // 0x110: ToneCurveActive (int16u)
    {
        let v = read_u16_be(data, 0x110);
        tags.push(mktag("CanonVRD", "ToneCurveActive", Value::U16(v), no_yes(v as u32)));
    }

    // 0x113: ToneCurveMode (int8u at 0x113)
    {
        let v = read_u8(data, 0x113) as u16;
        let print = match v { 0 => "RGB", 1 => "Luminance", _ => "" };
        tags.push(mktag("CanonVRD", "ToneCurveMode", Value::U16(v),
            if print.is_empty() { v.to_string() } else { print.to_string() }));
    }

    // 0x114: BrightnessAdj (int8s)
    {
        let v = read_i8(data, 0x114) as i16;
        tags.push(mktag("CanonVRD", "BrightnessAdj", Value::I16(v), v.to_string()));
    }

    // 0x115: ContrastAdj (int8s)
    {
        let v = read_i8(data, 0x115) as i16;
        tags.push(mktag("CanonVRD", "ContrastAdj", Value::I16(v), v.to_string()));
    }

    // 0x116: SaturationAdj (int16s)
    {
        let v = read_i16_be(data, 0x116);
        tags.push(mktag("CanonVRD", "SaturationAdj", Value::I16(v), v.to_string()));
    }

    // 0x11e: ColorToneAdj (int32s)
    {
        let v = read_i32_be(data, 0x11e);
        tags.push(mktag("CanonVRD", "ColorToneAdj", Value::I32(v), v.to_string()));
    }

    // Tone curve data (int16u[21] each)
    // 0x126: LuminanceCurvePoints
    {
        let vals = read_u16_array(data, 0x126, 21);
        let print = tone_curve_print_u16(&vals);
        let raw = Value::List(vals.iter().map(|&v| Value::U16(v)).collect());
        tags.push(mktag("CanonVRD", "LuminanceCurvePoints", raw, print));
    }

    // 0x150: LuminanceCurveLimits (int16u[4])
    {
        let vals = read_u16_array(data, 0x150, 4);
        let print = format!("{} {} {} {}", vals[0], vals[1], vals[2], vals[3]);
        let raw = Value::List(vals.iter().map(|&v| Value::U16(v)).collect());
        tags.push(mktag("CanonVRD", "LuminanceCurveLimits", raw, print));
    }

    // 0x159: ToneCurveInterpolation (int8u)
    {
        let v = read_u8(data, 0x159) as u16;
        let print = match v { 0 => "Curve", 1 => "Straight", _ => "" };
        tags.push(mktag("CanonVRD", "ToneCurveInterpolation", Value::U16(v),
            if print.is_empty() { v.to_string() } else { print.to_string() }));
    }

    // 0x160: RedCurvePoints
    {
        let vals = read_u16_array(data, 0x160, 21);
        let print = tone_curve_print_u16(&vals);
        let raw = Value::List(vals.iter().map(|&v| Value::U16(v)).collect());
        tags.push(mktag("CanonVRD", "RedCurvePoints", raw, print));
    }

    // 0x18a: RedCurveLimits (int16u[4])
    {
        let vals = read_u16_array(data, 0x18a, 4);
        let print = format!("{} {} {} {}", vals[0], vals[1], vals[2], vals[3]);
        let raw = Value::List(vals.iter().map(|&v| Value::U16(v)).collect());
        tags.push(mktag("CanonVRD", "RedCurveLimits", raw, print));
    }

    // 0x19a: GreenCurvePoints
    {
        let vals = read_u16_array(data, 0x19a, 21);
        let print = tone_curve_print_u16(&vals);
        let raw = Value::List(vals.iter().map(|&v| Value::U16(v)).collect());
        tags.push(mktag("CanonVRD", "GreenCurvePoints", raw, print));
    }

    // 0x1c4: GreenCurveLimits (int16u[4])
    {
        let vals = read_u16_array(data, 0x1c4, 4);
        let print = format!("{} {} {} {}", vals[0], vals[1], vals[2], vals[3]);
        let raw = Value::List(vals.iter().map(|&v| Value::U16(v)).collect());
        tags.push(mktag("CanonVRD", "GreenCurveLimits", raw, print));
    }

    // 0x1d4: BlueCurvePoints
    {
        let vals = read_u16_array(data, 0x1d4, 21);
        let print = tone_curve_print_u16(&vals);
        let raw = Value::List(vals.iter().map(|&v| Value::U16(v)).collect());
        tags.push(mktag("CanonVRD", "BlueCurvePoints", raw, print));
    }

    // 0x1fe: BlueCurveLimits (int16u[4])
    {
        let vals = read_u16_array(data, 0x1fe, 4);
        let print = format!("{} {} {} {}", vals[0], vals[1], vals[2], vals[3]);
        let raw = Value::List(vals.iter().map(|&v| Value::U16(v)).collect());
        tags.push(mktag("CanonVRD", "BlueCurveLimits", raw, print));
    }

    // 0x20e: RGBCurvePoints
    {
        let vals = read_u16_array(data, 0x20e, 21);
        let print = tone_curve_print_u16(&vals);
        let raw = Value::List(vals.iter().map(|&v| Value::U16(v)).collect());
        tags.push(mktag("CanonVRD", "RGBCurvePoints", raw, print));
    }

    // 0x238: RGBCurveLimits (int16u[4])
    {
        let vals = read_u16_array(data, 0x238, 4);
        let print = format!("{} {} {} {}", vals[0], vals[1], vals[2], vals[3]);
        let raw = Value::List(vals.iter().map(|&v| Value::U16(v)).collect());
        tags.push(mktag("CanonVRD", "RGBCurveLimits", raw, print));
    }

    // 0x244: CropActive (int16u)
    {
        let v = read_u16_be(data, 0x244);
        tags.push(mktag("CanonVRD", "CropActive", Value::U16(v), no_yes(v as u32)));
    }

    // 0x246: CropLeft (int16u)
    {
        let v = read_u16_be(data, 0x246);
        tags.push(mktag("CanonVRD", "CropLeft", Value::U16(v), v.to_string()));
    }

    // 0x248: CropTop (int16u)
    {
        let v = read_u16_be(data, 0x248);
        tags.push(mktag("CanonVRD", "CropTop", Value::U16(v), v.to_string()));
    }

    // 0x24a: CropWidth (int16u)
    {
        let v = read_u16_be(data, 0x24a);
        tags.push(mktag("CanonVRD", "CropWidth", Value::U16(v), v.to_string()));
    }

    // 0x24c: CropHeight (int16u)
    {
        let v = read_u16_be(data, 0x24c);
        tags.push(mktag("CanonVRD", "CropHeight", Value::U16(v), v.to_string()));
    }

    // 0x25a: SharpnessAdj (int16u)
    {
        let v = read_u16_be(data, 0x25a);
        tags.push(mktag("CanonVRD", "SharpnessAdj", Value::U16(v), v.to_string()));
    }

    // 0x260: CropAspectRatio (int16u)
    {
        let v = read_u16_be(data, 0x260);
        let print = match v {
            0 => "Free",
            1 => "3:2",
            2 => "2:3",
            3 => "4:3",
            4 => "3:4",
            5 => "A-size Landscape",
            6 => "A-size Portrait",
            7 => "Letter-size Landscape",
            8 => "Letter-size Portrait",
            9 => "4:5",
            10 => "5:4",
            11 => "1:1",
            12 => "Circle",
            65535 => "Custom",
            _ => "",
        };
        tags.push(mktag("CanonVRD", "CropAspectRatio", Value::U16(v),
            if print.is_empty() { v.to_string() } else { print.to_string() }));
    }

    // 0x262: ConstrainedCropWidth (float)
    {
        let v = read_f32_be(data, 0x262);
        let print = format!("{:.7}", v).trim_end_matches('0').trim_end_matches('.').to_string();
        tags.push(mktag("CanonVRD", "ConstrainedCropWidth", Value::F32(v), print));
    }

    // 0x266: ConstrainedCropHeight (float)
    {
        let v = read_f32_be(data, 0x266);
        let print = format!("{:.7}", v).trim_end_matches('0').trim_end_matches('.').to_string();
        tags.push(mktag("CanonVRD", "ConstrainedCropHeight", Value::F32(v), print));
    }

    // 0x26a: CheckMark (int16u)
    {
        let v = read_u16_be(data, 0x26a);
        let print = match v { 0 => "Clear".to_string(), 1..=3 => v.to_string(), _ => v.to_string() };
        tags.push(mktag("CanonVRD", "CheckMark", Value::U16(v), print));
    }

    // 0x26e: Rotation (int16u)
    {
        let v = read_u16_be(data, 0x26e);
        let print = match v { 0 => "0", 1 => "90", 2 => "180", 3 => "270", _ => "" };
        tags.push(mktag("CanonVRD", "Rotation", Value::U16(v),
            if print.is_empty() { v.to_string() } else { print.to_string() }));
    }

    // 0x270: WorkColorSpace (int16u)
    {
        let v = read_u16_be(data, 0x270);
        let print = match v {
            0 => "sRGB",
            1 => "Adobe RGB",
            2 => "Wide Gamut RGB",
            3 => "Apple RGB",
            4 => "ColorMatch RGB",
            _ => "",
        };
        tags.push(mktag("CanonVRD", "WorkColorSpace", Value::U16(v),
            if print.is_empty() { v.to_string() } else { print.to_string() }));
    }
}

fn read_u16_array(data: &[u8], off: usize, count: usize) -> Vec<u16> {
    let end = off + count * 2;
    if end > data.len() { return vec![0u16; count]; }
    (0..count).map(|i| read_u16_be(data, off + i * 2)).collect()
}

fn tone_curve_print_u16(vals: &[u16]) -> String {
    if vals.len() != 21 { return vals.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" "); }
    let n = vals[0] as usize;
    if n < 2 || n > 10 { return vals.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" "); }
    let mut result = String::new();
    for i in 0..n {
        if i > 0 { result.push(' '); }
        result.push('(');
        result.push_str(&vals[1 + i * 2].to_string());
        result.push(',');
        result.push_str(&vals[2 + i * 2].to_string());
        result.push(')');
    }
    result
}

/// Parse VRD Version 2/3 binary data (int16s format, little-endian)
fn parse_vrd_ver2(data: &[u8], tags: &mut Vec<Tag>) {
    // FORMAT => 'int16s' means each tag is at offset * 2 bytes, and read as i16
    // (unless explicitly overridden)
    if data.len() < 4 { return; }

    let read_i16_at = |idx: usize| -> Option<i16> {
        let byte_off = idx * 2;
        if byte_off + 2 <= data.len() { Some(read_i16_be(data, byte_off)) } else { None }
    };

    // 0x02: PictureStyle
    if let Some(v) = read_i16_at(0x02) {
        let print = match v {
            0 => "Standard",
            1 => "Portrait",
            2 => "Landscape",
            3 => "Neutral",
            4 => "Faithful",
            5 => "Monochrome",
            6 => "Unknown?",
            7 => "Custom",
            _ => "",
        };
        tags.push(mktag("CanonVRD", "PictureStyle", Value::I16(v),
            if print.is_empty() { v.to_string() } else { print.to_string() }));
    }

    // 0x03: IsCustomPictureStyle
    if let Some(v) = read_i16_at(0x03) {
        tags.push(mktag("CanonVRD", "IsCustomPictureStyle", Value::I16(v), no_yes(v as u32)));
    }

    // Standard picture style params
    if let Some(v) = read_i16_at(0x0d) { tags.push(mktag("CanonVRD", "StandardRawColorTone", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x0e) { tags.push(mktag("CanonVRD", "StandardRawSaturation", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x0f) { tags.push(mktag("CanonVRD", "StandardRawContrast", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x10) {
        tags.push(mktag("CanonVRD", "StandardRawLinear", Value::I16(v), no_yes(v as u32)));
    }
    if let Some(v) = read_i16_at(0x11) { tags.push(mktag("CanonVRD", "StandardRawSharpness", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x12) { tags.push(mktag("CanonVRD", "StandardRawHighlightPoint", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x13) { tags.push(mktag("CanonVRD", "StandardRawShadowPoint", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x14) { tags.push(mktag("CanonVRD", "StandardOutputHighlightPoint", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x15) { tags.push(mktag("CanonVRD", "StandardOutputShadowPoint", Value::I16(v), v.to_string())); }

    // Portrait picture style params
    if let Some(v) = read_i16_at(0x16) { tags.push(mktag("CanonVRD", "PortraitRawColorTone", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x17) { tags.push(mktag("CanonVRD", "PortraitRawSaturation", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x18) { tags.push(mktag("CanonVRD", "PortraitRawContrast", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x19) {
        tags.push(mktag("CanonVRD", "PortraitRawLinear", Value::I16(v), no_yes(v as u32)));
    }
    if let Some(v) = read_i16_at(0x1a) { tags.push(mktag("CanonVRD", "PortraitRawSharpness", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x1b) { tags.push(mktag("CanonVRD", "PortraitRawHighlightPoint", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x1c) { tags.push(mktag("CanonVRD", "PortraitRawShadowPoint", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x1d) { tags.push(mktag("CanonVRD", "PortraitOutputHighlightPoint", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x1e) { tags.push(mktag("CanonVRD", "PortraitOutputShadowPoint", Value::I16(v), v.to_string())); }

    // Landscape
    if let Some(v) = read_i16_at(0x1f) { tags.push(mktag("CanonVRD", "LandscapeRawColorTone", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x20) { tags.push(mktag("CanonVRD", "LandscapeRawSaturation", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x21) { tags.push(mktag("CanonVRD", "LandscapeRawContrast", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x22) {
        tags.push(mktag("CanonVRD", "LandscapeRawLinear", Value::I16(v), no_yes(v as u32)));
    }
    if let Some(v) = read_i16_at(0x23) { tags.push(mktag("CanonVRD", "LandscapeRawSharpness", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x24) { tags.push(mktag("CanonVRD", "LandscapeRawHighlightPoint", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x25) { tags.push(mktag("CanonVRD", "LandscapeRawShadowPoint", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x26) { tags.push(mktag("CanonVRD", "LandscapeOutputHighlightPoint", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x27) { tags.push(mktag("CanonVRD", "LandscapeOutputShadowPoint", Value::I16(v), v.to_string())); }

    // Neutral
    if let Some(v) = read_i16_at(0x28) { tags.push(mktag("CanonVRD", "NeutralRawColorTone", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x29) { tags.push(mktag("CanonVRD", "NeutralRawSaturation", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x2a) { tags.push(mktag("CanonVRD", "NeutralRawContrast", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x2b) {
        tags.push(mktag("CanonVRD", "NeutralRawLinear", Value::I16(v), no_yes(v as u32)));
    }
    if let Some(v) = read_i16_at(0x2c) { tags.push(mktag("CanonVRD", "NeutralRawSharpness", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x2d) { tags.push(mktag("CanonVRD", "NeutralRawHighlightPoint", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x2e) { tags.push(mktag("CanonVRD", "NeutralRawShadowPoint", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x2f) { tags.push(mktag("CanonVRD", "NeutralOutputHighlightPoint", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x30) { tags.push(mktag("CanonVRD", "NeutralOutputShadowPoint", Value::I16(v), v.to_string())); }

    // Faithful
    if let Some(v) = read_i16_at(0x31) { tags.push(mktag("CanonVRD", "FaithfulRawColorTone", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x32) { tags.push(mktag("CanonVRD", "FaithfulRawSaturation", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x33) { tags.push(mktag("CanonVRD", "FaithfulRawContrast", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x34) {
        tags.push(mktag("CanonVRD", "FaithfulRawLinear", Value::I16(v), no_yes(v as u32)));
    }
    if let Some(v) = read_i16_at(0x35) { tags.push(mktag("CanonVRD", "FaithfulRawSharpness", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x36) { tags.push(mktag("CanonVRD", "FaithfulRawHighlightPoint", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x37) { tags.push(mktag("CanonVRD", "FaithfulRawShadowPoint", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x38) { tags.push(mktag("CanonVRD", "FaithfulOutputHighlightPoint", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x39) { tags.push(mktag("CanonVRD", "FaithfulOutputShadowPoint", Value::I16(v), v.to_string())); }

    // Monochrome
    if let Some(v) = read_i16_at(0x3a) {
        let print = match v { -2 => "None", -1 => "Yellow", 0 => "Orange", 1 => "Red", 2 => "Green", _ => "" };
        tags.push(mktag("CanonVRD", "MonochromeFilterEffect", Value::I16(v),
            if print.is_empty() { v.to_string() } else { print.to_string() }));
    }
    if let Some(v) = read_i16_at(0x3b) {
        let print = match v { -2 => "None", -1 => "Sepia", 0 => "Blue", 1 => "Purple", 2 => "Green", _ => "" };
        tags.push(mktag("CanonVRD", "MonochromeToningEffect", Value::I16(v),
            if print.is_empty() { v.to_string() } else { print.to_string() }));
    }
    if let Some(v) = read_i16_at(0x3c) { tags.push(mktag("CanonVRD", "MonochromeContrast", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x3d) {
        tags.push(mktag("CanonVRD", "MonochromeLinear", Value::I16(v), no_yes(v as u32)));
    }
    if let Some(v) = read_i16_at(0x3e) { tags.push(mktag("CanonVRD", "MonochromeSharpness", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x3f) { tags.push(mktag("CanonVRD", "MonochromeRawHighlightPoint", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x40) { tags.push(mktag("CanonVRD", "MonochromeRawShadowPoint", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x41) { tags.push(mktag("CanonVRD", "MonochromeOutputHighlightPoint", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x42) { tags.push(mktag("CanonVRD", "MonochromeOutputShadowPoint", Value::I16(v), v.to_string())); }

    // Custom picture style
    if let Some(v) = read_i16_at(0x4c) { tags.push(mktag("CanonVRD", "CustomColorTone", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x4d) { tags.push(mktag("CanonVRD", "CustomSaturation", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x4e) { tags.push(mktag("CanonVRD", "CustomContrast", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x4f) {
        tags.push(mktag("CanonVRD", "CustomLinear", Value::I16(v), no_yes(v as u32)));
    }
    if let Some(v) = read_i16_at(0x50) { tags.push(mktag("CanonVRD", "CustomSharpness", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x51) { tags.push(mktag("CanonVRD", "CustomRawHighlightPoint", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x52) { tags.push(mktag("CanonVRD", "CustomRawShadowPoint", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x53) { tags.push(mktag("CanonVRD", "CustomOutputHighlightPoint", Value::I16(v), v.to_string())); }
    if let Some(v) = read_i16_at(0x54) { tags.push(mktag("CanonVRD", "CustomOutputShadowPoint", Value::I16(v), v.to_string())); }
}
