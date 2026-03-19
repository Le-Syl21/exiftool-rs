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
        "http://ns.adobe.com/xap/1.0/bj/" => "xmpBJ",
        "http://ns.adobe.com/xap/1.0/sType/Job#" => "stJob",
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

/// Check whether an attribute's local_name is the GCamera HDRPlus makernote field.
/// ExifTool maps GCamera:HdrPlusMakernote (and GCamera:hdrp_makernote) → HDRPlusMakerNote.
fn is_hdrp_makernote_attr(local_name: &str) -> bool {
    local_name == "HdrPlusMakernote" || local_name == "hdrp_makernote"
}

/// Emit the HDRPlusMakerNote binary tag + all decoded HDRP sub-tags.
fn emit_hdrp_makernote(b64_value: &str, tags: &mut Vec<Tag>) {
    use crate::metadata::google_hdrp::decode_hdrp_makernote;

    // Emit HDRPlusMakerNote as a binary tag (ExifTool shows "(Binary data N bytes...)")
    let raw_bytes = b64_value.trim().len() * 3 / 4; // approximate decoded size
    let print = format!("(Binary data {} bytes, use -b option to extract)", raw_bytes);
    tags.push(Tag {
        id: TagId::Text("GCamera:HdrPlusMakernote".into()),
        name: "HDRPlusMakerNote".into(),
        description: "HDRPlusMakerNote".into(),
        group: TagGroup {
            family0: "XMP".into(),
            family1: "XMP-GCamera".into(),
            family2: "Other".into(),
        },
        raw_value: Value::String(b64_value.to_string()),
        print_value: print,
        priority: 0,
    });

    // Decode and emit HDRP protobuf sub-tags
    let hdrp_tags = decode_hdrp_makernote(b64_value);
    tags.extend(hdrp_tags);
}

impl XmpReader {
    /// Parse XMP metadata from an XML byte slice.
    pub fn read(data: &[u8]) -> Result<Vec<Tag>> {
        let mut tags = Vec::new();

        // Handle UTF-16/32 BOM and convert to UTF-8 (from Perl XMP.pm line 4286)
        // For UTF-16 inputs we need an owned String to borrow from; for UTF-8 we borrow directly.
        let converted: Option<String> = if data.starts_with(&[0xFE, 0xFF]) {
            let units: Vec<u16> = data[2..].chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
            Some(String::from_utf16_lossy(&units))
        } else if data.starts_with(&[0xFF, 0xFE]) && !data.starts_with(&[0xFF, 0xFE, 0x00, 0x00]) {
            let units: Vec<u16> = data[2..].chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            Some(String::from_utf16_lossy(&units))
        } else if data.len() > 4 && data[0] == 0 && data[1] != 0 {
            // UTF-16 BE without BOM (starts with \0<)
            let units: Vec<u16> = data.chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
            Some(String::from_utf16_lossy(&units))
        } else {
            None
        };
        let xml_data: &str = if let Some(ref s) = converted {
            s.as_str()
        } else if data.starts_with(&[0xEF, 0xBB, 0xBF]) {
            // UTF-8 BOM — skip it
            std::str::from_utf8(&data[3..])
                .map_err(|e| Error::InvalidXmp(format!("invalid UTF-8: {}", e)))?
        } else {
            // UTF-8 (default)
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
        // Track elements with rdf:parseType='Resource' (bare structs).
        // Each entry is the path depth at which we entered such an element.
        let mut parse_resource_depths: Vec<usize> = Vec::new();

        // GContainer struct: collect per-field lists for DirectoryItemMime/Semantic/Length.
        // Key: flat field name (e.g. "Mime", "Semantic", "Length"), Values: collected per li.
        let mut gcontainer_fields: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        // Whether we're currently inside a GContainer:Directory/Seq context.
        let mut in_gcontainer_seq = false;
        // Whether we're inside a GContainer:Directory/Seq/li (struct li).
        let mut in_gcontainer_li = false;

        for event in parser {
            match event {
                Ok(XmlEvent::StartElement {
                    name, attributes, ..
                }) => {
                    // Track the path
                    let ns_uri = name.namespace.as_deref().unwrap_or("");
                    path.push((ns_uri.to_string(), name.local_name.clone()));
                    current_text.clear();

                    // Track rdf:parseType='Resource' (bare struct context)
                    let has_parse_resource = attributes.iter().any(|a| {
                        a.name.local_name == "parseType"
                            && (a.name.namespace.as_deref() == Some("http://www.w3.org/1999/02/22-rdf-syntax-ns#")
                                || a.name.prefix.as_deref() == Some("rdf"))
                            && a.value == "Resource"
                    });
                    if has_parse_resource {
                        parse_resource_depths.push(path.len());
                    }

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
                    // e.g., <rdf:Description GCamera:HdrPlusMakernote="...">
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

                            if !attr.value.is_empty() {
                                // Special handling: GCamera:HdrPlusMakernote / GCamera:hdrp_makernote
                                // → emit HDRPlusMakerNote (binary) + decode HDRP sub-tags
                                if group_prefix == "GCamera" && is_hdrp_makernote_attr(&attr.name.local_name) {
                                    emit_hdrp_makernote(&attr.value, &mut tags);
                                    continue;
                                }

                                let category = namespace_category(group_prefix);
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

                    // Detect GContainer:Directory/rdf:Seq entry
                    // Path looks like: [..., (GContainer_ns, "Directory"), (rdf_ns, "Seq")]
                    if name.local_name == "Seq"
                        && name.namespace.as_deref() == Some("http://www.w3.org/1999/02/22-rdf-syntax-ns#")
                    {
                        // Check if the parent is GContainer:Directory
                        if let Some(parent) = path.iter().rev().nth(1) {
                            if parent.1 == "Directory"
                                && parent.0 == "http://ns.google.com/photos/1.0/container/"
                            {
                                in_gcontainer_seq = true;
                            }
                        }
                    }

                    // Inside GContainer Seq: rdf:li starts a struct item
                    if in_gcontainer_seq
                        && name.local_name == "li"
                        && name.namespace.as_deref() == Some("http://www.w3.org/1999/02/22-rdf-syntax-ns#")
                    {
                        in_gcontainer_li = true;
                        in_rdf_li = true;
                    } else if name.local_name == "li"
                        && name.namespace.as_deref() == Some("http://www.w3.org/1999/02/22-rdf-syntax-ns#")
                    {
                        in_rdf_li = true;
                    }

                    // Inside a GContainer struct li: capture Container:Item attributes
                    // These are struct fields: Item:Mime, Item:Semantic, Item:Length
                    if in_gcontainer_li
                        && name.local_name == "Item"
                        && name.namespace.as_deref() == Some("http://ns.google.com/photos/1.0/container/")
                    {
                        // Collect Item:Mime, Item:Semantic, Item:Length for this li entry
                        let mut found: std::collections::HashMap<String, String> =
                            std::collections::HashMap::new();
                        for attr in &attributes {
                            if attr.name.namespace.as_deref() == Some("http://ns.google.com/photos/1.0/container/item/") {
                                let field = ucfirst(&attr.name.local_name);
                                found.insert(field, attr.value.clone());
                            }
                        }
                        // Accumulate: for each known field, push value or empty string
                        // (so all lists stay aligned)
                        let known = ["Mime", "Semantic", "Length"];
                        for k in &known {
                            if let Some(v) = found.get(*k) {
                                gcontainer_fields.entry(k.to_string())
                                    .or_default()
                                    .push(v.clone());
                            }
                        }
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
                        if in_gcontainer_li {
                            // GContainer struct li: fields were captured as attributes, not text
                            in_gcontainer_li = false;
                        } else if !current_text.trim().is_empty() {
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
                        // If this is the GContainer Seq, emit DirectoryItem* tags
                        if in_gcontainer_seq && name.local_name == "Seq" {
                            in_gcontainer_seq = false;
                            // Emit each field as DirectoryItem{Field}
                            for (field, values) in &gcontainer_fields {
                                let tag_name = format!("DirectoryItem{}", field);
                                let value = if values.len() == 1 {
                                    Value::String(values[0].clone())
                                } else {
                                    Value::List(values.iter().map(|s| Value::String(s.clone())).collect())
                                };
                                let print_value = value.to_display_string();
                                tags.push(Tag {
                                    id: TagId::Text(format!("GContainer:{}", tag_name)),
                                    name: tag_name.clone(),
                                    description: tag_name.clone(),
                                    group: TagGroup {
                                        family0: "XMP".into(),
                                        family1: "XMP-GContainer".into(),
                                        family2: "Image".into(),
                                    },
                                    raw_value: value,
                                    print_value,
                                    priority: 0,
                                });
                            }
                            gcontainer_fields.clear();
                        } else if !list_values.is_empty() {
                            // The parent element is the actual property
                            if let Some(parent) = path.iter().rev().nth(1) {
                                let prefix = namespace_prefix(&parent.0);
                                let tag_name = &parent.1;
                                let group_prefix =
                                    if prefix.is_empty() { "XMP" } else { prefix };
                                let _category = namespace_category(group_prefix);

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

                                // Check if this property is inside a struct rdf:li
                                // Path (from end): ..., [struct_bag_prop], Bag, li, [tag_name], [Bag/Seq/Alt]
                                // Find li between tag_name and struct_bag_prop
                                // path currently has: ..., struct_bag_prop, Bag, li, tag_name, Alt
                                // rev(): Alt, tag_name, li, Bag, struct_bag_prop, ...
                                let in_struct_li = path.iter().rev()
                                    .skip(1) // skip Alt (current)
                                    .skip(1) // skip tag_name (parent)
                                    .any(|(ns, ln)| ln == "li" && ns == "http://www.w3.org/1999/02/22-rdf-syntax-ns#");

                                let (full_name, emit_group_prefix) = if in_struct_li {
                                    // Find the bag/seq property name (ancestor before the li)
                                    // Skip: tag_name, li, Bag/Seq, then find the struct bag property
                                    let struct_parent = path.iter().rev()
                                        .skip(1) // skip Alt (current closing element is not in path yet)
                                        .skip(1) // skip tag_name (this is the field)
                                        .skip_while(|(ns, ln)| ln == "li" || ln == "Bag" || ln == "Seq" || ln == "Alt" || ns == "http://www.w3.org/1999/02/22-rdf-syntax-ns#")
                                        .find(|(ns, ln)| ln != "Description" && ns != "http://www.w3.org/1999/02/22-rdf-syntax-ns#");
                                    if let Some((sp_ns, sp_ln)) = struct_parent {
                                        let sp_prefix = namespace_prefix(sp_ns);
                                        let sp_gp = if sp_prefix.is_empty() { "XMP" } else { sp_prefix };
                                        let field_uc = ucfirst(tag_name);
                                        let parent_uc = ucfirst(sp_ln);
                                        let field_stripped = strip_struct_prefix(&parent_uc, &field_uc);
                                        let flat = format!("{}{}", parent_uc, field_stripped);
                                        (flat, sp_gp.to_string())
                                    } else {
                                        (ucfirst(tag_name), group_prefix.to_string())
                                    }
                                } else {
                                    (ucfirst(tag_name), group_prefix.to_string())
                                };

                                let emit_cat = namespace_category(&emit_group_prefix);
                                let print_value = value.to_display_string();

                                tags.push(Tag {
                                    id: TagId::Text(format!("{}:{}", emit_group_prefix, tag_name)),
                                    name: full_name.clone(),
                                    description: full_name,
                                    group: TagGroup {
                                        family0: "XMP".to_string(),
                                        family1: format!("XMP-{}", emit_group_prefix),
                                        family2: emit_cat.to_string(),
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

                    // Struct properties inside rdf:li (e.g., stJob:name inside xmpBJ:JobRef/Bag/li)
                    // Perl flattens as "{ParentBag}{FieldName}" → "JobRefName"
                    if !current_text.trim().is_empty() && in_rdf_li
                        && name.namespace.as_deref() != Some("http://www.w3.org/1999/02/22-rdf-syntax-ns#")
                        && name.local_name != "Description"
                    {
                        // Find the Bag/Seq parent property name
                        // Find the Bag/Seq parent: skip li, Bag, Seq, Alt, Description, and the current tag name
                        let cur_name = &name.local_name;
                        let parent_name = path.iter().rev()
                            .find(|(ns, ln)| ln != "li" && ln != "Bag" && ln != "Seq" && ln != "Alt"
                                && ln != "Description" && ln != cur_name
                                && ns != "http://www.w3.org/1999/02/22-rdf-syntax-ns#")
                            .map(|(_, ln)| ln.as_str())
                            .unwrap_or("");
                        if !parent_name.is_empty() {
                            // Perl strips struct-type prefix from field names when the parent
                            // name ends with the same prefix as the field name starts with.
                            // E.g., parent "AboutCvTerm" ends with "CvTerm", field "CvTermName"
                            // starts with "CvTerm" → strip "CvTerm" → flat = "AboutCvTerm" + "Name"
                            let field_local = ucfirst(&name.local_name);
                            let parent_ucfirst = ucfirst(parent_name);
                            let field_stripped = strip_struct_prefix(&parent_ucfirst, &field_local);
                            let flat_name = format!("{}{}", parent_ucfirst, field_stripped);
                            let prefix = namespace_prefix(ns_uri);
                            let group_prefix = if prefix.is_empty() { "XMP" } else { prefix };
                            let category = namespace_category(group_prefix);
                            tags.push(Tag {
                                id: TagId::Text(format!("{}:{}", group_prefix, flat_name)),
                                name: flat_name.clone(), description: flat_name,
                                group: TagGroup { family0: "XMP".into(), family1: format!("XMP-{}", group_prefix), family2: category.into() },
                                raw_value: Value::String(current_text.trim().to_string()),
                                print_value: current_text.trim().to_string(), priority: 0,
                            });
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
                            let print_value = value.to_display_string();

                            // Check if we're inside a bare struct (rdf:parseType='Resource')
                            // In that case, flatten: {StructParent}{FieldName}
                            // The struct parent element is at depth (parse_resource_depths.last() - 1)
                            let full_name = if let Some(&struct_depth) = parse_resource_depths.last() {
                                // The struct element is at index struct_depth - 1 in path
                                if struct_depth >= 1 && struct_depth <= path.len() {
                                    let struct_elem = &path[struct_depth - 1];
                                    let struct_parent_name = ucfirst(&struct_elem.1);
                                    let field_uc = ucfirst(tag_name);
                                    let field_stripped = strip_struct_prefix(&struct_parent_name, &field_uc);
                                    format!("{}{}", struct_parent_name, field_stripped)
                                } else {
                                    ucfirst(tag_name)
                                }
                            } else {
                                ucfirst(tag_name)
                            };

                            tags.push(Tag {
                                id: TagId::Text(format!("{}:{}", group_prefix, tag_name)),
                                name: full_name.clone(),
                                description: full_name,
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

                    // Pop parse_resource_depths if we're leaving that element
                    if parse_resource_depths.last() == Some(&path.len()) {
                        parse_resource_depths.pop();
                    }
                    path.pop();
                    current_text.clear();
                }
                Err(_) => break,
                _ => {}
            }
        }

        // Post-processing: emit GainMap Warning if DirectoryItemSemantic contains "GainMap"
        let has_gainmap = tags.iter().any(|t| {
            t.name == "DirectoryItemSemantic"
                && t.print_value.contains("GainMap")
        });
        if has_gainmap {
            // Find DirectoryItemMime and DirectoryItemLength for the GainMap entry
            // Emit warning about GainMap image/jpeg not found in trailer
            let gainmap_mime = tags.iter()
                .find(|t| t.name == "DirectoryItemSemantic")
                .and_then(|t| {
                    // Find the semantic that is GainMap and get the corresponding Mime
                    // For simplicity, look for GainMap in the values
                    if let Value::List(ref items) = t.raw_value {
                        items.iter().enumerate()
                            .find(|(_, v)| v.to_display_string() == "GainMap")
                            .map(|(i, _)| i)
                    } else {
                        None
                    }
                })
                .and_then(|idx| {
                    tags.iter()
                        .find(|t| t.name == "DirectoryItemMime")
                        .and_then(|t| match &t.raw_value {
                            Value::List(items) => items.get(idx).map(|v| v.to_display_string()),
                            Value::String(s) => if idx == 0 { Some(s.clone()) } else { None },
                            _ => None,
                        })
                })
                .unwrap_or_else(|| "image/jpeg".to_string());

            let warning_msg = format!(
                "[minor] Error reading GainMap {} from trailer",
                gainmap_mime
            );
            tags.push(Tag {
                id: TagId::Text("Warning".into()),
                name: "Warning".into(),
                description: "Warning".into(),
                group: TagGroup {
                    family0: "ExifTool".into(),
                    family1: "ExifTool".into(),
                    family2: "Other".into(),
                },
                raw_value: Value::String(warning_msg.clone()),
                print_value: warning_msg,
                priority: 0,
            });
        }

        Ok(tags)
    }
}

/// Strip struct-type prefix from field name when the parent name ends with that prefix.
/// E.g., parent "AboutCvTerm" ends with "CvTerm", field "CvTermName" starts with "CvTerm"
/// → return "Name" (stripped), so flat = "AboutCvTerm" + "Name" = "AboutCvTermName"
fn strip_struct_prefix(parent: &str, field: &str) -> String {
    // Try progressively shorter suffixes of parent (min 2 chars, must start at word boundary)
    let parent_chars: Vec<char> = parent.chars().collect();
    for start in 1..parent_chars.len().saturating_sub(1) {
        // Only try positions that start with uppercase (word boundary)
        if !parent_chars[start].is_uppercase() {
            continue;
        }
        let suffix: String = parent_chars[start..].iter().collect();
        if field.starts_with(suffix.as_str()) && suffix.len() > 1 {
            let stripped = &field[suffix.len()..];
            if !stripped.is_empty() {
                return stripped.to_string();
            }
        }
    }
    field.to_string()
}

fn ucfirst(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}
