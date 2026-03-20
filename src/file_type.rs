/// Supported file types and their detection logic.
///
/// Mirrors ExifTool's %fileTypeLookup, %magicNumber, and %mimeType.
/// Covers all 150+ formats supported by ExifTool.

/// Known file types that exiftool can process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(non_camel_case_types)]
pub enum FileType {
    // ===== Images - Standard =====
    Jpeg,
    Tiff,
    Png,
    Gif,
    Bmp,
    WebP,
    Heif,
    Avif,
    Psd,
    Jp2,
    J2c,
    Jxl,
    Jxr,
    Flif,
    Bpg,
    Exr,
    Ico,
    Jps,
    // ===== Images - Specialized =====
    DjVu,
    Xcf,
    Pcx,
    Pict,
    Psp,
    Hdr,
    Rwz,
    Btf,
    Mng,
    PhotoCd,
    // ===== Images - RAW =====
    Cr2,
    Cr3,
    Crw,
    Nef,
    Arw,
    Sr2,
    Srf,
    Orf,
    Rw2,
    Dng,
    Raf,
    Pef,
    Dcr,
    Mrw,
    Erf,
    Fff,
    Iiq,
    Rwl,
    Mef,
    Srw,
    X3f,
    Gpr,
    Arq,
    ThreeFR,
    Crm,
    // ===== Video =====
    Mp4,
    QuickTime,
    Avi,
    Mkv,
    WebM,
    Wmv,
    Asf,
    Flv,
    Mxf,
    Czi,
    M2ts,
    Mpeg,
    ThreeGP,
    RealMedia,
    R3d,
    Dvb,
    Lrv,
    Mqv,
    F4v,
    Wtv,
    DvrMs,
    // ===== Audio =====
    Mp3,
    Flac,
    Ogg,
    Wav,
    Aiff,
    Aac,
    Opus,
    Mpc,
    Ape,
    WavPack,
    Ofr,
    Dsf,
    Audible,
    RealAudio,
    Wma,
    M4a,
    Dss,
    // ===== Documents =====
    Pdf,
    PostScript,
    Doc,
    Docx,
    Xls,
    Xlsx,
    Ppt,
    Pptx,
    Numbers,
    Pages,
    Key,
    InDesign,
    Rtf,
    // ===== Archives =====
    Zip,
    Rar,
    SevenZ,
    Gzip,
    // ===== Metadata / Other =====
    Xmp,
    Mie,
    Exv,
    Vrd,
    Icc,
    Html,
    Exe,
    Font,
    Swf,
    Dicom,
    Fits,
    // ===== Newly added =====
    Moi,
    MacOs,
    Json,
    Pcap,
    Pcapng,
    Svg,
    // ===== New formats =====
    Pgf,
    Xisf,
    Torrent,
    Mobi,
    SonyPmp,
    // ===== Additional formats =====
    Plist,
    Aae,
    KyoceraRaw,
    // ===== Portable Float Map =====
    PortableFloatMap,
    // ===== OpenDocument =====
    Ods,
    Odt,
    Odp,
    Odg,
    Odf,
    Odb,
    Odi,
    Odc,
}

/// Indicates the read/write capability for a file type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    Read,
    ReadWrite,
    ReadWriteCreate,
}

impl FileType {
    /// Human-readable file type description.
    pub fn description(self) -> &'static str {
        match self {
            // Standard images
            FileType::Jpeg => "JPEG image",
            FileType::Tiff => "TIFF image",
            FileType::Png => "PNG image",
            FileType::Gif => "GIF image",
            FileType::Bmp => "BMP image",
            FileType::WebP => "WebP image",
            FileType::Heif => "HEIF/HEIC image",
            FileType::Avif => "AVIF image",
            FileType::Psd => "Adobe Photoshop Document",
            FileType::Jp2 => "JPEG 2000 image",
            FileType::J2c => "JPEG 2000 Codestream",
            FileType::Jxl => "JPEG XL image",
            FileType::Jxr => "JPEG XR / HD Photo",
            FileType::Flif => "Free Lossless Image Format",
            FileType::Bpg => "Better Portable Graphics",
            FileType::Exr => "OpenEXR image",
            FileType::Ico => "Windows Icon",
            FileType::Jps => "JPEG Stereo image",
            // Specialized images
            FileType::DjVu => "DjVu document",
            FileType::Xcf => "GIMP image",
            FileType::Pcx => "PCX image",
            FileType::Pict => "Apple PICT",
            FileType::Psp => "Paint Shop Pro image",
            FileType::Pgf => "Progressive Graphics File",
            FileType::Xisf => "PixInsight XISF image",
            FileType::Torrent => "BitTorrent descriptor",
            FileType::Mobi => "Mobipocket Book",
            FileType::SonyPmp => "Sony PMP video",
            FileType::Plist => "PLIST",
            FileType::Aae => "AAE",
            FileType::KyoceraRaw => "Kyocera Contax N RAW",
            FileType::PortableFloatMap => "Portable Float Map",
            FileType::Hdr => "Radiance HDR",
            FileType::Rwz => "Rawzor compressed image",
            FileType::Btf => "BigTIFF image",
            FileType::Mng => "MNG animation",
            FileType::PhotoCd => "Kodak Photo CD",
            // RAW
            FileType::Cr2 => "Canon CR2 RAW",
            FileType::Cr3 => "Canon CR3 RAW",
            FileType::Crw => "Canon CRW RAW",
            FileType::Nef => "Nikon NEF RAW",
            FileType::Arw => "Sony ARW RAW",
            FileType::Sr2 => "Sony SR2 RAW",
            FileType::Srf => "Sony SRF RAW",
            FileType::Orf => "Olympus ORF RAW",
            FileType::Rw2 => "Panasonic RW2 RAW",
            FileType::Dng => "Adobe Digital Negative",
            FileType::Raf => "Fujifilm RAF RAW",
            FileType::Pef => "Pentax PEF RAW",
            FileType::Dcr => "Kodak DCR RAW",
            FileType::Mrw => "Minolta MRW RAW",
            FileType::Erf => "Epson ERF RAW",
            FileType::Fff => "Hasselblad FFF RAW",
            FileType::Iiq => "Phase One IIQ RAW",
            FileType::Rwl => "Leica RWL RAW",
            FileType::Mef => "Mamiya MEF RAW",
            FileType::Srw => "Samsung SRW RAW",
            FileType::X3f => "Sigma X3F RAW",
            FileType::Gpr => "GoPro GPR RAW",
            FileType::Arq => "Sony ARQ RAW",
            FileType::ThreeFR => "Hasselblad 3FR RAW",
            FileType::Crm => "Canon Cinema RAW",
            // Video
            FileType::Mp4 => "MP4 video",
            FileType::QuickTime => "QuickTime video",
            FileType::Avi => "AVI video",
            FileType::Mkv => "Matroska video",
            FileType::WebM => "WebM video",
            FileType::Wmv => "Windows Media Video",
            FileType::Asf => "Advanced Systems Format",
            FileType::Flv => "Flash Video",
            FileType::Mxf => "Material Exchange Format",
            FileType::Czi => "CZI",
            FileType::M2ts => "MPEG-2 Transport Stream",
            FileType::Mpeg => "MPEG video",
            FileType::ThreeGP => "3GPP multimedia",
            FileType::RealMedia => "RealMedia",
            FileType::R3d => "Redcode RAW video",
            FileType::Dvb => "Digital Video Broadcasting",
            FileType::Lrv => "GoPro Low-Res Video",
            FileType::Mqv => "Sony Movie",
            FileType::F4v => "Adobe Flash Video",
            FileType::Wtv => "Windows Recorded TV",
            FileType::DvrMs => "Microsoft DVR",
            // Audio
            FileType::Mp3 => "MP3 audio",
            FileType::Flac => "FLAC audio",
            FileType::Ogg => "Ogg Vorbis audio",
            FileType::Wav => "WAV audio",
            FileType::Aiff => "AIFF",
            FileType::Aac => "AAC audio",
            FileType::Opus => "Opus audio",
            FileType::Mpc => "Musepack audio",
            FileType::Ape => "Monkey's Audio",
            FileType::WavPack => "WavPack audio",
            FileType::Ofr => "OptimFROG audio",
            FileType::Dsf => "DSD Stream File",
            FileType::Audible => "Audible audiobook",
            FileType::RealAudio => "RealAudio",
            FileType::Wma => "Windows Media Audio",
            FileType::M4a => "MPEG-4 Audio",
            FileType::Dss => "DSS",
            // Documents
            FileType::Pdf => "PDF document",
            FileType::PostScript => "PostScript",
            FileType::Doc => "Microsoft Word (legacy)",
            FileType::Docx => "Microsoft Word",
            FileType::Xls => "Microsoft Excel (legacy)",
            FileType::Xlsx => "Microsoft Excel",
            FileType::Ppt => "Microsoft PowerPoint (legacy)",
            FileType::Pptx => "Microsoft PowerPoint",
            FileType::Numbers => "Apple Numbers",
            FileType::Pages => "Apple Pages",
            FileType::Key => "Apple Keynote",
            FileType::InDesign => "Adobe InDesign",
            FileType::Rtf => "Rich Text Format",
            // Archives
            FileType::Zip => "ZIP archive",
            FileType::Rar => "RAR archive",
            FileType::SevenZ => "7-Zip archive",
            FileType::Gzip => "GZIP",
            // Metadata / Other
            FileType::Xmp => "XMP sidecar",
            FileType::Mie => "MIE metadata",
            FileType::Exv => "Exiv2 metadata",
            FileType::Vrd => "Canon VRD recipe",
            FileType::Icc => "ICC color profile",
            FileType::Html => "HTML document",
            FileType::Exe => "Windows executable",
            FileType::Font => "Font file",
            FileType::Swf => "Shockwave Flash",
            FileType::Dicom => "DICOM medical image",
            FileType::Fits => "FITS astronomical image",
            FileType::Moi => "MOI",
            FileType::MacOs => "MacOS",
            FileType::Json => "JSON",
            FileType::Pcap => "PCAP",
            FileType::Pcapng => "PCAPNG",
            FileType::Svg => "SVG",
            FileType::Pgf => "PGF",
            FileType::Xisf => "XISF",
            FileType::Torrent => "Torrent",
            FileType::Mobi => "MOBI",
            FileType::SonyPmp => "PMP",
            FileType::Plist => "PLIST",
            FileType::Aae => "AAE",
            FileType::KyoceraRaw => "KyoceraRaw",
            FileType::PortableFloatMap => "PFM",
            FileType::Ods => "ODS",
            FileType::Odt => "ODT",
            FileType::Odp => "ODP",
            FileType::Odg => "ODG",
            FileType::Odf => "ODF",
            FileType::Odb => "ODB",
            FileType::Odi => "ODI",
            FileType::Odc => "ODC",
        }
    }

    /// MIME type for this file type.
    pub fn mime_type(self) -> &'static str {
        match self {
            FileType::Jpeg => "image/jpeg",
            FileType::Tiff | FileType::Btf => "image/tiff",
            FileType::Png => "image/png",
            FileType::Gif => "image/gif",
            FileType::Bmp => "image/bmp",
            FileType::WebP => "image/webp",
            FileType::Heif => "image/heif",
            FileType::Avif => "image/avif",
            FileType::Psd => "image/vnd.adobe.photoshop",
            FileType::Jp2 => "image/jp2",
            FileType::J2c => "image/x-j2c",
            FileType::Jxl => "image/jxl",
            FileType::Jxr => "image/jxr",
            FileType::Flif => "image/flif",
            FileType::Bpg => "image/bpg",
            FileType::Exr => "image/x-exr",
            FileType::Ico => "image/x-icon",
            FileType::Jps => "image/x-jps",
            FileType::DjVu => "image/vnd.djvu",
            FileType::Xcf => "image/x-xcf",
            FileType::Pcx => "image/x-pcx",
            FileType::Pict => "image/x-pict",
            FileType::Psp => "image/x-psp",
            FileType::Hdr => "image/vnd.radiance",
            FileType::Rwz => "image/x-rawzor",
            FileType::Mng => "video/x-mng",
            FileType::PhotoCd => "image/x-photo-cd",
            // RAW → use specific MIME where available
            FileType::Cr2 => "image/x-canon-cr2",
            FileType::Cr3 | FileType::Crm => "image/x-canon-cr3",
            FileType::Crw => "image/x-canon-crw",
            FileType::Nef => "image/x-nikon-nef",
            FileType::Arw | FileType::Arq => "image/x-sony-arw",
            FileType::Sr2 => "image/x-sony-sr2",
            FileType::Srf => "image/x-sony-srf",
            FileType::Orf => "image/x-olympus-orf",
            FileType::Rw2 => "image/x-panasonic-rw2",
            FileType::Dng | FileType::Gpr => "image/x-adobe-dng",
            FileType::Raf => "image/x-fuji-raf",
            FileType::Pef => "image/x-pentax-pef",
            FileType::Dcr => "image/x-kodak-dcr",
            FileType::Mrw => "image/x-minolta-mrw",
            FileType::Erf => "image/x-epson-erf",
            FileType::Fff | FileType::ThreeFR => "image/x-hasselblad-fff",
            FileType::Iiq => "image/x-phaseone-iiq",
            FileType::Rwl => "image/x-leica-rwl",
            FileType::Mef => "image/x-mamiya-mef",
            FileType::Srw => "image/x-samsung-srw",
            FileType::X3f => "image/x-sigma-x3f",
            // Video
            FileType::Mp4 | FileType::F4v => "video/mp4",
            FileType::QuickTime | FileType::Mqv => "video/quicktime",
            FileType::Avi => "video/x-msvideo",
            FileType::Mkv => "video/x-matroska",
            FileType::WebM => "video/webm",
            FileType::Wmv => "video/x-ms-wmv",
            FileType::Asf => "video/x-ms-asf",
            FileType::Flv => "video/x-flv",
            FileType::Mxf => "application/mxf",
            FileType::Czi => "image/czi",
            FileType::M2ts => "video/mp2t",
            FileType::Mpeg => "video/mpeg",
            FileType::ThreeGP => "video/3gpp",
            FileType::RealMedia => "application/vnd.rn-realmedia",
            FileType::R3d => "video/x-red-r3d",
            FileType::Dvb => "video/dvb",
            FileType::Lrv => "video/mp4",
            FileType::Wtv => "video/x-ms-wtv",
            FileType::DvrMs => "video/x-ms-dvr",
            // Audio
            FileType::Mp3 => "audio/mpeg",
            FileType::Flac => "audio/flac",
            FileType::Ogg | FileType::Opus => "audio/ogg",
            FileType::Wav => "audio/wav",
            FileType::Aiff => "audio/x-aiff",
            FileType::Aac => "audio/aac",
            FileType::Mpc => "audio/x-musepack",
            FileType::Ape => "audio/x-ape",
            FileType::WavPack => "audio/x-wavpack",
            FileType::Ofr => "audio/x-ofr",
            FileType::Dsf => "audio/dsf",
            FileType::Audible => "audio/x-pn-audibleaudio",
            FileType::RealAudio => "audio/x-pn-realaudio",
            FileType::Wma => "audio/x-ms-wma",
            FileType::M4a => "audio/mp4",
            FileType::Dss => "audio/x-dss",
            // Documents
            FileType::Pdf => "application/pdf",
            FileType::PostScript => "application/postscript",
            FileType::Doc => "application/msword",
            FileType::Docx => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            FileType::Xls => "application/vnd.ms-excel",
            FileType::Xlsx => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            FileType::Ppt => "application/vnd.ms-powerpoint",
            FileType::Pptx => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            FileType::Numbers => "application/x-iwork-numbers-sffnumbers",
            FileType::Pages => "application/x-iwork-pages-sffpages",
            FileType::Key => "application/x-iwork-keynote-sffkey",
            FileType::InDesign => "application/x-indesign",
            FileType::Rtf => "application/rtf",
            // Archives
            FileType::Zip => "application/zip",
            FileType::Rar => "application/x-rar-compressed",
            FileType::SevenZ => "application/x-7z-compressed",
            FileType::Gzip => "application/x-gzip",
            // Metadata / Other
            FileType::Xmp => "application/rdf+xml",
            FileType::Mie => "application/x-mie",
            FileType::Exv => "application/x-exv",
            FileType::Vrd => "application/x-canon-vrd",
            FileType::Icc => "application/vnd.icc.profile",
            FileType::Html => "text/html",
            FileType::Exe => "application/x-dosexec",
            FileType::Font => "font/sfnt",
            FileType::Swf => "application/x-shockwave-flash",
            FileType::Dicom => "application/dicom",
            FileType::Fits => "application/fits",
            FileType::Moi => "application/octet-stream",
            FileType::MacOs => "application/unknown",
            FileType::Json => "application/json",
            FileType::Pcap => "application/vnd.tcpdump.pcap",
            FileType::Pcapng => "application/vnd.tcpdump.pcap",
            FileType::Svg => "image/svg+xml",
            FileType::Pgf => "image/pgf",
            FileType::Xisf => "application/xisf",
            FileType::Torrent => "application/x-bittorrent",
            FileType::Mobi => "application/x-mobipocket-ebook",
            FileType::SonyPmp => "image/x-sony-pmp",
            FileType::Plist => "application/x-plist",
            FileType::Aae => "application/vnd.apple.photos",
            FileType::KyoceraRaw => "image/x-raw",
            FileType::PortableFloatMap => "image/x-pfm",
            FileType::Ods => "application/vnd.oasis.opendocument.spreadsheet",
            FileType::Odt => "application/vnd.oasis.opendocument.text",
            FileType::Odp => "application/vnd.oasis.opendocument.presentation",
            FileType::Odg => "application/vnd.oasis.opendocument.graphics",
            FileType::Odf => "application/vnd.oasis.opendocument.formula",
            FileType::Odb => "application/vnd.oasis.opendocument.database",
            FileType::Odi => "application/vnd.oasis.opendocument.image",
            FileType::Odc => "application/vnd.oasis.opendocument.chart",
        }
    }

    /// Common file extensions for this type.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            FileType::Jpeg => &["jpg", "jpeg", "jpe", "jif", "jfif"],
            FileType::Tiff => &["tif", "tiff"],
            FileType::Png => &["png"],
            FileType::Gif => &["gif"],
            FileType::Bmp => &["bmp", "dib"],
            FileType::WebP => &["webp"],
            FileType::Heif => &["heif", "heic", "hif"],
            FileType::Avif => &["avif"],
            FileType::Psd => &["psd", "psb", "psdt"],
            FileType::Jp2 => &["jp2", "jpf", "jpm", "jpx", "jph"],
            FileType::J2c => &["j2c", "j2k", "jpc"],
            FileType::Jxl => &["jxl"],
            FileType::Jxr => &["jxr", "hdp", "wdp"],
            FileType::Flif => &["flif"],
            FileType::Bpg => &["bpg"],
            FileType::Exr => &["exr"],
            FileType::Ico => &["ico", "cur"],
            FileType::Jps => &["jps"],
            FileType::DjVu => &["djvu", "djv"],
            FileType::Xcf => &["xcf"],
            FileType::Pcx => &["pcx"],
            FileType::Pict => &["pict", "pct"],
            FileType::Psp => &["psp", "pspimage"],
            FileType::Hdr => &["hdr"],
            FileType::Rwz => &["rwz"],
            FileType::Btf => &["btf"],
            FileType::Mng => &["mng", "jng"],
            FileType::PhotoCd => &["pcd"],
            // RAW
            FileType::Cr2 => &["cr2"],
            FileType::Cr3 => &["cr3"],
            FileType::Crw => &["crw", "ciff"],
            FileType::Nef => &["nef", "nrw"],
            FileType::Arw => &["arw"],
            FileType::Sr2 => &["sr2"],
            FileType::Srf => &["srf"],
            FileType::Orf => &["orf", "ori"],
            FileType::Rw2 => &["rw2"],
            FileType::Dng => &["dng"],
            FileType::Raf => &["raf"],
            FileType::Pef => &["pef"],
            FileType::Dcr => &["dcr"],
            FileType::Mrw => &["mrw"],
            FileType::Erf => &["erf"],
            FileType::Fff => &["fff"],
            FileType::Iiq => &["iiq"],
            FileType::Rwl => &["rwl"],
            FileType::Mef => &["mef"],
            FileType::Srw => &["srw"],
            FileType::X3f => &["x3f"],
            FileType::Gpr => &["gpr"],
            FileType::Arq => &["arq"],
            FileType::ThreeFR => &["3fr"],
            FileType::Crm => &["crm"],
            // Video
            FileType::Mp4 => &["mp4", "m4v"],
            FileType::QuickTime => &["mov", "qt"],
            FileType::Avi => &["avi"],
            FileType::Mkv => &["mkv", "mks"],
            FileType::WebM => &["webm"],
            FileType::Wmv => &["wmv"],
            FileType::Asf => &["asf"],
            FileType::Flv => &["flv"],
            FileType::Mxf => &["mxf"],
            FileType::Czi => &["czi"],
            FileType::M2ts => &["m2ts", "mts", "m2t", "ts"],
            FileType::Mpeg => &["mpg", "mpeg", "m2v", "mpv"],
            FileType::ThreeGP => &["3gp", "3gpp", "3g2", "3gp2"],
            FileType::RealMedia => &["rm", "rv", "rmvb"],
            FileType::R3d => &["r3d"],
            FileType::Dvb => &["dvb"],
            FileType::Lrv => &["lrv", "lrf"],
            FileType::Mqv => &["mqv"],
            FileType::F4v => &["f4v", "f4a", "f4b", "f4p"],
            FileType::Wtv => &["wtv"],
            FileType::DvrMs => &["dvr-ms"],
            // Audio
            FileType::Mp3 => &["mp3"],
            FileType::Flac => &["flac"],
            FileType::Ogg => &["ogg", "oga", "ogv"],
            FileType::Wav => &["wav"],
            FileType::Aiff => &["aiff", "aif", "aifc"],
            FileType::Aac => &["aac"],
            FileType::Opus => &["opus"],
            FileType::Mpc => &["mpc"],
            FileType::Ape => &["ape"],
            FileType::WavPack => &["wv", "wvp"],
            FileType::Ofr => &["ofr"],
            FileType::Dsf => &["dsf"],
            FileType::Audible => &["aa", "aax"],
            FileType::RealAudio => &["ra"],
            FileType::Wma => &["wma"],
            FileType::M4a => &["m4a", "m4b", "m4p"],
            FileType::Dss => &["dss"],
            // Documents
            FileType::Pdf => &["pdf"],
            FileType::PostScript => &["ps", "eps", "epsf"],
            FileType::Doc => &["doc", "dot"],
            FileType::Docx => &["docx", "docm"],
            FileType::Xls => &["xls", "xlt"],
            FileType::Xlsx => &["xlsx", "xlsm", "xlsb"],
            FileType::Ppt => &["ppt", "pps", "pot"],
            FileType::Pptx => &["pptx", "pptm"],
            FileType::Numbers => &["numbers", "nmbtemplate"],
            FileType::Pages => &["pages"],
            FileType::Key => &["key", "kth"],
            FileType::InDesign => &["ind", "indd", "indt"],
            FileType::Rtf => &["rtf"],
            // Archives
            FileType::Zip => &["zip"],
            FileType::Rar => &["rar"],
            FileType::SevenZ => &["7z"],
            FileType::Gzip => &["gz", "gzip"],
            // Metadata / Other
            FileType::Xmp => &["xmp", "inx", "xml"],
            FileType::Mie => &["mie"],
            FileType::Exv => &["exv"],
            FileType::Vrd => &["vrd", "dr4"],
            FileType::Icc => &["icc", "icm"],
            FileType::Html => &["html", "htm", "xhtml", "svg"],
            FileType::Exe => &["exe", "dll", "elf", "so", "dylib", "a", "macho", "o"],
            FileType::Font => &["ttf", "otf", "woff", "woff2", "ttc", "dfont", "afm", "pfa", "pfb"],
            FileType::Swf => &["swf"],
            FileType::Dicom => &["dcm"],
            FileType::Fits => &["fits", "fit", "fts"],
            FileType::Moi => &["moi"],
            FileType::MacOs => &["macos"],
            FileType::Json => &["json"],
            FileType::Pcap => &["pcap", "cap"],
            FileType::Pcapng => &["pcapng", "ntar"],
            FileType::Svg => &["svg"],
            FileType::Pgf => &["pgf"],
            FileType::Xisf => &["xisf"],
            FileType::Torrent => &["torrent"],
            FileType::Mobi => &["mobi", "azw", "azw3"],
            FileType::SonyPmp => &["pmp"],
            FileType::Plist => &["plist"],
            FileType::Aae => &["aae"],
            FileType::KyoceraRaw => &["raw"],
            FileType::PortableFloatMap => &["pfm"],
            FileType::Ods => &["ods"],
            FileType::Odt => &["odt"],
            FileType::Odp => &["odp"],
            FileType::Odg => &["odg"],
            FileType::Odf => &["odf"],
            FileType::Odb => &["odb"],
            FileType::Odi => &["odi"],
            FileType::Odc => &["odc"],
        }
    }

    /// Read/Write/Create support level.
    pub fn support(self) -> Support {
        match self {
            // R/W/C
            FileType::Xmp | FileType::Mie | FileType::Exv => Support::ReadWriteCreate,
            // R/W
            FileType::Jpeg
            | FileType::Tiff
            | FileType::Png
            | FileType::Gif
            | FileType::WebP
            | FileType::Heif
            | FileType::Avif
            | FileType::Psd
            | FileType::Jp2
            | FileType::Jxl
            | FileType::Jxr
            | FileType::Flif
            | FileType::Cr2
            | FileType::Cr3
            | FileType::Crw
            | FileType::Nef
            | FileType::Arw
            | FileType::Arq
            | FileType::Sr2
            | FileType::Orf
            | FileType::Rw2
            | FileType::Dng
            | FileType::Raf
            | FileType::Pef
            | FileType::Erf
            | FileType::Fff
            | FileType::Iiq
            | FileType::Rwl
            | FileType::Mef
            | FileType::Srw
            | FileType::X3f
            | FileType::Gpr
            | FileType::Crm
            | FileType::Mp4
            | FileType::QuickTime
            | FileType::ThreeGP
            | FileType::Dvb
            | FileType::Lrv
            | FileType::Mqv
            | FileType::F4v
            | FileType::Pdf
            | FileType::PostScript
            | FileType::InDesign
            | FileType::Vrd
            | FileType::Audible => Support::ReadWrite,
            // R only
            _ => Support::Read,
        }
    }

    /// Returns an iterator over all file types.
    pub fn all() -> &'static [FileType] {
        ALL_FILE_TYPES
    }
}

static ALL_FILE_TYPES: &[FileType] = &[
    // Images - Standard
    FileType::Jpeg, FileType::Tiff, FileType::Png, FileType::Gif, FileType::Bmp,
    FileType::WebP, FileType::Heif, FileType::Avif, FileType::Psd, FileType::Jp2,
    FileType::J2c, FileType::Jxl, FileType::Jxr, FileType::Flif, FileType::Bpg,
    FileType::Exr, FileType::Ico, FileType::Jps,
    // Images - Specialized
    FileType::DjVu, FileType::Xcf, FileType::Pcx, FileType::Pict, FileType::Psp,
    FileType::Hdr, FileType::Rwz, FileType::Btf, FileType::Mng, FileType::PhotoCd,
    // RAW
    FileType::Cr2, FileType::Cr3, FileType::Crw, FileType::Nef, FileType::Arw,
    FileType::Sr2, FileType::Srf, FileType::Orf, FileType::Rw2, FileType::Dng,
    FileType::Raf, FileType::Pef, FileType::Dcr, FileType::Mrw, FileType::Erf,
    FileType::Fff, FileType::Iiq, FileType::Rwl, FileType::Mef, FileType::Srw,
    FileType::X3f, FileType::Gpr, FileType::Arq, FileType::ThreeFR, FileType::Crm,
    // Video
    FileType::Mp4, FileType::QuickTime, FileType::Avi, FileType::Mkv, FileType::WebM,
    FileType::Wmv, FileType::Asf, FileType::Flv, FileType::Mxf, FileType::M2ts,
    FileType::Mpeg, FileType::ThreeGP, FileType::RealMedia, FileType::R3d,
    FileType::Dvb, FileType::Lrv, FileType::Mqv, FileType::F4v, FileType::Wtv,
    FileType::DvrMs,
    // Audio
    FileType::Mp3, FileType::Flac, FileType::Ogg, FileType::Wav, FileType::Aiff,
    FileType::Aac, FileType::Opus, FileType::Mpc, FileType::Ape, FileType::WavPack,
    FileType::Ofr, FileType::Dsf, FileType::Audible, FileType::RealAudio,
    FileType::Wma, FileType::M4a, FileType::Dss,
    // Documents
    FileType::Pdf, FileType::PostScript, FileType::Doc, FileType::Docx,
    FileType::Xls, FileType::Xlsx, FileType::Ppt, FileType::Pptx,
    FileType::Numbers, FileType::Pages, FileType::Key,
    FileType::InDesign, FileType::Rtf,
    // Archives
    FileType::Zip, FileType::Rar, FileType::SevenZ, FileType::Gzip,
    // Metadata / Other
    FileType::Xmp, FileType::Mie, FileType::Exv, FileType::Vrd, FileType::Icc,
    FileType::Html, FileType::Exe, FileType::Font, FileType::Swf,
    FileType::Dicom, FileType::Fits,
    FileType::Moi, FileType::MacOs, FileType::Json,
    FileType::Pcap, FileType::Pcapng,
    FileType::Svg,
    FileType::Pgf, FileType::Xisf, FileType::Torrent, FileType::Mobi, FileType::SonyPmp,
    FileType::Plist, FileType::Aae, FileType::KyoceraRaw,
    FileType::PortableFloatMap,
    // OpenDocument
    FileType::Ods, FileType::Odt, FileType::Odp, FileType::Odg,
    FileType::Odf, FileType::Odb, FileType::Odi, FileType::Odc,
];

/// Detect file type from magic bytes (first 64+ bytes of a file).
pub fn detect_from_magic(header: &[u8]) -> Option<FileType> {
    if header.len() < 4 {
        return None;
    }

    // ===== Images =====

    // JPEG: FF D8 FF
    if header.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(FileType::Jpeg);
    }

    // PNG: 89 50 4E 47 0D 0A 1A 0A
    if header.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some(FileType::Png);
    }

    // GIF: "GIF87a" or "GIF89a"
    if header.starts_with(b"GIF8") && header.len() >= 6 && (header[4] == b'7' || header[4] == b'9') {
        return Some(FileType::Gif);
    }

    // TIFF / TIFF-based RAW: "II" or "MM" + magic 42
    if header.len() >= 4 {
        let is_le = header[0] == b'I' && header[1] == b'I' && header[2] == 0x2A && header[3] == 0x00;
        let is_be = header[0] == b'M' && header[1] == b'M' && header[2] == 0x00 && header[3] == 0x2A;
        if is_le || is_be {
            // CR2: "II" + "CR" at offset 8
            if header.len() >= 10 && is_le && header[8] == b'C' && header[9] == b'R' {
                return Some(FileType::Cr2);
            }
            // IIQ: "IIII" (LE) or "MMMM" (BE) at offset 8
            if header.len() >= 12 && is_le && &header[8..12] == b"IIII" {
                return Some(FileType::Iiq);
            }
            if header.len() >= 12 && is_be && &header[8..12] == b"MMMM" {
                return Some(FileType::Iiq);
            }
            // ORF: "IIRO" or "IIRS" (Olympus)
            if header.len() >= 4 && is_le && header[0] == b'I' && header[1] == b'I' {
                if header.len() >= 8 {
                    // Check for ORF signature at specific offsets (Olympus uses standard TIFF with specific patterns)
                    // For now, fall through to generic TIFF and detect by extension
                }
            }
            // BigTIFF: magic 43 instead of 42
            // (handled below)
            return Some(FileType::Tiff);
        }
        // BigTIFF: "II" + 0x2B or "MM" + 0x002B
        let is_btf_le = header[0] == b'I' && header[1] == b'I' && header[2] == 0x2B && header[3] == 0x00;
        let is_btf_be = header[0] == b'M' && header[1] == b'M' && header[2] == 0x00 && header[3] == 0x2B;
        if is_btf_le || is_btf_be {
            return Some(FileType::Btf);
        }
    }

    // BMP: "BM"
    if header.starts_with(b"BM") && header.len() >= 6 {
        return Some(FileType::Bmp);
    }

    // RIFF container: WebP, AVI, WAV
    if header.len() >= 12 && header.starts_with(b"RIFF") {
        match &header[8..12] {
            b"WEBP" => return Some(FileType::WebP),
            b"AVI " => return Some(FileType::Avi),
            b"WAVE" => return Some(FileType::Wav),
            _ => {}
        }
    }

    // PSD: "8BPS"
    if header.starts_with(b"8BPS") {
        return Some(FileType::Psd);
    }

    // JPEG 2000: 00 00 00 0C 6A 50 20 20 (jp2 signature box)
    if header.len() >= 12 && header.starts_with(&[0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20]) {
        return Some(FileType::Jp2);
    }

    // JPEG 2000 codestream: FF 4F FF 51
    if header.starts_with(&[0xFF, 0x4F, 0xFF, 0x51]) {
        return Some(FileType::J2c);
    }

    // JPEG XL: FF 0A (bare codestream) or 00 00 00 0C 4A 58 4C 20 (container)
    if header.len() >= 2 && header[0] == 0xFF && header[1] == 0x0A {
        return Some(FileType::Jxl);
    }
    if header.len() >= 12 && header.starts_with(&[0x00, 0x00, 0x00, 0x0C, 0x4A, 0x58, 0x4C, 0x20]) {
        return Some(FileType::Jxl);
    }

    // FLIF: "FLIF"
    if header.starts_with(b"FLIF") {
        return Some(FileType::Flif);
    }

    // BPG: 0x425047FB
    if header.starts_with(&[0x42, 0x50, 0x47, 0xFB]) {
        return Some(FileType::Bpg);
    }

    // OpenEXR: 76 2F 31 01
    if header.starts_with(&[0x76, 0x2F, 0x31, 0x01]) {
        return Some(FileType::Exr);
    }

    // ICO: 00 00 01 00 (icon) or 00 00 02 00 (cursor)
    if header.len() >= 4 && header[0] == 0 && header[1] == 0 && (header[2] == 1 || header[2] == 2) && header[3] == 0 {
        return Some(FileType::Ico);
    }

    // DjVu: "AT&TFORM"
    if header.len() >= 8 && header.starts_with(b"AT&TFORM") {
        return Some(FileType::DjVu);
    }

    // GIMP XCF: "gimp xcf"
    if header.starts_with(b"gimp xcf") {
        return Some(FileType::Xcf);
    }

    // MNG: 8A 4D 4E 47 0D 0A 1A 0A
    if header.starts_with(&[0x8A, 0x4D, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some(FileType::Mng);
    }

    // JNG: 8B 4A 4E 47 0D 0A 1A 0A
    if header.starts_with(&[0x8B, 0x4A, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some(FileType::Mng);
    }

    // Radiance HDR: "#?RADIANCE"
    if header.len() >= 10 && header.starts_with(b"#?RADIANCE") {
        return Some(FileType::Hdr);
    }

    // Portable Float Map: "PF\n" (color) or "Pf\n" (grayscale)
    if header.len() >= 3 && header[0] == b'P' && (header[1] == b'F' || header[1] == b'f') && header[2] == b'\n' {
        return Some(FileType::PortableFloatMap);
    }

    // ===== RAW formats with unique magic =====

    // Fujifilm RAF: "FUJIFILMCCD-RAW"
    if header.len() >= 15 && header.starts_with(b"FUJIFILMCCD-RAW") {
        return Some(FileType::Raf);
    }

    // Canon CRW: "II" + 0x1A00 + "HEAPCCDR"
    if header.len() >= 14 && header[0] == b'I' && header[1] == b'I'
        && header[2] == 0x1A && header[3] == 0x00
        && &header[6..14] == b"HEAPCCDR"
    {
        return Some(FileType::Crw);
    }

    // Minolta MRW: 00 4D 52 4D
    if header.starts_with(&[0x00, 0x4D, 0x52, 0x4D]) {
        return Some(FileType::Mrw);
    }

    // Sigma X3F: "FOVb"
    if header.starts_with(b"FOVb") {
        return Some(FileType::X3f);
    }

    // Panasonic RW2: "IIU" (special TIFF variant)
    if header.len() >= 4 && header[0] == b'I' && header[1] == b'I' && header[2] == 0x55 && header[3] == 0x00 {
        return Some(FileType::Rw2);
    }

    // ===== Video / QuickTime container =====

    // QuickTime / MP4 / HEIF / AVIF / CR3: check for ftyp box
    if header.len() >= 12 && &header[4..8] == b"ftyp" {
        let brand = &header[8..12];
        // HEIF/HEIC
        if brand == b"heic" || brand == b"mif1" || brand == b"heim" || brand == b"heis"
            || brand == b"msf1"
        {
            return Some(FileType::Heif);
        }
        // AVIF
        if brand == b"avif" || brand == b"avis" {
            return Some(FileType::Avif);
        }
        // Canon CR3
        if brand == b"crx " {
            return Some(FileType::Cr3);
        }
        // QuickTime
        if brand == b"qt  " {
            return Some(FileType::QuickTime);
        }
        // 3GP
        if brand == b"3gp4" || brand == b"3gp5" || brand == b"3gp6" || brand == b"3g2a" {
            return Some(FileType::ThreeGP);
        }
        // M4A/M4V
        if brand == b"M4A " || brand == b"M4B " || brand == b"M4P " {
            return Some(FileType::M4a);
        }
        if brand == b"M4V " || brand == b"M4VH" || brand == b"M4VP" {
            return Some(FileType::Mp4);
        }
        // F4V
        if brand == b"F4V " || brand == b"F4P " {
            return Some(FileType::F4v);
        }
        // Default ftyp → MP4
        return Some(FileType::Mp4);
    }

    // QuickTime without ftyp: check for common atom types at offset 4
    if header.len() >= 8 {
        let atom_type = &header[4..8];
        if atom_type == b"moov" || atom_type == b"mdat" || atom_type == b"wide"
            || atom_type == b"free" || atom_type == b"pnot" || atom_type == b"skip"
        {
            return Some(FileType::QuickTime);
        }
    }

    // Matroska/WebM: EBML header 0x1A45DFA3
    if header.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return Some(FileType::Mkv);
        // Note: WebM vs MKV distinction requires reading the DocType element inside EBML
    }

    // FLV: "FLV\x01"
    if header.starts_with(b"FLV\x01") {
        return Some(FileType::Flv);
    }

    // ASF/WMV/WMA: 30 26 B2 75 8E 66 CF 11
    if header.len() >= 16 && header.starts_with(&[0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11]) {
        return Some(FileType::Asf);
        // WMV/WMA distinction is done by content/extension
    }

    // MXF: 06 0E 2B 34 02 05 01 01
    if header.len() >= 8 && header.starts_with(&[0x06, 0x0E, 0x2B, 0x34]) {
        return Some(FileType::Mxf);
    }

    // ZISRAW/CZI: "ZISRAWFILE" magic
    if header.len() >= 10 && header.starts_with(b"ZISRAWFILE") {
        return Some(FileType::Czi);
    }

    // ICC Profile: "acsp" at offset 36 (must be before MPEG check which has loose matching)
    if header.len() >= 40 && &header[36..40] == b"acsp" {
        return Some(FileType::Icc);
    }

    // MPEG: 00 00 01 Bx (system header) or 00 00 01 BA (pack start)
    if header.len() >= 4 && header[0] == 0 && header[1] == 0 && header[2] == 1
        && (header[3] == 0xBA || header[3] == 0xBB || (header[3] & 0xF0) == 0xE0)
    {
        return Some(FileType::Mpeg);
    }

    // MPEG-2 TS: 0x47 sync byte every 188 or 192 bytes
    if header.len() >= 1 && header[0] == 0x47 {
        if header.len() >= 376 && header[188] == 0x47 {
            return Some(FileType::M2ts);
        }
        if header.len() >= 384 && header[192] == 0x47 {
            return Some(FileType::M2ts);
        }
    }

    // RealMedia: ".RMF"
    if header.starts_with(b".RMF") {
        return Some(FileType::RealMedia);
    }

    // RED R3D: "RED1" or "RED2"
    if header.starts_with(b"RED1") || header.starts_with(b"RED2") {
        return Some(FileType::R3d);
    }

    // ===== Audio =====

    // MP3: ID3 tag or MPEG sync word
    if header.starts_with(b"ID3") {
        return Some(FileType::Mp3);
    }
    // AAC ADTS: sync=0xFFF (12 bits), then layer bits 13-14 must be 00
    // 0xFF F0 or 0xFF F1 (MPEG-2 AAC) or 0xFF F8/F9 (MPEG-4 AAC with CRC/no-CRC)
    // Distinguishing from MP3: layer bits are 00 for AAC, non-zero for MP3
    if header.len() >= 2 && header[0] == 0xFF
        && (header[1] == 0xF0 || header[1] == 0xF1
            || header[1] == 0xF8 || header[1] == 0xF9)
    {
        return Some(FileType::Aac);
    }
    // MPEG audio sync: 0xFF + 0xE0 mask (after other FF-starting formats)
    if header.len() >= 2 && header[0] == 0xFF && (header[1] & 0xE0) == 0xE0 {
        return Some(FileType::Mp3);
    }

    // FLAC: "fLaC"
    if header.starts_with(b"fLaC") {
        return Some(FileType::Flac);
    }

    // OGG: "OggS"
    if header.starts_with(b"OggS") {
        return Some(FileType::Ogg);
    }

    // AIFF: "FORM" + "AIFF" or "AIFC"
    if header.len() >= 12 && header.starts_with(b"FORM") {
        if &header[8..12] == b"AIFF" || &header[8..12] == b"AIFC" {
            return Some(FileType::Aiff);
        }
    }

    // APE: "MAC "
    if header.starts_with(b"MAC ") {
        return Some(FileType::Ape);
    }

    // Kyocera Contax N RAW: 'ARECOYK' at offset 0x19
    if header.len() >= 0x20 && &header[0x19..0x20] == b"ARECOYK" {
        return Some(FileType::KyoceraRaw);
    }

    // Musepack: "MP+" or "MPCK"
    if header.starts_with(b"MP+") || header.starts_with(b"MPCK") {
        return Some(FileType::Mpc);
    }

    // WavPack: "wvpk"
    if header.starts_with(b"wvpk") {
        return Some(FileType::WavPack);
    }

    // DSD/DSF: "DSD "
    if header.starts_with(b"DSD ") {
        return Some(FileType::Dsf);
    }

    // OptimFROG: "OFR "
    if header.starts_with(b"OFR ") {
        return Some(FileType::Ofr);
    }

    // RealAudio: ".ra\xFD"
    if header.len() >= 4 && header[0] == b'.' && header[1] == b'r' && header[2] == b'a' && header[3] == 0xFD {
        return Some(FileType::RealAudio);
    }

    // DSS (Olympus Digital Speech Standard): "\x02dss" or "\x03ds2"
    if header.len() >= 4
        && (header[0] == 0x02 || header[0] == 0x03)
        && (header[1] == b'd')
        && (header[2] == b's')
        && (header[3] == b's' || header[3] == b'2')
    {
        return Some(FileType::Dss);
    }

    // ===== Documents =====

    // PDF: "%PDF-"
    if header.starts_with(b"%PDF-") {
        return Some(FileType::Pdf);
    }

    // PostScript: "%!PS" or "%!Adobe"
    if header.starts_with(b"%!PS") || header.starts_with(b"%!Adobe") {
        return Some(FileType::PostScript);
    }

    // MS Office legacy (DOC/XLS/PPT): OLE2 compound binary D0 CF 11 E0
    if header.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) {
        return Some(FileType::Doc); // Distinguishing DOC/XLS/PPT requires deeper parsing
    }

    // RTF: "{\rtf"
    if header.starts_with(b"{\\rtf") {
        return Some(FileType::Rtf);
    }

    // InDesign: 06 06 ED F5 D8 1D 46 E5
    if header.len() >= 8 && header.starts_with(&[0x06, 0x06, 0xED, 0xF5, 0xD8, 0x1D, 0x46, 0xE5]) {
        return Some(FileType::InDesign);
    }

    // ===== Archives =====

    // ZIP (and DOCX/XLSX/PPTX/EPUB/etc.): "PK\x03\x04"
    if header.starts_with(&[0x50, 0x4B, 0x03, 0x04]) {
        return Some(FileType::Zip);
        // DOCX/XLSX/PPTX are ZIP files; distinguishing requires checking [Content_Types].xml
    }

    // RAR: "Rar!\x1A\x07"
    if header.len() >= 6 && header.starts_with(b"Rar!\x1A\x07") {
        return Some(FileType::Rar);
    }

    // 7-Zip: "7z\xBC\xAF\x27\x1C"
    if header.len() >= 6 && header.starts_with(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]) {
        return Some(FileType::SevenZ);
    }

    // GZIP: 1F 8B
    if header.len() >= 2 && header[0] == 0x1F && header[1] == 0x8B {
        return Some(FileType::Gzip);
    }

    // PCAPNG: 0x0A 0x0D 0x0D 0x0A (Section Header Block)
    if header.len() >= 4 && header[0] == 0x0A && header[1] == 0x0D && header[2] == 0x0D && header[3] == 0x0A {
        return Some(FileType::Pcapng);
    }

    // PCAP: D4 C3 B2 A1 (little-endian) or A1 B2 C3 D4 (big-endian)
    if header.len() >= 4 && (
        (header[0] == 0xD4 && header[1] == 0xC3 && header[2] == 0xB2 && header[3] == 0xA1) ||
        (header[0] == 0xA1 && header[1] == 0xB2 && header[2] == 0xC3 && header[3] == 0xD4)
    ) {
        return Some(FileType::Pcap);
    }

    // ===== Other =====

    // SWF: "FWS" (uncompressed) or "CWS" (compressed) or "ZWS"
    if header.len() >= 3
        && ((header[0] == b'F' || header[0] == b'C' || header[0] == b'Z')
            && header[1] == b'W'
            && header[2] == b'S')
    {
        return Some(FileType::Swf);
    }

    // DICOM: "DICM" at offset 128
    if header.len() >= 132 && &header[128..132] == b"DICM" {
        return Some(FileType::Dicom);
    }

    // FITS: "SIMPLE  ="
    if header.len() >= 9 && header.starts_with(b"SIMPLE  =") {
        return Some(FileType::Fits);
    }

    // MIE: "~\x10\x04" + version
    if header.len() >= 4 && header[0] == 0x7E && header[1] == 0x10 && header[2] == 0x04 {
        return Some(FileType::Mie);
    }

    // XMP sidecar (starts with XML PI or xpacket)
    if header.starts_with(b"<?xpacket") || header.starts_with(b"<x:xmpmeta") {
        return Some(FileType::Xmp);
    }

    // XML-based formats: look deeper to classify
    if header.starts_with(b"<?xml") || header.starts_with(b"<svg") {
        let preview = &header[..header.len().min(512)];
        if preview.windows(4).any(|w| w == b"<svg") {
            return Some(FileType::Svg);
        }
        if preview.windows(5).any(|w| w == b"<html" || w == b"<HTML") {
            return Some(FileType::Html); // XHTML
        }
        if preview.windows(10).any(|w| w == b"<x:xmpmeta") || preview.windows(9).any(|w| w == b"<?xpacket") {
            return Some(FileType::Xmp);
        }
        if preview.windows(4).any(|w| w == b"<rdf" || w == b"<RDF") {
            return Some(FileType::Xmp);
        }
        // Apple PLIST
        if preview.windows(7).any(|w| w == b"<plist") || preview.windows(20).any(|w| w == b"DTD PLIST") {
            return Some(FileType::Plist);
        }
        // Default XML → XMP (most XML files ExifTool handles contain XMP)
        return Some(FileType::Xmp);
    }

    // HTML
    if header.starts_with(b"<!DOCTYPE html") || header.starts_with(b"<!doctype html")
        || header.starts_with(b"<!DOCTYPE HTML") || header.starts_with(b"<html") || header.starts_with(b"<HTML")
    {
        return Some(FileType::Html);
    }

    // ELF executable: \x7FELF
    if header.starts_with(&[0x7F, b'E', b'L', b'F']) {
        return Some(FileType::Exe);
    }

    // Mach-O 32-bit: FEEDFACE (big) or CEFAEDFE (little)
    if header.starts_with(&[0xFE, 0xED, 0xFA, 0xCE])
        || header.starts_with(&[0xCE, 0xFA, 0xED, 0xFE])
    {
        return Some(FileType::Exe);
    }

    // Mach-O 64-bit: FEEDFACF (big) or CFFAEDFE (little)
    if header.starts_with(&[0xFE, 0xED, 0xFA, 0xCF])
        || header.starts_with(&[0xCF, 0xFA, 0xED, 0xFE])
    {
        return Some(FileType::Exe);
    }

    // Mach-O Universal/Fat binary: CAFEBABE
    if header.starts_with(&[0xCA, 0xFE, 0xBA, 0xBE]) {
        return Some(FileType::Exe);
    }

    // PE executable: "MZ"
    if header.starts_with(b"MZ") {
        return Some(FileType::Exe);
    }

    // TrueType font: 00 01 00 00 or "true" or "typ1"
    if (header.starts_with(&[0x00, 0x01, 0x00, 0x00]) || header.starts_with(b"true") || header.starts_with(b"typ1"))
        && header.len() >= 12
    {
        return Some(FileType::Font);
    }

    // TrueType Collection: "ttcf"
    if header.starts_with(b"ttcf") {
        return Some(FileType::Font);
    }

    // OpenType font: "OTTO"
    if header.starts_with(b"OTTO") {
        return Some(FileType::Font);
    }

    // WOFF: "wOFF"
    if header.starts_with(b"wOFF") {
        return Some(FileType::Font);
    }

    // WOFF2: "wOF2"
    if header.starts_with(b"wOF2") {
        return Some(FileType::Font);
    }

    // MOI: starts with "V6" (camcorder info file)
    if header.len() >= 2 && header[0] == b'V' && header[1] == b'6' {
        return Some(FileType::Moi);
    }

    // PGF: "PGF"
    if header.starts_with(b"PGF") {
        return Some(FileType::Pgf);
    }

    // XISF: "XISF0100"
    if header.starts_with(b"XISF0100") {
        return Some(FileType::Xisf);
    }

    // Paint Shop Pro: "Paint Shop Pro Image File\n\x1a\0\0\0\0\0"
    if header.len() >= 27 && header.starts_with(b"Paint Shop Pro Image File\n\x1a") {
        return Some(FileType::Psp);
    }

    // Sony PMP: magic at offset 8-11 is 00 00 00 7C (header length = 124)
    // and byte 4 is 0x00 (part of file size field)
    if header.len() >= 12
        && header[8] == 0x00 && header[9] == 0x00
        && header[10] == 0x00 && header[11] == 0x7C
    {
        return Some(FileType::SonyPmp);
    }

    None
}

/// Detect file type from file extension.
pub fn detect_from_extension(ext: &str) -> Option<FileType> {
    let ext_lower = ext.to_ascii_lowercase();
    let ext_lower = ext_lower.trim_start_matches('.');

    for &ft in ALL_FILE_TYPES {
        for known_ext in ft.extensions() {
            if ext_lower == *known_ext {
                return Some(ft);
            }
        }
    }

    None
}

impl std::fmt::Display for FileType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.description())
    }
}

impl std::fmt::Display for Support {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Support::Read => write!(f, "R"),
            Support::ReadWrite => write!(f, "R/W"),
            Support::ReadWriteCreate => write!(f, "R/W/C"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_jpeg() {
        let header = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F'];
        assert_eq!(detect_from_magic(&header), Some(FileType::Jpeg));
    }

    #[test]
    fn test_detect_png() {
        let header = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        assert_eq!(detect_from_magic(&header), Some(FileType::Png));
    }

    #[test]
    fn test_detect_tiff_le() {
        let header = [b'I', b'I', 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00];
        assert_eq!(detect_from_magic(&header), Some(FileType::Tiff));
    }

    #[test]
    fn test_detect_tiff_be() {
        let header = [b'M', b'M', 0x00, 0x2A, 0x00, 0x00, 0x00, 0x08];
        assert_eq!(detect_from_magic(&header), Some(FileType::Tiff));
    }

    #[test]
    fn test_detect_cr2() {
        let header = [b'I', b'I', 0x2A, 0x00, 0x10, 0x00, 0x00, 0x00, b'C', b'R'];
        assert_eq!(detect_from_magic(&header), Some(FileType::Cr2));
    }

    #[test]
    fn test_detect_pdf() {
        let header = b"%PDF-1.7 some more data here";
        assert_eq!(detect_from_magic(header), Some(FileType::Pdf));
    }

    #[test]
    fn test_detect_webp() {
        let header = b"RIFF\x00\x00\x00\x00WEBP";
        assert_eq!(detect_from_magic(header), Some(FileType::WebP));
    }

    #[test]
    fn test_detect_heif() {
        let mut header = [0u8; 16];
        header[4..8].copy_from_slice(b"ftyp");
        header[8..12].copy_from_slice(b"heic");
        assert_eq!(detect_from_magic(&header), Some(FileType::Heif));
    }

    #[test]
    fn test_detect_avif() {
        let mut header = [0u8; 16];
        header[4..8].copy_from_slice(b"ftyp");
        header[8..12].copy_from_slice(b"avif");
        assert_eq!(detect_from_magic(&header), Some(FileType::Avif));
    }

    #[test]
    fn test_detect_cr3() {
        let mut header = [0u8; 16];
        header[4..8].copy_from_slice(b"ftyp");
        header[8..12].copy_from_slice(b"crx ");
        assert_eq!(detect_from_magic(&header), Some(FileType::Cr3));
    }

    #[test]
    fn test_detect_flac() {
        assert_eq!(detect_from_magic(b"fLaC\x00\x00"), Some(FileType::Flac));
    }

    #[test]
    fn test_detect_ogg() {
        assert_eq!(detect_from_magic(b"OggS\x00\x02"), Some(FileType::Ogg));
    }

    #[test]
    fn test_detect_mp3_id3() {
        assert_eq!(detect_from_magic(b"ID3\x04\x00"), Some(FileType::Mp3));
    }

    #[test]
    fn test_detect_rar() {
        assert_eq!(detect_from_magic(b"Rar!\x1A\x07\x01\x00"), Some(FileType::Rar));
    }

    #[test]
    fn test_detect_7z() {
        assert_eq!(detect_from_magic(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]), Some(FileType::SevenZ));
    }

    #[test]
    fn test_detect_gzip() {
        assert_eq!(detect_from_magic(&[0x1F, 0x8B, 0x08, 0x00]), Some(FileType::Gzip));
    }

    #[test]
    fn test_detect_raf() {
        assert_eq!(detect_from_magic(b"FUJIFILMCCD-RAW 0201"), Some(FileType::Raf));
    }

    #[test]
    fn test_detect_psd() {
        assert_eq!(detect_from_magic(b"8BPS\x00\x01"), Some(FileType::Psd));
    }

    #[test]
    fn test_detect_from_extension() {
        assert_eq!(detect_from_extension("jpg"), Some(FileType::Jpeg));
        assert_eq!(detect_from_extension(".JPEG"), Some(FileType::Jpeg));
        assert_eq!(detect_from_extension("cr2"), Some(FileType::Cr2));
        assert_eq!(detect_from_extension("cr3"), Some(FileType::Cr3));
        assert_eq!(detect_from_extension("nef"), Some(FileType::Nef));
        assert_eq!(detect_from_extension("arw"), Some(FileType::Arw));
        assert_eq!(detect_from_extension("dng"), Some(FileType::Dng));
        assert_eq!(detect_from_extension("raf"), Some(FileType::Raf));
        assert_eq!(detect_from_extension("mp4"), Some(FileType::Mp4));
        assert_eq!(detect_from_extension("mov"), Some(FileType::QuickTime));
        assert_eq!(detect_from_extension("flac"), Some(FileType::Flac));
        assert_eq!(detect_from_extension("docx"), Some(FileType::Docx));
        assert_eq!(detect_from_extension("xlsx"), Some(FileType::Xlsx));
        assert_eq!(detect_from_extension("3fr"), Some(FileType::ThreeFR));
        assert_eq!(detect_from_extension("xyz"), None);
    }

    #[test]
    fn test_all_types_have_extensions() {
        for &ft in FileType::all() {
            assert!(!ft.extensions().is_empty(), "{:?} has no extensions", ft);
        }
    }

    #[test]
    fn test_all_types_have_mime() {
        for &ft in FileType::all() {
            assert!(!ft.mime_type().is_empty(), "{:?} has no MIME type", ft);
        }
    }

    #[test]
    fn test_total_format_count() {
        assert!(FileType::all().len() >= 100, "Expected 100+ formats, got {}", FileType::all().len());
    }
}
