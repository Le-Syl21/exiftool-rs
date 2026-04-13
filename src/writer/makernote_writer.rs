//! MakerNotes writer.
//!
//! Modifies maker note IFD entries in-place, preserving the overall structure.
//! This is the "boss final" of metadata writing because:
//! 1. MakerNotes have manufacturer-specific headers
//! 2. Offset bases vary (some relative to TIFF header, some self-contained)
//! 3. Changing a value size can break all subsequent offsets
//!
//! Strategy: only allow in-place modifications (same size or smaller).
//! For larger values, append at end of MakerNote block.

use crate::metadata::exif::ByteOrderMark;

/// Modify a MakerNote IFD entry in-place.
///
/// `mn_data` is the full MakerNote blob (mutable).
/// `ifd_offset` is the offset to the IFD within mn_data.
/// `tag_id` is the tag to modify.
/// `new_value` is the replacement value bytes.
/// `byte_order` is the byte order of the IFD.
///
/// Returns true if the tag was found and modified.
pub fn modify_makernote_tag(
    mn_data: &mut Vec<u8>,
    ifd_offset: usize,
    tag_id: u16,
    new_value: &[u8],
    byte_order: ByteOrderMark,
) -> bool {
    if ifd_offset + 2 > mn_data.len() {
        return false;
    }

    let entry_count = read_u16(mn_data, ifd_offset, byte_order) as usize;
    if entry_count == 0 || entry_count > 500 {
        return false;
    }

    let entries_start = ifd_offset + 2;

    for i in 0..entry_count {
        let eoff = entries_start + i * 12;
        if eoff + 12 > mn_data.len() {
            break;
        }

        let tag = read_u16(mn_data, eoff, byte_order);
        if tag != tag_id {
            continue;
        }

        let data_type = read_u16(mn_data, eoff + 2, byte_order);
        let count = read_u32(mn_data, eoff + 4, byte_order);
        let type_size = match data_type {
            1 | 2 | 6 | 7 => 1usize,
            3 | 8 => 2,
            4 | 9 | 11 | 13 => 4,
            5 | 10 | 12 => 8,
            _ => 1,
        };
        let old_total = type_size * count as usize;

        if new_value.len() <= 4 {
            // New value fits inline
            let new_count = (new_value.len() / type_size.max(1)) as u32;
            write_u32(mn_data, eoff + 4, new_count, byte_order);
            let mut padded = [0u8; 4];
            padded[..new_value.len()].copy_from_slice(new_value);
            mn_data[eoff + 8..eoff + 12].copy_from_slice(&padded);
            return true;
        }

        if old_total > 4 && new_value.len() <= old_total {
            // New value fits in existing location
            let value_offset = read_u32(mn_data, eoff + 8, byte_order) as usize;
            if value_offset + new_value.len() <= mn_data.len() {
                let new_count = (new_value.len() / type_size.max(1)) as u32;
                write_u32(mn_data, eoff + 4, new_count, byte_order);
                mn_data[value_offset..value_offset + new_value.len()].copy_from_slice(new_value);
                // Zero-fill remainder
                for b in &mut mn_data[value_offset + new_value.len()..value_offset + old_total] {
                    *b = 0;
                }
                return true;
            }
        }

        // Value is larger - append at end
        let new_offset = mn_data.len() as u32;
        let new_count = (new_value.len() / type_size.max(1)) as u32;
        write_u32(mn_data, eoff + 4, new_count, byte_order);
        write_u32(mn_data, eoff + 8, new_offset, byte_order);
        mn_data.extend_from_slice(new_value);
        if new_value.len() % 2 != 0 {
            mn_data.push(0);
        }
        return true;
    }

    false
}

/// Add a new tag to a MakerNote IFD.
/// This is complex because it requires shifting all subsequent entries.
/// For safety, we only support this for MakerNotes that have room or can be extended.
pub fn add_makernote_tag(
    mn_data: &mut Vec<u8>,
    ifd_offset: usize,
    tag_id: u16,
    data_type: u16,
    value: &[u8],
    byte_order: ByteOrderMark,
) -> bool {
    // This is a simplified implementation that appends the tag
    // at the end of the IFD entries list.
    // A full implementation would insert in sorted order.

    if ifd_offset + 2 > mn_data.len() {
        return false;
    }

    let entry_count = read_u16(mn_data, ifd_offset, byte_order);
    let new_count = entry_count + 1;

    // We need 12 more bytes for the new entry in the IFD
    // This requires shifting everything after the IFD entries
    // For now, only support if MakerNote can be resized
    let entries_end = ifd_offset + 2 + (entry_count as usize) * 12;
    if entries_end + 12 > mn_data.len() {
        // Extend the buffer
        mn_data.resize(entries_end + 16, 0);
    }

    // Update entry count
    write_u16(mn_data, ifd_offset, new_count, byte_order);

    // Write new entry at the end of existing entries
    let new_entry_offset = entries_end;
    let type_size = match data_type {
        1 | 2 | 6 | 7 => 1usize,
        3 | 8 => 2,
        4 | 9 | 11 | 13 => 4,
        5 | 10 | 12 => 8,
        _ => 1,
    };
    let count = (value.len() / type_size.max(1)) as u32;

    write_u16(mn_data, new_entry_offset, tag_id, byte_order);
    write_u16(mn_data, new_entry_offset + 2, data_type, byte_order);
    write_u32(mn_data, new_entry_offset + 4, count, byte_order);

    if value.len() <= 4 {
        let mut padded = [0u8; 4];
        padded[..value.len()].copy_from_slice(value);
        mn_data[new_entry_offset + 8..new_entry_offset + 12].copy_from_slice(&padded);
    } else {
        let value_offset = mn_data.len() as u32;
        write_u32(mn_data, new_entry_offset + 8, value_offset, byte_order);
        mn_data.extend_from_slice(value);
        if value.len() % 2 != 0 {
            mn_data.push(0);
        }
    }

    true
}

fn read_u16(data: &[u8], offset: usize, bo: ByteOrderMark) -> u16 {
    if offset + 2 > data.len() {
        return 0;
    }
    match bo {
        ByteOrderMark::LittleEndian => u16::from_le_bytes([data[offset], data[offset + 1]]),
        ByteOrderMark::BigEndian => u16::from_be_bytes([data[offset], data[offset + 1]]),
    }
}

fn read_u32(data: &[u8], offset: usize, bo: ByteOrderMark) -> u32 {
    if offset + 4 > data.len() {
        return 0;
    }
    match bo {
        ByteOrderMark::LittleEndian => u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]),
        ByteOrderMark::BigEndian => u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]),
    }
}

fn write_u16(data: &mut [u8], offset: usize, value: u16, bo: ByteOrderMark) {
    if offset + 2 > data.len() {
        return;
    }
    let bytes = match bo {
        ByteOrderMark::LittleEndian => value.to_le_bytes(),
        ByteOrderMark::BigEndian => value.to_be_bytes(),
    };
    data[offset..offset + 2].copy_from_slice(&bytes);
}

fn write_u32(data: &mut [u8], offset: usize, value: u32, bo: ByteOrderMark) {
    if offset + 4 > data.len() {
        return;
    }
    let bytes = match bo {
        ByteOrderMark::LittleEndian => value.to_le_bytes(),
        ByteOrderMark::BigEndian => value.to_be_bytes(),
    };
    data[offset..offset + 4].copy_from_slice(&bytes);
}
