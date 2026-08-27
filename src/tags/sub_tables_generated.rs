//! MakerNotes sub-table dispatch.
//!
//! **Hand-written, despite the file name.** `scripts/gen_sub_tables.pl` claims
//! this path in its usage line but has never produced it: its output is a
//! `decode_sub_table` entry point that appears nowhere in the tree. The name
//! said "generated" for long enough that nobody looked inside, which is how a
//! dispatcher here came to answer with the name of a layout instead of decoding
//! one -- see the test at the bottom of this file.
//!
//! Selection conditions belong in `variant_selectors_generated`, which does come
//! from ExifTool. What is left here is the decoding around them.
//!
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
    /// The TIFF format name ExifTool would use, e.g. `int32u`.
    ///
    /// Several conditions test it -- Olympus decides three of its sub-tables on
    /// `$format ne "ifd" and $format ne "int32u"` alone -- so a selector that
    /// cannot see it has to refuse those tags entirely.
    pub format: &'static str,
}

/// ExifTool's name for a TIFF data type, as its conditions spell it.
#[must_use]
pub fn tiff_format_name(data_type: u16) -> &'static str {
    match data_type {
        1 => "int8u",
        2 => "string",
        3 => "int16u",
        4 => "int32u",
        5 => "rational64u",
        6 => "int8s",
        7 => "undef",
        8 => "int16s",
        9 => "int32s",
        10 => "rational64s",
        11 => "float",
        12 => "double",
        13 => "ifd",
        _ => "unknown",
    }
}

impl<'a> DispatchContext<'a> {
    pub fn version_prefix(&self) -> &str {
        if self.data.len() >= 4 {
            std::str::from_utf8(&self.data[..4]).unwrap_or("")
        } else {
            ""
        }
    }
    pub fn first_byte(&self) -> u8 {
        self.data.first().copied().unwrap_or(0)
    }
}

// ============================================================================
// Canon: CameraInfo (0x000D) — 36 variants by Model regex
// ============================================================================

#[allow(clippy::if_same_then_else)]

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

    // ShotInfoVersion is a real Nikon tag. The variant is not: it names the
    // layout we would decode with, which is our business and not a value.
    let _ = variant;
    let tags = vec![mk("Nikon", "ShotInfoVersion", ver)];

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
        if d[4] > 0 {
            tags.push(mk("Nikon", "ExitPupilPosition", &format!("{}", d[4])));
        }
        if d[5] > 0 {
            let ap = 2.0_f64.powf(d[5] as f64 / 24.0);
            tags.push(mk("Nikon", "AFAperture", &format!("{:.1}", ap)));
        }
        // Offsets from Perl Nikon.pm LensData01 table (version 0101+):
        // 0x04=ExitPupilPosition, 0x05=AFAperture,
        // 0x08=FocusPosition, 0x09=FocusDistance,
        // 0x0A=MCUVersion, 0x0B=LensIDNumber,
        // 0x0C=LensFStops, 0x0D=MinFocalLength, 0x0E=MaxFocalLength,
        // 0x0F=MaxApertureAtMinFocal, 0x10=MaxApertureAtMaxFocal,
        // 0x11=EffectiveMaxAperture
        if d[4] > 0 {
            let ep = if d[4] > 0 { 2048.0 / d[4] as f64 } else { 0.0 };
            tags.push(mk("Nikon", "ExitPupilPosition", &format!("{:.1} mm", ep)));
        }
        if d[5] > 0 {
            let ap = 2.0_f64.powf(d[5] as f64 / 24.0);
            tags.push(mk("Nikon", "AFAperture", &format!("{:.1}", ap)));
        }
        if d.len() > 0x08 {
            tags.push(mk("Nikon", "FocusPosition", &format!("0x{:02x}", d[0x08])));
        }
        if d.len() > 0x09 && d[0x09] > 0 {
            let dist = 0.01 * 10.0_f64.powf(d[0x09] as f64 / 40.0);
            tags.push(mk("Nikon", "FocusDistance", &format!("{:.2} m", dist)));
        }
        // Nikon.pm:5545-5549 — LensData01 `0x0a => { Name => 'FocalLength',
        // Priority => 0, %nikonFocalConversions }`: ValueConv `5 * 2**($val/24)`,
        // PrintConv `sprintf("%.1f mm",$val)`. Version 0100 goes to LensData00
        // instead (Nikon.pm:2820-2828), where 0x0a is MaxApertureAtMinFocal.
        if ver == "0101" && d.len() > 0x0A {
            let fl = 5.0 * 2.0_f64.powf(d[0x0A] as f64 / 24.0);
            let mut t = mk("Nikon", "FocalLength", &format!("{:.1} mm", fl));
            t.raw_value = Value::F64(fl);
            t.priority = crate::tag::PRIORITY_EXPLICIT_ZERO;
            tags.push(t);
        }
        if d.len() > 0x0B {
            tags.push(mk("Nikon", "LensIDNumber", &format!("{}", d[0x0B])));
        }
        // Nikon.pm:5554-5560 — LensData01 `0x0c => { Name => 'LensFStops',
        // ValueConv => '$val / 12', PrintConv => 'sprintf("%.2f", $val)' }`.
        if ver == "0101" && d.len() > 0x0C {
            let fs = d[0x0C] as f64 / 12.0;
            let mut t = mk("Nikon", "LensFStops", &format!("{:.2}", fs));
            t.raw_value = Value::F64(fs);
            tags.push(t);
        }
        // MCUVersion at 0x11, EffectiveMaxAperture at 0x12 (Perl LensData01 offsets).
        if d.len() > 0x11 {
            tags.push(mkn("Nikon", "MCUVersion", d[0x11] as i32));
        }
        if d.len() > 0x0D && d[0x0D] > 0 {
            let fl = 5.0 * 2.0_f64.powf(d[0x0D] as f64 / 24.0);
            tags.push(mk("Nikon", "MinFocalLength", &format!("{:.1} mm", fl)));
        }
        if d.len() > 0x0E && d[0x0E] > 0 {
            let fl = 5.0 * 2.0_f64.powf(d[0x0E] as f64 / 24.0);
            tags.push(mk("Nikon", "MaxFocalLength", &format!("{:.1} mm", fl)));
        }
        if d.len() > 0x0F && d[0x0F] > 0 {
            let ap = 2.0_f64.powf(d[0x0F] as f64 / 24.0);
            tags.push(mk("Nikon", "MaxApertureAtMinFocal", &format!("{:.1}", ap)));
        }
        if d.len() > 0x10 && d[0x10] > 0 {
            let ap = 2.0_f64.powf(d[0x10] as f64 / 24.0);
            tags.push(mk("Nikon", "MaxApertureAtMaxFocal", &format!("{:.1}", ap)));
        }
        if d.len() > 0x12 && d[0x12] > 0 {
            let ap = 2.0_f64.powf(d[0x12] as f64 / 24.0);
            tags.push(mk("Nikon", "EffectiveMaxAperture", &format!("{:.1}", ap)));
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
        tags.push(mk(
            "Nikon",
            "ContrastDetectAF",
            if d[4] == 0 { "Off" } else { "On" },
        ));
        let af_area = match d[5] {
            0 => "Single Area",
            1 => "Dynamic Area",
            2 => "Dynamic Area (closest)",
            3 => "Group Dynamic",
            4 => "Dynamic Area (9 points)",
            5 => "Dynamic Area (21 points)",
            6 => "Dynamic Area (51 points)",
            8 => "Auto-area",
            10 => "Dynamic Area (pinpoint)",
            12 => "Wide (S)",
            14 => "Wide (L)",
            _ => "",
        };
        if !af_area.is_empty() {
            tags.push(mk("Nikon", "AFAreaMode", af_area));
        }
        let phase = match d[6] {
            0 => "Off",
            1 => "On (51-point)",
            2 => "On (11-point)",
            3 => "On (39-point)",
            4 => "On (73-point)",
            5 => "On (5-point)",
            6 => "On (105-point)",
            7 => "On (153-point)",
            _ => "On",
        };
        tags.push(mk("Nikon", "PhaseDetectAF", phase));
    }

    tags
}

// ============================================================================
// Sony: CameraSettings (0x0114) — 4 variants by byte count
// ============================================================================

pub fn dispatch_sony_camera_settings(ctx: &DispatchContext) -> Vec<Tag> {
    // Same as the Canon case above: the variant is known, the layouts are not
    // ported, and naming the variant is not a value anyone can use.
    let _ = crate::tags::variant_selectors_generated::variant_for("Sony", 0x0114, ctx.model, ctx.data, ctx.count, ctx.format);
    Vec::new()
}

// ============================================================================
// Sony: Tag2010 — 9 variants by model regex
// ============================================================================

pub fn dispatch_sony_tag2010(ctx: &DispatchContext) -> Vec<Tag> {
    let m = ctx.model;
    let variant = if m == "NEX-5N" {
        "Tag2010a"
    } else if m.starts_with("SLT-A65")
        || m.starts_with("SLT-A77")
        || m.starts_with("NEX-7")
        || m.starts_with("NEX-VG20")
        || m == "Lunar"
    {
        "Tag2010b"
    } else if m.starts_with("SLT-A37") || m.starts_with("SLT-A57") || m == "NEX-F3" {
        "Tag2010c"
    } else if m.starts_with("DSC-HX") || m.starts_with("DSC-TX") || m.starts_with("DSC-WX") {
        "Tag2010d"
    } else if m.starts_with("SLT-A99")
        || m == "HV"
        || m.starts_with("SLT-A58")
        || m.starts_with("ILCE-3")
        || m.starts_with("NEX-")
        || m.starts_with("DSC-RX1")
        || m == "DSC-RX100"
        || m == "Stellar"
    {
        "Tag2010e"
    } else if m == "DSC-RX100M2" || m.starts_with("DSC-QX1") {
        "Tag2010f"
    } else if m.starts_with("ILCE-7")
        || m.starts_with("ILCE-5")
        || m.starts_with("ILCE-6000")
        || m.starts_with("ILCA-")
        || m.starts_with("DSC-RX10")
        || m.starts_with("DSC-RX100M3")
    {
        "Tag2010g"
    } else if m.starts_with("ILCE-63")
        || m.starts_with("ILCE-65")
        || m.starts_with("ILCE-7RM2")
        || m.starts_with("ILCE-7SM2")
        || m.starts_with("ILCA-99M2")
    {
        "Tag2010h"
    } else if m.starts_with("ILCE-") || m.starts_with("ZV-") {
        "Tag2010i"
    } else {
        return Vec::new();
    };

    decode_ciphered(variant, ctx)
}

/// Decode a Sony ciphered block whose variant the generated selector resolves.
#[must_use]
pub fn decode_sony_ciphered(tag: u16, ctx: &DispatchContext) -> Vec<Tag> {
    // The conditions that pick a variant are tested against the block as it
    // sits in the file, before any deciphering -- that is where ExifTool tests
    // them, and half of them look at the first byte.
    match crate::tags::sony_ciphered_generated::variant_for(tag, ctx.model, ctx.data, false, false) {
        Some(variant) => decode_ciphered(variant.table, ctx),
        None => Vec::new(),
    }
}

/// Decipher a Sony sub-table and decode it with the generated tables.
///
/// The block is byte-substitution enciphered; the decoders address it by byte
/// offset. Both halves come from ExifTool's Sony.pm, so neither can drift.
fn decode_ciphered(variant: &str, ctx: &DispatchContext) -> Vec<Tag> {
    let mut buf = ctx.data.to_vec();
    // Only the enciphered tables are substituted; ShotInfo and its like sit in
    // plain sight, and deciphering one of those would turn it to noise.
    if crate::tags::sony_ciphered_generated::is_enciphered(variant) {
        crate::metadata::sony_decrypt::sony_decipher(&mut buf);
    }
    crate::tags::sony_ciphered_generated::decode(variant, &buf, ctx.model)
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
    decode_ciphered(variant, ctx)
}

// ============================================================================
// Helpers
// ============================================================================

fn mk(module: &str, name: &str, value: &str) -> Tag {
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: "MakerNotes".into(),
            family1: module.into(),
            family2: "Camera".into(),
            family3: "Main".into(),
        },
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
        group: TagGroup {
            family0: "MakerNotes".into(),
            family1: module.into(),
            family2: "Camera".into(),
            family3: "Main".into(),
        },
        raw_value: Value::I32(value),
        print_value: value.to_string(),
        priority: 0,
    }
}

#[cfg(test)]
mod tests {
    /// No dispatcher may report the name of a layout as if it were a value.
    ///
    /// Three of them used to: a body whose layout we cannot read produced a tag
    /// like `EncryptedVariant: Tag2010i`, which ExifTool has no equivalent for.
    /// It made a gap look filled, and it took a bug report to notice.
    #[test]
    fn no_dispatcher_invents_a_variant_tag() {
        let src = include_str!("sub_tables_generated.rs");
        for (n, line) in src.lines().enumerate() {
            assert!(
                // Built at runtime so this very line does not match itself.
                !line.contains(&format!("{}\", {}", "Variant", "variant")),
                "line {}: a dispatcher is reporting a layout name as a tag value: {}",
                n + 1,
                line.trim()
            );
        }
    }
}
