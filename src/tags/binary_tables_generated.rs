//! Auto-generated decoders for ExifTool's binary sub-tables.
//!
//! Do not edit: regenerate with
//! `perl scripts/gen_binary_tables.pl ../exiftool/lib > src/tags/binary_tables_generated.rs`.
//!
//! 52 tables, 1460 fields. A binary sub-table is a block of
//! bytes addressed by index: ExifTool's ProcessBinaryData reads the entry at
//! `(index - FIRST_ENTRY) * sizeof(FORMAT)`, and a field's own Format says
//! what to read there. What the generator could not express is on its stderr.
//! Generated code: the shape of a table decides what is written, so a helper
//! no table happens to need and a cast that happens to be a no-op are both
//! ordinary here rather than something to tidy away by hand.
#![allow(dead_code, unused_parens, unused_mut)]
#![allow(
    clippy::too_many_lines,
    clippy::match_same_arms,
    clippy::unreadable_literal,
    clippy::unnecessary_cast,
    clippy::identity_op,
    clippy::cast_lossless
)]

use std::sync::LazyLock;

use regex_lite::Regex;

use crate::tags::conv_expr::{self, Val as Conv};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

/// Which end the file puts first.
pub type ByteOrder = crate::metadata::exif::ByteOrderMark;

/// What the fields of this block have read so far, by the name ExifTool
/// stores them under.
pub type State = Vec<(String, f64)>;

fn dm_get(dm: &State, name: &str) -> Option<f64> {
    dm.iter().rev().find(|(n, _)| n == name).map(|(_, v)| *v)
}

/// What a conversion of this block can ask the file about.
///
/// ExifTool keeps these on the object: `$$self{Model}` tells one encoding of
/// TargetExposureTime from another, and `$$self{FILE_TYPE} eq "CRW"` decides
/// whether an ExposureTime of zero means one second or nothing at all.
struct Ctx<'a> {
    model: &'a str,
    file_type: &'a str,
    dm: &'a State,
}

impl conv_expr::ParseState for Ctx<'_> {
    fn member(&self, name: &str) -> Option<Conv> {
        match name {
            "Model" => Some(Conv::Str(self.model.to_string())),
            "FILE_TYPE" | "FileType" => Some(Conv::Str(self.file_type.to_string())),
            _ => dm_get(self.dm, name).map(Conv::Num),
        }
    }
}

fn u8_at(d: &[u8], o: usize) -> Option<u8> { d.get(o).copied() }
fn i8_at(d: &[u8], o: usize) -> Option<i8> { d.get(o).map(|b| *b as i8) }

fn u16_at(d: &[u8], o: usize, bo: ByteOrder) -> Option<u16> {
    let b = [*d.get(o)?, *d.get(o + 1)?];
    Some(if bo == ByteOrder::BigEndian { u16::from_be_bytes(b) } else { u16::from_le_bytes(b) })
}
fn i16_at(d: &[u8], o: usize, bo: ByteOrder) -> Option<i16> { u16_at(d, o, bo).map(|v| v as i16) }

fn u32_at(d: &[u8], o: usize, bo: ByteOrder) -> Option<u32> {
    let b = [*d.get(o)?, *d.get(o + 1)?, *d.get(o + 2)?, *d.get(o + 3)?];
    Some(if bo == ByteOrder::BigEndian { u32::from_be_bytes(b) } else { u32::from_le_bytes(b) })
}
fn i32_at(d: &[u8], o: usize, bo: ByteOrder) -> Option<i32> { u32_at(d, o, bo).map(|v| v as i32) }

/// A 16-bit value stored the other way round from the rest of the block.
fn u16rev_at(d: &[u8], o: usize, bo: ByteOrder) -> Option<u16> {
    let other = if bo == ByteOrder::BigEndian { ByteOrder::LittleEndian } else { ByteOrder::BigEndian };
    u16_at(d, o, other)
}

/// ExifTool's rational32 is two 16-bit halves -- four bytes, not the eight of
/// the rational64 EXIF writes. A zero denominator reads as infinity, and 0/0
/// as nothing at all.
fn rat32u_at(d: &[u8], o: usize, bo: ByteOrder) -> Option<f64> {
    ratio(f64::from(u16_at(d, o, bo)?), f64::from(u16_at(d, o + 2, bo)?))
}
fn rat32s_at(d: &[u8], o: usize, bo: ByteOrder) -> Option<f64> {
    ratio(f64::from(i16_at(d, o, bo)?), f64::from(i16_at(d, o + 2, bo)?))
}
fn ratio(n: f64, d: f64) -> Option<f64> {
    if d == 0.0 { return if n == 0.0 { None } else { Some(f64::INFINITY) }; }
    Some(n / d)
}

/// N bytes as text. A `string` stops at its first NUL, as ExifTool's reader
/// does; an `undef` is the bytes as they are.
fn text_at(d: &[u8], o: usize, n: usize, stop_at_nul: bool) -> Option<String> {
    let raw = d.get(o..o + n)?;
    let end = if stop_at_nul {
        raw.iter().position(|b| *b == 0).unwrap_or(raw.len())
    } else {
        raw.len()
    };
    Some(raw[..end].iter().map(|b| *b as char).collect())
}

/// Whether the block opens with these bytes, `None` accepting anything.
fn prefix_matches(d: &[u8], pat: &[Option<(u8, u8)>]) -> bool {
    if d.len() < pat.len() { return false; }
    pat.iter().zip(d).all(|(p, b)| p.is_none_or(|(lo, hi)| *b >= lo && *b <= hi))
}

fn mk(
    name: &str,
    id: u16,
    print_value: String,
    raw: Value,
    grp1: &'static str,
    grp2: &'static str,
    priority: i32,
) -> Tag {
    Tag {
        id: TagId::Numeric(id),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: "MakerNotes".into(),
            family1: grp1.into(),
            family2: grp2.into(),
            family3: crate::tag::MAIN_DOCUMENT.into(),
        },
        raw_value: raw,
        print_value,
        priority,
    }
}

static MODEL_RE_0: LazyLock<Regex> = LazyLock::new(|| Regex::new("EOS").expect("generated pattern"));
static MODEL_RE_1: LazyLock<Regex> = LazyLock::new(|| Regex::new("EOS-1DS?$").expect("generated pattern"));
static MODEL_RE_2: LazyLock<Regex> = LazyLock::new(|| Regex::new("\\b(20D|350D|REBEL XT|Kiss Digital N)\\b").expect("generated pattern"));
static MODEL_RE_3: LazyLock<Regex> = LazyLock::new(|| Regex::new("\\b1D$").expect("generated pattern"));
static MODEL_RE_4: LazyLock<Regex> = LazyLock::new(|| Regex::new("\\b1DS$").expect("generated pattern"));
static MODEL_RE_5: LazyLock<Regex> = LazyLock::new(|| Regex::new("\\b1D Mark III$").expect("generated pattern"));
static MODEL_RE_6: LazyLock<Regex> = LazyLock::new(|| Regex::new("EOS 60D$").expect("generated pattern"));
static MODEL_RE_7: LazyLock<Regex> = LazyLock::new(|| Regex::new("\\b(1200D|REBEL T5|Kiss X70)\\b").expect("generated pattern"));
static MODEL_RE_8: LazyLock<Regex> = LazyLock::new(|| Regex::new("(650D|REBEL T4i|Kiss X6i)\\b").expect("generated pattern"));
static MODEL_RE_9: LazyLock<Regex> = LazyLock::new(|| Regex::new("(700D|REBEL T5i|Kiss X7i)\\b").expect("generated pattern"));
static MODEL_RE_10: LazyLock<Regex> = LazyLock::new(|| Regex::new("^Canon EOS 5DS").expect("generated pattern"));
static MODEL_RE_11: LazyLock<Regex> = LazyLock::new(|| Regex::new("\\b1DS?$").expect("generated pattern"));
static MODEL_RE_12: LazyLock<Regex> = LazyLock::new(|| Regex::new("\\b1Ds? Mark II$").expect("generated pattern"));
static MODEL_RE_13: LazyLock<Regex> = LazyLock::new(|| Regex::new("\\b1Ds? Mark II N$").expect("generated pattern"));
static MODEL_RE_14: LazyLock<Regex> = LazyLock::new(|| Regex::new("\\b1Ds? Mark III$").expect("generated pattern"));
static MODEL_RE_15: LazyLock<Regex> = LazyLock::new(|| Regex::new("\\b1D Mark IV$").expect("generated pattern"));
static MODEL_RE_16: LazyLock<Regex> = LazyLock::new(|| Regex::new("EOS-1D X$").expect("generated pattern"));
static MODEL_RE_17: LazyLock<Regex> = LazyLock::new(|| Regex::new("EOS 5D$").expect("generated pattern"));
static MODEL_RE_18: LazyLock<Regex> = LazyLock::new(|| Regex::new("EOS 5D Mark II$").expect("generated pattern"));
static MODEL_RE_19: LazyLock<Regex> = LazyLock::new(|| Regex::new("EOS 5D Mark III$").expect("generated pattern"));
static MODEL_RE_20: LazyLock<Regex> = LazyLock::new(|| Regex::new("EOS 6D$").expect("generated pattern"));
static MODEL_RE_21: LazyLock<Regex> = LazyLock::new(|| Regex::new("EOS 7D$").expect("generated pattern"));
static MODEL_RE_22: LazyLock<Regex> = LazyLock::new(|| Regex::new("EOS 40D$").expect("generated pattern"));
static MODEL_RE_23: LazyLock<Regex> = LazyLock::new(|| Regex::new("EOS 50D$").expect("generated pattern"));
static MODEL_RE_24: LazyLock<Regex> = LazyLock::new(|| Regex::new("EOS 70D$").expect("generated pattern"));
static MODEL_RE_25: LazyLock<Regex> = LazyLock::new(|| Regex::new("EOS 80D$").expect("generated pattern"));
static MODEL_RE_26: LazyLock<Regex> = LazyLock::new(|| Regex::new("\\b(450D|REBEL XSi|Kiss X2)\\b").expect("generated pattern"));
static MODEL_RE_27: LazyLock<Regex> = LazyLock::new(|| Regex::new("\\b(500D|REBEL T1i|Kiss X3)\\b").expect("generated pattern"));
static MODEL_RE_28: LazyLock<Regex> = LazyLock::new(|| Regex::new("\\b(550D|REBEL T2i|Kiss X4)\\b").expect("generated pattern"));
static MODEL_RE_29: LazyLock<Regex> = LazyLock::new(|| Regex::new("\\b(600D|REBEL T3i|Kiss X5)\\b").expect("generated pattern"));
static MODEL_RE_30: LazyLock<Regex> = LazyLock::new(|| Regex::new("\\b(650D|REBEL T4i|Kiss X6i)\\b").expect("generated pattern"));
static MODEL_RE_31: LazyLock<Regex> = LazyLock::new(|| Regex::new("\\b(700D|REBEL T5i|Kiss X7i)\\b").expect("generated pattern"));
static MODEL_RE_32: LazyLock<Regex> = LazyLock::new(|| Regex::new("\\b(750D|Rebel T6i|Kiss X8i)\\b").expect("generated pattern"));
static MODEL_RE_33: LazyLock<Regex> = LazyLock::new(|| Regex::new("\\b(760D|Rebel T6s|8000D)\\b").expect("generated pattern"));
static MODEL_RE_34: LazyLock<Regex> = LazyLock::new(|| Regex::new("\\b(1000D|REBEL XS|Kiss F)\\b").expect("generated pattern"));
static MODEL_RE_35: LazyLock<Regex> = LazyLock::new(|| Regex::new("\\b(1100D|REBEL T3|Kiss X50)\\b").expect("generated pattern"));
static MODEL_RE_36: LazyLock<Regex> = LazyLock::new(|| Regex::new("\\bEOS R[56]$").expect("generated pattern"));
static MODEL_RE_37: LazyLock<Regex> = LazyLock::new(|| Regex::new("\\bEOS (R6m2|R8|R50)$").expect("generated pattern"));
static MODEL_RE_38: LazyLock<Regex> = LazyLock::new(|| Regex::new("\\bEOS R6 Mark III$").expect("generated pattern"));
static MODEL_RE_39: LazyLock<Regex> = LazyLock::new(|| Regex::new("\\bG5 X Mark II$").expect("generated pattern"));

/// Decode one binary sub-table by the name ExifTool gives it.
#[must_use]
pub fn decode(
    table: &str,
    data: &[u8],
    model: &str,
    bo: ByteOrder,
    file_type: &str,
    format: &str,
    dm: &mut State,
) -> Vec<Tag> {
    match table {
        "ColorData1" => canon_colordata1(data, model, bo, file_type, format, dm),
        "ColorData2" => canon_colordata2(data, model, bo, file_type, format, dm),
        "ColorData3" => canon_colordata3(data, model, bo, file_type, format, dm),
        "ColorData4" => canon_colordata4(data, model, bo, file_type, format, dm),
        "ColorData5" => canon_colordata5(data, model, bo, file_type, format, dm),
        "ColorData6" => canon_colordata6(data, model, bo, file_type, format, dm),
        "ColorData7" => canon_colordata7(data, model, bo, file_type, format, dm),
        "ColorData8" => canon_colordata8(data, model, bo, file_type, format, dm),
        "ColorData9" => canon_colordata9(data, model, bo, file_type, format, dm),
        "ColorData10" => canon_colordata10(data, model, bo, file_type, format, dm),
        "ColorData11" => canon_colordata11(data, model, bo, file_type, format, dm),
        "ColorData12" => canon_colordata12(data, model, bo, file_type, format, dm),
        "ColorDataUnknown" => canon_colordataunknown(data, model, bo, file_type, format, dm),
        "ShotInfo" => canon_shotinfo(data, model, bo, file_type, format, dm),
        "CameraInfo1D" => canon_camerainfo1d(data, model, bo, file_type, format, dm),
        "CameraInfo1DmkII" => canon_camerainfo1dmkii(data, model, bo, file_type, format, dm),
        "CameraInfo1DmkIIN" => canon_camerainfo1dmkiin(data, model, bo, file_type, format, dm),
        "CameraInfo1DmkIII" => canon_camerainfo1dmkiii(data, model, bo, file_type, format, dm),
        "CameraInfo1DmkIV" => canon_camerainfo1dmkiv(data, model, bo, file_type, format, dm),
        "CameraInfo1DX" => canon_camerainfo1dx(data, model, bo, file_type, format, dm),
        "CameraInfo5D" => canon_camerainfo5d(data, model, bo, file_type, format, dm),
        "CameraInfo5DmkII" => canon_camerainfo5dmkii(data, model, bo, file_type, format, dm),
        "CameraInfo5DmkIII" => canon_camerainfo5dmkiii(data, model, bo, file_type, format, dm),
        "CameraInfo6D" => canon_camerainfo6d(data, model, bo, file_type, format, dm),
        "CameraInfo7D" => canon_camerainfo7d(data, model, bo, file_type, format, dm),
        "CameraInfo40D" => canon_camerainfo40d(data, model, bo, file_type, format, dm),
        "CameraInfo50D" => canon_camerainfo50d(data, model, bo, file_type, format, dm),
        "CameraInfo60D" => canon_camerainfo60d(data, model, bo, file_type, format, dm),
        "CameraInfo70D" => canon_camerainfo70d(data, model, bo, file_type, format, dm),
        "CameraInfo80D" => canon_camerainfo80d(data, model, bo, file_type, format, dm),
        "CameraInfo450D" => canon_camerainfo450d(data, model, bo, file_type, format, dm),
        "CameraInfo500D" => canon_camerainfo500d(data, model, bo, file_type, format, dm),
        "CameraInfo550D" => canon_camerainfo550d(data, model, bo, file_type, format, dm),
        "CameraInfo600D" => canon_camerainfo600d(data, model, bo, file_type, format, dm),
        "CameraInfo650D" => canon_camerainfo650d(data, model, bo, file_type, format, dm),
        "CameraInfo750D" => canon_camerainfo750d(data, model, bo, file_type, format, dm),
        "CameraInfo1000D" => canon_camerainfo1000d(data, model, bo, file_type, format, dm),
        "CameraInfoR6" => canon_camerainfor6(data, model, bo, file_type, format, dm),
        "CameraInfoR6m2" => canon_camerainfor6m2(data, model, bo, file_type, format, dm),
        "CameraInfoR6m3" => canon_camerainfor6m3(data, model, bo, file_type, format, dm),
        "CameraInfoG5XII" => canon_camerainfog5xii(data, model, bo, file_type, format, dm),
        "CameraInfoPowerShot" => canon_camerainfopowershot(data, model, bo, file_type, format, dm),
        "CameraInfoPowerShot2" => canon_camerainfopowershot2(data, model, bo, file_type, format, dm),
        "CameraInfoUnknown32" => canon_camerainfounknown32(data, model, bo, file_type, format, dm),
        "CameraInfoUnknown16" => canon_camerainfounknown16(data, model, bo, file_type, format, dm),
        "CameraInfoUnknown" => canon_camerainfounknown(data, model, bo, file_type, format, dm),
        "ColorCalib" => canon_colorcalib(data, model, bo, file_type, format, dm),
        "ColorCoefs" => canon_colorcoefs(data, model, bo, file_type, format, dm),
        "ColorCoefs2" => canon_colorcoefs2(data, model, bo, file_type, format, dm),
        "ColorCalib2" => canon_colorcalib2(data, model, bo, file_type, format, dm),
        "PSInfo" => canon_psinfo(data, model, bo, file_type, format, dm),
        "PSInfo2" => canon_psinfo2(data, model, bo, file_type, format, dm),
        _ => Vec::new(),
    }
}

/// `Image::ExifTool::Canon::ColorData1` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colordata1(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x32 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsAsShot", 0x19, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x3a, bo) {
        dm.push(("ColorTempAsShot".to_string(), f64::from(v)));
        tags.push(mk("ColorTempAsShot", 0x1d, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x3c + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsAuto", 0x1e, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x44, bo) {
        dm.push(("ColorTempAuto".to_string(), f64::from(v)));
        tags.push(mk("ColorTempAuto", 0x22, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x46 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsDaylight", 0x23, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x4e, bo) {
        dm.push(("ColorTempDaylight".to_string(), f64::from(v)));
        tags.push(mk("ColorTempDaylight", 0x27, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x50 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsShade", 0x28, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x58, bo) {
        dm.push(("ColorTempShade".to_string(), f64::from(v)));
        tags.push(mk("ColorTempShade", 0x2c, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x5a + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsCloudy", 0x2d, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x62, bo) {
        dm.push(("ColorTempCloudy".to_string(), f64::from(v)));
        tags.push(mk("ColorTempCloudy", 0x31, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x64 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsTungsten", 0x32, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x6c, bo) {
        dm.push(("ColorTempTungsten".to_string(), f64::from(v)));
        tags.push(mk("ColorTempTungsten", 0x36, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x6e + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsFluorescent", 0x37, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x76, bo) {
        dm.push(("ColorTempFluorescent".to_string(), f64::from(v)));
        tags.push(mk("ColorTempFluorescent", 0x3b, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x78 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsFlash", 0x3c, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x80, bo) {
        dm.push(("ColorTempFlash".to_string(), f64::from(v)));
        tags.push(mk("ColorTempFlash", 0x40, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x82 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsCustom1", 0x41, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x8a, bo) {
        dm.push(("ColorTempCustom1".to_string(), f64::from(v)));
        tags.push(mk("ColorTempCustom1", 0x45, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x8c + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsCustom2", 0x46, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x94, bo) {
        dm.push(("ColorTempCustom2".to_string(), f64::from(v)));
        tags.push(mk("ColorTempCustom2", 0x4a, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(sub) = data.get(0x96..0x96 + 120) {
        tags.extend(canon_colorcalib(sub, model, bo, file_type, format, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::ColorData2` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colordata2(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x30 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsAuto", 0x18, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x38, bo) {
        dm.push(("ColorTempAuto".to_string(), f64::from(v)));
        tags.push(mk("ColorTempAuto", 0x1c, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x3a + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x42, bo) {
        dm.push(("ColorTempUnknown".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x44 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsAsShot", 0x22, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x4c, bo) {
        dm.push(("ColorTempAsShot".to_string(), f64::from(v)));
        tags.push(mk("ColorTempAsShot", 0x26, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x4e + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsDaylight", 0x27, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x56, bo) {
        dm.push(("ColorTempDaylight".to_string(), f64::from(v)));
        tags.push(mk("ColorTempDaylight", 0x2b, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x58 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsShade", 0x2c, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x60, bo) {
        dm.push(("ColorTempShade".to_string(), f64::from(v)));
        tags.push(mk("ColorTempShade", 0x30, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x62 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsCloudy", 0x31, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x6a, bo) {
        dm.push(("ColorTempCloudy".to_string(), f64::from(v)));
        tags.push(mk("ColorTempCloudy", 0x35, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x6c + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsTungsten", 0x36, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x74, bo) {
        dm.push(("ColorTempTungsten".to_string(), f64::from(v)));
        tags.push(mk("ColorTempTungsten", 0x3a, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x76 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsFluorescent", 0x3b, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x7e, bo) {
        dm.push(("ColorTempFluorescent".to_string(), f64::from(v)));
        tags.push(mk("ColorTempFluorescent", 0x3f, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x80 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsKelvin", 0x40, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x88, bo) {
        dm.push(("ColorTempKelvin".to_string(), f64::from(v)));
        tags.push(mk("ColorTempKelvin", 0x44, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x8a + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsFlash", 0x45, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x92, bo) {
        dm.push(("ColorTempFlash".to_string(), f64::from(v)));
        tags.push(mk("ColorTempFlash", 0x49, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x94 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x9c, bo) {
        dm.push(("ColorTempUnknown2".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x9e + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xa6, bo) {
        dm.push(("ColorTempUnknown3".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xa8 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xb0, bo) {
        dm.push(("ColorTempUnknown4".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xb2 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xba, bo) {
        dm.push(("ColorTempUnknown5".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xbc + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xc4, bo) {
        dm.push(("ColorTempUnknown6".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xc6 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xce, bo) {
        dm.push(("ColorTempUnknown7".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xd0 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xd8, bo) {
        dm.push(("ColorTempUnknown8".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xda + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xe2, bo) {
        dm.push(("ColorTempUnknown9".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xe4 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xec, bo) {
        dm.push(("ColorTempUnknown10".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xee + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xf6, bo) {
        dm.push(("ColorTempUnknown11".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xf8 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x100, bo) {
        dm.push(("ColorTempUnknown12".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x102 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x10a, bo) {
        dm.push(("ColorTempUnknown13".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x10c + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x114, bo) {
        dm.push(("ColorTempUnknown14".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x116 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x11e, bo) {
        dm.push(("ColorTempUnknown15".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x120 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsPC1", 0x90, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x128, bo) {
        dm.push(("ColorTempPC1".to_string(), f64::from(v)));
        tags.push(mk("ColorTempPC1", 0x94, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x12a + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsPC2", 0x95, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x132, bo) {
        dm.push(("ColorTempPC2".to_string(), f64::from(v)));
        tags.push(mk("ColorTempPC2", 0x99, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x134 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsPC3", 0x9a, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x13c, bo) {
        dm.push(("ColorTempPC3".to_string(), f64::from(v)));
        tags.push(mk("ColorTempPC3", 0x9e, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x13e + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x146, bo) {
        dm.push(("ColorTempUnknown16".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match u32_at(data, 0x4d4 + k * 4, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            let ctx = Ctx { model, file_type, dm };
            let mut cv = Conv::Str(s.clone());
            if let Some(x) = conv_expr::eval_with("Image::ExifTool::Canon::SwapWords($val)", &cv, &ctx) { cv = x; }
            let raw = Value::String(cv.as_string());
            tags.push(mk("RawMeasuredRGGB", 0x26a, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(sub) = data.get(0x148..0x148 + 120) {
        tags.extend(canon_colorcalib(sub, model, bo, file_type, format, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::ColorData3` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colordata3(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = i16_at(data, 0x0, bo) {
        dm.push(("ColorDataVersion".to_string(), f64::from(v)));
        let s = match v as i64 {
            1 => "1 (1DmkIIN/5D/30D/400D)".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ColorDataVersion", 0x0, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x7e + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsAsShot", 0x3f, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x86, bo) {
        dm.push(("ColorTempAsShot".to_string(), f64::from(v)));
        tags.push(mk("ColorTempAsShot", 0x43, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x88 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsAuto", 0x44, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x90, bo) {
        dm.push(("ColorTempAuto".to_string(), f64::from(v)));
        tags.push(mk("ColorTempAuto", 0x48, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x92 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsMeasured", 0x49, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x9a, bo) {
        dm.push(("ColorTempMeasured".to_string(), f64::from(v)));
        tags.push(mk("ColorTempMeasured", 0x4d, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x9c + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsDaylight", 0x4e, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0xa4, bo) {
        dm.push(("ColorTempDaylight".to_string(), f64::from(v)));
        tags.push(mk("ColorTempDaylight", 0x52, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xa6 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsShade", 0x53, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0xae, bo) {
        dm.push(("ColorTempShade".to_string(), f64::from(v)));
        tags.push(mk("ColorTempShade", 0x57, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xb0 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsCloudy", 0x58, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0xb8, bo) {
        dm.push(("ColorTempCloudy".to_string(), f64::from(v)));
        tags.push(mk("ColorTempCloudy", 0x5c, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xba + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsTungsten", 0x5d, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0xc2, bo) {
        dm.push(("ColorTempTungsten".to_string(), f64::from(v)));
        tags.push(mk("ColorTempTungsten", 0x61, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xc4 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsFluorescent", 0x62, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0xcc, bo) {
        dm.push(("ColorTempFluorescent".to_string(), f64::from(v)));
        tags.push(mk("ColorTempFluorescent", 0x66, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xce + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsKelvin", 0x67, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0xd6, bo) {
        dm.push(("ColorTempKelvin".to_string(), f64::from(v)));
        tags.push(mk("ColorTempKelvin", 0x6b, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xd8 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsFlash", 0x6c, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0xe0, bo) {
        dm.push(("ColorTempFlash".to_string(), f64::from(v)));
        tags.push(mk("ColorTempFlash", 0x70, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xe2 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsPC1", 0x71, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0xea, bo) {
        dm.push(("ColorTempPC1".to_string(), f64::from(v)));
        tags.push(mk("ColorTempPC1", 0x75, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xec + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsPC2", 0x76, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0xf4, bo) {
        dm.push(("ColorTempPC2".to_string(), f64::from(v)));
        tags.push(mk("ColorTempPC2", 0x7a, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xf6 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsPC3", 0x7b, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0xfe, bo) {
        dm.push(("ColorTempPC3".to_string(), f64::from(v)));
        tags.push(mk("ColorTempPC3", 0x7f, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x100 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsCustom", 0x80, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x108, bo) {
        dm.push(("ColorTempCustom".to_string(), f64::from(v)));
        tags.push(mk("ColorTempCustom", 0x84, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match u16_at(data, 0x188 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("PerChannelBlackLevel", 0xc4, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x490, bo) {
        dm.push(("FlashOutput".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val >= 255 ? 255 : exp(($val-200)/16*log(2))", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val == 255 ? \"Strobe or Misfire\" : sprintf(\"%.0f%%\", $val * 100)", &cv, &ctx) { cv = x; }
        tags.push(mk("FlashOutput", 0x248, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = i16_at(data, 0x492, bo) {
        dm.push(("FlashBatteryLevel".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val ? sprintf(\"%.2fV\", $val * 5 / 186) : \"n/a\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FlashBatteryLevel", 0x249, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = i16_at(data, 0x494, bo) {
        dm.push(("ColorTempFlashData".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("($val < 2000 or $val > 12000) ? undef : $val", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            tags.push(mk("ColorTempFlashData", 0x24a, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match u32_at(data, 0x50e + k * 4, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            let ctx = Ctx { model, file_type, dm };
            let mut cv = Conv::Str(s.clone());
            if let Some(x) = conv_expr::eval_with("Image::ExifTool::Canon::SwapWords($val)", &cv, &ctx) { cv = x; }
            let raw = Value::String(cv.as_string());
            tags.push(mk("MeasuredRGGBData", 0x287, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(sub) = data.get(0x10a..0x10a + 120) {
        tags.extend(canon_colorcalib(sub, model, bo, file_type, format, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::ColorData4` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colordata4(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = i16_at(data, 0x0, bo) {
        dm.push(("ColorDataVersion".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let s = match v as i64 {
                2 => "2 (1DmkIII)".to_string(),
                3 => "3 (40D)".to_string(),
                4 => "4 (1DSmkIII)".to_string(),
                5 => "5 (450D/1000D)".to_string(),
                6 => "6 (50D/5DmkII)".to_string(),
                7 => "7 (500D/550D/7D/1DmkIV)".to_string(),
                9 => "9 (60D/1100D)".to_string(),
                other => other.to_string(),
            };
            tags.push(mk("ColorDataVersion", 0x0, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match u16_at(data, 0x1ce + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("AverageBlackLevel", 0xe7, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x4d6, bo) {
        dm.push(("FlashOutput".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val >= 255 ? 255 : exp(($val-200)/16*log(2))", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val == 255 ? \"Strobe or Misfire\" : sprintf(\"%.0f%%\", $val * 100)", &cv, &ctx) { cv = x; }
        tags.push(mk("FlashOutput", 0x26b, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = i16_at(data, 0x4d8, bo) {
        dm.push(("FlashBatteryLevel".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val ? sprintf(\"%.2fV\", $val * 5 / 186) : \"n/a\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FlashBatteryLevel", 0x26c, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match u32_at(data, 0x500 + k * 4, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            let ctx = Ctx { model, file_type, dm };
            let mut cv = Conv::Str(s.clone());
            if let Some(x) = conv_expr::eval_with("Image::ExifTool::Canon::SwapWords($val)", &cv, &ctx) { cv = x; }
            let raw = Value::String(cv.as_string());
            tags.push(mk("RawMeasuredRGGB", 0x280, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if (dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 4.0) || dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 5.0)) {
        {
            let mut parts = Vec::new();
            for k in 0..4 {
                match u16_at(data, 0x568 + k * 2, bo) {
                    Some(x) => parts.push(x.to_string()),
                    None => { parts.clear(); break }
                }
            }
            if !parts.is_empty() {
                let s = parts.join(" ");
                tags.push(mk("PerChannelBlackLevel", 0x2b4, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
            }
        }
    }
    if (dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 4.0) || dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 5.0)) {
        if let Some(v) = u16_at(data, 0x570, bo) {
            dm.push(("NormalWhiteLevel".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let rc = conv_expr::eval_with("$val || undef", &Conv::Num(f64::from(v)), &ctx);
            if rc.as_ref() != Some(&Conv::Undef) {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let v = rc.map_or(v, |x| x.as_num() as _);
                tags.push(mk("NormalWhiteLevel", 0x2b8, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
            }
        }
    }
    if (dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 4.0) || dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 5.0)) {
        if let Some(v) = u16_at(data, 0x572, bo) {
            dm.push(("SpecularWhiteLevel".to_string(), f64::from(v)));
            tags.push(mk("SpecularWhiteLevel", 0x2b9, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if (dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 4.0) || dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 5.0)) {
        if let Some(v) = u16_at(data, 0x574, bo) {
            dm.push(("LinearityUpperMargin".to_string(), f64::from(v)));
            tags.push(mk("LinearityUpperMargin", 0x2ba, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if (dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 6.0) || dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 7.0)) {
        {
            let mut parts = Vec::new();
            for k in 0..4 {
                match u16_at(data, 0x596 + k * 2, bo) {
                    Some(x) => parts.push(x.to_string()),
                    None => { parts.clear(); break }
                }
            }
            if !parts.is_empty() {
                let s = parts.join(" ");
                tags.push(mk("PerChannelBlackLevel", 0x2cb, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
            }
        }
    }
    if (dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 6.0) || dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 7.0)) {
        if let Some(v) = u16_at(data, 0x59e, bo) {
            dm.push(("NormalWhiteLevel".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let rc = conv_expr::eval_with("$val || undef", &Conv::Num(f64::from(v)), &ctx);
            if rc.as_ref() != Some(&Conv::Undef) {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let v = rc.map_or(v, |x| x.as_num() as _);
                tags.push(mk("NormalWhiteLevel", 0x2cf, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
            }
        }
    }
    if !((dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 6.0) || dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 7.0))) && dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 9.0) {
        {
            let mut parts = Vec::new();
            for k in 0..4 {
                match u16_at(data, 0x59e + k * 2, bo) {
                    Some(x) => parts.push(x.to_string()),
                    None => { parts.clear(); break }
                }
            }
            if !parts.is_empty() {
                let s = parts.join(" ");
                tags.push(mk("PerChannelBlackLevel", 0x2cf, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
            }
        }
    }
    if (dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 6.0) || dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 7.0)) {
        if let Some(v) = u16_at(data, 0x5a0, bo) {
            dm.push(("SpecularWhiteLevel".to_string(), f64::from(v)));
            tags.push(mk("SpecularWhiteLevel", 0x2d0, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if (dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 6.0) || dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 7.0)) {
        if let Some(v) = u16_at(data, 0x5a2, bo) {
            dm.push(("LinearityUpperMargin".to_string(), f64::from(v)));
            tags.push(mk("LinearityUpperMargin", 0x2d1, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 9.0) {
        if let Some(v) = u16_at(data, 0x5a6, bo) {
            dm.push(("NormalWhiteLevel".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let rc = conv_expr::eval_with("$val || undef", &Conv::Num(f64::from(v)), &ctx);
            if rc.as_ref() != Some(&Conv::Undef) {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let v = rc.map_or(v, |x| x.as_num() as _);
                tags.push(mk("NormalWhiteLevel", 0x2d3, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
            }
        }
    }
    if dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 9.0) {
        if let Some(v) = u16_at(data, 0x5a8, bo) {
            dm.push(("SpecularWhiteLevel".to_string(), f64::from(v)));
            tags.push(mk("SpecularWhiteLevel", 0x2d4, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 9.0) {
        if let Some(v) = u16_at(data, 0x5aa, bo) {
            dm.push(("LinearityUpperMargin".to_string(), f64::from(v)));
            tags.push(mk("LinearityUpperMargin", 0x2d5, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if let Some(sub) = data.get(0x7e..0x7e + 210) {
        tags.extend(canon_colorcoefs(sub, model, bo, file_type, format, dm));
    }
    if let Some(sub) = data.get(0x150..0x150 + 120) {
        tags.extend(canon_colorcalib(sub, model, bo, file_type, format, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::ColorData5` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colordata5(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = i16_at(data, 0x0, bo) {
        dm.push(("ColorDataVersion".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let s = match v as i64 {
                -4 => "-4 (M100/M5/M6)".to_string(),
                -3 => "-3 (M10/M3)".to_string(),
                other => other.to_string(),
            };
            tags.push(mk("ColorDataVersion", 0x0, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if dm_get(dm, "ColorDataVersion").is_some_and(|v| v == -3.0) {
        {
            let mut parts = Vec::new();
            for k in 0..4 {
                match i16_at(data, 0x210 + k * 2, bo) {
                    Some(x) => parts.push(x.to_string()),
                    None => { parts.clear(); break }
                }
            }
            if !parts.is_empty() {
                let s = parts.join(" ");
                tags.push(mk("PerChannelBlackLevel", 0x108, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
            }
        }
    }
    if dm_get(dm, "ColorDataVersion").is_some_and(|v| v == -4.0) {
        {
            let mut parts = Vec::new();
            for k in 0..4 {
                match i16_at(data, 0x29a + k * 2, bo) {
                    Some(x) => parts.push(x.to_string()),
                    None => { parts.clear(); break }
                }
            }
            if !parts.is_empty() {
                let s = parts.join(" ");
                tags.push(mk("PerChannelBlackLevel", 0x14d, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
            }
        }
    }
    if dm_get(dm, "ColorDataVersion").is_some_and(|v| v == -3.0) {
        if let Some(v) = u16_at(data, 0x52c, bo) {
            dm.push(("SpecularWhiteLevel".to_string(), f64::from(v)));
            tags.push(mk("SpecularWhiteLevel", 0x296, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if dm_get(dm, "ColorDataVersion").is_some_and(|v| v == -4.0) {
        if let Some(v) = u16_at(data, 0xad2, bo) {
            dm.push(("NormalWhiteLevel".to_string(), f64::from(v)));
            tags.push(mk("NormalWhiteLevel", 0x569, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if dm_get(dm, "ColorDataVersion").is_some_and(|v| v == -4.0) {
        if let Some(v) = u16_at(data, 0xad4, bo) {
            dm.push(("SpecularWhiteLevel".to_string(), f64::from(v)));
            tags.push(mk("SpecularWhiteLevel", 0x56a, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if dm_get(dm, "ColorDataVersion").is_some_and(|v| v == -3.0) {
        if let Some(sub) = data.get(0x8e..0x8e + 230) {
            tags.extend(canon_colorcoefs(sub, model, bo, file_type, format, dm));
        }
    }
    if !(dm_get(dm, "ColorDataVersion").is_some_and(|v| v == -3.0)) && dm_get(dm, "ColorDataVersion").is_some_and(|v| v == -4.0) {
        if let Some(sub) = data.get(0x8e..0x8e + 368) {
            tags.extend(canon_colorcoefs2(sub, model, bo, file_type, format, dm));
        }
    }
    if dm_get(dm, "ColorDataVersion").is_some_and(|v| v == -3.0) {
        if let Some(sub) = data.get(0x174..0x174 + 150) {
            tags.extend(canon_colorcalib2(sub, model, bo, file_type, format, dm));
        }
    }
    if dm_get(dm, "ColorDataVersion").is_some_and(|v| v == -4.0) {
        if let Some(sub) = data.get(0x1fe..0x1fe + 150) {
            tags.extend(canon_colorcalib2(sub, model, bo, file_type, format, dm));
        }
    }
    tags
}

/// `Image::ExifTool::Canon::ColorData6` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colordata6(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = i16_at(data, 0x0, bo) {
        dm.push(("ColorDataVersion".to_string(), f64::from(v)));
        let s = match v as i64 {
            10 => "10 (600D/1200D)".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ColorDataVersion", 0x0, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x7e + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsAsShot", 0x3f, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x86, bo) {
        dm.push(("ColorTempAsShot".to_string(), f64::from(v)));
        tags.push(mk("ColorTempAsShot", 0x43, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x88 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsAuto", 0x44, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x90, bo) {
        dm.push(("ColorTempAuto".to_string(), f64::from(v)));
        tags.push(mk("ColorTempAuto", 0x48, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x92 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsMeasured", 0x49, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x9a, bo) {
        dm.push(("ColorTempMeasured".to_string(), f64::from(v)));
        tags.push(mk("ColorTempMeasured", 0x4d, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x9c + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xa4, bo) {
        dm.push(("ColorTempUnknown".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xa6 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xae, bo) {
        dm.push(("ColorTempUnknown2".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xb0 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xb8, bo) {
        dm.push(("ColorTempUnknown3".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xba + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xc2, bo) {
        dm.push(("ColorTempUnknown4".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xc4 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xcc, bo) {
        dm.push(("ColorTempUnknown5".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xce + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsDaylight", 0x67, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0xd6, bo) {
        dm.push(("ColorTempDaylight".to_string(), f64::from(v)));
        tags.push(mk("ColorTempDaylight", 0x6b, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xd8 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsShade", 0x6c, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0xe0, bo) {
        dm.push(("ColorTempShade".to_string(), f64::from(v)));
        tags.push(mk("ColorTempShade", 0x70, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xe2 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsCloudy", 0x71, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0xea, bo) {
        dm.push(("ColorTempCloudy".to_string(), f64::from(v)));
        tags.push(mk("ColorTempCloudy", 0x75, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xec + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsTungsten", 0x76, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0xf4, bo) {
        dm.push(("ColorTempTungsten".to_string(), f64::from(v)));
        tags.push(mk("ColorTempTungsten", 0x7a, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xf6 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsFluorescent", 0x7b, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0xfe, bo) {
        dm.push(("ColorTempFluorescent".to_string(), f64::from(v)));
        tags.push(mk("ColorTempFluorescent", 0x7f, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x100 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsKelvin", 0x80, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x108, bo) {
        dm.push(("ColorTempKelvin".to_string(), f64::from(v)));
        tags.push(mk("ColorTempKelvin", 0x84, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x10a + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsFlash", 0x85, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x112, bo) {
        dm.push(("ColorTempFlash".to_string(), f64::from(v)));
        tags.push(mk("ColorTempFlash", 0x89, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x114 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x11c, bo) {
        dm.push(("ColorTempUnknown6".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x11e + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x126, bo) {
        dm.push(("ColorTempUnknown7".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x128 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x130, bo) {
        dm.push(("ColorTempUnknown8".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x132 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x13a, bo) {
        dm.push(("ColorTempUnknown9".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x13c + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x144, bo) {
        dm.push(("ColorTempUnknown10".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x146 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x14e, bo) {
        dm.push(("ColorTempUnknown11".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x150 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x158, bo) {
        dm.push(("ColorTempUnknown12".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x15a + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x162, bo) {
        dm.push(("ColorTempUnknown13".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x164 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x16c, bo) {
        dm.push(("ColorTempUnknown14".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x16e + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x176, bo) {
        dm.push(("ColorTempUnknown15".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match u16_at(data, 0x1f6 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("AverageBlackLevel", 0xfb, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match u32_at(data, 0x328 + k * 4, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            let ctx = Ctx { model, file_type, dm };
            let mut cv = Conv::Str(s.clone());
            if let Some(x) = conv_expr::eval_with("Image::ExifTool::Canon::SwapWords($val)", &cv, &ctx) { cv = x; }
            let raw = Value::String(cv.as_string());
            tags.push(mk("RawMeasuredRGGB", 0x194, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match u16_at(data, 0x3be + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("PerChannelBlackLevel", 0x1df, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u16_at(data, 0x3c6, bo) {
        dm.push(("NormalWhiteLevel".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val || undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            tags.push(mk("NormalWhiteLevel", 0x1e3, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u16_at(data, 0x3c8, bo) {
        dm.push(("SpecularWhiteLevel".to_string(), f64::from(v)));
        tags.push(mk("SpecularWhiteLevel", 0x1e4, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x3ca, bo) {
        dm.push(("LinearityUpperMargin".to_string(), f64::from(v)));
        tags.push(mk("LinearityUpperMargin", 0x1e5, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(sub) = data.get(0x178..0x178 + 120) {
        tags.extend(canon_colorcalib(sub, model, bo, file_type, format, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::ColorData7` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colordata7(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = i16_at(data, 0x0, bo) {
        dm.push(("ColorDataVersion".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let s = match v as i64 {
                10 => "10 (1DX/5DmkIII/6D/70D/100D/650D/700D/M/M2)".to_string(),
                11 => "11 (7DmkII/750D/760D/8000D)".to_string(),
                other => other.to_string(),
            };
            tags.push(mk("ColorDataVersion", 0x0, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x7e + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsAsShot", 0x3f, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x86, bo) {
        dm.push(("ColorTempAsShot".to_string(), f64::from(v)));
        tags.push(mk("ColorTempAsShot", 0x43, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x88 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsAuto", 0x44, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x90, bo) {
        dm.push(("ColorTempAuto".to_string(), f64::from(v)));
        tags.push(mk("ColorTempAuto", 0x48, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x92 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsMeasured", 0x49, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x9a, bo) {
        dm.push(("ColorTempMeasured".to_string(), f64::from(v)));
        tags.push(mk("ColorTempMeasured", 0x4d, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x9c + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xa4, bo) {
        dm.push(("ColorTempUnknown".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xa6 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xae, bo) {
        dm.push(("ColorTempUnknown2".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xb0 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xb8, bo) {
        dm.push(("ColorTempUnknown3".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xba + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xc2, bo) {
        dm.push(("ColorTempUnknown4".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xc4 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xcc, bo) {
        dm.push(("ColorTempUnknown5".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xce + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xd6, bo) {
        dm.push(("ColorTempUnknown6".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xd8 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xe0, bo) {
        dm.push(("ColorTempUnknown7".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xe2 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xea, bo) {
        dm.push(("ColorTempUnknown8".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xec + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xf4, bo) {
        dm.push(("ColorTempUnknown9".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xf6 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xfe, bo) {
        dm.push(("ColorTempUnknown10".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x100 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsDaylight", 0x80, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x108, bo) {
        dm.push(("ColorTempDaylight".to_string(), f64::from(v)));
        tags.push(mk("ColorTempDaylight", 0x84, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x10a + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsShade", 0x85, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x112, bo) {
        dm.push(("ColorTempShade".to_string(), f64::from(v)));
        tags.push(mk("ColorTempShade", 0x89, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x114 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsCloudy", 0x8a, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x11c, bo) {
        dm.push(("ColorTempCloudy".to_string(), f64::from(v)));
        tags.push(mk("ColorTempCloudy", 0x8e, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x11e + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsTungsten", 0x8f, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x126, bo) {
        dm.push(("ColorTempTungsten".to_string(), f64::from(v)));
        tags.push(mk("ColorTempTungsten", 0x93, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x128 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsFluorescent", 0x94, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x130, bo) {
        dm.push(("ColorTempFluorescent".to_string(), f64::from(v)));
        tags.push(mk("ColorTempFluorescent", 0x98, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x132 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsKelvin", 0x99, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x13a, bo) {
        dm.push(("ColorTempKelvin".to_string(), f64::from(v)));
        tags.push(mk("ColorTempKelvin", 0x9d, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x13c + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsFlash", 0x9e, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x144, bo) {
        dm.push(("ColorTempFlash".to_string(), f64::from(v)));
        tags.push(mk("ColorTempFlash", 0xa2, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x146 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x14e, bo) {
        dm.push(("ColorTempUnknown11".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x150 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x158, bo) {
        dm.push(("ColorTempUnknown12".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x15a + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x162, bo) {
        dm.push(("ColorTempUnknown13".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x164 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x16c, bo) {
        dm.push(("ColorTempUnknown14".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x16e + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x176, bo) {
        dm.push(("ColorTempUnknown15".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x178 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x180, bo) {
        dm.push(("ColorTempUnknown16".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x182 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x18a, bo) {
        dm.push(("ColorTempUnknown17".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x18c + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x194, bo) {
        dm.push(("ColorTempUnknown18".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x196 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x19e, bo) {
        dm.push(("ColorTempUnknown19".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1a0 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1a8, bo) {
        dm.push(("ColorTempUnknown20".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match u16_at(data, 0x228 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("AverageBlackLevel", 0x114, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x330, bo) {
        dm.push(("FlashOutput".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val >= 255 ? 255 : exp(($val-200)/16*log(2))", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val == 255 ? \"Strobe or Misfire\" : sprintf(\"%.0f%%\", $val * 100)", &cv, &ctx) { cv = x; }
        tags.push(mk("FlashOutput", 0x198, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = i16_at(data, 0x332, bo) {
        dm.push(("FlashBatteryLevel".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val ? sprintf(\"%.2fV\", $val * 5 / 186) : \"n/a\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FlashBatteryLevel", 0x199, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 10.0) {
        {
            let mut parts = Vec::new();
            for k in 0..4 {
                match u32_at(data, 0x35a + k * 4, bo) {
                    Some(x) => parts.push(x.to_string()),
                    None => { parts.clear(); break }
                }
            }
            if !parts.is_empty() {
                let s = parts.join(" ");
                let ctx = Ctx { model, file_type, dm };
                let mut cv = Conv::Str(s.clone());
                if let Some(x) = conv_expr::eval_with("Image::ExifTool::Canon::SwapWords($val)", &cv, &ctx) { cv = x; }
                let raw = Value::String(cv.as_string());
                tags.push(mk("RawMeasuredRGGB", 0x1ad, cv.as_string(), raw, GRP1, GRP2, PRIO));
            }
        }
    }
    if dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 10.0) {
        {
            let mut parts = Vec::new();
            for k in 0..4 {
                match u16_at(data, 0x3f0 + k * 2, bo) {
                    Some(x) => parts.push(x.to_string()),
                    None => { parts.clear(); break }
                }
            }
            if !parts.is_empty() {
                let s = parts.join(" ");
                tags.push(mk("PerChannelBlackLevel", 0x1f8, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
            }
        }
    }
    if dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 10.0) {
        if let Some(v) = u16_at(data, 0x3f8, bo) {
            dm.push(("NormalWhiteLevel".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let rc = conv_expr::eval_with("$val || undef", &Conv::Num(f64::from(v)), &ctx);
            if rc.as_ref() != Some(&Conv::Undef) {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let v = rc.map_or(v, |x| x.as_num() as _);
                tags.push(mk("NormalWhiteLevel", 0x1fc, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
            }
        }
    }
    if dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 10.0) {
        if let Some(v) = u16_at(data, 0x3fa, bo) {
            dm.push(("SpecularWhiteLevel".to_string(), f64::from(v)));
            tags.push(mk("SpecularWhiteLevel", 0x1fd, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 10.0) {
        if let Some(v) = u16_at(data, 0x3fc, bo) {
            dm.push(("LinearityUpperMargin".to_string(), f64::from(v)));
            tags.push(mk("LinearityUpperMargin", 0x1fe, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 11.0) {
        {
            let mut parts = Vec::new();
            for k in 0..4 {
                match u32_at(data, 0x4d6 + k * 4, bo) {
                    Some(x) => parts.push(x.to_string()),
                    None => { parts.clear(); break }
                }
            }
            if !parts.is_empty() {
                let s = parts.join(" ");
                let ctx = Ctx { model, file_type, dm };
                let mut cv = Conv::Str(s.clone());
                if let Some(x) = conv_expr::eval_with("Image::ExifTool::Canon::SwapWords($val)", &cv, &ctx) { cv = x; }
                let raw = Value::String(cv.as_string());
                tags.push(mk("RawMeasuredRGGB", 0x26b, cv.as_string(), raw, GRP1, GRP2, PRIO));
            }
        }
    }
    if dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 11.0) {
        {
            let mut parts = Vec::new();
            for k in 0..4 {
                match u16_at(data, 0x5b0 + k * 2, bo) {
                    Some(x) => parts.push(x.to_string()),
                    None => { parts.clear(); break }
                }
            }
            if !parts.is_empty() {
                let s = parts.join(" ");
                tags.push(mk("PerChannelBlackLevel", 0x2d8, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
            }
        }
    }
    if dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 11.0) {
        if let Some(v) = u16_at(data, 0x5b8, bo) {
            dm.push(("NormalWhiteLevel".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let rc = conv_expr::eval_with("$val || undef", &Conv::Num(f64::from(v)), &ctx);
            if rc.as_ref() != Some(&Conv::Undef) {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let v = rc.map_or(v, |x| x.as_num() as _);
                tags.push(mk("NormalWhiteLevel", 0x2dc, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
            }
        }
    }
    if dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 11.0) {
        if let Some(v) = u16_at(data, 0x5ba, bo) {
            dm.push(("SpecularWhiteLevel".to_string(), f64::from(v)));
            tags.push(mk("SpecularWhiteLevel", 0x2dd, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 11.0) {
        if let Some(v) = u16_at(data, 0x5bc, bo) {
            dm.push(("LinearityUpperMargin".to_string(), f64::from(v)));
            tags.push(mk("LinearityUpperMargin", 0x2de, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if let Some(sub) = data.get(0x1aa..0x1aa + 120) {
        tags.extend(canon_colorcalib(sub, model, bo, file_type, format, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::ColorData8` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colordata8(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = i16_at(data, 0x0, bo) {
        dm.push(("ColorDataVersion".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let s = match v as i64 {
                12 => "12 (1DXmkII/5DS/5DSR)".to_string(),
                13 => "13 (80D/5DmkIV)".to_string(),
                14 => "14 (1300D/2000D/4000D)".to_string(),
                15 => "15 (6DmkII/77D/200D/800D,9000D)".to_string(),
                other => other.to_string(),
            };
            tags.push(mk("ColorDataVersion", 0x0, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x7e + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsAsShot", 0x3f, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x86, bo) {
        dm.push(("ColorTempAsShot".to_string(), f64::from(v)));
        tags.push(mk("ColorTempAsShot", 0x43, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x88 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsAuto", 0x44, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x90, bo) {
        dm.push(("ColorTempAuto".to_string(), f64::from(v)));
        tags.push(mk("ColorTempAuto", 0x48, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x92 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsMeasured", 0x49, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x9a, bo) {
        dm.push(("ColorTempMeasured".to_string(), f64::from(v)));
        tags.push(mk("ColorTempMeasured", 0x4d, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x9c + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xa4, bo) {
        dm.push(("ColorTempUnknown".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xa6 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xae, bo) {
        dm.push(("ColorTempUnknown2".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xb0 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xb8, bo) {
        dm.push(("ColorTempUnknown3".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xba + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xc2, bo) {
        dm.push(("ColorTempUnknown4".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xc4 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xcc, bo) {
        dm.push(("ColorTempUnknown5".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xce + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xd6, bo) {
        dm.push(("ColorTempUnknown6".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xd8 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xe0, bo) {
        dm.push(("ColorTempUnknown7".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xe2 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xea, bo) {
        dm.push(("ColorTempUnknown8".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xec + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xf4, bo) {
        dm.push(("ColorTempUnknown9".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xf6 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xfe, bo) {
        dm.push(("ColorTempUnknown10".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x100 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x108, bo) {
        dm.push(("ColorTempUnknown11".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x10a + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsDaylight", 0x85, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x112, bo) {
        dm.push(("ColorTempDaylight".to_string(), f64::from(v)));
        tags.push(mk("ColorTempDaylight", 0x89, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x114 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsShade", 0x8a, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x11c, bo) {
        dm.push(("ColorTempShade".to_string(), f64::from(v)));
        tags.push(mk("ColorTempShade", 0x8e, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x11e + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsCloudy", 0x8f, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x126, bo) {
        dm.push(("ColorTempCloudy".to_string(), f64::from(v)));
        tags.push(mk("ColorTempCloudy", 0x93, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x128 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsTungsten", 0x94, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x130, bo) {
        dm.push(("ColorTempTungsten".to_string(), f64::from(v)));
        tags.push(mk("ColorTempTungsten", 0x98, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x132 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsFluorescent", 0x99, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x13a, bo) {
        dm.push(("ColorTempFluorescent".to_string(), f64::from(v)));
        tags.push(mk("ColorTempFluorescent", 0x9d, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x13c + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsKelvin", 0x9e, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x144, bo) {
        dm.push(("ColorTempKelvin".to_string(), f64::from(v)));
        tags.push(mk("ColorTempKelvin", 0xa2, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x146 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsFlash", 0xa3, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x14e, bo) {
        dm.push(("ColorTempFlash".to_string(), f64::from(v)));
        tags.push(mk("ColorTempFlash", 0xa7, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x150 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x158, bo) {
        dm.push(("ColorTempUnknown12".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x15a + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x162, bo) {
        dm.push(("ColorTempUnknown13".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x164 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x16c, bo) {
        dm.push(("ColorTempUnknown14".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x16e + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x176, bo) {
        dm.push(("ColorTempUnknown15".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x178 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x180, bo) {
        dm.push(("ColorTempUnknown16".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x182 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x18a, bo) {
        dm.push(("ColorTempUnknown17".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x18c + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x194, bo) {
        dm.push(("ColorTempUnknown18".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x196 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x19e, bo) {
        dm.push(("ColorTempUnknown19".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1a0 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1a8, bo) {
        dm.push(("ColorTempUnknown20".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1aa + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1b2, bo) {
        dm.push(("ColorTempUnknown21".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1b4 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1bc, bo) {
        dm.push(("ColorTempUnknown22".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1be + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1c6, bo) {
        dm.push(("ColorTempUnknown23".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1c8 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1d0, bo) {
        dm.push(("ColorTempUnknown24".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1d2 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1da, bo) {
        dm.push(("ColorTempUnknown25".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1dc + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1e4, bo) {
        dm.push(("ColorTempUnknown26".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1e6 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1ee, bo) {
        dm.push(("ColorTempUnknown27".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1f0 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1f8, bo) {
        dm.push(("ColorTempUnknown28".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1fa + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x202, bo) {
        dm.push(("ColorTempUnknown29".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x204 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x20c, bo) {
        dm.push(("ColorTempUnknown30".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match u16_at(data, 0x28c + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("AverageBlackLevel", 0x146, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 14.0) {
        {
            let mut parts = Vec::new();
            for k in 0..4 {
                match u16_at(data, 0x458 + k * 2, bo) {
                    Some(x) => parts.push(x.to_string()),
                    None => { parts.clear(); break }
                }
            }
            if !parts.is_empty() {
                let s = parts.join(" ");
                tags.push(mk("PerChannelBlackLevel", 0x22c, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
            }
        }
    }
    if dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 14.0) {
        if let Some(v) = u16_at(data, 0x460, bo) {
            dm.push(("NormalWhiteLevel".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let rc = conv_expr::eval_with("$val || undef", &Conv::Num(f64::from(v)), &ctx);
            if rc.as_ref() != Some(&Conv::Undef) {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let v = rc.map_or(v, |x| x.as_num() as _);
                tags.push(mk("NormalWhiteLevel", 0x230, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
            }
        }
    }
    if dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 14.0) {
        if let Some(v) = u16_at(data, 0x462, bo) {
            dm.push(("SpecularWhiteLevel".to_string(), f64::from(v)));
            tags.push(mk("SpecularWhiteLevel", 0x231, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 14.0) {
        if let Some(v) = u16_at(data, 0x464, bo) {
            dm.push(("LinearityUpperMargin".to_string(), f64::from(v)));
            tags.push(mk("LinearityUpperMargin", 0x232, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if (dm_get(dm, "ColorDataVersion").is_some_and(|v| v < 14.0) || dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 15.0)) {
        {
            let mut parts = Vec::new();
            for k in 0..4 {
                match u16_at(data, 0x614 + k * 2, bo) {
                    Some(x) => parts.push(x.to_string()),
                    None => { parts.clear(); break }
                }
            }
            if !parts.is_empty() {
                let s = parts.join(" ");
                tags.push(mk("PerChannelBlackLevel", 0x30a, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
            }
        }
    }
    if (dm_get(dm, "ColorDataVersion").is_some_and(|v| v < 14.0) || dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 15.0)) {
        if let Some(v) = u16_at(data, 0x61c, bo) {
            dm.push(("NormalWhiteLevel".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let rc = conv_expr::eval_with("$val || undef", &Conv::Num(f64::from(v)), &ctx);
            if rc.as_ref() != Some(&Conv::Undef) {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let v = rc.map_or(v, |x| x.as_num() as _);
                tags.push(mk("NormalWhiteLevel", 0x30e, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
            }
        }
    }
    if (dm_get(dm, "ColorDataVersion").is_some_and(|v| v < 14.0) || dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 15.0)) {
        if let Some(v) = u16_at(data, 0x61e, bo) {
            dm.push(("SpecularWhiteLevel".to_string(), f64::from(v)));
            tags.push(mk("SpecularWhiteLevel", 0x30f, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if (dm_get(dm, "ColorDataVersion").is_some_and(|v| v < 14.0) || dm_get(dm, "ColorDataVersion").is_some_and(|v| v == 15.0)) {
        if let Some(v) = u16_at(data, 0x620, bo) {
            dm.push(("LinearityUpperMargin".to_string(), f64::from(v)));
            tags.push(mk("LinearityUpperMargin", 0x310, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if let Some(sub) = data.get(0x20e..0x20e + 120) {
        tags.extend(canon_colorcalib(sub, model, bo, file_type, format, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::ColorData9` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colordata9(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = i16_at(data, 0x0, bo) {
        dm.push(("ColorDataVersion".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let s = match v as i64 {
                16 => "16 (M50)".to_string(),
                17 => "17 (R)".to_string(),
                18 => "18 (RP/250D)".to_string(),
                19 => "19 (90D/850D/M6mkII/M200)".to_string(),
                other => other.to_string(),
            };
            tags.push(mk("ColorDataVersion", 0x0, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x8e + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsAsShot", 0x47, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x96, bo) {
        dm.push(("ColorTempAsShot".to_string(), f64::from(v)));
        tags.push(mk("ColorTempAsShot", 0x4b, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x98 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsAuto", 0x4c, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0xa0, bo) {
        dm.push(("ColorTempAuto".to_string(), f64::from(v)));
        tags.push(mk("ColorTempAuto", 0x50, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xa2 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsMeasured", 0x51, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0xaa, bo) {
        dm.push(("ColorTempMeasured".to_string(), f64::from(v)));
        tags.push(mk("ColorTempMeasured", 0x55, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xac + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xb4, bo) {
        dm.push(("ColorTempUnknown".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xb6 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xbe, bo) {
        dm.push(("ColorTempUnknown2".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xc0 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xc8, bo) {
        dm.push(("ColorTempUnknown3".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xca + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xd2, bo) {
        dm.push(("ColorTempUnknown4".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xd4 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xdc, bo) {
        dm.push(("ColorTempUnknown5".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xde + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xe6, bo) {
        dm.push(("ColorTempUnknown6".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xe8 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xf0, bo) {
        dm.push(("ColorTempUnknown7".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xf2 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xfa, bo) {
        dm.push(("ColorTempUnknown8".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xfc + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x104, bo) {
        dm.push(("ColorTempUnknown9".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x106 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x10e, bo) {
        dm.push(("ColorTempUnknown10".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x110 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsDaylight", 0x88, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x118, bo) {
        dm.push(("ColorTempDaylight".to_string(), f64::from(v)));
        tags.push(mk("ColorTempDaylight", 0x8c, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x11a + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsShade", 0x8d, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x122, bo) {
        dm.push(("ColorTempShade".to_string(), f64::from(v)));
        tags.push(mk("ColorTempShade", 0x91, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x124 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsCloudy", 0x92, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x12c, bo) {
        dm.push(("ColorTempCloudy".to_string(), f64::from(v)));
        tags.push(mk("ColorTempCloudy", 0x96, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x12e + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsTungsten", 0x97, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x136, bo) {
        dm.push(("ColorTempTungsten".to_string(), f64::from(v)));
        tags.push(mk("ColorTempTungsten", 0x9b, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x138 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsFluorescent", 0x9c, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x140, bo) {
        dm.push(("ColorTempFluorescent".to_string(), f64::from(v)));
        tags.push(mk("ColorTempFluorescent", 0xa0, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x142 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsKelvin", 0xa1, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x14a, bo) {
        dm.push(("ColorTempKelvin".to_string(), f64::from(v)));
        tags.push(mk("ColorTempKelvin", 0xa5, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x14c + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsFlash", 0xa6, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x154, bo) {
        dm.push(("ColorTempFlash".to_string(), f64::from(v)));
        tags.push(mk("ColorTempFlash", 0xaa, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x156 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x15e, bo) {
        dm.push(("ColorTempUnknown11".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x160 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x168, bo) {
        dm.push(("ColorTempUnknown12".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x16a + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x172, bo) {
        dm.push(("ColorTempUnknown13".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x174 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x17c, bo) {
        dm.push(("ColorTempUnknown14".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x17e + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x186, bo) {
        dm.push(("ColorTempUnknown15".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x188 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x190, bo) {
        dm.push(("ColorTempUnknown16".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x192 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x19a, bo) {
        dm.push(("ColorTempUnknown17".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x19c + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1a4, bo) {
        dm.push(("ColorTempUnknown18".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1a6 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1ae, bo) {
        dm.push(("ColorTempUnknown19".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1b0 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1b8, bo) {
        dm.push(("ColorTempUnknown20".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1ba + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1c2, bo) {
        dm.push(("ColorTempUnknown21".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1c4 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1cc, bo) {
        dm.push(("ColorTempUnknown22".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1ce + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1d6, bo) {
        dm.push(("ColorTempUnknown23".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1d8 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1e0, bo) {
        dm.push(("ColorTempUnknown24".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1e2 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1ea, bo) {
        dm.push(("ColorTempUnknown25".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1ec + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1f4, bo) {
        dm.push(("ColorTempUnknown26".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1f6 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1fe, bo) {
        dm.push(("ColorTempUnknown27".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x200 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x208, bo) {
        dm.push(("ColorTempUnknown28".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x20a + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x212, bo) {
        dm.push(("ColorTempUnknown29".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match u16_at(data, 0x292 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("PerChannelBlackLevel", 0x149, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u16_at(data, 0x638, bo) {
        dm.push(("NormalWhiteLevel".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val || undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            tags.push(mk("NormalWhiteLevel", 0x31c, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u16_at(data, 0x63a, bo) {
        dm.push(("SpecularWhiteLevel".to_string(), f64::from(v)));
        tags.push(mk("SpecularWhiteLevel", 0x31d, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x63c, bo) {
        dm.push(("LinearityUpperMargin".to_string(), f64::from(v)));
        tags.push(mk("LinearityUpperMargin", 0x31e, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(sub) = data.get(0x214..0x214 + 120) {
        tags.extend(canon_colorcalib(sub, model, bo, file_type, format, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::ColorData10` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colordata10(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = i16_at(data, 0x0, bo) {
        dm.push(("ColorDataVersion".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let s = match v as i64 {
                32 => "32 (1DXmkIII)".to_string(),
                33 => "33 (R5/R6)".to_string(),
                other => other.to_string(),
            };
            tags.push(mk("ColorDataVersion", 0x0, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xaa + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsAsShot", 0x55, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0xb2, bo) {
        dm.push(("ColorTempAsShot".to_string(), f64::from(v)));
        tags.push(mk("ColorTempAsShot", 0x59, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xb4 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsAuto", 0x5a, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0xbc, bo) {
        dm.push(("ColorTempAuto".to_string(), f64::from(v)));
        tags.push(mk("ColorTempAuto", 0x5e, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xbe + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsMeasured", 0x5f, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0xc6, bo) {
        dm.push(("ColorTempMeasured".to_string(), f64::from(v)));
        tags.push(mk("ColorTempMeasured", 0x63, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xc8 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xd0, bo) {
        dm.push(("ColorTempUnknown".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xd2 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xda, bo) {
        dm.push(("ColorTempUnknown2".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xdc + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xe4, bo) {
        dm.push(("ColorTempUnknown3".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xe6 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xee, bo) {
        dm.push(("ColorTempUnknown4".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xf0 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xf8, bo) {
        dm.push(("ColorTempUnknown5".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xfa + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x102, bo) {
        dm.push(("ColorTempUnknown6".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x104 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x10c, bo) {
        dm.push(("ColorTempUnknown7".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x10e + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x116, bo) {
        dm.push(("ColorTempUnknown8".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x118 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x120, bo) {
        dm.push(("ColorTempUnknown9".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x122 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x12a, bo) {
        dm.push(("ColorTempUnknown10".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x12c + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsDaylight", 0x96, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x134, bo) {
        dm.push(("ColorTempDaylight".to_string(), f64::from(v)));
        tags.push(mk("ColorTempDaylight", 0x9a, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x136 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsShade", 0x9b, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x13e, bo) {
        dm.push(("ColorTempShade".to_string(), f64::from(v)));
        tags.push(mk("ColorTempShade", 0x9f, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x140 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsCloudy", 0xa0, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x148, bo) {
        dm.push(("ColorTempCloudy".to_string(), f64::from(v)));
        tags.push(mk("ColorTempCloudy", 0xa4, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x14a + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsTungsten", 0xa5, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x152, bo) {
        dm.push(("ColorTempTungsten".to_string(), f64::from(v)));
        tags.push(mk("ColorTempTungsten", 0xa9, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x154 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsFluorescent", 0xaa, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x15c, bo) {
        dm.push(("ColorTempFluorescent".to_string(), f64::from(v)));
        tags.push(mk("ColorTempFluorescent", 0xae, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x15e + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsKelvin", 0xaf, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x166, bo) {
        dm.push(("ColorTempKelvin".to_string(), f64::from(v)));
        tags.push(mk("ColorTempKelvin", 0xb3, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x168 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsFlash", 0xb4, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x170, bo) {
        dm.push(("ColorTempFlash".to_string(), f64::from(v)));
        tags.push(mk("ColorTempFlash", 0xb8, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x172 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x17a, bo) {
        dm.push(("ColorTempUnknown11".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x17c + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x184, bo) {
        dm.push(("ColorTempUnknown12".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x186 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x18e, bo) {
        dm.push(("ColorTempUnknown13".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x190 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x198, bo) {
        dm.push(("ColorTempUnknown14".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x19a + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1a2, bo) {
        dm.push(("ColorTempUnknown15".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1a4 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1ac, bo) {
        dm.push(("ColorTempUnknown16".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1ae + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1b6, bo) {
        dm.push(("ColorTempUnknown17".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1b8 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1c0, bo) {
        dm.push(("ColorTempUnknown18".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1c2 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1ca, bo) {
        dm.push(("ColorTempUnknown19".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1cc + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1d4, bo) {
        dm.push(("ColorTempUnknown20".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1d6 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1de, bo) {
        dm.push(("ColorTempUnknown21".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1e0 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1e8, bo) {
        dm.push(("ColorTempUnknown22".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1ea + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1f2, bo) {
        dm.push(("ColorTempUnknown23".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1f4 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1fc, bo) {
        dm.push(("ColorTempUnknown24".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1fe + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x206, bo) {
        dm.push(("ColorTempUnknown25".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x208 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x210, bo) {
        dm.push(("ColorTempUnknown26".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x212 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x21a, bo) {
        dm.push(("ColorTempUnknown27".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x21c + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x224, bo) {
        dm.push(("ColorTempUnknown28".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x226 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x22e, bo) {
        dm.push(("ColorTempUnknown29".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match u16_at(data, 0x2ae + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("PerChannelBlackLevel", 0x157, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x532, bo) {
        dm.push(("FlashOutput".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val >= 255 ? 255 : exp(($val-200)/16*log(2))", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val == 255 ? \"Strobe or Misfire\" : sprintf(\"%.0f%%\", $val * 100)", &cv, &ctx) { cv = x; }
        tags.push(mk("FlashOutput", 0x299, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = i16_at(data, 0x534, bo) {
        dm.push(("FlashBatteryLevel".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val ? sprintf(\"%.2fV\", $val * 5 / 186) : \"n/a\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FlashBatteryLevel", 0x29a, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x654, bo) {
        dm.push(("NormalWhiteLevel".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val || undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            tags.push(mk("NormalWhiteLevel", 0x32a, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u16_at(data, 0x656, bo) {
        dm.push(("SpecularWhiteLevel".to_string(), f64::from(v)));
        tags.push(mk("SpecularWhiteLevel", 0x32b, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x658, bo) {
        dm.push(("LinearityUpperMargin".to_string(), f64::from(v)));
        tags.push(mk("LinearityUpperMargin", 0x32c, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(sub) = data.get(0x230..0x230 + 120) {
        tags.extend(canon_colorcalib(sub, model, bo, file_type, format, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::ColorData11` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colordata11(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = i16_at(data, 0x0, bo) {
        dm.push(("ColorDataVersion".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let s = match v as i64 {
                34 => "34 (R3)".to_string(),
                48 => "48 (R7/R10/R50/R6mkII)".to_string(),
                other => other.to_string(),
            };
            tags.push(mk("ColorDataVersion", 0x0, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xd2 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsAsShot", 0x69, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0xda, bo) {
        dm.push(("ColorTempAsShot".to_string(), f64::from(v)));
        tags.push(mk("ColorTempAsShot", 0x6d, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xdc + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsAuto", 0x6e, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0xe4, bo) {
        dm.push(("ColorTempAuto".to_string(), f64::from(v)));
        tags.push(mk("ColorTempAuto", 0x72, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xe6 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsMeasured", 0x73, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0xee, bo) {
        dm.push(("ColorTempMeasured".to_string(), f64::from(v)));
        tags.push(mk("ColorTempMeasured", 0x77, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xf0 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xf8, bo) {
        dm.push(("ColorTempUnknown".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xfa + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x102, bo) {
        dm.push(("ColorTempUnknown2".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x104 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x10c, bo) {
        dm.push(("ColorTempUnknown3".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x10e + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x116, bo) {
        dm.push(("ColorTempUnknown4".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x118 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x120, bo) {
        dm.push(("ColorTempUnknown5".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x122 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x12a, bo) {
        dm.push(("ColorTempUnknown6".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x12c + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x134, bo) {
        dm.push(("ColorTempUnknown7".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x136 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x13e, bo) {
        dm.push(("ColorTempUnknown8".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x140 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x148, bo) {
        dm.push(("ColorTempUnknown9".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x14a + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x152, bo) {
        dm.push(("ColorTempUnknown10".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x154 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x15c, bo) {
        dm.push(("ColorTempUnknown11".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x15e + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x166, bo) {
        dm.push(("ColorTempUnknown12".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x168 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x170, bo) {
        dm.push(("ColorTempUnknown13".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x172 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x17a, bo) {
        dm.push(("ColorTempUnknown14".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x17c + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x184, bo) {
        dm.push(("ColorTempUnknown15".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x186 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x18e, bo) {
        dm.push(("ColorTempUnknown16".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x190 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x198, bo) {
        dm.push(("ColorTempUnknown17".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x19a + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsDaylight", 0xcd, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x1a2, bo) {
        dm.push(("ColorTempDaylight".to_string(), f64::from(v)));
        tags.push(mk("ColorTempDaylight", 0xd1, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1a4 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsShade", 0xd2, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x1ac, bo) {
        dm.push(("ColorTempShade".to_string(), f64::from(v)));
        tags.push(mk("ColorTempShade", 0xd6, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1ae + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsCloudy", 0xd7, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x1b6, bo) {
        dm.push(("ColorTempCloudy".to_string(), f64::from(v)));
        tags.push(mk("ColorTempCloudy", 0xdb, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1b8 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsTungsten", 0xdc, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x1c0, bo) {
        dm.push(("ColorTempTungsten".to_string(), f64::from(v)));
        tags.push(mk("ColorTempTungsten", 0xe0, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1c2 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsFluorescent", 0xe1, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x1ca, bo) {
        dm.push(("ColorTempFluorescent".to_string(), f64::from(v)));
        tags.push(mk("ColorTempFluorescent", 0xe5, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1cc + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsKelvin", 0xe6, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x1d4, bo) {
        dm.push(("ColorTempKelvin".to_string(), f64::from(v)));
        tags.push(mk("ColorTempKelvin", 0xea, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1d6 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsFlash", 0xeb, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x1de, bo) {
        dm.push(("ColorTempFlash".to_string(), f64::from(v)));
        tags.push(mk("ColorTempFlash", 0xef, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1e0 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1e8, bo) {
        dm.push(("ColorTempUnknown18".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1ea + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1f2, bo) {
        dm.push(("ColorTempUnknown19".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1f4 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1fc, bo) {
        dm.push(("ColorTempUnknown20".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1fe + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x206, bo) {
        dm.push(("ColorTempUnknown21".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x208 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x210, bo) {
        dm.push(("ColorTempUnknown22".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x212 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x21a, bo) {
        dm.push(("ColorTempUnknown23".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x21c + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x224, bo) {
        dm.push(("ColorTempUnknown24".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x226 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x22e, bo) {
        dm.push(("ColorTempUnknown25".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x230 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x238, bo) {
        dm.push(("ColorTempUnknown26".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x23a + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x242, bo) {
        dm.push(("ColorTempUnknown27".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x244 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x24c, bo) {
        dm.push(("ColorTempUnknown28".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match u16_at(data, 0x2d6 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("PerChannelBlackLevel", 0x16b, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u16_at(data, 0x500, bo) {
        dm.push(("NormalWhiteLevel".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val || undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            tags.push(mk("NormalWhiteLevel", 0x280, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u16_at(data, 0x502, bo) {
        dm.push(("SpecularWhiteLevel".to_string(), f64::from(v)));
        tags.push(mk("SpecularWhiteLevel", 0x281, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x504, bo) {
        dm.push(("LinearityUpperMargin".to_string(), f64::from(v)));
        tags.push(mk("LinearityUpperMargin", 0x282, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(sub) = data.get(0x258..0x258 + 120) {
        tags.extend(canon_colorcalib(sub, model, bo, file_type, format, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::ColorData12` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colordata12(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = i16_at(data, 0x0, bo) {
        dm.push(("ColorDataVersion".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let s = match v as i64 {
                64 => "64 (R1/R5mkII)".to_string(),
                65 => "65 (R50V)".to_string(),
                other => other.to_string(),
            };
            tags.push(mk("ColorDataVersion", 0x0, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xd2 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsAsShot", 0x69, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0xda, bo) {
        dm.push(("ColorTempAsShot".to_string(), f64::from(v)));
        tags.push(mk("ColorTempAsShot", 0x6d, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xdc + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsDaylight", 0x6e, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0xe4, bo) {
        dm.push(("ColorTempDaylight".to_string(), f64::from(v)));
        tags.push(mk("ColorTempDaylight", 0x72, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xe6 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsShade", 0x73, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0xee, bo) {
        dm.push(("ColorTempShade".to_string(), f64::from(v)));
        tags.push(mk("ColorTempShade", 0x77, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xf0 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsCloudy", 0x78, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0xf8, bo) {
        dm.push(("ColorTempCloudy".to_string(), f64::from(v)));
        tags.push(mk("ColorTempCloudy", 0x7c, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xfa + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsTungsten", 0x7d, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x102, bo) {
        dm.push(("ColorTempTungsten".to_string(), f64::from(v)));
        tags.push(mk("ColorTempTungsten", 0x81, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x104 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsFluorescent", 0x82, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x10c, bo) {
        dm.push(("ColorTempFluorescent".to_string(), f64::from(v)));
        tags.push(mk("ColorTempFluorescent", 0x86, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x10e + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsFlash", 0x87, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x116, bo) {
        dm.push(("ColorTempFlash".to_string(), f64::from(v)));
        tags.push(mk("ColorTempFlash", 0x8b, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x118 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x120, bo) {
        dm.push(("ColorTempUnknown2".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x122 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x12a, bo) {
        dm.push(("ColorTempUnknown3".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x12c + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x134, bo) {
        dm.push(("ColorTempUnknown4".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x136 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x13e, bo) {
        dm.push(("ColorTempUnknown5".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x140 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x148, bo) {
        dm.push(("ColorTempUnknown6".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x14a + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x152, bo) {
        dm.push(("ColorTempUnknown7".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x154 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x15c, bo) {
        dm.push(("ColorTempUnknown8".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x15e + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x166, bo) {
        dm.push(("ColorTempUnknown9".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x168 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x170, bo) {
        dm.push(("ColorTempUnknown10".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x172 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x17a, bo) {
        dm.push(("ColorTempUnknown11".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x17c + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x184, bo) {
        dm.push(("ColorTempUnknown12".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x186 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x18e, bo) {
        dm.push(("ColorTempUnknown13".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x190 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x198, bo) {
        dm.push(("ColorTempUnknown14".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x19a + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1a2, bo) {
        dm.push(("ColorTempUnknown15".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1a4 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1ac, bo) {
        dm.push(("ColorTempUnknown16".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1ae + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1b6, bo) {
        dm.push(("ColorTempUnknown17".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1b8 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1c0, bo) {
        dm.push(("ColorTempUnknown18".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1c2 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1ca, bo) {
        dm.push(("ColorTempUnknown19".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1cc + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1d4, bo) {
        dm.push(("ColorTempUnknown20".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1d6 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1de, bo) {
        dm.push(("ColorTempUnknown21".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1e0 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1e8, bo) {
        dm.push(("ColorTempUnknown22".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1ea + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1f2, bo) {
        dm.push(("ColorTempUnknown23".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1f4 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1fc, bo) {
        dm.push(("ColorTempUnknown24".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1fe + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x206, bo) {
        dm.push(("ColorTempUnknown25".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x208 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x210, bo) {
        dm.push(("ColorTempUnknown26".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x212 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x21a, bo) {
        dm.push(("ColorTempUnknown27".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x21c + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x224, bo) {
        dm.push(("ColorTempUnknown28".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x226 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x22e, bo) {
        dm.push(("ColorTempUnknown29".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x230 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x238, bo) {
        dm.push(("ColorTempUnknown30".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x23a + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x242, bo) {
        dm.push(("ColorTempUnknown31".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x244 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x24c, bo) {
        dm.push(("ColorTempUnknown32".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x24e + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x256, bo) {
        dm.push(("ColorTempUnknown33".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match u16_at(data, 0x2fe + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("PerChannelBlackLevel", 0x17f, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x406, bo) {
        dm.push(("FlashOutput".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val >= 255 ? 255 : exp(($val-200)/16*log(2))", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val == 255 ? \"Strobe or Misfire\" : sprintf(\"%.0f%%\", $val * 100)", &cv, &ctx) { cv = x; }
        tags.push(mk("FlashOutput", 0x203, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = i16_at(data, 0x408, bo) {
        dm.push(("FlashBatteryLevel".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val ? sprintf(\"%.2fV\", $val * 5 / 186) : \"n/a\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FlashBatteryLevel", 0x204, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x528, bo) {
        dm.push(("NormalWhiteLevel".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val || undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            tags.push(mk("NormalWhiteLevel", 0x294, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u16_at(data, 0x52a, bo) {
        dm.push(("SpecularWhiteLevel".to_string(), f64::from(v)));
        tags.push(mk("SpecularWhiteLevel", 0x295, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x52c, bo) {
        dm.push(("LinearityUpperMargin".to_string(), f64::from(v)));
        tags.push(mk("LinearityUpperMargin", 0x296, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(sub) = data.get(0x280..0x280 + 120) {
        tags.extend(canon_colorcalib(sub, model, bo, file_type, format, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::ColorDataUnknown` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colordataunknown(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = i16_at(data, 0x0, bo) {
        dm.push(("ColorDataVersion".to_string(), f64::from(v)));
        tags.push(mk("ColorDataVersion", 0x0, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    tags
}

/// `Image::ExifTool::Canon::ShotInfo` -- FORMAT int16s, FIRST_ENTRY 1.
fn canon_shotinfo(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Image";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = i16_at(data, 0x2, bo) {
        dm.push(("AutoISO".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("exp($val/32*log(2))*100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("sprintf(\"%.0f\",$val)", &cv, &ctx) { cv = x; }
        tags.push(mk("AutoISO", 0x1, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = i16_at(data, 0x4, bo) {
        dm.push(("BaseISO".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp($val/32*log(2))*100/32", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("sprintf(\"%.0f\",$val)", &cv, &ctx) { cv = x; }
            tags.push(mk("BaseISO", 0x2, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x6, bo) {
        dm.push(("MeasuredEV".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 32 + 5", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("sprintf(\"%.2f\",$val)", &cv, &ctx) { cv = x; }
        tags.push(mk("MeasuredEV", 0x3, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = i16_at(data, 0x8, bo) {
        dm.push(("TargetAperture".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val > 0 ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(Image::ExifTool::Canon::CanonEv($val)*log(2)/2)", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("sprintf(\"%.2g\",$val)", &cv, &ctx) { cv = x; }
            tags.push(mk("TargetAperture", 0x4, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0xa, bo) {
        dm.push(("TargetExposureTime".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("($val > -1000 and ($val or $$self{Model}=~/(EOS|PowerShot|IXUS|IXY)/))? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(-Image::ExifTool::Canon::CanonEv($val)*log(2))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("Image::ExifTool::Exif::PrintExposureTime($val)", &cv, &ctx) { cv = x; }
            tags.push(mk("TargetExposureTime", 0x5, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0xc, bo) {
        dm.push(("ExposureCompensation".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("Image::ExifTool::Canon::CanonEv($val)", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("Image::ExifTool::Exif::PrintFraction($val)", &cv, &ctx) { cv = x; }
        tags.push(mk("ExposureCompensation", 0x6, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = i16_at(data, 0xe, bo) {
        dm.push(("WhiteBalance".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Auto".to_string(),
            1 => "Daylight".to_string(),
            2 => "Cloudy".to_string(),
            3 => "Tungsten".to_string(),
            4 => "Fluorescent".to_string(),
            5 => "Flash".to_string(),
            6 => "Custom".to_string(),
            7 => "Black & White".to_string(),
            8 => "Shade".to_string(),
            9 => "Manual Temperature (Kelvin)".to_string(),
            10 => "PC Set1".to_string(),
            11 => "PC Set2".to_string(),
            12 => "PC Set3".to_string(),
            14 => "Daylight Fluorescent".to_string(),
            15 => "Custom 1".to_string(),
            16 => "Custom 2".to_string(),
            17 => "Underwater".to_string(),
            18 => "Custom 3".to_string(),
            19 => "Custom 4".to_string(),
            20 => "PC Set4".to_string(),
            21 => "PC Set5".to_string(),
            23 => "Auto (ambience priority)".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("WhiteBalance", 0x7, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i16_at(data, 0x10, bo) {
        dm.push(("SlowShutter".to_string(), f64::from(v)));
        let s = match v as i64 {
            -1 => "n/a".to_string(),
            0 => "Off".to_string(),
            1 => "Night Scene".to_string(),
            2 => "On".to_string(),
            3 => "None".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SlowShutter", 0x8, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i16_at(data, 0x12, bo) {
        dm.push(("SequenceNumber".to_string(), f64::from(v)));
        tags.push(mk("SequenceNumber", 0x9, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i16_at(data, 0x14, bo) {
        dm.push(("OpticalZoomCode".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val == 8 ? \"n/a\" : $val", &cv, &ctx) { cv = x; }
        tags.push(mk("OpticalZoomCode", 0xa, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if (MODEL_RE_0.is_match(model) && !MODEL_RE_1.is_match(model)) {
        if let Some(v) = i16_at(data, 0x18, bo) {
            dm.push(("CameraTemperature".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
            if rc.as_ref() != Some(&Conv::Undef) {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let v = rc.map_or(v, |x| x.as_num() as _);
                let mut cv = Conv::Num(f64::from(v));
                if let Some(x) = conv_expr::eval_with("$val - 128", &cv, &ctx) { cv = x; }
                let raw = Value::F64(cv.as_num());
                if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
                tags.push(mk("CameraTemperature", 0xc, cv.as_string(), raw, GRP1, GRP2, PRIO));
            }
        }
    }
    if let Some(v) = i16_at(data, 0x1a, bo) {
        dm.push(("FlashGuideNumber".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val==-1 ? undef : $val", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("$val / 32", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            tags.push(mk("FlashGuideNumber", 0xd, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x1c, bo) {
        dm.push(("AFPointsInFocus".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val==0 ? undef : $val", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let s = match v as i64 {
                12288 => "None (MF)".to_string(),
                12289 => "Right".to_string(),
                12290 => "Center".to_string(),
                12291 => "Center+Right".to_string(),
                12292 => "Left".to_string(),
                12293 => "Left+Right".to_string(),
                12294 => "Left+Center".to_string(),
                12295 => "All".to_string(),
                other => other.to_string(),
            };
            tags.push(mk("AFPointsInFocus", 0xe, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x1e, bo) {
        dm.push(("FlashExposureComp".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("Image::ExifTool::Canon::CanonEv($val)", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("Image::ExifTool::Exif::PrintFraction($val)", &cv, &ctx) { cv = x; }
        tags.push(mk("FlashExposureComp", 0xf, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = i16_at(data, 0x20, bo) {
        dm.push(("AutoExposureBracketing".to_string(), f64::from(v)));
        let s = match v as i64 {
            -1 => "On".to_string(),
            0 => "Off".to_string(),
            1 => "On (shot 1)".to_string(),
            2 => "On (shot 2)".to_string(),
            3 => "On (shot 3)".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("AutoExposureBracketing", 0x10, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i16_at(data, 0x22, bo) {
        dm.push(("AEBBracketValue".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("Image::ExifTool::Canon::CanonEv($val)", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("Image::ExifTool::Exif::PrintFraction($val)", &cv, &ctx) { cv = x; }
        tags.push(mk("AEBBracketValue", 0x11, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = i16_at(data, 0x24, bo) {
        dm.push(("ControlMode".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "n/a".to_string(),
            1 => "Camera Local Control".to_string(),
            3 => "Computer Remote Control".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ControlMode", 0x12, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x26, bo) {
        dm.push(("FocusDistanceUpper".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("($val) || undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
            tags.push(mk("FocusDistanceUpper", 0x13, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if dm_get(dm, "FocusDistanceUpper").is_some_and(|v| v != 0.0) {
        if let Some(v) = u16_at(data, 0x28, bo) {
            dm.push(("FocusDistanceLower".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
            tags.push(mk("FocusDistanceLower", 0x14, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x2a, bo) {
        dm.push(("FNumber".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(Image::ExifTool::Canon::CanonEv($val)*log(2)/2)", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("sprintf(\"%.2g\",$val)", &cv, &ctx) { cv = x; }
            tags.push(mk("FNumber", 0x15, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if MODEL_RE_2.is_match(model) {
        if let Some(v) = i16_at(data, 0x2c, bo) {
            dm.push(("ExposureTime".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let rc = conv_expr::eval_with("($val or $$self{FILE_TYPE} eq \"CRW\") ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
            if rc.as_ref() != Some(&Conv::Undef) {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let v = rc.map_or(v, |x| x.as_num() as _);
                let mut cv = Conv::Num(f64::from(v));
                if let Some(x) = conv_expr::eval_with("exp(-Image::ExifTool::Canon::CanonEv($val)*log(2))*1000/32", &cv, &ctx) { cv = x; }
                let raw = Value::F64(cv.as_num());
                if let Some(x) = conv_expr::eval_with("Image::ExifTool::Exif::PrintExposureTime($val)", &cv, &ctx) { cv = x; }
                tags.push(mk("ExposureTime", 0x16, cv.as_string(), raw, GRP1, GRP2, PRIO));
            }
        }
    }
    if !(MODEL_RE_2.is_match(model)) {
        if let Some(v) = i16_at(data, 0x2c, bo) {
            dm.push(("ExposureTime".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let rc = conv_expr::eval_with("($val or $$self{FILE_TYPE} eq \"CRW\") ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
            if rc.as_ref() != Some(&Conv::Undef) {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let v = rc.map_or(v, |x| x.as_num() as _);
                let mut cv = Conv::Num(f64::from(v));
                if let Some(x) = conv_expr::eval_with("exp(-Image::ExifTool::Canon::CanonEv($val)*log(2))", &cv, &ctx) { cv = x; }
                let raw = Value::F64(cv.as_num());
                if let Some(x) = conv_expr::eval_with("Image::ExifTool::Exif::PrintExposureTime($val)", &cv, &ctx) { cv = x; }
                tags.push(mk("ExposureTime", 0x16, cv.as_string(), raw, GRP1, GRP2, PRIO));
            }
        }
    }
    if let Some(v) = i16_at(data, 0x2e, bo) {
        dm.push(("MeasuredEV2".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("$val / 8 - 6", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            tags.push(mk("MeasuredEV2", 0x17, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x30, bo) {
        dm.push(("BulbDuration".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 10", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("BulbDuration", 0x18, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = i16_at(data, 0x34, bo) {
        dm.push(("CameraType".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "n/a".to_string(),
            248 => "EOS High-end".to_string(),
            250 => "Compact".to_string(),
            252 => "EOS Mid-range".to_string(),
            255 => "DV Camera".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("CameraType", 0x1a, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i16_at(data, 0x36, bo) {
        dm.push(("AutoRotate".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val >= 0 ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let s = match v as i64 {
                -1 => "n/a".to_string(),
                0 => "None".to_string(),
                1 => "Rotate 90 CW".to_string(),
                2 => "Rotate 180".to_string(),
                3 => "Rotate 270 CW".to_string(),
                other => other.to_string(),
            };
            tags.push(mk("AutoRotate", 0x1b, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x38, bo) {
        dm.push(("NDFilter".to_string(), f64::from(v)));
        let s = match v as i64 {
            -1 => "n/a".to_string(),
            0 => "Off".to_string(),
            1 => "On".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("NDFilter", 0x1c, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i16_at(data, 0x3a, bo) {
        dm.push(("SelfTimer2".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val >= 0 ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("$val / 10", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            tags.push(mk("SelfTimer2", 0x1d, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x42, bo) {
        dm.push(("FlashOutput".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("($$self{Model}=~/(PowerShot|IXUS|IXY)/ or $val) ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            tags.push(mk("FlashOutput", 0x21, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    tags
}

/// `Image::ExifTool::Canon::CameraInfo1D` -- FORMAT int8u, FIRST_ENTRY 0.
fn canon_camerainfo1d(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = u8_at(data, 0x4) {
        dm.push(("ExposureTime".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(4*log(2)*(1-Image::ExifTool::Canon::CanonEv($val-24)))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("Image::ExifTool::Exif::PrintExposureTime($val)", &cv, &ctx) { cv = x; }
            tags.push(mk("ExposureTime", 0x4, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u16_at(data, 0xa, bo) {
        dm.push(("FocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
            tags.push(mk("FocalLength", 0xa, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u16rev_at(data, 0xd, bo) {
        dm.push(("LensType".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let s = match v as i64 {
                -1 => "n/a".to_string(),
                1 => "Canon EF 50mm f/1.8".to_string(),
                2 => "Canon EF 28mm f/2.8 or Sigma Lens".to_string(),
                3 => "Canon EF 135mm f/2.8 Soft".to_string(),
                4 => "Canon EF 35-105mm f/3.5-4.5 or Sigma Lens".to_string(),
                5 => "Canon EF 35-70mm f/3.5-4.5".to_string(),
                6 => "Canon EF 28-70mm f/3.5-4.5 or Sigma or Tokina Lens".to_string(),
                7 => "Canon EF 100-300mm f/5.6L".to_string(),
                8 => "Canon EF 100-300mm f/5.6 or Sigma or Tokina Lens".to_string(),
                9 => "Canon EF 70-210mm f/4".to_string(),
                10 => "Canon EF 50mm f/2.5 Macro or Sigma Lens".to_string(),
                11 => "Canon EF 35mm f/2".to_string(),
                13 => "Canon EF 15mm f/2.8 Fisheye".to_string(),
                14 => "Canon EF 50-200mm f/3.5-4.5L".to_string(),
                15 => "Canon EF 50-200mm f/3.5-4.5".to_string(),
                16 => "Canon EF 35-135mm f/3.5-4.5".to_string(),
                17 => "Canon EF 35-70mm f/3.5-4.5A".to_string(),
                18 => "Canon EF 28-70mm f/3.5-4.5".to_string(),
                20 => "Canon EF 100-200mm f/4.5A".to_string(),
                21 => "Canon EF 80-200mm f/2.8L".to_string(),
                22 => "Canon EF 20-35mm f/2.8L or Tokina Lens".to_string(),
                23 => "Canon EF 35-105mm f/3.5-4.5".to_string(),
                24 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
                25 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
                26 => "Canon EF 100mm f/2.8 Macro or Other Lens".to_string(),
                27 => "Canon EF 35-80mm f/4-5.6".to_string(),
                28 => "Canon EF 80-200mm f/4.5-5.6 or Tamron Lens".to_string(),
                29 => "Canon EF 50mm f/1.8 II".to_string(),
                30 => "Canon EF 35-105mm f/4.5-5.6".to_string(),
                31 => "Canon EF 75-300mm f/4-5.6 or Tamron Lens".to_string(),
                32 => "Canon EF 24mm f/2.8 or Sigma Lens".to_string(),
                33 => "Voigtlander or Carl Zeiss Lens".to_string(),
                35 => "Canon EF 35-80mm f/4-5.6".to_string(),
                36 => "Canon EF 38-76mm f/4.5-5.6".to_string(),
                37 => "Canon EF 35-80mm f/4-5.6 or Tamron Lens".to_string(),
                38 => "Canon EF 80-200mm f/4.5-5.6 II".to_string(),
                39 => "Canon EF 75-300mm f/4-5.6".to_string(),
                40 => "Canon EF 28-80mm f/3.5-5.6".to_string(),
                41 => "Canon EF 28-90mm f/4-5.6".to_string(),
                42 => "Canon EF 28-200mm f/3.5-5.6 or Tamron Lens".to_string(),
                43 => "Canon EF 28-105mm f/4-5.6".to_string(),
                44 => "Canon EF 90-300mm f/4.5-5.6".to_string(),
                45 => "Canon EF-S 18-55mm f/3.5-5.6 [II]".to_string(),
                46 => "Canon EF 28-90mm f/4-5.6".to_string(),
                47 => "Zeiss Milvus 35mm f/2 or 50mm f/2".to_string(),
                48 => "Canon EF-S 18-55mm f/3.5-5.6 IS".to_string(),
                49 => "Canon EF-S 55-250mm f/4-5.6 IS".to_string(),
                50 => "Canon EF-S 18-200mm f/3.5-5.6 IS".to_string(),
                51 => "Canon EF-S 18-135mm f/3.5-5.6 IS".to_string(),
                52 => "Canon EF-S 18-55mm f/3.5-5.6 IS II".to_string(),
                53 => "Canon EF-S 18-55mm f/3.5-5.6 III".to_string(),
                54 => "Canon EF-S 55-250mm f/4-5.6 IS II".to_string(),
                60 => "Irix 11mm f/4 or 15mm f/2.4".to_string(),
                63 => "Irix 30mm F1.4 Dragonfly".to_string(),
                80 => "Canon TS-E 50mm f/2.8L Macro".to_string(),
                81 => "Canon TS-E 90mm f/2.8L Macro".to_string(),
                82 => "Canon TS-E 135mm f/4L Macro".to_string(),
                94 => "Canon TS-E 17mm f/4L".to_string(),
                95 => "Canon TS-E 24mm f/3.5L II".to_string(),
                103 => "Samyang AF 14mm f/2.8 EF or Rokinon Lens".to_string(),
                106 => "Rokinon SP / Samyang XP 35mm f/1.2".to_string(),
                112 => "Sigma 28mm f/1.5 FF High-speed Prime or other Sigma Lens".to_string(),
                117 => "Tamron 35-150mm f/2.8-4.0 Di VC OSD (A043) or other Tamron Lens".to_string(),
                124 => "Canon MP-E 65mm f/2.8 1-5x Macro Photo".to_string(),
                125 => "Canon TS-E 24mm f/3.5L".to_string(),
                126 => "Canon TS-E 45mm f/2.8".to_string(),
                127 => "Canon TS-E 90mm f/2.8 or Tamron Lens".to_string(),
                129 => "Canon EF 300mm f/2.8L USM".to_string(),
                130 => "Canon EF 50mm f/1.0L USM".to_string(),
                131 => "Canon EF 28-80mm f/2.8-4L USM or Sigma Lens".to_string(),
                132 => "Canon EF 1200mm f/5.6L USM".to_string(),
                134 => "Canon EF 600mm f/4L IS USM".to_string(),
                135 => "Canon EF 200mm f/1.8L USM".to_string(),
                136 => "Canon EF 300mm f/2.8L USM".to_string(),
                137 => "Canon EF 85mm f/1.2L USM or Sigma or Tamron Lens".to_string(),
                138 => "Canon EF 28-80mm f/2.8-4L".to_string(),
                139 => "Canon EF 400mm f/2.8L USM".to_string(),
                140 => "Canon EF 500mm f/4.5L USM".to_string(),
                141 => "Canon EF 500mm f/4.5L USM".to_string(),
                142 => "Canon EF 300mm f/2.8L IS USM".to_string(),
                143 => "Canon EF 500mm f/4L IS USM or Sigma Lens".to_string(),
                144 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
                145 => "Canon EF 100-300mm f/4.5-5.6 USM".to_string(),
                146 => "Canon EF 70-210mm f/3.5-4.5 USM".to_string(),
                147 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
                148 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
                149 => "Canon EF 100mm f/2 USM".to_string(),
                150 => "Canon EF 14mm f/2.8L USM or Sigma Lens".to_string(),
                151 => "Canon EF 200mm f/2.8L USM".to_string(),
                152 => "Canon EF 300mm f/4L IS USM or Sigma Lens".to_string(),
                153 => "Canon EF 35-350mm f/3.5-5.6L USM or Sigma or Tamron Lens".to_string(),
                154 => "Canon EF 20mm f/2.8 USM or Zeiss Lens".to_string(),
                155 => "Canon EF 85mm f/1.8 USM or Sigma Lens".to_string(),
                156 => "Canon EF 28-105mm f/3.5-4.5 USM or Tamron Lens".to_string(),
                160 => "Canon EF 20-35mm f/3.5-4.5 USM or Tamron or Tokina Lens".to_string(),
                161 => "Canon EF 28-70mm f/2.8L USM or Other Lens".to_string(),
                162 => "Canon EF 200mm f/2.8L USM".to_string(),
                163 => "Canon EF 300mm f/4L".to_string(),
                164 => "Canon EF 400mm f/5.6L".to_string(),
                165 => "Canon EF 70-200mm f/2.8L USM".to_string(),
                166 => "Canon EF 70-200mm f/2.8L USM + 1.4x".to_string(),
                167 => "Canon EF 70-200mm f/2.8L USM + 2x".to_string(),
                168 => "Canon EF 28mm f/1.8 USM or Sigma Lens".to_string(),
                169 => "Canon EF 17-35mm f/2.8L USM or Sigma Lens".to_string(),
                170 => "Canon EF 200mm f/2.8L II USM or Sigma Lens".to_string(),
                171 => "Canon EF 300mm f/4L USM".to_string(),
                172 => "Canon EF 400mm f/5.6L USM or Sigma Lens".to_string(),
                173 => "Canon EF 180mm Macro f/3.5L USM or Sigma Lens".to_string(),
                174 => "Canon EF 135mm f/2L USM or Other Lens".to_string(),
                175 => "Canon EF 400mm f/2.8L USM".to_string(),
                176 => "Canon EF 24-85mm f/3.5-4.5 USM".to_string(),
                177 => "Canon EF 300mm f/4L IS USM".to_string(),
                178 => "Canon EF 28-135mm f/3.5-5.6 IS".to_string(),
                179 => "Canon EF 24mm f/1.4L USM".to_string(),
                180 => "Canon EF 35mm f/1.4L USM or Other Lens".to_string(),
                181 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 1.4x or Sigma Lens".to_string(),
                182 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 2x or Sigma Lens".to_string(),
                183 => "Canon EF 100-400mm f/4.5-5.6L IS USM or Sigma Lens".to_string(),
                184 => "Canon EF 400mm f/2.8L USM + 2x".to_string(),
                185 => "Canon EF 600mm f/4L IS USM".to_string(),
                186 => "Canon EF 70-200mm f/4L USM".to_string(),
                187 => "Canon EF 70-200mm f/4L USM + 1.4x".to_string(),
                188 => "Canon EF 70-200mm f/4L USM + 2x".to_string(),
                189 => "Canon EF 70-200mm f/4L USM + 2.8x".to_string(),
                190 => "Canon EF 100mm f/2.8 Macro USM".to_string(),
                191 => "Canon EF 400mm f/4 DO IS or Sigma Lens".to_string(),
                193 => "Canon EF 35-80mm f/4-5.6 USM".to_string(),
                194 => "Canon EF 80-200mm f/4.5-5.6 USM".to_string(),
                195 => "Canon EF 35-105mm f/4.5-5.6 USM".to_string(),
                196 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
                197 => "Canon EF 75-300mm f/4-5.6 IS USM or Sigma Lens".to_string(),
                198 => "Canon EF 50mm f/1.4 USM or Other Lens".to_string(),
                199 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
                200 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
                201 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
                202 => "Canon EF 28-80mm f/3.5-5.6 USM IV".to_string(),
                208 => "Canon EF 22-55mm f/4-5.6 USM".to_string(),
                209 => "Canon EF 55-200mm f/4.5-5.6".to_string(),
                210 => "Canon EF 28-90mm f/4-5.6 USM".to_string(),
                211 => "Canon EF 28-200mm f/3.5-5.6 USM".to_string(),
                212 => "Canon EF 28-105mm f/4-5.6 USM".to_string(),
                213 => "Canon EF 90-300mm f/4.5-5.6 USM or Tamron Lens".to_string(),
                214 => "Canon EF-S 18-55mm f/3.5-5.6 USM".to_string(),
                215 => "Canon EF 55-200mm f/4.5-5.6 II USM".to_string(),
                217 => "Tamron AF 18-270mm f/3.5-6.3 Di II VC PZD".to_string(),
                220 => "Yongnuo YN 50mm f/1.8".to_string(),
                224 => "Canon EF 70-200mm f/2.8L IS USM".to_string(),
                225 => "Canon EF 70-200mm f/2.8L IS USM + 1.4x".to_string(),
                226 => "Canon EF 70-200mm f/2.8L IS USM + 2x".to_string(),
                227 => "Canon EF 70-200mm f/2.8L IS USM + 2.8x".to_string(),
                228 => "Canon EF 28-105mm f/3.5-4.5 USM".to_string(),
                229 => "Canon EF 16-35mm f/2.8L USM".to_string(),
                230 => "Canon EF 24-70mm f/2.8L USM".to_string(),
                231 => "Canon EF 17-40mm f/4L USM or Sigma Lens".to_string(),
                232 => "Canon EF 70-300mm f/4.5-5.6 DO IS USM".to_string(),
                233 => "Canon EF 28-300mm f/3.5-5.6L IS USM".to_string(),
                234 => "Canon EF-S 17-85mm f/4-5.6 IS USM or Tokina Lens".to_string(),
                235 => "Canon EF-S 10-22mm f/3.5-4.5 USM".to_string(),
                236 => "Canon EF-S 60mm f/2.8 Macro USM".to_string(),
                237 => "Canon EF 24-105mm f/4L IS USM".to_string(),
                238 => "Canon EF 70-300mm f/4-5.6 IS USM".to_string(),
                239 => "Canon EF 85mm f/1.2L II USM or Rokinon Lens".to_string(),
                240 => "Canon EF-S 17-55mm f/2.8 IS USM or Sigma Lens".to_string(),
                241 => "Canon EF 50mm f/1.2L USM".to_string(),
                242 => "Canon EF 70-200mm f/4L IS USM".to_string(),
                243 => "Canon EF 70-200mm f/4L IS USM + 1.4x".to_string(),
                244 => "Canon EF 70-200mm f/4L IS USM + 2x".to_string(),
                245 => "Canon EF 70-200mm f/4L IS USM + 2.8x".to_string(),
                246 => "Canon EF 16-35mm f/2.8L II USM".to_string(),
                247 => "Canon EF 14mm f/2.8L II USM".to_string(),
                248 => "Canon EF 200mm f/2L IS USM or Sigma Lens".to_string(),
                249 => "Canon EF 800mm f/5.6L IS USM".to_string(),
                250 => "Canon EF 24mm f/1.4L II USM or Sigma Lens".to_string(),
                251 => "Canon EF 70-200mm f/2.8L IS II USM".to_string(),
                252 => "Canon EF 70-200mm f/2.8L IS II USM + 1.4x".to_string(),
                253 => "Canon EF 70-200mm f/2.8L IS II USM + 2x".to_string(),
                254 => "Canon EF 100mm f/2.8L Macro IS USM or Tamron Lens".to_string(),
                255 => "Sigma 24-105mm f/4 DG OS HSM | A or Other Lens".to_string(),
                368 => "Sigma 14-24mm f/2.8 DG HSM | A or other Sigma Lens".to_string(),
                488 => "Canon EF-S 15-85mm f/3.5-5.6 IS USM".to_string(),
                489 => "Canon EF 70-300mm f/4-5.6L IS USM".to_string(),
                490 => "Canon EF 8-15mm f/4L Fisheye USM".to_string(),
                491 => "Canon EF 300mm f/2.8L IS II USM or Tamron Lens".to_string(),
                492 => "Canon EF 400mm f/2.8L IS II USM".to_string(),
                493 => "Canon EF 500mm f/4L IS II USM or EF 24-105mm f4L IS USM".to_string(),
                494 => "Canon EF 600mm f/4L IS II USM".to_string(),
                495 => "Canon EF 24-70mm f/2.8L II USM or Sigma Lens".to_string(),
                496 => "Canon EF 200-400mm f/4L IS USM".to_string(),
                499 => "Canon EF 200-400mm f/4L IS USM + 1.4x".to_string(),
                502 => "Canon EF 28mm f/2.8 IS USM or Tamron Lens".to_string(),
                503 => "Canon EF 24mm f/2.8 IS USM".to_string(),
                504 => "Canon EF 24-70mm f/4L IS USM".to_string(),
                505 => "Canon EF 35mm f/2 IS USM".to_string(),
                506 => "Canon EF 400mm f/4 DO IS II USM".to_string(),
                507 => "Canon EF 16-35mm f/4L IS USM".to_string(),
                508 => "Canon EF 11-24mm f/4L USM or Tamron Lens".to_string(),
                624 => "Sigma 70-200mm f/2.8 DG OS HSM | S or other Sigma Lens".to_string(),
                747 => "Canon EF 100-400mm f/4.5-5.6L IS II USM or Tamron Lens".to_string(),
                748 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 1.4x or Tamron Lens".to_string(),
                749 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 2x or Tamron Lens".to_string(),
                750 => "Canon EF 35mm f/1.4L II USM or Tamron Lens".to_string(),
                751 => "Canon EF 16-35mm f/2.8L III USM".to_string(),
                752 => "Canon EF 24-105mm f/4L IS II USM".to_string(),
                753 => "Canon EF 85mm f/1.4L IS USM".to_string(),
                754 => "Canon EF 70-200mm f/4L IS II USM".to_string(),
                757 => "Canon EF 400mm f/2.8L IS III USM".to_string(),
                758 => "Canon EF 600mm f/4L IS III USM".to_string(),
                923 => "Meike/SKY 85mm f/1.8 DCM".to_string(),
                1136 => "Sigma 24-70mm f/2.8 DG OS HSM | A".to_string(),
                4142 => "Canon EF-S 18-135mm f/3.5-5.6 IS STM".to_string(),
                4143 => "Canon EF-M 18-55mm f/3.5-5.6 IS STM or Tamron Lens".to_string(),
                4144 => "Canon EF 40mm f/2.8 STM".to_string(),
                4145 => "Canon EF-M 22mm f/2 STM".to_string(),
                4146 => "Canon EF-S 18-55mm f/3.5-5.6 IS STM".to_string(),
                4147 => "Canon EF-M 11-22mm f/4-5.6 IS STM".to_string(),
                4148 => "Canon EF-S 55-250mm f/4-5.6 IS STM".to_string(),
                4149 => "Canon EF-M 55-200mm f/4.5-6.3 IS STM".to_string(),
                4150 => "Canon EF-S 10-18mm f/4.5-5.6 IS STM".to_string(),
                4152 => "Canon EF 24-105mm f/3.5-5.6 IS STM".to_string(),
                4153 => "Canon EF-M 15-45mm f/3.5-6.3 IS STM".to_string(),
                4154 => "Canon EF-S 24mm f/2.8 STM".to_string(),
                4155 => "Canon EF-M 28mm f/3.5 Macro IS STM".to_string(),
                4156 => "Canon EF 50mm f/1.8 STM".to_string(),
                4157 => "Canon EF-M 18-150mm f/3.5-6.3 IS STM".to_string(),
                4158 => "Canon EF-S 18-55mm f/4-5.6 IS STM".to_string(),
                4159 => "Canon EF-M 32mm f/1.4 STM".to_string(),
                4160 => "Canon EF-S 35mm f/2.8 Macro IS STM".to_string(),
                4208 => "Sigma 56mm f/1.4 DC DN | C or other Sigma Lens".to_string(),
                4976 => "Sigma 16-300mm F3.5-6.7 DC OS | C (025)".to_string(),
                6512 => "Sigma 12mm F1.4 DC | C".to_string(),
                36910 => "Canon EF 70-300mm f/4-5.6 IS II USM".to_string(),
                36912 => "Canon EF-S 18-135mm f/3.5-5.6 IS USM".to_string(),
                61182 => "Canon RF 50mm F1.2L USM or other Canon RF Lens".to_string(),
                61491 => "Canon CN-E 14mm T3.1 L F".to_string(),
                61492 => "Canon CN-E 24mm T1.5 L F".to_string(),
                61494 => "Canon CN-E 85mm T1.3 L F".to_string(),
                61495 => "Canon CN-E 135mm T2.2 L F".to_string(),
                61496 => "Canon CN-E 35mm T1.5 L F".to_string(),
                65535 => "n/a".to_string(),
                other => other.to_string(),
            };
            tags.push(mk("LensType", 0xd, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u16_at(data, 0xe, bo) {
        dm.push(("MinFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MinFocalLength", 0xe, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x10, bo) {
        dm.push(("MaxFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MaxFocalLength", 0x10, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if MODEL_RE_3.is_match(model) {
        if let Some(v) = u8_at(data, 0x41) {
            dm.push(("SharpnessFrequency".to_string(), f64::from(v)));
            let s = match v as i64 {
                0 => "n/a".to_string(),
                1 => "Lowest".to_string(),
                2 => "Low".to_string(),
                3 => "Standard".to_string(),
                4 => "High".to_string(),
                5 => "Highest".to_string(),
                other => other.to_string(),
            };
            tags.push(mk("SharpnessFrequency", 0x41, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if MODEL_RE_3.is_match(model) {
        if let Some(v) = i8_at(data, 0x42) {
            dm.push(("Sharpness".to_string(), f64::from(v)));
            tags.push(mk("Sharpness", 0x42, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if MODEL_RE_3.is_match(model) {
        if let Some(v) = u8_at(data, 0x44) {
            dm.push(("WhiteBalance".to_string(), f64::from(v)));
            let s = match v as i64 {
                0 => "Auto".to_string(),
                1 => "Daylight".to_string(),
                2 => "Cloudy".to_string(),
                3 => "Tungsten".to_string(),
                4 => "Fluorescent".to_string(),
                5 => "Flash".to_string(),
                6 => "Custom".to_string(),
                7 => "Black & White".to_string(),
                8 => "Shade".to_string(),
                9 => "Manual Temperature (Kelvin)".to_string(),
                10 => "PC Set1".to_string(),
                11 => "PC Set2".to_string(),
                12 => "PC Set3".to_string(),
                14 => "Daylight Fluorescent".to_string(),
                15 => "Custom 1".to_string(),
                16 => "Custom 2".to_string(),
                17 => "Underwater".to_string(),
                18 => "Custom 3".to_string(),
                19 => "Custom 4".to_string(),
                20 => "PC Set4".to_string(),
                21 => "PC Set5".to_string(),
                23 => "Auto (ambience priority)".to_string(),
                other => other.to_string(),
            };
            tags.push(mk("WhiteBalance", 0x44, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if MODEL_RE_4.is_match(model) {
        if let Some(v) = u8_at(data, 0x47) {
            dm.push(("SharpnessFrequency".to_string(), f64::from(v)));
            let s = match v as i64 {
                0 => "n/a".to_string(),
                1 => "Lowest".to_string(),
                2 => "Low".to_string(),
                3 => "Standard".to_string(),
                4 => "High".to_string(),
                5 => "Highest".to_string(),
                other => other.to_string(),
            };
            tags.push(mk("SharpnessFrequency", 0x47, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if MODEL_RE_3.is_match(model) {
        if let Some(v) = u16_at(data, 0x48, bo) {
            dm.push(("ColorTemperature".to_string(), f64::from(v)));
            tags.push(mk("ColorTemperature", 0x48, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if !(MODEL_RE_3.is_match(model)) && MODEL_RE_4.is_match(model) {
        if let Some(v) = i8_at(data, 0x48) {
            dm.push(("Sharpness".to_string(), f64::from(v)));
            tags.push(mk("Sharpness", 0x48, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if MODEL_RE_4.is_match(model) {
        if let Some(v) = u8_at(data, 0x4a) {
            dm.push(("WhiteBalance".to_string(), f64::from(v)));
            let s = match v as i64 {
                0 => "Auto".to_string(),
                1 => "Daylight".to_string(),
                2 => "Cloudy".to_string(),
                3 => "Tungsten".to_string(),
                4 => "Fluorescent".to_string(),
                5 => "Flash".to_string(),
                6 => "Custom".to_string(),
                7 => "Black & White".to_string(),
                8 => "Shade".to_string(),
                9 => "Manual Temperature (Kelvin)".to_string(),
                10 => "PC Set1".to_string(),
                11 => "PC Set2".to_string(),
                12 => "PC Set3".to_string(),
                14 => "Daylight Fluorescent".to_string(),
                15 => "Custom 1".to_string(),
                16 => "Custom 2".to_string(),
                17 => "Underwater".to_string(),
                18 => "Custom 3".to_string(),
                19 => "Custom 4".to_string(),
                20 => "PC Set4".to_string(),
                21 => "PC Set5".to_string(),
                23 => "Auto (ambience priority)".to_string(),
                other => other.to_string(),
            };
            tags.push(mk("WhiteBalance", 0x4a, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if MODEL_RE_3.is_match(model) {
        if let Some(v) = u8_at(data, 0x4b) {
            dm.push(("PictureStyle".to_string(), f64::from(v)));
            let s = match v as i64 {
                0 => "None".to_string(),
                1 => "Standard".to_string(),
                2 => "Portrait".to_string(),
                3 => "High Saturation".to_string(),
                4 => "Adobe RGB".to_string(),
                5 => "Low Saturation".to_string(),
                6 => "CM Set 1".to_string(),
                7 => "CM Set 2".to_string(),
                33 => "User Def. 1".to_string(),
                34 => "User Def. 2".to_string(),
                35 => "User Def. 3".to_string(),
                65 => "PC 1".to_string(),
                66 => "PC 2".to_string(),
                67 => "PC 3".to_string(),
                129 => "Standard".to_string(),
                130 => "Portrait".to_string(),
                131 => "Landscape".to_string(),
                132 => "Neutral".to_string(),
                133 => "Faithful".to_string(),
                134 => "Monochrome".to_string(),
                135 => "Auto".to_string(),
                136 => "Fine Detail".to_string(),
                255 => "n/a".to_string(),
                65535 => "n/a".to_string(),
                other => other.to_string(),
            };
            tags.push(mk("PictureStyle", 0x4b, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if MODEL_RE_4.is_match(model) {
        if let Some(v) = u16_at(data, 0x4e, bo) {
            dm.push(("ColorTemperature".to_string(), f64::from(v)));
            tags.push(mk("ColorTemperature", 0x4e, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if MODEL_RE_4.is_match(model) {
        if let Some(v) = u8_at(data, 0x51) {
            dm.push(("PictureStyle".to_string(), f64::from(v)));
            let s = match v as i64 {
                0 => "None".to_string(),
                1 => "Standard".to_string(),
                2 => "Portrait".to_string(),
                3 => "High Saturation".to_string(),
                4 => "Adobe RGB".to_string(),
                5 => "Low Saturation".to_string(),
                6 => "CM Set 1".to_string(),
                7 => "CM Set 2".to_string(),
                33 => "User Def. 1".to_string(),
                34 => "User Def. 2".to_string(),
                35 => "User Def. 3".to_string(),
                65 => "PC 1".to_string(),
                66 => "PC 2".to_string(),
                67 => "PC 3".to_string(),
                129 => "Standard".to_string(),
                130 => "Portrait".to_string(),
                131 => "Landscape".to_string(),
                132 => "Neutral".to_string(),
                133 => "Faithful".to_string(),
                134 => "Monochrome".to_string(),
                135 => "Auto".to_string(),
                136 => "Fine Detail".to_string(),
                255 => "n/a".to_string(),
                65535 => "n/a".to_string(),
                other => other.to_string(),
            };
            tags.push(mk("PictureStyle", 0x51, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    tags
}

/// `Image::ExifTool::Canon::CameraInfo1DmkII` -- FORMAT int8u, FIRST_ENTRY 0.
fn canon_camerainfo1dmkii(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = u8_at(data, 0x4) {
        dm.push(("ExposureTime".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(4*log(2)*(1-Image::ExifTool::Canon::CanonEv($val-24)))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("Image::ExifTool::Exif::PrintExposureTime($val)", &cv, &ctx) { cv = x; }
            tags.push(mk("ExposureTime", 0x4, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u16rev_at(data, 0x9, bo) {
        dm.push(("FocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
            tags.push(mk("FocalLength", 0x9, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u16rev_at(data, 0xc, bo) {
        dm.push(("LensType".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let s = match v as i64 {
                -1 => "n/a".to_string(),
                1 => "Canon EF 50mm f/1.8".to_string(),
                2 => "Canon EF 28mm f/2.8 or Sigma Lens".to_string(),
                3 => "Canon EF 135mm f/2.8 Soft".to_string(),
                4 => "Canon EF 35-105mm f/3.5-4.5 or Sigma Lens".to_string(),
                5 => "Canon EF 35-70mm f/3.5-4.5".to_string(),
                6 => "Canon EF 28-70mm f/3.5-4.5 or Sigma or Tokina Lens".to_string(),
                7 => "Canon EF 100-300mm f/5.6L".to_string(),
                8 => "Canon EF 100-300mm f/5.6 or Sigma or Tokina Lens".to_string(),
                9 => "Canon EF 70-210mm f/4".to_string(),
                10 => "Canon EF 50mm f/2.5 Macro or Sigma Lens".to_string(),
                11 => "Canon EF 35mm f/2".to_string(),
                13 => "Canon EF 15mm f/2.8 Fisheye".to_string(),
                14 => "Canon EF 50-200mm f/3.5-4.5L".to_string(),
                15 => "Canon EF 50-200mm f/3.5-4.5".to_string(),
                16 => "Canon EF 35-135mm f/3.5-4.5".to_string(),
                17 => "Canon EF 35-70mm f/3.5-4.5A".to_string(),
                18 => "Canon EF 28-70mm f/3.5-4.5".to_string(),
                20 => "Canon EF 100-200mm f/4.5A".to_string(),
                21 => "Canon EF 80-200mm f/2.8L".to_string(),
                22 => "Canon EF 20-35mm f/2.8L or Tokina Lens".to_string(),
                23 => "Canon EF 35-105mm f/3.5-4.5".to_string(),
                24 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
                25 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
                26 => "Canon EF 100mm f/2.8 Macro or Other Lens".to_string(),
                27 => "Canon EF 35-80mm f/4-5.6".to_string(),
                28 => "Canon EF 80-200mm f/4.5-5.6 or Tamron Lens".to_string(),
                29 => "Canon EF 50mm f/1.8 II".to_string(),
                30 => "Canon EF 35-105mm f/4.5-5.6".to_string(),
                31 => "Canon EF 75-300mm f/4-5.6 or Tamron Lens".to_string(),
                32 => "Canon EF 24mm f/2.8 or Sigma Lens".to_string(),
                33 => "Voigtlander or Carl Zeiss Lens".to_string(),
                35 => "Canon EF 35-80mm f/4-5.6".to_string(),
                36 => "Canon EF 38-76mm f/4.5-5.6".to_string(),
                37 => "Canon EF 35-80mm f/4-5.6 or Tamron Lens".to_string(),
                38 => "Canon EF 80-200mm f/4.5-5.6 II".to_string(),
                39 => "Canon EF 75-300mm f/4-5.6".to_string(),
                40 => "Canon EF 28-80mm f/3.5-5.6".to_string(),
                41 => "Canon EF 28-90mm f/4-5.6".to_string(),
                42 => "Canon EF 28-200mm f/3.5-5.6 or Tamron Lens".to_string(),
                43 => "Canon EF 28-105mm f/4-5.6".to_string(),
                44 => "Canon EF 90-300mm f/4.5-5.6".to_string(),
                45 => "Canon EF-S 18-55mm f/3.5-5.6 [II]".to_string(),
                46 => "Canon EF 28-90mm f/4-5.6".to_string(),
                47 => "Zeiss Milvus 35mm f/2 or 50mm f/2".to_string(),
                48 => "Canon EF-S 18-55mm f/3.5-5.6 IS".to_string(),
                49 => "Canon EF-S 55-250mm f/4-5.6 IS".to_string(),
                50 => "Canon EF-S 18-200mm f/3.5-5.6 IS".to_string(),
                51 => "Canon EF-S 18-135mm f/3.5-5.6 IS".to_string(),
                52 => "Canon EF-S 18-55mm f/3.5-5.6 IS II".to_string(),
                53 => "Canon EF-S 18-55mm f/3.5-5.6 III".to_string(),
                54 => "Canon EF-S 55-250mm f/4-5.6 IS II".to_string(),
                60 => "Irix 11mm f/4 or 15mm f/2.4".to_string(),
                63 => "Irix 30mm F1.4 Dragonfly".to_string(),
                80 => "Canon TS-E 50mm f/2.8L Macro".to_string(),
                81 => "Canon TS-E 90mm f/2.8L Macro".to_string(),
                82 => "Canon TS-E 135mm f/4L Macro".to_string(),
                94 => "Canon TS-E 17mm f/4L".to_string(),
                95 => "Canon TS-E 24mm f/3.5L II".to_string(),
                103 => "Samyang AF 14mm f/2.8 EF or Rokinon Lens".to_string(),
                106 => "Rokinon SP / Samyang XP 35mm f/1.2".to_string(),
                112 => "Sigma 28mm f/1.5 FF High-speed Prime or other Sigma Lens".to_string(),
                117 => "Tamron 35-150mm f/2.8-4.0 Di VC OSD (A043) or other Tamron Lens".to_string(),
                124 => "Canon MP-E 65mm f/2.8 1-5x Macro Photo".to_string(),
                125 => "Canon TS-E 24mm f/3.5L".to_string(),
                126 => "Canon TS-E 45mm f/2.8".to_string(),
                127 => "Canon TS-E 90mm f/2.8 or Tamron Lens".to_string(),
                129 => "Canon EF 300mm f/2.8L USM".to_string(),
                130 => "Canon EF 50mm f/1.0L USM".to_string(),
                131 => "Canon EF 28-80mm f/2.8-4L USM or Sigma Lens".to_string(),
                132 => "Canon EF 1200mm f/5.6L USM".to_string(),
                134 => "Canon EF 600mm f/4L IS USM".to_string(),
                135 => "Canon EF 200mm f/1.8L USM".to_string(),
                136 => "Canon EF 300mm f/2.8L USM".to_string(),
                137 => "Canon EF 85mm f/1.2L USM or Sigma or Tamron Lens".to_string(),
                138 => "Canon EF 28-80mm f/2.8-4L".to_string(),
                139 => "Canon EF 400mm f/2.8L USM".to_string(),
                140 => "Canon EF 500mm f/4.5L USM".to_string(),
                141 => "Canon EF 500mm f/4.5L USM".to_string(),
                142 => "Canon EF 300mm f/2.8L IS USM".to_string(),
                143 => "Canon EF 500mm f/4L IS USM or Sigma Lens".to_string(),
                144 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
                145 => "Canon EF 100-300mm f/4.5-5.6 USM".to_string(),
                146 => "Canon EF 70-210mm f/3.5-4.5 USM".to_string(),
                147 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
                148 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
                149 => "Canon EF 100mm f/2 USM".to_string(),
                150 => "Canon EF 14mm f/2.8L USM or Sigma Lens".to_string(),
                151 => "Canon EF 200mm f/2.8L USM".to_string(),
                152 => "Canon EF 300mm f/4L IS USM or Sigma Lens".to_string(),
                153 => "Canon EF 35-350mm f/3.5-5.6L USM or Sigma or Tamron Lens".to_string(),
                154 => "Canon EF 20mm f/2.8 USM or Zeiss Lens".to_string(),
                155 => "Canon EF 85mm f/1.8 USM or Sigma Lens".to_string(),
                156 => "Canon EF 28-105mm f/3.5-4.5 USM or Tamron Lens".to_string(),
                160 => "Canon EF 20-35mm f/3.5-4.5 USM or Tamron or Tokina Lens".to_string(),
                161 => "Canon EF 28-70mm f/2.8L USM or Other Lens".to_string(),
                162 => "Canon EF 200mm f/2.8L USM".to_string(),
                163 => "Canon EF 300mm f/4L".to_string(),
                164 => "Canon EF 400mm f/5.6L".to_string(),
                165 => "Canon EF 70-200mm f/2.8L USM".to_string(),
                166 => "Canon EF 70-200mm f/2.8L USM + 1.4x".to_string(),
                167 => "Canon EF 70-200mm f/2.8L USM + 2x".to_string(),
                168 => "Canon EF 28mm f/1.8 USM or Sigma Lens".to_string(),
                169 => "Canon EF 17-35mm f/2.8L USM or Sigma Lens".to_string(),
                170 => "Canon EF 200mm f/2.8L II USM or Sigma Lens".to_string(),
                171 => "Canon EF 300mm f/4L USM".to_string(),
                172 => "Canon EF 400mm f/5.6L USM or Sigma Lens".to_string(),
                173 => "Canon EF 180mm Macro f/3.5L USM or Sigma Lens".to_string(),
                174 => "Canon EF 135mm f/2L USM or Other Lens".to_string(),
                175 => "Canon EF 400mm f/2.8L USM".to_string(),
                176 => "Canon EF 24-85mm f/3.5-4.5 USM".to_string(),
                177 => "Canon EF 300mm f/4L IS USM".to_string(),
                178 => "Canon EF 28-135mm f/3.5-5.6 IS".to_string(),
                179 => "Canon EF 24mm f/1.4L USM".to_string(),
                180 => "Canon EF 35mm f/1.4L USM or Other Lens".to_string(),
                181 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 1.4x or Sigma Lens".to_string(),
                182 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 2x or Sigma Lens".to_string(),
                183 => "Canon EF 100-400mm f/4.5-5.6L IS USM or Sigma Lens".to_string(),
                184 => "Canon EF 400mm f/2.8L USM + 2x".to_string(),
                185 => "Canon EF 600mm f/4L IS USM".to_string(),
                186 => "Canon EF 70-200mm f/4L USM".to_string(),
                187 => "Canon EF 70-200mm f/4L USM + 1.4x".to_string(),
                188 => "Canon EF 70-200mm f/4L USM + 2x".to_string(),
                189 => "Canon EF 70-200mm f/4L USM + 2.8x".to_string(),
                190 => "Canon EF 100mm f/2.8 Macro USM".to_string(),
                191 => "Canon EF 400mm f/4 DO IS or Sigma Lens".to_string(),
                193 => "Canon EF 35-80mm f/4-5.6 USM".to_string(),
                194 => "Canon EF 80-200mm f/4.5-5.6 USM".to_string(),
                195 => "Canon EF 35-105mm f/4.5-5.6 USM".to_string(),
                196 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
                197 => "Canon EF 75-300mm f/4-5.6 IS USM or Sigma Lens".to_string(),
                198 => "Canon EF 50mm f/1.4 USM or Other Lens".to_string(),
                199 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
                200 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
                201 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
                202 => "Canon EF 28-80mm f/3.5-5.6 USM IV".to_string(),
                208 => "Canon EF 22-55mm f/4-5.6 USM".to_string(),
                209 => "Canon EF 55-200mm f/4.5-5.6".to_string(),
                210 => "Canon EF 28-90mm f/4-5.6 USM".to_string(),
                211 => "Canon EF 28-200mm f/3.5-5.6 USM".to_string(),
                212 => "Canon EF 28-105mm f/4-5.6 USM".to_string(),
                213 => "Canon EF 90-300mm f/4.5-5.6 USM or Tamron Lens".to_string(),
                214 => "Canon EF-S 18-55mm f/3.5-5.6 USM".to_string(),
                215 => "Canon EF 55-200mm f/4.5-5.6 II USM".to_string(),
                217 => "Tamron AF 18-270mm f/3.5-6.3 Di II VC PZD".to_string(),
                220 => "Yongnuo YN 50mm f/1.8".to_string(),
                224 => "Canon EF 70-200mm f/2.8L IS USM".to_string(),
                225 => "Canon EF 70-200mm f/2.8L IS USM + 1.4x".to_string(),
                226 => "Canon EF 70-200mm f/2.8L IS USM + 2x".to_string(),
                227 => "Canon EF 70-200mm f/2.8L IS USM + 2.8x".to_string(),
                228 => "Canon EF 28-105mm f/3.5-4.5 USM".to_string(),
                229 => "Canon EF 16-35mm f/2.8L USM".to_string(),
                230 => "Canon EF 24-70mm f/2.8L USM".to_string(),
                231 => "Canon EF 17-40mm f/4L USM or Sigma Lens".to_string(),
                232 => "Canon EF 70-300mm f/4.5-5.6 DO IS USM".to_string(),
                233 => "Canon EF 28-300mm f/3.5-5.6L IS USM".to_string(),
                234 => "Canon EF-S 17-85mm f/4-5.6 IS USM or Tokina Lens".to_string(),
                235 => "Canon EF-S 10-22mm f/3.5-4.5 USM".to_string(),
                236 => "Canon EF-S 60mm f/2.8 Macro USM".to_string(),
                237 => "Canon EF 24-105mm f/4L IS USM".to_string(),
                238 => "Canon EF 70-300mm f/4-5.6 IS USM".to_string(),
                239 => "Canon EF 85mm f/1.2L II USM or Rokinon Lens".to_string(),
                240 => "Canon EF-S 17-55mm f/2.8 IS USM or Sigma Lens".to_string(),
                241 => "Canon EF 50mm f/1.2L USM".to_string(),
                242 => "Canon EF 70-200mm f/4L IS USM".to_string(),
                243 => "Canon EF 70-200mm f/4L IS USM + 1.4x".to_string(),
                244 => "Canon EF 70-200mm f/4L IS USM + 2x".to_string(),
                245 => "Canon EF 70-200mm f/4L IS USM + 2.8x".to_string(),
                246 => "Canon EF 16-35mm f/2.8L II USM".to_string(),
                247 => "Canon EF 14mm f/2.8L II USM".to_string(),
                248 => "Canon EF 200mm f/2L IS USM or Sigma Lens".to_string(),
                249 => "Canon EF 800mm f/5.6L IS USM".to_string(),
                250 => "Canon EF 24mm f/1.4L II USM or Sigma Lens".to_string(),
                251 => "Canon EF 70-200mm f/2.8L IS II USM".to_string(),
                252 => "Canon EF 70-200mm f/2.8L IS II USM + 1.4x".to_string(),
                253 => "Canon EF 70-200mm f/2.8L IS II USM + 2x".to_string(),
                254 => "Canon EF 100mm f/2.8L Macro IS USM or Tamron Lens".to_string(),
                255 => "Sigma 24-105mm f/4 DG OS HSM | A or Other Lens".to_string(),
                368 => "Sigma 14-24mm f/2.8 DG HSM | A or other Sigma Lens".to_string(),
                488 => "Canon EF-S 15-85mm f/3.5-5.6 IS USM".to_string(),
                489 => "Canon EF 70-300mm f/4-5.6L IS USM".to_string(),
                490 => "Canon EF 8-15mm f/4L Fisheye USM".to_string(),
                491 => "Canon EF 300mm f/2.8L IS II USM or Tamron Lens".to_string(),
                492 => "Canon EF 400mm f/2.8L IS II USM".to_string(),
                493 => "Canon EF 500mm f/4L IS II USM or EF 24-105mm f4L IS USM".to_string(),
                494 => "Canon EF 600mm f/4L IS II USM".to_string(),
                495 => "Canon EF 24-70mm f/2.8L II USM or Sigma Lens".to_string(),
                496 => "Canon EF 200-400mm f/4L IS USM".to_string(),
                499 => "Canon EF 200-400mm f/4L IS USM + 1.4x".to_string(),
                502 => "Canon EF 28mm f/2.8 IS USM or Tamron Lens".to_string(),
                503 => "Canon EF 24mm f/2.8 IS USM".to_string(),
                504 => "Canon EF 24-70mm f/4L IS USM".to_string(),
                505 => "Canon EF 35mm f/2 IS USM".to_string(),
                506 => "Canon EF 400mm f/4 DO IS II USM".to_string(),
                507 => "Canon EF 16-35mm f/4L IS USM".to_string(),
                508 => "Canon EF 11-24mm f/4L USM or Tamron Lens".to_string(),
                624 => "Sigma 70-200mm f/2.8 DG OS HSM | S or other Sigma Lens".to_string(),
                747 => "Canon EF 100-400mm f/4.5-5.6L IS II USM or Tamron Lens".to_string(),
                748 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 1.4x or Tamron Lens".to_string(),
                749 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 2x or Tamron Lens".to_string(),
                750 => "Canon EF 35mm f/1.4L II USM or Tamron Lens".to_string(),
                751 => "Canon EF 16-35mm f/2.8L III USM".to_string(),
                752 => "Canon EF 24-105mm f/4L IS II USM".to_string(),
                753 => "Canon EF 85mm f/1.4L IS USM".to_string(),
                754 => "Canon EF 70-200mm f/4L IS II USM".to_string(),
                757 => "Canon EF 400mm f/2.8L IS III USM".to_string(),
                758 => "Canon EF 600mm f/4L IS III USM".to_string(),
                923 => "Meike/SKY 85mm f/1.8 DCM".to_string(),
                1136 => "Sigma 24-70mm f/2.8 DG OS HSM | A".to_string(),
                4142 => "Canon EF-S 18-135mm f/3.5-5.6 IS STM".to_string(),
                4143 => "Canon EF-M 18-55mm f/3.5-5.6 IS STM or Tamron Lens".to_string(),
                4144 => "Canon EF 40mm f/2.8 STM".to_string(),
                4145 => "Canon EF-M 22mm f/2 STM".to_string(),
                4146 => "Canon EF-S 18-55mm f/3.5-5.6 IS STM".to_string(),
                4147 => "Canon EF-M 11-22mm f/4-5.6 IS STM".to_string(),
                4148 => "Canon EF-S 55-250mm f/4-5.6 IS STM".to_string(),
                4149 => "Canon EF-M 55-200mm f/4.5-6.3 IS STM".to_string(),
                4150 => "Canon EF-S 10-18mm f/4.5-5.6 IS STM".to_string(),
                4152 => "Canon EF 24-105mm f/3.5-5.6 IS STM".to_string(),
                4153 => "Canon EF-M 15-45mm f/3.5-6.3 IS STM".to_string(),
                4154 => "Canon EF-S 24mm f/2.8 STM".to_string(),
                4155 => "Canon EF-M 28mm f/3.5 Macro IS STM".to_string(),
                4156 => "Canon EF 50mm f/1.8 STM".to_string(),
                4157 => "Canon EF-M 18-150mm f/3.5-6.3 IS STM".to_string(),
                4158 => "Canon EF-S 18-55mm f/4-5.6 IS STM".to_string(),
                4159 => "Canon EF-M 32mm f/1.4 STM".to_string(),
                4160 => "Canon EF-S 35mm f/2.8 Macro IS STM".to_string(),
                4208 => "Sigma 56mm f/1.4 DC DN | C or other Sigma Lens".to_string(),
                4976 => "Sigma 16-300mm F3.5-6.7 DC OS | C (025)".to_string(),
                6512 => "Sigma 12mm F1.4 DC | C".to_string(),
                36910 => "Canon EF 70-300mm f/4-5.6 IS II USM".to_string(),
                36912 => "Canon EF-S 18-135mm f/3.5-5.6 IS USM".to_string(),
                61182 => "Canon RF 50mm F1.2L USM or other Canon RF Lens".to_string(),
                61491 => "Canon CN-E 14mm T3.1 L F".to_string(),
                61492 => "Canon CN-E 24mm T1.5 L F".to_string(),
                61494 => "Canon CN-E 85mm T1.3 L F".to_string(),
                61495 => "Canon CN-E 135mm T2.2 L F".to_string(),
                61496 => "Canon CN-E 35mm T1.5 L F".to_string(),
                65535 => "n/a".to_string(),
                other => other.to_string(),
            };
            tags.push(mk("LensType", 0xc, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u16rev_at(data, 0x11, bo) {
        dm.push(("MinFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MinFocalLength", 0x11, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x13, bo) {
        dm.push(("MaxFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MaxFocalLength", 0x13, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x2d) {
        dm.push(("FocalType".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Fixed".to_string(),
            2 => "Zoom".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("FocalType", 0x2d, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x36) {
        dm.push(("WhiteBalance".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Auto".to_string(),
            1 => "Daylight".to_string(),
            2 => "Cloudy".to_string(),
            3 => "Tungsten".to_string(),
            4 => "Fluorescent".to_string(),
            5 => "Flash".to_string(),
            6 => "Custom".to_string(),
            7 => "Black & White".to_string(),
            8 => "Shade".to_string(),
            9 => "Manual Temperature (Kelvin)".to_string(),
            10 => "PC Set1".to_string(),
            11 => "PC Set2".to_string(),
            12 => "PC Set3".to_string(),
            14 => "Daylight Fluorescent".to_string(),
            15 => "Custom 1".to_string(),
            16 => "Custom 2".to_string(),
            17 => "Underwater".to_string(),
            18 => "Custom 3".to_string(),
            19 => "Custom 4".to_string(),
            20 => "PC Set4".to_string(),
            21 => "PC Set5".to_string(),
            23 => "Auto (ambience priority)".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("WhiteBalance", 0x36, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x37, bo) {
        dm.push(("ColorTemperature".to_string(), f64::from(v)));
        tags.push(mk("ColorTemperature", 0x37, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x39, bo) {
        dm.push(("CanonImageSize".to_string(), f64::from(v)));
        let s = match v as i64 {
            -1 => "n/a".to_string(),
            0 => "Large".to_string(),
            1 => "Medium".to_string(),
            2 => "Small".to_string(),
            5 => "Medium 1".to_string(),
            6 => "Medium 2".to_string(),
            7 => "Medium 3".to_string(),
            8 => "Postcard".to_string(),
            9 => "Widescreen".to_string(),
            10 => "Medium Widescreen".to_string(),
            14 => "Small 1".to_string(),
            15 => "Small 2".to_string(),
            16 => "Small 3".to_string(),
            128 => "640x480 Movie".to_string(),
            129 => "Medium Movie".to_string(),
            130 => "Small Movie".to_string(),
            137 => "1280x720 Movie".to_string(),
            142 => "1920x1080 Movie".to_string(),
            143 => "4096x2160 Movie".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("CanonImageSize", 0x39, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x66) {
        dm.push(("JPEGQuality".to_string(), f64::from(v)));
        tags.push(mk("JPEGQuality", 0x66, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x6c) {
        dm.push(("PictureStyle".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "None".to_string(),
            1 => "Standard".to_string(),
            2 => "Portrait".to_string(),
            3 => "High Saturation".to_string(),
            4 => "Adobe RGB".to_string(),
            5 => "Low Saturation".to_string(),
            6 => "CM Set 1".to_string(),
            7 => "CM Set 2".to_string(),
            33 => "User Def. 1".to_string(),
            34 => "User Def. 2".to_string(),
            35 => "User Def. 3".to_string(),
            65 => "PC 1".to_string(),
            66 => "PC 2".to_string(),
            67 => "PC 3".to_string(),
            129 => "Standard".to_string(),
            130 => "Portrait".to_string(),
            131 => "Landscape".to_string(),
            132 => "Neutral".to_string(),
            133 => "Faithful".to_string(),
            134 => "Monochrome".to_string(),
            135 => "Auto".to_string(),
            136 => "Fine Detail".to_string(),
            255 => "n/a".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("PictureStyle", 0x6c, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0x6e) {
        dm.push(("Saturation".to_string(), f64::from(v)));
        tags.push(mk("Saturation", 0x6e, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0x6f) {
        dm.push(("ColorTone".to_string(), f64::from(v)));
        tags.push(mk("ColorTone", 0x6f, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0x72) {
        dm.push(("Sharpness".to_string(), f64::from(v)));
        tags.push(mk("Sharpness", 0x72, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0x73) {
        dm.push(("Contrast".to_string(), f64::from(v)));
        tags.push(mk("Contrast", 0x73, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(text) = text_at(data, 0x75, 5, true) {
        tags.push(mk("ISO", 0x75, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    tags
}

/// `Image::ExifTool::Canon::CameraInfo1DmkIIN` -- FORMAT int8u, FIRST_ENTRY 0.
fn canon_camerainfo1dmkiin(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = u8_at(data, 0x4) {
        dm.push(("ExposureTime".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(4*log(2)*(1-Image::ExifTool::Canon::CanonEv($val-24)))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("Image::ExifTool::Exif::PrintExposureTime($val)", &cv, &ctx) { cv = x; }
            tags.push(mk("ExposureTime", 0x4, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u16rev_at(data, 0x9, bo) {
        dm.push(("FocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
            tags.push(mk("FocalLength", 0x9, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u16rev_at(data, 0xc, bo) {
        dm.push(("LensType".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let s = match v as i64 {
                -1 => "n/a".to_string(),
                1 => "Canon EF 50mm f/1.8".to_string(),
                2 => "Canon EF 28mm f/2.8 or Sigma Lens".to_string(),
                3 => "Canon EF 135mm f/2.8 Soft".to_string(),
                4 => "Canon EF 35-105mm f/3.5-4.5 or Sigma Lens".to_string(),
                5 => "Canon EF 35-70mm f/3.5-4.5".to_string(),
                6 => "Canon EF 28-70mm f/3.5-4.5 or Sigma or Tokina Lens".to_string(),
                7 => "Canon EF 100-300mm f/5.6L".to_string(),
                8 => "Canon EF 100-300mm f/5.6 or Sigma or Tokina Lens".to_string(),
                9 => "Canon EF 70-210mm f/4".to_string(),
                10 => "Canon EF 50mm f/2.5 Macro or Sigma Lens".to_string(),
                11 => "Canon EF 35mm f/2".to_string(),
                13 => "Canon EF 15mm f/2.8 Fisheye".to_string(),
                14 => "Canon EF 50-200mm f/3.5-4.5L".to_string(),
                15 => "Canon EF 50-200mm f/3.5-4.5".to_string(),
                16 => "Canon EF 35-135mm f/3.5-4.5".to_string(),
                17 => "Canon EF 35-70mm f/3.5-4.5A".to_string(),
                18 => "Canon EF 28-70mm f/3.5-4.5".to_string(),
                20 => "Canon EF 100-200mm f/4.5A".to_string(),
                21 => "Canon EF 80-200mm f/2.8L".to_string(),
                22 => "Canon EF 20-35mm f/2.8L or Tokina Lens".to_string(),
                23 => "Canon EF 35-105mm f/3.5-4.5".to_string(),
                24 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
                25 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
                26 => "Canon EF 100mm f/2.8 Macro or Other Lens".to_string(),
                27 => "Canon EF 35-80mm f/4-5.6".to_string(),
                28 => "Canon EF 80-200mm f/4.5-5.6 or Tamron Lens".to_string(),
                29 => "Canon EF 50mm f/1.8 II".to_string(),
                30 => "Canon EF 35-105mm f/4.5-5.6".to_string(),
                31 => "Canon EF 75-300mm f/4-5.6 or Tamron Lens".to_string(),
                32 => "Canon EF 24mm f/2.8 or Sigma Lens".to_string(),
                33 => "Voigtlander or Carl Zeiss Lens".to_string(),
                35 => "Canon EF 35-80mm f/4-5.6".to_string(),
                36 => "Canon EF 38-76mm f/4.5-5.6".to_string(),
                37 => "Canon EF 35-80mm f/4-5.6 or Tamron Lens".to_string(),
                38 => "Canon EF 80-200mm f/4.5-5.6 II".to_string(),
                39 => "Canon EF 75-300mm f/4-5.6".to_string(),
                40 => "Canon EF 28-80mm f/3.5-5.6".to_string(),
                41 => "Canon EF 28-90mm f/4-5.6".to_string(),
                42 => "Canon EF 28-200mm f/3.5-5.6 or Tamron Lens".to_string(),
                43 => "Canon EF 28-105mm f/4-5.6".to_string(),
                44 => "Canon EF 90-300mm f/4.5-5.6".to_string(),
                45 => "Canon EF-S 18-55mm f/3.5-5.6 [II]".to_string(),
                46 => "Canon EF 28-90mm f/4-5.6".to_string(),
                47 => "Zeiss Milvus 35mm f/2 or 50mm f/2".to_string(),
                48 => "Canon EF-S 18-55mm f/3.5-5.6 IS".to_string(),
                49 => "Canon EF-S 55-250mm f/4-5.6 IS".to_string(),
                50 => "Canon EF-S 18-200mm f/3.5-5.6 IS".to_string(),
                51 => "Canon EF-S 18-135mm f/3.5-5.6 IS".to_string(),
                52 => "Canon EF-S 18-55mm f/3.5-5.6 IS II".to_string(),
                53 => "Canon EF-S 18-55mm f/3.5-5.6 III".to_string(),
                54 => "Canon EF-S 55-250mm f/4-5.6 IS II".to_string(),
                60 => "Irix 11mm f/4 or 15mm f/2.4".to_string(),
                63 => "Irix 30mm F1.4 Dragonfly".to_string(),
                80 => "Canon TS-E 50mm f/2.8L Macro".to_string(),
                81 => "Canon TS-E 90mm f/2.8L Macro".to_string(),
                82 => "Canon TS-E 135mm f/4L Macro".to_string(),
                94 => "Canon TS-E 17mm f/4L".to_string(),
                95 => "Canon TS-E 24mm f/3.5L II".to_string(),
                103 => "Samyang AF 14mm f/2.8 EF or Rokinon Lens".to_string(),
                106 => "Rokinon SP / Samyang XP 35mm f/1.2".to_string(),
                112 => "Sigma 28mm f/1.5 FF High-speed Prime or other Sigma Lens".to_string(),
                117 => "Tamron 35-150mm f/2.8-4.0 Di VC OSD (A043) or other Tamron Lens".to_string(),
                124 => "Canon MP-E 65mm f/2.8 1-5x Macro Photo".to_string(),
                125 => "Canon TS-E 24mm f/3.5L".to_string(),
                126 => "Canon TS-E 45mm f/2.8".to_string(),
                127 => "Canon TS-E 90mm f/2.8 or Tamron Lens".to_string(),
                129 => "Canon EF 300mm f/2.8L USM".to_string(),
                130 => "Canon EF 50mm f/1.0L USM".to_string(),
                131 => "Canon EF 28-80mm f/2.8-4L USM or Sigma Lens".to_string(),
                132 => "Canon EF 1200mm f/5.6L USM".to_string(),
                134 => "Canon EF 600mm f/4L IS USM".to_string(),
                135 => "Canon EF 200mm f/1.8L USM".to_string(),
                136 => "Canon EF 300mm f/2.8L USM".to_string(),
                137 => "Canon EF 85mm f/1.2L USM or Sigma or Tamron Lens".to_string(),
                138 => "Canon EF 28-80mm f/2.8-4L".to_string(),
                139 => "Canon EF 400mm f/2.8L USM".to_string(),
                140 => "Canon EF 500mm f/4.5L USM".to_string(),
                141 => "Canon EF 500mm f/4.5L USM".to_string(),
                142 => "Canon EF 300mm f/2.8L IS USM".to_string(),
                143 => "Canon EF 500mm f/4L IS USM or Sigma Lens".to_string(),
                144 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
                145 => "Canon EF 100-300mm f/4.5-5.6 USM".to_string(),
                146 => "Canon EF 70-210mm f/3.5-4.5 USM".to_string(),
                147 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
                148 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
                149 => "Canon EF 100mm f/2 USM".to_string(),
                150 => "Canon EF 14mm f/2.8L USM or Sigma Lens".to_string(),
                151 => "Canon EF 200mm f/2.8L USM".to_string(),
                152 => "Canon EF 300mm f/4L IS USM or Sigma Lens".to_string(),
                153 => "Canon EF 35-350mm f/3.5-5.6L USM or Sigma or Tamron Lens".to_string(),
                154 => "Canon EF 20mm f/2.8 USM or Zeiss Lens".to_string(),
                155 => "Canon EF 85mm f/1.8 USM or Sigma Lens".to_string(),
                156 => "Canon EF 28-105mm f/3.5-4.5 USM or Tamron Lens".to_string(),
                160 => "Canon EF 20-35mm f/3.5-4.5 USM or Tamron or Tokina Lens".to_string(),
                161 => "Canon EF 28-70mm f/2.8L USM or Other Lens".to_string(),
                162 => "Canon EF 200mm f/2.8L USM".to_string(),
                163 => "Canon EF 300mm f/4L".to_string(),
                164 => "Canon EF 400mm f/5.6L".to_string(),
                165 => "Canon EF 70-200mm f/2.8L USM".to_string(),
                166 => "Canon EF 70-200mm f/2.8L USM + 1.4x".to_string(),
                167 => "Canon EF 70-200mm f/2.8L USM + 2x".to_string(),
                168 => "Canon EF 28mm f/1.8 USM or Sigma Lens".to_string(),
                169 => "Canon EF 17-35mm f/2.8L USM or Sigma Lens".to_string(),
                170 => "Canon EF 200mm f/2.8L II USM or Sigma Lens".to_string(),
                171 => "Canon EF 300mm f/4L USM".to_string(),
                172 => "Canon EF 400mm f/5.6L USM or Sigma Lens".to_string(),
                173 => "Canon EF 180mm Macro f/3.5L USM or Sigma Lens".to_string(),
                174 => "Canon EF 135mm f/2L USM or Other Lens".to_string(),
                175 => "Canon EF 400mm f/2.8L USM".to_string(),
                176 => "Canon EF 24-85mm f/3.5-4.5 USM".to_string(),
                177 => "Canon EF 300mm f/4L IS USM".to_string(),
                178 => "Canon EF 28-135mm f/3.5-5.6 IS".to_string(),
                179 => "Canon EF 24mm f/1.4L USM".to_string(),
                180 => "Canon EF 35mm f/1.4L USM or Other Lens".to_string(),
                181 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 1.4x or Sigma Lens".to_string(),
                182 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 2x or Sigma Lens".to_string(),
                183 => "Canon EF 100-400mm f/4.5-5.6L IS USM or Sigma Lens".to_string(),
                184 => "Canon EF 400mm f/2.8L USM + 2x".to_string(),
                185 => "Canon EF 600mm f/4L IS USM".to_string(),
                186 => "Canon EF 70-200mm f/4L USM".to_string(),
                187 => "Canon EF 70-200mm f/4L USM + 1.4x".to_string(),
                188 => "Canon EF 70-200mm f/4L USM + 2x".to_string(),
                189 => "Canon EF 70-200mm f/4L USM + 2.8x".to_string(),
                190 => "Canon EF 100mm f/2.8 Macro USM".to_string(),
                191 => "Canon EF 400mm f/4 DO IS or Sigma Lens".to_string(),
                193 => "Canon EF 35-80mm f/4-5.6 USM".to_string(),
                194 => "Canon EF 80-200mm f/4.5-5.6 USM".to_string(),
                195 => "Canon EF 35-105mm f/4.5-5.6 USM".to_string(),
                196 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
                197 => "Canon EF 75-300mm f/4-5.6 IS USM or Sigma Lens".to_string(),
                198 => "Canon EF 50mm f/1.4 USM or Other Lens".to_string(),
                199 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
                200 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
                201 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
                202 => "Canon EF 28-80mm f/3.5-5.6 USM IV".to_string(),
                208 => "Canon EF 22-55mm f/4-5.6 USM".to_string(),
                209 => "Canon EF 55-200mm f/4.5-5.6".to_string(),
                210 => "Canon EF 28-90mm f/4-5.6 USM".to_string(),
                211 => "Canon EF 28-200mm f/3.5-5.6 USM".to_string(),
                212 => "Canon EF 28-105mm f/4-5.6 USM".to_string(),
                213 => "Canon EF 90-300mm f/4.5-5.6 USM or Tamron Lens".to_string(),
                214 => "Canon EF-S 18-55mm f/3.5-5.6 USM".to_string(),
                215 => "Canon EF 55-200mm f/4.5-5.6 II USM".to_string(),
                217 => "Tamron AF 18-270mm f/3.5-6.3 Di II VC PZD".to_string(),
                220 => "Yongnuo YN 50mm f/1.8".to_string(),
                224 => "Canon EF 70-200mm f/2.8L IS USM".to_string(),
                225 => "Canon EF 70-200mm f/2.8L IS USM + 1.4x".to_string(),
                226 => "Canon EF 70-200mm f/2.8L IS USM + 2x".to_string(),
                227 => "Canon EF 70-200mm f/2.8L IS USM + 2.8x".to_string(),
                228 => "Canon EF 28-105mm f/3.5-4.5 USM".to_string(),
                229 => "Canon EF 16-35mm f/2.8L USM".to_string(),
                230 => "Canon EF 24-70mm f/2.8L USM".to_string(),
                231 => "Canon EF 17-40mm f/4L USM or Sigma Lens".to_string(),
                232 => "Canon EF 70-300mm f/4.5-5.6 DO IS USM".to_string(),
                233 => "Canon EF 28-300mm f/3.5-5.6L IS USM".to_string(),
                234 => "Canon EF-S 17-85mm f/4-5.6 IS USM or Tokina Lens".to_string(),
                235 => "Canon EF-S 10-22mm f/3.5-4.5 USM".to_string(),
                236 => "Canon EF-S 60mm f/2.8 Macro USM".to_string(),
                237 => "Canon EF 24-105mm f/4L IS USM".to_string(),
                238 => "Canon EF 70-300mm f/4-5.6 IS USM".to_string(),
                239 => "Canon EF 85mm f/1.2L II USM or Rokinon Lens".to_string(),
                240 => "Canon EF-S 17-55mm f/2.8 IS USM or Sigma Lens".to_string(),
                241 => "Canon EF 50mm f/1.2L USM".to_string(),
                242 => "Canon EF 70-200mm f/4L IS USM".to_string(),
                243 => "Canon EF 70-200mm f/4L IS USM + 1.4x".to_string(),
                244 => "Canon EF 70-200mm f/4L IS USM + 2x".to_string(),
                245 => "Canon EF 70-200mm f/4L IS USM + 2.8x".to_string(),
                246 => "Canon EF 16-35mm f/2.8L II USM".to_string(),
                247 => "Canon EF 14mm f/2.8L II USM".to_string(),
                248 => "Canon EF 200mm f/2L IS USM or Sigma Lens".to_string(),
                249 => "Canon EF 800mm f/5.6L IS USM".to_string(),
                250 => "Canon EF 24mm f/1.4L II USM or Sigma Lens".to_string(),
                251 => "Canon EF 70-200mm f/2.8L IS II USM".to_string(),
                252 => "Canon EF 70-200mm f/2.8L IS II USM + 1.4x".to_string(),
                253 => "Canon EF 70-200mm f/2.8L IS II USM + 2x".to_string(),
                254 => "Canon EF 100mm f/2.8L Macro IS USM or Tamron Lens".to_string(),
                255 => "Sigma 24-105mm f/4 DG OS HSM | A or Other Lens".to_string(),
                368 => "Sigma 14-24mm f/2.8 DG HSM | A or other Sigma Lens".to_string(),
                488 => "Canon EF-S 15-85mm f/3.5-5.6 IS USM".to_string(),
                489 => "Canon EF 70-300mm f/4-5.6L IS USM".to_string(),
                490 => "Canon EF 8-15mm f/4L Fisheye USM".to_string(),
                491 => "Canon EF 300mm f/2.8L IS II USM or Tamron Lens".to_string(),
                492 => "Canon EF 400mm f/2.8L IS II USM".to_string(),
                493 => "Canon EF 500mm f/4L IS II USM or EF 24-105mm f4L IS USM".to_string(),
                494 => "Canon EF 600mm f/4L IS II USM".to_string(),
                495 => "Canon EF 24-70mm f/2.8L II USM or Sigma Lens".to_string(),
                496 => "Canon EF 200-400mm f/4L IS USM".to_string(),
                499 => "Canon EF 200-400mm f/4L IS USM + 1.4x".to_string(),
                502 => "Canon EF 28mm f/2.8 IS USM or Tamron Lens".to_string(),
                503 => "Canon EF 24mm f/2.8 IS USM".to_string(),
                504 => "Canon EF 24-70mm f/4L IS USM".to_string(),
                505 => "Canon EF 35mm f/2 IS USM".to_string(),
                506 => "Canon EF 400mm f/4 DO IS II USM".to_string(),
                507 => "Canon EF 16-35mm f/4L IS USM".to_string(),
                508 => "Canon EF 11-24mm f/4L USM or Tamron Lens".to_string(),
                624 => "Sigma 70-200mm f/2.8 DG OS HSM | S or other Sigma Lens".to_string(),
                747 => "Canon EF 100-400mm f/4.5-5.6L IS II USM or Tamron Lens".to_string(),
                748 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 1.4x or Tamron Lens".to_string(),
                749 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 2x or Tamron Lens".to_string(),
                750 => "Canon EF 35mm f/1.4L II USM or Tamron Lens".to_string(),
                751 => "Canon EF 16-35mm f/2.8L III USM".to_string(),
                752 => "Canon EF 24-105mm f/4L IS II USM".to_string(),
                753 => "Canon EF 85mm f/1.4L IS USM".to_string(),
                754 => "Canon EF 70-200mm f/4L IS II USM".to_string(),
                757 => "Canon EF 400mm f/2.8L IS III USM".to_string(),
                758 => "Canon EF 600mm f/4L IS III USM".to_string(),
                923 => "Meike/SKY 85mm f/1.8 DCM".to_string(),
                1136 => "Sigma 24-70mm f/2.8 DG OS HSM | A".to_string(),
                4142 => "Canon EF-S 18-135mm f/3.5-5.6 IS STM".to_string(),
                4143 => "Canon EF-M 18-55mm f/3.5-5.6 IS STM or Tamron Lens".to_string(),
                4144 => "Canon EF 40mm f/2.8 STM".to_string(),
                4145 => "Canon EF-M 22mm f/2 STM".to_string(),
                4146 => "Canon EF-S 18-55mm f/3.5-5.6 IS STM".to_string(),
                4147 => "Canon EF-M 11-22mm f/4-5.6 IS STM".to_string(),
                4148 => "Canon EF-S 55-250mm f/4-5.6 IS STM".to_string(),
                4149 => "Canon EF-M 55-200mm f/4.5-6.3 IS STM".to_string(),
                4150 => "Canon EF-S 10-18mm f/4.5-5.6 IS STM".to_string(),
                4152 => "Canon EF 24-105mm f/3.5-5.6 IS STM".to_string(),
                4153 => "Canon EF-M 15-45mm f/3.5-6.3 IS STM".to_string(),
                4154 => "Canon EF-S 24mm f/2.8 STM".to_string(),
                4155 => "Canon EF-M 28mm f/3.5 Macro IS STM".to_string(),
                4156 => "Canon EF 50mm f/1.8 STM".to_string(),
                4157 => "Canon EF-M 18-150mm f/3.5-6.3 IS STM".to_string(),
                4158 => "Canon EF-S 18-55mm f/4-5.6 IS STM".to_string(),
                4159 => "Canon EF-M 32mm f/1.4 STM".to_string(),
                4160 => "Canon EF-S 35mm f/2.8 Macro IS STM".to_string(),
                4208 => "Sigma 56mm f/1.4 DC DN | C or other Sigma Lens".to_string(),
                4976 => "Sigma 16-300mm F3.5-6.7 DC OS | C (025)".to_string(),
                6512 => "Sigma 12mm F1.4 DC | C".to_string(),
                36910 => "Canon EF 70-300mm f/4-5.6 IS II USM".to_string(),
                36912 => "Canon EF-S 18-135mm f/3.5-5.6 IS USM".to_string(),
                61182 => "Canon RF 50mm F1.2L USM or other Canon RF Lens".to_string(),
                61491 => "Canon CN-E 14mm T3.1 L F".to_string(),
                61492 => "Canon CN-E 24mm T1.5 L F".to_string(),
                61494 => "Canon CN-E 85mm T1.3 L F".to_string(),
                61495 => "Canon CN-E 135mm T2.2 L F".to_string(),
                61496 => "Canon CN-E 35mm T1.5 L F".to_string(),
                65535 => "n/a".to_string(),
                other => other.to_string(),
            };
            tags.push(mk("LensType", 0xc, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u16rev_at(data, 0x11, bo) {
        dm.push(("MinFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MinFocalLength", 0x11, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x13, bo) {
        dm.push(("MaxFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MaxFocalLength", 0x13, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x36) {
        dm.push(("WhiteBalance".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Auto".to_string(),
            1 => "Daylight".to_string(),
            2 => "Cloudy".to_string(),
            3 => "Tungsten".to_string(),
            4 => "Fluorescent".to_string(),
            5 => "Flash".to_string(),
            6 => "Custom".to_string(),
            7 => "Black & White".to_string(),
            8 => "Shade".to_string(),
            9 => "Manual Temperature (Kelvin)".to_string(),
            10 => "PC Set1".to_string(),
            11 => "PC Set2".to_string(),
            12 => "PC Set3".to_string(),
            14 => "Daylight Fluorescent".to_string(),
            15 => "Custom 1".to_string(),
            16 => "Custom 2".to_string(),
            17 => "Underwater".to_string(),
            18 => "Custom 3".to_string(),
            19 => "Custom 4".to_string(),
            20 => "PC Set4".to_string(),
            21 => "PC Set5".to_string(),
            23 => "Auto (ambience priority)".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("WhiteBalance", 0x36, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x37, bo) {
        dm.push(("ColorTemperature".to_string(), f64::from(v)));
        tags.push(mk("ColorTemperature", 0x37, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x73) {
        dm.push(("PictureStyle".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "None".to_string(),
            1 => "Standard".to_string(),
            2 => "Portrait".to_string(),
            3 => "High Saturation".to_string(),
            4 => "Adobe RGB".to_string(),
            5 => "Low Saturation".to_string(),
            6 => "CM Set 1".to_string(),
            7 => "CM Set 2".to_string(),
            33 => "User Def. 1".to_string(),
            34 => "User Def. 2".to_string(),
            35 => "User Def. 3".to_string(),
            65 => "PC 1".to_string(),
            66 => "PC 2".to_string(),
            67 => "PC 3".to_string(),
            129 => "Standard".to_string(),
            130 => "Portrait".to_string(),
            131 => "Landscape".to_string(),
            132 => "Neutral".to_string(),
            133 => "Faithful".to_string(),
            134 => "Monochrome".to_string(),
            135 => "Auto".to_string(),
            136 => "Fine Detail".to_string(),
            255 => "n/a".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("PictureStyle", 0x73, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0x74) {
        dm.push(("Sharpness".to_string(), f64::from(v)));
        tags.push(mk("Sharpness", 0x74, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0x75) {
        dm.push(("Contrast".to_string(), f64::from(v)));
        tags.push(mk("Contrast", 0x75, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0x76) {
        dm.push(("Saturation".to_string(), f64::from(v)));
        tags.push(mk("Saturation", 0x76, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0x77) {
        dm.push(("ColorTone".to_string(), f64::from(v)));
        tags.push(mk("ColorTone", 0x77, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(text) = text_at(data, 0x79, 5, true) {
        tags.push(mk("ISO", 0x79, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    tags
}

/// `Image::ExifTool::Canon::CameraInfo1DmkIII` -- FORMAT int8u, FIRST_ENTRY 0.
fn canon_camerainfo1dmkiii(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = u8_at(data, 0x3) {
        dm.push(("FNumber".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(($val-8)/16*log(2))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("sprintf(\"%.2g\",$val)", &cv, &ctx) { cv = x; }
            tags.push(mk("FNumber", 0x3, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x4) {
        dm.push(("ExposureTime".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(4*log(2)*(1-Image::ExifTool::Canon::CanonEv($val-24)))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("Image::ExifTool::Exif::PrintExposureTime($val)", &cv, &ctx) { cv = x; }
            tags.push(mk("ExposureTime", 0x4, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x6) {
        dm.push(("ISO".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("100*exp(($val/8-9)*log(2))", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("sprintf(\"%.0f\",$val)", &cv, &ctx) { cv = x; }
        tags.push(mk("ISO", 0x6, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x18) {
        dm.push(("CameraTemperature".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 128", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
        tags.push(mk("CameraTemperature", 0x18, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if (dm_get(dm, "LensType").is_some_and(|v| v != 0.0) && dm_get(dm, "LensType").is_some_and(|v| v == 124.0)) {
        if let Some(v) = u8_at(data, 0x1b) {
            dm.push(("MacroMagnification".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp((75-$val) * log(2) * 3 / 40)", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("sprintf(\"%.1fx\",$val)", &cv, &ctx) { cv = x; }
            tags.push(mk("MacroMagnification", 0x1b, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u16rev_at(data, 0x1d, bo) {
        dm.push(("FocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
            tags.push(mk("FocalLength", 0x1d, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x30) {
        dm.push(("CameraOrientation".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Horizontal (normal)".to_string(),
            1 => "Rotate 90 CW".to_string(),
            2 => "Rotate 270 CW".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("CameraOrientation", 0x30, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x43, bo) {
        dm.push(("FocusDistanceUpper".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceUpper", 0x43, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x45, bo) {
        dm.push(("FocusDistanceLower".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceLower", 0x45, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x5e, bo) {
        dm.push(("WhiteBalance".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Auto".to_string(),
            1 => "Daylight".to_string(),
            2 => "Cloudy".to_string(),
            3 => "Tungsten".to_string(),
            4 => "Fluorescent".to_string(),
            5 => "Flash".to_string(),
            6 => "Custom".to_string(),
            7 => "Black & White".to_string(),
            8 => "Shade".to_string(),
            9 => "Manual Temperature (Kelvin)".to_string(),
            10 => "PC Set1".to_string(),
            11 => "PC Set2".to_string(),
            12 => "PC Set3".to_string(),
            14 => "Daylight Fluorescent".to_string(),
            15 => "Custom 1".to_string(),
            16 => "Custom 2".to_string(),
            17 => "Underwater".to_string(),
            18 => "Custom 3".to_string(),
            19 => "Custom 4".to_string(),
            20 => "PC Set4".to_string(),
            21 => "PC Set5".to_string(),
            23 => "Auto (ambience priority)".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("WhiteBalance", 0x5e, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x62, bo) {
        dm.push(("ColorTemperature".to_string(), f64::from(v)));
        tags.push(mk("ColorTemperature", 0x62, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x86) {
        dm.push(("PictureStyle".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "None".to_string(),
            1 => "Standard".to_string(),
            2 => "Portrait".to_string(),
            3 => "High Saturation".to_string(),
            4 => "Adobe RGB".to_string(),
            5 => "Low Saturation".to_string(),
            6 => "CM Set 1".to_string(),
            7 => "CM Set 2".to_string(),
            33 => "User Def. 1".to_string(),
            34 => "User Def. 2".to_string(),
            35 => "User Def. 3".to_string(),
            65 => "PC 1".to_string(),
            66 => "PC 2".to_string(),
            67 => "PC 3".to_string(),
            129 => "Standard".to_string(),
            130 => "Portrait".to_string(),
            131 => "Landscape".to_string(),
            132 => "Neutral".to_string(),
            133 => "Faithful".to_string(),
            134 => "Monochrome".to_string(),
            135 => "Auto".to_string(),
            136 => "Fine Detail".to_string(),
            255 => "n/a".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("PictureStyle", 0x86, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x111, bo) {
        dm.push(("LensType".to_string(), f64::from(v)));
        let s = match v as i64 {
            -1 => "n/a".to_string(),
            1 => "Canon EF 50mm f/1.8".to_string(),
            2 => "Canon EF 28mm f/2.8 or Sigma Lens".to_string(),
            3 => "Canon EF 135mm f/2.8 Soft".to_string(),
            4 => "Canon EF 35-105mm f/3.5-4.5 or Sigma Lens".to_string(),
            5 => "Canon EF 35-70mm f/3.5-4.5".to_string(),
            6 => "Canon EF 28-70mm f/3.5-4.5 or Sigma or Tokina Lens".to_string(),
            7 => "Canon EF 100-300mm f/5.6L".to_string(),
            8 => "Canon EF 100-300mm f/5.6 or Sigma or Tokina Lens".to_string(),
            9 => "Canon EF 70-210mm f/4".to_string(),
            10 => "Canon EF 50mm f/2.5 Macro or Sigma Lens".to_string(),
            11 => "Canon EF 35mm f/2".to_string(),
            13 => "Canon EF 15mm f/2.8 Fisheye".to_string(),
            14 => "Canon EF 50-200mm f/3.5-4.5L".to_string(),
            15 => "Canon EF 50-200mm f/3.5-4.5".to_string(),
            16 => "Canon EF 35-135mm f/3.5-4.5".to_string(),
            17 => "Canon EF 35-70mm f/3.5-4.5A".to_string(),
            18 => "Canon EF 28-70mm f/3.5-4.5".to_string(),
            20 => "Canon EF 100-200mm f/4.5A".to_string(),
            21 => "Canon EF 80-200mm f/2.8L".to_string(),
            22 => "Canon EF 20-35mm f/2.8L or Tokina Lens".to_string(),
            23 => "Canon EF 35-105mm f/3.5-4.5".to_string(),
            24 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            25 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            26 => "Canon EF 100mm f/2.8 Macro or Other Lens".to_string(),
            27 => "Canon EF 35-80mm f/4-5.6".to_string(),
            28 => "Canon EF 80-200mm f/4.5-5.6 or Tamron Lens".to_string(),
            29 => "Canon EF 50mm f/1.8 II".to_string(),
            30 => "Canon EF 35-105mm f/4.5-5.6".to_string(),
            31 => "Canon EF 75-300mm f/4-5.6 or Tamron Lens".to_string(),
            32 => "Canon EF 24mm f/2.8 or Sigma Lens".to_string(),
            33 => "Voigtlander or Carl Zeiss Lens".to_string(),
            35 => "Canon EF 35-80mm f/4-5.6".to_string(),
            36 => "Canon EF 38-76mm f/4.5-5.6".to_string(),
            37 => "Canon EF 35-80mm f/4-5.6 or Tamron Lens".to_string(),
            38 => "Canon EF 80-200mm f/4.5-5.6 II".to_string(),
            39 => "Canon EF 75-300mm f/4-5.6".to_string(),
            40 => "Canon EF 28-80mm f/3.5-5.6".to_string(),
            41 => "Canon EF 28-90mm f/4-5.6".to_string(),
            42 => "Canon EF 28-200mm f/3.5-5.6 or Tamron Lens".to_string(),
            43 => "Canon EF 28-105mm f/4-5.6".to_string(),
            44 => "Canon EF 90-300mm f/4.5-5.6".to_string(),
            45 => "Canon EF-S 18-55mm f/3.5-5.6 [II]".to_string(),
            46 => "Canon EF 28-90mm f/4-5.6".to_string(),
            47 => "Zeiss Milvus 35mm f/2 or 50mm f/2".to_string(),
            48 => "Canon EF-S 18-55mm f/3.5-5.6 IS".to_string(),
            49 => "Canon EF-S 55-250mm f/4-5.6 IS".to_string(),
            50 => "Canon EF-S 18-200mm f/3.5-5.6 IS".to_string(),
            51 => "Canon EF-S 18-135mm f/3.5-5.6 IS".to_string(),
            52 => "Canon EF-S 18-55mm f/3.5-5.6 IS II".to_string(),
            53 => "Canon EF-S 18-55mm f/3.5-5.6 III".to_string(),
            54 => "Canon EF-S 55-250mm f/4-5.6 IS II".to_string(),
            60 => "Irix 11mm f/4 or 15mm f/2.4".to_string(),
            63 => "Irix 30mm F1.4 Dragonfly".to_string(),
            80 => "Canon TS-E 50mm f/2.8L Macro".to_string(),
            81 => "Canon TS-E 90mm f/2.8L Macro".to_string(),
            82 => "Canon TS-E 135mm f/4L Macro".to_string(),
            94 => "Canon TS-E 17mm f/4L".to_string(),
            95 => "Canon TS-E 24mm f/3.5L II".to_string(),
            103 => "Samyang AF 14mm f/2.8 EF or Rokinon Lens".to_string(),
            106 => "Rokinon SP / Samyang XP 35mm f/1.2".to_string(),
            112 => "Sigma 28mm f/1.5 FF High-speed Prime or other Sigma Lens".to_string(),
            117 => "Tamron 35-150mm f/2.8-4.0 Di VC OSD (A043) or other Tamron Lens".to_string(),
            124 => "Canon MP-E 65mm f/2.8 1-5x Macro Photo".to_string(),
            125 => "Canon TS-E 24mm f/3.5L".to_string(),
            126 => "Canon TS-E 45mm f/2.8".to_string(),
            127 => "Canon TS-E 90mm f/2.8 or Tamron Lens".to_string(),
            129 => "Canon EF 300mm f/2.8L USM".to_string(),
            130 => "Canon EF 50mm f/1.0L USM".to_string(),
            131 => "Canon EF 28-80mm f/2.8-4L USM or Sigma Lens".to_string(),
            132 => "Canon EF 1200mm f/5.6L USM".to_string(),
            134 => "Canon EF 600mm f/4L IS USM".to_string(),
            135 => "Canon EF 200mm f/1.8L USM".to_string(),
            136 => "Canon EF 300mm f/2.8L USM".to_string(),
            137 => "Canon EF 85mm f/1.2L USM or Sigma or Tamron Lens".to_string(),
            138 => "Canon EF 28-80mm f/2.8-4L".to_string(),
            139 => "Canon EF 400mm f/2.8L USM".to_string(),
            140 => "Canon EF 500mm f/4.5L USM".to_string(),
            141 => "Canon EF 500mm f/4.5L USM".to_string(),
            142 => "Canon EF 300mm f/2.8L IS USM".to_string(),
            143 => "Canon EF 500mm f/4L IS USM or Sigma Lens".to_string(),
            144 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            145 => "Canon EF 100-300mm f/4.5-5.6 USM".to_string(),
            146 => "Canon EF 70-210mm f/3.5-4.5 USM".to_string(),
            147 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            148 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            149 => "Canon EF 100mm f/2 USM".to_string(),
            150 => "Canon EF 14mm f/2.8L USM or Sigma Lens".to_string(),
            151 => "Canon EF 200mm f/2.8L USM".to_string(),
            152 => "Canon EF 300mm f/4L IS USM or Sigma Lens".to_string(),
            153 => "Canon EF 35-350mm f/3.5-5.6L USM or Sigma or Tamron Lens".to_string(),
            154 => "Canon EF 20mm f/2.8 USM or Zeiss Lens".to_string(),
            155 => "Canon EF 85mm f/1.8 USM or Sigma Lens".to_string(),
            156 => "Canon EF 28-105mm f/3.5-4.5 USM or Tamron Lens".to_string(),
            160 => "Canon EF 20-35mm f/3.5-4.5 USM or Tamron or Tokina Lens".to_string(),
            161 => "Canon EF 28-70mm f/2.8L USM or Other Lens".to_string(),
            162 => "Canon EF 200mm f/2.8L USM".to_string(),
            163 => "Canon EF 300mm f/4L".to_string(),
            164 => "Canon EF 400mm f/5.6L".to_string(),
            165 => "Canon EF 70-200mm f/2.8L USM".to_string(),
            166 => "Canon EF 70-200mm f/2.8L USM + 1.4x".to_string(),
            167 => "Canon EF 70-200mm f/2.8L USM + 2x".to_string(),
            168 => "Canon EF 28mm f/1.8 USM or Sigma Lens".to_string(),
            169 => "Canon EF 17-35mm f/2.8L USM or Sigma Lens".to_string(),
            170 => "Canon EF 200mm f/2.8L II USM or Sigma Lens".to_string(),
            171 => "Canon EF 300mm f/4L USM".to_string(),
            172 => "Canon EF 400mm f/5.6L USM or Sigma Lens".to_string(),
            173 => "Canon EF 180mm Macro f/3.5L USM or Sigma Lens".to_string(),
            174 => "Canon EF 135mm f/2L USM or Other Lens".to_string(),
            175 => "Canon EF 400mm f/2.8L USM".to_string(),
            176 => "Canon EF 24-85mm f/3.5-4.5 USM".to_string(),
            177 => "Canon EF 300mm f/4L IS USM".to_string(),
            178 => "Canon EF 28-135mm f/3.5-5.6 IS".to_string(),
            179 => "Canon EF 24mm f/1.4L USM".to_string(),
            180 => "Canon EF 35mm f/1.4L USM or Other Lens".to_string(),
            181 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 1.4x or Sigma Lens".to_string(),
            182 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 2x or Sigma Lens".to_string(),
            183 => "Canon EF 100-400mm f/4.5-5.6L IS USM or Sigma Lens".to_string(),
            184 => "Canon EF 400mm f/2.8L USM + 2x".to_string(),
            185 => "Canon EF 600mm f/4L IS USM".to_string(),
            186 => "Canon EF 70-200mm f/4L USM".to_string(),
            187 => "Canon EF 70-200mm f/4L USM + 1.4x".to_string(),
            188 => "Canon EF 70-200mm f/4L USM + 2x".to_string(),
            189 => "Canon EF 70-200mm f/4L USM + 2.8x".to_string(),
            190 => "Canon EF 100mm f/2.8 Macro USM".to_string(),
            191 => "Canon EF 400mm f/4 DO IS or Sigma Lens".to_string(),
            193 => "Canon EF 35-80mm f/4-5.6 USM".to_string(),
            194 => "Canon EF 80-200mm f/4.5-5.6 USM".to_string(),
            195 => "Canon EF 35-105mm f/4.5-5.6 USM".to_string(),
            196 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            197 => "Canon EF 75-300mm f/4-5.6 IS USM or Sigma Lens".to_string(),
            198 => "Canon EF 50mm f/1.4 USM or Other Lens".to_string(),
            199 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            200 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            201 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            202 => "Canon EF 28-80mm f/3.5-5.6 USM IV".to_string(),
            208 => "Canon EF 22-55mm f/4-5.6 USM".to_string(),
            209 => "Canon EF 55-200mm f/4.5-5.6".to_string(),
            210 => "Canon EF 28-90mm f/4-5.6 USM".to_string(),
            211 => "Canon EF 28-200mm f/3.5-5.6 USM".to_string(),
            212 => "Canon EF 28-105mm f/4-5.6 USM".to_string(),
            213 => "Canon EF 90-300mm f/4.5-5.6 USM or Tamron Lens".to_string(),
            214 => "Canon EF-S 18-55mm f/3.5-5.6 USM".to_string(),
            215 => "Canon EF 55-200mm f/4.5-5.6 II USM".to_string(),
            217 => "Tamron AF 18-270mm f/3.5-6.3 Di II VC PZD".to_string(),
            220 => "Yongnuo YN 50mm f/1.8".to_string(),
            224 => "Canon EF 70-200mm f/2.8L IS USM".to_string(),
            225 => "Canon EF 70-200mm f/2.8L IS USM + 1.4x".to_string(),
            226 => "Canon EF 70-200mm f/2.8L IS USM + 2x".to_string(),
            227 => "Canon EF 70-200mm f/2.8L IS USM + 2.8x".to_string(),
            228 => "Canon EF 28-105mm f/3.5-4.5 USM".to_string(),
            229 => "Canon EF 16-35mm f/2.8L USM".to_string(),
            230 => "Canon EF 24-70mm f/2.8L USM".to_string(),
            231 => "Canon EF 17-40mm f/4L USM or Sigma Lens".to_string(),
            232 => "Canon EF 70-300mm f/4.5-5.6 DO IS USM".to_string(),
            233 => "Canon EF 28-300mm f/3.5-5.6L IS USM".to_string(),
            234 => "Canon EF-S 17-85mm f/4-5.6 IS USM or Tokina Lens".to_string(),
            235 => "Canon EF-S 10-22mm f/3.5-4.5 USM".to_string(),
            236 => "Canon EF-S 60mm f/2.8 Macro USM".to_string(),
            237 => "Canon EF 24-105mm f/4L IS USM".to_string(),
            238 => "Canon EF 70-300mm f/4-5.6 IS USM".to_string(),
            239 => "Canon EF 85mm f/1.2L II USM or Rokinon Lens".to_string(),
            240 => "Canon EF-S 17-55mm f/2.8 IS USM or Sigma Lens".to_string(),
            241 => "Canon EF 50mm f/1.2L USM".to_string(),
            242 => "Canon EF 70-200mm f/4L IS USM".to_string(),
            243 => "Canon EF 70-200mm f/4L IS USM + 1.4x".to_string(),
            244 => "Canon EF 70-200mm f/4L IS USM + 2x".to_string(),
            245 => "Canon EF 70-200mm f/4L IS USM + 2.8x".to_string(),
            246 => "Canon EF 16-35mm f/2.8L II USM".to_string(),
            247 => "Canon EF 14mm f/2.8L II USM".to_string(),
            248 => "Canon EF 200mm f/2L IS USM or Sigma Lens".to_string(),
            249 => "Canon EF 800mm f/5.6L IS USM".to_string(),
            250 => "Canon EF 24mm f/1.4L II USM or Sigma Lens".to_string(),
            251 => "Canon EF 70-200mm f/2.8L IS II USM".to_string(),
            252 => "Canon EF 70-200mm f/2.8L IS II USM + 1.4x".to_string(),
            253 => "Canon EF 70-200mm f/2.8L IS II USM + 2x".to_string(),
            254 => "Canon EF 100mm f/2.8L Macro IS USM or Tamron Lens".to_string(),
            255 => "Sigma 24-105mm f/4 DG OS HSM | A or Other Lens".to_string(),
            368 => "Sigma 14-24mm f/2.8 DG HSM | A or other Sigma Lens".to_string(),
            488 => "Canon EF-S 15-85mm f/3.5-5.6 IS USM".to_string(),
            489 => "Canon EF 70-300mm f/4-5.6L IS USM".to_string(),
            490 => "Canon EF 8-15mm f/4L Fisheye USM".to_string(),
            491 => "Canon EF 300mm f/2.8L IS II USM or Tamron Lens".to_string(),
            492 => "Canon EF 400mm f/2.8L IS II USM".to_string(),
            493 => "Canon EF 500mm f/4L IS II USM or EF 24-105mm f4L IS USM".to_string(),
            494 => "Canon EF 600mm f/4L IS II USM".to_string(),
            495 => "Canon EF 24-70mm f/2.8L II USM or Sigma Lens".to_string(),
            496 => "Canon EF 200-400mm f/4L IS USM".to_string(),
            499 => "Canon EF 200-400mm f/4L IS USM + 1.4x".to_string(),
            502 => "Canon EF 28mm f/2.8 IS USM or Tamron Lens".to_string(),
            503 => "Canon EF 24mm f/2.8 IS USM".to_string(),
            504 => "Canon EF 24-70mm f/4L IS USM".to_string(),
            505 => "Canon EF 35mm f/2 IS USM".to_string(),
            506 => "Canon EF 400mm f/4 DO IS II USM".to_string(),
            507 => "Canon EF 16-35mm f/4L IS USM".to_string(),
            508 => "Canon EF 11-24mm f/4L USM or Tamron Lens".to_string(),
            624 => "Sigma 70-200mm f/2.8 DG OS HSM | S or other Sigma Lens".to_string(),
            747 => "Canon EF 100-400mm f/4.5-5.6L IS II USM or Tamron Lens".to_string(),
            748 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 1.4x or Tamron Lens".to_string(),
            749 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 2x or Tamron Lens".to_string(),
            750 => "Canon EF 35mm f/1.4L II USM or Tamron Lens".to_string(),
            751 => "Canon EF 16-35mm f/2.8L III USM".to_string(),
            752 => "Canon EF 24-105mm f/4L IS II USM".to_string(),
            753 => "Canon EF 85mm f/1.4L IS USM".to_string(),
            754 => "Canon EF 70-200mm f/4L IS II USM".to_string(),
            757 => "Canon EF 400mm f/2.8L IS III USM".to_string(),
            758 => "Canon EF 600mm f/4L IS III USM".to_string(),
            923 => "Meike/SKY 85mm f/1.8 DCM".to_string(),
            1136 => "Sigma 24-70mm f/2.8 DG OS HSM | A".to_string(),
            4142 => "Canon EF-S 18-135mm f/3.5-5.6 IS STM".to_string(),
            4143 => "Canon EF-M 18-55mm f/3.5-5.6 IS STM or Tamron Lens".to_string(),
            4144 => "Canon EF 40mm f/2.8 STM".to_string(),
            4145 => "Canon EF-M 22mm f/2 STM".to_string(),
            4146 => "Canon EF-S 18-55mm f/3.5-5.6 IS STM".to_string(),
            4147 => "Canon EF-M 11-22mm f/4-5.6 IS STM".to_string(),
            4148 => "Canon EF-S 55-250mm f/4-5.6 IS STM".to_string(),
            4149 => "Canon EF-M 55-200mm f/4.5-6.3 IS STM".to_string(),
            4150 => "Canon EF-S 10-18mm f/4.5-5.6 IS STM".to_string(),
            4152 => "Canon EF 24-105mm f/3.5-5.6 IS STM".to_string(),
            4153 => "Canon EF-M 15-45mm f/3.5-6.3 IS STM".to_string(),
            4154 => "Canon EF-S 24mm f/2.8 STM".to_string(),
            4155 => "Canon EF-M 28mm f/3.5 Macro IS STM".to_string(),
            4156 => "Canon EF 50mm f/1.8 STM".to_string(),
            4157 => "Canon EF-M 18-150mm f/3.5-6.3 IS STM".to_string(),
            4158 => "Canon EF-S 18-55mm f/4-5.6 IS STM".to_string(),
            4159 => "Canon EF-M 32mm f/1.4 STM".to_string(),
            4160 => "Canon EF-S 35mm f/2.8 Macro IS STM".to_string(),
            4208 => "Sigma 56mm f/1.4 DC DN | C or other Sigma Lens".to_string(),
            4976 => "Sigma 16-300mm F3.5-6.7 DC OS | C (025)".to_string(),
            6512 => "Sigma 12mm F1.4 DC | C".to_string(),
            36910 => "Canon EF 70-300mm f/4-5.6 IS II USM".to_string(),
            36912 => "Canon EF-S 18-135mm f/3.5-5.6 IS USM".to_string(),
            61182 => "Canon RF 50mm F1.2L USM or other Canon RF Lens".to_string(),
            61491 => "Canon CN-E 14mm T3.1 L F".to_string(),
            61492 => "Canon CN-E 24mm T1.5 L F".to_string(),
            61494 => "Canon CN-E 85mm T1.3 L F".to_string(),
            61495 => "Canon CN-E 135mm T2.2 L F".to_string(),
            61496 => "Canon CN-E 35mm T1.5 L F".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("LensType", 0x111, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x113, bo) {
        dm.push(("MinFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MinFocalLength", 0x113, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x115, bo) {
        dm.push(("MaxFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MaxFocalLength", 0x115, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(text) = text_at(data, 0x136, 6, true) {
        tags.push(mk("FirmwareVersion", 0x136, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x172, bo) {
        dm.push(("FileIndex".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val + 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("FileIndex", 0x172, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x176, bo) {
        dm.push(("ShutterCount".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val + 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("ShutterCount", 0x176, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x17e, bo) {
        dm.push(("DirectoryIndex".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("DirectoryIndex", 0x17e, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if MODEL_RE_5.is_match(model) {
        if let Some(v) = u32_at(data, 0x45a, bo) {
            dm.push(("TimeStamp1".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
            if rc.as_ref() != Some(&Conv::Undef) {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let v = rc.map_or(v, |x| x.as_num() as _);
                let mut cv = Conv::Num(f64::from(v));
                if let Some(x) = conv_expr::eval_with("ConvertUnixTime($val)", &cv, &ctx) { cv = x; }
                let raw = Value::F64(cv.as_num());
                if let Some(x) = conv_expr::eval_with("$self->ConvertDateTime($val)", &cv, &ctx) { cv = x; }
                tags.push(mk("TimeStamp1", 0x45a, cv.as_string(), raw, GRP1, GRP2, PRIO));
            }
        }
    }
    if let Some(v) = u32_at(data, 0x45e, bo) {
        dm.push(("TimeStamp".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("ConvertUnixTime($val)", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("$self->ConvertDateTime($val)", &cv, &ctx) { cv = x; }
            tags.push(mk("TimeStamp", 0x45e, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(sub) = data.get(0x2aa..) {
        tags.extend(canon_psinfo(sub, model, bo, file_type, format, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::CameraInfo1DmkIV` -- FORMAT int8u, FIRST_ENTRY 0.
fn canon_camerainfo1dmkiv(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(text) = text_at(data, 0x0, 509, false) {
        tags.push(mk("FirmwareVersionLookAhead", 0x0, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x3) {
        dm.push(("FNumber".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(($val-8)/16*log(2))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("sprintf(\"%.2g\",$val)", &cv, &ctx) { cv = x; }
            tags.push(mk("FNumber", 0x3, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x4) {
        dm.push(("ExposureTime".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(4*log(2)*(1-Image::ExifTool::Canon::CanonEv($val-24)))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("Image::ExifTool::Exif::PrintExposureTime($val)", &cv, &ctx) { cv = x; }
            tags.push(mk("ExposureTime", 0x4, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x6) {
        dm.push(("ISO".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("100*exp(($val/8-9)*log(2))", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("sprintf(\"%.0f\",$val)", &cv, &ctx) { cv = x; }
        tags.push(mk("ISO", 0x6, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x7) {
        dm.push(("HighlightTonePriority".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "People".to_string(),
            1 => "sRGB".to_string(),
            2 => "Adobe RGB".to_string(),
            3 => "User 1".to_string(),
            4 => "User 2".to_string(),
            5 => "User 3".to_string(),
            6 => "To Do".to_string(),
            65535 => "n/a".to_string(),
            2415919104 => "Format 1".to_string(),
            2684354560 => "Format 2".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("HighlightTonePriority", 0x7, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x8) {
        dm.push(("MeasuredEV2".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("$val / 8 - 6", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            tags.push(mk("MeasuredEV2", 0x8, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x9) {
        dm.push(("MeasuredEV3".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("$val / 8 - 6", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            tags.push(mk("MeasuredEV3", 0x9, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x15) {
        dm.push(("FlashMeteringMode".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "E-TTL".to_string(),
            3 => "TTL".to_string(),
            4 => "External Auto".to_string(),
            5 => "External Manual".to_string(),
            6 => "Off".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("FlashMeteringMode", 0x15, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x19) {
        dm.push(("CameraTemperature".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 128", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
        tags.push(mk("CameraTemperature", 0x19, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x1e, bo) {
        dm.push(("FocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
            tags.push(mk("FocalLength", 0x1e, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x35) {
        dm.push(("CameraOrientation".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Horizontal (normal)".to_string(),
            1 => "Rotate 90 CW".to_string(),
            2 => "Rotate 270 CW".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("CameraOrientation", 0x35, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x54, bo) {
        dm.push(("FocusDistanceUpper".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceUpper", 0x54, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x56, bo) {
        dm.push(("FocusDistanceLower".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceLower", 0x56, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x78, bo) {
        dm.push(("WhiteBalance".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Auto".to_string(),
            1 => "Daylight".to_string(),
            2 => "Cloudy".to_string(),
            3 => "Tungsten".to_string(),
            4 => "Fluorescent".to_string(),
            5 => "Flash".to_string(),
            6 => "Custom".to_string(),
            7 => "Black & White".to_string(),
            8 => "Shade".to_string(),
            9 => "Manual Temperature (Kelvin)".to_string(),
            10 => "PC Set1".to_string(),
            11 => "PC Set2".to_string(),
            12 => "PC Set3".to_string(),
            14 => "Daylight Fluorescent".to_string(),
            15 => "Custom 1".to_string(),
            16 => "Custom 2".to_string(),
            17 => "Underwater".to_string(),
            18 => "Custom 3".to_string(),
            19 => "Custom 4".to_string(),
            20 => "PC Set4".to_string(),
            21 => "PC Set5".to_string(),
            23 => "Auto (ambience priority)".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("WhiteBalance", 0x78, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x7c, bo) {
        dm.push(("ColorTemperature".to_string(), f64::from(v)));
        tags.push(mk("ColorTemperature", 0x7c, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x14f, bo) {
        dm.push(("LensType".to_string(), f64::from(v)));
        let s = match v as i64 {
            -1 => "n/a".to_string(),
            1 => "Canon EF 50mm f/1.8".to_string(),
            2 => "Canon EF 28mm f/2.8 or Sigma Lens".to_string(),
            3 => "Canon EF 135mm f/2.8 Soft".to_string(),
            4 => "Canon EF 35-105mm f/3.5-4.5 or Sigma Lens".to_string(),
            5 => "Canon EF 35-70mm f/3.5-4.5".to_string(),
            6 => "Canon EF 28-70mm f/3.5-4.5 or Sigma or Tokina Lens".to_string(),
            7 => "Canon EF 100-300mm f/5.6L".to_string(),
            8 => "Canon EF 100-300mm f/5.6 or Sigma or Tokina Lens".to_string(),
            9 => "Canon EF 70-210mm f/4".to_string(),
            10 => "Canon EF 50mm f/2.5 Macro or Sigma Lens".to_string(),
            11 => "Canon EF 35mm f/2".to_string(),
            13 => "Canon EF 15mm f/2.8 Fisheye".to_string(),
            14 => "Canon EF 50-200mm f/3.5-4.5L".to_string(),
            15 => "Canon EF 50-200mm f/3.5-4.5".to_string(),
            16 => "Canon EF 35-135mm f/3.5-4.5".to_string(),
            17 => "Canon EF 35-70mm f/3.5-4.5A".to_string(),
            18 => "Canon EF 28-70mm f/3.5-4.5".to_string(),
            20 => "Canon EF 100-200mm f/4.5A".to_string(),
            21 => "Canon EF 80-200mm f/2.8L".to_string(),
            22 => "Canon EF 20-35mm f/2.8L or Tokina Lens".to_string(),
            23 => "Canon EF 35-105mm f/3.5-4.5".to_string(),
            24 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            25 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            26 => "Canon EF 100mm f/2.8 Macro or Other Lens".to_string(),
            27 => "Canon EF 35-80mm f/4-5.6".to_string(),
            28 => "Canon EF 80-200mm f/4.5-5.6 or Tamron Lens".to_string(),
            29 => "Canon EF 50mm f/1.8 II".to_string(),
            30 => "Canon EF 35-105mm f/4.5-5.6".to_string(),
            31 => "Canon EF 75-300mm f/4-5.6 or Tamron Lens".to_string(),
            32 => "Canon EF 24mm f/2.8 or Sigma Lens".to_string(),
            33 => "Voigtlander or Carl Zeiss Lens".to_string(),
            35 => "Canon EF 35-80mm f/4-5.6".to_string(),
            36 => "Canon EF 38-76mm f/4.5-5.6".to_string(),
            37 => "Canon EF 35-80mm f/4-5.6 or Tamron Lens".to_string(),
            38 => "Canon EF 80-200mm f/4.5-5.6 II".to_string(),
            39 => "Canon EF 75-300mm f/4-5.6".to_string(),
            40 => "Canon EF 28-80mm f/3.5-5.6".to_string(),
            41 => "Canon EF 28-90mm f/4-5.6".to_string(),
            42 => "Canon EF 28-200mm f/3.5-5.6 or Tamron Lens".to_string(),
            43 => "Canon EF 28-105mm f/4-5.6".to_string(),
            44 => "Canon EF 90-300mm f/4.5-5.6".to_string(),
            45 => "Canon EF-S 18-55mm f/3.5-5.6 [II]".to_string(),
            46 => "Canon EF 28-90mm f/4-5.6".to_string(),
            47 => "Zeiss Milvus 35mm f/2 or 50mm f/2".to_string(),
            48 => "Canon EF-S 18-55mm f/3.5-5.6 IS".to_string(),
            49 => "Canon EF-S 55-250mm f/4-5.6 IS".to_string(),
            50 => "Canon EF-S 18-200mm f/3.5-5.6 IS".to_string(),
            51 => "Canon EF-S 18-135mm f/3.5-5.6 IS".to_string(),
            52 => "Canon EF-S 18-55mm f/3.5-5.6 IS II".to_string(),
            53 => "Canon EF-S 18-55mm f/3.5-5.6 III".to_string(),
            54 => "Canon EF-S 55-250mm f/4-5.6 IS II".to_string(),
            60 => "Irix 11mm f/4 or 15mm f/2.4".to_string(),
            63 => "Irix 30mm F1.4 Dragonfly".to_string(),
            80 => "Canon TS-E 50mm f/2.8L Macro".to_string(),
            81 => "Canon TS-E 90mm f/2.8L Macro".to_string(),
            82 => "Canon TS-E 135mm f/4L Macro".to_string(),
            94 => "Canon TS-E 17mm f/4L".to_string(),
            95 => "Canon TS-E 24mm f/3.5L II".to_string(),
            103 => "Samyang AF 14mm f/2.8 EF or Rokinon Lens".to_string(),
            106 => "Rokinon SP / Samyang XP 35mm f/1.2".to_string(),
            112 => "Sigma 28mm f/1.5 FF High-speed Prime or other Sigma Lens".to_string(),
            117 => "Tamron 35-150mm f/2.8-4.0 Di VC OSD (A043) or other Tamron Lens".to_string(),
            124 => "Canon MP-E 65mm f/2.8 1-5x Macro Photo".to_string(),
            125 => "Canon TS-E 24mm f/3.5L".to_string(),
            126 => "Canon TS-E 45mm f/2.8".to_string(),
            127 => "Canon TS-E 90mm f/2.8 or Tamron Lens".to_string(),
            129 => "Canon EF 300mm f/2.8L USM".to_string(),
            130 => "Canon EF 50mm f/1.0L USM".to_string(),
            131 => "Canon EF 28-80mm f/2.8-4L USM or Sigma Lens".to_string(),
            132 => "Canon EF 1200mm f/5.6L USM".to_string(),
            134 => "Canon EF 600mm f/4L IS USM".to_string(),
            135 => "Canon EF 200mm f/1.8L USM".to_string(),
            136 => "Canon EF 300mm f/2.8L USM".to_string(),
            137 => "Canon EF 85mm f/1.2L USM or Sigma or Tamron Lens".to_string(),
            138 => "Canon EF 28-80mm f/2.8-4L".to_string(),
            139 => "Canon EF 400mm f/2.8L USM".to_string(),
            140 => "Canon EF 500mm f/4.5L USM".to_string(),
            141 => "Canon EF 500mm f/4.5L USM".to_string(),
            142 => "Canon EF 300mm f/2.8L IS USM".to_string(),
            143 => "Canon EF 500mm f/4L IS USM or Sigma Lens".to_string(),
            144 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            145 => "Canon EF 100-300mm f/4.5-5.6 USM".to_string(),
            146 => "Canon EF 70-210mm f/3.5-4.5 USM".to_string(),
            147 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            148 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            149 => "Canon EF 100mm f/2 USM".to_string(),
            150 => "Canon EF 14mm f/2.8L USM or Sigma Lens".to_string(),
            151 => "Canon EF 200mm f/2.8L USM".to_string(),
            152 => "Canon EF 300mm f/4L IS USM or Sigma Lens".to_string(),
            153 => "Canon EF 35-350mm f/3.5-5.6L USM or Sigma or Tamron Lens".to_string(),
            154 => "Canon EF 20mm f/2.8 USM or Zeiss Lens".to_string(),
            155 => "Canon EF 85mm f/1.8 USM or Sigma Lens".to_string(),
            156 => "Canon EF 28-105mm f/3.5-4.5 USM or Tamron Lens".to_string(),
            160 => "Canon EF 20-35mm f/3.5-4.5 USM or Tamron or Tokina Lens".to_string(),
            161 => "Canon EF 28-70mm f/2.8L USM or Other Lens".to_string(),
            162 => "Canon EF 200mm f/2.8L USM".to_string(),
            163 => "Canon EF 300mm f/4L".to_string(),
            164 => "Canon EF 400mm f/5.6L".to_string(),
            165 => "Canon EF 70-200mm f/2.8L USM".to_string(),
            166 => "Canon EF 70-200mm f/2.8L USM + 1.4x".to_string(),
            167 => "Canon EF 70-200mm f/2.8L USM + 2x".to_string(),
            168 => "Canon EF 28mm f/1.8 USM or Sigma Lens".to_string(),
            169 => "Canon EF 17-35mm f/2.8L USM or Sigma Lens".to_string(),
            170 => "Canon EF 200mm f/2.8L II USM or Sigma Lens".to_string(),
            171 => "Canon EF 300mm f/4L USM".to_string(),
            172 => "Canon EF 400mm f/5.6L USM or Sigma Lens".to_string(),
            173 => "Canon EF 180mm Macro f/3.5L USM or Sigma Lens".to_string(),
            174 => "Canon EF 135mm f/2L USM or Other Lens".to_string(),
            175 => "Canon EF 400mm f/2.8L USM".to_string(),
            176 => "Canon EF 24-85mm f/3.5-4.5 USM".to_string(),
            177 => "Canon EF 300mm f/4L IS USM".to_string(),
            178 => "Canon EF 28-135mm f/3.5-5.6 IS".to_string(),
            179 => "Canon EF 24mm f/1.4L USM".to_string(),
            180 => "Canon EF 35mm f/1.4L USM or Other Lens".to_string(),
            181 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 1.4x or Sigma Lens".to_string(),
            182 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 2x or Sigma Lens".to_string(),
            183 => "Canon EF 100-400mm f/4.5-5.6L IS USM or Sigma Lens".to_string(),
            184 => "Canon EF 400mm f/2.8L USM + 2x".to_string(),
            185 => "Canon EF 600mm f/4L IS USM".to_string(),
            186 => "Canon EF 70-200mm f/4L USM".to_string(),
            187 => "Canon EF 70-200mm f/4L USM + 1.4x".to_string(),
            188 => "Canon EF 70-200mm f/4L USM + 2x".to_string(),
            189 => "Canon EF 70-200mm f/4L USM + 2.8x".to_string(),
            190 => "Canon EF 100mm f/2.8 Macro USM".to_string(),
            191 => "Canon EF 400mm f/4 DO IS or Sigma Lens".to_string(),
            193 => "Canon EF 35-80mm f/4-5.6 USM".to_string(),
            194 => "Canon EF 80-200mm f/4.5-5.6 USM".to_string(),
            195 => "Canon EF 35-105mm f/4.5-5.6 USM".to_string(),
            196 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            197 => "Canon EF 75-300mm f/4-5.6 IS USM or Sigma Lens".to_string(),
            198 => "Canon EF 50mm f/1.4 USM or Other Lens".to_string(),
            199 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            200 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            201 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            202 => "Canon EF 28-80mm f/3.5-5.6 USM IV".to_string(),
            208 => "Canon EF 22-55mm f/4-5.6 USM".to_string(),
            209 => "Canon EF 55-200mm f/4.5-5.6".to_string(),
            210 => "Canon EF 28-90mm f/4-5.6 USM".to_string(),
            211 => "Canon EF 28-200mm f/3.5-5.6 USM".to_string(),
            212 => "Canon EF 28-105mm f/4-5.6 USM".to_string(),
            213 => "Canon EF 90-300mm f/4.5-5.6 USM or Tamron Lens".to_string(),
            214 => "Canon EF-S 18-55mm f/3.5-5.6 USM".to_string(),
            215 => "Canon EF 55-200mm f/4.5-5.6 II USM".to_string(),
            217 => "Tamron AF 18-270mm f/3.5-6.3 Di II VC PZD".to_string(),
            220 => "Yongnuo YN 50mm f/1.8".to_string(),
            224 => "Canon EF 70-200mm f/2.8L IS USM".to_string(),
            225 => "Canon EF 70-200mm f/2.8L IS USM + 1.4x".to_string(),
            226 => "Canon EF 70-200mm f/2.8L IS USM + 2x".to_string(),
            227 => "Canon EF 70-200mm f/2.8L IS USM + 2.8x".to_string(),
            228 => "Canon EF 28-105mm f/3.5-4.5 USM".to_string(),
            229 => "Canon EF 16-35mm f/2.8L USM".to_string(),
            230 => "Canon EF 24-70mm f/2.8L USM".to_string(),
            231 => "Canon EF 17-40mm f/4L USM or Sigma Lens".to_string(),
            232 => "Canon EF 70-300mm f/4.5-5.6 DO IS USM".to_string(),
            233 => "Canon EF 28-300mm f/3.5-5.6L IS USM".to_string(),
            234 => "Canon EF-S 17-85mm f/4-5.6 IS USM or Tokina Lens".to_string(),
            235 => "Canon EF-S 10-22mm f/3.5-4.5 USM".to_string(),
            236 => "Canon EF-S 60mm f/2.8 Macro USM".to_string(),
            237 => "Canon EF 24-105mm f/4L IS USM".to_string(),
            238 => "Canon EF 70-300mm f/4-5.6 IS USM".to_string(),
            239 => "Canon EF 85mm f/1.2L II USM or Rokinon Lens".to_string(),
            240 => "Canon EF-S 17-55mm f/2.8 IS USM or Sigma Lens".to_string(),
            241 => "Canon EF 50mm f/1.2L USM".to_string(),
            242 => "Canon EF 70-200mm f/4L IS USM".to_string(),
            243 => "Canon EF 70-200mm f/4L IS USM + 1.4x".to_string(),
            244 => "Canon EF 70-200mm f/4L IS USM + 2x".to_string(),
            245 => "Canon EF 70-200mm f/4L IS USM + 2.8x".to_string(),
            246 => "Canon EF 16-35mm f/2.8L II USM".to_string(),
            247 => "Canon EF 14mm f/2.8L II USM".to_string(),
            248 => "Canon EF 200mm f/2L IS USM or Sigma Lens".to_string(),
            249 => "Canon EF 800mm f/5.6L IS USM".to_string(),
            250 => "Canon EF 24mm f/1.4L II USM or Sigma Lens".to_string(),
            251 => "Canon EF 70-200mm f/2.8L IS II USM".to_string(),
            252 => "Canon EF 70-200mm f/2.8L IS II USM + 1.4x".to_string(),
            253 => "Canon EF 70-200mm f/2.8L IS II USM + 2x".to_string(),
            254 => "Canon EF 100mm f/2.8L Macro IS USM or Tamron Lens".to_string(),
            255 => "Sigma 24-105mm f/4 DG OS HSM | A or Other Lens".to_string(),
            368 => "Sigma 14-24mm f/2.8 DG HSM | A or other Sigma Lens".to_string(),
            488 => "Canon EF-S 15-85mm f/3.5-5.6 IS USM".to_string(),
            489 => "Canon EF 70-300mm f/4-5.6L IS USM".to_string(),
            490 => "Canon EF 8-15mm f/4L Fisheye USM".to_string(),
            491 => "Canon EF 300mm f/2.8L IS II USM or Tamron Lens".to_string(),
            492 => "Canon EF 400mm f/2.8L IS II USM".to_string(),
            493 => "Canon EF 500mm f/4L IS II USM or EF 24-105mm f4L IS USM".to_string(),
            494 => "Canon EF 600mm f/4L IS II USM".to_string(),
            495 => "Canon EF 24-70mm f/2.8L II USM or Sigma Lens".to_string(),
            496 => "Canon EF 200-400mm f/4L IS USM".to_string(),
            499 => "Canon EF 200-400mm f/4L IS USM + 1.4x".to_string(),
            502 => "Canon EF 28mm f/2.8 IS USM or Tamron Lens".to_string(),
            503 => "Canon EF 24mm f/2.8 IS USM".to_string(),
            504 => "Canon EF 24-70mm f/4L IS USM".to_string(),
            505 => "Canon EF 35mm f/2 IS USM".to_string(),
            506 => "Canon EF 400mm f/4 DO IS II USM".to_string(),
            507 => "Canon EF 16-35mm f/4L IS USM".to_string(),
            508 => "Canon EF 11-24mm f/4L USM or Tamron Lens".to_string(),
            624 => "Sigma 70-200mm f/2.8 DG OS HSM | S or other Sigma Lens".to_string(),
            747 => "Canon EF 100-400mm f/4.5-5.6L IS II USM or Tamron Lens".to_string(),
            748 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 1.4x or Tamron Lens".to_string(),
            749 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 2x or Tamron Lens".to_string(),
            750 => "Canon EF 35mm f/1.4L II USM or Tamron Lens".to_string(),
            751 => "Canon EF 16-35mm f/2.8L III USM".to_string(),
            752 => "Canon EF 24-105mm f/4L IS II USM".to_string(),
            753 => "Canon EF 85mm f/1.4L IS USM".to_string(),
            754 => "Canon EF 70-200mm f/4L IS II USM".to_string(),
            757 => "Canon EF 400mm f/2.8L IS III USM".to_string(),
            758 => "Canon EF 600mm f/4L IS III USM".to_string(),
            923 => "Meike/SKY 85mm f/1.8 DCM".to_string(),
            1136 => "Sigma 24-70mm f/2.8 DG OS HSM | A".to_string(),
            4142 => "Canon EF-S 18-135mm f/3.5-5.6 IS STM".to_string(),
            4143 => "Canon EF-M 18-55mm f/3.5-5.6 IS STM or Tamron Lens".to_string(),
            4144 => "Canon EF 40mm f/2.8 STM".to_string(),
            4145 => "Canon EF-M 22mm f/2 STM".to_string(),
            4146 => "Canon EF-S 18-55mm f/3.5-5.6 IS STM".to_string(),
            4147 => "Canon EF-M 11-22mm f/4-5.6 IS STM".to_string(),
            4148 => "Canon EF-S 55-250mm f/4-5.6 IS STM".to_string(),
            4149 => "Canon EF-M 55-200mm f/4.5-6.3 IS STM".to_string(),
            4150 => "Canon EF-S 10-18mm f/4.5-5.6 IS STM".to_string(),
            4152 => "Canon EF 24-105mm f/3.5-5.6 IS STM".to_string(),
            4153 => "Canon EF-M 15-45mm f/3.5-6.3 IS STM".to_string(),
            4154 => "Canon EF-S 24mm f/2.8 STM".to_string(),
            4155 => "Canon EF-M 28mm f/3.5 Macro IS STM".to_string(),
            4156 => "Canon EF 50mm f/1.8 STM".to_string(),
            4157 => "Canon EF-M 18-150mm f/3.5-6.3 IS STM".to_string(),
            4158 => "Canon EF-S 18-55mm f/4-5.6 IS STM".to_string(),
            4159 => "Canon EF-M 32mm f/1.4 STM".to_string(),
            4160 => "Canon EF-S 35mm f/2.8 Macro IS STM".to_string(),
            4208 => "Sigma 56mm f/1.4 DC DN | C or other Sigma Lens".to_string(),
            4976 => "Sigma 16-300mm F3.5-6.7 DC OS | C (025)".to_string(),
            6512 => "Sigma 12mm F1.4 DC | C".to_string(),
            36910 => "Canon EF 70-300mm f/4-5.6 IS II USM".to_string(),
            36912 => "Canon EF-S 18-135mm f/3.5-5.6 IS USM".to_string(),
            61182 => "Canon RF 50mm F1.2L USM or other Canon RF Lens".to_string(),
            61491 => "Canon CN-E 14mm T3.1 L F".to_string(),
            61492 => "Canon CN-E 24mm T1.5 L F".to_string(),
            61494 => "Canon CN-E 85mm T1.3 L F".to_string(),
            61495 => "Canon CN-E 135mm T2.2 L F".to_string(),
            61496 => "Canon CN-E 35mm T1.5 L F".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("LensType", 0x14f, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x151, bo) {
        dm.push(("MinFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MinFocalLength", 0x151, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x153, bo) {
        dm.push(("MaxFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MaxFocalLength", 0x153, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(text) = text_at(data, 0x1ed, 6, true) {
        tags.push(mk("FirmwareVersion", 0x1ed, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x22c, bo) {
        dm.push(("FileIndex".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val + 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("FileIndex", 0x22c, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x238, bo) {
        dm.push(("DirectoryIndex".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("DirectoryIndex", 0x238, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(sub) = data.get(0x368..) {
        tags.extend(canon_psinfo(sub, model, bo, file_type, format, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::CameraInfo1DX` -- FORMAT int8u, FIRST_ENTRY 0.
fn canon_camerainfo1dx(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(text) = text_at(data, 0x0, 651, false) {
        tags.push(mk("FirmwareVersionLookAhead", 0x0, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x3) {
        dm.push(("FNumber".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(($val-8)/16*log(2))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("sprintf(\"%.2g\",$val)", &cv, &ctx) { cv = x; }
            tags.push(mk("FNumber", 0x3, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x4) {
        dm.push(("ExposureTime".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(4*log(2)*(1-Image::ExifTool::Canon::CanonEv($val-24)))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("Image::ExifTool::Exif::PrintExposureTime($val)", &cv, &ctx) { cv = x; }
            tags.push(mk("ExposureTime", 0x4, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x6) {
        dm.push(("ISO".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("100*exp(($val/8-9)*log(2))", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("sprintf(\"%.0f\",$val)", &cv, &ctx) { cv = x; }
        tags.push(mk("ISO", 0x6, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x1b) {
        dm.push(("CameraTemperature".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 128", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
        tags.push(mk("CameraTemperature", 0x1b, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x23, bo) {
        dm.push(("FocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
            tags.push(mk("FocalLength", 0x23, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x7d) {
        dm.push(("CameraOrientation".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Horizontal (normal)".to_string(),
            1 => "Rotate 90 CW".to_string(),
            2 => "Rotate 270 CW".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("CameraOrientation", 0x7d, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x8c, bo) {
        dm.push(("FocusDistanceUpper".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceUpper", 0x8c, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x8e, bo) {
        dm.push(("FocusDistanceLower".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceLower", 0x8e, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0xbc, bo) {
        dm.push(("WhiteBalance".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Auto".to_string(),
            1 => "Daylight".to_string(),
            2 => "Cloudy".to_string(),
            3 => "Tungsten".to_string(),
            4 => "Fluorescent".to_string(),
            5 => "Flash".to_string(),
            6 => "Custom".to_string(),
            7 => "Black & White".to_string(),
            8 => "Shade".to_string(),
            9 => "Manual Temperature (Kelvin)".to_string(),
            10 => "PC Set1".to_string(),
            11 => "PC Set2".to_string(),
            12 => "PC Set3".to_string(),
            14 => "Daylight Fluorescent".to_string(),
            15 => "Custom 1".to_string(),
            16 => "Custom 2".to_string(),
            17 => "Underwater".to_string(),
            18 => "Custom 3".to_string(),
            19 => "Custom 4".to_string(),
            20 => "PC Set4".to_string(),
            21 => "PC Set5".to_string(),
            23 => "Auto (ambience priority)".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("WhiteBalance", 0xbc, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0xc0, bo) {
        dm.push(("ColorTemperature".to_string(), f64::from(v)));
        tags.push(mk("ColorTemperature", 0xc0, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0xf4) {
        dm.push(("PictureStyle".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "None".to_string(),
            1 => "Standard".to_string(),
            2 => "Portrait".to_string(),
            3 => "High Saturation".to_string(),
            4 => "Adobe RGB".to_string(),
            5 => "Low Saturation".to_string(),
            6 => "CM Set 1".to_string(),
            7 => "CM Set 2".to_string(),
            33 => "User Def. 1".to_string(),
            34 => "User Def. 2".to_string(),
            35 => "User Def. 3".to_string(),
            65 => "PC 1".to_string(),
            66 => "PC 2".to_string(),
            67 => "PC 3".to_string(),
            129 => "Standard".to_string(),
            130 => "Portrait".to_string(),
            131 => "Landscape".to_string(),
            132 => "Neutral".to_string(),
            133 => "Faithful".to_string(),
            134 => "Monochrome".to_string(),
            135 => "Auto".to_string(),
            136 => "Fine Detail".to_string(),
            255 => "n/a".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("PictureStyle", 0xf4, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x1a7, bo) {
        dm.push(("LensType".to_string(), f64::from(v)));
        let s = match v as i64 {
            -1 => "n/a".to_string(),
            1 => "Canon EF 50mm f/1.8".to_string(),
            2 => "Canon EF 28mm f/2.8 or Sigma Lens".to_string(),
            3 => "Canon EF 135mm f/2.8 Soft".to_string(),
            4 => "Canon EF 35-105mm f/3.5-4.5 or Sigma Lens".to_string(),
            5 => "Canon EF 35-70mm f/3.5-4.5".to_string(),
            6 => "Canon EF 28-70mm f/3.5-4.5 or Sigma or Tokina Lens".to_string(),
            7 => "Canon EF 100-300mm f/5.6L".to_string(),
            8 => "Canon EF 100-300mm f/5.6 or Sigma or Tokina Lens".to_string(),
            9 => "Canon EF 70-210mm f/4".to_string(),
            10 => "Canon EF 50mm f/2.5 Macro or Sigma Lens".to_string(),
            11 => "Canon EF 35mm f/2".to_string(),
            13 => "Canon EF 15mm f/2.8 Fisheye".to_string(),
            14 => "Canon EF 50-200mm f/3.5-4.5L".to_string(),
            15 => "Canon EF 50-200mm f/3.5-4.5".to_string(),
            16 => "Canon EF 35-135mm f/3.5-4.5".to_string(),
            17 => "Canon EF 35-70mm f/3.5-4.5A".to_string(),
            18 => "Canon EF 28-70mm f/3.5-4.5".to_string(),
            20 => "Canon EF 100-200mm f/4.5A".to_string(),
            21 => "Canon EF 80-200mm f/2.8L".to_string(),
            22 => "Canon EF 20-35mm f/2.8L or Tokina Lens".to_string(),
            23 => "Canon EF 35-105mm f/3.5-4.5".to_string(),
            24 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            25 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            26 => "Canon EF 100mm f/2.8 Macro or Other Lens".to_string(),
            27 => "Canon EF 35-80mm f/4-5.6".to_string(),
            28 => "Canon EF 80-200mm f/4.5-5.6 or Tamron Lens".to_string(),
            29 => "Canon EF 50mm f/1.8 II".to_string(),
            30 => "Canon EF 35-105mm f/4.5-5.6".to_string(),
            31 => "Canon EF 75-300mm f/4-5.6 or Tamron Lens".to_string(),
            32 => "Canon EF 24mm f/2.8 or Sigma Lens".to_string(),
            33 => "Voigtlander or Carl Zeiss Lens".to_string(),
            35 => "Canon EF 35-80mm f/4-5.6".to_string(),
            36 => "Canon EF 38-76mm f/4.5-5.6".to_string(),
            37 => "Canon EF 35-80mm f/4-5.6 or Tamron Lens".to_string(),
            38 => "Canon EF 80-200mm f/4.5-5.6 II".to_string(),
            39 => "Canon EF 75-300mm f/4-5.6".to_string(),
            40 => "Canon EF 28-80mm f/3.5-5.6".to_string(),
            41 => "Canon EF 28-90mm f/4-5.6".to_string(),
            42 => "Canon EF 28-200mm f/3.5-5.6 or Tamron Lens".to_string(),
            43 => "Canon EF 28-105mm f/4-5.6".to_string(),
            44 => "Canon EF 90-300mm f/4.5-5.6".to_string(),
            45 => "Canon EF-S 18-55mm f/3.5-5.6 [II]".to_string(),
            46 => "Canon EF 28-90mm f/4-5.6".to_string(),
            47 => "Zeiss Milvus 35mm f/2 or 50mm f/2".to_string(),
            48 => "Canon EF-S 18-55mm f/3.5-5.6 IS".to_string(),
            49 => "Canon EF-S 55-250mm f/4-5.6 IS".to_string(),
            50 => "Canon EF-S 18-200mm f/3.5-5.6 IS".to_string(),
            51 => "Canon EF-S 18-135mm f/3.5-5.6 IS".to_string(),
            52 => "Canon EF-S 18-55mm f/3.5-5.6 IS II".to_string(),
            53 => "Canon EF-S 18-55mm f/3.5-5.6 III".to_string(),
            54 => "Canon EF-S 55-250mm f/4-5.6 IS II".to_string(),
            60 => "Irix 11mm f/4 or 15mm f/2.4".to_string(),
            63 => "Irix 30mm F1.4 Dragonfly".to_string(),
            80 => "Canon TS-E 50mm f/2.8L Macro".to_string(),
            81 => "Canon TS-E 90mm f/2.8L Macro".to_string(),
            82 => "Canon TS-E 135mm f/4L Macro".to_string(),
            94 => "Canon TS-E 17mm f/4L".to_string(),
            95 => "Canon TS-E 24mm f/3.5L II".to_string(),
            103 => "Samyang AF 14mm f/2.8 EF or Rokinon Lens".to_string(),
            106 => "Rokinon SP / Samyang XP 35mm f/1.2".to_string(),
            112 => "Sigma 28mm f/1.5 FF High-speed Prime or other Sigma Lens".to_string(),
            117 => "Tamron 35-150mm f/2.8-4.0 Di VC OSD (A043) or other Tamron Lens".to_string(),
            124 => "Canon MP-E 65mm f/2.8 1-5x Macro Photo".to_string(),
            125 => "Canon TS-E 24mm f/3.5L".to_string(),
            126 => "Canon TS-E 45mm f/2.8".to_string(),
            127 => "Canon TS-E 90mm f/2.8 or Tamron Lens".to_string(),
            129 => "Canon EF 300mm f/2.8L USM".to_string(),
            130 => "Canon EF 50mm f/1.0L USM".to_string(),
            131 => "Canon EF 28-80mm f/2.8-4L USM or Sigma Lens".to_string(),
            132 => "Canon EF 1200mm f/5.6L USM".to_string(),
            134 => "Canon EF 600mm f/4L IS USM".to_string(),
            135 => "Canon EF 200mm f/1.8L USM".to_string(),
            136 => "Canon EF 300mm f/2.8L USM".to_string(),
            137 => "Canon EF 85mm f/1.2L USM or Sigma or Tamron Lens".to_string(),
            138 => "Canon EF 28-80mm f/2.8-4L".to_string(),
            139 => "Canon EF 400mm f/2.8L USM".to_string(),
            140 => "Canon EF 500mm f/4.5L USM".to_string(),
            141 => "Canon EF 500mm f/4.5L USM".to_string(),
            142 => "Canon EF 300mm f/2.8L IS USM".to_string(),
            143 => "Canon EF 500mm f/4L IS USM or Sigma Lens".to_string(),
            144 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            145 => "Canon EF 100-300mm f/4.5-5.6 USM".to_string(),
            146 => "Canon EF 70-210mm f/3.5-4.5 USM".to_string(),
            147 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            148 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            149 => "Canon EF 100mm f/2 USM".to_string(),
            150 => "Canon EF 14mm f/2.8L USM or Sigma Lens".to_string(),
            151 => "Canon EF 200mm f/2.8L USM".to_string(),
            152 => "Canon EF 300mm f/4L IS USM or Sigma Lens".to_string(),
            153 => "Canon EF 35-350mm f/3.5-5.6L USM or Sigma or Tamron Lens".to_string(),
            154 => "Canon EF 20mm f/2.8 USM or Zeiss Lens".to_string(),
            155 => "Canon EF 85mm f/1.8 USM or Sigma Lens".to_string(),
            156 => "Canon EF 28-105mm f/3.5-4.5 USM or Tamron Lens".to_string(),
            160 => "Canon EF 20-35mm f/3.5-4.5 USM or Tamron or Tokina Lens".to_string(),
            161 => "Canon EF 28-70mm f/2.8L USM or Other Lens".to_string(),
            162 => "Canon EF 200mm f/2.8L USM".to_string(),
            163 => "Canon EF 300mm f/4L".to_string(),
            164 => "Canon EF 400mm f/5.6L".to_string(),
            165 => "Canon EF 70-200mm f/2.8L USM".to_string(),
            166 => "Canon EF 70-200mm f/2.8L USM + 1.4x".to_string(),
            167 => "Canon EF 70-200mm f/2.8L USM + 2x".to_string(),
            168 => "Canon EF 28mm f/1.8 USM or Sigma Lens".to_string(),
            169 => "Canon EF 17-35mm f/2.8L USM or Sigma Lens".to_string(),
            170 => "Canon EF 200mm f/2.8L II USM or Sigma Lens".to_string(),
            171 => "Canon EF 300mm f/4L USM".to_string(),
            172 => "Canon EF 400mm f/5.6L USM or Sigma Lens".to_string(),
            173 => "Canon EF 180mm Macro f/3.5L USM or Sigma Lens".to_string(),
            174 => "Canon EF 135mm f/2L USM or Other Lens".to_string(),
            175 => "Canon EF 400mm f/2.8L USM".to_string(),
            176 => "Canon EF 24-85mm f/3.5-4.5 USM".to_string(),
            177 => "Canon EF 300mm f/4L IS USM".to_string(),
            178 => "Canon EF 28-135mm f/3.5-5.6 IS".to_string(),
            179 => "Canon EF 24mm f/1.4L USM".to_string(),
            180 => "Canon EF 35mm f/1.4L USM or Other Lens".to_string(),
            181 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 1.4x or Sigma Lens".to_string(),
            182 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 2x or Sigma Lens".to_string(),
            183 => "Canon EF 100-400mm f/4.5-5.6L IS USM or Sigma Lens".to_string(),
            184 => "Canon EF 400mm f/2.8L USM + 2x".to_string(),
            185 => "Canon EF 600mm f/4L IS USM".to_string(),
            186 => "Canon EF 70-200mm f/4L USM".to_string(),
            187 => "Canon EF 70-200mm f/4L USM + 1.4x".to_string(),
            188 => "Canon EF 70-200mm f/4L USM + 2x".to_string(),
            189 => "Canon EF 70-200mm f/4L USM + 2.8x".to_string(),
            190 => "Canon EF 100mm f/2.8 Macro USM".to_string(),
            191 => "Canon EF 400mm f/4 DO IS or Sigma Lens".to_string(),
            193 => "Canon EF 35-80mm f/4-5.6 USM".to_string(),
            194 => "Canon EF 80-200mm f/4.5-5.6 USM".to_string(),
            195 => "Canon EF 35-105mm f/4.5-5.6 USM".to_string(),
            196 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            197 => "Canon EF 75-300mm f/4-5.6 IS USM or Sigma Lens".to_string(),
            198 => "Canon EF 50mm f/1.4 USM or Other Lens".to_string(),
            199 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            200 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            201 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            202 => "Canon EF 28-80mm f/3.5-5.6 USM IV".to_string(),
            208 => "Canon EF 22-55mm f/4-5.6 USM".to_string(),
            209 => "Canon EF 55-200mm f/4.5-5.6".to_string(),
            210 => "Canon EF 28-90mm f/4-5.6 USM".to_string(),
            211 => "Canon EF 28-200mm f/3.5-5.6 USM".to_string(),
            212 => "Canon EF 28-105mm f/4-5.6 USM".to_string(),
            213 => "Canon EF 90-300mm f/4.5-5.6 USM or Tamron Lens".to_string(),
            214 => "Canon EF-S 18-55mm f/3.5-5.6 USM".to_string(),
            215 => "Canon EF 55-200mm f/4.5-5.6 II USM".to_string(),
            217 => "Tamron AF 18-270mm f/3.5-6.3 Di II VC PZD".to_string(),
            220 => "Yongnuo YN 50mm f/1.8".to_string(),
            224 => "Canon EF 70-200mm f/2.8L IS USM".to_string(),
            225 => "Canon EF 70-200mm f/2.8L IS USM + 1.4x".to_string(),
            226 => "Canon EF 70-200mm f/2.8L IS USM + 2x".to_string(),
            227 => "Canon EF 70-200mm f/2.8L IS USM + 2.8x".to_string(),
            228 => "Canon EF 28-105mm f/3.5-4.5 USM".to_string(),
            229 => "Canon EF 16-35mm f/2.8L USM".to_string(),
            230 => "Canon EF 24-70mm f/2.8L USM".to_string(),
            231 => "Canon EF 17-40mm f/4L USM or Sigma Lens".to_string(),
            232 => "Canon EF 70-300mm f/4.5-5.6 DO IS USM".to_string(),
            233 => "Canon EF 28-300mm f/3.5-5.6L IS USM".to_string(),
            234 => "Canon EF-S 17-85mm f/4-5.6 IS USM or Tokina Lens".to_string(),
            235 => "Canon EF-S 10-22mm f/3.5-4.5 USM".to_string(),
            236 => "Canon EF-S 60mm f/2.8 Macro USM".to_string(),
            237 => "Canon EF 24-105mm f/4L IS USM".to_string(),
            238 => "Canon EF 70-300mm f/4-5.6 IS USM".to_string(),
            239 => "Canon EF 85mm f/1.2L II USM or Rokinon Lens".to_string(),
            240 => "Canon EF-S 17-55mm f/2.8 IS USM or Sigma Lens".to_string(),
            241 => "Canon EF 50mm f/1.2L USM".to_string(),
            242 => "Canon EF 70-200mm f/4L IS USM".to_string(),
            243 => "Canon EF 70-200mm f/4L IS USM + 1.4x".to_string(),
            244 => "Canon EF 70-200mm f/4L IS USM + 2x".to_string(),
            245 => "Canon EF 70-200mm f/4L IS USM + 2.8x".to_string(),
            246 => "Canon EF 16-35mm f/2.8L II USM".to_string(),
            247 => "Canon EF 14mm f/2.8L II USM".to_string(),
            248 => "Canon EF 200mm f/2L IS USM or Sigma Lens".to_string(),
            249 => "Canon EF 800mm f/5.6L IS USM".to_string(),
            250 => "Canon EF 24mm f/1.4L II USM or Sigma Lens".to_string(),
            251 => "Canon EF 70-200mm f/2.8L IS II USM".to_string(),
            252 => "Canon EF 70-200mm f/2.8L IS II USM + 1.4x".to_string(),
            253 => "Canon EF 70-200mm f/2.8L IS II USM + 2x".to_string(),
            254 => "Canon EF 100mm f/2.8L Macro IS USM or Tamron Lens".to_string(),
            255 => "Sigma 24-105mm f/4 DG OS HSM | A or Other Lens".to_string(),
            368 => "Sigma 14-24mm f/2.8 DG HSM | A or other Sigma Lens".to_string(),
            488 => "Canon EF-S 15-85mm f/3.5-5.6 IS USM".to_string(),
            489 => "Canon EF 70-300mm f/4-5.6L IS USM".to_string(),
            490 => "Canon EF 8-15mm f/4L Fisheye USM".to_string(),
            491 => "Canon EF 300mm f/2.8L IS II USM or Tamron Lens".to_string(),
            492 => "Canon EF 400mm f/2.8L IS II USM".to_string(),
            493 => "Canon EF 500mm f/4L IS II USM or EF 24-105mm f4L IS USM".to_string(),
            494 => "Canon EF 600mm f/4L IS II USM".to_string(),
            495 => "Canon EF 24-70mm f/2.8L II USM or Sigma Lens".to_string(),
            496 => "Canon EF 200-400mm f/4L IS USM".to_string(),
            499 => "Canon EF 200-400mm f/4L IS USM + 1.4x".to_string(),
            502 => "Canon EF 28mm f/2.8 IS USM or Tamron Lens".to_string(),
            503 => "Canon EF 24mm f/2.8 IS USM".to_string(),
            504 => "Canon EF 24-70mm f/4L IS USM".to_string(),
            505 => "Canon EF 35mm f/2 IS USM".to_string(),
            506 => "Canon EF 400mm f/4 DO IS II USM".to_string(),
            507 => "Canon EF 16-35mm f/4L IS USM".to_string(),
            508 => "Canon EF 11-24mm f/4L USM or Tamron Lens".to_string(),
            624 => "Sigma 70-200mm f/2.8 DG OS HSM | S or other Sigma Lens".to_string(),
            747 => "Canon EF 100-400mm f/4.5-5.6L IS II USM or Tamron Lens".to_string(),
            748 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 1.4x or Tamron Lens".to_string(),
            749 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 2x or Tamron Lens".to_string(),
            750 => "Canon EF 35mm f/1.4L II USM or Tamron Lens".to_string(),
            751 => "Canon EF 16-35mm f/2.8L III USM".to_string(),
            752 => "Canon EF 24-105mm f/4L IS II USM".to_string(),
            753 => "Canon EF 85mm f/1.4L IS USM".to_string(),
            754 => "Canon EF 70-200mm f/4L IS II USM".to_string(),
            757 => "Canon EF 400mm f/2.8L IS III USM".to_string(),
            758 => "Canon EF 600mm f/4L IS III USM".to_string(),
            923 => "Meike/SKY 85mm f/1.8 DCM".to_string(),
            1136 => "Sigma 24-70mm f/2.8 DG OS HSM | A".to_string(),
            4142 => "Canon EF-S 18-135mm f/3.5-5.6 IS STM".to_string(),
            4143 => "Canon EF-M 18-55mm f/3.5-5.6 IS STM or Tamron Lens".to_string(),
            4144 => "Canon EF 40mm f/2.8 STM".to_string(),
            4145 => "Canon EF-M 22mm f/2 STM".to_string(),
            4146 => "Canon EF-S 18-55mm f/3.5-5.6 IS STM".to_string(),
            4147 => "Canon EF-M 11-22mm f/4-5.6 IS STM".to_string(),
            4148 => "Canon EF-S 55-250mm f/4-5.6 IS STM".to_string(),
            4149 => "Canon EF-M 55-200mm f/4.5-6.3 IS STM".to_string(),
            4150 => "Canon EF-S 10-18mm f/4.5-5.6 IS STM".to_string(),
            4152 => "Canon EF 24-105mm f/3.5-5.6 IS STM".to_string(),
            4153 => "Canon EF-M 15-45mm f/3.5-6.3 IS STM".to_string(),
            4154 => "Canon EF-S 24mm f/2.8 STM".to_string(),
            4155 => "Canon EF-M 28mm f/3.5 Macro IS STM".to_string(),
            4156 => "Canon EF 50mm f/1.8 STM".to_string(),
            4157 => "Canon EF-M 18-150mm f/3.5-6.3 IS STM".to_string(),
            4158 => "Canon EF-S 18-55mm f/4-5.6 IS STM".to_string(),
            4159 => "Canon EF-M 32mm f/1.4 STM".to_string(),
            4160 => "Canon EF-S 35mm f/2.8 Macro IS STM".to_string(),
            4208 => "Sigma 56mm f/1.4 DC DN | C or other Sigma Lens".to_string(),
            4976 => "Sigma 16-300mm F3.5-6.7 DC OS | C (025)".to_string(),
            6512 => "Sigma 12mm F1.4 DC | C".to_string(),
            36910 => "Canon EF 70-300mm f/4-5.6 IS II USM".to_string(),
            36912 => "Canon EF-S 18-135mm f/3.5-5.6 IS USM".to_string(),
            61182 => "Canon RF 50mm F1.2L USM or other Canon RF Lens".to_string(),
            61491 => "Canon CN-E 14mm T3.1 L F".to_string(),
            61492 => "Canon CN-E 24mm T1.5 L F".to_string(),
            61494 => "Canon CN-E 85mm T1.3 L F".to_string(),
            61495 => "Canon CN-E 135mm T2.2 L F".to_string(),
            61496 => "Canon CN-E 35mm T1.5 L F".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("LensType", 0x1a7, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x1a9, bo) {
        dm.push(("MinFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MinFocalLength", 0x1a9, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x1ab, bo) {
        dm.push(("MaxFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MaxFocalLength", 0x1ab, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(text) = text_at(data, 0x280, 6, true) {
        tags.push(mk("FirmwareVersion", 0x280, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x2d0, bo) {
        dm.push(("FileIndex".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val + 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("FileIndex", 0x2d0, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x2dc, bo) {
        dm.push(("DirectoryIndex".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("DirectoryIndex", 0x2dc, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(sub) = data.get(0x3f4..) {
        tags.extend(canon_psinfo2(sub, model, bo, file_type, format, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::CameraInfo5D` -- FORMAT int8s, FIRST_ENTRY 0.
fn canon_camerainfo5d(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = u8_at(data, 0x3) {
        dm.push(("FNumber".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(($val-8)/16*log(2))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("sprintf(\"%.2g\",$val)", &cv, &ctx) { cv = x; }
            tags.push(mk("FNumber", 0x3, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x4) {
        dm.push(("ExposureTime".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(4*log(2)*(1-Image::ExifTool::Canon::CanonEv($val-24)))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("Image::ExifTool::Exif::PrintExposureTime($val)", &cv, &ctx) { cv = x; }
            tags.push(mk("ExposureTime", 0x4, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x6) {
        dm.push(("ISO".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("100*exp(($val/8-9)*log(2))", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("sprintf(\"%.0f\",$val)", &cv, &ctx) { cv = x; }
        tags.push(mk("ISO", 0x6, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0xc, bo) {
        dm.push(("LensType".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let s = match v as i64 {
                -1 => "n/a".to_string(),
                1 => "Canon EF 50mm f/1.8".to_string(),
                2 => "Canon EF 28mm f/2.8 or Sigma Lens".to_string(),
                3 => "Canon EF 135mm f/2.8 Soft".to_string(),
                4 => "Canon EF 35-105mm f/3.5-4.5 or Sigma Lens".to_string(),
                5 => "Canon EF 35-70mm f/3.5-4.5".to_string(),
                6 => "Canon EF 28-70mm f/3.5-4.5 or Sigma or Tokina Lens".to_string(),
                7 => "Canon EF 100-300mm f/5.6L".to_string(),
                8 => "Canon EF 100-300mm f/5.6 or Sigma or Tokina Lens".to_string(),
                9 => "Canon EF 70-210mm f/4".to_string(),
                10 => "Canon EF 50mm f/2.5 Macro or Sigma Lens".to_string(),
                11 => "Canon EF 35mm f/2".to_string(),
                13 => "Canon EF 15mm f/2.8 Fisheye".to_string(),
                14 => "Canon EF 50-200mm f/3.5-4.5L".to_string(),
                15 => "Canon EF 50-200mm f/3.5-4.5".to_string(),
                16 => "Canon EF 35-135mm f/3.5-4.5".to_string(),
                17 => "Canon EF 35-70mm f/3.5-4.5A".to_string(),
                18 => "Canon EF 28-70mm f/3.5-4.5".to_string(),
                20 => "Canon EF 100-200mm f/4.5A".to_string(),
                21 => "Canon EF 80-200mm f/2.8L".to_string(),
                22 => "Canon EF 20-35mm f/2.8L or Tokina Lens".to_string(),
                23 => "Canon EF 35-105mm f/3.5-4.5".to_string(),
                24 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
                25 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
                26 => "Canon EF 100mm f/2.8 Macro or Other Lens".to_string(),
                27 => "Canon EF 35-80mm f/4-5.6".to_string(),
                28 => "Canon EF 80-200mm f/4.5-5.6 or Tamron Lens".to_string(),
                29 => "Canon EF 50mm f/1.8 II".to_string(),
                30 => "Canon EF 35-105mm f/4.5-5.6".to_string(),
                31 => "Canon EF 75-300mm f/4-5.6 or Tamron Lens".to_string(),
                32 => "Canon EF 24mm f/2.8 or Sigma Lens".to_string(),
                33 => "Voigtlander or Carl Zeiss Lens".to_string(),
                35 => "Canon EF 35-80mm f/4-5.6".to_string(),
                36 => "Canon EF 38-76mm f/4.5-5.6".to_string(),
                37 => "Canon EF 35-80mm f/4-5.6 or Tamron Lens".to_string(),
                38 => "Canon EF 80-200mm f/4.5-5.6 II".to_string(),
                39 => "Canon EF 75-300mm f/4-5.6".to_string(),
                40 => "Canon EF 28-80mm f/3.5-5.6".to_string(),
                41 => "Canon EF 28-90mm f/4-5.6".to_string(),
                42 => "Canon EF 28-200mm f/3.5-5.6 or Tamron Lens".to_string(),
                43 => "Canon EF 28-105mm f/4-5.6".to_string(),
                44 => "Canon EF 90-300mm f/4.5-5.6".to_string(),
                45 => "Canon EF-S 18-55mm f/3.5-5.6 [II]".to_string(),
                46 => "Canon EF 28-90mm f/4-5.6".to_string(),
                47 => "Zeiss Milvus 35mm f/2 or 50mm f/2".to_string(),
                48 => "Canon EF-S 18-55mm f/3.5-5.6 IS".to_string(),
                49 => "Canon EF-S 55-250mm f/4-5.6 IS".to_string(),
                50 => "Canon EF-S 18-200mm f/3.5-5.6 IS".to_string(),
                51 => "Canon EF-S 18-135mm f/3.5-5.6 IS".to_string(),
                52 => "Canon EF-S 18-55mm f/3.5-5.6 IS II".to_string(),
                53 => "Canon EF-S 18-55mm f/3.5-5.6 III".to_string(),
                54 => "Canon EF-S 55-250mm f/4-5.6 IS II".to_string(),
                60 => "Irix 11mm f/4 or 15mm f/2.4".to_string(),
                63 => "Irix 30mm F1.4 Dragonfly".to_string(),
                80 => "Canon TS-E 50mm f/2.8L Macro".to_string(),
                81 => "Canon TS-E 90mm f/2.8L Macro".to_string(),
                82 => "Canon TS-E 135mm f/4L Macro".to_string(),
                94 => "Canon TS-E 17mm f/4L".to_string(),
                95 => "Canon TS-E 24mm f/3.5L II".to_string(),
                103 => "Samyang AF 14mm f/2.8 EF or Rokinon Lens".to_string(),
                106 => "Rokinon SP / Samyang XP 35mm f/1.2".to_string(),
                112 => "Sigma 28mm f/1.5 FF High-speed Prime or other Sigma Lens".to_string(),
                117 => "Tamron 35-150mm f/2.8-4.0 Di VC OSD (A043) or other Tamron Lens".to_string(),
                124 => "Canon MP-E 65mm f/2.8 1-5x Macro Photo".to_string(),
                125 => "Canon TS-E 24mm f/3.5L".to_string(),
                126 => "Canon TS-E 45mm f/2.8".to_string(),
                127 => "Canon TS-E 90mm f/2.8 or Tamron Lens".to_string(),
                129 => "Canon EF 300mm f/2.8L USM".to_string(),
                130 => "Canon EF 50mm f/1.0L USM".to_string(),
                131 => "Canon EF 28-80mm f/2.8-4L USM or Sigma Lens".to_string(),
                132 => "Canon EF 1200mm f/5.6L USM".to_string(),
                134 => "Canon EF 600mm f/4L IS USM".to_string(),
                135 => "Canon EF 200mm f/1.8L USM".to_string(),
                136 => "Canon EF 300mm f/2.8L USM".to_string(),
                137 => "Canon EF 85mm f/1.2L USM or Sigma or Tamron Lens".to_string(),
                138 => "Canon EF 28-80mm f/2.8-4L".to_string(),
                139 => "Canon EF 400mm f/2.8L USM".to_string(),
                140 => "Canon EF 500mm f/4.5L USM".to_string(),
                141 => "Canon EF 500mm f/4.5L USM".to_string(),
                142 => "Canon EF 300mm f/2.8L IS USM".to_string(),
                143 => "Canon EF 500mm f/4L IS USM or Sigma Lens".to_string(),
                144 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
                145 => "Canon EF 100-300mm f/4.5-5.6 USM".to_string(),
                146 => "Canon EF 70-210mm f/3.5-4.5 USM".to_string(),
                147 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
                148 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
                149 => "Canon EF 100mm f/2 USM".to_string(),
                150 => "Canon EF 14mm f/2.8L USM or Sigma Lens".to_string(),
                151 => "Canon EF 200mm f/2.8L USM".to_string(),
                152 => "Canon EF 300mm f/4L IS USM or Sigma Lens".to_string(),
                153 => "Canon EF 35-350mm f/3.5-5.6L USM or Sigma or Tamron Lens".to_string(),
                154 => "Canon EF 20mm f/2.8 USM or Zeiss Lens".to_string(),
                155 => "Canon EF 85mm f/1.8 USM or Sigma Lens".to_string(),
                156 => "Canon EF 28-105mm f/3.5-4.5 USM or Tamron Lens".to_string(),
                160 => "Canon EF 20-35mm f/3.5-4.5 USM or Tamron or Tokina Lens".to_string(),
                161 => "Canon EF 28-70mm f/2.8L USM or Other Lens".to_string(),
                162 => "Canon EF 200mm f/2.8L USM".to_string(),
                163 => "Canon EF 300mm f/4L".to_string(),
                164 => "Canon EF 400mm f/5.6L".to_string(),
                165 => "Canon EF 70-200mm f/2.8L USM".to_string(),
                166 => "Canon EF 70-200mm f/2.8L USM + 1.4x".to_string(),
                167 => "Canon EF 70-200mm f/2.8L USM + 2x".to_string(),
                168 => "Canon EF 28mm f/1.8 USM or Sigma Lens".to_string(),
                169 => "Canon EF 17-35mm f/2.8L USM or Sigma Lens".to_string(),
                170 => "Canon EF 200mm f/2.8L II USM or Sigma Lens".to_string(),
                171 => "Canon EF 300mm f/4L USM".to_string(),
                172 => "Canon EF 400mm f/5.6L USM or Sigma Lens".to_string(),
                173 => "Canon EF 180mm Macro f/3.5L USM or Sigma Lens".to_string(),
                174 => "Canon EF 135mm f/2L USM or Other Lens".to_string(),
                175 => "Canon EF 400mm f/2.8L USM".to_string(),
                176 => "Canon EF 24-85mm f/3.5-4.5 USM".to_string(),
                177 => "Canon EF 300mm f/4L IS USM".to_string(),
                178 => "Canon EF 28-135mm f/3.5-5.6 IS".to_string(),
                179 => "Canon EF 24mm f/1.4L USM".to_string(),
                180 => "Canon EF 35mm f/1.4L USM or Other Lens".to_string(),
                181 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 1.4x or Sigma Lens".to_string(),
                182 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 2x or Sigma Lens".to_string(),
                183 => "Canon EF 100-400mm f/4.5-5.6L IS USM or Sigma Lens".to_string(),
                184 => "Canon EF 400mm f/2.8L USM + 2x".to_string(),
                185 => "Canon EF 600mm f/4L IS USM".to_string(),
                186 => "Canon EF 70-200mm f/4L USM".to_string(),
                187 => "Canon EF 70-200mm f/4L USM + 1.4x".to_string(),
                188 => "Canon EF 70-200mm f/4L USM + 2x".to_string(),
                189 => "Canon EF 70-200mm f/4L USM + 2.8x".to_string(),
                190 => "Canon EF 100mm f/2.8 Macro USM".to_string(),
                191 => "Canon EF 400mm f/4 DO IS or Sigma Lens".to_string(),
                193 => "Canon EF 35-80mm f/4-5.6 USM".to_string(),
                194 => "Canon EF 80-200mm f/4.5-5.6 USM".to_string(),
                195 => "Canon EF 35-105mm f/4.5-5.6 USM".to_string(),
                196 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
                197 => "Canon EF 75-300mm f/4-5.6 IS USM or Sigma Lens".to_string(),
                198 => "Canon EF 50mm f/1.4 USM or Other Lens".to_string(),
                199 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
                200 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
                201 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
                202 => "Canon EF 28-80mm f/3.5-5.6 USM IV".to_string(),
                208 => "Canon EF 22-55mm f/4-5.6 USM".to_string(),
                209 => "Canon EF 55-200mm f/4.5-5.6".to_string(),
                210 => "Canon EF 28-90mm f/4-5.6 USM".to_string(),
                211 => "Canon EF 28-200mm f/3.5-5.6 USM".to_string(),
                212 => "Canon EF 28-105mm f/4-5.6 USM".to_string(),
                213 => "Canon EF 90-300mm f/4.5-5.6 USM or Tamron Lens".to_string(),
                214 => "Canon EF-S 18-55mm f/3.5-5.6 USM".to_string(),
                215 => "Canon EF 55-200mm f/4.5-5.6 II USM".to_string(),
                217 => "Tamron AF 18-270mm f/3.5-6.3 Di II VC PZD".to_string(),
                220 => "Yongnuo YN 50mm f/1.8".to_string(),
                224 => "Canon EF 70-200mm f/2.8L IS USM".to_string(),
                225 => "Canon EF 70-200mm f/2.8L IS USM + 1.4x".to_string(),
                226 => "Canon EF 70-200mm f/2.8L IS USM + 2x".to_string(),
                227 => "Canon EF 70-200mm f/2.8L IS USM + 2.8x".to_string(),
                228 => "Canon EF 28-105mm f/3.5-4.5 USM".to_string(),
                229 => "Canon EF 16-35mm f/2.8L USM".to_string(),
                230 => "Canon EF 24-70mm f/2.8L USM".to_string(),
                231 => "Canon EF 17-40mm f/4L USM or Sigma Lens".to_string(),
                232 => "Canon EF 70-300mm f/4.5-5.6 DO IS USM".to_string(),
                233 => "Canon EF 28-300mm f/3.5-5.6L IS USM".to_string(),
                234 => "Canon EF-S 17-85mm f/4-5.6 IS USM or Tokina Lens".to_string(),
                235 => "Canon EF-S 10-22mm f/3.5-4.5 USM".to_string(),
                236 => "Canon EF-S 60mm f/2.8 Macro USM".to_string(),
                237 => "Canon EF 24-105mm f/4L IS USM".to_string(),
                238 => "Canon EF 70-300mm f/4-5.6 IS USM".to_string(),
                239 => "Canon EF 85mm f/1.2L II USM or Rokinon Lens".to_string(),
                240 => "Canon EF-S 17-55mm f/2.8 IS USM or Sigma Lens".to_string(),
                241 => "Canon EF 50mm f/1.2L USM".to_string(),
                242 => "Canon EF 70-200mm f/4L IS USM".to_string(),
                243 => "Canon EF 70-200mm f/4L IS USM + 1.4x".to_string(),
                244 => "Canon EF 70-200mm f/4L IS USM + 2x".to_string(),
                245 => "Canon EF 70-200mm f/4L IS USM + 2.8x".to_string(),
                246 => "Canon EF 16-35mm f/2.8L II USM".to_string(),
                247 => "Canon EF 14mm f/2.8L II USM".to_string(),
                248 => "Canon EF 200mm f/2L IS USM or Sigma Lens".to_string(),
                249 => "Canon EF 800mm f/5.6L IS USM".to_string(),
                250 => "Canon EF 24mm f/1.4L II USM or Sigma Lens".to_string(),
                251 => "Canon EF 70-200mm f/2.8L IS II USM".to_string(),
                252 => "Canon EF 70-200mm f/2.8L IS II USM + 1.4x".to_string(),
                253 => "Canon EF 70-200mm f/2.8L IS II USM + 2x".to_string(),
                254 => "Canon EF 100mm f/2.8L Macro IS USM or Tamron Lens".to_string(),
                255 => "Sigma 24-105mm f/4 DG OS HSM | A or Other Lens".to_string(),
                368 => "Sigma 14-24mm f/2.8 DG HSM | A or other Sigma Lens".to_string(),
                488 => "Canon EF-S 15-85mm f/3.5-5.6 IS USM".to_string(),
                489 => "Canon EF 70-300mm f/4-5.6L IS USM".to_string(),
                490 => "Canon EF 8-15mm f/4L Fisheye USM".to_string(),
                491 => "Canon EF 300mm f/2.8L IS II USM or Tamron Lens".to_string(),
                492 => "Canon EF 400mm f/2.8L IS II USM".to_string(),
                493 => "Canon EF 500mm f/4L IS II USM or EF 24-105mm f4L IS USM".to_string(),
                494 => "Canon EF 600mm f/4L IS II USM".to_string(),
                495 => "Canon EF 24-70mm f/2.8L II USM or Sigma Lens".to_string(),
                496 => "Canon EF 200-400mm f/4L IS USM".to_string(),
                499 => "Canon EF 200-400mm f/4L IS USM + 1.4x".to_string(),
                502 => "Canon EF 28mm f/2.8 IS USM or Tamron Lens".to_string(),
                503 => "Canon EF 24mm f/2.8 IS USM".to_string(),
                504 => "Canon EF 24-70mm f/4L IS USM".to_string(),
                505 => "Canon EF 35mm f/2 IS USM".to_string(),
                506 => "Canon EF 400mm f/4 DO IS II USM".to_string(),
                507 => "Canon EF 16-35mm f/4L IS USM".to_string(),
                508 => "Canon EF 11-24mm f/4L USM or Tamron Lens".to_string(),
                624 => "Sigma 70-200mm f/2.8 DG OS HSM | S or other Sigma Lens".to_string(),
                747 => "Canon EF 100-400mm f/4.5-5.6L IS II USM or Tamron Lens".to_string(),
                748 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 1.4x or Tamron Lens".to_string(),
                749 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 2x or Tamron Lens".to_string(),
                750 => "Canon EF 35mm f/1.4L II USM or Tamron Lens".to_string(),
                751 => "Canon EF 16-35mm f/2.8L III USM".to_string(),
                752 => "Canon EF 24-105mm f/4L IS II USM".to_string(),
                753 => "Canon EF 85mm f/1.4L IS USM".to_string(),
                754 => "Canon EF 70-200mm f/4L IS II USM".to_string(),
                757 => "Canon EF 400mm f/2.8L IS III USM".to_string(),
                758 => "Canon EF 600mm f/4L IS III USM".to_string(),
                923 => "Meike/SKY 85mm f/1.8 DCM".to_string(),
                1136 => "Sigma 24-70mm f/2.8 DG OS HSM | A".to_string(),
                4142 => "Canon EF-S 18-135mm f/3.5-5.6 IS STM".to_string(),
                4143 => "Canon EF-M 18-55mm f/3.5-5.6 IS STM or Tamron Lens".to_string(),
                4144 => "Canon EF 40mm f/2.8 STM".to_string(),
                4145 => "Canon EF-M 22mm f/2 STM".to_string(),
                4146 => "Canon EF-S 18-55mm f/3.5-5.6 IS STM".to_string(),
                4147 => "Canon EF-M 11-22mm f/4-5.6 IS STM".to_string(),
                4148 => "Canon EF-S 55-250mm f/4-5.6 IS STM".to_string(),
                4149 => "Canon EF-M 55-200mm f/4.5-6.3 IS STM".to_string(),
                4150 => "Canon EF-S 10-18mm f/4.5-5.6 IS STM".to_string(),
                4152 => "Canon EF 24-105mm f/3.5-5.6 IS STM".to_string(),
                4153 => "Canon EF-M 15-45mm f/3.5-6.3 IS STM".to_string(),
                4154 => "Canon EF-S 24mm f/2.8 STM".to_string(),
                4155 => "Canon EF-M 28mm f/3.5 Macro IS STM".to_string(),
                4156 => "Canon EF 50mm f/1.8 STM".to_string(),
                4157 => "Canon EF-M 18-150mm f/3.5-6.3 IS STM".to_string(),
                4158 => "Canon EF-S 18-55mm f/4-5.6 IS STM".to_string(),
                4159 => "Canon EF-M 32mm f/1.4 STM".to_string(),
                4160 => "Canon EF-S 35mm f/2.8 Macro IS STM".to_string(),
                4208 => "Sigma 56mm f/1.4 DC DN | C or other Sigma Lens".to_string(),
                4976 => "Sigma 16-300mm F3.5-6.7 DC OS | C (025)".to_string(),
                6512 => "Sigma 12mm F1.4 DC | C".to_string(),
                36910 => "Canon EF 70-300mm f/4-5.6 IS II USM".to_string(),
                36912 => "Canon EF-S 18-135mm f/3.5-5.6 IS USM".to_string(),
                61182 => "Canon RF 50mm F1.2L USM or other Canon RF Lens".to_string(),
                61491 => "Canon CN-E 14mm T3.1 L F".to_string(),
                61492 => "Canon CN-E 24mm T1.5 L F".to_string(),
                61494 => "Canon CN-E 85mm T1.3 L F".to_string(),
                61495 => "Canon CN-E 135mm T2.2 L F".to_string(),
                61496 => "Canon CN-E 35mm T1.5 L F".to_string(),
                65535 => "n/a".to_string(),
                other => other.to_string(),
            };
            tags.push(mk("LensType", 0xc, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x17) {
        dm.push(("CameraTemperature".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 128", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
        tags.push(mk("CameraTemperature", 0x17, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if (dm_get(dm, "LensType").is_some_and(|v| v != 0.0) && dm_get(dm, "LensType").is_some_and(|v| v == 124.0)) {
        if let Some(v) = i8_at(data, 0x1b) {
            dm.push(("MacroMagnification".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp((75-$val) * log(2) * 3 / 40)", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("sprintf(\"%.1fx\",$val)", &cv, &ctx) { cv = x; }
            tags.push(mk("MacroMagnification", 0x1b, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i8_at(data, 0x27) {
        dm.push(("CameraOrientation".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Horizontal (normal)".to_string(),
            1 => "Rotate 90 CW".to_string(),
            2 => "Rotate 270 CW".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("CameraOrientation", 0x27, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x28, bo) {
        dm.push(("FocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
            tags.push(mk("FocalLength", 0x28, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u16rev_at(data, 0x38, bo) {
        dm.push(("AFPointsInFocus5D".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Center".to_string(),
            1 => "Top".to_string(),
            2 => "Bottom".to_string(),
            3 => "Upper-left".to_string(),
            4 => "Upper-right".to_string(),
            5 => "Lower-left".to_string(),
            6 => "Lower-right".to_string(),
            7 => "Left".to_string(),
            8 => "Right".to_string(),
            9 => "AI Servo1".to_string(),
            10 => "AI Servo2".to_string(),
            11 => "AI Servo3".to_string(),
            12 => "AI Servo4".to_string(),
            13 => "AI Servo5".to_string(),
            14 => "AI Servo6".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("AFPointsInFocus5D", 0x38, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x54, bo) {
        dm.push(("WhiteBalance".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Auto".to_string(),
            1 => "Daylight".to_string(),
            2 => "Cloudy".to_string(),
            3 => "Tungsten".to_string(),
            4 => "Fluorescent".to_string(),
            5 => "Flash".to_string(),
            6 => "Custom".to_string(),
            7 => "Black & White".to_string(),
            8 => "Shade".to_string(),
            9 => "Manual Temperature (Kelvin)".to_string(),
            10 => "PC Set1".to_string(),
            11 => "PC Set2".to_string(),
            12 => "PC Set3".to_string(),
            14 => "Daylight Fluorescent".to_string(),
            15 => "Custom 1".to_string(),
            16 => "Custom 2".to_string(),
            17 => "Underwater".to_string(),
            18 => "Custom 3".to_string(),
            19 => "Custom 4".to_string(),
            20 => "PC Set4".to_string(),
            21 => "PC Set5".to_string(),
            23 => "Auto (ambience priority)".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("WhiteBalance", 0x54, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x58, bo) {
        dm.push(("ColorTemperature".to_string(), f64::from(v)));
        tags.push(mk("ColorTemperature", 0x58, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x6c) {
        dm.push(("PictureStyle".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "None".to_string(),
            1 => "Standard".to_string(),
            2 => "Portrait".to_string(),
            3 => "High Saturation".to_string(),
            4 => "Adobe RGB".to_string(),
            5 => "Low Saturation".to_string(),
            6 => "CM Set 1".to_string(),
            7 => "CM Set 2".to_string(),
            33 => "User Def. 1".to_string(),
            34 => "User Def. 2".to_string(),
            35 => "User Def. 3".to_string(),
            65 => "PC 1".to_string(),
            66 => "PC 2".to_string(),
            67 => "PC 3".to_string(),
            129 => "Standard".to_string(),
            130 => "Portrait".to_string(),
            131 => "Landscape".to_string(),
            132 => "Neutral".to_string(),
            133 => "Faithful".to_string(),
            134 => "Monochrome".to_string(),
            135 => "Auto".to_string(),
            136 => "Fine Detail".to_string(),
            255 => "n/a".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("PictureStyle", 0x6c, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x93, bo) {
        dm.push(("MinFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MinFocalLength", 0x93, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x95, bo) {
        dm.push(("MaxFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MaxFocalLength", 0x95, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x97, bo) {
        dm.push(("LensType".to_string(), f64::from(v)));
        let s = match v as i64 {
            -1 => "n/a".to_string(),
            1 => "Canon EF 50mm f/1.8".to_string(),
            2 => "Canon EF 28mm f/2.8 or Sigma Lens".to_string(),
            3 => "Canon EF 135mm f/2.8 Soft".to_string(),
            4 => "Canon EF 35-105mm f/3.5-4.5 or Sigma Lens".to_string(),
            5 => "Canon EF 35-70mm f/3.5-4.5".to_string(),
            6 => "Canon EF 28-70mm f/3.5-4.5 or Sigma or Tokina Lens".to_string(),
            7 => "Canon EF 100-300mm f/5.6L".to_string(),
            8 => "Canon EF 100-300mm f/5.6 or Sigma or Tokina Lens".to_string(),
            9 => "Canon EF 70-210mm f/4".to_string(),
            10 => "Canon EF 50mm f/2.5 Macro or Sigma Lens".to_string(),
            11 => "Canon EF 35mm f/2".to_string(),
            13 => "Canon EF 15mm f/2.8 Fisheye".to_string(),
            14 => "Canon EF 50-200mm f/3.5-4.5L".to_string(),
            15 => "Canon EF 50-200mm f/3.5-4.5".to_string(),
            16 => "Canon EF 35-135mm f/3.5-4.5".to_string(),
            17 => "Canon EF 35-70mm f/3.5-4.5A".to_string(),
            18 => "Canon EF 28-70mm f/3.5-4.5".to_string(),
            20 => "Canon EF 100-200mm f/4.5A".to_string(),
            21 => "Canon EF 80-200mm f/2.8L".to_string(),
            22 => "Canon EF 20-35mm f/2.8L or Tokina Lens".to_string(),
            23 => "Canon EF 35-105mm f/3.5-4.5".to_string(),
            24 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            25 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            26 => "Canon EF 100mm f/2.8 Macro or Other Lens".to_string(),
            27 => "Canon EF 35-80mm f/4-5.6".to_string(),
            28 => "Canon EF 80-200mm f/4.5-5.6 or Tamron Lens".to_string(),
            29 => "Canon EF 50mm f/1.8 II".to_string(),
            30 => "Canon EF 35-105mm f/4.5-5.6".to_string(),
            31 => "Canon EF 75-300mm f/4-5.6 or Tamron Lens".to_string(),
            32 => "Canon EF 24mm f/2.8 or Sigma Lens".to_string(),
            33 => "Voigtlander or Carl Zeiss Lens".to_string(),
            35 => "Canon EF 35-80mm f/4-5.6".to_string(),
            36 => "Canon EF 38-76mm f/4.5-5.6".to_string(),
            37 => "Canon EF 35-80mm f/4-5.6 or Tamron Lens".to_string(),
            38 => "Canon EF 80-200mm f/4.5-5.6 II".to_string(),
            39 => "Canon EF 75-300mm f/4-5.6".to_string(),
            40 => "Canon EF 28-80mm f/3.5-5.6".to_string(),
            41 => "Canon EF 28-90mm f/4-5.6".to_string(),
            42 => "Canon EF 28-200mm f/3.5-5.6 or Tamron Lens".to_string(),
            43 => "Canon EF 28-105mm f/4-5.6".to_string(),
            44 => "Canon EF 90-300mm f/4.5-5.6".to_string(),
            45 => "Canon EF-S 18-55mm f/3.5-5.6 [II]".to_string(),
            46 => "Canon EF 28-90mm f/4-5.6".to_string(),
            47 => "Zeiss Milvus 35mm f/2 or 50mm f/2".to_string(),
            48 => "Canon EF-S 18-55mm f/3.5-5.6 IS".to_string(),
            49 => "Canon EF-S 55-250mm f/4-5.6 IS".to_string(),
            50 => "Canon EF-S 18-200mm f/3.5-5.6 IS".to_string(),
            51 => "Canon EF-S 18-135mm f/3.5-5.6 IS".to_string(),
            52 => "Canon EF-S 18-55mm f/3.5-5.6 IS II".to_string(),
            53 => "Canon EF-S 18-55mm f/3.5-5.6 III".to_string(),
            54 => "Canon EF-S 55-250mm f/4-5.6 IS II".to_string(),
            60 => "Irix 11mm f/4 or 15mm f/2.4".to_string(),
            63 => "Irix 30mm F1.4 Dragonfly".to_string(),
            80 => "Canon TS-E 50mm f/2.8L Macro".to_string(),
            81 => "Canon TS-E 90mm f/2.8L Macro".to_string(),
            82 => "Canon TS-E 135mm f/4L Macro".to_string(),
            94 => "Canon TS-E 17mm f/4L".to_string(),
            95 => "Canon TS-E 24mm f/3.5L II".to_string(),
            103 => "Samyang AF 14mm f/2.8 EF or Rokinon Lens".to_string(),
            106 => "Rokinon SP / Samyang XP 35mm f/1.2".to_string(),
            112 => "Sigma 28mm f/1.5 FF High-speed Prime or other Sigma Lens".to_string(),
            117 => "Tamron 35-150mm f/2.8-4.0 Di VC OSD (A043) or other Tamron Lens".to_string(),
            124 => "Canon MP-E 65mm f/2.8 1-5x Macro Photo".to_string(),
            125 => "Canon TS-E 24mm f/3.5L".to_string(),
            126 => "Canon TS-E 45mm f/2.8".to_string(),
            127 => "Canon TS-E 90mm f/2.8 or Tamron Lens".to_string(),
            129 => "Canon EF 300mm f/2.8L USM".to_string(),
            130 => "Canon EF 50mm f/1.0L USM".to_string(),
            131 => "Canon EF 28-80mm f/2.8-4L USM or Sigma Lens".to_string(),
            132 => "Canon EF 1200mm f/5.6L USM".to_string(),
            134 => "Canon EF 600mm f/4L IS USM".to_string(),
            135 => "Canon EF 200mm f/1.8L USM".to_string(),
            136 => "Canon EF 300mm f/2.8L USM".to_string(),
            137 => "Canon EF 85mm f/1.2L USM or Sigma or Tamron Lens".to_string(),
            138 => "Canon EF 28-80mm f/2.8-4L".to_string(),
            139 => "Canon EF 400mm f/2.8L USM".to_string(),
            140 => "Canon EF 500mm f/4.5L USM".to_string(),
            141 => "Canon EF 500mm f/4.5L USM".to_string(),
            142 => "Canon EF 300mm f/2.8L IS USM".to_string(),
            143 => "Canon EF 500mm f/4L IS USM or Sigma Lens".to_string(),
            144 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            145 => "Canon EF 100-300mm f/4.5-5.6 USM".to_string(),
            146 => "Canon EF 70-210mm f/3.5-4.5 USM".to_string(),
            147 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            148 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            149 => "Canon EF 100mm f/2 USM".to_string(),
            150 => "Canon EF 14mm f/2.8L USM or Sigma Lens".to_string(),
            151 => "Canon EF 200mm f/2.8L USM".to_string(),
            152 => "Canon EF 300mm f/4L IS USM or Sigma Lens".to_string(),
            153 => "Canon EF 35-350mm f/3.5-5.6L USM or Sigma or Tamron Lens".to_string(),
            154 => "Canon EF 20mm f/2.8 USM or Zeiss Lens".to_string(),
            155 => "Canon EF 85mm f/1.8 USM or Sigma Lens".to_string(),
            156 => "Canon EF 28-105mm f/3.5-4.5 USM or Tamron Lens".to_string(),
            160 => "Canon EF 20-35mm f/3.5-4.5 USM or Tamron or Tokina Lens".to_string(),
            161 => "Canon EF 28-70mm f/2.8L USM or Other Lens".to_string(),
            162 => "Canon EF 200mm f/2.8L USM".to_string(),
            163 => "Canon EF 300mm f/4L".to_string(),
            164 => "Canon EF 400mm f/5.6L".to_string(),
            165 => "Canon EF 70-200mm f/2.8L USM".to_string(),
            166 => "Canon EF 70-200mm f/2.8L USM + 1.4x".to_string(),
            167 => "Canon EF 70-200mm f/2.8L USM + 2x".to_string(),
            168 => "Canon EF 28mm f/1.8 USM or Sigma Lens".to_string(),
            169 => "Canon EF 17-35mm f/2.8L USM or Sigma Lens".to_string(),
            170 => "Canon EF 200mm f/2.8L II USM or Sigma Lens".to_string(),
            171 => "Canon EF 300mm f/4L USM".to_string(),
            172 => "Canon EF 400mm f/5.6L USM or Sigma Lens".to_string(),
            173 => "Canon EF 180mm Macro f/3.5L USM or Sigma Lens".to_string(),
            174 => "Canon EF 135mm f/2L USM or Other Lens".to_string(),
            175 => "Canon EF 400mm f/2.8L USM".to_string(),
            176 => "Canon EF 24-85mm f/3.5-4.5 USM".to_string(),
            177 => "Canon EF 300mm f/4L IS USM".to_string(),
            178 => "Canon EF 28-135mm f/3.5-5.6 IS".to_string(),
            179 => "Canon EF 24mm f/1.4L USM".to_string(),
            180 => "Canon EF 35mm f/1.4L USM or Other Lens".to_string(),
            181 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 1.4x or Sigma Lens".to_string(),
            182 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 2x or Sigma Lens".to_string(),
            183 => "Canon EF 100-400mm f/4.5-5.6L IS USM or Sigma Lens".to_string(),
            184 => "Canon EF 400mm f/2.8L USM + 2x".to_string(),
            185 => "Canon EF 600mm f/4L IS USM".to_string(),
            186 => "Canon EF 70-200mm f/4L USM".to_string(),
            187 => "Canon EF 70-200mm f/4L USM + 1.4x".to_string(),
            188 => "Canon EF 70-200mm f/4L USM + 2x".to_string(),
            189 => "Canon EF 70-200mm f/4L USM + 2.8x".to_string(),
            190 => "Canon EF 100mm f/2.8 Macro USM".to_string(),
            191 => "Canon EF 400mm f/4 DO IS or Sigma Lens".to_string(),
            193 => "Canon EF 35-80mm f/4-5.6 USM".to_string(),
            194 => "Canon EF 80-200mm f/4.5-5.6 USM".to_string(),
            195 => "Canon EF 35-105mm f/4.5-5.6 USM".to_string(),
            196 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            197 => "Canon EF 75-300mm f/4-5.6 IS USM or Sigma Lens".to_string(),
            198 => "Canon EF 50mm f/1.4 USM or Other Lens".to_string(),
            199 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            200 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            201 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            202 => "Canon EF 28-80mm f/3.5-5.6 USM IV".to_string(),
            208 => "Canon EF 22-55mm f/4-5.6 USM".to_string(),
            209 => "Canon EF 55-200mm f/4.5-5.6".to_string(),
            210 => "Canon EF 28-90mm f/4-5.6 USM".to_string(),
            211 => "Canon EF 28-200mm f/3.5-5.6 USM".to_string(),
            212 => "Canon EF 28-105mm f/4-5.6 USM".to_string(),
            213 => "Canon EF 90-300mm f/4.5-5.6 USM or Tamron Lens".to_string(),
            214 => "Canon EF-S 18-55mm f/3.5-5.6 USM".to_string(),
            215 => "Canon EF 55-200mm f/4.5-5.6 II USM".to_string(),
            217 => "Tamron AF 18-270mm f/3.5-6.3 Di II VC PZD".to_string(),
            220 => "Yongnuo YN 50mm f/1.8".to_string(),
            224 => "Canon EF 70-200mm f/2.8L IS USM".to_string(),
            225 => "Canon EF 70-200mm f/2.8L IS USM + 1.4x".to_string(),
            226 => "Canon EF 70-200mm f/2.8L IS USM + 2x".to_string(),
            227 => "Canon EF 70-200mm f/2.8L IS USM + 2.8x".to_string(),
            228 => "Canon EF 28-105mm f/3.5-4.5 USM".to_string(),
            229 => "Canon EF 16-35mm f/2.8L USM".to_string(),
            230 => "Canon EF 24-70mm f/2.8L USM".to_string(),
            231 => "Canon EF 17-40mm f/4L USM or Sigma Lens".to_string(),
            232 => "Canon EF 70-300mm f/4.5-5.6 DO IS USM".to_string(),
            233 => "Canon EF 28-300mm f/3.5-5.6L IS USM".to_string(),
            234 => "Canon EF-S 17-85mm f/4-5.6 IS USM or Tokina Lens".to_string(),
            235 => "Canon EF-S 10-22mm f/3.5-4.5 USM".to_string(),
            236 => "Canon EF-S 60mm f/2.8 Macro USM".to_string(),
            237 => "Canon EF 24-105mm f/4L IS USM".to_string(),
            238 => "Canon EF 70-300mm f/4-5.6 IS USM".to_string(),
            239 => "Canon EF 85mm f/1.2L II USM or Rokinon Lens".to_string(),
            240 => "Canon EF-S 17-55mm f/2.8 IS USM or Sigma Lens".to_string(),
            241 => "Canon EF 50mm f/1.2L USM".to_string(),
            242 => "Canon EF 70-200mm f/4L IS USM".to_string(),
            243 => "Canon EF 70-200mm f/4L IS USM + 1.4x".to_string(),
            244 => "Canon EF 70-200mm f/4L IS USM + 2x".to_string(),
            245 => "Canon EF 70-200mm f/4L IS USM + 2.8x".to_string(),
            246 => "Canon EF 16-35mm f/2.8L II USM".to_string(),
            247 => "Canon EF 14mm f/2.8L II USM".to_string(),
            248 => "Canon EF 200mm f/2L IS USM or Sigma Lens".to_string(),
            249 => "Canon EF 800mm f/5.6L IS USM".to_string(),
            250 => "Canon EF 24mm f/1.4L II USM or Sigma Lens".to_string(),
            251 => "Canon EF 70-200mm f/2.8L IS II USM".to_string(),
            252 => "Canon EF 70-200mm f/2.8L IS II USM + 1.4x".to_string(),
            253 => "Canon EF 70-200mm f/2.8L IS II USM + 2x".to_string(),
            254 => "Canon EF 100mm f/2.8L Macro IS USM or Tamron Lens".to_string(),
            255 => "Sigma 24-105mm f/4 DG OS HSM | A or Other Lens".to_string(),
            368 => "Sigma 14-24mm f/2.8 DG HSM | A or other Sigma Lens".to_string(),
            488 => "Canon EF-S 15-85mm f/3.5-5.6 IS USM".to_string(),
            489 => "Canon EF 70-300mm f/4-5.6L IS USM".to_string(),
            490 => "Canon EF 8-15mm f/4L Fisheye USM".to_string(),
            491 => "Canon EF 300mm f/2.8L IS II USM or Tamron Lens".to_string(),
            492 => "Canon EF 400mm f/2.8L IS II USM".to_string(),
            493 => "Canon EF 500mm f/4L IS II USM or EF 24-105mm f4L IS USM".to_string(),
            494 => "Canon EF 600mm f/4L IS II USM".to_string(),
            495 => "Canon EF 24-70mm f/2.8L II USM or Sigma Lens".to_string(),
            496 => "Canon EF 200-400mm f/4L IS USM".to_string(),
            499 => "Canon EF 200-400mm f/4L IS USM + 1.4x".to_string(),
            502 => "Canon EF 28mm f/2.8 IS USM or Tamron Lens".to_string(),
            503 => "Canon EF 24mm f/2.8 IS USM".to_string(),
            504 => "Canon EF 24-70mm f/4L IS USM".to_string(),
            505 => "Canon EF 35mm f/2 IS USM".to_string(),
            506 => "Canon EF 400mm f/4 DO IS II USM".to_string(),
            507 => "Canon EF 16-35mm f/4L IS USM".to_string(),
            508 => "Canon EF 11-24mm f/4L USM or Tamron Lens".to_string(),
            624 => "Sigma 70-200mm f/2.8 DG OS HSM | S or other Sigma Lens".to_string(),
            747 => "Canon EF 100-400mm f/4.5-5.6L IS II USM or Tamron Lens".to_string(),
            748 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 1.4x or Tamron Lens".to_string(),
            749 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 2x or Tamron Lens".to_string(),
            750 => "Canon EF 35mm f/1.4L II USM or Tamron Lens".to_string(),
            751 => "Canon EF 16-35mm f/2.8L III USM".to_string(),
            752 => "Canon EF 24-105mm f/4L IS II USM".to_string(),
            753 => "Canon EF 85mm f/1.4L IS USM".to_string(),
            754 => "Canon EF 70-200mm f/4L IS II USM".to_string(),
            757 => "Canon EF 400mm f/2.8L IS III USM".to_string(),
            758 => "Canon EF 600mm f/4L IS III USM".to_string(),
            923 => "Meike/SKY 85mm f/1.8 DCM".to_string(),
            1136 => "Sigma 24-70mm f/2.8 DG OS HSM | A".to_string(),
            4142 => "Canon EF-S 18-135mm f/3.5-5.6 IS STM".to_string(),
            4143 => "Canon EF-M 18-55mm f/3.5-5.6 IS STM or Tamron Lens".to_string(),
            4144 => "Canon EF 40mm f/2.8 STM".to_string(),
            4145 => "Canon EF-M 22mm f/2 STM".to_string(),
            4146 => "Canon EF-S 18-55mm f/3.5-5.6 IS STM".to_string(),
            4147 => "Canon EF-M 11-22mm f/4-5.6 IS STM".to_string(),
            4148 => "Canon EF-S 55-250mm f/4-5.6 IS STM".to_string(),
            4149 => "Canon EF-M 55-200mm f/4.5-6.3 IS STM".to_string(),
            4150 => "Canon EF-S 10-18mm f/4.5-5.6 IS STM".to_string(),
            4152 => "Canon EF 24-105mm f/3.5-5.6 IS STM".to_string(),
            4153 => "Canon EF-M 15-45mm f/3.5-6.3 IS STM".to_string(),
            4154 => "Canon EF-S 24mm f/2.8 STM".to_string(),
            4155 => "Canon EF-M 28mm f/3.5 Macro IS STM".to_string(),
            4156 => "Canon EF 50mm f/1.8 STM".to_string(),
            4157 => "Canon EF-M 18-150mm f/3.5-6.3 IS STM".to_string(),
            4158 => "Canon EF-S 18-55mm f/4-5.6 IS STM".to_string(),
            4159 => "Canon EF-M 32mm f/1.4 STM".to_string(),
            4160 => "Canon EF-S 35mm f/2.8 Macro IS STM".to_string(),
            4208 => "Sigma 56mm f/1.4 DC DN | C or other Sigma Lens".to_string(),
            4976 => "Sigma 16-300mm F3.5-6.7 DC OS | C (025)".to_string(),
            6512 => "Sigma 12mm F1.4 DC | C".to_string(),
            36910 => "Canon EF 70-300mm f/4-5.6 IS II USM".to_string(),
            36912 => "Canon EF-S 18-135mm f/3.5-5.6 IS USM".to_string(),
            61182 => "Canon RF 50mm F1.2L USM or other Canon RF Lens".to_string(),
            61491 => "Canon CN-E 14mm T3.1 L F".to_string(),
            61492 => "Canon CN-E 24mm T1.5 L F".to_string(),
            61494 => "Canon CN-E 85mm T1.3 L F".to_string(),
            61495 => "Canon CN-E 135mm T2.2 L F".to_string(),
            61496 => "Canon CN-E 35mm T1.5 L F".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("LensType", 0x97, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(text) = text_at(data, 0xa4, 8, true) {
        tags.push(mk("FirmwareRevision", 0xa4, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    if let Some(text) = text_at(data, 0xac, 16, true) {
        tags.push(mk("ShortOwnerName", 0xac, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0xcc, bo) {
        dm.push(("DirectoryIndex".to_string(), f64::from(v)));
        tags.push(mk("DirectoryIndex", 0xcc, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0xd0, bo) {
        dm.push(("FileIndex".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val + 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("FileIndex", 0xd0, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0xe8) {
        dm.push(("ContrastStandard".to_string(), f64::from(v)));
        tags.push(mk("ContrastStandard", 0xe8, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0xe9) {
        dm.push(("ContrastPortrait".to_string(), f64::from(v)));
        tags.push(mk("ContrastPortrait", 0xe9, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0xea) {
        dm.push(("ContrastLandscape".to_string(), f64::from(v)));
        tags.push(mk("ContrastLandscape", 0xea, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0xeb) {
        dm.push(("ContrastNeutral".to_string(), f64::from(v)));
        tags.push(mk("ContrastNeutral", 0xeb, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0xec) {
        dm.push(("ContrastFaithful".to_string(), f64::from(v)));
        tags.push(mk("ContrastFaithful", 0xec, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0xed) {
        dm.push(("ContrastMonochrome".to_string(), f64::from(v)));
        tags.push(mk("ContrastMonochrome", 0xed, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0xee) {
        dm.push(("ContrastUserDef1".to_string(), f64::from(v)));
        tags.push(mk("ContrastUserDef1", 0xee, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0xef) {
        dm.push(("ContrastUserDef2".to_string(), f64::from(v)));
        tags.push(mk("ContrastUserDef2", 0xef, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0xf0) {
        dm.push(("ContrastUserDef3".to_string(), f64::from(v)));
        tags.push(mk("ContrastUserDef3", 0xf0, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0xf1) {
        dm.push(("SharpnessStandard".to_string(), f64::from(v)));
        tags.push(mk("SharpnessStandard", 0xf1, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0xf2) {
        dm.push(("SharpnessPortrait".to_string(), f64::from(v)));
        tags.push(mk("SharpnessPortrait", 0xf2, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0xf3) {
        dm.push(("SharpnessLandscape".to_string(), f64::from(v)));
        tags.push(mk("SharpnessLandscape", 0xf3, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0xf4) {
        dm.push(("SharpnessNeutral".to_string(), f64::from(v)));
        tags.push(mk("SharpnessNeutral", 0xf4, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0xf5) {
        dm.push(("SharpnessFaithful".to_string(), f64::from(v)));
        tags.push(mk("SharpnessFaithful", 0xf5, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0xf6) {
        dm.push(("SharpnessMonochrome".to_string(), f64::from(v)));
        tags.push(mk("SharpnessMonochrome", 0xf6, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0xf7) {
        dm.push(("SharpnessUserDef1".to_string(), f64::from(v)));
        tags.push(mk("SharpnessUserDef1", 0xf7, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0xf8) {
        dm.push(("SharpnessUserDef2".to_string(), f64::from(v)));
        tags.push(mk("SharpnessUserDef2", 0xf8, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0xf9) {
        dm.push(("SharpnessUserDef3".to_string(), f64::from(v)));
        tags.push(mk("SharpnessUserDef3", 0xf9, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0xfa) {
        dm.push(("SaturationStandard".to_string(), f64::from(v)));
        tags.push(mk("SaturationStandard", 0xfa, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0xfb) {
        dm.push(("SaturationPortrait".to_string(), f64::from(v)));
        tags.push(mk("SaturationPortrait", 0xfb, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0xfc) {
        dm.push(("SaturationLandscape".to_string(), f64::from(v)));
        tags.push(mk("SaturationLandscape", 0xfc, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0xfd) {
        dm.push(("SaturationNeutral".to_string(), f64::from(v)));
        tags.push(mk("SaturationNeutral", 0xfd, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0xfe) {
        dm.push(("SaturationFaithful".to_string(), f64::from(v)));
        tags.push(mk("SaturationFaithful", 0xfe, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0xff) {
        dm.push(("FilterEffectMonochrome".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            0 => "None".to_string(),
            1 => "Yellow".to_string(),
            2 => "Orange".to_string(),
            3 => "Red".to_string(),
            4 => "Green".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("FilterEffectMonochrome", 0xff, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0x100) {
        dm.push(("SaturationUserDef1".to_string(), f64::from(v)));
        tags.push(mk("SaturationUserDef1", 0x100, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0x101) {
        dm.push(("SaturationUserDef2".to_string(), f64::from(v)));
        tags.push(mk("SaturationUserDef2", 0x101, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0x102) {
        dm.push(("SaturationUserDef3".to_string(), f64::from(v)));
        tags.push(mk("SaturationUserDef3", 0x102, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0x103) {
        dm.push(("ColorToneStandard".to_string(), f64::from(v)));
        tags.push(mk("ColorToneStandard", 0x103, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0x104) {
        dm.push(("ColorTonePortrait".to_string(), f64::from(v)));
        tags.push(mk("ColorTonePortrait", 0x104, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0x105) {
        dm.push(("ColorToneLandscape".to_string(), f64::from(v)));
        tags.push(mk("ColorToneLandscape", 0x105, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0x106) {
        dm.push(("ColorToneNeutral".to_string(), f64::from(v)));
        tags.push(mk("ColorToneNeutral", 0x106, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0x107) {
        dm.push(("ColorToneFaithful".to_string(), f64::from(v)));
        tags.push(mk("ColorToneFaithful", 0x107, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0x108) {
        dm.push(("ToningEffectMonochrome".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            0 => "None".to_string(),
            1 => "Sepia".to_string(),
            2 => "Blue".to_string(),
            3 => "Purple".to_string(),
            4 => "Green".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ToningEffectMonochrome", 0x108, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0x109) {
        dm.push(("ColorToneUserDef1".to_string(), f64::from(v)));
        tags.push(mk("ColorToneUserDef1", 0x109, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0x10a) {
        dm.push(("ColorToneUserDef2".to_string(), f64::from(v)));
        tags.push(mk("ColorToneUserDef2", 0x10a, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i8_at(data, 0x10b) {
        dm.push(("ColorToneUserDef3".to_string(), f64::from(v)));
        tags.push(mk("ColorToneUserDef3", 0x10b, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x10c, bo) {
        dm.push(("UserDef1PictureStyle".to_string(), f64::from(v)));
        let s = match v as i64 {
            65 => "PC 1".to_string(),
            66 => "PC 2".to_string(),
            67 => "PC 3".to_string(),
            129 => "Standard".to_string(),
            130 => "Portrait".to_string(),
            131 => "Landscape".to_string(),
            132 => "Neutral".to_string(),
            133 => "Faithful".to_string(),
            134 => "Monochrome".to_string(),
            135 => "Auto".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("UserDef1PictureStyle", 0x10c, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x10e, bo) {
        dm.push(("UserDef2PictureStyle".to_string(), f64::from(v)));
        let s = match v as i64 {
            65 => "PC 1".to_string(),
            66 => "PC 2".to_string(),
            67 => "PC 3".to_string(),
            129 => "Standard".to_string(),
            130 => "Portrait".to_string(),
            131 => "Landscape".to_string(),
            132 => "Neutral".to_string(),
            133 => "Faithful".to_string(),
            134 => "Monochrome".to_string(),
            135 => "Auto".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("UserDef2PictureStyle", 0x10e, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x110, bo) {
        dm.push(("UserDef3PictureStyle".to_string(), f64::from(v)));
        let s = match v as i64 {
            65 => "PC 1".to_string(),
            66 => "PC 2".to_string(),
            67 => "PC 3".to_string(),
            129 => "Standard".to_string(),
            130 => "Portrait".to_string(),
            131 => "Landscape".to_string(),
            132 => "Neutral".to_string(),
            133 => "Faithful".to_string(),
            134 => "Monochrome".to_string(),
            135 => "Auto".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("UserDef3PictureStyle", 0x110, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x11c, bo) {
        dm.push(("TimeStamp".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("ConvertUnixTime($val)", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("$self->ConvertDateTime($val)", &cv, &ctx) { cv = x; }
            tags.push(mk("TimeStamp", 0x11c, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    tags
}

/// `Image::ExifTool::Canon::CameraInfo5DmkII` -- FORMAT int8u, FIRST_ENTRY 0.
fn canon_camerainfo5dmkii(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(text) = text_at(data, 0x0, 388, false) {
        tags.push(mk("FirmwareVersionLookAhead", 0x0, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x3) {
        dm.push(("FNumber".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(($val-8)/16*log(2))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("sprintf(\"%.2g\",$val)", &cv, &ctx) { cv = x; }
            tags.push(mk("FNumber", 0x3, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x4) {
        dm.push(("ExposureTime".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(4*log(2)*(1-Image::ExifTool::Canon::CanonEv($val-24)))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("Image::ExifTool::Exif::PrintExposureTime($val)", &cv, &ctx) { cv = x; }
            tags.push(mk("ExposureTime", 0x4, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x6) {
        dm.push(("ISO".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("100*exp(($val/8-9)*log(2))", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("sprintf(\"%.0f\",$val)", &cv, &ctx) { cv = x; }
        tags.push(mk("ISO", 0x6, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x7) {
        dm.push(("HighlightTonePriority".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "People".to_string(),
            1 => "sRGB".to_string(),
            2 => "Adobe RGB".to_string(),
            3 => "User 1".to_string(),
            4 => "User 2".to_string(),
            5 => "User 3".to_string(),
            6 => "To Do".to_string(),
            65535 => "n/a".to_string(),
            2415919104 => "Format 1".to_string(),
            2684354560 => "Format 2".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("HighlightTonePriority", 0x7, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x13) {
        let v = v & 0x7f;
        dm.push(("FlashModel".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "n/a".to_string(),
            4 => "Speedlite 540EZ".to_string(),
            5 => "Speedlite 380EX".to_string(),
            6 => "Speedlite 550EX".to_string(),
            8 => "Speedlite ST-E2".to_string(),
            9 => "Speedlite MR-14EX".to_string(),
            12 => "Speedlite 580EX".to_string(),
            13 => "Speedlite 430EX".to_string(),
            17 => "Speedlite 580EX II".to_string(),
            18 => "Speedlite 430EX II".to_string(),
            22 => "Speedlite 600EX-RT".to_string(),
            23 => "Speedlite 600EX II-RT".to_string(),
            24 => "Speedlite 90EX".to_string(),
            25 => "Speedlite 430EX III-RT".to_string(),
            31 => "Speedlite EL-1 ver2".to_string(),
            33 => "Speedlite EL-5".to_string(),
            34 => "Speedlite EL-10".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("FlashModel", 0x13, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x15) {
        dm.push(("FlashMeteringMode".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "E-TTL".to_string(),
            3 => "TTL".to_string(),
            4 => "External Auto".to_string(),
            5 => "External Manual".to_string(),
            6 => "Off".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("FlashMeteringMode", 0x15, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x19) {
        dm.push(("CameraTemperature".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 128", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
        tags.push(mk("CameraTemperature", 0x19, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if (dm_get(dm, "LensType").is_some_and(|v| v != 0.0) && dm_get(dm, "LensType").is_some_and(|v| v == 124.0)) {
        if let Some(v) = u8_at(data, 0x1b) {
            dm.push(("MacroMagnification".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp((75-$val) * log(2) * 3 / 40)", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("sprintf(\"%.1fx\",$val)", &cv, &ctx) { cv = x; }
            tags.push(mk("MacroMagnification", 0x1b, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u16rev_at(data, 0x1e, bo) {
        dm.push(("FocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
            tags.push(mk("FocalLength", 0x1e, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x31) {
        dm.push(("CameraOrientation".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Horizontal (normal)".to_string(),
            1 => "Rotate 90 CW".to_string(),
            2 => "Rotate 270 CW".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("CameraOrientation", 0x31, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x50, bo) {
        dm.push(("FocusDistanceUpper".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceUpper", 0x50, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x52, bo) {
        dm.push(("FocusDistanceLower".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceLower", 0x52, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x6f, bo) {
        dm.push(("WhiteBalance".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Auto".to_string(),
            1 => "Daylight".to_string(),
            2 => "Cloudy".to_string(),
            3 => "Tungsten".to_string(),
            4 => "Fluorescent".to_string(),
            5 => "Flash".to_string(),
            6 => "Custom".to_string(),
            7 => "Black & White".to_string(),
            8 => "Shade".to_string(),
            9 => "Manual Temperature (Kelvin)".to_string(),
            10 => "PC Set1".to_string(),
            11 => "PC Set2".to_string(),
            12 => "PC Set3".to_string(),
            14 => "Daylight Fluorescent".to_string(),
            15 => "Custom 1".to_string(),
            16 => "Custom 2".to_string(),
            17 => "Underwater".to_string(),
            18 => "Custom 3".to_string(),
            19 => "Custom 4".to_string(),
            20 => "PC Set4".to_string(),
            21 => "PC Set5".to_string(),
            23 => "Auto (ambience priority)".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("WhiteBalance", 0x6f, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x73, bo) {
        dm.push(("ColorTemperature".to_string(), f64::from(v)));
        tags.push(mk("ColorTemperature", 0x73, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0xa7) {
        dm.push(("PictureStyle".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "None".to_string(),
            1 => "Standard".to_string(),
            2 => "Portrait".to_string(),
            3 => "High Saturation".to_string(),
            4 => "Adobe RGB".to_string(),
            5 => "Low Saturation".to_string(),
            6 => "CM Set 1".to_string(),
            7 => "CM Set 2".to_string(),
            33 => "User Def. 1".to_string(),
            34 => "User Def. 2".to_string(),
            35 => "User Def. 3".to_string(),
            65 => "PC 1".to_string(),
            66 => "PC 2".to_string(),
            67 => "PC 3".to_string(),
            129 => "Standard".to_string(),
            130 => "Portrait".to_string(),
            131 => "Landscape".to_string(),
            132 => "Neutral".to_string(),
            133 => "Faithful".to_string(),
            134 => "Monochrome".to_string(),
            135 => "Auto".to_string(),
            136 => "Fine Detail".to_string(),
            255 => "n/a".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("PictureStyle", 0xa7, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0xbd) {
        dm.push(("HighISONoiseReduction".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Standard".to_string(),
            1 => "Low".to_string(),
            2 => "Strong".to_string(),
            3 => "Off".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("HighISONoiseReduction", 0xbd, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0xbf) {
        dm.push(("AutoLightingOptimizer".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Standard".to_string(),
            1 => "Low".to_string(),
            2 => "Strong".to_string(),
            3 => "Off".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("AutoLightingOptimizer", 0xbf, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0xe6, bo) {
        dm.push(("LensType".to_string(), f64::from(v)));
        let s = match v as i64 {
            -1 => "n/a".to_string(),
            1 => "Canon EF 50mm f/1.8".to_string(),
            2 => "Canon EF 28mm f/2.8 or Sigma Lens".to_string(),
            3 => "Canon EF 135mm f/2.8 Soft".to_string(),
            4 => "Canon EF 35-105mm f/3.5-4.5 or Sigma Lens".to_string(),
            5 => "Canon EF 35-70mm f/3.5-4.5".to_string(),
            6 => "Canon EF 28-70mm f/3.5-4.5 or Sigma or Tokina Lens".to_string(),
            7 => "Canon EF 100-300mm f/5.6L".to_string(),
            8 => "Canon EF 100-300mm f/5.6 or Sigma or Tokina Lens".to_string(),
            9 => "Canon EF 70-210mm f/4".to_string(),
            10 => "Canon EF 50mm f/2.5 Macro or Sigma Lens".to_string(),
            11 => "Canon EF 35mm f/2".to_string(),
            13 => "Canon EF 15mm f/2.8 Fisheye".to_string(),
            14 => "Canon EF 50-200mm f/3.5-4.5L".to_string(),
            15 => "Canon EF 50-200mm f/3.5-4.5".to_string(),
            16 => "Canon EF 35-135mm f/3.5-4.5".to_string(),
            17 => "Canon EF 35-70mm f/3.5-4.5A".to_string(),
            18 => "Canon EF 28-70mm f/3.5-4.5".to_string(),
            20 => "Canon EF 100-200mm f/4.5A".to_string(),
            21 => "Canon EF 80-200mm f/2.8L".to_string(),
            22 => "Canon EF 20-35mm f/2.8L or Tokina Lens".to_string(),
            23 => "Canon EF 35-105mm f/3.5-4.5".to_string(),
            24 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            25 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            26 => "Canon EF 100mm f/2.8 Macro or Other Lens".to_string(),
            27 => "Canon EF 35-80mm f/4-5.6".to_string(),
            28 => "Canon EF 80-200mm f/4.5-5.6 or Tamron Lens".to_string(),
            29 => "Canon EF 50mm f/1.8 II".to_string(),
            30 => "Canon EF 35-105mm f/4.5-5.6".to_string(),
            31 => "Canon EF 75-300mm f/4-5.6 or Tamron Lens".to_string(),
            32 => "Canon EF 24mm f/2.8 or Sigma Lens".to_string(),
            33 => "Voigtlander or Carl Zeiss Lens".to_string(),
            35 => "Canon EF 35-80mm f/4-5.6".to_string(),
            36 => "Canon EF 38-76mm f/4.5-5.6".to_string(),
            37 => "Canon EF 35-80mm f/4-5.6 or Tamron Lens".to_string(),
            38 => "Canon EF 80-200mm f/4.5-5.6 II".to_string(),
            39 => "Canon EF 75-300mm f/4-5.6".to_string(),
            40 => "Canon EF 28-80mm f/3.5-5.6".to_string(),
            41 => "Canon EF 28-90mm f/4-5.6".to_string(),
            42 => "Canon EF 28-200mm f/3.5-5.6 or Tamron Lens".to_string(),
            43 => "Canon EF 28-105mm f/4-5.6".to_string(),
            44 => "Canon EF 90-300mm f/4.5-5.6".to_string(),
            45 => "Canon EF-S 18-55mm f/3.5-5.6 [II]".to_string(),
            46 => "Canon EF 28-90mm f/4-5.6".to_string(),
            47 => "Zeiss Milvus 35mm f/2 or 50mm f/2".to_string(),
            48 => "Canon EF-S 18-55mm f/3.5-5.6 IS".to_string(),
            49 => "Canon EF-S 55-250mm f/4-5.6 IS".to_string(),
            50 => "Canon EF-S 18-200mm f/3.5-5.6 IS".to_string(),
            51 => "Canon EF-S 18-135mm f/3.5-5.6 IS".to_string(),
            52 => "Canon EF-S 18-55mm f/3.5-5.6 IS II".to_string(),
            53 => "Canon EF-S 18-55mm f/3.5-5.6 III".to_string(),
            54 => "Canon EF-S 55-250mm f/4-5.6 IS II".to_string(),
            60 => "Irix 11mm f/4 or 15mm f/2.4".to_string(),
            63 => "Irix 30mm F1.4 Dragonfly".to_string(),
            80 => "Canon TS-E 50mm f/2.8L Macro".to_string(),
            81 => "Canon TS-E 90mm f/2.8L Macro".to_string(),
            82 => "Canon TS-E 135mm f/4L Macro".to_string(),
            94 => "Canon TS-E 17mm f/4L".to_string(),
            95 => "Canon TS-E 24mm f/3.5L II".to_string(),
            103 => "Samyang AF 14mm f/2.8 EF or Rokinon Lens".to_string(),
            106 => "Rokinon SP / Samyang XP 35mm f/1.2".to_string(),
            112 => "Sigma 28mm f/1.5 FF High-speed Prime or other Sigma Lens".to_string(),
            117 => "Tamron 35-150mm f/2.8-4.0 Di VC OSD (A043) or other Tamron Lens".to_string(),
            124 => "Canon MP-E 65mm f/2.8 1-5x Macro Photo".to_string(),
            125 => "Canon TS-E 24mm f/3.5L".to_string(),
            126 => "Canon TS-E 45mm f/2.8".to_string(),
            127 => "Canon TS-E 90mm f/2.8 or Tamron Lens".to_string(),
            129 => "Canon EF 300mm f/2.8L USM".to_string(),
            130 => "Canon EF 50mm f/1.0L USM".to_string(),
            131 => "Canon EF 28-80mm f/2.8-4L USM or Sigma Lens".to_string(),
            132 => "Canon EF 1200mm f/5.6L USM".to_string(),
            134 => "Canon EF 600mm f/4L IS USM".to_string(),
            135 => "Canon EF 200mm f/1.8L USM".to_string(),
            136 => "Canon EF 300mm f/2.8L USM".to_string(),
            137 => "Canon EF 85mm f/1.2L USM or Sigma or Tamron Lens".to_string(),
            138 => "Canon EF 28-80mm f/2.8-4L".to_string(),
            139 => "Canon EF 400mm f/2.8L USM".to_string(),
            140 => "Canon EF 500mm f/4.5L USM".to_string(),
            141 => "Canon EF 500mm f/4.5L USM".to_string(),
            142 => "Canon EF 300mm f/2.8L IS USM".to_string(),
            143 => "Canon EF 500mm f/4L IS USM or Sigma Lens".to_string(),
            144 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            145 => "Canon EF 100-300mm f/4.5-5.6 USM".to_string(),
            146 => "Canon EF 70-210mm f/3.5-4.5 USM".to_string(),
            147 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            148 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            149 => "Canon EF 100mm f/2 USM".to_string(),
            150 => "Canon EF 14mm f/2.8L USM or Sigma Lens".to_string(),
            151 => "Canon EF 200mm f/2.8L USM".to_string(),
            152 => "Canon EF 300mm f/4L IS USM or Sigma Lens".to_string(),
            153 => "Canon EF 35-350mm f/3.5-5.6L USM or Sigma or Tamron Lens".to_string(),
            154 => "Canon EF 20mm f/2.8 USM or Zeiss Lens".to_string(),
            155 => "Canon EF 85mm f/1.8 USM or Sigma Lens".to_string(),
            156 => "Canon EF 28-105mm f/3.5-4.5 USM or Tamron Lens".to_string(),
            160 => "Canon EF 20-35mm f/3.5-4.5 USM or Tamron or Tokina Lens".to_string(),
            161 => "Canon EF 28-70mm f/2.8L USM or Other Lens".to_string(),
            162 => "Canon EF 200mm f/2.8L USM".to_string(),
            163 => "Canon EF 300mm f/4L".to_string(),
            164 => "Canon EF 400mm f/5.6L".to_string(),
            165 => "Canon EF 70-200mm f/2.8L USM".to_string(),
            166 => "Canon EF 70-200mm f/2.8L USM + 1.4x".to_string(),
            167 => "Canon EF 70-200mm f/2.8L USM + 2x".to_string(),
            168 => "Canon EF 28mm f/1.8 USM or Sigma Lens".to_string(),
            169 => "Canon EF 17-35mm f/2.8L USM or Sigma Lens".to_string(),
            170 => "Canon EF 200mm f/2.8L II USM or Sigma Lens".to_string(),
            171 => "Canon EF 300mm f/4L USM".to_string(),
            172 => "Canon EF 400mm f/5.6L USM or Sigma Lens".to_string(),
            173 => "Canon EF 180mm Macro f/3.5L USM or Sigma Lens".to_string(),
            174 => "Canon EF 135mm f/2L USM or Other Lens".to_string(),
            175 => "Canon EF 400mm f/2.8L USM".to_string(),
            176 => "Canon EF 24-85mm f/3.5-4.5 USM".to_string(),
            177 => "Canon EF 300mm f/4L IS USM".to_string(),
            178 => "Canon EF 28-135mm f/3.5-5.6 IS".to_string(),
            179 => "Canon EF 24mm f/1.4L USM".to_string(),
            180 => "Canon EF 35mm f/1.4L USM or Other Lens".to_string(),
            181 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 1.4x or Sigma Lens".to_string(),
            182 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 2x or Sigma Lens".to_string(),
            183 => "Canon EF 100-400mm f/4.5-5.6L IS USM or Sigma Lens".to_string(),
            184 => "Canon EF 400mm f/2.8L USM + 2x".to_string(),
            185 => "Canon EF 600mm f/4L IS USM".to_string(),
            186 => "Canon EF 70-200mm f/4L USM".to_string(),
            187 => "Canon EF 70-200mm f/4L USM + 1.4x".to_string(),
            188 => "Canon EF 70-200mm f/4L USM + 2x".to_string(),
            189 => "Canon EF 70-200mm f/4L USM + 2.8x".to_string(),
            190 => "Canon EF 100mm f/2.8 Macro USM".to_string(),
            191 => "Canon EF 400mm f/4 DO IS or Sigma Lens".to_string(),
            193 => "Canon EF 35-80mm f/4-5.6 USM".to_string(),
            194 => "Canon EF 80-200mm f/4.5-5.6 USM".to_string(),
            195 => "Canon EF 35-105mm f/4.5-5.6 USM".to_string(),
            196 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            197 => "Canon EF 75-300mm f/4-5.6 IS USM or Sigma Lens".to_string(),
            198 => "Canon EF 50mm f/1.4 USM or Other Lens".to_string(),
            199 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            200 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            201 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            202 => "Canon EF 28-80mm f/3.5-5.6 USM IV".to_string(),
            208 => "Canon EF 22-55mm f/4-5.6 USM".to_string(),
            209 => "Canon EF 55-200mm f/4.5-5.6".to_string(),
            210 => "Canon EF 28-90mm f/4-5.6 USM".to_string(),
            211 => "Canon EF 28-200mm f/3.5-5.6 USM".to_string(),
            212 => "Canon EF 28-105mm f/4-5.6 USM".to_string(),
            213 => "Canon EF 90-300mm f/4.5-5.6 USM or Tamron Lens".to_string(),
            214 => "Canon EF-S 18-55mm f/3.5-5.6 USM".to_string(),
            215 => "Canon EF 55-200mm f/4.5-5.6 II USM".to_string(),
            217 => "Tamron AF 18-270mm f/3.5-6.3 Di II VC PZD".to_string(),
            220 => "Yongnuo YN 50mm f/1.8".to_string(),
            224 => "Canon EF 70-200mm f/2.8L IS USM".to_string(),
            225 => "Canon EF 70-200mm f/2.8L IS USM + 1.4x".to_string(),
            226 => "Canon EF 70-200mm f/2.8L IS USM + 2x".to_string(),
            227 => "Canon EF 70-200mm f/2.8L IS USM + 2.8x".to_string(),
            228 => "Canon EF 28-105mm f/3.5-4.5 USM".to_string(),
            229 => "Canon EF 16-35mm f/2.8L USM".to_string(),
            230 => "Canon EF 24-70mm f/2.8L USM".to_string(),
            231 => "Canon EF 17-40mm f/4L USM or Sigma Lens".to_string(),
            232 => "Canon EF 70-300mm f/4.5-5.6 DO IS USM".to_string(),
            233 => "Canon EF 28-300mm f/3.5-5.6L IS USM".to_string(),
            234 => "Canon EF-S 17-85mm f/4-5.6 IS USM or Tokina Lens".to_string(),
            235 => "Canon EF-S 10-22mm f/3.5-4.5 USM".to_string(),
            236 => "Canon EF-S 60mm f/2.8 Macro USM".to_string(),
            237 => "Canon EF 24-105mm f/4L IS USM".to_string(),
            238 => "Canon EF 70-300mm f/4-5.6 IS USM".to_string(),
            239 => "Canon EF 85mm f/1.2L II USM or Rokinon Lens".to_string(),
            240 => "Canon EF-S 17-55mm f/2.8 IS USM or Sigma Lens".to_string(),
            241 => "Canon EF 50mm f/1.2L USM".to_string(),
            242 => "Canon EF 70-200mm f/4L IS USM".to_string(),
            243 => "Canon EF 70-200mm f/4L IS USM + 1.4x".to_string(),
            244 => "Canon EF 70-200mm f/4L IS USM + 2x".to_string(),
            245 => "Canon EF 70-200mm f/4L IS USM + 2.8x".to_string(),
            246 => "Canon EF 16-35mm f/2.8L II USM".to_string(),
            247 => "Canon EF 14mm f/2.8L II USM".to_string(),
            248 => "Canon EF 200mm f/2L IS USM or Sigma Lens".to_string(),
            249 => "Canon EF 800mm f/5.6L IS USM".to_string(),
            250 => "Canon EF 24mm f/1.4L II USM or Sigma Lens".to_string(),
            251 => "Canon EF 70-200mm f/2.8L IS II USM".to_string(),
            252 => "Canon EF 70-200mm f/2.8L IS II USM + 1.4x".to_string(),
            253 => "Canon EF 70-200mm f/2.8L IS II USM + 2x".to_string(),
            254 => "Canon EF 100mm f/2.8L Macro IS USM or Tamron Lens".to_string(),
            255 => "Sigma 24-105mm f/4 DG OS HSM | A or Other Lens".to_string(),
            368 => "Sigma 14-24mm f/2.8 DG HSM | A or other Sigma Lens".to_string(),
            488 => "Canon EF-S 15-85mm f/3.5-5.6 IS USM".to_string(),
            489 => "Canon EF 70-300mm f/4-5.6L IS USM".to_string(),
            490 => "Canon EF 8-15mm f/4L Fisheye USM".to_string(),
            491 => "Canon EF 300mm f/2.8L IS II USM or Tamron Lens".to_string(),
            492 => "Canon EF 400mm f/2.8L IS II USM".to_string(),
            493 => "Canon EF 500mm f/4L IS II USM or EF 24-105mm f4L IS USM".to_string(),
            494 => "Canon EF 600mm f/4L IS II USM".to_string(),
            495 => "Canon EF 24-70mm f/2.8L II USM or Sigma Lens".to_string(),
            496 => "Canon EF 200-400mm f/4L IS USM".to_string(),
            499 => "Canon EF 200-400mm f/4L IS USM + 1.4x".to_string(),
            502 => "Canon EF 28mm f/2.8 IS USM or Tamron Lens".to_string(),
            503 => "Canon EF 24mm f/2.8 IS USM".to_string(),
            504 => "Canon EF 24-70mm f/4L IS USM".to_string(),
            505 => "Canon EF 35mm f/2 IS USM".to_string(),
            506 => "Canon EF 400mm f/4 DO IS II USM".to_string(),
            507 => "Canon EF 16-35mm f/4L IS USM".to_string(),
            508 => "Canon EF 11-24mm f/4L USM or Tamron Lens".to_string(),
            624 => "Sigma 70-200mm f/2.8 DG OS HSM | S or other Sigma Lens".to_string(),
            747 => "Canon EF 100-400mm f/4.5-5.6L IS II USM or Tamron Lens".to_string(),
            748 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 1.4x or Tamron Lens".to_string(),
            749 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 2x or Tamron Lens".to_string(),
            750 => "Canon EF 35mm f/1.4L II USM or Tamron Lens".to_string(),
            751 => "Canon EF 16-35mm f/2.8L III USM".to_string(),
            752 => "Canon EF 24-105mm f/4L IS II USM".to_string(),
            753 => "Canon EF 85mm f/1.4L IS USM".to_string(),
            754 => "Canon EF 70-200mm f/4L IS II USM".to_string(),
            757 => "Canon EF 400mm f/2.8L IS III USM".to_string(),
            758 => "Canon EF 600mm f/4L IS III USM".to_string(),
            923 => "Meike/SKY 85mm f/1.8 DCM".to_string(),
            1136 => "Sigma 24-70mm f/2.8 DG OS HSM | A".to_string(),
            4142 => "Canon EF-S 18-135mm f/3.5-5.6 IS STM".to_string(),
            4143 => "Canon EF-M 18-55mm f/3.5-5.6 IS STM or Tamron Lens".to_string(),
            4144 => "Canon EF 40mm f/2.8 STM".to_string(),
            4145 => "Canon EF-M 22mm f/2 STM".to_string(),
            4146 => "Canon EF-S 18-55mm f/3.5-5.6 IS STM".to_string(),
            4147 => "Canon EF-M 11-22mm f/4-5.6 IS STM".to_string(),
            4148 => "Canon EF-S 55-250mm f/4-5.6 IS STM".to_string(),
            4149 => "Canon EF-M 55-200mm f/4.5-6.3 IS STM".to_string(),
            4150 => "Canon EF-S 10-18mm f/4.5-5.6 IS STM".to_string(),
            4152 => "Canon EF 24-105mm f/3.5-5.6 IS STM".to_string(),
            4153 => "Canon EF-M 15-45mm f/3.5-6.3 IS STM".to_string(),
            4154 => "Canon EF-S 24mm f/2.8 STM".to_string(),
            4155 => "Canon EF-M 28mm f/3.5 Macro IS STM".to_string(),
            4156 => "Canon EF 50mm f/1.8 STM".to_string(),
            4157 => "Canon EF-M 18-150mm f/3.5-6.3 IS STM".to_string(),
            4158 => "Canon EF-S 18-55mm f/4-5.6 IS STM".to_string(),
            4159 => "Canon EF-M 32mm f/1.4 STM".to_string(),
            4160 => "Canon EF-S 35mm f/2.8 Macro IS STM".to_string(),
            4208 => "Sigma 56mm f/1.4 DC DN | C or other Sigma Lens".to_string(),
            4976 => "Sigma 16-300mm F3.5-6.7 DC OS | C (025)".to_string(),
            6512 => "Sigma 12mm F1.4 DC | C".to_string(),
            36910 => "Canon EF 70-300mm f/4-5.6 IS II USM".to_string(),
            36912 => "Canon EF-S 18-135mm f/3.5-5.6 IS USM".to_string(),
            61182 => "Canon RF 50mm F1.2L USM or other Canon RF Lens".to_string(),
            61491 => "Canon CN-E 14mm T3.1 L F".to_string(),
            61492 => "Canon CN-E 24mm T1.5 L F".to_string(),
            61494 => "Canon CN-E 85mm T1.3 L F".to_string(),
            61495 => "Canon CN-E 135mm T2.2 L F".to_string(),
            61496 => "Canon CN-E 35mm T1.5 L F".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("LensType", 0xe6, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0xe8, bo) {
        dm.push(("MinFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MinFocalLength", 0xe8, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0xea, bo) {
        dm.push(("MaxFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MaxFocalLength", 0xea, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(text) = text_at(data, 0x17e, 6, true) {
        tags.push(mk("FirmwareVersion", 0x17e, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    if let Some(text) = text_at(data, 0x18e, 32, true) {
        tags.push(mk("OwnerName", 0x18e, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x1bb, bo) {
        dm.push(("FileIndex".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val + 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("FileIndex", 0x1bb, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x1c7, bo) {
        dm.push(("DirectoryIndex".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("DirectoryIndex", 0x1c7, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(sub) = data.get(0x2f7..) {
        tags.extend(canon_psinfo(sub, model, bo, file_type, format, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::CameraInfo5DmkIII` -- FORMAT int8u, FIRST_ENTRY 0.
fn canon_camerainfo5dmkiii(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(text) = text_at(data, 0x0, 589, false) {
        tags.push(mk("FirmwareVersionLookAhead", 0x0, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x3) {
        dm.push(("FNumber".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(($val-8)/16*log(2))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("sprintf(\"%.2g\",$val)", &cv, &ctx) { cv = x; }
            tags.push(mk("FNumber", 0x3, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x4) {
        dm.push(("ExposureTime".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(4*log(2)*(1-Image::ExifTool::Canon::CanonEv($val-24)))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("Image::ExifTool::Exif::PrintExposureTime($val)", &cv, &ctx) { cv = x; }
            tags.push(mk("ExposureTime", 0x4, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x6) {
        dm.push(("ISO".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("100*exp(($val/8-9)*log(2))", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("sprintf(\"%.0f\",$val)", &cv, &ctx) { cv = x; }
        tags.push(mk("ISO", 0x6, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x1b) {
        dm.push(("CameraTemperature".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 128", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
        tags.push(mk("CameraTemperature", 0x1b, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x23, bo) {
        dm.push(("FocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
            tags.push(mk("FocalLength", 0x23, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x7d) {
        dm.push(("CameraOrientation".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Horizontal (normal)".to_string(),
            1 => "Rotate 90 CW".to_string(),
            2 => "Rotate 270 CW".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("CameraOrientation", 0x7d, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x8c, bo) {
        dm.push(("FocusDistanceUpper".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceUpper", 0x8c, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x8e, bo) {
        dm.push(("FocusDistanceLower".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceLower", 0x8e, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0xbc, bo) {
        dm.push(("WhiteBalance".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Auto".to_string(),
            1 => "Daylight".to_string(),
            2 => "Cloudy".to_string(),
            3 => "Tungsten".to_string(),
            4 => "Fluorescent".to_string(),
            5 => "Flash".to_string(),
            6 => "Custom".to_string(),
            7 => "Black & White".to_string(),
            8 => "Shade".to_string(),
            9 => "Manual Temperature (Kelvin)".to_string(),
            10 => "PC Set1".to_string(),
            11 => "PC Set2".to_string(),
            12 => "PC Set3".to_string(),
            14 => "Daylight Fluorescent".to_string(),
            15 => "Custom 1".to_string(),
            16 => "Custom 2".to_string(),
            17 => "Underwater".to_string(),
            18 => "Custom 3".to_string(),
            19 => "Custom 4".to_string(),
            20 => "PC Set4".to_string(),
            21 => "PC Set5".to_string(),
            23 => "Auto (ambience priority)".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("WhiteBalance", 0xbc, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0xc0, bo) {
        dm.push(("ColorTemperature".to_string(), f64::from(v)));
        tags.push(mk("ColorTemperature", 0xc0, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0xf4) {
        dm.push(("PictureStyle".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "None".to_string(),
            1 => "Standard".to_string(),
            2 => "Portrait".to_string(),
            3 => "High Saturation".to_string(),
            4 => "Adobe RGB".to_string(),
            5 => "Low Saturation".to_string(),
            6 => "CM Set 1".to_string(),
            7 => "CM Set 2".to_string(),
            33 => "User Def. 1".to_string(),
            34 => "User Def. 2".to_string(),
            35 => "User Def. 3".to_string(),
            65 => "PC 1".to_string(),
            66 => "PC 2".to_string(),
            67 => "PC 3".to_string(),
            129 => "Standard".to_string(),
            130 => "Portrait".to_string(),
            131 => "Landscape".to_string(),
            132 => "Neutral".to_string(),
            133 => "Faithful".to_string(),
            134 => "Monochrome".to_string(),
            135 => "Auto".to_string(),
            136 => "Fine Detail".to_string(),
            255 => "n/a".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("PictureStyle", 0xf4, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x153, bo) {
        dm.push(("LensType".to_string(), f64::from(v)));
        let s = match v as i64 {
            -1 => "n/a".to_string(),
            1 => "Canon EF 50mm f/1.8".to_string(),
            2 => "Canon EF 28mm f/2.8 or Sigma Lens".to_string(),
            3 => "Canon EF 135mm f/2.8 Soft".to_string(),
            4 => "Canon EF 35-105mm f/3.5-4.5 or Sigma Lens".to_string(),
            5 => "Canon EF 35-70mm f/3.5-4.5".to_string(),
            6 => "Canon EF 28-70mm f/3.5-4.5 or Sigma or Tokina Lens".to_string(),
            7 => "Canon EF 100-300mm f/5.6L".to_string(),
            8 => "Canon EF 100-300mm f/5.6 or Sigma or Tokina Lens".to_string(),
            9 => "Canon EF 70-210mm f/4".to_string(),
            10 => "Canon EF 50mm f/2.5 Macro or Sigma Lens".to_string(),
            11 => "Canon EF 35mm f/2".to_string(),
            13 => "Canon EF 15mm f/2.8 Fisheye".to_string(),
            14 => "Canon EF 50-200mm f/3.5-4.5L".to_string(),
            15 => "Canon EF 50-200mm f/3.5-4.5".to_string(),
            16 => "Canon EF 35-135mm f/3.5-4.5".to_string(),
            17 => "Canon EF 35-70mm f/3.5-4.5A".to_string(),
            18 => "Canon EF 28-70mm f/3.5-4.5".to_string(),
            20 => "Canon EF 100-200mm f/4.5A".to_string(),
            21 => "Canon EF 80-200mm f/2.8L".to_string(),
            22 => "Canon EF 20-35mm f/2.8L or Tokina Lens".to_string(),
            23 => "Canon EF 35-105mm f/3.5-4.5".to_string(),
            24 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            25 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            26 => "Canon EF 100mm f/2.8 Macro or Other Lens".to_string(),
            27 => "Canon EF 35-80mm f/4-5.6".to_string(),
            28 => "Canon EF 80-200mm f/4.5-5.6 or Tamron Lens".to_string(),
            29 => "Canon EF 50mm f/1.8 II".to_string(),
            30 => "Canon EF 35-105mm f/4.5-5.6".to_string(),
            31 => "Canon EF 75-300mm f/4-5.6 or Tamron Lens".to_string(),
            32 => "Canon EF 24mm f/2.8 or Sigma Lens".to_string(),
            33 => "Voigtlander or Carl Zeiss Lens".to_string(),
            35 => "Canon EF 35-80mm f/4-5.6".to_string(),
            36 => "Canon EF 38-76mm f/4.5-5.6".to_string(),
            37 => "Canon EF 35-80mm f/4-5.6 or Tamron Lens".to_string(),
            38 => "Canon EF 80-200mm f/4.5-5.6 II".to_string(),
            39 => "Canon EF 75-300mm f/4-5.6".to_string(),
            40 => "Canon EF 28-80mm f/3.5-5.6".to_string(),
            41 => "Canon EF 28-90mm f/4-5.6".to_string(),
            42 => "Canon EF 28-200mm f/3.5-5.6 or Tamron Lens".to_string(),
            43 => "Canon EF 28-105mm f/4-5.6".to_string(),
            44 => "Canon EF 90-300mm f/4.5-5.6".to_string(),
            45 => "Canon EF-S 18-55mm f/3.5-5.6 [II]".to_string(),
            46 => "Canon EF 28-90mm f/4-5.6".to_string(),
            47 => "Zeiss Milvus 35mm f/2 or 50mm f/2".to_string(),
            48 => "Canon EF-S 18-55mm f/3.5-5.6 IS".to_string(),
            49 => "Canon EF-S 55-250mm f/4-5.6 IS".to_string(),
            50 => "Canon EF-S 18-200mm f/3.5-5.6 IS".to_string(),
            51 => "Canon EF-S 18-135mm f/3.5-5.6 IS".to_string(),
            52 => "Canon EF-S 18-55mm f/3.5-5.6 IS II".to_string(),
            53 => "Canon EF-S 18-55mm f/3.5-5.6 III".to_string(),
            54 => "Canon EF-S 55-250mm f/4-5.6 IS II".to_string(),
            60 => "Irix 11mm f/4 or 15mm f/2.4".to_string(),
            63 => "Irix 30mm F1.4 Dragonfly".to_string(),
            80 => "Canon TS-E 50mm f/2.8L Macro".to_string(),
            81 => "Canon TS-E 90mm f/2.8L Macro".to_string(),
            82 => "Canon TS-E 135mm f/4L Macro".to_string(),
            94 => "Canon TS-E 17mm f/4L".to_string(),
            95 => "Canon TS-E 24mm f/3.5L II".to_string(),
            103 => "Samyang AF 14mm f/2.8 EF or Rokinon Lens".to_string(),
            106 => "Rokinon SP / Samyang XP 35mm f/1.2".to_string(),
            112 => "Sigma 28mm f/1.5 FF High-speed Prime or other Sigma Lens".to_string(),
            117 => "Tamron 35-150mm f/2.8-4.0 Di VC OSD (A043) or other Tamron Lens".to_string(),
            124 => "Canon MP-E 65mm f/2.8 1-5x Macro Photo".to_string(),
            125 => "Canon TS-E 24mm f/3.5L".to_string(),
            126 => "Canon TS-E 45mm f/2.8".to_string(),
            127 => "Canon TS-E 90mm f/2.8 or Tamron Lens".to_string(),
            129 => "Canon EF 300mm f/2.8L USM".to_string(),
            130 => "Canon EF 50mm f/1.0L USM".to_string(),
            131 => "Canon EF 28-80mm f/2.8-4L USM or Sigma Lens".to_string(),
            132 => "Canon EF 1200mm f/5.6L USM".to_string(),
            134 => "Canon EF 600mm f/4L IS USM".to_string(),
            135 => "Canon EF 200mm f/1.8L USM".to_string(),
            136 => "Canon EF 300mm f/2.8L USM".to_string(),
            137 => "Canon EF 85mm f/1.2L USM or Sigma or Tamron Lens".to_string(),
            138 => "Canon EF 28-80mm f/2.8-4L".to_string(),
            139 => "Canon EF 400mm f/2.8L USM".to_string(),
            140 => "Canon EF 500mm f/4.5L USM".to_string(),
            141 => "Canon EF 500mm f/4.5L USM".to_string(),
            142 => "Canon EF 300mm f/2.8L IS USM".to_string(),
            143 => "Canon EF 500mm f/4L IS USM or Sigma Lens".to_string(),
            144 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            145 => "Canon EF 100-300mm f/4.5-5.6 USM".to_string(),
            146 => "Canon EF 70-210mm f/3.5-4.5 USM".to_string(),
            147 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            148 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            149 => "Canon EF 100mm f/2 USM".to_string(),
            150 => "Canon EF 14mm f/2.8L USM or Sigma Lens".to_string(),
            151 => "Canon EF 200mm f/2.8L USM".to_string(),
            152 => "Canon EF 300mm f/4L IS USM or Sigma Lens".to_string(),
            153 => "Canon EF 35-350mm f/3.5-5.6L USM or Sigma or Tamron Lens".to_string(),
            154 => "Canon EF 20mm f/2.8 USM or Zeiss Lens".to_string(),
            155 => "Canon EF 85mm f/1.8 USM or Sigma Lens".to_string(),
            156 => "Canon EF 28-105mm f/3.5-4.5 USM or Tamron Lens".to_string(),
            160 => "Canon EF 20-35mm f/3.5-4.5 USM or Tamron or Tokina Lens".to_string(),
            161 => "Canon EF 28-70mm f/2.8L USM or Other Lens".to_string(),
            162 => "Canon EF 200mm f/2.8L USM".to_string(),
            163 => "Canon EF 300mm f/4L".to_string(),
            164 => "Canon EF 400mm f/5.6L".to_string(),
            165 => "Canon EF 70-200mm f/2.8L USM".to_string(),
            166 => "Canon EF 70-200mm f/2.8L USM + 1.4x".to_string(),
            167 => "Canon EF 70-200mm f/2.8L USM + 2x".to_string(),
            168 => "Canon EF 28mm f/1.8 USM or Sigma Lens".to_string(),
            169 => "Canon EF 17-35mm f/2.8L USM or Sigma Lens".to_string(),
            170 => "Canon EF 200mm f/2.8L II USM or Sigma Lens".to_string(),
            171 => "Canon EF 300mm f/4L USM".to_string(),
            172 => "Canon EF 400mm f/5.6L USM or Sigma Lens".to_string(),
            173 => "Canon EF 180mm Macro f/3.5L USM or Sigma Lens".to_string(),
            174 => "Canon EF 135mm f/2L USM or Other Lens".to_string(),
            175 => "Canon EF 400mm f/2.8L USM".to_string(),
            176 => "Canon EF 24-85mm f/3.5-4.5 USM".to_string(),
            177 => "Canon EF 300mm f/4L IS USM".to_string(),
            178 => "Canon EF 28-135mm f/3.5-5.6 IS".to_string(),
            179 => "Canon EF 24mm f/1.4L USM".to_string(),
            180 => "Canon EF 35mm f/1.4L USM or Other Lens".to_string(),
            181 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 1.4x or Sigma Lens".to_string(),
            182 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 2x or Sigma Lens".to_string(),
            183 => "Canon EF 100-400mm f/4.5-5.6L IS USM or Sigma Lens".to_string(),
            184 => "Canon EF 400mm f/2.8L USM + 2x".to_string(),
            185 => "Canon EF 600mm f/4L IS USM".to_string(),
            186 => "Canon EF 70-200mm f/4L USM".to_string(),
            187 => "Canon EF 70-200mm f/4L USM + 1.4x".to_string(),
            188 => "Canon EF 70-200mm f/4L USM + 2x".to_string(),
            189 => "Canon EF 70-200mm f/4L USM + 2.8x".to_string(),
            190 => "Canon EF 100mm f/2.8 Macro USM".to_string(),
            191 => "Canon EF 400mm f/4 DO IS or Sigma Lens".to_string(),
            193 => "Canon EF 35-80mm f/4-5.6 USM".to_string(),
            194 => "Canon EF 80-200mm f/4.5-5.6 USM".to_string(),
            195 => "Canon EF 35-105mm f/4.5-5.6 USM".to_string(),
            196 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            197 => "Canon EF 75-300mm f/4-5.6 IS USM or Sigma Lens".to_string(),
            198 => "Canon EF 50mm f/1.4 USM or Other Lens".to_string(),
            199 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            200 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            201 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            202 => "Canon EF 28-80mm f/3.5-5.6 USM IV".to_string(),
            208 => "Canon EF 22-55mm f/4-5.6 USM".to_string(),
            209 => "Canon EF 55-200mm f/4.5-5.6".to_string(),
            210 => "Canon EF 28-90mm f/4-5.6 USM".to_string(),
            211 => "Canon EF 28-200mm f/3.5-5.6 USM".to_string(),
            212 => "Canon EF 28-105mm f/4-5.6 USM".to_string(),
            213 => "Canon EF 90-300mm f/4.5-5.6 USM or Tamron Lens".to_string(),
            214 => "Canon EF-S 18-55mm f/3.5-5.6 USM".to_string(),
            215 => "Canon EF 55-200mm f/4.5-5.6 II USM".to_string(),
            217 => "Tamron AF 18-270mm f/3.5-6.3 Di II VC PZD".to_string(),
            220 => "Yongnuo YN 50mm f/1.8".to_string(),
            224 => "Canon EF 70-200mm f/2.8L IS USM".to_string(),
            225 => "Canon EF 70-200mm f/2.8L IS USM + 1.4x".to_string(),
            226 => "Canon EF 70-200mm f/2.8L IS USM + 2x".to_string(),
            227 => "Canon EF 70-200mm f/2.8L IS USM + 2.8x".to_string(),
            228 => "Canon EF 28-105mm f/3.5-4.5 USM".to_string(),
            229 => "Canon EF 16-35mm f/2.8L USM".to_string(),
            230 => "Canon EF 24-70mm f/2.8L USM".to_string(),
            231 => "Canon EF 17-40mm f/4L USM or Sigma Lens".to_string(),
            232 => "Canon EF 70-300mm f/4.5-5.6 DO IS USM".to_string(),
            233 => "Canon EF 28-300mm f/3.5-5.6L IS USM".to_string(),
            234 => "Canon EF-S 17-85mm f/4-5.6 IS USM or Tokina Lens".to_string(),
            235 => "Canon EF-S 10-22mm f/3.5-4.5 USM".to_string(),
            236 => "Canon EF-S 60mm f/2.8 Macro USM".to_string(),
            237 => "Canon EF 24-105mm f/4L IS USM".to_string(),
            238 => "Canon EF 70-300mm f/4-5.6 IS USM".to_string(),
            239 => "Canon EF 85mm f/1.2L II USM or Rokinon Lens".to_string(),
            240 => "Canon EF-S 17-55mm f/2.8 IS USM or Sigma Lens".to_string(),
            241 => "Canon EF 50mm f/1.2L USM".to_string(),
            242 => "Canon EF 70-200mm f/4L IS USM".to_string(),
            243 => "Canon EF 70-200mm f/4L IS USM + 1.4x".to_string(),
            244 => "Canon EF 70-200mm f/4L IS USM + 2x".to_string(),
            245 => "Canon EF 70-200mm f/4L IS USM + 2.8x".to_string(),
            246 => "Canon EF 16-35mm f/2.8L II USM".to_string(),
            247 => "Canon EF 14mm f/2.8L II USM".to_string(),
            248 => "Canon EF 200mm f/2L IS USM or Sigma Lens".to_string(),
            249 => "Canon EF 800mm f/5.6L IS USM".to_string(),
            250 => "Canon EF 24mm f/1.4L II USM or Sigma Lens".to_string(),
            251 => "Canon EF 70-200mm f/2.8L IS II USM".to_string(),
            252 => "Canon EF 70-200mm f/2.8L IS II USM + 1.4x".to_string(),
            253 => "Canon EF 70-200mm f/2.8L IS II USM + 2x".to_string(),
            254 => "Canon EF 100mm f/2.8L Macro IS USM or Tamron Lens".to_string(),
            255 => "Sigma 24-105mm f/4 DG OS HSM | A or Other Lens".to_string(),
            368 => "Sigma 14-24mm f/2.8 DG HSM | A or other Sigma Lens".to_string(),
            488 => "Canon EF-S 15-85mm f/3.5-5.6 IS USM".to_string(),
            489 => "Canon EF 70-300mm f/4-5.6L IS USM".to_string(),
            490 => "Canon EF 8-15mm f/4L Fisheye USM".to_string(),
            491 => "Canon EF 300mm f/2.8L IS II USM or Tamron Lens".to_string(),
            492 => "Canon EF 400mm f/2.8L IS II USM".to_string(),
            493 => "Canon EF 500mm f/4L IS II USM or EF 24-105mm f4L IS USM".to_string(),
            494 => "Canon EF 600mm f/4L IS II USM".to_string(),
            495 => "Canon EF 24-70mm f/2.8L II USM or Sigma Lens".to_string(),
            496 => "Canon EF 200-400mm f/4L IS USM".to_string(),
            499 => "Canon EF 200-400mm f/4L IS USM + 1.4x".to_string(),
            502 => "Canon EF 28mm f/2.8 IS USM or Tamron Lens".to_string(),
            503 => "Canon EF 24mm f/2.8 IS USM".to_string(),
            504 => "Canon EF 24-70mm f/4L IS USM".to_string(),
            505 => "Canon EF 35mm f/2 IS USM".to_string(),
            506 => "Canon EF 400mm f/4 DO IS II USM".to_string(),
            507 => "Canon EF 16-35mm f/4L IS USM".to_string(),
            508 => "Canon EF 11-24mm f/4L USM or Tamron Lens".to_string(),
            624 => "Sigma 70-200mm f/2.8 DG OS HSM | S or other Sigma Lens".to_string(),
            747 => "Canon EF 100-400mm f/4.5-5.6L IS II USM or Tamron Lens".to_string(),
            748 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 1.4x or Tamron Lens".to_string(),
            749 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 2x or Tamron Lens".to_string(),
            750 => "Canon EF 35mm f/1.4L II USM or Tamron Lens".to_string(),
            751 => "Canon EF 16-35mm f/2.8L III USM".to_string(),
            752 => "Canon EF 24-105mm f/4L IS II USM".to_string(),
            753 => "Canon EF 85mm f/1.4L IS USM".to_string(),
            754 => "Canon EF 70-200mm f/4L IS II USM".to_string(),
            757 => "Canon EF 400mm f/2.8L IS III USM".to_string(),
            758 => "Canon EF 600mm f/4L IS III USM".to_string(),
            923 => "Meike/SKY 85mm f/1.8 DCM".to_string(),
            1136 => "Sigma 24-70mm f/2.8 DG OS HSM | A".to_string(),
            4142 => "Canon EF-S 18-135mm f/3.5-5.6 IS STM".to_string(),
            4143 => "Canon EF-M 18-55mm f/3.5-5.6 IS STM or Tamron Lens".to_string(),
            4144 => "Canon EF 40mm f/2.8 STM".to_string(),
            4145 => "Canon EF-M 22mm f/2 STM".to_string(),
            4146 => "Canon EF-S 18-55mm f/3.5-5.6 IS STM".to_string(),
            4147 => "Canon EF-M 11-22mm f/4-5.6 IS STM".to_string(),
            4148 => "Canon EF-S 55-250mm f/4-5.6 IS STM".to_string(),
            4149 => "Canon EF-M 55-200mm f/4.5-6.3 IS STM".to_string(),
            4150 => "Canon EF-S 10-18mm f/4.5-5.6 IS STM".to_string(),
            4152 => "Canon EF 24-105mm f/3.5-5.6 IS STM".to_string(),
            4153 => "Canon EF-M 15-45mm f/3.5-6.3 IS STM".to_string(),
            4154 => "Canon EF-S 24mm f/2.8 STM".to_string(),
            4155 => "Canon EF-M 28mm f/3.5 Macro IS STM".to_string(),
            4156 => "Canon EF 50mm f/1.8 STM".to_string(),
            4157 => "Canon EF-M 18-150mm f/3.5-6.3 IS STM".to_string(),
            4158 => "Canon EF-S 18-55mm f/4-5.6 IS STM".to_string(),
            4159 => "Canon EF-M 32mm f/1.4 STM".to_string(),
            4160 => "Canon EF-S 35mm f/2.8 Macro IS STM".to_string(),
            4208 => "Sigma 56mm f/1.4 DC DN | C or other Sigma Lens".to_string(),
            4976 => "Sigma 16-300mm F3.5-6.7 DC OS | C (025)".to_string(),
            6512 => "Sigma 12mm F1.4 DC | C".to_string(),
            36910 => "Canon EF 70-300mm f/4-5.6 IS II USM".to_string(),
            36912 => "Canon EF-S 18-135mm f/3.5-5.6 IS USM".to_string(),
            61182 => "Canon RF 50mm F1.2L USM or other Canon RF Lens".to_string(),
            61491 => "Canon CN-E 14mm T3.1 L F".to_string(),
            61492 => "Canon CN-E 24mm T1.5 L F".to_string(),
            61494 => "Canon CN-E 85mm T1.3 L F".to_string(),
            61495 => "Canon CN-E 135mm T2.2 L F".to_string(),
            61496 => "Canon CN-E 35mm T1.5 L F".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("LensType", 0x153, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x155, bo) {
        dm.push(("MinFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MinFocalLength", 0x155, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x157, bo) {
        dm.push(("MaxFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MaxFocalLength", 0x157, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(text) = text_at(data, 0x164, 5, false) {
        tags.push(mk("LensSerialNumber", 0x164, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    if let Some(text) = text_at(data, 0x23c, 6, true) {
        tags.push(mk("FirmwareVersion", 0x23c, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x28c, bo) {
        dm.push(("FileIndex".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val + 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("FileIndex", 0x28c, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x290, bo) {
        dm.push(("FileIndex2".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val + 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("FileIndex2", 0x290, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x298, bo) {
        dm.push(("DirectoryIndex".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("DirectoryIndex", 0x298, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x29c, bo) {
        dm.push(("DirectoryIndex2".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("DirectoryIndex2", 0x29c, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(sub) = data.get(0x3b0..) {
        tags.extend(canon_psinfo2(sub, model, bo, file_type, format, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::CameraInfo6D` -- FORMAT int8u, FIRST_ENTRY 0.
fn canon_camerainfo6d(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = u8_at(data, 0x3) {
        dm.push(("FNumber".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(($val-8)/16*log(2))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("sprintf(\"%.2g\",$val)", &cv, &ctx) { cv = x; }
            tags.push(mk("FNumber", 0x3, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x4) {
        dm.push(("ExposureTime".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(4*log(2)*(1-Image::ExifTool::Canon::CanonEv($val-24)))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("Image::ExifTool::Exif::PrintExposureTime($val)", &cv, &ctx) { cv = x; }
            tags.push(mk("ExposureTime", 0x4, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x6) {
        dm.push(("ISO".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("100*exp(($val/8-9)*log(2))", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("sprintf(\"%.0f\",$val)", &cv, &ctx) { cv = x; }
        tags.push(mk("ISO", 0x6, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x1b) {
        dm.push(("CameraTemperature".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 128", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
        tags.push(mk("CameraTemperature", 0x1b, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x23, bo) {
        dm.push(("FocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
            tags.push(mk("FocalLength", 0x23, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x83) {
        dm.push(("CameraOrientation".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Horizontal (normal)".to_string(),
            1 => "Rotate 90 CW".to_string(),
            2 => "Rotate 270 CW".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("CameraOrientation", 0x83, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x92, bo) {
        dm.push(("FocusDistanceUpper".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceUpper", 0x92, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x94, bo) {
        dm.push(("FocusDistanceLower".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceLower", 0x94, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0xc2, bo) {
        dm.push(("WhiteBalance".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Auto".to_string(),
            1 => "Daylight".to_string(),
            2 => "Cloudy".to_string(),
            3 => "Tungsten".to_string(),
            4 => "Fluorescent".to_string(),
            5 => "Flash".to_string(),
            6 => "Custom".to_string(),
            7 => "Black & White".to_string(),
            8 => "Shade".to_string(),
            9 => "Manual Temperature (Kelvin)".to_string(),
            10 => "PC Set1".to_string(),
            11 => "PC Set2".to_string(),
            12 => "PC Set3".to_string(),
            14 => "Daylight Fluorescent".to_string(),
            15 => "Custom 1".to_string(),
            16 => "Custom 2".to_string(),
            17 => "Underwater".to_string(),
            18 => "Custom 3".to_string(),
            19 => "Custom 4".to_string(),
            20 => "PC Set4".to_string(),
            21 => "PC Set5".to_string(),
            23 => "Auto (ambience priority)".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("WhiteBalance", 0xc2, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0xc6, bo) {
        dm.push(("ColorTemperature".to_string(), f64::from(v)));
        tags.push(mk("ColorTemperature", 0xc6, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0xfa) {
        dm.push(("PictureStyle".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "None".to_string(),
            1 => "Standard".to_string(),
            2 => "Portrait".to_string(),
            3 => "High Saturation".to_string(),
            4 => "Adobe RGB".to_string(),
            5 => "Low Saturation".to_string(),
            6 => "CM Set 1".to_string(),
            7 => "CM Set 2".to_string(),
            33 => "User Def. 1".to_string(),
            34 => "User Def. 2".to_string(),
            35 => "User Def. 3".to_string(),
            65 => "PC 1".to_string(),
            66 => "PC 2".to_string(),
            67 => "PC 3".to_string(),
            129 => "Standard".to_string(),
            130 => "Portrait".to_string(),
            131 => "Landscape".to_string(),
            132 => "Neutral".to_string(),
            133 => "Faithful".to_string(),
            134 => "Monochrome".to_string(),
            135 => "Auto".to_string(),
            136 => "Fine Detail".to_string(),
            255 => "n/a".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("PictureStyle", 0xfa, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x161, bo) {
        dm.push(("LensType".to_string(), f64::from(v)));
        let s = match v as i64 {
            -1 => "n/a".to_string(),
            1 => "Canon EF 50mm f/1.8".to_string(),
            2 => "Canon EF 28mm f/2.8 or Sigma Lens".to_string(),
            3 => "Canon EF 135mm f/2.8 Soft".to_string(),
            4 => "Canon EF 35-105mm f/3.5-4.5 or Sigma Lens".to_string(),
            5 => "Canon EF 35-70mm f/3.5-4.5".to_string(),
            6 => "Canon EF 28-70mm f/3.5-4.5 or Sigma or Tokina Lens".to_string(),
            7 => "Canon EF 100-300mm f/5.6L".to_string(),
            8 => "Canon EF 100-300mm f/5.6 or Sigma or Tokina Lens".to_string(),
            9 => "Canon EF 70-210mm f/4".to_string(),
            10 => "Canon EF 50mm f/2.5 Macro or Sigma Lens".to_string(),
            11 => "Canon EF 35mm f/2".to_string(),
            13 => "Canon EF 15mm f/2.8 Fisheye".to_string(),
            14 => "Canon EF 50-200mm f/3.5-4.5L".to_string(),
            15 => "Canon EF 50-200mm f/3.5-4.5".to_string(),
            16 => "Canon EF 35-135mm f/3.5-4.5".to_string(),
            17 => "Canon EF 35-70mm f/3.5-4.5A".to_string(),
            18 => "Canon EF 28-70mm f/3.5-4.5".to_string(),
            20 => "Canon EF 100-200mm f/4.5A".to_string(),
            21 => "Canon EF 80-200mm f/2.8L".to_string(),
            22 => "Canon EF 20-35mm f/2.8L or Tokina Lens".to_string(),
            23 => "Canon EF 35-105mm f/3.5-4.5".to_string(),
            24 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            25 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            26 => "Canon EF 100mm f/2.8 Macro or Other Lens".to_string(),
            27 => "Canon EF 35-80mm f/4-5.6".to_string(),
            28 => "Canon EF 80-200mm f/4.5-5.6 or Tamron Lens".to_string(),
            29 => "Canon EF 50mm f/1.8 II".to_string(),
            30 => "Canon EF 35-105mm f/4.5-5.6".to_string(),
            31 => "Canon EF 75-300mm f/4-5.6 or Tamron Lens".to_string(),
            32 => "Canon EF 24mm f/2.8 or Sigma Lens".to_string(),
            33 => "Voigtlander or Carl Zeiss Lens".to_string(),
            35 => "Canon EF 35-80mm f/4-5.6".to_string(),
            36 => "Canon EF 38-76mm f/4.5-5.6".to_string(),
            37 => "Canon EF 35-80mm f/4-5.6 or Tamron Lens".to_string(),
            38 => "Canon EF 80-200mm f/4.5-5.6 II".to_string(),
            39 => "Canon EF 75-300mm f/4-5.6".to_string(),
            40 => "Canon EF 28-80mm f/3.5-5.6".to_string(),
            41 => "Canon EF 28-90mm f/4-5.6".to_string(),
            42 => "Canon EF 28-200mm f/3.5-5.6 or Tamron Lens".to_string(),
            43 => "Canon EF 28-105mm f/4-5.6".to_string(),
            44 => "Canon EF 90-300mm f/4.5-5.6".to_string(),
            45 => "Canon EF-S 18-55mm f/3.5-5.6 [II]".to_string(),
            46 => "Canon EF 28-90mm f/4-5.6".to_string(),
            47 => "Zeiss Milvus 35mm f/2 or 50mm f/2".to_string(),
            48 => "Canon EF-S 18-55mm f/3.5-5.6 IS".to_string(),
            49 => "Canon EF-S 55-250mm f/4-5.6 IS".to_string(),
            50 => "Canon EF-S 18-200mm f/3.5-5.6 IS".to_string(),
            51 => "Canon EF-S 18-135mm f/3.5-5.6 IS".to_string(),
            52 => "Canon EF-S 18-55mm f/3.5-5.6 IS II".to_string(),
            53 => "Canon EF-S 18-55mm f/3.5-5.6 III".to_string(),
            54 => "Canon EF-S 55-250mm f/4-5.6 IS II".to_string(),
            60 => "Irix 11mm f/4 or 15mm f/2.4".to_string(),
            63 => "Irix 30mm F1.4 Dragonfly".to_string(),
            80 => "Canon TS-E 50mm f/2.8L Macro".to_string(),
            81 => "Canon TS-E 90mm f/2.8L Macro".to_string(),
            82 => "Canon TS-E 135mm f/4L Macro".to_string(),
            94 => "Canon TS-E 17mm f/4L".to_string(),
            95 => "Canon TS-E 24mm f/3.5L II".to_string(),
            103 => "Samyang AF 14mm f/2.8 EF or Rokinon Lens".to_string(),
            106 => "Rokinon SP / Samyang XP 35mm f/1.2".to_string(),
            112 => "Sigma 28mm f/1.5 FF High-speed Prime or other Sigma Lens".to_string(),
            117 => "Tamron 35-150mm f/2.8-4.0 Di VC OSD (A043) or other Tamron Lens".to_string(),
            124 => "Canon MP-E 65mm f/2.8 1-5x Macro Photo".to_string(),
            125 => "Canon TS-E 24mm f/3.5L".to_string(),
            126 => "Canon TS-E 45mm f/2.8".to_string(),
            127 => "Canon TS-E 90mm f/2.8 or Tamron Lens".to_string(),
            129 => "Canon EF 300mm f/2.8L USM".to_string(),
            130 => "Canon EF 50mm f/1.0L USM".to_string(),
            131 => "Canon EF 28-80mm f/2.8-4L USM or Sigma Lens".to_string(),
            132 => "Canon EF 1200mm f/5.6L USM".to_string(),
            134 => "Canon EF 600mm f/4L IS USM".to_string(),
            135 => "Canon EF 200mm f/1.8L USM".to_string(),
            136 => "Canon EF 300mm f/2.8L USM".to_string(),
            137 => "Canon EF 85mm f/1.2L USM or Sigma or Tamron Lens".to_string(),
            138 => "Canon EF 28-80mm f/2.8-4L".to_string(),
            139 => "Canon EF 400mm f/2.8L USM".to_string(),
            140 => "Canon EF 500mm f/4.5L USM".to_string(),
            141 => "Canon EF 500mm f/4.5L USM".to_string(),
            142 => "Canon EF 300mm f/2.8L IS USM".to_string(),
            143 => "Canon EF 500mm f/4L IS USM or Sigma Lens".to_string(),
            144 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            145 => "Canon EF 100-300mm f/4.5-5.6 USM".to_string(),
            146 => "Canon EF 70-210mm f/3.5-4.5 USM".to_string(),
            147 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            148 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            149 => "Canon EF 100mm f/2 USM".to_string(),
            150 => "Canon EF 14mm f/2.8L USM or Sigma Lens".to_string(),
            151 => "Canon EF 200mm f/2.8L USM".to_string(),
            152 => "Canon EF 300mm f/4L IS USM or Sigma Lens".to_string(),
            153 => "Canon EF 35-350mm f/3.5-5.6L USM or Sigma or Tamron Lens".to_string(),
            154 => "Canon EF 20mm f/2.8 USM or Zeiss Lens".to_string(),
            155 => "Canon EF 85mm f/1.8 USM or Sigma Lens".to_string(),
            156 => "Canon EF 28-105mm f/3.5-4.5 USM or Tamron Lens".to_string(),
            160 => "Canon EF 20-35mm f/3.5-4.5 USM or Tamron or Tokina Lens".to_string(),
            161 => "Canon EF 28-70mm f/2.8L USM or Other Lens".to_string(),
            162 => "Canon EF 200mm f/2.8L USM".to_string(),
            163 => "Canon EF 300mm f/4L".to_string(),
            164 => "Canon EF 400mm f/5.6L".to_string(),
            165 => "Canon EF 70-200mm f/2.8L USM".to_string(),
            166 => "Canon EF 70-200mm f/2.8L USM + 1.4x".to_string(),
            167 => "Canon EF 70-200mm f/2.8L USM + 2x".to_string(),
            168 => "Canon EF 28mm f/1.8 USM or Sigma Lens".to_string(),
            169 => "Canon EF 17-35mm f/2.8L USM or Sigma Lens".to_string(),
            170 => "Canon EF 200mm f/2.8L II USM or Sigma Lens".to_string(),
            171 => "Canon EF 300mm f/4L USM".to_string(),
            172 => "Canon EF 400mm f/5.6L USM or Sigma Lens".to_string(),
            173 => "Canon EF 180mm Macro f/3.5L USM or Sigma Lens".to_string(),
            174 => "Canon EF 135mm f/2L USM or Other Lens".to_string(),
            175 => "Canon EF 400mm f/2.8L USM".to_string(),
            176 => "Canon EF 24-85mm f/3.5-4.5 USM".to_string(),
            177 => "Canon EF 300mm f/4L IS USM".to_string(),
            178 => "Canon EF 28-135mm f/3.5-5.6 IS".to_string(),
            179 => "Canon EF 24mm f/1.4L USM".to_string(),
            180 => "Canon EF 35mm f/1.4L USM or Other Lens".to_string(),
            181 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 1.4x or Sigma Lens".to_string(),
            182 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 2x or Sigma Lens".to_string(),
            183 => "Canon EF 100-400mm f/4.5-5.6L IS USM or Sigma Lens".to_string(),
            184 => "Canon EF 400mm f/2.8L USM + 2x".to_string(),
            185 => "Canon EF 600mm f/4L IS USM".to_string(),
            186 => "Canon EF 70-200mm f/4L USM".to_string(),
            187 => "Canon EF 70-200mm f/4L USM + 1.4x".to_string(),
            188 => "Canon EF 70-200mm f/4L USM + 2x".to_string(),
            189 => "Canon EF 70-200mm f/4L USM + 2.8x".to_string(),
            190 => "Canon EF 100mm f/2.8 Macro USM".to_string(),
            191 => "Canon EF 400mm f/4 DO IS or Sigma Lens".to_string(),
            193 => "Canon EF 35-80mm f/4-5.6 USM".to_string(),
            194 => "Canon EF 80-200mm f/4.5-5.6 USM".to_string(),
            195 => "Canon EF 35-105mm f/4.5-5.6 USM".to_string(),
            196 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            197 => "Canon EF 75-300mm f/4-5.6 IS USM or Sigma Lens".to_string(),
            198 => "Canon EF 50mm f/1.4 USM or Other Lens".to_string(),
            199 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            200 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            201 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            202 => "Canon EF 28-80mm f/3.5-5.6 USM IV".to_string(),
            208 => "Canon EF 22-55mm f/4-5.6 USM".to_string(),
            209 => "Canon EF 55-200mm f/4.5-5.6".to_string(),
            210 => "Canon EF 28-90mm f/4-5.6 USM".to_string(),
            211 => "Canon EF 28-200mm f/3.5-5.6 USM".to_string(),
            212 => "Canon EF 28-105mm f/4-5.6 USM".to_string(),
            213 => "Canon EF 90-300mm f/4.5-5.6 USM or Tamron Lens".to_string(),
            214 => "Canon EF-S 18-55mm f/3.5-5.6 USM".to_string(),
            215 => "Canon EF 55-200mm f/4.5-5.6 II USM".to_string(),
            217 => "Tamron AF 18-270mm f/3.5-6.3 Di II VC PZD".to_string(),
            220 => "Yongnuo YN 50mm f/1.8".to_string(),
            224 => "Canon EF 70-200mm f/2.8L IS USM".to_string(),
            225 => "Canon EF 70-200mm f/2.8L IS USM + 1.4x".to_string(),
            226 => "Canon EF 70-200mm f/2.8L IS USM + 2x".to_string(),
            227 => "Canon EF 70-200mm f/2.8L IS USM + 2.8x".to_string(),
            228 => "Canon EF 28-105mm f/3.5-4.5 USM".to_string(),
            229 => "Canon EF 16-35mm f/2.8L USM".to_string(),
            230 => "Canon EF 24-70mm f/2.8L USM".to_string(),
            231 => "Canon EF 17-40mm f/4L USM or Sigma Lens".to_string(),
            232 => "Canon EF 70-300mm f/4.5-5.6 DO IS USM".to_string(),
            233 => "Canon EF 28-300mm f/3.5-5.6L IS USM".to_string(),
            234 => "Canon EF-S 17-85mm f/4-5.6 IS USM or Tokina Lens".to_string(),
            235 => "Canon EF-S 10-22mm f/3.5-4.5 USM".to_string(),
            236 => "Canon EF-S 60mm f/2.8 Macro USM".to_string(),
            237 => "Canon EF 24-105mm f/4L IS USM".to_string(),
            238 => "Canon EF 70-300mm f/4-5.6 IS USM".to_string(),
            239 => "Canon EF 85mm f/1.2L II USM or Rokinon Lens".to_string(),
            240 => "Canon EF-S 17-55mm f/2.8 IS USM or Sigma Lens".to_string(),
            241 => "Canon EF 50mm f/1.2L USM".to_string(),
            242 => "Canon EF 70-200mm f/4L IS USM".to_string(),
            243 => "Canon EF 70-200mm f/4L IS USM + 1.4x".to_string(),
            244 => "Canon EF 70-200mm f/4L IS USM + 2x".to_string(),
            245 => "Canon EF 70-200mm f/4L IS USM + 2.8x".to_string(),
            246 => "Canon EF 16-35mm f/2.8L II USM".to_string(),
            247 => "Canon EF 14mm f/2.8L II USM".to_string(),
            248 => "Canon EF 200mm f/2L IS USM or Sigma Lens".to_string(),
            249 => "Canon EF 800mm f/5.6L IS USM".to_string(),
            250 => "Canon EF 24mm f/1.4L II USM or Sigma Lens".to_string(),
            251 => "Canon EF 70-200mm f/2.8L IS II USM".to_string(),
            252 => "Canon EF 70-200mm f/2.8L IS II USM + 1.4x".to_string(),
            253 => "Canon EF 70-200mm f/2.8L IS II USM + 2x".to_string(),
            254 => "Canon EF 100mm f/2.8L Macro IS USM or Tamron Lens".to_string(),
            255 => "Sigma 24-105mm f/4 DG OS HSM | A or Other Lens".to_string(),
            368 => "Sigma 14-24mm f/2.8 DG HSM | A or other Sigma Lens".to_string(),
            488 => "Canon EF-S 15-85mm f/3.5-5.6 IS USM".to_string(),
            489 => "Canon EF 70-300mm f/4-5.6L IS USM".to_string(),
            490 => "Canon EF 8-15mm f/4L Fisheye USM".to_string(),
            491 => "Canon EF 300mm f/2.8L IS II USM or Tamron Lens".to_string(),
            492 => "Canon EF 400mm f/2.8L IS II USM".to_string(),
            493 => "Canon EF 500mm f/4L IS II USM or EF 24-105mm f4L IS USM".to_string(),
            494 => "Canon EF 600mm f/4L IS II USM".to_string(),
            495 => "Canon EF 24-70mm f/2.8L II USM or Sigma Lens".to_string(),
            496 => "Canon EF 200-400mm f/4L IS USM".to_string(),
            499 => "Canon EF 200-400mm f/4L IS USM + 1.4x".to_string(),
            502 => "Canon EF 28mm f/2.8 IS USM or Tamron Lens".to_string(),
            503 => "Canon EF 24mm f/2.8 IS USM".to_string(),
            504 => "Canon EF 24-70mm f/4L IS USM".to_string(),
            505 => "Canon EF 35mm f/2 IS USM".to_string(),
            506 => "Canon EF 400mm f/4 DO IS II USM".to_string(),
            507 => "Canon EF 16-35mm f/4L IS USM".to_string(),
            508 => "Canon EF 11-24mm f/4L USM or Tamron Lens".to_string(),
            624 => "Sigma 70-200mm f/2.8 DG OS HSM | S or other Sigma Lens".to_string(),
            747 => "Canon EF 100-400mm f/4.5-5.6L IS II USM or Tamron Lens".to_string(),
            748 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 1.4x or Tamron Lens".to_string(),
            749 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 2x or Tamron Lens".to_string(),
            750 => "Canon EF 35mm f/1.4L II USM or Tamron Lens".to_string(),
            751 => "Canon EF 16-35mm f/2.8L III USM".to_string(),
            752 => "Canon EF 24-105mm f/4L IS II USM".to_string(),
            753 => "Canon EF 85mm f/1.4L IS USM".to_string(),
            754 => "Canon EF 70-200mm f/4L IS II USM".to_string(),
            757 => "Canon EF 400mm f/2.8L IS III USM".to_string(),
            758 => "Canon EF 600mm f/4L IS III USM".to_string(),
            923 => "Meike/SKY 85mm f/1.8 DCM".to_string(),
            1136 => "Sigma 24-70mm f/2.8 DG OS HSM | A".to_string(),
            4142 => "Canon EF-S 18-135mm f/3.5-5.6 IS STM".to_string(),
            4143 => "Canon EF-M 18-55mm f/3.5-5.6 IS STM or Tamron Lens".to_string(),
            4144 => "Canon EF 40mm f/2.8 STM".to_string(),
            4145 => "Canon EF-M 22mm f/2 STM".to_string(),
            4146 => "Canon EF-S 18-55mm f/3.5-5.6 IS STM".to_string(),
            4147 => "Canon EF-M 11-22mm f/4-5.6 IS STM".to_string(),
            4148 => "Canon EF-S 55-250mm f/4-5.6 IS STM".to_string(),
            4149 => "Canon EF-M 55-200mm f/4.5-6.3 IS STM".to_string(),
            4150 => "Canon EF-S 10-18mm f/4.5-5.6 IS STM".to_string(),
            4152 => "Canon EF 24-105mm f/3.5-5.6 IS STM".to_string(),
            4153 => "Canon EF-M 15-45mm f/3.5-6.3 IS STM".to_string(),
            4154 => "Canon EF-S 24mm f/2.8 STM".to_string(),
            4155 => "Canon EF-M 28mm f/3.5 Macro IS STM".to_string(),
            4156 => "Canon EF 50mm f/1.8 STM".to_string(),
            4157 => "Canon EF-M 18-150mm f/3.5-6.3 IS STM".to_string(),
            4158 => "Canon EF-S 18-55mm f/4-5.6 IS STM".to_string(),
            4159 => "Canon EF-M 32mm f/1.4 STM".to_string(),
            4160 => "Canon EF-S 35mm f/2.8 Macro IS STM".to_string(),
            4208 => "Sigma 56mm f/1.4 DC DN | C or other Sigma Lens".to_string(),
            4976 => "Sigma 16-300mm F3.5-6.7 DC OS | C (025)".to_string(),
            6512 => "Sigma 12mm F1.4 DC | C".to_string(),
            36910 => "Canon EF 70-300mm f/4-5.6 IS II USM".to_string(),
            36912 => "Canon EF-S 18-135mm f/3.5-5.6 IS USM".to_string(),
            61182 => "Canon RF 50mm F1.2L USM or other Canon RF Lens".to_string(),
            61491 => "Canon CN-E 14mm T3.1 L F".to_string(),
            61492 => "Canon CN-E 24mm T1.5 L F".to_string(),
            61494 => "Canon CN-E 85mm T1.3 L F".to_string(),
            61495 => "Canon CN-E 135mm T2.2 L F".to_string(),
            61496 => "Canon CN-E 35mm T1.5 L F".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("LensType", 0x161, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x163, bo) {
        dm.push(("MinFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MinFocalLength", 0x163, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x165, bo) {
        dm.push(("MaxFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MaxFocalLength", 0x165, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(text) = text_at(data, 0x256, 6, true) {
        tags.push(mk("FirmwareVersion", 0x256, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x2aa, bo) {
        dm.push(("FileIndex".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val + 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("FileIndex", 0x2aa, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x2b6, bo) {
        dm.push(("DirectoryIndex".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("DirectoryIndex", 0x2b6, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(sub) = data.get(0x3c6..) {
        tags.extend(canon_psinfo2(sub, model, bo, file_type, format, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::CameraInfo7D` -- FORMAT int8u, FIRST_ENTRY 0.
fn canon_camerainfo7d(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(text) = text_at(data, 0x0, 434, false) {
        tags.push(mk("FirmwareVersionLookAhead", 0x0, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x3) {
        dm.push(("FNumber".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(($val-8)/16*log(2))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("sprintf(\"%.2g\",$val)", &cv, &ctx) { cv = x; }
            tags.push(mk("FNumber", 0x3, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x4) {
        dm.push(("ExposureTime".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(4*log(2)*(1-Image::ExifTool::Canon::CanonEv($val-24)))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("Image::ExifTool::Exif::PrintExposureTime($val)", &cv, &ctx) { cv = x; }
            tags.push(mk("ExposureTime", 0x4, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x6) {
        dm.push(("ISO".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("100*exp(($val/8-9)*log(2))", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("sprintf(\"%.0f\",$val)", &cv, &ctx) { cv = x; }
        tags.push(mk("ISO", 0x6, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x7) {
        dm.push(("HighlightTonePriority".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "People".to_string(),
            1 => "sRGB".to_string(),
            2 => "Adobe RGB".to_string(),
            3 => "User 1".to_string(),
            4 => "User 2".to_string(),
            5 => "User 3".to_string(),
            6 => "To Do".to_string(),
            65535 => "n/a".to_string(),
            2415919104 => "Format 1".to_string(),
            2684354560 => "Format 2".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("HighlightTonePriority", 0x7, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x8) {
        dm.push(("MeasuredEV2".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("$val / 8 - 6", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            tags.push(mk("MeasuredEV2", 0x8, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x9) {
        dm.push(("MeasuredEV".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("$val / 8 - 6", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            tags.push(mk("MeasuredEV", 0x9, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x15) {
        dm.push(("FlashMeteringMode".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "E-TTL".to_string(),
            3 => "TTL".to_string(),
            4 => "External Auto".to_string(),
            5 => "External Manual".to_string(),
            6 => "Off".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("FlashMeteringMode", 0x15, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x19) {
        dm.push(("CameraTemperature".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 128", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
        tags.push(mk("CameraTemperature", 0x19, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x1e, bo) {
        dm.push(("FocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
            tags.push(mk("FocalLength", 0x1e, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x35) {
        dm.push(("CameraOrientation".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Horizontal (normal)".to_string(),
            1 => "Rotate 90 CW".to_string(),
            2 => "Rotate 270 CW".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("CameraOrientation", 0x35, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x54, bo) {
        dm.push(("FocusDistanceUpper".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceUpper", 0x54, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x56, bo) {
        dm.push(("FocusDistanceLower".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceLower", 0x56, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x77, bo) {
        dm.push(("WhiteBalance".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Auto".to_string(),
            1 => "Daylight".to_string(),
            2 => "Cloudy".to_string(),
            3 => "Tungsten".to_string(),
            4 => "Fluorescent".to_string(),
            5 => "Flash".to_string(),
            6 => "Custom".to_string(),
            7 => "Black & White".to_string(),
            8 => "Shade".to_string(),
            9 => "Manual Temperature (Kelvin)".to_string(),
            10 => "PC Set1".to_string(),
            11 => "PC Set2".to_string(),
            12 => "PC Set3".to_string(),
            14 => "Daylight Fluorescent".to_string(),
            15 => "Custom 1".to_string(),
            16 => "Custom 2".to_string(),
            17 => "Underwater".to_string(),
            18 => "Custom 3".to_string(),
            19 => "Custom 4".to_string(),
            20 => "PC Set4".to_string(),
            21 => "PC Set5".to_string(),
            23 => "Auto (ambience priority)".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("WhiteBalance", 0x77, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x7b, bo) {
        dm.push(("ColorTemperature".to_string(), f64::from(v)));
        tags.push(mk("ColorTemperature", 0x7b, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0xaf) {
        dm.push(("CameraPictureStyle".to_string(), f64::from(v)));
        let s = match v as i64 {
            33 => "User Defined 1".to_string(),
            34 => "User Defined 2".to_string(),
            35 => "User Defined 3".to_string(),
            129 => "Standard".to_string(),
            130 => "Portrait".to_string(),
            131 => "Landscape".to_string(),
            132 => "Neutral".to_string(),
            133 => "Faithful".to_string(),
            134 => "Monochrome".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("CameraPictureStyle", 0xaf, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0xc9) {
        dm.push(("HighISONoiseReduction".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Standard".to_string(),
            1 => "Low".to_string(),
            2 => "Strong".to_string(),
            3 => "Off".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("HighISONoiseReduction", 0xc9, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x112, bo) {
        dm.push(("LensType".to_string(), f64::from(v)));
        let s = match v as i64 {
            -1 => "n/a".to_string(),
            1 => "Canon EF 50mm f/1.8".to_string(),
            2 => "Canon EF 28mm f/2.8 or Sigma Lens".to_string(),
            3 => "Canon EF 135mm f/2.8 Soft".to_string(),
            4 => "Canon EF 35-105mm f/3.5-4.5 or Sigma Lens".to_string(),
            5 => "Canon EF 35-70mm f/3.5-4.5".to_string(),
            6 => "Canon EF 28-70mm f/3.5-4.5 or Sigma or Tokina Lens".to_string(),
            7 => "Canon EF 100-300mm f/5.6L".to_string(),
            8 => "Canon EF 100-300mm f/5.6 or Sigma or Tokina Lens".to_string(),
            9 => "Canon EF 70-210mm f/4".to_string(),
            10 => "Canon EF 50mm f/2.5 Macro or Sigma Lens".to_string(),
            11 => "Canon EF 35mm f/2".to_string(),
            13 => "Canon EF 15mm f/2.8 Fisheye".to_string(),
            14 => "Canon EF 50-200mm f/3.5-4.5L".to_string(),
            15 => "Canon EF 50-200mm f/3.5-4.5".to_string(),
            16 => "Canon EF 35-135mm f/3.5-4.5".to_string(),
            17 => "Canon EF 35-70mm f/3.5-4.5A".to_string(),
            18 => "Canon EF 28-70mm f/3.5-4.5".to_string(),
            20 => "Canon EF 100-200mm f/4.5A".to_string(),
            21 => "Canon EF 80-200mm f/2.8L".to_string(),
            22 => "Canon EF 20-35mm f/2.8L or Tokina Lens".to_string(),
            23 => "Canon EF 35-105mm f/3.5-4.5".to_string(),
            24 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            25 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            26 => "Canon EF 100mm f/2.8 Macro or Other Lens".to_string(),
            27 => "Canon EF 35-80mm f/4-5.6".to_string(),
            28 => "Canon EF 80-200mm f/4.5-5.6 or Tamron Lens".to_string(),
            29 => "Canon EF 50mm f/1.8 II".to_string(),
            30 => "Canon EF 35-105mm f/4.5-5.6".to_string(),
            31 => "Canon EF 75-300mm f/4-5.6 or Tamron Lens".to_string(),
            32 => "Canon EF 24mm f/2.8 or Sigma Lens".to_string(),
            33 => "Voigtlander or Carl Zeiss Lens".to_string(),
            35 => "Canon EF 35-80mm f/4-5.6".to_string(),
            36 => "Canon EF 38-76mm f/4.5-5.6".to_string(),
            37 => "Canon EF 35-80mm f/4-5.6 or Tamron Lens".to_string(),
            38 => "Canon EF 80-200mm f/4.5-5.6 II".to_string(),
            39 => "Canon EF 75-300mm f/4-5.6".to_string(),
            40 => "Canon EF 28-80mm f/3.5-5.6".to_string(),
            41 => "Canon EF 28-90mm f/4-5.6".to_string(),
            42 => "Canon EF 28-200mm f/3.5-5.6 or Tamron Lens".to_string(),
            43 => "Canon EF 28-105mm f/4-5.6".to_string(),
            44 => "Canon EF 90-300mm f/4.5-5.6".to_string(),
            45 => "Canon EF-S 18-55mm f/3.5-5.6 [II]".to_string(),
            46 => "Canon EF 28-90mm f/4-5.6".to_string(),
            47 => "Zeiss Milvus 35mm f/2 or 50mm f/2".to_string(),
            48 => "Canon EF-S 18-55mm f/3.5-5.6 IS".to_string(),
            49 => "Canon EF-S 55-250mm f/4-5.6 IS".to_string(),
            50 => "Canon EF-S 18-200mm f/3.5-5.6 IS".to_string(),
            51 => "Canon EF-S 18-135mm f/3.5-5.6 IS".to_string(),
            52 => "Canon EF-S 18-55mm f/3.5-5.6 IS II".to_string(),
            53 => "Canon EF-S 18-55mm f/3.5-5.6 III".to_string(),
            54 => "Canon EF-S 55-250mm f/4-5.6 IS II".to_string(),
            60 => "Irix 11mm f/4 or 15mm f/2.4".to_string(),
            63 => "Irix 30mm F1.4 Dragonfly".to_string(),
            80 => "Canon TS-E 50mm f/2.8L Macro".to_string(),
            81 => "Canon TS-E 90mm f/2.8L Macro".to_string(),
            82 => "Canon TS-E 135mm f/4L Macro".to_string(),
            94 => "Canon TS-E 17mm f/4L".to_string(),
            95 => "Canon TS-E 24mm f/3.5L II".to_string(),
            103 => "Samyang AF 14mm f/2.8 EF or Rokinon Lens".to_string(),
            106 => "Rokinon SP / Samyang XP 35mm f/1.2".to_string(),
            112 => "Sigma 28mm f/1.5 FF High-speed Prime or other Sigma Lens".to_string(),
            117 => "Tamron 35-150mm f/2.8-4.0 Di VC OSD (A043) or other Tamron Lens".to_string(),
            124 => "Canon MP-E 65mm f/2.8 1-5x Macro Photo".to_string(),
            125 => "Canon TS-E 24mm f/3.5L".to_string(),
            126 => "Canon TS-E 45mm f/2.8".to_string(),
            127 => "Canon TS-E 90mm f/2.8 or Tamron Lens".to_string(),
            129 => "Canon EF 300mm f/2.8L USM".to_string(),
            130 => "Canon EF 50mm f/1.0L USM".to_string(),
            131 => "Canon EF 28-80mm f/2.8-4L USM or Sigma Lens".to_string(),
            132 => "Canon EF 1200mm f/5.6L USM".to_string(),
            134 => "Canon EF 600mm f/4L IS USM".to_string(),
            135 => "Canon EF 200mm f/1.8L USM".to_string(),
            136 => "Canon EF 300mm f/2.8L USM".to_string(),
            137 => "Canon EF 85mm f/1.2L USM or Sigma or Tamron Lens".to_string(),
            138 => "Canon EF 28-80mm f/2.8-4L".to_string(),
            139 => "Canon EF 400mm f/2.8L USM".to_string(),
            140 => "Canon EF 500mm f/4.5L USM".to_string(),
            141 => "Canon EF 500mm f/4.5L USM".to_string(),
            142 => "Canon EF 300mm f/2.8L IS USM".to_string(),
            143 => "Canon EF 500mm f/4L IS USM or Sigma Lens".to_string(),
            144 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            145 => "Canon EF 100-300mm f/4.5-5.6 USM".to_string(),
            146 => "Canon EF 70-210mm f/3.5-4.5 USM".to_string(),
            147 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            148 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            149 => "Canon EF 100mm f/2 USM".to_string(),
            150 => "Canon EF 14mm f/2.8L USM or Sigma Lens".to_string(),
            151 => "Canon EF 200mm f/2.8L USM".to_string(),
            152 => "Canon EF 300mm f/4L IS USM or Sigma Lens".to_string(),
            153 => "Canon EF 35-350mm f/3.5-5.6L USM or Sigma or Tamron Lens".to_string(),
            154 => "Canon EF 20mm f/2.8 USM or Zeiss Lens".to_string(),
            155 => "Canon EF 85mm f/1.8 USM or Sigma Lens".to_string(),
            156 => "Canon EF 28-105mm f/3.5-4.5 USM or Tamron Lens".to_string(),
            160 => "Canon EF 20-35mm f/3.5-4.5 USM or Tamron or Tokina Lens".to_string(),
            161 => "Canon EF 28-70mm f/2.8L USM or Other Lens".to_string(),
            162 => "Canon EF 200mm f/2.8L USM".to_string(),
            163 => "Canon EF 300mm f/4L".to_string(),
            164 => "Canon EF 400mm f/5.6L".to_string(),
            165 => "Canon EF 70-200mm f/2.8L USM".to_string(),
            166 => "Canon EF 70-200mm f/2.8L USM + 1.4x".to_string(),
            167 => "Canon EF 70-200mm f/2.8L USM + 2x".to_string(),
            168 => "Canon EF 28mm f/1.8 USM or Sigma Lens".to_string(),
            169 => "Canon EF 17-35mm f/2.8L USM or Sigma Lens".to_string(),
            170 => "Canon EF 200mm f/2.8L II USM or Sigma Lens".to_string(),
            171 => "Canon EF 300mm f/4L USM".to_string(),
            172 => "Canon EF 400mm f/5.6L USM or Sigma Lens".to_string(),
            173 => "Canon EF 180mm Macro f/3.5L USM or Sigma Lens".to_string(),
            174 => "Canon EF 135mm f/2L USM or Other Lens".to_string(),
            175 => "Canon EF 400mm f/2.8L USM".to_string(),
            176 => "Canon EF 24-85mm f/3.5-4.5 USM".to_string(),
            177 => "Canon EF 300mm f/4L IS USM".to_string(),
            178 => "Canon EF 28-135mm f/3.5-5.6 IS".to_string(),
            179 => "Canon EF 24mm f/1.4L USM".to_string(),
            180 => "Canon EF 35mm f/1.4L USM or Other Lens".to_string(),
            181 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 1.4x or Sigma Lens".to_string(),
            182 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 2x or Sigma Lens".to_string(),
            183 => "Canon EF 100-400mm f/4.5-5.6L IS USM or Sigma Lens".to_string(),
            184 => "Canon EF 400mm f/2.8L USM + 2x".to_string(),
            185 => "Canon EF 600mm f/4L IS USM".to_string(),
            186 => "Canon EF 70-200mm f/4L USM".to_string(),
            187 => "Canon EF 70-200mm f/4L USM + 1.4x".to_string(),
            188 => "Canon EF 70-200mm f/4L USM + 2x".to_string(),
            189 => "Canon EF 70-200mm f/4L USM + 2.8x".to_string(),
            190 => "Canon EF 100mm f/2.8 Macro USM".to_string(),
            191 => "Canon EF 400mm f/4 DO IS or Sigma Lens".to_string(),
            193 => "Canon EF 35-80mm f/4-5.6 USM".to_string(),
            194 => "Canon EF 80-200mm f/4.5-5.6 USM".to_string(),
            195 => "Canon EF 35-105mm f/4.5-5.6 USM".to_string(),
            196 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            197 => "Canon EF 75-300mm f/4-5.6 IS USM or Sigma Lens".to_string(),
            198 => "Canon EF 50mm f/1.4 USM or Other Lens".to_string(),
            199 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            200 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            201 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            202 => "Canon EF 28-80mm f/3.5-5.6 USM IV".to_string(),
            208 => "Canon EF 22-55mm f/4-5.6 USM".to_string(),
            209 => "Canon EF 55-200mm f/4.5-5.6".to_string(),
            210 => "Canon EF 28-90mm f/4-5.6 USM".to_string(),
            211 => "Canon EF 28-200mm f/3.5-5.6 USM".to_string(),
            212 => "Canon EF 28-105mm f/4-5.6 USM".to_string(),
            213 => "Canon EF 90-300mm f/4.5-5.6 USM or Tamron Lens".to_string(),
            214 => "Canon EF-S 18-55mm f/3.5-5.6 USM".to_string(),
            215 => "Canon EF 55-200mm f/4.5-5.6 II USM".to_string(),
            217 => "Tamron AF 18-270mm f/3.5-6.3 Di II VC PZD".to_string(),
            220 => "Yongnuo YN 50mm f/1.8".to_string(),
            224 => "Canon EF 70-200mm f/2.8L IS USM".to_string(),
            225 => "Canon EF 70-200mm f/2.8L IS USM + 1.4x".to_string(),
            226 => "Canon EF 70-200mm f/2.8L IS USM + 2x".to_string(),
            227 => "Canon EF 70-200mm f/2.8L IS USM + 2.8x".to_string(),
            228 => "Canon EF 28-105mm f/3.5-4.5 USM".to_string(),
            229 => "Canon EF 16-35mm f/2.8L USM".to_string(),
            230 => "Canon EF 24-70mm f/2.8L USM".to_string(),
            231 => "Canon EF 17-40mm f/4L USM or Sigma Lens".to_string(),
            232 => "Canon EF 70-300mm f/4.5-5.6 DO IS USM".to_string(),
            233 => "Canon EF 28-300mm f/3.5-5.6L IS USM".to_string(),
            234 => "Canon EF-S 17-85mm f/4-5.6 IS USM or Tokina Lens".to_string(),
            235 => "Canon EF-S 10-22mm f/3.5-4.5 USM".to_string(),
            236 => "Canon EF-S 60mm f/2.8 Macro USM".to_string(),
            237 => "Canon EF 24-105mm f/4L IS USM".to_string(),
            238 => "Canon EF 70-300mm f/4-5.6 IS USM".to_string(),
            239 => "Canon EF 85mm f/1.2L II USM or Rokinon Lens".to_string(),
            240 => "Canon EF-S 17-55mm f/2.8 IS USM or Sigma Lens".to_string(),
            241 => "Canon EF 50mm f/1.2L USM".to_string(),
            242 => "Canon EF 70-200mm f/4L IS USM".to_string(),
            243 => "Canon EF 70-200mm f/4L IS USM + 1.4x".to_string(),
            244 => "Canon EF 70-200mm f/4L IS USM + 2x".to_string(),
            245 => "Canon EF 70-200mm f/4L IS USM + 2.8x".to_string(),
            246 => "Canon EF 16-35mm f/2.8L II USM".to_string(),
            247 => "Canon EF 14mm f/2.8L II USM".to_string(),
            248 => "Canon EF 200mm f/2L IS USM or Sigma Lens".to_string(),
            249 => "Canon EF 800mm f/5.6L IS USM".to_string(),
            250 => "Canon EF 24mm f/1.4L II USM or Sigma Lens".to_string(),
            251 => "Canon EF 70-200mm f/2.8L IS II USM".to_string(),
            252 => "Canon EF 70-200mm f/2.8L IS II USM + 1.4x".to_string(),
            253 => "Canon EF 70-200mm f/2.8L IS II USM + 2x".to_string(),
            254 => "Canon EF 100mm f/2.8L Macro IS USM or Tamron Lens".to_string(),
            255 => "Sigma 24-105mm f/4 DG OS HSM | A or Other Lens".to_string(),
            368 => "Sigma 14-24mm f/2.8 DG HSM | A or other Sigma Lens".to_string(),
            488 => "Canon EF-S 15-85mm f/3.5-5.6 IS USM".to_string(),
            489 => "Canon EF 70-300mm f/4-5.6L IS USM".to_string(),
            490 => "Canon EF 8-15mm f/4L Fisheye USM".to_string(),
            491 => "Canon EF 300mm f/2.8L IS II USM or Tamron Lens".to_string(),
            492 => "Canon EF 400mm f/2.8L IS II USM".to_string(),
            493 => "Canon EF 500mm f/4L IS II USM or EF 24-105mm f4L IS USM".to_string(),
            494 => "Canon EF 600mm f/4L IS II USM".to_string(),
            495 => "Canon EF 24-70mm f/2.8L II USM or Sigma Lens".to_string(),
            496 => "Canon EF 200-400mm f/4L IS USM".to_string(),
            499 => "Canon EF 200-400mm f/4L IS USM + 1.4x".to_string(),
            502 => "Canon EF 28mm f/2.8 IS USM or Tamron Lens".to_string(),
            503 => "Canon EF 24mm f/2.8 IS USM".to_string(),
            504 => "Canon EF 24-70mm f/4L IS USM".to_string(),
            505 => "Canon EF 35mm f/2 IS USM".to_string(),
            506 => "Canon EF 400mm f/4 DO IS II USM".to_string(),
            507 => "Canon EF 16-35mm f/4L IS USM".to_string(),
            508 => "Canon EF 11-24mm f/4L USM or Tamron Lens".to_string(),
            624 => "Sigma 70-200mm f/2.8 DG OS HSM | S or other Sigma Lens".to_string(),
            747 => "Canon EF 100-400mm f/4.5-5.6L IS II USM or Tamron Lens".to_string(),
            748 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 1.4x or Tamron Lens".to_string(),
            749 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 2x or Tamron Lens".to_string(),
            750 => "Canon EF 35mm f/1.4L II USM or Tamron Lens".to_string(),
            751 => "Canon EF 16-35mm f/2.8L III USM".to_string(),
            752 => "Canon EF 24-105mm f/4L IS II USM".to_string(),
            753 => "Canon EF 85mm f/1.4L IS USM".to_string(),
            754 => "Canon EF 70-200mm f/4L IS II USM".to_string(),
            757 => "Canon EF 400mm f/2.8L IS III USM".to_string(),
            758 => "Canon EF 600mm f/4L IS III USM".to_string(),
            923 => "Meike/SKY 85mm f/1.8 DCM".to_string(),
            1136 => "Sigma 24-70mm f/2.8 DG OS HSM | A".to_string(),
            4142 => "Canon EF-S 18-135mm f/3.5-5.6 IS STM".to_string(),
            4143 => "Canon EF-M 18-55mm f/3.5-5.6 IS STM or Tamron Lens".to_string(),
            4144 => "Canon EF 40mm f/2.8 STM".to_string(),
            4145 => "Canon EF-M 22mm f/2 STM".to_string(),
            4146 => "Canon EF-S 18-55mm f/3.5-5.6 IS STM".to_string(),
            4147 => "Canon EF-M 11-22mm f/4-5.6 IS STM".to_string(),
            4148 => "Canon EF-S 55-250mm f/4-5.6 IS STM".to_string(),
            4149 => "Canon EF-M 55-200mm f/4.5-6.3 IS STM".to_string(),
            4150 => "Canon EF-S 10-18mm f/4.5-5.6 IS STM".to_string(),
            4152 => "Canon EF 24-105mm f/3.5-5.6 IS STM".to_string(),
            4153 => "Canon EF-M 15-45mm f/3.5-6.3 IS STM".to_string(),
            4154 => "Canon EF-S 24mm f/2.8 STM".to_string(),
            4155 => "Canon EF-M 28mm f/3.5 Macro IS STM".to_string(),
            4156 => "Canon EF 50mm f/1.8 STM".to_string(),
            4157 => "Canon EF-M 18-150mm f/3.5-6.3 IS STM".to_string(),
            4158 => "Canon EF-S 18-55mm f/4-5.6 IS STM".to_string(),
            4159 => "Canon EF-M 32mm f/1.4 STM".to_string(),
            4160 => "Canon EF-S 35mm f/2.8 Macro IS STM".to_string(),
            4208 => "Sigma 56mm f/1.4 DC DN | C or other Sigma Lens".to_string(),
            4976 => "Sigma 16-300mm F3.5-6.7 DC OS | C (025)".to_string(),
            6512 => "Sigma 12mm F1.4 DC | C".to_string(),
            36910 => "Canon EF 70-300mm f/4-5.6 IS II USM".to_string(),
            36912 => "Canon EF-S 18-135mm f/3.5-5.6 IS USM".to_string(),
            61182 => "Canon RF 50mm F1.2L USM or other Canon RF Lens".to_string(),
            61491 => "Canon CN-E 14mm T3.1 L F".to_string(),
            61492 => "Canon CN-E 24mm T1.5 L F".to_string(),
            61494 => "Canon CN-E 85mm T1.3 L F".to_string(),
            61495 => "Canon CN-E 135mm T2.2 L F".to_string(),
            61496 => "Canon CN-E 35mm T1.5 L F".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("LensType", 0x112, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x114, bo) {
        dm.push(("MinFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MinFocalLength", 0x114, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x116, bo) {
        dm.push(("MaxFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MaxFocalLength", 0x116, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(text) = text_at(data, 0x1ac, 6, true) {
        tags.push(mk("FirmwareVersion", 0x1ac, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x1eb, bo) {
        dm.push(("FileIndex".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val + 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("FileIndex", 0x1eb, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x1f7, bo) {
        dm.push(("DirectoryIndex".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("DirectoryIndex", 0x1f7, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(sub) = data.get(0x327..) {
        tags.extend(canon_psinfo(sub, model, bo, file_type, format, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::CameraInfo40D` -- FORMAT int8u, FIRST_ENTRY 0.
fn canon_camerainfo40d(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = u8_at(data, 0x3) {
        dm.push(("FNumber".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(($val-8)/16*log(2))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("sprintf(\"%.2g\",$val)", &cv, &ctx) { cv = x; }
            tags.push(mk("FNumber", 0x3, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x4) {
        dm.push(("ExposureTime".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(4*log(2)*(1-Image::ExifTool::Canon::CanonEv($val-24)))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("Image::ExifTool::Exif::PrintExposureTime($val)", &cv, &ctx) { cv = x; }
            tags.push(mk("ExposureTime", 0x4, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x6) {
        dm.push(("ISO".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("100*exp(($val/8-9)*log(2))", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("sprintf(\"%.0f\",$val)", &cv, &ctx) { cv = x; }
        tags.push(mk("ISO", 0x6, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x15) {
        dm.push(("FlashMeteringMode".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "E-TTL".to_string(),
            3 => "TTL".to_string(),
            4 => "External Auto".to_string(),
            5 => "External Manual".to_string(),
            6 => "Off".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("FlashMeteringMode", 0x15, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x18) {
        dm.push(("CameraTemperature".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 128", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
        tags.push(mk("CameraTemperature", 0x18, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if (dm_get(dm, "LensType").is_some_and(|v| v != 0.0) && dm_get(dm, "LensType").is_some_and(|v| v == 124.0)) {
        if let Some(v) = u8_at(data, 0x1b) {
            dm.push(("MacroMagnification".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp((75-$val) * log(2) * 3 / 40)", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("sprintf(\"%.1fx\",$val)", &cv, &ctx) { cv = x; }
            tags.push(mk("MacroMagnification", 0x1b, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u16rev_at(data, 0x1d, bo) {
        dm.push(("FocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
            tags.push(mk("FocalLength", 0x1d, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x30) {
        dm.push(("CameraOrientation".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Horizontal (normal)".to_string(),
            1 => "Rotate 90 CW".to_string(),
            2 => "Rotate 270 CW".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("CameraOrientation", 0x30, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x43, bo) {
        dm.push(("FocusDistanceUpper".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceUpper", 0x43, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x45, bo) {
        dm.push(("FocusDistanceLower".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceLower", 0x45, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x6f, bo) {
        dm.push(("WhiteBalance".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Auto".to_string(),
            1 => "Daylight".to_string(),
            2 => "Cloudy".to_string(),
            3 => "Tungsten".to_string(),
            4 => "Fluorescent".to_string(),
            5 => "Flash".to_string(),
            6 => "Custom".to_string(),
            7 => "Black & White".to_string(),
            8 => "Shade".to_string(),
            9 => "Manual Temperature (Kelvin)".to_string(),
            10 => "PC Set1".to_string(),
            11 => "PC Set2".to_string(),
            12 => "PC Set3".to_string(),
            14 => "Daylight Fluorescent".to_string(),
            15 => "Custom 1".to_string(),
            16 => "Custom 2".to_string(),
            17 => "Underwater".to_string(),
            18 => "Custom 3".to_string(),
            19 => "Custom 4".to_string(),
            20 => "PC Set4".to_string(),
            21 => "PC Set5".to_string(),
            23 => "Auto (ambience priority)".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("WhiteBalance", 0x6f, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x73, bo) {
        dm.push(("ColorTemperature".to_string(), f64::from(v)));
        tags.push(mk("ColorTemperature", 0x73, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0xd6, bo) {
        dm.push(("LensType".to_string(), f64::from(v)));
        let s = match v as i64 {
            -1 => "n/a".to_string(),
            1 => "Canon EF 50mm f/1.8".to_string(),
            2 => "Canon EF 28mm f/2.8 or Sigma Lens".to_string(),
            3 => "Canon EF 135mm f/2.8 Soft".to_string(),
            4 => "Canon EF 35-105mm f/3.5-4.5 or Sigma Lens".to_string(),
            5 => "Canon EF 35-70mm f/3.5-4.5".to_string(),
            6 => "Canon EF 28-70mm f/3.5-4.5 or Sigma or Tokina Lens".to_string(),
            7 => "Canon EF 100-300mm f/5.6L".to_string(),
            8 => "Canon EF 100-300mm f/5.6 or Sigma or Tokina Lens".to_string(),
            9 => "Canon EF 70-210mm f/4".to_string(),
            10 => "Canon EF 50mm f/2.5 Macro or Sigma Lens".to_string(),
            11 => "Canon EF 35mm f/2".to_string(),
            13 => "Canon EF 15mm f/2.8 Fisheye".to_string(),
            14 => "Canon EF 50-200mm f/3.5-4.5L".to_string(),
            15 => "Canon EF 50-200mm f/3.5-4.5".to_string(),
            16 => "Canon EF 35-135mm f/3.5-4.5".to_string(),
            17 => "Canon EF 35-70mm f/3.5-4.5A".to_string(),
            18 => "Canon EF 28-70mm f/3.5-4.5".to_string(),
            20 => "Canon EF 100-200mm f/4.5A".to_string(),
            21 => "Canon EF 80-200mm f/2.8L".to_string(),
            22 => "Canon EF 20-35mm f/2.8L or Tokina Lens".to_string(),
            23 => "Canon EF 35-105mm f/3.5-4.5".to_string(),
            24 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            25 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            26 => "Canon EF 100mm f/2.8 Macro or Other Lens".to_string(),
            27 => "Canon EF 35-80mm f/4-5.6".to_string(),
            28 => "Canon EF 80-200mm f/4.5-5.6 or Tamron Lens".to_string(),
            29 => "Canon EF 50mm f/1.8 II".to_string(),
            30 => "Canon EF 35-105mm f/4.5-5.6".to_string(),
            31 => "Canon EF 75-300mm f/4-5.6 or Tamron Lens".to_string(),
            32 => "Canon EF 24mm f/2.8 or Sigma Lens".to_string(),
            33 => "Voigtlander or Carl Zeiss Lens".to_string(),
            35 => "Canon EF 35-80mm f/4-5.6".to_string(),
            36 => "Canon EF 38-76mm f/4.5-5.6".to_string(),
            37 => "Canon EF 35-80mm f/4-5.6 or Tamron Lens".to_string(),
            38 => "Canon EF 80-200mm f/4.5-5.6 II".to_string(),
            39 => "Canon EF 75-300mm f/4-5.6".to_string(),
            40 => "Canon EF 28-80mm f/3.5-5.6".to_string(),
            41 => "Canon EF 28-90mm f/4-5.6".to_string(),
            42 => "Canon EF 28-200mm f/3.5-5.6 or Tamron Lens".to_string(),
            43 => "Canon EF 28-105mm f/4-5.6".to_string(),
            44 => "Canon EF 90-300mm f/4.5-5.6".to_string(),
            45 => "Canon EF-S 18-55mm f/3.5-5.6 [II]".to_string(),
            46 => "Canon EF 28-90mm f/4-5.6".to_string(),
            47 => "Zeiss Milvus 35mm f/2 or 50mm f/2".to_string(),
            48 => "Canon EF-S 18-55mm f/3.5-5.6 IS".to_string(),
            49 => "Canon EF-S 55-250mm f/4-5.6 IS".to_string(),
            50 => "Canon EF-S 18-200mm f/3.5-5.6 IS".to_string(),
            51 => "Canon EF-S 18-135mm f/3.5-5.6 IS".to_string(),
            52 => "Canon EF-S 18-55mm f/3.5-5.6 IS II".to_string(),
            53 => "Canon EF-S 18-55mm f/3.5-5.6 III".to_string(),
            54 => "Canon EF-S 55-250mm f/4-5.6 IS II".to_string(),
            60 => "Irix 11mm f/4 or 15mm f/2.4".to_string(),
            63 => "Irix 30mm F1.4 Dragonfly".to_string(),
            80 => "Canon TS-E 50mm f/2.8L Macro".to_string(),
            81 => "Canon TS-E 90mm f/2.8L Macro".to_string(),
            82 => "Canon TS-E 135mm f/4L Macro".to_string(),
            94 => "Canon TS-E 17mm f/4L".to_string(),
            95 => "Canon TS-E 24mm f/3.5L II".to_string(),
            103 => "Samyang AF 14mm f/2.8 EF or Rokinon Lens".to_string(),
            106 => "Rokinon SP / Samyang XP 35mm f/1.2".to_string(),
            112 => "Sigma 28mm f/1.5 FF High-speed Prime or other Sigma Lens".to_string(),
            117 => "Tamron 35-150mm f/2.8-4.0 Di VC OSD (A043) or other Tamron Lens".to_string(),
            124 => "Canon MP-E 65mm f/2.8 1-5x Macro Photo".to_string(),
            125 => "Canon TS-E 24mm f/3.5L".to_string(),
            126 => "Canon TS-E 45mm f/2.8".to_string(),
            127 => "Canon TS-E 90mm f/2.8 or Tamron Lens".to_string(),
            129 => "Canon EF 300mm f/2.8L USM".to_string(),
            130 => "Canon EF 50mm f/1.0L USM".to_string(),
            131 => "Canon EF 28-80mm f/2.8-4L USM or Sigma Lens".to_string(),
            132 => "Canon EF 1200mm f/5.6L USM".to_string(),
            134 => "Canon EF 600mm f/4L IS USM".to_string(),
            135 => "Canon EF 200mm f/1.8L USM".to_string(),
            136 => "Canon EF 300mm f/2.8L USM".to_string(),
            137 => "Canon EF 85mm f/1.2L USM or Sigma or Tamron Lens".to_string(),
            138 => "Canon EF 28-80mm f/2.8-4L".to_string(),
            139 => "Canon EF 400mm f/2.8L USM".to_string(),
            140 => "Canon EF 500mm f/4.5L USM".to_string(),
            141 => "Canon EF 500mm f/4.5L USM".to_string(),
            142 => "Canon EF 300mm f/2.8L IS USM".to_string(),
            143 => "Canon EF 500mm f/4L IS USM or Sigma Lens".to_string(),
            144 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            145 => "Canon EF 100-300mm f/4.5-5.6 USM".to_string(),
            146 => "Canon EF 70-210mm f/3.5-4.5 USM".to_string(),
            147 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            148 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            149 => "Canon EF 100mm f/2 USM".to_string(),
            150 => "Canon EF 14mm f/2.8L USM or Sigma Lens".to_string(),
            151 => "Canon EF 200mm f/2.8L USM".to_string(),
            152 => "Canon EF 300mm f/4L IS USM or Sigma Lens".to_string(),
            153 => "Canon EF 35-350mm f/3.5-5.6L USM or Sigma or Tamron Lens".to_string(),
            154 => "Canon EF 20mm f/2.8 USM or Zeiss Lens".to_string(),
            155 => "Canon EF 85mm f/1.8 USM or Sigma Lens".to_string(),
            156 => "Canon EF 28-105mm f/3.5-4.5 USM or Tamron Lens".to_string(),
            160 => "Canon EF 20-35mm f/3.5-4.5 USM or Tamron or Tokina Lens".to_string(),
            161 => "Canon EF 28-70mm f/2.8L USM or Other Lens".to_string(),
            162 => "Canon EF 200mm f/2.8L USM".to_string(),
            163 => "Canon EF 300mm f/4L".to_string(),
            164 => "Canon EF 400mm f/5.6L".to_string(),
            165 => "Canon EF 70-200mm f/2.8L USM".to_string(),
            166 => "Canon EF 70-200mm f/2.8L USM + 1.4x".to_string(),
            167 => "Canon EF 70-200mm f/2.8L USM + 2x".to_string(),
            168 => "Canon EF 28mm f/1.8 USM or Sigma Lens".to_string(),
            169 => "Canon EF 17-35mm f/2.8L USM or Sigma Lens".to_string(),
            170 => "Canon EF 200mm f/2.8L II USM or Sigma Lens".to_string(),
            171 => "Canon EF 300mm f/4L USM".to_string(),
            172 => "Canon EF 400mm f/5.6L USM or Sigma Lens".to_string(),
            173 => "Canon EF 180mm Macro f/3.5L USM or Sigma Lens".to_string(),
            174 => "Canon EF 135mm f/2L USM or Other Lens".to_string(),
            175 => "Canon EF 400mm f/2.8L USM".to_string(),
            176 => "Canon EF 24-85mm f/3.5-4.5 USM".to_string(),
            177 => "Canon EF 300mm f/4L IS USM".to_string(),
            178 => "Canon EF 28-135mm f/3.5-5.6 IS".to_string(),
            179 => "Canon EF 24mm f/1.4L USM".to_string(),
            180 => "Canon EF 35mm f/1.4L USM or Other Lens".to_string(),
            181 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 1.4x or Sigma Lens".to_string(),
            182 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 2x or Sigma Lens".to_string(),
            183 => "Canon EF 100-400mm f/4.5-5.6L IS USM or Sigma Lens".to_string(),
            184 => "Canon EF 400mm f/2.8L USM + 2x".to_string(),
            185 => "Canon EF 600mm f/4L IS USM".to_string(),
            186 => "Canon EF 70-200mm f/4L USM".to_string(),
            187 => "Canon EF 70-200mm f/4L USM + 1.4x".to_string(),
            188 => "Canon EF 70-200mm f/4L USM + 2x".to_string(),
            189 => "Canon EF 70-200mm f/4L USM + 2.8x".to_string(),
            190 => "Canon EF 100mm f/2.8 Macro USM".to_string(),
            191 => "Canon EF 400mm f/4 DO IS or Sigma Lens".to_string(),
            193 => "Canon EF 35-80mm f/4-5.6 USM".to_string(),
            194 => "Canon EF 80-200mm f/4.5-5.6 USM".to_string(),
            195 => "Canon EF 35-105mm f/4.5-5.6 USM".to_string(),
            196 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            197 => "Canon EF 75-300mm f/4-5.6 IS USM or Sigma Lens".to_string(),
            198 => "Canon EF 50mm f/1.4 USM or Other Lens".to_string(),
            199 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            200 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            201 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            202 => "Canon EF 28-80mm f/3.5-5.6 USM IV".to_string(),
            208 => "Canon EF 22-55mm f/4-5.6 USM".to_string(),
            209 => "Canon EF 55-200mm f/4.5-5.6".to_string(),
            210 => "Canon EF 28-90mm f/4-5.6 USM".to_string(),
            211 => "Canon EF 28-200mm f/3.5-5.6 USM".to_string(),
            212 => "Canon EF 28-105mm f/4-5.6 USM".to_string(),
            213 => "Canon EF 90-300mm f/4.5-5.6 USM or Tamron Lens".to_string(),
            214 => "Canon EF-S 18-55mm f/3.5-5.6 USM".to_string(),
            215 => "Canon EF 55-200mm f/4.5-5.6 II USM".to_string(),
            217 => "Tamron AF 18-270mm f/3.5-6.3 Di II VC PZD".to_string(),
            220 => "Yongnuo YN 50mm f/1.8".to_string(),
            224 => "Canon EF 70-200mm f/2.8L IS USM".to_string(),
            225 => "Canon EF 70-200mm f/2.8L IS USM + 1.4x".to_string(),
            226 => "Canon EF 70-200mm f/2.8L IS USM + 2x".to_string(),
            227 => "Canon EF 70-200mm f/2.8L IS USM + 2.8x".to_string(),
            228 => "Canon EF 28-105mm f/3.5-4.5 USM".to_string(),
            229 => "Canon EF 16-35mm f/2.8L USM".to_string(),
            230 => "Canon EF 24-70mm f/2.8L USM".to_string(),
            231 => "Canon EF 17-40mm f/4L USM or Sigma Lens".to_string(),
            232 => "Canon EF 70-300mm f/4.5-5.6 DO IS USM".to_string(),
            233 => "Canon EF 28-300mm f/3.5-5.6L IS USM".to_string(),
            234 => "Canon EF-S 17-85mm f/4-5.6 IS USM or Tokina Lens".to_string(),
            235 => "Canon EF-S 10-22mm f/3.5-4.5 USM".to_string(),
            236 => "Canon EF-S 60mm f/2.8 Macro USM".to_string(),
            237 => "Canon EF 24-105mm f/4L IS USM".to_string(),
            238 => "Canon EF 70-300mm f/4-5.6 IS USM".to_string(),
            239 => "Canon EF 85mm f/1.2L II USM or Rokinon Lens".to_string(),
            240 => "Canon EF-S 17-55mm f/2.8 IS USM or Sigma Lens".to_string(),
            241 => "Canon EF 50mm f/1.2L USM".to_string(),
            242 => "Canon EF 70-200mm f/4L IS USM".to_string(),
            243 => "Canon EF 70-200mm f/4L IS USM + 1.4x".to_string(),
            244 => "Canon EF 70-200mm f/4L IS USM + 2x".to_string(),
            245 => "Canon EF 70-200mm f/4L IS USM + 2.8x".to_string(),
            246 => "Canon EF 16-35mm f/2.8L II USM".to_string(),
            247 => "Canon EF 14mm f/2.8L II USM".to_string(),
            248 => "Canon EF 200mm f/2L IS USM or Sigma Lens".to_string(),
            249 => "Canon EF 800mm f/5.6L IS USM".to_string(),
            250 => "Canon EF 24mm f/1.4L II USM or Sigma Lens".to_string(),
            251 => "Canon EF 70-200mm f/2.8L IS II USM".to_string(),
            252 => "Canon EF 70-200mm f/2.8L IS II USM + 1.4x".to_string(),
            253 => "Canon EF 70-200mm f/2.8L IS II USM + 2x".to_string(),
            254 => "Canon EF 100mm f/2.8L Macro IS USM or Tamron Lens".to_string(),
            255 => "Sigma 24-105mm f/4 DG OS HSM | A or Other Lens".to_string(),
            368 => "Sigma 14-24mm f/2.8 DG HSM | A or other Sigma Lens".to_string(),
            488 => "Canon EF-S 15-85mm f/3.5-5.6 IS USM".to_string(),
            489 => "Canon EF 70-300mm f/4-5.6L IS USM".to_string(),
            490 => "Canon EF 8-15mm f/4L Fisheye USM".to_string(),
            491 => "Canon EF 300mm f/2.8L IS II USM or Tamron Lens".to_string(),
            492 => "Canon EF 400mm f/2.8L IS II USM".to_string(),
            493 => "Canon EF 500mm f/4L IS II USM or EF 24-105mm f4L IS USM".to_string(),
            494 => "Canon EF 600mm f/4L IS II USM".to_string(),
            495 => "Canon EF 24-70mm f/2.8L II USM or Sigma Lens".to_string(),
            496 => "Canon EF 200-400mm f/4L IS USM".to_string(),
            499 => "Canon EF 200-400mm f/4L IS USM + 1.4x".to_string(),
            502 => "Canon EF 28mm f/2.8 IS USM or Tamron Lens".to_string(),
            503 => "Canon EF 24mm f/2.8 IS USM".to_string(),
            504 => "Canon EF 24-70mm f/4L IS USM".to_string(),
            505 => "Canon EF 35mm f/2 IS USM".to_string(),
            506 => "Canon EF 400mm f/4 DO IS II USM".to_string(),
            507 => "Canon EF 16-35mm f/4L IS USM".to_string(),
            508 => "Canon EF 11-24mm f/4L USM or Tamron Lens".to_string(),
            624 => "Sigma 70-200mm f/2.8 DG OS HSM | S or other Sigma Lens".to_string(),
            747 => "Canon EF 100-400mm f/4.5-5.6L IS II USM or Tamron Lens".to_string(),
            748 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 1.4x or Tamron Lens".to_string(),
            749 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 2x or Tamron Lens".to_string(),
            750 => "Canon EF 35mm f/1.4L II USM or Tamron Lens".to_string(),
            751 => "Canon EF 16-35mm f/2.8L III USM".to_string(),
            752 => "Canon EF 24-105mm f/4L IS II USM".to_string(),
            753 => "Canon EF 85mm f/1.4L IS USM".to_string(),
            754 => "Canon EF 70-200mm f/4L IS II USM".to_string(),
            757 => "Canon EF 400mm f/2.8L IS III USM".to_string(),
            758 => "Canon EF 600mm f/4L IS III USM".to_string(),
            923 => "Meike/SKY 85mm f/1.8 DCM".to_string(),
            1136 => "Sigma 24-70mm f/2.8 DG OS HSM | A".to_string(),
            4142 => "Canon EF-S 18-135mm f/3.5-5.6 IS STM".to_string(),
            4143 => "Canon EF-M 18-55mm f/3.5-5.6 IS STM or Tamron Lens".to_string(),
            4144 => "Canon EF 40mm f/2.8 STM".to_string(),
            4145 => "Canon EF-M 22mm f/2 STM".to_string(),
            4146 => "Canon EF-S 18-55mm f/3.5-5.6 IS STM".to_string(),
            4147 => "Canon EF-M 11-22mm f/4-5.6 IS STM".to_string(),
            4148 => "Canon EF-S 55-250mm f/4-5.6 IS STM".to_string(),
            4149 => "Canon EF-M 55-200mm f/4.5-6.3 IS STM".to_string(),
            4150 => "Canon EF-S 10-18mm f/4.5-5.6 IS STM".to_string(),
            4152 => "Canon EF 24-105mm f/3.5-5.6 IS STM".to_string(),
            4153 => "Canon EF-M 15-45mm f/3.5-6.3 IS STM".to_string(),
            4154 => "Canon EF-S 24mm f/2.8 STM".to_string(),
            4155 => "Canon EF-M 28mm f/3.5 Macro IS STM".to_string(),
            4156 => "Canon EF 50mm f/1.8 STM".to_string(),
            4157 => "Canon EF-M 18-150mm f/3.5-6.3 IS STM".to_string(),
            4158 => "Canon EF-S 18-55mm f/4-5.6 IS STM".to_string(),
            4159 => "Canon EF-M 32mm f/1.4 STM".to_string(),
            4160 => "Canon EF-S 35mm f/2.8 Macro IS STM".to_string(),
            4208 => "Sigma 56mm f/1.4 DC DN | C or other Sigma Lens".to_string(),
            4976 => "Sigma 16-300mm F3.5-6.7 DC OS | C (025)".to_string(),
            6512 => "Sigma 12mm F1.4 DC | C".to_string(),
            36910 => "Canon EF 70-300mm f/4-5.6 IS II USM".to_string(),
            36912 => "Canon EF-S 18-135mm f/3.5-5.6 IS USM".to_string(),
            61182 => "Canon RF 50mm F1.2L USM or other Canon RF Lens".to_string(),
            61491 => "Canon CN-E 14mm T3.1 L F".to_string(),
            61492 => "Canon CN-E 24mm T1.5 L F".to_string(),
            61494 => "Canon CN-E 85mm T1.3 L F".to_string(),
            61495 => "Canon CN-E 135mm T2.2 L F".to_string(),
            61496 => "Canon CN-E 35mm T1.5 L F".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("LensType", 0xd6, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0xd8, bo) {
        dm.push(("MinFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MinFocalLength", 0xd8, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0xda, bo) {
        dm.push(("MaxFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MaxFocalLength", 0xda, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(text) = text_at(data, 0xff, 6, true) {
        tags.push(mk("FirmwareVersion", 0xff, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x133, bo) {
        dm.push(("FileIndex".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val + 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("FileIndex", 0x133, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x13f, bo) {
        dm.push(("DirectoryIndex".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("DirectoryIndex", 0x13f, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(text) = text_at(data, 0x92b, 64, true) {
        tags.push(mk("LensModel", 0x92b, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    if let Some(sub) = data.get(0x25b..) {
        tags.extend(canon_psinfo(sub, model, bo, file_type, format, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::CameraInfo50D` -- FORMAT int8u, FIRST_ENTRY 0.
fn canon_camerainfo50d(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(text) = text_at(data, 0x0, 356, false) {
        tags.push(mk("FirmwareVersionLookAhead", 0x0, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x3) {
        dm.push(("FNumber".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(($val-8)/16*log(2))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("sprintf(\"%.2g\",$val)", &cv, &ctx) { cv = x; }
            tags.push(mk("FNumber", 0x3, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x4) {
        dm.push(("ExposureTime".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(4*log(2)*(1-Image::ExifTool::Canon::CanonEv($val-24)))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("Image::ExifTool::Exif::PrintExposureTime($val)", &cv, &ctx) { cv = x; }
            tags.push(mk("ExposureTime", 0x4, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x6) {
        dm.push(("ISO".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("100*exp(($val/8-9)*log(2))", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("sprintf(\"%.0f\",$val)", &cv, &ctx) { cv = x; }
        tags.push(mk("ISO", 0x6, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x7) {
        dm.push(("HighlightTonePriority".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "People".to_string(),
            1 => "sRGB".to_string(),
            2 => "Adobe RGB".to_string(),
            3 => "User 1".to_string(),
            4 => "User 2".to_string(),
            5 => "User 3".to_string(),
            6 => "To Do".to_string(),
            65535 => "n/a".to_string(),
            2415919104 => "Format 1".to_string(),
            2684354560 => "Format 2".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("HighlightTonePriority", 0x7, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x15) {
        dm.push(("FlashMeteringMode".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "E-TTL".to_string(),
            3 => "TTL".to_string(),
            4 => "External Auto".to_string(),
            5 => "External Manual".to_string(),
            6 => "Off".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("FlashMeteringMode", 0x15, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x19) {
        dm.push(("CameraTemperature".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 128", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
        tags.push(mk("CameraTemperature", 0x19, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x1e, bo) {
        dm.push(("FocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
            tags.push(mk("FocalLength", 0x1e, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x31) {
        dm.push(("CameraOrientation".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Horizontal (normal)".to_string(),
            1 => "Rotate 90 CW".to_string(),
            2 => "Rotate 270 CW".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("CameraOrientation", 0x31, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x50, bo) {
        dm.push(("FocusDistanceUpper".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceUpper", 0x50, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x52, bo) {
        dm.push(("FocusDistanceLower".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceLower", 0x52, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x6f, bo) {
        dm.push(("WhiteBalance".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Auto".to_string(),
            1 => "Daylight".to_string(),
            2 => "Cloudy".to_string(),
            3 => "Tungsten".to_string(),
            4 => "Fluorescent".to_string(),
            5 => "Flash".to_string(),
            6 => "Custom".to_string(),
            7 => "Black & White".to_string(),
            8 => "Shade".to_string(),
            9 => "Manual Temperature (Kelvin)".to_string(),
            10 => "PC Set1".to_string(),
            11 => "PC Set2".to_string(),
            12 => "PC Set3".to_string(),
            14 => "Daylight Fluorescent".to_string(),
            15 => "Custom 1".to_string(),
            16 => "Custom 2".to_string(),
            17 => "Underwater".to_string(),
            18 => "Custom 3".to_string(),
            19 => "Custom 4".to_string(),
            20 => "PC Set4".to_string(),
            21 => "PC Set5".to_string(),
            23 => "Auto (ambience priority)".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("WhiteBalance", 0x6f, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x73, bo) {
        dm.push(("ColorTemperature".to_string(), f64::from(v)));
        tags.push(mk("ColorTemperature", 0x73, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0xa7) {
        dm.push(("PictureStyle".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "None".to_string(),
            1 => "Standard".to_string(),
            2 => "Portrait".to_string(),
            3 => "High Saturation".to_string(),
            4 => "Adobe RGB".to_string(),
            5 => "Low Saturation".to_string(),
            6 => "CM Set 1".to_string(),
            7 => "CM Set 2".to_string(),
            33 => "User Def. 1".to_string(),
            34 => "User Def. 2".to_string(),
            35 => "User Def. 3".to_string(),
            65 => "PC 1".to_string(),
            66 => "PC 2".to_string(),
            67 => "PC 3".to_string(),
            129 => "Standard".to_string(),
            130 => "Portrait".to_string(),
            131 => "Landscape".to_string(),
            132 => "Neutral".to_string(),
            133 => "Faithful".to_string(),
            134 => "Monochrome".to_string(),
            135 => "Auto".to_string(),
            136 => "Fine Detail".to_string(),
            255 => "n/a".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("PictureStyle", 0xa7, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0xbd) {
        dm.push(("HighISONoiseReduction".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Standard".to_string(),
            1 => "Low".to_string(),
            2 => "Strong".to_string(),
            3 => "Off".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("HighISONoiseReduction", 0xbd, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0xbf) {
        dm.push(("AutoLightingOptimizer".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Standard".to_string(),
            1 => "Low".to_string(),
            2 => "Strong".to_string(),
            3 => "Off".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("AutoLightingOptimizer", 0xbf, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0xea, bo) {
        dm.push(("LensType".to_string(), f64::from(v)));
        let s = match v as i64 {
            -1 => "n/a".to_string(),
            1 => "Canon EF 50mm f/1.8".to_string(),
            2 => "Canon EF 28mm f/2.8 or Sigma Lens".to_string(),
            3 => "Canon EF 135mm f/2.8 Soft".to_string(),
            4 => "Canon EF 35-105mm f/3.5-4.5 or Sigma Lens".to_string(),
            5 => "Canon EF 35-70mm f/3.5-4.5".to_string(),
            6 => "Canon EF 28-70mm f/3.5-4.5 or Sigma or Tokina Lens".to_string(),
            7 => "Canon EF 100-300mm f/5.6L".to_string(),
            8 => "Canon EF 100-300mm f/5.6 or Sigma or Tokina Lens".to_string(),
            9 => "Canon EF 70-210mm f/4".to_string(),
            10 => "Canon EF 50mm f/2.5 Macro or Sigma Lens".to_string(),
            11 => "Canon EF 35mm f/2".to_string(),
            13 => "Canon EF 15mm f/2.8 Fisheye".to_string(),
            14 => "Canon EF 50-200mm f/3.5-4.5L".to_string(),
            15 => "Canon EF 50-200mm f/3.5-4.5".to_string(),
            16 => "Canon EF 35-135mm f/3.5-4.5".to_string(),
            17 => "Canon EF 35-70mm f/3.5-4.5A".to_string(),
            18 => "Canon EF 28-70mm f/3.5-4.5".to_string(),
            20 => "Canon EF 100-200mm f/4.5A".to_string(),
            21 => "Canon EF 80-200mm f/2.8L".to_string(),
            22 => "Canon EF 20-35mm f/2.8L or Tokina Lens".to_string(),
            23 => "Canon EF 35-105mm f/3.5-4.5".to_string(),
            24 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            25 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            26 => "Canon EF 100mm f/2.8 Macro or Other Lens".to_string(),
            27 => "Canon EF 35-80mm f/4-5.6".to_string(),
            28 => "Canon EF 80-200mm f/4.5-5.6 or Tamron Lens".to_string(),
            29 => "Canon EF 50mm f/1.8 II".to_string(),
            30 => "Canon EF 35-105mm f/4.5-5.6".to_string(),
            31 => "Canon EF 75-300mm f/4-5.6 or Tamron Lens".to_string(),
            32 => "Canon EF 24mm f/2.8 or Sigma Lens".to_string(),
            33 => "Voigtlander or Carl Zeiss Lens".to_string(),
            35 => "Canon EF 35-80mm f/4-5.6".to_string(),
            36 => "Canon EF 38-76mm f/4.5-5.6".to_string(),
            37 => "Canon EF 35-80mm f/4-5.6 or Tamron Lens".to_string(),
            38 => "Canon EF 80-200mm f/4.5-5.6 II".to_string(),
            39 => "Canon EF 75-300mm f/4-5.6".to_string(),
            40 => "Canon EF 28-80mm f/3.5-5.6".to_string(),
            41 => "Canon EF 28-90mm f/4-5.6".to_string(),
            42 => "Canon EF 28-200mm f/3.5-5.6 or Tamron Lens".to_string(),
            43 => "Canon EF 28-105mm f/4-5.6".to_string(),
            44 => "Canon EF 90-300mm f/4.5-5.6".to_string(),
            45 => "Canon EF-S 18-55mm f/3.5-5.6 [II]".to_string(),
            46 => "Canon EF 28-90mm f/4-5.6".to_string(),
            47 => "Zeiss Milvus 35mm f/2 or 50mm f/2".to_string(),
            48 => "Canon EF-S 18-55mm f/3.5-5.6 IS".to_string(),
            49 => "Canon EF-S 55-250mm f/4-5.6 IS".to_string(),
            50 => "Canon EF-S 18-200mm f/3.5-5.6 IS".to_string(),
            51 => "Canon EF-S 18-135mm f/3.5-5.6 IS".to_string(),
            52 => "Canon EF-S 18-55mm f/3.5-5.6 IS II".to_string(),
            53 => "Canon EF-S 18-55mm f/3.5-5.6 III".to_string(),
            54 => "Canon EF-S 55-250mm f/4-5.6 IS II".to_string(),
            60 => "Irix 11mm f/4 or 15mm f/2.4".to_string(),
            63 => "Irix 30mm F1.4 Dragonfly".to_string(),
            80 => "Canon TS-E 50mm f/2.8L Macro".to_string(),
            81 => "Canon TS-E 90mm f/2.8L Macro".to_string(),
            82 => "Canon TS-E 135mm f/4L Macro".to_string(),
            94 => "Canon TS-E 17mm f/4L".to_string(),
            95 => "Canon TS-E 24mm f/3.5L II".to_string(),
            103 => "Samyang AF 14mm f/2.8 EF or Rokinon Lens".to_string(),
            106 => "Rokinon SP / Samyang XP 35mm f/1.2".to_string(),
            112 => "Sigma 28mm f/1.5 FF High-speed Prime or other Sigma Lens".to_string(),
            117 => "Tamron 35-150mm f/2.8-4.0 Di VC OSD (A043) or other Tamron Lens".to_string(),
            124 => "Canon MP-E 65mm f/2.8 1-5x Macro Photo".to_string(),
            125 => "Canon TS-E 24mm f/3.5L".to_string(),
            126 => "Canon TS-E 45mm f/2.8".to_string(),
            127 => "Canon TS-E 90mm f/2.8 or Tamron Lens".to_string(),
            129 => "Canon EF 300mm f/2.8L USM".to_string(),
            130 => "Canon EF 50mm f/1.0L USM".to_string(),
            131 => "Canon EF 28-80mm f/2.8-4L USM or Sigma Lens".to_string(),
            132 => "Canon EF 1200mm f/5.6L USM".to_string(),
            134 => "Canon EF 600mm f/4L IS USM".to_string(),
            135 => "Canon EF 200mm f/1.8L USM".to_string(),
            136 => "Canon EF 300mm f/2.8L USM".to_string(),
            137 => "Canon EF 85mm f/1.2L USM or Sigma or Tamron Lens".to_string(),
            138 => "Canon EF 28-80mm f/2.8-4L".to_string(),
            139 => "Canon EF 400mm f/2.8L USM".to_string(),
            140 => "Canon EF 500mm f/4.5L USM".to_string(),
            141 => "Canon EF 500mm f/4.5L USM".to_string(),
            142 => "Canon EF 300mm f/2.8L IS USM".to_string(),
            143 => "Canon EF 500mm f/4L IS USM or Sigma Lens".to_string(),
            144 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            145 => "Canon EF 100-300mm f/4.5-5.6 USM".to_string(),
            146 => "Canon EF 70-210mm f/3.5-4.5 USM".to_string(),
            147 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            148 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            149 => "Canon EF 100mm f/2 USM".to_string(),
            150 => "Canon EF 14mm f/2.8L USM or Sigma Lens".to_string(),
            151 => "Canon EF 200mm f/2.8L USM".to_string(),
            152 => "Canon EF 300mm f/4L IS USM or Sigma Lens".to_string(),
            153 => "Canon EF 35-350mm f/3.5-5.6L USM or Sigma or Tamron Lens".to_string(),
            154 => "Canon EF 20mm f/2.8 USM or Zeiss Lens".to_string(),
            155 => "Canon EF 85mm f/1.8 USM or Sigma Lens".to_string(),
            156 => "Canon EF 28-105mm f/3.5-4.5 USM or Tamron Lens".to_string(),
            160 => "Canon EF 20-35mm f/3.5-4.5 USM or Tamron or Tokina Lens".to_string(),
            161 => "Canon EF 28-70mm f/2.8L USM or Other Lens".to_string(),
            162 => "Canon EF 200mm f/2.8L USM".to_string(),
            163 => "Canon EF 300mm f/4L".to_string(),
            164 => "Canon EF 400mm f/5.6L".to_string(),
            165 => "Canon EF 70-200mm f/2.8L USM".to_string(),
            166 => "Canon EF 70-200mm f/2.8L USM + 1.4x".to_string(),
            167 => "Canon EF 70-200mm f/2.8L USM + 2x".to_string(),
            168 => "Canon EF 28mm f/1.8 USM or Sigma Lens".to_string(),
            169 => "Canon EF 17-35mm f/2.8L USM or Sigma Lens".to_string(),
            170 => "Canon EF 200mm f/2.8L II USM or Sigma Lens".to_string(),
            171 => "Canon EF 300mm f/4L USM".to_string(),
            172 => "Canon EF 400mm f/5.6L USM or Sigma Lens".to_string(),
            173 => "Canon EF 180mm Macro f/3.5L USM or Sigma Lens".to_string(),
            174 => "Canon EF 135mm f/2L USM or Other Lens".to_string(),
            175 => "Canon EF 400mm f/2.8L USM".to_string(),
            176 => "Canon EF 24-85mm f/3.5-4.5 USM".to_string(),
            177 => "Canon EF 300mm f/4L IS USM".to_string(),
            178 => "Canon EF 28-135mm f/3.5-5.6 IS".to_string(),
            179 => "Canon EF 24mm f/1.4L USM".to_string(),
            180 => "Canon EF 35mm f/1.4L USM or Other Lens".to_string(),
            181 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 1.4x or Sigma Lens".to_string(),
            182 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 2x or Sigma Lens".to_string(),
            183 => "Canon EF 100-400mm f/4.5-5.6L IS USM or Sigma Lens".to_string(),
            184 => "Canon EF 400mm f/2.8L USM + 2x".to_string(),
            185 => "Canon EF 600mm f/4L IS USM".to_string(),
            186 => "Canon EF 70-200mm f/4L USM".to_string(),
            187 => "Canon EF 70-200mm f/4L USM + 1.4x".to_string(),
            188 => "Canon EF 70-200mm f/4L USM + 2x".to_string(),
            189 => "Canon EF 70-200mm f/4L USM + 2.8x".to_string(),
            190 => "Canon EF 100mm f/2.8 Macro USM".to_string(),
            191 => "Canon EF 400mm f/4 DO IS or Sigma Lens".to_string(),
            193 => "Canon EF 35-80mm f/4-5.6 USM".to_string(),
            194 => "Canon EF 80-200mm f/4.5-5.6 USM".to_string(),
            195 => "Canon EF 35-105mm f/4.5-5.6 USM".to_string(),
            196 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            197 => "Canon EF 75-300mm f/4-5.6 IS USM or Sigma Lens".to_string(),
            198 => "Canon EF 50mm f/1.4 USM or Other Lens".to_string(),
            199 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            200 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            201 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            202 => "Canon EF 28-80mm f/3.5-5.6 USM IV".to_string(),
            208 => "Canon EF 22-55mm f/4-5.6 USM".to_string(),
            209 => "Canon EF 55-200mm f/4.5-5.6".to_string(),
            210 => "Canon EF 28-90mm f/4-5.6 USM".to_string(),
            211 => "Canon EF 28-200mm f/3.5-5.6 USM".to_string(),
            212 => "Canon EF 28-105mm f/4-5.6 USM".to_string(),
            213 => "Canon EF 90-300mm f/4.5-5.6 USM or Tamron Lens".to_string(),
            214 => "Canon EF-S 18-55mm f/3.5-5.6 USM".to_string(),
            215 => "Canon EF 55-200mm f/4.5-5.6 II USM".to_string(),
            217 => "Tamron AF 18-270mm f/3.5-6.3 Di II VC PZD".to_string(),
            220 => "Yongnuo YN 50mm f/1.8".to_string(),
            224 => "Canon EF 70-200mm f/2.8L IS USM".to_string(),
            225 => "Canon EF 70-200mm f/2.8L IS USM + 1.4x".to_string(),
            226 => "Canon EF 70-200mm f/2.8L IS USM + 2x".to_string(),
            227 => "Canon EF 70-200mm f/2.8L IS USM + 2.8x".to_string(),
            228 => "Canon EF 28-105mm f/3.5-4.5 USM".to_string(),
            229 => "Canon EF 16-35mm f/2.8L USM".to_string(),
            230 => "Canon EF 24-70mm f/2.8L USM".to_string(),
            231 => "Canon EF 17-40mm f/4L USM or Sigma Lens".to_string(),
            232 => "Canon EF 70-300mm f/4.5-5.6 DO IS USM".to_string(),
            233 => "Canon EF 28-300mm f/3.5-5.6L IS USM".to_string(),
            234 => "Canon EF-S 17-85mm f/4-5.6 IS USM or Tokina Lens".to_string(),
            235 => "Canon EF-S 10-22mm f/3.5-4.5 USM".to_string(),
            236 => "Canon EF-S 60mm f/2.8 Macro USM".to_string(),
            237 => "Canon EF 24-105mm f/4L IS USM".to_string(),
            238 => "Canon EF 70-300mm f/4-5.6 IS USM".to_string(),
            239 => "Canon EF 85mm f/1.2L II USM or Rokinon Lens".to_string(),
            240 => "Canon EF-S 17-55mm f/2.8 IS USM or Sigma Lens".to_string(),
            241 => "Canon EF 50mm f/1.2L USM".to_string(),
            242 => "Canon EF 70-200mm f/4L IS USM".to_string(),
            243 => "Canon EF 70-200mm f/4L IS USM + 1.4x".to_string(),
            244 => "Canon EF 70-200mm f/4L IS USM + 2x".to_string(),
            245 => "Canon EF 70-200mm f/4L IS USM + 2.8x".to_string(),
            246 => "Canon EF 16-35mm f/2.8L II USM".to_string(),
            247 => "Canon EF 14mm f/2.8L II USM".to_string(),
            248 => "Canon EF 200mm f/2L IS USM or Sigma Lens".to_string(),
            249 => "Canon EF 800mm f/5.6L IS USM".to_string(),
            250 => "Canon EF 24mm f/1.4L II USM or Sigma Lens".to_string(),
            251 => "Canon EF 70-200mm f/2.8L IS II USM".to_string(),
            252 => "Canon EF 70-200mm f/2.8L IS II USM + 1.4x".to_string(),
            253 => "Canon EF 70-200mm f/2.8L IS II USM + 2x".to_string(),
            254 => "Canon EF 100mm f/2.8L Macro IS USM or Tamron Lens".to_string(),
            255 => "Sigma 24-105mm f/4 DG OS HSM | A or Other Lens".to_string(),
            368 => "Sigma 14-24mm f/2.8 DG HSM | A or other Sigma Lens".to_string(),
            488 => "Canon EF-S 15-85mm f/3.5-5.6 IS USM".to_string(),
            489 => "Canon EF 70-300mm f/4-5.6L IS USM".to_string(),
            490 => "Canon EF 8-15mm f/4L Fisheye USM".to_string(),
            491 => "Canon EF 300mm f/2.8L IS II USM or Tamron Lens".to_string(),
            492 => "Canon EF 400mm f/2.8L IS II USM".to_string(),
            493 => "Canon EF 500mm f/4L IS II USM or EF 24-105mm f4L IS USM".to_string(),
            494 => "Canon EF 600mm f/4L IS II USM".to_string(),
            495 => "Canon EF 24-70mm f/2.8L II USM or Sigma Lens".to_string(),
            496 => "Canon EF 200-400mm f/4L IS USM".to_string(),
            499 => "Canon EF 200-400mm f/4L IS USM + 1.4x".to_string(),
            502 => "Canon EF 28mm f/2.8 IS USM or Tamron Lens".to_string(),
            503 => "Canon EF 24mm f/2.8 IS USM".to_string(),
            504 => "Canon EF 24-70mm f/4L IS USM".to_string(),
            505 => "Canon EF 35mm f/2 IS USM".to_string(),
            506 => "Canon EF 400mm f/4 DO IS II USM".to_string(),
            507 => "Canon EF 16-35mm f/4L IS USM".to_string(),
            508 => "Canon EF 11-24mm f/4L USM or Tamron Lens".to_string(),
            624 => "Sigma 70-200mm f/2.8 DG OS HSM | S or other Sigma Lens".to_string(),
            747 => "Canon EF 100-400mm f/4.5-5.6L IS II USM or Tamron Lens".to_string(),
            748 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 1.4x or Tamron Lens".to_string(),
            749 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 2x or Tamron Lens".to_string(),
            750 => "Canon EF 35mm f/1.4L II USM or Tamron Lens".to_string(),
            751 => "Canon EF 16-35mm f/2.8L III USM".to_string(),
            752 => "Canon EF 24-105mm f/4L IS II USM".to_string(),
            753 => "Canon EF 85mm f/1.4L IS USM".to_string(),
            754 => "Canon EF 70-200mm f/4L IS II USM".to_string(),
            757 => "Canon EF 400mm f/2.8L IS III USM".to_string(),
            758 => "Canon EF 600mm f/4L IS III USM".to_string(),
            923 => "Meike/SKY 85mm f/1.8 DCM".to_string(),
            1136 => "Sigma 24-70mm f/2.8 DG OS HSM | A".to_string(),
            4142 => "Canon EF-S 18-135mm f/3.5-5.6 IS STM".to_string(),
            4143 => "Canon EF-M 18-55mm f/3.5-5.6 IS STM or Tamron Lens".to_string(),
            4144 => "Canon EF 40mm f/2.8 STM".to_string(),
            4145 => "Canon EF-M 22mm f/2 STM".to_string(),
            4146 => "Canon EF-S 18-55mm f/3.5-5.6 IS STM".to_string(),
            4147 => "Canon EF-M 11-22mm f/4-5.6 IS STM".to_string(),
            4148 => "Canon EF-S 55-250mm f/4-5.6 IS STM".to_string(),
            4149 => "Canon EF-M 55-200mm f/4.5-6.3 IS STM".to_string(),
            4150 => "Canon EF-S 10-18mm f/4.5-5.6 IS STM".to_string(),
            4152 => "Canon EF 24-105mm f/3.5-5.6 IS STM".to_string(),
            4153 => "Canon EF-M 15-45mm f/3.5-6.3 IS STM".to_string(),
            4154 => "Canon EF-S 24mm f/2.8 STM".to_string(),
            4155 => "Canon EF-M 28mm f/3.5 Macro IS STM".to_string(),
            4156 => "Canon EF 50mm f/1.8 STM".to_string(),
            4157 => "Canon EF-M 18-150mm f/3.5-6.3 IS STM".to_string(),
            4158 => "Canon EF-S 18-55mm f/4-5.6 IS STM".to_string(),
            4159 => "Canon EF-M 32mm f/1.4 STM".to_string(),
            4160 => "Canon EF-S 35mm f/2.8 Macro IS STM".to_string(),
            4208 => "Sigma 56mm f/1.4 DC DN | C or other Sigma Lens".to_string(),
            4976 => "Sigma 16-300mm F3.5-6.7 DC OS | C (025)".to_string(),
            6512 => "Sigma 12mm F1.4 DC | C".to_string(),
            36910 => "Canon EF 70-300mm f/4-5.6 IS II USM".to_string(),
            36912 => "Canon EF-S 18-135mm f/3.5-5.6 IS USM".to_string(),
            61182 => "Canon RF 50mm F1.2L USM or other Canon RF Lens".to_string(),
            61491 => "Canon CN-E 14mm T3.1 L F".to_string(),
            61492 => "Canon CN-E 24mm T1.5 L F".to_string(),
            61494 => "Canon CN-E 85mm T1.3 L F".to_string(),
            61495 => "Canon CN-E 135mm T2.2 L F".to_string(),
            61496 => "Canon CN-E 35mm T1.5 L F".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("LensType", 0xea, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0xec, bo) {
        dm.push(("MinFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MinFocalLength", 0xec, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0xee, bo) {
        dm.push(("MaxFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MaxFocalLength", 0xee, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(text) = text_at(data, 0x15e, 6, true) {
        tags.push(mk("FirmwareVersion", 0x15e, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x19b, bo) {
        dm.push(("FileIndex".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val + 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("FileIndex", 0x19b, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x1a7, bo) {
        dm.push(("DirectoryIndex".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("DirectoryIndex", 0x1a7, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(sub) = data.get(0x2d7..) {
        tags.extend(canon_psinfo(sub, model, bo, file_type, format, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::CameraInfo60D` -- FORMAT int8u, FIRST_ENTRY 0.
fn canon_camerainfo60d(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = u8_at(data, 0x3) {
        dm.push(("FNumber".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(($val-8)/16*log(2))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("sprintf(\"%.2g\",$val)", &cv, &ctx) { cv = x; }
            tags.push(mk("FNumber", 0x3, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x4) {
        dm.push(("ExposureTime".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(4*log(2)*(1-Image::ExifTool::Canon::CanonEv($val-24)))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("Image::ExifTool::Exif::PrintExposureTime($val)", &cv, &ctx) { cv = x; }
            tags.push(mk("ExposureTime", 0x4, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x6) {
        dm.push(("ISO".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("100*exp(($val/8-9)*log(2))", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("sprintf(\"%.0f\",$val)", &cv, &ctx) { cv = x; }
        tags.push(mk("ISO", 0x6, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x19) {
        dm.push(("CameraTemperature".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 128", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
        tags.push(mk("CameraTemperature", 0x19, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x1e, bo) {
        dm.push(("FocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
            tags.push(mk("FocalLength", 0x1e, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if MODEL_RE_6.is_match(model) {
        if let Some(v) = u8_at(data, 0x36) {
            dm.push(("CameraOrientation".to_string(), f64::from(v)));
            let s = match v as i64 {
                0 => "Horizontal (normal)".to_string(),
                1 => "Rotate 90 CW".to_string(),
                2 => "Rotate 270 CW".to_string(),
                other => other.to_string(),
            };
            tags.push(mk("CameraOrientation", 0x36, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if MODEL_RE_7.is_match(model) {
        if let Some(v) = u8_at(data, 0x3a) {
            dm.push(("CameraOrientation".to_string(), f64::from(v)));
            let s = match v as i64 {
                0 => "Horizontal (normal)".to_string(),
                1 => "Rotate 90 CW".to_string(),
                2 => "Rotate 270 CW".to_string(),
                other => other.to_string(),
            };
            tags.push(mk("CameraOrientation", 0x3a, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if MODEL_RE_6.is_match(model) {
        if let Some(v) = u16rev_at(data, 0x55, bo) {
            dm.push(("FocusDistanceUpper".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
            tags.push(mk("FocusDistanceUpper", 0x55, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if MODEL_RE_6.is_match(model) {
        if let Some(v) = u16rev_at(data, 0x57, bo) {
            dm.push(("FocusDistanceLower".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
            tags.push(mk("FocusDistanceLower", 0x57, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if MODEL_RE_6.is_match(model) {
        if let Some(v) = u16_at(data, 0x7d, bo) {
            dm.push(("ColorTemperature".to_string(), f64::from(v)));
            tags.push(mk("ColorTemperature", 0x7d, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u16rev_at(data, 0xe8, bo) {
        dm.push(("LensType".to_string(), f64::from(v)));
        let s = match v as i64 {
            -1 => "n/a".to_string(),
            1 => "Canon EF 50mm f/1.8".to_string(),
            2 => "Canon EF 28mm f/2.8 or Sigma Lens".to_string(),
            3 => "Canon EF 135mm f/2.8 Soft".to_string(),
            4 => "Canon EF 35-105mm f/3.5-4.5 or Sigma Lens".to_string(),
            5 => "Canon EF 35-70mm f/3.5-4.5".to_string(),
            6 => "Canon EF 28-70mm f/3.5-4.5 or Sigma or Tokina Lens".to_string(),
            7 => "Canon EF 100-300mm f/5.6L".to_string(),
            8 => "Canon EF 100-300mm f/5.6 or Sigma or Tokina Lens".to_string(),
            9 => "Canon EF 70-210mm f/4".to_string(),
            10 => "Canon EF 50mm f/2.5 Macro or Sigma Lens".to_string(),
            11 => "Canon EF 35mm f/2".to_string(),
            13 => "Canon EF 15mm f/2.8 Fisheye".to_string(),
            14 => "Canon EF 50-200mm f/3.5-4.5L".to_string(),
            15 => "Canon EF 50-200mm f/3.5-4.5".to_string(),
            16 => "Canon EF 35-135mm f/3.5-4.5".to_string(),
            17 => "Canon EF 35-70mm f/3.5-4.5A".to_string(),
            18 => "Canon EF 28-70mm f/3.5-4.5".to_string(),
            20 => "Canon EF 100-200mm f/4.5A".to_string(),
            21 => "Canon EF 80-200mm f/2.8L".to_string(),
            22 => "Canon EF 20-35mm f/2.8L or Tokina Lens".to_string(),
            23 => "Canon EF 35-105mm f/3.5-4.5".to_string(),
            24 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            25 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            26 => "Canon EF 100mm f/2.8 Macro or Other Lens".to_string(),
            27 => "Canon EF 35-80mm f/4-5.6".to_string(),
            28 => "Canon EF 80-200mm f/4.5-5.6 or Tamron Lens".to_string(),
            29 => "Canon EF 50mm f/1.8 II".to_string(),
            30 => "Canon EF 35-105mm f/4.5-5.6".to_string(),
            31 => "Canon EF 75-300mm f/4-5.6 or Tamron Lens".to_string(),
            32 => "Canon EF 24mm f/2.8 or Sigma Lens".to_string(),
            33 => "Voigtlander or Carl Zeiss Lens".to_string(),
            35 => "Canon EF 35-80mm f/4-5.6".to_string(),
            36 => "Canon EF 38-76mm f/4.5-5.6".to_string(),
            37 => "Canon EF 35-80mm f/4-5.6 or Tamron Lens".to_string(),
            38 => "Canon EF 80-200mm f/4.5-5.6 II".to_string(),
            39 => "Canon EF 75-300mm f/4-5.6".to_string(),
            40 => "Canon EF 28-80mm f/3.5-5.6".to_string(),
            41 => "Canon EF 28-90mm f/4-5.6".to_string(),
            42 => "Canon EF 28-200mm f/3.5-5.6 or Tamron Lens".to_string(),
            43 => "Canon EF 28-105mm f/4-5.6".to_string(),
            44 => "Canon EF 90-300mm f/4.5-5.6".to_string(),
            45 => "Canon EF-S 18-55mm f/3.5-5.6 [II]".to_string(),
            46 => "Canon EF 28-90mm f/4-5.6".to_string(),
            47 => "Zeiss Milvus 35mm f/2 or 50mm f/2".to_string(),
            48 => "Canon EF-S 18-55mm f/3.5-5.6 IS".to_string(),
            49 => "Canon EF-S 55-250mm f/4-5.6 IS".to_string(),
            50 => "Canon EF-S 18-200mm f/3.5-5.6 IS".to_string(),
            51 => "Canon EF-S 18-135mm f/3.5-5.6 IS".to_string(),
            52 => "Canon EF-S 18-55mm f/3.5-5.6 IS II".to_string(),
            53 => "Canon EF-S 18-55mm f/3.5-5.6 III".to_string(),
            54 => "Canon EF-S 55-250mm f/4-5.6 IS II".to_string(),
            60 => "Irix 11mm f/4 or 15mm f/2.4".to_string(),
            63 => "Irix 30mm F1.4 Dragonfly".to_string(),
            80 => "Canon TS-E 50mm f/2.8L Macro".to_string(),
            81 => "Canon TS-E 90mm f/2.8L Macro".to_string(),
            82 => "Canon TS-E 135mm f/4L Macro".to_string(),
            94 => "Canon TS-E 17mm f/4L".to_string(),
            95 => "Canon TS-E 24mm f/3.5L II".to_string(),
            103 => "Samyang AF 14mm f/2.8 EF or Rokinon Lens".to_string(),
            106 => "Rokinon SP / Samyang XP 35mm f/1.2".to_string(),
            112 => "Sigma 28mm f/1.5 FF High-speed Prime or other Sigma Lens".to_string(),
            117 => "Tamron 35-150mm f/2.8-4.0 Di VC OSD (A043) or other Tamron Lens".to_string(),
            124 => "Canon MP-E 65mm f/2.8 1-5x Macro Photo".to_string(),
            125 => "Canon TS-E 24mm f/3.5L".to_string(),
            126 => "Canon TS-E 45mm f/2.8".to_string(),
            127 => "Canon TS-E 90mm f/2.8 or Tamron Lens".to_string(),
            129 => "Canon EF 300mm f/2.8L USM".to_string(),
            130 => "Canon EF 50mm f/1.0L USM".to_string(),
            131 => "Canon EF 28-80mm f/2.8-4L USM or Sigma Lens".to_string(),
            132 => "Canon EF 1200mm f/5.6L USM".to_string(),
            134 => "Canon EF 600mm f/4L IS USM".to_string(),
            135 => "Canon EF 200mm f/1.8L USM".to_string(),
            136 => "Canon EF 300mm f/2.8L USM".to_string(),
            137 => "Canon EF 85mm f/1.2L USM or Sigma or Tamron Lens".to_string(),
            138 => "Canon EF 28-80mm f/2.8-4L".to_string(),
            139 => "Canon EF 400mm f/2.8L USM".to_string(),
            140 => "Canon EF 500mm f/4.5L USM".to_string(),
            141 => "Canon EF 500mm f/4.5L USM".to_string(),
            142 => "Canon EF 300mm f/2.8L IS USM".to_string(),
            143 => "Canon EF 500mm f/4L IS USM or Sigma Lens".to_string(),
            144 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            145 => "Canon EF 100-300mm f/4.5-5.6 USM".to_string(),
            146 => "Canon EF 70-210mm f/3.5-4.5 USM".to_string(),
            147 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            148 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            149 => "Canon EF 100mm f/2 USM".to_string(),
            150 => "Canon EF 14mm f/2.8L USM or Sigma Lens".to_string(),
            151 => "Canon EF 200mm f/2.8L USM".to_string(),
            152 => "Canon EF 300mm f/4L IS USM or Sigma Lens".to_string(),
            153 => "Canon EF 35-350mm f/3.5-5.6L USM or Sigma or Tamron Lens".to_string(),
            154 => "Canon EF 20mm f/2.8 USM or Zeiss Lens".to_string(),
            155 => "Canon EF 85mm f/1.8 USM or Sigma Lens".to_string(),
            156 => "Canon EF 28-105mm f/3.5-4.5 USM or Tamron Lens".to_string(),
            160 => "Canon EF 20-35mm f/3.5-4.5 USM or Tamron or Tokina Lens".to_string(),
            161 => "Canon EF 28-70mm f/2.8L USM or Other Lens".to_string(),
            162 => "Canon EF 200mm f/2.8L USM".to_string(),
            163 => "Canon EF 300mm f/4L".to_string(),
            164 => "Canon EF 400mm f/5.6L".to_string(),
            165 => "Canon EF 70-200mm f/2.8L USM".to_string(),
            166 => "Canon EF 70-200mm f/2.8L USM + 1.4x".to_string(),
            167 => "Canon EF 70-200mm f/2.8L USM + 2x".to_string(),
            168 => "Canon EF 28mm f/1.8 USM or Sigma Lens".to_string(),
            169 => "Canon EF 17-35mm f/2.8L USM or Sigma Lens".to_string(),
            170 => "Canon EF 200mm f/2.8L II USM or Sigma Lens".to_string(),
            171 => "Canon EF 300mm f/4L USM".to_string(),
            172 => "Canon EF 400mm f/5.6L USM or Sigma Lens".to_string(),
            173 => "Canon EF 180mm Macro f/3.5L USM or Sigma Lens".to_string(),
            174 => "Canon EF 135mm f/2L USM or Other Lens".to_string(),
            175 => "Canon EF 400mm f/2.8L USM".to_string(),
            176 => "Canon EF 24-85mm f/3.5-4.5 USM".to_string(),
            177 => "Canon EF 300mm f/4L IS USM".to_string(),
            178 => "Canon EF 28-135mm f/3.5-5.6 IS".to_string(),
            179 => "Canon EF 24mm f/1.4L USM".to_string(),
            180 => "Canon EF 35mm f/1.4L USM or Other Lens".to_string(),
            181 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 1.4x or Sigma Lens".to_string(),
            182 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 2x or Sigma Lens".to_string(),
            183 => "Canon EF 100-400mm f/4.5-5.6L IS USM or Sigma Lens".to_string(),
            184 => "Canon EF 400mm f/2.8L USM + 2x".to_string(),
            185 => "Canon EF 600mm f/4L IS USM".to_string(),
            186 => "Canon EF 70-200mm f/4L USM".to_string(),
            187 => "Canon EF 70-200mm f/4L USM + 1.4x".to_string(),
            188 => "Canon EF 70-200mm f/4L USM + 2x".to_string(),
            189 => "Canon EF 70-200mm f/4L USM + 2.8x".to_string(),
            190 => "Canon EF 100mm f/2.8 Macro USM".to_string(),
            191 => "Canon EF 400mm f/4 DO IS or Sigma Lens".to_string(),
            193 => "Canon EF 35-80mm f/4-5.6 USM".to_string(),
            194 => "Canon EF 80-200mm f/4.5-5.6 USM".to_string(),
            195 => "Canon EF 35-105mm f/4.5-5.6 USM".to_string(),
            196 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            197 => "Canon EF 75-300mm f/4-5.6 IS USM or Sigma Lens".to_string(),
            198 => "Canon EF 50mm f/1.4 USM or Other Lens".to_string(),
            199 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            200 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            201 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            202 => "Canon EF 28-80mm f/3.5-5.6 USM IV".to_string(),
            208 => "Canon EF 22-55mm f/4-5.6 USM".to_string(),
            209 => "Canon EF 55-200mm f/4.5-5.6".to_string(),
            210 => "Canon EF 28-90mm f/4-5.6 USM".to_string(),
            211 => "Canon EF 28-200mm f/3.5-5.6 USM".to_string(),
            212 => "Canon EF 28-105mm f/4-5.6 USM".to_string(),
            213 => "Canon EF 90-300mm f/4.5-5.6 USM or Tamron Lens".to_string(),
            214 => "Canon EF-S 18-55mm f/3.5-5.6 USM".to_string(),
            215 => "Canon EF 55-200mm f/4.5-5.6 II USM".to_string(),
            217 => "Tamron AF 18-270mm f/3.5-6.3 Di II VC PZD".to_string(),
            220 => "Yongnuo YN 50mm f/1.8".to_string(),
            224 => "Canon EF 70-200mm f/2.8L IS USM".to_string(),
            225 => "Canon EF 70-200mm f/2.8L IS USM + 1.4x".to_string(),
            226 => "Canon EF 70-200mm f/2.8L IS USM + 2x".to_string(),
            227 => "Canon EF 70-200mm f/2.8L IS USM + 2.8x".to_string(),
            228 => "Canon EF 28-105mm f/3.5-4.5 USM".to_string(),
            229 => "Canon EF 16-35mm f/2.8L USM".to_string(),
            230 => "Canon EF 24-70mm f/2.8L USM".to_string(),
            231 => "Canon EF 17-40mm f/4L USM or Sigma Lens".to_string(),
            232 => "Canon EF 70-300mm f/4.5-5.6 DO IS USM".to_string(),
            233 => "Canon EF 28-300mm f/3.5-5.6L IS USM".to_string(),
            234 => "Canon EF-S 17-85mm f/4-5.6 IS USM or Tokina Lens".to_string(),
            235 => "Canon EF-S 10-22mm f/3.5-4.5 USM".to_string(),
            236 => "Canon EF-S 60mm f/2.8 Macro USM".to_string(),
            237 => "Canon EF 24-105mm f/4L IS USM".to_string(),
            238 => "Canon EF 70-300mm f/4-5.6 IS USM".to_string(),
            239 => "Canon EF 85mm f/1.2L II USM or Rokinon Lens".to_string(),
            240 => "Canon EF-S 17-55mm f/2.8 IS USM or Sigma Lens".to_string(),
            241 => "Canon EF 50mm f/1.2L USM".to_string(),
            242 => "Canon EF 70-200mm f/4L IS USM".to_string(),
            243 => "Canon EF 70-200mm f/4L IS USM + 1.4x".to_string(),
            244 => "Canon EF 70-200mm f/4L IS USM + 2x".to_string(),
            245 => "Canon EF 70-200mm f/4L IS USM + 2.8x".to_string(),
            246 => "Canon EF 16-35mm f/2.8L II USM".to_string(),
            247 => "Canon EF 14mm f/2.8L II USM".to_string(),
            248 => "Canon EF 200mm f/2L IS USM or Sigma Lens".to_string(),
            249 => "Canon EF 800mm f/5.6L IS USM".to_string(),
            250 => "Canon EF 24mm f/1.4L II USM or Sigma Lens".to_string(),
            251 => "Canon EF 70-200mm f/2.8L IS II USM".to_string(),
            252 => "Canon EF 70-200mm f/2.8L IS II USM + 1.4x".to_string(),
            253 => "Canon EF 70-200mm f/2.8L IS II USM + 2x".to_string(),
            254 => "Canon EF 100mm f/2.8L Macro IS USM or Tamron Lens".to_string(),
            255 => "Sigma 24-105mm f/4 DG OS HSM | A or Other Lens".to_string(),
            368 => "Sigma 14-24mm f/2.8 DG HSM | A or other Sigma Lens".to_string(),
            488 => "Canon EF-S 15-85mm f/3.5-5.6 IS USM".to_string(),
            489 => "Canon EF 70-300mm f/4-5.6L IS USM".to_string(),
            490 => "Canon EF 8-15mm f/4L Fisheye USM".to_string(),
            491 => "Canon EF 300mm f/2.8L IS II USM or Tamron Lens".to_string(),
            492 => "Canon EF 400mm f/2.8L IS II USM".to_string(),
            493 => "Canon EF 500mm f/4L IS II USM or EF 24-105mm f4L IS USM".to_string(),
            494 => "Canon EF 600mm f/4L IS II USM".to_string(),
            495 => "Canon EF 24-70mm f/2.8L II USM or Sigma Lens".to_string(),
            496 => "Canon EF 200-400mm f/4L IS USM".to_string(),
            499 => "Canon EF 200-400mm f/4L IS USM + 1.4x".to_string(),
            502 => "Canon EF 28mm f/2.8 IS USM or Tamron Lens".to_string(),
            503 => "Canon EF 24mm f/2.8 IS USM".to_string(),
            504 => "Canon EF 24-70mm f/4L IS USM".to_string(),
            505 => "Canon EF 35mm f/2 IS USM".to_string(),
            506 => "Canon EF 400mm f/4 DO IS II USM".to_string(),
            507 => "Canon EF 16-35mm f/4L IS USM".to_string(),
            508 => "Canon EF 11-24mm f/4L USM or Tamron Lens".to_string(),
            624 => "Sigma 70-200mm f/2.8 DG OS HSM | S or other Sigma Lens".to_string(),
            747 => "Canon EF 100-400mm f/4.5-5.6L IS II USM or Tamron Lens".to_string(),
            748 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 1.4x or Tamron Lens".to_string(),
            749 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 2x or Tamron Lens".to_string(),
            750 => "Canon EF 35mm f/1.4L II USM or Tamron Lens".to_string(),
            751 => "Canon EF 16-35mm f/2.8L III USM".to_string(),
            752 => "Canon EF 24-105mm f/4L IS II USM".to_string(),
            753 => "Canon EF 85mm f/1.4L IS USM".to_string(),
            754 => "Canon EF 70-200mm f/4L IS II USM".to_string(),
            757 => "Canon EF 400mm f/2.8L IS III USM".to_string(),
            758 => "Canon EF 600mm f/4L IS III USM".to_string(),
            923 => "Meike/SKY 85mm f/1.8 DCM".to_string(),
            1136 => "Sigma 24-70mm f/2.8 DG OS HSM | A".to_string(),
            4142 => "Canon EF-S 18-135mm f/3.5-5.6 IS STM".to_string(),
            4143 => "Canon EF-M 18-55mm f/3.5-5.6 IS STM or Tamron Lens".to_string(),
            4144 => "Canon EF 40mm f/2.8 STM".to_string(),
            4145 => "Canon EF-M 22mm f/2 STM".to_string(),
            4146 => "Canon EF-S 18-55mm f/3.5-5.6 IS STM".to_string(),
            4147 => "Canon EF-M 11-22mm f/4-5.6 IS STM".to_string(),
            4148 => "Canon EF-S 55-250mm f/4-5.6 IS STM".to_string(),
            4149 => "Canon EF-M 55-200mm f/4.5-6.3 IS STM".to_string(),
            4150 => "Canon EF-S 10-18mm f/4.5-5.6 IS STM".to_string(),
            4152 => "Canon EF 24-105mm f/3.5-5.6 IS STM".to_string(),
            4153 => "Canon EF-M 15-45mm f/3.5-6.3 IS STM".to_string(),
            4154 => "Canon EF-S 24mm f/2.8 STM".to_string(),
            4155 => "Canon EF-M 28mm f/3.5 Macro IS STM".to_string(),
            4156 => "Canon EF 50mm f/1.8 STM".to_string(),
            4157 => "Canon EF-M 18-150mm f/3.5-6.3 IS STM".to_string(),
            4158 => "Canon EF-S 18-55mm f/4-5.6 IS STM".to_string(),
            4159 => "Canon EF-M 32mm f/1.4 STM".to_string(),
            4160 => "Canon EF-S 35mm f/2.8 Macro IS STM".to_string(),
            4208 => "Sigma 56mm f/1.4 DC DN | C or other Sigma Lens".to_string(),
            4976 => "Sigma 16-300mm F3.5-6.7 DC OS | C (025)".to_string(),
            6512 => "Sigma 12mm F1.4 DC | C".to_string(),
            36910 => "Canon EF 70-300mm f/4-5.6 IS II USM".to_string(),
            36912 => "Canon EF-S 18-135mm f/3.5-5.6 IS USM".to_string(),
            61182 => "Canon RF 50mm F1.2L USM or other Canon RF Lens".to_string(),
            61491 => "Canon CN-E 14mm T3.1 L F".to_string(),
            61492 => "Canon CN-E 24mm T1.5 L F".to_string(),
            61494 => "Canon CN-E 85mm T1.3 L F".to_string(),
            61495 => "Canon CN-E 135mm T2.2 L F".to_string(),
            61496 => "Canon CN-E 35mm T1.5 L F".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("LensType", 0xe8, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0xea, bo) {
        dm.push(("MinFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MinFocalLength", 0xea, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0xec, bo) {
        dm.push(("MaxFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MaxFocalLength", 0xec, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(text) = text_at(data, 0x199, 6, true) {
        tags.push(mk("FirmwareVersion", 0x199, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    if MODEL_RE_6.is_match(model) {
        if let Some(v) = u32_at(data, 0x1d9, bo) {
            dm.push(("FileIndex".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("$val + 1", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            tags.push(mk("FileIndex", 0x1d9, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if MODEL_RE_6.is_match(model) {
        if let Some(v) = u32_at(data, 0x1e5, bo) {
            dm.push(("DirectoryIndex".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("$val - 1", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            tags.push(mk("DirectoryIndex", 0x1e5, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if MODEL_RE_7.is_match(model) {
        if let Some(sub) = data.get(0x2f9..) {
            tags.extend(canon_psinfo2(sub, model, bo, file_type, format, dm));
        }
    }
    if MODEL_RE_6.is_match(model) {
        if let Some(sub) = data.get(0x321..) {
            tags.extend(canon_psinfo2(sub, model, bo, file_type, format, dm));
        }
    }
    tags
}

/// `Image::ExifTool::Canon::CameraInfo70D` -- FORMAT int8u, FIRST_ENTRY 0.
fn canon_camerainfo70d(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = u8_at(data, 0x3) {
        dm.push(("FNumber".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(($val-8)/16*log(2))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("sprintf(\"%.2g\",$val)", &cv, &ctx) { cv = x; }
            tags.push(mk("FNumber", 0x3, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x4) {
        dm.push(("ExposureTime".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(4*log(2)*(1-Image::ExifTool::Canon::CanonEv($val-24)))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("Image::ExifTool::Exif::PrintExposureTime($val)", &cv, &ctx) { cv = x; }
            tags.push(mk("ExposureTime", 0x4, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x6) {
        dm.push(("ISO".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("100*exp(($val/8-9)*log(2))", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("sprintf(\"%.0f\",$val)", &cv, &ctx) { cv = x; }
        tags.push(mk("ISO", 0x6, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x1b) {
        dm.push(("CameraTemperature".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 128", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
        tags.push(mk("CameraTemperature", 0x1b, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x23, bo) {
        dm.push(("FocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
            tags.push(mk("FocalLength", 0x23, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x84) {
        dm.push(("CameraOrientation".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Horizontal (normal)".to_string(),
            1 => "Rotate 90 CW".to_string(),
            2 => "Rotate 270 CW".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("CameraOrientation", 0x84, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x93, bo) {
        dm.push(("FocusDistanceUpper".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceUpper", 0x93, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x95, bo) {
        dm.push(("FocusDistanceLower".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceLower", 0x95, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0xc7, bo) {
        dm.push(("ColorTemperature".to_string(), f64::from(v)));
        tags.push(mk("ColorTemperature", 0xc7, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x166, bo) {
        dm.push(("LensType".to_string(), f64::from(v)));
        let s = match v as i64 {
            -1 => "n/a".to_string(),
            1 => "Canon EF 50mm f/1.8".to_string(),
            2 => "Canon EF 28mm f/2.8 or Sigma Lens".to_string(),
            3 => "Canon EF 135mm f/2.8 Soft".to_string(),
            4 => "Canon EF 35-105mm f/3.5-4.5 or Sigma Lens".to_string(),
            5 => "Canon EF 35-70mm f/3.5-4.5".to_string(),
            6 => "Canon EF 28-70mm f/3.5-4.5 or Sigma or Tokina Lens".to_string(),
            7 => "Canon EF 100-300mm f/5.6L".to_string(),
            8 => "Canon EF 100-300mm f/5.6 or Sigma or Tokina Lens".to_string(),
            9 => "Canon EF 70-210mm f/4".to_string(),
            10 => "Canon EF 50mm f/2.5 Macro or Sigma Lens".to_string(),
            11 => "Canon EF 35mm f/2".to_string(),
            13 => "Canon EF 15mm f/2.8 Fisheye".to_string(),
            14 => "Canon EF 50-200mm f/3.5-4.5L".to_string(),
            15 => "Canon EF 50-200mm f/3.5-4.5".to_string(),
            16 => "Canon EF 35-135mm f/3.5-4.5".to_string(),
            17 => "Canon EF 35-70mm f/3.5-4.5A".to_string(),
            18 => "Canon EF 28-70mm f/3.5-4.5".to_string(),
            20 => "Canon EF 100-200mm f/4.5A".to_string(),
            21 => "Canon EF 80-200mm f/2.8L".to_string(),
            22 => "Canon EF 20-35mm f/2.8L or Tokina Lens".to_string(),
            23 => "Canon EF 35-105mm f/3.5-4.5".to_string(),
            24 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            25 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            26 => "Canon EF 100mm f/2.8 Macro or Other Lens".to_string(),
            27 => "Canon EF 35-80mm f/4-5.6".to_string(),
            28 => "Canon EF 80-200mm f/4.5-5.6 or Tamron Lens".to_string(),
            29 => "Canon EF 50mm f/1.8 II".to_string(),
            30 => "Canon EF 35-105mm f/4.5-5.6".to_string(),
            31 => "Canon EF 75-300mm f/4-5.6 or Tamron Lens".to_string(),
            32 => "Canon EF 24mm f/2.8 or Sigma Lens".to_string(),
            33 => "Voigtlander or Carl Zeiss Lens".to_string(),
            35 => "Canon EF 35-80mm f/4-5.6".to_string(),
            36 => "Canon EF 38-76mm f/4.5-5.6".to_string(),
            37 => "Canon EF 35-80mm f/4-5.6 or Tamron Lens".to_string(),
            38 => "Canon EF 80-200mm f/4.5-5.6 II".to_string(),
            39 => "Canon EF 75-300mm f/4-5.6".to_string(),
            40 => "Canon EF 28-80mm f/3.5-5.6".to_string(),
            41 => "Canon EF 28-90mm f/4-5.6".to_string(),
            42 => "Canon EF 28-200mm f/3.5-5.6 or Tamron Lens".to_string(),
            43 => "Canon EF 28-105mm f/4-5.6".to_string(),
            44 => "Canon EF 90-300mm f/4.5-5.6".to_string(),
            45 => "Canon EF-S 18-55mm f/3.5-5.6 [II]".to_string(),
            46 => "Canon EF 28-90mm f/4-5.6".to_string(),
            47 => "Zeiss Milvus 35mm f/2 or 50mm f/2".to_string(),
            48 => "Canon EF-S 18-55mm f/3.5-5.6 IS".to_string(),
            49 => "Canon EF-S 55-250mm f/4-5.6 IS".to_string(),
            50 => "Canon EF-S 18-200mm f/3.5-5.6 IS".to_string(),
            51 => "Canon EF-S 18-135mm f/3.5-5.6 IS".to_string(),
            52 => "Canon EF-S 18-55mm f/3.5-5.6 IS II".to_string(),
            53 => "Canon EF-S 18-55mm f/3.5-5.6 III".to_string(),
            54 => "Canon EF-S 55-250mm f/4-5.6 IS II".to_string(),
            60 => "Irix 11mm f/4 or 15mm f/2.4".to_string(),
            63 => "Irix 30mm F1.4 Dragonfly".to_string(),
            80 => "Canon TS-E 50mm f/2.8L Macro".to_string(),
            81 => "Canon TS-E 90mm f/2.8L Macro".to_string(),
            82 => "Canon TS-E 135mm f/4L Macro".to_string(),
            94 => "Canon TS-E 17mm f/4L".to_string(),
            95 => "Canon TS-E 24mm f/3.5L II".to_string(),
            103 => "Samyang AF 14mm f/2.8 EF or Rokinon Lens".to_string(),
            106 => "Rokinon SP / Samyang XP 35mm f/1.2".to_string(),
            112 => "Sigma 28mm f/1.5 FF High-speed Prime or other Sigma Lens".to_string(),
            117 => "Tamron 35-150mm f/2.8-4.0 Di VC OSD (A043) or other Tamron Lens".to_string(),
            124 => "Canon MP-E 65mm f/2.8 1-5x Macro Photo".to_string(),
            125 => "Canon TS-E 24mm f/3.5L".to_string(),
            126 => "Canon TS-E 45mm f/2.8".to_string(),
            127 => "Canon TS-E 90mm f/2.8 or Tamron Lens".to_string(),
            129 => "Canon EF 300mm f/2.8L USM".to_string(),
            130 => "Canon EF 50mm f/1.0L USM".to_string(),
            131 => "Canon EF 28-80mm f/2.8-4L USM or Sigma Lens".to_string(),
            132 => "Canon EF 1200mm f/5.6L USM".to_string(),
            134 => "Canon EF 600mm f/4L IS USM".to_string(),
            135 => "Canon EF 200mm f/1.8L USM".to_string(),
            136 => "Canon EF 300mm f/2.8L USM".to_string(),
            137 => "Canon EF 85mm f/1.2L USM or Sigma or Tamron Lens".to_string(),
            138 => "Canon EF 28-80mm f/2.8-4L".to_string(),
            139 => "Canon EF 400mm f/2.8L USM".to_string(),
            140 => "Canon EF 500mm f/4.5L USM".to_string(),
            141 => "Canon EF 500mm f/4.5L USM".to_string(),
            142 => "Canon EF 300mm f/2.8L IS USM".to_string(),
            143 => "Canon EF 500mm f/4L IS USM or Sigma Lens".to_string(),
            144 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            145 => "Canon EF 100-300mm f/4.5-5.6 USM".to_string(),
            146 => "Canon EF 70-210mm f/3.5-4.5 USM".to_string(),
            147 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            148 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            149 => "Canon EF 100mm f/2 USM".to_string(),
            150 => "Canon EF 14mm f/2.8L USM or Sigma Lens".to_string(),
            151 => "Canon EF 200mm f/2.8L USM".to_string(),
            152 => "Canon EF 300mm f/4L IS USM or Sigma Lens".to_string(),
            153 => "Canon EF 35-350mm f/3.5-5.6L USM or Sigma or Tamron Lens".to_string(),
            154 => "Canon EF 20mm f/2.8 USM or Zeiss Lens".to_string(),
            155 => "Canon EF 85mm f/1.8 USM or Sigma Lens".to_string(),
            156 => "Canon EF 28-105mm f/3.5-4.5 USM or Tamron Lens".to_string(),
            160 => "Canon EF 20-35mm f/3.5-4.5 USM or Tamron or Tokina Lens".to_string(),
            161 => "Canon EF 28-70mm f/2.8L USM or Other Lens".to_string(),
            162 => "Canon EF 200mm f/2.8L USM".to_string(),
            163 => "Canon EF 300mm f/4L".to_string(),
            164 => "Canon EF 400mm f/5.6L".to_string(),
            165 => "Canon EF 70-200mm f/2.8L USM".to_string(),
            166 => "Canon EF 70-200mm f/2.8L USM + 1.4x".to_string(),
            167 => "Canon EF 70-200mm f/2.8L USM + 2x".to_string(),
            168 => "Canon EF 28mm f/1.8 USM or Sigma Lens".to_string(),
            169 => "Canon EF 17-35mm f/2.8L USM or Sigma Lens".to_string(),
            170 => "Canon EF 200mm f/2.8L II USM or Sigma Lens".to_string(),
            171 => "Canon EF 300mm f/4L USM".to_string(),
            172 => "Canon EF 400mm f/5.6L USM or Sigma Lens".to_string(),
            173 => "Canon EF 180mm Macro f/3.5L USM or Sigma Lens".to_string(),
            174 => "Canon EF 135mm f/2L USM or Other Lens".to_string(),
            175 => "Canon EF 400mm f/2.8L USM".to_string(),
            176 => "Canon EF 24-85mm f/3.5-4.5 USM".to_string(),
            177 => "Canon EF 300mm f/4L IS USM".to_string(),
            178 => "Canon EF 28-135mm f/3.5-5.6 IS".to_string(),
            179 => "Canon EF 24mm f/1.4L USM".to_string(),
            180 => "Canon EF 35mm f/1.4L USM or Other Lens".to_string(),
            181 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 1.4x or Sigma Lens".to_string(),
            182 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 2x or Sigma Lens".to_string(),
            183 => "Canon EF 100-400mm f/4.5-5.6L IS USM or Sigma Lens".to_string(),
            184 => "Canon EF 400mm f/2.8L USM + 2x".to_string(),
            185 => "Canon EF 600mm f/4L IS USM".to_string(),
            186 => "Canon EF 70-200mm f/4L USM".to_string(),
            187 => "Canon EF 70-200mm f/4L USM + 1.4x".to_string(),
            188 => "Canon EF 70-200mm f/4L USM + 2x".to_string(),
            189 => "Canon EF 70-200mm f/4L USM + 2.8x".to_string(),
            190 => "Canon EF 100mm f/2.8 Macro USM".to_string(),
            191 => "Canon EF 400mm f/4 DO IS or Sigma Lens".to_string(),
            193 => "Canon EF 35-80mm f/4-5.6 USM".to_string(),
            194 => "Canon EF 80-200mm f/4.5-5.6 USM".to_string(),
            195 => "Canon EF 35-105mm f/4.5-5.6 USM".to_string(),
            196 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            197 => "Canon EF 75-300mm f/4-5.6 IS USM or Sigma Lens".to_string(),
            198 => "Canon EF 50mm f/1.4 USM or Other Lens".to_string(),
            199 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            200 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            201 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            202 => "Canon EF 28-80mm f/3.5-5.6 USM IV".to_string(),
            208 => "Canon EF 22-55mm f/4-5.6 USM".to_string(),
            209 => "Canon EF 55-200mm f/4.5-5.6".to_string(),
            210 => "Canon EF 28-90mm f/4-5.6 USM".to_string(),
            211 => "Canon EF 28-200mm f/3.5-5.6 USM".to_string(),
            212 => "Canon EF 28-105mm f/4-5.6 USM".to_string(),
            213 => "Canon EF 90-300mm f/4.5-5.6 USM or Tamron Lens".to_string(),
            214 => "Canon EF-S 18-55mm f/3.5-5.6 USM".to_string(),
            215 => "Canon EF 55-200mm f/4.5-5.6 II USM".to_string(),
            217 => "Tamron AF 18-270mm f/3.5-6.3 Di II VC PZD".to_string(),
            220 => "Yongnuo YN 50mm f/1.8".to_string(),
            224 => "Canon EF 70-200mm f/2.8L IS USM".to_string(),
            225 => "Canon EF 70-200mm f/2.8L IS USM + 1.4x".to_string(),
            226 => "Canon EF 70-200mm f/2.8L IS USM + 2x".to_string(),
            227 => "Canon EF 70-200mm f/2.8L IS USM + 2.8x".to_string(),
            228 => "Canon EF 28-105mm f/3.5-4.5 USM".to_string(),
            229 => "Canon EF 16-35mm f/2.8L USM".to_string(),
            230 => "Canon EF 24-70mm f/2.8L USM".to_string(),
            231 => "Canon EF 17-40mm f/4L USM or Sigma Lens".to_string(),
            232 => "Canon EF 70-300mm f/4.5-5.6 DO IS USM".to_string(),
            233 => "Canon EF 28-300mm f/3.5-5.6L IS USM".to_string(),
            234 => "Canon EF-S 17-85mm f/4-5.6 IS USM or Tokina Lens".to_string(),
            235 => "Canon EF-S 10-22mm f/3.5-4.5 USM".to_string(),
            236 => "Canon EF-S 60mm f/2.8 Macro USM".to_string(),
            237 => "Canon EF 24-105mm f/4L IS USM".to_string(),
            238 => "Canon EF 70-300mm f/4-5.6 IS USM".to_string(),
            239 => "Canon EF 85mm f/1.2L II USM or Rokinon Lens".to_string(),
            240 => "Canon EF-S 17-55mm f/2.8 IS USM or Sigma Lens".to_string(),
            241 => "Canon EF 50mm f/1.2L USM".to_string(),
            242 => "Canon EF 70-200mm f/4L IS USM".to_string(),
            243 => "Canon EF 70-200mm f/4L IS USM + 1.4x".to_string(),
            244 => "Canon EF 70-200mm f/4L IS USM + 2x".to_string(),
            245 => "Canon EF 70-200mm f/4L IS USM + 2.8x".to_string(),
            246 => "Canon EF 16-35mm f/2.8L II USM".to_string(),
            247 => "Canon EF 14mm f/2.8L II USM".to_string(),
            248 => "Canon EF 200mm f/2L IS USM or Sigma Lens".to_string(),
            249 => "Canon EF 800mm f/5.6L IS USM".to_string(),
            250 => "Canon EF 24mm f/1.4L II USM or Sigma Lens".to_string(),
            251 => "Canon EF 70-200mm f/2.8L IS II USM".to_string(),
            252 => "Canon EF 70-200mm f/2.8L IS II USM + 1.4x".to_string(),
            253 => "Canon EF 70-200mm f/2.8L IS II USM + 2x".to_string(),
            254 => "Canon EF 100mm f/2.8L Macro IS USM or Tamron Lens".to_string(),
            255 => "Sigma 24-105mm f/4 DG OS HSM | A or Other Lens".to_string(),
            368 => "Sigma 14-24mm f/2.8 DG HSM | A or other Sigma Lens".to_string(),
            488 => "Canon EF-S 15-85mm f/3.5-5.6 IS USM".to_string(),
            489 => "Canon EF 70-300mm f/4-5.6L IS USM".to_string(),
            490 => "Canon EF 8-15mm f/4L Fisheye USM".to_string(),
            491 => "Canon EF 300mm f/2.8L IS II USM or Tamron Lens".to_string(),
            492 => "Canon EF 400mm f/2.8L IS II USM".to_string(),
            493 => "Canon EF 500mm f/4L IS II USM or EF 24-105mm f4L IS USM".to_string(),
            494 => "Canon EF 600mm f/4L IS II USM".to_string(),
            495 => "Canon EF 24-70mm f/2.8L II USM or Sigma Lens".to_string(),
            496 => "Canon EF 200-400mm f/4L IS USM".to_string(),
            499 => "Canon EF 200-400mm f/4L IS USM + 1.4x".to_string(),
            502 => "Canon EF 28mm f/2.8 IS USM or Tamron Lens".to_string(),
            503 => "Canon EF 24mm f/2.8 IS USM".to_string(),
            504 => "Canon EF 24-70mm f/4L IS USM".to_string(),
            505 => "Canon EF 35mm f/2 IS USM".to_string(),
            506 => "Canon EF 400mm f/4 DO IS II USM".to_string(),
            507 => "Canon EF 16-35mm f/4L IS USM".to_string(),
            508 => "Canon EF 11-24mm f/4L USM or Tamron Lens".to_string(),
            624 => "Sigma 70-200mm f/2.8 DG OS HSM | S or other Sigma Lens".to_string(),
            747 => "Canon EF 100-400mm f/4.5-5.6L IS II USM or Tamron Lens".to_string(),
            748 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 1.4x or Tamron Lens".to_string(),
            749 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 2x or Tamron Lens".to_string(),
            750 => "Canon EF 35mm f/1.4L II USM or Tamron Lens".to_string(),
            751 => "Canon EF 16-35mm f/2.8L III USM".to_string(),
            752 => "Canon EF 24-105mm f/4L IS II USM".to_string(),
            753 => "Canon EF 85mm f/1.4L IS USM".to_string(),
            754 => "Canon EF 70-200mm f/4L IS II USM".to_string(),
            757 => "Canon EF 400mm f/2.8L IS III USM".to_string(),
            758 => "Canon EF 600mm f/4L IS III USM".to_string(),
            923 => "Meike/SKY 85mm f/1.8 DCM".to_string(),
            1136 => "Sigma 24-70mm f/2.8 DG OS HSM | A".to_string(),
            4142 => "Canon EF-S 18-135mm f/3.5-5.6 IS STM".to_string(),
            4143 => "Canon EF-M 18-55mm f/3.5-5.6 IS STM or Tamron Lens".to_string(),
            4144 => "Canon EF 40mm f/2.8 STM".to_string(),
            4145 => "Canon EF-M 22mm f/2 STM".to_string(),
            4146 => "Canon EF-S 18-55mm f/3.5-5.6 IS STM".to_string(),
            4147 => "Canon EF-M 11-22mm f/4-5.6 IS STM".to_string(),
            4148 => "Canon EF-S 55-250mm f/4-5.6 IS STM".to_string(),
            4149 => "Canon EF-M 55-200mm f/4.5-6.3 IS STM".to_string(),
            4150 => "Canon EF-S 10-18mm f/4.5-5.6 IS STM".to_string(),
            4152 => "Canon EF 24-105mm f/3.5-5.6 IS STM".to_string(),
            4153 => "Canon EF-M 15-45mm f/3.5-6.3 IS STM".to_string(),
            4154 => "Canon EF-S 24mm f/2.8 STM".to_string(),
            4155 => "Canon EF-M 28mm f/3.5 Macro IS STM".to_string(),
            4156 => "Canon EF 50mm f/1.8 STM".to_string(),
            4157 => "Canon EF-M 18-150mm f/3.5-6.3 IS STM".to_string(),
            4158 => "Canon EF-S 18-55mm f/4-5.6 IS STM".to_string(),
            4159 => "Canon EF-M 32mm f/1.4 STM".to_string(),
            4160 => "Canon EF-S 35mm f/2.8 Macro IS STM".to_string(),
            4208 => "Sigma 56mm f/1.4 DC DN | C or other Sigma Lens".to_string(),
            4976 => "Sigma 16-300mm F3.5-6.7 DC OS | C (025)".to_string(),
            6512 => "Sigma 12mm F1.4 DC | C".to_string(),
            36910 => "Canon EF 70-300mm f/4-5.6 IS II USM".to_string(),
            36912 => "Canon EF-S 18-135mm f/3.5-5.6 IS USM".to_string(),
            61182 => "Canon RF 50mm F1.2L USM or other Canon RF Lens".to_string(),
            61491 => "Canon CN-E 14mm T3.1 L F".to_string(),
            61492 => "Canon CN-E 24mm T1.5 L F".to_string(),
            61494 => "Canon CN-E 85mm T1.3 L F".to_string(),
            61495 => "Canon CN-E 135mm T2.2 L F".to_string(),
            61496 => "Canon CN-E 35mm T1.5 L F".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("LensType", 0x166, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x168, bo) {
        dm.push(("MinFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MinFocalLength", 0x168, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x16a, bo) {
        dm.push(("MaxFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MaxFocalLength", 0x16a, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(text) = text_at(data, 0x25e, 6, true) {
        tags.push(mk("FirmwareVersion", 0x25e, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x2b3, bo) {
        dm.push(("FileIndex".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val + 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("FileIndex", 0x2b3, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x2bf, bo) {
        dm.push(("DirectoryIndex".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("DirectoryIndex", 0x2bf, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(sub) = data.get(0x3cf..) {
        tags.extend(canon_psinfo2(sub, model, bo, file_type, format, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::CameraInfo80D` -- FORMAT int8u, FIRST_ENTRY 0.
fn canon_camerainfo80d(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = u8_at(data, 0x3) {
        dm.push(("FNumber".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(($val-8)/16*log(2))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("sprintf(\"%.2g\",$val)", &cv, &ctx) { cv = x; }
            tags.push(mk("FNumber", 0x3, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x4) {
        dm.push(("ExposureTime".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(4*log(2)*(1-Image::ExifTool::Canon::CanonEv($val-24)))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("Image::ExifTool::Exif::PrintExposureTime($val)", &cv, &ctx) { cv = x; }
            tags.push(mk("ExposureTime", 0x4, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x6) {
        dm.push(("ISO".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("100*exp(($val/8-9)*log(2))", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("sprintf(\"%.0f\",$val)", &cv, &ctx) { cv = x; }
        tags.push(mk("ISO", 0x6, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x1b) {
        dm.push(("CameraTemperature".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 128", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
        tags.push(mk("CameraTemperature", 0x1b, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x23, bo) {
        dm.push(("FocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
            tags.push(mk("FocalLength", 0x23, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x96) {
        dm.push(("CameraOrientation".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Horizontal (normal)".to_string(),
            1 => "Rotate 90 CW".to_string(),
            2 => "Rotate 270 CW".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("CameraOrientation", 0x96, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0xa5, bo) {
        dm.push(("FocusDistanceUpper".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceUpper", 0xa5, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0xa7, bo) {
        dm.push(("FocusDistanceLower".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceLower", 0xa7, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x13a, bo) {
        dm.push(("ColorTemperature".to_string(), f64::from(v)));
        tags.push(mk("ColorTemperature", 0x13a, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x189, bo) {
        dm.push(("LensType".to_string(), f64::from(v)));
        let s = match v as i64 {
            -1 => "n/a".to_string(),
            1 => "Canon EF 50mm f/1.8".to_string(),
            2 => "Canon EF 28mm f/2.8 or Sigma Lens".to_string(),
            3 => "Canon EF 135mm f/2.8 Soft".to_string(),
            4 => "Canon EF 35-105mm f/3.5-4.5 or Sigma Lens".to_string(),
            5 => "Canon EF 35-70mm f/3.5-4.5".to_string(),
            6 => "Canon EF 28-70mm f/3.5-4.5 or Sigma or Tokina Lens".to_string(),
            7 => "Canon EF 100-300mm f/5.6L".to_string(),
            8 => "Canon EF 100-300mm f/5.6 or Sigma or Tokina Lens".to_string(),
            9 => "Canon EF 70-210mm f/4".to_string(),
            10 => "Canon EF 50mm f/2.5 Macro or Sigma Lens".to_string(),
            11 => "Canon EF 35mm f/2".to_string(),
            13 => "Canon EF 15mm f/2.8 Fisheye".to_string(),
            14 => "Canon EF 50-200mm f/3.5-4.5L".to_string(),
            15 => "Canon EF 50-200mm f/3.5-4.5".to_string(),
            16 => "Canon EF 35-135mm f/3.5-4.5".to_string(),
            17 => "Canon EF 35-70mm f/3.5-4.5A".to_string(),
            18 => "Canon EF 28-70mm f/3.5-4.5".to_string(),
            20 => "Canon EF 100-200mm f/4.5A".to_string(),
            21 => "Canon EF 80-200mm f/2.8L".to_string(),
            22 => "Canon EF 20-35mm f/2.8L or Tokina Lens".to_string(),
            23 => "Canon EF 35-105mm f/3.5-4.5".to_string(),
            24 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            25 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            26 => "Canon EF 100mm f/2.8 Macro or Other Lens".to_string(),
            27 => "Canon EF 35-80mm f/4-5.6".to_string(),
            28 => "Canon EF 80-200mm f/4.5-5.6 or Tamron Lens".to_string(),
            29 => "Canon EF 50mm f/1.8 II".to_string(),
            30 => "Canon EF 35-105mm f/4.5-5.6".to_string(),
            31 => "Canon EF 75-300mm f/4-5.6 or Tamron Lens".to_string(),
            32 => "Canon EF 24mm f/2.8 or Sigma Lens".to_string(),
            33 => "Voigtlander or Carl Zeiss Lens".to_string(),
            35 => "Canon EF 35-80mm f/4-5.6".to_string(),
            36 => "Canon EF 38-76mm f/4.5-5.6".to_string(),
            37 => "Canon EF 35-80mm f/4-5.6 or Tamron Lens".to_string(),
            38 => "Canon EF 80-200mm f/4.5-5.6 II".to_string(),
            39 => "Canon EF 75-300mm f/4-5.6".to_string(),
            40 => "Canon EF 28-80mm f/3.5-5.6".to_string(),
            41 => "Canon EF 28-90mm f/4-5.6".to_string(),
            42 => "Canon EF 28-200mm f/3.5-5.6 or Tamron Lens".to_string(),
            43 => "Canon EF 28-105mm f/4-5.6".to_string(),
            44 => "Canon EF 90-300mm f/4.5-5.6".to_string(),
            45 => "Canon EF-S 18-55mm f/3.5-5.6 [II]".to_string(),
            46 => "Canon EF 28-90mm f/4-5.6".to_string(),
            47 => "Zeiss Milvus 35mm f/2 or 50mm f/2".to_string(),
            48 => "Canon EF-S 18-55mm f/3.5-5.6 IS".to_string(),
            49 => "Canon EF-S 55-250mm f/4-5.6 IS".to_string(),
            50 => "Canon EF-S 18-200mm f/3.5-5.6 IS".to_string(),
            51 => "Canon EF-S 18-135mm f/3.5-5.6 IS".to_string(),
            52 => "Canon EF-S 18-55mm f/3.5-5.6 IS II".to_string(),
            53 => "Canon EF-S 18-55mm f/3.5-5.6 III".to_string(),
            54 => "Canon EF-S 55-250mm f/4-5.6 IS II".to_string(),
            60 => "Irix 11mm f/4 or 15mm f/2.4".to_string(),
            63 => "Irix 30mm F1.4 Dragonfly".to_string(),
            80 => "Canon TS-E 50mm f/2.8L Macro".to_string(),
            81 => "Canon TS-E 90mm f/2.8L Macro".to_string(),
            82 => "Canon TS-E 135mm f/4L Macro".to_string(),
            94 => "Canon TS-E 17mm f/4L".to_string(),
            95 => "Canon TS-E 24mm f/3.5L II".to_string(),
            103 => "Samyang AF 14mm f/2.8 EF or Rokinon Lens".to_string(),
            106 => "Rokinon SP / Samyang XP 35mm f/1.2".to_string(),
            112 => "Sigma 28mm f/1.5 FF High-speed Prime or other Sigma Lens".to_string(),
            117 => "Tamron 35-150mm f/2.8-4.0 Di VC OSD (A043) or other Tamron Lens".to_string(),
            124 => "Canon MP-E 65mm f/2.8 1-5x Macro Photo".to_string(),
            125 => "Canon TS-E 24mm f/3.5L".to_string(),
            126 => "Canon TS-E 45mm f/2.8".to_string(),
            127 => "Canon TS-E 90mm f/2.8 or Tamron Lens".to_string(),
            129 => "Canon EF 300mm f/2.8L USM".to_string(),
            130 => "Canon EF 50mm f/1.0L USM".to_string(),
            131 => "Canon EF 28-80mm f/2.8-4L USM or Sigma Lens".to_string(),
            132 => "Canon EF 1200mm f/5.6L USM".to_string(),
            134 => "Canon EF 600mm f/4L IS USM".to_string(),
            135 => "Canon EF 200mm f/1.8L USM".to_string(),
            136 => "Canon EF 300mm f/2.8L USM".to_string(),
            137 => "Canon EF 85mm f/1.2L USM or Sigma or Tamron Lens".to_string(),
            138 => "Canon EF 28-80mm f/2.8-4L".to_string(),
            139 => "Canon EF 400mm f/2.8L USM".to_string(),
            140 => "Canon EF 500mm f/4.5L USM".to_string(),
            141 => "Canon EF 500mm f/4.5L USM".to_string(),
            142 => "Canon EF 300mm f/2.8L IS USM".to_string(),
            143 => "Canon EF 500mm f/4L IS USM or Sigma Lens".to_string(),
            144 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            145 => "Canon EF 100-300mm f/4.5-5.6 USM".to_string(),
            146 => "Canon EF 70-210mm f/3.5-4.5 USM".to_string(),
            147 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            148 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            149 => "Canon EF 100mm f/2 USM".to_string(),
            150 => "Canon EF 14mm f/2.8L USM or Sigma Lens".to_string(),
            151 => "Canon EF 200mm f/2.8L USM".to_string(),
            152 => "Canon EF 300mm f/4L IS USM or Sigma Lens".to_string(),
            153 => "Canon EF 35-350mm f/3.5-5.6L USM or Sigma or Tamron Lens".to_string(),
            154 => "Canon EF 20mm f/2.8 USM or Zeiss Lens".to_string(),
            155 => "Canon EF 85mm f/1.8 USM or Sigma Lens".to_string(),
            156 => "Canon EF 28-105mm f/3.5-4.5 USM or Tamron Lens".to_string(),
            160 => "Canon EF 20-35mm f/3.5-4.5 USM or Tamron or Tokina Lens".to_string(),
            161 => "Canon EF 28-70mm f/2.8L USM or Other Lens".to_string(),
            162 => "Canon EF 200mm f/2.8L USM".to_string(),
            163 => "Canon EF 300mm f/4L".to_string(),
            164 => "Canon EF 400mm f/5.6L".to_string(),
            165 => "Canon EF 70-200mm f/2.8L USM".to_string(),
            166 => "Canon EF 70-200mm f/2.8L USM + 1.4x".to_string(),
            167 => "Canon EF 70-200mm f/2.8L USM + 2x".to_string(),
            168 => "Canon EF 28mm f/1.8 USM or Sigma Lens".to_string(),
            169 => "Canon EF 17-35mm f/2.8L USM or Sigma Lens".to_string(),
            170 => "Canon EF 200mm f/2.8L II USM or Sigma Lens".to_string(),
            171 => "Canon EF 300mm f/4L USM".to_string(),
            172 => "Canon EF 400mm f/5.6L USM or Sigma Lens".to_string(),
            173 => "Canon EF 180mm Macro f/3.5L USM or Sigma Lens".to_string(),
            174 => "Canon EF 135mm f/2L USM or Other Lens".to_string(),
            175 => "Canon EF 400mm f/2.8L USM".to_string(),
            176 => "Canon EF 24-85mm f/3.5-4.5 USM".to_string(),
            177 => "Canon EF 300mm f/4L IS USM".to_string(),
            178 => "Canon EF 28-135mm f/3.5-5.6 IS".to_string(),
            179 => "Canon EF 24mm f/1.4L USM".to_string(),
            180 => "Canon EF 35mm f/1.4L USM or Other Lens".to_string(),
            181 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 1.4x or Sigma Lens".to_string(),
            182 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 2x or Sigma Lens".to_string(),
            183 => "Canon EF 100-400mm f/4.5-5.6L IS USM or Sigma Lens".to_string(),
            184 => "Canon EF 400mm f/2.8L USM + 2x".to_string(),
            185 => "Canon EF 600mm f/4L IS USM".to_string(),
            186 => "Canon EF 70-200mm f/4L USM".to_string(),
            187 => "Canon EF 70-200mm f/4L USM + 1.4x".to_string(),
            188 => "Canon EF 70-200mm f/4L USM + 2x".to_string(),
            189 => "Canon EF 70-200mm f/4L USM + 2.8x".to_string(),
            190 => "Canon EF 100mm f/2.8 Macro USM".to_string(),
            191 => "Canon EF 400mm f/4 DO IS or Sigma Lens".to_string(),
            193 => "Canon EF 35-80mm f/4-5.6 USM".to_string(),
            194 => "Canon EF 80-200mm f/4.5-5.6 USM".to_string(),
            195 => "Canon EF 35-105mm f/4.5-5.6 USM".to_string(),
            196 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            197 => "Canon EF 75-300mm f/4-5.6 IS USM or Sigma Lens".to_string(),
            198 => "Canon EF 50mm f/1.4 USM or Other Lens".to_string(),
            199 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            200 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            201 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            202 => "Canon EF 28-80mm f/3.5-5.6 USM IV".to_string(),
            208 => "Canon EF 22-55mm f/4-5.6 USM".to_string(),
            209 => "Canon EF 55-200mm f/4.5-5.6".to_string(),
            210 => "Canon EF 28-90mm f/4-5.6 USM".to_string(),
            211 => "Canon EF 28-200mm f/3.5-5.6 USM".to_string(),
            212 => "Canon EF 28-105mm f/4-5.6 USM".to_string(),
            213 => "Canon EF 90-300mm f/4.5-5.6 USM or Tamron Lens".to_string(),
            214 => "Canon EF-S 18-55mm f/3.5-5.6 USM".to_string(),
            215 => "Canon EF 55-200mm f/4.5-5.6 II USM".to_string(),
            217 => "Tamron AF 18-270mm f/3.5-6.3 Di II VC PZD".to_string(),
            220 => "Yongnuo YN 50mm f/1.8".to_string(),
            224 => "Canon EF 70-200mm f/2.8L IS USM".to_string(),
            225 => "Canon EF 70-200mm f/2.8L IS USM + 1.4x".to_string(),
            226 => "Canon EF 70-200mm f/2.8L IS USM + 2x".to_string(),
            227 => "Canon EF 70-200mm f/2.8L IS USM + 2.8x".to_string(),
            228 => "Canon EF 28-105mm f/3.5-4.5 USM".to_string(),
            229 => "Canon EF 16-35mm f/2.8L USM".to_string(),
            230 => "Canon EF 24-70mm f/2.8L USM".to_string(),
            231 => "Canon EF 17-40mm f/4L USM or Sigma Lens".to_string(),
            232 => "Canon EF 70-300mm f/4.5-5.6 DO IS USM".to_string(),
            233 => "Canon EF 28-300mm f/3.5-5.6L IS USM".to_string(),
            234 => "Canon EF-S 17-85mm f/4-5.6 IS USM or Tokina Lens".to_string(),
            235 => "Canon EF-S 10-22mm f/3.5-4.5 USM".to_string(),
            236 => "Canon EF-S 60mm f/2.8 Macro USM".to_string(),
            237 => "Canon EF 24-105mm f/4L IS USM".to_string(),
            238 => "Canon EF 70-300mm f/4-5.6 IS USM".to_string(),
            239 => "Canon EF 85mm f/1.2L II USM or Rokinon Lens".to_string(),
            240 => "Canon EF-S 17-55mm f/2.8 IS USM or Sigma Lens".to_string(),
            241 => "Canon EF 50mm f/1.2L USM".to_string(),
            242 => "Canon EF 70-200mm f/4L IS USM".to_string(),
            243 => "Canon EF 70-200mm f/4L IS USM + 1.4x".to_string(),
            244 => "Canon EF 70-200mm f/4L IS USM + 2x".to_string(),
            245 => "Canon EF 70-200mm f/4L IS USM + 2.8x".to_string(),
            246 => "Canon EF 16-35mm f/2.8L II USM".to_string(),
            247 => "Canon EF 14mm f/2.8L II USM".to_string(),
            248 => "Canon EF 200mm f/2L IS USM or Sigma Lens".to_string(),
            249 => "Canon EF 800mm f/5.6L IS USM".to_string(),
            250 => "Canon EF 24mm f/1.4L II USM or Sigma Lens".to_string(),
            251 => "Canon EF 70-200mm f/2.8L IS II USM".to_string(),
            252 => "Canon EF 70-200mm f/2.8L IS II USM + 1.4x".to_string(),
            253 => "Canon EF 70-200mm f/2.8L IS II USM + 2x".to_string(),
            254 => "Canon EF 100mm f/2.8L Macro IS USM or Tamron Lens".to_string(),
            255 => "Sigma 24-105mm f/4 DG OS HSM | A or Other Lens".to_string(),
            368 => "Sigma 14-24mm f/2.8 DG HSM | A or other Sigma Lens".to_string(),
            488 => "Canon EF-S 15-85mm f/3.5-5.6 IS USM".to_string(),
            489 => "Canon EF 70-300mm f/4-5.6L IS USM".to_string(),
            490 => "Canon EF 8-15mm f/4L Fisheye USM".to_string(),
            491 => "Canon EF 300mm f/2.8L IS II USM or Tamron Lens".to_string(),
            492 => "Canon EF 400mm f/2.8L IS II USM".to_string(),
            493 => "Canon EF 500mm f/4L IS II USM or EF 24-105mm f4L IS USM".to_string(),
            494 => "Canon EF 600mm f/4L IS II USM".to_string(),
            495 => "Canon EF 24-70mm f/2.8L II USM or Sigma Lens".to_string(),
            496 => "Canon EF 200-400mm f/4L IS USM".to_string(),
            499 => "Canon EF 200-400mm f/4L IS USM + 1.4x".to_string(),
            502 => "Canon EF 28mm f/2.8 IS USM or Tamron Lens".to_string(),
            503 => "Canon EF 24mm f/2.8 IS USM".to_string(),
            504 => "Canon EF 24-70mm f/4L IS USM".to_string(),
            505 => "Canon EF 35mm f/2 IS USM".to_string(),
            506 => "Canon EF 400mm f/4 DO IS II USM".to_string(),
            507 => "Canon EF 16-35mm f/4L IS USM".to_string(),
            508 => "Canon EF 11-24mm f/4L USM or Tamron Lens".to_string(),
            624 => "Sigma 70-200mm f/2.8 DG OS HSM | S or other Sigma Lens".to_string(),
            747 => "Canon EF 100-400mm f/4.5-5.6L IS II USM or Tamron Lens".to_string(),
            748 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 1.4x or Tamron Lens".to_string(),
            749 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 2x or Tamron Lens".to_string(),
            750 => "Canon EF 35mm f/1.4L II USM or Tamron Lens".to_string(),
            751 => "Canon EF 16-35mm f/2.8L III USM".to_string(),
            752 => "Canon EF 24-105mm f/4L IS II USM".to_string(),
            753 => "Canon EF 85mm f/1.4L IS USM".to_string(),
            754 => "Canon EF 70-200mm f/4L IS II USM".to_string(),
            757 => "Canon EF 400mm f/2.8L IS III USM".to_string(),
            758 => "Canon EF 600mm f/4L IS III USM".to_string(),
            923 => "Meike/SKY 85mm f/1.8 DCM".to_string(),
            1136 => "Sigma 24-70mm f/2.8 DG OS HSM | A".to_string(),
            4142 => "Canon EF-S 18-135mm f/3.5-5.6 IS STM".to_string(),
            4143 => "Canon EF-M 18-55mm f/3.5-5.6 IS STM or Tamron Lens".to_string(),
            4144 => "Canon EF 40mm f/2.8 STM".to_string(),
            4145 => "Canon EF-M 22mm f/2 STM".to_string(),
            4146 => "Canon EF-S 18-55mm f/3.5-5.6 IS STM".to_string(),
            4147 => "Canon EF-M 11-22mm f/4-5.6 IS STM".to_string(),
            4148 => "Canon EF-S 55-250mm f/4-5.6 IS STM".to_string(),
            4149 => "Canon EF-M 55-200mm f/4.5-6.3 IS STM".to_string(),
            4150 => "Canon EF-S 10-18mm f/4.5-5.6 IS STM".to_string(),
            4152 => "Canon EF 24-105mm f/3.5-5.6 IS STM".to_string(),
            4153 => "Canon EF-M 15-45mm f/3.5-6.3 IS STM".to_string(),
            4154 => "Canon EF-S 24mm f/2.8 STM".to_string(),
            4155 => "Canon EF-M 28mm f/3.5 Macro IS STM".to_string(),
            4156 => "Canon EF 50mm f/1.8 STM".to_string(),
            4157 => "Canon EF-M 18-150mm f/3.5-6.3 IS STM".to_string(),
            4158 => "Canon EF-S 18-55mm f/4-5.6 IS STM".to_string(),
            4159 => "Canon EF-M 32mm f/1.4 STM".to_string(),
            4160 => "Canon EF-S 35mm f/2.8 Macro IS STM".to_string(),
            4208 => "Sigma 56mm f/1.4 DC DN | C or other Sigma Lens".to_string(),
            4976 => "Sigma 16-300mm F3.5-6.7 DC OS | C (025)".to_string(),
            6512 => "Sigma 12mm F1.4 DC | C".to_string(),
            36910 => "Canon EF 70-300mm f/4-5.6 IS II USM".to_string(),
            36912 => "Canon EF-S 18-135mm f/3.5-5.6 IS USM".to_string(),
            61182 => "Canon RF 50mm F1.2L USM or other Canon RF Lens".to_string(),
            61491 => "Canon CN-E 14mm T3.1 L F".to_string(),
            61492 => "Canon CN-E 24mm T1.5 L F".to_string(),
            61494 => "Canon CN-E 85mm T1.3 L F".to_string(),
            61495 => "Canon CN-E 135mm T2.2 L F".to_string(),
            61496 => "Canon CN-E 35mm T1.5 L F".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("LensType", 0x189, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x18b, bo) {
        dm.push(("MinFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MinFocalLength", 0x18b, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x18d, bo) {
        dm.push(("MaxFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MaxFocalLength", 0x18d, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(text) = text_at(data, 0x45a, 6, true) {
        tags.push(mk("FirmwareVersion", 0x45a, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x4ae, bo) {
        dm.push(("FileIndex".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val + 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("FileIndex", 0x4ae, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x4ba, bo) {
        dm.push(("DirectoryIndex".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("DirectoryIndex", 0x4ba, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    tags
}

/// `Image::ExifTool::Canon::CameraInfo450D` -- FORMAT int8u, FIRST_ENTRY 0.
fn canon_camerainfo450d(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = u8_at(data, 0x3) {
        dm.push(("FNumber".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(($val-8)/16*log(2))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("sprintf(\"%.2g\",$val)", &cv, &ctx) { cv = x; }
            tags.push(mk("FNumber", 0x3, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x4) {
        dm.push(("ExposureTime".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(4*log(2)*(1-Image::ExifTool::Canon::CanonEv($val-24)))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("Image::ExifTool::Exif::PrintExposureTime($val)", &cv, &ctx) { cv = x; }
            tags.push(mk("ExposureTime", 0x4, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x6) {
        dm.push(("ISO".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("100*exp(($val/8-9)*log(2))", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("sprintf(\"%.0f\",$val)", &cv, &ctx) { cv = x; }
        tags.push(mk("ISO", 0x6, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x15) {
        dm.push(("FlashMeteringMode".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "E-TTL".to_string(),
            3 => "TTL".to_string(),
            4 => "External Auto".to_string(),
            5 => "External Manual".to_string(),
            6 => "Off".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("FlashMeteringMode", 0x15, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x18) {
        dm.push(("CameraTemperature".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 128", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
        tags.push(mk("CameraTemperature", 0x18, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if (dm_get(dm, "LensType").is_some_and(|v| v != 0.0) && dm_get(dm, "LensType").is_some_and(|v| v == 124.0)) {
        if let Some(v) = u8_at(data, 0x1b) {
            dm.push(("MacroMagnification".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp((75-$val) * log(2) * 3 / 40)", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("sprintf(\"%.1fx\",$val)", &cv, &ctx) { cv = x; }
            tags.push(mk("MacroMagnification", 0x1b, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u16rev_at(data, 0x1d, bo) {
        dm.push(("FocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
            tags.push(mk("FocalLength", 0x1d, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x30) {
        dm.push(("CameraOrientation".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Horizontal (normal)".to_string(),
            1 => "Rotate 90 CW".to_string(),
            2 => "Rotate 270 CW".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("CameraOrientation", 0x30, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x43, bo) {
        dm.push(("FocusDistanceUpper".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceUpper", 0x43, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x45, bo) {
        dm.push(("FocusDistanceLower".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceLower", 0x45, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x6f, bo) {
        dm.push(("WhiteBalance".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Auto".to_string(),
            1 => "Daylight".to_string(),
            2 => "Cloudy".to_string(),
            3 => "Tungsten".to_string(),
            4 => "Fluorescent".to_string(),
            5 => "Flash".to_string(),
            6 => "Custom".to_string(),
            7 => "Black & White".to_string(),
            8 => "Shade".to_string(),
            9 => "Manual Temperature (Kelvin)".to_string(),
            10 => "PC Set1".to_string(),
            11 => "PC Set2".to_string(),
            12 => "PC Set3".to_string(),
            14 => "Daylight Fluorescent".to_string(),
            15 => "Custom 1".to_string(),
            16 => "Custom 2".to_string(),
            17 => "Underwater".to_string(),
            18 => "Custom 3".to_string(),
            19 => "Custom 4".to_string(),
            20 => "PC Set4".to_string(),
            21 => "PC Set5".to_string(),
            23 => "Auto (ambience priority)".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("WhiteBalance", 0x6f, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x73, bo) {
        dm.push(("ColorTemperature".to_string(), f64::from(v)));
        tags.push(mk("ColorTemperature", 0x73, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0xde, bo) {
        dm.push(("LensType".to_string(), f64::from(v)));
        let s = match v as i64 {
            -1 => "n/a".to_string(),
            1 => "Canon EF 50mm f/1.8".to_string(),
            2 => "Canon EF 28mm f/2.8 or Sigma Lens".to_string(),
            3 => "Canon EF 135mm f/2.8 Soft".to_string(),
            4 => "Canon EF 35-105mm f/3.5-4.5 or Sigma Lens".to_string(),
            5 => "Canon EF 35-70mm f/3.5-4.5".to_string(),
            6 => "Canon EF 28-70mm f/3.5-4.5 or Sigma or Tokina Lens".to_string(),
            7 => "Canon EF 100-300mm f/5.6L".to_string(),
            8 => "Canon EF 100-300mm f/5.6 or Sigma or Tokina Lens".to_string(),
            9 => "Canon EF 70-210mm f/4".to_string(),
            10 => "Canon EF 50mm f/2.5 Macro or Sigma Lens".to_string(),
            11 => "Canon EF 35mm f/2".to_string(),
            13 => "Canon EF 15mm f/2.8 Fisheye".to_string(),
            14 => "Canon EF 50-200mm f/3.5-4.5L".to_string(),
            15 => "Canon EF 50-200mm f/3.5-4.5".to_string(),
            16 => "Canon EF 35-135mm f/3.5-4.5".to_string(),
            17 => "Canon EF 35-70mm f/3.5-4.5A".to_string(),
            18 => "Canon EF 28-70mm f/3.5-4.5".to_string(),
            20 => "Canon EF 100-200mm f/4.5A".to_string(),
            21 => "Canon EF 80-200mm f/2.8L".to_string(),
            22 => "Canon EF 20-35mm f/2.8L or Tokina Lens".to_string(),
            23 => "Canon EF 35-105mm f/3.5-4.5".to_string(),
            24 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            25 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            26 => "Canon EF 100mm f/2.8 Macro or Other Lens".to_string(),
            27 => "Canon EF 35-80mm f/4-5.6".to_string(),
            28 => "Canon EF 80-200mm f/4.5-5.6 or Tamron Lens".to_string(),
            29 => "Canon EF 50mm f/1.8 II".to_string(),
            30 => "Canon EF 35-105mm f/4.5-5.6".to_string(),
            31 => "Canon EF 75-300mm f/4-5.6 or Tamron Lens".to_string(),
            32 => "Canon EF 24mm f/2.8 or Sigma Lens".to_string(),
            33 => "Voigtlander or Carl Zeiss Lens".to_string(),
            35 => "Canon EF 35-80mm f/4-5.6".to_string(),
            36 => "Canon EF 38-76mm f/4.5-5.6".to_string(),
            37 => "Canon EF 35-80mm f/4-5.6 or Tamron Lens".to_string(),
            38 => "Canon EF 80-200mm f/4.5-5.6 II".to_string(),
            39 => "Canon EF 75-300mm f/4-5.6".to_string(),
            40 => "Canon EF 28-80mm f/3.5-5.6".to_string(),
            41 => "Canon EF 28-90mm f/4-5.6".to_string(),
            42 => "Canon EF 28-200mm f/3.5-5.6 or Tamron Lens".to_string(),
            43 => "Canon EF 28-105mm f/4-5.6".to_string(),
            44 => "Canon EF 90-300mm f/4.5-5.6".to_string(),
            45 => "Canon EF-S 18-55mm f/3.5-5.6 [II]".to_string(),
            46 => "Canon EF 28-90mm f/4-5.6".to_string(),
            47 => "Zeiss Milvus 35mm f/2 or 50mm f/2".to_string(),
            48 => "Canon EF-S 18-55mm f/3.5-5.6 IS".to_string(),
            49 => "Canon EF-S 55-250mm f/4-5.6 IS".to_string(),
            50 => "Canon EF-S 18-200mm f/3.5-5.6 IS".to_string(),
            51 => "Canon EF-S 18-135mm f/3.5-5.6 IS".to_string(),
            52 => "Canon EF-S 18-55mm f/3.5-5.6 IS II".to_string(),
            53 => "Canon EF-S 18-55mm f/3.5-5.6 III".to_string(),
            54 => "Canon EF-S 55-250mm f/4-5.6 IS II".to_string(),
            60 => "Irix 11mm f/4 or 15mm f/2.4".to_string(),
            63 => "Irix 30mm F1.4 Dragonfly".to_string(),
            80 => "Canon TS-E 50mm f/2.8L Macro".to_string(),
            81 => "Canon TS-E 90mm f/2.8L Macro".to_string(),
            82 => "Canon TS-E 135mm f/4L Macro".to_string(),
            94 => "Canon TS-E 17mm f/4L".to_string(),
            95 => "Canon TS-E 24mm f/3.5L II".to_string(),
            103 => "Samyang AF 14mm f/2.8 EF or Rokinon Lens".to_string(),
            106 => "Rokinon SP / Samyang XP 35mm f/1.2".to_string(),
            112 => "Sigma 28mm f/1.5 FF High-speed Prime or other Sigma Lens".to_string(),
            117 => "Tamron 35-150mm f/2.8-4.0 Di VC OSD (A043) or other Tamron Lens".to_string(),
            124 => "Canon MP-E 65mm f/2.8 1-5x Macro Photo".to_string(),
            125 => "Canon TS-E 24mm f/3.5L".to_string(),
            126 => "Canon TS-E 45mm f/2.8".to_string(),
            127 => "Canon TS-E 90mm f/2.8 or Tamron Lens".to_string(),
            129 => "Canon EF 300mm f/2.8L USM".to_string(),
            130 => "Canon EF 50mm f/1.0L USM".to_string(),
            131 => "Canon EF 28-80mm f/2.8-4L USM or Sigma Lens".to_string(),
            132 => "Canon EF 1200mm f/5.6L USM".to_string(),
            134 => "Canon EF 600mm f/4L IS USM".to_string(),
            135 => "Canon EF 200mm f/1.8L USM".to_string(),
            136 => "Canon EF 300mm f/2.8L USM".to_string(),
            137 => "Canon EF 85mm f/1.2L USM or Sigma or Tamron Lens".to_string(),
            138 => "Canon EF 28-80mm f/2.8-4L".to_string(),
            139 => "Canon EF 400mm f/2.8L USM".to_string(),
            140 => "Canon EF 500mm f/4.5L USM".to_string(),
            141 => "Canon EF 500mm f/4.5L USM".to_string(),
            142 => "Canon EF 300mm f/2.8L IS USM".to_string(),
            143 => "Canon EF 500mm f/4L IS USM or Sigma Lens".to_string(),
            144 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            145 => "Canon EF 100-300mm f/4.5-5.6 USM".to_string(),
            146 => "Canon EF 70-210mm f/3.5-4.5 USM".to_string(),
            147 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            148 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            149 => "Canon EF 100mm f/2 USM".to_string(),
            150 => "Canon EF 14mm f/2.8L USM or Sigma Lens".to_string(),
            151 => "Canon EF 200mm f/2.8L USM".to_string(),
            152 => "Canon EF 300mm f/4L IS USM or Sigma Lens".to_string(),
            153 => "Canon EF 35-350mm f/3.5-5.6L USM or Sigma or Tamron Lens".to_string(),
            154 => "Canon EF 20mm f/2.8 USM or Zeiss Lens".to_string(),
            155 => "Canon EF 85mm f/1.8 USM or Sigma Lens".to_string(),
            156 => "Canon EF 28-105mm f/3.5-4.5 USM or Tamron Lens".to_string(),
            160 => "Canon EF 20-35mm f/3.5-4.5 USM or Tamron or Tokina Lens".to_string(),
            161 => "Canon EF 28-70mm f/2.8L USM or Other Lens".to_string(),
            162 => "Canon EF 200mm f/2.8L USM".to_string(),
            163 => "Canon EF 300mm f/4L".to_string(),
            164 => "Canon EF 400mm f/5.6L".to_string(),
            165 => "Canon EF 70-200mm f/2.8L USM".to_string(),
            166 => "Canon EF 70-200mm f/2.8L USM + 1.4x".to_string(),
            167 => "Canon EF 70-200mm f/2.8L USM + 2x".to_string(),
            168 => "Canon EF 28mm f/1.8 USM or Sigma Lens".to_string(),
            169 => "Canon EF 17-35mm f/2.8L USM or Sigma Lens".to_string(),
            170 => "Canon EF 200mm f/2.8L II USM or Sigma Lens".to_string(),
            171 => "Canon EF 300mm f/4L USM".to_string(),
            172 => "Canon EF 400mm f/5.6L USM or Sigma Lens".to_string(),
            173 => "Canon EF 180mm Macro f/3.5L USM or Sigma Lens".to_string(),
            174 => "Canon EF 135mm f/2L USM or Other Lens".to_string(),
            175 => "Canon EF 400mm f/2.8L USM".to_string(),
            176 => "Canon EF 24-85mm f/3.5-4.5 USM".to_string(),
            177 => "Canon EF 300mm f/4L IS USM".to_string(),
            178 => "Canon EF 28-135mm f/3.5-5.6 IS".to_string(),
            179 => "Canon EF 24mm f/1.4L USM".to_string(),
            180 => "Canon EF 35mm f/1.4L USM or Other Lens".to_string(),
            181 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 1.4x or Sigma Lens".to_string(),
            182 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 2x or Sigma Lens".to_string(),
            183 => "Canon EF 100-400mm f/4.5-5.6L IS USM or Sigma Lens".to_string(),
            184 => "Canon EF 400mm f/2.8L USM + 2x".to_string(),
            185 => "Canon EF 600mm f/4L IS USM".to_string(),
            186 => "Canon EF 70-200mm f/4L USM".to_string(),
            187 => "Canon EF 70-200mm f/4L USM + 1.4x".to_string(),
            188 => "Canon EF 70-200mm f/4L USM + 2x".to_string(),
            189 => "Canon EF 70-200mm f/4L USM + 2.8x".to_string(),
            190 => "Canon EF 100mm f/2.8 Macro USM".to_string(),
            191 => "Canon EF 400mm f/4 DO IS or Sigma Lens".to_string(),
            193 => "Canon EF 35-80mm f/4-5.6 USM".to_string(),
            194 => "Canon EF 80-200mm f/4.5-5.6 USM".to_string(),
            195 => "Canon EF 35-105mm f/4.5-5.6 USM".to_string(),
            196 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            197 => "Canon EF 75-300mm f/4-5.6 IS USM or Sigma Lens".to_string(),
            198 => "Canon EF 50mm f/1.4 USM or Other Lens".to_string(),
            199 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            200 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            201 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            202 => "Canon EF 28-80mm f/3.5-5.6 USM IV".to_string(),
            208 => "Canon EF 22-55mm f/4-5.6 USM".to_string(),
            209 => "Canon EF 55-200mm f/4.5-5.6".to_string(),
            210 => "Canon EF 28-90mm f/4-5.6 USM".to_string(),
            211 => "Canon EF 28-200mm f/3.5-5.6 USM".to_string(),
            212 => "Canon EF 28-105mm f/4-5.6 USM".to_string(),
            213 => "Canon EF 90-300mm f/4.5-5.6 USM or Tamron Lens".to_string(),
            214 => "Canon EF-S 18-55mm f/3.5-5.6 USM".to_string(),
            215 => "Canon EF 55-200mm f/4.5-5.6 II USM".to_string(),
            217 => "Tamron AF 18-270mm f/3.5-6.3 Di II VC PZD".to_string(),
            220 => "Yongnuo YN 50mm f/1.8".to_string(),
            224 => "Canon EF 70-200mm f/2.8L IS USM".to_string(),
            225 => "Canon EF 70-200mm f/2.8L IS USM + 1.4x".to_string(),
            226 => "Canon EF 70-200mm f/2.8L IS USM + 2x".to_string(),
            227 => "Canon EF 70-200mm f/2.8L IS USM + 2.8x".to_string(),
            228 => "Canon EF 28-105mm f/3.5-4.5 USM".to_string(),
            229 => "Canon EF 16-35mm f/2.8L USM".to_string(),
            230 => "Canon EF 24-70mm f/2.8L USM".to_string(),
            231 => "Canon EF 17-40mm f/4L USM or Sigma Lens".to_string(),
            232 => "Canon EF 70-300mm f/4.5-5.6 DO IS USM".to_string(),
            233 => "Canon EF 28-300mm f/3.5-5.6L IS USM".to_string(),
            234 => "Canon EF-S 17-85mm f/4-5.6 IS USM or Tokina Lens".to_string(),
            235 => "Canon EF-S 10-22mm f/3.5-4.5 USM".to_string(),
            236 => "Canon EF-S 60mm f/2.8 Macro USM".to_string(),
            237 => "Canon EF 24-105mm f/4L IS USM".to_string(),
            238 => "Canon EF 70-300mm f/4-5.6 IS USM".to_string(),
            239 => "Canon EF 85mm f/1.2L II USM or Rokinon Lens".to_string(),
            240 => "Canon EF-S 17-55mm f/2.8 IS USM or Sigma Lens".to_string(),
            241 => "Canon EF 50mm f/1.2L USM".to_string(),
            242 => "Canon EF 70-200mm f/4L IS USM".to_string(),
            243 => "Canon EF 70-200mm f/4L IS USM + 1.4x".to_string(),
            244 => "Canon EF 70-200mm f/4L IS USM + 2x".to_string(),
            245 => "Canon EF 70-200mm f/4L IS USM + 2.8x".to_string(),
            246 => "Canon EF 16-35mm f/2.8L II USM".to_string(),
            247 => "Canon EF 14mm f/2.8L II USM".to_string(),
            248 => "Canon EF 200mm f/2L IS USM or Sigma Lens".to_string(),
            249 => "Canon EF 800mm f/5.6L IS USM".to_string(),
            250 => "Canon EF 24mm f/1.4L II USM or Sigma Lens".to_string(),
            251 => "Canon EF 70-200mm f/2.8L IS II USM".to_string(),
            252 => "Canon EF 70-200mm f/2.8L IS II USM + 1.4x".to_string(),
            253 => "Canon EF 70-200mm f/2.8L IS II USM + 2x".to_string(),
            254 => "Canon EF 100mm f/2.8L Macro IS USM or Tamron Lens".to_string(),
            255 => "Sigma 24-105mm f/4 DG OS HSM | A or Other Lens".to_string(),
            368 => "Sigma 14-24mm f/2.8 DG HSM | A or other Sigma Lens".to_string(),
            488 => "Canon EF-S 15-85mm f/3.5-5.6 IS USM".to_string(),
            489 => "Canon EF 70-300mm f/4-5.6L IS USM".to_string(),
            490 => "Canon EF 8-15mm f/4L Fisheye USM".to_string(),
            491 => "Canon EF 300mm f/2.8L IS II USM or Tamron Lens".to_string(),
            492 => "Canon EF 400mm f/2.8L IS II USM".to_string(),
            493 => "Canon EF 500mm f/4L IS II USM or EF 24-105mm f4L IS USM".to_string(),
            494 => "Canon EF 600mm f/4L IS II USM".to_string(),
            495 => "Canon EF 24-70mm f/2.8L II USM or Sigma Lens".to_string(),
            496 => "Canon EF 200-400mm f/4L IS USM".to_string(),
            499 => "Canon EF 200-400mm f/4L IS USM + 1.4x".to_string(),
            502 => "Canon EF 28mm f/2.8 IS USM or Tamron Lens".to_string(),
            503 => "Canon EF 24mm f/2.8 IS USM".to_string(),
            504 => "Canon EF 24-70mm f/4L IS USM".to_string(),
            505 => "Canon EF 35mm f/2 IS USM".to_string(),
            506 => "Canon EF 400mm f/4 DO IS II USM".to_string(),
            507 => "Canon EF 16-35mm f/4L IS USM".to_string(),
            508 => "Canon EF 11-24mm f/4L USM or Tamron Lens".to_string(),
            624 => "Sigma 70-200mm f/2.8 DG OS HSM | S or other Sigma Lens".to_string(),
            747 => "Canon EF 100-400mm f/4.5-5.6L IS II USM or Tamron Lens".to_string(),
            748 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 1.4x or Tamron Lens".to_string(),
            749 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 2x or Tamron Lens".to_string(),
            750 => "Canon EF 35mm f/1.4L II USM or Tamron Lens".to_string(),
            751 => "Canon EF 16-35mm f/2.8L III USM".to_string(),
            752 => "Canon EF 24-105mm f/4L IS II USM".to_string(),
            753 => "Canon EF 85mm f/1.4L IS USM".to_string(),
            754 => "Canon EF 70-200mm f/4L IS II USM".to_string(),
            757 => "Canon EF 400mm f/2.8L IS III USM".to_string(),
            758 => "Canon EF 600mm f/4L IS III USM".to_string(),
            923 => "Meike/SKY 85mm f/1.8 DCM".to_string(),
            1136 => "Sigma 24-70mm f/2.8 DG OS HSM | A".to_string(),
            4142 => "Canon EF-S 18-135mm f/3.5-5.6 IS STM".to_string(),
            4143 => "Canon EF-M 18-55mm f/3.5-5.6 IS STM or Tamron Lens".to_string(),
            4144 => "Canon EF 40mm f/2.8 STM".to_string(),
            4145 => "Canon EF-M 22mm f/2 STM".to_string(),
            4146 => "Canon EF-S 18-55mm f/3.5-5.6 IS STM".to_string(),
            4147 => "Canon EF-M 11-22mm f/4-5.6 IS STM".to_string(),
            4148 => "Canon EF-S 55-250mm f/4-5.6 IS STM".to_string(),
            4149 => "Canon EF-M 55-200mm f/4.5-6.3 IS STM".to_string(),
            4150 => "Canon EF-S 10-18mm f/4.5-5.6 IS STM".to_string(),
            4152 => "Canon EF 24-105mm f/3.5-5.6 IS STM".to_string(),
            4153 => "Canon EF-M 15-45mm f/3.5-6.3 IS STM".to_string(),
            4154 => "Canon EF-S 24mm f/2.8 STM".to_string(),
            4155 => "Canon EF-M 28mm f/3.5 Macro IS STM".to_string(),
            4156 => "Canon EF 50mm f/1.8 STM".to_string(),
            4157 => "Canon EF-M 18-150mm f/3.5-6.3 IS STM".to_string(),
            4158 => "Canon EF-S 18-55mm f/4-5.6 IS STM".to_string(),
            4159 => "Canon EF-M 32mm f/1.4 STM".to_string(),
            4160 => "Canon EF-S 35mm f/2.8 Macro IS STM".to_string(),
            4208 => "Sigma 56mm f/1.4 DC DN | C or other Sigma Lens".to_string(),
            4976 => "Sigma 16-300mm F3.5-6.7 DC OS | C (025)".to_string(),
            6512 => "Sigma 12mm F1.4 DC | C".to_string(),
            36910 => "Canon EF 70-300mm f/4-5.6 IS II USM".to_string(),
            36912 => "Canon EF-S 18-135mm f/3.5-5.6 IS USM".to_string(),
            61182 => "Canon RF 50mm F1.2L USM or other Canon RF Lens".to_string(),
            61491 => "Canon CN-E 14mm T3.1 L F".to_string(),
            61492 => "Canon CN-E 24mm T1.5 L F".to_string(),
            61494 => "Canon CN-E 85mm T1.3 L F".to_string(),
            61495 => "Canon CN-E 135mm T2.2 L F".to_string(),
            61496 => "Canon CN-E 35mm T1.5 L F".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("LensType", 0xde, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(text) = text_at(data, 0x107, 6, true) {
        tags.push(mk("FirmwareVersion", 0x107, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    if let Some(text) = text_at(data, 0x10f, 32, true) {
        tags.push(mk("OwnerName", 0x10f, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x133, bo) {
        dm.push(("DirectoryIndex".to_string(), f64::from(v)));
        tags.push(mk("DirectoryIndex", 0x133, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x13f, bo) {
        dm.push(("FileIndex".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val + 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("FileIndex", 0x13f, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(text) = text_at(data, 0x933, 64, true) {
        tags.push(mk("LensModel", 0x933, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    if let Some(sub) = data.get(0x263..) {
        tags.extend(canon_psinfo(sub, model, bo, file_type, format, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::CameraInfo500D` -- FORMAT int8u, FIRST_ENTRY 0.
fn canon_camerainfo500d(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = u8_at(data, 0x3) {
        dm.push(("FNumber".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(($val-8)/16*log(2))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("sprintf(\"%.2g\",$val)", &cv, &ctx) { cv = x; }
            tags.push(mk("FNumber", 0x3, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x4) {
        dm.push(("ExposureTime".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(4*log(2)*(1-Image::ExifTool::Canon::CanonEv($val-24)))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("Image::ExifTool::Exif::PrintExposureTime($val)", &cv, &ctx) { cv = x; }
            tags.push(mk("ExposureTime", 0x4, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x6) {
        dm.push(("ISO".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("100*exp(($val/8-9)*log(2))", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("sprintf(\"%.0f\",$val)", &cv, &ctx) { cv = x; }
        tags.push(mk("ISO", 0x6, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x7) {
        dm.push(("HighlightTonePriority".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "People".to_string(),
            1 => "sRGB".to_string(),
            2 => "Adobe RGB".to_string(),
            3 => "User 1".to_string(),
            4 => "User 2".to_string(),
            5 => "User 3".to_string(),
            6 => "To Do".to_string(),
            65535 => "n/a".to_string(),
            2415919104 => "Format 1".to_string(),
            2684354560 => "Format 2".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("HighlightTonePriority", 0x7, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x15) {
        dm.push(("FlashMeteringMode".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "E-TTL".to_string(),
            3 => "TTL".to_string(),
            4 => "External Auto".to_string(),
            5 => "External Manual".to_string(),
            6 => "Off".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("FlashMeteringMode", 0x15, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x19) {
        dm.push(("CameraTemperature".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 128", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
        tags.push(mk("CameraTemperature", 0x19, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x1e, bo) {
        dm.push(("FocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
            tags.push(mk("FocalLength", 0x1e, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x31) {
        dm.push(("CameraOrientation".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Horizontal (normal)".to_string(),
            1 => "Rotate 90 CW".to_string(),
            2 => "Rotate 270 CW".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("CameraOrientation", 0x31, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x50, bo) {
        dm.push(("FocusDistanceUpper".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceUpper", 0x50, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x52, bo) {
        dm.push(("FocusDistanceLower".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceLower", 0x52, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x73, bo) {
        dm.push(("WhiteBalance".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Auto".to_string(),
            1 => "Daylight".to_string(),
            2 => "Cloudy".to_string(),
            3 => "Tungsten".to_string(),
            4 => "Fluorescent".to_string(),
            5 => "Flash".to_string(),
            6 => "Custom".to_string(),
            7 => "Black & White".to_string(),
            8 => "Shade".to_string(),
            9 => "Manual Temperature (Kelvin)".to_string(),
            10 => "PC Set1".to_string(),
            11 => "PC Set2".to_string(),
            12 => "PC Set3".to_string(),
            14 => "Daylight Fluorescent".to_string(),
            15 => "Custom 1".to_string(),
            16 => "Custom 2".to_string(),
            17 => "Underwater".to_string(),
            18 => "Custom 3".to_string(),
            19 => "Custom 4".to_string(),
            20 => "PC Set4".to_string(),
            21 => "PC Set5".to_string(),
            23 => "Auto (ambience priority)".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("WhiteBalance", 0x73, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x77, bo) {
        dm.push(("ColorTemperature".to_string(), f64::from(v)));
        tags.push(mk("ColorTemperature", 0x77, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0xab) {
        dm.push(("PictureStyle".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "None".to_string(),
            1 => "Standard".to_string(),
            2 => "Portrait".to_string(),
            3 => "High Saturation".to_string(),
            4 => "Adobe RGB".to_string(),
            5 => "Low Saturation".to_string(),
            6 => "CM Set 1".to_string(),
            7 => "CM Set 2".to_string(),
            33 => "User Def. 1".to_string(),
            34 => "User Def. 2".to_string(),
            35 => "User Def. 3".to_string(),
            65 => "PC 1".to_string(),
            66 => "PC 2".to_string(),
            67 => "PC 3".to_string(),
            129 => "Standard".to_string(),
            130 => "Portrait".to_string(),
            131 => "Landscape".to_string(),
            132 => "Neutral".to_string(),
            133 => "Faithful".to_string(),
            134 => "Monochrome".to_string(),
            135 => "Auto".to_string(),
            136 => "Fine Detail".to_string(),
            255 => "n/a".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("PictureStyle", 0xab, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0xbc) {
        dm.push(("HighISONoiseReduction".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Standard".to_string(),
            1 => "Low".to_string(),
            2 => "Strong".to_string(),
            3 => "Off".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("HighISONoiseReduction", 0xbc, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0xbe) {
        dm.push(("AutoLightingOptimizer".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Standard".to_string(),
            1 => "Low".to_string(),
            2 => "Strong".to_string(),
            3 => "Off".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("AutoLightingOptimizer", 0xbe, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0xf6, bo) {
        dm.push(("LensType".to_string(), f64::from(v)));
        let s = match v as i64 {
            -1 => "n/a".to_string(),
            1 => "Canon EF 50mm f/1.8".to_string(),
            2 => "Canon EF 28mm f/2.8 or Sigma Lens".to_string(),
            3 => "Canon EF 135mm f/2.8 Soft".to_string(),
            4 => "Canon EF 35-105mm f/3.5-4.5 or Sigma Lens".to_string(),
            5 => "Canon EF 35-70mm f/3.5-4.5".to_string(),
            6 => "Canon EF 28-70mm f/3.5-4.5 or Sigma or Tokina Lens".to_string(),
            7 => "Canon EF 100-300mm f/5.6L".to_string(),
            8 => "Canon EF 100-300mm f/5.6 or Sigma or Tokina Lens".to_string(),
            9 => "Canon EF 70-210mm f/4".to_string(),
            10 => "Canon EF 50mm f/2.5 Macro or Sigma Lens".to_string(),
            11 => "Canon EF 35mm f/2".to_string(),
            13 => "Canon EF 15mm f/2.8 Fisheye".to_string(),
            14 => "Canon EF 50-200mm f/3.5-4.5L".to_string(),
            15 => "Canon EF 50-200mm f/3.5-4.5".to_string(),
            16 => "Canon EF 35-135mm f/3.5-4.5".to_string(),
            17 => "Canon EF 35-70mm f/3.5-4.5A".to_string(),
            18 => "Canon EF 28-70mm f/3.5-4.5".to_string(),
            20 => "Canon EF 100-200mm f/4.5A".to_string(),
            21 => "Canon EF 80-200mm f/2.8L".to_string(),
            22 => "Canon EF 20-35mm f/2.8L or Tokina Lens".to_string(),
            23 => "Canon EF 35-105mm f/3.5-4.5".to_string(),
            24 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            25 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            26 => "Canon EF 100mm f/2.8 Macro or Other Lens".to_string(),
            27 => "Canon EF 35-80mm f/4-5.6".to_string(),
            28 => "Canon EF 80-200mm f/4.5-5.6 or Tamron Lens".to_string(),
            29 => "Canon EF 50mm f/1.8 II".to_string(),
            30 => "Canon EF 35-105mm f/4.5-5.6".to_string(),
            31 => "Canon EF 75-300mm f/4-5.6 or Tamron Lens".to_string(),
            32 => "Canon EF 24mm f/2.8 or Sigma Lens".to_string(),
            33 => "Voigtlander or Carl Zeiss Lens".to_string(),
            35 => "Canon EF 35-80mm f/4-5.6".to_string(),
            36 => "Canon EF 38-76mm f/4.5-5.6".to_string(),
            37 => "Canon EF 35-80mm f/4-5.6 or Tamron Lens".to_string(),
            38 => "Canon EF 80-200mm f/4.5-5.6 II".to_string(),
            39 => "Canon EF 75-300mm f/4-5.6".to_string(),
            40 => "Canon EF 28-80mm f/3.5-5.6".to_string(),
            41 => "Canon EF 28-90mm f/4-5.6".to_string(),
            42 => "Canon EF 28-200mm f/3.5-5.6 or Tamron Lens".to_string(),
            43 => "Canon EF 28-105mm f/4-5.6".to_string(),
            44 => "Canon EF 90-300mm f/4.5-5.6".to_string(),
            45 => "Canon EF-S 18-55mm f/3.5-5.6 [II]".to_string(),
            46 => "Canon EF 28-90mm f/4-5.6".to_string(),
            47 => "Zeiss Milvus 35mm f/2 or 50mm f/2".to_string(),
            48 => "Canon EF-S 18-55mm f/3.5-5.6 IS".to_string(),
            49 => "Canon EF-S 55-250mm f/4-5.6 IS".to_string(),
            50 => "Canon EF-S 18-200mm f/3.5-5.6 IS".to_string(),
            51 => "Canon EF-S 18-135mm f/3.5-5.6 IS".to_string(),
            52 => "Canon EF-S 18-55mm f/3.5-5.6 IS II".to_string(),
            53 => "Canon EF-S 18-55mm f/3.5-5.6 III".to_string(),
            54 => "Canon EF-S 55-250mm f/4-5.6 IS II".to_string(),
            60 => "Irix 11mm f/4 or 15mm f/2.4".to_string(),
            63 => "Irix 30mm F1.4 Dragonfly".to_string(),
            80 => "Canon TS-E 50mm f/2.8L Macro".to_string(),
            81 => "Canon TS-E 90mm f/2.8L Macro".to_string(),
            82 => "Canon TS-E 135mm f/4L Macro".to_string(),
            94 => "Canon TS-E 17mm f/4L".to_string(),
            95 => "Canon TS-E 24mm f/3.5L II".to_string(),
            103 => "Samyang AF 14mm f/2.8 EF or Rokinon Lens".to_string(),
            106 => "Rokinon SP / Samyang XP 35mm f/1.2".to_string(),
            112 => "Sigma 28mm f/1.5 FF High-speed Prime or other Sigma Lens".to_string(),
            117 => "Tamron 35-150mm f/2.8-4.0 Di VC OSD (A043) or other Tamron Lens".to_string(),
            124 => "Canon MP-E 65mm f/2.8 1-5x Macro Photo".to_string(),
            125 => "Canon TS-E 24mm f/3.5L".to_string(),
            126 => "Canon TS-E 45mm f/2.8".to_string(),
            127 => "Canon TS-E 90mm f/2.8 or Tamron Lens".to_string(),
            129 => "Canon EF 300mm f/2.8L USM".to_string(),
            130 => "Canon EF 50mm f/1.0L USM".to_string(),
            131 => "Canon EF 28-80mm f/2.8-4L USM or Sigma Lens".to_string(),
            132 => "Canon EF 1200mm f/5.6L USM".to_string(),
            134 => "Canon EF 600mm f/4L IS USM".to_string(),
            135 => "Canon EF 200mm f/1.8L USM".to_string(),
            136 => "Canon EF 300mm f/2.8L USM".to_string(),
            137 => "Canon EF 85mm f/1.2L USM or Sigma or Tamron Lens".to_string(),
            138 => "Canon EF 28-80mm f/2.8-4L".to_string(),
            139 => "Canon EF 400mm f/2.8L USM".to_string(),
            140 => "Canon EF 500mm f/4.5L USM".to_string(),
            141 => "Canon EF 500mm f/4.5L USM".to_string(),
            142 => "Canon EF 300mm f/2.8L IS USM".to_string(),
            143 => "Canon EF 500mm f/4L IS USM or Sigma Lens".to_string(),
            144 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            145 => "Canon EF 100-300mm f/4.5-5.6 USM".to_string(),
            146 => "Canon EF 70-210mm f/3.5-4.5 USM".to_string(),
            147 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            148 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            149 => "Canon EF 100mm f/2 USM".to_string(),
            150 => "Canon EF 14mm f/2.8L USM or Sigma Lens".to_string(),
            151 => "Canon EF 200mm f/2.8L USM".to_string(),
            152 => "Canon EF 300mm f/4L IS USM or Sigma Lens".to_string(),
            153 => "Canon EF 35-350mm f/3.5-5.6L USM or Sigma or Tamron Lens".to_string(),
            154 => "Canon EF 20mm f/2.8 USM or Zeiss Lens".to_string(),
            155 => "Canon EF 85mm f/1.8 USM or Sigma Lens".to_string(),
            156 => "Canon EF 28-105mm f/3.5-4.5 USM or Tamron Lens".to_string(),
            160 => "Canon EF 20-35mm f/3.5-4.5 USM or Tamron or Tokina Lens".to_string(),
            161 => "Canon EF 28-70mm f/2.8L USM or Other Lens".to_string(),
            162 => "Canon EF 200mm f/2.8L USM".to_string(),
            163 => "Canon EF 300mm f/4L".to_string(),
            164 => "Canon EF 400mm f/5.6L".to_string(),
            165 => "Canon EF 70-200mm f/2.8L USM".to_string(),
            166 => "Canon EF 70-200mm f/2.8L USM + 1.4x".to_string(),
            167 => "Canon EF 70-200mm f/2.8L USM + 2x".to_string(),
            168 => "Canon EF 28mm f/1.8 USM or Sigma Lens".to_string(),
            169 => "Canon EF 17-35mm f/2.8L USM or Sigma Lens".to_string(),
            170 => "Canon EF 200mm f/2.8L II USM or Sigma Lens".to_string(),
            171 => "Canon EF 300mm f/4L USM".to_string(),
            172 => "Canon EF 400mm f/5.6L USM or Sigma Lens".to_string(),
            173 => "Canon EF 180mm Macro f/3.5L USM or Sigma Lens".to_string(),
            174 => "Canon EF 135mm f/2L USM or Other Lens".to_string(),
            175 => "Canon EF 400mm f/2.8L USM".to_string(),
            176 => "Canon EF 24-85mm f/3.5-4.5 USM".to_string(),
            177 => "Canon EF 300mm f/4L IS USM".to_string(),
            178 => "Canon EF 28-135mm f/3.5-5.6 IS".to_string(),
            179 => "Canon EF 24mm f/1.4L USM".to_string(),
            180 => "Canon EF 35mm f/1.4L USM or Other Lens".to_string(),
            181 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 1.4x or Sigma Lens".to_string(),
            182 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 2x or Sigma Lens".to_string(),
            183 => "Canon EF 100-400mm f/4.5-5.6L IS USM or Sigma Lens".to_string(),
            184 => "Canon EF 400mm f/2.8L USM + 2x".to_string(),
            185 => "Canon EF 600mm f/4L IS USM".to_string(),
            186 => "Canon EF 70-200mm f/4L USM".to_string(),
            187 => "Canon EF 70-200mm f/4L USM + 1.4x".to_string(),
            188 => "Canon EF 70-200mm f/4L USM + 2x".to_string(),
            189 => "Canon EF 70-200mm f/4L USM + 2.8x".to_string(),
            190 => "Canon EF 100mm f/2.8 Macro USM".to_string(),
            191 => "Canon EF 400mm f/4 DO IS or Sigma Lens".to_string(),
            193 => "Canon EF 35-80mm f/4-5.6 USM".to_string(),
            194 => "Canon EF 80-200mm f/4.5-5.6 USM".to_string(),
            195 => "Canon EF 35-105mm f/4.5-5.6 USM".to_string(),
            196 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            197 => "Canon EF 75-300mm f/4-5.6 IS USM or Sigma Lens".to_string(),
            198 => "Canon EF 50mm f/1.4 USM or Other Lens".to_string(),
            199 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            200 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            201 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            202 => "Canon EF 28-80mm f/3.5-5.6 USM IV".to_string(),
            208 => "Canon EF 22-55mm f/4-5.6 USM".to_string(),
            209 => "Canon EF 55-200mm f/4.5-5.6".to_string(),
            210 => "Canon EF 28-90mm f/4-5.6 USM".to_string(),
            211 => "Canon EF 28-200mm f/3.5-5.6 USM".to_string(),
            212 => "Canon EF 28-105mm f/4-5.6 USM".to_string(),
            213 => "Canon EF 90-300mm f/4.5-5.6 USM or Tamron Lens".to_string(),
            214 => "Canon EF-S 18-55mm f/3.5-5.6 USM".to_string(),
            215 => "Canon EF 55-200mm f/4.5-5.6 II USM".to_string(),
            217 => "Tamron AF 18-270mm f/3.5-6.3 Di II VC PZD".to_string(),
            220 => "Yongnuo YN 50mm f/1.8".to_string(),
            224 => "Canon EF 70-200mm f/2.8L IS USM".to_string(),
            225 => "Canon EF 70-200mm f/2.8L IS USM + 1.4x".to_string(),
            226 => "Canon EF 70-200mm f/2.8L IS USM + 2x".to_string(),
            227 => "Canon EF 70-200mm f/2.8L IS USM + 2.8x".to_string(),
            228 => "Canon EF 28-105mm f/3.5-4.5 USM".to_string(),
            229 => "Canon EF 16-35mm f/2.8L USM".to_string(),
            230 => "Canon EF 24-70mm f/2.8L USM".to_string(),
            231 => "Canon EF 17-40mm f/4L USM or Sigma Lens".to_string(),
            232 => "Canon EF 70-300mm f/4.5-5.6 DO IS USM".to_string(),
            233 => "Canon EF 28-300mm f/3.5-5.6L IS USM".to_string(),
            234 => "Canon EF-S 17-85mm f/4-5.6 IS USM or Tokina Lens".to_string(),
            235 => "Canon EF-S 10-22mm f/3.5-4.5 USM".to_string(),
            236 => "Canon EF-S 60mm f/2.8 Macro USM".to_string(),
            237 => "Canon EF 24-105mm f/4L IS USM".to_string(),
            238 => "Canon EF 70-300mm f/4-5.6 IS USM".to_string(),
            239 => "Canon EF 85mm f/1.2L II USM or Rokinon Lens".to_string(),
            240 => "Canon EF-S 17-55mm f/2.8 IS USM or Sigma Lens".to_string(),
            241 => "Canon EF 50mm f/1.2L USM".to_string(),
            242 => "Canon EF 70-200mm f/4L IS USM".to_string(),
            243 => "Canon EF 70-200mm f/4L IS USM + 1.4x".to_string(),
            244 => "Canon EF 70-200mm f/4L IS USM + 2x".to_string(),
            245 => "Canon EF 70-200mm f/4L IS USM + 2.8x".to_string(),
            246 => "Canon EF 16-35mm f/2.8L II USM".to_string(),
            247 => "Canon EF 14mm f/2.8L II USM".to_string(),
            248 => "Canon EF 200mm f/2L IS USM or Sigma Lens".to_string(),
            249 => "Canon EF 800mm f/5.6L IS USM".to_string(),
            250 => "Canon EF 24mm f/1.4L II USM or Sigma Lens".to_string(),
            251 => "Canon EF 70-200mm f/2.8L IS II USM".to_string(),
            252 => "Canon EF 70-200mm f/2.8L IS II USM + 1.4x".to_string(),
            253 => "Canon EF 70-200mm f/2.8L IS II USM + 2x".to_string(),
            254 => "Canon EF 100mm f/2.8L Macro IS USM or Tamron Lens".to_string(),
            255 => "Sigma 24-105mm f/4 DG OS HSM | A or Other Lens".to_string(),
            368 => "Sigma 14-24mm f/2.8 DG HSM | A or other Sigma Lens".to_string(),
            488 => "Canon EF-S 15-85mm f/3.5-5.6 IS USM".to_string(),
            489 => "Canon EF 70-300mm f/4-5.6L IS USM".to_string(),
            490 => "Canon EF 8-15mm f/4L Fisheye USM".to_string(),
            491 => "Canon EF 300mm f/2.8L IS II USM or Tamron Lens".to_string(),
            492 => "Canon EF 400mm f/2.8L IS II USM".to_string(),
            493 => "Canon EF 500mm f/4L IS II USM or EF 24-105mm f4L IS USM".to_string(),
            494 => "Canon EF 600mm f/4L IS II USM".to_string(),
            495 => "Canon EF 24-70mm f/2.8L II USM or Sigma Lens".to_string(),
            496 => "Canon EF 200-400mm f/4L IS USM".to_string(),
            499 => "Canon EF 200-400mm f/4L IS USM + 1.4x".to_string(),
            502 => "Canon EF 28mm f/2.8 IS USM or Tamron Lens".to_string(),
            503 => "Canon EF 24mm f/2.8 IS USM".to_string(),
            504 => "Canon EF 24-70mm f/4L IS USM".to_string(),
            505 => "Canon EF 35mm f/2 IS USM".to_string(),
            506 => "Canon EF 400mm f/4 DO IS II USM".to_string(),
            507 => "Canon EF 16-35mm f/4L IS USM".to_string(),
            508 => "Canon EF 11-24mm f/4L USM or Tamron Lens".to_string(),
            624 => "Sigma 70-200mm f/2.8 DG OS HSM | S or other Sigma Lens".to_string(),
            747 => "Canon EF 100-400mm f/4.5-5.6L IS II USM or Tamron Lens".to_string(),
            748 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 1.4x or Tamron Lens".to_string(),
            749 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 2x or Tamron Lens".to_string(),
            750 => "Canon EF 35mm f/1.4L II USM or Tamron Lens".to_string(),
            751 => "Canon EF 16-35mm f/2.8L III USM".to_string(),
            752 => "Canon EF 24-105mm f/4L IS II USM".to_string(),
            753 => "Canon EF 85mm f/1.4L IS USM".to_string(),
            754 => "Canon EF 70-200mm f/4L IS II USM".to_string(),
            757 => "Canon EF 400mm f/2.8L IS III USM".to_string(),
            758 => "Canon EF 600mm f/4L IS III USM".to_string(),
            923 => "Meike/SKY 85mm f/1.8 DCM".to_string(),
            1136 => "Sigma 24-70mm f/2.8 DG OS HSM | A".to_string(),
            4142 => "Canon EF-S 18-135mm f/3.5-5.6 IS STM".to_string(),
            4143 => "Canon EF-M 18-55mm f/3.5-5.6 IS STM or Tamron Lens".to_string(),
            4144 => "Canon EF 40mm f/2.8 STM".to_string(),
            4145 => "Canon EF-M 22mm f/2 STM".to_string(),
            4146 => "Canon EF-S 18-55mm f/3.5-5.6 IS STM".to_string(),
            4147 => "Canon EF-M 11-22mm f/4-5.6 IS STM".to_string(),
            4148 => "Canon EF-S 55-250mm f/4-5.6 IS STM".to_string(),
            4149 => "Canon EF-M 55-200mm f/4.5-6.3 IS STM".to_string(),
            4150 => "Canon EF-S 10-18mm f/4.5-5.6 IS STM".to_string(),
            4152 => "Canon EF 24-105mm f/3.5-5.6 IS STM".to_string(),
            4153 => "Canon EF-M 15-45mm f/3.5-6.3 IS STM".to_string(),
            4154 => "Canon EF-S 24mm f/2.8 STM".to_string(),
            4155 => "Canon EF-M 28mm f/3.5 Macro IS STM".to_string(),
            4156 => "Canon EF 50mm f/1.8 STM".to_string(),
            4157 => "Canon EF-M 18-150mm f/3.5-6.3 IS STM".to_string(),
            4158 => "Canon EF-S 18-55mm f/4-5.6 IS STM".to_string(),
            4159 => "Canon EF-M 32mm f/1.4 STM".to_string(),
            4160 => "Canon EF-S 35mm f/2.8 Macro IS STM".to_string(),
            4208 => "Sigma 56mm f/1.4 DC DN | C or other Sigma Lens".to_string(),
            4976 => "Sigma 16-300mm F3.5-6.7 DC OS | C (025)".to_string(),
            6512 => "Sigma 12mm F1.4 DC | C".to_string(),
            36910 => "Canon EF 70-300mm f/4-5.6 IS II USM".to_string(),
            36912 => "Canon EF-S 18-135mm f/3.5-5.6 IS USM".to_string(),
            61182 => "Canon RF 50mm F1.2L USM or other Canon RF Lens".to_string(),
            61491 => "Canon CN-E 14mm T3.1 L F".to_string(),
            61492 => "Canon CN-E 24mm T1.5 L F".to_string(),
            61494 => "Canon CN-E 85mm T1.3 L F".to_string(),
            61495 => "Canon CN-E 135mm T2.2 L F".to_string(),
            61496 => "Canon CN-E 35mm T1.5 L F".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("LensType", 0xf6, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0xf8, bo) {
        dm.push(("MinFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MinFocalLength", 0xf8, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0xfa, bo) {
        dm.push(("MaxFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MaxFocalLength", 0xfa, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(text) = text_at(data, 0x190, 6, true) {
        tags.push(mk("FirmwareVersion", 0x190, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x1d3, bo) {
        dm.push(("FileIndex".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val + 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("FileIndex", 0x1d3, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x1df, bo) {
        dm.push(("DirectoryIndex".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("DirectoryIndex", 0x1df, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(sub) = data.get(0x30b..) {
        tags.extend(canon_psinfo(sub, model, bo, file_type, format, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::CameraInfo550D` -- FORMAT int8u, FIRST_ENTRY 0.
fn canon_camerainfo550d(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = u8_at(data, 0x3) {
        dm.push(("FNumber".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(($val-8)/16*log(2))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("sprintf(\"%.2g\",$val)", &cv, &ctx) { cv = x; }
            tags.push(mk("FNumber", 0x3, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x4) {
        dm.push(("ExposureTime".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(4*log(2)*(1-Image::ExifTool::Canon::CanonEv($val-24)))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("Image::ExifTool::Exif::PrintExposureTime($val)", &cv, &ctx) { cv = x; }
            tags.push(mk("ExposureTime", 0x4, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x6) {
        dm.push(("ISO".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("100*exp(($val/8-9)*log(2))", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("sprintf(\"%.0f\",$val)", &cv, &ctx) { cv = x; }
        tags.push(mk("ISO", 0x6, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x7) {
        dm.push(("HighlightTonePriority".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "People".to_string(),
            1 => "sRGB".to_string(),
            2 => "Adobe RGB".to_string(),
            3 => "User 1".to_string(),
            4 => "User 2".to_string(),
            5 => "User 3".to_string(),
            6 => "To Do".to_string(),
            65535 => "n/a".to_string(),
            2415919104 => "Format 1".to_string(),
            2684354560 => "Format 2".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("HighlightTonePriority", 0x7, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x15) {
        dm.push(("FlashMeteringMode".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "E-TTL".to_string(),
            3 => "TTL".to_string(),
            4 => "External Auto".to_string(),
            5 => "External Manual".to_string(),
            6 => "Off".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("FlashMeteringMode", 0x15, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x19) {
        dm.push(("CameraTemperature".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 128", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
        tags.push(mk("CameraTemperature", 0x19, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x1e, bo) {
        dm.push(("FocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
            tags.push(mk("FocalLength", 0x1e, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x35) {
        dm.push(("CameraOrientation".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Horizontal (normal)".to_string(),
            1 => "Rotate 90 CW".to_string(),
            2 => "Rotate 270 CW".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("CameraOrientation", 0x35, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x54, bo) {
        dm.push(("FocusDistanceUpper".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceUpper", 0x54, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x56, bo) {
        dm.push(("FocusDistanceLower".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceLower", 0x56, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x78, bo) {
        dm.push(("WhiteBalance".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Auto".to_string(),
            1 => "Daylight".to_string(),
            2 => "Cloudy".to_string(),
            3 => "Tungsten".to_string(),
            4 => "Fluorescent".to_string(),
            5 => "Flash".to_string(),
            6 => "Custom".to_string(),
            7 => "Black & White".to_string(),
            8 => "Shade".to_string(),
            9 => "Manual Temperature (Kelvin)".to_string(),
            10 => "PC Set1".to_string(),
            11 => "PC Set2".to_string(),
            12 => "PC Set3".to_string(),
            14 => "Daylight Fluorescent".to_string(),
            15 => "Custom 1".to_string(),
            16 => "Custom 2".to_string(),
            17 => "Underwater".to_string(),
            18 => "Custom 3".to_string(),
            19 => "Custom 4".to_string(),
            20 => "PC Set4".to_string(),
            21 => "PC Set5".to_string(),
            23 => "Auto (ambience priority)".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("WhiteBalance", 0x78, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x7c, bo) {
        dm.push(("ColorTemperature".to_string(), f64::from(v)));
        tags.push(mk("ColorTemperature", 0x7c, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0xb0) {
        dm.push(("PictureStyle".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "None".to_string(),
            1 => "Standard".to_string(),
            2 => "Portrait".to_string(),
            3 => "High Saturation".to_string(),
            4 => "Adobe RGB".to_string(),
            5 => "Low Saturation".to_string(),
            6 => "CM Set 1".to_string(),
            7 => "CM Set 2".to_string(),
            33 => "User Def. 1".to_string(),
            34 => "User Def. 2".to_string(),
            35 => "User Def. 3".to_string(),
            65 => "PC 1".to_string(),
            66 => "PC 2".to_string(),
            67 => "PC 3".to_string(),
            129 => "Standard".to_string(),
            130 => "Portrait".to_string(),
            131 => "Landscape".to_string(),
            132 => "Neutral".to_string(),
            133 => "Faithful".to_string(),
            134 => "Monochrome".to_string(),
            135 => "Auto".to_string(),
            136 => "Fine Detail".to_string(),
            255 => "n/a".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("PictureStyle", 0xb0, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0xff, bo) {
        dm.push(("LensType".to_string(), f64::from(v)));
        let s = match v as i64 {
            -1 => "n/a".to_string(),
            1 => "Canon EF 50mm f/1.8".to_string(),
            2 => "Canon EF 28mm f/2.8 or Sigma Lens".to_string(),
            3 => "Canon EF 135mm f/2.8 Soft".to_string(),
            4 => "Canon EF 35-105mm f/3.5-4.5 or Sigma Lens".to_string(),
            5 => "Canon EF 35-70mm f/3.5-4.5".to_string(),
            6 => "Canon EF 28-70mm f/3.5-4.5 or Sigma or Tokina Lens".to_string(),
            7 => "Canon EF 100-300mm f/5.6L".to_string(),
            8 => "Canon EF 100-300mm f/5.6 or Sigma or Tokina Lens".to_string(),
            9 => "Canon EF 70-210mm f/4".to_string(),
            10 => "Canon EF 50mm f/2.5 Macro or Sigma Lens".to_string(),
            11 => "Canon EF 35mm f/2".to_string(),
            13 => "Canon EF 15mm f/2.8 Fisheye".to_string(),
            14 => "Canon EF 50-200mm f/3.5-4.5L".to_string(),
            15 => "Canon EF 50-200mm f/3.5-4.5".to_string(),
            16 => "Canon EF 35-135mm f/3.5-4.5".to_string(),
            17 => "Canon EF 35-70mm f/3.5-4.5A".to_string(),
            18 => "Canon EF 28-70mm f/3.5-4.5".to_string(),
            20 => "Canon EF 100-200mm f/4.5A".to_string(),
            21 => "Canon EF 80-200mm f/2.8L".to_string(),
            22 => "Canon EF 20-35mm f/2.8L or Tokina Lens".to_string(),
            23 => "Canon EF 35-105mm f/3.5-4.5".to_string(),
            24 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            25 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            26 => "Canon EF 100mm f/2.8 Macro or Other Lens".to_string(),
            27 => "Canon EF 35-80mm f/4-5.6".to_string(),
            28 => "Canon EF 80-200mm f/4.5-5.6 or Tamron Lens".to_string(),
            29 => "Canon EF 50mm f/1.8 II".to_string(),
            30 => "Canon EF 35-105mm f/4.5-5.6".to_string(),
            31 => "Canon EF 75-300mm f/4-5.6 or Tamron Lens".to_string(),
            32 => "Canon EF 24mm f/2.8 or Sigma Lens".to_string(),
            33 => "Voigtlander or Carl Zeiss Lens".to_string(),
            35 => "Canon EF 35-80mm f/4-5.6".to_string(),
            36 => "Canon EF 38-76mm f/4.5-5.6".to_string(),
            37 => "Canon EF 35-80mm f/4-5.6 or Tamron Lens".to_string(),
            38 => "Canon EF 80-200mm f/4.5-5.6 II".to_string(),
            39 => "Canon EF 75-300mm f/4-5.6".to_string(),
            40 => "Canon EF 28-80mm f/3.5-5.6".to_string(),
            41 => "Canon EF 28-90mm f/4-5.6".to_string(),
            42 => "Canon EF 28-200mm f/3.5-5.6 or Tamron Lens".to_string(),
            43 => "Canon EF 28-105mm f/4-5.6".to_string(),
            44 => "Canon EF 90-300mm f/4.5-5.6".to_string(),
            45 => "Canon EF-S 18-55mm f/3.5-5.6 [II]".to_string(),
            46 => "Canon EF 28-90mm f/4-5.6".to_string(),
            47 => "Zeiss Milvus 35mm f/2 or 50mm f/2".to_string(),
            48 => "Canon EF-S 18-55mm f/3.5-5.6 IS".to_string(),
            49 => "Canon EF-S 55-250mm f/4-5.6 IS".to_string(),
            50 => "Canon EF-S 18-200mm f/3.5-5.6 IS".to_string(),
            51 => "Canon EF-S 18-135mm f/3.5-5.6 IS".to_string(),
            52 => "Canon EF-S 18-55mm f/3.5-5.6 IS II".to_string(),
            53 => "Canon EF-S 18-55mm f/3.5-5.6 III".to_string(),
            54 => "Canon EF-S 55-250mm f/4-5.6 IS II".to_string(),
            60 => "Irix 11mm f/4 or 15mm f/2.4".to_string(),
            63 => "Irix 30mm F1.4 Dragonfly".to_string(),
            80 => "Canon TS-E 50mm f/2.8L Macro".to_string(),
            81 => "Canon TS-E 90mm f/2.8L Macro".to_string(),
            82 => "Canon TS-E 135mm f/4L Macro".to_string(),
            94 => "Canon TS-E 17mm f/4L".to_string(),
            95 => "Canon TS-E 24mm f/3.5L II".to_string(),
            103 => "Samyang AF 14mm f/2.8 EF or Rokinon Lens".to_string(),
            106 => "Rokinon SP / Samyang XP 35mm f/1.2".to_string(),
            112 => "Sigma 28mm f/1.5 FF High-speed Prime or other Sigma Lens".to_string(),
            117 => "Tamron 35-150mm f/2.8-4.0 Di VC OSD (A043) or other Tamron Lens".to_string(),
            124 => "Canon MP-E 65mm f/2.8 1-5x Macro Photo".to_string(),
            125 => "Canon TS-E 24mm f/3.5L".to_string(),
            126 => "Canon TS-E 45mm f/2.8".to_string(),
            127 => "Canon TS-E 90mm f/2.8 or Tamron Lens".to_string(),
            129 => "Canon EF 300mm f/2.8L USM".to_string(),
            130 => "Canon EF 50mm f/1.0L USM".to_string(),
            131 => "Canon EF 28-80mm f/2.8-4L USM or Sigma Lens".to_string(),
            132 => "Canon EF 1200mm f/5.6L USM".to_string(),
            134 => "Canon EF 600mm f/4L IS USM".to_string(),
            135 => "Canon EF 200mm f/1.8L USM".to_string(),
            136 => "Canon EF 300mm f/2.8L USM".to_string(),
            137 => "Canon EF 85mm f/1.2L USM or Sigma or Tamron Lens".to_string(),
            138 => "Canon EF 28-80mm f/2.8-4L".to_string(),
            139 => "Canon EF 400mm f/2.8L USM".to_string(),
            140 => "Canon EF 500mm f/4.5L USM".to_string(),
            141 => "Canon EF 500mm f/4.5L USM".to_string(),
            142 => "Canon EF 300mm f/2.8L IS USM".to_string(),
            143 => "Canon EF 500mm f/4L IS USM or Sigma Lens".to_string(),
            144 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            145 => "Canon EF 100-300mm f/4.5-5.6 USM".to_string(),
            146 => "Canon EF 70-210mm f/3.5-4.5 USM".to_string(),
            147 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            148 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            149 => "Canon EF 100mm f/2 USM".to_string(),
            150 => "Canon EF 14mm f/2.8L USM or Sigma Lens".to_string(),
            151 => "Canon EF 200mm f/2.8L USM".to_string(),
            152 => "Canon EF 300mm f/4L IS USM or Sigma Lens".to_string(),
            153 => "Canon EF 35-350mm f/3.5-5.6L USM or Sigma or Tamron Lens".to_string(),
            154 => "Canon EF 20mm f/2.8 USM or Zeiss Lens".to_string(),
            155 => "Canon EF 85mm f/1.8 USM or Sigma Lens".to_string(),
            156 => "Canon EF 28-105mm f/3.5-4.5 USM or Tamron Lens".to_string(),
            160 => "Canon EF 20-35mm f/3.5-4.5 USM or Tamron or Tokina Lens".to_string(),
            161 => "Canon EF 28-70mm f/2.8L USM or Other Lens".to_string(),
            162 => "Canon EF 200mm f/2.8L USM".to_string(),
            163 => "Canon EF 300mm f/4L".to_string(),
            164 => "Canon EF 400mm f/5.6L".to_string(),
            165 => "Canon EF 70-200mm f/2.8L USM".to_string(),
            166 => "Canon EF 70-200mm f/2.8L USM + 1.4x".to_string(),
            167 => "Canon EF 70-200mm f/2.8L USM + 2x".to_string(),
            168 => "Canon EF 28mm f/1.8 USM or Sigma Lens".to_string(),
            169 => "Canon EF 17-35mm f/2.8L USM or Sigma Lens".to_string(),
            170 => "Canon EF 200mm f/2.8L II USM or Sigma Lens".to_string(),
            171 => "Canon EF 300mm f/4L USM".to_string(),
            172 => "Canon EF 400mm f/5.6L USM or Sigma Lens".to_string(),
            173 => "Canon EF 180mm Macro f/3.5L USM or Sigma Lens".to_string(),
            174 => "Canon EF 135mm f/2L USM or Other Lens".to_string(),
            175 => "Canon EF 400mm f/2.8L USM".to_string(),
            176 => "Canon EF 24-85mm f/3.5-4.5 USM".to_string(),
            177 => "Canon EF 300mm f/4L IS USM".to_string(),
            178 => "Canon EF 28-135mm f/3.5-5.6 IS".to_string(),
            179 => "Canon EF 24mm f/1.4L USM".to_string(),
            180 => "Canon EF 35mm f/1.4L USM or Other Lens".to_string(),
            181 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 1.4x or Sigma Lens".to_string(),
            182 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 2x or Sigma Lens".to_string(),
            183 => "Canon EF 100-400mm f/4.5-5.6L IS USM or Sigma Lens".to_string(),
            184 => "Canon EF 400mm f/2.8L USM + 2x".to_string(),
            185 => "Canon EF 600mm f/4L IS USM".to_string(),
            186 => "Canon EF 70-200mm f/4L USM".to_string(),
            187 => "Canon EF 70-200mm f/4L USM + 1.4x".to_string(),
            188 => "Canon EF 70-200mm f/4L USM + 2x".to_string(),
            189 => "Canon EF 70-200mm f/4L USM + 2.8x".to_string(),
            190 => "Canon EF 100mm f/2.8 Macro USM".to_string(),
            191 => "Canon EF 400mm f/4 DO IS or Sigma Lens".to_string(),
            193 => "Canon EF 35-80mm f/4-5.6 USM".to_string(),
            194 => "Canon EF 80-200mm f/4.5-5.6 USM".to_string(),
            195 => "Canon EF 35-105mm f/4.5-5.6 USM".to_string(),
            196 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            197 => "Canon EF 75-300mm f/4-5.6 IS USM or Sigma Lens".to_string(),
            198 => "Canon EF 50mm f/1.4 USM or Other Lens".to_string(),
            199 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            200 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            201 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            202 => "Canon EF 28-80mm f/3.5-5.6 USM IV".to_string(),
            208 => "Canon EF 22-55mm f/4-5.6 USM".to_string(),
            209 => "Canon EF 55-200mm f/4.5-5.6".to_string(),
            210 => "Canon EF 28-90mm f/4-5.6 USM".to_string(),
            211 => "Canon EF 28-200mm f/3.5-5.6 USM".to_string(),
            212 => "Canon EF 28-105mm f/4-5.6 USM".to_string(),
            213 => "Canon EF 90-300mm f/4.5-5.6 USM or Tamron Lens".to_string(),
            214 => "Canon EF-S 18-55mm f/3.5-5.6 USM".to_string(),
            215 => "Canon EF 55-200mm f/4.5-5.6 II USM".to_string(),
            217 => "Tamron AF 18-270mm f/3.5-6.3 Di II VC PZD".to_string(),
            220 => "Yongnuo YN 50mm f/1.8".to_string(),
            224 => "Canon EF 70-200mm f/2.8L IS USM".to_string(),
            225 => "Canon EF 70-200mm f/2.8L IS USM + 1.4x".to_string(),
            226 => "Canon EF 70-200mm f/2.8L IS USM + 2x".to_string(),
            227 => "Canon EF 70-200mm f/2.8L IS USM + 2.8x".to_string(),
            228 => "Canon EF 28-105mm f/3.5-4.5 USM".to_string(),
            229 => "Canon EF 16-35mm f/2.8L USM".to_string(),
            230 => "Canon EF 24-70mm f/2.8L USM".to_string(),
            231 => "Canon EF 17-40mm f/4L USM or Sigma Lens".to_string(),
            232 => "Canon EF 70-300mm f/4.5-5.6 DO IS USM".to_string(),
            233 => "Canon EF 28-300mm f/3.5-5.6L IS USM".to_string(),
            234 => "Canon EF-S 17-85mm f/4-5.6 IS USM or Tokina Lens".to_string(),
            235 => "Canon EF-S 10-22mm f/3.5-4.5 USM".to_string(),
            236 => "Canon EF-S 60mm f/2.8 Macro USM".to_string(),
            237 => "Canon EF 24-105mm f/4L IS USM".to_string(),
            238 => "Canon EF 70-300mm f/4-5.6 IS USM".to_string(),
            239 => "Canon EF 85mm f/1.2L II USM or Rokinon Lens".to_string(),
            240 => "Canon EF-S 17-55mm f/2.8 IS USM or Sigma Lens".to_string(),
            241 => "Canon EF 50mm f/1.2L USM".to_string(),
            242 => "Canon EF 70-200mm f/4L IS USM".to_string(),
            243 => "Canon EF 70-200mm f/4L IS USM + 1.4x".to_string(),
            244 => "Canon EF 70-200mm f/4L IS USM + 2x".to_string(),
            245 => "Canon EF 70-200mm f/4L IS USM + 2.8x".to_string(),
            246 => "Canon EF 16-35mm f/2.8L II USM".to_string(),
            247 => "Canon EF 14mm f/2.8L II USM".to_string(),
            248 => "Canon EF 200mm f/2L IS USM or Sigma Lens".to_string(),
            249 => "Canon EF 800mm f/5.6L IS USM".to_string(),
            250 => "Canon EF 24mm f/1.4L II USM or Sigma Lens".to_string(),
            251 => "Canon EF 70-200mm f/2.8L IS II USM".to_string(),
            252 => "Canon EF 70-200mm f/2.8L IS II USM + 1.4x".to_string(),
            253 => "Canon EF 70-200mm f/2.8L IS II USM + 2x".to_string(),
            254 => "Canon EF 100mm f/2.8L Macro IS USM or Tamron Lens".to_string(),
            255 => "Sigma 24-105mm f/4 DG OS HSM | A or Other Lens".to_string(),
            368 => "Sigma 14-24mm f/2.8 DG HSM | A or other Sigma Lens".to_string(),
            488 => "Canon EF-S 15-85mm f/3.5-5.6 IS USM".to_string(),
            489 => "Canon EF 70-300mm f/4-5.6L IS USM".to_string(),
            490 => "Canon EF 8-15mm f/4L Fisheye USM".to_string(),
            491 => "Canon EF 300mm f/2.8L IS II USM or Tamron Lens".to_string(),
            492 => "Canon EF 400mm f/2.8L IS II USM".to_string(),
            493 => "Canon EF 500mm f/4L IS II USM or EF 24-105mm f4L IS USM".to_string(),
            494 => "Canon EF 600mm f/4L IS II USM".to_string(),
            495 => "Canon EF 24-70mm f/2.8L II USM or Sigma Lens".to_string(),
            496 => "Canon EF 200-400mm f/4L IS USM".to_string(),
            499 => "Canon EF 200-400mm f/4L IS USM + 1.4x".to_string(),
            502 => "Canon EF 28mm f/2.8 IS USM or Tamron Lens".to_string(),
            503 => "Canon EF 24mm f/2.8 IS USM".to_string(),
            504 => "Canon EF 24-70mm f/4L IS USM".to_string(),
            505 => "Canon EF 35mm f/2 IS USM".to_string(),
            506 => "Canon EF 400mm f/4 DO IS II USM".to_string(),
            507 => "Canon EF 16-35mm f/4L IS USM".to_string(),
            508 => "Canon EF 11-24mm f/4L USM or Tamron Lens".to_string(),
            624 => "Sigma 70-200mm f/2.8 DG OS HSM | S or other Sigma Lens".to_string(),
            747 => "Canon EF 100-400mm f/4.5-5.6L IS II USM or Tamron Lens".to_string(),
            748 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 1.4x or Tamron Lens".to_string(),
            749 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 2x or Tamron Lens".to_string(),
            750 => "Canon EF 35mm f/1.4L II USM or Tamron Lens".to_string(),
            751 => "Canon EF 16-35mm f/2.8L III USM".to_string(),
            752 => "Canon EF 24-105mm f/4L IS II USM".to_string(),
            753 => "Canon EF 85mm f/1.4L IS USM".to_string(),
            754 => "Canon EF 70-200mm f/4L IS II USM".to_string(),
            757 => "Canon EF 400mm f/2.8L IS III USM".to_string(),
            758 => "Canon EF 600mm f/4L IS III USM".to_string(),
            923 => "Meike/SKY 85mm f/1.8 DCM".to_string(),
            1136 => "Sigma 24-70mm f/2.8 DG OS HSM | A".to_string(),
            4142 => "Canon EF-S 18-135mm f/3.5-5.6 IS STM".to_string(),
            4143 => "Canon EF-M 18-55mm f/3.5-5.6 IS STM or Tamron Lens".to_string(),
            4144 => "Canon EF 40mm f/2.8 STM".to_string(),
            4145 => "Canon EF-M 22mm f/2 STM".to_string(),
            4146 => "Canon EF-S 18-55mm f/3.5-5.6 IS STM".to_string(),
            4147 => "Canon EF-M 11-22mm f/4-5.6 IS STM".to_string(),
            4148 => "Canon EF-S 55-250mm f/4-5.6 IS STM".to_string(),
            4149 => "Canon EF-M 55-200mm f/4.5-6.3 IS STM".to_string(),
            4150 => "Canon EF-S 10-18mm f/4.5-5.6 IS STM".to_string(),
            4152 => "Canon EF 24-105mm f/3.5-5.6 IS STM".to_string(),
            4153 => "Canon EF-M 15-45mm f/3.5-6.3 IS STM".to_string(),
            4154 => "Canon EF-S 24mm f/2.8 STM".to_string(),
            4155 => "Canon EF-M 28mm f/3.5 Macro IS STM".to_string(),
            4156 => "Canon EF 50mm f/1.8 STM".to_string(),
            4157 => "Canon EF-M 18-150mm f/3.5-6.3 IS STM".to_string(),
            4158 => "Canon EF-S 18-55mm f/4-5.6 IS STM".to_string(),
            4159 => "Canon EF-M 32mm f/1.4 STM".to_string(),
            4160 => "Canon EF-S 35mm f/2.8 Macro IS STM".to_string(),
            4208 => "Sigma 56mm f/1.4 DC DN | C or other Sigma Lens".to_string(),
            4976 => "Sigma 16-300mm F3.5-6.7 DC OS | C (025)".to_string(),
            6512 => "Sigma 12mm F1.4 DC | C".to_string(),
            36910 => "Canon EF 70-300mm f/4-5.6 IS II USM".to_string(),
            36912 => "Canon EF-S 18-135mm f/3.5-5.6 IS USM".to_string(),
            61182 => "Canon RF 50mm F1.2L USM or other Canon RF Lens".to_string(),
            61491 => "Canon CN-E 14mm T3.1 L F".to_string(),
            61492 => "Canon CN-E 24mm T1.5 L F".to_string(),
            61494 => "Canon CN-E 85mm T1.3 L F".to_string(),
            61495 => "Canon CN-E 135mm T2.2 L F".to_string(),
            61496 => "Canon CN-E 35mm T1.5 L F".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("LensType", 0xff, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x101, bo) {
        dm.push(("MinFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MinFocalLength", 0x101, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x103, bo) {
        dm.push(("MaxFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MaxFocalLength", 0x103, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(text) = text_at(data, 0x1a4, 6, true) {
        tags.push(mk("FirmwareVersion", 0x1a4, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x1e4, bo) {
        dm.push(("FileIndex".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val + 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("FileIndex", 0x1e4, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x1f0, bo) {
        dm.push(("DirectoryIndex".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("DirectoryIndex", 0x1f0, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(sub) = data.get(0x31c..) {
        tags.extend(canon_psinfo(sub, model, bo, file_type, format, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::CameraInfo600D` -- FORMAT int8u, FIRST_ENTRY 0.
fn canon_camerainfo600d(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = u8_at(data, 0x3) {
        dm.push(("FNumber".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(($val-8)/16*log(2))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("sprintf(\"%.2g\",$val)", &cv, &ctx) { cv = x; }
            tags.push(mk("FNumber", 0x3, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x4) {
        dm.push(("ExposureTime".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(4*log(2)*(1-Image::ExifTool::Canon::CanonEv($val-24)))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("Image::ExifTool::Exif::PrintExposureTime($val)", &cv, &ctx) { cv = x; }
            tags.push(mk("ExposureTime", 0x4, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x6) {
        dm.push(("ISO".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("100*exp(($val/8-9)*log(2))", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("sprintf(\"%.0f\",$val)", &cv, &ctx) { cv = x; }
        tags.push(mk("ISO", 0x6, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x7) {
        dm.push(("HighlightTonePriority".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "People".to_string(),
            1 => "sRGB".to_string(),
            2 => "Adobe RGB".to_string(),
            3 => "User 1".to_string(),
            4 => "User 2".to_string(),
            5 => "User 3".to_string(),
            6 => "To Do".to_string(),
            65535 => "n/a".to_string(),
            2415919104 => "Format 1".to_string(),
            2684354560 => "Format 2".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("HighlightTonePriority", 0x7, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x15) {
        dm.push(("FlashMeteringMode".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "E-TTL".to_string(),
            3 => "TTL".to_string(),
            4 => "External Auto".to_string(),
            5 => "External Manual".to_string(),
            6 => "Off".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("FlashMeteringMode", 0x15, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x19) {
        dm.push(("CameraTemperature".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 128", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
        tags.push(mk("CameraTemperature", 0x19, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x1e, bo) {
        dm.push(("FocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
            tags.push(mk("FocalLength", 0x1e, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x38) {
        dm.push(("CameraOrientation".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Horizontal (normal)".to_string(),
            1 => "Rotate 90 CW".to_string(),
            2 => "Rotate 270 CW".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("CameraOrientation", 0x38, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x57, bo) {
        dm.push(("FocusDistanceUpper".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceUpper", 0x57, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x59, bo) {
        dm.push(("FocusDistanceLower".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceLower", 0x59, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x7b, bo) {
        dm.push(("WhiteBalance".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Auto".to_string(),
            1 => "Daylight".to_string(),
            2 => "Cloudy".to_string(),
            3 => "Tungsten".to_string(),
            4 => "Fluorescent".to_string(),
            5 => "Flash".to_string(),
            6 => "Custom".to_string(),
            7 => "Black & White".to_string(),
            8 => "Shade".to_string(),
            9 => "Manual Temperature (Kelvin)".to_string(),
            10 => "PC Set1".to_string(),
            11 => "PC Set2".to_string(),
            12 => "PC Set3".to_string(),
            14 => "Daylight Fluorescent".to_string(),
            15 => "Custom 1".to_string(),
            16 => "Custom 2".to_string(),
            17 => "Underwater".to_string(),
            18 => "Custom 3".to_string(),
            19 => "Custom 4".to_string(),
            20 => "PC Set4".to_string(),
            21 => "PC Set5".to_string(),
            23 => "Auto (ambience priority)".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("WhiteBalance", 0x7b, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x7f, bo) {
        dm.push(("ColorTemperature".to_string(), f64::from(v)));
        tags.push(mk("ColorTemperature", 0x7f, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0xb3) {
        dm.push(("PictureStyle".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "None".to_string(),
            1 => "Standard".to_string(),
            2 => "Portrait".to_string(),
            3 => "High Saturation".to_string(),
            4 => "Adobe RGB".to_string(),
            5 => "Low Saturation".to_string(),
            6 => "CM Set 1".to_string(),
            7 => "CM Set 2".to_string(),
            33 => "User Def. 1".to_string(),
            34 => "User Def. 2".to_string(),
            35 => "User Def. 3".to_string(),
            65 => "PC 1".to_string(),
            66 => "PC 2".to_string(),
            67 => "PC 3".to_string(),
            129 => "Standard".to_string(),
            130 => "Portrait".to_string(),
            131 => "Landscape".to_string(),
            132 => "Neutral".to_string(),
            133 => "Faithful".to_string(),
            134 => "Monochrome".to_string(),
            135 => "Auto".to_string(),
            136 => "Fine Detail".to_string(),
            255 => "n/a".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("PictureStyle", 0xb3, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0xea, bo) {
        dm.push(("LensType".to_string(), f64::from(v)));
        let s = match v as i64 {
            -1 => "n/a".to_string(),
            1 => "Canon EF 50mm f/1.8".to_string(),
            2 => "Canon EF 28mm f/2.8 or Sigma Lens".to_string(),
            3 => "Canon EF 135mm f/2.8 Soft".to_string(),
            4 => "Canon EF 35-105mm f/3.5-4.5 or Sigma Lens".to_string(),
            5 => "Canon EF 35-70mm f/3.5-4.5".to_string(),
            6 => "Canon EF 28-70mm f/3.5-4.5 or Sigma or Tokina Lens".to_string(),
            7 => "Canon EF 100-300mm f/5.6L".to_string(),
            8 => "Canon EF 100-300mm f/5.6 or Sigma or Tokina Lens".to_string(),
            9 => "Canon EF 70-210mm f/4".to_string(),
            10 => "Canon EF 50mm f/2.5 Macro or Sigma Lens".to_string(),
            11 => "Canon EF 35mm f/2".to_string(),
            13 => "Canon EF 15mm f/2.8 Fisheye".to_string(),
            14 => "Canon EF 50-200mm f/3.5-4.5L".to_string(),
            15 => "Canon EF 50-200mm f/3.5-4.5".to_string(),
            16 => "Canon EF 35-135mm f/3.5-4.5".to_string(),
            17 => "Canon EF 35-70mm f/3.5-4.5A".to_string(),
            18 => "Canon EF 28-70mm f/3.5-4.5".to_string(),
            20 => "Canon EF 100-200mm f/4.5A".to_string(),
            21 => "Canon EF 80-200mm f/2.8L".to_string(),
            22 => "Canon EF 20-35mm f/2.8L or Tokina Lens".to_string(),
            23 => "Canon EF 35-105mm f/3.5-4.5".to_string(),
            24 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            25 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            26 => "Canon EF 100mm f/2.8 Macro or Other Lens".to_string(),
            27 => "Canon EF 35-80mm f/4-5.6".to_string(),
            28 => "Canon EF 80-200mm f/4.5-5.6 or Tamron Lens".to_string(),
            29 => "Canon EF 50mm f/1.8 II".to_string(),
            30 => "Canon EF 35-105mm f/4.5-5.6".to_string(),
            31 => "Canon EF 75-300mm f/4-5.6 or Tamron Lens".to_string(),
            32 => "Canon EF 24mm f/2.8 or Sigma Lens".to_string(),
            33 => "Voigtlander or Carl Zeiss Lens".to_string(),
            35 => "Canon EF 35-80mm f/4-5.6".to_string(),
            36 => "Canon EF 38-76mm f/4.5-5.6".to_string(),
            37 => "Canon EF 35-80mm f/4-5.6 or Tamron Lens".to_string(),
            38 => "Canon EF 80-200mm f/4.5-5.6 II".to_string(),
            39 => "Canon EF 75-300mm f/4-5.6".to_string(),
            40 => "Canon EF 28-80mm f/3.5-5.6".to_string(),
            41 => "Canon EF 28-90mm f/4-5.6".to_string(),
            42 => "Canon EF 28-200mm f/3.5-5.6 or Tamron Lens".to_string(),
            43 => "Canon EF 28-105mm f/4-5.6".to_string(),
            44 => "Canon EF 90-300mm f/4.5-5.6".to_string(),
            45 => "Canon EF-S 18-55mm f/3.5-5.6 [II]".to_string(),
            46 => "Canon EF 28-90mm f/4-5.6".to_string(),
            47 => "Zeiss Milvus 35mm f/2 or 50mm f/2".to_string(),
            48 => "Canon EF-S 18-55mm f/3.5-5.6 IS".to_string(),
            49 => "Canon EF-S 55-250mm f/4-5.6 IS".to_string(),
            50 => "Canon EF-S 18-200mm f/3.5-5.6 IS".to_string(),
            51 => "Canon EF-S 18-135mm f/3.5-5.6 IS".to_string(),
            52 => "Canon EF-S 18-55mm f/3.5-5.6 IS II".to_string(),
            53 => "Canon EF-S 18-55mm f/3.5-5.6 III".to_string(),
            54 => "Canon EF-S 55-250mm f/4-5.6 IS II".to_string(),
            60 => "Irix 11mm f/4 or 15mm f/2.4".to_string(),
            63 => "Irix 30mm F1.4 Dragonfly".to_string(),
            80 => "Canon TS-E 50mm f/2.8L Macro".to_string(),
            81 => "Canon TS-E 90mm f/2.8L Macro".to_string(),
            82 => "Canon TS-E 135mm f/4L Macro".to_string(),
            94 => "Canon TS-E 17mm f/4L".to_string(),
            95 => "Canon TS-E 24mm f/3.5L II".to_string(),
            103 => "Samyang AF 14mm f/2.8 EF or Rokinon Lens".to_string(),
            106 => "Rokinon SP / Samyang XP 35mm f/1.2".to_string(),
            112 => "Sigma 28mm f/1.5 FF High-speed Prime or other Sigma Lens".to_string(),
            117 => "Tamron 35-150mm f/2.8-4.0 Di VC OSD (A043) or other Tamron Lens".to_string(),
            124 => "Canon MP-E 65mm f/2.8 1-5x Macro Photo".to_string(),
            125 => "Canon TS-E 24mm f/3.5L".to_string(),
            126 => "Canon TS-E 45mm f/2.8".to_string(),
            127 => "Canon TS-E 90mm f/2.8 or Tamron Lens".to_string(),
            129 => "Canon EF 300mm f/2.8L USM".to_string(),
            130 => "Canon EF 50mm f/1.0L USM".to_string(),
            131 => "Canon EF 28-80mm f/2.8-4L USM or Sigma Lens".to_string(),
            132 => "Canon EF 1200mm f/5.6L USM".to_string(),
            134 => "Canon EF 600mm f/4L IS USM".to_string(),
            135 => "Canon EF 200mm f/1.8L USM".to_string(),
            136 => "Canon EF 300mm f/2.8L USM".to_string(),
            137 => "Canon EF 85mm f/1.2L USM or Sigma or Tamron Lens".to_string(),
            138 => "Canon EF 28-80mm f/2.8-4L".to_string(),
            139 => "Canon EF 400mm f/2.8L USM".to_string(),
            140 => "Canon EF 500mm f/4.5L USM".to_string(),
            141 => "Canon EF 500mm f/4.5L USM".to_string(),
            142 => "Canon EF 300mm f/2.8L IS USM".to_string(),
            143 => "Canon EF 500mm f/4L IS USM or Sigma Lens".to_string(),
            144 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            145 => "Canon EF 100-300mm f/4.5-5.6 USM".to_string(),
            146 => "Canon EF 70-210mm f/3.5-4.5 USM".to_string(),
            147 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            148 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            149 => "Canon EF 100mm f/2 USM".to_string(),
            150 => "Canon EF 14mm f/2.8L USM or Sigma Lens".to_string(),
            151 => "Canon EF 200mm f/2.8L USM".to_string(),
            152 => "Canon EF 300mm f/4L IS USM or Sigma Lens".to_string(),
            153 => "Canon EF 35-350mm f/3.5-5.6L USM or Sigma or Tamron Lens".to_string(),
            154 => "Canon EF 20mm f/2.8 USM or Zeiss Lens".to_string(),
            155 => "Canon EF 85mm f/1.8 USM or Sigma Lens".to_string(),
            156 => "Canon EF 28-105mm f/3.5-4.5 USM or Tamron Lens".to_string(),
            160 => "Canon EF 20-35mm f/3.5-4.5 USM or Tamron or Tokina Lens".to_string(),
            161 => "Canon EF 28-70mm f/2.8L USM or Other Lens".to_string(),
            162 => "Canon EF 200mm f/2.8L USM".to_string(),
            163 => "Canon EF 300mm f/4L".to_string(),
            164 => "Canon EF 400mm f/5.6L".to_string(),
            165 => "Canon EF 70-200mm f/2.8L USM".to_string(),
            166 => "Canon EF 70-200mm f/2.8L USM + 1.4x".to_string(),
            167 => "Canon EF 70-200mm f/2.8L USM + 2x".to_string(),
            168 => "Canon EF 28mm f/1.8 USM or Sigma Lens".to_string(),
            169 => "Canon EF 17-35mm f/2.8L USM or Sigma Lens".to_string(),
            170 => "Canon EF 200mm f/2.8L II USM or Sigma Lens".to_string(),
            171 => "Canon EF 300mm f/4L USM".to_string(),
            172 => "Canon EF 400mm f/5.6L USM or Sigma Lens".to_string(),
            173 => "Canon EF 180mm Macro f/3.5L USM or Sigma Lens".to_string(),
            174 => "Canon EF 135mm f/2L USM or Other Lens".to_string(),
            175 => "Canon EF 400mm f/2.8L USM".to_string(),
            176 => "Canon EF 24-85mm f/3.5-4.5 USM".to_string(),
            177 => "Canon EF 300mm f/4L IS USM".to_string(),
            178 => "Canon EF 28-135mm f/3.5-5.6 IS".to_string(),
            179 => "Canon EF 24mm f/1.4L USM".to_string(),
            180 => "Canon EF 35mm f/1.4L USM or Other Lens".to_string(),
            181 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 1.4x or Sigma Lens".to_string(),
            182 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 2x or Sigma Lens".to_string(),
            183 => "Canon EF 100-400mm f/4.5-5.6L IS USM or Sigma Lens".to_string(),
            184 => "Canon EF 400mm f/2.8L USM + 2x".to_string(),
            185 => "Canon EF 600mm f/4L IS USM".to_string(),
            186 => "Canon EF 70-200mm f/4L USM".to_string(),
            187 => "Canon EF 70-200mm f/4L USM + 1.4x".to_string(),
            188 => "Canon EF 70-200mm f/4L USM + 2x".to_string(),
            189 => "Canon EF 70-200mm f/4L USM + 2.8x".to_string(),
            190 => "Canon EF 100mm f/2.8 Macro USM".to_string(),
            191 => "Canon EF 400mm f/4 DO IS or Sigma Lens".to_string(),
            193 => "Canon EF 35-80mm f/4-5.6 USM".to_string(),
            194 => "Canon EF 80-200mm f/4.5-5.6 USM".to_string(),
            195 => "Canon EF 35-105mm f/4.5-5.6 USM".to_string(),
            196 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            197 => "Canon EF 75-300mm f/4-5.6 IS USM or Sigma Lens".to_string(),
            198 => "Canon EF 50mm f/1.4 USM or Other Lens".to_string(),
            199 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            200 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            201 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            202 => "Canon EF 28-80mm f/3.5-5.6 USM IV".to_string(),
            208 => "Canon EF 22-55mm f/4-5.6 USM".to_string(),
            209 => "Canon EF 55-200mm f/4.5-5.6".to_string(),
            210 => "Canon EF 28-90mm f/4-5.6 USM".to_string(),
            211 => "Canon EF 28-200mm f/3.5-5.6 USM".to_string(),
            212 => "Canon EF 28-105mm f/4-5.6 USM".to_string(),
            213 => "Canon EF 90-300mm f/4.5-5.6 USM or Tamron Lens".to_string(),
            214 => "Canon EF-S 18-55mm f/3.5-5.6 USM".to_string(),
            215 => "Canon EF 55-200mm f/4.5-5.6 II USM".to_string(),
            217 => "Tamron AF 18-270mm f/3.5-6.3 Di II VC PZD".to_string(),
            220 => "Yongnuo YN 50mm f/1.8".to_string(),
            224 => "Canon EF 70-200mm f/2.8L IS USM".to_string(),
            225 => "Canon EF 70-200mm f/2.8L IS USM + 1.4x".to_string(),
            226 => "Canon EF 70-200mm f/2.8L IS USM + 2x".to_string(),
            227 => "Canon EF 70-200mm f/2.8L IS USM + 2.8x".to_string(),
            228 => "Canon EF 28-105mm f/3.5-4.5 USM".to_string(),
            229 => "Canon EF 16-35mm f/2.8L USM".to_string(),
            230 => "Canon EF 24-70mm f/2.8L USM".to_string(),
            231 => "Canon EF 17-40mm f/4L USM or Sigma Lens".to_string(),
            232 => "Canon EF 70-300mm f/4.5-5.6 DO IS USM".to_string(),
            233 => "Canon EF 28-300mm f/3.5-5.6L IS USM".to_string(),
            234 => "Canon EF-S 17-85mm f/4-5.6 IS USM or Tokina Lens".to_string(),
            235 => "Canon EF-S 10-22mm f/3.5-4.5 USM".to_string(),
            236 => "Canon EF-S 60mm f/2.8 Macro USM".to_string(),
            237 => "Canon EF 24-105mm f/4L IS USM".to_string(),
            238 => "Canon EF 70-300mm f/4-5.6 IS USM".to_string(),
            239 => "Canon EF 85mm f/1.2L II USM or Rokinon Lens".to_string(),
            240 => "Canon EF-S 17-55mm f/2.8 IS USM or Sigma Lens".to_string(),
            241 => "Canon EF 50mm f/1.2L USM".to_string(),
            242 => "Canon EF 70-200mm f/4L IS USM".to_string(),
            243 => "Canon EF 70-200mm f/4L IS USM + 1.4x".to_string(),
            244 => "Canon EF 70-200mm f/4L IS USM + 2x".to_string(),
            245 => "Canon EF 70-200mm f/4L IS USM + 2.8x".to_string(),
            246 => "Canon EF 16-35mm f/2.8L II USM".to_string(),
            247 => "Canon EF 14mm f/2.8L II USM".to_string(),
            248 => "Canon EF 200mm f/2L IS USM or Sigma Lens".to_string(),
            249 => "Canon EF 800mm f/5.6L IS USM".to_string(),
            250 => "Canon EF 24mm f/1.4L II USM or Sigma Lens".to_string(),
            251 => "Canon EF 70-200mm f/2.8L IS II USM".to_string(),
            252 => "Canon EF 70-200mm f/2.8L IS II USM + 1.4x".to_string(),
            253 => "Canon EF 70-200mm f/2.8L IS II USM + 2x".to_string(),
            254 => "Canon EF 100mm f/2.8L Macro IS USM or Tamron Lens".to_string(),
            255 => "Sigma 24-105mm f/4 DG OS HSM | A or Other Lens".to_string(),
            368 => "Sigma 14-24mm f/2.8 DG HSM | A or other Sigma Lens".to_string(),
            488 => "Canon EF-S 15-85mm f/3.5-5.6 IS USM".to_string(),
            489 => "Canon EF 70-300mm f/4-5.6L IS USM".to_string(),
            490 => "Canon EF 8-15mm f/4L Fisheye USM".to_string(),
            491 => "Canon EF 300mm f/2.8L IS II USM or Tamron Lens".to_string(),
            492 => "Canon EF 400mm f/2.8L IS II USM".to_string(),
            493 => "Canon EF 500mm f/4L IS II USM or EF 24-105mm f4L IS USM".to_string(),
            494 => "Canon EF 600mm f/4L IS II USM".to_string(),
            495 => "Canon EF 24-70mm f/2.8L II USM or Sigma Lens".to_string(),
            496 => "Canon EF 200-400mm f/4L IS USM".to_string(),
            499 => "Canon EF 200-400mm f/4L IS USM + 1.4x".to_string(),
            502 => "Canon EF 28mm f/2.8 IS USM or Tamron Lens".to_string(),
            503 => "Canon EF 24mm f/2.8 IS USM".to_string(),
            504 => "Canon EF 24-70mm f/4L IS USM".to_string(),
            505 => "Canon EF 35mm f/2 IS USM".to_string(),
            506 => "Canon EF 400mm f/4 DO IS II USM".to_string(),
            507 => "Canon EF 16-35mm f/4L IS USM".to_string(),
            508 => "Canon EF 11-24mm f/4L USM or Tamron Lens".to_string(),
            624 => "Sigma 70-200mm f/2.8 DG OS HSM | S or other Sigma Lens".to_string(),
            747 => "Canon EF 100-400mm f/4.5-5.6L IS II USM or Tamron Lens".to_string(),
            748 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 1.4x or Tamron Lens".to_string(),
            749 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 2x or Tamron Lens".to_string(),
            750 => "Canon EF 35mm f/1.4L II USM or Tamron Lens".to_string(),
            751 => "Canon EF 16-35mm f/2.8L III USM".to_string(),
            752 => "Canon EF 24-105mm f/4L IS II USM".to_string(),
            753 => "Canon EF 85mm f/1.4L IS USM".to_string(),
            754 => "Canon EF 70-200mm f/4L IS II USM".to_string(),
            757 => "Canon EF 400mm f/2.8L IS III USM".to_string(),
            758 => "Canon EF 600mm f/4L IS III USM".to_string(),
            923 => "Meike/SKY 85mm f/1.8 DCM".to_string(),
            1136 => "Sigma 24-70mm f/2.8 DG OS HSM | A".to_string(),
            4142 => "Canon EF-S 18-135mm f/3.5-5.6 IS STM".to_string(),
            4143 => "Canon EF-M 18-55mm f/3.5-5.6 IS STM or Tamron Lens".to_string(),
            4144 => "Canon EF 40mm f/2.8 STM".to_string(),
            4145 => "Canon EF-M 22mm f/2 STM".to_string(),
            4146 => "Canon EF-S 18-55mm f/3.5-5.6 IS STM".to_string(),
            4147 => "Canon EF-M 11-22mm f/4-5.6 IS STM".to_string(),
            4148 => "Canon EF-S 55-250mm f/4-5.6 IS STM".to_string(),
            4149 => "Canon EF-M 55-200mm f/4.5-6.3 IS STM".to_string(),
            4150 => "Canon EF-S 10-18mm f/4.5-5.6 IS STM".to_string(),
            4152 => "Canon EF 24-105mm f/3.5-5.6 IS STM".to_string(),
            4153 => "Canon EF-M 15-45mm f/3.5-6.3 IS STM".to_string(),
            4154 => "Canon EF-S 24mm f/2.8 STM".to_string(),
            4155 => "Canon EF-M 28mm f/3.5 Macro IS STM".to_string(),
            4156 => "Canon EF 50mm f/1.8 STM".to_string(),
            4157 => "Canon EF-M 18-150mm f/3.5-6.3 IS STM".to_string(),
            4158 => "Canon EF-S 18-55mm f/4-5.6 IS STM".to_string(),
            4159 => "Canon EF-M 32mm f/1.4 STM".to_string(),
            4160 => "Canon EF-S 35mm f/2.8 Macro IS STM".to_string(),
            4208 => "Sigma 56mm f/1.4 DC DN | C or other Sigma Lens".to_string(),
            4976 => "Sigma 16-300mm F3.5-6.7 DC OS | C (025)".to_string(),
            6512 => "Sigma 12mm F1.4 DC | C".to_string(),
            36910 => "Canon EF 70-300mm f/4-5.6 IS II USM".to_string(),
            36912 => "Canon EF-S 18-135mm f/3.5-5.6 IS USM".to_string(),
            61182 => "Canon RF 50mm F1.2L USM or other Canon RF Lens".to_string(),
            61491 => "Canon CN-E 14mm T3.1 L F".to_string(),
            61492 => "Canon CN-E 24mm T1.5 L F".to_string(),
            61494 => "Canon CN-E 85mm T1.3 L F".to_string(),
            61495 => "Canon CN-E 135mm T2.2 L F".to_string(),
            61496 => "Canon CN-E 35mm T1.5 L F".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("LensType", 0xea, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0xec, bo) {
        dm.push(("MinFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MinFocalLength", 0xec, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0xee, bo) {
        dm.push(("MaxFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MaxFocalLength", 0xee, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(text) = text_at(data, 0x19b, 6, true) {
        tags.push(mk("FirmwareVersion", 0x19b, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x1db, bo) {
        dm.push(("FileIndex".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val + 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("FileIndex", 0x1db, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x1e7, bo) {
        dm.push(("DirectoryIndex".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("DirectoryIndex", 0x1e7, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(sub) = data.get(0x2fb..) {
        tags.extend(canon_psinfo2(sub, model, bo, file_type, format, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::CameraInfo650D` -- FORMAT int8u, FIRST_ENTRY 0.
fn canon_camerainfo650d(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = u8_at(data, 0x3) {
        dm.push(("FNumber".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(($val-8)/16*log(2))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("sprintf(\"%.2g\",$val)", &cv, &ctx) { cv = x; }
            tags.push(mk("FNumber", 0x3, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x4) {
        dm.push(("ExposureTime".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(4*log(2)*(1-Image::ExifTool::Canon::CanonEv($val-24)))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("Image::ExifTool::Exif::PrintExposureTime($val)", &cv, &ctx) { cv = x; }
            tags.push(mk("ExposureTime", 0x4, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x6) {
        dm.push(("ISO".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("100*exp(($val/8-9)*log(2))", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("sprintf(\"%.0f\",$val)", &cv, &ctx) { cv = x; }
        tags.push(mk("ISO", 0x6, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x1b) {
        dm.push(("CameraTemperature".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 128", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
        tags.push(mk("CameraTemperature", 0x1b, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x23, bo) {
        dm.push(("FocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
            tags.push(mk("FocalLength", 0x23, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x7d) {
        dm.push(("CameraOrientation".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Horizontal (normal)".to_string(),
            1 => "Rotate 90 CW".to_string(),
            2 => "Rotate 270 CW".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("CameraOrientation", 0x7d, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x8c, bo) {
        dm.push(("FocusDistanceUpper".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceUpper", 0x8c, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x8e, bo) {
        dm.push(("FocusDistanceLower".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceLower", 0x8e, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0xbc, bo) {
        dm.push(("WhiteBalance".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Auto".to_string(),
            1 => "Daylight".to_string(),
            2 => "Cloudy".to_string(),
            3 => "Tungsten".to_string(),
            4 => "Fluorescent".to_string(),
            5 => "Flash".to_string(),
            6 => "Custom".to_string(),
            7 => "Black & White".to_string(),
            8 => "Shade".to_string(),
            9 => "Manual Temperature (Kelvin)".to_string(),
            10 => "PC Set1".to_string(),
            11 => "PC Set2".to_string(),
            12 => "PC Set3".to_string(),
            14 => "Daylight Fluorescent".to_string(),
            15 => "Custom 1".to_string(),
            16 => "Custom 2".to_string(),
            17 => "Underwater".to_string(),
            18 => "Custom 3".to_string(),
            19 => "Custom 4".to_string(),
            20 => "PC Set4".to_string(),
            21 => "PC Set5".to_string(),
            23 => "Auto (ambience priority)".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("WhiteBalance", 0xbc, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0xc0, bo) {
        dm.push(("ColorTemperature".to_string(), f64::from(v)));
        tags.push(mk("ColorTemperature", 0xc0, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0xf4) {
        dm.push(("PictureStyle".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "None".to_string(),
            1 => "Standard".to_string(),
            2 => "Portrait".to_string(),
            3 => "High Saturation".to_string(),
            4 => "Adobe RGB".to_string(),
            5 => "Low Saturation".to_string(),
            6 => "CM Set 1".to_string(),
            7 => "CM Set 2".to_string(),
            33 => "User Def. 1".to_string(),
            34 => "User Def. 2".to_string(),
            35 => "User Def. 3".to_string(),
            65 => "PC 1".to_string(),
            66 => "PC 2".to_string(),
            67 => "PC 3".to_string(),
            129 => "Standard".to_string(),
            130 => "Portrait".to_string(),
            131 => "Landscape".to_string(),
            132 => "Neutral".to_string(),
            133 => "Faithful".to_string(),
            134 => "Monochrome".to_string(),
            135 => "Auto".to_string(),
            136 => "Fine Detail".to_string(),
            255 => "n/a".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("PictureStyle", 0xf4, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x127, bo) {
        dm.push(("LensType".to_string(), f64::from(v)));
        let s = match v as i64 {
            -1 => "n/a".to_string(),
            1 => "Canon EF 50mm f/1.8".to_string(),
            2 => "Canon EF 28mm f/2.8 or Sigma Lens".to_string(),
            3 => "Canon EF 135mm f/2.8 Soft".to_string(),
            4 => "Canon EF 35-105mm f/3.5-4.5 or Sigma Lens".to_string(),
            5 => "Canon EF 35-70mm f/3.5-4.5".to_string(),
            6 => "Canon EF 28-70mm f/3.5-4.5 or Sigma or Tokina Lens".to_string(),
            7 => "Canon EF 100-300mm f/5.6L".to_string(),
            8 => "Canon EF 100-300mm f/5.6 or Sigma or Tokina Lens".to_string(),
            9 => "Canon EF 70-210mm f/4".to_string(),
            10 => "Canon EF 50mm f/2.5 Macro or Sigma Lens".to_string(),
            11 => "Canon EF 35mm f/2".to_string(),
            13 => "Canon EF 15mm f/2.8 Fisheye".to_string(),
            14 => "Canon EF 50-200mm f/3.5-4.5L".to_string(),
            15 => "Canon EF 50-200mm f/3.5-4.5".to_string(),
            16 => "Canon EF 35-135mm f/3.5-4.5".to_string(),
            17 => "Canon EF 35-70mm f/3.5-4.5A".to_string(),
            18 => "Canon EF 28-70mm f/3.5-4.5".to_string(),
            20 => "Canon EF 100-200mm f/4.5A".to_string(),
            21 => "Canon EF 80-200mm f/2.8L".to_string(),
            22 => "Canon EF 20-35mm f/2.8L or Tokina Lens".to_string(),
            23 => "Canon EF 35-105mm f/3.5-4.5".to_string(),
            24 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            25 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            26 => "Canon EF 100mm f/2.8 Macro or Other Lens".to_string(),
            27 => "Canon EF 35-80mm f/4-5.6".to_string(),
            28 => "Canon EF 80-200mm f/4.5-5.6 or Tamron Lens".to_string(),
            29 => "Canon EF 50mm f/1.8 II".to_string(),
            30 => "Canon EF 35-105mm f/4.5-5.6".to_string(),
            31 => "Canon EF 75-300mm f/4-5.6 or Tamron Lens".to_string(),
            32 => "Canon EF 24mm f/2.8 or Sigma Lens".to_string(),
            33 => "Voigtlander or Carl Zeiss Lens".to_string(),
            35 => "Canon EF 35-80mm f/4-5.6".to_string(),
            36 => "Canon EF 38-76mm f/4.5-5.6".to_string(),
            37 => "Canon EF 35-80mm f/4-5.6 or Tamron Lens".to_string(),
            38 => "Canon EF 80-200mm f/4.5-5.6 II".to_string(),
            39 => "Canon EF 75-300mm f/4-5.6".to_string(),
            40 => "Canon EF 28-80mm f/3.5-5.6".to_string(),
            41 => "Canon EF 28-90mm f/4-5.6".to_string(),
            42 => "Canon EF 28-200mm f/3.5-5.6 or Tamron Lens".to_string(),
            43 => "Canon EF 28-105mm f/4-5.6".to_string(),
            44 => "Canon EF 90-300mm f/4.5-5.6".to_string(),
            45 => "Canon EF-S 18-55mm f/3.5-5.6 [II]".to_string(),
            46 => "Canon EF 28-90mm f/4-5.6".to_string(),
            47 => "Zeiss Milvus 35mm f/2 or 50mm f/2".to_string(),
            48 => "Canon EF-S 18-55mm f/3.5-5.6 IS".to_string(),
            49 => "Canon EF-S 55-250mm f/4-5.6 IS".to_string(),
            50 => "Canon EF-S 18-200mm f/3.5-5.6 IS".to_string(),
            51 => "Canon EF-S 18-135mm f/3.5-5.6 IS".to_string(),
            52 => "Canon EF-S 18-55mm f/3.5-5.6 IS II".to_string(),
            53 => "Canon EF-S 18-55mm f/3.5-5.6 III".to_string(),
            54 => "Canon EF-S 55-250mm f/4-5.6 IS II".to_string(),
            60 => "Irix 11mm f/4 or 15mm f/2.4".to_string(),
            63 => "Irix 30mm F1.4 Dragonfly".to_string(),
            80 => "Canon TS-E 50mm f/2.8L Macro".to_string(),
            81 => "Canon TS-E 90mm f/2.8L Macro".to_string(),
            82 => "Canon TS-E 135mm f/4L Macro".to_string(),
            94 => "Canon TS-E 17mm f/4L".to_string(),
            95 => "Canon TS-E 24mm f/3.5L II".to_string(),
            103 => "Samyang AF 14mm f/2.8 EF or Rokinon Lens".to_string(),
            106 => "Rokinon SP / Samyang XP 35mm f/1.2".to_string(),
            112 => "Sigma 28mm f/1.5 FF High-speed Prime or other Sigma Lens".to_string(),
            117 => "Tamron 35-150mm f/2.8-4.0 Di VC OSD (A043) or other Tamron Lens".to_string(),
            124 => "Canon MP-E 65mm f/2.8 1-5x Macro Photo".to_string(),
            125 => "Canon TS-E 24mm f/3.5L".to_string(),
            126 => "Canon TS-E 45mm f/2.8".to_string(),
            127 => "Canon TS-E 90mm f/2.8 or Tamron Lens".to_string(),
            129 => "Canon EF 300mm f/2.8L USM".to_string(),
            130 => "Canon EF 50mm f/1.0L USM".to_string(),
            131 => "Canon EF 28-80mm f/2.8-4L USM or Sigma Lens".to_string(),
            132 => "Canon EF 1200mm f/5.6L USM".to_string(),
            134 => "Canon EF 600mm f/4L IS USM".to_string(),
            135 => "Canon EF 200mm f/1.8L USM".to_string(),
            136 => "Canon EF 300mm f/2.8L USM".to_string(),
            137 => "Canon EF 85mm f/1.2L USM or Sigma or Tamron Lens".to_string(),
            138 => "Canon EF 28-80mm f/2.8-4L".to_string(),
            139 => "Canon EF 400mm f/2.8L USM".to_string(),
            140 => "Canon EF 500mm f/4.5L USM".to_string(),
            141 => "Canon EF 500mm f/4.5L USM".to_string(),
            142 => "Canon EF 300mm f/2.8L IS USM".to_string(),
            143 => "Canon EF 500mm f/4L IS USM or Sigma Lens".to_string(),
            144 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            145 => "Canon EF 100-300mm f/4.5-5.6 USM".to_string(),
            146 => "Canon EF 70-210mm f/3.5-4.5 USM".to_string(),
            147 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            148 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            149 => "Canon EF 100mm f/2 USM".to_string(),
            150 => "Canon EF 14mm f/2.8L USM or Sigma Lens".to_string(),
            151 => "Canon EF 200mm f/2.8L USM".to_string(),
            152 => "Canon EF 300mm f/4L IS USM or Sigma Lens".to_string(),
            153 => "Canon EF 35-350mm f/3.5-5.6L USM or Sigma or Tamron Lens".to_string(),
            154 => "Canon EF 20mm f/2.8 USM or Zeiss Lens".to_string(),
            155 => "Canon EF 85mm f/1.8 USM or Sigma Lens".to_string(),
            156 => "Canon EF 28-105mm f/3.5-4.5 USM or Tamron Lens".to_string(),
            160 => "Canon EF 20-35mm f/3.5-4.5 USM or Tamron or Tokina Lens".to_string(),
            161 => "Canon EF 28-70mm f/2.8L USM or Other Lens".to_string(),
            162 => "Canon EF 200mm f/2.8L USM".to_string(),
            163 => "Canon EF 300mm f/4L".to_string(),
            164 => "Canon EF 400mm f/5.6L".to_string(),
            165 => "Canon EF 70-200mm f/2.8L USM".to_string(),
            166 => "Canon EF 70-200mm f/2.8L USM + 1.4x".to_string(),
            167 => "Canon EF 70-200mm f/2.8L USM + 2x".to_string(),
            168 => "Canon EF 28mm f/1.8 USM or Sigma Lens".to_string(),
            169 => "Canon EF 17-35mm f/2.8L USM or Sigma Lens".to_string(),
            170 => "Canon EF 200mm f/2.8L II USM or Sigma Lens".to_string(),
            171 => "Canon EF 300mm f/4L USM".to_string(),
            172 => "Canon EF 400mm f/5.6L USM or Sigma Lens".to_string(),
            173 => "Canon EF 180mm Macro f/3.5L USM or Sigma Lens".to_string(),
            174 => "Canon EF 135mm f/2L USM or Other Lens".to_string(),
            175 => "Canon EF 400mm f/2.8L USM".to_string(),
            176 => "Canon EF 24-85mm f/3.5-4.5 USM".to_string(),
            177 => "Canon EF 300mm f/4L IS USM".to_string(),
            178 => "Canon EF 28-135mm f/3.5-5.6 IS".to_string(),
            179 => "Canon EF 24mm f/1.4L USM".to_string(),
            180 => "Canon EF 35mm f/1.4L USM or Other Lens".to_string(),
            181 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 1.4x or Sigma Lens".to_string(),
            182 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 2x or Sigma Lens".to_string(),
            183 => "Canon EF 100-400mm f/4.5-5.6L IS USM or Sigma Lens".to_string(),
            184 => "Canon EF 400mm f/2.8L USM + 2x".to_string(),
            185 => "Canon EF 600mm f/4L IS USM".to_string(),
            186 => "Canon EF 70-200mm f/4L USM".to_string(),
            187 => "Canon EF 70-200mm f/4L USM + 1.4x".to_string(),
            188 => "Canon EF 70-200mm f/4L USM + 2x".to_string(),
            189 => "Canon EF 70-200mm f/4L USM + 2.8x".to_string(),
            190 => "Canon EF 100mm f/2.8 Macro USM".to_string(),
            191 => "Canon EF 400mm f/4 DO IS or Sigma Lens".to_string(),
            193 => "Canon EF 35-80mm f/4-5.6 USM".to_string(),
            194 => "Canon EF 80-200mm f/4.5-5.6 USM".to_string(),
            195 => "Canon EF 35-105mm f/4.5-5.6 USM".to_string(),
            196 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            197 => "Canon EF 75-300mm f/4-5.6 IS USM or Sigma Lens".to_string(),
            198 => "Canon EF 50mm f/1.4 USM or Other Lens".to_string(),
            199 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            200 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            201 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            202 => "Canon EF 28-80mm f/3.5-5.6 USM IV".to_string(),
            208 => "Canon EF 22-55mm f/4-5.6 USM".to_string(),
            209 => "Canon EF 55-200mm f/4.5-5.6".to_string(),
            210 => "Canon EF 28-90mm f/4-5.6 USM".to_string(),
            211 => "Canon EF 28-200mm f/3.5-5.6 USM".to_string(),
            212 => "Canon EF 28-105mm f/4-5.6 USM".to_string(),
            213 => "Canon EF 90-300mm f/4.5-5.6 USM or Tamron Lens".to_string(),
            214 => "Canon EF-S 18-55mm f/3.5-5.6 USM".to_string(),
            215 => "Canon EF 55-200mm f/4.5-5.6 II USM".to_string(),
            217 => "Tamron AF 18-270mm f/3.5-6.3 Di II VC PZD".to_string(),
            220 => "Yongnuo YN 50mm f/1.8".to_string(),
            224 => "Canon EF 70-200mm f/2.8L IS USM".to_string(),
            225 => "Canon EF 70-200mm f/2.8L IS USM + 1.4x".to_string(),
            226 => "Canon EF 70-200mm f/2.8L IS USM + 2x".to_string(),
            227 => "Canon EF 70-200mm f/2.8L IS USM + 2.8x".to_string(),
            228 => "Canon EF 28-105mm f/3.5-4.5 USM".to_string(),
            229 => "Canon EF 16-35mm f/2.8L USM".to_string(),
            230 => "Canon EF 24-70mm f/2.8L USM".to_string(),
            231 => "Canon EF 17-40mm f/4L USM or Sigma Lens".to_string(),
            232 => "Canon EF 70-300mm f/4.5-5.6 DO IS USM".to_string(),
            233 => "Canon EF 28-300mm f/3.5-5.6L IS USM".to_string(),
            234 => "Canon EF-S 17-85mm f/4-5.6 IS USM or Tokina Lens".to_string(),
            235 => "Canon EF-S 10-22mm f/3.5-4.5 USM".to_string(),
            236 => "Canon EF-S 60mm f/2.8 Macro USM".to_string(),
            237 => "Canon EF 24-105mm f/4L IS USM".to_string(),
            238 => "Canon EF 70-300mm f/4-5.6 IS USM".to_string(),
            239 => "Canon EF 85mm f/1.2L II USM or Rokinon Lens".to_string(),
            240 => "Canon EF-S 17-55mm f/2.8 IS USM or Sigma Lens".to_string(),
            241 => "Canon EF 50mm f/1.2L USM".to_string(),
            242 => "Canon EF 70-200mm f/4L IS USM".to_string(),
            243 => "Canon EF 70-200mm f/4L IS USM + 1.4x".to_string(),
            244 => "Canon EF 70-200mm f/4L IS USM + 2x".to_string(),
            245 => "Canon EF 70-200mm f/4L IS USM + 2.8x".to_string(),
            246 => "Canon EF 16-35mm f/2.8L II USM".to_string(),
            247 => "Canon EF 14mm f/2.8L II USM".to_string(),
            248 => "Canon EF 200mm f/2L IS USM or Sigma Lens".to_string(),
            249 => "Canon EF 800mm f/5.6L IS USM".to_string(),
            250 => "Canon EF 24mm f/1.4L II USM or Sigma Lens".to_string(),
            251 => "Canon EF 70-200mm f/2.8L IS II USM".to_string(),
            252 => "Canon EF 70-200mm f/2.8L IS II USM + 1.4x".to_string(),
            253 => "Canon EF 70-200mm f/2.8L IS II USM + 2x".to_string(),
            254 => "Canon EF 100mm f/2.8L Macro IS USM or Tamron Lens".to_string(),
            255 => "Sigma 24-105mm f/4 DG OS HSM | A or Other Lens".to_string(),
            368 => "Sigma 14-24mm f/2.8 DG HSM | A or other Sigma Lens".to_string(),
            488 => "Canon EF-S 15-85mm f/3.5-5.6 IS USM".to_string(),
            489 => "Canon EF 70-300mm f/4-5.6L IS USM".to_string(),
            490 => "Canon EF 8-15mm f/4L Fisheye USM".to_string(),
            491 => "Canon EF 300mm f/2.8L IS II USM or Tamron Lens".to_string(),
            492 => "Canon EF 400mm f/2.8L IS II USM".to_string(),
            493 => "Canon EF 500mm f/4L IS II USM or EF 24-105mm f4L IS USM".to_string(),
            494 => "Canon EF 600mm f/4L IS II USM".to_string(),
            495 => "Canon EF 24-70mm f/2.8L II USM or Sigma Lens".to_string(),
            496 => "Canon EF 200-400mm f/4L IS USM".to_string(),
            499 => "Canon EF 200-400mm f/4L IS USM + 1.4x".to_string(),
            502 => "Canon EF 28mm f/2.8 IS USM or Tamron Lens".to_string(),
            503 => "Canon EF 24mm f/2.8 IS USM".to_string(),
            504 => "Canon EF 24-70mm f/4L IS USM".to_string(),
            505 => "Canon EF 35mm f/2 IS USM".to_string(),
            506 => "Canon EF 400mm f/4 DO IS II USM".to_string(),
            507 => "Canon EF 16-35mm f/4L IS USM".to_string(),
            508 => "Canon EF 11-24mm f/4L USM or Tamron Lens".to_string(),
            624 => "Sigma 70-200mm f/2.8 DG OS HSM | S or other Sigma Lens".to_string(),
            747 => "Canon EF 100-400mm f/4.5-5.6L IS II USM or Tamron Lens".to_string(),
            748 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 1.4x or Tamron Lens".to_string(),
            749 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 2x or Tamron Lens".to_string(),
            750 => "Canon EF 35mm f/1.4L II USM or Tamron Lens".to_string(),
            751 => "Canon EF 16-35mm f/2.8L III USM".to_string(),
            752 => "Canon EF 24-105mm f/4L IS II USM".to_string(),
            753 => "Canon EF 85mm f/1.4L IS USM".to_string(),
            754 => "Canon EF 70-200mm f/4L IS II USM".to_string(),
            757 => "Canon EF 400mm f/2.8L IS III USM".to_string(),
            758 => "Canon EF 600mm f/4L IS III USM".to_string(),
            923 => "Meike/SKY 85mm f/1.8 DCM".to_string(),
            1136 => "Sigma 24-70mm f/2.8 DG OS HSM | A".to_string(),
            4142 => "Canon EF-S 18-135mm f/3.5-5.6 IS STM".to_string(),
            4143 => "Canon EF-M 18-55mm f/3.5-5.6 IS STM or Tamron Lens".to_string(),
            4144 => "Canon EF 40mm f/2.8 STM".to_string(),
            4145 => "Canon EF-M 22mm f/2 STM".to_string(),
            4146 => "Canon EF-S 18-55mm f/3.5-5.6 IS STM".to_string(),
            4147 => "Canon EF-M 11-22mm f/4-5.6 IS STM".to_string(),
            4148 => "Canon EF-S 55-250mm f/4-5.6 IS STM".to_string(),
            4149 => "Canon EF-M 55-200mm f/4.5-6.3 IS STM".to_string(),
            4150 => "Canon EF-S 10-18mm f/4.5-5.6 IS STM".to_string(),
            4152 => "Canon EF 24-105mm f/3.5-5.6 IS STM".to_string(),
            4153 => "Canon EF-M 15-45mm f/3.5-6.3 IS STM".to_string(),
            4154 => "Canon EF-S 24mm f/2.8 STM".to_string(),
            4155 => "Canon EF-M 28mm f/3.5 Macro IS STM".to_string(),
            4156 => "Canon EF 50mm f/1.8 STM".to_string(),
            4157 => "Canon EF-M 18-150mm f/3.5-6.3 IS STM".to_string(),
            4158 => "Canon EF-S 18-55mm f/4-5.6 IS STM".to_string(),
            4159 => "Canon EF-M 32mm f/1.4 STM".to_string(),
            4160 => "Canon EF-S 35mm f/2.8 Macro IS STM".to_string(),
            4208 => "Sigma 56mm f/1.4 DC DN | C or other Sigma Lens".to_string(),
            4976 => "Sigma 16-300mm F3.5-6.7 DC OS | C (025)".to_string(),
            6512 => "Sigma 12mm F1.4 DC | C".to_string(),
            36910 => "Canon EF 70-300mm f/4-5.6 IS II USM".to_string(),
            36912 => "Canon EF-S 18-135mm f/3.5-5.6 IS USM".to_string(),
            61182 => "Canon RF 50mm F1.2L USM or other Canon RF Lens".to_string(),
            61491 => "Canon CN-E 14mm T3.1 L F".to_string(),
            61492 => "Canon CN-E 24mm T1.5 L F".to_string(),
            61494 => "Canon CN-E 85mm T1.3 L F".to_string(),
            61495 => "Canon CN-E 135mm T2.2 L F".to_string(),
            61496 => "Canon CN-E 35mm T1.5 L F".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("LensType", 0x127, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x129, bo) {
        dm.push(("MinFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MinFocalLength", 0x129, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x12b, bo) {
        dm.push(("MaxFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MaxFocalLength", 0x12b, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if MODEL_RE_8.is_match(model) {
        if let Some(text) = text_at(data, 0x21b, 6, true) {
            tags.push(mk("FirmwareVersion", 0x21b, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
        }
    }
    if MODEL_RE_9.is_match(model) {
        if let Some(text) = text_at(data, 0x220, 6, true) {
            tags.push(mk("FirmwareVersion", 0x220, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
        }
    }
    if MODEL_RE_8.is_match(model) {
        if let Some(v) = u32_at(data, 0x270, bo) {
            dm.push(("FileIndex".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("$val + 1", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            tags.push(mk("FileIndex", 0x270, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if MODEL_RE_9.is_match(model) {
        if let Some(v) = u32_at(data, 0x274, bo) {
            dm.push(("FileIndex".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("$val + 1", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            tags.push(mk("FileIndex", 0x274, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if MODEL_RE_8.is_match(model) {
        if let Some(v) = u32_at(data, 0x27c, bo) {
            dm.push(("DirectoryIndex".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("$val - 1", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            tags.push(mk("DirectoryIndex", 0x27c, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if MODEL_RE_9.is_match(model) {
        if let Some(v) = u32_at(data, 0x280, bo) {
            dm.push(("DirectoryIndex".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("$val - 1", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            tags.push(mk("DirectoryIndex", 0x280, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(sub) = data.get(0x390..) {
        tags.extend(canon_psinfo2(sub, model, bo, file_type, format, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::CameraInfo750D` -- FORMAT int8u, FIRST_ENTRY 0.
fn canon_camerainfo750d(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = u8_at(data, 0x3) {
        dm.push(("FNumber".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(($val-8)/16*log(2))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("sprintf(\"%.2g\",$val)", &cv, &ctx) { cv = x; }
            tags.push(mk("FNumber", 0x3, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x4) {
        dm.push(("ExposureTime".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(4*log(2)*(1-Image::ExifTool::Canon::CanonEv($val-24)))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("Image::ExifTool::Exif::PrintExposureTime($val)", &cv, &ctx) { cv = x; }
            tags.push(mk("ExposureTime", 0x4, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x6) {
        dm.push(("ISO".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("100*exp(($val/8-9)*log(2))", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("sprintf(\"%.0f\",$val)", &cv, &ctx) { cv = x; }
        tags.push(mk("ISO", 0x6, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x1b) {
        dm.push(("CameraTemperature".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 128", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
        tags.push(mk("CameraTemperature", 0x1b, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x23, bo) {
        dm.push(("FocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
            tags.push(mk("FocalLength", 0x23, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x96) {
        dm.push(("CameraOrientation".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Horizontal (normal)".to_string(),
            1 => "Rotate 90 CW".to_string(),
            2 => "Rotate 270 CW".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("CameraOrientation", 0x96, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0xa5, bo) {
        dm.push(("FocusDistanceUpper".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceUpper", 0xa5, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0xa7, bo) {
        dm.push(("FocusDistanceLower".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceLower", 0xa7, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x131, bo) {
        dm.push(("WhiteBalance".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Auto".to_string(),
            1 => "Daylight".to_string(),
            2 => "Cloudy".to_string(),
            3 => "Tungsten".to_string(),
            4 => "Fluorescent".to_string(),
            5 => "Flash".to_string(),
            6 => "Custom".to_string(),
            7 => "Black & White".to_string(),
            8 => "Shade".to_string(),
            9 => "Manual Temperature (Kelvin)".to_string(),
            10 => "PC Set1".to_string(),
            11 => "PC Set2".to_string(),
            12 => "PC Set3".to_string(),
            14 => "Daylight Fluorescent".to_string(),
            15 => "Custom 1".to_string(),
            16 => "Custom 2".to_string(),
            17 => "Underwater".to_string(),
            18 => "Custom 3".to_string(),
            19 => "Custom 4".to_string(),
            20 => "PC Set4".to_string(),
            21 => "PC Set5".to_string(),
            23 => "Auto (ambience priority)".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("WhiteBalance", 0x131, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x135, bo) {
        dm.push(("ColorTemperature".to_string(), f64::from(v)));
        tags.push(mk("ColorTemperature", 0x135, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x169) {
        dm.push(("PictureStyle".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "None".to_string(),
            1 => "Standard".to_string(),
            2 => "Portrait".to_string(),
            3 => "High Saturation".to_string(),
            4 => "Adobe RGB".to_string(),
            5 => "Low Saturation".to_string(),
            6 => "CM Set 1".to_string(),
            7 => "CM Set 2".to_string(),
            33 => "User Def. 1".to_string(),
            34 => "User Def. 2".to_string(),
            35 => "User Def. 3".to_string(),
            65 => "PC 1".to_string(),
            66 => "PC 2".to_string(),
            67 => "PC 3".to_string(),
            129 => "Standard".to_string(),
            130 => "Portrait".to_string(),
            131 => "Landscape".to_string(),
            132 => "Neutral".to_string(),
            133 => "Faithful".to_string(),
            134 => "Monochrome".to_string(),
            135 => "Auto".to_string(),
            136 => "Fine Detail".to_string(),
            255 => "n/a".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("PictureStyle", 0x169, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x184, bo) {
        dm.push(("LensType".to_string(), f64::from(v)));
        let s = match v as i64 {
            -1 => "n/a".to_string(),
            1 => "Canon EF 50mm f/1.8".to_string(),
            2 => "Canon EF 28mm f/2.8 or Sigma Lens".to_string(),
            3 => "Canon EF 135mm f/2.8 Soft".to_string(),
            4 => "Canon EF 35-105mm f/3.5-4.5 or Sigma Lens".to_string(),
            5 => "Canon EF 35-70mm f/3.5-4.5".to_string(),
            6 => "Canon EF 28-70mm f/3.5-4.5 or Sigma or Tokina Lens".to_string(),
            7 => "Canon EF 100-300mm f/5.6L".to_string(),
            8 => "Canon EF 100-300mm f/5.6 or Sigma or Tokina Lens".to_string(),
            9 => "Canon EF 70-210mm f/4".to_string(),
            10 => "Canon EF 50mm f/2.5 Macro or Sigma Lens".to_string(),
            11 => "Canon EF 35mm f/2".to_string(),
            13 => "Canon EF 15mm f/2.8 Fisheye".to_string(),
            14 => "Canon EF 50-200mm f/3.5-4.5L".to_string(),
            15 => "Canon EF 50-200mm f/3.5-4.5".to_string(),
            16 => "Canon EF 35-135mm f/3.5-4.5".to_string(),
            17 => "Canon EF 35-70mm f/3.5-4.5A".to_string(),
            18 => "Canon EF 28-70mm f/3.5-4.5".to_string(),
            20 => "Canon EF 100-200mm f/4.5A".to_string(),
            21 => "Canon EF 80-200mm f/2.8L".to_string(),
            22 => "Canon EF 20-35mm f/2.8L or Tokina Lens".to_string(),
            23 => "Canon EF 35-105mm f/3.5-4.5".to_string(),
            24 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            25 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            26 => "Canon EF 100mm f/2.8 Macro or Other Lens".to_string(),
            27 => "Canon EF 35-80mm f/4-5.6".to_string(),
            28 => "Canon EF 80-200mm f/4.5-5.6 or Tamron Lens".to_string(),
            29 => "Canon EF 50mm f/1.8 II".to_string(),
            30 => "Canon EF 35-105mm f/4.5-5.6".to_string(),
            31 => "Canon EF 75-300mm f/4-5.6 or Tamron Lens".to_string(),
            32 => "Canon EF 24mm f/2.8 or Sigma Lens".to_string(),
            33 => "Voigtlander or Carl Zeiss Lens".to_string(),
            35 => "Canon EF 35-80mm f/4-5.6".to_string(),
            36 => "Canon EF 38-76mm f/4.5-5.6".to_string(),
            37 => "Canon EF 35-80mm f/4-5.6 or Tamron Lens".to_string(),
            38 => "Canon EF 80-200mm f/4.5-5.6 II".to_string(),
            39 => "Canon EF 75-300mm f/4-5.6".to_string(),
            40 => "Canon EF 28-80mm f/3.5-5.6".to_string(),
            41 => "Canon EF 28-90mm f/4-5.6".to_string(),
            42 => "Canon EF 28-200mm f/3.5-5.6 or Tamron Lens".to_string(),
            43 => "Canon EF 28-105mm f/4-5.6".to_string(),
            44 => "Canon EF 90-300mm f/4.5-5.6".to_string(),
            45 => "Canon EF-S 18-55mm f/3.5-5.6 [II]".to_string(),
            46 => "Canon EF 28-90mm f/4-5.6".to_string(),
            47 => "Zeiss Milvus 35mm f/2 or 50mm f/2".to_string(),
            48 => "Canon EF-S 18-55mm f/3.5-5.6 IS".to_string(),
            49 => "Canon EF-S 55-250mm f/4-5.6 IS".to_string(),
            50 => "Canon EF-S 18-200mm f/3.5-5.6 IS".to_string(),
            51 => "Canon EF-S 18-135mm f/3.5-5.6 IS".to_string(),
            52 => "Canon EF-S 18-55mm f/3.5-5.6 IS II".to_string(),
            53 => "Canon EF-S 18-55mm f/3.5-5.6 III".to_string(),
            54 => "Canon EF-S 55-250mm f/4-5.6 IS II".to_string(),
            60 => "Irix 11mm f/4 or 15mm f/2.4".to_string(),
            63 => "Irix 30mm F1.4 Dragonfly".to_string(),
            80 => "Canon TS-E 50mm f/2.8L Macro".to_string(),
            81 => "Canon TS-E 90mm f/2.8L Macro".to_string(),
            82 => "Canon TS-E 135mm f/4L Macro".to_string(),
            94 => "Canon TS-E 17mm f/4L".to_string(),
            95 => "Canon TS-E 24mm f/3.5L II".to_string(),
            103 => "Samyang AF 14mm f/2.8 EF or Rokinon Lens".to_string(),
            106 => "Rokinon SP / Samyang XP 35mm f/1.2".to_string(),
            112 => "Sigma 28mm f/1.5 FF High-speed Prime or other Sigma Lens".to_string(),
            117 => "Tamron 35-150mm f/2.8-4.0 Di VC OSD (A043) or other Tamron Lens".to_string(),
            124 => "Canon MP-E 65mm f/2.8 1-5x Macro Photo".to_string(),
            125 => "Canon TS-E 24mm f/3.5L".to_string(),
            126 => "Canon TS-E 45mm f/2.8".to_string(),
            127 => "Canon TS-E 90mm f/2.8 or Tamron Lens".to_string(),
            129 => "Canon EF 300mm f/2.8L USM".to_string(),
            130 => "Canon EF 50mm f/1.0L USM".to_string(),
            131 => "Canon EF 28-80mm f/2.8-4L USM or Sigma Lens".to_string(),
            132 => "Canon EF 1200mm f/5.6L USM".to_string(),
            134 => "Canon EF 600mm f/4L IS USM".to_string(),
            135 => "Canon EF 200mm f/1.8L USM".to_string(),
            136 => "Canon EF 300mm f/2.8L USM".to_string(),
            137 => "Canon EF 85mm f/1.2L USM or Sigma or Tamron Lens".to_string(),
            138 => "Canon EF 28-80mm f/2.8-4L".to_string(),
            139 => "Canon EF 400mm f/2.8L USM".to_string(),
            140 => "Canon EF 500mm f/4.5L USM".to_string(),
            141 => "Canon EF 500mm f/4.5L USM".to_string(),
            142 => "Canon EF 300mm f/2.8L IS USM".to_string(),
            143 => "Canon EF 500mm f/4L IS USM or Sigma Lens".to_string(),
            144 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            145 => "Canon EF 100-300mm f/4.5-5.6 USM".to_string(),
            146 => "Canon EF 70-210mm f/3.5-4.5 USM".to_string(),
            147 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            148 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            149 => "Canon EF 100mm f/2 USM".to_string(),
            150 => "Canon EF 14mm f/2.8L USM or Sigma Lens".to_string(),
            151 => "Canon EF 200mm f/2.8L USM".to_string(),
            152 => "Canon EF 300mm f/4L IS USM or Sigma Lens".to_string(),
            153 => "Canon EF 35-350mm f/3.5-5.6L USM or Sigma or Tamron Lens".to_string(),
            154 => "Canon EF 20mm f/2.8 USM or Zeiss Lens".to_string(),
            155 => "Canon EF 85mm f/1.8 USM or Sigma Lens".to_string(),
            156 => "Canon EF 28-105mm f/3.5-4.5 USM or Tamron Lens".to_string(),
            160 => "Canon EF 20-35mm f/3.5-4.5 USM or Tamron or Tokina Lens".to_string(),
            161 => "Canon EF 28-70mm f/2.8L USM or Other Lens".to_string(),
            162 => "Canon EF 200mm f/2.8L USM".to_string(),
            163 => "Canon EF 300mm f/4L".to_string(),
            164 => "Canon EF 400mm f/5.6L".to_string(),
            165 => "Canon EF 70-200mm f/2.8L USM".to_string(),
            166 => "Canon EF 70-200mm f/2.8L USM + 1.4x".to_string(),
            167 => "Canon EF 70-200mm f/2.8L USM + 2x".to_string(),
            168 => "Canon EF 28mm f/1.8 USM or Sigma Lens".to_string(),
            169 => "Canon EF 17-35mm f/2.8L USM or Sigma Lens".to_string(),
            170 => "Canon EF 200mm f/2.8L II USM or Sigma Lens".to_string(),
            171 => "Canon EF 300mm f/4L USM".to_string(),
            172 => "Canon EF 400mm f/5.6L USM or Sigma Lens".to_string(),
            173 => "Canon EF 180mm Macro f/3.5L USM or Sigma Lens".to_string(),
            174 => "Canon EF 135mm f/2L USM or Other Lens".to_string(),
            175 => "Canon EF 400mm f/2.8L USM".to_string(),
            176 => "Canon EF 24-85mm f/3.5-4.5 USM".to_string(),
            177 => "Canon EF 300mm f/4L IS USM".to_string(),
            178 => "Canon EF 28-135mm f/3.5-5.6 IS".to_string(),
            179 => "Canon EF 24mm f/1.4L USM".to_string(),
            180 => "Canon EF 35mm f/1.4L USM or Other Lens".to_string(),
            181 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 1.4x or Sigma Lens".to_string(),
            182 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 2x or Sigma Lens".to_string(),
            183 => "Canon EF 100-400mm f/4.5-5.6L IS USM or Sigma Lens".to_string(),
            184 => "Canon EF 400mm f/2.8L USM + 2x".to_string(),
            185 => "Canon EF 600mm f/4L IS USM".to_string(),
            186 => "Canon EF 70-200mm f/4L USM".to_string(),
            187 => "Canon EF 70-200mm f/4L USM + 1.4x".to_string(),
            188 => "Canon EF 70-200mm f/4L USM + 2x".to_string(),
            189 => "Canon EF 70-200mm f/4L USM + 2.8x".to_string(),
            190 => "Canon EF 100mm f/2.8 Macro USM".to_string(),
            191 => "Canon EF 400mm f/4 DO IS or Sigma Lens".to_string(),
            193 => "Canon EF 35-80mm f/4-5.6 USM".to_string(),
            194 => "Canon EF 80-200mm f/4.5-5.6 USM".to_string(),
            195 => "Canon EF 35-105mm f/4.5-5.6 USM".to_string(),
            196 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            197 => "Canon EF 75-300mm f/4-5.6 IS USM or Sigma Lens".to_string(),
            198 => "Canon EF 50mm f/1.4 USM or Other Lens".to_string(),
            199 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            200 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            201 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            202 => "Canon EF 28-80mm f/3.5-5.6 USM IV".to_string(),
            208 => "Canon EF 22-55mm f/4-5.6 USM".to_string(),
            209 => "Canon EF 55-200mm f/4.5-5.6".to_string(),
            210 => "Canon EF 28-90mm f/4-5.6 USM".to_string(),
            211 => "Canon EF 28-200mm f/3.5-5.6 USM".to_string(),
            212 => "Canon EF 28-105mm f/4-5.6 USM".to_string(),
            213 => "Canon EF 90-300mm f/4.5-5.6 USM or Tamron Lens".to_string(),
            214 => "Canon EF-S 18-55mm f/3.5-5.6 USM".to_string(),
            215 => "Canon EF 55-200mm f/4.5-5.6 II USM".to_string(),
            217 => "Tamron AF 18-270mm f/3.5-6.3 Di II VC PZD".to_string(),
            220 => "Yongnuo YN 50mm f/1.8".to_string(),
            224 => "Canon EF 70-200mm f/2.8L IS USM".to_string(),
            225 => "Canon EF 70-200mm f/2.8L IS USM + 1.4x".to_string(),
            226 => "Canon EF 70-200mm f/2.8L IS USM + 2x".to_string(),
            227 => "Canon EF 70-200mm f/2.8L IS USM + 2.8x".to_string(),
            228 => "Canon EF 28-105mm f/3.5-4.5 USM".to_string(),
            229 => "Canon EF 16-35mm f/2.8L USM".to_string(),
            230 => "Canon EF 24-70mm f/2.8L USM".to_string(),
            231 => "Canon EF 17-40mm f/4L USM or Sigma Lens".to_string(),
            232 => "Canon EF 70-300mm f/4.5-5.6 DO IS USM".to_string(),
            233 => "Canon EF 28-300mm f/3.5-5.6L IS USM".to_string(),
            234 => "Canon EF-S 17-85mm f/4-5.6 IS USM or Tokina Lens".to_string(),
            235 => "Canon EF-S 10-22mm f/3.5-4.5 USM".to_string(),
            236 => "Canon EF-S 60mm f/2.8 Macro USM".to_string(),
            237 => "Canon EF 24-105mm f/4L IS USM".to_string(),
            238 => "Canon EF 70-300mm f/4-5.6 IS USM".to_string(),
            239 => "Canon EF 85mm f/1.2L II USM or Rokinon Lens".to_string(),
            240 => "Canon EF-S 17-55mm f/2.8 IS USM or Sigma Lens".to_string(),
            241 => "Canon EF 50mm f/1.2L USM".to_string(),
            242 => "Canon EF 70-200mm f/4L IS USM".to_string(),
            243 => "Canon EF 70-200mm f/4L IS USM + 1.4x".to_string(),
            244 => "Canon EF 70-200mm f/4L IS USM + 2x".to_string(),
            245 => "Canon EF 70-200mm f/4L IS USM + 2.8x".to_string(),
            246 => "Canon EF 16-35mm f/2.8L II USM".to_string(),
            247 => "Canon EF 14mm f/2.8L II USM".to_string(),
            248 => "Canon EF 200mm f/2L IS USM or Sigma Lens".to_string(),
            249 => "Canon EF 800mm f/5.6L IS USM".to_string(),
            250 => "Canon EF 24mm f/1.4L II USM or Sigma Lens".to_string(),
            251 => "Canon EF 70-200mm f/2.8L IS II USM".to_string(),
            252 => "Canon EF 70-200mm f/2.8L IS II USM + 1.4x".to_string(),
            253 => "Canon EF 70-200mm f/2.8L IS II USM + 2x".to_string(),
            254 => "Canon EF 100mm f/2.8L Macro IS USM or Tamron Lens".to_string(),
            255 => "Sigma 24-105mm f/4 DG OS HSM | A or Other Lens".to_string(),
            368 => "Sigma 14-24mm f/2.8 DG HSM | A or other Sigma Lens".to_string(),
            488 => "Canon EF-S 15-85mm f/3.5-5.6 IS USM".to_string(),
            489 => "Canon EF 70-300mm f/4-5.6L IS USM".to_string(),
            490 => "Canon EF 8-15mm f/4L Fisheye USM".to_string(),
            491 => "Canon EF 300mm f/2.8L IS II USM or Tamron Lens".to_string(),
            492 => "Canon EF 400mm f/2.8L IS II USM".to_string(),
            493 => "Canon EF 500mm f/4L IS II USM or EF 24-105mm f4L IS USM".to_string(),
            494 => "Canon EF 600mm f/4L IS II USM".to_string(),
            495 => "Canon EF 24-70mm f/2.8L II USM or Sigma Lens".to_string(),
            496 => "Canon EF 200-400mm f/4L IS USM".to_string(),
            499 => "Canon EF 200-400mm f/4L IS USM + 1.4x".to_string(),
            502 => "Canon EF 28mm f/2.8 IS USM or Tamron Lens".to_string(),
            503 => "Canon EF 24mm f/2.8 IS USM".to_string(),
            504 => "Canon EF 24-70mm f/4L IS USM".to_string(),
            505 => "Canon EF 35mm f/2 IS USM".to_string(),
            506 => "Canon EF 400mm f/4 DO IS II USM".to_string(),
            507 => "Canon EF 16-35mm f/4L IS USM".to_string(),
            508 => "Canon EF 11-24mm f/4L USM or Tamron Lens".to_string(),
            624 => "Sigma 70-200mm f/2.8 DG OS HSM | S or other Sigma Lens".to_string(),
            747 => "Canon EF 100-400mm f/4.5-5.6L IS II USM or Tamron Lens".to_string(),
            748 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 1.4x or Tamron Lens".to_string(),
            749 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 2x or Tamron Lens".to_string(),
            750 => "Canon EF 35mm f/1.4L II USM or Tamron Lens".to_string(),
            751 => "Canon EF 16-35mm f/2.8L III USM".to_string(),
            752 => "Canon EF 24-105mm f/4L IS II USM".to_string(),
            753 => "Canon EF 85mm f/1.4L IS USM".to_string(),
            754 => "Canon EF 70-200mm f/4L IS II USM".to_string(),
            757 => "Canon EF 400mm f/2.8L IS III USM".to_string(),
            758 => "Canon EF 600mm f/4L IS III USM".to_string(),
            923 => "Meike/SKY 85mm f/1.8 DCM".to_string(),
            1136 => "Sigma 24-70mm f/2.8 DG OS HSM | A".to_string(),
            4142 => "Canon EF-S 18-135mm f/3.5-5.6 IS STM".to_string(),
            4143 => "Canon EF-M 18-55mm f/3.5-5.6 IS STM or Tamron Lens".to_string(),
            4144 => "Canon EF 40mm f/2.8 STM".to_string(),
            4145 => "Canon EF-M 22mm f/2 STM".to_string(),
            4146 => "Canon EF-S 18-55mm f/3.5-5.6 IS STM".to_string(),
            4147 => "Canon EF-M 11-22mm f/4-5.6 IS STM".to_string(),
            4148 => "Canon EF-S 55-250mm f/4-5.6 IS STM".to_string(),
            4149 => "Canon EF-M 55-200mm f/4.5-6.3 IS STM".to_string(),
            4150 => "Canon EF-S 10-18mm f/4.5-5.6 IS STM".to_string(),
            4152 => "Canon EF 24-105mm f/3.5-5.6 IS STM".to_string(),
            4153 => "Canon EF-M 15-45mm f/3.5-6.3 IS STM".to_string(),
            4154 => "Canon EF-S 24mm f/2.8 STM".to_string(),
            4155 => "Canon EF-M 28mm f/3.5 Macro IS STM".to_string(),
            4156 => "Canon EF 50mm f/1.8 STM".to_string(),
            4157 => "Canon EF-M 18-150mm f/3.5-6.3 IS STM".to_string(),
            4158 => "Canon EF-S 18-55mm f/4-5.6 IS STM".to_string(),
            4159 => "Canon EF-M 32mm f/1.4 STM".to_string(),
            4160 => "Canon EF-S 35mm f/2.8 Macro IS STM".to_string(),
            4208 => "Sigma 56mm f/1.4 DC DN | C or other Sigma Lens".to_string(),
            4976 => "Sigma 16-300mm F3.5-6.7 DC OS | C (025)".to_string(),
            6512 => "Sigma 12mm F1.4 DC | C".to_string(),
            36910 => "Canon EF 70-300mm f/4-5.6 IS II USM".to_string(),
            36912 => "Canon EF-S 18-135mm f/3.5-5.6 IS USM".to_string(),
            61182 => "Canon RF 50mm F1.2L USM or other Canon RF Lens".to_string(),
            61491 => "Canon CN-E 14mm T3.1 L F".to_string(),
            61492 => "Canon CN-E 24mm T1.5 L F".to_string(),
            61494 => "Canon CN-E 85mm T1.3 L F".to_string(),
            61495 => "Canon CN-E 135mm T2.2 L F".to_string(),
            61496 => "Canon CN-E 35mm T1.5 L F".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("LensType", 0x184, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x186, bo) {
        dm.push(("MinFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MinFocalLength", 0x186, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x188, bo) {
        dm.push(("MaxFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MaxFocalLength", 0x188, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(text) = text_at(data, 0x43d, 6, true) {
        tags.push(mk("FirmwareVersion", 0x43d, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    if let Some(text) = text_at(data, 0x449, 6, true) {
        tags.push(mk("FirmwareVersion", 0x449, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    tags
}

/// `Image::ExifTool::Canon::CameraInfo1000D` -- FORMAT int8u, FIRST_ENTRY 0.
fn canon_camerainfo1000d(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = u8_at(data, 0x3) {
        dm.push(("FNumber".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(($val-8)/16*log(2))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("sprintf(\"%.2g\",$val)", &cv, &ctx) { cv = x; }
            tags.push(mk("FNumber", 0x3, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x4) {
        dm.push(("ExposureTime".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp(4*log(2)*(1-Image::ExifTool::Canon::CanonEv($val-24)))", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("Image::ExifTool::Exif::PrintExposureTime($val)", &cv, &ctx) { cv = x; }
            tags.push(mk("ExposureTime", 0x4, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x6) {
        dm.push(("ISO".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("100*exp(($val/8-9)*log(2))", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("sprintf(\"%.0f\",$val)", &cv, &ctx) { cv = x; }
        tags.push(mk("ISO", 0x6, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x13) {
        let v = v & 0x7f;
        dm.push(("FlashModel".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "n/a".to_string(),
            4 => "Speedlite 540EZ".to_string(),
            5 => "Speedlite 380EX".to_string(),
            6 => "Speedlite 550EX".to_string(),
            8 => "Speedlite ST-E2".to_string(),
            9 => "Speedlite MR-14EX".to_string(),
            12 => "Speedlite 580EX".to_string(),
            13 => "Speedlite 430EX".to_string(),
            17 => "Speedlite 580EX II".to_string(),
            18 => "Speedlite 430EX II".to_string(),
            22 => "Speedlite 600EX-RT".to_string(),
            23 => "Speedlite 600EX II-RT".to_string(),
            24 => "Speedlite 90EX".to_string(),
            25 => "Speedlite 430EX III-RT".to_string(),
            31 => "Speedlite EL-1 ver2".to_string(),
            33 => "Speedlite EL-5".to_string(),
            34 => "Speedlite EL-10".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("FlashModel", 0x13, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x15) {
        dm.push(("FlashMeteringMode".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "E-TTL".to_string(),
            3 => "TTL".to_string(),
            4 => "External Auto".to_string(),
            5 => "External Manual".to_string(),
            6 => "Off".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("FlashMeteringMode", 0x15, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u8_at(data, 0x18) {
        dm.push(("CameraTemperature".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 128", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
        tags.push(mk("CameraTemperature", 0x18, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if (dm_get(dm, "LensType").is_some_and(|v| v != 0.0) && dm_get(dm, "LensType").is_some_and(|v| v == 124.0)) {
        if let Some(v) = u8_at(data, 0x1b) {
            dm.push(("MacroMagnification".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("exp((75-$val) * log(2) * 3 / 40)", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("sprintf(\"%.1fx\",$val)", &cv, &ctx) { cv = x; }
            tags.push(mk("MacroMagnification", 0x1b, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u16rev_at(data, 0x1d, bo) {
        dm.push(("FocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let rc = conv_expr::eval_with("$val ? $val : undef", &Conv::Num(f64::from(v)), &ctx);
        if rc.as_ref() != Some(&Conv::Undef) {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let v = rc.map_or(v, |x| x.as_num() as _);
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
            tags.push(mk("FocalLength", 0x1d, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = u8_at(data, 0x30) {
        dm.push(("CameraOrientation".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Horizontal (normal)".to_string(),
            1 => "Rotate 90 CW".to_string(),
            2 => "Rotate 270 CW".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("CameraOrientation", 0x30, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x43, bo) {
        dm.push(("FocusDistanceUpper".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceUpper", 0x43, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0x45, bo) {
        dm.push(("FocusDistanceLower".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val / 100", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("$val > 655.345 ? \"inf\" : \"$val m\"", &cv, &ctx) { cv = x; }
        tags.push(mk("FocusDistanceLower", 0x45, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x6f, bo) {
        dm.push(("WhiteBalance".to_string(), f64::from(v)));
        let s = match v as i64 {
            0 => "Auto".to_string(),
            1 => "Daylight".to_string(),
            2 => "Cloudy".to_string(),
            3 => "Tungsten".to_string(),
            4 => "Fluorescent".to_string(),
            5 => "Flash".to_string(),
            6 => "Custom".to_string(),
            7 => "Black & White".to_string(),
            8 => "Shade".to_string(),
            9 => "Manual Temperature (Kelvin)".to_string(),
            10 => "PC Set1".to_string(),
            11 => "PC Set2".to_string(),
            12 => "PC Set3".to_string(),
            14 => "Daylight Fluorescent".to_string(),
            15 => "Custom 1".to_string(),
            16 => "Custom 2".to_string(),
            17 => "Underwater".to_string(),
            18 => "Custom 3".to_string(),
            19 => "Custom 4".to_string(),
            20 => "PC Set4".to_string(),
            21 => "PC Set5".to_string(),
            23 => "Auto (ambience priority)".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("WhiteBalance", 0x6f, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x73, bo) {
        dm.push(("ColorTemperature".to_string(), f64::from(v)));
        tags.push(mk("ColorTemperature", 0x73, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0xe2, bo) {
        dm.push(("LensType".to_string(), f64::from(v)));
        let s = match v as i64 {
            -1 => "n/a".to_string(),
            1 => "Canon EF 50mm f/1.8".to_string(),
            2 => "Canon EF 28mm f/2.8 or Sigma Lens".to_string(),
            3 => "Canon EF 135mm f/2.8 Soft".to_string(),
            4 => "Canon EF 35-105mm f/3.5-4.5 or Sigma Lens".to_string(),
            5 => "Canon EF 35-70mm f/3.5-4.5".to_string(),
            6 => "Canon EF 28-70mm f/3.5-4.5 or Sigma or Tokina Lens".to_string(),
            7 => "Canon EF 100-300mm f/5.6L".to_string(),
            8 => "Canon EF 100-300mm f/5.6 or Sigma or Tokina Lens".to_string(),
            9 => "Canon EF 70-210mm f/4".to_string(),
            10 => "Canon EF 50mm f/2.5 Macro or Sigma Lens".to_string(),
            11 => "Canon EF 35mm f/2".to_string(),
            13 => "Canon EF 15mm f/2.8 Fisheye".to_string(),
            14 => "Canon EF 50-200mm f/3.5-4.5L".to_string(),
            15 => "Canon EF 50-200mm f/3.5-4.5".to_string(),
            16 => "Canon EF 35-135mm f/3.5-4.5".to_string(),
            17 => "Canon EF 35-70mm f/3.5-4.5A".to_string(),
            18 => "Canon EF 28-70mm f/3.5-4.5".to_string(),
            20 => "Canon EF 100-200mm f/4.5A".to_string(),
            21 => "Canon EF 80-200mm f/2.8L".to_string(),
            22 => "Canon EF 20-35mm f/2.8L or Tokina Lens".to_string(),
            23 => "Canon EF 35-105mm f/3.5-4.5".to_string(),
            24 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            25 => "Canon EF 35-80mm f/4-5.6 Power Zoom".to_string(),
            26 => "Canon EF 100mm f/2.8 Macro or Other Lens".to_string(),
            27 => "Canon EF 35-80mm f/4-5.6".to_string(),
            28 => "Canon EF 80-200mm f/4.5-5.6 or Tamron Lens".to_string(),
            29 => "Canon EF 50mm f/1.8 II".to_string(),
            30 => "Canon EF 35-105mm f/4.5-5.6".to_string(),
            31 => "Canon EF 75-300mm f/4-5.6 or Tamron Lens".to_string(),
            32 => "Canon EF 24mm f/2.8 or Sigma Lens".to_string(),
            33 => "Voigtlander or Carl Zeiss Lens".to_string(),
            35 => "Canon EF 35-80mm f/4-5.6".to_string(),
            36 => "Canon EF 38-76mm f/4.5-5.6".to_string(),
            37 => "Canon EF 35-80mm f/4-5.6 or Tamron Lens".to_string(),
            38 => "Canon EF 80-200mm f/4.5-5.6 II".to_string(),
            39 => "Canon EF 75-300mm f/4-5.6".to_string(),
            40 => "Canon EF 28-80mm f/3.5-5.6".to_string(),
            41 => "Canon EF 28-90mm f/4-5.6".to_string(),
            42 => "Canon EF 28-200mm f/3.5-5.6 or Tamron Lens".to_string(),
            43 => "Canon EF 28-105mm f/4-5.6".to_string(),
            44 => "Canon EF 90-300mm f/4.5-5.6".to_string(),
            45 => "Canon EF-S 18-55mm f/3.5-5.6 [II]".to_string(),
            46 => "Canon EF 28-90mm f/4-5.6".to_string(),
            47 => "Zeiss Milvus 35mm f/2 or 50mm f/2".to_string(),
            48 => "Canon EF-S 18-55mm f/3.5-5.6 IS".to_string(),
            49 => "Canon EF-S 55-250mm f/4-5.6 IS".to_string(),
            50 => "Canon EF-S 18-200mm f/3.5-5.6 IS".to_string(),
            51 => "Canon EF-S 18-135mm f/3.5-5.6 IS".to_string(),
            52 => "Canon EF-S 18-55mm f/3.5-5.6 IS II".to_string(),
            53 => "Canon EF-S 18-55mm f/3.5-5.6 III".to_string(),
            54 => "Canon EF-S 55-250mm f/4-5.6 IS II".to_string(),
            60 => "Irix 11mm f/4 or 15mm f/2.4".to_string(),
            63 => "Irix 30mm F1.4 Dragonfly".to_string(),
            80 => "Canon TS-E 50mm f/2.8L Macro".to_string(),
            81 => "Canon TS-E 90mm f/2.8L Macro".to_string(),
            82 => "Canon TS-E 135mm f/4L Macro".to_string(),
            94 => "Canon TS-E 17mm f/4L".to_string(),
            95 => "Canon TS-E 24mm f/3.5L II".to_string(),
            103 => "Samyang AF 14mm f/2.8 EF or Rokinon Lens".to_string(),
            106 => "Rokinon SP / Samyang XP 35mm f/1.2".to_string(),
            112 => "Sigma 28mm f/1.5 FF High-speed Prime or other Sigma Lens".to_string(),
            117 => "Tamron 35-150mm f/2.8-4.0 Di VC OSD (A043) or other Tamron Lens".to_string(),
            124 => "Canon MP-E 65mm f/2.8 1-5x Macro Photo".to_string(),
            125 => "Canon TS-E 24mm f/3.5L".to_string(),
            126 => "Canon TS-E 45mm f/2.8".to_string(),
            127 => "Canon TS-E 90mm f/2.8 or Tamron Lens".to_string(),
            129 => "Canon EF 300mm f/2.8L USM".to_string(),
            130 => "Canon EF 50mm f/1.0L USM".to_string(),
            131 => "Canon EF 28-80mm f/2.8-4L USM or Sigma Lens".to_string(),
            132 => "Canon EF 1200mm f/5.6L USM".to_string(),
            134 => "Canon EF 600mm f/4L IS USM".to_string(),
            135 => "Canon EF 200mm f/1.8L USM".to_string(),
            136 => "Canon EF 300mm f/2.8L USM".to_string(),
            137 => "Canon EF 85mm f/1.2L USM or Sigma or Tamron Lens".to_string(),
            138 => "Canon EF 28-80mm f/2.8-4L".to_string(),
            139 => "Canon EF 400mm f/2.8L USM".to_string(),
            140 => "Canon EF 500mm f/4.5L USM".to_string(),
            141 => "Canon EF 500mm f/4.5L USM".to_string(),
            142 => "Canon EF 300mm f/2.8L IS USM".to_string(),
            143 => "Canon EF 500mm f/4L IS USM or Sigma Lens".to_string(),
            144 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            145 => "Canon EF 100-300mm f/4.5-5.6 USM".to_string(),
            146 => "Canon EF 70-210mm f/3.5-4.5 USM".to_string(),
            147 => "Canon EF 35-135mm f/4-5.6 USM".to_string(),
            148 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            149 => "Canon EF 100mm f/2 USM".to_string(),
            150 => "Canon EF 14mm f/2.8L USM or Sigma Lens".to_string(),
            151 => "Canon EF 200mm f/2.8L USM".to_string(),
            152 => "Canon EF 300mm f/4L IS USM or Sigma Lens".to_string(),
            153 => "Canon EF 35-350mm f/3.5-5.6L USM or Sigma or Tamron Lens".to_string(),
            154 => "Canon EF 20mm f/2.8 USM or Zeiss Lens".to_string(),
            155 => "Canon EF 85mm f/1.8 USM or Sigma Lens".to_string(),
            156 => "Canon EF 28-105mm f/3.5-4.5 USM or Tamron Lens".to_string(),
            160 => "Canon EF 20-35mm f/3.5-4.5 USM or Tamron or Tokina Lens".to_string(),
            161 => "Canon EF 28-70mm f/2.8L USM or Other Lens".to_string(),
            162 => "Canon EF 200mm f/2.8L USM".to_string(),
            163 => "Canon EF 300mm f/4L".to_string(),
            164 => "Canon EF 400mm f/5.6L".to_string(),
            165 => "Canon EF 70-200mm f/2.8L USM".to_string(),
            166 => "Canon EF 70-200mm f/2.8L USM + 1.4x".to_string(),
            167 => "Canon EF 70-200mm f/2.8L USM + 2x".to_string(),
            168 => "Canon EF 28mm f/1.8 USM or Sigma Lens".to_string(),
            169 => "Canon EF 17-35mm f/2.8L USM or Sigma Lens".to_string(),
            170 => "Canon EF 200mm f/2.8L II USM or Sigma Lens".to_string(),
            171 => "Canon EF 300mm f/4L USM".to_string(),
            172 => "Canon EF 400mm f/5.6L USM or Sigma Lens".to_string(),
            173 => "Canon EF 180mm Macro f/3.5L USM or Sigma Lens".to_string(),
            174 => "Canon EF 135mm f/2L USM or Other Lens".to_string(),
            175 => "Canon EF 400mm f/2.8L USM".to_string(),
            176 => "Canon EF 24-85mm f/3.5-4.5 USM".to_string(),
            177 => "Canon EF 300mm f/4L IS USM".to_string(),
            178 => "Canon EF 28-135mm f/3.5-5.6 IS".to_string(),
            179 => "Canon EF 24mm f/1.4L USM".to_string(),
            180 => "Canon EF 35mm f/1.4L USM or Other Lens".to_string(),
            181 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 1.4x or Sigma Lens".to_string(),
            182 => "Canon EF 100-400mm f/4.5-5.6L IS USM + 2x or Sigma Lens".to_string(),
            183 => "Canon EF 100-400mm f/4.5-5.6L IS USM or Sigma Lens".to_string(),
            184 => "Canon EF 400mm f/2.8L USM + 2x".to_string(),
            185 => "Canon EF 600mm f/4L IS USM".to_string(),
            186 => "Canon EF 70-200mm f/4L USM".to_string(),
            187 => "Canon EF 70-200mm f/4L USM + 1.4x".to_string(),
            188 => "Canon EF 70-200mm f/4L USM + 2x".to_string(),
            189 => "Canon EF 70-200mm f/4L USM + 2.8x".to_string(),
            190 => "Canon EF 100mm f/2.8 Macro USM".to_string(),
            191 => "Canon EF 400mm f/4 DO IS or Sigma Lens".to_string(),
            193 => "Canon EF 35-80mm f/4-5.6 USM".to_string(),
            194 => "Canon EF 80-200mm f/4.5-5.6 USM".to_string(),
            195 => "Canon EF 35-105mm f/4.5-5.6 USM".to_string(),
            196 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            197 => "Canon EF 75-300mm f/4-5.6 IS USM or Sigma Lens".to_string(),
            198 => "Canon EF 50mm f/1.4 USM or Other Lens".to_string(),
            199 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            200 => "Canon EF 75-300mm f/4-5.6 USM".to_string(),
            201 => "Canon EF 28-80mm f/3.5-5.6 USM".to_string(),
            202 => "Canon EF 28-80mm f/3.5-5.6 USM IV".to_string(),
            208 => "Canon EF 22-55mm f/4-5.6 USM".to_string(),
            209 => "Canon EF 55-200mm f/4.5-5.6".to_string(),
            210 => "Canon EF 28-90mm f/4-5.6 USM".to_string(),
            211 => "Canon EF 28-200mm f/3.5-5.6 USM".to_string(),
            212 => "Canon EF 28-105mm f/4-5.6 USM".to_string(),
            213 => "Canon EF 90-300mm f/4.5-5.6 USM or Tamron Lens".to_string(),
            214 => "Canon EF-S 18-55mm f/3.5-5.6 USM".to_string(),
            215 => "Canon EF 55-200mm f/4.5-5.6 II USM".to_string(),
            217 => "Tamron AF 18-270mm f/3.5-6.3 Di II VC PZD".to_string(),
            220 => "Yongnuo YN 50mm f/1.8".to_string(),
            224 => "Canon EF 70-200mm f/2.8L IS USM".to_string(),
            225 => "Canon EF 70-200mm f/2.8L IS USM + 1.4x".to_string(),
            226 => "Canon EF 70-200mm f/2.8L IS USM + 2x".to_string(),
            227 => "Canon EF 70-200mm f/2.8L IS USM + 2.8x".to_string(),
            228 => "Canon EF 28-105mm f/3.5-4.5 USM".to_string(),
            229 => "Canon EF 16-35mm f/2.8L USM".to_string(),
            230 => "Canon EF 24-70mm f/2.8L USM".to_string(),
            231 => "Canon EF 17-40mm f/4L USM or Sigma Lens".to_string(),
            232 => "Canon EF 70-300mm f/4.5-5.6 DO IS USM".to_string(),
            233 => "Canon EF 28-300mm f/3.5-5.6L IS USM".to_string(),
            234 => "Canon EF-S 17-85mm f/4-5.6 IS USM or Tokina Lens".to_string(),
            235 => "Canon EF-S 10-22mm f/3.5-4.5 USM".to_string(),
            236 => "Canon EF-S 60mm f/2.8 Macro USM".to_string(),
            237 => "Canon EF 24-105mm f/4L IS USM".to_string(),
            238 => "Canon EF 70-300mm f/4-5.6 IS USM".to_string(),
            239 => "Canon EF 85mm f/1.2L II USM or Rokinon Lens".to_string(),
            240 => "Canon EF-S 17-55mm f/2.8 IS USM or Sigma Lens".to_string(),
            241 => "Canon EF 50mm f/1.2L USM".to_string(),
            242 => "Canon EF 70-200mm f/4L IS USM".to_string(),
            243 => "Canon EF 70-200mm f/4L IS USM + 1.4x".to_string(),
            244 => "Canon EF 70-200mm f/4L IS USM + 2x".to_string(),
            245 => "Canon EF 70-200mm f/4L IS USM + 2.8x".to_string(),
            246 => "Canon EF 16-35mm f/2.8L II USM".to_string(),
            247 => "Canon EF 14mm f/2.8L II USM".to_string(),
            248 => "Canon EF 200mm f/2L IS USM or Sigma Lens".to_string(),
            249 => "Canon EF 800mm f/5.6L IS USM".to_string(),
            250 => "Canon EF 24mm f/1.4L II USM or Sigma Lens".to_string(),
            251 => "Canon EF 70-200mm f/2.8L IS II USM".to_string(),
            252 => "Canon EF 70-200mm f/2.8L IS II USM + 1.4x".to_string(),
            253 => "Canon EF 70-200mm f/2.8L IS II USM + 2x".to_string(),
            254 => "Canon EF 100mm f/2.8L Macro IS USM or Tamron Lens".to_string(),
            255 => "Sigma 24-105mm f/4 DG OS HSM | A or Other Lens".to_string(),
            368 => "Sigma 14-24mm f/2.8 DG HSM | A or other Sigma Lens".to_string(),
            488 => "Canon EF-S 15-85mm f/3.5-5.6 IS USM".to_string(),
            489 => "Canon EF 70-300mm f/4-5.6L IS USM".to_string(),
            490 => "Canon EF 8-15mm f/4L Fisheye USM".to_string(),
            491 => "Canon EF 300mm f/2.8L IS II USM or Tamron Lens".to_string(),
            492 => "Canon EF 400mm f/2.8L IS II USM".to_string(),
            493 => "Canon EF 500mm f/4L IS II USM or EF 24-105mm f4L IS USM".to_string(),
            494 => "Canon EF 600mm f/4L IS II USM".to_string(),
            495 => "Canon EF 24-70mm f/2.8L II USM or Sigma Lens".to_string(),
            496 => "Canon EF 200-400mm f/4L IS USM".to_string(),
            499 => "Canon EF 200-400mm f/4L IS USM + 1.4x".to_string(),
            502 => "Canon EF 28mm f/2.8 IS USM or Tamron Lens".to_string(),
            503 => "Canon EF 24mm f/2.8 IS USM".to_string(),
            504 => "Canon EF 24-70mm f/4L IS USM".to_string(),
            505 => "Canon EF 35mm f/2 IS USM".to_string(),
            506 => "Canon EF 400mm f/4 DO IS II USM".to_string(),
            507 => "Canon EF 16-35mm f/4L IS USM".to_string(),
            508 => "Canon EF 11-24mm f/4L USM or Tamron Lens".to_string(),
            624 => "Sigma 70-200mm f/2.8 DG OS HSM | S or other Sigma Lens".to_string(),
            747 => "Canon EF 100-400mm f/4.5-5.6L IS II USM or Tamron Lens".to_string(),
            748 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 1.4x or Tamron Lens".to_string(),
            749 => "Canon EF 100-400mm f/4.5-5.6L IS II USM + 2x or Tamron Lens".to_string(),
            750 => "Canon EF 35mm f/1.4L II USM or Tamron Lens".to_string(),
            751 => "Canon EF 16-35mm f/2.8L III USM".to_string(),
            752 => "Canon EF 24-105mm f/4L IS II USM".to_string(),
            753 => "Canon EF 85mm f/1.4L IS USM".to_string(),
            754 => "Canon EF 70-200mm f/4L IS II USM".to_string(),
            757 => "Canon EF 400mm f/2.8L IS III USM".to_string(),
            758 => "Canon EF 600mm f/4L IS III USM".to_string(),
            923 => "Meike/SKY 85mm f/1.8 DCM".to_string(),
            1136 => "Sigma 24-70mm f/2.8 DG OS HSM | A".to_string(),
            4142 => "Canon EF-S 18-135mm f/3.5-5.6 IS STM".to_string(),
            4143 => "Canon EF-M 18-55mm f/3.5-5.6 IS STM or Tamron Lens".to_string(),
            4144 => "Canon EF 40mm f/2.8 STM".to_string(),
            4145 => "Canon EF-M 22mm f/2 STM".to_string(),
            4146 => "Canon EF-S 18-55mm f/3.5-5.6 IS STM".to_string(),
            4147 => "Canon EF-M 11-22mm f/4-5.6 IS STM".to_string(),
            4148 => "Canon EF-S 55-250mm f/4-5.6 IS STM".to_string(),
            4149 => "Canon EF-M 55-200mm f/4.5-6.3 IS STM".to_string(),
            4150 => "Canon EF-S 10-18mm f/4.5-5.6 IS STM".to_string(),
            4152 => "Canon EF 24-105mm f/3.5-5.6 IS STM".to_string(),
            4153 => "Canon EF-M 15-45mm f/3.5-6.3 IS STM".to_string(),
            4154 => "Canon EF-S 24mm f/2.8 STM".to_string(),
            4155 => "Canon EF-M 28mm f/3.5 Macro IS STM".to_string(),
            4156 => "Canon EF 50mm f/1.8 STM".to_string(),
            4157 => "Canon EF-M 18-150mm f/3.5-6.3 IS STM".to_string(),
            4158 => "Canon EF-S 18-55mm f/4-5.6 IS STM".to_string(),
            4159 => "Canon EF-M 32mm f/1.4 STM".to_string(),
            4160 => "Canon EF-S 35mm f/2.8 Macro IS STM".to_string(),
            4208 => "Sigma 56mm f/1.4 DC DN | C or other Sigma Lens".to_string(),
            4976 => "Sigma 16-300mm F3.5-6.7 DC OS | C (025)".to_string(),
            6512 => "Sigma 12mm F1.4 DC | C".to_string(),
            36910 => "Canon EF 70-300mm f/4-5.6 IS II USM".to_string(),
            36912 => "Canon EF-S 18-135mm f/3.5-5.6 IS USM".to_string(),
            61182 => "Canon RF 50mm F1.2L USM or other Canon RF Lens".to_string(),
            61491 => "Canon CN-E 14mm T3.1 L F".to_string(),
            61492 => "Canon CN-E 24mm T1.5 L F".to_string(),
            61494 => "Canon CN-E 85mm T1.3 L F".to_string(),
            61495 => "Canon CN-E 135mm T2.2 L F".to_string(),
            61496 => "Canon CN-E 35mm T1.5 L F".to_string(),
            65535 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("LensType", 0xe2, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0xe4, bo) {
        dm.push(("MinFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MinFocalLength", 0xe4, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16rev_at(data, 0xe6, bo) {
        dm.push(("MaxFocalLength".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val mm\"", &cv, &ctx) { cv = x; }
        tags.push(mk("MaxFocalLength", 0xe6, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(text) = text_at(data, 0x10b, 6, true) {
        tags.push(mk("FirmwareVersion", 0x10b, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x137, bo) {
        dm.push(("DirectoryIndex".to_string(), f64::from(v)));
        tags.push(mk("DirectoryIndex", 0x137, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0x143, bo) {
        dm.push(("FileIndex".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val + 1", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        tags.push(mk("FileIndex", 0x143, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(text) = text_at(data, 0x937, 64, true) {
        tags.push(mk("LensModel", 0x937, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
    }
    if let Some(sub) = data.get(0x267..) {
        tags.extend(canon_psinfo(sub, model, bo, file_type, format, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::CameraInfoR6` -- FORMAT int8u, FIRST_ENTRY 0.
fn canon_camerainfor6(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = u8_at(data, 0x9da) {
        dm.push(("CameraTemperature".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("$val - 128", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
        tags.push(mk("CameraTemperature", 0x9da, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u32_at(data, 0xaf1, bo) {
        dm.push(("ShutterCount".to_string(), f64::from(v)));
        tags.push(mk("ShutterCount", 0xaf1, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    tags
}

/// `Image::ExifTool::Canon::CameraInfoR6m2` -- FORMAT int8u, FIRST_ENTRY 0.
fn canon_camerainfor6m2(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = u32_at(data, 0xd29, bo) {
        dm.push(("ShutterCount".to_string(), f64::from(v)));
        tags.push(mk("ShutterCount", 0xd29, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    tags
}

/// `Image::ExifTool::Canon::CameraInfoR6m3` -- FORMAT int8u, FIRST_ENTRY 0.
fn canon_camerainfor6m3(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = u16_at(data, 0x86d, bo) {
        dm.push(("ImageCount".to_string(), f64::from(v)));
        tags.push(mk("ImageCount", 0x86d, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    tags
}

/// `Image::ExifTool::Canon::CameraInfoG5XII` -- FORMAT int8u, FIRST_ENTRY 0.
fn canon_camerainfog5xii(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if file_type == "JPEG" {
        if let Some(v) = u32_at(data, 0x293, bo) {
            dm.push(("ShutterCount".to_string(), f64::from(v)));
            tags.push(mk("ShutterCount", 0x293, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if file_type == "CR3" {
        if let Some(v) = u32_at(data, 0xa95, bo) {
            dm.push(("ShutterCount".to_string(), f64::from(v)));
            tags.push(mk("ShutterCount", 0xa95, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if file_type == "JPEG" {
        if let Some(v) = u32_at(data, 0xb21, bo) {
            dm.push(("DirectoryIndex".to_string(), f64::from(v)));
            tags.push(mk("DirectoryIndex", 0xb21, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
        }
    }
    if file_type == "JPEG" {
        if let Some(v) = u32_at(data, 0xb2d, bo) {
            dm.push(("FileIndex".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let mut cv = Conv::Num(f64::from(v));
            if let Some(x) = conv_expr::eval_with("$val + 1", &cv, &ctx) { cv = x; }
            let raw = Value::F64(cv.as_num());
            tags.push(mk("FileIndex", 0xb2d, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    tags
}

/// `Image::ExifTool::Canon::CameraInfoPowerShot` -- FORMAT int32s, FIRST_ENTRY 0.
fn canon_camerainfopowershot(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = i32_at(data, 0x0, bo) {
        dm.push(("ISO".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("100*exp((($val-411)/96)*log(2))", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("sprintf(\"%.0f\",$val)", &cv, &ctx) { cv = x; }
        tags.push(mk("ISO", 0x0, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x14, bo) {
        dm.push(("FNumber".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("exp($val/192*log(2))", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("sprintf(\"%.2g\",$val)", &cv, &ctx) { cv = x; }
        tags.push(mk("FNumber", 0x5, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x18, bo) {
        dm.push(("ExposureTime".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("exp(-$val/96*log(2))", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("Image::ExifTool::Exif::PrintExposureTime($val)", &cv, &ctx) { cv = x; }
        tags.push(mk("ExposureTime", 0x6, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x5c, bo) {
        dm.push(("Rotation".to_string(), f64::from(v)));
        tags.push(mk("Rotation", 0x17, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if dm_get(dm, "CameraInfoCount").is_some_and(|v| v == 138.0) {
        if let Some(v) = i32_at(data, 0x21c, bo) {
            dm.push(("CameraTemperature".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
            tags.push(mk("CameraTemperature", 0x87, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if dm_get(dm, "CameraInfoCount").is_some_and(|v| v == 148.0) {
        if let Some(v) = i32_at(data, 0x244, bo) {
            dm.push(("CameraTemperature".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
            tags.push(mk("CameraTemperature", 0x91, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    tags
}

/// `Image::ExifTool::Canon::CameraInfoPowerShot2` -- FORMAT int32s, FIRST_ENTRY 0.
fn canon_camerainfopowershot2(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = i32_at(data, 0x4, bo) {
        dm.push(("ISO".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("100*exp((($val-411)/96)*log(2))", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("sprintf(\"%.0f\",$val)", &cv, &ctx) { cv = x; }
        tags.push(mk("ISO", 0x1, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x18, bo) {
        dm.push(("FNumber".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("exp($val/192*log(2))", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("sprintf(\"%.2g\",$val)", &cv, &ctx) { cv = x; }
        tags.push(mk("FNumber", 0x6, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x1c, bo) {
        dm.push(("ExposureTime".to_string(), f64::from(v)));
        let ctx = Ctx { model, file_type, dm };
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval_with("exp(-$val/96*log(2))", &cv, &ctx) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval_with("Image::ExifTool::Exif::PrintExposureTime($val)", &cv, &ctx) { cv = x; }
        tags.push(mk("ExposureTime", 0x7, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x60, bo) {
        dm.push(("Rotation".to_string(), f64::from(v)));
        tags.push(mk("Rotation", 0x18, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if dm_get(dm, "CameraInfoCount").is_some_and(|v| v == 156.0) {
        if let Some(v) = i32_at(data, 0x264, bo) {
            dm.push(("CameraTemperature".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
            tags.push(mk("CameraTemperature", 0x99, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if dm_get(dm, "CameraInfoCount").is_some_and(|v| v == 162.0) {
        if let Some(v) = i32_at(data, 0x27c, bo) {
            dm.push(("CameraTemperature".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
            tags.push(mk("CameraTemperature", 0x9f, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if dm_get(dm, "CameraInfoCount").is_some_and(|v| v == 167.0) {
        if let Some(v) = i32_at(data, 0x290, bo) {
            dm.push(("CameraTemperature".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
            tags.push(mk("CameraTemperature", 0xa4, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if dm_get(dm, "CameraInfoCount").is_some_and(|v| v == 171.0) {
        if let Some(v) = i32_at(data, 0x2a0, bo) {
            dm.push(("CameraTemperature".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
            tags.push(mk("CameraTemperature", 0xa8, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if dm_get(dm, "CameraInfoCount").is_some_and(|v| v == 264.0) {
        if let Some(v) = i32_at(data, 0x414, bo) {
            dm.push(("CameraTemperature".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
            tags.push(mk("CameraTemperature", 0x105, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    tags
}

/// `Image::ExifTool::Canon::CameraInfoUnknown32` -- FORMAT int32s, FIRST_ENTRY 0.
fn canon_camerainfounknown32(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if dm_get(dm, "CameraInfoCount").is_some_and(|v| v == 72.0) {
        if let Some(v) = i32_at(data, 0x11c, bo) {
            dm.push(("CameraTemperature".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
            tags.push(mk("CameraTemperature", 0x47, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if dm_get(dm, "CameraInfoCount").is_some_and(|v| v == 85.0) {
        if let Some(v) = i32_at(data, 0x14c, bo) {
            dm.push(("CameraTemperature".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
            tags.push(mk("CameraTemperature", 0x53, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if (dm_get(dm, "CameraInfoCount").is_some_and(|v| v == 93.0) || dm_get(dm, "CameraInfoCount").is_some_and(|v| v == 94.0)) {
        if let Some(v) = i32_at(data, 0x16c, bo) {
            dm.push(("CameraTemperature".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
            tags.push(mk("CameraTemperature", 0x5b, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if dm_get(dm, "CameraInfoCount").is_some_and(|v| v == 96.0) {
        if let Some(v) = i32_at(data, 0x170, bo) {
            dm.push(("CameraTemperature".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
            tags.push(mk("CameraTemperature", 0x5c, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if dm_get(dm, "CameraInfoCount").is_some_and(|v| v == 104.0) {
        if let Some(v) = i32_at(data, 0x190, bo) {
            dm.push(("CameraTemperature".to_string(), f64::from(v)));
            let ctx = Ctx { model, file_type, dm };
            let mut cv = Conv::Num(f64::from(v));
            let raw = Value::F64(cv.as_num());
            if let Some(x) = conv_expr::eval_with("\"$val C\"", &cv, &ctx) { cv = x; }
            tags.push(mk("CameraTemperature", 0x64, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    tags
}

/// `Image::ExifTool::Canon::CameraInfoUnknown16` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_camerainfounknown16(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    tags
}

/// `Image::ExifTool::Canon::CameraInfoUnknown` -- FORMAT int8s, FIRST_ENTRY 0.
fn canon_camerainfounknown(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if MODEL_RE_10.is_match(model) {
        if let Some(text) = text_at(data, 0x16b, 5, false) {
            tags.push(mk("LensSerialNumber", 0x16b, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
        }
    }
    if prefix_matches(data.get(0x5c1..).unwrap_or(&[]), &[Some((0x30, 0x39)), Some((46, 46)), Some((0x30, 0x39)), Some((46, 46)), Some((0x30, 0x39)), Some((0, 0))]) {
        if let Some(text) = text_at(data, 0x5c1, 6, true) {
            tags.push(mk("FirmwareVersion", 0x5c1, text.clone(), Value::String(text), GRP1, GRP2, PRIO));
        }
    }
    tags
}

/// `Image::ExifTool::Canon::ColorCalib` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colorcalib(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x0 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x8 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x10 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x18 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x20 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x28 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x30 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x38 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x40 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x48 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x50 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x58 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x60 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x68 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x70 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    tags
}

/// `Image::ExifTool::Canon::ColorCoefs` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colorcoefs(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x0 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsAsShot", 0x0, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x8, bo) {
        dm.push(("ColorTempAsShot".to_string(), f64::from(v)));
        tags.push(mk("ColorTempAsShot", 0x4, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xa + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsAuto", 0x5, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x12, bo) {
        dm.push(("ColorTempAuto".to_string(), f64::from(v)));
        tags.push(mk("ColorTempAuto", 0x9, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x14 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsMeasured", 0xa, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x1c, bo) {
        dm.push(("ColorTempMeasured".to_string(), f64::from(v)));
        tags.push(mk("ColorTempMeasured", 0xe, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x1e + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x26, bo) {
        dm.push(("ColorTempUnknown".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x28 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsDaylight", 0x14, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x30, bo) {
        dm.push(("ColorTempDaylight".to_string(), f64::from(v)));
        tags.push(mk("ColorTempDaylight", 0x18, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x32 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsShade", 0x19, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x3a, bo) {
        dm.push(("ColorTempShade".to_string(), f64::from(v)));
        tags.push(mk("ColorTempShade", 0x1d, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x3c + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsCloudy", 0x1e, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x44, bo) {
        dm.push(("ColorTempCloudy".to_string(), f64::from(v)));
        tags.push(mk("ColorTempCloudy", 0x22, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x46 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsTungsten", 0x23, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x4e, bo) {
        dm.push(("ColorTempTungsten".to_string(), f64::from(v)));
        tags.push(mk("ColorTempTungsten", 0x27, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x50 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsFluorescent", 0x28, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x58, bo) {
        dm.push(("ColorTempFluorescent".to_string(), f64::from(v)));
        tags.push(mk("ColorTempFluorescent", 0x2c, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x5a + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsKelvin", 0x2d, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x62, bo) {
        dm.push(("ColorTempKelvin".to_string(), f64::from(v)));
        tags.push(mk("ColorTempKelvin", 0x31, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x64 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsFlash", 0x32, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x6c, bo) {
        dm.push(("ColorTempFlash".to_string(), f64::from(v)));
        tags.push(mk("ColorTempFlash", 0x36, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x6e + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x76, bo) {
        dm.push(("ColorTempUnknown2".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x78 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x80, bo) {
        dm.push(("ColorTempUnknown3".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x82 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x8a, bo) {
        dm.push(("ColorTempUnknown4".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x8c + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x94, bo) {
        dm.push(("ColorTempUnknown5".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x96 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x9e, bo) {
        dm.push(("ColorTempUnknown6".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xa0 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xa8, bo) {
        dm.push(("ColorTempUnknown7".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xaa + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xb2, bo) {
        dm.push(("ColorTempUnknown8".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xb4 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xbc, bo) {
        dm.push(("ColorTempUnknown9".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xbe + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xc6, bo) {
        dm.push(("ColorTempUnknown10".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xc8 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xd0, bo) {
        dm.push(("ColorTempUnknown11".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xd2 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xda, bo) {
        dm.push(("ColorTempUnknown12".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xdc + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xe4, bo) {
        dm.push(("ColorTempUnknown13".to_string(), f64::from(v)));
    }
    tags
}

/// `Image::ExifTool::Canon::ColorCoefs2` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colorcoefs2(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x0 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsAsShot", 0x0, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0xe, bo) {
        dm.push(("ColorTempAsShot".to_string(), f64::from(v)));
        tags.push(mk("ColorTempAsShot", 0x7, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x10 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsAuto", 0x8, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x1e, bo) {
        dm.push(("ColorTempAuto".to_string(), f64::from(v)));
        tags.push(mk("ColorTempAuto", 0xf, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x20 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsMeasured", 0x10, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x2e, bo) {
        dm.push(("ColorTempMeasured".to_string(), f64::from(v)));
        tags.push(mk("ColorTempMeasured", 0x17, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x30 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x3e, bo) {
        dm.push(("ColorTempUnknown".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x40 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsDaylight", 0x20, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x4e, bo) {
        dm.push(("ColorTempDaylight".to_string(), f64::from(v)));
        tags.push(mk("ColorTempDaylight", 0x27, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x50 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsShade", 0x28, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x5e, bo) {
        dm.push(("ColorTempShade".to_string(), f64::from(v)));
        tags.push(mk("ColorTempShade", 0x2f, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x60 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsCloudy", 0x30, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x6e, bo) {
        dm.push(("ColorTempCloudy".to_string(), f64::from(v)));
        tags.push(mk("ColorTempCloudy", 0x37, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x70 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsTungsten", 0x38, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x7e, bo) {
        dm.push(("ColorTempTungsten".to_string(), f64::from(v)));
        tags.push(mk("ColorTempTungsten", 0x3f, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x80 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsFluorescent", 0x40, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x8e, bo) {
        dm.push(("ColorTempFluorescent".to_string(), f64::from(v)));
        tags.push(mk("ColorTempFluorescent", 0x47, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x90 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsKelvin", 0x48, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0x9e, bo) {
        dm.push(("ColorTempKelvin".to_string(), f64::from(v)));
        tags.push(mk("ColorTempKelvin", 0x4f, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xa0 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            tags.push(mk("WB_RGGBLevelsFlash", 0x50, s.clone(), Value::String(s), GRP1, GRP2, PRIO));
        }
    }
    if let Some(v) = i16_at(data, 0xae, bo) {
        dm.push(("ColorTempFlash".to_string(), f64::from(v)));
        tags.push(mk("ColorTempFlash", 0x57, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xb0 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xbe, bo) {
        dm.push(("ColorTempUnknown2".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xc0 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xce, bo) {
        dm.push(("ColorTempUnknown3".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xd0 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xde, bo) {
        dm.push(("ColorTempUnknown4".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xe0 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xee, bo) {
        dm.push(("ColorTempUnknown5".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0xf0 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0xfe, bo) {
        dm.push(("ColorTempUnknown6".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x100 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x10e, bo) {
        dm.push(("ColorTempUnknown7".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x110 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x11e, bo) {
        dm.push(("ColorTempUnknown8".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x120 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x12e, bo) {
        dm.push(("ColorTempUnknown9".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x130 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x13e, bo) {
        dm.push(("ColorTempUnknown10".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x140 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x14e, bo) {
        dm.push(("ColorTempUnknown11".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x150 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x15e, bo) {
        dm.push(("ColorTempUnknown12".to_string(), f64::from(v)));
    }
    {
        let mut parts = Vec::new();
        for k in 0..4 {
            match i16_at(data, 0x160 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    if let Some(v) = i16_at(data, 0x16e, bo) {
        dm.push(("ColorTempUnknown13".to_string(), f64::from(v)));
    }
    tags
}

/// `Image::ExifTool::Canon::ColorCalib2` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colorcalib2(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    {
        let mut parts = Vec::new();
        for k in 0..5 {
            match i16_at(data, 0x0 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..5 {
            match i16_at(data, 0xa + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..5 {
            match i16_at(data, 0x14 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..5 {
            match i16_at(data, 0x1e + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..5 {
            match i16_at(data, 0x28 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..5 {
            match i16_at(data, 0x32 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..5 {
            match i16_at(data, 0x3c + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..5 {
            match i16_at(data, 0x46 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..5 {
            match i16_at(data, 0x50 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..5 {
            match i16_at(data, 0x5a + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..5 {
            match i16_at(data, 0x64 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..5 {
            match i16_at(data, 0x6e + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..5 {
            match i16_at(data, 0x78 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..5 {
            match i16_at(data, 0x82 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    {
        let mut parts = Vec::new();
        for k in 0..5 {
            match i16_at(data, 0x8c + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
    }
    tags
}

/// `Image::ExifTool::Canon::PSInfo` -- FORMAT int8u, FIRST_ENTRY 0.
fn canon_psinfo(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = i32_at(data, 0x0, bo) {
        dm.push(("ContrastStandard".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ContrastStandard", 0x0, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x4, bo) {
        dm.push(("SharpnessStandard".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SharpnessStandard", 0x4, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x8, bo) {
        dm.push(("SaturationStandard".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SaturationStandard", 0x8, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xc, bo) {
        dm.push(("ColorToneStandard".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ColorToneStandard", 0xc, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x10, bo) {
        dm.push(("FilterEffectStandard".to_string(), f64::from(v)));
    }
    if let Some(v) = i32_at(data, 0x14, bo) {
        dm.push(("ToningEffectStandard".to_string(), f64::from(v)));
    }
    if let Some(v) = i32_at(data, 0x18, bo) {
        dm.push(("ContrastPortrait".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ContrastPortrait", 0x18, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x1c, bo) {
        dm.push(("SharpnessPortrait".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SharpnessPortrait", 0x1c, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x20, bo) {
        dm.push(("SaturationPortrait".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SaturationPortrait", 0x20, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x24, bo) {
        dm.push(("ColorTonePortrait".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ColorTonePortrait", 0x24, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x28, bo) {
        dm.push(("FilterEffectPortrait".to_string(), f64::from(v)));
    }
    if let Some(v) = i32_at(data, 0x2c, bo) {
        dm.push(("ToningEffectPortrait".to_string(), f64::from(v)));
    }
    if let Some(v) = i32_at(data, 0x30, bo) {
        dm.push(("ContrastLandscape".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ContrastLandscape", 0x30, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x34, bo) {
        dm.push(("SharpnessLandscape".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SharpnessLandscape", 0x34, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x38, bo) {
        dm.push(("SaturationLandscape".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SaturationLandscape", 0x38, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x3c, bo) {
        dm.push(("ColorToneLandscape".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ColorToneLandscape", 0x3c, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x40, bo) {
        dm.push(("FilterEffectLandscape".to_string(), f64::from(v)));
    }
    if let Some(v) = i32_at(data, 0x44, bo) {
        dm.push(("ToningEffectLandscape".to_string(), f64::from(v)));
    }
    if let Some(v) = i32_at(data, 0x48, bo) {
        dm.push(("ContrastNeutral".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ContrastNeutral", 0x48, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x4c, bo) {
        dm.push(("SharpnessNeutral".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SharpnessNeutral", 0x4c, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x50, bo) {
        dm.push(("SaturationNeutral".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SaturationNeutral", 0x50, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x54, bo) {
        dm.push(("ColorToneNeutral".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ColorToneNeutral", 0x54, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x58, bo) {
        dm.push(("FilterEffectNeutral".to_string(), f64::from(v)));
    }
    if let Some(v) = i32_at(data, 0x5c, bo) {
        dm.push(("ToningEffectNeutral".to_string(), f64::from(v)));
    }
    if let Some(v) = i32_at(data, 0x60, bo) {
        dm.push(("ContrastFaithful".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ContrastFaithful", 0x60, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x64, bo) {
        dm.push(("SharpnessFaithful".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SharpnessFaithful", 0x64, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x68, bo) {
        dm.push(("SaturationFaithful".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SaturationFaithful", 0x68, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x6c, bo) {
        dm.push(("ColorToneFaithful".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ColorToneFaithful", 0x6c, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x70, bo) {
        dm.push(("FilterEffectFaithful".to_string(), f64::from(v)));
    }
    if let Some(v) = i32_at(data, 0x74, bo) {
        dm.push(("ToningEffectFaithful".to_string(), f64::from(v)));
    }
    if let Some(v) = i32_at(data, 0x78, bo) {
        dm.push(("ContrastMonochrome".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ContrastMonochrome", 0x78, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x7c, bo) {
        dm.push(("SharpnessMonochrome".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SharpnessMonochrome", 0x7c, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x80, bo) {
        dm.push(("SaturationMonochrome".to_string(), f64::from(v)));
    }
    if let Some(v) = i32_at(data, 0x84, bo) {
        dm.push(("ColorToneMonochrome".to_string(), f64::from(v)));
    }
    if let Some(v) = i32_at(data, 0x88, bo) {
        dm.push(("FilterEffectMonochrome".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            0 => "None".to_string(),
            1 => "Yellow".to_string(),
            2 => "Orange".to_string(),
            3 => "Red".to_string(),
            4 => "Green".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("FilterEffectMonochrome", 0x88, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x8c, bo) {
        dm.push(("ToningEffectMonochrome".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            0 => "None".to_string(),
            1 => "Sepia".to_string(),
            2 => "Blue".to_string(),
            3 => "Purple".to_string(),
            4 => "Green".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ToningEffectMonochrome", 0x8c, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x90, bo) {
        dm.push(("ContrastUserDef1".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ContrastUserDef1", 0x90, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x94, bo) {
        dm.push(("SharpnessUserDef1".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SharpnessUserDef1", 0x94, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x98, bo) {
        dm.push(("SaturationUserDef1".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SaturationUserDef1", 0x98, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x9c, bo) {
        dm.push(("ColorToneUserDef1".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ColorToneUserDef1", 0x9c, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xa0, bo) {
        dm.push(("FilterEffectUserDef1".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            0 => "None".to_string(),
            1 => "Yellow".to_string(),
            2 => "Orange".to_string(),
            3 => "Red".to_string(),
            4 => "Green".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("FilterEffectUserDef1", 0xa0, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xa4, bo) {
        dm.push(("ToningEffectUserDef1".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            0 => "None".to_string(),
            1 => "Sepia".to_string(),
            2 => "Blue".to_string(),
            3 => "Purple".to_string(),
            4 => "Green".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ToningEffectUserDef1", 0xa4, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xa8, bo) {
        dm.push(("ContrastUserDef2".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ContrastUserDef2", 0xa8, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xac, bo) {
        dm.push(("SharpnessUserDef2".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SharpnessUserDef2", 0xac, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xb0, bo) {
        dm.push(("SaturationUserDef2".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SaturationUserDef2", 0xb0, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xb4, bo) {
        dm.push(("ColorToneUserDef2".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ColorToneUserDef2", 0xb4, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xb8, bo) {
        dm.push(("FilterEffectUserDef2".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            0 => "None".to_string(),
            1 => "Yellow".to_string(),
            2 => "Orange".to_string(),
            3 => "Red".to_string(),
            4 => "Green".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("FilterEffectUserDef2", 0xb8, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xbc, bo) {
        dm.push(("ToningEffectUserDef2".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            0 => "None".to_string(),
            1 => "Sepia".to_string(),
            2 => "Blue".to_string(),
            3 => "Purple".to_string(),
            4 => "Green".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ToningEffectUserDef2", 0xbc, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xc0, bo) {
        dm.push(("ContrastUserDef3".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ContrastUserDef3", 0xc0, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xc4, bo) {
        dm.push(("SharpnessUserDef3".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SharpnessUserDef3", 0xc4, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xc8, bo) {
        dm.push(("SaturationUserDef3".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SaturationUserDef3", 0xc8, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xcc, bo) {
        dm.push(("ColorToneUserDef3".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ColorToneUserDef3", 0xcc, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xd0, bo) {
        dm.push(("FilterEffectUserDef3".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            0 => "None".to_string(),
            1 => "Yellow".to_string(),
            2 => "Orange".to_string(),
            3 => "Red".to_string(),
            4 => "Green".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("FilterEffectUserDef3", 0xd0, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xd4, bo) {
        dm.push(("ToningEffectUserDef3".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            0 => "None".to_string(),
            1 => "Sepia".to_string(),
            2 => "Blue".to_string(),
            3 => "Purple".to_string(),
            4 => "Green".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ToningEffectUserDef3", 0xd4, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0xd8, bo) {
        dm.push(("UserDef1PictureStyle".to_string(), f64::from(v)));
        let s = match v as i64 {
            65 => "PC 1".to_string(),
            66 => "PC 2".to_string(),
            67 => "PC 3".to_string(),
            129 => "Standard".to_string(),
            130 => "Portrait".to_string(),
            131 => "Landscape".to_string(),
            132 => "Neutral".to_string(),
            133 => "Faithful".to_string(),
            134 => "Monochrome".to_string(),
            135 => "Auto".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("UserDef1PictureStyle", 0xd8, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0xda, bo) {
        dm.push(("UserDef2PictureStyle".to_string(), f64::from(v)));
        let s = match v as i64 {
            65 => "PC 1".to_string(),
            66 => "PC 2".to_string(),
            67 => "PC 3".to_string(),
            129 => "Standard".to_string(),
            130 => "Portrait".to_string(),
            131 => "Landscape".to_string(),
            132 => "Neutral".to_string(),
            133 => "Faithful".to_string(),
            134 => "Monochrome".to_string(),
            135 => "Auto".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("UserDef2PictureStyle", 0xda, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0xdc, bo) {
        dm.push(("UserDef3PictureStyle".to_string(), f64::from(v)));
        let s = match v as i64 {
            65 => "PC 1".to_string(),
            66 => "PC 2".to_string(),
            67 => "PC 3".to_string(),
            129 => "Standard".to_string(),
            130 => "Portrait".to_string(),
            131 => "Landscape".to_string(),
            132 => "Neutral".to_string(),
            133 => "Faithful".to_string(),
            134 => "Monochrome".to_string(),
            135 => "Auto".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("UserDef3PictureStyle", 0xdc, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    tags
}

/// `Image::ExifTool::Canon::PSInfo2` -- FORMAT int8u, FIRST_ENTRY 0.
fn canon_psinfo2(data: &[u8], model: &str, bo: ByteOrder, file_type: &str, format: &str, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = crate::tag::PRIORITY_EXPLICIT_ZERO;
    let mut tags = Vec::new();
    let _ = (data, model, bo, file_type, format, &dm);
    if let Some(v) = i32_at(data, 0x0, bo) {
        dm.push(("ContrastStandard".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ContrastStandard", 0x0, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x4, bo) {
        dm.push(("SharpnessStandard".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SharpnessStandard", 0x4, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x8, bo) {
        dm.push(("SaturationStandard".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SaturationStandard", 0x8, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xc, bo) {
        dm.push(("ColorToneStandard".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ColorToneStandard", 0xc, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x10, bo) {
        dm.push(("FilterEffectStandard".to_string(), f64::from(v)));
    }
    if let Some(v) = i32_at(data, 0x14, bo) {
        dm.push(("ToningEffectStandard".to_string(), f64::from(v)));
    }
    if let Some(v) = i32_at(data, 0x18, bo) {
        dm.push(("ContrastPortrait".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ContrastPortrait", 0x18, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x1c, bo) {
        dm.push(("SharpnessPortrait".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SharpnessPortrait", 0x1c, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x20, bo) {
        dm.push(("SaturationPortrait".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SaturationPortrait", 0x20, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x24, bo) {
        dm.push(("ColorTonePortrait".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ColorTonePortrait", 0x24, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x28, bo) {
        dm.push(("FilterEffectPortrait".to_string(), f64::from(v)));
    }
    if let Some(v) = i32_at(data, 0x2c, bo) {
        dm.push(("ToningEffectPortrait".to_string(), f64::from(v)));
    }
    if let Some(v) = i32_at(data, 0x30, bo) {
        dm.push(("ContrastLandscape".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ContrastLandscape", 0x30, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x34, bo) {
        dm.push(("SharpnessLandscape".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SharpnessLandscape", 0x34, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x38, bo) {
        dm.push(("SaturationLandscape".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SaturationLandscape", 0x38, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x3c, bo) {
        dm.push(("ColorToneLandscape".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ColorToneLandscape", 0x3c, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x40, bo) {
        dm.push(("FilterEffectLandscape".to_string(), f64::from(v)));
    }
    if let Some(v) = i32_at(data, 0x44, bo) {
        dm.push(("ToningEffectLandscape".to_string(), f64::from(v)));
    }
    if let Some(v) = i32_at(data, 0x48, bo) {
        dm.push(("ContrastNeutral".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ContrastNeutral", 0x48, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x4c, bo) {
        dm.push(("SharpnessNeutral".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SharpnessNeutral", 0x4c, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x50, bo) {
        dm.push(("SaturationNeutral".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SaturationNeutral", 0x50, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x54, bo) {
        dm.push(("ColorToneNeutral".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ColorToneNeutral", 0x54, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x58, bo) {
        dm.push(("FilterEffectNeutral".to_string(), f64::from(v)));
    }
    if let Some(v) = i32_at(data, 0x5c, bo) {
        dm.push(("ToningEffectNeutral".to_string(), f64::from(v)));
    }
    if let Some(v) = i32_at(data, 0x60, bo) {
        dm.push(("ContrastFaithful".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ContrastFaithful", 0x60, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x64, bo) {
        dm.push(("SharpnessFaithful".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SharpnessFaithful", 0x64, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x68, bo) {
        dm.push(("SaturationFaithful".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SaturationFaithful", 0x68, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x6c, bo) {
        dm.push(("ColorToneFaithful".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ColorToneFaithful", 0x6c, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x70, bo) {
        dm.push(("FilterEffectFaithful".to_string(), f64::from(v)));
    }
    if let Some(v) = i32_at(data, 0x74, bo) {
        dm.push(("ToningEffectFaithful".to_string(), f64::from(v)));
    }
    if let Some(v) = i32_at(data, 0x78, bo) {
        dm.push(("ContrastMonochrome".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ContrastMonochrome", 0x78, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x7c, bo) {
        dm.push(("SharpnessMonochrome".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SharpnessMonochrome", 0x7c, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x80, bo) {
        dm.push(("SaturationMonochrome".to_string(), f64::from(v)));
    }
    if let Some(v) = i32_at(data, 0x84, bo) {
        dm.push(("ColorToneMonochrome".to_string(), f64::from(v)));
    }
    if let Some(v) = i32_at(data, 0x88, bo) {
        dm.push(("FilterEffectMonochrome".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            0 => "None".to_string(),
            1 => "Yellow".to_string(),
            2 => "Orange".to_string(),
            3 => "Red".to_string(),
            4 => "Green".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("FilterEffectMonochrome", 0x88, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x8c, bo) {
        dm.push(("ToningEffectMonochrome".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            0 => "None".to_string(),
            1 => "Sepia".to_string(),
            2 => "Blue".to_string(),
            3 => "Purple".to_string(),
            4 => "Green".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ToningEffectMonochrome", 0x8c, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x90, bo) {
        dm.push(("ContrastAuto".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ContrastAuto", 0x90, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x94, bo) {
        dm.push(("SharpnessAuto".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SharpnessAuto", 0x94, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x98, bo) {
        dm.push(("SaturationAuto".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SaturationAuto", 0x98, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0x9c, bo) {
        dm.push(("ColorToneAuto".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ColorToneAuto", 0x9c, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xa0, bo) {
        dm.push(("FilterEffectAuto".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            0 => "None".to_string(),
            1 => "Yellow".to_string(),
            2 => "Orange".to_string(),
            3 => "Red".to_string(),
            4 => "Green".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("FilterEffectAuto", 0xa0, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xa4, bo) {
        dm.push(("ToningEffectAuto".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            0 => "None".to_string(),
            1 => "Sepia".to_string(),
            2 => "Blue".to_string(),
            3 => "Purple".to_string(),
            4 => "Green".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ToningEffectAuto", 0xa4, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xa8, bo) {
        dm.push(("ContrastUserDef1".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ContrastUserDef1", 0xa8, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xac, bo) {
        dm.push(("SharpnessUserDef1".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SharpnessUserDef1", 0xac, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xb0, bo) {
        dm.push(("SaturationUserDef1".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SaturationUserDef1", 0xb0, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xb4, bo) {
        dm.push(("ColorToneUserDef1".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ColorToneUserDef1", 0xb4, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xb8, bo) {
        dm.push(("FilterEffectUserDef1".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            0 => "None".to_string(),
            1 => "Yellow".to_string(),
            2 => "Orange".to_string(),
            3 => "Red".to_string(),
            4 => "Green".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("FilterEffectUserDef1", 0xb8, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xbc, bo) {
        dm.push(("ToningEffectUserDef1".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            0 => "None".to_string(),
            1 => "Sepia".to_string(),
            2 => "Blue".to_string(),
            3 => "Purple".to_string(),
            4 => "Green".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ToningEffectUserDef1", 0xbc, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xc0, bo) {
        dm.push(("ContrastUserDef2".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ContrastUserDef2", 0xc0, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xc4, bo) {
        dm.push(("SharpnessUserDef2".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SharpnessUserDef2", 0xc4, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xc8, bo) {
        dm.push(("SaturationUserDef2".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SaturationUserDef2", 0xc8, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xcc, bo) {
        dm.push(("ColorToneUserDef2".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ColorToneUserDef2", 0xcc, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xd0, bo) {
        dm.push(("FilterEffectUserDef2".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            0 => "None".to_string(),
            1 => "Yellow".to_string(),
            2 => "Orange".to_string(),
            3 => "Red".to_string(),
            4 => "Green".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("FilterEffectUserDef2", 0xd0, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xd4, bo) {
        dm.push(("ToningEffectUserDef2".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            0 => "None".to_string(),
            1 => "Sepia".to_string(),
            2 => "Blue".to_string(),
            3 => "Purple".to_string(),
            4 => "Green".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ToningEffectUserDef2", 0xd4, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xd8, bo) {
        dm.push(("ContrastUserDef3".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ContrastUserDef3", 0xd8, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xdc, bo) {
        dm.push(("SharpnessUserDef3".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SharpnessUserDef3", 0xdc, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xe0, bo) {
        dm.push(("SaturationUserDef3".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("SaturationUserDef3", 0xe0, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xe4, bo) {
        dm.push(("ColorToneUserDef3".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ColorToneUserDef3", 0xe4, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xe8, bo) {
        dm.push(("FilterEffectUserDef3".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            0 => "None".to_string(),
            1 => "Yellow".to_string(),
            2 => "Orange".to_string(),
            3 => "Red".to_string(),
            4 => "Green".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("FilterEffectUserDef3", 0xe8, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = i32_at(data, 0xec, bo) {
        dm.push(("ToningEffectUserDef3".to_string(), f64::from(v)));
        let s = match v as i64 {
            -559038737 => "n/a".to_string(),
            0 => "None".to_string(),
            1 => "Sepia".to_string(),
            2 => "Blue".to_string(),
            3 => "Purple".to_string(),
            4 => "Green".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("ToningEffectUserDef3", 0xec, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0xf0, bo) {
        dm.push(("UserDef1PictureStyle".to_string(), f64::from(v)));
        let s = match v as i64 {
            65 => "PC 1".to_string(),
            66 => "PC 2".to_string(),
            67 => "PC 3".to_string(),
            129 => "Standard".to_string(),
            130 => "Portrait".to_string(),
            131 => "Landscape".to_string(),
            132 => "Neutral".to_string(),
            133 => "Faithful".to_string(),
            134 => "Monochrome".to_string(),
            135 => "Auto".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("UserDef1PictureStyle", 0xf0, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0xf2, bo) {
        dm.push(("UserDef2PictureStyle".to_string(), f64::from(v)));
        let s = match v as i64 {
            65 => "PC 1".to_string(),
            66 => "PC 2".to_string(),
            67 => "PC 3".to_string(),
            129 => "Standard".to_string(),
            130 => "Portrait".to_string(),
            131 => "Landscape".to_string(),
            132 => "Neutral".to_string(),
            133 => "Faithful".to_string(),
            134 => "Monochrome".to_string(),
            135 => "Auto".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("UserDef2PictureStyle", 0xf2, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0xf4, bo) {
        dm.push(("UserDef3PictureStyle".to_string(), f64::from(v)));
        let s = match v as i64 {
            65 => "PC 1".to_string(),
            66 => "PC 2".to_string(),
            67 => "PC 3".to_string(),
            129 => "Standard".to_string(),
            130 => "Portrait".to_string(),
            131 => "Landscape".to_string(),
            132 => "Neutral".to_string(),
            133 => "Faithful".to_string(),
            134 => "Monochrome".to_string(),
            135 => "Auto".to_string(),
            other => other.to_string(),
        };
        tags.push(mk("UserDef3PictureStyle", 0xf4, s, Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    tags
}

/// Which sub-table a Main-table id opens, by the conditions ExifTool writes
/// on it.
///
/// `None` means no arm matched, which for an id whose arms are all
/// sub-directories means ExifTool extracts nothing at all.
#[must_use]
pub fn variant_for(
    module: &str,
    tag: u16,
    model: &str,
    data: &[u8],
    count: usize,
    format: &str,
) -> Option<&'static str> {
    let _ = (model, data, count, format);
    match (module, tag) {
        ("Canon", 0x000d) => {
            if (count != 0 && MODEL_RE_11.is_match(model)) {
                return Some("CameraInfo1D");
            }
            if MODEL_RE_12.is_match(model) {
                return Some("CameraInfo1DmkII");
            }
            if MODEL_RE_13.is_match(model) {
                return Some("CameraInfo1DmkIIN");
            }
            if MODEL_RE_14.is_match(model) {
                return Some("CameraInfo1DmkIII");
            }
            if MODEL_RE_15.is_match(model) {
                return Some("CameraInfo1DmkIV");
            }
            if MODEL_RE_16.is_match(model) {
                return Some("CameraInfo1DX");
            }
            if MODEL_RE_17.is_match(model) {
                return Some("CameraInfo5D");
            }
            if MODEL_RE_18.is_match(model) {
                return Some("CameraInfo5DmkII");
            }
            if MODEL_RE_19.is_match(model) {
                return Some("CameraInfo5DmkIII");
            }
            if MODEL_RE_20.is_match(model) {
                return Some("CameraInfo6D");
            }
            if MODEL_RE_21.is_match(model) {
                return Some("CameraInfo7D");
            }
            if MODEL_RE_22.is_match(model) {
                return Some("CameraInfo40D");
            }
            if MODEL_RE_23.is_match(model) {
                return Some("CameraInfo50D");
            }
            if MODEL_RE_6.is_match(model) {
                return Some("CameraInfo60D");
            }
            if MODEL_RE_24.is_match(model) {
                return Some("CameraInfo70D");
            }
            if MODEL_RE_25.is_match(model) {
                return Some("CameraInfo80D");
            }
            if MODEL_RE_26.is_match(model) {
                return Some("CameraInfo450D");
            }
            if MODEL_RE_27.is_match(model) {
                return Some("CameraInfo500D");
            }
            if MODEL_RE_28.is_match(model) {
                return Some("CameraInfo550D");
            }
            if MODEL_RE_29.is_match(model) {
                return Some("CameraInfo600D");
            }
            if MODEL_RE_30.is_match(model) {
                return Some("CameraInfo650D");
            }
            if MODEL_RE_31.is_match(model) {
                return Some("CameraInfo650D");
            }
            if MODEL_RE_32.is_match(model) {
                return Some("CameraInfo750D");
            }
            if MODEL_RE_33.is_match(model) {
                return Some("CameraInfo750D");
            }
            if MODEL_RE_34.is_match(model) {
                return Some("CameraInfo1000D");
            }
            if MODEL_RE_35.is_match(model) {
                return Some("CameraInfo600D");
            }
            if MODEL_RE_7.is_match(model) {
                return Some("CameraInfo60D");
            }
            if MODEL_RE_36.is_match(model) {
                return Some("CameraInfoR6");
            }
            if MODEL_RE_37.is_match(model) {
                return Some("CameraInfoR6m2");
            }
            if MODEL_RE_38.is_match(model) {
                return Some("CameraInfoR6m3");
            }
            if MODEL_RE_39.is_match(model) {
                return Some("CameraInfoG5XII");
            }
            if (format == "int32u" && (count == 138 || count == 148)) {
                return Some("CameraInfoPowerShot");
            }
            if (format == "int32u" && (count == 156 || (count == 162 || (count == 167 || (count == 171 || count == 264))))) {
                return Some("CameraInfoPowerShot2");
            }
            if format.starts_with("int32") {
                return Some("CameraInfoUnknown32");
            }
            if format.starts_with("int16") {
                return Some("CameraInfoUnknown16");
            }
            Some("CameraInfoUnknown")
        }
        ("Canon", 0x4001) => {
            if count == 582 {
                return Some("ColorData1");
            }
            if count == 653 {
                return Some("ColorData2");
            }
            if count == 796 {
                return Some("ColorData3");
            }
            if (count == 692 || (count == 674 || (count == 702 || (count == 1227 || (count == 1250 || (count == 1251 || (count == 1337 || (count == 1338 || count == 1346)))))))) {
                return Some("ColorData4");
            }
            if count == 5120 {
                return Some("ColorData5");
            }
            if (count == 1273 || count == 1275) {
                return Some("ColorData6");
            }
            if (count == 1312 || (count == 1313 || (count == 1316 || count == 1506))) {
                return Some("ColorData7");
            }
            if (count == 1560 || (count == 1592 || (count == 1353 || count == 1602))) {
                return Some("ColorData8");
            }
            if (count == 1816 || (count == 1820 || count == 1824)) {
                return Some("ColorData9");
            }
            if (count == 2024 || count == 3656) {
                return Some("ColorData10");
            }
            if ((count == 3973 || count == 3778) && prefix_matches(data.get(0..).unwrap_or(&[]), &[Some((48, 64))])) {
                return Some("ColorData11");
            }
            if (count == 4528 || count == 3778) {
                return Some("ColorData12");
            }
            Some("ColorDataUnknown")
        }
        _ => None,
    }
}

/// What a Main-table id stores on the file while testing its own condition.
///
/// `($$self{CameraInfoCount} = $count) and ...` is an assignment used as the
/// test: ExifTool keeps the block's own length whether or not that arm is the
/// one taken, and the sub-table indexes its last fields from it. The caller
/// seeds the state with this before decoding.
#[must_use]
pub fn count_member(module: &str, tag: u16) -> Option<&'static str> {
    match (module, tag) {
        ("Canon", 0x000d) => Some("CameraInfoCount"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every generated pattern must compile: a bad one would otherwise
    /// only surface as a panic on whichever file first reaches it.
    #[test]
    fn every_model_pattern_compiles() {
        LazyLock::force(&MODEL_RE_0);
        LazyLock::force(&MODEL_RE_1);
        LazyLock::force(&MODEL_RE_2);
        LazyLock::force(&MODEL_RE_3);
        LazyLock::force(&MODEL_RE_4);
        LazyLock::force(&MODEL_RE_5);
        LazyLock::force(&MODEL_RE_6);
        LazyLock::force(&MODEL_RE_7);
        LazyLock::force(&MODEL_RE_8);
        LazyLock::force(&MODEL_RE_9);
        LazyLock::force(&MODEL_RE_10);
        LazyLock::force(&MODEL_RE_11);
        LazyLock::force(&MODEL_RE_12);
        LazyLock::force(&MODEL_RE_13);
        LazyLock::force(&MODEL_RE_14);
        LazyLock::force(&MODEL_RE_15);
        LazyLock::force(&MODEL_RE_16);
        LazyLock::force(&MODEL_RE_17);
        LazyLock::force(&MODEL_RE_18);
        LazyLock::force(&MODEL_RE_19);
        LazyLock::force(&MODEL_RE_20);
        LazyLock::force(&MODEL_RE_21);
        LazyLock::force(&MODEL_RE_22);
        LazyLock::force(&MODEL_RE_23);
        LazyLock::force(&MODEL_RE_24);
        LazyLock::force(&MODEL_RE_25);
        LazyLock::force(&MODEL_RE_26);
        LazyLock::force(&MODEL_RE_27);
        LazyLock::force(&MODEL_RE_28);
        LazyLock::force(&MODEL_RE_29);
        LazyLock::force(&MODEL_RE_30);
        LazyLock::force(&MODEL_RE_31);
        LazyLock::force(&MODEL_RE_32);
        LazyLock::force(&MODEL_RE_33);
        LazyLock::force(&MODEL_RE_34);
        LazyLock::force(&MODEL_RE_35);
        LazyLock::force(&MODEL_RE_36);
        LazyLock::force(&MODEL_RE_37);
        LazyLock::force(&MODEL_RE_38);
        LazyLock::force(&MODEL_RE_39);
    }
}
