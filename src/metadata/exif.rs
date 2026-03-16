//! EXIF/TIFF IFD metadata reader.
//!
//! Implements reading of TIFF IFD structures used in EXIF, GPS, and Interop metadata.
//! Mirrors the core logic of ExifTool's Exif.pm ProcessExif function.

use byteorder::{BigEndian, ByteOrder, LittleEndian};

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::tags::exif as exif_tags;
use crate::value::Value;

/// Byte order of the TIFF data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteOrderMark {
    LittleEndian,
    BigEndian,
}

/// Parsed TIFF header.
#[derive(Debug)]
pub struct TiffHeader {
    pub byte_order: ByteOrderMark,
    pub ifd0_offset: u32,
}

/// EXIF IFD entry as read from the file.
#[derive(Debug)]
struct IfdEntry {
    tag: u16,
    data_type: u16,
    count: u32,
    value_offset: u32,
    /// For values that fit in 4 bytes, the raw 4 bytes
    inline_data: [u8; 4],
}

/// Size in bytes for each TIFF data type.
fn type_size(data_type: u16) -> Option<usize> {
    match data_type {
        1 => Some(1),  // BYTE
        2 => Some(1),  // ASCII
        3 => Some(2),  // SHORT
        4 => Some(4),  // LONG
        5 => Some(8),  // RATIONAL
        6 => Some(1),  // SBYTE
        7 => Some(1),  // UNDEFINED
        8 => Some(2),  // SSHORT
        9 => Some(4),  // SLONG
        10 => Some(8), // SRATIONAL
        11 => Some(4), // FLOAT
        12 => Some(8), // DOUBLE
        13 => Some(4), // IFD
        _ => None,
    }
}

/// Parse a TIFF header from raw bytes.
pub fn parse_tiff_header(data: &[u8]) -> Result<TiffHeader> {
    if data.len() < 8 {
        return Err(Error::InvalidTiffHeader);
    }

    let byte_order = match (data[0], data[1]) {
        (b'I', b'I') => ByteOrderMark::LittleEndian,
        (b'M', b'M') => ByteOrderMark::BigEndian,
        _ => return Err(Error::InvalidTiffHeader),
    };

    let magic = match byte_order {
        ByteOrderMark::LittleEndian => LittleEndian::read_u16(&data[2..4]),
        ByteOrderMark::BigEndian => BigEndian::read_u16(&data[2..4]),
    };

    if magic != 42 {
        return Err(Error::InvalidTiffHeader);
    }

    let ifd0_offset = match byte_order {
        ByteOrderMark::LittleEndian => LittleEndian::read_u32(&data[4..8]),
        ByteOrderMark::BigEndian => BigEndian::read_u32(&data[4..8]),
    };

    Ok(TiffHeader {
        byte_order,
        ifd0_offset,
    })
}

/// EXIF metadata reader.
pub struct ExifReader;

impl ExifReader {
    /// Parse EXIF data from a byte slice (starting at the TIFF header).
    pub fn read(data: &[u8]) -> Result<Vec<Tag>> {
        let header = parse_tiff_header(data)?;
        let mut tags = Vec::new();

        // Read IFD0 (main image)
        Self::read_ifd(data, &header, header.ifd0_offset, "IFD0", &mut tags)?;

        // Extract Make + Model for MakerNotes detection and sub-table dispatch
        let make = tags
            .iter()
            .find(|t| t.name == "Make")
            .map(|t| t.print_value.clone())
            .unwrap_or_default();

        let model = tags
            .iter()
            .find(|t| t.name == "Model")
            .map(|t| t.print_value.clone())
            .unwrap_or_default();

        // Store model for sub-table dispatch
        let make_and_model = if model.is_empty() { make.clone() } else { model };

        // Find and parse MakerNotes
        // Look for the MakerNote tag (0x927C) that was stored as Undefined
        let mn_info: Option<(usize, usize)> = {
            // Re-scan ExifIFD for MakerNote offset/size
            let mut result = None;
            Self::find_makernote(data, &header, &mut result);
            result
        };

        if let Some((mn_offset, mn_size)) = mn_info {
            let mn_tags = crate::metadata::makernotes::parse_makernotes(
                data, mn_offset, mn_size, &make, &make_and_model, header.byte_order,
            );
            // Remove the raw MakerNote tag and replace with parsed tags
            tags.retain(|t| t.name != "MakerNote");
            tags.extend(mn_tags);
        }

        Ok(tags)
    }

    /// Find MakerNote (tag 0x927C) offset and size in ExifIFD.
    fn find_makernote(data: &[u8], header: &TiffHeader, result: &mut Option<(usize, usize)>) {
        // First find ExifIFD offset from IFD0
        let ifd0_offset = header.ifd0_offset as usize;
        if ifd0_offset + 2 > data.len() {
            return;
        }
        let entry_count = read_u16(data, ifd0_offset, header.byte_order) as usize;
        let entries_start = ifd0_offset + 2;

        for i in 0..entry_count {
            let eoff = entries_start + i * 12;
            if eoff + 12 > data.len() { break; }
            let tag = read_u16(data, eoff, header.byte_order);
            if tag == 0x8769 {
                // ExifIFD pointer
                let exif_offset = read_u32(data, eoff + 8, header.byte_order) as usize;
                Self::find_makernote_in_ifd(data, header, exif_offset, result);
                break;
            }
        }
    }

    fn find_makernote_in_ifd(data: &[u8], header: &TiffHeader, ifd_offset: usize, result: &mut Option<(usize, usize)>) {
        if ifd_offset + 2 > data.len() {
            return;
        }
        let entry_count = read_u16(data, ifd_offset, header.byte_order) as usize;
        let entries_start = ifd_offset + 2;

        for i in 0..entry_count {
            let eoff = entries_start + i * 12;
            if eoff + 12 > data.len() { break; }
            let tag = read_u16(data, eoff, header.byte_order);
            if tag == 0x927C {
                let data_type = read_u16(data, eoff + 2, header.byte_order);
                let count = read_u32(data, eoff + 4, header.byte_order) as usize;
                let type_size = match data_type { 1 | 2 | 6 | 7 => 1, 3 | 8 => 2, 4 | 9 | 11 | 13 => 4, 5 | 10 | 12 => 8, _ => 1 };
                let total_size = type_size * count;

                if total_size <= 4 {
                    // Inline - too small for real MakerNotes
                    break;
                }
                let offset = read_u32(data, eoff + 8, header.byte_order) as usize;
                if offset + total_size <= data.len() {
                    *result = Some((offset, total_size));
                }
                break;
            }
        }
    }

    /// Parse EXIF data from a byte slice with an explicit byte order and offset.
    fn read_ifd(
        data: &[u8],
        header: &TiffHeader,
        offset: u32,
        ifd_name: &str,
        tags: &mut Vec<Tag>,
    ) -> Result<Option<u32>> {
        let offset = offset as usize;
        if offset + 2 > data.len() {
            return Err(Error::InvalidExif(format!(
                "{} offset {} beyond data length {}",
                ifd_name,
                offset,
                data.len()
            )));
        }

        let entry_count = read_u16(data, offset, header.byte_order) as usize;
        let entries_start = offset + 2;
        let entries_end = entries_start + entry_count * 12;

        // Validate: at minimum, first entry must fit
        if entries_start + 12 > data.len() && entry_count > 0 {
            return Err(Error::InvalidExif(format!(
                "{} entries extend beyond data (need {}, have {})",
                ifd_name,
                entries_start + 12,
                data.len()
            )));
        }
        // Clamp entry count if IFD extends beyond data
        let entry_count = entry_count.min((data.len().saturating_sub(entries_start)) / 12);
        let entries_end = entries_start + entry_count * 12;

        for i in 0..entry_count {
            let entry_offset = entries_start + i * 12;
            let entry = parse_ifd_entry(data, entry_offset, header.byte_order);

            // Check for sub-IFDs (ExifIFD, GPS, Interop)
            match entry.tag {
                0x8769 => {
                    // ExifIFD
                    let sub_offset = entry.value_offset;
                    if (sub_offset as usize) < data.len() {
                        let _ = Self::read_ifd(data, header, sub_offset, "ExifIFD", tags);
                    }
                    continue;
                }
                0x8825 => {
                    // GPS IFD
                    let sub_offset = entry.value_offset;
                    if (sub_offset as usize) < data.len() {
                        let _ = Self::read_ifd(data, header, sub_offset, "GPS", tags);
                    }
                    continue;
                }
                0xA005 => {
                    // Interop IFD
                    let sub_offset = entry.value_offset;
                    if (sub_offset as usize) < data.len() {
                        let _ = Self::read_ifd(data, header, sub_offset, "InteropIFD", tags);
                    }
                    continue;
                }
                _ => {}
            }

            if let Some(value) = read_ifd_value(data, &entry, header.byte_order) {
                let tag_info = exif_tags::lookup(ifd_name, entry.tag);
                let (name, description, family2) = match tag_info {
                    Some(info) => (
                        info.name.to_string(),
                        info.description.to_string(),
                        info.family2.to_string(),
                    ),
                    None => {
                        // Fallback to generated tags
                        match exif_tags::lookup_generated(entry.tag) {
                            Some((n, d)) => (n.to_string(), d.to_string(), "Other".to_string()),
                            None => (
                                format!("Tag0x{:04X}", entry.tag),
                                format!("Unknown tag 0x{:04X}", entry.tag),
                                "Other".to_string(),
                            ),
                        }
                    }
                };

                let print_value =
                    exif_tags::print_conv(ifd_name, entry.tag, &value)
                        .or_else(|| {
                            // Fallback to generated print conversions
                            value.as_u64()
                                .and_then(|v| crate::tags::print_conv_generated::print_conv_by_name(&name, v as i64))
                                .map(|s| s.to_string())
                        })
                        .unwrap_or_else(|| value.to_display_string());

                tags.push(Tag {
                    id: TagId::Numeric(entry.tag),
                    name,
                    description,
                    group: TagGroup {
                        family0: "EXIF".to_string(),
                        family1: ifd_name.to_string(),
                        family2,
                    },
                    raw_value: value,
                    print_value,
                    priority: 0,
                });
            }
        }

        // Read next IFD offset
        let next_ifd_offset = if entries_end + 4 <= data.len() {
            read_u32(data, entries_end, header.byte_order)
        } else { 0 };
        if next_ifd_offset != 0 && ifd_name == "IFD0" {
            // IFD1 = thumbnail
            let _ = Self::read_ifd(data, header, next_ifd_offset, "IFD1", tags);

            // Create ThumbnailImage tag if offset+length are present
            let thumb_offset = tags.iter()
                .find(|t| t.name == "ThumbnailOffset" && t.group.family1 == "IFD1")
                .and_then(|t| t.raw_value.as_u64());
            let thumb_length = tags.iter()
                .find(|t| t.name == "ThumbnailLength" && t.group.family1 == "IFD1")
                .and_then(|t| t.raw_value.as_u64());

            if let (Some(off), Some(len)) = (thumb_offset, thumb_length) {
                let off = off as usize;
                let len = len as usize;
                if off + len <= data.len() && len > 0 {
                    tags.push(Tag {
                        id: TagId::Text("ThumbnailImage".into()),
                        name: "ThumbnailImage".into(),
                        description: "Thumbnail Image".into(),
                        group: TagGroup { family0: "EXIF".into(), family1: "IFD1".into(), family2: "Image".into() },
                        raw_value: Value::Binary(data[off..off+len].to_vec()),
                        print_value: format!("(Binary data {} bytes)", len),
                        priority: 0,
                    });
                }
            }
        }

        Ok(if next_ifd_offset != 0 {
            Some(next_ifd_offset)
        } else {
            None
        })
    }
}

fn parse_ifd_entry(data: &[u8], offset: usize, byte_order: ByteOrderMark) -> IfdEntry {
    let tag = read_u16(data, offset, byte_order);
    let data_type = read_u16(data, offset + 2, byte_order);
    let count = read_u32(data, offset + 4, byte_order);
    let value_offset = read_u32(data, offset + 8, byte_order);
    let mut inline_data = [0u8; 4];
    inline_data.copy_from_slice(&data[offset + 8..offset + 12]);

    IfdEntry {
        tag,
        data_type,
        count,
        value_offset,
        inline_data,
    }
}

fn read_ifd_value(data: &[u8], entry: &IfdEntry, byte_order: ByteOrderMark) -> Option<Value> {
    let elem_size = type_size(entry.data_type)?;
    let total_size = elem_size * entry.count as usize;

    let value_data = if total_size <= 4 {
        &entry.inline_data[..total_size]
    } else {
        let offset = entry.value_offset as usize;
        if offset + total_size > data.len() {
            return None;
        }
        &data[offset..offset + total_size]
    };

    match entry.data_type {
        // BYTE
        1 => {
            if entry.count == 1 {
                Some(Value::U8(value_data[0]))
            } else {
                Some(Value::List(value_data.iter().map(|&b| Value::U8(b)).collect()))
            }
        }
        // ASCII
        2 => {
            let s = String::from_utf8_lossy(value_data);
            Some(Value::String(s.trim_end_matches('\0').to_string()))
        }
        // SHORT
        3 => {
            if entry.count == 1 {
                Some(Value::U16(read_u16(value_data, 0, byte_order)))
            } else {
                let vals: Vec<Value> = (0..entry.count as usize)
                    .map(|i| Value::U16(read_u16(value_data, i * 2, byte_order)))
                    .collect();
                Some(Value::List(vals))
            }
        }
        // LONG
        4 | 13 => {
            if entry.count == 1 {
                Some(Value::U32(read_u32(value_data, 0, byte_order)))
            } else {
                let vals: Vec<Value> = (0..entry.count as usize)
                    .map(|i| Value::U32(read_u32(value_data, i * 4, byte_order)))
                    .collect();
                Some(Value::List(vals))
            }
        }
        // RATIONAL (unsigned)
        5 => {
            if entry.count == 1 {
                let n = read_u32(value_data, 0, byte_order);
                let d = read_u32(value_data, 4, byte_order);
                Some(Value::URational(n, d))
            } else {
                let vals: Vec<Value> = (0..entry.count as usize)
                    .map(|i| {
                        let n = read_u32(value_data, i * 8, byte_order);
                        let d = read_u32(value_data, i * 8 + 4, byte_order);
                        Value::URational(n, d)
                    })
                    .collect();
                Some(Value::List(vals))
            }
        }
        // SBYTE
        6 => {
            if entry.count == 1 {
                Some(Value::I16(value_data[0] as i8 as i16))
            } else {
                let vals: Vec<Value> = value_data
                    .iter()
                    .map(|&b| Value::I16(b as i8 as i16))
                    .collect();
                Some(Value::List(vals))
            }
        }
        // UNDEFINED
        7 => Some(Value::Undefined(value_data.to_vec())),
        // SSHORT
        8 => {
            if entry.count == 1 {
                Some(Value::I16(read_i16(value_data, 0, byte_order)))
            } else {
                let vals: Vec<Value> = (0..entry.count as usize)
                    .map(|i| Value::I16(read_i16(value_data, i * 2, byte_order)))
                    .collect();
                Some(Value::List(vals))
            }
        }
        // SLONG
        9 => {
            if entry.count == 1 {
                Some(Value::I32(read_i32(value_data, 0, byte_order)))
            } else {
                let vals: Vec<Value> = (0..entry.count as usize)
                    .map(|i| Value::I32(read_i32(value_data, i * 4, byte_order)))
                    .collect();
                Some(Value::List(vals))
            }
        }
        // SRATIONAL
        10 => {
            if entry.count == 1 {
                let n = read_i32(value_data, 0, byte_order);
                let d = read_i32(value_data, 4, byte_order);
                Some(Value::IRational(n, d))
            } else {
                let vals: Vec<Value> = (0..entry.count as usize)
                    .map(|i| {
                        let n = read_i32(value_data, i * 8, byte_order);
                        let d = read_i32(value_data, i * 8 + 4, byte_order);
                        Value::IRational(n, d)
                    })
                    .collect();
                Some(Value::List(vals))
            }
        }
        // FLOAT
        11 => {
            if entry.count == 1 {
                let bits = read_u32(value_data, 0, byte_order);
                Some(Value::F32(f32::from_bits(bits)))
            } else {
                let vals: Vec<Value> = (0..entry.count as usize)
                    .map(|i| {
                        let bits = read_u32(value_data, i * 4, byte_order);
                        Value::F32(f32::from_bits(bits))
                    })
                    .collect();
                Some(Value::List(vals))
            }
        }
        // DOUBLE
        12 => {
            if entry.count == 1 {
                let bits = read_u64(value_data, 0, byte_order);
                Some(Value::F64(f64::from_bits(bits)))
            } else {
                let vals: Vec<Value> = (0..entry.count as usize)
                    .map(|i| {
                        let bits = read_u64(value_data, i * 8, byte_order);
                        Value::F64(f64::from_bits(bits))
                    })
                    .collect();
                Some(Value::List(vals))
            }
        }
        _ => None,
    }
}

// Byte-order-aware read helpers
fn read_u16(data: &[u8], offset: usize, bo: ByteOrderMark) -> u16 {
    match bo {
        ByteOrderMark::LittleEndian => LittleEndian::read_u16(&data[offset..]),
        ByteOrderMark::BigEndian => BigEndian::read_u16(&data[offset..]),
    }
}

fn read_u32(data: &[u8], offset: usize, bo: ByteOrderMark) -> u32 {
    match bo {
        ByteOrderMark::LittleEndian => LittleEndian::read_u32(&data[offset..]),
        ByteOrderMark::BigEndian => BigEndian::read_u32(&data[offset..]),
    }
}

fn read_u64(data: &[u8], offset: usize, bo: ByteOrderMark) -> u64 {
    match bo {
        ByteOrderMark::LittleEndian => LittleEndian::read_u64(&data[offset..]),
        ByteOrderMark::BigEndian => BigEndian::read_u64(&data[offset..]),
    }
}

fn read_i16(data: &[u8], offset: usize, bo: ByteOrderMark) -> i16 {
    match bo {
        ByteOrderMark::LittleEndian => LittleEndian::read_i16(&data[offset..]),
        ByteOrderMark::BigEndian => BigEndian::read_i16(&data[offset..]),
    }
}

fn read_i32(data: &[u8], offset: usize, bo: ByteOrderMark) -> i32 {
    match bo {
        ByteOrderMark::LittleEndian => LittleEndian::read_i32(&data[offset..]),
        ByteOrderMark::BigEndian => BigEndian::read_i32(&data[offset..]),
    }
}
