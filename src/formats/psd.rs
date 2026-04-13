//! Adobe Photoshop PSD/PSB file format reader.
//!
//! Parses PSD headers, image resource blocks (IRBs) containing EXIF, IPTC,
//! XMP, ICC profiles, and layer info. Mirrors ExifTool's Photoshop.pm.

use crate::error::{Error, Result};
use crate::formats::icc;
use crate::md5;
use crate::metadata::{ExifReader, IptcReader, XmpReader};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_psd(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 26 || !data.starts_with(b"8BPS") {
        return Err(Error::InvalidData("not a PSD/PSB file".into()));
    }

    let mut tags = Vec::new();
    let version = u16::from_be_bytes([data[4], data[5]]);
    let is_psb = version == 2;
    let _ = is_psb; // version used if needed

    // Header: channels, height, width, depth, color mode
    let num_channels = u16::from_be_bytes([data[12], data[13]]);
    let height = u32::from_be_bytes([data[14], data[15], data[16], data[17]]);
    let width = u32::from_be_bytes([data[18], data[19], data[20], data[21]]);
    let bit_depth = u16::from_be_bytes([data[22], data[23]]);
    let color_mode = u16::from_be_bytes([data[24], data[25]]);

    tags.push(mk("ImageWidth", "Image Width", Value::U32(width)));
    tags.push(mk("ImageHeight", "Image Height", Value::U32(height)));
    tags.push(mk(
        "NumChannels",
        "Number of Channels",
        Value::U16(num_channels),
    ));
    tags.push(mk("BitDepth", "Bit Depth", Value::U16(bit_depth)));

    let color_mode_str = match color_mode {
        0 => "Bitmap",
        1 => "Grayscale",
        2 => "Indexed",
        3 => "RGB",
        4 => "CMYK",
        7 => "Multichannel",
        8 => "Duotone",
        9 => "Lab",
        _ => "Unknown",
    };
    tags.push(mk(
        "ColorMode",
        "Color Mode",
        Value::String(color_mode_str.into()),
    ));

    // Skip color mode data section
    let mut pos = 26;
    if pos + 4 > data.len() {
        return Ok(tags);
    }
    let color_data_len =
        u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
    pos += 4 + color_data_len;

    // Image resource section
    if pos + 4 > data.len() {
        return Ok(tags);
    }
    let irb_len =
        u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
    pos += 4;

    let irb_end = (pos + irb_len).min(data.len());
    read_irb_resources(data, pos, irb_end, &mut tags);

    // Layer and mask section
    pos = irb_end;
    if pos + 4 <= data.len() {
        let layer_len = if is_psb && pos + 8 <= data.len() {
            let l = u64::from_be_bytes([
                data[pos],
                data[pos + 1],
                data[pos + 2],
                data[pos + 3],
                data[pos + 4],
                data[pos + 5],
                data[pos + 6],
                data[pos + 7],
            ]) as usize;
            pos += 8;
            l
        } else {
            let l = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
                as usize;
            pos += 4;
            l
        };

        if layer_len > 0 && pos + 4 <= data.len() {
            let _layer_info_len = if is_psb && pos + 8 <= data.len() {
                let l = u64::from_be_bytes([
                    data[pos],
                    data[pos + 1],
                    data[pos + 2],
                    data[pos + 3],
                    data[pos + 4],
                    data[pos + 5],
                    data[pos + 6],
                    data[pos + 7],
                ]) as usize;
                pos += 8;
                l
            } else {
                let l = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
                    as usize;
                pos += 4;
                l
            };

            // layer_info_len == 0 means no layers, but layer count (0) is still present
            // Perl: $len += 2 to include the layer count, then calls ProcessLayers with DirLen=2
            if pos + 2 <= data.len() {
                let layer_count = i16::from_be_bytes([data[pos], data[pos + 1]]);
                let actual_count = layer_count.unsigned_abs();
                tags.push(mk("LayerCount", "Layer Count", Value::U16(actual_count)));
                if layer_count < 0 {
                    tags.push(mk(
                        "TransparencyPresent",
                        "Transparency",
                        Value::String("Yes".into()),
                    ));
                }
            }
        }
    }

    // Look for PhotoMechanic IPTC trailer at end of file
    // The trailer is raw IPTC data appended to the PSD file
    scan_photomechanic_trailer(data, &mut tags);

    Ok(tags)
}

/// Scan for PhotoMechanic IPTC trailer at the end of the PSD file.
/// PhotoMechanic appends IPTC record 2 data (datasets 209-239) to supported files.
fn scan_photomechanic_trailer(data: &[u8], tags: &mut Vec<Tag>) {
    // Look backwards from end of file for IPTC data (0x1C markers)
    // The trailer starts with 0x1C 0x02 (record 2 dataset marker)
    // Scan up to 4096 bytes from end
    let search_from = if data.len() > 4096 {
        data.len() - 4096
    } else {
        0
    };
    let search_data = &data[search_from..];

    // Find sequences of 0x1C 0x02 XX 0x00 0x04 (record 2, length=4 big-endian)
    let mut pos = 0;
    let mut pm_start = None;

    while pos + 9 <= search_data.len() {
        if search_data[pos] == 0x1C && search_data[pos + 1] == 0x02 {
            let dataset = search_data[pos + 2];
            // Check if this is a PhotoMechanic dataset
            if dataset >= 209 && dataset <= 239 {
                let len = u16::from_be_bytes([search_data[pos + 3], search_data[pos + 4]]) as usize;
                if len == 4 && pos + 9 <= search_data.len() {
                    // This looks like a valid PhotoMechanic IPTC record
                    if pm_start.is_none() {
                        pm_start = Some(pos);
                    }
                }
            }
        }
        pos += 1;
    }

    if let Some(start) = pm_start {
        // Parse the IPTC data from this point
        if let Ok(pm_tags) = IptcReader::read(&search_data[start..]) {
            tags.extend(pm_tags);
        }
    }
}

/// Parse Photoshop Image Resource Blocks (IRBs) - public for use by PDF reader.
pub fn read_irb_resources(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>) {
    let mut pos = start;

    while pos + 12 <= end {
        // Signature: "8BIM" (or "PHUT", "AgHg", etc.)
        if &data[pos..pos + 4] != b"8BIM"
            && &data[pos..pos + 4] != b"PHUT"
            && &data[pos..pos + 4] != b"AgHg"
        {
            break;
        }
        pos += 4;

        // Resource ID (2 bytes BE)
        let resource_id = u16::from_be_bytes([data[pos], data[pos + 1]]);
        pos += 2;

        // Pascal string name (1 byte length + data, padded to even)
        let name_len = data[pos] as usize;
        pos += 1;
        pos += name_len;
        if (name_len + 1) % 2 != 0 {
            pos += 1;
        }

        if pos + 4 > end {
            break;
        }

        // Data length (4 bytes BE)
        let data_len =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        pos += 4;

        if pos + data_len > end {
            break;
        }

        let resource_data = &data[pos..pos + data_len];

        match resource_id {
            // Resolution info (0x03ED)
            0x03ED => {
                if resource_data.len() >= 16 {
                    let h_res_fixed = u32::from_be_bytes([
                        resource_data[0],
                        resource_data[1],
                        resource_data[2],
                        resource_data[3],
                    ]);
                    let h_res_unit = u16::from_be_bytes([resource_data[4], resource_data[5]]);
                    let v_res_fixed = u32::from_be_bytes([
                        resource_data[8],
                        resource_data[9],
                        resource_data[10],
                        resource_data[11],
                    ]);
                    let v_res_unit = u16::from_be_bytes([resource_data[12], resource_data[13]]);

                    let h_res = h_res_fixed as f64 / 65536.0;
                    let v_res = v_res_fixed as f64 / 65536.0;

                    tags.push(mk(
                        "XResolution",
                        "X Resolution",
                        Value::String(format!("{:.0}", h_res)),
                    ));
                    tags.push(mk(
                        "YResolution",
                        "Y Resolution",
                        Value::String(format!("{:.0}", v_res)),
                    ));

                    let h_unit_str = match h_res_unit {
                        1 => "inches",
                        2 => "cm",
                        _ => "unknown",
                    };
                    let v_unit_str = match v_res_unit {
                        1 => "inches",
                        2 => "cm",
                        _ => "unknown",
                    };
                    tags.push(mk(
                        "DisplayedUnitsX",
                        "Displayed Units X",
                        Value::String(h_unit_str.into()),
                    ));
                    tags.push(mk(
                        "DisplayedUnitsY",
                        "Displayed Units Y",
                        Value::String(v_unit_str.into()),
                    ));
                }
            }
            // IPTC-IIM (0x0404)
            0x0404 => {
                // Compute MD5 of IPTC data for CurrentIPTCDigest
                let digest_hex = md5::md5_hex(resource_data);
                tags.push(mk(
                    "CurrentIPTCDigest",
                    "Current IPTC Digest",
                    Value::String(digest_hex),
                ));
                if let Ok(iptc_tags) = IptcReader::read(resource_data) {
                    tags.extend(iptc_tags);
                }
            }
            // JPEG Quality (0x0406)
            0x0406 => {
                if resource_data.len() >= 4 {
                    let quality = u16::from_be_bytes([resource_data[0], resource_data[1]]);
                    let format = u16::from_be_bytes([resource_data[2], resource_data[3]]);
                    let format_str = match format {
                        0 => "Standard",
                        1 => "Optimized",
                        257 => "Progressive (3 scans)",
                        258 => "Progressive (4 scans)",
                        259 => "Progressive (5 scans)",
                        _ => "Unknown",
                    };
                    let q = match quality {
                        q if q <= 3 => "Low",
                        q if q <= 5 => "Medium",
                        q if q <= 8 => "High",
                        q if q <= 10 => "Very High",
                        _ => "Maximum",
                    };
                    tags.push(mk(
                        "PhotoshopQuality",
                        "Photoshop Quality",
                        Value::String(format!("{} ({})", q, quality)),
                    ));
                    tags.push(mk(
                        "PhotoshopFormat",
                        "Photoshop Format",
                        Value::String(format_str.into()),
                    ));
                }
            }
            // Copyright flag (0x040A)
            0x040A => {
                if !resource_data.is_empty() {
                    let flag = resource_data[0];
                    let val = if flag == 0 { "False" } else { "True" };
                    tags.push(mk(
                        "CopyrightFlag",
                        "Copyright Flag",
                        Value::String(val.into()),
                    ));
                }
            }
            // URL (0x040B)
            0x040B => {
                if let Ok(url) = std::str::from_utf8(resource_data) {
                    let url = url.trim().to_string();
                    if !url.is_empty() {
                        tags.push(mk("URL", "URL", Value::String(url)));
                    }
                }
            }
            // Global Angle (0x040D)
            0x040D => {
                // ExifTool takes the last occurrence of duplicate IRB resource IDs.
                // Remove any previous GlobalAngle before pushing the new value.
                tags.retain(|t| t.name != "GlobalAngle");
                if resource_data.len() >= 4 {
                    let angle = u32::from_be_bytes([
                        resource_data[0],
                        resource_data[1],
                        resource_data[2],
                        resource_data[3],
                    ]);
                    tags.push(mk("GlobalAngle", "Global Angle", Value::U32(angle)));
                }
                // If len < 4 (e.g. 1 byte), don't push - effectively suppresses GlobalAngle
            }
            // ICC Profile (0x040F)
            0x040F => {
                // Parse ICC profile tags inline
                if let Ok(icc_tags) = icc::read_icc(resource_data) {
                    tags.extend(icc_tags);
                }
            }
            // Global Altitude (0x0419)
            0x0419 => {
                if resource_data.len() >= 4 {
                    let alt = u32::from_be_bytes([
                        resource_data[0],
                        resource_data[1],
                        resource_data[2],
                        resource_data[3],
                    ]);
                    tags.push(mk("GlobalAltitude", "Global Altitude", Value::U32(alt)));
                }
            }
            // Slice Info (0x041A)
            0x041A => {
                parse_psd_slices(resource_data, tags);
            }
            // URL_List (0x041E)
            0x041E => {
                if resource_data.len() >= 4 {
                    // List of URLs
                    let count = u32::from_be_bytes([
                        resource_data[0],
                        resource_data[1],
                        resource_data[2],
                        resource_data[3],
                    ]);
                    let url_val = if count == 0 {
                        String::new()
                    } else {
                        // Try to extract URL strings (simplified)
                        String::new()
                    };
                    tags.push(mk("URL_List", "URL List", Value::String(url_val)));
                }
            }
            // Version Info (0x0421) - HasRealMergedData, WriterName, ReaderName
            0x0421 => {
                parse_psd_version_info(resource_data, tags);
            }
            // EXIF (0x0422)
            0x0422 => {
                if let Ok(exif_tags) = ExifReader::read(resource_data) {
                    tags.extend(exif_tags);
                }
            }
            // XMP (0x0424)
            0x0424 => {
                if let Ok(xmp_tags) = XmpReader::read(resource_data) {
                    tags.extend(xmp_tags);
                }
            }
            // IPTCDigest (0x0425)
            0x0425 => {
                let hex = resource_data
                    .iter()
                    .map(|b| format!("{:02x}", b))
                    .collect::<String>();
                if !hex.is_empty() {
                    tags.push(mk("IPTCDigest", "IPTC Digest", Value::String(hex)));
                }
            }
            // PrintScaleInfo (0x0426)
            0x0426 => {
                parse_psd_print_scale(resource_data, tags);
            }
            _ => {}
        }

        pos += data_len;
        if data_len % 2 != 0 {
            pos += 1;
        }
    }
}

/// Parse SliceInfo resource (0x041A) for group name and count.
fn parse_psd_slices(data: &[u8], tags: &mut Vec<Tag>) {
    // Slice resource has a header then slice records
    // At offset 20: var_ustr32 (4 bytes len + UTF-16 data) = SlicesGroupName
    // At offset 20 + len: NumSlices (int32u)
    if data.len() < 24 {
        return;
    }

    // Skip first 20 bytes (bounding box: top, left, bottom, right each int32)
    let pos = 20usize;
    if pos + 4 > data.len() {
        return;
    }

    // SlicesGroupName: 4 bytes = char count (UTF-16), then 2*count bytes
    let char_count =
        u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
    let str_end = pos + 4 + char_count * 2;

    if str_end > data.len() {
        return;
    }

    // Decode UTF-16BE string
    let utf16: Vec<u16> = data[pos + 4..str_end]
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    let group_name = String::from_utf16_lossy(&utf16);
    if !group_name.is_empty() {
        tags.push(mk(
            "SlicesGroupName",
            "Slices Group Name",
            Value::String(group_name),
        ));
    }

    // NumSlices follows
    if str_end + 4 <= data.len() {
        let num_slices = u32::from_be_bytes([
            data[str_end],
            data[str_end + 1],
            data[str_end + 2],
            data[str_end + 3],
        ]);
        tags.push(mk("NumSlices", "Number of Slices", Value::U32(num_slices)));
    }
}

/// Parse VersionInfo resource (0x0421): HasRealMergedData, WriterName, ReaderName.
fn parse_psd_version_info(data: &[u8], tags: &mut Vec<Tag>) {
    // offset 0: version (int32u) - skip
    // offset 4: HasRealMergedData (int8u)
    // offset 5: WriterName (var_ustr32: 4 byte count + UTF-16)
    // after: ReaderName (var_ustr32)
    if data.len() < 5 {
        return;
    }

    let has_merged = data[4];
    tags.push(mk(
        "HasRealMergedData",
        "Has Real Merged Data",
        Value::String(if has_merged != 0 { "Yes" } else { "No" }.into()),
    ));

    // WriterName
    let pos = 5usize;
    if pos + 4 > data.len() {
        return;
    }
    let char_count =
        u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
    let str_end = pos + 4 + char_count * 2;
    if str_end > data.len() {
        return;
    }

    let utf16: Vec<u16> = data[pos + 4..str_end]
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    let writer_name = String::from_utf16_lossy(&utf16);
    if !writer_name.is_empty() {
        tags.push(mk("WriterName", "Writer Name", Value::String(writer_name)));
    }

    // ReaderName
    let pos2 = str_end;
    if pos2 + 4 > data.len() {
        return;
    }
    let char_count2 =
        u32::from_be_bytes([data[pos2], data[pos2 + 1], data[pos2 + 2], data[pos2 + 3]]) as usize;
    let str_end2 = pos2 + 4 + char_count2 * 2;
    if str_end2 > data.len() {
        return;
    }

    let utf16_2: Vec<u16> = data[pos2 + 4..str_end2]
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    let reader_name = String::from_utf16_lossy(&utf16_2);
    if !reader_name.is_empty() {
        tags.push(mk("ReaderName", "Reader Name", Value::String(reader_name)));
    }
}

/// Parse PrintScaleInfo resource (0x0426): PrintStyle, PrintPosition, PrintScale.
fn parse_psd_print_scale(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 14 {
        return;
    }

    let style = u16::from_be_bytes([data[0], data[1]]);
    let style_str = match style {
        0 => "Centered",
        1 => "Size to Fit",
        2 => "User Defined",
        _ => "Unknown",
    };
    tags.push(mk(
        "PrintStyle",
        "Print Style",
        Value::String(style_str.into()),
    ));

    // PrintPosition: two floats (x, y)
    let x = f32::from_be_bytes([data[2], data[3], data[4], data[5]]);
    let y = f32::from_be_bytes([data[6], data[7], data[8], data[9]]);
    tags.push(mk(
        "PrintPosition",
        "Print Position",
        Value::String(format!("{} {}", x, y)),
    ));

    // PrintScale: one float
    let scale = f32::from_be_bytes([data[10], data[11], data[12], data[13]]);
    tags.push(mk(
        "PrintScale",
        "Print Scale",
        Value::String(format!("{}", scale)),
    ));
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let print_value = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "Photoshop".into(),
            family1: "Photoshop".into(),
            family2: "Image".into(),
        },
        raw_value: value,
        print_value,
        priority: 0,
    }
}
