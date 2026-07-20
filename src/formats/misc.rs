//! Shared helper functions for miscellaneous format readers.

use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

/// Build a tag whose family-0 and family-1 groups are both `family`.
///
/// `family` is the name of the ExifTool table the tag comes from, not the file
/// type: several formats ExifTool parses inline (BMP, PCX, PGF, ICO, PPM, PICT,
/// BPG, FLIF, WPG, PCAP) have tables grouped `File`, so their readers pass
/// `"File"` here.
pub(crate) fn mktag(family: &str, name: &str, description: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: family.into(),
            family1: family.into(),
            family2: "Other".into(),
            family3: "Main".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}
