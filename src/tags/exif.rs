//! EXIF tag definitions and print conversions.
//!
//! Mirrors the tag tables from ExifTool's Exif.pm.
//! This covers the most commonly used EXIF tags across IFD0, ExifIFD, and GPS.

use crate::value::Value;

/// Static tag information entry.
pub struct TagInfo {
    pub tag_id: u16,
    pub name: &'static str,
    pub description: &'static str,
    pub family2: &'static str,
}

/// Lookup tag info by IFD name and tag ID.
pub fn lookup(ifd: &str, tag_id: u16) -> Option<&'static TagInfo> {
    let table = match ifd {
        "IFD0" | "IFD1" => IFD0_TAGS,
        "ExifIFD" => EXIF_IFD_TAGS,
        "GPS" => GPS_TAGS,
        "InteropIFD" => INTEROP_TAGS,
        _ => IFD0_TAGS,
    };

    table.iter().find(|t| t.tag_id == tag_id)
}

/// Reverse lookup: find the (ifd, tag_id) for an EXIF tag name. Searches ExifIFD
/// first (where most photographic tags live), then IFD0, GPS and Interop.
pub fn tag_id_by_name(name: &str) -> Option<(&'static str, u16)> {
    for (ifd, table) in [
        ("ExifIFD", EXIF_IFD_TAGS),
        ("IFD0", IFD0_TAGS),
        ("GPS", GPS_TAGS),
        ("InteropIFD", INTEROP_TAGS),
    ] {
        if let Some(t) = table.iter().find(|t| t.name == name) {
            return Some((ifd, t.tag_id));
        }
    }
    None
}

/// Apply the EXIF print-conversion chain to a tag identified by name. Used for
/// XMP exif:/tiff: namespace tags, which ExifTool shares with the EXIF table:
/// hand-written conv, then generated enum conv. Returns None if neither applies.
pub fn print_conv_by_tag_name(name: &str, value: &Value) -> Option<String> {
    if let Some((ifd, id)) = tag_id_by_name(name) {
        if let Some(s) = print_conv(ifd, id, value) {
            return Some(s);
        }
    }
    if let Some(v) = value.as_u64() {
        if let Some(s) = super::print_conv_generated::print_conv_by_name(name, v as i64) {
            return Some(s.to_string());
        }
    }
    None
}

/// Lookup tag name from generated tables (fallback for unknown tags).
pub fn lookup_generated(tag_id: u16) -> Option<(&'static str, &'static str)> {
    super::generated::GENERATED_EXIF_TAGS
        .iter()
        .find(|t| t.tag_id == tag_id)
        .map(|t| (t.name, t.description))
}

/// Apply print conversion for known tags.
pub fn print_conv(ifd: &str, tag_id: u16, value: &Value) -> Option<String> {
    match (ifd, tag_id) {
        // SubjectDistance: "$val m" (inf/undef passed through).
        ("ExifIFD", 0x9206) => {
            let d = value.to_display_string();
            if d == "inf" || d == "undef" || d.is_empty() {
                return Some(d);
            }
            return Some(format!("{} m", d));
        }
        // GPSHPositioningError: "$val m"
        ("GPS", 0x001F) => {
            return Some(format!("{} m", value.to_display_string()));
        }
        // GPS reference/mode enums (string or short values).
        ("GPS", 0x0009) => {
            return Some(match value.to_display_string().trim() {
                "A" => "Measurement Active",
                "V" => "Measurement Void",
                _ => return None,
            }
            .to_string());
        }
        ("GPS", 0x000A) => {
            return Some(match value.to_display_string().trim() {
                "2" => "2-Dimensional Measurement",
                "3" => "3-Dimensional Measurement",
                _ => return None,
            }
            .to_string());
        }
        ("GPS", 0x000C) | ("GPS", 0x0019) => {
            let v = value.to_display_string();
            return Some(match v.trim() {
                "K" => "km/h".to_string(),
                "M" => "mph".to_string(),
                "N" => "knots".to_string(),
                other => format!("Unknown ({})", other),
            });
        }
        ("GPS", 0x000E) | ("GPS", 0x0010) | ("GPS", 0x0017) => {
            let v = value.to_display_string();
            return Some(match v.trim() {
                "M" => "Magnetic North".to_string(),
                "T" => "True North".to_string(),
                other => format!("Unknown ({})", other),
            });
        }
        // GPSTimeStamp (3 rationals H M S): ExifTool ConvertTimeStamp -> "HH:MM:SS[.ff]"
        ("GPS", 0x0007) => {
            let parts: Vec<f64> = value
                .to_display_string()
                .split_whitespace()
                .filter_map(|s| s.parse::<f64>().ok())
                .collect();
            if parts.len() == 3 {
                let total = (parts[0] * 60.0 + parts[1]) * 60.0 + parts[2];
                let h = (total / 3600.0).floor();
                let rem = total - h * 3600.0;
                let m = (rem / 60.0).floor();
                let secs = rem - m * 60.0;
                // %012.9f then trim trailing zeros and a bare decimal point.
                let mut ss = format!("{:012.9}", secs);
                if ss.contains('.') {
                    ss = ss.trim_end_matches('0').trim_end_matches('.').to_string();
                }
                return Some(format!("{:02}:{:02}:{}", h as i64, m as i64, ss));
            }
        }
        // GPSVersionID, DNGVersion, DNGBackwardVersion: join components with "."
        ("GPS", 0x0000) | (_, 0xC612) | (_, 0xC613) => {
            let joined: Vec<String> = value
                .to_display_string()
                .split_whitespace()
                .map(|s| s.to_string())
                .collect();
            if !joined.is_empty() {
                return Some(joined.join("."));
            }
        }
        // SubfileType / NewSubfileType (%subfileType)
        (_, 0x00FE) => {
            if let Some(v) = value.as_u64() {
                let s = match v {
                    0 => "Full-resolution image",
                    1 => "Reduced-resolution image",
                    2 => "Single page of multi-page image",
                    3 => "Single page of multi-page reduced-resolution image",
                    4 => "Transparency mask",
                    5 => "Transparency mask of reduced-resolution image",
                    6 => "Transparency mask of multi-page image",
                    7 => "Transparency mask of reduced-resolution multi-page image",
                    8 => "Depth map",
                    9 => "Depth map of reduced-resolution image",
                    16 => "Enhanced image data",
                    _ => return None,
                };
                return Some(s.to_string());
            }
        }
        // Orientation
        (_, 0x0112) => {
            if let Some(v) = value.as_u64() {
                return Some(
                    match v {
                        1 => "Horizontal (normal)",
                        2 => "Mirror horizontal",
                        3 => "Rotate 180",
                        4 => "Mirror vertical",
                        5 => "Mirror horizontal and rotate 270 CW",
                        6 => "Rotate 90 CW",
                        7 => "Mirror horizontal and rotate 90 CW",
                        8 => "Rotate 270 CW",
                        _ => return None,
                    }
                    .to_string(),
                );
            }
        }
        // ResolutionUnit
        (_, 0x0128) => {
            if let Some(v) = value.as_u64() {
                return Some(
                    match v {
                        1 => "No Absolute Unit",
                        2 => "inches",
                        3 => "centimeters",
                        _ => return None,
                    }
                    .to_string(),
                );
            }
        }
        // YCbCrPositioning
        (_, 0x0213) => {
            if let Some(v) = value.as_u64() {
                return Some(
                    match v {
                        1 => "Centered",
                        2 => "Co-sited",
                        _ => return None,
                    }
                    .to_string(),
                );
            }
        }
        // ExposureProgram
        ("ExifIFD", 0x8822) => {
            if let Some(v) = value.as_u64() {
                return Some(
                    match v {
                        0 => "Not Defined",
                        1 => "Manual",
                        2 => "Program AE",
                        3 => "Aperture-priority AE",
                        4 => "Shutter speed priority AE",
                        5 => "Creative (Slow speed)",
                        6 => "Action (High speed)",
                        7 => "Portrait",
                        8 => "Landscape",
                        _ => return None,
                    }
                    .to_string(),
                );
            }
        }
        // MeteringMode
        ("ExifIFD", 0x9207) => {
            if let Some(v) = value.as_u64() {
                return Some(
                    match v {
                        0 => "Unknown",
                        1 => "Average",
                        2 => "Center-weighted average",
                        3 => "Spot",
                        4 => "Multi-spot",
                        5 => "Multi-segment",
                        6 => "Partial",
                        255 => "Other",
                        _ => return None,
                    }
                    .to_string(),
                );
            }
        }
        // LightSource
        ("ExifIFD", 0x9208) => {
            if let Some(v) = value.as_u64() {
                return Some(
                    match v {
                        0 => "Unknown",
                        1 => "Daylight",
                        2 => "Fluorescent",
                        3 => "Tungsten (Incandescent)",
                        4 => "Flash",
                        9 => "Fine Weather",
                        10 => "Cloudy",
                        11 => "Shade",
                        255 => "Other",
                        _ => return None,
                    }
                    .to_string(),
                );
            }
        }
        // Flash (Perl %flash table, keyed by the int16u bitmask)
        (_, 0x9209) => {
            if let Some(v) = value.as_u64() {
                let s = match v {
                    0x00 => "No Flash",
                    0x01 => "Fired",
                    0x05 => "Fired, Return not detected",
                    0x07 => "Fired, Return detected",
                    0x08 => "On, Did not fire",
                    0x09 => "On, Fired",
                    0x0d => "On, Return not detected",
                    0x0f => "On, Return detected",
                    0x10 => "Off, Did not fire",
                    0x14 => "Off, Did not fire, Return not detected",
                    0x18 => "Auto, Did not fire",
                    0x19 => "Auto, Fired",
                    0x1d => "Auto, Fired, Return not detected",
                    0x1f => "Auto, Fired, Return detected",
                    0x20 => "No flash function",
                    0x30 => "Off, No flash function",
                    0x41 => "Fired, Red-eye reduction",
                    0x45 => "Fired, Red-eye reduction, Return not detected",
                    0x47 => "Fired, Red-eye reduction, Return detected",
                    0x49 => "On, Red-eye reduction",
                    0x4d => "On, Red-eye reduction, Return not detected",
                    0x4f => "On, Red-eye reduction, Return detected",
                    0x50 => "Off, Red-eye reduction",
                    0x58 => "Auto, Did not fire, Red-eye reduction",
                    0x59 => "Auto, Fired, Red-eye reduction",
                    0x5d => "Auto, Fired, Red-eye reduction, Return not detected",
                    0x5f => "Auto, Fired, Red-eye reduction, Return detected",
                    _ => return None,
                };
                return Some(s.to_string());
            }
        }
        // ColorSpace (PrintHex; default "Unknown (0xN)").
        ("ExifIFD", 0xA001) => {
            if let Some(v) = value.as_u64() {
                return Some(match v {
                    1 => "sRGB".to_string(),
                    2 => "Adobe RGB".to_string(),
                    0xFFFD => "Wide Gamut RGB".to_string(),
                    0xFFFE => "ICC Profile".to_string(),
                    0xFFFF => "Uncalibrated".to_string(),
                    n => format!("Unknown (0x{:x})", n),
                });
            }
        }
        // ExposureTime — ExifTool's PrintExposureTime: "1/60" or "4" (no " s").
        (_, 0x829A) => {
            if let Value::URational(n, d) = value {
                if *d != 0 {
                    let secs = *n as f64 / *d as f64;
                    if secs > 0.0 && secs < 0.250_01 {
                        return Some(format!("1/{}", (0.5 + 1.0 / secs).floor() as i64));
                    }
                    let s = format!("{:.1}", secs);
                    return Some(s.strip_suffix(".0").map(str::to_string).unwrap_or(s));
                }
            }
        }
        // FNumber — Exif::PrintFNumber: "%.2f" below f/1, else "%.1f".
        (_, 0x829D) => {
            if let Some(v) = value.as_f64() {
                return Some(if v > 0.0 && v < 1.0 {
                    format!("{:.2}", v)
                } else {
                    format!("{:.1}", v)
                });
            }
        }
        // ApertureValue / MaxApertureValue — APEX: FNumber = 2^(val/2), "%.1f".
        (_, 0x9202) | (_, 0x9205) => {
            if let Some(v) = value.as_f64() {
                return Some(format!("{:.1}", 2f64.powf(v / 2.0)));
            }
        }
        // ShutterSpeedValue — APEX: exposure = 2^(-val) (0 if |val|>=100),
        // then ExifTool's PrintExposureTime.
        (_, 0x9201) => {
            if let Some(v) = value.as_f64() {
                let secs = if v.abs() < 100.0 { 2f64.powf(-v) } else { 0.0 };
                if secs > 0.0 && secs < 0.250_01 {
                    return Some(format!("1/{}", (0.5 + 1.0 / secs).floor() as i64));
                }
                let s = format!("{:.1}", secs);
                return Some(s.strip_suffix(".0").map(str::to_string).unwrap_or(s));
            }
        }
        // InteropIndex — ExifTool PrintConv.
        ("InteropIFD", 0x0001) => {
            if let Value::String(s) = value {
                let s = s.trim_end_matches('\0').trim();
                return Some(
                    match s {
                        "R98" => "R98 - DCF basic file (sRGB)",
                        "R03" => "R03 - DCF option file (Adobe RGB)",
                        "THM" => "THM - DCF thumbnail file",
                        other => other,
                    }
                    .to_string(),
                );
            }
        }
        // InteropVersion — raw 4-char string like ExifVersion (e.g. "0100").
        ("InteropIFD", 0x0002) => {
            if let Value::Undefined(ref data) = value {
                let s = crate::encoding::decode_utf8_or_latin1(data);
                let s = s.trim_end_matches('\0');
                if !s.is_empty() {
                    return Some(s.to_string());
                }
            }
        }
        // UserComment — 8-byte charset ID prefix, then the comment text.
        ("ExifIFD", 0x9286) => {
            if let Value::Undefined(ref data) = value {
                if data.len() >= 8 {
                    let (charset, body) = data.split_at(8);
                    let text = if charset.starts_with(b"UNICODE") {
                        let u16s: Vec<u16> = body
                            .chunks_exact(2)
                            .map(|c| u16::from_be_bytes([c[0], c[1]]))
                            .collect();
                        String::from_utf16_lossy(&u16s)
                    } else {
                        crate::encoding::decode_utf8_or_latin1(body).to_string()
                    };
                    return Some(text.trim_end_matches(['\0', ' ']).to_string());
                }
            }
        }
        // FocalLength - format as "X.Y mm"
        ("ExifIFD", 0x920A) => {
            if let Some(v) = value.as_f64() {
                return Some(format!("{:.1} mm", v));
            }
        }
        // GPS Latitude/Longitude Ref
        ("GPS", 0x0001) | ("GPS", 0x0003) => {
            if let Value::String(s) = value {
                return Some(
                    match s.as_str() {
                        "N" => "North".to_string(),
                        "S" => "South".to_string(),
                        "E" => "East".to_string(),
                        "W" => "West".to_string(),
                        other => format!("Unknown ({})", other.trim()),
                    },
                );
            }
        }
        // GPS Altitude Ref
        ("GPS", 0x0005) => {
            if let Some(v) = value.as_u64() {
                return Some(
                    match v {
                        0 => "Above Sea Level",
                        1 => "Below Sea Level",
                        _ => return None,
                    }
                    .to_string(),
                );
            }
        }
        // ExposureMode
        ("ExifIFD", 0xA402) => {
            if let Some(v) = value.as_u64() {
                return Some(
                    match v {
                        0 => "Auto",
                        1 => "Manual",
                        2 => "Auto bracket",
                        _ => return None,
                    }
                    .to_string(),
                );
            }
        }
        // WhiteBalance
        ("ExifIFD", 0xA403) => {
            if let Some(v) = value.as_u64() {
                return Some(
                    match v {
                        0 => "Auto",
                        1 => "Manual",
                        _ => return None,
                    }
                    .to_string(),
                );
            }
        }
        // SceneCaptureType
        ("ExifIFD", 0xA406) => {
            if let Some(v) = value.as_u64() {
                return Some(
                    match v {
                        0 => "Standard",
                        1 => "Landscape",
                        2 => "Portrait",
                        3 => "Night",
                        _ => return None,
                    }
                    .to_string(),
                );
            }
        }
        // Contrast
        ("ExifIFD", 0xA408) => {
            if let Some(v) = value.as_u64() {
                return Some(
                    match v {
                        0 => "Normal",
                        1 => "Low",
                        2 => "High",
                        _ => return None,
                    }
                    .to_string(),
                );
            }
        }
        // Saturation
        ("ExifIFD", 0xA409) => {
            if let Some(v) = value.as_u64() {
                return Some(
                    match v {
                        0 => "Normal",
                        1 => "Low",
                        2 => "High",
                        _ => return None,
                    }
                    .to_string(),
                );
            }
        }
        // Sharpness
        ("ExifIFD", 0xA40A) => {
            if let Some(v) = value.as_u64() {
                return Some(
                    match v {
                        0 => "Normal",
                        1 => "Soft",
                        2 => "Hard",
                        _ => return None,
                    }
                    .to_string(),
                );
            }
        }
        // CustomRendered
        ("ExifIFD", 0xA401) => {
            if let Some(v) = value.as_u64() {
                return Some(
                    match v {
                        0 => "Normal",
                        1 => "Custom",
                        _ => return None,
                    }
                    .to_string(),
                );
            }
        }
        // SensingMethod
        ("ExifIFD", 0xA217) => {
            if let Some(v) = value.as_u64() {
                return Some(
                    match v {
                        1 => "Not defined",
                        2 => "One-chip color area",
                        3 => "Two-chip color area",
                        4 => "Three-chip color area",
                        5 => "Color sequential area",
                        7 => "Trilinear",
                        8 => "Color sequential linear",
                        _ => return None,
                    }
                    .to_string(),
                );
            }
        }
        // FileSource
        ("ExifIFD", 0xA300) => {
            if let Value::Undefined(ref data) = value {
                // Sigma incorrectly writes this as 4 bytes "\3\0\0\0".
                if data == b"\x03\x00\x00\x00" {
                    return Some("Sigma Digital Camera".to_string());
                }
                if !data.is_empty() {
                    return Some(
                        match data[0] {
                            1 => "Film Scanner".to_string(),
                            2 => "Reflection Print Scanner".to_string(),
                            3 => "Digital Camera".to_string(),
                            n => format!("Unknown ({})", n),
                        },
                    );
                }
            }
        }
        // SceneType: 1 => Directly photographed, else Unknown (N).
        ("ExifIFD", 0xA301) => {
            if let Value::Undefined(ref data) = value {
                if !data.is_empty() {
                    return Some(if data[0] == 1 {
                        "Directly photographed".to_string()
                    } else {
                        format!("Unknown ({})", data[0])
                    });
                }
            }
        }
        // YCbCrSubSampling (0x0212): int16u[2] keyed string enum.
        (_, 0x0212) => {
            let s = value.to_display_string();
            let label = match s.trim() {
                "1 1" => Some("YCbCr4:4:4 (1 1)"),
                "2 1" => Some("YCbCr4:2:2 (2 1)"),
                "2 2" => Some("YCbCr4:2:0 (2 2)"),
                "4 1" => Some("YCbCr4:1:1 (4 1)"),
                "4 2" => Some("YCbCr4:1:0 (4 2)"),
                "1 2" => Some("YCbCr4:4:0 (1 2)"),
                "1 4" => Some("YCbCr4:4:1 (1 4)"),
                "2 4" => Some("YCbCr4:2:1 (2 4)"),
                _ => None,
            };
            if let Some(l) = label {
                return Some(l.to_string());
            }
        }
        // CFAPattern (0xA302) / CFAPattern2 (0x828E): decode the colour-filter grid.
        (_, 0xA302) | (_, 0x828E) => {
            if let Value::Undefined(ref b) = value {
                if let Some(s) = print_cfa_pattern(b) {
                    return Some(s);
                }
            }
        }
        // CFAPlaneColor (0xc616): map each plane index to a colour name, join with ",".
        (_, 0xC616) => {
            let cols = ["Red", "Green", "Blue", "Cyan", "Magenta", "Yellow", "White"];
            let s = value.to_display_string();
            let mapped: Vec<String> = s
                .split_whitespace()
                .map(|n| {
                    n.parse::<usize>()
                        .ok()
                        .and_then(|i| cols.get(i))
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| format!("Unknown({})", n))
                })
                .collect();
            if !mapped.is_empty() {
                return Some(mapped.join(","));
            }
        }
        // CR2CFAPattern (0xc5e0): ValueConv + PrintConv collapsed to the colour grid.
        (_, 0xC5E0) => {
            if let Some(v) = value.as_u64() {
                return match v {
                    1 => Some("[Red,Green][Green,Blue]"),
                    2 => Some("[Blue,Green][Green,Red]"),
                    3 => Some("[Green,Blue][Red,Green]"),
                    4 => Some("[Green,Red][Blue,Green]"),
                    _ => None,
                }
                .map(str::to_string);
            }
        }
        // ColorMap (0x0140): ExifTool marks it Binary => 1 (shown as a placeholder).
        (_, 0x0140) => {
            let bytes = match value {
                Value::List(items) => items.len() * 2,
                Value::Binary(b) | Value::Undefined(b) => b.len(),
                _ => 0,
            };
            if bytes > 0 {
                return Some(format!(
                    "(Binary data {} bytes, use -b option to extract)",
                    bytes
                ));
            }
        }
        // LensInfo (0xA432) / DNGLensInfo (0xC630): Exif::PrintLensInfo.
        (_, 0xA432) | (_, 0xC630) => {
            let s = value.to_display_string();
            if let Some(formatted) = print_lens_info(&s) {
                return Some(formatted);
            }
        }
        // ExposureCompensation / ExposureBiasValue (Exif::PrintFraction)
        (_, 0x9204) => {
            if let Some(v) = value.as_f64() {
                return Some(print_fraction(v));
            }
        }
        // Compression (IFD0/IFD1)
        (_, 0x0103) => {
            if let Some(v) = value.as_u64() {
                return Some(
                    match v {
                        1 => "Uncompressed",
                        2 => "CCITT 1D",
                        3 => "T4/Group 3 Fax",
                        4 => "T6/Group 4 Fax",
                        5 => "LZW",
                        6 => "JPEG (old-style)",
                        7 => "JPEG",
                        8 => "Adobe Deflate",
                        9 => "JBIG B&W",
                        10 => "JBIG Color",
                        99 => "JPEG",
                        262 => "Kodak 262",
                        32766 => "NeXt or Sony ARW Compressed 2",
                        32767 => "Sony ARW Compressed",
                        32769 => "Packed RAW",
                        32770 => "Samsung SRW Compressed",
                        32771 => "CCIRLEW",
                        32772 => "Samsung SRW Compressed 2",
                        32773 => "PackBits",
                        32809 => "Thunderscan",
                        32867 => "Kodak KDC Compressed",
                        32895 => "IT8CTPAD",
                        32896 => "IT8LW",
                        32897 => "IT8MP",
                        32898 => "IT8BL",
                        32908 => "PixarFilm",
                        32909 => "PixarLog",
                        32946 => "Deflate",
                        32947 => "DCS",
                        33003 => "Aperio JPEG 2000 YCbCr",
                        33005 => "Aperio JPEG 2000 RGB",
                        34661 => "JBIG",
                        34676 => "SGILog",
                        34677 => "SGILog24",
                        34712 => "JPEG 2000",
                        34713 => "Nikon NEF Compressed",
                        34715 => "JBIG2 TIFF FX",
                        34718 => "Microsoft Document Imaging (MDI) Binary Level Codec",
                        34719 => "Microsoft Document Imaging (MDI) Progressive Transform Codec",
                        34720 => "Microsoft Document Imaging (MDI) Vector",
                        34887 => "ESRI Lerc",
                        34892 => "Lossy JPEG",
                        34925 => "LZMA2",
                        34926 => "Zstd (old)",
                        34927 => "WebP (old)",
                        34933 => "PNG",
                        34934 => "JPEG XR",
                        50000 => "Zstd",
                        50001 => "WebP",
                        50002 => "JPEG XL (old)",
                        52546 => "JPEG XL",
                        65000 => "Kodak DCR Compressed",
                        65535 => "Pentax PEF Compressed",
                        _ => return None,
                    }
                    .to_string(),
                );
            }
        }
        // PhotometricInterpretation
        (_, 0x0106) => {
            if let Some(v) = value.as_u64() {
                return Some(
                    match v {
                        0 => "WhiteIsZero",
                        1 => "BlackIsZero",
                        2 => "RGB",
                        3 => "RGB Palette",
                        4 => "Transparency Mask",
                        5 => "CMYK",
                        6 => "YCbCr",
                        8 => "CIELab",
                        _ => return None,
                    }
                    .to_string(),
                );
            }
        }
        // ExifVersion / FlashpixVersion — ExifTool shows the raw 4-char string
        // (e.g. "0221"), only stripping trailing nulls; no decimal point.
        ("ExifIFD", 0x9000) | ("ExifIFD", 0xA000) => {
            if let Value::Undefined(ref data) = value {
                let s = crate::encoding::decode_utf8_or_latin1(data);
                let s = s.trim_end_matches('\0');
                if !s.is_empty() {
                    return Some(s.to_string());
                }
            }
        }
        // ComponentsConfiguration
        ("ExifIFD", 0x9101) => {
            if let Value::Undefined(ref data) = value {
                let components: Vec<&str> = data
                    .iter()
                    .map(|&b| match b {
                        0 => "-",
                        1 => "Y",
                        2 => "Cb",
                        3 => "Cr",
                        4 => "R",
                        5 => "G",
                        6 => "B",
                        _ => "?",
                    })
                    .collect();
                return Some(components.join(", "));
            }
        }
        // FocalLengthIn35mmFormat
        ("ExifIFD", 0xA405) => {
            if let Some(v) = value.as_u64() {
                if v > 0 {
                    return Some(format!("{} mm", v));
                }
            }
        }
        // ISO
        ("ExifIFD", 0x8827) => {
            // Keep as-is (number)
        }
        // ModelTransform (0x85D8) and PixelScale (0x830E): space-separated doubles
        // with Perl-style %.15g precision
        (_, 0x85D8) | (_, 0x830E) => {
            return Some(format_geotiff_doubles(value));
        }
        _ => {}
    }
    None
}

/// Format a double or list of doubles with Perl-style %.15g precision and space separator.
/// This matches ExifTool's output format for GeoTiff coordinate arrays.
/// Port of Exif.pm `PrintFraction`: render a signed value as +N, +N/2, +N/3,
/// or %+.3g, with a round-off guard.
pub fn print_fraction(mut val: f64) -> String {
    val *= 1.00001; // avoid round-off errors
    if val == 0.0 {
        "0".to_string()
    } else if (val.trunc() / val) > 0.999 {
        format!("{:+}", val.trunc() as i64)
    } else if (((val * 2.0).trunc()) / (val * 2.0)) > 0.999 {
        format!("{:+}/2", (val * 2.0).trunc() as i64)
    } else if (((val * 3.0).trunc()) / (val * 3.0)) > 0.999 {
        format!("{:+}/3", (val * 3.0).trunc() as i64)
    } else {
        // Perl sprintf("%+.3g", $val) — 3 significant digits, forced sign.
        let s = crate::value::format_g_prec(val, 3);
        if val > 0.0 {
            format!("+{}", s)
        } else {
            s
        }
    }
}

/// Decode an EXIF CFAPattern undef block to "[col,col][...]" (Exif PrintCFAPattern).
/// Layout: int16 horizontal repeat, int16 vertical repeat, then h*v colour indices.
fn print_cfa_pattern(b: &[u8]) -> Option<String> {
    if b.len() < 4 {
        return None;
    }
    // Try big-endian then little-endian for the 2x int16 header.
    for be in [true, false] {
        let (w, h) = if be {
            (
                u16::from_be_bytes([b[0], b[1]]) as usize,
                u16::from_be_bytes([b[2], b[3]]) as usize,
            )
        } else {
            (
                u16::from_le_bytes([b[0], b[1]]) as usize,
                u16::from_le_bytes([b[2], b[3]]) as usize,
            )
        };
        if w == 0 || h == 0 || 4 + w * h > b.len() {
            continue;
        }
        let cols = ["Red", "Green", "Blue", "Cyan", "Magenta", "Yellow", "White"];
        let mut out = String::from("[");
        for i in 0..(w * h) {
            let idx = b[4 + i] as usize;
            out.push_str(cols.get(idx).copied().unwrap_or("Unknown"));
            if i + 1 == w * h {
                break;
            }
            // Each row holds `h` (the 2nd value) entries in ExifTool's loop.
            if (i + 1) % h == 0 {
                out.push_str("][");
            } else {
                out.push(',');
            }
        }
        out.push(']');
        return Some(out);
    }
    None
}

/// Port of Exif.pm `PrintLensInfo`: 4 values (min/max focal, min/max aperture)
/// → "min-max mm f/min-max". Returns None if the value isn't 4 numbers.
pub fn print_lens_info(val: &str) -> Option<String> {
    let vals: Vec<&str> = val.split_whitespace().collect();
    if vals.len() != 4 {
        return None;
    }
    // Each value must be a float, "inf"/"undef" (→ "?").
    let norm: Vec<String> = vals
        .iter()
        .map(|&v| {
            if v == "inf" || v == "undef" {
                "?".to_string()
            } else {
                v.to_string()
            }
        })
        .collect();
    if !norm
        .iter()
        .all(|v| v == "?" || v.parse::<f64>().is_ok())
    {
        return None;
    }
    let mut out = norm[0].clone();
    if norm[1] != "0" && norm[1] != norm[0] {
        out.push_str(&format!("-{}", norm[1]));
    }
    out.push_str(&format!("mm f/{}", norm[2]));
    if norm[3] != "0" && norm[3] != norm[2] {
        out.push_str(&format!("-{}", norm[3]));
    }
    Some(out)
}

pub fn format_geotiff_doubles(value: &Value) -> String {
    match value {
        Value::F64(v) => crate::value::format_g15(*v),
        Value::F32(v) => crate::value::format_g15(*v as f64),
        Value::List(items) => items
            .iter()
            .map(|item| match item {
                Value::F64(v) => crate::value::format_g15(*v),
                Value::F32(v) => crate::value::format_g15(*v as f64),
                _ => item.to_display_string(),
            })
            .collect::<Vec<_>>()
            .join(" "),
        _ => value.to_display_string(),
    }
}

// ============================================================================
// Tag tables
// ============================================================================

static IFD0_TAGS: &[TagInfo] = &[
    TagInfo {
        tag_id: 0x00FE,
        name: "SubfileType",
        description: "Subfile Type",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x00FF,
        name: "OldSubfileType",
        description: "Old Subfile Type",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x0100,
        name: "ImageWidth",
        description: "Image Width",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x0101,
        name: "ImageHeight",
        description: "Image Height",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x0102,
        name: "BitsPerSample",
        description: "Bits Per Sample",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x0103,
        name: "Compression",
        description: "Compression",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x0106,
        name: "PhotometricInterpretation",
        description: "Photometric Interpretation",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x0107,
        name: "Thresholding",
        description: "Thresholding",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x010A,
        name: "FillOrder",
        description: "Fill Order",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x010D,
        name: "DocumentName",
        description: "Document Name",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x010E,
        name: "ImageDescription",
        description: "Image Description",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x010F,
        name: "Make",
        description: "Camera Make",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0x0110,
        name: "Model",
        description: "Camera Model",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0x0111,
        name: "StripOffsets",
        description: "Strip Offsets",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x0112,
        name: "Orientation",
        description: "Orientation",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x0115,
        name: "SamplesPerPixel",
        description: "Samples Per Pixel",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x0116,
        name: "RowsPerStrip",
        description: "Rows Per Strip",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x0117,
        name: "StripByteCounts",
        description: "Strip Byte Counts",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x0118,
        name: "MinSampleValue",
        description: "Min Sample Value",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x0119,
        name: "MaxSampleValue",
        description: "Max Sample Value",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x011A,
        name: "XResolution",
        description: "X Resolution",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x011B,
        name: "YResolution",
        description: "Y Resolution",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x011C,
        name: "PlanarConfiguration",
        description: "Planar Configuration",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x011D,
        name: "PageName",
        description: "Page Name",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x0128,
        name: "ResolutionUnit",
        description: "Resolution Unit",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x0129,
        name: "PageNumber",
        description: "Page Number",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x012D,
        name: "TransferFunction",
        description: "Transfer Function",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x0131,
        name: "Software",
        description: "Software",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x0132,
        name: "ModifyDate",
        description: "Modify Date",
        family2: "Time",
    },
    TagInfo {
        tag_id: 0x013B,
        name: "Artist",
        description: "Artist",
        family2: "Author",
    },
    TagInfo {
        tag_id: 0x013C,
        name: "HostComputer",
        description: "Host Computer",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x013D,
        name: "Predictor",
        description: "Predictor",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x013E,
        name: "WhitePoint",
        description: "White Point",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x013F,
        name: "PrimaryChromaticities",
        description: "Primary Chromaticities",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x0140,
        name: "ColorMap",
        description: "Color Map",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x0141,
        name: "HalftoneHints",
        description: "Halftone Hints",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x0142,
        name: "TileWidth",
        description: "Tile Width",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x0143,
        name: "TileLength",
        description: "Tile Length",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x0153,
        name: "SampleFormat",
        description: "Sample Format",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x0201,
        name: "ThumbnailOffset",
        description: "Thumbnail Offset",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x0202,
        name: "ThumbnailLength",
        description: "Thumbnail Length",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x0211,
        name: "YCbCrCoefficients",
        description: "YCbCr Coefficients",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x0212,
        name: "YCbCrSubSampling",
        description: "YCbCr Sub Sampling",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x0213,
        name: "YCbCrPositioning",
        description: "YCbCr Positioning",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x0214,
        name: "ReferenceBlackWhite",
        description: "Reference Black White",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x02BC,
        name: "ApplicationNotes",
        description: "Application Notes (XMP)",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x4746,
        name: "Rating",
        description: "Rating",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x4749,
        name: "RatingPercent",
        description: "Rating Percent",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x828D,
        name: "CFARepeatPatternDim",
        description: "CFA Repeat Pattern Dim",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x828E,
        name: "CFAPattern2",
        description: "CFA Pattern 2",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x8298,
        name: "Copyright",
        description: "Copyright",
        family2: "Author",
    },
    TagInfo {
        tag_id: 0x83BB,
        name: "IPTC-NAA",
        description: "IPTC-NAA",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x8649,
        name: "PhotoshopSettings",
        description: "Photoshop Settings",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x8769,
        name: "ExifOffset",
        description: "Exif IFD Pointer",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x8773,
        name: "ICC_Profile",
        description: "ICC Profile",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x8825,
        name: "GPSInfo",
        description: "GPS Info IFD Pointer",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x830E,
        name: "PixelScale",
        description: "Pixel Scale",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x85D8,
        name: "ModelTransform",
        description: "Model Transform",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x8680,
        name: "IntergraphMatrix",
        description: "Intergraph Matrix",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x9216,
        name: "TIFF-EPStandardID",
        description: "TIFF-EP Standard ID",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x87AF,
        name: "GeoTiffDirectory",
        description: "GeoTiff Directory",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x87B0,
        name: "GeoTiffDoubleParams",
        description: "GeoTiff Double Params",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x87B1,
        name: "GeoTiffAsciiParams",
        description: "GeoTiff ASCII Params",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0xC612,
        name: "DNGVersion",
        description: "DNG Version",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0xC613,
        name: "DNGBackwardVersion",
        description: "DNG Backward Version",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0xC614,
        name: "UniqueCameraModel",
        description: "Unique Camera Model",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xC621,
        name: "ColorMatrix1",
        description: "Color Matrix 1",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0xC622,
        name: "ColorMatrix2",
        description: "Color Matrix 2",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0xC628,
        name: "AsShotNeutral",
        description: "As Shot Neutral",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xC623,
        name: "CameraCalibration1",
        description: "Camera Calibration 1",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xC624,
        name: "CameraCalibration2",
        description: "Camera Calibration 2",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xC62F,
        name: "CameraSerialNumber",
        description: "Camera Serial Number",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xC630,
        name: "DNGLensInfo",
        description: "DNG Lens Info",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xC65A,
        name: "CalibrationIlluminant1",
        description: "Calibration Illuminant 1",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xC65B,
        name: "CalibrationIlluminant2",
        description: "Calibration Illuminant 2",
        family2: "Camera",
    },
];

static EXIF_IFD_TAGS: &[TagInfo] = &[
    TagInfo {
        tag_id: 0x829A,
        name: "ExposureTime",
        description: "Exposure Time",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0x829D,
        name: "FNumber",
        description: "F Number",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0x8822,
        name: "ExposureProgram",
        description: "Exposure Program",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0x8827,
        name: "ISO",
        description: "ISO Speed",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0x8830,
        name: "SensitivityType",
        description: "Sensitivity Type",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0x9000,
        name: "ExifVersion",
        description: "Exif Version",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x9003,
        name: "DateTimeOriginal",
        description: "Date/Time Original",
        family2: "Time",
    },
    TagInfo {
        tag_id: 0x9004,
        name: "CreateDate",
        description: "Create Date",
        family2: "Time",
    },
    TagInfo {
        tag_id: 0x9010,
        name: "OffsetTime",
        description: "Offset Time",
        family2: "Time",
    },
    TagInfo {
        tag_id: 0x9011,
        name: "OffsetTimeOriginal",
        description: "Offset Time Original",
        family2: "Time",
    },
    TagInfo {
        tag_id: 0x9012,
        name: "OffsetTimeDigitized",
        description: "Offset Time Digitized",
        family2: "Time",
    },
    TagInfo {
        tag_id: 0x9101,
        name: "ComponentsConfiguration",
        description: "Components Configuration",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x9102,
        name: "CompressedBitsPerPixel",
        description: "Compressed Bits Per Pixel",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x9201,
        name: "ShutterSpeedValue",
        description: "Shutter Speed Value",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0x9202,
        name: "ApertureValue",
        description: "Aperture Value",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0x9203,
        name: "BrightnessValue",
        description: "Brightness Value",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0x9204,
        name: "ExposureCompensation",
        description: "Exposure Compensation",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0x9205,
        name: "MaxApertureValue",
        description: "Max Aperture Value",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0x9207,
        name: "MeteringMode",
        description: "Metering Mode",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0x9208,
        name: "LightSource",
        description: "Light Source",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0x9209,
        name: "Flash",
        description: "Flash",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0x920A,
        name: "FocalLength",
        description: "Focal Length",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0x920D,
        name: "Noise",
        description: "Noise",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0x927C,
        name: "MakerNote",
        description: "Maker Note",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0x9286,
        name: "UserComment",
        description: "User Comment",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x9290,
        name: "SubSecTime",
        description: "Sub Sec Time",
        family2: "Time",
    },
    TagInfo {
        tag_id: 0x9291,
        name: "SubSecTimeOriginal",
        description: "Sub Sec Time Original",
        family2: "Time",
    },
    TagInfo {
        tag_id: 0x9292,
        name: "SubSecTimeDigitized",
        description: "Sub Sec Time Digitized",
        family2: "Time",
    },
    TagInfo {
        tag_id: 0xA000,
        name: "FlashpixVersion",
        description: "Flashpix Version",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0xA001,
        name: "ColorSpace",
        description: "Color Space",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0xA002,
        name: "ExifImageWidth",
        description: "Exif Image Width",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0xA003,
        name: "ExifImageHeight",
        description: "Exif Image Height",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0xA005,
        name: "InteropOffset",
        description: "Interoperability IFD Pointer",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0xA20E,
        name: "FocalPlaneXResolution",
        description: "Focal Plane X Resolution",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xA20F,
        name: "FocalPlaneYResolution",
        description: "Focal Plane Y Resolution",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xA210,
        name: "FocalPlaneResolutionUnit",
        description: "Focal Plane Resolution Unit",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xA217,
        name: "SensingMethod",
        description: "Sensing Method",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xA300,
        name: "FileSource",
        description: "File Source",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xA301,
        name: "SceneType",
        description: "Scene Type",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xA401,
        name: "CustomRendered",
        description: "Custom Rendered",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xA402,
        name: "ExposureMode",
        description: "Exposure Mode",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xA403,
        name: "WhiteBalance",
        description: "White Balance",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xA404,
        name: "DigitalZoomRatio",
        description: "Digital Zoom Ratio",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xA405,
        name: "FocalLengthIn35mmFormat",
        description: "Focal Length In 35mm Format",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xA406,
        name: "SceneCaptureType",
        description: "Scene Capture Type",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xA408,
        name: "Contrast",
        description: "Contrast",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xA409,
        name: "Saturation",
        description: "Saturation",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xA40A,
        name: "Sharpness",
        description: "Sharpness",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xA420,
        name: "ImageUniqueID",
        description: "Image Unique ID",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0xA430,
        name: "OwnerName",
        description: "Owner Name",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xA431,
        name: "SerialNumber",
        description: "Serial Number",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xA432,
        name: "LensInfo",
        description: "Lens Info",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xA433,
        name: "LensMake",
        description: "Lens Make",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xA434,
        name: "LensModel",
        description: "Lens Model",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xA435,
        name: "LensSerialNumber",
        description: "Lens Serial Number",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xA460,
        name: "CompositeImage",
        description: "Composite Image",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0xA461,
        name: "SourceImageNumberOfCompositeImage",
        description: "Source Image Number",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0xA462,
        name: "SourceExposureTimesOfCompositeImage",
        description: "Source Exposure Times",
        family2: "Image",
    },
    // Additional ExifIFD tags
    TagInfo {
        tag_id: 0x9206,
        name: "SubjectDistance",
        description: "Subject Distance",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0x9214,
        name: "SubjectArea",
        description: "Subject Area",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xA215,
        name: "ExposureIndex",
        description: "Exposure Index",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xA302,
        name: "CFAPattern",
        description: "CFA Pattern",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xA407,
        name: "GainControl",
        description: "Gain Control",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xA40B,
        name: "DeviceSettingDescription",
        description: "Device Setting Description",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xA40C,
        name: "SubjectDistanceRange",
        description: "Subject Distance Range",
        family2: "Camera",
    },
    TagInfo {
        tag_id: 0xA500,
        name: "Gamma",
        description: "Gamma",
        family2: "Image",
    },
    // Print Image Matching
    TagInfo {
        tag_id: 0xC4A5,
        name: "PrintImageMatching",
        description: "Print Image Matching",
        family2: "Image",
    },
];

static GPS_TAGS: &[TagInfo] = &[
    TagInfo {
        tag_id: 0x0000,
        name: "GPSVersionID",
        description: "GPS Version ID",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x0001,
        name: "GPSLatitudeRef",
        description: "GPS Latitude Ref",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x0002,
        name: "GPSLatitude",
        description: "GPS Latitude",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x0003,
        name: "GPSLongitudeRef",
        description: "GPS Longitude Ref",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x0004,
        name: "GPSLongitude",
        description: "GPS Longitude",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x0005,
        name: "GPSAltitudeRef",
        description: "GPS Altitude Ref",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x0006,
        name: "GPSAltitude",
        description: "GPS Altitude",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x0007,
        name: "GPSTimeStamp",
        description: "GPS Time Stamp",
        family2: "Time",
    },
    TagInfo {
        tag_id: 0x0008,
        name: "GPSSatellites",
        description: "GPS Satellites",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x0009,
        name: "GPSStatus",
        description: "GPS Status",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x000A,
        name: "GPSMeasureMode",
        description: "GPS Measure Mode",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x000B,
        name: "GPSDOP",
        description: "GPS Dilution Of Precision",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x000C,
        name: "GPSSpeedRef",
        description: "GPS Speed Ref",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x000D,
        name: "GPSSpeed",
        description: "GPS Speed",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x000E,
        name: "GPSTrackRef",
        description: "GPS Track Ref",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x000F,
        name: "GPSTrack",
        description: "GPS Track",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x0010,
        name: "GPSImgDirectionRef",
        description: "GPS Img Direction Ref",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x0011,
        name: "GPSImgDirection",
        description: "GPS Img Direction",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x0012,
        name: "GPSMapDatum",
        description: "GPS Map Datum",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x0013,
        name: "GPSDestLatitudeRef",
        description: "GPS Dest Latitude Ref",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x0014,
        name: "GPSDestLatitude",
        description: "GPS Dest Latitude",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x0015,
        name: "GPSDestLongitudeRef",
        description: "GPS Dest Longitude Ref",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x0016,
        name: "GPSDestLongitude",
        description: "GPS Dest Longitude",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x0017,
        name: "GPSDestBearingRef",
        description: "GPS Dest Bearing Ref",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x0018,
        name: "GPSDestBearing",
        description: "GPS Dest Bearing",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x0019,
        name: "GPSDestDistanceRef",
        description: "GPS Dest Distance Ref",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x001A,
        name: "GPSDestDistance",
        description: "GPS Dest Distance",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x001B,
        name: "GPSProcessingMethod",
        description: "GPS Processing Method",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x001C,
        name: "GPSAreaInformation",
        description: "GPS Area Information",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x001D,
        name: "GPSDateStamp",
        description: "GPS Date Stamp",
        family2: "Time",
    },
    TagInfo {
        tag_id: 0x001E,
        name: "GPSDifferential",
        description: "GPS Differential",
        family2: "Location",
    },
    TagInfo {
        tag_id: 0x001F,
        name: "GPSHPositioningError",
        description: "GPS Horizontal Positioning Error",
        family2: "Location",
    },
];

static INTEROP_TAGS: &[TagInfo] = &[
    TagInfo {
        tag_id: 0x0001,
        name: "InteropIndex",
        description: "Interoperability Index",
        family2: "Image",
    },
    TagInfo {
        tag_id: 0x0002,
        name: "InteropVersion",
        description: "Interoperability Version",
        family2: "Image",
    },
];
