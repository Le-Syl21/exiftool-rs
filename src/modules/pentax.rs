//! Image::ExifTool::Pentax (Image/ExifTool/Pentax.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, n, re, ret};

const MODULE: &str = "Image::ExifTool::Pentax";

fn f(x: f64) -> Value {
    Value::Num(x)
}

/// `sort @list` (string order).
fn sort_str(v: &mut [Value]) {
    v.sort_by(|a, b| a.to_bytes()[..].cmp(&b.to_bytes()[..]));
}

//------------------------------------------------------------------------------
/// Convert filter settings (ref PH, K-5)
/// Inputs: 0) value to convert, 1) flag for inverse conversion, 2) lookup table
/// Returns: converted value
pub fn print_filter(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (val, inv, conv) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let mut vals: Vec<Value>;
    let mut cval: Vec<Value> = Vec::new();
    let (mut t, mut v);
    let filter_settings = crate::runtime::lex_hash(it, MODULE, "filterSettings");

    if !inv.truthy() {
        // forward conversion (reading):
        vals = pl::split_ws(&val);
        t = if vals.is_empty() { Value::Undef } else { vals.remove(0) };
        let c = conv.get(t.to_bytes());
        cval.push(if c.truthy() { c } else { pl::concat(&pl::concat(&pl::s("Unknown ("), &t), &pl::s(")")) });
        while !vals.is_empty() {
            t = vals.remove(0);
            v = if vals.is_empty() { Value::Undef } else { vals.remove(0) };
            if !t.truthy() {
                continue;
            }
            if v.is_undef() {
                break;
            }
            let c = filter_settings.get(t.to_bytes());
            if c.truthy() {
                let c1 = c.at(1);
                if c1.is_ref() {
                    let x = c1.get(v.to_bytes());
                    v = if x.truthy() { x } else { pl::concat(&pl::concat(&pl::s("Unknown("), &v), &pl::s(")")) };
                } else if v.truthy() {
                    v = pl::sprintf_v(&c1, &[v]);
                }
                cval.push(pl::concat(&pl::concat(&c.at(0), &pl::s("=")), &v));
            } else {
                cval.push(pl::concat(&pl::concat(&pl::concat(&pl::s("Unknown("), &t), &pl::s(")=")), &v));
            }
        }
        if cval.is_empty() {
            pl::ret_undef()
        } else {
            ret(Value::new_array(cval))
        }
    } else {
        // reverse conversion (writing):
        vals = pl::split(&re(r",\s*", ""), &val, 0);
        // convert filter name
        conv.delete("OTHER"); // avoid recursion
        let first = if vals.is_empty() { Value::Undef } else { vals.remove(0) };
        v = pl::call(it, "Image::ExifTool::ReverseLookup", vec![first, conv.clone()])?;
        conv.set("OTHER", Value::Code(it.sub_ref("Image::ExifTool::Pentax::PrintFilter")));
        if v.is_undef() {
            return pl::ret_undef();
        }
        cval.push(v);
        // generate a lookup table for the filter setting name
        let setting_names = Value::new_hash();
        for k in filter_settings.keys() {
            setting_names.set(&k, filter_settings.get(&k).at(0));
        }
        // convert filter settings
        for v in vals.iter_mut() {
            let Some(m) = re(r"^(.*)=(.*)$", "").m(v) else { return pl::ret_undef() };
            t = cap(&m, 1);
            *v = cap(&m, 2);
            // look up settings name
            t = pl::call(it, "Image::ExifTool::ReverseLookup", vec![t, setting_names.clone()])?;
            if t.is_undef() {
                return pl::ret_undef();
            }
            let fs1 = filter_settings.get(t.to_bytes()).at(1);
            if fs1.is_ref() {
                // look up settings value
                *v = pl::call(it, "Image::ExifTool::ReverseLookup", vec![v.clone(), fs1])?;
                if v.is_undef() {
                    return pl::ret_undef();
                }
            } else if !pl::call(it, "Image::ExifTool::IsInt", vec![v.clone()])?.truthy() {
                return pl::ret_undef();
            }
            cval.push(t.clone());
            cval.push(v.clone());
        }
        while cval.len() < 17 {
            cval.push(n(0)); // pad with zeros if necessary
        }
        ret(pl::join(" ", &cval))
    }
}

//------------------------------------------------------------------------------
/// Decode AF bit mask (ref 29)
/// Inputs: 0) raw binary value, 1) number of AF points, 2) number of bits per AF point,
///         3) bit mask, 4) bit value (undef for any bit set)
pub fn decode_af_points(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (val, num, bits, mask, bit_val) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4));
    let mut bytes = pl::split_ws(&val);
    if bytes.is_empty() {
        return ret(pl::s("(none)"));
    }
    let mut i = n(1); // (starts at AF point number 1)
    let mut shift = pl::sub(&n(8), &bits);
    let mut byte = bytes.remove(0);
    let mut bit_list: Vec<Value> = Vec::new();
    loop {
        if bit_val.truthy() {
            if pl::num_eq(&ops::bit_and(&ops::shift_right(&byte, &shift), &mask), &bit_val) {
                bit_list.push(i.clone());
            }
        } else if ops::bit_and(&ops::shift_right(&byte, &shift), &mask).truthy() {
            bit_list.push(i.clone());
        }
        i = pl::add(&i, &n(1));
        if pl::gt(&i, &num) {
            break;
        }
        shift = pl::sub(&shift, &bits);
        if pl::lt(&shift, &n(0)) {
            if bytes.is_empty() {
                break;
            }
            byte = bytes.remove(0);
            shift = pl::add(&shift, &n(8));
        }
    }
    ret(pl::join(",", &bit_list))
}

//------------------------------------------------------------------------------
/// Print K-3III AF Point names (ref PH)
/// Inputs: 0) value, 1) ExifTool ref, 2) optional value to match
pub fn af_point_names_k3iii(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let av = pl::split_ws(&arg(&a, 0));
    let match_ = arg(&a, 2);
    let k3iii_af = crate::runtime::lex_array(it, MODULE, "k3iiiAF");
    let mut pts: Vec<Value> = Vec::new();
    let name = |i: usize| {
        let p = k3iii_af.at(i as i64);
        if p.truthy() {
            p
        } else {
            pl::s(&format!("Unknown({i})"))
        }
    };
    if match_.truthy() {
        for (i, x) in av.iter().enumerate() {
            if pl::num_eq(x, &match_) {
                pts.push(name(i));
            }
        }
    } else {
        for (i, x) in av.iter().enumerate() {
            if x.truthy() {
                pts.push(name(i));
            }
        }
    }
    if pts.is_empty() {
        return ret(pl::s("(none)"));
    }
    sort_str(&mut pts);
    ret(pl::join(",", &pts))
}

//------------------------------------------------------------------------------
/// Print K-3III AF point values (ref PH)
/// Inputs: 0) value, 1) ExifTool ref
/// Notes: this is experimental and not well understood
pub fn af_point_values_k3iii(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut av = pl::split_ws(&arg(&a, 0));
    let k3iii_af = crate::runtime::lex_array(it, MODULE, "k3iiiAF");
    let mut vals: Vec<Value> = Vec::new();
    // (I really don't understand why Pentax seemed to use 28 instead of 41 for
    // the first index of the 2nd value, because as it stands there is
    // overlap between the ranges of the 1st and 2nd values, and there is
    // no way to tell which is which for cases were multiple points have values)
    let last = av.len();
    for i in 0..last {
        if !av[i].truthy() {
            continue;
        }
        let k = k3iii_af.at(i as i64);
        let pt = if k.truthy() { pl::concat(&k, &pl::s("=")) } else { pl::concat(&k3iii_af.at(i as i64 - 28), &pl::s("=/")) };
        vals.push(pl::concat(&pt, &av[i]));
        let next = av.get(i + 28).cloned().unwrap_or_default();
        if !next.truthy() {
            continue;
        }
        let lv = vals.len() - 1;
        vals[lv] = pl::concat(&vals[lv], &pl::concat(&pl::s("/"), &next));
        av[i + 28] = Value::Undef;
    }
    if vals.is_empty() {
        return ret(pl::s("(none)"));
    }
    sort_str(&mut vals);
    ret(pl::join(",", &vals))
}

//------------------------------------------------------------------------------
/// Print K-3III AF point positions from tag 0x040c (ref KG)
/// Inputs: 0) raw value, 1) ExifTool ref
pub fn af_areas_k3iii(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let val = arg(&a, 0);
    if !val.truthy() {
        return ret(pl::s("(none)"));
    }
    let vals = pl::split_ws(&val);
    // flags bits: [mask, value, description]
    let flags: [(i64, i64, &str); 3] = [(0x10, 0x10, "central"), (0x08, 0, "peripheral"), (0x04, 0x04, "in-focus")];
    let mut strs: Vec<Value> = Vec::new();
    let el = |i: usize| vals.get(i).cloned().unwrap_or_default();
    let mut i = 0usize;
    while i + 7 <= vals.len() {
        let mut av: Vec<Value> = Vec::new();
        for fl in &flags {
            if pl::num_eq(&ops::bit_and(&el(i + 6), &n(fl.0)), &n(fl.1)) {
                av.push(pl::s(fl.2));
            }
        }
        let tail = if av.is_empty() { pl::s("") } else { pl::concat(&pl::concat(&pl::s("("), &pl::join(",", &av)), &pl::s(")")) };
        strs.push(pl::concat(&pl::concat(&pl::concat(&el(i + 2), &pl::s(",")), &el(i + 3)), &tail));
        i += 7;
    }
    ret(Value::new_array(strs))
}

//------------------------------------------------------------------------------
/// Convert Pentax hex-based EV (modulo 8) to real number
/// Inputs: 0) value to convert
/// eg) 0x00 -> 0
///     0x03 -> 0.33333
///     0x04 -> 0.5
///     0x05 -> 0.66666
///     0x08 -> 1   ...  etc
pub fn pentax_ev(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut val = arg(&a, 0);
    if ops::bit_and(&val, &n(0x01)).truthy() {
        let sign = if pl::lt(&val, &n(0)) { n(-1) } else { n(1) };
        let frac = ops::bit_and(&pl::mul(&val, &sign), &n(0x07));
        if pl::num_eq(&frac, &n(0x03)) {
            val = pl::add(&val, &pl::mul(&sign, &pl::sub(&pl::div(&n(8), &n(3))?, &frac)));
        } else if pl::num_eq(&frac, &n(0x05)) {
            val = pl::add(&val, &pl::mul(&sign, &pl::sub(&pl::div(&n(16), &n(3))?, &frac)));
        }
    }
    ret(pl::div(&val, &n(8))?)
}

//------------------------------------------------------------------------------
/// Convert number to Pentax hex-based EV (modulo 8)
/// Inputs: 0) number
/// Returns: Pentax EV code
pub fn pentax_ev_inv(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let num = arg(&a, 0);
    let mut val = pl::mul(&num, &n(8));
    // extra fudging makes sure 0.3 and 0.33333 both round up to 3, etc
    let sign = if pl::lt(&num, &n(0)) { n(-1) } else { n(1) };
    let inum = pl::sub(&pl::mul(&num, &sign), &pl::int(&pl::mul(&num, &sign)));
    if pl::gt(&inum, &f(0.29)) && pl::lt(&inum, &f(0.4)) {
        val = pl::add(&val, &pl::div(&sign, &n(3))?);
    } else if pl::gt(&inum, &f(0.6)) && pl::lt(&inum, &f(0.71)) {
        val = pl::sub(&val, &pl::div(&sign, &n(3))?);
    }
    ret(pl::int(&pl::add(&val, &pl::mul(&f(0.5), &sign))))
}

//------------------------------------------------------------------------------
/// Encrypt or decrypt Pentax ShutterCount (symmetrical encryption) - PH
/// Inputs: 0) shutter count value, 1) ExifTool object ref
/// Returns: Encrypted or decrypted ShutterCount
pub fn crypt_shutter_count(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (val, et) = (arg(&a, 0), arg(&a, 1));
    // Pentax Date and Time values are used in the encryption
    let (pd, pt) = (et.get("PentaxDate"), et.get("PentaxTime"));
    if !(pd.truthy() && pt.truthy() && pl::length(&pd).to_i64() == 4 && pl::length(&pt).to_i64() >= 3) {
        return pl::ret_undef();
    }
    // get Date and Time as integers (after padding Time with a null byte)
    let date = pl::unpack1("N", &pd)?;
    let time = pl::unpack1("N", &pl::concat(&pt, &pl::s("\0")))?;
    ret(ops::bit_xor(&ops::bit_xor(&val, &date), &pl::sub(&n(0xffffffff), &time)))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Pentax::PrintFilter", print_filter);
    it.define_native("Image::ExifTool::Pentax::DecodeAFPoints", decode_af_points);
    it.define_native("Image::ExifTool::Pentax::AFPointNamesK3III", af_point_names_k3iii);
    it.define_native("Image::ExifTool::Pentax::AFPointValuesK3III", af_point_values_k3iii);
    it.define_native("Image::ExifTool::Pentax::AFAreasK3III", af_areas_k3iii);
    it.define_native("Image::ExifTool::Pentax::PentaxEv", pentax_ev);
    it.define_native("Image::ExifTool::Pentax::PentaxEvInv", pentax_ev_inv);
    it.define_native("Image::ExifTool::Pentax::CryptShutterCount", crypt_shutter_count);
}
