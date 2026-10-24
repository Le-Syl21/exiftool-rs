//! The ported modules, one per ExifTool Perl file.

use crate::runtime::Registrar;

/// Each module's sub registration, by Perl package.
pub static REGISTRARS: &[(&str, Registrar)] = &[];
