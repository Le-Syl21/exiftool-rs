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
    let file_length = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    tags.push(mktag("SWF", "FlashVersion", "Flash Version", Value::U8(version)));
    tags.push(mktag("SWF", "Compressed", "Compressed", Value::String(if compressed { "Yes" } else { "No" }.into())));
    tags.push(mktag("SWF", "UncompressedSize", "Uncompressed Size", Value::U32(file_length)));

    Ok(tags)
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
    let _version = data[1];
    let _encoding = data[2];
    let bpp = data[3];
    let xmin = u16::from_le_bytes([data[4], data[5]]);
    let ymin = u16::from_le_bytes([data[6], data[7]]);
    let xmax = u16::from_le_bytes([data[8], data[9]]);
    let ymax = u16::from_le_bytes([data[10], data[11]]);
    let hdpi = u16::from_le_bytes([data[12], data[13]]);
    let vdpi = u16::from_le_bytes([data[14], data[15]]);
    let num_planes = data[65];

    tags.push(mktag("PCX", "ImageWidth", "Image Width", Value::U16(xmax - xmin + 1)));
    tags.push(mktag("PCX", "ImageHeight", "Image Height", Value::U16(ymax - ymin + 1)));
    tags.push(mktag("PCX", "BitsPerPixel", "Bits Per Pixel", Value::U8(bpp)));
    tags.push(mktag("PCX", "NumPlanes", "Color Planes", Value::U8(num_planes)));
    tags.push(mktag("PCX", "XResolution", "X Resolution", Value::U16(hdpi)));
    tags.push(mktag("PCX", "YResolution", "Y Resolution", Value::U16(vdpi)));

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
    if data.len() < 10 || data[0] != 0x1F || data[1] != 0x8B {
        return Err(Error::InvalidData("not a GZIP file".into()));
    }

    let mut tags = Vec::new();
    let method = data[2];
    let flags = data[3];

    tags.push(mktag("GZIP", "CompressionMethod", "Compression", Value::String(
        if method == 8 { "Deflate" } else { "Unknown" }.into()
    )));

    // Modification time (bytes 4-7, Unix timestamp)
    let mtime = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    if mtime > 0 {
        tags.push(mktag("GZIP", "ModifyDate", "Modify Date", Value::U32(mtime)));
    }

    // OS (byte 9)
    let os = match data[9] {
        0 => "FAT/DOS",
        3 => "Unix",
        7 => "Macintosh",
        10 => "NTFS",
        255 => "Unknown",
        _ => "Other",
    };
    tags.push(mktag("GZIP", "OperatingSystem", "Operating System", Value::String(os.into())));

    // Filename (if FNAME flag set)
    if flags & 0x08 != 0 {
        let mut pos = 10;
        // Skip FEXTRA if present
        if flags & 0x04 != 0 && pos + 2 <= data.len() {
            let xlen = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
            pos += 2 + xlen;
        }
        if pos < data.len() {
            let name_end = data[pos..].iter().position(|&b| b == 0).unwrap_or(0);
            let filename = String::from_utf8_lossy(&data[pos..pos + name_end]).to_string();
            if !filename.is_empty() {
                tags.push(mktag("GZIP", "ArchivedFileName", "Archived File Name", Value::String(filename)));
            }
        }
    }

    Ok(tags)
}

// ============================================================================
// RAR
// ============================================================================

pub fn read_rar(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 7 || !data.starts_with(b"Rar!\x1A\x07") {
        return Err(Error::InvalidData("not a RAR file".into()));
    }

    let mut tags = Vec::new();
    let version = if data.len() >= 8 && data[6] == 0x01 && data[7] == 0x00 {
        "5.0+"
    } else {
        "4.x"
    };
    tags.push(mktag("RAR", "RARVersion", "RAR Version", Value::String(version.into())));

    Ok(tags)
}

// ============================================================================
// SVG (via XMP)
// ============================================================================

pub fn read_svg(data: &[u8]) -> Result<Vec<Tag>> {
    let text = String::from_utf8_lossy(&data[..data.len().min(8192)]);

    if !text.contains("<svg") {
        return Err(Error::InvalidData("not an SVG file".into()));
    }

    let mut tags = Vec::new();

    // Extract SVG attributes
    if let Some(svg_pos) = text.find("<svg") {
        let rest = &text[svg_pos..];
        if let Some(end) = rest.find('>') {
            let svg_tag = &rest[..end];
            if let Some(w) = extract_xml_attr(svg_tag, "width") {
                tags.push(mktag("SVG", "ImageWidth", "Image Width", Value::String(w)));
            }
            if let Some(h) = extract_xml_attr(svg_tag, "height") {
                tags.push(mktag("SVG", "ImageHeight", "Image Height", Value::String(h)));
            }
            if let Some(vb) = extract_xml_attr(svg_tag, "viewBox") {
                tags.push(mktag("SVG", "ViewBox", "View Box", Value::String(vb)));
            }
        }
    }

    // Try XMP
    if let Ok(xmp_tags) = XmpReader::read(data) {
        tags.extend(xmp_tags);
    }

    Ok(tags)
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
    tags.push(mktag("JSON", "JSONType", "JSON Type", Value::String(
        if trimmed.starts_with('{') { "Object" } else { "Array" }.into()
    )));

    Ok(tags)
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
