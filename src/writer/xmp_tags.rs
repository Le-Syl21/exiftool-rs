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
use crate::value::format_g15;
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
        .filter(|t| t.conv != XmpConv::Unported)
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
        values: convert(t, value)?,
        kind: match t.kind {
            XmpKind::Simple => XmpValueKind::Simple,
            XmpKind::LangAlt => XmpValueKind::LangAlt,
            XmpKind::Bag => XmpValueKind::Bag,
            XmpKind::Seq => XmpValueKind::Seq,
            XmpKind::Alt => XmpValueKind::Alt,
        },
    })
}

/// A value through the tag's inverse conversions (`ConvInv` in Writer.pl:
/// `PrintConvInv`, then `ValueConvInv`) and `CheckXMP`'s formatting. Several
/// values for a list the conversion splits (`exifEX:LensInfo`); `None` when
/// ExifTool rejects the value.
fn convert(t: &XmpWriteInfo, value: &str) -> Option<Vec<String>> {
    let vals = value_conv_inv(t, &print_conv_inv(t, value)?)?;
    vals.iter().map(|v| check_xmp(t, v)).collect()
}

/// `PrintConvInv`, or the inverse of the PrintConv hash.
fn print_conv_inv(t: &XmpWriteInfo, value: &str) -> Option<String> {
    let v = value.to_string();
    Some(match t.conv {
        XmpConv::ConvertParameter => convert_parameter(&v)?,
        XmpConv::GpsDegreesLat | XmpConv::GpsDmsLat => {
            to_degrees_perl(&v, Some(true)).map(format_g15)?
        }
        XmpConv::GpsDegreesLon | XmpConv::GpsDmsLon => {
            to_degrees_perl(&v, Some(false)).map(format_g15)?
        }
        XmpConv::StripMm => strip_suffix_unit(&v, "mm"),
        XmpConv::StripM | XmpConv::GpsAltitude => strip_suffix_unit(&v, "m"),
        XmpConv::ShutterApex => convert_fraction(&v),
        XmpConv::LensInfo => convert_lens_info(&v),
        XmpConv::UnixTime | XmpConv::PdfDate => inverse_date_time(&v)?,
        XmpConv::PhotoMechPrefs => photomech_prefs(&v),
        XmpConv::DottedVersion => dotted_version(&v)?,
        XmpConv::FourCc => {
            let b = v.as_bytes();
            if b.len() == 4 && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_') {
                u32::from_be_bytes([b[0], b[1], b[2], b[3]]).to_string()
            } else {
                v
            }
        }
        _ => {
            if t.print_inv.is_empty() {
                v
            } else if let Some((_, raw)) = t
                .print_inv
                .iter()
                .find(|(p, _)| p.eq_ignore_ascii_case(value))
            {
                raw.to_string()
            } else if t.print_strict {
                // ReverseLookup: a hash without OTHER takes its printed
                // values only (a raw value needs `-n` or `#`).
                return None;
            } else {
                v
            }
        }
    })
}

/// `ValueConvInv`.
fn value_conv_inv(t: &XmpWriteInfo, v: &str) -> Option<Vec<String>> {
    let one = |s: String| Some(vec![s]);
    match t.conv {
        XmpConv::GpsDmsLat => one(to_dms_xmp(v.parse().ok()?, 'N', 'S')),
        XmpConv::GpsDmsLon => one(to_dms_xmp(v.parse().ok()?, 'E', 'W')),
        // /((?=\d|\.\d)\d*(?:\.\d*)?)/: the first unsigned number.
        XmpConv::GpsAltitude => one(first_unsigned_number(v)?),
        XmpConv::Apex => {
            let f = perl_number(v)?;
            one(format_g15(if f > 0.0 {
                2.0 * f.ln() / 2f64.ln()
            } else {
                0.0
            }))
        }
        XmpConv::ShutterApex => {
            let f = perl_number(v)?;
            one(format_g15(if f > 0.0 { -f.ln() / 2f64.ln() } else { 0.0 }))
        }
        XmpConv::Base64 => one(encode_base64(v.as_bytes())),
        XmpConv::UnixTime => one(get_unix_time(v)?.to_string()),
        XmpConv::PlusVocab => one(format!("http://ns.useplus.org/ldf/vocab/{v}")),
        XmpConv::IptcTime => one(crate::writer::iptc_writer::iptc_time(v)?),
        XmpConv::IntegerList => {
            // `warn "Expected one or more integer values" if $val =~ /[^-\d ]/`
            v.bytes()
                .all(|c| c == b'-' || c == b' ' || c.is_ascii_digit())
                .then(|| vec![v.to_string()])
        }
        XmpConv::PdfName => one(format!("/{v}")),
        XmpConv::PdfDate => one(pdf_date(v)),
        XmpConv::LensInfo => {
            let parts: Vec<&str> = v.split_whitespace().collect();
            let rats: Option<Vec<String>> = (parts.len() == 4)
                .then(|| {
                    parts
                        .iter()
                        .map(|p| match *p {
                            "inf" => Some("1/0".to_string()),
                            "undef" => Some("0/0".to_string()),
                            p if is_float(p) => {
                                let (n, d) = rationalize_max(p.parse().ok()?, 0x7fff_ffff as f64);
                                Some(format!("{n}/{d}"))
                            }
                            _ => None,
                        })
                        .collect()
                })
                .flatten();
            match (rats, t.kind) {
                // exifEX:LensInfo is a Seq: the list of four rationals.
                (Some(r), XmpKind::Seq) => Some(r),
                (Some(r), _) => one(r.join(" ")),
                // Not four numbers: the value as it is.
                (None, _) => one(v.to_string()),
            }
        }
        _ => one(v.to_string()),
    }
}

/// `CheckXMP`: the value formatted for the property's `Writable` type.
fn check_xmp(t: &XmpWriteInfo, v: &str) -> Option<String> {
    let v = v.to_string();
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

// ── Inverse conversions of the XMP tables, ported from ExifTool 13.59 ──────

/// Perl's `IsFloat`: `/^[+-]?(?=\d|\.\d)\d*(\.\d*)?([Ee]([+-]?\d+))?$/`.
fn is_perl_float(v: &str) -> bool {
    let s = v.strip_prefix(['+', '-']).unwrap_or(v);
    let (mant, exp) = match s.find(['e', 'E']) {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    let (int, frac) = mant.split_once('.').unwrap_or((mant, ""));
    let digits = |x: &str| x.bytes().all(|b| b.is_ascii_digit());
    let lead = int.bytes().next().is_some_and(|b| b.is_ascii_digit())
        || frac.bytes().next().is_some_and(|b| b.is_ascii_digit());
    let exp_ok = exp.is_none_or(|e| {
        let e = e.strip_prefix(['+', '-']).unwrap_or(e);
        !e.is_empty() && digits(e)
    });
    lead && digits(int) && digits(frac) && exp_ok
}

/// A string used as a number in a Perl expression: ExifTool turns the
/// "isn't numeric" warning into a rejected value. Surrounding white space
/// is allowed, as Perl allows it.
fn perl_number(v: &str) -> Option<f64> {
    let t = v.trim();
    if is_perl_float(t) {
        t.parse().ok()
    } else {
        match t.to_ascii_lowercase().as_str() {
            "inf" | "infinity" | "+inf" => Some(f64::INFINITY),
            "-inf" | "-infinity" => Some(f64::NEG_INFINITY),
            _ => None,
        }
    }
}

/// Exif.pm `ConvertParameter`: Normal 0, Low/Soft 1, High/Hard 2 (or the
/// sign of a number).
pub(crate) fn convert_parameter(v: &str) -> Option<String> {
    let float = is_perl_float(v).then(|| v.parse::<f64>().ok()).flatten();
    // A word starting with one of these letters (`\b` then the letter).
    let word_starts = |letters: &[u8]| {
        let b = v.as_bytes();
        (0..b.len()).any(|i| {
            letters.contains(&b[i].to_ascii_lowercase())
                && (i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_'))
        })
    };
    let n = if word_starts(b"n") || float == Some(0.0) {
        0
    } else if word_starts(b"sl") || float.is_some_and(|f| f < 0.0) {
        1
    } else if word_starts(b"h") || float.is_some() {
        2
    } else {
        return None;
    };
    Some(n.to_string())
}

/// `$val=~s/\s*<unit>$//`.
pub(crate) fn strip_suffix_unit(v: &str, unit: &str) -> String {
    match v.strip_suffix(unit) {
        Some(rest) => rest.trim_end().to_string(),
        None => v.to_string(),
    }
}

/// Exif.pm `ConvertFraction`: the first `n/d` becomes its value.
pub(crate) fn convert_fraction(v: &str) -> String {
    let b = v.as_bytes();
    for i in 0..b.len() {
        let start = i;
        let mut j = i;
        if matches!(b[j], b'+' | b'-') {
            j += 1;
        }
        let ds = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j == ds || j >= b.len() || b[j] != b'/' {
            continue;
        }
        let k0 = j + 1;
        let mut k = k0;
        while k < b.len() && b[k].is_ascii_digit() {
            k += 1;
        }
        if k == k0 {
            continue;
        }
        let (Ok(n), Ok(d)) = (v[start..j].parse::<f64>(), v[k0..k].parse::<f64>()) else {
            continue;
        };
        return if d != 0.0 {
            format_g15(n / d)
        } else if n != 0.0 {
            "inf".into()
        } else {
            "undef".into()
        };
    }
    v.to_string()
}

/// GPS.pm `ToDegrees($val, 1, $coord)`: degrees out of up to three numbers
/// in any text, negative for an S or W reference. `lat`: which half of a
/// `lat N, lon E` pair to take. `None` for no number (ExifTool's '').
fn to_degrees_perl(v: &str, lat: Option<bool>) -> Option<f64> {
    if v.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|w| w == "inf" || w == "undef")
    {
        return None;
    }
    let mut v = v.to_string();
    if let Some(lat) = lat {
        // /^(.*(?:N(?:orth)?|S(?:outh)?)),\s*(.*(?:E(?:ast)?|W(?:est)?))$/i
        let up = v.to_ascii_uppercase();
        let ends = |s: &str, refs: &[&str]| refs.iter().any(|r| s.ends_with(r));
        let split = (0..v.len()).filter(|&i| v.is_char_boundary(i)).find(|&i| {
            v[i..].starts_with(',')
                && ends(&up[..i], &["N", "NORTH", "S", "SOUTH"])
                && ends(up.trim_end(), &["E", "EAST", "W", "WEST"])
        });
        if let Some(i) = split {
            v = if lat {
                v[..i].to_string()
            } else {
                v[i + 1..].trim_start().to_string()
            };
        }
    }
    // /((?:[+-]?)(?=\d|\.\d)\d*(?:\.\d*)?(?:[Ee][+-]\d+)?)/g
    let b = v.as_bytes();
    let mut nums = Vec::new();
    let mut i = 0;
    while i < b.len() && nums.len() < 3 {
        let start = i;
        let mut j = i;
        if matches!(b[j], b'+' | b'-') {
            j += 1;
        }
        let digit_at = |k: usize| k < b.len() && b[k].is_ascii_digit();
        if !(digit_at(j) || (j < b.len() && b[j] == b'.' && digit_at(j + 1))) {
            i += 1;
            continue;
        }
        while digit_at(j) {
            j += 1;
        }
        if j < b.len() && b[j] == b'.' {
            j += 1;
            while digit_at(j) {
                j += 1;
            }
        }
        if j + 1 < b.len() && matches!(b[j], b'e' | b'E') && matches!(b[j + 1], b'+' | b'-') {
            let mut k = j + 2;
            while digit_at(k) {
                k += 1;
            }
            if k > j + 2 {
                j = k;
            }
        }
        nums.push(v[start..j].parse::<f64>().ok()?);
        i = j;
    }
    let d = *nums.first()?;
    let deg = d + (nums.get(1).unwrap_or(&0.0) + nums.get(2).unwrap_or(&0.0) / 60.0) / 60.0;
    // /[^A-Z](S(outh)?|W(est)?)\s*$/i
    let up = v.trim_end().to_ascii_uppercase();
    let south = ["S", "SOUTH", "W", "WEST"].iter().any(|r| {
        up.strip_suffix(r)
            .and_then(|rest| rest.bytes().last())
            .is_some_and(|c| !c.is_ascii_uppercase())
    });
    Some(if south { -deg } else { deg })
}

/// GPS.pm `ToDMS($self, $val, 2, $ref)`: XMP's `DDD,MM.mmmmmmmm[NSEW]`,
/// trailing zeros of the minutes removed.
fn to_dms_xmp(val: f64, pos: char, neg: char) -> String {
    let (val, r) = if val < 0.0 { (-val, neg) } else { (val, pos) };
    let mut d = val.trunc();
    let mut m: f64 = format!("{:.8}", (val - d) * 60.0).parse().unwrap_or(0.0);
    if m >= 60.0 {
        m -= 60.0;
        d += 1.0;
    }
    let s = format!("{},{:.8}", d as i64, m);
    // s/(\d)0+$ref$/$1$ref/
    let trimmed = s.trim_end_matches('0');
    let cut = if trimmed.ends_with('.') && trimmed.len() + 1 < s.len() {
        trimmed.len() + 1
    } else if trimmed.ends_with('.') {
        s.len()
    } else {
        trimmed.len()
    };
    format!("{}{r}", &s[..cut])
}

/// `$val=~/((?=\d|\.\d)\d*(?:\.\d*)?)/ ? $1 : undef`.
fn first_unsigned_number(v: &str) -> Option<String> {
    let b = v.as_bytes();
    let digit = |k: usize| k < b.len() && b[k].is_ascii_digit();
    let i = (0..b.len()).find(|&i| digit(i) || (b[i] == b'.' && digit(i + 1)))?;
    let mut j = i;
    while digit(j) {
        j += 1;
    }
    if j < b.len() && b[j] == b'.' {
        j += 1;
        while digit(j) {
            j += 1;
        }
    }
    Some(v[i..j].to_string())
}

/// Exif.pm `ConvertLensInfo`: `12-20mm f/3.8-4.5` → `12 20 3.8 4.5`
/// (`GetLensInfo`, `?` for an unknown value); other values as they are.
pub(crate) fn convert_lens_info(v: &str) -> String {
    // /($pat)(?:-($pat))?\s*mm.*?(?:[fF]\/?\s*)($pat)(?:-($pat))?/, $pat = \d+(?:\.\d+)?|\?
    let b = v.as_bytes();
    let num = |i: usize| -> Option<usize> {
        if i < b.len() && b[i] == b'?' {
            return Some(i + 1);
        }
        let mut j = i;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j == i {
            return None;
        }
        if j + 1 < b.len() && b[j] == b'.' && b[j + 1].is_ascii_digit() {
            j += 1;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
        }
        Some(j)
    };
    let range = |i: usize| -> Option<(&str, Option<&str>, usize)> {
        let a = num(i)?;
        if a < b.len() && b[a] == b'-' {
            if let Some(c) = num(a + 1) {
                return Some((&v[i..a], Some(&v[a + 1..c]), c));
            }
        }
        Some((&v[i..a], None, a))
    };
    for i in 0..b.len() {
        let Some((f1, f2, mut p)) = range(i) else {
            continue;
        };
        while p < b.len() && b[p].is_ascii_whitespace() {
            p += 1;
        }
        if !v[p..].starts_with("mm") {
            continue;
        }
        // Lazily, the first f-number after "mm".
        let rest = p + 2;
        let found = (rest..b.len()).find_map(|k| {
            if !matches!(b[k], b'f' | b'F') {
                return None;
            }
            let mut q = k + 1;
            if q < b.len() && b[q] == b'/' {
                q += 1;
            }
            while q < b.len() && b[q].is_ascii_whitespace() {
                q += 1;
            }
            range(q)
        });
        let Some((a1, a2, _)) = found else {
            continue;
        };
        let undef = |s: &str| {
            if s == "?" {
                "undef".to_string()
            } else {
                s.to_string()
            }
        };
        let f2 = f2.filter(|s| !is_zero_perl(s)).unwrap_or(f1);
        let a2 = a2.filter(|s| !is_zero_perl(s)).unwrap_or(a1);
        return [f1, f2, a1, a2].map(undef).join(" ");
    }
    v.to_string()
}

/// A Perl false string: `""`, `"0"` (`$a[1] or $a[1] = $a[0]`).
fn is_zero_perl(s: &str) -> bool {
    s.is_empty() || s == "0"
}

/// Writer.pl `Rationalize($val, $maxInt)`.
fn rationalize_max(val: f64, max: f64) -> (i64, u64) {
    if val == 0.0 {
        return (0, 1);
    }
    let sign = if val < 0.0 { -1 } else { 1 };
    let v = val.abs();
    let mut fracs: Vec<f64> = Vec::new();
    let mut frac = v;
    let mut best: Option<(f64, f64)> = None;
    loop {
        let (mut n, mut d) = ((frac + 0.5).trunc(), 1.0);
        for &f in &fracs {
            let nn = f * n + d;
            d = n;
            n = nn;
        }
        if n > max || d > max {
            if best.is_some() {
                break;
            }
            return if v < 1.0 {
                (sign, max as u64)
            } else {
                (sign * max as i64, 1)
            };
        }
        best = Some((n, d));
        if ((n / d - v) / v).abs() < 1e-8 {
            break;
        }
        let int = frac.trunc();
        fracs.insert(0, int);
        frac -= int;
        if frac == 0.0 || fracs.len() > 64 {
            break;
        }
        frac = 1.0 / frac;
    }
    let (n, d) = best.unwrap_or((0.0, 1.0));
    (sign * n as i64, d as u64)
}

/// WriteXMP.pl `EncodeBase64`: base64 in lines of 60 characters, each
/// ending with a newline.
fn encode_base64(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(45) {
        for g in chunk.chunks(3) {
            let n = (u32::from(g[0]) << 16)
                | (u32::from(*g.get(1).unwrap_or(&0)) << 8)
                | u32::from(*g.get(2).unwrap_or(&0));
            for k in 0..4 {
                if k <= g.len() {
                    out.push(A[((n >> (18 - 6 * k)) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out.push('\n');
    }
    out
}

/// Writer.pl `InverseDateTime($val)` (no DateFormat option): any date and
/// time → `YYYY:mm:dd HH:MM:SS[.ss][zone]`. `None` when ExifTool warns.
pub(crate) fn inverse_date_time(val: &str) -> Option<String> {
    let mut v = val.to_string();
    let mut tz = String::new();
    // s/([-+])(\d{1,2}):?(\d{2})\s*(DST)?$//i
    let up = v.to_ascii_uppercase();
    let body = up.strip_suffix("DST").unwrap_or(&up).trim_end().len();
    let b = v.as_bytes();
    let zone = (0..body).rev().find_map(|i| {
        if !matches!(b[i], b'+' | b'-') {
            return None;
        }
        let z = &v[i + 1..body];
        let digits: String = z.chars().filter(|c| *c != ':').collect();
        let colon_ok = z.matches(':').count() <= 1 && !z.starts_with(':');
        if !(3..=4).contains(&digits.len())
            || !digits.bytes().all(|c| c.is_ascii_digit())
            || !colon_ok
            || (z.contains(':') && z.len() - z.find(':')? != 3)
        {
            return None;
        }
        let (h, m) = digits.split_at(digits.len() - 2);
        Some((
            i,
            format!("{}{:02}:{m}", b[i] as char, h.parse::<u32>().ok()?),
        ))
    });
    if let Some((i, z)) = zone {
        v.truncate(i);
        tz = z;
    } else if v.ends_with(['Z', 'z']) {
        v.pop();
        tz = "Z".into();
    } else if v.eq_ignore_ascii_case("now") {
        return Some(time_now());
    }
    // /(\d{4})/g, then every /\d{1,2}/ after it.
    let b = v.as_bytes();
    let yr_at =
        (0..b.len()).find(|&i| i + 4 <= b.len() && b[i..i + 4].iter().all(u8::is_ascii_digit))?;
    let yr = &v[yr_at..yr_at + 4];
    let mut a: Vec<String> = Vec::new();
    let mut i = yr_at + 4;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let n = if i + 1 < b.len() && b[i + 1].is_ascii_digit() {
                2
            } else {
                1
            };
            a.push(format!("{:0>2}", &v[i..i + n]));
            i += n;
        } else {
            i += 1;
        }
    }
    if a.len() < 3 {
        return None;
    }
    let ss = a.get(4).cloned();
    while a.len() < 5 {
        a.push("00".into());
    }
    let fs = if a.len() > 5 {
        // /(\.\d+)\s*$/
        let t = v.trim_end();
        let digits = t.len() - t.trim_end_matches(|c: char| c.is_ascii_digit()).len();
        let at = t.len() - digits;
        if digits > 0 && at > 0 && t.as_bytes()[at - 1] == b'.' {
            t[at - 1..].to_string()
        } else {
            String::new()
        }
    } else {
        String::new()
    };
    let num = |s: &str| s.parse::<u32>().unwrap_or(0);
    let ss = match ss {
        Some(s) if num(&s) < 60 => format!(":{s}"),
        _ => ":00".into(),
    };
    if !(1..=12).contains(&num(&a[0]))
        || !(1..=31).contains(&num(&a[1]))
        || num(&a[2]) > 24
        || num(&a[3]) > 59
    {
        return None;
    }
    Some(format!(
        "{yr}:{}:{} {}:{}{ss}{fs}{tz}",
        a[0], a[1], a[2], a[3]
    ))
}

/// Writer.pl `TimeNow`: the local date and time, with its zone.
fn time_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let off = crate::formats::gzip::local_tz_offset(now);
    let t = now + off;
    let (days, secs) = (t.div_euclid(86400), t.rem_euclid(86400));
    // Civil date from days since 1970-01-01.
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let min = (off as f64 / 60.0).round() as i64;
    let (sign, min) = if min < 0 { ('-', -min) } else { ('+', min) };
    format!(
        "{y:4}:{m:02}:{d:02} {:02}:{:02}:{:02}{sign}{:02}:{:02}",
        secs / 3600,
        secs / 60 % 60,
        secs % 60,
        min / 60,
        min % 60
    )
}

/// ExifTool.pm `GetUnixTime` (UTC): `YYYY:mm:dd HH:MM:SS` → seconds.
fn get_unix_time(v: &str) -> Option<i64> {
    if v == "0000:00:00 00:00:00" {
        return Some(0);
    }
    // /^(\d+)[-:](\d+)[-:](\d+)\s+(\d+):(\d+):(\d+)(.*)/
    let (date, rest) = v.split_once(|c: char| c.is_ascii_whitespace())?;
    let d: Vec<&str> = date.split(['-', ':']).collect();
    let rest = rest.trim_start();
    let t: Vec<&str> = rest.splitn(3, ':').collect();
    if d.len() != 3 || t.len() != 3 {
        return None;
    }
    let sec_digits: String = t[2].chars().take_while(char::is_ascii_digit).collect();
    let n = |s: &str| s.parse::<i64>().ok();
    let (y, mo, da, h, mi, s) = (
        n(d[0])?,
        n(d[1])?,
        n(d[2])?,
        n(t[0])?,
        n(t[1])?,
        n(&sec_digits)?,
    );
    let days = {
        let y = if mo <= 2 { y - 1 } else { y };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let doy = (153 * ((mo + 9) % 12) + 2) / 5 + da - 1;
        era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468
    };
    Some(days * 86400 + h * 3600 + mi * 60 + s)
}

/// PDF.pm `WritePDFValue($val, "date")` with its parentheses removed:
/// `D:YYYYmmddHHMMSS+HH'MM'`.
fn pdf_date(v: &str) -> String {
    let mut s = v.to_string();
    // s/(:\d{2})\.\d*/$1/
    if let Some(i) = (0..s.len().saturating_sub(3)).find(|&i| {
        let b = s.as_bytes();
        b[i] == b':' && b[i + 1].is_ascii_digit() && b[i + 2].is_ascii_digit() && b[i + 3] == b'.'
    }) {
        let end = s[i + 4..]
            .find(|c: char| !c.is_ascii_digit())
            .map_or(s.len(), |k| i + 4 + k);
        s.replace_range(i + 3..end, "");
    }
    // s/([-+]\d{2}):(\d{2})/${1}'${2}'/
    if let Some(i) = (0..s.len().saturating_sub(5)).find(|&i| {
        let b = s.as_bytes();
        matches!(b[i], b'+' | b'-')
            && b[i + 1].is_ascii_digit()
            && b[i + 2].is_ascii_digit()
            && b[i + 3] == b':'
            && b[i + 4].is_ascii_digit()
            && b[i + 5].is_ascii_digit()
    }) {
        s = format!("{}'{}'{}", &s[..i + 3], &s[i + 4..i + 6], &s[i + 6..]);
    }
    let s: String = s
        .chars()
        .filter(|c| !matches!(c, ' ' | ':' | '(' | ')'))
        .collect();
    format!("D:{s}")
}

/// PhotoMechanic `Prefs`: `Tagged:1, ColorClass:2, Rating:3, FrameNum:4` →
/// `1:2:3:4`; other values as they are.
fn photomech_prefs(v: &str) -> String {
    // s[Tagged:\s*(\d+).*ColorClass:\s*(\d+).*Rating:\s*(\d+).*FrameNum:\s*(\S*)][$1:$2:$3:$4]is
    let lc = v.to_ascii_lowercase();
    let field = |from: usize, key: &str, digits: bool| -> Option<(usize, usize, usize)> {
        let k = from + lc[from..].find(key)?;
        let mut p = k + key.len();
        p += lc[p..].len() - lc[p..].trim_start().len();
        let len = lc[p..]
            .find(|c: char| {
                if digits {
                    !c.is_ascii_digit()
                } else {
                    c.is_whitespace()
                }
            })
            .unwrap_or(lc.len() - p);
        (digits && len == 0)
            .then_some(())
            .map_or(Some((k, p, p + len)), |_| None)
    };
    let try_at = || -> Option<String> {
        let (t0, a, a_end) = field(0, "tagged:", true)?;
        // Greedy `.*`: the last of each key that still lets the rest match.
        let keys = ["colorclass:", "rating:", "framenum:"];
        let mut pos = a_end;
        let mut vals = vec![&v[a..a_end]];
        let mut end = a_end;
        for (n, key) in keys.iter().enumerate() {
            let starts: Vec<usize> = lc[pos..].match_indices(key).map(|(i, _)| pos + i).collect();
            let mut chosen = None;
            for &st in starts.iter().rev() {
                let Some((_, b, b_end)) = field(st, key, n < 2) else {
                    continue;
                };
                // The remaining keys must follow.
                if keys[n + 1..].iter().all(|k| lc[b_end..].contains(k)) {
                    chosen = Some((b, b_end));
                    break;
                }
            }
            let (b, b_end) = chosen?;
            vals.push(&v[b..b_end]);
            pos = b_end;
            end = b_end;
        }
        Some(format!("{}{}{}", &v[..t0], vals.join(":"), &v[end..]))
    };
    try_at().unwrap_or_else(|| v.to_string())
}

/// HDRGainMap `HDRGainMapVersion`: `a.b.c.d` → the int32u of those bytes.
fn dotted_version(v: &str) -> Option<String> {
    let parts: Vec<&str> = v.split('.').collect();
    if parts.len() == 4
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()))
    {
        // pack('C*') warns on a part over 255: the value is rejected.
        let bytes: Option<Vec<u8>> = parts.iter().map(|p| p.parse::<u8>().ok()).collect();
        let b = bytes?;
        Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]).to_string())
    } else {
        Some(v.to_string())
    }
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
        let fnum = resolve(Some("exif"), "FNumber", "2.8").unwrap();
        assert_eq!(fnum.create[0].values, ["14/5"]);
    }

    /// The property values Perl ExifTool 13.59 writes for each ported
    /// conversion (`exiftool -o out.xmp -<tag>=<value> empty.xmp`).
    #[test]
    fn conversions_write_what_perl_writes() {
        let cases: &[(&str, &str, &str, &[&str])] = &[
            ("exif", "GPSLatitude", "48.8566", &["48,51.396N"]),
            ("exif", "GPSLatitude", "48 30 S", &["48,30.0S"]),
            ("exif", "GPSLongitude", "-122.0", &["122,0.0W"]),
            ("exif", "GPSLatitude", "30.5, 2.3 E", &["30,32.3N"]),
            ("drone-dji", "GPSLatitude", "48 30 S", &["-48.5"]),
            ("exif", "ApertureValue", "2.8", &["24361/8200"]),
            ("exif", "ShutterSpeedValue", "1/100", &["13301/2002"]),
            ("exif", "FocalLength", "50 mm", &["50/1"]),
            ("exif", "FocalLengthIn35mmFormat", "75 mm", &["75"]),
            ("exif", "GPSAltitude", "-5 m", &["5/1"]),
            ("exif", "Saturation", "High", &["2"]),
            ("exif", "Saturation", "1", &["2"]),
            ("exif", "Sharpness", "Soft", &["1"]),
            ("exif", "ExposureCompensation", "+1/3", &["+1/3"]),
            (
                "exifEX",
                "LensInfo",
                "18-55mm f/3.5-5.6",
                &["18/1", "55/1", "7/2", "28/5"],
            ),
            ("aux", "LensInfo", "50mm f/1.4", &["50/1 50/1 7/5 7/5"]),
            ("aux", "LensInfo", "junk", &["junk"]),
            ("aux", "LensID", "12 34", &["12 34"]),
            (
                "GSpherical",
                "TimeStamp",
                "2024:01:01 12:00:00+02:00",
                &["1704110400"],
            ),
            (
                "plus",
                "CopyrightStatus",
                "Protected",
                &["http://ns.useplus.org/ldf/vocab/CS-PRO"],
            ),
            ("pdf", "Trapped", "True", &["/True"]),
            (
                "pdfx",
                "SourceModified",
                "2024:01:15 12:34:56.5+01:00",
                &["D:20240115123456+01'00'"],
            ),
            (
                "photomech",
                "TimeCreated",
                "10:20:30+01:00",
                &["102030+0100"],
            ),
            (
                "photomech",
                "Prefs",
                "Tagged:1, ColorClass:2, Rating:3, FrameNum:4",
                &["1:2:3:4"],
            ),
            ("HDRGainMap", "HDRGainMapVersion", "1.2.3.4", &["16909060"]),
            ("apdi", "NativeFormat", "JPEG", &["1246774599"]),
            ("apdi", "NativeFormat", "abc", &["abc"]),
            ("GImage", "ImageData", "hello", &["aGVsbG8=\n"]),
            ("exif", "ColorSpace", "Adobe RGB", &["2"]),
        ];
        for (ns, tag, value, want) in cases {
            let t = resolve(Some(ns), tag, value)
                .unwrap_or_else(|| panic!("{ns}:{tag}={value} rejected"));
            assert_eq!(t.create[0].values, *want, "{ns}:{tag}={value}");
        }
        // Values Perl rejects.
        for (ns, tag, value) in [
            ("exif", "GPSLatitude", "abc"),
            ("exif", "ApertureValue", "abc"),
            ("exif", "ShutterSpeedValue", "1/0"),
            ("exif", "Saturation", "x"),
            ("exif", "ColorSpace", "2"),
            ("exifEX", "LensInfo", "junk"),
            ("aux", "LensID", "x1"),
            ("GSpherical", "TimeStamp", "2024:13:01 00:00:00"),
            ("plus", "CopyrightStatus", "CS-PRO"),
            ("pdf", "Trapped", "maybe"),
            ("pdfx", "SourceModified", "2024:01:15"),
            ("HDRGainMap", "HDRGainMapVersion", "300.1.1.1"),
        ] {
            assert!(
                resolve(Some(ns), tag, value).is_none(),
                "{ns}:{tag}={value}"
            );
        }
    }

    #[test]
    fn every_xmp_conversion_is_ported() {
        assert!(XMP_WRITE_TAGS.iter().all(|t| t.conv != XmpConv::Unported));
    }
}
