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
        "http://ns.useplus.org/ldf/xmp/1.0/" => "plus",
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

        // Pre-pass: collect rdf:nodeID-mapped bag/seq values for later reference resolution.
        // Also strip invalid XML chars from xpacket processing instructions.
        let xml_clean: String = sanitize_xmp_xml(xml_data);
        let xml_for_parse: &str = &xml_clean;

        // Check if this is RDF/XMP format or generic XML
        let is_rdf = xml_for_parse.contains("rdf:RDF") || xml_for_parse.contains("rdf:Description");
        if !is_rdf {
            // Generic XML: extract tags by building tag names from element paths
            return read_generic_xml(xml_for_parse);
        }

        // Pre-pass: collect rdf:nodeID → list values (for Bag/Seq with nodeIDs)
        let node_bags: std::collections::HashMap<String, Vec<String>> =
            collect_node_bag_values(xml_for_parse);

        let parser = EventReader::from_str(xml_for_parse);
        let mut path: Vec<(String, String)> = Vec::new(); // (namespace, local_name)
        let mut current_text = String::new();
        let mut in_rdf_li = false;
        let mut list_values: Vec<String> = Vec::new();
        // Track depths where we should emit even with empty text (ExifTool et:id format)
        let mut emit_empty_depths: std::collections::HashSet<usize> = std::collections::HashSet::new();
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

        // Language-alt tracking:
        // - current_li_lang: the xml:lang value on the current inner rdf:li
        // - in_lang_alt: we're inside a rdf:Alt element
        // - lang_alt_in_bag: the rdf:Alt is itself inside an outer rdf:li (Bag-of-lang-alt)
        // - bag_lang_values: per-lang accumulated list for bag-of-lang-alt
        // - bag_item_count: number of Bag items processed (for empty-slot tracking)
        let mut current_li_lang: Option<String> = None;
        let mut in_lang_alt = false;
        let mut lang_alt_in_bag = false;
        let mut bag_lang_values: std::collections::HashMap<String, Vec<Option<String>>> =
            std::collections::HashMap::new();
        let mut bag_item_count: usize = 0;

        for event in parser {
            match event {
                Ok(XmlEvent::StartElement {
                    name, attributes, ..
                }) => {
                    // Track the path
                    let ns_uri = name.namespace.as_deref().unwrap_or("");
                    path.push((ns_uri.to_string(), name.local_name.clone()));
                    current_text.clear();

                    // Track elements with et:id (ExifTool internal format): emit even if empty
                    let has_et_id = attributes.iter().any(|a| {
                        a.name.local_name == "id"
                            && (a.name.prefix.as_deref() == Some("et")
                                || a.name.namespace.as_deref() == Some("http://ns.exiftool.org/1.0/"))
                    });
                    if has_et_id {
                        emit_empty_depths.insert(path.len());
                    }

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

                    // Check if this element has a rdf:nodeID reference to a known bag/seq.
                    // E.g., <dc:subject rdf:nodeID="anon2"/> — emit the bag values as a tag.
                    // This is for non-Description elements that reference a nodeID bag.
                    if name.local_name != "Description"
                        && name.local_name != "RDF"
                        && name.namespace.as_deref() != Some("http://www.w3.org/1999/02/22-rdf-syntax-ns#")
                    {
                        if let Some(node_ref) = attributes.iter().find(|a| {
                            a.name.local_name == "nodeID"
                                && (a.name.prefix.as_deref() == Some("rdf")
                                    || a.name.namespace.as_deref() == Some("http://www.w3.org/1999/02/22-rdf-syntax-ns#"))
                        }) {
                            if let Some(bag_values) = node_bags.get(&node_ref.value) {
                                let ns_uri = name.namespace.as_deref().unwrap_or("");
                                let prefix = namespace_prefix(ns_uri);
                                let group_prefix = if prefix.is_empty() {
                                    name.prefix.as_deref().unwrap_or("XMP")
                                } else {
                                    prefix
                                };
                                let category = namespace_category(group_prefix);
                                let full_name = ucfirst(&name.local_name);
                                let value = if bag_values.len() == 1 {
                                    Value::String(bag_values[0].clone())
                                } else {
                                    Value::List(bag_values.iter().map(|s| Value::String(s.clone())).collect())
                                };
                                let pv = value.to_display_string();
                                tags.push(Tag {
                                    id: TagId::Text(format!("{}:{}", group_prefix, name.local_name)),
                                    name: full_name.clone(),
                                    description: full_name,
                                    group: TagGroup {
                                        family0: "XMP".into(),
                                        family1: format!("XMP-{}", group_prefix),
                                        family2: category.into(),
                                    },
                                    raw_value: value,
                                    print_value: pv,
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
                            // Skip rdf:nodeID on Description (it's just an identifier, not a value)
                            if attr.name.local_name == "nodeID"
                                && (attr.name.prefix.as_deref() == Some("rdf")
                                    || attr.name.namespace.as_deref() == Some("http://www.w3.org/1999/02/22-rdf-syntax-ns#"))
                            {
                                continue;
                            }
                            if attr.name.prefix.as_deref() == Some("xmlns") { continue; }
                            if attr.name.local_name.starts_with("xmlns") { continue; }
                            // Skip ExifTool-internal attributes (et:toolkit, et:id, et:desc, etc.)
                            if attr.name.prefix.as_deref() == Some("et")
                                || attr.name.namespace.as_deref() == Some("http://ns.exiftool.org/1.0/")
                                || attr.name.namespace.as_deref() == Some("http://ns.exiftool.ca/1.0/")
                            { continue; }

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
                        if in_lang_alt {
                            // Inner rdf:li inside rdf:Alt — track the xml:lang attribute
                            current_li_lang = attributes.iter()
                                .find(|a| a.name.local_name == "lang"
                                    && (a.name.prefix.as_deref() == Some("xml")
                                        || a.name.namespace.as_deref() == Some("http://www.w3.org/XML/1998/namespace")))
                                .map(|a| a.value.clone());
                        }
                        // Note: outer rdf:li increment happens when it closes (to correctly track which item we're on)
                        in_rdf_li = true;
                    }

                    // Detect rdf:Alt
                    if name.local_name == "Alt"
                        && name.namespace.as_deref() == Some("http://www.w3.org/1999/02/22-rdf-syntax-ns#")
                    {
                        in_lang_alt = true;
                        // Check if we're inside an outer rdf:li (Bag item)
                        // Path ends with: ..., Bag, li, Alt (just pushed)
                        let depth = path.len();
                        if depth >= 3 {
                            let li_elem = &path[depth - 2]; // li (just before Alt)
                            let bag_elem = &path[depth - 3]; // Bag
                            if li_elem.1 == "li" && li_elem.0 == "http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                                && bag_elem.1 == "Bag" && bag_elem.0 == "http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                            {
                                lang_alt_in_bag = true;
                            }
                        }
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
                        if in_lang_alt {
                            // Inner rdf:li inside rdf:Alt — store by lang
                            let lang = current_li_lang.take().unwrap_or_else(|| "x-default".to_string());
                            let text = normalize_xml_text(&current_text);
                            // Present-but-empty → Some(""), absent → padding with None below
                            let opt_text: Option<String> = Some(text.clone());
                            if lang_alt_in_bag {
                                // Bag-of-lang-alt: accumulate per-lang for current bag item
                                let entry = bag_lang_values.entry(lang.clone()).or_default();
                                // Pad to bag_item_count (how many outer lis have closed so far)
                                // bag_item_count is the count of completed outer lis
                                while entry.len() < bag_item_count {
                                    entry.push(None);
                                }
                                entry.push(opt_text);
                            } else {
                                // Simple lang-alt: use list_values for x-default, track others separately
                                if lang == "x-default" {
                                    list_values.push(text);
                                } else {
                                    // Store non-default lang values with "-lang" suffix
                                    bag_lang_values.entry(lang).or_default().push(opt_text);
                                }
                            }
                        } else if lang_alt_in_bag && !in_lang_alt {
                            // Closing an outer rdf:li in bag-of-alt mode
                            // (the Alt inside it has already closed and set in_lang_alt=false)
                            // Increment bag_item_count to mark this item as complete
                            bag_item_count += 1;
                            // list_values not used in bag-of-alt mode
                        } else if in_gcontainer_li {
                            // GContainer struct li: fields were captured as attributes, not text
                            in_gcontainer_li = false;
                        } else if !normalize_xml_text(&current_text).is_empty() {
                            list_values.push(normalize_xml_text(&current_text));
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
                        // Handle closing rdf:Alt (simple lang-alt or end of inner Alt in bag-of-alt)
                        if name.local_name == "Alt" {
                            in_lang_alt = false;
                            // If this Alt was inside a Bag li, don't emit yet — wait for Bag close
                            if lang_alt_in_bag {
                                // Reset for next Alt item — the outer li close will be next
                                path.pop();
                                current_text.clear();
                                continue;
                            }
                            // Simple lang-alt (not in bag): emit tag with x-default as main value
                            // and per-lang variants
                            if let Some(parent) = path.iter().rev().nth(1) {
                                let prefix = namespace_prefix(&parent.0);
                                let tag_name = parent.1.clone();
                                let group_prefix = if prefix.is_empty() { "XMP" } else { prefix };
                                let category = namespace_category(group_prefix);

                                // Check if this lang-alt field is inside a struct rdf:li
                                // Path (from end): Alt, CvTermName, li, Bag/Seq, AboutCvTerm, ...
                                // Check path.rev().nth(2) == "li" and path.rev().nth(3) == Bag/Seq
                                let rdf_ns = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
                                let in_struct_li_alt = path.iter().rev().nth(2)
                                    .map(|(ns, ln)| ln == "li" && ns == rdf_ns)
                                    .unwrap_or(false);
                                let (full_tag_name, emit_group_prefix, emit_category) = if in_struct_li_alt {
                                    // Find struct bag property name: skip li, Bag/Seq, then get the property
                                    let struct_parent = path.iter().rev()
                                        .skip(2) // skip tag_name and li
                                        .skip_while(|(ns, ln)| ln == "li" || ln == "Bag" || ln == "Seq" || ln == "Alt" || ns == rdf_ns)
                                        .find(|(ns, ln)| ln != "Description" && ns != rdf_ns);
                                    if let Some((sp_ns, sp_ln)) = struct_parent {
                                        let sp_prefix = namespace_prefix(sp_ns);
                                        let sp_gp = if sp_prefix.is_empty() { "XMP" } else { sp_prefix };
                                        let field_uc = ucfirst(&tag_name);
                                        let parent_uc = ucfirst(sp_ln);
                                        let stripped = strip_struct_prefix(&parent_uc, &field_uc);
                                        let flat = format!("{}{}", parent_uc, stripped);
                                        let cat = namespace_category(sp_gp);
                                        (flat, sp_gp.to_string(), cat.to_string())
                                    } else {
                                        (ucfirst(&tag_name), group_prefix.to_string(), category.to_string())
                                    }
                                } else {
                                    (ucfirst(&tag_name), group_prefix.to_string(), category.to_string())
                                };

                                // Emit x-default as main tag
                                if !list_values.is_empty() {
                                    let main_val = if list_values.len() == 1 {
                                        Value::String(list_values[0].clone())
                                    } else {
                                        Value::List(list_values.iter().map(|s| Value::String(s.clone())).collect())
                                    };
                                    let pv = main_val.to_display_string();
                                    tags.push(Tag {
                                        id: TagId::Text(format!("{}:{}", emit_group_prefix, tag_name)),
                                        name: full_tag_name.clone(),
                                        description: full_tag_name.clone(),
                                        group: TagGroup {
                                            family0: "XMP".into(),
                                            family1: format!("XMP-{}", emit_group_prefix),
                                            family2: emit_category.clone(),
                                        },
                                        raw_value: main_val,
                                        print_value: pv,
                                        priority: 0,
                                    });
                                    list_values.clear();
                                }
                                // Emit per-lang variants as TagName-lang
                                let mut lang_keys: Vec<String> = bag_lang_values.keys().cloned().collect();
                                lang_keys.sort();
                                for lang in &lang_keys {
                                    let vals = &bag_lang_values[lang];
                                    let non_none: Vec<String> = vals.iter()
                                        .filter_map(|v| v.clone())
                                        .collect();
                                    if !non_none.is_empty() {
                                        let lang_tag = format!("{}-{}", full_tag_name, lang);
                                        let val = if non_none.len() == 1 {
                                            Value::String(non_none[0].clone())
                                        } else {
                                            Value::List(non_none.iter().map(|s| Value::String(s.clone())).collect())
                                        };
                                        let pv = val.to_display_string();
                                        tags.push(Tag {
                                            id: TagId::Text(format!("{}-{}:{}", emit_group_prefix, lang, tag_name)),
                                            name: lang_tag.clone(),
                                            description: lang_tag.clone(),
                                            group: TagGroup {
                                                family0: "XMP".into(),
                                                family1: format!("XMP-{}", emit_group_prefix),
                                                family2: emit_category.clone(),
                                            },
                                            raw_value: val,
                                            print_value: pv,
                                            priority: 0,
                                        });
                                    }
                                }
                                bag_lang_values.clear();
                            }
                            path.pop();
                            current_text.clear();
                            continue;
                        }

                        // Handle closing rdf:Bag when it's a Bag-of-lang-alt
                        if name.local_name == "Bag" && lang_alt_in_bag {
                            lang_alt_in_bag = false;
                            bag_item_count = 0;
                            // Find the parent property name
                            if let Some(parent) = path.iter().rev().nth(1) {
                                let prefix = namespace_prefix(&parent.0);
                                let tag_name = parent.1.clone();
                                let group_prefix = if prefix.is_empty() { "XMP" } else { prefix };
                                let category = namespace_category(group_prefix);

                                // Collect all language codes (maintaining insertion order: x-default first)
                                let mut lang_keys: Vec<String> = bag_lang_values.keys().cloned().collect();
                                // Put x-default first
                                lang_keys.sort_by(|a, b| {
                                    if a == "x-default" { std::cmp::Ordering::Less }
                                    else if b == "x-default" { std::cmp::Ordering::Greater }
                                    else { a.cmp(b) }
                                });

                                for lang in &lang_keys {
                                    let vals = &bag_lang_values[lang];
                                    let is_default = lang == "x-default"; // kept for tag naming below

                                    // None = lang absent for this bag item → skip
                                    // Some("") = lang present but empty → keep as empty slot
                                    // Some(s) = lang present with value s → use s
                                    //
                                    // For x-default: skip None (absent items shouldn't affect default)
                                    // For other langs: skip None (absent), keep Some("") (present but empty)
                                    let joined: String = vals.iter()
                                        .filter_map(|v| v.as_deref()) // None filtered out
                                        .collect::<Vec<_>>()
                                        .join(", ");

                                    // Only emit if there's something meaningful
                                    let has_content = vals.iter().any(|v| v.is_some());
                                    if !has_content {
                                        continue;
                                    }

                                    let (tag_key, tag_display) = if is_default {
                                        (ucfirst(&tag_name), ucfirst(&tag_name))
                                    } else {
                                        let lt = format!("{}-{}", ucfirst(&tag_name), lang);
                                        (lt.clone(), lt)
                                    };

                                    tags.push(Tag {
                                        id: TagId::Text(format!("{}:{}", group_prefix, tag_key)),
                                        name: tag_key.clone(),
                                        description: tag_display,
                                        group: TagGroup {
                                            family0: "XMP".into(),
                                            family1: format!("XMP-{}", group_prefix),
                                            family2: category.into(),
                                        },
                                        raw_value: Value::String(joined.clone()),
                                        print_value: joined,
                                        priority: 0,
                                    });
                                }
                                bag_lang_values.clear();
                            }
                            path.pop();
                            current_text.clear();
                            continue;
                        }

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
                                // Skip RDF structural elements (RDF, Description, etc.)
                                if tag_name == "RDF" || tag_name == "xmpmeta"
                                    || parent.0 == "http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                                    || parent.0 == "adobe:ns:meta/"
                                {
                                    list_values.clear();
                                    path.pop();
                                    current_text.clear();
                                    continue;
                                }
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
                    if !normalize_xml_text(&current_text).is_empty() && in_rdf_li
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
                                raw_value: Value::String(normalize_xml_text(&current_text)),
                                print_value: normalize_xml_text(&current_text), priority: 0,
                            });
                        }
                        path.pop();
                        current_text.clear();
                        continue;
                    }

                    // Simple property with text content (or explicitly empty with et:id)
                    let has_et_depth = emit_empty_depths.contains(&path.len());
                    if (!normalize_xml_text(&current_text).is_empty() || has_et_depth) && !in_rdf_li {
                        let prefix = namespace_prefix(ns_uri);
                        let tag_name = &name.local_name;

                        // Skip RDF structural elements
                        if tag_name != "Description"
                            && name.namespace.as_deref()
                                != Some("http://www.w3.org/1999/02/22-rdf-syntax-ns#")
                        {
                            let group_prefix = if prefix.is_empty() { "XMP" } else { prefix };
                            let category = namespace_category(group_prefix);

                            let value = Value::String(normalize_xml_text(&current_text));
                            let print_value = value.to_display_string();

                            // Check if we're inside a bare struct (rdf:parseType='Resource')
                            // In that case, flatten: {StructParent}{FieldName}
                            // The struct parent element is at depth (parse_resource_depths.last() - 1)
                            let remapped = remap_xmp_tag_name(group_prefix, tag_name);
                            let full_name = if let Some(&struct_depth) = parse_resource_depths.last() {
                                // The struct element is at index struct_depth - 1 in path
                                if struct_depth >= 1 && struct_depth <= path.len() {
                                    let struct_elem = &path[struct_depth - 1];
                                    let struct_parent_name = ucfirst(&struct_elem.1);
                                    let field_uc = remapped.clone();
                                    let field_stripped = strip_struct_prefix(&struct_parent_name, &field_uc);
                                    format!("{}{}", struct_parent_name, field_stripped)
                                } else {
                                    remapped
                                }
                            } else {
                                remapped
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
                    emit_empty_depths.remove(&path.len());
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
///
/// Also handles PLUS-style: parent "CopyrightOwner", field "CopyrightOwnerName"
/// → field starts with parent → strip entire parent → "Name"
/// so flat = "CopyrightOwner" + "Name" = "CopyrightOwnerName"
fn strip_struct_prefix(parent: &str, field: &str) -> String {
    // First: try stripping the full parent name as prefix (e.g., CopyrightOwner from CopyrightOwnerName)
    if field.starts_with(parent) && field.len() > parent.len() {
        let stripped = &field[parent.len()..];
        if !stripped.is_empty() {
            return stripped.to_string();
        }
    }

    // Next: try progressively shorter suffixes of parent (min 2 chars, must start at word boundary)
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

/// Remap XMP tag names where ExifTool uses a different Name than the XMP local name.
fn remap_xmp_tag_name(group_prefix: &str, local_name: &str) -> String {
    match (group_prefix, local_name) {
        // tiff: namespace remappings
        ("tiff", "ImageLength") => "ImageHeight".into(),
        ("tiff", "BitsPerSample") => "BitsPerSample".into(),
        // exif: namespace remappings
        ("exif", "PixelXDimension") => "ExifImageWidth".into(),
        ("exif", "PixelYDimension") => "ExifImageHeight".into(),
        // photoshop: namespace remappings
        ("photoshop", "ICCProfile") => "ICCProfileName".into(),
        // plus: namespace remappings
        ("plus", "Version") => "PLUSVersion".into(),
        _ => {
            // For unknown/ExifTool-internal namespaces (group_prefix = "XMP" or anything unknown),
            // if the local name has only uppercase letters (e.g. "ISO"), ExifTool normalizes it:
            // "ISO" → lowercase → ucfirst → "Iso"
            // Mixed-case names are kept as-is with ucfirst.
            let has_lowercase = local_name.chars().any(|c| c.is_lowercase());
            let has_uppercase = local_name.chars().any(|c| c.is_uppercase());
            if has_uppercase && !has_lowercase && local_name.len() > 1 {
                ucfirst(&local_name.to_lowercase())
            } else {
                ucfirst(local_name)
            }
        }
    }
}

fn ucfirst(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

/// Normalize XML text content: trim outer whitespace and collapse internal whitespace sequences
/// (including newlines from multi-line XMP text nodes) into single spaces.
/// This matches ExifTool's XML text normalization behavior.
fn normalize_xml_text(s: &str) -> String {
    let trimmed = s.trim();
    if !trimmed.contains('\n') && !trimmed.contains('\r') {
        // Fast path: no line breaks
        return trimmed.to_string();
    }
    // Collapse any sequence of whitespace (including newlines) into a single space
    let mut result = String::with_capacity(trimmed.len());
    let mut last_was_space = false;
    for c in trimmed.chars() {
        if c.is_whitespace() {
            if !last_was_space {
                result.push(' ');
                last_was_space = true;
            }
        } else {
            result.push(c);
            last_was_space = false;
        }
    }
    result
}

/// Read generic (non-RDF) XML files by building tag names from element paths.
/// This mirrors ExifTool's XMP.pm generic XML handling.
fn read_generic_xml(xml: &str) -> Result<Vec<Tag>> {
    use xml::reader::{EventReader, XmlEvent};
    let mut tags = Vec::new();
    let mut seen_names: std::collections::HashSet<String> = std::collections::HashSet::new();

    let parser = EventReader::from_str(xml);
    let mut path: Vec<String> = Vec::new(); // element local names (ucfirst'd)
    let mut current_text = String::new();

    // Accumulate full path as tag name prefix: root element name + child names concatenated
    // Each path component is ucfirst'd to produce CamelCase tag names (e.g., GpxTrkName).
    // Attributes on elements are emitted as TagName = value (path + attrName)

    // Track which namespace URIs were declared on the root element (xmlns=...)
    let mut root_xmlns: Option<String> = None;

    for event in parser {
        match event {
            Ok(XmlEvent::StartElement { name, attributes, namespace, .. }) => {
                let local = ucfirst(&name.local_name);
                let path_str = format!("{}{}", path.join(""), local);
                path.push(local.clone());
                current_text.clear();

                // Emit default xmlns (xmlns="uri") as {ElemName}Xmlns tag
                // xml-rs exposes xmlns in the namespace mappings
                // The default namespace (no prefix) is exposed via namespace.get("")
                // We look for newly-declared default NS at this element
                use xml::namespace::Namespace;
                // Check if there's a new default namespace declared at this element
                // xml-rs merges namespaces so we check the full namespace map
                // The simplest approach: check if attributes contain xmlns-like entries
                // xml-rs exposes xmlns as regular attribute with prefix="xmlns", local=""
                // OR as a namespace mapping
                // Actually, in xml-rs the namespace object contains ALL in-scope namespaces.
                // We need to detect which ones are NEW at this element.
                // The simplest heuristic: only emit xmlns for root element (path depth 1)
                if path.len() == 1 {
                    // Root element: emit its default xmlns
                    if let Some(default_ns) = namespace.get("") {
                        // Emit as {RootName}Xmlns = default_ns_uri
                        let tag_name = format!("{}Xmlns", local);
                        if !seen_names.contains(&tag_name) {
                            seen_names.insert(tag_name.clone());
                            root_xmlns = Some(default_ns.to_string());
                            let val = Value::String(default_ns.to_string());
                            let pv = val.to_display_string();
                            tags.push(Tag {
                                id: TagId::Text(format!("XMP:{}", tag_name)),
                                name: tag_name.clone(), description: tag_name,
                                group: TagGroup { family0: "XMP".into(), family1: "XMP".into(), family2: "Other".into() },
                                raw_value: val, print_value: pv, priority: 0,
                            });
                        }
                    }
                }

                // Emit attributes as tags (only first occurrence)
                for attr in &attributes {
                    let aname = &attr.name;
                    if aname.prefix.as_deref() == Some("xmlns")
                        || aname.local_name == "xmlns"
                        || aname.local_name.starts_with("xmlns:")
                    {
                        // Skip xmlns declarations (handled via namespace above)
                        continue;
                    }
                    // For xsi:schemaLocation → emit as {path}SchemaLocation
                    let attr_local = ucfirst(&aname.local_name);
                    let tag_name = format!("{}{}", path_str, attr_local);
                    if !seen_names.contains(&tag_name) {
                        seen_names.insert(tag_name.clone());
                        // Determine group prefix from namespace
                        let attr_ns = aname.namespace.as_deref().unwrap_or("");
                        let pfx = namespace_prefix(attr_ns);
                        let group_pfx = if pfx.is_empty() {
                            aname.prefix.as_deref().unwrap_or("XMP")
                        } else { pfx };
                        // Normalize attribute value to collapse internal whitespace/newlines
                        let attr_val = normalize_xml_text(&attr.value);
                        let val = Value::String(attr_val.clone());
                        let pv = val.to_display_string();
                        tags.push(Tag {
                            id: TagId::Text(format!("XMP:{}", tag_name)),
                            name: tag_name.clone(), description: tag_name,
                            group: TagGroup { family0: "XMP".into(), family1: format!("XMP-{}", group_pfx), family2: "Other".into() },
                            raw_value: val, print_value: pv, priority: 0,
                        });
                    }
                }
            }
            Ok(XmlEvent::Characters(text)) | Ok(XmlEvent::CData(text)) => {
                current_text.push_str(&text);
            }
            Ok(XmlEvent::EndElement { .. }) => {
                let text = normalize_xml_text(&current_text);
                if !text.is_empty() && !path.is_empty() {
                    let tag_name = path.join("");
                    if !seen_names.contains(&tag_name) {
                        seen_names.insert(tag_name.clone());
                        let val = Value::String(text.clone());
                        let pv = val.to_display_string();
                        tags.push(Tag {
                            id: TagId::Text(format!("XMP:{}", tag_name)),
                            name: tag_name.clone(), description: tag_name,
                            group: TagGroup { family0: "XMP".into(), family1: "XMP".into(), family2: "Other".into() },
                            raw_value: val, print_value: pv, priority: 0,
                        });
                    }
                }
                current_text.clear();
                path.pop();
            }
            Err(_) => break,
            _ => {}
        }
    }
    Ok(tags)
}

/// Sanitize XMP XML: replace invalid XML characters (e.g., in xpacket PI values) with spaces.
/// This handles xpacket begin="" which may contain non-XML-legal bytes like 0x1A.
fn sanitize_xmp_xml(xml: &str) -> String {
    let mut result = String::with_capacity(xml.len());
    let mut in_pi = false; // inside a processing instruction <?...?>
    let chars: Vec<char> = xml.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if !in_pi && i + 1 < chars.len() && c == '<' && chars[i+1] == '?' {
            in_pi = true;
            result.push(c);
        } else if in_pi && c == '?' && i + 1 < chars.len() && chars[i+1] == '>' {
            in_pi = false;
            result.push(c);
            result.push(chars[i+1]);
            i += 2;
            continue;
        } else if in_pi {
            // Replace invalid XML chars in PI with space
            if c == '\t' || c == '\n' || c == '\r' || (c as u32 >= 0x20 && c as u32 <= 0xD7FF)
                || (c as u32 >= 0xE000 && c as u32 <= 0xFFFD)
                || (c as u32 >= 0x10000 && c as u32 <= 0x10FFFF) {
                result.push(c);
            } else {
                result.push(' ');
            }
        } else {
            result.push(c);
        }
        i += 1;
    }
    result
}

/// Pre-pass: collect rdf:nodeID-mapped Bag/Seq values from XMP XML.
/// Returns a map from nodeID string → list of rdf:li text values.
fn collect_node_bag_values(xml: &str) -> std::collections::HashMap<String, Vec<String>> {
    use xml::reader::{EventReader, XmlEvent};
    let mut map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();

    let parser = EventReader::from_str(xml);
    let mut current_node_id: Option<String> = None;
    let mut current_items: Vec<String> = Vec::new();
    let mut in_li = false;
    let mut current_text = String::new();
    let rdf_ns = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";

    for event in parser {
        match event {
            Ok(XmlEvent::StartElement { name, attributes, .. }) => {
                current_text.clear();
                let local = &name.local_name;
                let ns = name.namespace.as_deref().unwrap_or("");

                if (local == "Bag" || local == "Seq" || local == "Alt") && ns == rdf_ns {
                    // Check for rdf:nodeID attribute
                    if let Some(nid) = attributes.iter().find(|a| {
                        a.name.local_name == "nodeID"
                            && (a.name.prefix.as_deref() == Some("rdf") || ns == rdf_ns)
                    }) {
                        current_node_id = Some(nid.value.clone());
                        current_items.clear();
                    }
                } else if local == "li" && ns == rdf_ns && current_node_id.is_some() {
                    in_li = true;
                }
            }
            Ok(XmlEvent::Characters(text)) | Ok(XmlEvent::CData(text)) => {
                if in_li {
                    current_text.push_str(&text);
                }
            }
            Ok(XmlEvent::EndElement { name }) => {
                let local = &name.local_name;
                let ns = name.namespace.as_deref().unwrap_or("");
                if local == "li" && ns == rdf_ns && in_li {
                    let val = normalize_xml_text(&current_text);
                    if !val.is_empty() {
                        current_items.push(val);
                    }
                    in_li = false;
                    current_text.clear();
                } else if (local == "Bag" || local == "Seq" || local == "Alt") && ns == rdf_ns {
                    if let Some(nid) = current_node_id.take() {
                        map.insert(nid, std::mem::take(&mut current_items));
                    }
                }
            }
            Err(_) => break,
            _ => {}
        }
    }
    map
}
