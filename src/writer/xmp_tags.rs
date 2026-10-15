//! Which XMP property a write request goes to, and how its value is
//! formatted, following ExifTool's `SetNewValue` (Writer.pl) and `CheckXMP`
//! (WriteXMP.pl) over the generated table of writable XMP properties.
//!
//! - `XMP-<ns>:Tag` writes the tag of that namespace.
//! - `XMP:Tag` (or an ungrouped tag routed to XMP) is created in every
//!   namespace defining the tag that is not marked `Avoid` (in all of them
//!   when each one is), and replaced where the packet already has it in
//!   the other namespaces: `XMP:Description` creates `dc:description` and
//!   updates an existing `xmp:Description`.

use crate::tags::xmp_write_generated::{XmpConv, XmpKind, XmpType, XmpWriteInfo, XMP_WRITE_TAGS};
use crate::writer::xmp_edit::{XmpSet, XmpValueKind};

/// The properties a request writes.
#[derive(Debug, Default)]
pub struct XmpTargets {
    /// Created (or replaced) properties.
    pub create: Vec<XmpSet>,
    /// Replaced only where they exist.
    pub edit_only: Vec<XmpSet>,
}

/// The writable XMP tags named `tag`, in the family-1 group `XMP-<prefix>`
/// when `prefix` is given.
fn candidates(prefix: Option<&str>, tag: &str) -> Vec<&'static XmpWriteInfo> {
    XMP_WRITE_TAGS
        .iter()
        .filter(|t| {
            (t.name.eq_ignore_ascii_case(tag) || t.property.eq_ignore_ascii_case(tag))
                && prefix.is_none_or(|p| {
                    t.group1
                        .strip_prefix("XMP-")
                        .is_some_and(|g| g.eq_ignore_ascii_case(p))
                        || t.prefix.eq_ignore_ascii_case(p)
                })
        })
        .collect()
}

/// Is `tag` a writable XMP tag (in any namespace)?
pub fn is_known(tag: &str) -> bool {
    !candidates(None, tag).is_empty()
}

/// Resolve a request for `tag` = `value`. `None` when no namespace can
/// take it (unknown tag, a conversion that has not been ported, or a value
/// ExifTool would reject).
pub fn resolve(prefix: Option<&str>, tag: &str, value: &str) -> Option<XmpTargets> {
    let rows: Vec<_> = candidates(prefix, tag)
        .into_iter()
        .filter(|t| t.conv == XmpConv::None)
        .collect();
    if rows.is_empty() {
        return None;
    }
    let any_preferred = rows.iter().any(|t| !t.avoid);
    let mut out = XmpTargets::default();
    for t in rows {
        let set = make_set(t, value)?;
        if prefix.is_some() || !t.avoid || !any_preferred {
            out.create.push(set);
        } else {
            out.edit_only.push(set);
        }
    }
    Some(out)
}

fn make_set(t: &XmpWriteInfo, value: &str) -> Option<XmpSet> {
    Some(XmpSet {
        uri: t.uri.to_string(),
        prefix: t.prefix.to_string(),
        property: t.property.to_string(),
        values: vec![convert(t, value)?],
        kind: match t.kind {
            XmpKind::Simple => XmpValueKind::Simple,
            XmpKind::LangAlt => XmpValueKind::LangAlt,
            XmpKind::Bag => XmpValueKind::Bag,
            XmpKind::Seq => XmpValueKind::Seq,
            XmpKind::Alt => XmpValueKind::Alt,
        },
    })
}

/// The inverse print conversion and `CheckXMP` formatting of a value.
fn convert(t: &XmpWriteInfo, value: &str) -> Option<String> {
    let mut v = value.to_string();
    if !t.print_inv.is_empty() {
        if let Some((_, raw)) = t
            .print_inv
            .iter()
            .find(|(p, _)| p.eq_ignore_ascii_case(value))
        {
            v = raw.to_string();
        } else if t.print_strict && !t.print_inv.iter().any(|(_, raw)| *raw == value) {
            return None;
        }
    }
    match t.writable {
        XmpType::String => {
            // Characters XML can't hold become '.'.
            Some(
                v.chars()
                    .map(|c| {
                        if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') {
                            '.'
                        } else {
                            c
                        }
                    })
                    .collect(),
            )
        }
        XmpType::Integer => {
            if is_int(&v) {
                Some(v)
            } else {
                let h = v
                    .strip_prefix("0x")
                    .or_else(|| v.strip_prefix("0X"))
                    .unwrap_or(&v);
                if (1..=8).contains(&h.len()) {
                    u64::from_str_radix(h, 16).ok().map(|n| n.to_string())
                } else {
                    None
                }
            }
        }
        XmpType::Real => is_float(&v).then_some(v.replace(',', ".")),
        XmpType::Rational => rational(&v),
        XmpType::Date => format_xmp_date(&v),
        XmpType::Boolean => {
            let lc = v.to_ascii_lowercase();
            Some(
                if v.is_empty() || lc.contains("false") || lc == "no" || lc == "0" {
                    "False"
                } else {
                    "True"
                }
                .to_string(),
            )
        }
    }
}

fn is_int(v: &str) -> bool {
    let d = v.strip_prefix(['+', '-']).unwrap_or(v);
    !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())
}

/// ExifTool's `IsFloat` (a comma also serves as the decimal point).
fn is_float(v: &str) -> bool {
    let v = v.replace(',', ".");
    let s = v.strip_prefix(['+', '-']).unwrap_or(&v);
    let (mant, exp) = match s.find(['e', 'E']) {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    let (int, frac) = mant.split_once('.').unwrap_or((mant, ""));
    let digits = |x: &str| x.bytes().all(|b| b.is_ascii_digit());
    let mant_ok = digits(int)
        && digits(frac)
        && (!int.is_empty() || !frac.is_empty())
        && !(int.is_empty() && mant.starts_with('.') && frac.is_empty());
    let exp_ok = exp.is_none_or(|e| {
        let e = e.strip_prefix(['+', '-']).unwrap_or(e);
        !e.is_empty() && digits(e)
    });
    mant_ok && exp_ok
}

/// A rational as CheckXMP writes it: `n/d`, or a float rationalized.
fn rational(v: &str) -> Option<String> {
    if v == "inf" {
        return Some("1/0".into());
    }
    if v == "undef" {
        return Some("0/0".into());
    }
    if let Some((n, d)) = v.split_once('/') {
        if is_int(n) && !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()) {
            return Some(v.to_string());
        }
        return None;
    }
    if !is_float(v) {
        return None;
    }
    let f: f64 = v.replace(',', ".").parse().ok()?;
    let (n, d) = crate::writer::exif_edit::rationalize_unsigned(f);
    Some(if f < 0.0 {
        format!("-{n}/{d}")
    } else {
        format!("{n}/{d}")
    })
}

/// ExifTool's `FormatXMPDate`: `YYYY:mm:dd HH:MM[:SS[.ss]][zone]` (or the
/// ISO form) → `YYYY-mm-ddTHH:MM:SS[zone]`; a date or a time alone too.
pub fn format_xmp_date(val: &str) -> Option<String> {
    let b = val.as_bytes();
    let digits =
        |r: std::ops::Range<usize>| r.end <= b.len() && b[r].iter().all(u8::is_ascii_digit);
    // Date and time.
    for i in 0..b.len() {
        if !digits(i..i + 4) || i + 16 > b.len() {
            continue;
        }
        let (s1, s2, s3) = (b[i + 4], b[i + 7], b[i + 10]);
        let ok = digits(i + 5..i + 7)
            && digits(i + 8..i + 10)
            && ((s1 == b':' && s2 == b':' && s3 == b' ')
                || (s1 == b'-' && s2 == b'-' && s3 == b'T'));
        if !ok {
            continue;
        }
        let (t, rest) = time_part(&val[i + 11..])?;
        let mut out = format!(
            "{}-{}-{}T{t}",
            &val[i..i + 4],
            &val[i + 5..i + 7],
            &val[i + 8..i + 10]
        );
        out.push_str(&zone(rest)?);
        return Some(out);
    }
    let trimmed = val.trim();
    // A date alone: YYYY, YYYY:mm or YYYY:mm:dd.
    let tb = trimmed.as_bytes();
    let date_only = matches!(tb.len(), 4 | 7 | 10)
        && tb.iter().enumerate().all(|(k, c)| {
            if k == 4 || k == 7 {
                *c == b':'
            } else {
                c.is_ascii_digit()
            }
        });
    if date_only {
        return Some(trimmed.replace(':', "-"));
    }
    // A time alone.
    let (t, rest) = time_part(trimmed)?;
    let mut out = t;
    out.push_str(&zone(rest.trim_end())?);
    Some(out)
}

/// `HH:MM[:SS[.ss]]` at the start of `s`, and what follows.
fn time_part(s: &str) -> Option<(String, &str)> {
    let b = s.as_bytes();
    let d2 = |i: usize| i + 2 <= b.len() && b[i..i + 2].iter().all(u8::is_ascii_digit);
    if !(d2(0) && b.get(2) == Some(&b':') && d2(3)) {
        return None;
    }
    let mut end = 5;
    if b.get(5) == Some(&b':') && d2(6) {
        end = 8;
        if b.get(8) == Some(&b'.') {
            end = 9;
            while end < b.len() && b[end].is_ascii_digit() {
                end += 1;
            }
        }
    }
    Some((s[..end].to_string(), &s[end..]))
}

/// The time zone after a time: empty, `Z` or `+-HH:MM`.
fn zone(rest: &str) -> Option<String> {
    if rest.is_empty() {
        return Some(String::new());
    }
    let b = rest.as_bytes();
    let ok = rest == "Z"
        || (b.len() == 6
            && matches!(b[0], b'+' | b'-')
            && b[1..3].iter().all(u8::is_ascii_digit)
            && b[3] == b':'
            && b[4..6].iter().all(u8::is_ascii_digit));
    ok.then(|| rest.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn family0_description_prefers_dublin_core() {
        // XMP-xmp, XMP-crs and XMP-crd define Description with `Avoid`.
        let t = resolve(None, "Description", "x").unwrap();
        assert_eq!(t.create.len(), 1);
        assert_eq!(t.create[0].prefix, "dc");
        assert_eq!(t.create[0].property, "description");
        assert_eq!(t.create[0].kind, XmpValueKind::LangAlt);
        assert!(t.edit_only.iter().any(|s| s.prefix == "xmp"));
        // XMP:Title also updates an existing pdf:Title.
        let t = resolve(None, "Title", "x").unwrap();
        assert_eq!(t.create[0].prefix, "dc");
        assert!(t.edit_only.iter().any(|s| s.prefix == "pdf"));
    }

    #[test]
    fn family1_group_names_its_namespace() {
        let t = resolve(Some("xmp"), "Description", "x").unwrap();
        assert_eq!(t.create.len(), 1);
        assert_eq!(t.create[0].prefix, "xmp");
        let t = resolve(Some("iptcCore"), "Location", "x").unwrap();
        assert_eq!(t.create[0].prefix, "Iptc4xmpCore");
        assert!(resolve(Some("dc"), "NoSuchTag", "x").is_none());
    }

    #[test]
    fn values_are_formatted_like_checkxmp() {
        assert_eq!(
            format_xmp_date("2024:01:31 12:34:56").unwrap(),
            "2024-01-31T12:34:56"
        );
        assert_eq!(
            format_xmp_date("2024:01:31 12:34:56+02:00").unwrap(),
            "2024-01-31T12:34:56+02:00"
        );
        assert_eq!(format_xmp_date("2024:01").unwrap(), "2024-01");
        assert!(format_xmp_date("garbage").is_none());
        let rating = resolve(Some("xmp"), "Rating", "5").unwrap();
        assert_eq!(rating.create[0].values, ["5"]);
        assert!(resolve(Some("xmp"), "Rating", "five").is_none());
        let fnum = resolve(Some("exif"), "FNumber", "2.8");
        // FNumber has a Perl conversion: not written rather than written wrong.
        if let Some(f) = fnum {
            assert_eq!(f.create[0].values, ["14/5"]);
        }
    }
}
