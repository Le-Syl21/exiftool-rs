//! Image::ExifTool::Microsoft (Image/ExifTool/Microsoft.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::perl::Value;
use crate::pl::{self, arg, n, re, ret};

/// `Image::ExifTool::NAME(ARGS)` (the functions Microsoft.pm imports).
fn et(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// check new value for Xtra tag
/// Inputs: 0) ExifTool object ref, 1) tagInfo hash ref, 2) raw value ref
/// Returns: error string, or undef on success
pub fn check_xtra(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et_, tag_info, val_pt) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let format = tag_info.get("Writable");
    if !format.truthy() {
        return ret(pl::s("Unknown format"));
    }
    if re(r"^int", "").is_match(&format) {
        if !et(it, "IsInt", vec![val_pt.deref()])?.truthy() {
            return ret(pl::s("Not an integer"));
        }
    } else if !pl::str_eq(&format, "Unicode") {
        let vals = Value::new_array(vec![val_pt.deref()]);
        if !write_xtra_value_impl(it, &et_, &tag_info, &vals)?.truthy() {
            return ret(pl::s("Invalid format"));
        }
    }
    pl::ret_undef()
}

/// Decode value(s) in Microsoft Xtra tag
/// Inputs: 0) ExifTool object ref, 1) value data
/// Returns: Scalar context: decoded value, List context: 0) decoded value, 1) format string
pub fn read_xtra_value(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et_, data) = (arg(&a, 0), arg(&a, 1));
    let mut format = Value::Undef;
    let vals = Value::new_array(Vec::new());

    if pl::lt(&pl::length(&data), &n(10)) {
        return pl::ret_undef();
    }

    let data_ref = pl::sref(data.clone());
    // (version flags according to the reference, but looks more like a count - PH)
    let count = et(it, "Get32u", vec![data_ref.clone(), n(0)])?;
    // point to start of first value (after 4-byte count, 4-byte length and 2-byte type)
    let mut val_pos = n(10);
    let mut i = n(0);
    loop {
        // (stored value includes size of $valLen and $valType, so subtract 6)
        let val_len = pl::sub(&et(it, "Get32u", vec![data_ref.clone(), pl::sub(&val_pos, &n(6))])?, &n(6));
        if pl::gt(&pl::add(&val_pos, &val_len), &pl::length(&data)) {
            break;
        }
        let val_type = et(it, "Get16u", vec![data_ref.clone(), pl::sub(&val_pos, &n(2))])?;
        let mut val = pl::substr(&data, val_pos.to_i64(), Some(val_len.to_i64()));
        // Note: all dumb Microsoft values are little-endian inside a big-endian-format file
        et(it, "SetByteOrder", vec![pl::s("II")])?;
        if pl::num_eq(&val_type, &n(8)) {
            format = pl::s("Unicode");
            val = pl::method(it, &et_, "Decode", vec![val, pl::s("UTF16")])?;
        } else if pl::num_eq(&val_type, &n(19)) && pl::num_eq(&val_len, &n(8)) {
            format = pl::s("int64u");
            val = et(it, "Get64u", vec![pl::sref(val), n(0)])?;
        } else if pl::num_eq(&val_type, &n(21)) && pl::num_eq(&val_len, &n(8)) {
            format = pl::s("date");
            val = et(it, "Get64u", vec![pl::sref(val), n(0)])?;
            // convert time from 100 ns intervals since Jan 1, 1601
            if val.truthy() {
                val = pl::sub(&pl::mul(&val, &Value::Num(1e-7)), &n(11644473600));
            }
            // (the Nikon S100 uses UTC timezone, same as ASF - PH)
            val = et(it, "ConvertUnixTime", vec![val, n(1)])?;
        } else if pl::num_eq(&val_type, &n(72)) && pl::num_eq(&val_len, &n(16)) {
            format = pl::s("GUID");
            let words = pl::unpack("VvvNN", &val)?;
            let packed = pl::pack("NnnNN", &words)?;
            val = pl::uc(&pl::unpack1("H*", &packed)?);
            re(r"(.{8})(.{4})(.{4})(.{4})", "").subst(&mut val, false, |c| {
                let mut o = Vec::new();
                for k in 1..=4 {
                    o.extend_from_slice(&pl::cap(c, k).to_bytes());
                    o.push(b'-');
                }
                o
            });
        } else if pl::num_eq(&val_type, &n(65)) && pl::gt(&val_len, &n(4)) {
            //PH (empirical)
            format = pl::s("variant");
            crate::runtime::require(it, "Image::ExifTool::FlashPix")?;
            // (necessary because ReadFPXValue updates this)
            let vt_variant = pl::call(it, "Image::ExifTool::FlashPix::VT_VARIANT", vec![])?;
            // read entry as a VT_VARIANT (use FlashPix module for this)
            let mut args = [et_.clone(), pl::sref(val), n(0), vt_variant, val_len.clone(), n(1)];
            val = pl::call_mut(it, "Image::ExifTool::FlashPix::ReadFPXValue", &mut args, Want::Scalar)?.pop().unwrap_or_default();
        } else {
            format = pl::concat(&pl::concat(&pl::s("Unknown("), &val_type), &pl::s(")"));
        }
        et(it, "SetByteOrder", vec![pl::s("MM")])?; // back to native QuickTime byte ordering
        vals.push(val);
        i = pl::add(&i, &n(1));
        if pl::ge(&i, &count) {
            break;
        }
        val_pos = pl::add(&val_pos, &pl::add(&val_len, &n(6))); // step to next value
        if pl::gt(&val_pos, &pl::length(&data)) {
            break;
        }
    }
    if want == Want::List {
        Ok(vec![vals, format])
    } else {
        ret(vals)
    }
}

fn write_xtra_value_impl(it: &mut Interp, et_: &Value, tag_info: &Value, vals: &Value) -> ER<Value> {
    let format = tag_info.get("Writable");
    let mut buff = pl::s("");
    let mut count = n(0);
    for val in vals.items() {
        et(it, "SetByteOrder", vec![pl::s("II")])?;
        let (mut typ, mut dat) = (Value::Undef, Value::Undef);
        if pl::str_eq(&format, "Unicode") {
            let enc = pl::method(it, et_, "Encode", vec![val.clone(), pl::s("UTF16"), pl::s("II")])?;
            dat = pl::concat(&enc, &pl::s("\0\0")); // (must be null terminated)
            typ = n(8);
        } else if pl::str_eq(&format, "int64u") {
            if et(it, "IsInt", vec![val.clone()])?.truthy() {
                dat = et(it, "Set64u", vec![val.clone()])?;
                typ = n(19);
            }
        } else if pl::str_eq(&format, "date") {
            dat = et(it, "GetUnixTime", vec![val.clone(), n(1)])?; // (convert to UTC, NC)
            if dat.truthy() {
                // 100ns intervals since Jan 1, 1601
                dat = et(it, "Set64u", vec![pl::mul(&pl::add(&dat, &n(11644473600)), &Value::Num(1e7))])?;
                typ = n(21);
            }
        } else if pl::str_eq(&format, "vt_filetime") {
            // 'date' value inside a VT_VARIANT
            dat = et(it, "GetUnixTime", vec![val.clone()])?; // (leave as local time, NC)
            if dat.truthy() {
                // 100ns intervals since Jan 1, 1601
                let a = et(it, "Set32u", vec![n(64)])?;
                let b = et(it, "Set64u", vec![pl::mul(&pl::add(&dat, &n(11644473600)), &Value::Num(1e7))])?;
                dat = pl::concat(&a, &b);
                typ = n(65);
            }
        } else if pl::str_eq(&format, "GUID") {
            dat = val.clone();
            pl::tr(&mut dat, "-", "", "d");
            if pl::num_eq(&pl::length(&dat), &n(32)) {
                let words = pl::unpack("NnnNN", &pl::pack("H*", &[dat])?)?;
                dat = pl::pack("VvvNN", &words)?;
                typ = n(72);
            }
        } else {
            let msg = pl::concat(&pl::s("Error converting value for Microsoft:"), &tag_info.get("Name"));
            pl::method(it, et_, "Warn", vec![msg])?;
        }
        et(it, "SetByteOrder", vec![pl::s("MM")])?;
        if !typ.is_undef() {
            count = pl::add(&count, &n(1));
            let a = et(it, "Set32u", vec![pl::add(&pl::length(&dat), &n(6))])?;
            let b = et(it, "Set16u", vec![typ])?;
            buff = pl::concat(&buff, &pl::concat(&pl::concat(&a, &b), &dat));
        }
    }
    Ok(if count.truthy() { pl::concat(&et(it, "Set32u", vec![count])?, &buff) } else { pl::s("") })
}

/// Write a Microsoft Xtra value
/// Inputs: 0) ExifTool object ref, 1) tagInfo ref, 2) reference to list of values
/// Returns: new value binary data (or empty string)
pub fn write_xtra_value(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(write_xtra_value_impl(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2))?)
}

fn add_new_values_impl(it: &mut Interp, et_: &Value, vals: &Value, nv_hash: &Value) -> ER<Value> {
    let new_vals = pl::method_list(it, et_, "GetNewValue", vec![nv_hash.clone()])?;
    if new_vals.is_empty() {
        return Ok(Value::Undef);
    }
    if pl::gt(&et_.path(&["OPTIONS", "Verbose"]), &n(1)) {
        for v in &new_vals {
            let msg = pl::concat(&pl::concat(&pl::concat(&pl::s("  + Microsoft:"), &nv_hash.path(&["TagInfo", "Name"])), &pl::concat(&pl::s(" = "), v)), &pl::s("\n"));
            pl::method(it, et_, "VPrint", vec![n(1), msg])?;
        }
    }
    for v in new_vals {
        vals.push(v);
    }
    Ok(n(1))
}

/// Add new values to list
/// Inputs: 0) ExifTool ref, 1) new value list ref, 2) nvHash ref
/// Returns: true if something was added
pub fn add_new_values(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(add_new_values_impl(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2))?)
}

/// `Set32u(8+length($tag)+length($buff)) . Set32u(length($tag)) . $tag . $buff`
fn xtra_entry(it: &mut Interp, tag: &Value, buff: &Value) -> ER<Value> {
    let a = et(it, "Set32u", vec![pl::add(&pl::add(&n(8), &pl::length(tag)), &pl::length(buff))])?;
    let b = et(it, "Set32u", vec![pl::length(tag)])?;
    Ok(pl::concat(&pl::concat(&pl::concat(&a, &b), tag), buff))
}

/// Write tags to a Microsoft Xtra MP4 atom
/// Inputs: 0) ExifTool object ref, 1) source dirInfo ref, 2) tag table ref
/// Returns: Microsoft Xtra data block (may be empty if no Xtra data) or undef on error
pub fn write_xtra(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et_, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if !et_.truthy() {
        return ret(n(1)); // allow dummy access
    }

    let del_group = et_.get("DEL_GROUP").truthy() && et_.path(&["DEL_GROUP", "Microsoft"]).truthy();
    let new_tags = pl::method(it, &et_, "GetNewTagInfoHash", vec![tag_table_ptr.clone()])?;

    if !(del_group || !new_tags.is_empty()) {
        return pl::ret_undef(); // don't rewrite if nothing to do
    }

    let mut data_pt = dir_info.get("DataPt");
    let mut data_len = pl::length(&data_pt.deref());
    let mut new_data = pl::s("");
    let mut pos = n(0);
    let (mut err, mut changed) = (false, false);
    let done = Value::new_hash();
    let mut tag: Value;

    if del_group {
        if data_len.truthy() {
            changed = true;
        }
        let empty = pl::s("");
        data_pt = empty;
        data_len = n(0);
    }
    loop {
        if pl::gt(&pl::add(&pos, &n(4)), &data_len) {
            break;
        }
        let size = et(it, "Get32u", vec![data_pt.clone(), pos.clone()])?; // (includes $size word)
        if pl::lt(&size, &n(8)) || pl::gt(&pl::add(&pos, &size), &data_len) {
            err = true;
            break;
        }
        let tag_len = et(it, "Get32u", vec![data_pt.clone(), pl::add(&pos, &n(4))])?;
        if pl::gt(&pl::add(&tag_len, &n(18)), &size) {
            err = true;
            break;
        }
        tag = pl::substr(&data_pt.deref(), pl::add(&pos, &n(8)).to_i64(), Some(tag_len.to_i64()));
        let new_vals = Value::new_array(Vec::new());
        #[allow(clippy::never_loop)]
        while new_tags.get(tag.to_bytes()).truthy() {
            let nv_hash = pl::method(it, &et_, "GetNewValueHash", vec![new_tags.get(tag.to_bytes())])?;
            if nv_hash.get("CreateOnly").truthy() {
                new_tags.delete(tag.to_bytes());
                break; // don't edit this tag
            }
            let val_pos = pl::add(&pl::add(&pos, &n(8)), &tag_len);
            let val_len = pl::sub(&pl::sub(&size, &n(8)), &tag_len);
            let raw = pl::substr(&data_pt.deref(), val_pos.to_i64(), Some(val_len.to_i64()));
            let val = read_xtra_value(it, vec![et_.clone(), raw], Want::Scalar)?.pop().unwrap_or_default();
            for v in val.items() {
                let overwrite = pl::method(it, &et_, "IsOverwriting", vec![nv_hash.clone(), v.clone()])?;
                if !overwrite.truthy() {
                    new_vals.push(v);
                    continue;
                }
                let msg = pl::concat(&pl::concat(&pl::concat(&pl::s("  - Microsoft:"), &new_tags.get(tag.to_bytes()).get("Name")), &pl::concat(&pl::s(" = "), &v)), &pl::s("\n"));
                pl::method(it, &et_, "VPrint", vec![n(1), msg])?;
                if done.get(tag.to_bytes()).truthy() {
                    continue;
                }
                done.set(tag.to_bytes(), n(1));
                add_new_values_impl(it, &et_, &new_vals, &nv_hash)?;
            }
            // add to the end of the list if this was a List-type tag and we didn't delete anything
            if !done.get(tag.to_bytes()).truthy() && new_tags.get(tag.to_bytes()).get("List").truthy() {
                if !add_new_values_impl(it, &et_, &new_vals, &nv_hash)?.truthy() {
                    break;
                }
                done.set(tag.to_bytes(), n(1));
            }
            break; // (it was a cheap goto)
        }
        if done.get(tag.to_bytes()).truthy() {
            changed = true;
            // write changed values
            let buff = write_xtra_value_impl(it, &et_, &new_tags.get(tag.to_bytes()), &new_vals)?;
            if pl::length(&buff).to_i64() != 0 {
                let e = xtra_entry(it, &tag, &buff)?;
                new_data = pl::concat(&new_data, &e);
            }
        } else {
            // nothing changed; just copy over
            new_data = pl::concat(&new_data, &pl::substr(&data_pt.deref(), pos.to_i64(), Some(size.to_i64())));
        }
        pos = pl::add(&pos, &size); // step to next entry
    }
    if err {
        pl::method(it, &et_, "Warn", vec![pl::s("Microsoft Xtra format error")])?;
        return pl::ret_undef();
    }
    // add any new tags
    let mut keys = new_tags.keys();
    keys.sort();
    for k in keys {
        tag = Value::bytes(&k);
        if done.get(&k).truthy() {
            continue;
        }
        let nv_hash = pl::method(it, &et_, "GetNewValueHash", vec![new_tags.get(&k)])?;
        if !(nv_hash.get("IsCreating").truthy() && !nv_hash.get("EditOnly").truthy()) {
            continue;
        }
        let new_vals = Value::new_array(Vec::new());
        if !add_new_values_impl(it, &et_, &new_vals, &nv_hash)?.truthy() {
            continue;
        }
        let buff = write_xtra_value_impl(it, &et_, &new_tags.get(&k), &new_vals)?;
        if pl::length(&buff).to_i64() != 0 {
            let e = xtra_entry(it, &tag, &buff)?;
            new_data = pl::concat(&new_data, &e);
            changed = true;
        }
    }
    if changed {
        et_.set("CHANGED", pl::add(&et_.get("CHANGED"), &n(1)));
    } else {
        new_data = Value::Undef;
    }
    ret(new_data)
}

/// Extract information from Xtra MP4 atom
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
/// Reference: http://code.google.com/p/mp4v2/ [since removed from trunk]
pub fn process_xtra(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et_, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let b = dir_info.get("Base");
    let data_pos = if b.truthy() { b } else { n(0) };
    let data_len = dir_info.get("DataLen");
    let mut pos = n(0);
    pl::method(it, &et_, "VerboseDir", vec![pl::s("Xtra"), n(0), data_len.clone()])?;
    loop {
        if pl::gt(&pl::add(&pos, &n(4)), &data_len) {
            break;
        }
        let size = et(it, "Get32u", vec![data_pt.clone(), pos.clone()])?; // (includes $size word)
        if pl::lt(&size, &n(8)) || pl::gt(&pl::add(&pos, &size), &data_len) {
            break;
        }
        let tag_len = et(it, "Get32u", vec![data_pt.clone(), pl::add(&pos, &n(4))])?;
        if pl::gt(&pl::add(&tag_len, &n(18)), &size) {
            break;
        }
        let val_len = pl::sub(&pl::sub(&size, &n(8)), &tag_len);
        if pl::gt(&tag_len, &n(0)) && pl::gt(&val_len, &n(0)) {
            let tag = pl::substr(&data_pt.deref(), pl::add(&pos, &n(8)).to_i64(), Some(tag_len.to_i64()));
            let val_pos = pl::add(&pl::add(&pos, &n(8)), &tag_len);
            let raw = pl::substr(&data_pt.deref(), val_pos.to_i64(), Some(val_len.to_i64()));
            let r = read_xtra_value(it, vec![et_.clone(), raw], Want::List)?;
            let (mut val, format) = (arg(&r, 0), arg(&r, 1));
            if val.is_undef() {
                break;
            }
            if val.len() == 1 {
                val = val.at(0);
            }
            let mut tag_info = pl::method(it, &et_, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
            if !tag_info.truthy() {
                // generate tag information for unrecognized tags
                let mut name = tag.clone();
                re(r"^WM/", "").subst(&mut name, false, |_| Vec::new());
                // $name =~ tr/-_A-Za-z0-9//dc;
                if re(r"^[-\w]+$", "").is_match(&name) {
                    tag_info = Value::new_hash();
                    tag_info.set("Name", pl::ucfirst(&name));
                    et(it, "AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), tag_info.clone()])?;
                    let msg = pl::concat(&pl::concat(&pl::s("[adding Microsoft:"), &tag), &pl::s("]\n"));
                    pl::method(it, &et_, "VPrint", vec![n(0), et_.get("INDENT"), msg])?;
                }
            }
            let count = if val.is_ref() { n(val.len() as i64) } else { n(1) };
            pl::method(
                it,
                &et_,
                "HandleTag",
                vec![
                    tag_table_ptr.clone(),
                    tag,
                    val,
                    pl::s("TagInfo"),
                    tag_info,
                    pl::s("DataPt"),
                    data_pt.clone(),
                    pl::s("DataPos"),
                    data_pos.clone(),
                    pl::s("Start"),
                    val_pos,
                    pl::s("Size"),
                    val_len,
                    pl::s("Format"),
                    format,
                    pl::s("Extra"),
                    pl::concat(&pl::s(" count="), &count),
                ],
            )?;
        }
        pos = pl::add(&pos, &size); // step to next entry
    }
    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Microsoft::CheckXtra", check_xtra);
    it.define_native("Image::ExifTool::Microsoft::ReadXtraValue", read_xtra_value);
    it.define_native("Image::ExifTool::Microsoft::WriteXtraValue", write_xtra_value);
    it.define_native("Image::ExifTool::Microsoft::AddNewValues", add_new_values);
    it.define_native("Image::ExifTool::Microsoft::WriteXtra", write_xtra);
    it.define_native("Image::ExifTool::Microsoft::ProcessXtra", process_xtra);
}
