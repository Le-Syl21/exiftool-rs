//! Canon MakerNotes sub-table decoders (auto-generated).
//! All CameraSettings + ShotInfo + FocalLength fields.

use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

/// Canon EV conversion — mirrors Perl's CanonEv() with 1/3 and 2/3 step handling
fn canon_ev(val: i32) -> f64 {
    let sign: f64 = if val < 0 { -1.0 } else { 1.0 };
    let v = val.unsigned_abs();
    let frac = v & 0x1F;
    let int_part = v - frac;
    let frac_val = match frac {
        0x0C => 32.0 / 3.0,
        0x14 => 64.0 / 3.0,
        _ => frac as f64,
    };
    sign * (int_part as f64 + frac_val) / 0x20 as f64
}

/// Print exposure time like Perl's PrintExposureTime
pub fn print_exposure_time(val: f64) -> String {
    if val <= 0.0 { return "0".to_string(); }
    if val < 0.25 - 0.001 {
        format!("1/{}", (0.5 + 1.0 / val) as u32)
    } else {
        format!("{:.1}", (val * 10.0 + 0.5) as u32 as f64 / 10.0)
    }
}

pub fn decode_camera_settings(values: &[i16]) -> Vec<Tag> {
    let mut tags = Vec::new();
    let get = |idx: usize| -> Option<i16> { values.get(idx).copied() };

    if let Some(v) = get(1) {
        let pv = match v {
            1 => "Macro",
            2 => "Normal",
            _ => "",
        };
        let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
        tags.push(mkt("MacroMode", Value::I16(v), pv));
    }
    if let Some(v) = get(2) {
        tags.push(mkt("SelfTimer", Value::I16(v), v.to_string()));
    }
    if let Some(v) = get(3) {
        tags.push(mkt("Quality", Value::I16(v), v.to_string()));
    }
    if let Some(v) = get(4) {
        let pv = match v {
            -1 => "n/a",
            0 => "Off",
            1 => "Auto",
            2 => "On",
            3 => "Red-eye reduction",
            4 => "Slow-sync",
            5 => "Red-eye reduction (Auto)",
            6 => "Red-eye reduction (On)",
            16 => "External flash",
            _ => "",
        };
        let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
        tags.push(mkt("CanonFlashMode", Value::I16(v), pv));
    }
    if let Some(v) = get(5) {
        let pv = match v {
            0 => "Single",
            1 => "Continuous",
            2 => "Movie",
            3 => "Continuous, Speed Priority",
            4 => "Continuous, Low",
            5 => "Continuous, High",
            6 => "Silent Single",
            8 => "Continuous, High+",
            9 => "Single, Silent",
            10 => "Continuous, Silent",
            _ => "",
        };
        let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
        tags.push(mkt("ContinuousDrive", Value::I16(v), pv));
    }
    if let Some(v) = get(7) {
        let pv = match v {
            0 => "One-shot AF",
            1 => "AI Servo AF",
            2 => "AI Focus AF",
            3 => "Manual Focus (3)",
            4 => "Single",
            5 => "Continuous",
            6 => "Manual Focus (6)",
            16 => "Pan Focus",
            256 => "One-shot AF (Live View)",
            257 => "AI Servo AF (Live View)",
            258 => "AI Focus AF (Live View)",
            512 => "Movie Snap Focus",
            519 => "Movie Servo AF",
            _ => "",
        };
        let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
        tags.push(mkt("FocusMode", Value::I16(v), pv));
    }
    if let Some(v) = get(9) {
        let pv = match v {
            1 => "JPEG",
            2 => "CRW+THM",
            3 => "AVI+THM",
            4 => "TIF",
            5 => "TIF+JPEG",
            6 => "CR2",
            7 => "CR2+JPEG",
            9 => "MOV",
            10 => "MP4",
            11 => "CRM",
            12 => "CR3",
            13 => "CR3+JPEG",
            14 => "HIF",
            15 => "CR3+HIF",
            _ => "",
        };
        let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
        tags.push(mkt("RecordMode", Value::I16(v), pv));
    }
    if let Some(v) = get(10) {
        tags.push(mkt("CanonImageSize", Value::I16(v), v.to_string()));
    }
    if let Some(v) = get(11) {
        tags.push(mkt("EasyMode", Value::I16(v), v.to_string()));
    }
    if let Some(v) = get(12) {
        let pv = match v {
            0 => "None",
            1 => "2x",
            2 => "4x",
            3 => "Other",
            _ => "",
        };
        let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
        tags.push(mkt("DigitalZoom", Value::I16(v), pv));
    }
    if let Some(v) = get(13) {
        tags.push(mkt("Contrast", Value::I16(v), v.to_string()));
    }
    if let Some(v) = get(14) {
        tags.push(mkt("Saturation", Value::I16(v), v.to_string()));
    }
    if let Some(v) = get(15) {
        tags.push(mkt("Sharpness", Value::I16(v), v.to_string()));
    }
    if let Some(v) = get(16) {
        // RawConv => '$val == 0x7fff ? undef : $val' (suppress 32767)
        if v != 0x7fff_u16 as i16 {
            // ValueConv: CameraISO lookup
            let pv = match v {
                0 => "n/a".to_string(), 14 => "Auto High".to_string(), 15 => "Auto".to_string(),
                16 => "50".to_string(), 17 => "100".to_string(), 18 => "200".to_string(),
                19 => "400".to_string(), 20 => "800".to_string(),
                _ => v.to_string(),
            };
            tags.push(mkt("CameraISO", Value::I16(v), pv));
        }
    }
    if let Some(v) = get(17) {
        let pv = match v {
            0 => "Default",
            1 => "Spot",
            2 => "Average",
            3 => "Evaluative",
            4 => "Partial",
            5 => "Center-weighted average",
            _ => "",
        };
        let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
        tags.push(mkt("MeteringMode", Value::I16(v), pv));
    }
    if let Some(v) = get(18) {
        let pv = match v {
            0 => "Manual",
            1 => "Auto",
            2 => "Not Known",
            3 => "Macro",
            4 => "Very Close",
            5 => "Close",
            6 => "Middle Range",
            7 => "Far Range",
            8 => "Pan Focus",
            9 => "Super Macro",
            10 => "Infinity",
            _ => "",
        };
        let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
        tags.push(mkt("FocusRange", Value::I16(v), pv));
    }
    if let Some(v) = get(19) {
        if v != 0 {
            let pv = match v {
                8197 => "",
                12288 => "",
                12289 => "",
                12290 => "",
                12291 => "",
                12292 => "",
                16385 => "",
                16390 => "",
                _ => "",
            };
            let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
            tags.push(mkt("AFPoint", Value::I16(v), pv));
        }
    }
    if let Some(v) = get(20) {
        let pv = match v {
            0 => "Easy",
            1 => "Program AE",
            2 => "Shutter speed priority AE",
            3 => "Aperture-priority AE",
            4 => "Manual",
            5 => "Depth-of-field AE",
            6 => "M-Dep",
            7 => "Bulb",
            8 => "Flexible-priority AE",
            _ => "",
        };
        let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
        tags.push(mkt("CanonExposureMode", Value::I16(v), pv));
    }
    if let Some(v) = get(22) {
        // RawConv: suppress if 0. PrintConv: -1 => "n/a" (rest from canonLensTypes table)
        if v != 0 {
            let pv = if v == -1 {
                "n/a".to_string()
            } else {
                canon_lens_type_name(v as u16).map(|s| s.to_string()).unwrap_or_else(|| v.to_string())
            };
            tags.push(mkt("LensType", Value::I16(v), pv));
        }
    }
    if let Some(v) = get(23) {
        tags.push(mkt("MaxFocalLength", Value::I16(v), v.to_string()));
    }
    if let Some(v) = get(24) {
        tags.push(mkt("MinFocalLength", Value::I16(v), v.to_string()));
    }
    if let Some(v) = get(25) {
        tags.push(mkt("FocalUnits", Value::I16(v), v.to_string()));
    }
    if let Some(v) = get(26) {
        tags.push(mkt("MaxAperture", Value::I16(v), v.to_string()));
    }
    if let Some(v) = get(27) {
        tags.push(mkt("MinAperture", Value::I16(v), v.to_string()));
    }
    if let Some(v) = get(28) {
        tags.push(mkt("FlashModel", Value::I16(v), v.to_string()));
    }
    if let Some(v) = get(29) {
        // FlashBits: BITMASK PrintConv
        // 0='(none)', bits: 0=Manual, 1=TTL, 2=A-TTL, 3=E-TTL, 4=FP sync enabled,
        //                   7=2nd-curtain sync used, 11=FP sync used, 13=Built-in, 14=External
        let pv = flash_bits_str(v as u16);
        tags.push(mkt("FlashBits", Value::I16(v), pv));
    }
    if let Some(v) = get(32) {
        if v != -1 {
            let pv = match v {
                0 => "Single",
                1 => "Continuous",
                8 => "Manual",
                _ => "",
            };
            let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
            tags.push(mkt("FocusContinuous", Value::I16(v), pv));
        }
    }
    if let Some(v) = get(33) {
        if v != -1 {
            let pv = match v {
                0 => "Normal AE",
                1 => "Exposure Compensation",
                2 => "AE Lock",
                3 => "AE Lock + Exposure Comp.",
                4 => "No AE",
                _ => "",
            };
            let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
            tags.push(mkt("AESetting", Value::I16(v), pv));
        }
    }
    if let Some(v) = get(34) {
        if v != -1 {
            let pv = match v {
                0 => "Off",
                1 => "On",
                2 => "Shoot Only",
                3 => "Panning",
                4 => "Dynamic",
                256 => "Off (2)",
                257 => "On (2)",
                258 => "Shoot Only (2)",
                259 => "Panning (2)",
                260 => "Dynamic (2)",
                _ => "",
            };
            let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
            tags.push(mkt("ImageStabilization", Value::I16(v), pv));
        }
    }
    if let Some(v) = get(35) {
        if v != 0 { tags.push(mkt("DisplayAperture", Value::I16(v), v.to_string())); }
    }
    if let Some(v) = get(36) {
        tags.push(mkt("ZoomSourceWidth", Value::I16(v), v.to_string()));
    }
    if let Some(v) = get(37) {
        tags.push(mkt("ZoomTargetWidth", Value::I16(v), v.to_string()));
    }
    if let Some(v) = get(39) {
        if v != -1 {
            let pv = match v {
                0 => "Center",
                1 => "AF Point",
                _ => "",
            };
            let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
            tags.push(mkt("SpotMeteringMode", Value::I16(v), pv));
        }
    }
    if let Some(v) = get(40) {
        if v != -1 {
            let pv = match v {
                0 => "Off",
                1 => "Vivid",
                2 => "Neutral",
                3 => "Smooth",
                4 => "Sepia",
                5 => "B&W",
                6 => "Custom",
                100 => "My Color Data",
                _ => "",
            };
            let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
            tags.push(mkt("PhotoEffect", Value::I16(v), pv));
        }
    }
    if let Some(v) = get(41) {
        let pv = match v {
            0 => "n/a",
            1280 => "",
            1282 => "",
            1284 => "",
            32767 => "",
            _ => "",
        };
        let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
        tags.push(mkt("ManualFlashOutput", Value::I16(v), pv));
    }
    if let Some(v) = get(42) {
        tags.push(mkt("ColorTone", Value::I16(v), v.to_string()));
    }
    if let Some(v) = get(46) {
        let pv = match v {
            0 => "n/a",
            1 => "sRAW1 (mRAW)",
            2 => "sRAW2 (sRAW)",
            _ => "",
        };
        let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
        tags.push(mkt("SRAWQuality", Value::I16(v), pv));
    }
    if let Some(v) = get(50) {
        let pv = match v {
            0 => "Disable",
            1 => "Enable",
            _ => "",
        };
        let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
        tags.push(mkt("FocusBracketing", Value::I16(v), pv));
    }
    if let Some(v) = get(51) {
        tags.push(mkt("Clarity", Value::I16(v), v.to_string()));
    }
    tags
}

pub fn decode_shot_info(values: &[i16], model: &str) -> Vec<Tag> {
    let mut tags = Vec::new();
    let get = |idx: usize| -> Option<i16> { values.get(idx).copied() };

    if let Some(v) = get(1) {
        // AutoISO: ValueConv => 'exp($val/32*log(2))*100', PrintConv => 'sprintf("%.0f",$val)'
        let auto_iso = (v as f64 / 32.0 * std::f64::consts::LN_2).exp() * 100.0;
        tags.push(mkt("AutoISO", Value::F64(auto_iso), format!("{:.0}", auto_iso)));
    }
    if let Some(v) = get(2) {
        if v != 0 {
            // BaseISO: ValueConv => 'exp($val/32*log(2))*100/32', PrintConv => 'sprintf("%.0f",$val)'
            let base_iso = (v as f64 / 32.0 * std::f64::consts::LN_2).exp() * 100.0 / 32.0;
            tags.push(mkt("BaseISO", Value::F64(base_iso), format!("{:.0}", base_iso)));
        }
    }
    if let Some(v) = get(3) {
        // MeasuredEV: ValueConv => '$val / 32 + 5', PrintConv => 'sprintf("%.2f",$val)'
        let mev = v as f64 / 32.0 + 5.0;
        tags.push(mkt("MeasuredEV", Value::F64(mev), format!("{:.2}", mev)));
    }
    if let Some(v) = get(4) {
        if v > 0 {
            // TargetAperture: ValueConv => 'exp(CanonEv($val)*log(2)/2)', PrintConv => 'sprintf("%.2g",$val)'
            let av = (canon_ev(v as i32) * std::f64::consts::LN_2 / 2.0).exp();
            tags.push(mkt("TargetAperture", Value::F64(av), format!("{:.2}", av)));
        }
    }
    if let Some(v) = get(5) {
        // TargetExposureTime: ValueConv => 'exp(-CanonEv($val)*log(2))'
        // RawConv: suppress if > -1000 && (val != 0 || model contains EOS/PowerShot)
        // For simplicity: suppress if val <= -1000 or (val == 0 for non-EOS)
        let raw = v as i32;
        let valid = raw > -1000 && (raw != 0 || model.contains("EOS") || model.contains("PowerShot") || model.contains("CRW"));
        if valid {
            let et = (-canon_ev(raw) * std::f64::consts::LN_2).exp();
            let pv = print_exposure_time(et);
            tags.push(mkt("TargetExposureTime", Value::F64(et), pv));
        }
    }
    if let Some(v) = get(6) {
        // ExposureCompensation: ValueConv => 'CanonEv($val)', PrintConv => PrintFraction
        let ev = canon_ev(v as i32);
        let pv = print_fraction(ev);
        tags.push(mkt("ExposureCompensation", Value::F64(ev), pv));
    }
    if let Some(v) = get(7) {
        // WhiteBalance: PrintConv => canonWhiteBalance table
        let pv = canon_white_balance_str(v);
        tags.push(mkt("WhiteBalance", Value::I16(v), pv));
    }
    if let Some(v) = get(8) {
        let pv = match v {
            -1 => "n/a",
            0 => "Off",
            1 => "Night Scene",
            2 => "On",
            3 => "None",
            _ => "",
        };
        let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
        tags.push(mkt("SlowShutter", Value::I16(v), pv));
    }
    if let Some(v) = get(9) {
        tags.push(mkt("SequenceNumber", Value::I16(v), v.to_string()));
    }
    if let Some(v) = get(10) {
        // OpticalZoomCode: PrintConv => '$val == 8 ? "n/a" : $val'
        let pv = if v == 8 { "n/a".to_string() } else { v.to_string() };
        tags.push(mkt("OpticalZoomCode", Value::I16(v), pv));
    }
    if let Some(v) = get(12) {
        if v != 0 { tags.push(mkt("CameraTemperature", Value::I16(v), v.to_string())); }
    }
    if let Some(v) = get(13) {
        // RawConv => '$val==-1 ? undef : $val', ValueConv => '$val / 32'
        if v != -1 {
            let val_f = v as f64 / 32.0;
            tags.push(mkt("FlashGuideNumber", Value::I16(v), format!("{:.2}", val_f)));
        }
    }
    if let Some(v) = get(14) {
        let pv = match v {
            12288 => "",
            12289 => "",
            12290 => "",
            12291 => "",
            12292 => "",
            12293 => "",
            12294 => "",
            12295 => "",
            _ => "",
        };
        let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
        tags.push(mkt("AFPointsInFocus", Value::I16(v), pv));
    }
    if let Some(v) = get(15) {
        // FlashExposureComp: ValueConv => 'CanonEv($val)', PrintConv => PrintFraction
        let ev = canon_ev(v as i32);
        let pv = print_fraction(ev);
        tags.push(mkt("FlashExposureComp", Value::F64(ev), pv));
    }
    if let Some(v) = get(16) {
        let pv = match v {
            -1 => "On",
            0 => "Off",
            1 => "On (shot 1)",
            2 => "On (shot 2)",
            3 => "On (shot 3)",
            _ => "",
        };
        let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
        tags.push(mkt("AutoExposureBracketing", Value::I16(v), pv));
    }
    if let Some(v) = get(17) {
        // AEBBracketValue: ValueConv => 'CanonEv($val)', PrintConv => PrintFraction
        let ev = canon_ev(v as i32);
        let pv = print_fraction(ev);
        tags.push(mkt("AEBBracketValue", Value::F64(ev), pv));
    }
    if let Some(v) = get(18) {
        let pv = match v {
            0 => "n/a",
            1 => "Camera Local Control",
            3 => "Computer Remote Control",
            _ => "",
        };
        let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
        tags.push(mkt("ControlMode", Value::I16(v), pv));
    }
    // Index 22: ExposureTime — Two variants depending on model:
    // 350D/20D: exp(-CanonEv(val)*log(2))*1000/32
    // Others: exp(-CanonEv(val)*log(2))
    // RawConv: ($val or FILE_TYPE eq "CRW") ? $val : undef
    if let Some(v) = get(22) {
        // For CRW files, v=0 is valid (1 sec). For JPEG, suppress 0.
        // We can't check file type here, so emit if non-zero
        if v != 0 {
            let ev = canon_ev(v as i32);
            // Use the generic formula (most models)
            let et = (-ev * std::f64::consts::LN_2).exp();
            let pv = crate::tags::canon_sub::print_exposure_time(et);
            tags.push(Tag {
                id: TagId::Text("ExposureTime".into()),
                name: "ExposureTime".into(),
                description: "Exposure Time".into(),
                group: TagGroup { family0: "MakerNotes".into(), family1: "Canon".into(), family2: "Camera".into() },
                raw_value: Value::F64(et),
                print_value: pv,
                priority: 0,
            });
        }
    }
    // FocusDistanceUpper/Lower: Format=int16u.
    // RawConv: '($$self{FocusDistanceUpper} = $val) || undef' — suppress when 0.
    // ValueConv: $val / 100; PrintConv: "> 655.345 ? 'inf' : '$val m'"
    let focus_upper = get(19).map(|v| v as u16).unwrap_or(0);
    if focus_upper != 0 {
        let m = focus_upper as f64 / 100.0;
        let pv = if m > 655.345 { "inf".to_string() } else { format!("{} m", m) };
        tags.push(mkt("FocusDistanceUpper", Value::U16(focus_upper), pv));
        // FocusDistanceLower: only emit when FocusDistanceUpper is non-zero (Condition)
        if let Some(v) = get(20).map(|v| v as u16) {
            let m = v as f64 / 100.0;
            let pv = if m > 655.345 { "inf".to_string() } else { format!("{} m", m) };
            tags.push(mkt("FocusDistanceLower", Value::U16(v), pv));
        }
    }
    // ShotInfo index 21: FNumber — ValueConv: exp(CanonEv(val)*log(2)/2)
    // Priority=0 in Perl (EXIF takes precedence). Emit anyway — EXIF dedup handles priority.
    if let Some(v) = get(21) {
        if v != 0 {
            let ev = canon_ev(v as i32);
            let fnum = (ev * std::f64::consts::LN_2 / 2.0).exp();
            tags.push(Tag {
                id: TagId::Text("FNumber".into()),
                name: "FNumber".into(),
                description: "F Number".into(),
                group: TagGroup { family0: "MakerNotes".into(), family1: "Canon".into(), family2: "Camera".into() },
                raw_value: Value::F64(fnum),
                print_value: format!("{:.1}", fnum),
                priority: 0,
            });
        }
    }
    if let Some(v) = get(23) {
        // MeasuredEV2: RawConv: suppress if 0; ValueConv => '$val / 8 - 6'
        if v != 0 {
            let mev2 = v as f64 / 8.0 - 6.0;
            tags.push(mkt("MeasuredEV2", Value::F64(mev2), format!("{}", mev2)));
        }
    }
    if let Some(v) = get(24) {
        // BulbDuration: ValueConv => '$val / 10'
        let bd = v as f64 / 10.0;
        tags.push(mkt("BulbDuration", Value::F64(bd), format!("{}", bd)));
    }
    if let Some(v) = get(26) {
        let pv = match v {
            0 => "n/a",
            248 => "EOS High-end",
            250 => "Compact",
            252 => "EOS Mid-range",
            255 => "DV Camera",
            _ => "",
        };
        let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
        tags.push(mkt("CameraType", Value::I16(v), pv));
    }
    if let Some(v) = get(27) {
        // RawConv => '$val >= 0 ? $val : undef' — suppress negative values
        if v >= 0 {
            let pv = match v {
                0 => "None",
                1 => "Rotate 90 CW",
                2 => "Rotate 180",
                3 => "Rotate 270 CW",
                _ => "",
            };
            let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
            tags.push(mkt("AutoRotate", Value::I16(v), pv));
        }
    }
    if let Some(v) = get(28) {
        let pv = match v {
            -1 => "n/a",
            0 => "Off",
            1 => "On",
            _ => "",
        };
        let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
        tags.push(mkt("NDFilter", Value::I16(v), pv));
    }
    if let Some(v) = get(29) {
        // RawConv => '$val >= 0 ? $val : undef' — suppress negative values
        if v >= 0 {
            let val_f = v as f64 / 10.0;
            tags.push(mkt("SelfTimer2", Value::I16(v), format!("{:.1}", val_f)));
        }
    }
    // FlashOutput: RawConv: '($$self{Model}=~/(PowerShot|IXUS|IXY)/ or $val) ? $val : undef'
    // Suppress when 0 for non-PowerShot models
    if let Some(v) = get(33) {
        let is_powershot = model.contains("PowerShot") || model.contains("IXUS")
            || model.contains("IXY");
        if v != 0 || is_powershot {
            // PrintConv for FlashOutput from ColorData3:
            // ValueConv: exp(($val-200)/16*log(2)), PrintConv: sprintf("%.0f%%", $val*100)
            // But here it's the ShotInfo FlashOutput which has a different scale.
            // For ShotInfo index 33: Perl just stores the raw int16s with no ValueConv.
            // So just emit the raw value.
            tags.push(mkt("FlashOutput", Value::I16(v), v.to_string()));
        }
    }
    tags
}

pub fn decode_focal_length(values: &[u16], model: &str) -> Vec<Tag> {
    let mut tags = Vec::new();
    if let Some(&v) = values.get(0) {
        if v != 0 {
            let pv = match v { 1 => "Fixed", 2 => "Zoom", _ => "" };
            let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
            tags.push(mkt("FocalType", Value::U16(v), pv));
        }
    }
    // FocalLength has Priority=0 in Perl (EXIF FocalLength takes precedence)
    // Suppress to avoid duplicates.
    // FocalPlaneXSize/YSize for older Canon models (Perl: Canon::FocalLength table)
    // Only present for some lower-end models, not 1D/5D/7D series
    let model_upper = model.to_uppercase();
    let is_1d_series = model_upper.contains("EOS-1D") || model_upper.contains("EOS 1D")
        || model_upper.contains("EOS 1DS") || model_upper.contains("EOS-1DS");
    let has_focal_plane = !is_1d_series && (model_upper.contains("REBEL") || model_upper.contains("300D")
        || model_upper.contains("350D") || model_upper.contains("400D")
        || model_upper.contains("POWERSHOT")
        || (model_upper.contains("EOS") && !model_upper.contains("EOS 5D") && !model_upper.contains("EOS 7D")));
    if has_focal_plane && values.len() >= 4 {
        let fpx = values.get(2).copied().unwrap_or(0);
        let fpy = values.get(3).copied().unwrap_or(0);
        if fpx > 0 {
            tags.push(mkt("FocalPlaneXSize", Value::U16(fpx), format!("{:.2} mm", fpx as f64 / 1000.0 * 25.4)));
        }
        if fpy > 0 {
            tags.push(mkt("FocalPlaneYSize", Value::U16(fpy), format!("{:.2} mm", fpy as f64 / 1000.0 * 25.4)));
        }
    }
    tags
}

fn mkt(name: &str, raw: Value, print_val: String) -> Tag {
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: "MakerNotes".into(),
            family1: "Canon".into(),
            family2: "Camera".into(),
        },
        raw_value: raw,
        print_value: print_val,
        priority: 0,
    }
}

/// Perl PrintFraction: convert a float EV value to fraction string like "+2/3", "-1/3", "0"
pub fn print_fraction(val: f64) -> String {
    // Perl's PrintFraction uses predefined fractions for common EV steps
    if val == 0.0 { return "0".to_string(); }
    let abs_val = val.abs();
    let sign = if val < 0.0 { "-" } else { "+" };
    // Common fractions in 1/3 stop increments
    let thirds = (abs_val * 3.0 + 0.5) as i64;
    let whole = thirds / 3;
    let rem = thirds % 3;
    match rem {
        0 => format!("{}{}", sign, whole),
        1 => {
            if whole == 0 { format!("{}1/3", sign) }
            else { format!("{}{} 1/3", sign, whole) }
        }
        2 => {
            if whole == 0 { format!("{}2/3", sign) }
            else { format!("{}{} 2/3", sign, whole) }
        }
        _ => format!("{}{:.0}", sign, abs_val),
    }
}

/// Perl canonWhiteBalance lookup table
pub fn canon_white_balance_str(v: i16) -> String {
    match v {
        0 => "Auto".to_string(),
        1 => "Daylight".to_string(),
        2 => "Cloudy".to_string(),
        3 => "Tungsten".to_string(),
        4 => "Fluorescent".to_string(),
        5 => "Flash".to_string(),
        6 => "Custom".to_string(),
        7 => "Black & White".to_string(),
        8 => "Shade".to_string(),
        9 => "Manual Temperature (Kelvin)".to_string(),
        10 => "PC Set1".to_string(),
        11 => "PC Set2".to_string(),
        12 => "PC Set3".to_string(),
        14 => "Daylight Fluorescent".to_string(),
        15 => "Custom 1".to_string(),
        16 => "Custom 2".to_string(),
        17 => "Underwater".to_string(),
        18 => "Custom 3".to_string(),
        19 => "Custom 4".to_string(),
        20 => "PC Set4".to_string(),
        21 => "PC Set5".to_string(),
        _ => v.to_string(),
    }
}

/// FlashBits bitmask to string (Perl: BITMASK PrintConv)
pub fn flash_bits_str(v: u16) -> String {
    if v == 0 { return "(none)".to_string(); }
    let bits = [
        (0, "Manual"),
        (1, "TTL"),
        (2, "A-TTL"),
        (3, "E-TTL"),
        (4, "FP sync enabled"),
        (7, "2nd-curtain sync used"),
        (11, "FP sync used"),
        (13, "Built-in"),
        (14, "External"),
    ];
    let mut parts: Vec<&str> = bits.iter()
        .filter(|&&(bit, _)| (v >> bit) & 1 == 1)
        .map(|&(_, name)| name)
        .collect();
    if parts.is_empty() {
        v.to_string()
    } else {
        parts.join(", ")
    }
}

/// Look up Canon lens name from canonLensTypes table.
pub fn canon_lens_type_name(val: u16) -> Option<&'static str> {
    match val {
        1 => Some("Canon EF 50mm f/1.8"),
        2 => Some("Canon EF 28mm f/2.8 or Sigma Lens"),
        3 => Some("Canon EF 135mm f/2.8 Soft"),
        4 => Some("Canon EF 35-105mm f/3.5-4.5 or Sigma Lens"),
        5 => Some("Canon EF 35-70mm f/3.5-4.5"),
        6 => Some("Canon EF 28-70mm f/3.5-4.5 or Sigma or Tokina Lens"),
        7 => Some("Canon EF 100-300mm f/5.6L"),
        8 => Some("Canon EF 100-300mm f/5.6 or Sigma or Tokina Lens"),
        9 => Some("Canon EF 70-210mm f/4"),
        10 => Some("Canon EF 50mm f/2.5 Macro or Sigma Lens"),
        11 => Some("Canon EF 35mm f/2"),
        13 => Some("Canon EF 15mm f/2.8 Fisheye"),
        14 => Some("Canon EF 50-200mm f/3.5-4.5L"),
        15 => Some("Canon EF 50-200mm f/3.5-4.5"),
        16 => Some("Canon EF 35-135mm f/3.5-4.5"),
        17 => Some("Canon EF 35-70mm f/3.5-4.5A"),
        18 => Some("Canon EF 28-70mm f/3.5-4.5"),
        20 => Some("Canon EF 100-200mm f/4.5A"),
        21 => Some("Canon EF 80-200mm f/2.8L"),
        22 => Some("Canon EF 20-35mm f/2.8L or Tokina Lens"),
        23 => Some("Canon EF 35-105mm f/3.5-4.5"),
        24 => Some("Canon EF 35-80mm f/4-5.6 Power Zoom"),
        25 => Some("Canon EF 35-80mm f/4-5.6 Power Zoom"),
        26 => Some("Canon EF 100mm f/2.8 Macro or Other Lens"),
        27 => Some("Canon EF 35-80mm f/4-5.6"),
        28 => Some("Canon EF 80-200mm f/4.5-5.6 or Tamron Lens"),
        29 => Some("Canon EF 50mm f/1.8 II"),
        30 => Some("Canon EF 35-105mm f/4.5-5.6"),
        31 => Some("Canon EF 75-300mm f/4-5.6 or Tamron Lens"),
        32 => Some("Canon EF 24mm f/2.8 or Sigma Lens"),
        33 => Some("Voigtlander or Carl Zeiss Lens"),
        35 => Some("Canon EF 35-80mm f/4-5.6"),
        36 => Some("Canon EF 38-76mm f/4.5-5.6"),
        37 => Some("Canon EF 35-80mm f/4-5.6 or Tamron Lens"),
        38 => Some("Canon EF 80-200mm f/4.5-5.6 II"),
        39 => Some("Canon EF 75-300mm f/4-5.6"),
        40 => Some("Canon EF 28-80mm f/3.5-5.6"),
        41 => Some("Canon EF 28-90mm f/4-5.6"),
        42 => Some("Canon EF 28-200mm f/3.5-5.6 or Tamron Lens"),
        43 => Some("Canon EF 28-105mm f/4-5.6"),
        44 => Some("Canon EF 90-300mm f/4.5-5.6"),
        45 => Some("Canon EF-S 18-55mm f/3.5-5.6 [II]"),
        46 => Some("Canon EF 28-90mm f/4-5.6"),
        47 => Some("Zeiss Milvus 35mm f/2 or 50mm f/2"),
        48 => Some("Canon EF-S 18-55mm f/3.5-5.6 IS"),
        49 => Some("Canon EF-S 55-250mm f/4-5.6 IS"),
        50 => Some("Canon EF-S 18-200mm f/3.5-5.6 IS"),
        51 => Some("Canon EF-S 18-135mm f/3.5-5.6 IS"),
        52 => Some("Canon EF-S 18-55mm f/3.5-5.6 IS II"),
        53 => Some("Canon EF-S 18-55mm f/3.5-5.6 III"),
        54 => Some("Canon EF-S 55-250mm f/4-5.6 IS II"),
        60 => Some("Irix 11mm f/4 or 15mm f/2.4"),
        63 => Some("Irix 30mm F1.4 Dragonfly"),
        80 => Some("Canon TS-E 50mm f/2.8L Macro"),
        81 => Some("Canon TS-E 90mm f/2.8L Macro"),
        82 => Some("Canon TS-E 135mm f/4L Macro"),
        94 => Some("Canon TS-E 17mm f/4L"),
        95 => Some("Canon TS-E 24mm f/3.5L II"),
        103 => Some("Samyang AF 14mm f/2.8 EF or Rokinon Lens"),
        106 => Some("Rokinon SP / Samyang XP 35mm f/1.2"),
        112 => Some("Sigma 28mm f/1.5 FF High-speed Prime or other Sigma Lens"),
        117 => Some("Tamron 35-150mm f/2.8-4.0 Di VC OSD (A043) or other Tamron Lens"),
        124 => Some("Canon MP-E 65mm f/2.8 1-5x Macro Photo"),
        125 => Some("Canon TS-E 24mm f/3.5L"),
        126 => Some("Canon TS-E 45mm f/2.8"),
        127 => Some("Canon TS-E 90mm f/2.8 or Tamron Lens"),
        129 => Some("Canon EF 300mm f/2.8L USM"),
        130 => Some("Canon EF 50mm f/1.0L USM"),
        131 => Some("Canon EF 28-80mm f/2.8-4L USM or Sigma Lens"),
        132 => Some("Canon EF 1200mm f/5.6L USM"),
        134 => Some("Canon EF 600mm f/4L IS USM"),
        135 => Some("Canon EF 200mm f/1.8L USM"),
        136 => Some("Canon EF 300mm f/2.8L USM"),
        137 => Some("Canon EF 85mm f/1.2L USM or Sigma or Tamron Lens"),
        138 => Some("Canon EF 28-80mm f/2.8-4L"),
        139 => Some("Canon EF 400mm f/2.8L USM"),
        140 => Some("Canon EF 500mm f/4.5L USM"),
        141 => Some("Canon EF 500mm f/4.5L USM"),
        142 => Some("Canon EF 300mm f/2.8L IS USM"),
        143 => Some("Canon EF 500mm f/4L IS USM or Sigma Lens"),
        144 => Some("Canon EF 35-135mm f/4-5.6 USM"),
        145 => Some("Canon EF 100-300mm f/4.5-5.6 USM"),
        146 => Some("Canon EF 70-210mm f/3.5-4.5 USM"),
        147 => Some("Canon EF 35-135mm f/4-5.6 USM"),
        148 => Some("Canon EF 28-80mm f/3.5-5.6 USM"),
        149 => Some("Canon EF 100mm f/2 USM"),
        150 => Some("Canon EF 14mm f/2.8L USM or Sigma Lens"),
        151 => Some("Canon EF 200mm f/2.8L USM"),
        152 => Some("Canon EF 300mm f/4L IS USM or Sigma Lens"),
        153 => Some("Canon EF 35-350mm f/3.5-5.6L USM or Sigma or Tamron Lens"),
        154 => Some("Canon EF 20mm f/2.8 USM or Zeiss Lens"),
        155 => Some("Canon EF 85mm f/1.8 USM or Sigma Lens"),
        156 => Some("Canon EF 28-105mm f/3.5-4.5 USM or Tamron Lens"),
        160 => Some("Canon EF 20-35mm f/3.5-4.5 USM or Tamron or Tokina Lens"),
        161 => Some("Canon EF 28-70mm f/2.8L USM or Other Lens"),
        162 => Some("Canon EF 200mm f/2.8L USM"),
        163 => Some("Canon EF 300mm f/4L"),
        164 => Some("Canon EF 400mm f/5.6L"),
        165 => Some("Canon EF 70-200mm f/2.8L USM"),
        166 => Some("Canon EF 70-200mm f/2.8L USM + 1.4x"),
        167 => Some("Canon EF 70-200mm f/2.8L USM + 2x"),
        168 => Some("Canon EF 28mm f/1.8 USM or Sigma Lens"),
        169 => Some("Canon EF 17-35mm f/2.8L USM or Sigma Lens"),
        170 => Some("Canon EF 200mm f/2.8L II USM or Sigma Lens"),
        171 => Some("Canon EF 300mm f/4L USM"),
        172 => Some("Canon EF 400mm f/5.6L USM or Sigma Lens"),
        173 => Some("Canon EF 180mm Macro f/3.5L USM or Sigma Lens"),
        174 => Some("Canon EF 135mm f/2L USM or Other Lens"),
        175 => Some("Canon EF 400mm f/2.8L USM"),
        176 => Some("Canon EF 24-85mm f/3.5-4.5 USM"),
        177 => Some("Canon EF 300mm f/4L IS USM"),
        178 => Some("Canon EF 28-135mm f/3.5-5.6 IS"),
        179 => Some("Canon EF 24mm f/1.4L USM"),
        180 => Some("Canon EF 35mm f/1.4L USM or Other Lens"),
        181 => Some("Canon EF 100-400mm f/4.5-5.6L IS USM + 1.4x or Sigma Lens"),
        182 => Some("Canon EF 100-400mm f/4.5-5.6L IS USM + 2x or Sigma Lens"),
        183 => Some("Canon EF 100-400mm f/4.5-5.6L IS USM or Sigma Lens"),
        184 => Some("Canon EF 400mm f/2.8L USM + 2x"),
        185 => Some("Canon EF 600mm f/4L IS USM"),
        186 => Some("Canon EF 70-200mm f/4L USM"),
        187 => Some("Canon EF 70-200mm f/4L USM + 1.4x"),
        188 => Some("Canon EF 70-200mm f/4L USM + 2x"),
        189 => Some("Canon EF 70-200mm f/4L USM + 2.8x"),
        190 => Some("Canon EF 100mm f/2.8 Macro USM"),
        191 => Some("Canon EF 400mm f/4 DO IS or Sigma Lens"),
        193 => Some("Canon EF 35-80mm f/4-5.6 USM"),
        194 => Some("Canon EF 80-200mm f/4.5-5.6 USM"),
        195 => Some("Canon EF 35-105mm f/4.5-5.6 USM"),
        196 => Some("Canon EF 75-300mm f/4-5.6 USM"),
        197 => Some("Canon EF 75-300mm f/4-5.6 IS USM or Sigma Lens"),
        198 => Some("Canon EF 50mm f/1.4 USM or Other Lens"),
        199 => Some("Canon EF 28-80mm f/3.5-5.6 USM"),
        200 => Some("Canon EF 75-300mm f/4-5.6 USM"),
        201 => Some("Canon EF 28-80mm f/3.5-5.6 USM"),
        202 => Some("Canon EF 28-80mm f/3.5-5.6 USM IV"),
        208 => Some("Canon EF 22-55mm f/4-5.6 USM"),
        209 => Some("Canon EF 55-200mm f/4.5-5.6"),
        210 => Some("Canon EF 28-90mm f/4-5.6 USM"),
        211 => Some("Canon EF 28-200mm f/3.5-5.6 USM"),
        212 => Some("Canon EF 28-105mm f/4-5.6 USM"),
        213 => Some("Canon EF 90-300mm f/4.5-5.6 USM or Tamron Lens"),
        214 => Some("Canon EF-S 18-55mm f/3.5-5.6 USM"),
        215 => Some("Canon EF 55-200mm f/4.5-5.6 II USM"),
        217 => Some("Tamron AF 18-270mm f/3.5-6.3 Di II VC PZD"),
        220 => Some("Yongnuo YN 50mm f/1.8"),
        224 => Some("Canon EF 70-200mm f/2.8L IS USM"),
        225 => Some("Canon EF 70-200mm f/2.8L IS USM + 1.4x"),
        226 => Some("Canon EF 70-200mm f/2.8L IS USM + 2x"),
        227 => Some("Canon EF 70-200mm f/2.8L IS USM + 2.8x"),
        228 => Some("Canon EF 28-105mm f/3.5-4.5 USM"),
        229 => Some("Canon EF 16-35mm f/2.8L USM"),
        230 => Some("Canon EF 24-70mm f/2.8L USM"),
        231 => Some("Canon EF 17-40mm f/4L USM or Sigma Lens"),
        232 => Some("Canon EF 70-300mm f/4.5-5.6 DO IS USM"),
        233 => Some("Canon EF 28-300mm f/3.5-5.6L IS USM"),
        234 => Some("Canon EF-S 17-85mm f/4-5.6 IS USM or Tokina Lens"),
        235 => Some("Canon EF-S 10-22mm f/3.5-4.5 USM"),
        236 => Some("Canon EF-S 60mm f/2.8 Macro USM"),
        237 => Some("Canon EF 24-105mm f/4L IS USM"),
        238 => Some("Canon EF 70-300mm f/4-5.6 IS USM"),
        239 => Some("Canon EF 85mm f/1.2L II USM or Rokinon Lens"),
        240 => Some("Canon EF-S 17-55mm f/2.8 IS USM or Sigma Lens"),
        241 => Some("Canon EF 50mm f/1.2L USM"),
        242 => Some("Canon EF 70-200mm f/4L IS USM"),
        243 => Some("Canon EF 70-200mm f/4L IS USM + 1.4x"),
        244 => Some("Canon EF 70-200mm f/4L IS USM + 2x"),
        245 => Some("Canon EF 70-200mm f/4L IS USM + 2.8x"),
        246 => Some("Canon EF 16-35mm f/2.8L II USM"),
        247 => Some("Canon EF 14mm f/2.8L II USM"),
        248 => Some("Canon EF 200mm f/2L IS USM or Sigma Lens"),
        249 => Some("Canon EF 800mm f/5.6L IS USM"),
        250 => Some("Canon EF 24mm f/1.4L II USM or Sigma Lens"),
        251 => Some("Canon EF 70-200mm f/2.8L IS II USM"),
        252 => Some("Canon EF 70-200mm f/2.8L IS II USM + 1.4x"),
        253 => Some("Canon EF 70-200mm f/2.8L IS II USM + 2x"),
        254 => Some("Canon EF 100mm f/2.8L Macro IS USM or Tamron Lens"),
        255 => Some("Sigma 24-105mm f/4 DG OS HSM | A or Other Lens"),
        368 => Some("Sigma 14-24mm f/2.8 DG HSM | A or other Sigma Lens"),
        488 => Some("Canon EF-S 15-85mm f/3.5-5.6 IS USM"),
        489 => Some("Canon EF 70-300mm f/4-5.6L IS USM"),
        490 => Some("Canon EF 8-15mm f/4L Fisheye USM"),
        491 => Some("Canon EF 300mm f/2.8L IS II USM or Tamron Lens"),
        492 => Some("Canon EF 400mm f/2.8L IS II USM"),
        493 => Some("Canon EF 500mm f/4L IS II USM or EF 24-105mm f4L IS USM"),
        494 => Some("Canon EF 600mm f/4L IS II USM"),
        495 => Some("Canon EF 24-70mm f/2.8L II USM or Sigma Lens"),
        496 => Some("Canon EF 200-400mm f/4L IS USM"),
        499 => Some("Canon EF 200-400mm f/4L IS USM + 1.4x"),
        502 => Some("Canon EF 28mm f/2.8 IS USM or Tamron Lens"),
        503 => Some("Canon EF 24mm f/2.8 IS USM"),
        504 => Some("Canon EF 24-70mm f/4L IS USM"),
        505 => Some("Canon EF 35mm f/2 IS USM"),
        506 => Some("Canon EF 400mm f/4 DO IS II USM"),
        507 => Some("Canon EF 16-35mm f/4L IS USM"),
        508 => Some("Canon EF 11-24mm f/4L USM or Tamron Lens"),
        624 => Some("Sigma 70-200mm f/2.8 DG OS HSM | S or other Sigma Lens"),
        747 => Some("Canon EF 100-400mm f/4.5-5.6L IS II USM or Tamron Lens"),
        748 => Some("Canon EF 100-400mm f/4.5-5.6L IS II USM + 1.4x or Tamron Lens"),
        749 => Some("Canon EF 100-400mm f/4.5-5.6L IS II USM + 2x or Tamron Lens"),
        750 => Some("Canon EF 35mm f/1.4L II USM or Tamron Lens"),
        751 => Some("Canon EF 16-35mm f/2.8L III USM"),
        752 => Some("Canon EF 24-105mm f/4L IS II USM"),
        753 => Some("Canon EF 85mm f/1.4L IS USM"),
        754 => Some("Canon EF 70-200mm f/4L IS II USM"),
        757 => Some("Canon EF 400mm f/2.8L IS III USM"),
        758 => Some("Canon EF 600mm f/4L IS III USM"),
        923 => Some("Meike/SKY 85mm f/1.8 DCM"),
        1136 => Some("Sigma 24-70mm f/2.8 DG OS HSM | A"),
        4142 => Some("Canon EF-S 18-135mm f/3.5-5.6 IS STM"),
        4143 => Some("Canon EF-M 18-55mm f/3.5-5.6 IS STM or Tamron Lens"),
        4144 => Some("Canon EF 40mm f/2.8 STM"),
        4145 => Some("Canon EF-M 22mm f/2 STM"),
        4146 => Some("Canon EF-S 18-55mm f/3.5-5.6 IS STM"),
        4147 => Some("Canon EF-M 11-22mm f/4-5.6 IS STM"),
        4148 => Some("Canon EF-S 55-250mm f/4-5.6 IS STM"),
        4149 => Some("Canon EF-M 55-200mm f/4.5-6.3 IS STM"),
        4150 => Some("Canon EF-S 10-18mm f/4.5-5.6 IS STM"),
        4152 => Some("Canon EF 24-105mm f/3.5-5.6 IS STM"),
        4153 => Some("Canon EF-M 15-45mm f/3.5-6.3 IS STM"),
        4154 => Some("Canon EF-S 24mm f/2.8 STM"),
        4155 => Some("Canon EF-M 28mm f/3.5 Macro IS STM"),
        4156 => Some("Canon EF 50mm f/1.8 STM"),
        4157 => Some("Canon EF-M 18-150mm f/3.5-6.3 IS STM"),
        4158 => Some("Canon EF-S 18-55mm f/4-5.6 IS STM"),
        4159 => Some("Canon EF-M 32mm f/1.4 STM"),
        4160 => Some("Canon EF-S 35mm f/2.8 Macro IS STM"),
        4208 => Some("Sigma 56mm f/1.4 DC DN | C or other Sigma Lens"),
        4976 => Some("Sigma 16-300mm F3.5-6.7 DC OS | C (025)"),
        6512 => Some("Sigma 12mm F1.4 DC | C"),
        36910 => Some("Canon EF 70-300mm f/4-5.6 IS II USM"),
        36912 => Some("Canon EF-S 18-135mm f/3.5-5.6 IS USM"),
        61182 => Some("Canon RF 50mm F1.2L USM or other Canon RF Lens"),
        61491 => Some("Canon CN-E 14mm T3.1 L F"),
        61492 => Some("Canon CN-E 24mm T1.5 L F"),
        61494 => Some("Canon CN-E 85mm T1.3 L F"),
        61495 => Some("Canon CN-E 135mm T2.2 L F"),
        61496 => Some("Canon CN-E 35mm T1.5 L F"),
        65535 => Some("n/a"),
        _ => None,
    }
}
