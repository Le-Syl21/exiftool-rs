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
