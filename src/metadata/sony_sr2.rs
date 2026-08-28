//! The encrypted SR2SubIFD of Sony raw files.
//!
//! Sony hangs a private IFD off DNGPrivateData (0xc634) and, inside it, points
//! at a second directory that is encrypted with a key stored alongside. It holds
//! the black and white levels, the colour matrix and the white-balance tables --
//! nineteen tags on a current body, none of which were being read.
//!
//! The decryptor for it already existed in [`crate::metadata::sony_decrypt`],
//! unused: this is what it was written for.

use crate::metadata::sony_decrypt::sony_decrypt_words;
use crate::tag::{Tag, TagGroup, TagId, MAIN_DOCUMENT};
use crate::value::Value;

const SR2_SUBIFD_OFFSET: u16 = 0x7200;
const SR2_SUBIFD_LENGTH: u16 = 0x7201;
const SR2_SUBIFD_KEY: u16 = 0x7221;

include!("sr2_tags.rs");

fn u16_at(d: &[u8], o: usize, le: bool) -> Option<u16> {
    let b = [*d.get(o)?, *d.get(o + 1)?];
    Some(if le {
        u16::from_le_bytes(b)
    } else {
        u16::from_be_bytes(b)
    })
}

fn u32_at(d: &[u8], o: usize, le: bool) -> Option<u32> {
    let b = [*d.get(o)?, *d.get(o + 1)?, *d.get(o + 2)?, *d.get(o + 3)?];
    Some(if le {
        u32::from_le_bytes(b)
    } else {
        u32::from_be_bytes(b)
    })
}

fn mk(group1: &str, name: &str, print: String, raw: Value) -> Tag {
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: "MakerNotes".into(),
            family1: group1.into(),
            family2: "Camera".into(),
            family3: MAIN_DOCUMENT.into(),
        },
        raw_value: raw,
        print_value: print,
        priority: 0,
    }
}

/// Width in bytes of a TIFF data type, and whether it is signed.
fn type_info(t: u16) -> Option<(usize, bool)> {
    Some(match t {
        1 | 7 => (1, false),
        2 => (1, false),
        3 => (2, false),
        4 => (4, false),
        6 => (1, true),
        8 => (2, true),
        9 => (4, true),
        _ => return None,
    })
}

/// Read one IFD's worth of integer values, as ExifTool prints them: space
/// separated, in order.
fn read_values(
    block: &[u8],
    base: usize,
    dtype: u16,
    count: usize,
    voff: usize,
    le: bool,
) -> Option<String> {
    let (w, signed) = type_info(dtype)?;
    let total = w.checked_mul(count)?;
    // Values of four bytes or fewer sit in the entry itself.
    let start = if total <= 4 {
        voff
    } else {
        base.checked_add(0)? + voff.checked_sub(0)?
    };
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let o = start + i * w;
        let v: i64 = match (w, signed) {
            (1, false) => i64::from(*block.get(o)?),
            (1, true) => i64::from(*block.get(o)? as i8),
            (2, false) => i64::from(u16_at(block, o, le)?),
            (2, true) => i64::from(u16_at(block, o, le)? as i16),
            (4, false) => i64::from(u32_at(block, o, le)?),
            _ => i64::from(u32_at(block, o, le)? as i32),
        };
        out.push(v.to_string());
    }
    Some(out.join(" "))
}

/// Read the SR2 private directory at `private_offset`, and the encrypted
/// sub-directory it points at.
#[must_use]
pub fn read(data: &[u8], private_offset: usize, le: bool) -> Vec<Tag> {
    let mut tags = Vec::new();
    let Some(n) = u16_at(data, private_offset, le) else {
        return tags;
    };
    let (mut off, mut len, mut key) = (None, None, None);
    for i in 0..n as usize {
        let e = private_offset + 2 + i * 12;
        let Some(tag) = u16_at(data, e, le) else {
            break;
        };
        let Some(v) = u32_at(data, e + 8, le) else {
            break;
        };
        match tag {
            SR2_SUBIFD_OFFSET => {
                off = Some(v as usize);
                tags.push(mk("SR2", "SR2SubIFDOffset", v.to_string(), Value::U32(v)));
            }
            SR2_SUBIFD_LENGTH => {
                len = Some(v as usize);
                tags.push(mk("SR2", "SR2SubIFDLength", v.to_string(), Value::U32(v)));
            }
            SR2_SUBIFD_KEY => {
                key = Some(v);
                tags.push(mk(
                    "SR2",
                    "SR2SubIFDKey",
                    format!("0x{v:08x}"),
                    Value::U32(v),
                ));
            }
            _ => {}
        }
    }

    let (Some(off), Some(len), Some(key)) = (off, len, key) else {
        return tags;
    };
    if len == 0 || off.checked_add(len).is_none_or(|e| e > data.len()) {
        return tags;
    }

    let mut block = data[off..off + len].to_vec();
    sony_decrypt_words(&mut block, 0, key);

    // Offsets inside the directory are file-absolute, so they index the block
    // once the directory's own position is taken off.
    let Some(count) = u16_at(&block, 0, le) else {
        return tags;
    };
    for i in 0..count as usize {
        let e = 2 + i * 12;
        let (Some(tag), Some(dtype), Some(cnt)) = (
            u16_at(&block, e, le),
            u16_at(&block, e + 2, le),
            u32_at(&block, e + 4, le),
        ) else {
            break;
        };
        // 0x74c0 is not a value but a list of offsets, one per data
        // directory: an A700 has fourteen of them, this ILCE-9 thirteen. They
        // point inside the same decrypted block, and each holds one tag --
        // `%Sony::SR2DataIFD` defines 0x7770 ColorMode and nothing else -- in
        // a family-1 group named after its position.
        if tag == 0x74c0 {
            let cnt = cnt as usize;
            let list = match u32_at(&block, e + 8, le) {
                Some(a) if (a as usize) >= off => (a as usize) - off,
                _ => continue,
            };
            for k in 0..cnt.min(20) {
                let Some(target) = u32_at(&block, list + k * 4, le) else {
                    break;
                };
                let Some(dir) = (target as usize).checked_sub(off) else {
                    continue;
                };
                let Some(entries) = u16_at(&block, dir, le) else {
                    continue;
                };
                for j in 0..entries as usize {
                    let de = dir + 2 + j * 12;
                    let (Some(dtag), Some(dtype), Some(dcnt)) = (
                        u16_at(&block, de, le),
                        u16_at(&block, de + 2, le),
                        u32_at(&block, de + 4, le),
                    ) else {
                        break;
                    };
                    if dtag != 0x7770 {
                        continue;
                    }
                    // A string of `dcnt` bytes, in the entry when it fits.
                    let n = dcnt as usize;
                    let start = if n <= 4 {
                        de + 8
                    } else {
                        match u32_at(&block, de + 8, le) {
                            Some(a) if (a as usize) >= off => (a as usize) - off,
                            _ => continue,
                        }
                    };
                    if dtype != 2 {
                        continue;
                    }
                    let Some(raw) = block.get(start..start + n) else {
                        continue;
                    };
                    let text: String = raw
                        .iter()
                        .take_while(|c| **c != 0)
                        .map(|c| *c as char)
                        .collect();
                    let group = if k == 0 {
                        "SR2DataIFD".to_string()
                    } else {
                        format!("SR2DataIFD{k}")
                    };
                    let mut t = mk(&group, "ColorMode", text.clone(), Value::String(text));
                    // `Priority => 0` (Sony.pm): these never displace the
                    // ColorMode the maker note itself reported.
                    t.priority = crate::tag::PRIORITY_EXPLICIT_ZERO;
                    tags.push(t);
                }
            }
            continue;
        }
        let Some(name) = SR2_SUB_IFD_TAGS
            .iter()
            .find(|(t, _)| *t == tag)
            .map(|(_, n)| *n)
        else {
            continue;
        };
        let cnt = cnt as usize;
        let Some((w, _)) = type_info(dtype) else {
            continue;
        };
        let total = w.saturating_mul(cnt);
        let voff = if total <= 4 {
            e + 8
        } else {
            match u32_at(&block, e + 8, le) {
                Some(a) if (a as usize) >= off => (a as usize) - off,
                _ => continue,
            }
        };
        if let Some(s) = read_values(&block, 0, dtype, cnt, voff, le) {
            tags.push(mk("SR2SubIFD", name, s.clone(), Value::String(s)));
        }
    }
    tags
}
