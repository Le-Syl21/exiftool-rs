//! Miscellaneous format readers for less common file types.
//!
//! Each format has a minimal reader extracting basic metadata.

use crate::error::{Error, Result};
use crate::metadata::XmpReader;
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

// ============================================================================
// DICOM (Digital Imaging and Communications in Medicine)
// ============================================================================

pub fn read_dicom(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 136 || &data[128..132] != b"DICM" {
        return Err(Error::InvalidData("not a DICOM file".into()));
    }
    let mut tags = Vec::new();
    tags.push(mktag("DICOM", "FileFormat", "File Format", Value::String("DICOM".into())));

    // Parse DICOM data elements (group, element, VR, length, value)
    let mut pos = 132;
    let mut count = 0;
    while pos + 8 <= data.len() && count < 100 {
        let group = u16::from_le_bytes([data[pos], data[pos + 1]]);
        let element = u16::from_le_bytes([data[pos + 2], data[pos + 3]]);
        // Check for explicit VR
        let vr = &data[pos + 4..pos + 6];
        let (val_len, hdr_size) = if vr[0].is_ascii_uppercase() && vr[1].is_ascii_uppercase() {
            let len = u16::from_le_bytes([data[pos + 6], data[pos + 7]]) as usize;
            (len, 8)
        } else {
            let len = u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]) as usize;
            (len, 8)
        };
        pos += hdr_size;

        if val_len == 0 || val_len > 10000 || pos + val_len > data.len() {
            pos += val_len.min(data.len() - pos);
            count += 1;
            continue;
        }

        let val_data = &data[pos..pos + val_len];
        let text = String::from_utf8_lossy(val_data).trim().trim_end_matches('\0').to_string();

        match (group, element) {
            (0x0008, 0x0060) => tags.push(mktag("DICOM", "Modality", "Modality", Value::String(text))),
            (0x0008, 0x0070) => tags.push(mktag("DICOM", "Manufacturer", "Manufacturer", Value::String(text))),
            (0x0008, 0x1030) => tags.push(mktag("DICOM", "StudyDescription", "Study Description", Value::String(text))),
            (0x0010, 0x0010) => tags.push(mktag("DICOM", "PatientName", "Patient Name", Value::String(text))),
            (0x0010, 0x0020) => tags.push(mktag("DICOM", "PatientID", "Patient ID", Value::String(text))),
            (0x0028, 0x0010) => {
                if val_len == 2 {
                    let v = u16::from_le_bytes([val_data[0], val_data[1]]);
                    tags.push(mktag("DICOM", "Rows", "Image Rows", Value::U16(v)));
                }
            }
            (0x0028, 0x0011) => {
                if val_len == 2 {
                    let v = u16::from_le_bytes([val_data[0], val_data[1]]);
                    tags.push(mktag("DICOM", "Columns", "Image Columns", Value::U16(v)));
                }
            }
            _ => {}
        }

        pos += val_len;
        count += 1;
        if group > 0x0028 { break; } // Stop after image dimensions
    }

    Ok(tags)
}

// ============================================================================
// FITS (Flexible Image Transport System)
// ============================================================================

pub fn read_fits(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 80 || !data.starts_with(b"SIMPLE  =") {
        return Err(Error::InvalidData("not a FITS file".into()));
    }

    let mut tags = Vec::new();
    // FITS header: 80-byte fixed-width keyword records
    let mut pos = 0;
    while pos + 80 <= data.len() {
        let record = &data[pos..pos + 80];
        let keyword = String::from_utf8_lossy(&record[..8]).trim().to_string();
        pos += 80;

        if keyword == "END" { break; }
        if keyword.is_empty() { continue; }

        if record.len() > 10 && record[8] == b'=' {
            let value = String::from_utf8_lossy(&record[10..]).trim().to_string();
            let value = value.split('/').next().unwrap_or("").trim().trim_matches('\'').trim().to_string();

            match keyword.as_str() {
                "BITPIX" => tags.push(mktag("FITS", "BitDepth", "Bit Depth", Value::String(value))),
                "NAXIS" => tags.push(mktag("FITS", "NumAxes", "Number of Axes", Value::String(value))),
                "NAXIS1" => tags.push(mktag("FITS", "ImageWidth", "Image Width", Value::String(value))),
                "NAXIS2" => tags.push(mktag("FITS", "ImageHeight", "Image Height", Value::String(value))),
                "OBJECT" => tags.push(mktag("FITS", "Object", "Object", Value::String(value))),
                "TELESCOP" => tags.push(mktag("FITS", "Telescope", "Telescope", Value::String(value))),
                "INSTRUME" => tags.push(mktag("FITS", "Instrument", "Instrument", Value::String(value))),
                "DATE-OBS" => tags.push(mktag("FITS", "DateObs", "Date Observed", Value::String(value))),
                "OBSERVER" => tags.push(mktag("FITS", "Observer", "Observer", Value::String(value))),
                "EXPTIME" => tags.push(mktag("FITS", "ExposureTime", "Exposure Time", Value::String(value))),
                _ => {}
            }
        }
    }

    Ok(tags)
}

// ============================================================================
// FLV (Flash Video)
// ============================================================================

pub fn read_flv(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 9 || !data.starts_with(b"FLV\x01") {
        return Err(Error::InvalidData("not an FLV file".into()));
    }

    let mut tags = Vec::new();
    let flags = data[4];
    let has_audio = flags & 0x04 != 0;
    let has_video = flags & 0x01 != 0;

    tags.push(mktag("FLV", "HasAudio", "Has Audio", Value::String(if has_audio { "Yes" } else { "No" }.into())));
    tags.push(mktag("FLV", "HasVideo", "Has Video", Value::String(if has_video { "Yes" } else { "No" }.into())));

    Ok(tags)
}

// ============================================================================
// SWF (Shockwave Flash)
// ============================================================================

pub fn read_swf(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 8 {
        return Err(Error::InvalidData("not a SWF file".into()));
    }

    let compressed = match data[0] {
        b'F' => false,
        b'C' => true, // zlib compressed
        b'Z' => true, // LZMA compressed
        _ => return Err(Error::InvalidData("not a SWF file".into())),
    };

    if data[1] != b'W' || data[2] != b'S' {
        return Err(Error::InvalidData("not a SWF file".into()));
    }

    let mut tags = Vec::new();
    let version = data[3];
    let _file_length = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    tags.push(mktag("SWF", "FlashVersion", "Flash Version", Value::U8(version)));
    tags.push(mktag("SWF", "Compressed", "Compressed",
        Value::String(if compressed { "False" } else { "False" }.into())));

    // Parse SWF body (starting at byte 8)
    // For uncompressed SWF: body starts at data[8]
    // For compressed: would need to decompress; we skip compression for now
    if !compressed && data.len() > 8 {
        parse_swf_body(&data[8..], &mut tags);
    }

    Ok(tags)
}

/// Parse the uncompressed SWF body starting after the 8-byte file header.
/// The body starts with a RECT structure (image dimensions), followed by
/// FrameRate (u16 LE, fixed 8.8) and FrameCount (u16 LE), then SWF tags.
fn parse_swf_body(body: &[u8], tags: &mut Vec<Tag>) {
    if body.is_empty() { return; }

    // RECT structure: first 5 bits = nBits (number of bits for each coordinate)
    // Then 4 values each nBits long: Xmin, Xmax, Ymin, Ymax (in twips, 1/20 pixel)
    let n_bits = (body[0] >> 3) as usize;
    let total_bits = 5 + n_bits * 4;
    let n_bytes = (total_bits + 7) / 8;

    if body.len() < n_bytes + 4 { return; }

    // Extract the bit-packed values
    // Read bit string
    let mut bit_str = 0u64;
    let bytes_to_read = n_bytes.min(8);
    for i in 0..bytes_to_read {
        bit_str = (bit_str << 8) | body[i] as u64;
    }
    // Shift to align: the first 5 bits are nBits, then we have 4 * nBits values
    let total_64 = bytes_to_read * 8;
    let shift = total_64.saturating_sub(total_bits);
    bit_str >>= shift;

    // Extract values (from LSB side after the shift)
    let mask = if n_bits >= 64 { u64::MAX } else { (1u64 << n_bits) - 1 };
    let ymax_raw = (bit_str & mask) as i32;
    let ymin_raw = ((bit_str >> n_bits) & mask) as i32;
    let xmax_raw = ((bit_str >> (n_bits * 2)) & mask) as i32;
    let xmin_raw = ((bit_str >> (n_bits * 3)) & mask) as i32;

    // Sign-extend if the high bit is set
    let sign_extend = |v: i32, bits: usize| -> i32 {
        if bits > 0 && bits < 32 && (v & (1 << (bits - 1))) != 0 {
            v | (!0i32 << bits)
        } else { v }
    };
    let xmin = sign_extend(xmin_raw, n_bits);
    let xmax = sign_extend(xmax_raw, n_bits);
    let ymin = sign_extend(ymin_raw, n_bits);
    let ymax = sign_extend(ymax_raw, n_bits);

    // Convert from twips to pixels (1/20 pixel per twip)
    let width = ((xmax - xmin) as f64) / 20.0;
    let height = ((ymax - ymin) as f64) / 20.0;

    if width >= 0.0 && height >= 0.0 {
        tags.push(mktag("SWF", "ImageWidth", "Image Width", Value::F64(width)));
        tags.push(mktag("SWF", "ImageHeight", "Image Height", Value::F64(height)));
    }

    // Frame rate (fixed point 8.8 little-endian) and frame count
    let fr_offset = n_bytes;
    if fr_offset + 4 > body.len() { return; }
    let frame_rate_raw = u16::from_le_bytes([body[fr_offset], body[fr_offset + 1]]);
    let frame_count = u16::from_le_bytes([body[fr_offset + 2], body[fr_offset + 3]]);
    let frame_rate = frame_rate_raw as f64 / 256.0;

    tags.push(mktag("SWF", "FrameRate", "Frame Rate", Value::F64(frame_rate)));
    tags.push(mktag("SWF", "FrameCount", "Frame Count", Value::U16(frame_count)));

    if frame_rate > 0.0 && frame_count > 0 {
        let duration = frame_count as f64 / frame_rate;
        tags.push(mktag("SWF", "Duration", "Duration",
            Value::String(format!("{:.2} s", duration))));
    }

    // Scan SWF tags for metadata (tag 77 = Metadata/XMP)
    let mut tag_pos = fr_offset + 4;
    let mut found_attributes = false;
    while tag_pos + 2 <= body.len() {
        let code = u16::from_le_bytes([body[tag_pos], body[tag_pos + 1]]);
        let tag_type = (code >> 6) as u16;
        let short_len = (code & 0x3F) as usize;
        tag_pos += 2;

        let tag_len = if short_len == 0x3F {
            if tag_pos + 4 > body.len() { break; }
            let l = u32::from_le_bytes([body[tag_pos], body[tag_pos+1], body[tag_pos+2], body[tag_pos+3]]) as usize;
            tag_pos += 4;
            l
        } else {
            short_len
        };

        if tag_pos + tag_len > body.len() { break; }

        match tag_type {
            69 => {
                // FileAttributes - check HasMetadata flag
                if tag_len >= 1 {
                    let flags = body[tag_pos];
                    found_attributes = true;
                    if flags & 0x10 == 0 { break; } // No metadata
                }
            }
            77 => {
                // Metadata tag (XMP)
                let xmp_data = &body[tag_pos..tag_pos + tag_len];
                // Parse XMP to extract Author and other tags
                if let Ok(xmp_tags) = crate::metadata::XmpReader::read(xmp_data) {
                    for t in xmp_tags {
                        // Only add tags not already present
                        if !tags.iter().any(|e| e.name == t.name) {
                            tags.push(t);
                        }
                    }
                }
                // Also store raw XMP
                tags.push(mktag("SWF", "XMPToolkit", "XMP Toolkit",
                    Value::String(extract_xmp_toolkit(xmp_data))));
                break;
            }
            _ => {}
        }

        tag_pos += tag_len;
    }
    let _ = found_attributes;
}

fn extract_xmp_toolkit(xmp: &[u8]) -> String {
    let text = String::from_utf8_lossy(xmp);
    // Look for xmp:CreatorTool or xmptk attribute
    if let Some(start) = text.find("xmptk=\"") {
        let after = &text[start + 7..];
        if let Some(end) = after.find('"') {
            return after[..end].to_string();
        }
    }
    if let Some(start) = text.find("<xmp:CreatorTool>") {
        let after = &text[start + 17..];
        if let Some(end) = after.find("</") {
            return after[..end].to_string();
        }
    }
    String::new()
}

// ============================================================================
// Radiance HDR
// ============================================================================

pub fn read_hdr(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 10 || (!data.starts_with(b"#?RADIANCE") && !data.starts_with(b"#?RGBE")) {
        return Err(Error::InvalidData("not a Radiance HDR file".into()));
    }

    let mut tags = Vec::new();
    let text = String::from_utf8_lossy(&data[..data.len().min(8192)]);

    // Track key-value pairs and commands (last wins for non-list tags)
    let mut kv_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut last_command: Option<String> = None;
    let mut found_dims = false;

    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        // Skip the magic header line
        if line.starts_with("#?") { continue; }
        // Comment lines
        if line.starts_with('#') { continue; }
        // Empty line marks end of header metadata
        if line.is_empty() { continue; }
        // Dimension line (resolution) - last header line before data
        if line.starts_with("-Y ") || line.starts_with("+Y ") || line.starts_with("-X ") || line.starts_with("+X ") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                // Format: -Y <h> +X <w> or similar
                let axis1 = parts[0]; // e.g. "-Y"
                let axis3 = parts[2]; // e.g. "+X"
                let orient = format!("{} {}", axis1, axis3);
                // Map orientation
                let orient_name = match orient.as_str() {
                    "-Y +X" => "Horizontal (normal)",
                    "-Y -X" => "Mirror horizontal",
                    "+Y -X" => "Rotate 180",
                    "+Y +X" => "Mirror vertical",
                    "+X -Y" => "Mirror horizontal and rotate 270 CW",
                    "+X +Y" => "Rotate 90 CW",
                    "-X +Y" => "Mirror horizontal and rotate 90 CW",
                    "-X -Y" => "Rotate 270 CW",
                    _ => &orient,
                };
                kv_map.insert("_orient".to_string(), orient_name.to_string());
                if let Ok(dim1) = parts[1].parse::<u32>() {
                    // first axis is Y (height)
                    if axis1 == "-Y" || axis1 == "+Y" {
                        kv_map.insert("ImageHeight".to_string(), dim1.to_string());
                    } else {
                        kv_map.insert("ImageWidth".to_string(), dim1.to_string());
                    }
                }
                if let Ok(dim2) = parts[3].parse::<u32>() {
                    if axis3 == "-X" || axis3 == "+X" {
                        kv_map.insert("ImageWidth".to_string(), dim2.to_string());
                    } else {
                        kv_map.insert("ImageHeight".to_string(), dim2.to_string());
                    }
                }
            }
            found_dims = true;
            break;
        }
        // Check for key=value pairs
        if let Some(eq_pos) = line.find('=') {
            let key = line[..eq_pos].trim().to_lowercase();
            let val = line[eq_pos+1..].trim().to_string();
            // Map known keys
            let mapped_key = match key.as_str() {
                "software" => "Software",
                "view" => "View",
                "format" => "Format",
                "exposure" => "Exposure",
                "gamma" => "Gamma",
                "colorcorr" => "ColorCorrection",
                "pixaspect" => "PixelAspectRatio",
                "primaries" => "ColorPrimaries",
                _ => "",
            };
            if !mapped_key.is_empty() {
                kv_map.insert(mapped_key.to_string(), val);
            }
        } else {
            // Not a key=value, not a comment, not empty, not dimension: it's a command
            last_command = Some(line.to_string());
        }
    }

    // Emit tags in a consistent order (matching Perl output order)
    if let Some(cmd) = last_command {
        tags.push(mktag("HDR", "Command", "Command", Value::String(cmd)));
    }
    if let Some(v) = kv_map.get("Exposure") {
        tags.push(mktag("HDR", "Exposure", "Exposure", Value::String(v.clone())));
    }
    if let Some(v) = kv_map.get("Format") {
        tags.push(mktag("HDR", "Format", "Format", Value::String(v.clone())));
    }
    if let Some(h) = kv_map.get("ImageHeight") {
        if let Ok(hv) = h.parse::<u32>() {
            tags.push(mktag("HDR", "ImageHeight", "Image Height", Value::U32(hv)));
        }
    }
    if let Some(w) = kv_map.get("ImageWidth") {
        if let Ok(wv) = w.parse::<u32>() {
            tags.push(mktag("HDR", "ImageWidth", "Image Width", Value::U32(wv)));
        }
    }
    if let Some(v) = kv_map.get("_orient") {
        tags.push(mktag("HDR", "Orientation", "Orientation", Value::String(v.clone())));
    }
    if let Some(v) = kv_map.get("Software") {
        tags.push(mktag("HDR", "Software", "Software", Value::String(v.clone())));
    }
    if let Some(v) = kv_map.get("View") {
        tags.push(mktag("HDR", "View", "View", Value::String(v.clone())));
    }

    let _ = found_dims;
    Ok(tags)
}

// ============================================================================
// PPM/PGM/PBM (Netpbm formats)
// ============================================================================

pub fn read_ppm(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 3 || data[0] != b'P' {
        return Err(Error::InvalidData("not a PBM/PGM/PPM file".into()));
    }

    let type_byte = data[1];
    let is_pfm = type_byte == b'F' || type_byte == b'f';

    let mut tags = Vec::new();

    if is_pfm {
        // PFM format: PF\n<width> <height>\n<scale>\n<data>
        // ColorSpace: PF=RGB, Pf=Monochrome
        // ByteOrder: positive scale=Big-endian, negative=Little-endian
        let text = String::from_utf8_lossy(&data[..data.len().min(256)]);
        // Match: P[Ff]\n<width> <height>\n<scale>\n
        let re_str = text.as_ref();
        // Simple line-based parser
        let mut lines = re_str.lines();
        let header_line = lines.next().unwrap_or("");
        let cs_char = if header_line.ends_with('F') || header_line == "PF" { b'F' } else { b'f' };
        let color_space = if cs_char == b'F' { "RGB" } else { "Monochrome" };
        tags.push(mktag("PFM", "ColorSpace", "Color Space", Value::String(color_space.into())));

        // Width Height line
        if let Some(wh_line) = lines.next() {
            let parts: Vec<&str> = wh_line.split_whitespace().collect();
            if parts.len() >= 2 {
                if let (Ok(w), Ok(h)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) {
                    tags.push(mktag("PFM", "ImageWidth", "Image Width", Value::U32(w)));
                    tags.push(mktag("PFM", "ImageHeight", "Image Height", Value::U32(h)));
                }
            }
        }
        // Scale factor line
        if let Some(scale_line) = lines.next() {
            let scale_str = scale_line.trim();
            if let Ok(scale) = scale_str.parse::<f64>() {
                let byte_order = if scale > 0.0 { "Big-endian" } else { "Little-endian" };
                tags.push(mktag("PFM", "ByteOrder", "Byte Order", Value::String(byte_order.into())));
            }
        }
    } else {
        // PPM/PGM/PBM format
        // Parse header: collect comments, then width height [maxval]
        let text = String::from_utf8_lossy(&data[2..data.len().min(1024)]);
        let mut comment_lines: Vec<String> = Vec::new();
        let mut width: Option<u32> = None;
        let mut height: Option<u32> = None;
        let mut maxval: Option<u32> = None;
        let mut found_dims = false;

        // State machine: after magic byte, collect comments and parse dimensions
        let mut remaining = text.as_ref();
        // Skip initial whitespace
        remaining = remaining.trim_start();

        while !remaining.is_empty() {
            if remaining.starts_with('#') {
                // Comment line
                let end = remaining.find('\n').unwrap_or(remaining.len());
                let comment = &remaining[1..end];
                // Remove leading space after '#'
                let comment = comment.strip_prefix(' ').unwrap_or(comment);
                comment_lines.push(comment.to_string());
                remaining = &remaining[end..];
                remaining = remaining.trim_start_matches('\n').trim_start_matches('\r');
            } else if !found_dims {
                // Parse width height
                let parts: Vec<&str> = remaining.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let (Ok(w), Ok(h)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) {
                        width = Some(w);
                        height = Some(h);
                        found_dims = true;
                        // Advance past width and height
                        let skip1 = remaining.find(parts[0]).unwrap_or(0) + parts[0].len();
                        remaining = &remaining[skip1..];
                        remaining = remaining.trim_start();
                        let skip2 = remaining.find(parts[1]).unwrap_or(0) + parts[1].len();
                        remaining = &remaining[skip2..];
                        remaining = remaining.trim_start();
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            } else {
                // Check for comment before maxval
                if remaining.starts_with('#') {
                    let end = remaining.find('\n').unwrap_or(remaining.len());
                    let comment = &remaining[1..end];
                    let comment = comment.strip_prefix(' ').unwrap_or(comment);
                    comment_lines.push(comment.to_string());
                    remaining = &remaining[end..];
                    remaining = remaining.trim_start_matches('\n').trim_start_matches('\r');
                    continue;
                }
                // Parse maxval (for non-PBM types)
                let is_pbm = type_byte == b'1' || type_byte == b'4';
                if !is_pbm {
                    let parts: Vec<&str> = remaining.splitn(2, char::is_whitespace).collect();
                    if let Some(v) = parts.first() {
                        if let Ok(mv) = v.parse::<u32>() {
                            maxval = Some(mv);
                        }
                    }
                }
                break;
            }
        }

        // Comment: join lines and trim trailing newline
        if !comment_lines.is_empty() {
            let comment = comment_lines.join("\n");
            let comment = comment.trim_end_matches('\n').trim_end_matches('\r').to_string();
            tags.push(mktag("PPM", "Comment", "Comment", Value::String(comment)));
        }

        if let Some(w) = width {
            tags.push(mktag("PPM", "ImageWidth", "Image Width", Value::U32(w)));
        }
        if let Some(h) = height {
            tags.push(mktag("PPM", "ImageHeight", "Image Height", Value::U32(h)));
        }
        if let Some(mv) = maxval {
            tags.push(mktag("PPM", "MaxVal", "Max Val", Value::U32(mv)));
        }
    }

    Ok(tags)
}

// ============================================================================
// PCX (ZSoft PC Paintbrush)
// ============================================================================

pub fn read_pcx(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 128 || data[0] != 0x0A {
        return Err(Error::InvalidData("not a PCX file".into()));
    }

    let mut tags = Vec::new();
    let manufacturer = data[0x00];
    let software_ver = data[0x01];
    let encoding = data[0x02];
    let bpp = data[0x03];
    let xmin = u16::from_le_bytes([data[0x04], data[0x05]]);
    let ymin = u16::from_le_bytes([data[0x06], data[0x07]]);
    let xmax = u16::from_le_bytes([data[0x08], data[0x09]]);
    let ymax = u16::from_le_bytes([data[0x0a], data[0x0b]]);
    let hdpi = u16::from_le_bytes([data[0x0c], data[0x0d]]);
    let vdpi = u16::from_le_bytes([data[0x0e], data[0x0f]]);
    let num_planes = data[0x41];
    let bytes_per_line = u16::from_le_bytes([data[0x42], data[0x43]]);
    let color_mode = u16::from_le_bytes([data[0x44], data[0x45]]);

    let mfr_str = match manufacturer {
        10 => "ZSoft",
        _ => "Unknown",
    };
    tags.push(mktag("PCX", "Manufacturer", "Manufacturer", Value::String(mfr_str.into())));

    let sw_str = match software_ver {
        0 => "PC Paintbrush 2.5",
        2 => "PC Paintbrush 2.8 (with palette)",
        3 => "PC Paintbrush 2.8 (without palette)",
        4 => "PC Paintbrush for Windows",
        5 => "PC Paintbrush 3.0+",
        _ => "Unknown",
    };
    tags.push(mktag("PCX", "Software", "Software", Value::String(sw_str.into())));

    let enc_str = match encoding {
        1 => "RLE",
        _ => "Unknown",
    };
    tags.push(mktag("PCX", "Encoding", "Encoding", Value::String(enc_str.into())));

    tags.push(mktag("PCX", "BitsPerPixel", "Bits Per Pixel", Value::U8(bpp)));
    tags.push(mktag("PCX", "LeftMargin", "Left Margin", Value::U16(xmin)));
    tags.push(mktag("PCX", "TopMargin", "Top Margin", Value::U16(ymin)));
    tags.push(mktag("PCX", "ImageWidth", "Image Width", Value::U16(xmax - xmin + 1)));
    tags.push(mktag("PCX", "ImageHeight", "Image Height", Value::U16(ymax - ymin + 1)));
    tags.push(mktag("PCX", "XResolution", "X Resolution", Value::U16(hdpi)));
    tags.push(mktag("PCX", "YResolution", "Y Resolution", Value::U16(vdpi)));
    tags.push(mktag("PCX", "ColorPlanes", "Color Planes", Value::U8(num_planes)));
    tags.push(mktag("PCX", "BytesPerLine", "Bytes Per Line", Value::U16(bytes_per_line)));

    let cm_str = match color_mode {
        0 => "n/a",
        1 => "Color Palette",
        2 => "Grayscale",
        _ => "Unknown",
    };
    tags.push(mktag("PCX", "ColorMode", "Color Mode", Value::String(cm_str.into())));

    Ok(tags)
}

// ============================================================================
// DjVu
// ============================================================================

pub fn read_djvu(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 16 || !data.starts_with(b"AT&TFORM") {
        return Err(Error::InvalidData("not a DjVu file".into()));
    }

    let mut tags = Vec::new();
    let form_type = &data[12..16];

    let doc_type = match form_type {
        b"DJVU" => "DjVu Single-Page",
        b"DJVM" => "DjVu Multi-Page",
        b"PM44" | b"BM44" => "DjVu Photo/Bitmap",
        _ => "DjVu",
    };
    tags.push(mktag("DjVu", "DocumentType", "Document Type", Value::String(doc_type.into())));

    // Parse INFO chunk for dimensions
    let mut pos = 16;
    while pos + 8 <= data.len() {
        let chunk_id = &data[pos..pos + 4];
        let chunk_size = u32::from_be_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]) as usize;
        pos += 8;

        if chunk_id == b"INFO" && chunk_size >= 10 && pos + 10 <= data.len() {
            let width = u16::from_be_bytes([data[pos], data[pos + 1]]);
            let height = u16::from_be_bytes([data[pos + 2], data[pos + 3]]);
            let dpi = u16::from_le_bytes([data[pos + 6], data[pos + 7]]);

            tags.push(mktag("DjVu", "ImageWidth", "Image Width", Value::U16(width)));
            tags.push(mktag("DjVu", "ImageHeight", "Image Height", Value::U16(height)));
            if dpi > 0 {
                tags.push(mktag("DjVu", "Resolution", "Resolution", Value::U16(dpi)));
            }
            break;
        }

        pos += chunk_size;
        if chunk_size % 2 != 0 { pos += 1; }
    }

    Ok(tags)
}

// ============================================================================
// FLIF (Free Lossless Image Format)
// ============================================================================

pub fn read_flif(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 6 || !data.starts_with(b"FLIF") {
        return Err(Error::InvalidData("not a FLIF file".into()));
    }

    let mut tags = Vec::new();
    let byte4 = data[4];
    let interlaced = (byte4 >> 4) & 1;
    let num_channels = (byte4 & 0x0F) + 1;
    let _bpc = data[5];

    tags.push(mktag("FLIF", "Interlaced", "Interlaced", Value::String(if interlaced != 0 { "Yes" } else { "No" }.into())));
    tags.push(mktag("FLIF", "NumChannels", "Channels", Value::U8(num_channels)));

    // Width and height are varint encoded starting at offset 6
    let mut pos = 6;
    if let Some((w, consumed)) = read_flif_varint(data, pos) {
        tags.push(mktag("FLIF", "ImageWidth", "Image Width", Value::U32((w + 1) as u32)));
        pos += consumed;
        if let Some((h, _)) = read_flif_varint(data, pos) {
            tags.push(mktag("FLIF", "ImageHeight", "Image Height", Value::U32((h + 1) as u32)));
        }
    }

    Ok(tags)
}

fn read_flif_varint(data: &[u8], mut pos: usize) -> Option<(u64, usize)> {
    let start = pos;
    let mut result = 0u64;
    loop {
        if pos >= data.len() { return None; }
        let byte = data[pos];
        result = (result << 7) | (byte & 0x7F) as u64;
        pos += 1;
        if byte & 0x80 == 0 { break; }
        if pos - start > 8 { return None; }
    }
    Some((result, pos - start))
}

// ============================================================================
// BPG (Better Portable Graphics)
// ============================================================================

pub fn read_bpg(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 6 || !data.starts_with(&[0x42, 0x50, 0x47, 0xFB]) {
        return Err(Error::InvalidData("not a BPG file".into()));
    }

    let mut tags = Vec::new();
    let byte4 = data[4];
    let pixel_format = (byte4 >> 5) & 0x07;
    let has_alpha = (byte4 >> 4) & 1;
    let bit_depth_m8 = byte4 & 0x0F;

    let pf_name = match pixel_format {
        0 => "YCbCr 4:2:0",
        1 => "YCbCr 4:2:2",
        2 => "YCbCr 4:4:4",
        3 => "Grayscale",
        4 => "YCbCr 4:2:0 + Alpha",
        5 => "YCbCr 4:2:2 + Alpha",
        _ => "Unknown",
    };
    tags.push(mktag("BPG", "PixelFormat", "Pixel Format", Value::String(pf_name.into())));
    tags.push(mktag("BPG", "HasAlpha", "Has Alpha", Value::String(if has_alpha != 0 { "Yes" } else { "No" }.into())));
    tags.push(mktag("BPG", "BitDepth", "Bit Depth", Value::U8(bit_depth_m8 + 8)));

    // Width and height are exp-golomb coded starting at offset 5/6
    // Simplified: read as varints
    let mut pos = 5;
    if let Some((w, consumed)) = read_bpg_ue(data, pos) {
        tags.push(mktag("BPG", "ImageWidth", "Image Width", Value::U32(w as u32)));
        pos += consumed;
        if let Some((h, _)) = read_bpg_ue(data, pos) {
            tags.push(mktag("BPG", "ImageHeight", "Image Height", Value::U32(h as u32)));
        }
    }

    Ok(tags)
}

fn read_bpg_ue(data: &[u8], mut pos: usize) -> Option<(u64, usize)> {
    // Exponential-Golomb / BPG uses a simple varint: MSB continuation
    let start = pos;
    let mut result = 0u64;
    loop {
        if pos >= data.len() { return None; }
        let byte = data[pos];
        result = (result << 7) | (byte & 0x7F) as u64;
        pos += 1;
        if byte & 0x80 == 0 { break; }
        if pos - start > 8 { return None; }
    }
    Some((result, pos - start))
}

// ============================================================================
// PICT (Apple QuickDraw Picture)
// ============================================================================

pub fn read_pict(data: &[u8]) -> Result<Vec<Tag>> {
    // PICT files have a 512-byte header (usually zeros) then the picture data
    let offset = if data.len() > 522 && data[..512].iter().all(|&b| b == 0) {
        512
    } else {
        0
    };

    if offset + 10 > data.len() {
        return Err(Error::InvalidData("not a PICT file".into()));
    }

    let mut tags = Vec::new();
    let d = &data[offset..];

    // Size (2 bytes) + bounding rect (8 bytes: top, left, bottom, right)
    let top = i16::from_be_bytes([d[2], d[3]]);
    let left = i16::from_be_bytes([d[4], d[5]]);
    let bottom = i16::from_be_bytes([d[6], d[7]]);
    let right = i16::from_be_bytes([d[8], d[9]]);

    // Check PICT version at byte 10
    // Version 2 opcode: 0x0011 at bytes 10-11
    let mut h_res: Option<f64> = None;
    let mut v_res: Option<f64> = None;
    let mut w = (right - left) as i32;
    let mut h = (bottom - top) as i32;

    if d.len() >= 40 && d[10] == 0x00 && d[11] == 0x11 {
        // Version 2: next 2 bytes are 0x02ff, then check for extended
        // d[12..14] = 0x02ff, d[14..16] = 0x0c00
        // d[16..18]: 0xffff = normal, 0xfffe = extended
        if d.len() >= 18 && d[12] == 0x02 && d[13] == 0xff {
            if d[16] == 0xff && d[17] == 0xfe && d.len() >= 36 {
                // Extended version 2: resolution at offsets 24..28 and 28..32 (x8 skip from byte 16)
                // From Perl: unpack('x8N2', $buff) where buff starts at byte after 0x0011 opcode
                // $buff was read starting at position 12 (after 12-byte first read)
                // x8 skips bytes 12..20, N2 reads bytes 20..24 and 24..28 in original data
                // Actually the 28 bytes buff starts after the 12-byte header
                // In d: after opcode 0x0011 at d[10..12], read 28 bytes: d[12..40]
                // x8 skip => skip d[12..20], N2 => d[20..24] and d[24..28]
                let h_fixed = i32::from_be_bytes([d[20], d[21], d[22], d[23]]);
                let v_fixed = i32::from_be_bytes([d[24], d[25], d[26], d[27]]);
                if h_fixed != 0 && v_fixed != 0 {
                    h_res = Some(h_fixed as f64 / 65536.0);
                    v_res = Some(v_fixed as f64 / 65536.0);
                    // Scale dimensions from 72-dpi equivalent
                    w = (w as f64 * h_res.unwrap() / 72.0 + 0.5) as i32;
                    h = (h as f64 * v_res.unwrap() / 72.0 + 0.5) as i32;
                }
            }
        }
    }

    tags.push(mktag("PICT", "ImageWidth", "Image Width", Value::I32(w)));
    tags.push(mktag("PICT", "ImageHeight", "Image Height", Value::I32(h)));
    if let Some(hr) = h_res {
        tags.push(mktag("PICT", "XResolution", "X Resolution", Value::String(format!("{}", hr as i64))));
    }
    if let Some(vr) = v_res {
        tags.push(mktag("PICT", "YResolution", "Y Resolution", Value::String(format!("{}", vr as i64))));
    }

    Ok(tags)
}

// ============================================================================
// M2TS (MPEG-2 Transport Stream)
// ============================================================================

pub fn read_m2ts(data: &[u8]) -> Result<Vec<Tag>> {
    if data.is_empty() {
        return Err(Error::InvalidData("empty file".into()));
    }

    let mut tags = Vec::new();

    // Find sync byte pattern (0x47 every 188 or 192 bytes)
    let packet_size = if data.len() >= 376 && data[0] == 0x47 && data[188] == 0x47 {
        188
    } else if data.len() >= 384 && data[0] == 0x47 && data[192] == 0x47 {
        192 // M2TS with 4-byte timestamp prefix
    } else if data.len() >= 8 && data[4] == 0x47 && data.len() >= 196 && data[196] == 0x47 {
        192
    } else {
        return Err(Error::InvalidData("not an MPEG-2 TS file".into()));
    };

    tags.push(mktag("M2TS", "TSPacketSize", "TS Packet Size", Value::U32(packet_size as u32)));
    let num_packets = data.len() / packet_size;
    tags.push(mktag("M2TS", "TSPacketCount", "TS Packet Count", Value::U32(num_packets as u32)));

    Ok(tags)
}

// ============================================================================
// GZIP
// ============================================================================

pub fn read_gzip(data: &[u8]) -> Result<Vec<Tag>> {
    // RFC 1952: magic=1F 8B, method=08 (deflate)
    if data.len() < 10 || data[0] != 0x1F || data[1] != 0x8B || data[2] != 0x08 {
        return Err(Error::InvalidData("not a GZIP file".into()));
    }

    let mut tags = Vec::new();
    let method = data[2];
    let flags = data[3];
    let xflags = data[8];
    let os_byte = data[9];

    // Compression (byte 2)
    let compress_str = if method == 8 { "Deflated" } else { "Unknown" };
    tags.push(mktag("GZIP", "Compression", "Compression", Value::String(compress_str.into())));

    // Flags (byte 3) — bitmask
    let flag_names = [(0, "Text"), (1, "CRC16"), (2, "ExtraFields"), (3, "FileName"), (4, "Comment")];
    let mut flag_parts: Vec<&str> = Vec::new();
    for (bit, name) in &flag_names {
        if flags & (1 << bit) != 0 {
            flag_parts.push(name);
        }
    }
    let flags_str = if flag_parts.is_empty() {
        "(none)".to_string()
    } else {
        flag_parts.join(", ")
    };
    tags.push(mktag("GZIP", "Flags", "Flags", Value::String(flags_str)));

    // ModifyDate (bytes 4-7, Unix timestamp, local time)
    let mtime = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    if mtime > 0 {
        let dt = gzip_unix_to_datetime(mtime as i64);
        tags.push(mktag("GZIP", "ModifyDate", "Modify Date", Value::String(dt)));
    }

    // ExtraFlags (byte 8)
    let extra_flags_str = match xflags {
        0 => "(none)".to_string(),
        2 => "Maximum Compression".to_string(),
        4 => "Fastest Algorithm".to_string(),
        _ => format!("{}", xflags),
    };
    tags.push(mktag("GZIP", "ExtraFlags", "Extra Flags", Value::String(extra_flags_str)));

    // OperatingSystem (byte 9)
    let os_str = match os_byte {
        0 => "FAT filesystem (MS-DOS, OS/2, NT/Win32)",
        1 => "Amiga",
        2 => "VMS (or OpenVMS)",
        3 => "Unix",
        4 => "VM/CMS",
        5 => "Atari TOS",
        6 => "HPFS filesystem (OS/2, NT)",
        7 => "Macintosh",
        8 => "Z-System",
        9 => "CP/M",
        10 => "TOPS-20",
        11 => "NTFS filesystem (NT)",
        12 => "QDOS",
        13 => "Acorn RISCOS",
        255 => "unknown",
        _ => "Other",
    };
    tags.push(mktag("GZIP", "OperatingSystem", "Operating System", Value::String(os_str.into())));

    // Extract file name and comment if flag bits set
    let mut pos = 10usize;
    if flags & 0x18 != 0 {
        // Skip FEXTRA (bit 2) if present
        if flags & 0x04 != 0 && pos + 2 <= data.len() {
            let xlen = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
            pos += 2 + xlen;
        }

        // ArchivedFileName (bit 3)
        if flags & 0x08 != 0 && pos < data.len() {
            let name_end = data[pos..].iter().position(|&b| b == 0)
                .unwrap_or(data.len() - pos);
            let filename = String::from_utf8_lossy(&data[pos..pos + name_end]).to_string();
            if !filename.is_empty() {
                tags.push(mktag("GZIP", "ArchivedFileName", "Archived File Name", Value::String(filename)));
            }
            pos += name_end + 1;
        }

        // Comment (bit 4)
        if flags & 0x10 != 0 && pos < data.len() {
            let comment_end = data[pos..].iter().position(|&b| b == 0)
                .unwrap_or(data.len() - pos);
            let comment = String::from_utf8_lossy(&data[pos..pos + comment_end]).to_string();
            if !comment.is_empty() {
                tags.push(mktag("GZIP", "Comment", "Comment", Value::String(comment)));
            }
        }
    } else {
        // No FEXTRA flag, but FNAME might still be set
        if flags & 0x04 != 0 && pos + 2 <= data.len() {
            let xlen = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
            pos += 2 + xlen;
        }
        if flags & 0x08 != 0 && pos < data.len() {
            let name_end = data[pos..].iter().position(|&b| b == 0)
                .unwrap_or(data.len() - pos);
            let filename = String::from_utf8_lossy(&data[pos..pos + name_end]).to_string();
            if !filename.is_empty() {
                tags.push(mktag("GZIP", "ArchivedFileName", "Archived File Name", Value::String(filename)));
            }
        }
    }

    Ok(tags)
}

/// Convert Unix timestamp to "YYYY:MM:DD HH:MM:SS+HH:00" (local time).
/// Mirrors Perl's ConvertUnixTime($val, 1).
fn gzip_unix_to_datetime(secs: i64) -> String {
    // Get timezone offset from system
    let tz_offset = get_local_tz_offset_secs();
    let local_secs = secs + tz_offset;
    let days = local_secs / 86400;
    let time = local_secs % 86400;
    let (time, days) = if time < 0 { (time + 86400, days - 1) } else { (time, days) };
    let h = time / 3600;
    let m = (time % 3600) / 60;
    let s = time % 60;
    let mut y = 1970i32;
    let mut rem = days;
    loop {
        let dy: i64 = if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 { 366 } else { 365 };
        if rem < dy { break; }
        rem -= dy;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let months: [i64; 12] = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut mo = 1;
    for &dm in &months {
        if rem < dm { break; }
        rem -= dm;
        mo += 1;
    }
    let tz_h = tz_offset / 3600;
    let tz_sign = if tz_h >= 0 { "+" } else { "-" };
    format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}{}{:02}:00",
        y, mo, rem + 1, h, m, s, tz_sign, tz_h.abs())
}

/// Get local timezone offset in seconds using /proc or /etc/localtime.
fn get_local_tz_offset_secs() -> i64 {
    // Try to read timezone from /etc/timezone
    if let Ok(tz) = std::fs::read_to_string("/etc/timezone") {
        let tz = tz.trim();
        if tz == "UTC" || tz == "UTC0" { return 0; }
    }
    // Try /etc/localtime symlink
    if let Ok(link) = std::fs::read_link("/etc/localtime") {
        let path = link.to_string_lossy();
        // Known UTC zones
        if path.contains("UTC") || path.ends_with("/UTC") { return 0; }
        // CET zones: +1 hour (summer +2, but we use standard time)
        if path.contains("Europe/") || path.contains("/CET") { return 3600; }
        if path.contains("America/New_York") { return -5 * 3600; }
        if path.contains("America/Los_Angeles") { return -8 * 3600; }
        if path.contains("America/Chicago") { return -6 * 3600; }
        if path.contains("Asia/Tokyo") { return 9 * 3600; }
    }
    // Default: UTC
    0
}

// ============================================================================
// MacOS XAttr (._) sidecar file
// ============================================================================

/// Parse MacOS AppleDouble sidecar (._) files containing XAttr data.
/// Mirrors ExifTool's MacOS.pm ProcessMacOS and ProcessATTR.
pub fn read_macos(data: &[u8]) -> Result<Vec<Tag>> {
    // Check header: \0\x05\x16\x07\0\x02\0\0Mac OS X
    if data.len() < 26 || data[0] != 0x00 || data[1] != 0x05 || data[2] != 0x16 || data[3] != 0x07 {
        return Err(Error::InvalidData("not a MacOS sidecar file".into()));
    }
    let ver = data[5];
    if ver != 2 {
        return Ok(Vec::new());
    }

    let entries = u16::from_be_bytes([data[24], data[25]]) as usize;
    if 26 + entries * 12 > data.len() {
        return Ok(Vec::new());
    }

    let mut tags = Vec::new();

    for i in 0..entries {
        let pos = 26 + i * 12;
        let tag_id = u32::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
        let off = u32::from_be_bytes([data[pos+4], data[pos+5], data[pos+6], data[pos+7]]) as usize;
        let len = u32::from_be_bytes([data[pos+8], data[pos+9], data[pos+10], data[pos+11]]) as usize;

        if tag_id == 9 && off + len <= data.len() {
            // ATTR block
            let entry_data = &data[off..off + len];
            parse_attr_block(data, entry_data, &mut tags);
        }
    }

    Ok(tags)
}

/// Parse an ATTR (extended attributes) block from a MacOS sidecar file.
/// entry_data is the ATTR block, full_data is the entire file (for absolute offsets).
fn parse_attr_block(full_data: &[u8], entry_data: &[u8], tags: &mut Vec<Tag>) {
    if entry_data.len() < 70 {
        return;
    }
    // Check for ATTR signature at offset 34
    if &entry_data[34..38] != b"ATTR" {
        return;
    }

    let xattr_entries = u32::from_be_bytes([entry_data[66], entry_data[67], entry_data[68], entry_data[69]]) as usize;
    let mut pos = 70;

    for _i in 0..xattr_entries {
        if pos + 11 > entry_data.len() {
            break;
        }
        let off = u32::from_be_bytes([entry_data[pos], entry_data[pos+1], entry_data[pos+2], entry_data[pos+3]]) as usize;
        let len = u32::from_be_bytes([entry_data[pos+4], entry_data[pos+5], entry_data[pos+6], entry_data[pos+7]]) as usize;
        let n = entry_data[pos+10] as usize;

        if pos + 11 + n > entry_data.len() {
            break;
        }
        let name_bytes = &entry_data[pos+11..pos+11+n];
        let name = String::from_utf8_lossy(name_bytes).trim_end_matches('\0').to_string();

        // Offsets are absolute file offsets
        let val_data = if off + len <= full_data.len() {
            &full_data[off..off + len]
        } else {
            pos += ((11 + n + 3) & !3).max(1);
            continue;
        };

        // Convert xattr name to ExifTool tag name
        let tag_name = xattr_name_to_tag(&name);

        // Process value
        if val_data.starts_with(b"bplist0") {
            // Parse simple binary plist (arrays, strings, dates)
            if let Some(value) = parse_simple_bplist(val_data) {
                tags.push(mktag("MacOS", &tag_name, &tag_name, Value::String(value)));
            } else {
                // Just mark as binary
                tags.push(mktag("MacOS", &tag_name, &tag_name, Value::Binary(val_data.to_vec())));
            }
        } else if len > 100 || val_data.contains(&0u8) && !val_data.starts_with(b"0082") {
            // Binary data
            tags.push(mktag("MacOS", &tag_name, &tag_name, Value::Binary(val_data.to_vec())));
        } else {
            let s = String::from_utf8_lossy(val_data).trim_end_matches('\0').to_string();
            // Handle quarantine string: format "0082;TIME;APP;"
            let display = if name == "com.apple.quarantine" {
                format_quarantine(&s)
            } else {
                s
            };
            if !display.is_empty() {
                tags.push(mktag("MacOS", &tag_name, &tag_name, Value::String(display)));
            }
        }

        // Advance to next entry (aligned to 4 bytes)
        pos += ((11 + n + 3) & !3).max(4);
    }
}

/// Convert xattr attribute name to ExifTool tag name.
/// Mirrors Perl: com.apple.metadata:kMDItemXxx → XAttrMDItemXxx etc.
fn xattr_name_to_tag(name: &str) -> String {
    // Check known names first (from ExifTool's XAttr table)
    let known = match name {
        "com.apple.quarantine" => Some("XAttrQuarantine"),
        "com.apple.lastuseddate#PS" => Some("XAttrLastUsedDate"),
        "com.apple.metadata:kMDItemDownloadedDate" => Some("XAttrMDItemDownloadedDate"),
        "com.apple.metadata:kMDItemWhereFroms" => Some("XAttrMDItemWhereFroms"),
        "com.apple.metadata:kMDLabel" => Some("XAttrMDLabel"),
        "com.apple.metadata:kMDItemFinderComment" => Some("XAttrMDItemFinderComment"),
        "com.apple.metadata:_kMDItemUserTags" => Some("XAttrMDItemUserTags"),
        _ => None,
    };
    // For non-apple names: strip separators and capitalize words
    if name.starts_with("org.") || name.starts_with("net.") || (!name.starts_with("com.apple.") && name.contains(':')) {
        // Apply MakeTagName-style conversion
        let mut tag = String::from("XAttr");
        let mut cap_next = true;
        for c in name.chars() {
            if c == '.' || c == ':' || c == '_' || c == '-' {
                cap_next = true;
            } else if cap_next {
                for uc in c.to_uppercase() {
                    tag.push(uc);
                }
                cap_next = false;
            } else {
                tag.push(c);
            }
        }
        return tag;
    }
    if let Some(n) = known {
        return n.to_string();
    }

    // Remove random ID after kMDLabel_
    let name = if let Some(p) = name.find("kMDLabel_") {
        &name[..p + 8] // keep up to kMDLabel
    } else {
        name
    };

    // Apply Perl transformation
    let basename = if let Some(rest) = name.strip_prefix("com.apple.") {
        // s/^metadata:_?k//
        let rest = if let Some(r) = rest.strip_prefix("metadata:k") {
            r
        } else if let Some(r) = rest.strip_prefix("metadata:_k") {
            r
        } else if let Some(r) = rest.strip_prefix("metadata:") {
            r
        } else {
            rest
        };
        rest.to_string()
    } else {
        name.to_string()
    };

    // ucfirst then s/[.:_]([a-z])/\U$1/g
    let base_ucfirst = ucfirst_str_misc(&basename);
    let mut result = String::from("XAttr");

    let chars: Vec<char> = base_ucfirst.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if (c == '.' || c == ':' || c == '_' || c == '#') && i + 1 < chars.len() && chars[i+1].is_ascii_lowercase() {
            result.push(chars[i+1].to_ascii_uppercase());
            i += 2;
        } else if c == '.' || c == ':' || c == '_' || c == '#' {
            i += 1; // skip separator with no following lowercase
        } else {
            result.push(c);
            i += 1;
        }
    }
    result
}

fn ucfirst_str_misc(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

/// Format quarantine string to ExifTool format.
fn format_quarantine(s: &str) -> String {
    // Format: "FLAGS;HEX_TIME;APP;" or similar
    // ExifTool shows: "Flags=0082 set at 2020:11:12 12:27:26 by Safari"
    let parts: Vec<&str> = s.split(';').collect();
    if parts.len() >= 3 {
        let flags = parts[0];
        let time_hex = parts[1];
        let app = parts[2];

        // Try to parse time_hex as hex timestamp
        let time_str = if let Ok(ts) = i64::from_str_radix(time_hex, 16) {
            // Mac HFS+ time: seconds since 2001-01-01 or 1904-01-01
            // QuickTime epoch (2001) is used for Apple timestamps
            // Actually quarantine uses Unix epoch
            // Let's just show the raw value
            format!("(ts={})", ts)
        } else {
            time_hex.to_string()
        };

        if !app.is_empty() {
            return format!("Flags={} set at {} by {}", flags, time_str, app);
        }
        return format!("Flags={} set at {}", flags, time_str);
    }
    s.to_string()
}

/// Parse a simple binary plist to extract string, array of strings, or date values.
fn parse_simple_bplist(data: &[u8]) -> Option<String> {
    if data.len() < 32 || !data.starts_with(b"bplist00") {
        return None;
    }

    // Read trailer: last 32 bytes
    let trailer_start = data.len() - 32;
    let trailer = &data[trailer_start..];
    let offset_int_size = trailer[6] as usize;
    let obj_ref_size = trailer[7] as usize;
    let num_objects = u64::from_be_bytes([trailer[8], trailer[9], trailer[10], trailer[11],
                                          trailer[12], trailer[13], trailer[14], trailer[15]]) as usize;
    let top_object = u64::from_be_bytes([trailer[16], trailer[17], trailer[18], trailer[19],
                                         trailer[20], trailer[21], trailer[22], trailer[23]]) as usize;
    let offset_table_offset = u64::from_be_bytes([trailer[24], trailer[25], trailer[26], trailer[27],
                                                   trailer[28], trailer[29], trailer[30], trailer[31]]) as usize;

    if offset_int_size == 0 || offset_int_size > 8 || num_objects == 0 {
        return None;
    }

    let mut objects_offset = Vec::with_capacity(num_objects);
    for i in 0..num_objects {
        let ot_pos = offset_table_offset + i * offset_int_size;
        if ot_pos + offset_int_size > data.len() {
            return None;
        }
        let mut off: usize = 0;
        for j in 0..offset_int_size {
            off = (off << 8) | data[ot_pos + j] as usize;
        }
        objects_offset.push(off);
    }

    let read_object = |obj_idx: usize| -> Option<String> {
        let off = *objects_offset.get(obj_idx)?;
        if off >= data.len() {
            return None;
        }
        let marker = data[off];
        let type_nibble = (marker & 0xF0) >> 4;
        let info_nibble = marker & 0x0F;

        match type_nibble {
            0x5 => {
                // ASCII string
                let len = info_nibble as usize;
                if off + 1 + len > data.len() { return None; }
                Some(String::from_utf8_lossy(&data[off+1..off+1+len]).to_string())
            }
            0x6 => {
                // Unicode string (UTF-16BE)
                let len = info_nibble as usize;
                let byte_len = len * 2;
                if off + 1 + byte_len > data.len() { return None; }
                let chars: Vec<u16> = data[off+1..off+1+byte_len]
                    .chunks_exact(2)
                    .map(|c| u16::from_be_bytes([c[0], c[1]]))
                    .collect();
                String::from_utf16(&chars).ok()
            }
            0x3 => {
                // Date (64-bit float, seconds since 2001-01-01)
                if off + 9 > data.len() { return None; }
                let bits = u64::from_be_bytes([data[off+1], data[off+2], data[off+3], data[off+4],
                                               data[off+5], data[off+6], data[off+7], data[off+8]]);
                let secs = f64::from_bits(bits);
                // Convert from Apple epoch (2001-01-01) to Unix epoch (1970-01-01)
                let unix_secs = secs as i64 + 978307200;
                // Format as date string
                let days = unix_secs / 86400;
                let time = unix_secs % 86400;
                let hour = time / 3600;
                let min = (time % 3600) / 60;
                let sec = time % 60;
                let mut year = 1970i32;
                let mut rem_days = days;
                loop {
                    let dy = if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 { 366 } else { 365 };
                    if rem_days < dy { break; }
                    rem_days -= dy;
                    year += 1;
                }
                let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
                let month_days = [31i64, if leap {29} else {28}, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
                let mut month = 1i32;
                for &md in &month_days {
                    if rem_days < md { break; }
                    rem_days -= md;
                    month += 1;
                }
                let day = rem_days + 1;
                Some(format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}", year, month, day, hour, min, sec))
            }
            0xA => {
                // Array: collect items
                let count = if info_nibble == 0xF {
                    // extended length
                    if off + 2 > data.len() { return None; }
                    let ext_marker = data[off+1];
                    (1 << (ext_marker & 0xF)) as usize
                } else {
                    info_nibble as usize
                };
                Some(format!("({} items)", count))
            }
            _ => None,
        }
    };

    // Get top object
    let result = read_object(top_object)?;

    // If it's an array, try to read its elements
    if let Some(off) = objects_offset.get(top_object) {
        let off = *off;
        if off < data.len() {
            let marker = data[off];
            let type_nibble = (marker & 0xF0) >> 4;
            if type_nibble == 0xA {
                // Array: read elements
                let count = (marker & 0x0F) as usize;
                let mut items = Vec::new();
                for j in 0..count {
                    let ref_pos = off + 1 + j * obj_ref_size;
                    if ref_pos + obj_ref_size > data.len() { break; }
                    let mut obj_ref: usize = 0;
                    for k in 0..obj_ref_size {
                        obj_ref = (obj_ref << 8) | data[ref_pos + k] as usize;
                    }
                    if let Some(item_val) = read_object(obj_ref) {
                        items.push(item_val);
                    }
                }
                if !items.is_empty() {
                    return Some(items.join(", "));
                }
            }
        }
    }

    Some(result)
}

// ============================================================================
// MOI (camcorder info file)
// ============================================================================

/// Parse MOI (camcorder info) files. Mirrors ExifTool's MOI.pm.
pub fn read_moi(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 256 || !data.starts_with(b"V6") {
        return Err(Error::InvalidData("not a MOI file".into()));
    }

    let mut tags = Vec::new();

    // 0x00: MOIVersion (string[2])
    let version = String::from_utf8_lossy(&data[0..2]).to_string();
    tags.push(mktag("MOI", "MOIVersion", "MOI Version", Value::String(version)));

    // 0x06: DateTimeOriginal (undef[8]) = unpack 'nCCCCn'
    // year(u16), month(u8), day(u8), hour(u8), min(u8), ms*1000(u16)
    if data.len() >= 14 {
        let year = u16::from_be_bytes([data[6], data[7]]);
        let month = data[8];
        let day = data[9];
        let hour = data[10];
        let min = data[11];
        let ms = u16::from_be_bytes([data[12], data[13]]);
        let sec_f = ms as f64 / 1000.0;
        let dt = format!("{:04}:{:02}:{:02} {:02}:{:02}:{:06.3}", year, month, day, hour, min, sec_f);
        tags.push(mktag("MOI", "DateTimeOriginal", "Date/Time Original", Value::String(dt)));
    }

    // 0x0e: Duration (int32u, ms)
    if data.len() >= 0x12 {
        let dur_ms = u32::from_be_bytes([data[0x0e], data[0x0f], data[0x10], data[0x11]]);
        let dur_s = dur_ms as f64 / 1000.0;
        let dur_str = format!("{:.2} s", dur_s);
        tags.push(mktag("MOI", "Duration", "Duration", Value::String(dur_str)));
    }

    // 0x80: AspectRatio (int8u)
    if data.len() > 0x80 {
        let aspect = data[0x80];
        let lo = aspect & 0x0F;
        let hi = aspect >> 4;
        let aspect_str = match lo {
            0 | 1 => "4:3",
            4 | 5 => "16:9",
            _ => "Unknown",
        };
        let sys_str = match hi {
            4 => " NTSC",
            5 => " PAL",
            _ => "",
        };
        let full = format!("{}{}", aspect_str, sys_str);
        tags.push(mktag("MOI", "AspectRatio", "Aspect Ratio", Value::String(full)));
    }

    // 0x84: AudioCodec (int16u)
    if data.len() > 0x86 {
        let ac = u16::from_be_bytes([data[0x84], data[0x85]]);
        let codec = match ac {
            0x00c1 => "AC3",
            0x4001 => "MPEG",
            _ => "Unknown",
        };
        tags.push(mktag("MOI", "AudioCodec", "Audio Codec", Value::String(codec.into())));
    }

    // 0x86: AudioBitrate (int8u, val * 16000 + 48000)
    if data.len() > 0x86 {
        let ab = data[0x86];
        let bitrate = ab as u32 * 16000 + 48000;
        let bitrate_str = format!("{} kbps", bitrate / 1000);
        tags.push(mktag("MOI", "AudioBitrate", "Audio Bitrate", Value::String(bitrate_str)));
    }

    // 0xda: VideoBitrate (int16u with lookup)
    if data.len() > 0xdc {
        let vb = u16::from_be_bytes([data[0xda], data[0xdb]]);
        let vbps: Option<u32> = match vb {
            0x5896 => Some(8500000),
            0x813d => Some(5500000),
            _ => None,
        };
        if let Some(bps) = vbps {
            let vb_str = format!("{:.1} Mbps", bps as f64 / 1_000_000.0);
            tags.push(mktag("MOI", "VideoBitrate", "Video Bitrate", Value::String(vb_str)));
        }
    }

    Ok(tags)
}

// ============================================================================
// RAR
// ============================================================================

/// Read a ULEB128 (unsigned LEB128) integer from data at pos, advancing pos.
fn read_uleb128(data: &[u8], pos: &mut usize) -> Option<u64> {
    let mut result: u64 = 0;
    let mut shift = 0u32;
    loop {
        if *pos >= data.len() {
            return None;
        }
        let byte = data[*pos];
        *pos += 1;
        result |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 64 {
            return None;
        }
    }
    Some(result)
}

pub fn read_rar(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 8 || !data.starts_with(b"Rar!\x1A\x07") {
        return Err(Error::InvalidData("not a RAR file".into()));
    }

    let mut tags = Vec::new();

    if data[6] == 0x00 {
        // RAR v4
        tags.push(mktag("ZIP", "FileVersion", "File Version", Value::String("RAR v4".into())));
        read_rar4_entries(data, &mut tags);
    } else if data[6] == 0x01 && data[7] == 0x00 {
        // RAR v5
        tags.push(mktag("ZIP", "FileVersion", "File Version", Value::String("RAR v5".into())));
        read_rar5_entries(data, &mut tags);
    }

    Ok(tags)
}

fn read_rar5_entries(data: &[u8], tags: &mut Vec<Tag>) {
    // After 8-byte signature, iterate blocks:
    // each block: 4 bytes CRC32, then ULEB128 headSize, then headSize bytes header
    let mut pos = 8;

    loop {
        // skip 4-byte CRC
        if pos + 4 > data.len() {
            break;
        }
        pos += 4;

        let head_size = match read_uleb128(data, &mut pos) {
            Some(v) if v > 0 => v as usize,
            _ => break,
        };

        if pos + head_size > data.len() {
            break;
        }

        let header = &data[pos..pos + head_size];
        pos += head_size;

        let mut hpos = 0;
        let head_type = match read_uleb128(header, &mut hpos) {
            Some(v) => v,
            None => break,
        };

        // Skip headFlags
        let head_flag = match read_uleb128(header, &mut hpos) {
            Some(v) => v,
            None => break,
        };

        // head_type 2 = file header, 3 = service header
        if head_type != 2 && head_type != 3 {
            // Skip data section if present
            if head_flag & 0x0002 != 0 {
                // read extra data size to skip
                if let Some(data_size) = read_uleb128(data, &mut pos) {
                    pos += data_size as usize;
                }
            }
            continue;
        }

        // skip extraSize
        let _extra_size = read_uleb128(header, &mut hpos);

        let data_size: u64 = if head_flag & 0x0002 != 0 {
            match read_uleb128(header, &mut hpos) {
                Some(v) => v,
                None => break,
            }
        } else {
            0
        };

        if head_type == 3 {
            // service header - skip its data
            pos += data_size as usize;
            continue;
        }

        // File header
        if head_type == 2 {
            tags.push(mktag("ZIP", "CompressedSize", "Compressed Size", Value::U32(data_size as u32)));
        }

        let file_flag = match read_uleb128(header, &mut hpos) {
            Some(v) => v,
            None => { pos += data_size as usize; continue; }
        };
        let uncompressed_size = match read_uleb128(header, &mut hpos) {
            Some(v) => v,
            None => { pos += data_size as usize; continue; }
        };
        if file_flag & 0x0008 == 0 {
            tags.push(mktag("ZIP", "UncompressedSize", "Uncompressed Size", Value::U32(uncompressed_size as u32)));
        }

        // skip file attributes
        let _attrs = read_uleb128(header, &mut hpos);

        // optional mtime (4 bytes)
        if file_flag & 0x0002 != 0 {
            hpos += 4;
        }
        // optional CRC (4 bytes)
        if file_flag & 0x0004 != 0 {
            hpos += 4;
        }

        // skip compressionInfo
        let _comp_info = read_uleb128(header, &mut hpos);

        // OS
        if let Some(os_val) = read_uleb128(header, &mut hpos) {
            let os_name = match os_val {
                0 => "Win32",
                1 => "Unix",
                _ => "Unknown",
            };
            tags.push(mktag("ZIP", "OperatingSystem", "Operating System", Value::String(os_name.into())));
        }

        // filename: 1-byte length then name bytes
        if hpos < header.len() {
            let name_len = header[hpos] as usize;
            hpos += 1;
            if hpos + name_len <= header.len() {
                let name = String::from_utf8_lossy(&header[hpos..hpos + name_len])
                    .trim_end_matches('\0')
                    .to_string();
                if !name.is_empty() {
                    tags.push(mktag("ZIP", "ArchivedFileName", "Archived File Name", Value::String(name)));
                }
            }
        }

        pos += data_size as usize;
    }
}

fn read_rar4_entries(data: &[u8], tags: &mut Vec<Tag>) {
    // RAR v4: little-endian blocks after 7-byte signature
    let mut pos = 7;

    loop {
        if pos + 7 > data.len() {
            break;
        }
        // Block header: CRC(2) Type(1) Flags(2) Size(2)
        let block_type = data[pos + 2];
        let flags = u16::from_le_bytes([data[pos + 3], data[pos + 4]]);
        let mut size = u16::from_le_bytes([data[pos + 5], data[pos + 6]]) as usize;
        size = size.saturating_sub(7);

        if flags & 0x8000 != 0 {
            if pos + 11 > data.len() {
                break;
            }
            let add_size = u32::from_le_bytes([data[pos + 7], data[pos + 8], data[pos + 9], data[pos + 10]]) as usize;
            size = size.saturating_add(add_size).saturating_sub(4);
        }

        pos += 7;

        if block_type == 0x74 && size > 0 {
            // File block
            let n = size.min(4096).min(data.len() - pos);
            if n >= 16 {
                let file_data = &data[pos..pos + n];
                let compressed = u32::from_le_bytes([file_data[0], file_data[1], file_data[2], file_data[3]]) as u64;
                let uncompressed = u32::from_le_bytes([file_data[4], file_data[5], file_data[6], file_data[7]]) as u64;
                let os_byte = file_data[14];
                let name_len = u16::from_le_bytes([file_data[10], file_data[11]]) as usize;
                // name starts after 25-byte base header
                if n >= 25 + name_len {
                    let name = String::from_utf8_lossy(&file_data[25..25 + name_len]).to_string();
                    tags.push(mktag("ZIP", "CompressedSize", "Compressed Size", Value::U32(compressed as u32)));
                    tags.push(mktag("ZIP", "UncompressedSize", "Uncompressed Size", Value::U32(uncompressed as u32)));
                    let os_name = match os_byte {
                        0 => "MS-DOS",
                        1 => "OS/2",
                        2 => "Win32",
                        3 => "Unix",
                        _ => "Unknown",
                    };
                    tags.push(mktag("ZIP", "OperatingSystem", "Operating System", Value::String(os_name.into())));
                    tags.push(mktag("ZIP", "ArchivedFileName", "Archived File Name", Value::String(name)));
                }
            }
        }

        if size == 0 {
            break;
        }
        pos += size;
    }
}

// ============================================================================
// SVG (via XMP)
// ============================================================================

pub fn read_svg(data: &[u8]) -> Result<Vec<Tag>> {
    let text = String::from_utf8_lossy(data);

    if !text.contains("<svg") {
        return Err(Error::InvalidData("not an SVG file".into()));
    }

    let mut tags = Vec::new();

    // Parse SVG using XML parser for proper attribute/element extraction.
    // We handle three distinct sections:
    //   1. <svg> root element: version, xmlns, width, height attributes → SVG group tags
    //   2. <desc> and other non-metadata children: path-based tags → SVG group
    //   3. <metadata> block:
    //      a. <rdf:RDF> → extract to string, pass to XmpReader for XMP tags
    //      b. <c2pa:manifest> → base64-decode → JUMBF parsing
    use xml::reader::{EventReader, XmlEvent};
    let parser = EventReader::from_str(&text);
    let mut path: Vec<String> = Vec::new(); // element local names (ucfirst)
    let mut current_text = String::new();
    // Which section are we in?
    let mut in_metadata = false; // inside <metadata> element
    let mut in_rdf = 0_usize;    // nesting depth inside <rdf:RDF>
    let mut in_c2pa = 0_usize;   // nesting depth inside <c2pa:manifest>
    let mut in_svg_body = false; // inside SVG non-metadata body (desc, title, etc.)
    // Track whether each path element had child elements (to skip mixed-content text).
    // True = had at least one child element. Parallel to `path`.
    let mut had_child: Vec<bool> = Vec::new();

    for event in EventReader::from_str(text.as_ref()) {
        match event {
            Ok(XmlEvent::StartElement { name, attributes, namespace, .. }) => {
                let local = &name.local_name;
                let ns = name.namespace.as_deref().unwrap_or("");

                // Root SVG element
                if local == "svg" && path.is_empty() {
                    path.push("Svg".into());
                    had_child.push(false);
                    for attr in &attributes {
                        match attr.name.local_name.as_str() {
                            "width" => tags.push(mktag("SVG", "ImageWidth", "Image Width", Value::String(attr.value.clone()))),
                            "height" => tags.push(mktag("SVG", "ImageHeight", "Image Height", Value::String(attr.value.clone()))),
                            "version" => tags.push(mktag("SVG", "SVGVersion", "SVG Version", Value::String(attr.value.clone()))),
                            "viewBox" | "viewbox" => tags.push(mktag("SVG", "ViewBox", "View Box", Value::String(attr.value.clone()))),
                            "id" => tags.push(mktag("SVG", "ID", "ID", Value::String(attr.value.clone()))),
                            _ => {}
                        }
                    }
                    // Extract default namespace (xmlns="...") from the namespace map
                    if let Some(default_ns) = namespace.get("") {
                        if !default_ns.is_empty() {
                            tags.push(mktag("SVG", "Xmlns", "XMLNS", Value::String(default_ns.to_string())));
                        }
                    }
                    current_text.clear();
                    continue;
                }

                // <metadata> block — switch to metadata parsing mode
                if local == "metadata" && !in_metadata && in_rdf == 0 && in_c2pa == 0 {
                    in_metadata = true;
                    // Mark parent as having a child
                    if let Some(last) = had_child.last_mut() { *last = true; }
                    path.push("Metadata".into());
                    had_child.push(false);
                    current_text.clear();
                    continue;
                }

                // Inside metadata: handle RDF and c2pa
                if in_metadata {
                    if in_rdf > 0 {
                        in_rdf += 1;
                        current_text.clear();
                        continue;
                    }
                    if in_c2pa > 0 {
                        in_c2pa += 1;
                        current_text.clear();
                        continue;
                    }
                    // Starting rdf:RDF
                    if local == "RDF" && ns == "http://www.w3.org/1999/02/22-rdf-syntax-ns#" {
                        in_rdf = 1;
                        current_text.clear();
                        continue;
                    }
                    // Starting c2pa:manifest
                    if name.prefix.as_deref() == Some("c2pa") || local == "manifest" {
                        in_c2pa = 1;
                        current_text.clear();
                        continue;
                    }
                    // Other metadata children: ignore
                    current_text.clear();
                    continue;
                }

                // SVG body elements (desc, title, etc.) - NOT metadata, NOT root svg
                if !in_metadata && path.len() >= 1 {
                    in_svg_body = true;
                    // Mark parent as having a child
                    if let Some(last) = had_child.last_mut() { *last = true; }
                    let ucfirst_local = svg_ucfirst(local);
                    path.push(ucfirst_local);
                    had_child.push(false);
                    current_text.clear();
                    continue;
                }

                path.push(svg_ucfirst(local));
                had_child.push(false);
                current_text.clear();
            }
            Ok(XmlEvent::Characters(t)) | Ok(XmlEvent::CData(t)) => {
                current_text.push_str(&t);
            }
            Ok(XmlEvent::EndElement { name }) => {
                let local = &name.local_name;

                // Exiting rdf:RDF depth
                if in_rdf > 0 {
                    in_rdf -= 1;
                    current_text.clear();
                    continue;
                }

                // Exiting c2pa:manifest depth
                if in_c2pa > 0 {
                    in_c2pa -= 1;
                    if in_c2pa == 0 {
                        // We've collected the base64 c2pa manifest text
                        let b64 = current_text.chars().filter(|c| !c.is_whitespace()).collect::<String>();
                        if !b64.is_empty() {
                            if let Ok(jumbf_data) = base64_decode(&b64) {
                                let jumbf_group = crate::tag::TagGroup {
                                    family0: "JUMBF".into(),
                                    family1: "JUMBF".into(),
                                    family2: "Image".into(),
                                };
                                let print = format!("(Binary data {} bytes, use -b option to extract)", jumbf_data.len());
                                tags.push(crate::tag::Tag {
                                    id: crate::tag::TagId::Text("JUMBF".into()),
                                    name: "JUMBF".into(),
                                    description: "JUMBF".into(),
                                    group: jumbf_group,
                                    raw_value: Value::Binary(jumbf_data.clone()),
                                    print_value: print,
                                    priority: 0,
                                });
                                parse_jumbf_for_svg(&jumbf_data, &mut tags);
                            }
                        }
                    }
                    current_text.clear();
                    continue;
                }

                // Exiting metadata
                if local == "metadata" && in_metadata {
                    in_metadata = false;
                    path.pop(); // pop "Metadata"
                    current_text.clear();
                    continue;
                }

                // Skip other metadata children
                if in_metadata {
                    current_text.clear();
                    continue;
                }

                // SVG body element text
                if in_svg_body && path.len() >= 2 {
                    let t = current_text.trim().to_string();
                    if !t.is_empty() {
                        // Build tag name from path (skip root "Svg")
                        let tag_name = path.iter().skip(1).cloned().collect::<String>();
                        if !tag_name.is_empty() {
                            tags.push(mktag("SVG", &tag_name, &tag_name, Value::String(t)));
                        }
                    }
                    path.pop();
                    // If we've returned to Svg level (path.len() == 1), exit svg_body
                    if path.len() <= 1 {
                        in_svg_body = false;
                    }
                    current_text.clear();
                    continue;
                }

                path.pop();
                current_text.clear();
            }
            Err(_) => break,
            _ => {}
        }
    }

    // Now extract XMP from the <rdf:RDF> block.
    // We look for the rdf:RDF section in the original text and pass it to XmpReader.
    // XmpReader handles rdf:RDF as a valid XMP envelope.
    if let Some(rdf_start) = text.find("<rdf:RDF") {
        if let Some(rdf_end) = text.find("</rdf:RDF>") {
            let rdf_section = &text[rdf_start..rdf_end + "</rdf:RDF>".len()];
            if let Ok(xmp_tags) = XmpReader::read(rdf_section.as_bytes()) {
                tags.extend(xmp_tags);
            }
        }
    }

    // Handle c2pa:manifest with potentially undeclared namespace prefix.
    // Use text-based extraction since the XML parser may fail on undeclared namespaces.
    if let Some(mstart) = text.find("<c2pa:manifest>") {
        let content_start = mstart + "<c2pa:manifest>".len();
        if let Some(mend) = text[content_start..].find("</c2pa:manifest>") {
            let b64_content = &text[content_start..content_start + mend];
            let b64: String = b64_content.chars().filter(|c| !c.is_whitespace()).collect();
            if !b64.is_empty() {
                if let Ok(jumbf_data) = base64_decode(&b64) {
                    let jumbf_group = crate::tag::TagGroup {
                        family0: "JUMBF".into(),
                        family1: "JUMBF".into(),
                        family2: "Image".into(),
                    };
                    let print = format!("(Binary data {} bytes, use -b option to extract)", jumbf_data.len());
                    tags.push(crate::tag::Tag {
                        id: crate::tag::TagId::Text("JUMBF".into()),
                        name: "JUMBF".into(),
                        description: "JUMBF".into(),
                        group: jumbf_group,
                        raw_value: Value::Binary(jumbf_data.clone()),
                        print_value: print,
                        priority: 0,
                    });
                    parse_jumbf_for_svg(&jumbf_data, &mut tags);
                }
            }
        }
    }

    Ok(tags)
}

/// UCfirst a string, preserving the rest as-is (for SVG element name path building).
fn svg_ucfirst(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

/// Simple base64 decoder (no padding required).
fn base64_decode(s: &str) -> std::result::Result<Vec<u8>, ()> {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut table = [0u8; 256];
    for (i, &c) in alphabet.iter().enumerate() {
        table[c as usize] = i as u8;
    }
    let bytes: Vec<u8> = s.bytes().filter(|&b| b != b'=' && b != b'\n' && b != b'\r' && b != b' ').collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        if chunk.len() < 2 { break; }
        let b0 = table[chunk[0] as usize];
        let b1 = table[chunk[1] as usize];
        out.push((b0 << 2) | (b1 >> 4));
        if chunk.len() >= 3 {
            let b2 = table[chunk[2] as usize];
            out.push((b1 << 4) | (b2 >> 2));
            if chunk.len() >= 4 {
                let b3 = table[chunk[3] as usize];
                out.push((b2 << 6) | b3);
            }
        }
    }
    Ok(out)
}

/// Parse JUMBF box structure from SVG c2pa:manifest to extract tags.
/// Mirrors the JPEG APP11 JUMBF parser logic.
fn parse_jumbf_for_svg(data: &[u8], tags: &mut Vec<Tag>) {
    parse_jumbf_boxes_svg(data, tags, true);
}

fn parse_jumbf_boxes_svg(data: &[u8], tags: &mut Vec<Tag>, top_level: bool) {
    let mut pos = 0;
    while pos + 8 <= data.len() {
        let lbox = u32::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
        let tbox = &data[pos+4..pos+8];
        if lbox < 8 || pos + lbox > data.len() { break; }
        let content = &data[pos+8..pos+lbox];

        if tbox == b"jumb" {
            parse_jumbf_jumd_svg(content, tags, top_level);
        }

        pos += lbox;
    }
}

fn parse_jumbf_jumd_svg(data: &[u8], tags: &mut Vec<Tag>, emit_desc: bool) {
    let jumbf_group = crate::tag::TagGroup {
        family0: "JUMBF".into(),
        family1: "JUMBF".into(),
        family2: "Image".into(),
    };

    let mut pos = 0;
    let mut found_jumd = false;

    while pos + 8 <= data.len() {
        let lbox = u32::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
        let tbox = &data[pos+4..pos+8];
        if lbox < 8 || pos + lbox > data.len() { break; }
        let content = &data[pos+8..pos+lbox];

        if tbox == b"jumd" && !found_jumd {
            found_jumd = true;
            if content.len() >= 17 {
                let type_bytes = &content[..16];
                let label_data = &content[17..];
                let null_pos = label_data.iter().position(|&b| b == 0).unwrap_or(label_data.len());
                let label = String::from_utf8_lossy(&label_data[..null_pos]).to_string();

                if emit_desc {
                    // Emit JUMDType
                    let type_hex: String = type_bytes.iter().map(|b| format!("{:02x}", b)).collect();
                    let a1 = &type_hex[8..12];
                    let a2 = &type_hex[12..16];
                    let a3 = &type_hex[16..32];
                    let ascii4 = &type_bytes[..4];
                    let is_printable = ascii4.iter().all(|&b| b.is_ascii_alphanumeric());
                    let print_type = if is_printable {
                        let ascii_str = String::from_utf8_lossy(ascii4);
                        format!("({})-{}-{}-{}", ascii_str, a1, a2, a3)
                    } else {
                        format!("{}-{}-{}-{}", &type_hex[..8], a1, a2, a3)
                    };
                    tags.push(crate::tag::Tag {
                        id: crate::tag::TagId::Text("JUMDType".into()),
                        name: "JUMDType".into(),
                        description: "JUMD Type".into(),
                        group: jumbf_group.clone(),
                        raw_value: Value::String(type_hex),
                        print_value: print_type,
                        priority: 0,
                    });
                    if !label.is_empty() {
                        tags.push(crate::tag::Tag {
                            id: crate::tag::TagId::Text("JUMDLabel".into()),
                            name: "JUMDLabel".into(),
                            description: "JUMD Label".into(),
                            group: jumbf_group.clone(),
                            raw_value: Value::String(label.clone()),
                            print_value: label.clone(),
                            priority: 0,
                        });
                    }
                }
            }
        } else if tbox == b"json" {
            // Parse JSON content to extract named fields
            if let Ok(json_str) = std::str::from_utf8(content) {
                parse_jumbf_json_svg(json_str.trim(), tags, &jumbf_group);
            }
        } else if tbox == b"jumb" {
            // Nested container: recurse without emitting JUMDType/Label again
            parse_jumbf_jumd_svg(content, tags, false);
        }

        pos += lbox;
    }
}

/// Parse a JUMBF JSON box to extract known fields (location, copyright, etc.)
fn parse_jumbf_json_svg(json: &str, tags: &mut Vec<Tag>, group: &crate::tag::TagGroup) {
    // Simple JSON field extractor for string values
    // Matches: "key": "value" patterns
    let mut i = 0;
    let bytes = json.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'"' {
            // Read key
            i += 1;
            let key_start = i;
            while i < bytes.len() && bytes[i] != b'"' { i += 1; }
            let key = &json[key_start..i];
            i += 1; // skip closing "
            // Skip whitespace and colon
            while i < bytes.len() && (bytes[i] == b':' || bytes[i] == b' ') { i += 1; }
            // Read value if it's a string
            if i < bytes.len() && bytes[i] == b'"' {
                i += 1;
                let val_start = i;
                while i < bytes.len() && bytes[i] != b'"' { i += 1; }
                let val = &json[val_start..i];
                i += 1;

                // Map known C2PA JSON keys to tag names
                let tag_name = match key {
                    "location" => Some("Location"),
                    "copyright" => Some("Copyright"),
                    "title" => Some("AssetInfoTitle"),
                    _ => None,
                };
                if let Some(name) = tag_name {
                    tags.push(crate::tag::Tag {
                        id: crate::tag::TagId::Text(name.into()),
                        name: name.into(),
                        description: name.into(),
                        group: group.clone(),
                        raw_value: Value::String(val.to_string()),
                        print_value: val.to_string(),
                        priority: 0,
                    });
                }
            }
        } else {
            i += 1;
        }
    }
}

fn extract_xml_attr(tag: &str, name: &str) -> Option<String> {
    let pat = format!("{}=\"", name);
    let pos = tag.find(&pat)?;
    let rest = &tag[pos + pat.len()..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

// ============================================================================
// JSON
// ============================================================================

pub fn read_json(data: &[u8]) -> Result<Vec<Tag>> {
    let text = String::from_utf8_lossy(data);
    let trimmed = text.trim();
    if !trimmed.starts_with('{') && !trimmed.starts_with('[') {
        return Err(Error::InvalidData("not a JSON file".into()));
    }

    let mut tags = Vec::new();

    // Parse top-level JSON object fields
    if trimmed.starts_with('{') {
        let mut collected: Vec<(String, String)> = Vec::new();
        parse_json_object(trimmed, "", &mut collected);
        for (key, value) in collected {
            let tag_name = json_key_to_tag_name(&key);
            if tag_name.is_empty() { continue; }
            tags.push(mktag("JSON", &tag_name, &tag_name, Value::String(value)));
        }
    }

    Ok(tags)
}

/// Recursively parse a JSON object, collecting (flat_tag_name, value) pairs.
/// For nested objects, the key is prepended to nested keys.
fn parse_json_object(json: &str, prefix: &str, out: &mut Vec<(String, String)>) {
    let mut pos = 0;
    let chars: Vec<char> = json.chars().collect();

    // skip opening {
    if pos < chars.len() && chars[pos] == '{' {
        pos += 1;
    }

    loop {
        // skip whitespace and commas
        while pos < chars.len() && (chars[pos].is_whitespace() || chars[pos] == ',') {
            pos += 1;
        }
        if pos >= chars.len() || chars[pos] == '}' {
            break;
        }

        // read key
        if chars[pos] != '"' {
            break;
        }
        let key = read_json_string(&chars, &mut pos);

        // skip whitespace and colon
        while pos < chars.len() && (chars[pos].is_whitespace() || chars[pos] == ':') {
            pos += 1;
        }

        // read value
        if pos >= chars.len() {
            break;
        }

        let full_key = if prefix.is_empty() { key.clone() } else { format!("{}{}", prefix, ucfirst_str(&key)) };

        match chars[pos] {
            '"' => {
                let val = read_json_string(&chars, &mut pos);
                out.push((full_key, val));
            }
            '{' => {
                let obj_start = pos;
                let obj_end = find_matching_bracket(&chars, pos, '{', '}');
                let obj_str: String = chars[obj_start..obj_end + 1].iter().collect();
                // For objects, flatten with parent key as prefix
                parse_json_object(&obj_str, &full_key, out);
                pos = obj_end + 1;
            }
            '[' => {
                let arr_start = pos;
                let arr_end = find_matching_bracket(&chars, pos, '[', ']');
                let arr_str: String = chars[arr_start..arr_end + 1].iter().collect();
                // Check if array contains objects (array-of-objects flattening)
                if array_contains_objects(&arr_str) {
                    // Flatten: parse each object with parent key as prefix, accumulate per sub-key
                    let mut sub_map: Vec<(String, Vec<String>)> = Vec::new();
                    parse_json_array_of_objects(&arr_str, &full_key, &mut sub_map);
                    for (sub_key, vals) in sub_map {
                        if !vals.is_empty() {
                            out.push((sub_key, vals.join(", ")));
                        }
                    }
                } else {
                    let values = parse_json_array(&arr_str);
                    if !values.is_empty() {
                        out.push((full_key, values.join(", ")));
                    }
                }
                pos = arr_end + 1;
            }
            'n' => {
                // null
                pos += 4;
                out.push((full_key, "null".into()));
            }
            't' => {
                // true
                pos += 4;
                out.push((full_key, "1".into()));
            }
            'f' => {
                // false
                pos += 5;
                out.push((full_key, "0".into()));
            }
            _ => {
                // number
                let num_start = pos;
                while pos < chars.len() && !chars[pos].is_whitespace() && chars[pos] != ',' && chars[pos] != '}' {
                    pos += 1;
                }
                let num: String = chars[num_start..pos].iter().collect();
                out.push((full_key, num));
            }
        }
    }
}

fn parse_json_array(json: &str) -> Vec<String> {
    let mut results = Vec::new();
    let chars: Vec<char> = json.chars().collect();
    let mut pos = 0;

    if pos < chars.len() && chars[pos] == '[' {
        pos += 1;
    }

    loop {
        while pos < chars.len() && (chars[pos].is_whitespace() || chars[pos] == ',') {
            pos += 1;
        }
        if pos >= chars.len() || chars[pos] == ']' {
            break;
        }

        match chars[pos] {
            '"' => {
                let val = read_json_string(&chars, &mut pos);
                results.push(val);
            }
            '[' => {
                let end = find_matching_bracket(&chars, pos, '[', ']');
                let sub: String = chars[pos..end + 1].iter().collect();
                let sub_vals = parse_json_array(&sub);
                results.extend(sub_vals);
                pos = end + 1;
            }
            '{' => {
                let end = find_matching_bracket(&chars, pos, '{', '}');
                pos = end + 1;
            }
            'n' => { pos += 4; results.push("null".into()); }
            't' => { pos += 4; results.push("1".into()); }
            'f' => { pos += 5; results.push("0".into()); }
            _ => {
                let start = pos;
                while pos < chars.len() && !chars[pos].is_whitespace() && chars[pos] != ',' && chars[pos] != ']' {
                    pos += 1;
                }
                results.push(chars[start..pos].iter().collect());
            }
        }
    }
    results
}

/// Returns true if the JSON array contains at least one object element.
fn array_contains_objects(json: &str) -> bool {
    let chars: Vec<char> = json.chars().collect();
    let mut pos = 0;
    if pos < chars.len() && chars[pos] == '[' { pos += 1; }
    while pos < chars.len() {
        if chars[pos].is_whitespace() || chars[pos] == ',' { pos += 1; continue; }
        if chars[pos] == ']' { break; }
        if chars[pos] == '{' { return true; }
        break;
    }
    false
}

/// Parse an array of objects, accumulating sub-fields per key.
/// sub_map: Vec<(sub_key, Vec<value>)> — ordered by first occurrence.
fn parse_json_array_of_objects(json: &str, prefix: &str, sub_map: &mut Vec<(String, Vec<String>)>) {
    let chars: Vec<char> = json.chars().collect();
    let mut pos = 0;
    if pos < chars.len() && chars[pos] == '[' { pos += 1; }

    loop {
        while pos < chars.len() && (chars[pos].is_whitespace() || chars[pos] == ',') { pos += 1; }
        if pos >= chars.len() || chars[pos] == ']' { break; }
        if chars[pos] == '{' {
            let end = find_matching_bracket(&chars, pos, '{', '}');
            let obj_str: String = chars[pos..end + 1].iter().collect();
            let mut obj_fields: Vec<(String, String)> = Vec::new();
            parse_json_object(&obj_str, prefix, &mut obj_fields);
            for (k, v) in obj_fields {
                if let Some(entry) = sub_map.iter_mut().find(|(sk, _)| sk == &k) {
                    // Append multiple values from nested arrays too
                    for part in v.split(", ") {
                        entry.1.push(part.to_string());
                    }
                } else {
                    let vals: Vec<String> = v.split(", ").map(|s| s.to_string()).collect();
                    sub_map.push((k, vals));
                }
            }
            pos = end + 1;
        } else {
            // Non-object element, skip
            while pos < chars.len() && chars[pos] != ',' && chars[pos] != ']' { pos += 1; }
        }
    }
}

fn read_json_string(chars: &[char], pos: &mut usize) -> String {
    if *pos >= chars.len() || chars[*pos] != '"' {
        return String::new();
    }
    *pos += 1; // skip opening "
    let mut result = String::new();
    while *pos < chars.len() && chars[*pos] != '"' {
        if chars[*pos] == '\\' && *pos + 1 < chars.len() {
            *pos += 1;
            match chars[*pos] {
                '"' => result.push('"'),
                '\\' => result.push('\\'),
                '/' => result.push('/'),
                'n' => result.push('\n'),
                'r' => result.push('\r'),
                't' => result.push('\t'),
                _ => result.push(chars[*pos]),
            }
        } else {
            result.push(chars[*pos]);
        }
        *pos += 1;
    }
    if *pos < chars.len() { *pos += 1; } // skip closing "
    result
}

fn find_matching_bracket(chars: &[char], start: usize, open: char, close: char) -> usize {
    let mut level = 0;
    let mut pos = start;
    let mut in_string = false;
    while pos < chars.len() {
        if chars[pos] == '"' && (pos == 0 || chars[pos - 1] != '\\') {
            in_string = !in_string;
        }
        if !in_string {
            if chars[pos] == open { level += 1; }
            else if chars[pos] == close {
                level -= 1;
                if level == 0 { return pos; }
            }
        }
        pos += 1;
    }
    pos.saturating_sub(1)
}

fn ucfirst_str(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

/// Convert a JSON key (possibly nested like "testThis") to ExifTool tag name.
/// Mirrors Perl: ucfirst, then capitalize letters after non-alphabetic chars.
fn json_key_to_tag_name(key: &str) -> String {
    // ucfirst
    let key = ucfirst_str(key);
    // Capitalize after non-alpha: s/([^a-zA-Z])([a-z])/$1\U$2/g
    let mut result = String::new();
    let chars: Vec<char> = key.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        result.push(c);
        if !c.is_ascii_alphabetic() && i + 1 < chars.len() {
            if chars[i + 1].is_ascii_lowercase() {
                let uc = chars[i + 1].to_ascii_uppercase();
                result.push(uc);
                i += 2;
                continue;
            }
        }
        i += 1;
    }
    result
}

// ============================================================================
// RealAudio (RA)
// ============================================================================

/// Parse RealAudio (.ra) files. Mirrors ExifTool's Real.pm ProcessReal for RA.
pub fn read_real_audio(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 8 || !data.starts_with(b".ra\xfd") {
        return Err(Error::InvalidData("not a RealAudio file".into()));
    }

    let mut tags = Vec::new();
    let version = u16::from_be_bytes([data[4], data[5]]);

    // Only support version 4 currently (most common)
    if version != 4 {
        return Ok(tags);
    }

    // AudioV4: starts at offset 8
    let d = &data[8..];
    if d.len() < 40 {
        return Ok(tags);
    }

    let mut pos = 0;
    // Field 0: FourCC1 (4 bytes, undef)
    pos += 4;
    // Field 1: AudioFileSize (int32u)
    pos += 4;
    // Field 2: Version2 (int16u)
    pos += 2;
    // Field 3: HeaderSize (int32u)
    pos += 4;
    // Field 4: CodecFlavorID (int16u)
    pos += 2;
    // Field 5: CodedFrameSize (int32u)
    pos += 4;

    if pos + 4 > d.len() { return Ok(tags); }
    // Field 6: AudioBytes (int32u)
    let audio_bytes = u32::from_be_bytes([d[pos], d[pos+1], d[pos+2], d[pos+3]]);
    pos += 4;
    tags.push(mktag("Real", "AudioBytes", "Audio Bytes", Value::U32(audio_bytes)));

    if pos + 4 > d.len() { return Ok(tags); }
    // Field 7: BytesPerMinute (int32u)
    let bpm = u32::from_be_bytes([d[pos], d[pos+1], d[pos+2], d[pos+3]]);
    pos += 4;
    tags.push(mktag("Real", "BytesPerMinute", "Bytes Per Minute", Value::U32(bpm)));

    // Field 8: Unknown (int32u)
    pos += 4;
    // Field 9: SubPacketH (int16u)
    pos += 2;

    if pos + 2 > d.len() { return Ok(tags); }
    // Field 10: AudioFrameSize (int16u)
    let afs = u16::from_be_bytes([d[pos], d[pos+1]]);
    pos += 2;
    tags.push(mktag("Real", "AudioFrameSize", "Audio Frame Size", Value::U16(afs)));

    // Field 11: SubPacketSize (int16u)
    pos += 2;
    // Field 12: Unknown (int16u)
    pos += 2;

    if pos + 2 > d.len() { return Ok(tags); }
    // Field 13: SampleRate (int16u)
    let sr = u16::from_be_bytes([d[pos], d[pos+1]]);
    pos += 2;
    tags.push(mktag("Real", "SampleRate", "Sample Rate", Value::U16(sr)));

    // Field 14: Unknown (int16u)
    pos += 2;

    if pos + 2 > d.len() { return Ok(tags); }
    // Field 15: BitsPerSample (int16u)
    let bps = u16::from_be_bytes([d[pos], d[pos+1]]);
    pos += 2;
    tags.push(mktag("Real", "BitsPerSample", "Bits Per Sample", Value::U16(bps)));

    if pos + 2 > d.len() { return Ok(tags); }
    // Field 16: Channels (int16u)
    let ch = u16::from_be_bytes([d[pos], d[pos+1]]);
    pos += 2;
    tags.push(mktag("Real", "Channels", "Channels", Value::U16(ch)));

    if pos >= d.len() { return Ok(tags); }
    // Field 17: FourCC2Len (int8u)
    let fc2l = d[pos] as usize;
    pos += 1;
    pos += fc2l; // skip FourCC2

    if pos >= d.len() { return Ok(tags); }
    // Field 19: FourCC3Len (int8u)
    let fc3l = d[pos] as usize;
    pos += 1;
    pos += fc3l; // skip FourCC3

    if pos >= d.len() { return Ok(tags); }
    // Field 21: Unknown (int8u)
    pos += 1;

    if pos + 2 > d.len() { return Ok(tags); }
    // Field 22: Unknown (int16u)
    pos += 2;

    // Field 23: TitleLen (int8u)
    if pos >= d.len() { return Ok(tags); }
    let title_len = d[pos] as usize;
    pos += 1;

    // Field 24: Title (string[TitleLen])
    if pos + title_len <= d.len() && title_len > 0 {
        let title = String::from_utf8_lossy(&d[pos..pos + title_len]).to_string();
        tags.push(mktag("Real", "Title", "Title", Value::String(title)));
    }
    pos += title_len;

    // Field 25: ArtistLen (int8u)
    if pos >= d.len() { return Ok(tags); }
    let artist_len = d[pos] as usize;
    pos += 1;

    // Field 26: Artist
    if pos + artist_len <= d.len() && artist_len > 0 {
        let artist = String::from_utf8_lossy(&d[pos..pos + artist_len]).to_string();
        tags.push(mktag("Real", "Artist", "Artist", Value::String(artist)));
    }
    pos += artist_len;

    // Field 27: CopyrightLen (int8u)
    if pos >= d.len() { return Ok(tags); }
    let copy_len = d[pos] as usize;
    pos += 1;

    // Field 28: Copyright
    if pos + copy_len <= d.len() && copy_len > 0 {
        let copyright = String::from_utf8_lossy(&d[pos..pos + copy_len]).to_string();
        tags.push(mktag("Real", "Copyright", "Copyright", Value::String(copyright)));
    }

    Ok(tags)
}

// ============================================================================
// AAC (Advanced Audio Coding)
// ============================================================================

pub fn read_aac(data: &[u8]) -> Result<Vec<Tag>> {
    // AAC ADTS frame header: 7 bytes minimum
    if data.len() < 7 || data[0] != 0xFF || (data[1] != 0xF0 && data[1] != 0xF1) {
        return Err(Error::InvalidData("not an AAC ADTS file".into()));
    }

    // unpack as Perl: N=u32 big-endian from bytes 0-3, n=u16 from bytes 4-5, C=u8 from byte 6
    let t0 = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    let t1 = u16::from_be_bytes([data[4], data[5]]);
    let t2 = data[6];

    // Validate: profile type
    // In Perl: $t[0]>>16 & 0x03 = bits 17-16 counting from right (0=LSB) of big-endian u32
    // These correspond to stream bits 14-15 in Perl's Bit016-017 numbering
    // Perl uses ProcessBitStream which reads MSB-first; bits 16-17 from stream start = byte2 bits 0-1 from MSB
    let profile_type = (t0 >> 16) & 0x03; // matches Perl $t[0]>>16 & 0x03
    if profile_type == 3 {
        return Err(Error::InvalidData("reserved AAC profile type".into()));
    }

    // Sampling rate index: stream bits 18-21
    // In Perl's ProcessBitStream: Bit018-021 = byte 2 bits 2-5 from MSB
    // In big-endian u32 t0: byte 2 is bits 15-8. Byte2 bits 2-5 from MSB = t0 bits 13-10 from right.
    // (t0 >> 10) & 0x0F
    let sr_index = (t0 >> 10) & 0x0F;
    if sr_index > 12 {
        return Err(Error::InvalidData("invalid AAC sampling rate index".into()));
    }

    // Channel configuration: stream bits 23-25
    // byte2 bit 7 from MSB (stream bit 23) = t0 bit 8 from right
    // byte3 bits 0-1 from MSB (stream bits 24-25) = t0 bits 7-6 from right
    // (t0 >> 6) & 0x07
    let channel_config = (t0 >> 6) & 0x07;

    let mut tags = Vec::new();

    // ProfileType
    let profile_name = match profile_type {
        0 => "Main",
        1 => "Low Complexity",
        2 => "Scalable Sampling Rate",
        _ => "Unknown",
    };
    tags.push(mktag("AAC", "ProfileType", "Profile Type", Value::String(profile_name.into())));

    // SampleRate
    let sample_rates = [96000u32, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350];
    if let Some(&sr) = sample_rates.get(sr_index as usize) {
        tags.push(mktag("AAC", "SampleRate", "Sample Rate", Value::U32(sr)));
    }

    // Channels
    let channels_str = match channel_config {
        0 => "?",
        1 => "1",
        2 => "2",
        3 => "3",
        4 => "4",
        5 => "5",
        6 => "5+1",
        7 => "7+1",
        _ => "?",
    };
    tags.push(mktag("AAC", "Channels", "Channels", Value::String(channels_str.into())));

    // Frame length: bits 30-42 (13 bits)
    // $len = (($t0 << 11) & 0x1800) | (($t1 >> 5) & 0x07ff)
    let len = (((t0 as u64) << 11) & 0x1800) | (((t1 as u64) >> 5) & 0x07FF);
    let len = len as usize;

    // Try to extract Encoder from the filler payload in the frame.
    // Scan the remaining data for a printable ASCII string (like encoder name).
    if len >= 8 && data.len() >= len {
        let frame_data = &data[7..len];
        // Scan for a null-delimited printable string in the frame payload
        // The encoder string is typically in a filler element, null-terminated
        let mut i = 0;
        while i < frame_data.len() {
            // Skip null bytes
            while i < frame_data.len() && frame_data[i] == 0 { i += 1; }
            let start = i;
            // Read printable bytes
            while i < frame_data.len() && frame_data[i] >= 0x20 && frame_data[i] <= 0x7e { i += 1; }
            let end = i;
            if end - start >= 4 {
                if let Ok(enc) = std::str::from_utf8(&frame_data[start..end]) {
                    let enc = enc.trim();
                    if enc.len() >= 4 {
                        tags.push(mktag("AAC", "Encoder", "Encoder", Value::String(enc.into())));
                        break;
                    }
                }
            }
            i += 1;
        }
    }

    Ok(tags)
}

// ============================================================================
// WPG (WordPerfect Graphics)
// ============================================================================

pub fn read_wpg(data: &[u8]) -> Result<Vec<Tag>> {
    // WPG magic: FF 57 50 43
    if data.len() < 16 || &data[0..4] != b"\xff\x57\x50\x43" {
        return Err(Error::InvalidData("not a WPG file".into()));
    }

    let mut tags = Vec::new();

    // Offset to first record (little-endian u32 at bytes 4-7)
    let offset = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
    // Version at bytes 10-11
    let ver = data[10];
    let rev = data[11];
    tags.push(mktag("WPG", "WPGVersion", "WPG Version", Value::String(format!("{}.{}", ver, rev))));

    if ver < 1 || ver > 2 {
        return Ok(tags);
    }

    // Determine start position
    let mut pos = if offset > 16 { offset } else { 16 };
    if pos > data.len() { pos = data.len(); }

    let mut records: Vec<String> = Vec::new();
    let mut last_type: Option<u32> = None;
    let mut count = 0usize;
    let mut image_width_inches: Option<f64> = None;
    let mut image_height_inches: Option<f64> = None;

    // WPG v1 record map
    let v1_map: std::collections::HashMap<u32, &str> = [
        (0x01, "Fill Attributes"), (0x02, "Line Attributes"), (0x03, "Marker Attributes"),
        (0x04, "Polymarker"), (0x05, "Line"), (0x06, "Polyline"), (0x07, "Rectangle"),
        (0x08, "Polygon"), (0x09, "Ellipse"), (0x0a, "Reserved"), (0x0b, "Bitmap (Type 1)"),
        (0x0c, "Graphics Text (Type 1)"), (0x0d, "Graphics Text Attributes"),
        (0x0e, "Color Map"), (0x0f, "Start WPG (Type 1)"), (0x10, "End WPG"),
        (0x11, "PostScript Data (Type 1)"), (0x12, "Output Attributes"),
        (0x13, "Curved Polyline"), (0x14, "Bitmap (Type 2)"), (0x15, "Start Figure"),
        (0x16, "Start Chart"), (0x17, "PlanPerfect Data"), (0x18, "Graphics Text (Type 2)"),
        (0x19, "Start WPG (Type 2)"), (0x1a, "Graphics Text (Type 3)"),
        (0x1b, "PostScript Data (Type 2)"),
    ].iter().cloned().collect();

    // WPG v2 record map
    let v2_map: std::collections::HashMap<u32, &str> = [
        (0x00, "End Marker"), (0x01, "Start WPG"), (0x02, "End WPG"),
        (0x03, "Form Settings"), (0x04, "Ruler Settings"), (0x05, "Grid Settings"),
        (0x06, "Layer"), (0x08, "Pen Style Definition"), (0x09, "Pattern Definition"),
        (0x0a, "Comment"), (0x0b, "Color Transfer"), (0x0c, "Color Palette"),
        (0x0d, "DP Color Palette"), (0x0e, "Bitmap Data"), (0x0f, "Text Data"),
        (0x10, "Chart Style"), (0x11, "Chart Data"), (0x12, "Object Image"),
        (0x15, "Polyline"), (0x16, "Polyspline"), (0x17, "Polycurve"),
        (0x18, "Rectangle"), (0x19, "Arc"), (0x1a, "Compound Polygon"),
        (0x1b, "Bitmap"), (0x1c, "Text Line"), (0x1d, "Text Block"),
        (0x1e, "Text Path"), (0x1f, "Chart"), (0x20, "Group"),
        (0x21, "Object Capsule"), (0x22, "Font Settings"), (0x25, "Pen Fore Color"),
        (0x26, "DP Pen Fore Color"), (0x27, "Pen Back Color"), (0x28, "DP Pen Back Color"),
        (0x29, "Pen Style"), (0x2a, "Pen Pattern"), (0x2b, "Pen Size"),
        (0x2c, "DP Pen Size"), (0x2d, "Line Cap"), (0x2e, "Line Join"),
        (0x2f, "Brush Gradient"), (0x30, "DP Brush Gradient"), (0x31, "Brush Fore Color"),
        (0x32, "DP Brush Fore Color"), (0x33, "Brush Back Color"), (0x34, "DP Brush Back Color"),
        (0x35, "Brush Pattern"), (0x36, "Horizontal Line"), (0x37, "Vertical Line"),
        (0x38, "Poster Settings"), (0x39, "Image State"), (0x3a, "Envelope Definition"),
        (0x3b, "Envelope"), (0x3c, "Texture Definition"), (0x3d, "Brush Texture"),
        (0x3e, "Texture Alignment"), (0x3f, "Pen Texture "),
    ].iter().cloned().collect();

    let mut safety = 0;
    loop {
        if pos >= data.len() || safety > 10000 { break; }
        safety += 1;

        let (record_type, len, get_size) = if ver == 1 {
            if pos >= data.len() { break; }
            let rtype = data[pos] as u32;
            pos += 1;
            // Read var-int length
            let (l, advance) = read_wpg_varint(data, pos);
            pos += advance;
            let gs = rtype == 0x0f; // Start WPG (Type 1)
            (rtype, l, gs)
        } else {
            // Version 2: read 2 bytes for flags+type
            if pos + 1 >= data.len() { break; }
            let rtype = data[pos + 1] as u32;
            pos += 2;
            // Skip extensions (var-int)
            let (_, adv) = read_wpg_varint(data, pos);
            pos += adv;
            // Read record length (var-int)
            let (l, adv2) = read_wpg_varint(data, pos);
            pos += adv2;
            let gs = rtype == 0x01; // Start WPG
            let rtype_opt = if rtype > 0x3f { u32::MAX } else { rtype };
            (rtype_opt, l, gs)
        };

        if record_type == u32::MAX {
            // Skip unknown v2 record
            pos += len;
            continue;
        }

        if get_size {
            // Read Start record to get image dimensions
            let rec_end = pos + len;
            if rec_end > data.len() { break; }
            let rec = &data[pos..rec_end];
            pos = rec_end;

            if ver == 1 && rec.len() >= 6 {
                // v1: skip 2 bytes, then u16 width, u16 height
                let w = u16::from_le_bytes([rec[2], rec[3]]) as f64;
                let h = u16::from_le_bytes([rec[4], rec[5]]) as f64;
                image_width_inches = Some(w / 1200.0);
                image_height_inches = Some(h / 1200.0);
            } else if ver == 2 && rec.len() >= 21 {
                // v2: xres(u16), yres(u16), precision(u8), then coordinates
                let xres = u16::from_le_bytes([rec[0], rec[1]]) as f64;
                let yres = u16::from_le_bytes([rec[2], rec[3]]) as f64;
                let precision = rec[4];
                let (x1, y1, x2, y2) = if precision == 0 && rec.len() >= 21 {
                    // int16s x4 at offset 13
                    let x1 = i16::from_le_bytes([rec[13], rec[14]]) as f64;
                    let y1 = i16::from_le_bytes([rec[15], rec[16]]) as f64;
                    let x2 = i16::from_le_bytes([rec[17], rec[18]]) as f64;
                    let y2 = i16::from_le_bytes([rec[19], rec[20]]) as f64;
                    (x1, y1, x2, y2)
                } else if precision == 1 && rec.len() >= 29 {
                    // int32s x4 at offset 13
                    let x1 = i32::from_le_bytes([rec[13], rec[14], rec[15], rec[16]]) as f64;
                    let y1 = i32::from_le_bytes([rec[17], rec[18], rec[19], rec[20]]) as f64;
                    let x2 = i32::from_le_bytes([rec[21], rec[22], rec[23], rec[24]]) as f64;
                    let y2 = i32::from_le_bytes([rec[25], rec[26], rec[27], rec[28]]) as f64;
                    (x1, y1, x2, y2)
                } else {
                    pos += 0; // skip
                    // Emit last_type
                    if let Some(lt) = last_type.take() {
                        let val = if count > 1 { format!("{}x{}", lt, count) } else { format!("{}", lt) };
                        records.push(format_wpg_record(lt, count, if ver == 1 { &v1_map } else { &v2_map }));
                    }
                    last_type = Some(record_type);
                    count = 1;
                    continue;
                };
                let w = (x2 - x1).abs();
                let h = (y2 - y1).abs();
                let xres_div = if xres == 0.0 { 1200.0 } else { xres };
                let yres_div = if yres == 0.0 { 1200.0 } else { yres };
                image_width_inches = Some(w / xres_div);
                image_height_inches = Some(h / yres_div);
            }
        } else {
            pos += len;
        }

        // Accumulate records (collapse sequential identical types)
        if last_type == Some(record_type) {
            count += 1;
        } else {
            if let Some(lt) = last_type.take() {
                records.push(format_wpg_record(lt, count, if ver == 1 { &v1_map } else { &v2_map }));
            }
            if record_type == 0 && ver == 2 { break; } // End Marker
            last_type = Some(record_type);
            count = 1;
        }
    }
    // Emit last record
    if let Some(lt) = last_type.take() {
        records.push(format_wpg_record(lt, count, if ver == 1 { &v1_map } else { &v2_map }));
    }

    if let Some(w) = image_width_inches {
        tags.push(mktag("WPG", "ImageWidthInches", "Image Width Inches", Value::String(format!("{:.2}", w))));
    }
    if let Some(h) = image_height_inches {
        tags.push(mktag("WPG", "ImageHeightInches", "Image Height Inches", Value::String(format!("{:.2}", h))));
    }
    if !records.is_empty() {
        let joined = records.join(", ");
        tags.push(mktag("WPG", "Records", "Records", Value::String(joined)));
    }

    Ok(tags)
}

fn format_wpg_record(rtype: u32, count: usize, map: &std::collections::HashMap<u32, &str>) -> String {
    let name = map.get(&rtype)
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("Unknown (0x{:02x})", rtype));
    if count > 1 {
        format!("{} x {}", name, count)
    } else {
        name
    }
}

fn read_wpg_varint(data: &[u8], pos: usize) -> (usize, usize) {
    if pos >= data.len() { return (0, 0); }
    let first = data[pos] as usize;
    if first != 0xFF {
        return (first, 1);
    }
    // 0xFF → read 2 more bytes as u16 LE
    if pos + 2 >= data.len() { return (0, 1); }
    let val = u16::from_le_bytes([data[pos + 1], data[pos + 2]]) as usize;
    if val & 0x8000 != 0 {
        // Read 2 more bytes
        if pos + 4 >= data.len() { return (val & 0x7FFF, 3); }
        let hi = u16::from_le_bytes([data[pos + 3], data[pos + 4]]) as usize;
        let full = ((val & 0x7FFF) << 16) | hi;
        return (full, 5);
    }
    (val, 3)
}

// ============================================================================
// Real Media Metafile (RAM/RPM)
// ============================================================================

pub fn read_ram(data: &[u8]) -> Result<Vec<Tag>> {
    // RAM files are text files with URLs, one per line
    // Must start with a valid URL or protocol
    if data.len() < 4 {
        return Err(Error::InvalidData("not a RAM file".into()));
    }

    let text = String::from_utf8_lossy(data);
    // Check for valid start: must begin with a URL-like protocol
    let first_line = text.lines().next().unwrap_or("").trim();
    // Validate: http:// lines must end with real media extensions
    let valid_protocols = ["rtsp://", "pnm://", "http://", "rtspt://", "rtspu://", "mmst://", "file://"];
    let has_valid = text.lines().any(|line| {
        let l = line.trim();
        valid_protocols.iter().any(|p| l.starts_with(p))
    });
    if !has_valid && !text.starts_with(".RMF") && !data.starts_with(b".ra\xfd") {
        return Err(Error::InvalidData("not a Real RAM file".into()));
    }

    let mut tags = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() { continue; }
        // Validate http:// URLs
        if line.starts_with("http://") {
            if !line.ends_with(".ra") && !line.ends_with(".rm") && !line.ends_with(".rv")
                && !line.ends_with(".rmvb") && !line.ends_with(".smil") {
                continue;
            }
        }
        if valid_protocols.iter().any(|p| line.starts_with(p)) {
            tags.push(mktag("Real", "URL", "URL", Value::String(line.into())));
        }
    }

    Ok(tags)
}

/// Parse DSS (Digital Speech Standard) voice recorder files.
/// Mirrors ExifTool's Olympus::ProcessDSS().
pub fn read_dss(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 68 {
        return Err(Error::InvalidData("DSS file too small".into()));
    }
    // Magic: \x02dss or \x03ds2
    if !(data[0] == 0x02 || data[0] == 0x03)
        || data[1] != b'd'
        || data[2] != b's'
        || (data[3] != b's' && data[3] != b'2')
    {
        return Err(Error::InvalidData("not a DSS/DS2 file".into()));
    }

    let mut tags = Vec::new();

    // Offset 12: Model, string[16]
    if data.len() >= 28 {
        let model_bytes = &data[12..28];
        let model = String::from_utf8_lossy(model_bytes)
            .trim_end_matches('\0')
            .trim()
            .to_string();
        if !model.is_empty() {
            tags.push(mktag("Olympus", "Model", "Camera Model Name", Value::String(model)));
        }
    }

    // Offset 38: StartTime, string[12] — format YYMMDDHHMMSS
    if data.len() >= 50 {
        let st_bytes = &data[38..50];
        let st_str = String::from_utf8_lossy(st_bytes);
        if let Some(dt) = parse_dss_time(&st_str) {
            tags.push(mktag("Olympus", "StartTime", "Start Time", Value::String(dt)));
        }
    }

    // Offset 50: EndTime, string[12]
    if data.len() >= 62 {
        let et_bytes = &data[50..62];
        let et_str = String::from_utf8_lossy(et_bytes);
        if let Some(dt) = parse_dss_time(&et_str) {
            tags.push(mktag("Olympus", "EndTime", "End Time", Value::String(dt)));
        }
    }

    // Offset 62: Duration, string[6] — format HHMMSS
    if data.len() >= 68 {
        let dur_bytes = &data[62..68];
        let dur_str = String::from_utf8_lossy(dur_bytes);
        if let Some(dur_secs) = parse_dss_duration(&dur_str) {
            let dur_display = dss_convert_duration(dur_secs);
            tags.push(mktag("Olympus", "Duration", "Duration", Value::String(dur_display)));
        }
    }

    Ok(tags)
}

/// Parse DSS time string YYMMDDHHMMSS → "20YY:MM:DD HH:MM:SS"
fn parse_dss_time(s: &str) -> Option<String> {
    let s = s.trim_matches('\0');
    if s.len() < 12 {
        return None;
    }
    let yy = &s[0..2];
    let mm = &s[2..4];
    let dd = &s[4..6];
    let hh = &s[6..8];
    let mi = &s[8..10];
    let ss = &s[10..12];
    // Validate digits
    if !yy.chars().all(|c| c.is_ascii_digit()) { return None; }
    Some(format!("20{}:{}:{} {}:{}:{}", yy, mm, dd, hh, mi, ss))
}

/// Parse DSS duration string HHMMSS → seconds
fn parse_dss_duration(s: &str) -> Option<f64> {
    let s = s.trim_matches('\0');
    if s.len() < 6 { return None; }
    let hh: u64 = s[0..2].parse().ok()?;
    let mm: u64 = s[2..4].parse().ok()?;
    let ss: u64 = s[4..6].parse().ok()?;
    Some(((hh * 60 + mm) * 60 + ss) as f64)
}

/// Convert duration in seconds to display string (mirrors ExifTool's ConvertDuration).
fn dss_convert_duration(secs: f64) -> String {
    if secs == 0.0 {
        return "0 s".to_string();
    }
    if secs < 30.0 {
        return format!("{:.2} s", secs);
    }
    let secs_u = (secs + 0.5) as u64;
    let h = secs_u / 3600;
    let m = (secs_u % 3600) / 60;
    let s = secs_u % 60;
    format!("{}:{:02}:{:02}", h, m, s)
}

// ============================================================================
// Helpers
// ============================================================================

fn mktag(family: &str, name: &str, description: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: family.into(),
            family1: family.into(),
            family2: "Other".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}

// ============================================================================
// InDesign format reader
// Extracts XMP metadata from Adobe InDesign (.indd) files.
// ============================================================================

pub fn read_indesign(data: &[u8]) -> Result<Vec<Tag>> {
    // InDesign master page GUID: 06 06 ED F5 D8 1D 46 E5 BD 31 EF E7 FE 74 B7 1D
    let master_guid = &[0x06u8, 0x06, 0xED, 0xF5, 0xD8, 0x1D, 0x46, 0xE5,
                         0xBD, 0x31, 0xEF, 0xE7, 0xFE, 0x74, 0xB7, 0x1D];
    let object_header_guid = &[0xDE, 0x39, 0x39, 0x79, 0x51, 0x88, 0x4B, 0x6C,
                                 0x8E, 0x63, 0xEE, 0xF8, 0xAE, 0xE0, 0xDD, 0x38];

    if data.len() < 4096 || !data.starts_with(master_guid) {
        return Err(crate::error::Error::InvalidData("not an InDesign file".into()));
    }

    // Read two master pages (each 4096 bytes) and pick the most current one
    if data.len() < 8192 {
        return Ok(vec![]);
    }

    let page1 = &data[..4096];
    let page2 = &data[4096..8192];

    // Master pages always use LE byte order ('II')
    // Determine current master page (highest sequence number wins)
    let cur_page = {
        let seq1 = u64::from_le_bytes(page1[264..272].try_into().unwrap_or([0;8]));
        let seq2 = if page2.starts_with(master_guid) {
            u64::from_le_bytes(page2[264..272].try_into().unwrap_or([0;8]))
        } else { 0 };
        if seq2 > seq1 { page2 } else { page1 }
    };

    // Stream byte order is at offset 24 of current master page: 1 = LE, 2 = BE
    let stream_is_le = cur_page[24] == 1;

    // Number of pages (determines start of stream objects) - master page is LE
    let pages = u32::from_le_bytes(cur_page[280..284].try_into().unwrap_or([0;4]));
    let start_pos = (pages as usize) * 4096;
    if start_pos >= data.len() {
        return Ok(vec![]);
    }

    // Scan contiguous objects for XMP
    // Object header GUID (16 bytes) + additional header data (16 bytes) = 32 bytes total
    let mut pos = start_pos;
    while pos + 32 <= data.len() {
        if &data[pos..pos+16] != object_header_guid {
            break;
        }
        // Object (stream) length at offset 24 in the 32-byte object header
        // The object header itself appears to always use LE byte order
        let obj_len = u32::from_le_bytes(
            data[pos+24..pos+28].try_into().unwrap_or([0;4])
        ) as usize;

        pos += 32;
        if obj_len == 0 || pos + obj_len > data.len() { break; }

        let obj_data = &data[pos..pos + obj_len];

        // XMP stream: 4-byte length prefix followed by XMP data
        // The actual XMP starts at offset 0 or 4 depending on encoding
        if obj_len > 56 {
            if let Some(xp_pos) = find_xpacket(obj_data) {
                let xmp_data = &obj_data[xp_pos..];
                if let Ok(xmp_tags) = crate::metadata::XmpReader::read(xmp_data) {
                    return Ok(xmp_tags);
                }
            }
        }

        pos += obj_len;
    }

    Ok(vec![])
}

fn find_xpacket(data: &[u8]) -> Option<usize> {
    // Look for "<?xpacket begin=" or "<x:xmpmeta"
    for i in 0..data.len().saturating_sub(10) {
        if data[i..].starts_with(b"<?xpacket") || data[i..].starts_with(b"<x:xmpmeta") {
            return Some(i);
        }
    }
    None
}

// ============================================================================
// PCAP (packet capture) format reader
// ============================================================================

pub fn read_pcap(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 24 {
        return Err(crate::error::Error::InvalidData("not a PCAP file".into()));
    }

    let is_le = data[0] == 0xD4 && data[1] == 0xC3;
    let r16 = |d: &[u8], o: usize| -> u16 {
        if o+2 > d.len() { return 0; }
        if is_le { u16::from_le_bytes([d[o], d[o+1]]) }
        else { u16::from_be_bytes([d[o], d[o+1]]) }
    };
    let r32 = |d: &[u8], o: usize| -> u32 {
        if o+4 > d.len() { return 0; }
        if is_le { u32::from_le_bytes([d[o], d[o+1], d[o+2], d[o+3]]) }
        else { u32::from_be_bytes([d[o], d[o+1], d[o+2], d[o+3]]) }
    };

    let maj = r16(data, 4);
    let min = r16(data, 6);
    let link_type = r32(data, 20);

    let mut tags = Vec::new();
    let bo_str = if is_le { "Little-endian (Intel, II)" } else { "Big-endian (Motorola, MM)" };
    tags.push(mktag("PCAP", "ByteOrder", "Byte Order", Value::String(bo_str.into())));
    tags.push(mktag("PCAP", "PCAPVersion", "PCAP Version",
        Value::String(format!("PCAP {}.{}", maj, min))));
    tags.push(mktag("PCAP", "LinkType", "Link Type",
        Value::String(pcap_link_type_name(link_type))));

    Ok(tags)
}

// ============================================================================
// PCAPNG (pcap next generation) format reader
// ============================================================================

pub fn read_pcapng(data: &[u8]) -> Result<Vec<Tag>> {
    // Section Header Block: 0x0A0D0D0A
    if data.len() < 28 || data[0] != 0x0A || data[1] != 0x0D || data[2] != 0x0D || data[3] != 0x0A {
        return Err(crate::error::Error::InvalidData("not a PCAPNG file".into()));
    }

    // Block length at offset 4 (4 bytes)
    // Byte order magic at offset 8: 0x1A2B3C4D (LE) or 0x4D3C2B1A (BE)
    let bo_magic_le = data.len() >= 12 &&
        data[8] == 0x4D && data[9] == 0x3C && data[10] == 0x2B && data[11] == 0x1A;
    let is_le = bo_magic_le;

    let r16 = |d: &[u8], o: usize| -> u16 {
        if o+2 > d.len() { return 0; }
        if is_le { u16::from_le_bytes([d[o], d[o+1]]) }
        else { u16::from_be_bytes([d[o], d[o+1]]) }
    };
    let r32 = |d: &[u8], o: usize| -> u32 {
        if o+4 > d.len() { return 0; }
        if is_le { u32::from_le_bytes([d[o], d[o+1], d[o+2], d[o+3]]) }
        else { u32::from_be_bytes([d[o], d[o+1], d[o+2], d[o+3]]) }
    };

    let maj = r16(data, 12);
    let min = r16(data, 14);
    let blk_len = r32(data, 4) as usize;

    let mut tags = Vec::new();
    let bo_str = if is_le { "Little-endian (Intel, II)" } else { "Big-endian (Motorola, MM)" };
    tags.push(mktag("PCAP", "ByteOrder", "Byte Order", Value::String(bo_str.into())));
    tags.push(mktag("PCAP", "PCAPVersion", "PCAP Version",
        Value::String(format!("PCAPNG {}.{}", maj, min))));

    // SHB structure: block_type(4) + block_len(4) + bo_magic(4) + major(2) + minor(2) + section_len(8)
    // Options start at offset 24 (after the 8-byte section_length field)
    let opt_start = 24usize;
    let opt_end = if blk_len > 4 && blk_len <= data.len() { blk_len - 4 } else { data.len() };
    parse_pcapng_options(data, opt_start, opt_end, is_le, &mut tags, "shb");

    // Parse Interface Description Block (IDB) right after the SHB
    let idb_start = if blk_len < data.len() { blk_len } else { return Ok(tags); };
    if idb_start + 20 <= data.len() {
        let idb_type = r32(data, idb_start);
        if idb_type == 1 {
            // IDB: block type(4) + block_len(4) + link_type(2) + reserved(2) + snap_len(4) = 16 bytes
            let idb_len = r32(data, idb_start + 4) as usize;
            let link_type = r32(data, idb_start + 8) & 0xFFFF;
            let link_name = pcap_link_type_name(link_type);
            tags.push(mktag("PCAP", "LinkType", "Link Type", Value::String(link_name)));

            // Parse IDB options (starting at offset idb_start + 16)
            let idb_opt_start = idb_start + 16;
            let idb_opt_end = if idb_start + idb_len > 4 && idb_start + idb_len <= data.len() {
                idb_start + idb_len - 4
            } else { data.len() };
            parse_pcapng_options(data, idb_opt_start, idb_opt_end, is_le, &mut tags, "idb");

            // Parse EPB/SPB blocks to find TimeStamp
            let epb_start = idb_start + idb_len;
            parse_pcapng_blocks(data, epb_start, is_le, &mut tags);
        }
    }

    Ok(tags)
}

fn parse_pcapng_options(data: &[u8], start: usize, end: usize, is_le: bool,
                         tags: &mut Vec<Tag>, ctx: &str) {
    let r16 = |d: &[u8], o: usize| -> u16 {
        if o+2 > d.len() { return 0; }
        if is_le { u16::from_le_bytes([d[o], d[o+1]]) }
        else { u16::from_be_bytes([d[o], d[o+1]]) }
    };

    let mut pos = start;
    while pos + 4 <= end.min(data.len()) {
        let opt_code = r16(data, pos);
        let opt_len = r16(data, pos + 2) as usize;
        pos += 4;
        if opt_code == 0 { break; } // opt_endofopt
        let padded_len = (opt_len + 3) & !3;
        if pos + opt_len > data.len() { break; }

        let opt_data = &data[pos..pos + opt_len];

        match (ctx, opt_code) {
            ("shb", 2) => { // shb_hardware
                let s = String::from_utf8_lossy(opt_data).to_string();
                tags.push(mktag("PCAP", "Hardware", "Hardware", Value::String(s)));
            }
            ("shb", 3) => { // shb_os
                let s = String::from_utf8_lossy(opt_data).to_string();
                tags.push(mktag("PCAP", "OperatingSystem", "Operating System", Value::String(s)));
            }
            ("shb", 4) => { // shb_userappl
                let s = String::from_utf8_lossy(opt_data).to_string();
                tags.push(mktag("PCAP", "UserApplication", "User Application", Value::String(s)));
            }
            ("idb", 2) => { // if_name
                let s = String::from_utf8_lossy(opt_data).to_string();
                tags.push(mktag("PCAP", "DeviceName", "Device Name", Value::String(s)));
            }
            ("idb", 9) => { // if_tsresol: timestamp resolution
                if opt_len >= 1 {
                    let tsresol = opt_data[0];
                    let resolution = if tsresol & 0x80 != 0 {
                        // Power of 2
                        let exp = tsresol & 0x7F;
                        format!("2^-{}", exp)
                    } else {
                        // Power of 10
                        let exp = tsresol & 0x7F;
                        format!("1e-{:02}", exp)
                    };
                    tags.push(mktag("PCAP", "TimeStampResolution", "Time Stamp Resolution",
                        Value::String(resolution)));
                }
            }
            ("idb", 12) => { // if_os
                let s = String::from_utf8_lossy(opt_data).to_string();
                if !tags.iter().any(|t| t.name == "OperatingSystem") {
                    tags.push(mktag("PCAP", "OperatingSystem", "Operating System", Value::String(s)));
                }
            }
            _ => {}
        }

        pos += padded_len;
    }
}

fn parse_pcapng_blocks(data: &[u8], start: usize, is_le: bool, tags: &mut Vec<Tag>) {
    let r32 = |d: &[u8], o: usize| -> u32 {
        if o+4 > d.len() { return 0; }
        if is_le { u32::from_le_bytes([d[o], d[o+1], d[o+2], d[o+3]]) }
        else { u32::from_be_bytes([d[o], d[o+1], d[o+2], d[o+3]]) }
    };
    let r64 = |d: &[u8], o: usize| -> u64 {
        if o+8 > d.len() { return 0; }
        if is_le { u64::from_le_bytes([d[o], d[o+1], d[o+2], d[o+3], d[o+4], d[o+5], d[o+6], d[o+7]]) }
        else { u64::from_be_bytes([d[o], d[o+1], d[o+2], d[o+3], d[o+4], d[o+5], d[o+6], d[o+7]]) }
    };

    let mut pos = start;
    while pos + 8 <= data.len() {
        let block_type = r32(data, pos);
        let block_len = r32(data, pos + 4) as usize;
        if block_len < 12 || pos + block_len > data.len() { break; }

        // EPB (Enhanced Packet Block) = type 6
        if block_type == 6 && block_len >= 28 {
            let ts_hi = r32(data, pos + 12) as u64;
            let ts_lo = r32(data, pos + 16) as u64;
            let ts_raw = (ts_hi << 32) | ts_lo;
            // Default resolution is 1e-6 (microseconds)
            let ts_secs = ts_raw / 1_000_000;
            let ts_usecs = ts_raw % 1_000_000;
            // Format as ExifTool does: YYYY:MM:DD HH:MM:SS.ssssss+ZZ:ZZ
            if let Some(dt) = format_unix_timestamp(ts_secs as i64, ts_usecs) {
                tags.push(mktag("PCAP", "TimeStamp", "Time Stamp", Value::String(dt)));
            }
            break; // Only need first packet timestamp
        }

        pos += block_len;
    }
}

fn format_unix_timestamp(secs: i64, usecs: u64) -> Option<String> {
    // Simple Unix timestamp to datetime conversion
    // This is a basic implementation - timezone from local offset
    // For now, use UTC + known local offset from Perl output
    // Perl shows: 2020:10:13 16:12:07.025764+02:00
    // We'll use UTC for simplicity but format it correctly
    use std::time::{SystemTime, UNIX_EPOCH};

    // Get local timezone offset using system time
    let tz_offset_secs = get_local_tz_offset();

    let adjusted = secs + tz_offset_secs as i64;

    // Compute Y/M/D H:M:S from Unix timestamp
    let (y, mo, d, h, mi, s) = unix_to_datetime(adjusted);
    let tz_hours = tz_offset_secs / 3600;
    let tz_mins = (tz_offset_secs.abs() % 3600) / 60;
    let tz_sign = if tz_offset_secs >= 0 { '+' } else { '-' };

    Some(format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}.{:06}{}{:02}:{:02}",
        y, mo, d, h, mi, s, usecs, tz_sign, tz_hours.abs(), tz_mins))
}

fn get_local_tz_offset() -> i32 {
    // Try to get timezone offset from system
    // This uses a simple method: compare local time to UTC
    use std::time::{SystemTime, UNIX_EPOCH};
    // For now return 0 (UTC) - the test data shows +02:00 but we can't easily detect this
    // without platform-specific code
    0
}

fn unix_to_datetime(secs: i64) -> (i32, u32, u32, u32, u32, u32) {
    // Basic implementation of Unix timestamp to calendar date
    const SECS_PER_DAY: i64 = 86400;
    const DAYS_PER_400Y: i64 = 146097;
    const DAYS_PER_100Y: i64 = 36524;
    const DAYS_PER_4Y: i64 = 1461;
    const DAYS_PER_Y: i64 = 365;

    let (days, rem) = if secs >= 0 {
        (secs / SECS_PER_DAY, secs % SECS_PER_DAY)
    } else {
        let d = (secs + 1) / SECS_PER_DAY - 1;
        let r = secs - d * SECS_PER_DAY;
        (d, r)
    };

    let h = (rem / 3600) as u32;
    let m = ((rem % 3600) / 60) as u32;
    let s = (rem % 60) as u32;

    // Days since 1970-01-01
    // Adjust to days since 2000-03-01 for easier calculation
    let z = days + 719468; // days from 0000-03-01
    let era = if z >= 0 { z } else { z - DAYS_PER_400Y + 1 } / DAYS_PER_400Y;
    let doe = z - era * DAYS_PER_400Y;
    let yoe = (doe - doe/1460 + doe/36524 - doe/146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365*yoe + yoe/4 - yoe/100);
    let mp = (5*doy + 2)/153;
    let d = doy - (153*mp+2)/5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };

    (y as i32, mo as u32, d as u32, h, m, s)
}

fn pcap_link_type_name(link_type: u32) -> String {
    match link_type {
        0 => "BSD Loopback".into(),
        1 => "IEEE 802.3 Ethernet".into(),
        9 => "PPP".into(),
        105 => "IEEE 802.11".into(),
        108 => "OpenBSD Loopback".into(),
        113 => "Linux SLL".into(),
        127 => "IEEE 802.11 Radiotap".into(),
        _ => format!("{}", link_type),
    }
}
