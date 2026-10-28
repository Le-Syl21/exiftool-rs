//! Image::ExifTool::Canon (Image/ExifTool/Canon.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::{die, Binding, ER};
use crate::interp::{Interp, Want};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, re, ret};

fn n(i: i64) -> Value {
    Value::Int(i)
}

fn f(x: f64) -> Value {
    Value::Num(x)
}

// ---- local helpers (Perl operators the shared helpers don't cover) ----

/// `$a % $b` (fatal "Illegal modulus zero", as in perl).
fn modulo(a: &Value, b: &Value) -> ER<Value> {
    match ops::modulo(a, b) {
        Ok(v) => Ok(v),
        Err(e) => die(e),
    }
}

/// `substr($s, $off, $len)` for in-range offsets (undef past the end, as perl).
fn substr(s: &Value, off: i64, len: Option<i64>) -> Value {
    let b = s.to_bytes();
    match crate::interp::eval::substr_range(b.len(), off, len) {
        Some((a, e)) => Value::bytes(&b[a..e]),
        None => Value::Undef,
    }
}

/// `@a` joined by `$sep`.
fn join(sep: &str, v: &[Value]) -> Value {
    let parts: Vec<Vec<u8>> = v.iter().map(|x| x.to_bytes().to_vec()).collect();
    Value::bytes(parts.join(sep.as_bytes()))
}

/// A package hash (`%Pkg::name`) as a reference.
fn global_hash(it: &mut Interp, name: &str) -> Value {
    match it.global(b'%', name) {
        Binding::Hash(h) => Value::HashRef(h),
        _ => unreachable!(),
    }
}

fn get16u(it: &mut Interp, data_pt: &Value, pos: Value) -> ER<Value> {
    pl::call(it, "Image::ExifTool::Get16u", vec![data_pt.clone(), pos])
}

fn get32u(it: &mut Interp, data_pt: &Value, pos: Value) -> ER<Value> {
    pl::call(it, "Image::ExifTool::Get32u", vec![data_pt.clone(), pos])
}

//------------------------------------------------------------------------------
/// Return lens name with teleconverter if applicable
/// Inputs: 0) lens name string, 1) short focal length
/// Returns: lens string with tc if appropriate
pub fn lens_with_tc(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(lens_with_tc_rs(arg(&a, 0), &arg(&a, 1)))
}

fn lens_with_tc_rs(mut lens: Value, short_focal: &Value) -> Value {
    // add teleconverter multiplication factor if applicable
    // (and if the LensType doesn't already include one)
    if !re(r"x$", "").is_match(&lens) {
        if let Some(m) = re(r"(\d+)", "").m(&lens) {
            let sf = cap(&m, 1); // short focal length
            for tc in [n(1), f(1.4), n(2), f(2.8)] {
                if pl::gt(&pl::abs(&pl::sub(short_focal, &pl::mul(&sf, &tc))), &f(0.9)) {
                    continue;
                }
                if pl::gt(&tc, &n(1)) {
                    lens = pl::concat(&lens, &pl::concat(&pl::concat(&pl::s(" + "), &tc), &pl::s("x")));
                }
                break;
            }
        }
    }
    lens
}

//------------------------------------------------------------------------------
/// Attempt to calculate sensor size for Canon cameras
/// Inputs: 0) ExifTool ref
/// Returns: Sensor diagonal size in mm, or undef
/// Notes: This algorithm is fairly reliable, but has been found to give incorrect
///        values for some firmware versions of the EOS 20D, A310, SD40 and IXUS 65
/// (ref http://wyw.dcweb.cn/download.asp?path=&file=jhead-2.96-ccdwidth_hack.zip)
pub fn calc_sensor_diag(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let et = arg(&a, 0);
    // calculation is based on the rational value of FocalPlaneX/YResolution
    // (most Canon cameras store the sensor size in the denominator)
    let te = et.get("TAG_EXTRA");
    if !(te.get("FocalPlaneXResolution").truthy() && te.get("FocalPlaneYResolution").truthy()) {
        return pl::ret_undef();
    }
    let xres = te.get("FocalPlaneXResolution").get("Rational");
    let yres = te.get("FocalPlaneYResolution").get("Rational");
    if !(xres.truthy() && yres.truthy()) {
        return pl::ret_undef();
    }
    // assumptions: 1) numerators are image width/height * 1000
    // 2) denominators are sensor width/height in inches * 1000
    let sep = re(r"[ \/]", "");
    let xres = pl::split(&sep, &xres, 0);
    let yres = pl::split(&sep, &yres, 0);
    let (x0, x1, y0, y1) = (arg(&xres, 0), arg(&xres, 1), arg(&yres, 0), arg(&yres, 1));
    // verify assumptions as best we can:
        // numerators are always divisible by 1000
    if pl::num_eq(&modulo(&x0, &n(1000))?, &n(0)) && pl::num_eq(&modulo(&y0, &n(1000))?, &n(0)) &&
        // at least 640x480 pixels (DC models - PH)
        pl::ge(&x0, &n(640000)) && pl::ge(&y0, &n(480000)) &&
        // ... but not too big!
        pl::lt(&x0, &n(10000000)) && pl::lt(&y0, &n(10000000)) &&
        // minimum sensor size is 0.061 inches (DC models - PH)
        pl::ge(&x1, &n(61)) && pl::lt(&x1, &n(1500)) &&
        pl::ge(&y1, &n(61)) && pl::lt(&y1, &n(1000)) &&
        // sensor isn't square (may happen if rationals have been reduced)
        !pl::num_eq(&x1, &y1)
    {
        let sq = pl::add(&pl::mul(&x1, &x1), &pl::mul(&y1, &y1));
        return ret(pl::mul(&f(sq.to_f64().sqrt()), &f(0.0254)));
    }
    pl::ret_undef()
}

//------------------------------------------------------------------------------
/// Attempt to identify the specific lens if multiple lenses have the same LensType
/// Inputs: 0) PrintConv hash ref, 1) LensType, 2) MinFocalLength, 3) MaxFocalLength
///         4) MaxAperture, 5) LensModel
/// Notes: PrintConv, LensType, MinFocalLength and MaxFocalLength must be defined.
///        Other inputs are optional.
pub fn print_lens_id(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (print_conv, lens_type, short_focal, long_focal, max_aperture, lens_model) =
        (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4), arg(&a, 5));
    let mut lens = Value::Undef;
    if !(pl::str_eq(&lens_type, "-1") || pl::str_eq(&lens_type, "65535")) {
        lens = print_conv.get(lens_type.to_bytes());
    }
    let key = |i: usize| pl::concat(&pl::concat(&lens_type, &pl::s(".")), &n(i as i64)).to_bytes();
    if lens.truthy() {
        // return this lens unless other lenses have the same LensType
        if !print_conv.get(key(1)).truthy() {
            return ret(lens_with_tc_rs(lens, &short_focal));
        }
        re(r" or .*", "s").subst(&mut lens, false, |_| Vec::new()); // remove everything after "or"
        // make list of all possible matching lenses
        let mut lenses = vec![lens.clone()];
        let mut i = 1;
        while print_conv.get(key(i)).truthy() {
            lenses.push(print_conv.get(key(i)));
            i += 1;
        }
        let (mut user, mut maybe, mut likely, mut matches): (Vec<Value>, Vec<Value>, Vec<Value>, Vec<Value>) = Default::default();
        let user_lens = global_hash(it, "Image::ExifTool::userLens");
        // look for lens in user-defined lenses
        for lens in &lenses {
            if user_lens.get(lens.to_bytes()).truthy() {
                user.push(lens.clone());
            }
        }
        let mut tcs = vec![n(1), f(1.4), n(2), f(2.8)];
        pl::at(10204);
        pl::warn_uninit(it, &lens_model, "$lensModel", "pattern match (m//)")?;
        if let Some(m) = re(r" \+ ((EXTENDER )?RF)?(\d+(\.\d*)?)x\b", "").m(&lens_model) {
            tcs = vec![cap(&m, 3)];
        }
        // attempt to determine actual lens
        for tc in &tcs {
            // loop through teleconverter scaling factors
            for lens in &lenses {
                let Some(m) = re(r"(\d+)(?:-(\d+))?mm.*?(?:[fF]\/?)(\d+(?:\.\d+)?)(?:-(\d+(?:\.\d+)?))?", "").m(lens) else { continue };
                // ($1=short focal, $2=long focal, $3=max aperture wide, $4=max aperture tele)
                let (mut sf, mut lf, mut sa, mut la) = (cap(&m, 1), cap(&m, 2), cap(&m, 3), cap(&m, 4));
                // see if we can rule out this lens by focal length or aperture
                if sf.truthy() && !lf.truthy() {
                    lf = sf.clone();
                }
                if sa.truthy() && !la.truthy() {
                    la = sa.clone();
                }
                // account for converter-specific LensType's (ie. end with " + #.#x")
                if let Some(m) = re(r" \+ (\d+(\.\d+)?)x$", "").m(lens) {
                    let c1 = cap(&m, 1);
                    sf = pl::mul(&sf, &c1);
                    lf = pl::mul(&lf, &c1);
                    sa = pl::mul(&sa, &c1);
                    la = pl::mul(&la, &c1);
                }
                if pl::gt(&pl::abs(&pl::sub(&short_focal, &pl::mul(&sf, tc))), &f(0.9)) {
                    continue;
                }
                let mut tclens = lens.clone();
                if let Some(m) = re(r"^(.*) \+ (RF)?(\d+(\.\d*)?)x$", "").m(lens) {
                    if !(cap(&m, 3).to_bytes() == tc.to_bytes()) {
                        continue;
                    }
                    // remove previous entry if same lens
                    let mut p = b"^".to_vec();
                    p.extend_from_slice(&cap(&m, 1).to_bytes());
                    // (the pattern is compiled only when the list is not empty)
                    pl::at(10225);
                    if !maybe.is_empty() && pl::re_dyn(&p, "")?.is_match(maybe.last().unwrap()) {
                        maybe.pop();
                    }
                    pl::at(10226);
                    if !likely.is_empty() && pl::re_dyn(&p, "")?.is_match(likely.last().unwrap()) {
                        likely.pop();
                    }
                    pl::at(10227);
                    if !matches.is_empty() && pl::re_dyn(&p, "")?.is_match(matches.last().unwrap()) {
                        matches.pop();

                    }
                } else if pl::gt(tc, &n(1)) {
                    tclens = pl::concat(&tclens, &pl::concat(&pl::concat(&pl::s(" + "), tc), &pl::s("x")));
                }
                maybe.push(tclens.clone());
                if pl::gt(&pl::abs(&pl::sub(&long_focal, &pl::mul(&lf, tc))), &f(0.9)) {
                    continue;
                }
                likely.push(tclens.clone());
                if max_aperture.truthy() {
                    // (not 100% sure that TC affects MaxAperture, but it should!)
                    // (RF 24-105mm F4L IS USM shows a MaxAperture of 4.177)
                    if pl::lt(&max_aperture, &pl::sub(&pl::mul(&sa, tc), &f(0.18))) {
                        continue;
                    }
                    if pl::gt(&max_aperture, &pl::add(&pl::mul(&la, tc), &f(0.18))) {
                        continue;
                    }
                }
                matches.push(tclens);
            }
            if !maybe.is_empty() {
                break;
            }
        }
        if !user.is_empty() {
            // choose the best match if we have more than one
            if user.len() > 1 {
                let mut good: Vec<Value> = Vec::new();
                for t in [&matches, &likely, &maybe] {
                    for x in t.iter() {
                        if user_lens.get(x.to_bytes()).truthy() {
                            good.push(x.clone());
                            continue;
                        }
                        // check for match with TC string removed
                        let Some(m) = re(r"^(.*) \+ \d+(\.\d+)?x$", "").m(x) else { continue };
                        if user_lens.get(cap(&m, 1).to_bytes()).truthy() {
                            good.push(x.clone());
                        }
                    }
                    if !good.is_empty() {
                        return ret(join(" or ", &good));
                    }
                }
            }
            // default to returning the first user-defined lens
            return ret(lens_with_tc_rs(user[0].clone(), &short_focal));
        }
        // differentiate Sigma Art/Contemporary/Sports models
        if matches.len() > 1 && lens_model.truthy() {
            if let Some(m) = re(r"(\| [ACS])", "").m(&lens_model) {
                let typ = cap(&m, 1);
                let typ_re = pl::re_dyn(&crate::perl::regex::quotemeta(&typ.to_bytes()), "")?;
                let best: Vec<Value> = matches.iter().filter(|l| typ_re.is_match(l)).cloned().collect();
                if !best.is_empty() {
                    matches = best;
                }
            }
        }
        if matches.is_empty() {
            matches = likely;
        }
        if matches.is_empty() {
            matches = maybe;
        }
        // use LensModel focal length and aperture if necessary and available
        let fl_re = re(r"(\d+(?:\.\d+)?(?:-\d+(?:\.\d+)?)?) ?mm ?f\/?(\d+(?:\.\d+)?(?:-\d+(?:\.\d+)?)?)", "i");
        if matches.len() > 1 && lens_model.truthy() {
            if let Some(m) = fl_re.m(&lens_model) {
                let (mm, fstop) = (cap(&m, 1), cap(&m, 2));
                let mut best = Vec::new();
                for lens in &matches {
                    let Some(m) = fl_re.m(lens) else { continue };
                    if mm.to_bytes() == cap(&m, 1).to_bytes() && fstop.to_bytes() == cap(&m, 2).to_bytes() {
                        best.push(lens.clone());
                    }
                }
                if !best.is_empty() {
                    matches = best;
                }
            }
        }

        let try_ref = Value::new_array(matches);
        pl::call(it, "Image::ExifTool::Exif::MatchLensModel", vec![try_ref.clone(), lens_model.clone()])?;
        let matches = try_ref.items();
        if !matches.is_empty() {
            return ret(join(" or ", &matches));
        }
    } else if lens_model.truthy() && re(r"\d", "").is_match(&lens_model) {
        // use lens model as written by the camera
        let canon_lens_types = global_hash(it, "Image::ExifTool::Canon::canonLensTypes");
        if print_conv.same_ref(&canon_lens_types) {
            // add "Canon" to the start since the Canon cameras only understand Canon lenses
            return ret(pl::concat(&pl::s("Canon "), &lens_model));
        } else {
            return ret(lens_model);
        }
    }
    let mut str = pl::s("");
    if short_focal.truthy() {
        str = pl::concat(&str, &pl::sprintf(" %d", &[short_focal.clone()]));
        if long_focal.truthy() && !pl::num_eq(&long_focal, &short_focal) {
            str = pl::concat(&str, &pl::sprintf("-%d", &[long_focal.clone()]));
        }
        str = pl::concat(&str, &pl::s("mm"));
    }
    // (careful because Sigma LensType's may not be integer, so use string comparison)
    if pl::str_eq(&lens_type, "-1") || pl::str_eq(&lens_type, "65535") {
        return ret(pl::concat(&pl::s("Unknown"), &str));
    }
    ret(pl::concat(&pl::concat(&pl::concat(&pl::s("Unknown ("), &lens_type), &pl::s(")")), &str))
}

//------------------------------------------------------------------------------
/// Swap 16-bit words in 32-bit integers
/// Inputs: 0) string of integers
/// Returns: string of word-swapped integers
pub fn swap_words(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut v = pl::split_ws(&arg(&a, 0));
    for x in v.iter_mut() {
        *x = ops::bit_and(&ops::bit_or(&ops::shift_right(x, &n(16)), &ops::shift_left(x, &n(16))), &n(0xffffffff));
    }
    ret(join(" ", &v))
}

//------------------------------------------------------------------------------
/// Validate first word of Canon binary data
/// Inputs: 0) data pointer, 1) offset, 2-N) list of valid values
/// Returns: true if data value is the same
pub fn validate(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (data_pt, offset) = (arg(&a, 0), arg(&a, 1));
    let vals = a.get(2..).unwrap_or(&[]).to_vec();
    // the first 16-bit value is the length of the data in bytes
    let data_val = get16u(it, &data_pt, offset)?;
    for val in &vals {
        if pl::num_eq(val, &data_val) {
            return ret(n(1));
        }
    }
    pl::ret_undef()
}

//------------------------------------------------------------------------------
/// Validate CanonAFInfo
/// Inputs: 0) data pointer, 1) offset, 2) size
/// Returns: true if data appears valid
pub fn validate_af_info(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (data_pt, offset, size) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if pl::lt(&size, &n(24)) {
        return ret(n(0)); // must be at least 24 bytes long (PowerShot Pro1)
    }
    let af = get16u(it, &data_pt, offset.clone())?;
    if !re(r"^(1|5|7|9|15|45|53)$", "").is_match(&af) {
        return ret(n(0)); // check NumAFPoints
    }
    let w1 = get16u(it, &data_pt, pl::add(&offset, &n(4)))?;
    let h1 = get16u(it, &data_pt, pl::add(&offset, &n(6)))?;
    if !(h1.truthy() && w1.truthy()) {
        return ret(n(0));
    }
    let f1 = pl::div(&w1, &h1)?;
    let near = |x: &Value, y: f64| pl::lt(&pl::abs(&pl::sub(x, &f(y))), &f(0.01));
    // check for normal aspect ratio
    if near(&f1, 1.33) || near(&f1, 1.67) {
        return ret(n(1));
    }
    // ZoomBrowser can modify this for rotated images (ref Joshua Bixby)
    if near(&f1, 0.75) || near(&f1, 0.60) {
        return ret(n(1));
    }
    let w2 = get16u(it, &data_pt, pl::add(&offset, &n(8)))?;
    let h2 = get16u(it, &data_pt, pl::add(&offset, &n(10)))?;
    if !(h2.truthy() && w2.truthy()) {
        return ret(n(0));
    }
    // compare aspect ratio with AF image size
    // (but the Powershot AFImageHeight is odd, hence the test above)
    if w1.to_bytes() == h1.to_bytes() {
        return ret(n(0));
    }
    let f2 = pl::div(&w2, &h2)?;
    if pl::lt(&pl::abs(&pl::sub(&n(1), &pl::div(&f1, &f2)?)), &f(0.01)) {
        return ret(n(1));
    }
    if pl::lt(&pl::abs(&pl::sub(&n(1), &pl::mul(&f1, &f2))), &f(0.01)) {
        return ret(n(1));
    }
    ret(n(0))
}

//------------------------------------------------------------------------------
/// Convert the CameraISO value
/// Inputs: 0) value, 1) set for inverse conversion
pub fn camera_iso(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (val, inv) = (arg(&a, 0), arg(&a, 1));
    let mut rtn_val = Value::Undef;
    let iso_lookup = Value::new_hash();
    iso_lookup.set("0", pl::s("n/a"));
    iso_lookup.set("14", pl::s("Auto High")); //PH (S3IS)
    iso_lookup.set("15", pl::s("Auto"));
    iso_lookup.set("16", n(50));
    iso_lookup.set("17", n(100));
    iso_lookup.set("18", n(200));
    iso_lookup.set("19", n(400));
    iso_lookup.set("20", n(800)); //PH
    if inv.truthy() {
        rtn_val = pl::call(it, "Image::ExifTool::ReverseLookup", vec![val.clone(), iso_lookup.clone()])?;
        if rtn_val.is_undef() && pl::call(it, "Image::ExifTool::IsInt", vec![val.clone()])?.truthy() {
            rtn_val = ops::bit_or(&ops::bit_and(&val, &n(0x3fff)), &n(0x4000));
        }
    } else if !pl::num_eq(&val, &n(0x7fff)) {
        if ops::bit_and(&val, &n(0x4000)).truthy() {
            rtn_val = ops::bit_and(&val, &n(0x3fff));
        } else {
            rtn_val = iso_lookup.get(val.to_bytes());
            if !rtn_val.truthy() {
                rtn_val = pl::concat(&pl::concat(&pl::s("Unknown ("), &val), &pl::s(")"));
            }
        }
    }
    ret(rtn_val)
}

//------------------------------------------------------------------------------
/// Print range of focal lengths
/// Inputs: 0) short focal, 1) long focal, 2) optional scaling factor
pub fn print_focal_range(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (short, long, mut scale) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));

    if !scale.truthy() {
        scale = n(1);
    }
    if pl::num_eq(&short, &long) {
        ret(pl::sprintf("%.1f mm", &[pl::mul(&short, &scale)]))
    } else {
        ret(pl::sprintf("%.1f - %.1f mm", &[pl::mul(&short, &scale), pl::mul(&long, &scale)]))
    }
}

//------------------------------------------------------------------------------
/// Print 1D AF points
/// Inputs: 0) value to convert
/// Focus point pattern:
///            A1  A2  A3  A4  A5  A6  A7
///      B1  B2  B3  B4  B5  B6  B7  B8  B9  B10
///    C1  C2  C3  C4  C5  C6  C7  C9  C9  C10  C11
///      D1  D2  D3  D4  D5  D6  D7  D8  D9  D10
///            E1  E2  E3  E4  E5  E6  E7
pub fn print_af_points_1d(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let val = arg(&a, 0);
    if !pl::num_eq(&pl::length(&val), &n(8)) {
        return ret(pl::s("Unknown"));
    }
    // list of focus point values for decoding the first byte of the 8-byte record.
    // they are the x/y positions of each bit in the AF point mask
    // (y is upper 3 bits / x is lower 5 bits)
    #[rustfmt::skip]
    let focus_pts: &[i64] = &[0,0,
              0x04,0x06,0x08,0x0a,0x0c,0x0e,0x10,         0,0,
      0x21,0x23,0x25,0x27,0x29,0x2b,0x2d,0x2f,0x31,0x33,
    0x40,0x42,0x44,0x46,0x48,0x4a,0x4c,0x4d,0x50,0x52,0x54,
      0x61,0x63,0x65,0x67,0x69,0x6b,0x6d,0x6f,0x71,0x73,  0,0,
              0x84,0x86,0x88,0x8a,0x8c,0x8e,0x90,   0,0,0,0,0
    ];
    let unpack = |t: &str, d: &Value| crate::perl::pack::unpack(t.as_bytes(), &d.to_bytes()).unwrap_or_default();
    let focus = arg(&unpack("C", &val), 0);
    let bits_str = arg(&unpack("b*", &substr(&val, 1, None)), 0).to_bytes();
    let mut bits = bits_str.iter().map(|b| Value::bytes([*b]));
    let mut rows = b"  AAAAAAA  BBBBBBBBBBCCCCCCCCCCCDDDDDDDDDD  EEEEEEE     ".iter().map(|b| Value::bytes([*b]));
    let mut focusing = Value::Undef;
    let mut points: Vec<Value> = Vec::new();
    let mut last_row = pl::s("");
    let mut col = n(0);
    for &focus_pt in focus_pts {
        let row = rows.next().unwrap_or_default();
        col = if row.to_bytes() == last_row.to_bytes() { pl::add(&col, &n(1)) } else { n(1) };
        last_row = row.clone();
        if focus.to_bytes() == n(focus_pt).to_bytes() {
            focusing = pl::concat(&row, &col);
        }
        if bits.next().unwrap_or_default().truthy() {
            points.push(pl::concat(&row, &col));
        }
    }
    if !focusing.truthy() {
        focusing = if pl::num_eq(&focus, &n(0xff)) { pl::s("Auto") } else { pl::sprintf("Unknown (0x%.2x)", &[focus.clone()]) };
    }
    ret(pl::concat(&pl::concat(&focusing, &pl::s(" (")), &pl::concat(&join(",", &points), &pl::s(")"))))
}

//------------------------------------------------------------------------------
/// Convert Canon hex-based EV (modulo 0x20) to real number
/// Inputs: 0) value to convert
/// eg) 0x00 -> 0
///     0x0c -> 0.33333
///     0x10 -> 0.5
///     0x14 -> 0.66666
///     0x20 -> 1   ...  etc
pub fn canon_ev(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut val = arg(&a, 0);
    let sign;
    // temporarily make the number positive
    if pl::lt(&val, &n(0)) {
        val = pl::neg(&val);
        sign = n(-1);
    } else {
        sign = n(1);
    }
    let mut frac = ops::bit_and(&val, &n(0x1f));
    val = pl::sub(&val, &frac); // remove fraction
    // Convert 1/3 and 2/3 codes
    if pl::num_eq(&frac, &n(0x0c)) {
        frac = pl::div(&n(0x20), &n(3))?;
    } else if pl::num_eq(&frac, &n(0x14)) {
        frac = pl::div(&n(0x40), &n(3))?;
    }
    ret(pl::div(&pl::mul(&sign, &pl::add(&val, &frac)), &n(0x20))?)
}

//------------------------------------------------------------------------------
/// Convert number to Canon hex-based EV (modulo 0x20)
/// Inputs: 0) number
/// Returns: Canon EV code
pub fn canon_ev_inv(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut num = arg(&a, 0);
    let sign;
    // temporarily make the number positive
    if pl::lt(&num, &n(0)) {
        num = pl::neg(&num);
        sign = n(-1);
    } else {
        sign = n(1);
    }
    let val = pl::int(&num);
    let mut frac = pl::sub(&num, &val);
    if pl::lt(&pl::abs(&pl::sub(&frac, &f(0.33))), &f(0.05)) {
        frac = n(0x0c)
    } else if pl::lt(&pl::abs(&pl::sub(&frac, &f(0.67))), &f(0.05)) {
        frac = n(0x14);
    } else {
        frac = pl::int(&pl::add(&pl::mul(&frac, &n(0x20)), &f(0.5)));
    }
    ret(pl::mul(&sign, &pl::add(&pl::mul(&val, &n(0x20)), &frac)))
}

// ---- helpers for the directory processors ----

/// `$$ref = $v` on a scalar reference.
fn set_deref(r: &Value, v: Value) -> ER<()> {
    match r {
        Value::ScalarRef(c) => *c.borrow_mut() = v,
        Value::LvRef(l) => l.set(v)?,
        _ => {}
    }
    Ok(())
}

/// `\$val`: a new reference to a copy of a scalar.
fn new_ref(v: Value) -> Value {
    Value::ScalarRef(std::rc::Rc::new(std::cell::RefCell::new(v)))
}

/// `$raf->Read($buff, $len)`: the method writes its second argument, which
/// a native can only pass through Perl code, so the call runs as a code
/// string with `$buff` bound. Returns (bytes read, $buff).
fn raf_read(it: &mut Interp, raf: &Value, len: Value) -> ER<(Value, Value)> {
    let scope = crate::interp::Scope::new(None);
    scope.scalar("raf", raf.clone());
    let buff = scope.scalar("buff", Value::Undef);
    scope.scalar("len", len);
    match it.eval_string(b"$raf->Read($buff, $len)", "Image::ExifTool::Canon", &scope, Want::Scalar) {
        Ok(v) => {
            let b = buff.borrow().clone();
            Ok((v.last().cloned().unwrap_or_default(), b))
        }
        Err(e) => Err(crate::interp::Flow::Die(e)),
    }
}

fn dump_key(s: &str) -> Value {
    pl::s(s)
}

//------------------------------------------------------------------------------
/// Read original decision data from file (variable length)
/// Inputs: 0) ExifTool object ref, 1) offset in file
/// Returns: reference to original decision data (or undef if no data)
pub fn read_odd(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, offset) = (arg(&a, 0), arg(&a, 1));
    if !offset.truthy() {
        return pl::ret_undef();
    }
    let mut warn = Value::Undef;
    let raf = et.get("RAF");
    if raf.is_undef() {
        return pl::ret_undef();
    }
    // the data block is a variable length and starts with 0xffffffff
    // followed a 4-byte (int32u) version number
    let pos = pl::method(it, &raf, "Tell", vec![])?;
    let mut buff = Value::Undef;
    let mut ok = false;
    if pl::method(it, &raf, "Seek", vec![offset.clone(), n(0)])?.truthy() {
        let (len, b) = raf_read(it, &raf, n(8))?;
        buff = b;
        ok = pl::num_eq(&len, &n(8)) && re(r"^\xff{4}.\0\0", "s").is_match(&buff);
    }
    if ok {
        let mut err = true;
        // must set byte order in case it is different than current byte order
        // (we could be reading this after byte order was changed)
        let old_order = pl::call(it, "Image::ExifTool::GetByteOrder", vec![])?;
        let mut version = get32u(it, &new_ref(buff.clone()), n(4))?;
        if pl::gt(&version, &n(20)) {
            pl::call(it, "Image::ExifTool::ToggleByteOrder", vec![])?;
            let packed = crate::perl::pack::pack(b"V", &[version.clone()]).unwrap_or_default();
            version = arg(&crate::perl::pack::unpack(b"N", &packed).unwrap_or_default(), 0);
        }
        if pl::num_eq(&version, &n(1)) ||   // 1Ds (big endian)
            pl::num_eq(&version, &n(2))     // 5D/20D (little endian)
        {
            // this data is structured as follows:
            //  4 bytes: all 0xff
            //  4 bytes: version number (=1 or 2)
            // 20 bytes: sha1
            //  4 bytes: record count
            // for each record:
            // |  4 bytes: record number (beginning at 0)
            // |  4 bytes: block offset
            // |  4 bytes: block length
            // | 20 bytes: block sha1
            let (len, buf2) = raf_read(it, &raf, n(24))?;
            if pl::num_eq(&len, &n(24)) {
                buff = pl::concat(&buff, &buf2);
                let count = get32u(it, &new_ref(buf2.clone()), n(20))?;
                // read all records if the count is reasonable
                if count.truthy() && pl::lt(&count, &n(20)) {
                    let want = pl::mul(&count, &n(32));
                    let (len, buf2) = raf_read(it, &raf, want.clone())?;
                    if pl::num_eq(&len, &want) {
                        buff = pl::concat(&buff, &buf2);
                        err = false;
                    }
                }
            }
        } else if pl::num_eq(&version, &n(3)) {
            // newer models (little endian)
            // this data is structured as follows:
            //  4 bytes: all 0xff
            //  4 bytes: version number (=3)
            // 24 bytes: sha1 A length (=20) + sha1 A
            // 24 bytes: sha1 B length (=20) + sha1 B
            //  4 bytes: length of remaining data (including this length word!)
            //  8 bytes: salt length (=4) + salt ?
            //  4 bytes: unknown (=3)
            //  4 bytes: size of file
            //  4 bytes: unknown (=1 for most models, 2 for 5DmkII)
            //  4 bytes: unknown (=1)
            //  4 bytes: unknown (always the same for a given firmware version)
            //  4 bytes: unknown (random)
            //  4 bytes: record count
            // for each record:
            // |  4 bytes: record number (beginning at 1)
            // |  8 bytes: salt length (=4) + salt ?
            // | 24 bytes: sha1 length (=20) + sha1
            // |  4 bytes: block count
            // | for each block:
            // | |  4 bytes: block offset
            // | |  4 bytes: block length
            // followed by zero padding to end of ODD data (~72 bytes)
            let mut i = 0;
            loop {
                if i == 3 {
                    err = false;
                    break; // success!
                }
                let (len, buf2) = raf_read(it, &raf, n(4))?;
                if !pl::num_eq(&len, &n(4)) {
                    break;
                }
                buff = pl::concat(&buff, &buf2);
                let mut len = get32u(it, &new_ref(buf2.clone()), n(0))?;
                // (the data length includes the length word itself - doh!)
                if i == 2 && pl::ge(&len, &n(4)) {
                    len = pl::sub(&len, &n(4));
                }
                // make sure records are a reasonable size (<= 64kB)
                if !pl::le(&len, &n(0x10000)) {
                    break;
                }
                let (got, buf2) = raf_read(it, &raf, len.clone())?;
                if !pl::num_eq(&got, &len) {
                    break;
                }
                buff = pl::concat(&buff, &buf2);
                i += 1;
            }
        } else {
            warn = pl::concat(&pl::s("Unsupported original decision data version "), &version);
        }
        pl::call(it, "Image::ExifTool::SetByteOrder", vec![old_order])?;
        if !err {
            if pl::method(it, &et, "Options", vec![pl::s("HtmlDump")])?.truthy() {
                pl::method(it, &et, "HDump", vec![offset.clone(), pl::length(&buff), dump_key("[OriginalDecisionData]"), Value::Undef])?;
            }
            pl::method(it, &raf, "Seek", vec![pos, n(0)])?; // restore original file position
            return ret(new_ref(buff));
        }
    }
    let msg = if warn.truthy() { warn } else { pl::s("Invalid original decision data") };
    pl::method(it, &et, "Warn", vec![msg])?;
    pl::method(it, &raf, "Seek", vec![pos, n(0)])?; // restore original file position
    pl::ret_undef()
}

//------------------------------------------------------------------------------
/// Process a serial stream of binary data
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
/// Notes: The tagID's for serial stream tags are consecutive indices beginning
///        at 0, and the corresponding values must be contiguous in memory.
///        "Unknown" tags must be used to skip padding or unknown values.
/// (does not yet extract Rational values)
pub fn process_serial_data(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let offset = dir_info.get("DirStart");
    let size = dir_info.get("DirLen");
    let base = dir_info.get("Base");
    let base = if base.truthy() { base } else { n(0) };
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    let data_pos = dir_info.get("DataPos");
    let data_pos = if data_pos.truthy() { data_pos } else { n(0) };

    // temporarily set Unknown option so GetTagInfo() will return existing unknown tags
    // (require to maintain serial data synchronization)
    let unknown = pl::method(it, &et, "Options", vec![pl::s("Unknown"), n(1)])?;
    // but disable unknown tag generation (because processing ends when we run out of tags)
    et.set("NO_UNKNOWN", n(1));

    if verbose.truthy() {
        pl::method(it, &et, "VerboseDir", vec![pl::s("SerialData"), Value::Undef, size.clone()])?;
    }

    // get default format ('int8u' unless specified)
    let default_format = tag_table_ptr.get("FORMAT");
    let default_format = if default_format.truthy() { default_format } else { pl::s("int8u") };

    let val_h = Value::new_hash();
    let mut pos = n(0);
    let mut index: i64 = 0;
    while tag_table_ptr.get(index.to_string()).truthy() && pl::le(&pos, &size) {
        let tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), n(index)])?;
        if !tag_info.truthy() {
            break;
        }
        let mut format = tag_info.get("Format");
        let mut count = n(1);
        if format.truthy() {
            if let Some(m) = re(r"(.*)\[(.*)\]", "").m(&format) {
                format = cap(&m, 1);
                count = cap(&m, 2);
                // evaluate count to allow count to be based on previous values
                //### eval Format (%val, $size)
                let scope = crate::interp::Scope::new(None);
                if let Value::HashRef(h) = &val_h {
                    scope.declare(b'%', "val", Binding::Hash(h.clone()));
                }
                scope.scalar("size", size.clone());
                let r = it.eval_string(&count.to_bytes(), "Image::ExifTool::Canon", &scope, Want::Scalar);
                count = match r {
                    Ok(v) => v.last().cloned().unwrap_or_default(),
                    Err(_) => Value::Undef,
                };
                let at = it.get_scalar("main::@");
                if at.truthy() {
                    let msg = pl::concat(&pl::concat(&pl::concat(&pl::s("Format "), &tag_info.get("Name")), &pl::s(": ")), &at);
                    it.warn(msg)?;
                    break;
                }
            } else if pl::str_eq(&format, "string") {
                // allow string with no specified count to run to end of block
                count = if pl::gt(&size, &pos) { pl::sub(&size, &pos) } else { n(0) };
            }
        } else {
            format = default_format.clone();
        }
        let fs = pl::call(it, "Image::ExifTool::FormatSize", vec![format.clone()])?;
        let len = pl::mul(&if fs.truthy() { fs } else { n(1) }, &count);
        if pl::gt(&pl::add(&pos, &len), &size) {
            break;
        }
        let val = pl::call(it, "Image::ExifTool::ReadValue", vec![data_pt.clone(), pl::add(&pos, &offset), format.clone(), count.clone(), pl::sub(&size, &pos)])?;
        if val.is_undef() {
            break;
        }
        if verbose.truthy() {
            let start = pl::add(&pos, &offset);
            let addr = pl::add(&pl::add(&start, &base), &data_pos);
            pl::method(
                it,
                &et,
                "VerboseInfo",
                vec![
                    n(index),
                    tag_info.clone(),
                    pl::s("Index"),
                    n(index),
                    pl::s("Table"),
                    tag_table_ptr.clone(),
                    pl::s("Value"),
                    val.clone(),
                    pl::s("DataPt"),
                    data_pt.clone(),
                    pl::s("Size"),
                    len.clone(),
                    pl::s("Start"),
                    start,
                    pl::s("Addr"),
                    addr,
                    pl::s("Format"),
                    format.clone(),
                    pl::s("Count"),
                    count.clone(),
                ],
            )?;
        }
        val_h.set(index.to_string(), val.clone());
        if tag_info.get("SubDirectory").truthy() {
            let sub_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![tag_info.get("SubDirectory").get("TagTable")])?;
            let sub_dir_info = Value::new_hash();
            sub_dir_info.set("DataPt", new_ref(val.clone()));
            sub_dir_info.set("DataPos", pl::add(&data_pos, &pos));
            sub_dir_info.set("DirStart", n(0));
            sub_dir_info.set("DirLen", pl::length(&val));
            pl::method(it, &et, "ProcessDirectory", vec![sub_dir_info, sub_table_ptr])?;
        } else if !tag_info.get("Unknown").truthy() || unknown.truthy() {
            // don't extract zero-length information
            let key = if count.truthy() { pl::method(it, &et, "FoundTag", vec![tag_info.clone(), val.clone()])? } else { Value::Undef };
            if key.truthy() {
                let opts = et.get("OPTIONS");
                if opts.get("SaveFormat").truthy() {
                    et.get_or_new_hash("TAG_EXTRA").get_or_new_hash(key.to_bytes()).set("G6", format.clone());
                }
                if opts.get("SaveBin").truthy() {
                    pl::at(10592);
                    let bin = substr(&pl::sderef(&data_pt)?, pl::add(&pos, &offset).to_i64(), Some(len.to_i64()));
                    et.get_or_new_hash("TAG_EXTRA").get_or_new_hash(key.to_bytes()).set("BinVal", bin);
                }
            }
        }
        pos = pl::add(&pos, &len);
        index += 1;
    }
    pl::method(it, &et, "Options", vec![pl::s("Unknown"), unknown])?; // restore Unknown option
    et.delete("NO_UNKNOWN");
    ret(n(1))
}

//------------------------------------------------------------------------------
/// Read CMT3 maker notes from CR3 file
/// Inputs: 0) ExifTool object reference, 1) dirInfo ref, 2) tag table ref
/// Returns: data block (may be empty if no Exif data) or undef on error
pub fn process_cmt3(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));

    // extract the static maker notes to copying to other file types if requested
    // Note: this won't copy makernotes in the timed metadata since these are stored
    // separately, but the only records they have that aren't in the static maker notes
    // (for the M50) are: ColorData9, Flavor, CanonCameraInfoUnknown,
    // VignettingCorrUnknown1, Canon_0x4033 and Canon_0x402e
    if (pl::method(it, &et, "Options", vec![pl::s("MakerNotes")])?.truthy() || et.get("REQ_TAG_LOOKUP").get("makernotecanon").truthy())
        && pl::gt(&dir_info.get("DirLen"), &n(8))
    {
        let data_pt = dir_info.get("DataPt");
        // remove old (unused) trailer
        // (s/// on $$dataPt autovivifies an undefined $dataPt, but a string
        //  or another kind of reference dies)
        pl::at(10717);
        if !data_pt.is_undef() {
            pl::sderef(&data_pt)?;
        }
        let mut d = data_pt.deref();
        if re(r"(II\x2a\0|MM\0\x2a)\0{4,10}$", "").subst(&mut d, false, |_| Vec::new()) > 0 {
            set_deref(&data_pt, d.clone())?;
        }
        // remove TIFF header and append as the Canon makernote trailer
        // (so offsets will be interpreted correctly)
        let val = pl::concat(&substr(&d, 8, None), &substr(&d, 0, Some(8)));
        let uuid = global_hash(it, "Image::ExifTool::Canon::uuid");
        pl::method(it, &et, "FoundTag", vec![uuid.get("CMT3"), new_ref(val)])?;
    }
    it.call_method(&et, "ProcessTIFF", vec![et.clone(), dir_info, tag_table_ptr], want)
}

//------------------------------------------------------------------------------
/// Process CTMD EXIF information
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_exif_info(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let start = dir_info.get("DirStart");
    let start = if start.truthy() { start } else { n(0) };
    let dir_len = dir_info.get("DirLen");
    pl::at(10735);
    let dir_len = if dir_len.truthy() { dir_len } else { pl::sub(&pl::length(&pl::sderef(&data_pt)?), &start) };
    let dir_end = pl::add(&start, &dir_len);
    // loop through TIFF-format EXIF/MakerNote records
    let mut pos = start.clone();
    while pl::lt(&pl::add(&pos, &n(8)), &dir_end) {
        let len = get32u(it, &data_pt, pos.clone())?;
        let tag = get32u(it, &data_pt, pl::add(&pos, &n(4)))?;
        // test size/tag for valid ExifInfo (not EXIF in CRM files)
        if pl::lt(&len, &n(8)) || pl::gt(&pl::add(&pos, &len), &dir_end) || !tag_table_ptr.get(tag.to_bytes()).truthy() {
            break;
        }
        if pl::num_eq(&pos, &start) {
            pl::method(it, &et, "VerboseDir", vec![pl::s("ExifInfo"), Value::Undef, dir_len.clone()])?;
        }
        let p8 = pl::add(&pos, &n(8));
        pl::method(
            it,
            &et,
            "HandleTag",
            vec![
                tag_table_ptr.clone(),
                tag,
                Value::Undef,
                pl::s("DataPt"),
                data_pt.clone(),
                pl::s("Base"),
                pl::add(&dir_info.get("Base"), &p8), // base for TIFF pointers
                pl::s("DataPos"),
                pl::neg(&p8), // (relative to Base)
                pl::s("Start"),
                p8.clone(),
                pl::s("Size"),
                pl::sub(&len, &n(8)),
            ],
        )?;
        pos = pl::add(&pos, &len);
    }
    ret(n(1))
}

//------------------------------------------------------------------------------
/// Process Canon Timed MetaData (ref PH)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_ctmd(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    pl::at(10765);
    let dir_len = pl::length(&pl::sderef(&data_pt)?);
    let mut pos = n(0);

    pl::call(it, "Image::ExifTool::SetByteOrder", vec![pl::s("II")])?;
    while pl::lt(&pl::add(&pos, &n(6)), &dir_len) {
        let size = get32u(it, &data_pt, pos.clone())?;
        let typ = get16u(it, &data_pt, pl::add(&pos, &n(4)))?;
        // what is the meaning of the 6-byte header of these records?:
        //  type 1 - 00 00 00 01 zz zz - TimeStamp(CR3/CRM); zz=00(CR3),ff(CRM)
        //  type 3 - 00 00 00 01 zz zz - ? "ff ff ff ff"; zz=00(CR3),ff(CRM)
        //  type 4 - 00 00 00 01 ff ff - FocalInfo(CR3/CRM)
        //  type 5 - 00 00 00 01 ff ff - ExposureInfo(CR3/CRM)
        //  type 6 - 00 04 00 01 ff ff - ? "03 04 00 80 e0 15 ff ff"(CRM) [0x15e0 = ColorTemperature?]
        //  type 7 - xx yy 00 01 ff ff - ExifIFD + MakerNotes(CR3), ?(CRM); xxyy=0101(CR3),0004(CRM)
        //  type 8 - 01 yy 00 01 ff ff - MakerNotes(CR3), ?(CRM); yy=01(CR3),04(CRM)
        //  type 9 - 01 yy 00 01 ff ff - MakerNotes(CR3), ?(CRM); yy=01(CR3),00(CRM)
        //  type 10- 01 00 00 01 ff ff - ? (CRM)
        //  type 11- 01 00 00 01 ff ff - ? (CRM)
        // --> maybe yy == 01 for ExifInfo?
        if pl::lt(&size, &n(12)) {
            pl::method(it, &et, "Warn", vec![pl::s("Short CTMD record")])?;
            break;
        }
        if pl::gt(&pl::add(&pos, &size), &dir_len) {
            pl::method(it, &et, "Warn", vec![pl::s("Truncated CTMD record")])?;
            break;
        }
        pl::method(it, &et, "VerboseDir", vec![pl::concat(&pl::s("CTMD type "), &typ), Value::Undef, pl::sub(&size, &n(6))])?;
        if pl::gt(&verbose, &n(2)) {
            let out = pl::method(it, &et, "Options", vec![pl::s("TextOut")])?;
            pl::call(
                it,
                "Image::ExifTool::HexDump",
                vec![
                    data_pt.clone(),
                    n(6), // dump 6-byte header
                    pl::s("Start"),
                    pl::add(&pos, &n(6)),
                    pl::s("Addr"),
                    pl::add(&pl::add(&dir_info.get("Base"), &pos), &n(6)),
                    pl::s("Prefix"),
                    et.get("INDENT"),
                    pl::s("Out"),
                    out,
                ],
            )?;
        }
        if tag_table_ptr.get(typ.to_bytes()).truthy() {
            pl::method(
                it,
                &et,
                "HandleTag",
                vec![
                    tag_table_ptr.clone(),
                    typ,
                    Value::Undef,
                    pl::s("DataPt"),
                    data_pt.clone(),
                    pl::s("Base"),
                    dir_info.get("Base"),
                    pl::s("Start"),
                    pl::add(&pos, &n(12)),
                    pl::s("Size"),
                    pl::sub(&size, &n(12)),
                ],
            )?;
        } else if verbose.truthy() {
            pl::method(
                it,
                &et,
                "VerboseDump",
                vec![data_pt.clone(), pl::s("Len"), pl::sub(&size, &n(12)), pl::s("Start"), pl::add(&pos, &n(12)), pl::s("DataPos"), dir_info.get("Base")],
            )?;
        }
        pos = pl::add(&pos, &size);
    }
    if !pl::num_eq(&pos, &dir_len) {
        pl::method(it, &et, "Warn", vec![pl::s("Error parsing Canon CTMD data"), n(1)])?;
    }
    ret(n(1))
}

//------------------------------------------------------------------------------
/// Process a creative filter data
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_filters(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let mut pos = dir_info.get("DirStart");
    let dir_len = dir_info.get("DirLen");
    let data_pos = dir_info.get("DataPos");
    let data_pos = if data_pos.truthy() { data_pos } else { n(0) };
    let end = pl::add(&pos, &dir_len);
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;

    if pl::lt(&dir_len, &n(8)) {
        return ret(n(0));
    }
    let num_filters = get32u(it, &data_pt, pl::add(&pos, &n(4)))?;
    if verbose.truthy() {
        pl::method(it, &et, "VerboseDir", vec![pl::s("Creative Filter"), num_filters.clone()])?;
    }
    pos = pl::add(&pos, &n(8));
    let mut err = Value::Undef;
    let mut i = n(0);
    'filters: while pl::lt(&i, &num_filters) {
        // read filter structure:
        // 4 bytes - filter number
        // 4 bytes - filter data length
        // 4 bytes - number of parameters:
        // |  4 bytes - parameter ID
        // |  4 bytes - parameter value count
        // |  4 bytes * count - parameter values (NC)
        if pl::gt(&pl::add(&pos, &n(12)), &end) {
            err = pl::concat(&pl::s("Truncated data for filter "), &i);
            break;
        }
        let fnum = get32u(it, &data_pt, pos.clone())?; // (is this an index or an ID?)
        let size = get32u(it, &data_pt, pl::add(&pos, &n(4)))?;
        let nparm = get32u(it, &data_pt, pl::add(&pos, &n(8)))?;
        let nxt = pl::add(&pl::add(&pos, &n(4)), &size);
        if pl::gt(&nxt, &end) {
            err = pl::concat(&pl::concat(&pl::concat(&pl::s("Invalid size ("), &size), &pl::s(") for filter ")), &i);
            break;
        }
        if verbose.truthy() {
            pl::method(it, &et, "VerboseDir", vec![pl::concat(&pl::s("Filter "), &fnum), nparm.clone(), size.clone()])?;
        }
        pos = pl::add(&pos, &n(12));
        let mut j = n(0);
        while pl::lt(&j, &nparm) {
            if pl::gt(&pl::add(&pos, &n(12)), &end) {
                err = pl::concat(&pl::concat(&pl::concat(&pl::s("Truncated data for filter "), &i), &pl::s(" param ")), &j);
                break;
            }
            let tag = get32u(it, &data_pt, pos.clone())?;
            let count = get32u(it, &data_pt, pl::add(&pos, &n(4)))?;
            pos = pl::add(&pos, &n(8));
            if pl::gt(&pl::add(&pos, &pl::mul(&n(4), &count)), &end) {
                err = pl::concat(&pl::concat(&pl::concat(&pl::s("Truncated value for filter "), &i), &pl::s(" param ")), &j);
                break;
            }
            let val = pl::call(it, "Image::ExifTool::ReadValue", vec![data_pt.clone(), pos.clone(), pl::s("int32s"), count.clone(), pl::mul(&n(4), &count)])?;
            pl::method(
                it,
                &et,
                "HandleTag",
                vec![
                    tag_table_ptr.clone(),
                    tag,
                    val,
                    pl::s("DataPt"),
                    data_pt.clone(),
                    pl::s("DataPos"),
                    data_pos.clone(),
                    pl::s("Start"),
                    pos.clone(),
                    pl::s("Size"),
                    pl::mul(&n(4), &count),
                ],
            )?;
            pos = pl::add(&pos, &pl::mul(&n(4), &count));
            j = pl::add(&j, &n(1));
        }
        pos = nxt; // step to next filter
        i = pl::add(&i, &n(1));
        continue 'filters;
    }
    if err.truthy() {
        pl::method(it, &et, "Warn", vec![err, n(1)])?;
    }
    ret(n(1))
}

//------------------------------------------------------------------------------
/// Write Canon maker notes
/// Inputs: 0) ExifTool object reference, 1) dirInfo ref, 2) tag table ref
/// Returns: data block (may be empty if no Exif data) or undef on error
pub fn write_canon(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if !et.truthy() {
        return ret(n(1)); // allow dummy access to autoload this package
    }
    let mut dir_data = pl::call(it, "Image::ExifTool::Exif::WriteExif", vec![et.clone(), dir_info.clone(), tag_table_ptr])?;
    // add footer which is written by some Canon models (format of a TIFF header)
    if !dir_data.is_undef() && pl::length(&dir_data).truthy() && dir_info.get("Fixup").truthy() {
        let bo = pl::call(it, "Image::ExifTool::GetByteOrder", vec![])?;
        let s16 = pl::call(it, "Image::ExifTool::Set16u", vec![n(42)])?;
        let s32 = pl::call(it, "Image::ExifTool::Set32u", vec![n(0)])?;
        dir_data = pl::concat(&pl::concat(&pl::concat(&dir_data, &bo), &s16), &s32);
        pl::method(it, &dir_info.get("Fixup"), "AddFixup", vec![pl::sub(&pl::length(&dir_data), &n(4))])?;
    }
    ret(dir_data)
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Canon::LensWithTC", lens_with_tc);
    it.define_native("Image::ExifTool::Canon::CalcSensorDiag", calc_sensor_diag);
    it.define_native("Image::ExifTool::Canon::PrintLensID", print_lens_id);
    it.define_native("Image::ExifTool::Canon::SwapWords", swap_words);
    it.define_native("Image::ExifTool::Canon::Validate", validate);
    it.define_native("Image::ExifTool::Canon::ValidateAFInfo", validate_af_info);
    it.define_native("Image::ExifTool::Canon::CameraISO", camera_iso);
    it.define_native("Image::ExifTool::Canon::PrintFocalRange", print_focal_range);
    it.define_native("Image::ExifTool::Canon::PrintAFPoints1D", print_af_points_1d);
    it.define_native("Image::ExifTool::Canon::CanonEv", canon_ev);
    it.define_native("Image::ExifTool::Canon::CanonEvInv", canon_ev_inv);
    it.define_native("Image::ExifTool::Canon::ReadODD", read_odd);
    it.define_native("Image::ExifTool::Canon::ProcessSerialData", process_serial_data);
    it.define_native("Image::ExifTool::Canon::ProcessCMT3", process_cmt3);
    it.define_native("Image::ExifTool::Canon::ProcessExifInfo", process_exif_info);
    it.define_native("Image::ExifTool::Canon::ProcessCTMD", process_ctmd);
    it.define_native("Image::ExifTool::Canon::ProcessFilters", process_filters);
    it.define_native("Image::ExifTool::Canon::WriteCanon", write_canon);
}
