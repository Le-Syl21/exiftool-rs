//! The built-in [`FormatExtension`]s (feature `extra-writers`): writers for
//! the formats Perl ExifTool reads but refuses to write ("Can't write MP3
//! files"), the root crate's (src/writer/), adapted:
//!
//! - [`Id3Writer`]: ID3v2 and ID3v1 in MP3 files, the ID3 chunk of AIFF
//!   files (requests of the `ID3`, `ID3v1` and `ID3v2*` groups).
//! - [`VorbisWriter`]: Vorbis comments in FLAC, Ogg Vorbis, Ogg FLAC and
//!   Opus files (the `Vorbis` group).
//! - [`MatroskaWriter`]: the segment Title and the SimpleTags of MKV and
//!   WebM files (the `Matroska` group, and `Info` for the Title).
//!
//! A request without a group goes to the writer of the file's type, for a
//! tag of that format, as ExifTool writes a tag named without a group to
//! the preferred group that has it: a tag of ID3.pm (`Title`, `Year`,
//! `Lyrics`...) for the ID3 writer, of the Vorbis.pm comments table
//! (`Title`, `Date`, `TrackNumber`...) for the Vorbis one, the segment
//! Title or a Matroska.pm standardized tag (`Artist`, `DateReleased`...)
//! for the Matroska one; any other name is left out. That is how a copy
//! from another format (`set_new_values_from_file`) gets here: a tag
//! copied without a destination group comes without one, so a FLAC's
//! `Vorbis:Title` copied to an MP3 is written as its ID3 Title, and
//! nothing becomes a Vorbis comment or a SimpleTag that the format does
//! not have. A request for another group (`EXIF:Artist` in an MP3, a copy
//! to `Vorbis:Title`) is left out. Each writer keeps everything it is not
//! asked to change (see its module).
//!
//! In a copy, a tag copied without a destination group is also written as
//! the destination's tag of the same meaning when the formats name it
//! differently (ID3 `Year`, Vorbis `DATE`, Matroska `DATE_RELEASED`,
//! QuickTime `ContentCreateDate`...), its value converted: see
//! [`FormatExtension::copy_edits`] and the `equivalents` module.

use std::sync::Arc;

use super::error::Result;
use super::exiftool::NewValue;
use super::extension::{CopiedTag, FormatExtension};

mod equivalents;
pub mod flac;
pub mod id3;
pub mod matroska;
pub mod ogg;

/// The built-in writers, as `ExifTool::new` registers them.
pub(crate) fn builtin() -> Vec<Arc<dyn FormatExtension>> {
    vec![Arc::new(Id3Writer), Arc::new(VorbisWriter), Arc::new(MatroskaWriter)]
}

/// `*` or `all`: every tag of a group.
fn is_all(tag: &str) -> bool {
    tag == "*" || tag.eq_ignore_ascii_case("all")
}

/// The group of an edit, lower case (empty for none).
fn group(nv: &NewValue) -> String {
    nv.group.as_deref().unwrap_or("").to_ascii_lowercase()
}

/// ID3 in MP3 and AIFF files (see [`id3`]).
pub struct Id3Writer;

impl FormatExtension for Id3Writer {
    fn name(&self) -> &str {
        "ID3"
    }

    fn file_types(&self) -> &[&str] {
        &["MP3", "AIFF"]
    }

    fn copy_edits(&self, _file_type: &str, copied: &[CopiedTag]) -> Vec<NewValue> {
        equivalents::edits_for_copy(equivalents::Format::Id3, copied)
    }

    fn write(&self, input: &[u8], file_type: &str, edits: &[NewValue]) -> Result<Vec<u8>> {
        use id3::{Id3Changes, Id3Scope};
        let mut changes = Id3Changes::default();
        for nv in edits {
            let Some(scope) = Id3Scope::from_group(nv.group.as_deref().unwrap_or("")) else {
                continue;
            };
            match &nv.value {
                None if is_all(&nv.tag) => changes.delete_all.push(scope),
                None => changes.delete.push((scope, nv.tag.to_ascii_lowercase())),
                Some(_) if is_all(&nv.tag) => {}
                Some(v) => changes.set.push((scope, nv.tag.clone(), v.clone())),
            }
        }
        if file_type.eq_ignore_ascii_case("AIFF") {
            id3::write_aiff(input, &changes)
        } else {
            id3::write_mp3(input, &changes)
        }
    }
}

/// Vorbis comments in FLAC, Ogg and Opus files (see [`flac`] and [`ogg`]).
pub struct VorbisWriter;

impl FormatExtension for VorbisWriter {
    fn name(&self) -> &str {
        "Vorbis"
    }

    fn file_types(&self) -> &[&str] {
        &["FLAC", "OGG", "OPUS"]
    }

    fn copy_edits(&self, _file_type: &str, copied: &[CopiedTag]) -> Vec<NewValue> {
        equivalents::edits_for_copy(equivalents::Format::Vorbis, copied)
    }

    fn write(&self, input: &[u8], file_type: &str, edits: &[NewValue]) -> Result<Vec<u8>> {
        let mut changes = flac::VorbisChanges::default();
        for nv in edits {
            let g = group(nv);
            if !matches!(g.as_str(), "" | "*" | "all" | "vorbis") {
                continue;
            }
            // (without a group, only the tags of ExifTool's table)
            if g != "vorbis" && !is_all(&nv.tag) && !flac::is_comment_tag(&nv.tag) {
                continue;
            }
            match &nv.value {
                None if is_all(&nv.tag) => changes.delete_all = true,
                None => changes.delete.push(nv.tag.to_ascii_lowercase()),
                Some(_) if is_all(&nv.tag) => {}
                Some(v) => changes.set.push((nv.tag.clone(), v.clone())),
            }
        }
        if file_type.eq_ignore_ascii_case("FLAC") {
            flac::write_flac(input, &changes)
        } else {
            ogg::write_ogg(input, &changes)
        }
    }
}

/// The Title and SimpleTags of MKV and WebM files (see [`matroska`]).
pub struct MatroskaWriter;

impl FormatExtension for MatroskaWriter {
    fn name(&self) -> &str {
        "Matroska"
    }

    fn file_types(&self) -> &[&str] {
        &["MKV", "WEBM"]
    }

    fn copy_edits(&self, _file_type: &str, copied: &[CopiedTag]) -> Vec<NewValue> {
        equivalents::edits_for_copy(equivalents::Format::Matroska, copied)
    }

    fn write(&self, input: &[u8], _file_type: &str, edits: &[NewValue]) -> Result<Vec<u8>> {
        let mut changes = matroska::MkvChanges::default();
        for nv in edits {
            let g = group(nv);
            // (the Title is in the Info group, as ExifTool reads it)
            let title = nv.tag.eq_ignore_ascii_case("Title");
            if !(matches!(g.as_str(), "" | "*" | "all" | "matroska") || (g == "info" && title)) {
                continue;
            }
            // (without a group, only the Title and the standardized tags)
            if matches!(g.as_str(), "" | "*" | "all") && !is_all(&nv.tag) && !matroska::is_std_tag(&nv.tag) {
                continue;
            }
            match &nv.value {
                None if is_all(&nv.tag) => changes.delete_all = true,
                None => changes.delete.push(nv.tag.to_ascii_lowercase()),
                Some(_) if is_all(&nv.tag) => {}
                Some(v) => changes.set.push((nv.tag.clone(), v.clone())),
            }
        }
        matroska::write_matroska(input, &changes)
    }
}
