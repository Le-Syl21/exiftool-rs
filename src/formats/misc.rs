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
