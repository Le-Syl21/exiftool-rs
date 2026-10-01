//! The interpreter behind `binary_tables_generated`.
//!
//! A binary sub-table is a block of bytes addressed by index: ExifTool's
//! ProcessBinaryData reads the entry at `index * sizeof(FORMAT)`, and a
//! field's own Format says what to read there. The generated module describes
//! every table as data -- where each field sits, how it is stored, which
//! conversions and which print map it carries, which condition rules it in --
//! and `run` walks that description. Each table used to be unrolled into a
//! function of its own; the decisions that code made are now fields of a
//! [`Field`], and `run` takes for each one the branch the unrolled code took,
//! so a table decodes exactly as it did.

use crate::tag::{Tag, TagGroup, TagId};
use crate::tags::conv_expr::{self, Val as Conv};
use crate::value::Value;

/// Which end the file puts first.
pub type ByteOrder = crate::metadata::exif::ByteOrderMark;

/// What the fields of this block have read so far, by the name ExifTool
/// stores them under.
/// A member is whatever the field read: a Nikon FirmwareVersion is a string,
/// and the conditions on the Z9's custom settings compare it to one.
pub type State = Vec<(String, Conv)>;

pub(crate) fn dm_val<'a>(dm: &'a State, name: &str) -> Option<&'a Conv> {
    dm.iter().rev().find(|(n, _)| n == name).map(|(_, v)| v)
}

pub(crate) fn dm_get(dm: &State, name: &str) -> Option<f64> {
    dm_val(dm, name).map(Conv::as_num)
}

/// Whether the block opens with these bytes. A position is `None` when the
/// pattern accepts anything there, and otherwise a set of ranges the byte has
/// to fall in -- `[\x01\x02\x10\x20]` is four of them.
pub(crate) type BytePat<'a> = &'a [Option<&'a [(u8, u8)]>];

pub(crate) fn prefix_matches(d: &[u8], pat: BytePat) -> bool {
    if d.len() < pat.len() {
        return false;
    }
    pat.iter()
        .zip(d)
        .all(|(p, b)| p.is_none_or(|set| set.iter().any(|(lo, hi)| *b >= *lo && *b <= *hi)))
}

/// Whether the block opens the way any one of these patterns says.
pub(crate) fn prefix_matches_any(d: &[u8], pats: &[BytePat]) -> bool {
    pats.iter().any(|p| prefix_matches(d, p))
}

/// How a number is stored in the block.
#[derive(Clone, Copy)]
pub(crate) enum Fmt {
    U8,
    I8,
    U16,
    I16,
    U32,
    I32,
    /// ExifTool's `int16uRev`: 16 bits stored the other way round from the
    /// rest of the block.
    U16Rev,
    /// ExifTool's rational32: two 16-bit halves.
    R32U,
    // ExifTool reads this format and the signed fixed-point one below in a
    // binary table as it does any other, but none of the tables generated
    // today declares either.
    #[allow(dead_code)]
    R32S,
    /// The eight-byte rational EXIF writes: two 32-bit halves.
    R64U,
    R64S,
    F32,
    F64,
    /// A 32-bit fixed-point value: the number over 65536.
    Fix32U,
    #[allow(dead_code)]
    Fix32S,
}

/// What a field is, which decides the path `run` takes for it.
#[derive(Clone, Copy)]
pub(crate) enum Kind {
    /// One number: read, masked, stored in the state, converted.
    Num,
    /// That many numbers, joined by spaces.
    Array(usize),
    /// Bytes as text. `len` None runs to the end of the block; `nul` stops at
    /// the first NUL (`string`) or keeps every byte (`undef`).
    Text { len: Option<usize>, nul: bool },
    /// UTF-16 text of that many bytes, or to the end of the block.
    Unicode(Option<usize>),
}

/// Where a sub-directory's bytes are.
#[derive(Clone, Copy)]
pub(crate) enum Span {
    /// The entry holds an offset into the block, and the sub-directory runs
    /// from there to the end.
    Pointer(Fmt),
    /// That many bytes from the entry.
    Len(usize),
    /// From the entry to the end of the block.
    ToEnd,
}

/// A field's Condition, compiled: it is handed the block, the file's make,
/// model, type and format, and the state read so far.
pub(crate) type CondFn = fn(&[u8], &str, &str, &str, &str, &State) -> bool;

/// A PrintConv hash, sorted by key for a binary search.
pub(crate) type PrintMap = &'static [(i64, &'static str)];

/// One entry of a table.
pub(crate) struct Field {
    pub(crate) name: &'static str,
    /// The entry's index, which is the tag id.
    pub(crate) id: u16,
    /// The entry's byte offset in the block.
    pub(crate) at: usize,
    pub(crate) fmt: Fmt,
    pub(crate) kind: Kind,
    /// 0 when the field has no Mask.
    pub(crate) mask: i64,
    pub(crate) shift: u32,
    /// `Unknown => 1`: not reported unless -u is given, which is not the
    /// default. The row is described in full all the same; only a number is
    /// still read, for the state.
    pub(crate) hidden: bool,
    /// The name the value is stored under when it is not the field's own: a
    /// number always stores itself, text only when it is a DataMember.
    pub(crate) dm: Option<&'static str>,
    pub(crate) cond: Option<CondFn>,
    pub(crate) rconv: Option<&'static str>,
    pub(crate) vconv: Option<&'static str>,
    pub(crate) pconv: Option<&'static str>,
    pub(crate) map: Option<PrintMap>,
}

impl Field {
    /// What a field is unless its table says otherwise: one int8u, read as it
    /// is and reported.
    pub(crate) const DEFAULT: Field = Field {
        name: "",
        id: 0,
        at: 0,
        fmt: Fmt::U8,
        kind: Kind::Num,
        mask: 0,
        shift: 0,
        hidden: false,
        dm: None,
        cond: None,
        rconv: None,
        vconv: None,
        pconv: None,
        map: None,
    };
}

/// A field that opens another table on part of the block.
pub(crate) struct Sub {
    /// The entry's byte offset in the block.
    pub(crate) at: usize,
    pub(crate) table: &'static Table,
    pub(crate) span: Span,
    pub(crate) cond: Option<CondFn>,
}

/// One binary sub-table: its family 0-2 groups, its priority, and its
/// entries in the order they are decoded.
pub(crate) struct Table {
    pub(crate) groups: [&'static str; 3],
    pub(crate) prio: i32,
    pub(crate) fields: &'static [Field],
    pub(crate) subs: &'static [Sub],
}

/// What the caller knows about the file the block comes from.
pub(crate) struct FileInfo<'a> {
    pub(crate) make: &'a str,
    pub(crate) model: &'a str,
    pub(crate) bo: ByteOrder,
    pub(crate) file_type: &'a str,
    pub(crate) format: &'a str,
}

/// Decode the table called `name`, or nothing when `tables`, sorted by name,
/// has no such table.
pub(crate) fn decode(
    tables: &[(&str, &Table)],
    name: &str,
    data: &[u8],
    file: &FileInfo,
    dm: &mut State,
) -> Vec<Tag> {
    match tables.binary_search_by(|(n, _)| (*n).cmp(name)) {
        Ok(i) => run(tables[i].1, data, file, dm),
        Err(_) => Vec::new(),
    }
}

/// What a conversion of this block can ask the file about.
///
/// ExifTool keeps these on the object: `$$self{Model}` tells one encoding of
/// TargetExposureTime from another, and `$$self{FILE_TYPE} eq "CRW"` decides
/// whether an ExposureTime of zero means one second or nothing at all.
struct Ctx<'a> {
    file: &'a FileInfo<'a>,
    dm: &'a State,
}

impl conv_expr::ParseState for Ctx<'_> {
    /// The reader options a conversion asks about. ExifTool's ByteUnit
    /// defaults to SI, and nothing here can set it to Binary.
    fn option(&self, name: &str) -> Option<Conv> {
        match name {
            "ByteUnit" => Some(Conv::Str("SI".to_string())),
            _ => None,
        }
    }

    fn member(&self, name: &str) -> Option<Conv> {
        match name {
            "Make" => Some(Conv::Str(self.file.make.to_string())),
            "Model" => Some(Conv::Str(self.file.model.to_string())),
            "FILE_TYPE" | "FileType" => Some(Conv::Str(self.file.file_type.to_string())),
            _ => dm_val(self.dm, name).cloned(),
        }
    }
}

/// Whether a conversion's answer lets the tag through: `undef` does not.
fn passes(c: Option<&Conv>) -> bool {
    c != Some(&Conv::Undef)
}

/// Walk a table over its block: every field in byte order, then every
/// sub-directory in the order the table lists them.
#[allow(clippy::too_many_lines)]
fn run(t: &Table, data: &[u8], file: &FileInfo, dm: &mut State) -> Vec<Tag> {
    let mut tags = Vec::new();
    let tag = |name: &str, id: u16, print_value: String, raw: Value| {
        mk(name, id, print_value, raw, t.groups, t.prio)
    };
    for f in t.fields {
        if let Some(c) = f.cond {
            if !c(data, file.make, file.model, file.file_type, file.format, dm) {
                continue;
            }
        }
        match f.kind {
            Kind::Num => {
                let Some(n) = read(data, f.at, f.fmt, file.bo) else {
                    continue;
                };
                let n = match n {
                    Num::Int(i) if f.mask != 0 => Num::Int((i & f.mask) >> f.shift),
                    n => n,
                };
                let v = n.to_f64();
                dm.push((f.dm.unwrap_or(f.name).to_string(), Conv::Num(v)));
                // The value goes into the state even when the field is not
                // reported, because a later field can be conditioned on it;
                // the conversions produce nothing but the tag, so they are
                // not run at all.
                if f.hidden {
                    continue;
                }
                let ctx = Ctx { file, dm };
                let mut base = Conv::Num(v);
                // A RawConv can rule the value out -- `$val ? $val : undef`
                // -- and it can change what kind of number it is: AVIHeader's
                // FrameRate is `1e6 / $val`, and casting that back to the
                // integer the reader produced turned 29.97 into 29.
                if let Some(e) = f.rconv {
                    let rc = conv_expr::eval_with(e, &base, &ctx);
                    if !passes(rc.as_ref()) {
                        continue;
                    }
                    base = rc.unwrap_or(base);
                }
                if let Some(map) = f.map {
                    // A hash conversion looks up the value AFTER any
                    // ValueConv, and a ValueConv that returns undef means no
                    // tag at all: H264's ExposureProgram is `$val == 15 ?
                    // undef : $val` and has a hash of its own, so running
                    // only the hash reported the raw 15.
                    if let Some(e) = f.vconv {
                        let vc = conv_expr::eval_with(e, &base, &ctx);
                        if !passes(vc.as_ref()) {
                            continue;
                        }
                        base = vc.unwrap_or(base);
                    }
                    #[allow(clippy::cast_possible_truncation)]
                    let k = base.as_num() as i64;
                    let s = match map.binary_search_by_key(&k, |e| e.0) {
                        Ok(i) => map[i].1.to_string(),
                        Err(_) => k.to_string(),
                    };
                    #[allow(clippy::cast_possible_truncation)]
                    let raw = Value::I32(base.as_num() as i32);
                    tags.push(tag(f.name, f.id, s, raw));
                } else if f.vconv.is_some() || f.pconv.is_some() {
                    let mut cv = base;
                    // A ValueConv that returns undef means the tag is not
                    // extracted at all.
                    if let Some(e) = f.vconv {
                        let vc = conv_expr::eval_with(e, &cv, &ctx);
                        if !passes(vc.as_ref()) {
                            continue;
                        }
                        if let Some(x) = vc {
                            cv = x;
                        }
                    }
                    let raw = Value::F64(cv.as_num());
                    if let Some(e) = f.pconv {
                        if let Some(x) = conv_expr::eval_with(e, &cv, &ctx) {
                            cv = x;
                        }
                    }
                    tags.push(tag(f.name, f.id, cv.as_string(), raw));
                } else {
                    #[allow(clippy::cast_possible_truncation)]
                    let raw = Value::I32(base.as_num() as i32);
                    tags.push(tag(f.name, f.id, base.as_string(), raw));
                }
            }
            // However many the Format says: ExifTool joins them all, and
            // WB_RedLevelsKelvin really is seventy-five numbers. An array
            // stores nothing in the state and has no Mask.
            Kind::Array(count) => {
                // Read and dropped when hidden, which has no effect at all.
                if f.hidden {
                    continue;
                }
                let w = width(f.fmt);
                let mut parts = Vec::new();
                for k in 0..count {
                    match read(data, f.at + k * w, f.fmt, file.bo) {
                        Some(x) => parts.push(x.to_text()),
                        None => {
                            parts.clear();
                            break;
                        }
                    }
                }
                if parts.is_empty() {
                    continue;
                }
                let s = parts.join(" ");
                // An array is ruled out by a RawConv as much as a scalar is:
                // Panasonic's Face2Position is `$$self{NumFacePositions} < 2
                // ? undef : $val`. The value the RawConv returns is not used.
                if let Some(e) = f.rconv {
                    let ctx = Ctx { file, dm };
                    let rc = conv_expr::eval_with(e, &Conv::Str(s.clone()), &ctx);
                    if !passes(rc.as_ref()) {
                        continue;
                    }
                }
                // An array carries its conversions as much as a scalar does:
                // RawMeasuredRGGB is four int32u read back with their halves
                // exchanged. A ValueConv here cannot rule the tag out.
                if f.vconv.is_some() || f.pconv.is_some() {
                    let ctx = Ctx { file, dm };
                    let mut cv = Conv::Str(s);
                    if let Some(e) = f.vconv {
                        if let Some(x) = conv_expr::eval_with(e, &cv, &ctx) {
                            cv = x;
                        }
                    }
                    let raw = Value::String(cv.as_string());
                    if let Some(e) = f.pconv {
                        if let Some(x) = conv_expr::eval_with(e, &cv, &ctx) {
                            cv = x;
                        }
                    }
                    tags.push(tag(f.name, f.id, cv.as_string(), raw));
                } else {
                    tags.push(tag(f.name, f.id, s.clone(), Value::String(s)));
                }
            }
            Kind::Text { len, nul } => {
                // Not even read when hidden, its DataMember included.
                if f.hidden {
                    continue;
                }
                let n = len.unwrap_or_else(|| data.len().saturating_sub(f.at));
                let Some(text) = text_at(data, f.at, n, nul) else {
                    continue;
                };
                // A string stores itself where ExifTool names it a
                // DataMember: the Z9's FirmwareVersion decides which of three
                // menu-settings layouts applies.
                if let Some(name) = f.dm {
                    dm.push((name.to_string(), Conv::Str(text.clone())));
                }
                // A text field carries its conversions as much as a number
                // does: a RawConv rules SerialInfo's two entries out unless
                // they look like a serial number.
                if f.rconv.is_some() || f.vconv.is_some() || f.pconv.is_some() {
                    let ctx = Ctx { file, dm };
                    let mut cv = Conv::Str(text);
                    if let Some(e) = f.rconv {
                        let rc = conv_expr::eval_with(e, &cv, &ctx);
                        if !passes(rc.as_ref()) {
                            continue;
                        }
                        if let Some(x) = rc {
                            cv = x;
                        }
                    }
                    if let Some(e) = f.vconv {
                        if let Some(x) = conv_expr::eval_with(e, &cv, &ctx) {
                            cv = x;
                        }
                    }
                    let raw = Value::String(cv.as_string());
                    if let Some(e) = f.pconv {
                        if let Some(x) = conv_expr::eval_with(e, &cv, &ctx) {
                            cv = x;
                        }
                    }
                    tags.push(tag(f.name, f.id, cv.as_string(), raw));
                } else {
                    tags.push(tag(f.name, f.id, text.clone(), Value::String(text)));
                }
            }
            // UTF-16 in the block's own byte order, reported as it is.
            Kind::Unicode(len) => {
                if f.hidden {
                    continue;
                }
                let n = len.unwrap_or_else(|| data.len().saturating_sub(f.at));
                if let Some(text) = utf16_at(data, f.at, n, file.bo) {
                    tags.push(tag(f.name, f.id, text.clone(), Value::String(text)));
                }
            }
        }
    }
    for s in t.subs {
        if let Some(c) = s.cond {
            if !c(data, file.make, file.model, file.file_type, file.format, dm) {
                continue;
            }
        }
        let sub = match s.span {
            // A negative offset wraps to one no block is long enough for.
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            Span::Pointer(fmt) => match read(data, s.at, fmt, file.bo) {
                Some(Num::Int(at)) => data.get(at as usize..),
                _ => None,
            },
            Span::Len(n) => data.get(s.at..s.at + n),
            Span::ToEnd => data.get(s.at..),
        };
        if let Some(sub) = sub {
            tags.extend(run(s.table, sub, file, dm));
        }
    }
    tags
}

fn mk(
    name: &str,
    id: u16,
    print_value: String,
    raw: Value,
    groups: [&str; 3],
    priority: i32,
) -> Tag {
    let [grp0, grp1, grp2] = groups;
    Tag {
        id: TagId::Numeric(id),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: grp0.into(),
            family1: grp1.into(),
            family2: grp2.into(),
            family3: crate::tag::MAIN_DOCUMENT.into(),
        },
        raw_value: raw,
        print_value,
        priority,
    }
}

/// A number as the reader produced it: only an integer can be masked, and a
/// float keeps its width because an array prints each element in it.
#[derive(Clone, Copy)]
enum Num {
    Int(i64),
    F32(f32),
    F64(f64),
}

impl Num {
    fn to_f64(self) -> f64 {
        match self {
            // Every integer format is 32 bits at most, so this is exact.
            #[allow(clippy::cast_precision_loss)]
            Num::Int(i) => i as f64,
            Num::F32(x) => f64::from(x),
            Num::F64(x) => x,
        }
    }

    fn to_text(self) -> String {
        match self {
            Num::Int(i) => i.to_string(),
            Num::F32(x) => x.to_string(),
            Num::F64(x) => x.to_string(),
        }
    }
}

fn read(d: &[u8], o: usize, fmt: Fmt, bo: ByteOrder) -> Option<Num> {
    Some(match fmt {
        Fmt::U8 => Num::Int(d.get(o).copied()?.into()),
        Fmt::I8 => Num::Int(d.get(o).map(|b| *b as i8)?.into()),
        Fmt::U16 => Num::Int(u16_at(d, o, bo)?.into()),
        Fmt::I16 => Num::Int(i16_at(d, o, bo)?.into()),
        Fmt::U32 => Num::Int(u32_at(d, o, bo)?.into()),
        Fmt::I32 => Num::Int(i32_at(d, o, bo)?.into()),
        Fmt::U16Rev => Num::Int(u16_at(d, o, other(bo))?.into()),
        // A zero denominator reads as infinity, and 0/0 as nothing at all.
        Fmt::R32U => Num::F64(ratio(
            f64::from(u16_at(d, o, bo)?),
            f64::from(u16_at(d, o + 2, bo)?),
        )?),
        Fmt::R32S => Num::F64(ratio(
            f64::from(i16_at(d, o, bo)?),
            f64::from(i16_at(d, o + 2, bo)?),
        )?),
        Fmt::R64U => Num::F64(ratio(
            f64::from(u32_at(d, o, bo)?),
            f64::from(u32_at(d, o + 4, bo)?),
        )?),
        Fmt::R64S => Num::F64(ratio(
            f64::from(i32_at(d, o, bo)?),
            f64::from(i32_at(d, o + 4, bo)?),
        )?),
        Fmt::F32 => Num::F32(f32::from_bits(u32_at(d, o, bo)?)),
        Fmt::F64 => Num::F64(f64_at(d, o, bo)?),
        Fmt::Fix32U => Num::F64(f64::from(u32_at(d, o, bo)?) / 65536.0),
        Fmt::Fix32S => Num::F64(f64::from(i32_at(d, o, bo)?) / 65536.0),
    })
}

fn width(fmt: Fmt) -> usize {
    match fmt {
        Fmt::U8 | Fmt::I8 => 1,
        Fmt::U16 | Fmt::I16 | Fmt::U16Rev => 2,
        Fmt::U32 | Fmt::I32 | Fmt::R32U | Fmt::R32S | Fmt::F32 | Fmt::Fix32U | Fmt::Fix32S => 4,
        Fmt::R64U | Fmt::R64S | Fmt::F64 => 8,
    }
}

fn other(bo: ByteOrder) -> ByteOrder {
    if bo == ByteOrder::BigEndian {
        ByteOrder::LittleEndian
    } else {
        ByteOrder::BigEndian
    }
}

fn u16_at(d: &[u8], o: usize, bo: ByteOrder) -> Option<u16> {
    let b = [*d.get(o)?, *d.get(o + 1)?];
    Some(if bo == ByteOrder::BigEndian {
        u16::from_be_bytes(b)
    } else {
        u16::from_le_bytes(b)
    })
}

fn i16_at(d: &[u8], o: usize, bo: ByteOrder) -> Option<i16> {
    u16_at(d, o, bo).map(|v| v as i16)
}

fn u32_at(d: &[u8], o: usize, bo: ByteOrder) -> Option<u32> {
    let b = [*d.get(o)?, *d.get(o + 1)?, *d.get(o + 2)?, *d.get(o + 3)?];
    Some(if bo == ByteOrder::BigEndian {
        u32::from_be_bytes(b)
    } else {
        u32::from_le_bytes(b)
    })
}

fn i32_at(d: &[u8], o: usize, bo: ByteOrder) -> Option<i32> {
    u32_at(d, o, bo).map(|v| v as i32)
}

fn f64_at(d: &[u8], o: usize, bo: ByteOrder) -> Option<f64> {
    let mut b = [0u8; 8];
    b.copy_from_slice(d.get(o..o + 8)?);
    Some(if bo == ByteOrder::BigEndian {
        f64::from_be_bytes(b)
    } else {
        f64::from_le_bytes(b)
    })
}

fn ratio(n: f64, d: f64) -> Option<f64> {
    if d == 0.0 {
        return if n == 0.0 { None } else { Some(f64::INFINITY) };
    }
    Some(n / d)
}

/// N bytes as UTF-16 text, in the block's byte order.
fn utf16_at(d: &[u8], o: usize, n: usize, bo: ByteOrder) -> Option<String> {
    let raw = d.get(o..o + n)?;
    let units: Vec<u16> = raw
        .chunks_exact(2)
        .map(|c| {
            if bo == ByteOrder::BigEndian {
                u16::from_be_bytes([c[0], c[1]])
            } else {
                u16::from_le_bytes([c[0], c[1]])
            }
        })
        .take_while(|u| *u != 0)
        .collect();
    Some(String::from_utf16_lossy(&units))
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

#[cfg(test)]
mod tests {
    use super::*;

    const F: Field = Field::DEFAULT;

    static NAMES: PrintMap = &[(1, "One"), (3, "Three")];

    static INNER: Table = Table {
        groups: ["MakerNotes", "Inner", "Camera"],
        prio: 0,
        fields: &[Field {
            name: "InnerByte",
            id: 0,
            at: 0,
            ..F
        }],
        subs: &[],
    };

    static OUTER: Table = Table {
        groups: ["MakerNotes", "Outer", "Camera"],
        prio: 0,
        fields: &[
            Field {
                name: "HighNibble",
                id: 0,
                at: 0,
                mask: 0xf0,
                shift: 4,
                map: Some(NAMES),
                ..F
            },
            Field {
                name: "Stored",
                id: 1,
                at: 1,
                dm: Some("Member"),
                hidden: true,
                ..F
            },
            Field {
                name: "Pair",
                id: 2,
                at: 2,
                fmt: Fmt::U16,
                kind: Kind::Array(2),
                ..F
            },
            Field {
                name: "HiddenPair",
                id: 6,
                at: 6,
                kind: Kind::Array(2),
                hidden: true,
                ..F
            },
            Field {
                name: "Text",
                id: 8,
                at: 8,
                kind: Kind::Text {
                    len: Some(4),
                    nul: true,
                },
                ..F
            },
            Field {
                name: "RuledOut",
                id: 12,
                at: 12,
                rconv: Some("$val ? $val : undef"),
                ..F
            },
            Field {
                name: "Conditioned",
                id: 13,
                at: 13,
                cond: Some(member_is_seven),
                ..F
            },
        ],
        subs: &[Sub {
            at: 14,
            table: &INNER,
            span: Span::Pointer(Fmt::U8),
            cond: None,
        }],
    };

    fn member_is_seven(_: &[u8], _: &str, _: &str, _: &str, _: &str, dm: &State) -> bool {
        dm_get(dm, "Member") == Some(7.0)
    }

    fn decode_outer(data: &[u8], dm: &mut State) -> Vec<(String, String)> {
        let file = FileInfo {
            make: "",
            model: "",
            bo: ByteOrder::BigEndian,
            file_type: "",
            format: "",
        };
        let tables: &[(&str, &Table)] = &[("Inner", &INNER), ("Outer", &OUTER)];
        decode(tables, "Outer", data, &file, dm)
            .into_iter()
            .map(|t| (t.name, t.print_value))
            .collect()
    }

    /// Each kind of field takes its own path: a mask and a map on a number, a
    /// hidden number that only stores itself, arrays joined by spaces, text
    /// cut at its NUL, a RawConv that rules a value out, a condition on what
    /// an earlier field stored, and a sub-directory found through a pointer.
    #[test]
    fn every_kind_of_field_decodes() {
        let data = [
            0x35, 7, 0, 1, 0, 2, 9, 9, b'a', b'b', 0, b'c', 0, 42, 16, 0, 0x55,
        ];
        let mut dm = State::new();
        let tags = decode_outer(&data, &mut dm);
        let want = [
            ("HighNibble", "Three"),
            ("Pair", "1 2"),
            ("Text", "ab"),
            ("Conditioned", "42"),
            ("InnerByte", "85"),
        ];
        let got: Vec<(&str, &str)> = tags.iter().map(|(n, v)| (n.as_str(), v.as_str())).collect();
        assert_eq!(got, want);
        let names: Vec<&str> = dm.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "HighNibble",
                "Member",
                "RuledOut",
                "Conditioned",
                "InnerByte"
            ]
        );
    }

    /// A value the map does not name prints as the number, and a condition
    /// that does not hold leaves its field out.
    #[test]
    fn misses_fall_back_to_the_number() {
        let data = [0x25, 6, 0, 1, 0, 2, 9, 9, b'a', b'b', 0, b'c', 5, 42, 0xff];
        let mut dm = State::new();
        let tags = decode_outer(&data, &mut dm);
        let got: Vec<(&str, &str)> = tags.iter().map(|(n, v)| (n.as_str(), v.as_str())).collect();
        assert_eq!(
            got,
            [
                ("HighNibble", "2"),
                ("Pair", "1 2"),
                ("Text", "ab"),
                ("RuledOut", "5")
            ]
        );
    }

    #[test]
    fn an_unknown_name_decodes_nothing() {
        let file = FileInfo {
            make: "",
            model: "",
            bo: ByteOrder::LittleEndian,
            file_type: "",
            format: "",
        };
        let tables: &[(&str, &Table)] = &[("Inner", &INNER), ("Outer", &OUTER)];
        assert!(decode(tables, "Missing", &[1, 2, 3], &file, &mut State::new()).is_empty());
    }
}
