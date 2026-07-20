//! Lecteur du format Garmin FIT (Flexible and Interoperable Data Transfer).
//!
//! Porté de `Image::ExifTool::Garmin::ProcessFIT` (ExifTool 13.59). Les tables
//! de tags vivent dans [`super::fit_tables`].
//!
//! Comme ExifTool sans l'option `ExtractEmbedded`, seul le PREMIER message de
//! chaque type global est extrait, et un avertissement `[minor]` est émis.

use super::fit_tables as tbl;
use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::{format_g15, Value};

// ────────────────────────────── API des tables ──────────────────────────────

/// Une entrée de table de champ FIT : numéro de champ, nom, ValueConv, PrintConv.
pub(crate) struct Field {
    pub num: u16,
    pub name: &'static str,
    pub conv: Conv,
    pub print: Print,
}

/// Conversion de valeur (`ValueConv` côté Perl).
pub(crate) enum Conv {
    /// Valeur brute.
    None,
    /// Secondes depuis le 31/12/1989 00:00:00 UTC → epoch Unix.
    FitTime,
    /// Semicercles → degrés (`$val * 180 / 0x80000000`).
    Semicircles,
    /// `$val / d`
    Div(f64),
    /// `$val * m`
    Mul(f64),
    /// `$val / d - s`
    DivSub(f64, f64),
    /// Divise chaque élément d'un tableau par `d`.
    DivEach(f64),
}

/// Conversion d'affichage (`PrintConv` côté Perl).
pub(crate) enum Print {
    /// Aucune.
    None,
    /// Table de correspondance valeur → libellé.
    Enum(&'static [(i64, &'static str)]),
    /// Date/heure locale à partir d'un epoch Unix.
    DateTime,
    /// Degrés/minutes/secondes ; `true` pour une latitude, `false` pour une longitude.
    Dms(bool),
    /// Ajoute une unité au texte de la valeur.
    Unit(&'static str),
}

// ─────────────────────────────── types de base ──────────────────────────────

/// Famille de type de base FIT, telle que lue dans un message de définition.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    U8,
    I8,
    U16,
    I16,
    U32,
    I32,
    U64,
    I64,
    F32,
    F64,
    Str,
    Bin,
}

impl Kind {
    fn size(self) -> usize {
        match self {
            Kind::U8 | Kind::I8 | Kind::Str | Kind::Bin => 1,
            Kind::U16 | Kind::I16 => 2,
            Kind::U32 | Kind::I32 | Kind::F32 => 4,
            Kind::U64 | Kind::I64 | Kind::F64 => 8,
        }
    }
}

/// Sentinelle « valeur invalide » d'un type de base FIT.
enum Invalid {
    /// La valeur entière indiquée marque l'absence de donnée.
    Int(i64),
    /// NaN (float32/float64).
    Nan,
    /// Chaîne vide.
    Empty,
    /// Aucune sentinelle exploitable (type `byte`, comparé à du binaire côté Perl).
    Never,
}

/// Table `%baseType` de Garmin.pm : type → (famille, sentinelle invalide).
fn base_type(t: u8) -> Option<(Kind, Invalid)> {
    Some(match t {
        0x00 => (Kind::U8, Invalid::Int(0xff)),             // enum
        0x01 => (Kind::I8, Invalid::Int(0x7f)),             // sint8
        0x02 => (Kind::U8, Invalid::Int(0xff)),             // uint8
        0x83 => (Kind::I16, Invalid::Int(0x7fff)),          // sint16
        0x84 => (Kind::U16, Invalid::Int(0xffff)),          // uint16
        0x85 => (Kind::I32, Invalid::Int(0x7fff_ffff)),     // sint32
        0x86 => (Kind::U32, Invalid::Int(0xffff_ffff)),     // uint32
        0x07 => (Kind::Str, Invalid::Empty),                // string
        0x88 => (Kind::F32, Invalid::Nan),                  // float32
        0x89 => (Kind::F64, Invalid::Nan),                  // float64
        0x0a => (Kind::U8, Invalid::Int(0)),                // uint8z
        0x8b => (Kind::U16, Invalid::Int(0)),               // uint16z
        0x8c => (Kind::U32, Invalid::Int(0)),               // uint32z
        0x0d => (Kind::Bin, Invalid::Never),                // byte
        0x8e => (Kind::I64, Invalid::Int(i64::MAX)),        // sint64
        0x8f => (Kind::U64, Invalid::Int(-1)),              // uint64 (0xffff_ffff_ffff_ffff)
        0x90 => (Kind::U64, Invalid::Int(0)),               // uint64z
        _ => return None,
    })
}

/// Un élément lu : entier ou flottant, pour préserver l'affichage entier exact.
#[derive(Clone, Copy)]
enum Num {
    I(i64),
    F(f64),
}

impl Num {
    fn as_f64(self) -> f64 {
        match self {
            Num::I(i) => i as f64,
            Num::F(f) => f,
        }
    }

    fn to_text(self) -> String {
        match self {
            Num::I(i) => i.to_string(),
            Num::F(f) => format_g15(f),
        }
    }
}

fn read_num(kind: Kind, buf: &[u8], be: bool) -> Num {
    macro_rules! int {
        ($ty:ty, $n:expr) => {{
            let mut a = [0u8; $n];
            a.copy_from_slice(&buf[..$n]);
            let v = if be {
                <$ty>::from_be_bytes(a)
            } else {
                <$ty>::from_le_bytes(a)
            };
            v as i64
        }};
    }
    match kind {
        Kind::U8 | Kind::Bin | Kind::Str => Num::I(buf[0] as i64),
        Kind::I8 => Num::I(buf[0] as i8 as i64),
        Kind::U16 => Num::I(int!(u16, 2)),
        Kind::I16 => Num::I(int!(i16, 2)),
        Kind::U32 => Num::I(int!(u32, 4)),
        Kind::I32 => Num::I(int!(i32, 4)),
        Kind::I64 => Num::I(int!(i64, 8)),
        // uint64 : conservé en i64 (bit-à-bit) comme le fait la sentinelle -1
        Kind::U64 => Num::I(int!(u64, 8)),
        Kind::F32 => {
            let mut a = [0u8; 4];
            a.copy_from_slice(&buf[..4]);
            Num::F(if be {
                f32::from_be_bytes(a)
            } else {
                f32::from_le_bytes(a)
            } as f64)
        }
        Kind::F64 => {
            let mut a = [0u8; 8];
            a.copy_from_slice(&buf[..8]);
            Num::F(if be {
                f64::from_be_bytes(a)
            } else {
                f64::from_le_bytes(a)
            })
        }
    }
}

// ────────────────────────────────── état ────────────────────────────────────

/// Définition d'un champ à l'intérieur d'un message.
struct FieldDef {
    num: u16,
    size: usize,
    base: u8,
    dev: bool,
}

/// Définition d'un message local (renouvelée à chaque message de définition).
struct MsgDef {
    big_endian: bool,
    global_num: u16,
    name: &'static str,
    size: usize,
    fields: Vec<FieldDef>,
    /// Emplacement du champ 253 (TimeStamp) : (taille, type, offset).
    ts_field: Option<(usize, u8, usize)>,
    /// Horodatage issu d'un en-tête compressé.
    ts_value: Option<i64>,
}

fn message_name(num: u16) -> Option<&'static str> {
    tbl::MESSAGE_NAMES
        .iter()
        .find(|(n, _)| *n == num)
        .map(|(_, s)| *s)
}

/// Table de champs extraite par défaut pour un message donné (les autres sont
/// marqués `Unknown => 1` côté Perl et ne produisent aucun tag).
fn fields_for(name: &str) -> Option<&'static [Field]> {
    match name {
        "Session" => Some(tbl::SESSION_FIELDS),
        "Lap" => Some(tbl::LAP_FIELDS),
        "Record" => Some(tbl::RECORD_FIELDS),
        "GPS" => Some(tbl::GPS_FIELDS),
        _ => None,
    }
}

fn lookup_field(table: &'static [Field], num: u16) -> Option<&'static Field> {
    table
        .iter()
        .find(|f| f.num == num)
        .or_else(|| tbl::COMMON_FIELDS.iter().find(|f| f.num == num))
}

// ──────────────────────────── conversions/affichage ─────────────────────────

/// Décalage entre l'epoch FIT (31/12/1989 00:00:00 UTC) et l'epoch Unix.
const FIT_EPOCH_OFFSET: i64 = 631_065_600;

fn apply_conv(conv: &Conv, vals: &[Num]) -> Vec<Num> {
    vals.iter()
        .map(|&v| match conv {
            Conv::None => v,
            Conv::FitTime => Num::I(match v {
                Num::I(i) => i + FIT_EPOCH_OFFSET,
                Num::F(f) => f as i64 + FIT_EPOCH_OFFSET,
            }),
            Conv::Semicircles => Num::F(v.as_f64() * 180.0 / 2147483648.0),
            Conv::Div(d) | Conv::DivEach(d) => Num::F(v.as_f64() / d),
            Conv::Mul(m) => Num::F(v.as_f64() * m),
            Conv::DivSub(d, s) => Num::F(v.as_f64() / d - s),
        })
        .collect()
}

/// `Image::ExifTool::GPS::ToDMS($self, $val, 1, "N"|"E")`.
fn to_dms(deg: f64, is_lat: bool) -> String {
    let refc = if is_lat {
        if deg < 0.0 {
            'S'
        } else {
            'N'
        }
    } else if deg < 0.0 {
        'W'
    } else {
        'E'
    };
    let a = deg.abs();
    let d = a.floor();
    let rem = (a - d) * 60.0;
    let m = rem.floor();
    let s = (rem - m) * 60.0;
    format!("{} deg {}' {:.2}\" {}", d as i64, m as i64, s, refc)
}

fn value_text(vals: &[Num]) -> String {
    vals.iter()
        .map(|v| v.to_text())
        .collect::<Vec<_>>()
        .join(" ")
}

fn print_text(print: &Print, vals: &[Num], raw: &str) -> String {
    match print {
        Print::None => raw.to_string(),
        Print::Unit(u) => format!("{raw} {u}"),
        Print::Enum(table) => {
            if vals.len() != 1 {
                return raw.to_string();
            }
            let key = match vals[0] {
                Num::I(i) => i,
                Num::F(f) => f as i64,
            };
            table
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, s)| s.to_string())
                .unwrap_or_else(|| format!("Unknown ({key})"))
        }
        Print::DateTime => {
            if vals.len() != 1 {
                return raw.to_string();
            }
            let secs = match vals[0] {
                Num::I(i) => i,
                Num::F(f) => f as i64,
            };
            crate::formats::gzip::gzip_unix_to_datetime(secs)
        }
        Print::Dms(is_lat) => {
            if vals.len() != 1 {
                return raw.to_string();
            }
            to_dms(vals[0].as_f64(), *is_lat)
        }
    }
}

fn mk_tag(group1: &str, name: &str, raw: Value, print: String) -> Tag {
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: "Garmin".into(),
            family1: group1.into(),
            family2: "Other".into(),
        },
        raw_value: raw,
        print_value: print,
        priority: 0,
    }
}

// ─────────────────────────────────── parseur ────────────────────────────────

/// Lit un fichier Garmin FIT et renvoie ses tags.
pub fn read_fit(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 12 || &data[8..12] != b".FIT" {
        return Err(Error::InvalidData("not a Garmin FIT file".into()));
    }

    let hdr_len = data[0] as usize;
    let data_len = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
    let end = (hdr_len + data_len).min(data.len());

    let mut tags = Vec::new();

    // ProtocolVersion, puis l'avertissement d'ExifTool (`$ee or $et->Warn(...)`).
    tags.push(Tag {
        id: TagId::Text("ProtocolVersion".into()),
        name: "ProtocolVersion".into(),
        description: "Protocol Version".into(),
        group: TagGroup {
            family0: "File".into(),
            family1: "File".into(),
            family2: "Other".into(),
        },
        raw_value: Value::U8(data[1]),
        print_value: data[1].to_string(),
        priority: 0,
    });
    let warn = "[minor] Use ExtractEmbedded option to extract all timed metadata";
    tags.push(Tag {
        id: TagId::Text("Warning".into()),
        name: "Warning".into(),
        description: "Warning".into(),
        group: TagGroup {
            family0: "ExifTool".into(),
            family1: "ExifTool".into(),
            family2: "Other".into(),
        },
        raw_value: Value::String(warn.into()),
        print_value: warn.into(),
        priority: 0,
    });

    // 16 définitions de messages locaux au plus (4 bits d'identifiant).
    let mut defs: Vec<Option<MsgDef>> = (0..16).map(|_| None).collect();
    let mut done: Vec<u16> = Vec::new();
    let mut timestamp: i64 = 0;
    let mut pos = hdr_len;

    while pos < end {
        let flags = data[pos];
        pos += 1;
        let local: usize;

        if flags & 0x80 != 0 {
            // En-tête compressé : l'horodatage courant est mis à jour par un
            // offset de 5 bits avec rebouclage (cf. commentaire de Garmin.pm).
            local = ((flags >> 5) & 0x03) as usize;
            let offset = (flags & 0x1f) as i64;
            if offset != 0 {
                let low = timestamp & 0x1f;
                let mut ts = (timestamp & !0x1f) + offset;
                if offset < low {
                    ts += 0x20;
                }
                if let Some(d) = defs[local].as_mut() {
                    d.ts_value = Some(ts);
                }
            }
        } else {
            local = (flags & 0x0f) as usize;
            if flags & 0x40 != 0 {
                // Message de définition.
                if pos + 5 > end {
                    break;
                }
                let big_endian = data[pos + 1] != 0;
                let gnum = if big_endian {
                    u16::from_be_bytes([data[pos + 2], data[pos + 3]])
                } else {
                    u16::from_le_bytes([data[pos + 2], data[pos + 3]])
                };
                let n_fields = data[pos + 4] as usize;
                pos += 5;
                if pos + n_fields * 3 > end {
                    break;
                }
                let name = message_name(gnum).unwrap_or("Unknown");
                let extract = fields_for(name).is_some();
                let mut fields = Vec::new();
                let mut ts_field = None;
                let mut total = 0usize;
                for i in 0..n_fields {
                    let f = &data[pos + i * 3..pos + i * 3 + 3];
                    let (num, size, base) = (f[0] as u16, f[1] as usize, f[2]);
                    if base_type(base).is_some() {
                        if num == 253 && ts_field.is_none() {
                            ts_field = Some((size, base, total));
                        }
                        if extract {
                            fields.push(FieldDef {
                                num,
                                size,
                                base,
                                dev: false,
                            });
                        }
                    }
                    total += size;
                }
                pos += n_fields * 3;

                if flags & 0x20 != 0 {
                    // Définitions de champs développeur.
                    if pos >= end {
                        break;
                    }
                    let n_dev = data[pos] as usize;
                    pos += 1;
                    if pos + n_dev * 3 > end {
                        break;
                    }
                    for i in 0..n_dev {
                        let f = &data[pos + i * 3..pos + i * 3 + 3];
                        if extract {
                            fields.push(FieldDef {
                                num: f[0] as u16,
                                size: f[1] as usize,
                                base: f[2],
                                dev: true,
                            });
                        }
                        total += f[1] as usize;
                    }
                    pos += n_dev * 3;
                }

                defs[local] = Some(MsgDef {
                    big_endian,
                    global_num: gnum,
                    name,
                    size: total,
                    fields,
                    ts_field,
                    ts_value: None,
                });
                continue;
            }
        }

        // Message de données.
        let Some(def) = defs[local].as_ref() else {
            break;
        };
        if pos + def.size > end {
            break;
        }
        // Sans ExtractEmbedded, un seul message par type global.
        if done.contains(&def.global_num) {
            pos += def.size;
            continue;
        }
        done.push(def.global_num);
        let body = &data[pos..pos + def.size];
        pos += def.size;
        let be = def.big_endian;

        // Horodatage courant (champ 253 ou en-tête compressé).
        let ts_val = match (def.ts_field, def.ts_value) {
            (Some((size, base, off)), _) => base_type(base).and_then(|(kind, _)| {
                if off + kind.size() <= body.len() && size >= kind.size() {
                    match read_num(kind, &body[off..], be) {
                        Num::I(i) => Some((i, true)),
                        Num::F(f) => Some((f as i64, true)),
                    }
                } else {
                    None
                }
            }),
            (None, Some(v)) => Some((v, false)),
            _ => None,
        };
        let extract = fields_for(def.name).is_some();
        if let Some((val, from_field)) = ts_val {
            if timestamp != val {
                timestamp = val;
                // Perl n'émet TimeStamp ici que si le message n'a pas de table
                // de champs, ou si l'horodatage vient d'un en-tête compressé.
                if !(extract && from_field) {
                    let secs = val + FIT_EPOCH_OFFSET;
                    tags.push(mk_tag(
                        def.name,
                        "TimeStamp",
                        Value::I32(val as i32),
                        crate::formats::gzip::gzip_unix_to_datetime(secs),
                    ));
                }
            }
        }

        let Some(table) = fields_for(def.name) else {
            continue;
        };

        let mut off = 0usize;
        for fd in &def.fields {
            let start = off;
            off += fd.size;
            if fd.dev {
                // Les champs développeur nécessitent DeveloperDataID/FieldDescription ;
                // non portés (aucun tag par défaut dans le corpus de référence).
                continue;
            }
            let Some((kind, invalid)) = base_type(fd.base) else {
                continue;
            };
            let Some(field) = lookup_field(table, fd.num) else {
                continue;
            };
            if start + fd.size > body.len() {
                continue;
            }
            let chunk = &body[start..start + fd.size];

            // Type `byte` : donnée binaire opaque.
            if kind == Kind::Bin {
                let text = format!("(Binary data {} bytes)", chunk.len());
                tags.push(mk_tag(
                    def.name,
                    field.name,
                    Value::Binary(chunk.to_vec()),
                    text,
                ));
                continue;
            }

            // Type `string` : octets jusqu'au premier NUL.
            if kind == Kind::Str {
                let cut = chunk.iter().position(|&b| b == 0).unwrap_or(chunk.len());
                let s = crate::encoding::decode_utf8_or_latin1(&chunk[..cut]).to_string();
                if s.is_empty() {
                    continue;
                }
                let print = print_text(&field.print, &[], &s);
                tags.push(mk_tag(def.name, field.name, Value::String(s), print));
                continue;
            }

            let esz = kind.size();
            if esz == 0 || fd.size % esz != 0 {
                continue;
            }
            let vals: Vec<Num> = (0..fd.size / esz)
                .map(|i| read_num(kind, &chunk[i * esz..], be))
                .collect();
            if vals.is_empty() {
                continue;
            }
            // Perl compare la valeur *entière* (éléments joints) à la sentinelle,
            // donc seul un scalaire peut être invalidé.
            if vals.len() == 1 {
                let skip = match (&invalid, vals[0]) {
                    (Invalid::Int(s), Num::I(i)) => i == *s,
                    (Invalid::Nan, Num::F(f)) => f.is_nan(),
                    (Invalid::Empty, _) => false,
                    _ => false,
                };
                if skip {
                    continue;
                }
            }

            let conv = apply_conv(&field.conv, &vals);
            let raw = value_text(&conv);
            let print = print_text(&field.print, &conv, &raw);
            tags.push(mk_tag(
                def.name,
                field.name,
                Value::String(raw),
                print,
            ));
        }
    }

    // ExifTool conserve la DERNIÈRE occurrence d'un même nom de tag ; le moteur
    // de ce crate garde la première. On dédoublonne donc ici en gardant la
    // dernière valeur rencontrée, à la position de la première occurrence.
    let mut seen: Vec<String> = Vec::new();
    let mut out: Vec<Option<Tag>> = Vec::new();
    for tag in tags {
        if let Some(i) = seen.iter().position(|n| *n == tag.name) {
            out[i] = Some(tag);
        } else {
            seen.push(tag.name.clone());
            out.push(Some(tag));
        }
    }
    Ok(out.into_iter().flatten().collect())
}
