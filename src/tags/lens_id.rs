//! Identifying a lens from an XMP LensID.
//!
//! `Image::ExifTool::XMP::PrintLensID` is the conversion behind the composite
//! `XMP-aux:LensID` Adobe applications write. It picks a maker's lens table by
//! the Make in the file, looks the id up, and then hands the answer to
//! `Exif::PrintLensID`, which narrows a "A or B or C" entry down using the
//! focal length and maximum aperture the file also records. Canon gets a
//! narrowing of its own, `Canon::PrintLensID`, which knows about
//! teleconverters.
//!
//! The one thing here that is not ExifTool's is `%Image::ExifTool::userLens`:
//! it is a user's own list of lenses, read from a .ExifTool_config that this
//! crate has no equivalent of, and it is empty in every run of ExifTool that
//! does not have one. Every branch that reads it is therefore the branch
//! ExifTool takes with an empty list, and is written that way.

use std::sync::LazyLock;

use regex_lite::Regex;

use super::lens_tables_generated as tables;

/// The lens name filed under this key, if the table has one.
///
/// The generated tables are sorted by key; the ones built here -- Sony's
/// E-mount list, Nikon's -- are built in the order ExifTool numbers them and
/// are sorted before any lookup, because a binary search over an unsorted
/// table answers with whatever it lands on.
fn get<'a>(table: &'a [(&'a str, &'a str)], key: &str) -> Option<&'a str> {
    table
        .binary_search_by(|(k, _)| (*k).cmp(key))
        .ok()
        .map(|i| table[i].1)
}

/// Sorted by key, ready for `get`.
fn sorted(rows: &[(String, String)]) -> Vec<(&str, &str)> {
    let mut v: Vec<(&str, &str)> = rows
        .iter()
        .map(|(k, val)| (k.as_str(), val.as_str()))
        .collect();
    v.sort_by(|a, b| a.0.cmp(b.0));
    v
}

/// Perl reads a leading number out of a string and calls the rest zero, which
/// is what happens to a LensID that is not a bare number.
fn numify(s: &str) -> f64 {
    let t = s.trim_start();
    let mut end = 0;
    let b = t.as_bytes();
    if end < b.len() && (b[end] == b'-' || b[end] == b'+') {
        end += 1;
    }
    while end < b.len() && b[end].is_ascii_digit() {
        end += 1;
    }
    if end < b.len() && b[end] == b'.' {
        let mut e = end + 1;
        while e < b.len() && b[e].is_ascii_digit() {
            e += 1;
        }
        if e > end + 1 {
            end = e;
        }
    }
    t[..end].parse().unwrap_or(0.0)
}

/// Perl prints a number without a trailing `.0`, and that is how these ids are
/// written into a key and into `Unknown (...)`.
fn num_str(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// The focal length and aperture range written into a lens name.
///
/// `Exif::GetLensInfo`: the short focal, the long one (the short one again for
/// a prime), the maximum aperture at the short focal, and at the long one.
#[must_use]
pub fn get_lens_info(lens: &str) -> Option<(f64, f64, f64, f64)> {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(\d+(?:\.\d+)?)(?:-(\d+(?:\.\d+)?))?\s*mm.*?(?:[fF]/?\s*)(\d+(?:\.\d+)?)(?:-(\d+(?:\.\d+)?))?",
        )
        .expect("static pattern")
    });
    let c = RE.captures(lens)?;
    let num = |i: usize| c.get(i).and_then(|m| m.as_str().parse::<f64>().ok());
    let sf = num(1)?;
    let sa = num(3)?;
    Some((sf, num(2).unwrap_or(sf), sa, num(4).unwrap_or(sa)))
}

/// `Exif::MatchLensModel`: narrow a list of candidates by what LensModel says.
fn match_lens_model(list: &mut Vec<String>, lens_model: Option<&str>) {
    let Some(model) = lens_model.filter(|m| !m.is_empty()) else {
        return;
    };
    if list.len() < 2 {
        return;
    }
    static FOCAL: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"((\d+-)?\d+mm)").expect("static pattern"));
    static APERTURE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)(?:F/?|1:)(\d+(\.\d+)?)").expect("static pattern"));

    if let Some(c) = FOCAL.captures(model) {
        let focal = c.get(1).map_or("", |m| m.as_str());
        let filt: Vec<String> = list.iter().filter(|l| l.contains(focal)).cloned().collect();
        if !filt.is_empty() && filt.len() < list.len() {
            *list = filt;
        }
    }
    if list.len() > 1 {
        if let Some(c) = APERTURE.captures(model) {
            let fnum = c.get(1).map_or("", |m| m.as_str());
            // `(F/?|1:)$fnum(\b|[A-Z])`, case-insensitively.
            let pat = format!(r"(?i)(F/?|1:){}(\b|[A-Z])", regex_escape(fnum));
            if let Ok(re) = Regex::new(&pat) {
                let filt: Vec<String> =
                    list.iter().filter(|l| re.is_match(l)).cloned().collect();
                if !filt.is_empty() && filt.len() < list.len() {
                    *list = filt;
                }
            }
        }
    }
    for pat in ["I+", "USM"] {
        if list.len() < 2 {
            break;
        }
        let Ok(re) = Regex::new(&format!(r"\b({pat})\b")) else {
            continue;
        };
        let Some(c) = re.captures(model) else { continue };
        let val = c.get(1).map_or("", |m| m.as_str());
        let Ok(word) = Regex::new(&format!(r"\b{}\b", regex_escape(val))) else {
            continue;
        };
        let filt: Vec<String> = list.iter().filter(|l| word.is_match(l)).cloned().collect();
        if !filt.is_empty() && filt.len() < list.len() {
            *list = filt;
        }
    }
}

fn regex_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if !c.is_alphanumeric() && c != '_' {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Every lens filed under this id: the plain key, then `id.1`, `id.2` ...
fn alternatives(table: &[(&str, &str)], id: &str) -> Vec<String> {
    let Some(first) = get(table, id) else {
        return Vec::new();
    };
    // "Canon EF 28mm f/2.8 or Sigma Lens" is the first of the list, not a
    // lens of its own: everything from " or " on names the others.
    let mut out = vec![first.split(" or ").next().unwrap_or(first).to_string()];
    let mut i = 1;
    while let Some(l) = get(table, &format!("{id}.{i}")) {
        out.push(l.to_string());
        i += 1;
    }
    out
}

/// `Canon::LensWithTC`: name the teleconverter the focal length implies.
fn lens_with_tc(lens: &str, short_focal: f64) -> String {
    static FIRST_NUM: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(\d+)").expect("static pattern"));
    if lens.ends_with('x') {
        return lens.to_string();
    }
    let Some(c) = FIRST_NUM.captures(lens) else {
        return lens.to_string();
    };
    let Ok(sf) = c[1].parse::<f64>() else {
        return lens.to_string();
    };
    for tc in [1.0, 1.4, 2.0, 2.8] {
        if (short_focal - sf * tc).abs() > 0.9 {
            continue;
        }
        return if tc > 1.0 {
            format!("{lens} + {}x", num_str(tc))
        } else {
            lens.to_string()
        };
    }
    lens.to_string()
}

/// `Canon::PrintLensID`: Canon writes the focal range and aperture the lens
/// reported, which tells the lenses sharing a LensType apart -- and says
/// whether a teleconverter is in the way.
#[allow(clippy::too_many_lines)]
fn canon_print_lens_id(
    table: &[(&str, &str)],
    is_canon_table: bool,
    lens_type: &str,
    short_focal: f64,
    long_focal: f64,
    max_aperture: f64,
    lens_model: Option<&str>,
) -> String {
    static LENS_RANGE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(\d+)(?:-(\d+))?mm.*?(?:[fF]/?)(\d+(?:\.\d+)?)(?:-(\d+(?:\.\d+)?))?")
            .expect("static pattern")
    });
    static PLUS_TC: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r" \+ (\d+(\.\d+)?)x$").expect("static pattern"));
    static NAMED_TC: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^(.*) \+ (RF)?(\d+(\.\d*)?)x$").expect("static pattern"));
    static MODEL_TC: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r" \+ ((EXTENDER )?RF)?(\d+(\.\d*)?)x\b").expect("static pattern")
    });

    let named = if lens_type == "-1" || lens_type == "65535" {
        None
    } else {
        get(table, lens_type)
    };
    if let Some(lens) = named {
        if get(table, &format!("{lens_type}.1")).is_none() {
            return lens_with_tc(lens, short_focal);
        }
        let lenses = alternatives(table, lens_type);
        let mut maybe: Vec<String> = Vec::new();
        let mut likely: Vec<String> = Vec::new();
        let mut matches: Vec<String> = Vec::new();
        // A LensModel naming its own teleconverter fixes the factor.
        let tcs: Vec<f64> = lens_model
            .and_then(|m| MODEL_TC.captures(m))
            .and_then(|c| c.get(3))
            .and_then(|m| m.as_str().parse::<f64>().ok())
            .map_or_else(|| vec![1.0, 1.4, 2.0, 2.8], |t| vec![t]);
        for tc in tcs {
            for lens in &lenses {
                let Some(c) = LENS_RANGE.captures(lens) else {
                    continue;
                };
                let n = |i: usize| c.get(i).and_then(|m| m.as_str().parse::<f64>().ok());
                let (mut sf, mut lf) = (n(1).unwrap_or(0.0), n(2).unwrap_or(0.0));
                let (mut sa, mut la) = (n(3).unwrap_or(0.0), n(4).unwrap_or(0.0));
                if sf != 0.0 && lf == 0.0 {
                    lf = sf;
                }
                if sa != 0.0 && la == 0.0 {
                    la = sa;
                }
                // A LensType that ends in " + 1.4x" already counts the converter.
                if let Some(c) = PLUS_TC.captures(lens) {
                    if let Ok(f) = c[1].parse::<f64>() {
                        sf *= f;
                        lf *= f;
                        sa *= f;
                        la *= f;
                    }
                }
                if (short_focal - sf * tc).abs() > 0.9 {
                    continue;
                }
                let mut tclens = lens.clone();
                if let Some(c) = NAMED_TC.captures(lens) {
                    if c.get(3).map_or("", |m| m.as_str()) != num_str(tc) {
                        continue;
                    }
                    let lns = c.get(1).map_or("", |m| m.as_str());
                    for list in [&mut maybe, &mut likely, &mut matches] {
                        if list.last().is_some_and(|l| l.starts_with(lns)) {
                            list.pop();
                        }
                    }
                } else if tc > 1.0 {
                    tclens = format!("{lens} + {}x", num_str(tc));
                }
                maybe.push(tclens.clone());
                if (long_focal - lf * tc).abs() > 0.9 {
                    continue;
                }
                likely.push(tclens.clone());
                if max_aperture != 0.0
                    && (max_aperture < sa * tc - 0.18 || max_aperture > la * tc + 0.18)
                {
                    continue;
                }
                matches.push(tclens);
            }
            if !maybe.is_empty() {
                break;
            }
        }
        // Sigma files an Art, a Contemporary and a Sports under one id.
        if matches.len() > 1 {
            if let Some(m) = lens_model {
                if let Some(c) = Regex::new(r"(\| [ACS])")
                    .ok()
                    .and_then(|re| re.captures(m).map(|c| c[1].to_string()))
                {
                    let best: Vec<String> =
                        matches.iter().filter(|l| l.contains(&c)).cloned().collect();
                    if !best.is_empty() {
                        matches = best;
                    }
                }
            }
        }
        if matches.is_empty() {
            matches = likely;
        }
        if matches.is_empty() {
            matches = maybe;
        }
        if matches.len() > 1 {
            if let Some(m) = lens_model {
                static MM_F: LazyLock<Regex> = LazyLock::new(|| {
                    Regex::new(
                        r"(?i)(\d+(?:\.\d+)?(?:-\d+(?:\.\d+)?)?) ?mm ?f/?(\d+(?:\.\d+)?(?:-\d+(?:\.\d+)?)?)",
                    )
                    .expect("static pattern")
                });
                if let Some(c) = MM_F.captures(m) {
                    let (mm, fstop) = (c[1].to_string(), c[2].to_string());
                    let best: Vec<String> = matches
                        .iter()
                        .filter(|l| {
                            MM_F.captures(l)
                                .is_some_and(|c| c[1] == mm && c[2] == fstop)
                        })
                        .cloned()
                        .collect();
                    if !best.is_empty() {
                        matches = best;
                    }
                }
            }
        }
        match_lens_model(&mut matches, lens_model);
        if !matches.is_empty() {
            return matches.join(" or ");
        }
    } else if lens_model.is_some_and(|m| m.chars().any(|c| c.is_ascii_digit())) {
        let m = lens_model.unwrap_or_default();
        // A Canon body only understands Canon lenses, so the model it wrote
        // is one.
        return if is_canon_table {
            format!("Canon {m}")
        } else {
            m.to_string()
        };
    }
    let mut str = String::new();
    if short_focal != 0.0 {
        str.push_str(&format!(" {}", short_focal as i64));
        if long_focal != 0.0 && long_focal != short_focal {
            str.push_str(&format!("-{}", long_focal as i64));
        }
        str.push_str("mm");
    }
    if lens_type == "-1" || lens_type == "65535" {
        format!("Unknown{str}")
    } else {
        format!("Unknown ({lens_type}){str}")
    }
}

/// Which of the lenses sharing an id this one is.
///
/// `Exif::PrintLensID`, as `XMP::PrintLensID` calls it: with a hash of lens
/// names in hand and no LensSpec, which cuts out the branches that read one.
#[allow(clippy::too_many_arguments)]
fn exif_print_lens_id<'a>(
    make: &str,
    model: &str,
    table: &'a [(&'a str, &'a str)],
    is_canon_table: bool,
    lens_type_prt: &str,
    lens_type: &str,
    focal_length: f64,
    max_aperture: f64,
    max_aperture_value: f64,
    short_focal: f64,
    long_focal: f64,
    lens_model: Option<&str>,
) -> Option<String> {
    let mut table: &'a [(&'a str, &'a str)] = table;
    // Only the Canon branch below reads this, and the Sony branch above --
    // the one that swaps in another maker's table -- returns before it.
    let mut lens_type = lens_type.to_string();
    let mut lens_type_prt = lens_type_prt.to_string();
    // MaxApertureValue stands in when MaxAperture is not there.
    let max_aperture = if max_aperture == 0.0 {
        max_aperture_value
    } else {
        max_aperture
    };

    if make == "SONY" {
        let n = numify(&lens_type);
        if lens_type == "65535" {
            // A manual lens reports nothing but this.
            if let Some(l) = get(table, &lens_type) {
                if focal_length == 0.0 && (max_aperture - 1.0).abs() < f64::EPSILON {
                    return Some(l.to_string());
                }
            }
            if model.contains("NEX") || model.contains("ILCE") {
                // Sony's E-mount lenses, filed under 65535 and 65535.N in the
                // order their names first appear in sonyLensTypes2.
                let mut did: Vec<&str> = Vec::new();
                let mut etype: Vec<(String, String)> = Vec::new();
                for (_, name) in tables::SONYLENSTYPES2 {
                    let lens = name.split(" or ").next().unwrap_or(name);
                    if did.contains(&lens) {
                        continue;
                    }
                    did.push(lens);
                    let key = if etype.is_empty() {
                        "65535".to_string()
                    } else {
                        format!("65535.{}", etype.len())
                    };
                    etype.push((key, lens.to_string()));
                }
                let owned = sorted(&etype);
                return Some(sony_etype_answer(
                    &owned,
                    &lens_type,
                    focal_length,
                    max_aperture,
                    lens_model,
                ));
            }
        } else if n != 65280.0 {
            // Metabones and the like add a fixed amount to the high byte of a
            // two-byte Canon LensType; Sigma's MC-11 adds 0x4900 to a Sigma one.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let raw = n as i64;
            let high = raw & 0xff00;
            if get(tables::METABONESID, &high.to_string()).is_some() {
                let sub = if raw >= 0xef00 {
                    0xef00
                } else if raw >= 0xbc00 {
                    0xbc00
                } else {
                    0x7700
                };
                lens_type = (raw - sub).to_string();
                table = tables::CANONLENSTYPES;
                if let Some(l) = get(table, &lens_type) {
                    lens_type_prt = l.to_string();
                }
            } else if (0x4900..=0x590a).contains(&raw) {
                lens_type = (raw - 0x4900).to_string();
                table = tables::SIGMALENSTYPESFULL;
                if let Some(l) = get(table, &lens_type) {
                    lens_type_prt = l.to_string();
                }
            }
        }
    } else if short_focal != 0.0
        && long_focal != 0.0
        && !lens_model.is_some_and(|m| {
            Regex::new(r"^TAMRON.*-\d+mm")
                .ok()
                .is_some_and(|re| re.is_match(m))
        })
    {
        // Canon, and the makers that write the same fields, say what focal
        // range the lens reported, which tells the candidates apart.
        return Some(canon_print_lens_id(
            table,
            is_canon_table,
            &lens_type,
            short_focal,
            long_focal,
            max_aperture,
            lens_model,
        ));
    }

    Some(narrow_by_focal(
        table,
        &lens_type,
        &lens_type_prt,
        focal_length,
        max_aperture,
        lens_model,
    ))
}

/// The generic narrowing: rule each candidate out by focal length, then pick
/// the one whose maximum aperture at this focal length is closest.
fn narrow_by_focal(
    table: &[(&str, &str)],
    lens_type: &str,
    lens_type_prt: &str,
    focal_length: f64,
    max_aperture: f64,
    lens_model: Option<&str>,
) -> String {
    let Some(named) = get(table, lens_type) else {
        return lens_model
            .filter(|m| !m.is_empty())
            .map_or_else(|| lens_type_prt.to_string(), ToString::to_string);
    };
    if get(table, &format!("{lens_type}.1")).is_none() {
        return named.to_string();
    }
    let lenses = alternatives(table, lens_type);
    sony_etype_narrow(&lenses, named, focal_length, max_aperture, lens_model)
}

/// The same narrowing over a list of candidates built in memory.
fn sony_etype_answer(
    table: &[(&str, &str)],
    lens_type: &str,
    focal_length: f64,
    max_aperture: f64,
    lens_model: Option<&str>,
) -> String {
    let named = get(table, lens_type).unwrap_or("");
    let mut lenses = vec![named.split(" or ").next().unwrap_or(named).to_string()];
    let mut i = 1;
    while let Some(l) = get(table, &format!("{lens_type}.{i}")) {
        lenses.push(l.to_string());
        i += 1;
    }
    sony_etype_narrow(&lenses, named, focal_length, max_aperture, lens_model)
}

fn sony_etype_narrow(
    lenses: &[String],
    named: &str,
    focal_length: f64,
    max_aperture: f64,
    lens_model: Option<&str>,
) -> String {
    static TELECONV: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r" \+ .*? (\d+(\.\d+)?)x( |$)").expect("static pattern"));
    let mut matches: Vec<String> = Vec::new();
    let mut best: Vec<String> = Vec::new();
    let mut diff: Option<f64> = None;
    for lens in lenses {
        let Some((mut sf, mut lf, mut sa, mut la)) = get_lens_info(lens) else {
            continue;
        };
        if sf == 0.0 {
            continue;
        }
        // A teleconverter in the name multiplies both ends.
        if let Some(c) = TELECONV.captures(lens) {
            if let Ok(f) = c[1].parse::<f64>() {
                sf *= f;
                lf *= f;
                sa *= f;
                la *= f;
            }
        }
        if focal_length != 0.0 && (focal_length < sf - 0.5 || focal_length > lf + 0.5) {
            continue;
        }
        if max_aperture != 0.0 {
            if max_aperture < sa - 0.15 || max_aperture > la + 0.15 {
                continue;
            }
            // Makers report the maximum aperture at the focal length in use,
            // so the closest one wins. Between the ends it varies as a
            // log-log line.
            let aa = if sf == lf || sa == la || focal_length <= sf {
                sa
            } else if focal_length >= lf {
                la
            } else {
                (sa.ln() + (la.ln() - sa.ln()) / (lf.ln() - sf.ln()) * (focal_length.ln() - sf.ln()))
                    .exp()
            };
            let d = (max_aperture - aa).abs();
            if let Some(prev) = diff {
                if d > prev + 0.15 {
                    continue;
                }
                if d < prev - 0.15 {
                    best.clear();
                }
            }
            diff = Some(d);
            best.push(lens.clone());
        }
        matches.push(lens.clone());
    }
    if best.is_empty() {
        best = matches;
    }
    if !best.is_empty() {
        match_lens_model(&mut best, lens_model);
        return best.join(" or ");
    }
    // Nothing could be ruled in: the entry as written, unless it names
    // several lenses and the file says which.
    if let Some(m) = lens_model.filter(|m| !m.is_empty()) {
        if named.contains(" or ") {
            return m.to_string();
        }
    }
    named.to_string()
}

/// `XMP::PrintLensID`: the LensID Adobe wrote, against the lens table of the
/// maker the file names.
///
/// `id`, `make`, `info` (LensInfo), `focal_length`, `lens_model` and `max_av`
/// are the composite's own values, in that order. `make_tag` and `model_tag`
/// are what the file says, which is what `Exif::PrintLensID` reads.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn xmp_print_lens_id(
    id: &str,
    make: &str,
    info: Option<&str>,
    focal_length: f64,
    lens_model: Option<&str>,
    max_av: f64,
    make_tag: &str,
    model_tag: &str,
) -> String {
    // Pentax changed its name to Ricoh; Olympus writes no XMP LensID at all.
    const MAKERS: &[(&str, &str, &str)] = &[
        ("Canon", "canonLensTypes", ""),
        ("Nikon", "nikonLensIDs", ""),
        ("Pentax", "pentaxLensTypes", "Ricoh"),
        ("Sony", "sonyLensTypes", ""),
        ("Sigma", "sigmaLensTypes", ""),
        ("Samsung", "samsungLensTypes", ""),
        ("Leica", "leicaLensTypes", ""),
    ];
    let lower = make.to_lowercase();
    for (mk, table_name, alt) in MAKERS {
        if !lower.contains(&mk.to_lowercase())
            && !(!alt.is_empty() && lower.contains(&alt.to_lowercase()))
        {
            continue;
        }
        let Some(mut table) = tables::table(table_name) else {
            break;
        };
        // Nikon's replacement table is built here and has to outlive the call.
        let nikon_rows;
        let nikon;
        // ExifTool gives up on the whole loop when the table it names is
        // empty -- which SigmaRaw's is, so a Sigma file gets no lens name.
        if table.is_empty() {
            break;
        }
        let (mut sf, mut lf, mut sa, mut la) = (0.0, 0.0, 0.0, 0.0);
        if let Some(info) = info.filter(|i| !i.is_empty()) {
            let a: Vec<f64> = info
                .split_whitespace()
                .map(|w| if w == "undef" { 0.0 } else { w.parse().unwrap_or(0.0) })
                .collect();
            sf = a.first().copied().unwrap_or(0.0);
            lf = a.get(1).copied().unwrap_or(0.0);
            sa = a.get(2).copied().unwrap_or(0.0);
            la = a.get(3).copied().unwrap_or(0.0);
            // For Sony the LensInfo may belong to another lens entirely: use
            // it only where it agrees with what the file otherwise says.
            let disagrees = (focal_length != 0.0
                && ((sf != 0.0 && focal_length < sf - 0.5)
                    || (lf != 0.0 && focal_length > lf + 0.5)))
                || (max_av != 0.0
                    && ((sa != 0.0 && max_av < sa - 0.15) || (la != 0.0 && max_av > la + 0.15)));
            if *mk == "Sony" && disagrees {
                sf = 0.0;
                lf = 0.0;
                sa = 0.0;
                la = 0.0;
            } else if max_av != 0.0 {
                // The wide-end maximum aperture is a poor stand-in for
                // MaxAperture when the file has a real one -- for every maker,
                // not only Sony: this is the `elsif` of the test above.
                sa = 0.0;
            }
        }
        let _ = la;
        let mut id = id.to_string();
        if *mk == "Pentax" && id.chars().all(|c| c.is_ascii_digit()) && !id.is_empty() {
            // CS4 writes an int16u where ExifTool keys on two int8u.
            let n: u32 = id.parse().unwrap_or(0);
            id = format!("{} {}", (n >> 8) & 0xff, n & 0xff);
        }
        if *mk == "Nikon" {
            // Adobe writes only part of the id; Apple Photos writes it whole,
            // as a number that has to go back to the hex bytes ExifTool keys on.
            let n = numify(&id);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let mut hex = format!("{:X}", n as u64);
            if hex.len() % 2 == 1 {
                hex.insert(0, '0');
            }
            let spaced: Vec<String> = hex
                .as_bytes()
                .chunks(2)
                .map(|c| String::from_utf8_lossy(c).to_string())
                .collect();
            id = spaced.join(" ");
            // Every lens whose id starts with these bytes, in table order and
            // without repeating a name.
            let mut used: Vec<&str> = Vec::new();
            let mut conv: Vec<(String, String)> = Vec::new();
            // (the rows are held outside this block, so the table can be used
            // after it)
            for (k, v) in table {
                if !k.starts_with(&id) {
                    continue;
                }
                if used.contains(v) {
                    continue;
                }
                used.push(v);
                let key = if conv.is_empty() {
                    id.clone()
                } else {
                    format!("{}.{}", id, conv.len())
                };
                conv.push((key, (*v).to_string()));
            }
            nikon_rows = conv;
            nikon = sorted(&nikon_rows);
            table = &nikon;
        }
        // `$str = $$printConv{$id} || "Unknown ($id)"`, then the narrowing
        // that knows about focal length, teleconverters and adapters.
        let prt = get(table, &id).map_or_else(|| format!("Unknown ({id})"), ToString::to_string);
        return exif_print_lens_id(
            make_tag,
            model_tag,
            table,
            *mk == "Canon",
            &prt,
            &id,
            focal_length,
            sa,
            max_av,
            sf,
            lf,
            lens_model,
        )
        .unwrap_or(prt);
    }
    format!("Unknown ({id})")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Case for case against Perl's own `XMP::PrintLensID`: every answer
    /// below is what `Image::ExifTool::XMP::PrintLensID` returned for those
    /// arguments.
    ///
    /// The Nikon cases here are the ones whose answer names a single lens.
    /// Where several lenses share the leading bytes of a Nikon id, ExifTool
    /// builds its shortlist from `keys %$printConv` -- Perl's hash order,
    /// which is randomised per process -- so its own answer changes from one
    /// run to the next: over 1430 generated cases, 150 to 166 of them differ
    /// between two runs of ExifTool. This crate walks the table in key order,
    /// which is one of the orders ExifTool can produce and the only one that
    /// gives the same answer twice.
    #[test]
    fn matches_perl() {
        // id, make, info, focal, lensModel, maxAv, Make, Model => answer
        type Case = (&'static str, &'static str, &'static str, f64, &'static str, f64,
                     &'static str, &'static str, &'static str);
        let cases: &[Case] = &[
            ("4", "Canon", "35 105 3.5 4.5", 50.0, "", 4.5, "Canon", "Canon EOS 5D",
             "Canon EF 35-105mm f/3.5-4.5"),
            ("2", "Canon", "", 0.0, "", 0.0, "Canon", "Canon EOS 5D",
             "Canon EF 28mm f/2.8 or Sigma 24mm f/2.8 Super Wide II"),
            ("61182", "Canon", "24 105 4 4", 50.0, "EF24-105mm f/4L IS USM", 4.0, "Canon", "Canon EOS R5",
             "Canon RF 24-105mm F4L IS USM"),
            ("65535", "SONY", "16 50 3.5 5.6", 16.0, "", 3.5, "SONY", "ILCE-6000",
             "Sony E PZ 16-50mm F3.5-5.6 OSS or Sony FE 16mm F3.5 Fisheye (SEL28F20 + SEL057FEC) or Sony E PZ 16-50mm F3.5-5.6 OSS II or Sigma 16-300mm F3.5-6.7 DC OS | C"),
            ("49216", "SONY", "", 50.0, "", 2.8, "SONY", "ILCE-7M3",
             "Unknown (49216)"),
            ("259", "NIKON CORPORATION", "18 200 3.5 5.6", 50.0, "", 4.2, "NIKON CORPORATION", "NIKON D700",
             "Unknown (01 03) 18-200mm"),
            ("254", "PENTAX", "", 50.0, "", 2.8, "PENTAX", "PENTAX K-3",
             "Unknown (0 254)"),
            ("7", "Samsung", "", 30.0, "", 2.0, "SAMSUNG", "NX300",
             "Samsung NX 60mm F2.8 Macro ED OIS SSA"),
            ("23", "LEICA", "", 25.0, "", 1.4, "LEICA", "D-LUX",
             "Summicron-M 50mm f/2 (III)"),
            ("99", "Sigma", "", 50.0, "", 1.4, "SIGMA", "SD1",
             "Unknown (99)"),
            ("1", "Olympus", "", 50.0, "", 2.0, "OLYMPUS", "E-M1",
             "Unknown (1)"),
            ("4", "Canon", "35 105 3.5 4.5", 0.0, "", 0.0, "Canon", "Canon EOS 5D",
             "Canon EF 35-105mm f/3.5-4.5"),
            ("147", "Canon", "100 400 4.5 5.6", 400.0, "EF100-400mm f/4.5-5.6L IS II USM", 5.6, "Canon", "Canon EOS 7D",
             "Canon EF 35-135mm f/4-5.6 USM"),
            ("32768", "NIKON CORPORATION", "", 50.0, "", 1.8, "NIKON CORPORATION", "NIKON Z 7",
             "Unknown (80 00)"),
            ("2", "Canon", "28 28 2.8 2.8", 28.0, "", 2.8, "Canon", "Canon EOS 5D",
             "Canon EF 28mm f/2.8"),
            ("22", "Canon", "20 35 2.8 2.8", 20.0, "", 2.8, "Canon", "Canon EOS 5D",
             "Canon EF 20-35mm f/2.8L"),
            ("10", "Canon", "50 50 2.5 2.5", 50.0, "", 2.5, "Canon", "Canon EOS 5D",
             "Canon EF 50mm f/2.5 Macro"),
            ("65535", "SONY", "", 0.0, "", 1.0, "SONY", "ILCE-7M3",
             "E-Mount, T-Mount, Other Lens or no lens"),
            ("30722", "SONY", "", 50.0, "", 1.8, "SONY", "ILCE-7M3",
             "Unknown (30722)"),
            ("18688", "SONY", "", 50.0, "", 1.4, "SONY", "ILCE-7M3",
             "Sigma MC-11 SA-E Mount Converter with not-supported Sigma lens"),
            ("6", "Canon", "28 70 3.5 4.5", 28.0, "", 3.5, "Canon", "Canon EOS 5D",
             "Canon EF 28-70mm f/3.5-4.5"),
            ("8", "Canon", "100 300 5.6 5.6", 100.0, "", 5.6, "Canon", "Canon EOS 5D",
             "Canon EF 100-300mm f/5.6"),
            ("511", "NIKON CORPORATION", "", 50.0, "", 1.4, "NIKON CORPORATION", "NIKON D850",
             "Unknown (01 FF)"),
            ("161", "Canon", "24 70 2.8 2.8", 24.0, "", 2.8, "Canon", "Canon EOS 5D",
             "Sigma 24-70mm f/2.8 EX or Tokina AT-X 24-70mm f/2.8 PRO FX (IF)"),
            ("4", "Ricoh", "", 50.0, "", 4.5, "RICOH", "PENTAX K-1",
             "Unknown (0 4)"),
            ("171", "Canon", "undef undef 2.8 undef", 16.0, "24-70mm F2.8 DG HSM | A", 5.6, "Canon", "Canon EOS 5D",
             "Canon EF 300mm f/4L USM"),
            ("25851", "SONY", "", 50.0, "TAMRON 28-75mm f/2.8", 2.0, "SONY", "DSLR-A900",
             "Beroflex 35-135mm F3.5-4.5"),
            ("11 4", "Pentax", "24 105 4 4", 200.0, "AF-S NIKKOR 50mm f/1.8G", 0.0, "PENTAX", "PENTAX K-3",
             "smc PENTAX-FA 645 45-85mm F4.5"),
            ("2667", "SONY", "18 200 3.5 5.6", 16.0, "", 6.3, "SONY", "NEX-5",
             "Minolta AF 35mm F2 New"),
            ("43", "Canon", "18 200 3.5 5.6", 200.0, "smc PENTAX-DA 18-55mm F3.5-5.6", 1.0, "Canon", "Canon EOS 5D",
             "Canon EF 28-105mm f/4-5.6"),
            ("4 21", "Pentax", "", 400.0, "smc PENTAX-DA 18-55mm F3.5-5.6", 3.5, "PENTAX", "PENTAX K-3",
             "Cosina AF 100-300mm F5.6-6.7"),
            ("8 210", "Pentax", "28 300 3.5 6.3", 0.0, "RF100-500mm F4.5-7.1 L IS USM + RF1.4x", 1.0, "RICOH", "PENTAX K-1",
             "smc PENTAX-DA 18-270mm F3.5-6.3 ED SDM"),
            ("4 12", "Pentax", "18 200 3.5 5.6", 50.0, "EF24-105mm f/4L IS USM", 6.3, "RICOH", "PENTAX K-1",
             "smc PENTAX-FA 50mm F1.4"),
            ("138", "SONY", "28 300 3.5 6.3", 0.0, "EF24-105mm f/4L IS USM", 4.0, "SONY", "DSLR-A900",
             "Soligor 19-35mm F3.5-4.5"),
            ("7 241", "Pentax", "", 50.0, "EF24-105mm f/4L IS USM", 1.0, "RICOH", "PENTAX K-1",
             "smc PENTAX-DA* 50-135mm F2.8 ED [IF] SDM (SDM unused)"),
            ("26071", "SONY", "undef undef 2.8 undef", 400.0, "EF24-105mm f/4L IS USM", 1.0, "SONY", "NEX-5",
             "Minolta AF 35-80mm F4-5.6"),
            ("2572", "SONY", "", 0.0, "smc PENTAX-DA 18-55mm F3.5-5.6", 3.5, "SONY", "ILCE-7M3",
             "Minolta/Sony AF 500mm F8 Reflex"),
            ("224", "SONY", "28 300 3.5 6.3", 16.0, "FE 50mm F1.8", 0.0, "SONY", "ILCE-7M3",
             "Tamron SP 90mm F2.8 Di Macro 1:1 USD (F004)"),
            ("5", "Samsung", "undef undef 2.8 undef", 100.0, "24-70mm F2.8 DG HSM | A", 1.4, "SAMSUNG", "NX300",
             "Samsung NX 20mm F2.8 Pancake"),
            ("2628", "SONY", "35 105 3.5 4.5", 200.0, "RF100-500mm F4.5-7.1 L IS USM + RF1.4x", 6.3, "SONY", "ILCE-7M3",
             "Minolta AF 80-200mm F2.8 HS-APO G"),
            ("4 247", "Pentax", "28 300 3.5 6.3", 200.0, "", 1.4, "RICOH", "PENTAX K-1",
             "smc PENTAX-DA FISH-EYE 10-17mm F3.5-4.5 ED[IF] + 2.8x"),
            ("4148", "Canon", "28 300 3.5 6.3", 0.0, "", 2.8, "Canon", "Canon EOS R5",
             "Canon EF-S 55-250mm f/4-5.6 IS STM"),
            ("214", "Canon", "", 400.0, "24-70mm F2.8 DG HSM | A", 1.4, "Canon", "Canon EOS R5",
             "Canon EF-S 18-55mm f/3.5-5.6 USM"),
            ("11 10", "Pentax", "16 50 3.5 5.6", 35.0, "smc PENTAX-DA 18-55mm F3.5-5.6", 6.3, "PENTAX", "PENTAX K-3",
             "smc PENTAX-FA 645 150mm F2.8 [IF]"),
            ("176", "Canon", "50 50 1.4 1.4", 400.0, "EF70-200mm f/2.8L IS III USM", 6.3, "Canon", "Canon EOS 5D",
             "Canon EF 24-85mm f/3.5-4.5 USM"),
            ("193", "Canon", "18 200 3.5 5.6", 35.0, "EF24-105mm f/4L IS USM", 3.5, "Canon", "Canon EOS 5D",
             "Canon EF 35-80mm f/4-5.6 USM"),
            ("24", "SONY", "70 200 2.8 2.8", 100.0, "EF24-105mm f/4L IS USM", 1.0, "SONY", "NEX-5",
             "EF24-105mm f/4L IS USM"),
            ("24", "LEICA", "18 200 3.5 5.6", 24.0, "", 4.5, "LEICA", "D-LUX",
             "Elmarit-M 21mm f/2.8 ASPH."),
            ("148", "Canon", "", 200.0, "EF70-200mm f/2.8L IS III USM", 3.5, "Canon", "Canon EOS 5D",
             "Canon EF 28-80mm f/3.5-5.6 USM"),
            ("57", "SONY", "24 105 4 4", 24.0, "smc PENTAX-DA 18-55mm F3.5-5.6", 1.4, "SONY", "NEX-5",
             "smc PENTAX-DA 18-55mm F3.5-5.6"),
            ("41", "SONY", "24 105 4 4", 100.0, "", 1.0, "SONY", "DSLR-A900",
             "Minolta/Sony AF DT 11-18mm F4.5-5.6 (D) or Tamron Lens"),
            ("214", "SONY", "undef undef 2.8 undef", 50.0, "", 1.0, "SONY", "DSLR-A900",
             "Tamron SP 150-600mm F5-6.3 Di USD"),
            ("4152", "Canon", "24 105 4 4", 400.0, "EF70-200mm f/2.8L IS III USM", 4.5, "Canon", "Canon EOS R5",
             "Canon EF 24-105mm f/3.5-5.6 IS STM"),
            ("45741", "SONY", "24 105 4 4", 16.0, "AF-S NIKKOR 50mm f/1.8G", 1.4, "SONY", "DSLR-A900",
             "AF-S NIKKOR 50mm f/1.8G"),
            ("4145", "Canon", "50 50 1.4 1.4", 16.0, "24-70mm F2.8 DG HSM | A", 5.6, "Canon", "Canon EOS R5",
             "Canon EF-M 22mm f/2 STM"),
            ("35", "Canon", "50 50 1.4 1.4", 24.0, "TAMRON 28-75mm f/2.8", 6.3, "Canon", "Canon EOS 5D",
             "Canon EF 35-80mm f/4-5.6"),
            ("63", "Canon", "28 300 3.5 6.3", 50.0, "AF-S NIKKOR 50mm f/1.8G", 5.6, "Canon", "Canon EOS 5D",
             "Irix 30mm F1.4 Dragonfly"),
            ("3 53", "Pentax", "undef undef 2.8 undef", 300.0, "AF-S NIKKOR 50mm f/1.8G", 2.0, "RICOH", "PENTAX K-1",
             "smc PENTAX-FA 28-80mm F3.5-5.6 AL"),
            ("496", "Canon", "16 50 3.5 5.6", 200.0, "EF70-200mm f/2.8L IS III USM", 5.6, "Canon", "Canon EOS 5D",
             "Canon EF 200-400mm f/4L IS USM"),
            ("5", "SONY", "35 105 3.5 4.5", 50.0, "FE 50mm F1.8", 5.6, "SONY", "DSLR-A900",
             "Minolta AF 35-70mm F3.5-4.5 [II]"),
            ("38", "LEICA", "", 16.0, "EF24-105mm f/4L IS USM", 6.3, "LEICA", "D-LUX",
             "Elmarit-M 90mm f/2.8"),
            ("4 51", "Pentax", "24 105 4 4", 0.0, "RF100-500mm F4.5-7.1 L IS USM + RF1.4x", 2.8, "RICOH", "PENTAX K-1",
             "smc PENTAX-D FA 50mm F2.8 Macro"),
            ("508", "Canon", "18 200 3.5 5.6", 16.0, "smc PENTAX-DA 18-55mm F3.5-5.6", 6.3, "Canon", "Canon EOS R5",
             "Unknown (508) 18-200mm"),
            ("9 0", "LEICA", "28 300 3.5 6.3", 100.0, "FE 50mm F1.8", 2.0, "LEICA", "D-LUX",
             "Apo-Telyt-M 135mm f/3.4"),
            ("7 235", "Pentax", "", 35.0, "AF-S NIKKOR 50mm f/1.8G", 6.3, "RICOH", "PENTAX K-1",
             "smc PENTAX-DA* 200mm F2.8 ED [IF] SDM (SDM unused)"),
            ("25", "SONY", "35 105 3.5 4.5", 100.0, "FE 50mm F1.8", 6.3, "SONY", "ILCE-7M3",
             "FE 50mm F1.8"),
            ("38", "SONY", "24 105 4 4", 300.0, "EF24-105mm f/4L IS USM", 2.8, "SONY", "NEX-5",
             "Minolta AF 17-35mm F2.8-4 (D)"),
            ("11 1", "Pentax", "50 50 1.4 1.4", 200.0, "AF-S NIKKOR 50mm f/1.8G", 4.0, "RICOH", "PENTAX K-1",
             "smc PENTAX-FA 645 75mm F2.8"),
            ("7 233", "Pentax", "50 50 1.4 1.4", 50.0, "RF100-500mm F4.5-7.1 L IS USM + RF1.4x", 1.4, "PENTAX", "PENTAX K-3",
             "smc PENTAX-DA 35mm F2.8 Macro Limited"),
            ("212", "SONY", "18 200 3.5 5.6", 100.0, "", 5.6, "SONY", "DSLR-A900",
             "Tamron 28-300mm F3.5-6.3 Di PZD"),
            ("7 204", "Pentax", "35 105 3.5 4.5", 100.0, "TAMRON 28-75mm f/2.8", 2.8, "PENTAX", "PENTAX K-3",
             "HD PENTAX-DA 15mm F4 ED AL Limited"),
            ("8 20", "Pentax", "", 16.0, "RF100-500mm F4.5-7.1 L IS USM + RF1.4x", 4.0, "RICOH", "PENTAX K-1",
             "Sigma 18-50mm F2.8-4.5 DC HSM"),
            ("26211", "SONY", "undef undef 2.8 undef", 200.0, "EF70-200mm f/2.8L IS III USM", 2.0, "SONY", "ILCE-7M3",
             "Minolta AF 100-300mm F4.5-5.6 xi"),
            ("2639", "SONY", "24 105 4 4", 24.0, "RF100-500mm F4.5-7.1 L IS USM + RF1.4x", 2.0, "SONY", "DSLR-A900",
             "Minolta AF 100mm F2.8 Macro"),
            ("26291", "SONY", "70 200 2.8 2.8", 16.0, "AF-S NIKKOR 50mm f/1.8G", 0.0, "SONY", "NEX-5",
             "Minolta AF 85mm F1.4 New"),
            ("4 16", "Pentax", "50 50 1.4 1.4", 200.0, "", 1.4, "RICOH", "PENTAX K-1",
             "Tamron AF 80-210mm F4-5.6 (178D)"),
            ("60", "SONY", "50 50 1.4 1.4", 24.0, "TAMRON 28-75mm f/2.8", 4.5, "SONY", "DSLR-A900",
             "Carl Zeiss Distagon T* 24mm F2 ZA SSM (SAL24F20Z)"),
            ("8 242", "Pentax", "35 105 3.5 4.5", 24.0, "RF100-500mm F4.5-7.1 L IS USM + RF1.4x", 4.5, "PENTAX", "PENTAX K-3",
             "smc PENTAX-DA* 16-50mm F2.8 ED AL [IF] SDM"),
            ("7 231", "Pentax", "18 200 3.5 5.6", 0.0, "EF70-200mm f/2.8L IS III USM", 2.0, "RICOH", "PENTAX K-1",
             "smc PENTAX-DA 18-250mm F3.5-6.3 ED AL [IF]"),
            ("2563", "SONY", "50 50 1.4 1.4", 0.0, "TAMRON 28-75mm f/2.8", 5.6, "SONY", "ILCE-7M3",
             "Sigma 400mm F5.6 APO"),
            ("2608", "SONY", "", 100.0, "EF70-200mm f/2.8L IS III USM", 6.3, "SONY", "ILCE-7M3",
             "Minolta AF 300mm F2.8 HS-APO G"),
            ("4 1", "Pentax", "35 105 3.5 4.5", 50.0, "FE 50mm F1.8", 1.0, "PENTAX", "PENTAX K-3",
             "smc PENTAX-FA SOFT 28mm F2.8"),
            ("4144", "Canon", "18 200 3.5 5.6", 35.0, "RF100-500mm F4.5-7.1 L IS USM + RF1.4x", 5.6, "Canon", "Canon EOS R5",
             "Canon EF 40mm f/2.8 STM"),
            ("26721", "SONY", "35 105 3.5 4.5", 0.0, "", 4.5, "SONY", "ILCE-7M3",
             "Minolta AF 24-105mm F3.5-4.5 (D)"),
            ("54", "SONY", "18 200 3.5 5.6", 50.0, "EF70-200mm f/2.8L IS III USM", 2.8, "SONY", "ILCE-7M3",
             "EF70-200mm f/2.8L IS III USM"),
            ("31", "SONY", "undef undef 2.8 undef", 24.0, "AF-S NIKKOR 50mm f/1.8G", 2.8, "SONY", "DSLR-A900",
             "AF-S NIKKOR 50mm f/1.8G"),
            ("11", "Samsung", "24 105 4 4", 0.0, "AF-S NIKKOR 50mm f/1.8G", 0.0, "SAMSUNG", "NX300",
             "Samsung NX 45mm F1.8 2D/3D"),
            ("33", "LEICA", "70 200 2.8 2.8", 100.0, "EF70-200mm f/2.8L IS III USM", 2.8, "LEICA", "D-LUX",
             "Summicron-M 50mm f/2 (IV, V) + 1.4x"),
            ("6528", "SONY", "50 50 1.4 1.4", 50.0, "EF70-200mm f/2.8L IS III USM", 6.3, "SONY", "ILCE-7M3",
             "Sigma 16mm F2.8 Filtermatic Fisheye"),
            ("26421", "SONY", "28 300 3.5 6.3", 100.0, "FE 50mm F1.8", 1.4, "SONY", "ILCE-7M3",
             "Minolta AF 24mm F2.8 New"),
            ("3 19", "Pentax", "35 105 3.5 4.5", 100.0, "", 4.5, "PENTAX", "PENTAX K-3",
             "smc PENTAX-F 24-50mm F4"),
            ("217", "SONY", "35 105 3.5 4.5", 16.0, "EF24-105mm f/4L IS USM", 0.0, "SONY", "ILCE-7M3",
             "Tamron SP 35mm F1.8 Di USD"),
            ("1", "LEICA", "undef undef 2.8 undef", 35.0, "TAMRON 28-75mm f/2.8", 3.5, "LEICA", "D-LUX",
             "Elmarit-M 21mm f/2.8"),
            ("4 49", "Pentax", "18 200 3.5 5.6", 300.0, "", 6.3, "PENTAX", "PENTAX K-3",
             "Tamron SP AF 28-75mm F2.8 XR Di LD Aspherical [IF] Macro"),
            ("170", "Canon", "16 50 3.5 5.6", 24.0, "AF-S NIKKOR 50mm f/1.8G", 4.5, "Canon", "Canon EOS R5",
             "Unknown (170) 16-50mm"),
            ("2604", "SONY", "18 200 3.5 5.6", 24.0, "FE 50mm F1.8", 0.0, "SONY", "NEX-5",
             "Minolta AF 80-200mm F4.5-5.6"),
            ("2629", "SONY", "35 105 3.5 4.5", 400.0, "TAMRON 28-75mm f/2.8", 0.0, "SONY", "ILCE-7M3",
             "Minolta AF 85mm F1.4 New"),
            ("25721", "SONY", "24 105 4 4", 100.0, "RF100-500mm F4.5-7.1 L IS USM + RF1.4x", 4.0, "SONY", "DSLR-A900",
             "Minolta/Sony AF 500mm F8 Reflex"),
            ("505", "Canon", "28 300 3.5 6.3", 100.0, "AF-S NIKKOR 50mm f/1.8G", 3.5, "Canon", "Canon EOS R5",
             "Canon EF 35mm f/2 IS USM"),
            ("1 0", "Pentax", "70 200 2.8 2.8", 16.0, "smc PENTAX-DA 18-55mm F3.5-5.6", 1.0, "RICOH", "PENTAX K-1",
             "K or M Lens"),
            ("FE 47 00 00 24 24 4B 06", "NIKON CORPORATION", "35 105 3.5 4.5", 24.0, "EF24-105mm f/4L IS USM", 0.0, "NIKON CORPORATION", "NIKON Z 9",
             "Tokina AT-X M35 PRO DX (AF 35mm f/2.8 Macro)"),
            ("26 40 2D 50 2C 3C 1C 06", "NIKON CORPORATION", "50 50 1.4 1.4", 400.0, "TAMRON 28-75mm f/2.8", 4.0, "NIKON CORPORATION", "NIKON Z 9",
             "AF Nikkor 35mm f/2"),
            ("26 3C 54 80 30 3C 1C 06", "NIKON CORPORATION", "16 50 3.5 5.6", 100.0, "FE 50mm F1.8", 6.3, "NIKON CORPORATION", "NIKON D850",
             "AF Nikkor 35mm f/2"),
            ("35 3C A0 A0 30 30 33 02", "NIKON CORPORATION", "undef undef 2.8 undef", 35.0, "smc PENTAX-DA 18-55mm F3.5-5.6", 3.5, "NIKON CORPORATION", "NIKON D850",
             "Zoom-Nikkor 1200-1700mm f/5.6-8 P ED IF"),
            ("4A 48 1E 1E 24 0C 4D 02", "NIKON CORPORATION", "28 300 3.5 6.3", 35.0, "", 3.5, "NIKON CORPORATION", "NIKON D850",
             "AF Nikkor 28mm f/2.8"),
            ("7A 3B 53 80 30 3C 4B 06", "NIKON CORPORATION", "70 200 2.8 2.8", 50.0, "", 5.6, "NIKON CORPORATION", "NIKON D850",
             "Unknown (07) 70-200mm"),
            ("79 48 3C 5C 24 24 1C 06", "NIKON CORPORATION", "28 300 3.5 6.3", 24.0, "EF70-200mm f/2.8L IS III USM", 0.0, "NIKON CORPORATION", "NIKON Z 9",
             "IX-Nikkor 24-70mm f/3.5-5.6"),
            ("79 54 31 31 0C 0C 4B 06", "NIKON CORPORATION", "18 200 3.5 5.6", 24.0, "24-70mm F2.8 DG HSM | A", 1.4, "NIKON CORPORATION", "NIKON Z 9",
             "IX-Nikkor 24-70mm f/3.5-5.6"),
            ("53 48 60 80 24 24 60 02", "NIKON CORPORATION", "28 300 3.5 6.3", 200.0, "RF100-500mm F4.5-7.1 L IS USM + RF1.4x", 1.0, "NIKON CORPORATION", "NIKON D850",
             "Unknown (35) 28-300mm"),
            ("9F 54 68 68 18 18 A2 06", "NIKON CORPORATION", "35 105 3.5 4.5", 24.0, "TAMRON 28-75mm f/2.8", 6.3, "NIKON CORPORATION", "NIKON D850",
             "AF Nikkor 24mm f/2.8"),
            ("9D 48 2B 50 24 24 4B 0E", "NIKON CORPORATION", "35 105 3.5 4.5", 200.0, "AF-S NIKKOR 50mm f/1.8G", 0.0, "NIKON CORPORATION", "NIKON D850",
             "AF Nikkor 24mm f/2.8"),
            ("8B 40 2D 80 2C 3C 8D 0E", "NIKON CORPORATION", "18 200 3.5 5.6", 16.0, "EF70-200mm f/2.8L IS III USM", 1.0, "NIKON CORPORATION", "NIKON Z 9",
             "AF Zoom-Nikkor 35-105mm f/3.5-4.5"),
            ("4B 3C A0 A0 30 30 E1 02", "NIKON CORPORATION", "", 35.0, "TAMRON 28-75mm f/2.8", 4.0, "NIKON CORPORATION", "NIKON D850",
             "AF Nikkor 28mm f/2.8"),
            ("9B 54 62 62 0C 0C 4B 06", "NIKON CORPORATION", "18 200 3.5 5.6", 0.0, "TAMRON 28-75mm f/2.8", 6.3, "NIKON CORPORATION", "NIKON D850",
             "AF Nikkor 24mm f/2.8"),
            ("9F 48 48 48 24 24 A1 06", "NIKON CORPORATION", "24 105 4 4", 24.0, "RF100-500mm F4.5-7.1 L IS USM + RF1.4x", 3.5, "NIKON CORPORATION", "NIKON Z 9",
             "AF Nikkor 24mm f/2.8"),
        ];
        for (id, make, info, fl, lm, mav, mk, md, want) in cases {
            let got = xmp_print_lens_id(
                id, make,
                if info.is_empty() { None } else { Some(info) },
                *fl,
                if lm.is_empty() { None } else { Some(lm) },
                *mav, mk, md,
            );
            assert_eq!(&got, want, "id {id} make {make}");
        }
    }
}
