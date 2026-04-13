//! TIFF file metadata writer.
//!
//! Rewrites TIFF files (and TIFF-based RAW: DNG, CR2, etc.) with updated metadata.

use crate::error::Result;
use crate::metadata::exif::{parse_tiff_header, ByteOrderMark};

/// Rewrite a TIFF file, updating specific IFD entries.
///
/// This is a simple implementation that modifies tag values in-place when possible,
/// or appends new data at the end of the file.
pub fn write_tiff(
    source: &[u8],
    changes: &[(u16, Vec<u8>)], // (tag_id, new_value_bytes)
) -> Result<Vec<u8>> {
    let header = parse_tiff_header(source)?;
    let bo = header.byte_order;

    let mut output = source.to_vec();

    // For each change, find the tag in IFD0 or ExifIFD and update
    let ifd0_offset = header.ifd0_offset as usize;
    for &(tag_id, ref new_data) in changes {
        if !try_update_tag(&mut output, ifd0_offset, bo, tag_id, new_data) {
            // Try ExifIFD
            if let Some(exif_off) = find_sub_ifd(&output, ifd0_offset, bo, 0x8769) {
                try_update_tag(&mut output, exif_off, bo, tag_id, new_data);
            }
        }
    }

    Ok(output)
}

/// Try to update a tag value in an IFD. Returns true if found and updated.
fn try_update_tag(
    data: &mut Vec<u8>,
    ifd_offset: usize,
    bo: ByteOrderMark,
    target_tag: u16,
    new_value: &[u8],
) -> bool {
    if ifd_offset + 2 > data.len() {
        return false;
    }

    let count = read_u16(data, ifd_offset, bo) as usize;

    for i in 0..count {
        let eoff = ifd_offset + 2 + i * 12;
        if eoff + 12 > data.len() {
            break;
        }

        let tag = read_u16(data, eoff, bo);
        if tag != target_tag {
            continue;
        }

        let old_count = read_u32(data, eoff + 4, bo);
        let dtype = read_u16(data, eoff + 2, bo);
        let type_size = match dtype {
            1 | 2 | 6 | 7 => 1usize,
            3 | 8 => 2,
            4 | 9 | 11 | 13 => 4,
            5 | 10 | 12 => 8,
            _ => 1,
        };
        let old_total = type_size * old_count as usize;

        let new_count = (new_value.len() / type_size.max(1)) as u32;

        // Update count
        match bo {
            ByteOrderMark::LittleEndian => {
                data[eoff + 4..eoff + 8].copy_from_slice(&new_count.to_le_bytes());
            }
            ByteOrderMark::BigEndian => {
                data[eoff + 4..eoff + 8].copy_from_slice(&new_count.to_be_bytes());
            }
        }

        if new_value.len() <= 4 {
            // Value fits inline
            let mut padded = [0u8; 4];
            padded[..new_value.len()].copy_from_slice(new_value);
            data[eoff + 8..eoff + 12].copy_from_slice(&padded);
        } else if old_total >= new_value.len() && old_total > 4 {
            // New value fits in old location - write in place
            let old_offset = read_u32(data, eoff + 8, bo) as usize;
            if old_offset + new_value.len() <= data.len() {
                data[old_offset..old_offset + new_value.len()].copy_from_slice(new_value);
                // Zero-fill remainder
                for b in &mut data[old_offset + new_value.len()..old_offset + old_total] {
                    *b = 0;
                }
            }
        } else {
            // Append new value at end of file
            let new_offset = data.len() as u32;
            match bo {
                ByteOrderMark::LittleEndian => {
                    data[eoff + 8..eoff + 12].copy_from_slice(&new_offset.to_le_bytes());
                }
                ByteOrderMark::BigEndian => {
                    data[eoff + 8..eoff + 12].copy_from_slice(&new_offset.to_be_bytes());
                }
            }
            data.extend_from_slice(new_value);
            // Pad to word boundary
            if new_value.len() % 2 != 0 {
                data.push(0);
            }
        }

        return true;
    }

    false
}

/// Find a sub-IFD pointer (ExifIFD=0x8769, GPS=0x8825) in an IFD.
fn find_sub_ifd(
    data: &[u8],
    ifd_offset: usize,
    bo: ByteOrderMark,
    pointer_tag: u16,
) -> Option<usize> {
    if ifd_offset + 2 > data.len() {
        return None;
    }
    let count = read_u16(data, ifd_offset, bo) as usize;

    for i in 0..count {
        let eoff = ifd_offset + 2 + i * 12;
        if eoff + 12 > data.len() {
            break;
        }
        let tag = read_u16(data, eoff, bo);
        if tag == pointer_tag {
            return Some(read_u32(data, eoff + 8, bo) as usize);
        }
    }
    None
}

fn read_u16(data: &[u8], offset: usize, bo: ByteOrderMark) -> u16 {
    match bo {
        ByteOrderMark::LittleEndian => u16::from_le_bytes([data[offset], data[offset + 1]]),
        ByteOrderMark::BigEndian => u16::from_be_bytes([data[offset], data[offset + 1]]),
    }
}

fn read_u32(data: &[u8], offset: usize, bo: ByteOrderMark) -> u32 {
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
