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
    parse_ciff_dir(data, hlen, data.len(), is_le, &mut tags, 0, false);

    // Scan for embedded XMP trailer (outside the CIFF structure).
    // Perl ExifTool calls ProcessTrailers which finds XMP appended after the CIFF block.
    if let Some(xmp_start) = find_xmp(data) {
        if let Some(xmp_end_raw) = find_xmp_end(data, xmp_start) {
            let xmp_data = &data[xmp_start..xmp_end_raw];
            if let Ok(xmp_tags) = crate::metadata::xmp::XmpReader::read(xmp_data) {
                tags.extend(xmp_tags);
            }
        }
    }

    Ok(tags)
}

/// Find the start of an XMP packet in the data.
fn find_xmp(data: &[u8]) -> Option<usize> {
    let marker = b"<?xpacket begin";
    // Search from the start, but skip the first 26 bytes (header)
    let start = 26;
    if data.len() <= start { return None; }
    data[start..].windows(marker.len())
        .position(|w| w == marker)
        .map(|p| p + start)
}

/// Find the end of an XMP packet (after <?xpacket end ...?>).
fn find_xmp_end(data: &[u8], xmp_start: usize) -> Option<usize> {
    let end_marker = b"<?xpacket end";
    if xmp_start >= data.len() { return None; }
    let after_start = &data[xmp_start..];
    if let Some(pos) = after_start.windows(end_marker.len()).position(|w| w == end_marker) {
        // Find the '?>' closing the processing instruction
        let close_search_start = xmp_start + pos + end_marker.len();
        if let Some(close_pos) = data[close_search_start..].windows(2).position(|w| w == b"?>") {
            return Some(close_search_start + close_pos + 2);
        }
        // Fallback: just return end of the end marker
        return Some(xmp_start + pos + end_marker.len());
    }
    None
}

fn parse_ciff_dir(
    data: &[u8],
    block_start: usize,
    block_end: usize,
    is_le: bool,
    tags: &mut Vec<Tag>,
    depth: u32,
    in_image_description: bool,
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
                // Detect ImageDescription directory (0x2804) to handle 0x0805 correctly
                let is_image_desc = tag_id == 0x2804;
                parse_ciff_dir(data, abs_offset, abs_offset + size_field, is_le, tags, depth + 1, is_image_desc);
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

        // Some CIFF tags have SubDirectory → binary data tables (from Perl CanonRaw.pm).
        // Check this BEFORE the name check, since sub-dir tags have empty names.
        if parse_ciff_binary_subdir(tag_id, value_data, is_le, tags) {
            continue; // sub-tags emitted, skip emitting the container tag
        }

        // Binary/large data tags: emit with "(Binary data N bytes)" value
        // Perl emits these as Binary when size > 512 or Binary=1
        match tag_id {
            0x2005 => {
                // RawData - always emit as binary
                let pv = format!("(Binary data {} bytes, use -b option to extract)", value_data.len());
                tags.push(Tag {
                    id: TagId::Numeric(tag_id),
                    name: "RawData".into(),
                    description: "Raw Data".into(),
                    group: TagGroup { family0: "MakerNotes".into(), family1: "MakerNotes".into(), family2: "Camera".into() },
                    raw_value: Value::Binary(value_data.to_vec()),
                    print_value: pv,
                    priority: 0,
                });
                continue;
            }
            0x2007 => {
                // JpgFromRaw - always emit as binary
                let pv = format!("(Binary data {} bytes, use -b option to extract)", value_data.len());
                tags.push(Tag {
                    id: TagId::Numeric(tag_id),
                    name: "JpgFromRaw".into(),
                    description: "Jpg From Raw".into(),
                    group: TagGroup { family0: "MakerNotes".into(), family1: "MakerNotes".into(), family2: "Camera".into() },
                    raw_value: Value::Binary(value_data.to_vec()),
                    print_value: pv,
                    priority: 0,
                });
                continue;
            }
            0x2008 => {
                // ThumbnailImage - always emit as binary
                let pv = format!("(Binary data {} bytes, use -b option to extract)", value_data.len());
                tags.push(Tag {
                    id: TagId::Numeric(tag_id),
                    name: "ThumbnailImage".into(),
                    description: "Thumbnail Image".into(),
                    group: TagGroup { family0: "MakerNotes".into(), family1: "MakerNotes".into(), family2: "Camera".into() },
                    raw_value: Value::Binary(value_data.to_vec()),
                    print_value: pv,
                    priority: 0,
                });
                continue;
            }
            _ => {}
        }

        // Tag 0x0805 has two meanings based on directory context:
        // - In ImageDescription directory: CanonFileDescription
        // - Elsewhere: UserComment
        let (name, description) = if tag_id == 0x0805 {
            if in_image_description {
                ("CanonFileDescription", "File Description")
            } else {
                ("UserComment", "User Comment")
            }
        } else {
            crw_tag_name(tag_id)
        };
        if name.is_empty() {
            continue;
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
            0x180b => {
                // SerialNumber: for EOS models: sprintf("%.10d",$val) = zero-padded 10 digits
                // For EOS D30: sprintf("%x-%.5d", $val>>16, $val & 0xffff)
                // For CRW files, assume EOS (non-D30), so use 10-digit format
                let n: u64 = raw_print.parse().unwrap_or(0);
                format!("{:010}", n)
            }
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
            0x183b => {
                // SerialNumberFormat: PrintConv with hex values
                let n: u32 = raw_print.parse().unwrap_or(0);
                match n {
                    0x90000000 => "Format 1".to_string(),
                    0xa0000000 => "Format 2".to_string(),
                    _ => format!("0x{:08x}", n),
                }
            }
            0x1834 => {
                // CanonModelID: PrintConv from Canon::canonModelID table
                let n: u32 = raw_print.parse().unwrap_or(0);
                canon_model_id_str(n)
            }
            0x100a => {
                // TargetImageType: PrintConv
                match raw_print.parse::<u32>().unwrap_or(99) {
                    0 => "Real-world Subject".to_string(),
                    1 => "Written Document".to_string(),
                    _ => raw_print,
                }
            }
            0x10b4 => {
                // ColorSpace: PrintConv
                match raw_print.parse::<u32>().unwrap_or(99) {
                    1 => "sRGB".to_string(),
                    2 => "Adobe RGB".to_string(),
                    0xffff => "Uncalibrated".to_string(),
                    _ => raw_print,
                }
            }
            _ => raw_print,
        };
        tags.push(Tag {
            id: TagId::Numeric(tag_id),
            name: name.to_string(),
            description: description.to_string(),
            group: TagGroup {
                family0: "MakerNotes".into(),
                family1: "MakerNotes".into(),
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
                family0: "MakerNotes".into(),
                family1: "MakerNotes".into(),
                family2: "Camera".into(),
            },
            raw_value: Value::String(val.clone()),
            print_value: val,
            priority: 0,
        }
    };
    let mk_raw = |name: &str, raw: Value, pv: String| -> Tag {
        Tag {
            id: TagId::Text(name.into()),
            name: name.into(),
            description: name.into(),
            group: TagGroup {
                family0: "MakerNotes".into(),
                family1: "MakerNotes".into(),
                family2: "Camera".into(),
            },
            raw_value: raw,
            print_value: pv,
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
    let ru16 = |d: &[u8], off: usize| -> u16 {
        if off + 2 > d.len() { return 0; }
        if is_le { u16::from_le_bytes([d[off], d[off+1]]) }
        else { u16::from_be_bytes([d[off], d[off+1]]) }
    };
    let ri16 = |d: &[u8], off: usize| -> i16 { ru16(d, off) as i16 };

    match tag_id {
        0x080a => {
            // CanonRawMakeModel: null-separated "Make\0Model\0" string
            let s = String::from_utf8_lossy(data);
            let parts: Vec<&str> = s.split('\0').filter(|p| !p.is_empty()).collect();
            if let Some(make) = parts.first() {
                tags.push(mk("Make", make.to_string()));
            }
            if let Some(model) = parts.get(1) {
                tags.push(mk("Model", model.to_string()));
            }
            true
        }
        0x102a => {
            // CanonShotInfo — int16s array, same format as JPEG MakerNote tag 0x0004
            let values: Vec<i16> = (0..data.len() / 2)
                .map(|i| ri16(data, i*2))
                .collect();
            let sub_tags = crate::tags::canon_sub::decode_shot_info(&values, "CRW");
            for t in sub_tags {
                tags.push(Tag {
                    group: TagGroup { family0: "MakerNotes".into(), family1: "MakerNotes".into(), family2: "Camera".into() },
                    ..t
                });
            }
            true
        }
        0x102d => {
            // CanonCameraSettings — int16s array, same format as JPEG MakerNote tag 0x0001
            let values: Vec<i16> = (0..data.len() / 2)
                .map(|i| ri16(data, i*2))
                .collect();
            let sub_tags = crate::tags::canon_sub::decode_camera_settings(&values);
            for t in sub_tags {
                tags.push(Tag {
                    group: TagGroup { family0: "MakerNotes".into(), family1: "MakerNotes".into(), family2: "Camera".into() },
                    ..t
                });
            }
            true
        }
        0x1031 => {
            // SensorInfo — int16s array (Canon::SensorInfo table)
            let n = data.len() / 2;
            if n > 1 { tags.push(mk("SensorWidth", ri16(data, 2).to_string())); }
            if n > 2 { tags.push(mk("SensorHeight", ri16(data, 4).to_string())); }
            if n > 5 { tags.push(mk("SensorLeftBorder", ri16(data, 10).to_string())); }
            if n > 6 { tags.push(mk("SensorTopBorder", ri16(data, 12).to_string())); }
            if n > 7 { tags.push(mk("SensorRightBorder", ri16(data, 14).to_string())); }
            if n > 8 { tags.push(mk("SensorBottomBorder", ri16(data, 16).to_string())); }
            if n > 9 { tags.push(mk("BlackMaskLeftBorder", ri16(data, 18).to_string())); }
            if n > 10 { tags.push(mk("BlackMaskTopBorder", ri16(data, 20).to_string())); }
            if n > 11 { tags.push(mk("BlackMaskRightBorder", ri16(data, 22).to_string())); }
            if n > 12 { tags.push(mk("BlackMaskBottomBorder", ri16(data, 24).to_string())); }
            true
        }
        0x1038 => {
            // CanonAFInfo — sequential int16u array (Canon::AFInfo, ProcessSerialData)
            // Layout (sequential, all int16u):
            //   [0] NumAFPoints
            //   [1] ValidAFPoints
            //   [2] CanonImageWidth
            //   [3] CanonImageHeight
            //   [4] AFImageWidth
            //   [5] AFImageHeight
            //   [6] AFAreaWidth
            //   [7] AFAreaHeight
            //   [8..8+N-1] AFAreaXPositions (N = NumAFPoints, int16s)
            //   [8+N..8+2N-1] AFAreaYPositions (int16s)
            //   [8+2N..8+2N+ceil(N/16)-1] AFPointsInFocus
            if data.len() >= 2 {
                let num_points = ru16(data, 0) as usize;
                let valid_points = ru16(data, 2);
                tags.push(mk("NumAFPoints", num_points.to_string()));
                tags.push(mk("ValidAFPoints", valid_points.to_string()));
                // Indices 2,3 = CanonImageWidth, CanonImageHeight
                if data.len() >= 8 {
                    tags.push(mk("CanonImageWidth", ru16(data, 4).to_string()));
                    tags.push(mk("CanonImageHeight", ru16(data, 6).to_string()));
                }
                // Indices 4,5 = AFImageWidth, AFImageHeight
                if data.len() >= 12 {
                    tags.push(mk("AFImageWidth", ru16(data, 8).to_string()));
                    tags.push(mk("AFImageHeight", ru16(data, 10).to_string()));
                }
                // Indices 6,7 = AFAreaWidth, AFAreaHeight (single values)
                if data.len() >= 16 {
                    tags.push(mk("AFAreaWidth", ru16(data, 12).to_string()));
                    tags.push(mk("AFAreaHeight", ru16(data, 14).to_string()));
                }
                // AFAreaXPositions: N int16s starting at byte 16
                let xpos_start = 16;
                let xpos_end = xpos_start + num_points * 2;
                if data.len() >= xpos_end && num_points > 0 {
                    let xpos: Vec<String> = (0..num_points).map(|i| {
                        ri16(data, xpos_start + i * 2).to_string()
                    }).collect();
                    tags.push(mk("AFAreaXPositions", xpos.join(" ")));
                }
                // AFAreaYPositions: N int16s starting after X positions
                let ypos_start = xpos_end;
                let ypos_end = ypos_start + num_points * 2;
                if data.len() >= ypos_end && num_points > 0 {
                    let ypos: Vec<String> = (0..num_points).map(|i| {
                        ri16(data, ypos_start + i * 2).to_string()
                    }).collect();
                    tags.push(mk("AFAreaYPositions", ypos.join(" ")));
                }
                // AFPointsInFocus: ceil(N/16) int16s
                let focus_start = ypos_end;
                let focus_count = (num_points + 15) / 16;
                if data.len() >= focus_start + focus_count * 2 && focus_count > 0 {
                    // Decode bitmask
                    let bits: u64 = (0..focus_count).fold(0u64, |acc, i| {
                        acc | ((ru16(data, focus_start + i * 2) as u64) << (i * 16))
                    });
                    // Count which AF points are in focus (0-based bit positions)
                    let in_focus: Vec<String> = (0..num_points)
                        .filter(|i| (bits >> i) & 1 == 1)
                        .map(|i| i.to_string())
                        .collect();
                    let pv = if in_focus.is_empty() {
                        bits.to_string()
                    } else {
                        in_focus.join(",")
                    };
                    tags.push(mk("AFPointsInFocus", pv));
                }
            }
            true
        }
        0x10a9 => {
            // ColorBalance — int16s array (Canon::ColorBalance table)
            // FIRST_ENTRY=1, FORMAT=int16u
            // indices: 1-4=WB_RGGBLevelsAuto, 5-8=Daylight, 9-12=Shade, 13-16=Cloudy,
            //          17-20=Tungsten, 21-24=Fluorescent, 25-28=Flash, 29-32=Custom,
            //          33-36=Kelvin, 37-40=BlackLevels
            let wb4_u = |idx: usize| -> String {
                // idx is 1-based per Perl FIRST_ENTRY=1
                // actual byte offset: (idx-1)*2
                let base = (idx - 1) * 2;
                format!("{} {} {} {}",
                    ri16(data, base),
                    ri16(data, base + 2),
                    ri16(data, base + 4),
                    ri16(data, base + 6))
            };
            let n = data.len() / 2 + 1; // n items (1-based count)
            // WB_RGGBLevelsAsShot: index 0 (byte 0) contains a selector; the actual
            // AS-SHOT levels are selected by the WhiteBalance value.
            // For the CRW file we just emit the standard named sets.
            if n > 4 { tags.push(mk("WB_RGGBLevelsAuto", wb4_u(1))); }
            if n > 8 { tags.push(mk("WB_RGGBLevelsDaylight", wb4_u(5))); }
            if n > 12 { tags.push(mk("WB_RGGBLevelsShade", wb4_u(9))); }
            if n > 16 { tags.push(mk("WB_RGGBLevelsCloudy", wb4_u(13))); }
            if n > 20 { tags.push(mk("WB_RGGBLevelsTungsten", wb4_u(17))); }
            if n > 24 { tags.push(mk("WB_RGGBLevelsFluorescent", wb4_u(21))); }
            if n > 28 { tags.push(mk("WB_RGGBLevelsFlash", wb4_u(25))); }
            if n > 32 { tags.push(mk("WB_RGGBLevelsCustom", wb4_u(29))); }
            if n > 36 { tags.push(mk("WB_RGGBLevelsKelvin", wb4_u(33))); }
            if n > 40 { tags.push(mk("WB_RGGBBlackLevels", wb4_u(37))); }
            true
        }
        0x10b5 => {
            // RawJpgInfo (SubDirectory → CanonRaw::RawJpgInfo, FORMAT=int16u, FIRST_ENTRY=1)
            // Index 1=RawJpgQuality, 2=RawJpgSize, 3=RawJpgWidth, 4=RawJpgHeight
            // FIRST_ENTRY=1 means byte offset of index N = (N-1)*2
            let n = data.len() / 2;
            if n >= 2 {
                // Index 1 = RawJpgQuality (byte 0)
                let quality = ru16(data, 0);
                let q_pv = match quality {
                    1 => "Economy".to_string(),
                    2 => "Normal".to_string(),
                    3 => "Fine".to_string(),
                    5 => "Superfine".to_string(),
                    _ => quality.to_string(),
                };
                tags.push(mk_raw("RawJpgQuality", Value::U16(quality), q_pv));
            }
            if n >= 3 {
                // Index 2 = RawJpgSize (byte 2)
                let size = ru16(data, 2);
                let s_pv = match size {
                    0 => "Large".to_string(),
                    1 => "Medium".to_string(),
                    2 => "Small".to_string(),
                    _ => size.to_string(),
                };
                tags.push(mk_raw("RawJpgSize", Value::U16(size), s_pv));
            }
            if n >= 4 {
                // Index 3 = RawJpgWidth (byte 4)
                let w = ru16(data, 4);
                tags.push(mk_raw("RawJpgWidth", Value::U16(w), w.to_string()));
            }
            if n >= 5 {
                // Index 4 = RawJpgHeight (byte 6)
                let h = ru16(data, 6);
                tags.push(mk_raw("RawJpgHeight", Value::U16(h), h.to_string()));
            }
            true
        }
        0x1093 => {
            // CanonFileInfo — int16s array (Canon::FileInfo, FORMAT=int16s, FIRST_ENTRY=1)
            // Index 3=BracketMode, 4=BracketValue, 5=BracketShotNumber
            // Note: index 1,2 are FileNumber for specific models (not EOS REBEL), skip
            // FIRST_ENTRY=1 means byte offset of index N = (N-1)*2
            let n = data.len() / 2 + 1; // 1-based count
            if n > 3 {
                // Index 3 = BracketMode (byte 4)
                let bracket_mode = ri16(data, 4);
                let bm_pv = match bracket_mode {
                    0 => "Off".to_string(),
                    1 => "AEB".to_string(),
                    2 => "FEB".to_string(),
                    3 => "ISO".to_string(),
                    4 => "WB".to_string(),
                    _ => bracket_mode.to_string(),
                };
                tags.push(mk_raw("BracketMode", Value::I16(bracket_mode), bm_pv));
            }
            if n > 4 {
                // Index 4 = BracketValue (byte 6)
                let bracket_val = ri16(data, 6);
                tags.push(mk_raw("BracketValue", Value::I16(bracket_val), bracket_val.to_string()));
            }
            if n > 5 {
                // Index 5 = BracketShotNumber (byte 8)
                let bracket_shot = ri16(data, 8);
                tags.push(mk_raw("BracketShotNumber", Value::I16(bracket_shot), bracket_shot.to_string()));
            }
            true
        }
        0x1803 => {
            // ImageFormat (SubDirectory → CanonRaw::ImageFormat, FORMAT=int32u)
            // 0=FileFormat, 1=TargetCompressionRatio(float)
            if data.len() >= 4 {
                let file_format = ru32(data, 0);
                let fmt_str = match file_format {
                    0x00020001 => "CRW".to_string(),
                    0x00010000 => "JPEG (lossy)".to_string(),
                    0x00010002 => "JPEG (non-quantization)".to_string(),
                    0x00010003 => "JPEG (lossy/non-quantization toggled)".to_string(),
                    _ => file_format.to_string(),
                };
                tags.push(mk("FileFormat", fmt_str));
            }
            if data.len() >= 8 {
                let ratio = rf32(data, 4);
                // Format to match Perl output (integer if whole number)
                let s = if ratio == ratio.floor() && ratio < 1e6 {
                    format!("{}", ratio as u32)
                } else {
                    format!("{}", ratio)
                };
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
                let s = if aspect == aspect.floor() && aspect < 1e6 {
                    format!("{}", aspect as u32)
                } else {
                    format!("{}", aspect)
                };
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
                // Perl: PrintConv not specified, uses default ValueConv output
                tags.push(mk("MeasuredEV", format!("{:.2}", val)));
            }
            true
        }
        0x180e => {
            // TimeStamp (SubDirectory → CanonRaw::TimeStamp, FORMAT=int32u, FIRST_ENTRY=0)
            // 0=DateTimeOriginal(unix time), 1=TimeZoneCode(int32s, /3600), 2=TimeZoneInfo
            if data.len() >= 4 {
                // DateTimeOriginal: ConvertUnixTime($val) => "YYYY:MM:DD HH:MM:SS"
                let unix_time = ru32(data, 0);
                let dt = unix_time_to_exif(unix_time);
                tags.push(mk_raw("DateTimeOriginal", Value::U32(unix_time), dt));
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
            if data.len() >= 4 {
                let ec = rf32(data, 0);
                tags.push(mk("ExposureCompensation", format!("{}", ec)));
            }
            if data.len() >= 8 {
                let sv = rf32(data, 4);
                // ShutterSpeedValue ValueConv: 'abs($val)<100 ? 1/(2**$val) : 0'
                let et = if (sv as f64).abs() < 100.0 {
                    2.0_f64.powf(-(sv as f64))
                } else { 0.0 };
                tags.push(mk("ShutterSpeedValue", format!("{}", sv)));
                // Emit ExposureTime for composites
                let et_print = crate::tags::canon_sub::print_exposure_time(et);
                tags.push(Tag {
                    id: TagId::Text("ExposureTime".into()),
                    name: "ExposureTime".into(),
                    description: "Exposure Time".into(),
                    group: TagGroup { family0: "MakerNotes".into(), family1: "MakerNotes".into(), family2: "Camera".into() },
                    raw_value: Value::F64(et),
                    print_value: et_print,
                    priority: 0,
                });
            }
            if data.len() >= 12 {
                let av = rf32(data, 8);
                // ApertureValue ValueConv: '2 ** ($val / 2)'
                let fn_val = 2.0_f64.powf(av as f64 / 2.0);
                tags.push(mk("ApertureValue", format!("{:.1}", fn_val)));
                // Also emit FNumber for composites
                tags.push(Tag {
                    id: TagId::Text("FNumber".into()),
                    name: "FNumber".into(),
                    description: "F Number".into(),
                    group: TagGroup { family0: "MakerNotes".into(), family1: "MakerNotes".into(), family2: "Camera".into() },
                    raw_value: Value::F64(fn_val),
                    print_value: format!("{:.1}", fn_val),
                    priority: 0,
                });
            }
            true
        }
        0x1835 => {
            // DecoderTable (SubDirectory → CanonRaw::DecoderTable, FORMAT=int32u, FIRST_ENTRY=0)
            // 0=DecoderTableNumber, 2=CompressedDataOffset, 3=CompressedDataLength
            if data.len() >= 4 {
                tags.push(mk("DecoderTableNumber", ru32(data, 0).to_string()));
            }
            if data.len() >= 12 {
                tags.push(mk("CompressedDataOffset", ru32(data, 8).to_string()));
            }
            if data.len() >= 16 {
                tags.push(mk("CompressedDataLength", ru32(data, 12).to_string()));
            }
            true
        }
        0x1029 => {
            // CanonFocalLength (SubDirectory → Canon::FocalLength, FORMAT=int16u)
            // 0=FocalType (PrintConv: 1=Fixed, 2=Zoom)
            // 1=FocalLength (ValueConv val/FocalUnits)
            // 2=FocalPlaneXSize (int16u, ValueConv val*25.4/1000)
            // 3=FocalPlaneYSize (int16u, ValueConv val*25.4/1000)
            if data.len() >= 2 {
                let focal_type = ru16(data, 0);
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
            // FocalLength (index 1): ValueConv = val / FocalUnits
            if data.len() >= 4 {
                let fl_raw = ru16(data, 2);
                let focal_units = ru16(data, 0);
                let fu = if focal_units > 0 { focal_units } else { 1 };
                if fl_raw > 0 {
                    let fl_mm = fl_raw as f64 / fu as f64;
                    tags.push(Tag {
                        id: TagId::Text("FocalLength".into()),
                        name: "FocalLength".into(),
                        description: "Focal Length".into(),
                        group: TagGroup { family0: "MakerNotes".into(), family1: "MakerNotes".into(), family2: "Camera".into() },
                        raw_value: Value::F64(fl_mm),
                        print_value: format!("{} mm", fl_mm as u32),
                        priority: 0,
                    });
                }
            }
            if data.len() >= 6 {
                let x_raw = ru16(data, 4);
                if x_raw >= 40 {
                    let x_mm = x_raw as f64 * 25.4 / 1000.0;
                    let print_str = format!("{:.2} mm", x_mm);
                    tags.push(Tag {
                        id: TagId::Text("FocalPlaneXSize".into()),
                        name: "FocalPlaneXSize".into(),
                        description: "Focal Plane X Size".into(),
                        group: TagGroup { family0: "MakerNotes".into(), family1: "MakerNotes".into(), family2: "Camera".into() },
                        raw_value: Value::F64(x_mm),
                        print_value: print_str,
                        priority: 0,
                    });
                }
            }
            if data.len() >= 8 {
                let y_raw = ru16(data, 6);
                if y_raw >= 40 {
                    let y_mm = y_raw as f64 * 25.4 / 1000.0;
                    let print_str = format!("{:.2} mm", y_mm);
                    tags.push(Tag {
                        id: TagId::Text("FocalPlaneYSize".into()),
                        name: "FocalPlaneYSize".into(),
                        description: "Focal Plane Y Size".into(),
                        group: TagGroup { family0: "MakerNotes".into(), family1: "MakerNotes".into(), family2: "Camera".into() },
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

/// Convert Unix timestamp to ExifTool date string "YYYY:MM:DD HH:MM:SS"
fn unix_time_to_exif(unix_time: u32) -> String {
    // Simple implementation without timezone conversion
    // (Perl: ConvertUnixTime which calls localtime)
    // We use UTC since we don't have timezone info at this point
    let t = unix_time as i64;
    // Days since epoch
    let days = t / 86400;
    let time_of_day = t % 86400;
    let h = time_of_day / 3600;
    let m = (time_of_day % 3600) / 60;
    let s = time_of_day % 60;

    // Gregorian calendar calculation
    let mut year = 1970i64;
    let mut remaining_days = days;
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        year += 1;
    }
    let month_days = [31i64, if is_leap_year(year) { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 1i64;
    for &md in &month_days {
        if remaining_days < md {
            break;
        }
        remaining_days -= md;
        month += 1;
    }
    let day = remaining_days + 1;
    format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}", year, month, day, h, m, s)
}

fn is_leap_year(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0)
}

fn crw_tag_name(tag_id: u16) -> (&'static str, &'static str) {
    // Tag IDs from Perl CanonRaw.pm (tag_id & 0x3FFF strips the data-type bits)
    match tag_id & 0x3FFF {
        0x0000 => ("NullRecord", "Null Record"),
        0x0032 => ("CanonColorInfo1", "Color Info 1"),
        // 0x0805 handled specially based on directory context (CanonFileDescription or UserComment)
        0x080a => ("CanonRawMakeModel", "Canon Raw Make Model"),  // Split into Make+Model in binary subdir
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
        0x1029 => ("", ""),  // CanonFocalLength (SubDirectory) — handled in binary subdir
        // SubDirectory containers — decoded into sub-tags by Perl, suppressed here
        0x102a => ("", ""),  // CanonShotInfo (SubDirectory)
        0x102d => ("", ""),  // CanonCameraSettings (SubDirectory)
        0x1031 => ("", ""),  // SensorInfo (SubDirectory)
        0x1038 => ("", ""),  // CanonAFInfo (SubDirectory)
        0x1093 => ("", ""),  // CanonFileInfo (SubDirectory)
        0x10a9 => ("", ""),  // ColorBalance (SubDirectory)
        0x10ae => ("ColorTemperature", "Color Temperature"),
        0x10b4 => ("ColorSpace", "Color Space"),
        0x10b5 => ("", ""),  // RawJpgInfo (SubDirectory)
        0x1803 => ("", ""),  // ImageFormat (SubDirectory) — handled in binary subdir
        0x1804 => ("RecordID", "Record ID"),
        0x1806 => ("SelfTimerTime", "Self Timer Time"),
        0x1807 => ("TargetDistanceSetting", "Target Distance Setting"),
        0x180b => ("SerialNumber", "Serial Number"),
        0x180e => ("", ""),  // TimeStamp (SubDirectory) — handled in binary subdir
        0x1810 => ("", ""),  // ImageInfo (SubDirectory) — handled in binary subdir
        0x1813 => ("", ""),  // FlashInfo (SubDirectory) — handled in binary subdir
        0x1814 => ("", ""),  // MeasuredEV — handled in binary subdir
        0x1817 => ("FileNumber", "File Number"),
        0x1818 => ("", ""),  // ExposureInfo (SubDirectory) — handled in binary subdir
        0x1834 => ("CanonModelID", "Model ID"),
        0x1835 => ("", ""),  // DecoderTable (SubDirectory) — handled in binary subdir
        0x183b => ("SerialNumberFormat", "Serial Number Format"),
        0x2005 => ("", ""),  // RawData — handled separately
        0x2007 => ("", ""),  // JpgFromRaw — handled separately
        0x2008 => ("", ""),  // ThumbnailImage — handled separately
        0x3002 => ("ShootingRecord", "Shooting Record"),
        0x3003 => ("MeasuredInfo", "Measured Info"),
        0x3004 => ("ColorInfo", "Color Info"),
        _ => ("", ""),
    }
}

/// Canon ModelID to string conversion (from Perl Canon::canonModelID table)
fn canon_model_id_str(id: u32) -> String {
    match id {
        0x80000001 => "EOS-1D".to_string(),
        0x80000167 => "EOS-1DS".to_string(),
        0x80000168 => "EOS 10D".to_string(),
        0x80000169 => "EOS Digital Rebel / 300D / Kiss Digital".to_string(),
        0x80000170 => "EOS Digital Rebel / 300D / Kiss Digital".to_string(), // alternate
        0x80000174 => "EOS-1D Mark II".to_string(),
        0x80000175 => "EOS 20D".to_string(),
        0x80000188 => "EOS-1Ds Mark II".to_string(),
        0x80000189 => "EOS Digital Rebel XT / 350D / Kiss Digital N".to_string(),
        0x80000213 => "EOS 5D".to_string(),
        0x80000232 => "EOS-1D Mark II N".to_string(),
        0x80000234 => "EOS 30D".to_string(),
        0x80000236 => "EOS Digital Rebel XTi / 400D / Kiss Digital X".to_string(),
        0x80000250 => "EOS 7D".to_string(),
        0x80000252 => "EOS Rebel T1i / 500D / Kiss X3".to_string(),
        0x80000254 => "EOS Rebel XS / 1000D / Kiss F".to_string(),
        0x80000261 => "EOS 50D".to_string(),
        0x80000270 => "EOS Rebel T2i / 550D / Kiss X4".to_string(),
        0x80000285 => "EOS Digital Rebel XSi / 450D / Kiss X2".to_string(),
        0x80000286 => "EOS-1D Mark III".to_string(),
        0x80000288 => "EOS 5D Mark II".to_string(),
        0x80000289 => "EOS Digital Rebel / 300D / Kiss Digital".to_string(), // 300D variant
        0x80000000 => "EOS Digital Rebel / 300D / Kiss Digital".to_string(),
        // Catch-all: format as hex
        _ => format!("0x{:08X}", id),
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
