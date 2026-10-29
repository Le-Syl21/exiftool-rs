//! Image::ExifTool::DNG (Image/ExifTool/DNG.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read DNG-specific information

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::cell;
use crate::modules::write_exif::{self, etf, get, inc, len, put, read_value, s16u, s32u, set_byte_order, write_value};

/// `Get16u($dataPt, $pos)`: read directly from a data reference; anything
/// else goes through the call, so its error names ExifTool.pm, as perl's.
fn g16u(it: &mut Interp, pt: &Value, pos: Value) -> ER<Value> {
    if !matches!(pt, Value::ScalarRef(_) | Value::LvRef(_)) {
        return pl::call(it, "Image::ExifTool::Get16u", vec![pt.clone(), pos]);
    }
    write_exif::g16u(it, pt, pos)
}

/// `Get32u($dataPt, $pos)` (see g16u).
fn g32u(it: &mut Interp, pt: &Value, pos: Value) -> ER<Value> {
    if !matches!(pt, Value::ScalarRef(_) | Value::LvRef(_)) {
        return pl::call(it, "Image::ExifTool::Get32u", vec![pt.clone(), pos]);
    }
    write_exif::g32u(it, pt, pos)
}
use crate::perl::{ops, Value};
use crate::pl::{self, arg, ret};

const MODULE: &str = "Image::ExifTool::DNG";

fn n(i: i64) -> Value {
    Value::Int(i)
}

fn s(x: &str) -> Value {
    Value::str(x)
}

/// `"$a$b..."`
fn cat(parts: &[&Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

/// `$et->Warn(MSG)`
fn warn(it: &mut Interp, et: &Value, msg: Value) -> ER<()> {
    pl::method(it, et, "Warn", vec![msg])?;
    Ok(())
}

/// `$et->Options(NAME)`
fn opt(it: &mut Interp, et: &Value, name: &str) -> ER<Value> {
    pl::method(it, et, "Options", vec![s(name)])
}

/// `\&NAME` for a sub of this file.
fn sub_ref(it: &mut Interp, name: &str) -> Value {
    Value::Code(it.sub_ref(&format!("{MODULE}::{name}")))
}

/// `$$h{key} += v`
fn add_to(h: &Value, key: &str, v: &Value) {
    h.set(key, pl::add(&h.get(key), v));
}

/// `$$h{key} -= v`
fn sub_from(h: &Value, key: &str, v: &Value) {
    h.set(key, pl::sub(&h.get(key), v));
}

/// Process DNG OriginalRawFileData information
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success, otherwise returns 0 and sets a Warning
pub fn process_original_raw(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let start = dir_info.get("DirStart");
    let end = pl::add(&start, &dir_info.get("DirLen"));
    let mut pos = start.clone();
    let mut err = Value::Undef;

    set_byte_order(it, s("MM"))?; // pointers are always big-endian in this structure
    for index in 0..8i64 {
        if pl::gt(&pl::add(&pos, &n(4)), &end) {
            break;
        }
        let mut val = g32u(it, &data_pt, pos.clone())?;
        if !val.truthy() {
            pos = pl::add(&pos, &n(4)); // ignore zero values
            continue;
        }
        let tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), n(index)])?;
        if !tag_info.truthy() {
            err = s(&format!("Missing DNG tag {index}"));
            break;
        }
        if index & 0x02 != 0 {
            // extract a simple file type (tags 2, 3, 6 and 7)
            val = pl::substr(&data_pt.deref(), pos.to_i64(), Some(4));
            pos = pl::add(&pos, &n(4));
        } else {
            // extract a compressed data block (tags 0, 1, 4 and 5)
            let nn = pl::int(&pl::div(&pl::add(&val, &n(65535)), &n(65536))?);
            let hdr_len = pl::mul(&n(4), &pl::add(&nn, &n(2)));
            if pl::gt(&pl::add(&pos, &hdr_len), &end) {
                err = s("");
                break;
            }
            let tag = tag_info.get("Name");
            // only extract this information if requested (because it takes time)
            let lc_tag = pl::lc(&tag);
            if (et.get("OPTIONS").get("Binary").truthy() && !et.get("EXCL_TAG_LOOKUP").get(lc_tag.to_bytes()).truthy()) || et.get("REQ_TAG_LOOKUP").get(lc_tag.to_bytes()).truthy() {
                if pl::eval_block(it, |it| crate::runtime::require(it, "Compress::Zlib"))?.is_none() {
                    err = s("Install Compress::Zlib to extract compressed images");
                    break;
                }
                val = s("");
                let mut p2 = pl::add(&pos, &g32u(it, &data_pt, pl::add(&pos, &n(4)))?);
                let mut i = n(0);
                while pl::lt(&i, &nn) {
                    // inflate this compressed block
                    let p1 = p2.clone();
                    p2 = pl::add(&pos, &g32u(it, &data_pt, pl::add(&pos, &pl::mul(&pl::add(&i, &n(2)), &n(4))))?);
                    if pl::ge(&p1, &p2) || pl::gt(&p2, &end) {
                        err = s("Bad compressed RAW image");
                        break;
                    }
                    let buff = pl::substr(&data_pt.deref(), p1.to_i64(), Some(pl::sub(&p2, &p1).to_i64()));
                    let (mut v2, mut stat) = (Value::Undef, Value::Undef);
                    let inflate = pl::call(it, "Compress::Zlib::inflateInit", vec![])?;
                    if inflate.truthy() {
                        let mut args = [buff];
                        let r = pl::method_mut(it, &inflate, "inflate", &mut args, Want::List)?;
                        v2 = arg(&r, 0);
                        stat = arg(&r, 1);
                    }
                    if inflate.truthy() && pl::num_eq(&stat, &pl::call(it, "Compress::Zlib::Z_STREAM_END", vec![])?) {
                        val = pl::concat(&val, &v2);
                    } else {
                        err = s("Error inflating compressed RAW image");
                        break;
                    }
                    i = pl::add(&i, &n(1));
                }
                pos = p2;
            } else {
                if pl::gt(&pl::add(&pos, &hdr_len), &end) {
                    err = s("");
                    break;
                }
                let l = g32u(it, &data_pt, pl::sub(&pl::add(&pos, &hdr_len), &n(4)))?;
                if pl::gt(&pl::add(&pos, &l), &end) {
                    err = s("");
                    break;
                }
                let _ = pl::substr(&data_pt.deref(), pl::add(&pos, &hdr_len).to_i64(), Some(pl::sub(&l, &hdr_len).to_i64()));
                val = cat(&[&s("Binary data "), &l, &s(" bytes")]);
                pos = pl::add(&pos, &l); // skip over this block
            }
        }
        pl::method(it, &et, "FoundTag", vec![tag_info, val])?;
    }
    if !err.is_undef() {
        let m = if err.truthy() { err } else { s("Bad OriginalRawFileData") };
        warn(it, &et, m)?;
    }
    ret(n(1))
}

/// Process Adobe DNGPrivateData directory
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_adobe_data(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let data_pos = dir_info.get("DataPos");
    let mut pos = dir_info.get("DirStart");
    let end = pl::add(&dir_info.get("DirLen"), &pos);
    let outfile = dir_info.get("OutFile");
    let verbose = opt(it, &et, "Verbose")?;
    let html_dump = opt(it, &et, "HtmlDump")?;

    pl::at(251);
    if !pl::re(r"^Adobe\0", "").is_match(&pl::sderef(&data_pt)?) {
        return ret(n(0));
    }
    if !outfile.truthy() {
        pl::method(it, &et, "VerboseDir", vec![dir_info.clone()])?;
        // don't parse makernotes if FastScan > 1
        let fast = opt(it, &et, "FastScan")?;
        if fast.truthy() && pl::gt(&fast, &n(1)) {
            return ret(n(1));
        }
    }
    if html_dump.truthy() {
        pl::method(it, &et, "HDump", vec![data_pos.clone(), n(6), s("Adobe DNGPrivateData header")])?;
    }
    set_byte_order(it, s("MM"))?; // always big endian
    pos = pl::add(&pos, &n(6));
    while pl::le(&pl::add(&pos, &n(8)), &end) {
        pl::at(262);
        let u = pl::unpack(&format!("x{}a4N", pos.to_text()), &pl::sderef(&data_pt)?)?;
        let (tag, size) = (arg(&u, 0), arg(&u, 1));
        pos = pl::add(&pos, &n(8));
        if pl::gt(&pl::add(&pos, &size), &end) {
            break;
        }
        let tag_info = tag_table_ptr.get(tag.to_bytes());
        if html_dump.truthy() {
            let mut name = cat(&[&s("Adobe"), &tag]);
            pl::tr(&mut name, " ", "", "d");
            pl::method(it, &et, "HDump", vec![pl::sub(&pl::add(&data_pos, &pos), &n(8)), n(8), cat(&[&name, &s(" header")]), cat(&[&s("Data Size: "), &size, &s(" bytes")])])?;
            // dump non-EXIF format data
            if !pl::re(r"^(MakN|SR2 )$", "").is_match(&tag) {
                pl::method(it, &et, "HDump", vec![pl::add(&data_pos, &pos), size.clone(), cat(&[&name, &s(" data")])])?;
            }
        }
        if verbose.truthy() && !outfile.truthy() {
            if !tag_info.truthy() {
                let m = cat(&[&et.get("INDENT"), &s("Unsupported DNGAdobeData record: ("), &tag, &s(")\n")]);
                pl::method(it, &et, "VPrint", vec![n(0), m])?;
            }
            let ti = if tag_info.is_hash() { tag_info.clone() } else { Value::Undef };
            pl::method(it, &et, "VerboseInfo", vec![tag.clone(), ti, s("DataPt"), data_pt.clone(), s("DataPos"), data_pos.clone(), s("Start"), pos.clone(), s("Size"), size.clone()])?;
        }
        let mut value = Value::Undef;
        while tag_info.truthy() {
            let (sub_table, sub_name, process_proc);
            if tag_info.is_hash() {
                if !tag_info.get("SubDirectory").truthy() {
                    if outfile.truthy() {
                        // copy value across to outfile
                        value = pl::substr(&data_pt.deref(), pos.to_i64(), Some(size.to_i64()));
                    } else {
                        let v = pl::substr(&data_pt.deref(), pos.to_i64(), Some(size.to_i64()));
                        pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tag.clone(), v])?;
                    }
                    break;
                }
                sub_table = etf(it, "GetTagTable", vec![tag_info.get("SubDirectory").get("TagTable")])?;
                sub_name = tag_info.get("Name");
                process_proc = tag_info.get("SubDirectory").get("ProcessProc");
            } else {
                sub_table = tag_table_ptr.clone();
                sub_name = s("AdobeMakN");
                process_proc = sub_ref(it, "ProcessAdobeMakN");
            }
            let sub_dir_info = Value::new_hash();
            sub_dir_info.set("Base", dir_info.get("Base"));
            sub_dir_info.set("DataPt", data_pt.clone());
            sub_dir_info.set("DataPos", data_pos.clone());
            sub_dir_info.set("DataLen", dir_info.get("DataLen"));
            sub_dir_info.set("DirStart", pos.clone());
            sub_dir_info.set("DirLen", size.clone());
            sub_dir_info.set("DirName", sub_name);
            if outfile.truthy() {
                sub_dir_info.set("Proc", process_proc); // WriteAdobeStuff() calls this to do the actual writing
                let wp = sub_ref(it, "WriteAdobeStuff");
                value = pl::method(it, &et, "WriteDirectory", vec![sub_dir_info, sub_table, wp])?;
                // use old directory if an error occurred
                if value.is_undef() {
                    value = pl::substr(&data_pt.deref(), pos.to_i64(), Some(size.to_i64()));
                }
            } else {
                // override process proc for MakN
                pl::method(it, &et, "ProcessDirectory", vec![sub_dir_info, sub_table, process_proc])?;
            }
            break;
        }
        if !value.is_undef() && len(&value).to_i64() > 0 {
            // add "Adobe" header if necessary
            let cur = outfile.deref();
            if !(cur.truthy() && len(&cur).to_i64() > 0) {
                pl::set_deref(&outfile, s("Adobe\0"))?;
            }
            let packed = pl::pack("N", &[len(&value)])?;
            let mut o = cat(&[&outfile.deref(), &tag, &packed, &value]);
            if len(&value).to_i64() & 0x01 != 0 {
                o = pl::concat(&o, &s("\0")); // pad if necessary
            }
            pl::set_deref(&outfile, o)?;
        }
        pos = pl::add(&pos, &size);
        if ops::bit_and(&size, &n(0x01)).truthy() {
            pos = pl::add(&pos, &n(1)); // (darn padding)
        }
    }
    if !pl::num_eq(&pos, &end) {
        warn(it, &et, cat(&[&pos, &s(" "), &end, &s(" Adobe private data is corrupt")]))?;
    }
    ret(n(1))
}

/// Process Adobe CRW directory
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success, otherwise returns 0 and sets a Warning
/// Notes: data has 4 byte header (2 for byte order and 2 for entry count)
/// - this routine would be as simple as ProcessAdobeMRW() below if Adobe hadn't
///   pulled the bonehead move of reformatting the CRW information
pub fn process_adobe_crw(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let start = dir_info.get("DirStart");
    let end = pl::add(&start, &dir_info.get("DirLen"));
    let verbose = opt(it, &et, "Verbose")?;
    let build_maker_notes = opt(it, &et, "MakerNotes")?;
    let outfile = dir_info.get("OutFile");
    let (mut new_tags, mut old_changed) = (Value::Undef, Value::Undef);

    set_byte_order(it, s("MM"))?; // always big endian
    if pl::lt(&dir_info.get("DirLen"), &n(4)) {
        return ret(n(0));
    }
    pl::at(359);
    let byte_order = pl::substr(&pl::sderef(&data_pt)?, start.to_i64(), Some(2));
    if !pl::re(r"^(II|MM)$", "").is_match(&byte_order) {
        return ret(n(0));
    }

    // initialize maker note data if building maker notes
    if build_maker_notes.truthy() {
        pl::call(it, "Image::ExifTool::CanonRaw::InitMakerNotes", vec![et.clone()])?;
    }

    let entries = g16u(it, &data_pt, pl::add(&start, &n(2)))?;
    let mut pos = pl::add(&start, &n(4));
    if !outfile.truthy() {
        pl::method(it, &et, "VerboseDir", vec![dir_info.clone(), entries.clone()])?;
    }
    if outfile.truthy() {
        // get hash of new tags
        new_tags = pl::method(it, &et, "GetNewTagInfoHash", vec![tag_table_ptr.clone()])?;
        pl::set_deref(&outfile, pl::substr(&data_pt.deref(), start.to_i64(), Some(4)))?;
        old_changed = et.get("CHANGED");
    }
    let crw_tag_format = pl::pkg_hash(it, "Image::ExifTool::CanonRaw::crwTagFormat");
    let format_number = pl::pkg_hash(it, "Image::ExifTool::Exif::formatNumber");
    let format_size = pl::pkg_array(it, "Image::ExifTool::Exif::formatSize");
    // loop through entries in Adobe CRW information
    let mut index = n(0);
    while pl::lt(&index, &entries) {
        if pl::gt(&pl::add(&pos, &n(6)), &end) {
            break;
        }
        let tag = g16u(it, &data_pt, pos.clone())?;
        let size = g32u(it, &data_pt, pl::add(&pos, &n(2)))?;
        pos = pl::add(&pos, &n(6));
        if pl::gt(&pl::add(&pos, &size), &end) {
            break;
        }
        let value = cell(pl::substr(&data_pt.deref(), pos.to_i64(), Some(size.to_i64())));
        let value_pt = Value::ScalarRef(value.clone());
        let tag_id = ops::bit_and(&tag, &n(0x3fff));
        let tag_type = ops::bit_and(&ops::shift_right(&tag, &n(8)), &n(0x38)); // get tag type
        let mut format = crw_tag_format.get(tag_type.to_bytes());
        let mut count = Value::Undef;
        let tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag_id.clone(), value_pt.clone()])?;
        if tag_info.truthy() {
            if tag_info.get("Format").truthy() {
                format = tag_info.get("Format");
            }
            count = tag_info.get("Count");
        }
        // set count to 1 by default for values that were in the directory entry
        if count.is_undef() && ops::bit_and(&tag, &n(0x4000)).truthy() && format.truthy() && !format.eq_str("string") {
            count = n(1);
        }
        // set count from tagInfo count if necessary
        if format.truthy() && !count.truthy() {
            // set count according to format and size
            let fnum = format_number.get(format.to_bytes());
            let fsiz = format_size.at(fnum.to_i64());
            pl::at(401);
            count = pl::int(&pl::div(&size, &fsiz)?);
        }
        if !format.truthy() {
            format = s("undef");
        }
        set_byte_order(it, byte_order.clone())?;
        let val = read_value(it, vec![value_pt.clone(), n(0), format.clone(), count.clone(), size.clone()])?;
        if outfile.truthy() {
            if tag_info.truthy() {
                let mut subdir = tag_info.get("SubDirectory");
                if subdir.truthy() && subdir.get("TagTable").truthy() {
                    let name = tag_info.get("Name");
                    let new_tag_table = etf(it, "GetTagTable", vec![subdir.get("TagTable")])?;
                    if !new_tag_table.truthy() {
                        return ret(n(0));
                    }
                    let mut subdir_start = n(0);
                    //### eval Start ()
                    if subdir.get("Start").truthy() {
                        let scope = crate::runtime::file_scope(it, MODULE);
                        subdir_start = pl::eval_site_in(it, &subdir.get("Start"), MODULE, Some(scope), &[], Want::Scalar).0;
                    }
                    let dir_data = value_pt.clone();
                    let subdir_info = Value::new_hash();
                    subdir_info.set("Name", name.clone());
                    subdir_info.set("DataPt", dir_data.clone());
                    subdir_info.set("DataLen", size.clone());
                    subdir_info.set("DirStart", subdir_start.clone());
                    subdir_info.set("DirLen", pl::sub(&size, &subdir_start));
                    subdir_info.set("Parent", dir_info.get("DirName"));
                    //### eval Validate ($dirData, $subdirStart, $size)
                    let invalid = if !subdir.get("Validate").is_undef() {
                        let scope = crate::runtime::file_scope(it, MODULE);
                        let vars = [("dirData", dir_data.clone()), ("subdirStart", subdir_start.clone()), ("size", size.clone())];
                        !pl::eval_site_in(it, &subdir.get("Validate"), MODULE, Some(scope), &vars, Want::Scalar).0.truthy()
                    } else {
                        false
                    };
                    if invalid {
                        warn(it, &et, cat(&[&s("Invalid "), &name, &s(" data")]))?;
                    } else {
                        subdir = pl::method(it, &et, "WriteDirectory", vec![subdir_info, new_tag_table])?;
                        if !subdir.is_undef() && len(&subdir).to_i64() > 0 {
                            if subdir_start.truthy() {
                                // add header before data directory
                                let v = pl::concat(&pl::substr(&get(&value), 0, Some(subdir_start.to_i64())), &subdir);
                                put(&value, v);
                            } else {
                                put(&value, subdir);
                            }
                        }
                    }
                } else if new_tags.get(tag_id.to_bytes()).truthy() {
                    let nv_hash = pl::method(it, &et, "GetNewValueHash", vec![tag_info.clone()])?;
                    if pl::method(it, &et, "IsOverwriting", vec![nv_hash.clone(), val.clone()])?.truthy() {
                        let mut new_val = pl::method(it, &et, "GetNewValue", vec![nv_hash.clone()])?;
                        let mut verbose_val = Value::Undef;
                        if pl::gt(&verbose, &n(1)) {
                            verbose_val = new_val.clone();
                        }
                        // convert to specified format if necessary
                        if !new_val.is_undef() && format.truthy() {
                            new_val = write_value(it, vec![new_val, format.clone(), count.clone()])?;
                        }
                        if !new_val.is_undef() {
                            pl::method(it, &et, "VerboseValue", vec![pl::concat(&s("- CanonRaw:"), &tag_info.get("Name")), get(&value)])?;
                            pl::method(it, &et, "VerboseValue", vec![pl::concat(&s("+ CanonRaw:"), &tag_info.get("Name")), verbose_val])?;
                            put(&value, new_val);
                            inc(&et, "CHANGED");
                        }
                    }
                }
            }
            // write out new value (always big-endian)
            set_byte_order(it, s("MM"))?;
            // (verified that there is no padding here)
            let v = get(&value);
            let s16 = s16u(it, tag.clone())?;
            let s32 = s32u(it, len(&v))?;
            pl::set_deref(&outfile, cat(&[&outfile.deref(), &s16, &s32, &v]))?;
        } else {
            pl::method(
                it,
                &et,
                "HandleTag",
                vec![
                    tag_table_ptr.clone(),
                    tag_id.clone(),
                    val.clone(),
                    s("Index"),
                    index.clone(),
                    s("DataPt"),
                    data_pt.clone(),
                    s("DataPos"),
                    dir_info.get("DataPos"),
                    s("Start"),
                    pos.clone(),
                    s("Size"),
                    size.clone(),
                    s("TagInfo"),
                    tag_info.clone(),
                ],
            )?;
            if build_maker_notes.truthy() {
                // build maker notes information if requested
                pl::call(it, "Image::ExifTool::CanonRaw::BuildMakerNotes", vec![et.clone(), tag_id.clone(), tag_info.clone(), value_pt.clone(), format.clone(), count.clone()])?;
            }
        }
        // (we lost the directory structure, but the second tag 0x0805
        // should be in the ImageDescription directory)
        if pl::num_eq(&tag_id, &n(0x0805)) {
            et.set("DIR_NAME", s("ImageDescription"));
        }
        set_byte_order(it, s("MM"))?;
        pos = pl::add(&pos, &size);
        index = pl::add(&index, &n(1));
    }
    if outfile.truthy() && (outfile.deref().is_undef() || !pl::num_eq(&index, &entries) || pl::num_eq(&et.get("CHANGED"), &old_changed)) {
        et.set("CHANGED", old_changed); // nothing changed
        pl::set_deref(&outfile, Value::Undef)?; // rewrite old directory
    }
    if !pl::num_eq(&index, &entries) {
        warn(it, &et, s("Truncated CRW notes"))?;
    } else if pl::lt(&pos, &end) {
        warn(it, &et, cat(&[&pl::sub(&end, &pos), &s(" extra bytes at end of CRW notes")]))?;
    }
    // finish building maker notes if necessary
    if build_maker_notes.truthy() {
        set_byte_order(it, byte_order)?;
        pl::call(it, "Image::ExifTool::CanonRaw::SaveMakerNotes", vec![et.clone()])?;
    }
    ret(n(1))
}

/// Process Adobe MRW directory
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success, otherwise returns 0 and sets a Warning
/// Notes: data has 4 byte header (2 for byte order and 2 for entry count)
pub fn process_adobe_mrw(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let data_pt = dir_info.get("DataPt");
    let dir_len = dir_info.get("DirLen");
    let dir_start = dir_info.get("DirStart");
    let outfile = dir_info.get("OutFile");

    // construct fake MRW file
    let mut buff = pl::concat(&s("\0MRM"), &pl::pack("N", &[pl::sub(&dir_len, &n(4))])?);
    // ignore leading byte order and directory count words
    pl::at(518);
    buff = pl::concat(&buff, &pl::substr(&pl::sderef(&data_pt)?, pl::add(&dir_start, &n(4)).to_i64(), Some(pl::sub(&dir_len, &n(4)).to_i64())));
    let raf = pl::method(it, &s("File::RandomAccess"), "new", vec![pl::sref(buff)])?;
    let d = Value::new_hash();
    d.set("RAF", raf);
    d.set("OutFile", outfile.clone());
    let rtn_val = pl::call(it, "Image::ExifTool::MinoltaRaw::ProcessMRW", vec![et.clone(), d])?;
    if outfile.truthy() && !outfile.deref().is_undef() && len(&outfile.deref()).to_i64() > 0 {
        // remove MRW header and add Adobe header
        let v = pl::concat(&pl::substr(&data_pt.deref(), dir_start.to_i64(), Some(4)), &pl::substr(&outfile.deref(), 8, None));
        pl::set_deref(&outfile, v)?;
    }
    ret(rtn_val)
}

/// Process Adobe RAF directory
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success, otherwise returns 0 and sets a Warning
pub fn process_adobe_raf(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if dir_info.get("OutFile").truthy() {
        return ret(n(0)); // (can't write this yet)
    }
    let data_pt = dir_info.get("DataPt");
    let mut pos = dir_info.get("DirStart");
    let dir_end = pl::add(&dir_info.get("DirLen"), &pos);
    let (mut read_it, mut warn_flag) = (false, false);

    // set byte order according to first 2 bytes of Adobe RAF data
    if pl::le(&pl::add(&pos, &n(2)), &dir_end) && set_byte_order(it, pl::substr(&data_pt.deref(), pos.to_i64(), Some(2)))?.truthy() {
        pos = pl::add(&pos, &n(2));
    } else {
        warn(it, &et, s("Invalid DNG RAF data"))?;
        return ret(n(0));
    }
    pl::method(it, &et, "VerboseDir", vec![dir_info.clone()])?;
    // make fake RAF object for processing (same acronym, different meaning)
    let raf = pl::method(it, &s("File::RandomAccess"), "new", vec![data_pt.clone()])?;
    let mut num = s("");
    // loop through all records in Adobe RAF data:
    // 0 - RAF table (not processed)
    // 1 - first RAF directory
    // 2 - second RAF directory (if available)
    loop {
        if pl::gt(&pl::add(&pos, &n(4)), &dir_end) {
            break;
        }
        let l = g32u(it, &data_pt, pos.clone())?;
        pos = pl::add(&pos, &pl::add(&n(4), &l)); // step to next entry in Adobe RAF record
        if !l.truthy() {
            break; // ends with an empty entry
        }
        if !read_it {
            read_it = true;
            continue; // ignore first entry (RAF table)
        }
        let d = Value::new_hash();
        d.set("RAF", raf.clone());
        d.set("DirStart", pl::sub(&pos, &l));
        et.set("SET_GROUP1", pl::concat(&s("RAF"), &num));
        if !pl::method(it, &et, "ProcessDirectory", vec![d, tag_table_ptr.clone()])?.truthy() {
            warn_flag = true;
        }
        et.delete("SET_GROUP1");
        num = pl::add(&(if num.truthy() { num.clone() } else { n(1) }), &n(1));
    }
    if warn_flag {
        warn(it, &et, s("Possibly corrupt RAF information"))?;
    }
    ret(n(1))
}

/// Process Adobe SR2 directory
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success, otherwise returns 0 and sets a Warning
/// Notes: data has 6 byte header (2 for byte order and 4 for original offset)
pub fn process_adobe_sr2(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if dir_info.get("OutFile").truthy() {
        return ret(n(0)); // (can't write this yet)
    }
    let data_pt = dir_info.get("DataPt");
    let start = dir_info.get("DirStart");
    let l = dir_info.get("DirLen");

    if pl::lt(&l, &n(6)) {
        return ret(n(0));
    }
    set_byte_order(it, s("MM"))?;
    let original_pos = g32u(it, &data_pt, pl::add(&start, &n(2)))?;
    if !set_byte_order(it, pl::substr(&data_pt.deref(), start.to_i64(), Some(2)))?.truthy() {
        return ret(n(0));
    }

    pl::method(it, &et, "VerboseDir", vec![dir_info.clone()])?;
    let data_pos = dir_info.get("DataPos");
    let dir_start = pl::add(&start, &n(6)); // pointer to maker note directory
    let dir_len = pl::sub(&l, &n(6));

    // initialize subdirectory information
    let fix = pl::sub(&pl::add(&data_pos, &dir_start), &original_pos);
    let subdir_info = Value::new_hash();
    subdir_info.set("DirName", s("AdobeSR2"));
    subdir_info.set("Base", pl::add(&dir_info.get("Base"), &fix));
    subdir_info.set("DataPt", data_pt.clone());
    subdir_info.set("DataPos", pl::sub(&data_pos, &fix));
    subdir_info.set("DataLen", dir_info.get("DataLen"));
    subdir_info.set("DirStart", dir_start);
    subdir_info.set("DirLen", dir_len);
    subdir_info.set("Parent", dir_info.get("DirName"));
    if opt(it, &et, "HtmlDump")?.truthy() {
        pl::method(it, &et, "HDump", vec![pl::add(&data_pos, &start), n(6), s("Adobe SR2 data")])?;
    }
    // parse the SR2 directory
    pl::method(it, &et, "ProcessDirectory", vec![subdir_info, tag_table_ptr])?;
    ret(n(1))
}

/// Process Adobe-mutilated IFD directory
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success, otherwise returns 0 and sets a Warning
/// Notes: data has 2 byte header (byte order of the data)
pub fn process_adobe_ifd(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if dir_info.get("OutFile").truthy() {
        return ret(n(0)); // (can't write this yet)
    }
    let data_pt = dir_info.get("DataPt");
    let mut pos = dir_info.get("DirStart");
    let data_pos = dir_info.get("DataPos");

    if pl::lt(&dir_info.get("DirLen"), &n(4)) {
        return ret(n(0));
    }
    pl::at(633);
    let data_order = pl::substr(&pl::sderef(&data_pt)?, pos.to_i64(), Some(2));
    if !set_byte_order(it, data_order.clone())?.truthy() {
        return ret(n(0)); // validate byte order of data
    }

    // parse the mutilated IFD.  This is similar to a TIFF IFD, except:
    // - data follows directly after Count entry in IFD
    // - byte order of IFD entries is always big-endian, but byte order of data changes
    set_byte_order(it, s("MM"))?; // IFD structure is always big-endian
    let entries = g16u(it, &data_pt, pl::add(&pos, &n(2)))?;
    pl::method(it, &et, "VerboseDir", vec![dir_info.clone(), entries.clone()])?;
    pos = pl::add(&pos, &n(4));

    let end = pl::add(&pos, &dir_info.get("DirLen"));
    let format_size = pl::pkg_array(it, "Image::ExifTool::Exif::formatSize");
    let format_name = pl::pkg_array(it, "Image::ExifTool::Exif::formatName");
    let mut index = n(0);
    while pl::lt(&index, &entries) {
        if pl::gt(&pl::add(&pos, &n(8)), &end) {
            break;
        }
        set_byte_order(it, s("MM"))?; // directory entries always big-endian (doh!)
        let tag_id = g16u(it, &data_pt, pos.clone())?;
        let format = g16u(it, &data_pt, pl::add(&pos, &n(2)))?;
        let count = g32u(it, &data_pt, pl::add(&pos, &n(4)))?;
        if pl::lt(&format, &n(1)) || pl::gt(&format, &n(13)) {
            // warn unless the IFD was just padded with zeros
            if format.truthy() {
                let f = cat(&[&s("Unknown format ("), &format, &s(") for "), &dir_info.get("DirName"), &s(" tag 0x%x")]);
                warn(it, &et, pl::sprintf_v(&f, &[tag_id.clone()]))?;
            }
            return ret(n(0)); // must be corrupted
        }
        let size = pl::mul(&format_size.at(format.to_i64()), &count);
        if pl::gt(&pl::add(&pl::add(&pos, &n(8)), &size), &end) {
            break;
        }
        let format_str = format_name.at(format.to_i64());
        set_byte_order(it, data_order.clone())?; // data stored in native order
        let val = read_value(it, vec![data_pt.clone(), pl::add(&pos, &n(8)), format_str, count, size.clone()])?;
        pl::method(
            it,
            &et,
            "HandleTag",
            vec![tag_table_ptr.clone(), tag_id, val, s("Index"), index.clone(), s("DataPt"), data_pt.clone(), s("DataPos"), data_pos.clone(), s("Start"), pl::add(&pos, &n(8)), s("Size"), size.clone()],
        )?;
        pos = pl::add(&pos, &pl::add(&n(8), &size));
        index = pl::add(&index, &n(1));
    }
    if pl::lt(&index, &entries) {
        warn(it, &et, cat(&[&s("Truncated "), &dir_info.get("DirName"), &s(" directory")]))?;
        return ret(n(0));
    }
    ret(n(1))
}

/// Process Adobe MakerNotes directory
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success, otherwise returns 0 and sets a Warning
/// Notes: data has 6 byte header (2 for byte order and 4 for original offset)
///        --> or 18 bytes for DNG converted from JPG by Adobe Camera Raw!
pub fn process_adobe_makn(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let start = dir_info.get("DirStart");
    let l = dir_info.get("DirLen");
    let outfile = dir_info.get("OutFile");

    if pl::lt(&l, &n(6)) {
        return ret(n(0));
    }
    set_byte_order(it, s("MM"))?;
    let original_pos = g32u(it, &data_pt, pl::add(&start, &n(2)))?;
    if !set_byte_order(it, pl::substr(&data_pt.deref(), start.to_i64(), Some(2)))?.truthy() {
        return ret(n(0));
    }

    if !outfile.truthy() {
        pl::method(it, &et, "VerboseDir", vec![dir_info.clone()])?;
    }
    let data_pos = dir_info.get("DataPos");
    let mut hdr_len = n(6);

    // 2018-09-27: hack for extra 12 bytes in MakN header of JPEG converted to DNG
    // by Adobe Camera Raw (4 bytes "00 00 00 01" followed by 8 unknown bytes)
    // - this is because CameraRaw copies the maker notes from the wrong location
    //   in a JPG image (off by 12 bytes presumably due to the JPEG headers)
    // - this hack won't work in most cases because the extra bytes are not consistent
    //   since they are just the data that existed in the JPG before the maker notes
    // - also, the last 12 bytes of the maker notes will be missing
    // - 2022-04-26: this bug still exists in Camera Raw 14.3
    if pl::ge(&l, &n(18)) && pl::substr(&data_pt.deref(), pl::add(&start, &n(6)).to_i64(), Some(4)).eq_str("\0\0\0\x01") {
        hdr_len = pl::add(&hdr_len, &n(12));
    }

    let dir_start = pl::add(&start, &hdr_len); // pointer to maker note directory
    let dir_len = pl::sub(&l, &hdr_len);

    let hdr = pl::substr(&data_pt.deref(), dir_start.to_i64(), Some(if pl::lt(&dir_len, &n(48)) { dir_len.to_i64() } else { 48 }));
    let tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), s("MakN"), pl::sref(hdr)])?;
    if !(tag_info.truthy() && tag_info.get("SubDirectory").truthy()) {
        return ret(n(0));
    }
    let subdir = tag_info.get("SubDirectory");
    let sub_table = etf(it, "GetTagTable", vec![subdir.get("TagTable")])?;
    // initialize subdirectory information
    let subdir_info = Value::new_hash();
    subdir_info.set("DirName", s("MakerNotes"));
    subdir_info.set("Name", tag_info.get("Name")); // needed for maker notes verbose dump
    subdir_info.set("Base", dir_info.get("Base"));
    subdir_info.set("DataPt", data_pt.clone());
    subdir_info.set("DataPos", data_pos.clone());
    subdir_info.set("DataLen", dir_info.get("DataLen"));
    subdir_info.set("DirStart", dir_start.clone());
    subdir_info.set("DirLen", dir_len.clone());
    subdir_info.set("TagInfo", tag_info.clone());
    subdir_info.set("FixBase", subdir.get("FixBase"));
    subdir_info.set("EntryBased", subdir.get("EntryBased"));
    subdir_info.set("Parent", dir_info.get("DirName"));
    // look for start of maker notes IFD
    let mut loc = pl::call(it, "Image::ExifTool::MakerNotes::LocateIFD", vec![et.clone(), subdir_info.clone()])?;
    if loc.is_undef() {
        warn(it, &et, s("Maker notes could not be parsed"))?;
        return ret(n(0));
    }
    if opt(it, &et, "HtmlDump")?.truthy() {
        pl::method(it, &et, "HDump", vec![pl::add(&data_pos, &start), hdr_len.clone(), s("Adobe MakN data")])?;
        if loc.truthy() {
            pl::method(it, &et, "HDump", vec![pl::add(&data_pos, &dir_start), loc.clone(), cat(&[&tag_info.get("Name"), &s(" header")])])?;
        }
    }

    let mut fix = n(0);
    if !subdir.get("Base").truthy() {
        // adjust base offset for current maker note position
        fix = pl::sub(&pl::add(&data_pos, &dir_start), &original_pos);
        add_to(&subdir_info, "Base", &fix);
        sub_from(&subdir_info, "DataPos", &fix);
    }
    if outfile.truthy() {
        // rewrite the maker notes directory
        let fixup = pl::method(it, &s("Image::ExifTool::Fixup"), "new", vec![])?;
        subdir_info.set("Fixup", fixup.clone());
        let old_changed = et.get("CHANGED");
        let buff = pl::method(it, &et, "WriteDirectory", vec![subdir_info.clone(), sub_table])?;
        // nothing to do if error writing directory or nothing changed
        if !(!buff.is_undef() && !pl::num_eq(&et.get("CHANGED"), &old_changed)) {
            et.set("CHANGED", old_changed);
            return ret(n(1));
        }
        // deleting maker notes if directory is empty
        if len(&buff).to_i64() == 0 {
            pl::set_deref(&outfile, s(""))?;
            return ret(n(1));
        }
        // apply a one-time fixup to offsets
        if subdir_info.get("Relative").truthy() {
            // shift all offsets to be relative to new base
            let base_shift = pl::sub(&pl::add(&pl::add(&data_pos, &dir_start), &dir_info.get("Base")), &subdir_info.get("Base"));
            add_to(&fixup, "Shift", &base_shift);
        } else {
            // shift offsets to position of original maker notes
            add_to(&fixup, "Shift", &original_pos);
        }
        // if we wrote the directory as a block the header is already included
        if subdir_info.get("BlockWrite").truthy() {
            loc = n(0);
        }
        add_to(&fixup, "Shift", &loc); // adjust for makernotes header
        let buff_pt = pl::sref(buff);
        pl::method(it, &fixup, "ApplyFixup", vec![buff_pt.clone()])?; // fix up pointer offsets
        // get copy of original Adobe header (6 or 18) and makernotes header ($loc)
        let header = pl::substr(&data_pt.deref(), start.to_i64(), Some(pl::add(&hdr_len, &loc).to_i64()));
        // add Adobe and makernotes headers to new directory
        pl::set_deref(&outfile, pl::concat(&header, &buff_pt.deref()))?;
    } else {
        // parse the maker notes directory
        pl::method(it, &et, "ProcessDirectory", vec![subdir_info.clone(), sub_table.clone(), subdir.get("ProcessProc")])?;
        // extract maker notes as a block if specified
        if opt(it, &et, "MakerNotes")?.truthy() || et.get("REQ_TAG_LOOKUP").get(pl::lc(&tag_info.get("Name")).to_bytes()).truthy() {
            let mut val = Value::Undef;
            if tag_info.get("MakerNotes").truthy() {
                subdir_info.set("Base", pl::add(&dir_info.get("Base"), &fix));
                subdir_info.set("DataPos", pl::sub(&data_pos, &fix));
                subdir_info.set("DirStart", dir_start.clone());
                subdir_info.set("DirLen", dir_len.clone());
                // rebuild the maker notes to identify all offsets that require fixing up
                val = pl::call(it, "Image::ExifTool::Exif::RebuildMakerNotes", vec![et.clone(), subdir_info.clone(), sub_table.clone()])?;
                if val.is_undef() && pl::gt(&dir_len, &n(4)) {
                    warn(it, &et, s("Error rebuilding maker notes (may be corrupt)"))?;
                }
            } else {
                // extract this directory as a block if specified
                if !tag_info.get("Writable").truthy() {
                    return ret(n(1));
                }
            }
            if val.is_undef() {
                val = pl::substr(&data_pt.deref(), 20, None);
            }
            let key = pl::method(it, &et, "FoundTag", vec![tag_info.clone(), val])?;
            if et.get("MAKER_NOTE_FIXUP").truthy() {
                et.get_or_new_hash("TAG_EXTRA").get_or_new_hash(key.to_bytes()).set("Fixup", et.get("MAKER_NOTE_FIXUP"));
                et.delete("MAKER_NOTE_FIXUP");
            }
        }
    }
    ret(n(1))
}

/// Write Adobe information (calls appropriate ProcessProc to do the actual work)
/// Inputs: 0) ExifTool object ref, 1) source dirInfo ref, 2) tag table ref
/// Returns: new data block (may be empty if directory is deleted) or undef on error
pub fn write_adobe_stuff(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if !et.truthy() {
        return ret(n(1)); // allow dummy access
    }
    let p = dir_info.get("Proc");
    let proc_ = if p.truthy() { p } else { sub_ref(it, "ProcessAdobeData") };
    let buff = cell(Value::Undef);
    dir_info.set("OutFile", Value::ScalarRef(buff.clone()));
    let r = pl::call_code(it, &proc_, vec![et, dir_info, tag_table_ptr], Want::Scalar)?;
    if !r.last().is_some_and(|v| v.truthy()) {
        put(&buff, Value::Undef);
    }
    ret(get(&buff))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::DNG::ProcessOriginalRaw", process_original_raw);
    it.define_native("Image::ExifTool::DNG::ProcessAdobeData", process_adobe_data);
    it.define_native("Image::ExifTool::DNG::ProcessAdobeCRW", process_adobe_crw);
    it.define_native("Image::ExifTool::DNG::ProcessAdobeMRW", process_adobe_mrw);
    it.define_native("Image::ExifTool::DNG::ProcessAdobeRAF", process_adobe_raf);
    it.define_native("Image::ExifTool::DNG::ProcessAdobeSR2", process_adobe_sr2);
    it.define_native("Image::ExifTool::DNG::ProcessAdobeIFD", process_adobe_ifd);
    it.define_native("Image::ExifTool::DNG::ProcessAdobeMakN", process_adobe_makn);
    it.define_native("Image::ExifTool::DNG::WriteAdobeStuff", write_adobe_stuff);
}
