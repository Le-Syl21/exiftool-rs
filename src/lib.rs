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
//! ## Supported Formats (55+ readers, 15 writers)
//!
//! **Images**: JPEG, TIFF, PNG, WebP, PSD, BMP, GIF, HEIF/AVIF, ICO, XCF, BPG, MIFF, PGF
//!
//! **Raw**: CR2, CR3, CRW, NEF, DNG, ARW, ORF, RAF, RW2, PEF, SR2, X3F, IIQ, 3FR, ERF, MRW
//!
//! **Video**: MP4/MOV, AVI, MKV, MTS, WTV, DV, FLV, SWF, MXF
//!
//! **Audio**: MP3, FLAC, WAV, OGG, AAC, AIFF, APE, MPC, WavPack, DSF, Audible
//!
//! **Documents**: PDF, RTF, HTML, PostScript, DjVu, OpenDocument, TNEF
//!
//! **Scientific**: DICOM, MRC, FITS, XISF
//!
//! **Other**: EXE/ELF/Mach-O, ZIP/RAR/GZ, ISO, LNK, Torrent, VCard, MIE, Lytro LFP, FLIR FPF, CaptureOne EIP
//!
//! **MakerNotes**: Canon, Nikon, Sony, Pentax, Olympus, Panasonic, Fujifilm, Samsung,
//! Sigma, Casio, Ricoh, Minolta, Apple, Google, FLIR, GE, GoPro

pub mod composite;
pub mod config;
pub mod error;


pub mod geolocation;
pub mod i18n;
pub mod exiftool;
pub mod file_type;
pub mod formats;
pub mod metadata;
pub mod tag;
pub mod tags;
pub mod value;
pub mod writer;
pub mod md5;

// Re-export main types at crate root
pub use crate::error::{Error, Result};
pub use crate::exiftool::{ExifTool, ImageInfo, Options};
pub use crate::file_type::FileType;
pub use crate::tag::{Tag, TagGroup, TagId};
pub use crate::value::Value;

/// Convenience function: extract metadata from a file in one call.
pub fn image_info<P: AsRef<std::path::Path>>(
    path: P,
) -> Result<ImageInfo> {
    ExifTool::new().image_info(path)
}

/// Detect the file type of the given file.
pub fn get_file_type<P: AsRef<std::path::Path>>(path: P) -> Result<FileType> {
    crate::exiftool::get_file_type(path)
}

/// Library version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
