//! Image::ExifTool::CanonCustom (Image/ExifTool/CanonCustom.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::perl::{ops, Value};
use crate::modules::random_access::substr;
use crate::pl::{self, arg, cap, n, re, ret};

/// `Image::ExifTool::NAME(ARGS)` (the functions CanonCustom.pm imports).
fn et(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// Conversion routines
/// Inputs: 0) value to convert
pub fn convert_pfn(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let val = arg(&a, 0);
    ret(if val.truthy() {
        if pl::num_eq(&val, &n(1)) {
            pl::s("On")
        } else {
            pl::concat(&pl::concat(&pl::s("On ("), &val), &pl::s(")"))
        }
    } else {
        pl::s("Off")
    })
}

pub fn convert_pfn_inv(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let val = arg(&a, 0);
    if let Some(m) = re(r"(\d+)", "").m(&val) {
        return ret(cap(&m, 1));
    }
    if re(r"on", "i").is_match(&val) {
        return ret(n(1));
    }
    if re(r"off", "i").is_match(&val) {
        return ret(n(0));
    }
    pl::ret_undef()
}

/// Read/Write Canon custom 2 directory (new for 1D Mark III)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_canon_custom2(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et_, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let offset = dir_info.get("DirStart");
    let size = dir_info.get("DirLen");
    let write = dir_info.get("Write");
    let verbose = pl::method(it, &et_, "Options", vec![pl::s("Verbose")])?;
    let mut new_tags = Value::Undef;

    if pl::lt(&size, &n(2)) {
        return ret(n(0));
    }
    // first entry in array must be the size
    let len = et(it, "Get16u", vec![data_pt.clone(), offset.clone()])?;
    if !(pl::num_eq(&len, &size) && pl::ge(&len, &n(8))) {
        pl::method(it, &et_, "Warn", vec![pl::s("Invalid CanonCustom2 data")])?;
        return ret(n(0));
    }
    // get group count
    let count = et(it, "Get32u", vec![data_pt.clone(), pl::add(&offset, &n(4))])?;
    if write.truthy() {
        new_tags = pl::method(it, &et_, "GetNewTagInfoHash", vec![tag_table_ptr.clone()])?;
        pl::method(it, &et_, "VPrint", vec![n(0), pl::s("  Rewriting CanonCustom2\n")])?;
    } else if verbose.truthy() {
        pl::method(it, &et_, "VerboseDir", vec![pl::s("CanonCustom2"), count, len.clone()])?;
    }
    let mut pos = pl::add(&offset, &n(8));
    let end = pl::add(&offset, &size);
    // loop through group records
    while pl::lt(&pos, &end) {
        if pl::gt(&pl::add(&pos, &n(12)), &end) {
            break;
        }
        let rec_num = et(it, "Get32u", vec![data_pt.clone(), pos.clone()])?;
        let rec_len = et(it, "Get32u", vec![data_pt.clone(), pl::add(&pos, &n(4))])?;
        let rec_count = et(it, "Get32u", vec![data_pt.clone(), pl::add(&pos, &n(8))])?;
        if pl::lt(&rec_len, &n(8)) {
            break; // must be at least 8 bytes for recNum and recLen
        }
        pos = pl::add(&pos, &n(12));
        let mut rec_pos = pos.clone();
        let rec_end = pl::sub(&pl::add(&pos, &rec_len), &n(8));
        if pl::gt(&rec_end, &end) {
            pl::method(it, &et_, "Warn", vec![pl::concat(&pl::s("Corrupted CanonCustom2 group "), &rec_num)])?;
            return ret(n(0));
        }
        if verbose.truthy() && !write.truthy() {
            pl::method(it, &et_, "VerboseDir", vec![pl::concat(&pl::s("CanonCustom2 group "), &rec_num), rec_count.clone(), rec_len.clone()])?;
        }
        let mut i = n(0);
        let mut num: Value;
        'rec: while pl::lt(&pl::add(&rec_pos, &n(8)), &rec_end) {
            // (the body; `next` continues with the increment below)
            'body: {
                let tag = et(it, "Get32u", vec![data_pt.clone(), rec_pos.clone()])?;
                num = et(it, "Get32u", vec![data_pt.clone(), pl::add(&rec_pos, &n(4))])?;
                let next_rec = pl::add(&pl::add(&rec_pos, &n(8)), &pl::mul(&num, &n(4)));
                if pl::gt(&next_rec, &rec_end) {
                    break 'rec;
                }
                // patch for EOS-1DXmkIII firmware 1.0.0 bug:
                if pl::num_eq(&tag, &n(0x70c)) && pl::num_eq(&num, &n(0x66)) && pl::lt(&pl::add(&next_rec, &n(8)), &rec_end) {
                    let tmp = et(it, "Get32u", vec![data_pt.clone(), pl::add(&next_rec, &n(4))])?;
                    if pl::num_eq(&tmp, &n(0x70f)) {
                        pl::method(it, &et_, "Warn", vec![pl::s("Fixed incorrect CanonCustom tag 0x70c size"), n(1)])?;
                        num = pl::add(&num, &n(1)); // (count should be one greater)
                    }
                }
                rec_pos = pl::add(&rec_pos, &n(8));
                let val = et(it, "ReadValue", vec![data_pt.clone(), rec_pos.clone(), pl::s("int32s"), num.clone(), pl::mul(&num, &n(4))])?;
                if write.truthy() {
                    // write new value
                    if !new_tags.get(tag.to_bytes()).truthy() {
                        break 'body;
                    }
                    let val_ref = pl::sref(val);
                    let tag_info = pl::method(it, &et_, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone(), val_ref.clone(), Value::Undef, num.clone()])?;
                    let val = val_ref.deref();
                    if !tag_info.truthy() {
                        break 'body;
                    }
                    let nv_hash = pl::method(it, &et_, "GetNewValueHash", vec![tag_info.clone()])?;
                    if !nv_hash.truthy() {
                        break 'body;
                    }
                    if !pl::method(it, &et_, "IsOverwriting", vec![nv_hash.clone(), val.clone()])?.truthy() {
                        break 'body;
                    }
                    let new_val = pl::method(it, &et_, "GetNewValue", vec![nv_hash])?;
                    if new_val.is_undef() {
                        break 'body; // can't delete from a custom table
                    }
                    et(it, "WriteValue", vec![new_val.clone(), pl::s("int32s"), num.clone(), data_pt.clone(), rec_pos.clone()])?;
                    let name = tag_info.get("Name");
                    pl::method(it, &et_, "VerboseValue", vec![pl::concat(&pl::s("- CanonCustom:"), &name), val])?;
                    pl::method(it, &et_, "VerboseValue", vec![pl::concat(&pl::s("+ CanonCustom:"), &name), new_val])?;
                    et_.set("CHANGED", pl::add(&et_.get("CHANGED"), &n(1)));
                } else {
                    // save extracted tag
                    let old_info = tag_table_ptr.get(tag.to_bytes());
                    pl::method(
                        it,
                        &et_,
                        "HandleTag",
                        vec![tag_table_ptr.clone(), tag.clone(), val, pl::s("Index"), i.clone(), pl::s("Format"), pl::s("int32u"), pl::s("Count"), num.clone(), pl::s("Size"), pl::mul(&num, &n(4))],
                    )?;
                    let tag_info = tag_table_ptr.get(tag.to_bytes());
                    // generate properly formatted description if we just added the tag
                    if tag_info.truthy() && !old_info.truthy() {
                        let mut desc = tag_info.get("Name");
                        pl::tr(&mut desc, "_", " ", "");
                        re(r"CanonCustom Functions", "").subst(&mut desc, false, |_| b"Canon Custom Functions ".to_vec());
                        tag_info.set("Description", desc);
                    }
                }
            }
            i = pl::add(&i, &n(1));
            rec_pos = pl::add(&rec_pos, &pl::mul(&n(4), &num));
        }
        if !pl::num_eq(&i, &rec_count) {
            let msg = pl::concat(&pl::concat(&pl::concat(&pl::s("Missing "), &pl::sub(&rec_count, &i)), &pl::s(" entries in CanonCustom2 group ")), &pl::concat(&rec_num, &pl::s(" directory")));
            pl::method(it, &et_, "Warn", vec![msg, n(1)])?;
        }
        pos = rec_end;
    }
    if !pl::num_eq(&pos, &end) {
        // Note: a firmware bug in the EOS M5 and M6 stores an incorrect
        // size for the 2nd CanonCustom2 record, so this message is expected
        // for these models...
        pl::method(it, &et_, "Warn", vec![pl::s("Possibly corrupted CanonCustom2 data")])?;
        return ret(n(0));
    }
    ret(n(1))
}

/// Write Canon custom 2 data
/// Inputs: 0) ExifTool object reference, 1) dirInfo hash ref, 2) tag table ref
/// Returns: New custom data block or undefined on error
pub fn write_canon_custom2(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et_, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if !et_.truthy() {
        return ret(n(1)); // allow dummy access to autoload this package
    }
    let data_pt = dir_info.get("DataPt");
    // edit a copy of the custom function 2 data
    let buff = substr(it, &data_pt.deref(), &dir_info.get("DirStart"), Some(&dir_info.get("DirLen")))?;
    let buff_ref = pl::sref(buff);
    let d = Value::new_hash();
    d.set("DataPt", buff_ref.clone());
    d.set("DirStart", n(0));
    d.set("DirLen", dir_info.get("DirLen"));
    d.set("Write", n(1));
    let r = process_canon_custom2(it, vec![et_, d, tag_table_ptr], Want::Scalar)?;
    if !arg(&r, 0).truthy() {
        return pl::ret_undef();
    }
    ret(buff_ref.deref())
}

/// Process Canon custom directory
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_canon_custom(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et_, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let offset = dir_info.get("DirStart");
    let size = dir_info.get("DirLen");
    let verbose = pl::method(it, &et_, "Options", vec![pl::s("Verbose")])?;

    // first entry in array must be the size
    let len = et(it, "Get16u", vec![data_pt.clone(), offset.clone()])?;
    if !(pl::num_eq(&len, &size) || (re(r"\bD60\b", "").is_match(&et_.get("Model")) && pl::num_eq(&pl::add(&len, &n(2)), &size))) {
        pl::method(it, &et_, "Warn", vec![pl::s("Invalid CanonCustom data")])?;
        return ret(n(0));
    }
    if verbose.truthy() {
        let c = pl::sub(&pl::div(&size, &n(2))?, &n(1));
        pl::method(it, &et_, "VerboseDir", vec![pl::s("CanonCustom"), c])?;
    }
    let mut pos = n(2);
    while pl::lt(&pos, &size) {
        // ($pos is position within custom directory)
        let mut val = et(it, "Get16u", vec![data_pt.clone(), pl::add(&offset, &pos)])?;
        let tag = ops::shift_right(&val, &n(8));
        val = ops::bit_and(&val, &n(0xff));
        let index = pl::sub(&pl::div(&pos, &n(2))?, &n(1));
        pl::method(it, &et_, "HandleTag", vec![tag_table_ptr.clone(), tag, val, pl::s("Index"), index, pl::s("Format"), pl::s("int8u"), pl::s("Count"), n(1), pl::s("Size"), n(1)])?;
        pos = pl::add(&pos, &n(2));
    }
    ret(n(1))
}

/// Check new value for Canon custom data block
/// Inputs: 0) ExifTool object reference, 1) tagInfo hash ref, 2) raw value ref
/// Returns: error string or undef (and may modify value) on success
pub fn check_canon_custom(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let val_ptr = arg(&a, 2);
    ret(et(it, "CheckValue", vec![val_ptr, pl::s("int8u")])?)
}

/// Write Canon custom data
/// Inputs: 0) ExifTool object reference, 1) dirInfo hash ref, 2) tag table ref
/// Returns: New custom data block or undefined on error
pub fn write_canon_custom(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et_, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if !et_.truthy() {
        return ret(n(1)); // allow dummy access to autoload this package
    }
    let mut data_pt = dir_info.get("DataPt");
    let ds = dir_info.get("DirStart");
    let dir_start = if ds.truthy() { ds } else { n(0) };
    let dl = dir_info.get("DirLen");
    let dir_len = if dl.truthy() { dl } else { pl::sub(&pl::length(&data_pt.deref()), &dir_start) };
    let dir_name = dir_info.get("DirName");
    let new_data = substr(it, &data_pt.deref(), &dir_start, Some(&dir_len))?;
    if !new_data.truthy() {
        return pl::ret_undef();
    }
    data_pt = pl::sref(new_data);

    // first entry in array must be the size
    let len = et(it, "Get16u", vec![data_pt.clone(), n(0)])?;
    if !(pl::num_eq(&len, &dir_len) || (re(r"\bD60\b", "").is_match(&et_.get("Model")) && pl::num_eq(&pl::add(&len, &n(2)), &dir_len))) {
        pl::method(it, &et_, "Warn", vec![pl::s("Invalid CanonCustom data")])?;
        return pl::ret_undef();
    }
    let new_tags = pl::method(it, &et_, "GetNewTagInfoHash", vec![tag_table_ptr])?;
    let mut pos = n(2);
    while pl::lt(&pos, &dir_len) {
        'body: {
            let mut val = et(it, "Get16u", vec![data_pt.clone(), pos.clone()])?;
            let tag = ops::shift_right(&val, &n(8));
            let tag_info = new_tags.get(tag.to_bytes());
            if !tag_info.truthy() {
                break 'body;
            }
            let nv_hash = pl::method(it, &et_, "GetNewValueHash", vec![tag_info.clone()])?;
            val = ops::bit_and(&val, &n(0xff));
            if !pl::method(it, &et_, "IsOverwriting", vec![nv_hash.clone(), val.clone()])?.truthy() {
                break 'body;
            }
            let new_val = pl::method(it, &et_, "GetNewValue", vec![nv_hash])?;
            if new_val.is_undef() {
                break 'body; // can't delete from a custom table
            }
            let v = pl::add(&ops::bit_and(&new_val, &n(0xff)), &ops::shift_left(&tag, &n(8)));
            et(it, "Set16u", vec![v, data_pt.clone(), pos.clone()])?;
            let name = tag_info.get("Name");
            let minus = pl::concat(&pl::concat(&pl::concat(&pl::s("- "), &dir_name), &pl::s(":")), &name);
            pl::method(it, &et_, "VerboseValue", vec![minus, val])?;
            let plus = pl::concat(&pl::concat(&pl::concat(&pl::s("+ "), &dir_name), &pl::s(":")), &name);
            pl::method(it, &et_, "VerboseValue", vec![plus, new_val])?;
            et_.set("CHANGED", pl::add(&et_.get("CHANGED"), &n(1)));
        }
        pos = pl::add(&pos, &n(2));
    }
    ret(data_pt.deref())
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::CanonCustom::ConvertPfn", convert_pfn);
    it.define_native("Image::ExifTool::CanonCustom::ConvertPfnInv", convert_pfn_inv);
    it.define_native("Image::ExifTool::CanonCustom::ProcessCanonCustom2", process_canon_custom2);
    it.define_native("Image::ExifTool::CanonCustom::WriteCanonCustom2", write_canon_custom2);
    it.define_native("Image::ExifTool::CanonCustom::ProcessCanonCustom", process_canon_custom);
    it.define_native("Image::ExifTool::CanonCustom::CheckCanonCustom", check_canon_custom);
    it.define_native("Image::ExifTool::CanonCustom::WriteCanonCustom", write_canon_custom);
}

