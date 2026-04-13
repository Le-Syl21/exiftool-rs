//! PPM/PGM/PBM (Netpbm) format reader.

use super::misc::mktag;
use crate::error::{Error, Result};
use crate::tag::Tag;
use crate::value::Value;

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
        let text = crate::encoding::decode_utf8_or_latin1(&data[..data.len().min(256)]);
        // Match: P[Ff]\n<width> <height>\n<scale>\n
        let re_str = text.as_str();
        // Simple line-based parser
        let mut lines = re_str.lines();
        let header_line = lines.next().unwrap_or("");
        let cs_char = if header_line.ends_with('F') || header_line == "PF" {
            b'F'
        } else {
            b'f'
        };
        let color_space = if cs_char == b'F' { "RGB" } else { "Monochrome" };
        tags.push(mktag(
            "PFM",
            "ColorSpace",
            "Color Space",
            Value::String(color_space.into()),
        ));

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
                let byte_order = if scale > 0.0 {
                    "Big-endian"
                } else {
                    "Little-endian"
                };
                tags.push(mktag(
                    "PFM",
                    "ByteOrder",
                    "Byte Order",
                    Value::String(byte_order.into()),
                ));
            }
        }
    } else {
        // PPM/PGM/PBM format
        // Parse header: collect comments, then width height [maxval]
        let text = crate::encoding::decode_utf8_or_latin1(&data[2..data.len().min(1024)]);
        let mut comment_lines: Vec<String> = Vec::new();
        let mut width: Option<u32> = None;
        let mut height: Option<u32> = None;
        let mut maxval: Option<u32> = None;
        let mut found_dims = false;

        // State machine: after magic byte, collect comments and parse dimensions
        let mut remaining = text.as_str();
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
            let comment = comment
                .trim_end_matches('\n')
                .trim_end_matches('\r')
                .to_string();
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
