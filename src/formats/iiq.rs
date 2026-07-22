//! Phase One IIQ RAW format reader.

use super::misc::mktag;
use crate::error::Result;
use crate::tag::Tag;
use crate::value::Value;

/// Read a Phase One IIQ file.
///
/// `collapse_duplicates` mirrors the engine-wide gate in `exiftool.rs`: ExifTool's
/// CLI turns the Duplicates option on together with -ee, and every instance of a
/// tag must then be reported. The name-level pruning below reproduces what
/// ExifTool's Duplicates-off collapse does for this format, so it must be skipped
/// when duplicates are kept — an IIQ genuinely holds IFD0, IFD1 and PhaseOne
/// copies of ImageWidth/StripOffsets/SubfileType, and `exiftool -ee` prints them all.
pub fn read_iiq(data: &[u8], collapse_duplicates: bool) -> Result<Vec<Tag>> {
    // Read standard TIFF tags first
    let mut tags = crate::formats::tiff::read_tiff(data).unwrap_or_default();

    // ExifByteOrder is added by the outer exiftool.rs code already; drop the
    // TIFF reader's copy unconditionally (this is not a Duplicates decision).
    tags.retain(|t| t.name != "ExifByteOrder");

    // Build ThumbnailTIFF now, while both IFDs are still complete: RebuildTIFF
    // (Exif.pm:6139) reads every required tag from the group of the
    // reduced-resolution IFD, and the duplicate pruning below would strip IFD1's
    // copies of BitsPerSample & co.
    if let Some(t) = crate::composite::build_thumbnail_tiff(&tags) {
        tags.push(t);
    }

    // For IIQ, the TIFF file has two IFDs:
    // IFD0: has the main camera settings but thumbnail image data (1x1 placeholder)
    // IFD1: has the reduced-resolution thumbnail image
    // We need to:
    // 1. Remove SubfileType=0 from IFD0 (keep IFD1's Reduced-resolution image)
    // 2. Remove duplicate TIFF tags (keep first occurrence)
    // 3. Remove IFD0's StripOffsets/StripByteCounts (keep IFD1's)
    // 4. Remove 1x1 ImageWidth/ImageHeight placeholders
    if collapse_duplicates {
        // First pass: identify which tags to remove
        let mut seen_names: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut strip_offsets_count = 0;
        let mut strip_bytes_count = 0;
        let mut subfile_type_removed = false;
        let mut remove_indices: std::collections::HashSet<usize> = std::collections::HashSet::new();

        // Tags that may legitimately appear in both IFDs and we should keep BOTH
        let keep_both: std::collections::HashSet<&str> =
            ["SubfileType", "StripOffsets", "StripByteCounts"]
                .iter()
                .cloned()
                .collect();

        for (i, t) in tags.iter().enumerate() {
            // Remove the first SubfileType (IFD0's "full image" marker = value 0).
            // Match on the raw numeric value (the print value is now "Full-resolution image").
            if t.name == "SubfileType" && !subfile_type_removed {
                let is_zero = t.raw_value.as_u64() == Some(0)
                    || matches!(&t.raw_value, Value::String(v) if v == "0")
                    || t.print_value == "0"
                    || t.print_value == "Full-resolution image";
                if is_zero {
                    remove_indices.insert(i);
                    subfile_type_removed = true;
                    continue;
                }
            }
            // Remove first StripOffsets (IFD0's strip) and first StripByteCounts
            if t.name == "StripOffsets" {
                strip_offsets_count += 1;
                if strip_offsets_count == 1 {
                    remove_indices.insert(i);
                    continue;
                }
            }
            if t.name == "StripByteCounts" {
                strip_bytes_count += 1;
                if strip_bytes_count == 1 {
                    remove_indices.insert(i);
                    continue;
                }
            }
            // Remove 1x1 ImageWidth/Height placeholders
            if (t.name == "ImageWidth" || t.name == "ImageHeight") && t.print_value == "1" {
                remove_indices.insert(i);
                continue;
            }
            // Deduplicate other tags (keep first occurrence)
            if !keep_both.contains(t.name.as_str()) {
                if seen_names.contains(&t.name) {
                    remove_indices.insert(i);
                } else {
                    seen_names.insert(t.name.clone());
                }
            }
        }

        let mut new_tags = Vec::new();
        for (i, t) in tags.into_iter().enumerate() {
            if !remove_indices.contains(&i) {
                new_tags.push(t);
            }
        }
        tags = new_tags;
    }

    // PhaseOne block starts at offset 8 with "IIII" (LE) or "MMMM" (BE) magic
    if data.len() < 20 {
        return Ok(tags);
    }
    let is_le = &data[8..12] == b"IIII";
    let is_be = &data[8..12] == b"MMMM";
    if !is_le && !is_be {
        return Ok(tags);
    }

    let phaseone_start = 8usize;

    // IFD offset is at bytes 8..12 of the PhaseOne block (relative to phaseone_start)
    let ifd_offset_in_block = iiq_read_u32(data, phaseone_start + 8, is_le) as usize;
    let abs_ifd_start = phaseone_start + ifd_offset_in_block;
    if abs_ifd_start + 8 > data.len() {
        return Ok(tags);
    }

    let num_entries = iiq_read_u32(data, abs_ifd_start, is_le) as usize;
    if num_entries > 300 || abs_ifd_start + 8 + num_entries * 16 > data.len() {
        return Ok(tags);
    }

    let entry_start = abs_ifd_start + 8;
    let mut phaseone_tags: Vec<Tag> = Vec::new();

    for i in 0..num_entries {
        let off = entry_start + i * 16;
        let tag_id = iiq_read_u32(data, off, is_le);
        // fmt_size: 1=string, 2=int16s, 4=int32s (or float for specific tags)
        let _fmt_size = iiq_read_u32(data, off + 4, is_le);
        let size = iiq_read_u32(data, off + 8, is_le) as usize;
        let val_or_ptr = iiq_read_u32(data, off + 12, is_le) as usize;

        // Get raw bytes
        let raw: &[u8] = if size <= 4 {
            // Value is stored inline at offset 12, as little-endian u32
            let end = (off + 12 + size).min(data.len());
            &data[off + 12..end]
        } else {
            let abs_ptr = phaseone_start + val_or_ptr;
            if abs_ptr + size > data.len() {
                continue;
            }
            &data[abs_ptr..abs_ptr + size]
        };

        iiq_decode_tag(
            tag_id,
            raw,
            is_le,
            size,
            data,
            phaseone_start,
            &mut phaseone_tags,
        );
    }

    // Parse SensorCalibration sub-block (tag 0x0110)
    iiq_parse_sensor_calibration(
        data,
        phaseone_start,
        is_le,
        entry_start,
        num_entries,
        &mut phaseone_tags,
    );

    // The PhaseOne block is IFD0's maker note — `MakerNotePhaseOne`,
    // `SubDirectory => { TagTable => 'Image::ExifTool::PhaseOne::Main' }`
    // (MakerNotes.pm lines 840-852) — so ExifTool reads it while it is still in
    // IFD0, before it ever reaches IFD1. Splice it in there rather than at the
    // end: `%Image::ExifTool::PhaseOne::Main` states no PRIORITY, so these tags
    // meet the TIFF ones at equal priority and FoundTag's last-wins rule
    // (ExifTool.pm:9545-9570) decides every shared name — PhaseOne takes ISO
    // and FocalLength off ExifIFD, but IFD1, read afterwards, keeps
    // StripOffsets. The central duplicate arbitration in `exiftool.rs` replays
    // that rule, so nothing may be dropped here: doing so would be first-wins.
    let ifd1_start = tags
        .iter()
        .position(|t| t.group.family1 == "IFD1")
        .unwrap_or(tags.len());
    tags.splice(ifd1_start..ifd1_start, phaseone_tags);

    // Add composite tags: RedBalance, BlueBalance from WB_RGBLevels
    {
        let wb_val = tags
            .iter()
            .find(|t| t.name == "WB_RGBLevels")
            .map(|t| t.print_value.clone());
        if let Some(s) = wb_val {
            let parts: Vec<&str> = s.split_whitespace().collect();
            if parts.len() >= 3 {
                if let Ok(r) = parts[0].parse::<f64>() {
                    tags.push(mktag(
                        "Composite",
                        "RedBalance",
                        "Red Balance",
                        Value::String(format!("{:.5}", r)),
                    ));
                }
                if let Ok(b) = parts[2].parse::<f64>() {
                    tags.push(mktag(
                        "Composite",
                        "BlueBalance",
                        "Blue Balance",
                        Value::String(format!("{:.5}", b)),
                    ));
                }
            }
        }
    }

    Ok(tags)
}

/// Format f64 to match Perl's %.15g format (15 significant digits, no trailing zeros)
fn iiq_fmt_f64(v: f64) -> String {
    // Use %.15g equivalent: 15 significant digits
    let _s = format!("{:.15e}", v);
    // Parse the scientific notation and convert to %.15g style
    // Simpler: just format with enough precision and let Rust handle it
    // Actually use a direct approach: format with 14 decimal places in the mantissa
    // then strip trailing zeros
    let formatted = format!("{:.14}", v);
    // Strip trailing zeros after decimal point
    if formatted.contains('.') {
        let stripped = formatted.trim_end_matches('0').trim_end_matches('.');
        stripped.to_string()
    } else {
        formatted
    }
}

fn iiq_read_u32(data: &[u8], off: usize, is_le: bool) -> u32 {
    if off + 4 > data.len() {
        return 0;
    }
    if is_le {
        u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
    } else {
        u32::from_be_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
    }
}

fn iiq_read_f32(data: &[u8], off: usize, is_le: bool) -> f32 {
    if off + 4 > data.len() {
        return 0.0;
    }
    let bytes = [data[off], data[off + 1], data[off + 2], data[off + 3]];
    if is_le {
        f32::from_le_bytes(bytes)
    } else {
        f32::from_be_bytes(bytes)
    }
}

fn iiq_read_str(raw: &[u8]) -> String {
    // Read null-terminated string
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    crate::encoding::decode_utf8_or_latin1(&raw[..end])
        .trim()
        .to_string()
}

fn iiq_decode_tag(
    tag_id: u32,
    raw: &[u8],
    is_le: bool,
    size: usize,
    full_data: &[u8],
    phaseone_start: usize,
    tags: &mut Vec<Tag>,
) {
    let push = |tags: &mut Vec<Tag>, name: &str, desc: &str, val: String| {
        let mut tag = mktag("MakerNotes", name, desc, Value::String(val));
        // Image::ExifTool::PhaseOne::Main: family 1 is the module name.
        tag.group.family1 = "PhaseOne".into();
        tags.push(tag);
    };

    match tag_id {
        0x010f => {
            // RawData (binary)
            let display = format!("(Binary data {} bytes, use -b option to extract)", size);
            push(tags, "RawData", "Raw Data", display);
        }
        0x0100 => {
            // CameraOrientation
            let v = if raw.len() >= 4 {
                iiq_read_u32(raw, 0, is_le) & 0x03
            } else {
                0
            };
            let s = match v {
                0 => "Horizontal (normal)".to_string(),
                1 => "Rotate 90 CW".to_string(),
                2 => "Rotate 270 CW".to_string(),
                3 => "Rotate 180".to_string(),
                _ => v.to_string(),
            };
            push(tags, "CameraOrientation", "Camera Orientation", s);
        }
        0x0102 => {
            // SerialNumber (string)
            push(tags, "SerialNumber", "Serial Number", iiq_read_str(raw));
        }
        0x0105 => {
            // ISO
            let v = if raw.len() >= 4 {
                iiq_read_u32(raw, 0, is_le)
            } else {
                0
            };
            push(tags, "ISO", "ISO", v.to_string());
        }
        0x0106 => {
            // ColorMatrix1 (9 floats)
            if raw.len() >= 36 {
                let vals: Vec<f32> = (0..9).map(|i| iiq_read_f32(raw, i * 4, is_le)).collect();
                let s: Vec<String> = vals.iter().map(|v| format!("{:.3}", v)).collect();
                push(tags, "ColorMatrix1", "Color Matrix 1", s.join(" "));
            }
        }
        0x0107 => {
            // WB_RGBLevels (3 floats) - promote to f64 for Perl-compatible precision
            if raw.len() >= 12 {
                let r = iiq_read_f32(raw, 0, is_le) as f64;
                let g = iiq_read_f32(raw, 4, is_le) as f64;
                let b = iiq_read_f32(raw, 8, is_le) as f64;
                // Normalize so G=1
                let s = if g != 0.0 {
                    format!("{} {} {}", iiq_fmt_f64(r / g), 1.0f64, iiq_fmt_f64(b / g))
                } else {
                    format!("{} {} {}", iiq_fmt_f64(r), iiq_fmt_f64(g), iiq_fmt_f64(b))
                };
                push(tags, "WB_RGBLevels", "WB RGB Levels", s);
            }
        }
        0x0108 => {
            let v = if raw.len() >= 4 {
                iiq_read_u32(raw, 0, is_le)
            } else {
                0
            };
            push(tags, "SensorWidth", "Sensor Width", v.to_string());
        }
        0x0109 => {
            let v = if raw.len() >= 4 {
                iiq_read_u32(raw, 0, is_le)
            } else {
                0
            };
            push(tags, "SensorHeight", "Sensor Height", v.to_string());
        }
        0x010a => {
            let v = if raw.len() >= 4 {
                iiq_read_u32(raw, 0, is_le)
            } else {
                0
            };
            push(
                tags,
                "SensorLeftMargin",
                "Sensor Left Margin",
                v.to_string(),
            );
        }
        0x010b => {
            let v = if raw.len() >= 4 {
                iiq_read_u32(raw, 0, is_le)
            } else {
                0
            };
            push(tags, "SensorTopMargin", "Sensor Top Margin", v.to_string());
        }
        0x010c => {
            let v = if raw.len() >= 4 {
                iiq_read_u32(raw, 0, is_le)
            } else {
                0
            };
            push(tags, "ImageWidth", "Image Width", v.to_string());
        }
        0x010d => {
            let v = if raw.len() >= 4 {
                iiq_read_u32(raw, 0, is_le)
            } else {
                0
            };
            push(tags, "ImageHeight", "Image Height", v.to_string());
        }
        0x010e => {
            // RawFormat
            let v = if raw.len() >= 4 {
                iiq_read_u32(raw, 0, is_le)
            } else {
                0
            };
            let s = match v {
                0 => "Uncompressed".to_string(),
                1 => "RAW 1".to_string(),
                2 => "RAW 2".to_string(),
                3 => "IIQ L".to_string(),
                5 => "IIQ S".to_string(),
                6 => "IIQ Sv2".to_string(),
                8 => "IIQ L16".to_string(),
                _ => v.to_string(),
            };
            push(tags, "RawFormat", "Raw Format", s);
        }
        0x0112 => {
            // DateTimeOriginal (PhaseOne.pm:97): int32u Unix time,
            // `ValueConv => 'ConvertUnixTime($val)'` (UTC — ExifTool only uses
            // local time when ConvertUnixTime is called with $isLocal set).
            if raw.len() >= 4 {
                let v = iiq_read_u32(raw, 0, is_le);
                push(
                    tags,
                    "DateTimeOriginal",
                    "Date/Time Original",
                    crate::metadata::makernotes::unix_time_to_datetime(v),
                );
                // `Priority => 0` (PhaseOne.pm line 102), so this copy never
                // displaces the EXIF DateTimeOriginal read before it.
                if let Some(t) = tags.last_mut() {
                    t.priority = crate::tag::PRIORITY_EXPLICIT_ZERO;
                }
            }
        }
        0x0113 => {
            let v = if raw.len() >= 4 {
                iiq_read_u32(raw, 0, is_le)
            } else {
                0
            };
            push(tags, "ImageNumber", "Image Number", v.to_string());
        }
        0x0203 => {
            push(tags, "Software", "Software", iiq_read_str(raw));
        }
        0x0204 => {
            push(tags, "System", "System", iiq_read_str(raw));
        }
        0x0210 => {
            // SensorTemperature (float)
            let v = if raw.len() >= 4 {
                iiq_read_f32(raw, 0, is_le)
            } else {
                0.0
            };
            push(
                tags,
                "SensorTemperature",
                "Sensor Temperature",
                format!("{:.2} C", v),
            );
        }
        0x0211 => {
            // SensorTemperature2 (float)
            let v = if raw.len() >= 4 {
                iiq_read_f32(raw, 0, is_le)
            } else {
                0.0
            };
            push(
                tags,
                "SensorTemperature2",
                "Sensor Temperature 2",
                format!("{:.2} C", v),
            );
        }
        0x021c => {
            // StripOffsets (PhaseOne.pm:144): `{ Name => 'StripOffsets', Binary => 1 }`.
            // The table FORMAT is int32s (PhaseOne.pm:32), so the value is the
            // space-joined list of int32s; the "Binary data N bytes" message counts
            // the characters of that converted string, as for BlackLevelData.
            let count = raw.len() / 4;
            if count > 0 {
                let s = (0..count)
                    .map(|i| iiq_read_u32(raw, i * 4, is_le) as i32)
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(" ");
                let display = format!("(Binary data {} bytes, use -b option to extract)", s.len());
                push(tags, "StripOffsets", "Strip Offsets", display);
            }
        }
        0x021d => {
            // BlackLevel
            let v = if raw.len() >= 4 {
                iiq_read_u32(raw, 0, is_le)
            } else {
                0
            };
            push(tags, "BlackLevel", "Black Level", v.to_string());
        }
        0x0222 => {
            // SplitColumn
            let v = if raw.len() >= 4 {
                iiq_read_u32(raw, 0, is_le)
            } else {
                0
            };
            push(tags, "SplitColumn", "Split Column", v.to_string());
        }
        0x0223 => {
            // BlackLevelData (int16u, binary)
            // Format as space-separated int16u values (matching Perl Binary output)
            let count = raw.len() / 2;
            if count > 0 {
                let vals: Vec<String> = (0..count)
                    .map(|i| {
                        let v = if is_le {
                            u16::from_le_bytes([raw[i * 2], raw[i * 2 + 1]])
                        } else {
                            u16::from_be_bytes([raw[i * 2], raw[i * 2 + 1]])
                        };
                        v.to_string()
                    })
                    .collect();
                let s = vals.join(" ");
                let display = format!("(Binary data {} bytes, use -b option to extract)", s.len());
                push(tags, "BlackLevelData", "Black Level Data", display);
            } else {
                push(
                    tags,
                    "BlackLevelData",
                    "Black Level Data",
                    format!(
                        "(Binary data {} bytes, use -b option to extract)",
                        raw.len()
                    ),
                );
            }
        }
        0x0226 => {
            // ColorMatrix2 (9 floats)
            if raw.len() >= 36 {
                let vals: Vec<f32> = (0..9).map(|i| iiq_read_f32(raw, i * 4, is_le)).collect();
                let s: Vec<String> = vals.iter().map(|v| format!("{:.3}", v)).collect();
                push(tags, "ColorMatrix2", "Color Matrix 2", s.join(" "));
            }
        }
        0x0301 => {
            // FirmwareVersions (string)
            push(
                tags,
                "FirmwareVersions",
                "Firmware Versions",
                iiq_read_str(raw),
            );
        }
        0x0400 => {
            // ShutterSpeedValue (float, convert: 2**(-val))
            let v = if raw.len() >= 4 {
                iiq_read_f32(raw, 0, is_le)
            } else {
                0.0
            };
            let exposure = if v.abs() < 100.0 {
                2.0f32.powf(-v)
            } else {
                0.0
            };
            // Format as fraction
            let s = iiq_format_exposure_time(exposure);
            push(tags, "ShutterSpeedValue", "Shutter Speed Value", s);
        }
        0x0401 => {
            // ApertureValue (float, convert: 2**(val/2))
            let v = if raw.len() >= 4 {
                iiq_read_f32(raw, 0, is_le)
            } else {
                0.0
            };
            let aperture = 2.0f32.powf(v / 2.0);
            push(
                tags,
                "ApertureValue",
                "Aperture Value",
                format!("{:.1}", aperture),
            );
        }
        0x0403 => {
            // FocalLength (float)
            let v = if raw.len() >= 4 {
                iiq_read_f32(raw, 0, is_le)
            } else {
                0.0
            };
            push(tags, "FocalLength", "Focal Length", format!("{:.1} mm", v));
        }
        0x0412 => {
            // LensModel (string)
            push(tags, "LensModel", "Lens Model", iiq_read_str(raw));
        }
        _ => {}
    }

    let _ = (full_data, phaseone_start); // suppress unused warnings
}

fn iiq_format_exposure_time(t: f32) -> String {
    if t <= 0.0 {
        return "0".to_string();
    }
    if t >= 1.0 {
        // Whole seconds or more
        let rounded = t.round() as u32;
        if (t - rounded as f32).abs() < 0.05 {
            return rounded.to_string();
        }
        return format!("{:.1}", t);
    }
    // Express as 1/N fraction
    let n = (1.0 / t).round() as u32;
    format!("1/{}", n)
}

fn iiq_parse_sensor_calibration(
    data: &[u8],
    phaseone_start: usize,
    is_le: bool,
    entry_start: usize,
    num_entries: usize,
    tags: &mut Vec<Tag>,
) {
    // Find tag 0x0110 (SensorCalibration sub-block)
    for i in 0..num_entries {
        let off = entry_start + i * 16;
        let tag_id = iiq_read_u32(data, off, is_le);
        if tag_id != 0x0110 {
            continue;
        }
        let size = iiq_read_u32(data, off + 8, is_le) as usize;
        let val_or_ptr = iiq_read_u32(data, off + 12, is_le) as usize;
        if size <= 4 {
            return;
        }

        let abs_ptr = phaseone_start + val_or_ptr;
        if abs_ptr + size > data.len() {
            return;
        }
        let sub = &data[abs_ptr..abs_ptr + size];

        // SensorCalibration sub-block: starts with IIII\\x01\\x00\\x00\\x00 or MMMM\\x00\\x00\\x00\\x01
        if sub.len() < 12 {
            return;
        }
        let sub_is_le = &sub[0..4] == b"IIII";
        let sub_is_be = &sub[0..4] == b"MMMM";
        if !sub_is_le && !sub_is_be {
            return;
        }

        let sub_ifd_off = iiq_read_u32(sub, 8, sub_is_le) as usize;
        if sub_ifd_off + 8 > sub.len() {
            return;
        }

        let num_sub = iiq_read_u32(sub, sub_ifd_off, sub_is_le) as usize;
        if num_sub > 300 {
            return;
        }
        let sub_entry_start = sub_ifd_off + 8;
        if sub_entry_start + num_sub * 12 > sub.len() {
            return;
        }

        // SensorCalibration uses 12-byte entries (no format field)
        for j in 0..num_sub {
            let eoff = sub_entry_start + j * 12;
            let etag = iiq_read_u32(sub, eoff, sub_is_le);
            let esize = iiq_read_u32(sub, eoff + 4, sub_is_le) as usize;
            let eval_ptr = iiq_read_u32(sub, eoff + 8, sub_is_le) as usize;

            if etag == 0x0400 {
                // SensorDefects (binary undef)
                let display = format!("(Binary data {} bytes, use -b option to extract)", esize);
                let mut tag = mktag(
                    "MakerNotes",
                    "SensorDefects",
                    "Sensor Defects",
                    Value::String(display),
                );
                tag.group.family1 = "PhaseOne".into();
                tags.push(tag);
            } else if etag == 0x0407 {
                // SensorCalibration SerialNumber (PhaseOne.pm:312), a second
                // copy of the 0x0102 value that ExifTool reports as well.
                // For the 12-byte sub-IFD entry the value pointer is the last
                // int32u of the entry and is relative to the sub-block start
                // (`$valuePtr += $dirStart`, PhaseOne.pm:670).
                if esize > 4 {
                    if eval_ptr + esize <= sub.len() {
                        let mut tag = mktag(
                            "MakerNotes",
                            "SerialNumber",
                            "Serial Number",
                            Value::String(iiq_read_str(&sub[eval_ptr..eval_ptr + esize])),
                        );
                        tag.group.family1 = "PhaseOne".into();
                        tags.push(tag);
                    }
                } else if eoff + 8 + esize <= sub.len() {
                    let mut tag = mktag(
                        "MakerNotes",
                        "SerialNumber",
                        "Serial Number",
                        Value::String(iiq_read_str(&sub[eoff + 8..eoff + 8 + esize])),
                    );
                    tag.group.family1 = "PhaseOne".into();
                    tags.push(tag);
                }
            }
        }
        return;
    }
}
