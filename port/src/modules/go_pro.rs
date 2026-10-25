//! Image::ExifTool::GoPro (Image/ExifTool/GoPro.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read information from GoPro videos: the GPMF records (gpmd samples, GPMF
//! box, APP6 segment) and their value conversions. The tag tables are data.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::exiftool::part2;
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, re, ret};
use crate::runtime;

const PKG: &str = "Image::ExifTool::GoPro";

// ---- local helpers ----

fn n(i: i64) -> Value {
    Value::Int(i)
}

fn s(x: &str) -> Value {
    pl::s(x)
}

/// `$obj->Method(ARGS)` in scalar context.
fn m(it: &mut Interp, obj: &Value, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::method(it, obj, name, args)
}

fn scalar(r: ER<Vec<Value>>) -> ER<Value> {
    Ok(r?.pop().unwrap_or_default())
}

fn cat(parts: &[&Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

/// A file lexical of GoPro.pm (`my %goProFmt`...).
fn lex_hash(it: &mut Interp, name: &str) -> Value {
    runtime::lex_hash(it, PKG, name)
}

/// `$raf->Read($buff, $len)`: the count, and `buff` updated.
fn raf_read(it: &mut Interp, raf: &Value, buff: &mut Value, len: Value) -> ER<Value> {
    let mut a = [buff.clone(), len];
    let r = scalar(pl::method_mut(it, raf, "Read", &mut a, Want::Scalar))?;
    *buff = a[0].clone();
    Ok(r)
}

// ---- the subs ----

/// Convert system time to date/time string
/// Inputs: 0) system time value, 1) ExifTool ref
/// Returns: EXIF-format date/time string with milliseconds
pub fn convert_system_time(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (mut val, et) = (arg(&a, 0), arg(&a, 1));
    let mut sl = et.get("SystemTimeList");
    if !sl.truthy() {
        return ret(s("<uncalibrated>"));
    }
    if !et.get("SystemTimeListSorted").truthy() {
        let mut v = sl.items();
        v.sort_by(|x, y| ops::num_cmp(&x.at(0), &y.at(0)).unwrap_or(std::cmp::Ordering::Equal));
        sl = Value::new_array(v);
        et.set("SystemTimeList", sl.clone());
        et.set("SystemTimeListSorted", n(1));
    }
    let (mut i, mut j) = (0i64, sl.len() as i64 - 1);
    // perform binary search to find this system time value
    while j - i > 1 {
        let t = (i + j) / 2;
        if pl::lt(&val, &sl.at(t).at(0)) {
            j = t;
        } else {
            i = t;
        }
    }
    if i == j || pl::num_eq(&sl.at(j).at(0), &sl.at(i).at(0)) {
        val = sl.at(i).at(1);
    } else {
        // interpolate between values
        let (si, sj) = (sl.at(i), sl.at(j));
        let num = pl::mul(&pl::sub(&sj.at(1), &si.at(1)), &pl::sub(&val, &si.at(0)));
        val = pl::add(&si.at(1), &pl::div(&num, &pl::sub(&sj.at(0), &si.at(0)))?);
    }
    // (a bit tricky to remove fractional seconds then add them back again after
    //  the date/time conversion while avoiding round-off errors which could
    //  put the seconds out by 1...)
    let sv = Value::bytes(val.to_bytes());
    let (t, f) = match re(r"^(\d+)(\.\d+)", "").m(&sv) {
        Some(c) => (cap(&c, 1), cap(&c, 2)),
        None => (Value::Undef, Value::Undef),
    };
    let utc = et.get("OPTIONS").get("QuickTimeUTC");
    let d = scalar(part2::convert_unix_time(it, vec![t, utc], Want::Scalar))?;
    ret(pl::concat(&d, &f))
}

/// Scale values by last 'SCAL' constants
/// Inputs: 0) value or list of values, 1) string of scale factors
/// Returns: nothing, but updates values
pub fn scale_values(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    if let Some(v) = scale_values_impl(&arg(&a, 0), &arg(&a, 1))? {
        it.set_arg(0, v)?;
    }
    pl::ret_nothing(want)
}

/// The new value of `$_[0]`, or None when ScaleValues returns early.
fn scale_values_impl(val: &Value, scl: &Value) -> ER<Option<Value>> {
    if !(val.truthy() && scl.truthy()) {
        return Ok(None);
    }
    let scl = pl::split_ws(scl);
    if scl.is_empty() {
        return Ok(None);
    }
    let mut scaled = Vec::new();
    let v = if pl::ref_of(val) == "ARRAY" { val.items() } else { vec![val.clone()] };
    for val in &v {
        let mut av = pl::split_ws(val);
        for k in 0..av.len() {
            av[k] = pl::div(&av[k], &scl[k % scl.len()])?;
        }
        scaled.push(pl::join(" ", &av));
    }
    Ok(Some(if scaled.len() > 1 { Value::new_array(scaled) } else { scaled.into_iter().next().unwrap_or_default() }))
}

/// Add units to values for human-readable output
/// Inputs: 0) ExifTool ref, 1) value, 2) tag key
/// Returns: converted value
pub fn add_units(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, mut val, tag) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    // ($$et{TAG_EXTRA}{$tag}{Units} autovivifies the intermediate hashes)
    if et.truthy() && et.get_or_new_hash("TAG_EXTRA").get_or_new_hash(tag.to_bytes()).get("Units").truthy() {
        let mut u = et.get("TAG_EXTRA").get(tag.to_bytes()).get("Units");
        if pl::ref_of(&u) != "ARRAY" {
            u = Value::new_array(vec![u]);
        }
        let mut av = pl::split_ws(&val);
        if u.len() == av.len() {
            for (i, x) in av.iter_mut().enumerate() {
                let ui = u.at(i as i64);
                if ui.truthy() {
                    *x = cat(&[x, &s(" "), &ui]);
                }
            }
            val = pl::join(" ", &av);
        }
    }
    ret(val)
}

/// Process string of values (or array of strings) to extract as separate tags
/// Inputs: 0) ExifTool object ref, 1) directory information ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_string(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let d = data_pt.deref();
    let list = if pl::ref_of(&d) == "ARRAY" { d.items() } else { vec![d] };
    m(it, &et, "VerboseDir", vec![s("GoPro structure")])?;
    let doc_num = et.get("DOC_NUM");
    let mut sub_doc = 0i64;
    for string in &list {
        let vals = pl::split_ws(string);
        let mut i = 0i64;
        for val in vals {
            m(it, &et, "HandleTag", vec![tag_table_ptr.clone(), n(i), val])?;
            i += 1;
            if tag_table_ptr.get(n(i).to_bytes()).truthy() {
                continue;
            }
            // increment subdoc for records stored as string of values (eg. GPS5)
            i = 0;
            sub_doc += 1;
            et.set("DOC_NUM", cat(&[&doc_num, &s("-"), &n(sub_doc)]));
        }
        if i != 0 {
            // increment subdoc for records stored as array of strings (eg. GPS9)
            sub_doc += 1;
            et.set("DOC_NUM", cat(&[&doc_num, &s("-"), &n(sub_doc)]));
        }
    }
    et.set("DOC_NUM", doc_num);
    ret(n(1))
}

/// Process "GP\x06\0" records in MP4 'mdat' atom
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref (RAF and DirLen)
/// Returns: size of GoPro record, or 0 on error
pub fn process_gp6(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let mut len = dir_info.get("DirLen");
    let mut buff = Value::Undef;
    while pl::gt(&len, &n(16)) {
        if !pl::num_eq(&raf_read(it, &raf, &mut buff, n(16))?, &n(16)) {
            break;
        }
        let ts = pl::unpack("a4N", &buff)?;
        let size = arg(&ts, 1);
        if pl::gt(&pl::add(&size, &n(16)), &len) || !re(r"^GP..\0", "s").is_match(&buff) {
            break;
        }
        if !pl::num_eq(&raf_read(it, &raf, &mut buff, size.clone())?, &size) {
            break;
        }
        if re(r"^DEVC", "").is_match(&buff) {
            let dc = pl::add(&et.get("DOC_COUNT"), &n(1));
            et.set("DOC_COUNT", dc.clone());
            et.set("DOC_NUM", dc);
            let tag_tbl = pl::call(it, "Image::ExifTool::GetTagTable", vec![s("Image::ExifTool::GoPro::GPMF")])?;
            let di = Value::new_hash();
            di.set("DataPt", pl::sref(buff.clone()));
            di.set("DataPos", pl::sub(&m(it, &raf, "Tell", vec![])?, &size));
            process_go_pro(it, vec![et.clone(), di, tag_tbl], Want::Scalar)?;
        }
        len = pl::sub(&len, &pl::add(&size, &n(16)));
    }
    et.delete("DOC_NUM");
    ret(pl::sub(&dir_info.get("DirLen"), &len))
}

/// Process GoPro metadata (gpmd samples, GPMF box, or APP6) (ref PH/1/2)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
/// - with hack to check for encrypted text in gpmd data (Rove Stealth 4K)
pub fn process_go_pro(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let data = data_pt.deref();
    let base = dir_info.get("Base");
    let ds = dir_info.get("DirStart");
    let mut pos = if ds.truthy() { ds } else { n(0) };
    let dl = dir_info.get("DirLen");
    let dir_end = pl::add(&pos, &if dl.truthy() { dl } else { pl::sub(&n(data.to_bytes().len() as i64), &pos) });
    let verbose = m(it, &et, "Options", vec![s("Verbose")])?;
    let unknown = if verbose.truthy() { verbose.clone() } else { m(it, &et, "Options", vec![s("Unknown")])? };
    let mut size = Value::Undef;
    let (mut typ, mut unit, mut scal, mut set_group0) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef);

    if verbose.truthy() {
        let dn = dir_info.get("DirName");
        m(it, &et, "VerboseDir", vec![if dn.truthy() { dn } else { s("GPMF") }, Value::Undef, pl::sub(&dir_end, &pos)])?;
    }
    et.set("FoundEmbedded", n(1));
    if pos.truthy() {
        let parent = dir_info.get("Parent");
        if parent.truthy() && parent.eq_str("APP6") {
            et.set("SET_GROUP0", s("APP6"));
            set_group0 = s("APP6");
        }
    } else if !et.get("SET_GROUP1").truthy() {
        // set group0 to "QuickTime" unless group1 is being changed (to Track#)
        et.set("SET_GROUP0", s("QuickTime"));
        set_group0 = s("QuickTime");
    }

    let go_pro_fmt = lex_hash(it, "goProFmt");
    let go_pro_size = lex_hash(it, "goProSize");
    let mut first = true;
    loop {
        if !first {
            // $pos+=($size+3)&0xfffffffc
            pos = pl::add(&pos, &ops::bit_and(&pl::add(&size, &n(3)), &n(0xfffffffc)));
        }
        first = false;
        if !pl::le(&pl::add(&pos, &n(8)), &dir_end) {
            break;
        }
        let u = pl::unpack(&format!("x{}a4CCn", pos.to_i64()), &data)?;
        let (tag, fmt, len, count) = (arg(&u, 0), arg(&u, 1), arg(&u, 2), arg(&u, 3));
        if re(r"[^-_a-zA-Z0-9 ]", "").is_match(&tag) {
            if !tag.eq_str("\0\0\0\0") {
                m(it, &et, "Warn", vec![s("Unrecognized GoPro record")])?;
            }
            break;
        }
        size = pl::mul(&len, &count);
        pos = pl::add(&pos, &n(8));
        if pl::gt(&pl::add(&pos, &size), &dir_end) {
            m(it, &et, "Warn", vec![s("Truncated GoPro record")])?;
            break;
        }
        let mut tag_info = m(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
        if tag.eq_str("\0\0\0\0") {
            break; // stop at null tag
        }
        if !(size.truthy() || verbose.truthy()) {
            continue; // don't save empty values unless verbose
        }
        let gf = go_pro_fmt.get(fmt.to_bytes());
        let format = if gf.truthy() { gf } else { s("undef") };
        let mut val;
        if pl::num_eq(&fmt, &n(0x3f)) && !typ.is_undef() {
            // decode structure with format given by previous 'TYPE'
            let mut v: Vec<Value> = Vec::new();
            let tp = pl::sref(typ.clone());
            let tlen = typ.to_bytes().len() as i64;
            let mut i = n(0);
            while pl::lt(&i, &count) {
                let mut sv: Vec<Value> = Vec::new();
                let mut l;
                let (mut j, mut p) = (0i64, n(0));
                while j < tlen {
                    let b = scalar(part2::get8u(it, vec![tp.clone(), n(j)], Want::Scalar))?;
                    let f = go_pro_fmt.get(b.to_bytes());
                    if !f.truthy() {
                        break;
                    }
                    let gs = go_pro_size.get(b.to_bytes());
                    l = if gs.truthy() { gs } else { scalar(part2::format_size(it, vec![f.clone()], Want::Scalar))? };
                    if !l.truthy() {
                        break;
                    }
                    if pl::gt(&pl::add(&p, &l), &len) {
                        break;
                    }
                    let off = pl::add(&pl::add(&pos, &pl::mul(&i, &len)), &p);
                    let sx = scalar(part2::read_value(it, vec![data_pt.clone(), off, f, Value::Undef, l.clone()], Want::Scalar))?;
                    if sx.is_undef() {
                        break;
                    }
                    sv.push(sx);
                    j += 1;
                    p = pl::add(&p, &l);
                }
                if !sv.is_empty() {
                    v.push(pl::join(" ", &sv));
                }
                i = pl::add(&i, &n(1));
            }
            val = if v.len() > 1 { Value::new_array(v) } else { v.into_iter().next().unwrap_or_default() };
        } else if (format.eq_str("undef") || format.eq_str("string")) && pl::gt(&count, &n(1)) && pl::gt(&len, &n(1)) {
            // unpack multiple undef/string values as a list
            let a = if format.eq_str("undef") { "a" } else { "A" };
            let t = format!("x{}{}", pos.to_i64(), format!("{a}{}", len.to_text()).repeat(count.to_i64().max(0) as usize));
            val = Value::new_array(pl::unpack(&t, &data)?);
        } else {
            val = scalar(part2::read_value(it, vec![data_pt.clone(), pos.clone(), format.clone(), Value::Undef, size.clone()], Want::Scalar))?;
        }
        // save TYPE, UNIT/SIUN and SCAL values for later
        if tag.eq_str("TYPE") {
            typ = val.clone();
        }
        if tag.eq_str("UNIT") || tag.eq_str("SIUN") {
            unit = val.clone();
        }
        if tag.eq_str("SCAL") {
            scal = val.clone();
        }

        if !tag_info.truthy() {
            if !unknown.truthy() {
                continue;
            }
            let name = pl::call(it, "Image::ExifTool::QuickTime::PrintableTagID", vec![tag.clone()])?;
            tag_info = Value::new_hash();
            tag_info.set("Name", cat(&[&s("GoPro_"), &name]));
            tag_info.set("Description", cat(&[&s("GoPro "), &name]));
            tag_info.set("Unknown", n(1));
            if !fmt.truthy() {
                let sd = Value::new_hash();
                sd.set("TagTable", s("Image::ExifTool::GoPro::GPMF"));
                tag_info.set("SubDirectory", sd);
            }
            pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), tag_info.clone()])?;
        }
        // apply scaling if available to last tag in this container
        if scal.truthy() && !tag.eq_str("SCAL") && pl::ge(&pl::add(&pl::add(&pos, &size), &n(3)), &dir_end) {
            if let Some(v) = scale_values_impl(&val, &scal)? {
                val = v;
            }
        }
        let extra = if verbose.truthy() {
            let c = if fmt.truthy() { Value::bytes([fmt.to_i64() as u8]) } else { s("\\0") };
            cat(&[&s(", type='"), &c, &s("' size="), &len, &s(" count="), &count])
        } else {
            Value::Undef
        };
        let key = m(
            it,
            &et,
            "HandleTag",
            vec![tag_table_ptr.clone(), tag.clone(), val, s("DataPt"), data_pt.clone(), s("DataPos"), dir_info.get("DataPos"), s("Base"), base.clone(), s("Start"), pos.clone(), s("Size"), size.clone(), s("TagInfo"), tag_info.clone(), s("Format"), format, s("Extra"), extra],
        )?;
        // save units for adding in print conversion if specified
        if tag_info.get("AddUnits").truthy() && key.truthy() {
            et.get_or_new_hash("TAG_EXTRA").get_or_new_hash(key.to_bytes()).set("Units", unit.clone());
        }
    }
    if set_group0.truthy() {
        et.delete("SET_GROUP0");
    }
    ret(n(1))
}

/// Register the ported subs under their Perl names.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::GoPro::ConvertSystemTime", convert_system_time);
    it.define_native("Image::ExifTool::GoPro::ScaleValues", scale_values);
    it.define_native("Image::ExifTool::GoPro::AddUnits", add_units);
    it.define_native("Image::ExifTool::GoPro::ProcessString", process_string);
    it.define_native("Image::ExifTool::GoPro::ProcessGP6", process_gp6);
    it.define_native("Image::ExifTool::GoPro::ProcessGoPro", process_go_pro);
}
