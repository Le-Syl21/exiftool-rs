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
    GE,
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

    // GoPro MakerNotes: binary format, not IFD (Perl: "Unrecognized MakerNotes")
    if make.to_uppercase().starts_with("GOPRO") {
        return vec![Tag {
            id: TagId::Text("Warning".into()),
            name: "Warning".into(), description: "Warning".into(),
            group: TagGroup { family0: "ExifTool".into(), family1: "ExifTool".into(), family2: "Other".into() },
            raw_value: Value::String("[minor] Unrecognized MakerNotes".into()),
            print_value: "[minor] Unrecognized MakerNotes".into(),
            priority: 0,
        }];
    }

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

    // Canon post-processing: OriginalDecisionData
    // The OriginalDecisionDataOffset tag gives a JPEG-file-relative offset to 512 bytes of binary data.
    // In TIFF-relative terms, subtract 12 (SOI + APP1-marker + size + "Exif\0\0" = 2+2+2+6=12 bytes).
    // Perl: Composite OriginalDecisionData requires OriginalDecisionDataOffset.
    if info.manufacturer == Manufacturer::Canon {
        if let Some(odd_tag) = tags.iter().find(|t| t.name == "OriginalDecisionDataOffset") {
            if let Some(jpeg_off) = odd_tag.raw_value.as_u64() {
                let jpeg_off = jpeg_off as usize;
                // TIFF data (data) starts at JPEG byte offset 12 (typical JPEG-APP1-EXIF layout)
                // Adjust: tiff_off = jpeg_off - 12
                let tiff_off = jpeg_off.saturating_sub(12);
                let odd_size = 512usize;
                if tiff_off > 0 && tiff_off + odd_size <= data.len() {
                    let bin_data = &data[tiff_off..tiff_off + odd_size];
                    // Perl outputs: "(Binary data N bytes, use -b option to extract)"
                    let pv = format!("(Binary data {} bytes, use -b option to extract)", odd_size);
                    tags.push(Tag {
                        id: TagId::Text("OriginalDecisionData".into()),
                        name: "OriginalDecisionData".into(),
                        description: "Original Decision Data".into(),
                        group: TagGroup { family0: "Composite".into(), family1: "Composite".into(), family2: "Other".into() },
                        raw_value: Value::Binary(bin_data.to_vec()),
                        print_value: pv,
                        priority: 0,
                    });
                }
            }
        }
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
        0x0701 => "ShutterButtonAFOnButton",
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
        // (52, "DataImprint"), // Condition: DiMAGE 7Hi only
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

    // TimeCreated at offset 0x14
    if d.len() > 0x16 {
        let h = d[0x14];
        let m = d[0x15];
        let s = d[0x16];
        tags.push(mk("TimeCreated", format!("{:02}:{:02}:{:02}", h, m, s)));
    }

    // WhiteBalance at offset 0x40
    if d.len() > 0x40 {
        tags.push(mk("WhiteBalance", d[0x40].to_string()));
    }
    // ISO at offset 0x60
    if d.len() > 0x61 {
        tags.push(mk("ISO", u16::from_be_bytes([d[0x60], d[0x61]]).to_string()));
    }
    // Sharpness at offset 0x6b
    if d.len() > 0x6b {
        tags.push(mk("Sharpness", d[0x6b].to_string()));
    }
    if d.len() > 98 {
        tags.push(mk("TotalZoom", u16::from_be_bytes([d[96], d[97]]).to_string()));
        tags.push(mk("DateTimeStamp", d[98].to_string()));
    }
    if d.len() > 102 {
        tags.push(mk("ColorMode", u32::from_be_bytes([d[100], d[101], d[102], d[103]]).to_string()));
        tags.push(mk("DigitalZoom", u32::from_be_bytes([d[104], d[105], d[106], d[107]]).to_string()));
    }
    // 0x6b: Sharpness (int8s) — Perl Kodak::Main
    if d.len() > 0x6b {
        tags.push(mk("Sharpness", (d[0x6b] as i8).to_string()));
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

/// PentaxEv: matches Perl's PentaxEv() from Pentax.pm (line 6815).
/// Adjusts values where val%8==3 or val%8==5 for exact 1/3-stop fractions, then divides by 8.
fn pentax_ev(val: i32) -> f64 {
    let mut v = val as f64;
    if val & 0x01 != 0 {
        let sign = if val < 0 { -1.0_f64 } else { 1.0_f64 };
        let frac = (val.abs() & 0x07) as i32;
        if frac == 0x03 {
            v += sign * (8.0 / 3.0 - frac as f64);
        } else if frac == 0x05 {
            v += sign * (16.0 / 3.0 - frac as f64);
        }
    }
    v / 8.0
}

/// Helper: create a Pentax MakerNotes tag with a string value.
fn mk_pentax(name: &str, print: &str) -> Tag {
    Tag {
        id: TagId::Text(name.to_string()), name: name.to_string(), description: name.to_string(),
        group: TagGroup { family0: "MakerNotes".into(), family1: "Pentax".into(), family2: "Camera".into() },
        raw_value: Value::String(print.to_string()), print_value: print.to_string(), priority: 0,
    }
}

/// Format a shutter speed value (like ExifTool PrintExposureTime).
fn print_exposure_time(val: f64) -> String {
    if val <= 0.0 { return "0".to_string(); }
    if val >= 1.0 { return format!("{}", val as u64); }
    let inv = (1.0 / val).round() as u64;
    format!("1/{}", inv)
}

/// Decode Pentax CameraSettings (tag 0x0205, 23 bytes).
/// From Perl Pentax::CameraSettings table.
fn decode_pentax_camera_settings(data: &[u8]) -> Vec<Tag> {
    let mut tags = Vec::new();
    if data.is_empty() { return tags; }
    let pb = |name: &str, v: &str| mk_pentax(name, v);

    // Byte 0: PictureMode2
    if data.len() > 0 {
        let b = data[0];
        let s = match b {
            0 => "Scene Mode", 1 => "Auto PICT", 2 => "Program AE", 3 => "Green Mode",
            4 => "Shutter Speed Priority", 5 => "Aperture Priority", 6 => "Program Tv Shift",
            7 => "Program Av Shift", 8 => "Manual", 9 => "Bulb",
            10 => "Aperture Priority, Off-Auto-Aperture", 11 => "Manual, Off-Auto-Aperture",
            12 => "Bulb, Off-Auto-Aperture", 13 => "Shutter & Aperture Priority AE",
            15 => "Sensitivity Priority AE", 16 => "Flash X-Sync Speed AE",
            _ => "",
        };
        let pm2_tmp = if s.is_empty() { b.to_string() } else { s.to_string() };
        tags.push(pb("PictureMode2", &pm2_tmp));
    }

    // Byte 1: bitmask fields — ProgramLine(0x03), EVSteps(0x20), E-DialInProgram(0x40), ApertureRingUse(0x80)
    if data.len() > 1 {
        let b = data[1];

        let pl = b & 0x03;
        let pl_s = match pl { 0 => "Normal", 1 => "Hi Speed", 2 => "Depth", 3 => "MTF", _ => "" };
        tags.push(pb("ProgramLine", pl_s));

        let ev = (b & 0x20) >> 5;
        tags.push(pb("EVSteps", if ev == 0 { "1/2 EV Steps" } else { "1/3 EV Steps" }));

        let ed = (b & 0x40) >> 6;
        tags.push(pb("E-DialInProgram", if ed == 0 { "Tv or Av" } else { "P Shift" }));

        let ar = (b & 0x80) >> 7;
        tags.push(pb("ApertureRingUse", if ar == 0 { "Prohibited" } else { "Permitted" }));
    }

    // Byte 2: FlashOptions(0xf0), MeteringMode2(0x0f)
    if data.len() > 2 {
        let b = data[2];
        let fo = (b & 0xf0) >> 4;
        let fo_s = match fo {
            0 => "Normal", 1 => "Red-eye reduction", 2 => "Auto",
            3 => "Auto, Red-eye reduction", 5 => "Wireless (Master)",
            6 => "Wireless (Control)", 8 => "Slow-sync",
            9 => "Slow-sync, Red-eye reduction", 10 => "Trailing-curtain Sync", _ => "",
        };
        let fo_tmp = if fo_s.is_empty() { fo.to_string() } else { fo_s.to_string() };
        tags.push(pb("FlashOptions", &fo_tmp));

        let mm = b & 0x0f;
        let mm_s = match mm {
            0 => "Multi-segment",
            v if v & 0x01 != 0 && v & 0x02 != 0 => "Center-weighted average, Spot",
            v if v & 0x01 != 0 => "Center-weighted average",
            v if v & 0x02 != 0 => "Spot",
            _ => "",
        };
        let mm2_tmp = if mm_s.is_empty() { mm.to_string() } else { mm_s.to_string() };
        tags.push(pb("MeteringMode2", &mm2_tmp));
    }

    // Byte 3: AFPointMode(0xf0), FocusMode2(0x0f)
    if data.len() > 3 {
        let b = data[3];
        // AFPointMode (mask 0xf0)
        let apm = (b & 0xf0) >> 4;
        let apm_tmp = if apm == 0 { "Auto".to_string() } else {
            let mut parts = Vec::new();
            if apm & 0x01 != 0 { parts.push("Select"); }
            if apm & 0x02 != 0 { parts.push("Fixed Center"); }
            if parts.is_empty() { apm.to_string() } else { parts.join(", ") }
        };
        tags.push(pb("AFPointMode", &apm_tmp));

        let fm = b & 0x0f;
        let fm_s = match fm { 0 => "Manual", 1 => "AF-S", 2 => "AF-C", 3 => "AF-A", _ => "" };
        let fm2_tmp = if fm_s.is_empty() { fm.to_string() } else { fm_s.to_string() };
        tags.push(pb("FocusMode2", &fm2_tmp));
    }

    // Bytes 4-5: AFPointSelected2 (int16u, little-endian)
    if data.len() > 5 {
        let v = u16::from_le_bytes([data[4], data[5]]);
        let aps2_tmp = if v == 0 {
            "Auto".to_string()
        } else {
            let mut bits = Vec::new();
            if v & (1 << 0) != 0 { bits.push("Upper-left"); }
            if v & (1 << 1) != 0 { bits.push("Top"); }
            if v & (1 << 2) != 0 { bits.push("Upper-right"); }
            if v & (1 << 3) != 0 { bits.push("Left"); }
            if v & (1 << 4) != 0 { bits.push("Mid-left"); }
            if v & (1 << 5) != 0 { bits.push("Center"); }
            if v & (1 << 6) != 0 { bits.push("Mid-right"); }
            if v & (1 << 7) != 0 { bits.push("Right"); }
            if v & (1 << 8) != 0 { bits.push("Lower-left"); }
            if v & (1 << 9) != 0 { bits.push("Bottom"); }
            if v & (1 << 10) != 0 { bits.push("Lower-right"); }
            if bits.is_empty() { v.to_string() } else { bits.join(", ") }
        };
        tags.push(pb("AFPointSelected2", &aps2_tmp));
    }

    // Byte 6: ISOFloor — ValueConv: int(100*exp(PentaxEv(val-32)*log(2))+0.5)
    if data.len() > 6 {
        let raw = data[6] as i32;
        let ev = pentax_ev(raw - 32);
        let iso = (100.0 * (ev * std::f64::consts::LN_2).exp() + 0.5) as i64;
        tags.push(pb("ISOFloor", &iso.to_string()));
    }

    // Byte 7: DriveMode2
    if data.len() > 7 {
        let b = data[7];
        let dm2_tmp = if b == 0 {
            "Single-frame".to_string()
        } else {
            let mut bits = Vec::new();
            if b & (1 << 0) != 0 { bits.push("Continuous"); }
            if b & (1 << 1) != 0 { bits.push("Continuous (Lo)"); }
            if b & (1 << 2) != 0 { bits.push("Self-timer (12 s)"); }
            if b & (1 << 3) != 0 { bits.push("Self-timer (2 s)"); }
            if b & (1 << 4) != 0 { bits.push("Remote Control (3 s delay)"); }
            if b & (1 << 5) != 0 { bits.push("Remote Control"); }
            if b & (1 << 6) != 0 { bits.push("Exposure Bracket"); }
            if b & (1 << 7) != 0 { bits.push("Multiple Exposure"); }
            if bits.is_empty() { b.to_string() } else { bits.join(", ") }
        };
        tags.push(pb("DriveMode2", &dm2_tmp));
    }

    // Byte 8: ExposureBracketStepSize
    if data.len() > 8 {
        let b = data[8];
        let ebs_s = match b {
            3 => "0.3", 4 => "0.5", 5 => "0.7", 8 => "1.0",
            11 => "1.3", 12 => "1.5", 13 => "1.7", 16 => "2.0", _ => "",
        };
        if !ebs_s.is_empty() {
            tags.push(pb("ExposureBracketStepSize", ebs_s));
        }
    }

    // Byte 9: BracketShotNumber
    if data.len() > 9 {
        let b = data[9];
        let bsn_s = match b {
            0x00 => "n/a", 0x02 => "1 of 2", 0x12 => "2 of 2",
            0x03 => "1 of 3", 0x13 => "2 of 3", 0x23 => "3 of 3",
            0x05 => "1 of 5", 0x15 => "2 of 5", 0x25 => "3 of 5",
            0x35 => "4 of 5", 0x45 => "5 of 5", _ => "",
        };
        if !bsn_s.is_empty() {
            tags.push(pb("BracketShotNumber", bsn_s));
        }
    }

    // Byte 10: WhiteBalanceSet(0xf0), MultipleExposureSet(0x0f)
    if data.len() > 10 {
        let b = data[10];
        let wb = (b & 0xf0) >> 4;
        let wb_s = match wb {
            0 => "Auto", 1 => "Daylight", 2 => "Shade", 3 => "Cloudy",
            4 => "Daylight Fluorescent", 5 => "Day White Fluorescent",
            6 => "White Fluorescent", 7 => "Tungsten", 8 => "Flash", 9 => "Manual",
            12 => "Set Color Temperature 1", 13 => "Set Color Temperature 2",
            14 => "Set Color Temperature 3", _ => "",
        };
        let wb_tmp = if wb_s.is_empty() { wb.to_string() } else { wb_s.to_string() };
        tags.push(pb("WhiteBalanceSet", &wb_tmp));

        let me = b & 0x0f;
        tags.push(pb("MultipleExposureSet", if me == 0 { "Off" } else { "On" }));
    }

    // Byte 13: RawAndJpgRecording
    if data.len() > 13 {
        let b = data[13];
        let s = match b {
            0x01 => "JPEG (Best)", 0x04 => "RAW (PEF, Best)", 0x05 => "RAW+JPEG (PEF, Best)",
            0x08 => "RAW (DNG, Best)", 0x09 => "RAW+JPEG (DNG, Best)",
            0x21 => "JPEG (Better)", 0x24 => "RAW (PEF, Better)", 0x25 => "RAW+JPEG (PEF, Better)",
            0x28 => "RAW (DNG, Better)", 0x29 => "RAW+JPEG (DNG, Better)",
            0x41 => "JPEG (Good)", 0x44 => "RAW (PEF, Good)", 0x45 => "RAW+JPEG (PEF, Good)",
            0x48 => "RAW (DNG, Good)", 0x49 => "RAW+JPEG (DNG, Good)", _ => "",
        };
        if !s.is_empty() {
            tags.push(pb("RawAndJpgRecording", s));
        }
    }

    // Byte 14: JpgRecordedPixels(0x03) — K10D only
    if data.len() > 14 {
        let b = data[14];
        let jp = b & 0x03;
        let jp_s = match jp { 0 => "10 MP", 1 => "6 MP", 2 => "2 MP", _ => "" };
        if !jp_s.is_empty() {
            tags.push(pb("JpgRecordedPixels", jp_s));
        }

        // SensitivitySteps(0x02) for K-5
        let ss = (b & 0x02) >> 1;
        tags.push(pb("SensitivitySteps", if ss == 0 { "1 EV Steps" } else { "As EV Steps" }));
    }

    // Byte 16: FlashOptions2(0xf0), MeteringMode3(0x0f) — K10D only
    if data.len() > 16 {
        let b = data[16];
        let fo2 = (b & 0xf0) >> 4;
        let fo2_s = match fo2 {
            0 => "Normal", 1 => "Red-eye reduction", 2 => "Auto",
            3 => "Auto, Red-eye reduction", 5 => "Wireless (Master)",
            6 => "Wireless (Control)", 8 => "Slow-sync",
            9 => "Slow-sync, Red-eye reduction", 10 => "Trailing-curtain Sync", _ => "",
        };
        let fo2_tmp = if fo2_s.is_empty() { fo2.to_string() } else { fo2_s.to_string() };
        tags.push(pb("FlashOptions2", &fo2_tmp));

        let mm3 = b & 0x0f;
        let mm3_s = match mm3 {
            0 => "Multi-segment",
            v if v & 0x01 != 0 && v & 0x02 != 0 => "Center-weighted average, Spot",
            v if v & 0x01 != 0 => "Center-weighted average",
            v if v & 0x02 != 0 => "Spot",
            _ => "",
        };
        let mm3_tmp = if mm3_s.is_empty() { mm3.to_string() } else { mm3_s.to_string() };
        tags.push(pb("MeteringMode3", &mm3_tmp));
    }

    // Byte 17: SRActive(0x80), Rotation(0x60), ISOSetting(0x04), SensitivitySteps(0x02) — K10D
    if data.len() > 17 {
        let b = data[17];
        let sr = (b & 0x80) >> 7;
        tags.push(pb("SRActive", if sr == 0 { "No" } else { "Yes" }));

        let rot = (b & 0x60) >> 5;
        let rot_s = match rot {
            0 => "Horizontal (normal)", 1 => "Rotate 180",
            2 => "Rotate 90 CW", 3 => "Rotate 270 CW", _ => "",
        };
        tags.push(pb("Rotation", rot_s));

        let iso = (b & 0x04) >> 2;
        tags.push(pb("ISOSetting", if iso == 0 { "Manual" } else { "Auto" }));

        // Remove SensitivitySteps from byte 17 since byte 14 already has it for K10D
        // (Perl uses Condition to differentiate models; K10D uses byte 17.4)
        let sens = (b & 0x02) >> 1;
        // Only emit if not already emitted from byte 14 (K10D vs K-5 models differ)
        // We emit it from byte 17 for K10D as that's what the test file shows
        let _ = sens; // Already handled from byte 14
    }

    // Byte 18: TvExposureTimeSetting — ValueConv: exp(-PentaxEv(val-68)*log(2))
    if data.len() > 18 {
        let raw = data[18] as i32;
        let ev = pentax_ev(raw - 68);
        let tv = (-ev * std::f64::consts::LN_2).exp();
        tags.push(pb("TvExposureTimeSetting", &print_exposure_time(tv)));
    }

    // Byte 19: AvApertureSetting — ValueConv: exp(PentaxEv(val-68)*log(2)/2)
    if data.len() > 19 {
        let raw = data[19] as i32;
        let ev = pentax_ev(raw - 68);
        let av = (ev * std::f64::consts::LN_2 / 2.0).exp();
        tags.push(pb("AvApertureSetting", &format!("{:.1}", av)));
    }

    // Byte 20: SvISOSetting — ValueConv: int(100*exp(PentaxEv(val-32)*log(2))+0.5)
    if data.len() > 20 {
        let raw = data[20] as i32;
        let ev = pentax_ev(raw - 32);
        let iso = (100.0 * (ev * std::f64::consts::LN_2).exp() + 0.5) as u32;
        tags.push(pb("SvISOSetting", &iso.to_string()));
    }

    // Byte 21: BaseExposureCompensation — ValueConv: PentaxEv(64-val)
    if data.len() > 21 {
        let raw = data[21] as i32;
        let ev = pentax_ev(64 - raw);
        let s = if ev == 0.0 { "0".to_string() } else { format!("{:+.1}", ev) };
        tags.push(pb("BaseExposureCompensation", &s));
    }

    tags
}

/// Decode Pentax AEInfo (tag 0x0206).
/// From Perl Pentax::AEInfo table.
fn decode_pentax_ae_info(data: &[u8]) -> Vec<Tag> {
    let mut tags = Vec::new();
    let pb = |name: &str, v: &str| mk_pentax(name, v);

    // Byte 0: AEExposureTime — 24*exp(-(val-32)*ln(2)/8)
    if data.len() > 0 {
        let raw = data[0] as f64;
        let tv = 24.0 * (-(raw - 32.0) * std::f64::consts::LN_2 / 8.0).exp();
        tags.push(pb("AEExposureTime", &print_exposure_time(tv)));
    }
    // Byte 1: AEAperture — exp((val-68)*ln(2)/16)
    if data.len() > 1 {
        let raw = data[1] as f64;
        let av = ((raw - 68.0) * std::f64::consts::LN_2 / 16.0).exp();
        tags.push(pb("AEAperture", &format!("{:.1}", av)));
    }
    // Byte 2: AE_ISO — 100*exp((val-32)*ln(2)/8)
    if data.len() > 2 {
        let raw = data[2] as f64;
        let iso = (100.0 * ((raw - 32.0) * std::f64::consts::LN_2 / 8.0).exp() + 0.5) as u32;
        tags.push(pb("AE_ISO", &iso.to_string()));
    }
    // Byte 3: AEXv — (val-64)/8
    if data.len() > 3 {
        let raw = data[3] as f64;
        tags.push(pb("AEXv", &format!("{:.4}", (raw - 64.0) / 8.0)));
    }
    // Byte 4: AEBXv (int8s) — val/8
    if data.len() > 4 {
        let raw = data[4] as i8 as f64;
        tags.push(pb("AEBXv", &format!("{:.4}", raw / 8.0)));
    }
    // Byte 5: AEMinExposureTime
    if data.len() > 5 {
        let raw = data[5] as f64;
        let tv = 24.0 * (-(raw - 32.0) * std::f64::consts::LN_2 / 8.0).exp();
        tags.push(pb("AEMinExposureTime", &print_exposure_time(tv)));
    }
    // Byte 6: AEProgramMode
    if data.len() > 6 {
        let b = data[6];
        let s = match b {
            0 => "M, P or TAv", 1 => "Av, B or X", 2 => "Tv", 3 => "Sv or Green Mode",
            8 => "Hi-speed Program", 11 => "Hi-speed Program (P-Shift)",
            16 => "DOF Program", 19 => "DOF Program (P-Shift)",
            24 => "MTF Program", 27 => "MTF Program (P-Shift)",
            35 => "Standard", 43 => "Portrait", 51 => "Landscape",
            59 => "Macro", 67 => "Sport", 75 => "Night Scene Portrait",
            83 => "No Flash", 91 => "Night Scene", 99 => "Surf & Snow",
            107 => "Text", 115 => "Sunset", 123 => "Kids",
            131 => "Pet", 139 => "Candlelight", 147 => "Museum",
            184 => "Shallow DOF Program", _ => "",
        };
        let aepm_tmp = if s.is_empty() { b.to_string() } else { s.to_string() };
        tags.push(pb("AEProgramMode", &aepm_tmp));
    }
    // Byte 8 (or 7 for small records): AEApertureSteps
    let offset_adj = if data.len() > 20 { 1usize } else { 0usize }; // Hook: size > 20 shifts by 1
    let base = 7 + offset_adj; // AEFlags at 7, then AEApertureSteps at 8
    if data.len() > base + 1 {
        let b = data[base + 1];
        let aeas_tmp = if b == 255 { "n/a".to_string() } else { b.to_string() };
        tags.push(pb("AEApertureSteps", &aeas_tmp));
    }
    // AEMaxAperture
    if data.len() > base + 2 {
        let raw = data[base + 2] as f64;
        let av = ((raw - 68.0) * std::f64::consts::LN_2 / 16.0).exp();
        tags.push(pb("AEMaxAperture", &format!("{:.1}", av)));
    }
    // AEMaxAperture2
    if data.len() > base + 3 {
        let raw = data[base + 3] as f64;
        let av = ((raw - 68.0) * std::f64::consts::LN_2 / 16.0).exp();
        tags.push(pb("AEMaxAperture2", &format!("{:.1}", av)));
    }
    // AEMinAperture
    if data.len() > base + 4 {
        let raw = data[base + 4] as f64;
        let av = ((raw - 68.0) * std::f64::consts::LN_2 / 16.0).exp();
        tags.push(pb("AEMinAperture", &format!("{:.0}", av)));
    }
    // AEMeteringMode
    if data.len() > base + 5 {
        let b = data[base + 5];
        let s = if b == 0 { "Multi-segment" }
            else if b & 0x10 != 0 && b & 0x20 != 0 { "Center-weighted average, Spot" }
            else if b & 0x10 != 0 { "Center-weighted average" }
            else if b & 0x20 != 0 { "Spot" }
            else { "" };
        let aemm_tmp = if s.is_empty() { b.to_string() } else { s.to_string() };
        tags.push(pb("AEMeteringMode", &aemm_tmp));
    }
    // FlashExposureCompSet (byte 14 from start, int8s) — ValueConv: PentaxEv(val)
    if data.len() > 14 {
        let raw = data[14] as i8 as i32;
        let ev = pentax_ev(raw);
        let s = if ev == 0.0 { "0".to_string() } else { format!("{:+.1}", ev) };
        tags.push(pb("FlashExposureCompSet", &s));
    }

    tags
}

/// Decode Pentax LensInfo (tag 0x0207) — dispatches based on data length.
/// From Perl: LensInfo (20 bytes), LensInfo2 (21 bytes), LensInfo4 (91 bytes), etc.
fn decode_pentax_lens_info(data: &[u8]) -> Vec<Tag> {
    let mut tags = Vec::new();
    let pb = |name: &str, v: &str| mk_pentax(name, v);
    let n = data.len();

    // Determine LensType and LensData start offset
    // LensInfo (old, ≤20 bytes): LensType at [0..2], LensData at [3..20]
    // LensInfo2 (21-89 bytes): LensType at [0..4] with transform, LensData at [4..21]
    // LensInfo4 (91 bytes): LensType at [1..5], LensData at [12..30]
    let (lens_data_start, lens_type_str) = if n <= 20 {
        // Old format: LensType as 2 bytes
        let lt = if n >= 2 { format!("{} {}", data[0], data[1]) } else { "0 0".to_string() };
        tags.push(pb("LensType", &lt));
        (3usize, lt)
    } else if n == 91 {
        // LensInfo4 format (K-r, K-5): LensType at bytes 1-4
        if n >= 5 {
            let b = data[1..5].to_vec();
            let t0 = b[0] & 0x0f;
            let t1 = (b[2] as u16) * 256 + b[3] as u16;
            let lt = format!("{} {}", t0, t1);
            tags.push(pb("LensType", &lt));
        }
        (12usize, "".to_string())
    } else {
        // LensInfo2 format (most models): LensType at bytes 0-3
        if n >= 4 {
            let t0 = data[0] & 0x0f;
            let t1 = (data[2] as u16) * 256 + data[3] as u16;
            let lt = format!("{} {}", t0, t1);
            tags.push(pb("LensType", &lt));
        }
        (4usize, "".to_string())
    };
    let _ = lens_type_str;

    // Decode LensData starting at lens_data_start
    if n > lens_data_start {
        let ld = &data[lens_data_start..];
        decode_pentax_lens_data(ld, &mut tags);
    }

    tags
}

/// Decode Pentax LensData sub-table (17-18 bytes binary).
/// From Perl Pentax::LensData table.
fn decode_pentax_lens_data(d: &[u8], tags: &mut Vec<Tag>) {
    let pb = |name: &str, v: &str| mk_pentax(name, v);

    // Byte 0: AutoAperture(bit0), MinAperture(bits 1-2), LensFStops(bits 4-6)
    if d.len() > 0 {
        let b = d[0];
        let aa = b & 0x01;
        tags.push(pb("AutoAperture", if aa == 0 { "On" } else { "Off" }));

        let ma_raw = (b & 0x06) >> 1;
        let ma_s = match ma_raw { 0 => "22", 1 => "32", 2 => "45", 3 => "16", _ => "" };
        tags.push(pb("MinAperture", ma_s));

        let lf = (b & 0x70) >> 4;
        let lf_stops = 5.0 + (lf ^ 0x07) as f64 / 2.0;
        tags.push(pb("LensFStops", &format!("{:.1}", lf_stops)));
    }

    // Byte 3: MinFocusDistance(bits 7-3), FocusRangeIndex(bits 2-0)
    if d.len() > 3 {
        let b = d[3];
        let mfd_raw = (b & 0xf8) >> 3;
        let mfd_s = match mfd_raw {
            0 => "0.13-0.19 m", 1 => "0.20-0.24 m", 2 => "0.25-0.28 m",
            3 => "0.28-0.30 m", 4 => "0.35-0.38 m", 5 => "0.40-0.45 m",
            6 => "0.49-0.50 m", 7 => "0.6 m", 8 => "0.7 m",
            9 => "0.8-0.9 m", 10 => "1.0 m", 11 => "1.1-1.2 m",
            12 => "1.4-1.5 m", 13 => "1.5 m", 14 => "2.0 m",
            15 => "2.0-2.1 m", 16 => "2.1 m", 17 => "2.2-2.9 m",
            18 => "3.0 m", 19 => "4-5 m", 20 => "5.6 m", _ => "",
        };
        if !mfd_s.is_empty() {
            tags.push(pb("MinFocusDistance", mfd_s));
        }

        let fri = b & 0x07;
        let fri_s = match fri {
            7 => "0 (very close)", 6 => "1 (close)", 4 => "2",
            5 => "3", 1 => "4", 0 => "5", 2 => "6 (far)", 3 => "7 (very far)", _ => "",
        };
        if !fri_s.is_empty() {
            tags.push(pb("FocusRangeIndex", fri_s));
        }
    }

    // Byte 9: LensFocalLength — 10*(val>>2) * 4**((val&3)-2)
    if d.len() > 9 {
        let b = d[9];
        let fl = 10.0 * (b >> 2) as f64 * 4.0_f64.powi((b & 0x03) as i32 - 2);
        tags.push(pb("LensFocalLength", &format!("{:.1} mm", fl)));
    }

    // Byte 10: NominalMaxAperture(bits 7-4), NominalMinAperture(bits 3-0)
    if d.len() > 10 {
        let b = d[10];
        let nmax = (b & 0xf0) >> 4;
        let nmin = b & 0x0f;
        let nmax_av = 2.0_f64.powf(nmax as f64 / 4.0);
        let nmin_av = 2.0_f64.powf((nmin as f64 + 10.0) / 4.0);
        tags.push(pb("NominalMaxAperture", &format!("{:.1}", nmax_av)));
        tags.push(pb("NominalMinAperture", &format!("{:.0}", nmin_av)));
    }

    // Byte 14: MaxAperture (bits 6-0, mask 0x7f) — val = 2**((raw-1)/32)
    if d.len() > 14 {
        let b = d[14] & 0x7f;
        if b > 1 {
            let av = 2.0_f64.powf((b as f64 - 1.0) / 32.0);
            tags.push(pb("MaxAperture", &format!("{:.1}", av)));
        }
    }
}

/// Decode Pentax FlashInfo (tag 0x0208, 27 bytes).
/// From Perl Pentax::FlashInfo table.
fn decode_pentax_flash_info(data: &[u8]) -> Vec<Tag> {
    let mut tags = Vec::new();
    if data.len() < 27 { return tags; }
    let pb = |name: &str, v: &str| mk_pentax(name, v);

    // Byte 0: FlashStatus
    let fs = data[0];
    let fs_s = match fs {
        0x00 => "Off", 0x01 => "Off (1)", 0x02 => "External, Did not fire",
        0x06 => "External, Fired", 0x08 => "Internal, Did not fire (0x08)",
        0x09 => "Internal, Did not fire", 0x0d => "Internal, Fired", _ => "",
    };
    let fs_tmp = if fs_s.is_empty() { format!("0x{:02x}", fs) } else { fs_s.to_string() };
    tags.push(pb("FlashStatus", &fs_tmp));

    // Byte 1: InternalFlashMode
    let ifm = data[1];
    let ifm_s = match ifm {
        0x00 => "n/a - Off-Auto-Aperture", 0x86 => "Fired, Wireless (Control)",
        0x95 => "Fired, Wireless (Master)", 0xc0 => "Fired",
        0xc1 => "Fired, Red-eye reduction", 0xc2 => "Fired, Auto",
        0xc3 => "Fired, Auto, Red-eye reduction",
        0xc6 => "Fired, Wireless (Control), Fired normally not as control",
        0xc8 => "Fired, Slow-sync", 0xc9 => "Fired, Slow-sync, Red-eye reduction",
        0xca => "Fired, Trailing-curtain Sync", 0xf0 => "Did not fire, Normal",
        0xf1 => "Did not fire, Red-eye reduction", 0xf2 => "Did not fire, Auto",
        0xf3 => "Did not fire, Auto, Red-eye reduction",
        0xf4 => "Did not fire, (Unknown 0xf4)", 0xf5 => "Did not fire, Wireless (Master)",
        0xf6 => "Did not fire, Wireless (Control)", 0xf8 => "Did not fire, Slow-sync",
        0xf9 => "Did not fire, Slow-sync, Red-eye reduction",
        0xfa => "Did not fire, Trailing-curtain Sync", _ => "",
    };
    let ifm_tmp = if ifm_s.is_empty() { format!("0x{:02x}", ifm) } else { ifm_s.to_string() };
    tags.push(pb("InternalFlashMode", &ifm_tmp));

    // Byte 2: ExternalFlashMode
    let efm = data[2];
    let efm_s = match efm {
        0x00 => "n/a - Off-Auto-Aperture", 0x3f => "Off", 0x40 => "On, Auto",
        0xbf => "On, Flash Problem", 0xc0 => "On, Manual", 0xc4 => "On, P-TTL Auto",
        0xc5 => "On, Contrast-control Sync", 0xc6 => "On, High-speed Sync",
        0xcc => "On, Wireless", 0xcd => "On, Wireless, High-speed Sync",
        0xf0 => "Not Connected", _ => "",
    };
    let efm_tmp = if efm_s.is_empty() { format!("0x{:02x}", efm) } else { efm_s.to_string() };
    tags.push(pb("ExternalFlashMode", &efm_tmp));

    // Byte 3: InternalFlashStrength
    tags.push(pb("InternalFlashStrength", &data[3].to_string()));

    // Bytes 4-7: TTL_DA_AUp, TTL_DA_ADown, TTL_DA_BUp, TTL_DA_BDown
    tags.push(pb("TTL_DA_AUp", &data[4].to_string()));
    tags.push(pb("TTL_DA_ADown", &data[5].to_string()));
    tags.push(pb("TTL_DA_BUp", &data[6].to_string()));
    tags.push(pb("TTL_DA_BDown", &data[7].to_string()));

    // Byte 24: ExternalFlashGuideNumber (bits 4-0, mask 0x1f)
    if data.len() > 24 {
        let raw = (data[24] & 0x1f) as i32;
        let gn_s = if raw == 0 {
            "n/a".to_string()
        } else {
            let raw_adj = if raw == 29 { -3i32 } else { raw };
            let gn = 2.0_f64.powf(raw_adj as f64 / 16.0 + 4.0);
            format!("{}", gn.round() as i64)
        };
        tags.push(pb("ExternalFlashGuideNumber", &gn_s));
    }

    // Byte 25: ExternalFlashExposureComp
    if data.len() > 25 {
        let b = data[25];
        let ec_s = match b {
            0 => "n/a", 144 => "n/a (Manual Mode)",
            164 => "-3.0", 167 => "-2.5", 168 => "-2.0", 171 => "-1.5",
            172 => "-1.0", 175 => "-0.5", 176 => "0.0", 179 => "0.5", 180 => "1.0",
            _ => "",
        };
        let ec_tmp = if ec_s.is_empty() { b.to_string() } else { ec_s.to_string() };
        tags.push(pb("ExternalFlashExposureComp", &ec_tmp));
    }

    // Byte 26: ExternalFlashBounce
    if data.len() > 26 {
        let b = data[26];
        let fb_s = match b { 0 => "n/a", 16 => "Direct", 48 => "Bounce", _ => "" };
        let fb_tmp = if fb_s.is_empty() { b.to_string() } else { fb_s.to_string() };
        tags.push(pb("ExternalFlashBounce", &fb_tmp));
    }

    tags
}

/// Decode Pentax CameraInfo (tag 0x0215, int32u format).
/// From Perl Pentax::CameraInfo table.
fn decode_pentax_camera_info(data: &[u8], byte_order: ByteOrderMark) -> Vec<Tag> {
    let mut tags = Vec::new();
    let pb = |name: &str, v: &str| mk_pentax(name, v);
    if data.len() < 4 { return tags; }

    // Word 0: PentaxModelID (priority 0 — skip)
    // Word 1: ManufactureDate — format YYYYMMDD as YYYY:MM:DD
    if data.len() >= 8 {
        let raw = read_u32(data, 4, byte_order);
        let s = raw.to_string();
        let date = if s.len() == 8 {
            format!("{}:{}:{}", &s[0..4], &s[4..6], &s[6..8])
        } else if s.len() == 7 {
            format!("200{}:{}:{}", &s[0..1], &s[1..3], &s[3..5])
        } else {
            format!("Unknown ({})", raw)
        };
        tags.push(pb("ManufactureDate", &date));
    }

    // Word 2+3: ProductionCode (int32u[2]) — join with "."
    if data.len() >= 16 {
        let a = read_u32(data, 8, byte_order);
        let b = read_u32(data, 12, byte_order);
        tags.push(pb("ProductionCode", &format!("{}.{}", a, b)));
    }

    // Word 4: InternalSerialNumber
    if data.len() >= 20 {
        let sn = read_u32(data, 16, byte_order);
        tags.push(pb("InternalSerialNumber", &sn.to_string()));
    }

    tags
}

/// Decode Pentax BatteryInfo (tag 0x0216).
/// From Perl Pentax::BatteryInfo table.
fn decode_pentax_battery_info(data: &[u8]) -> Vec<Tag> {
    let mut tags = Vec::new();
    let pb = |name: &str, v: &str| mk_pentax(name, v);
    if data.is_empty() { return tags; }

    // Byte 0.1: PowerSource (mask 0x0f)
    let b0 = data[0];
    let ps = b0 & 0x0f;
    let ps_s = match ps {
        1 => "Camera Battery", 2 => "Body Battery",
        3 => "Grip Battery", 4 => "External Power Supply", _ => "",
    };
    let ps_tmp = if ps_s.is_empty() { ps.to_string() } else { ps_s.to_string() };
    tags.push(pb("PowerSource", &ps_tmp));

    if data.len() > 1 {
        let b1 = data[1];
        // Byte 1.1: BodyBatteryState (mask 0xf0) >> 4
        let bbs = (b1 & 0xf0) >> 4;
        let bbs_s = match bbs {
            1 => "Empty or Missing", 2 => "Almost Empty",
            3 => "Running Low", 4 => "Full", 5 => "Full", _ => "",
        };
        let bbs_tmp = if bbs_s.is_empty() { bbs.to_string() } else { bbs_s.to_string() };
        tags.push(pb("BodyBatteryState", &bbs_tmp));

        // Byte 1.2: GripBatteryState (mask 0x0f)
        let gbs = b1 & 0x0f;
        let gbs_s = match gbs {
            1 => "Empty or Missing", 2 => "Almost Empty",
            3 => "Running Low", 4 => "Full", _ => "",
        };
        let gbs_tmp = if gbs_s.is_empty() { gbs.to_string() } else { gbs_s.to_string() };
        tags.push(pb("GripBatteryState", &gbs_tmp));
    }

    // Bytes 2-5: BodyBatteryADNoLoad, BodyBatteryADLoad, GripBatteryADNoLoad, GripBatteryADLoad
    if data.len() > 2 { tags.push(pb("BodyBatteryADNoLoad", &data[2].to_string())); }
    if data.len() > 3 { tags.push(pb("BodyBatteryADLoad", &data[3].to_string())); }
    if data.len() > 4 { tags.push(pb("GripBatteryADNoLoad", &data[4].to_string())); }
    if data.len() > 5 { tags.push(pb("GripBatteryADLoad", &data[5].to_string())); }

    tags
}

/// Decode Pentax AFInfo (tag 0x021F).
/// From Perl Pentax::AFInfo table.
fn decode_pentax_af_info(data: &[u8], byte_order: ByteOrderMark) -> Vec<Tag> {
    let mut tags = Vec::new();
    let pb = |name: &str, v: &str| mk_pentax(name, v);

    // Bytes 4-5: AFPredictor (int16s)
    if data.len() > 5 {
        let v = read_u16(data, 4, byte_order) as i16;
        tags.push(pb("AFPredictor", &v.to_string()));
    }

    // Byte 6: AFDefocus
    if data.len() > 6 {
        tags.push(pb("AFDefocus", &data[6].to_string()));
    }

    // Byte 7: AFIntegrationTime — val*2 ms
    if data.len() > 7 {
        let ms = (data[7] as u32) * 2;
        tags.push(pb("AFIntegrationTime", &format!("{} ms", ms)));
    }

    // Byte 11: AFPointsInFocus
    if data.len() > 11 {
        let b = data[11];
        let s = match b {
            0 => "None", 1 => "Lower-left, Bottom", 2 => "Bottom",
            3 => "Lower-right, Bottom", 4 => "Mid-left, Center",
            5 => "Center (horizontal)", 6 => "Mid-right, Center",
            7 => "Upper-left, Top", 8 => "Top", 9 => "Upper-right, Top",
            10 => "Right", 11 => "Lower-left, Mid-left", 12 => "Upper-left, Mid-left",
            13 => "Bottom, Center", 14 => "Top, Center",
            15 => "Lower-right, Mid-right", 16 => "Upper-right, Mid-right",
            17 => "Left", 18 => "Mid-left", 19 => "Center (vertical)", 20 => "Mid-right",
            _ => "",
        };
        let af_tmp = if s.is_empty() { b.to_string() } else { s.to_string() };
        tags.push(pb("AFPointsInFocus", &af_tmp));
    }

    tags
}

/// Decode Pentax ColorInfo (tag 0x0222).
/// Contains WBShiftAB (byte 0x10, int8s) and WBShiftGM (byte 0x11, int8s).
fn decode_pentax_color_info(data: &[u8]) -> Vec<Tag> {
    let mut tags = Vec::new();
    let pb = |name: &str, v: &str| mk_pentax(name, v);

    if data.len() > 0x10 {
        let ab = data[0x10] as i8;
        tags.push(pb("WBShiftAB", &ab.to_string()));
    }
    if data.len() > 0x11 {
        let gm = data[0x11] as i8;
        tags.push(pb("WBShiftGM", &gm.to_string()));
    }

    tags
}

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
        _ => {
            // Unrecognized version - encrypted versions handled by decrypt_nikon_subtables
        }
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
                tags.push(mk_nikon_str("ShotInfoVersion", version));

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
                    tags.push(mk_nikon_str("FlashInfoVersion", fi_ver));
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
                        tags.push(mk_nikon_str("FlashSource", flash_source));
                    }

                    // FlashFirmware at offset 6
                    if decrypted.len() >= 8 {
                        let fw_major = decrypted[6];
                        let fw_minor = decrypted[7];
                        if fw_major > 0 {
                            tags.push(mk_nikon_str("ExternalFlashFirmware",
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
    if mn_data.len() >= 12 && mn_data.starts_with(b"FUJIFILM") {
        let ifd_off = u32::from_le_bytes([mn_data[8], mn_data[9], mn_data[10], mn_data[11]]) as usize;
        return MakerNoteInfo {
            manufacturer: Manufacturer::Fujifilm,
            ifd_offset: ifd_off,
            _base_adjust: 0,
            byte_order: Some(ByteOrderMark::LittleEndian),
        };
    }

    // GENERALE (GE cameras use Fujifilm-like format)
    if mn_data.len() >= 12 && mn_data.starts_with(b"GENERALE") {
        let ifd_off = u32::from_le_bytes([mn_data[8], mn_data[9], mn_data[10], mn_data[11]]) as usize;
        return MakerNoteInfo {
            manufacturer: Manufacturer::GE,
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
            manufacturer: Manufacturer::GE,
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
    } else if make_upper.starts_with("GENERAL") || make_upper.starts_with("GEDSC") {
        Manufacturer::GE
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

    // Pentax PreviewImage state: track PreviewImageStart and PreviewImageLength
    let mut pentax_preview_start: Option<usize> = None;
    let mut pentax_preview_length: Option<usize> = None;

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
                    crate::tags::canon_sub::decode_focal_length(&values, model_name)
                }
                (Manufacturer::Canon, 0x000D) => {
                    let variant_tags = subs::dispatch_canon_camera_info(&dispatch_ctx);
                    let known_format = !variant_tags.is_empty(); // Only decode for known models
                    // Note: variant_tags contains CameraInfoVariant (internal metadata), don't add to output
                    let mut t = Vec::new();
                    t.extend(decode_canon_camera_info_common(value_data, count as usize, byte_order));
                    // Decode CameraInfo1DmkIII fields (FORMAT='int8u', byte offsets)
                    // Perl table: Canon::CameraInfo1DmkIII
                    let d = value_data;
                    // Only decode 1DmkIII-specific layout (data size ~1536 bytes)
                    let is_1dmk3 = model_name.contains("1D Mark III") || model_name.contains("1DS Mark III");
                    if known_format && is_1dmk3 {
                        // Read helpers
                        let rb = |off: usize| -> u8 { if off < d.len() { d[off] } else { 0 } };
                        let r16le = |off: usize| -> u16 {
                            if off + 2 <= d.len() { u16::from_le_bytes([d[off], d[off+1]]) } else { 0 }
                        };
                        let r16be = |off: usize| -> u16 {
                            if off + 2 <= d.len() { u16::from_be_bytes([d[off], d[off+1]]) } else { 0 }
                        };
                        let r32le = |off: usize| -> u32 {
                            if off + 4 <= d.len() { u32::from_le_bytes([d[off],d[off+1],d[off+2],d[off+3]]) } else { 0 }
                        };
                        // Single-byte fields (int8u)
                        if d.len() > 48 { t.push(mk_canon_str("CameraOrientation", &{
                            let v = rb(0x30);
                            let s = match v { 0=>"Horizontal (normal)", 1=>"Rotate 90 CW", 2=>"Rotate 270 CW", _=> "" };
                            if s.is_empty() { v.to_string() } else { s.to_string() }
                        })); }
                        if d.len() > 134 { t.push(mk_canon_str("PictureStyle", &rb(0x86).to_string())); }
                        // int16u fields (little-endian)
                        if d.len() > 96 { let v = r16le(0x5e); let pv_s; let pv = canon_wb_name(v as i16); let pv_owned = if pv.is_empty() { pv_s = v.to_string(); pv_s.as_str() } else { pv }; t.push(mk_canon_str("WhiteBalance", pv_owned)); }
                        if d.len() > 100 { let v = r16le(0x62); if v > 0 { t.push(mk_canon_str("ColorTemperature", &v.to_string())); } }
                        // LensType at 0x111 = 273 (big-endian int16u)
                        if d.len() > 275 { t.push(mk_canon_str("MinFocalLength", &r16le(0x113).to_string())); }
                        if d.len() > 277 { t.push(mk_canon_str("MaxFocalLength", &r16le(0x115).to_string())); }
                        // FirmwareVersion string at 0x136 = 310, length 6
                        if d.len() >= 316 {
                            let fw = String::from_utf8_lossy(&d[0x136..0x136+6]).trim_end_matches('\0').to_string();
                            if !fw.is_empty() { t.push(mk_canon_str("FirmwareVersion", &fw)); }
                        }
                        // int32u fields (little-endian)
                        // FileIndex at 0x172 = 370, ValueConv += 1
                        if d.len() > 374 { let v = r32le(0x172); t.push(mk_canon_str("FileIndex", &(v + 1).to_string())); }
                        // ShutterCount at 0x176 = 374, ValueConv += 1
                        if d.len() > 378 { let v = r32le(0x176); t.push(mk_canon_str("ShutterCount", &(v + 1).to_string())); }
                        // DirectoryIndex at 0x17e = 382, ValueConv -= 1
                        if d.len() > 386 { let v = r32le(0x17e) as i32; t.push(mk_canon_str("DirectoryIndex", &(v - 1).to_string())); }
                        // TimeStamp1 at 0x45a = 1114 (only for 1DMarkIII, not 1DSmkIII)
                        // RawConv => '$val ? $val : undef' (suppress if 0)
                        if model_name.contains("1D Mark III") && d.len() > 1118 {
                            let v = r32le(0x45a);
                            if v > 0 {
                                let dt = unix_time_to_datetime(v);
                                t.push(mk_canon_str("TimeStamp1", &dt));
                            }
                        }
                        // TimeStamp at 0x45e = 1118 (both 1DmkIII and 1DSmkIII)
                        if d.len() > 1122 {
                            let v = r32le(0x45e);
                            if v > 0 {
                                let dt = unix_time_to_datetime(v);
                                t.push(mk_canon_str("TimeStamp", &dt));
                            }
                        }
                        // PictureStyleInfo at 0x2aa = 682 (SubDirectory — PSInfo table)
                        let ps_base = 0x2aausize;
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
                        if d.len() > ps_base + 222 {
                            for &(off, name) in PS_FIELDS {
                                let abs = ps_base + off;
                                if abs + 4 <= d.len() {
                                    let v = i32::from_le_bytes([d[abs], d[abs+1], d[abs+2], d[abs+3]]);
                                    // UserDefNPictureStyle: print as picture style name
                                    let pv = if name.starts_with("UserDef") && name.ends_with("Style") {
                                        let s = match v as u32 { 0x41 => "Standard", 0x42 => "Portrait", 0x43 => "Landscape",
                                            0x44 => "Neutral", 0x45 => "Faithful", 0x51 => "Monochrome",
                                            0x81 => "Standard", 0x82 => "Portrait", _ => "" };
                                        if s.is_empty() { v.to_string() } else { s.to_string() }
                                    } else { v.to_string() };
                                    t.push(mk_canon_str(name, &pv));
                                }
                            }
                        }
                        let _ = r16be; // suppress unused warning
                    } else if known_format {
                        // Generic CameraInfo fields for other models (byte offsets, int8u single bytes)
                        // This is a fallback for non-1DmkIII models that were using the old code
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
                    // Canon AspectInfo: int32u format (from Perl Canon::AspectInfo)
                    // index 0: AspectRatio, 1: CroppedImageWidth, 2: CroppedImageHeight,
                    // 3: CroppedImageLeft, 4: CroppedImageTop
                    let mut t = Vec::new();
                    let n = count as usize;
                    if n >= 1 {
                        let v = read_u32(value_data, 0, byte_order);
                        let s = match v { 0 => "3:2", 1 => "1:1", 2 => "4:3", 7 => "16:9", 8 => "4:5",
                            12 => "3:2 (APS-H crop)", 13 => "3:2 (APS-C crop)", 258 => "4:3 crop", _ => "" };
                        if !s.is_empty() { t.push(mk_canon_str("AspectRatio", s)); }
                        // CroppedImage dimensions at indices 1-4 (when count >= 5)
                        if n >= 5 {
                            let names = ["CroppedImageWidth","CroppedImageHeight","CroppedImageLeft","CroppedImageTop"];
                            for (i, name) in names.iter().enumerate() {
                                let v = read_u32(value_data, (i+1)*4, byte_order);
                                t.push(mk_canon_str(name, &v.to_string()));
                            }
                        }
                    }
                    t
                }
                (Manufacturer::Canon, 0x0098) => {
                    // Canon CropInfo: int16u format (from Perl Canon::CropInfo)
                    let mut t = Vec::new();
                    if count as usize >= 4 {
                        let rd = |i: usize| -> u16 { read_u16(value_data, i * 2, byte_order) };
                        t.push(mk_canon_str("CropLeftMargin", &rd(0).to_string()));
                        t.push(mk_canon_str("CropRightMargin", &rd(1).to_string()));
                        t.push(mk_canon_str("CropTopMargin", &rd(2).to_string()));
                        t.push(mk_canon_str("CropBottomMargin", &rd(3).to_string()));
                    }
                    t
                }
                (Manufacturer::Canon, 0x00A0) => {
                    // Canon ProcessingInfo: int16s format (from Perl Canon::Processing)
                    // FIRST_ENTRY=1, so index i corresponds to int16s[i] (0-based)
                    let mut t = Vec::new();
                    let rd = |i: usize| -> i16 { read_u16(value_data, i * 2, byte_order) as i16 };
                    if count as usize >= 8 {
                        t.push(mk_canon_str("ToneCurve", &rd(1).to_string()));
                        // index 2 = Sharpness (condition-based, skip)
                        t.push(mk_canon_str("SharpnessFrequency", &rd(3).to_string()));
                        t.push(mk_canon_str("SensorRedLevel", &rd(4).to_string()));
                        t.push(mk_canon_str("SensorBlueLevel", &rd(5).to_string()));
                        t.push(mk_canon_str("WhiteBalanceRed", &rd(6).to_string()));
                        t.push(mk_canon_str("WhiteBalanceBlue", &rd(7).to_string()));
                    }
                    if count as usize >= 14 {
                        t.push(mk_canon_str("DigitalGain", &rd(11).to_string()));
                        t.push(mk_canon_str("WBShiftAB", &rd(12).to_string()));
                        t.push(mk_canon_str("WBShiftGM", &rd(13).to_string()));
                    }
                    t
                }
                (Manufacturer::Canon, 0x0093) => {
                    // Canon FileInfo: int16s format, FIRST_ENTRY=1 (from Perl Canon::FileInfo)
                    // Tag 0x0093 is a subdirectory decoded here
                    let mut t = Vec::new();
                    let rd = |i: usize| -> i16 {
                        if i * 2 + 2 > value_data.len() { return 0; }
                        read_u16(value_data, i * 2, byte_order) as i16
                    };
                    let n = count as usize;
                    // index 7: RawJpgSize (skip if < 0)
                    if n > 7 { let v = rd(7); if v >= 0 {
                        let pv = match v { 0=>"Large", 1=>"Medium 1", 2=>"Medium 2", 3=>"Small 1", 4=>"Small 2", 5=>"Small 3", 14=>"Medium", 15=>"Small", _=>""};
                        let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
                        t.push(mk_canon_str("RawJpgSize", &pv));
                    }}
                    // index 8: LongExposureNoiseReduction2 (skip if < 0)
                    if n > 8 { let v = rd(8); if v >= 0 {
                        let pv = match v { 0=>"Off", 1=>"On (1D)", 3=>"On", 4=>"Auto", _=>""};
                        let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
                        t.push(mk_canon_str("LongExposureNoiseReduction2", &pv));
                    }}
                    // index 9: WBBracketMode
                    if n > 9 { let v = rd(9);
                        let pv = match v { 0=>"Off", 1=>"On (shift AB)", 2=>"On (shift GM)", _=>""};
                        let pv = if pv.is_empty() { v.to_string() } else { pv.to_string() };
                        t.push(mk_canon_str("WBBracketMode", &pv));
                    }
                    // index 12: WBBracketValueAB
                    if n > 12 { t.push(mk_canon_str("WBBracketValueAB", &rd(12).to_string())); }
                    // index 13: WBBracketValueGM
                    if n > 13 { t.push(mk_canon_str("WBBracketValueGM", &rd(13).to_string())); }
                    // index 19: LiveViewShooting (off/on)
                    if n > 19 { let v = rd(19);
                        t.push(mk_canon_str("LiveViewShooting", if v == 0 { "Off" } else { "On" }));
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
                (Manufacturer::Canon, 0x4013) => {
                    // Canon AFMicroAdj: int32s, FIRST_ENTRY=1
                    // index 1 (bytes 4..8): AFMicroAdjMode (int32s)
                    // index 2 (bytes 8..16): AFMicroAdjValue (rational64s = num/denom)
                    let mut t = Vec::new();
                    let d = value_data;
                    if d.len() >= 8 {
                        let mode = i32::from_le_bytes([d[4], d[5], d[6], d[7]]);
                        let pv = match mode { 0=>"Disable", 1=>"Adjust all by the same amount", 2=>"Adjust by lens", _=>""};
                        let pv = if pv.is_empty() { mode.to_string() } else { pv.to_string() };
                        t.push(mk_canon_str("AFMicroAdjMode", &pv));
                    }
                    if d.len() >= 16 {
                        let num = i32::from_le_bytes([d[8], d[9], d[10], d[11]]) as f64;
                        let den = i32::from_le_bytes([d[12], d[13], d[14], d[15]]) as f64;
                        let val = if den != 0.0 { num / den } else { 0.0 };
                        t.push(mk_canon_str("AFMicroAdjValue", &format!("{:.0}", val)));
                    }
                    t
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
                    // Ricoh ImageInfo: Big-Endian binary (from Perl Ricoh::ImageInfo)
                    let mut t = Vec::new();
                    let d = value_data;
                    if d.len() >= 4 {
                        let w = u16::from_be_bytes([d[0], d[1]]);
                        let h = u16::from_be_bytes([d[2], d[3]]);
                        t.push(mk_nikon_str("RicohImageWidth", &w.to_string()));
                        t.push(mk_nikon_str("RicohImageHeight", &h.to_string()));
                    }
                    if d.len() >= 13 {
                        // RicohDate at offset 6 (7 bytes hex-encoded date)
                        let date = format!("{:02x}{:02x}:{:02x}:{:02x} {:02x}:{:02x}:{:02x}",
                            d[6], d[7], d[8], d[9], d[10], d[11], d[12]);
                        t.push(mk_nikon_str("RicohDate", &date));
                    }
                    // ManufactureDate at offset 28-35 (from Perl Ricoh.pm)
                    // These come from the main Ricoh IFD, not ImageInfo
                    t
                }
                // Olympus TextInfo (tag 0x0208): space-separated key value pairs
                (Manufacturer::Olympus, 0x0208) | (Manufacturer::OlympusNew, 0x0208) => {
                    let text = String::from_utf8_lossy(value_data);
                    let mut t = Vec::new();
                    // Format: "[section] Key=Value Key=Value" with space separation
                    for token in text.split_whitespace() {
                        if token.starts_with('[') { continue; } // skip section headers
                        if let Some(eq) = token.find('=') {
                            let key = &token[..eq];
                            let val = &token[eq+1..];
                            // Rename "Type" to "CameraType" to avoid conflict
                            let key = if key == "Type" { "CameraType" } else { key };
                            if !key.is_empty() && !val.is_empty() {
                                t.push(Tag {
                                    id: TagId::Text(key.to_string()),
                                    name: key.to_string(), description: key.to_string(),
                                    group: TagGroup { family0: "MakerNotes".into(), family1: "Olympus".into(), family2: "Camera".into() },
                                    raw_value: Value::String(val.to_string()), print_value: val.to_string(), priority: 0,
                                });
                            }
                        }
                    }
                    t
                }
                // Pentax binary sub-tables (from Perl Pentax.pm)
                (Manufacturer::Pentax, 0x0205) => decode_pentax_camera_settings(value_data),
                (Manufacturer::Pentax, 0x0206) => decode_pentax_ae_info(value_data),
                (Manufacturer::Pentax, 0x0207) => decode_pentax_lens_info(value_data),
                (Manufacturer::Pentax, 0x0208) => {
                    if value_data.len() == 27 {
                        decode_pentax_flash_info(value_data)
                    } else {
                        Vec::new() // FlashInfoUnknown — no known tags
                    }
                },
                (Manufacturer::Pentax, 0x0215) => decode_pentax_camera_info(value_data, byte_order),
                (Manufacturer::Pentax, 0x0216) => decode_pentax_battery_info(value_data),
                (Manufacturer::Pentax, 0x021F) => decode_pentax_af_info(value_data, byte_order),
                (Manufacturer::Pentax, 0x0222) => decode_pentax_color_info(value_data),
                (Manufacturer::Pentax, 0x005C) => decode_binary_subtable(value_data, "Pentax", PENTAX_SR_INFO),
                // Apple RunTime plist
                (Manufacturer::Apple, 0x0003) => decode_apple_runtime(value_data),
                // Ricoh RicohSubdir (tag 0x2001): contains ManufactureDate1/ManufactureDate2
                (Manufacturer::Ricoh, 0x2001) => decode_ricoh_subdir(value_data, data, byte_order),
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
            // Determine context-specific tag table
            let oly_table: &[(u16, &str)] = match tag_id {
                0x2010 => &crate::tags::makernotes::OLYMPUS_EQUIPMENT,
                0x2020 => &crate::tags::makernotes::OLYMPUS_CAMERA_SETTINGS,
                0x2030 | 0x2031 => &crate::tags::makernotes::OLYMPUS_RAW_DEV,
                0x2040 => &crate::tags::makernotes::OLYMPUS_IMAGE_PROCESSING,
                0x2050 => &crate::tags::makernotes::OLYMPUS_FOCUS_INFO,
                _ => &[],
            };

            let parse_oly_ifd = |ifd_data: &[u8], ifd_off: usize| -> Vec<Tag> {
                let mut sub_tags = Vec::new();
                if ifd_off + 2 > ifd_data.len() { return sub_tags; }
                let ec = read_u16(ifd_data, ifd_off, byte_order) as usize;
                for j in 0..ec.min(100) {
                    let eoff = ifd_off + 2 + j * 12;
                    if eoff + 12 > ifd_data.len() { break; }
                    let stid = read_u16(ifd_data, eoff, byte_order);
                    // Look up in context-specific table first
                    let name = oly_table.iter().find(|&&(id, _)| id == stid)
                        .map(|&(_, n)| n)
                        .unwrap_or("Unknown");
                    if name == "Unknown" { continue; }
                    let sdt = read_u16(ifd_data, eoff + 2, byte_order);
                    let scnt = read_u32(ifd_data, eoff + 4, byte_order) as usize;
                    let sts = match sdt { 1|2|6|7 => 1, 3|8 => 2, 4|9|11|13 => 4, 5|10|12 => 8, _ => 1 };
                    let stotal = sts * scnt;
                    let sval = if stotal <= 4 {
                        &ifd_data[eoff+8..(eoff+8+stotal).min(ifd_data.len())]
                    } else {
                        let off = read_u32(ifd_data, eoff + 8, byte_order) as usize;
                        if off + stotal <= ifd_data.len() { &ifd_data[off..off+stotal] }
                        else { continue; }
                    };
                    let val = crate::metadata::makernotes::decode_mn_value(sval, sdt, scnt, byte_order);

                    // Special print conversions for Olympus Equipment sub-IFD
                    // LensType (Equipment 0x0201): 6 int8u bytes → key "%x %.2x %.2x" (bytes 0,2,3) → lens name
                    // Extender (Equipment 0x0301): 6 int8u bytes → key "%x %.2x" (bytes 0,2) → extender name
                    let pv: String = if tag_id == 0x2010 && stid == 0x0201 && sdt == 1 && scnt >= 4 {
                        // LensType: ValueConv = sprintf("%x %.2x %.2x", bytes[0], bytes[2], bytes[3])
                        let b0 = sval.first().copied().unwrap_or(0) as u32;
                        let b2 = sval.get(2).copied().unwrap_or(0) as u32;
                        let b3 = sval.get(3).copied().unwrap_or(0) as u32;
                        let key = format!("{:x} {:02x} {:02x}", b0, b2, b3);
                        crate::tags::makernotes::olympus_lens_type_name(&key)
                            .map(|s| s.to_string())
                            .unwrap_or(key)
                    } else if tag_id == 0x2010 && stid == 0x0301 && sdt == 1 && scnt >= 3 {
                        // Extender: ValueConv = sprintf("%x %.2x", bytes[0], bytes[2])
                        let b0 = sval.first().copied().unwrap_or(0) as u32;
                        let b2 = sval.get(2).copied().unwrap_or(0) as u32;
                        let key = format!("{:x} {:02x}", b0, b2);
                        crate::tags::makernotes::olympus_extender_name(&key)
                            .map(|s| s.to_string())
                            .unwrap_or(key)
                    } else {
                        val.to_display_string()
                    };

                    sub_tags.push(Tag {
                        id: TagId::Text(name.to_string()), name: name.to_string(),
                        description: name.to_string(),
                        group: TagGroup { family0: "MakerNotes".into(), family1: "Olympus".into(), family2: "Camera".into() },
                        raw_value: val, print_value: pv, priority: 0,
                    });
                }
                sub_tags
            };

            if (data_type == 4 || data_type == 13) && count == 1 {
                let sub_off = read_u32(value_data, 0, byte_order) as usize;
                if sub_off > 0 && sub_off + 2 < data.len() {
                    let sub_tags = parse_oly_ifd(data, sub_off);
                    if !sub_tags.is_empty() {
                        tags.extend(sub_tags);
                        continue;
                    }
                }
            } else if data_type == 7 && total_size > 12 {
                // Old Olympus inline sub-IFD: value_data IS the IFD blob,
                // but value offsets inside it are TIFF-relative (not blob-relative).
                // Pass the full `data` buffer with the absolute value_offset.
                let sub_off = value_offset as usize;
                let sub_tags = parse_oly_ifd(data, sub_off);
                if !sub_tags.is_empty() {
                    tags.extend(sub_tags);
                    continue;
                }
            }
        }

        // Look up tag name
        let group_name = manufacturer_group_name(manufacturer);
        let (name, description) = mn_tags::lookup(manufacturer, tag_id);

        // Suppress Unknown tags
        if name == "Unknown" || name.contains("Unknown") {
            continue;
        }

        // Suppress Canon tag 0x0000
        if tag_id == 0x0000 && manufacturer == Manufacturer::Canon {
            continue;
        }

        // SubDirectory suppression list: these are container tags, not leaf tags
        let is_subdirectory = matches!((manufacturer, tag_id),
            (Manufacturer::Canon, 0x0001) | // CanonCameraSettings
            (Manufacturer::Canon, 0x0002) | // CanonFocalLength
            (Manufacturer::Canon, 0x0003) | // CanonFlashInfo (Unknown => 1)
            (Manufacturer::Canon, 0x0004) | // CanonShotInfo
            (Manufacturer::Canon, 0x000D) | // CanonCameraInfo
            (Manufacturer::Canon, 0x0093) | // CanonFileInfo (SubDirectory)
            (Manufacturer::Canon, 0x0012) | // CanonAFInfo
            (Manufacturer::Canon, 0x0026) | // CanonAFInfo2
            (Manufacturer::Canon, 0x0098) | // CropInfo
            (Manufacturer::Canon, 0x0099) | // CustomFunctions2
            (Manufacturer::Canon, 0x009A) | // AspectInfo
            (Manufacturer::Canon, 0x00A0) | // ProcessingInfo
            (Manufacturer::Canon, 0x00A9) | // ColorBalance
            (Manufacturer::Canon, 0x00AA) | // MeasuredColor
            (Manufacturer::Canon, 0x00E0) | // SensorInfo
            (Manufacturer::Canon, 0x4001) | // ColorData
            (Manufacturer::Canon, 0x4013) | // AFMicroAdj
            (Manufacturer::Nikon, 0x0011) | // PreviewIFD
            (Manufacturer::Nikon, 0x0088) | // AFInfo
            (Manufacturer::Nikon, 0x0091) | // ShotInfo
            (Manufacturer::Nikon, 0x0097) | // ColorBalance
            (Manufacturer::Nikon, 0x0098) | // LensData
            (Manufacturer::Nikon, 0x00A8) | // FlashInfo
            (Manufacturer::Nikon, 0x00B7) | // AFInfo2
            (Manufacturer::Minolta, 0x0001) | // CameraSettings
            (Manufacturer::Minolta, 0x0003) | // CameraSettings
            (Manufacturer::Apple, 0x0003) |  // RunTime
            (Manufacturer::Sony, 0x2000) |   // SonyIDC
            // Pentax: these are SubDirectory container tags decoded above
            (Manufacturer::Pentax, 0x0205) | // CameraSettings
            (Manufacturer::Pentax, 0x0206) | // AEInfo
            (Manufacturer::Pentax, 0x0207) | // LensInfo
            (Manufacturer::Pentax, 0x0208) | // FlashInfo
            (Manufacturer::Pentax, 0x0215) | // CameraInfo
            (Manufacturer::Pentax, 0x0216) | // BatteryInfo
            (Manufacturer::Pentax, 0x021F) | // AFInfo
            (Manufacturer::Pentax, 0x0222) | // ColorInfo
            (Manufacturer::Pentax, 0x003f) | // LensRec (SubDirectory)
            (Manufacturer::Pentax, 0x005C)   // SRInfo
        );
        if is_subdirectory { continue; }

        // Canon ImageUniqueID (0x0028): suppress if all-zero bytes (Perl RawConv)
        if manufacturer == Manufacturer::Canon && tag_id == 0x0028 {
            if value_data.iter().all(|&b| b == 0) { continue; }
        }

        // GE MakerNote: filter to known tags only
        if manufacturer == Manufacturer::GE {
            let known_ge = matches!(name, "Macro" | "GEModel" | "GEMake" | "Warning");
            if !known_ge { continue; }
        }

        // Ricoh WhiteBalanceFineTune: only valid when format is int16u (data_type == 3)
        if manufacturer == Manufacturer::Ricoh && tag_id == 0x1004 && data_type != 3 {
            continue;
        }

        // Pentax ColorTemperature (0x0050): suppress when val==0, apply ValueConv 53190-val
        if manufacturer == Manufacturer::Pentax && tag_id == 0x0050 {
            if let Some(v) = value.as_u64() {
                if v == 0 { continue; }
                // ValueConv: 53190 - val
                let converted = 53190i64 - v as i64;
                let pv = converted.to_string();
                tags.push(Tag {
                    id: TagId::Numeric(tag_id),
                    name: name.to_string(), description: description.to_string(),
                    group: TagGroup { family0: "MakerNotes".to_string(), family1: "Pentax".to_string(), family2: "Camera".to_string() },
                    raw_value: value, print_value: pv, priority: 0,
                });
                continue;
            } else {
                continue;
            }
        }

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

        // Track Pentax PreviewImage offset/length for post-loop synthesis
        if manufacturer == Manufacturer::Pentax {
            if tag_id == 0x0004 {
                if let Some(v) = value.as_u64() { pentax_preview_start = Some(v as usize); }
            } else if tag_id == 0x0003 {
                if let Some(v) = value.as_u64() { pentax_preview_length = Some(v as usize); }
            }
        }

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

    // Synthesize Pentax PreviewImage from PreviewImageStart + PreviewImageLength
    if manufacturer == Manufacturer::Pentax {
        if let (Some(_start), Some(len)) = (pentax_preview_start, pentax_preview_length) {
            if len > 0 {
                tags.push(Tag {
                    id: TagId::Text("PreviewImage".to_string()),
                    name: "PreviewImage".to_string(),
                    description: "Preview Image".to_string(),
                    group: TagGroup {
                        family0: "MakerNotes".to_string(),
                        family1: "Pentax".to_string(),
                        family2: "Image".to_string(),
                    },
                    raw_value: Value::Binary(Vec::new()),
                    print_value: format!("(Binary data {} bytes, use -b option to extract)", len),
                    priority: 0,
                });
            }
        }
    }

    // Synthesize Olympus PreviewImage from PreviewImageStart + PreviewImageLength
    // (from CameraSettings sub-IFD tags 0x0101 and 0x0102)
    if manufacturer == Manufacturer::Olympus || manufacturer == Manufacturer::OlympusNew {
        if !tags.iter().any(|t| t.name == "PreviewImage") {
            let preview_start = tags.iter()
                .find(|t| t.name == "PreviewImageStart")
                .and_then(|t| t.raw_value.as_u64())
                .map(|v| v as usize);
            let preview_len = tags.iter()
                .find(|t| t.name == "PreviewImageLength")
                .and_then(|t| t.raw_value.as_u64())
                .map(|v| v as usize);
            if let (Some(start), Some(len)) = (preview_start, preview_len) {
                if len > 0 && start > 0 && start + len <= data.len() {
                    tags.push(Tag {
                        id: TagId::Text("PreviewImage".to_string()),
                        name: "PreviewImage".to_string(),
                        description: "Preview Image".to_string(),
                        group: TagGroup {
                            family0: "MakerNotes".to_string(),
                            family1: "Olympus".to_string(),
                            family2: "Image".to_string(),
                        },
                        raw_value: Value::Binary(data[start..start + len].to_vec()),
                        print_value: format!("(Binary data {} bytes, use -b option to extract)", len),
                        priority: 0,
                    });
                }
            }
        }

        // Synthesize ExtenderStatus composite (Perl Olympus.pm ExtenderStatus sub)
        // Requires: Extender (ValueConv key), LensType (PrintConv string), MaxApertureValue
        // Since MaxApertureValue comes from EXIF (not available here), we compute what we can:
        // If Extender key's second token hex value is 0, status = 0 (Not attached)
        // If key is '0 04' (EC-14), we'd need MaxApertureValue to decide 1 or 2
        // For all other extenders (non-EC14), status = 1 (Attached)
        if !tags.iter().any(|t| t.name == "ExtenderStatus") {
            // Get the Extender raw bytes to compute ValueConv key
            let extender_pv = tags.iter().find(|t| t.name == "Extender").map(|t| t.print_value.clone());
            if let Some(ext_pv) = extender_pv {
                // Map print value back to status
                let (status_val, status_str) = if ext_pv == "None" {
                    (0u32, "Not attached")
                } else {
                    // Extender is attached (covers EC-14, EX-25, EC-20, etc.)
                    // For EC-14 ('0 04'), Perl checks MaxApertureValue vs lens max aperture.
                    // Without MaxApertureValue from EXIF here, conservatively say Attached.
                    (1u32, "Attached")
                };
                tags.push(Tag {
                    id: TagId::Text("ExtenderStatus".to_string()),
                    name: "ExtenderStatus".to_string(),
                    description: "Extender Status".to_string(),
                    group: TagGroup {
                        family0: "MakerNotes".to_string(),
                        family1: "Olympus".to_string(),
                        family2: "Camera".to_string(),
                    },
                    raw_value: Value::U32(status_val),
                    print_value: status_str.to_string(),
                    priority: 0,
                });
            }
        }
    }
}

pub fn decode_mn_value(data: &[u8], data_type: u16, count: usize, bo: ByteOrderMark) -> Value {
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
        Manufacturer::GE => "GE",
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
/// Perl: Canon::AFInfo2, FORMAT='int16u', ProcessSerialData
/// Sequential fields: [0]=AFInfoSize, [1]=AFAreaMode, [2]=NumAFPoints, [3]=ValidAFPoints,
/// [4]=CanonImageWidth, [5]=CanonImageHeight, [6]=AFImageWidth, [7]=AFImageHeight,
/// then variable-length arrays of size NumAFPoints: Widths, Heights, XPos, YPos,
/// then AFPointsInFocus (ceil(N/16) words), AFPointsSelected (EOS, ceil(N/16) words)
fn decode_canon_afinfo2(data: &[u8], count: usize, bo: ByteOrderMark) -> Vec<Tag> {
    let mut tags = Vec::new();
    let rd = |i: usize| -> u16 { read_u16(data, i * 2, bo) };
    let rdi = |i: usize| -> i16 { read_u16(data, i * 2, bo) as i16 };

    if count < 8 { return tags; }

    // seq 1: AFAreaMode
    let area_mode = rd(1);
    let area_mode_str = match area_mode {
        0 => "Off (Manual Focus)", 1 => "AF Point Expansion (surround)",
        2 => "Single-point AF", 4 => "Auto", 5 => "Face Detect AF",
        6 => "Face + Tracking", 7 => "Zone AF", 8 => "AF Point Expansion (4 point)",
        9 => "Spot AF", 10 => "AF Point Expansion (8 point)",
        11 => "Flexizone Multi (49 point)", 12 => "Flexizone Multi (9 point)",
        13 => "Flexizone Single", 14 => "Large Zone AF", _ => "",
    };
    let area_mode_pv = if area_mode_str.is_empty() { area_mode.to_string() } else { area_mode_str.to_string() };
    tags.push(mk_canon_str("AFAreaMode", &area_mode_pv));

    let num_af = rd(2) as usize;
    let valid_af = rd(3) as usize;
    let img_w = rd(4);
    let img_h = rd(5);
    let af_w = rd(6);
    let af_h = rd(7);

    tags.push(mk_canon("NumAFPoints", Value::U16(num_af as u16)));
    tags.push(mk_canon("ValidAFPoints", Value::U16(valid_af as u16)));
    tags.push(mk_canon("CanonImageWidth", Value::U16(img_w)));
    tags.push(mk_canon("CanonImageHeight", Value::U16(img_h)));
    tags.push(mk_canon("AFImageWidth", Value::U16(af_w)));
    tags.push(mk_canon("AFImageHeight", Value::U16(af_h)));

    // Variable-length arrays starting at seq 8
    let base = 8;
    if num_af > 0 && base + num_af * 4 <= count {
        // AFAreaWidths at base, AFAreaHeights at base+num_af, XPos at base+2*num_af, YPos at base+3*num_af
        let widths: Vec<String> = (0..num_af).map(|i| rdi(base + i).to_string()).collect();
        let heights: Vec<String> = (0..num_af).map(|i| rdi(base + num_af + i).to_string()).collect();
        let x_pos: Vec<String> = (0..num_af).map(|i| rdi(base + num_af * 2 + i).to_string()).collect();
        let y_pos: Vec<String> = (0..num_af).map(|i| rdi(base + num_af * 3 + i).to_string()).collect();

        if !widths.is_empty() {
            tags.push(mk_canon_str("AFAreaWidths", &widths.join(" ")));
            tags.push(mk_canon_str("AFAreaHeights", &heights.join(" ")));
            tags.push(mk_canon_str("AFAreaXPositions", &x_pos.join(" ")));
            tags.push(mk_canon_str("AFAreaYPositions", &y_pos.join(" ")));
        }

        // AFPointsInFocus: ceil(num_af/16) int16s words, decoded as bitmask
        let focus_words = (num_af + 15) / 16;
        let focus_base = base + num_af * 4;
        if focus_base + focus_words <= count {
            let mut focus_bits: u64 = 0;
            for w in 0..focus_words {
                let word = rd(focus_base + w) as u64;
                focus_bits |= word << (w * 16);
            }
            // Print as decimal bit index of set bits
            let mut set_bits: Vec<u32> = Vec::new();
            for b in 0..num_af { if focus_bits & (1u64 << b) != 0 { set_bits.push(b as u32); } }
            let pv = if set_bits.len() == 1 { set_bits[0].to_string() }
                     else { set_bits.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" ") };
            if !pv.is_empty() { tags.push(mk_canon_str("AFPointsInFocus", &pv)); }

            // AFPointsSelected: another ceil(num_af/16) words (EOS models)
            let sel_base = focus_base + focus_words;
            if sel_base + focus_words <= count {
                let mut sel_bits: u64 = 0;
                for w in 0..focus_words {
                    let word = rd(sel_base + w) as u64;
                    sel_bits |= word << (w * 16);
                }
                let mut sel_set: Vec<u32> = Vec::new();
                for b in 0..num_af { if sel_bits & (1u64 << b) != 0 { sel_set.push(b as u32); } }
                let spv = if sel_set.len() == 1 { sel_set[0].to_string() }
                          else { sel_set.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" ") };
                if !spv.is_empty() { tags.push(mk_canon_str("AFPointsSelected", &spv)); }
            }
        }
    }

    tags
}

/// Decode Canon ColorData (tag 0x4001).
/// Dispatches to the correct sub-table based on version/count.
/// ColorData4 (count=674/692/702/1227/1250/1251/1337/1338/1346): version 2-9, used by 1DmkIII, 40D, etc.
/// ColorData3 (count=796): version 1, used by 1DmkIIN, 5D, 30D, 400D.
fn decode_canon_color_data(data: &[u8], count: usize, bo: ByteOrderMark) -> Vec<Tag> {
    let mut tags = Vec::new();
    let rd = |i: usize| -> i16 {
        if i * 2 + 2 > data.len() { return 0; }
        read_u16(data, i * 2, bo) as i16
    };
    let rdu = |i: usize| -> u16 {
        if i * 2 + 2 > data.len() { return 0; }
        read_u16(data, i * 2, bo)
    };

    if count < 50 { return tags; }

    let version = rd(0);
    let version_str = match version {
        1 => "1 (1DmkIIN/5D/30D/400D)",
        2 => "2 (1DmkIII)",
        3 => "3 (40D)",
        4 => "4 (1DSmkIII)",
        5 => "5 (450D/1000D)",
        6 => "6 (50D/5DmkII)",
        7 => "7 (500D/550D/7D/1DmkIV)",
        9 => "9 (60D/1100D)",
        _ => "",
    };
    let ver_pv = if version_str.is_empty() { version.to_string() } else { version_str.to_string() };
    tags.push(mk_canon_str("ColorDataVersion", &ver_pv));

    // ColorData4: version 2-9 (count 674..1346) — uses ColorCoefs subdir at index 0x3f (63)
    // ColorData3: version 1 (count=796) — uses its own WB layout at index 0x3f (63)
    // Both have WB data starting at index 63 (0x3f), but with different WB block layouts.
    // ColorCoefs (for ColorData4): AsShot[0..3], CT[4], Auto[5..8], CT[9], Measured[10..13], CT[14],
    //   (skip Unknown[15..18], CT[19]), Daylight[20..23], CT[24], Shade[25..28], CT[29],
    //   Cloudy[30..33], CT[34], Tungsten[35..38], CT[39], Fluorescent[40..43], CT[44],
    //   Kelvin[45..48], CT[49], Flash[50..53], CT[54]
    //   (more unknown blocks follow...)
    //   Fluorescent[40..43] = index 63+40=103, Flash[50..53] = 63+50=113
    //   But 0x37 in ColorCoefs = Unknown2 (skip), 0x3c = Unknown3 (skip), etc.
    //   The known non-Unknown blocks end at Flash (0x36 = 54 within ColorCoefs).
    //   After many Unknown blocks: PC1 at 0x64 (100), PC2 at 0x69 (105), PC3 at 0x6e (110).
    //   ColorCoefs ends at 0x72 (114) = ColorTempUnknown13 → total 105 valid entries (undef[210])
    //
    // ColorData3 (count=796): WB at 0x3f (63) with its own layout (WB_AsShot, CT, ..., many unknowns)
    //   RawMeasuredRGGB at 0x26a (618) — int32u[4] with word swap

    if count >= 674 {
        // ColorData4 layout (1DmkIII, 40D, 50D, 5DmkII, etc.)
        // ColorCoefs subdir starts at index 63 (0x3f in int16s words)
        let cc = 63usize; // ColorCoefs base index

        // Helper to read 4-value RGGB block and format as string
        let wb4 = |base: usize| -> String {
            let r = rd(base) as u16;
            let g1 = rd(base+1) as u16;
            let g2 = rd(base+2) as u16;
            let b = rd(base+3) as u16;
            format!("{} {} {} {}", r, g1, g2, b)
        };
        let ct = |i: usize| -> u16 { rdu(i) };

        // AsShot: cc+0
        if cc + 4 < count { tags.push(mk_canon_str("WB_RGGBLevelsAsShot", &wb4(cc))); }
        if cc + 4 < count { let t = ct(cc+4); if t > 0 { tags.push(mk_canon_str("ColorTempAsShot", &t.to_string())); } }
        // Auto: cc+5
        if cc + 9 < count { tags.push(mk_canon_str("WB_RGGBLevelsAuto", &wb4(cc+5))); }
        if cc + 9 < count { let t = ct(cc+9); if t > 0 { tags.push(mk_canon_str("ColorTempAuto", &t.to_string())); } }
        // Measured: cc+10
        if cc + 14 < count { tags.push(mk_canon_str("WB_RGGBLevelsMeasured", &wb4(cc+10))); }
        if cc + 14 < count { let t = ct(cc+14); if t > 0 { tags.push(mk_canon_str("ColorTempMeasured", &t.to_string())); } }
        // (skip Unknown at cc+15..19)
        // Daylight: cc+20 (0x14 in ColorCoefs)
        if cc + 24 < count { tags.push(mk_canon_str("WB_RGGBLevelsDaylight", &wb4(cc+20))); }
        if cc + 24 < count { let t = ct(cc+24); if t > 0 { tags.push(mk_canon_str("ColorTempDaylight", &t.to_string())); } }
        // Shade: cc+25 (0x19)
        if cc + 29 < count { tags.push(mk_canon_str("WB_RGGBLevelsShade", &wb4(cc+25))); }
        if cc + 29 < count { let t = ct(cc+29); if t > 0 { tags.push(mk_canon_str("ColorTempShade", &t.to_string())); } }
        // Cloudy: cc+30 (0x1e)
        if cc + 34 < count { tags.push(mk_canon_str("WB_RGGBLevelsCloudy", &wb4(cc+30))); }
        if cc + 34 < count { let t = ct(cc+34); if t > 0 { tags.push(mk_canon_str("ColorTempCloudy", &t.to_string())); } }
        // Tungsten: cc+35 (0x23)
        if cc + 39 < count { tags.push(mk_canon_str("WB_RGGBLevelsTungsten", &wb4(cc+35))); }
        if cc + 39 < count { let t = ct(cc+39); if t > 0 { tags.push(mk_canon_str("ColorTempTungsten", &t.to_string())); } }
        // Fluorescent: cc+40 (0x28)
        if cc + 44 < count { tags.push(mk_canon_str("WB_RGGBLevelsFluorescent", &wb4(cc+40))); }
        if cc + 44 < count { let t = ct(cc+44); if t > 0 { tags.push(mk_canon_str("ColorTempFluorescent", &t.to_string())); } }
        // Kelvin: cc+45 (0x2d)
        if cc + 49 < count { tags.push(mk_canon_str("WB_RGGBLevelsKelvin", &wb4(cc+45))); }
        if cc + 49 < count { let t = ct(cc+49); if t > 0 { tags.push(mk_canon_str("ColorTempKelvin", &t.to_string())); } }
        // Flash: cc+50 (0x32)
        if cc + 54 < count { tags.push(mk_canon_str("WB_RGGBLevelsFlash", &wb4(cc+50))); }
        if cc + 54 < count { let t = ct(cc+54); if t > 0 { tags.push(mk_canon_str("ColorTempFlash", &t.to_string())); } }

        // WB_RGGBLevels alias (same as AsShot)
        if cc + 4 < count { tags.push(mk_canon_str("WB_RGGBLevels", &wb4(cc))); }

        // AverageBlackLevel at index 0xe7 (231) — int16u[4]
        let abl = 0xe7usize;
        if abl + 4 <= count {
            let v: Vec<String> = (0..4).map(|i| rdu(abl+i).to_string()).collect();
            tags.push(mk_canon_str("AverageBlackLevel", &v.join(" ")));
        }

        // FlashBatteryLevel at index 0x26c (620) — single int16s
        // PrintConv: '$val ? sprintf("%.2fV", $val * 5 / 186) : "n/a"'
        let fbl = 0x26cusize;
        if fbl < count {
            let v = rdu(fbl) as u16;
            let pv = if v > 0 { format!("{:.2}V", v as f64 * 5.0 / 186.0) } else { "n/a".to_string() };
            tags.push(mk_canon_str("FlashBatteryLevel", &pv));
        }

        // RawMeasuredRGGB at index 0x280 (640) — int32u[4] with word swap
        // Each value is stored as two adjacent int16u with big-endian word order
        // ValueConv: SwapWords (swap high/low 16-bit words of each 32-bit value)
        let rmb = 0x280usize;
        if rmb + 8 <= count {
            let mut vals = Vec::new();
            for i in 0..4 {
                let lo = rdu(rmb + i*2) as u32;
                let hi = rdu(rmb + i*2 + 1) as u32;
                vals.push((hi << 16) | lo);
            }
            tags.push(mk_canon_str("RawMeasuredRGGB", &vals.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" ")));
        }

    } else if count >= 580 {
        // ColorData3 layout (1DmkIIN, 5D, 30D, 400D — count=796, version=1)
        // WB data starts at 0x3f (63), layout similar to ColorData4's ColorCoefs
        // but with its own unknowns. RawMeasuredRGGB at 0x26a (618).
        let _cc = 63usize;
        let wb4 = |base: usize| -> String {
            let r = rd(base) as u16;
            let g1 = rd(base+1) as u16;
            let g2 = rd(base+2) as u16;
            let b = rd(base+3) as u16;
            format!("{} {} {} {}", r, g1, g2, b)
        };
        let ct = |i: usize| -> u16 { rdu(i) };

        // AsShot at 0x26 (38) relative to whole array (not ColorCoefs)
        // ColorData3 table (Perl Canon::ColorData3):
        // 0x26 = WB_RGGBLevelsAsShot, 0x2a = CT AsShot, 0x2b = CT (same?), ...
        // Use Perl table offsets directly:
        let base = 0x26usize; // WB_RGGBLevelsAsShot at index 0x26 in ColorData3
        if base + 4 < count { tags.push(mk_canon_str("WB_RGGBLevelsAsShot", &wb4(base))); }
        if base + 4 < count { let t = ct(base+4); if t > 0 { tags.push(mk_canon_str("ColorTempAsShot", &t.to_string())); } }
        // 0x2b = WB_RGGBLevelsDaylight
        let dl = 0x2busize;
        if dl + 4 < count { tags.push(mk_canon_str("WB_RGGBLevelsDaylight", &wb4(dl))); }
        if dl + 4 < count { let t = ct(dl+4); if t > 0 { tags.push(mk_canon_str("ColorTempDaylight", &t.to_string())); } }

        // WB_RGGBLevels alias
        if base + 4 < count { tags.push(mk_canon_str("WB_RGGBLevels", &wb4(base))); }

        // RawMeasuredRGGB at 0x26a (618) with word swap
        let rmb = 0x26ausize;
        if rmb + 8 <= count {
            let mut vals = Vec::new();
            for i in 0..4 {
                let lo = rdu(rmb + i*2) as u32;
                let hi = rdu(rmb + i*2 + 1) as u32;
                vals.push((hi << 16) | lo);
            }
            tags.push(mk_canon_str("RawMeasuredRGGB", &vals.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" ")));
        }
    } else {
        // Older ColorData (count < 580) — simple WB layout
        let wb_base = if count > 350 { 50 } else if count > 200 { 25 } else { 19 };
        let wb4 = |base: usize| -> String {
            let r = rd(base) as u16;
            let g1 = rd(base+1) as u16;
            let b = rd(base+2) as u16;
            let g2 = rd(base+3) as u16;
            format!("{} {} {} {}", r, g1, b, g2)
        };
        if wb_base + 4 <= count {
            tags.push(mk_canon_str("WB_RGGBLevelsAsShot", &wb4(wb_base)));
            tags.push(mk_canon_str("WB_RGGBLevels", &wb4(wb_base)));
            let temp = rdu(wb_base + 4);
            if temp > 0 { tags.push(mk_canon_str("ColorTempAsShot", &temp.to_string())); }
        }
    }

    tags
}

/// Convert Unix timestamp (seconds since 1970-01-01) to Exif datetime string "YYYY:MM:DD HH:MM:SS"
fn unix_time_to_datetime(secs: u32) -> String {
    let s = secs as i64;
    let sec = (s % 60) as u32;
    let min_total = s / 60;
    let min = (min_total % 60) as u32;
    let hour_total = min_total / 60;
    let hour = (hour_total % 24) as u32;
    let days = hour_total / 24;
    // Algorithm from https://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe/1460 + doe/36524 - doe/146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365*yoe + yoe/4 - yoe/100);
    let mp = (5*doy + 2) / 153;
    let d = doy - (153*mp + 2)/5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}", y, m, d, hour, min, sec)
}

/// Return Canon white balance name for int16s value (from Perl %canonWhiteBalance)
fn canon_wb_name(v: i16) -> &'static str {
    match v {
        0 => "Auto", 1 => "Daylight", 2 => "Cloudy", 3 => "Tungsten",
        4 => "Fluorescent", 5 => "Flash", 6 => "Custom", 8 => "Shade",
        9 => "Kelvin", 10 => "PC Set 1", 11 => "PC Set 2", 12 => "PC Set 3",
        14 => "Daylight Fluorescent", 15 => "Custom 1", 16 => "Custom 2",
        17 => "Underwater", _ => "",
    }
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

/// Decode Ricoh RicohSubdir (tag 0x2001): IFD or text header "[Ricoh Camera Info]"
/// followed by a sub-IFD containing ManufactureDate1 (0x0004) and ManufactureDate2 (0x0005).
fn decode_ricoh_subdir(data: &[u8], full_data: &[u8], _parent_bo: ByteOrderMark) -> Vec<Tag> {
    let mut tags = Vec::new();
    // Data may start with "[Ricoh Camera Info]\0" (20 bytes), then a Big-Endian IFD
    let ifd_start = if data.len() > 20 && data.starts_with(b"[Ricoh Camera Info]") {
        20
    } else {
        0
    };
    let ifd_data = &data[ifd_start..];
    let bo = ByteOrderMark::BigEndian; // Ricoh subdirs use Big-Endian
    if ifd_data.len() < 2 { return tags; }
    let entry_count = read_u16(ifd_data, 0, bo) as usize;
    if entry_count > 100 { return tags; }
    for i in 0..entry_count {
        let eoff = 2 + i * 12;
        if eoff + 12 > ifd_data.len() { break; }
        let tag_id = read_u16(ifd_data, eoff, bo);
        let data_type = read_u16(ifd_data, eoff + 2, bo);
        let count = read_u32(ifd_data, eoff + 4, bo) as usize;
        let type_size: usize = match data_type { 1|2|6|7 => 1, 3|8 => 2, 4|9|11|13 => 4, 5|10|12 => 8, _ => 1 };
        let total_size = count * type_size;
        let value_data = if total_size <= 4 {
            &ifd_data[eoff + 8..eoff + 8 + total_size.min(4)]
        } else {
            let raw_offset = read_u32(ifd_data, eoff + 8, bo) as usize;
            // Try offset relative to ifd_data first, then to data, then to full_data (absolute TIFF)
            if raw_offset + total_size <= ifd_data.len() {
                &ifd_data[raw_offset..raw_offset + total_size]
            } else if raw_offset + total_size <= data.len() {
                &data[raw_offset..raw_offset + total_size]
            } else if raw_offset + total_size <= full_data.len() {
                &full_data[raw_offset..raw_offset + total_size]
            } else {
                continue;
            }
        };

        let name = match tag_id {
            0x0004 => "ManufactureDate1",
            0x0005 => "ManufactureDate2",
            _ => continue,
        };

        let val = if data_type == 2 {
            // ASCII string
            String::from_utf8_lossy(value_data).trim_end_matches('\0').to_string()
        } else {
            continue;
        };

        {
            tags.push(Tag {
                id: TagId::Numeric(tag_id),
                name: name.to_string(),
                description: name.to_string(),
                group: TagGroup {
                    family0: "MakerNotes".into(),
                    family1: "Ricoh".into(),
                    family2: "Time".into(),
                },
                raw_value: Value::String(val.clone()),
                print_value: val,
                priority: 0,
            });
        }
    }
    tags
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
