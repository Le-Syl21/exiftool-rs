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
        "IFD0" | "IFD1" => &IFD0_TAGS[..],
        "ExifIFD" => &EXIF_IFD_TAGS[..],
        "GPS" => &GPS_TAGS[..],
        "InteropIFD" => &INTEROP_TAGS[..],
        _ => &IFD0_TAGS[..],
    };

    table.iter().find(|t| t.tag_id == tag_id)
}

/// Lookup tag name from generated tables (fallback for unknown tags).
pub fn lookup_generated(tag_id: u16) -> Option<(&'static str, &'static str)> {
    super::generated::GENERATED_EXIF_TAGS.iter()
        .find(|t| t.tag_id == tag_id)
        .map(|t| (t.name, t.description))
}

/// Apply print conversion for known tags.
pub fn print_conv(ifd: &str, tag_id: u16, value: &Value) -> Option<String> {
    match (ifd, tag_id) {
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
        // Flash
        ("ExifIFD", 0x9209) => {
            if let Some(v) = value.as_u64() {
                let fired = if v & 1 != 0 { "Fired" } else { "No Flash" };
                return Some(fired.to_string());
            }
        }
        // ColorSpace
        ("ExifIFD", 0xA001) => {
            if let Some(v) = value.as_u64() {
                return Some(
                    match v {
                        1 => "sRGB",
                        2 => "Adobe RGB",
                        0xFFFF => "Uncalibrated",
                        _ => return None,
                    }
                    .to_string(),
                );
            }
        }
        // ExposureTime - format as "1/X s"
        ("ExifIFD", 0x829A) => {
            if let Value::URational(n, d) = value {
                if *d != 0 {
                    if *n == 1 {
                        return Some(format!("1/{} s", d));
                    } else {
                        let secs = *n as f64 / *d as f64;
                        if secs >= 1.0 {
                            return Some(format!("{} s", secs));
                        } else {
                            return Some(format!("1/{} s", (*d as f64 / *n as f64).round() as u64));
                        }
                    }
                }
            }
        }
        // FNumber - format as "f/X.Y"
        ("ExifIFD", 0x829D) => {
            if let Some(v) = value.as_f64() {
                return Some(format!("f/{:.1}", v));
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
                        "N" => "North",
                        "S" => "South",
                        "E" => "East",
                        "W" => "West",
                        _ => return None,
                    }
                    .to_string(),
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
                return Some(match v {
                    0 => "Auto", 1 => "Manual", 2 => "Auto bracket",
                    _ => return None,
                }.to_string());
            }
        }
        // WhiteBalance
        ("ExifIFD", 0xA403) => {
            if let Some(v) = value.as_u64() {
                return Some(match v {
                    0 => "Auto", 1 => "Manual",
                    _ => return None,
                }.to_string());
            }
        }
        // SceneCaptureType
        ("ExifIFD", 0xA406) => {
            if let Some(v) = value.as_u64() {
                return Some(match v {
                    0 => "Standard", 1 => "Landscape", 2 => "Portrait", 3 => "Night",
                    _ => return None,
                }.to_string());
            }
        }
        // Contrast
        ("ExifIFD", 0xA408) => {
            if let Some(v) = value.as_u64() {
                return Some(match v {
                    0 => "Normal", 1 => "Low", 2 => "High",
                    _ => return None,
                }.to_string());
            }
        }
        // Saturation
        ("ExifIFD", 0xA409) => {
            if let Some(v) = value.as_u64() {
                return Some(match v {
                    0 => "Normal", 1 => "Low", 2 => "High",
                    _ => return None,
                }.to_string());
            }
        }
        // Sharpness
        ("ExifIFD", 0xA40A) => {
            if let Some(v) = value.as_u64() {
                return Some(match v {
                    0 => "Normal", 1 => "Soft", 2 => "Hard",
                    _ => return None,
                }.to_string());
            }
        }
        // CustomRendered
        ("ExifIFD", 0xA401) => {
            if let Some(v) = value.as_u64() {
                return Some(match v {
                    0 => "Normal", 1 => "Custom",
                    _ => return None,
                }.to_string());
            }
        }
        // SensingMethod
        ("ExifIFD", 0xA217) => {
            if let Some(v) = value.as_u64() {
                return Some(match v {
                    1 => "Not defined", 2 => "One-chip color area",
                    3 => "Two-chip color area", 4 => "Three-chip color area",
                    5 => "Color sequential area", 7 => "Trilinear",
                    8 => "Color sequential linear",
                    _ => return None,
                }.to_string());
            }
        }
        // FileSource
        ("ExifIFD", 0xA300) => {
            if let Value::Undefined(ref data) = value {
                if data.len() >= 1 {
                    return Some(match data[0] {
                        1 => "Film Scanner", 2 => "Reflection Print Scanner",
                        3 => "Digital Camera",
                        _ => return None,
                    }.to_string());
                }
            }
        }
        // SceneType
        ("ExifIFD", 0xA301) => {
            if let Value::Undefined(ref data) = value {
                if data.len() >= 1 && data[0] == 1 {
                    return Some("Directly photographed".to_string());
                }
            }
        }
        // Compression (IFD0/IFD1)
        (_, 0x0103) => {
            if let Some(v) = value.as_u64() {
                return Some(match v {
                    1 => "Uncompressed", 2 => "CCITT 1D", 3 => "T4/Group 3 Fax",
                    4 => "T6/Group 4 Fax", 5 => "LZW", 6 => "JPEG (old-style)",
                    7 => "JPEG", 8 => "Adobe Deflate", 32773 => "PackBits",
                    34712 => "JPEG 2000",
                    _ => return None,
                }.to_string());
            }
        }
        // PhotometricInterpretation
        (_, 0x0106) => {
            if let Some(v) = value.as_u64() {
                return Some(match v {
                    0 => "WhiteIsZero", 1 => "BlackIsZero", 2 => "RGB",
                    3 => "RGB Palette", 4 => "Transparency Mask",
                    5 => "CMYK", 6 => "YCbCr", 8 => "CIELab",
                    _ => return None,
                }.to_string());
            }
        }
        // ExifVersion / FlashpixVersion
        ("ExifIFD", 0x9000) | ("ExifIFD", 0xA000) => {
            if let Value::Undefined(ref data) = value {
                let s = String::from_utf8_lossy(data);
                if s.len() == 4 {
                    return Some(format!("{}.{}", &s[0..2], &s[2..4]));
                }
            }
        }
        // ComponentsConfiguration
        ("ExifIFD", 0x9101) => {
            if let Value::Undefined(ref data) = value {
                let components: Vec<&str> = data.iter().map(|&b| match b {
                    0 => "-", 1 => "Y", 2 => "Cb", 3 => "Cr", 4 => "R", 5 => "G", 6 => "B",
                    _ => "?",
                }).collect();
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

/// Format a double value with Perl-style %.15g precision.
/// Uses fixed notation when exponent is in [-4, 15), scientific otherwise.
/// Trailing zeros are stripped.
fn format_g15(v: f64) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    let abs_v = v.abs();
    let exp = abs_v.log10().floor() as i32;
    if exp >= -4 && exp < 15 {
        let decimal_places = (14 - exp).max(0) as usize;
        let s = format!("{:.prec$}", v, prec = decimal_places);
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s
        }
    } else {
        format!("{:.14e}", v)
    }
}

/// Format a double or list of doubles with Perl-style %.15g precision and space separator.
/// This matches ExifTool's output format for GeoTiff coordinate arrays.
pub fn format_geotiff_doubles(value: &Value) -> String {
    match value {
        Value::F64(v) => format_g15(*v),
        Value::F32(v) => format_g15(*v as f64),
        Value::List(items) => {
            items.iter().map(|item| {
                match item {
                    Value::F64(v) => format_g15(*v),
                    Value::F32(v) => format_g15(*v as f64),
                    _ => item.to_display_string(),
                }
            }).collect::<Vec<_>>().join(" ")
        }
        _ => value.to_display_string(),
    }
}

// ============================================================================
// Tag tables
// ============================================================================

static IFD0_TAGS: &[TagInfo] = &[
    TagInfo { tag_id: 0x00FE, name: "SubfileType", description: "Subfile Type", family2: "Image" },
    TagInfo { tag_id: 0x00FF, name: "OldSubfileType", description: "Old Subfile Type", family2: "Image" },
    TagInfo { tag_id: 0x0100, name: "ImageWidth", description: "Image Width", family2: "Image" },
    TagInfo { tag_id: 0x0101, name: "ImageHeight", description: "Image Height", family2: "Image" },
    TagInfo { tag_id: 0x0102, name: "BitsPerSample", description: "Bits Per Sample", family2: "Image" },
    TagInfo { tag_id: 0x0103, name: "Compression", description: "Compression", family2: "Image" },
    TagInfo { tag_id: 0x0106, name: "PhotometricInterpretation", description: "Photometric Interpretation", family2: "Image" },
    TagInfo { tag_id: 0x0107, name: "Thresholding", description: "Thresholding", family2: "Image" },
    TagInfo { tag_id: 0x010A, name: "FillOrder", description: "Fill Order", family2: "Image" },
    TagInfo { tag_id: 0x010D, name: "DocumentName", description: "Document Name", family2: "Image" },
    TagInfo { tag_id: 0x010E, name: "ImageDescription", description: "Image Description", family2: "Image" },
    TagInfo { tag_id: 0x010F, name: "Make", description: "Camera Make", family2: "Camera" },
    TagInfo { tag_id: 0x0110, name: "Model", description: "Camera Model", family2: "Camera" },
    TagInfo { tag_id: 0x0111, name: "StripOffsets", description: "Strip Offsets", family2: "Image" },
    TagInfo { tag_id: 0x0112, name: "Orientation", description: "Orientation", family2: "Image" },
    TagInfo { tag_id: 0x0115, name: "SamplesPerPixel", description: "Samples Per Pixel", family2: "Image" },
    TagInfo { tag_id: 0x0116, name: "RowsPerStrip", description: "Rows Per Strip", family2: "Image" },
    TagInfo { tag_id: 0x0117, name: "StripByteCounts", description: "Strip Byte Counts", family2: "Image" },
    TagInfo { tag_id: 0x0118, name: "MinSampleValue", description: "Min Sample Value", family2: "Image" },
    TagInfo { tag_id: 0x0119, name: "MaxSampleValue", description: "Max Sample Value", family2: "Image" },
    TagInfo { tag_id: 0x011A, name: "XResolution", description: "X Resolution", family2: "Image" },
    TagInfo { tag_id: 0x011B, name: "YResolution", description: "Y Resolution", family2: "Image" },
    TagInfo { tag_id: 0x011C, name: "PlanarConfiguration", description: "Planar Configuration", family2: "Image" },
    TagInfo { tag_id: 0x011D, name: "PageName", description: "Page Name", family2: "Image" },
    TagInfo { tag_id: 0x0128, name: "ResolutionUnit", description: "Resolution Unit", family2: "Image" },
    TagInfo { tag_id: 0x0129, name: "PageNumber", description: "Page Number", family2: "Image" },
    TagInfo { tag_id: 0x012D, name: "TransferFunction", description: "Transfer Function", family2: "Image" },
    TagInfo { tag_id: 0x0131, name: "Software", description: "Software", family2: "Image" },
    TagInfo { tag_id: 0x0132, name: "ModifyDate", description: "Modify Date", family2: "Time" },
    TagInfo { tag_id: 0x013B, name: "Artist", description: "Artist", family2: "Author" },
    TagInfo { tag_id: 0x013C, name: "HostComputer", description: "Host Computer", family2: "Image" },
    TagInfo { tag_id: 0x013D, name: "Predictor", description: "Predictor", family2: "Image" },
    TagInfo { tag_id: 0x013E, name: "WhitePoint", description: "White Point", family2: "Image" },
    TagInfo { tag_id: 0x013F, name: "PrimaryChromaticities", description: "Primary Chromaticities", family2: "Image" },
    TagInfo { tag_id: 0x0140, name: "ColorMap", description: "Color Map", family2: "Image" },
    TagInfo { tag_id: 0x0141, name: "HalftoneHints", description: "Halftone Hints", family2: "Image" },
    TagInfo { tag_id: 0x0142, name: "TileWidth", description: "Tile Width", family2: "Image" },
    TagInfo { tag_id: 0x0143, name: "TileLength", description: "Tile Length", family2: "Image" },
    TagInfo { tag_id: 0x0153, name: "SampleFormat", description: "Sample Format", family2: "Image" },
    TagInfo { tag_id: 0x0201, name: "ThumbnailOffset", description: "Thumbnail Offset", family2: "Image" },
    TagInfo { tag_id: 0x0202, name: "ThumbnailLength", description: "Thumbnail Length", family2: "Image" },
    TagInfo { tag_id: 0x0211, name: "YCbCrCoefficients", description: "YCbCr Coefficients", family2: "Image" },
    TagInfo { tag_id: 0x0212, name: "YCbCrSubSampling", description: "YCbCr Sub Sampling", family2: "Image" },
    TagInfo { tag_id: 0x0213, name: "YCbCrPositioning", description: "YCbCr Positioning", family2: "Image" },
    TagInfo { tag_id: 0x0214, name: "ReferenceBlackWhite", description: "Reference Black White", family2: "Image" },
    TagInfo { tag_id: 0x02BC, name: "ApplicationNotes", description: "Application Notes (XMP)", family2: "Image" },
    TagInfo { tag_id: 0x4746, name: "Rating", description: "Rating", family2: "Image" },
    TagInfo { tag_id: 0x4749, name: "RatingPercent", description: "Rating Percent", family2: "Image" },
    TagInfo { tag_id: 0x8298, name: "Copyright", description: "Copyright", family2: "Author" },
    TagInfo { tag_id: 0x83BB, name: "IPTC-NAA", description: "IPTC-NAA", family2: "Image" },
    TagInfo { tag_id: 0x8649, name: "PhotoshopSettings", description: "Photoshop Settings", family2: "Image" },
    TagInfo { tag_id: 0x8769, name: "ExifOffset", description: "Exif IFD Pointer", family2: "Image" },
    TagInfo { tag_id: 0x8773, name: "ICC_Profile", description: "ICC Profile", family2: "Image" },
    TagInfo { tag_id: 0x8825, name: "GPSInfo", description: "GPS Info IFD Pointer", family2: "Location" },
    TagInfo { tag_id: 0x830E, name: "PixelScale", description: "Pixel Scale", family2: "Location" },
    TagInfo { tag_id: 0x85D8, name: "ModelTransform", description: "Model Transform", family2: "Location" },
    TagInfo { tag_id: 0x8680, name: "IntergraphMatrix", description: "Intergraph Matrix", family2: "Image" },
    TagInfo { tag_id: 0x87AF, name: "GeoTiffDirectory", description: "GeoTiff Directory", family2: "Location" },
    TagInfo { tag_id: 0x87B0, name: "GeoTiffDoubleParams", description: "GeoTiff Double Params", family2: "Location" },
    TagInfo { tag_id: 0x87B1, name: "GeoTiffAsciiParams", description: "GeoTiff ASCII Params", family2: "Location" },
    TagInfo { tag_id: 0xC612, name: "DNGVersion", description: "DNG Version", family2: "Image" },
    TagInfo { tag_id: 0xC613, name: "DNGBackwardVersion", description: "DNG Backward Version", family2: "Image" },
    TagInfo { tag_id: 0xC614, name: "UniqueCameraModel", description: "Unique Camera Model", family2: "Camera" },
    TagInfo { tag_id: 0xC621, name: "ColorMatrix1", description: "Color Matrix 1", family2: "Image" },
    TagInfo { tag_id: 0xC622, name: "ColorMatrix2", description: "Color Matrix 2", family2: "Image" },
    TagInfo { tag_id: 0xC628, name: "AsShotNeutral", description: "As Shot Neutral", family2: "Camera" },
    TagInfo { tag_id: 0xC62F, name: "CameraCalibration1", description: "Camera Calibration 1", family2: "Camera" },
    TagInfo { tag_id: 0xC630, name: "CameraCalibration2", description: "Camera Calibration 2", family2: "Camera" },
    TagInfo { tag_id: 0xC65A, name: "CalibrationIlluminant1", description: "Calibration Illuminant 1", family2: "Camera" },
    TagInfo { tag_id: 0xC65B, name: "CalibrationIlluminant2", description: "Calibration Illuminant 2", family2: "Camera" },
];

static EXIF_IFD_TAGS: &[TagInfo] = &[
    TagInfo { tag_id: 0x829A, name: "ExposureTime", description: "Exposure Time", family2: "Camera" },
    TagInfo { tag_id: 0x829D, name: "FNumber", description: "F Number", family2: "Camera" },
    TagInfo { tag_id: 0x8822, name: "ExposureProgram", description: "Exposure Program", family2: "Camera" },
    TagInfo { tag_id: 0x8827, name: "ISO", description: "ISO Speed", family2: "Camera" },
    TagInfo { tag_id: 0x8830, name: "SensitivityType", description: "Sensitivity Type", family2: "Camera" },
    TagInfo { tag_id: 0x9000, name: "ExifVersion", description: "Exif Version", family2: "Image" },
    TagInfo { tag_id: 0x9003, name: "DateTimeOriginal", description: "Date/Time Original", family2: "Time" },
    TagInfo { tag_id: 0x9004, name: "CreateDate", description: "Create Date", family2: "Time" },
    TagInfo { tag_id: 0x9010, name: "OffsetTime", description: "Offset Time", family2: "Time" },
    TagInfo { tag_id: 0x9011, name: "OffsetTimeOriginal", description: "Offset Time Original", family2: "Time" },
    TagInfo { tag_id: 0x9012, name: "OffsetTimeDigitized", description: "Offset Time Digitized", family2: "Time" },
    TagInfo { tag_id: 0x9101, name: "ComponentsConfiguration", description: "Components Configuration", family2: "Image" },
    TagInfo { tag_id: 0x9102, name: "CompressedBitsPerPixel", description: "Compressed Bits Per Pixel", family2: "Image" },
    TagInfo { tag_id: 0x9201, name: "ShutterSpeedValue", description: "Shutter Speed Value", family2: "Camera" },
    TagInfo { tag_id: 0x9202, name: "ApertureValue", description: "Aperture Value", family2: "Camera" },
    TagInfo { tag_id: 0x9203, name: "BrightnessValue", description: "Brightness Value", family2: "Camera" },
    TagInfo { tag_id: 0x9204, name: "ExposureCompensation", description: "Exposure Compensation", family2: "Camera" },
    TagInfo { tag_id: 0x9205, name: "MaxApertureValue", description: "Max Aperture Value", family2: "Camera" },
    TagInfo { tag_id: 0x9207, name: "MeteringMode", description: "Metering Mode", family2: "Camera" },
    TagInfo { tag_id: 0x9208, name: "LightSource", description: "Light Source", family2: "Camera" },
    TagInfo { tag_id: 0x9209, name: "Flash", description: "Flash", family2: "Camera" },
    TagInfo { tag_id: 0x920A, name: "FocalLength", description: "Focal Length", family2: "Camera" },
    TagInfo { tag_id: 0x927C, name: "MakerNote", description: "Maker Note", family2: "Camera" },
    TagInfo { tag_id: 0x9286, name: "UserComment", description: "User Comment", family2: "Image" },
    TagInfo { tag_id: 0x9290, name: "SubSecTime", description: "Sub Sec Time", family2: "Time" },
    TagInfo { tag_id: 0x9291, name: "SubSecTimeOriginal", description: "Sub Sec Time Original", family2: "Time" },
    TagInfo { tag_id: 0x9292, name: "SubSecTimeDigitized", description: "Sub Sec Time Digitized", family2: "Time" },
    TagInfo { tag_id: 0xA000, name: "FlashpixVersion", description: "Flashpix Version", family2: "Image" },
    TagInfo { tag_id: 0xA001, name: "ColorSpace", description: "Color Space", family2: "Image" },
    TagInfo { tag_id: 0xA002, name: "ExifImageWidth", description: "Exif Image Width", family2: "Image" },
    TagInfo { tag_id: 0xA003, name: "ExifImageHeight", description: "Exif Image Height", family2: "Image" },
    TagInfo { tag_id: 0xA005, name: "InteropOffset", description: "Interoperability IFD Pointer", family2: "Image" },
    TagInfo { tag_id: 0xA20E, name: "FocalPlaneXResolution", description: "Focal Plane X Resolution", family2: "Camera" },
    TagInfo { tag_id: 0xA20F, name: "FocalPlaneYResolution", description: "Focal Plane Y Resolution", family2: "Camera" },
    TagInfo { tag_id: 0xA210, name: "FocalPlaneResolutionUnit", description: "Focal Plane Resolution Unit", family2: "Camera" },
    TagInfo { tag_id: 0xA217, name: "SensingMethod", description: "Sensing Method", family2: "Camera" },
    TagInfo { tag_id: 0xA300, name: "FileSource", description: "File Source", family2: "Camera" },
    TagInfo { tag_id: 0xA301, name: "SceneType", description: "Scene Type", family2: "Camera" },
    TagInfo { tag_id: 0xA401, name: "CustomRendered", description: "Custom Rendered", family2: "Camera" },
    TagInfo { tag_id: 0xA402, name: "ExposureMode", description: "Exposure Mode", family2: "Camera" },
    TagInfo { tag_id: 0xA403, name: "WhiteBalance", description: "White Balance", family2: "Camera" },
    TagInfo { tag_id: 0xA404, name: "DigitalZoomRatio", description: "Digital Zoom Ratio", family2: "Camera" },
    TagInfo { tag_id: 0xA405, name: "FocalLengthIn35mmFormat", description: "Focal Length In 35mm Format", family2: "Camera" },
    TagInfo { tag_id: 0xA406, name: "SceneCaptureType", description: "Scene Capture Type", family2: "Camera" },
    TagInfo { tag_id: 0xA408, name: "Contrast", description: "Contrast", family2: "Camera" },
    TagInfo { tag_id: 0xA409, name: "Saturation", description: "Saturation", family2: "Camera" },
    TagInfo { tag_id: 0xA40A, name: "Sharpness", description: "Sharpness", family2: "Camera" },
    TagInfo { tag_id: 0xA420, name: "ImageUniqueID", description: "Image Unique ID", family2: "Image" },
    TagInfo { tag_id: 0xA430, name: "OwnerName", description: "Owner Name", family2: "Camera" },
    TagInfo { tag_id: 0xA431, name: "SerialNumber", description: "Serial Number", family2: "Camera" },
    TagInfo { tag_id: 0xA432, name: "LensInfo", description: "Lens Info", family2: "Camera" },
    TagInfo { tag_id: 0xA433, name: "LensMake", description: "Lens Make", family2: "Camera" },
    TagInfo { tag_id: 0xA434, name: "LensModel", description: "Lens Model", family2: "Camera" },
    TagInfo { tag_id: 0xA435, name: "LensSerialNumber", description: "Lens Serial Number", family2: "Camera" },
    TagInfo { tag_id: 0xA460, name: "CompositeImage", description: "Composite Image", family2: "Image" },
    TagInfo { tag_id: 0xA461, name: "SourceImageNumberOfCompositeImage", description: "Source Image Number", family2: "Image" },
    TagInfo { tag_id: 0xA462, name: "SourceExposureTimesOfCompositeImage", description: "Source Exposure Times", family2: "Image" },
    // Additional ExifIFD tags
    TagInfo { tag_id: 0x9206, name: "SubjectDistance", description: "Subject Distance", family2: "Camera" },
    TagInfo { tag_id: 0x9214, name: "SubjectArea", description: "Subject Area", family2: "Camera" },
    TagInfo { tag_id: 0xA215, name: "ExposureIndex", description: "Exposure Index", family2: "Camera" },
    TagInfo { tag_id: 0xA302, name: "CFAPattern", description: "CFA Pattern", family2: "Camera" },
    TagInfo { tag_id: 0xA407, name: "GainControl", description: "Gain Control", family2: "Camera" },
    TagInfo { tag_id: 0xA40B, name: "DeviceSettingDescription", description: "Device Setting Description", family2: "Camera" },
    TagInfo { tag_id: 0xA40C, name: "SubjectDistanceRange", description: "Subject Distance Range", family2: "Camera" },
    TagInfo { tag_id: 0xA500, name: "Gamma", description: "Gamma", family2: "Image" },
    // Print Image Matching
    TagInfo { tag_id: 0xC4A5, name: "PrintImageMatching", description: "Print Image Matching", family2: "Image" },
];

static GPS_TAGS: &[TagInfo] = &[
    TagInfo { tag_id: 0x0000, name: "GPSVersionID", description: "GPS Version ID", family2: "Location" },
    TagInfo { tag_id: 0x0001, name: "GPSLatitudeRef", description: "GPS Latitude Ref", family2: "Location" },
    TagInfo { tag_id: 0x0002, name: "GPSLatitude", description: "GPS Latitude", family2: "Location" },
    TagInfo { tag_id: 0x0003, name: "GPSLongitudeRef", description: "GPS Longitude Ref", family2: "Location" },
    TagInfo { tag_id: 0x0004, name: "GPSLongitude", description: "GPS Longitude", family2: "Location" },
    TagInfo { tag_id: 0x0005, name: "GPSAltitudeRef", description: "GPS Altitude Ref", family2: "Location" },
    TagInfo { tag_id: 0x0006, name: "GPSAltitude", description: "GPS Altitude", family2: "Location" },
    TagInfo { tag_id: 0x0007, name: "GPSTimeStamp", description: "GPS Time Stamp", family2: "Time" },
    TagInfo { tag_id: 0x0008, name: "GPSSatellites", description: "GPS Satellites", family2: "Location" },
    TagInfo { tag_id: 0x0009, name: "GPSStatus", description: "GPS Status", family2: "Location" },
    TagInfo { tag_id: 0x000A, name: "GPSMeasureMode", description: "GPS Measure Mode", family2: "Location" },
    TagInfo { tag_id: 0x000B, name: "GPSDOP", description: "GPS Dilution Of Precision", family2: "Location" },
    TagInfo { tag_id: 0x000C, name: "GPSSpeedRef", description: "GPS Speed Ref", family2: "Location" },
    TagInfo { tag_id: 0x000D, name: "GPSSpeed", description: "GPS Speed", family2: "Location" },
    TagInfo { tag_id: 0x000E, name: "GPSTrackRef", description: "GPS Track Ref", family2: "Location" },
    TagInfo { tag_id: 0x000F, name: "GPSTrack", description: "GPS Track", family2: "Location" },
    TagInfo { tag_id: 0x0010, name: "GPSImgDirectionRef", description: "GPS Img Direction Ref", family2: "Location" },
    TagInfo { tag_id: 0x0011, name: "GPSImgDirection", description: "GPS Img Direction", family2: "Location" },
    TagInfo { tag_id: 0x0012, name: "GPSMapDatum", description: "GPS Map Datum", family2: "Location" },
    TagInfo { tag_id: 0x0013, name: "GPSDestLatitudeRef", description: "GPS Dest Latitude Ref", family2: "Location" },
    TagInfo { tag_id: 0x0014, name: "GPSDestLatitude", description: "GPS Dest Latitude", family2: "Location" },
    TagInfo { tag_id: 0x0015, name: "GPSDestLongitudeRef", description: "GPS Dest Longitude Ref", family2: "Location" },
    TagInfo { tag_id: 0x0016, name: "GPSDestLongitude", description: "GPS Dest Longitude", family2: "Location" },
    TagInfo { tag_id: 0x0017, name: "GPSDestBearingRef", description: "GPS Dest Bearing Ref", family2: "Location" },
    TagInfo { tag_id: 0x0018, name: "GPSDestBearing", description: "GPS Dest Bearing", family2: "Location" },
    TagInfo { tag_id: 0x0019, name: "GPSDestDistanceRef", description: "GPS Dest Distance Ref", family2: "Location" },
    TagInfo { tag_id: 0x001A, name: "GPSDestDistance", description: "GPS Dest Distance", family2: "Location" },
    TagInfo { tag_id: 0x001B, name: "GPSProcessingMethod", description: "GPS Processing Method", family2: "Location" },
    TagInfo { tag_id: 0x001C, name: "GPSAreaInformation", description: "GPS Area Information", family2: "Location" },
    TagInfo { tag_id: 0x001D, name: "GPSDateStamp", description: "GPS Date Stamp", family2: "Time" },
    TagInfo { tag_id: 0x001E, name: "GPSDifferential", description: "GPS Differential", family2: "Location" },
    TagInfo { tag_id: 0x001F, name: "GPSHPositioningError", description: "GPS Horizontal Positioning Error", family2: "Location" },
];

static INTEROP_TAGS: &[TagInfo] = &[
    TagInfo { tag_id: 0x0001, name: "InteropIndex", description: "Interoperability Index", family2: "Image" },
    TagInfo { tag_id: 0x0002, name: "InteropVersion", description: "Interoperability Version", family2: "Image" },
];
