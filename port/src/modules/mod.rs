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
pub mod apple;
pub mod canon;
pub mod canon_custom;
pub mod canon_raw;
pub mod canon_vrd;
pub mod dji;
pub mod exif;
pub mod exiftool;
pub mod fits;
pub mod fixup;
pub mod flash_pix;
pub mod flir;
pub mod foto_station;
pub mod fuji_film;
pub mod geo_tiff;
pub mod gif;
pub mod gimp;
pub mod go_pro;
pub mod gps;
pub mod hp;
pub mod icc_profile;
pub mod iptc;
pub mod iso;
pub mod itc;
pub mod jpeg;
pub mod jpeg2000;
pub mod json;
pub mod jvc;
pub mod kodak;
pub mod kyocera_raw;
pub mod leaf;
pub mod lytro;
pub mod maker_notes;
pub mod microsoft;
pub mod minolta;
pub mod minolta_raw;
pub mod mrc;
pub mod nikon;
pub mod nikon_capture;
pub mod nikon_settings;
pub mod olympus;
pub mod palm;
pub mod panasonic;
pub mod panasonic_raw;
pub mod pcap;
pub mod pdf;
pub mod pentax;
pub mod pgf;
pub mod phase_one;
pub mod photo_mechanic;
pub mod photoshop;
pub mod plus;
pub mod png;
pub mod ppm;
pub mod print_im;
pub mod psp;
pub mod qualcomm;
pub mod quick_time;
pub mod radiance;
pub mod random_access;
pub mod red;
pub mod ricoh;
pub mod samsung;
pub mod sanyo;
pub mod sigma_raw;
pub mod sony;
pub mod sony_idc;
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
    ("Image::ExifTool::Apple", apple::register),
    ("Image::ExifTool::BMP", bmp::register),
    ("Image::ExifTool::BPG", bpg::register),
    ("Image::ExifTool::Canon", canon::register),
    ("Image::ExifTool::CanonCustom", canon_custom::register),
    ("Image::ExifTool::CanonRaw", canon_raw::register),
    ("Image::ExifTool::CanonVRD", canon_vrd::register),
    ("Image::ExifTool::DJI", dji::register),
    ("Image::ExifTool::Exif", exif::register),
    ("Image::ExifTool::FITS", fits::register),
    ("Image::ExifTool::FLIR", flir::register),
    ("Image::ExifTool::Fixup", fixup::register),
    ("Image::ExifTool::FlashPix", flash_pix::register),
    ("Image::ExifTool::FotoStation", foto_station::register),
    ("Image::ExifTool::FujiFilm", fuji_film::register),
    ("Image::ExifTool::GIF", gif::register),
    ("Image::ExifTool::GIMP", gimp::register),
    ("Image::ExifTool::GPS", gps::register),
    ("Image::ExifTool::GeoTiff", geo_tiff::register),
    ("Image::ExifTool::GoPro", go_pro::register),
    ("Image::ExifTool::HP", hp::register),
    ("Image::ExifTool::ICC_Profile", icc_profile::register),
    ("Image::ExifTool::IPTC", iptc::register),
    ("Image::ExifTool::ISO", iso::register),
    ("Image::ExifTool::ITC", itc::register),
    ("Image::ExifTool::JPEG", jpeg::register),
    ("Image::ExifTool::JSON", json::register),
    ("Image::ExifTool::JVC", jvc::register),
    ("Image::ExifTool::Jpeg2000", jpeg2000::register),
    ("Image::ExifTool::Kodak", kodak::register),
    ("Image::ExifTool::KyoceraRaw", kyocera_raw::register),
    ("Image::ExifTool::Leaf", leaf::register),
    ("Image::ExifTool::Lytro", lytro::register),
    ("Image::ExifTool::MRC", mrc::register),
    ("Image::ExifTool::MakerNotes", maker_notes::register),
    ("Image::ExifTool::Microsoft", microsoft::register),
    ("Image::ExifTool::Minolta", minolta::register),
    ("Image::ExifTool::MinoltaRaw", minolta_raw::register),
    ("Image::ExifTool::Nikon", nikon::register),
    ("Image::ExifTool::NikonCapture", nikon_capture::register),
    ("Image::ExifTool::NikonSettings", nikon_settings::register),
    ("Image::ExifTool::Olympus", olympus::register),
    ("Image::ExifTool::PCAP", pcap::register),
    ("Image::ExifTool::PDF", pdf::register),
    ("Image::ExifTool::PGF", pgf::register),
    ("Image::ExifTool::PLUS", plus::register),
    ("Image::ExifTool::PNG", png::register),
    ("Image::ExifTool::PPM", ppm::register),
    ("Image::ExifTool::PSP", psp::register),
    ("Image::ExifTool::Palm", palm::register),
    ("Image::ExifTool::Panasonic", panasonic::register),
    ("Image::ExifTool::PanasonicRaw", panasonic_raw::register),
    ("Image::ExifTool::Pentax", pentax::register),
    ("Image::ExifTool::PhaseOne", phase_one::register),
    ("Image::ExifTool::PhotoMechanic", photo_mechanic::register),
    ("Image::ExifTool::Photoshop", photoshop::register),
    ("Image::ExifTool::PrintIM", print_im::register),
    ("Image::ExifTool::Qualcomm", qualcomm::register),
    ("Image::ExifTool::QuickTime", quick_time::register),
    ("Image::ExifTool::Radiance", radiance::register),
    ("Image::ExifTool::Red", red::register),
    ("Image::ExifTool::Ricoh", ricoh::register),
    ("Image::ExifTool::Samsung", samsung::register),
    ("Image::ExifTool::Sanyo", sanyo::register),
    ("Image::ExifTool::SigmaRaw", sigma_raw::register),
    ("Image::ExifTool::Sony", sony::register),
    ("Image::ExifTool::SonyIDC", sony_idc::register),
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
