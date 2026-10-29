//! Image::ExifTool::ICC_Profile (Image/ExifTool/ICC_Profile.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::{die, Binding, ER};
use crate::interp::{Interp, Scope, Want};
use crate::modules::random_access::{raf_read, substr};
use crate::perl::Value;
use crate::pl::{self, arg, cap, re, ret};

fn n(i: i64) -> Value {
    Value::Int(i)
}

fn get16u(it: &mut Interp, dp: &Value, off: Value) -> ER<Value> {
    pl::call(it, "Image::ExifTool::Get16u", vec![dp.clone(), off])
}

fn get32u(it: &mut Interp, dp: &Value, off: Value) -> ER<Value> {
    pl::call(it, "Image::ExifTool::Get32u", vec![dp.clone(), off])
}

/// `ReadValue(...)`, in the context given.
fn read_value(it: &mut Interp, args: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    it.call_named("Image::ExifTool::ReadValue", args, want)
}

/// The key/value pairs of a hash-list argument.
fn kv(pairs: Vec<(&str, Value)>) -> Vec<Value> {
    pairs.into_iter().flat_map(|(k, v)| [pl::s(k), v]).collect()
}

/// `$raf->METHOD(...)` dies as perl does on an undefined `$raf` (the read
/// itself runs as a Perl snippet, whose messages name the snippet).
fn raf_defined(raf: &Value, method: &str) -> ER<()> {
    if raf.is_undef() {
        return crate::interp::eval::die(format!("Can't call method \"{method}\" on an undefined value"));
    }
    Ok(())
}

/// Print ICC Profile ID in hex
/// Inputs: 1) string of numbers
/// Returns: string of hex digits
pub fn hex_id(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut val = arg(&a, 0);
    let vals = pl::split_ws(&val);
    // return a simple zero if no MD5 done
    let r = re(r"^0", "");
    if !vals.iter().any(|v| !r.is_match(v)) {
        return ret(n(0));
    }
    val = pl::s("");
    for v in &vals {
        val = pl::concat(&val, &pl::sprintf("%.2x", &[v.clone()]));
    }
    ret(val)
}

/// Get formatted value from ICC tag (which has the type embedded)
/// Inputs: 0) data reference, 1) offset to tag data, 2) tag data size
/// Returns: Formatted value or undefined if format not supported
/// Notes: The following types are handled by BinaryTables:
///  chromaticityType, colorantTableType, measurementType, viewingConditionsType
/// The following types are currently not handled (most are large tables):
///  curveType, lut16Type, lut8Type, lutAtoBType, lutBtoAType, namedColor2Type,
///  parametricCurveType, profileSeqDescType, responseCurveSet16Type
/// The multiLocalizedUnicodeType must be handled by the calling routine.
pub fn format_icc_tag(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (data_pt, offset, size) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));

    let type_ = if pl::ge(&size, &n(8)) {
        // get data type from start of tag data
        pl::at(1010);
        substr(it, &pl::sderef(&data_pt)?, &offset, Some(&n(4)))?
    } else {
        pl::s("err")
    };
    // colorantOrderType
    if type_.eq_str("clro") && pl::ge(&size, &n(12)) {
        let num = get32u(it, &data_pt, pl::add(&offset, &n(8)))?;
        if pl::ge(&size, &pl::add(&num, &n(12))) {
            let pos = pl::add(&offset, &n(12));
            let tmpl = format!("x{} c{}", pos.to_text(), num.to_text());
            pl::at(1019);
            let vals = match crate::perl::pack::unpack(tmpl.as_bytes(), &pl::sderef(&data_pt)?.to_bytes()) {
                Ok(v) => v,
                Err(e) => return die(e),
            };
            let joined: Vec<Vec<u8>> = vals.iter().map(|v| v.to_bytes().to_vec()).collect();
            return ret(Value::bytes(joined.join(&b' ')));
        }
    }
    // dataType
    if type_.eq_str("data") && pl::ge(&size, &n(12)) {
        let form = get32u(it, &data_pt, pl::add(&offset, &n(8)))?;
        // format 0 is UTF-8 data
        if pl::num_eq(&form, &n(0)) {
            pl::at(1026);
            return ret(substr(it, &pl::sderef(&data_pt)?, &pl::add(&offset, &n(12)), Some(&pl::sub(&size, &n(12))))?);
        }
        // binary data and other data types treat as binary (ie. don't format)
    }
    // dateTimeType
    if type_.eq_str("dtim") && pl::ge(&size, &n(20)) {
        let mut v = Vec::new();
        for k in [8, 10, 12, 14, 16, 18] {
            v.push(get16u(it, &data_pt, pl::add(&offset, &n(k)))?);
        }
        return ret(pl::sprintf("%.4d:%.2d:%.2d %.2d:%.2d:%.2d", &v));
    }
    // s15Fixed16ArrayType
    if type_.eq_str("sf32") {
        let cnt = pl::div(&pl::sub(&size, &n(8)), &n(4))?;
        return read_value(it, vec![data_pt.clone(), pl::add(&offset, &n(8)), pl::s("fixed32s"), cnt, pl::sub(&size, &n(8))], want);
    }
    // signatureType
    if type_.eq_str("sig ") && pl::ge(&size, &n(12)) {
        pl::at(1042);
        return ret(substr(it, &pl::sderef(&data_pt)?, &pl::add(&offset, &n(8)), Some(&n(4)))?);
    }
    // textType
    if type_.eq_str("text") {
        pl::at(1045);
        return ret(substr(it, &pl::sderef(&data_pt)?, &pl::add(&offset, &n(8)), Some(&pl::sub(&size, &n(8))))?);
    }
    // textDescriptionType (ref 2, replaced by multiLocalizedUnicodeType)
    if type_.eq_str("desc") && pl::ge(&size, &n(12)) {
        let len = get32u(it, &data_pt, pl::add(&offset, &n(8)))?;
        if pl::ge(&size, &pl::add(&len, &n(12))) {
            pl::at(1050);
            let mut str_ = substr(it, &pl::sderef(&data_pt)?, &pl::add(&offset, &n(12)), Some(&len))?;
            re(r"\0.*", "s").subst(&mut str_, false, |_| Vec::new()); // truncate at null terminator
            return ret(str_);
        }
    }
    // u16Fixed16ArrayType
    if type_.eq_str("uf32") {
        let cnt = pl::div(&pl::sub(&size, &n(8)), &n(4))?;
        return read_value(it, vec![data_pt.clone(), pl::add(&offset, &n(8)), pl::s("fixed32u"), cnt, pl::sub(&size, &n(8))], want);
    }
    // uInt32ArrayType
    if type_.eq_str("ui32") {
        let cnt = pl::div(&pl::sub(&size, &n(8)), &n(4))?;
        return read_value(it, vec![data_pt.clone(), pl::add(&offset, &n(8)), pl::s("int32u"), cnt, pl::sub(&size, &n(8))], want);
    }
    // uInt64ArrayType
    if type_.eq_str("ui64") {
        let cnt = pl::div(&pl::sub(&size, &n(8)), &n(8))?;
        return read_value(it, vec![data_pt.clone(), pl::add(&offset, &n(8)), pl::s("int64u"), cnt, pl::sub(&size, &n(8))], want);
    }
    // uInt8ArrayType
    if type_.eq_str("ui08") {
        return read_value(it, vec![data_pt.clone(), pl::add(&offset, &n(8)), pl::s("int8u"), pl::sub(&size, &n(8)), pl::sub(&size, &n(8))], want);
    }
    // XYZType
    if type_.eq_str("XYZ ") {
        let mut str_ = pl::s("");
        let mut pos = n(8);
        while pl::le(&pl::add(&pos, &n(12)), &size) {
            if str_.truthy() {
                str_ = pl::concat(&str_, &pl::s(", "));
            }
            let v = read_value(it, vec![data_pt.clone(), pl::add(&offset, &pos), pl::s("fixed32s"), n(3), pl::sub(&size, &pos)], Want::Scalar)?;
            str_ = pl::concat(&str_, &v.last().cloned().unwrap_or_default());
            pos = pl::add(&pos, &n(12));
        }
        return ret(str_);
    }
    pl::ret_undef() // data type is not supported
}

/// Process ICC metadata record (ref 5)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_metadata(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let dir_start = dir_info.get("DirStart");
    let dir_len = dir_info.get("DirLen");
    let dir_end = pl::add(&dir_start, &dir_len);

    pl::at(1096);
    if pl::lt(&dir_len, &n(16)) || !substr(it, &pl::sderef(&data_pt)?, &dir_start, Some(&n(4)))?.eq_str("dict") {
        pl::method(it, &et, "Warn", vec![pl::s("Invalid ICC meta dictionary")])?;
        return ret(n(0));
    }
    let num = get32u(it, &data_pt, pl::add(&dir_start, &n(8)))?;
    pl::method(it, &et, "VerboseDir", vec![pl::s("Metadata"), num.clone()])?;
    let size = get32u(it, &data_pt, pl::add(&dir_start, &n(12)))?;
    if pl::lt(&size, &n(16)) {
        pl::method(it, &et, "Warn", vec![pl::s("Invalid ICC meta record size")])?;
        return ret(n(0));
    }
    // NOTE: In the example the minimum offset is 20,
    // but this doesn't jive with the table (both in ref 5)
    let min_ptr = pl::add(&n(16), &pl::mul(&size, &num));
    let mut index = n(0);
    while pl::lt(&index, &num) {
        let entry = pl::add(&pl::add(&dir_start, &n(16)), &pl::mul(&size, &index));
        if pl::gt(&pl::add(&entry, &size), &dir_end) {
            pl::method(it, &et, "Warn", vec![pl::s("Truncated ICC meta dictionary")])?;
            break;
        }
        let name_ptr = get32u(it, &data_pt, entry.clone())?;
        let name_len = get32u(it, &data_pt, pl::add(&entry, &n(4)))?;
        let value_ptr = get32u(it, &data_pt, pl::add(&entry, &n(8)))?;
        let value_len = get32u(it, &data_pt, pl::add(&entry, &n(12)))?;
        if !(name_ptr.truthy() && value_ptr.truthy()) {
            // ignore if offsets are zero
            index = pl::add(&index, &n(1));
            continue;
        }
        if pl::lt(&name_ptr, &min_ptr) || pl::gt(&pl::add(&name_ptr, &name_len), &dir_len) || pl::lt(&value_ptr, &min_ptr) || pl::gt(&pl::add(&value_ptr, &value_len), &dir_len) {
            pl::method(it, &et, "Warn", vec![pl::s("Corrupted ICC meta dictionary")])?;
            break;
        }
        pl::at(1125);
        let mut tag = substr(it, &pl::sderef(&data_pt)?, &pl::add(&dir_start, &name_ptr), Some(&name_len))?;
        let mut val = substr(it, &pl::sderef(&data_pt)?, &pl::add(&dir_start, &value_ptr), Some(&value_len))?;
        tag = pl::method(it, &et, "Decode", vec![tag, pl::s("UTF16"), pl::s("MM"), pl::s("UTF8")])?;
        val = pl::method(it, &et, "Decode", vec![val, pl::s("UTF16"), pl::s("MM")])?;
        // generate tagInfo if it doesn't exist
        if !tag_table_ptr.get(tag.to_bytes()).truthy() {
            let mut name = {
                let mut o = tag.to_bytes().to_vec();
                if let Some(f) = o.first_mut() {
                    *f = f.to_ascii_uppercase();
                }
                Value::bytes(o)
            };
            re(r"\s+(.)", "").subst(&mut name, true, |c| cap(c, 1).to_bytes().to_ascii_uppercase());
            name = Value::bytes(crate::perl::tr::tr(&name.to_bytes(), b"-_a-zA-Z0-9", b"", "dc").0);
            if pl::length(&name).to_i64() == 0 {
                index = pl::add(&index, &n(1));
                continue;
            }
            let indent = et.get("INDENT");
            let msg = pl::concat(&pl::concat(&pl::s("[adding "), &tag), &pl::s("]\n"));
            pl::method(it, &et, "VPrint", vec![n(0), indent, msg])?;
            let ti = Value::new_hash();
            ti.set("Name", name);
            pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), ti])?;
        }
        pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tag, val])?;
        index = pl::add(&index, &n(1));
    }
    ret(n(1))
}

/// Write ICC profile file
/// Inputs: 0) ExifTool object reference, 1) Reference to directory information
/// Returns: 1 on success, 0 if this wasn't a valid ICC file,
///          or -1 if a write error occurred
pub fn write_icc(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    // first make sure this is a valid ICC file (or no file at all)
    let raf = dir_info.get("RAF");
    let mut buff = Value::Undef;
    pl::at(1154);
    raf_defined(&raf, "Read")?;
    if raf_read(it, &raf, &mut buff, n(24))?.truthy() && validate_icc_impl(it, &Value::binary(buff.to_bytes()))?.truthy() {
        return ret(n(0));
    }
    // now write the new ICC
    buff = write_icc_profile_impl(it, &et, &dir_info)?;
    if !buff.is_undef() && pl::length(&buff).truthy() {
        if !pl::call(it, "Image::ExifTool::Write", vec![dir_info.get("OutFile"), buff])?.truthy() {
            return ret(n(-1));
        }
    } else {
        pl::method(it, &et, "Error", vec![pl::s("No ICC information to write")])?;
    }
    ret(n(1))
}

fn write_icc_profile_impl(it: &mut Interp, et: &Value, dir_info: &Value) -> ER<Value> {
    if !et.truthy() {
        return Ok(n(1)); // allow dummy access
    }
    let dn = dir_info.get("DirName");
    let dir_name = if dn.truthy() { dn } else { pl::s("ICC_Profile") };
    // (don't write AsShotICCProfile or CurrentICCProfile here)
    if !dir_name.eq_str("ICC_Profile") {
        return Ok(Value::Undef);
    }
    let extra = match it.global(b'%', "Image::ExifTool::Extra") {
        Binding::Hash(h) => h.borrow().get(&dir_name.to_bytes()).cloned().unwrap_or_default(),
        _ => Value::Undef,
    };
    let nv_hash = pl::method(it, et, "GetNewValueHash", vec![extra])?;
    let mut val = pl::method(it, et, "GetNewValue", vec![nv_hash.clone()])?;
    if val.is_undef() {
        val = pl::s("");
    }
    if !pl::method(it, et, "IsOverwriting", vec![nv_hash, val.clone()])?.truthy() {
        return Ok(Value::Undef);
    }
    et.set("CHANGED", pl::add(&et.get("CHANGED"), &n(1)));
    Ok(val)
}

/// Write ICC data as a block
/// Inputs: 0) ExifTool object reference, 1) source dirInfo reference,
///         2) tag table reference
/// Returns: ICC data block (may be empty if no ICC data)
/// Notes: Increments ExifTool CHANGED flag if changed
pub fn write_icc_profile(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(write_icc_profile_impl(it, &arg(&a, 0), &arg(&a, 1))?)
}

fn validate_icc_impl(it: &mut Interp, val_ptr: &Value) -> ER<Value> {
    let mut err = Value::Undef;
    pl::at(1194);
    let v = pl::sderef(&val_ptr)?;
    if pl::lt(&pl::length(&v), &n(24)) {
        return Ok(pl::s("Invalid ICC profile"));
    }
    let class = substr(it, &v, &n(12), Some(&n(4)))?;
    let profile_class = crate::runtime::lex_hash(it, "Image::ExifTool::ICC_Profile", "profileClass");
    if !profile_class.get(class.to_bytes()).truthy() {
        err = pl::s("profile class");
    }
    let col = substr(it, &v, &n(16), Some(&n(4)))?; // ColorSpaceData
    let con = substr(it, &v, &n(20), Some(&n(4)))?; // ConnectionSpace
    let m = re(r"(XYZ |Lab |Luv |YCbr|Yxy |RGB |GRAY|HSV |HLS |CMYK|CMY |[2-9A-F]CLR|nc..|\0{4})", "");
    if !m.is_match(&col) {
        err = pl::s("color space");
    }
    if !m.is_match(&con) {
        err = pl::s("connection space");
    }
    Ok(if err.truthy() { pl::concat(&pl::concat(&pl::s("Invalid ICC profile (bad "), &err), &pl::s(")")) } else { Value::Undef })
}

/// Validate ICC data
/// Inputs: 0) ICC data reference
/// Returns: error string or undef on success
pub fn validate_icc(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(validate_icc_impl(it, &arg(&a, 0))?)
}

/// Process ICC profile file
/// Inputs: 0) ExifTool object reference, 1) Reference to directory information
/// Returns: 1 if this was an ICC file
pub fn process_icc(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let mut buff = Value::Undef;
    pl::at(1213);
    raf_defined(&raf, "Read")?;
    if !pl::num_eq(&raf_read(it, &raf, &mut buff, n(24))?, &n(24)) {
        return ret(n(0));
    }
    // check to see if this is a valid ICC profile file
    if validate_icc_impl(it, &Value::binary(buff.to_bytes()))?.truthy() {
        return ret(n(0));
    }
    pl::method(it, &et, "SetFileType", vec![])?;
    // read the profile
    let size = match crate::perl::pack::unpack(b"N", &buff.to_bytes()) {
        Ok(v) => arg(&v, 0),
        Err(e) => return die(e),
    };
    if pl::lt(&size, &n(128)) || crate::perl::ops::bit_and(&size, &n(0x80000000)).truthy() {
        pl::method(it, &et, "Error", vec![pl::concat(&pl::concat(&pl::s("Bad ICC Profile length ("), &size), &pl::s(")"))])?;
        return ret(n(1));
    }
    pl::method(it, &raf, "Seek", vec![n(0), n(0)])?;
    if !pl::num_eq(&raf_read(it, &raf, &mut buff, size.clone())?, &size) {
        pl::method(it, &et, "Error", vec![pl::s("Truncated ICC profile")])?;
        return ret(n(1));
    }
    let dir_info2 = Value::new_hash();
    dir_info2.set("DataPt", Value::binary(buff.to_bytes()));
    dir_info2.set("DataLen", size.clone());
    dir_info2.set("DirStart", n(0));
    dir_info2.set("DirLen", size);
    let tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![pl::s("Image::ExifTool::ICC_Profile::Main")])?;
    process_icc_profile(it, vec![et, dir_info2, tag_table_ptr], want)
}

/// Process ICC_Profile APP13 record
/// Inputs: 0) ExifTool object reference, 1) Reference to directory information
///         2) Tag table reference (undefined to read ICC file)
/// Returns: 1 on success
pub fn process_icc_profile(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let mut data_pt = dir_info.get("DataPt");
    let ds = dir_info.get("DirStart");
    let dir_start = if ds.truthy() { ds } else { n(0) };
    let dir_len = dir_info.get("DirLen");
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;

    if pl::lt(&dir_len, &n(4)) {
        return ret(n(0));
    }

    // extract binary ICC_Profile data block if binary mode or requested
    if ((et.get("TAGS_FROM_FILE").truthy() && !et.path(&["EXCL_TAG_LOOKUP", "icc_profile"]).truthy()) || et.path(&["REQ_TAG_LOOKUP", "icc_profile"]).truthy())
        // (don't extract from AsShotICCProfile or CurrentICCProfile)
        && (!dir_info.get("Name").truthy() || dir_info.get("Name").eq_str("ICC_Profile"))
    {
        pl::at(1259);
        if data_pt.is_undef() {
            data_pt = pl::sref(Value::Undef); // (substr($$dataPt...) as a method argument autovivifies)
        }
        let v = substr(it, &pl::sderef(&data_pt)?, &dir_start, Some(&dir_len))?;
        pl::method(it, &et, "FoundTag", vec![pl::s("ICC_Profile"), v])?;
    }

    pl::call(it, "Image::ExifTool::SetByteOrder", vec![pl::s("MM")])?; // ICC_Profile is always big-endian

    // check length of table
    let len = get32u(it, &data_pt, dir_start.clone())?;
    if !pl::num_eq(&len, &dir_len) || pl::lt(&len, &n(128)) {
        pl::method(it, &et, "Warn", vec![pl::concat(&pl::concat(&pl::s("Bad length ICC_Profile (length "), &len), &pl::s(")"))])?;
        if pl::lt(&len, &n(128)) || pl::lt(&dir_len, &len) {
            return ret(n(0));
        }
    }
    let mut pos = pl::add(&dir_start, &n(128)); // position at start of table
    let num_entries = get32u(it, &data_pt, pos.clone())?;
    if pl::lt(&num_entries, &n(1)) || pl::ge(&num_entries, &n(0x100)) || pl::gt(&pl::add(&pl::mul(&num_entries, &n(12)), &n(132)), &dir_len) {
        pl::method(it, &et, "Warn", vec![pl::concat(&pl::concat(&pl::s("Bad ICC_Profile table ("), &num_entries), &pl::s(" entries)"))])?;
        return ret(n(0));
    }

    if verbose.truthy() {
        pl::method(it, &et, "VerboseDir", vec![pl::s("ICC_Profile"), num_entries.clone(), dir_len.clone()])?;
        let fake_info = Value::new_hash();
        fake_info.set("Name", pl::s("ProfileHeader"));
        fake_info.set("SubDirectory", Value::new_hash());
        pl::method(it, &et, "VerboseInfo", vec![Value::Undef, fake_info])?;
    }
    // increment ICC dir count
    let dc = et.get_or_new_hash("DIR_COUNT");
    let prev = dc.get("ICC");
    let dir_count = pl::add(&if prev.truthy() { prev } else { n(0) }, &n(1));
    dc.set("ICC", dir_count.clone());
    if pl::gt(&dir_count, &n(1)) {
        et.set("SET_GROUP1", pl::concat(&pl::s("+"), &dir_count));
    }
    // process the header block
    let subdir_info = Value::new_hash();
    subdir_info.set("Name", pl::s("ProfileHeader"));
    subdir_info.set("DataPt", data_pt.clone());
    subdir_info.set("DataLen", dir_info.get("DataLen"));
    subdir_info.set("DirStart", dir_start.clone());
    subdir_info.set("DirLen", n(128));
    subdir_info.set("Parent", dir_info.get("DirName"));
    subdir_info.set("DirName", pl::s("Header"));
    let mut new_tag_table = pl::call(it, "Image::ExifTool::GetTagTable", vec![pl::s("Image::ExifTool::ICC_Profile::Header")])?;
    pl::method(it, &et, "ProcessDirectory", vec![subdir_info.clone(), new_tag_table.clone()])?;

    pos = pl::add(&pos, &n(4)); // skip item count
    let mut index = n(0);
    while pl::lt(&index, &num_entries) {
        let cur_index = index.clone();
        index = pl::add(&index, &n(1));
        let index = cur_index;
        let tag_id = substr(it, &pl::sderef(&data_pt)?, &pos, Some(&n(4)))?;
        let offset = get32u(it, &data_pt, pl::add(&pos, &n(4)))?;
        let size = get32u(it, &data_pt, pl::add(&pos, &n(8)))?;
        pos = pl::add(&pos, &n(12));
        let mut tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag_id.clone()])?;
        // unknown tags aren't generated automatically by GetTagInfo()
        // if the tagID's aren't numeric, so we must do this manually:
        if !tag_info.truthy() && (et.path(&["OPTIONS", "Unknown"]).truthy() || verbose.truthy()) {
            tag_info = Value::new_hash();
            tag_info.set("Unknown", n(1));
            pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_table_ptr.clone(), tag_id.clone(), tag_info.clone()])?;
        }
        if tag_info.is_undef() {
            continue;
        }

        if pl::gt(&pl::add(&offset, &size), &dir_len) {
            pl::method(it, &et, "Warn", vec![pl::s("Bad ICC_Profile table (truncated)")])?;
            break;
        }
        let value_ptr = pl::add(&dir_start, &offset);

        let subdir = tag_info.get("SubDirectory");
        // format the value unless this is a subdirectory
        let mut value = Value::Undef;
        let fmt;
        if pl::gt(&size, &n(4)) {
            fmt = substr(it, &pl::sderef(&data_pt)?, &value_ptr, Some(&n(4)))?;
            // handle multiLocalizedUnicodeType
            if fmt.eq_str("mluc") && !subdir.truthy() {
                if pl::lt(&size, &n(28)) {
                    continue;
                }
                let count = get32u(it, &data_pt, pl::add(&value_ptr, &n(8)))?;
                let rec_len = get32u(it, &data_pt, pl::add(&value_ptr, &n(12)))?;
                if pl::lt(&rec_len, &n(12)) {
                    continue;
                }
                let mut i = n(0);
                while pl::lt(&i, &count) {
                    let rec_pos = pl::add(&pl::add(&value_ptr, &n(16)), &pl::mul(&i, &rec_len));
                    if pl::gt(&pl::add(&rec_pos, &rec_len), &pl::add(&value_ptr, &size)) {
                        break;
                    }
                    let mut lang = substr(it, &pl::sderef(&data_pt)?, &rec_pos, Some(&n(4)))?;
                    let mut lang_info = Value::Undef;
                    // validate language code and change to standard case (just in case)
                    let changed = re(r"^([a-z]{2})([A-Z]{2})$", "i").subst(&mut lang, false, |c| {
                        let mut o = cap(c, 1).to_bytes().to_ascii_lowercase();
                        o.push(b'-');
                        o.extend_from_slice(&cap(c, 2).to_bytes().to_ascii_uppercase());
                        o
                    }) > 0;
                    if changed && !lang.eq_str("en-US") {
                        lang_info = pl::call(it, "Image::ExifTool::GetLangInfo", vec![tag_info.clone(), lang.clone()])?;
                    }
                    let str_len = get32u(it, &data_pt, pl::add(&rec_pos, &n(4)))?;
                    let str_pos = get32u(it, &data_pt, pl::add(&rec_pos, &n(8)))?;
                    if pl::gt(&pl::add(&str_pos, &str_len), &size) {
                        break;
                    }
                    let mut str_ = substr(it, &pl::sderef(&data_pt)?, &pl::add(&value_ptr, &str_pos), Some(&str_len))?;
                    str_ = pl::method(it, &et, "Decode", vec![str_, pl::s("UTF16")])?;
                    let ti = if lang_info.truthy() { lang_info } else { tag_info.clone() };
                    let mut args = vec![tag_table_ptr.clone(), tag_id.clone(), str_.clone()];
                    args.extend(kv(vec![
                        ("TagInfo", ti),
                        ("Table", tag_table_ptr.clone()),
                        ("Index", index.clone()),
                        ("Value", str_),
                        ("DataPt", data_pt.clone()),
                        ("Size", str_len),
                        ("Start", pl::add(&value_ptr, &str_pos)),
                        ("Format", pl::concat(&pl::concat(&pl::s("type '"), &fmt), &pl::s("'"))),
                    ]));
                    pl::method(it, &et, "HandleTag", args)?;
                    i = pl::add(&i, &n(1));
                }
                if pl::lt(&i, &count) {
                    let msg = pl::concat(&pl::concat(&pl::s("Corrupted "), &tag_info.get("Name")), &pl::s(" data"));
                    pl::method(it, &et, "Warn", vec![msg])?;
                }
                continue;
            }
        } else {
            fmt = pl::s("err ");
        }
        if !subdir.truthy() {
            value = format_icc_tag(it, vec![data_pt.clone(), value_ptr.clone(), size.clone()], Want::Scalar)?.pop().unwrap_or_default();
        }
        if verbose.truthy() {
            let mut args = vec![tag_id.clone(), tag_info.clone()];
            args.extend(kv(vec![
                ("Table", tag_table_ptr.clone()),
                ("Index", index.clone()),
                ("Value", value.clone()),
                ("DataPt", data_pt.clone()),
                ("Size", size.clone()),
                ("Start", value_ptr.clone()),
                ("Format", pl::concat(&pl::concat(&pl::s("type '"), &fmt), &pl::s("'"))),
            ]));
            pl::method(it, &et, "VerboseInfo", args)?;
        }
        if subdir.truthy() {
            let name = tag_info.get("Name");
            // (undef $newTagTable: it is assigned below on every path that uses it)
            if subdir.get("TagTable").truthy() {
                new_tag_table = pl::call(it, "Image::ExifTool::GetTagTable", vec![subdir.get("TagTable")])?;
                if !new_tag_table.truthy() {
                    pl::warn(it, &format!("Unknown tag table {}\n", subdir.get("TagTable").to_text()))?;
                    continue;
                }
            } else {
                pl::warn(it, &format!("Must specify TagTable for SubDirectory {}\n", name.to_text()))?;
                continue;
            }
            // %subdirInfo = (...): the same hash, refilled
            for k in subdir_info.keys() {
                subdir_info.delete(&k);
            }
            subdir_info.set("Name", name.clone());
            subdir_info.set("DataPt", data_pt.clone());
            subdir_info.set("DataPos", dir_info.get("DataPos"));
            subdir_info.set("DataLen", dir_info.get("DataLen"));
            subdir_info.set("DirStart", value_ptr.clone());
            subdir_info.set("DirLen", size.clone());
            subdir_info.set("DirName", name.clone());
            subdir_info.set("Parent", dir_info.get("DirName"));
            let type_ = fmt.clone();
            //### eval Validate ($type)
            let validate = subdir.get("Validate");
            let invalid = if validate.is_undef() {
                false
            } else {
                let scope = Scope::new(None);
                scope.scalar("type", type_);
                match it.eval_string(&validate.to_bytes(), "Image::ExifTool::ICC_Profile", &scope, Want::Scalar) {
                    Ok(v) => !v.last().is_some_and(|x| x.truthy()),
                    Err(_) => true,
                }
            };
            if invalid {
                let msg = pl::concat(&pl::concat(&pl::s("Invalid ICC "), &name), &pl::s(" data"));
                pl::method(it, &et, "Warn", vec![msg])?;
            } else {
                pl::method(it, &et, "ProcessDirectory", vec![subdir_info.clone(), new_tag_table.clone(), subdir.get("ProcessProc")])?;
            }
        } else if !value.is_undef() {
            pl::method(it, &et, "FoundTag", vec![tag_info.clone(), value])?;
        } else {
            value = substr(it, &pl::sderef(&data_pt)?, &value_ptr, Some(&size))?;
            // treat unsupported formats as binary data
            if tag_info.get("ValueConv").is_undef() {
                tag_info.set("ValueConv", pl::s(r"\$val"));
            }
            pl::method(it, &et, "FoundTag", vec![tag_info.clone(), value])?;
        }
    }
    et.delete("SET_GROUP1");
    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::ICC_Profile::HexID", hex_id);
    it.define_native("Image::ExifTool::ICC_Profile::FormatICCTag", format_icc_tag);
    it.define_native("Image::ExifTool::ICC_Profile::ProcessMetadata", process_metadata);
    it.define_native("Image::ExifTool::ICC_Profile::WriteICC", write_icc);
    it.define_native("Image::ExifTool::ICC_Profile::WriteICC_Profile", write_icc_profile);
    it.define_native("Image::ExifTool::ICC_Profile::ValidateICC", validate_icc);
    it.define_native("Image::ExifTool::ICC_Profile::ProcessICC", process_icc);
    it.define_native("Image::ExifTool::ICC_Profile::ProcessICC_Profile", process_icc_profile);
}
