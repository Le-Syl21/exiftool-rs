//! # exiftool
//!
//! A Rust implementation of [ExifTool](https://exiftool.org/) for reading metadata
//! from image, audio, video, and document files.
//!
//! ## Quick Start
//!
//! ```no_run
//! use exiftool::ExifTool;
//!
//! let et = ExifTool::new();
//! let info = et.image_info("photo.jpg").unwrap();
//! for (tag, value) in &info {
//!     println!("{}: {}", tag, value);
//! }
//! ```
//!
//! ## Supported Formats
//!
//! Currently supports reading metadata from:
//! - **JPEG** — EXIF, IPTC, XMP
//! - **PNG** — tEXt, iTXt, eXIf chunks
//! - **TIFF** — Full IFD structure (also CR2, NEF, DNG, ARW)
//!
//! More formats will be added incrementally.

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
