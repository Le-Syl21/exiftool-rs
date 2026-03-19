//! # exiftool
//!
//! A Rust reimplementation of [ExifTool](https://exiftool.org/) for reading, writing,
//! and editing metadata in image, audio, video, and document files.
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
//! ## Supported Formats (30+ readers, 15 writers)
//!
//! **Images**: JPEG, TIFF, PNG, WebP, PSD, BMP, GIF, HEIF/AVIF, ICO
//! **Raw**: CR2, NEF, DNG, ARW, ORF, RAF, RW2, PEF, SR2, X3F, 3FR, ERF
//! **Video**: MP4/MOV, AVI, MKV
//! **Audio**: MP3, FLAC, WAV, OGG
//! **Documents**: PDF

pub mod composite;
pub mod config;
pub mod error;


pub mod geolocation;
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
