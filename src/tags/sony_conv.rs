//! Sony MakerNotes print conversions.

/// Print conversion for Sony CreativeStyle (tag 0xB020).
pub fn creative_style(s: &str) -> &str {
    match s.trim() {
        "Standard" => "Standard",
        "Vivid" => "Vivid",
        "Portrait" => "Portrait",
        "Landscape" => "Landscape",
        "Sunset" => "Sunset",
        "Nightview" => "Night View/Portrait",
        "BW" => "B&W",
        "Neutral" => "Neutral",
        "Clear" => "Clear",
        "Deep" => "Deep",
        "Light" => "Light",
        "Autumnleaves" => "Autumn Leaves",
        "Sepia" => "Sepia",
        "None" => "None",
        "AdobeRGB" => "Adobe RGB",
        "Real" => "Real",
        "VV2" => "Vivid 2",
        "FL" => "FL",
        "IN" => "IN",
        "SH" => "SH",
        other => other,
    }
}

/// Print conversion for Sony SceneMode (tag 0xB023).
pub fn scene_mode(v: u64) -> Option<&'static str> {
    Some(match v {
        0 => "Standard",
        1 => "Portrait",
        2 => "Text",
        3 => "Night Scene",
        4 => "Sunset",
        5 => "Sports",
        6 => "Landscape",
        7 => "Night Portrait",
        8 => "Macro",
        9 => "Super Macro",
        16 => "Auto",
        17 => "Night View/Portrait",
        18 => "Sweep Panorama",
        19 => "Handheld Night Shot",
        20 => "Anti Motion Blur",
        21 => "Cont. Priority AE",
        22 => "Auto+",
        23 => "3D Sweep Panorama",
        24 => "Superior Auto",
        25 => "High Sensitivity",
        26 => "Fireworks",
        27 => "Food",
        28 => "Pet",
        33 => "HDR",
        0xFFFF => "n/a",
        _ => return None,
    })
}

/// Print conversion for Sony DynamicRangeOptimizer (tag 0xB025).
pub fn dro(v: u64) -> Option<&'static str> {
    Some(match v {
        0 => "Off",
        1 => "Standard",
        2 => "Advanced Auto",
        3 => "Auto",
        8 => "Advanced Lv1",
        9 => "Advanced Lv2",
        10 => "Advanced Lv3",
        11 => "Advanced Lv4",
        12 => "Advanced Lv5",
        16 => "Lv1",
        17 => "Lv2",
        18 => "Lv3",
        19 => "Lv4",
        20 => "Lv5",
        _ => return None,
    })
}

/// Print conversion for Sony ColorMode (tag 0xB029).
pub fn color_mode(v: u64) -> Option<&'static str> {
    Some(match v {
        0 => "Standard",
        1 => "Vivid",
        2 => "Portrait",
        3 => "Landscape",
        4 => "Sunset",
        5 => "Night View/Portrait",
        6 => "B&W",
        7 => "Adobe RGB",
        12 => "Neutral",
        13 => "Clear",
        14 => "Deep",
        15 => "Light",
        16 => "Autumn Leaves",
        17 => "Sepia",
        100 => "Neutral",
        101 => "Clear",
        102 => "Deep",
        103 => "Light",
        104 => "Night View",
        105 => "Autumn Leaves",
        255 => "Off",
        0xFFFFFFFF => "n/a",
        _ => return None,
    })
}

/// Print conversion for Sony ExposureMode (tag 0xB041).
pub fn exposure_mode(v: u64) -> Option<&'static str> {
    Some(match v {
        0 => "Program AE",
        1 => "Portrait",
        2 => "Beach",
        3 => "Sports",
        4 => "Snow",
        5 => "Landscape",
        6 => "Auto",
        7 => "Aperture-priority AE",
        8 => "Shutter speed priority AE",
        9 => "Night Scene / Twilight",
        10 => "Hi-Speed Shutter",
        11 => "Twilight Portrait",
        12 => "Soft Snap/Portrait",
        13 => "Fireworks",
        14 => "Smile Shutter",
        15 => "Manual",
        18 => "High Sensitivity",
        19 => "Macro",
        20 => "Advanced Sports Shooting",
        29 => "Underwater",
        33 => "Food",
        34 => "Sweep Panorama",
        35 => "Handheld Night Shot",
        36 => "Anti Motion Blur",
        37 => "Pet",
        38 => "Backlight Correction HDR",
        40 => "Superior Auto",
        47 => "HDR",
        49 => "Movie",
        _ => return None,
    })
}

/// Print conversion for Sony FocusMode (tag 0x201B).
pub fn focus_mode(v: u64) -> Option<&'static str> {
    Some(match v {
        0 => "Manual",
        2 => "AF-S",
        3 => "AF-C",
        4 => "AF-A",
        6 => "DMF",
        _ => return None,
    })
}

/// Print conversion for Sony AFAreaMode (tag 0x201C).
pub fn af_area_mode(v: u64) -> Option<&'static str> {
    Some(match v {
        0 => "Wide",
        1 => "Spot",
        2 => "Zone",
        3 => "Center",
        4 => "Flexible Spot",
        6 => "Flexible Spot (S)",
        7 => "Flexible Spot (M)",
        8 => "Flexible Spot (L)",
        9 => "Expand Flexible Spot",
        _ => return None,
    })
}
