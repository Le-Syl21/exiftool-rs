//! The root crate's `ExifTool` API (`exiftool_rs::ExifTool`), over
//! Image::ExifTool itself: each method calls the ExifTool method a Perl
//! program would (see `docs/port/API.md`).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::interp::Want;
use crate::perl::Value as PV;

use super::engine::{self, call, pv_array, pv_path, pv_str, Engine};
use super::error::{Error, Result};
use super::file_type::{self, FileType};
use super::tag::Tag;

/// Processing options for metadata extraction.
#[derive(Debug, Clone)]
pub struct Options {
    /// Include duplicate tags (different groups may have same tag name).
    /// ExifTool's `Duplicates` option.
    pub duplicates: bool,
    /// Apply print conversions (human-readable values) in
    /// [`ExifTool::image_info`]. A [`Tag`] always has both its
    /// `print_value` and its `raw_value`.
    pub print_conv: bool,
    /// Fast scan level (ExifTool's `FastScan` option).
    pub fast_scan: u8,
    /// Only extract these tags (empty = all): the tag list of ExifTool's
    /// `ImageInfo`, so `Group:Tag` and `Group:*` work as there.
    pub requested_tags: Vec<String>,
    /// Extract embedded documents/data (ExifTool's `ExtractEmbedded`; like
    /// the exiftool application's `-ee`, it turns `Duplicates` on).
    pub extract_embedded: u8,
    /// Show unknown tags: 0=off, 1=-u, 2=-U (ExifTool's `Unknown` option).
    pub show_unknown: u8,
    /// Process compressed data in files (ExifTool's `Compress`, `-z`).
    pub process_compressed: bool,
    /// Use the MWG (Metadata Working Group) composite tags for reading and
    /// writing (Image::ExifTool::MWG, `-use MWG`).
    pub use_mwg: bool,
    /// Reverse-geocode `Geolocation*` tags from GPS coordinates
    /// (ExifTool's `Geolocation` API option). Off by default, like ExifTool.
    pub geolocation: bool,
    /// Output structured XMP (ExifTool's `Struct` option, `-struct`): a
    /// struct-valued property becomes one tag holding a [`Value::Struct`]
    /// (or a list of them) instead of its flattened fields.
    ///
    /// [`Value::Struct`]: super::value::Value::Struct
    pub struct_output: bool,
    /// Language of tag descriptions and printed values (ExifTool's `Lang`
    /// option, `-lang`): `None` or `"en"` for English, one of ExifTool's
    /// languages (`fr`, `de`, `ja`, `zh_cn`...), or one exiftool-rs adds
    /// (`ar`, `bn`, `hi`, `pt`). `zh` is `zh_cn`, and `zh-CN` is `zh_cn`, as
    /// `-lang` takes it. A language ExifTool does not have is ignored, as
    /// ExifTool ignores it. Writing takes values in this language too.
    pub lang: Option<String>,
    /// Run each call on an interpreter of its own (with a new
    /// Image::ExifTool object), as separate runs of the exiftool
    /// application, so that no call depends on the ones before it. Off by
    /// default: an `ExifTool` keeps one interpreter and one Image::ExifTool
    /// object for all its calls, as a Perl program reusing one
    /// `Image::ExifTool->new` object for file after file, with the state
    /// ExifTool carries from one file to the next in Perl (see
    /// `docs/port/API.md`), and without starting an interpreter (about
    /// 16 ms) for each call.
    pub fresh_interpreter: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            duplicates: false,
            print_conv: true,
            fast_scan: 0,
            requested_tags: Vec::new(),
            extract_embedded: 0,
            show_unknown: 0,
            process_compressed: false,
            use_mwg: false,
            geolocation: false,
            struct_output: false,
            lang: None,
            fresh_interpreter: false,
        }
    }
}

impl Options {
    /// The ExifTool options for reading.
    fn read_options(&self) -> Vec<PV> {
        let mut o: Vec<(&str, PV)> = vec![
            ("PrintConv", PV::Int(1)),
            ("Duplicates", PV::Int((self.duplicates || self.extract_embedded > 0) as i64)),
            ("Struct", PV::Int(self.struct_output as i64)),
            // the TIFF format of each value (family 6 group), to type numbers
            ("SaveFormat", PV::Int(1)),
        ];
        if self.fast_scan > 0 {
            o.push(("FastScan", PV::Int(self.fast_scan as i64)));
        }
        if self.extract_embedded > 0 {
            o.push(("ExtractEmbedded", PV::Int(self.extract_embedded as i64)));
        }
        if self.show_unknown > 0 {
            o.push(("Unknown", PV::Int(self.show_unknown as i64)));
        }
        if self.process_compressed {
            o.push(("Compress", PV::Int(1)));
        }
        if self.geolocation {
            o.push(("Geolocation", PV::Int(1)));
        }
        if let Some(lang) = self.lang_code() {
            o.push(("Lang", pv_str(&lang)));
        }
        flatten(o)
    }

    /// The ExifTool options for writing: values are given in their printed
    /// form, as the root crate takes them (a number is still accepted where
    /// ExifTool's inverse conversion takes one).
    fn write_options(&self) -> Vec<PV> {
        let mut o: Vec<(&str, PV)> = vec![("PrintConv", PV::Int(1))];
        if self.process_compressed {
            o.push(("Compress", PV::Int(1)));
        }
        // (ExifTool takes a translated PrintConv value back when its
        // language is set)
        if let Some(lang) = self.lang_code() {
            o.push(("Lang", pv_str(&lang)));
        }
        flatten(o)
    }

    /// `lang` as ExifTool's `Lang` option takes it: lower case with
    /// underlines, as the exiftool application makes it (`zh-CN` is
    /// `zh_cn`), and `zh` for `zh_cn`. `None` for English.
    fn lang_code(&self) -> Option<String> {
        let lang = self.lang.as_deref()?.trim().replace('-', "_").to_ascii_lowercase();
        match lang.as_str() {
            "" | "en" => None,
            "zh" => Some("zh_cn".to_string()),
            _ => Some(lang),
        }
    }
}

fn flatten(o: Vec<(&str, PV)>) -> Vec<PV> {
    o.into_iter().flat_map(|(k, v)| [pv_str(k), v]).collect()
}

/// A queued tag change for writing.
#[derive(Debug, Clone)]
pub struct NewValue {
    /// Tag name (e.g., "Artist", "Copyright", "XMP:Title")
    pub tag: String,
    /// Group prefix if specified (e.g., "EXIF", "XMP", "IPTC")
    pub group: Option<String>,
    /// New value (None = delete tag)
    pub value: Option<String>,
}

impl NewValue {
    /// `Group:Tag`, as ExifTool's SetNewValue takes it.
    fn name(&self) -> String {
        match &self.group {
            Some(g) => format!("{g}:{}", self.tag),
            None => self.tag.clone(),
        }
    }

    /// `-Group:Tag=value`, as the request would be written on a command line.
    fn describe(&self) -> String {
        let name = self.name();
        match &self.value {
            Some(v) => format!("{name}={v}"),
            None => format!("{name}= (delete)"),
        }
    }
}

/// One queued request, replayed on ExifTool in order by `write_info`.
#[derive(Debug, Clone)]
enum Request {
    /// `SetNewValue`
    Set(NewValue),
    /// `SetNewValuesFromFile`
    Copy { path: PathBuf, tags: Vec<String> },
}

/// The main ExifTool engine — read, write, and edit metadata.
///
/// # Reading metadata
/// ```no_run
/// use exiftool_port::ExifTool;
///
/// let et = ExifTool::new();
///
/// // Full tag structs
/// let tags = et.extract_info("photo.jpg").unwrap();
/// for tag in &tags {
///     println!("[{}] {}: {}", tag.group.family0, tag.name, tag.print_value);
/// }
///
/// // Simple name→value map
/// let info = et.image_info("photo.jpg").unwrap();
/// println!("Camera: {}", info.get("Model").unwrap_or(&String::new()));
/// ```
///
/// # Writing metadata
/// ```no_run
/// use exiftool_port::ExifTool;
///
/// let mut et = ExifTool::new();
/// et.set_new_value("Artist", Some("John Doe"));
/// et.set_new_value("Copyright", Some("2024"));
/// et.write_info("input.jpg", "output.jpg").unwrap();
/// ```
pub struct ExifTool {
    options: Options,
    requests: Vec<Request>,
    /// What the last `write_info` call was warned about (see [`ExifTool::write_warnings`]).
    write_warnings: std::sync::Mutex<Vec<String>>,
    /// The interpreter running Image::ExifTool for this object's calls.
    engine: Engine,
}

/// What a read is of: a file, or data in memory.
enum Source {
    Path(PathBuf),
    Data(Vec<u8>),
}

/// Result of metadata extraction: maps tag names to display values.
pub type ImageInfo = HashMap<String, String>;

/// A tag read by ExifTool: its tag key (`Name`, `Name (1)`...), the tag, its
/// write group, and whether ExifTool can write it.
struct ReadTag {
    key: String,
    tag: Tag,
    write_group: Option<String>,
    writable: bool,
}

impl ExifTool {
    /// Create a new ExifTool instance with default options.
    pub fn new() -> Self {
        Self::with_options(Options::default())
    }

    /// Create a new ExifTool instance with custom options.
    pub fn with_options(options: Options) -> Self {
        Self {
            options,
            requests: Vec::new(),
            write_warnings: std::sync::Mutex::new(Vec::new()),
            engine: Engine::default(),
        }
    }

    /// Get a mutable reference to the options.
    pub fn options_mut(&mut self) -> &mut Options {
        &mut self.options
    }

    /// Get a reference to the options.
    pub fn options(&self) -> &Options {
        &self.options
    }

    // ================================================================
    // Writing API
    // ================================================================

    /// Queue a new tag value for writing: ExifTool's `SetNewValue`, as the
    /// exiftool application calls it for `-TAG=VALUE` (so several values
    /// for a list tag add up, and a protected tag named explicitly is
    /// written).
    ///
    /// # Arguments
    /// * `tag` - Tag name, optionally prefixed with group (e.g., "Artist", "XMP:Title", "EXIF:Copyright")
    /// * `value` - New value, or None to delete the tag
    pub fn set_new_value(&mut self, tag: &str, value: Option<&str>) {
        let (group, tag_name) = match tag.find(':') {
            Some(colon_pos) => (Some(tag[..colon_pos].to_string()), tag[colon_pos + 1..].to_string()),
            None => (None, tag.to_string()),
        };
        self.requests.push(Request::Set(NewValue {
            tag: tag_name,
            group,
            value: value.map(|v| v.to_string()),
        }));
    }

    /// Clear all queued new values.
    pub fn clear_new_values(&mut self) {
        self.requests.clear();
    }

    /// Copy tags from a source file, queuing them as new values: ExifTool's
    /// `SetNewValuesFromFile` (which reads the source with duplicates).
    /// Optionally filter by tag names (its tag list).
    ///
    /// Returns the number of tags it sets, as the exiftool application
    /// counts them for `-tagsFromFile`. A request queued after this one
    /// overrides it, as with ExifTool (a deletion queued after a copy wins).
    pub fn set_new_values_from_file<P: AsRef<Path>>(&mut self, src_path: P, tags_to_copy: Option<&[&str]>) -> Result<u32> {
        let path = src_path.as_ref().to_path_buf();
        fs::File::open(&path).map_err(Error::Io)?;
        let tags: Vec<String> = tags_to_copy.unwrap_or(&[]).iter().map(|t| t.to_string()).collect();
        let options = self.options.clone();
        let (p, t) = (path.clone(), tags.clone());
        let (count, error) = self.run(move |it| -> Result<(u32, Option<String>)> {
            let args = vec![pv_path(&p), pv_array(t.iter().map(|t| pv_str(t)).collect()), pv_array(options.write_options())];
            let r = call(it, "CountCopy", args, Want::Scalar)?;
            let f = engine::array(r.first().unwrap_or(&PV::Undef));
            let n = f.first().map(PV::to_i64).unwrap_or(0).max(0) as u32;
            Ok((n, f.get(1).and_then(engine::opt_text)))
        })?;
        if let Some(e) = error {
            return Err(Error::InvalidData(e));
        }
        self.requests.push(Request::Copy { path, tags });
        Ok(count)
    }

    /// Set a file's name based on a tag value.
    pub fn set_file_name_from_tag<P: AsRef<Path>>(&self, path: P, tag_name: &str, template: &str) -> Result<String> {
        let path = path.as_ref();
        let tags = self.extract_info(path)?;

        let tag_value = tags
            .iter()
            .find(|t| t.name.to_lowercase() == tag_name.to_lowercase())
            .map(|t| &t.print_value)
            .ok_or_else(|| Error::TagNotFound(tag_name.to_string()))?;

        // Template: "prefix%vsuffix.ext", or the tag value with the file's extension
        let new_name = if template.contains('%') {
            template.replace("%v", value_to_filename(tag_value).as_str())
        } else {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let clean = value_to_filename(tag_value);
            if ext.is_empty() {
                clean
            } else {
                format!("{}.{}", clean, ext)
            }
        };

        let parent = path.parent().unwrap_or(Path::new(""));
        let new_path = parent.join(&new_name);

        fs::rename(path, &new_path).map_err(Error::Io)?;
        Ok(new_path.to_string_lossy().to_string())
    }

    /// Write queued changes to a file: ExifTool's `WriteInfo`, with the
    /// requests replayed in the order they were queued.
    ///
    /// If `dst_path` is the same as `src_path`, the file is modified in place.
    /// Either way the output goes to a new temporary file next to `dst_path`
    /// (with the permissions of the file it replaces), renamed over it once
    /// complete, and removed if anything fails.
    ///
    /// Returns the number of queued requests that changed the file, checked
    /// by reading the result back: a written tag must be there with its new
    /// value, a deleted tag or group must be gone. Deleting a tag the file
    /// does not have changes nothing, so it is not counted, but it is not an
    /// error either: like ExifTool's "1 image files unchanged", the call
    /// returns `Ok(0)` and still writes `dst_path`.
    ///
    /// When none of the queued requests takes effect, a distinct `dst_path`
    /// gets ExifTool's unchanged copy and the call returns `Ok(0)`
    /// (ExifTool's "1 image files copied"); in place, the call fails and
    /// leaves the file alone (ExifTool's "Nothing to do."). ExifTool's
    /// warnings ("Tag 'X' is not defined"...) are kept for
    /// [`write_warnings`](Self::write_warnings).
    pub fn write_info<P: AsRef<Path>, Q: AsRef<Path>>(&self, src_path: P, dst_path: Q) -> Result<u32> {
        let src_path = src_path.as_ref();
        let dst_path = dst_path.as_ref();
        self.set_write_warnings(Vec::new());
        fs::File::open(src_path).map_err(Error::Io)?;

        let requests = self.requests.clone();
        let options = self.options.clone();
        let (src, dst) = (src_path.to_path_buf(), dst_path.to_path_buf());
        let (rtn, output, warnings, error) = self.run(move |it| -> Result<_> {
            let ops: Vec<PV> = requests
                .iter()
                .map(|r| match r {
                    Request::Set(nv) => pv_array(vec![pv_str("set"), pv_str(&nv.name()), nv.value.as_deref().map(pv_str).unwrap_or(PV::Undef)]),
                    Request::Copy { path, tags } => pv_array(vec![pv_str("copy"), pv_path(path), pv_array(tags.iter().map(|t| pv_str(t)).collect())]),
                })
                .collect();
            let r = call(it, "Write", vec![pv_path(&src), pv_path(&dst), pv_array(ops), pv_array(options.write_options())], Want::Scalar)?;
            let f = engine::array(r.first().unwrap_or(&PV::Undef));
            let rtn = f.first().map(PV::to_i64).unwrap_or(0);
            let output = match f.get(1) {
                Some(PV::ScalarRef(s)) => s.borrow().to_bytes().to_vec(),
                _ => Vec::new(),
            };
            let warnings: Vec<String> = f.get(2).map(engine::array).unwrap_or_default().iter().map(engine::text).collect();
            Ok((rtn, output, warnings, f.get(3).and_then(engine::opt_text)))
        })?;
        self.set_write_warnings(warnings);
        if rtn == 0 {
            let msg = error.unwrap_or_else(|| "ExifTool could not write the file".to_string());
            return Err(if msg.contains("not yet supported") || msg.contains("Can't write") || msg.contains("not supported") {
                Error::UnsupportedFileType(msg)
            } else {
                Error::InvalidData(msg)
            });
        }

        let temp = TempFile::create(dst_path, &output)?;
        let requests = self.counted_requests();
        let (applied, skipped) = self.count_applied(src_path, temp.path(), &requests);
        let nothing_done = !requests.is_empty() && skipped == requests.len();
        if nothing_done && same_file(src_path, dst_path) {
            let described: Vec<String> = requests.iter().map(NewValue::describe).collect();
            return Err(Error::UnsupportedFileType(format!("nothing to do: none of the requested changes could be written to this file ({})", described.join(", "))));
        }
        temp.persist(dst_path)?;
        Ok(if nothing_done { 0 } else { applied })
    }

    /// The warnings of the last [`write_info`](Self::write_info) call, as
    /// ExifTool gave them: those of `SetNewValue` ("Tag 'X' is not
    /// defined"...), of `SetNewValuesFromFile`, and of `WriteInfo`.
    pub fn write_warnings(&self) -> Vec<String> {
        self.write_warnings.lock().map(|w| w.clone()).unwrap_or_default()
    }

    fn set_write_warnings(&self, warnings: Vec<String>) {
        if let Ok(mut w) = self.write_warnings.lock() {
            *w = warnings;
        }
    }

    /// The requests as `write_info` counts them: a copy stands for each tag
    /// it sets, with the value the source has, unless a later request is for
    /// the same tag (or a whole group), which overrides it.
    fn counted_requests(&self) -> Vec<NewValue> {
        let mut out = Vec::new();
        for (i, r) in self.requests.iter().enumerate() {
            match r {
                Request::Set(nv) => out.push(nv.clone()),
                Request::Copy { path, tags } => {
                    let later = &self.requests[i + 1..];
                    let overridden = |name: &str| later.iter().any(|r| matches!(r, Request::Set(nv) if nv.tag.eq_ignore_ascii_case(name) || is_all(&nv.tag)));
                    // (with MWG as the write has it: MWG adds composite
                    // tags only, which are not copied)
                    let reader = Options {
                        duplicates: true,
                        requested_tags: tags.clone(),
                        lang: self.options.lang.clone(),
                        use_mwg: self.options.use_mwg,
                        fresh_interpreter: self.options.fresh_interpreter,
                        ..Options::default()
                    };
                    let Ok(src) = self.read_as(reader, path) else { continue };
                    for t in copied_tags(&src) {
                        if !overridden(&t.tag.name) {
                            out.push(NewValue {
                                tag: t.tag.name.clone(),
                                group: Some(t.tag.group.family1.clone()),
                                value: Some(t.tag.print_value.clone()),
                            });
                        }
                    }
                }
            }
        }
        out
    }

    /// How many of `requests` the rewrite of `before` into `after` carried
    /// out, judged by what ExifTool reads in each, and how many do not hold
    /// in `after` (a deletion of something already absent holds without
    /// being counted).
    fn count_applied(&self, before: &Path, after: &Path, requests: &[NewValue]) -> (u32, usize) {
        let keep = |t: &ReadTag| !matches!(t.tag.group.family0.as_str(), "System" | "Composite" | "ExifTool");
        let read = |p: &Path| -> Vec<ReadTag> {
            // (in the language of the requested values)
            let reader = Options {
                duplicates: true,
                use_mwg: self.options.use_mwg,
                lang: self.options.lang.clone(),
                fresh_interpreter: self.options.fresh_interpreter,
                ..Options::default()
            };
            self.read_as(reader, p).map(|v| v.into_iter().filter(keep).collect()).unwrap_or_default()
        };
        let after_tags = read(after);
        let before_tags = read(before);
        let value = |name: &str| before_tags.iter().find(|t| t.tag.name == name).map(|t| t.tag.print_value.as_str());
        let file_type = value("FileType").and_then(|code| file_type::from_exiftool(code, value("FileTypeExtension")));
        let holds = |nv: &NewValue, from: &[ReadTag], to: &[ReadTag]| match png_group_deleted(nv, file_type, to).or_else(|| tiff_exif_all_applied(nv, file_type, from, to)) {
            Some(done) => done,
            None => request_applied(nv, from, to),
        };
        let mut changed = 0;
        let mut skipped = 0;
        for nv in requests {
            if !holds(nv, &before_tags, &after_tags) {
                skipped += 1;
                continue;
            }
            // ExifTool reports a file "unchanged" when the only edit deletes
            // a tag it did not have; writing a value always counts, even the
            // value already there.
            let already_gone = nv.value.is_none() && png_group_deleted(nv, file_type, &before_tags).unwrap_or_else(|| request_applied(nv, &before_tags, &before_tags));
            if !already_gone {
                changed += 1;
            }
        }
        (changed, skipped)
    }

    /// Returns the set of tag names (lowercase) that are writable for a given
    /// file type: every tag ExifTool can write (`GetWritableTags`) when
    /// ExifTool can write this type (`CanWrite`), an empty set otherwise.
    /// Never `None`: ExifTool has no open-ended format.
    pub fn writable_tags(file_type: FileType) -> Option<std::collections::HashSet<&'static str>> {
        use std::collections::HashSet;
        use std::sync::OnceLock;
        static ALL: OnceLock<HashSet<&'static str>> = OnceLock::new();
        if file_type.support() == file_type::Support::Read {
            return Some(HashSet::new());
        }
        let all = ALL.get_or_init(|| engine::writable_tags().into_iter().map(|t| &*Box::leak(t.to_ascii_lowercase().into_boxed_str())).collect());
        Some(all.clone())
    }

    // ================================================================
    // Reading API
    // ================================================================

    /// Extract metadata from a file and return a simple name→value map.
    ///
    /// This is the high-level one-shot API, equivalent to ExifTool's
    /// `ImageInfo()`. With `duplicates`, a tag that is not the one ExifTool
    /// gives priority to is keyed `Name [Group0:Group1]`.
    pub fn image_info<P: AsRef<Path>>(&self, path: P) -> Result<ImageInfo> {
        let tags = self.read(path.as_ref())?;
        Ok(self.get_info(&tags))
    }

    /// Extract all metadata tags from a file: ExifTool's `ImageInfo`, in the
    /// order of its `GetTagList` (the order `exiftool` lists them).
    ///
    /// Returns the full `Tag` structs with groups, raw values, etc.
    pub fn extract_info<P: AsRef<Path>>(&self, path: P) -> Result<Vec<Tag>> {
        Ok(self.read(path.as_ref())?.into_iter().map(|t| t.tag).collect())
    }

    /// Extract metadata from in-memory data (ExifTool's `ImageInfo` on a
    /// reference to the data). ExifTool identifies the data by its content
    /// only; `path` is not read.
    pub fn extract_info_from_bytes(&self, data: &[u8], path: &Path) -> Result<Vec<Tag>> {
        let _ = path;
        Ok(self.read_source(self.options.clone(), Source::Data(data.to_vec()))?.into_iter().map(|t| t.tag).collect())
    }

    fn read(&self, path: &Path) -> Result<Vec<ReadTag>> {
        self.read_as(self.options.clone(), path)
    }

    /// Read a file with other options, on this object's interpreter.
    fn read_as(&self, options: Options, path: &Path) -> Result<Vec<ReadTag>> {
        fs::File::open(path).map_err(Error::Io)?;
        self.read_source(options, Source::Path(path.to_path_buf()))
    }

    /// Run `f` on this object's interpreter (see [`Options::fresh_interpreter`]).
    fn run<R: Send + 'static>(&self, f: impl FnOnce(&mut crate::interp::Interp) -> R + Send + 'static) -> R {
        self.engine.run(self.options.fresh_interpreter, self.options.use_mwg, f)
    }

    fn read_source(&self, options: Options, source: Source) -> Result<Vec<ReadTag>> {
        self.engine.run(options.fresh_interpreter, options.use_mwg, move |it| {
            let source = match &source {
                Source::Path(p) => pv_path(p),
                Source::Data(d) => PV::binary(d),
            };
            let tags: Vec<PV> = options.requested_tags.iter().map(|t| pv_str(t)).collect();
            let r = call(it, "Read", vec![source, pv_array(options.read_options()), pv_array(tags)], Want::Scalar)?;
            Ok(engine::array(r.first().unwrap_or(&PV::Undef))
                .iter()
                .map(|rec| {
                    let (key, tag, write_group, writable) = engine::record_to_tag(rec);
                    ReadTag { key, tag, write_group, writable }
                })
                .collect())
        })
    }

    /// The tags read, as a name→value map: ExifTool's tag keys, with a
    /// duplicate (`Name (1)`) keyed `Name [Group0:Group1]`.
    fn get_info(&self, tags: &[ReadTag]) -> ImageInfo {
        let mut info = ImageInfo::new();
        for t in tags {
            let value = if self.options.print_conv { t.tag.print_value.clone() } else { t.tag.raw_value.to_display_string() };
            let key = if t.key.contains(' ') { format!("{} [{}:{}]", t.tag.name, t.tag.group.family0, t.tag.group.family1) } else { t.tag.name.clone() };
            info.entry(key).or_insert(value);
        }
        info
    }
}

impl Default for ExifTool {
    fn default() -> Self {
        Self::new()
    }
}

/// The tags `SetNewValuesFromFile` copies from a source read with
/// duplicates: those of the source's metadata (not its file system,
/// composite or ExifTool tags), with a value.
fn copied_tags(src: &[ReadTag]) -> Vec<&ReadTag> {
    src.iter()
        .filter(|t| !matches!(t.tag.group.family0.as_str(), "File" | "System" | "Composite" | "ExifTool"))
        .filter(|t| !t.tag.print_value.is_empty() && !t.tag.print_value.starts_with("(Binary data"))
        .collect()
}

/// Detect the file type of a file: the `FileType` ExifTool finds for it
/// from its magic number and extension.
pub fn get_file_type<P: AsRef<Path>>(path: P) -> Result<FileType> {
    let path = path.as_ref();
    fs::File::open(path).map_err(Error::Io)?;
    match engine::type_of_file(path) {
        Some((code, ext)) => file_type::from_exiftool(&code, ext.as_deref()).ok_or(Error::UnsupportedFileType(code)),
        None => Err(Error::UnsupportedFileType("unknown".into())),
    }
}

/// `*` or `all`: every tag of a group.
fn is_all(tag: &str) -> bool {
    tag == "*" || tag.eq_ignore_ascii_case("all")
}

/// Does `tag` belong to `group` (any family, case-insensitive)? `None`, `*`
/// and `all` match every tag.
fn in_group(tag: &Tag, group: Option<&str>) -> bool {
    match group {
        None => true,
        Some(g) if is_all(g) => true,
        Some(g) => {
            let g = g.to_ascii_lowercase();
            [&tag.group.family0, &tag.group.family1, &tag.group.family2].iter().any(|f| f.to_ascii_lowercase() == g)
        }
    }
}

/// Values compared loosely: case, surrounding space and list spacing aside.
fn same_value(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.trim().split(',').map(|p| p.trim().to_ascii_lowercase()).collect::<Vec<_>>();
    norm(a) == norm(b)
}

fn is_tiff_based(file_type: Option<FileType>) -> bool {
    matches!(file_type, Some(FileType::Tiff | FileType::Dng | FileType::Cr2 | FileType::Nef | FileType::Arw | FileType::Orf | FileType::Pef))
}

/// `EXIF:all=` on a TIFF-based file: ExifTool never deletes IFD0 there, so
/// the request is done when an ExifIFD was there and is gone. `None` for
/// any other request.
fn tiff_exif_all_applied(nv: &NewValue, file_type: Option<FileType>, before: &[ReadTag], after: &[ReadTag]) -> Option<bool> {
    let exif_all = nv.value.is_none() && is_all(&nv.tag) && nv.group.as_deref().is_some_and(|g| g.eq_ignore_ascii_case("EXIF"));
    if !is_tiff_based(file_type) || !exif_all {
        return None;
    }
    let in_exif_ifd = |t: &&ReadTag| matches!(t.tag.group.family1.as_str(), "ExifIFD" | "InteropIFD");
    Some(before.iter().any(|t| in_exif_ifd(&t)) && !after.iter().any(|t| in_exif_ifd(&t)))
}

/// The writable PNG tags `PNG:all=` leaves: the name of the ICC profile,
/// which goes with the profile.
fn kept_by_png_all(name: &str) -> bool {
    matches!(name, "ProfileName" | "ICC_Profile")
}

/// Is a PNG group deletion done, judged from what `tags` still holds?
/// `PNG:all` reaches the tags ExifTool can write (the textual ones, the
/// tIME/gAMA/sRGB chunks), not those it only reads (the image header,
/// cHRM, sBIT, PLTE...) that the PNG group also names; `all` reaches every
/// group ExifTool deletes in a PNG. `None` for any other request.
fn png_group_deleted(nv: &NewValue, file_type: Option<FileType>, tags: &[ReadTag]) -> Option<bool> {
    if file_type != Some(FileType::Png) || nv.value.is_some() || !is_all(&nv.tag) {
        return None;
    }
    let png = |t: &ReadTag| t.tag.group.family1 == "PNG" && t.writable && !kept_by_png_all(&t.tag.name);
    let phys = |t: &ReadTag| t.tag.group.family1 == "PNG-pHYs";
    let icc = |t: &ReadTag| t.tag.group.family0 == "ICC_Profile";
    let g = nv.group.as_deref().unwrap_or("").to_ascii_lowercase();
    let hit: Box<dyn Fn(&ReadTag) -> bool> = match g.as_str() {
        "" | "*" | "all" => Box::new(|t: &ReadTag| png(t) || phys(t) || icc(t) || matches!(t.tag.group.family0.as_str(), "EXIF" | "XMP" | "IPTC" | "MakerNotes")),
        "png" => Box::new(png),
        "png-phys" => Box::new(phys),
        "icc_profile" => Box::new(icc),
        _ => return None,
    };
    Some(!tags.iter().any(hit))
}

/// Did `nv` take effect, comparing what ExifTool reads before and after?
///
/// - a deleted tag (or `Group:all`) must be absent afterwards, from where
///   the request reaches (an EXIF tag from its write group: `EXIF:Model=`
///   leaves `IFD1:Model`);
/// - a written tag must be present afterwards, either holding the requested
///   value or with a value it did not have before (a value ExifTool
///   formats differently, such as a GPS coordinate given in degrees).
fn request_applied(nv: &NewValue, before: &[ReadTag], after: &[ReadTag]) -> bool {
    let group = nv.group.as_deref().filter(|g| !g.is_empty());
    let all = is_all(&nv.tag);
    match &nv.value {
        None => {
            if all && group.is_none_or(is_all) {
                return !after.iter().any(|t| matches!(t.tag.group.family0.as_str(), "EXIF" | "XMP" | "IPTC" | "MakerNotes"));
            }
            // `QuickTime:all` is the metadata (ItemList, UserData, Keys), not
            // the movie structure the QuickTime group also names.
            let qt_all = all && group.is_some_and(|g| g.eq_ignore_ascii_case("quicktime"));
            let reached = |t: &ReadTag| {
                all || t.tag.group.family0 != "EXIF" || group.is_some_and(|g| g.eq_ignore_ascii_case(&t.tag.group.family1)) || t.write_group.as_deref().is_none_or(|w| w == t.tag.group.family1)
            };
            !after.iter().any(|t| {
                in_group(&t.tag, group)
                    && (all || t.tag.name.eq_ignore_ascii_case(&nv.tag))
                    && reached(t)
                    && (!qt_all || matches!(t.tag.group.family1.as_str(), "ItemList" | "UserData" | "Keys" | "iTunes"))
            })
        }
        Some(v) => {
            if all {
                return false;
            }
            let new_here = |t: &Tag| !before.iter().any(|b| b.tag.name == t.name && b.tag.group.family1 == t.group.family1 && b.tag.print_value == t.print_value);
            after.iter().any(|t| {
                let t = &t.tag;
                if !in_group(t, group) {
                    return false;
                }
                let holds = same_value(&t.print_value, v) || same_value(&t.raw_value.to_display_string(), v);
                if t.name.eq_ignore_ascii_case(&nv.tag) {
                    holds || new_here(t)
                } else {
                    // under another name the tag is written as (an alias,
                    // a shortcut)
                    holds && new_here(t)
                }
            })
        }
    }
}

/// Convert a tag value to a safe filename.
fn value_to_filename(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Parse a date shift string like "+1:0:0" (add 1 hour) or "-0:30:0" (subtract 30 min).
/// Returns (sign, hours, minutes, seconds).
pub fn parse_date_shift(shift: &str) -> Option<(i32, u32, u32, u32)> {
    let (sign, rest) = if let Some(stripped) = shift.strip_prefix('-') {
        (-1, stripped)
    } else if let Some(stripped) = shift.strip_prefix('+') {
        (1, stripped)
    } else {
        (1, shift)
    };

    let parts: Vec<&str> = rest.split(':').collect();
    match parts.len() {
        1 => {
            let h: u32 = parts[0].parse().ok()?;
            Some((sign, h, 0, 0))
        }
        2 => {
            let h: u32 = parts[0].parse().ok()?;
            let m: u32 = parts[1].parse().ok()?;
            Some((sign, h, m, 0))
        }
        3 => {
            let h: u32 = parts[0].parse().ok()?;
            let m: u32 = parts[1].parse().ok()?;
            let s: u32 = parts[2].parse().ok()?;
            Some((sign, h, m, s))
        }
        _ => None,
    }
}

/// Shift a datetime string by the given amount.
/// Input format: "YYYY:MM:DD HH:MM:SS"
pub fn shift_datetime(datetime: &str, shift: &str) -> Option<String> {
    let (sign, hours, minutes, seconds) = parse_date_shift(shift)?;

    // Parse date/time
    if datetime.len() < 19 {
        return None;
    }
    let year: i32 = datetime[0..4].parse().ok()?;
    let month: u32 = datetime[5..7].parse().ok()?;
    let day: u32 = datetime[8..10].parse().ok()?;
    let hour: u32 = datetime[11..13].parse().ok()?;
    let min: u32 = datetime[14..16].parse().ok()?;
    let sec: u32 = datetime[17..19].parse().ok()?;

    // Convert to total seconds, shift, convert back
    let total_secs = (hour * 3600 + min * 60 + sec) as i64
        + sign as i64 * (hours * 3600 + minutes * 60 + seconds) as i64;

    let days_shift = if total_secs < 0 {
        -1 - (-total_secs - 1) / 86400
    } else {
        total_secs / 86400
    };

    let time_secs = ((total_secs % 86400) + 86400) % 86400;
    let new_hour = (time_secs / 3600) as u32;
    let new_min = ((time_secs % 3600) / 60) as u32;
    let new_sec = (time_secs % 60) as u32;

    // Simple day shifting (doesn't handle month/year rollover perfectly for large shifts)
    let mut new_day = day as i32 + days_shift as i32;
    let mut new_month = month;
    let mut new_year = year;

    let days_in_month = |m: u32, y: i32| -> i32 {
        match m {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                    29
                } else {
                    28
                }
            }
            _ => 30,
        }
    };

    while new_day > days_in_month(new_month, new_year) {
        new_day -= days_in_month(new_month, new_year);
        new_month += 1;
        if new_month > 12 {
            new_month = 1;
            new_year += 1;
        }
    }
    while new_day < 1 {
        new_month = if new_month == 1 { 12 } else { new_month - 1 };
        if new_month == 12 {
            new_year -= 1;
        }
        new_day += days_in_month(new_month, new_year);
    }

    Some(format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
        new_year, new_month, new_day, new_hour, new_min, new_sec
    ))
}

/// Do `a` and `b` name the same file (the destination may not exist yet)?
fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// The output of a write, in a new file next to its destination: created
/// exclusively under a fresh name (that keeps the destination's extension,
/// so that ExifTool can read it back), with the permissions of the file it
/// replaces, and removed unless it is persisted.
struct TempFile {
    path: Option<PathBuf>,
}

impl TempFile {
    fn create(dst: &Path, bytes: &[u8]) -> Result<TempFile> {
        use std::io::Write;
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let dir = match dst.parent() {
            Some(p) if !p.as_os_str().is_empty() => p,
            _ => Path::new("."),
        };
        let name = dst.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let replaced = fs::metadata(dst).ok().filter(|m| m.is_file()).map(|m| m.permissions());
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
        let mut attempt = 0;
        loop {
            let seq = SEQ.fetch_add(1, Ordering::Relaxed);
            let path = dir.join(format!(".exiftool-{}-{seq}-{nanos:x}-{name}", std::process::id()));
            match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    let temp = TempFile { path: Some(path) };
                    if let Some(p) = replaced {
                        file.set_permissions(p).map_err(Error::Io)?;
                    }
                    file.write_all(bytes).map_err(Error::Io)?;
                    file.sync_all().ok();
                    return Ok(temp);
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && attempt < 100 => attempt += 1,
                Err(e) => return Err(Error::Io(e)),
            }
        }
    }

    fn path(&self) -> &Path {
        self.path.as_deref().unwrap_or(Path::new(""))
    }

    /// Rename over `dst`.
    fn persist(mut self, dst: &Path) -> Result<()> {
        let path = self.path.take().unwrap_or_default();
        if let Err(e) = fs::rename(&path, dst) {
            let _ = fs::remove_file(&path);
            return Err(Error::Io(e));
        }
        Ok(())
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if let Some(p) = self.path.take() {
            let _ = fs::remove_file(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_shift() {
        assert_eq!(parse_date_shift("+1:30:00"), Some((1, 1, 30, 0)));
        assert_eq!(parse_date_shift("-2"), Some((-1, 2, 0, 0)));
        assert_eq!(shift_datetime("2024:12:31 23:30:00", "+1:0:0").as_deref(), Some("2025:01:01 00:30:00"));
    }
}
