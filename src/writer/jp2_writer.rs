//! JPEG 2000 / JXL metadata writer.
//! Adds/replaces uuid and xml boxes for EXIF and XMP.

use crate::error::{Error, Result};

const UUID_XMP: [u8; 16] = [
    0xBE, 0x7A, 0xCF, 0xCB, 0x97, 0xA9, 0x42, 0xE8, 0x9C, 0x71, 0x99, 0x94, 0x91, 0xE3, 0xAF, 0xAC,
];

pub fn write_jp2(
    source: &[u8],
    new_xmp: Option<&[u8]>,
    new_exif: Option<&[u8]>,
) -> Result<Vec<u8>> {
    if source.len() < 12 {
        return Err(Error::InvalidData("file too small".into()));
    }
    let mut output = Vec::with_capacity(source.len());
    let mut pos = 0;
    let mut wrote_xmp = false;

    // Copy JP2 signature if present
    if source.starts_with(&[0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20]) {
        output.extend_from_slice(&source[..12]);
        pos = 12;
    }

    while pos + 8 <= source.len() {
        let box_size = u32::from_be_bytes([
            source[pos],
            source[pos + 1],
            source[pos + 2],
            source[pos + 3],
        ]) as usize;
        let box_type = &source[pos + 4..pos + 8];
        let actual_size = if box_size == 0 {
            source.len() - pos
        } else {
            box_size
        };
        if actual_size < 8 || pos + actual_size > source.len() {
            break;
        }

        match box_type {
            b"uuid"
                if actual_size > 24
                    && source[pos + 8..pos + 24] == UUID_XMP
                    && new_xmp.is_some() =>
            {
                // Replace XMP uuid box
                if let Some(xmp) = new_xmp {
                    write_box(&mut output, b"uuid", &UUID_XMP, xmp);
                    wrote_xmp = true;
                }
            }
            b"xml " if new_xmp.is_some() => {
                // Replace xml box
                if let Some(xmp) = new_xmp {
                    let size = (xmp.len() + 8) as u32;
                    output.extend_from_slice(&size.to_be_bytes());
                    output.extend_from_slice(b"xml ");
                    output.extend_from_slice(xmp);
                    wrote_xmp = true;
                }
            }
            b"Exif" if new_exif.is_some() => {
                if let Some(exif) = new_exif {
                    let size = (exif.len() + 12) as u32; // +4 for offset prefix
                    output.extend_from_slice(&size.to_be_bytes());
                    output.extend_from_slice(b"Exif");
                    output.extend_from_slice(&[0, 0, 0, 0]); // offset
                    output.extend_from_slice(exif);
                }
            }
            _ => {
                output.extend_from_slice(&source[pos..pos + actual_size]);
            }
        }
        pos += actual_size;
    }

    // Append new XMP if not yet written
    if !wrote_xmp {
        if let Some(xmp) = new_xmp {
            write_box(&mut output, b"uuid", &UUID_XMP, xmp);
        }
    }

    Ok(output)
}

fn write_box(output: &mut Vec<u8>, box_type: &[u8; 4], prefix: &[u8], data: &[u8]) {
    let size = (8 + prefix.len() + data.len()) as u32;
    output.extend_from_slice(&size.to_be_bytes());
    output.extend_from_slice(box_type);
    output.extend_from_slice(prefix);
    output.extend_from_slice(data);
}
