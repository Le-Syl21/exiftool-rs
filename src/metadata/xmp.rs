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
        "http://ns.adobe.com/xap/1.0/g/" => "xmpG",
        "http://ns.adobe.com/xap/1.0/g/img/" => "xmpGImg",
        "http://ns.adobe.com/xap/1.0/sType/Dimensions#" => "stDim",
        "http://ns.adobe.com/xap/1.0/sType/ResourceRef#" => "stRef",
        "http://ns.adobe.com/xap/1.0/sType/Font#" => "stFnt",
        "http://ns.adobe.com/xap/1.0/sType/ManifestItem#" => "stMfs",
        "http://www.w3.org/2000/01/rdf-schema#" => "rdfs",
        "http://ns.microsoft.com/photo/1.0/" => "MicrosoftPhoto",
        "http://ns.useplus.org/ldf/xmp/1.0/" => "plus",
        "http://ns.adobe.com/xap/1.0/sType/Area#" => "stArea",
        "http://www.metadataworkinggroup.com/schemas/regions/" => "mwg-rs",
        "http://www.metadataworkinggroup.com/schemas/keywords/" => "mwg-kw",
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
    let print = format!(
        "(Binary data {} bytes, use -b option to extract)",
        raw_bytes
    );
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
            let units: Vec<u16> = data[2..]
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            Some(String::from_utf16_lossy(&units))
        } else if data.starts_with(&[0xFF, 0xFE]) && !data.starts_with(&[0xFF, 0xFE, 0x00, 0x00]) {
            let units: Vec<u16> = data[2..]
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            Some(String::from_utf16_lossy(&units))
        } else if data.len() > 4 && data[0] == 0 && data[1] != 0 {
            // UTF-16 BE without BOM (starts with \0<)
            let units: Vec<u16> = data
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
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
        let xml_sanitized: String = sanitize_xmp_xml(xml_data);
        let xml_clean: String = fix_malformed_xml(&xml_sanitized);
        let xml_for_parse: &str = &xml_clean;

        // INX detection: InDesign Interchange format — XMP is embedded in CDATA
        // Detect by: starts with <?xml, followed by <?aid on next line
        let is_inx = {
            let trimmed = xml_for_parse.trim_start();
            trimmed.starts_with("<?xml") && {
                // Look for <?aid on one of the first few lines
                trimmed
                    .lines()
                    .take(5)
                    .any(|l| l.trim_start().starts_with("<?aid "))
            }
        };
        if is_inx {
            // Extract XMP from CDATA: find '<![CDATA[<?xpacket begin' ... '<?xpacket end...?>]]>'
            if let Some(cdata_start) = xml_for_parse.find("<![CDATA[<?xpacket begin") {
                let xmp_start = cdata_start + 9; // skip '<![CDATA['
                                                 // Find the end: '<?xpacket end="r"?>]]>' or '<?xpacket end="w"?>]]>'
                if let Some(end_marker) = xml_for_parse[xmp_start..].find("<?xpacket end=") {
                    let after_end = xmp_start + end_marker;
                    if let Some(close) = xml_for_parse[after_end..].find("?>") {
                        let xmp_end = after_end + close + 2; // include '?>'
                        let xmp_data = &xml_for_parse[xmp_start..xmp_end];
                        // Recursively parse the embedded XMP
                        let xmp_bytes = xmp_data.as_bytes().to_vec();
                        return XmpReader::read(&xmp_bytes);
                    }
                }
            }
            return Ok(tags);
        }

        // Check if this is RDF/XMP format or generic XML
        let is_rdf = xml_for_parse.contains("rdf:RDF") || xml_for_parse.contains("rdf:Description");
        if !is_rdf {
            // Generic XML: extract tags by building tag names from element paths
            return read_generic_xml(xml_for_parse);
        }

        // Pre-pass: collect rdf:nodeID → list values (for Bag/Seq with nodeIDs)
        let node_bags: std::collections::HashMap<String, Vec<String>> =
            collect_node_bag_values(xml_for_parse);

        // Pre-pass: collect all properties of blank nodes (rdf:nodeID Descriptions)
        // Maps nodeID → Vec<(ns_uri, local_name, value)>
        let blank_node_props: std::collections::HashMap<String, Vec<(String, String, String)>> =
            collect_blank_node_properties(xml_for_parse);

        // Pre-pass: find nodeIDs that are "inline referenced" — i.e., they appear as
        // <rdf:Description rdf:nodeID="X"> INSIDE another property element (not at the RDF top level).
        // These blank nodes should suppress direct property emission from top-level descriptions.
        let inline_referenced_node_ids: std::collections::HashSet<String> =
            collect_inline_referenced_node_ids(xml_for_parse);

        let parser = EventReader::from_str(xml_for_parse);
        let mut path: Vec<(String, String)> = Vec::new(); // (namespace, local_name)
        let mut current_text = String::new();
        let mut in_rdf_li = false;
        let mut list_values: Vec<String> = Vec::new();
        // Track depths where we should emit even with empty text (ExifTool et:id format)
        let mut emit_empty_depths: std::collections::HashSet<usize> =
            std::collections::HashSet::new();
        // Track elements with rdf:parseType='Resource' (bare structs).
        // Each entry is the path depth at which we entered such an element.
        let mut parse_resource_depths: Vec<usize> = Vec::new();

        // Blank node tracking: when we enter a <rdf:Description rdf:nodeID="X"> inside a property,
        // track the nodeID and parent property so we can emit all blank node props on close.
        // Stack of (nodeID, parent_local_name) for inline blank node Descriptions.
        let mut inline_blank_node_stack: Vec<(String, String)> = Vec::new();
        // Track depth of top-level blank-node Descriptions (parent is rdf:RDF or rdf:Description without property parent).
        // Properties inside these should NOT be emitted directly — only via blank node references.
        let mut suppress_direct_emit_depth: Option<usize> = None;

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
                                || a.name.namespace.as_deref()
                                    == Some("http://ns.exiftool.org/1.0/"))
                    });
                    if has_et_id {
                        emit_empty_depths.insert(path.len());
                    }

                    // Track rdf:parseType='Resource' (bare struct context)
                    let has_parse_resource = attributes.iter().any(|a| {
                        a.name.local_name == "parseType"
                            && (a.name.namespace.as_deref()
                                == Some("http://www.w3.org/1999/02/22-rdf-syntax-ns#")
                                || a.name.prefix.as_deref() == Some("rdf"))
                            && a.value == "Resource"
                    });
                    if has_parse_resource {
                        parse_resource_depths.push(path.len());
                    }

                    // x:xmpmeta or x:xapmeta — extract XMPToolkit from x:xmptk/x:xaptk attribute
                    if name.local_name == "xmpmeta" || name.local_name == "xapmeta" {
                        for attr in &attributes {
                            if attr.name.local_name == "xmptk" || attr.name.local_name == "xaptk" {
                                tags.push(Tag {
                                    id: TagId::Text("x:xmptk".into()),
                                    name: "XMPToolkit".into(),
                                    description: "XMP Toolkit".into(),
                                    group: TagGroup {
                                        family0: "XMP".into(),
                                        family1: "XMP-x".into(),
                                        family2: "Other".into(),
                                    },
                                    raw_value: Value::String(attr.value.clone()),
                                    print_value: attr.value.clone(),
                                    priority: 0,
                                });
                            }
                        }
                    }

                    // Check if this element has a rdf:nodeID reference to a known bag/seq or blank node.
                    // E.g., <dc:subject rdf:nodeID="anon2"/> — emit the bag values as a tag.
                    // E.g., <ph:tester rdf:nodeID="abc"/> — emit all blank node properties as TesterXxx.
                    // This is for non-Description elements that reference a nodeID bag/blank-node.
                    if name.local_name != "Description"
                        && name.local_name != "RDF"
                        && name.namespace.as_deref()
                            != Some("http://www.w3.org/1999/02/22-rdf-syntax-ns#")
                    {
                        if let Some(node_ref) = attributes.iter().find(|a| {
                            a.name.local_name == "nodeID"
                                && (a.name.prefix.as_deref() == Some("rdf")
                                    || a.name.namespace.as_deref()
                                        == Some("http://www.w3.org/1999/02/22-rdf-syntax-ns#"))
                        }) {
                            let node_id = &node_ref.value;
                            if let Some(bag_values) = node_bags.get(node_id) {
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
                                    Value::List(
                                        bag_values
                                            .iter()
                                            .map(|s| Value::String(s.clone()))
                                            .collect(),
                                    )
                                };
                                let pv = value.to_display_string();
                                tags.push(Tag {
                                    id: TagId::Text(format!(
                                        "{}:{}",
                                        group_prefix, name.local_name
                                    )),
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
                            // Blank node properties: emit all properties prefixed with this element's name
                            if let Some(bn_props) = blank_node_props.get(node_id) {
                                let elem_ns = name.namespace.as_deref().unwrap_or("");
                                let elem_prefix_ns = namespace_prefix(elem_ns);
                                let elem_group = if elem_prefix_ns.is_empty() {
                                    name.prefix.as_deref().unwrap_or("XMP")
                                } else {
                                    elem_prefix_ns
                                };
                                let parent_uc = ucfirst(&strip_non_ascii(&name.local_name));
                                // Build prefix from ancestor path + this element
                                let anc_prefix =
                                    build_struct_tag_prefix_without_last(&path, &name.local_name);
                                let elem_flat = if anc_prefix.is_empty() {
                                    parent_uc.clone()
                                } else {
                                    let stripped = strip_struct_prefix(&anc_prefix, &parent_uc);
                                    format!("{}{}", anc_prefix, stripped)
                                };
                                for (prop_ns, prop_local, prop_val) in bn_props {
                                    let prop_prefix = namespace_prefix(prop_ns);
                                    let prop_group = if prop_prefix.is_empty() {
                                        elem_group
                                    } else {
                                        prop_prefix
                                    };
                                    let prop_cat = namespace_category(prop_group);
                                    let prop_uc = ucfirst(&strip_non_ascii(prop_local));
                                    let stripped = strip_struct_prefix(&elem_flat, &prop_uc);
                                    let flat_raw = format!("{}{}", elem_flat, stripped);
                                    let flat = apply_flat_name_remap(&flat_raw).to_string();
                                    tags.push(Tag {
                                        id: TagId::Text(format!("{}:{}", prop_group, flat)),
                                        name: flat.clone(),
                                        description: flat,
                                        group: TagGroup {
                                            family0: "XMP".into(),
                                            family1: format!("XMP-{}", prop_group),
                                            family2: prop_cat.into(),
                                        },
                                        raw_value: Value::String(prop_val.clone()),
                                        print_value: prop_val.clone(),
                                        priority: 0,
                                    });
                                }
                            }
                        }
                    }

                    // Handle rdf:resource attribute on property elements (RDF/XML shorthand).
                    // E.g., <rdfs:seeAlso rdf:resource='plus:Licensee'/> → SeeAlso = plus:Licensee
                    // This is like a simple text value but provided via rdf:resource attribute.
                    // Skip if inside a suppressed blank-node Description.
                    let in_suppressed_bn = suppress_direct_emit_depth
                        .map(|d| path.len() > d)
                        .unwrap_or(false);
                    if name.local_name != "Description"
                        && name.local_name != "RDF"
                        && !in_suppressed_bn
                        && name.namespace.as_deref()
                            != Some("http://www.w3.org/1999/02/22-rdf-syntax-ns#")
                    {
                        if let Some(res_attr) = attributes.iter().find(|a| {
                            a.name.local_name == "resource"
                                && (a.name.prefix.as_deref() == Some("rdf")
                                    || a.name.namespace.as_deref()
                                        == Some("http://www.w3.org/1999/02/22-rdf-syntax-ns#"))
                        }) {
                            let resource_val = res_attr.value.clone();
                            let ns_uri = name.namespace.as_deref().unwrap_or("");
                            let prefix = namespace_prefix(ns_uri);
                            let group_prefix = if prefix.is_empty() {
                                name.prefix.as_deref().unwrap_or("XMP")
                            } else {
                                prefix
                            };
                            let category = namespace_category(group_prefix);
                            // Build full tag name using ancestor path
                            let remapped = remap_xmp_tag_name(group_prefix, &name.local_name);
                            let full_name = if !parse_resource_depths.is_empty() {
                                let ancestor_prefix =
                                    build_struct_tag_prefix_without_last(&path, &name.local_name);
                                if !ancestor_prefix.is_empty() {
                                    let field_stripped =
                                        strip_struct_prefix(&ancestor_prefix, &remapped);
                                    let candidate =
                                        format!("{}{}", ancestor_prefix, field_stripped);
                                    apply_flat_name_remap(&candidate).to_string()
                                } else {
                                    apply_flat_name_remap(&remapped).to_string()
                                }
                            } else {
                                apply_flat_name_remap(&remapped).to_string()
                            };
                            tags.push(Tag {
                                id: TagId::Text(format!("{}:{}", group_prefix, name.local_name)),
                                name: full_name.clone(),
                                description: full_name,
                                group: TagGroup {
                                    family0: "XMP".into(),
                                    family1: format!("XMP-{}", group_prefix),
                                    family2: category.into(),
                                },
                                raw_value: Value::String(resource_val.clone()),
                                print_value: resource_val,
                                priority: 0,
                            });
                        }
                    }

                    // Pre-check: is this a top-level nodeID Description that should suppress direct emission?
                    // Only suppress if the nodeID is also referenced inline (inside a property element).
                    let rdf_ns_check = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
                    let desc_node_id = attributes
                        .iter()
                        .find(|a| {
                            a.name.local_name == "nodeID"
                                && (a.name.prefix.as_deref() == Some("rdf")
                                    || a.name.namespace.as_deref() == Some(rdf_ns_check))
                        })
                        .map(|a| a.value.clone());
                    let is_top_level_blank_node_desc = name.local_name == "Description"
                        && name.namespace.as_deref() == Some(rdf_ns_check)
                        && desc_node_id
                            .as_ref()
                            .map(|nid| inline_referenced_node_ids.contains(nid.as_str()))
                            .unwrap_or(false)
                        && path
                            .iter()
                            .rev()
                            .nth(1)
                            .map(|(ns, ln)| {
                                ns == rdf_ns_check
                                    || ln == "RDF"
                                    || ln == "xmpmeta"
                                    || ln == "xapmeta"
                            })
                            .unwrap_or(true); // if no parent, treat as top-level

                    // Extract attributes on rdf:Description as tags
                    // e.g., <rdf:Description GCamera:HdrPlusMakernote="...">
                    // Skip if this is a top-level blank node Description (its attrs stored in blank_node_props).
                    if name.local_name == "Description" && !is_top_level_blank_node_desc {
                        for attr in &attributes {
                            // Emit rdf:about as "About" tag, skip xmlns
                            if attr.name.local_name == "about" {
                                if !attr.value.is_empty() {
                                    tags.push(Tag {
                                        id: TagId::Text("rdf:about".into()),
                                        name: "About".into(),
                                        description: "About".into(),
                                        group: TagGroup {
                                            family0: "XMP".into(),
                                            family1: "XMP-rdf".into(),
                                            family2: "Other".into(),
                                        },
                                        raw_value: Value::String(attr.value.clone()),
                                        print_value: attr.value.clone(),
                                        priority: 0,
                                    });
                                }
                                continue;
                            }
                            // Skip rdf:nodeID on Description (it's just an identifier, not a value)
                            if attr.name.local_name == "nodeID"
                                && (attr.name.prefix.as_deref() == Some("rdf")
                                    || attr.name.namespace.as_deref()
                                        == Some("http://www.w3.org/1999/02/22-rdf-syntax-ns#"))
                            {
                                continue;
                            }
                            if attr.name.prefix.as_deref() == Some("xmlns") {
                                continue;
                            }
                            if attr.name.local_name.starts_with("xmlns") {
                                continue;
                            }
                            // Skip ExifTool-internal attributes (et:toolkit, et:id, et:desc, etc.)
                            if attr.name.prefix.as_deref() == Some("et")
                                || attr.name.namespace.as_deref()
                                    == Some("http://ns.exiftool.org/1.0/")
                                || attr.name.namespace.as_deref()
                                    == Some("http://ns.exiftool.ca/1.0/")
                            {
                                continue;
                            }

                            let attr_ns = attr.name.namespace.as_deref().unwrap_or("");
                            let attr_prefix = namespace_prefix(attr_ns);
                            let group_prefix = if attr_prefix.is_empty() {
                                attr.name.prefix.as_deref().unwrap_or("XMP")
                            } else {
                                attr_prefix
                            };

                            {
                                // Special handling: GCamera:HdrPlusMakernote / GCamera:hdrp_makernote
                                // → emit HDRPlusMakerNote (binary) + decode HDRP sub-tags (non-empty only)
                                if !attr.value.is_empty()
                                    && group_prefix == "GCamera"
                                    && is_hdrp_makernote_attr(&attr.name.local_name)
                                {
                                    emit_hdrp_makernote(&attr.value, &mut tags);
                                    continue;
                                }

                                let category = namespace_category(group_prefix);
                                let remapped =
                                    remap_xmp_tag_name(group_prefix, &attr.name.local_name);
                                // If Description is inline inside a property element,
                                // prefix the tag with the property element's name.
                                // path = [..., parent_prop, Description] → parent_prop is rev().nth(1)
                                let rdf_ns2 = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
                                let full_name = if let Some(parent_elem) = path.iter().rev().nth(1)
                                {
                                    if parent_elem.0 != rdf_ns2
                                        && parent_elem.1 != "RDF"
                                        && parent_elem.1 != "xmpmeta"
                                        && parent_elem.1 != "xapmeta"
                                    {
                                        let parent_uc = ucfirst(&strip_non_ascii(&parent_elem.1));
                                        let field_uc =
                                            ucfirst(&strip_non_ascii(&attr.name.local_name));
                                        let stripped = strip_struct_prefix(&parent_uc, &field_uc);
                                        apply_flat_name_remap(&format!("{}{}", parent_uc, stripped))
                                            .to_string()
                                    } else {
                                        remapped
                                    }
                                } else {
                                    remapped
                                };
                                tags.push(Tag {
                                    id: TagId::Text(format!(
                                        "{}:{}",
                                        group_prefix, attr.name.local_name
                                    )),
                                    name: full_name.clone(),
                                    description: full_name,
                                    group: TagGroup {
                                        family0: "XMP".to_string(),
                                        family1: format!("XMP-{}", group_prefix),
                                        family2: category.to_string(),
                                    },
                                    raw_value: parse_xmp_value(&attr.value),
                                    print_value: attr.value.clone(),
                                    priority: 0,
                                });
                            }
                        }
                    }

                    // Track inline blank-node Descriptions: <rdf:Description rdf:nodeID="X"> inside a property.
                    // When this Description closes, we emit ALL blank node X properties with the parent prefix.
                    // Top-level blank-node Descriptions that are inline-referenced suppress direct emission.
                    // Top-level blank-node Descriptions NOT inline-referenced emit their props directly here.
                    let rdf_ns = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
                    if name.local_name == "Description" && name.namespace.as_deref() == Some(rdf_ns)
                    {
                        if let Some(nid_attr) = attributes.iter().find(|a| {
                            a.name.local_name == "nodeID"
                                && (a.name.prefix.as_deref() == Some("rdf")
                                    || a.name.namespace.as_deref() == Some(rdf_ns))
                        }) {
                            let nid = nid_attr.value.clone();
                            // Check if parent is a non-RDF property element (inline reference)
                            let parent_is_property = path
                                .iter()
                                .rev()
                                .nth(1)
                                .map(|(pns, pln)| {
                                    pns != rdf_ns
                                        && pln != "RDF"
                                        && pln != "xmpmeta"
                                        && pln != "xapmeta"
                                })
                                .unwrap_or(false);

                            if parent_is_property {
                                // Inline blank-node: parent property will claim these props
                                if let Some(parent) = path.iter().rev().nth(1) {
                                    inline_blank_node_stack.push((nid, parent.1.clone()));
                                }
                            } else if inline_referenced_node_ids.contains(nid.as_str()) {
                                // Top-level nodeID Description that IS referenced inline elsewhere:
                                // suppress direct emission here (props will be emitted when the inline ref is processed).
                                suppress_direct_emit_depth = Some(path.len());
                            } else {
                                // Top-level nodeID Description NOT referenced inline anywhere:
                                // emit all its blank-node properties directly now.
                                if let Some(bn_props) = blank_node_props.get(nid.as_str()) {
                                    for (prop_ns, prop_local, prop_val) in bn_props.clone() {
                                        let prop_prefix = namespace_prefix(&prop_ns);
                                        let prop_prefix = if prop_prefix.is_empty() {
                                            "XMP"
                                        } else {
                                            prop_prefix
                                        };
                                        let category = namespace_category(prop_prefix);
                                        let remapped = remap_xmp_tag_name(prop_prefix, &prop_local);
                                        tags.push(Tag {
                                            id: TagId::Text(format!(
                                                "{}:{}",
                                                prop_prefix, prop_local
                                            )),
                                            name: remapped.clone(),
                                            description: remapped,
                                            group: TagGroup {
                                                family0: "XMP".to_string(),
                                                family1: format!("XMP-{}", prop_prefix),
                                                family2: category.to_string(),
                                            },
                                            raw_value: parse_xmp_value(&prop_val),
                                            print_value: prop_val.clone(),
                                            priority: 0,
                                        });
                                    }
                                }
                                // Suppress the rest of the Description processing (already emitted)
                                suppress_direct_emit_depth = Some(path.len());
                            }
                        }
                    }

                    // Extract attributes on non-RDF struct elements (shorthand struct values).
                    // E.g., <exif:Flash exif:Fired="False" exif:Mode="2" .../>
                    //        <xapMM:DerivedFrom stRef:instanceID="..." stRef:documentID="..."/>
                    // These attributes are struct fields, flattened as ParentFieldName.
                    // Only apply when the element is NOT a Description and NOT an RDF structural element.
                    let is_rdf_structural = name.namespace.as_deref() == Some(rdf_ns)
                        || name.local_name == "Description"
                        || name.local_name == "RDF"
                        || name.local_name == "li"
                        || name.local_name == "Seq"
                        || name.local_name == "Bag"
                        || name.local_name == "Alt"
                        || name.local_name == "xmpmeta"
                        || name.local_name == "xapmeta"
                        || name.namespace.as_deref() == Some("adobe:ns:meta/");
                    if !is_rdf_structural && !attributes.is_empty() {
                        let elem_ns = name.namespace.as_deref().unwrap_or("");
                        let elem_prefix = namespace_prefix(elem_ns);
                        let elem_group = if elem_prefix.is_empty() {
                            name.prefix.as_deref().unwrap_or("XMP")
                        } else {
                            elem_prefix
                        };
                        // Build struct parent context: path of ancestor names BEFORE this element.
                        // path already includes the current element (just pushed), so we use
                        // build_struct_tag_prefix_without_last to exclude the current element.
                        let ancestors_prefix =
                            build_struct_tag_prefix_without_last(&path, &name.local_name);
                        let elem_uc = ucfirst(&strip_non_ascii(&name.local_name));
                        // Full path including this element: ancestors_prefix + elem_uc (with strip)
                        let elem_flat = if ancestors_prefix.is_empty() {
                            elem_uc.clone()
                        } else {
                            let stripped = strip_struct_prefix(&ancestors_prefix, &elem_uc);
                            format!("{}{}", ancestors_prefix, stripped)
                        };

                        for attr in &attributes {
                            // Skip xmlns, rdf:*, et:*, xml:* attributes
                            if attr.name.prefix.as_deref() == Some("xmlns") {
                                continue;
                            }
                            if attr.name.local_name.starts_with("xmlns") {
                                continue;
                            }
                            if attr.name.prefix.as_deref() == Some("rdf")
                                || attr.name.namespace.as_deref() == Some(rdf_ns)
                            {
                                continue;
                            }
                            if attr.name.prefix.as_deref() == Some("et")
                                || attr.name.namespace.as_deref()
                                    == Some("http://ns.exiftool.org/1.0/")
                                || attr.name.namespace.as_deref()
                                    == Some("http://ns.exiftool.ca/1.0/")
                            {
                                continue;
                            }
                            if attr.name.prefix.as_deref() == Some("xml")
                                || attr.name.namespace.as_deref()
                                    == Some("http://www.w3.org/XML/1998/namespace")
                            {
                                continue;
                            }

                            let attr_ns = attr.name.namespace.as_deref().unwrap_or("");
                            let attr_prefix_resolved = namespace_prefix(attr_ns);
                            let attr_group = if attr_prefix_resolved.is_empty() {
                                attr.name.prefix.as_deref().unwrap_or(elem_group)
                            } else {
                                attr_prefix_resolved
                            };
                            let field_uc = ucfirst(&strip_non_ascii(&attr.name.local_name));
                            // Build flattened name: elem_flat + field_stripped
                            let field_stripped = strip_struct_prefix(&elem_flat, &field_uc);
                            let flat_name_raw = format!("{}{}", elem_flat, field_stripped);
                            let flat_name = apply_flat_name_remap(&flat_name_raw).to_string();
                            let category = namespace_category(attr_group);
                            let pv = attr.value.clone();
                            tags.push(Tag {
                                id: TagId::Text(format!("{}:{}", attr_group, flat_name)),
                                name: flat_name.clone(),
                                description: flat_name,
                                group: TagGroup {
                                    family0: "XMP".into(),
                                    family1: format!("XMP-{}", elem_group),
                                    family2: category.into(),
                                },
                                raw_value: parse_xmp_value(&attr.value),
                                print_value: pv,
                                priority: 0,
                            });
                        }
                    }

                    // Detect GContainer:Directory/rdf:Seq entry
                    // Path looks like: [..., (GContainer_ns, "Directory"), (rdf_ns, "Seq")]
                    if name.local_name == "Seq"
                        && name.namespace.as_deref()
                            == Some("http://www.w3.org/1999/02/22-rdf-syntax-ns#")
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
                        && name.namespace.as_deref()
                            == Some("http://www.w3.org/1999/02/22-rdf-syntax-ns#")
                    {
                        in_gcontainer_li = true;
                        in_rdf_li = true;
                    } else if name.local_name == "li"
                        && name.namespace.as_deref()
                            == Some("http://www.w3.org/1999/02/22-rdf-syntax-ns#")
                    {
                        if in_lang_alt {
                            // Inner rdf:li inside rdf:Alt — track the xml:lang attribute
                            current_li_lang = attributes
                                .iter()
                                .find(|a| {
                                    a.name.local_name == "lang"
                                        && (a.name.prefix.as_deref() == Some("xml")
                                            || a.name.namespace.as_deref()
                                                == Some("http://www.w3.org/XML/1998/namespace"))
                                })
                                .map(|a| a.value.clone());
                        }
                        // Note: outer rdf:li increment happens when it closes (to correctly track which item we're on)
                        in_rdf_li = true;
                    }

                    // Detect rdf:Alt
                    if name.local_name == "Alt"
                        && name.namespace.as_deref()
                            == Some("http://www.w3.org/1999/02/22-rdf-syntax-ns#")
                    {
                        in_lang_alt = true;
                        // Check if we're inside an outer rdf:li (Bag item)
                        // Path ends with: ..., Bag, li, Alt (just pushed)
                        let depth = path.len();
                        if depth >= 3 {
                            let li_elem = &path[depth - 2]; // li (just before Alt)
                            let bag_elem = &path[depth - 3]; // Bag
                            if li_elem.1 == "li"
                                && li_elem.0 == "http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                                && bag_elem.1 == "Bag"
                                && bag_elem.0 == "http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                            {
                                lang_alt_in_bag = true;
                            }
                        }
                    }

                    // Inside a GContainer struct li: capture Container:Item attributes
                    // These are struct fields: Item:Mime, Item:Semantic, Item:Length
                    if in_gcontainer_li
                        && name.local_name == "Item"
                        && name.namespace.as_deref()
                            == Some("http://ns.google.com/photos/1.0/container/")
                    {
                        // Collect Item:Mime, Item:Semantic, Item:Length for this li entry
                        let mut found: std::collections::HashMap<String, String> =
                            std::collections::HashMap::new();
                        for attr in &attributes {
                            if attr.name.namespace.as_deref()
                                == Some("http://ns.google.com/photos/1.0/container/item/")
                            {
                                let field = ucfirst(&attr.name.local_name);
                                found.insert(field, attr.value.clone());
                            }
                        }
                        // Accumulate: for each known field, push value or empty string
                        // (so all lists stay aligned)
                        let known = ["Mime", "Semantic", "Length"];
                        for k in &known {
                            if let Some(v) = found.get(*k) {
                                gcontainer_fields
                                    .entry(k.to_string())
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
                        && name.namespace.as_deref()
                            == Some("http://www.w3.org/1999/02/22-rdf-syntax-ns#")
                    {
                        if in_lang_alt {
                            // Inner rdf:li inside rdf:Alt — store by lang
                            let lang = current_li_lang
                                .take()
                                .unwrap_or_else(|| "x-default".to_string());
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
                        && name.namespace.as_deref()
                            == Some("http://www.w3.org/1999/02/22-rdf-syntax-ns#")
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

                                // Check if this lang-alt field is inside a struct context.
                                // Either path[rev(2)] == rdf:li (e.g., CvTermName inside li)
                                // or we're inside any rdf:parseType="Resource" struct.
                                let rdf_ns = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
                                let in_struct_li_alt = !parse_resource_depths.is_empty()
                                    || path
                                        .iter()
                                        .rev()
                                        .nth(2)
                                        .map(|(ns, ln)| ln == "li" && ns == rdf_ns)
                                        .unwrap_or(false);
                                let (full_tag_name, emit_group_prefix, emit_category) =
                                    if in_struct_li_alt {
                                        // Use full ancestor path prefix (excluding current tag_name)
                                        // path currently has: ..., struct_ancestors..., li, tag_name, Alt
                                        // We want the prefix from ancestors before tag_name
                                        let ancestor_prefix =
                                            build_struct_tag_prefix_without_last(&path, &tag_name);
                                        let field_uc = ucfirst(&strip_non_ascii(&tag_name));
                                        if !ancestor_prefix.is_empty() {
                                            let stripped =
                                                strip_struct_prefix(&ancestor_prefix, &field_uc);
                                            let flat_raw =
                                                format!("{}{}", ancestor_prefix, stripped);
                                            let flat = apply_flat_name_remap(&flat_raw).to_string();
                                            // Find namespace from struct parent
                                            let sp_gp = path
                                                .iter()
                                                .rev()
                                                .skip(2)
                                                .skip_while(|(ns, ln)| {
                                                    ln == "li"
                                                        || ln == "Bag"
                                                        || ln == "Seq"
                                                        || ln == "Alt"
                                                        || ns == rdf_ns
                                                })
                                                .find(|(ns, ln)| {
                                                    ln != "Description" && ns != rdf_ns
                                                })
                                                .map(|(sp_ns, _)| {
                                                    let p = namespace_prefix(sp_ns);
                                                    if p.is_empty() {
                                                        group_prefix
                                                    } else {
                                                        p
                                                    }
                                                })
                                                .unwrap_or(group_prefix);
                                            let cat = namespace_category(sp_gp);
                                            (flat, sp_gp.to_string(), cat.to_string())
                                        } else {
                                            (
                                                ucfirst(&strip_non_ascii(&tag_name)),
                                                group_prefix.to_string(),
                                                category.to_string(),
                                            )
                                        }
                                    } else {
                                        let tn = apply_flat_name_remap(&ucfirst(&strip_non_ascii(
                                            &tag_name,
                                        )))
                                        .to_string();
                                        (tn, group_prefix.to_string(), category.to_string())
                                    };

                                // Emit x-default as main tag
                                // Only emit if there's at least one non-empty value
                                let has_nonempty = list_values.iter().any(|s| !s.is_empty());
                                if !list_values.is_empty() && has_nonempty {
                                    let main_val = if list_values.len() == 1 {
                                        Value::String(list_values[0].clone())
                                    } else {
                                        Value::List(
                                            list_values
                                                .iter()
                                                .map(|s| Value::String(s.clone()))
                                                .collect(),
                                        )
                                    };
                                    let pv = main_val.to_display_string();
                                    tags.push(Tag {
                                        id: TagId::Text(format!(
                                            "{}:{}",
                                            emit_group_prefix, tag_name
                                        )),
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
                                }
                                list_values.clear();
                                // Emit per-lang variants as TagName-lang
                                let mut lang_keys: Vec<String> =
                                    bag_lang_values.keys().cloned().collect();
                                lang_keys.sort();
                                for lang in &lang_keys {
                                    let vals = &bag_lang_values[lang];
                                    let non_none: Vec<String> =
                                        vals.iter().filter_map(|v| v.clone()).collect();
                                    if !non_none.is_empty() {
                                        let lang_tag = format!("{}-{}", full_tag_name, lang);
                                        let val = if non_none.len() == 1 {
                                            Value::String(non_none[0].clone())
                                        } else {
                                            Value::List(
                                                non_none
                                                    .iter()
                                                    .map(|s| Value::String(s.clone()))
                                                    .collect(),
                                            )
                                        };
                                        let pv = val.to_display_string();
                                        tags.push(Tag {
                                            id: TagId::Text(format!(
                                                "{}-{}:{}",
                                                emit_group_prefix, lang, tag_name
                                            )),
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
                            // Find the parent property name using full ancestor path
                            if let Some(parent) = path.iter().rev().nth(1) {
                                let prefix = namespace_prefix(&parent.0);
                                let tag_name = parent.1.clone();
                                let group_prefix = if prefix.is_empty() { "XMP" } else { prefix };
                                let category = namespace_category(group_prefix);

                                // Build full flat name from ancestor path
                                let ancestor_prefix =
                                    build_struct_tag_prefix_without_last(&path, &tag_name);
                                let field_uc = ucfirst(&strip_non_ascii(&tag_name));
                                let full_flat_base = if !ancestor_prefix.is_empty() {
                                    let stripped = strip_struct_prefix(&ancestor_prefix, &field_uc);
                                    let raw = format!("{}{}", ancestor_prefix, stripped);
                                    apply_flat_name_remap(&raw).to_string()
                                } else {
                                    apply_flat_name_remap(&field_uc).to_string()
                                };

                                // Collect all language codes (maintaining insertion order: x-default first)
                                let mut lang_keys: Vec<String> =
                                    bag_lang_values.keys().cloned().collect();
                                // Put x-default first
                                lang_keys.sort_by(|a, b| {
                                    if a == "x-default" {
                                        std::cmp::Ordering::Less
                                    } else if b == "x-default" {
                                        std::cmp::Ordering::Greater
                                    } else {
                                        a.cmp(b)
                                    }
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
                                    let joined: String = vals
                                        .iter()
                                        .filter_map(|v| v.as_deref()) // None filtered out
                                        .collect::<Vec<_>>()
                                        .join(", ");

                                    // Only emit if there's something meaningful
                                    let has_content = vals.iter().any(|v| v.is_some());
                                    if !has_content {
                                        continue;
                                    }

                                    let (tag_key, tag_display) = if is_default {
                                        (full_flat_base.clone(), full_flat_base.clone())
                                    } else {
                                        let lt = format!("{}-{}", full_flat_base, lang);
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
                                    Value::List(
                                        values.iter().map(|s| Value::String(s.clone())).collect(),
                                    )
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
                                if tag_name == "RDF"
                                    || tag_name == "xmpmeta"
                                    || tag_name == "xapmeta"
                                    || parent.0 == "http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                                    || parent.0 == "adobe:ns:meta/"
                                {
                                    list_values.clear();
                                    path.pop();
                                    current_text.clear();
                                    continue;
                                }
                                let group_prefix = if prefix.is_empty() { "XMP" } else { prefix };
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

                                // Use full ancestor path for struct flattening
                                let ancestor_prefix =
                                    build_struct_tag_prefix_without_last(&path, tag_name);
                                let field_uc = ucfirst(&strip_non_ascii(tag_name));
                                let (full_name, emit_group_prefix) = if !ancestor_prefix.is_empty()
                                {
                                    let field_stripped =
                                        strip_struct_prefix(&ancestor_prefix, &field_uc);
                                    let raw = format!("{}{}", ancestor_prefix, field_stripped);
                                    let flat = apply_flat_name_remap(&raw).to_string();
                                    // Find namespace from the outermost struct ancestor
                                    let sp_gp = path
                                        .iter()
                                        .rev()
                                        .skip(1) // skip current list element (Seq/Bag/Alt)
                                        .skip(1) // skip tag_name
                                        .skip_while(|(ns, ln)| {
                                            ln == "li"
                                                || ln == "Bag"
                                                || ln == "Seq"
                                                || ln == "Alt"
                                                || ns
                                                    == "http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                                        })
                                        .find(|(ns, ln)| {
                                            ln != "Description"
                                                && ns
                                                    != "http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                                        })
                                        .map(|(sp_ns, _)| {
                                            let p = namespace_prefix(sp_ns);
                                            if p.is_empty() {
                                                group_prefix
                                            } else {
                                                p
                                            }
                                        })
                                        .unwrap_or(group_prefix);
                                    (flat, sp_gp.to_string())
                                } else {
                                    let flat = apply_flat_name_remap(&field_uc).to_string();
                                    (flat, group_prefix.to_string())
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
                    // Perl flattens as "{FullPathPrefix}{FieldName}" → "JobRefName"
                    if !normalize_xml_text(&current_text).is_empty()
                        && in_rdf_li
                        && name.namespace.as_deref()
                            != Some("http://www.w3.org/1999/02/22-rdf-syntax-ns#")
                        && name.local_name != "Description"
                    {
                        // Build the full ancestor prefix for struct flattening,
                        // excluding the current element (which is the field itself).
                        let ancestor_prefix =
                            build_struct_tag_prefix_without_last(&path, &name.local_name);
                        if !ancestor_prefix.is_empty() {
                            let field_local = ucfirst(&strip_non_ascii(&name.local_name));
                            let field_stripped =
                                strip_struct_prefix(&ancestor_prefix, &field_local);
                            let flat_name_raw = format!("{}{}", ancestor_prefix, field_stripped);
                            let flat_name = apply_flat_name_remap(&flat_name_raw).to_string();
                            let prefix = namespace_prefix(ns_uri);
                            let group_prefix = if prefix.is_empty() { "XMP" } else { prefix };
                            let category = namespace_category(group_prefix);
                            tags.push(Tag {
                                id: TagId::Text(format!("{}:{}", group_prefix, flat_name)),
                                name: flat_name.clone(),
                                description: flat_name,
                                group: TagGroup {
                                    family0: "XMP".into(),
                                    family1: format!("XMP-{}", group_prefix),
                                    family2: category.into(),
                                },
                                raw_value: parse_xmp_value(&normalize_xml_text(&current_text)),
                                print_value: normalize_xml_text(&current_text),
                                priority: 0,
                            });
                        }
                        path.pop();
                        current_text.clear();
                        continue;
                    }

                    // Simple property with text content (or explicitly empty with et:id)
                    // Skip emission when inside a top-level blank-node Description (suppress_direct_emit_depth).
                    let has_et_depth = emit_empty_depths.contains(&path.len());
                    let in_suppressed_blank_node = suppress_direct_emit_depth
                        .map(|d| path.len() > d)
                        .unwrap_or(false);
                    if (!normalize_xml_text(&current_text).is_empty() || has_et_depth)
                        && !in_rdf_li
                        && !in_suppressed_blank_node
                    {
                        let prefix = namespace_prefix(ns_uri);
                        let tag_name = &name.local_name;

                        // Skip RDF structural elements
                        if tag_name != "Description"
                            && name.namespace.as_deref()
                                != Some("http://www.w3.org/1999/02/22-rdf-syntax-ns#")
                        {
                            let group_prefix = if prefix.is_empty() { "XMP" } else { prefix };
                            let category = namespace_category(group_prefix);

                            let text_val = normalize_xml_text(&current_text);
                            let value = parse_xmp_value(&text_val);
                            let print_value = value.to_display_string();

                            // Build full ancestor path for struct flattening.
                            // Applies when inside rdf:parseType="Resource" structs OR when
                            // nested inside a property element (e.g., blank-node Description).
                            let remapped = remap_xmp_tag_name(group_prefix, tag_name);
                            let ancestor_prefix =
                                build_struct_tag_prefix_without_last(&path, tag_name);
                            let full_name = if !ancestor_prefix.is_empty() {
                                let field_stripped =
                                    strip_struct_prefix(&ancestor_prefix, &remapped);
                                let candidate = format!("{}{}", ancestor_prefix, field_stripped);
                                apply_flat_name_remap(&candidate).to_string()
                            } else {
                                apply_flat_name_remap(&remapped).to_string()
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

                    // If closing an inline blank-node Description, emit ALL blank node properties
                    // prefixed with the parent property element's name.
                    // Also clear suppress_direct_emit_depth when the top-level nodeID Description closes.
                    if name.local_name == "Description"
                        && name.namespace.as_deref()
                            == Some("http://www.w3.org/1999/02/22-rdf-syntax-ns#")
                    {
                        // Clear suppression if this Description was the suppressed one
                        if suppress_direct_emit_depth == Some(path.len()) {
                            suppress_direct_emit_depth = None;
                        }

                        if let Some((node_id, parent_local)) =
                            inline_blank_node_stack.last().cloned()
                        {
                            // Check this Description is closing (path should end with Description)
                            if path
                                .last()
                                .map(|(_, ln)| ln == "Description")
                                .unwrap_or(false)
                            {
                                inline_blank_node_stack.pop();
                                if let Some(bn_props) = blank_node_props.get(&node_id) {
                                    let parent_uc = ucfirst(&strip_non_ascii(&parent_local));
                                    // Find the parent element's namespace to get group prefix
                                    let parent_ns = path
                                        .iter()
                                        .rev()
                                        .nth(1)
                                        .map(|(ns, _)| ns.as_str())
                                        .unwrap_or("");
                                    let parent_prefix_ns = namespace_prefix(parent_ns);
                                    let parent_group = if parent_prefix_ns.is_empty() {
                                        "XMP"
                                    } else {
                                        parent_prefix_ns
                                    };
                                    for (prop_ns, prop_local, prop_val) in bn_props {
                                        let prop_prefix = namespace_prefix(prop_ns);
                                        let prop_group = if prop_prefix.is_empty() {
                                            parent_group
                                        } else {
                                            prop_prefix
                                        };
                                        let prop_cat = namespace_category(prop_group);
                                        let prop_uc = ucfirst(&strip_non_ascii(prop_local));
                                        let stripped = strip_struct_prefix(&parent_uc, &prop_uc);
                                        let flat_raw = format!("{}{}", parent_uc, stripped);
                                        let flat = apply_flat_name_remap(&flat_raw).to_string();
                                        // Avoid duplicates (e.g., properties already emitted inline)
                                        if !tags.iter().any(|t| t.name == flat) {
                                            tags.push(Tag {
                                                id: TagId::Text(format!("{}:{}", prop_group, flat)),
                                                name: flat.clone(),
                                                description: flat,
                                                group: TagGroup {
                                                    family0: "XMP".into(),
                                                    family1: format!("XMP-{}", prop_group),
                                                    family2: prop_cat.into(),
                                                },
                                                raw_value: Value::String(prop_val.clone()),
                                                print_value: prop_val.clone(),
                                                priority: 0,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }

                    path.pop();
                    current_text.clear();
                }
                Err(_) => continue,
                _ => {}
            }
        }

        // Post-processing: emit GainMap Warning if DirectoryItemSemantic contains "GainMap"
        let has_gainmap = tags
            .iter()
            .any(|t| t.name == "DirectoryItemSemantic" && t.print_value.contains("GainMap"));
        if has_gainmap {
            // Find DirectoryItemMime and DirectoryItemLength for the GainMap entry
            // Emit warning about GainMap image/jpeg not found in trailer
            let gainmap_mime = tags
                .iter()
                .find(|t| t.name == "DirectoryItemSemantic")
                .and_then(|t| {
                    // Find the semantic that is GainMap and get the corresponding Mime
                    // For simplicity, look for GainMap in the values
                    if let Value::List(ref items) = t.raw_value {
                        items
                            .iter()
                            .enumerate()
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
                            Value::String(s) => {
                                if idx == 0 {
                                    Some(s.clone())
                                } else {
                                    None
                                }
                            }
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

        // Post-processing: compute Flash composite from FlashFired/Return/Mode/Function/RedEyeMode
        // This mirrors ExifTool's XMP Composite Flash tag.
        if !tags.iter().any(|t| t.name == "Flash") {
            let get_bool = |name: &str| -> Option<bool> {
                tags.iter()
                    .find(|t| t.name == name)
                    .map(|t| t.print_value.to_lowercase() == "true")
            };
            let get_int = |name: &str| -> Option<u32> {
                tags.iter()
                    .find(|t| t.name == name)
                    .and_then(|t| t.print_value.parse::<u32>().ok())
            };
            let flash_fired = get_bool("FlashFired");
            let flash_return = get_int("FlashReturn");
            let flash_mode = get_int("FlashMode");
            let flash_function = get_bool("FlashFunction");
            let flash_red_eye = get_bool("FlashRedEyeMode");
            // Only emit if we have at least one relevant tag
            if flash_fired.is_some()
                || flash_return.is_some()
                || flash_mode.is_some()
                || flash_function.is_some()
                || flash_red_eye.is_some()
            {
                let val: u32 = (if flash_fired.unwrap_or(false) {
                    0x01
                } else {
                    0
                }) | ((flash_return.unwrap_or(0) & 0x03) << 1)
                    | ((flash_mode.unwrap_or(0) & 0x03) << 3)
                    | (if flash_function.unwrap_or(false) {
                        0x20
                    } else {
                        0
                    })
                    | (if flash_red_eye.unwrap_or(false) {
                        0x40
                    } else {
                        0
                    });
                let flash_str = flash_numeric_to_string(val);
                tags.push(Tag {
                    id: TagId::Text("Flash:Flash".into()),
                    name: "Flash".into(),
                    description: "Flash".into(),
                    group: TagGroup {
                        family0: "XMP".into(),
                        family1: "XMP-exif".into(),
                        family2: "Camera".into(),
                    },
                    raw_value: Value::String(format!("{}", val)),
                    print_value: flash_str,
                    priority: 0,
                });
            }
        }

        // Post-processing: aggregate duplicate tag names (same name, different values)
        // into a single tag with comma-joined print_value. This matches ExifTool behavior
        // where repeated struct properties (e.g. in Bag/Seq items) are combined into one tag.
        // For exact duplicate names, keep only the first instance but join all values.
        let tags = aggregate_duplicate_xmp_tags(tags);

        Ok(tags)
    }
}

/// Aggregate tags with the same name into a single tag with comma-joined values.
/// The first occurrence is kept; subsequent occurrences with the same name have their
/// values appended to the first occurrence (comma-separated).
fn aggregate_duplicate_xmp_tags(tags: Vec<Tag>) -> Vec<Tag> {
    let mut result: Vec<Tag> = Vec::with_capacity(tags.len());
    let mut name_to_idx: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    for tag in tags {
        if let Some(&idx) = name_to_idx.get(&tag.name) {
            // Aggregate: append value to the existing tag
            let existing = &mut result[idx];
            if existing.print_value != tag.print_value {
                existing.print_value = format!("{}, {}", existing.print_value, tag.print_value);
            }
            // Don't push a new entry
        } else {
            let idx = result.len();
            name_to_idx.insert(tag.name.clone(), idx);
            result.push(tag);
        }
    }
    result
}

/// Convert numeric flash value to ExifTool flash description string.
fn flash_numeric_to_string(val: u32) -> String {
    match val {
        0x00 => "No Flash".into(),
        0x01 => "Fired".into(),
        0x05 => "Fired, Return not detected".into(),
        0x07 => "Fired, Return detected".into(),
        0x08 => "On, Did not fire".into(),
        0x09 => "On, Fired".into(),
        0x0d => "On, Return not detected".into(),
        0x0f => "On, Return detected".into(),
        0x10 => "Off, Did not fire".into(),
        0x14 => "Off, Did not fire, Return not detected".into(),
        0x18 => "Auto, Did not fire".into(),
        0x19 => "Auto, Fired".into(),
        0x1d => "Auto, Fired, Return not detected".into(),
        0x1f => "Auto, Fired, Return detected".into(),
        0x20 => "No flash function".into(),
        0x30 => "Off, No flash function".into(),
        0x41 => "Fired, Red-eye reduction".into(),
        0x45 => "Fired, Red-eye reduction, Return not detected".into(),
        0x47 => "Fired, Red-eye reduction, Return detected".into(),
        0x49 => "On, Red-eye reduction".into(),
        0x4d => "On, Red-eye reduction, Return not detected".into(),
        0x4f => "On, Red-eye reduction, Return detected".into(),
        0x50 => "Off, Red-eye reduction".into(),
        0x58 => "Auto, Did not fire, Red-eye reduction".into(),
        0x59 => "Auto, Fired, Red-eye reduction".into(),
        0x5d => "Auto, Fired, Red-eye reduction, Return not detected".into(),
        0x5f => "Auto, Fired, Red-eye reduction, Return detected".into(),
        _ => format!("Unknown (0x{:02x})", val),
    }
}

/// Like build_struct_tag_prefix but excludes the element with the given local name
/// (used when path already includes the current element but we want the prefix without it).
fn build_struct_tag_prefix_without_last(path: &[(String, String)], exclude_ln: &str) -> String {
    build_struct_tag_prefix_excluding(path, Some(exclude_ln))
}

fn build_struct_tag_prefix_excluding(
    path: &[(String, String)],
    exclude_last: Option<&str>,
) -> String {
    let rdf_ns = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
    let mut result = String::new();
    let effective_path: &[(String, String)] = if let Some(excl) = exclude_last {
        // Find the last occurrence of excl in path and trim there
        let mut end = path.len();
        for i in (0..path.len()).rev() {
            if path[i].1 == excl {
                end = i;
                break;
            }
        }
        &path[..end]
    } else {
        path
    };
    for (ns, ln) in effective_path {
        if ns == rdf_ns
            || ln == "Description"
            || ln == "RDF"
            || ln == "xmpmeta"
            || ln == "xapmeta"
            || ns == "adobe:ns:meta/"
        {
            continue;
        }
        // Skip rdf list/struct elements
        if ln == "Seq" || ln == "Bag" || ln == "Alt" || ln == "li" {
            continue;
        }
        let part = ucfirst(&strip_non_ascii(ln));
        if result.is_empty() {
            result = part;
        } else {
            // Strip prefix overlap between result suffix and part
            let stripped = strip_struct_prefix(&result, &part);
            result = format!("{}{}", result, stripped);
        }
    }
    result
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
    // First strip non-ASCII characters from the local name (mirrors Perl byte-mode behavior)
    let clean_name = strip_non_ascii(local_name);
    let local_name = clean_name.as_str();
    match (group_prefix, local_name) {
        // tiff: namespace remappings
        ("tiff", "ImageLength") => "ImageHeight".into(),
        ("tiff", "BitsPerSample") => "BitsPerSample".into(),
        // exif: namespace remappings
        ("exif", "PixelXDimension") => "ExifImageWidth".into(),
        ("exif", "PixelYDimension") => "ExifImageHeight".into(),
        ("exif", "ExposureBiasValue") => "ExposureCompensation".into(),
        // photoshop: namespace remappings
        ("photoshop", "ICCProfile") => "ICCProfileName".into(),
        ("photoshop", "ColorMode") => "ColorMode".into(),
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

/// Apply known struct flat-name remappings.
/// ExifTool defines pre-computed flat tag names for well-known structs.
/// E.g., ArtworkOrObjectAOTitle → ArtworkTitle, KeywordsHierarchyKeyword → HierarchicalKeywords1.
/// Apply flat name remap that may involve dynamic prefix substitution.
/// This converts concatenated property-path names to their ExifTool tag Names.
fn apply_flat_name_remap(name: &str) -> String {
    // Dynamic prefix substitutions first (can't be done in a simple match)
    // MWG Regions Extensions: RegionsRegionListExtensions* → RegionExtensions*
    // Then apply any further remappings to the suffix (e.g., ArtworkOrObject → Artwork)
    if let Some(rest) = name.strip_prefix("RegionsRegionListExtensions") {
        let remapped_rest = apply_flat_name_remap(rest);
        return format!("RegionExtensions{}", remapped_rest);
    }

    let mapped = match name {
        // IPTC Extension: ArtworkOrObject struct
        "ArtworkOrObjectAOCopyrightNotice" => "ArtworkCopyrightNotice",
        "ArtworkOrObjectAOCreator" => "ArtworkCreator",
        "ArtworkOrObjectAODateCreated" => "ArtworkDateCreated",
        "ArtworkOrObjectAOSource" => "ArtworkSource",
        "ArtworkOrObjectAOSourceInvNo" => "ArtworkSourceInventoryNo",
        "ArtworkOrObjectAOTitle" => "ArtworkTitle",
        "ArtworkOrObjectAOCurrentCopyrightOwnerName" => "ArtworkCopyrightOwnerName",
        "ArtworkOrObjectAOCurrentCopyrightOwnerId" => "ArtworkCopyrightOwnerID",
        "ArtworkOrObjectAOCurrentLicensorName" => "ArtworkLicensorName",
        "ArtworkOrObjectAOCurrentLicensorId" => "ArtworkLicensorID",
        "ArtworkOrObjectAOCreatorId" => "ArtworkCreatorID",
        "ArtworkOrObjectAOCircaDateCreated" => "ArtworkCircaDateCreated",
        "ArtworkOrObjectAOStylePeriod" => "ArtworkStylePeriod",
        "ArtworkOrObjectAOSourceInvURL" => "ArtworkSourceInvURL",
        "ArtworkOrObjectAOContentDescription" => "ArtworkContentDescription",
        "ArtworkOrObjectAOContributionDescription" => "ArtworkContributionDescription",
        "ArtworkOrObjectAOPhysicalDescription" => "ArtworkPhysicalDescription",
        // MWG Regions: Regions struct flat names (property path → Name)
        "RegionsRegionListName" => "RegionName",
        "RegionsRegionListType" => "RegionType",
        "RegionsRegionListDescription" => "RegionDescription",
        "RegionsRegionListFocusUsage" => "RegionFocusUsage",
        "RegionsRegionListBarCodeValue" => "RegionBarCodeValue",
        "RegionsRegionListSeeAlso" => "RegionSeeAlso",
        "RegionsRegionListRotation" => "RegionRotation",
        "RegionsRegionListAreaH" => "RegionAreaH",
        "RegionsRegionListAreaW" => "RegionAreaW",
        "RegionsRegionListAreaX" => "RegionAreaX",
        "RegionsRegionListAreaY" => "RegionAreaY",
        "RegionsRegionListAreaD" => "RegionAreaD",
        "RegionsRegionListAreaUnit" => "RegionAreaUnit",
        // MWG Keywords: Keywords struct flat names
        "KeywordsHierarchyKeyword" => "HierarchicalKeywords1",
        "KeywordsHierarchyChildrenKeyword" => "HierarchicalKeywords2",
        "KeywordsHierarchyChildrenChildrenKeyword" => "HierarchicalKeywords3",
        "KeywordsHierarchyChildrenChildrenChildrenKeyword" => "HierarchicalKeywords4",
        "KeywordsHierarchyChildrenChildrenChildrenChildrenKeyword" => "HierarchicalKeywords5",
        "KeywordsHierarchyChildrenChildrenChildrenChildrenChildrenKeyword" => {
            "HierarchicalKeywords6"
        }
        // xmpTPg: Colorants struct (FlatName => 'Colorant')
        "ColorantsSwatchName" => "ColorantSwatchName",
        "ColorantsMode" => "ColorantMode",
        "ColorantsType" => "ColorantType",
        "ColorantsCyan" => "ColorantCyan",
        "ColorantsMagenta" => "ColorantMagenta",
        "ColorantsYellow" => "ColorantYellow",
        "ColorantsBlack" => "ColorantBlack",
        "ColorantsGray" => "ColorantGray",
        "ColorantsRed" => "ColorantRed",
        "ColorantsGreen" => "ColorantGreen",
        "ColorantsBlue" => "ColorantBlue",
        "ColorantsL" => "ColorantL",
        "ColorantsA" => "ColorantA",
        "ColorantsB" => "ColorantB",
        // xmpTPg: Fonts struct (FlatName => '' → fields stand alone)
        "FontsFontName" => "FontName",
        "FontsFontFamily" => "FontFamily",
        "FontsFontFace" => "FontFace",
        "FontsFontType" => "FontType",
        "FontsVersionString" => "FontVersion",
        "FontsComposite" => "FontComposite",
        "FontsFontFileName" => "FontFileName",
        // xmp: Thumbnails struct
        "ThumbnailsFormat" => "ThumbnailFormat",
        "ThumbnailsWidth" => "ThumbnailWidth",
        "ThumbnailsHeight" => "ThumbnailHeight",
        "ThumbnailsImage" => "ThumbnailImage",
        _ => name,
    };
    mapped.to_string()
}

/// Parse an XMP text value into the appropriate Value type.
/// XMP rational values (e.g., "28/10", "5800/1000") are stored as Value::URational
/// so that composite computation can parse them as f64.
fn parse_xmp_value(text: &str) -> Value {
    // Try rational: N/D where N and D are integers (no whitespace)
    if let Some(slash) = text.find('/') {
        let num_str = &text[..slash];
        let den_str = &text[slash + 1..];
        if !num_str.is_empty()
            && !den_str.is_empty()
            && !num_str.contains(' ')
            && !den_str.contains(' ')
        {
            if let (Ok(n), Ok(d)) = (num_str.parse::<i64>(), den_str.parse::<u64>()) {
                if d > 0 {
                    if n >= 0 {
                        return Value::URational(n as u32, d as u32);
                    } else {
                        return Value::IRational(n as i32, d as i32);
                    }
                }
            }
        }
    }
    Value::String(text.to_string())
}

fn ucfirst(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

/// Strip non-ASCII characters from a tag name component.
/// Perl XMP.pm works in byte-string mode where tag names extracted from XML
/// may contain raw UTF-8 bytes (e.g. U+2182 encoded as \xe2\x86\x82).
/// Perl naturally strips bytes > 0x7F when building ASCII tag names.
/// This mirrors that behavior by removing non-ASCII Unicode characters.
fn strip_non_ascii(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii()).collect()
}

/// Convert XML element names to ExifTool-style CamelCase tag names.
/// Mirrors Perl: `my $name = ucfirst lc $tag; $name =~ s/_(.)/\U$1/g;`
/// e.g. IMAGE_CREATION → ImageCreation, GENERAL_CREATION_INFO → GeneralCreationInfo
fn xml_elem_to_camel(s: &str) -> String {
    // If the string contains underscores or is ALL_CAPS, do full conversion:
    // lowercase, ucfirst, remove underscores capitalizing next char
    if s.contains('_') || s.chars().all(|c| c.is_uppercase() || !c.is_alphabetic()) {
        let lower = s.to_lowercase();
        let mut result = String::with_capacity(lower.len());
        let mut capitalize_next = true;
        for ch in lower.chars() {
            if ch == '_' {
                capitalize_next = true;
            } else if capitalize_next {
                for c in ch.to_uppercase() {
                    result.push(c);
                }
                capitalize_next = false;
            } else {
                result.push(ch);
            }
        }
        result
    } else {
        // camelCase or lowercase: just ucfirst
        let mut chars = s.chars();
        match chars.next() {
            None => String::new(),
            Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        }
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
    // Track whether the current element has had any child elements (to detect leaf nodes)
    // Each entry corresponds to the matching path depth: true = has children
    let mut has_children: Vec<bool> = Vec::new();

    // Accumulate full path as tag name prefix: root element name + child names concatenated
    // Each path component is ucfirst'd to produce CamelCase tag names (e.g., GpxTrkName).
    // Attributes on elements are emitted as TagName = value (path + attrName)

    // Track which namespace URIs were declared on the root element (xmlns=...)

    for event in parser {
        match event {
            Ok(XmlEvent::StartElement {
                name,
                attributes,
                namespace,
                ..
            }) => {
                let local = xml_elem_to_camel(&name.local_name);
                let path_str = format!("{}{}", path.join(""), local);
                // Mark parent as having a child element
                if let Some(last) = has_children.last_mut() {
                    *last = true;
                }
                path.push(local.clone());
                has_children.push(false);
                current_text.clear();

                // Emit default xmlns (xmlns="uri") as {ElemName}Xmlns tag
                // xml-rs exposes xmlns in the namespace mappings
                // The default namespace (no prefix) is exposed via namespace.get("")
                // We look for newly-declared default NS at this element

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
                        // Emit as {RootName}Xmlns = default_ns_uri (CamelCase root name)
                        let tag_name = format!("{}Xmlns", local);
                        if !seen_names.contains(&tag_name) {
                            seen_names.insert(tag_name.clone());
                            let val = Value::String(default_ns.to_string());
                            let pv = val.to_display_string();
                            tags.push(Tag {
                                id: TagId::Text(format!("XMP:{}", tag_name)),
                                name: tag_name.clone(),
                                description: tag_name,
                                group: TagGroup {
                                    family0: "XMP".into(),
                                    family1: "XMP".into(),
                                    family2: "Other".into(),
                                },
                                raw_value: val,
                                print_value: pv,
                                priority: 0,
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
                    let attr_local = xml_elem_to_camel(&aname.local_name);
                    let tag_name = format!("{}{}", path_str, attr_local);
                    if !seen_names.contains(&tag_name) {
                        seen_names.insert(tag_name.clone());
                        // Determine group prefix from namespace
                        let attr_ns = aname.namespace.as_deref().unwrap_or("");
                        let pfx = namespace_prefix(attr_ns);
                        let group_pfx = if pfx.is_empty() {
                            aname.prefix.as_deref().unwrap_or("XMP")
                        } else {
                            pfx
                        };
                        // Normalize attribute value to collapse internal whitespace/newlines
                        let attr_val = normalize_xml_text(&attr.value);
                        let val = Value::String(attr_val.clone());
                        let pv = val.to_display_string();
                        tags.push(Tag {
                            id: TagId::Text(format!("XMP:{}", tag_name)),
                            name: tag_name.clone(),
                            description: tag_name,
                            group: TagGroup {
                                family0: "XMP".into(),
                                family1: format!("XMP-{}", group_pfx),
                                family2: "Other".into(),
                            },
                            raw_value: val,
                            print_value: pv,
                            priority: 0,
                        });
                    }
                }
            }
            Ok(XmlEvent::Characters(text)) | Ok(XmlEvent::CData(text)) => {
                current_text.push_str(&text);
            }
            Ok(XmlEvent::EndElement { .. }) => {
                let text = normalize_xml_text(&current_text);
                let is_leaf = !has_children.last().copied().unwrap_or(false);
                // Emit tag if: has text content OR is a leaf node (no child elements, i.e. empty element)
                if (is_leaf || !text.is_empty()) && !path.is_empty() {
                    let tag_name = path.join("");
                    if !seen_names.contains(&tag_name) {
                        seen_names.insert(tag_name.clone());
                        let val = Value::String(text.clone());
                        let pv = val.to_display_string();
                        tags.push(Tag {
                            id: TagId::Text(format!("XMP:{}", tag_name)),
                            name: tag_name.clone(),
                            description: tag_name,
                            group: TagGroup {
                                family0: "XMP".into(),
                                family1: "XMP".into(),
                                family2: "Other".into(),
                            },
                            raw_value: val,
                            print_value: pv,
                            priority: 0,
                        });
                    }
                }
                current_text.clear();
                has_children.pop();
                path.pop();
            }
            Err(_) => continue,
            _ => {}
        }
    }
    Ok(tags)
}

/// Fix malformed XML by removing unmatched closing tags.
/// Uses a tag stack to detect closing tags that have no matching open tag.
/// XML tag names are ASCII, so byte-level < > scanning is safe.
fn fix_malformed_xml(xml: &str) -> String {
    let mut stack: Vec<String> = Vec::new();
    let mut result = String::with_capacity(xml.len());
    let mut pos = 0;

    while pos < xml.len() {
        if let Some(rel) = xml[pos..].find('<') {
            let lt = pos + rel;
            // Emit everything before this '<'
            result.push_str(&xml[pos..lt]);
            pos = lt;

            // Check what kind of tag
            let rest = &xml[pos..];
            if rest.starts_with("</") {
                // Closing tag
                if let Some(gt_rel) = rest.find('>') {
                    let tag_name = rest[2..gt_rel].trim().to_string();
                    let gt = pos + gt_rel;
                    if stack.last().map(|s| s == &tag_name).unwrap_or(false) {
                        // Matched: emit and pop
                        stack.pop();
                        result.push_str(&xml[pos..=gt]);
                    } else if stack.contains(&tag_name) {
                        // Matches something deeper: pop up to it and emit
                        while stack.last().map(|s| s != &tag_name).unwrap_or(false) {
                            stack.pop();
                        }
                        stack.pop();
                        result.push_str(&xml[pos..=gt]);
                    }
                    // else: unmatched closing tag — skip it (emit nothing)
                    pos = gt + 1;
                } else {
                    result.push('<');
                    pos += 1;
                }
            } else if rest.starts_with("<!") || rest.starts_with("<?") {
                // Comment, CDATA, or PI — emit as-is until closing marker
                let end = if rest.starts_with("<!--") {
                    rest.find("-->").map(|e| pos + e + 3)
                } else if rest.starts_with("<![CDATA[") {
                    rest.find("]]>").map(|e| pos + e + 3)
                } else {
                    // Processing instruction
                    rest.find("?>").map(|e| pos + e + 2)
                };
                if let Some(end_pos) = end {
                    result.push_str(&xml[pos..end_pos]);
                    pos = end_pos;
                } else {
                    result.push('<');
                    pos += 1;
                }
            } else {
                // Opening or self-closing tag
                if let Some(gt_rel) = rest.find('>') {
                    let gt = pos + gt_rel;
                    let inner = rest[1..gt_rel].trim();
                    let is_self_closing = inner.ends_with('/');
                    if !is_self_closing {
                        let tag_name = inner
                            .split(|c: char| c.is_whitespace() || c == '/')
                            .next()
                            .unwrap_or("")
                            .to_string();
                        if !tag_name.is_empty() {
                            stack.push(tag_name);
                        }
                    }
                    result.push_str(&xml[pos..=gt]);
                    pos = gt + 1;
                } else {
                    result.push('<');
                    pos += 1;
                }
            }
        } else {
            // No more '<': emit the rest
            result.push_str(&xml[pos..]);
            break;
        }
    }
    result
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
        if !in_pi && i + 1 < chars.len() && c == '<' && chars[i + 1] == '?' {
            in_pi = true;
            result.push(c);
        } else if in_pi && c == '?' && i + 1 < chars.len() && chars[i + 1] == '>' {
            in_pi = false;
            result.push(c);
            result.push(chars[i + 1]);
            i += 2;
            continue;
        } else if in_pi {
            // Replace invalid XML chars in PI with space
            if c == '\t'
                || c == '\n'
                || c == '\r'
                || (c as u32 >= 0x20 && c as u32 <= 0xD7FF)
                || (c as u32 >= 0xE000 && c as u32 <= 0xFFFD)
                || (c as u32 >= 0x10000 && c as u32 <= 0x10FFFF)
            {
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
            Ok(XmlEvent::StartElement {
                name, attributes, ..
            }) => {
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
            Err(_) => continue,
            _ => {}
        }
    }
    map
}

/// Pre-pass: collect nodeIDs that appear inline — i.e., rdf:Description rdf:nodeID="X"
/// nested inside a property element (not as a direct child of rdf:RDF or x:xmpmeta).
/// These nodeIDs are referenced inline by their parent property, so the top-level
/// Description with the same nodeID should NOT emit tags directly.
fn collect_inline_referenced_node_ids(xml: &str) -> std::collections::HashSet<String> {
    use xml::reader::{EventReader, XmlEvent};
    let mut set: std::collections::HashSet<String> = std::collections::HashSet::new();
    let parser = EventReader::from_str(xml);
    let rdf_ns = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
    // Track the stack of (namespace, local_name) for parent context
    let mut path: Vec<(String, String)> = Vec::new();

    for event in parser {
        match event {
            Ok(XmlEvent::StartElement {
                name, attributes, ..
            }) => {
                let local = name.local_name.clone();
                let ns = name.namespace.as_deref().unwrap_or("").to_string();

                // Check if this is rdf:Description with rdf:nodeID
                if local == "Description" && ns == rdf_ns {
                    if let Some(nid_attr) = attributes.iter().find(|a| {
                        a.name.local_name == "nodeID"
                            && (a.name.prefix.as_deref() == Some("rdf")
                                || a.name.namespace.as_deref() == Some(rdf_ns))
                    }) {
                        // Determine if this Description is inline-referenced:
                        // it must have a non-RDF, non-xmpmeta, non-RDF-container parent.
                        // Direct children of rdf:RDF, x:xmpmeta, x:xapmeta are top-level.
                        let parent_is_top_level = path
                            .last()
                            .map(|(pns, pln)| {
                                (pln == "RDF" && pns == rdf_ns)
                                    || pln == "xmpmeta"
                                    || pln == "xapmeta"
                            })
                            .unwrap_or(true);

                        if !parent_is_top_level {
                            set.insert(nid_attr.value.clone());
                        }
                    }
                }

                path.push((ns, local));
            }
            Ok(XmlEvent::EndElement { .. }) => {
                path.pop();
            }
            Err(_) => continue,
            _ => {}
        }
    }
    set
}

/// Pre-pass: collect ALL properties of blank nodes (rdf:Description with rdf:nodeID).
/// Returns a map from nodeID → Vec<(namespace_uri, local_name, value)>.
/// Handles attributes on Description, child elements with text, and rdf:resource attributes.
/// Multiple Descriptions with the same nodeID are merged.
fn collect_blank_node_properties(
    xml: &str,
) -> std::collections::HashMap<String, Vec<(String, String, String)>> {
    use xml::reader::{EventReader, XmlEvent};
    let mut map: std::collections::HashMap<String, Vec<(String, String, String)>> =
        std::collections::HashMap::new();
    let parser = EventReader::from_str(xml);
    let mut current_node_id: Option<String> = None;
    let mut current_text = String::new();
    let mut in_property = false;
    let mut current_prop_ns = String::new();
    let mut current_prop_local = String::new();
    let rdf_ns = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";

    for event in parser {
        match event {
            Ok(XmlEvent::StartElement {
                name, attributes, ..
            }) => {
                current_text.clear();
                let local = &name.local_name;
                let ns = name.namespace.as_deref().unwrap_or("");

                if local == "Description" && ns == rdf_ns {
                    // Check for rdf:nodeID
                    if let Some(nid_attr) = attributes.iter().find(|a| {
                        a.name.local_name == "nodeID"
                            && (a.name.prefix.as_deref() == Some("rdf")
                                || a.name.namespace.as_deref() == Some(rdf_ns))
                    }) {
                        let nid = nid_attr.value.clone();
                        current_node_id = Some(nid.clone());
                        // Collect attribute properties (non-xmlns, non-rdf, non-about)
                        let entry = map.entry(nid).or_default();
                        for attr in &attributes {
                            if attr.name.local_name == "nodeID" || attr.name.local_name == "about" {
                                continue;
                            }
                            if attr.name.prefix.as_deref() == Some("xmlns") {
                                continue;
                            }
                            if attr.name.local_name.starts_with("xmlns") {
                                continue;
                            }
                            if attr.name.prefix.as_deref() == Some("rdf")
                                || attr.name.namespace.as_deref() == Some(rdf_ns)
                            {
                                continue;
                            }
                            let attr_ns = attr.name.namespace.as_deref().unwrap_or("").to_string();
                            entry.push((attr_ns, attr.name.local_name.clone(), attr.value.clone()));
                        }
                    }
                } else if current_node_id.is_some() && ns != rdf_ns && local != "RDF" {
                    // Property child element inside a blank node Description
                    in_property = true;
                    current_prop_ns = ns.to_string();
                    current_prop_local = local.clone();
                    // Check for rdf:resource attribute
                    if let Some(res_attr) = attributes.iter().find(|a| {
                        a.name.local_name == "resource"
                            && a.name.namespace.as_deref() == Some(rdf_ns)
                    }) {
                        let nid = current_node_id.as_ref().unwrap().clone();
                        let entry = map.entry(nid).or_default();
                        entry.push((ns.to_string(), local.clone(), res_attr.value.clone()));
                        in_property = false; // self-closing with resource
                    }
                }
            }
            Ok(XmlEvent::Characters(text)) | Ok(XmlEvent::CData(text)) => {
                if in_property {
                    current_text.push_str(&text);
                }
            }
            Ok(XmlEvent::EndElement { name }) => {
                let local = &name.local_name;
                let ns = name.namespace.as_deref().unwrap_or("");
                if local == "Description" && ns == rdf_ns {
                    current_node_id = None;
                    in_property = false;
                } else if in_property {
                    // Closing a property child element
                    let text = normalize_xml_text(&current_text);
                    if let Some(nid) = &current_node_id {
                        let entry = map.entry(nid.clone()).or_default();
                        entry.push((current_prop_ns.clone(), current_prop_local.clone(), text));
                    }
                    in_property = false;
                    current_text.clear();
                }
            }
            Err(_) => continue,
            _ => {}
        }
    }
    map
}
