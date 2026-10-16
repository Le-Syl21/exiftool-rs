//! Structured XMP output (ExifTool's `-struct` / `Struct` option).
//!
//! The streaming reader in [`super::xmp`] flattens structures as it parses and
//! carries every flat-name override ExifTool applies (`RegionInfo` flattens to
//! `RegionName`, `ArtworkOrObject` to `ArtworkTitle`, ...). Rather than teach
//! each of its emission sites about structures, this pass runs over the same
//! packet a second time as a tree:
//!
//! 1. every top-level property of an `rdf:Description` is rebuilt as a tree
//!    and classified: a structure, or a list holding one, is a *struct root*;
//! 2. the packet is re-serialised without the struct roots and read again by
//!    the unchanged streaming reader, which yields exactly the tags that are
//!    not derived from a structure, overrides included;
//! 3. one struct-valued tag per root is inserted where the property sits in
//!    document order (before the next non-struct property's tags).
//!
//! Field keys are the capitalised local names in sorted order; a language
//! alternative inside a structure expands to `Field` (x-default) and
//! `Field-lang`; a field from a foreign namespace in an open structure is
//! qualified as `XMP-prefix:Field`; lists inside structures are always lists;
//! leaf dates get the same ValueConv as flattened tags.
use super::xmp::{
    apply_flat_name_remap, convert_xmp_date, exiftool_namespace_alias, namespace_category,
    namespace_prefix, normalize_xml_text, remap_xmp_tag_name, strip_non_ascii, ucfirst, XmpReader,
};

thread_local! {
    /// Set while the pruned packet is re-read, so that read does not recurse
    /// into this pass; the `-struct` list rule stays in force meanwhile.
    static IN_PASS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Flattened tags replaced by structures in this extraction. Composite
    /// tags still read them (ExifTool builds `Composite:Flash` from the
    /// flattened `FlashFired`... values whether or not `-struct` is on).
    static SUPPRESSED: std::cell::RefCell<Vec<Tag>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Forget the flattened tags replaced in the previous extraction.
pub fn reset_suppressed() {
    SUPPRESSED.with(|s| s.borrow_mut().clear());
}

/// Flattened tags replaced by structures in the current extraction.
pub fn suppressed() -> Vec<Tag> {
    SUPPRESSED.with(|s| s.borrow().clone())
}

/// Tag names ExifTool gives structure properties whose XML name differs
/// (`Name =>` overrides on `Struct` definitions in XMP2.pl and MWG.pm).
fn root_override(prefix: &str, local: &str) -> Option<&'static str> {
    Some(match (prefix, local) {
        ("mwg-rs", "Regions") => "RegionInfo",
        ("acdsee-rs", "Regions") => "RegionInfoACDSee",
        ("mwg-kw", "Keywords") => "KeywordInfo",
        ("crs", "RangeMaskMapInfo") => "RangeMask",
        _ => return None,
    })
}
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;
use xml::reader::{EventReader, XmlEvent};
use xml::writer::{EmitterConfig, EventWriter};

const RDF_NS: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";

/// One element of a top-level property subtree.
#[derive(Debug, Default)]
struct Node {
    ns: String,
    prefix: String,
    local: String,
    /// (namespace, prefix, local name, value) of each attribute.
    attrs: Vec<(String, String, String, String)>,
    children: Vec<Node>,
    text: String,
}

impl Node {
    fn is_rdf(&self, local: &str) -> bool {
        self.ns == RDF_NS && self.local == local
    }
    fn attr(&self, ns: &str, local: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(a_ns, _, a_local, _)| a_ns == ns && a_local == local)
            .map(|(_, _, _, v)| v.as_str())
    }
    /// Attributes that are structure fields: not RDF, XML or namespace declarations.
    fn field_attrs(&self) -> impl Iterator<Item = &(String, String, String, String)> {
        self.attrs.iter().filter(|(ns, prefix, local, _)| {
            ns != RDF_NS && ns != XML_NS && prefix != "xmlns" && local != "xmlns"
        })
    }
}

/// Intermediate tree with language alternatives kept apart, since they expand
/// into several fields of the enclosing structure.
#[derive(Debug)]
enum Built {
    Scalar(String),
    List(Vec<Built>),
    Struct(Vec<(String, Built)>),
    LangAlt(Vec<(String, String)>),
}

impl Built {
    fn is_root(&self) -> bool {
        match self {
            Built::Struct(_) => true,
            Built::List(items) => items.iter().any(|i| matches!(i, Built::Struct(_))),
            _ => false,
        }
    }
}

/// A top-level property in document order.
struct Property {
    prefix: String,
    local: String,
    value: Built,
}

/// Group prefix ExifTool uses for a namespace, falling back to the document's.
fn group_prefix(ns: &str, doc_prefix: &str) -> String {
    let known = namespace_prefix(ns);
    let prefix = if known.is_empty() { doc_prefix } else { known };
    exiftool_namespace_alias(prefix)
        .unwrap_or(prefix)
        .to_string()
}

/// Field key inside a structure. ExifTool's struct definitions name fields
/// without a namespace; an open structure (`mwg-rs:Extensions`) qualifies a
/// field from another namespace as `XMP-prefix:Field`, except the sType
/// (`st*`) and RDF schema (`rdfs:seeAlso`) namespaces shared by every struct.
/// `parent_prefix` is the namespace of the enclosing structure element.
fn field_key(field_prefix: &str, local: &str, parent_prefix: &str) -> String {
    let name = ucfirst(&strip_non_ascii(local));
    if field_prefix.is_empty()
        || field_prefix == parent_prefix
        || field_prefix.starts_with("st")
        || field_prefix == "rdfs"
    {
        name
    } else {
        format!("XMP-{field_prefix}:{name}")
    }
}

fn leaf(text: &str) -> Built {
    let text = normalize_xml_text(text);
    Built::Scalar(convert_xmp_date(&text).unwrap_or(text))
}

fn build_lang_alt(alt: &Node) -> Built {
    let mut entries = Vec::new();
    for li in alt.children.iter().filter(|c| c.is_rdf("li")) {
        let lang = li.attr(XML_NS, "lang").unwrap_or("x-default").to_string();
        entries.push((lang, normalize_xml_text(&li.text)));
    }
    Built::LangAlt(entries)
}

/// Value of a property or list-item element.
fn build(node: &Node, parent_prefix: &str) -> Built {
    if let Some(resource) = node.attr(RDF_NS, "resource") {
        return Built::Scalar(resource.to_string());
    }
    let elements: Vec<&Node> = node.children.iter().collect();
    if let [only] = elements.as_slice() {
        if only.is_rdf("Alt") {
            return build_lang_alt(only);
        }
        if only.is_rdf("Bag") || only.is_rdf("Seq") {
            return Built::List(
                only.children
                    .iter()
                    .filter(|c| c.is_rdf("li"))
                    .map(|li| build(li, parent_prefix))
                    .collect(),
            );
        }
    }
    let parse_resource = node.attr(RDF_NS, "parseType") == Some("Resource");
    let has_fields = node.field_attrs().next().is_some()
        || elements.iter().any(|c| !c.is_rdf("Description"))
        || elements.iter().any(|c| c.is_rdf("Description"));
    if parse_resource || has_fields {
        let mut fields: Vec<(String, Built)> = Vec::new();
        let mut push = |node: &Node| {
            for (ns, prefix, local, value) in node.field_attrs() {
                let key = field_key(&group_prefix(ns, prefix), local, parent_prefix);
                fields.push((key, leaf(value)));
            }
            for child in node.children.iter().filter(|c| !c.is_rdf("Description")) {
                if child.ns == RDF_NS {
                    continue;
                }
                let child_prefix = group_prefix(&child.ns, &child.prefix);
                let key = field_key(&child_prefix, &child.local, parent_prefix);
                fields.push((key, build(child, &child_prefix)));
            }
        };
        push(node);
        for description in node.children.iter().filter(|c| c.is_rdf("Description")) {
            push(description);
        }
        return Built::Struct(fields);
    }
    leaf(&node.text)
}

/// Convert to a [`Value`], expanding language alternatives into their parent.
fn to_value(built: Built) -> Value {
    match built {
        Built::Scalar(s) => Value::String(s),
        Built::LangAlt(entries) => Value::String(
            entries
                .iter()
                .find(|(lang, _)| lang == "x-default")
                .or_else(|| entries.first())
                .map(|(_, v)| v.clone())
                .unwrap_or_default(),
        ),
        Built::List(items) => Value::List(items.into_iter().map(to_value).collect()),
        Built::Struct(fields) => {
            let mut out: Vec<(String, Value)> = Vec::new();
            let lang_key = |key: &str, lang: &str| {
                if lang == "x-default" {
                    key.to_string()
                } else {
                    format!("{key}-{lang}")
                }
            };
            for (key, value) in fields {
                match value {
                    Built::LangAlt(entries) => {
                        for (lang, text) in entries {
                            out.push((lang_key(&key, &lang), Value::String(text)));
                        }
                    }
                    // A bag of language alternatives: one list per language.
                    Built::List(items)
                        if !items.is_empty()
                            && items.iter().all(|i| matches!(i, Built::LangAlt(_))) =>
                    {
                        // ExifTool keeps each language's list aligned with the bag:
                        // an item without that language contributes "" up to the
                        // last item that has it (`Field1-fr: ["", "ingénieux"]`).
                        let mut per_lang: Vec<(String, Vec<Value>)> = Vec::new();
                        for (index, item) in items.iter().enumerate() {
                            if let Built::LangAlt(entries) = item {
                                for (lang, text) in entries {
                                    let k = lang_key(&key, lang);
                                    let values =
                                        match per_lang.iter_mut().find(|(name, _)| *name == k) {
                                            Some((_, values)) => values,
                                            None => {
                                                per_lang.push((k, Vec::new()));
                                                &mut per_lang.last_mut().unwrap().1
                                            }
                                        };
                                    while values.len() < index {
                                        values.push(Value::String(String::new()));
                                    }
                                    values.push(Value::String(text.clone()));
                                }
                            }
                        }
                        for (k, values) in per_lang {
                            out.push((k, Value::List(values)));
                        }
                    }
                    other => out.push((key, to_value(other))),
                }
            }
            out.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Struct(out)
        }
    }
}

/// Parse the packet into top-level properties and the packet without its
/// struct roots. Returns `None` when the packet is not well formed.
fn split(xml: &str) -> Option<(Vec<(Property, bool)>, String)> {
    let mut output: Vec<u8> = Vec::with_capacity(xml.len());
    let mut writer = EventWriter::new_with_config(
        &mut output,
        EmitterConfig::new()
            .write_document_declaration(false)
            .perform_indent(false)
            .normalize_empty_elements(false),
    );
    let mut stack: Vec<(String, String)> = Vec::new(); // (namespace, local) of open elements
    let mut properties: Vec<(Property, bool)> = Vec::new();
    // While inside a top-level property: its node stack and buffered events.
    let mut nodes: Vec<Node> = Vec::new();
    let mut buffered: Vec<XmlEvent> = Vec::new();
    let mut property_depth: Option<usize> = None;

    for event in EventReader::from_str(xml) {
        let event = event.ok()?;
        match &event {
            XmlEvent::StartDocument { .. } | XmlEvent::EndDocument => continue,
            XmlEvent::StartElement {
                name, attributes, ..
            } => {
                let ns = name.namespace.clone().unwrap_or_default();
                let top_level = property_depth.is_none()
                    && ns != RDF_NS
                    && matches!(stack.as_slice(), [.., (rdf, description)] if rdf == RDF_NS && description == "Description");
                if top_level {
                    property_depth = Some(stack.len());
                }
                if property_depth.is_some() {
                    nodes.push(Node {
                        ns,
                        prefix: name.prefix.clone().unwrap_or_default(),
                        local: name.local_name.clone(),
                        attrs: attributes
                            .iter()
                            .map(|a| {
                                (
                                    a.name.namespace.clone().unwrap_or_default(),
                                    a.name.prefix.clone().unwrap_or_default(),
                                    a.name.local_name.clone(),
                                    a.value.clone(),
                                )
                            })
                            .collect(),
                        ..Node::default()
                    });
                }
                stack.push((
                    name.namespace.clone().unwrap_or_default(),
                    name.local_name.clone(),
                ));
            }
            XmlEvent::EndElement { .. } => {
                stack.pop();
                if property_depth.is_some() {
                    let node = nodes.pop()?;
                    if let Some(parent) = nodes.last_mut() {
                        parent.children.push(node);
                    } else {
                        // The top-level property is complete.
                        let prefix = group_prefix(&node.ns, &node.prefix);
                        let value = build(&node, &prefix);
                        let root = value.is_root();
                        properties.push((
                            Property {
                                prefix,
                                local: node.local.clone(),
                                value,
                            },
                            root,
                        ));
                        property_depth = None;
                        buffered.push(event);
                        if !root {
                            for e in buffered.drain(..) {
                                if let Some(w) = e.as_writer_event() {
                                    writer.write(w).ok()?;
                                }
                            }
                        }
                        buffered.clear();
                        continue;
                    }
                }
            }
            XmlEvent::Characters(text) | XmlEvent::CData(text) => {
                if let Some(node) = nodes.last_mut() {
                    node.text.push_str(text);
                }
            }
            XmlEvent::Whitespace(text) => {
                if let Some(node) = nodes.last_mut() {
                    node.text.push_str(text);
                }
            }
            _ => {}
        }
        if property_depth.is_some() {
            buffered.push(event);
        } else if let Some(w) = event.as_writer_event() {
            writer.write(w).ok()?;
        }
    }
    drop(writer);
    Some((properties, String::from_utf8(output).ok()?))
}

/// Tag name ExifTool reports for a top-level property. Structure roots keep
/// their plain capitalised name (`RegionInfo`, `KeywordInfo`); the remaps apply
/// to simple properties only (`ISOSpeedRatings` to `ISO`).
fn tag_name(prefix: &str, local: &str) -> String {
    apply_flat_name_remap(&remap_xmp_tag_name(prefix, local))
}

fn root_name(prefix: &str, local: &str) -> String {
    root_override(prefix, local)
        .map(str::to_string)
        .unwrap_or_else(|| ucfirst(&strip_non_ascii(local)))
}

fn struct_tag(property: Property) -> Tag {
    let name = root_name(&property.prefix, &property.local);
    let value = to_value(property.value);
    Tag {
        id: TagId::Text(format!("{}:{}", property.prefix, name)),
        name: name.clone(),
        description: name,
        group: TagGroup {
            family0: "XMP".into(),
            family1: format!("XMP-{}", property.prefix),
            family2: namespace_category(&property.prefix).to_string(),
            family3: "Main".into(),
        },
        print_value: value.serialize_struct(),
        raw_value: value,
        priority: 0,
    }
}

/// Replace the flattened fields of structured properties in `tags` by one
/// struct-valued tag each. Falls back to `tags` unchanged when the packet
/// cannot be re-read.
pub(crate) fn apply(xml: &str, tags: Vec<Tag>) -> Vec<Tag> {
    if IN_PASS.with(|p| p.get()) {
        return tags;
    }
    let Some((properties, pruned)) = split(xml) else {
        return tags;
    };
    if !properties.iter().any(|(_, root)| *root) {
        return tags;
    }
    IN_PASS.with(|p| p.set(true));
    let reread = XmpReader::read(pruned.as_bytes());
    IN_PASS.with(|p| p.set(false));
    let Ok(mut result) = reread else { return tags };

    // The flattened tags that the pruned packet no longer yields are the ones
    // the structures replace; composites may still need them.
    let mut remaining: Vec<(String, String, String)> = result
        .iter()
        .map(|t| {
            (
                t.group.family1.clone(),
                t.name.clone(),
                t.print_value.clone(),
            )
        })
        .collect();
    let replaced: Vec<Tag> = tags
        .into_iter()
        .filter(|t| {
            let key = (
                t.group.family1.clone(),
                t.name.clone(),
                t.print_value.clone(),
            );
            match remaining.iter().position(|k| *k == key) {
                Some(i) => {
                    remaining.swap_remove(i);
                    false
                }
                None => true,
            }
        })
        .collect();
    SUPPRESSED.with(|s| s.borrow_mut().extend(replaced));

    // For each root, the next non-struct property in document order: its tags
    // mark where the struct tag is inserted.
    let nexts: Vec<Option<(String, String)>> = (0..properties.len())
        .map(|i| {
            properties[i + 1..]
                .iter()
                .find(|(_, root)| !*root)
                .map(|(p, _)| (p.prefix.clone(), p.local.clone()))
        })
        .collect();
    for ((property, root), next) in properties.into_iter().zip(nexts) {
        if !root {
            continue;
        }
        let tag = struct_tag(property);
        let index = next
            .and_then(|(prefix, local)| {
                let family1 = format!("XMP-{prefix}");
                let name = tag_name(&prefix, &local);
                result.iter().position(|t| {
                    t.group.family1 == family1
                        && (t.name == name || t.name.starts_with(&format!("{name}-")))
                })
            })
            .unwrap_or_else(|| {
                result
                    .iter()
                    .rposition(|t| t.group.family0 == "XMP")
                    .map_or(result.len(), |p| p + 1)
            });
        result.insert(index, tag);
    }
    result
}
