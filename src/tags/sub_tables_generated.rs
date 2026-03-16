//! MakerNotes sub-table dispatch system.
//!
//! Properly dispatches to model-specific binary structure decoders
//! based on the same conditions as Perl ExifTool:
//! - Camera Model string
//! - Version prefix (first 4 bytes of binary data)
//! - Data byte count
//! - First byte value (Sony encrypted tags)
//!
//! Architecture mirrors ExifTool's Condition-based dispatch exactly.

use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

/// Context for sub-table dispatch decisions.
pub struct DispatchContext<'a> {
    pub model: &'a str,
    pub data: &'a [u8],
    pub count: usize,
    pub byte_order_le: bool,
}

impl<'a> DispatchContext<'a> {
    pub fn version_prefix(&self) -> &str {
        if self.data.len() >= 4 {
            std::str::from_utf8(&self.data[..4]).unwrap_or("")
        } else { "" }
    }
    pub fn first_byte(&self) -> u8 {
        self.data.first().copied().unwrap_or(0)
    }
}

// ============================================================================
// Canon: CameraInfo (0x000D) — 36 variants by Model regex
// ============================================================================

pub fn dispatch_canon_camera_info(ctx: &DispatchContext) -> Vec<Tag> {
    let m = ctx.model;
    // Exact order from Canon.pm lines 1307-1494
    let variant = if m.contains("1DS") || (m.contains("1D") && !m.contains("Mark") && !m.contains("1DX")) {
        "CameraInfo1D"
    } else if m.contains("1D") && m.contains("Mark II") && !m.contains("Mark III") && !m.contains("Mark IV") && !m.contains("II N") {
        "CameraInfo1DmkII"
    } else if m.contains("1D") && m.contains("Mark II N") {
        "CameraInfo1DmkIIN"
    } else if m.contains("1D") && m.contains("Mark III") {
        "CameraInfo1DmkIII"
    } else if m.contains("1D Mark IV") {
        "CameraInfo1DmkIV"
    } else if m.contains("1D X") {
        "CameraInfo1DX"
    } else if m.contains("5D") && !m.contains("Mark") {
        "CameraInfo5D"
    } else if m.contains("5D Mark II") && !m.contains("Mark III") {
        "CameraInfo5DmkII"
    } else if m.contains("5D Mark III") {
        "CameraInfo5DmkIII"
    } else if m.contains("6D") && !m.contains("Mark") {
        "CameraInfo6D"
    } else if m.contains("7D") && !m.contains("Mark") {
        "CameraInfo7D"
    } else if m.contains("40D") {
        "CameraInfo40D"
    } else if m.contains("50D") {
        "CameraInfo50D"
    } else if m.contains("60D") {
        "CameraInfo60D"
    } else if m.contains("70D") {
        "CameraInfo70D"
    } else if m.contains("80D") {
        "CameraInfo80D"
    } else if m.contains("450D") || m.contains("REBEL XSi") || m.contains("Kiss X2") {
        "CameraInfo450D"
    } else if m.contains("500D") || m.contains("REBEL T1i") || m.contains("Kiss X3") {
        "CameraInfo500D"
    } else if m.contains("550D") || m.contains("REBEL T2i") || m.contains("Kiss X4") {
        "CameraInfo550D"
    } else if m.contains("600D") || m.contains("REBEL T3i") || m.contains("Kiss X5") {
        "CameraInfo600D"
    } else if m.contains("650D") || m.contains("REBEL T4i") || m.contains("Kiss X6i") {
        "CameraInfo650D"
    } else if m.contains("700D") || m.contains("REBEL T5i") || m.contains("Kiss X7i") {
        "CameraInfo650D" // reuses 650D
    } else if m.contains("750D") || m.contains("Rebel T6i") || m.contains("Kiss X8i") {
        "CameraInfo750D"
    } else if m.contains("1000D") || m.contains("REBEL XS") || m.contains("Kiss F") {
        "CameraInfo1000D"
    } else if m.contains("1100D") || m.contains("REBEL T3") || m.contains("Kiss X50") {
        "CameraInfo600D" // reuses 600D
    } else if m.contains("EOS R5") || m.contains("EOS R6") {
        "CameraInfoR6"
    } else {
        return Vec::new();
    };
    vec![mk("Canon", "CameraInfoVariant", variant)]
}

// ============================================================================
// Nikon: ShotInfo (0x0091) — 30 variants by version prefix + count
// ============================================================================

pub fn dispatch_nikon_shot_info(ctx: &DispatchContext) -> Vec<Tag> {
    let ver = ctx.version_prefix();
    let c = ctx.count;

    let variant = match ver {
        "0208" => "ShotInfoD80",
        "0209" => "ShotInfoD40",
        "0213" => "ShotInfoD90",
        "0220" => "ShotInfoD7000",
        "0223" => "ShotInfoD4",
        "0231" => "ShotInfoD4S",
        "0222" => "ShotInfoD800",
        "0233" => "ShotInfoD810",
        "0243" => "ShotInfoD850",
        "0232" => "ShotInfoD610",
        "0246" => "ShotInfoD6",
        "0242" => "ShotInfoD7500",
        "0245" => "ShotInfoD780",
        // Ambiguous versions: must check count
        "0210" => match c {
            5399 => "ShotInfoD3a",
            5408 | 5412 => "ShotInfoD3b",
            5291 => "ShotInfoD300a",
            5303 => "ShotInfoD300b",
            _ => return Vec::new(),
        },
        "0214" if c == 5409 => "ShotInfoD3X",
        "0218" if c == 5356 || c == 5388 => "ShotInfoD3S",
        "0216" if c == 5311 => "ShotInfoD300S",
        "0212" if c == 5312 => "ShotInfoD700",
        "0215" if c == 6745 => "ShotInfoD5000",
        "0221" if c == 8902 => "ShotInfoD5100",
        "0226" if c == 11587 => "ShotInfoD5200",
        "0805" => "ShotInfoZ9",
        "0806" => "ShotInfoZ8",
        v if v.starts_with("080") => "ShotInfoZ7II",
        v if v.starts_with("081") => "ShotInfoZ6III",
        _ => return Vec::new(),
    };

    let tags = vec![
        mk("Nikon", "ShotInfoVersion", ver),
        mk("Nikon", "ShotInfoVariant", variant),
    ];

    // Nikon ShotInfo is encrypted (DecryptStart=4) — version prefix readable
    // Decryption requires SerialNumber + ShutterCount, not available here

    tags
}

// ============================================================================
// Nikon: LensData (0x0098) — 8 variants by version prefix
// ============================================================================

pub fn dispatch_nikon_lens_data(ctx: &DispatchContext) -> Vec<Tag> {
    let ver = ctx.version_prefix();
    let d = ctx.data;

    let (_variant, encrypted) = match ver {
        "0100" => ("LensData0100", false),
        "0101" => ("LensData0101", false),
        v if v.starts_with("020") => ("LensData0201", true),
        "0204" => ("LensData0204", true),
        v if v.starts_with("040") => ("LensData0400", true),
        "0402" => ("LensData0402", true),
        "0403" => ("LensData0403", true),
        v if v.starts_with("080") => ("LensData0800", true),
        _ => return Vec::new(),
    };

    let mut tags = vec![mk("Nikon", "LensDataVersion", ver)];

    // Unencrypted versions: extract full lens info
    if !encrypted && d.len() >= 13 {
        if d[4] > 0 { tags.push(mk("Nikon", "ExitPupilPosition", &format!("{}", d[4]))); }
        if d[5] > 0 {
            let ap = 2.0_f64.powf(d[5] as f64 / 24.0);
            tags.push(mk("Nikon", "AFAperture", &format!("{:.1}", ap)));
        }
        if d[6] > 0 {
            let fl = 5.0 * 2.0_f64.powf(d[6] as f64 / 24.0);
            tags.push(mk("Nikon", "MinFocalLength", &format!("{:.1}", fl)));
        }
        if d[7] > 0 {
            let fl = 5.0 * 2.0_f64.powf(d[7] as f64 / 24.0);
            tags.push(mk("Nikon", "MaxFocalLength", &format!("{:.1}", fl)));
        }
        if d[8] > 0 { tags.push(mk("Nikon", "FocusPosition", &format!("0x{:02X}", d[8]))); }
        if d[9] > 0 {
            let dist = 0.01 * 10.0_f64.powf(d[9] as f64 / 40.0);
            tags.push(mk("Nikon", "FocusDistance", &format!("{:.2} m", dist)));
        }
        if d[10] > 0 {
            let ap = 2.0_f64.powf(d[10] as f64 / 24.0);
            tags.push(mk("Nikon", "MaxApertureAtMaxFocal", &format!("{:.1}", ap)));
        }
        if d.len() > 11 && d[11] > 0 {
            let ap = 2.0_f64.powf(d[11] as f64 / 24.0);
            tags.push(mk("Nikon", "MaxApertureAtMinFocal", &format!("{:.1}", ap)));
        }
        if d.len() > 12 && d[12] > 0 {
            let ap = 2.0_f64.powf(d[12] as f64 / 24.0);
            tags.push(mk("Nikon", "EffectiveMaxAperture", &format!("{:.1}", ap)));
        }
        if d.len() > 13 { tags.push(mkn("Nikon", "MCUVersion", d[13] as i32)); }
        // LensIDNumber
        if d.len() >= 8 {
            tags.push(mk("Nikon", "LensIDNumber", &format!("{}", d[4])));
        }
    }

    tags
}

// ============================================================================
// Nikon: AFInfo2 (0x00B7) — 5 variants by version, NOT encrypted
// ============================================================================

pub fn dispatch_nikon_af_info2(ctx: &DispatchContext) -> Vec<Tag> {
    let ver = ctx.version_prefix();
    let d = ctx.data;
    let mut tags = vec![mk("Nikon", "AFInfo2Version", ver)];

    if d.len() >= 8 {
        tags.push(mk("Nikon", "ContrastDetectAF", if d[4] == 0 { "Off" } else { "On" }));
        let af_area = match d[5] {
            0 => "Single Area", 1 => "Dynamic Area", 2 => "Dynamic Area (closest)",
            3 => "Group Dynamic", 4 => "Dynamic Area (9 points)",
            5 => "Dynamic Area (21 points)", 6 => "Dynamic Area (51 points)",
            8 => "Auto-area", 10 => "Dynamic Area (pinpoint)",
            12 => "Wide (S)", 14 => "Wide (L)", _ => "",
        };
        if !af_area.is_empty() { tags.push(mk("Nikon", "AFAreaMode", af_area)); }
        let phase = match d[6] {
            0 => "Off", 1 => "On (51-point)", 2 => "On (11-point)",
            3 => "On (39-point)", 4 => "On (73-point)", 5 => "On (5-point)",
            6 => "On (105-point)", 7 => "On (153-point)", _ => "On",
        };
        tags.push(mk("Nikon", "PhaseDetectAF", phase));
    }

    tags
}

// ============================================================================
// Sony: CameraSettings (0x0114) — 4 variants by byte count
// ============================================================================

pub fn dispatch_sony_camera_settings(ctx: &DispatchContext) -> Vec<Tag> {
    let variant = match ctx.count {
        280 | 364 => "CameraSettings",     // A200/A300/A350/A700/A850/A900
        332 => "CameraSettings2",          // A230/A290/A330/A380/A390
        1536 | 2048 => "CameraSettings3",  // NEX/A5xx/A33/A55 (LittleEndian)
        _ => return Vec::new(),
    };
    vec![mk("Sony", "CameraSettingsVariant", variant)]
}

// ============================================================================
// Sony: Tag2010 — 9 variants by model regex
// ============================================================================

pub fn dispatch_sony_tag2010(ctx: &DispatchContext) -> Vec<Tag> {
    let m = ctx.model;
    let variant = if m == "NEX-5N" { "Tag2010a" }
    else if m.starts_with("SLT-A65") || m.starts_with("SLT-A77") || m.starts_with("NEX-7")
         || m.starts_with("NEX-VG20") || m == "Lunar" { "Tag2010b" }
    else if m.starts_with("SLT-A37") || m.starts_with("SLT-A57") || m == "NEX-F3" { "Tag2010c" }
    else if m.starts_with("DSC-HX") || m.starts_with("DSC-TX") || m.starts_with("DSC-WX") { "Tag2010d" }
    else if m.starts_with("SLT-A99") || m == "HV" || m.starts_with("SLT-A58")
         || m.starts_with("ILCE-3") || m.starts_with("NEX-") || m.starts_with("DSC-RX1")
         || m == "DSC-RX100" || m == "Stellar" { "Tag2010e" }
    else if m == "DSC-RX100M2" || m.starts_with("DSC-QX1") { "Tag2010f" }
    else if m.starts_with("ILCE-7") || m.starts_with("ILCE-5") || m.starts_with("ILCE-6000")
         || m.starts_with("ILCA-") || m.starts_with("DSC-RX10") || m.starts_with("DSC-RX100M3") { "Tag2010g" }
    else if m.starts_with("ILCE-63") || m.starts_with("ILCE-65") || m.starts_with("ILCE-7RM2")
         || m.starts_with("ILCE-7SM2") || m.starts_with("ILCA-99M2") { "Tag2010h" }
    else if m.starts_with("ILCE-") || m.starts_with("ZV-") { "Tag2010i" }
    else { return Vec::new(); };

    // All Tag2010 variants are encrypted
    vec![mk("Sony", "EncryptedVariant", variant)]
}

// ============================================================================
// Sony: Tag9400 — variants by first byte
// ============================================================================

pub fn dispatch_sony_tag9400(ctx: &DispatchContext) -> Vec<Tag> {
    let variant = match ctx.first_byte() {
        0x07 | 0x09 | 0x0a => "Tag9400a",
        0x0c => "Tag9400b",
        0x23 | 0x24 | 0x26 | 0x28 | 0x31 | 0x32 | 0x33 | 0x41 => "Tag9400c",
        _ => return Vec::new(),
    };
    vec![mk("Sony", "EncryptedVariant", variant)]
}

// ============================================================================
// Helpers
// ============================================================================

fn mk(module: &str, name: &str, value: &str) -> Tag {
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup { family0: "MakerNotes".into(), family1: module.into(), family2: "Camera".into() },
        raw_value: Value::String(value.to_string()),
        print_value: value.to_string(),
        priority: 0,
    }
}

fn mkn(module: &str, name: &str, value: i32) -> Tag {
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup { family0: "MakerNotes".into(), family1: module.into(), family2: "Camera".into() },
        raw_value: Value::I32(value),
        print_value: value.to_string(),
        priority: 0,
    }
}
