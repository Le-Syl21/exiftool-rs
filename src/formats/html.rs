//! HTML metadata reader.
//!
//! Extracts `<meta>` tags, `<title>`, embedded MSO XML, and XMP from HTML files.
//! Mirrors ExifTool's HTML.pm.

use crate::error::{Error, Result};
use crate::metadata::XmpReader;
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_html(data: &[u8]) -> Result<Vec<Tag>> {
    let text = String::from_utf8_lossy(data);
    let lower = text.to_lowercase();

    if !lower.contains("<html") && !lower.contains("<!doctype html") && !lower.contains("<?xml") {
        return Err(Error::InvalidData("not an HTML file".into()));
    }

    let mut tags = Vec::new();

    // Extract <title>
    if let Some(start) = lower.find("<title") {
        let rest = &text[start..];
        if let Some(close) = rest.find('>') {
            let after = &rest[close + 1..];
            if let Some(end) = after.to_lowercase().find("</title") {
                let title = after[..end].trim().to_string();
                if !title.is_empty() {
                    tags.push(mk("Title", "Title", Value::String(title)));
                }
            }
        }
    }

    // Extract <meta> tags - with namespace-aware mapping
    let mut search_pos = 0;
    while let Some(meta_pos) = lower[search_pos..].find("<meta") {
        let abs_pos = search_pos + meta_pos;
        let rest = &text[abs_pos..];
        // Find end of meta tag - could be /> or >
        let end = rest.find('>').unwrap_or(rest.len());
        let meta_tag = &rest[..end];

        let name = extract_attr(meta_tag, "name")
            .or_else(|| extract_attr(meta_tag, "property"))
            .or_else(|| extract_attr(meta_tag, "http-equiv"));
        let content = extract_attr(meta_tag, "content");

        if let (Some(name_raw), Some(content)) = (name, content) {
            if !name_raw.is_empty() && !content.is_empty() {
                let (tag_name, _group) = map_html_meta_name(&name_raw);
                if !tag_name.is_empty() {
                    tags.push(mk(&tag_name, &name_raw, Value::String(html_decode(&content))));
                }
            }
        }

        search_pos = abs_pos + end.max(5);
    }

    // Look for embedded MSO/Office XML (<!--[if gte mso 9]><xml>...</xml>)
    parse_mso_xml(&text, &mut tags);

    // Look for embedded XMP
    if let Some(xmp_start) = find_bytes(data, b"<?xpacket begin") {
        if let Some(xmp_end) = find_bytes(&data[xmp_start..], b"<?xpacket end") {
            let end = xmp_start + xmp_end + 20;
            if end <= data.len() {
                if let Ok(xmp_tags) = XmpReader::read(&data[xmp_start..end]) {
                    tags.extend(xmp_tags);
                }
            }
        }
    }

    Ok(tags)
}

/// Map HTML meta tag name (with namespace prefix) to ExifTool tag name.
/// Returns (tag_name, group_suffix).
fn map_html_meta_name(name: &str) -> (String, String) {
    let lower = name.to_lowercase();

    // Namespace-prefixed: "dc:creator", "ncc:charset", "prod:recLocation", etc.
    if let Some(colon_pos) = lower.find(':') {
        let ns = &lower[..colon_pos];
        let local = &lower[colon_pos + 1..];

        match ns {
            "dc" => {
                // Dublin Core namespace - map to ExifTool names
                let tag = match local {
                    "title" => "Title",
                    "creator" => "Creator",
                    "subject" => "Subject",
                    "description" => "Description",
                    "format" => "Format",
                    "identifier" => "Identifier",
                    "language" => "Language",
                    "publisher" => "Publisher",
                    "relation" => "Relation",
                    "rights" => "Rights",
                    "source" => "Source",
                    "type" => "Type",
                    "contributor" => "Contributor",
                    "coverage" => "Coverage",
                    "date" => "Date",
                    _ => "",
                };
                if !tag.is_empty() {
                    return (tag.to_string(), "dc".to_string());
                }
                return (capitalize_tag(local), "dc".to_string());
            }
            "ncc" => {
                // NCC (Daisy 2.02) tags
                let tag = match local {
                    "charset" => "CharacterSet",
                    "depth" => "Depth",
                    "files" => "Files",
                    "footnotes" => "Footnotes",
                    "generator" => "Generator",
                    "kbytesize" => "KByteSize",
                    "maxpagenormal" => "MaxPageNormal",
                    "multimediatype" => "MultimediaType",
                    "narrator" => "Narrator",
                    "pagefront" => "PageFront",
                    "pagenormal" => "PageNormal",
                    "pagespecial" => "PageSpecial",
                    "prodnotes" => "ProdNotes",
                    "producer" => "Producer",
                    "produceddate" => "ProducedDate",
                    "revision" => "Revision",
                    "revisiondate" => "RevisionDate",
                    "setinfo" => "SetInfo",
                    "sidebars" => "Sidebars",
                    "sourcedate" => "SourceDate",
                    "sourceedition" => "SourceEdition",
                    "sourcepublisher" => "SourcePublisher",
                    "sourcerights" => "SourceRights",
                    "sourcetitle" => "SourceTitle",
                    "tocitems" => "TOCItems",
                    "totaltime" => "Duration",
                    _ => "",
                };
                if !tag.is_empty() {
                    return (tag.to_string(), "ncc".to_string());
                }
                return (capitalize_tag(local), "ncc".to_string());
            }
            "prod" => {
                // Production namespace
                let tag = match local {
                    "reclocation" => "RecLocation",
                    "recengineer" => "RecEngineer",
                    _ => "",
                };
                if !tag.is_empty() {
                    return (tag.to_string(), "prod".to_string());
                }
                return (capitalize_tag(local), "prod".to_string());
            }
            "vw96" => {
                let tag = match local {
                    "objecttype" => "ObjectType",
                    _ => "",
                };
                if !tag.is_empty() {
                    return (tag.to_string(), "vw96".to_string());
                }
                return (capitalize_tag(local), "vw96".to_string());
            }
            "http-equiv" | "http" => {
                // http-equiv tags
                let tag = match local {
                    "content-type" => "ContentType",
                    "content-language" => "ContentLanguage",
                    "content-script-type" => "ContentScriptType",
                    "content-style-type" => "ContentStyleType",
                    "expires" => "Expires",
                    "pragma" => "Pragma",
                    "refresh" => "Refresh",
                    _ => "",
                };
                if !tag.is_empty() {
                    return (tag.to_string(), "equiv".to_string());
                }
            }
            _ => {}
        }
    }

    // Check if it's a plain http-equiv name (from name= attribute matching http-equiv tags)
    let tag = match lower.as_str() {
        "content-type" => "ContentType",
        "content-language" => "ContentLanguage",
        "author" => "Author",
        "description" => "Description",
        "keywords" => "Keywords",
        "generator" => "Generator",
        "copyright" => "Copyright",
        "title" => "Title",
        "robots" => "Robots",
        "subject" => "Subject",
        "abstract" => "Abstract",
        "classification" => "Classification",
        "distribution" => "Distribution",
        "formatter" => "Formatter",
        "originator" => "Originator",
        "owner" => "Owner",
        "rating" => "Rating",
        "refresh" => "Refresh",
        _ => "",
    };

    if !tag.is_empty() {
        return (tag.to_string(), "HTML".to_string());
    }

    // Fallback: capitalize and strip special chars
    let tag_name = name.replace([':', '.', '-', ' '], "");
    (tag_name, "HTML".to_string())
}

/// Capitalize first letter of a tag name (for unknown namespace locals).
fn capitalize_tag(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

/// Parse embedded Microsoft Office XML from HTML comments like <!--[if gte mso 9]><xml>...</xml>
fn parse_mso_xml(text: &str, tags: &mut Vec<Tag>) {
    // Find MSO XML block
    let xml_start = if let Some(p) = text.find("><xml>") {
        p + 1
    } else if let Some(p) = text.find("<xml>") {
        p
    } else {
        return;
    };

    let xml_section = &text[xml_start..];
    let xml_end = if let Some(p) = xml_section.find("</xml>") {
        p + 6
    } else {
        return;
    };

    let xml = &xml_section[..xml_end];

    // Parse o:DocumentProperties
    parse_office_xml_section(xml, "o:DocumentProperties", tags);

    // Parse o:CustomDocumentProperties
    parse_office_custom_props(xml, tags);
}

/// Parse a named XML section for Office document properties.
fn parse_office_xml_section(xml: &str, section: &str, tags: &mut Vec<Tag>) {
    let open = format!("<{}>", section);
    let close = format!("</{}>", section);

    let start = if let Some(p) = xml.find(&open) { p + open.len() } else { return; };
    let end = if let Some(p) = xml[start..].find(&close) { start + p } else { return; };

    let section_xml = &xml[start..end];

    // Known field mappings for o:DocumentProperties
    let fields = [
        ("Subject", "Subject"),
        ("Author", "Author"),
        ("Keywords", "Keywords"),
        ("Description", "Description"),
        ("Template", "Template"),
        ("LastAuthor", "LastAuthor"),
        ("Revision", "RevisionNumber"),
        ("TotalTime", "TotalEditTime"),
        ("Created", "CreateDate"),
        ("LastSaved", "ModifyDate"),
        ("LastPrinted", "LastPrinted"),
        ("Pages", "Pages"),
        ("Words", "Words"),
        ("Characters", "Characters"),
        ("Category", "Category"),
        ("Manager", "Manager"),
        ("Company", "Company"),
        ("Lines", "Lines"),
        ("Paragraphs", "Paragraphs"),
        ("CharactersWithSpaces", "CharactersWithSpaces"),
        ("Version", "RevisionNumber"),
    ];

    for (xml_name, tag_name) in &fields {
        let open_tag = format!("<o:{}>", xml_name);
        let close_tag = format!("</o:{}>", xml_name);
        if let Some(val) = extract_between(section_xml, &open_tag, &close_tag) {
            let val = xml_decode(&val);
            if !val.is_empty() {
                // Convert date fields
                let val = if tag_name.contains("Date") || tag_name.contains("Created") || tag_name.contains("Saved") || tag_name.contains("Printed") {
                    convert_xmp_date(&val)
                } else if *tag_name == "TotalEditTime" {
                    // TotalTime is in minutes in Office XML
                    if let Ok(mins) = val.parse::<u64>() {
                        if mins == 1 {
                            "1 minute".to_string()
                        } else {
                            format!("{} minutes", mins)
                        }
                    } else {
                        val
                    }
                } else {
                    val
                };
                tags.push(mk(tag_name, tag_name, Value::String(val)));
            }
        }
    }
}

/// Parse o:CustomDocumentProperties for arbitrary named properties.
fn parse_office_custom_props(xml: &str, tags: &mut Vec<Tag>) {
    let open = "<o:CustomDocumentProperties>";
    let close = "</o:CustomDocumentProperties>";

    let start = if let Some(p) = xml.find(open) { p + open.len() } else { return; };
    let end = if let Some(p) = xml[start..].find(close) { start + p } else { return; };

    let section = &xml[start..end];

    // Each property is like: <o:PropName dt:dt="string">value</o:PropName>
    // where PropName has _x0020_ for spaces
    let mut pos = 0;
    while let Some(tag_start) = section[pos..].find("<o:") {
        let abs_start = pos + tag_start;
        let rest = &section[abs_start + 3..]; // skip "<o:"

        // Get tag name (up to '>' or ' ')
        let name_end = rest.find(|c: char| c == '>' || c == ' ' || c == '/').unwrap_or(rest.len());
        let raw_tag_name = &rest[..name_end];

        // Find '>' to get past attributes
        let close_bracket = if let Some(p) = rest.find('>') { p } else { break; };
        let content_start = abs_start + 3 + close_bracket + 1;

        // Find closing tag
        let close_tag = format!("</o:{}>", raw_tag_name);
        if let Some(close_pos) = section[content_start..].find(&close_tag) {
            let value = section[content_start..content_start + close_pos].trim().to_string();
            let value = xml_decode(&value);

            // Decode _x0020_ as space and create clean tag name
            let clean_name = raw_tag_name.replace("_x0020_", " ");
            // Convert to ExifTool-style: capitalize words, remove spaces
            let tag_name = clean_name.split_whitespace()
                .map(|w| {
                    let mut chars = w.chars();
                    match chars.next() {
                        None => String::new(),
                        Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
                    }
                })
                .collect::<String>();

            if !tag_name.is_empty() && !value.is_empty() {
                tags.push(mk(&tag_name, &clean_name, Value::String(value)));
            }

            pos = content_start + close_pos + close_tag.len();
        } else {
            pos = abs_start + 3 + name_end + 1;
        }
    }
}

/// Extract content between two string delimiters.
fn extract_between(s: &str, open: &str, close: &str) -> Option<String> {
    let start = s.find(open)? + open.len();
    let end = s[start..].find(close)?;
    Some(s[start..start + end].to_string())
}

/// Convert XMP/ISO 8601 date format to ExifTool format.
fn convert_xmp_date(s: &str) -> String {
    // e.g. "2010-06-28T23:52:00Z" -> "2010:06:28 23:52:00Z"
    if s.len() >= 19 && s.chars().nth(4) == Some('-') {
        let date = s[..10].replace('-', ":");
        let time_part = &s[11..];
        format!("{} {}", date, time_part)
    } else if s.len() >= 10 && s.chars().nth(4) == Some('-') {
        s[..10].replace('-', ":")
    } else {
        s.to_string()
    }
}

/// Decode common HTML entities.
fn html_decode(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#13;", "\r")
        .replace("&#10;", "\n")
}

/// Decode XML entities.
fn xml_decode(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#13;", "\r")
        .replace("&#10;", "\n")
}

fn extract_attr(tag: &str, attr_name: &str) -> Option<String> {
    let lower = tag.to_lowercase();
    let pattern = format!("{}=", attr_name);
    let pos = lower.find(&pattern)?;
    let rest = &tag[pos + pattern.len()..];
    let rest = rest.trim_start();

    if rest.starts_with('"') {
        let end = rest[1..].find('"')?;
        Some(rest[1..1 + end].to_string())
    } else if rest.starts_with('\'') {
        let end = rest[1..].find('\'')?;
        Some(rest[1..1 + end].to_string())
    } else {
        let end = rest.find(|c: char| c.is_whitespace() || c == '>' || c == '/').unwrap_or(rest.len());
        Some(rest[..end].to_string())
    }
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup { family0: "HTML".into(), family1: "HTML".into(), family2: "Document".into() },
        raw_value: value, print_value: pv, priority: 0,
    }
}
