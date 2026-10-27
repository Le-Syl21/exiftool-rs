//! Image::ExifTool::Charset (Image/ExifTool/Charset.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, n, re, ret};
use crate::runtime;

const MODULE: &str = "Image::ExifTool::Charset";

/// `$a & $b` as a number.
fn band(a: &Value, b: i64) -> Value {
    ops::bit_and(a, &n(b))
}

/// `$a >> $b`
fn shr(a: &Value, b: i64) -> Value {
    ops::shift_right(a, &n(b))
}

/// `$a << $b`
fn shl(a: &Value, b: i64) -> Value {
    ops::shift_left(a, &n(b))
}

/// `%csType` (a `use vars` package variable).
fn cs_type(it: &mut Interp) -> Value {
    pl::pkg_hash(it, "Image::ExifTool::Charset::csType")
}

/// `foreach (@$r)` (also `f(@$r)`, sub arguments) on a variable: an
/// undefined `$r` is autovivified to an empty array, anything else is read
/// as `@$r` under strict refs.
fn foreach_av(r: &mut Value) -> ER<Vec<Value>> {
    if r.is_undef() {
        *r = Value::new_array(Vec::new());
        return Ok(Vec::new());
    }
    pl::aderef(r)
}

/// `$$h{$k}` with a value as the key.
fn hget(h: &Value, k: &Value) -> Value {
    h.get(&*k.to_bytes())
}

//------------------------------------------------------------------------------
// Load character set module
// Inputs: 0) Module name
// Returns: Reference to lookup hash, or undef on error
pub fn load_charset(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let charset = arg(&a, 0);
    let charset_table = runtime::lex_hash(it, MODULE, "charsetTable");
    let mut conv = hget(&charset_table, &charset);
    if !conv.truthy() {
        // load translation module
        let module = format!("Image::ExifTool::Charset::{}", charset.to_text());
        // no strict 'refs';
        let h = pl::pkg_hash(it, &module);
        let mut ok = !h.is_empty();
        if !ok {
            // eval "require $module"
            let scope = crate::interp::Scope::new(None);
            let code = format!("require {module}");
            ok = match it.eval_string(code.as_bytes(), MODULE, &scope, Want::Scalar) {
                Ok(v) => v.last().is_some_and(|v| v.truthy()),
                Err(_) => false,
            };
        }
        if ok {
            conv = pl::pkg_hash(it, &module);
            charset_table.set(&*charset.to_bytes(), conv.clone());
        }
    }
    ret(conv)
}

//------------------------------------------------------------------------------
// Does an array contain valid UTF-16 characters?
// Inputs: 0) array reference to list of UCS-2 values
// Returns: 0=invalid UTF-16, 1=valid UTF-16 with no surrogates, 2=valid UTF-16 with surrogates
pub fn is_utf16(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut uni = arg(&a, 0);
    let mut surrogate = Value::Undef;
    pl::at(122);
    for v in foreach_av(&mut uni)? {
        let hi_bits = band(&v, 0xfc00);
        if pl::num_eq(&hi_bits, &n(0xfc00)) {
            // check for invalid values in UTF-16
            if pl::num_eq(&v, &n(0xffff)) || pl::num_eq(&v, &n(0xfffe)) || (pl::ge(&v, &n(0xfdd0)) && pl::le(&v, &n(0xfdef))) {
                return ret(n(0));
            }
        } else if surrogate.truthy() {
            if !pl::num_eq(&hi_bits, &n(0xdc00)) {
                return ret(n(0));
            }
            surrogate = n(0);
        } else {
            if pl::num_eq(&hi_bits, &n(0xdc00)) {
                return ret(n(0));
            }
            if pl::num_eq(&hi_bits, &n(0xd800)) {
                surrogate = n(1);
            }
        }
    }
    if surrogate.is_undef() {
        return ret(n(1));
    }
    if !surrogate.truthy() {
        return ret(n(2));
    }
    ret(n(0))
}

//------------------------------------------------------------------------------
// Decompose string with specified encoding into an array of integer code points
// Inputs: 0) ExifTool object ref (or undef), 1) string, 2) character set name,
//         3) optional byte order ('II','MM','Unknown' or undef to use ExifTool ordering)
// Returns: Reference to array of Unicode values
// Notes: Accepts any type of character set
// - byte order only used for fixed-width 2-byte and 4-byte character sets
// - byte order mark observed and then removed with UCS2 and UCS4
// - no warnings are issued if ExifTool object is not provided
// - sets ExifTool WrongByteOrder flag if byte order is Unknown and current order is wrong
pub fn decompose(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| decompose_body(it, a))
}

fn decompose_body(it: &mut Interp, a: Vec<Value>) -> ER<Vec<Value>> {
    let (et, mut val, charset) = (arg(&a, 0), arg(&a, 1), arg(&a, 2)); // ($byteOrder assigned later if required)
    let cst = cs_type(it);
    let type_ = hget(&cst, &charset);
    let mut uni: Vec<Value> = Vec::new();
    let mut conv = Value::Undef;

    if band(&type_, 0x001).truthy() {
        conv = load_charset(it, vec![charset.clone()], Want::Scalar)?.pop().unwrap_or_default();
        if !conv.truthy() {
            // (shouldn't happen)
            if et.truthy() {
                pl::method(it, &et, "Warn", vec![pl::s(&format!("Invalid character set {}", charset.to_text()))])?;
            }
            return ret(Value::new_array(uni)); // error!
        }
    } else if pl::num_eq(&type_, &n(0x100)) {
        // convert ASCII and UTF8 (treat ASCII as UTF8)
        let perl_ver = it.get_scalar("main::]");
        if pl::lt(&perl_ver, &Value::Num(5.006001)) {
            // do it ourself
            uni = pl::call_list(it, "Image::ExifTool::UnpackUTF8", vec![val.clone()])?;
        } else {
            // handle warnings from malformed UTF-8
            it.set_scalar("Image::ExifTool::evalWarning", Value::Undef);
            let handler = Value::Code(it.sub_ref("Image::ExifTool::SetWarning"));
            uni = pl::local_sig_warn(it, handler, |it| {
                // (somehow the meaning of "U0" was reversed in Perl 5.10.0!)
                let t = if pl::lt(&it.get_scalar("main::]"), &Value::Num(5.010000)) { "U0U*" } else { "C0U*" };
                let uni = pl::unpack_warn(it, t, &val)?;
                // issue warning if we had errors
                if it.get_scalar("Image::ExifTool::evalWarning").truthy() && et.truthy() && !et.get("WarnBadUTF8").truthy() {
                    pl::method(it, &et, "Warn", vec![pl::s("Malformed UTF-8 character(s)")])?;
                    et.set("WarnBadUTF8", n(1));
                }
                Ok(uni)
            })?;
        }
        return ret(Value::new_array(uni)); // all done!
    }
    if band(&type_, 0x100).truthy() {
        // 1-byte fixed-width characters
        uni = pl::unpack("C*", &val)?;
        for v in uni.iter_mut() {
            let c = hget(&conv, v);
            if !c.is_undef() {
                *v = c;
            }
        }
    } else if band(&type_, 0x600).truthy() {
        // 2-byte or 4-byte fixed-width characters
        let mut unknown = false;
        let mut byte_order = arg(&a, 3);
        if !byte_order.truthy() {
            byte_order = pl::call(it, "Image::ExifTool::GetByteOrder", vec![])?;
        } else if pl::str_eq(&byte_order, "Unknown") {
            byte_order = pl::call(it, "Image::ExifTool::GetByteOrder", vec![])?;
            unknown = true;
        }
        let mut fmt = pl::s(if pl::str_eq(&byte_order, "MM") { "n*" } else { "v*" });
        if band(&type_, 0x400).truthy() {
            // 4-byte
            fmt = pl::uc(&fmt); // unpack as 'N*' or 'V*'
            // honour BOM if it exists
            let mut bom = Value::Undef;
            if re(r"^(\0\0\xfe\xff|\xff\xfe\0\0)", "").subst(&mut val, false, |c| {
                bom = pl::cap(c, 1);
                Vec::new()
            }) > 0
            {
                fmt = pl::s(if &bom.to_bytes()[..] == b"\0\0\xfe\xff" { "N*" } else { "V*" });
            }
            unknown = false; // (byte order logic applies to 2-byte only)
        } else {
            let mut bom = Value::Undef;
            if re(r"^(\xfe\xff|\xff\xfe)", "").subst(&mut val, false, |c| {
                bom = pl::cap(c, 1);
                Vec::new()
            }) > 0
            {
                fmt = pl::s(if &bom.to_bytes()[..] == b"\xfe\xff" { "n*" } else { "v*" });
                unknown = false;
            }
        }
        // convert from UCS2 or UCS4
        uni = pl::unpack(&fmt.to_text(), &val)?;

        if !conv.truthy() {
            // no translation necessary
            if unknown {
                // check the byte order
                let (mut bh, mut bl) = (std::collections::HashSet::new(), std::collections::HashSet::new());
                let (mut zh, mut zl) = (0i64, 0i64);
                for v in &uni {
                    bh.insert(shr(v, 8).to_bytes().to_vec());
                    bl.insert(band(v, 0xff).to_bytes().to_vec());
                    if !band(v, 0xff00).truthy() {
                        zh += 1;
                    }
                    if !band(v, 0x00ff).truthy() {
                        zl += 1;
                    }
                }
                // count the number of unique values in the hi and lo bytes
                let (bh, bl) = (bh.len(), bl.len());
                // the byte with the greater number of unique values should be
                // the low-order byte, otherwise the byte which is zero more
                // often is likely the high-order byte
                if bh > bl || (bh == bl && zl > zh) {
                    // we guessed wrong, so decode using the other byte order
                    pl::tr(&mut fmt, "nvNV", "vnVN", "");
                    uni = pl::unpack(&fmt.to_text(), &val)?;
                    et.set("WrongByteOrder", n(1));
                }
            }
            // handle surrogate pairs of UTF-16
            if pl::str_eq(&charset, "UTF16") {
                let mut i = 0usize;
                while (i as i64) < uni.len() as i64 - 1 {
                    if pl::num_eq(&band(&uni[i], 0xfc00), &n(0xd800)) && pl::num_eq(&band(&uni[i + 1], 0xfc00), &n(0xdc00)) {
                        let cp = pl::add(&pl::add(&n(0x10000), &shl(&band(&uni[i], 0x3ff), 10)), &band(&uni[i + 1], 0x3ff));
                        uni.splice(i..i + 2, [cp]);
                    }
                    i += 1;
                }
            }
        } else if unknown {
            // count encoding errors as we do the translation
            let mut e1 = 0;
            for v in uni.iter_mut() {
                let c = hget(&conv, v);
                if !c.is_undef() {
                    *v = c;
                    continue;
                }
                e1 += 1;
            }
            // try the other byte order if we had any errors
            if e1 != 0 {
                let fmt = if pl::str_eq(&byte_order, "MM") { "v*" } else { "n*" }; //(reversed)
                let mut tr_ = pl::unpack(fmt, &val)?;
                let mut e2 = 0;
                for v in tr_.iter_mut() {
                    let c = hget(&conv, v);
                    if !c.is_undef() {
                        *v = c;
                        continue;
                    }
                    e2 += 1;
                }
                // use this byte order if there are fewer errors
                if e2 < e1 {
                    et.set("WrongByteOrder", n(1));
                    return ret(Value::new_array(tr_));
                }
            }
        } else {
            // translate any characters found in the lookup
            for v in uni.iter_mut() {
                let c = hget(&conv, v);
                if !c.is_undef() {
                    *v = c;
                }
            }
        }
    } else {
        // variable-width characters
        // unpack into bytes
        let mut bytes: std::collections::VecDeque<Value> = pl::unpack("C*", &val)?.into();
        while let Some(mut ch) = bytes.pop_front() {
            let mut cv = hget(&conv, &ch);
            // pass straight through if no translation
            if !cv.truthy() {
                uni.push(ch);
                continue;
            }
            // byte translates into single Unicode character
            if !cv.is_ref() {
                uni.push(cv);
                continue;
            }
            // byte maps into multiple Unicode characters
            if pl::ref_of(&cv) == "ARRAY" {
                uni.extend(cv.items());
                continue;
            }
            // handle 2-byte character codes
            match bytes.pop_front() {
                Some(c2) => {
                    ch = c2;
                    if hget(&cv, &ch).truthy() {
                        cv = hget(&cv, &ch);
                        if !cv.is_ref() {
                            uni.push(cv);
                            continue;
                        }
                        uni.extend(cv.items()); // multiple Unicode characters
                    } else {
                        uni.push(n(b'?' as i64)); // encoding error
                        bytes.push_front(ch);
                    }
                }
                None => {
                    uni.push(n(b'?' as i64)); // encoding error
                }
            }
        }
    }
    ret(Value::new_array(uni))
}

//------------------------------------------------------------------------------
// Convert array of code point integers into a string with specified encoding
// Inputs: 0) ExifTool ref (or undef), 1) unicode character array ref,
//         2) character set (note: not all types are supported)
//         3) byte order ('MM' or 'II', multi-byte sets only, defaults to current byte order)
// Returns: converted string (truncated at null character if it exists), empty on error
// Notes: converts elements of input character array to new code points
// - ExifTool ref may be undef provided $charset is defined
pub fn recompose(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| recompose_body(it, a))
}

/// Replace the elements of the array `@$r` (the caller's array is changed
/// in place, as `foreach (@$uni) { $_ = ... }` does).
fn set_items(r: &Value, items: Vec<Value>) {
    if let Value::ArrayRef(rc) = r {
        *rc.borrow_mut() = items;
    }
}

fn recompose_body(it: &mut Interp, a: Vec<Value>) -> ER<Vec<Value>> {
    let (et, mut uni, mut charset) = (arg(&a, 0), arg(&a, 1), arg(&a, 2)); // ($byteOrder assigned later if required)
    let mut out_val;
    let (mut conv, mut inv) = (Value::Undef, Value::Undef);
    if !charset.truthy() {
        charset = et.get("OPTIONS").get("Charset");
    }
    let cst = cs_type(it);
    let cs_type = hget(&cst, &charset);
    if pl::num_eq(&cs_type, &n(0x100)) {
        // UTF8 (also treat ASCII as UTF8)
        let perl_ver = it.get_scalar("main::]");
        if pl::ge(&perl_ver, &Value::Num(5.006001)) {
            // let Perl do it
            pl::at(321);
            out_val = pl::pack("C0U*", &pl::aderef(&uni)?)?;
        } else {
            // do it ourself
            pl::at(324);
            let items = foreach_av(&mut uni)?;
            out_val = pl::call(it, "Image::ExifTool::PackUTF8", items)?;
        }
        re(r"\0.*", "s").subst(&mut out_val, false, |_| Vec::new()); // truncate at null terminator
        return ret(out_val);
    }
    // get references to forward and inverse lookup tables
    if band(&cs_type, 0x801).truthy() {
        conv = load_charset(it, vec![charset.clone()], Want::Scalar)?.pop().unwrap_or_default();
        if !conv.truthy() {
            if et.truthy() {
                pl::method(it, &et, "Warn", vec![pl::s(&format!("Missing charset {}", charset.to_text()))])?;
            }
            return ret(pl::s(""));
        }
        let unicode2byte = runtime::lex_hash(it, MODULE, "unicode2byte");
        inv = hget(&unicode2byte, &charset);
        // generate inverse lookup if necessary
        if !inv.truthy() {
            if !cs_type.truthy() || band(&cs_type, 0x802).truthy() {
                if et.truthy() {
                    pl::method(it, &et, "Warn", vec![pl::s(&format!("Invalid destination charset {}", charset.to_text()))])?;
                }
                return ret(pl::s(""));
            }
            // prepare table to convert from Unicode to 1-byte characters
            let inv_h = Value::new_hash();
            for ch in conv.keys() {
                inv_h.set(&*conv.get(&*ch).to_bytes(), Value::bytes(&ch[..]));
            }
            inv = inv_h;
            unicode2byte.set(&*charset.to_bytes(), inv.clone());
        }
    }
    if band(&cs_type, 0x100).truthy() {
        // 1-byte fixed-width
        // convert to specified character set
        pl::at(353);
        let mut items = foreach_av(&mut uni)?;
        for v in items.iter_mut() {
            if pl::lt(v, &n(0x80)) {
                continue;
            }
            let x = hget(&inv, v);
            if x.truthy() {
                *v = x;
                continue;
            }
            // our tables omit 1-byte characters with the same values as Unicode,
            // so pass them straight through after making sure there isn't a
            // different character with this byte value
            if pl::lt(v, &n(0x100)) && !hget(&conv, v).truthy() {
                continue;
            }
            *v = n(b'?' as i64); // set invalid characters to '?'
            if et.truthy() && !et.get("EncodingError").truthy() {
                pl::method(it, &et, "Warn", vec![pl::s(&format!("Some character(s) could not be encoded in {}", charset.to_text()))])?;
                et.set("EncodingError", n(1));
            }
        }
        set_items(&uni, items.clone());
        // repack as an 8-bit string and truncate at null
        out_val = pl::pack("C*", &items)?;
        re(r"\0.*", "s").subst(&mut out_val, false, |_| Vec::new());
    } else {
        // 2-byte and 4-byte fixed-width
        // convert if required
        if inv.truthy() {
            pl::at(372);
            let mut items = foreach_av(&mut uni)?;
            for v in items.iter_mut() {
                let x = hget(&inv, v);
                if x.truthy() {
                    *v = x;
                }
            }
            set_items(&uni, items);
        }
        // generate surrogate pairs of UTF-16
        if pl::str_eq(&charset, "UTF16") {
            pl::at(377);
            let mut items = pl::aderef(&uni)?;
            let mut i = 0usize;
            while i < items.len() {
                if pl::ge(&items[i], &n(0x10000)) && pl::lt(&items[i], &n(0x10ffff)) {
                    let t = pl::sub(&items[i], &n(0x10000));
                    let w1 = pl::add(&n(0xd800), &band(&shr(&t, 10), 0x3ff));
                    let w2 = pl::add(&n(0xdc00), &band(&t, 0x3ff));
                    items.splice(i..i + 1, [w1, w2]);
                    i += 1; // skip surrogate pair
                }
                i += 1;
            }
            set_items(&uni, items);
        }
        // pack as 2- or 4-byte integer in specified byte order
        let mut byte_order = arg(&a, 3);
        if !byte_order.truthy() {
            byte_order = pl::call(it, "Image::ExifTool::GetByteOrder", vec![])?;
        }
        let mut fmt = pl::s(if pl::str_eq(&byte_order, "MM") { "n*" } else { "v*" });
        if band(&cs_type, 0x400).truthy() {
            fmt = pl::uc(&fmt);
        }
        pl::at(390);
        out_val = pl::pack(&fmt.to_text(), &pl::aderef(&uni)?)?;
    }
    ret(out_val)
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Charset::LoadCharset", load_charset);
    it.define_native("Image::ExifTool::Charset::IsUTF16", is_utf16);
    it.define_native("Image::ExifTool::Charset::Decompose", decompose);
    it.define_native("Image::ExifTool::Charset::Recompose", recompose);
}
