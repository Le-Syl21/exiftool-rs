//! Image::ExifTool::WriteIPTC (Image/ExifTool/WriteIPTC.pl), ported sub by sub (see docs/port/PORTING.md).
//!
//! Write IPTC meta information

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::random_access::perl_eval1;
use crate::modules::write_exif::{band, cat, cell, etf, get, hex, inc, keys, len, opt, ord, print_out, put, rep, rf, seq, sort_num, substr, wlex_hash};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, re, ret};

const MODULE: &str = "Image::ExifTool::IPTC";
const FILE: &str = "WriteIPTC.pl";

fn n(i: i64) -> Value {
    Value::Int(i)
}

/// Inverse print conversion for CodedCharacterSet
/// Inputs: 0) value
pub fn print_inv_coded_charset(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let val = arg(&a, 0);
    let inv = wlex_hash(it, MODULE, FILE, "iptcCharsetInv");
    let mut code = inv.get(pl::uc(&val).to_bytes());
    if !code.truthy() {
        code = val.clone();
        if re(r"ESC *", "i").subst(&mut code, true, |_| b"\x1b".to_vec()) > 0 {
            // translate ESC chars
            re(r", \x1b", "").subst(&mut code, true, |_| b"\x1b".to_vec()); // remove comma separators
            pl::tr(&mut code, " ", "", "d"); // remove spaces
        } else {
            pl::warn(it, "Bad syntax (use 'UTF8' or 'ESC X Y[, ...]')\n")?;
        }
    }
    ret(code)
}

/// validate raw values for writing
/// Inputs: 0) ExifTool object ref, 1) tagInfo hash ref, 2) raw value ref
/// Returns: error string or undef (and possibly changes value) on success
pub fn check_iptc(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, tag_info, val_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let mut format = tag_info.get("Format");
    if !format.truthy() {
        format = tag_info.get("Table").get("FORMAT");
    }
    if !format.truthy() {
        format = pl::s("");
    }
    if let Some(c) = re(r"^int(\d+)", "").m(&format) {
        let c1 = cap(&c, 1);
        let bytes = pl::int(&pl::div(&if c1.truthy() { c1 } else { n(0) }, &n(8))?);
        if !pl::num_eq(&bytes, &n(1)) && !pl::num_eq(&bytes, &n(2)) && !pl::num_eq(&bytes, &n(4)) {
            return ret(pl::s(&format!("Can't write {}-byte integer", bytes.to_text())));
        }
        let mut val = val_ptr.deref();
        if !pl::call(it, "Image::ExifTool::IsInt", vec![val.clone()])?.truthy() {
            if !pl::call(it, "Image::ExifTool::IsHex", vec![val.clone()])?.truthy() {
                return ret(pl::s("Not an integer"));
            }
            val = hex(&val);
            pl::set_deref(&val_ptr, val.clone())?;
        }
        let mut i = n(0);
        while pl::lt(&i, &bytes) {
            val = ops::shift_right(&val, &n(8));
            i = pl::add(&i, &n(1));
        }
        if val.truthy() {
            return ret(pl::s(&format!("Value too large for {}-byte format", bytes.to_text())));
        }
    } else if let Some(c) = re(r"^(string|digits|undef)\[?(\d+),?(\d*)\]?$", "").m(&format) {
        let (fmt, minlen, mut maxlen) = (cap(&c, 1), cap(&c, 2), cap(&c, 3));
        let mut l = len(&val_ptr.deref());
        if fmt.eq_str("digits") {
            if !re(r"^\d*$", "").is_match(&val_ptr.deref()) {
                return ret(pl::s("Non-numeric characters in value"));
            }
            if pl::lt(&l, &minlen) && l.truthy() {
                // left pad with zeros if necessary
                let v = pl::concat(&rep(b"0", &pl::sub(&minlen, &l)), &val_ptr.deref());
                pl::set_deref(&val_ptr, v)?;
                l = minlen.clone();
            }
        }
        if !minlen.is_undef() && !fmt.eq_str("string") {
            // (must truncate strings later, after recoding)
            if !maxlen.truthy() {
                maxlen = minlen.clone();
            }
            if pl::lt(&l, &minlen) {
                if !et.get("OPTIONS").get("IgnoreMinorErrors").truthy() {
                    return ret(pl::s(&format!("[Minor] String too short (minlen is {})", minlen.to_text())));
                }
                et.set("CHECK_WARN", cat(&[pl::s("String too short for IPTC:"), tag_info.get("Name"), pl::s(" (written anyway)")]));
            } else if pl::gt(&l, &maxlen) && !et.get("OPTIONS").get("IgnoreMinorErrors").truthy() {
                et.set("CHECK_WARN", cat(&[pl::s("[Minor] IPTC:"), tag_info.get("Name"), pl::s(" exceeds length limit (truncated)")]));
                let v = substr(it, &val_ptr.deref(), &n(0), Some(&maxlen))?;
                pl::set_deref(&val_ptr, v)?;
            }
        }
    } else {
        return ret(pl::concat(&pl::concat(&pl::s("Bad IPTC Format ("), &format), &pl::s(")")));
    }
    pl::ret_undef()
}

/// The body of FormatIPTC, on references (as the Perl is).
fn format_iptc_v(it: &mut Interp, et: &Value, tag_info: &Value, val_ptr: &Value, xlat_ptr: &Value, rec: &Value, read: &Value) -> ER<()> {
    let mut format = tag_info.get("Format");
    if !format.truthy() {
        format = tag_info.get("Table").get("FORMAT");
    }
    if !format.truthy() {
        return Ok(());
    }
    if let Some(c) = re(r"^int(\d+)", "").m(&format) {
        if read.truthy() {
            let l = len(&val_ptr.deref());
            if pl::le(&l, &n(8)) {
                // limit integer conversion to 8 bytes long
                let mut val = n(0);
                let mut i = n(0);
                while pl::lt(&i, &l) {
                    let ch = substr(it, &val_ptr.deref(), &i, Some(&n(1)))?;
                    val = pl::add(&pl::mul(&val, &n(256)), &ord(&ch));
                    i = pl::add(&i, &n(1));
                }
                pl::set_deref(val_ptr, val)?;
            }
        } else {
            let c1 = cap(&c, 1);
            let l = pl::int(&pl::div(&if c1.truthy() { c1 } else { n(0) }, &n(8))?);
            if pl::num_eq(&l, &n(1)) {
                // 1 byte
                let v = crate::modules::write_exif::chr(&band(&val_ptr.deref(), 0xff));
                pl::set_deref(val_ptr, v)?;
            } else if pl::num_eq(&l, &n(2)) {
                // 2-byte integer
                let v = pl::pack("n", &[val_ptr.deref()])?;
                pl::set_deref(val_ptr, v)?;
            } else {
                // 4-byte integer
                let v = pl::pack("N", &[val_ptr.deref()])?;
                pl::set_deref(val_ptr, v)?;
            }
        }
    } else if re(r"^string", "").is_match(&format) {
        if pl::num_eq(rec, &n(1)) {
            if tag_info.get("Name").eq_str("CodedCharacterSet") {
                let x = pl::call(it, "Image::ExifTool::IPTC::HandleCodedCharset", vec![et.clone(), val_ptr.deref()])?;
                pl::set_deref(xlat_ptr, x)?;
            }
        } else if xlat_ptr.deref().truthy() && pl::lt(rec, &n(7)) && re(r"[\x80-\xff]", "").is_match(&val_ptr.deref()) {
            pl::call(it, "Image::ExifTool::IPTC::TranslateCodedString", vec![et.clone(), val_ptr.clone(), xlat_ptr.clone(), read.clone()])?;
        }
        // must check length now (after any string recoding)
        if !read.truthy() {
            if let Some(c) = re(r"^string\[(\d+),?(\d*)\]$", "").m(&format) {
                let (minlen, mut maxlen) = (cap(&c, 1), cap(&c, 2));
                let l = len(&val_ptr.deref());
                if !maxlen.truthy() {
                    maxlen = minlen.clone();
                }
                if pl::lt(&l, &minlen) {
                    let msg = cat(&[pl::s("String too short for IPTC:"), tag_info.get("Name"), pl::s(" (padded)")]);
                    if pl::method(it, et, "Warn", vec![msg, n(2)])?.truthy() {
                        let v = pl::concat(&val_ptr.deref(), &rep(b" ", &pl::sub(&minlen, &l)));
                        pl::set_deref(val_ptr, v)?;
                    }
                } else if pl::gt(&l, &maxlen) {
                    let msg = cat(&[pl::s("IPTC:"), tag_info.get("Name"), pl::s(" exceeds length limit (truncated)")]);
                    if pl::method(it, et, "Warn", vec![msg, n(2)])?.truthy() {
                        let v = substr(it, &val_ptr.deref(), &n(0), Some(&maxlen))?;
                        pl::set_deref(val_ptr, v)?;
                        // make sure UTF-8 is still valid
                        let x = xlat_ptr.deref();
                        let cs = if x.truthy() { x } else { opt(it, et, "Charset")? };
                        if cs.eq_str("UTF8") {
                            crate::runtime::require(it, "Image::ExifTool::XMP")?;
                            pl::call(it, "Image::ExifTool::XMP::FixUTF8", vec![val_ptr.clone(), pl::s(".")])?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// format IPTC data for writing
/// Inputs: 0) ExifTool object ref, 1) tagInfo pointer,
///         2) value reference (changed if necessary),
///         3) reference to character set for translation (changed if necessary)
///         4) record number, 5) flag set to read value (instead of write)
pub fn format_iptc(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    format_iptc_v(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2), &arg(&a, 3), &arg(&a, 4), &arg(&a, 5))?;
    pl::ret_nothing(want)
}

/// generate IPTC-format date
/// Inputs: 0) EXIF-format date string (YYYY:mm:dd) or date/time string
/// Returns: IPTC-format date string (YYYYmmdd), or undef and issue warning on error
pub fn iptc_date(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut val = arg(&a, 0);
    if re(r"^.*?(\d{4})[-:/.]?(\d{2})[-:/.]?(\d{2}).*", "s").subst(&mut val, false, |c| cat(&[cap(c, 1), cap(c, 2), cap(c, 3)]).to_bytes().to_vec()) == 0 {
        pl::warn(it, "Invalid date format (use YYYY:mm:dd)\n")?;
        val = Value::Undef;
    }
    ret(val)
}

/// generate IPTC-format time
/// Inputs: 0) EXIF-format time string (HH:MM:SS[+/-HH:MM]) or date/time string
/// Returns: IPTC-format time string (HHMMSS+HHMM), or undef and issue warning on error
pub fn iptc_time(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut val = arg(&a, 0);
    let m = re(r"(.*?)\b(\d{1,2})(:?)(\d{2})(:?)(\d{2})(\S*)\s*$", "s").m(&val).filter(|c| cap(c, 3).truthy() || !cap(c, 5).truthy());
    if let Some(c) = m {
        val = pl::sprintf("%.2d%.2d%.2d", &[cap(&c, 2), cap(&c, 4), cap(&c, 6)]);
        let (date, mut tz) = (cap(&c, 1), cap(&c, 7));
        if let Some(t) = re(r"([+-]\d{1,2}):?(\d{2})", "").m(&tz) {
            tz = pl::sprintf("%+.2d%.2d", &[cap(&t, 1), cap(&t, 2)]);
        } else if re(r"Z", "i").is_match(&tz) {
            tz = pl::s("+0000"); // UTC
        } else {
            // use local system timezone by default
            let (tm, time);
            let dm = if date.truthy() { re(r"^(\d{4}):(\d{2}):(\d{2})\s*$", "").m(&date) } else { None };
            let dm = match dm {
                Some(d) => {
                    if pl::eval_block(it, |it| crate::runtime::require(it, "Time::Local"))?.is_some() {
                        Some(d)
                    } else {
                        None
                    }
                }
                None => None,
            };
            if let Some(d) = dm {
                // we were given a date too, so determine the local timezone
                // offset at the specified date/time
                let dd = vec![cap(&d, 3), pl::sub(&cap(&d, 2), &n(1)), cap(&d, 1)];
                let t = re(r"(\d{2})(\d{2})(\d{2})", "").m(&val).unwrap_or_default();
                let mut v = vec![cap(&t, 3), cap(&t, 2), cap(&t, 1)];
                v.extend(dd);
                time = pl::call(it, "Image::ExifTool::TimeLocal", v.clone())?;
                tm = Value::new_array(v);
            } else {
                // it is difficult to get the proper local timezone offset for this
                // time because the date tag is written separately.  (The offset may be
                // different on a different date due to daylight savings time.)  In this
                // case the best we can do easily is to use the current timezone offset.
                let tc: Cell = cell(Value::Undef);
                time = perl_eval1(it, "time", MODULE, &[])?;
                put(&tc, time.clone());
                tm = perl_eval1(it, "[localtime($time)]", MODULE, &[("time", &tc)])?;
            }
            tz = pl::call(it, "Image::ExifTool::TimeZoneString", vec![tm, time])?;
            pl::tr(&mut tz, ":", "", "d");
        }
        val = pl::concat(&val, &tz);
    } else {
        pl::warn(it, "Invalid time format (use HH:MM:SS[+/-HH:MM])\n")?;
        val = Value::Undef; // time format error
    }
    ret(val)
}

/// Inverse print conversion for IPTC date or time value
/// Inputs: 0) ExifTool ref, 1) IPTC date or 'now'
/// Returns: IPTC date
pub fn inverse_date_or_time(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, val) = (arg(&a, 0), arg(&a, 1));
    if pl::lc(&val).eq_str("now") {
        return ret(pl::method(it, &et, "TimeNow", vec![])?);
    }
    ret(val)
}

/// Convert picture number
/// Inputs: 0) value
/// Returns: Converted value
pub fn convert_picture_number(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut val = arg(&a, 0);
    if seq(&val, &rep(b"\0", &n(16))) {
        val = pl::s("Unknown");
    } else if pl::ge(&len(&val), &n(16)) {
        let mut vals = pl::unpack("nNA8n", &val)?;
        vals.resize(4, Value::Undef);
        val = vals[0].clone();
        let manufacturer = wlex_hash(it, MODULE, FILE, "manufacturer");
        let manu = manufacturer.get(val.to_bytes());
        if manu.truthy() {
            val = cat(&[val, pl::s(" ("), manu, pl::s(")")]);
        }
        val = cat(&[val, pl::s(", equip "), vals[1].clone()]);
        re(r"(\d{4})(\d{2})(\d{2})", "").subst(&mut vals[2], false, |c| cat(&[cap(c, 1), pl::s(":"), cap(c, 2), pl::s(":"), cap(c, 3)]).to_bytes().to_vec());
        val = cat(&[val, pl::s(", "), vals[2].clone(), pl::s(", no. "), vals[3].clone()]);
    } else {
        val = pl::s("<format error>");
    }
    ret(val)
}

/// Inverse picture number conversion
/// Inputs: 0) value
/// Returns: Converted value (or undef on error)
pub fn inv_convert_picture_number(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut val = arg(&a, 0);
    re(r"\(.*\)", "").subst(&mut val, true, |_| Vec::new()); // remove manufacturer description
    pl::tr(&mut val, ":", "", "d"); // remove date separators
    pl::tr(&mut val, "0-9", " ", "c"); // turn remaining non-numbers to spaces
    let vals = pl::split_ws(&val);
    if vals.len() >= 4 {
        val = pl::pack("nNA8n", &vals)?;
    } else if re(r"unknown", "i").is_match(&val) {
        val = rep(b"\0", &n(16));
    } else {
        val = Value::Undef;
    }
    ret(val)
}

/// `$$h{$a}->{$b}` (the inner hash autovivified, as the arrow does)
fn hh(h: &Value, a: &Value) -> Value {
    h.get_or_new_hash(a.to_bytes())
}

/// Write IPTC data record
/// Inputs: 0) ExifTool object ref, 1) source dirInfo ref, 2) tag table ref
/// Returns: IPTC data block (may be empty if no IPTC data)
/// Notes: Increments ExifTool CHANGED flag for each tag changed
pub fn do_write_iptc(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    ret(do_write_iptc_v(it, &et, &dir_info, &tag_table_ptr)?)
}

fn do_write_iptc_v(it: &mut Interp, et: &Value, dir_info: &Value, tag_table_ptr: &Value) -> ER<Value> {
    let verbose = opt(it, et, "Verbose")?;
    let out = opt(it, et, "TextOut")?;
    let iptc_main = pl::pkg_hash(it, "Image::ExifTool::IPTC::Main");

    // avoid editing IPTC directory unless necessary:
    // - improves speed
    // - avoids changing current MD5 digest unnecessarily
    // - avoids adding mandatory tags unless some other IPTC is changed
    let edit_dirs = et.get("EDIT_DIRS");
    if !(edit_dirs.exists(dir_info.get("DirName").to_bytes()) ||
        // standard IPTC tags in other locations should be edited too (eg. AFCP_IPTC)
        (seq(tag_table_ptr, &iptc_main) && edit_dirs.exists("IPTC")))
    {
        return Ok(Value::Undef);
    }
    let mut data_pt = dir_info.get("DataPt");
    if !data_pt.truthy() {
        data_pt = pl::sref(pl::s(""));
    }
    let start = dir_info.get("DirStart");
    let mut start = if start.truthy() { start } else { n(0) };
    let mut dir_len = dir_info.get("DirLen");
    let mut tag_info;
    let iptc_info = Value::new_hash();

    // start by assuming default IPTC encoding
    let xlat = cell(opt(it, et, "CharsetIPTC")?);
    if seq(&get(&xlat), &opt(it, et, "Charset")?) {
        put(&xlat, Value::Undef);
    }

    // make sure our dataLen is defined (note: allow zero length directory)
    if dir_len.is_undef() {
        let mut data_len = dir_info.get("DataLen");
        if data_len.is_undef() {
            data_len = len(&data_pt.deref());
        }
        dir_len = pl::sub(&data_len, &start);
    }
    // quick check for improperly byte-swapped IPTC
    if pl::ge(&dir_len, &n(4)) && !substr(it, &data_pt.deref(), &start, Some(&n(1)))?.eq_str("\x1c") && substr(it, &data_pt.deref(), &pl::add(&start, &n(3)), Some(&n(1)))?.eq_str("\x1c") {
        pl::method(it, et, "Warn", vec![pl::s("IPTC data was improperly byte-swapped")])?;
        let d = pl::concat(&substr(it, &data_pt.deref(), &start, Some(&dir_len))?, &pl::s("\0\0\0"));
        let new_data = pl::pack("N*", &pl::unpack("V*", &d)?)?;
        data_pt = pl::sref(new_data);
        start = n(0);
        // NOTE: MUST NOT access $dirInfo DataPt, DirStart or DataLen after this!
    }
    // generate lookup so we can find the record numbers
    let record_num = Value::new_hash();
    for tag in pl::call_list(it, "Image::ExifTool::TagTableKeys", vec![tag_table_ptr.clone()])? {
        tag_info = tag_table_ptr.get(tag.to_bytes());
        if !tag_info.get("SubDirectory").truthy() {
            continue;
        }
        let table = tag_info.get("SubDirectory").get("TagTable");
        if !table.truthy() {
            continue;
        }
        let sub_table_ptr = etf(it, "GetTagTable", vec![table])?;
        record_num.set(sub_table_ptr.to_bytes(), tag);
    }

    // loop through new values and accumulate all IPTC information
    // into lists based on their IPTC record type
    for ti in pl::method_list(it, et, "GetNewTagInfoList", vec![])? {
        let table = ti.get("Table");
        let record = record_num.get(table.to_bytes());
        // ignore tags we aren't writing to this directory
        if record.is_undef() {
            continue;
        }
        if iptc_info.get(record.to_bytes()).is_undef() {
            iptc_info.set(record.to_bytes(), Value::new_array(vec![]));
        }
        iptc_info.get(record.to_bytes()).push(ti);
    }

    // get sorted list of records used.  Might as well be organized and
    // write our records in order of record number first, then tag number
    let mut record_list = keys(&iptc_info);
    sort_num(&mut record_list);
    let set = Value::new_hash();
    for record in &record_list {
        // sort tagInfo lists by tagID
        let l = iptc_info.get(record.to_bytes());
        let mut items = l.items();
        items.sort_by(|a, b| ops::num_cmp(&a.get("TagID"), &b.get("TagID")).unwrap_or(std::cmp::Ordering::Equal));
        if let Value::ArrayRef(r) = &l {
            *r.borrow_mut() = items.clone();
        }
        // build hash of all tagIDs to set
        for ti in items {
            hh(&set, record).set(ti.get("TagID").to_bytes(), ti);
        }
    }
    // run through the old IPTC data, inserting our records in
    // sequence and deleting existing records where necessary
    // (the IPTC specification states that records must occur in
    // numerical order, but tags within records need not be ordered)
    let mut pos = start.clone();
    let mut tail = pos.clone(); // old data written up to this point
    let dir_end = pl::add(&start, &dir_len);
    let mut new_data = pl::s("");
    let mut last_rec = n(-1);
    let mut last_rec_pos = n(0);
    let mut all_mandatory = n(0);
    let found_rec = Value::new_hash(); // found flags: 0x01-existed before, 0x02-deleted, 0x04-created
    let mut add_now = Value::Undef;
    let mandatory_h = wlex_hash(it, MODULE, FILE, "mandatory");
    tag_info = Value::Undef;

    'outer: loop {
        'body: {
            // get next IPTC record from input directory
            let (mut id, mut rec, mut tag, mut l, mut value_ptr) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef, Value::Undef);
            if pl::le(&pl::add(&pos, &n(5)), &dir_end) {
                let buff = substr(it, &data_pt.deref(), &pos, Some(&n(5)))?;
                let v = pl::unpack("CCCn", &buff)?;
                id = arg(&v, 0);
                rec = arg(&v, 1);
                tag = arg(&v, 2);
                l = arg(&v, 3);
                if pl::num_eq(&id, &n(0x1c)) {
                    if pl::lt(&rec, &last_rec) {
                        if pl::num_eq(&rec, &n(0)) {
                            if pl::method(it, et, "Warn", vec![pl::s("IPTC record 0 encountered, subsequent records ignored"), n(2)])?.truthy() {
                                return Ok(Value::Undef);
                            }
                            rec = Value::Undef;
                            pos = dir_end.clone();
                            l = n(0);
                        } else if pl::method(it, et, "Warn", vec![pl::s("IPTC doesn't conform to spec: Records out of sequence"), n(2)])?.truthy() {
                            return Ok(Value::Undef);
                        }
                    }
                    // handle extended IPTC entry if necessary
                    pos = pl::add(&pos, &n(5)); // step to after field header
                    if band(&l, 0x8000).truthy() {
                        let mut nn = band(&l, 0x7fff); // get num bytes in length field
                        if pl::le(&pl::add(&pos, &nn), &dir_end) && pl::le(&nn, &n(8)) {
                            // determine length (a big-endian, variable sized int)
                            l = n(0);
                            while nn.truthy() {
                                let ch = substr(it, &data_pt.deref(), &pos, Some(&n(1)))?;
                                l = pl::add(&pl::mul(&l, &n(256)), &ord(&ch));
                                pos = pl::add(&pos, &n(1));
                                nn = pl::sub(&nn, &n(1));
                            }
                        } else {
                            l = dir_end.clone(); // invalid length
                        }
                    }
                    value_ptr = pos.clone();
                    pos = pl::add(&pos, &l); // step $pos to next entry
                    // make sure we don't go past the end of data
                    // (this can only happen if original data is bad)
                    if pl::gt(&pos, &dir_end) {
                        pos = dir_end.clone();
                    }
                } else {
                    rec = Value::Undef;
                }
            }
            // write out all our records that come before this one
            let write_rec = rec.is_undef() || !pl::num_eq(&rec, &last_rec);
            if write_rec || add_now.truthy() {
                loop {
                    let new_rec = record_list.first().cloned().unwrap_or_default();
                    if add_now.truthy() {
                        tag_info = add_now.clone();
                    } else if new_rec.is_undef() || !pl::num_eq(&new_rec, &last_rec) {
                        // handle mandatory tags in last record unless it was empty
                        if pl::gt(&len(&new_data), &last_rec_pos) {
                            if pl::gt(&all_mandatory, &n(1)) {
                                // entire lastRec contained mandatory tags, and at least one tag
                                // was deleted, so delete entire record unless we specifically
                                // added a mandatory tag
                                let mut num = n(0);
                                let fr = hh(&found_rec, &last_rec);
                                for k in fr.keys() {
                                    let code = fr.get(&k);
                                    if band(&code, 0x04).truthy() {
                                        num = n(0);
                                        break;
                                    }
                                    if pl::num_eq(&band(&code, 0x03), &n(0x01)) {
                                        num = pl::add(&num, &n(1));
                                    }
                                }
                                if num.truthy() {
                                    new_data = substr(it, &new_data, &n(0), Some(&last_rec_pos))?;
                                    if pl::gt(&verbose, &n(1)) {
                                        print_out(it, &out, cat(&[pl::s("    - "), num, pl::s(" mandatory tags\n")]))?;
                                    }
                                }
                            } else if mandatory_h.get(last_rec.to_bytes()).truthy() && seq(tag_table_ptr, &iptc_main) {
                                // add required mandatory tags
                                let mandatory = mandatory_h.get(last_rec.to_bytes());
                                let mut sub_table_ptr = Value::Undef;
                                let mut mand_tags = keys(&mandatory);
                                sort_num(&mut mand_tags);
                                for mand_tag in mand_tags {
                                    if hh(&found_rec, &last_rec).get(mand_tag.to_bytes()).truthy() {
                                        continue;
                                    }
                                    if !sub_table_ptr.truthy() {
                                        tag_info = tag_table_ptr.get(last_rec.to_bytes());
                                        if !(tag_info.truthy() && tag_info.get("SubDirectory").truthy()) {
                                            pl::warn(it, "WriteIPTC: Internal error 1\n")?;
                                            continue;
                                        }
                                        if !tag_info.get("SubDirectory").get("TagTable").truthy() {
                                            continue;
                                        }
                                        sub_table_ptr = etf(it, "GetTagTable", vec![tag_info.get("SubDirectory").get("TagTable")])?;
                                    }
                                    tag_info = sub_table_ptr.get(mand_tag.to_bytes());
                                    if !tag_info.truthy() {
                                        pl::warn(it, "WriteIPTC: Internal error 2\n")?;
                                        continue;
                                    }
                                    let value = cell(mandatory.get(mand_tag.to_bytes()));
                                    pl::method(it, et, "VerboseValue", vec![pl::concat(&pl::s("+ IPTC:"), &tag_info.get("Name")), get(&value), pl::s(" (mandatory)")])?;
                                    // apply necessary format conversions
                                    format_iptc_v(it, et, &tag_info, &rf(&value), &rf(&xlat), &last_rec, &Value::Undef)?;
                                    l = len(&get(&value));
                                    // generate our new entry
                                    let entry = pl::pack("CCCn", &[n(0x1c), last_rec.clone(), mand_tag.clone(), len(&get(&value))])?;
                                    new_data = cat(&[new_data, entry, get(&value)]); // add entry to new IPTC data
                                    // (don't mark as changed if just mandatory tags changed)
                                    // ++$$et{CHANGED};
                                }
                            }
                        }
                        if new_rec.is_undef() {
                            break;
                        }
                        last_rec = new_rec.clone();
                        last_rec_pos = len(&new_data);
                        all_mandatory = n(1);
                    }
                    if !add_now.truthy() {
                        // compare current entry with entry next in line to write out
                        // (write out our tags in numerical order even though
                        // this isn't required by the IPTC spec)
                        if !rec.is_undef() && pl::le(&rec, &new_rec) {
                            break;
                        }
                        tag_info = iptc_info.get(new_rec.to_bytes()).at(0);
                    }
                    let new_tag = tag_info.get("TagID");
                    let nv_hash = pl::method(it, et, "GetNewValueHash", vec![tag_info.clone()])?;
                    // only add new values if...
                    let mut do_set = false;
                    let f = hh(&found_rec, &new_rec).get(new_tag.to_bytes());
                    let found = if f.truthy() { f } else { n(0) };
                    if band(&found, 0x02).truthy() {
                        // ...tag existed before and was deleted (unless we already added it)
                        if !band(&found, 0x04).truthy() {
                            do_set = true;
                        }
                    } else if tag_info.get("List").truthy() {
                        // ...tag is List and it existed before or we are creating it
                        if if found.truthy() { !nv_hash.get("CreateOnly").truthy() } else { nv_hash.get("IsCreating").truthy() } {
                            do_set = true;
                        }
                    } else {
                        // ...tag didn't exist before and we are creating it
                        if !found.truthy() && nv_hash.get("IsCreating").truthy() {
                            do_set = true;
                        }
                    }
                    if do_set {
                        let values = pl::method_list(it, et, "GetNewValue", vec![nv_hash.clone()])?;
                        if !values.is_empty() {
                            hh(&found_rec, &new_rec).set(new_tag.to_bytes(), ops::bit_or(&found, &n(0x04)));
                        }
                        // write tags for each value in list
                        for v in values {
                            let value = cell(v);
                            pl::method(it, et, "VerboseValue", vec![cat(&[pl::s("+ "), dir_info.get("DirName"), pl::s(":"), tag_info.get("Name")]), get(&value)])?;
                            // reset allMandatory flag if a non-mandatory tag is written
                            if all_mandatory.truthy() {
                                let mandatory = mandatory_h.get(new_rec.to_bytes());
                                if !(mandatory.truthy() && mandatory.get(new_tag.to_bytes()).truthy()) {
                                    all_mandatory = n(0);
                                }
                            }
                            // apply necessary format conversions
                            format_iptc_v(it, et, &tag_info, &rf(&value), &rf(&xlat), &new_rec, &Value::Undef)?;
                            // (note: IPTC string values are NOT null terminated)
                            l = len(&get(&value));
                            // generate our new entry
                            let mut entry = pl::pack("CCC", &[n(0x1c), new_rec.clone(), new_tag.clone()])?;
                            if pl::le(&l, &n(0x7fff)) {
                                entry = pl::concat(&entry, &pl::pack("n", &[l.clone()])?);
                            } else {
                                // extended dataset tag
                                entry = pl::concat(&entry, &pl::pack("nN", &[n(0x8004), l.clone()])?);
                            }
                            new_data = cat(&[new_data, entry, get(&value)]); // add entry to new IPTC data
                            inc(et, "CHANGED");
                        }
                    }
                    // continue on with regular programming if done adding tag now
                    if add_now.truthy() {
                        add_now = Value::Undef;
                        if write_rec {
                            continue;
                        }
                        break;
                    }
                    // remove this tagID from the sorted write list
                    let lst = iptc_info.get(new_rec.to_bytes());
                    if let Value::ArrayRef(r) = &lst {
                        let mut v = r.borrow_mut();
                        if !v.is_empty() {
                            v.remove(0);
                        }
                    }
                    if lst.is_empty() && !record_list.is_empty() {
                        record_list.remove(0);
                    }
                }
                if write_rec {
                    // all done if no more records to write
                    if rec.is_undef() {
                        break 'outer;
                    }
                    // update last record variables
                    last_rec = rec.clone();
                    last_rec_pos = len(&new_data);
                    all_mandatory = n(1);
                }
            }
            // set flag indicating we found this tag
            let fr = hh(&found_rec, &rec);
            let old = fr.get(tag.to_bytes());
            let nv = if old.truthy() { old } else { n(0) };
            fr.set(tag.to_bytes(), if nv.truthy() { nv } else { n(0x01) });
            // write out this record unless we are setting it with a new value
            tag_info = hh(&set, &rec).get(tag.to_bytes());
            if tag_info.truthy() {
                let nv_hash = pl::method(it, et, "GetNewValueHash", vec![tag_info.clone()])?;
                l = pl::sub(&pos, &value_ptr);
                let val = cell(substr(it, &data_pt.deref(), &value_ptr, Some(&l))?);
                // remove null terminator if it exists (written by braindead software like Picasa 2.0)
                if tag_info.get("Format").truthy() && re(r"^string", "").is_match(&tag_info.get("Format")) {
                    let mut v = get(&val);
                    if re(r"\0+$", "").subst(&mut v, false, |_| Vec::new()) > 0 {
                        put(&val, v);
                    }
                }
                let old_xlat = get(&xlat);
                format_iptc_v(it, et, &tag_info, &rf(&val), &rf(&xlat), &rec, &n(1))?;
                if pl::method(it, et, "IsOverwriting", vec![nv_hash.clone(), get(&val)])?.truthy() {
                    put(&xlat, old_xlat); // don't change translation (not writing this value)
                    pl::method(it, et, "VerboseValue", vec![cat(&[pl::s("- "), dir_info.get("DirName"), pl::s(":"), tag_info.get("Name")]), get(&val)])?;
                    inc(et, "CHANGED");
                    // set deleted flag to indicate we found and deleted this tag
                    let fr = hh(&found_rec, &rec);
                    fr.set(tag.to_bytes(), ops::bit_or(&fr.get(tag.to_bytes()), &n(0x02)));
                    // increment allMandatory flag to indicate a tag was removed
                    if all_mandatory.truthy() {
                        all_mandatory = pl::add(&all_mandatory, &n(1));
                    }
                    // write this tag now if overwriting an existing value
                    let nvv = nv_hash.get("Value");
                    if nvv.truthy() && !viv_items(&nv_hash, "Value").is_empty() && !record_list.is_empty() && pl::num_eq(&record_list[0], &rec) && !band(&hh(&found_rec, &rec).get(tag.to_bytes()), 0x04).truthy() {
                        add_now = tag_info.clone();
                    }
                    break 'body;
                }
            } else if pl::num_eq(&rec, &n(1)) && pl::num_eq(&tag, &n(90)) {
                // handle CodedCharacterSet tag
                let val = substr(it, &data_pt.deref(), &value_ptr, Some(&pl::sub(&pos, &value_ptr)))?;
                let x = pl::call(it, "Image::ExifTool::IPTC::HandleCodedCharset", vec![et.clone(), val])?;
                put(&xlat, x);
            }
            // reset allMandatory flag if a non-mandatory tag is written
            if all_mandatory.truthy() {
                let mandatory = mandatory_h.get(rec.to_bytes());
                if !(mandatory.truthy() && mandatory.get(tag.to_bytes()).truthy()) {
                    all_mandatory = n(0);
                }
            }
            // write out the record
            let seg = substr(it, &data_pt.deref(), &tail, Some(&pl::sub(&pos, &tail)))?;
            new_data = pl::concat(&new_data, &seg);
            let _ = (&id, &l);
        }
        tail = pos.clone();
    }
    // make sure the rest of the data is zero
    if pl::lt(&tail, &dir_end) {
        let pad = substr(it, &data_pt.deref(), &tail, Some(&pl::sub(&dir_end, &tail)))?;
        if re(r"[^\0]", "").is_match(&pad) && pl::method(it, et, "Warn", vec![pl::s("Unrecognized data in IPTC padding"), n(2)])?.truthy() {
            return Ok(Value::Undef);
        }
    }
    Ok(new_data)
}

/// `@{$$h{k}}` in a boolean/list rvalue context (no autovivification).
fn viv_items(h: &Value, k: &str) -> Vec<Value> {
    h.get(k).items()
}

/// Write IPTC data record and calculate NewIPTCDigest
/// Inputs: 0) ExifTool object ref, 1) source dirInfo ref, 2) tag table ref
/// Returns: IPTC data block (may be empty if no IPTC data)
/// Notes: Increments ExifTool CHANGED flag for each tag changed
pub fn write_iptc(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if !et.truthy() {
        return ret(n(1)); // allow dummy access to autoload this package
    }

    let new_data = cell(do_write_iptc_v(it, &et, &dir_info, &tag_table_ptr)?);

    // calculate standard IPTC digests only if we are writing or deleting
    // Photoshop:IPTCDigest with a value of 'new' or 'old'
    while it.get_scalar("Image::ExifTool::Photoshop::iptcDigestInfo").truthy() {
        let di = it.get_scalar("Image::ExifTool::Photoshop::iptcDigestInfo");
        let nv_hash = et.get("NEW_VALUE").get(di.to_bytes());
        if nv_hash.is_undef() {
            break;
        }
        let path = pl::method(it, &et, "MetadataPath", vec![])?;
        if !pl::call(it, "Image::ExifTool::IPTC::IsStandardIPTC", vec![path])?.truthy() {
            break;
        }
        let mut values = pl::method_list(it, &et, "GetNewValue", vec![nv_hash.clone()])?;
        if nv_hash.get("DelValue").truthy() {
            values.extend(nv_hash.get("DelValue").items());
        }
        let new = values.iter().filter(|v| re(r"^new$", "").is_match(v)).count();
        let old = values.iter().filter(|v| re(r"^old$", "").is_match(v)).count();
        if new == 0 && old == 0 {
            break;
        }
        if pl::eval_block(it, |it| crate::runtime::require(it, "Digest::MD5"))?.is_none() {
            pl::method(it, &et, "Warn", vec![pl::s("Digest::MD5 must be installed to calculate IPTC digest")])?;
            break;
        }
        let mut data_pt;
        if new > 0 {
            if !get(&new_data).is_undef() {
                data_pt = rf(&new_data);
            } else {
                data_pt = dir_info.get("DataPt");
                if dir_info.get("DirStart").truthy() || !pl::num_eq(&len(&data_pt.deref()), &dir_info.get("DirLen")) {
                    let b = substr(it, &data_pt.deref(), &dir_info.get("DirStart"), Some(&dir_info.get("DirLen")))?;
                    data_pt = pl::sref(b);
                }
            }
            // set NewIPTCDigest data member unless IPTC is being deleted
            if len(&data_pt.deref()).truthy() {
                let d = pl::call(it, "Digest::MD5::md5", vec![data_pt.deref()])?;
                et.set("NewIPTCDigest", d);
            }
        }
        if old > 0 {
            if new > 0 && get(&new_data).is_undef() {
                et.set("OldIPTCDigest", et.get("NewIPTCDigest"));
            } else if dir_info.get("DataPt").truthy() {
                //(may be undef if creating new IPTC)
                data_pt = dir_info.get("DataPt");
                if dir_info.get("DirStart").truthy() || !pl::num_eq(&len(&data_pt.deref()), &dir_info.get("DirLen")) {
                    let b = substr(it, &data_pt.deref(), &dir_info.get("DirStart"), Some(&dir_info.get("DirLen")))?;
                    data_pt = pl::sref(b);
                }
                if len(&data_pt.deref()).truthy() {
                    let d = pl::call(it, "Digest::MD5::md5", vec![data_pt.deref()])?;
                    et.set("OldIPTCDigest", d);
                }
            }
        }
        break;
    }
    // set changed if ForceWrite tag was set to "IPTC"
    if !get(&new_data).is_undef() && len(&get(&new_data)).truthy() && et.get("FORCE_WRITE").get("IPTC").truthy() {
        inc(&et, "CHANGED");
    }
    ret(get(&new_data))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::IPTC::PrintInvCodedCharset", print_inv_coded_charset);
    it.define_native("Image::ExifTool::IPTC::CheckIPTC", check_iptc);
    it.define_native("Image::ExifTool::IPTC::FormatIPTC", format_iptc);
    it.define_native("Image::ExifTool::IPTC::IptcDate", iptc_date);
    it.define_native("Image::ExifTool::IPTC::IptcTime", iptc_time);
    it.define_native("Image::ExifTool::IPTC::InverseDateOrTime", inverse_date_or_time);
    it.define_native("Image::ExifTool::IPTC::ConvertPictureNumber", convert_picture_number);
    it.define_native("Image::ExifTool::IPTC::InvConvertPictureNumber", inv_convert_picture_number);
    it.define_native("Image::ExifTool::IPTC::DoWriteIPTC", do_write_iptc);
    it.define_native("Image::ExifTool::IPTC::WriteIPTC", write_iptc);
}
