//! Image::ExifTool::FLIR (Image/ExifTool/FLIR.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell, substr};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, n, re, ret};

/// `Image::ExifTool::NAME(ARGS)` (the functions FLIR.pm imports).
fn et(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// `$raf->Read($buff, $len)`: the count, and `buff` updated.
fn raf_read(it: &mut Interp, raf: &Value, buff: &mut Value, len: Value) -> ER<Value> {
    let mut args = [buff.clone(), len];
    let r = pl::method_mut(it, raf, "Read", &mut args, Want::Scalar)?;
    *buff = args[0].clone();
    Ok(r.into_iter().next().unwrap_or_default())
}

/// Get image type from raw image data
/// Inputs: 0) ExifTool ref, 1) image data, 2) tag name
/// Returns: image type (PNG, JPG, TIFF or undef)
/// - image itself is stored in $$et{$tag}
pub fn get_image_type(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et_, mut val, tag) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let w = et_.get(pl::concat(&tag, &pl::s("Width")).to_bytes());
    let h = et_.get(pl::concat(&tag, &pl::s("Height")).to_bytes());
    let mut typ = pl::s("DAT");
    // add TIFF header only if this looks like 16-bit raw data
    // (note: MakeTiffHeader currently works only for little-endian,
    //  and I haven't seen any big-endian samples, but check anwyay)
    if re(r"^\x89PNG\r\n\x1a\n", "").is_match(&val) {
        typ = pl::s("PNG");
    } else if re(r"^\xff\xd8\xff", "").is_match(&val) {
        // (haven't seen this, but just in case - PH)
        typ = pl::s("JPG");
    } else if !pl::num_eq(&pl::length(&val), &pl::mul(&pl::mul(&w, &h), &n(2))) {
        let msg = pl::concat(&pl::concat(&pl::s("Unrecognized FLIR "), &tag), &pl::s(" data format"));
        pl::method(it, &et_, "Warn", vec![msg])?;
    } else if et(it, "GetByteOrder", vec![])?.eq_str("II") {
        let hdr = et(it, "MakeTiffHeader", vec![w, h, n(1), n(16)])?;
        val = pl::concat(&hdr, &val);
        typ = pl::s("TIFF");
    } else {
        pl::method(it, &et_, "Warn", vec![pl::concat(&pl::s("Don't yet support big-endian TIFF "), &tag)])?;
    }
    // save image data
    et_.set(tag.to_bytes(), val);
    ret(typ)
}

fn unescape_flir_impl(it: &mut Interp, ch: &Value) -> ER<Value> {
    if !pl::num_eq(&pl::length(ch), &n(4)) {
        return Ok(ch.clone()); // escaped ASCII char (eg. '\\')
    }
    let val = hex(ch);
    if pl::lt(&val, &n(0x80)) {
        return Ok(Value::bytes([val.to_i64() as u8])); // simple ASCII
    }
    if pl::ge(&it.get_scalar("main::]"), &Value::Num(5.006001)) {
        return pl::pack("C0U", &[val]);
    }
    et(it, "PackUTF8", vec![val])
}

/// `hex $char` for the 4 hex digits matched by the caller.
fn hex(ch: &Value) -> Value {
    let mut v: i64 = 0;
    for &b in ch.to_bytes().iter() {
        match (b as char).to_digit(16) {
            Some(d) => v = v * 16 + d as i64,
            None => break,
        }
    }
    n(v)
}

/// Unescape FLIR Unicode character
/// Inputs: 0) escaped character code
/// Returns: UTF8 character
pub fn unescape_flir(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(unescape_flir_impl(it, &arg(&a, 0))?)
}

/// Process FLIR text info record (ref PH)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_flir_text(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et_, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let ds = dir_info.get("DirStart");
    let dir_start = if ds.truthy() { ds } else { n(0) };
    let dir_len = dir_info.get("DirLen");

    if pl::lt(&dir_len, &n(12)) {
        return ret(n(0));
    }

    pl::method(it, &et_, "VerboseDir", vec![pl::s("FLIR Text")])?;

    let mut dat = substr(it, &data_pt.deref(), &pl::add(&dir_start, &n(12)), Some(&pl::sub(&dir_len, &n(12))))?;
    re(r"\0.*", "s").subst(&mut dat, false, |_| Vec::new()); // truncate at null

    // the parameter text contains an additional header entry...
    let param_info = pl::pkg_hash(it, "Image::ExifTool::FLIR::ParamInfo");
    if tag_table_ptr.same_ref(&param_info) {
        if let Some(m) = re(r"# (Generated) at (.*?)[\n\r]", "").m(&dat) {
            pl::method(it, &et_, "HandleTag", vec![tag_table_ptr.clone(), cap(&m, 1), cap(&m, 2)])?;
        }
    }

    for m in re(r#".(\d+).(label|value|param) (unicode|text) "(.*)""#, "").each(&dat) {
        let (tag, mut val) = (pl::concat(&pl::ucfirst(&cap(&m, 2)), &cap(&m, 1)), cap(&m, 4));
        if pl::str_eq(&cap(&m, 3), "unicode") && re(r"\\", "").is_match(&val) {
            // convert escaped Unicode characters (backslash followed by 4 hex digits)
            let mut err = None;
            re(r"\\([0-9a-fA-F]{4}|.)", "s").subst(&mut val, true, |c| match unescape_flir_impl(it, &cap(c, 1)) {
                Ok(v) => v.to_bytes().to_vec(),
                Err(e) => {
                    err.get_or_insert(e);
                    Vec::new()
                }
            });
            if let Some(e) = err {
                return Err(e);
            }
            pl::method(it, &et_, "Decode", vec![val.clone(), pl::s("UTF8")])?;
        }
        if !tag_table_ptr.get(tag.to_bytes()).truthy() {
            let ti = Value::new_hash();
            ti.set("Name", tag.clone());
            et(it, "AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), ti])?;
        }
        pl::method(it, &et_, "HandleTag", vec![tag_table_ptr.clone(), tag, val])?;
    }

    ret(n(1))
}

/// Process FLIR measurement tool record (ref 6)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
/// (code-driven decoding isn't pretty, but sometimes it is necessary)
pub fn process_meas_info(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et_, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let ds = dir_info.get("DirStart");
    let dir_start = if ds.truthy() { ds } else { n(0) };
    let data_pos = dir_info.get("DataPos");
    let dir_end = pl::add(&dir_start, &dir_info.get("DirLen"));

    let mut pos = pl::add(&dir_start, &n(12));
    if pl::gt(&pos, &dir_end) {
        return ret(n(0));
    }
    if pl::ge(&et(it, "Get16u", vec![data_pt.clone(), dir_start.clone()])?, &n(0x100)) {
        et(it, "ToggleByteOrder", vec![])?;
    }
    let mut i = n(1);
    loop {
        if pl::gt(&pl::add(&pos, &n(2)), &dir_end) {
            break;
        }
        let rec_len = et(it, "Get16u", vec![data_pt.clone(), pos.clone()])?;
        if pl::lt(&rec_len, &n(0x28)) || pl::gt(&pl::add(&pos, &rec_len), &dir_end) {
            break;
        }
        let pre = pl::concat(&pl::s("Meas"), &i);
        pl::method(it, &et_, "VerboseDir", vec![pl::concat(&pl::s("MeasInfo "), &i), Value::Undef, rec_len.clone()])?;
        pl::method(it, &et_, "VerboseDump", vec![data_pt.clone(), pl::s("Len"), rec_len.clone(), pl::s("Start"), pos.clone(), pl::s("DataPos"), data_pos.clone()])?;
        let coord_len = et(it, "Get16u", vec![data_pt.clone(), pl::add(&pos, &n(4))])?;
        // generate tag table entries for this tool if necessary
        for t in ["Type", "Params", "Label"] {
            let tag = pl::concat(&pre, &pl::s(t));
            if tag_table_ptr.get(tag.to_bytes()).truthy() {
                break;
            }
            let tag_info = Value::new_hash();
            tag_info.set("Name", tag.clone());
            let pc = tag_table_ptr.get_or_new_hash(format!("Meas1{t}")).get("PrintConv");
            tag_info.set("PrintConv", pc);
            et(it, "AddTagToTable", vec![tag_table_ptr.clone(), tag, tag_info])?;
        }
        // extract measurement tool type
        let dp = |start: Value, size: Value| vec![pl::s("DataPt"), data_pt.clone(), pl::s("DataPos"), data_pos.clone(), pl::s("Start"), start, pl::s("Size"), size];
        let mut args = vec![tag_table_ptr.clone(), pl::concat(&pre, &pl::s("Type")), Value::Undef];
        args.extend(dp(pl::add(&pos, &n(0x0a)), n(2)));
        pl::method(it, &et_, "HandleTag", args)?;
        if pl::gt(&pl::add(&pl::add(&pos, &n(0x24)), &coord_len), &dir_end) {
            break;
        }
        // extract measurement parameters
        let mut args = vec![tag_table_ptr.clone(), pl::concat(&pre, &pl::s("Params")), Value::Undef];
        args.extend(dp(pl::add(&pos, &n(0x24)), coord_len.clone()));
        pl::method(it, &et_, "HandleTag", args)?;
        let uni = Value::new_array(Vec::new());
        // extract label (sometimes-null-terminated Unicode)
        let mut p = pl::add(&n(0x24), &coord_len);
        while pl::lt(&p, &pl::sub(&rec_len, &n(1))) {
            let ch = et(it, "Get16u", vec![data_pt.clone(), pl::add(&p, &pos)])?;
            // FLIR Tools v2.0 for Mac doesn't properly null-terminate these strings,
            // so end the string at any funny character
            if pl::lt(&ch, &n(0x20)) || pl::gt(&ch, &n(0x7f)) {
                break;
            }
            uni.push(ch);
            p = pl::add(&p, &n(2));
        }
        // convert to the ExifTool character set
        crate::runtime::require(it, "Image::ExifTool::Charset")?;
        let val = pl::call(it, "Image::ExifTool::Charset::Recompose", vec![et_.clone(), uni.clone()])?;
        let mut args = vec![tag_table_ptr.clone(), pl::concat(&pre, &pl::s("Label")), val];
        args.extend(dp(pl::add(&pl::add(&pos, &n(0x24)), &coord_len), n(2 * uni.len() as i64)));
        pl::method(it, &et_, "HandleTag", args)?;
        pos = pl::add(&pos, &rec_len); // step to next record
        i = pl::add(&i, &n(1));
    }
    ret(n(1))
}

/// Process FLIR FFF record (ref PH/1/3)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 if this was a valid FFF record
pub fn process_flir(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et_, dir_info, mut tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let r = dir_info.get("RAF");
    let raf = if r.truthy() { r } else { pl::method(it, &pl::s("File::RandomAccess"), "new", vec![dir_info.get("DataPt")])? };
    let verbose = pl::method(it, &et_, "Options", vec![pl::s("Verbose")])?;
    let out = pl::method(it, &et_, "Options", vec![pl::s("TextOut")])?;
    let base = pl::method(it, &raf, "Tell", vec![])?;
    let (mut hdr, mut buff) = (Value::Undef, Value::Undef);
    let rec = cell(Value::Undef);

    // read and verify FFF header
    let ok = pl::num_eq(&raf_read(it, &raf, &mut hdr, n(0x40))?, &n(0x40));
    let m = if ok { re(r"^([AF]FF)\0", "").m(&hdr) } else { None };
    let Some(m) = m else { return ret(n(0)) };

    let typ = cap(&m, 1);

    // set file type if reading from FFF or SEQ file ($tagTablePtr will not be defined)
    if !tag_table_ptr.truthy() {
        let ft = if pl::str_eq(&typ, "FFF") { pl::s("FLIR") } else { pl::s("SEQ") };
        pl::method(it, &et_, "SetFileType", vec![ft])?;
    }

    // FLIR file header (ref 3)
    // 0x00 - string[4] file format ID = "FFF\0"
    // 0x04 - string[16] file creator: seen "\0","MTX IR\0","CAMCTRL\0"
    // 0x14 - int32u file format version = 100
    // 0x18 - int32u offset to record directory
    // 0x1c - int32u number of entries in record directory
    // 0x20 - int32u next free index ID = 2
    // 0x24 - int16u swap pattern = 0 (?)
    // 0x28 - int16u[7] spares
    // 0x34 - int32u[2] reserved
    // 0x3c - int32u checksum

    // determine byte ordering by validating version number
    // (in my samples FLIR APP1 is big-endian, FFF files are little-endian)
    let mut i = n(0);
    loop {
        let ver = et(it, "Get32u", vec![pl::sref(hdr.clone()), n(0x14)])?;
        if pl::ge(&ver, &n(100)) && pl::lt(&ver, &n(200)) {
            break; // (have seen 100 and 101 - PH)
        }
        et(it, "ToggleByteOrder", vec![])?;
        if i.truthy() {
            if et_.get("DOC_NUM").truthy() {
                return ret(n(0));
            }
            pl::method(it, &et_, "Warn", vec![pl::concat(&pl::concat(&pl::s("Unsupported FLIR "), &typ), &pl::s(" version"))])?;
            return ret(n(1));
        }
        i = pl::add(&i, &n(1));
    }

    // read the FLIR record directory
    let pos = et(it, "Get32u", vec![pl::sref(hdr.clone()), n(0x18)])?;
    let num = et(it, "Get32u", vec![pl::sref(hdr.clone()), n(0x1c)])?;
    let mut ok = pl::method(it, &raf, "Seek", vec![pl::add(&base, &pos)])?.truthy();
    if ok {
        let len = pl::mul(&num, &n(0x20));
        ok = pl::num_eq(&raf_read(it, &raf, &mut buff, len.clone())?, &len);
    }
    if !ok {
        pl::method(it, &et_, "Warn", vec![pl::s("Truncated FLIR FFF directory")])?;
        return ret(if et_.get("DOC_NUM").truthy() { n(0) } else { n(1) });
    }

    if !tag_table_ptr.truthy() {
        tag_table_ptr = et(it, "GetTagTable", vec![pl::concat(&pl::s("Image::ExifTool::FLIR::"), &typ)])?;
        et_.set("SET_GROUP0", pl::s("FLIR")); // (set group 0 to 'FLIR' for FFF files)
    }

    // process the header data
    pl::method(it, &et_, "HandleTag", vec![tag_table_ptr.clone(), pl::s("_header"), hdr.clone()])?;

    let mut success = n(1);
    let old_indent = et_.get("INDENT");
    et_.set("INDENT", pl::concat(&et_.get("INDENT"), &pl::s("| ")));
    pl::method(it, &et_, "VerboseDir", vec![typ.clone(), num.clone()])?;

    let buff_ref = pl::sref(buff.clone());
    i = n(0);
    'rec: while pl::lt(&i, &num) {
        'body: {
            // FLIR record entry (ref 3):
            // 0x00 - int16u record type
            // 0x02 - int16u record subtype: RawData 1=BE, 2=LE, 3=PNG; 1 for other record types
            // 0x04 - int32u record version: seen 0x64,0x66,0x67,0x68,0x6f,0x104
            // 0x08 - int32u index id = 1
            // 0x0c - int32u record offset from start of FLIR data
            // 0x10 - int32u record length
            // 0x14 - int32u parent = 0 (?)
            // 0x18 - int32u object number = 0 (?)
            // 0x1c - int32u checksum: 0 for no checksum

            let entry = pl::mul(&i, &n(0x20));
            let rec_type = et(it, "Get16u", vec![buff_ref.clone(), entry.clone()])?;
            if pl::num_eq(&rec_type, &n(0)) {
                if verbose.truthy() {
                    let line = pl::concat(&pl::concat(&et_.get("INDENT"), &i), &pl::s(") FLIR Record 0x00 (empty)\n"));
                    pl::print_to(it, Some(&out), &[line])?;
                }
                break 'body;
            }
            let rec_pos = et(it, "Get32u", vec![buff_ref.clone(), pl::add(&entry, &n(0x0c))])?;
            let rec_len = et(it, "Get32u", vec![buff_ref.clone(), pl::add(&entry, &n(0x10))])?;

            if verbose.truthy() {
                let line = pl::sprintf("%s%d) FLIR Record 0x%.2x, offset 0x%.4x, length 0x%.4x\n", &[et_.get("INDENT"), i.clone(), rec_type.clone(), rec_pos.clone(), rec_len.clone()]);
                pl::print_to(it, Some(&out), &[line])?;
            }

            // skip RawData records for embedded documents
            if pl::num_eq(&rec_type, &n(1)) && et_.get("DOC_NUM").truthy() && pl::lt(&pl::method(it, &et_, "Options", vec![pl::s("ExtractEmbedded")])?, &n(2)) {
                if !pl::method(it, &raf, "Seek", vec![pl::add(&pl::add(&base, &rec_pos), &rec_len)])?.truthy() {
                    success = n(0);
                    break 'rec;
                }
                break 'body;
            }
            let mut ok = pl::method(it, &raf, "Seek", vec![pl::add(&base, &rec_pos)])?.truthy();
            if ok {
                ok = pl::num_eq(&raf_read_cell(it, &raf, &rec, rec_len.clone())?, &rec_len);
            }
            if !ok {
                if et_.get("DOC_NUM").truthy() {
                    success = n(0); // abort processing more documents
                } else {
                    pl::method(it, &et_, "Warn", vec![pl::s("Invalid FLIR record")])?;
                }
                break 'rec;
            }
            if tag_table_ptr.get(rec_type.to_bytes()).truthy() {
                pl::method(
                    it,
                    &et_,
                    "HandleTag",
                    vec![
                        tag_table_ptr.clone(),
                        rec_type,
                        Value::Undef,
                        pl::s("Base"),
                        base.clone(),
                        pl::s("DataPt"),
                        Value::ScalarRef(rec.clone()),
                        pl::s("DataPos"),
                        rec_pos,
                        pl::s("Start"),
                        n(0),
                        pl::s("Size"),
                        rec_len,
                    ],
                )?;
            } else if pl::gt(&verbose, &n(2)) {
                pl::method(it, &et_, "VerboseDump", vec![Value::ScalarRef(rec.clone()), pl::s("Len"), rec_len, pl::s("DataPos"), rec_pos])?;
            }
        }
        i = pl::add(&i, &n(1));
    }
    et_.delete("SET_GROUP0");
    et_.set("INDENT", old_indent);

    // extract information from subsequent frames in SEQ file if ExtractEmbedded is used
    if dir_info.get("RAF").truthy() && pl::method(it, &et_, "Options", vec![pl::s("ExtractEmbedded")])?.truthy() && !et_.get("DOC_NUM").truthy() {
        loop {
            et_.set("DOC_NUM", pl::add(&et_.get("DOC_COUNT"), &n(1)));
            let r = process_flir(it, vec![et_.clone(), dir_info.clone(), tag_table_ptr.clone()], Want::Scalar)?;
            if !arg(&r, 0).truthy() {
                break;
            }
            // (DOC_COUNT will be incremented automatically if we extracted any tags)
        }
        et_.delete("DOC_NUM");
    }
    ret(success)
}

/// Process FLIR public image format (FPF) file (ref PH/4)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 if this was a valid FFF file
pub fn process_fpf(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et_, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let mut buff = Value::Undef;

    if !(pl::num_eq(&raf_read(it, &raf, &mut buff, n(892))?, &n(892)) && re(r"^FPF Public Image Format\0", "").is_match(&buff)) {
        return ret(n(0));
    }

    // I think these are always little-endian, but check FPFVersion just in case
    et(it, "SetByteOrder", vec![pl::s("II")])?;
    let buff_ref = pl::sref(buff);
    if !ops::bit_and(&et(it, "Get32u", vec![buff_ref.clone(), n(0x20)])?, &n(0xffff)).truthy() {
        et(it, "ToggleByteOrder", vec![])?;
    }

    let tag_table_ptr = et(it, "GetTagTable", vec![pl::s("Image::ExifTool::FLIR::FPF")])?;
    pl::method(it, &et_, "SetFileType", vec![])?;
    let d = Value::new_hash();
    d.set("DataPt", buff_ref);
    d.set("Parent", pl::s("FPF"));
    pl::method(it, &et_, "ProcessDirectory", vec![d, tag_table_ptr])?;
    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::FLIR::GetImageType", get_image_type);
    it.define_native("Image::ExifTool::FLIR::UnescapeFLIR", unescape_flir);
    it.define_native("Image::ExifTool::FLIR::ProcessFLIRText", process_flir_text);
    it.define_native("Image::ExifTool::FLIR::ProcessMeasInfo", process_meas_info);
    it.define_native("Image::ExifTool::FLIR::ProcessFLIR", process_flir);
    it.define_native("Image::ExifTool::FLIR::ProcessFPF", process_fpf);
}
