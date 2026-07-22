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

/// Canon tag names carried by more than one Canon binary sub-table with
/// disagreeing `GROUPS => { 2 => ... }`. For these the by-name majority in the
/// generated tables is unreliable (see the guard in [`family2_for`]) and the
/// Canon reader's per-sub-table category is authoritative.
fn is_canon_ambiguous(name: &str) -> bool {
    matches!(
        name,
        "WhiteBalance"
            | "ColorTemperature"
            | "PictureStyle"
            | "Sharpness"
            | "SharpnessFrequency"
            | "MeasuredEV"
            | "MeasuredEV2"
            | "FocusDistanceUpper"
            | "FocusDistanceLower"
            | "FocalLength"
    )
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
    // `Unknown` is not a category any ExifTool table hands out for a tag it
    // describes; it is the default of the tables ExifTool invents entries in for
    // material it has NO description for — `%Image::ExifTool::XMP::other`,
    // `GROUPS => { 2 => 'Unknown' }` (XMP.pm line 2740) above all. A reader that
    // already settled on it has said "this tag is in no ExifTool table", so the
    // lookups below — all keyed on the tag's NAME — have nothing to contribute
    // and could only drag in an unrelated table's answer.
    //
    // This is what keeps the category of a property read back from an
    // ExifTool-written RDF/XML dump at `Unknown`. ExifTool restores such a
    // property's ORIGINAL families 0 and 1 from the namespace URI it was written
    // under — `EXIF:IFD0`, `MakerNotes:Nikon`, `XML:XML-File`, … (XMP.pm lines
    // 3599-3614) — but the tag itself is still invented in `XMP::other`, so its
    // groups no longer name the table it came from and cannot key the lookup.
    if current == XMP_UNKNOWN_NAMESPACE {
        return None;
    }
    // A tag FoundTag invents from a bare NAME that %Image::ExifTool::Extra does
    // not describe gets `Groups => \%allGroupsExifTool`, i.e. `( 0 => 'ExifTool',
    // 1 => 'ExifTool', 2 => 'ExifTool' )` (ExifTool.pm:1226, applied at
    // ExifTool.pm:9465). Such a name belongs to no table, so the lookups below —
    // all keyed on the name — could only drag in an unrelated table's category.
    if family0 == "ExifTool" {
        return None;
    }
    // A few readers label a table with a family-0 group of their own choosing
    // where ExifTool's table declares a different one; the generated tables are
    // keyed on ExifTool's, so resolve the category under it. The KyoceraRaw
    // reader stamps family 0 `KyoceraRaw`, but `KyoceraRaw::Main` is
    // `GROUPS => { 0 => 'MakerNotes' }` (KyoceraRaw.pm line 27), which is where
    // its FNumber/ISO/WB_RGGBLevels overrides to `Image` live. The Font readers
    // stamp `File`, but `Font::Main` is group 0 `Font` (Font.pm line 199), where
    // its `Document` default sits. Both g1 groups are unique to their one table,
    // so the remap can pull no unrelated category in.
    // GoPro is the same story from the other side: in a JPEG APP6 segment the
    // reader (and ExifTool at run time) stamps family 0 `APP6`, but the GoPro
    // tables carry no family-0 override so `-listx` — and thus the generated
    // table — keys them under family 0 `GoPro` (GoPro.pm), where the `Camera`
    // default lives. g1 `GoPro` is unique to those tables.
    let family0 = match family1 {
        "KyoceraRaw" => "MakerNotes",
        "Font" => "Font",
        "GoPro" => "GoPro",
        _ => family0,
    };
    // Three tag names live in a sub-table whose `GROUPS => { 2 => ... }` is
    // outvoted by its siblings, all of which share the same family-1 group so no
    // tier can separate them:
    //   * `Nikon::Main` 0x0002 ISO is `Groups => { 2 => 'Image' }` (Nikon.pm line
    //     1804) while the ISO of every other Nikon table is `Camera`.
    //   * `Sony::PMP` is `GROUPS => { 0 => 'MakerNotes', 2 => 'Image' }` (Sony.pm
    //     line 10632) and its ExposureTime (line 10693) adds no override, while
    //     the Sony maker-note tables put ExposureTime in `Camera`.
    //   * `FlashPix::SummaryInfo` is `GROUPS => { 2 => 'Document' }`
    //     (FlashPix.pm line 388) and its RevisionNumber (line 407) adds no
    //     override, but `FlashPix::DataObject` (line 884) and its siblings are
    //     `Other` and outvote it.
    // Each reader stamps the category of the table it actually decoded, so defer
    // to it — exactly as the Canon guard below does.
    let maker_ambiguous = match family1 {
        "Nikon" => name == "ISO",
        "Sony" => family0 == "MakerNotes" && name == "ExposureTime",
        "FlashPix" => name == "RevisionNumber",
        _ => false,
    };
    if maker_ambiguous {
        return None;
    }
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
    // an unrelated namespace's answer, so stop here — except when the reader saw
    // the property's value parse as a standard date, because ExifTool then
    // overwrites the invented tag's category with `Time` (XMP.pm line 3684).
    if family0 == "XMP" {
        if current == "Time" {
            return None;
        }
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
    // The JSON reader invents a tag for every key it meets; `JSON::Main` is
    // `GROUPS => { 2 => 'Other' }` and its NOTES say ExifTool extracts any key
    // "even if not listed" (JSON.pm line 23), so a dynamic key is `Other`. The
    // reader stamps that; the bare-name tier would drag in an unrelated table's
    // answer (Description -> Video, Title -> Audio, People -> Image), so keep it.
    if family0 == "JSON" {
        return None;
    }
    // The Audible reader extracts every key in an .aa metadata dictionary;
    // `Audible::Main` is `GROUPS => { 2 => 'Audio' }` and its NOTES say any key is
    // kept "even if not listed" (Audible.pm line 25), so a dynamic key is `Audio`
    // (the reader also stamps the table's Time/Author/Preview overrides). The
    // bare-name tier would mis-assign a dynamic key (Codec -> Video,
    // Description -> Video, ShortDescription -> Image), so keep the reader's.
    if family0 == "Audible" {
        return None;
    }
    // The VCard reader builds a tag per vCard property, suffixing the TYPE
    // parameter (AddressWork, PhotoJpeg); `VCard::Main` is `GROUPS => { 2 =>
    // 'Document' }` with per-property overrides (Adr/Geo -> Location,
    // Photo -> Preview, Fn/N -> Author, Bday/Tz -> Time; VCard.pm line 40+), which
    // the reader stamps from the base property. The suffixed name is not in any
    // table, so the bare-name tier would keep `Document`; defer to the reader —
    // exactly as the sibling VCalendar guard does.
    if family0 == "VCard" {
        return None;
    }
    // The MRW reader assigns each MinoltaRaw sub-table's category directly
    // (PRD/WBG/Main => Camera, RIF => Image; MinoltaRaw.pm). The bare-name tier
    // would flip a PRD dimension to Image (ImageWidth, ImageHeight, BitDepth) or
    // keep a RIF setting at Camera, so keep the reader's category.
    if family1 == "MinoltaRaw" {
        return None;
    }
    // A handful of Canon tag names live in several Canon binary sub-tables whose
    // `GROUPS => { 2 => ... }` disagree: e.g. WhiteBalance/ColorTemperature are
    // `Image` in Canon::Processing (Canon.pm line 7203) and Canon::ColorData* but
    // `Camera` in Canon::CameraSettings (line 2220); FocusDistanceUpper/Lower and
    // MeasuredEV2 are `Image` in Canon::ShotInfo (line 2778) but `Camera` in the
    // Canon::CameraInfo* tables. Because `-listx` collapses every one of those
    // sub-tables to the single family-1 group `Canon`, the generated tables cannot
    // tell them apart and the by-name majority answers `Camera`, which is wrong for
    // the instance ExifTool actually keeps. The Canon reader decodes each
    // sub-directory and stamps its true `GROUPS` category (Image for
    // ShotInfo/FocalLength/Processing/FileInfo/ColorData, Camera for
    // CameraSettings/CameraInfo), so for these names defer to whatever it set —
    // exactly as the MinoltaRaw guard does. Only names that genuinely differ per
    // sub-table are listed, so no unambiguous Canon tag is affected.
    if matches!(family1, "Canon" | "CanonRaw" | "CIFF") && is_canon_ambiguous(name) {
        return None;
    }
    // Every table under `%Image::ExifTool::CanonVRD::DR4*` shares
    // `GROUPS => { 1 => 'CanonDR4', 2 => 'Image' }` with no per-tag override
    // (CanonVRD.pm line 1006 and siblings), so the whole CanonDR4 group is Image.
    // The DR4 reader already stamps that; the bare-name tier would wrongly pull a
    // handful (PictureStyle, Rotation, AutoLightingOptimizer) to Camera, so keep
    // the reader's category.
    if family1 == "CanonDR4" {
        return None;
    }
    // The DSS reader resolves `Olympus::DSS` in full — `GROUPS => { 0 =>
    // 'MakerNotes', 2 => 'Audio' }` (Olympus.pm line 4243) with the StartTime /
    // EndTime `Time` overrides — and it is the only reader that stamps family 0
    // `Olympus` (the Olympus maker-note reader stamps `MakerNotes`). The
    // bare-name tier would drag in an unrelated table's answer (Model -> Camera,
    // Duration -> Video), so keep the reader's category.
    if family0 == "Olympus" {
        return None;
    }
    // Every table ExifTool declares as `GROUPS => { 0 => 'XML', 1 => 'XML' }`
    // invents a tag for each property it meets, so a name that reached this far
    // is by construction absent from ExifTool's tables and the bare-name tier
    // could only match an unrelated table by coincidence (Brightness -> Camera,
    // Rotation -> Image). The categories differ per table — `OOXML::Main` is
    // `Document` (OOXML.pm line 56), `CaptureOne::Main` is `Image`
    // (CaptureOne.pm line 26), each with a `Time` override for date-shaped names
    // — and only the reader knows which one it ran, so keep its category.
    if family0 == "XML" && family1 == "XML" {
        return None;
    }
    // `PCAP::Main` is `GROUPS => { 0 => 'File', 1 => 'File', 2 => 'Other' }`
    // (PCAP.pm line 21) while `DPX::Main` (DPX.pm line 23) and `Other::PFM`
    // (Other.pm line 22) are `Image`; all three declare a ByteOrder tag in the
    // very same File/File group, so no tier can separate them and the majority
    // answers `Image`. Each reader stamps its own table's category.
    if family0 == "File" && family1 == "File" && name == "ByteOrder" {
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

    #[test]
    fn canon_ambiguous_names_defer_to_reader() {
        // WhiteBalance/FocusDistanceUpper live in several Canon sub-tables whose
        // categories disagree; the reader knows the real one, so defer to it.
        assert_eq!(
            family2_for("MakerNotes", "Canon", "WhiteBalance", "Image"),
            None
        );
        assert_eq!(
            family2_for("MakerNotes", "CanonRaw", "FocusDistanceUpper", "Camera"),
            None
        );
        // FocalType is a genuine tie (Camera in some tables, Image in others), so
        // the caller keeps whatever the reader decoded — the Canon::FocalLength
        // reader now stamps Image, and the tie honours it.
        assert_eq!(
            family2_for("MakerNotes", "CIFF", "FocalType", "Image"),
            Some("Image")
        );
        // An unambiguous Canon tag is still resolved from the tables.
        assert_eq!(
            family2_for("MakerNotes", "Canon", "SensorWidth", "Camera"),
            Some("Image")
        );
        // Same name outside a Canon group is unaffected by the guard.
        assert_eq!(
            family2_for("MakerNotes", "Nikon", "WhiteBalance", "Camera"),
            Some("Camera")
        );
    }

    #[test]
    fn outvoted_sub_table_names_defer_to_reader() {
        // Nikon::Main ISO is Image but every other Nikon table's ISO is Camera.
        assert_eq!(family2_for("MakerNotes", "Nikon", "ISO", "Image"), None);
        // Sony::PMP is an Image table; the Sony maker-note tables are Camera.
        assert_eq!(
            family2_for("MakerNotes", "Sony", "ExposureTime", "Image"),
            None
        );
        // FlashPix::SummaryInfo is Document; the sibling property sets are Other.
        assert_eq!(
            family2_for("FlashPix", "FlashPix", "RevisionNumber", "Document"),
            None
        );
        // A neighbouring name in the same group is still resolved from the tables.
        assert_eq!(
            family2_for("FlashPix", "FlashPix", "Dictionary", "Document"),
            Some("Other")
        );
        assert_eq!(
            family2_for("MakerNotes", "Nikon", "WhiteBalance", "Image"),
            Some("Camera")
        );
    }

    #[test]
    fn olympus_dss_group_is_resolved_by_the_reader() {
        // Olympus::DSS is Audio with Time overrides; the bare name would answer
        // Camera for Model and Video for Duration.
        assert_eq!(family2_for("Olympus", "Olympus", "Model", "Audio"), None);
        assert_eq!(family2_for("Olympus", "Olympus", "Duration", "Audio"), None);
        // The Olympus maker notes are family 0 MakerNotes and unaffected: their
        // Model is resolved from the tables as usual.
        assert_eq!(
            family2_for("MakerNotes", "Olympus", "Model", "Audio"),
            Some("Camera")
        );
    }

    #[test]
    fn canon_dr4_group_is_image() {
        // Every CanonVRD::DR4 table is Image; keep the reader's category.
        assert_eq!(
            family2_for("Trailer", "CanonDR4", "PictureStyle", "Image"),
            None
        );
    }
}
