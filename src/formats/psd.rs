//! Adobe Photoshop PSD/PSB file format reader.
//!
//! Parses PSD headers, image resource blocks (IRBs) containing EXIF, IPTC,
//! XMP, ICC profiles, and layer info. Mirrors ExifTool's Photoshop.pm.

use crate::error::{Error, Result};
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

    tags.push(mk(
        "FileVersion",
        "File Version",
        Value::String(if is_psb { "PSB".into() } else { "PSD".into() }),
    ));

    // Header: channels, height, width, depth, color mode
    let num_channels = u16::from_be_bytes([data[12], data[13]]);
    let height = u32::from_be_bytes([data[14], data[15], data[16], data[17]]);
    let width = u32::from_be_bytes([data[18], data[19], data[20], data[21]]);
    let bit_depth = u16::from_be_bytes([data[22], data[23]]);
    let color_mode = u16::from_be_bytes([data[24], data[25]]);

    tags.push(mk("ImageWidth", "Image Width", Value::U32(width)));
    tags.push(mk("ImageHeight", "Image Height", Value::U32(height)));
    tags.push(mk("NumChannels", "Number of Channels", Value::U16(num_channels)));
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
    let color_data_len = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
    pos += 4 + color_data_len;

    // Image resource section
    if pos + 4 > data.len() {
        return Ok(tags);
    }
    let irb_len = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
    pos += 4;

    let irb_end = (pos + irb_len).min(data.len());
    read_irb_resources(data, pos, irb_end, &mut tags);

    // Layer and mask section
    pos = irb_end;
    if pos + 4 <= data.len() {
        let layer_len = if is_psb && pos + 8 <= data.len() {
            let l = u64::from_be_bytes([
                data[pos], data[pos + 1], data[pos + 2], data[pos + 3],
                data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7],
            ]) as usize;
            pos += 8;
            l
        } else {
            let l = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
            pos += 4;
            l
        };

        if layer_len > 0 && pos + 4 <= data.len() {
            let layer_info_len = if is_psb && pos + 8 <= data.len() {
                let l = u64::from_be_bytes([
                    data[pos], data[pos + 1], data[pos + 2], data[pos + 3],
                    data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7],
                ]) as usize;
                pos += 8;
                l
            } else {
                let l = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
                pos += 4;
                l
            };

            if layer_info_len > 2 && pos + 2 <= data.len() {
                let layer_count = i16::from_be_bytes([data[pos], data[pos + 1]]);
                let actual_count = layer_count.unsigned_abs();
                tags.push(mk(
                    "LayerCount",
                    "Layer Count",
                    Value::U16(actual_count),
                ));
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

    Ok(tags)
}

/// Parse Photoshop Image Resource Blocks (IRBs).
fn read_irb_resources(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>) {
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
        let data_len = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        pos += 4;

        if pos + data_len > end {
            break;
        }

        let resource_data = &data[pos..pos + data_len];

        match resource_id {
            // Resolution info
            0x03ED => {
                if resource_data.len() >= 16 {
                    let h_res_fixed = u32::from_be_bytes([
                        resource_data[0], resource_data[1], resource_data[2], resource_data[3],
                    ]);
                    let h_res_unit = u16::from_be_bytes([resource_data[4], resource_data[5]]);
                    let v_res_fixed = u32::from_be_bytes([
                        resource_data[8], resource_data[9], resource_data[10], resource_data[11],
                    ]);

                    let h_res = h_res_fixed as f64 / 65536.0;
                    let v_res = v_res_fixed as f64 / 65536.0;
                    let unit = if h_res_unit == 1 { "dpi" } else { "dpcm" };

                    tags.push(mk(
                        "XResolution",
                        "X Resolution",
                        Value::String(format!("{:.0} {}", h_res, unit)),
                    ));
                    tags.push(mk(
                        "YResolution",
                        "Y Resolution",
                        Value::String(format!("{:.0} {}", v_res, unit)),
                    ));
                }
            }
            // IPTC-IIM
            0x0404 => {
                if let Ok(iptc_tags) = IptcReader::read(resource_data) {
                    tags.extend(iptc_tags);
                }
            }
            // ICC Profile
            0x040F => {
                tags.push(mk(
                    "ICC_Profile",
                    "ICC Profile",
                    Value::Binary(resource_data.to_vec()),
                ));
            }
            // EXIF
            0x0422 => {
                if let Ok(exif_tags) = ExifReader::read(resource_data) {
                    tags.extend(exif_tags);
                }
            }
            // XMP
            0x0424 => {
                if let Ok(xmp_tags) = XmpReader::read(resource_data) {
                    tags.extend(xmp_tags);
                }
            }
            // Print flags
            0x03F3 => {
                // Just note that print info exists
            }
            // JPEG Quality
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
                    // Quality is 1-12 in Photoshop
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
            _ => {}
        }

        pos += data_len;
        if data_len % 2 != 0 {
            pos += 1;
        }
    }
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
