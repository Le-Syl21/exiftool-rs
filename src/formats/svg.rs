//! SVG (Scalable Vector Graphics) format reader.

use super::misc::mktag;
use crate::error::{Error, Result};
use crate::metadata::XmpReader;
use crate::tag::Tag;
use crate::value::Value;

pub fn read_svg(data: &[u8]) -> Result<Vec<Tag>> {
    let text = crate::encoding::decode_utf8_or_latin1(data);

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
    let _parser = EventReader::from_str(&text);
    let mut path: Vec<String> = Vec::new(); // element local names (ucfirst)
    let mut current_text = String::new();
    // Which section are we in?
    let mut in_metadata = false; // inside <metadata> element
    let mut in_rdf = 0_usize; // nesting depth inside <rdf:RDF>
    let mut in_c2pa = 0_usize; // nesting depth inside <c2pa:manifest>
    let mut in_svg_body = false; // inside SVG non-metadata body (desc, title, etc.)
                                 // Track whether each path element had child elements (to skip mixed-content text).
                                 // True = had at least one child element. Parallel to `path`.
    let mut had_child: Vec<bool> = Vec::new();

    for event in EventReader::from_str(text.as_str()) {
        match event {
            Ok(XmlEvent::StartElement {
                name,
                attributes,
                namespace,
                ..
            }) => {
                let local = &name.local_name;
                let ns = name.namespace.as_deref().unwrap_or("");

                // Root SVG element
                if local == "svg" && path.is_empty() {
                    path.push("Svg".into());
                    had_child.push(false);
                    for attr in &attributes {
                        match attr.name.local_name.as_str() {
                            "width" => tags.push(mktag(
                                "SVG",
                                "ImageWidth",
                                "Image Width",
                                Value::String(attr.value.clone()),
                            )),
                            "height" => tags.push(mktag(
                                "SVG",
                                "ImageHeight",
                                "Image Height",
                                Value::String(attr.value.clone()),
                            )),
                            "version" => tags.push(mktag(
                                "SVG",
                                "SVGVersion",
                                "SVG Version",
                                Value::String(attr.value.clone()),
                            )),
                            "viewBox" | "viewbox" => tags.push(mktag(
                                "SVG",
                                "ViewBox",
                                "View Box",
                                Value::String(attr.value.clone()),
                            )),
                            "id" => tags.push(mktag(
                                "SVG",
                                "ID",
                                "ID",
                                Value::String(attr.value.clone()),
                            )),
                            _ => {}
                        }
                    }
                    // Extract default namespace (xmlns="...") from the namespace map
                    if let Some(default_ns) = namespace.get("") {
                        if !default_ns.is_empty() {
                            tags.push(mktag(
                                "SVG",
                                "Xmlns",
                                "XMLNS",
                                Value::String(default_ns.to_string()),
                            ));
                        }
                    }
                    current_text.clear();
                    continue;
                }

                // <metadata> block — switch to metadata parsing mode
                if local == "metadata" && !in_metadata && in_rdf == 0 && in_c2pa == 0 {
                    in_metadata = true;
                    // Mark parent as having a child
                    if let Some(last) = had_child.last_mut() {
                        *last = true;
                    }
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
                if !in_metadata && !path.is_empty() {
                    in_svg_body = true;
                    // Mark parent as having a child
                    if let Some(last) = had_child.last_mut() {
                        *last = true;
                    }
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
                        let b64 = current_text
                            .chars()
                            .filter(|c| !c.is_whitespace())
                            .collect::<String>();
                        if !b64.is_empty() {
                            if let Ok(jumbf_data) = base64_decode(&b64) {
                                let jumbf_group = crate::tag::TagGroup {
                                    family0: "JUMBF".into(),
                                    family1: "JUMBF".into(),
                                    family2: "Image".into(),
                                };
                                let print = format!(
                                    "(Binary data {} bytes, use -b option to extract)",
                                    jumbf_data.len()
                                );
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
                    path.pop();
                    had_child.pop();
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
                    let this_had_child = had_child.pop().unwrap_or(false);
                    let t = current_text.trim().to_string();
                    // Only emit if this element has no child elements (pure text node)
                    if !t.is_empty() && !this_had_child {
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
                had_child.pop();
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
                    let print = format!(
                        "(Binary data {} bytes, use -b option to extract)",
                        jumbf_data.len()
                    );
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
    let bytes: Vec<u8> = s
        .bytes()
        .filter(|&b| b != b'=' && b != b'\n' && b != b'\r' && b != b' ')
        .collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        if chunk.len() < 2 {
            break;
        }
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
        let lbox =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let tbox = &data[pos + 4..pos + 8];
        if lbox < 8 || pos + lbox > data.len() {
            break;
        }
        let content = &data[pos + 8..pos + lbox];

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
        let lbox =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let tbox = &data[pos + 4..pos + 8];
        if lbox < 8 || pos + lbox > data.len() {
            break;
        }
        let content = &data[pos + 8..pos + lbox];

        if tbox == b"jumd" && !found_jumd {
            found_jumd = true;
            if content.len() >= 17 {
                let type_bytes = &content[..16];
                let label_data = &content[17..];
                let null_pos = label_data
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(label_data.len());
                let label =
                    crate::encoding::decode_utf8_or_latin1(&label_data[..null_pos]).to_string();

                if emit_desc {
                    // Emit JUMDType
                    let type_hex: String =
                        type_bytes.iter().map(|b| format!("{:02x}", b)).collect();
                    let a1 = &type_hex[8..12];
                    let a2 = &type_hex[12..16];
                    let a3 = &type_hex[16..32];
                    let ascii4 = &type_bytes[..4];
                    let is_printable = ascii4.iter().all(|&b| b.is_ascii_alphanumeric());
                    let print_type = if is_printable {
                        let ascii_str = crate::encoding::decode_utf8_or_latin1(ascii4);
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
            while i < bytes.len() && bytes[i] != b'"' {
                i += 1;
            }
            let key = &json[key_start..i];
            i += 1; // skip closing "
                    // Skip whitespace and colon
            while i < bytes.len() && (bytes[i] == b':' || bytes[i] == b' ') {
                i += 1;
            }
            // Read value if it's a string
            if i < bytes.len() && bytes[i] == b'"' {
                i += 1;
                let val_start = i;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += 1;
                }
                let val = &json[val_start..i];
                i += 1;

                // Map known C2PA JSON keys to tag names (matching ExifTool's Jpeg2000 JUMBF table)
                let tag_name = match key {
                    "location" => Some("Location"),
                    "copyright" => Some("Copyright"),
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
