//! Edit an existing XMP packet in place.
//!
//! [`xmp_writer::build_xmp`](crate::writer::xmp_writer::build_xmp) writes a
//! new packet, which is fine when a file has no XMP but loses every property
//! it was not given when one exists. This module edits the packet's text
//! instead: it finds the top-level properties of each `rdf:Description`
//! (child elements and attributes), removes the ones a request deletes,
//! replaces or adds the ones it writes, and leaves the rest of the text as
//! it was.
//!
//! Matching follows ExifTool's naming: a property belongs to the family-1
//! group `XMP-<prefix>`, where `<prefix>` is ExifTool's standard prefix for
//! the namespace URI (not whatever prefix the file declared), and its tag
//! name is the property name with ExifTool's capitalisation. `XMP:all`
//! deletes the packet; a packet left without any property is deleted too,
//! as ExifTool deletes it (the `x:xmptk` toolkit attribute is not a
//! property).
//!
//! Only top-level properties are addressed: a flattened structure field
//! such as `XMP-iptcCore:CreatorCity` is not, so a request for one is left
//! unapplied and the caller reports it as such.

use crate::error::{Error, Result};
use std::ops::Range;

const RDF_NS: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const X_NS: &str = "adobe:ns:meta/";

/// The kind of value a written property takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XmpValueKind {
    Simple,
    LangAlt,
    Bag,
    Seq,
    Alt,
}

/// A property to write.
#[derive(Debug, Clone)]
pub struct XmpSet {
    /// Namespace URI.
    pub uri: String,
    /// Preferred prefix (used if the packet does not declare the URI).
    pub prefix: String,
    /// Property (local) name.
    pub property: String,
    pub values: Vec<String>,
    pub kind: XmpValueKind,
}

/// The XMP part of a write request.
#[derive(Debug, Clone, Default)]
pub struct XmpChanges {
    /// `XMP:all`: delete the packet.
    pub delete_all: bool,
    /// Lower-case standard prefixes whose properties all go (`XMP-dc:all`).
    pub delete_groups: Vec<String>,
    /// (lower-case standard prefix or None, lower-case tag name).
    pub delete_tags: Vec<(Option<String>, String)>,
    pub set: Vec<XmpSet>,
    /// Properties replaced only where the packet has them (ExifTool's
    /// non-preferred namespaces for a tag written as `XMP:Tag`).
    pub edit_only: Vec<XmpSet>,
    /// The `x:xmptk` value to stamp on a packet this edit changes.
    pub toolkit: String,
}

impl XmpChanges {
    pub fn is_empty(&self) -> bool {
        !self.delete_all
            && self.delete_groups.is_empty()
            && self.delete_tags.is_empty()
            && self.set.is_empty()
            && self.edit_only.is_empty()
    }
}

/// What became of the packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XmpOutcome {
    Unchanged,
    Removed,
    Rewritten(Vec<u8>),
}

#[derive(Debug, Clone)]
struct Attr {
    qname: String,
    value: String,
    /// From the whitespace before the name to after the closing quote.
    span: Range<usize>,
    /// The value, between the quotes.
    value_span: Range<usize>,
}

#[derive(Debug, Clone)]
struct Elem {
    qname: String,
    /// The `<` of the start tag.
    start: usize,
    /// Just past the start tag's `>`.
    tag_end: usize,
    self_closing: bool,
    /// Just past the end tag (or the empty-element tag).
    end: usize,
    attrs: Vec<Attr>,
    parent: Option<usize>,
    children: Vec<usize>,
}

struct Doc<'a> {
    text: &'a str,
    elems: Vec<Elem>,
}

fn malformed(why: &str) -> Error {
    Error::InvalidXmp(format!("can't edit XMP: {why}"))
}

fn is_name_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b':' | b'_' | b'-' | b'.') || c >= 0x80
}

impl<'a> Doc<'a> {
    fn parse(text: &'a str) -> Result<Self> {
        let b = text.as_bytes();
        let mut elems: Vec<Elem> = Vec::new();
        let mut stack: Vec<usize> = Vec::new();
        let mut i = 0;
        let find = |from: usize, pat: &str| -> Result<usize> {
            text[from..]
                .find(pat)
                .map(|p| from + p)
                .ok_or_else(|| malformed("unterminated markup"))
        };
        while i < b.len() {
            if b[i] != b'<' {
                i += 1;
                continue;
            }
            let rest = &text[i..];
            if rest.starts_with("<?") {
                i = find(i, "?>")? + 2;
            } else if rest.starts_with("<!--") {
                i = find(i, "-->")? + 3;
            } else if rest.starts_with("<![CDATA[") {
                i = find(i, "]]>")? + 3;
            } else if rest.starts_with("<!") {
                i = find(i, ">")? + 1;
            } else if rest.starts_with("</") {
                let mut j = i + 2;
                while j < b.len() && is_name_char(b[j]) {
                    j += 1;
                }
                let name = &text[i + 2..j];
                let close = find(j, ">")?;
                let top = stack.pop().ok_or_else(|| malformed("unbalanced end tag"))?;
                if elems[top].qname != name {
                    return Err(malformed("mismatched end tag"));
                }
                elems[top].end = close + 1;
                i = close + 1;
            } else {
                // Start tag.
                let mut j = i + 1;
                while j < b.len() && is_name_char(b[j]) {
                    j += 1;
                }
                if j == i + 1 {
                    return Err(malformed("bad start tag"));
                }
                let qname = text[i + 1..j].to_string();
                let mut attrs = Vec::new();
                let (tag_end, self_closing) = loop {
                    let ws = j;
                    while j < b.len() && b[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    if j >= b.len() {
                        return Err(malformed("unterminated start tag"));
                    }
                    if b[j] == b'>' {
                        break (j + 1, false);
                    }
                    if b[j] == b'/' {
                        if b.get(j + 1) != Some(&b'>') {
                            return Err(malformed("bad empty-element tag"));
                        }
                        break (j + 2, true);
                    }
                    if j == ws {
                        return Err(malformed("attributes must be separated"));
                    }
                    let ns = j;
                    while j < b.len() && is_name_char(b[j]) {
                        j += 1;
                    }
                    if j == ns {
                        return Err(malformed("bad attribute"));
                    }
                    let aname = text[ns..j].to_string();
                    while j < b.len() && b[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    if b.get(j) != Some(&b'=') {
                        return Err(malformed("attribute without value"));
                    }
                    j += 1;
                    while j < b.len() && b[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    let q = *b
                        .get(j)
                        .ok_or_else(|| malformed("attribute without value"))?;
                    if q != b'"' && q != b'\'' {
                        return Err(malformed("unquoted attribute"));
                    }
                    let vstart = j + 1;
                    let vend = text[vstart..]
                        .find(q as char)
                        .map(|p| vstart + p)
                        .ok_or_else(|| malformed("unterminated attribute"))?;
                    attrs.push(Attr {
                        qname: aname,
                        value: text[vstart..vend].to_string(),
                        span: ws..vend + 1,
                        value_span: vstart..vend,
                    });
                    j = vend + 1;
                };
                let idx = elems.len();
                let parent = stack.last().copied();
                elems.push(Elem {
                    qname,
                    start: i,
                    tag_end,
                    self_closing,
                    end: tag_end,
                    attrs,
                    parent,
                    children: Vec::new(),
                });
                if let Some(p) = parent {
                    elems[p].children.push(idx);
                }
                if !self_closing {
                    stack.push(idx);
                }
                i = tag_end;
            }
        }
        if !stack.is_empty() {
            return Err(malformed("unclosed element"));
        }
        Ok(Doc { text, elems })
    }

    /// The URI `prefix` is bound to at element `at` ("" for the default).
    fn resolve(&self, at: usize, prefix: &str) -> Option<String> {
        if prefix == "xml" {
            return Some("http://www.w3.org/XML/1998/namespace".into());
        }
        let want = if prefix.is_empty() {
            "xmlns".to_string()
        } else {
            format!("xmlns:{prefix}")
        };
        let mut cur = Some(at);
        while let Some(e) = cur {
            if let Some(a) = self.elems[e].attrs.iter().find(|a| a.qname == want) {
                return Some(a.value.clone());
            }
            cur = self.elems[e].parent;
        }
        None
    }

    fn split(qname: &str) -> (&str, &str) {
        qname.split_once(':').unwrap_or(("", qname))
    }

    /// (namespace URI, local name) of an element.
    fn elem_name(&self, e: usize) -> (Option<String>, String) {
        let (p, l) = Self::split(&self.elems[e].qname);
        (self.resolve(e, p), l.to_string())
    }

    /// The prefix bound to `uri` in scope at `at`.
    fn prefix_for(&self, at: usize, uri: &str) -> Option<String> {
        let mut cur = Some(at);
        while let Some(e) = cur {
            for a in &self.elems[e].attrs {
                if let Some(p) = a.qname.strip_prefix("xmlns:") {
                    if a.value == uri && self.resolve(at, p).as_deref() == Some(uri) {
                        return Some(p.to_string());
                    }
                }
            }
            cur = self.elems[e].parent;
        }
        None
    }
}

/// A top-level property: an element or an attribute of a Description.
struct Prop {
    uri: String,
    local: String,
    /// Text to remove when the property is deleted.
    span: Range<usize>,
    /// The value of an attribute property (None for an element).
    attr_value: Option<Range<usize>>,
}

/// ExifTool's group prefix and tag name for a property, lower-cased.
fn exiftool_names(uri: &str, local: &str, declared_prefix: &str) -> (String, String) {
    let std = crate::metadata::xmp::namespace_prefix(uri);
    let prefix = if std.is_empty() { declared_prefix } else { std };
    let name = crate::metadata::xmp::remap_xmp_tag_name(prefix, local);
    // ExifTool shortens a few prefixes in its group names (`XMP-iptcCore`).
    let prefix = crate::metadata::xmp::exiftool_namespace_alias(prefix).unwrap_or(prefix);
    (prefix.to_ascii_lowercase(), name.to_ascii_lowercase())
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn serialize_property(qname: &str, set: &XmpSet, indent: &str) -> String {
    let mut s = String::new();
    match set.kind {
        XmpValueKind::Simple => {
            let v = set.values.first().map(String::as_str).unwrap_or("");
            s.push_str(&format!("{indent}<{qname}>{}</{qname}>", escape(v)));
        }
        XmpValueKind::LangAlt => {
            let v = set.values.first().map(String::as_str).unwrap_or("");
            s.push_str(&format!(
                "{indent}<{qname}>\n{indent} <rdf:Alt>\n{indent}  <rdf:li xml:lang='x-default'>{}</rdf:li>\n{indent} </rdf:Alt>\n{indent}</{qname}>",
                escape(v)
            ));
        }
        XmpValueKind::Bag | XmpValueKind::Seq | XmpValueKind::Alt => {
            let c = match set.kind {
                XmpValueKind::Bag => "rdf:Bag",
                XmpValueKind::Seq => "rdf:Seq",
                _ => "rdf:Alt",
            };
            s.push_str(&format!("{indent}<{qname}>\n{indent} <{c}>\n"));
            for v in &set.values {
                s.push_str(&format!("{indent}  <rdf:li>{}</rdf:li>\n", escape(v)));
            }
            s.push_str(&format!("{indent} </{c}>\n{indent}</{qname}>"));
        }
    }
    s
}

/// The packet a file without XMP starts from.
fn skeleton(toolkit: &str) -> String {
    format!(
        "<?xpacket begin='\u{feff}' id='W5M0MpCehiHzreSzNTczkc9d'?>\n\
         <x:xmpmeta xmlns:x='adobe:ns:meta/' x:xmptk='{}'>\n\
         <rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'>\n\n \
         <rdf:Description rdf:about=''>\n \
         </rdf:Description>\n\
         </rdf:RDF>\n\
         </x:xmpmeta>\n\
         <?xpacket end='w'?>",
        escape(toolkit)
    )
}

/// The text of a packet: UTF-8, or UTF-16 (either byte order), which
/// ExifTool reads too and writes back as UTF-8.
fn packet_text(x: &[u8]) -> Result<String> {
    if let Ok(s) = std::str::from_utf8(x) {
        return Ok(s.to_string());
    }
    let (body, big) = if let Some(r) = x.strip_prefix(b"\xfe\xff") {
        (r, true)
    } else if let Some(r) = x.strip_prefix(b"\xff\xfe") {
        (r, false)
    } else if x.len() >= 2 && x[0] == 0 && x[1] != 0 {
        (x, true)
    } else if x.len() >= 2 && x[0] != 0 && x[1] == 0 {
        (x, false)
    } else {
        return Err(malformed("not UTF-8"));
    };
    utf16(body, big)
}

fn utf16(body: &[u8], big: bool) -> Result<String> {
    if body.len() % 2 != 0 {
        return Err(malformed("odd-length UTF-16"));
    }
    let units = body.chunks(2).map(|c| {
        if big {
            u16::from_be_bytes([c[0], c[1]])
        } else {
            u16::from_le_bytes([c[0], c[1]])
        }
    });
    char::decode_utf16(units)
        .collect::<std::result::Result<String, _>>()
        .map_err(|_| malformed("bad UTF-16"))
}

/// Apply `changes` to the packet `existing` (`None` when the file has none).
pub fn edit_xmp(existing: Option<&[u8]>, changes: &XmpChanges) -> Result<XmpOutcome> {
    if changes.is_empty() {
        return Ok(XmpOutcome::Unchanged);
    }
    let had_xmp = existing.is_some_and(|x| !x.is_empty());
    // Deleting the packet and writing properties in the same request leaves
    // only the new properties.
    let base: String = match existing {
        Some(x) if !changes.delete_all && !x.is_empty() => packet_text(x)?,
        _ => {
            if changes.set.is_empty() {
                return Ok(if had_xmp && changes.delete_all {
                    XmpOutcome::Removed
                } else {
                    XmpOutcome::Unchanged
                });
            }
            skeleton(&changes.toolkit)
        }
    };
    let doc = Doc::parse(&base)?;

    // Find the Descriptions and their properties.
    let rdf = (0..doc.elems.len())
        .find(|&e| doc.elem_name(e) == (Some(RDF_NS.to_string()), "RDF".to_string()))
        .ok_or_else(|| malformed("no rdf:RDF element"))?;
    let descs: Vec<usize> = doc.elems[rdf]
        .children
        .iter()
        .copied()
        .filter(|&e| doc.elem_name(e) == (Some(RDF_NS.to_string()), "Description".to_string()))
        .collect();
    let mut props: Vec<(Prop, String)> = Vec::new(); // (property, declared prefix)
    for &d in &descs {
        for a in &doc.elems[d].attrs {
            let (p, l) = Doc::split(&a.qname);
            if p.is_empty() || p == "xmlns" || p == "xml" {
                continue;
            }
            let Some(uri) = doc.resolve(d, p) else {
                continue;
            };
            if uri == RDF_NS {
                continue;
            }
            props.push((
                Prop {
                    uri,
                    local: l.to_string(),
                    span: a.span.clone(),
                    attr_value: Some(a.value_span.clone()),
                },
                p.to_string(),
            ));
        }
        let mut prev_end = doc.elems[d].tag_end;
        for &c in &doc.elems[d].children {
            let (uri, local) = doc.elem_name(c);
            let start = doc.elems[c].start;
            // Take the whitespace before the element along with it.
            let ws_start = if doc.text[prev_end..start].trim().is_empty() {
                prev_end
            } else {
                start
            };
            prev_end = doc.elems[c].end;
            let Some(uri) = uri else {
                continue;
            };
            let (p, _) = Doc::split(&doc.elems[c].qname);
            props.push((
                Prop {
                    uri,
                    local,
                    span: ws_start..doc.elems[c].end,
                    attr_value: None,
                },
                p.to_string(),
            ));
        }
    }

    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    let mut remaining = props.len();
    let mut changed = false;
    for (prop, declared) in &props {
        let (prefix, name) = exiftool_names(&prop.uri, &prop.local, declared);
        let local_lc = prop.local.to_ascii_lowercase();
        let deleted = changes.delete_groups.contains(&prefix)
            || changes.delete_tags.iter().any(|(g, n)| {
                g.as_ref().is_none_or(|g| *g == prefix) && (*n == name || *n == local_lc)
            })
            || changes
                .set
                .iter()
                .any(|s| s.uri == prop.uri && s.property == prop.local);
        if deleted {
            edits.push((prop.span.clone(), String::new()));
            remaining -= 1;
            changed = true;
        } else if let Some(s) = changes
            .edit_only
            .iter()
            .find(|s| s.uri == prop.uri && s.property == prop.local)
        {
            // Replaced where it is, under the prefix the packet uses (an
            // attribute keeps holding a single value).
            match &prop.attr_value {
                Some(v) => edits.push((v.clone(), escape(s.values.first().map_or("", |v| v)))),
                None => {
                    let qname = format!("{declared}:{}", prop.local);
                    let body = format!("\n{}", serialize_property(&qname, s, "  "));
                    edits.push((prop.span.clone(), body));
                }
            }
            changed = true;
        }
    }

    if changes.set.is_empty() {
        if !changed {
            return Ok(XmpOutcome::Unchanged);
        }
        if remaining == 0 {
            return Ok(XmpOutcome::Removed);
        }
    }

    // New properties go into the first Description.
    if !changes.set.is_empty() {
        let d = *descs
            .first()
            .ok_or_else(|| malformed("no rdf:Description element"))?;
        let mut decls = String::new();
        let mut body = String::new();
        let mut bound: Vec<(String, String)> = Vec::new(); // prefixes added here
        for s in &changes.set {
            let prefix = match doc.prefix_for(d, &s.uri) {
                Some(p) => p,
                None => match bound.iter().find(|(_, u)| *u == s.uri) {
                    Some((p, _)) => p.clone(),
                    None => {
                        // The preferred prefix, unless it is taken.
                        let mut p = s.prefix.clone();
                        let mut n = 0;
                        while doc.resolve(d, &p).is_some() || bound.iter().any(|(b, _)| *b == p) {
                            p = format!("{}{n}", s.prefix);
                            n += 1;
                        }
                        decls.push_str(&format!("\n  xmlns:{p}='{}'", escape(&s.uri)));
                        bound.push((p.clone(), s.uri.clone()));
                        p
                    }
                },
            };
            body.push('\n');
            body.push_str(&serialize_property(
                &format!("{prefix}:{}", s.property),
                s,
                "  ",
            ));
        }
        let e = &doc.elems[d];
        if e.self_closing {
            // `<rdf:Description .../>` becomes an element with content.
            edits.push((
                e.tag_end - 2..e.tag_end,
                format!("{decls}>{body}\n </{}>", e.qname),
            ));
        } else {
            if !decls.is_empty() {
                edits.push((e.tag_end - 1..e.tag_end - 1, decls));
            }
            edits.push((e.tag_end..e.tag_end, body));
        }
        changed = true;
    }
    if !changed {
        return Ok(XmpOutcome::Unchanged);
    }

    // Stamp the toolkit, as ExifTool stamps its own on any XMP it rewrites.
    if !changes.toolkit.is_empty() {
        if let Some(meta) = (0..doc.elems.len())
            .find(|&e| doc.elem_name(e) == (Some(X_NS.to_string()), "xmpmeta".to_string()))
        {
            let em = &doc.elems[meta];
            let tk = em.attrs.iter().find(|a| {
                let (p, l) = Doc::split(&a.qname);
                l == "xmptk" && doc.resolve(meta, p).as_deref() == Some(X_NS)
            });
            match tk {
                Some(a) => edits.push((a.value_span.clone(), escape(&changes.toolkit))),
                None => {
                    let p = doc.prefix_for(meta, X_NS).unwrap_or_else(|| "x".into());
                    let at = if em.self_closing {
                        em.tag_end - 2
                    } else {
                        em.tag_end - 1
                    };
                    edits.push((at..at, format!(" {p}:xmptk='{}'", escape(&changes.toolkit))));
                }
            }
        }
    }

    // Apply the edits back to front: a deletion that starts where an
    // insertion goes is applied first, and insertions at one point keep
    // their order.
    let mut indexed: Vec<(usize, (Range<usize>, String))> = edits.into_iter().enumerate().collect();
    indexed.sort_by(|(ia, (a, _)), (ib, (b, _))| {
        b.start
            .cmp(&a.start)
            .then(b.end.cmp(&a.end))
            .then(ib.cmp(ia))
    });
    let mut out = base.clone();
    let mut floor = usize::MAX;
    for (_, (r, s)) in indexed {
        if r.end > floor {
            return Err(malformed("overlapping edits"));
        }
        out.replace_range(r.clone(), &s);
        floor = r.start;
    }
    Ok(XmpOutcome::Rewritten(out.into_bytes()))
}

/// Read `extra` along with `main` (ExifTool's JPEG extended XMP, or a
/// second standard packet): its `rdf:Description`s are appended to the
/// `rdf:RDF` of `main`, each carrying the namespace declarations it
/// inherited in `extra`.
pub fn merge_packets(main: &[u8], extra: &[u8]) -> Result<Vec<u8>> {
    let main_text = packet_text(main)?;
    let extra_text = packet_text(extra)?;
    let (main_text, extra_text) = (main_text.as_str(), extra_text.as_str());
    let rdf_of = |doc: &Doc| {
        (0..doc.elems.len())
            .find(|&e| doc.elem_name(e) == (Some(RDF_NS.to_string()), "RDF".to_string()))
            .ok_or_else(|| malformed("no rdf:RDF element"))
    };
    let mdoc = Doc::parse(main_text)?;
    let mrdf = rdf_of(&mdoc)?;
    let close = {
        let e = &mdoc.elems[mrdf];
        if e.self_closing {
            return Err(malformed("empty rdf:RDF element"));
        }
        main_text[..e.end]
            .rfind("</")
            .ok_or_else(|| malformed("no rdf:RDF end tag"))?
    };
    let edoc = Doc::parse(extra_text)?;
    let erdf = rdf_of(&edoc)?;
    let mut added = String::new();
    for &d in &edoc.elems[erdf].children {
        let e = &edoc.elems[d];
        // Declarations in scope from the ancestors, unless redeclared here.
        let mut decls = String::new();
        let mut seen: Vec<&str> = e.attrs.iter().map(|a| a.qname.as_str()).collect();
        let mut cur = e.parent;
        while let Some(p) = cur {
            for a in &edoc.elems[p].attrs {
                if (a.qname == "xmlns" || a.qname.starts_with("xmlns:"))
                    && !seen.contains(&a.qname.as_str())
                {
                    decls.push_str(&format!(" {}='{}'", a.qname, escape(&a.value)));
                    seen.push(&a.qname);
                }
            }
            cur = edoc.elems[p].parent;
        }
        let name_end = e.start + 1 + e.qname.len();
        added.push(' ');
        added.push_str(&extra_text[e.start..name_end]);
        added.push_str(&decls);
        added.push_str(&extra_text[name_end..e.end]);
        added.push('\n');
    }
    let mut out = String::with_capacity(main.len() + added.len());
    out.push_str(&main_text[..close]);
    out.push_str(&added);
    out.push_str(&main_text[close..]);
    Ok(out.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PACKET: &str = "<?xpacket begin='\u{feff}' id='W5M0MpCehiHzreSzNTczkc9d'?>\n\
<x:xmpmeta xmlns:x='adobe:ns:meta/' x:xmptk='Some Toolkit'>\n\
<rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'>\n\
 <rdf:Description rdf:about=''\n  xmlns:dc='http://purl.org/dc/elements/1.1/'\n  xmlns:exif='http://ns.adobe.com/exif/1.0/'\n  exif:GPSLatitude='51,30.042N'>\n\
  <dc:description>\n   <rdf:Alt>\n    <rdf:li xml:lang='x-default'>old</rdf:li>\n   </rdf:Alt>\n  </dc:description>\n\
  <exif:GPSLongitude>0,7.476W</exif:GPSLongitude>\n\
 </rdf:Description>\n\
</rdf:RDF>\n\
</x:xmpmeta>\n\
<?xpacket end='w'?>";

    fn tags(x: &[u8]) -> Vec<(String, String, String)> {
        crate::metadata::XmpReader::read(x)
            .unwrap()
            .into_iter()
            .map(|t| (t.group.family1, t.name, t.print_value))
            .collect()
    }

    fn ch() -> XmpChanges {
        XmpChanges {
            toolkit: "test kit".into(),
            ..Default::default()
        }
    }

    #[test]
    fn deletes_one_property_and_keeps_the_others() {
        let c = XmpChanges {
            delete_tags: vec![(Some("dc".into()), "description".into())],
            ..ch()
        };
        let XmpOutcome::Rewritten(out) = edit_xmp(Some(PACKET.as_bytes()), &c).unwrap() else {
            panic!()
        };
        let t = tags(&out);
        assert!(!t.iter().any(|(_, n, _)| n == "Description"), "{t:?}");
        assert!(t
            .iter()
            .any(|(g, n, _)| g == "XMP-exif" && n == "GPSLatitude"));
        assert!(t
            .iter()
            .any(|(g, n, _)| g == "XMP-exif" && n == "GPSLongitude"));
        assert!(t
            .iter()
            .any(|(_, n, v)| n == "XMPToolkit" && v == "test kit"));
    }

    #[test]
    fn deleting_every_property_removes_the_packet() {
        let c = XmpChanges {
            delete_tags: vec![
                (Some("dc".into()), "description".into()),
                (Some("exif".into()), "gpslatitude".into()),
            ],
            delete_groups: vec!["exif".into()],
            ..ch()
        };
        assert_eq!(
            edit_xmp(Some(PACKET.as_bytes()), &c).unwrap(),
            XmpOutcome::Removed
        );
        let c = XmpChanges {
            delete_all: true,
            ..ch()
        };
        assert_eq!(
            edit_xmp(Some(PACKET.as_bytes()), &c).unwrap(),
            XmpOutcome::Removed
        );
        assert_eq!(edit_xmp(None, &c).unwrap(), XmpOutcome::Unchanged);
    }

    #[test]
    fn a_request_for_another_group_changes_nothing() {
        let c = XmpChanges {
            delete_tags: vec![(Some("xmp".into()), "description".into())],
            ..ch()
        };
        assert_eq!(
            edit_xmp(Some(PACKET.as_bytes()), &c).unwrap(),
            XmpOutcome::Unchanged
        );
    }

    #[test]
    fn writes_replace_and_add() {
        let set = |uri: &str, prefix: &str, prop: &str, v: &str, kind| XmpSet {
            uri: uri.into(),
            prefix: prefix.into(),
            property: prop.into(),
            values: vec![v.into()],
            kind,
        };
        let c = XmpChanges {
            set: vec![
                set(
                    "http://purl.org/dc/elements/1.1/",
                    "dc",
                    "description",
                    "new <one>",
                    XmpValueKind::LangAlt,
                ),
                set(
                    "http://purl.org/dc/elements/1.1/",
                    "dc",
                    "title",
                    "A title",
                    XmpValueKind::LangAlt,
                ),
                set(
                    "http://ns.adobe.com/xap/1.0/",
                    "xmp",
                    "Rating",
                    "3",
                    XmpValueKind::Simple,
                ),
            ],
            ..ch()
        };
        for src in [Some(PACKET.as_bytes()), None] {
            let XmpOutcome::Rewritten(out) = edit_xmp(src, &c).unwrap() else {
                panic!()
            };
            let t = tags(&out);
            assert!(
                t.iter()
                    .any(|(g, n, v)| g == "XMP-dc" && n == "Description" && v == "new <one>"),
                "{t:?}"
            );
            assert!(t
                .iter()
                .any(|(g, n, v)| g == "XMP-dc" && n == "Title" && v == "A title"));
            assert!(t
                .iter()
                .any(|(g, n, v)| g == "XMP-xmp" && n == "Rating" && v == "3"));
            assert_eq!(t.iter().filter(|(_, n, _)| n == "Description").count(), 1);
        }
    }

    #[test]
    fn empty_description_element_gets_content() {
        let p = "<x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'><rdf:Description rdf:about='' xmlns:dc='http://purl.org/dc/elements/1.1/' dc:format='image/heic'/></rdf:RDF></x:xmpmeta>";
        let c = XmpChanges {
            set: vec![XmpSet {
                uri: "http://ns.adobe.com/xap/1.0/".into(),
                prefix: "xmp".into(),
                property: "Label".into(),
                values: vec!["Red".into()],
                kind: XmpValueKind::Simple,
            }],
            ..ch()
        };
        let XmpOutcome::Rewritten(out) = edit_xmp(Some(p.as_bytes()), &c).unwrap() else {
            panic!()
        };
        let t = tags(&out);
        assert!(
            t.iter()
                .any(|(g, n, v)| g == "XMP-xmp" && n == "Label" && v == "Red"),
            "{t:?}"
        );
        assert!(t.iter().any(|(g, n, _)| g == "XMP-dc" && n == "Format"));
    }

    #[test]
    fn malformed_packets_are_refused() {
        let c = XmpChanges {
            delete_tags: vec![(None, "description".into())],
            ..ch()
        };
        for bad in ["<x:xmpmeta><rdf:RDF>", "<a b=c/>", "<a></b>", "<a x='1'"] {
            assert!(edit_xmp(Some(bad.as_bytes()), &c).is_err(), "{bad}");
        }
    }
}
