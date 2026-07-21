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

/// Family 2 of `%Image::ExifTool::XMP::other`, the table ExifTool invents tags
/// in when it meets an XMP property from a namespace it has no schema for.
const XMP_UNKNOWN_NAMESPACE: &str = "Unknown";

/// Pick a category out of the candidates ExifTool uses for one key.
///
/// A single candidate is the answer. Several mean the tables disagree and none
/// is more common than the others, so the category depends on the originating
/// table: `current` wins if it is one of them, otherwise there is nothing better
/// to go on than the first candidate.
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
    // A CIFF record embedded in a JPEG is read with the very tables a .crw file
    // uses; ExifTool only overrides their family-1 group name (`Groups => { 1 =>
    // 'CIFF' }`), so the categories must be looked up under the real ones.
    let aliases: &[&str] = if family1 == "CIFF" {
        &["CanonRaw", "Canon"]
    } else if family0 == "QuickTime" && family1 != "QuickTime" {
        // A QuickTime tag's family-1 group names the directory it was found in
        // (Track1, ItemList, UserData, …); the categories are keyed on the
        // container itself.
        &["QuickTime"]
    } else {
        &[]
    };
    for g1 in std::iter::once(family1).chain(aliases.iter().copied()) {
        let key3 = format!("{family0}{SEP}{g1}{SEP}{name}");
        if let Some(c) = lookup(FAMILY2_BY_G0_G1_NAME, &key3) {
            return Some(choose(c, current));
        }
    }
    let key2 = format!("{family0}{SEP}{name}");
    if let Some(c) = lookup(FAMILY2_BY_G0_NAME, &key2) {
        return Some(choose(c, current));
    }
    // An XMP property none of ExifTool's schemas describe goes to XMP::other,
    // whose category is Unknown. Falling through to the bare name would drag in
    // an unrelated namespace's answer, so stop here.
    if family0 == "XMP" {
        return Some(XMP_UNKNOWN_NAMESPACE);
    }
    // FITS::Main invents a dynamic tag for every keyword it has no entry for,
    // inheriting its GROUPS => { 2 => 'Image' } default; the reader already
    // resolves the whole FITS table (Image, plus the Time/Author overrides), so
    // the bare-name tier must not run — it would mis-assign a dynamic tag whose
    // name collides with an unrelated table (Checksum -> Location, Creator ->
    // Author). Keep the reader's category.
    if family0 == "FITS" {
        return None;
    }
    // VCalendar (iCalendar) tags are resolved in full by the VCard reader from
    // the VCalendar table (Document default, Time/Location overrides, inherited
    // by prefixed component and TZID-parameter tags). The bare-name tier would
    // mis-assign a dynamic compound tag whose name collides with an unrelated
    // table (SequenceNumber -> Camera, Summary -> Video), so keep the reader's
    // category.
    if family0 == "VCalendar" {
        return None;
    }
    // The MRW reader assigns each MinoltaRaw sub-table's category directly
    // (PRD/WBG/Main => Camera, RIF => Image; MinoltaRaw.pm). The bare-name tier
    // would flip a PRD dimension to Image (ImageWidth, ImageHeight, BitDepth) or
    // keep a RIF setting at Camera, so keep the reader's category.
    if family1 == "MinoltaRaw" {
        return None;
    }
    lookup(FAMILY2_BY_NAME, name).map(|c| choose(c, current))
}

/// Whether an XMP property belongs to no schema ExifTool knows.
///
/// Such a property gets a tag description invented on the spot -- XMP.pm's
/// `{ Name => $name, IsDefault => 1, Priority => 0 }` -- so it carries priority
/// 0 and can never displace an already-stored tag of the same name. This is the
/// same lookup [`family2_for`] uses to send the tag to `XMP::other`, but it
/// reads only the property's own name and namespace, so it may be asked before
/// family 2 has been resolved.
///
/// ```
/// # use exiftool_rs::tags::group2::xmp_property_is_unknown;
/// assert!(xmp_property_is_unknown("XMP-Nikon", "Iso"));
/// assert!(!xmp_property_is_unknown("XMP-dc", "Title"));
/// ```
pub fn xmp_property_is_unknown(family1: &str, name: &str) -> bool {
    let key3 = format!("XMP{SEP}{family1}{SEP}{name}");
    if lookup(FAMILY2_BY_G0_G1_NAME, &key3).is_some() {
        return false;
    }
    let key2 = format!("XMP{SEP}{name}");
    lookup(FAMILY2_BY_G0_NAME, &key2).is_none()
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
    fn xmp_property_from_an_unschemed_namespace_is_unknown() {
        assert_eq!(
            family2_for("XMP", "XMP-XMP", "KmlDocumentPlacemarkName", "Other"),
            Some("Unknown")
        );
        // A property ExifTool does know keeps its schema's category.
        assert_eq!(
            family2_for("XMP", "XMP-dc", "Creator", "Other"),
            Some("Author")
        );
    }

    #[test]
    fn unknown_tag_yields_none() {
        assert_eq!(family2_for("EXIF", "IFD0", "NoSuchTagName", "Image"), None);
    }
}
