//! IPTC (International Press Telecommunications Council) metadata reader.
//!
//! Reads IPTC-IIM (Information Interchange Model) records, commonly found
//! in JPEG APP13 Photoshop segments. Mirrors ExifTool's IPTC.pm.

use crate::error::Result;
use crate::tag::{Tag, TagGroup, TagId};
use crate::tags::iptc as iptc_tags;
use crate::value::Value;

/// IPTC metadata reader.
pub struct IptcReader;

impl IptcReader {
    /// Parse IPTC data from a raw byte slice.
    ///
    /// IPTC-IIM format: sequences of records, each:
    ///   - 1 byte:  tag marker (0x1C)
    ///   - 1 byte:  record number
    ///   - 1 byte:  dataset number
    ///   - 2 bytes: data length (big-endian), or extended if >= 0x8000
    ///   - N bytes: data
    pub fn read(data: &[u8]) -> Result<Vec<Tag>> {
        let mut tags = Vec::new();
        let mut pos = 0;

        while pos + 5 <= data.len() {
            // Check for IPTC tag marker
            if data[pos] != 0x1C {
                // Skip non-IPTC data
                pos += 1;
                continue;
            }

            let record = data[pos + 1];
            let dataset = data[pos + 2];
            let length = u16::from_be_bytes([data[pos + 3], data[pos + 4]]) as usize;

            pos += 5;

            // Extended dataset length (bit 15 set means the length field itself
            // gives the number of bytes in an extended length that follows)
            if length >= 0x8000 {
                // Skip extended length datasets for now
                break;
            }

            if pos + length > data.len() {
                break;
            }

            let value_data = &data[pos..pos + length];
            pos += length;

            // Only handle Application Record (record 2) for now, it has the useful tags
            let ifd_name = match record {
                1 => "IPTCEnvelope",
                2 => "IPTCApplication",
                _ => continue,
            };

            // Check for PhotoMechanic SoftEdit fields BEFORE string decoding
            // (These are int32s, not strings, so must be decoded as binary)
            if record == 2 && dataset >= 209 && dataset <= 222 {
                // Decode as binary (int32s)
                let bin_value = Value::Binary(value_data.to_vec());
                if let Some((pm_name, pm_print)) = lookup_photomechanic(dataset, &bin_value) {
                    tags.push(Tag {
                        id: TagId::Numeric(((record as u16) << 8) | dataset as u16),
                        name: pm_name.clone(),
                        description: pm_name,
                        group: TagGroup {
                            family0: "PhotoMechanic".to_string(),
                            family1: "PhotoMechanic".to_string(),
                            family2: "Image".to_string(),
                        },
                        raw_value: bin_value,
                        print_value: pm_print,
                        priority: 0,
                    });
                    continue;
                }
            }

            let value = if iptc_tags::is_string_tag(record, dataset) {
                Value::String(
                    String::from_utf8_lossy(value_data)
                        .trim_end_matches('\0')
                        .to_string(),
                )
            } else if length <= 2 {
                match length {
                    1 => Value::U8(value_data[0]),
                    2 => Value::U16(u16::from_be_bytes([value_data[0], value_data[1]])),
                    _ => Value::Binary(value_data.to_vec()),
                }
            } else {
                Value::Binary(value_data.to_vec())
            };

            let tag_info = iptc_tags::lookup(record, dataset);
            let (name, description) = match tag_info {
                Some(info) => (info.name.to_string(), info.description.to_string()),
                None => {
                    // Suppress unknown IPTC records (don't emit IPTC:N:N format)
                    continue;
                },
            };

            let print_value = value.to_display_string();

            tags.push(Tag {
                id: TagId::Numeric(((record as u16) << 8) | dataset as u16),
                name,
                description,
                group: TagGroup {
                    family0: "IPTC".to_string(),
                    family1: ifd_name.to_string(),
                    family2: "Other".to_string(),
                },
                raw_value: value,
                print_value,
                priority: 0,
            });
        }

        Ok(tags)
    }
}

/// Look up a PhotoMechanic SoftEdit field (IPTC record 2, dataset 209-239).
/// Returns (tag_name, print_value) or None if unknown.
fn lookup_photomechanic(dataset: u8, value: &Value) -> Option<(String, String)> {
    // PhotoMechanic fields are FORMAT='int32s' - 4 bytes big-endian signed int
    let int_val = if let Value::Binary(ref b) = value {
        if b.len() == 4 {
            i32::from_be_bytes([b[0], b[1], b[2], b[3]])
        } else {
            return None;
        }
    } else {
        return None;
    };

    let color_classes = [
        "0 (None)", "1 (Winner)", "2 (Winner alt)", "3 (Superior)",
        "4 (Superior alt)", "5 (Typical)", "6 (Typical alt)", "7 (Extras)", "8 (Trash)",
    ];

    match dataset {
        209 => Some(("RawCropLeft".to_string(), format!("{:.3}%", int_val as f64 / 655.36))),
        210 => Some(("RawCropTop".to_string(), format!("{:.3}%", int_val as f64 / 655.36))),
        211 => Some(("RawCropRight".to_string(), format!("{:.3}%", int_val as f64 / 655.36))),
        212 => Some(("RawCropBottom".to_string(), format!("{:.3}%", int_val as f64 / 655.36))),
        213 => Some(("ConstrainedCropWidth".to_string(), int_val.to_string())),
        214 => Some(("ConstrainedCropHeight".to_string(), int_val.to_string())),
        215 => Some(("FrameNum".to_string(), int_val.to_string())),
        216 => {
            let rot = match int_val {
                0 => "0", 1 => "90", 2 => "180", 3 => "270", _ => "0",
            };
            Some(("Rotation".to_string(), rot.to_string()))
        }
        217 => Some(("CropLeft".to_string(), int_val.to_string())),
        218 => Some(("CropTop".to_string(), int_val.to_string())),
        219 => Some(("CropRight".to_string(), int_val.to_string())),
        220 => Some(("CropBottom".to_string(), int_val.to_string())),
        221 => {
            let v = if int_val == 0 { "No" } else { "Yes" };
            Some(("Tagged".to_string(), v.to_string()))
        }
        222 => {
            let idx = int_val as usize;
            let class = if idx < color_classes.len() {
                color_classes[idx].to_string()
            } else {
                format!("{}", int_val)
            };
            Some(("ColorClass".to_string(), class))
        }
        223 => Some(("Rating".to_string(), int_val.to_string())),
        236 => Some(("PreviewCropLeft".to_string(), format!("{:.3}%", int_val as f64 / 655.36))),
        237 => Some(("PreviewCropTop".to_string(), format!("{:.3}%", int_val as f64 / 655.36))),
        238 => Some(("PreviewCropRight".to_string(), format!("{:.3}%", int_val as f64 / 655.36))),
        239 => Some(("PreviewCropBottom".to_string(), format!("{:.3}%", int_val as f64 / 655.36))),
        _ => None,
    }
}
