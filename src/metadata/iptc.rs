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
