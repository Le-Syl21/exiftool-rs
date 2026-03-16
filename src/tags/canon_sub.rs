//! Canon MakerNotes sub-table decoders (auto-generated).
//! All CameraSettings + ShotInfo + FocalLength fields.

use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

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
        tags.push(mkt("CameraISO", Value::I16(v), v.to_string()));
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
        tags.push(mkt("LensType", Value::I16(v), v.to_string()));
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
        let pv = match v {
            0 => "Manual",
            1 => "TTL",
            2 => "A-TTL",
            3 => "E-TTL",
            4 => "FP sync enabled",
            7 => "2nd-curtain sync used",
            11 => "FP sync used",
            13 => "Built-in",
            14 => "External",
            _ => "",
        };
        let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
        tags.push(mkt("FlashBits", Value::I16(v), pv));
    }
    if let Some(v) = get(32) {
        let pv = match v {
            0 => "Single",
            1 => "Continuous",
            8 => "Manual",
            _ => "",
        };
        let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
        tags.push(mkt("FocusContinuous", Value::I16(v), pv));
    }
    if let Some(v) = get(33) {
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
    if let Some(v) = get(34) {
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
    if let Some(v) = get(35) {
        tags.push(mkt("DisplayAperture", Value::I16(v), v.to_string()));
    }
    if let Some(v) = get(36) {
        tags.push(mkt("ZoomSourceWidth", Value::I16(v), v.to_string()));
    }
    if let Some(v) = get(37) {
        tags.push(mkt("ZoomTargetWidth", Value::I16(v), v.to_string()));
    }
    if let Some(v) = get(39) {
        let pv = match v {
            0 => "Center",
            1 => "AF Point",
            _ => "",
        };
        let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
        tags.push(mkt("SpotMeteringMode", Value::I16(v), pv));
    }
    if let Some(v) = get(40) {
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

pub fn decode_shot_info(values: &[i16]) -> Vec<Tag> {
    let mut tags = Vec::new();
    let get = |idx: usize| -> Option<i16> { values.get(idx).copied() };

    if let Some(v) = get(1) {
        tags.push(mkt("AutoISO", Value::I16(v), v.to_string()));
    }
    if let Some(v) = get(2) {
        tags.push(mkt("BaseISO", Value::I16(v), v.to_string()));
    }
    if let Some(v) = get(3) {
        tags.push(mkt("MeasuredEV", Value::I16(v), v.to_string()));
    }
    if let Some(v) = get(4) {
        tags.push(mkt("TargetAperture", Value::I16(v), v.to_string()));
    }
    if let Some(v) = get(5) {
        tags.push(mkt("TargetExposureTime", Value::I16(v), v.to_string()));
    }
    if let Some(v) = get(6) {
        tags.push(mkt("ExposureCompensation", Value::I16(v), v.to_string()));
    }
    if let Some(v) = get(7) {
        tags.push(mkt("WhiteBalance", Value::I16(v), v.to_string()));
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
        tags.push(mkt("OpticalZoomCode", Value::I16(v), v.to_string()));
    }
    if let Some(v) = get(12) {
        tags.push(mkt("CameraTemperature", Value::I16(v), v.to_string()));
    }
    if let Some(v) = get(13) {
        tags.push(mkt("FlashGuideNumber", Value::I16(v), v.to_string()));
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
        tags.push(mkt("FlashExposureComp", Value::I16(v), v.to_string()));
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
        tags.push(mkt("AEBBracketValue", Value::I16(v), v.to_string()));
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
    if let Some(v) = get(19) {
        tags.push(mkt("FocusDistanceUpper", Value::I16(v), v.to_string()));
    }
    if let Some(v) = get(20) {
        tags.push(mkt("FocusDistanceLower", Value::I16(v), v.to_string()));
    }
    if let Some(v) = get(21) {
        tags.push(mkt("FNumber", Value::I16(v), v.to_string()));
    }
    if let Some(v) = get(23) {
        tags.push(mkt("MeasuredEV2", Value::I16(v), v.to_string()));
    }
    if let Some(v) = get(24) {
        tags.push(mkt("BulbDuration", Value::I16(v), v.to_string()));
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
        let pv = match v {
            -1 => "n/a",
            0 => "None",
            1 => "Rotate 90 CW",
            2 => "Rotate 180",
            3 => "Rotate 270 CW",
            _ => "",
        };
        let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
        tags.push(mkt("AutoRotate", Value::I16(v), pv));
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
        tags.push(mkt("SelfTimer2", Value::I16(v), v.to_string()));
    }
    if let Some(v) = get(33) {
        tags.push(mkt("FlashOutput", Value::I16(v), v.to_string()));
    }
    tags
}

pub fn decode_focal_length(values: &[u16]) -> Vec<Tag> {
    let mut tags = Vec::new();
    if let Some(&v) = values.get(0) {
        let pv = match v { 1 => "Fixed", 2 => "Zoom", _ => "" };
        let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
        tags.push(mkt("FocalType", Value::U16(v), pv));
    }
    if let Some(&v) = values.get(1) {
        tags.push(mkt("FocalLength", Value::U16(v), format!("{} mm", v)));
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
