//! IPTC-IIM metadata writer.
//!
//! Builds IPTC-IIM binary data from tag name-value pairs.

/// An IPTC record to write.
pub struct IptcRecord {
    pub record: u8,
    pub dataset: u8,
    pub data: Vec<u8>,
}

/// Build IPTC-IIM binary data from a list of records.
pub fn build_iptc(records: &[IptcRecord]) -> Vec<u8> {
    let mut output = Vec::new();

    for rec in records {
        if rec.data.len() > 0x7FFF {
            continue; // Skip oversized records (no extended length support yet)
        }

        output.push(0x1C); // Tag marker
        output.push(rec.record);
        output.push(rec.dataset);
        let len = rec.data.len() as u16;
        output.extend_from_slice(&len.to_be_bytes());
        output.extend_from_slice(&rec.data);
    }

    output
}

/// Map IPTC tag name to (record, dataset).
pub fn tag_name_to_iptc(name: &str) -> Option<(u8, u8)> {
    Some(match name.to_lowercase().as_str() {
        "objectname" | "title" => (2, 5),
        "urgency" => (2, 10),
        "category" => (2, 15),
        "supplementalcategories" => (2, 20),
        "keywords" => (2, 25),
        "specialinstructions" => (2, 40),
        "datecreated" => (2, 55),
        "timecreated" => (2, 60),
        "by-line" | "author" | "byline" => (2, 80),
        "by-linetitle" | "authorsposition" | "bylinetitle" => (2, 85),
        "city" => (2, 90),
        "sub-location" | "sublocation" => (2, 92),
        "province-state" | "state" | "provincestate" => (2, 95),
        "country-primarylocationcode" | "countrycode" => (2, 100),
        "country-primarylocationname" | "country" => (2, 101),
        "headline" => (2, 105),
        "credit" => (2, 110),
        "source" => (2, 115),
        "copyrightnotice" | "copyright" => (2, 116),
        "contact" => (2, 118),
        "caption-abstract" | "caption" | "description" => (2, 120),
        "writer-editor" | "captionwriter" => (2, 122),
        _ => return None,
    })
}
