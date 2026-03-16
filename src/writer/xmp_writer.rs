//! XMP metadata writer.
//!
//! Builds XMP XML/RDF from tag name-value pairs.
//! Mirrors ExifTool's WriteXMP.pl.

use std::collections::BTreeMap;

/// An XMP property to write.
#[derive(Debug, Clone)]
pub struct XmpProperty {
    /// Namespace prefix (e.g., "dc", "xmp", "tiff", "exif")
    pub namespace: String,
    /// Property name (e.g., "title", "creator", "description")
    pub property: String,
    /// Value (simple string, or multiple for lists)
    pub values: Vec<String>,
    /// Property type
    pub prop_type: XmpPropertyType,
}

#[derive(Debug, Clone, Copy)]
pub enum XmpPropertyType {
    Simple,
    LangAlt,
    Bag,
    Seq,
}

/// Build XMP XML from a list of properties.
pub fn build_xmp(properties: &[XmpProperty]) -> String {
    let mut xml = String::new();

    // xpacket header
    xml.push_str("<?xpacket begin='\u{FEFF}' id='W5M0MpCehiHzreSzNTczkc9d'?>\n");
    xml.push_str("<x:xmpmeta xmlns:x='adobe:ns:meta/'>\n");
    xml.push_str("<rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'>\n");

    // Group properties by namespace
    let mut by_ns: BTreeMap<String, Vec<&XmpProperty>> = BTreeMap::new();
    for prop in properties {
        by_ns.entry(prop.namespace.clone()).or_default().push(prop);
    }

    // Collect all needed namespace declarations
    let mut ns_decls = Vec::new();
    for ns in by_ns.keys() {
        if let Some(uri) = namespace_uri(ns) {
            ns_decls.push(format!("xmlns:{}='{}'", ns, uri));
        }
    }

    // Open rdf:Description with all namespace declarations
    xml.push_str("<rdf:Description rdf:about=''\n");
    for decl in &ns_decls {
        xml.push_str("  ");
        xml.push_str(decl);
        xml.push('\n');
    }
    xml.push_str(">\n");

    // Write properties
    for (ns, props) in &by_ns {
        for prop in props {
            write_property(&mut xml, ns, prop);
        }
    }

    xml.push_str("</rdf:Description>\n");
    xml.push_str("</rdf:RDF>\n");
    xml.push_str("</x:xmpmeta>\n");

    // Padding for in-place editing (ExifTool adds ~2kB of padding)
    for _ in 0..24 {
        xml.push_str("                                                                                \n");
    }

    xml.push_str("<?xpacket end='w'?>");

    xml
}

fn write_property(xml: &mut String, ns: &str, prop: &XmpProperty) {
    match prop.prop_type {
        XmpPropertyType::Simple => {
            if let Some(val) = prop.values.first() {
                xml.push_str(&format!(
                    "  <{}:{}>{}</{}:{}>\n",
                    ns, prop.property, escape_xml(val), ns, prop.property
                ));
            }
        }
        XmpPropertyType::LangAlt => {
            if let Some(val) = prop.values.first() {
                xml.push_str(&format!("  <{}:{}>\n", ns, prop.property));
                xml.push_str("    <rdf:Alt>\n");
                xml.push_str(&format!(
                    "      <rdf:li xml:lang='x-default'>{}</rdf:li>\n",
                    escape_xml(val)
                ));
                xml.push_str("    </rdf:Alt>\n");
                xml.push_str(&format!("  </{}:{}>\n", ns, prop.property));
            }
        }
        XmpPropertyType::Bag => {
            xml.push_str(&format!("  <{}:{}>\n", ns, prop.property));
            xml.push_str("    <rdf:Bag>\n");
            for val in &prop.values {
                xml.push_str(&format!("      <rdf:li>{}</rdf:li>\n", escape_xml(val)));
            }
            xml.push_str("    </rdf:Bag>\n");
            xml.push_str(&format!("  </{}:{}>\n", ns, prop.property));
        }
        XmpPropertyType::Seq => {
            xml.push_str(&format!("  <{}:{}>\n", ns, prop.property));
            xml.push_str("    <rdf:Seq>\n");
            for val in &prop.values {
                xml.push_str(&format!("      <rdf:li>{}</rdf:li>\n", escape_xml(val)));
            }
            xml.push_str("    </rdf:Seq>\n");
            xml.push_str(&format!("  </{}:{}>\n", ns, prop.property));
        }
    }
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn namespace_uri(prefix: &str) -> Option<&'static str> {
    Some(match prefix {
        "dc" => "http://purl.org/dc/elements/1.1/",
        "xmp" => "http://ns.adobe.com/xap/1.0/",
        "xmpMM" => "http://ns.adobe.com/xap/1.0/mm/",
        "xmpRights" => "http://ns.adobe.com/xap/1.0/rights/",
        "tiff" => "http://ns.adobe.com/tiff/1.0/",
        "exif" => "http://ns.adobe.com/exif/1.0/",
        "exifEX" => "http://cipa.jp/exif/1.0/",
        "aux" => "http://ns.adobe.com/exif/1.0/aux/",
        "photoshop" => "http://ns.adobe.com/photoshop/1.0/",
        "crs" => "http://ns.adobe.com/camera-raw-settings/1.0/",
        "lr" => "http://ns.adobe.com/lightroom/1.0/",
        "Iptc4xmpCore" => "http://iptc.org/std/Iptc4xmpCore/1.0/xmlns/",
        "Iptc4xmpExt" => "http://iptc.org/std/Iptc4xmpExt/2008-02-29/",
        "pdf" => "http://ns.adobe.com/pdf/1.3/",
        "pdfx" => "http://ns.adobe.com/pdfx/1.3/",
        _ => return None,
    })
}
