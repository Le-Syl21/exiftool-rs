//! The ported modules, one per ExifTool Perl file.

use crate::runtime::Registrar;

pub mod aac;
pub mod aes;
pub mod afcp;
pub mod aiff;
pub mod ape;
pub mod app12;
pub mod apple;
pub mod archive_zip;
pub mod asf;
pub mod audible;
pub mod big_tiff;
pub mod bmp;
pub mod bpg;
pub mod brotli;
pub mod bunzip2;
pub mod build_tag_lookup;
pub mod bzz;
pub mod canon;
pub mod canon_custom;
pub mod canon_raw;
pub mod canon_vrd;
pub mod capture_one;
pub mod cbor;
pub mod charset;
pub mod core;
pub mod dicom;
pub mod dj_vu;
pub mod dji;
pub mod dng;
pub mod dpx;
pub mod dsf;
pub mod dv;
pub mod exe;
pub mod exif;
pub mod exiftool;
pub mod fits;
pub mod fixup;
pub mod flac;
pub mod flash;
pub mod flash_pix;
pub mod flif;
pub mod flir;
pub mod font;
pub mod foto_station;
pub mod fuji_film;
pub mod garmin;
pub mod gcstring;
pub mod geo_tiff;
pub mod geolocation;
pub mod geotag;
pub mod gif;
pub mod gimp;
pub mod gm;
pub mod go_pro;
pub mod google;
pub mod gps;
pub mod h264;
pub mod hp;
pub mod html;
pub mod html_dump;
pub mod i_work;
pub mod icc_profile;
pub mod ico;
pub mod id3;
pub mod import;
pub mod in_design;
pub mod iptc;
pub mod iso;
pub mod itc;
pub mod jpeg;
pub mod jpeg2000;
pub mod jpeg_digest;
pub mod json;
pub mod jvc;
pub mod kandao;
pub mod kodak;
pub mod kyocera_raw;
pub mod leaf;
pub mod lif;
pub mod ligo_gps;
pub mod lnk;
pub mod lytro;
pub mod lzma;
pub mod m2ts;
pub mod mac_os;
pub mod maker_notes;
pub mod matroska;
pub mod microsoft;
pub mod mie;
pub mod miff;
pub mod minolta;
pub mod minolta_raw;
pub mod misb;
pub mod moi;
pub mod mpc;
pub mod mpeg;
pub mod mpf;
pub mod mrc;
pub mod mwg;
pub mod mxf;
pub mod nikon;
pub mod nikon_capture;
pub mod nikon_settings;
pub mod ogg;
pub mod olympus;
pub mod ooxml;
pub mod open_exr;
pub mod other;
pub mod palm;
pub mod panasonic;
pub mod panasonic_raw;
pub mod parrot;
pub mod pcap;
pub mod pcx;
pub mod pdf;
pub mod pentax;
pub mod pgf;
pub mod phase_one;
pub mod photo_cd;
pub mod photo_mechanic;
pub mod photoshop;
pub mod pict;
pub mod plist;
pub mod plot;
pub mod plus;
pub mod png;
pub mod post_script;
pub mod ppm;
pub mod print_im;
pub mod protobuf;
pub mod psp;
pub mod qualcomm;
pub mod quick_time;
pub mod quick_time_stream;
pub mod radiance;
pub mod random_access;
pub mod rawzor;
pub mod real;
pub mod red;
pub mod ricoh;
pub mod riff;
pub mod rsrc;
pub mod rtf;
pub mod samsung;
pub mod sanyo;
pub mod scalado;
pub mod seven_z;
pub mod shift;
pub mod shortcuts;
pub mod sigma_raw;
pub mod sony;
pub mod sony_idc;
pub mod tag_info_xml;
pub mod tag_lookup;
pub mod text;
pub mod tnef;
pub mod time_piece;
pub mod torrent;
pub mod trailer;
pub mod v_card;
pub mod validate;
pub mod vorbis;
pub mod wav_pack;
pub mod wpg;
pub mod write_canon_raw;
pub mod write_exif;
pub mod write_iptc;
pub mod write_pdf;
pub mod write_photoshop;
pub mod write_png;
pub mod write_post_script;
pub mod write_quick_time;
pub mod write_riff;
pub mod write_xmp;
pub mod writer;
pub mod writer_a;
pub mod writer_b;
pub mod wtv;
pub mod xisf;
pub mod xmp;
pub mod xmp2;
pub mod xmp_struct;
pub mod zip;
pub mod zisraw;
pub mod zlib;

/// Each file's sub registration, by its path as a package name
/// (Image/ExifTool/XMPStruct.pl is Image::ExifTool::XMPStruct).
pub static REGISTRARS: &[(&str, Registrar)] = &[
    ("File::RandomAccess", random_access::register),
    ("Image::ExifTool", exiftool::register),
    ("Image::ExifTool::7Z", seven_z::register),
    ("Image::ExifTool::AAC", aac::register),
    ("Image::ExifTool::AES", aes::register),
    ("Image::ExifTool::AFCP", afcp::register),
    ("Image::ExifTool::AIFF", aiff::register),
    ("Image::ExifTool::APE", ape::register),
    ("Image::ExifTool::APP12", app12::register),
    ("Image::ExifTool::ASF", asf::register),
    ("Image::ExifTool::Apple", apple::register),
    ("Image::ExifTool::Audible", audible::register),
    ("Image::ExifTool::BMP", bmp::register),
    ("Image::ExifTool::BPG", bpg::register),
    ("Image::ExifTool::BZZ", bzz::register),
    ("Image::ExifTool::BigTIFF", big_tiff::register),
    ("Image::ExifTool::BuildTagLookup", build_tag_lookup::register),
    ("Image::ExifTool::CBOR", cbor::register),
    ("Image::ExifTool::Canon", canon::register),
    ("Image::ExifTool::CanonCustom", canon_custom::register),
    ("Image::ExifTool::CanonRaw", canon_raw::register),
    ("Image::ExifTool::CanonVRD", canon_vrd::register),
    ("Image::ExifTool::CaptureOne", capture_one::register),
    ("Image::ExifTool::Charset", charset::register),
    ("Image::ExifTool::DICOM", dicom::register),
    ("Image::ExifTool::DJI", dji::register),
    ("Image::ExifTool::DNG", dng::register),
    ("Image::ExifTool::DPX", dpx::register),
    ("Image::ExifTool::DSF", dsf::register),
    ("Image::ExifTool::DV", dv::register),
    ("Image::ExifTool::DjVu", dj_vu::register),
    ("Image::ExifTool::EXE", exe::register),
    ("Image::ExifTool::Exif", exif::register),
    ("Image::ExifTool::FITS", fits::register),
    ("Image::ExifTool::FLAC", flac::register),
    ("Image::ExifTool::FLIF", flif::register),
    ("Image::ExifTool::FLIR", flir::register),
    ("Image::ExifTool::Fixup", fixup::register),
    ("Image::ExifTool::Flash", flash::register),
    ("Image::ExifTool::FlashPix", flash_pix::register),
    ("Image::ExifTool::Font", font::register),
    ("Image::ExifTool::FotoStation", foto_station::register),
    ("Image::ExifTool::FujiFilm", fuji_film::register),
    ("Image::ExifTool::GIF", gif::register),
    ("Image::ExifTool::GIMP", gimp::register),
    ("Image::ExifTool::GM", gm::register),
    ("Image::ExifTool::GPS", gps::register),
    ("Image::ExifTool::Garmin", garmin::register),
    ("Image::ExifTool::GeoTiff", geo_tiff::register),
    ("Image::ExifTool::Geolocation", geolocation::register),
    ("Image::ExifTool::Geotag", geotag::register),
    ("Image::ExifTool::GoPro", go_pro::register),
    ("Image::ExifTool::Google", google::register),
    ("Image::ExifTool::H264", h264::register),
    ("Image::ExifTool::HP", hp::register),
    ("Image::ExifTool::HTML", html::register),
    ("Image::ExifTool::HtmlDump", html_dump::register),
    ("Image::ExifTool::ICC_Profile", icc_profile::register),
    ("Image::ExifTool::ICO", ico::register),
    ("Image::ExifTool::ID3", id3::register),
    ("Image::ExifTool::IPTC", iptc::register),
    ("Image::ExifTool::ISO", iso::register),
    ("Image::ExifTool::ITC", itc::register),
    ("Image::ExifTool::Import", import::register),
    ("Image::ExifTool::InDesign", in_design::register),
    ("Image::ExifTool::JPEG", jpeg::register),
    ("Image::ExifTool::JPEGDigest", jpeg_digest::register),
    ("Image::ExifTool::JSON", json::register),
    ("Image::ExifTool::JVC", jvc::register),
    ("Image::ExifTool::Jpeg2000", jpeg2000::register),
    ("Image::ExifTool::Kandao", kandao::register),
    ("Image::ExifTool::Kodak", kodak::register),
    ("Image::ExifTool::KyoceraRaw", kyocera_raw::register),
    ("Image::ExifTool::LIF", lif::register),
    ("Image::ExifTool::LNK", lnk::register),
    ("Image::ExifTool::Leaf", leaf::register),
    ("Image::ExifTool::LigoGPS", ligo_gps::register),
    ("Image::ExifTool::Lytro", lytro::register),
    ("Image::ExifTool::M2TS", m2ts::register),
    ("Image::ExifTool::MIE", mie::register),
    ("Image::ExifTool::MIFF", miff::register),
    ("Image::ExifTool::MISB", misb::register),
    ("Image::ExifTool::MOI", moi::register),
    ("Image::ExifTool::MPC", mpc::register),
    ("Image::ExifTool::MPEG", mpeg::register),
    ("Image::ExifTool::MPF", mpf::register),
    ("Image::ExifTool::MRC", mrc::register),
    ("Image::ExifTool::MWG", mwg::register),
    ("Image::ExifTool::MXF", mxf::register),
    ("Image::ExifTool::MacOS", mac_os::register),
    ("Image::ExifTool::MakerNotes", maker_notes::register),
    ("Image::ExifTool::Matroska", matroska::register),
    ("Image::ExifTool::Microsoft", microsoft::register),
    ("Image::ExifTool::Minolta", minolta::register),
    ("Image::ExifTool::MinoltaRaw", minolta_raw::register),
    ("Image::ExifTool::Nikon", nikon::register),
    ("Image::ExifTool::NikonCapture", nikon_capture::register),
    ("Image::ExifTool::NikonSettings", nikon_settings::register),
    ("Image::ExifTool::OOXML", ooxml::register),
    ("Image::ExifTool::Ogg", ogg::register),
    ("Image::ExifTool::Olympus", olympus::register),
    ("Image::ExifTool::OpenEXR", open_exr::register),
    ("Image::ExifTool::Other", other::register),
    ("Image::ExifTool::PCAP", pcap::register),
    ("Image::ExifTool::PCX", pcx::register),
    ("Image::ExifTool::PDF", pdf::register),
    ("Image::ExifTool::PGF", pgf::register),
    ("Image::ExifTool::PICT", pict::register),
    ("Image::ExifTool::PLIST", plist::register),
    ("Image::ExifTool::PLUS", plus::register),
    ("Image::ExifTool::PNG", png::register),
    ("Image::ExifTool::PPM", ppm::register),
    ("Image::ExifTool::PSP", psp::register),
    ("Image::ExifTool::Palm", palm::register),
    ("Image::ExifTool::Panasonic", panasonic::register),
    ("Image::ExifTool::PanasonicRaw", panasonic_raw::register),
    ("Image::ExifTool::Parrot", parrot::register),
    ("Image::ExifTool::Pentax", pentax::register),
    ("Image::ExifTool::PhaseOne", phase_one::register),
    ("Image::ExifTool::PhotoCD", photo_cd::register),
    ("Image::ExifTool::PhotoMechanic", photo_mechanic::register),
    ("Image::ExifTool::Photoshop", photoshop::register),
    ("Image::ExifTool::Plot", plot::register),
    ("Image::ExifTool::PostScript", post_script::register),
    ("Image::ExifTool::PrintIM", print_im::register),
    ("Image::ExifTool::Protobuf", protobuf::register),
    ("Image::ExifTool::Qualcomm", qualcomm::register),
    ("Image::ExifTool::QuickTime", quick_time::register),
    ("Image::ExifTool::QuickTimeStream", quick_time_stream::register),
    ("Image::ExifTool::RIFF", riff::register),
    ("Image::ExifTool::RSRC", rsrc::register),
    ("Image::ExifTool::RTF", rtf::register),
    ("Image::ExifTool::Radiance", radiance::register),
    ("Image::ExifTool::Rawzor", rawzor::register),
    ("Image::ExifTool::Real", real::register),
    ("Image::ExifTool::Red", red::register),
    ("Image::ExifTool::Ricoh", ricoh::register),
    ("Image::ExifTool::Samsung", samsung::register),
    ("Image::ExifTool::Sanyo", sanyo::register),
    ("Image::ExifTool::Scalado", scalado::register),
    ("Image::ExifTool::Shift", shift::register),
    ("Image::ExifTool::Shortcuts", shortcuts::register),
    ("Image::ExifTool::SigmaRaw", sigma_raw::register),
    ("Image::ExifTool::Sony", sony::register),
    ("Image::ExifTool::SonyIDC", sony_idc::register),
    ("Image::ExifTool::TNEF", tnef::register),
    ("Image::ExifTool::TagInfoXML", tag_info_xml::register),
    ("Image::ExifTool::TagLookup", tag_lookup::register),
    ("Image::ExifTool::Text", text::register),
    ("Image::ExifTool::Torrent", torrent::register),
    ("Image::ExifTool::Trailer", trailer::register),
    ("Image::ExifTool::VCard", v_card::register),
    ("Image::ExifTool::Validate", validate::register),
    ("Image::ExifTool::Vorbis", vorbis::register),
    ("Image::ExifTool::WPG", wpg::register),
    ("Image::ExifTool::WTV", wtv::register),
    ("Image::ExifTool::WavPack", wav_pack::register),
    ("Image::ExifTool::WriteCanonRaw", write_canon_raw::register),
    ("Image::ExifTool::WriteExif", write_exif::register),
    ("Image::ExifTool::WriteIPTC", write_iptc::register),
    ("Image::ExifTool::WritePDF", write_pdf::register),
    ("Image::ExifTool::WritePNG", write_png::register),
    ("Image::ExifTool::WritePhotoshop", write_photoshop::register),
    ("Image::ExifTool::WritePostScript", write_post_script::register),
    ("Image::ExifTool::WriteQuickTime", write_quick_time::register),
    ("Image::ExifTool::WriteRIFF", write_riff::register),
    ("Image::ExifTool::WriteXMP", write_xmp::register),
    ("Image::ExifTool::Writer", writer::register),
    ("Image::ExifTool::XISF", xisf::register),
    ("Image::ExifTool::XMP", xmp::register),
    ("Image::ExifTool::XMP2", xmp2::register),
    ("Image::ExifTool::XMPStruct", xmp_struct::register),
    ("Image::ExifTool::ZIP", zip::register),
    ("Image::ExifTool::ZISRAW", zisraw::register),
    ("Image::ExifTool::iWork", i_work::register),
];

/// Modules ported completely: they load from their data dump and their
/// Rust subs only. The others run from their Perl source, with their
/// ported subs replacing the Perl ones.
pub static PORTED: &[&str] = &[];

pub fn is_ported(module: &str) -> bool {
    PORTED.contains(&module)
}
