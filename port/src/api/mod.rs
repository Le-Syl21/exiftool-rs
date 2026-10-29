//! The high-level Rust API of the root crate (`exiftool_rs`), kept as a
//! facade over the port: `ExifTool`, `Options`, `Tag`, `Value`,
//! `extract_info`, `write_info`... with the same names and signatures, now
//! answered by Image::ExifTool itself (see `docs/port/API.md`).
//!
//! ```no_run
//! use exiftool_port::ExifTool;
//!
//! let et = ExifTool::new();
//! let tags = et.extract_info("photo.jpg").unwrap();
//! for tag in &tags {
//!     println!("{}: {}", tag.name, tag.print_value);
//! }
//! ```

mod engine;
pub mod error;
pub mod exiftool;
pub mod extension;
#[cfg(feature = "extra-writers")]
pub mod extra_writers;
pub mod file_type;
pub mod tag;
pub mod value;

pub use error::{Error, Result};
pub use exiftool::{ExifTool, ImageInfo, NewValue, Options};
pub use extension::FormatExtension;
pub use file_type::FileType;
pub use tag::{Tag, TagGroup, TagId};
pub use value::Value;

/// Convenience function: extract metadata from a file in one call.
pub fn image_info<P: AsRef<std::path::Path>>(path: P) -> Result<ImageInfo> {
    ExifTool::new().image_info(path)
}

/// Detect the file type of the given file.
pub fn get_file_type<P: AsRef<std::path::Path>>(path: P) -> Result<FileType> {
    exiftool::get_file_type(path)
}

/// Library version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The Perl ExifTool release this crate is a port of.
pub const EXIFTOOL_VERSION: &str = "13.59";
