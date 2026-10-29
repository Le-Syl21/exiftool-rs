//! Image::ExifTool::FITS (Image/ExifTool/FITS.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

/// `$raf->Read($buff, $len) == $len`
fn read_eq(it: &mut Interp, raf: &Value, buff: &mut Value, len: i64) -> ER<bool> {
    let mut args = [buff.clone(), n(len)];
    let r = pl::method_mut(it, raf, "Read", &mut args, Want::Scalar)?.pop().unwrap_or_default();
    *buff = args[0].clone();
    Ok(pl::num_eq(&r, &n(len)))
}

fn warn(it: &mut Interp, et: &Value, msg: &str) -> ER<()> {
    pl::method(it, et, "Warn", vec![pl::s(msg)])?;
    Ok(())
}

/// Read information in a FITS file
/// Inputs: 0) ExifTool ref, 1) dirInfo ref
/// Returns: 1 on success, 0 if this wasn't a valid FITS file
pub fn process_fits(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let (mut buff, mut tag, mut cont) = (Value::Undef, Value::Undef, Value::Undef);

    if !(read_eq(it, &raf, &mut buff, 80)? && re(r"^SIMPLE  = {20}T", "").is_match(&buff)) {
        return ret(n(0));
    }
    pl::method(it, &et, "SetFileType", vec![])?;
    let tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![pl::s("Image::ExifTool::FITS::Main")])?;

    loop {
        if !read_eq(it, &raf, &mut buff, 80)? {
            warn(it, &et, "Truncated FITS header")?;
            break;
        }
        let mut key = pl::substr(&buff, 0, Some(8));
        re(r" +$", "").subst(&mut key, false, |_| Vec::new()); // remove trailing space from key
        if pl::str_eq(&key, "CONTINUE") {
            if cont.is_undef() {
                warn(it, &et, "Unexpected FITS CONTINUE keyword")?;
                continue;
            }
        } else {
            if !cont.is_undef() {
                // the previous value wasn't continued, so store with the trailing '&'
                pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tag.clone(), pl::concat(&cont, &pl::s("&"))])?;
                cont = Value::Undef;
            }
            if pl::str_eq(&key, "END") {
                break;
            }
            // make sure the key is valid
            if !re(r"^[-_A-Z0-9]*$", "").is_match(&key) {
                warn(it, &et, "Format error in FITS header")?;
                break;
            }
            if pl::str_eq(&key, "COMMENT") || pl::str_eq(&key, "HISTORY") {
                let mut val = pl::substr(&buff, 8, None); // comments start in column 9
                re(r" +$", "").subst(&mut val, false, |_| Vec::new()); // remove trailing spaces
                pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), key.clone(), val])?;
                continue;
            }
            // ignore other lines that aren't tags
            if !pl::str_eq(&pl::substr(&buff, 8, Some(2)), "= ") {
                continue;
            }
            // save tag name (avoiding potential conflict with ExifTool variables)
            let special = pl::pkg_hash(it, "Image::ExifTool::specialTags");
            tag = if special.get(key.to_bytes()).truthy() { pl::concat(&pl::s("_"), &key) } else { key.clone() };
            // add to tag table if necessary
            if !tag_table_ptr.get(tag.to_bytes()).truthy() {
                let mut name = pl::ucfirst(&pl::lc(&tag)); // make tag name lower case with leading capital
                re(r"_(.)", "").subst(&mut name, true, |c| pl::uc(&cap(c, 1)).to_bytes().to_vec()); // remove all '_' and capitalize subsequent letter
                let ti = Value::new_hash();
                ti.set("Name", name);
                pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), ti])?;
            }
        }
        let mut val = pl::substr(&buff, 10, None);
        // parse quoted values
        if let Some(m) = re(r"^'(.*?)'(.*)", "").m(&val) {
            (val, buff) = (cap(&m, 1), cap(&m, 2));
            // handle escaped quotes
            while let Some(m) = re(r"^('.*?)'(.*)", "").m(&buff) {
                val = pl::concat(&val, &cap(&m, 1));
                buff = cap(&m, 2);
            }
            re(r" +$", "").subst(&mut val, false, |_| Vec::new()); // remove trailing spaces
            if !cont.is_undef() {
                val = pl::concat(&cont, &val);
                cont = Value::Undef;
            }
            // check for possible continuation, removing trailing '&'
            if re(r"\&$", "").subst(&mut val, false, |_| Vec::new()) > 0 {
                cont = val;
                continue;
            }
        } else if !cont.is_undef() {
            warn(it, &et, "Invalid FITS CONTINUE value")?;
            continue;
        } else {
            re(r" *(\/.*)?$", "").subst(&mut val, false, |_| Vec::new()); // remove trailing spaces and comment
            if pl::length(&val).to_i64() == 0 {
                continue; // ignore undefined values
            }
            re(r"^ +", "").subst(&mut val, false, |_| Vec::new()); // remove leading spaces
            // re-format floating-point values to use 'e'
            if re(r"^[+-]?(?=\d|\.\d)\d*(\.\d*)?([ED]([+-]?\d+))?$", "").is_match(&val) {
                pl::tr(&mut val, "DE", "e", "");
            }
        }
        pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tag.clone(), val])?;
    }
    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::FITS::ProcessFITS", process_fits);
}
