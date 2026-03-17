//! MakerNotes detection and parsing.
//!
//! Detects manufacturer-specific maker note headers and dispatches to
//! the appropriate tag table. Mirrors ExifTool's MakerNotes.pm.

use crate::metadata::exif::ByteOrderMark;
use crate::tag::{Tag, TagGroup, TagId};
use crate::tags::makernotes as mn_tags;
use crate::value::Value;

/// Manufacturer identification from maker note header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Manufacturer {
    Canon,
    Nikon,
    NikonOld,
    Sony,
    Pentax,
    Olympus,
    OlympusNew,
    Panasonic,
    Fujifilm,
    Samsung,
    Sigma,
    Casio,
    CasioType2,
    Ricoh,
    Minolta,
    Apple,
    Google,
    DJI,
    Unknown,
}

/// Result of detecting a maker note format.
struct MakerNoteInfo {
    manufacturer: Manufacturer,
    ifd_offset: usize,   // Offset to IFD start within maker note data
    _base_adjust: i64,    // Base offset adjustment for value pointers
    byte_order: Option<ByteOrderMark>, // Override byte order, or None for auto-detect
}

/// Parse maker notes from raw EXIF data.
///
/// `data` is the full TIFF data (from TIFF header start).
/// `mn_offset` is the offset to the MakerNote value within TIFF data.
/// `mn_size` is the size of the MakerNote value.
/// `make` is the camera Make string (for fallback detection).
/// `parent_byte_order` is the byte order of the parent EXIF structure.
pub fn parse_makernotes(
    data: &[u8],
    mn_offset: usize,
    mn_size: usize,
    make: &str,
    model: &str,
    parent_byte_order: ByteOrderMark,
) -> Vec<Tag> {
    if mn_size < 12 || mn_offset + mn_size > data.len() {
        return Vec::new();
    }

    let mn_data = &data[mn_offset..mn_offset + mn_size];

    // GE MakerNotes: FixBase needed (Perl emits Warning)
    if mn_data.starts_with(b"GE\0\0") || mn_data.starts_with(b"GENIC\0") {
        // GE offsets need FixBase which we don't implement — emit Warning like Perl
        let mut tags = Vec::new();
        tags.push(Tag {
            id: TagId::Text("Warning".into()),
            name: "Warning".into(), description: "Warning".into(),
            group: TagGroup { family0: "ExifTool".into(), family1: "ExifTool".into(), family2: "Other".into() },
            raw_value: Value::String("[minor] Suspicious MakerNotes offset for tag 0x0200".into()),
            print_value: "[minor] Suspicious MakerNotes offset for tag 0x0200".into(),
            priority: 0,
        });
        // Still parse what we can
        let info = detect_manufacturer(mn_data, make);
        let byte_order = info.byte_order.unwrap_or(parent_byte_order);
        let ifd_abs = mn_offset + info.ifd_offset;
        read_makernote_ifd(data, ifd_abs, byte_order, info.manufacturer, &mut tags, model);
        return tags;
    }

    // JVC Text format: "VER:xxxxQTY:yyyy..." — parse directly
    if mn_data.starts_with(b"VER:") {
        return decode_jvc_text(mn_data);
    }

    // Kodak binary: "KDK INFO" or "KDK" — not IFD, decode directly
    if mn_data.starts_with(b"KDK") {
        let start = if mn_data.starts_with(b"KDK INFO") { 8 } else { 8 };
        return decode_kodak_binary(&mn_data[start..]);
    }

    // Google HDRP: "HDRP\x02" or "HDRP\x03" — text-based MakerNote
    // (from Perl Google.pm: ProcessHDRPMakerNote — key:value lines)
    if mn_data.starts_with(b"HDRP") {
        return decode_google_hdrp(mn_data);
    }

    let info = detect_manufacturer(mn_data, make);

    let byte_order = info.byte_order.unwrap_or(parent_byte_order);

    // Calculate absolute IFD start in the full TIFF data
    let ifd_abs = mn_offset + info.ifd_offset;
    if ifd_abs + 2 > data.len() {
        return Vec::new();
    }

    // For manufacturers with self-contained TIFF headers (Nikon, Olympus new, Fuji),
    // we need to parse relative to their own TIFF header.
    // For others (Canon, Sony, Pentax, Panasonic), offsets are relative to the main TIFF header.
    let parse_data;
    let parse_offset;

    match info.manufacturer {
        Manufacturer::Nikon if info.ifd_offset >= 10 => {
            // Nikon type 2: has own TIFF header at mn_offset+10
            let tiff_start = mn_offset + 10;
            if tiff_start + 8 > data.len() {
                return Vec::new();
            }
            let sub = &data[tiff_start..(mn_offset + mn_size).min(data.len())];
            let ifd_off = read_u32(sub, 4, byte_order) as usize;
            parse_data = sub;
            parse_offset = ifd_off;
        }
        Manufacturer::Nikon => {
            // Headerless Nikon (Coolpix etc.): IFD directly, offsets relative to TIFF
            parse_data = data;
            parse_offset = mn_offset + info.ifd_offset;
        }
        Manufacturer::OlympusNew => {
            // OLYMPUS\0 + II/MM(2) + version(2) + IFD at byte 12
            // (from Perl: Start => '$valuePtr + 12', Base => '$start - 12')
            // Offsets in IFD are relative to start of MakerNote data
            parse_data = &data[mn_offset..(mn_offset + mn_size).min(data.len())];
            parse_offset = 12; // IFD directly at byte 12
        }
        Manufacturer::Apple => {
            // Apple iOS: IFD at mn_offset+14, offsets relative to mn_offset
            // (Start = valuePtr + 14, Base = start - 14)
            parse_data = &data[mn_offset..(mn_offset + mn_size).min(data.len())];
            parse_offset = 14; // IFD starts at offset 14 within MakerNote
        }
        Manufacturer::Fujifilm => {
            // FUJIFILM: IFD at OffsetPt (byte 8-11 LE), offsets relative to MN start
            // (from Perl: OffsetPt => '$valuePtr+8', Base => '$start')
            parse_data = &data[mn_offset..(mn_offset + mn_size).min(data.len())];
            parse_offset = info.ifd_offset; // = value read from bytes 8-11
        }
        _ => {
            // Default: offsets relative to main TIFF header
            // BUT: for Motorola, PENTAX\0, Leica5, ISL, SonyEricsson, Kyocera,
            // Olympus2/3 — offsets are relative to MakerNote start (Base = $start - N)
            // Detect by checking if ifd_offset matches a self-contained pattern
            let mn_bytes = &data[mn_offset..(mn_offset + mn_size).min(data.len())];
            let is_self_contained = mn_bytes.starts_with(b"MOT\0")
                || mn_bytes.starts_with(b"PENTAX \0")
                || mn_bytes.starts_with(b"KYOCERA")
                || mn_bytes.starts_with(b"ISLMAKERNOTE")
                || mn_bytes.starts_with(b"SEMC MS\0")
                || (mn_bytes.starts_with(b"LEICA\0") && mn_bytes.len() > 7
                    && (mn_bytes[7] == 1 || mn_bytes[7] == 4 || mn_bytes[7] == 5));

            if is_self_contained {
                parse_data = mn_bytes;
                parse_offset = info.ifd_offset;
            } else {
                parse_data = data;
                parse_offset = ifd_abs;
            }
        }
    }

    // Read IFD entries
    let mut tags = Vec::new();
    read_makernote_ifd(parse_data, parse_offset, byte_order, info.manufacturer, &mut tags, model);

    // Nikon second pass: decrypt encrypted sub-tables (only for type 2 with TIFF header)
    if info.manufacturer == Manufacturer::Nikon && info.ifd_offset >= 10 {
        decrypt_nikon_subtables(parse_data, parse_offset, byte_order, &mut tags, model);
    }

    tags
}

/// Decode Google HDRP MakerNote (text-based key:value from Perl Google.pm).
fn decode_google_hdrp(data: &[u8]) -> Vec<Tag> {
    let mut tags = Vec::new();
    // Skip HDRP header (first 4-5 bytes), then decompress/decode
    // The actual MakerNote text is base64-encoded, then gzipped, then protobuf.
    // But after decoding by Perl, the tags are text lines like "AndroidRelease: value"
    // In our MN data, the raw HDRP binary is complex. However, some Google cameras
    // store tags as plain text after the HDRP header.

    // Try to find text content after HDRP header
    let text = String::from_utf8_lossy(data);
    for line in text.lines() {
        if let Some(colon) = line.find(':') {
            let key = line[..colon].trim();
            let val = line[colon+1..].trim();
            if !key.is_empty() && !val.is_empty() && key.chars().all(|c| c.is_alphanumeric() || c == '_') {
                tags.push(Tag {
                    id: TagId::Text(key.to_string()),
                    name: key.to_string(), description: key.to_string(),
                    group: TagGroup { family0: "MakerNotes".into(), family1: "Google".into(), family2: "Camera".into() },
                    raw_value: Value::String(val.to_string()), print_value: val.to_string(), priority: 0,
                });
            }
        }
    }
    tags
}

/// Decode Canon CustomFunctions2 (from Perl CanonCustom.pm ProcessCanonCustom2).
fn decode_canon_custom_functions2(data: &[u8], bo: ByteOrderMark) -> Vec<Tag> {
    let mut tags = Vec::new();
    if data.len() < 8 { return tags; }

    let size = read_u16(data, 0, bo) as usize;
    // Size check: Perl validates size == data.len() but be lenient
    if size < 8 || data.len() < 8 { return tags; }

    let group_count = read_u32(data, 4, bo) as usize;
    let mut pos = 8;

    for _ in 0..group_count.min(20) {
        if pos + 12 > data.len() { break; }
        let _rec_num = read_u32(data, pos, bo);
        let rec_len = read_u32(data, pos + 4, bo) as usize;
        let rec_count = read_u32(data, pos + 8, bo) as usize;
        pos += 12;
        if rec_len < 8 { break; }
        let rec_end = pos + rec_len - 8;
        if rec_end > data.len() { break; }

        for _ in 0..rec_count.min(50) {
            if pos + 8 > rec_end { break; }
            let tag_id = read_u32(data, pos, bo);
            let num_vals = read_u32(data, pos + 4, bo) as usize;
            pos += 8;
            if pos + num_vals * 4 > rec_end { break; }

            let val = if num_vals > 0 && pos + 4 <= data.len() {
                read_u32(data, pos, bo)
            } else { 0 };

            // Look up tag name from CustomFunctions2 table
            let name = canon_custom2_name(tag_id);
            if !name.is_empty() {
                tags.push(mk_canon_str(name, &val.to_string()));
            } else if tag_id > 0 {
                // Emit unknown custom functions with their hex ID
                tags.push(mk_canon_str(
                    &format!("CustomFunc-0x{:04X}", tag_id),
                    &val.to_string(),
                ));
            }

            pos += num_vals * 4;
        }
    }
    tags
}

fn canon_custom2_name(id: u32) -> &'static str {
    match id {
        0x0101 => "ExposureLevelIncrements",
        0x0102 => "ISOSpeedIncrements",
        0x0103 => "ISOSpeedRange",
        0x0104 => "AEBAutoCancel",
        0x0105 => "AEBSequence",
        0x0106 => "AEBShotCount",
        0x0107 => "SpotMeterLinkToAFPoint",
        0x0108 => "SafetyShift",
        0x0109 => "UsableShootingModes",
        0x010A => "UsableMeteringModes",
        0x010B => "ExposureModeInManual",
        0x010C => "ShutterSpeedRange",
        0x010D => "ApertureRange",
        0x010E => "ApplyShootingMeteringMode",
        0x010F => "FlashSyncSpeedAv",
        0x0110 => "AEMicroadjustment",
        0x0111 => "FEMicroadjustment",
        0x0112 => "SameExposureForNewAperture",
        0x0113 => "ExposureCompAutoCancel",
        0x0114 => "AELockMeterModeAfterFocus",
        0x0201 => "LongExposureNoiseReduction",
        0x0202 => "HighISONoiseReduction",
        0x0203 => "HighlightTonePriority",
        0x0204 => "AutoLightingOptimizer",
        0x0304 => "ETTLII",
        0x0305 => "ShutterCurtainSync",
        0x0306 => "FlashFiring",
        0x0407 => "ViewInfoDuringExposure",
        0x0408 => "LCDIlluminationDuringBulb",
        0x0409 => "InfoButtonWhenShooting",
        0x040A => "ViewfinderWarnings",
        0x040B => "LVShootingAreaDisplay",
        0x040C => "LVShootingAreaDisplay",
        0x0501 => "USMLensElectronicMF",
        0x0502 => "AIServoTrackingSensitivity",
        0x0503 => "AIServoImagePriority",
        0x0504 => "AIServoTrackingMethod",
        0x0505 => "LensDriveNoAF",
        0x0506 => "LensAFStopButton",
        0x0507 => "AFMicroadjustment",
        0x0508 => "AFPointAreaExpansion",
        0x0509 => "SelectableAFPoint",
        0x050A => "SwitchToRegisteredAFPoint",
        0x050B => "AFPointAutoSelection",
        0x050C => "AFPointDisplayDuringFocus",
        0x050D => "AFPointBrightness",
        0x050E => "AFAssistBeam",
        0x050F => "AFPointSelectionMethod",
        0x0510 => "VFDisplayIllumination",
        0x0511 => "AFDuringLiveView",
        0x0512 => "SelectAFAreaSelectMode",
        0x0513 => "ManualAFPointSelectPattern",
        0x0514 => "DisplayAllAFPoints",
        0x0515 => "FocusDisplayAIServoAndMF",
        0x0516 => "OrientationLinkedAFPoint",
        0x0517 => "MultiControllerWhileMetering",
        0x0518 => "AccelerationTracking",
        0x0519 => "AIServoFirstImagePriority",
        0x051A => "AIServoSecondImagePriority",
        0x051B => "AFAreaSelectMethod",
        0x051C => "AutoAFPointColorTracking",
        0x051D => "VFDisplayIllumination",
        0x051E => "InitialAFPointAIServoAF",
        0x060F => "MirrorLockup",
        0x0610 => "ContinuousShootingSpeed",
        0x0611 => "ContinuousShotLimit",
        0x0612 => "RestrictDriveModes",
        0x0701 => "Shutter_AELock",
        0x0702 => "AFOnAELockButtonSwitch",
        0x0703 => "QuickControlDialInMeter",
        0x0704 => "SetButtonWhenShooting",
        0x0705 => "ManualTv",
        0x0706 => "DialDirectionTvAv",
        0x0707 => "AvSettingWithoutLens",
        0x0708 => "WBMediaImageSizeSetting",
        0x0709 => "LockMicrophoneButton",
        0x070A => "ButtonFunctionControlOff",
        0x070B => "AssignFuncButton",
        0x070C => "CustomControls",
        0x070D => "StartMovieShooting",
        0x070E => "FlashButtonFunction",
        0x070F => "MultiFunctionLock",
        0x0710 => "TrashButtonFunction",
        0x0711 => "ShutterReleaseWithoutLens",
        0x0712 => "ControlRingRotation",
        0x0713 => "FocusRingRotation",
        0x0714 => "RFLensMFFocusRingSensitivity",
        0x0715 => "CustomizeDials",
        0x080B => "FocusingScreen",
        0x080C => "TimerLength",
        0x080D => "ShortReleaseTimeLag",
        0x080E => "AddAspectRatioInfo",
        0x080F => "AddOriginalDecisionData",
        0x0810 => "LiveViewExposureSimulation",
        0x0811 => "LCDDisplayAtPowerOn",
        0x0812 => "MemoAudioQuality",
        0x0813 => "DefaultEraseOption",
        0x0814 => "RetractLensOnPowerOff",
        0x0815 => "AddIPTCInformation",
        0x0816 => "AudioCompression",
        _ => "",
    }
}

/// Decode Minolta CameraSettings (int32u format, from Perl Minolta.pm).
fn decode_minolta_camera_settings(data: &[u8], bo: ByteOrderMark) -> Vec<Tag> {
    let mut tags = Vec::new();
    let rd = |idx: usize| -> u32 {
        let off = idx * 4;
        if off + 4 > data.len() { return 0; }
        read_u32(data, off, bo)
    };
    let mk = |name: &str, val: String| Tag {
        id: TagId::Text(name.into()), name: name.into(), description: name.into(),
        group: TagGroup { family0: "MakerNotes".into(), family1: "Minolta".into(), family2: "Camera".into() },
        raw_value: Value::String(val.clone()), print_value: val, priority: 0,
    };

    static FIELDS: &[(usize, &str)] = &[
        (1, "ExposureMode"), (2, "FlashMode"), (3, "WhiteBalance"),
        (4, "MinoltaImageSize"), (5, "MinoltaQuality"), (6, "DriveMode"),
        (7, "MeteringMode"), (8, "ISO"), (9, "ExposureTime"), (10, "FNumber"),
        (11, "MacroMode"), (12, "DigitalZoom"), (13, "ExposureCompensation"),
        (14, "BracketStep"), (16, "IntervalLength"), (17, "IntervalNumber"),
        (18, "FocalLength"), (19, "FocusDistance"),
        (20, "FlashFired"), (21, "MinoltaDate"), (22, "MinoltaTime"),
        (23, "MaxAperture"), (26, "FileNumberMemory"), (27, "LastFileNumber"),
        (28, "ColorBalanceRed"), (29, "ColorBalanceGreen"), (30, "ColorBalanceBlue"),
        (31, "Saturation"), (32, "Contrast"), (33, "Sharpness"),
        (34, "SubjectProgram"), (35, "FlashExposureComp"), (36, "ISOSetting"),
        (37, "MinoltaModelID"), (38, "IntervalMode"), (39, "FolderName"),
        (40, "ColorMode"), (41, "ColorFilter"), (42, "BWFilter"),
        (43, "InternalFlash"), (44, "Brightness"),
        (45, "SpotFocusPointX"), (46, "SpotFocusPointY"),
        (47, "WideFocusZone"), (48, "FocusMode"),
        (49, "FocusArea"), (50, "DECPosition"),
        (52, "DataImprint"),
    ];

    let max_idx = data.len() / 4;
    for &(idx, name) in FIELDS {
        if idx < max_idx {
            let val = rd(idx);
            tags.push(mk(name, val.to_string()));
        }
    }
    tags
}

/// Decode Kodak binary MakerNotes (from Perl Kodak.pm, FORMAT=int8u mixed).
fn decode_kodak_binary(d: &[u8]) -> Vec<Tag> {
    let mut tags = Vec::new();
    let mk = |name: &str, val: String| Tag {
        id: TagId::Text(name.into()), name: name.into(), description: name.into(),
        group: TagGroup { family0: "MakerNotes".into(), family1: "Kodak".into(), family2: "Camera".into() },
        raw_value: Value::String(val.clone()), print_value: val, priority: 0,
    };

    if d.len() < 60 { return tags; }

    // From Perl Kodak::Main (byte offsets, big-endian)
    let model = String::from_utf8_lossy(&d[0..8]).trim_end_matches('\0').to_string();
    if !model.is_empty() { tags.push(mk("KodakModel", model)); }

    tags.push(mk("Quality", d[9].to_string()));
    tags.push(mk("BurstMode", d[10].to_string()));

    let w = u16::from_be_bytes([d[12], d[13]]);
    let h = u16::from_be_bytes([d[14], d[15]]);
    tags.push(mk("KodakImageWidth", w.to_string()));
    tags.push(mk("KodakImageHeight", h.to_string()));

    let year = u16::from_be_bytes([d[16], d[17]]);
    tags.push(mk("YearCreated", year.to_string()));
    tags.push(mk("MonthDayCreated", format!("{:02}:{:02}", d[18], d[19])));

    tags.push(mk("ShutterMode", d[27].to_string()));
    tags.push(mk("MeteringMode", d[28].to_string()));

    let fnum = u16::from_be_bytes([d[30], d[31]]);
    tags.push(mk("FNumber", format!("{:.1}", fnum as f64 / 100.0)));

    let exp = u32::from_be_bytes([d[32], d[33], d[34], d[35]]);
    if exp > 0 { tags.push(mk("ExposureTime", exp.to_string())); }

    let comp = i16::from_be_bytes([d[36], d[37]]);
    tags.push(mk("ExposureCompensation", comp.to_string()));

    tags.push(mk("FocusMode", d[56].to_string()));

    if d.len() > 58 {
        tags.push(mk("WhiteBalance", d[57].to_string()));
    }
    if d.len() > 72 {
        tags.push(mk("Sharpness", d[72].to_string()));
    }
    if d.len() > 77 {
        tags.push(mk("ISO", u16::from_be_bytes([d[76], d[77]]).to_string()));
    }
    if d.len() > 98 {
        tags.push(mk("TotalZoom", u16::from_be_bytes([d[96], d[97]]).to_string()));
        tags.push(mk("DateTimeStamp", d[98].to_string()));
    }
    if d.len() > 102 {
        tags.push(mk("ColorMode", u32::from_be_bytes([d[100], d[101], d[102], d[103]]).to_string()));
        tags.push(mk("DigitalZoom", u32::from_be_bytes([d[104], d[105], d[106], d[107]]).to_string()));
    }
    if d.len() > 109 {
        tags.push(mk("Sharpness2", d[108].to_string()));
    }
    if d.len() > 94 {
        tags.push(mk("FlashMode", d[92].to_string()));
        tags.push(mk("FlashFired", d[93].to_string()));
        tags.push(mk("ISOSetting", d[94].to_string()));
    }
    if d.len() > 112 {
        tags.push(mk("SequenceNumber", u32::from_be_bytes([d[108], d[109], d[110], d[111]]).to_string()));
    }

    tags
}

/// Decode JVC text-format MakerNotes ("VER:0100QTY:FINE...").
fn decode_jvc_text(data: &[u8]) -> Vec<Tag> {
    let mut tags = Vec::new();
    let text = String::from_utf8_lossy(data);

    // Parse KEY:VALUE pairs (3-letter key, 3-4 char value)
    let mut pos = 0;
    let bytes = text.as_bytes();
    while pos + 7 <= bytes.len() {
        // Key is uppercase letters until ':'
        let key_start = pos;
        while pos < bytes.len() && bytes[pos] != b':' { pos += 1; }
        if pos >= bytes.len() { break; }
        let key = &text[key_start..pos];
        pos += 1; // skip ':'

        // Value is next 4 bytes (or until next uppercase letter)
        let val_start = pos;
        while pos < bytes.len() && pos - val_start < 4 && !bytes[pos].is_ascii_uppercase() {
            pos += 1;
        }
        // Extend if still lowercase/digits
        while pos < bytes.len() && !bytes[pos].is_ascii_uppercase() && bytes[pos] != 0 {
            pos += 1;
        }
        let val = text[val_start..pos].trim_end_matches('\0').trim();

        let (name, print_val) = match key {
            "VER" => ("MakerNoteVersion", val.to_string()),
            "QTY" => ("Quality", match val {
                "STND" | "STD" => "Normal".to_string(),
                "FINE" => "Fine".to_string(),
                _ => val.to_string(),
            }),
            _ => continue,
        };

        tags.push(Tag {
            id: TagId::Text(name.to_string()),
            name: name.to_string(),
            description: name.to_string(),
            group: TagGroup { family0: "MakerNotes".into(), family1: "JVC".into(), family2: "Camera".into() },
            raw_value: Value::String(val.to_string()),
            print_value: print_val,
            priority: 0,
        });
    }

    tags
}

/// Generic binary sub-table decoder: extract int8u values at fixed offsets.
fn decode_binary_subtable(data: &[u8], module: &str, table: &[(usize, &str)]) -> Vec<Tag> {
    let mut tags = Vec::new();
    for &(offset, name) in table {
        if offset < data.len() {
            let val = data[offset];
            tags.push(Tag {
                id: TagId::Text(name.to_string()),
                name: name.to_string(), description: name.to_string(),
                group: TagGroup { family0: "MakerNotes".into(), family1: module.into(), family2: "Camera".into() },
                raw_value: Value::U8(val), print_value: val.to_string(), priority: 0,
            });
        }
    }
    tags
}

// Pentax binary sub-tables (from Perl Pentax.pm)
static PENTAX_SR_INFO: &[(usize, &str)] = &[(0, "SRResult"), (1, "ShakeReduction"), (2, "SRHalfPressTime"), (3, "SRFocalLength")];
static PENTAX_AE_INFO: &[(usize, &str)] = &[
    (0, "AEExposureTime"), (1, "AEAperture"), (2, "AE_ISO"), (3, "AEXv"),
    (4, "AEBXv"), (5, "AEMinExposureTime"), (6, "AEProgramMode"),
    (8, "AEApertureSteps"), (9, "AEMaxAperture"), (10, "AEMaxAperture2"),
    (11, "AEMinAperture"), (12, "AEMeteringMode"),
];
static PENTAX_AF_INFO: &[(usize, &str)] = &[
    (4, "AFPredictor"), (7, "AFIntegrationTime"), (11, "AFPointsInFocus"),
];
static PENTAX_LENS_INFO: &[(usize, &str)] = &[
    (0, "LensType"), (3, "LensData"),
];
static PENTAX_FLASH_INFO: &[(usize, &str)] = &[
    (0, "FlashStatus"), (1, "InternalFlashMode"), (2, "ExternalFlashMode"),
    (3, "InternalFlashStrength"), (25, "ExternalFlashExposureComp"),
    (26, "ExternalFlashBounce"),
];
static PENTAX_CAMERA_SETTINGS: &[(usize, &str)] = &[
    (0, "PictureMode2"), (2, "FlashOptions"), (3, "AFPointMode"),
    (4, "AFPointSelected2"), (6, "ISOFloor"), (7, "DriveMode2"),
    (8, "ExposureBracketStepSize"), (9, "BracketShotNumber"),
    (10, "WhiteBalanceSet"), (16, "FlashOptions2"),
];
static PENTAX_CAMERA_INFO: &[(usize, &str)] = &[
    // int32u format — offsets are word indices, not byte indices
];
static PENTAX_BATTERY_INFO: &[(usize, &str)] = &[
    (2, "BodyBatteryADNoLoad"), (3, "BodyBatteryADLoad"),
    (4, "GripBatteryADNoLoad"), (5, "GripBatteryADLoad"),
];

/// Decode Apple RunTime binary plist (tag 0x0003).
fn decode_apple_runtime(data: &[u8]) -> Vec<Tag> {
    let mut tags = Vec::new();

    if let Some(dict) = crate::formats::plist::parse_binary_plist(data) {
        use crate::formats::plist::PlistValue;

        if let Some(PlistValue::Int(v)) = dict.get("flags") {
            let flag_str = match *v {
                1 => "Valid",
                3 => "Valid, Has been rounded",
                _ => "",
            };
            let print = if flag_str.is_empty() { v.to_string() } else { flag_str.to_string() };
            tags.push(Tag {
                id: TagId::Text("RunTimeFlags".into()), name: "RunTimeFlags".into(),
                description: "Run Time Flags".into(),
                group: TagGroup { family0: "MakerNotes".into(), family1: "Apple".into(), family2: "Image".into() },
                raw_value: Value::I32(*v as i32), print_value: print, priority: 0,
            });
        }
        if let Some(PlistValue::Int(v)) = dict.get("value") {
            tags.push(Tag {
                id: TagId::Text("RunTimeValue".into()), name: "RunTimeValue".into(),
                description: "Run Time Value".into(),
                group: TagGroup { family0: "MakerNotes".into(), family1: "Apple".into(), family2: "Image".into() },
                raw_value: Value::String(v.to_string()), print_value: v.to_string(), priority: 0,
            });
        }
        if let Some(PlistValue::Int(v)) = dict.get("epoch") {
            tags.push(Tag {
                id: TagId::Text("RunTimeEpoch".into()), name: "RunTimeEpoch".into(),
                description: "Run Time Epoch".into(),
                group: TagGroup { family0: "MakerNotes".into(), family1: "Apple".into(), family2: "Image".into() },
                raw_value: Value::I32(*v as i32), print_value: v.to_string(), priority: 0,
            });
        }
        if let Some(PlistValue::Int(v)) = dict.get("timescale") {
            tags.push(Tag {
                id: TagId::Text("RunTimeScale".into()), name: "RunTimeScale".into(),
                description: "Run Time Scale".into(),
                group: TagGroup { family0: "MakerNotes".into(), family1: "Apple".into(), family2: "Image".into() },
                raw_value: Value::String(v.to_string()), print_value: v.to_string(), priority: 0,
            });

            // RunTimeSincePowerUp composite
            if let Some(PlistValue::Int(value)) = dict.get("value") {
                if *v > 0 {
                    let secs = *value as f64 / *v as f64;
                    let h = (secs / 3600.0) as u32;
                    let m = ((secs % 3600.0) / 60.0) as u32;
                    let s = secs % 60.0;
                    tags.push(Tag {
                        id: TagId::Text("RunTimeSincePowerUp".into()),
                        name: "RunTimeSincePowerUp".into(),
                        description: "Run Time Since Power Up".into(),
                        group: TagGroup { family0: "Composite".into(), family1: "Composite".into(), family2: "Image".into() },
                        raw_value: Value::String(format!("{:.0}", secs)),
                        print_value: format!("{}:{:02}:{:02}", h, m, s as u32),
                        priority: 0,
                    });
                }
            }
        }
    }

    tags
}

/// Decode a PreviewIFD sub-directory — extract PreviewImageStart/Length.
fn decode_preview_ifd(data: &[u8], offset: usize, bo: ByteOrderMark) -> Vec<Tag> {
    let mut tags = Vec::new();
    if offset + 2 > data.len() { return tags; }

    let count = read_u16(data, offset, bo) as usize;
    for i in 0..count.min(20) {
        let eoff = offset + 2 + i * 12;
        if eoff + 12 > data.len() { break; }
        let tag_id = read_u16(data, eoff, bo);
        let val = read_u32(data, eoff + 8, bo);

        match tag_id {
            0x0201 => {
                tags.push(mk_nikon_str("PreviewImageStart", &val.to_string()));
            }
            0x0202 => {
                tags.push(mk_nikon_str("PreviewImageLength", &val.to_string()));
                // Also emit PreviewImage as binary marker
                if val > 0 {
                    tags.push(Tag {
                        id: TagId::Text("PreviewImage".into()),
                        name: "PreviewImage".into(),
                        description: "Preview Image".into(),
                        group: TagGroup { family0: "MakerNotes".into(), family1: "PreviewIFD".into(), family2: "Image".into() },
                        raw_value: Value::Binary(Vec::new()), // placeholder
                        print_value: format!("(Binary data {} bytes)", val),
                        priority: 0,
                    });
                }
            }
            _ => {}
        }
    }
    tags
}

/// Decode Nikon AFInfo (tag 0x0088).
fn decode_nikon_afinfo(data: &[u8], _bo: ByteOrderMark) -> Vec<Tag> {
    let mut tags = Vec::new();
    if data.len() < 4 { return tags; }

    // AFAreaMode (byte 0)
    let af_area = match data[0] {
        0 => "Single Area",
        1 => "Dynamic Area",
        2 => "Dynamic Area (closest subject)",
        3 => "Group Dynamic",
        4 => "Single Area (wide)",
        5 => "Dynamic Area (wide)",
        _ => "",
    };
    if !af_area.is_empty() {
        tags.push(mk_nikon_str("AFAreaMode", af_area));
    }

    // AFPoint (byte 1)
    let af_point = match data[1] {
        0 => "Center",
        1 => "Top",
        2 => "Bottom",
        3 => "Mid-left",
        4 => "Mid-right",
        5 => "Upper-left",
        6 => "Upper-right",
        7 => "Lower-left",
        8 => "Lower-right",
        9 => "Far Left",
        10 => "Far Right",
        _ => "",
    };
    if !af_point.is_empty() {
        tags.push(mk_nikon_str("AFPoint", af_point));
    }

    // AFPointsInFocus (bytes 2-3, bitmask for 7/11 points)
    if data.len() >= 4 {
        let mask = u16::from_le_bytes([data[2], data[3]]);
        let points: Vec<&str> = (0..11).filter(|&i| mask & (1 << i) != 0).map(|i| match i {
            0 => "Center",
            1 => "Top",
            2 => "Bottom",
            3 => "Mid-left",
            4 => "Mid-right",
            5 => "Upper-left",
            6 => "Upper-right",
            7 => "Lower-left",
            8 => "Lower-right",
            9 => "Far Left",
            10 => "Far Right",
            _ => "",
        }).collect();
        let pv = if points.is_empty() { "(none)".to_string() } else { points.join(", ") };
        tags.push(mk_nikon_str("AFPointsInFocus", &pv));
    }

    tags
}

/// Decode Nikon FlashInfo (tag 0x00A8).
fn decode_nikon_flashinfo(data: &[u8], _bo: ByteOrderMark) -> Vec<Tag> {
    let mut tags = Vec::new();
    if data.len() < 5 { return tags; }

    // Version (first 4 bytes ASCII)
    let version = std::str::from_utf8(&data[..4]).unwrap_or("");
    tags.push(mk_nikon_str("FlashInfoVersion", version));

    if data.len() >= 15 {
        // FlashSource (byte 4)
        let source = match data[4] {
            0 => "None",
            1 => "External",
            2 => "Internal",
            _ => "",
        };
        if !source.is_empty() {
            tags.push(mk_nikon_str("FlashSource", source));
        }

        // ExternalFlashFirmware (bytes 6-7)
        if data[6] > 0 {
            tags.push(mk_nikon_str("ExternalFlashFirmware",
                &format!("{}.{:02}", data[6], data[7])));
        }

        // ExternalFlashFlags (byte 8)
        if data[8] != 0 {
            tags.push(mk_nikon_str("ExternalFlashFlags",
                &format!("0x{:02X}", data[8])));
        }

        // FlashCommanderMode (byte 9, in some versions)
        if data.len() > 9 {
            let cmd = match data[9] & 0x80 {
                0 => "Off",
                _ => "On",
            };
            tags.push(mk_nikon_str("FlashCommanderMode", cmd));
        }

        // FlashControlMode (byte 10)
        if data.len() > 10 {
            let mode = match data[10] & 0x0F {
                0 => "Off",
                1 => "iTTL-BL",
                2 => "iTTL",
                3 => "Auto Aperture",
                4 => "Automatic",
                5 => "GN (distance priority)",
                6 => "Manual",
                7 => "Repeating Flash",
                _ => "",
            };
            if !mode.is_empty() {
                tags.push(mk_nikon_str("FlashControlMode", mode));
            }
        }

        // FlashCompensation (byte 10 high nibble)
        if data.len() > 10 {
            let comp = (data[10] >> 4) as i8;
            let ev = comp as f64 / 6.0;
            tags.push(mk_nikon_str("FlashCompensation", &format!("{}", ev)));
        }

        // ExternalFlashFlags (byte 8)
        if data.len() > 8 {
            let flags = data[8];
            let flag_str = if flags == 0 { "(none)".to_string() } else { format!("0x{:02X}", flags) };
            tags.push(mk_nikon_str("ExternalFlashFlags", &flag_str));
        }

        // FlashGNDistance (byte 14)
        if data.len() > 14 {
            tags.push(mk_nikon_str("FlashGNDistance", &format!("{}", data[14])));
        }

        // Flash group control modes (bytes 15-18 if available)
        if data.len() > 15 {
            let grp_a = match data[15] & 0x0F {
                0 => "Off",
                1 => "iTTL-BL",
                2 => "iTTL",
                3 => "Auto Aperture",
                6 => "Manual",
                _ => "",
            };
            if !grp_a.is_empty() {
                tags.push(mk_nikon_str("FlashGroupAControlMode", grp_a));
            }
        }
        if data.len() > 16 {
            let grp_b = match data[16] & 0x0F {
                0 => "Off",
                1 => "iTTL-BL",
                2 => "iTTL",
                3 => "Auto Aperture",
                6 => "Manual",
                _ => "",
            };
            if !grp_b.is_empty() {
                tags.push(mk_nikon_str("FlashGroupBControlMode", grp_b));
            }
        }

        // Compensation values (emit even when 0)
        if data.len() > 17 {
            let comp_a = (data[17] >> 4) as i8;
            tags.push(mk_nikon_str("FlashGroupACompensation", &format!("{}", comp_a as f64 / 6.0)));
        }
        if data.len() > 18 {
            let comp_b = (data[18] >> 4) as i8;
            tags.push(mk_nikon_str("FlashGroupBCompensation", &format!("{}", comp_b as f64 / 6.0)));
        }
    }

    tags
}

/// Decode Nikon ColorBalance (tag 0x0097).
/// Version 0103 (D70): WB_RGBGLevels at offset 20, 4 × int16u
fn decode_nikon_color_balance(data: &[u8], bo: ByteOrderMark) -> Vec<Tag> {
    let mut tags = Vec::new();
    if data.len() < 4 { return tags; }

    let version = std::str::from_utf8(&data[..4]).unwrap_or("");

    match version {
        "0103" => {
            // D70: WB at offset 20, 4 × int16u (R, G1, B, G2)
            if data.len() >= 28 {
                let r = read_u16(data, 20, bo);
                let g = read_u16(data, 22, bo);
                let b = read_u16(data, 24, bo);
                let g2 = read_u16(data, 26, bo);
                tags.push(mk_nikon_str("WB_RGBGLevels",
                    &format!("{} {} {} {}", r, g, b, g2)));
            }
        }
        "0100" => {
            // D100: WB at offset 72, same format
            if data.len() >= 80 {
                let r = read_u16(data, 72, bo);
                let g = read_u16(data, 74, bo);
                let b = read_u16(data, 76, bo);
                let g2 = read_u16(data, 78, bo);
                tags.push(mk_nikon_str("WB_RGBGLevels",
                    &format!("{} {} {} {}", r, g, b, g2)));
            }
        }
        "0102" => {
            // D2H: WB at offset 6, same format
            if data.len() >= 14 {
                let r = read_u16(data, 6, bo);
                let g = read_u16(data, 8, bo);
                let b = read_u16(data, 10, bo);
                let g2 = read_u16(data, 12, bo);
                tags.push(mk_nikon_str("WB_RGBGLevels",
                    &format!("{} {} {} {}", r, g, b, g2)));
            }
        }
        _ => {} // Encrypted versions handled by decrypt_nikon_subtables
    }

    tags
}

fn mk_nikon_str(name: &str, value: &str) -> Tag {
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(), description: name.to_string(),
        group: TagGroup { family0: "MakerNotes".into(), family1: "Nikon".into(), family2: "Camera".into() },
        raw_value: Value::String(value.to_string()), print_value: value.to_string(), priority: 0,
    }
}

/// Decrypt Nikon encrypted sub-tables (ShotInfo, LensData, FlashInfo).
/// Uses SerialNumber + ShutterCount extracted from previously parsed tags.
fn decrypt_nikon_subtables(
    data: &[u8],
    ifd_offset: usize,
    byte_order: ByteOrderMark,
    tags: &mut Vec<Tag>,
    model: &str,
) {
    // Extract decryption keys from already-parsed tags
    // Mirrors Perl's SerialKey() function from Nikon.pm
    let serial_str = tags.iter()
        .find(|t| t.name == "SerialNumber" || t.name == "SerialNumber2")
        .map(|t| t.print_value.clone())
        .unwrap_or_default();
    let shutter_count = tags.iter()
        .find(|t| t.name == "ShutterCount")
        .and_then(|t| t.raw_value.as_u64())
        .unwrap_or(0) as u32;

    // SerialKey(): use serial if purely numeric, else fixed values per model
    // (mirrors Perl Nikon.pm SerialKey function)
    let serial: u32 = if serial_str.trim().chars().all(|c| c.is_ascii_digit()) && !serial_str.is_empty() {
        serial_str.trim().parse().unwrap_or(0)
    } else if model.contains("D50") {
        0x22
    } else {
        0x60 // D200, D40X, D70, D80, etc.
    };

    if shutter_count == 0 {
        return; // Can't decrypt without shutter count
    }

    // Scan IFD for encrypted tags and decrypt them
    if ifd_offset + 2 > data.len() { return; }
    let entry_count = read_u16(data, ifd_offset, byte_order) as usize;

    for i in 0..entry_count {
        let eoff = ifd_offset + 2 + i * 12;
        if eoff + 12 > data.len() { break; }

        let tag_id = read_u16(data, eoff, byte_order);
        let data_type = read_u16(data, eoff + 2, byte_order);
        let count = read_u32(data, eoff + 4, byte_order) as usize;

        let type_size = match data_type {
            1 | 2 | 6 | 7 => 1, 3 | 8 => 2, 4 | 9 | 11 | 13 => 4, 5 | 10 | 12 => 8, _ => 1,
        };
        let total_size = type_size * count;
        if total_size <= 4 { continue; }

        let value_offset = read_u32(data, eoff + 8, byte_order) as usize;
        if value_offset + total_size > data.len() { continue; }

        match tag_id {
            0x0091 => {
                // ShotInfo: decrypt and extract ShutterCount etc.
                let mut decrypted = data[value_offset..value_offset + total_size].to_vec();
                crate::metadata::nikon_decrypt::nikon_decrypt(&mut decrypted, serial, shutter_count, 4);

                // Extract version prefix (unencrypted first 4 bytes)
                let version = std::str::from_utf8(&data[value_offset..value_offset + 4]).unwrap_or("");
                tags.push(mk_canon_str("ShotInfoVersion", version));

                // Decrypt reveals ShotInfo fields depending on version
                // For now, extract what we can
            }
            0x00A8 => {
                // FlashInfo: decrypt and decode
                let mut decrypted = data[value_offset..value_offset + total_size].to_vec();
                crate::metadata::nikon_decrypt::nikon_decrypt(&mut decrypted, serial, shutter_count, 4);

                // Extract FlashInfo version (first 4 bytes unencrypted)
                if total_size >= 4 {
                    let fi_ver = std::str::from_utf8(&data[value_offset..value_offset + 4]).unwrap_or("");
                    tags.push(mk_canon_str("FlashInfoVersion", fi_ver));
                }

                // Decode FlashInfo fields
                if decrypted.len() >= 10 {
                    let flash_source = match decrypted[4] {
                        0 => "None",
                        1 => "External",
                        2 => "Internal",
                        _ => "",
                    };
                    if !flash_source.is_empty() {
                        tags.push(mk_canon_str("FlashSource", flash_source));
                    }

                    // FlashFirmware at offset 6
                    if decrypted.len() >= 8 {
                        let fw_major = decrypted[6];
                        let fw_minor = decrypted[7];
                        if fw_major > 0 {
                            tags.push(mk_canon_str("ExternalFlashFirmware",
                                &format!("{}.{}", fw_major, fw_minor)));
                        }
                    }
                }
            }
            0x0098 => {
                // LensData: decrypt if version 02xx+, then decode using LensData01 offsets
                let ver = std::str::from_utf8(&data[value_offset..value_offset + 4.min(data.len() - value_offset)]).unwrap_or("");
                if ver.starts_with("02") || ver.starts_with("04") || ver.starts_with("08") {
                    let mut decrypted = data[value_offset..value_offset + total_size].to_vec();
                    crate::metadata::nikon_decrypt::nikon_decrypt(&mut decrypted, serial, shutter_count, 4);
                    // After decryption, decode directly using LensData01 offsets
                    // (same structure as unencrypted 0101, just with encryption removed)
                    tags.push(mk_nikon_str("LensDataVersion", ver));
                    let d = &decrypted;
                    if d.len() >= 0x12 {
                        // Offsets from Perl LensData01 table
                        if d[4] > 0 {
                            let ep = 2048.0 / d[4] as f64;
                            tags.push(mk_nikon_str("ExitPupilPosition", &format!("{:.1}", ep)));
                        }
                        if d[5] > 0 {
                            let ap = 2.0_f64.powf(d[5] as f64 / 24.0);
                            tags.push(mk_nikon_str("AFAperture", &format!("{:.1}", ap)));
                        }
                        if d[8] > 0 { tags.push(mk_nikon_str("FocusPosition", &format!("0x{:02X}", d[8]))); }
                        if d[9] > 0 {
                            let dist = 0.01 * 10.0_f64.powf(d[9] as f64 / 40.0);
                            tags.push(mk_nikon_str("FocusDistance", &format!("{:.2} m", dist)));
                        }
                        if d.len() > 0x0A { tags.push(mk_nikon_str("MCUVersion", &d[0x0A].to_string())); }
                        if d.len() > 0x0B { tags.push(mk_nikon_str("LensIDNumber", &d[0x0B].to_string())); }
                        if d.len() > 0x0D && d[0x0D] > 0 {
                            let fl = 5.0 * 2.0_f64.powf(d[0x0D] as f64 / 24.0);
                            tags.push(mk_nikon_str("MinFocalLength", &format!("{:.1}", fl)));
                        }
                        if d.len() > 0x0E && d[0x0E] > 0 {
                            let fl = 5.0 * 2.0_f64.powf(d[0x0E] as f64 / 24.0);
                            tags.push(mk_nikon_str("MaxFocalLength", &format!("{:.1}", fl)));
                        }
                        if d.len() > 0x0F && d[0x0F] > 0 {
                            let ap = 2.0_f64.powf(d[0x0F] as f64 / 24.0);
                            tags.push(mk_nikon_str("MaxApertureAtMinFocal", &format!("{:.1}", ap)));
                        }
                        if d.len() > 0x10 && d[0x10] > 0 {
                            let ap = 2.0_f64.powf(d[0x10] as f64 / 24.0);
                            tags.push(mk_nikon_str("MaxApertureAtMaxFocal", &format!("{:.1}", ap)));
                        }
                        if d.len() > 0x11 && d[0x11] > 0 {
                            let ap = 2.0_f64.powf(d[0x11] as f64 / 24.0);
                            tags.push(mk_nikon_str("EffectiveMaxAperture", &format!("{:.1}", ap)));
                        }
                    }
                }
            }
            0x0097 => {
                // ColorBalance: decrypt if version 02xx+
                // Perl: ColorBalance02 uses DecryptStart=>4, DirOffset=>6
                // WB_RGGBLevels at offset 4+6=10 (4 × int16u)
                let ver = std::str::from_utf8(&data[value_offset..value_offset + 4.min(data.len() - value_offset)]).unwrap_or("");
                if ver.starts_with("02") {
                    let mut decrypted = data[value_offset..value_offset + total_size].to_vec();
                    crate::metadata::nikon_decrypt::nikon_decrypt(&mut decrypted, serial, shutter_count, 4);
                    // WB_RGGBLevels at offset 10 (DecryptStart=4 + DirOffset=6)
                    if decrypted.len() >= 18 {
                        let off = 10;
                        let r = u16::from_le_bytes([decrypted[off], decrypted[off+1]]);
                        let g1 = u16::from_le_bytes([decrypted[off+2], decrypted[off+3]]);
                        let g2 = u16::from_le_bytes([decrypted[off+4], decrypted[off+5]]);
                        let b = u16::from_le_bytes([decrypted[off+6], decrypted[off+7]]);
                        tags.push(mk_nikon_str("WB_RGGBLevels",
                            &format!("{} {} {} {}", r, g1, g2, b)));
                    }
                }
            }
            _ => {}
        }
    }
}

/// Detect manufacturer from maker note header bytes.
fn detect_manufacturer(mn_data: &[u8], make: &str) -> MakerNoteInfo {
    let make_upper = make.to_uppercase();

    // Nikon type 2: "Nikon\0\x02\x10\0\0" followed by TIFF header at offset 10
    if mn_data.len() >= 18 && mn_data.starts_with(b"Nikon\0\x02") {
        return MakerNoteInfo {
            manufacturer: Manufacturer::Nikon,
            ifd_offset: 18, // Skip Nikon header(10) + TIFF header(8)
            _base_adjust: 0,
            byte_order: detect_tiff_byte_order(&mn_data[10..]),
        };
    }

    // Nikon type 1: "Nikon\0\x01\0"
    if mn_data.starts_with(b"Nikon\0\x01") {
        return MakerNoteInfo {
            manufacturer: Manufacturer::NikonOld,
            ifd_offset: 8,
            _base_adjust: 0,
            byte_order: Some(ByteOrderMark::BigEndian),
        };
    }

    // OLYMPUS\0II or OLYMPUS\0MM (new format)
    if mn_data.len() >= 12 && mn_data.starts_with(b"OLYMPUS\0") {
        return MakerNoteInfo {
            manufacturer: Manufacturer::OlympusNew,
            ifd_offset: 12,
            _base_adjust: 0,
            byte_order: detect_tiff_byte_order(&mn_data[8..]),
        };
    }

    // OM SYSTEM\0
    if mn_data.len() >= 16 && mn_data.starts_with(b"OM SYSTEM\0") {
        return MakerNoteInfo {
            manufacturer: Manufacturer::OlympusNew,
            ifd_offset: 16,
            _base_adjust: 0,
            byte_order: detect_tiff_byte_order(&mn_data[12..]),
        };
    }

    // OLYMP\0 or EPSON\0 (old format)
    if mn_data.starts_with(b"OLYMP\0") || mn_data.starts_with(b"EPSON\0") {
        return MakerNoteInfo {
            manufacturer: Manufacturer::Olympus,
            ifd_offset: 8,
            _base_adjust: 0,
            byte_order: None,
        };
    }

    // FUJIFILM (8 bytes, then 4-byte LE offset to IFD)
    if mn_data.len() >= 12 && (mn_data.starts_with(b"FUJIFILM") || mn_data.starts_with(b"GENERALE")) {
        let ifd_off = u32::from_le_bytes([mn_data[8], mn_data[9], mn_data[10], mn_data[11]]) as usize;
        return MakerNoteInfo {
            manufacturer: Manufacturer::Fujifilm,
            ifd_offset: ifd_off,
            _base_adjust: 0,
            byte_order: Some(ByteOrderMark::LittleEndian),
        };
    }

    // Sony DSC/CAM/MOBILE
    if mn_data.starts_with(b"SONY DSC") || mn_data.starts_with(b"SONY CAM")
        || mn_data.starts_with(b"SONY MOBILE")
    {
        return MakerNoteInfo {
            manufacturer: Manufacturer::Sony,
            ifd_offset: 12,
            _base_adjust: 0,
            byte_order: None,
        };
    }

    // Panasonic\0
    if mn_data.starts_with(b"Panasonic\0") {
        return MakerNoteInfo {
            manufacturer: Manufacturer::Panasonic,
            ifd_offset: 12,
            _base_adjust: 0,
            byte_order: None,
        };
    }

    // Sanyo: "SANYO\0" (6 bytes) + 2 padding + IFD
    // (from Perl: Start => '$valuePtr + 8')
    if mn_data.starts_with(b"SANYO\0") {
        return MakerNoteInfo {
            manufacturer: Manufacturer::Unknown,
            ifd_offset: 8,
            _base_adjust: 0,
            byte_order: None,
        };
    }

    // Casio Type 2: "QVC\0" or "DCI\0"
    // (from Perl: Start => '$valuePtr + 6')
    if mn_data.starts_with(b"QVC\0") || mn_data.starts_with(b"DCI\0") {
        return MakerNoteInfo {
            manufacturer: Manufacturer::CasioType2,
            ifd_offset: 6,
            _base_adjust: 0,
            byte_order: None,
        };
    }

    // Kodak: "KDK INFO" — NOT an IFD, binary format
    if mn_data.starts_with(b"KDK INFO") {
        // Kodak uses binary data, not IFD — handled separately
        return MakerNoteInfo {
            manufacturer: Manufacturer::Unknown,
            ifd_offset: 0, // special marker for non-IFD
            _base_adjust: 0,
            byte_order: Some(ByteOrderMark::BigEndian),
        };
    }

    // Ricoh: "RICOH\0\0\0" (8 bytes) + IFD
    // (from Perl MakerNotes.pm: Start => '$valuePtr + 8')
    if mn_data.starts_with(b"Ricoh") || mn_data.starts_with(b"RICOH") {
        return MakerNoteInfo {
            manufacturer: Manufacturer::Ricoh,
            ifd_offset: 8,
            _base_adjust: 0,
            byte_order: None,
        };
    }

    // GE: "GE\0\0" or "GENIC\0", Start => valuePtr + 18
    if mn_data.starts_with(b"GE\0\0") || mn_data.starts_with(b"GENIC\0") {
        return MakerNoteInfo {
            manufacturer: Manufacturer::Unknown,
            ifd_offset: 18,
            _base_adjust: 0,
            byte_order: None,
        };
    }

    // Motorola: "MOT\0", Start => valuePtr + 8, Base => start - 8
    if mn_data.starts_with(b"MOT\0") {
        return MakerNoteInfo {
            manufacturer: Manufacturer::Unknown,
            ifd_offset: 8,
            _base_adjust: 0,
            byte_order: None,
        };
    }

    // Sony PIC: "SONY PIC\0" — offset 12
    if mn_data.starts_with(b"SONY PIC\0") {
        return MakerNoteInfo {
            manufacturer: Manufacturer::Sony,
            ifd_offset: 12,
            _base_adjust: 0,
            byte_order: None,
        };
    }
    // Sony PI: "SONY PI\0" — offset 12
    if mn_data.starts_with(b"SONY PI\0") {
        return MakerNoteInfo {
            manufacturer: Manufacturer::Sony,
            ifd_offset: 12,
            _base_adjust: 0,
            byte_order: None,
        };
    }
    // Sigma: "SIGMA\0\0\0" or "FOVEON\0\0" — offset 10
    if mn_data.starts_with(b"SIGMA\0") || mn_data.starts_with(b"FOVEON\0") {
        return MakerNoteInfo {
            manufacturer: Manufacturer::Sigma,
            ifd_offset: 10,
            _base_adjust: 0,
            byte_order: None,
        };
    }
    // PENTAX \0 (new) — offset 10, self-contained
    if mn_data.starts_with(b"PENTAX \0") {
        return MakerNoteInfo {
            manufacturer: Manufacturer::Pentax,
            ifd_offset: 10,
            _base_adjust: 0,
            byte_order: detect_tiff_byte_order(&mn_data[6..]),
        };
    }
    // LEICA\0 with various subtypes
    if mn_data.starts_with(b"LEICA\0") && mn_data.len() >= 8 {
        return MakerNoteInfo {
            manufacturer: Manufacturer::Panasonic, // Leica uses Panasonic tables
            ifd_offset: 8,
            _base_adjust: 0,
            byte_order: None,
        };
    }
    // LEICA CAMERA AG\0
    if mn_data.starts_with(b"LEICA CAMERA AG\0") {
        return MakerNoteInfo {
            manufacturer: Manufacturer::Panasonic,
            ifd_offset: 18,
            _base_adjust: 0,
            byte_order: None,
        };
    }
    // Kyocera: "KYOCERA\0" — offset 22, base = start+2
    if mn_data.starts_with(b"KYOCERA") {
        return MakerNoteInfo {
            manufacturer: Manufacturer::Unknown,
            ifd_offset: 22,
            _base_adjust: 0,
            byte_order: None,
        };
    }
    // ISL: "ISLMAKERNOTE000\0"
    if mn_data.starts_with(b"ISLMAKERNOTE") {
        return MakerNoteInfo {
            manufacturer: Manufacturer::Unknown,
            ifd_offset: 24,
            _base_adjust: 0,
            byte_order: None,
        };
    }
    // Sony Ericsson: "SEMC MS\0"
    if mn_data.starts_with(b"SEMC MS\0") {
        return MakerNoteInfo {
            manufacturer: Manufacturer::Sony,
            ifd_offset: 20,
            _base_adjust: 0,
            byte_order: None,
        };
    }
    // HP: "Hewlett-Packard" or "Vivitar"
    if mn_data.starts_with(b"Hewlett-Packard") || mn_data.starts_with(b"Vivitar") {
        return MakerNoteInfo {
            manufacturer: Manufacturer::Unknown,
            ifd_offset: 0,
            _base_adjust: 0,
            byte_order: None,
        };
    }
    // Samsung: "SAMSUNG" or headerless with Make
    if mn_data.starts_with(b"SAMSUNG") {
        return MakerNoteInfo {
            manufacturer: Manufacturer::Samsung,
            ifd_offset: 0,
            _base_adjust: 0,
            byte_order: None,
        };
    }
    // Ricoh-Pentax: "RICOH\0II" or "RICOH\0MM"
    if mn_data.len() >= 8 && mn_data.starts_with(b"RICOH\0") &&
        (mn_data[6] == b'I' || mn_data[6] == b'M') {
        return MakerNoteInfo {
            manufacturer: Manufacturer::Pentax,
            ifd_offset: 8,
            _base_adjust: 0,
            byte_order: detect_tiff_byte_order(&mn_data[6..]),
        };
    }

    // JVC: "JVC " (4 bytes) + IFD
    // (from Perl MakerNotes.pm: Start => '$valuePtr + 4')
    if mn_data.starts_with(b"JVC ") {
        return MakerNoteInfo {
            manufacturer: Manufacturer::Unknown,
            ifd_offset: 4,
            _base_adjust: 0,
            byte_order: None,
        };
    }

    // JVC Text: "VER:xxxxQTY:yyy..." — text-format MakerNotes
    // (from Perl MakerNotes.pm: MakerNoteJVCText)
    if mn_data.starts_with(b"VER:") && make.to_uppercase().contains("JVC") || make.to_uppercase().contains("VICTOR") {
        // Not an IFD — parse as text key:value pairs
        // Return special marker; we'll decode in the dispatch
        return MakerNoteInfo {
            manufacturer: Manufacturer::Unknown,
            ifd_offset: 0,
            _base_adjust: 0,
            byte_order: None,
        };
    }

    // Apple iOS: "Apple iOS\0\0\x01" + MM/II + IFD (no standard TIFF header!)
    if mn_data.len() >= 16 && mn_data.starts_with(b"Apple iOS\0") {
        // "Apple iOS\0" (10 bytes) + "\0\x01" (2 bytes) + "MM" or "II" (2 bytes) + IFD directly
        let bo = if mn_data[12] == b'M' && mn_data[13] == b'M' {
            Some(ByteOrderMark::BigEndian)
        } else if mn_data[12] == b'I' && mn_data[13] == b'I' {
            Some(ByteOrderMark::LittleEndian)
        } else {
            None
        };
        return MakerNoteInfo {
            manufacturer: Manufacturer::Apple,
            ifd_offset: 14, // After "Apple iOS\0\0\x01MM" — IFD starts immediately
            _base_adjust: 0,
            byte_order: bo,
        };
    }

    // Pentax: "AOC\0"
    if mn_data.starts_with(b"AOC\0") {
        return MakerNoteInfo {
            manufacturer: Manufacturer::Pentax,
            ifd_offset: 6,
            _base_adjust: 0,
            byte_order: None,
        };
    }

    // PENTAX \0
    if mn_data.starts_with(b"PENTAX \0") {
        return MakerNoteInfo {
            manufacturer: Manufacturer::Pentax,
            ifd_offset: 10,
            _base_adjust: 0,
            byte_order: None,
        };
    }

    // Samsung: "SAMSUNG\0"
    if mn_data.starts_with(b"SAMSUNG\0") {
        return MakerNoteInfo {
            manufacturer: Manufacturer::Samsung,
            ifd_offset: 8,
            _base_adjust: 0,
            byte_order: None,
        };
    }

    // SIGMA\0
    if mn_data.starts_with(b"SIGMA\0") || mn_data.starts_with(b"FOVEON\0") {
        return MakerNoteInfo {
            manufacturer: Manufacturer::Sigma,
            ifd_offset: 10,
            _base_adjust: 0,
            byte_order: None,
        };
    }

    // Fallback by Make string
    let mfr = if make_upper.starts_with("CANON") {
        Manufacturer::Canon
    } else if make_upper.starts_with("NIKON") {
        Manufacturer::Nikon
    } else if make_upper.starts_with("SONY") {
        Manufacturer::Sony
    } else if make_upper.starts_with("OLYMPUS") || make_upper.starts_with("OM DIGITAL") {
        Manufacturer::Olympus
    } else if make_upper.starts_with("PENTAX") || make_upper.starts_with("RICOH") {
        Manufacturer::Pentax
    } else if make_upper.starts_with("PANASONIC") || make_upper.starts_with("LEICA") {
        Manufacturer::Panasonic
    } else if make_upper.starts_with("FUJI") {
        Manufacturer::Fujifilm
    } else if make_upper.starts_with("SAMSUNG") {
        Manufacturer::Samsung
    } else if make_upper.starts_with("CASIO") {
        Manufacturer::Casio
    } else if make_upper.starts_with("RICOH") {
        Manufacturer::Ricoh
    } else if make_upper.starts_with("MINOLTA") || make_upper.starts_with("KONICA") {
        Manufacturer::Minolta
    } else if make_upper.starts_with("APPLE") {
        Manufacturer::Apple
    } else if make_upper.starts_with("GOOGLE") {
        Manufacturer::Google
    } else if make_upper.starts_with("DJI") {
        Manufacturer::DJI
    } else {
        Manufacturer::Unknown
    };

    MakerNoteInfo {
        manufacturer: mfr,
        ifd_offset: 0, // No header, IFD starts immediately
        _base_adjust: 0,
        byte_order: None,
    }
}

/// Detect byte order from a TIFF header at the given position.
fn detect_tiff_byte_order(data: &[u8]) -> Option<ByteOrderMark> {
    if data.len() < 4 {
        return None;
    }
    if data[0] == b'I' && data[1] == b'I' && data[2] == 0x2A && data[3] == 0x00 {
        Some(ByteOrderMark::LittleEndian)
    } else if data[0] == b'M' && data[1] == b'M' && data[2] == 0x00 && data[3] == 0x2A {
        Some(ByteOrderMark::BigEndian)
    } else {
        None
    }
}

/// Read IFD entries from maker note data and convert to tags.
fn read_makernote_ifd(
    data: &[u8],
    ifd_offset: usize,
    byte_order: ByteOrderMark,
    manufacturer: Manufacturer,
    tags: &mut Vec<Tag>,
    model_name: &str,
) {
    if ifd_offset + 2 > data.len() {
        return;
    }

    let entry_count = read_u16(data, ifd_offset, byte_order) as usize;
    if entry_count == 0 || entry_count > 500 {
        return;
    }

    let entries_start = ifd_offset + 2;

    for i in 0..entry_count {
        let entry_offset = entries_start + i * 12;
        if entry_offset + 12 > data.len() {
            break;
        }

        let tag_id = read_u16(data, entry_offset, byte_order);
        let data_type = read_u16(data, entry_offset + 2, byte_order);
        let count = read_u32(data, entry_offset + 4, byte_order);
        let value_offset = read_u32(data, entry_offset + 8, byte_order);

        // Validate entry
        if data_type == 0 || data_type > 13 || count > 100000 {
            continue;
        }

        let type_size = match data_type {
            1 | 2 | 6 | 7 => 1,
            3 | 8 => 2,
            4 | 9 | 11 | 13 => 4,
            5 | 10 | 12 => 8,
            _ => continue,
        };

        let total_size = type_size * count as usize;

        let value_data = if total_size <= 4 {
            &data[entry_offset + 8..(entry_offset + 8 + total_size).min(data.len())]
        } else {
            let off = value_offset as usize;
            if off + total_size > data.len() {
                // Emit Warning for suspicious offset (like Perl Exif.pm:6582)
                if !tags.iter().any(|t| t.name == "Warning") {
                    tags.push(Tag {
                        id: TagId::Text("Warning".into()),
                        name: "Warning".into(), description: "Warning".into(),
                        group: TagGroup { family0: "ExifTool".into(), family1: "ExifTool".into(), family2: "Other".into() },
                        raw_value: Value::String(format!("[minor] Suspicious MakerNotes offset for tag 0x{:04X}", tag_id)),
                        print_value: format!("[minor] Suspicious MakerNotes offset for tag 0x{:04X}", tag_id),
                        priority: 0,
                    });
                }
                continue;
            }
            &data[off..off + total_size]
        };

        // Decode value
        let value = decode_mn_value(value_data, data_type, count as usize, byte_order);

        // Sub-table dispatch: decode binary structures into individual tags
        {
            use crate::tags::sub_tables_generated::{self as subs, DispatchContext};

            let dispatch_ctx = DispatchContext {
                model: &model_name,
                data: value_data,
                count: count as usize,
                byte_order_le: byte_order == ByteOrderMark::LittleEndian,
            };

            let sub_tags = match (manufacturer, tag_id) {
                // Canon sub-tables
                (Manufacturer::Canon, 0x0001) => {
                    let values: Vec<i16> = (0..count as usize)
                        .map(|i| read_u16(value_data, i * 2, byte_order) as i16)
                        .collect();
                    crate::tags::canon_sub::decode_camera_settings(&values)
                }
                (Manufacturer::Canon, 0x0004) => {
                    let values: Vec<i16> = (0..count as usize)
                        .map(|i| read_u16(value_data, i * 2, byte_order) as i16)
                        .collect();
                    crate::tags::canon_sub::decode_shot_info(&values)
                }
                (Manufacturer::Canon, 0x0002) => {
                    let values: Vec<u16> = (0..count as usize)
                        .map(|i| read_u16(value_data, i * 2, byte_order))
                        .collect();
                    crate::tags::canon_sub::decode_focal_length(&values)
                }
                (Manufacturer::Canon, 0x000D) => {
                    let mut t = subs::dispatch_canon_camera_info(&dispatch_ctx);
                    t.extend(decode_canon_camera_info_common(value_data, count as usize, byte_order));
                    // Decode model-specific CameraInfo fields (int8u offsets)
                    let d = value_data;
                    static CAMERA_INFO_FIELDS: &[(usize, &str)] = &[
                        (3, "FNumber"), (4, "ExposureTime"), (6, "ISO"),
                        (24, "CameraTemperature"), (29, "FocalLength"),
                        (48, "CameraOrientation"), (67, "FocusDistanceUpper"),
                        (69, "FocusDistanceLower"), (94, "WhiteBalance"),
                        (98, "ColorTemperature"), (134, "PictureStyle"),
                        (275, "MinFocalLength"), (277, "MaxFocalLength"),
                        (370, "FileIndex"), (374, "ShutterCount"), (382, "DirectoryIndex"),
                    ];
                    for &(off, name) in CAMERA_INFO_FIELDS {
                        if off < d.len() {
                            t.push(mk_canon_str(name, &d[off].to_string()));
                        }
                    }
                    // FirmwareVersion (string at offset 310, ~6 bytes)
                    if d.len() > 316 {
                        let fw = String::from_utf8_lossy(&d[310..316]).trim_end_matches('\0').to_string();
                        if !fw.is_empty() { t.push(mk_canon_str("FirmwareVersion", &fw)); }
                    }
                    // PictureStyleInfo sub-structure at offset 682 (int32s values)
                    static PS_FIELDS: &[(usize, &str)] = &[
                        (0,"ContrastStandard"),(4,"SharpnessStandard"),(8,"SaturationStandard"),(12,"ColorToneStandard"),
                        (24,"ContrastPortrait"),(28,"SharpnessPortrait"),(32,"SaturationPortrait"),(36,"ColorTonePortrait"),
                        (48,"ContrastLandscape"),(52,"SharpnessLandscape"),(56,"SaturationLandscape"),(60,"ColorToneLandscape"),
                        (72,"ContrastNeutral"),(76,"SharpnessNeutral"),(80,"SaturationNeutral"),(84,"ColorToneNeutral"),
                        (96,"ContrastFaithful"),(100,"SharpnessFaithful"),(104,"SaturationFaithful"),(108,"ColorToneFaithful"),
                        (120,"ContrastMonochrome"),(124,"SharpnessMonochrome"),
                        (136,"FilterEffectMonochrome"),(140,"ToningEffectMonochrome"),
                        (144,"ContrastUserDef1"),(148,"SharpnessUserDef1"),(152,"SaturationUserDef1"),(156,"ColorToneUserDef1"),
                        (160,"FilterEffectUserDef1"),(164,"ToningEffectUserDef1"),
                        (168,"ContrastUserDef2"),(172,"SharpnessUserDef2"),(176,"SaturationUserDef2"),(180,"ColorToneUserDef2"),
                        (184,"FilterEffectUserDef2"),(188,"ToningEffectUserDef2"),
                        (192,"ContrastUserDef3"),(196,"SharpnessUserDef3"),(200,"SaturationUserDef3"),(204,"ColorToneUserDef3"),
                        (208,"FilterEffectUserDef3"),(212,"ToningEffectUserDef3"),
                        (216,"UserDef1PictureStyle"),(218,"UserDef2PictureStyle"),(220,"UserDef3PictureStyle"),
                    ];
                    let ps_base = 682;
                    if d.len() > ps_base + 222 {
                        for &(off, name) in PS_FIELDS {
                            let abs = ps_base + off;
                            if abs + 4 <= d.len() {
                                let v = i32::from_le_bytes([d[abs], d[abs+1], d[abs+2], d[abs+3]]);
                                t.push(mk_canon_str(name, &v.to_string()));
                            }
                        }
                    }
                    t
                }
                (Manufacturer::Canon, 0x0012) => {
                    // Canon AFInfo (old): int16u array
                    decode_canon_afinfo(value_data, count as usize, byte_order)
                }
                (Manufacturer::Canon, 0x0026) => {
                    // Canon AFInfo2 (same structure as AFInfo but different tag)
                    decode_canon_afinfo2(value_data, count as usize, byte_order)
                }
                (Manufacturer::Canon, 0x009A) => {
                    // Canon AspectInfo: int32u format
                    let mut t = Vec::new();
                    if count as usize >= 1 {
                        let v = read_u32(value_data, 0, byte_order);
                        let s = match v { 0 => "3:2", 1 => "1:1", 2 => "4:3", 7 => "16:9", 8 => "4:5", _ => "" };
                        if !s.is_empty() { t.push(mk_canon_str("AspectRatio", s)); }
                        // CroppedImage dimensions at indices 1-6
                        if count as usize >= 7 {
                            let names = ["CroppedImageWidth","CroppedImageHeight","CroppedImageLeft","CroppedImageTop",
                                         "CropLeftMargin","CropTopMargin"];
                            for (i, name) in names.iter().enumerate() {
                                let v = read_u32(value_data, (i+1)*4, byte_order);
                                t.push(mk_canon_str(name, &v.to_string()));
                            }
                        }
                    }
                    t
                }
                (Manufacturer::Canon, 0x0099) => {
                    // Canon CustomFunctions2 (from Perl CanonCustom::ProcessCanonCustom2)
                    // Format: size(2) + pad(2) + count(4) + groups of records
                    // Each group: recNum(4) + recLen(4) + recCount(4) + entries
                    // Each entry: tag(4) + numValues(4) + values(4*N)
                    decode_canon_custom_functions2(value_data, byte_order)
                }
                (Manufacturer::Canon, 0x00E0) => {
                    // Canon SensorInfo: int16s, indices 1-12 (from Perl Canon::SensorInfo)
                    let mut t = Vec::new();
                    if count as usize >= 13 {
                        let rd = |i: usize| -> i16 { read_u16(value_data, i * 2, byte_order) as i16 };
                        for (i, name) in [(1,"SensorWidth"),(2,"SensorHeight"),
                            (5,"SensorLeftBorder"),(6,"SensorTopBorder"),
                            (7,"SensorRightBorder"),(8,"SensorBottomBorder"),
                            (9,"BlackMaskLeftBorder"),(10,"BlackMaskTopBorder"),
                            (11,"BlackMaskRightBorder"),(12,"BlackMaskBottomBorder")] {
                            t.push(mk_canon_str(name, &(rd(i).to_string())));
                        }
                    }
                    t
                }
                (Manufacturer::Canon, 0x00A9) => {
                    // Canon ColorBalance: int16u array with WB_RGGB levels
                    decode_canon_color_balance(value_data, count as usize, byte_order)
                }
                (Manufacturer::Canon, 0x00AA) => {
                    // Canon MeasuredColor → MeasuredRGGB
                    if count as usize >= 5 {
                        let r = read_u16(value_data, 2, byte_order);
                        let g1 = read_u16(value_data, 4, byte_order);
                        let g2 = read_u16(value_data, 6, byte_order);
                        let b = read_u16(value_data, 8, byte_order);
                        vec![mk_canon_str("MeasuredRGGB", &format!("{} {} {} {}", r, g1, g2, b))]
                    } else { Vec::new() }
                }
                (Manufacturer::Canon, 0x0026) => {
                    // Canon AFInfo2: int16s array
                    decode_canon_afinfo2(value_data, count as usize, byte_order)
                }
                (Manufacturer::Canon, 0x4001) => {
                    // Canon ColorData: int16s array (WB levels)
                    decode_canon_color_data(value_data, count as usize, byte_order)
                }
                // Nikon sub-tables
                (Manufacturer::Nikon, 0x0011) => {
                    // PreviewIFD: the value is an offset to a sub-IFD in the data
                    let preview_off = read_u32(value_data, 0, byte_order) as usize;
                    // The offset is relative to the beginning of parse_data
                    if preview_off > 0 && preview_off < data.len() {
                        decode_preview_ifd(data, preview_off, byte_order)
                    } else { Vec::new() }
                }
                (Manufacturer::Nikon, 0x0088) => decode_nikon_afinfo(value_data, byte_order),
                (Manufacturer::Nikon, 0x0097) => decode_nikon_color_balance(value_data, byte_order),
                (Manufacturer::Nikon, 0x00A8) => decode_nikon_flashinfo(value_data, byte_order),
                (Manufacturer::Nikon, 0x0091) => subs::dispatch_nikon_shot_info(&dispatch_ctx),
                (Manufacturer::Nikon, 0x0098) => subs::dispatch_nikon_lens_data(&dispatch_ctx),
                (Manufacturer::Nikon, 0x00B7) => subs::dispatch_nikon_af_info2(&dispatch_ctx),
                // PrintIM in MakerNotes (tag 0x0E00) — extract version
                (_, 0x0E00) => {
                    if value_data.len() > 11 && value_data.starts_with(b"PrintIM") {
                        let ver = String::from_utf8_lossy(&value_data[7..11]).to_string();
                        vec![Tag {
                            id: TagId::Text("PrintIMVersion".into()),
                            name: "PrintIMVersion".into(), description: "PrintIM Version".into(),
                            group: TagGroup { family0: "PrintIM".into(), family1: "PrintIM".into(), family2: "Printing".into() },
                            raw_value: Value::String(ver.clone()), print_value: ver, priority: 0,
                        }]
                    } else { Vec::new() }
                }
                // Minolta PreviewImage — extract from PreviewImageLength tag
                (Manufacturer::Minolta, 0x0089) => {
                    let len_val = if total_size <= 4 {
                        read_u32(value_data, 0, byte_order) as usize
                    } else { 0 };
                    let mut t = Vec::new();
                    // Keep PreviewImageLength tag
                    t.push(Tag {
                        id: TagId::Text("PreviewImageLength".into()),
                        name: "PreviewImageLength".into(), description: "Preview Image Length".into(),
                        group: TagGroup { family0: "MakerNotes".into(), family1: "Minolta".into(), family2: "Image".into() },
                        raw_value: Value::U32(len_val as u32), print_value: len_val.to_string(), priority: 0,
                    });
                    if len_val > 0 {
                        t.push(Tag {
                            id: TagId::Text("PreviewImage".into()),
                            name: "PreviewImage".into(), description: "Preview Image".into(),
                            group: TagGroup { family0: "MakerNotes".into(), family1: "Minolta".into(), family2: "Image".into() },
                            raw_value: Value::Binary(Vec::new()),
                            print_value: format!("(Binary data {} bytes)", len_val),
                            priority: 0,
                        });
                    }
                    t
                }
                // Minolta CameraSettings binary sub-table (int32u format)
                (Manufacturer::Minolta, 0x0001) | (Manufacturer::Minolta, 0x0003) => {
                    decode_minolta_camera_settings(value_data, byte_order)
                }
                // Ricoh ImageInfo binary sub-table (tag 0x1001)
                (Manufacturer::Ricoh, 0x1001) => {
                    let mut t = Vec::new();
                    let d = value_data;
                    if d.len() >= 42 {
                        let w = u16::from_le_bytes([d[0], d[1]]);
                        let h = u16::from_le_bytes([d[2], d[3]]);
                        t.push(mk_nikon_str("RicohImageWidth", &w.to_string()));
                        t.push(mk_nikon_str("RicohImageHeight", &h.to_string()));
                        // RicohDate at offset 6 (7 bytes encoded)
                        if d.len() >= 13 {
                            let date = format!("{:02x}{:02x}:{:02x}:{:02x} {:02x}:{:02x}:{:02x}",
                                d[6], d[7], d[8], d[9], d[10], d[11], d[12]);
                            t.push(mk_nikon_str("RicohDate", &date));
                        }
                        // ManufactureDate1 at offset ~42+ (varies by model)
                    }
                    t
                }
                // Pentax binary sub-tables (from Perl Pentax.pm)
                (Manufacturer::Pentax, 0x0205) => decode_binary_subtable(value_data, "Pentax", PENTAX_CAMERA_SETTINGS),
                (Manufacturer::Pentax, 0x0206) => decode_binary_subtable(value_data, "Pentax", PENTAX_AE_INFO),
                (Manufacturer::Pentax, 0x0207) => decode_binary_subtable(value_data, "Pentax", PENTAX_LENS_INFO),
                (Manufacturer::Pentax, 0x0208) => decode_binary_subtable(value_data, "Pentax", PENTAX_FLASH_INFO),
                (Manufacturer::Pentax, 0x0215) => decode_binary_subtable(value_data, "Pentax", PENTAX_CAMERA_INFO),
                (Manufacturer::Pentax, 0x0216) => decode_binary_subtable(value_data, "Pentax", PENTAX_BATTERY_INFO),
                (Manufacturer::Pentax, 0x021F) => decode_binary_subtable(value_data, "Pentax", PENTAX_AF_INFO),
                (Manufacturer::Pentax, 0x005C) => decode_binary_subtable(value_data, "Pentax", PENTAX_SR_INFO),
                // Apple RunTime plist
                (Manufacturer::Apple, 0x0003) => decode_apple_runtime(value_data),
                // Sony sub-tables
                (Manufacturer::Sony, 0x0114) => subs::dispatch_sony_camera_settings(&dispatch_ctx),
                (Manufacturer::Sony, 0x2010) => subs::dispatch_sony_tag2010(&dispatch_ctx),
                (Manufacturer::Sony, 0x9400) => subs::dispatch_sony_tag9400(&dispatch_ctx),
                _ => Vec::new(),
            };

            if !sub_tags.is_empty() {
                tags.extend(sub_tags);
                continue;
            }
        }

        // Olympus sub-IFDs (0x2010-0x2050): Equipment, CameraSettings, FocusInfo etc.
        // Two formats (from Perl Olympus.pm):
        //   1. format=ifd/int32u → offset to sub-IFD
        //   2. format=undefined → data IS the sub-IFD inline
        if (manufacturer == Manufacturer::Olympus || manufacturer == Manufacturer::OlympusNew)
            && tag_id >= 0x2010 && tag_id <= 0x2050
        {
            if data_type == 4 && count == 1 {
                // Case 2: offset to sub-IFD (OlympusNew)
                let sub_off = read_u32(value_data, 0, byte_order) as usize;
                if sub_off > 0 && sub_off + 2 < data.len() {
                    let mut sub_tags = Vec::new();
                    read_makernote_ifd(data, sub_off, byte_order, manufacturer, &mut sub_tags, model_name);
                    if !sub_tags.is_empty() {
                        tags.extend(sub_tags);
                        continue;
                    }
                }
            } else if data_type == 7 && total_size > 12 {
                // Case 1: data IS the sub-IFD inline (old Olympus)
                let mut sub_tags = Vec::new();
                read_makernote_ifd(value_data, 0, byte_order, manufacturer, &mut sub_tags, model_name);
                if !sub_tags.is_empty() {
                    tags.extend(sub_tags);
                    continue;
                }
            }
        }

        // Look up tag name
        let group_name = manufacturer_group_name(manufacturer);
        let (name, description) = mn_tags::lookup(manufacturer, tag_id);

        // Apply manufacturer-specific print conversions
        let print_value = apply_mn_print_conv(manufacturer, tag_id, &value)
            .or_else(|| {
                // Fallback to generated print conversions
                let module = manufacturer_group_name(manufacturer);
                value.as_u64()
                    .and_then(|v| crate::tags::print_conv_generated::print_conv(module, tag_id, v as i64))
                    .map(|s| s.to_string())
                    .or_else(|| {
                        // Try by tag name
                        value.as_u64()
                            .and_then(|v| crate::tags::print_conv_generated::print_conv_by_name(name, v as i64))
                            .map(|s| s.to_string())
                    })
            })
            .unwrap_or_else(|| value.to_display_string());

        tags.push(Tag {
            id: TagId::Numeric(tag_id),
            name: name.to_string(),
            description: description.to_string(),
            group: TagGroup {
                family0: "MakerNotes".to_string(),
                family1: group_name.to_string(),
                family2: "Camera".to_string(),
            },
            raw_value: value,
            print_value,
            priority: 0,
        });
    }
}

fn decode_mn_value(data: &[u8], data_type: u16, count: usize, bo: ByteOrderMark) -> Value {
    match data_type {
        1 | 7 => {
            // BYTE / UNDEFINED
            if count == 1 { Value::U8(data[0]) }
            else { Value::Undefined(data.to_vec()) }
        }
        2 => {
            // ASCII
            Value::String(
                String::from_utf8_lossy(data)
                    .trim_end_matches('\0')
                    .to_string(),
            )
        }
        3 => {
            // SHORT
            if count == 1 {
                Value::U16(read_u16(data, 0, bo))
            } else {
                Value::List((0..count).map(|i| Value::U16(read_u16(data, i * 2, bo))).collect())
            }
        }
        4 | 13 => {
            // LONG / IFD
            if count == 1 {
                Value::U32(read_u32(data, 0, bo))
            } else {
                Value::List((0..count).map(|i| Value::U32(read_u32(data, i * 4, bo))).collect())
            }
        }
        5 => {
            // RATIONAL
            if count == 1 && data.len() >= 8 {
                Value::URational(read_u32(data, 0, bo), read_u32(data, 4, bo))
            } else {
                Value::Undefined(data.to_vec())
            }
        }
        8 => {
            // SSHORT
            if count == 1 {
                Value::I16(read_u16(data, 0, bo) as i16)
            } else {
                Value::List((0..count).map(|i| Value::I16(read_u16(data, i * 2, bo) as i16)).collect())
            }
        }
        9 => {
            // SLONG
            if count == 1 {
                Value::I32(read_u32(data, 0, bo) as i32)
            } else {
                Value::List((0..count).map(|i| Value::I32(read_u32(data, i * 4, bo) as i32)).collect())
            }
        }
        10 => {
            // SRATIONAL
            if count == 1 && data.len() >= 8 {
                Value::IRational(read_u32(data, 0, bo) as i32, read_u32(data, 4, bo) as i32)
            } else {
                Value::Undefined(data.to_vec())
            }
        }
        _ => Value::Undefined(data.to_vec()),
    }
}

fn manufacturer_group_name(mfr: Manufacturer) -> &'static str {
    match mfr {
        Manufacturer::Canon => "Canon",
        Manufacturer::Nikon | Manufacturer::NikonOld => "Nikon",
        Manufacturer::Sony => "Sony",
        Manufacturer::Pentax => "Pentax",
        Manufacturer::Olympus | Manufacturer::OlympusNew => "Olympus",
        Manufacturer::Panasonic => "Panasonic",
        Manufacturer::Fujifilm => "Fujifilm",
        Manufacturer::Samsung => "Samsung",
        Manufacturer::Sigma => "Sigma",
        Manufacturer::Casio | Manufacturer::CasioType2 => "Casio",
        Manufacturer::Ricoh => "Ricoh",
        Manufacturer::Minolta => "Minolta",
        Manufacturer::Apple => "Apple",
        Manufacturer::Google => "Google",
        Manufacturer::DJI => "DJI",
        Manufacturer::Unknown => "MakerNotes",
    }
}

/// Apply manufacturer-specific print conversions.
/// Decode Canon CameraInfo common fields (indices 3-5: BracketMode/Value/ShotNumber).
/// These are present in all CameraInfo variants at the same indices.
fn decode_canon_camera_info_common(data: &[u8], count: usize, bo: ByteOrderMark) -> Vec<Tag> {
    let mut tags = Vec::new();
    if count < 6 { return tags; }

    // The format varies (int8u for most, int16s for some), but indices 3-5 are common
    // For int8u format: each element is 1 byte
    // For int16s format: each element is 2 bytes
    // Detect based on data size vs count
    let elem_size = if data.len() >= count * 2 { 2 } else { 1 };

    let read_val = |idx: usize| -> i32 {
        if elem_size == 2 {
            read_u16(data, idx * 2, bo) as i16 as i32
        } else {
            if idx < data.len() { data[idx] as i8 as i32 } else { 0 }
        }
    };

    let bracket_mode = read_val(3);
    let bracket_value = read_val(4);
    let bracket_shot = read_val(5);

    let bm_str = match bracket_mode {
        0 => "Off",
        1 => "AEB",
        2 => "FEB",
        3 => "ISO",
        4 => "WB",
        _ => "",
    };
    let bm_print = if bm_str.is_empty() { bracket_mode.to_string() } else { bm_str.to_string() };
    tags.push(Tag {
        id: TagId::Text("BracketMode".into()), name: "BracketMode".into(),
        description: "Bracket Mode".into(),
        group: TagGroup { family0: "MakerNotes".into(), family1: "Canon".into(), family2: "Camera".into() },
        raw_value: Value::I32(bracket_mode), print_value: bm_print, priority: 0,
    });
    tags.push(mk_canon("BracketValue", Value::I32(bracket_value)));
    tags.push(mk_canon("BracketShotNumber", Value::I32(bracket_shot)));

    tags
}

/// Decode Canon ColorBalance (tag 0x00A9).
/// Structure: [count][R G1 B G2] × N white balance sets + [R G1 B G2] black levels
fn decode_canon_color_balance(data: &[u8], count: usize, bo: ByteOrderMark) -> Vec<Tag> {
    let mut tags = Vec::new();
    let rd = |i: usize| -> u16 { read_u16(data, i * 2, bo) };

    if count < 5 { return tags; }

    // First value is the number of entries or a version marker
    // Common layout: [header] [Auto: R G1 B G2] [Daylight: R G1 B G2] ...
    let wb_names = [
        "Auto", "Daylight", "Shade", "Cloudy", "Tungsten",
        "Fluorescent", "Flash", "Custom", "Kelvin",
    ];

    let base = 1; // Skip first value (count/version)
    let mut offset = base;

    for name in &wb_names {
        if offset + 4 > count { break; }
        let r = rd(offset);
        let g1 = rd(offset + 1);
        let b = rd(offset + 2);
        let g2 = rd(offset + 3);

        if r > 0 || g1 > 0 { // Skip empty entries
            tags.push(mk_canon_str(
                &format!("WB_RGGBLevels{}", name),
                &format!("{} {} {} {}", r, g1, b, g2),
            ));

            // First entry (Auto) is also WB_RGGBLevels
            if *name == "Auto" {
                tags.push(mk_canon_str("WB_RGGBLevels", &format!("{} {} {} {}", r, g1, b, g2)));
            }
        }

        offset += 4;
    }

    // Black levels at end of data
    if count >= offset + 4 {
        // Last 4 values are typically black levels
        let bl_base = count - 4;
        let r = rd(bl_base);
        let g1 = rd(bl_base + 1);
        let b = rd(bl_base + 2);
        let g2 = rd(bl_base + 3);
        tags.push(mk_canon_str("WB_RGGBBlackLevels", &format!("{} {} {} {}", r, g1, b, g2)));
    }

    // MeasuredRGGB from MeasuredColor tag (0x00AA) - handled separately
    // But we can compute RedBalance and BlueBalance here
    if let Some(auto_tag) = tags.iter().find(|t| t.name == "WB_RGGBLevels") {
        if let Value::String(ref s) = auto_tag.raw_value {
            let parts: Vec<f64> = s.split_whitespace().filter_map(|p| p.parse().ok()).collect();
            if parts.len() >= 4 && parts[1] > 0.0 {
                let red_bal = parts[0] / parts[1];
                let blue_bal = parts[2] / parts[1];
                tags.push(mk_canon_str("RedBalance", &format!("{:.6}", red_bal)));
                tags.push(mk_canon_str("BlueBalance", &format!("{:.6}", blue_bal)));
            }
        }
    }

    tags
}

/// Decode Canon AFInfo (tag 0x0012, old format).
fn decode_canon_afinfo(data: &[u8], count: usize, bo: ByteOrderMark) -> Vec<Tag> {
    let mut tags = Vec::new();
    let rd = |i: usize| -> u16 { read_u16(data, i * 2, bo) };

    if count < 5 { return tags; }

    let num_af = rd(0) as usize;
    let valid_af = rd(1);
    let img_w = rd(2);
    let img_h = rd(3);
    let af_w = rd(4);

    tags.push(mk_canon("NumAFPoints", Value::U16(num_af as u16)));
    tags.push(mk_canon("ValidAFPoints", Value::U16(valid_af)));
    tags.push(mk_canon("CanonImageWidth", Value::U16(img_w)));
    tags.push(mk_canon("CanonImageHeight", Value::U16(img_h)));
    tags.push(mk_canon("AFImageWidth", Value::U16(af_w)));

    // AFImageHeight at index 5 if available
    if count > 5 {
        tags.push(mk_canon("AFImageHeight", Value::U16(rd(5))));
    }

    // AF area layout: [6]=AFAreaWidth [7]=AFAreaHeight [8..8+N]=XPos [8+N..8+2N]=YPos
    if num_af > 0 && 8 + num_af * 2 <= count {
        tags.push(mk_canon("AFAreaWidth", Value::U16(rd(6))));
        tags.push(mk_canon("AFAreaHeight", Value::U16(rd(7))));

        let x_pos: Vec<String> = (0..num_af).map(|i| (rd(8 + i) as i16).to_string()).collect();
        tags.push(mk_canon_str("AFAreaXPositions", &x_pos.join(" ")));

        let y_pos: Vec<String> = (0..num_af).map(|i| (rd(8 + num_af + i) as i16).to_string()).collect();
        tags.push(mk_canon_str("AFAreaYPositions", &y_pos.join(" ")));
    }

    tags
}

/// Decode Canon AFInfo2 (tag 0x0026).
fn decode_canon_afinfo2(data: &[u8], count: usize, bo: ByteOrderMark) -> Vec<Tag> {
    let mut tags = Vec::new();
    let rd = |i: usize| -> i16 { read_u16(data, i * 2, bo) as i16 };

    if count < 8 { return tags; }

    let num_af = rd(2) as usize;
    let valid_af = rd(3) as usize;
    let img_w = rd(4) as u16;
    let img_h = rd(5) as u16;
    let af_w = rd(6) as u16;
    let af_h = rd(7) as u16;

    tags.push(mk_canon("NumAFPoints", Value::U16(num_af as u16)));
    tags.push(mk_canon("ValidAFPoints", Value::U16(valid_af as u16)));
    tags.push(mk_canon("CanonImageWidth", Value::U16(img_w)));
    tags.push(mk_canon("CanonImageHeight", Value::U16(img_h)));
    tags.push(mk_canon("AFImageWidth", Value::U16(af_w)));
    tags.push(mk_canon("AFImageHeight", Value::U16(af_h)));

    // AF Area dimensions and positions (variable count based on NumAFPoints)
    let base = 8;
    if num_af > 0 && base + num_af * 4 <= count {
        // AFAreaWidths at base, AFAreaHeights at base+num_af, etc.
        let widths: Vec<String> = (0..num_af).map(|i| rd(base + i).to_string()).collect();
        let heights: Vec<String> = (0..num_af).map(|i| rd(base + num_af + i).to_string()).collect();
        let x_pos: Vec<String> = (0..num_af).map(|i| rd(base + num_af * 2 + i).to_string()).collect();
        let y_pos: Vec<String> = (0..num_af).map(|i| rd(base + num_af * 3 + i).to_string()).collect();

        if !widths.is_empty() {
            tags.push(mk_canon_str("AFAreaWidth", &widths.join(" ")));
            tags.push(mk_canon_str("AFAreaHeight", &heights.join(" ")));
            tags.push(mk_canon_str("AFAreaXPositions", &x_pos.join(" ")));
            tags.push(mk_canon_str("AFAreaYPositions", &y_pos.join(" ")));
        }
    }

    tags
}

/// Decode Canon ColorData (tag 0x4001).
fn decode_canon_color_data(data: &[u8], count: usize, bo: ByteOrderMark) -> Vec<Tag> {
    let mut tags = Vec::new();
    let rd = |i: usize| -> i16 { read_u16(data, i * 2, bo) as i16 };

    if count < 50 { return tags; }

    let version = rd(0);
    tags.push(mk_canon_str("ColorDataVersion", &version.to_string()));

    // Determine WB offset based on count (from Perl Canon.pm ColorData tables)
    let wb_base = if count > 580 { 63 }   // ColorData3 (1DmkIII etc.)
        else if count > 350 { 50 }         // ColorData2
        else if count > 200 { 25 }         // ColorData1
        else { 19 };                       // Older cameras

    // WB_RGGBLevelsAsShot (before Auto in ColorData3)
    if wb_base >= 5 && wb_base + 4 <= count {
        let r = rd(wb_base) as u16;
        let g1 = rd(wb_base + 1) as u16;
        let b = rd(wb_base + 2) as u16;
        let g2 = rd(wb_base + 3) as u16;
        tags.push(mk_canon_str("WB_RGGBLevelsAsShot", &format!("{} {} {} {}", r, g1, b, g2)));
        let temp = rd(wb_base + 4) as u16;
        if temp > 0 { tags.push(mk_canon_str("ColorTempAsShot", &temp.to_string())); }
    }
    // WB_RGGBLevelsAuto (4 values: R, G1, B, G2)
    if wb_base + 4 <= count {
        let r = rd(wb_base) as u16;
        let g1 = rd(wb_base + 1) as u16;
        let b = rd(wb_base + 2) as u16;
        let g2 = rd(wb_base + 3) as u16;
        tags.push(mk_canon_str("WB_RGGBLevelsAuto", &format!("{} {} {} {}", r, g1, b, g2)));
        tags.push(mk_canon_str("WB_RGGBLevels", &format!("{} {} {} {}", r, g1, b, g2)));
    }

    // Subsequent WB blocks (each 4 values + 1 color temp)
    let wb_names = ["Daylight", "Cloudy", "Tungsten", "Fluorescent", "Flash", "Custom", "Kelvin", "Shade"];
    let mut offset = wb_base + 5; // After Auto + ColorTemp

    for name in &wb_names {
        if offset + 4 > count { break; }
        let r = rd(offset) as u16;
        let g1 = rd(offset + 1) as u16;
        let b = rd(offset + 2) as u16;
        let g2 = rd(offset + 3) as u16;
        tags.push(mk_canon_str(
            &format!("WB_RGGBLevels{}", name),
            &format!("{} {} {} {}", r, g1, b, g2),
        ));
        // ColorTemp for this WB mode
        if offset + 4 < count {
            let temp = rd(offset + 4) as u16;
            if temp > 0 {
                tags.push(mk_canon_str(&format!("ColorTemp{}", name), &temp.to_string()));
            }
        }
        offset += 5; // 4 RGGB + 1 ColorTemp
    }

    // WB_RGGBBlackLevels (usually near the end of the data)
    // Common offset for many cameras
    if count > 100 {
        let bl_base = count - 8; // Approximate
        if bl_base + 4 <= count {
            let r = rd(bl_base) as u16;
            let g1 = rd(bl_base + 1) as u16;
            let b = rd(bl_base + 2) as u16;
            let g2 = rd(bl_base + 3) as u16;
            if r > 0 || g1 > 0 {
                tags.push(mk_canon_str("WB_RGGBBlackLevels", &format!("{} {} {} {}", r, g1, b, g2)));
            }
        }
    }

    // MeasuredRGGB
    let meas_base = wb_base - 4;
    if meas_base + 4 <= count && meas_base > 0 {
        let r = rd(meas_base) as u16;
        let g1 = rd(meas_base + 1) as u16;
        let b = rd(meas_base + 2) as u16;
        let g2 = rd(meas_base + 3) as u16;
        if r > 0 && g1 > 0 {
            tags.push(mk_canon_str("MeasuredRGGB", &format!("{} {} {} {}", r, g1, b, g2)));
        }
    }

    tags
}

fn mk_canon(name: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup { family0: "MakerNotes".into(), family1: "Canon".into(), family2: "Camera".into() },
        raw_value: value, print_value: pv, priority: 0,
    }
}

fn mk_canon_str(name: &str, value: &str) -> Tag {
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup { family0: "MakerNotes".into(), family1: "Canon".into(), family2: "Camera".into() },
        raw_value: Value::String(value.to_string()),
        print_value: value.to_string(),
        priority: 0,
    }
}

fn apply_mn_print_conv(manufacturer: Manufacturer, tag_id: u16, value: &Value) -> Option<String> {
    use crate::tags::{nikon_conv, sony_conv};

    match manufacturer {
        Manufacturer::Nikon | Manufacturer::NikonOld => {
            let v = value.as_u64();
            match tag_id {
                0x0087 => v.and_then(|v| nikon_conv::flash_mode(v)).map(|s| s.to_string()),
                0x0089 => v.map(|v| nikon_conv::shooting_mode(v as u16)),
                0x001E => v.and_then(|v| nikon_conv::color_space(v)).map(|s| s.to_string()),
                0x0022 => v.and_then(|v| nikon_conv::active_d_lighting(v)).map(|s| s.to_string()),
                0x002A => v.and_then(|v| nikon_conv::vignette_control(v)).map(|s| s.to_string()),
                0x00B1 => v.and_then(|v| nikon_conv::high_iso_nr(v)).map(|s| s.to_string()),
                0x0093 => v.and_then(|v| nikon_conv::nef_compression(v)).map(|s| s.to_string()),
                _ => None,
            }
        }
        Manufacturer::Sony => {
            let v = value.as_u64();
            match tag_id {
                0xB020 => value.as_str().map(|s| sony_conv::creative_style(s).to_string()),
                0xB023 => v.and_then(|v| sony_conv::scene_mode(v)).map(|s| s.to_string()),
                0xB025 => v.and_then(|v| sony_conv::dro(v)).map(|s| s.to_string()),
                0xB029 => v.and_then(|v| sony_conv::color_mode(v)).map(|s| s.to_string()),
                0xB041 => v.and_then(|v| sony_conv::exposure_mode(v)).map(|s| s.to_string()),
                0x201B => v.and_then(|v| sony_conv::focus_mode(v)).map(|s| s.to_string()),
                0x201C => v.and_then(|v| sony_conv::af_area_mode(v)).map(|s| s.to_string()),
                _ => None,
            }
        }
        _ => None,
    }
}

fn read_u16(data: &[u8], offset: usize, bo: ByteOrderMark) -> u16 {
    if offset + 2 > data.len() { return 0; }
    match bo {
        ByteOrderMark::LittleEndian => u16::from_le_bytes([data[offset], data[offset + 1]]),
        ByteOrderMark::BigEndian => u16::from_be_bytes([data[offset], data[offset + 1]]),
    }
}

fn read_u32(data: &[u8], offset: usize, bo: ByteOrderMark) -> u32 {
    if offset + 4 > data.len() { return 0; }
    match bo {
        ByteOrderMark::LittleEndian => u32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]),
        ByteOrderMark::BigEndian => u32::from_be_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]),
    }
}
