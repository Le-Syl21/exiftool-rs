//! Image::ExifTool::MWG (Image/ExifTool/MWG.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};
use crate::runtime;

const MODULE: &str = "Image::ExifTool::MWG";

//------------------------------------------------------------------------------
// Load the MWG Composite tags
pub fn load(it: &mut Interp, _a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let mwg_loaded = runtime::lex_scalar(it, MODULE, "mwgLoaded");
    if mwg_loaded.borrow().truthy() {
        return pl::ret_nothing(want);
    }

    // add our composite tags
    pl::call(it, "Image::ExifTool::AddCompositeTags", vec![pl::s("Image::ExifTool::MWG")])?;
    // must also add to lookup so we can write them
    // (since MWG tags aren't in the tag lookup by default)
    let comp = pl::pkg_hash(it, "Image::ExifTool::MWG::Composite");
    pl::call(it, "Image::ExifTool::AddTagsToLookup", vec![comp, pl::s("Image::ExifTool::Composite")])?;

    // modify EXIF:Artist to behave as a list-type tag
    let exif_main = pl::pkg_hash(it, "Image::ExifTool::Exif::Main");
    let artist = exif_main.get(b"315"); // 0x13b
    artist.set("List", n(1));
    artist.set("IsOverwriting", Value::Code(it.sub_ref("Image::ExifTool::MWG::OverwriteStringList")));
    artist.set("RawConv", Value::Code(it.sub_ref("Image::ExifTool::MWG::StringToList")));

    // enable MWG strict mode if not set already
    // (causes non-standard EXIF, IPTC and XMP to be ignored)
    if it.get_scalar("Image::ExifTool::MWG::strict").is_undef() {
        it.set_scalar("Image::ExifTool::MWG::strict", n(1));
    }

    *mwg_loaded.borrow_mut() = n(1);
    ret(n(1))
}

//------------------------------------------------------------------------------
// Change a list of values to a string using MWG rules
// Inputs: 0)reference to list of values
// Returns: string of values (and may reformat list entries)
pub fn list_to_string(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let vals = arg(&a, 0);
    // (foreach autovivifies an undefined $vals; anything else but an ARRAY
    // reference dies under "strict refs")
    let mut items = if vals.is_undef() {
        Vec::new()
    } else {
        pl::at(582);
        pl::aderef(&vals)?
    };
    for v in items.iter_mut() {
        // double all quotes in value and quote the value if it begins
        // with a quote or contains a semicolon-space separator
        if re(r#"^""#, "").is_match(v) || re("; ", "").is_match(v) {
            re(r#"""#, "").subst(v, true, |_| b"\"\"".to_vec()); // double all quotes
            *v = pl::concat(&pl::concat(&pl::s("\""), v), &pl::s("\"")); // quote the value
        }
    }
    if let Value::ArrayRef(rc) = &vals {
        *rc.borrow_mut() = items.clone();
    }
    ret(pl::join("; ", &items))
}

//------------------------------------------------------------------------------
// Change a string value to a list of values using MWG rules
// Inputs: 0) string of values, 1) ExifTool ref
// Returns: value or list reference if more than one value
// Notes: Sets Warning tag on error
pub fn string_to_list(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (str_, et) = (arg(&a, 0), arg(&a, 1));
    let mut vals: Vec<Value> = Vec::new();
    let mut in_quotes = Value::Undef;
    let t = pl::split(&re("; ", ""), &str_, -1);
    for mut v in t {
        let was_quotes = in_quotes.clone();
        if !in_quotes.truthy() && re(r#"^""#, "").subst(&mut v, false, |_| Vec::new()) > 0 {
            in_quotes = n(1);
        }
        if in_quotes.truthy() {
            // remove the last quote and reset the inQuotes flag if
            // the value ended in an odd number of quotes
            if re(r#"((^|[^"])("")*)"$"#, "").subst(&mut v, false, |c| cap(c, 1).to_bytes().to_vec()) > 0 {
                in_quotes = n(0);
            }
            re(r#""""#, "").subst(&mut v, true, |_| b"\"".to_vec()); // un-double the contained quotes
        }
        if was_quotes.truthy() {
            // previous separator was quoted, so concatinate with previous value
            let last = vals.len() - 1;
            vals[last] = pl::concat(&pl::concat(&vals[last], &pl::s("; ")), &v);
        } else {
            vals.push(v);
        }
    }
    if in_quotes.truthy() {
        pl::method(it, &et, "Warn", vec![pl::s("Incorrectly quoted MWG string-list value")])?;
    }
    if vals.len() > 1 {
        ret(Value::new_array(vals))
    } else {
        ret(vals.into_iter().next().unwrap_or_default())
    }
}

//------------------------------------------------------------------------------
// Handle logic for overwriting EXIF string-type list tag
// Inputs: 0) ExifTool ref, 1) new value hash ref,
//         2) old string value (or undef if it didn't exist), 3) new value ref
// Returns: 1 and sets the new value for the tag
pub fn overwrite_string_list(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| {
        let (et, nv_hash, val, new_value_pt) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
        let mut new: Vec<Value> = Vec::new();
        let mut del_index = Value::Undef;
        let write_mode = pl::method(it, &et, "Options", vec![pl::s("WriteMode")])?;
        if !pl::str_eq(&write_mode, "wcg") {
            if !val.is_undef() {
                if !re("w", "i").is_match(&write_mode) {
                    return ret(n(0));
                }
            } else if !re("c", "i").is_match(&write_mode) {
                return ret(n(0));
            }
        }
        if nv_hash.get("DelValue").truthy() && !val.is_undef() {
            // preserve specified old values
            let old = string_to_list(it, vec![val.clone(), et.clone()], Want::Scalar)?.pop().unwrap_or_default();
            let old: Vec<Value> = if pl::ref_of(&old) == "ARRAY" { old.items() } else { vec![old] };
            let del_value = nv_hash.get("DelValue");
            pl::at(645);
            if !pl::aderef(&del_value)?.is_empty() {
                let del = Value::new_hash();
                for d in pl::aderef(&del_value)? {
                    del.set(&*d.to_bytes(), n(1));
                }
                for o in old {
                    if !del.get(&*o.to_bytes()).truthy() {
                        new.push(o);
                        continue;
                    }
                    if !del_index.truthy() {
                        del_index = n(new.len() as i64);
                    }
                }
            } else {
                new.extend(old);
            }
        }
        // add new values (at location of deleted values, if any)
        let value = nv_hash.get("Value");
        if value.truthy() {
            if !del_index.is_undef() {
                let at = (del_index.to_i64().max(0) as usize).min(new.len());
                pl::at(659);
                new.splice(at..at, pl::aderef(&value)?);
            } else {
                pl::at(661);
                new.extend(pl::aderef(&value)?);
            }
        }
        if !new.is_empty() {
            // convert back to string format
            let s = list_to_string(it, vec![Value::new_array(new)], Want::Scalar)?.pop().unwrap_or_default();
            pl::set_deref(&new_value_pt, s)?;
        } else {
            pl::set_deref(&new_value_pt, Value::Undef)?; // delete the tag
        }
        ret(n(1))
    })
}

//------------------------------------------------------------------------------
// Reconcile IPTC digest after writing an MWG tag
// Inputs: 0) ExifTool object ref
// Returns: empty string
pub fn reconcile_iptc_digest(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let et = arg(&a, 0);

    // set new value for IPTCDigest if not done already
    let info = it.get_scalar("Image::ExifTool::Photoshop::iptcDigestInfo");
    if !(info.truthy() && et.get("NEW_VALUE").get(&*info.to_bytes()).truthy()) {
        // write new IPTCDigest only if it doesn't exist or
        // is the same as the digest of the original IPTC
        // (capture warning messages)
        let _a = pl::method_list(it, &et, "SetNewValue", vec![pl::s("Photoshop:IPTCDigest"), pl::s("old"), pl::s("Protected"), n(1), pl::s("DelValue"), n(1)])?;
        let _a = pl::method_list(it, &et, "SetNewValue", vec![pl::s("Photoshop:IPTCDigest"), pl::s("new"), pl::s("Protected"), n(1)])?;
    }
    ret(pl::s(""))
}

/// `$$r[i]` read under strict refs: an undefined `$r` would be autovivified
/// (undef here), anything but an ARRAY reference dies.
fn aelem(r: &Value, i: i64) -> ER<Value> {
    if !r.is_undef() {
        pl::aderef(r)?;
    }
    Ok(r.at(i))
}

//------------------------------------------------------------------------------
// Recover strings which were truncated by IPTC dataset length limit
// Inputs: 0) IPTC value, 1) XMP value, 2) length limit
// Notes: handles the case where IPTC and/or XMP values are lists
pub fn recover_truncated_iptc(it: &mut Interp, a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    let (iptc, mut xmp, limit) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));

    if xmp.is_undef() {
        return ret(iptc);
    }
    if iptc.is_ref() {
        if !xmp.is_ref() {
            xmp = Value::new_array(vec![xmp]);
        }
        let mut vals = Vec::new();
        pl::at(706);
        let items = pl::aderef(&iptc)?;
        for (i, v) in items.iter().enumerate() {
            pl::at(706);
            let x = aelem(&xmp, i as i64)?;
            vals.extend(recover_truncated_iptc(it, vec![v.clone(), x, limit.clone()], w)?);
        }
        return ret(Value::new_array(vals));
    } else if !iptc.is_undef() && pl::num_eq(&pl::length(&iptc), &limit) {
        if xmp.is_ref() {
            pl::at(711);
            xmp = aelem(&xmp, 0)?; // take first element of list
        }
        if pl::gt(&pl::length(&xmp), &limit) && iptc.to_bytes() == pl::substr(&xmp, 0, Some(limit.to_i64())).to_bytes() {
            return ret(xmp);
        }
    }
    ret(iptc)
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::MWG::Load", load);
    it.define_native("Image::ExifTool::MWG::ListToString", list_to_string);
    it.define_native("Image::ExifTool::MWG::StringToList", string_to_list);
    it.define_native("Image::ExifTool::MWG::OverwriteStringList", overwrite_string_list);
    it.define_native("Image::ExifTool::MWG::ReconcileIPTCDigest", reconcile_iptc_digest);
    it.define_native("Image::ExifTool::MWG::RecoverTruncatedIPTC", recover_truncated_iptc);
}
