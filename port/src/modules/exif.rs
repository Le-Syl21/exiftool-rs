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

/// `Set16u($v)`: the bytes, in the current byte order.
fn set16u(it: &mut Interp, v: &Value) -> ER<Value> {
    pl::call(it, "Image::ExifTool::Set16u", vec![v.clone()])
}

/// `Set32u($v)`
fn set32u(it: &mut Interp, v: &Value) -> ER<Value> {
    pl::call(it, "Image::ExifTool::Set32u", vec![v.clone()])
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

/// Attempt to identify the specific lens if multiple lenses have the same LensType
/// Inputs: 0) ExifTool object ref, 1) LensType print value, 2) PrintConv hash ref,
///         3) LensSpec print value, 4) LensType numerical value, 5) FocalLength,
///         6) MaxAperture, 7) MaxApertureValue, 8) MinFocalLength, 9) MaxFocalLength,
///         10) LensModel, 11) LensFocalRange, 12) LensSpec
pub fn print_lens_id(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, mut lens_type_prt, mut print_conv, lens_spec_prt, mut lens_type, focal_length) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4), arg(&a, 5));
    let (mut max_aperture, max_aperture_value, mut short_focal, mut long_focal, lens_model) = (arg(&a, 6), arg(&a, 7), arg(&a, 8), arg(&a, 9), arg(&a, 10));
    let lens_focal_range = arg(&a, 11);
    // this logic relies on the LensType lookup:
    if lens_type.is_undef() {
        return pl::ret_undef();
    }
    // get print conversion hash if necessary
    if !print_conv.truthy() {
        print_conv = et.path(&["TAG_INFO", "LensType", "PrintConv"]);
    }
    // just copy LensType PrintConv value if it was a lens name
    // (Olympus or Panasonic -- just exclude things like Nikon and Leaf LensType)
    if print_conv.ref_type() != "HASH" {
        if print_conv.ref_type() == "ARRAY" && print_conv.at(0).ref_type() == "HASH" {
            print_conv = print_conv.at(0);
            re(r";.*", "").subst(&mut lens_type_prt, false, |_| Vec::new());
            re(r" .*", "").subst(&mut lens_type, false, |_| Vec::new());
        } else {
            if re(r"mm", "").is_match(&lens_type_prt) {
                return ret(lens_type_prt);
            }
            if re(r"(\d)\/F", "").subst(&mut lens_type_prt, false, |c| [&cap(c, 1).to_bytes()[..], b"mm F"].concat()) > 0 {
                return ret(lens_type_prt);
            }
            return pl::ret_undef();
        }
    }
    // get LensSpec information if available (Sony)
    let (mut sf0, mut lf0, mut sa0, mut la0) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef);
    if lens_spec_prt.truthy() {
        if let Some([w, x, y, z]) = get_lens_info_list(&lens_spec_prt, &Value::Undef)? {
            (sf0, lf0, sa0, la0) = (w, x, y, z);
        }
        if !sa0.truthy() {
            sf0 = Value::Undef; // (make sure aperture isn't zero)
        }
    }
    // use MaxApertureValue if MaxAperture is not available
    if !max_aperture.truthy() {
        max_aperture = max_aperture_value;
    }
    if lens_focal_range.truthy() {
        if let Some(m) = re(r"^(\d+)(?: (?:to )?(\d+))?$", "").m(&lens_focal_range) {
            let two = cap(&m, 2);
            short_focal = cap(&m, 1);
            long_focal = if two.truthy() { two } else { cap(&m, 1) };
        }
    }
    if et.get("Make").eq_str("SONY") {
        if lens_type.eq_str("65535") {
            // patch for manual lens (forum17379)
            let pc = print_conv.get(lens_type.to_bytes());
            if pc.truthy() && !focal_length.truthy() && pl::num_eq(&max_aperture, &n(1)) {
                return ret(pc);
            }
            // handle Sony E-type lenses when LensType2 isn't valid (NEX/ILCE models only)
            if re(r"NEX|ILCE", "").is_match(&et.get("Model")) {
                let sony_etype = crate::runtime::lex_hash(it, "Image::ExifTool::Exif", "sonyEtype");
                if sony_etype.is_empty() {
                    let (mut index, mut did) = (Value::Undef, std::collections::HashSet::<Vec<u8>>::new());
                    crate::runtime::require(it, "Image::ExifTool::Sony")?;
                    let types2 = pkg_hash(it, "Image::ExifTool::Sony::sonyLensTypes2");
                    let mut keys = types2.keys();
                    keys.sort();
                    for k in keys {
                        let mut lens = types2.get(&k);
                        re(r" or .*", "").subst(&mut lens, false, |_| Vec::new());
                        if did.contains(&lens.to_bytes()[..]) {
                            continue;
                        }
                        let i;
                        if index.truthy() {
                            i = pl::concat(&pl::s("65535."), &index);
                            index = pl::add(&index, &n(1));
                        } else {
                            i = n(65535);
                            index = n(1);
                        }
                        sony_etype.set(i.to_bytes(), lens.clone());
                        did.insert(lens.to_bytes().to_vec());
                    }
                }
                print_conv = sony_etype;
            }
        } else if !pl::num_eq(&lens_type, &n(0xff00)) {
            // Patch for Metabones or other adapters on Sony E-mount cameras (ref Jos Roost)
            // Metabones Canon EF to E-mount adapters add 0xef00, 0xbc00 or 0x7700 to the
            // high byte for 2-byte Canon LensType values, so we need to adjust for these.
            // Offset 0xef00 is also used by Sigma MC-11, Fotodiox and Viltrox EF-E adapters.
            // Have to exclude A-mount Sigma Filtermatic with 'odd' LensType=0xff00.
            crate::runtime::require(it, "Image::ExifTool::Minolta")?;
            let metabones_id = pkg_hash(it, "Image::ExifTool::Minolta::metabonesID");
            if metabones_id.get(ops::bit_and(&lens_type, &n(0xff00)).to_bytes()).truthy() {
                let off = if pl::ge(&lens_type, &n(0xef00)) {
                    n(0xef00)
                } else if pl::ge(&lens_type, &n(0xbc00)) {
                    n(0xbc00)
                } else {
                    n(0x7700)
                };
                lens_type = pl::sub(&lens_type, &off);
                crate::runtime::require(it, "Image::ExifTool::Canon")?;
                print_conv = pkg_hash(it, "Image::ExifTool::Canon::canonLensTypes");
                let pc = print_conv.get(lens_type.to_bytes());
                if pc.truthy() {
                    lens_type_prt = pc;
                }
            // Test for Sigma MC-11 SA-E adapter with Sigma SA lens using 0x4900 offset.
            // (upper limit of test cuts off two highest Sigma lenses, but prevents
            // conflict with old Minolta 25xxx and higher ID's)
            } else if pl::ge(&lens_type, &n(0x4900)) && pl::le(&lens_type, &n(0x590a)) {
                crate::runtime::require(it, "Image::ExifTool::Sigma")?;
                lens_type = pl::sub(&lens_type, &n(0x4900));
                print_conv = pkg_hash(it, "Image::ExifTool::Sigma::sigmaLensTypes");
                let pc = print_conv.get(lens_type.to_bytes());
                if pc.truthy() {
                    lens_type_prt = pc;
                }
            }
        }
    // (Min/MaxFocalLength may report the current focal length for Tamron zoom lenses)
    } else if short_focal.truthy() && long_focal.truthy() && (!lens_model.truthy() || !re(r"^TAMRON.*-\d+mm", "").is_match(&lens_model)) {
        // Canon (and some other makes) include makernote information
        // which allows better lens identification
        crate::runtime::require(it, "Image::ExifTool::Canon")?;
        return it.call_named("Image::ExifTool::Canon::PrintLensID", vec![print_conv, lens_type, short_focal, long_focal, max_aperture, lens_model], want);
    }
    let mut lens = print_conv.get(lens_type.to_bytes());
    if !lens.truthy() {
        return ret(if lens_model.truthy() { lens_model } else { lens_type_prt });
    }
    let key = |i: i64| -> Vec<u8> { [&lens_type.to_bytes()[..], b".", i.to_string().as_bytes()].concat() };
    if !print_conv.get(key(1)).truthy() {
        return ret(lens);
    }
    re(r" or .*", "s").subst(&mut lens, false, |_| Vec::new()); // remove everything after "or"
    // make list of all possible matching lenses
    let mut lenses = vec![lens.clone()];
    let mut i = 1;
    while print_conv.get(key(i)).truthy() {
        lenses.push(print_conv.get(key(i)));
        i += 1;
    }
    // attempt to determine actual lens
    let user_lens = pkg_hash(it, "Image::ExifTool::userLens");
    let (mut matches, mut best, mut user, mut diff): (Vec<Value>, Vec<Value>, Vec<Value>, Value) = (Vec::new(), Vec::new(), Vec::new(), Value::Undef);
    for lens in &lenses {
        if user_lens.get(lens.to_bytes()).truthy() {
            user.push(lens.clone());
        }
        // sf = short focal
        // lf = long focal
        // sa = max aperture at short focal
        // la = max aperture at long focal
        let (mut sf, mut lf, mut sa, mut la) = match get_lens_info_list(lens, &Value::Undef)? {
            Some([w, x, y, z]) => (w, x, y, z),
            None => (Value::Undef, Value::Undef, Value::Undef, Value::Undef),
        };
        if !sf.truthy() {
            continue;
        }
        // check against LensSpec parameters if available
        if sf0.truthy() {
            if pl::gt(&pl::abs(&pl::sub(&sf, &sf0)), &Value::Num(0.5))
                || pl::gt(&pl::abs(&pl::sub(&sa, &sa0)), &Value::Num(0.15))
                || pl::gt(&pl::abs(&pl::sub(&lf, &lf0)), &Value::Num(0.5))
                || pl::gt(&pl::abs(&pl::sub(&la, &la0)), &Value::Num(0.15))
            {
                continue;
            }
            // the basic parameters match, but also check against additional lens features:
            // for Sony A and E lenses, the full LensSpec string should match with end of LensType,
            // excluding any part between () at the end, and preceded by a space (the space
            // ensures that e.g. Zeiss Loxia 21mm having LensSpec "E 21mm F2.8" will not be
            // identified as "Sony FE 21mm F2.8 (SEL28F20 + SEL075UWC)")
            if lens_spec_prt.truthy() {
                let mut p = b" ".to_vec();
                p.extend_from_slice(&crate::perl::regex::quotemeta(&lens_spec_prt.to_bytes()));
                p.extend_from_slice(b"( \\(| GM$|$)");
                if pl::re_dyn(&p, "")?.is_match(lens) {
                    best = vec![lens.clone()];
                    break;
                }
            }
            // exactly-matching Sony lens should have been found above, so only add non-Sony lenses
            if !re(r"^Sony ", "").is_match(lens) {
                best.push(lens.clone());
            }
            continue;
        }
        // adjust focal length and aperture if teleconverter is attached (Minolta)
        if let Some(m) = re(r" \+ .*? (\d+(\.\d+)?)x( |$)", "").m(lens) {
            let x = cap(&m, 1);
            sf = pl::mul(&sf, &x);
            lf = pl::mul(&lf, &x);
            sa = pl::mul(&sa, &x);
            la = pl::mul(&la, &x);
        }
        // see if we can rule out this lens using FocalLength and MaxAperture
        if focal_length.truthy() {
            if pl::lt(&focal_length, &pl::sub(&sf, &Value::Num(0.5))) {
                continue;
            }
            if pl::gt(&focal_length, &pl::add(&lf, &Value::Num(0.5))) {
                continue;
            }
        }
        if max_aperture.truthy() {
            // it seems that most manufacturers set MaxAperture and MaxApertureValue
            // to the maximum aperture (smallest F number) for the current focal length
            // of the lens, so assume that MaxAperture varies with focal length and find
            // the closest match (this is somewhat contrary to the EXIF specification which
            // states "The smallest F number of the lens", without mention of focal length)
            if pl::lt(&max_aperture, &pl::sub(&sa, &Value::Num(0.15))) {
                continue; // (0.15 is arbitrary)
            }
            if pl::gt(&max_aperture, &pl::add(&la, &Value::Num(0.15))) {
                continue;
            }
            // now determine the best match for this aperture
            let aa; // approximate maximum aperture at this focal length
            if pl::num_eq(&sf, &lf) || pl::num_eq(&sa, &la) || pl::le(&focal_length, &sf) {
                // either 1) prime lens, 2) fixed-aperture zoom, or 3) zoom at min focal
                aa = sa.clone();
            } else if pl::ge(&focal_length, &lf) {
                aa = la.clone();
            } else {
                // assume a log-log variation of max aperture with focal length
                // (see http://regex.info/blog/2006-10-05/263)
                let t = pl::div(&pl::sub(&log(&la)?, &log(&sa)?), &pl::sub(&log(&lf)?, &log(&sf)?))?;
                aa = exp(&pl::add(&log(&sa)?, &pl::mul(&t, &pl::sub(&log(&focal_length)?, &log(&sf)?))));
                // a linear relationship between 1/FocalLength and 1/MaxAperture fits Sony better (ref 27)
                //$aa = 1 / (1/$sa + (1/$focalLength - 1/$sf) * (1/$la - 1/$sa) / (1/$lf - 1/$sf));
            }
            let d = pl::abs(&pl::sub(&max_aperture, &aa));
            if !diff.is_undef() {
                if pl::gt(&d, &pl::add(&diff, &Value::Num(0.15))) {
                    continue; // (0.15 is arbitrary)
                }
                if pl::lt(&d, &pl::sub(&diff, &Value::Num(0.15))) {
                    best.clear();
                }
            }
            diff = d;
            best.push(lens.clone());
        }
        matches.push(lens.clone());
    }
    // return the user-defined lens if it exists
    if !user.is_empty() {
        // choose the best match if we have more than one
        if user.len() > 1 {
            let mut good = Vec::new();
            for try_ in [&best, &matches] {
                for x in try_.iter() {
                    if user_lens.get(x.to_bytes()).truthy() {
                        good.push(x.clone());
                    }
                }
                if !good.is_empty() {
                    return ret(join(b" or ", &good));
                }
            }
        }
        return ret(join(b" or ", &user));
    }
    // return the best match(es) from the possible lenses, after checking against LensModel
    if best.is_empty() {
        best = matches;
    }
    if !best.is_empty() {
        let b = Value::new_array(best);
        match_lens_model_impl(&b, &lens_model)?;
        return ret(join(b" or ", &b.items()));
    }
    let lens = print_conv.get(lens_type.to_bytes());
    if lens_model.truthy() && re(r" or ", "").is_match(&lens) {
        return ret(lens_model); // (eg. Sony NEX-5N)
    }
    ret(lens)
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

/// Generate TIFF file from scratch (in current byte order)
/// Inputs: 0) hash of IFD entries (TagID => Value; multiple values space-delimited)
///         1) raw image data reference
/// Returns: TIFF image data, or undef on error
pub fn generate_tiff(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(generate_tiff_impl(it, &arg(&a, 0), &arg(&a, 1))?)
}

fn generate_tiff_impl(it: &mut Interp, entries: &Value, data_pt: &Value) -> ER<Value> {
    let mut offset_pos = Value::Undef;

    let num = n(entries.len() as i64);
    let mut ifd_buff = get_byte_order(it)?;
    for x in [set16u(it, &n(42))?, set32u(it, &n(8))?, set16u(it, &num)?] {
        ifd_buff = pl::concat(&ifd_buff, &x);
    }
    let mut val_buff = pl::s("");
    let tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![pl::s("Image::ExifTool::Exif::Main")])?;
    let format_number = pkg_hash(it, "Image::ExifTool::Exif::formatNumber");
    let format_size = pkg_array(it, "Image::ExifTool::Exif::formatSize");
    let mut keys: Vec<Value> = entries.keys().into_iter().map(Value::from).collect();
    keys.sort_by(|x, y| ops::num_cmp(x, y).unwrap_or(std::cmp::Ordering::Equal));
    for tag in keys {
        let tag_info = tag_table_ptr.get(tag.to_bytes());
        let fmt = if tag_info.ref_type() == "HASH" { tag_info.get("Writable") } else { pl::s("int32u") };
        if fmt.is_undef() {
            return Ok(Value::Undef);
        }
        let mut val = pl::call(it, "Image::ExifTool::WriteValue", vec![entries.get(tag.to_bytes()), fmt.clone(), n(-1)])?;
        if val.is_undef() {
            return Ok(Value::Undef);
        }
        let format = format_number.get(fmt.to_bytes());
        let cnt = pl::div(&pl::length(&val), &format_size.at(format.to_i64()))?;
        for x in [set16u(it, &tag)?, set16u(it, &format)?, set32u(it, &cnt)?] {
            ifd_buff = pl::concat(&ifd_buff, &x);
        }
        if pl::num_eq(&tag, &n(0x111)) {
            offset_pos = pl::length(&ifd_buff); // (remember StripOffsets position)
        }
        if pl::gt(&pl::length(&val), &n(4)) {
            let off = pl::add(&pl::add(&pl::add(&n(10), &pl::mul(&n(12), &num)), &n(4)), &pl::length(&val_buff));
            let x = set32u(it, &off)?;
            ifd_buff = pl::concat(&ifd_buff, &x);
            val_buff = pl::concat(&val_buff, &val);
        } else {
            if pl::lt(&pl::length(&val), &n(4)) {
                let pad = 4 - pl::length(&val).to_i64();
                val = pl::concat(&val, &Value::bytes(vec![0u8; pad.max(0) as usize]));
            }
            ifd_buff = pl::concat(&ifd_buff, &val);
        }
    }
    ifd_buff = pl::concat(&ifd_buff, &Value::bytes(b"\0\0\0\0")); // (no IFD1)
    if !offset_pos.truthy() {
        return Ok(Value::Undef);
    }
    let r = sref(ifd_buff);
    let total = pl::add(&pl::length(&r.deref()), &pl::length(&val_buff));
    pl::call(it, "Image::ExifTool::Set32u", vec![total, r.clone(), offset_pos])?;
    Ok(pl::concat(&pl::concat(&r.deref(), &val_buff), &data_pt.deref()))
}

/// Rebuild TIFF thumbnail(s)/preview(s) into stand-alone files with current byte order
/// Inputs: 0) ExifTool ref, 1) SubfileType, 2) Compression, 3) ImageWidth, 4) ImageHeight,
///         5) BitsPerSample, 6) PhotometricInterpretation, 7) StripOffsets, 8) SamplesPerPixel,
///         9) RowsPerStrip, 10) StripByteCounts, 10) PlanarConfiguration, 11) Orientation
/// Returns: 0) TIFF image or undef, 1/2) Family 0/1 groups for TIFF preview IFD
pub fn rebuild_tiff(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let et = arg(&a, 0);
    let value = et.get("VALUE");
    let (mut rtn, mut grp0, mut grp1) = (Value::Undef, Value::Undef, Value::Undef);
    if et.get("FILE_TYPE").eq_str("RWZ") {
        return pl::ret_undef();
    }
    let mut i: i64 = 0;
    'sub_file: loop {
        let key = pl::s(&if i != 0 { format!("SubfileType ({i})") } else { "SubfileType".to_string() });
        i += 1;
        if value.get(key.to_bytes()).is_undef() {
            break;
        }
        if !pl::num_eq(&value.get(key.to_bytes()), &n(1)) {
            continue; // (reduced-resolution image)
        }
        let grp = pl::method(it, &et, "GetGroup", vec![key.clone(), n(1)])?;
        let cmp = pl::method(it, &et, "FindValue", vec![pl::s("Compression"), grp.clone()])?;
        if !pl::num_eq(&cmp, &n(1)) {
            continue; // (no compression)
        }
        let vals = Value::new_hash();
        vals.set("Compression", cmp);
        vals.set("PlanarConfiguration", n(1));
        vals.set("Orientation", n(1));
        for t in ["ImageWidth", "ImageHeight", "BitsPerSample", "PhotometricInterpretation", "StripOffsets", "SamplesPerPixel", "RowsPerStrip", "StripByteCounts", "PlanarConfiguration", "Orientation"] {
            let val = pl::method(it, &et, "FindValue", vec![pl::s(t), grp.clone()])?;
            if !val.is_undef() {
                vals.set(t, val);
                continue;
            }
            if vals.get(t).is_undef() {
                continue 'sub_file;
            }
        }
        let (w, h) = (vals.get("ImageWidth"), vals.get("ImageHeight"));
        let bits = pl::split_ws(&vals.get("BitsPerSample"));
        let mut row_bytes = n(0);
        for b in &bits {
            row_bytes = pl::add(&row_bytes, &pl::mul(&w, &pl::int(&pl::div(&pl::add(b, &n(7)), &n(8))?)));
        }
        let mut dat = pl::s("");
        let off = pl::split_ws(&vals.get("StripOffsets"));
        let len = pl::split_ws(&vals.get("StripByteCounts"));
        // read the image data
        for j in 0..off.len() {
            if !pl::num_eq(&arg(&len, j), &pl::mul(&row_bytes, &vals.get("RowsPerStrip"))) {
                continue 'sub_file;
            }
            let tmp = pl::method(it, &et, "ExtractBinary", vec![off[j].clone(), arg(&len, j)])?;
            if tmp.is_undef() {
                continue 'sub_file;
            }
            dat = pl::concat(&dat, &tmp);
        }
        // generate the TIFF image
        let entries = Value::new_hash();
        for (k, v) in [
            (0x0fe, n(0)),                                  // SubfileType = 0
            (0x100, w.clone()),                             // ImageWidth
            (0x101, h.clone()),                             // ImageHeight
            (0x102, vals.get("BitsPerSample")),             // BitsPerSample
            (0x103, vals.get("Compression")),               // Compression
            (0x106, vals.get("PhotometricInterpretation")), // PhotometricInterpretation
            (0x111, n(0)),                                  // StripOffsets (will be adjusted later)
            (0x112, vals.get("Orientation")),               // Orientation
            (0x115, vals.get("SamplesPerPixel")),           // SamplesPerPixel
            (0x116, h.clone()),                             // RowsPerStrip
            (0x117, pl::mul(&h, &row_bytes)),               // StripByteCounts
            (0x11a, n(72)),                                 // XResolution = 72
            (0x11b, n(72)),                                 // YResolution = 72
            (0x11c, vals.get("PlanarConfiguration")),       // PlanarConfiguration
            (0x128, n(2)),                                  // ResolutionUnit = 2
        ] {
            entries.set(k.to_string(), v);
        }
        let img = generate_tiff_impl(it, &entries, &sref(dat))?;

        if img.is_undef() {
            let msg = format!("Invalid {}TIFF data", if pl::gt(&w, &n(256)) { "Preview" } else { "Thumbnail" });
            pl::method(it, &et, "Warn", vec![pl::s(&msg)])?;
        } else if rtn.truthy() || pl::gt(&w, &n(256)) {
            // (call it a preview if larger than 256 pixels)
            let mut args = vec![pl::s("PreviewTIFF"), sref(img)];
            args.extend(pl::method_list(it, &et, "GetGroup", vec![key.clone()])?);
            pl::method(it, &et, "FoundTag", args)?;
        } else {
            rtn = sref(img);
            let g = pl::method_list(it, &et, "GetGroup", vec![key.clone()])?;
            grp0 = arg(&g, 0);
            grp1 = arg(&g, 1);
        }
    }
    if want != Want::List {
        return ret(rtn);
    }
    Ok(vec![rtn, grp0, grp1])
}

/// Extract image from file
/// Inputs: 0) ExifTool object reference, 1) data offset (in file), 2) data length
///         3) [optional] tag name
/// Returns: Reference to Image if specifically requested or "Binary data" message
///          Returns undef if there was an error loading the image
pub fn extract_image(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, offset, len, tag) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    // my $dataPt = \$$et{EXIF_DATA};  (the reference creates the element)
    if !et.exists("EXIF_DATA") {
        et.set("EXIF_DATA", Value::Undef);
    }
    let data = et.get("EXIF_DATA");
    let data_pos = et.get("EXIF_POS");
    let mut image;

    // no image if length is zero, and don't try to extract binary from XMP file
    if !len.truthy() || et.get("FILE_TYPE").eq_str("XMP") {
        return pl::ret_undef();
    }

    // take data from EXIF block if possible
    if !data_pos.is_undef() && pl::ge(&offset, &data_pos) && pl::le(&pl::add(&offset, &len), &pl::add(&data_pos, &pl::length(&data))) {
        image = substr(&data, pl::sub(&offset, &data_pos).to_i64(), Some(len.to_i64()));
    } else {
        image = pl::method(it, &et, "ExtractBinary", vec![offset.clone(), len.clone(), tag.clone()])?;
        if image.is_undef() {
            return pl::ret_undef();
        }
        // patch for incorrect ThumbnailOffset in some Sony DSLR-A100 ARW images
        if tag.truthy() && tag.eq_str("ThumbnailImage") && et.get("TIFF_TYPE").eq_str("ARW") && et.get("Model").eq_str("DSLR-A100") && pl::lt(&offset, &n(0x10000)) && !re(r"^(Binary data|\xff\xd8\xff)", "").is_match(&image) {
            let try_ = pl::method(it, &et, "ExtractBinary", vec![pl::add(&offset, &n(0x10000)), len.clone(), tag.clone()])?;
            if !try_.is_undef() && re(r"^\xff\xd8\xff", "").is_match(&try_) {
                image = try_;
                let v = et.get("VALUE");
                v.set("ThumbnailOffset", pl::add(&v.get("ThumbnailOffset"), &n(0x10000)));
                pl::method(it, &et, "Warn", vec![pl::s("Adjusted incorrect A100 ThumbnailOffset"), n(1)])?;
            }
        }
    }
    it.call_method(&et, "ValidateImage", vec![et.clone(), sref(image), tag], want)
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

/// `$et->Warn(MSG)` / `$et->Warn(MSG, MINOR)`: its return value.
fn et_warn(it: &mut Interp, et: &Value, msg: Value, minor: Option<Value>) -> ER<Value> {
    let mut args = vec![msg];
    if let Some(m) = minor {
        args.push(m);
    }
    pl::method(it, et, "Warn", args)
}

/// `Get16u($dataPt, $pos)`
fn get16u(it: &mut Interp, data_pt: &Value, pos: &Value) -> ER<Value> {
    pl::call(it, "Image::ExifTool::Get16u", vec![data_pt.clone(), pos.clone()])
}

/// `Get32u($dataPt, $pos)`
fn get32u(it: &mut Interp, data_pt: &Value, pos: &Value) -> ER<Value> {
    pl::call(it, "Image::ExifTool::Get32u", vec![data_pt.clone(), pos.clone()])
}

/// `SetByteOrder($order)`
fn set_byte_order(it: &mut Interp, order: &Value) -> ER<Value> {
    pl::call(it, "Image::ExifTool::SetByteOrder", vec![order.clone()])
}

/// `$obj->METHOD(ARGS)` with the arguments aliased (`$raf->Read($buff, $n)`
/// writes `$buff`): the values written back are in `args` after the call.
fn method_aliased(it: &mut Interp, obj: &Value, name: &str, args: &mut [Value]) -> ER<Value> {
    let scope = Scope::new(None);
    let o = scope.scalar("o", obj.clone());
    let cells: Vec<_> = args.iter().enumerate().map(|(i, v)| scope.scalar(&format!("a{i}"), v.clone())).collect();
    let list: Vec<String> = (0..args.len()).map(|i| format!("$a{i}")).collect();
    let code = format!("$o->{name}({})", list.join(","));
    let saved_err = it.get_scalar("main::@");
    let r = it.eval_string(code.as_bytes(), "Image::ExifTool::Exif", &scope, Want::Scalar);
    it.set_scalar("main::@", saved_err);
    drop(o);
    for (i, c) in cells.iter().enumerate() {
        args[i] = c.borrow().clone();
    }
    match r {
        Ok(v) => Ok(v.last().cloned().unwrap_or_default()),
        Err(e) => Err(Flow::Die(e)),
    }
}

/// `eval $code` at a ProcessExif eval site, with the variables the site has
/// in scope: the result (undef when the code dies, `$@` set as perl does)
/// and the variables' values afterwards.
fn eval_site(it: &mut Interp, code: &Value, vars: &[(&str, Value)], want: Want) -> (Value, Vec<Value>) {
    let scope = Scope::new(None);
    let cells: Vec<_> = vars.iter().map(|(name, v)| scope.scalar(name, v.clone())).collect();
    let r = it.eval_string(&code.to_bytes(), "Image::ExifTool::Exif", &scope, want);
    let v = match r {
        Ok(v) => v.last().cloned().unwrap_or_default(),
        Err(_) => Value::Undef,
    };
    let after = cells.iter().map(|c| c.borrow().clone()).collect();
    (v, after)
}

/// `$s =~ tr/\x00-\x1f\x7f-\xff/./`
fn tr_unprintable(s: &Value) -> Value {
    Value::bytes(crate::perl::tr::tr(&s.to_bytes(), b"\\x00-\\x1f\\x7f-\\xff", b".", "").0)
}

/// `$$et{INDENT} =~ s/..$//`
fn indent_trim(et: &Value) {
    let mut ind = et.get("INDENT");
    if re(r"..$", "").subst(&mut ind, false, |_| Vec::new()) > 0 {
        et.set("INDENT", ind);
    }
}

/// Process EXIF directory
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success, otherwise returns 0 and sets a Warning
pub fn process_exif(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let or0 = |v: Value| if v.truthy() { v } else { n(0) };
    let mut data_pt = dir_info.get("DataPt");
    let mut data_pos = or0(dir_info.get("DataPos"));
    let mut data_len = dir_info.get("DataLen");
    let mut dir_start = or0(dir_info.get("DirStart"));
    let mut dir_len = {
        let v = dir_info.get("DirLen");
        if v.truthy() {
            v
        } else {
            pl::sub(&data_len, &dir_start)
        }
    };
    let mut dir_name = dir_info.get("DirName");
    let mut base = or0(dir_info.get("Base"));
    let first_base = base.clone();
    let raf = dir_info.get("RAF");
    let options = et.get("OPTIONS");
    let (mut verbose, validate, mut save_format, save_bin) = (options.get("Verbose"), options.get("Validate"), options.get("SaveFormat"), options.get("SaveBin"));
    let html_dump = et.get("HTML_DUMP");
    let mut success = n(1);
    let (mut tag_key, mut dir_size, mut maker_addr, mut str_enc, offset_info, mut off_name, mut next_off_name, mut do_hash);
    tag_key = Value::Undef;
    dir_size = Value::Undef;
    maker_addr = Value::Undef;
    str_enc = Value::Undef;
    offset_info = Value::new_hash();
    off_name = Value::Undef;
    next_off_name = Value::Undef;
    do_hash = Value::Undef;
    let _ = &tag_key;
    let in_maker_notes = tag_table_ptr.path(&["GROUPS", "0"]).eq_str("MakerNotes");
    let exif_main = pkg_hash(it, "Image::ExifTool::Exif::Main");
    let is_exif = tag_table_ptr.same_ref(&exif_main);
    let format_name = pkg_array(it, "Image::ExifTool::Exif::formatName");
    let format_size = pkg_array(it, "Image::ExifTool::Exif::formatSize");
    let format_number = pkg_hash(it, "Image::ExifTool::Exif::formatNumber");
    let int_format = pkg_hash(it, "Image::ExifTool::Exif::intFormat");
    let save_for_validate = pkg_hash(it, "Image::ExifTool::Exif::saveForValidate");
    let limit = n(10 * 1024 * 1024); // BINARY_DATA_LIMIT

    // warn for incorrect maker notes in CR3 files
    if dir_name.eq_str("MakerNotes") {
        if et.get("FileType").eq_str("CR3") && dir_info.get("Parent").truthy() && dir_info.get("Parent").eq_str("ExifIFD") {
            et_warn(it, &et, pl::s("MakerNotes shouldn't exist ExifIFD of CR3 image"), Some(n(1)))?;
        }
        if dir_info.get("TagInfo").truthy() && dir_info.get("TagInfo").get("MakerNotes").truthy() && et.get("ExifByteOrder").truthy() {
            let bo = get_byte_order(it)?;
            if et.get("ExifByteOrder").to_bytes() != bo.to_bytes() {
                let bo = get_byte_order(it)?;
                pl::method(it, &et, "FoundTag", vec![pl::s("MakerNoteByteOrder"), bo])?;
            }
        }
    }
    // set flag to calculate image data hash if requested
    if et.get("ImageDataHash").truthy() && ((et.get("FILE_TYPE").eq_str("TIFF") && !base.truthy() && !in_maker_notes) || (et.get("FILE_TYPE").eq_str("RAF") && dir_name.eq_str("FujiIFD"))) {
        do_hash = n(1);
    }

    // set encoding to assume for strings
    if tag_table_ptr.path(&["GROUPS", "0"]).eq_str("EXIF") {
        str_enc = pl::method(it, &et, "Options", vec![pl::s("CharsetEXIF")])?;
    }

    // ignore non-standard EXIF while in strict MWG compatibility mode
    let mwg_strict = it.get_scalar("Image::ExifTool::MWG::strict");
    if (validate.truthy() || mwg_strict.truthy()) && dir_name.eq_str("IFD0") && is_exif && re(r"^(JPEG|TIFF|PSD)$", "").is_match(&et.get("FILE_TYPE")) {
        let path = pl::method(it, &et, "MetadataPath", vec![])?;
        if re(r"^(JPEG-APP1-IFD0|TIFF-IFD0|PSD-EXIFInfo-IFD0)$", "").is_match(&path) {
            if !et.get("DOC_NUM").truthy() {
                if et.get("HasExif").truthy() {
                    et_warn(it, &et, pl::concat(&pl::s("Duplicate EXIF at "), &path), None)?;
                }
                et.set("HasExif", n(1));
            }
        } else if mwg_strict.truthy() {
            et_warn(it, &et, pl::concat(&pl::s("Ignored non-standard EXIF at "), &path), None)?;
            return ret(n(0));
        } else {
            et_warn(it, &et, pl::concat(&pl::s("Non-standard EXIF at "), &path), Some(n(1)))?;
        }
    }
    // mix htmlDump and Validate into verbose so we can test for all at once
    if html_dump.truthy() {
        verbose = n(-1);
    }
    if validate.truthy() && !verbose.truthy() {
        verbose = n(-2);
    }
    if dir_name.eq_str("EXIF") {
        dir_name = pl::s("IFD0");
        dir_info.set("DirName", dir_name.clone());
    }
    if re(r"^(IFD0|SubIFD)$", "").is_match(&dir_name) && dir_info.get("Multi").is_undef() {
        dir_info.set("Multi", n(1));
    }
    // get a more descriptive name for MakerNote sub-directories
    let mut dir = dir_info.get("Name");
    if !(dir.truthy() && in_maker_notes && !re(r"^MakerNote", "").is_match(&dir)) {
        dir = dir_name.clone();
    }
    let dir_s = |d: &Value| d.to_bytes().to_vec();

    let (mut num_entries, mut dir_end) = (Value::Undef, Value::Undef);
    if pl::ge(&dir_start, &n(0)) && pl::le(&dir_start, &pl::sub(&data_len, &n(2))) {
        // make sure data is large enough (patches bug in Olympus subdirectory lengths)
        num_entries = get16u(it, &data_pt, &dir_start)?;
        dir_size = pl::add(&n(2), &pl::mul(&n(12), &num_entries));
        dir_end = pl::add(&dir_start, &dir_size);
        if pl::gt(&dir_size, &dir_len) {
            if (pl::gt(&verbose, &n(0)) || validate.truthy()) && !dir_info.get("SubIFD").truthy() {
                let short = pl::sub(&dir_size, &dir_len);
                indent_trim(&et); // keep indent the same
                let msg = [&b"Short directory size for "[..], &dir_s(&dir), b" (missing ", &short.to_bytes(), b" bytes)"].concat();
                et_warn(it, &et, Value::bytes(msg), None)?;
                et.set("INDENT", pl::concat(&et.get("INDENT"), &pl::s("| ")));
            }
            if pl::gt(&dir_end, &data_len) {
                dir_size = Value::Undef; // read from file if necessary
            }
        }
    }
    // read IFD from file if necessary
    if !dir_size.truthy() {
        success = n(0);
        if raf.truthy() {
            // read the count of entries in this IFD
            let offset = pl::add(&dir_start, &data_pos);
            let (mut buff, mut buf2) = (Value::Undef, Value::Undef);
            let sought = pl::method(it, &raf, "Seek", vec![pl::add(&offset, &base), n(0)])?;
            let ok = sought.truthy() && {
                let mut ra = [buff.clone(), n(2)];
                let r = method_aliased(it, &raf, "Read", &mut ra)?;
                buff = ra[0].clone();
                pl::num_eq(&r, &n(2))
            };
            if ok {
                let len = pl::mul(&n(12), &get16u(it, &sref(buff.clone()), &n(0))?);
                // also read next IFD pointer if available
                let mut ra = [buf2.clone(), pl::add(&len, &n(4))];
                let r = method_aliased(it, &raf, "Read", &mut ra)?;
                buf2 = ra[0].clone();
                if pl::ge(&r, &len) {
                    buff = pl::concat(&buff, &buf2);
                    data_pt = sref(buff.clone());
                    dir_info.set("DataPt", data_pt.clone());
                    data_pos = offset.clone();
                    dir_info.set("DataPos", data_pos.clone());
                    data_len = pl::length(&buff);
                    dir_info.set("DataLen", data_len.clone());
                    dir_start = n(0);
                    dir_info.set("DirStart", dir_start.clone());
                    dir_len = pl::length(&buff);
                    dir_info.set("DirLen", dir_len.clone());
                    success = n(1);
                }
            }
        }
        if success.truthy() {
            num_entries = get16u(it, &data_pt, &dir_start)?;
        } else {
            et_warn(it, &et, Value::bytes([&b"Bad "[..], &dir_s(&dir), b" directory"].concat()), Some(Value::from(in_maker_notes)))?;
            if !(in_maker_notes && pl::ge(&dir_len, &n(14)) && pl::ge(&dir_start, &n(0)) && pl::le(&pl::add(&dir_start, &dir_len), &pl::length(&data_pt.deref()))) {
                return ret(n(0));
            }
            dir_size = dir_len.clone();
            num_entries = pl::int(&pl::div(&pl::sub(&dir_size, &n(2)), &n(12))?); // read what we can
            pl::call(it, "Image::ExifTool::Set16u", vec![num_entries.clone(), data_pt.clone(), dir_start.clone()])?;
        }
        dir_size = pl::add(&n(2), &pl::mul(&n(12), &num_entries));
        dir_end = pl::add(&dir_start, &dir_size);
    }
    let _ = &dir_size;
    if pl::gt(&verbose, &n(0)) {
        let bo = get_byte_order(it)?;
        pl::method(it, &et, "VerboseDir", vec![dir_name.clone(), num_entries.clone(), Value::Undef, bo])?;
    }
    let bytes_from_end = pl::sub(&data_len, &dir_end);
    if pl::lt(&bytes_from_end, &n(4)) && !(pl::num_eq(&bytes_from_end, &n(2)) || pl::num_eq(&bytes_from_end, &n(0))) {
        let msg = [&b"Illegal "[..], &dir_s(&dir), b" directory size (", &num_entries.to_bytes(), b" entries)"].concat();
        et_warn(it, &et, Value::bytes(msg), None)?;
        return ret(n(0));
    }
    // fix base offset for maker notes if necessary
    if !dir_info.get("MakerNoteAddr").is_undef() {
        maker_addr = dir_info.get("MakerNoteAddr");
        dir_info.delete("MakerNoteAddr");
        if pl::call(it, "Image::ExifTool::MakerNotes::FixBase", vec![et.clone(), dir_info.clone()])?.truthy() {
            base = dir_info.get("Base");
            data_pos = dir_info.get("DataPos");
        }
    }
    if html_dump.truthy() {
        off_name = dir_info.get("OffsetName");
        let long_name = if dir.eq_str("MakerNotes") {
            let nm = dir_info.get("Name");
            if nm.truthy() {
                nm
            } else {
                dir.clone()
            }
        } else {
            dir.clone()
        };
        if !maker_addr.is_undef() {
            let hdr_len = pl::sub(&pl::add(&pl::add(&dir_start, &data_pos), &base), &maker_addr);
            if pl::gt(&hdr_len, &n(0)) {
                pl::method(it, &et, "HDump", vec![maker_addr.clone(), hdr_len, pl::s("MakerNotes header"), long_name.clone()])?;
            }
        }
        if !dir_info.get("NoDumpEntryCount").truthy() {
            let pos = pl::add(&pl::add(&dir_start, &data_pos), &base);
            pl::method(it, &et, "HDump", vec![pos, n(2), pl::concat(&long_name, &pl::s(" entries")), pl::concat(&pl::s("Entry count: "), &num_entries), Value::Undef, off_name.clone()])?;
        }
        let mut tip = Value::Undef;
        let mut id = off_name.clone();
        if pl::ge(&bytes_from_end, &n(4)) {
            let nxt = match re(r"^(.*?)(\d+)$", "").m(&dir) {
                Some(m) => pl::concat(&cap(&m, 1), &pl::add(&cap(&m, 2), &n(1))),
                None => pl::s("Next IFD"),
            };
            let off = get32u(it, &data_pt, &dir_end)?;
            tip = pl::sprintf_v(&pl::concat(&nxt, &pl::s(" offset: 0x%.4x")), &[off.clone()]);
            if off.truthy() {
                let (o, i, _) = next_offset_name_impl(&et, &off_name);
                next_off_name = o;
                id = i;
            }
        }
        let pos = pl::add(&pl::add(&dir_end, &data_pos), &base);
        pl::method(it, &et, "HDump", vec![pos, n(4), pl::s("Next IFD"), tip, n(0), id])?;
    }

    // patch for Canon EOS 40D firmware 1.0.4 bug (incorrect directory counts)
    // (must do this before parsing directory or CameraSettings offset will be suspicious)
    if in_maker_notes && et.get("Model").eq_str("Canon EOS 40D") && num_entries.truthy() {
        let entry = pl::add(&pl::add(&dir_start, &n(2)), &pl::mul(&n(12), &pl::sub(&num_entries, &n(1))));
        let fmt = get16u(it, &data_pt, &pl::add(&entry, &n(2)))?;
        if pl::lt(&fmt, &n(1)) || pl::gt(&fmt, &n(13)) {
            let pos = pl::add(&pl::add(&entry, &data_pos), &base);
            pl::method(it, &et, "HDump", vec![pos, n(12), pl::s("[invalid IFD entry]"), pl::concat(&pl::s("Bad format type: "), &fmt), n(1), off_name.clone()])?;
            // adjust the number of directory entries
            num_entries = pl::sub(&num_entries, &n(1));
            dir_end = pl::sub(&dir_end, &n(12));
        }
    }

    // make sure that Compression and SubfileType are defined for this IFD (for Condition's)
    et.set("SubfileType", pl::s(""));
    et.set("Compression", pl::s(""));

    // loop through all entries in an EXIF directory (IFD)
    let (mut val_end, mut off_list, mut off_hash, mut map_fmt) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef);
    let mut val_pos: Vec<Value> = Vec::new();
    if tag_table_ptr.get("VARS").truthy() {
        map_fmt = tag_table_ptr.path(&["VARS", "MAP_FORMAT"]);
    }

    let (mut warn_count, mut last_id) = (n(0), n(-1));
    let mut index = n(0);
    'entry: loop {
        if !pl::lt(&index, &num_entries) {
            break;
        }
        let cur_index = index.clone();
        index = pl::add(&index, &n(1));
        let index = cur_index;
        if pl::gt(&warn_count, &n(10)) {
            let msg = [&b"Too many warnings -- "[..], &dir_s(&dir), b" parsing aborted"].concat();
            if et_warn(it, &et, Value::bytes(msg), Some(n(2)))?.truthy() {
                return ret(n(0));
            }
        }
        let entry = pl::add(&pl::add(&dir_start, &n(2)), &pl::mul(&n(12), &index));
        let tag_id = get16u(it, &data_pt, &entry)?;
        let mut format = get16u(it, &data_pt, &pl::add(&entry, &n(2)))?;
        let mut count = get32u(it, &data_pt, &pl::add(&entry, &n(4)))?;
        // (Apple uses the BigTIFF format code 16 in the maker notes of their ProRaw DNG files)
        if (pl::lt(&format, &n(1)) || pl::gt(&format, &n(13))) && !pl::num_eq(&format, &n(129)) && !(pl::num_eq(&format, &n(16)) && et.get("Make").eq_str("Apple") && in_maker_notes) {
            if map_fmt.truthy() && map_fmt.get(format.to_bytes()).truthy() {
                format = map_fmt.get(format.to_bytes());
            } else {
                let pos = pl::add(&pl::add(&entry, &data_pos), &base);
                pl::method(it, &et, "HDump", vec![pos, n(12), pl::s("[invalid IFD entry]"), pl::concat(&pl::s("Bad format type: "), &format), n(1), off_name.clone()])?;
                // warn unless the IFD was just padded with zeros
                if format.truthy() || validate.truthy() {
                    let msg = [&b"Bad format ("[..], &format.to_bytes(), b") for ", &dir_s(&dir), b" entry ", &index.to_bytes()].concat();
                    et_warn(it, &et, Value::bytes(msg), Some(Value::from(in_maker_notes)))?;
                    warn_count = pl::add(&warn_count, &n(1));
                }
                // assume corrupted IFD if this is our first entry (except Sony ILCE which have an empty first entry)
                if index.truthy() || re(r"^ILCE", "").is_match(&et.get("Model")) {
                    continue 'entry;
                }
                // $et->Warn(sprintf('Format code 0x%x encountered -- Possibly corrupted IFD'));
                return ret(n(0));
            }
        }
        let mut format_str = format_name.at(format.to_i64()); // get name of this format
        let mut value_data_pt = data_pt.clone();
        let mut value_data_pos = data_pos.clone();
        let mut value_data_len = data_len.clone();
        let mut value_ptr = pl::add(&entry, &n(8)); // pointer to value within $$dataPt
        let mut tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag_id.clone()])?;
        let (mut orig_form_str, mut bad, mut rational, mut bin_val, mut sub_off_name) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef, Value::Undef);
        // save the EXIF format codes if requested
        if save_format.truthy() {
            save_format = format_str.clone();
            et.get_or_new_hash("SaveFormat").set(save_format.to_bytes(), n(1));
        }
        // hack to patch incorrect count in Kodak SubIFD3 tags
        let tag_entry = tag_table_ptr.get(tag_id.to_bytes());
        if pl::lt(&count, &n(2)) && tag_entry.ref_type() == "HASH" && tag_entry.get("FixCount").truthy() {
            if !off_list.truthy() {
                let r = pl::call_list(it, "Image::ExifTool::Exif::GetOffList", vec![data_pt.clone(), dir_start.clone(), data_pos.clone(), num_entries.clone(), tag_table_ptr.clone()])?;
                off_list = arg(&r, 0);
                off_hash = arg(&r, 1);
            }
            let k = get32u(it, &data_pt, &value_ptr)?;
            let i = off_hash.get(k.to_bytes());
            if !i.is_undef() && pl::lt(&i, &n(off_list.len() as i64 - 1)) {
                let old_count = count.clone();
                count = pl::int(&pl::div(&pl::sub(&off_list.at(pl::add(&i, &n(1)).to_i64()), &off_list.at(i.to_i64())), &format_size.at(format.to_i64()))?);
                if !pl::num_eq(&old_count, &count) {
                    orig_form_str = Value::bytes([&format_name.at(format.to_i64()).to_bytes()[..], b"[", &old_count.to_bytes(), b"]"].concat());
                }
            }
        }
        if validate.truthy() && !in_maker_notes {
            pl::call(it, "Image::ExifTool::Validate::ValidateExif", vec![et.clone(), tag_table_ptr.clone(), tag_id.clone(), tag_info.clone(), last_id.clone(), dir.clone(), count.clone(), format_str.clone()])?;
        }
        let mut size = pl::mul(&count, &format_size.at(format.to_i64()));
        let mut read_size = size.clone();
        if pl::gt(&size, &n(4)) {
            if pl::gt(&size, &n(0x7fffffff)) && (!tag_info.truthy() || !tag_info.get("ReadFromRAF").truthy()) {
                let msg = pl::sprintf("Invalid size (%u) for %s %s", &[size.clone(), dir.clone(), tag_name_impl(&tag_id, &tag_info)]);
                et_warn(it, &et, msg, Some(Value::from(in_maker_notes)))?;
                warn_count = pl::add(&warn_count, &n(1));
                continue 'entry;
            }
            value_ptr = get32u(it, &data_pt, &value_ptr)?;
            if validate.truthy() && !in_maker_notes {
                let tag_name = tag_name_impl(&tag_id, &tag_info);
                if ops::bit_and(&value_ptr, &n(1)).truthy() {
                    let msg = [&b"Odd offset for "[..], &dir_s(&dir), b" ", &tag_name.to_bytes()].concat();
                    et_warn(it, &et, Value::bytes(msg), Some(n(1)))?;
                }
                let file_size = et.get("VALUE").get("FileSize");
                if pl::lt(&value_ptr, &n(8)) || (pl::gt(&pl::add(&value_ptr, &size), &pl::length(&data_pt.deref())) && pl::gt(&pl::add(&value_ptr, &size), &file_size)) {
                    let msg = [&b"Invalid offset for "[..], &dir_s(&dir), b" ", &tag_name.to_bytes()].concat();
                    et_warn(it, &et, Value::bytes(msg), None)?;
                    warn_count = pl::add(&warn_count, &n(1));
                    continue 'entry;
                }
                if pl::gt(&pl::add(&value_ptr, &size), &pl::add(&dir_start, &data_pos)) && pl::lt(&value_ptr, &pl::add(&pl::add(&dir_end, &data_pos), &n(4))) {
                    let msg = [&b"Value for "[..], &dir_s(&dir), b" ", &tag_name.to_bytes(), b" overlaps IFD"].concat();
                    et_warn(it, &et, Value::bytes(msg), None)?;
                }
                for vp in &val_pos {
                    if pl::ge(&vp.at(0), &pl::add(&value_ptr, &size)) || pl::le(&pl::add(&vp.at(0), &vp.at(1)), &value_ptr) {
                        continue;
                    }
                    let msg = [&b"Value for "[..], &dir_s(&dir), b" ", &tag_name.to_bytes(), b" overlaps ", &vp.at(2).to_bytes()].concat();
                    et_warn(it, &et, Value::bytes(msg), None)?;
                }
                val_pos.push(Value::new_array(vec![value_ptr.clone(), size.clone(), tag_name]));
            }
            // fix valuePtr if necessary
            if dir_info.get("FixOffsets").truthy() {
                let w_flag = Value::Undef;
                if !val_end.truthy() {
                    val_end = pl::add(&pl::add(&data_pos, &dir_end), &n(4));
                }
                //#### eval FixOffsets ($valuePtr, $valEnd, $size, $tagID, $wFlag)
                let (_, after) = eval_site(
                    it,
                    &dir_info.get("FixOffsets"),
                    &[("valuePtr", value_ptr.clone()), ("valEnd", val_end.clone()), ("size", size.clone()), ("tagID", tag_id.clone()), ("wFlag", w_flag), ("et", et.clone()), ("dirInfo", dir_info.clone()), ("tagTablePtr", tag_table_ptr.clone()), ("tagInfo", tag_info.clone()), ("dataPt", data_pt.clone()), ("dataPos", data_pos.clone()), ("dirStart", dir_start.clone()), ("entry", entry.clone()), ("index", index.clone())],
                    Want::Void,
                );
                value_ptr = after[0].clone();
                val_end = after[1].clone();
                size = after[2].clone();
            }
            let mut suspect = Value::Undef;
            // offset shouldn't point into TIFF header
            if pl::lt(&value_ptr, &n(8)) && !dir_info.get("ZeroOffsetOK").truthy() {
                suspect = warn_count.clone();
            }
            // convert offset to pointer in $$dataPt
            if dir_info.get("EntryBased").truthy() || (tag_entry.ref_type() == "HASH" && tag_entry.get("EntryBased").truthy()) {
                value_ptr = pl::add(&value_ptr, &entry);
            } else {
                value_ptr = pl::sub(&value_ptr, &data_pos);
            }
            // value shouldn't overlap our directory
            if pl::lt(&value_ptr, &dir_end) && pl::gt(&pl::add(&value_ptr, &size), &dir_start) {
                suspect = warn_count.clone();
            }
            // load value from file if necessary
            if pl::lt(&value_ptr, &n(0)) || pl::gt(&pl::add(&value_ptr, &size), &data_len) {
                // get value by seeking in file if we are allowed
                let mut buff = Value::Undef;
                if raf.truthy() {
                    // avoid loading large binary data unless necessary
                    #[allow(clippy::never_loop)]
                    while pl::gt(&size, &limit) {
                        if tag_info.truthy() {
                            // make large unknown blocks binary data
                            if tag_info.get("Unknown").truthy() {
                                tag_info.set("Binary", n(1));
                            }
                            if !tag_info.get("Binary").truthy() {
                                break; // must read non-binary data
                            }
                            if tag_info.get("SubDirectory").truthy() {
                                break;
                            }
                            let lc_tag = Value::bytes(tag_info.get("Name").to_bytes().to_ascii_lowercase());
                            if et.get("OPTIONS").get("Binary").truthy() && !et.get("EXCL_TAG_LOOKUP").get(lc_tag.to_bytes()).truthy() {
                                // read binary data if specified unless tagsFromFile won't use it
                                if !(et.get("TAGS_FROM_FILE").truthy() && tag_info.get("Protected").truthy()) {
                                    break;
                                }
                            }
                            // must read if tag is specified by name
                            if et.get("REQ_TAG_LOOKUP").get(lc_tag.to_bytes()).truthy() {
                                break;
                            }
                        } else {
                            // must read value if needed for a condition
                            if !tag_info.is_undef() {
                                break;
                            }
                        }
                        // (note: changing the value without changing $size will cause
                        // a warning in the verbose output, but we need to maintain the
                        // proper size for the htmlDump, so we can't change this)
                        buff = Value::bytes([&b"Binary data "[..], &size.to_bytes(), b" bytes"].concat());
                        read_size = pl::length(&buff);
                        break;
                    }
                    // read from file if necessary
                    if buff.is_undef() {
                        let (mut wrn, mut trunc_ok) = (Value::Undef, Value::Undef);
                        let read_from_raf = tag_info.truthy() && tag_info.get("ReadFromRAF").truthy();
                        let pos = pl::add(&pl::add(&base, &value_ptr), &data_pos);
                        if !pl::method(it, &raf, "Seek", vec![pos.clone(), n(0)])?.truthy() {
                            wrn = Value::bytes([&b"Invalid offset for "[..], &dir_s(&dir), b" entry ", &index.to_bytes()].concat());
                        } else if read_from_raf && pl::gt(&size, &limit) && !et.get("REQ_TAG_LOOKUP").get(tag_info.get("Name").to_bytes().to_ascii_lowercase()).truthy() {
                            buff = Value::bytes([&tag_info.get("Name").to_bytes()[..], b" data ", &size.to_bytes(), b" bytes"].concat());
                            read_size = pl::length(&buff);
                        } else {
                            let mut ra = [buff.clone(), size.clone()];
                            let r = method_aliased(it, &raf, "Read", &mut ra)?;
                            buff = ra[0].clone();
                            if !pl::num_eq(&r, &size) {
                                let f = [&b"Error reading value for "[..], &dir_s(&dir), b" entry ", &index.to_bytes(), b", ID 0x%.4x"].concat();
                                wrn = pl::sprintf_v(&Value::bytes(f), &[tag_id.clone()]);
                                if tag_info.truthy() && !tag_info.get("Unknown").truthy() {
                                    wrn = pl::concat(&wrn, &pl::concat(&pl::s(" "), &tag_info.get("Name")));
                                    trunc_ok = tag_info.get("TruncateOK");
                                }
                            } else if read_from_raf {
                                // seek back to the start of the value
                                pl::method(it, &raf, "Seek", vec![pos.clone(), n(0)])?;
                            }
                        }
                        if wrn.truthy() {
                            let minor = if in_maker_notes { n(1) } else { trunc_ok.clone() };
                            et_warn(it, &et, wrn, Some(minor))?;
                            if !(in_maker_notes || html_dump.truthy() || trunc_ok.truthy()) {
                                return ret(n(0));
                            }
                            warn_count = pl::add(&warn_count, &n(1));
                            if buff.is_undef() {
                                buff = pl::s("");
                            }
                            read_size = pl::length(&buff);
                            if !trunc_ok.truthy() {
                                bad = n(1);
                            }
                        }
                    }
                    value_data_len = pl::length(&buff);
                    value_data_pt = sref(buff);
                    value_data_pos = pl::add(&value_ptr, &data_pos);
                    value_ptr = n(0);
                } else {
                    let (mut tag_str, mut tmp_info, mut leica_trailer) = (Value::Undef, Value::Undef, Value::Undef);
                    if tag_info.truthy() {
                        tag_str = tag_info.get("Name");
                        leica_trailer = tag_info.get("LeicaTrailer");
                    } else if !tag_info.is_undef() {
                        tmp_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag_id.clone(), sref(pl::s("")), format_str.clone(), count.clone()])?;
                        if tmp_info.truthy() {
                            tag_str = tmp_info.get("Name");
                            leica_trailer = tmp_info.get("LeicaTrailer");
                        }
                    }
                    if tag_info.truthy() && tag_info.get("ChangeBase").truthy() {
                        // adjust base offset for this tag only
                        //#### eval ChangeBase ($dirStart,$dataPos)
                        let (new_base, _) = eval_site(it, &tag_info.get("ChangeBase"), &[("dirStart", dir_start.clone()), ("dataPos", data_pos.clone()), ("et", et.clone()), ("dirInfo", dir_info.clone()), ("tagInfo", tag_info.clone()), ("dataPt", data_pt.clone())], Want::Scalar);
                        value_ptr = pl::add(&value_ptr, &new_base);
                    }
                    if !tag_str.truthy() {
                        tag_str = pl::sprintf("tag 0x%.4x", &[tag_id.clone()]);
                    }
                    // allow PreviewImage to run outside EXIF data
                    let et_raf = et.get("RAF");
                    if tag_str.eq_str("PreviewImage") && et_raf.truthy() {
                        let pos = pl::method(it, &et_raf, "Tell", vec![])?;
                        buff = pl::method(it, &et, "ExtractBinary", vec![pl::add(&pl::add(&base, &value_ptr), &data_pos), size.clone(), pl::s("PreviewImage")])?;
                        pl::method(it, &et_raf, "Seek", vec![pos, n(0)])?;
                        value_data_pt = sref(buff.clone());
                        value_data_pos = pl::add(&value_ptr, &data_pos);
                        value_data_len = size.clone();
                        value_ptr = n(0);
                    } else if leica_trailer.truthy() && et_raf.truthy() {
                        if pl::gt(&verbose, &n(0)) {
                            let msg = [&et.get("INDENT").to_bytes()[..], &index.to_bytes(), b") ", &tag_str.to_bytes(), b" --> (outside APP1 segment)\n"].concat();
                            pl::method(it, &et, "VPrint", vec![n(0), Value::bytes(msg)])?;
                        }
                        if pl::method(it, &et, "Options", vec![pl::s("FastScan")])?.truthy() {
                            et_warn(it, &et, pl::s("Ignored Leica MakerNote trailer"), None)?;
                        } else {
                            crate::runtime::require(it, "Image::ExifTool::Fixup")?;
                            let lt = Value::new_hash();
                            lt.set("TagInfo", if tag_info.truthy() { tag_info.clone() } else { tmp_info.clone() });
                            lt.set("Offset", pl::add(&pl::add(&base, &value_ptr), &data_pos));
                            lt.set("Size", size.clone());
                            let cls = pl::s("Image::ExifTool::Fixup");
                            lt.set("Fixup", pl::method(it, &cls, "new", vec![])?);
                            et.set("LeicaTrailer", lt);
                        }
                    } else {
                        let msg = [&b"Bad offset for "[..], &dir_s(&dir), b" ", &tag_str.to_bytes()].concat();
                        et_warn(it, &et, Value::bytes(msg), Some(Value::from(in_maker_notes)))?;
                        warn_count = pl::add(&warn_count, &n(1));
                    }
                    if buff.is_undef() {
                        value_data_pt = pl::s("");
                        value_data_pos = pl::add(&value_ptr, &data_pos);
                        value_data_len = n(0);
                        value_ptr = n(0);
                        bad = n(1);
                    }
                }
            }
            // warn about suspect offsets if they didn't already cause another warning
            if !suspect.is_undef() && pl::num_eq(&suspect, &warn_count) {
                let tag_str = if tag_info.truthy() { tag_info.get("Name") } else { pl::sprintf("tag 0x%.4x", &[tag_id.clone()]) };
                let msg = [&b"Suspicious "[..], &dir_s(&dir), b" offset for ", &tag_str.to_bytes()].concat();
                if et_warn(it, &et, Value::bytes(msg), Some(Value::from(in_maker_notes)))?.truthy() {
                    warn_count = pl::add(&warn_count, &n(1));
                    if !verbose.truthy() {
                        continue 'entry;
                    }
                }
            }
        }
        // treat single unknown byte as int8u
        if pl::num_eq(&format, &n(7)) && pl::num_eq(&count, &n(1)) {
            format_str = pl::s("int8u");
        }

        let (mut val, mut subdir, mut wrong_format) = (Value::Undef, Value::Undef, Value::Undef);
        if pl::gt(&tag_id, &n(0xf000)) && is_exif {
            let old_info = tag_table_ptr.get(tag_id.to_bytes());
            if (!old_info.truthy() || (old_info.ref_type() == "HASH" && old_info.get("Condition").truthy() && !old_info.get("PSRaw").truthy())) && !bad.truthy() {
                // handle special case of Photoshop RAW tags (0xfde8-0xfe58)
                // --> generate tags from the value if possible
                val = pl::call(it, "Image::ExifTool::ReadValue", vec![value_data_pt.clone(), value_ptr.clone(), format_str.clone(), count.clone(), read_size.clone()])?;
                let m = if !val.is_undef() { re(r"(.*): (.*)", "").m(&val) } else { None };
                if let Some(m) = m {
                    let mut tag = cap(&m, 1);
                    val = cap(&m, 2);
                    re(r"'s", "").subst(&mut tag, false, |_| Vec::new()); // remove 's (so "Owner's Name" becomes "OwnerName")
                    tag = Value::bytes(crate::perl::tr::tr(&tag.to_bytes(), b"a-zA-Z0-9_", b"", "cd").0); // remove unknown characters
                    if tag.truthy() {
                        tag_info = Value::new_hash();
                        tag_info.set("Name", tag);
                        tag_info.set("Condition", pl::s("$$self{TIFF_TYPE} ne \"DCR\""));
                        tag_info.set("ValueConv", pl::s("$_=$val;s/^.*: //;$_")); // remove descr
                        tag_info.set("PSRaw", n(1)); // (just as flag to avoid adding this again)
                        pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_table_ptr.clone(), tag_id.clone(), tag_info.clone()])?;
                        // generate conditional list if a conditional tag already existed
                        if old_info.truthy() {
                            tag_table_ptr.set(tag_id.to_bytes(), Value::new_array(vec![old_info, tag_info.clone()]));
                        }
                    }
                }
            }
        }
        if !tag_info.is_undef() && !tag_info.truthy() {
            if bad.truthy() {
                tag_info = Value::Undef;
            } else {
                // GetTagInfo() required the value for a Condition
                let l = if pl::lt(&read_size, &n(128)) { read_size.clone() } else { n(128) };
                let tmp_val = substr(&value_data_pt.deref(), value_ptr.to_i64(), Some(l.to_i64()));
                // (use original format name in this call -- $formatStr may have been changed to int8u)
                tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag_id.clone(), sref(tmp_val), format_name.at(format.to_i64()), count.clone()])?;
            }
        }
        // make sure we are handling the 'ifd' format properly
        if (pl::num_eq(&format, &n(13)) || pl::num_eq(&format, &n(18))) && (!tag_info.truthy() || !tag_info.get("SubIFD").truthy()) {
            let s = pl::sprintf("%s tag 0x%.4x IFD format not handled", &[dir_name.clone(), tag_id.clone()]);
            et_warn(it, &et, s, Some(Value::from(in_maker_notes)))?;
        }
        if !tag_info.is_undef() {
            let mut read_format = tag_info.get("Format");
            subdir = tag_info.get("SubDirectory");
            // unless otherwise specified, all SubDirectory data except
            // EXIF SubIFD offsets should be unformatted
            if subdir.truthy() && !tag_info.get("SubIFD").truthy() && !read_format.truthy() {
                read_format = pl::s("undef");
            }
            // override EXIF format if specified
            if read_format.truthy() {
                format_str = read_format.clone();
                let new_num = format_number.get(format_str.to_bytes());
                if new_num.truthy() && !pl::num_eq(&new_num, &format) {
                    orig_form_str = Value::bytes([&format_name.at(format.to_i64()).to_bytes()[..], b"[", &count.to_bytes(), b"]"].concat());
                    format = new_num;
                    if tag_info.get("FixedSize").truthy() {
                        read_size = tag_info.get("FixedSize");
                        size = read_size.clone();
                    }
                    // adjust number of items for new format size
                    count = pl::int(&pl::div(&size, &format_size.at(format.to_i64()))?);
                }
            }
            // verify that offset-type values are integral
            if (tag_info.get("IsOffset").truthy() || tag_info.get("SubIFD").truthy()) && !int_format.get(format_str.to_bytes()).truthy() {
                let msg = pl::sprintf("Wrong format (%s) for %s 0x%.4x %s", &[format_str.clone(), dir.clone(), tag_id.clone(), tag_info.get("Name")]);
                et_warn(it, &et, msg, None)?;
                if validate.truthy() {
                    let k = [&dir_s(&dir)[..], b":", &tag_info.get("Name").to_bytes()].concat();
                    et.get_or_new_hash("WrongFormat").set(k, n(1));
                    offset_info.set(tag_id.to_bytes(), Value::new_array(vec![tag_info.clone(), pl::s("")]));
                }
                if !verbose.truthy() {
                    continue 'entry;
                }
                wrong_format = n(1);
            }
        } else if !verbose.truthy() {
            continue 'entry;
        }
        if !bad.truthy() {
            // limit maximum length of data to reformat
            // (avoids long delays when processing some corrupted files)
            let mut warned = Value::Undef;
            let not_bin = !re(r"^(undef|string|binary)$", "").is_match(&format_str);
            if pl::gt(&count, &n(100000)) && not_bin {
                let tag_name = if tag_info.truthy() { tag_info.get("Name") } else { pl::sprintf("tag 0x%.4x", &[tag_id.clone()]) };
                // (count of 196608 is typical for ColorMap)
                if !tag_name.eq_str("TransferFunction") || !pl::num_eq(&count, &n(196608)) {
                    let minor = if pl::gt(&count, &n(2000000)) { n(0) } else { n(2) };
                    let msg = [&b"Ignoring "[..], &dir_name.to_bytes(), b" ", &tag_name.to_bytes(), b" with excessive count"].concat();
                    if et_warn(it, &et, Value::bytes(msg), Some(minor))?.truthy() {
                        if !et.get("OPTIONS").get("HtmlDump").truthy() {
                            continue 'entry;
                        }
                        warned = n(1);
                    }
                }
            }
            if pl::gt(&count, &n(500)) && not_bin && (!tag_info.truthy() || tag_info.get("LongBinary").truthy() || warned.truthy()) && !et.get("OPTIONS").get("IgnoreMinorErrors").truthy() {
                if !warned.truthy() {
                    et_warn(it, &et, pl::s("Not decoding some large array(s). Ignore minor errors to decode"), Some(n(2)))?;
                }
                if et.get("TAGS_FROM_FILE").truthy() {
                    continue 'entry; // don't generate bogus value when copying tags
                }
                val = Value::bytes([&b"(large array of "[..], &count.to_bytes(), b" ", &format_str.to_bytes(), b" values)"].concat());
            } else {
                // convert according to specified format
                let rat = sref(Value::Undef);
                val = pl::call(it, "Image::ExifTool::ReadValue", vec![value_data_pt.clone(), value_ptr.clone(), format_str.clone(), count.clone(), read_size.clone(), rat.clone()])?;
                rational = rat.deref();
                if save_bin.truthy() {
                    bin_val = substr(&value_data_pt.deref(), value_ptr.to_i64(), Some(read_size.to_i64()));
                }
                // re-code if necessary
                if !val.is_undef() {
                    if format_str.eq_str("utf8") {
                        val = pl::method(it, &et, "Decode", vec![val.clone(), pl::s("UTF8")])?;
                    } else if str_enc.truthy() && format_str.eq_str("string") {
                        val = pl::method(it, &et, "Decode", vec![val.clone(), str_enc.clone()])?;
                    }
                }
            }
        }

        if verbose.truthy() {
            let mut tval = val.clone();
            // also show as a rational
            if !rational.is_undef() {
                tval = Value::bytes([&tval.to_bytes()[..], b" (", &rational.to_bytes(), b")"].concat());
            }
            if html_dump.truthy() {
                let tag_name;
                let mut col_name;
                if tag_info.truthy() {
                    tag_name = tag_info.get("Name");
                } else if pl::num_eq(&tag_id, &n(0x927c)) && dir_name.eq_str("ExifIFD") {
                    tag_name = pl::s("MakerNotes");
                } else {
                    tag_name = pl::sprintf("Tag 0x%.4x", &[tag_id.clone()]);
                }
                let dname = pl::sprintf_v(&Value::bytes([&dir_s(&dir)[..], b"-%.2d"].concat()), &[index.clone()]);
                // build our tool tip
                if pl::lt(&size, &n(0)) {
                    size = pl::mul(&count, &format_size.at(format.to_i64()));
                }
                let mut fstr = Value::bytes([&format_name.at(format.to_i64()).to_bytes()[..], b"[", &count.to_bytes(), b"]"].concat());
                if orig_form_str.truthy() && orig_form_str.to_bytes() != fstr.to_bytes() {
                    fstr = Value::bytes([&orig_form_str.to_bytes()[..], b" read as ", &fstr.to_bytes()].concat());
                }
                if wrong_format.truthy() {
                    fstr = pl::concat(&fstr, &pl::s(" <-- WRONG"));
                }
                let mut tip = pl::concat(&pl::sprintf("Tag ID: 0x%.4x\n", &[tag_id.clone()]), &Value::bytes([&b"Format: "[..], &fstr.to_bytes(), b"\nSize: ", &size.to_bytes(), b" bytes\n"].concat()));
                if pl::gt(&size, &n(4)) {
                    let off_pt = get32u(it, &data_pt, &pl::add(&entry, &n(8)))?;
                    // (test this with ../pics/{CanonEOS-1D_XMarkIII.hif,PanasonicDC-G9.rw2,*.raf})
                    let exif_pos = or0(et.get("EXIF_POS"));
                    let fudge = {
                        let f = et.get("BASE_FUDGE");
                        if f.truthy() {
                            f
                        } else {
                            or0(et.get("BASE"))
                        }
                    };
                    let act_pt = pl::add(&pl::sub(&pl::add(&pl::add(&value_ptr, &value_data_pos), &base), &exif_pos), &fudge);
                    tip = pl::concat(&tip, &pl::sprintf("Value offset: 0x%.4x\n", &[off_pt.clone()]));
                    // highlight tag name (red for bad size)
                    let mut style = if bad.truthy() || tval.is_undef() { "V" } else { "H" };
                    if !pl::num_eq(&act_pt, &off_pt) {
                        tip = pl::concat(&tip, &pl::sprintf("Actual offset: 0x%.4x\n", &[act_pt.clone()]));
                        let sign = if pl::lt(&act_pt, &off_pt) { "-" } else { "" };
                        tip = pl::concat(&tip, &pl::sprintf(&format!("Offset base: {sign}0x%.4x\n"), &[pl::abs(&pl::sub(&act_pt, &off_pt))]));
                        if style == "H" {
                            style = "F"; // purple for different offsets
                        }
                    }
                    if et.get("EXIF_POS").truthy() && !et.get("BASE_FUDGE").truthy() {
                        tip = pl::concat(&tip, &pl::sprintf("File offset:   0x%.4x\n", &[pl::add(&act_pt, &et.get("EXIF_POS"))]));
                    }
                    col_name = Value::bytes([format!("<span class={style}>").as_bytes(), &tag_name.to_bytes()[..], b"</span>"].concat());
                    if ops::bit_and(&off_pt, &n(1)).truthy() {
                        col_name = pl::concat(&col_name, &pl::s(" <span class=V>(odd)</span>"));
                    }
                } else {
                    col_name = tag_name.clone();
                }
                if wrong_format.truthy() {
                    col_name = pl::concat(&col_name, &pl::s(" <span class=V>(err)</span>"));
                }
                if pl::le(&tag_id, &last_id) && !in_maker_notes {
                    col_name = pl::concat(&col_name, &pl::s(" <span class=V>(seq)</span>"));
                }
                last_id = tag_id.clone();
                if tval.is_undef() {
                    tval = pl::s("<bad size/offset>");
                } else {
                    if pl::gt(&pl::length(&tval), &n(32)) {
                        tval = pl::concat(&substr(&tval, 0, Some(28)), &pl::s("[...]"));
                    }
                    if re(r"^(string|undef|binary)", "").is_match(&format_str) {
                        // translate non-printable characters
                        tval = tr_unprintable(&tval);
                    } else if tag_info.truthy() && is_int(it, &tval)? {
                        if tag_info.get("IsOffset").truthy() || tag_info.get("SubIFD").truthy() {
                            tval = pl::sprintf("0x%.4x", &[tval.clone()]);
                            let exif_pos = or0(et.get("EXIF_POS"));
                            let fudge = {
                                let f = et.get("BASE_FUDGE");
                                if f.truthy() {
                                    f
                                } else {
                                    or0(et.get("BASE"))
                                }
                            };
                            let act_pt = pl::add(&pl::sub(&pl::add(&val, &base), &exif_pos), &fudge);
                            if !pl::num_eq(&act_pt, &val) {
                                tval = pl::concat(&tval, &pl::sprintf("\nActual offset: 0x%.4x", &[act_pt.clone()]));
                                let sign = if pl::lt(&act_pt, &val) { "-" } else { "" };
                                tval = pl::concat(&tval, &pl::sprintf(&format!("\nOffset base: {sign}0x%.4x"), &[pl::abs(&pl::sub(&act_pt, &val))]));
                            }
                            if et.get("EXIF_POS").truthy() && !et.get("BASE_FUDGE").truthy() {
                                tip = pl::concat(&tip, &pl::sprintf("File offset:   0x%.4x\n", &[pl::add(&act_pt, &et.get("EXIF_POS"))]));
                            }
                        } else if tag_info.get("PrintHex").truthy() {
                            tval = pl::sprintf("0x%x", &[tval.clone()]);
                        }
                    }
                }
                tip = pl::concat(&tip, &pl::concat(&pl::s("Value: "), &tval));
                let mut id = off_name.clone();
                let mut sid = Value::Undef;
                if tag_info.truthy() && tag_info.get("SubIFD").truthy() {
                    let (o, i, s) = next_offset_name_impl(&et, &off_name);
                    sub_off_name = o;
                    id = i;
                    sid = s;
                }
                let pos = pl::add(&pl::add(&entry, &data_pos), &base);
                pl::method(it, &et, "HDump", vec![pos, n(12), Value::bytes([&dname.to_bytes()[..], b" ", &col_name.to_bytes()].concat()), tip, n(1), id])?;
                if pl::lt(&value_data_len, &n(0)) {
                    continue 'entry; // don't process bad pointer entry
                }
                if pl::gt(&size, &n(4)) {
                    let exif_dump_pos = pl::add(&pl::add(&value_ptr, &value_data_pos), &base);
                    let mut flag = n(0);
                    if subdir.truthy() {
                        if tag_info.get("MakerNotes").truthy() {
                            flag = n(0x04);
                        } else if tag_info.get("NestedHtmlDump").truthy() {
                            flag = if pl::num_eq(&tag_info.get("NestedHtmlDump"), &n(2)) { n(0x10) } else { n(0x04) };
                        }
                    }
                    // add value data block (underlining maker notes data)
                    let label = pl::concat(&tag_name, &pl::s(" value"));
                    pl::method(it, &et, "HDump", vec![exif_dump_pos.clone(), size.clone(), label.clone(), pl::s("SAME"), flag, sid])?;
                    if subdir.truthy() && tag_info.get("MakerNotes").truthy() && tag_info.get("NotIFD").truthy() {
                        pl::method(it, &et, "HDump", vec![exif_dump_pos, size.clone(), label, Value::Undef, Value::Undef, dir_info.get("OffsetName")])?;
                    }
                }
            } else {
                if pl::le(&tag_id, &last_id) && !in_maker_notes {
                    let str = if tag_info.truthy() { pl::concat(&pl::s(" "), &tag_info.get("Name")) } else { pl::s("") };
                    let msg = if pl::num_eq(&tag_id, &last_id) {
                        pl::sprintf("Duplicate tag 0x%.4x%s in %s", &[tag_id.clone(), str, dir_name.clone()])
                    } else {
                        pl::sprintf("Tag ID 0x%.4x%s out of sequence in %s", &[tag_id.clone(), str, dir_name.clone()])
                    };
                    et_warn(it, &et, msg, None)?;
                }
                last_id = tag_id.clone();
                if pl::gt(&verbose, &n(0)) {
                    let mut fstr = format_name.at(format.to_i64());
                    if orig_form_str.truthy() {
                        fstr = Value::bytes([&orig_form_str.to_bytes()[..], b" read as ", &fstr.to_bytes()].concat());
                    }
                    let args = vec![
                        tag_id.clone(),
                        tag_info.clone(),
                        pl::s("Table"),
                        tag_table_ptr.clone(),
                        pl::s("Index"),
                        index.clone(),
                        pl::s("Value"),
                        tval.clone(),
                        pl::s("DataPt"),
                        value_data_pt.clone(),
                        pl::s("DataPos"),
                        pl::add(&value_data_pos, &base),
                        pl::s("Size"),
                        size.clone(),
                        pl::s("Start"),
                        value_ptr.clone(),
                        pl::s("Format"),
                        fstr,
                        pl::s("Count"),
                        count.clone(),
                    ];
                    pl::method(it, &et, "VerboseInfo", args)?;
                }
            }
            if !tag_info.truthy() || wrong_format.truthy() {
                continue 'entry;
            }
        }
        if val.is_undef() {
            continue 'entry;
        }
//..............................................................................
// Handle SubDirectory tag types
//
        if subdir.truthy() {
            // don't process empty subdirectories
            if !size.truthy() {
                if !(tag_info.get("MakerNotes").truthy() || in_maker_notes) {
                    let msg = [&b"Empty "[..], &tag_info.get("Name").to_bytes(), b" data"].concat();
                    et_warn(it, &et, Value::bytes(msg), Some(n(1)))?;
                }
                continue 'entry;
            }
            let (mut values, mut new_byte_order, mut invalid): (std::collections::VecDeque<Value>, Value, Value) = (Default::default(), Value::Undef, Value::Undef);
            let tag_str = tag_info.get("Name");
            if subdir.get("MaxSubdirs").truthy() {
                values = pl::split_ws(&val).into();
                // limit the number of subdirectories we parse
                let over = pl::sub(&n(values.len() as i64), &subdir.get("MaxSubdirs"));
                if pl::gt(&over, &n(0)) {
                    let msg = [&b"Ignoring "[..], &over.to_bytes(), b" ", &tag_str.to_bytes(), b" directories"].concat();
                    et_warn(it, &et, Value::bytes(msg), None)?;
                    values.truncate(subdir.get("MaxSubdirs").to_i64().max(0) as usize);
                }
                val = values.pop_front().unwrap_or_default();
            }
            let new_tag_table = if subdir.get("TagTable").truthy() {
                let t = pl::call(it, "Image::ExifTool::GetTagTable", vec![subdir.get("TagTable")])?;
                if !t.truthy() {
                    let msg = [&b"Unknown tag table "[..], &subdir.get("TagTable").to_bytes()].concat();
                    pl::warn(it, &String::from_utf8_lossy(&msg))?;
                    continue 'entry;
                }
                t
            } else {
                tag_table_ptr.clone() // use existing table
            };
            // loop through all sub-directories specified by this tag
            let mut dir_num: i64 = 0;
            loop {
                let mut subdir_base = base.clone();
                let mut subdir_data_pt = value_data_pt.clone();
                let mut subdir_data_pos = value_data_pos.clone();
                let mut subdir_data_len = value_data_len.clone();
                let mut subdir_start = value_ptr.clone();
                if !subdir.get("Start").is_undef() {
                    // set local $valuePtr relative to file $base for eval
                    let value_ptr = pl::add(&subdir_start, &subdir_data_pos);
                    //#### eval Start ($valuePtr, $val)
                    let (mut new_start, _) = eval_site(
                        it,
                        &subdir.get("Start"),
                        &[("valuePtr", value_ptr), ("val", val.clone()), ("et", et.clone()), ("dirInfo", dir_info.clone()), ("tagInfo", tag_info.clone()), ("subdir", subdir.clone()), ("dataPt", data_pt.clone()), ("dataPos", data_pos.clone()), ("dirStart", dir_start.clone()), ("size", size.clone())],
                        Want::Scalar,
                    );
                    if !is_int(it, &new_start)? {
                        et_warn(it, &et, pl::concat(&pl::s("Bad value for "), &tag_str), None)?;
                        break;
                    }
                    // convert back to relative to $subdirDataPt
                    new_start = pl::sub(&new_start, &subdir_data_pos);
                    // adjust directory size if necessary
                    if !(tag_info.get("SubIFD").truthy() || subdir.get("BadOffset").truthy()) {
                        size = pl::sub(&size, &pl::sub(&new_start, &subdir_start));
                    }
                    subdir_start = new_start;
                }
                // this is a pain, but some maker notes are always a specific
                // byte order, regardless of the byte order of the file
                let old_byte_order = get_byte_order(it)?;
                new_byte_order = subdir.get("ByteOrder");
                if new_byte_order.truthy() {
                    if re(r"^Little", "i").is_match(&new_byte_order) {
                        new_byte_order = pl::s("II");
                    } else if re(r"^Big", "i").is_match(&new_byte_order) {
                        new_byte_order = pl::s("MM");
                    } else if subdir.get("OffsetPt").truthy() {
                        new_byte_order = Value::Undef;
                        pl::warn(it, "Can't have variable byte ordering for SubDirectories using OffsetPt")?;
                        break;
                    } else if pl::le(&pl::add(&subdir_start, &n(2)), &subdir_data_len) {
                        // attempt to determine the byte ordering by checking
                        // the number of directory entries.  This is an int16u
                        // that should be a reasonable value.
                        let num = get16u(it, &subdir_data_pt, &subdir_start)?;
                        if ops::bit_and(&num, &n(0xff00)).truthy() && pl::gt(&ops::shift_right(&num, &n(8)), &ops::bit_and(&num, &n(0xff))) {
                            // This looks wrong, we shouldn't have this many entries
                            new_byte_order = match &*old_byte_order.to_bytes() {
                                b"II" => pl::s("MM"),
                                b"MM" => pl::s("II"),
                                _ => Value::Undef,
                            };
                        } else {
                            new_byte_order = old_byte_order.clone();
                        }
                    }
                } else {
                    new_byte_order = old_byte_order.clone();
                }
                // set base offset if necessary
                if subdir.get("Base").truthy() {
                    // calculate subdirectory start relative to $base for eval
                    let start = pl::add(&subdir_start, &subdir_data_pos);
                    //#### eval Base ($start,$base)
                    let (b, _) = eval_site(it, &subdir.get("Base"), &[("start", start), ("base", base.clone()), ("et", et.clone()), ("dirInfo", dir_info.clone()), ("tagInfo", tag_info.clone()), ("subdir", subdir.clone()), ("dataPt", data_pt.clone()), ("val", val.clone())], Want::Scalar);
                    subdir_base = pl::add(&b, &base);
                }
                // add offset to the start of the directory if necessary
                if subdir.get("OffsetPt").truthy() {
                    //#### eval OffsetPt ($valuePtr)
                    let (pos, _) = eval_site(it, &subdir.get("OffsetPt"), &[("valuePtr", value_ptr.clone()), ("et", et.clone()), ("dirInfo", dir_info.clone()), ("tagInfo", tag_info.clone()), ("subdir", subdir.clone()), ("dataPt", data_pt.clone()), ("val", val.clone())], Want::Scalar);
                    if pl::gt(&pl::add(&pos, &n(4)), &subdir_data_len) {
                        et_warn(it, &et, Value::bytes([&b"Bad "[..], &tag_str.to_bytes(), b" OffsetPt"].concat()), None)?;
                        break;
                    }
                    set_byte_order(it, &new_byte_order)?;
                    subdir_start = pl::add(&subdir_start, &get32u(it, &subdir_data_pt, &pos)?);
                    set_byte_order(it, &old_byte_order)?;
                }
                if pl::lt(&subdir_start, &n(0)) || pl::gt(&pl::add(&subdir_start, &n(2)), &subdir_data_len) {
                    // convert $subdirStart back to a file offset
                    if raf.truthy() {
                        // reset SubDirectory buffer (we will load it later)
                        let buff = pl::s("");
                        subdir_data_pt = sref(buff.clone());
                        size = pl::length(&buff);
                        subdir_data_len = size.clone();
                    } else {
                        let mut msg = Value::bytes([&b"Bad "[..], &tag_str.to_bytes(), b" SubDirectory start"].concat());
                        if pl::gt(&verbose, &n(0)) {
                            if pl::lt(&subdir_start, &n(0)) {
                                msg = Value::bytes([&msg.to_bytes()[..], b" (directory start ", &subdir_start.to_bytes(), b" is before EXIF start)"].concat());
                            } else {
                                let end = pl::add(&subdir_start, &size);
                                msg = Value::bytes([&msg.to_bytes()[..], b" (directory end is ", &end.to_bytes(), b" but EXIF size is only ", &subdir_data_len.to_bytes(), b")"].concat());
                            }
                        }
                        et_warn(it, &et, msg, Some(Value::from(in_maker_notes)))?;
                        break;
                    }
                }

                // must update subdirDataPos if $base changes for this subdirectory
                subdir_data_pos = pl::add(&subdir_data_pos, &pl::sub(&base, &subdir_base));

                // build information hash for new directory
                let subdir_info = Value::new_hash();
                for (k, v) in [
                    ("Name", tag_str.clone()),
                    ("Base", subdir_base.clone()),
                    ("DataPt", subdir_data_pt.clone()),
                    ("DataPos", subdir_data_pos.clone()),
                    ("DataLen", subdir_data_len.clone()),
                    ("DirStart", subdir_start.clone()),
                    ("DirLen", size.clone()),
                    ("RAF", raf.clone()),
                    ("Parent", dir_name.clone()),
                    ("DirName", subdir.get("DirName")),
                    ("FixBase", subdir.get("FixBase")),
                    ("FixOffsets", subdir.get("FixOffsets")),
                    ("EntryBased", subdir.get("EntryBased")),
                    ("TagInfo", tag_info.clone()),
                    ("SubIFD", tag_info.get("SubIFD")),
                    ("Subdir", subdir.clone()),
                    ("OffsetName", sub_off_name.clone()),
                ] {
                    subdir_info.set(k, v);
                }
                // (remember: some cameras incorrectly write maker notes in IFD0)
                if tag_info.get("MakerNotes").truthy() {
                    // don't parse makernotes if FastScan > 1
                    let fast = pl::method(it, &et, "Options", vec![pl::s("FastScan")])?;
                    if fast.truthy() && pl::gt(&fast, &n(1)) {
                        break;
                    }
                    subdir_info.set("MakerNoteAddr", pl::add(&pl::add(&value_ptr, &value_data_pos), &base));
                    if !subdir.get("Base").is_undef() {
                        subdir_info.set("NoFixBase", n(1));
                    }
                }
                // set directory IFD name from group name of family 1 if it exists,
                // unless the tag is writable as a block in which case group 1 may
                // have been set automatically
                if tag_info.get("Groups").truthy() && !tag_info.get("Writable").truthy() {
                    subdir_info.set("DirName", tag_info.path(&["Groups", "1"]));
                    // number multiple subdirectories
                    if dir_num != 0 {
                        let mut d = subdir_info.get("DirName");
                        let num = dir_num.to_string();
                        re(r"\d*$", "").subst(&mut d, false, |_| num.as_bytes().to_vec());
                        subdir_info.set("DirName", d);
                    }
                }
                set_byte_order(it, &new_byte_order)?; // set byte order for this subdir
                // validate the subdirectory if necessary
                let dir_data = subdir_data_pt.clone(); // set data pointer to be used in eval
                //#### eval Validate ($val, $dirData, $subdirStart, $size)
                let mut ok = n(0);
                let invalid_now = if !subdir.get("Validate").is_undef() {
                    let (r, _) = eval_site(it, &subdir.get("Validate"), &[("val", val.clone()), ("dirData", dir_data), ("subdirStart", subdir_start.clone()), ("size", size.clone()), ("et", et.clone()), ("dirInfo", dir_info.clone()), ("tagInfo", tag_info.clone()), ("subdir", subdir.clone())], Want::Scalar);
                    !r.truthy()
                } else {
                    false
                };
                if invalid_now {
                    et_warn(it, &et, Value::bytes([&b"Invalid "[..], &tag_str.to_bytes(), b" data"].concat()), Some(Value::from(in_maker_notes)))?;
                    invalid = n(1);
                } else {
                    if !subdir_info.get("DirName").truthy() && in_maker_notes {
                        subdir_info.set("DirName", tag_info.get("Name"));
                    }
                    // process the subdirectory
                    ok = pl::method(it, &et, "ProcessDirectory", vec![subdir_info.clone(), new_tag_table.clone(), subdir.get("ProcessProc")])?;
                }
                // print debugging information if there were errors
                if !ok.truthy() && pl::gt(&verbose, &n(1)) && !pl::num_eq(&subdir_start, &value_ptr) {
                    let out = pl::method(it, &et, "Options", vec![pl::s("TextOut")])?;
                    let scope = Scope::new(None);
                    scope.scalar("out", out);
                    scope.scalar("indent", et.get("INDENT"));
                    scope.scalar("subdirStart", subdir_start.clone());
                    let code = br#"printf $out "%s    (SubDirectory start = 0x%x)\n", $indent, $subdirStart;"#;
                    if let Err(e) = it.eval_string(code, "Image::ExifTool::Exif", &scope, Want::Void) {
                        return Err(Flow::Die(e));
                    }
                }
                set_byte_order(it, &old_byte_order)?; // restore original byte swapping

                if values.is_empty() {
                    break;
                }
                val = values.pop_front().unwrap_or_default(); // continue with next subdir
                dir_num += 1;
            }
            let do_maker = pl::method(it, &et, "Options", vec![pl::s("MakerNotes")])?;
            let lc_tag = tag_str.to_bytes().to_ascii_lowercase();
            if !(do_maker.truthy() || et.get("REQ_TAG_LOOKUP").get(&lc_tag).truthy() || tag_info.get("BlockExtract").truthy()) {
                continue 'entry;
            }
            // extract as a block if specified
            if tag_info.get("MakerNotes").truthy() {
                // save maker note byte order (if it was significant and valid)
                if subdir.get("ByteOrder").truthy() && !invalid.truthy() {
                    let ubo = et.get("UnknownByteOrder");
                    et.set("MAKER_NOTE_BYTE_ORDER", if !ubo.is_undef() { ubo } else { new_byte_order.clone() });
                }
                if do_maker.truthy() && do_maker.eq_str("2") {
                    // extract maker notes without rebuilding (no fixup information)
                    et.delete("MAKER_NOTE_FIXUP");
                } else if !tag_info.get("NotIFD").truthy() || tag_info.get("IsPhaseOne").truthy() {
                    // this is a pain, but we must rebuild EXIF-type maker notes to
                    // include all the value data if data was outside the maker notes
                    let maker_dir_info = Value::new_hash();
                    for (k, v) in [
                        ("Name", tag_str.clone()),
                        ("Base", base.clone()),
                        ("DataPt", value_data_pt.clone()),
                        ("DataPos", value_data_pos.clone()),
                        ("DataLen", value_data_len.clone()),
                        ("DirStart", value_ptr.clone()),
                        ("DirLen", size.clone()),
                        ("RAF", raf.clone()),
                        ("Parent", dir_name.clone()),
                        ("DirName", pl::s("MakerNotes")),
                        ("FixOffsets", subdir.get("FixOffsets")),
                        ("TagInfo", tag_info.clone()),
                    ] {
                        maker_dir_info.set(k, v);
                    }
                    let val2;
                    if tag_info.get("IsPhaseOne").truthy() {
                        et.set("DropTags", n(1));
                        val2 = pl::call(it, "Image::ExifTool::PhaseOne::WritePhaseOne", vec![et.clone(), maker_dir_info.clone(), new_tag_table.clone()])?;
                        et.delete("DropTags");
                    } else {
                        if subdir.get("FixBase").truthy() {
                            maker_dir_info.set("FixBase", n(1));
                        }
                        // rebuild maker notes (creates $$et{MAKER_NOTE_FIXUP})
                        val2 = pl::call(it, "Image::ExifTool::Exif::RebuildMakerNotes", vec![et.clone(), maker_dir_info.clone(), new_tag_table.clone()])?;
                    }
                    if !val2.is_undef() {
                        val = val2;
                    } else if pl::gt(&size, &n(4)) {
                        et_warn(it, &et, pl::s("Error rebuilding maker notes (may be corrupt)"), None)?;
                    }
                }
            } else {
                // extract this directory as a block if specified
                if !tag_info.get("Writable").truthy() {
                    continue 'entry;
                }
            }
        }
//..............................................................................
        // convert to absolute offsets if this tag is an offset
        //#### eval IsOffset ($val, $et)
        let is_offset = tag_info.get("IsOffset");
        if is_offset.truthy() && eval_site(it, &is_offset, &[("val", val.clone()), ("et", et.clone()), ("tagInfo", tag_info.clone()), ("dirInfo", dir_info.clone())], Want::Scalar).0.truthy() {
            let mut offset_base = if is_offset.eq_str("2") { first_base.clone() } else { base.clone() };
            offset_base = pl::add(&offset_base, &et.get("BASE"));
            // handle offsets which use a wrong base (Minolta A200)
            if tag_info.get("WrongBase").truthy() {
                //#### eval WrongBase ($self)
                let (wb, _) = eval_site(it, &tag_info.get("WrongBase"), &[("self", et.clone()), ("et", et.clone()), ("tagInfo", tag_info.clone()), ("val", val.clone())], Want::Scalar);
                offset_base = pl::add(&offset_base, &if wb.truthy() { wb } else { n(0) });
            }
            let mut vals = pl::split_ws(&val);
            for v in vals.iter_mut() {
                *v = pl::add(v, &offset_base);
            }
            val = join(b" ", &vals);
        }
        if validate.truthy() || do_hash.truthy() {
            if tag_info.get("OffsetPair").truthy() {
                offset_info.set(tag_id.to_bytes(), Value::new_array(vec![tag_info.clone(), val.clone()]));
            } else if save_for_validate.get(tag_id.to_bytes()).truthy() && is_exif {
                offset_info.set(tag_id.to_bytes(), val.clone());
            }
        }
        // save the value of this tag
        tag_key = pl::method(it, &et, "FoundTag", vec![tag_info.clone(), val.clone()])?;
        if !tag_key.is_undef() {
            // set the group 1 name for tags in specified tables
            if tag_table_ptr.get("SET_GROUP1").truthy() {
                pl::method(it, &et, "SetGroup", vec![tag_key.clone(), dir_name.clone()])?;
            }
            // save original components of rational numbers (used when copying)
            if !rational.is_undef() {
                et.get_or_new_hash("TAG_EXTRA").get_or_new_hash(tag_key.to_bytes()).set("Rational", rational.clone());
            }
            if !bin_val.is_undef() {
                et.get_or_new_hash("TAG_EXTRA").get_or_new_hash(tag_key.to_bytes()).set("BinVal", bin_val.clone());
            }
            if save_format.truthy() {
                et.get_or_new_hash("TAG_EXTRA").get_or_new_hash(tag_key.to_bytes()).set("G6", save_format.clone());
            }
            if et.get("MAKER_NOTE_FIXUP").truthy() {
                et.get_or_new_hash("TAG_EXTRA").get_or_new_hash(tag_key.to_bytes()).set("Fixup", et.get("MAKER_NOTE_FIXUP"));
                et.delete("MAKER_NOTE_FIXUP");
            }
        }
    }

    if !offset_info.is_empty() {
        // calculate image data hash if requested
        if do_hash.truthy() {
            pl::call(it, "Image::ExifTool::Exif::AddImageDataHash", vec![et.clone(), dir_info.clone(), offset_info.clone()])?;
        }
        // validate image data offsets for this IFD (note: modifies %offsetInfo)
        if validate.truthy() {
            pl::call(it, "Image::ExifTool::Validate::ValidateOffsetInfo", vec![et.clone(), offset_info.clone(), dir_name.clone(), Value::from(in_maker_notes)])?;
        }
    }

    // scan for subsequent IFD's if specified
    if dir_info.get("Multi").truthy() && pl::ge(&bytes_from_end, &n(4)) {
        // use same directory information for trailing directory,
        // but change the start location (ProcessDirectory will
        // test to make sure we don't reprocess the same dir twice)
        let new_dir_info = dir_info.hash_copy();
        new_dir_info.set("Multi", n(0)); // prevent recursion
        new_dir_info.set("OffsetName", next_off_name.clone());
        indent_trim(&et); // keep indent the same
        loop {
            let offset = get32u(it, &data_pt, &dir_end)?;
            if !offset.truthy() {
                break;
            }
            new_dir_info.set("DirStart", pl::sub(&offset, &data_pos));
            // increment IFD number
            let mut dn = new_dir_info.get("DirName");
            let mut one = Value::Undef;
            let ifd_num = if re(r"(\d+)$", "").subst(&mut dn, false, |c| {
                one = cap(c, 1);
                Vec::new()
            }) > 0
            {
                new_dir_info.set("DirName", dn.clone());
                one
            } else {
                n(0)
            };
            new_dir_info.set("DirName", pl::concat(&new_dir_info.get("DirName"), &pl::add(&ifd_num, &n(1))));
            // must validate SubIFD1 because the nextIFD pointer is invalid for some RAW formats
            if !new_dir_info.get("DirName").eq_str("SubIFD1") || pl::call(it, "Image::ExifTool::Exif::ValidateIFD", vec![new_dir_info.clone()])?.truthy() {
                let path = et.get("PATH");
                let cur = match &path {
                    Value::ArrayRef(r) => r.borrow_mut().pop().unwrap_or_default(),
                    _ => Value::Undef,
                };
                if !pl::method(it, &et, "ProcessDirectory", vec![new_dir_info.clone(), tag_table_ptr.clone()])?.truthy() {
                    success = n(0);
                }
                et.get_or_new_array("PATH").push(cur);
                if success.truthy() && pl::ge(&new_dir_info.get("BytesFromEnd"), &n(4)) {
                    data_pt = new_dir_info.get("DataPt");
                    data_pos = new_dir_info.get("DataPos");
                    dir_end = new_dir_info.get("DirEnd");
                    continue;
                }
            } else if verbose.truthy() || et.get("TIFF_TYPE").eq_str("TIFF") {
                et_warn(it, &et, pl::s("Ignored bad IFD linked from SubIFD"), None)?;
            }
            break;
        }
    } else if !dir_info.get("Multi").is_undef() {
        // return necessary parameters for parsing next IFD
        dir_info.set("DirEnd", dir_end.clone());
        dir_info.set("OffsetName", next_off_name.clone());
        dir_info.set("BytesFromEnd", bytes_from_end.clone());
    }
    ret(success)
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Exif::BINARY_DATA_LIMIT", binary_data_limit);
    it.define_native("Image::ExifTool::Exif::AUTOLOAD", autoload);
    it.define_native("Image::ExifTool::Exif::IdentifyRawFile", identify_raw_file);
    it.define_native("Image::ExifTool::Exif::CalculateLV", calculate_lv);
    it.define_native("Image::ExifTool::Exif::CalcScaleFactor35efl", calc_scale_factor35efl);
    it.define_native("Image::ExifTool::Exif::PrintFraction", print_fraction);
    it.define_native("Image::ExifTool::Exif::ConvertFraction", convert_fraction);
    it.define_native("Image::ExifTool::Exif::ConvertExifText", convert_exif_text);
    it.define_native("Image::ExifTool::Exif::PrintSFR", print_sfr);
    it.define_native("Image::ExifTool::Exif::PrintParameter", print_parameter);
    it.define_native("Image::ExifTool::Exif::ConvertParameter", convert_parameter);
    it.define_native("Image::ExifTool::Exif::RedBlueBalance", red_blue_balance);
    it.define_native("Image::ExifTool::Exif::PrintExposureTime", print_exposure_time);
    it.define_native("Image::ExifTool::Exif::PrintFNumber", print_fnumber);
    it.define_native("Image::ExifTool::Exif::DecodeCFAPattern", decode_cfa_pattern);
    it.define_native("Image::ExifTool::Exif::PrintCFAPattern", print_cfa_pattern);
    it.define_native("Image::ExifTool::Exif::PrintOpcode", print_opcode);
    it.define_native("Image::ExifTool::Exif::PrintLensInfo", print_lens_info);
    it.define_native("Image::ExifTool::Exif::GetLensInfo", get_lens_info);
    it.define_native("Image::ExifTool::Exif::MatchLensModel", match_lens_model);
    it.define_native("Image::ExifTool::Exif::PrintLensID", print_lens_id);
    it.define_native("Image::ExifTool::Exif::ExifDate", exif_date);
    it.define_native("Image::ExifTool::Exif::ExifTime", exif_time);
    it.define_native("Image::ExifTool::Exif::GenerateTIFF", generate_tiff);
    it.define_native("Image::ExifTool::Exif::RebuildTIFF", rebuild_tiff);
    it.define_native("Image::ExifTool::Exif::ExtractImage", extract_image);
    it.define_native("Image::ExifTool::Exif::TagName", tag_name);
    it.define_native("Image::ExifTool::Exif::NextOffsetName", next_offset_name);
    it.define_native("Image::ExifTool::Exif::ProcessExif", process_exif);
}
