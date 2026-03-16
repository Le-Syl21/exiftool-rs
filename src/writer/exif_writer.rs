//! EXIF/TIFF IFD writer.
//!
//! Builds TIFF header + IFD entries from a set of tags.
//! Mirrors ExifTool's WriteExif.pl.

use crate::error::Result;
use crate::metadata::exif::ByteOrderMark;

/// TIFF data format constants.
#[derive(Debug, Clone, Copy)]
pub enum ExifFormat {
    Byte = 1,
    Ascii = 2,
    Short = 3,
    Long = 4,
    Rational = 5,
    SByte = 6,
    Undefined = 7,
    SShort = 8,
    SLong = 9,
    SRational = 10,
    Float = 11,
    Double = 12,
}

impl ExifFormat {
    pub fn size(self) -> usize {
        match self {
            ExifFormat::Byte | ExifFormat::Ascii | ExifFormat::SByte | ExifFormat::Undefined => 1,
            ExifFormat::Short | ExifFormat::SShort => 2,
            ExifFormat::Long | ExifFormat::SLong | ExifFormat::Float => 4,
            ExifFormat::Rational | ExifFormat::SRational | ExifFormat::Double => 8,
        }
    }
}

/// An IFD entry to write.
#[derive(Debug, Clone)]
pub struct IfdEntry {
    pub tag: u16,
    pub format: ExifFormat,
    pub data: Vec<u8>,
}

/// Build a complete TIFF/EXIF blob from IFD entries.
///
/// Returns the raw bytes including TIFF header, IFD0, and optional sub-IFDs.
pub fn build_exif(
    ifd0_entries: &[IfdEntry],
    exif_ifd_entries: &[IfdEntry],
    gps_entries: &[IfdEntry],
    byte_order: ByteOrderMark,
) -> Result<Vec<u8>> {
    let mut output = Vec::new();

    // TIFF header (8 bytes)
    match byte_order {
        ByteOrderMark::LittleEndian => {
            output.extend_from_slice(b"II");
            output.extend_from_slice(&42u16.to_le_bytes());
            output.extend_from_slice(&8u32.to_le_bytes()); // IFD0 offset
        }
        ByteOrderMark::BigEndian => {
            output.extend_from_slice(b"MM");
            output.extend_from_slice(&42u16.to_be_bytes());
            output.extend_from_slice(&8u32.to_be_bytes());
        }
    }

    // Collect all entries for IFD0, including sub-IFD pointers
    let mut all_ifd0 = ifd0_entries.to_vec();

    // We'll need to fixup ExifIFD and GPS pointers later
    let exif_ifd_pointer_idx = if !exif_ifd_entries.is_empty() {
        let idx = all_ifd0.len();
        all_ifd0.push(IfdEntry {
            tag: 0x8769, // ExifIFD pointer
            format: ExifFormat::Long,
            data: vec![0, 0, 0, 0], // placeholder
        });
        Some(idx)
    } else {
        None
    };

    let gps_pointer_idx = if !gps_entries.is_empty() {
        let idx = all_ifd0.len();
        all_ifd0.push(IfdEntry {
            tag: 0x8825, // GPS IFD pointer
            format: ExifFormat::Long,
            data: vec![0, 0, 0, 0], // placeholder
        });
        Some(idx)
    } else {
        None
    };

    // Sort IFD0 by tag ID
    all_ifd0.sort_by_key(|e| e.tag);

    // Build IFD0
    let (ifd0_bytes, ifd0_overflow) = build_ifd(&all_ifd0, byte_order, output.len());
    let _ifd0_end = output.len() + ifd0_bytes.len() + ifd0_overflow.len() + 4; // +4 for next IFD pointer

    output.extend_from_slice(&ifd0_bytes);
    output.extend_from_slice(&write_u32(0, byte_order)); // Next IFD = 0 (no IFD1)
    output.extend_from_slice(&ifd0_overflow);

    // Build ExifIFD
    if !exif_ifd_entries.is_empty() {
        let exif_ifd_offset = output.len() as u32;
        // Fixup the pointer in IFD0
        if let Some(idx) = exif_ifd_pointer_idx {
            fixup_ifd_pointer(&mut output, &all_ifd0, idx, exif_ifd_offset, byte_order, 8);
        }

        let mut sorted_exif = exif_ifd_entries.to_vec();
        sorted_exif.sort_by_key(|e| e.tag);

        let (exif_bytes, exif_overflow) = build_ifd(&sorted_exif, byte_order, output.len());
        output.extend_from_slice(&exif_bytes);
        output.extend_from_slice(&write_u32(0, byte_order)); // Next IFD = 0
        output.extend_from_slice(&exif_overflow);
    }

    // Build GPS IFD
    if !gps_entries.is_empty() {
        let gps_ifd_offset = output.len() as u32;
        if let Some(idx) = gps_pointer_idx {
            fixup_ifd_pointer(&mut output, &all_ifd0, idx, gps_ifd_offset, byte_order, 8);
        }

        let mut sorted_gps = gps_entries.to_vec();
        sorted_gps.sort_by_key(|e| e.tag);

        let (gps_bytes, gps_overflow) = build_ifd(&sorted_gps, byte_order, output.len());
        output.extend_from_slice(&gps_bytes);
        output.extend_from_slice(&write_u32(0, byte_order));
        output.extend_from_slice(&gps_overflow);
    }

    Ok(output)
}

/// Build a single IFD. Returns (ifd_entries_bytes, overflow_data).
fn build_ifd(
    entries: &[IfdEntry],
    byte_order: ByteOrderMark,
    base_offset: usize,
) -> (Vec<u8>, Vec<u8>) {
    let mut ifd = Vec::new();
    let mut overflow = Vec::new();

    // Entry count
    ifd.extend_from_slice(&write_u16(entries.len() as u16, byte_order));

    // Calculate where overflow data starts
    // IFD: 2 (count) + entries.len() * 12 + 4 (next IFD pointer)
    let overflow_start = base_offset + 2 + entries.len() * 12 + 4;

    for entry in entries {
        let count = entry.data.len() / entry.format.size().max(1);

        ifd.extend_from_slice(&write_u16(entry.tag, byte_order));
        ifd.extend_from_slice(&write_u16(entry.format as u16, byte_order));
        ifd.extend_from_slice(&write_u32(count as u32, byte_order));

        if entry.data.len() <= 4 {
            // Inline value (pad to 4 bytes)
            let mut padded = [0u8; 4];
            padded[..entry.data.len()].copy_from_slice(&entry.data);
            ifd.extend_from_slice(&padded);
        } else {
            // Offset to overflow
            let offset = (overflow_start + overflow.len()) as u32;
            ifd.extend_from_slice(&write_u32(offset, byte_order));
            overflow.extend_from_slice(&entry.data);
            // Pad to word boundary
            if entry.data.len() % 2 != 0 {
                overflow.push(0);
            }
        }
    }

    (ifd, overflow)
}

/// Fixup a pointer in an already-written IFD.
fn fixup_ifd_pointer(
    output: &mut [u8],
    _entries: &[IfdEntry],
    entry_idx: usize,
    target_offset: u32,
    byte_order: ByteOrderMark,
    ifd_start: usize,
) {
    // Position of this entry's value field in output
    let entry_pos = ifd_start + 2 + entry_idx * 12 + 8; // +8 to skip tag(2)+format(2)+count(4)
    if entry_pos + 4 <= output.len() {
        let bytes = write_u32(target_offset, byte_order);
        output[entry_pos..entry_pos + 4].copy_from_slice(&bytes);
    }
}

// ============================================================================
// Value encoding helpers
// ============================================================================

/// Encode a string as EXIF ASCII (null-terminated).
pub fn encode_ascii(s: &str) -> Vec<u8> {
    let mut data = s.as_bytes().to_vec();
    data.push(0); // null terminator
    data
}

/// Encode a u16 value.
pub fn encode_u16(v: u16, bo: ByteOrderMark) -> Vec<u8> {
    write_u16(v, bo).to_vec()
}

/// Encode a u32 value.
pub fn encode_u32(v: u32, bo: ByteOrderMark) -> Vec<u8> {
    write_u32(v, bo).to_vec()
}

/// Encode an unsigned rational (numerator/denominator).
pub fn encode_urational(num: u32, den: u32, bo: ByteOrderMark) -> Vec<u8> {
    let mut data = Vec::with_capacity(8);
    data.extend_from_slice(&write_u32(num, bo));
    data.extend_from_slice(&write_u32(den, bo));
    data
}

/// Encode a signed rational.
pub fn encode_srational(num: i32, den: i32, bo: ByteOrderMark) -> Vec<u8> {
    let mut data = Vec::with_capacity(8);
    data.extend_from_slice(&write_u32(num as u32, bo));
    data.extend_from_slice(&write_u32(den as u32, bo));
    data
}

fn write_u16(v: u16, bo: ByteOrderMark) -> [u8; 2] {
    match bo {
        ByteOrderMark::LittleEndian => v.to_le_bytes(),
        ByteOrderMark::BigEndian => v.to_be_bytes(),
    }
}

fn write_u32(v: u32, bo: ByteOrderMark) -> [u8; 4] {
    match bo {
        ByteOrderMark::LittleEndian => v.to_le_bytes(),
        ByteOrderMark::BigEndian => v.to_be_bytes(),
    }
}
