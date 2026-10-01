//! The interpreter behind `sony_ciphered_generated`.
//!
//! Sony's ciphered sub-tables are addressed by byte offset and, once
//! deciphered, read little-endian; a field's own Format says what to read at
//! its offset. The generated module describes every table as data -- one row
//! per field, in the order the fields are read, then the sub-directories --
//! and `run` walks those rows. Each table used to be unrolled into a function
//! of its own; the choices that code made are now the [`Shape`] of a row, and
//! `run` takes for each one the branch the unrolled code took, so a table
//! decodes exactly as it did.
//!
//! This is deliberately not the `binary_tables` interpreter. The two
//! generators disagree on more than the data: whether an offset is scaled by
//! the table's FORMAT, whether a RawConv's answer is cast back to the field's
//! own type, whether an int32u wraps or saturates, what an array does with
//! its conversions. One walker for both would need a switch for each of those
//! or would change what one side prints.

use std::sync::LazyLock;

use regex_lite::Regex;

use crate::tag::{Tag, TagGroup, TagId};
use crate::tags::conv_expr::{self, Val as Conv};
use crate::value::Value;

/// What the sub-tables have read so far in this file.
///
/// ExifTool calls these DATAMEMBERs and keeps them on the object, not on the
/// table: Tag9050 reads LensMount, and Tag940c -- a different block entirely --
/// decides whether to report LensE-mountVersion by what it said.
///
/// A member is whatever the field read: MetaVersion is a string, and the
/// conditions on FocusMode compare it to one.
pub type State = Vec<(String, Conv)>;

/// How a number is stored in the block. Every one of them is little-endian.
#[derive(Clone, Copy)]
pub(crate) enum Fmt {
    U8,
    I8,
    U16,
    I16,
    U32,
    // The generator reads int32s and rational32s as it does the other
    // formats, but no Sony table generated today declares a field of either.
    #[allow(dead_code)]
    I32,
    /// ExifTool's rational32: two 16-bit halves. It is four bytes wide, not
    /// the eight of the rational64 EXIF writes -- reading it as two 32-bit
    /// halves turned 1/20 into 1/271.
    R32U,
    #[allow(dead_code)]
    R32S,
}

/// A field's Condition when it is more than a test of the model: a
/// DataMember, the Software, or the guards of `[{...},{...}]` alternatives,
/// compiled by the generator. It is handed the model and the state so far.
pub(crate) type CondFn = fn(&str, &State) -> bool;

/// When a field, or a sub-directory, is read at all.
pub(crate) enum Cond {
    Always,
    /// `$$self{Model} =~ /.../`.
    Model(&'static LazyLock<Regex>),
    /// `$$self{Model} !~ /.../`.
    NotModel(&'static LazyLock<Regex>),
    Fn(CondFn),
}

/// A PrintConv hash, sorted by key for a binary search.
pub(crate) type PrintMap = &'static [(i64, &'static str)];

/// How a single number prints.
pub(crate) enum Print {
    /// As the number it is.
    Plain,
    /// Through a PrintConv hash; a value it does not name prints as the
    /// number.
    Map(PrintMap),
    /// Through a ValueConv and a PrintConv expression, either of which may be
    /// missing.
    Expr {
        value: Option<&'static str>,
        print: Option<&'static str>,
    },
}

/// How an array of numbers prints. Any other conversion it carries is not
/// applied: that is what the unrolled code did.
pub(crate) enum ArrayPrint {
    /// The values joined by spaces.
    Plain,
    /// `unpack "H*", pack "C*", split " ", $val`: each value as two hex
    /// digits, run together.
    Hex,
    /// A hash keyed by the whole joined value -- Shutter's is
    /// `{ '0 0 0' => 'Silent / Electronic (0 0 0)' }` -- with an OTHER
    /// expression for anything it does not name.
    Keyed {
        keys: &'static [(&'static str, &'static str)],
        other: Option<&'static str>,
    },
}

/// What a field is, which decides the path `run` takes for it.
pub(crate) enum Shape {
    /// One number: read, masked, stored in the state, ruled out or reshaped
    /// by a RawConv, printed.
    Scalar {
        /// 0 when the field has no Mask; `shift` brings its lowest bit down
        /// to bit 0.
        mask: u32,
        shift: u32,
        /// The name the value is stored under: the field's own unless the
        /// source names a DataMember.
        member: &'static str,
        raw_conv: Option<&'static str>,
        print: Print,
    },
    /// `count` numbers of the field's format, one after the other. An array
    /// stores nothing in the state.
    Array { count: u16, print: ArrayPrint },
    /// `string[N]`, which stops at its first NUL, or `undef[N]`, which keeps
    /// every byte.
    Text {
        len: u16,
        stop_at_nul: bool,
        /// Where ExifTool names the text a DataMember: stored only when the
        /// field carries no conversion.
        member: Option<&'static str>,
        value: Option<&'static str>,
        print: Option<&'static str>,
    },
}

/// One entry of a table.
pub(crate) struct Field {
    /// The entry's byte offset, which ExifTool reports as its tag id.
    pub(crate) off: u16,
    pub(crate) name: &'static str,
    /// What a number is read as; text ignores it.
    pub(crate) fmt: Fmt,
    pub(crate) cond: Cond,
    /// `Hidden => 1`: a number is still read and stored, for the fields after
    /// it, but nothing is reported.
    pub(crate) hidden: bool,
    /// The field's own family 2, when it overrides the table's. ExifTool
    /// takes it from the tag's `Groups` and falls back to the table's; there
    /// is no lookup by name in that chain.
    pub(crate) group2: Option<&'static str>,
    pub(crate) shape: Shape,
}

impl Field {
    /// What a field is unless its table says otherwise: one int8u, read
    /// whatever the model, reported, and printed as the number.
    pub(crate) const DEFAULT: Field = Field {
        off: 0,
        name: "",
        fmt: Fmt::U8,
        cond: Cond::Always,
        hidden: false,
        group2: None,
        shape: Shape::Scalar {
            mask: 0,
            shift: 0,
            member: "",
            raw_conv: None,
            print: Print::Plain,
        },
    };
}

/// A field that opens another table on `len` bytes of the block.
pub(crate) struct SubDir {
    pub(crate) off: u16,
    pub(crate) len: u16,
    pub(crate) cond: Cond,
    pub(crate) table: &'static Table,
}

/// One sub-table: its family 2, its priority, and its entries in the order
/// they are decoded.
pub(crate) struct Table {
    pub(crate) group2: &'static str,
    pub(crate) priority: i32,
    pub(crate) fields: &'static [Field],
    pub(crate) subdirs: &'static [SubDir],
}

impl Cond {
    fn holds(&self, model: &str, dm: &State) -> bool {
        match self {
            Cond::Always => true,
            Cond::Model(re) => re.is_match(model),
            Cond::NotModel(re) => !re.is_match(model),
            Cond::Fn(f) => f(model, dm),
        }
    }
}

/// Walk a table over its deciphered block: every field in byte order, then
/// every sub-directory in the order the table lists them.
///
/// A condition is tested just before its field is read, against the state as
/// the fields before it left it.
pub(crate) fn run(t: &Table, data: &[u8], model: &str, dm: &mut State) -> Vec<Tag> {
    let mut tags = Vec::new();
    for f in t.fields {
        if !f.cond.holds(model, dm) {
            continue;
        }
        let at = usize::from(f.off);
        match &f.shape {
            Shape::Scalar {
                mask,
                shift,
                member,
                raw_conv,
                print,
            } => {
                let Some(mut v) = read(data, at, f.fmt) else {
                    continue;
                };
                if *mask != 0 {
                    if let Num::Int(i) = v {
                        v = Num::Int((i & i64::from(*mask)) >> shift);
                    }
                }
                dm.push(((*member).to_string(), Conv::Num(v.to_f64())));
                // A RawConv can rule the value out entirely -- `$val ? $val :
                // undef` means "only when it is not zero" -- or reshape it, and
                // what it answers goes back into the field's own type. One the
                // evaluator declines leaves the raw value.
                if let Some(e) = raw_conv {
                    let rc = conv_expr::eval(e, &Conv::Num(v.to_f64()));
                    if rc.as_ref() == Some(&Conv::Undef) {
                        continue;
                    }
                    if let Some(x) = rc {
                        v = cast_back(f.fmt, x.as_num());
                    }
                }
                // The value is in the state, which is all a hidden field is
                // read for; the conversions produce nothing but the tag.
                if f.hidden {
                    continue;
                }
                tags.push(match print {
                    Print::Map(map) => {
                        let k = v.to_i64();
                        let s = match map.binary_search_by_key(&k, |e| e.0) {
                            Ok(i) => map[i].1.to_string(),
                            Err(_) => k.to_string(),
                        };
                        mk(f, t, s, v.to_value())
                    }
                    Print::Expr { value, print } => {
                        let mut cv = Conv::Num(v.to_f64());
                        eval_into(*value, &mut cv);
                        let raw = Value::F64(cv.as_num());
                        eval_into(*print, &mut cv);
                        mk(f, t, cv.as_string(), raw)
                    }
                    Print::Plain => mk(f, t, v.to_text(), v.to_value()),
                });
            }
            Shape::Array { count, print } => {
                // An array stores nothing, so a hidden one is not even read.
                if f.hidden {
                    continue;
                }
                let w = width(f.fmt);
                let mut parts = Vec::new();
                for k in 0..usize::from(*count) {
                    match read(data, at + k * w, f.fmt) {
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
                let printed = match print {
                    ArrayPrint::Plain => s.clone(),
                    ArrayPrint::Hex => parts
                        .iter()
                        .map(|p| format!("{:02x}", p.parse::<u32>().unwrap_or(0)))
                        .collect(),
                    ArrayPrint::Keyed { keys, other } => match keys.iter().find(|e| e.0 == s) {
                        Some(e) => e.1.to_string(),
                        None => match other {
                            Some(e) => conv_expr::eval(e, &Conv::Str(s.clone()))
                                .map_or_else(|| s.clone(), |v| v.as_string()),
                            None => s.clone(),
                        },
                    },
                };
                tags.push(mk(f, t, printed, Value::String(s)));
            }
            Shape::Text {
                len,
                stop_at_nul,
                member,
                value,
                print,
            } => {
                let Some(text) = text_at(data, at, usize::from(*len), *stop_at_nul) else {
                    continue;
                };
                // The conversions apply to text as much as to numbers:
                // LensSpecFeatures is two bytes read as hex and then named.
                // A text field with a conversion stores nothing, so a hidden
                // one has nothing left to do.
                if value.is_some() || print.is_some() {
                    if f.hidden {
                        continue;
                    }
                    let mut cv = Conv::Str(text);
                    eval_into(*value, &mut cv);
                    let raw = Value::String(cv.as_string());
                    eval_into(*print, &mut cv);
                    tags.push(mk(f, t, cv.as_string(), raw));
                } else {
                    // MetaVersion is read in ShotInfo, and the Main table's
                    // two FocusMode offsets are told apart by it.
                    if let Some(name) = member {
                        dm.push(((*name).to_string(), Conv::Str(text.clone())));
                    }
                    if !f.hidden {
                        tags.push(mk(f, t, text.clone(), Value::String(text)));
                    }
                }
            }
        }
    }
    for sd in t.subdirs {
        if !sd.cond.holds(model, dm) {
            continue;
        }
        let at = usize::from(sd.off);
        if let Some(sub) = data.get(at..at + usize::from(sd.len)) {
            tags.extend(run(sd.table, sub, model, dm));
        }
    }
    tags
}

/// Run an expression over the value, keeping the value when the evaluator
/// declines it.
fn eval_into(expr: Option<&str>, cv: &mut Conv) {
    if let Some(e) = expr {
        if let Some(x) = conv_expr::eval(e, cv) {
            *cv = x;
        }
    }
}

/// A tag as the table reports it: addressed by its offset, which is how
/// ExifTool tells apart the pairs it keeps -- Tag9050b defines
/// APS-CSizeCapture at both 0x0114 and 0x01ee, and an ILCE-9 reports both.
fn mk(f: &Field, t: &Table, print_value: String, raw: Value) -> Tag {
    Tag {
        id: TagId::Numeric(f.off),
        name: f.name.to_string(),
        description: f.name.to_string(),
        group: TagGroup {
            family0: "MakerNotes".into(),
            family1: "Sony".into(),
            family2: f.group2.unwrap_or(t.group2).into(),
            family3: crate::tag::MAIN_DOCUMENT.into(),
        },
        raw_value: raw,
        print_value,
        priority: t.priority,
    }
}

/// A number as the reader produced it: an integer of the field's own type,
/// held widened, or a ratio.
#[derive(Clone, Copy)]
enum Num {
    Int(i64),
    Float(f64),
}

impl Num {
    fn to_f64(self) -> f64 {
        match self {
            // Every integer format is 32 bits at most, so this is exact.
            #[allow(clippy::cast_precision_loss)]
            Num::Int(i) => i as f64,
            Num::Float(x) => x,
        }
    }

    /// The key a PrintConv hash is looked up by.
    #[allow(clippy::cast_possible_truncation)]
    fn to_i64(self) -> i64 {
        match self {
            Num::Int(i) => i,
            Num::Float(x) => x as i64,
        }
    }

    fn to_text(self) -> String {
        match self {
            Num::Int(i) => i.to_string(),
            Num::Float(x) => x.to_string(),
        }
    }

    /// The raw value of a number printed as it is or through a hash: its low
    /// 32 bits, so an int32u of 2^31 or more comes out negative.
    #[allow(clippy::cast_possible_truncation)]
    fn to_value(self) -> Value {
        Value::I32(self.to_i64() as i32)
    }
}

fn read(d: &[u8], o: usize, fmt: Fmt) -> Option<Num> {
    Some(match fmt {
        Fmt::U8 => Num::Int(d.get(o).copied()?.into()),
        Fmt::I8 => Num::Int(d.get(o).map(|b| *b as i8)?.into()),
        Fmt::U16 => Num::Int(u16::from_le_bytes(b2(d, o)?).into()),
        Fmt::I16 => Num::Int(i16::from_le_bytes(b2(d, o)?).into()),
        Fmt::U32 => Num::Int(u32::from_le_bytes(b4(d, o)?).into()),
        Fmt::I32 => Num::Int(i32::from_le_bytes(b4(d, o)?).into()),
        // A zero denominator reads as infinity, and 0/0 as nothing at all.
        Fmt::R32U => Num::Float(ratio(
            f64::from(u16::from_le_bytes(b2(d, o)?)),
            f64::from(u16::from_le_bytes(b2(d, o + 2)?)),
        )?),
        Fmt::R32S => Num::Float(ratio(
            f64::from(i16::from_le_bytes(b2(d, o)?)),
            f64::from(i16::from_le_bytes(b2(d, o + 2)?)),
        )?),
    })
}

/// `x as T` for the format's own type `T`: what a RawConv's answer became
/// when it was assigned back to the value the reader produced.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn cast_back(fmt: Fmt, x: f64) -> Num {
    match fmt {
        Fmt::U8 => Num::Int((x as u8).into()),
        Fmt::I8 => Num::Int((x as i8).into()),
        Fmt::U16 => Num::Int((x as u16).into()),
        Fmt::I16 => Num::Int((x as i16).into()),
        Fmt::U32 => Num::Int((x as u32).into()),
        Fmt::I32 => Num::Int((x as i32).into()),
        Fmt::R32U | Fmt::R32S => Num::Float(x),
    }
}

fn width(fmt: Fmt) -> usize {
    match fmt {
        Fmt::U8 | Fmt::I8 => 1,
        Fmt::U16 | Fmt::I16 => 2,
        Fmt::U32 | Fmt::I32 | Fmt::R32U | Fmt::R32S => 4,
    }
}

fn b2(d: &[u8], o: usize) -> Option<[u8; 2]> {
    Some([*d.get(o)?, *d.get(o + 1)?])
}

fn b4(d: &[u8], o: usize) -> Option<[u8; 4]> {
    Some([*d.get(o)?, *d.get(o + 1)?, *d.get(o + 2)?, *d.get(o + 3)?])
}

fn ratio(n: f64, d: f64) -> Option<f64> {
    if d == 0.0 {
        return if n == 0.0 { None } else { Some(f64::INFINITY) };
    }
    Some(n / d)
}

/// N bytes read as text. A `string` stops at its first NUL; an `undef` keeps
/// what is there. Both come back as the bytes they are, so a value that is not
/// ASCII survives to whatever decodes it later.
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

    static ALPHA: LazyLock<Regex> = LazyLock::new(|| Regex::new("^Alpha").expect("pattern"));

    /// A plain number stored under its own name.
    const fn num(name: &'static str, raw_conv: Option<&'static str>) -> Shape {
        Shape::Scalar {
            mask: 0,
            shift: 0,
            member: name,
            raw_conv,
            print: Print::Plain,
        }
    }

    static INNER: Table = Table {
        group2: "Camera",
        priority: 0,
        fields: &[Field {
            off: 1,
            name: "InnerByte",
            shape: num("InnerByte", None),
            ..F
        }],
        subdirs: &[],
    };

    static OUTER: Table = Table {
        group2: "Image",
        priority: 0,
        fields: &[
            Field {
                off: 0,
                name: "HighNibble",
                shape: Shape::Scalar {
                    mask: 0xf0,
                    shift: 4,
                    member: "HighNibble",
                    raw_conv: None,
                    print: Print::Map(NAMES),
                },
                ..F
            },
            Field {
                off: 1,
                name: "Stored",
                hidden: true,
                shape: Shape::Scalar {
                    mask: 0,
                    shift: 0,
                    member: "Member",
                    raw_conv: None,
                    print: Print::Plain,
                },
                ..F
            },
            Field {
                off: 2,
                name: "Pair",
                fmt: Fmt::U16,
                shape: Shape::Array {
                    count: 2,
                    print: ArrayPrint::Plain,
                },
                ..F
            },
            Field {
                off: 6,
                name: "Hex",
                shape: Shape::Array {
                    count: 2,
                    print: ArrayPrint::Hex,
                },
                ..F
            },
            Field {
                off: 8,
                name: "Word",
                group2: Some("Author"),
                shape: Shape::Text {
                    len: 4,
                    stop_at_nul: true,
                    member: Some("Word"),
                    value: None,
                    print: None,
                },
                ..F
            },
            Field {
                off: 12,
                name: "RuledOut",
                shape: num("RuledOut", Some("$val ? $val : undef")),
                ..F
            },
            Field {
                off: 13,
                name: "Conditioned",
                cond: Cond::Fn(member_is_seven),
                shape: num("Conditioned", None),
                ..F
            },
            Field {
                off: 14,
                name: "AlphaOnly",
                cond: Cond::Model(&ALPHA),
                shape: num("AlphaOnly", None),
                ..F
            },
            Field {
                off: 14,
                name: "NotAlpha",
                cond: Cond::NotModel(&ALPHA),
                shape: num("NotAlpha", Some("$val + 300")),
                ..F
            },
            Field {
                off: 15,
                name: "Shutter",
                shape: Shape::Array {
                    count: 2,
                    print: ArrayPrint::Keyed {
                        keys: &[("0 0", "Silent")],
                        other: None,
                    },
                },
                ..F
            },
        ],
        subdirs: &[SubDir {
            off: 16,
            len: 2,
            cond: Cond::Always,
            table: &INNER,
        }],
    };

    fn member_is_seven(_: &str, dm: &State) -> bool {
        dm.iter()
            .rev()
            .find(|(n, _)| n == "Member")
            .is_some_and(|(_, v)| v.as_num() == 7.0)
    }

    fn decode(data: &[u8], model: &str, dm: &mut State) -> Vec<(String, String, String)> {
        run(&OUTER, data, model, dm)
            .into_iter()
            .map(|t| (t.name, t.print_value, t.group.family2))
            .collect()
    }

    /// Each shape takes its own path: a mask and a map on a number, a hidden
    /// number that only stores itself, arrays joined, as hex and keyed, text
    /// cut at its NUL that stores itself, a RawConv that rules a value out,
    /// conditions on the state and on the model, and a sub-directory.
    #[test]
    fn every_shape_decodes() {
        let data = [
            0x35, 7, 1, 0, 2, 0, 0xab, 1, b'a', b'b', 0, b'c', 0, 42, 9, 0, 0, 0x55,
        ];
        let mut dm = State::new();
        let got = decode(&data, "Alpha 1", &mut dm);
        let want = [
            ("HighNibble", "Three", "Image"),
            ("Pair", "1 2", "Image"),
            ("Hex", "ab01", "Image"),
            ("Word", "ab", "Author"),
            ("Conditioned", "42", "Image"),
            ("AlphaOnly", "9", "Image"),
            ("Shutter", "Silent", "Image"),
            ("InnerByte", "85", "Camera"),
        ];
        let got: Vec<(&str, &str, &str)> = got
            .iter()
            .map(|(n, v, g)| (n.as_str(), v.as_str(), g.as_str()))
            .collect();
        assert_eq!(got, want);
        let names: Vec<&str> = dm.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "HighNibble",
                "Member",
                "Word",
                "RuledOut",
                "Conditioned",
                "AlphaOnly",
                "InnerByte"
            ]
        );
    }

    /// A value the map does not name prints as the number, a condition that
    /// does not hold leaves its field out, a RawConv's answer is cast back to
    /// the field's type (300 more than an int8u saturates at 255), an array
    /// the block cannot hold reports nothing, and an unnamed key goes through
    /// as the joined value.
    #[test]
    fn misses_and_edges() {
        let data = [
            0x25, 6, 1, 0, 2, 0, 0xab, 1, b'a', b'b', 0, b'c', 5, 42, 9, 3,
        ];
        let mut dm = State::new();
        let got = decode(&data, "Beta", &mut dm);
        let got: Vec<(&str, &str)> = got
            .iter()
            .map(|(n, v, _)| (n.as_str(), v.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("HighNibble", "2"),
                ("Pair", "1 2"),
                ("Hex", "ab01"),
                ("Word", "ab"),
                ("RuledOut", "5"),
                ("NotAlpha", "255"),
            ]
        );
    }
}
