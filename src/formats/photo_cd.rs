//! Kodak Photo CD format reader.

use super::gzip::gzip_unix_to_datetime;
use super::misc::mktag;
use crate::error::{Error, Result};
use crate::tag::Tag;
use crate::value::Value;

pub fn read_photo_cd(data: &[u8]) -> Result<Vec<Tag>> {
    // PCD magic at byte 2048: "PCD_IPI"
    if data.len() < 2056 || &data[2048..2055] != b"PCD_IPI" {
        return Err(Error::InvalidData("not a PhotoCD file".into()));
    }
    let pcd = &data[2048..]; // PCD block (2048+ bytes)
    if pcd.len() < 1540 {
        return Err(Error::InvalidData("PhotoCD data too short".into()));
    }

    let mut tags: Vec<Tag> = Vec::new();

    // Byte 7: SpecificationVersion (int8u[2])
    let sv = pcd[7];
    let sv2 = pcd[8];
    if sv != 255 || sv2 != 255 {
        tags.push(mktag(
            "PhotoCD",
            "SpecificationVersion",
            "Specification Version",
            Value::String(format!("{}.{}", sv, sv2)),
        ));
    }

    // Byte 9: AuthoringSoftwareRelease (int8u[2])
    let ar = pcd[9];
    let ar2 = pcd[10];
    if ar != 255 || ar2 != 255 {
        tags.push(mktag(
            "PhotoCD",
            "AuthoringSoftwareRelease",
            "Authoring Software Release",
            Value::String(format!("{}.{}", ar, ar2)),
        ));
    }

    // Byte 11: ImageMagnificationDescriptor (int8u[2])
    let im1 = pcd[11];
    let im2 = pcd[12];
    tags.push(mktag(
        "PhotoCD",
        "ImageMagnificationDescriptor",
        "Image Magnification Descriptor",
        Value::String(format!("{}.{}", im1, im2)),
    ));

    // Byte 13: CreateDate (int32u BE, unix time)
    if pcd.len() >= 17 {
        let ts = u32::from_be_bytes([pcd[13], pcd[14], pcd[15], pcd[16]]);
        if ts != 0xffffffff {
            let dt = gzip_unix_to_datetime(ts as i64);
            tags.push(mktag(
                "PhotoCD",
                "CreateDate",
                "Create Date",
                Value::String(dt),
            ));
        }
    }

    // Byte 17: ModifyDate (int32u BE, unix time)
    if pcd.len() >= 21 {
        let ts = u32::from_be_bytes([pcd[17], pcd[18], pcd[19], pcd[20]]);
        if ts != 0xffffffff {
            let dt = gzip_unix_to_datetime(ts as i64);
            tags.push(mktag(
                "PhotoCD",
                "ModifyDate",
                "Modify Date",
                Value::String(dt),
            ));
        }
    }

    // Byte 21: ImageMedium
    let medium = pcd[21];
    let medium_str = match medium {
        0 => "Color negative",
        1 => "Color reversal",
        2 => "Color hard copy",
        3 => "Thermal hard copy",
        4 => "Black and white negative",
        5 => "Black and white reversal",
        6 => "Black and white hard copy",
        7 => "Internegative",
        8 => "Synthetic image",
        _ => "",
    };
    if !medium_str.is_empty() {
        tags.push(mktag(
            "PhotoCD",
            "ImageMedium",
            "Image Medium",
            Value::String(medium_str.into()),
        ));
    }

    // Byte 22: ProductType (string[20])
    if pcd.len() >= 42 {
        let s = pcd_rtrim_str(&pcd[22..42]);
        if !s.is_empty() {
            tags.push(mktag(
                "PhotoCD",
                "ProductType",
                "Product Type",
                Value::String(s),
            ));
        }
    }

    // Byte 42: ScannerVendorID (string[20])
    if pcd.len() >= 62 {
        let s = pcd_rtrim_str(&pcd[42..62]);
        if !s.is_empty() {
            tags.push(mktag(
                "PhotoCD",
                "ScannerVendorID",
                "Scanner Vendor ID",
                Value::String(s),
            ));
        }
    }

    // Byte 62: ScannerProductID (string[16])
    if pcd.len() >= 78 {
        let s = pcd_rtrim_str(&pcd[62..78]);
        if !s.is_empty() {
            tags.push(mktag(
                "PhotoCD",
                "ScannerProductID",
                "Scanner Product ID",
                Value::String(s),
            ));
        }
    }

    // Byte 78: ScannerFirmwareVersion (string[4])
    if pcd.len() >= 82 {
        let s = pcd_rtrim_str(&pcd[78..82]);
        if !s.is_empty() {
            tags.push(mktag(
                "PhotoCD",
                "ScannerFirmwareVersion",
                "Scanner Firmware Version",
                Value::String(s),
            ));
        }
    }

    // Byte 82: ScannerFirmwareDate (string[8])
    if pcd.len() >= 90 {
        let s = pcd_rtrim_str(&pcd[82..90]);
        // Always emit (even if empty string)
        tags.push(mktag(
            "PhotoCD",
            "ScannerFirmwareDate",
            "Scanner Firmware Date",
            Value::String(s),
        ));
    }

    // Byte 90: ScannerSerialNumber (string[20])
    if pcd.len() >= 110 {
        let s = pcd_rtrim_str(&pcd[90..110]);
        if !s.is_empty() {
            tags.push(mktag(
                "PhotoCD",
                "ScannerSerialNumber",
                "Scanner Serial Number",
                Value::String(s),
            ));
        }
    }

    // Byte 110: ScannerPixelSize (undef[2]) - hex nibbles joined with '.'
    if pcd.len() >= 112 {
        let h1 = pcd[110];
        let h2 = pcd[111];
        let pixel_size = format!("{:02x}.{:02x}", h1, h2)
            .trim_start_matches('0')
            .to_string();
        let pixel_size = if pixel_size.starts_with('.') {
            format!("0{}", pixel_size)
        } else {
            pixel_size
        };
        tags.push(mktag(
            "PhotoCD",
            "ScannerPixelSize",
            "Scanner Pixel Size",
            Value::String(format!("{} micrometers", pixel_size)),
        ));
    }

    // Byte 112: ImageWorkstationMake (string[20])
    if pcd.len() >= 132 {
        let s = pcd_rtrim_str(&pcd[112..132]);
        if !s.is_empty() {
            tags.push(mktag(
                "PhotoCD",
                "ImageWorkstationMake",
                "Image Workstation Make",
                Value::String(s),
            ));
        }
    }

    // Byte 132: CharacterSet
    if pcd.len() >= 133 {
        let cs = pcd[132];
        let cs_str = match cs {
            1 => "38 characters ISO 646",
            2 => "65 characters ISO 646",
            3 => "95 characters ISO 646",
            4 => "191 characters ISO 8850-1",
            5 => "ISO 2022",
            6 => "Includes characters not ISO 2375 registered",
            _ => "",
        };
        if !cs_str.is_empty() {
            tags.push(mktag(
                "PhotoCD",
                "CharacterSet",
                "Character Set",
                Value::String(cs_str.into()),
            ));
        }
    }

    // Byte 165: PhotoFinisherName (string[60])
    if pcd.len() >= 225 {
        let s = pcd_rtrim_str(&pcd[165..225]);
        if !s.is_empty() {
            tags.push(mktag(
                "PhotoCD",
                "PhotoFinisherName",
                "Photo Finisher Name",
                Value::String(s),
            ));
        }
    }

    // Check for SBA marker at bytes 225-228
    let has_sba = pcd.len() >= 228 && &pcd[225..228] == b"SBA";

    if has_sba && pcd.len() >= 230 {
        // Byte 228: SceneBalanceAlgorithmRevision (int8u[2])
        let r1 = pcd[228];
        let r2 = pcd[229];
        tags.push(mktag(
            "PhotoCD",
            "SceneBalanceAlgorithmRevision",
            "Scene Balance Algorithm Revision",
            Value::String(format!("{}.{}", r1, r2)),
        ));

        // Byte 230: SceneBalanceAlgorithmCommand
        let cmd = pcd[230];
        let cmd_str = match cmd {
            0 => "Neutral SBA On, Color SBA On",
            1 => "Neutral SBA Off, Color SBA Off",
            2 => "Neutral SBA On, Color SBA Off",
            3 => "Neutral SBA Off, Color SBA On",
            _ => "",
        };
        if !cmd_str.is_empty() {
            tags.push(mktag(
                "PhotoCD",
                "SceneBalanceAlgorithmCommand",
                "Scene Balance Algorithm Command",
                Value::String(cmd_str.into()),
            ));
        }

        // Byte 325: SceneBalanceAlgorithmFilmID (int16u BE)
        if pcd.len() >= 327 {
            let film_id = u16::from_be_bytes([pcd[325], pcd[326]]) as u32;
            let film_str = pcd_film_id_name(film_id);
            tags.push(mktag(
                "PhotoCD",
                "SceneBalanceAlgorithmFilmID",
                "Scene Balance Algorithm Film ID",
                Value::String(film_str.to_string()),
            ));
        }

        // Byte 331: CopyrightStatus
        if pcd.len() >= 332 {
            let cs = pcd[331];
            let cs_str = match cs {
                1 => "Restrictions apply",
                0xff => "Not specified",
                _ => "",
            };
            if !cs_str.is_empty() {
                tags.push(mktag(
                    "PhotoCD",
                    "CopyrightStatus",
                    "Copyright Status",
                    Value::String(cs_str.into()),
                ));
            }
        }
    }

    // Byte 1538: Orientation and size info
    if pcd.len() >= 1539 {
        let byte = pcd[1538];
        let orient_raw = byte & 0x03;
        let size_raw = (byte & 0x0c) >> 2;
        let class_raw = (byte & 0x60) >> 5;

        let orient_str = match orient_raw {
            0 => "Horizontal (normal)",
            1 => "Rotate 270 CW",
            2 => "Rotate 180",
            3 => "Rotate 90 CW",
            _ => "",
        };
        tags.push(mktag(
            "PhotoCD",
            "Orientation",
            "Orientation",
            Value::String(orient_str.into()),
        ));

        // ImageWidth and ImageHeight depend on orientation
        // Base size: 768x512 (landscape), 512x768 (portrait for rotate 90/270)
        // size_raw: 0=Base (768x512), 1=4Base, 2=16Base
        // scale factor: $val * 2 || 1 = if size_raw > 0 { size_raw * 2 } else { 1 }
        let scale = if size_raw > 0 {
            (size_raw * 2) as u32
        } else {
            1
        };
        let (w, h) = if orient_raw & 0x01 != 0 {
            (512 * scale, 768 * scale) // portrait
        } else {
            (768 * scale, 512 * scale) // landscape
        };
        tags.push(mktag(
            "PhotoCD",
            "ImageWidth",
            "Image Width",
            Value::String(w.to_string()),
        ));
        tags.push(mktag(
            "PhotoCD",
            "ImageHeight",
            "Image Height",
            Value::String(h.to_string()),
        ));

        let class_str = match class_raw {
            0 => "Class 1 - 35mm film; Pictoral hard copy",
            1 => "Class 2 - Large format film",
            2 => "Class 3 - Text and graphics, high resolution",
            3 => "Class 4 - Text and graphics, high dynamic range",
            _ => "",
        };
        if !class_str.is_empty() {
            tags.push(mktag(
                "PhotoCD",
                "CompressionClass",
                "Compression Class",
                Value::String(class_str.into()),
            ));
        }
    }

    Ok(tags)
}

fn pcd_rtrim_str(bytes: &[u8]) -> String {
    // Perl's string[] format reads to null terminator, then trims trailing spaces/NULs
    let null_end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let s = crate::encoding::decode_utf8_or_latin1(&bytes[..null_end]);
    s.trim_end_matches([' ', '\0']).to_string()
}

fn pcd_film_id_name(n: u32) -> &'static str {
    match n {
        1 => "3M ScotchColor AT 100",
        2 => "3M ScotchColor AT 200",
        3 => "3M ScotchColor HR2 400",
        7 => "3M Scotch HR 200 Gen 2",
        9 => "3M Scotch HR 400 Gen 2",
        16 => "Agfa Agfacolor XRS 400 Gen 1",
        17 => "Agfa Agfacolor XRG/XRS 400",
        18 => "Agfa Agfacolor XRG/XRS 200",
        19 => "Agfa Agfacolor XRS 1000 Gen 2",
        20 => "Agfa Agfacolor XRS 400 Gen 2",
        21 => "Agfa Agfacolor XRS/XRC 100",
        26 => "Fuji Reala 100 (JAPAN)",
        27 => "Fuji Reala 100 Gen 1",
        28 => "Fuji Reala 100 Gen 2",
        29 => "Fuji SHR 400 Gen 2",
        30 => "Fuji Super HG 100",
        31 => "Fuji Super HG 1600 Gen 1",
        32 => "Fuji Super HG 200",
        33 => "Fuji Super HG 400",
        34 => "Fuji Super HG 100 Gen 2",
        35 => "Fuji Super HR 100 Gen 1",
        36 => "Fuji Super HR 100 Gen 2",
        37 => "Fuji Super HR 1600 Gen 2",
        38 => "Fuji Super HR 200 Gen 1",
        39 => "Fuji Super HR 200 Gen 2",
        40 => "Fuji Super HR 400 Gen 1",
        43 => "Fuji NSP 160S (Pro)",
        45 => "Kodak Kodacolor VR 100 Gen 2",
        47 => "Kodak Gold 400 Gen 3",
        55 => "Kodak Ektar 100 Gen 1",
        56 => "Kodak Ektar 1000 Gen 1",
        57 => "Kodak Ektar 125 Gen 1",
        58 => "Kodak Royal Gold 25 RZ",
        60 => "Kodak Gold 1600 Gen 1",
        61 => "Kodak Gold 200 Gen 2",
        62 => "Kodak Gold 400 Gen 2",
        65 => "Kodak Kodacolor VR 100 Gen 1",
        66 => "Kodak Kodacolor VR 1000 Gen 2",
        67 => "Kodak Kodacolor VR 1000 Gen 1",
        68 => "Kodak Kodacolor VR 200 Gen 1",
        69 => "Kodak Kodacolor VR 400 Gen 1",
        70 => "Kodak Kodacolor VR 200 Gen 2",
        71 => "Kodak Kodacolor VRG 100 Gen 1",
        72 => "Kodak Gold 100 Gen 2",
        73 => "Kodak Kodacolor VRG 200 Gen 1",
        74 => "Kodak Gold 400 Gen 1",
        87 => "Kodak Ektacolor Gold 160",
        88 => "Kodak Ektapress 1600 Gen 1 PPC",
        89 => "Kodak Ektapress Gold 100 Gen 1 PPA",
        90 => "Kodak Ektapress Gold 400 PPB-3",
        92 => "Kodak Ektar 25 Professional PHR",
        97 => "Kodak T-Max 100 Professional",
        98 => "Kodak T-Max 3200 Professional",
        99 => "Kodak T-Max 400 Professional",
        101 => "Kodak Vericolor 400 Prof VPH",
        102 => "Kodak Vericolor III Pro",
        121 => "Konika Konica Color SR-G 3200",
        122 => "Konika Konica Color Super SR100",
        123 => "Konika Konica Color Super SR 400",
        138 => "Kodak Gold Unknown",
        139 => "Kodak Unknown Neg A- Normal SBA",
        143 => "Kodak Ektar 100 Gen 2",
        147 => "Kodak Kodacolor CII",
        148 => "Kodak Kodacolor II",
        149 => "Kodak Gold Plus 200 Gen 3",
        150 => "Kodak Internegative +10% Contrast",
        151 => "Agfa Agfacolor Ultra 50",
        152 => "Fuji NHG 400",
        153 => "Agfa Agfacolor XRG 100",
        154 => "Kodak Gold Plus 100 Gen 3",
        155 => "Konika Konica Color Super SR200 Gen 1",
        156 => "Konika Konica Color SR-G 160",
        157 => "Agfa Agfacolor Optima 125",
        158 => "Agfa Agfacolor Portrait 160",
        162 => "Kodak Kodacolor VRG 400 Gen 1",
        163 => "Kodak Gold 200 Gen 1",
        164 => "Kodak Kodacolor VRG 100 Gen 2",
        174 => "Kodak Internegative +20% Contrast",
        175 => "Kodak Internegative +30% Contrast",
        176 => "Kodak Internegative +40% Contrast",
        184 => "Kodak TMax-100 D-76 CI = .40",
        185 => "Kodak TMax-100 D-76 CI = .50",
        186 => "Kodak TMax-100 D-76 CI = .55",
        187 => "Kodak TMax-100 D-76 CI = .70",
        188 => "Kodak TMax-100 D-76 CI = .80",
        189 => "Kodak TMax-100 TMax CI = .40",
        190 => "Kodak TMax-100 TMax CI = .50",
        191 => "Kodak TMax-100 TMax CI = .55",
        192 => "Kodak TMax-100 TMax CI = .70",
        193 => "Kodak TMax-100 TMax CI = .80",
        195 => "Kodak TMax-400 D-76 CI = .40",
        196 => "Kodak TMax-400 D-76 CI = .50",
        197 => "Kodak TMax-400 D-76 CI = .55",
        198 => "Kodak TMax-400 D-76 CI = .70",
        214 => "Kodak TMax-400 D-76 CI = .80",
        215 => "Kodak TMax-400 TMax CI = .40",
        216 => "Kodak TMax-400 TMax CI = .50",
        217 => "Kodak TMax-400 TMax CI = .55",
        218 => "Kodak TMax-400 TMax CI = .70",
        219 => "Kodak TMax-400 TMax CI = .80",
        224 => "3M ScotchColor ATG 400/EXL 400",
        266 => "Agfa Agfacolor Optima 200",
        267 => "Konika Impressa 50",
        268 => "Polaroid Polaroid CP 200",
        269 => "Konika Konica Color Super SR200 Gen 2",
        270 => "ILFORD XP2 400",
        271 => "Polaroid Polaroid Color HD2 100",
        272 => "Polaroid Polaroid Color HD2 400",
        273 => "Polaroid Polaroid Color HD2 200",
        282 => "3M ScotchColor ATG-1 200",
        284 => "Konika XG 400",
        307 => "Kodak Universal Reversal B/W",
        308 => "Kodak RPC Copy Film Gen 1",
        312 => "Kodak Universal E6",
        324 => "Kodak Gold Ultra 400 Gen 4",
        328 => "Fuji Super G 100",
        329 => "Fuji Super G 200",
        330 => "Fuji Super G 400 Gen 2",
        333 => "Kodak Universal K14",
        334 => "Fuji Super G 400 Gen 1",
        366 => "Kodak Vericolor HC 6329 VHC",
        367 => "Kodak Vericolor HC 4329 VHC",
        368 => "Kodak Vericolor L 6013 VPL",
        369 => "Kodak Vericolor L 4013 VPL",
        418 => "Kodak Ektacolor Gold II 400 Prof",
        430 => "Kodak Royal Gold 1000",
        431 => "Kodak Kodacolor VR 200 / 5093",
        432 => "Kodak Gold Plus 100 Gen 4",
        443 => "Kodak Royal Gold 100",
        444 => "Kodak Royal Gold 400",
        445 => "Kodak Universal E6 auto-balance",
        446 => "Kodak Universal E6 illum. corr.",
        447 => "Kodak Universal K14 auto-balance",
        448 => "Kodak Universal K14 illum. corr.",
        449 => "Kodak Ektar 100 Gen 3 SY",
        456 => "Kodak Ektar 25",
        457 => "Kodak Ektar 100 Gen 3 CX",
        458 => "Kodak Ektapress Plus 100 Prof PJA-1",
        459 => "Kodak Ektapress Gold II 100 Prof",
        460 => "Kodak Pro 100 PRN",
        461 => "Kodak Vericolor HC 100 Prof VHC-2",
        462 => "Kodak Prof Color Neg 100",
        463 => "Kodak Ektar 1000 Gen 2",
        464 => "Kodak Ektapress Plus 1600 Pro PJC-1",
        465 => "Kodak Ektapress Gold II 1600 Prof",
        466 => "Kodak Super Gold 1600 GF Gen 2",
        467 => "Kodak Kodacolor 100 Print Gen 4",
        468 => "Kodak Super Gold 100 Gen 4",
        469 => "Kodak Gold 100 Gen 4",
        470 => "Kodak Gold III 100 Gen 4",
        471 => "Kodak Funtime 100 FA",
        472 => "Kodak Funtime 200 FB",
        473 => "Kodak Kodacolor VR 200 Gen 4",
        474 => "Kodak Gold Super 200 Gen 4",
        475 => "Kodak Kodacolor 200 Print Gen 4",
        476 => "Kodak Super Gold 200 Gen 4",
        477 => "Kodak Gold 200 Gen 4",
        478 => "Kodak Gold III 200 Gen 4",
        479 => "Kodak Gold Ultra 400 Gen 5",
        480 => "Kodak Super Gold 400 Gen 5",
        481 => "Kodak Gold 400 Gen 5",
        482 => "Kodak Gold III 400 Gen 5",
        483 => "Kodak Kodacolor 400 Print Gen 5",
        484 => "Kodak Ektapress Plus 400 Prof PJB-2",
        485 => "Kodak Ektapress Gold II 400 Prof G5",
        486 => "Kodak Pro 400 PPF-2",
        487 => "Kodak Ektacolor Gold II 400 EGP-4",
        488 => "Kodak Ektacolor Gold 400 Prof EGP-4",
        489 => "Kodak Ektapress Gold II Multspd PJM",
        490 => "Kodak Pro 400 MC PMC",
        491 => "Kodak Vericolor 400 Prof VPH-2",
        492 => "Kodak Vericolor 400 Plus Prof VPH-2",
        493 => "Kodak Unknown Neg Product Code 83",
        505 => "Kodak Ektacolor Pro Gold 160 GPX",
        508 => "Kodak Royal Gold 200",
        517 => "Kodak 4050000000",
        519 => "Kodak Gold Plus 100 Gen 5",
        520 => "Kodak Gold 800 Gen 1",
        521 => "Kodak Gold Super 200 Gen 5",
        522 => "Kodak Ektapress Plus 200 Prof",
        523 => "Kodak 4050 E6 auto-balance",
        524 => "Kodak 4050 E6 ilum. corr.",
        525 => "Kodak 4050 K14",
        526 => "Kodak 4050 K14 auto-balance",
        527 => "Kodak 4050 K14 ilum. corr.",
        528 => "Kodak 4050 Reversal B&W",
        532 => "Kodak Advantix 200",
        533 => "Kodak Advantix 400",
        534 => "Kodak Advantix 100",
        535 => "Kodak Ektapress Multspd Prof PJM-2",
        536 => "Kodak Kodacolor VR 200 Gen 5",
        537 => "Kodak Funtime 200 FB Gen 2",
        538 => "Kodak Commercial 200",
        539 => "Kodak Royal Gold 25 Copystand",
        540 => "Kodak Kodacolor DA 100 Gen 5",
        545 => "Kodak Kodacolor VR 400 Gen 2",
        546 => "Kodak Gold 100 Gen 6",
        547 => "Kodak Gold 200 Gen 6",
        548 => "Kodak Gold 400 Gen 6",
        549 => "Kodak Royal Gold 100 Gen 2",
        550 => "Kodak Royal Gold 200 Gen 2",
        551 => "Kodak Royal Gold 400 Gen 2",
        552 => "Kodak Gold Max 800 Gen 2",
        554 => "Kodak 4050 E6 high contrast",
        555 => "Kodak 4050 E6 low saturation high contrast",
        556 => "Kodak 4050 E6 low saturation",
        557 => "Kodak Universal E-6 Low Saturation",
        558 => "Kodak T-Max T400 CN",
        563 => "Kodak Ektapress PJ100",
        564 => "Kodak Ektapress PJ400",
        565 => "Kodak Ektapress PJ800",
        567 => "Kodak Portra 160NC",
        568 => "Kodak Portra 160VC",
        569 => "Kodak Portra 400NC",
        570 => "Kodak Portra 400VC",
        575 => "Kodak Advantix 100-2",
        576 => "Kodak Advantix 200-2",
        577 => "Kodak Advantix Black & White + 400",
        578 => "Kodak Ektapress PJ800-2",
        _ => "",
    }
}
