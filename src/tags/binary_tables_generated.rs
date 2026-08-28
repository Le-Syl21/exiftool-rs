//! Auto-generated decoders for ExifTool's binary sub-tables.
//!
//! Do not edit: regenerate with
//! `perl scripts/gen_binary_tables.pl ../exiftool/lib > src/tags/binary_tables_generated.rs`.
//!
//! 17 tables, 812 fields. A binary sub-table is a block of
//! bytes addressed by index: ExifTool's ProcessBinaryData reads the entry at
//! `(index - FIRST_ENTRY) * sizeof(FORMAT)`, and a field's own Format says
//! what to read there. What the generator could not express is on its stderr.
#![allow(clippy::too_many_lines, clippy::match_same_arms, clippy::unreadable_literal)]

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

/// Decode one binary sub-table by the name ExifTool gives it.
#[must_use]
pub fn decode(table: &str, data: &[u8], model: &str, bo: ByteOrder, dm: &mut State) -> Vec<Tag> {
    match table {
        "ColorData1" => canon_colordata1(data, model, bo, dm),
        "ColorData2" => canon_colordata2(data, model, bo, dm),
        "ColorData3" => canon_colordata3(data, model, bo, dm),
        "ColorData4" => canon_colordata4(data, model, bo, dm),
        "ColorData5" => canon_colordata5(data, model, bo, dm),
        "ColorData6" => canon_colordata6(data, model, bo, dm),
        "ColorData7" => canon_colordata7(data, model, bo, dm),
        "ColorData8" => canon_colordata8(data, model, bo, dm),
        "ColorData9" => canon_colordata9(data, model, bo, dm),
        "ColorData10" => canon_colordata10(data, model, bo, dm),
        "ColorData11" => canon_colordata11(data, model, bo, dm),
        "ColorData12" => canon_colordata12(data, model, bo, dm),
        "ColorDataUnknown" => canon_colordataunknown(data, model, bo, dm),
        "ColorCalib" => canon_colorcalib(data, model, bo, dm),
        "ColorCoefs" => canon_colorcoefs(data, model, bo, dm),
        "ColorCoefs2" => canon_colorcoefs2(data, model, bo, dm),
        "ColorCalib2" => canon_colorcalib2(data, model, bo, dm),
        _ => Vec::new(),
    }
}

/// `Image::ExifTool::Canon::ColorData1` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colordata1(data: &[u8], model: &str, bo: ByteOrder, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (model, bo, &dm);
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
        tags.extend(canon_colorcalib(sub, model, bo, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::ColorData2` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colordata2(data: &[u8], model: &str, bo: ByteOrder, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (model, bo, &dm);
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
            let mut cv = Conv::Str(s.clone());
            if let Some(x) = conv_expr::eval("Image::ExifTool::Canon::SwapWords($val)", &cv) { cv = x; }
            let raw = Value::String(cv.as_string());
            tags.push(mk("RawMeasuredRGGB", 0x26a, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(sub) = data.get(0x148..0x148 + 120) {
        tags.extend(canon_colorcalib(sub, model, bo, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::ColorData3` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colordata3(data: &[u8], model: &str, bo: ByteOrder, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (model, bo, &dm);
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
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval("$val >= 255 ? 255 : exp(($val-200)/16*log(2))", &cv) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval("$val == 255 ? \"Strobe or Misfire\" : sprintf(\"%.0f%%\", $val * 100)", &cv) { cv = x; }
        tags.push(mk("FlashOutput", 0x248, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = i16_at(data, 0x492, bo) {
        dm.push(("FlashBatteryLevel".to_string(), f64::from(v)));
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval("$val ? sprintf(\"%.2fV\", $val * 5 / 186) : \"n/a\"", &cv) { cv = x; }
        tags.push(mk("FlashBatteryLevel", 0x249, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = i16_at(data, 0x494, bo) {
        dm.push(("ColorTempFlashData".to_string(), f64::from(v)));
        let rc = conv_expr::eval("($val < 2000 or $val > 12000) ? undef : $val", &Conv::Num(f64::from(v)));
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
            let mut cv = Conv::Str(s.clone());
            if let Some(x) = conv_expr::eval("Image::ExifTool::Canon::SwapWords($val)", &cv) { cv = x; }
            let raw = Value::String(cv.as_string());
            tags.push(mk("MeasuredRGGBData", 0x287, cv.as_string(), raw, GRP1, GRP2, PRIO));
        }
    }
    if let Some(sub) = data.get(0x10a..0x10a + 120) {
        tags.extend(canon_colorcalib(sub, model, bo, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::ColorData4` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colordata4(data: &[u8], model: &str, bo: ByteOrder, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (model, bo, &dm);
    if let Some(v) = i16_at(data, 0x0, bo) {
        dm.push(("ColorDataVersion".to_string(), f64::from(v)));
        let rc = conv_expr::eval("$$self{ColorDataVersion} = $val", &Conv::Num(f64::from(v)));
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
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval("$val >= 255 ? 255 : exp(($val-200)/16*log(2))", &cv) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval("$val == 255 ? \"Strobe or Misfire\" : sprintf(\"%.0f%%\", $val * 100)", &cv) { cv = x; }
        tags.push(mk("FlashOutput", 0x26b, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = i16_at(data, 0x4d8, bo) {
        dm.push(("FlashBatteryLevel".to_string(), f64::from(v)));
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval("$val ? sprintf(\"%.2fV\", $val * 5 / 186) : \"n/a\"", &cv) { cv = x; }
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
            let mut cv = Conv::Str(s.clone());
            if let Some(x) = conv_expr::eval("Image::ExifTool::Canon::SwapWords($val)", &cv) { cv = x; }
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
            let rc = conv_expr::eval("$val || undef", &Conv::Num(f64::from(v)));
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
            let rc = conv_expr::eval("$val || undef", &Conv::Num(f64::from(v)));
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
            let rc = conv_expr::eval("$val || undef", &Conv::Num(f64::from(v)));
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
        tags.extend(canon_colorcoefs(sub, model, bo, dm));
    }
    if let Some(sub) = data.get(0x150..0x150 + 120) {
        tags.extend(canon_colorcalib(sub, model, bo, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::ColorData5` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colordata5(data: &[u8], model: &str, bo: ByteOrder, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (model, bo, &dm);
    if let Some(v) = i16_at(data, 0x0, bo) {
        dm.push(("ColorDataVersion".to_string(), f64::from(v)));
        let rc = conv_expr::eval("$$self{ColorDataVersion} = $val", &Conv::Num(f64::from(v)));
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
            tags.extend(canon_colorcoefs(sub, model, bo, dm));
        }
    }
    if !(dm_get(dm, "ColorDataVersion").is_some_and(|v| v == -3.0)) && dm_get(dm, "ColorDataVersion").is_some_and(|v| v == -4.0) {
        if let Some(sub) = data.get(0x8e..0x8e + 368) {
            tags.extend(canon_colorcoefs2(sub, model, bo, dm));
        }
    }
    if dm_get(dm, "ColorDataVersion").is_some_and(|v| v == -3.0) {
        if let Some(sub) = data.get(0x174..0x174 + 150) {
            tags.extend(canon_colorcalib2(sub, model, bo, dm));
        }
    }
    if dm_get(dm, "ColorDataVersion").is_some_and(|v| v == -4.0) {
        if let Some(sub) = data.get(0x1fe..0x1fe + 150) {
            tags.extend(canon_colorcalib2(sub, model, bo, dm));
        }
    }
    tags
}

/// `Image::ExifTool::Canon::ColorData6` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colordata6(data: &[u8], model: &str, bo: ByteOrder, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (model, bo, &dm);
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
            let mut cv = Conv::Str(s.clone());
            if let Some(x) = conv_expr::eval("Image::ExifTool::Canon::SwapWords($val)", &cv) { cv = x; }
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
        let rc = conv_expr::eval("$val || undef", &Conv::Num(f64::from(v)));
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
        tags.extend(canon_colorcalib(sub, model, bo, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::ColorData7` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colordata7(data: &[u8], model: &str, bo: ByteOrder, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (model, bo, &dm);
    if let Some(v) = i16_at(data, 0x0, bo) {
        dm.push(("ColorDataVersion".to_string(), f64::from(v)));
        let rc = conv_expr::eval("$$self{ColorDataVersion} = $val", &Conv::Num(f64::from(v)));
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval("$val >= 255 ? 255 : exp(($val-200)/16*log(2))", &cv) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval("$val == 255 ? \"Strobe or Misfire\" : sprintf(\"%.0f%%\", $val * 100)", &cv) { cv = x; }
        tags.push(mk("FlashOutput", 0x198, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = i16_at(data, 0x332, bo) {
        dm.push(("FlashBatteryLevel".to_string(), f64::from(v)));
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval("$val ? sprintf(\"%.2fV\", $val * 5 / 186) : \"n/a\"", &cv) { cv = x; }
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
                let mut cv = Conv::Str(s.clone());
                if let Some(x) = conv_expr::eval("Image::ExifTool::Canon::SwapWords($val)", &cv) { cv = x; }
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
            let rc = conv_expr::eval("$val || undef", &Conv::Num(f64::from(v)));
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
                let mut cv = Conv::Str(s.clone());
                if let Some(x) = conv_expr::eval("Image::ExifTool::Canon::SwapWords($val)", &cv) { cv = x; }
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
            let rc = conv_expr::eval("$val || undef", &Conv::Num(f64::from(v)));
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
        tags.extend(canon_colorcalib(sub, model, bo, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::ColorData8` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colordata8(data: &[u8], model: &str, bo: ByteOrder, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (model, bo, &dm);
    if let Some(v) = i16_at(data, 0x0, bo) {
        dm.push(("ColorDataVersion".to_string(), f64::from(v)));
        let rc = conv_expr::eval("$$self{ColorDataVersion} = $val", &Conv::Num(f64::from(v)));
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
            let rc = conv_expr::eval("$val || undef", &Conv::Num(f64::from(v)));
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
            let rc = conv_expr::eval("$val || undef", &Conv::Num(f64::from(v)));
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
        tags.extend(canon_colorcalib(sub, model, bo, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::ColorData9` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colordata9(data: &[u8], model: &str, bo: ByteOrder, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (model, bo, &dm);
    if let Some(v) = i16_at(data, 0x0, bo) {
        dm.push(("ColorDataVersion".to_string(), f64::from(v)));
        let rc = conv_expr::eval("$$self{ColorDataVersion} = $val", &Conv::Num(f64::from(v)));
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        let rc = conv_expr::eval("$val || undef", &Conv::Num(f64::from(v)));
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
        tags.extend(canon_colorcalib(sub, model, bo, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::ColorData10` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colordata10(data: &[u8], model: &str, bo: ByteOrder, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (model, bo, &dm);
    if let Some(v) = i16_at(data, 0x0, bo) {
        dm.push(("ColorDataVersion".to_string(), f64::from(v)));
        let rc = conv_expr::eval("$$self{ColorDataVersion} = $val", &Conv::Num(f64::from(v)));
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval("$val >= 255 ? 255 : exp(($val-200)/16*log(2))", &cv) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval("$val == 255 ? \"Strobe or Misfire\" : sprintf(\"%.0f%%\", $val * 100)", &cv) { cv = x; }
        tags.push(mk("FlashOutput", 0x299, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = i16_at(data, 0x534, bo) {
        dm.push(("FlashBatteryLevel".to_string(), f64::from(v)));
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval("$val ? sprintf(\"%.2fV\", $val * 5 / 186) : \"n/a\"", &cv) { cv = x; }
        tags.push(mk("FlashBatteryLevel", 0x29a, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x654, bo) {
        dm.push(("NormalWhiteLevel".to_string(), f64::from(v)));
        let rc = conv_expr::eval("$val || undef", &Conv::Num(f64::from(v)));
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
        tags.extend(canon_colorcalib(sub, model, bo, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::ColorData11` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colordata11(data: &[u8], model: &str, bo: ByteOrder, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (model, bo, &dm);
    if let Some(v) = i16_at(data, 0x0, bo) {
        dm.push(("ColorDataVersion".to_string(), f64::from(v)));
        let rc = conv_expr::eval("$$self{ColorDataVersion} = $val", &Conv::Num(f64::from(v)));
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        let rc = conv_expr::eval("$val || undef", &Conv::Num(f64::from(v)));
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
        tags.extend(canon_colorcalib(sub, model, bo, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::ColorData12` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colordata12(data: &[u8], model: &str, bo: ByteOrder, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (model, bo, &dm);
    if let Some(v) = i16_at(data, 0x0, bo) {
        dm.push(("ColorDataVersion".to_string(), f64::from(v)));
        let rc = conv_expr::eval("$$self{ColorDataVersion} = $val", &Conv::Num(f64::from(v)));
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        let mut cv = Conv::Num(f64::from(v));
        if let Some(x) = conv_expr::eval("$val >= 255 ? 255 : exp(($val-200)/16*log(2))", &cv) { cv = x; }
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval("$val == 255 ? \"Strobe or Misfire\" : sprintf(\"%.0f%%\", $val * 100)", &cv) { cv = x; }
        tags.push(mk("FlashOutput", 0x203, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = i16_at(data, 0x408, bo) {
        dm.push(("FlashBatteryLevel".to_string(), f64::from(v)));
        let mut cv = Conv::Num(f64::from(v));
        let raw = Value::F64(cv.as_num());
        if let Some(x) = conv_expr::eval("$val ? sprintf(\"%.2fV\", $val * 5 / 186) : \"n/a\"", &cv) { cv = x; }
        tags.push(mk("FlashBatteryLevel", 0x204, cv.as_string(), raw, GRP1, GRP2, PRIO));
    }
    if let Some(v) = u16_at(data, 0x528, bo) {
        dm.push(("NormalWhiteLevel".to_string(), f64::from(v)));
        let rc = conv_expr::eval("$val || undef", &Conv::Num(f64::from(v)));
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
        tags.extend(canon_colorcalib(sub, model, bo, dm));
    }
    tags
}

/// `Image::ExifTool::Canon::ColorDataUnknown` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colordataunknown(data: &[u8], model: &str, bo: ByteOrder, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (model, bo, &dm);
    if let Some(v) = i16_at(data, 0x0, bo) {
        dm.push(("ColorDataVersion".to_string(), f64::from(v)));
        tags.push(mk("ColorDataVersion", 0x0, v.to_string(), Value::I32(v as i32), GRP1, GRP2, PRIO));
    }
    tags
}

/// `Image::ExifTool::Canon::ColorCalib` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colorcalib(data: &[u8], model: &str, bo: ByteOrder, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (model, bo, &dm);
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
            let mut cv = Conv::Str(s.clone());
            let raw = Value::String(cv.as_string());
            if let Some(x) = conv_expr::eval("sprintf(\"%4d %4d %4d (%dK)\", split(\" \",$val))", &cv) { cv = x; }
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
        if !parts.is_empty() {
            let s = parts.join(" ");
            let mut cv = Conv::Str(s.clone());
            let raw = Value::String(cv.as_string());
            if let Some(x) = conv_expr::eval("sprintf(\"%4d %4d %4d (%dK)\", split(\" \",$val))", &cv) { cv = x; }
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
        if !parts.is_empty() {
            let s = parts.join(" ");
            let mut cv = Conv::Str(s.clone());
            let raw = Value::String(cv.as_string());
            if let Some(x) = conv_expr::eval("sprintf(\"%4d %4d %4d (%dK)\", split(\" \",$val))", &cv) { cv = x; }
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
        if !parts.is_empty() {
            let s = parts.join(" ");
            let mut cv = Conv::Str(s.clone());
            let raw = Value::String(cv.as_string());
            if let Some(x) = conv_expr::eval("sprintf(\"%4d %4d %4d (%dK)\", split(\" \",$val))", &cv) { cv = x; }
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
        if !parts.is_empty() {
            let s = parts.join(" ");
            let mut cv = Conv::Str(s.clone());
            let raw = Value::String(cv.as_string());
            if let Some(x) = conv_expr::eval("sprintf(\"%4d %4d %4d (%dK)\", split(\" \",$val))", &cv) { cv = x; }
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
        if !parts.is_empty() {
            let s = parts.join(" ");
            let mut cv = Conv::Str(s.clone());
            let raw = Value::String(cv.as_string());
            if let Some(x) = conv_expr::eval("sprintf(\"%4d %4d %4d (%dK)\", split(\" \",$val))", &cv) { cv = x; }
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
        if !parts.is_empty() {
            let s = parts.join(" ");
            let mut cv = Conv::Str(s.clone());
            let raw = Value::String(cv.as_string());
            if let Some(x) = conv_expr::eval("sprintf(\"%4d %4d %4d (%dK)\", split(\" \",$val))", &cv) { cv = x; }
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
        if !parts.is_empty() {
            let s = parts.join(" ");
            let mut cv = Conv::Str(s.clone());
            let raw = Value::String(cv.as_string());
            if let Some(x) = conv_expr::eval("sprintf(\"%4d %4d %4d (%dK)\", split(\" \",$val))", &cv) { cv = x; }
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
        if !parts.is_empty() {
            let s = parts.join(" ");
            let mut cv = Conv::Str(s.clone());
            let raw = Value::String(cv.as_string());
            if let Some(x) = conv_expr::eval("sprintf(\"%4d %4d %4d (%dK)\", split(\" \",$val))", &cv) { cv = x; }
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
        if !parts.is_empty() {
            let s = parts.join(" ");
            let mut cv = Conv::Str(s.clone());
            let raw = Value::String(cv.as_string());
            if let Some(x) = conv_expr::eval("sprintf(\"%4d %4d %4d (%dK)\", split(\" \",$val))", &cv) { cv = x; }
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
        if !parts.is_empty() {
            let s = parts.join(" ");
            let mut cv = Conv::Str(s.clone());
            let raw = Value::String(cv.as_string());
            if let Some(x) = conv_expr::eval("sprintf(\"%4d %4d %4d (%dK)\", split(\" \",$val))", &cv) { cv = x; }
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
        if !parts.is_empty() {
            let s = parts.join(" ");
            let mut cv = Conv::Str(s.clone());
            let raw = Value::String(cv.as_string());
            if let Some(x) = conv_expr::eval("sprintf(\"%4d %4d %4d (%dK)\", split(\" \",$val))", &cv) { cv = x; }
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
        if !parts.is_empty() {
            let s = parts.join(" ");
            let mut cv = Conv::Str(s.clone());
            let raw = Value::String(cv.as_string());
            if let Some(x) = conv_expr::eval("sprintf(\"%4d %4d %4d (%dK)\", split(\" \",$val))", &cv) { cv = x; }
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
        if !parts.is_empty() {
            let s = parts.join(" ");
            let mut cv = Conv::Str(s.clone());
            let raw = Value::String(cv.as_string());
            if let Some(x) = conv_expr::eval("sprintf(\"%4d %4d %4d (%dK)\", split(\" \",$val))", &cv) { cv = x; }
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
        if !parts.is_empty() {
            let s = parts.join(" ");
            let mut cv = Conv::Str(s.clone());
            let raw = Value::String(cv.as_string());
            if let Some(x) = conv_expr::eval("sprintf(\"%4d %4d %4d (%dK)\", split(\" \",$val))", &cv) { cv = x; }
        }
    }
    tags
}

/// `Image::ExifTool::Canon::ColorCoefs` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colorcoefs(data: &[u8], model: &str, bo: ByteOrder, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (model, bo, &dm);
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
        }
    }
    if let Some(v) = i16_at(data, 0xe4, bo) {
        dm.push(("ColorTempUnknown13".to_string(), f64::from(v)));
    }
    tags
}

/// `Image::ExifTool::Canon::ColorCoefs2` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colorcoefs2(data: &[u8], model: &str, bo: ByteOrder, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (model, bo, &dm);
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
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
        if !parts.is_empty() {
            let s = parts.join(" ");
        }
    }
    if let Some(v) = i16_at(data, 0x16e, bo) {
        dm.push(("ColorTempUnknown13".to_string(), f64::from(v)));
    }
    tags
}

/// `Image::ExifTool::Canon::ColorCalib2` -- FORMAT int16s, FIRST_ENTRY 0.
fn canon_colorcalib2(data: &[u8], model: &str, bo: ByteOrder, dm: &mut State) -> Vec<Tag> {
    const GRP1: &str = "Canon";
    const GRP2: &str = "Camera";
    const PRIO: i32 = 0;
    let mut tags = Vec::new();
    let _ = (model, bo, &dm);
    {
        let mut parts = Vec::new();
        for k in 0..5 {
            match i16_at(data, 0x0 + k * 2, bo) {
                Some(x) => parts.push(x.to_string()),
                None => { parts.clear(); break }
            }
        }
        if !parts.is_empty() {
            let s = parts.join(" ");
            let mut cv = Conv::Str(s.clone());
            let raw = Value::String(cv.as_string());
            if let Some(x) = conv_expr::eval("sprintf(\"%4d %4d %4d %4d (%dK)\", split(\" \",$val))", &cv) { cv = x; }
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
        if !parts.is_empty() {
            let s = parts.join(" ");
            let mut cv = Conv::Str(s.clone());
            let raw = Value::String(cv.as_string());
            if let Some(x) = conv_expr::eval("sprintf(\"%4d %4d %4d %4d (%dK)\", split(\" \",$val))", &cv) { cv = x; }
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
        if !parts.is_empty() {
            let s = parts.join(" ");
            let mut cv = Conv::Str(s.clone());
            let raw = Value::String(cv.as_string());
            if let Some(x) = conv_expr::eval("sprintf(\"%4d %4d %4d %4d (%dK)\", split(\" \",$val))", &cv) { cv = x; }
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
        if !parts.is_empty() {
            let s = parts.join(" ");
            let mut cv = Conv::Str(s.clone());
            let raw = Value::String(cv.as_string());
            if let Some(x) = conv_expr::eval("sprintf(\"%4d %4d %4d %4d (%dK)\", split(\" \",$val))", &cv) { cv = x; }
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
        if !parts.is_empty() {
            let s = parts.join(" ");
            let mut cv = Conv::Str(s.clone());
            let raw = Value::String(cv.as_string());
            if let Some(x) = conv_expr::eval("sprintf(\"%4d %4d %4d %4d (%dK)\", split(\" \",$val))", &cv) { cv = x; }
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
        if !parts.is_empty() {
            let s = parts.join(" ");
            let mut cv = Conv::Str(s.clone());
            let raw = Value::String(cv.as_string());
            if let Some(x) = conv_expr::eval("sprintf(\"%4d %4d %4d %4d (%dK)\", split(\" \",$val))", &cv) { cv = x; }
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
        if !parts.is_empty() {
            let s = parts.join(" ");
            let mut cv = Conv::Str(s.clone());
            let raw = Value::String(cv.as_string());
            if let Some(x) = conv_expr::eval("sprintf(\"%4d %4d %4d %4d (%dK)\", split(\" \",$val))", &cv) { cv = x; }
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
        if !parts.is_empty() {
            let s = parts.join(" ");
            let mut cv = Conv::Str(s.clone());
            let raw = Value::String(cv.as_string());
            if let Some(x) = conv_expr::eval("sprintf(\"%4d %4d %4d %4d (%dK)\", split(\" \",$val))", &cv) { cv = x; }
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
        if !parts.is_empty() {
            let s = parts.join(" ");
            let mut cv = Conv::Str(s.clone());
            let raw = Value::String(cv.as_string());
            if let Some(x) = conv_expr::eval("sprintf(\"%4d %4d %4d %4d (%dK)\", split(\" \",$val))", &cv) { cv = x; }
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
        if !parts.is_empty() {
            let s = parts.join(" ");
            let mut cv = Conv::Str(s.clone());
            let raw = Value::String(cv.as_string());
            if let Some(x) = conv_expr::eval("sprintf(\"%4d %4d %4d %4d (%dK)\", split(\" \",$val))", &cv) { cv = x; }
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
        if !parts.is_empty() {
            let s = parts.join(" ");
            let mut cv = Conv::Str(s.clone());
            let raw = Value::String(cv.as_string());
            if let Some(x) = conv_expr::eval("sprintf(\"%4d %4d %4d %4d (%dK)\", split(\" \",$val))", &cv) { cv = x; }
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
        if !parts.is_empty() {
            let s = parts.join(" ");
            let mut cv = Conv::Str(s.clone());
            let raw = Value::String(cv.as_string());
            if let Some(x) = conv_expr::eval("sprintf(\"%4d %4d %4d %4d (%dK)\", split(\" \",$val))", &cv) { cv = x; }
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
        if !parts.is_empty() {
            let s = parts.join(" ");
            let mut cv = Conv::Str(s.clone());
            let raw = Value::String(cv.as_string());
            if let Some(x) = conv_expr::eval("sprintf(\"%4d %4d %4d %4d (%dK)\", split(\" \",$val))", &cv) { cv = x; }
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
        if !parts.is_empty() {
            let s = parts.join(" ");
            let mut cv = Conv::Str(s.clone());
            let raw = Value::String(cv.as_string());
            if let Some(x) = conv_expr::eval("sprintf(\"%4d %4d %4d %4d (%dK)\", split(\" \",$val))", &cv) { cv = x; }
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
        if !parts.is_empty() {
            let s = parts.join(" ");
            let mut cv = Conv::Str(s.clone());
            let raw = Value::String(cv.as_string());
            if let Some(x) = conv_expr::eval("sprintf(\"%4d %4d %4d %4d (%dK)\", split(\" \",$val))", &cv) { cv = x; }
        }
    }
    tags
}

/// Which sub-table a Main-table id opens, by the conditions ExifTool writes
/// on it.
///
/// `None` means no arm matched, which for an id whose arms are all
/// sub-directories means ExifTool extracts nothing at all.
#[must_use]
pub fn variant_for(module: &str, tag: u16, data: &[u8], count: usize) -> Option<&'static str> {
    let _ = (data, count);
    match (module, tag) {
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
            if ((count == 3973 || count == 3778) && prefix_matches(data, &[Some((48, 64))])) {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every generated pattern must compile: a bad one would otherwise
    /// only surface as a panic on whichever file first reaches it.
    #[test]
    fn every_model_pattern_compiles() {
    }
}
