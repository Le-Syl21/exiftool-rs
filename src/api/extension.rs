//! Extensions: Rust code the facade calls for what Image::ExifTool itself
//! does not do (see `docs/port/API.md`).
//!
//! ExifTool's own ways of being extended work in the port unchanged, since
//! the port runs Perl: a config file (`.ExifTool_config`, `-config`) with
//! UserDefined tags, a custom module it loads. A [`FormatExtension`] is for
//! what is better done in Rust, a whole format engine: it writes a file type
//! ExifTool can only read. `write_info` calls one only for a source type
//! ExifTool cannot write (`CanWrite`), so nothing changes for a type ExifTool
//! writes; the result is then read back by ExifTool, as any write is, to
//! count the requests that took effect.
//!
//! The built-in extensions (feature `extra-writers`, on by default) are the
//! root crate's writers for ID3 (MP3, AIFF), Vorbis comments (FLAC, Ogg,
//! Opus) and Matroska (MKV, WebM), in [`extra_writers`](super::extra_writers).

use super::error::Result;
use super::exiftool::NewValue;

/// A writer for file types ExifTool reads but does not write.
///
/// ```no_run
/// use exiftool_port::extension::FormatExtension;
/// use exiftool_port::exiftool::NewValue;
///
/// /// Text files: a request for `Text:Title` sets the first line.
/// struct TitleLine;
///
/// impl FormatExtension for TitleLine {
///     fn name(&self) -> &str {
///         "title line"
///     }
///     fn file_types(&self) -> &[&str] {
///         &["TXT"]
///     }
///     fn write(&self, input: &[u8], _file_type: &str, edits: &[NewValue]) -> exiftool_port::Result<Vec<u8>> {
///         let Some(title) = edits.iter().rev().find(|e| e.tag == "Title").and_then(|e| e.value.clone()) else {
///             return Ok(input.to_vec());
///         };
///         let rest = input.splitn(2, |&b| b == b'\n').nth(1).unwrap_or(&[]);
///         Ok([title.as_bytes(), b"\n", rest].concat())
///     }
/// }
///
/// let mut et = exiftool_port::ExifTool::new();
/// et.register_extension(TitleLine);
/// ```
pub trait FormatExtension: Send + Sync {
    /// A short name, for messages.
    fn name(&self) -> &str;

    /// The file types it writes, as ExifTool's `FileType` names them
    /// (`MP3`, `FLAC`, `OGG`, `MKV`...). A destination file name is accepted
    /// when ExifTool finds no type for it, or one of these
    /// (`GetFileType`: `.opus` is `OGG`).
    fn file_types(&self) -> &[&str];

    /// The whole new file: `input` (a file of type `file_type`, one of
    /// [`file_types`](Self::file_types)) with `edits` applied, in order.
    ///
    /// The edits are the requests queued on the `ExifTool`, a copy
    /// (`set_new_values_from_file`) given as one edit per new value it
    /// queues, as ExifTool's `SetNewValuesFromFile` queues it: the tag name
    /// and value it copies, and the destination group it gives, which is
    /// none for a tag copied without one (the whole file, a tag list of
    /// `Title` or `-all`): such an edit is for the destination's preferred
    /// group for that name, whatever group the source had (a FLAC's Vorbis
    /// Title copied to an MP3 is an edit of `Title`, for its ID3 Title;
    /// these are the edits [`copy_edits`](Self::copy_edits) gives). A
    /// group the tag list names is kept (`Vorbis:Title`, `Title>ID3:Title`,
    /// `all:all`). An edit the extension cannot apply
    /// (another group, a tag it does not write) is left out: it is not
    /// counted, as ExifTool does not count what it cannot place. An error
    /// fails the write, and nothing is written.
    fn write(&self, input: &[u8], file_type: &str, edits: &[NewValue]) -> Result<Vec<u8>>;

    /// The edits that stand for the tags a copy (`set_new_values_from_file`)
    /// queues without a destination group, for a file of type `file_type`:
    /// by default, one edit of each tag's name, without a group, as
    /// ExifTool's `SetNewValuesFromFile` queues it. An extension may also
    /// write the source's tags its format names differently (the built-in
    /// ones write an ID3 `Year` copied to a FLAC as its Vorbis `DATE`: see
    /// `docs/port/API.md`). Only for these tags: a tag copied to a group the
    /// tag list names, and every `set_new_value` request, are given to
    /// [`write`](Self::write) as they are.
    fn copy_edits(&self, file_type: &str, copied: &[CopiedTag]) -> Vec<NewValue> {
        let _ = file_type;
        copied.iter().map(|t| NewValue { tag: t.tag.clone(), group: None, value: Some(t.value.clone()) }).collect()
    }
}

/// A tag a copy queues without a destination group, as the source has it
/// (see [`FormatExtension::copy_edits`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopiedTag {
    /// ExifTool's name of the tag (`Year`, `TrackNumber`).
    pub tag: String,
    /// The value ExifTool copies (the printed value, the ValueConv value
    /// for a `TAG#`; a list joined with ", ").
    pub value: String,
    /// The tag's family 0 group in the source (`ID3`, `Vorbis`,
    /// `Matroska`, `QuickTime`), none for an expression or an assignment.
    pub family0: Option<String>,
    /// Its family 1 group (`ID3v2_3`, `Vorbis`, `Info`, `ItemList`).
    pub family1: Option<String>,
}

/// Does `ext` handle files of `file_type` (case aside)?
pub(crate) fn handles(ext: &dyn FormatExtension, file_type: &str) -> bool {
    ext.file_types().iter().any(|t| t.eq_ignore_ascii_case(file_type))
}

/// The extensions an `ExifTool` starts with: the built-in writers when the
/// `extra-writers` feature is on, none otherwise.
pub(crate) fn builtin() -> Vec<std::sync::Arc<dyn FormatExtension>> {
    #[cfg(feature = "extra-writers")]
    {
        super::extra_writers::builtin()
    }
    #[cfg(not(feature = "extra-writers"))]
    {
        Vec::new()
    }
}
