//! Sigma/Foveon X3F RAW file format reader.
//!
//! Parses X3F files: header (v2.x/v4), extended header, PROP key-value pairs,
//! and embedded JPEG (JpgFromRaw / PreviewImage).
//! Mirrors ExifTool's SigmaRaw.pm.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_x3f(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 40 {
        return Err(Error::InvalidData("X3F too short".into()));
    }
    if &data[0..4] != b"FOVb" {
        return Err(Error::InvalidData("not an X3F file".into()));
    }

    let mut tags = Vec::new();

    // --- File version (little-endian uint32 at offset 4) ---
    let ver_raw = u32_le(data, 4);
    let ver_major = (ver_raw >> 16) as f64;
    let ver_minor = (ver_raw & 0xffff) as f64;
    let ver_f = ver_major + ver_minor / 10000.0; // for comparisons (2.2 → 2.0002, enough)
    let ver_str = format!("{}.{}", ver_raw >> 16, ver_raw & 0xffff);
    tags.push(mk_tag_str("FileVersion", "File Version", ver_str, "X3F", "Main", "Image"));

    if ver_raw >> 16 >= 4 {
        // Version 4.x header — different layout
        parse_header4(data, &mut tags);
    } else {
        // Version 2.x header
        parse_header2(data, ver_f, &mut tags);
    }

    // --- Directory pointer: last 4 bytes of file ---
    if data.len() < 4 {
        return Ok(tags);
    }
    let dir_offset = u32_le(data, data.len() - 4) as usize;
    if dir_offset + 12 > data.len() {
        return Ok(tags);
    }

    // --- Directory header: "SECd" + ver(4) + entries(4) ---
    if &data[dir_offset..dir_offset + 4] != b"SECd" {
        return Ok(tags);
    }
    let entries = u32_le(data, dir_offset + 8) as usize;
    if entries == 0 || entries > 100 {
        return Ok(tags);
    }
    let dir_data_start = dir_offset + 12;
    if dir_data_start + entries * 12 > data.len() {
        return Ok(tags);
    }

    // Track if we already found a JpgFromRaw
    let mut found_jpg_from_raw = false;

    // --- Parse each directory entry ---
    for i in 0..entries {
        let pos = dir_data_start + i * 12;
        let sec_offset = u32_le(data, pos) as usize;
        let sec_len = u32_le(data, pos + 4) as usize;
        let tag_bytes = &data[pos + 8..pos + 12];

        if sec_offset + sec_len > data.len() {
            continue;
        }
        let sec_data = &data[sec_offset..sec_offset + sec_len];

        match tag_bytes {
            b"PROP" => {
                parse_properties(sec_data, &mut tags);
            }
            b"IMA2" => {
                if !found_jpg_from_raw {
                    if let Some(img_data) = extract_image_data(sec_data) {
                        // Full-size JPEG with EXIF becomes JpgFromRaw
                        if img_data.starts_with(b"\xff\xd8\xff\xe1") {
                            found_jpg_from_raw = true;
                            // Extract EXIF from embedded JPEG
                            if let Ok(jpeg_tags) = crate::formats::jpeg::read_jpeg(img_data) {
                                tags.extend(jpeg_tags);
                            }
                            // Also store as JpgFromRaw binary tag
                            tags.push(mk_tag_binary(
                                "JpgFromRaw", "Jpg From Raw",
                                img_data.to_vec(),
                                "X3F", "Main", "Preview",
                            ));
                        } else {
                            // Non-full-size preview
                            tags.push(mk_tag_binary(
                                "PreviewImage", "Preview Image",
                                img_data.to_vec(),
                                "X3F", "Main", "Preview",
                            ));
                        }
                    }
                } else {
                    // Additional IMA2 entries become PreviewImage
                    if let Some(img_data) = extract_image_data(sec_data) {
                        tags.push(mk_tag_binary(
                            "PreviewImage", "Preview Image",
                            img_data.to_vec(),
                            "X3F", "Main", "Preview",
                        ));
                    }
                }
            }
            b"IMAG" => {
                if let Some(img_data) = extract_image_data(sec_data) {
                    tags.push(mk_tag_binary(
                        "PreviewImage", "Preview Image",
                        img_data.to_vec(),
                        "X3F", "Main", "Preview",
                    ));
                }
            }
            _ => {}
        }
    }

    Ok(tags)
}

/// Extract image payload from a SECi section (28-byte subsection header + data).
/// Returns image bytes only if it's a JPEG (format 18 = 0x12) full-size preview.
fn extract_image_data(sec_data: &[u8]) -> Option<&[u8]> {
    if sec_data.len() < 28 {
        return None;
    }
    if &sec_data[0..4] != b"SECi" {
        return None;
    }
    // Check: version 2.0 (bytes 4-7 = 00 00 02 00), type 2 (bytes 8-11 = 02 00 00 00),
    // format 0x12=18 (bytes 12-15 = 12 00 00 00) → full-size JPEG preview
    let sec_ver = u16_le(sec_data, 6); // major version
    let img_type = u32_le(sec_data, 8);
    let img_fmt = u32_le(sec_data, 12);

    if sec_ver == 2 && img_type == 2 && img_fmt == 0x12 {
        let payload = &sec_data[28..];
        if !payload.is_empty() {
            return Some(payload);
        }
    }
    None
}

/// Parse a v2.x X3F header.
fn parse_header2(data: &[u8], ver_f: f64, tags: &mut Vec<Tag>) {
    // Determine header length
    // v2.3 → 104 bytes, v2.1/v2.2 → 72 bytes
    let hdr_len = if ver_f >= 2.0003 { 104usize } else { 72usize };
    let has_extended = data.len() >= hdr_len + 160;

    // ImageUniqueID: bytes 8..24 (16 bytes) — hex string
    if data.len() >= 24 {
        let uid = hex_bytes(&data[8..24]);
        tags.push(mk_tag_str("ImageUniqueID", "Image Unique ID", uid, "X3F", "Header", "Camera"));
    }

    // MarkBits: uint32 at offset 24 (position 6 in int32u array starting at 0)
    if data.len() >= 28 {
        let mark = u32_le(data, 24);
        // Perl: PrintConv => { BITMASK => {} } — with no bits defined, prints "(none)" when 0
        let mark_str = if mark == 0 { "(none)".to_string() } else { mark.to_string() };
        tags.push(mk_tag_str("MarkBits", "Mark Bits", mark_str, "X3F", "Header", "Image"));
    }

    // ImageWidth: uint32 at offset 28
    if data.len() >= 32 {
        let w = u32_le(data, 28);
        tags.push(mk_tag_u32("ImageWidth", "Image Width", w, "X3F", "Header", "Image"));
    }

    // ImageHeight: uint32 at offset 32
    if data.len() >= 36 {
        let h = u32_le(data, 32);
        tags.push(mk_tag_u32("ImageHeight", "Image Height", h, "X3F", "Header", "Image"));
    }

    // Rotation: uint32 at offset 36
    if data.len() >= 40 {
        let r = u32_le(data, 36);
        tags.push(mk_tag_u32("Rotation", "Rotation", r, "X3F", "Header", "Image"));
    }

    // WhiteBalance: string[32] at offset 40
    if data.len() >= 72 {
        let wb = read_cstr(&data[40..72]);
        if !wb.is_empty() {
            tags.push(mk_tag_str("WhiteBalance", "White Balance", wb, "X3F", "Header", "Camera"));
        }
    }

    // SceneCaptureType: string[32] at offset 72 (only in v2.3+, hdrLen=104)
    if hdr_len >= 104 && data.len() >= 104 {
        let sct = read_cstr(&data[72..104]);
        if !sct.is_empty() {
            tags.push(mk_tag_str("SceneCaptureType", "Scene Capture Type", sct, "X3F", "Header", "Image"));
        }
    }

    // Extended header (v2.1/v2.2/v2.3): follows at hdr_len, is 160 bytes
    // Format: 32 bytes of tag-index array, then up to 32 float values
    if has_extended {
        let ext_start = hdr_len;
        // Tag indices: 32 uint8 values; each non-zero entry says "there's data here"
        let tag_indices = &data[ext_start..ext_start + 32];
        for (i, &tidx) in tag_indices.iter().enumerate() {
            if tidx == 0 {
                continue;
            }
            let float_offset = ext_start + 32 + i * 4;
            if float_offset + 4 > data.len() {
                continue;
            }
            let val = f32_le(data, float_offset);
            let val_str = format!("{:.1}", val);

            // tidx corresponds to HeaderExt table index:
            // 1=ExposureAdjust, 2=Contrast, 3=Shadow, 4=Highlight,
            // 5=Saturation, 6=Sharpness, 7=RedAdjust, 8=GreenAdjust,
            // 9=BlueAdjust, 10=X3FillLight
            let name = match tidx {
                1  => "ExposureAdjust",
                2  => "Contrast",
                3  => "Shadow",
                4  => "Highlight",
                5  => "Saturation",
                6  => "Sharpness",
                7  => "RedAdjust",
                8  => "GreenAdjust",
                9  => "BlueAdjust",
                10 => "X3FillLight",
                _  => continue,
            };
            tags.push(mk_tag_str(name, name, val_str, "X3F", "HeaderExt", "Camera"));
        }
    }
}

/// Parse a v4.x X3F header.
fn parse_header4(data: &[u8], tags: &mut Vec<Tag>) {
    // ImageWidth: uint32 at offset 40 (index 10 in int32u array)
    if data.len() >= 44 {
        let w = u32_le(data, 40);
        tags.push(mk_tag_u32("ImageWidth", "Image Width", w, "X3F", "Header", "Image"));
    }
    // ImageHeight: uint32 at offset 44 (index 11)
    if data.len() >= 48 {
        let h = u32_le(data, 44);
        tags.push(mk_tag_u32("ImageHeight", "Image Height", h, "X3F", "Header", "Image"));
    }
    // Rotation: uint32 at offset 48 (index 12)
    if data.len() >= 52 {
        let r = u32_le(data, 48);
        tags.push(mk_tag_u32("Rotation", "Rotation", r, "X3F", "Header", "Image"));
    }
}

/// Parse PROP section (SECp): key=value UTF-16LE pairs.
fn parse_properties(sec_data: &[u8], tags: &mut Vec<Tag>) {
    if sec_data.len() < 24 {
        return;
    }
    if &sec_data[0..4] != b"SECp" {
        return;
    }
    let entries = u32_le(sec_data, 8) as usize;
    let fmt = u32_le(sec_data, 12);
    if fmt != 0 {
        return; // only UTF-16LE supported
    }
    let char_len = u32_le(sec_data, 20) as usize;
    let char_start = 24 + 8 * entries;

    if char_start + char_len * 2 > sec_data.len() {
        return;
    }

    // Decode UTF-16LE character array
    let chars_bytes = &sec_data[char_start..char_start + char_len * 2];
    let mut chars = Vec::with_capacity(char_len);
    for i in 0..char_len {
        chars.push(u16::from_le_bytes([chars_bytes[i * 2], chars_bytes[i * 2 + 1]]));
    }

    for i in 0..entries {
        let entry_off = 24 + i * 8;
        if entry_off + 8 > sec_data.len() {
            break;
        }
        let name_pos = u32_le(sec_data, entry_off) as usize;
        let val_pos = u32_le(sec_data, entry_off + 4) as usize;

        if name_pos >= chars.len() || val_pos >= chars.len() {
            continue;
        }

        let prop_name = extract_utf16_str(&chars, name_pos);
        let prop_val = extract_utf16_str(&chars, val_pos);

        // Map PROP key to ExifTool tag name + print conversion
        if let Some((tag_name, tag_desc, print_val)) = map_prop(&prop_name, &prop_val) {
            tags.push(mk_tag_str(tag_name, tag_desc, print_val, "X3F", "Properties", "Camera"));
        }
    }
}

/// Map a PROP key/value to (tag_name, tag_description, print_value).
/// Returns None if the key is not recognized.
fn map_prop(key: &str, val: &str) -> Option<(&'static str, &'static str, String)> {
    match key {
        "AEMODE" => {
            let pv = match val {
                "8" => "8-segment",
                "C" => "Center-weighted average",
                "A" => "Average",
                _ => val,
            };
            Some(("MeteringMode", "Metering Mode", pv.to_string()))
        }
        "AFMODE" => Some(("FocusMode", "Focus Mode", val.to_string())),
        "AP_DESC" => Some(("ApertureDisplayed", "Aperture Displayed", val.to_string())),
        "APERTURE" => {
            // FNumber
            if let Ok(f) = val.parse::<f64>() {
                Some(("FNumber", "F Number", format!("{:.1}", f)))
            } else {
                Some(("FNumber", "F Number", val.to_string()))
            }
        }
        "BRACKET" => Some(("BracketShot", "Bracket Shot", val.to_string())),
        "BURST" => Some(("BurstShot", "Burst Shot", val.to_string())),
        "CAMMANUF" => Some(("Make", "Make", val.to_string())),
        "CAMMODEL" => Some(("Model", "Model", val.to_string())),
        "CAMNAME" => Some(("CameraName", "Camera Name", val.to_string())),
        "CAMSERIAL" => Some(("SerialNumber", "Serial Number", val.to_string())),
        "CM_DESC" => Some(("SceneCaptureType", "Scene Capture Type", val.to_string())),
        "COLORSPACE" => Some(("ColorSpace", "Color Space", val.to_string())),
        "DRIVE" => {
            let pv = match val {
                "SINGLE" => "Single Shot",
                "MULTI"  => "Multi Shot",
                "2S"     => "2 s Timer",
                "10S"    => "10 s Timer",
                "UP"     => "Mirror Up",
                "AB"     => "Auto Bracket",
                "OFF"    => "Off",
                _        => val,
            };
            Some(("DriveMode", "Drive Mode", pv.to_string()))
        }
        "EXPCOMP" => Some(("ExposureCompensation", "Exposure Compensation", val.to_string())),
        "EXPNET" => Some(("NetExposureCompensation", "Net Exposure Compensation", val.to_string())),
        "EXPTIME" => {
            // IntegrationTime: value is in microseconds
            if let Ok(usec) = val.parse::<f64>() {
                let secs = usec * 1e-6;
                let print = print_exposure_time(secs);
                Some(("IntegrationTime", "Integration Time", print))
            } else {
                Some(("IntegrationTime", "Integration Time", val.to_string()))
            }
        }
        "FIRMVERS" => Some(("FirmwareVersion", "Firmware Version", val.to_string())),
        "FLASH" => {
            // ucfirst(lc($val))
            let pv = ucfirst_lc(val);
            Some(("FlashMode", "Flash Mode", pv))
        }
        "FLASHEXPCOMP" => Some(("FlashExposureComp", "Flash Exposure Comp", val.to_string())),
        "FLASHPOWER" => Some(("FlashPower", "Flash Power", val.to_string())),
        "FLASHTTLMODE" => Some(("FlashTTLMode", "Flash TTL Mode", val.to_string())),
        "FLASHTYPE" => Some(("FlashType", "Flash Type", val.to_string())),
        "FLENGTH" => {
            if let Ok(f) = val.parse::<f64>() {
                Some(("FocalLength", "Focal Length", format!("{:.1} mm", f)))
            } else {
                Some(("FocalLength", "Focal Length", val.to_string()))
            }
        }
        "FLEQ35MM" => {
            if let Ok(f) = val.parse::<f64>() {
                Some(("FocalLengthIn35mmFormat", "Focal Length In 35mm Format", format!("{:.1} mm", f)))
            } else {
                Some(("FocalLengthIn35mmFormat", "Focal Length In 35mm Format", val.to_string()))
            }
        }
        "FOCUS" => {
            let pv = match val {
                "AF"      => "Auto-focus Locked",
                "NO LOCK" => "Auto-focus Didn't Lock",
                "M"       => "Manual",
                _         => val,
            };
            Some(("Focus", "Focus", pv.to_string()))
        }
        "IMAGERBOARDID" => Some(("ImagerBoardID", "Imager Board ID", val.to_string())),
        "IMAGERTEMP" => {
            Some(("SensorTemperature", "Sensor Temperature", format!("{} C", val)))
        }
        "IMAGEBOARDID" => Some(("ImageBoardID", "Image Board ID", val.to_string())),
        "ISO" => Some(("ISO", "ISO", val.to_string())),
        "LENSARANGE" => Some(("LensApertureRange", "Lens Aperture Range", val.to_string())),
        "LENSFRANGE" => Some(("LensFocalRange", "Lens Focal Range", val.to_string())),
        "LENSMODEL" => {
            // LensType — if it looks like a hex number, convert to hex display
            let pv = if val.chars().all(|c| c.is_ascii_hexdigit()) && !val.is_empty() {
                // Map hex value to lens name if known
                if let Ok(hex_val) = u32::from_str_radix(val.trim(), 16) {
                    lens_type_name(hex_val)
                } else {
                    val.to_string()
                }
            } else {
                val.to_string()
            };
            Some(("LensType", "Lens Type", pv))
        }
        "PMODE" => {
            let pv = match val {
                "P" => "Program",
                "A" => "Aperture Priority",
                "S" => "Shutter Priority",
                "M" => "Manual",
                _   => val,
            };
            Some(("ExposureProgram", "Exposure Program", pv.to_string()))
        }
        "RESOLUTION" => {
            let pv = match val {
                "LOW" => "Low",
                "MED" => "Medium",
                "HI"  => "High",
                _     => val,
            };
            Some(("Quality", "Quality", pv.to_string()))
        }
        "SENSORID" => Some(("SensorID", "Sensor ID", val.to_string())),
        "SH_DESC" => Some(("ShutterSpeedDisplayed", "Shutter Speed Displayed", val.to_string())),
        "SHUTTER" => {
            if let Ok(f) = val.parse::<f64>() {
                let print = print_exposure_time(f);
                Some(("ExposureTime", "Exposure Time", print))
            } else {
                Some(("ExposureTime", "Exposure Time", val.to_string()))
            }
        }
        "TIME" => {
            // Unix timestamp → "YYYY:MM:DD HH:MM:SS"
            if let Ok(ts) = val.parse::<i64>() {
                let dt = unix_to_datetime(ts);
                Some(("DateTimeOriginal", "Date/Time Original", dt))
            } else {
                Some(("DateTimeOriginal", "Date/Time Original", val.to_string()))
            }
        }
        "WB_DESC" => Some(("WhiteBalance", "White Balance", val.to_string())),
        "VERSION_BF" => Some(("VersionBF", "Version BF", val.to_string())),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn u32_le(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([data[offset], data[offset+1], data[offset+2], data[offset+3]])
}

fn u16_le(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([data[offset], data[offset+1]])
}

fn f32_le(data: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes([data[offset], data[offset+1], data[offset+2], data[offset+3]])
}

fn hex_bytes(data: &[u8]) -> String {
    data.iter().map(|b| format!("{:02x}", b)).collect()
}

fn read_cstr(data: &[u8]) -> String {
    let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    String::from_utf8_lossy(&data[..end]).into_owned()
}

fn extract_utf16_str(chars: &[u16], pos: usize) -> String {
    let mut end = pos;
    while end < chars.len() && chars[end] != 0 {
        end += 1;
    }
    // Since PROP strings are ASCII-compatible, just take low bytes
    chars[pos..end].iter()
        .map(|&c| char::from_u32(c as u32).unwrap_or('?'))
        .collect()
}

/// Format exposure time as "1/N" or decimal.
fn print_exposure_time(secs: f64) -> String {
    if secs <= 0.0 {
        return secs.to_string();
    }
    if secs >= 1.0 {
        return format!("{}", secs as u32);
    }
    // Express as fraction
    let inv = 1.0 / secs;
    let rounded = inv.round() as u64;
    format!("1/{}", rounded)
}

/// Convert Unix timestamp to "YYYY:MM:DD HH:MM:SS".
fn unix_to_datetime(ts: i64) -> String {
    // Simple implementation without external crates
    // Seconds since 1970-01-01 00:00:00 UTC
    let mut secs = ts;
    if secs < 0 {
        return "0000:00:00 00:00:00".to_string();
    }

    let s = (secs % 60) as u32;
    secs /= 60;
    let m = (secs % 60) as u32;
    secs /= 60;
    let h = (secs % 24) as u32;
    secs /= 24;

    // Days since epoch to year/month/day
    let (y, mo, d) = days_to_ymd(secs as u32);
    format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}", y, mo, d, h, m, s)
}

fn days_to_ymd(mut days: u32) -> (u32, u32, u32) {
    // Gregorian calendar calculation
    let mut year = 1970u32;
    loop {
        let leap = is_leap(year);
        let days_in_year = if leap { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let leap = is_leap(year);
    let month_days = [31u32, if leap {29} else {28}, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 1u32;
    for &md in &month_days {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }
    (year, month, days + 1)
}

fn is_leap(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn ucfirst_lc(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut chars = lower.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => {
            let upper: String = c.to_uppercase().collect();
            upper + chars.as_str()
        }
    }
}

/// Look up Sigma lens type by hex value (from %Image::ExifTool::Sigma::sigmaLensTypes).
fn lens_type_name(hex_val: u32) -> String {
    // Partial list from Sigma.pm sigmaLensTypes — enough for test images
    let name = match hex_val {
        0x000 => "Sigma Lens (0x0)",
        0x100 => "Sigma Lens (0x100)",
        0x101 => "Sigma 18-50mm F3.5-5.6 DC",
        0x103 => "Sigma 18-125mm F3.5-5.6 DC",
        0x104 => "Sigma 18-200mm F3.5-6.3 DC",
        0x105 => "Sigma 24-60mm F2.8 EX DG",
        0x106 => "Sigma 17-70mm F2.8-4.5 DC Macro",
        0x107 => "Sigma 18-50mm F2.8 EX DC",
        0x108 => "Sigma 70-200mm F2.8 II EX DG APO Macro",
        0x109 => "Sigma 50-150mm F2.8 EX DC APO HSM",
        0x10a => "Sigma 28mm F1.8 EX DG",
        0x10b => "Sigma 70mm F2.8 EX DG Macro",
        0x10c => "Sigma 18-50mm F2.8 EX DC Macro",
        0x129 => "Sigma 14mm F2.8 EX Aspherical HSM",
        0x12c => "Sigma 20mm F1.8 EX DG Aspherical RF",
        0x130 => "Sigma 30mm F1.4 EX DC HSM",
        0x145 => "Sigma 15-30mm F3.5-4.5 EX DG Aspherical",
        0x146 => "Sigma 18-35mm F3.5-4.5 Aspherical",
        0x150 => "Sigma 50mm F2.8 EX DG Macro",
        0x151 => "Sigma 105mm F2.8 EX DG Macro",
        0x152 => "Sigma 180mm F3.5 EX DG APO HSM Macro",
        0x153 => "Sigma 150mm F2.8 EX DG HSM APO Macro",
        0x154 => "Sigma 10-20mm F4-5.6 EX DC",
        0x155 => "Sigma 12-24mm F4.5-5.6 EX DG Aspherical",
        0x156 => "Sigma 17-35mm F2.8-4 EX DG Aspherical",
        0x157 => "Sigma 24mm F1.8 EX DG Aspherical Macro",
        0x158 => "Sigma 28-70mm F2.8-4 DG",
        0x169 => "Sigma 70-300mm F4-5.6 APO DG Macro",
        0x184 => "Sigma 24-70mm F2.8 EX DG Macro",
        0x190 => "Sigma APO 70-200mm F2.8 EX DG",
        0x194 => "Sigma 300mm F2.8 APO EX DG HSM",
        0x195 => "Sigma 500mm F4.5 APO EX DG HSM",
        0x1a0 => "Sigma 24-135mm F2.8-4.5",
        _ => "",
    };
    if name.is_empty() {
        format!("Unknown ({:#x})", hex_val)
    } else {
        name.to_string()
    }
}

// ---------------------------------------------------------------------------
// Tag constructors
// ---------------------------------------------------------------------------

fn mk_tag_str(name: &str, desc: &str, value: String, f0: &str, f1: &str, f2: &str) -> Tag {
    let pv = value.clone();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: desc.to_string(),
        group: TagGroup {
            family0: f0.to_string(),
            family1: f1.to_string(),
            family2: f2.to_string(),
        },
        raw_value: Value::String(value),
        print_value: pv,
        priority: 0,
    }
}

fn mk_tag_u32(name: &str, desc: &str, value: u32, f0: &str, f1: &str, f2: &str) -> Tag {
    let pv = value.to_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: desc.to_string(),
        group: TagGroup {
            family0: f0.to_string(),
            family1: f1.to_string(),
            family2: f2.to_string(),
        },
        raw_value: Value::U32(value),
        print_value: pv,
        priority: 0,
    }
}

fn mk_tag_binary(name: &str, desc: &str, data: Vec<u8>, f0: &str, f1: &str, f2: &str) -> Tag {
    let pv = format!("(Binary data {} bytes, use -b option to extract)", data.len());
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: desc.to_string(),
        group: TagGroup {
            family0: f0.to_string(),
            family1: f1.to_string(),
            family2: f2.to_string(),
        },
        raw_value: Value::Binary(data),
        print_value: pv,
        priority: 0,
    }
}
