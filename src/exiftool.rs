//! Core ExifTool struct and public API.
//!
//! This is the main entry point for reading metadata from files.
//! Mirrors ExifTool.pm's ImageInfo/ExtractInfo/GetInfo pipeline.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::error::{Error, Result};
use crate::file_type::{self, FileType};
use crate::formats;
use crate::metadata::exif::ByteOrderMark;
use crate::tag::Tag;
use crate::value::Value;
use crate::writer::{
    exif_writer, iptc_writer, jpeg_writer, matroska_writer, mp4_writer, pdf_writer, png_writer,
    psd_writer, tiff_writer, webp_writer, xmp_writer,
};

/// Processing options for metadata extraction.
#[derive(Debug, Clone)]
pub struct Options {
    /// Include duplicate tags (different groups may have same tag name).
    pub duplicates: bool,
    /// Apply print conversions (human-readable values).
    pub print_conv: bool,
    /// Fast scan level: 0=normal, 1=skip composite, 2=skip maker notes, 3=skip thumbnails.
    pub fast_scan: u8,
    /// Only extract these tag names (empty = all).
    pub requested_tags: Vec<String>,
    /// Extract embedded documents/data (video frames, etc.). Level: 0=off, 1=-ee, 2=-ee2, 3=-ee3.
    pub extract_embedded: u8,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            duplicates: false,
            print_conv: true,
            fast_scan: 0,
            requested_tags: Vec::new(),
            extract_embedded: 0,
        }
    }
}

/// The main ExifTool struct. Create one and use it to extract metadata from files.
///
/// # Example
/// ```no_run
/// use exiftool_rs::ExifTool;
///
/// let mut et = ExifTool::new();
/// let info = et.image_info("photo.jpg").unwrap();
/// for (name, value) in &info {
///     println!("{}: {}", name, value);
/// }
/// ```
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

/// The main ExifTool engine — read, write, and edit metadata.
///
/// # Reading metadata
/// ```no_run
/// use exiftool_rs::ExifTool;
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
/// use exiftool_rs::ExifTool;
///
/// let mut et = ExifTool::new();
/// et.set_new_value("Artist", Some("John Doe"));
/// et.set_new_value("Copyright", Some("2024"));
/// et.write_info("input.jpg", "output.jpg").unwrap();
/// ```
pub struct ExifTool {
    options: Options,
    new_values: Vec<NewValue>,
}

/// Result of metadata extraction: maps tag names to display values.
pub type ImageInfo = HashMap<String, String>;

impl ExifTool {
    /// Create a new ExifTool instance with default options.
    pub fn new() -> Self {
        Self {
            options: Options::default(),
            new_values: Vec::new(),
        }
    }

    /// Create a new ExifTool instance with custom options.
    pub fn with_options(options: Options) -> Self {
        Self {
            options,
            new_values: Vec::new(),
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

    /// Queue a new tag value for writing.
    ///
    /// Call this one or more times, then call `write_info()` to apply changes.
    ///
    /// # Arguments
    /// * `tag` - Tag name, optionally prefixed with group (e.g., "Artist", "XMP:Title", "EXIF:Copyright")
    /// * `value` - New value, or None to delete the tag
    ///
    /// # Example
    /// ```no_run
    /// use exiftool_rs::ExifTool;
    /// let mut et = ExifTool::new();
    /// et.set_new_value("Artist", Some("John Doe"));
    /// et.set_new_value("Copyright", Some("2024 John Doe"));
    /// et.set_new_value("XMP:Title", Some("My Photo"));
    /// et.write_info("photo.jpg", "photo_out.jpg").unwrap();
    /// ```
    pub fn set_new_value(&mut self, tag: &str, value: Option<&str>) {
        let (group, tag_name) = if let Some(colon_pos) = tag.find(':') {
            (
                Some(tag[..colon_pos].to_string()),
                tag[colon_pos + 1..].to_string(),
            )
        } else {
            (None, tag.to_string())
        };

        self.new_values.push(NewValue {
            tag: tag_name,
            group,
            value: value.map(|v| v.to_string()),
        });
    }

    /// Clear all queued new values.
    pub fn clear_new_values(&mut self) {
        self.new_values.clear();
    }

    /// Copy tags from a source file, queuing them as new values.
    ///
    /// Reads all tags from `src_path` and queues them for writing.
    /// Optionally filter by tag names.
    pub fn set_new_values_from_file<P: AsRef<Path>>(
        &mut self,
        src_path: P,
        tags_to_copy: Option<&[&str]>,
    ) -> Result<u32> {
        let src_tags = self.extract_info(src_path)?;
        let mut count = 0u32;

        for tag in &src_tags {
            // Skip file-level tags that shouldn't be copied
            if tag.group.family0 == "File" || tag.group.family0 == "Composite" {
                continue;
            }
            // Skip binary/undefined data and empty values
            if tag.print_value.starts_with("(Binary") || tag.print_value.starts_with("(Undefined") {
                continue;
            }
            if tag.print_value.is_empty() {
                continue;
            }

            // Filter by requested tags
            if let Some(filter) = tags_to_copy {
                let name_lower = tag.name.to_lowercase();
                if !filter.iter().any(|f| f.to_lowercase() == name_lower) {
                    continue;
                }
            }

            let _full_tag = format!("{}:{}", tag.group.family0, tag.name);
            self.new_values.push(NewValue {
                tag: tag.name.clone(),
                group: Some(tag.group.family0.clone()),
                value: Some(tag.print_value.clone()),
            });
            count += 1;
        }

        Ok(count)
    }

    /// Set a file's name based on a tag value.
    pub fn set_file_name_from_tag<P: AsRef<Path>>(
        &self,
        path: P,
        tag_name: &str,
        template: &str,
    ) -> Result<String> {
        let path = path.as_ref();
        let tags = self.extract_info(path)?;

        let tag_value = tags
            .iter()
            .find(|t| t.name.to_lowercase() == tag_name.to_lowercase())
            .map(|t| &t.print_value)
            .ok_or_else(|| Error::TagNotFound(tag_name.to_string()))?;

        // Build new filename from template
        // Template: "prefix%value%suffix.ext" or just use the tag value
        let new_name = if template.contains('%') {
            template.replace("%v", value_to_filename(tag_value).as_str())
        } else {
            // Default: use tag value as filename, keep extension
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

    /// Write queued changes to a file.
    ///
    /// If `dst_path` is the same as `src_path`, the file is modified in-place
    /// (via a temporary file).
    pub fn write_info<P: AsRef<Path>, Q: AsRef<Path>>(
        &self,
        src_path: P,
        dst_path: Q,
    ) -> Result<u32> {
        let src_path = src_path.as_ref();
        let dst_path = dst_path.as_ref();
        let data = fs::read(src_path).map_err(Error::Io)?;

        let file_type = self.detect_file_type(&data, src_path)?;
        let output = self.apply_changes(&data, file_type)?;

        // Write to temp file first, then rename (atomic)
        let temp_path = dst_path.with_extension("exiftool_tmp");
        fs::write(&temp_path, &output).map_err(Error::Io)?;
        fs::rename(&temp_path, dst_path).map_err(Error::Io)?;

        Ok(self.new_values.len() as u32)
    }

    /// Apply queued changes to in-memory data.
    fn apply_changes(&self, data: &[u8], file_type: FileType) -> Result<Vec<u8>> {
        match file_type {
            FileType::Jpeg => self.write_jpeg(data),
            FileType::Png => self.write_png(data),
            FileType::Tiff
            | FileType::Dng
            | FileType::Cr2
            | FileType::Nef
            | FileType::Arw
            | FileType::Orf
            | FileType::Pef => self.write_tiff(data),
            FileType::WebP => self.write_webp(data),
            FileType::Mp4
            | FileType::QuickTime
            | FileType::M4a
            | FileType::ThreeGP
            | FileType::F4v => self.write_mp4(data),
            FileType::Psd => self.write_psd(data),
            FileType::Pdf => self.write_pdf(data),
            FileType::Heif | FileType::Avif => self.write_mp4(data),
            FileType::Mkv | FileType::WebM => self.write_matroska(data),
            FileType::Gif => {
                let comment = self
                    .new_values
                    .iter()
                    .find(|nv| nv.tag.to_lowercase() == "comment")
                    .and_then(|nv| nv.value.clone());
                crate::writer::gif_writer::write_gif(data, comment.as_deref())
            }
            FileType::Flac => {
                let changes: Vec<(&str, &str)> = self
                    .new_values
                    .iter()
                    .filter_map(|nv| Some((nv.tag.as_str(), nv.value.as_deref()?)))
                    .collect();
                crate::writer::flac_writer::write_flac(data, &changes)
            }
            FileType::Mp3 | FileType::Aiff => {
                let changes: Vec<(&str, &str)> = self
                    .new_values
                    .iter()
                    .filter_map(|nv| Some((nv.tag.as_str(), nv.value.as_deref()?)))
                    .collect();
                crate::writer::id3_writer::write_id3(data, &changes)
            }
            FileType::Jp2 | FileType::Jxl => {
                let new_xmp = if self
                    .new_values
                    .iter()
                    .any(|nv| nv.group.as_deref() == Some("XMP"))
                {
                    let refs: Vec<&NewValue> = self
                        .new_values
                        .iter()
                        .filter(|nv| nv.group.as_deref() == Some("XMP"))
                        .collect();
                    Some(self.build_new_xmp(&refs))
                } else {
                    None
                };
                crate::writer::jp2_writer::write_jp2(data, new_xmp.as_deref(), None)
            }
            FileType::PostScript => {
                let changes: Vec<(&str, &str)> = self
                    .new_values
                    .iter()
                    .filter_map(|nv| Some((nv.tag.as_str(), nv.value.as_deref()?)))
                    .collect();
                crate::writer::ps_writer::write_postscript(data, &changes)
            }
            FileType::Ogg | FileType::Opus => {
                let changes: Vec<(&str, &str)> = self
                    .new_values
                    .iter()
                    .filter_map(|nv| Some((nv.tag.as_str(), nv.value.as_deref()?)))
                    .collect();
                crate::writer::ogg_writer::write_ogg(data, &changes)
            }
            FileType::Xmp => {
                let props: Vec<xmp_writer::XmpProperty> = self
                    .new_values
                    .iter()
                    .filter_map(|nv| {
                        let val = nv.value.as_deref()?;
                        Some(xmp_writer::XmpProperty {
                            namespace: nv.group.clone().unwrap_or_else(|| "dc".into()),
                            property: nv.tag.clone(),
                            values: vec![val.to_string()],
                            prop_type: xmp_writer::XmpPropertyType::Simple,
                        })
                    })
                    .collect();
                Ok(crate::writer::xmp_sidecar_writer::write_xmp_sidecar(&props))
            }
            _ => Err(Error::UnsupportedFileType(format!(
                "writing not yet supported for {}",
                file_type
            ))),
        }
    }

    /// Returns the set of tag names (lowercase) that are writable for a given file type.
    /// Returns `None` if any tag is writable (open-ended formats like PNG, FLAC, MKV).
    /// Returns `Some(empty set)` if the format has no writer.
    pub fn writable_tags(file_type: FileType) -> Option<std::collections::HashSet<&'static str>> {
        use std::collections::HashSet;

        // EXIF tags supported by exif_writer
        const EXIF_TAGS: &[&str] = &[
            "imagedescription",
            "make",
            "model",
            "orientation",
            "xresolution",
            "yresolution",
            "resolutionunit",
            "software",
            "modifydate",
            "datetime",
            "artist",
            "copyright",
            "datetimeoriginal",
            "createdate",
            "datetimedigitized",
            "usercomment",
            "imageuniqueid",
            "ownername",
            "cameraownername",
            "serialnumber",
            "bodyserialnumber",
            "lensmake",
            "lensmodel",
            "lensserialnumber",
        ];

        // IPTC tags supported by iptc_writer
        const IPTC_TAGS: &[&str] = &[
            "objectname",
            "title",
            "urgency",
            "category",
            "supplementalcategories",
            "keywords",
            "specialinstructions",
            "datecreated",
            "timecreated",
            "by-line",
            "author",
            "byline",
            "by-linetitle",
            "authorsposition",
            "bylinetitle",
            "city",
            "sub-location",
            "sublocation",
            "province-state",
            "state",
            "provincestate",
            "country-primarylocationcode",
            "countrycode",
            "country-primarylocationname",
            "country",
            "headline",
            "credit",
            "source",
            "copyrightnotice",
            "contact",
            "caption-abstract",
            "caption",
            "description",
            "writer-editor",
            "captionwriter",
        ];

        // XMP auto-detected tags (no group prefix needed)
        const XMP_AUTO_TAGS: &[&str] = &[
            "title",
            "description",
            "subject",
            "creator",
            "rights",
            "keywords",
            "rating",
            "label",
            "hierarchicalsubject",
        ];

        // ID3 tags
        const ID3_TAGS: &[&str] = &[
            "title",
            "artist",
            "album",
            "year",
            "date",
            "track",
            "genre",
            "comment",
            "composer",
            "albumartist",
            "encoder",
            "encodedby",
            "publisher",
            "copyright",
            "bpm",
            "lyrics",
        ];

        // MP4/MOV ilst tags
        const MP4_TAGS: &[&str] = &[
            "title",
            "artist",
            "album",
            "year",
            "date",
            "comment",
            "genre",
            "composer",
            "writer",
            "encoder",
            "encodedby",
            "grouping",
            "lyrics",
            "description",
            "albumartist",
            "copyright",
        ];

        // PDF Info dict tags
        const PDF_TAGS: &[&str] = &[
            "title", "author", "subject", "keywords", "creator", "producer",
        ];

        // PostScript DSC tags
        const PS_TAGS: &[&str] = &[
            "title",
            "creator",
            "author",
            "for",
            "creationdate",
            "createdate",
        ];

        match file_type {
            // Open-ended: any tag name accepted
            FileType::Png
            | FileType::Flac
            | FileType::Mkv
            | FileType::WebM
            | FileType::Ogg
            | FileType::Opus
            | FileType::Xmp => None,

            // JPEG: EXIF + IPTC + XMP auto + comment
            FileType::Jpeg => {
                let mut set: HashSet<&str> = HashSet::new();
                set.extend(EXIF_TAGS);
                set.extend(IPTC_TAGS);
                set.extend(XMP_AUTO_TAGS);
                set.insert("comment");
                Some(set)
            }

            // TIFF-based: EXIF only
            FileType::Tiff
            | FileType::Dng
            | FileType::Cr2
            | FileType::Nef
            | FileType::Arw
            | FileType::Orf
            | FileType::Pef => {
                let mut set: HashSet<&str> = HashSet::new();
                set.extend(EXIF_TAGS);
                Some(set)
            }

            // WebP: EXIF + XMP auto
            FileType::WebP => {
                let mut set: HashSet<&str> = HashSet::new();
                set.extend(EXIF_TAGS);
                set.extend(XMP_AUTO_TAGS);
                Some(set)
            }

            // MP4/MOV/HEIF: ilst + XMP auto
            FileType::Mp4
            | FileType::QuickTime
            | FileType::M4a
            | FileType::ThreeGP
            | FileType::F4v
            | FileType::Heif
            | FileType::Avif => {
                let mut set: HashSet<&str> = HashSet::new();
                set.extend(MP4_TAGS);
                set.extend(XMP_AUTO_TAGS);
                Some(set)
            }

            // PSD: IPTC + XMP auto
            FileType::Psd => {
                let mut set: HashSet<&str> = HashSet::new();
                set.extend(IPTC_TAGS);
                set.extend(XMP_AUTO_TAGS);
                Some(set)
            }

            FileType::Pdf => Some(PDF_TAGS.iter().copied().collect()),
            FileType::PostScript => Some(PS_TAGS.iter().copied().collect()),

            FileType::Mp3 | FileType::Aiff => Some(ID3_TAGS.iter().copied().collect()),

            FileType::Gif => {
                let mut set: HashSet<&str> = HashSet::new();
                set.insert("comment");
                Some(set)
            }

            // JP2/JXL: XMP only (with group prefix)
            FileType::Jp2 | FileType::Jxl => Some(XMP_AUTO_TAGS.iter().copied().collect()),

            // No writer
            _ => Some(HashSet::new()),
        }
    }

    /// Write metadata changes to JPEG data.
    fn write_jpeg(&self, data: &[u8]) -> Result<Vec<u8>> {
        // Classify new values by target group
        let mut exif_values: Vec<&NewValue> = Vec::new();
        let mut xmp_values: Vec<&NewValue> = Vec::new();
        let mut iptc_values: Vec<&NewValue> = Vec::new();
        let mut comment_value: Option<&str> = None;
        let mut remove_exif = false;
        let mut remove_xmp = false;
        let mut remove_iptc = false;
        let mut remove_comment = false;

        for nv in &self.new_values {
            let group = nv.group.as_deref().unwrap_or("");
            let group_upper = group.to_uppercase();

            // Check for group deletion
            if nv.value.is_none() && nv.tag == "*" {
                match group_upper.as_str() {
                    "EXIF" => {
                        remove_exif = true;
                        continue;
                    }
                    "XMP" => {
                        remove_xmp = true;
                        continue;
                    }
                    "IPTC" => {
                        remove_iptc = true;
                        continue;
                    }
                    _ => {}
                }
            }

            match group_upper.as_str() {
                "XMP" => xmp_values.push(nv),
                "IPTC" => iptc_values.push(nv),
                "EXIF" | "IFD0" | "EXIFIFD" | "GPS" => exif_values.push(nv),
                "" => {
                    // Auto-detect best group based on tag name
                    if nv.tag.to_lowercase() == "comment" {
                        if nv.value.is_none() {
                            remove_comment = true;
                        } else {
                            comment_value = nv.value.as_deref();
                        }
                    } else if is_xmp_tag(&nv.tag) {
                        xmp_values.push(nv);
                    } else {
                        exif_values.push(nv);
                    }
                }
                _ => exif_values.push(nv), // default to EXIF
            }
        }

        // Build new EXIF data
        let new_exif = if !exif_values.is_empty() {
            Some(self.build_new_exif(data, &exif_values)?)
        } else {
            None
        };

        // Build new XMP data
        let new_xmp = if !xmp_values.is_empty() {
            Some(self.build_new_xmp(&xmp_values))
        } else {
            None
        };

        // Build new IPTC data
        let new_iptc_data = if !iptc_values.is_empty() {
            let records: Vec<iptc_writer::IptcRecord> = iptc_values
                .iter()
                .filter_map(|nv| {
                    let value = nv.value.as_deref()?;
                    let (record, dataset) = iptc_writer::tag_name_to_iptc(&nv.tag)?;
                    Some(iptc_writer::IptcRecord {
                        record,
                        dataset,
                        data: value.as_bytes().to_vec(),
                    })
                })
                .collect();
            if records.is_empty() {
                None
            } else {
                Some(iptc_writer::build_iptc(&records))
            }
        } else {
            None
        };

        // Rewrite JPEG
        jpeg_writer::write_jpeg(
            data,
            new_exif.as_deref(),
            new_xmp.as_deref(),
            new_iptc_data.as_deref(),
            comment_value,
            remove_exif,
            remove_xmp,
            remove_iptc,
            remove_comment,
        )
    }

    /// Build new EXIF data by merging existing EXIF with queued changes.
    fn build_new_exif(&self, jpeg_data: &[u8], values: &[&NewValue]) -> Result<Vec<u8>> {
        let bo = ByteOrderMark::BigEndian;
        let mut ifd0_entries = Vec::new();
        let mut exif_entries = Vec::new();
        let mut gps_entries = Vec::new();

        // Step 1: Extract existing EXIF entries from the JPEG
        let existing = extract_existing_exif_entries(jpeg_data, bo);
        for entry in &existing {
            match classify_exif_tag(entry.tag) {
                ExifIfdGroup::Ifd0 => ifd0_entries.push(entry.clone()),
                ExifIfdGroup::ExifIfd => exif_entries.push(entry.clone()),
                ExifIfdGroup::Gps => gps_entries.push(entry.clone()),
            }
        }

        // Step 2: Apply queued changes (add/replace/delete)
        let deleted_tags: Vec<u16> = values
            .iter()
            .filter(|nv| nv.value.is_none())
            .filter_map(|nv| tag_name_to_id(&nv.tag))
            .collect();

        // Remove deleted tags
        ifd0_entries.retain(|e| !deleted_tags.contains(&e.tag));
        exif_entries.retain(|e| !deleted_tags.contains(&e.tag));
        gps_entries.retain(|e| !deleted_tags.contains(&e.tag));

        // Add/replace new values
        for nv in values {
            if nv.value.is_none() {
                continue;
            }
            let value_str = nv.value.as_deref().unwrap_or("");
            let group = nv.group.as_deref().unwrap_or("");

            if let Some((tag_id, format, encoded)) = encode_exif_tag(&nv.tag, value_str, group, bo)
            {
                let entry = exif_writer::IfdEntry {
                    tag: tag_id,
                    format,
                    data: encoded,
                };

                let target = match group.to_uppercase().as_str() {
                    "GPS" => &mut gps_entries,
                    "EXIFIFD" => &mut exif_entries,
                    _ => match classify_exif_tag(tag_id) {
                        ExifIfdGroup::ExifIfd => &mut exif_entries,
                        ExifIfdGroup::Gps => &mut gps_entries,
                        ExifIfdGroup::Ifd0 => &mut ifd0_entries,
                    },
                };

                // Replace existing or add new
                if let Some(existing) = target.iter_mut().find(|e| e.tag == tag_id) {
                    *existing = entry;
                } else {
                    target.push(entry);
                }
            }
        }

        // Remove sub-IFD pointers from entries (they'll be rebuilt by build_exif)
        ifd0_entries.retain(|e| e.tag != 0x8769 && e.tag != 0x8825 && e.tag != 0xA005);

        exif_writer::build_exif(&ifd0_entries, &exif_entries, &gps_entries, bo)
    }

    /// Write metadata changes to PNG data.
    fn write_png(&self, data: &[u8]) -> Result<Vec<u8>> {
        let mut new_text: Vec<(&str, &str)> = Vec::new();
        let mut remove_text: Vec<&str> = Vec::new();

        // Collect text-based changes
        // We need to hold the strings in vectors that live long enough
        let owned_pairs: Vec<(String, String)> = self
            .new_values
            .iter()
            .filter(|nv| nv.value.is_some())
            .map(|nv| (nv.tag.clone(), nv.value.clone().unwrap()))
            .collect();

        for (tag, value) in &owned_pairs {
            new_text.push((tag.as_str(), value.as_str()));
        }

        for nv in &self.new_values {
            if nv.value.is_none() {
                remove_text.push(&nv.tag);
            }
        }

        png_writer::write_png(data, &new_text, None, &remove_text)
    }

    /// Write metadata changes to PSD data.
    fn write_psd(&self, data: &[u8]) -> Result<Vec<u8>> {
        let mut iptc_values = Vec::new();
        let mut xmp_values = Vec::new();

        for nv in &self.new_values {
            let group = nv.group.as_deref().unwrap_or("").to_uppercase();
            match group.as_str() {
                "XMP" => xmp_values.push(nv),
                "IPTC" => iptc_values.push(nv),
                _ => {
                    if is_xmp_tag(&nv.tag) {
                        xmp_values.push(nv);
                    } else {
                        iptc_values.push(nv);
                    }
                }
            }
        }

        let new_iptc = if !iptc_values.is_empty() {
            let records: Vec<_> = iptc_values
                .iter()
                .filter_map(|nv| {
                    let value = nv.value.as_deref()?;
                    let (record, dataset) = iptc_writer::tag_name_to_iptc(&nv.tag)?;
                    Some(iptc_writer::IptcRecord {
                        record,
                        dataset,
                        data: value.as_bytes().to_vec(),
                    })
                })
                .collect();
            if records.is_empty() {
                None
            } else {
                Some(iptc_writer::build_iptc(&records))
            }
        } else {
            None
        };

        let new_xmp = if !xmp_values.is_empty() {
            let refs: Vec<&NewValue> = xmp_values.to_vec();
            Some(self.build_new_xmp(&refs))
        } else {
            None
        };

        psd_writer::write_psd(data, new_iptc.as_deref(), new_xmp.as_deref())
    }

    /// Write metadata changes to Matroska (MKV/WebM) data.
    fn write_matroska(&self, data: &[u8]) -> Result<Vec<u8>> {
        let changes: Vec<(&str, &str)> = self
            .new_values
            .iter()
            .filter_map(|nv| {
                let value = nv.value.as_deref()?;
                Some((nv.tag.as_str(), value))
            })
            .collect();

        matroska_writer::write_matroska(data, &changes)
    }

    /// Write metadata changes to PDF data.
    fn write_pdf(&self, data: &[u8]) -> Result<Vec<u8>> {
        let changes: Vec<(&str, &str)> = self
            .new_values
            .iter()
            .filter_map(|nv| {
                let value = nv.value.as_deref()?;
                Some((nv.tag.as_str(), value))
            })
            .collect();

        pdf_writer::write_pdf(data, &changes)
    }

    /// Write metadata changes to MP4/MOV data.
    fn write_mp4(&self, data: &[u8]) -> Result<Vec<u8>> {
        let mut ilst_tags: Vec<([u8; 4], String)> = Vec::new();
        let mut xmp_values: Vec<&NewValue> = Vec::new();

        for nv in &self.new_values {
            if nv.value.is_none() {
                continue;
            }
            let group = nv.group.as_deref().unwrap_or("").to_uppercase();
            if group == "XMP" {
                xmp_values.push(nv);
            } else if let Some(key) = mp4_writer::tag_to_ilst_key(&nv.tag) {
                ilst_tags.push((key, nv.value.clone().unwrap()));
            }
        }

        let tag_refs: Vec<(&[u8; 4], &str)> =
            ilst_tags.iter().map(|(k, v)| (k, v.as_str())).collect();

        let new_xmp = if !xmp_values.is_empty() {
            let refs: Vec<&NewValue> = xmp_values.to_vec();
            Some(self.build_new_xmp(&refs))
        } else {
            None
        };

        mp4_writer::write_mp4(data, &tag_refs, new_xmp.as_deref())
    }

    /// Write metadata changes to WebP data.
    fn write_webp(&self, data: &[u8]) -> Result<Vec<u8>> {
        let mut exif_values: Vec<&NewValue> = Vec::new();
        let mut xmp_values: Vec<&NewValue> = Vec::new();
        let mut remove_exif = false;
        let mut remove_xmp = false;

        for nv in &self.new_values {
            let group = nv.group.as_deref().unwrap_or("").to_uppercase();
            if nv.value.is_none() && nv.tag == "*" {
                if group == "EXIF" {
                    remove_exif = true;
                }
                if group == "XMP" {
                    remove_xmp = true;
                }
                continue;
            }
            match group.as_str() {
                "XMP" => xmp_values.push(nv),
                _ => exif_values.push(nv),
            }
        }

        let new_exif = if !exif_values.is_empty() {
            let bo = ByteOrderMark::BigEndian;
            let mut entries = Vec::new();
            for nv in &exif_values {
                if let Some(ref v) = nv.value {
                    let group = nv.group.as_deref().unwrap_or("");
                    if let Some((tag_id, format, encoded)) = encode_exif_tag(&nv.tag, v, group, bo)
                    {
                        entries.push(exif_writer::IfdEntry {
                            tag: tag_id,
                            format,
                            data: encoded,
                        });
                    }
                }
            }
            if !entries.is_empty() {
                Some(exif_writer::build_exif(&entries, &[], &[], bo)?)
            } else {
                None
            }
        } else {
            None
        };

        let new_xmp = if !xmp_values.is_empty() {
            Some(self.build_new_xmp(&xmp_values.to_vec()))
        } else {
            None
        };

        webp_writer::write_webp(
            data,
            new_exif.as_deref(),
            new_xmp.as_deref(),
            remove_exif,
            remove_xmp,
        )
    }

    /// Write metadata changes to TIFF data.
    fn write_tiff(&self, data: &[u8]) -> Result<Vec<u8>> {
        let bo = if data.starts_with(b"II") {
            ByteOrderMark::LittleEndian
        } else {
            ByteOrderMark::BigEndian
        };

        let mut changes: Vec<(u16, Vec<u8>)> = Vec::new();
        for nv in &self.new_values {
            if let Some(ref value) = nv.value {
                let group = nv.group.as_deref().unwrap_or("");
                if let Some((tag_id, _format, encoded)) = encode_exif_tag(&nv.tag, value, group, bo)
                {
                    changes.push((tag_id, encoded));
                }
            }
        }

        tiff_writer::write_tiff(data, &changes)
    }

    /// Build new XMP data from queued values.
    fn build_new_xmp(&self, values: &[&NewValue]) -> Vec<u8> {
        let mut properties = Vec::new();

        for nv in values {
            let value_str = match &nv.value {
                Some(v) => v.clone(),
                None => continue,
            };

            let ns = nv.group.as_deref().unwrap_or("dc").to_lowercase();
            let ns = if ns == "xmp" { "xmp".to_string() } else { ns };

            let prop_type = match nv.tag.to_lowercase().as_str() {
                "title" | "description" | "rights" => xmp_writer::XmpPropertyType::LangAlt,
                "subject" | "keywords" => xmp_writer::XmpPropertyType::Bag,
                "creator" => xmp_writer::XmpPropertyType::Seq,
                _ => xmp_writer::XmpPropertyType::Simple,
            };

            let values = if matches!(
                prop_type,
                xmp_writer::XmpPropertyType::Bag | xmp_writer::XmpPropertyType::Seq
            ) {
                value_str.split(',').map(|s| s.trim().to_string()).collect()
            } else {
                vec![value_str]
            };

            properties.push(xmp_writer::XmpProperty {
                namespace: ns,
                property: nv.tag.clone(),
                values,
                prop_type,
            });
        }

        xmp_writer::build_xmp(&properties).into_bytes()
    }

    // ================================================================
    // Reading API
    // ================================================================

    /// Extract metadata from a file and return a simple name→value map.
    ///
    /// This is the high-level one-shot API, equivalent to ExifTool's `ImageInfo()`.
    pub fn image_info<P: AsRef<Path>>(&self, path: P) -> Result<ImageInfo> {
        let tags = self.extract_info(path)?;
        Ok(self.get_info(&tags))
    }

    /// Extract all metadata tags from a file.
    ///
    /// Returns the full `Tag` structs with groups, raw values, etc.
    pub fn extract_info<P: AsRef<Path>>(&self, path: P) -> Result<Vec<Tag>> {
        let path = path.as_ref();
        let data = fs::read(path).map_err(Error::Io)?;

        self.extract_info_from_bytes(&data, path)
    }

    /// Extract metadata from in-memory data.
    pub fn extract_info_from_bytes(&self, data: &[u8], path: &Path) -> Result<Vec<Tag>> {
        let file_type_result = self.detect_file_type(data, path);
        let (file_type, mut tags) = match file_type_result {
            Ok(ft) => {
                let t = self
                    .process_file(data, ft)
                    .or_else(|_| self.process_by_extension(data, path))?;
                (Some(ft), t)
            }
            Err(_) => {
                // File type unknown by magic/extension — try extension-based fallback
                let t = self.process_by_extension(data, path)?;
                (None, t)
            }
        };
        let file_type = file_type.unwrap_or(FileType::Zip); // placeholder for file-level tags

        // Add file-level tags
        tags.push(Tag {
            id: crate::tag::TagId::Text("FileType".into()),
            name: "FileType".into(),
            description: "File Type".into(),
            group: crate::tag::TagGroup {
                family0: "File".into(),
                family1: "File".into(),
                family2: "Other".into(),
            },
            raw_value: Value::String(format!("{:?}", file_type)),
            print_value: file_type.description().to_string(),
            priority: 0,
        });

        tags.push(Tag {
            id: crate::tag::TagId::Text("MIMEType".into()),
            name: "MIMEType".into(),
            description: "MIME Type".into(),
            group: crate::tag::TagGroup {
                family0: "File".into(),
                family1: "File".into(),
                family2: "Other".into(),
            },
            raw_value: Value::String(file_type.mime_type().to_string()),
            print_value: file_type.mime_type().to_string(),
            priority: 0,
        });

        if let Ok(metadata) = fs::metadata(path) {
            tags.push(Tag {
                id: crate::tag::TagId::Text("FileSize".into()),
                name: "FileSize".into(),
                description: "File Size".into(),
                group: crate::tag::TagGroup {
                    family0: "File".into(),
                    family1: "File".into(),
                    family2: "Other".into(),
                },
                raw_value: Value::U32(metadata.len() as u32),
                print_value: format_file_size(metadata.len()),
                priority: 0,
            });
        }

        // Add more file-level tags
        let file_tag = |name: &str, val: Value| -> Tag {
            Tag {
                id: crate::tag::TagId::Text(name.to_string()),
                name: name.to_string(),
                description: name.to_string(),
                group: crate::tag::TagGroup {
                    family0: "File".into(),
                    family1: "File".into(),
                    family2: "Other".into(),
                },
                raw_value: val.clone(),
                print_value: val.to_display_string(),
                priority: 0,
            }
        };

        if let Some(fname) = path.file_name().and_then(|n| n.to_str()) {
            tags.push(file_tag("FileName", Value::String(fname.to_string())));
        }
        if let Some(dir) = path.parent().and_then(|p| p.to_str()) {
            tags.push(file_tag("Directory", Value::String(dir.to_string())));
        }
        // Use the canonical (first) extension from the FileType, matching Perl ExifTool behavior.
        let canonical_ext = file_type.extensions().first().copied().unwrap_or("");
        if !canonical_ext.is_empty() {
            tags.push(file_tag(
                "FileTypeExtension",
                Value::String(canonical_ext.to_string()),
            ));
        }

        #[cfg(unix)]
        if let Ok(metadata) = fs::metadata(path) {
            use std::os::unix::fs::MetadataExt;
            let mode = metadata.mode();
            tags.push(file_tag(
                "FilePermissions",
                Value::String(format!("{:o}", mode & 0o7777)),
            ));

            // FileModifyDate
            if let Ok(modified) = metadata.modified() {
                if let Ok(dur) = modified.duration_since(std::time::UNIX_EPOCH) {
                    let secs = dur.as_secs() as i64;
                    tags.push(file_tag(
                        "FileModifyDate",
                        Value::String(unix_to_datetime(secs)),
                    ));
                }
            }
            // FileAccessDate
            if let Ok(accessed) = metadata.accessed() {
                if let Ok(dur) = accessed.duration_since(std::time::UNIX_EPOCH) {
                    let secs = dur.as_secs() as i64;
                    tags.push(file_tag(
                        "FileAccessDate",
                        Value::String(unix_to_datetime(secs)),
                    ));
                }
            }
            // FileInodeChangeDate (ctime on Unix)
            let ctime = metadata.ctime();
            if ctime > 0 {
                tags.push(file_tag(
                    "FileInodeChangeDate",
                    Value::String(unix_to_datetime(ctime)),
                ));
            }
        }

        // ExifByteOrder (from TIFF header)
        {
            let bo_str = if data.len() > 8 {
                // Check EXIF in JPEG or TIFF header or WebP/RIFF EXIF chunk
                let check: Option<&[u8]> = if data.starts_with(&[0xFF, 0xD8]) {
                    // JPEG: find APP1 EXIF header
                    data.windows(6)
                        .position(|w| w == b"Exif\0\0")
                        .map(|p| &data[p + 6..])
                } else if data.starts_with(b"FUJIFILMCCD-RAW") && data.len() >= 0x60 {
                    // RAF: look in the embedded JPEG for EXIF byte order
                    let jpeg_offset =
                        u32::from_be_bytes([data[0x54], data[0x55], data[0x56], data[0x57]])
                            as usize;
                    let jpeg_length =
                        u32::from_be_bytes([data[0x58], data[0x59], data[0x5A], data[0x5B]])
                            as usize;
                    if jpeg_offset > 0 && jpeg_offset + jpeg_length <= data.len() {
                        let jpeg = &data[jpeg_offset..jpeg_offset + jpeg_length];
                        jpeg.windows(6)
                            .position(|w| w == b"Exif\0\0")
                            .map(|p| &jpeg[p + 6..])
                    } else {
                        None
                    }
                } else if data.starts_with(b"RIFF") && data.len() >= 12 {
                    // RIFF/WebP: find EXIF chunk
                    let mut riff_bo: Option<&[u8]> = None;
                    let mut pos = 12usize;
                    while pos + 8 <= data.len() {
                        let cid = &data[pos..pos + 4];
                        let csz = u32::from_le_bytes([
                            data[pos + 4],
                            data[pos + 5],
                            data[pos + 6],
                            data[pos + 7],
                        ]) as usize;
                        let cstart = pos + 8;
                        let cend = (cstart + csz).min(data.len());
                        if cid == b"EXIF" && cend > cstart {
                            let exif_data = &data[cstart..cend];
                            let tiff = if exif_data.starts_with(b"Exif\0\0") {
                                &exif_data[6..]
                            } else {
                                exif_data
                            };
                            riff_bo = Some(tiff);
                            break;
                        }
                        // Also check LIST chunks
                        if cid == b"LIST" && cend >= cstart + 4 {
                            // recurse not needed for this simple scan - just advance
                        }
                        pos = cend + (csz & 1);
                    }
                    riff_bo
                } else if data.starts_with(&[0x00, 0x00, 0x00, 0x0C, b'J', b'X', b'L', b' ']) {
                    // JXL container: scan for brob Exif box and decompress to get byte order
                    let mut jxl_bo: Option<String> = None;
                    let mut jpos = 12usize; // skip JXL signature box
                    while jpos + 8 <= data.len() {
                        let bsize = u32::from_be_bytes([
                            data[jpos],
                            data[jpos + 1],
                            data[jpos + 2],
                            data[jpos + 3],
                        ]) as usize;
                        let btype = &data[jpos + 4..jpos + 8];
                        if bsize < 8 || jpos + bsize > data.len() {
                            break;
                        }
                        if btype == b"brob" && jpos + bsize > 12 {
                            let inner_type = &data[jpos + 8..jpos + 12];
                            if inner_type == b"Exif" || inner_type == b"exif" {
                                let brotli_payload = &data[jpos + 12..jpos + bsize];
                                use std::io::Cursor;
                                let mut inp = Cursor::new(brotli_payload);
                                let mut out: Vec<u8> = Vec::new();
                                if brotli::BrotliDecompress(&mut inp, &mut out).is_ok() {
                                    let exif_start = if out.len() > 4 { 4 } else { 0 };
                                    if exif_start < out.len() {
                                        if out[exif_start..].starts_with(b"MM") {
                                            jxl_bo = Some("Big-endian (Motorola, MM)".to_string());
                                        } else if out[exif_start..].starts_with(b"II") {
                                            jxl_bo = Some("Little-endian (Intel, II)".to_string());
                                        }
                                    }
                                }
                                break;
                            }
                        }
                        jpos += bsize;
                    }
                    if let Some(bo) = jxl_bo {
                        if !bo.is_empty() && file_type != FileType::Btf {
                            tags.push(file_tag("ExifByteOrder", Value::String(bo)));
                        }
                    }
                    // Return None to skip the generic byte order check below
                    None
                } else if data.starts_with(&[0x00, b'M', b'R', b'M']) {
                    // MRW: find TTW segment which contains TIFF/EXIF data
                    let mrw_data_offset = if data.len() >= 8 {
                        u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize + 8
                    } else {
                        0
                    };
                    let mut mrw_bo: Option<&[u8]> = None;
                    let mut mpos = 8usize;
                    while mpos + 8 <= mrw_data_offset.min(data.len()) {
                        let seg_tag = &data[mpos..mpos + 4];
                        let seg_len = u32::from_be_bytes([
                            data[mpos + 4],
                            data[mpos + 5],
                            data[mpos + 6],
                            data[mpos + 7],
                        ]) as usize;
                        if seg_tag == b"\x00TTW" && mpos + 8 + seg_len <= data.len() {
                            mrw_bo = Some(&data[mpos + 8..mpos + 8 + seg_len]);
                            break;
                        }
                        mpos += 8 + seg_len;
                    }
                    mrw_bo
                } else {
                    Some(data)
                };
                if let Some(tiff) = check {
                    if tiff.starts_with(b"II") {
                        "Little-endian (Intel, II)"
                    } else if tiff.starts_with(b"MM") {
                        "Big-endian (Motorola, MM)"
                    } else {
                        ""
                    }
                } else {
                    ""
                }
            } else {
                ""
            };
            // Suppress ExifByteOrder for BigTIFF, Canon VRD/DR4 (Perl doesn't output it for these)
            // Also skip if already emitted by ExifReader (TIFF-based formats)
            let already_has_exifbyteorder = tags.iter().any(|t| t.name == "ExifByteOrder");
            if !bo_str.is_empty()
                && !already_has_exifbyteorder
                && file_type != FileType::Btf
                && file_type != FileType::Dr4
                && file_type != FileType::Vrd
                && file_type != FileType::Crw
            {
                tags.push(file_tag("ExifByteOrder", Value::String(bo_str.to_string())));
            }
        }

        tags.push(file_tag(
            "ExifToolVersion",
            Value::String(crate::VERSION.to_string()),
        ));

        // Compute composite tags
        let composite = crate::composite::compute_composite_tags(&tags);
        tags.extend(composite);

        // FLIR post-processing: remove LensID composite for FLIR cameras.
        // Perl's LensID composite requires LensType EXIF tag (not present in FLIR images),
        // and LensID-2 requires LensModel to match /(mm|\d\/F)/ (FLIR names like "FOL7"
        // don't match).  Our composite.rs uses a simpler fallback that picks up any non-empty
        // LensModel, so we remove LensID when the image is from a FLIR camera with FFF data.
        {
            let is_flir_fff = tags
                .iter()
                .any(|t| t.group.family0 == "APP1" && t.group.family1 == "FLIR");
            if is_flir_fff {
                tags.retain(|t| !(t.name == "LensID" && t.group.family0 == "Composite"));
            }
        }

        // Olympus post-processing: remove the generic "Lens" composite for Olympus cameras.
        // In Perl, the "Lens" composite tag requires Canon:MinFocalLength (Canon namespace).
        // Our composite.rs generates Lens for any manufacturer that has MinFocalLength +
        // MaxFocalLength (e.g., Olympus Equipment sub-IFD).  Remove it for non-Canon cameras.
        {
            let make = tags
                .iter()
                .find(|t| t.name == "Make")
                .map(|t| t.print_value.clone())
                .unwrap_or_default();
            if !make.to_uppercase().contains("CANON") {
                tags.retain(|t| t.name != "Lens" || t.group.family0 != "Composite");
            }
        }

        // Priority-based deduplication: when the same tag name appears from both RIFF (priority 0)
        // and MakerNotes/EXIF (priority 0 but higher-quality source), remove the RIFF copy.
        // Mirrors ExifTool's PRIORITY => 0 behavior for RIFF StreamHeader tags.
        {
            let riff_priority_zero_tags = ["Quality", "SampleSize", "StreamType"];
            for tag_name in &riff_priority_zero_tags {
                let has_makernotes = tags
                    .iter()
                    .any(|t| t.name == *tag_name && t.group.family0 != "RIFF");
                if has_makernotes {
                    tags.retain(|t| !(t.name == *tag_name && t.group.family0 == "RIFF"));
                }
            }
        }

        // Priority-based deduplication: when the same tag name appears multiple times,
        // keep only the one with the highest priority (e.g., EXIF over JFIF, FFF over MakerNote).
        if !self.options.duplicates {
            let mut best_priority: HashMap<String, i32> = HashMap::new();
            for tag in &tags {
                let entry = best_priority
                    .entry(tag.name.clone())
                    .or_insert(tag.priority);
                if tag.priority > *entry {
                    *entry = tag.priority;
                }
            }
            tags.retain(|t| t.priority >= *best_priority.get(&t.name).unwrap_or(&0));
        }

        // Filter by requested tags if specified
        if !self.options.requested_tags.is_empty() {
            let requested: Vec<String> = self
                .options
                .requested_tags
                .iter()
                .map(|t| t.to_lowercase())
                .collect();
            tags.retain(|t| requested.contains(&t.name.to_lowercase()));
        }

        Ok(tags)
    }

    /// Format extracted tags into a simple name→value map.
    ///
    /// Handles duplicate tag names by appending group info.
    fn get_info(&self, tags: &[Tag]) -> ImageInfo {
        let mut info = ImageInfo::new();
        let mut seen: HashMap<String, (usize, i32)> = HashMap::new(); // (count, best priority)

        for tag in tags {
            let value = if self.options.print_conv {
                &tag.print_value
            } else {
                &tag.raw_value.to_display_string()
            };

            let entry = seen.entry(tag.name.clone()).or_insert((0, i32::MIN));
            entry.0 += 1;

            if entry.0 == 1 {
                entry.1 = tag.priority;
                info.insert(tag.name.clone(), value.clone());
            } else if tag.priority > entry.1 {
                // Higher priority tag replaces the previous one
                entry.1 = tag.priority;
                info.insert(tag.name.clone(), value.clone());
            } else if self.options.duplicates {
                let key = format!("{} [{}:{}]", tag.name, tag.group.family0, tag.group.family1);
                info.insert(key, value.clone());
            }
        }

        info
    }

    /// Detect file type from magic bytes and extension.
    fn detect_file_type(&self, data: &[u8], path: &Path) -> Result<FileType> {
        // Try magic bytes first
        let header_len = data.len().min(256);
        if let Some(ft) = file_type::detect_from_magic(&data[..header_len]) {
            // Override ICO to Font if extension is .dfont (Mac resource fork)
            if ft == FileType::Ico {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if ext.eq_ignore_ascii_case("dfont") {
                        return Ok(FileType::Font);
                    }
                }
            }
            // Override JPEG to JPS if the file extension is .jps
            if ft == FileType::Jpeg {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if ext.eq_ignore_ascii_case("jps") {
                        return Ok(FileType::Jps);
                    }
                }
            }
            // Override PLIST to AAE if extension is .aae
            if ft == FileType::Plist {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if ext.eq_ignore_ascii_case("aae") {
                        return Ok(FileType::Aae);
                    }
                }
            }
            // Override XMP to PLIST/AAE if extension is .plist or .aae
            if ft == FileType::Xmp {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if ext.eq_ignore_ascii_case("plist") {
                        return Ok(FileType::Plist);
                    }
                    if ext.eq_ignore_ascii_case("aae") {
                        return Ok(FileType::Aae);
                    }
                }
            }
            // Override to PhotoCD if extension is .pcd (file starts with 0xFF padding)
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if ext.eq_ignore_ascii_case("pcd")
                    && data.len() >= 2056
                    && &data[2048..2055] == b"PCD_IPI"
                {
                    return Ok(FileType::PhotoCd);
                }
            }
            // Override MP3 to MPC/APE/WavPack if extension says otherwise
            if ft == FileType::Mp3 {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if ext.eq_ignore_ascii_case("mpc") {
                        return Ok(FileType::Mpc);
                    }
                    if ext.eq_ignore_ascii_case("ape") {
                        return Ok(FileType::Ape);
                    }
                    if ext.eq_ignore_ascii_case("wv") {
                        return Ok(FileType::WavPack);
                    }
                }
            }
            // For ZIP files, check if it's an EIP (by extension) or OpenDocument format
            if ft == FileType::Zip {
                // Check extension first for EIP
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if ext.eq_ignore_ascii_case("eip") {
                        return Ok(FileType::Eip);
                    }
                }
                if let Some(od_type) = detect_opendocument_type(data) {
                    return Ok(od_type);
                }
            }
            return Ok(ft);
        }

        // Fall back to extension
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if let Some(ft) = file_type::detect_from_extension(ext) {
                return Ok(ft);
            }
        }

        let ext_str = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("unknown");
        Err(Error::UnsupportedFileType(ext_str.to_string()))
    }

    /// Dispatch to the appropriate format reader.
    fn process_file(&self, data: &[u8], file_type: FileType) -> Result<Vec<Tag>> {
        match file_type {
            FileType::Jpeg | FileType::Jps => formats::jpeg::read_jpeg(data),
            FileType::Png | FileType::Mng => formats::png::read_png(data),
            // All TIFF-based formats (TIFF + most RAW formats)
            FileType::Tiff
            | FileType::Btf
            | FileType::Dng
            | FileType::Cr2
            | FileType::Nef
            | FileType::Arw
            | FileType::Sr2
            | FileType::Orf
            | FileType::Pef
            | FileType::Erf
            | FileType::Fff
            | FileType::Rwl
            | FileType::Mef
            | FileType::Srw
            | FileType::Gpr
            | FileType::Arq
            | FileType::ThreeFR
            | FileType::Dcr
            | FileType::Rw2
            | FileType::Srf => formats::tiff::read_tiff(data),
            // Phase One IIQ: TIFF + PhaseOne maker note block
            FileType::Iiq => formats::misc::read_iiq(data),
            // Image formats
            FileType::Gif => formats::gif::read_gif(data),
            FileType::Bmp => formats::bmp::read_bmp(data),
            FileType::WebP | FileType::Avi | FileType::Wav => formats::riff::read_riff(data),
            FileType::Psd => formats::psd::read_psd(data),
            // Audio formats
            FileType::Mp3 => formats::id3::read_mp3(data),
            FileType::Flac => formats::flac::read_flac(data),
            FileType::Ogg | FileType::Opus => formats::ogg::read_ogg(data),
            FileType::Aiff => formats::aiff::read_aiff(data),
            // Video formats
            FileType::Mp4
            | FileType::QuickTime
            | FileType::M4a
            | FileType::ThreeGP
            | FileType::Heif
            | FileType::Avif
            | FileType::Cr3
            | FileType::Crm
            | FileType::F4v
            | FileType::Mqv
            | FileType::Lrv => {
                formats::quicktime::read_quicktime_with_ee(data, self.options.extract_embedded)
            }
            FileType::Mkv | FileType::WebM => formats::matroska::read_matroska(data),
            FileType::Asf | FileType::Wmv | FileType::Wma => formats::asf::read_asf(data),
            FileType::Wtv => formats::wtv::read_wtv(data),
            // RAW formats with custom containers
            FileType::Crw => formats::canon_raw::read_crw(data),
            FileType::Raf => formats::raf::read_raf(data),
            FileType::Mrw => formats::mrw::read_mrw(data),
            FileType::Mrc => formats::mrc::read_mrc(data),
            // Image formats
            FileType::Jp2 => formats::jp2::read_jp2(data),
            FileType::J2c => formats::jp2::read_j2c(data),
            FileType::Jxl => formats::jp2::read_jxl(data),
            FileType::Ico => formats::ico::read_ico(data),
            FileType::Icc => formats::icc::read_icc(data),
            // Documents
            FileType::Pdf => formats::pdf::read_pdf(data),
            FileType::PostScript => {
                // PFA fonts start with %!PS-AdobeFont or %!FontType1
                if data.starts_with(b"%!PS-AdobeFont") || data.starts_with(b"%!FontType1") {
                    formats::font::read_pfa(data)
                        .or_else(|_| formats::postscript::read_postscript(data))
                } else {
                    formats::postscript::read_postscript(data)
                }
            }
            FileType::Eip => formats::capture_one::read_eip(data),
            FileType::Zip
            | FileType::Docx
            | FileType::Xlsx
            | FileType::Pptx
            | FileType::Doc
            | FileType::Xls
            | FileType::Ppt => formats::zip::read_zip(data),
            FileType::Rtf => formats::rtf::read_rtf(data),
            FileType::InDesign => formats::misc::read_indesign(data),
            FileType::Pcap => formats::misc::read_pcap(data),
            FileType::Pcapng => formats::misc::read_pcapng(data),
            // Canon VRD / DR4
            FileType::Vrd => formats::canon_vrd::read_vrd(data).or_else(|_| Ok(Vec::new())),
            FileType::Dr4 => formats::canon_vrd::read_dr4(data).or_else(|_| Ok(Vec::new())),
            // Metadata / Other
            FileType::Xmp => formats::xmp_file::read_xmp(data),
            FileType::Svg => formats::misc::read_svg(data),
            FileType::Html => {
                // SVG files that weren't detected by magic (e.g., via extension fallback)
                let is_svg = data.windows(4).take(512).any(|w| w == b"<svg");
                if is_svg {
                    formats::misc::read_svg(data)
                } else {
                    formats::html::read_html(data)
                }
            }
            FileType::Exe => formats::exe::read_exe(data),
            FileType::Font => {
                // AFM: Adobe Font Metrics text file
                if data.starts_with(b"StartFontMetrics") {
                    return formats::font::read_afm(data);
                }
                // PFA: PostScript Type 1 ASCII font
                if data.starts_with(b"%!PS-AdobeFont") || data.starts_with(b"%!FontType1") {
                    return formats::font::read_pfa(data).or_else(|_| Ok(Vec::new()));
                }
                // PFB: PostScript Type 1 Binary font
                if data.len() >= 2 && data[0] == 0x80 && (data[1] == 0x01 || data[1] == 0x02) {
                    return formats::font::read_pfb(data).or_else(|_| Ok(Vec::new()));
                }
                formats::font::read_font(data)
            }
            // Audio with ID3
            FileType::WavPack | FileType::Dsf => formats::id3::read_mp3(data),
            FileType::Ape => formats::ape::read_ape(data),
            FileType::Mpc => formats::ape::read_mpc(data),
            FileType::Aac => formats::misc::read_aac(data),
            FileType::RealAudio => formats::misc::read_real_audio(data).or_else(|_| Ok(Vec::new())),
            FileType::RealMedia => formats::misc::read_real_media(data).or_else(|_| Ok(Vec::new())),
            // Misc formats
            FileType::Czi => formats::misc::read_czi(data).or_else(|_| Ok(Vec::new())),
            FileType::PhotoCd => formats::misc::read_photo_cd(data).or_else(|_| Ok(Vec::new())),
            FileType::Dicom => formats::dicom::read_dicom(data),
            FileType::Fits => formats::misc::read_fits(data),
            FileType::Flv => formats::misc::read_flv(data),
            FileType::Mxf => formats::misc::read_mxf(data).or_else(|_| Ok(Vec::new())),
            FileType::Swf => formats::misc::read_swf(data),
            FileType::Hdr => formats::misc::read_hdr(data),
            FileType::DjVu => formats::djvu::read_djvu(data),
            FileType::Xcf => formats::gimp::read_xcf(data),
            FileType::Mie => formats::mie::read_mie(data),
            FileType::Lfp => formats::lytro::read_lfp(data),
            // FileType::Miff dispatched via string extension below
            FileType::Fpf => formats::flir_fpf::read_fpf(data),
            FileType::Flif => formats::misc::read_flif(data),
            FileType::Bpg => formats::misc::read_bpg(data),
            FileType::Pcx => formats::misc::read_pcx(data),
            FileType::Pict => formats::misc::read_pict(data),
            FileType::Mpeg => formats::mpeg::read_mpeg(data),
            FileType::M2ts => formats::misc::read_m2ts(data, self.options.extract_embedded),
            FileType::Gzip => formats::misc::read_gzip(data),
            FileType::Rar => formats::misc::read_rar(data),
            FileType::SevenZ => formats::misc::read_7z(data),
            FileType::Dss => formats::misc::read_dss(data),
            FileType::Moi => formats::misc::read_moi(data),
            FileType::MacOs => formats::misc::read_macos(data),
            FileType::Json => formats::misc::read_json(data),
            // New formats
            FileType::Pgf => formats::pgf::read_pgf(data),
            FileType::Xisf => formats::xisf::read_xisf(data),
            FileType::Torrent => formats::torrent::read_torrent(data),
            FileType::Mobi => formats::palm::read_palm(data),
            FileType::Psp => formats::psp::read_psp(data),
            FileType::SonyPmp => formats::sony_pmp::read_sony_pmp(data),
            FileType::Audible => formats::audible::read_audible(data),
            FileType::Exr => formats::openexr::read_openexr(data),
            // New formats
            FileType::Plist => {
                if data.starts_with(b"bplist") {
                    formats::plist::read_binary_plist_tags(data)
                } else {
                    formats::plist::read_xml_plist(data)
                }
            }
            FileType::Aae => {
                if data.starts_with(b"bplist") {
                    formats::plist::read_binary_plist_tags(data)
                } else {
                    formats::plist::read_aae_plist(data)
                }
            }
            FileType::KyoceraRaw => formats::misc::read_kyocera_raw(data),
            FileType::PortableFloatMap => formats::misc::read_pfm(data),
            FileType::Ods
            | FileType::Odt
            | FileType::Odp
            | FileType::Odg
            | FileType::Odf
            | FileType::Odb
            | FileType::Odi
            | FileType::Odc => formats::zip::read_zip(data),
            FileType::Lif => formats::misc::read_lif(data),
            FileType::Rwz => formats::misc::read_rawzor(data),
            FileType::Jxr => formats::misc::read_jxr(data),
            _ => Err(Error::UnsupportedFileType(format!("{}", file_type))),
        }
    }

    /// Fallback: try to read file based on extension for formats without magic detection.
    fn process_by_extension(&self, data: &[u8], path: &Path) -> Result<Vec<Tag>> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        match ext.as_str() {
            "ppm" | "pgm" | "pbm" => formats::misc::read_ppm(data),
            "pfm" => {
                // PFM can be Portable Float Map or Printer Font Metrics
                if data.len() >= 3 && data[0] == b'P' && (data[1] == b'f' || data[1] == b'F') {
                    formats::misc::read_ppm(data)
                } else {
                    Ok(Vec::new()) // Printer Font Metrics
                }
            }
            "json" => formats::misc::read_json(data),
            "svg" => formats::misc::read_svg(data),
            "ram" => formats::misc::read_ram(data).or_else(|_| Ok(Vec::new())),
            "txt" | "log" | "igc" => Ok(compute_text_tags(data, false)),
            "csv" => Ok(compute_text_tags(data, true)),
            "url" => formats::lnk::read_url(data).or_else(|_| Ok(Vec::new())),
            "lnk" => formats::lnk::read_lnk(data).or_else(|_| Ok(Vec::new())),
            "gpx" | "kml" | "xml" | "inx" => formats::xmp_file::read_xmp(data),
            "plist" => {
                if data.starts_with(b"bplist") {
                    formats::plist::read_binary_plist_tags(data).or_else(|_| Ok(Vec::new()))
                } else {
                    formats::plist::read_xml_plist(data).or_else(|_| Ok(Vec::new()))
                }
            }
            "aae" => {
                if data.starts_with(b"bplist") {
                    formats::plist::read_binary_plist_tags(data).or_else(|_| Ok(Vec::new()))
                } else {
                    formats::plist::read_aae_plist(data).or_else(|_| Ok(Vec::new()))
                }
            }
            "vcf" | "ics" | "vcard" => {
                let s = crate::encoding::decode_utf8_or_latin1(&data[..data.len().min(100)]);
                if s.contains("BEGIN:VCALENDAR") {
                    formats::vcard::read_ics(data).or_else(|_| Ok(Vec::new()))
                } else {
                    formats::vcard::read_vcf(data).or_else(|_| Ok(Vec::new()))
                }
            }
            "xcf" => Ok(Vec::new()), // GIMP
            "vrd" => formats::canon_vrd::read_vrd(data).or_else(|_| Ok(Vec::new())),
            "dr4" => formats::canon_vrd::read_dr4(data).or_else(|_| Ok(Vec::new())),
            "indd" | "indt" => Ok(Vec::new()), // InDesign
            "x3f" => formats::sigma_raw::read_x3f(data).or_else(|_| Ok(Vec::new())),
            "mie" => Ok(Vec::new()), // MIE
            "exr" => Ok(Vec::new()), // OpenEXR
            "wpg" => formats::misc::read_wpg(data).or_else(|_| Ok(Vec::new())),
            "moi" => formats::misc::read_moi(data).or_else(|_| Ok(Vec::new())),
            "macos" => formats::misc::read_macos(data).or_else(|_| Ok(Vec::new())),
            "dpx" => formats::dpx::read_dpx(data).or_else(|_| Ok(Vec::new())),
            "r3d" => formats::red::read_r3d(data).or_else(|_| Ok(Vec::new())),
            "tnef" => formats::tnef::read_tnef(data).or_else(|_| Ok(Vec::new())),
            "ppt" | "fpx" => formats::flashpix::read_fpx(data).or_else(|_| Ok(Vec::new())),
            "fpf" => formats::flir_fpf::read_fpf(data).or_else(|_| Ok(Vec::new())),
            "itc" => formats::misc::read_itc(data).or_else(|_| Ok(Vec::new())),
            "mpg" | "mpeg" | "m1v" | "m2v" | "mpv" => {
                formats::mpeg::read_mpeg(data).or_else(|_| Ok(Vec::new()))
            }
            "dv" => formats::dv::read_dv(data, data.len() as u64).or_else(|_| Ok(Vec::new())),
            "czi" => formats::misc::read_czi(data).or_else(|_| Ok(Vec::new())),
            "miff" => formats::miff::read_miff(data).or_else(|_| Ok(Vec::new())),
            "lfp" | "mrc" | "dss" | "mobi" | "psp" | "pgf" | "raw" | "pmp" | "torrent" | "xisf"
            | "mxf" | "dfont" => Ok(Vec::new()),
            "iso" => formats::iso::read_iso(data).or_else(|_| Ok(Vec::new())),
            "afm" => formats::font::read_afm(data).or_else(|_| Ok(Vec::new())),
            "pfa" => formats::font::read_pfa(data).or_else(|_| Ok(Vec::new())),
            "pfb" => formats::font::read_pfb(data).or_else(|_| Ok(Vec::new())),
            _ => Err(Error::UnsupportedFileType(ext)),
        }
    }
}

impl Default for ExifTool {
    fn default() -> Self {
        Self::new()
    }
}

/// Detect OpenDocument file type by reading the `mimetype` entry from a ZIP.
/// Returns None if not an OpenDocument file.
fn detect_opendocument_type(data: &[u8]) -> Option<FileType> {
    // OpenDocument ZIPs have "mimetype" as the FIRST local file entry (uncompressed)
    if data.len() < 30 || data[0..4] != [0x50, 0x4B, 0x03, 0x04] {
        return None;
    }
    let compression = u16::from_le_bytes([data[8], data[9]]);
    let compressed_size = u32::from_le_bytes([data[18], data[19], data[20], data[21]]) as usize;
    let name_len = u16::from_le_bytes([data[26], data[27]]) as usize;
    let extra_len = u16::from_le_bytes([data[28], data[29]]) as usize;
    let name_start = 30;
    if name_start + name_len > data.len() {
        return None;
    }
    let filename = std::str::from_utf8(&data[name_start..name_start + name_len]).unwrap_or("");
    if filename != "mimetype" || compression != 0 {
        return None;
    }
    let content_start = name_start + name_len + extra_len;
    let content_end = (content_start + compressed_size).min(data.len());
    if content_start >= content_end {
        return None;
    }
    let mime = std::str::from_utf8(&data[content_start..content_end])
        .unwrap_or("")
        .trim();
    match mime {
        "application/vnd.oasis.opendocument.spreadsheet" => Some(FileType::Ods),
        "application/vnd.oasis.opendocument.text" => Some(FileType::Odt),
        "application/vnd.oasis.opendocument.presentation" => Some(FileType::Odp),
        "application/vnd.oasis.opendocument.graphics" => Some(FileType::Odg),
        "application/vnd.oasis.opendocument.formula" => Some(FileType::Odf),
        "application/vnd.oasis.opendocument.database" => Some(FileType::Odb),
        "application/vnd.oasis.opendocument.image" => Some(FileType::Odi),
        "application/vnd.oasis.opendocument.chart" => Some(FileType::Odc),
        _ => None,
    }
}

/// Detect the file type of a file at the given path.
pub fn get_file_type<P: AsRef<Path>>(path: P) -> Result<FileType> {
    let path = path.as_ref();
    let mut file = fs::File::open(path).map_err(Error::Io)?;
    let mut header = [0u8; 256];
    use std::io::Read;
    let n = file.read(&mut header).map_err(Error::Io)?;

    if let Some(ft) = file_type::detect_from_magic(&header[..n]) {
        return Ok(ft);
    }

    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        if let Some(ft) = file_type::detect_from_extension(ext) {
            return Ok(ft);
        }
    }

    Err(Error::UnsupportedFileType("unknown".into()))
}

/// Classification of EXIF tags into IFD groups.
enum ExifIfdGroup {
    Ifd0,
    ExifIfd,
    Gps,
}

/// Determine which IFD a tag belongs to based on its ID.
fn classify_exif_tag(tag_id: u16) -> ExifIfdGroup {
    match tag_id {
        // ExifIFD tags
        0x829A..=0x829D | 0x8822..=0x8827 | 0x8830 | 0x9000..=0x9292 | 0xA000..=0xA435 => {
            ExifIfdGroup::ExifIfd
        }
        // GPS tags
        0x0000..=0x001F if tag_id <= 0x001F => ExifIfdGroup::Gps,
        // Everything else → IFD0
        _ => ExifIfdGroup::Ifd0,
    }
}

/// Extract existing EXIF entries from a JPEG file's APP1 segment.
fn extract_existing_exif_entries(
    jpeg_data: &[u8],
    target_bo: ByteOrderMark,
) -> Vec<exif_writer::IfdEntry> {
    let mut entries = Vec::new();

    // Find EXIF APP1 segment
    let mut pos = 2; // Skip SOI
    while pos + 4 <= jpeg_data.len() {
        if jpeg_data[pos] != 0xFF {
            pos += 1;
            continue;
        }
        let marker = jpeg_data[pos + 1];
        pos += 2;

        if marker == 0xDA || marker == 0xD9 {
            break; // SOS or EOI
        }
        if marker == 0xFF || marker == 0x00 || marker == 0xD8 || (0xD0..=0xD7).contains(&marker) {
            continue;
        }

        if pos + 2 > jpeg_data.len() {
            break;
        }
        let seg_len = u16::from_be_bytes([jpeg_data[pos], jpeg_data[pos + 1]]) as usize;
        if seg_len < 2 || pos + seg_len > jpeg_data.len() {
            break;
        }

        let seg_data = &jpeg_data[pos + 2..pos + seg_len];

        // EXIF APP1
        if marker == 0xE1 && seg_data.len() > 14 && seg_data.starts_with(b"Exif\0\0") {
            let tiff_data = &seg_data[6..];
            extract_ifd_entries(tiff_data, target_bo, &mut entries);
            break;
        }

        pos += seg_len;
    }

    entries
}

/// Extract IFD entries from TIFF data, re-encoding values in the target byte order.
fn extract_ifd_entries(
    tiff_data: &[u8],
    target_bo: ByteOrderMark,
    entries: &mut Vec<exif_writer::IfdEntry>,
) {
    use crate::metadata::exif::parse_tiff_header;

    let header = match parse_tiff_header(tiff_data) {
        Ok(h) => h,
        Err(_) => return,
    };

    let src_bo = header.byte_order;

    // Read IFD0
    read_ifd_for_merge(
        tiff_data,
        header.ifd0_offset as usize,
        src_bo,
        target_bo,
        entries,
    );

    // Find ExifIFD and GPS pointers
    let ifd0_offset = header.ifd0_offset as usize;
    if ifd0_offset + 2 > tiff_data.len() {
        return;
    }
    let count = read_u16_bo(tiff_data, ifd0_offset, src_bo) as usize;
    for i in 0..count {
        let eoff = ifd0_offset + 2 + i * 12;
        if eoff + 12 > tiff_data.len() {
            break;
        }
        let tag = read_u16_bo(tiff_data, eoff, src_bo);
        let value_off = read_u32_bo(tiff_data, eoff + 8, src_bo) as usize;

        match tag {
            0x8769 => read_ifd_for_merge(tiff_data, value_off, src_bo, target_bo, entries),
            0x8825 => read_ifd_for_merge(tiff_data, value_off, src_bo, target_bo, entries),
            _ => {}
        }
    }
}

/// Read a single IFD and extract entries for merge.
fn read_ifd_for_merge(
    data: &[u8],
    offset: usize,
    src_bo: ByteOrderMark,
    target_bo: ByteOrderMark,
    entries: &mut Vec<exif_writer::IfdEntry>,
) {
    if offset + 2 > data.len() {
        return;
    }
    let count = read_u16_bo(data, offset, src_bo) as usize;

    for i in 0..count {
        let eoff = offset + 2 + i * 12;
        if eoff + 12 > data.len() {
            break;
        }

        let tag = read_u16_bo(data, eoff, src_bo);
        let dtype = read_u16_bo(data, eoff + 2, src_bo);
        let count_val = read_u32_bo(data, eoff + 4, src_bo);

        // Skip sub-IFD pointers and MakerNote
        if tag == 0x8769 || tag == 0x8825 || tag == 0xA005 || tag == 0x927C {
            continue;
        }

        let type_size = match dtype {
            1 | 2 | 6 | 7 => 1usize,
            3 | 8 => 2,
            4 | 9 | 11 | 13 => 4,
            5 | 10 | 12 => 8,
            _ => continue,
        };

        let total_size = type_size * count_val as usize;
        let raw_data = if total_size <= 4 {
            data[eoff + 8..eoff + 12].to_vec()
        } else {
            let voff = read_u32_bo(data, eoff + 8, src_bo) as usize;
            if voff + total_size > data.len() {
                continue;
            }
            data[voff..voff + total_size].to_vec()
        };

        // Re-encode multi-byte values if byte orders differ
        let final_data = if src_bo != target_bo && type_size > 1 {
            reencode_bytes(&raw_data, dtype, count_val as usize, src_bo, target_bo)
        } else {
            raw_data[..total_size].to_vec()
        };

        let format = match dtype {
            1 => exif_writer::ExifFormat::Byte,
            2 => exif_writer::ExifFormat::Ascii,
            3 => exif_writer::ExifFormat::Short,
            4 => exif_writer::ExifFormat::Long,
            5 => exif_writer::ExifFormat::Rational,
            6 => exif_writer::ExifFormat::SByte,
            7 => exif_writer::ExifFormat::Undefined,
            8 => exif_writer::ExifFormat::SShort,
            9 => exif_writer::ExifFormat::SLong,
            10 => exif_writer::ExifFormat::SRational,
            11 => exif_writer::ExifFormat::Float,
            12 => exif_writer::ExifFormat::Double,
            _ => continue,
        };

        entries.push(exif_writer::IfdEntry {
            tag,
            format,
            data: final_data,
        });
    }
}

/// Re-encode multi-byte values when converting between byte orders.
fn reencode_bytes(
    data: &[u8],
    dtype: u16,
    count: usize,
    src_bo: ByteOrderMark,
    dst_bo: ByteOrderMark,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    match dtype {
        3 | 8 => {
            // 16-bit
            for i in 0..count {
                let v = read_u16_bo(data, i * 2, src_bo);
                match dst_bo {
                    ByteOrderMark::LittleEndian => out.extend_from_slice(&v.to_le_bytes()),
                    ByteOrderMark::BigEndian => out.extend_from_slice(&v.to_be_bytes()),
                }
            }
        }
        4 | 9 | 11 | 13 => {
            // 32-bit
            for i in 0..count {
                let v = read_u32_bo(data, i * 4, src_bo);
                match dst_bo {
                    ByteOrderMark::LittleEndian => out.extend_from_slice(&v.to_le_bytes()),
                    ByteOrderMark::BigEndian => out.extend_from_slice(&v.to_be_bytes()),
                }
            }
        }
        5 | 10 => {
            // Rational (two 32-bit)
            for i in 0..count {
                let n = read_u32_bo(data, i * 8, src_bo);
                let d = read_u32_bo(data, i * 8 + 4, src_bo);
                match dst_bo {
                    ByteOrderMark::LittleEndian => {
                        out.extend_from_slice(&n.to_le_bytes());
                        out.extend_from_slice(&d.to_le_bytes());
                    }
                    ByteOrderMark::BigEndian => {
                        out.extend_from_slice(&n.to_be_bytes());
                        out.extend_from_slice(&d.to_be_bytes());
                    }
                }
            }
        }
        12 => {
            // 64-bit double
            for i in 0..count {
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&data[i * 8..i * 8 + 8]);
                if src_bo != dst_bo {
                    bytes.reverse();
                }
                out.extend_from_slice(&bytes);
            }
        }
        _ => out.extend_from_slice(data),
    }
    out
}

fn read_u16_bo(data: &[u8], offset: usize, bo: ByteOrderMark) -> u16 {
    if offset + 2 > data.len() {
        return 0;
    }
    match bo {
        ByteOrderMark::LittleEndian => u16::from_le_bytes([data[offset], data[offset + 1]]),
        ByteOrderMark::BigEndian => u16::from_be_bytes([data[offset], data[offset + 1]]),
    }
}

fn read_u32_bo(data: &[u8], offset: usize, bo: ByteOrderMark) -> u32 {
    if offset + 4 > data.len() {
        return 0;
    }
    match bo {
        ByteOrderMark::LittleEndian => u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]),
        ByteOrderMark::BigEndian => u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]),
    }
}

/// Map tag name to numeric EXIF tag ID.
fn tag_name_to_id(name: &str) -> Option<u16> {
    encode_exif_tag(name, "", "", ByteOrderMark::BigEndian).map(|(id, _, _)| id)
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

fn unix_to_datetime(secs: i64) -> String {
    let days = secs / 86400;
    let time = secs % 86400;
    let h = time / 3600;
    let m = (time % 3600) / 60;
    let s = time % 60;
    let mut y = 1970i32;
    let mut rem = days;
    loop {
        let dy = if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
            366
        } else {
            365
        };
        if rem < dy {
            break;
        }
        rem -= dy;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let months = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut mo = 1;
    for &dm in &months {
        if rem < dm {
            break;
        }
        rem -= dm;
        mo += 1;
    }
    format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
        y,
        mo,
        rem + 1,
        h,
        m,
        s
    )
}

fn format_file_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} bytes", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} kB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

/// Check if a tag name is typically XMP.
fn is_xmp_tag(tag: &str) -> bool {
    matches!(
        tag.to_lowercase().as_str(),
        "title"
            | "description"
            | "subject"
            | "creator"
            | "rights"
            | "keywords"
            | "rating"
            | "label"
            | "hierarchicalsubject"
    )
}

/// Encode an EXIF tag value to binary.
/// Returns (tag_id, format, encoded_data) or None if tag is unknown.
fn encode_exif_tag(
    tag_name: &str,
    value: &str,
    _group: &str,
    bo: ByteOrderMark,
) -> Option<(u16, exif_writer::ExifFormat, Vec<u8>)> {
    let tag_lower = tag_name.to_lowercase();

    // Map common tag names to EXIF tag IDs and formats
    let (tag_id, format): (u16, exif_writer::ExifFormat) = match tag_lower.as_str() {
        // IFD0 string tags
        "imagedescription" => (0x010E, exif_writer::ExifFormat::Ascii),
        "make" => (0x010F, exif_writer::ExifFormat::Ascii),
        "model" => (0x0110, exif_writer::ExifFormat::Ascii),
        "software" => (0x0131, exif_writer::ExifFormat::Ascii),
        "modifydate" | "datetime" => (0x0132, exif_writer::ExifFormat::Ascii),
        "artist" => (0x013B, exif_writer::ExifFormat::Ascii),
        "copyright" => (0x8298, exif_writer::ExifFormat::Ascii),
        // IFD0 numeric tags
        "orientation" => (0x0112, exif_writer::ExifFormat::Short),
        "xresolution" => (0x011A, exif_writer::ExifFormat::Rational),
        "yresolution" => (0x011B, exif_writer::ExifFormat::Rational),
        "resolutionunit" => (0x0128, exif_writer::ExifFormat::Short),
        // ExifIFD tags
        "datetimeoriginal" => (0x9003, exif_writer::ExifFormat::Ascii),
        "createdate" | "datetimedigitized" => (0x9004, exif_writer::ExifFormat::Ascii),
        "usercomment" => (0x9286, exif_writer::ExifFormat::Undefined),
        "imageuniqueid" => (0xA420, exif_writer::ExifFormat::Ascii),
        "ownername" | "cameraownername" => (0xA430, exif_writer::ExifFormat::Ascii),
        "serialnumber" | "bodyserialnumber" => (0xA431, exif_writer::ExifFormat::Ascii),
        "lensmake" => (0xA433, exif_writer::ExifFormat::Ascii),
        "lensmodel" => (0xA434, exif_writer::ExifFormat::Ascii),
        "lensserialnumber" => (0xA435, exif_writer::ExifFormat::Ascii),
        _ => return None,
    };

    let encoded = match format {
        exif_writer::ExifFormat::Ascii => exif_writer::encode_ascii(value),
        exif_writer::ExifFormat::Short => {
            let v: u16 = value.parse().ok()?;
            exif_writer::encode_u16(v, bo)
        }
        exif_writer::ExifFormat::Long => {
            let v: u32 = value.parse().ok()?;
            exif_writer::encode_u32(v, bo)
        }
        exif_writer::ExifFormat::Rational => {
            // Parse "N/D" or just "N"
            if let Some(slash) = value.find('/') {
                let num: u32 = value[..slash].trim().parse().ok()?;
                let den: u32 = value[slash + 1..].trim().parse().ok()?;
                exif_writer::encode_urational(num, den, bo)
            } else if let Ok(v) = value.parse::<f64>() {
                // Convert float to rational
                let den = 10000u32;
                let num = (v * den as f64).round() as u32;
                exif_writer::encode_urational(num, den, bo)
            } else {
                return None;
            }
        }
        exif_writer::ExifFormat::Undefined => {
            // UserComment: 8 bytes charset + data
            let mut data = vec![0x41, 0x53, 0x43, 0x49, 0x49, 0x00, 0x00, 0x00]; // "ASCII\0\0\0"
            data.extend_from_slice(value.as_bytes());
            data
        }
        _ => return None,
    };

    Some((tag_id, format, encoded))
}

/// Compute text file tags (from Perl Text.pm).
fn compute_text_tags(data: &[u8], is_csv: bool) -> Vec<Tag> {
    let mut tags = Vec::new();
    let mk = |name: &str, val: String| Tag {
        id: crate::tag::TagId::Text(name.into()),
        name: name.into(),
        description: name.into(),
        group: crate::tag::TagGroup {
            family0: "File".into(),
            family1: "File".into(),
            family2: "Other".into(),
        },
        raw_value: Value::String(val.clone()),
        print_value: val,
        priority: 0,
    };

    // Detect encoding and BOM
    let is_ascii = data.iter().all(|&b| b < 128);
    let has_utf8_bom = data.starts_with(&[0xEF, 0xBB, 0xBF]);
    let has_utf16le_bom =
        data.starts_with(&[0xFF, 0xFE]) && !data.starts_with(&[0xFF, 0xFE, 0x00, 0x00]);
    let has_utf16be_bom = data.starts_with(&[0xFE, 0xFF]);
    let has_utf32le_bom = data.starts_with(&[0xFF, 0xFE, 0x00, 0x00]);
    let has_utf32be_bom = data.starts_with(&[0x00, 0x00, 0xFE, 0xFF]);

    // Detect if file has weird non-text control characters (like multi-byte unicode without BOM)
    let has_weird_ctrl = data.iter().any(|&b| {
        (b <= 0x06) || (0x0e..=0x1a).contains(&b) || (0x1c..=0x1f).contains(&b) || b == 0x7f
    });

    let (encoding, is_bom, is_utf16) = if has_utf32le_bom {
        ("utf-32le", true, false)
    } else if has_utf32be_bom {
        ("utf-32be", true, false)
    } else if has_utf16le_bom {
        ("utf-16le", true, true)
    } else if has_utf16be_bom {
        ("utf-16be", true, true)
    } else if has_weird_ctrl {
        // Not a text file (has binary-like control chars but no recognized multi-byte marker)
        return tags;
    } else if is_ascii {
        ("us-ascii", false, false)
    } else {
        // Check UTF-8
        let is_valid_utf8 = std::str::from_utf8(data).is_ok();
        if is_valid_utf8 {
            if has_utf8_bom {
                ("utf-8", true, false)
            } else {
                // Check if it has high bytes suggesting iso-8859-1 vs utf-8
                // Perl's IsUTF8: returns >0 if valid UTF-8 with multi-byte, 0 if ASCII, <0 if invalid
                // For simplicity: valid UTF-8 without BOM = utf-8
                ("utf-8", false, false)
            }
        } else if !data.iter().any(|&b| (0x80..=0x9f).contains(&b)) {
            ("iso-8859-1", false, false)
        } else {
            ("unknown-8bit", false, false)
        }
    };

    tags.push(mk("MIMEEncoding", encoding.into()));

    if is_bom {
        tags.push(mk("ByteOrderMark", "Yes".into()));
    }

    // Count newlines and detect type
    let has_cr = data.contains(&b'\r');
    let has_lf = data.contains(&b'\n');
    let newline_type = if has_cr && has_lf {
        "Windows CRLF"
    } else if has_lf {
        "Unix LF"
    } else if has_cr {
        "Macintosh CR"
    } else {
        "(none)"
    };
    tags.push(mk("Newlines", newline_type.into()));

    if is_csv {
        // CSV analysis: detect delimiter, quoting, column count, row count
        let text = crate::encoding::decode_utf8_or_latin1(data);
        let mut delim = "";
        let mut quot = "";
        let mut ncols = 1usize;
        let mut nrows = 0usize;

        for line in text.lines() {
            if nrows == 0 {
                // Detect delimiter from first line
                let comma_count = line.matches(',').count();
                let semi_count = line.matches(';').count();
                let tab_count = line.matches('\t').count();
                if comma_count > semi_count && comma_count > tab_count {
                    delim = ",";
                    ncols = comma_count + 1;
                } else if semi_count > tab_count {
                    delim = ";";
                    ncols = semi_count + 1;
                } else if tab_count > 0 {
                    delim = "\t";
                    ncols = tab_count + 1;
                } else {
                    delim = "";
                    ncols = 1;
                }
                // Detect quoting
                if line.contains('"') {
                    quot = "\"";
                } else if line.contains('\'') {
                    quot = "'";
                }
            }
            nrows += 1;
            if nrows >= 1000 {
                break;
            }
        }

        let delim_display = match delim {
            "," => "Comma",
            ";" => "Semicolon",
            "\t" => "Tab",
            _ => "(none)",
        };
        let quot_display = match quot {
            "\"" => "Double quotes",
            "'" => "Single quotes",
            _ => "(none)",
        };

        tags.push(mk("Delimiter", delim_display.into()));
        tags.push(mk("Quoting", quot_display.into()));
        tags.push(mk("ColumnCount", ncols.to_string()));
        if nrows > 0 {
            tags.push(mk("RowCount", nrows.to_string()));
        }
    } else if !is_utf16 {
        // Line count and word count for plain text files (not UTF-16/32)
        let line_count = data.iter().filter(|&&b| b == b'\n').count();
        let line_count = if line_count == 0 && !data.is_empty() {
            1
        } else {
            line_count
        };
        tags.push(mk("LineCount", line_count.to_string()));

        let text = crate::encoding::decode_utf8_or_latin1(data);
        let word_count = text.split_whitespace().count();
        tags.push(mk("WordCount", word_count.to_string()));
    }

    tags
}
