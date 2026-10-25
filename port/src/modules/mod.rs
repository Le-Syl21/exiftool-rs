//! The ported modules, one per ExifTool Perl file.

use crate::runtime::Registrar;

pub mod archive_zip;
pub mod brotli;
pub mod core;
pub mod zlib;

pub mod canon;
pub mod exif;
pub mod exiftool;
pub mod fixup;
pub mod gps;
pub mod icc_profile;
pub mod iptc;
pub mod jpeg;
pub mod maker_notes;
pub mod photoshop;
pub mod print_im;
pub mod random_access;
pub mod xmp;
pub mod xmp_struct;

/// Each file's sub registration, by its path as a package name
/// (Image/ExifTool/XMPStruct.pl is Image::ExifTool::XMPStruct).
pub static REGISTRARS: &[(&str, Registrar)] = &[
    ("File::RandomAccess", random_access::register),
    ("Image::ExifTool", exiftool::register),
    ("Image::ExifTool::Canon", canon::register),
    ("Image::ExifTool::Exif", exif::register),
    ("Image::ExifTool::Fixup", fixup::register),
    ("Image::ExifTool::GPS", gps::register),
    ("Image::ExifTool::ICC_Profile", icc_profile::register),
    ("Image::ExifTool::IPTC", iptc::register),
    ("Image::ExifTool::JPEG", jpeg::register),
    ("Image::ExifTool::MakerNotes", maker_notes::register),
    ("Image::ExifTool::Photoshop", photoshop::register),
    ("Image::ExifTool::PrintIM", print_im::register),
    ("Image::ExifTool::XMP", xmp::register),
    ("Image::ExifTool::XMPStruct", xmp_struct::register),
];

/// Modules ported completely: they load from their data dump and their
/// Rust subs only. The others run from their Perl source, with their
/// ported subs replacing the Perl ones.
pub static PORTED: &[&str] = &[];

pub fn is_ported(module: &str) -> bool {
    PORTED.contains(&module)
}
