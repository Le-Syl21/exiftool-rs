//! TIFF file format reader.
//!
//! TIFF files are essentially a raw IFD structure, which is the same as EXIF.
//! Many RAW formats (CR2, NEF, DNG, ARW, ORF) are TIFF-based.
//! Also handles BigTIFF (magic 0x2B) and Panasonic RW2 (magic 0x55).

use crate::error::{Error, Result};
use crate::metadata::ExifReader;
use crate::tag::Tag;

/// Extract all metadata tags from a TIFF file.
pub fn read_tiff(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 8 {
        return Err(Error::InvalidData("file too small for TIFF".into()));
    }

    let is_le = data[0] == b'I' && data[1] == b'I';
    let is_be = data[0] == b'M' && data[1] == b'M';

    if !is_le && !is_be {
        return Err(Error::InvalidData("not a TIFF file".into()));
    }

    let magic = if is_le {
        u16::from_le_bytes([data[2], data[3]])
    } else {
        u16::from_be_bytes([data[2], data[3]])
    };

    match magic {
        // Standard TIFF
        42 => ExifReader::read(data),
        // BigTIFF (magic 43) - IFD offset is 8 bytes at offset 8
        43 => {
            // BigTIFF has a different structure but IFD0 tags are similar
            // For now, read as standard TIFF (works for basic tags)
            // The ExifReader uses 32-bit offsets so may miss some data,
            // but the IFD structure is compatible for basic extraction
            let mut patched = data.to_vec();
            // Patch magic to 42 so ExifReader accepts it
            if is_le {
                patched[2] = 0x2A;
                patched[3] = 0x00;
            } else {
                patched[2] = 0x00;
                patched[3] = 0x2A;
            }
            // BigTIFF: offset size(2)=8, always 0(2), IFD offset(8)
            if data.len() >= 16 {
                let ifd_offset = if is_le {
                    u64::from_le_bytes([
                        data[8], data[9], data[10], data[11],
                        data[12], data[13], data[14], data[15],
                    ])
                } else {
                    u64::from_be_bytes([
                        data[8], data[9], data[10], data[11],
                        data[12], data[13], data[14], data[15],
                    ])
                };
                // Patch IFD0 offset to standard 4-byte location
                let offset32 = ifd_offset as u32;
                if is_le {
                    let bytes = offset32.to_le_bytes();
                    patched[4..8].copy_from_slice(&bytes);
                } else {
                    let bytes = offset32.to_be_bytes();
                    patched[4..8].copy_from_slice(&bytes);
                }
            }
            ExifReader::read(&patched)
        }
        // Panasonic RW2 (magic 0x55)
        0x55 => {
            // RW2 uses standard TIFF IFD structure but with magic 0x55
            let mut patched = data.to_vec();
            if is_le {
                patched[2] = 0x2A;
                patched[3] = 0x00;
            } else {
                patched[2] = 0x00;
                patched[3] = 0x2A;
            }
            ExifReader::read(&patched)
        }
        _ => Err(Error::InvalidData(format!("unknown TIFF magic: 0x{:04X}", magic))),
    }
}
