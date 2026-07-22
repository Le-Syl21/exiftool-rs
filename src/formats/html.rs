//! HTML metadata reader.
//!
//! Extracts `<meta>` tags, `<title>`, embedded MSO XML, and XMP from HTML files.
//! Mirrors ExifTool's HTML.pm.

use crate::error::{Error, Result};
use crate::metadata::XmpReader;
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_html(data: &[u8]) -> Result<Vec<Tag>> {
    let text = crate::encoding::decode_utf8_or_latin1(data);
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

        // `http-equiv` is a namespace of its own in ExifTool, so remember which
        // attribute carried the name.
        let mut from_http_equiv = false;
        let name = extract_attr(meta_tag, "name")
            .or_else(|| extract_attr(meta_tag, "property"))
            .or_else(|| {
                let v = extract_attr(meta_tag, "http-equiv");
                from_http_equiv = v.is_some();
                v
            });
        let content = extract_attr(meta_tag, "content");

        if let (Some(name_raw), Some(content)) = (name, content) {
            if !name_raw.is_empty() && !content.is_empty() {
                let (tag_name, group) = map_html_meta_name(&name_raw);
                let group = if from_http_equiv { "equiv" } else { &group };
                if !tag_name.is_empty() {
                    tags.push(mk_group(
                        &meta_group1(group),
                        &tag_name,
                        &name_raw,
                        Value::String(html_decode(&content)),
                    ));
                }
            }
        }

        search_pos = abs_pos + end.max(5);
    }

    // HTML.pm marks several meta tags as List (Seq/Bag): repeated values are
    // accumulated into one comma-joined tag rather than emitted separately.
    const HTML_LIST_TAGS: &[&str] = &[
        "Keywords",
        "Robots",
        "Googlebot",
        "Contributor",
        "Coverage",
        "Creator",
        "Identifier",
        "Language",
        "Publisher",
        "Relation",
        "Source",
        "Subject",
        "Type",
    ];
    {
        let mut merged: Vec<Tag> = Vec::with_capacity(tags.len());
        for tag in tags.into_iter() {
            if HTML_LIST_TAGS.contains(&tag.name.as_str()) {
                if let Some(existing) = merged
                    .iter_mut()
                    .find(|t| t.name == tag.name && t.group.family0 == tag.group.family0)
                {
                    // Grow a list so JSON emits an array for these List=>1 tags,
                    // keeping print_value as the ", "-joined string.
                    match &mut existing.raw_value {
                        Value::List(items) => items.push(Value::String(tag.print_value.clone())),
                        _ => {
                            existing.raw_value = Value::List(vec![
                                Value::String(existing.print_value.clone()),
                                Value::String(tag.print_value.clone()),
                            ]);
                        }
                    }
                    existing.print_value = format!("{}, {}", existing.print_value, tag.print_value);
                    continue;
                }
            }
            merged.push(tag);
        }
        tags = merged;
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

    let start = if let Some(p) = xml.find(&open) {
        p + open.len()
    } else {
        return;
    };
    let end = if let Some(p) = xml[start..].find(&close) {
        start + p
    } else {
        return;
    };

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
                let val = if tag_name.contains("Date")
                    || tag_name.contains("Created")
                    || tag_name.contains("Saved")
                    || tag_name.contains("Printed")
                {
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
                tags.push(mk_office(tag_name, tag_name, Value::String(val)));
            }
        }
    }
}

/// Parse o:CustomDocumentProperties for arbitrary named properties.
fn parse_office_custom_props(xml: &str, tags: &mut Vec<Tag>) {
    let open = "<o:CustomDocumentProperties>";
    let close = "</o:CustomDocumentProperties>";

    let start = if let Some(p) = xml.find(open) {
        p + open.len()
    } else {
        return;
    };
    let end = if let Some(p) = xml[start..].find(close) {
        start + p
    } else {
        return;
    };

    let section = &xml[start..end];

    // Each property is like: <o:PropName dt:dt="string">value</o:PropName>
    // where PropName has _x0020_ for spaces
    let mut pos = 0;
    while let Some(tag_start) = section[pos..].find("<o:") {
        let abs_start = pos + tag_start;
        let rest = &section[abs_start + 3..]; // skip "<o:"

        // Get tag name (up to '>' or ' ')
        let name_end = rest.find(['>', ' ', '/']).unwrap_or(rest.len());
        let raw_tag_name = &rest[..name_end];

        // Find '>' to get past attributes
        let close_bracket = if let Some(p) = rest.find('>') {
            p
        } else {
            break;
        };
        let content_start = abs_start + 3 + close_bracket + 1;

        // Find closing tag
        let close_tag = format!("</o:{}>", raw_tag_name);
        if let Some(close_pos) = section[content_start..].find(&close_tag) {
            let value = section[content_start..content_start + close_pos]
                .trim()
                .to_string();
            let value = xml_decode(&value);

            // Decode _x0020_ as space and create clean tag name
            let clean_name = raw_tag_name.replace("_x0020_", " ");
            // Convert to ExifTool-style: capitalize words, remove spaces
            let tag_name = clean_name
                .split_whitespace()
                .map(|w| {
                    let mut chars = w.chars();
                    match chars.next() {
                        None => String::new(),
                        Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
                    }
                })
                .collect::<String>();

            if !tag_name.is_empty() && !value.is_empty() {
                tags.push(mk_office(&tag_name, &clean_name, Value::String(value)));
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

/// `%Image::ExifTool::HTML::entityNum` (HTML.pm lines 38-95): every named HTML
/// character reference, sorted by name for binary search.
#[rustfmt::skip]
static HTML_ENTITIES: &[(&str, u32)] = &[
    ("AElig", 198),
    ("Aacute", 193),
    ("Acirc", 194),
    ("Agrave", 192),
    ("Alpha", 913),
    ("Aring", 197),
    ("Atilde", 195),
    ("Auml", 196),
    ("Beta", 914),
    ("Ccedil", 199),
    ("Chi", 935),
    ("Dagger", 8225),
    ("Delta", 916),
    ("ETH", 208),
    ("Eacute", 201),
    ("Ecirc", 202),
    ("Egrave", 200),
    ("Epsilon", 917),
    ("Eta", 919),
    ("Euml", 203),
    ("Gamma", 915),
    ("Iacute", 205),
    ("Icirc", 206),
    ("Igrave", 204),
    ("Iota", 921),
    ("Iuml", 207),
    ("Kappa", 922),
    ("Lambda", 923),
    ("Mu", 924),
    ("Ntilde", 209),
    ("Nu", 925),
    ("OElig", 338),
    ("Oacute", 211),
    ("Ocirc", 212),
    ("Ograve", 210),
    ("Omega", 937),
    ("Omicron", 927),
    ("Oslash", 216),
    ("Otilde", 213),
    ("Ouml", 214),
    ("Phi", 934),
    ("Pi", 928),
    ("Prime", 8243),
    ("Psi", 936),
    ("Rho", 929),
    ("Scaron", 352),
    ("Sigma", 931),
    ("THORN", 222),
    ("Tau", 932),
    ("Theta", 920),
    ("Uacute", 218),
    ("Ucirc", 219),
    ("Ugrave", 217),
    ("Upsilon", 933),
    ("Uuml", 220),
    ("Xi", 926),
    ("Yacute", 221),
    ("Yuml", 376),
    ("Zeta", 918),
    ("aacute", 225),
    ("acirc", 226),
    ("acute", 180),
    ("aelig", 230),
    ("agrave", 224),
    ("alefsym", 8501),
    ("alpha", 945),
    ("amp", 38),
    ("and", 8743),
    ("ang", 8736),
    ("apos", 39),
    ("aring", 229),
    ("asymp", 8776),
    ("atilde", 227),
    ("auml", 228),
    ("bdquo", 8222),
    ("beta", 946),
    ("brvbar", 166),
    ("bull", 8226),
    ("cap", 8745),
    ("ccedil", 231),
    ("cedil", 184),
    ("cent", 162),
    ("chi", 967),
    ("circ", 710),
    ("clubs", 9827),
    ("cong", 8773),
    ("copy", 169),
    ("crarr", 8629),
    ("cup", 8746),
    ("curren", 164),
    ("dArr", 8659),
    ("dagger", 8224),
    ("darr", 8595),
    ("deg", 176),
    ("delta", 948),
    ("diams", 9830),
    ("divide", 247),
    ("eacute", 233),
    ("ecirc", 234),
    ("egrave", 232),
    ("empty", 8709),
    ("emsp", 8195),
    ("ensp", 8194),
    ("epsilon", 949),
    ("equiv", 8801),
    ("eta", 951),
    ("eth", 240),
    ("euml", 235),
    ("euro", 8364),
    ("exist", 8707),
    ("fnof", 402),
    ("forall", 8704),
    ("frac12", 189),
    ("frac14", 188),
    ("frac34", 190),
    ("frasl", 8260),
    ("gamma", 947),
    ("ge", 8805),
    ("gt", 62),
    ("hArr", 8660),
    ("harr", 8596),
    ("hearts", 9829),
    ("hellip", 8230),
    ("iacute", 237),
    ("icirc", 238),
    ("iexcl", 161),
    ("igrave", 236),
    ("image", 8465),
    ("infin", 8734),
    ("int", 8747),
    ("iota", 953),
    ("iquest", 191),
    ("isin", 8712),
    ("iuml", 239),
    ("kappa", 954),
    ("lArr", 8656),
    ("lambda", 955),
    ("lang", 9001),
    ("laquo", 171),
    ("larr", 8592),
    ("lceil", 8968),
    ("ldquo", 8220),
    ("le", 8804),
    ("lfloor", 8970),
    ("lowast", 8727),
    ("loz", 9674),
    ("lrm", 8206),
    ("lsaquo", 8249),
    ("lsquo", 8216),
    ("lt", 60),
    ("macr", 175),
    ("mdash", 8212),
    ("micro", 181),
    ("middot", 183),
    ("minus", 8722),
    ("mu", 956),
    ("nabla", 8711),
    ("nbsp", 160),
    ("ndash", 8211),
    ("ne", 8800),
    ("ni", 8715),
    ("not", 172),
    ("notin", 8713),
    ("nsub", 8836),
    ("ntilde", 241),
    ("nu", 957),
    ("oacute", 243),
    ("ocirc", 244),
    ("oelig", 339),
    ("ograve", 242),
    ("oline", 8254),
    ("omega", 969),
    ("omicron", 959),
    ("oplus", 8853),
    ("or", 8744),
    ("ordf", 170),
    ("ordm", 186),
    ("oslash", 248),
    ("otilde", 245),
    ("otimes", 8855),
    ("ouml", 246),
    ("para", 182),
    ("part", 8706),
    ("permil", 8240),
    ("perp", 8869),
    ("phi", 966),
    ("pi", 960),
    ("piv", 982),
    ("plusmn", 177),
    ("pound", 163),
    ("prime", 8242),
    ("prod", 8719),
    ("prop", 8733),
    ("psi", 968),
    ("quot", 34),
    ("rArr", 8658),
    ("radic", 8730),
    ("rang", 9002),
    ("raquo", 187),
    ("rarr", 8594),
    ("rceil", 8969),
    ("rdquo", 8221),
    ("real", 8476),
    ("reg", 174),
    ("rfloor", 8971),
    ("rho", 961),
    ("rlm", 8207),
    ("rsaquo", 8250),
    ("rsquo", 8217),
    ("sbquo", 8218),
    ("scaron", 353),
    ("sdot", 8901),
    ("sect", 167),
    ("shy", 173),
    ("sigma", 963),
    ("sigmaf", 962),
    ("sim", 8764),
    ("spades", 9824),
    ("sub", 8834),
    ("sube", 8838),
    ("sum", 8721),
    ("sup", 8835),
    ("sup1", 185),
    ("sup2", 178),
    ("sup3", 179),
    ("supe", 8839),
    ("szlig", 223),
    ("tau", 964),
    ("there4", 8756),
    ("theta", 952),
    ("thetasym", 977),
    ("thinsp", 8201),
    ("thorn", 254),
    ("tilde", 732),
    ("times", 215),
    ("trade", 8482),
    ("uArr", 8657),
    ("uacute", 250),
    ("uarr", 8593),
    ("ucirc", 251),
    ("ugrave", 249),
    ("uml", 168),
    ("upsih", 978),
    ("upsilon", 965),
    ("uuml", 252),
    ("weierp", 8472),
    ("xi", 958),
    ("yacute", 253),
    ("yen", 165),
    ("yuml", 255),
    ("zeta", 950),
    ("zwj", 8205),
    ("zwnj", 8204),
];

/// `%Image::ExifTool::XMP::charNum` (XMP.pm line 2874): the five references XML
/// itself defines, which is all a bare `UnescapeXML` resolves.
static XML_ENTITIES: &[(&str, u32)] = &[
    ("amp", 38),
    ("apos", 39),
    ("gt", 62),
    ("lt", 60),
    ("quot", 34),
];

/// `Image::ExifTool::XMP::UnescapeXML` (XMP.pm lines 2875-2881) with the
/// `UnescapeChar` lookup (lines 2919-2936): replace every `&name;`, `&#N;` and
/// `&#xH;` the table resolves, and leave anything else exactly as it stands.
fn unescape_refs(s: &str, table: &[(&str, u32)]) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        // `&(#?\w+);` — the reference name is word characters, optionally
        // preceded by '#'.
        let body = &rest[1..];
        let name_len = body
            .char_indices()
            .find(|(i, c)| !(c.is_alphanumeric() || *c == '_' || (*i == 0 && *c == '#')))
            .map_or(body.len(), |(i, _)| i);
        let name = &body[..name_len];
        let terminated = body[name_len..].starts_with(';');
        let code = if !terminated || name.is_empty() {
            None
        } else {
            table
                .binary_search_by(|(n, _)| (*n).cmp(name))
                .ok()
                .map(|i| table[i].1)
                .or_else(|| {
                    let hex = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X"));
                    match hex {
                        Some(h) if !h.is_empty() => u32::from_str_radix(h, 16).ok(),
                        _ => name.strip_prefix('#').and_then(|d| d.parse().ok()),
                    }
                })
        };
        match code.and_then(char::from_u32) {
            Some(c) => {
                out.push(c);
                rest = &body[name_len + 1..];
            }
            None => {
                out.push('&');
                rest = body;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `UnescapeHTML` (HTML.pm lines 401-405): UnescapeXML over the full HTML
/// entity table.
fn html_decode(s: &str) -> String {
    unescape_refs(s, HTML_ENTITIES)
}

/// A bare `UnescapeXML` (HTML.pm line 515), which resolves only the five XML
/// references.
fn xml_decode(s: &str) -> String {
    unescape_refs(s, XML_ENTITIES)
}

fn extract_attr(tag: &str, attr_name: &str) -> Option<String> {
    let lower = tag.to_lowercase();
    let pattern = format!("{}=", attr_name);
    let pos = lower.find(&pattern)?;
    let rest = &tag[pos + pattern.len()..];
    let rest = rest.trim_start();

    if let Some(stripped) = rest.strip_prefix('"') {
        let end = stripped.find('"')?;
        Some(stripped[..end].to_string())
    } else if let Some(stripped) = rest.strip_prefix('\'') {
        let end = stripped.find('\'')?;
        Some(stripped[..end].to_string())
    } else {
        let end = rest
            .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .unwrap_or(rest.len());
        Some(rest[..end].to_string())
    }
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Family-1 group of a `<meta>` tag, from the namespace prefix
/// [`map_html_meta_name`] recognised. ExifTool keeps one table per namespace and
/// names the group after it.
fn meta_group1(namespace: &str) -> String {
    match namespace {
        "HTML" => "HTML".to_string(),
        // http-equiv meta tags are the one namespace not named after HTML.
        "equiv" => "HTTP-equiv".to_string(),
        ns => format!("HTML-{ns}"),
    }
}

/// Build a tag from the Microsoft Office XML block an exported document carries.
fn mk_office(name: &str, description: &str, value: Value) -> Tag {
    mk_group("HTML-office", name, description, value)
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    mk_group("HTML", name, description, value)
}

fn mk_group(group1: &str, name: &str, description: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "HTML".into(),
            family1: group1.into(),
            family2: "Document".into(),
            family3: "Main".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}
