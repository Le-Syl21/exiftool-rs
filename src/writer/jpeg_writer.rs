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
/// `new_exif` is the new TIFF block of the EXIF segment (without the
/// "Exif\0\0" header), or None to keep existing. It replaces the file's EXIF
/// segment where it is, or, when the file has none, goes right after the
/// leading APP0 segments (JFIF), where ExifTool creates it.
/// `new_xmp` is the new XMP data (including namespace header), or None to keep existing.
/// `new_iptc` is the new IPTC/Photoshop data, or None to keep existing.
/// `new_comment` is the new JPEG comment, or None to keep existing.
/// `remove_exif` / `remove_xmp` / `remove_iptc` / `remove_comment`: if true, remove that segment.
#[allow(clippy::too_many_arguments)]
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
    let has_exif = extract_jpeg_exif(source).is_some();

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
                    write_app1_exif(&mut output, exif)?;
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
            let out_pos = output.len();
            output.extend_from_slice(&source[pos..]);
            // What follows the scan moved by the size change of the
            // metadata: fix the absolute and MPF-relative offsets into it.
            let shift = out_pos as i64 - pos as i64;
            if shift != 0 {
                shift_afcp_trailer(&mut output, pos, shift);
                shift_mpf_offsets(source, &mut output, pos, shift);
            }
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

        // A new EXIF segment comes right after the APP0 segments
        // (Writer.pl WriteJPEG: "EXIF information must come immediately
        // after APP0").
        if !has_exif && !wrote_exif && marker != 0xE0 {
            if let Some(exif) = new_exif {
                write_app1_exif(&mut output, exif)?;
                wrote_exif = true;
            }
        }

        // Decide what to do with this segment
        match marker {
            // APP1 - EXIF or XMP
            0xE1 => {
                if is_exif_segment(marker, seg_data) {
                    if remove_exif {
                        // Skip (remove)
                    } else if let (Some(exif), false) = (new_exif, wrote_exif) {
                        // The first EXIF segment is the one that was edited.
                        write_app1_exif(&mut output, exif)?;
                        wrote_exif = true;
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

fn write_app1_exif(output: &mut Vec<u8>, exif_data: &[u8]) -> Result<()> {
    // APP1 marker + length + "Exif\0\0" + TIFF data
    let mut segment = Vec::new();
    segment.extend_from_slice(b"Exif\0\0");
    segment.extend_from_slice(exif_data);

    // +2 for the length field itself. ExifTool would split a bigger block
    // into a non-standard multi-segment EXIF; that is not written here.
    let total_len = u16::try_from(segment.len() + 2)
        .map_err(|_| Error::InvalidData("EXIF is too large for JPEG segment".into()))?;
    output.push(0xFF);
    output.push(0xE1);
    output.extend_from_slice(&total_len.to_be_bytes());
    output.extend_from_slice(&segment);
    Ok(())
}

/// The TIFF block (past "Exif\0\0") of the first APP1 EXIF segment of a
/// JPEG, or `None` if the file has no EXIF.
pub fn extract_jpeg_exif(jpeg: &[u8]) -> Option<&[u8]> {
    let mut pos = 2; // skip SOI (FF D8)
    while pos + 4 <= jpeg.len() {
        if jpeg[pos] != 0xFF {
            break;
        }
        let marker = jpeg[pos + 1];
        // Fill bytes and standalone markers carry no length payload.
        if marker == 0xFF {
            pos += 1;
            continue;
        }
        if marker == 0x00 || marker == 0x01 || (0xD0..=0xD9).contains(&marker) {
            pos += 2;
            continue;
        }
        if marker == 0xDA {
            break; // start of scan: compressed image data follows
        }
        let len = u16::from_be_bytes([jpeg[pos + 2], jpeg[pos + 3]]) as usize;
        if len < 2 || pos + 2 + len > jpeg.len() {
            break;
        }
        let seg = &jpeg[pos + 4..pos + 2 + len];
        if marker == 0xE1 && is_exif_segment(marker, seg) {
            return Some(&seg[6..]);
        }
        pos += 2 + len;
    }
    None
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

/// Extract the raw IPTC-IIM bytes (Photoshop IRB resource `0x0404`) from a
/// JPEG, so a write can merge into the existing IPTC rather than replacing it
/// and dropping every other dataset (issue #7). Returns `None` if the file
/// has no IPTC.
pub fn extract_jpeg_iptc_iim(jpeg: &[u8]) -> Option<Vec<u8>> {
    const PS_HEADER: &[u8] = b"Photoshop 3.0\0";
    let mut pos = 2; // skip SOI (FF D8)
    while pos + 4 <= jpeg.len() {
        if jpeg[pos] != 0xFF {
            break;
        }
        let marker = jpeg[pos + 1];
        // Standalone markers carry no length payload.
        if marker == 0x01 || (0xD0..=0xD9).contains(&marker) {
            pos += 2;
            continue;
        }
        if marker == 0xDA {
            break; // start of scan: compressed image data follows
        }
        let len = u16::from_be_bytes([jpeg[pos + 2], jpeg[pos + 3]]) as usize;
        if len < 2 || pos + 2 + len > jpeg.len() {
            break;
        }
        let seg = &jpeg[pos + 4..pos + 2 + len];
        if marker == 0xED && seg.starts_with(PS_HEADER) {
            return crate::formats::jpeg::extract_photoshop_irbs(&seg[PS_HEADER.len()..]).0;
        }
        pos += 2 + len;
    }
    None
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

/// Offsets of the segments of a JPEG up to the start of scan: `(marker,
/// position of the segment data (past the length), data length)`.
fn segments(jpeg: &[u8]) -> Vec<(u8, usize, usize)> {
    let mut out = Vec::new();
    let mut pos = 2;
    while pos + 4 <= jpeg.len() {
        if jpeg[pos] != 0xFF {
            break;
        }
        let marker = jpeg[pos + 1];
        if marker == 0xFF {
            pos += 1;
            continue;
        }
        if marker == 0x00 || marker == 0x01 || (0xD0..=0xD9).contains(&marker) {
            pos += 2;
            continue;
        }
        if marker == 0xDA {
            break;
        }
        let len = u16::from_be_bytes([jpeg[pos + 2], jpeg[pos + 3]]) as usize;
        if len < 2 || pos + 2 + len > jpeg.len() {
            break;
        }
        out.push((marker, pos + 4, len - 2));
        pos += 2 + len;
    }
    out
}

/// The resolution of the JFIF APP0 segment of a JPEG, when it comes before
/// the EXIF segment (or the file has no EXIF): ExifTool then uses it for
/// the resolution tags of a new IFD0.
pub fn jfif_resolution_before_exif(
    jpeg: &[u8],
) -> Option<crate::writer::exif_edit::JfifResolution> {
    for (marker, at, len) in segments(jpeg) {
        let seg = &jpeg[at..at + len];
        if marker == 0xE1 && is_exif_segment(marker, seg) {
            return None;
        }
        if marker == 0xE0 && seg.len() >= 12 && seg.starts_with(b"JFIF\0") {
            return Some(crate::writer::exif_edit::JfifResolution {
                unit: seg[7],
                x: u16::from_be_bytes([seg[8], seg[9]]),
                y: u16::from_be_bytes([seg[10], seg[11]]),
            });
        }
    }
    None
}

/// The resolution of the first JFIF APP0 segment of a JPEG, wherever it is.
pub fn jfif_resolution(jpeg: &[u8]) -> Option<crate::writer::exif_edit::JfifResolution> {
    segments(jpeg).into_iter().find_map(|(marker, at, len)| {
        let seg = &jpeg[at..at + len];
        (marker == 0xE0 && seg.len() >= 12 && seg.starts_with(b"JFIF\0")).then(|| {
            crate::writer::exif_edit::JfifResolution {
                unit: seg[7],
                x: u16::from_be_bytes([seg[8], seg[9]]),
                y: u16::from_be_bytes([seg[10], seg[11]]),
            }
        })
    })
}

/// Fix the offsets into what follows the scan data (an AFCP trailer, MPF
/// images) after a rewrite of `source` into `output` moved the scan data
/// from `src_scan` to `out_scan`: the IPTC and XMP pass
/// (`jpeg_iptc_xmp`) changes segment sizes after `write_jpeg`.
pub(crate) fn fix_offsets_after_scan(
    source: &[u8],
    output: &mut [u8],
    src_scan: usize,
    out_scan: usize,
) {
    let shift = out_scan as i64 - src_scan as i64;
    if shift != 0 {
        shift_afcp_trailer(output, src_scan, shift);
        shift_mpf_offsets(source, output, src_scan, shift);
    }
}

/// Shift the absolute offsets of an AFCP trailer (AFCP.pm) that moved by
/// `shift` bytes in the output. `src_scan` is where the source's scan data
/// starts: everything from there on was copied unchanged, at `src_scan +
/// shift` in the output. The trailer is found from its end record (`AXS!` or
/// `AXS*`, the start offset, a checksum), the last one whose start offset
/// leads to a matching header: other trailers (Samsung, MIE...) may follow
/// it. A trailer whose offsets were already wrong is left as it is, as
/// ExifTool only adjusts them when it reads the file.
fn shift_afcp_trailer(out: &mut [u8], src_scan: usize, shift: i64) {
    let n = out.len();
    let Some(out_scan) = usize::try_from(src_scan as i64 + shift).ok() else {
        return;
    };
    if n < out_scan + 24 {
        return;
    }
    let mut p = n - 12;
    loop {
        if &out[p..p + 3] == b"AXS"
            && matches!(out[p + 3], b'!' | b'*')
            && patch_afcp(out, p, src_scan, out_scan, shift)
        {
            return;
        }
        if p == out_scan {
            return;
        }
        p -= 1;
    }
}

/// Patch the AFCP trailer whose end record is at `end` in `out`, if it is
/// one. Returns whether it was.
fn patch_afcp(out: &mut [u8], end: usize, src_scan: usize, out_scan: usize, shift: i64) -> bool {
    let be = out[end + 3] == b'!';
    let get = |b: &[u8], p: usize| {
        let w = [b[p], b[p + 1], b[p + 2], b[p + 3]];
        if be {
            u32::from_be_bytes(w)
        } else {
            u32::from_le_bytes(w)
        }
    };
    let put = |v: u32| if be { v.to_be_bytes() } else { v.to_le_bytes() };
    let start = get(out, end + 4) as usize;
    let Some(new_start) = usize::try_from(start as i64 + shift).ok() else {
        return false;
    };
    // The header must be in the copied part, before the end record, and
    // where the (old) start offset says.
    if start < src_scan || new_start < out_scan || new_start + 12 > end {
        return false;
    }
    if out[new_start..new_start + 4] != out[end..end + 4] {
        return false;
    }
    let count = if be {
        u16::from_be_bytes([out[new_start + 6], out[new_start + 7]])
    } else {
        u16::from_le_bytes([out[new_start + 6], out[new_start + 7]])
    } as usize;
    if new_start + 12 + 12 * count > end {
        return false;
    }
    for i in 0..count {
        let p = new_start + 12 + 12 * i + 8;
        let off = get(out, p) as usize;
        if off >= start {
            if let Ok(v) = u32::try_from(off as i64 + shift) {
                out[p..p + 4].copy_from_slice(&put(v));
            }
        }
    }
    match u32::try_from(new_start) {
        Ok(v) => {
            out[end + 4..end + 8].copy_from_slice(&put(v));
            true
        }
        Err(_) => false,
    }
}

/// Shift the image offsets of an MPF APP2 segment (CIPA DC-007) after a
/// rewrite: they are relative to the segment's TIFF header and point past
/// the end of the first image, so they change when a segment between the
/// MPF segment and the scan data changed size. `src_scan` is where the
/// source's scan data starts and `shift` how far it moved in the output.
fn shift_mpf_offsets(src: &[u8], out: &mut [u8], src_scan: usize, shift: i64) {
    let find = |b: &[u8]| {
        segments(b)
            .into_iter()
            .find(|&(m, at, len)| m == 0xE2 && len >= 12 && b[at..].starts_with(b"MPF\0"))
            .map(|(_, at, len)| (at + 4, len - 4))
    };
    let (Some((src_hdr, _)), Some((out_hdr, out_len))) = (find(src), find(out)) else {
        return;
    };
    // How much the images moved relative to the MPF header.
    let rel = shift - (out_hdr as i64 - src_hdr as i64);
    if rel == 0 {
        return;
    }
    let tiff = &out[out_hdr..out_hdr + out_len];
    let be = match &tiff[..4] {
        b"MM\0\x2a" => true,
        b"II\x2a\0" => false,
        _ => return,
    };
    let u16_at = |p: usize| {
        let w = [tiff[p], tiff[p + 1]];
        if be {
            u16::from_be_bytes(w)
        } else {
            u16::from_le_bytes(w)
        }
    };
    let u32_at = |p: usize| {
        let w = [tiff[p], tiff[p + 1], tiff[p + 2], tiff[p + 3]];
        if be {
            u32::from_be_bytes(w)
        } else {
            u32::from_le_bytes(w)
        }
    };
    let ifd = u32_at(4) as usize;
    if ifd + 2 > tiff.len() {
        return;
    }
    let n = u16_at(ifd) as usize;
    if ifd + 2 + 12 * n > tiff.len() {
        return;
    }
    let Some(e) = (0..n)
        .map(|i| ifd + 2 + 12 * i)
        .find(|&e| u16_at(e) == 0xB002 && u16_at(e + 2) == 7)
    else {
        return;
    };
    let count = u32_at(e + 4) as usize;
    let list = if count <= 4 {
        e + 8
    } else {
        u32_at(e + 8) as usize
    };
    if count % 16 != 0 || list + count > tiff.len() {
        return;
    }
    let mut patches = Vec::new();
    for i in 0..count / 16 {
        let p = list + 16 * i + 8;
        let off = u32_at(p) as usize;
        // Offset 0 is the first image itself; patch only what lies after the
        // source's scan start.
        if off != 0 && src_hdr + off >= src_scan {
            if let Ok(v) = u32::try_from(off as i64 + rel) {
                patches.push((
                    out_hdr + p,
                    if be { v.to_be_bytes() } else { v.to_le_bytes() },
                ));
            }
        }
    }
    for (p, v) in patches {
        out[p..p + 4].copy_from_slice(&v);
    }
}
