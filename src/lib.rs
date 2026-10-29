//! A faithful port of Perl ExifTool 13.60.
//!
//! Each module mirrors one Perl file and keeps its sub names, so any function
//! here can be read side by side with the Perl it comes from. See
//! `docs/port/ARCHITECTURE.md`.

pub mod api;
pub mod build_info;
pub mod cli;
pub mod interp;
pub mod modules;
pub mod perl;
pub mod pl;
pub mod runtime;

// The root crate's API (`exiftool_rs::ExifTool`...), at the crate root as
// there, with its modules under their names.
pub use crate::api::{error, exiftool, extension, file_type, get_file_type, image_info, tag, value, Error, ExifTool, FileType, FormatExtension, ImageInfo, NewValue, Options, Result, Tag, TagGroup, TagId, Value, EXIFTOOL_VERSION, VERSION};
