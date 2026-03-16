//! JPEG metadata writer.
//!
//! Rewrites JPEG files with updated/added/removed metadata segments.
//! Mirrors ExifTool's WriteJPEG in Writer.pl.

use crate::error::{Error, Result};

/// JPEG segment to write.
#[derive(Debug, Clone)]
pub struct JpegSegment {
    pub marker: u8,
    pub data: Vec<u8>,
}

/// Rewrite a JPEG file, replacing/adding/removing metadata segments.
///
/// `source` is the original JPEG data.
/// `new_exif` is the new EXIF data (including "Exif\0\0" header), or None to keep existing.
/// `new_xmp` is the new XMP data (including namespace header), or None to keep existing.
/// `new_iptc` is the new IPTC/Photoshop data, or None to keep existing.
/// `new_comment` is the new JPEG comment, or None to keep existing.
/// `remove_exif` / `remove_xmp` / `remove_iptc` / `remove_comment`: if true, remove that segment.
pub fn write_jpeg(
    source: &[u8],
    new_exif: Option<&[u8]>,
    new_xmp: Option<&[u8]>,
    new_iptc: Option<&[u8]>,
    new_comment: Option<&str>,
    remove_exif: bool,
    remove_xmp: bool,
    remove_iptc: bool,
    remove_comment: bool,
) -> Result<Vec<u8>> {
    if source.len() < 2 || source[0] != 0xFF || source[1] != 0xD8 {
        return Err(Error::InvalidData("not a JPEG file".into()));
    }

    let mut output = Vec::with_capacity(source.len());
    output.extend_from_slice(&[0xFF, 0xD8]); // SOI

    let mut pos = 2;
    let mut wrote_exif = false;
    let mut wrote_xmp = false;
    let mut wrote_iptc = false;
    let mut wrote_comment = false;
    let mut past_first_segment = false;

    // Write new segments before the first existing segment
    // (ExifTool writes EXIF first, then XMP, then IPTC)

    while pos + 4 <= source.len() {
        // Find next marker
        if source[pos] != 0xFF {
            pos += 1;
            continue;
        }

        let marker = source[pos + 1];
        pos += 2;

        // Skip padding 0xFF bytes
        if marker == 0xFF || marker == 0x00 {
            continue;
        }

        // SOS: end of metadata - write any remaining new segments then copy rest
        if marker == 0xDA {
            // Before SOS, inject any new segments we haven't written yet
            if !wrote_exif {
                if let Some(exif) = new_exif {
                    write_app1_exif(&mut output, exif);
                }
            }
            if !wrote_xmp {
                if let Some(xmp) = new_xmp {
                    write_app1_xmp(&mut output, xmp);
                }
            }
            if !wrote_iptc {
                if let Some(iptc) = new_iptc {
                    write_app13_iptc(&mut output, iptc);
                }
            }
            if !wrote_comment {
                if let Some(comment) = new_comment {
                    write_com(&mut output, comment);
                }
            }

            // Copy SOS marker and all remaining data (image data + EOI)
            output.extend_from_slice(&[0xFF, marker]);
            output.extend_from_slice(&source[pos..]);
            return Ok(output);
        }

        // Markers without payload (restart markers, etc.)
        if marker == 0xD8 || (0xD0..=0xD7).contains(&marker) {
            output.extend_from_slice(&[0xFF, marker]);
            continue;
        }

        // Read segment length
        if pos + 2 > source.len() {
            break;
        }
        let seg_len = u16::from_be_bytes([source[pos], source[pos + 1]]) as usize;
        if seg_len < 2 || pos + seg_len > source.len() {
            break;
        }

        let seg_data = &source[pos + 2..pos + seg_len];

        // On first non-SOI segment, inject new segments if needed
        if !past_first_segment {
            past_first_segment = true;
            // Write new EXIF before existing segments (unless we'll replace an existing one)
            if new_exif.is_some() && !is_exif_segment(marker, seg_data) {
                if let Some(exif) = new_exif {
                    write_app1_exif(&mut output, exif);
                    wrote_exif = true;
                }
            }
        }

        // Decide what to do with this segment
        match marker {
            // APP1 - EXIF or XMP
            0xE1 => {
                if is_exif_segment(marker, seg_data) {
                    if remove_exif {
                        // Skip (remove)
                    } else if let Some(exif) = new_exif {
                        if !wrote_exif {
                            write_app1_exif(&mut output, exif);
                            wrote_exif = true;
                        }
                        // Skip original EXIF segment (replaced)
                    } else {
                        // Keep original
                        write_segment(&mut output, marker, &source[pos..pos + seg_len]);
                    }
                } else if is_xmp_segment(seg_data) {
                    if remove_xmp {
                        // Skip
                    } else if let Some(xmp) = new_xmp {
                        if !wrote_xmp {
                            write_app1_xmp(&mut output, xmp);
                            wrote_xmp = true;
                        }
                    } else {
                        write_segment(&mut output, marker, &source[pos..pos + seg_len]);
                    }
                } else {
                    // Other APP1 segment - keep
                    write_segment(&mut output, marker, &source[pos..pos + seg_len]);
                }
            }
            // APP13 - Photoshop/IPTC
            0xED => {
                if remove_iptc {
                    // Skip
                } else if let Some(iptc) = new_iptc {
                    if !wrote_iptc {
                        write_app13_iptc(&mut output, iptc);
                        wrote_iptc = true;
                    }
                } else {
                    write_segment(&mut output, marker, &source[pos..pos + seg_len]);
                }
            }
            // COM - Comment
            0xFE => {
                if remove_comment {
                    // Skip
                } else if let Some(comment) = new_comment {
                    if !wrote_comment {
                        write_com(&mut output, comment);
                        wrote_comment = true;
                    }
                } else {
                    write_segment(&mut output, marker, &source[pos..pos + seg_len]);
                }
            }
            // All other segments - copy as-is
            _ => {
                write_segment(&mut output, marker, &source[pos..pos + seg_len]);
            }
        }

        pos += seg_len;
    }

    // If we never hit SOS (unusual but possible for metadata-only JPEG)
    Ok(output)
}

fn is_exif_segment(_marker: u8, data: &[u8]) -> bool {
    data.len() > 6 && data.starts_with(b"Exif\0\0")
}

fn is_xmp_segment(data: &[u8]) -> bool {
    data.len() > 29 && data.starts_with(b"http://ns.adobe.com/xap/1.0/\0")
}

fn write_segment(output: &mut Vec<u8>, marker: u8, data: &[u8]) {
    output.push(0xFF);
    output.push(marker);
    output.extend_from_slice(data);
}

fn write_app1_exif(output: &mut Vec<u8>, exif_data: &[u8]) {
    // APP1 marker + length + "Exif\0\0" + TIFF data
    let mut segment = Vec::new();
    segment.extend_from_slice(b"Exif\0\0");
    segment.extend_from_slice(exif_data);

    let total_len = (segment.len() + 2) as u16; // +2 for length field itself
    output.push(0xFF);
    output.push(0xE1);
    output.extend_from_slice(&total_len.to_be_bytes());
    output.extend_from_slice(&segment);
}

fn write_app1_xmp(output: &mut Vec<u8>, xmp_data: &[u8]) {
    let mut segment = Vec::new();
    segment.extend_from_slice(b"http://ns.adobe.com/xap/1.0/\0");
    segment.extend_from_slice(xmp_data);

    // XMP can be large; split if needed (>65533 bytes)
    if segment.len() + 2 > 65535 {
        // Write only what fits; extended XMP not yet supported
        let max = 65533;
        let total_len = (max + 2) as u16;
        output.push(0xFF);
        output.push(0xE1);
        output.extend_from_slice(&total_len.to_be_bytes());
        output.extend_from_slice(&segment[..max]);
    } else {
        let total_len = (segment.len() + 2) as u16;
        output.push(0xFF);
        output.push(0xE1);
        output.extend_from_slice(&total_len.to_be_bytes());
        output.extend_from_slice(&segment);
    }
}

fn write_app13_iptc(output: &mut Vec<u8>, iptc_data: &[u8]) {
    let mut segment = Vec::new();
    segment.extend_from_slice(b"Photoshop 3.0\0");
    // Wrap in 8BIM resource block
    segment.extend_from_slice(b"8BIM");
    segment.extend_from_slice(&0x0404u16.to_be_bytes()); // IPTC resource ID
    segment.push(0); // Pascal string name (length 0)
    segment.push(0); // Padding
    segment.extend_from_slice(&(iptc_data.len() as u32).to_be_bytes());
    segment.extend_from_slice(iptc_data);
    if iptc_data.len() % 2 != 0 {
        segment.push(0); // Pad to even
    }

    let total_len = (segment.len() + 2) as u16;
    output.push(0xFF);
    output.push(0xED);
    output.extend_from_slice(&total_len.to_be_bytes());
    output.extend_from_slice(&segment);
}

fn write_com(output: &mut Vec<u8>, comment: &str) {
    let data = comment.as_bytes();
    let total_len = (data.len() + 2) as u16;
    output.push(0xFF);
    output.push(0xFE);
    output.extend_from_slice(&total_len.to_be_bytes());
    output.extend_from_slice(data);
}
