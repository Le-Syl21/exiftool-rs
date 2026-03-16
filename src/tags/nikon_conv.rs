//! Nikon MakerNotes print conversions.

/// Print conversion for Nikon FlashMode (tag 0x0087).
pub fn flash_mode(v: u64) -> Option<&'static str> {
    Some(match v {
        0 => "Did Not Fire",
        1 => "Fired, Manual",
        3 => "Not Ready",
        7 => "Fired, External",
        8 => "Fired, Commander Mode",
        9 => "Fired, TTL Mode",
        18 => "LED Light",
        _ => return None,
    })
}

/// Print conversion for Nikon ShootingMode (tag 0x0089) - bitmask.
pub fn shooting_mode(v: u16) -> String {
    let mut modes = Vec::new();
    if v & 0x0001 != 0 { modes.push("Continuous"); }
    if v & 0x0002 != 0 { modes.push("Delay"); }
    if v & 0x0004 != 0 { modes.push("PC Control"); }
    if v & 0x0008 != 0 { modes.push("Self-timer"); }
    if v & 0x0010 != 0 { modes.push("Exposure Bracketing"); }
    if v & 0x0020 != 0 { modes.push("Auto ISO"); }
    if v & 0x0040 != 0 { modes.push("White-Balance Bracketing"); }
    if v & 0x0080 != 0 { modes.push("IR Control"); }
    if v & 0x0100 != 0 { modes.push("D-Lighting Bracketing"); }
    if modes.is_empty() { "Single-Frame".to_string() } else { modes.join(", ") }
}

/// Print conversion for Nikon ColorSpace (tag 0x001E).
pub fn color_space(v: u64) -> Option<&'static str> {
    Some(match v {
        1 => "sRGB",
        2 => "Adobe RGB",
        _ => return None,
    })
}

/// Print conversion for Nikon ActiveD-Lighting (tag 0x0022).
pub fn active_d_lighting(v: u64) -> Option<&'static str> {
    Some(match v {
        0 => "Off",
        1 => "Low",
        3 => "Normal",
        5 => "High",
        7 => "Extra High",
        8 => "Extra High 1",
        9 => "Extra High 2",
        10 => "Extra High 3",
        11 => "Extra High 4",
        0xFFFF => "Auto",
        _ => return None,
    })
}

/// Print conversion for Nikon VignetteControl (tag 0x002A).
pub fn vignette_control(v: u64) -> Option<&'static str> {
    Some(match v {
        0 => "Off",
        1 => "Low",
        3 => "Normal",
        5 => "High",
        _ => return None,
    })
}

/// Print conversion for Nikon HighISONoiseReduction (tag 0x00B1).
pub fn high_iso_nr(v: u64) -> Option<&'static str> {
    Some(match v {
        0 => "Off",
        1 => "Minimal",
        2 => "Low",
        3 => "Medium Low",
        4 => "Normal",
        5 => "Medium High",
        6 => "High",
        _ => return None,
    })
}

/// Print conversion for Nikon NEFCompression (tag 0x0093).
pub fn nef_compression(v: u64) -> Option<&'static str> {
    Some(match v {
        1 => "Lossy (type 1)",
        2 => "Uncompressed",
        3 => "Lossless",
        4 => "Lossy (type 2)",
        5 => "Striped packed 12 bits",
        6 => "Uncompressed (reduced to 12 bit)",
        7 => "Unpacked 12 bits",
        8 => "Small",
        9 => "Packed 12 bits",
        10 => "Packed 14 bits",
        13 => "High Efficiency",
        14 => "High Efficiency*",
        _ => return None,
    })
}
