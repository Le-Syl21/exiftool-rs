//! Image::ExifTool::Exif (Image/ExifTool/Exif.pm), ported sub by sub (see docs/port/PORTING.md).

use std::cell::RefCell;
use std::rc::Rc;

use crate::interp::eval::{die, Flow, ER};
use crate::interp::{Binding, Interp, Scope, Want};
use crate::perl::{ops, pack, Value};
use crate::pl::{self, arg, cap, re, ret};

// ---- local helpers (Perl operations the shared helpers do not cover) ----

fn n(i: i64) -> Value {
    Value::Int(i)
}

/// `%Pkg::name` as a hash reference.
fn pkg_hash(it: &mut Interp, name: &str) -> Value {
    match it.global(b'%', name) {
        Binding::Hash(h) => Value::HashRef(h),
        _ => unreachable!(),
    }
}

/// `@Pkg::name` as an array reference.
#[allow(dead_code)]
fn pkg_array(it: &mut Interp, name: &str) -> Value {
    match it.global(b'@', name) {
        Binding::Array(a) => Value::ArrayRef(a),
        _ => unreachable!(),
    }
}

/// `\$v`: a reference to a new scalar holding `v`.
fn sref(v: Value) -> Value {
    Value::ScalarRef(Rc::new(RefCell::new(v)))
}

/// Call `name` with each of `args` aliased (`Func($a, $b)` in Perl, where
/// the sub may write `$_[i]`): the values written back are in `args` after
/// the call. Runs the call as a tiny Perl expression so the interpreter
/// sets the aliases up; `$@` is left as it was.
fn call_aliased(it: &mut Interp, name: &str, args: &mut [Value], want: Want) -> ER<Vec<Value>> {
    let scope = Scope::new(None);
    let cells: Vec<_> = args.iter().enumerate().map(|(i, v)| scope.scalar(&format!("a{i}"), v.clone())).collect();
    let list: Vec<String> = (0..args.len()).map(|i| format!("$a{i}")).collect();
    let code = format!("{name}({})", list.join(","));
    let saved_err = it.get_scalar("main::@");
    let r = it.eval_string(code.as_bytes(), "Image::ExifTool::Exif", &scope, want);
    it.set_scalar("main::@", saved_err);
    for (i, c) in cells.iter().enumerate() {
        args[i] = c.borrow().clone();
    }
    r.map_err(Flow::Die)
}

/// `Image::ExifTool::IsFloat($v)` (which may change `$v`: commas to decimals).
fn is_float(it: &mut Interp, v: &mut Value) -> ER<bool> {
    let mut a = [v.clone()];
    let r = call_aliased(it, "Image::ExifTool::IsFloat", &mut a, Want::Scalar)?;
    *v = a[0].clone();
    Ok(r.last().is_some_and(|x| x.truthy()))
}

/// `Image::ExifTool::IsInt($v)`
#[allow(dead_code)]
fn is_int(it: &mut Interp, v: &Value) -> ER<bool> {
    Ok(pl::call(it, "Image::ExifTool::IsInt", vec![v.clone()])?.truthy())
}

/// `sqrt($x)`, with perl's error.
fn sqrt(v: &Value) -> ER<Value> {
    let f = v.to_f64();
    if f < 0.0 {
        return die(format!("Can't take sqrt of {}", crate::perl::sprintf::format_g(f, 6, false, false)));
    }
    Ok(Value::Num(f.sqrt()))
}

/// `log($x)`, with perl's error.
fn log(v: &Value) -> ER<Value> {
    let f = v.to_f64();
    if f <= 0.0 {
        return die(format!("Can't take log of {}", crate::perl::sprintf::format_g(f, 6, false, false)));
    }
    Ok(Value::Num(f.ln()))
}

/// `exp($x)`
#[allow(dead_code)]
fn exp(v: &Value) -> Value {
    Value::Num(v.to_f64().exp())
}

/// `$a % $b`, with perl's error.
fn modulo(a: &Value, b: &Value) -> ER<Value> {
    match ops::modulo(a, b) {
        Ok(v) => Ok(v),
        Err(e) => die(e),
    }
}

/// `unpack(TEMPLATE, $data)` in list context.
fn unpack(t: &[u8], data: &Value) -> ER<Vec<Value>> {
    match pack::unpack(t, &data.to_bytes()) {
        Ok(v) => Ok(v),
        Err(e) => die(e),
    }
}

/// `pack(TEMPLATE, LIST)`
fn pack(t: &[u8], args: &[Value]) -> ER<Value> {
    match pack::pack(t, args) {
        Ok(v) => Ok(Value::bytes(v)),
        Err(e) => die(e),
    }
}

/// `substr($s, $off, $len)` (undef when outside the string).
fn substr(s: &Value, off: i64, len: Option<i64>) -> Value {
    let b = s.to_bytes();
    match crate::interp::eval::substr_range(b.len(), off, len) {
        Some((x, y)) => Value::bytes(&b[x..y]),
        None => Value::Undef,
    }
}

/// `join(SEP, LIST)`
fn join(sep: &[u8], list: &[Value]) -> Value {
    let parts: Vec<Vec<u8>> = list.iter().map(|x| x.to_bytes().to_vec()).collect();
    Value::bytes(parts.join(sep))
}

/// `@$a = LIST` on an array reference.
fn set_items(a: &Value, v: Vec<Value>) {
    if let Value::ArrayRef(r) = a {
        *r.borrow_mut() = v;
    }
}

/// `GetByteOrder()`
fn get_byte_order(it: &mut Interp) -> ER<Value> {
    pl::call(it, "Image::ExifTool::GetByteOrder", vec![])
}

// ---- the subs, in the order of Exif.pm ----

/// size limit for loading binary data block into memory
pub fn binary_data_limit(_it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(n(10 * 1024 * 1024))
}

/// AutoLoad our writer routines when necessary
pub fn autoload(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let name = it.get_scalar("Image::ExifTool::Exif::AUTOLOAD");
    let mut args = vec![name];
    args.extend(a);
    it.call_named("Image::ExifTool::DoAutoLoad", args, want)
}

/// Identify RAW file type for some TIFF-based formats using Compression value
/// Inputs: 0) ExifTool object reference, 1) Compression value
/// - sets TIFF_TYPE and FileType if identified
pub fn identify_raw_file(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, comp) = (arg(&a, 0), arg(&a, 1));
    if et.get("FILE_TYPE").eq_str("TIFF") && !et.get("IdentifiedRawFile").truthy() {
        let compression = pkg_hash(it, "Image::ExifTool::Exif::compression");
        let c = compression.get(comp.to_bytes());
        if c.truthy() {
            if let Some(m) = re(r"^\w+ ([A-Z]{3}) Compressed$", "").m(&c) {
                et.set("TIFF_TYPE", cap(&m, 1));
                pl::method(it, &et, "OverrideFileType", vec![et.get("TIFF_TYPE")])?;
                et.set("IdentifiedRawFile", n(1));
            }
        }
    }
    pl::ret_nothing(want)
}

/// The float extracted by CalculateLV's validity check.
const FLOAT_RE: &str = r"([+-]?(?=\d|\.\d)\d*(\.\d*)?([Ee]([+-]?\d+))?)";

/// Calculate LV (Light Value)
/// Inputs: 0) Aperture, 1) ShutterSpeed, 2) ISO
/// Returns: LV value (and converts input values to floating point if necessary)
pub fn calculate_lv(it: &mut Interp, mut a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    // do validity checks on arguments
    if a.len() < 3 {
        return pl::ret_undef();
    }
    for i in 0..a.len() {
        if !a[i].truthy() {
            return pl::ret_undef();
        }
        let Some(m) = re(FLOAT_RE, "").m(&a[i]) else { return pl::ret_undef() };
        let f = cap(&m, 1);
        if !pl::gt(&f, &n(0)) {
            return pl::ret_undef();
        }
        a[i] = f.clone(); // extract float from any other garbage
        it.set_arg(i, f)?;
    }
    // (A light value of 0 is defined as f/1.0 at 1 second with ISO 100)
    let x = pl::div(&pl::mul(&pl::mul(&a[0], &a[0]), &n(100)), &pl::mul(&a[1], &a[2]))?;
    ret(pl::div(&log(&x)?, &log(&n(2))?)?)
}

/// Calculate scale factor for 35mm effective focal length (ref 26/PH)
/// Inputs: 0) ExifTool object ref
///         1) Focal length
///         2) Focal length in 35mm format
///         3) Canon digital zoom factor
///         4) Focal plane diagonal size (in mm)
///         5) Sensor size (X and Y in mm)
///         6/7) Focal plane X/Y size (in mm)
///         8) focal plane resolution units (1=None,2=inches,3=cm,4=mm,5=um)
///         9/10) Focal plane X/Y resolution
///         11/12,13/14...) Image width/height in order of precedence (first valid pair is used)
/// Returns: 35mm conversion factor (or undefined if it can't be calculated)
pub fn calc_scale_factor35efl(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let et = arg(&a, 0);
    let mut args: Vec<Value> = a.iter().skip(1).cloned().collect();
    let res = arg(&args, 7); // save resolution units (in case they have been converted to string)
    let sens_xy = arg(&args, 4);
    // Image::ExifTool::ToFloat(@_) (converts the caller's values too)
    let defined: Vec<bool> = args.iter().map(|v| !v.is_undef()).collect();
    call_aliased(it, "Image::ExifTool::ToFloat", &mut args, Want::Scalar)?;
    for (i, d) in defined.iter().enumerate() {
        if *d {
            it.set_arg(i + 1, args[i].clone())?;
        }
    }
    let mut q = std::collections::VecDeque::from(args);
    let focal = shift(&mut q);
    let foc35 = shift(&mut q);

    if focal.truthy() && foc35.truthy() {
        return ret(pl::div(&foc35, &focal)?);
    }

    let digz = {
        let v = shift(&mut q);
        if v.truthy() {
            v
        } else {
            n(1)
        }
    };
    let mut diag = shift(&mut q);
    let sens = shift(&mut q);
    // calculate Canon sensor size using a dedicated algorithm
    if et.get("Make").eq_str("Canon") {
        crate::runtime::require(it, "Image::ExifTool::Canon")?;
        let canon_diag = pl::call(it, "Image::ExifTool::Canon::CalcSensorDiag", vec![et.clone()])?;
        if canon_diag.truthy() {
            diag = canon_diag;
        }
    }
    if !(diag.truthy() && is_float(it, &mut diag)?) {
        let m = if sens.truthy() { re(r" (\d+(\.?\d*)?)$", "").m(&sens_xy) } else { None };
        if let Some(m) = m {
            let one = cap(&m, 1);
            diag = sqrt(&pl::add(&pl::mul(&sens, &sens), &pl::mul(&one, &one)))?;
        } else {
            diag = Value::Undef;
            let xsize = shift(&mut q);
            let ysize = shift(&mut q);
            if xsize.truthy() && ysize.truthy() {
                // validate by checking aspect ratio because FocalPlaneX/YSize is not reliable
                let a = pl::div(&xsize, &ysize)?;
                if pl::lt(&pl::abs(&pl::sub(&a, &Value::Num(1.3333))), &Value::Num(0.1)) || pl::lt(&pl::abs(&pl::sub(&a, &Value::Num(1.5))), &Value::Num(0.1)) {
                    diag = sqrt(&pl::add(&pl::mul(&xsize, &xsize), &pl::mul(&ysize, &ysize)))?;
                }
            }
        }
        if !diag.truthy() {
            // get number of mm in units (assume inches unless otherwise specified)
            let lkup = |k: &[u8]| -> Value {
                match k {
                    b"3" => n(10),
                    b"4" => n(1),
                    b"5" => Value::Num(0.001),
                    b"cm" => n(10),
                    b"mm" => n(1),
                    b"um" => Value::Num(0.001),
                    _ => Value::Undef,
                }
            };
            let key = {
                let v = shift(&mut q);
                if v.truthy() {
                    v
                } else if res.truthy() {
                    res.clone()
                } else {
                    pl::s("")
                }
            };
            let units = {
                let u = lkup(&key.to_bytes());
                if u.truthy() {
                    u
                } else {
                    Value::Num(25.4)
                }
            };
            let mut x_res = shift(&mut q);
            if !x_res.truthy() {
                return pl::ret_undef();
            }
            let mut y_res = {
                let v = shift(&mut q);
                if v.truthy() {
                    v
                } else {
                    x_res.clone()
                }
            };
            if !(is_float(it, &mut x_res)? && !pl::num_eq(&x_res, &n(0))) {
                return pl::ret_undef();
            }
            if !(is_float(it, &mut y_res)? && !pl::num_eq(&y_res, &n(0))) {
                return pl::ret_undef();
            }
            let (mut w, mut h);
            loop {
                if q.len() < 2 {
                    return pl::ret_undef();
                }
                w = shift(&mut q);
                h = shift(&mut q);
                if !(w.truthy() && h.truthy()) {
                    continue;
                }
                let a = pl::div(&w, &h)?;
                if pl::gt(&a, &Value::Num(0.5)) && pl::lt(&a, &n(2)) {
                    break; // stop if we get a reasonable value
                }
            }
            // calculate focal plane size in mm
            w = pl::mul(&w, &pl::div(&units, &x_res)?);
            h = pl::mul(&h, &pl::div(&units, &y_res)?);
            diag = sqrt(&pl::add(&pl::mul(&w, &w), &pl::mul(&h, &h)))?;
            // make sure size is reasonable
            if !(pl::gt(&diag, &n(1)) && pl::lt(&diag, &n(100))) {
                return pl::ret_undef();
            }
        }
    }
    ret(pl::div(&pl::mul(&sqrt(&n(36 * 36 + 24 * 24))?, &digz), &diag)?)
}

/// `shift` on CalcScaleFactor35efl's `@_`.
fn shift(q: &mut std::collections::VecDeque<Value>) -> Value {
    q.pop_front().unwrap_or_default()
}

/// Print exposure compensation fraction
pub fn print_fraction(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut val = arg(&a, 0);
    let mut str = Value::Undef;
    if !val.is_undef() {
        val = pl::mul(&val, &Value::Num(1.00001)); // avoid round-off errors
        if !val.truthy() {
            str = pl::s("0");
        } else if pl::gt(&pl::div(&pl::int(&val), &val)?, &Value::Num(0.999)) {
            str = pl::sprintf("%+d", &[pl::int(&val)]);
        } else if pl::gt(&pl::div(&pl::int(&pl::mul(&val, &n(2))), &pl::mul(&val, &n(2)))?, &Value::Num(0.999)) {
            str = pl::sprintf("%+d/2", &[pl::int(&pl::mul(&val, &n(2)))]);
        } else if pl::gt(&pl::div(&pl::int(&pl::mul(&val, &n(3))), &pl::mul(&val, &n(3)))?, &Value::Num(0.999)) {
            str = pl::sprintf("%+d/3", &[pl::int(&pl::mul(&val, &n(3)))]);
        } else {
            str = pl::sprintf("%+.3g", &[val]);
        }
    }
    ret(str)
}

/// Convert fraction or number to floating point value (or 'undef' or 'inf')
pub fn convert_fraction(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut val = arg(&a, 0);
    if let Some(m) = re(r"([-+]?\d+)/(\d+)", "").m(&val) {
        let (one, two) = (cap(&m, 1), cap(&m, 2));
        val = if two.truthy() {
            pl::div(&one, &two)?
        } else if one.truthy() {
            pl::s("inf")
        } else {
            pl::s("undef")
        };
    }
    ret(val)
}

/// Convert EXIF text to something readable
/// Inputs: 0) ExifTool object reference, 1) EXIF text,
///         2) [optional] 1 to apply CharsetEXIF to ASCII text,
///         3) tag name for warning message (may be argument 2)
/// Returns: text encoded according to Charset option (with trailing spaces removed)
pub fn convert_exif_text(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, val, ascii_flex, mut tag) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    if pl::lt(&pl::length(&val), &n(8)) {
        return ret(val);
    }
    let id = substr(&val, 0, Some(8));
    let mut str = substr(&val, 8, None);
    let mut type_ = Value::Undef;

    et.delete("WrongByteOrder");
    if et.get("OPTIONS").get("Validate").truthy() {
        if let Some(m) = re(r"^(ASCII|UNICODE|JIS)?\0* \0*$", "").m(&id) {
            let one = cap(&m, 1);
            let mut msg = if one.truthy() { one } else { pl::s("Undefined") };
            msg = pl::concat(&msg, &pl::s(" text header"));
            if tag.truthy() {
                msg = pl::concat(&msg, &pl::concat(&pl::s(" for "), &tag));
            }
            msg = pl::concat(&msg, &pl::s(" has spaces instead of nulls"));
            pl::method(it, &et, "Warn", vec![msg])?;
        }
    }
    // Note: allow spaces instead of nulls in the ID codes because
    // it is fairly common for camera manufacturers to get this wrong
    // (also handle Canon ZoomBrowser EX 4.5 null followed by 7 bytes of garbage)
    if re(r"^(ASCII)?(\0|[\0 ]+$)", "").is_match(&id) {
        // truncate at null terminator (shouldn't have a null based on the
        // EXIF spec, but it seems that few people actually read the spec)
        re(r"\0.*", "s").subst(&mut str, false, |_| Vec::new());
        // allow ASCII text to contain any other specified encoding
        if ascii_flex.truthy() && ascii_flex.eq_str("1") {
            let enc = pl::method(it, &et, "Options", vec![pl::s("CharsetEXIF")])?;
            if enc.truthy() {
                str = pl::method(it, &et, "Decode", vec![str, enc])?;
            }
        }
    // by the EXIF spec, the following string should be "UNICODE\0", but
    // apparently Kodak sometimes uses "Unicode\0" in the APP3 "Meta" information.
    // However, unfortunately Ricoh uses "Unicode\0" in the RR30 EXIF UserComment
    // when the text is actually ASCII, so only recognize uppercase "UNICODE\0".
    } else if let Some(m) = re(r"^(UNICODE)[\0 ]$", "").m(&id) {
        type_ = cap(&m, 1);
        // MicrosoftPhoto writes as little-endian even in big-endian EXIF,
        // so we must guess at the true byte ordering
        str = pl::method(it, &et, "Decode", vec![str, pl::s("UTF16"), pl::s("Unknown")])?;
    } else if let Some(m) = re(r"^(JIS)[\0 ]{5}$", "").m(&id) {
        type_ = cap(&m, 1);
        str = pl::method(it, &et, "Decode", vec![str, pl::s("JIS"), pl::s("Unknown")])?;
    } else {
        if ascii_flex.truthy() && !ascii_flex.eq_str("1") {
            tag = ascii_flex.clone();
        }
        let mut msg = pl::s("Invalid EXIF text encoding");
        if tag.truthy() {
            msg = pl::concat(&msg, &pl::concat(&pl::s(" for "), &tag));
        }
        pl::method(it, &et, "Warn", vec![msg])?;
        str = pl::concat(&id, &str);
    }
    if et.get("WrongByteOrder").truthy() && et.get("OPTIONS").get("Validate").truthy() {
        let mut msg = pl::s("Wrong byte order for EXIF");
        if tag.truthy() {
            msg = pl::concat(&msg, &pl::concat(&pl::s(" "), &tag));
        }
        if type_.truthy() {
            msg = pl::concat(&msg, &pl::concat(&pl::s(" "), &type_));
        }
        msg = pl::concat(&msg, &pl::s(" text"));
        pl::method(it, &et, "Warn", vec![msg])?;
    }
    re(r" +$", "").subst(&mut str, false, |_| Vec::new()); // trim trailing blanks
    ret(str)
}

/// Print conversion for SpatialFrequencyResponse
pub fn print_sfr(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let val = arg(&a, 0);
    if !pl::gt(&pl::length(&val), &n(4)) {
        return ret(val);
    }
    let vref = sref(val.clone());
    let nn = pl::call(it, "Image::ExifTool::Get16u", vec![vref.clone(), n(0)])?;
    let m = pl::call(it, "Image::ExifTool::Get16u", vec![vref.clone(), n(2)])?;
    let mut cols = pl::split(&re(r"\0", ""), &substr(&val, 4, None), pl::add(&nn, &n(1)).to_i64());
    let pos = pl::sub(&pl::length(&val), &pl::mul(&pl::mul(&n(8), &nn), &m));
    if !(pl::num_eq(&n(cols.len() as i64), &pl::add(&nn, &n(1))) && pl::ge(&pos, &n(4))) {
        return ret(val);
    }
    cols.pop();
    let mut i = n(0);
    while pl::lt(&i, &nn) {
        let mut rows = Vec::new();
        let mut j = n(0);
        while pl::lt(&j, &m) {
            let off = pl::add(&pos, &pl::mul(&n(8), &pl::add(&i, &pl::mul(&j, &nn))));
            rows.push(pl::call(it, "Image::ExifTool::GetRational64u", vec![vref.clone(), off])?);
            j = pl::add(&j, &n(1));
        }
        let k = i.to_i64() as usize;
        let add = pl::concat(&pl::concat(&pl::s("="), &join(b",", &rows)), &pl::s(""));
        if k >= cols.len() {
            cols.resize(k + 1, Value::Undef);
        }
        cols[k] = pl::concat(&cols[k], &add);
        i = pl::add(&i, &n(1));
    }
    ret(join(b"; ", &cols))
}

/// Print numerical parameter value (with sign, or 'Normal' for zero)
/// Inputs: 0) value, 1) flag for inverse conversion, 2) conversion hash reference
pub fn print_parameter(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (mut val, inv) = (arg(&a, 0), arg(&a, 1));
    if inv.truthy() {
        return ret(val);
    }
    if pl::gt(&val, &n(0)) {
        if pl::gt(&val, &n(0xfff0)) {
            // a negative value in disguise?
            val = pl::sub(&val, &n(0x10000));
        } else {
            val = pl::concat(&pl::s("+"), &val);
        }
    }
    ret(val)
}

/// Convert parameter back to standard EXIF value
///   0,0.00,etc or "Normal" => 0
///   -1,-2,etc or "Soft" or "Low" => 1
///   +1,+2,1,2,etc or "Hard" or "High" => 2
pub fn convert_parameter(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut val = arg(&a, 0);
    let is_float = is_float(it, &mut val)?;
    // normal is a value of zero
    if re(r"\bn", "i").is_match(&val) || (is_float && pl::num_eq(&val, &n(0))) {
        return ret(n(0));
    }
    // "soft", "low" or any negative number is a value of 1
    if re(r"\b(s|l)", "i").is_match(&val) || (is_float && pl::lt(&val, &n(0))) {
        return ret(n(1));
    }
    // "hard", "high" or any positive number is a value of 2
    if re(r"\bh", "i").is_match(&val) || is_float {
        return ret(n(2));
    }
    pl::ret_undef()
}

/// Calculate Red/BlueBalance
/// Inputs: 0) 0=red, 1=blue, 1-8) WB_RGGB/RGBG/RBGG/GRBG/GRGB/RGB/GRB/RBLevels,
///         8) red or blue level, 9) green level
pub fn red_blue_balance(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let rggb_lookup = crate::runtime::lex_array(it, "Image::ExifTool::Exif", "rggbLookup");
    let blue = arg(&a, 0);
    let mut rest = 1; // the next argument "shift" takes
    let mut val = Value::Undef;
    let num = rggb_lookup.len();
    for i in 0..num {
        let levels = arg(&a, rest);
        rest += 1;
        if !levels.truthy() {
            continue;
        }
        let levels = pl::split_ws(&levels);
        if levels.len() < 2 {
            continue;
        }
        let lookup = rggb_lookup.at(i as i64);
        let lv = |idx: &Value| -> Value { Value::new_array(levels.clone()).at(idx.to_i64()) };
        let mut g = lookup.at(1); // get green level or index
        let bi = pl::mul(&blue, &n(3)).to_i64();
        if pl::lt(&g, &n(4)) {
            if levels.len() < 3 {
                continue;
            }
            g = pl::div(&pl::add(&lv(&g), &lv(&lookup.at(2))), &n(2))?;
            if !g.truthy() {
                continue;
            }
        } else if pl::lt(&lv(&lookup.at(bi)), &n(4)) {
            g = n(1); // Some Nikon cameras use a scaling factor of 1 (E5700)
        }
        val = pl::div(&lv(&lookup.at(bi)), &g)?;
        break;
    }
    let (x, y) = (arg(&a, rest), arg(&a, rest + 1));
    if val.is_undef() && (x.truthy() && y.truthy()) {
        val = pl::div(&x, &y)?;
    }
    ret(val)
}

/// Print exposure time as a fraction
pub fn print_exposure_time(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut secs = arg(&a, 0);
    if !is_float(it, &mut secs)? {
        return ret(secs);
    }
    if pl::lt(&secs, &Value::Num(0.25001)) && pl::gt(&secs, &n(0)) {
        return ret(pl::sprintf("1/%d", &[pl::int(&pl::add(&Value::Num(0.5), &pl::div(&n(1), &secs)?))]));
    }
    // $_ = sprintf("%.1f",$secs); s/\.0$//; return $_;  (the global $_)
    let mut t = pl::sprintf("%.1f", &[secs]);
    it.set_scalar("main::_", t.clone());
    if re(r"\.0$", "").subst(&mut t, false, |_| Vec::new()) > 0 {
        it.set_scalar("main::_", t.clone());
    }
    ret(it.get_scalar("main::_"))
}

/// Print FNumber
pub fn print_fnumber(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut val = arg(&a, 0);
    if is_float(it, &mut val)? && pl::gt(&val, &n(0)) {
        // round to 1 decimal place, or 2 for values < 1.0
        val = pl::sprintf(if pl::lt(&val, &n(1)) { "%.2f" } else { "%.1f" }, &[val.clone()]);
    }
    ret(val)
}

/// Decode raw CFAPattern value
/// Inputs: 0) ExifTool ref, 1) binary value
/// Returns: string of numbers
pub fn decode_cfa_pattern(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, mut val) = (arg(&a, 0), arg(&a, 1));
    // some panasonic cameras (SV-AS3, SV-AS30) write this in ascii (very odd)
    if re(r"^[0-6]+$", "").is_match(&val) {
        pl::method(it, &self_, "Warn", vec![pl::s("Incorrectly formatted CFAPattern"), n(1)])?;
        val = Value::bytes(crate::perl::tr::tr(&val.to_bytes(), b"0-6", b"\\x00-\\x06", "").0);
    }
    if !pl::ge(&pl::length(&val), &n(4)) {
        return ret(val);
    }
    let t: &[u8] = if get_byte_order(it)?.eq_str("II") { b"v2C*" } else { b"n2C*" };
    let mut av = unpack(t, &val)?;
    let end = pl::add(&n(2), &pl::mul(&arg(&av, 0), &arg(&av, 1)));
    if pl::gt(&end, &n(av.len() as i64)) {
        // try swapping byte order (I have seen this order different than in EXIF)
        let xy = unpack(b"n2", &pack(b"v2", &[arg(&av, 0), arg(&av, 1)])?)?;
        let (x, y) = (arg(&xy, 0), arg(&xy, 1));
        if pl::lt(&n(av.len() as i64), &pl::add(&n(2), &pl::mul(&x, &y))) {
            pl::method(it, &self_, "Warn", vec![pl::s("Invalid CFAPattern"), n(1)])?;
        } else {
            while av.len() < 2 {
                av.push(Value::Undef);
            }
            av[0] = x;
            av[1] = y;
            // (can't technically be wrong because the order isn't well defined by the EXIF spec)
            // $self->Warn('Wrong byte order for CFAPattern');
        }
    }
    let sep = it.get_scalar("main::\"");
    ret(join(&sep.to_bytes(), &av))
}

/// Print CFA Pattern
pub fn print_cfa_pattern(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let val = arg(&a, 0);
    let av = pl::split_ws(&val);
    if av.len() < 2 {
        return ret(pl::s("<truncated data>"));
    }
    if !(av[0].truthy() && av[1].truthy()) {
        return ret(pl::s("<zero pattern size>"));
    }
    let end = pl::add(&n(2), &pl::mul(&av[0], &av[1]));
    if pl::gt(&end, &n(av.len() as i64)) {
        return ret(pl::s("<invalid pattern size>"));
    }
    let cfa_color = Value::new_array(["Red", "Green", "Blue", "Cyan", "Magenta", "Yellow", "White"].iter().map(|s| pl::s(s)).collect());
    let arr = Value::new_array(av.clone());
    let (mut pos, mut rtn_val) = (n(2), pl::s("["));
    loop {
        let c = cfa_color.at(arr.at(pos.to_i64()).to_i64());
        rtn_val = pl::concat(&rtn_val, &if c.truthy() { c } else { pl::s("Unknown") });
        pos = pl::add(&pos, &n(1));
        if pl::ge(&pos, &end) {
            break;
        }
        if modulo(&pl::sub(&pos, &n(2)), &av[1])?.truthy() {
            rtn_val = pl::concat(&rtn_val, &pl::s(","));
            continue;
        }
        rtn_val = pl::concat(&rtn_val, &pl::s("]["));
    }
    ret(pl::concat(&rtn_val, &pl::s("]")))
}

/// Print Opcode List
/// Inputs: 0) value, 1) flag for inverse conversion, 2) conversion hash reference
/// Returns: converted value
pub fn print_opcode(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (val, inv, conv) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if inv.truthy() {
        return pl::ret_undef(); // (can't do inverse conversion)
    }
    let data = val.deref();
    let len = pl::length(&data);
    if !pl::gt(&len, &n(4)) {
        return ret(pl::s(""));
    }
    let num = arg(&unpack(b"N", &data)?, 0);
    let mut pos = n(4);
    let mut ops_ = Vec::new();
    let mut i = n(0);
    while pl::lt(&i, &num) {
        if !pl::le(&pl::add(&pos, &n(16)), &len) {
            ops_.push(pl::s("<err>"));
            break;
        }
        let mut t = b"x".to_vec();
        t.extend_from_slice(&pos.to_bytes());
        t.extend_from_slice(b"N4");
        let v = unpack(&t, &data)?;
        let (op, l) = (arg(&v, 0), arg(&v, 3));
        let c = conv.get(op.to_bytes());
        ops_.push(if c.truthy() { c } else { pl::concat(&pl::concat(&pl::s("[opcode "), &op), &pl::s("]")) });
        pos = pl::add(&pos, &pl::add(&n(16), &l));
        i = pl::add(&i, &n(1));
    }
    ret(join(b", ", &ops_))
}

/// Print conversion for lens info
/// Inputs: 0) string of values (min focal, max focal, min F, max F)
/// Returns: string in the form "12-20mm f/3.8-4.5" or "50mm f/1.4"
pub fn print_lens_info(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut val = arg(&a, 0);
    let mut vals = pl::split_ws(&val);
    if vals.len() != 4 {
        return ret(val);
    }
    let mut c = 0;
    for v in vals.iter_mut() {
        if is_float(it, v)? {
            c += 1;
            continue;
        }
        if v.eq_str("inf") {
            *v = pl::s("?");
            c += 1;
            continue;
        }
        if v.eq_str("undef") {
            *v = pl::s("?");
            c += 1;
            continue;
        }
    }
    if c != 4 {
        return ret(val);
    }
    val = vals[0].clone();
    // (the Pentax Q writes zero for upper value of fixed-focal-length lenses)
    if vals[1].truthy() && vals[1].to_bytes() != vals[0].to_bytes() {
        val = pl::concat(&val, &pl::concat(&pl::s("-"), &vals[1]));
    }
    val = pl::concat(&val, &pl::concat(&pl::s("mm f/"), &vals[2]));
    if vals[3].truthy() && vals[3].to_bytes() != vals[2].to_bytes() {
        val = pl::concat(&val, &pl::concat(&pl::s("-"), &vals[3]));
    }
    ret(val)
}

/// Get lens info from lens model string
/// Inputs: 0) lens string, 1) flag to allow unknown "?" values
/// Returns: 0) min focal, 1) max focal, 2) min aperture, 3) max aperture
/// Notes: returns empty list if lens string could not be parsed
pub fn get_lens_info(_it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let rtn = get_lens_info_list(&arg(&a, 0), &arg(&a, 1))?;
    match rtn {
        None => pl::ret_nothing(want),
        Some(v) if want == Want::List => Ok(v.to_vec()),
        Some(v) => ret(n(v.len() as i64)), // (an array in scalar context)
    }
}

/// GetLensInfo's list (`None` for the empty list).
fn get_lens_info_list(lens: &Value, unk: &Value) -> ER<Option<[Value; 4]>> {
    // extract focal length and aperture ranges for this lens
    let mut pat = String::from("\\d+(?:\\.\\d+)?");
    if unk.truthy() {
        pat.push_str("|\\?");
    }
    let full = format!("({pat})(?:-({pat}))?\\s*mm.*?(?:[fF]\\/?\\s*)({pat})(?:-({pat}))?");
    let Some(m) = pl::re_dyn(full.as_bytes(), "")?.m(lens) else { return Ok(None) };
    // ($1=short focal, $2=long focal, $3=max aperture wide, $4=max aperture tele)
    let mut av = [cap(&m, 1), cap(&m, 2), cap(&m, 3), cap(&m, 4)];
    if !av[1].truthy() {
        av[1] = av[0].clone();
    }
    if !av[3].truthy() {
        av[3] = av[2].clone();
    }
    if unk.truthy() {
        for x in av.iter_mut() {
            if x.eq_str("?") {
                *x = pl::s("undef");
            }
        }
    }
    Ok(Some(av))
}

/// Match lens in list of possbilities based on value of LensModel
/// Inputs: 0) reference to list of possible models, 1) LensModel string
/// - updates list on return; guaranteed not to remove all list entries
pub fn match_lens_model(_it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (try_, lens_model) = (arg(&a, 0), arg(&a, 1));
    match_lens_model_impl(&try_, &lens_model)?;
    pl::ret_nothing(want)
}

fn match_lens_model_impl(try_: &Value, lens_model: &Value) -> ER<()> {
    if try_.len() > 1 && lens_model.truthy() {
        // filter by focal length
        if let Some(m) = re(r"((\d+-)?\d+mm)", "").m(lens_model) {
            let focal = cap(&m, 1);
            let r = pl::re_dyn(&focal.to_bytes(), "")?;
            let filt: Vec<Value> = try_.items().into_iter().filter(|x| r.is_match(x)).collect();
            if !filt.is_empty() && filt.len() < try_.len() {
                set_items(try_, filt);
            }
        }
        // filter by aperture
        if try_.len() > 1 {
            if let Some(m) = re(r"(?:F/?|1:)(\d+(\.\d+)?)", "i").m(lens_model) {
                let fnum = cap(&m, 1);
                let mut p = b"(F/?|1:)".to_vec();
                p.extend_from_slice(&fnum.to_bytes());
                p.extend_from_slice(b"(\\b|[A-Z])");
                let r = pl::re_dyn(&p, "i")?;
                let filt: Vec<Value> = try_.items().into_iter().filter(|x| r.is_match(x)).collect();
                if !filt.is_empty() && filt.len() < try_.len() {
                    set_items(try_, filt);
                }
            }
        }
        // filter by model version, and other lens parameters
        for pat in ["I+", "USM"] {
            if try_.len() <= 1 {
                continue;
            }
            let Some(m) = pl::re_dyn(format!("\\b({pat})\\b").as_bytes(), "")?.m(lens_model) else { continue };
            let val = cap(&m, 1);
            let mut p = b"\\b".to_vec();
            p.extend_from_slice(&val.to_bytes());
            p.extend_from_slice(b"\\b");
            let r = pl::re_dyn(&p, "")?;
            let filt: Vec<Value> = try_.items().into_iter().filter(|x| r.is_match(x)).collect();
            if !filt.is_empty() && filt.len() < try_.len() {
                set_items(try_, filt);
            }
        }
    }
    Ok(())
}

/// Translate date into standard EXIF format
/// Inputs: 0) date
/// Returns: date in format '2003:10:22'
/// - bad formats recognized: '2003-10-22','2003/10/22','2003 10 22','20031022'
/// - removes null terminator if it exists
pub fn exif_date(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut date = arg(&a, 0);
    re(r"\0$", "").subst(&mut date, false, |_| Vec::new()); // remove any null terminator
    // separate year:month:day with colons
    // (have seen many other characters, including nulls, used erroneously)
    re(r"(\d{4})[^\d]*(\d{2})[^\d]*(\d{2})$", "").subst(&mut date, false, |c| [&cap(c, 1).to_bytes()[..], b":", &cap(c, 2).to_bytes()[..], b":", &cap(c, 3).to_bytes()[..]].concat());
    ret(date)
}

/// Translate time into standard EXIF format
/// Inputs: 0) time
/// Returns: time in format '10:30:55'
/// - bad formats recognized: '10 30 55', '103055', '103055+0500'
/// - removes null terminator if it exists
/// - leaves time zone intact if specified (eg. '10:30:55+05:00')
pub fn exif_time(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut time = arg(&a, 0);
    if !time.is_undef() {
        time = Value::bytes(crate::perl::tr::tr(&time.to_bytes(), b" ", b":", "").0); // use ':' (not ' ') as a separator
    }
    re(r"\0$", "").subst(&mut time, false, |_| Vec::new()); // remove any null terminator
    // add separators if they don't exist
    re(r"^(\d{2})(\d{2})(\d{2})", "").subst(&mut time, false, |c| [&cap(c, 1).to_bytes()[..], b":", &cap(c, 2).to_bytes()[..], b":", &cap(c, 3).to_bytes()[..]].concat());
    re(r"([+-]\d{2})(\d{2})\s*$", "").subst(&mut time, false, |c| [&cap(c, 1).to_bytes()[..], b":", &cap(c, 2).to_bytes()[..]].concat()); // to timezone too
    ret(time)
}

/// Utility routine to return tag ID string for warnings
/// Inputs: 0) Tag ID, 1) [optional] TagInfo ref
/// Returns: "tag 0xXXXX NAME"
pub fn tag_name(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(tag_name_impl(&arg(&a, 0), &arg(&a, 1)))
}

fn tag_name_impl(tag_id: &Value, tag_info: &Value) -> Value {
    let tag_name = if tag_info.truthy() { pl::concat(&pl::s(" "), &tag_info.get("Name")) } else { pl::s("") };
    pl::sprintf("tag 0x%.4x%s", &[tag_id.clone(), tag_name])
}

/// Get class name of next IFD offset for HtmlDump output
/// Inputs: 0) ExifTool ref, 1) current class ID
/// Returns: 0) new IFD offset name, 1) new class ID including "Offset_" for new offset
///          2) new "Offset_" ID
pub fn next_offset_name(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (o, i, s) = next_offset_name_impl(&arg(&a, 0), &arg(&a, 1));
    Ok(vec![o, i, s])
}

fn next_offset_name_impl(et: &Value, id: &Value) -> (Value, Value, Value) {
    let cur = et.get("OffsetNum");
    et.set("OffsetNum", if !cur.is_undef() { pl::add(&cur, &n(1)) } else { n(0) });
    let off_name = pl::concat(&pl::s("o"), &et.get("OffsetNum"));
    let sid = pl::concat(&pl::s("Offset_"), &off_name);
    let id = pl::concat(&if !id.is_undef() { pl::concat(id, &pl::s(" ")) } else { pl::s("") }, &sid);
    (off_name, id, sid)
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    let p = "Image::ExifTool::Exif::";
    let subs: &[(&str, fn(&mut Interp, Vec<Value>, Want) -> ER<Vec<Value>>)] = &[
        ("BINARY_DATA_LIMIT", binary_data_limit),
        ("AUTOLOAD", autoload),
        ("IdentifyRawFile", identify_raw_file),
        ("CalculateLV", calculate_lv),
        ("CalcScaleFactor35efl", calc_scale_factor35efl),
        ("PrintFraction", print_fraction),
        ("ConvertFraction", convert_fraction),
        ("ConvertExifText", convert_exif_text),
        ("PrintSFR", print_sfr),
        ("PrintParameter", print_parameter),
        ("ConvertParameter", convert_parameter),
        ("RedBlueBalance", red_blue_balance),
        ("PrintExposureTime", print_exposure_time),
        ("PrintFNumber", print_fnumber),
        ("DecodeCFAPattern", decode_cfa_pattern),
        ("PrintCFAPattern", print_cfa_pattern),
        ("PrintOpcode", print_opcode),
        ("PrintLensInfo", print_lens_info),
        ("GetLensInfo", get_lens_info),
        ("MatchLensModel", match_lens_model),
        ("ExifDate", exif_date),
        ("ExifTime", exif_time),
        ("TagName", tag_name),
        ("NextOffsetName", next_offset_name),
    ];
    for (name, f) in subs {
        it.define_native(&format!("{p}{name}"), *f);
    }
}
