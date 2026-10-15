//! TIFF file metadata writer.
//!
//! Edits TIFF files and TIFF-based RAW files (DNG, CR2, NEF, ARW, ORF, PEF)
//! in place. The whole file is the EXIF block, so the in-place IFD editor of
//! [`exif_edit`] does the work: an edited directory is rewritten where it was
//! or moved to the end of the file when it grows, a new or longer value is
//! appended, and everything else -- image strips and tiles, SubIFDs, maker
//! notes and their private offsets -- keeps its offset.
//!
//! What may be deleted follows ExifTool's `WriteDirectory` (Writer.pl):
//! IFD0 is never deleted from a TIFF, `EXIF:all` only drops the ExifIFD,
//! and a RAW file keeps its IFD0, ExifIFD and maker notes whatever is asked.

use crate::error::{Error, Result};
use crate::file_type::FileType;
use crate::writer::exif_edit::{self, ExifChanges, ExifOutcome, Ifd};

/// ExifTool's `%rawType` (Writer.pl), restricted to the types written here.
pub fn is_raw_type(file_type: FileType) -> bool {
    matches!(
        file_type,
        FileType::Cr2 | FileType::Nef | FileType::Arw | FileType::Orf | FileType::Pef
    )
}

/// Restrict an EXIF request to what ExifTool applies to a TIFF-based file.
///
/// - `EXIF:all` drops the ExifIFD only (GPS and the image directories stay),
///   and nothing at all from a RAW file;
/// - `IFD0:all` does nothing;
/// - a RAW file keeps its ExifIFD and its maker notes;
/// - only tags this crate knows how to write may be deleted by name, so a
///   request can never remove a tag that describes the image data.
pub fn tiff_changes(changes: &ExifChanges, file_type: FileType) -> ExifChanges {
    let raw = is_raw_type(file_type);
    let mut out = changes.clone();
    out.delete_all = false;
    out.delete_dirs.retain(|d| *d != Ifd::Ifd0);
    if changes.delete_all && !out.delete_dirs.contains(&Ifd::ExifIfd) {
        out.delete_dirs.push(Ifd::ExifIfd);
    }
    if raw {
        out.delete_dirs.retain(|d| *d != Ifd::ExifIfd);
        out.delete_maker_notes = false;
    }
    out.delete_tags
        .retain(|(_, name)| exif_edit::tag_spec(name).is_some());
    out
}

/// Apply `changes` to the TIFF-based file `source`. Returns the input
/// unchanged when the request does not change anything.
pub fn write_tiff(source: &[u8], changes: &ExifChanges, file_type: FileType) -> Result<Vec<u8>> {
    if !(source.starts_with(b"II\x2a\0") || source.starts_with(b"MM\0\x2a")) {
        return Err(Error::InvalidData("not a TIFF file".into()));
    }
    let changes = tiff_changes(changes, file_type);
    match exif_edit::edit_exif(Some(source), &changes)? {
        ExifOutcome::Unchanged => Ok(source.to_vec()),
        ExifOutcome::Rewritten(t) => Ok(t),
        // Only an IFD0 left with nothing in it can go, which a TIFF image
        // never is; refuse rather than write a file without an image.
        ExifOutcome::Removed => Err(Error::InvalidData(
            "the edit would remove IFD0 from a TIFF file".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer::exif_edit::SetValue;

    /// A little-endian TIFF: one 2x1 strip, IFD0 with the image tags and
    /// Model, and an ExifIFD holding DateTimeOriginal.
    fn sample() -> Vec<u8> {
        let mut t = b"II\x2a\0\x08\0\0\0".to_vec();
        // IFD0 at 8: 8 entries.
        let entries: [(u16, u16, u32, u32); 8] = [
            (0x0100, 3, 1, 2),   // ImageWidth
            (0x0101, 3, 1, 1),   // ImageLength
            (0x0103, 3, 1, 1),   // Compression
            (0x0106, 3, 1, 1),   // PhotometricInterpretation
            (0x0110, 2, 6, 200), // Model -> "Model\0" at 200
            (0x0111, 4, 1, 220), // StripOffsets
            (0x0117, 4, 1, 2),   // StripByteCounts
            (0x8769, 4, 1, 120), // ExifIFD
        ];
        t.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for (tag, typ, n, v) in entries {
            t.extend_from_slice(&tag.to_le_bytes());
            t.extend_from_slice(&typ.to_le_bytes());
            t.extend_from_slice(&n.to_le_bytes());
            t.extend_from_slice(&v.to_le_bytes());
        }
        t.extend_from_slice(&0u32.to_le_bytes());
        t.resize(120, 0);
        // ExifIFD at 120: DateTimeOriginal.
        t.extend_from_slice(&1u16.to_le_bytes());
        t.extend_from_slice(&0x9003u16.to_le_bytes());
        t.extend_from_slice(&2u16.to_le_bytes());
        t.extend_from_slice(&20u32.to_le_bytes());
        t.extend_from_slice(&160u32.to_le_bytes());
        t.extend_from_slice(&0u32.to_le_bytes());
        t.resize(160, 0);
        t.extend_from_slice(b"2020:01:02 03:04:05\0");
        t.resize(200, 0);
        t.extend_from_slice(b"Model\0");
        t.resize(220, 0);
        t.extend_from_slice(&[0xAB, 0xCD]);
        t
    }

    fn names(data: &[u8]) -> Vec<(String, String)> {
        let tags = crate::metadata::ExifReader::read(data).unwrap();
        tags.into_iter()
            .map(|t| (t.group.family1.clone(), t.name.clone()))
            .collect()
    }

    fn set(name: &str, v: &str) -> SetValue {
        let spec = exif_edit::tag_spec(name).unwrap();
        SetValue {
            ifd: spec.ifd,
            tag: spec.tag,
            format: spec.format,
            big_endian: exif_edit::encode_value(
                &spec,
                v,
                crate::metadata::exif::ByteOrderMark::BigEndian,
            )
            .unwrap(),
            little_endian: exif_edit::encode_value(
                &spec,
                v,
                crate::metadata::exif::ByteOrderMark::LittleEndian,
            )
            .unwrap(),
        }
    }

    #[test]
    fn adds_and_deletes_tags_keeping_the_strip() {
        let src = sample();
        let changes = ExifChanges {
            set: vec![set("Artist", "Added by test")],
            delete_tags: vec![(None, "model".into())],
            ..Default::default()
        };
        let out = write_tiff(&src, &changes, FileType::Tiff).unwrap();
        // The strip did not move and is intact.
        assert_eq!(&out[220..222], &[0xAB, 0xCD]);
        let n = names(&out);
        assert!(n.contains(&("IFD0".into(), "Artist".into())));
        assert!(!n.iter().any(|(_, t)| t == "Model"));
        assert!(n.contains(&("ExifIFD".into(), "DateTimeOriginal".into())));
    }

    #[test]
    fn exif_all_drops_only_the_exif_ifd() {
        // ExifTool 13.59 on a DNG: `-EXIF:all=` removes the ExifIFD and
        // leaves IFD0 (Model included) alone.
        let src = sample();
        let changes = ExifChanges {
            delete_all: true,
            ..Default::default()
        };
        let out = write_tiff(&src, &changes, FileType::Dng).unwrap();
        let n = names(&out);
        assert!(n.contains(&("IFD0".into(), "Model".into())));
        assert!(!n.iter().any(|(g, _)| g == "ExifIFD"));
        assert_eq!(&out[220..222], &[0xAB, 0xCD]);
    }

    #[test]
    fn raw_keeps_ifd0_and_exif_ifd() {
        // "Can't delete IFD0/ExifIFD from CR2": nothing changes.
        let src = sample();
        let changes = ExifChanges {
            delete_all: true,
            ..Default::default()
        };
        let out = write_tiff(&src, &changes, FileType::Cr2).unwrap();
        assert_eq!(out, src);
    }

    #[test]
    fn image_structure_tags_cannot_be_deleted() {
        let src = sample();
        let changes = ExifChanges {
            delete_tags: vec![(None, "stripoffsets".into())],
            ..Default::default()
        };
        assert_eq!(write_tiff(&src, &changes, FileType::Tiff).unwrap(), src);
    }
}
