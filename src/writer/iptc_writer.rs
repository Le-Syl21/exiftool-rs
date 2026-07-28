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

/// Parse an existing IPTC-IIM block into its datasets, so a write can merge
/// changes into it instead of replacing the whole block (issue #7).
///
/// Each dataset is `0x1C <record> <dataset> <len:u16-be> <data>`. Extended
/// length (top bit of the length field set) is rare and unsupported: parsing
/// stops there rather than risk misreading the rest.
pub fn parse_iim(data: &[u8]) -> Vec<IptcRecord> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos + 5 <= data.len() {
        if data[pos] != 0x1C {
            break;
        }
        let record = data[pos + 1];
        let dataset = data[pos + 2];
        let len = u16::from_be_bytes([data[pos + 3], data[pos + 4]]);
        if len & 0x8000 != 0 {
            break; // extended length, not supported
        }
        let len = len as usize;
        pos += 5;
        if pos + len > data.len() {
            break;
        }
        out.push(IptcRecord {
            record,
            dataset,
            data: data[pos..pos + len].to_vec(),
        });
        pos += len;
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_iim_roundtrips_build() {
        let records = vec![
            IptcRecord {
                record: 1,
                dataset: 90,
                data: vec![0x1B, 0x25, 0x47],
            },
            IptcRecord {
                record: 2,
                dataset: 80,
                data: b"Martxn".to_vec(),
            },
            IptcRecord {
                record: 2,
                dataset: 25,
                data: b"kw".to_vec(),
            },
        ];
        let built = build_iptc(&records);
        let parsed = parse_iim(&built);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].record, 1);
        assert_eq!(parsed[0].dataset, 90);
        assert_eq!(parsed[1].data, b"Martxn");
        assert_eq!(parsed[2].dataset, 25);
    }

    #[test]
    fn parse_iim_stops_on_garbage() {
        // Not starting with 0x1C → empty.
        assert!(parse_iim(&[0x00, 0x01, 0x02]).is_empty());
        assert!(parse_iim(&[]).is_empty());
    }

    #[test]
    fn parse_iim_truncated_length_is_safe() {
        // Declares 10 bytes but only 2 follow → drop the incomplete dataset.
        let bytes = [0x1C, 2, 80, 0x00, 0x0A, b'a', b'b'];
        assert!(parse_iim(&bytes).is_empty());
    }
}
