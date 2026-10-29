//! Image::ExifTool::Ricoh (Image/ExifTool/Ricoh.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Ricoh EXIF maker notes tags

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::substr;
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

fn hash(pairs: Vec<(&str, Value)>) -> Value {
    let h = Value::new_hash();
    for (k, v) in pairs {
        h.set(k, v);
    }
    h
}

fn or(a: Value, b: impl FnOnce() -> Value) -> Value {
    if a.truthy() {
        a
    } else {
        b()
    }
}

fn warn(it: &mut Interp, et: &Value, msg: Value) -> ER<()> {
    pl::method(it, et, "Warn", vec![msg])?;
    Ok(())
}

fn warn_n(it: &mut Interp, et: &Value, msg: &str, level: i64) -> ER<()> {
    pl::method(it, et, "Warn", vec![pl::s(msg), n(level)])?;
    Ok(())
}

//------------------------------------------------------------------------------
/// Process Ricoh RDT* real-time metadata
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success, otherwise returns 0 and sets a Warning
pub fn process_ricoh_rdt(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let dir_len = dir_info.get("DirLen");
    let dir_name = dir_info.get("DirName");
    let mut rdtg = Value::Undef;
    if pl::lt(&dir_len, &n(16)) {
        return ret(n(0));
    }
    let ee = pl::method(it, &et, "Options", vec![pl::s("ExtractEmbedded")])?;
    if !ee.truthy() {
        warn_n(it, &et, "Use ExtractEmbedded option to read Ricoh real-time metadata", 3)?;
        return ret(n(1));
    }
    pl::at(1014);
    let endian = substr(it, &pl::sderef(&data_pt)?, &n(8), Some(&n(2)))?;
    call(it, "SetByteOrder", vec![pl::s(if pl::str_eq(&endian, "\x23\x01") { "II" } else { "MM" })])?;
    let mut count = call(it, "Get32u", vec![data_pt.clone(), n(0)])?;
    let len = call(it, "Get16u", vec![data_pt.clone(), n(6)])?;
    if pl::str_eq(&dir_name, "RicohRDTG") {
        if pl::lt(&ee, &n(2)) {
            warn_n(it, &et, "Set ExtractEmbedded option to 2 or higher to extract frame timestamps", 3)?;
            return ret(n(1));
        }
        rdtg = n(0);
        if pl::gt(&len, &n(8)) {
            warn(it, &et, pl::s("Unexpected RDTG record length"))?;
        }
    }
    if pl::gt(&pl::add(&pl::mul(&count, &len), &n(16)), &dir_len) {
        warn(it, &et, pl::concat(&pl::concat(&pl::s("Truncated "), &dir_name), &pl::s(" data")))?;
        pl::at(1028);
        count = pl::int(&pl::div(&pl::sub(&dir_len, &n(16)), &len)?);
    }
    pl::method(it, &et, "VerboseDir", vec![dir_name.clone()])?;
    dir_info.set("DirStart", n(16));
    dir_info.set("DirLen", len.clone());
    let mut i = n(0);
    while pl::lt(&i, &count) {
        let dc = pl::add(&et.get("DOC_COUNT"), &n(1));
        et.set("DOC_COUNT", dc.clone());
        et.set("DOC_NUM", dc);
        if !rdtg.is_undef() {
            let r = rdtg.clone();
            rdtg = pl::add(&rdtg, &n(1));
            pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), n(100), r])?;
        }
        pl::method(it, &et, "ProcessBinaryData", vec![dir_info.clone(), tag_table_ptr.clone()])?;
        dir_info.set("DirStart", pl::add(&dir_info.get("DirStart"), &len));
        i = pl::add(&i, &n(1));
    }
    et.delete("DOC_NUM");
    ret(n(1))
}

//------------------------------------------------------------------------------
/// Process Ricoh text-based maker notes
/// Inputs: 0) ExifTool object reference
///         1) Reference to directory information hash
///         2) Pointer to tag table for this directory
/// Returns: 1 on success, otherwise returns 0 and sets a Warning
pub fn process_ricoh_text(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let data_len = dir_info.get("DataLen");
    let dir_start = or(dir_info.get("DirStart"), || n(0));
    let dir_len = or(dir_info.get("DirLen"), || pl::sub(&data_len, &dir_start));
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;

    pl::at(1058);
    let data = substr(it, &pl::sderef(&data_pt)?, &dir_start, Some(&dir_len))?;
    if re(r"^\0", "").is_match(&data) {
        return ret(n(1)); // blank Ricoh maker notes
    }
    pl::method(it, &et, "VerboseDir", vec![pl::s("RicohText"), Value::Undef, dir_len.clone()])?;
    // validate text maker notes
    if !re(r"^(Rev|Rv)", "").is_match(&data) {
        warn(it, &et, pl::s("Bad Ricoh maker notes"))?;
        return ret(n(0));
    }
    for c in re(r"([A-Z][a-z]{1,2})([0-9A-F]+);", "s").each(&data) {
        let tag = cap(&c, 1);
        let val = cap(&c, 2);
        let mut tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
        if verbose.truthy() {
            pl::method(it, &et, "VerboseInfo", vec![tag.clone(), tag_info.clone(), pl::s("Table"), tag_table_ptr.clone(), pl::s("Value"), val.clone()])?;
        }
        if !tag_info.truthy() {
            if !et.get("OPTIONS").get("Unknown").truthy() {
                continue;
            }
            let lim = Value::Code(it.sub_ref("Image::ExifTool::LimitLongValues"));
            tag_info = hash(vec![("Name", pl::concat(&pl::s("Ricoh_Text_"), &tag)), ("Unknown", n(1)), ("PrintConv", lim)]);
            // add tag information to table
            call(it, "AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), tag_info.clone()])?;
        }
        pl::method(it, &et, "FoundTag", vec![tag_info, val])?;
    }
    ret(n(1))
}

//------------------------------------------------------------------------------
/// Process Ricoh APP5 RMETA information
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success, otherwise returns 0 and sets a Warning
pub fn process_ricoh_rmeta(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let dir_start = dir_info.get("DirStart");
    pl::at(1100);
    let data_len = pl::length(&pl::sderef(&data_pt)?);
    let dir_len = pl::sub(&data_len, &dir_start);
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;

    if verbose.truthy() {
        pl::method(it, &et, "VerboseDir", vec![pl::s("Ricoh RMETA")])?;
    }
    if pl::lt(&dir_len, &n(20)) {
        warn_n(it, &et, "Truncated Ricoh RMETA data", 1)?;
        return ret(n(0));
    }
    let mut byte_order = substr(it, &data_pt.deref(), &dir_start, Some(&n(2)))?;
    if pl::str_eq(&byte_order, "\0\0") {
        byte_order = call(it, "GetByteOrder", vec![])?; // (same order as container)
    }
    if !call(it, "SetByteOrder", vec![byte_order.clone()])?.truthy() {
        warn_n(it, &et, "Bad Ricoh RMETA data", 1)?;
        return ret(n(0));
    }
    // get the RMETA segment number
    let rmeta_num = call(it, "Get16u", vec![data_pt.clone(), pl::add(&dir_start, &n(4))])?;
    if !pl::num_eq(&rmeta_num, &n(0)) {
        // not sure how to recognize audio, so do it by checking for "RIFF" header
        // and assume all subsequent RMETA segments are part of the audio data
        // (but it looks like the int16u at $dirStart+6 is the next block number
        // if the data is continued, or 0 for the last block)
        if pl::lt(&dir_len, &n(14)) {
            warn_n(it, &et, "Short Ricoh RMETA block", 1)?;
            return ret(n(0));
        }
        if re(r"^.{20}BARCODE", "s").is_match(&data_pt.deref()) {
            let mut val = substr(it, &data_pt.deref(), &n(20), None)?;
            re(r"\0.*", "s").subst(&mut val, false, |_| Vec::new());
            re(r"^BARCODE\w+,\d{2},", "").subst(&mut val, false, |_| Vec::new());
            let mut codes: Vec<Value> = Vec::new();
            loop {
                let mut one = Value::Undef;
                let did = re(r"(\d+),", "").subst(&mut val, false, |c| {
                    one = cap(c, 1);
                    Vec::new()
                }) > 0;
                if !(did && pl::ge(&pl::length(&val), &one)) {
                    break;
                }
                codes.push(substr(it, &val, &n(0), Some(&one))?);
                if !pl::gt(&pl::length(&val), &one) {
                    break;
                }
                val = substr(it, &val, &pl::add(&one, &n(1)), None)?;
            }
            if !codes.is_empty() {
                pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("_barcode"), Value::new_array(codes)])?;
            }
            return ret(n(1));
        } else if re(r"^.{18}ASCII", "s").is_match(&data_pt.deref()) {
            // (ignore barcode tag names for now)
            return ret(n(1));
        }
        let audio_len = call(it, "Get16u", vec![data_pt.clone(), pl::add(&dir_start, &n(12))])?;
        if pl::gt(&pl::add(&audio_len, &n(14)), &dir_len) {
            warn_n(it, &et, "Truncated Ricoh RMETA audio data", 1)?;
            return ret(n(0));
        }
        let buff = substr(it, &data_pt.deref(), &pl::add(&dir_start, &n(14)), Some(&audio_len))?;
        let sound_file = et.get("VALUE").get("SoundFile");
        if pl::ge(&audio_len, &n(4)) && pl::str_eq(&substr(it, &buff, &n(0), Some(&n(4)))?, "RIFF") {
            pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("_audio"), pl::sref(buff)])?;
        } else if sound_file.truthy() {
            pl::at(1140);
            let sound = pl::sderef(&sound_file)?;
            pl::set_deref(&sound_file, pl::concat(&sound, &buff))?;
        } else {
            warn_n(it, &et, "Unknown Ricoh RMETA type", 1)?;
            return ret(n(0));
        }
        return ret(n(1));
    }
    // decode standard RMETA tag directory
    let (mut tags, mut vals, mut nums): (Vec<Value>, Vec<Value>, Vec<Value>) = (Vec::new(), Vec::new(), Vec::new());
    let (mut val_pos, mut num_pos) = (Value::Undef, Value::Undef);
    let mut pos = pl::add(&dir_start, &call(it, "Get16u", vec![data_pt.clone(), pl::add(&dir_start, &n(8))])?);
    let num_entries = call(it, "Get16u", vec![data_pt.clone(), pos.clone()])?;
    if pl::gt(&num_entries, &n(100)) {
        warn(it, &et, pl::s("Bad RMETA entry count"))?;
        return ret(n(0));
    }
    pos = pl::add(&pos, &n(10)); // start of first RMETA section
    // loop through RMETA sections
    while pl::le(&pos, &pl::sub(&data_len, &n(4))) {
        let type_ = call(it, "Get16u", vec![data_pt.clone(), pos.clone()])?;
        let mut size = call(it, "Get16u", vec![data_pt.clone(), pl::add(&pos, &n(2))])?;
        if !size.truthy() {
            break;
        }
        pos = pl::add(&pos, &n(4));
        size = pl::sub(&size, &n(2));
        if pl::lt(&size, &n(0)) || pl::gt(&pl::add(&pos, &size), &data_len) {
            warn_n(it, &et, "Corrupted Ricoh RMETA data", 1)?;
            break;
        }
        let dat = substr(it, &data_pt.deref(), &pos, Some(&size))?;
        if verbose.truthy() {
            let msg = pl::concat(&et.get("INDENT"), &pl::concat(&pl::concat(&pl::concat(&pl::s("RMETA section type="), &type_), &pl::s(" size=")), &pl::concat(&size, &pl::s("\n"))));
            pl::method(it, &et, "VPrint", vec![n(2), msg])?;
            pl::method(it, &et, "VerboseDump", vec![pl::sref(dat.clone()), pl::s("Addr"), pl::add(&dir_info.get("DataPos"), &pos)])?;
        }
        if pl::num_eq(&type_, &n(1)) {
            // section 1: tag names
            // save the tag names
            tags = pl::split(&re(r"\0", ""), &dat, pl::add(&num_entries, &n(1)).to_i64());
        } else if pl::num_eq(&type_, &n(2)) || pl::num_eq(&type_, &n(18)) {
            // section 2/18: string values (G800 uses type 18)
            // save the tag values (assume "ASCII\0" encoding since others never seen)
            vals = pl::split(&re(r"\0", ""), &dat, pl::add(&num_entries, &n(1)).to_i64());
            val_pos = pos.clone(); // save position of first string value
        } else if pl::num_eq(&type_, &n(3)) {
            // section 3: numerical values
            if pl::lt(&size, &pl::mul(&num_entries, &n(2))) {
                warn(it, &et, pl::s("Truncated RMETA section 3"))?;
            } else {
                // save the numerical tag values
                // (0=empty, 0xffff=text input, otherwise menu item number)
                let t = format!("{}{}", if pl::str_eq(&byte_order, "MM") { "n" } else { "v" }, num_entries.to_text());
                nums = pl::unpack(&t, &dat)?;
                num_pos = pos.clone(); // save position of numerical values
            }
        } else if !pl::num_eq(&type_, &n(16)) {
            let msg = pl::concat(&pl::concat(&pl::concat(&pl::concat(&pl::s("Unrecognized RMETA section (type "), &type_), &pl::s(", len ")), &size), &pl::s(")"));
            warn(it, &et, msg)?;
        }
        pos = pl::add(&pos, &size);
    }
    if tags.is_empty() && vals.is_empty() {
        return ret(n(1));
    }
    if !val_pos.truthy() {
        val_pos = n(0); // (just in case there was no value section)
    }
    // find next tag in null-delimited list
    // unpack numerical values from block of int16u values
    let mut i = n(0);
    while pl::lt(&i, &num_entries) {
        let ii = i.to_i64() as usize;
        let mut tag = arg(&tags, ii);
        let mut val = arg(&vals, ii);
        if val.is_undef() {
            val = pl::s("");
        }
        'entry: {
            if !(!tag.is_undef() && pl::length(&tag).truthy()) {
                if !pl::length(&val).truthy() {
                    val_pos = pl::add(&val_pos, &n(1));
                    break 'entry; // (skip empty entries)
                }
                tag = pl::s("");
            }
            let mut name = tag.clone();
            re(r"\b([a-z])", "s").subst(&mut name, true, |c| cap(c, 1).to_bytes().to_ascii_uppercase()); // capitalize all words
            re(r" (\w)", "").subst(&mut name, true, |c| cap(c, 1).to_bytes().to_ascii_uppercase()); // remove special characters
            if !pl::length(&name).truthy() {
                name = pl::s("RMETA_Unknown");
            }
            let mut num = arg(&nums, ii);
            let mut tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
            if tag_info.truthy() {
                // make sure print conversion is defined
                if pl::ref_of(&tag_info.get("PrintConv")) != "HASH" {
                    tag_info.set("PrintConv", Value::new_hash());
                }
            } else {
                // create tagInfo hash
                tag_info = hash(vec![("Name", name.clone()), ("PrintConv", Value::new_hash())]);
                call(it, "AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), tag_info.clone()])?;
            }
            // use string value directly if no numerical value
            if num.is_undef() {
                num = val.clone();
            }
            // add conversion for this value (replacing any existing entry)
            let conv = if pl::length(&val).truthy() { val.clone() } else { num.clone() };
            tag_info.get("PrintConv").set(num.to_bytes(), conv);
            if verbose.truthy() {
                let mut dat_parms: Vec<Value> = Vec::new();
                if pl::length(&val).truthy() {
                    dat_parms = vec![pl::s("Start"), val_pos.clone(), pl::s("Size"), pl::length(&val), pl::s("Format"), pl::s("string")];
                } else if num_pos.truthy() {
                    dat_parms = vec![pl::s("Start"), pl::add(&num_pos, &pl::mul(&i, &n(2))), pl::s("Size"), n(2), pl::s("Format"), pl::s("int16u")];
                }
                if !dat_parms.is_empty() {
                    dat_parms.extend([pl::s("DataPt"), data_pt.clone(), pl::s("DataPos"), dir_info.get("DataPos")]);
                }
                let mut args = vec![tag.clone(), tag_info.clone(), pl::s("Table"), tag_table_ptr.clone(), pl::s("Value"), num.clone()];
                args.extend(dat_parms);
                pl::method(it, &et, "VerboseInfo", args)?;
            }
            pl::method(it, &et, "FoundTag", vec![tag_info, num])?;
            val_pos = pl::add(&val_pos, &pl::add(&pl::length(&val), &n(1)));
        }
        i = pl::add(&i, &n(1));
    }
    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Ricoh::ProcessRicohRDT", process_ricoh_rdt);
    it.define_native("Image::ExifTool::Ricoh::ProcessRicohText", process_ricoh_text);
    it.define_native("Image::ExifTool::Ricoh::ProcessRicohRMETA", process_ricoh_rmeta);
}
