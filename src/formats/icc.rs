//! ICC Color Profile reader.
//!
//! Parses ICC profile header for color space and rendering intent info.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

/// Read an s15Fixed16 value (Get32s / 0x10000) rounding to 5 decimals, matching
/// ExifTool's GetFixed32s ("remove insignificant digits").
fn fixed32s(b: &[u8]) -> f64 {
    let v = i32::from_be_bytes([b[0], b[1], b[2], b[3]]) as f64 / 65536.0;
    let r = if v > 0.0 { 0.5 } else { -0.5 };
    ((v * 1e5 + r) as i64) as f64 / 1e5
}

/// Format three s15Fixed16 XYZ components Perl-style (%.15g, space-joined).
fn xyz_str(b: &[u8]) -> String {
    format!(
        "{} {} {}",
        crate::value::format_g15(fixed32s(&b[0..4])),
        crate::value::format_g15(fixed32s(&b[4..8])),
        crate::value::format_g15(fixed32s(&b[8..12]))
    )
}

pub fn read_icc(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 128 || &data[36..40] != b"acsp" {
        return Err(Error::InvalidData("not an ICC profile".into()));
    }

    let mut tags = Vec::new();

    let preferred_cmm = crate::encoding::decode_utf8_or_latin1(&data[4..8])
        .trim()
        .to_string();
    if !preferred_cmm.is_empty() && preferred_cmm != "\0\0\0\0" {
        let cmm = icc_vendor(&preferred_cmm)
            .map(str::to_string)
            .unwrap_or(preferred_cmm);
        tags.push(mk("ProfileCMMType", "Profile CMM Type", Value::String(cmm)));
    }

    let major = data[8];
    let minor = (data[9] >> 4) & 0x0F;
    let patch = data[9] & 0x0F;
    tags.push(mk(
        "ProfileVersion",
        "Profile Version",
        Value::String(format!("{}.{}.{}", major, minor, patch)),
    ));

    let device_class = crate::encoding::decode_utf8_or_latin1(&data[12..16]).to_string();
    let class_name = match device_class.trim() {
        "scnr" => "Input Device Profile",
        "mntr" => "Display Device Profile",
        "prtr" => "Output Device Profile",
        "link" => "DeviceLink Profile",
        "spac" => "ColorSpace Conversion Profile",
        "abst" => "Abstract Profile",
        "nmcl" => "Named Color Profile",
        _ => &device_class,
    };
    tags.push(mk(
        "ProfileClass",
        "Profile Class",
        Value::String(class_name.to_string()),
    ));

    let color_space = crate::encoding::decode_utf8_or_latin1(&data[16..20])
        .trim()
        .to_string();
    let cs_name = match color_space.as_str() {
        "XYZ" => "XYZ",
        "Lab" => "Lab",
        "Luv" => "Luv",
        "YCbr" => "YCbCr",
        "Yxy" => "Yxy",
        "RGB" => "RGB",
        "GRAY" => "Grayscale",
        "HSV" => "HSV",
        "HLS" => "HLS",
        "CMYK" => "CMYK",
        "CMY" => "CMY",
        _ => &color_space,
    };
    tags.push(mk(
        "ColorSpaceData",
        "Color Space",
        Value::String(cs_name.to_string()),
    ));

    let pcs = crate::encoding::decode_utf8_or_latin1(&data[20..24])
        .trim()
        .to_string();
    tags.push(mk(
        "ProfileConnectionSpace",
        "Connection Space",
        Value::String(pcs),
    ));

    // Creation date (bytes 24-35): year(2), month(2), day(2), hour(2), minute(2), second(2)
    let year = u16::from_be_bytes([data[24], data[25]]);
    let month = u16::from_be_bytes([data[26], data[27]]);
    let day = u16::from_be_bytes([data[28], data[29]]);
    let hour = u16::from_be_bytes([data[30], data[31]]);
    let min = u16::from_be_bytes([data[32], data[33]]);
    let sec = u16::from_be_bytes([data[34], data[35]]);
    tags.push(mk(
        "ProfileDateTime",
        "Profile Date/Time",
        Value::String(format!(
            "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
            year, month, day, hour, min, sec
        )),
    ));

    // Primary platform (bytes 40-43)
    let platform = crate::encoding::decode_utf8_or_latin1(&data[40..44])
        .trim()
        .to_string();
    let platform_name = match platform.as_str() {
        "APPL" => "Apple Computer Inc.",
        "MSFT" => "Microsoft Corporation",
        "SGI" => "Silicon Graphics Inc.",
        "SUNW" => "Sun Microsystems Inc.",
        "TGNT" => "Taligent Inc.",
        _ => &platform,
    };
    if !platform_name.is_empty() {
        tags.push(mk(
            "PrimaryPlatform",
            "Primary Platform",
            Value::String(platform_name.to_string()),
        ));
    }

    // Rendering intent (byte 67)
    let intent = u32::from_be_bytes([data[64], data[65], data[66], data[67]]);
    let intent_name = match intent {
        0 => "Perceptual",
        1 => "Media-Relative Colorimetric",
        2 => "Saturation",
        3 => "ICC-Absolute Colorimetric",
        _ => "Unknown",
    };
    tags.push(mk(
        "RenderingIntent",
        "Rendering Intent",
        Value::String(intent_name.to_string()),
    ));

    // CMMFlags (bytes 44-47)
    let flags = u32::from_be_bytes([data[44], data[45], data[46], data[47]]);
    let cmm_flags = format!(
        "{}{}",
        if flags & 0x01 != 0 {
            "Embedded, "
        } else {
            "Not Embedded, "
        },
        if flags & 0x02 != 0 {
            "Not Independent"
        } else {
            "Independent"
        }
    );
    tags.push(mk("CMMFlags", "CMM Flags", Value::String(cmm_flags)));

    // DeviceAttributes (bytes 56-63, int32u[2]); ExifTool's PrintConv uses the
    // second word ($v[1] = bytes 60-63) for the reflective/glossy/etc. flags.
    let attr = u32::from_be_bytes([data[60], data[61], data[62], data[63]]);
    let dev_attr = format!(
        "{}{}{}{}",
        if attr & 0x01 != 0 {
            "Transparency, "
        } else {
            "Reflective, "
        },
        if attr & 0x02 != 0 {
            "Matte, "
        } else {
            "Glossy, "
        },
        if attr & 0x04 != 0 {
            "Negative, "
        } else {
            "Positive, "
        },
        if attr & 0x08 != 0 { "B&W" } else { "Color" }
    );
    tags.push(mk(
        "DeviceAttributes",
        "Device Attributes",
        Value::String(dev_attr),
    ));

    // Device manufacturer (bytes 48-51) and model (52-55)
    let manu_sig = crate::encoding::decode_utf8_or_latin1(&data[48..52]).to_string();
    if manu_sig.bytes().any(|b| b > 0x20) {
        // PrintConv via %manuSig (matching the 4-char signature incl. trailing spaces).
        let manufacturer = icc_manu_sig(&manu_sig)
            .map(str::to_string)
            .unwrap_or_else(|| manu_sig.trim().to_string());
        tags.push(mk(
            "DeviceManufacturer",
            "Device Manufacturer",
            Value::String(manufacturer),
        ));
    }
    // DeviceModel: always emit (may be empty string), from ICC profile header bytes 52-55
    let dev_model_raw = &data[52..56];
    let dev_model = if dev_model_raw.iter().all(|&b| b == 0) {
        String::new()
    } else {
        // string[4]: truncate at the first null (e.g. [0,0,0,1] -> "").
        crate::encoding::decode_utf8_or_latin1(dev_model_raw)
            .split('\0')
            .next()
            .unwrap_or("")
            .trim()
            .to_string()
    };
    tags.push(mk("DeviceModel", "Device Model", Value::String(dev_model)));

    // ProfileFileSignature (bytes 36-39, should be "acsp")
    tags.push(mk(
        "ProfileFileSignature",
        "Profile File Signature",
        Value::String("acsp".into()),
    ));

    // ConnectionSpaceIlluminant (bytes 68-79, XYZ)
    if data.len() >= 80 {
        tags.push(mk(
            "ConnectionSpaceIlluminant",
            "Connection Space Illuminant",
            Value::String(xyz_str(&data[68..80])),
        ));
    }

    // ProfileCreator (bytes 80-83) - always emit (may be empty)
    if data.len() >= 84 {
        let raw = &data[80..84];
        let creator = if raw.iter().all(|&b| b == 0) {
            String::new()
        } else {
            let code = crate::encoding::decode_utf8_or_latin1(raw)
                .trim_end_matches('\0')
                .trim()
                .to_string();
            // ProfileCreator is a registered ICC vendor signature (ADBE -> Adobe Systems Inc.).
            icc_vendor(&code).map(str::to_string).unwrap_or(code)
        };
        tags.push(mk(
            "ProfileCreator",
            "Profile Creator",
            Value::String(creator),
        ));
    }

    // ProfileID (bytes 84-99, MD5) — ExifTool shows "0" when not computed (all zero).
    if data.len() >= 100 {
        let raw = &data[84..100];
        let id = if raw.iter().all(|&b| b == 0) {
            "0".to_string()
        } else {
            raw.iter().map(|b| format!("{:02x}", b)).collect()
        };
        tags.push(mk("ProfileID", "Profile ID", Value::String(id)));
    }

    // Profile description tag - search in tag table
    if data.len() >= 132 {
        let tag_count = u32::from_be_bytes([data[128], data[129], data[130], data[131]]) as usize;
        let mut tpos = 132;
        for _ in 0..tag_count.min(100) {
            if tpos + 12 > data.len() {
                break;
            }
            let sig = &data[tpos..tpos + 4];
            let offset = u32::from_be_bytes([
                data[tpos + 4],
                data[tpos + 5],
                data[tpos + 6],
                data[tpos + 7],
            ]) as usize;
            let size = u32::from_be_bytes([
                data[tpos + 8],
                data[tpos + 9],
                data[tpos + 10],
                data[tpos + 11],
            ]) as usize;
            tpos += 12;

            if sig == b"desc" && offset + size <= data.len() && size > 12 {
                // 'desc' type: 4 bytes signature + 4 reserved + 4 bytes string length + string
                let d = &data[offset..offset + size];
                if d.len() >= 12 && &d[0..4] == b"desc" {
                    let str_len = u32::from_be_bytes([d[8], d[9], d[10], d[11]]) as usize;
                    if 12 + str_len <= d.len() {
                        let desc = crate::encoding::decode_utf8_or_latin1(&d[12..12 + str_len])
                            .trim_end_matches('\0')
                            .to_string();
                        if !desc.is_empty() {
                            tags.push(mk(
                                "ProfileDescription",
                                "Profile Description",
                                Value::String(desc),
                            ));
                        }
                    }
                }
            }

            if sig == b"cprt" && offset + size <= data.len() && size > 8 {
                let d = &data[offset..offset + size];
                if d.len() >= 8 && &d[0..4] == b"text" {
                    let text = crate::encoding::decode_utf8_or_latin1(&d[8..])
                        .trim_end_matches('\0')
                        .to_string();
                    if !text.is_empty() {
                        tags.push(mk(
                            "ProfileCopyright",
                            "Profile Copyright",
                            Value::String(text),
                        ));
                    }
                }
            }

            // Map ICC tag signatures to names (from Perl ICC_Profile.pm)
            if offset + size <= data.len() && size >= 8 {
                let d = &data[offset..offset + size];
                let tag_name = match sig {
                    b"rXYZ" => "RedMatrixColumn",
                    b"gXYZ" => "GreenMatrixColumn",
                    b"bXYZ" => "BlueMatrixColumn",
                    b"wtpt" => "MediaWhitePoint",
                    b"bkpt" => "MediaBlackPoint",
                    b"lumi" => "Luminance",
                    b"rTRC" => "RedTRC",
                    b"gTRC" => "GreenTRC",
                    b"bTRC" => "BlueTRC",
                    b"tech" => "Technology",
                    b"dmnd" => "DeviceMfgDesc",
                    b"dmdd" => "DeviceModelDesc",
                    b"vued" => "ViewingCondDesc",
                    b"view" => "ViewingConditions",
                    b"meas" => "MeasurementInfo",
                    b"chad" => "ChromaticAdaptation",
                    _ => "",
                };
                if !tag_name.is_empty() {
                    let type_sig = &d[0..4];
                    let value = match type_sig {
                        b"XYZ " if d.len() >= 20 => {
                            // XYZ type: 3 x s15Fixed16
                            xyz_str(&d[8..20])
                        }
                        b"curv" => {
                            // ExifTool's FormatICCTag only handles XYZ/text types; curveType
                            // is left as raw binary (e.g. RedTRC -> "(Binary data 14 bytes...)").
                            tags.push(mk(tag_name, tag_name, Value::Undefined(d.to_vec())));
                            String::new()
                        }
                        b"desc" if d.len() >= 12 => {
                            let len = u32::from_be_bytes([d[8], d[9], d[10], d[11]]) as usize;
                            if 12 + len <= d.len() {
                                crate::encoding::decode_utf8_or_latin1(&d[12..12 + len])
                                    .trim_end_matches('\0')
                                    .to_string()
                            } else {
                                String::new()
                            }
                        }
                        b"mluc" if d.len() >= 20 => {
                            // multiLocalizedUnicode
                            let rec_count = u32::from_be_bytes([d[8], d[9], d[10], d[11]]) as usize;
                            if rec_count > 0 && d.len() >= 20 {
                                let str_off =
                                    u32::from_be_bytes([d[20], d[21], d[22], d[23]]) as usize;
                                let str_len =
                                    u32::from_be_bytes([d[16], d[17], d[18], d[19]]) as usize;
                                if str_off + str_len <= d.len() {
                                    let units: Vec<u16> = d[str_off..str_off + str_len]
                                        .chunks_exact(2)
                                        .map(|c| u16::from_be_bytes([c[0], c[1]]))
                                        .collect();
                                    String::from_utf16_lossy(&units)
                                        .trim_end_matches('\0')
                                        .to_string()
                                } else {
                                    String::new()
                                }
                            } else {
                                String::new()
                            }
                        }
                        b"sig " if d.len() >= 12 => {
                            let s = crate::encoding::decode_utf8_or_latin1(&d[8..12]);
                            if tag_name == "Technology" {
                                icc_technology(&s)
                            } else {
                                s.trim().to_string()
                            }
                        }
                        b"meas" if d.len() >= 36 => {
                            // measurement type
                            let observer = match u32::from_be_bytes([d[8], d[9], d[10], d[11]]) {
                                1 => "CIE 1931",
                                2 => "CIE 1964",
                                _ => "Unknown",
                            };
                            tags.push(mk(
                                "MeasurementObserver",
                                "Measurement Observer",
                                Value::String(observer.into()),
                            ));
                            let geometry = match u32::from_be_bytes([d[24], d[25], d[26], d[27]]) {
                                1 => "0/45 or 45/0",
                                2 => "0/d or d/0",
                                _ => "Unknown",
                            };
                            tags.push(mk(
                                "MeasurementGeometry",
                                "Measurement Geometry",
                                Value::String(geometry.into()),
                            ));
                            let illum = match u32::from_be_bytes([d[32], d[33], d[34], d[35]]) {
                                1 => "D50",
                                2 => "D65",
                                3 => "D93",
                                4 => "F2",
                                5 => "D55",
                                6 => "A",
                                7 => "E",
                                8 => "F8",
                                _ => "Unknown",
                            };
                            tags.push(mk(
                                "MeasurementIlluminant",
                                "Measurement Illuminant",
                                Value::String(illum.into()),
                            ));
                            // Backing and flare
                            let backing_x =
                                i32::from_be_bytes([d[12], d[13], d[14], d[15]]) as f64 / 65536.0;
                            let backing_y =
                                i32::from_be_bytes([d[16], d[17], d[18], d[19]]) as f64 / 65536.0;
                            let backing_z =
                                i32::from_be_bytes([d[20], d[21], d[22], d[23]]) as f64 / 65536.0;
                            let f5 = |v: f64| crate::value::format_g15((v * 1e5).round() / 1e5);
                            tags.push(mk(
                                "MeasurementBacking",
                                "Measurement Backing",
                                Value::String(format!(
                                    "{} {} {}",
                                    f5(backing_x),
                                    f5(backing_y),
                                    f5(backing_z)
                                )),
                            ));
                            // fixed32u, GetFixed32u rounds to 5 decimals; PrintConv $val*100."%".
                            let raw =
                                u32::from_be_bytes([d[28], d[29], d[30], d[31]]) as f64 / 65536.0;
                            let flare = (raw * 1e5).round() / 1e5;
                            tags.push(mk(
                                "MeasurementFlare",
                                "Measurement Flare",
                                Value::String(format!(
                                    "{}%",
                                    crate::value::format_g15(flare * 100.0)
                                )),
                            ));
                            String::new() // sub-tags already pushed
                        }
                        b"view" if d.len() >= 28 => {
                            let x = i32::from_be_bytes([d[8], d[9], d[10], d[11]]) as f64 / 65536.0;
                            let y =
                                i32::from_be_bytes([d[12], d[13], d[14], d[15]]) as f64 / 65536.0;
                            let z =
                                i32::from_be_bytes([d[16], d[17], d[18], d[19]]) as f64 / 65536.0;
                            // ICC s15Fixed16: round to 5 decimals then %.15g (no trailing zeros).
                            let f5 = |v: f64| crate::value::format_g15((v * 1e5).round() / 1e5);
                            tags.push(mk(
                                "ViewingCondIlluminant",
                                "Viewing Cond Illuminant",
                                Value::String(format!("{} {} {}", f5(x), f5(y), f5(z))),
                            ));
                            let sx =
                                i32::from_be_bytes([d[20], d[21], d[22], d[23]]) as f64 / 65536.0;
                            let sy =
                                i32::from_be_bytes([d[24], d[25], d[26], d[27]]) as f64 / 65536.0;
                            let sz =
                                i32::from_be_bytes([d[28], d[29], d[30], d[31]]) as f64 / 65536.0;
                            tags.push(mk(
                                "ViewingCondSurround",
                                "Viewing Cond Surround",
                                Value::String(format!("{} {} {}", f5(sx), f5(sy), f5(sz))),
                            ));
                            if d.len() >= 36 {
                                let illum_type =
                                    match u32::from_be_bytes([d[32], d[33], d[34], d[35]]) {
                                        1 => "D50",
                                        2 => "D65",
                                        3 => "D93",
                                        4 => "F2",
                                        5 => "D55",
                                        6 => "A",
                                        7 => "E",
                                        8 => "F8",
                                        _ => "Unknown",
                                    };
                                tags.push(mk(
                                    "ViewingCondIlluminantType",
                                    "Viewing Cond Illuminant Type",
                                    Value::String(illum_type.into()),
                                ));
                            }
                            String::new()
                        }
                        _ => String::new(),
                    };
                    if !value.is_empty() {
                        tags.push(mk(tag_name, tag_name, Value::String(value)));
                    }
                }
            }
        }
    }

    Ok(tags)
}

/// ICC manufacturer/model signature PrintConv (ICC_Profile.pm %manuSig table).
fn icc_manu_sig(sig: &str) -> Option<&'static str> {
    Some(match sig {
        "4d2p" => "Erdt Systems GmbH & Co KG",
        "AAMA" => "Aamazing Technologies, Inc.",
        "ACER" => "Acer Peripherals",
        "ACLT" => "Acolyte Color Research",
        "ACTI" => "Actix Systems, Inc.",
        "ADAR" => "Adara Technology, Inc.",
        "ADBE" => "Adobe Systems Inc.",
        "ADI " => "ADI Systems, Inc.",
        "AGFA" => "Agfa Graphics N.V.",
        "ALMD" => "Alps Electric USA, Inc.",
        "ALPS" => "Alps Electric USA, Inc.",
        "ALWN" => "Alwan Color Expertise",
        "AMTI" => "Amiable Technologies, Inc.",
        "AOC " => "AOC International (U.S.A), Ltd.",
        "APAG" => "Apago",
        "APPL" => "Apple Computer Inc.",
        "appl" => "Apple Computer Inc.",
        "AST " => "AST",
        "AT&T" => "AT&T Computer Systems",
        "BAEL" => "BARBIERI electronic",
        "berg" => "bergdesign incorporated",
        "bICC" => "basICColor GmbH",
        "BRCO" => "Barco NV",
        "BRKP" => "Breakpoint Pty Limited",
        "BROT" => "Brother Industries, LTD",
        "BULL" => "Bull",
        "BUS " => "Bus Computer Systems",
        "C-IT" => "C-Itoh",
        "CAMR" => "Intel Corporation",
        "CANO" => "Canon, Inc. (Canon Development Americas, Inc.)",
        "CARR" => "Carroll Touch",
        "CASI" => "Casio Computer Co., Ltd.",
        "CBUS" => "Colorbus PL",
        "CEL " => "Crossfield",
        "CELx" => "Crossfield",
        "ceyd" => "Integrated Color Solutions, Inc.",
        "CGS " => "CGS Publishing Technologies International GmbH",
        "CHM " => "Rochester Robotics",
        "CIGL" => "Colour Imaging Group, London",
        "CITI" => "Citizen",
        "CL00" => "Candela, Ltd.",
        "CLIQ" => "Color IQ",
        "clsp" => "MacDermid ColorSpan, Inc.",
        "CMCO" => "Chromaco, Inc.",
        "CMiX" => "CHROMiX",
        "COLO" => "Colorgraphic Communications Corporation",
        "COMP" => "COMPAQ Computer Corporation",
        "COMp" => "Compeq USA/Focus Technology",
        "CONR" => "Conrac Display Products",
        "CORD" => "Cordata Technologies, Inc.",
        "CPQ " => "Compaq Computer Corporation",
        "CPRO" => "ColorPro",
        "CRN " => "Cornerstone",
        "CTX " => "CTX International, Inc.",
        "CVIS" => "ColorVision",
        "CWC " => "Fujitsu Laboratories, Ltd.",
        "DARI" => "Darius Technology, Ltd.",
        "DATA" => "Dataproducts",
        "DCP " => "Dry Creek Photo",
        "DCRC" => "Digital Contents Resource Center, Chung-Ang University",
        "DELL" => "Dell Computer Corporation",
        "DIC " => "Dainippon Ink and Chemicals",
        "DICO" => "Diconix",
        "DIGI" => "Digital",
        "DL&C" => "Digital Light & Color",
        "DPLG" => "Doppelganger, LLC",
        "DS  " => "Dainippon Screen",
        "ds  " => "Dainippon Screen",
        "DSOL" => "DOOSOL",
        "DUPN" => "DuPont",
        "dupn" => "DuPont",
        "Eizo" => "EIZO NANAO CORPORATION",
        "EPSO" => "Epson",
        "ESKO" => "Esko-Graphics",
        "ETRI" => "Electronics and Telecommunications Research Institute",
        "EVER" => "Everex Systems, Inc.",
        "EXAC" => "ExactCODE GmbH",
        "FALC" => "Falco Data Products, Inc.",
        "FF  " => "Fuji Photo Film Co.,LTD",
        "FFEI" => "FujiFilm Electronic Imaging, Ltd.",
        "ffei" => "FujiFilm Electronic Imaging, Ltd.",
        "flux" => "FluxData Corporation",
        "FNRD" => "fnord software",
        "FORA" => "Fora, Inc.",
        "FORE" => "Forefront Technology Corporation",
        "FP  " => "Fujitsu",
        "FPA " => "WayTech Development, Inc.",
        "FUJI" => "Fujitsu",
        "FX  " => "Fuji Xerox Co., Ltd.",
        "GCC " => "GCC Technologies, Inc.",
        "GGSL" => "Global Graphics Software Limited",
        "GMB " => "Gretagmacbeth",
        "GMG " => "GMG GmbH & Co. KG",
        "GOLD" => "GoldStar Technology, Inc.",
        "GOOG" => "Google",
        "GPRT" => "Giantprint Pty Ltd",
        "GTMB" => "Gretagmacbeth",
        "GVC " => "WayTech Development, Inc.",
        "GW2K" => "Sony Corporation",
        "HCI " => "HCI",
        "HDM " => "Heidelberger Druckmaschinen AG",
        "HERM" => "Hermes",
        "HITA" => "Hitachi America, Ltd.",
        "HiTi" => "HiTi Digital, Inc.",
        "HP  " => "Hewlett-Packard",
        "HTC " => "Hitachi, Ltd.",
        "IBM " => "IBM Corporation",
        "IDNT" => "Scitex Corporation, Ltd.",
        "Idnt" => "Scitex Corporation, Ltd.",
        "IEC " => "Hewlett-Packard",
        "IIYA" => "Iiyama North America, Inc.",
        "IKEG" => "Ikegami Electronics, Inc.",
        "IMAG" => "Image Systems Corporation",
        "IMI " => "Ingram Micro, Inc.",
        "Inca" => "Inca Digital Printers Ltd.",
        "INTC" => "Intel Corporation",
        "INTL" => "N/A (INTL)",
        "INTR" => "Intra Electronics USA, Inc.",
        "IOCO" => "Iocomm International Technology Corporation",
        "IPS " => "InfoPrint Solutions Company",
        "IRIS" => "Scitex Corporation, Ltd.",
        "Iris" => "Scitex Corporation, Ltd.",
        "iris" => "Scitex Corporation, Ltd.",
        "ISL " => "Ichikawa Soft Laboratory",
        "ITNL" => "N/A (ITNL)",
        "IVM " => "IVM",
        "IWAT" => "Iwatsu Electric Co., Ltd.",
        "JPEG" => "Joint Photographic Experts Group",
        "JSFT" => "Jetsoft Development",
        "JVC " => "JVC Information Products Co.",
        "KART" => "Scitex Corporation, Ltd.",
        "Kart" => "Scitex Corporation, Ltd.",
        "kart" => "Scitex Corporation, Ltd.",
        "KFC " => "KFC Computek Components Corporation",
        "KLH " => "KLH Computers",
        "KMHD" => "Konica Minolta Holdings, Inc.",
        "KNCA" => "Konica Corporation",
        "KODA" => "Kodak",
        "KYOC" => "Kyocera",
        "LCAG" => "Leica Camera AG",
        "LCCD" => "Leeds Colour",
        "lcms" => "Little CMS",
        "LDAK" => "Left Dakota",
        "LEAD" => "Leading Technology, Inc.",
        "Leaf" => "Leaf",
        "LEXM" => "Lexmark International, Inc.",
        "LINK" => "Link Computer, Inc.",
        "LINO" => "Linotronic",
        "Lino" => "Linotronic",
        "lino" => "Linotronic",
        "LITE" => "Lite-On, Inc.",
        "MAGC" => "Mag Computronic (USA) Inc.",
        "MAGI" => "MAG Innovision, Inc.",
        "MANN" => "Mannesmann",
        "MICN" => "Micron Technology, Inc.",
        "MICR" => "Microtek",
        "MICV" => "Microvitec, Inc.",
        "MINO" => "Minolta",
        "MITS" => "Mitsubishi Electronics America, Inc.",
        "MITs" => "Mitsuba Corporation",
        "Mits" => "Mitsubishi Electric Corporation Kyoto Works",
        "MNLT" => "Minolta",
        "MODG" => "Modgraph, Inc.",
        "MONI" => "Monitronix, Inc.",
        "MONS" => "Monaco Systems Inc.",
        "MORS" => "Morse Technology, Inc.",
        "MOTI" => "Motive Systems",
        "MSFT" => "Microsoft Corporation",
        "MUTO" => "MUTOH INDUSTRIES LTD.",
        "NANA" => "NANAO USA Corporation",
        "NEC " => "NEC Corporation",
        "NEXP" => "NexPress Solutions LLC",
        "NISS" => "Nissei Sangyo America, Ltd.",
        "NKON" => "Nikon Corporation",
        "ob4d" => "Erdt Systems GmbH & Co KG",
        "obic" => "Medigraph GmbH",
        "OCE " => "Oce Technologies B.V.",
        "OCEC" => "OceColor",
        "OKI " => "Oki",
        "OKID" => "Okidata",
        "OKIP" => "Okidata",
        "OLIV" => "Olivetti",
        "OLYM" => "OLYMPUS OPTICAL CO., LTD",
        "ONYX" => "Onyx Graphics",
        "OPTI" => "Optiquest",
        "PACK" => "Packard Bell",
        "PANA" => "Matsushita Electric Industrial Co., Ltd.",
        "PANT" => "Pantone, Inc.",
        "PBN " => "Packard Bell",
        "PFU " => "PFU Limited",
        "PHIL" => "Philips Consumer Electronics Co.",
        "PNTX" => "HOYA Corporation PENTAX Imaging Systems Division",
        "POne" => "Phase One A/S",
        "PREM" => "Premier Computer Innovations",
        "PRIN" => "Princeton Graphic Systems",
        "PRIP" => "Princeton Publishing Labs",
        "QLUX" => "Hong Kong",
        "QMS " => "QMS, Inc.",
        "QPCD" => "QPcard AB",
        "QUAD" => "QuadLaser",
        "quby" => "Qubyx Sarl",
        "QUME" => "Qume Corporation",
        "RADI" => "Radius, Inc.",
        "RDDx" => "Integrated Color Solutions, Inc.",
        "RDG " => "Roland DG Corporation",
        "REDM" => "REDMS Group, Inc.",
        "RELI" => "Relisys",
        "RGMS" => "Rolf Gierling Multitools",
        "RICO" => "Ricoh Corporation",
        "RNLD" => "Edmund Ronald",
        "ROYA" => "Royal",
        "RPC " => "Ricoh Printing Systems,Ltd.",
        "RTL " => "Royal Information Electronics Co., Ltd.",
        "SAMP" => "Sampo Corporation of America",
        "SAMS" => "Samsung, Inc.",
        "SANT" => "Jaime Santana Pomares",
        "SCIT" => "Scitex Corporation, Ltd.",
        "Scit" => "Scitex Corporation, Ltd.",
        "scit" => "Scitex Corporation, Ltd.",
        "SCRN" => "Dainippon Screen",
        "scrn" => "Dainippon Screen",
        "SDP " => "Scitex Corporation, Ltd.",
        "Sdp " => "Scitex Corporation, Ltd.",
        "sdp " => "Scitex Corporation, Ltd.",
        "SEC " => "SAMSUNG ELECTRONICS CO.,LTD",
        "SEIK" => "Seiko Instruments U.S.A., Inc.",
        "SEIk" => "Seikosha",
        "SGUY" => "ScanGuy.com",
        "SHAR" => "Sharp Laboratories",
        "SICC" => "International Color Consortium",
        "siwi" => "SIWI GRAFIKA CORPORATION",
        "SONY" => "SONY Corporation",
        "Sony" => "Sony Corporation",
        "SPCL" => "SpectraCal",
        "STAR" => "Star",
        "STC " => "Sampo Technology Corporation",
        "TALO" => "Talon Technology Corporation",
        "TAND" => "Tandy",
        "TATU" => "Tatung Co. of America, Inc.",
        "TAXA" => "TAXAN America, Inc.",
        "TDS " => "Tokyo Denshi Sekei K.K.",
        "TECO" => "TECO Information Systems, Inc.",
        "TEGR" => "Tegra",
        "TEKT" => "Tektronix, Inc.",
        "TI  " => "Texas Instruments",
        "TMKR" => "TypeMaker Ltd.",
        "TOSB" => "TOSHIBA corp.",
        "TOSH" => "Toshiba, Inc.",
        "TOTK" => "TOTOKU ELECTRIC Co., LTD",
        "TRIU" => "Triumph",
        "TSBT" => "TOSHIBA TEC CORPORATION",
        "TTX " => "TTX Computer Products, Inc.",
        "TVM " => "TVM Professional Monitor Corporation",
        "TW  " => "TW Casper Corporation",
        "ULSX" => "Ulead Systems",
        "UNIS" => "Unisys",
        "UTZF" => "Utz Fehlau & Sohn",
        "VARI" => "Varityper",
        "VIEW" => "Viewsonic",
        "VISL" => "Visual communication",
        "VIVO" => "Vivo Mobile Communication Co., Ltd",
        "WANG" => "Wang",
        "WLBR" => "Wilbur Imaging",
        "WTG2" => "Ware To Go",
        "WYSE" => "WYSE Technology",
        "XERX" => "Xerox Corporation",
        "XM  " => "Xiaomi",
        "XRIT" => "X-Rite",
        "yxym" => "YxyMaster GmbH",
        "Zebr" => "Zebra Technologies Inc",
        "ZRAN" => "Zoran Corporation",
        _ => return None,
    })
}

/// ICC Technology signature PrintConv (ICC_Profile.pm tech table).
fn icc_technology(sig: &str) -> String {
    match sig {
        "fscn" => "Film Scanner",
        "dcam" => "Digital Camera",
        "rscn" => "Reflective Scanner",
        "ijet" => "Ink Jet Printer",
        "twax" => "Thermal Wax Printer",
        "epho" => "Electrophotographic Printer",
        "esta" => "Electrostatic Printer",
        "dsub" => "Dye Sublimation Printer",
        "rpho" => "Photographic Paper Printer",
        "fprn" => "Film Writer",
        "vidm" => "Video Monitor",
        "vidc" => "Video Camera",
        "pjtv" => "Projection Television",
        "CRT " => "Cathode Ray Tube Display",
        "PMD " => "Passive Matrix Display",
        "AMD " => "Active Matrix Display",
        "KPCD" => "Photo CD",
        "imgs" => "Photo Image Setter",
        "grav" => "Gravure",
        "offs" => "Offset Lithography",
        "silk" => "Silkscreen",
        "flex" => "Flexography",
        "mpfs" => "Motion Picture Film Scanner",
        "mpfr" => "Motion Picture Film Recorder",
        "dmpc" => "Digital Motion Picture Camera",
        "dcpj" => "Digital Cinema Projector",
        other => return other.trim().to_string(),
    }
    .to_string()
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "ICC_Profile".into(),
            family1: "ICC_Profile".into(),
            family2: "Image".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}

/// Parse ICC profile tags from raw profile data embedded in JPEG APP2.
pub fn parse_icc_tags(data: &[u8]) -> Vec<Tag> {
    read_icc(data).unwrap_or_default()
}

/// ExifTool ICC registered-vendor signatures (subset) for ProfileCMMType etc.
fn icc_vendor(code: &str) -> Option<&'static str> {
    Some(match code.trim() {
        "ADBE" => "Adobe Systems Inc.",
        "APPL" => "Apple Computer Inc.",
        "MSFT" => "Microsoft Corporation",
        "KODA" => "Kodak",
        "Lino" | "LINO" | "lino" => "Linotronic",
        "SGI" => "Silicon Graphics Inc.",
        "SUNW" => "Sun Microsystems Inc.",
        "TGNT" => "Taligent Inc.",
        "HP" => "Hewlett-Packard",
        "NKON" => "Nikon Corporation",
        "CANO" => "Canon",
        "EPSO" => "Epson",
        "FF" => "Fujifilm",
        "KONI" => "Konica Minolta",
        "LOGO" => "Logo",
        "SONY" => "Sony Corporation",
        "DL&C" => "Digital Light & Color",
        "GMG" => "GMG",
        "ZC00" => "Zoran Corporation",
        "ICC" => "ICC",
        _ => return None,
    })
}
