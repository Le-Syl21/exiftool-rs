//! Family-2 (category) resolution against ExifTool's own group tables.
//!
//! ExifTool gives every tag a family-2 category — `Image`, `Camera`, `Time`,
//! `Author`, … — taken from the tag's `Groups => { 2 => ... }` override or,
//! failing that, from its table's `GROUPS` default. Our format readers each
//! decided that category on their own, which drifted badly from upstream; this
//! module replaces the guesswork with the generated tables in
//! [`super::group2_generated`], which are extracted from ExifTool itself.
//!
//! The lookup is deliberately conservative. A tag NAME does not determine its
//! category — `BitsPerSample` is `Image` under `File` and `Audio` under RIFF —
//! so the tables are consulted from the most specific key to the least, and an
//! ambiguous key defers to whatever the parser already chose: the parser knows
//! which table the tag came from, and a name lookup does not.

use super::group2_generated::{
    Family2Entry, FAMILY2_BY_G0_G1_NAME, FAMILY2_BY_G0_NAME, FAMILY2_BY_NAME,
};

/// Separator between key components. Cannot occur in a group or tag name.
const SEP: char = '\u{1}';

fn lookup<'a>(table: &'a [Family2Entry], key: &str) -> Option<&'a [&'static str]> {
    table
        .binary_search_by(|(k, _)| (*k).cmp(key))
        .ok()
        .map(|i| table[i].1)
}

/// Pick a category out of the candidates ExifTool uses for one key.
///
/// A single candidate is the answer. Several mean the category depends on the
/// originating table, so `current` wins if it is one of them; otherwise there is
/// nothing better to go on than the first candidate.
fn choose(candidates: &[&'static str], current: &str) -> &'static str {
    if candidates.len() == 1 {
        return candidates[0];
    }
    candidates
        .iter()
        .find(|c| **c == current)
        .or_else(|| candidates.first())
        .copied()
        .unwrap_or("Other")
}

/// The family-2 category ExifTool assigns to a tag we placed in `family0` /
/// `family1`, or `None` when ExifTool knows no such tag and `current` must
/// stand.
///
/// ```
/// # use exiftool_rs::tags::group2::family2_for;
/// // The EXIF table default: an exposure time is Image, not Time.
/// assert_eq!(family2_for("EXIF", "ExifIFD", "ExposureTime", "Time"), Some("Image"));
/// // Unknown to ExifTool: leave the parser's choice alone.
/// assert_eq!(family2_for("EXIF", "IFD0", "NotATag", "Image"), None);
/// ```
pub fn family2_for(
    family0: &str,
    family1: &str,
    name: &str,
    current: &str,
) -> Option<&'static str> {
    let key3 = format!("{family0}{SEP}{family1}{SEP}{name}");
    if let Some(c) = lookup(FAMILY2_BY_G0_G1_NAME, &key3) {
        return Some(choose(c, current));
    }
    let key2 = format!("{family0}{SEP}{name}");
    if let Some(c) = lookup(FAMILY2_BY_G0_NAME, &key2) {
        return Some(choose(c, current));
    }
    lookup(FAMILY2_BY_NAME, name).map(|c| choose(c, current))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_sorted_by_key() {
        for table in [FAMILY2_BY_G0_G1_NAME, FAMILY2_BY_G0_NAME, FAMILY2_BY_NAME] {
            assert!(
                table.windows(2).all(|w| w[0].0 < w[1].0),
                "generated family-2 table is not sorted; binary search would misfire"
            );
            assert!(table.iter().all(|(_, cats)| !cats.is_empty()));
        }
    }

    #[test]
    fn exif_table_default_beats_the_tag_name() {
        // ExposureTime carries no Groups override, so it inherits Exif::Main's
        // `2 => 'Image'` — it is NOT a Time tag.
        assert_eq!(
            family2_for("EXIF", "ExifIFD", "ExposureTime", "Time"),
            Some("Image")
        );
    }

    #[test]
    fn gps_tags_are_location() {
        assert_eq!(
            family2_for("EXIF", "GPS", "GPSLatitude", "Image"),
            Some("Location")
        );
    }

    #[test]
    fn ambiguous_name_keeps_the_parsers_choice() {
        // BitsPerSample is Image for images and Audio for sound: both are
        // candidates under the bare name, so whichever the parser set stands.
        assert_eq!(
            family2_for("Nonexistent", "Nonexistent", "BitsPerSample", "Audio"),
            Some("Audio")
        );
        assert_eq!(
            family2_for("Nonexistent", "Nonexistent", "BitsPerSample", "Image"),
            Some("Image")
        );
    }

    #[test]
    fn unknown_tag_yields_none() {
        assert_eq!(family2_for("EXIF", "IFD0", "NoSuchTagName", "Image"), None);
    }
}
