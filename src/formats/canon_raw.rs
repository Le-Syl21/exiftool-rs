//! Canon CRW (CIFF) file format reader.
//!
//! Parses CIFF (Camera Image File Format) blocks used in Canon's legacy CRW files.
//! Mirrors ExifTool's CanonRaw.pm.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_crw(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 14 {
        return Err(Error::InvalidData("file too small for CRW".into()));
    }

    // Byte order (first 2 bytes)
    let is_le = data[0] == b'I' && data[1] == b'I';
    if !is_le && !(data[0] == b'M' && data[1] == b'M') {
        return Err(Error::InvalidData("invalid CRW byte order".into()));
    }

    // Header length
    let hlen = if is_le {
        u32::from_le_bytes([data[2], data[3], data[4], data[5]]) as usize
    } else {
        u32::from_be_bytes([data[2], data[3], data[4], data[5]]) as usize
    };

    // Validate HEAP signature
    if hlen < 14 || data.len() < hlen || &data[6..10] != b"HEAP" {
        return Err(Error::InvalidData("invalid CRW HEAP signature".into()));
    }

    let mut tags = Vec::new();

    // The root directory starts after the header and spans the rest of the file
    parse_ciff_dir(data, hlen, data.len(), is_le, &mut tags, 0);

    Ok(tags)
}

fn parse_ciff_dir(
    data: &[u8],
    block_start: usize,
    block_end: usize,
    is_le: bool,
    tags: &mut Vec<Tag>,
    depth: u32,
) {
    if depth > 10 || block_end <= block_start || block_end > data.len() {
        return;
    }

    // Last 4 bytes of block contain directory offset (relative to block_start)
    if block_end < block_start + 4 {
        return;
    }
    let dir_offset = read_u32(data, block_end - 4, is_le) as usize + block_start;

    if dir_offset + 2 > block_end {
        return;
    }

    let num_entries = read_u16(data, dir_offset, is_le) as usize;
    let mut pos = dir_offset + 2;

    for _ in 0..num_entries {
        if pos + 10 > block_end {
            break;
        }

        let raw_tag = read_u16(data, pos, is_le);
        let size_field = read_u32(data, pos + 2, is_le) as usize;
        let value_offset = read_u32(data, pos + 6, is_le) as usize;
        let entry_pos = pos; // save for valueInDir case
        pos += 10;

        // From Perl CanonRaw.pm:
        // $tagID = $tag & 0x3fff
        // $tagType = ($tag >> 8) & 0x38
        // $valueInDir = ($tag & 0x4000) -- value stored inline in dir entry
        if (raw_tag & 0x8000) != 0 { continue; } // bad entry

        let tag_id = raw_tag & 0x3FFF;
        let data_type = (raw_tag >> 8) & 0x38;
        let value_in_dir = (raw_tag & 0x4000) != 0;

        // Subdirectory check: type 0x28 or 0x30 AND not valueInDir
        if (data_type == 0x28 || data_type == 0x30) && !value_in_dir {
            let abs_offset = value_offset + block_start;
            if abs_offset + size_field <= block_end {
                parse_ciff_dir(data, abs_offset, abs_offset + size_field, is_le, tags, depth + 1);
            }
            continue;
        }

        // Determine value data
        let (value_data, _size): (&[u8], usize) = if value_in_dir {
            // Value stored in directory entry: 8 bytes (size_field + value_offset fields)
            if entry_pos + 2 + 8 > data.len() { continue; }
            (&data[entry_pos + 2..entry_pos + 10], 8)
        } else {
            let abs_offset = value_offset + block_start;
            if abs_offset + size_field > data.len() { continue; }
            (&data[abs_offset..abs_offset + size_field], size_field)
        };

        let (name, description) = crw_tag_name(tag_id);
        if name.is_empty() {
            continue;
        }

        // Some CIFF tags have SubDirectory → binary data tables (from Perl CanonRaw.pm).
        // For those, parse the binary data and emit sub-tags instead of the raw container.
        if parse_ciff_binary_subdir(tag_id, value_data, is_le, tags) {
            continue; // sub-tags emitted, skip emitting the container tag
        }

        let value = match data_type {
            0x00 => {
                // Raw bytes / string
                let s = String::from_utf8_lossy(value_data)
                    .trim_end_matches('\0')
                    .to_string();
                if s.chars().all(|c| c.is_ascii_graphic() || c.is_ascii_whitespace()) && !s.is_empty() {
                    Value::String(s)
                } else {
                    Value::Binary(value_data.to_vec())
                }
            }
            0x08 => {
                // ASCII string
                Value::String(
                    String::from_utf8_lossy(value_data)
                        .trim_end_matches('\0')
                        .to_string(),
                )
            }
            0x10 => {
                // int16u: extract first 2 bytes (value may be in 8-byte inline block)
                if value_data.len() >= 2 {
                    Value::U16(read_u16(value_data, 0, is_le))
                } else {
                    Value::Binary(value_data.to_vec())
                }
            }
            0x18 => {
                // int32u: extract first 4 bytes (value may be in 8-byte inline block)
                if value_data.len() >= 4 {
                    Value::U32(read_u32(value_data, 0, is_le))
                } else {
                    Value::Binary(value_data.to_vec())
                }
            }
            _ => Value::Binary(value_data.to_vec()),
        };

        let raw_print = value.to_display_string();
        // Apply tag-specific print conversions from Perl CanonRaw.pm
        let print_value = match tag_id {
            0x1817 => {
                // FileNumber: PrintConv => '$_=$val;s/(\d+)(\d{4})/$1-$2/;$_'
                // Splits number so last 4 digits become a suffix after dash
                let n: u64 = raw_print.parse().unwrap_or(0);
                if n >= 10000 {
                    let prefix = n / 10000;
                    let suffix = n % 10000;
                    format!("{}-{:04}", prefix, suffix)
                } else {
                    raw_print
                }
            }
            _ => raw_print,
        };
        tags.push(Tag {
            id: TagId::Numeric(tag_id),
            name: name.to_string(),
            description: description.to_string(),
            group: TagGroup {
                family0: "CanonRaw".into(),
                family1: "CanonRaw".into(),
                family2: "Camera".into(),
            },
            raw_value: value,
            print_value,
            priority: 0,
        });
    }
}

/// Parse a CIFF tag that has a SubDirectory pointing to a binary data table.
/// Returns true if the tag was handled (sub-tags emitted), false otherwise.
/// Based on Perl CanonRaw.pm SubDirectory/ProcessBinaryData tables.
fn parse_ciff_binary_subdir(tag_id: u16, data: &[u8], is_le: bool, tags: &mut Vec<Tag>) -> bool {
    let mk = |name: &str, val: String| -> Tag {
        Tag {
            id: TagId::Text(name.into()),
            name: name.into(),
            description: name.into(),
            group: TagGroup {
                family0: "CanonRaw".into(),
                family1: "CanonRaw".into(),
                family2: "Camera".into(),
            },
            raw_value: Value::String(val.clone()),
            print_value: val,
            priority: 0,
        }
    };
    let rf32 = |d: &[u8], off: usize| -> f32 {
        if off + 4 > d.len() { return 0.0; }
        if is_le { f32::from_le_bytes([d[off], d[off+1], d[off+2], d[off+3]]) }
        else { f32::from_be_bytes([d[off], d[off+1], d[off+2], d[off+3]]) }
    };
    let ru32 = |d: &[u8], off: usize| -> u32 {
        if off + 4 > d.len() { return 0; }
        if is_le { u32::from_le_bytes([d[off], d[off+1], d[off+2], d[off+3]]) }
        else { u32::from_be_bytes([d[off], d[off+1], d[off+2], d[off+3]]) }
    };
    let ri32 = |d: &[u8], off: usize| -> i32 { ru32(d, off) as i32 };
    let _ru16 = |d: &[u8], off: usize| -> u16 {
        if off + 2 > d.len() { return 0; }
        if is_le { u16::from_le_bytes([d[off], d[off+1]]) }
        else { u16::from_be_bytes([d[off], d[off+1]]) }
    };

    match tag_id {
        0x1803 => {
            // ImageFormat (SubDirectory → CanonRaw::ImageFormat, FORMAT=int32u)
            // 0=FileFormat, 1=TargetCompressionRatio(float)
            if data.len() >= 4 {
                let file_format = ru32(data, 0);
                let fmt_str = match file_format {
                    0x00010000 => "65536".to_string(), // Perl shows raw value, PrintConv shows text
                    _ => file_format.to_string(),
                };
                tags.push(mk("FileFormat", fmt_str));
            }
            if data.len() >= 8 {
                let ratio = rf32(data, 4);
                let s = format!("{}", ratio);
                tags.push(mk("TargetCompressionRatio", s));
            }
            true
        }
        0x1810 => {
            // ImageInfo (SubDirectory → CanonRaw::ImageInfo, FORMAT=int32u)
            // Indices: 0=ImageWidth, 1=ImageHeight, 2=PixelAspectRatio(float),
            //          3=Rotation(int32s), 4=ComponentBitDepth, 5=ColorBitDepth, 6=ColorBW
            if data.len() >= 4 { tags.push(mk("ImageWidth", ru32(data, 0).to_string())); }
            if data.len() >= 8 { tags.push(mk("ImageHeight", ru32(data, 4).to_string())); }
            if data.len() >= 12 {
                let aspect = rf32(data, 8); // PixelAspectRatio is float
                let s = format!("{}", aspect);
                tags.push(mk("PixelAspectRatio", s));
            }
            if data.len() >= 16 {
                let rot = ri32(data, 12);
                tags.push(mk("Rotation", rot.to_string()));
            }
            if data.len() >= 20 { tags.push(mk("ComponentBitDepth", ru32(data, 16).to_string())); }
            if data.len() >= 24 { tags.push(mk("ColorBitDepth", ru32(data, 20).to_string())); }
            if data.len() >= 28 { tags.push(mk("ColorBW", ru32(data, 24).to_string())); }
            true
        }
        0x1813 => {
            // FlashInfo (SubDirectory → CanonRaw::FlashInfo, FORMAT=float)
            // 0=FlashGuideNumber, 1=FlashThreshold
            if data.len() >= 4 { tags.push(mk("FlashGuideNumber", format!("{}", rf32(data, 0)))); }
            if data.len() >= 8 { tags.push(mk("FlashThreshold", format!("{}", rf32(data, 4)))); }
            true
        }
        0x1814 => {
            // MeasuredEV (NOT a SubDirectory; single float with ValueConv $val+5)
            if data.len() >= 4 {
                let raw = rf32(data, 0);
                let val = raw + 5.0;
                tags.push(mk("MeasuredEV", format!("{}", val)));
            }
            true
        }
        0x180e => {
            // TimeStamp (SubDirectory → CanonRaw::TimeStamp, FORMAT=int32u)
            // 0=DateTimeOriginal(unix time), 1=TimeZoneCode(int32s, /3600), 2=TimeZoneInfo
            if data.len() >= 4 {
                // DateTimeOriginal: unix time → we'll just show as raw for now
                // (Perl: ValueConv => 'ConvertUnixTime($val)')
                // Showing as integer since we don't want to add date parsing complexity here
            }
            if data.len() >= 8 {
                let tz_raw = ri32(data, 4);
                let tz_hours = tz_raw as f64 / 3600.0;
                let tz_str = if tz_hours == tz_hours.floor() {
                    format!("{}", tz_hours as i64)
                } else {
                    format!("{}", tz_hours)
                };
                tags.push(mk("TimeZoneCode", tz_str));
            }
            if data.len() >= 12 {
                tags.push(mk("TimeZoneInfo", ru32(data, 8).to_string()));
            }
            true
        }
        0x1818 => {
            // ExposureInfo (SubDirectory → CanonRaw::ExposureInfo, FORMAT=float)
            // 0=ExposureCompensation, 1=ShutterSpeedValue, 2=ApertureValue
            // These are complex conversions; for now just skip
            true
        }
        0x1029 => {
            // CanonFocalLength (SubDirectory → Canon::FocalLength, FORMAT=int16u)
            // 0=FocalType (PrintConv: 1=Fixed, 2=Zoom)
            // 1=FocalLength (ValueConv val/FocalUnits; has Priority=0 so EXIF takes precedence, skip)
            // 2=FocalPlaneXSize (int16u, ValueConv val*25.4/1000, only for certain models)
            // 3=FocalPlaneYSize (int16u, ValueConv val*25.4/1000, only for certain models)
            let ru16l = |d: &[u8], off: usize| -> u16 {
                if off + 2 > d.len() { return 0; }
                if is_le { u16::from_le_bytes([d[off], d[off+1]]) }
                else { u16::from_be_bytes([d[off], d[off+1]]) }
            };
            if data.len() >= 2 {
                let focal_type = ru16l(data, 0);
                let ft_str = match focal_type {
                    1 => "Fixed".to_string(),
                    2 => "Zoom".to_string(),
                    _ => focal_type.to_string(),
                };
                // RawConv: '$val ? $val : undef' — skip if zero
                if focal_type != 0 {
                    tags.push(mk("FocalType", ft_str));
                }
            }
            // Skip FocalLength (index 1) — EXIF has it with higher priority
            if data.len() >= 6 {
                // FocalPlaneXSize at index 2, FocalPlaneYSize at index 3
                // ValueConv: val * 25.4 / 1000 (convert 1/1000 inch to mm)
                // RawConv: '$val < 40 ? undef : $val' — skip if < 40
                // PrintConv: sprintf("%.2f mm", $val) — 2 decimal places for display
                // Store full precision in raw_value for composite calculations
                let x_raw = ru16l(data, 4);
                if x_raw >= 40 {
                    let x_mm = x_raw as f64 * 25.4 / 1000.0;
                    let print_str = format!("{:.2} mm", x_mm);
                    tags.push(Tag {
                        id: TagId::Text("FocalPlaneXSize".into()),
                        name: "FocalPlaneXSize".into(),
                        description: "Focal Plane X Size".into(),
                        group: TagGroup { family0: "CanonRaw".into(), family1: "CanonRaw".into(), family2: "Camera".into() },
                        raw_value: Value::F64(x_mm),
                        print_value: print_str,
                        priority: 0,
                    });
                }
            }
            if data.len() >= 8 {
                let y_raw = ru16l(data, 6);
                if y_raw >= 40 {
                    let y_mm = y_raw as f64 * 25.4 / 1000.0;
                    let print_str = format!("{:.2} mm", y_mm);
                    tags.push(Tag {
                        id: TagId::Text("FocalPlaneYSize".into()),
                        name: "FocalPlaneYSize".into(),
                        description: "Focal Plane Y Size".into(),
                        group: TagGroup { family0: "CanonRaw".into(), family1: "CanonRaw".into(), family2: "Camera".into() },
                        raw_value: Value::F64(y_mm),
                        print_value: print_str,
                        priority: 0,
                    });
                }
            }
            true
        }
        _ => false,
    }
}

fn crw_tag_name(tag_id: u16) -> (&'static str, &'static str) {
    // Tag IDs from Perl CanonRaw.pm (tag_id & 0x3FFF strips the data-type bits)
    match tag_id & 0x3FFF {
        0x0000 => ("NullRecord", "Null Record"),
        0x0032 => ("CanonColorInfo1", "Color Info 1"),
        0x0805 => ("CanonFileDescription", "File Description"),
        0x080a => ("", ""),  // CanonRawMakeModel: combined make/model string, not emitted directly
        0x080b => ("CanonFirmwareVersion", "Firmware Version"),
        0x080c => ("ComponentVersion", "Component Version"),
        0x080d => ("ROMOperationMode", "ROM Operation Mode"),
        0x0810 => ("OwnerName", "Owner Name"),
        0x0815 => ("CanonImageType", "Image Type"),
        0x0816 => ("OriginalFileName", "Original File Name"),
        0x0817 => ("ThumbnailFileName", "Thumbnail File Name"),
        0x100a => ("TargetImageType", "Target Image Type"),
        0x1010 => ("ShutterReleaseMethod", "Shutter Release Method"),
        0x1011 => ("ShutterReleaseTiming", "Shutter Release Timing"),
        0x1016 => ("ReleaseSetting", "Release Setting"),
        0x101c => ("BaseISO", "Base ISO"),
        0x1026 => ("", ""),  // unknown, skip
        0x1029 => ("CanonFocalLength", "Focal Length"),
        0x102a => ("CanonShotInfo", "Shot Info"),
        0x102d => ("CanonCameraSettings", "Camera Settings"),
        0x1031 => ("SensorInfo", "Sensor Info"),
        0x1038 => ("CanonAFInfo", "AF Info"),
        0x1093 => ("CanonFileInfo", "File Info"),
        0x10a9 => ("ColorBalance", "Color Balance"),
        0x10b4 => ("ColorSpace", "Color Space"),
        0x10b5 => ("RawJpgInfo", "Raw Jpg Info"),
        0x1803 => ("ImageFormat", "Image Format"),
        0x1804 => ("RecordID", "Record ID"),
        0x1806 => ("SelfTimerTime", "Self Timer Time"),
        0x1807 => ("TargetDistanceSetting", "Target Distance Setting"),
        0x180e => ("TimeStamp", "Time Stamp"),
        0x1810 => ("ImageInfo", "Image Info"),
        0x1813 => ("FlashInfo", "Flash Info"),
        0x1814 => ("MeasuredEV", "Measured EV"),
        0x1817 => ("FileNumber", "File Number"),
        0x1818 => ("ExposureInfo", "Exposure Info"),
        0x1834 => ("CanonModelID", "Model ID"),
        0x1835 => ("DecoderTable", "Decoder Table"),
        0x183b => ("SerialNumber", "Serial Number"),
        0x3002 => ("ShootingRecord", "Shooting Record"),
        0x3003 => ("MeasuredInfo", "Measured Info"),
        0x3004 => ("ColorInfo", "Color Info"),
        _ => ("", ""),
    }
}

fn read_u16(data: &[u8], offset: usize, is_le: bool) -> u16 {
    if is_le {
        u16::from_le_bytes([data[offset], data[offset + 1]])
    } else {
        u16::from_be_bytes([data[offset], data[offset + 1]])
    }
}

fn read_u32(data: &[u8], offset: usize, is_le: bool) -> u32 {
    if is_le {
        u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]])
    } else {
        u32::from_be_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]])
    }
}
