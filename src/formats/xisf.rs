//! XISF (Extensible Image Serialization Format) reader.
//!
//! Parses PixInsight XISF files.
//! Mirrors ExifTool's XISF.pm.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_xisf(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 16 || !data.starts_with(b"XISF0100") {
        return Err(Error::InvalidData("not an XISF file".into()));
    }

    let mut tags = Vec::new();

    // Header length at offset 8 (little-endian uint32)
    let hdr_len = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize;

    if 16 + hdr_len > data.len() {
        return Err(Error::InvalidData("truncated XISF header".into()));
    }

    let xml_data = &data[16..16 + hdr_len];
    let xml_str = String::from_utf8_lossy(xml_data);

    // Store the raw XML as binary (matches ExifTool behavior)
    tags.push(mk("XML", "XML", Value::Binary(xml_data.to_vec())));

    // Parse XML attributes to extract metadata
    parse_xisf_xml(&xml_str, &mut tags);

    Ok(tags)
}

fn parse_xisf_xml(xml: &str, tags: &mut Vec<Tag>) {
    // Extract Image element attributes
    // Find the <Image ...> tag (may span to closing > with child elements after it)
    if let Some(img_start) = xml.find("<Image ") {
        let img_section = &xml[img_start..];
        let img_end = img_section.find('>').unwrap_or(img_section.len());
        let img_attrs = &img_section[7..img_end]; // skip "<Image "

        // Parse geometry="W:H:N"
        if let Some(geo) = extract_attr(img_attrs, "geometry") {
            let parts: Vec<&str> = geo.split(':').collect();
            if parts.len() >= 2 {
                if let Ok(w) = parts[0].parse::<u32>() {
                    tags.push(mk("ImageWidth", "Image Width", Value::U32(w)));
                }
                if let Ok(h) = parts[1].parse::<u32>() {
                    tags.push(mk("ImageHeight", "Image Height", Value::U32(h)));
                }
                if parts.len() >= 3 {
                    if let Ok(n) = parts[2].parse::<u32>() {
                        tags.push(mk("NumPlanes", "Number of Planes", Value::U32(n)));
                    }
                }
                tags.push(mk("ImageGeometry", "Image Geometry", Value::String(geo)));
            }
        }

        if let Some(v) = extract_attr(img_attrs, "sampleFormat") {
            tags.push(mk("ImageSampleFormat", "Image Sample Format", Value::String(v)));
        }
        if let Some(v) = extract_attr(img_attrs, "colorSpace") {
            tags.push(mk("ColorSpace", "Color Space", Value::String(v)));
        }
        if let Some(v) = extract_attr(img_attrs, "location") {
            tags.push(mk("ImageLocation", "Image Location", Value::String(v)));
        }

        // Parse <Data> child element within Image block
        // Find Image end tag or next major element
        let img_block_end = img_section.find("</Image>")
            .or_else(|| img_section.find("<Metadata"))
            .unwrap_or(img_section.len());
        let img_block = &img_section[..img_block_end];

        // <Data compression="zlib:65536" encoding="base64">...</Data>
        if let Some(data_start) = img_block.find("<Data ").or_else(|| img_block.find("<Data>")) {
            let data_section = &img_block[data_start..];
            let data_tag_end = data_section.find('>').unwrap_or(data_section.len());
            let data_attrs = &data_section[5..data_tag_end]; // skip "<Data"

            if let Some(v) = extract_attr(data_attrs, "compression") {
                tags.push(mk("ImageDataCompression", "Image Data Compression", Value::String(v)));
            }
            if let Some(v) = extract_attr(data_attrs, "encoding") {
                tags.push(mk("ImageDataEncoding", "Image Data Encoding", Value::String(v)));
            }

            // Store binary image data
            if let Some(text_start) = data_section.find('>') {
                let after = &data_section[text_start + 1..];
                if let Some(text_end) = after.find('<') {
                    let b64 = after[..text_end].trim();
                    if !b64.is_empty() {
                        tags.push(mk("ImageData", "Image Data", Value::Binary(b64.as_bytes().to_vec())));
                    }
                }
            }
        }

        // <Resolution horizontal="72" vertical="72" unit="inch"/>
        if let Some(res_start) = img_block.find("<Resolution ") {
            let res_section = &img_block[res_start..];
            let res_end = res_section.find("/>")
                .or_else(|| res_section.find('>'))
                .unwrap_or(res_section.len());
            let res_attrs = &res_section[12..res_end]; // skip "<Resolution "

            if let Some(h) = extract_attr(res_attrs, "horizontal") {
                tags.push(mk("XResolution", "X Resolution", Value::String(h)));
            }
            if let Some(v) = extract_attr(res_attrs, "vertical") {
                tags.push(mk("YResolution", "Y Resolution", Value::String(v)));
            }
            if let Some(u) = extract_attr(res_attrs, "unit") {
                tags.push(mk("ResolutionUnit", "Resolution Unit", Value::String(u)));
            }
        }
    }

    // Extract Metadata element properties
    // <Metadata>...</Metadata>
    let meta_start_opt = xml.find("<Metadata>").or_else(|| xml.find("<Metadata "));
    if let Some(meta_start) = meta_start_opt {
        let meta_section = &xml[meta_start..];
        let meta_end = meta_section.find("</Metadata>").map(|e| e + 11).unwrap_or(meta_section.len());
        parse_property_elements(&meta_section[..meta_end], tags);
    }
}

fn parse_property_elements(xml: &str, tags: &mut Vec<Tag>) {
    // <Property id="..." type="..." value="..."/> or <Property ...>text</Property>
    let mut search = xml;
    while let Some(p) = search.find("<Property ") {
        let section = &search[p..];
        let end = section.find("/>").map(|e| e + 2)
            .or_else(|| section.find("</Property>").map(|e| e + 11))
            .unwrap_or(section.len());
        let inner = &section[10..end]; // skip "<Property "

        if let Some(id) = extract_attr(inner, "id") {
            // Remove XISF: namespace prefix
            let bare_id = id.trim_start_matches("XISF:");
            let val = extract_attr(inner, "value")
                .or_else(|| extract_element_text(inner));
            if let Some(val) = val {
                let tag_name = xisf_property_to_tag(bare_id);
                if !tag_name.is_empty() {
                    // Convert ISO 8601 date to ExifTool format
                    let val = if tag_name == "CreateDate" {
                        iso8601_to_exif(&val)
                    } else {
                        val
                    };
                    tags.push(mk(&tag_name, &tag_name, Value::String(val)));
                }
            }
        }

        let advance = p + end;
        if advance >= search.len() { break; }
        search = &search[advance..];
    }
}

fn iso8601_to_exif(s: &str) -> String {
    // Convert "2019-09-18T22:57:08Z" to "2019:09:18 22:57:08Z"
    // or "2019-09-18T22:57:08+01:00" to "2019:09:18 22:57:08+01:00"
    let s = s.trim();
    if s.len() >= 19 {
        let bytes = s.as_bytes();
        // Check format: YYYY-MM-DDTHH:MM:SS...
        if bytes[4] == b'-' && bytes[7] == b'-' && bytes[10] == b'T' {
            let date_part = format!("{}:{}:{}", &s[0..4], &s[5..7], &s[8..10]);
            let time_part = &s[11..]; // HH:MM:SS...
            return format!("{} {}", date_part, time_part);
        }
    }
    s.to_string()
}

fn xisf_property_to_tag(id: &str) -> String {
    match id {
        "CreationTime" => "CreateDate".into(),
        "CreatorApplication" => "CreatorApplication".into(),
        "CreatorModule" => "CreatorModule".into(),
        "CreatorOS" => "CreatorOS".into(),
        "CompressionLevel" => "CompressionLevel".into(),
        "CompressionCodecs" => "CompressionCodecs".into(),
        "Description" => "Description".into(),
        "Keywords" => "Keywords".into(),
        "Title" => "Title".into(),
        "Copyright" => "Copyright".into(),
        "Authors" => "Authors".into(),
        "BriefDescription" => "BriefDescription".into(),
        "ResolutionUnit" => "ResolutionUnit".into(),
        "XResolution" | "ImageResolutionHorizontal" => "XResolution".into(),
        "YResolution" | "ImageResolutionVertical" => "YResolution".into(),
        _ => String::new(),
    }
}

/// Extract an XML attribute value from an attribute string
fn extract_attr(attrs: &str, name: &str) -> Option<String> {
    // Search for: name="..." or name='...'
    let search_dq = format!("{}=\"", name);
    let search_sq = format!("{}='", name);

    if let Some(p) = attrs.find(&search_dq) {
        let after = p + search_dq.len();
        let rest = &attrs[after..];
        let end = rest.find('"').unwrap_or(rest.len());
        return Some(rest[..end].to_string());
    }
    if let Some(p) = attrs.find(&search_sq) {
        let after = p + search_sq.len();
        let rest = &attrs[after..];
        let end = rest.find('\'').unwrap_or(rest.len());
        return Some(rest[..end].to_string());
    }
    None
}

/// Extract text content from an XML element
fn extract_element_text(xml: &str) -> Option<String> {
    if let Some(start) = xml.find('>') {
        let after = &xml[start + 1..];
        if let Some(end) = after.find('<') {
            let text = after[..end].trim();
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }
    None
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "XML".into(),
            family1: "XML".into(),
            family2: "Image".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}
