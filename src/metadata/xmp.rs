//! XMP (Extensible Metadata Platform) reader.
//!
//! Parses Adobe XMP metadata stored as XML/RDF. Mirrors ExifTool's XMP.pm.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

use xml::reader::{EventReader, XmlEvent};

/// XMP metadata reader.
pub struct XmpReader;

/// Known XMP namespace prefixes.
fn namespace_prefix(uri: &str) -> &str {
    match uri {
        "http://purl.org/dc/elements/1.1/" => "dc",
        "http://ns.adobe.com/xap/1.0/" => "xmp",
        "http://ns.adobe.com/xap/1.0/mm/" => "xmpMM",
        "http://ns.adobe.com/xap/1.0/rights/" => "xmpRights",
        "http://ns.adobe.com/tiff/1.0/" => "tiff",
        "http://ns.adobe.com/exif/1.0/" => "exif",
        "http://ns.adobe.com/exif/1.0/aux/" => "aux",
        "http://ns.adobe.com/photoshop/1.0/" => "photoshop",
        "http://ns.adobe.com/camera-raw-settings/1.0/" => "crs",
        "http://ns.adobe.com/lightroom/1.0/" => "lr",
        "http://iptc.org/std/Iptc4xmpCore/1.0/xmlns/" => "Iptc4xmpCore",
        "http://iptc.org/std/Iptc4xmpExt/2008-02-29/" => "Iptc4xmpExt",
        "http://ns.google.com/photos/1.0/camera/" => "GCamera",
        "http://ns.google.com/photos/1.0/image/" => "GImage",
        "http://ns.google.com/photos/1.0/container/" => "GContainer",
        "http://ns.google.com/photos/1.0/container/item/" => "GContainerItem",
        "http://ns.google.com/photos/dd/1.0/device/" => "GDevice",
        "http://ns.adobe.com/xmp/note/" => "xmpNote",
        "adobe:ns:meta/" => "x",
        "http://ns.adobe.com/pdf/1.3/" => "pdf",
        "http://ns.adobe.com/xap/1.0/t/pg/" => "xmpTPg",
        "http://ns.adobe.com/xap/1.0/sType/Dimensions#" => "stDim",
        "http://ns.adobe.com/xap/1.0/sType/ResourceRef#" => "stRef",
        "http://ns.microsoft.com/photo/1.0/" => "MicrosoftPhoto",
        _ => "",
    }
}

/// Category for an XMP namespace.
fn namespace_category(prefix: &str) -> &str {
    match prefix {
        "dc" => "Author",
        "xmp" | "xmpMM" | "xmpRights" => "Other",
        "tiff" => "Image",
        "exif" | "aux" => "Camera",
        "photoshop" => "Image",
        "Iptc4xmpCore" | "Iptc4xmpExt" => "Other",
        _ => "Other",
    }
}

impl XmpReader {
    /// Parse XMP metadata from an XML byte slice.
    pub fn read(data: &[u8]) -> Result<Vec<Tag>> {
        let mut tags = Vec::new();

        // Handle UTF-16/32 BOM and convert to UTF-8 (from Perl XMP.pm line 4286)
        let converted: String;
        let xml_data = if data.starts_with(&[0xFE, 0xFF]) {
            // UTF-16 BE BOM
            let units: Vec<u16> = data[2..].chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
            converted = String::from_utf16_lossy(&units);
            converted.as_str()
        } else if data.starts_with(&[0xFF, 0xFE]) && !data.starts_with(&[0xFF, 0xFE, 0x00, 0x00]) {
            // UTF-16 LE BOM
            let units: Vec<u16> = data[2..].chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            converted = String::from_utf16_lossy(&units);
            converted.as_str()
        } else if data.starts_with(&[0xEF, 0xBB, 0xBF]) {
            // UTF-8 BOM — skip it
            converted = String::new(); // unused
            std::str::from_utf8(&data[3..])
                .map_err(|e| Error::InvalidXmp(format!("invalid UTF-8: {}", e)))?
        } else if data.len() > 4 && data[0] == 0 && data[1] != 0 {
            // UTF-16 BE without BOM (starts with \0<)
            let units: Vec<u16> = data.chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
            converted = String::from_utf16_lossy(&units);
            converted.as_str()
        } else {
            // UTF-8 (default)
            converted = String::new();
            std::str::from_utf8(data)
                .or_else(|_| {
                    let trimmed = &data[..data.iter().rposition(|&b| b == b'>').unwrap_or(0) + 1];
                    std::str::from_utf8(trimmed)
                })
                .map_err(|e| Error::InvalidXmp(format!("invalid UTF-8: {}", e)))?
        };

        let parser = EventReader::from_str(xml_data);
        let mut path: Vec<(String, String)> = Vec::new(); // (namespace, local_name)
        let mut current_text = String::new();
        let mut in_rdf_li = false;
        let mut list_values: Vec<String> = Vec::new();

        for event in parser {
            match event {
                Ok(XmlEvent::StartElement {
                    name, attributes, ..
                }) => {
                    // Track the path
                    let ns_uri = name.namespace.as_deref().unwrap_or("");
                    path.push((ns_uri.to_string(), name.local_name.clone()));
                    current_text.clear();

                    // x:xmpmeta — extract XMPToolkit from x:xmptk attribute
                    if name.local_name == "xmpmeta" {
                        for attr in &attributes {
                            if attr.name.local_name == "xmptk" || attr.name.local_name == "xaptk" {
                                tags.push(Tag {
                                    id: TagId::Text("x:xmptk".into()),
                                    name: "XMPToolkit".into(),
                                    description: "XMP Toolkit".into(),
                                    group: TagGroup { family0: "XMP".into(), family1: "XMP-x".into(), family2: "Other".into() },
                                    raw_value: Value::String(attr.value.clone()),
                                    print_value: attr.value.clone(),
                                    priority: 0,
                                });
                            }
                        }
                    }

                    // Extract attributes on rdf:Description as tags
                    // e.g., <rdf:Description GCamera:HDRPlusMakernote="...">
                    if name.local_name == "Description" {
                        for attr in &attributes {
                            // Emit rdf:about as "About" tag, skip xmlns
                            if attr.name.local_name == "about" {
                                if !attr.value.is_empty() {
                                    tags.push(Tag {
                                        id: TagId::Text("rdf:about".into()),
                                        name: "About".into(), description: "About".into(),
                                        group: TagGroup { family0: "XMP".into(), family1: "XMP-rdf".into(), family2: "Other".into() },
                                        raw_value: Value::String(attr.value.clone()),
                                        print_value: attr.value.clone(), priority: 0,
                                    });
                                }
                                continue;
                            }
                            if attr.name.prefix.as_deref() == Some("xmlns") { continue; }
                            if attr.name.local_name.starts_with("xmlns") { continue; }

                            let attr_ns = attr.name.namespace.as_deref().unwrap_or("");
                            let attr_prefix = namespace_prefix(attr_ns);
                            let group_prefix = if attr_prefix.is_empty() {
                                attr.name.prefix.as_deref().unwrap_or("XMP")
                            } else {
                                attr_prefix
                            };
                            let category = namespace_category(group_prefix);

                            if !attr.value.is_empty() {
                                let full_name = ucfirst(&attr.name.local_name);
                                tags.push(Tag {
                                    id: TagId::Text(format!("{}:{}", group_prefix, attr.name.local_name)),
                                    name: full_name,
                                    description: attr.name.local_name.clone(),
                                    group: TagGroup {
                                        family0: "XMP".to_string(),
                                        family1: format!("XMP-{}", group_prefix),
                                        family2: category.to_string(),
                                    },
                                    raw_value: Value::String(attr.value.clone()),
                                    print_value: attr.value.clone(),
                                    priority: 0,
                                });
                            }
                        }
                    }

                    if name.local_name == "li"
                        && name.namespace.as_deref() == Some("http://www.w3.org/1999/02/22-rdf-syntax-ns#")
                    {
                        in_rdf_li = true;
                    }
                }
                Ok(XmlEvent::Characters(text)) | Ok(XmlEvent::CData(text)) => {
                    current_text.push_str(&text);
                }
                Ok(XmlEvent::EndElement { name }) => {
                    let ns_uri = name.namespace.as_deref().unwrap_or("");

                    // Handle rdf:li list items
                    if name.local_name == "li"
                        && name.namespace.as_deref() == Some("http://www.w3.org/1999/02/22-rdf-syntax-ns#")
                    {
                        if !current_text.trim().is_empty() {
                            list_values.push(current_text.trim().to_string());
                        }
                        in_rdf_li = false;
                        path.pop();
                        current_text.clear();
                        continue;
                    }

                    // When we close a Seq/Bag/Alt, emit the collected list
                    if (name.local_name == "Seq"
                        || name.local_name == "Bag"
                        || name.local_name == "Alt")
                        && name.namespace.as_deref() == Some("http://www.w3.org/1999/02/22-rdf-syntax-ns#")
                    {
                        if !list_values.is_empty() {
                            // The parent element is the actual property
                            if let Some(parent) = path.iter().rev().nth(1) {
                                let prefix = namespace_prefix(&parent.0);
                                let tag_name = &parent.1;
                                let group_prefix =
                                    if prefix.is_empty() { "XMP" } else { prefix };
                                let category = namespace_category(group_prefix);

                                let value = if list_values.len() == 1 {
                                    Value::String(list_values[0].clone())
                                } else {
                                    Value::List(
                                        list_values
                                            .iter()
                                            .map(|s| Value::String(s.clone()))
                                            .collect(),
                                    )
                                };

                                let full_name = ucfirst(tag_name);
                                let print_value = value.to_display_string();

                                tags.push(Tag {
                                    id: TagId::Text(format!("{}:{}", group_prefix, tag_name)),
                                    name: full_name,
                                    description: tag_name.clone(),
                                    group: TagGroup {
                                        family0: "XMP".to_string(),
                                        family1: format!("XMP-{}", group_prefix),
                                        family2: category.to_string(),
                                    },
                                    raw_value: value,
                                    print_value,
                                    priority: 0,
                                });
                            }
                            list_values.clear();
                        }
                        path.pop();
                        current_text.clear();
                        continue;
                    }

                    // Simple property with text content
                    if !current_text.trim().is_empty() && !in_rdf_li {
                        let prefix = namespace_prefix(ns_uri);
                        let tag_name = &name.local_name;

                        // Skip RDF structural elements
                        if tag_name != "Description"
                            && name.namespace.as_deref()
                                != Some("http://www.w3.org/1999/02/22-rdf-syntax-ns#")
                        {
                            let group_prefix = if prefix.is_empty() { "XMP" } else { prefix };
                            let category = namespace_category(group_prefix);

                            let value = Value::String(current_text.trim().to_string());
                            let full_name = ucfirst(tag_name);
                            let print_value = value.to_display_string();

                            tags.push(Tag {
                                id: TagId::Text(format!("{}:{}", group_prefix, tag_name)),
                                name: full_name,
                                description: tag_name.clone(),
                                group: TagGroup {
                                    family0: "XMP".to_string(),
                                    family1: format!("XMP-{}", group_prefix),
                                    family2: category.to_string(),
                                },
                                raw_value: value,
                                print_value,
                                priority: 0,
                            });
                        }
                    }

                    path.pop();
                    current_text.clear();
                }
                Err(_) => break,
                _ => {}
            }
        }

        Ok(tags)
    }
}

fn ucfirst(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}
