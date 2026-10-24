//! The ported modules, one per ExifTool Perl file.

pub mod core;

use crate::runtime::Registrar;

/// Each module's sub registration, by Perl package.
pub static REGISTRARS: &[(&str, Registrar)] = &[];

/// Modules ported completely: they load from their data dump and their
/// Rust subs only. The others run from their Perl source, with their
/// ported subs replacing the Perl ones.
pub static PORTED: &[&str] = &[];

pub fn is_ported(module: &str) -> bool {
    PORTED.contains(&module)
}
