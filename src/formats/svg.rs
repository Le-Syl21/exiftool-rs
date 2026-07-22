//! SVG (Scalable Vector Graphics) format reader.

use super::misc::mktag;
use crate::error::{Error, Result};
use crate::metadata::XmpReader;
use crate::tag::Tag;
use crate::value::Value;

/// A tag of `%Image::ExifTool::XMP::SVG` (XMP2.pl:1824-1846).
///
/// That table is `GROUPS => { 0 => 'SVG', 1 => 'SVG', 2 => 'Image' }`, and it is
/// the table FoundXMP picks when the property carries no namespace prefix
/// (XMP.pm:3449-3454). The category applies to every tag it invents there too,
/// `Xmlns` included, so it is stamped here rather than left to the family-2
/// table pass, which only knows the tags ExifTool names explicitly.
fn svg_tag(name: &str, description: &str, value: Value) -> Tag {
    let mut tag = mktag("SVG", name, description, value);
    tag.group.family2 = "Image".into();
    tag
}

/// Family-1 group of a property in an SVG file that is NOT in the SVG namespace.
///
/// FoundXMP routes such a property to `%Image::ExifTool::XMP::otherSVG`
/// (XMP.pm:3455-3458), whose `GROUPS => { 0 => 'SVG', 2 => 'Unknown' }`
/// (XMP2.pl:1849-1852) has no family 1: XMP.pm:3718 builds it as
/// `"$$tagTablePtr{GROUPS}{0}-$ns"`, i.e. `SVG-<prefix>`, where `<prefix>` is
/// the prefix the file declared -- rewritten to ExifTool's own prefix first if
/// the URI is one ExifTool knows (XMP.pm:3925-3931).
fn svg_group_prefix(uri: &str, declared: Option<&str>) -> Option<String> {
    let declared = declared?;
    if declared.is_empty() || declared == "svg" || declared == "xml" || declared == "xmlns" {
        return None;
    }
    let std_prefix = crate::metadata::xmp::namespace_prefix(uri);
    if std_prefix.is_empty() {
        Some(declared.to_string())
    } else {
        Some(std_prefix.to_string())
    }
}

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
    // Family-1 group prefix contributed by each path element, parallel to `path`:
    // `None` for a property in the SVG namespace itself. GetXMPTagID keeps the
    // namespace of the FIRST property that contributes to the tag name and has
    // one (`$namespace = $ns unless $namespace`, XMP.pm:3064), so the emit below
    // takes the first `Some` along the path.
    let mut path_group: Vec<Option<String>> = Vec::new();
    // Prefix the c2pa manifest element was declared with, for its `SVG-<prefix>`
    // family-1 group (XMP.pm:3718).
    let mut c2pa_prefix = String::from("c2pa");

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
                    path_group.push(None);
                    for attr in &attributes {
                        match attr.name.local_name.as_str() {
                            "width" => tags.push(svg_tag(
                                "ImageWidth",
                                "Image Width",
                                Value::String(attr.value.clone()),
                            )),
                            "height" => tags.push(svg_tag(
                                "ImageHeight",
                                "Image Height",
                                Value::String(attr.value.clone()),
                            )),
                            "version" => tags.push(svg_tag(
                                "SVGVersion",
                                "SVG Version",
                                Value::String(attr.value.clone()),
                            )),
                            "viewBox" | "viewbox" => tags.push(svg_tag(
                                "ViewBox",
                                "View Box",
                                Value::String(attr.value.clone()),
                            )),
                            "id" => {
                                tags.push(svg_tag("ID", "ID", Value::String(attr.value.clone())))
                            }
                            _ => {}
                        }
                    }
                    // Extract default namespace (xmlns="...") from the namespace map
                    if let Some(default_ns) = namespace.get("") {
                        if !default_ns.is_empty() {
                            tags.push(svg_tag(
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
                    path_group.push(None);
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
                        c2pa_prefix = name.prefix.as_deref().unwrap_or("c2pa").to_string();
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
                    path_group.push(svg_group_prefix(ns, name.prefix.as_deref()));
                    current_text.clear();
                    continue;
                }

                path.push(svg_ucfirst(local));
                had_child.push(false);
                path_group.push(svg_group_prefix(ns, name.prefix.as_deref()));
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
                                let jumbf_group = manifest_group(&c2pa_prefix);
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
                    path_group.pop();
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
                            match path_group.iter().skip(1).flatten().next() {
                                // A non-SVG namespace: XMP::otherSVG, family 1
                                // `SVG-<prefix>` and family 2 `Unknown`.
                                Some(prefix) => {
                                    let mut tag =
                                        mktag("SVG", &tag_name, &tag_name, Value::String(t));
                                    tag.group.family1 = format!("SVG-{prefix}");
                                    tag.group.family2 = "Unknown".into();
                                    tags.push(tag);
                                }
                                None => tags.push(svg_tag(&tag_name, &tag_name, Value::String(t))),
                            }
                        }
                    }
                    path.pop();
                    path_group.pop();
                    // If we've returned to Svg level (path.len() == 1), exit svg_body
                    if path.len() <= 1 {
                        in_svg_body = false;
                    }
                    current_text.clear();
                    continue;
                }

                path.pop();
                had_child.pop();
                path_group.pop();
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
                    let jumbf_group = manifest_group("c2pa");
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

/// Groups of the `JUMBF` tag an SVG's `c2pa:manifest` property produces.
///
/// The tag is `'c2pa:manifest' => { Name => 'JUMBF', Groups => { 0 => 'JUMBF' } }`
/// of `%Image::ExifTool::XMP::otherSVG` (XMP2.pl:1853-1861): family 0 is
/// overridden to `JUMBF`, but family 1 and 2 are the ones the table gives every
/// property in a foreign namespace -- `SVG-<prefix>` (XMP.pm:3718) and `Unknown`
/// (XMP2.pl:1850). The JUMBF boxes *inside* the manifest are a different table
/// and keep their own groups.
fn manifest_group(prefix: &str) -> crate::tag::TagGroup {
    crate::tag::TagGroup {
        family0: "JUMBF".into(),
        family1: format!("SVG-{prefix}"),
        family2: "Unknown".into(),
        family3: "Main".into(),
    }
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

/// Parse the JUMBF box structure of an SVG `c2pa:manifest`.
///
/// Identical to the JPEG APP11 path: the same `Jpeg2000::Main` box chain, the
/// same `ProcessJUMB` sub-document numbering (Jpeg2000.pm:777-797) and the same
/// `JSON::Main` parse of every `json` box (Jpeg2000.pm:409-419).
fn parse_jumbf_for_svg(data: &[u8], tags: &mut Vec<Tag>) {
    crate::formats::jpeg::parse_jumbf_boxes(data, tags);
}
