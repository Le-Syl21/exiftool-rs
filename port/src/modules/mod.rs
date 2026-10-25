//! The ported modules, one per ExifTool Perl file.

use crate::runtime::Registrar;

pub mod archive_zip;
pub mod brotli;
pub mod core;
pub mod zlib;

pub mod aac;
pub mod aiff;
pub mod bmp;
pub mod bpg;
pub mod canon;
pub mod exif;
pub mod exiftool;
pub mod fits;
pub mod fixup;
pub mod foto_station;
pub mod geo_tiff;
pub mod gimp;
pub mod gps;
pub mod icc_profile;
pub mod iptc;
pub mod iso;
pub mod itc;
pub mod jpeg;
pub mod json;
pub mod jvc;
pub mod kyocera_raw;
pub mod lytro;
pub mod maker_notes;
pub mod mrc;
pub mod palm;
pub mod pcap;
pub mod pgf;
pub mod photo_mechanic;
pub mod photoshop;
pub mod plus;
pub mod ppm;
pub mod print_im;
pub mod psp;
pub mod radiance;
pub mod random_access;
pub mod red;
pub mod vorbis;
pub mod wpg;
pub mod xisf;
pub mod xmp;
pub mod xmp_struct;

/// Each file's sub registration, by its path as a package name
/// (Image/ExifTool/XMPStruct.pl is Image::ExifTool::XMPStruct).
pub static REGISTRARS: &[(&str, Registrar)] = &[
    ("File::RandomAccess", random_access::register),
    ("Image::ExifTool", exiftool::register),
    ("Image::ExifTool::AAC", aac::register),
    ("Image::ExifTool::AIFF", aiff::register),
    ("Image::ExifTool::BMP", bmp::register),
    ("Image::ExifTool::BPG", bpg::register),
    ("Image::ExifTool::Canon", canon::register),
    ("Image::ExifTool::Exif", exif::register),
    ("Image::ExifTool::FITS", fits::register),
    ("Image::ExifTool::Fixup", fixup::register),
    ("Image::ExifTool::FotoStation", foto_station::register),
    ("Image::ExifTool::GIMP", gimp::register),
    ("Image::ExifTool::GPS", gps::register),
    ("Image::ExifTool::GeoTiff", geo_tiff::register),
    ("Image::ExifTool::ICC_Profile", icc_profile::register),
    ("Image::ExifTool::IPTC", iptc::register),
    ("Image::ExifTool::ISO", iso::register),
    ("Image::ExifTool::ITC", itc::register),
    ("Image::ExifTool::JPEG", jpeg::register),
    ("Image::ExifTool::JSON", json::register),
    ("Image::ExifTool::JVC", jvc::register),
    ("Image::ExifTool::KyoceraRaw", kyocera_raw::register),
    ("Image::ExifTool::Lytro", lytro::register),
    ("Image::ExifTool::MRC", mrc::register),
    ("Image::ExifTool::MakerNotes", maker_notes::register),
    ("Image::ExifTool::PCAP", pcap::register),
    ("Image::ExifTool::PGF", pgf::register),
    ("Image::ExifTool::PLUS", plus::register),
    ("Image::ExifTool::PPM", ppm::register),
    ("Image::ExifTool::PSP", psp::register),
    ("Image::ExifTool::Palm", palm::register),
    ("Image::ExifTool::PhotoMechanic", photo_mechanic::register),
    ("Image::ExifTool::Photoshop", photoshop::register),
    ("Image::ExifTool::PrintIM", print_im::register),
    ("Image::ExifTool::Radiance", radiance::register),
    ("Image::ExifTool::Red", red::register),
    ("Image::ExifTool::Vorbis", vorbis::register),
    ("Image::ExifTool::WPG", wpg::register),
    ("Image::ExifTool::XISF", xisf::register),
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
