//! Radiance HDR format reader.

use super::misc::mktag;
use crate::error::{Error, Result};
use crate::tag::Tag;
use crate::value::Value;

pub fn read_hdr(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 10 || (!data.starts_with(b"#?RADIANCE") && !data.starts_with(b"#?RGBE")) {
        return Err(Error::InvalidData("not a Radiance HDR file".into()));
    }

    let mut tags = Vec::new();
    let text = crate::encoding::decode_utf8_or_latin1(&data[..data.len().min(8192)]);

    // Track key-value pairs and commands (last wins for non-list tags)
    let mut kv_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut last_command: Option<String> = None;
    let mut found_dims = false;

    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        // Skip the magic header line
        if line.starts_with("#?") {
            continue;
        }
        // Comment lines
        if line.starts_with('#') {
            continue;
        }
        // Empty line marks end of header metadata
        if line.is_empty() {
            continue;
        }
        // Dimension line (resolution) - last header line before data
        if line.starts_with("-Y ")
            || line.starts_with("+Y ")
            || line.starts_with("-X ")
            || line.starts_with("+X ")
        {
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
            let val = line[eq_pos + 1..].trim().to_string();
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
        tags.push(mktag(
            "HDR",
            "Exposure",
            "Exposure",
            Value::String(v.clone()),
        ));
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
        tags.push(mktag(
            "HDR",
            "Orientation",
            "Orientation",
            Value::String(v.clone()),
        ));
    }
    if let Some(v) = kv_map.get("Software") {
        tags.push(mktag(
            "HDR",
            "Software",
            "Software",
            Value::String(v.clone()),
        ));
    }
    if let Some(v) = kv_map.get("View") {
        tags.push(mktag("HDR", "View", "View", Value::String(v.clone())));
    }

    let _ = found_dims;
    Ok(tags)
}
