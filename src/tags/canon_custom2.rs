//! The interpreter behind `canon_custom2_generated`: Canon's second-generation
//! custom functions (`%CanonCustom::Functions2`, read by ProcessCanonCustom2).
//!
//! Each custom function is a tag id holding a list of int32s. ExifTool takes
//! the first variant of the tag whose Condition holds, then converts the
//! values with that variant's ValueConv and PrintConv; a tag none of whose
//! variants holds is not reported.

use std::sync::LazyLock;

use regex_lite::Regex;

use super::canon_custom2_generated::FUNCTIONS2;
use super::conv_expr::{self, Val};

/// A variant's Condition.
pub(crate) enum Cond {
    Always,
    /// `$count == N`: the number of values stored.
    Count(u32),
    /// `$$self{Model} =~ /.../` (or `!~` when the flag is set), any one of
    /// them holding.
    Model(&'static [(&'static LazyLock<Regex>, bool)]),
}

/// How one value, or the whole list, is printed.
pub(crate) enum Print {
    /// No conversion: the value as read.
    Raw,
    /// A Perl expression, evaluated by `tags::conv_expr`.
    Expr(&'static str),
    /// A PrintConv hash, keyed by the value's text, with the `BITMASK` names
    /// it falls back to.
    Map(
        &'static [(&'static str, &'static str)],
        Option<&'static [(u32, &'static str)]>,
    ),
}

/// A variant's PrintConv: one for the whole value, or one per element.
pub(crate) enum Conv {
    One(Print),
    List(&'static [Print]),
}

/// One variant of a custom function.
pub(crate) struct Variant {
    pub(crate) name: &'static str,
    pub(crate) cond: Cond,
    /// A ValueConv list, applied element by element (`None` leaves one as it is).
    pub(crate) value: Option<&'static [Option<&'static str>]>,
    pub(crate) print: Conv,
}

impl Cond {
    fn holds(&self, model: &str, count: usize) -> bool {
        match self {
            Cond::Always => true,
            Cond::Count(n) => count == *n as usize,
            Cond::Model(alts) => alts.iter().any(|(re, neg)| re.is_match(model) != *neg),
        }
    }
}

/// ExifTool's DecodeBits over 32-bit words.
fn decode_bits(val: &str, names: &[(u32, &str)]) -> String {
    let mut bits = Vec::new();
    for (word, v) in val.split_whitespace().enumerate() {
        let v: i64 = v.parse().unwrap_or(0);
        for i in 0..32u32 {
            if v & (1 << i) != 0 {
                let n = i + 32 * word as u32;
                bits.push(
                    names
                        .iter()
                        .find(|(k, _)| *k == n)
                        .map_or_else(|| format!("[{n}]"), |(_, s)| (*s).to_string()),
                );
            }
        }
    }
    if bits.is_empty() {
        "(none)".to_string()
    } else {
        bits.join(", ")
    }
}

fn apply(print: &Print, val: &str) -> String {
    match print {
        Print::Raw => val.to_string(),
        Print::Expr(e) => conv_expr::eval(e, &Val::Str(val.to_string()))
            .map_or_else(|| val.to_string(), |v| v.as_string()),
        Print::Map(map, bits) => match map.iter().find(|(k, _)| *k == val) {
            Some((_, s)) => (*s).to_string(),
            None => match bits {
                Some(names) => decode_bits(val, names),
                None => format!("Unknown ({val})"),
            },
        },
    }
}

/// The name and printed value of custom function `tag_id` holding `vals`, or
/// `None` when ExifTool reports nothing for it.
pub(crate) fn decode(tag_id: u32, vals: &[i32], model: &str) -> Option<(&'static str, String)> {
    let variants = FUNCTIONS2
        .binary_search_by_key(&tag_id, |(id, _)| *id)
        .ok()
        .map(|i| FUNCTIONS2[i].1)?;
    let v = variants.iter().find(|v| v.cond.holds(model, vals.len()))?;
    let mut elems: Vec<String> = vals.iter().map(i32::to_string).collect();
    if let Some(vconv) = v.value {
        for (i, e) in elems.iter_mut().enumerate() {
            if let Some(Some(expr)) = vconv.get(i) {
                if let Some(r) = conv_expr::eval(expr, &Val::Str(e.clone())) {
                    *e = r.as_string();
                }
            }
        }
    }
    let printed = match &v.print {
        Conv::One(p) => apply(p, &elems.join(" ")),
        Conv::List(list) => elems
            .iter()
            .enumerate()
            .map(|(i, e)| list.get(i).map_or_else(|| e.clone(), |p| apply(p, e)))
            .collect::<Vec<_>>()
            .join("; "),
    };
    Some((v.name, printed))
}

#[cfg(test)]
mod tests {
    use super::decode;

    #[test]
    fn model_conditions_pick_the_variant() {
        // ExposureLevelIncrements: the 1D variant, then the other models'.
        assert_eq!(
            decode(0x0101, &[0], "Canon EOS-1D Mark III"),
            Some((
                "ExposureLevelIncrements",
                "1/3-stop set, 1/3-stop comp.".into()
            ))
        );
        assert_eq!(
            decode(0x0101, &[0], ""),
            Some(("ExposureLevelIncrements", "1/3 Stop".into()))
        );
    }

    #[test]
    fn count_conditions_and_conversion_lists() {
        // AEBShotCount stored as one value, whatever the model.
        assert_eq!(
            decode(0x0106, &[3], ""),
            Some(("AEBShotCount", "7 shots".into()))
        );
        // AFMicroadjustment: a PrintConv list, joined with "; ".
        assert_eq!(
            decode(0x0507, &[0, 0, 0, 0, 0], ""),
            Some(("AFMicroadjustment", "Disable; 0; 0; 0; 0".into()))
        );
    }
}
