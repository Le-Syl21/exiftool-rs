//! Image::ExifTool::Validate (Image/ExifTool/Validate.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, n, re, ret};
use crate::runtime;

const MODULE: &str = "Image::ExifTool::Validate";

/// `$$h{$k}` with a value as the key.
fn hget(h: &Value, k: &Value) -> Value {
    h.get(&*k.to_bytes())
}

/// A file lexical hash of Validate.pm (`my %name`).
fn my_hash(it: &mut Interp, name: &str) -> Value {
    runtime::lex_hash(it, MODULE, name)
}

/// `sort keys %$h` (string order).
fn sorted_keys(h: &Value) -> Vec<Value> {
    let mut k: Vec<Value> = h.keys().into_iter().map(|k| Value::bytes(&k[..])).collect();
    k.sort_by(|a, b| a.to_bytes().cmp(&b.to_bytes()));
    k
}

/// `sort { $a <=> $b } keys %$h`
fn sorted_keys_num(h: &Value) -> Vec<Value> {
    let mut k: Vec<Value> = h.keys().into_iter().map(|k| Value::bytes(&k[..])).collect();
    k.sort_by(|a, b| ops::num_cmp(a, b).unwrap_or(std::cmp::Ordering::Equal));
    k
}

/// `\&Image::ExifTool::SetWarning`
fn set_warning_ref(it: &mut Interp) -> Value {
    Value::Code(it.sub_ref("Image::ExifTool::SetWarning"))
}

//------------------------------------------------------------------------------
// Validate the raw value of a tag
// Inputs: 0) ExifTool ref, 1) tag key, 2) raw tag value
// Returns: nothing, but issues a minor Warning if a problem was detected
pub fn validate_raw(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (self_, tag, val) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let tag_info = hget(&self_.get("TAG_INFO"), &tag);
    let mut wrn = Value::Undef;

    // evaluate Validate code if specified
    if tag_info.get("Validate").truthy() {
        let handler = set_warning_ref(it);
        pl::local_sig_warn(it, handler, |it| {
            it.set_scalar("Image::ExifTool::evalWarning", Value::Undef);
            //### eval Validate ($self, $val, $tagInfo)
            let scope = runtime::file_scope(it, MODULE);
            let (wrn, _) = pl::eval_site_in(it, &tag_info.get("Validate"), MODULE, Some(scope), &[("self", self_.clone()), ("val", val.clone()), ("tagInfo", tag_info.clone())], Want::Scalar);
            let mut err = it.get_scalar("Image::ExifTool::evalWarning");
            if !err.truthy() {
                err = it.get_scalar("main::@");
            }
            if wrn.truthy() || err.truthy() {
                let tag_name = pl::call(it, "Image::ExifTool::GetTagName", vec![tag.clone()])?;
                let name = pl::concat(&pl::concat(&tag_info.get("Table").get("GROUPS").get("0"), &pl::s(":")), &tag_name);
                if err.truthy() {
                    let m = pl::concat(&pl::concat(&pl::concat(&pl::s("Validate "), &name), &pl::s(": ")), &err);
                    pl::method(it, &self_, "Warn", vec![m, n(1)])?;
                }
                if wrn.truthy() {
                    let m = pl::concat(&pl::concat(&wrn, &pl::s(" for ")), &name);
                    pl::method(it, &self_, "Warn", vec![m, n(1)])?;
                }
            }
            Ok(())
        })?;
    }
    // check for unknown values in PrintConv lookup for all standard EXIF tags
    let exif_main = pl::pkg_hash(it, "Image::ExifTool::Exif::Main");
    let exif_spec = pl::pkg_hash(it, "Image::ExifTool::Validate::exifSpec");
    let table = tag_info.get("Table");
    if pl::ref_of(&tag_info.get("PrintConv")) == "HASH" && (pl::str_eq(&table.get("SHORT_NAME"), "GPS::Main") || (table.same_ref(&exif_main) && hget(&exif_spec, &tag_info.get("TagID")).truthy())) {
        let prt = pl::method(it, &self_, "GetValue", vec![tag.clone(), pl::s("PrintConv")])?;
        if prt.truthy() && re(r"^Unknown \(", "").is_match(&prt) {
            wrn = pl::s("Unknown value for");
        }
    }
    if pl::str_eq(&val, "undef") {
        wrn = pl::s("Undefined value for");
    }
    if wrn.truthy() {
        let tag_name = pl::call(it, "Image::ExifTool::GetTagName", vec![tag.clone()])?;
        let name = pl::concat(&pl::concat(&self_.get("DIR_NAME"), &pl::s(":")), &tag_name);
        let m = pl::concat(&pl::concat(&wrn, &pl::s(" ")), &name);
        pl::method(it, &self_, "Warn", vec![m, n(1)])?;
    }
    pl::ret_nothing(want)
}

/// `$validDateField[$i][$j]`
fn valid_date_field(it: &mut Interp, i: usize, j: i64) -> Value {
    runtime::lex_array(it, MODULE, "validDateField").at(i as i64).at(j)
}

//------------------------------------------------------------------------------
// Validate raw EXIF date/time value
// Inputs: 0) date/time value
// Returns: error string
pub fn validate_exif_date(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let val = arg(&a, 0);
    if let Some(c) = re(r"^\d{4}:(\d{2}):(\d{2}) (\d{2}):(\d{2}):(\d{2})$", "").m(&val) {
        let a: Vec<Value> = (1..=5).map(|i| cap(&c, i)).collect();
        let mut bad = Vec::new();
        for (i, v) in a.iter().enumerate() {
            if pl::str_eq(v, "  ") || (pl::ge(v, &valid_date_field(it, i, 1)) && pl::le(v, &valid_date_field(it, i, 2))) {
                continue;
            }
            bad.push(valid_date_field(it, i, 0));
        }
        if !bad.is_empty() {
            return ret(pl::concat(&pl::join("+", &bad), &pl::s(" out of range")));
        }
    // the EXIF specification allows blank fields or an entire blank value
    } else if !pl::str_eq(&val, "    :  :     :  :  ") && !pl::str_eq(&val, "                   ") {
        return ret(pl::s("Invalid date/time format"));
    }
    pl::ret_undef() // OK!
}

//------------------------------------------------------------------------------
// Validate EXIF-reformatted XMP date/time value
// Inputs: 0) date/time value
// Returns: error string
pub fn validate_xmp_date(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let val = arg(&a, 0);
    let pats = [
        r"^\d{4}$",
        r"^\d{4}:(\d{2})$",
        r"^\d{4}:(\d{2}):(\d{2})$",
        r"^\d{4}:(\d{2}):(\d{2}) (\d{2}):(\d{2})()(Z|[-+](\d{2}):(\d{2}))?$",
        r"^\d{4}:(\d{2}):(\d{2}) (\d{2}):(\d{2}):(\d{2})(Z|[-+](\d{2}):(\d{2}))?$",
        r"^\d{4}:(\d{2}):(\d{2}) (\d{2}):(\d{2}):(\d{2})\.?\d*(Z|[-+](\d{2}):(\d{2}))?$",
    ];
    let m = pats.iter().find_map(|p| re(p, "").m(&val));
    if let Some(c) = m {
        let a: Vec<Value> = [1, 2, 3, 4, 5, 7, 8].iter().map(|&i| cap(&c, i)).collect();
        let mut bad = Vec::new();
        for (i, v) in a.iter().enumerate() {
            if v.is_undef() {
                break;
            }
            if pl::str_eq(v, "") || (pl::ge(v, &valid_date_field(it, i, 1)) && pl::le(v, &valid_date_field(it, i, 2))) {
                continue;
            }
            bad.push(valid_date_field(it, i, 0));
        }
        if !bad.is_empty() {
            return ret(pl::concat(&pl::join("+", &bad), &pl::s(" out of range")));
        }
    } else {
        return ret(pl::s("Invalid date/time format"));
    }
    pl::ret_undef() // OK!
}

//------------------------------------------------------------------------------
// Validate EXIF tag
// Inputs: 0) ExifTool ref, 1) tag table ref, 2) tag ID, 3) tagInfo ref,
//         4) previous tag ID, 5) IFD name, 6) number of values, 7) value format string
// Returns: Nothing, but sets Warning tags if any problems are found
pub fn validate_exif(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, tag_table_ptr, tag, mut tag_info, last_tag, ifd, count, format_str) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4), arg(&a, 5), arg(&a, 6), arg(&a, 7));
    let warn = |it: &mut Interp, msg: Value, minor: Option<Value>| -> ER<()> {
        let mut args = vec![msg];
        if let Some(m) = minor {
            args.push(m);
        }
        pl::method(it, &et, "Warn", args)?;
        Ok(())
    };

    if pl::le(&tag, &last_tag) {
        warn(it, pl::s(&format!("Entries in {} are out of order", ifd.to_text())), None)?;
    }

    // (get tagInfo for unknown tags if Unknown option not used)
    let tt_tag = hget(&tag_table_ptr, &tag);
    if tag_info.is_undef() && tt_tag.truthy() && pl::ref_of(&tt_tag) == "HASH" {
        tag_info = tt_tag.clone();
    }
    let other_spec = my_hash(it, "otherSpec");
    let file_type = et.get("FileType");
    let os_ft = hget(&other_spec, &file_type);
    if !tag_info.is_undef() {
        let mut ti = if tag_info.truthy() { tag_info.clone() } else { tt_tag.clone() };
        if pl::ref_of(&ti) == "ARRAY" {
            ti = ti.at(-1);
        }
        let std_format = my_hash(it, "stdFormat");
        let mut std_fmt = hget(&std_format, &ifd);
        if !std_fmt.truthy() {
            std_fmt = std_format.get("IFD");
        }
        let exif_main = pl::pkg_hash(it, "Image::ExifTool::Exif::Main");
        let exif_spec = pl::pkg_hash(it, "Image::ExifTool::Validate::exifSpec");
        let is_main = tag_table_ptr.same_ref(&exif_main);
        let std_fmt_any_ifd = my_hash(it, "stdFormatAnyIFD");
        if !std_fmt.get("All").is_undef()
            || (is_main && (hget(&exif_spec, &tag).truthy() || hget(&std_fmt, &tag).truthy() || (pl::ge(&tag, &n(0xc612)) && pl::le(&tag, &n(0xcd48)) && hget(&std_fmt, &tag).is_undef())))
            || pl::str_eq(&tag_table_ptr.get("SHORT_NAME"), "GPS::Main")
        {
            let mut wgp = ti.get("WriteGroup");
            if !wgp.truthy() {
                wgp = tag_table_ptr.get("WRITE_GROUP");
            }
            if wgp.truthy()
                && wgp.to_bytes() != ifd.to_bytes()
                && !pl::str_eq(&wgp, "All")
                && !ti.get("OffsetPair").truthy()
                && (re(r"^(Sub|Profile)?IFD\d*$", "").is_match(&ifd) ^ re(r"^(Sub)?IFD\d*$", "").is_match(&wgp))
                && (ti.get("Writable").truthy() || ti.get("WriteGroup").truthy())
                && !re(r"^SRF\d+$", "").is_match(&ifd)
            {
                let m = pl::sprintf("Wrong IFD for 0x%.4x %s (should be %s not %s)", &[tag.clone(), ti.get("Name"), wgp.clone(), ifd.clone()]);
                warn(it, m, None)?;
            }
            let mut fmt = hget(&std_fmt, &tag);
            if !fmt.truthy() {
                fmt = ti.get("Writable");
            }
            if fmt.truthy() {
                let mut p = b"^".to_vec();
                p.extend_from_slice(&fmt.to_bytes());
                p.push(b'$');
                let int_format = pl::pkg_hash(it, "Image::ExifTool::Exif::intFormat");
                if !pl::re_dyn(&p, "")?.is_match(&format_str) && (!tag_info.truthy() || !tag_info.get("IsOffset").truthy() || hget(&int_format, &format_str).truthy()) {
                    let m = pl::sprintf("Non-standard format (%s) for %s 0x%.4x %s", &[format_str.clone(), ifd.clone(), tag.clone(), ti.get("Name")]);
                    warn(it, m, None)?;
                }
            }
        } else if hget(&std_fmt_any_ifd, &tag).truthy() {
            if ti.get("Writable").truthy() || ti.get("WriteGroup").truthy() {
                let mut wgp = ti.get("WriteGroup");
                if !wgp.truthy() {
                    wgp = tag_table_ptr.get("WRITE_GROUP");
                }
                if wgp.truthy() && wgp.to_bytes() != ifd.to_bytes() {
                    let m = pl::sprintf("Wrong IFD for 0x%.4x %s (should be %s not %s)", &[tag.clone(), ti.get("Name"), wgp.clone(), ifd.clone()]);
                    warn(it, m, None)?;
                }
            }
        } else if !os_ft.truthy() || (!hget(&os_ft, &tag).truthy() && !os_ft.get("All").truthy()) {
            if is_main || ti.get("Unknown").truthy() {
                if !hget(&other_spec, &ifd).truthy() {
                    let m = pl::sprintf("Non-standard %s tag 0x%.4x %s", &[ifd.clone(), tag.clone(), ti.get("Name")]);
                    warn(it, m, Some(n(1)))?;
                }
            }
        }
        // change expected count from read Format to Writable size
        let mut ti_count = ti.get("Count");
        if ti_count.truthy() {
            let format_number = pl::pkg_hash(it, "Image::ExifTool::Exif::formatNumber");
            if ti.get("Format").truthy() && ti.get("Writable").truthy() && hget(&format_number, &ti.get("Format")).truthy() && hget(&format_number, &ti.get("Writable")).truthy() {
                let format_size = pl::pkg_array(it, "Image::ExifTool::Exif::formatSize");
                let s1 = format_size.at(hget(&format_number, &ti.get("Format")).to_i64());
                let s2 = format_size.at(hget(&format_number, &ti.get("Writable")).to_i64());
                ti_count = pl::int(&pl::div(&pl::mul(&ti_count, &s1), &s2)?);
            }
            if pl::gt(&ti_count, &n(0)) && !pl::num_eq(&count, &ti_count) {
                let m = pl::sprintf("Non-standard count (%d) for %s 0x%.4x %s", &[count.clone(), ifd.clone(), tag.clone(), ti.get("Name")]);
                warn(it, m, None)?;
            }
        }
    } else if !os_ft.truthy() || (!hget(&os_ft, &tag).truthy() && !os_ft.get("All").truthy()) {
        if !hget(&other_spec, &ifd).truthy() {
            let m = pl::sprintf("Unknown %s tag 0x%.4x", &[ifd.clone(), tag.clone()]);
            warn(it, m, Some(n(1)))?;
        }
    }
    pl::ret_nothing(want)
}

//------------------------------------------------------------------------------
// Validate image data offsets/sizes
// Inputs: 0) ExifTool ref, 1) offset info hash ref (arrays of tagInfo/value pairs, keyed by tagID)
//         2) directory name, 3) optional flag for minor warning
pub fn validate_offset_info(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| validate_offset_info_body(it, a, want))
}

fn validate_offset_info_body(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, offset_info, dir_name, minor) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    let dn = dir_name.to_text();

    let file_size = et.get("VALUE").get("FileSize");
    if !file_size.truthy() {
        return pl::ret_nothing(want);
    }

    // (don't test RWZ files and some other file types)
    if et.get("DontValidateImageData").truthy() {
        return pl::ret_nothing(want);
    }
    // (Minolta A200 uses wrong byte order for these)
    if pl::str_eq(&et.get("TIFF_TYPE"), "MRW") && pl::str_eq(&dir_name, "IFD0") && re(r"^DiMAGE A200", "").is_match(&et.get("Model")) {
        return pl::ret_nothing(want);
    }
    // (don't test 3FR, RWL or RW2 files)
    if re(r"^(3FR|RWL|RW2)$", "").is_match(&et.get("TIFF_TYPE")) {
        return pl::ret_nothing(want);
    }

    pl::call(it, "Image::ExifTool::Exif::ValidateImageData", vec![et.clone(), offset_info.clone(), dir_name.clone()])?;

    let warn = |it: &mut Interp, msg: String, minor: &Value| -> ER<()> {
        pl::method(it, &et, "Warn", vec![pl::s(&msg), minor.clone()])?;
        Ok(())
    };
    // loop through all offsets
    while !offset_info.keys().is_empty() {
        let id1 = sorted_keys(&offset_info).into_iter().next().unwrap_or_default();
        let mut offsets = hget(&offset_info, &id1);
        offset_info.delete(&*id1.to_bytes());
        if pl::ref_of(&offsets) != "ARRAY" {
            continue;
        }
        let id2 = offsets.at(0).get("OffsetPair");
        if !(!id2.is_undef() && hget(&offset_info, &id2).truthy()) {
            if !(offsets.at(0).get("NotRealPair").truthy() || (!id2.is_undef() && pl::num_eq(&id2, &n(-1)))) {
                let corr = if offsets.at(0).get("IsOffset").truthy() { "size" } else { "offset" };
                if !minor.truthy() {
                    let m = format!("{dn}:{} is missing the corresponding {corr} tag", offsets.at(0).get("Name").to_text());
                    pl::method(it, &et, "Warn", vec![pl::s(&m)])?;
                }
            }
            continue;
        }
        let mut sizes = hget(&offset_info, &id2);
        offset_info.delete(&*id2.to_bytes());
        if sizes.at(0).get("IsOffset").truthy() {
            std::mem::swap(&mut sizes, &mut offsets);
        }
        let mut offsets_l = pl::split_ws(&offsets.at(1));
        let mut sizes_l = pl::split_ws(&sizes.at(1));
        let oname = offsets.at(0).get("Name").to_text();
        let sname = sizes.at(0).get("Name").to_text();
        if sizes_l.len() != offsets_l.len() {
            let m = pl::sprintf("Wrong number of values in %s 0x%.4x %s", &[dir_name.clone(), offsets.at(0).get("TagID"), offsets.at(0).get("Name")]);
            pl::method(it, &et, "Warn", vec![m, minor.clone()])?;
            continue;
        }
        while let Some(start) = offsets_l.pop() {
            let end = pl::add(&start, &sizes_l.pop().unwrap_or_default());
            if pl::num_eq(&start, &n(0)) {
                warn(it, format!("{dn}:{oname} is zero"), &minor)?;
            }
            if pl::num_eq(&start, &end) {
                warn(it, format!("{dn}:{sname} is zero"), &minor)?;
            }
            if !pl::gt(&end, &file_size) {
                continue;
            }
            if pl::ge(&start, &file_size) {
                if pl::num_eq(&start, &Value::Int(0xffffffff)) {
                    warn(it, format!("{dn}:{oname} is invalid (0xffffffff)"), &minor)?;
                } else {
                    warn(it, format!("{dn}:{oname} is past end of file"), &minor)?;
                }
            } else {
                warn(it, format!("{dn}:{oname}+{sname} runs past end of file"), &minor)?;
            }
            break;
        }
    }
    pl::ret_nothing(want)
}

//------------------------------------------------------------------------------
// Finish Validating tags
// Inputs: 0) ExifTool ref, 1) True to generate Validate tag
pub fn finish_validate(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| finish_validate_body(it, a, want))
}

fn finish_validate_body(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, mk_tag) = (arg(&a, 0), arg(&a, 1));

    let mut file_type = et.get("FILE_TYPE");
    if !file_type.truthy() {
        file_type = pl::s("");
    }
    if pl::str_eq(&file_type, "TIFF") {
        file_type = et.get("TIFF_TYPE");
    }
    let ft = file_type.to_text();

    let valid_value_all = my_hash(it, "validValue");
    if hget(&valid_value_all, &file_type).truthy() {
        let handler = set_warning_ref(it);
        pl::local_sig_warn(it, handler, |it| {
            let ver_check_all = my_hash(it, "verCheck");
            for grp in sorted_keys(&hget(&valid_value_all, &file_type)) {
                if !hget(&et.get("FOUND_DIR"), &grp).truthy() {
                    continue;
                }
                let val_h = Value::new_hash();
                let info = Value::new_hash();
                let mut minor = Value::Undef;
                let (mut ver_tag, mut ver) = (Value::Undef, Value::Undef);
                let ver_check = hget(&ver_check_all, &grp);
                if ver_check.truthy() {
                    ver_tag = ver_check.keys().first().map(|k| Value::bytes(&k[..])).unwrap_or_default();
                    ver = hget(&et.get("VALUE"), &ver_tag);
                    pl::tr(&mut ver, "0-9", "", "dc"); // (remove non-digits)
                    if !re(r"^\d{4}$", "").is_match(&ver) {
                        ver = Value::Undef; // (already warned if invalid version)
                    }
                }
                // get all tags in this group
                for key in sorted_keys(&et.get("VALUE")) {
                    let g1 = pl::method(it, &et, "GetGroup", vec![key.clone(), n(1)])?;
                    if g1.to_bytes() != grp.to_bytes() {
                        continue;
                    }
                    if hget(&et.get("TAG_EXTRA"), &key).get("G3").truthy() {
                        continue; // ignore sub-documents
                    }
                    // fill in %val lookup with values based on tag ID
                    let ti = hget(&et.get("TAG_INFO"), &key);
                    let tag = ti.get("TagID");
                    val_h.set(&*tag.to_bytes(), hget(&et.get("VALUE"), &key));
                    // save TagInfo ref for later
                    info.set(&*tag.to_bytes(), ti.clone());
                    if ver.is_undef() {
                        continue;
                    }
                    let chk = hget(&ver_check, &ver_tag);
                    let ct = hget(&chk, &tag);
                    if ct.is_undef() || pl::num_eq(&ct, &n(1)) || pl::ge(&ver, &ct) {
                        continue;
                    }
                    let vstr;
                    if pl::str_eq(&ver_tag, "GPSVersionID") {
                        let mut v = ct.clone();
                        re(r"^(\d)(\d)(\d)", "").subst(&mut v, false, |c| format!("{}.{}.{}.", cap(c, 1).to_text(), cap(c, 2).to_text(), cap(c, 3).to_text()).into_bytes());
                        vstr = v;
                    } else {
                        vstr = pl::sprintf("%.4d", &[ct.clone()]);
                    }
                    let m = pl::sprintf("%s tag 0x%.4x %s requires %s %s or higher", &[grp.clone(), tag.clone(), ti.get("Name"), ver_tag.clone(), vstr]);
                    pl::method(it, &et, "Warn", vec![m])?;
                }
                // make quick lookup for values based on tag ID
                let valid_value = hget(&hget(&valid_value_all, &file_type), &grp);
                for tag in sorted_keys_num(&valid_value) {
                    let val = hget(&val_h, &tag);
                    let (mut pre, mut post) = (Value::Undef, Value::Undef);
                    let code = hget(&valid_value, &tag);
                    if !code.is_undef() {
                        //### eval ($val, %val)
                        let scope = runtime::file_scope(it, MODULE);
                        let (result, _) = pl::eval_site_in(it, &code, MODULE, Some(scope), &[("val", val.clone()), ("%val", val_h.clone())], Want::Scalar);
                        if result.is_undef() {
                            pre = pl::s("Internal error validating");
                        } else if pl::str_eq(&result, "") {
                            pre = if !val.is_undef() { pl::s("Invalid value for") } else { pl::s(&format!("Missing required {ft}")) };
                        } else {
                            if pl::str_eq(&result, "1") {
                                continue;
                            }
                            pre = result;
                        }
                    } else {
                        if val.is_undef() {
                            continue;
                        }
                        post = pl::s(&format!("is not allowed in {ft}"));
                        minor = n(1);
                    }
                    let name;
                    if hget(&info, &tag).truthy() {
                        name = hget(&info, &tag).get("Name");
                    } else {
                        let table = format!("Image::ExifTool::{}::Main", if pl::str_eq(&grp, "GPS") { "GPS" } else { "Exif" });
                        let tt = pl::call(it, "Image::ExifTool::GetTagTable", vec![pl::s(&table)])?;
                        let mut tag_info = hget(&tt, &tag);
                        if pl::ref_of(&tag_info) == "ARRAY" {
                            tag_info = tag_info.at(0);
                        }
                        name = if tag_info.truthy() { tag_info.get("Name") } else { pl::s("<unknown>") };
                    }
                    let wf = et.get("WrongFormat");
                    if wf.truthy() && wf.get(format!("{}:{}", grp.to_text(), name.to_text())).truthy() {
                        continue;
                    }
                    if pre.truthy() {
                        pre = pl::concat(&pre, &pl::s(" "));
                    } else {
                        pre = pl::s("");
                    }
                    if post.truthy() {
                        post = pl::concat(&pl::s(" "), &post);
                    } else {
                        post = pl::s("");
                    }
                    let m = pl::sprintf("%s%s tag 0x%.4x %s%s", &[pre, grp.clone(), tag.clone(), name, post]);
                    pl::method(it, &et, "Warn", vec![m, minor.clone()])?;
                }
            }
            Ok(())
        })?;
    }
    // validate file extension
    let filename = et.get("FILENAME");
    if !pl::str_eq(&filename, "") {
        let file_ext = match re(r"^.*\.([^.]+)$", "s").m(&filename) {
            Some(c) => pl::uc(&cap(&c, 1)),
            None => pl::s(""),
        };
        let ext_file_type = pl::call(it, "Image::ExifTool::GetFileType", vec![file_ext.clone()])?;
        if ext_file_type.truthy() && ext_file_type.to_bytes() != file_type.to_bytes() {
            let norm_ext = et.get("VALUE").get("FileTypeExtension");
            if norm_ext.truthy() && norm_ext.to_bytes() != file_ext.to_bytes() {
                let lkup = hget(&pl::pkg_hash(it, "Image::ExifTool::fileTypeLookup"), &file_ext);
                if lkup.is_ref() || lkup.to_bytes() != norm_ext.to_bytes() {
                    let m = format!("File has wrong extension (should be {}, not {})", norm_ext.to_text(), file_ext.to_text());
                    pl::method(it, &et, "Warn", vec![pl::s(&m)])?;
                }
            }
        }
    }
    // issue warning if FastScan option used
    if pl::method(it, &et, "Options", vec![pl::s("FastScan")])?.truthy() {
        pl::method(it, &et, "Warn", vec![pl::s("Validation incomplete because FastScan option used")])?;
    }

    // generate Validate tag if necessary
    if mk_tag.truthy() {
        let value = et.get("VALUE");
        let dupl = et.get("DUPL_TAG");
        let or0 = |v: Value| if v.truthy() { v } else { n(0) };
        let mut num = vec![
            if value.get("Error").truthy() { pl::add(&or0(dupl.get("Error")), &n(1)) } else { n(0) },
            if value.get("Warning").truthy() { pl::add(&or0(dupl.get("Warning")), &n(1)) } else { n(0) },
            n(0),
        ];
        let mut key = pl::s("Warning");
        loop {
            let v = hget(&et.get("VALUE"), &key);
            if v.truthy() && re(r"^\[minor\]", "i").is_match(&v) {
                num[2] = pl::add(&num[2], &n(1));
            }
            key = pl::method(it, &et, "NextTagKey", vec![key.clone()])?;
            if !key.truthy() {
                break;
            }
        }
        pl::method(it, &et, "FoundTag", vec![pl::s("Validate"), pl::join(" ", &num)])?;
    }
    pl::ret_nothing(want)
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Validate::ValidateRaw", validate_raw);
    it.define_native("Image::ExifTool::Validate::ValidateExifDate", validate_exif_date);
    it.define_native("Image::ExifTool::Validate::ValidateXMPDate", validate_xmp_date);
    it.define_native("Image::ExifTool::Validate::ValidateExif", validate_exif);
    it.define_native("Image::ExifTool::Validate::ValidateOffsetInfo", validate_offset_info);
    it.define_native("Image::ExifTool::Validate::FinishValidate", finish_validate);
}
