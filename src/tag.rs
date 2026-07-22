use crate::value::Value;

/// Identifies the metadata group hierarchy (mirrors ExifTool's Group0..Group3).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TagGroup {
    /// Family 0: Information type (EXIF, IPTC, XMP, ICC_Profile, etc.)
    pub family0: String,
    /// Family 1: Specific location (IFD0, ExifIFD, GPS, XMP-dc, etc.)
    pub family1: String,
    /// Family 2: Category (Image, Camera, Location, Time, Author, etc.)
    pub family2: String,
    /// Family 3: Document number ([`MAIN_DOCUMENT`], "Doc1", "Doc2", etc.).
    ///
    /// Tags read from the file itself belong to the main document. Formats that
    /// carry repeated or embedded sub-documents (e.g. the timed messages of a
    /// Garmin FIT file under `ExtractEmbedded`) place each one in its own
    /// numbered document, exactly as ExifTool's `-G3` output does.
    pub family3: String,
}

/// Family 3 value for tags belonging to the file's main document.
pub const MAIN_DOCUMENT: &str = "Main";

/// Sentinel [`Tag::priority`] meaning "the source table gives this tag a
/// priority of 0", as opposed to the struct's plain `0`, which means "the
/// decoder said nothing".
///
/// ExifTool draws the same distinction, through definedness rather than a
/// sentinel. `FoundTag` resolves the priority in two stages: first from the tag
/// and its table (ExifTool.pm:9469-9472),
///
/// ```text
/// my $priority = $$tagInfo{Priority};
/// unless (defined $priority) {
///     $priority = $$tbl{PRIORITY};
///     $priority = 0 if not defined $priority and $$tagInfo{Avoid};
/// }
/// ```
///
/// then, only when that left it undefined, from the directory
/// (ExifTool.pm:9552-9562): 0 for a LOW_PRIORITY_DIR, otherwise the normal
/// default of 1. A table-stated 0 and an unstated priority therefore take
/// different branches, and a stated 0 is additionally promoted back to 1 inside
/// the PRIORITY_DIR. `Avoid => 1` lands in the *stated* branch, since the block
/// above assigns to `$priority` and thereby defines it.
///
/// A distinct value is used rather than a new field because the priority takes
/// part in the duplicate competition as a plain number everywhere else, and
/// several readers already set values of their own (-1, 2, 5, 10).
pub const PRIORITY_EXPLICIT_ZERO: i32 = i32::MIN;

/// Build a `Warning` tag the way `Image::ExifTool::Warn` does.
///
/// `%Image::ExifTool::Extra` declares `Warning => { Priority => 0, Groups =>
/// \%allGroupsExifTool }` (ExifTool.pm:1298-1300). The stated `Priority => 0` is
/// what makes the FIRST warning of a file the one reported when duplicates are
/// collapsed: FoundTag promotes the stored tag's 0 to 1 for `Warning`
/// unconditionally ("never override a Warning tag because they may be added by
/// ValueConv", ExifTool.pm:9541-9548), so the incoming 0 never reaches it.
pub fn warning_tag(message: impl Into<String>) -> Tag {
    let message = message.into();
    Tag {
        id: TagId::Text("Warning".into()),
        name: "Warning".into(),
        description: "Warning".into(),
        group: TagGroup {
            family0: "ExifTool".into(),
            family1: "ExifTool".into(),
            family2: "Other".into(),
            family3: MAIN_DOCUMENT.into(),
        },
        raw_value: crate::value::Value::String(message.clone()),
        print_value: message,
        priority: PRIORITY_EXPLICIT_ZERO,
    }
}

impl Default for TagGroup {
    /// An empty group in the main document.
    fn default() -> Self {
        Self {
            family0: String::new(),
            family1: String::new(),
            family2: String::new(),
            family3: MAIN_DOCUMENT.to_string(),
        }
    }
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
    /// The priority as a plain comparable number, with
    /// [`PRIORITY_EXPLICIT_ZERO`] folded back to the 0 it stands for.
    ///
    /// Use it wherever priorities are merely ranked against each other. Only
    /// the duplicate arbitration needs to tell a table-stated 0 from an
    /// unstated priority, and it reads [`Tag::priority`] directly.
    pub fn priority_rank(&self) -> i32 {
        if self.priority == PRIORITY_EXPLICIT_ZERO {
            0
        } else {
            self.priority
        }
    }

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
                family3: "Main".into(),
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
        assert_eq!(tag.display_value(true), "0.01");
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
            family3: "Main".into(),
        };
        let g2 = g1.clone();
        assert_eq!(g1, g2);
    }
}
