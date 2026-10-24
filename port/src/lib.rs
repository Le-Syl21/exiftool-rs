//! A faithful port of Perl ExifTool 13.59.
//!
//! Each module mirrors one Perl file and keeps its sub names, so any function
//! here can be read side by side with the Perl it comes from. See
//! `docs/port/ARCHITECTURE.md`.

pub mod interp;
pub mod modules;
pub mod perl;
pub mod runtime;
