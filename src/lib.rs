// Clippy: allow style/complexity lints across the crate.
// These are mostly mechanical patterns inherited from the Perl-to-Rust port.
// TODO: progressively fix and remove these allows.
#![allow(
    clippy::collapsible_if,
    clippy::collapsible_match,
    clippy::collapsible_str_replace,
    clippy::bool_comparison,
    clippy::cmp_owned,
    clippy::double_ended_iterator_last,
    clippy::empty_line_after_doc_comments,
    clippy::doc_lazy_continuation,
    clippy::explicit_counter_loop,
    clippy::field_reassign_with_default,
    clippy::get_first,
    clippy::identity_op,
    clippy::if_same_then_else,
    clippy::implicit_saturating_sub,
    clippy::iter_cloned_collect,
    clippy::len_without_is_empty,
    clippy::len_zero,
    clippy::let_and_return,
    clippy::manual_contains,
    clippy::manual_flatten,
    clippy::manual_pattern_char_comparison,
    clippy::manual_range_contains,
    clippy::manual_range_patterns,
    clippy::manual_strip,
    clippy::map_clone,
    clippy::mixed_case_hex_literals,
    clippy::needless_borrow,
    clippy::needless_else,
    clippy::needless_ifs,
    clippy::needless_late_init,
    clippy::needless_lifetimes,
    clippy::needless_match,
    clippy::needless_range_loop,
    clippy::needless_return,
    clippy::needless_splitn,
    clippy::nonminimal_bool,
    clippy::op_ref,
    clippy::precedence,
    clippy::redundant_closure,
    clippy::redundant_slicing,
    clippy::same_item_push,
    clippy::single_match,
    clippy::too_many_arguments,
    clippy::trim_split_whitespace,
    clippy::unnecessary_cast,
    clippy::unnecessary_map_or,
    clippy::unused_enumerate_index,
    clippy::useless_conversion,
    clippy::useless_format,
    clippy::vec_init_then_push,
    clippy::while_let_loop
)]

//! # exiftool-rs
//!
//! A pure Rust reimplementation of [ExifTool](https://exiftool.org/) for reading, writing,
//! and editing metadata in image, audio, video, and document files.
//!
//! **194/194 test files (100%)** produce identical tag names as Perl ExifTool v13.53.
//! Over 11,600 tags verified across 55+ file formats.
//!
//! ## Quick Start
//!
//! ```no_run
//! use exiftool_rs::ExifTool;
//!
//! let et = ExifTool::new();
//! let tags = et.extract_info("photo.jpg").unwrap();
//! for tag in &tags {
//!     println!("{}: {}", tag.name, tag.print_value);
//! }
//! ```
//!
//! ## One-liner
//!
//! ```no_run
//! let info = exiftool_rs::image_info("photo.jpg").unwrap();
//! println!("Camera: {}", info.get("Model").unwrap_or(&String::new()));
//! ```
//!
//! ## Writing Tags
//!
//! ```no_run
//! use exiftool_rs::ExifTool;
//!
//! let mut et = ExifTool::new();
//! et.set_new_value("Artist", Some("John Doe"));
//! et.write_info("photo.jpg", "photo_out.jpg").unwrap();
//! ```
//!
//! ## Supported Formats (93 readers, 15 writers)
//!
//! **Images**: JPEG, TIFF, PNG, WebP, PSD, BMP, GIF, HEIF/AVIF, ICO, XCF, BPG, MIFF, PGF, PPM, PCX, PICT, JXR, FLIF, MNG, Radiance HDR, OpenEXR, PSP, InDesign
//!
//! **Raw**: CR2, CR3, CRW, CRM, NEF, DNG, ARW, ORF, RAF, RW2, PEF, SR2, X3F, IIQ, 3FR, ERF, MRW, SRW, Rawzor, KyoceraRaw
//!
//! **Video**: MP4/MOV, AVI, MKV, MTS, MPEG, WTV, DV, FLV, SWF, MXF, ASF/WMV, Real
//!
//! **Audio**: MP3, FLAC, WAV, OGG, AAC, AIFF, APE, MPC, WavPack, DSF, Audible, Opus
//!
//! **Documents**: PDF, RTF, HTML, PostScript, DjVu, OpenDocument, TNEF, Font (TTF/OTF/WOFF)
//!
//! **Scientific**: DICOM, MRC, FITS, XISF, DPX, LIF (Leica)
//!
//! **Archives**: ZIP, 7Z, RAR, GZIP, ISO, Torrent
//!
//! **Other**: EXE/ELF/Mach-O, LNK, VCard, ICS, JSON, PLIST, MIE, Lytro LFP, FLIR FPF, CaptureOne EIP, Palm PDB, PCAP
//!
//! **Timed metadata** (`-ee`): freeGPS (dashcams), GoPro GPMF, Google CAMM, NMEA, Kenwood, DJI, Insta360
//!
//! **MakerNotes**: Canon, Nikon, Sony, Pentax, Olympus, Panasonic, Fujifilm, Samsung,
//! Sigma, Casio, Ricoh, Minolta, Apple, Google, FLIR, GE, GoPro

pub mod composite;
pub mod config;
pub mod encoding;
pub mod error;

pub mod exiftool;
pub mod file_type;
pub mod formats;
pub mod geolocation;
pub mod i18n;
pub mod md5;
pub mod metadata;
pub mod tag;
pub mod tags;
pub mod value;
pub mod writer;

// Re-export main types at crate root
pub use crate::error::{Error, Result};
pub use crate::exiftool::{ExifTool, ImageInfo, Options};
pub use crate::file_type::FileType;
pub use crate::tag::{Tag, TagGroup, TagId};
pub use crate::value::Value;

/// Convenience function: extract metadata from a file in one call.
pub fn image_info<P: AsRef<std::path::Path>>(path: P) -> Result<ImageInfo> {
    ExifTool::new().image_info(path)
}

/// Detect the file type of the given file.
pub fn get_file_type<P: AsRef<std::path::Path>>(path: P) -> Result<FileType> {
    crate::exiftool::get_file_type(path)
}

/// Library version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
