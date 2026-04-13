use crate::value::Value;

/// Identifies the metadata group hierarchy (mirrors ExifTool's Group0/Group1/Group2).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TagGroup {
    /// Family 0: Information type (EXIF, IPTC, XMP, ICC_Profile, etc.)
    pub family0: String,
    /// Family 1: Specific location (IFD0, ExifIFD, GPS, XMP-dc, etc.)
    pub family1: String,
    /// Family 2: Category (Image, Camera, Location, Time, Author, etc.)
    pub family2: String,
}

/// A resolved metadata tag with its value and metadata.
#[derive(Debug, Clone)]
pub struct Tag {
    /// Tag identifier (numeric for EXIF/IPTC, string key for XMP)
    pub id: TagId,
    /// Canonical tag name (e.g., "ExposureTime", "Artist", "GPSLatitude")
    pub name: String,
    /// Human-readable description
    pub description: String,
    /// Group hierarchy
    pub group: TagGroup,
    /// The raw value
    pub raw_value: Value,
    /// Human-readable print conversion of the value
    pub print_value: String,
    /// Priority for conflict resolution (higher wins)
    pub priority: i32,
}

impl Tag {
    /// Get the display value respecting the print_conv option.
    /// When `numeric` is true (-n flag), returns the raw value.
    /// When `numeric` is false, returns the print-converted value.
    pub fn display_value(&self, numeric: bool) -> String {
        if numeric {
            self.raw_value.to_display_string()
        } else {
            self.print_value.clone()
        }
    }
}

/// Tag identifier - can be numeric (EXIF/IPTC) or string (XMP).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TagId {
    /// Numeric ID (EXIF IFD tag, IPTC record:dataset)
    Numeric(u16),
    /// String key (XMP property path)
    Text(String),
}

impl std::fmt::Display for TagId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TagId::Numeric(id) => write!(f, "0x{:04x}", id),
            TagId::Text(s) => write!(f, "{}", s),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tag(raw: Value, print: &str) -> Tag {
        Tag {
            id: TagId::Numeric(0x0001),
            name: "TestTag".to_string(),
            description: "Test Tag".to_string(),
            group: TagGroup {
                family0: "EXIF".to_string(),
                family1: "IFD0".to_string(),
                family2: "Image".to_string(),
            },
            raw_value: raw,
            print_value: print.to_string(),
            priority: 0,
        }
    }

    // ── TagId Display ──────────────────────────────────────────────

    #[test]
    fn tag_id_numeric_display_low() {
        assert_eq!(format!("{}", TagId::Numeric(0x0001)), "0x0001");
    }

    #[test]
    fn tag_id_numeric_display_hex() {
        assert_eq!(format!("{}", TagId::Numeric(0x00FF)), "0x00ff");
    }

    #[test]
    fn tag_id_numeric_display_zero() {
        assert_eq!(format!("{}", TagId::Numeric(0)), "0x0000");
    }

    #[test]
    fn tag_id_numeric_display_max() {
        assert_eq!(format!("{}", TagId::Numeric(0xFFFF)), "0xffff");
    }

    #[test]
    fn tag_id_text_display() {
        assert_eq!(format!("{}", TagId::Text("dc:title".into())), "dc:title");
    }

    #[test]
    fn tag_id_text_display_empty() {
        assert_eq!(format!("{}", TagId::Text(String::new())), "");
    }

    // ── Tag::display_value ─────────────────────────────────────────

    #[test]
    fn display_value_numeric_true_returns_raw() {
        let tag = make_tag(Value::URational(1, 100), "0.01 s");
        assert_eq!(tag.display_value(true), "1/100");
    }

    #[test]
    fn display_value_numeric_false_returns_print() {
        let tag = make_tag(Value::URational(1, 100), "0.01 s");
        assert_eq!(tag.display_value(false), "0.01 s");
    }

    #[test]
    fn display_value_string_raw() {
        let tag = make_tag(Value::String("Canon EOS R5".into()), "Canon EOS R5");
        assert_eq!(tag.display_value(true), "Canon EOS R5");
        assert_eq!(tag.display_value(false), "Canon EOS R5");
    }

    // ── TagId equality ─────────────────────────────────────────────

    #[test]
    fn tag_id_equality() {
        assert_eq!(TagId::Numeric(42), TagId::Numeric(42));
        assert_ne!(TagId::Numeric(1), TagId::Numeric(2));
        assert_eq!(TagId::Text("foo".into()), TagId::Text("foo".into()));
        assert_ne!(TagId::Numeric(1), TagId::Text("1".into()));
    }

    // ── TagGroup equality ──────────────────────────────────────────

    #[test]
    fn tag_group_equality() {
        let g1 = TagGroup {
            family0: "EXIF".into(),
            family1: "IFD0".into(),
            family2: "Image".into(),
        };
        let g2 = g1.clone();
        assert_eq!(g1, g2);
    }
}
