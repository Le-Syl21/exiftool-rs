//! The pieces of Perl that ExifTool's behaviour depends on: the scalar,
//! `sprintf`, `pack`/`unpack`, regular expressions, and the interpreter that
//! evaluates the code strings in ExifTool's tables.

pub mod ops;
pub mod sprintf;
pub mod value;

pub use value::{Number, Value};
