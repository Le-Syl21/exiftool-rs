//! WebP metadata writer.
//!
//! Rewrites WebP files with updated/added EXIF and XMP chunks.
//! WebP uses RIFF container with EXIF and XMP chunks.

use crate::error::{Error, Result};

/// Rewrite a WebP file with updated EXIF and/or XMP.
pub fn write_webp(
    source: &[u8],
    new_exif: Option<&[u8]>,
    new_xmp: Option<&[u8]>,
    remove_exif: bool,
    remove_xmp: bool,
) -> Result<Vec<u8>> {
    if source.len() < 12 || !source.starts_with(b"RIFF") || &source[8..12] != b"WEBP" {
        return Err(Error::InvalidData("not a WebP file".into()));
    }

    let mut output = Vec::with_capacity(source.len());
    // RIFF header (we'll update the size later)
    output.extend_from_slice(b"RIFF");
    output.extend_from_slice(&[0, 0, 0, 0]); // placeholder for size
    output.extend_from_slice(b"WEBP");

    let mut pos = 12;
    let mut wrote_exif = false;
    let mut wrote_xmp = false;
    let mut _has_vp8x = false;

    while pos + 8 <= source.len() {
        let chunk_id = &source[pos..pos + 4];
        let chunk_size = u32::from_le_bytes([
            source[pos + 4],
            source[pos + 5],
            source[pos + 6],
            source[pos + 7],
        ]) as usize;
        let chunk_end = pos + 8 + chunk_size;
        let padded_end = chunk_end + (chunk_size & 1); // Pad to even

        if chunk_end > source.len() {
            break;
        }

        match chunk_id {
            b"VP8X" => {
                _has_vp8x = true;
                // Copy VP8X but update flags if we're adding EXIF/XMP
                let mut vp8x_data = source[pos + 8..chunk_end].to_vec();
                if vp8x_data.len() >= 4 {
                    if new_exif.is_some() {
                        vp8x_data[0] |= 0x08; // EXIF flag
                    }
                    if new_xmp.is_some() {
                        vp8x_data[0] |= 0x04; // XMP flag
                    }
                    if remove_exif {
                        vp8x_data[0] &= !0x08;
                    }
                    if remove_xmp {
                        vp8x_data[0] &= !0x04;
                    }
                }
                write_riff_chunk(&mut output, b"VP8X", &vp8x_data);
            }
            b"EXIF" => {
                if remove_exif {
                    // Skip
                } else if let Some(exif) = new_exif {
                    write_riff_chunk(&mut output, b"EXIF", exif);
                    wrote_exif = true;
                } else {
                    // Keep original
                    copy_chunk(&mut output, source, pos, chunk_size);
                }
            }
            b"XMP " => {
                if remove_xmp {
                    // Skip
                } else if let Some(xmp) = new_xmp {
                    write_riff_chunk(&mut output, b"XMP ", xmp);
                    wrote_xmp = true;
                } else {
                    copy_chunk(&mut output, source, pos, chunk_size);
                }
            }
            _ => {
                // Copy other chunks as-is
                copy_chunk(&mut output, source, pos, chunk_size);
            }
        }

        pos = if padded_end <= source.len() {
            padded_end
        } else {
            chunk_end
        };
    }

    // Append new chunks if not yet written
    if !wrote_exif {
        if let Some(exif) = new_exif {
            write_riff_chunk(&mut output, b"EXIF", exif);
        }
    }
    if !wrote_xmp {
        if let Some(xmp) = new_xmp {
            write_riff_chunk(&mut output, b"XMP ", xmp);
        }
    }

    // Update RIFF size (total file size - 8)
    let riff_size = (output.len() - 8) as u32;
    output[4..8].copy_from_slice(&riff_size.to_le_bytes());

    Ok(output)
}

fn write_riff_chunk(output: &mut Vec<u8>, id: &[u8; 4], data: &[u8]) {
    output.extend_from_slice(id);
    output.extend_from_slice(&(data.len() as u32).to_le_bytes());
    output.extend_from_slice(data);
    if data.len() % 2 != 0 {
        output.push(0); // Pad to even
    }
}

fn copy_chunk(output: &mut Vec<u8>, source: &[u8], pos: usize, chunk_size: usize) {
    let end = pos + 8 + chunk_size;
    let padded = end + (chunk_size & 1);
    let actual_end = padded.min(source.len());
    output.extend_from_slice(&source[pos..actual_end]);
}
